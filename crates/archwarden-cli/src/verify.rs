//! Proving that a rule bites.
//!
//! # What this answers, and what `explain` could not
//!
//! `config explain <rule-id>` says what a rule *reaches*: which files its scope
//! covers and what it is flagging today. A rule can be schema-valid, cover the
//! right paths, appear in `explain` and still enforce nothing, because its own
//! condition never fires on anything. Coverage is not efficacy, and the gap
//! between them is invisible from the outside — a rule enforcing nothing looks
//! exactly like a repository that satisfies it, which `CONFIG.md` calls the
//! worst failure a linter has.
//!
//! So: for each rule, synthesise an input that *should* violate it, evaluate
//! the rule against that input in memory, and say whether it fired. Nothing is
//! written to the repository, and nothing is read that `check` does not already
//! read.
//!
//! # What it does not prove, said plainly
//!
//! That a rule fires on a violation of **its own terms**. It cannot know what
//! you meant.
//!
//! Issue #24 is the sharp example. A `forbid_import_from_packages` list was
//! missing `@Dependencies`, real imports crossed the boundary, and the run was
//! green. Synthesising a violation from that rule's own list would have used
//! one of the packages it *does* name, and reported a confident tick. An
//! incomplete list is a question about intent, and no amount of evaluation
//! recovers intent from a config.
//!
//! What it does catch is the class where the rule's terms are self-defeating: a
//! scope that reaches nothing, an `except` that exempts everything it covers, a
//! pattern nothing can match, a rule shadowed into inertness by another. Those
//! all look active in `explain` and enforce nothing.
//!
//! The report says this in its own footer. A verification tool that oversold
//! itself would be the very thing it exists to prevent.
//!
//! # Probing at real paths
//!
//! The violating *edge* is synthesised; the paths are not. For each rule the
//! probe is placed at a directory or file this repository actually has and the
//! rule actually covers. Generating a path from a glob would mean writing a
//! second, worse implementation of what the scope already decides — and one
//! that disagreed with it would report a failure nobody could reproduce.
//!
//! The cost is stated rather than hidden: a rule whose scope reaches nothing in
//! this repository cannot be probed, and is reported as unverified with that as
//! the reason. `doctor` is the command that complains about it.

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRule, CompiledRuleKind},
    facts::{ExportFact, ExportKind, ExportTags, FileFacts, ImportFact, Span},
    hash::ContentHash,
    path::{FileClass, RepoRelPath},
    traits::{DirectoryContext, Exists, FileContext, RuleEngine},
};
use archwarden_engine::walk::RepoTree;

/// A name no repository is expected to contain, used for the synthesised
/// entry. Suffixed until it collides with nothing the rule would allow.
const PROBE: &str = "archwarden-probe";

/// What one rule's verification found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    /// The rule.
    pub rule_id: String,
    /// Its kind, as written in the config.
    pub kind: &'static str,
    /// What happened when it was handed a violation.
    pub verdict: Verdict,
}

/// The three answers, and the middle one is the reason this exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Handed a violation, the rule reported it.
    Fires {
        /// What the violation was, for a reader deciding whether it is the one
        /// they care about.
        on: String,
    },
    /// Handed a violation, the rule said nothing.
    ///
    /// The finding this command exists for.
    Silent {
        /// What it was handed and did not report.
        on: String,
    },
    /// No violation could be synthesised, and why.
    ///
    /// Never silence: a rule that went unchecked is reported as unchecked, the
    /// same way `check --file` names the rules it could not evaluate. A partial
    /// answer that says which part is missing is worth more than a confident
    /// one that is wrong.
    Unverified {
        /// The reason, as a sentence.
        why: String,
    },
}

impl Verdict {
    /// Whether this verdict should fail a build.
    #[must_use]
    pub fn is_silent(&self) -> bool {
        matches!(self, Self::Silent { .. })
    }
}

/// Verifies every rule in the configuration against the walked repository.
#[must_use]
pub fn verify(config: &CompiledConfig, tree: &RepoTree) -> Vec<Verification> {
    // Zipped with the engines the same way `doctor` does it, so the question
    // "does this rule cover this file?" is answered by the code `check` uses
    // rather than by a second implementation that could disagree.
    config
        .rules()
        .zip(archwarden_rules::engines_for(config))
        .map(|(rule, engine)| Verification {
            rule_id: rule.id.as_str().to_owned(),
            kind: rule.kind.type_name(),
            verdict: verdict_for(rule, engine.as_ref(), tree),
        })
        .collect()
}

fn verdict_for(rule: &CompiledRule, engine: &dyn RuleEngine, tree: &RepoTree) -> Verdict {
    match &rule.kind {
        CompiledRuleKind::Structure { .. } => forbidden_subfolder(rule, engine, tree),
        CompiledRuleKind::SpecPair { .. } => a_file_with_no_spec(rule, engine, tree),
        CompiledRuleKind::Presence { .. } => a_directory_holding_nothing(rule, engine, tree),
        CompiledRuleKind::Pair { .. } => a_file_with_no_companion(rule, engine, tree),
        CompiledRuleKind::Frontmatter { .. } => a_document_with_no_block(rule, engine, tree),
        // A rule that only forbids *reaching* has nothing a probe can plant.
        // Every other verdict here hands an engine one synthetic file; a chain
        // needs at least two, resolved against each other, which is the whole
        // pipeline run inside a probe. Checked before `crossed_boundary`,
        // which would otherwise explain it as "the rule only requires an
        // import" -- a sentence about a different rule.
        CompiledRuleKind::ImportBoundary {
            forbid,
            forbid_packages,
            forbid_reaching,
            ..
        } if forbid.is_empty() && forbid_packages.is_empty() && !forbid_reaching.is_empty() => {
            Verdict::Unverified {
                why: "the rule forbids reaching a path rather than importing \
                      one, and planting that means two files that resolve \
                      against each other -- the resolver run inside a probe"
                    .to_owned(),
            }
        }

        CompiledRuleKind::ImportBoundary {
            forbid,
            forbid_packages,
            except_from,
            ..
        } => crossed_boundary(rule, engine, tree, forbid, forbid_packages, except_from),

        // A violation here is a *file name*, and producing one means running a
        // regex backwards. `naming` and `call-obligation` both hold a
        // `file_pattern` whose language is what a violating name would have to
        // come from, and inventing a string that matches an arbitrary regex is
        // a generator this does not have.
        CompiledRuleKind::Naming { .. } | CompiledRuleKind::CallObligation { .. } => {
            Verdict::Unverified {
                why: "a violation means inventing a filename that matches this rule's \
                      `file_pattern`, which is a regex run backwards"
                    .to_owned(),
            }
        }

        // Planting a violation means two files that import each other and a
        // resolver that places both, which is the whole `check` pipeline run
        // inside a probe. Every other probe here hands an engine synthetic
        // facts and reads the verdict; this one would have to build a
        // repository on disk to get the edges, and a probe that heavy is a
        // second implementation of the thing it is checking.
        CompiledRuleKind::ImportCycle { .. } => Verdict::Unverified {
            why: "planting a cycle means writing two files that import each \
                  other and resolving both, which is the resolver run inside a \
                  probe"
                .to_owned(),
        },

        // Synthesising a passthrough file is possible and the shapes are
        // configurable -- `reexport`, `alias`, `wrapper`, partial forms, the
        // `package.json` entrypoint exemption. A probe that covered one form
        // would tick for a rule configured for another, which is a confident
        // answer about the wrong question.
        CompiledRuleKind::NoPassthrough { .. } => Verdict::Unverified {
            why: "which shape of forwarding counts is configurable, and a probe \
                  of one shape would tick for a rule about another"
                .to_owned(),
        },
    }
}

/// A document this rule covers, handed facts saying it has no block.
///
/// Absence is easy to synthesise, and this rule's own documentation says a
/// document with no frontmatter must be a finding rather than a skip -- so the
/// probe is the exact case the rule promises to catch.
fn a_document_with_no_block(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
) -> Verdict {
    let Some(covered) = tree
        .directories()
        .flat_map(|(_, directory)| directory.files.iter())
        .map(|file| &file.path)
        .find(|path| engine.applies_to(path))
    else {
        return Verdict::Unverified {
            why: format!(
                "no document in this repository is one `{}` is about",
                rule.id
            ),
        };
    };

    let docs = archwarden_core::docs::DocFacts {
        path: covered.clone(),
        content_hash: ContentHash::of(PROBE.as_bytes()),
        frontmatter: archwarden_core::docs::Frontmatter::Absent,
        headings: Vec::new(),
    };

    let findings = engine.check_file(FileContext {
        path: covered,
        facts: None,
        docs: Some(&docs),
        siblings: &[],
        exists: Exists::none(),
        graph: None,
    });

    let on = format!("`{covered}` with no frontmatter block");
    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}

/// A file this rule covers, in a repository holding nothing else.
///
/// The probe is a real file -- one the rule says it applies to -- asked about
/// against an empty repository. Nothing has to be invented, unlike `naming`,
/// where a violating input is a filename and producing one means running a
/// regex backwards; here the violating input is the *absence* of the
/// companion, and absence is easy to synthesise.
fn a_file_with_no_companion(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
) -> Verdict {
    let Some(covered) = tree
        .directories()
        .flat_map(|(_, directory)| directory.files.iter())
        .map(|file| &file.path)
        .find(|path| engine.applies_to(path))
    else {
        return Verdict::Unverified {
            why: format!(
                "no file in this repository is one `{}` asks for a companion of",
                rule.id
            ),
        };
    };

    let findings = engine.check_file(FileContext {
        path: covered,
        facts: None,
        docs: None,
        siblings: &[],
        exists: Exists::none(),
        graph: None,
    });

    let on = format!("`{covered}` with its companion missing");
    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}

/// A directory this rule covers, emptied.
///
/// The cleanest synthesis of the six: a rule that asks for files is violated
/// by a directory with none, and nothing has to be invented -- unlike `naming`,
/// where a violating input is a filename and producing one means running a
/// regex backwards.
fn a_directory_holding_nothing(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
) -> Verdict {
    let Some(directory) = a_directory_in_scope(rule, tree) else {
        return Verdict::Unverified {
            why: format!(
                "no directory in this repository is inside `{}`",
                rule.scope.patterns().join("`, `")
            ),
        };
    };

    let findings = engine.check_directory(DirectoryContext {
        path: directory,
        subdirectories: &[],
        files: &[],
    });

    let on = format!("`{directory}` holding none of the files it requires");
    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}

/// A directory this rule covers, offered a violation of each axis it
/// constrains.
///
/// A `structure` rule constrains two independent things: which subfolders a
/// directory may hold, and what its files may be called. It may constrain
/// either, both, or — the case the command exists to catch — neither.
///
/// Probing only the subfolder axis reported every filename-only rule as
/// enforcing nothing, which is a false negative on the one line a reader acts
/// on: *"5 enforce nothing"* invites deleting five rules that work. So each
/// axis the rule actually constrains gets a probe, and the rule is verified if
/// any of them fires. Only a rule that constrains neither is silent, and that
/// one really does enforce nothing.
fn forbidden_subfolder(rule: &CompiledRule, engine: &dyn RuleEngine, tree: &RepoTree) -> Verdict {
    let Some(directory) = a_directory_in_scope(rule, tree) else {
        return Verdict::Unverified {
            why: format!(
                "no directory in this repository is inside `{}`",
                rule.scope.patterns().join("`, `")
            ),
        };
    };

    let mut attempted = Vec::new();

    if constrains_subfolders(&rule.kind) {
        let probe = unclaimed_name(&rule.kind);
        let on = format!("an unlisted `{probe}/` folder in `{directory}`");
        let findings = engine.check_directory(DirectoryContext {
            path: directory,
            subdirectories: std::slice::from_ref(&probe),
            files: &[],
        });
        if !findings.is_empty() {
            return Verdict::Fires { on };
        }
        attempted.push(on);
    }

    if constrains_filenames(&rule.kind) {
        // A name no `filename_patterns` regex in this repository accepts: the
        // probe marker, with capitals and an extension the patterns are written
        // against. `unclaimed_filename` checks it rather than assuming.
        let probe = unclaimed_filename(&rule.kind);
        let on = format!("a file named `{probe}` in `{directory}`");
        let findings = engine.check_directory(DirectoryContext {
            path: directory,
            subdirectories: &[],
            files: std::slice::from_ref(&probe),
        });
        if !findings.is_empty() {
            return Verdict::Fires { on };
        }
        attempted.push(on);
    }

    // Neither axis is constrained: the rule asks nothing of the directories it
    // covers, which is exactly the state this command was written to name.
    if attempted.is_empty() {
        return Verdict::Silent {
            on: format!("`{directory}`, which it constrains in no way at all"),
        };
    }

    Verdict::Silent {
        on: attempted.join(", and "),
    }
}

/// Whether the rule says anything about which subfolders may be there.
fn constrains_subfolders(kind: &CompiledRuleKind) -> bool {
    matches!(
        kind,
        CompiledRuleKind::Structure {
            allowed_subfolders: Some(_),
            ..
        }
    ) || matches!(
        kind,
        CompiledRuleKind::Structure {
            subfolder_patterns,
            ..
        } if !subfolder_patterns.is_empty()
    )
}

/// Whether the rule says anything about what the files may be called.
fn constrains_filenames(kind: &CompiledRuleKind) -> bool {
    matches!(
        kind,
        CompiledRuleKind::Structure {
            filename_patterns,
            ..
        } if !filename_patterns.is_empty()
    )
}

/// A filename none of the rule's patterns accept.
///
/// Tried rather than assumed: a rule whose pattern happens to accept the probe
/// would be reported silent for a name it was right to allow, which is the
/// same false negative one layer down.
fn unclaimed_filename(kind: &CompiledRuleKind) -> String {
    let CompiledRuleKind::Structure {
        filename_patterns, ..
    } = kind
    else {
        return PROBE.to_owned();
    };

    // Capitals and an unlikely extension, because the patterns these rules
    // carry are overwhelmingly lower-case-with-dashes over a known suffix.
    for candidate in [
        format!("{PROBE}-INVALID-Name.probe"),
        format!("{PROBE}-INVALID-Name"),
        format!("__{PROBE}__"),
    ] {
        if !filename_patterns
            .iter()
            .any(|pattern| pattern.is_match(&candidate))
        {
            return candidate;
        }
    }

    format!("{PROBE}-INVALID-Name.probe")
}

/// A source file this rule covers, with no spec beside it.
///
/// The issue expected this one to be impossible -- "the violation is the
/// *absence* of a file, which cannot be synthesised as a file at all". It can.
/// The rule is offered one file at a time together with what else is in the
/// folder, so a file whose only sibling is itself *is* the absence, and no
/// spec has to exist anywhere for the rule to be asked about it.
///
/// The probe carries a function export because `skip_type_only` exempts a file
/// with nothing at runtime to test, and a probe that tripped over that
/// exemption would report a working rule as silent.
fn a_file_with_no_spec(rule: &CompiledRule, engine: &dyn RuleEngine, tree: &RepoTree) -> Verdict {
    let Some(directory) = a_directory_in_scope(rule, tree) else {
        return Verdict::Unverified {
            why: format!(
                "no directory in this repository is inside `{}`",
                rule.scope.patterns().join("`, `")
            ),
        };
    };

    let name = format!("{PROBE}.ts");
    let Ok(lonely) = directory.join(&name) else {
        return Verdict::Unverified {
            why: format!("`{directory}` cannot hold a probe file"),
        };
    };

    if !engine.applies_to(&lonely) {
        return Verdict::Unverified {
            why: format!(
                "the rule covers `{directory}` but not a file directly in it, so \
                 the probe has nowhere to sit"
            ),
        };
    }

    let mut facts = FileFacts::unparsed(lonely.clone(), ContentHash::of(PROBE.as_bytes()));
    facts.exports.push(ExportFact {
        name: Some("Probe".to_owned()),
        tags: ExportTags::only(ExportKind::Function),
        is_default: false,
        reexport_from: None,
        forwards: None,
        annotations: Vec::new(),
        span: Span::new(0, 1),
    });

    let findings = engine.check_file(FileContext {
        path: &lonely,
        facts: Some(&facts),
        docs: None,
        siblings: std::slice::from_ref(&name),
        exists: Exists::none(),
        graph: None,
    });

    let on = format!("`{lonely}` with no spec beside it");
    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}

/// A file this rule covers, importing something the rule forbids.
fn crossed_boundary(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
    forbid: &archwarden_core::glob::PathSet,
    forbid_packages: &[String],
    except_from: &archwarden_core::glob::PathSet,
) -> Verdict {
    let Some(importer) = a_file_in_scope(rule, engine, tree, except_from) else {
        return Verdict::Unverified {
            why: format!(
                "no source file in this repository is inside `{}` without being \
                 exempted by `except_from`",
                rule.scope.patterns().join("`, `")
            ),
        };
    };

    // The forbidden path half first: it is the half that needs resolution, and
    // so the half most likely to be enforcing nothing.
    let (import, on) = if forbid.is_empty() {
        let Some(package) = forbid_packages.first() else {
            return Verdict::Unverified {
                why: "the rule only requires an import, and a file that imports \
                      nothing is not a violation this can tell apart from a file \
                      the rule does not cover"
                    .to_owned(),
            };
        };
        (
            probe_import(package.clone(), None),
            format!("`{importer}` importing the package `{package}`"),
        )
    } else {
        let Some(target) = a_file_matching(forbid, tree) else {
            return Verdict::Unverified {
                why: format!(
                    "no file in this repository matches `{}`, so there is nothing \
                     for a probe to import",
                    forbid.patterns().join("`, `")
                ),
            };
        };
        (
            probe_import(format!("./{}", target.as_str()), Some(target.clone())),
            format!("`{importer}` importing `{target}`"),
        )
    };

    let mut facts = FileFacts::unparsed(importer.clone(), ContentHash::of(PROBE.as_bytes()));
    facts.imports.push(import);

    let findings = engine.check_file(FileContext {
        path: importer,
        facts: Some(&facts),
        docs: None,
        siblings: &[],
        exists: Exists::none(),
        graph: None,
    });

    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}

/// An import as the parser would have recorded it, already resolved.
///
/// `type_only` is false: a rule with `include_type_only: false` still catches a
/// value import, so this is the probe that asks the least of the rule.
fn probe_import(specifier: String, resolved: Option<RepoRelPath>) -> ImportFact {
    ImportFact {
        specifier,
        resolved,
        type_only: false,
        names: Vec::new(),
        span: Span::new(0, 1),
    }
}

/// A directory this rule's scope covers.
fn a_directory_in_scope<'a>(rule: &CompiledRule, tree: &'a RepoTree) -> Option<&'a RepoRelPath> {
    tree.directories()
        .map(|(path, _)| path)
        .find(|path| rule.scope.matches_dir(path.as_path()))
}

/// A source file this rule applies to and does not exempt.
fn a_file_in_scope<'a>(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &'a RepoTree,
    except_from: &archwarden_core::glob::PathSet,
) -> Option<&'a RepoRelPath> {
    let _ = rule;
    tree.files()
        .filter(|file| file.class == FileClass::Source)
        .map(|file| &file.path)
        .find(|path| engine.applies_to(path) && !except_from.is_match(path.as_path()))
}

/// A file in this repository that the given globs match.
fn a_file_matching<'a>(
    forbid: &archwarden_core::glob::PathSet,
    tree: &'a RepoTree,
) -> Option<&'a RepoRelPath> {
    tree.files()
        .filter(|file| file.class == FileClass::Source)
        .map(|file| &file.path)
        .find(|path| forbid.is_match(path.as_path()))
}

/// A folder name this rule does not already permit.
///
/// A `structure` rule that happened to allow a folder called
/// `archwarden-probe` would be handed something legal and reported as silent,
/// which is a false accusation in the one command whose job is not to make
/// them.
fn unclaimed_name(kind: &CompiledRuleKind) -> String {
    let CompiledRuleKind::Structure {
        allowed_subfolders,
        warn_subfolders,
        recurse_into,
        ..
    } = kind
    else {
        return PROBE.to_owned();
    };

    let claimed = |name: &str| {
        allowed_subfolders
            .iter()
            .flatten()
            .any(|other| other == name)
            || warn_subfolders.iter().any(|other| other == name)
            || recurse_into.iter().any(|other| other == name)
    };

    let mut name = PROBE.to_owned();
    let mut suffix = 2;
    while claimed(&name) {
        name = format!("{PROBE}-{suffix}");
        suffix += 1;
    }
    name
}

/// One rule's verdict, as JSON.
#[derive(Debug, serde::Serialize)]
struct JsonVerification<'a> {
    rule_id: &'a str,
    kind: &'a str,
    /// `fires`, `silent` or `unverified`: a stable slug a CI job can branch on.
    verdict: &'static str,
    /// What the rule was handed, when there was something to hand it.
    #[serde(skip_serializing_if = "Option::is_none")]
    probe: Option<&'a str>,
    /// Why nothing could be handed to it, when that is the answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
}

/// Writes the verifications in the requested format.
pub fn render(
    verifications: &[Verification],
    format: crate::report::Format,
    out: &mut dyn std::io::Write,
) {
    match format {
        crate::report::Format::Json => {
            let envelope: Vec<JsonVerification<'_>> = verifications
                .iter()
                .map(|verification| {
                    let (verdict, probe, reason) = match &verification.verdict {
                        Verdict::Fires { on } => ("fires", Some(on.as_str()), None),
                        Verdict::Silent { on } => ("silent", Some(on.as_str()), None),
                        Verdict::Unverified { why } => ("unverified", None, Some(why.as_str())),
                    };
                    JsonVerification {
                        rule_id: &verification.rule_id,
                        kind: verification.kind,
                        verdict,
                        probe,
                        reason,
                    }
                })
                .collect();
            match serde_json::to_string_pretty(&envelope) {
                Ok(json) => {
                    let _ = writeln!(out, "{json}");
                }
                Err(error) => {
                    let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
                }
            }
        }
        crate::report::Format::Text => render_text(verifications, out),
    }
}

fn render_text(verifications: &[Verification], out: &mut dyn std::io::Write) {
    for verification in verifications {
        match &verification.verdict {
            Verdict::Fires { on } => {
                let _ = writeln!(out, "✓ {} — fires on {on}", verification.rule_id);
            }
            Verdict::Silent { on } => {
                let _ = writeln!(out, "✗ {} — silent on {on}", verification.rule_id);
            }
            Verdict::Unverified { why } => {
                let _ = writeln!(out, "? {} — not verified: {why}", verification.rule_id);
            }
        }
    }

    let silent = verifications
        .iter()
        .filter(|verification| verification.verdict.is_silent())
        .count();
    let unverified = verifications
        .iter()
        .filter(|verification| matches!(verification.verdict, Verdict::Unverified { .. }))
        .count();
    let fires = verifications.len() - silent - unverified;

    let _ = writeln!(
        out,
        "\n{fires} enforce something, {silent} enforce nothing, {unverified} not verified"
    );

    // Said on every run, including the clean one. The command's whole value is
    // that it does not overstate what it checked -- an agent reading a wall of
    // ticks and concluding its config is sound would be back in the state the
    // issue described, one level up.
    let _ = writeln!(
        out,
        "\nThis proves each rule fires on a violation of its own terms. It cannot\n\
         know what you meant: a list missing an entry is a question about intent,\n\
         and a rule with a hole in it ticks here."
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::SkipDirs, glob::PathSet, ids::RuleId, level::Level, pattern::Pattern,
        scope::Scope,
    };
    use camino::Utf8PathBuf;

    fn tree_at(entries: &[&str]) -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");
        for relative in entries {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().expect("a file has a parent"))
                .expect("create dirs");
            std::fs::write(&path, "export const x = 1;").expect("write file");
        }
        (dir, root)
    }

    fn config_of(rules: Vec<CompiledRule>) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b"verify"),
        )
    }

    fn rule(id: &str, scope: &[&str], kind: CompiledRuleKind) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(scope.iter().copied()).expect("valid scope"),
            kind,
        }
    }

    fn boundary(forbid: &[&str], packages: &[&str], except_from: &[&str]) -> CompiledRuleKind {
        CompiledRuleKind::ImportBoundary {
            forbid: PathSet::compile(forbid.iter().map(|g| (*g).to_owned())).expect("valid globs"),
            groups: Vec::new(),
            allow: None,
            allow_packages: None,
            require: PathSet::default(),
            forbid_packages: packages.iter().map(|p| (*p).to_owned()).collect(),
            forbid_reaching: PathSet::default(),
            except: PathSet::default(),
            except_from: PathSet::compile(except_from.iter().map(|g| (*g).to_owned()))
                .expect("valid globs"),
            include_type_only: true,
        }
    }

    fn verdict(entries: &[&str], rules: Vec<CompiledRule>) -> Verdict {
        let (guard, root) = tree_at(entries);
        let config = config_of(rules);
        let tree = archwarden_engine::walk::walk(&root, &config).expect("walks");
        let mut verifications = verify(&config, &tree);
        drop(guard);
        verifications.pop().expect("one rule, one verdict").verdict
    }

    /// The rule the issue's author proved by hand, by planting a file and
    /// deleting it: a relative escape out of a package into an app.
    #[test]
    fn a_boundary_that_bites_is_reported_as_firing() {
        let verdict = verdict(
            &["packages/domain/order.ts", "apps/api/src/env.ts"],
            vec![rule(
                "domain-is-self-contained",
                &["packages/domain/**"],
                boundary(&["apps/**"], &[], &[]),
            )],
        );

        assert!(
            matches!(verdict, Verdict::Fires { .. }),
            "it should catch an import of `apps/**`: {verdict:?}"
        );
    }

    /// And the finding this command exists for: a rule that covers the right
    /// files, appears in `explain`, and enforces nothing -- here because
    /// `except_from` exempts everything its scope reaches.
    #[test]
    fn a_boundary_exempted_into_inertness_is_reported_as_silent() {
        let verdict = verdict(
            &["packages/domain/order.ts", "apps/api/src/env.ts"],
            vec![rule(
                "domain-is-self-contained",
                &["packages/domain/**"],
                boundary(&["apps/**"], &[], &["packages/domain/**"]),
            )],
        );

        assert!(
            matches!(verdict, Verdict::Unverified { .. }),
            "every file it covers is exempt, so there is nothing to probe with: {verdict:?}"
        );
    }

    /// A rule that only forbids *reaching* cannot be probed, and says so in
    /// those words.
    ///
    /// It fell through to the "only requires an import" branch before, which
    /// is a true-sounding sentence about a rule that does not require an
    /// import at all — and a wrong explanation of an `unverified` is worse
    /// than a vague one, because a reader acts on it.
    #[test]
    fn a_boundary_that_only_forbids_reaching_says_why_it_cannot_be_probed() {
        let mut kind = boundary(&[], &[], &[]);
        let CompiledRuleKind::ImportBoundary {
            forbid_reaching: slot,
            ..
        } = &mut kind
        else {
            panic!("built as an import-boundary rule");
        };
        *slot = PathSet::compile(["packages/db/**".to_owned()]).expect("valid globs");

        let verdict = verdict(
            &["packages/ui/button.tsx", "packages/db/client.ts"],
            vec![rule("ui-must-not-reach-db", &["packages/ui/**"], kind)],
        );

        let Verdict::Unverified { why } = &verdict else {
            panic!("a chain cannot be planted by a probe: {verdict:?}");
        };
        assert!(
            why.contains("reach"),
            "the reason has to name the half that could not be probed: {why}"
        );
    }

    /// A rule that forbids *both* is still probed for the half a probe can
    /// reach.
    ///
    /// The refusal above is for a rule with nothing else to test. A rule that
    /// also forbids a direct import has a verifiable half, and reporting the
    /// whole rule as `unverified` would hide a `forbid_import_from` that
    /// enforces nothing — which is the finding this command exists for.
    #[test]
    fn a_boundary_that_forbids_both_is_still_probed_for_the_direct_half() {
        let mut kind = boundary(&["apps/**"], &[], &[]);
        let CompiledRuleKind::ImportBoundary {
            forbid_reaching: slot,
            ..
        } = &mut kind
        else {
            panic!("built as an import-boundary rule");
        };
        *slot = PathSet::compile(["packages/db/**".to_owned()]).expect("valid globs");

        let verdict = verdict(
            &["packages/domain/order.ts", "apps/api/src/env.ts"],
            vec![rule(
                "domain-is-self-contained",
                &["packages/domain/**"],
                kind,
            )],
        );

        assert!(
            matches!(verdict, Verdict::Fires { .. }),
            "the direct half is probeable and must still be probed: {verdict:?}"
        );
    }

    /// A `structure` rule is handed a folder it does not allow.
    #[test]
    fn a_structure_rule_is_probed_with_a_folder_it_forbids() {
        let verdict = verdict(
            &["src/order/types/x.ts"],
            vec![rule(
                "entity-shape",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec!["types".to_owned()]),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )],
        );

        assert!(matches!(verdict, Verdict::Fires { .. }), "{verdict:?}");
    }

    /// A `structure` rule that constrains filenames is probed with a filename.
    ///
    /// Issue #49: every `structure` rule was probed by offering it an unlisted
    /// folder. A rule that says nothing about subfolders is correctly silent on
    /// that, and was reported as enforcing nothing — five of fourteen rules in
    /// one repository, all five of which fire on the axis they actually
    /// constrain.
    ///
    /// Worse than a wrong tick, because of what the command is for. `#24` asked
    /// for it precisely because `explain` shows coverage and not efficacy, so
    /// *"5 enforce nothing"* is the line somebody acts on — and acting on it
    /// here means deleting five rules that work. A verifier that reports a
    /// false negative is worse than no verifier, for the reason the docs give
    /// about silent rules: it is indistinguishable from the real thing.
    #[test]
    fn a_structure_rule_that_only_constrains_filenames_is_probed_with_a_filename() {
        let verdict = verdict(
            &["scripts/build.ts"],
            vec![rule(
                "scripts-kebab-case",
                &["scripts"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: None,
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: vec![Pattern::compile(r"^[a-z0-9-]+\.ts$").expect("valid")],
                },
            )],
        );

        assert!(
            matches!(verdict, Verdict::Fires { .. }),
            "it refuses `NomeErrado.ts` and should be probed with one: {verdict:?}"
        );
    }

    /// A rule constraining both axes is verified if either one fires.
    #[test]
    fn a_structure_rule_constraining_both_axes_is_probed_on_both() {
        let verdict = verdict(
            &["src/order/types/x.ts"],
            vec![rule(
                "entity-shape",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec!["types".to_owned()]),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: vec![Pattern::compile(r"^[a-z0-9-]+\.ts$").expect("valid")],
                },
            )],
        );

        assert!(matches!(verdict, Verdict::Fires { .. }), "{verdict:?}");
    }

    /// And a `structure` rule that constrains neither axis really does enforce
    /// nothing, which is the answer the command exists to give.
    #[test]
    fn a_structure_rule_constraining_nothing_is_still_reported_silent() {
        let verdict = verdict(
            &["src/order/x.ts"],
            vec![rule(
                "says-nothing",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: None,
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )],
        );

        assert!(
            matches!(verdict, Verdict::Silent { .. }),
            "a rule with no requirement at all should still be caught: {verdict:?}"
        );
    }

    /// Which axes a rule constrains, asked directly.
    ///
    /// The two probes are chosen by these, so a function that answered `true`
    /// for everything would put every rule through both — and one that
    /// answered `false` for everything would put it through neither and call
    /// it silent, which is the bug this replaced.
    #[test]
    fn each_axis_is_recognised_on_its_own() {
        let none = CompiledRuleKind::Structure {
            allowed_subfolders: None,
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
            filename_patterns: Vec::new(),
        };
        assert!(!constrains_subfolders(&none));
        assert!(!constrains_filenames(&none));

        let folders = CompiledRuleKind::Structure {
            allowed_subfolders: Some(Vec::new()),
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
            filename_patterns: Vec::new(),
        };
        assert!(
            constrains_subfolders(&folders),
            "an empty allow-list forbids every subfolder, which is a constraint"
        );
        assert!(!constrains_filenames(&folders));

        let names = CompiledRuleKind::Structure {
            allowed_subfolders: None,
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
            filename_patterns: vec![Pattern::compile("^a$").expect("valid")],
        };
        assert!(!constrains_subfolders(&names));
        assert!(constrains_filenames(&names));

        let patterned = CompiledRuleKind::Structure {
            allowed_subfolders: None,
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: vec![Pattern::compile("^a$").expect("valid")],
            filename_patterns: Vec::new(),
        };
        assert!(
            constrains_subfolders(&patterned),
            "a subfolder regex is a constraint on subfolders"
        );
    }

    /// The probe filename has to be one the rule refuses.
    ///
    /// A name the pattern happens to accept would be reported silent for a
    /// file the rule was right to allow — the same false negative one layer
    /// down, and the one this whole change is about.
    #[test]
    fn the_probe_filename_is_one_the_patterns_reject() {
        // Written against the patterns themselves rather than against a
        // spelling: the contract is "this rule rejects the probe", and a test
        // that checked a suffix would pass for a probe the rule accepts.
        for source in [r"\.probe$", r"^[a-z0-9-]+\.ts$", r"^archwarden-.*$"] {
            let pattern = Pattern::compile(source).expect("valid");
            let kind = CompiledRuleKind::Structure {
                allowed_subfolders: None,
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: Vec::new(),
                filename_patterns: vec![pattern],
            };

            let probe = unclaimed_filename(&kind);
            // The name is printed back — "a file named `X` in `Y`" — so it has
            // to read as archwarden's, not as a file the reader might go
            // looking for in their own repository.
            assert!(
                probe.contains(PROBE),
                "`{probe}` does not name itself as a probe, and it is shown to \
                 a reader as the thing the rule was handed"
            );

            let CompiledRuleKind::Structure {
                filename_patterns, ..
            } = &kind
            else {
                unreachable!("built as a structure rule")
            };
            assert!(
                !filename_patterns
                    .iter()
                    .any(|pattern| pattern.is_match(&probe)),
                "`{probe}` is a name `{source}` accepts, so the rule is right to \
                 stay silent about it and would be called idle for doing so"
            );
        }
    }

    /// The case the issue called impossible. `spec-pair` reports through
    /// `check_directory`, and what it is handed is a listing -- so a listing
    /// with a lone source file in it is the absence, synthesised.
    #[test]
    fn a_spec_pair_rule_is_probed_with_a_file_that_has_no_spec() {
        let verdict = verdict(
            &["src/order/x.ts", "src/order/x.spec.ts"],
            vec![rule(
                "calcs-need-spec",
                &["src/*"],
                CompiledRuleKind::SpecPair {
                    subfolders: vec![".".to_owned()],
                    spec_markers: vec!["spec".to_owned()],
                    ignore_files: PathSet::default(),
                    spec_dirs: Vec::new(),
                    require_non_empty_spec: false,
                    skip_type_only: false,
                },
            )],
        );

        assert!(matches!(verdict, Verdict::Fires { .. }), "{verdict:?}");
    }

    /// A scope that reaches nothing cannot be probed, and says so rather than
    /// accusing the rule of being silent. The two are different problems and
    /// `doctor` owns the first.
    #[test]
    fn a_rule_that_reaches_nothing_is_unverified_rather_than_silent() {
        let verdict = verdict(
            &["src/order/x.ts"],
            vec![rule(
                "nowhere",
                &["packages/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec!["types".to_owned()]),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )],
        );

        assert!(matches!(verdict, Verdict::Unverified { .. }), "{verdict:?}");
    }

    /// A boundary whose forbidden side names nothing this repository has is
    /// unverified too: the probe would have to import a file that does not
    /// exist, and a rule cannot be blamed for not catching it.
    #[test]
    fn a_boundary_with_nothing_to_import_is_unverified() {
        let verdict = verdict(
            &["packages/domain/order.ts"],
            vec![rule(
                "domain-is-self-contained",
                &["packages/domain/**"],
                boundary(&["apps/**"], &[], &[]),
            )],
        );

        assert!(matches!(verdict, Verdict::Unverified { .. }), "{verdict:?}");
    }

    /// The two kinds whose violation is a filename say so, rather than being
    /// left out of the report. A rule that went unchecked has to be visible as
    /// unchecked.
    #[test]
    fn the_kinds_that_cannot_be_synthesised_are_named() {
        let verdict = verdict(
            &["src/order/create.use-case.ts"],
            vec![rule(
                "usecase-name",
                &["src/*"],
                CompiledRuleKind::Naming {
                    file_pattern: Pattern::compile(r"^(?<name>[a-z-]+)\.use-case\.ts$")
                        .expect("valid pattern"),
                    dir_pattern: None,
                    name_template: "{{pascal(name)}}".to_owned(),
                    kind: archwarden_core::facts::KindFilter::Any,
                    annotation: Vec::new(),
                    signature_hint: None,
                },
            )],
        );

        let Verdict::Unverified { why } = verdict else {
            panic!("a filename cannot be invented: {verdict:?}");
        };
        assert!(why.contains("file_pattern"), "{why}");
    }

    /// A `structure` rule that allows a folder by the probe's own name is
    /// handed a different one. Being told a rule is silent because it was
    /// handed something legal is a false accusation, in the one command whose
    /// job is not to make them.
    #[test]
    fn the_probe_never_uses_a_name_the_rule_allows() {
        let verdict = verdict(
            &["src/order/types/x.ts"],
            vec![rule(
                "entity-shape",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec![PROBE.to_owned(), "types".to_owned()]),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )],
        );

        assert!(
            matches!(verdict, Verdict::Fires { .. }),
            "the probe should have picked another name: {verdict:?}"
        );
    }
}
