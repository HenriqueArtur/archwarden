//! `agent-guide` — the rule set as a digest for an agent's context.
//!
//! Layer 3 of `AGENT-INTEGRATION.md`. Deterministic: the same configuration
//! produces the same value, so the output can be committed or regenerated
//! without either choice creating noise.
//!
//! The digest itself moved here in 0.18; the three renderings stayed in
//! `archwarden-cli`. That is the same seam [`crate::render`] draws for the
//! report — a shape a program consumes is a contract, and the markdown and the
//! page are a surface's own. `GuideFormat` stayed behind with them because it
//! carries `clap::ValueEnum`, on the argument decision 20 already made about
//! `LevelFilter`: a command-line vocabulary is not an operation.

use std::fmt::Write as _;

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRule, CompiledRuleKind},
    facts::{ExportKind, KindFilter},
    path::RepoRelPath,
};
use serde::Serialize;

/// The version of the `agent-guide` JSON shape.
pub const GUIDE_VERSION: u32 = 0;

/// Every rule the guide covers, in configuration order.
#[derive(Debug, Serialize)]
pub struct Guide<'a> {
    /// The shape's version, [`GUIDE_VERSION`].
    pub version: u32,
    /// The scope the guide was restricted to, when it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<&'a str>,
    /// The kinds it was restricted to, when it was.
    ///
    /// Carried so a renderer can tell an empty *repository* from an empty
    /// *slice of one*. Without it the digest said "No rules are configured"
    /// to somebody with nine rules and none of the kind they asked about —
    /// two states, one sentence, and one of them false. Issue #97.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<&'a str>,
    /// Every rule it covers, in configuration order.
    pub rules: Vec<GuideRule<'a>>,
}

/// One rule, as the digest carries it.
#[derive(Debug, Serialize)]
pub struct GuideRule<'a> {
    /// The rule's id.
    pub id: &'a str,
    /// Its kind, as written in the config's `type`.
    pub kind: &'static str,
    /// `error` or `warning`.
    pub level: &'a str,
    /// The module it belongs to, when it belongs to one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<&'a str>,
    /// Directory globs the rule governs.
    pub applies_to: &'a [String],
    /// One sentence per requirement, the same prose `describe` prints.
    pub requires: Vec<String>,
    /// Why the rule exists, when its author said. A digest without them is a
    /// list of prohibitions, which is what an agent works around. Issue #46.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<&'a str>,
    /// Why the module it belongs to exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_why: Option<&'a str>,
}

/// Every rule kind archwarden has, as written in a config and on the command
/// line.
///
/// Listed here rather than derived, because `CompiledRuleKind::type_name` maps
/// one way only. The compiler cannot enforce this list — a variant added to
/// the enum still compiles with nothing added here — so
/// `every_kind_the_tool_has_is_accepted` builds one rule of every kind and
/// checks both directions. It is a test standing where a type would be better,
/// and it is load-bearing: `no-passthrough` was missing from this list from the
/// day that rule shipped, so `agent-guide --kinds no-passthrough` refused a
/// kind archwarden has.
pub const KINDS: [&str; 10] = [
    "structure",
    "naming",
    "spec-pair",
    "no-passthrough",
    "presence",
    "pair",
    "frontmatter",
    "import-boundary",
    "import-cycle",
    "call-obligation",
];

/// Checks the kinds a caller asked for.
///
/// # Errors
/// A message naming the one archwarden does not have, and the ones it does.
/// Refused rather than answered with an empty digest: an agent handed one
/// would conclude the project has no rules of that kind, which is the wrong
/// lesson to draw from a typo.
pub fn guide_kinds(kinds: &[String]) -> Result<(), String> {
    for kind in kinds {
        if !KINDS.contains(&kind.as_str()) {
            let known: Vec<String> = KINDS.iter().map(|kind| format!("`{kind}`")).collect();
            return Err(format!(
                "no rule kind is called `{kind}`; there is {}",
                known.join(", ")
            ));
        }
    }
    Ok(())
}

/// Builds the guide, optionally restricted to rules that can fire under
/// `scope` and to the kinds named.
///
/// Both filters are AND, and an empty `kinds` means every kind: "the import
/// boundaries that affect this directory" is one question, not two.
#[must_use]
pub fn guide<'a>(
    config: &'a CompiledConfig,
    scope: Option<&'a RepoRelPath>,
    kinds: &'a [String],
) -> Guide<'a> {
    Guide {
        version: GUIDE_VERSION,
        scope: scope.map(RepoRelPath::as_str),
        kinds: kinds.iter().map(String::as_str).collect(),
        rules: config
            .rules()
            .filter(|rule| kinds.is_empty() || kinds.iter().any(|k| k == rule.kind.type_name()))
            .filter(|rule| scope.is_none_or(|prefix| reaches(rule, prefix)))
            .map(|rule| GuideRule {
                id: rule.id.as_str(),
                kind: rule.kind.type_name(),
                level: rule.level.as_str(),
                module: rule
                    .module
                    .as_ref()
                    .map(archwarden_core::ids::ModuleId::as_str),
                applies_to: rule.scope.patterns(),
                requires: requirements(&rule.kind),
                why: rule.why.as_deref(),
                module_why: rule.module_why.as_deref(),
            })
            .collect(),
    }
}

/// Whether a rule could ever fire under `prefix`.
///
/// Two directions, because a user asking for "the guide for `packages/domain`"
/// means both. A rule scoped to `packages/**` governs that directory; a rule
/// scoped to `packages/domain/src/*` lives inside it. Either answer is yes.
fn reaches(rule: &CompiledRule, prefix: &RepoRelPath) -> bool {
    if rule.scope.matches_dir(prefix.as_path()) {
        return true;
    }

    let prefix = prefix.as_str();
    rule.scope.patterns().iter().any(|pattern| {
        prefix.is_empty() || pattern == prefix || pattern.starts_with(&format!("{prefix}/"))
    })
}

/// One sentence per thing the rule requires.
///
/// One arm per rule kind, each a handful of lines. Splitting it would put the
/// arms somewhere the exhaustive `match` no longer names them, which is the
/// property that makes a kind added without a sentence fail to compile.
#[allow(clippy::too_many_lines, reason = "one arm per rule kind; see above")]
fn requirements(kind: &CompiledRuleKind) -> Vec<String> {
    match kind {
        CompiledRuleKind::Structure {
            allowed_subfolders,
            warn_subfolders,
            subfolder_patterns,
            filename_patterns,
            ..
        } => {
            let mut lines = Vec::new();
            if allowed_subfolders.is_some() || !warn_subfolders.is_empty() {
                let allowed = allowed_subfolders.as_deref().unwrap_or_default();
                let mut line = format!("subfolders: {}", join(allowed));
                if !warn_subfolders.is_empty() {
                    let _ = write!(line, "; allowed with a warning: {}", join(warn_subfolders));
                }
                lines.push(line);
            }
            if !subfolder_patterns.is_empty() {
                lines.push(format!(
                    "subfolder names must match: {}",
                    join(
                        &subfolder_patterns
                            .iter()
                            .map(|p| p.as_str().to_owned())
                            .collect::<Vec<_>>()
                    )
                ));
            }
            if !filename_patterns.is_empty() {
                lines.push(format!(
                    "filenames must match: {}",
                    join(
                        &filename_patterns
                            .iter()
                            .map(|p| p.as_str().to_owned())
                            .collect::<Vec<_>>()
                    )
                ));
            }
            lines
        }
        CompiledRuleKind::NoPassthrough {
            forms,
            allow_package_entrypoints,
            allow_partial,
            ..
        } => {
            let mut shapes = Vec::new();
            if forms.reexport {
                shapes.push("re-exporting it");
            }
            if forms.alias {
                shapes.push("aliasing it");
            }
            if forms.wrapper {
                shapes.push("wrapping it in a one-line function");
            }
            let mut lines = vec![format!(
                "a file must add something of its own: not only {}",
                join(&shapes.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>()),
            )];
            if *allow_package_entrypoints {
                lines.push(
                    "a package entry point is exempt: forwarding is what a public API is for"
                        .to_owned(),
                );
            }
            if !*allow_partial {
                lines.push("forwarding some exports while declaring others counts too".to_owned());
            }
            lines
        }
        CompiledRuleKind::Naming {
            file_pattern,
            dir_pattern,
            name_template,
            kind,
            annotation,
            signature_hint,
        } => {
            // The directory half belongs in the same sentence, not in a note
            // under it. An agent reading "files matching `^(?<action>...)$`
            // must export `{{pascal(entity)}}{{pascal(action)}}`" without being
            // told where `entity` comes from cannot produce the name, and this
            // digest is what it has instead of the config.
            //
            // The annotation belongs in that sentence too, and not in a note
            // under it beside the hint: one of those two is enforced and the
            // other is advice, and a digest that lists them together teaches an
            // agent to treat both as optional.
            let mut lines = vec![format!(
                "files matching `{}`{} must export `{name_template}`{}{}",
                file_pattern.as_str(),
                dir_pattern.as_ref().map_or_else(String::new, |pattern| {
                    format!(", in a directory matching `{}`", pattern.as_str())
                }),
                declared_as(kind),
                annotated_as(annotation),
            )];
            if let Some(hint) = signature_hint {
                lines.push(format!("suggested signature: `{hint}`"));
            }
            lines
        }
        CompiledRuleKind::SpecPair {
            subfolders,
            spec_markers,
            require_non_empty_spec,
            ..
        } => {
            let markers = spec_markers
                .iter()
                .map(|marker| format!(".{marker}."))
                .collect::<Vec<_>>();
            let mut line = format!(
                "every file in {} needs a sibling named with {}",
                join(subfolders),
                join(&markers)
            );
            if *require_non_empty_spec {
                line.push_str(", containing at least one test case");
            }
            vec![line]
        }
        CompiledRuleKind::ImportCycle { include_type_only } => {
            // One sentence, because the rule is one sentence. The chain that
            // broke it is a property of the repository rather than of the
            // rule, so there is nothing here for an agent to satisfy beyond
            // "do not close a loop".
            let mut line =
                "must not sit on an import cycle, directly or through other files".to_owned();
            if !include_type_only {
                line.push_str(" (a loop made only of `import type` is exempt)");
            }
            vec![line]
        }
        CompiledRuleKind::ImportBoundary {
            forbid,
            require,
            groups: _,
            allow,
            allow_packages,
            forbid_packages,
            forbid_reaching,
            except,
            except_from,
            include_type_only,
        } => {
            let mut lines = Vec::new();
            // First, because it is the strongest sentence a boundary can say:
            // everything not named is refused, including what does not exist
            // yet. An agent reading the denials first would take the silence
            // about everything else for permission.
            if let Some(allow) = allow {
                lines.push(format!(
                    "may import only from {} (its own files are always allowed, \
                     and packages are governed separately)",
                    join(allow.patterns())
                ));
            }
            if let Some(packages) = allow_packages {
                lines.push(format!(
                    "may import only these packages: {}",
                    join(packages.as_slice())
                ));
            }
            if !forbid.is_empty() {
                let mut line = format!("must not import from {}", join(forbid.patterns()));
                if !except.is_empty() {
                    let _ = write!(line, ", except {}", join(except.patterns()));
                }
                if !include_type_only {
                    line.push_str(" (type-only imports are exempt)");
                }
                lines.push(line);
            }
            // A separate sentence from the one above, because it is a
            // separate obligation: an agent that satisfies "do not import
            // `packages/db`" can still violate this by importing something
            // that does, and a digest that folded the two would have it
            // believe one edit covered both.
            if !forbid_reaching.is_empty() {
                let mut line = format!(
                    "must not end up depending on {}, however many imports away",
                    join(forbid_reaching.patterns())
                );
                if !except.is_empty() {
                    let _ = write!(line, ", except {}", join(except.patterns()));
                }
                if !include_type_only {
                    line.push_str(" (type-only edges are not followed)");
                }
                lines.push(line);
            }
            // The dependency half, in the same voice. An agent about to reach
            // for `three` has to learn it here or not at all: this digest is
            // what it has instead of the config, and a rule the digest omits is
            // a rule it will violate.
            if !forbid_packages.is_empty() {
                let mut line = format!(
                    "must not import the package {} (nor anything under it)",
                    join(forbid_packages)
                );
                if !except_from.is_empty() {
                    let _ = write!(line, "; only {} may", join(except_from.patterns()));
                }
                lines.push(line);
            }
            if !require.is_empty() {
                lines.push(format!("must import from {}", join(require.patterns())));
            }
            lines
        }
        CompiledRuleKind::Frontmatter {
            file_pattern,
            require,
            one_of,
            equals,
        } => {
            let mut lines = vec![format!(
                "documents matching `{}` need frontmatter",
                file_pattern.as_str()
            )];
            if !require.is_empty() {
                let quoted: Vec<String> = require.iter().map(|k| format!("`{k}`")).collect();
                lines.push(format!("carrying: {}", quoted.join(", ")));
            }
            for (key, accepted) in one_of {
                let quoted: Vec<String> = accepted.iter().map(|v| format!("`{v}`")).collect();
                lines.push(format!("`{key}` one of: {}", quoted.join(", ")));
            }
            for (key, template) in equals {
                lines.push(format!("`{key}` equal to `{template}`"));
            }
            lines
        }
        CompiledRuleKind::Pair {
            file_pattern,
            must_exist,
        } => vec![format!(
            "files matching `{}` need `{must_exist}` beside them",
            file_pattern.as_str()
        )],
        CompiledRuleKind::Presence {
            require,
            require_any,
        } => {
            let mut lines = Vec::new();
            if !require.is_empty() {
                // Comma-joined, not `join`: that helper reads "a or b", and
                // every one of these is required.
                let quoted: Vec<String> = require.iter().map(|n| format!("`{n}`")).collect();
                lines.push(format!("must contain: {}", quoted.join(", ")));
            }
            if !require_any.is_empty() {
                lines.push(format!(
                    "at least one file matching: {}",
                    join(
                        &require_any
                            .iter()
                            .map(|p| p.as_str().to_owned())
                            .collect::<Vec<_>>()
                    )
                ));
            }
            lines
        }
        CompiledRuleKind::CallObligation {
            file_pattern,
            symbol,
            imported_from,
        } => vec![format!(
            "files matching `{}` must call `{symbol}`, imported from `{imported_from}`",
            file_pattern.as_str()
        )],
    }
}

fn declared_as(kind: &KindFilter) -> String {
    match kind {
        KindFilter::OneOf(tags) => {
            let kinds: Vec<String> = tags
                .iter()
                .map(|k| ExportKind::as_str(k).to_owned())
                .collect();
            format!(", declared as {}", join(&kinds))
        }
        // `Any`, and any filter added later: the rule asked for no particular
        // form, and naming one would teach the agent a constraint that is not
        // there.
        _ => String::new(),
    }
}

/// The clause naming the type the export must write down, if the rule asks.
///
/// Empty for a rule that asks for none, which keeps the sentence of every rule
/// written before this field existed byte-identical -- `agent-guide` is
/// documented as deterministic and safe to commit, so an unrelated rule
/// growing a clause would show up as a diff in a repository nobody touched.
fn annotated_as(annotation: &[String]) -> String {
    if annotation.is_empty() {
        return String::new();
    }

    format!(", annotated {}", join(annotation))
}

/// `` `a` ``, `` `b` `` and `` `c` `` — the list form the digest is written in.
///
/// Public for the same reason [`crate::describe::join_or`] is: the markdown
/// renderer builds its own lines with it, and two copies of a comma rule is
/// two copies that drift.
#[must_use]
pub fn join(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| format!("`{item}`")).collect();
    match quoted.split_last() {
        None => "nothing".to_owned(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::{CompiledRule, SkipDirs},
        facts::{ExportTags, KindFilter},
        glob::PathSet,
        hash::ContentHash,
        ids::{ModuleId, RuleId},
        level::Level,
        pattern::Pattern,
        scope::Scope,
    };

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn rule(
        id: &str,
        module: Option<&str>,
        scope: &[&str],
        kind: CompiledRuleKind,
    ) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: module.map(|m| ModuleId::new(m).expect("valid module")),
            why: None,
            module_why: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(scope.iter().copied()).expect("valid scope"),
            kind,
        }
    }

    fn config(rules: Vec<CompiledRule>) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b"guide"),
        )
    }

    fn set(patterns: &[&str]) -> PathSet {
        PathSet::compile(patterns.iter().map(|p| (*p).to_owned())).expect("valid globs")
    }

    fn naming() -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile(r"^(?<name>[a-z0-9-]+)\.use-case\.ts$")
                .expect("valid pattern"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            annotation: Vec::new(),
            signature_hint: Some("(deps: Deps): UseCase".to_owned()),
        }
    }

    fn boundary() -> CompiledRuleKind {
        CompiledRuleKind::ImportBoundary {
            forbid: set(&["src/infra/**"]),
            groups: Vec::new(),
            allow: None,
            allow_packages: None,
            require: PathSet::default(),
            forbid_packages: Vec::new(),
            forbid_reaching: PathSet::default(),
            except: PathSet::default(),
            except_from: PathSet::default(),
            include_type_only: true,
        }
    }

    /// A boundary about a dependency rather than a layer.
    fn package_boundary(except_from: &[&str]) -> CompiledRuleKind {
        CompiledRuleKind::ImportBoundary {
            forbid: PathSet::default(),
            groups: Vec::new(),
            allow: None,
            allow_packages: None,
            require: PathSet::default(),
            forbid_packages: vec!["three".to_owned()],
            forbid_reaching: PathSet::default(),
            except: PathSet::default(),
            except_from: set(except_from),
            include_type_only: true,
        }
    }

    fn mixed() -> CompiledConfig {
        config(vec![
            rule("usecase-name", None, &["src/*"], naming()),
            rule("no-infra", None, &["src/**"], boundary()),
            rule("also-no-infra", None, &["packages/**"], boundary()),
        ])
    }

    /// Every requirement sentence the digest carries, which is what the
    /// renderers lay out. Asserting on these rather than on markdown is the
    /// point of the split: the sentences are the operation's answer, and the
    /// headings around them are a surface's choice.
    fn sentences(config: &CompiledConfig, scope: Option<&RepoRelPath>, kinds: &[&str]) -> String {
        let owned: Vec<String> = kinds.iter().map(|k| (*k).to_owned()).collect();
        guide(config, scope, &owned)
            .rules
            .iter()
            .flat_map(|rule| {
                rule.requires
                    .iter()
                    .cloned()
                    .chain(rule.why.map(str::to_owned))
                    .chain(rule.module_why.map(str::to_owned))
                    .chain(std::iter::once(rule.id.to_owned()))
                    .chain(rule.applies_to.iter().cloned())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The digest as a value, which is what every surface serialises.
    fn as_json(config: &CompiledConfig, scope: Option<&RepoRelPath>) -> serde_json::Value {
        serde_json::to_value(guide(config, scope, &[])).expect("serialises")
    }

    /// The digest is what an agent has instead of the config, so a rule it
    /// omits is a rule the agent will violate. Issue #14's angle: the rule
    /// used to live in a second config file archwarden never read.
    #[test]
    fn the_guide_names_a_forbidden_package_and_who_may_import_it() {
        let quarantined = config(vec![rule(
            "three-is-quarantined",
            None,
            &["src/**"],
            package_boundary(&["src/scripts/three/**"]),
        )]);
        let markdown = sentences(&quarantined, None, &[]);

        assert!(
            markdown.contains("must not import the package `three` (nor anything under it)"),
            "{markdown}"
        );
        assert!(
            markdown.contains("only `src/scripts/three/**` may"),
            "the one directory allowed is the half an agent needs most: {markdown}"
        );

        // And with nothing exempt, the sentence stops rather than trailing off
        // into an empty list.
        let everywhere = config(vec![rule(
            "no-three",
            None,
            &["src/**"],
            package_boundary(&[]),
        )]);
        let markdown = sentences(&everywhere, None, &[]);
        assert!(markdown.contains("(nor anything under it)"), "{markdown}");
        assert!(!markdown.contains("only "), "{markdown}");
    }

    /// Issue #44. The digest is what an agent has before it writes a document,
    /// and the frontmatter is the half a human never reads.
    #[test]
    fn a_frontmatter_rule_lists_its_keys_and_vocabularies() {
        let config = config(vec![rule(
            "projeto-frontmatter",
            None,
            &["projetos/*"],
            CompiledRuleKind::Frontmatter {
                file_pattern: Pattern::compile(r"^projeto\.md$").expect("valid"),
                require: vec!["id".to_owned(), "nivel".to_owned()],
                one_of: vec![("nivel".to_owned(), vec!["1".to_owned(), "2".to_owned()])],
                equals: vec![("id".to_owned(), "{{raw(dirname)}}".to_owned())],
            },
        )]);

        let markdown = sentences(&config, None, &[]);

        assert!(
            markdown.contains(r"documents matching `^projeto\.md$` need frontmatter"),
            "{markdown}"
        );
        assert!(markdown.contains("carrying: `id`, `nivel`"), "{markdown}");
        assert!(markdown.contains("`nivel` one of: `1`, `2`"), "{markdown}");
        assert!(
            markdown.contains("`id` equal to `{{raw(dirname)}}`"),
            "{markdown}"
        );
    }

    /// Issue #45. The digest has to say which of the two files needs the
    /// other, because the rule is one-directional and an agent that got it
    /// backwards would create the wrong file.
    #[test]
    fn a_pair_rule_says_which_file_needs_which() {
        let config = config(vec![rule(
            "licao-tem-notas",
            None,
            &["projetos/*"],
            CompiledRuleKind::Pair {
                file_pattern: Pattern::compile(r"^projeto\.md$").expect("valid"),
                must_exist: "notas.md".to_owned(),
            },
        )]);

        let markdown = sentences(&config, None, &[]);

        assert!(
            markdown.contains(r"files matching `^projeto\.md$` need `notas.md` beside them"),
            "{markdown}"
        );
    }

    /// Issue #42. The digest teaches the rules before a question is asked, and
    /// "a lesson directory has these four files" is the one an agent most needs
    /// before creating one.
    #[test]
    fn a_presence_rule_lists_what_must_exist() {
        let config = config(vec![rule(
            "licao-completa",
            None,
            &["projetos/*"],
            CompiledRuleKind::Presence {
                require: vec!["projeto.md".to_owned(), "notas.md".to_owned()],
                require_any: vec![Pattern::compile(r"\.ino$").expect("valid")],
            },
        )]);

        let markdown = sentences(&config, None, &[]);

        assert!(
            markdown.contains("must contain: `projeto.md`, `notas.md`"),
            "{markdown}"
        );
        assert!(
            markdown.contains(r"at least one file matching: `\.ino$`"),
            "{markdown}"
        );
    }

    /// A digest is what an agent has instead of the config, and "the folder
    /// name has to look like this" is exactly what it needs before creating
    /// one.
    #[test]
    fn a_subfolder_pattern_appears_in_the_digest() {
        let by_shape = config(vec![rule(
            "licao-nome-da-pasta",
            None,
            &["projetos"],
            CompiledRuleKind::Structure {
                allowed_subfolders: None,
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: vec![Pattern::compile(r"^\d{2}-[a-z0-9-]+$").expect("valid")],
                filename_patterns: Vec::new(),
            },
        )]);

        let markdown = sentences(&by_shape, None, &[]);

        assert!(
            markdown.contains(r"subfolder names must match: `^\d{2}-[a-z0-9-]+$`"),
            "{markdown}"
        );
    }

    #[test]
    fn a_required_annotation_is_part_of_the_sentence() {
        let annotated = CompiledRuleKind::Naming {
            file_pattern: Pattern::compile(r"^(?<tool>[a-z0-9-]+)\.tool\.ts$").expect("valid"),
            dir_pattern: None,
            name_template: "AGENT_TOOL".to_owned(),
            kind: KindFilter::OneOf(ExportTags::only(ExportKind::Const)),
            annotation: vec!["AgentToolModule".to_owned()],
            signature_hint: None,
        };
        let config = config(vec![rule("tools", None, &["src/*"], annotated)]);

        let markdown = sentences(&config, None, &[]);

        assert!(
            markdown.contains("annotated `AgentToolModule`"),
            "{markdown}"
        );
    }

    /// "Just the import boundaries that reach this directory" is a real
    /// question, and answering it by hand means reading past everything else.
    #[test]
    fn a_kind_narrows_the_digest_to_that_kind() {
        let markdown = sentences(&mixed(), None, &["import-boundary"]);

        assert!(markdown.contains("no-infra"), "{markdown}");
        assert!(markdown.contains("also-no-infra"), "{markdown}");
        assert!(!markdown.contains("usecase-name"), "{markdown}");
    }

    /// Several kinds are a set, however they were written on the command line.
    #[test]
    fn several_kinds_are_a_set() {
        let markdown = sentences(&mixed(), None, &["import-boundary", "naming"]);

        assert!(markdown.contains("no-infra"), "{markdown}");
        assert!(markdown.contains("usecase-name"), "{markdown}");
    }

    /// With `--scope`, because the question that prompted this was "the import
    /// boundaries *that affect this directory*" -- one filter answers half of
    /// it.
    #[test]
    fn a_kind_composes_with_a_scope() {
        let markdown = sentences(&mixed(), Some(&path("src")), &["import-boundary"]);

        assert!(markdown.contains("no-infra"), "{markdown}");
        assert!(
            !markdown.contains("also-no-infra"),
            "scoped to packages: {markdown}"
        );
        assert!(!markdown.contains("usecase-name"), "{markdown}");
    }

    /// A kind no rule has is refused, not answered with an empty digest. An
    /// agent handed an empty guide would conclude the project has no rules of
    /// that kind, which is the wrong lesson to draw from a typo.
    ///
    /// `boundary` rather than a misspelling, because the likely mistake is a
    /// user writing the short name they say out loud.
    #[test]
    fn an_unknown_kind_is_refused_and_names_the_real_ones() {
        let message = guide_kinds(&["boundary".to_owned()]).expect_err("no such kind");

        assert!(message.contains("`boundary`"), "{message}");
        assert!(message.contains("`import-boundary`"), "{message}");
        assert!(message.contains("`spec-pair`"), "{message}");
    }

    /// `KINDS` and the enum say the same thing, in both directions.
    ///
    /// The list is hand-written and the compiler cannot check it, so this
    /// stands in its place. Built from real `CompiledRuleKind` values rather
    /// than from a second list of strings, because a second list of strings is
    /// exactly what drifted: `no-passthrough` was absent from `KINDS` from the
    /// day that rule shipped, and the test that was here listed five of the
    /// then-eight kinds, so it agreed.
    #[test]
    fn every_kind_the_tool_has_is_accepted() {
        let every: Vec<CompiledRuleKind> = vec![
            CompiledRuleKind::Structure {
                allowed_subfolders: None,
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: Vec::new(),
                filename_patterns: Vec::new(),
            },
            naming(),
            CompiledRuleKind::SpecPair {
                subfolders: vec![".".to_owned()],
                spec_markers: vec!["spec".to_owned()],
                ignore_files: PathSet::default(),
                spec_dirs: Vec::new(),
                require_non_empty_spec: false,
                skip_type_only: false,
            },
            CompiledRuleKind::NoPassthrough {
                forms: archwarden_core::compiled::PassthroughForms {
                    reexport: true,
                    alias: true,
                    wrapper: true,
                },
                except: PathSet::default(),
                allow_package_entrypoints: true,
                allow_partial: true,
            },
            CompiledRuleKind::Presence {
                require: Vec::new(),
                require_any: Vec::new(),
            },
            CompiledRuleKind::Pair {
                file_pattern: Pattern::compile(r"^x\.ts$").expect("valid pattern"),
                must_exist: "y.ts".to_owned(),
            },
            CompiledRuleKind::Frontmatter {
                file_pattern: Pattern::compile(r"^DOC\.md$").expect("valid pattern"),
                require: Vec::new(),
                one_of: Vec::new(),
                equals: Vec::new(),
            },
            boundary(),
            CompiledRuleKind::ImportCycle {
                include_type_only: true,
            },
            CompiledRuleKind::CallObligation {
                file_pattern: Pattern::compile(r"^route\.ts$").expect("valid pattern"),
                symbol: "Event.save".to_owned(),
                imported_from: "@org/events".to_owned(),
            },
        ];

        for kind in &every {
            let name = kind.type_name();
            guide_kinds(&[name.to_owned()]).unwrap_or_else(|_| {
                panic!("`{name}` is a kind archwarden has, and `KINDS` omits it")
            });
        }

        let mut named: Vec<&str> = every.iter().map(CompiledRuleKind::type_name).collect();
        named.sort_unstable();
        let mut listed: Vec<&str> = KINDS.to_vec();
        listed.sort_unstable();
        assert_eq!(
            listed, named,
            "`KINDS` names a kind the enum does not have, or the sample above \
             is missing one the enum does"
        );
    }

    /// `--scope` keeps a rule that governs the directory asked about.
    #[test]
    fn a_rule_governing_the_scope_is_kept() {
        let config = config(vec![rule("wide", None, &["packages/**"], naming())]);
        let scope = path("packages/domain");

        assert_eq!(guide(&config, Some(&scope), &[]).rules.len(), 1);
    }

    /// And one that lives *inside* it, which is the other direction a user
    /// means by "the guide for this package".
    #[test]
    fn a_rule_inside_the_scope_is_kept() {
        let config = config(vec![rule(
            "inner",
            None,
            &["packages/domain/src/*"],
            naming(),
        )]);
        let scope = path("packages/domain");

        assert_eq!(guide(&config, Some(&scope), &[]).rules.len(), 1);
    }

    /// A rule that can never fire under the scope is left out -- that is the
    /// whole point of the flag.
    #[test]
    fn a_rule_elsewhere_is_dropped() {
        let config = config(vec![
            rule("here", None, &["packages/domain/**"], naming()),
            rule("elsewhere", None, &["apps/web/**"], naming()),
        ]);

        let scope = path("packages/domain");
        let built = guide(&config, Some(&scope), &[]);
        let ids: Vec<_> = built.rules.iter().map(|r| r.id).collect();
        assert_eq!(ids, ["here"]);
    }

    /// No scope means the whole configuration.
    #[test]
    fn no_scope_keeps_everything() {
        let config = config(vec![
            rule("here", None, &["packages/domain/**"], naming()),
            rule("elsewhere", None, &["apps/web/**"], naming()),
        ]);

        assert_eq!(guide(&config, None, &[]).rules.len(), 2);
    }

    /// A scope of the repository root keeps everything, which is what
    /// `--scope .` means and what any rule at the top level needs.
    #[test]
    fn the_root_as_a_scope_keeps_everything() {
        let config = config(vec![
            rule("here", None, &["packages/domain/**"], naming()),
            rule("elsewhere", None, &["apps/web/**"], naming()),
        ]);
        let root = path(".");

        assert!(root.is_root());
        assert_eq!(guide(&config, Some(&root), &[]).rules.len(), 2);
    }

    /// A structure rule may constrain only the warn list, or only filenames.
    /// Each half has to reach the guide on its own, or a rule would be listed
    /// with nothing under it.
    #[test]
    fn each_half_of_a_structure_rule_stands_alone() {
        let warn_only = sentences(
            &config(vec![rule(
                "shape",
                None,
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(Vec::new()),
                    warn_subfolders: vec!["shared".to_owned()],
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )]),
            None,
            &[],
        );
        assert!(warn_only.contains("subfolders: nothing"), "{warn_only}");
        assert!(
            warn_only.contains("allowed with a warning: `shared`"),
            "{warn_only}"
        );

        let names_only = sentences(
            &config(vec![rule(
                "shape",
                None,
                &["src/*"],
                CompiledRuleKind::Structure {
                    // Absent, not empty: after issue #40 an empty list is a
                    // constraint -- "no subfolder may exist here" -- and the
                    // digest has to say so.
                    allowed_subfolders: None,
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: vec![Pattern::compile("^[a-z-]+\\.ts$").expect("valid")],
                },
            )]),
            None,
            &[],
        );
        assert!(names_only.contains("filenames must match:"), "{names_only}");
        assert!(
            !names_only.contains("subfolders:"),
            "a rule that constrains no subfolders says nothing about them: {names_only}"
        );
    }

    /// A structure rule that constrains only its subfolders says nothing about
    /// filenames, for the same reason.
    #[test]
    fn a_structure_rule_without_filename_patterns_says_nothing_about_names() {
        let markdown = sentences(
            &config(vec![rule(
                "shape",
                None,
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec!["types".to_owned()]),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )]),
            None,
            &[],
        );

        assert!(markdown.contains("subfolders: `types`"), "{markdown}");
        assert!(!markdown.contains("allowed with a warning"), "{markdown}");
        assert!(!markdown.contains("filenames must match"), "{markdown}");
    }

    /// The JSON is the machine-readable half, versioned like the others.
    #[test]
    fn the_json_shape_is_versioned() {
        let scope = path("src");
        let json = as_json(
            &config(vec![rule(
                "usecase-name",
                Some("app"),
                &["src/*"],
                naming(),
            )]),
            Some(&scope),
        );
        let parsed = &json;

        assert_eq!(parsed["version"], 0);
        assert_eq!(parsed["scope"], "src");
        assert_eq!(parsed["rules"][0]["id"], "usecase-name");
        assert_eq!(parsed["rules"][0]["kind"], "naming");
        assert_eq!(parsed["rules"][0]["level"], "error");
        assert_eq!(parsed["rules"][0]["module"], "app");
        assert_eq!(parsed["rules"][0]["applies_to"][0], "src/*");
        assert!(
            parsed["rules"][0]["requires"][0]
                .as_str()
                .is_some_and(|line| line.contains("{{pascal(name)}}")),
            "{json}"
        );
    }

    /// An unrestricted guide omits the scope rather than sending null.
    #[test]
    fn an_unrestricted_guide_omits_the_scope() {
        let json = as_json(
            &config(vec![rule("usecase-name", None, &["src/*"], naming())]),
            None,
        );

        assert!(
            json.get("scope").is_none(),
            "omitted rather than sent as null: {json}"
        );
    }

    /// Every rule kind states its requirement, because a guide that quietly
    /// omitted one would teach an incomplete rule set.
    #[test]
    fn every_rule_kind_states_its_requirement() {
        let config = config(vec![
            rule(
                "shape",
                None,
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec!["types".to_owned()]),
                    warn_subfolders: vec!["shared".to_owned()],
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: vec![Pattern::compile("^[a-z-]+\\.ts$").expect("valid")],
                },
            ),
            rule("name", None, &["src/*"], naming()),
            rule(
                "spec",
                None,
                &["src/*"],
                CompiledRuleKind::SpecPair {
                    subfolders: vec![".".to_owned()],
                    spec_markers: vec!["spec".to_owned(), "test".to_owned()],
                    ignore_files: PathSet::default(),
                    spec_dirs: Vec::new(),
                    require_non_empty_spec: true,
                    skip_type_only: false,
                },
            ),
            rule(
                "boundary",
                None,
                &["src/**"],
                CompiledRuleKind::ImportBoundary {
                    forbid: set(&["src/infra/**"]),
                    groups: Vec::new(),
                    allow: None,
                    allow_packages: None,
                    require: set(&["src/telemetry/**"]),
                    forbid_packages: Vec::new(),
                    forbid_reaching: PathSet::default(),
                    except: set(&["src/infra/types/**"]),
                    except_from: PathSet::default(),
                    include_type_only: false,
                },
            ),
            rule(
                "audit",
                None,
                &["src/*"],
                CompiledRuleKind::CallObligation {
                    file_pattern: Pattern::compile(r"^route\.post\.ts$").expect("valid"),
                    symbol: "Event.save".to_owned(),
                    imported_from: "@org/domain/event".to_owned(),
                },
            ),
        ]);

        let built = guide(&config, None, &[]);
        for entry in &built.rules {
            assert!(
                !entry.requires.is_empty(),
                "`{}` ({}) says nothing it requires",
                entry.id,
                entry.kind
            );
        }

        let markdown = sentences(&config, None, &[]);
        assert!(markdown.contains("subfolders: `types`"), "{markdown}");
        assert!(
            markdown.contains("allowed with a warning: `shared`"),
            "{markdown}"
        );
        assert!(markdown.contains("filenames must match:"), "{markdown}");
        assert!(
            markdown.contains("needs a sibling named with `.spec.` or `.test.`"),
            "{markdown}"
        );
        assert!(
            markdown.contains("containing at least one test case"),
            "{markdown}"
        );
        assert!(
            markdown.contains("must not import from `src/infra/**`, except `src/infra/types/**`"),
            "{markdown}"
        );
        assert!(
            markdown.contains("type-only imports are exempt"),
            "{markdown}"
        );
        assert!(
            markdown.contains("must import from `src/telemetry/**`"),
            "{markdown}"
        );
        assert!(
            markdown.contains("must call `Event.save`, imported from `@org/domain/event`"),
            "{markdown}"
        );
    }

    /// `forbid_reaching` is its own sentence, and only when it is set.
    ///
    /// A digest that folded it into the `forbid_import_from` line would have an
    /// agent believe one edit covers both, and they are different obligations:
    /// removing the import it names does not remove the dependency this one is
    /// about. A digest that printed it for a rule that does not set it would
    /// teach a constraint nobody wrote.
    #[test]
    fn reaching_is_its_own_sentence_and_only_when_it_is_set() {
        let with = |patterns: &[&str], exceptions: &[&str], type_only: bool| {
            let mut kind = boundary();
            let CompiledRuleKind::ImportBoundary {
                forbid: forbid_slot,
                forbid_reaching,
                except,
                include_type_only,
                ..
            } = &mut kind
            else {
                panic!("built as an import-boundary rule");
            };
            *forbid_slot = PathSet::default();
            *except = set(exceptions);
            *forbid_reaching = set(patterns);
            *include_type_only = type_only;
            sentences(
                &config(vec![rule("reach", None, &["packages/ui/*"], kind)]),
                None,
                &[],
            )
        };

        let markdown = with(&["packages/db/**"], &[], true);
        assert!(
            markdown.contains("must not end up depending on `packages/db/**`"),
            "{markdown}"
        );
        assert!(
            !markdown.contains("must not import from"),
            "the direct sentence is a different obligation and this rule does \
             not make it: {markdown}"
        );
        assert!(
            !markdown.contains("type-only"),
            "the default follows type edges, so there is nothing to say: \
             {markdown}"
        );
        assert!(
            !markdown.contains("except"),
            "a rule with no exceptions must not advertise an empty list of \
             them, which reads as a carve-out nobody wrote: {markdown}"
        );

        assert!(
            with(&["packages/db/**"], &["packages/db/types/**"], true)
                .contains("except `packages/db/types/**`"),
            "an exception changes what the rule permits, so the digest names it"
        );
        assert!(
            with(&["packages/db/**"], &[], false).contains("type-only edges are not followed"),
            "the opt-out changes what the rule enforces, so the digest says so"
        );

        let silent = sentences(
            &config(vec![rule("plain", None, &["packages/ui/*"], boundary())]),
            None,
            &[],
        );
        assert!(
            !silent.contains("end up depending on"),
            "a rule that says nothing about reach teaches nothing about it: \
             {silent}"
        );
    }

    /// `kind: "any"` asks for no declaration form, so the guide must not
    /// invent one -- an agent taught a constraint that is not there writes to
    /// satisfy a rule nobody set.
    #[test]
    fn any_form_teaches_no_form() {
        let markdown = sentences(
            &config(vec![rule(
                "name",
                None,
                &["src/*"],
                CompiledRuleKind::Naming {
                    file_pattern: Pattern::compile("^(?<name>[a-z]+)\\.ts$").expect("valid"),
                    dir_pattern: None,
                    name_template: "{{pascal(name)}}".to_owned(),
                    kind: KindFilter::Any,
                    annotation: Vec::new(),
                    signature_hint: None,
                },
            )]),
            None,
            &[],
        );

        assert!(
            markdown.contains("must export `{{pascal(name)}}`"),
            "{markdown}"
        );
        assert!(!markdown.contains("declared as"), "{markdown}");
    }

    /// The template is reproduced verbatim in the requirement sentence, which
    /// is the operation's answer. How a renderer lays it out is that surface's
    /// business; that the sentence carries the template is this crate's.
    #[test]
    fn a_naming_rules_requirement_names_its_template_and_form() {
        let said = sentences(
            &config(vec![rule(
                "usecase-name",
                Some("app"),
                &["src/*"],
                naming(),
            )]),
            None,
            &[],
        );

        assert!(said.contains("{{pascal(name)}}"), "{said}");
        assert!(said.contains("declared as `function`"), "{said}");
        assert!(
            said.contains("suggested signature: `(deps: Deps): UseCase`"),
            "{said}"
        );
    }

    /// Configuration order is preserved, so a diff of a committed guide
    /// follows the config rather than an internal ordering.
    #[test]
    fn rules_come_back_in_configuration_order() {
        let config = config(vec![
            rule("second", None, &["src/*"], naming()),
            rule("first", None, &["src/*"], naming()),
        ]);
        let digest = guide(&config, None, &[]);
        let ids: Vec<&str> = digest.rules.iter().map(|rule| rule.id).collect();

        assert_eq!(ids, ["second", "first"], "config order, not alphabetical");
    }

    /// An allowlist is stated as an allowlist, and says out loud that packages
    /// are governed separately — an agent reading a denial list first would
    /// take the silence about everything else for permission.
    #[test]
    fn an_allowlist_says_it_is_one_and_names_its_two_halves() {
        let said = sentences(
            &config(vec![rule(
                "only-these",
                None,
                &["src/*"],
                CompiledRuleKind::ImportBoundary {
                    forbid: PathSet::default(),
                    groups: Vec::new(),
                    allow: Some(set(&["src/domain/**"])),
                    allow_packages: Some(vec!["zod".to_owned()]),
                    require: PathSet::default(),
                    forbid_packages: Vec::new(),
                    forbid_reaching: PathSet::default(),
                    except: PathSet::default(),
                    except_from: PathSet::default(),
                    include_type_only: true,
                },
            )]),
            None,
            &[],
        );

        assert!(
            said.contains("may import only from `src/domain/**`"),
            "{said}"
        );
        assert!(
            said.contains("packages are governed separately"),
            "the two lists are not one list: {said}"
        );
        assert!(
            said.contains("may import only these packages: `zod`"),
            "{said}"
        );
    }

    /// Every shape of forwarding the rule counts is named, and so are both
    /// exemptions — a digest that named the rule without its exemptions would
    /// have an agent rewrite a package entry point that was always fine.
    #[test]
    fn a_no_passthrough_rule_names_the_forms_and_the_exemptions() {
        let said = sentences(
            &config(vec![rule(
                "adds-something",
                None,
                &["src/*"],
                CompiledRuleKind::NoPassthrough {
                    forms: archwarden_core::compiled::PassthroughForms {
                        reexport: true,
                        alias: true,
                        wrapper: true,
                    },
                    except: PathSet::default(),
                    allow_package_entrypoints: true,
                    allow_partial: false,
                },
            )]),
            None,
            &[],
        );

        assert!(said.contains("re-exporting it"), "{said}");
        assert!(said.contains("aliasing it"), "{said}");
        assert!(
            said.contains("wrapping it in a one-line function"),
            "{said}"
        );
        assert!(said.contains("a package entry point is exempt"), "{said}");
        assert!(
            said.contains("forwarding some exports while declaring others counts too"),
            "{said}"
        );
    }

    /// A spec-pair rule names the folders, the markers, and whether an empty
    /// file would satisfy it — which is the whole of a TDD gate.
    #[test]
    fn a_spec_pair_rule_names_its_markers_and_whether_empty_counts() {
        let said = sentences(
            &config(vec![rule(
                "needs-spec",
                None,
                &["src/*"],
                CompiledRuleKind::SpecPair {
                    subfolders: vec![".".to_owned()],
                    spec_markers: vec!["spec".to_owned(), "test".to_owned()],
                    ignore_files: PathSet::default(),
                    spec_dirs: Vec::new(),
                    require_non_empty_spec: true,
                    skip_type_only: false,
                },
            )]),
            None,
            &[],
        );

        assert!(said.contains("needs a sibling named with"), "{said}");
        assert!(said.contains("`.spec.`"), "{said}");
        assert!(said.contains("`.test.`"), "{said}");
        assert!(
            said.contains("containing at least one test case"),
            "an empty spec would satisfy a gate that exists to refuse one: {said}"
        );
    }

    /// A cycle rule is one sentence, and says when a type-only loop is exempt.
    #[test]
    fn a_cycle_rule_says_whether_a_type_only_loop_counts() {
        let counted = sentences(
            &config(vec![rule(
                "no-loops",
                None,
                &["src/*"],
                CompiledRuleKind::ImportCycle {
                    include_type_only: true,
                },
            )]),
            None,
            &[],
        );
        assert!(
            counted.contains("must not sit on an import cycle"),
            "{counted}"
        );
        assert!(!counted.contains("is exempt"), "{counted}");

        let exempt = sentences(
            &config(vec![rule(
                "no-loops",
                None,
                &["src/*"],
                CompiledRuleKind::ImportCycle {
                    include_type_only: false,
                },
            )]),
            None,
            &[],
        );
        assert!(
            exempt.contains("a loop made only of `import type` is exempt"),
            "{exempt}"
        );
    }

    /// A naming rule that constrains the directory as well says so, because a
    /// file written at the right name in the wrong folder still fails.
    #[test]
    fn a_naming_rule_with_a_directory_pattern_names_it() {
        let said = sentences(
            &config(vec![rule(
                "usecase-name",
                None,
                &["src/*"],
                CompiledRuleKind::Naming {
                    file_pattern: Pattern::compile(r"^(?<name>[a-z-]+)\.ts$").expect("valid"),
                    dir_pattern: Some(Pattern::compile("^use-cases$").expect("valid")),
                    name_template: "{{pascal(name)}}".to_owned(),
                    kind: KindFilter::Any,
                    annotation: Vec::new(),
                    signature_hint: None,
                },
            )]),
            None,
            &[],
        );

        assert!(
            said.contains("in a directory matching `^use-cases$`"),
            "{said}"
        );
    }

    /// A frontmatter rule lists the keys it requires, beside the vocabularies.
    #[test]
    fn a_frontmatter_rule_lists_the_keys_it_requires() {
        let said = sentences(
            &config(vec![rule(
                "doc-shape",
                None,
                &["docs/*"],
                CompiledRuleKind::Frontmatter {
                    file_pattern: Pattern::compile(r"\.md$").expect("valid"),
                    require: vec!["title".to_owned(), "status".to_owned()],
                    one_of: Vec::new(),
                    equals: Vec::new(),
                },
            )]),
            None,
            &[],
        );

        assert!(said.contains("carrying: `title`, `status`"), "{said}");
    }

    /// A presence rule states both halves: the files that must be there by
    /// name, and the pattern at least one file has to match.
    #[test]
    fn a_presence_rule_states_both_halves() {
        let said = sentences(
            &config(vec![rule(
                "licao-completa",
                None,
                &["projetos/*"],
                CompiledRuleKind::Presence {
                    require: vec!["projeto.md".to_owned()],
                    require_any: vec![Pattern::compile(r"\.ino$").expect("valid")],
                },
            )]),
            None,
            &[],
        );

        assert!(said.contains("must contain: `projeto.md`"), "{said}");
        assert!(
            said.contains("at least one file matching: `\\.ino$`"),
            "{said}"
        );
    }
}
