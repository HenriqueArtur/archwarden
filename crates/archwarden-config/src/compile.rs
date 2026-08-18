//! Lowering a merged config into compiled rules.
//!
//! This is where "your config might be valid" becomes "your config is valid".
//! Every glob is built, every regex is compiled, every template is checked
//! against the capture groups its pattern actually defines. What comes out
//! cannot be invalid, so no rule engine ever re-checks any of it.

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRule, CompiledRuleKind, SkipDirs, SkipScope},
    facts::{ExportKind, ExportTags, KindFilter},
    glob::{GlobError, PathSet},
    hash::ContentHash,
    ids::RuleId,
    pattern::{Pattern, PatternError},
    scope::{Scope, ScopeError},
    template,
};

use crate::{
    config::{self, Config},
    extends::MergedConfig,
    rule::{MustExport, Rule, SpecPairRule},
};

/// The spelling `must_export.kind` uses for "any declaration form".
const KIND_ANY: &str = "any";

/// Why a config could not be compiled.
///
/// Every variant names the rule, because a config has many and a message that
/// does not say which one leaves the user searching.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CompileError {
    /// A rule takes the id `governance`, which findings about ungoverned
    /// files already report under.
    ///
    /// Refused rather than resolved, because the two would be indistinguishable
    /// in `arch.baseline.json` — which keys on rule id and path — so accepting
    /// one would silently accept the other.
    #[error(
        "rule `{rule}` takes the id `governance`, which `governance: closed` \
         already reports under; a baseline could not tell the two apart"
    )]
    ReservedRuleId {
        /// The rule.
        rule: RuleId,
    },

    /// A rule names a module the config never declared.
    #[error("rule `{rule}` names module `{module}`, which this config does not declare")]
    UnknownModule {
        /// The rule.
        rule: RuleId,
        /// The name it used.
        module: archwarden_core::ids::ModuleId,
    },

    /// A rule names a decision the config never declared.
    ///
    /// Refused here rather than reported by `config doctor`, on the precedent
    /// [`UnknownModule`](Self::UnknownModule) already sets: a reference to
    /// nothing is a typo, and a typo should fail when the config loads rather
    /// than in a separate command the user may never run. A rule that names
    /// *no* decision is a different thing entirely, and stays valid. Issue
    /// #100.
    #[error("rule `{rule}` names decision `{decision}`, which this config does not declare")]
    UnknownDecision {
        /// The rule.
        rule: RuleId,
        /// The reference that matched nothing.
        decision: archwarden_core::ids::DecisionId,
    },

    /// A decision supersedes one the config never declared.
    #[error("decision `{decision}` supersedes `{superseded}`, which this config does not declare")]
    UnknownSuperseded {
        /// The decision doing the replacing.
        decision: archwarden_core::ids::DecisionId,
        /// The reference that matched nothing.
        superseded: archwarden_core::ids::DecisionId,
    },

    /// A decision another one supersedes says it is something else.
    ///
    /// Refused rather than silently overridden: a field that can contradict
    /// the edge is a field that will, and the omission it protects against --
    /// writing `supersedes` and forgetting to edit the old decision -- is what
    /// disarms `superseded-decision-still-enforced`. Issue #115.
    #[error(
        "decision `{decision}` says it is `{status}`, and `{by}` supersedes it; \
         drop the `status` or drop the supersession"
    )]
    StatusContradictsSupersession {
        /// The decision that was replaced.
        decision: archwarden_core::ids::DecisionId,
        /// What it claimed to be.
        status: &'static str,
        /// What replaced it.
        by: archwarden_core::ids::DecisionId,
    },

    /// Supersession runs in a circle.
    #[error("supersession runs in a circle: {decisions}")]
    SupersessionCycle {
        /// The chain, with the id it returned to on both ends.
        decisions: String,
    },

    /// A rejected option names a rule the config never declared.
    ///
    /// The alternative points at a rule the author already wrote, and a
    /// reference to nothing is a typo -- refused where a rule naming an
    /// undeclared module already is. Issue #114.
    #[error(
        "decision `{decision}` says `{option}` is refused by rule `{rule}`, \
         which this config does not declare"
    )]
    UnknownRefusingRule {
        /// The decision.
        decision: archwarden_core::ids::DecisionId,
        /// The option it rejected.
        option: String,
        /// The rule that does not exist.
        rule: RuleId,
    },

    /// A `metadata` rule asks about a key no comment could ever spell.
    ///
    /// The suppression grammar reaches every key beginning with `allow`
    /// first — `// archwarden-allow: x` is a suppression and never a claim —
    /// so a rule asking for one would report the key absent from every file in
    /// its scope, for ever, with no edit that could satisfy it. Refused where
    /// the config compiles, on [`UnknownDecision`](Self::UnknownDecision)'s
    /// precedent: a rule that cannot be met is a typo, not a style.
    #[error(
        "rule `{rule}` asks for metadata key `{key}`, which no comment can \
         spell: `archwarden-{key}:` reads as an `archwarden-allow` suppression"
    )]
    UnreachableMetadataKey {
        /// The rule.
        rule: RuleId,
        /// The key nothing could declare.
        key: String,
    },

    /// A rule names a module that declared no paths.
    #[error("rule `{rule}` names module `{module}`, which declares no `scope`")]
    ModuleHasNoScope {
        /// The rule.
        rule: RuleId,
        /// The module with nothing to be.
        module: archwarden_core::ids::ModuleId,
    },

    /// A rule quantifies over a kind no module wears.
    #[error("rule `{rule}` is about kind `{kind}`, which no module with a `scope` declares")]
    UnknownKind {
        /// The rule.
        rule: RuleId,
        /// The label nothing wears.
        kind: String,
    },

    /// A rule says what it permits and what it forbids.
    #[error(
        "rule `{rule}` sets `only_import_from` and `{other}`; \
         \"only these, except those\" is two rules"
    )]
    AllowlistAndDenylist {
        /// The rule.
        rule: RuleId,
        /// The field that contradicts the allowlist.
        other: &'static str,
    },

    /// A rule says its scope twice, in two fields.
    #[error("rule `{rule}` sets both `{one}` and `{other}`; use one")]
    ScopeSaidTwice {
        /// The rule.
        rule: RuleId,
        /// The first field.
        one: &'static str,
        /// The second.
        other: &'static str,
    },

    /// A rule says its scope in neither field.
    #[error("rule `{rule}` sets neither `{one}` nor `{other}`; it governs nothing")]
    ScopeMissing {
        /// The rule.
        rule: RuleId,
        /// The first field it could have used.
        one: &'static str,
        /// The second.
        other: &'static str,
    },

    /// A module's scope glob is not valid.
    ///
    /// Named by module rather than by rule, because every rule inside it is
    /// fine and the module is the thing to fix. Issue #74.
    #[error("module `{module}`: {source}")]
    ModuleScope {
        /// The module.
        module: archwarden_core::ids::ModuleId,
        /// What went wrong.
        #[source]
        source: ScopeError,
    },

    /// A rule's scope glob is not valid.
    #[error("rule `{rule}`: {source}")]
    Scope {
        /// The rule.
        rule: RuleId,
        /// What went wrong.
        #[source]
        source: ScopeError,
    },

    /// A glob outside a scope is not valid.
    #[error("rule `{rule}`, field `{field}`: {source}")]
    Glob {
        /// The rule.
        rule: RuleId,
        /// Which field held the glob.
        field: &'static str,
        /// What went wrong.
        #[source]
        source: GlobError,
    },

    /// A filename pattern is not valid, or uses an unsupported construct.
    #[error("rule `{rule}`, field `{field}`: {source}")]
    Pattern {
        /// The rule.
        rule: RuleId,
        /// Which field held the pattern.
        field: &'static str,
        /// What went wrong.
        #[source]
        source: Box<PatternError>,
    },

    /// `must_export.kind` names something that is not a declaration form.
    #[error(
        "rule `{rule}`: `{name}` is not an export kind. \
         Valid kinds are {available}, or `any`."
    )]
    UnknownExportKind {
        /// The rule.
        rule: RuleId,
        /// The name as written.
        name: String,
        /// The valid names.
        available: String,
    },

    /// `must_export` asks for an annotation on a form that cannot carry one.
    ///
    /// A rule with no satisfying input, which is worse than a wrong rule: it
    /// looks exactly like a repository nobody has migrated yet, and every file
    /// under it is reported forever.
    #[error(
        "rule `{rule}`: `annotation` cannot be satisfied by an export declared \
         as {kinds}. Only a binding (`const`, `let`, `var`) or a `class`, \
         through its `implements` clause, writes a type down beside its name; \
         a function declares a *return* type, which is a different claim."
    )]
    UnannotatableKind {
        /// The rule.
        rule: RuleId,
        /// The forms the rule accepts, none of which can be annotated.
        kinds: String,
    },

    /// A `pair.must_exist` is absolute, or empty.
    #[error(
        "rule `{rule}`: `must_exist` is relative to the file that needs the \
         companion, and `{path}` is not a relative path. Write `notas.md`, or \
         `../projeto.md` to reach out of the directory."
    )]
    CompanionNotRelative {
        /// The rule.
        rule: RuleId,
        /// The path as written.
        path: String,
    },

    /// A `pair.must_exist` carries a template placeholder.
    #[error(
        "rule `{rule}`: `must_exist` is literal, and `{path}` reads as a \
         template. A `pair` rule asks for one named companion beside the file, \
         so `{{{{...}}}}` is not substituted here and would be hunted for as \
         part of the name. For a companion whose name varies with the file, \
         `naming.must_export` and `frontmatter.equals` are where templates \
         live; a fixed name is what this field takes."
    )]
    CompanionIsATemplate {
        /// The rule.
        rule: RuleId,
        /// The path as written.
        path: String,
    },

    /// A `presence.require` entry names a path rather than a file.
    #[error(
        "rule `{rule}`: `require` takes filenames, and `{entry}` is a path. \
         One rule answers for one directory, which is what lets `describe` \
         answer for a directory that does not exist yet. Scope a second rule \
         one level down instead."
    )]
    RequireIsAPath {
        /// The rule.
        rule: RuleId,
        /// The entry as written.
        entry: String,
    },

    /// A `spec-pair` `spec_dirs` entry is not a single directory name.
    #[error(
        "rule `{rule}`: `spec_dirs` takes directory names, and `{entry}` is a \
         path. A spec directory is one level beside the file — `__tests__`, not \
         `__tests__/unit` — because a rule that reached further would accept a \
         spec anywhere below and report nothing. Name the deeper directory as \
         its own entry if it is also a spec directory."
    )]
    SpecDirIsAPath {
        /// The rule.
        rule: RuleId,
        /// The entry as written.
        entry: String,
    },

    /// A `spec-pair` marker is not a single filename component.
    #[error(
        "rule `{rule}`: `{marker}` is not a spec marker. A marker is one \
         filename component such as `spec` or `test`; the extension comes \
         from the source file, so `Component.tsx` wants `Component.spec.tsx` \
         without being told."
    )]
    InvalidSpecMarker {
        /// The rule.
        rule: RuleId,
        /// The marker as written.
        marker: String,
    },

    /// A `must_export.name` template refers to a capture group that neither
    /// pattern on the rule defines.
    #[error("rule `{rule}`: {source}")]
    Template {
        /// The rule.
        rule: RuleId,
        /// What went wrong.
        #[source]
        source: template::TemplateError,
    },

    /// `file_pattern` and `dir_pattern` both define the same capture group.
    #[error(
        "rule `{rule}`: capture group `{group}` is defined by both \
         `file_pattern` and `dir_pattern`, so `{{{{...({group})}}}}` in the \
         template has two values and no rule for choosing between them. \
         Rename one of them."
    )]
    DuplicateCaptureGroup {
        /// The rule.
        rule: RuleId,
        /// The group both patterns define.
        group: String,
    },

    /// The top-level `ignore` list holds an invalid glob.
    #[error("`ignore`: {source}")]
    Ignore {
        /// What went wrong.
        #[source]
        source: GlobError,
    },

    /// `skip_dirs.globs` holds an invalid glob.
    #[error("`skip_dirs.globs`: {source}")]
    SkipDirs {
        /// What went wrong.
        #[source]
        source: GlobError,
    },
}

/// Compiles a merged config.
///
/// # Errors
/// See [`CompileError`].
pub fn compile(merged: &MergedConfig) -> Result<CompiledConfig, CompileError> {
    let config = &merged.config;

    let modules = Modules::compile(config)?;
    let decisions = compile_decisions(config)?;

    let mut rules = Vec::new();
    for (module, module_why, rule) in config.rules() {
        // Before anything else about the rule: an id that collides with the
        // one `governance: closed` reports under would be indistinguishable
        // from it in `arch.baseline.json`, which keys on rule and path.
        if rule.id().as_str() == archwarden_core::ids::GOVERNANCE_RULE_ID {
            return Err(CompileError::ReservedRuleId {
                rule: rule.id().clone(),
            });
        }
        // A reference is checked against the declared list here, once, rather
        // than by every surface that later resolves one. That is what lets
        // `CompiledRule::decision` promise it names something.
        if let Some(named) = rule.decision()
            && !decisions.iter().any(|decision| &decision.id == named)
        {
            return Err(CompileError::UnknownDecision {
                rule: rule.id().clone(),
                decision: named.clone(),
            });
        }

        rules.push(compile_rule(
            rule,
            module.cloned(),
            module_why.map(ToOwned::to_owned),
            &modules,
            module,
        )?);
    }

    let ignore =
        PathSet::compile(&config.ignore).map_err(|source| CompileError::Ignore { source })?;

    let skip_dirs = SkipDirs {
        prefixes: config.skip_dirs.prefixes.clone(),
        globs: PathSet::compile(&config.skip_dirs.globs)
            .map_err(|source| CompileError::SkipDirs { source })?,
        scope: match config.skip_dirs.scope {
            config::SkipScope::Structure => SkipScope::Structure,
            config::SkipScope::Walk => SkipScope::Walk,
        },
    };

    Ok(
        CompiledConfig::new(rules, ignore, skip_dirs, rules_hash(config))
            .with_modules(modules.compiled())
            .with_decisions(decisions)
            .with_languages(archwarden_core::compiled::Languages {
                astro: config.languages.contains(&config::Language::Astro),
            })
            .with_governance(config.governance.level()),
    )
}

/// Lowers the declared decisions.
///
/// Was infallible while a decision was only prose. It now carries two
/// references — what it supersedes and what refuses each rejected option — and
/// a dangling one is a typo, refused here for the reason every other dangling
/// reference is. Issues #114 and #115.
fn compile_decisions(
    config: &Config,
) -> Result<Vec<archwarden_core::compiled::CompiledDecision>, CompileError> {
    let declared: std::collections::BTreeSet<&str> = config
        .decisions
        .iter()
        .map(|decision| decision.id.as_str())
        .collect();
    let rules: std::collections::BTreeSet<&str> = config
        .rules()
        .map(|(_, _, rule)| rule.id().as_str())
        .collect();

    // Who replaced whom, so the reverse can be read without a second list and
    // so a decision's status can be told from the edge rather than repeated.
    let mut superseded_by: std::collections::BTreeMap<&str, Vec<archwarden_core::ids::DecisionId>> =
        std::collections::BTreeMap::new();
    for decision in &config.decisions {
        for replaced in &decision.supersedes {
            if !declared.contains(replaced.as_str()) {
                return Err(CompileError::UnknownSuperseded {
                    decision: decision.id.clone(),
                    superseded: replaced.clone(),
                });
            }
            superseded_by
                .entry(replaced.as_str())
                .or_default()
                .push(decision.id.clone());
        }
    }
    refuse_supersession_cycles(config)?;

    config
        .decisions
        .iter()
        .map(|decision| {
            let replaced_by = superseded_by
                .get(decision.id.as_str())
                .cloned()
                .unwrap_or_default();

            Ok(archwarden_core::compiled::CompiledDecision {
                id: decision.id.clone(),
                title: decision.title.clone(),
                why: decision.why.clone(),
                link: decision.link.clone(),
                status: status_of(decision, replaced_by.first())?,
                supersedes: decision.supersedes.iter().cloned().collect(),
                superseded_by: replaced_by,
                alternatives: decision
                    .alternatives
                    .iter()
                    .map(|alternative| {
                        if let Some(rule) = &alternative.refused_by
                            && !rules.contains(rule.as_str())
                        {
                            return Err(CompileError::UnknownRefusingRule {
                                decision: decision.id.clone(),
                                option: alternative.option.clone(),
                                rule: rule.clone(),
                            });
                        }
                        Ok(archwarden_core::compiled::CompiledAlternative {
                            option: alternative.option.clone(),
                            why_not: alternative.why_not.clone(),
                            refused_by: alternative.refused_by.clone(),
                        })
                    })
                    .collect::<Result<_, CompileError>>()?,
            })
        })
        .collect()
}

/// A decision's status, with supersession deciding it when there is any.
///
/// Inferred rather than repeated: somebody who writes `supersedes` and forgets
/// to edit the old decision leaves a config that says two things, and disarms
/// `superseded-decision-still-enforced` — which is the check with the most
/// value here. Saying it out loud agrees with the edge and is allowed; saying
/// the opposite is refused, not silently overridden.
fn status_of(
    decision: &config::Decision,
    replaced_by: Option<&archwarden_core::ids::DecisionId>,
) -> Result<archwarden_core::compiled::DecisionStatus, CompileError> {
    let written = decision.status.map(|status| match status {
        config::DecisionStatus::Accepted => archwarden_core::compiled::DecisionStatus::Accepted,
        config::DecisionStatus::Proposed => archwarden_core::compiled::DecisionStatus::Proposed,
        config::DecisionStatus::Superseded => archwarden_core::compiled::DecisionStatus::Superseded,
    });

    let Some(by) = replaced_by else {
        return Ok(written.unwrap_or(archwarden_core::compiled::DecisionStatus::Accepted));
    };

    match written {
        None | Some(archwarden_core::compiled::DecisionStatus::Superseded) => {
            Ok(archwarden_core::compiled::DecisionStatus::Superseded)
        }
        Some(other) => Err(CompileError::StatusContradictsSupersession {
            decision: decision.id.clone(),
            status: other.as_str(),
            by: by.clone(),
        }),
    }
}

/// Refuses a decision that replaces itself, directly or around a loop.
///
/// A cycle leaves a chain with no end, and every surface that draws one would
/// walk it forever. Reported with the ids in it, because "there is a cycle" is
/// a sentence nobody can act on.
fn refuse_supersession_cycles(config: &Config) -> Result<(), CompileError> {
    let edges: std::collections::BTreeMap<&str, Vec<&str>> = config
        .decisions
        .iter()
        .map(|decision| {
            (
                decision.id.as_str(),
                decision
                    .supersedes
                    .iter()
                    .map(archwarden_core::ids::DecisionId::as_str)
                    .collect(),
            )
        })
        .collect();

    for decision in &config.decisions {
        let start = decision.id.as_str();
        let mut walked = vec![start];
        let mut seen: std::collections::BTreeSet<&str> = [start].into_iter().collect();
        let mut frontier = edges.get(start).cloned().unwrap_or_default();

        while let Some(next) = frontier.pop() {
            if next == start {
                walked.push(start);
                return Err(CompileError::SupersessionCycle {
                    decisions: walked.join(" → "),
                });
            }
            if seen.insert(next) {
                walked.push(next);
                frontier.extend(edges.get(next).cloned().unwrap_or_default());
            }
        }
    }

    Ok(())
}

/// The paths a boundary forbids, from whichever field it used.
///
/// `forbid_module` and `forbid_import_from` are refused together, on the same
/// argument as `from` and `from_module`: one rule, one way of saying what it
/// is about. Neither is fine — a boundary may forbid nothing and require
/// something instead, which `must_import_from` is for.
fn forbidden_paths(
    id: &RuleId,
    rule: &crate::rule::ImportBoundaryRule,
    modules: &Modules,
) -> Result<PathSet, CompileError> {
    if !rule.forbid_module.is_empty() && !rule.forbid_import_from.is_empty() {
        return Err(CompileError::ScopeSaidTwice {
            rule: id.clone(),
            one: "forbid_import_from",
            other: "forbid_module",
        });
    }

    if rule.forbid_module.is_empty() {
        return globs(id, "forbid_import_from", &rule.forbid_import_from);
    }

    let mut patterns: Vec<String> = Vec::new();
    for named in &rule.forbid_module {
        patterns.extend(modules.paths_of(id, named)?.iter().cloned());
    }
    PathSet::compile(&patterns).map_err(|source| CompileError::Glob {
        rule: id.clone(),
        field: "forbid_module",
        source,
    })
}

/// The paths a boundary rule refuses to let its files *end up* depending on.
///
/// The same shape as [`forbidden_paths`], and the same refusal when both the
/// globs and the modules are given: two ways to fill one set means an author
/// has to be told which one won, and a rule nobody can predict is worse than a
/// rule that will not compile.
fn reaching_paths(
    id: &RuleId,
    rule: &crate::rule::ImportBoundaryRule,
    modules: &Modules,
) -> Result<PathSet, CompileError> {
    if !rule.forbid_reaching_modules.is_empty() && !rule.forbid_reaching.is_empty() {
        return Err(CompileError::ScopeSaidTwice {
            rule: id.clone(),
            one: "forbid_reaching",
            other: "forbid_reaching_modules",
        });
    }

    if rule.forbid_reaching_modules.is_empty() {
        return globs(id, "forbid_reaching", &rule.forbid_reaching);
    }

    let mut patterns: Vec<String> = Vec::new();
    for named in &rule.forbid_reaching_modules {
        patterns.extend(modules.paths_of(id, named)?.iter().cloned());
    }
    PathSet::compile(&patterns).map_err(|source| CompileError::Glob {
        rule: id.clone(),
        field: "forbid_reaching_modules",
        source,
    })
}

/// The groups a boundary's importers fall into, one per module it covers.
///
/// One group for a rule about one module or one set of globs, and one *per
/// module* for a rule about a kind. That distinction is the whole of the
/// self-import question: an assembly may import its own files and not its
/// siblings', and only per-module groups can tell those apart.
fn importer_groups(
    id: &RuleId,
    rule: &crate::rule::ImportBoundaryRule,
    modules: &Modules,
) -> Result<Vec<PathSet>, CompileError> {
    let Some(kind) = &rule.from_kind else {
        return Ok(Vec::new());
    };

    let mut groups = Vec::new();
    for (module, worn) in &modules.kinds {
        if worn != kind {
            continue;
        }
        let paths = modules.paths_of(id, module)?;
        groups.push(
            PathSet::compile(paths).map_err(|source| CompileError::Glob {
                rule: id.clone(),
                field: "from_kind",
                source,
            })?,
        );
    }
    Ok(groups)
}

/// The paths a boundary permits, when it works that way at all.
///
/// `None` when neither allowlist field is set, and that is not the same as an
/// empty set: empty would mean "nothing in this repository may be imported",
/// which is a far louder statement than "this rule does not work by allowlist".
///
/// Refused alongside `forbid_import_from`: "only these, except those" reads as
/// one sentence and is two rules, and two rules is what a reader can follow.
/// `except` is refused too — it shields against a prohibition, and an
/// exception to a *permission* is as meaningless as `RULES.md` already says an
/// exception to a requirement is.
fn permitted_paths(
    id: &RuleId,
    rule: &crate::rule::ImportBoundaryRule,
    modules: &Modules,
) -> Result<Option<PathSet>, CompileError> {
    let by_glob = !rule.only_import_from.is_empty();
    let by_module = !rule.only_import_from_modules.is_empty();
    let by_kind = !rule.only_import_from_kinds.is_empty();

    if by_kind {
        if by_glob || by_module {
            return Err(CompileError::ScopeSaidTwice {
                rule: id.clone(),
                one: "only_import_from_kinds",
                other: "only_import_from",
            });
        }
        let mut patterns = Vec::new();
        for kind in &rule.only_import_from_kinds {
            patterns.extend(modules.paths_of_kind(id, kind)?);
        }
        return PathSet::compile(&patterns)
            .map(Some)
            .map_err(|source| CompileError::Glob {
                rule: id.clone(),
                field: "only_import_from_kinds",
                source,
            });
    }

    if by_glob && by_module {
        return Err(CompileError::ScopeSaidTwice {
            rule: id.clone(),
            one: "only_import_from",
            other: "only_import_from_modules",
        });
    }
    if !by_glob && !by_module {
        return Ok(None);
    }
    if !rule.forbid_import_from.is_empty() || !rule.forbid_module.is_empty() {
        return Err(CompileError::AllowlistAndDenylist {
            rule: id.clone(),
            other: "forbid_import_from",
        });
    }
    if !rule.except.is_empty() {
        return Err(CompileError::AllowlistAndDenylist {
            rule: id.clone(),
            other: "except",
        });
    }

    let patterns: Vec<String> = if by_module {
        let mut collected = Vec::new();
        for named in &rule.only_import_from_modules {
            collected.extend(modules.paths_of(id, named)?.iter().cloned());
        }
        collected
    } else {
        rule.only_import_from.iter().cloned().collect()
    };

    PathSet::compile(&patterns)
        .map(Some)
        .map_err(|source| CompileError::Glob {
            rule: id.clone(),
            field: "only_import_from",
            source,
        })
}

/// A rule's scope, from whichever field it used.
///
/// A boundary may say who it is about as globs (`from`) or as a module
/// (`from_module`), and exactly one of those is required. Both is refused
/// rather than resolved: two spellings of one scope on one rule is the
/// ambiguity that produces a rule enforcing something nobody meant, and unlike
/// glob containment this one is decidable at compile time.
fn compile_scope(
    rule: &Rule,
    id: &RuleId,
    modules: &Modules,
    inside: Option<&archwarden_core::ids::ModuleId>,
) -> Result<Scope, CompileError> {
    let own = if let Rule::ImportBoundary(boundary) = rule {
        // A kind selects every module that wears it, which is the whole point:
        // the seventh assembly is governed because it exists, not because
        // somebody remembered. Issue #76.
        if let Some(kind) = &boundary.from_kind {
            if !boundary.from.is_empty() || boundary.from_module.is_some() {
                return Err(CompileError::ScopeSaidTwice {
                    rule: id.clone(),
                    one: "from_kind",
                    other: "from",
                });
            }
            let patterns = modules.paths_of_kind(id, kind)?;
            return Scope::compile(&patterns)
                .map_err(|source| CompileError::Scope {
                    rule: id.clone(),
                    source,
                })
                .map(|own| {
                    inside
                        .and_then(|m| modules.scopes.get(m))
                        .map_or(own.clone(), |outer| own.within(outer))
                });
        }
        match (boundary.from.is_empty(), boundary.from_module.as_ref()) {
            (false, Some(_)) => {
                return Err(CompileError::ScopeSaidTwice {
                    rule: id.clone(),
                    one: "from",
                    other: "from_module",
                });
            }
            (true, None) => {
                return Err(CompileError::ScopeMissing {
                    rule: id.clone(),
                    one: "from",
                    other: "from_module",
                });
            }
            (true, Some(named)) => Scope::compile(modules.paths_of(id, named)?),
            (false, None) => Scope::compile(rule.scope()),
        }
    } else {
        Scope::compile(rule.scope())
    }
    .map_err(|source| CompileError::Scope {
        rule: id.clone(),
        source,
    })?;

    // Narrowed, never replaced: a rule keeps its own scope and reaches where
    // the module it lives in also reaches. See `Scope::within` for why this is
    // not a refusal.
    Ok(inside
        .and_then(|id| modules.scopes.get(id))
        .map_or(own.clone(), |outer| own.within(outer)))
}

/// Hashes the effective rule set, for the `findings` cache key.
///
/// Derived from the merged config's serialised rules rather than from the
/// files on disk, so a preset reshuffle that produces the same rules does not
/// invalidate the cache, while any real change to a rule does.
fn rules_hash(config: &Config) -> ContentHash {
    let rules: Vec<_> = config.rules().collect();
    let serialised = serde_json::to_vec(&rules).unwrap_or_default();
    ContentHash::of(&serialised)
}

/// The modules a config declares that have paths of their own.
///
/// Compiled once and consulted by every rule, rather than once per rule: a
/// module of nine rules would otherwise build the same globs nine times, and
/// a boundary naming a module needs one it does not live in.
struct Modules {
    /// Scope by id, for narrowing a rule that lives inside one.
    scopes: std::collections::BTreeMap<archwarden_core::ids::ModuleId, Scope>,
    /// The patterns as written, for a rule that names a module and needs its
    /// paths as a `PathSet` rather than as a scope.
    patterns: std::collections::BTreeMap<archwarden_core::ids::ModuleId, Vec<String>>,
    /// Every id the config declares, including those with no scope: naming one
    /// of those is a different mistake from naming one that does not exist,
    /// and the two deserve different sentences.
    declared: std::collections::BTreeSet<archwarden_core::ids::ModuleId>,
    /// What sort each module said it is, for rules that quantify over sorts.
    kinds: std::collections::BTreeMap<archwarden_core::ids::ModuleId, String>,
}

impl Modules {
    fn compile(config: &Config) -> Result<Self, CompileError> {
        let mut scopes = std::collections::BTreeMap::new();
        let mut patterns = std::collections::BTreeMap::new();
        let mut declared = std::collections::BTreeSet::new();
        let mut kinds = std::collections::BTreeMap::new();

        for module in &config.modules {
            declared.insert(module.id.clone());
            if let Some(kind) = &module.kind {
                kinds.insert(module.id.clone(), kind.clone());
            }
            if module.scope.is_empty() {
                continue;
            }
            let scope =
                Scope::compile(&module.scope).map_err(|source| CompileError::ModuleScope {
                    module: module.id.clone(),
                    source,
                })?;
            scopes.insert(module.id.clone(), scope);
            patterns.insert(
                module.id.clone(),
                module.scope.iter().map(ToOwned::to_owned).collect(),
            );
        }

        Ok(Self {
            scopes,
            patterns,
            declared,
            kinds,
        })
    }

    /// The paths every module of this sort is.
    ///
    /// A kind nothing wears is refused rather than compiled into a scope that
    /// selects nothing: a rule quantifying over an empty set governs nothing,
    /// silently, which is the failure the quantifier exists to remove.
    fn paths_of_kind(&self, rule: &RuleId, kind: &str) -> Result<Vec<String>, CompileError> {
        let mut collected = Vec::new();
        for (id, worn) in &self.kinds {
            if worn != kind {
                continue;
            }
            collected.extend(self.paths_of(rule, id)?.iter().cloned());
        }

        if collected.is_empty() {
            return Err(CompileError::UnknownKind {
                rule: rule.clone(),
                kind: kind.to_owned(),
            });
        }
        Ok(collected)
    }

    /// The modules, as the rest of the run sees them.
    fn compiled(&self) -> Vec<archwarden_core::compiled::CompiledModule> {
        self.declared
            .iter()
            .map(|id| archwarden_core::compiled::CompiledModule {
                id: id.clone(),
                scope: self.scopes.get(id).cloned(),
                kind: self.kinds.get(id).cloned(),
            })
            .collect()
    }

    /// The paths a named module is, or why it cannot answer.
    fn paths_of(
        &self,
        rule: &RuleId,
        module: &archwarden_core::ids::ModuleId,
    ) -> Result<&[String], CompileError> {
        if !self.declared.contains(module) {
            return Err(CompileError::UnknownModule {
                rule: rule.clone(),
                module: module.clone(),
            });
        }
        self.patterns
            .get(module)
            .map(Vec::as_slice)
            .ok_or_else(|| CompileError::ModuleHasNoScope {
                rule: rule.clone(),
                module: module.clone(),
            })
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one arm per rule kind, each a literal. Splitting it would put the \
              arms somewhere the exhaustive match no longer names them, which \
              is what makes a kind added without lowering fail to build"
)]
fn compile_rule(
    rule: &Rule,
    module: Option<archwarden_core::ids::ModuleId>,
    module_why: Option<String>,
    modules: &Modules,
    inside: Option<&archwarden_core::ids::ModuleId>,
) -> Result<CompiledRule, CompileError> {
    let id = rule.id().clone();

    let scope = compile_scope(rule, &id, modules, inside)?;

    let kind = match rule {
        Rule::Structure(r) => CompiledRuleKind::Structure {
            allowed_subfolders: r.allowed_subfolders.clone(),
            warn_subfolders: r.warn_subfolders.clone(),
            recurse_into: r.recurse_into.clone(),
            subfolder_patterns: r
                .subfolder_patterns
                .iter()
                .map(|p| pattern(&id, "subfolder_patterns", p))
                .collect::<Result<_, _>>()?,
            filename_patterns: r
                .filename_patterns
                .iter()
                .map(|p| pattern(&id, "filename_patterns", p))
                .collect::<Result<_, _>>()?,
        },

        Rule::Naming(r) => {
            let file_pattern = pattern(&id, "file_pattern", &r.file_pattern)?;
            let dir_pattern = r
                .dir_pattern
                .as_deref()
                .map(|source| pattern(&id, "dir_pattern", source))
                .transpose()?;
            check_template(&id, &file_pattern, dir_pattern.as_ref(), &r.must_export)?;

            let kind = export_kind(&id, &r.must_export)?;
            let annotation = annotation(&id, &kind, &r.must_export)?;

            CompiledRuleKind::Naming {
                kind,
                name_template: r.must_export.name.clone(),
                annotation,
                signature_hint: r.must_export.signature_hint.clone(),
                file_pattern,
                dir_pattern,
            }
        }

        Rule::SpecPair(r) => CompiledRuleKind::SpecPair {
            subfolders: r.subfolders.iter().cloned().collect(),
            spec_markers: spec_markers(&id, r)?,
            ignore_files: globs(&id, "ignore_files", &r.ignore_files)?,
            spec_dirs: spec_dirs(&id, r)?,
            require_non_empty_spec: r.require_non_empty_spec,
            skip_type_only: r.skip_type_only,
        },

        Rule::NoPassthrough(r) => CompiledRuleKind::NoPassthrough {
            forms: archwarden_core::compiled::PassthroughForms {
                reexport: r.forms.contains(&crate::rule::PassthroughForm::Reexport),
                alias: r.forms.contains(&crate::rule::PassthroughForm::Alias),
                wrapper: r.forms.contains(&crate::rule::PassthroughForm::Wrapper),
            },
            except: globs(&id, "except", &r.except)?,
            allow_package_entrypoints: r.allow_package_entrypoints,
            allow_partial: r.allow_partial,
        },

        Rule::ImportCycle(r) => CompiledRuleKind::ImportCycle {
            include_type_only: r.include_type_only,
        },

        Rule::ImportBoundary(r) => CompiledRuleKind::ImportBoundary {
            // A named module becomes its paths here, so nothing downstream
            // knows the difference: the engine sees the `PathSet` it always
            // saw, and the config says `infrastructure` instead of repeating
            // that module's globs. Issue #74.
            forbid: forbidden_paths(&id, r, modules)?,
            allow: permitted_paths(&id, r, modules)?,
            groups: importer_groups(&id, r, modules)?,
            allow_packages: (!r.only_import_from_packages.is_empty())
                .then(|| r.only_import_from_packages.iter().cloned().collect()),
            require: globs(&id, "must_import_from", &r.must_import_from)?,
            forbid_packages: r.forbid_import_from_packages.iter().cloned().collect(),
            forbid_reaching: reaching_paths(&id, r, modules)?,
            except: globs(&id, "except", &r.except)?,
            except_from: globs(&id, "except_from", &r.except_from)?,
            include_type_only: r.include_type_only,
        },

        Rule::Presence(r) => CompiledRuleKind::Presence {
            require: r
                .require
                .iter()
                .map(|name| require_name(&id, name))
                .collect::<Result<_, _>>()?,
            require_any: r
                .require_any
                .iter()
                .map(|p| pattern(&id, "require_any", p))
                .collect::<Result<_, _>>()?,
        },

        Rule::Pair(r) => CompiledRuleKind::Pair {
            file_pattern: pattern(&id, "file_pattern", &r.file_pattern)?,
            must_exist: companion(&id, &r.must_exist)?,
        },

        Rule::Frontmatter(r) => CompiledRuleKind::Frontmatter {
            file_pattern: pattern(&id, "file_pattern", &r.file_pattern)?,
            require: r.require.iter().cloned().collect(),
            one_of: r
                .one_of
                .iter()
                .map(|(key, values)| (key.clone(), values.iter().cloned().collect()))
                .collect(),
            equals: r
                .equals
                .iter()
                .map(|(key, template)| {
                    check_document_template(&id, template)?;
                    Ok((key.clone(), template.clone()))
                })
                .collect::<Result<_, CompileError>>()?,
        },

        Rule::Metadata(r) => CompiledRuleKind::Metadata {
            require: r
                .require
                .iter()
                .map(|key| reachable_key(&id, key))
                .collect::<Result<_, CompileError>>()?,
            one_of: r
                .one_of
                .iter()
                .map(|(key, values)| {
                    Ok((
                        reachable_key(&id, key)?,
                        values.iter().cloned().collect::<Vec<_>>(),
                    ))
                })
                .collect::<Result<_, CompileError>>()?,
            equals: r
                .equals
                .iter()
                .map(|(key, template)| {
                    check_document_template(&id, template)?;
                    Ok((reachable_key(&id, key)?, template.clone()))
                })
                .collect::<Result<_, CompileError>>()?,
        },

        Rule::Frozen(_) => CompiledRuleKind::Frozen,

        Rule::Mirror(r) => CompiledRuleKind::Mirror {
            file_pattern: pattern(&id, "file_pattern", &r.file_pattern)?,
            must_exist: r.must_exist.clone(),
        },

        Rule::ExportShape(r) => {
            CompiledRuleKind::ExportShape(archwarden_core::compiled::ExportShape {
                forbid_default: r.forbid_default,
                max_exports: r.max_exports,
                must_return: r
                    .must_return
                    .iter()
                    .map(|p| pattern(&id, "must_return", p))
                    .collect::<Result<_, _>>()?,
            })
        }

        Rule::CallObligation(r) => CompiledRuleKind::CallObligation {
            file_pattern: pattern(&id, "file_pattern", &r.file_pattern)?,
            symbol: r.must_call.symbol.clone(),
            imported_from: r.must_call.imported_from.clone(),
        },
    };

    // The second axis, and only when the rule asks for it. `None` rather than an
    // empty filter: "does not narrow" and "narrows to nothing" are different
    // statements, and only one of them should cost a resolution pass.
    let imports = import_filter(&id, rule)?;

    Ok(CompiledRule {
        id,
        module,
        imports,
        why: rule.why().map(ToOwned::to_owned),
        module_why,
        decision: rule.decision().cloned(),
        level: rule.level(),
        scope,
        kind,
    })
}

/// Compiles a rule's import filter, when it has one.
///
/// Decision 25. Globs are matched against the resolved path, so they are built
/// the same way a boundary's are — the alternative would be a second glob
/// dialect for the same job, and two dialects eventually disagree.
fn import_filter(
    id: &RuleId,
    rule: &Rule,
) -> Result<Option<archwarden_core::compiled::ImportFilter>, CompileError> {
    let paths = rule.when_importing();
    let packages = rule.when_importing_packages();

    if paths.is_empty() && packages.is_empty() {
        return Ok(None);
    }

    Ok(Some(archwarden_core::compiled::ImportFilter {
        paths: archwarden_core::glob::PathSet::compile(paths.as_slice().iter().cloned()).map_err(
            |source| CompileError::Glob {
                rule: id.clone(),
                field: "when_importing",
                source,
            },
        )?,
        packages: packages.to_vec(),
    }))
}

/// A metadata key, refused if no comment could spell it.
///
/// Asked of the fact grammar itself rather than of a list of reserved words
/// kept here, so the two can never drift: whatever the suppression parser
/// accepts is exactly what this refuses.
fn reachable_key(rule: &RuleId, key: &str) -> Result<String, CompileError> {
    if archwarden_core::facts::MetadataFact::key_is_reachable(key) {
        return Ok(key.to_owned());
    }

    Err(CompileError::UnreachableMetadataKey {
        rule: rule.clone(),
        key: key.to_owned(),
    })
}

/// The only group a document template may name.
///
/// A `naming` template renders from the capture groups of a `file_pattern`; a
/// document has one thing worth agreeing with, and it is the directory it sits
/// in. Refused rather than rendered empty, because a template naming a group
/// nobody defines is a rule that would quietly demand the wrong value.
const DOCUMENT_GROUP: &str = "dirname";

fn check_document_template(rule: &RuleId, source: &str) -> Result<(), CompileError> {
    template::render(source, |group| {
        (group == DOCUMENT_GROUP).then(|| "placeholder".to_owned())
    })
    .map(|_| ())
    .map_err(|source| CompileError::Template {
        rule: rule.clone(),
        source,
    })
}

/// A `must_exist` path, refused if it is absolute or empty.
///
/// Relative, always: the file the rule is about is the anchor, and an absolute
/// path would make the rule say the same thing from every directory it covers
/// -- which is a `presence` rule scoped there, written the confusing way.
fn companion(rule: &RuleId, path: &str) -> Result<String, CompileError> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err(CompileError::CompanionNotRelative {
            rule: rule.clone(),
            path: path.to_owned(),
        });
    }

    // Literal means literal, and saying so in the docs was not enough. The
    // template form is the one `naming.must_export` and `frontmatter.equals`
    // accept, so reaching for it here is the obvious mistake -- and it used to
    // compile, run, and report every governed file as missing a companion with
    // braces in its name. Sixteen confident findings about a file nothing could
    // create is worse than the rule not existing.
    if trimmed.contains("{{") {
        return Err(CompileError::CompanionIsATemplate {
            rule: rule.clone(),
            path: path.to_owned(),
        });
    }

    Ok(trimmed.to_owned())
}

/// A `require` entry, refused if it is a path rather than a name.
///
/// A rule answers for one directory's contract, which is what lets `describe`
/// answer for a directory that does not exist yet. An entry reaching into a
/// subdirectory would make one rule answer for two, and the same requirement
/// is already sayable by a second rule scoped one level down -- so this is a
/// redirection, not a limitation.
fn require_name(rule: &RuleId, name: &str) -> Result<String, CompileError> {
    if name.contains('/') || name.contains('\\') {
        return Err(CompileError::RequireIsAPath {
            rule: rule.clone(),
            entry: name.to_owned(),
        });
    }

    Ok(name.to_owned())
}

fn pattern(rule: &RuleId, field: &'static str, source: &str) -> Result<Pattern, CompileError> {
    Pattern::compile(source).map_err(|error| CompileError::Pattern {
        rule: rule.clone(),
        field,
        source: Box::new(error),
    })
}

fn globs<'a, I>(rule: &RuleId, field: &'static str, patterns: I) -> Result<PathSet, CompileError>
where
    I: IntoIterator<Item = &'a String>,
{
    PathSet::compile(patterns).map_err(|source| CompileError::Glob {
        rule: rule.clone(),
        field,
        source,
    })
}

/// Validates the `spec-pair` markers.
///
/// A marker is one filename component -- `spec`, `test` -- and the extension
/// is taken from the source file. A marker carrying a dot or an extension is
/// almost always someone writing the old whole-suffix form, and guessing what
/// A `spec_dirs` entry, refused if it is a path rather than a directory name.
///
/// The rule reaches one level: a spec at `<dir>/<named>/x.spec.ts` counts and
/// `<dir>/<named>/unit/x.spec.ts` does not. An entry with a separator asks for
/// the second, and accepting it silently would make the rule reach further
/// than it says — which is how a `spec-pair` rule stops reporting and starts
/// looking like a repository that is fully tested.
fn spec_dirs(rule: &RuleId, spec: &SpecPairRule) -> Result<Vec<String>, CompileError> {
    let mut names = Vec::new();
    for entry in &spec.spec_dirs {
        let trimmed = entry.trim();
        if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains('\\') {
            return Err(CompileError::SpecDirIsAPath {
                rule: rule.clone(),
                entry: entry.clone(),
            });
        }
        names.push(trimmed.to_owned());
    }
    Ok(names)
}

/// they meant would be worse than saying so.
fn spec_markers(rule: &RuleId, spec: &SpecPairRule) -> Result<Vec<String>, CompileError> {
    let mut markers = Vec::new();

    for marker in &spec.spec_markers {
        let trimmed = marker.trim_start_matches('.');
        if trimmed.is_empty() || trimmed.contains('.') {
            return Err(CompileError::InvalidSpecMarker {
                rule: rule.clone(),
                marker: marker.clone(),
            });
        }
        markers.push(trimmed.to_owned());
    }

    Ok(markers)
}

fn export_kind(rule: &RuleId, must_export: &MustExport) -> Result<KindFilter, CompileError> {
    let mut tags = ExportTags::none();

    for name in &must_export.kind {
        if name == KIND_ANY {
            return Ok(KindFilter::Any);
        }

        let kind = ExportKind::parse(name).ok_or_else(|| CompileError::UnknownExportKind {
            rule: rule.clone(),
            name: name.clone(),
            available: ExportKind::ALL.map(ExportKind::as_str).join(", "),
        })?;

        tags = tags.with(kind);
    }

    Ok(KindFilter::OneOf(tags))
}

/// The forms that have somewhere to write a type down beside the name.
///
/// A binding takes an annotation after the colon; a class names its contracts
/// in `implements`. A function has a *return* type, an interface and a type
/// alias *are* the type, an enum declares one, and a re-export's declaration is
/// in another file — none of those is a place this rule could read.
const ANNOTATABLE: [ExportKind; 5] = [
    ExportKind::Const,
    ExportKind::Let,
    ExportKind::Var,
    ExportKind::Arrow,
    ExportKind::Class,
];

/// The required annotations, refusing a rule no file could satisfy.
///
/// `kind: "any"` passes: it accepts the annotatable forms among everything
/// else, so a file that satisfies the rule exists.
fn annotation(
    rule: &RuleId,
    kind: &KindFilter,
    must_export: &MustExport,
) -> Result<Vec<String>, CompileError> {
    let Some(annotation) = must_export.annotation.as_ref() else {
        return Ok(Vec::new());
    };

    if !ANNOTATABLE
        .iter()
        .any(|form| kind.accepts(ExportTags::only(*form)))
    {
        return Err(CompileError::UnannotatableKind {
            rule: rule.clone(),
            kinds: must_export
                .kind
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", "),
        });
    }

    Ok(annotation.iter().cloned().collect())
}

/// Renders the export-name template against both patterns' capture groups.
///
/// A rule whose template names a group no pattern defines is a config bug that
/// would otherwise surface only when a file happened to match, which could be
/// months later or never.
fn check_template(
    rule: &RuleId,
    file_pattern: &Pattern,
    dir_pattern: Option<&Pattern>,
    must_export: &MustExport,
) -> Result<(), CompileError> {
    let from_file = file_pattern.capture_names();
    let from_dir = dir_pattern.map(Pattern::capture_names).unwrap_or_default();

    // Refused rather than resolved by precedence. The two patterns share one
    // template namespace so that `{{pascal(entity)}}{{pascal(action)}}` reads
    // as one name rather than as two sources spliced together -- and the price
    // of that is that a group defined twice has no answer. Picking the
    // filename's silently would make the rule demand the wrong export on every
    // file in the scope, which is the state where a `naming` rule gets deleted
    // rather than fixed.
    if let Some(group) = from_file.iter().find(|group| from_dir.contains(group)) {
        return Err(CompileError::DuplicateCaptureGroup {
            rule: rule.clone(),
            group: (*group).to_owned(),
        });
    }

    let lookup = |group: &str| {
        (from_file.contains(&group) || from_dir.contains(&group))
            // The value is irrelevant: only whether the group exists is being
            // checked here.
            .then(|| "placeholder".to_owned())
    };

    let annotations = must_export
        .annotation
        .iter()
        .flat_map(|patterns| patterns.iter());

    for text in [Some(&must_export.name), must_export.signature_hint.as_ref()]
        .into_iter()
        .flatten()
        .chain(annotations)
    {
        template::render(text, lookup).map_err(|source| CompileError::Template {
            rule: rule.clone(),
            source,
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::ids::DecisionId;

    /// `must_exist` names one companion beside the file, literally.
    ///
    /// Tested here rather than only through the CLI: this crate owns the rule
    /// about what the field accepts, and a crate that leans on an integration
    /// test three crates away has no test of its own contract.
    #[test]
    fn a_companion_is_a_relative_literal_path() {
        let id = RuleId::new("r").expect("valid id");

        assert_eq!(companion(&id, "notas.md").expect("relative"), "notas.md");
        assert_eq!(
            companion(&id, "  ../projeto.md  ").expect("relative"),
            "../projeto.md",
            "and it is trimmed"
        );
    }

    /// Absolute or empty is refused: the file that needs the companion is the
    /// anchor, and an absolute path would say the same thing from every
    /// directory the rule covers.
    #[test]
    fn a_companion_that_is_not_relative_is_refused() {
        let id = RuleId::new("r").expect("valid id");

        for path in ["/etc/hosts", "\\windows\\path", "", "   "] {
            assert!(
                companion(&id, path).is_err(),
                "`{path}` was accepted as a companion"
            );
        }
    }

    /// A `spec_dirs` entry is a directory name, and the names survive.
    ///
    /// Tested here rather than only through the CLI: a validator returning an
    /// empty list would drop every entry, the feature would stop working, and
    /// the CLI test — which only asserts that a *path* is refused — would keep
    /// passing.
    #[test]
    fn a_spec_dir_is_a_directory_name_and_it_survives() {
        let id = RuleId::new("r").expect("valid id");
        let spec = |dirs: &[&str]| SpecPairRule {
            id: id.clone(),
            level: Level::Error,
            why: None,
            decision: None,
            roots: crate::one_or_many::OneOrMany::One("src/*".to_owned()),
            subfolders: crate::one_or_many::OneOrMany::One(".".to_owned()),
            spec_markers: crate::one_or_many::OneOrMany::One("spec".to_owned()),
            spec_dirs: crate::one_or_many::OneOrMany::Many(
                dirs.iter().map(|d| (*d).to_owned()).collect(),
            ),
            ignore_files: crate::one_or_many::OneOrMany::Many(Vec::new()),
            require_non_empty_spec: false,
            skip_type_only: false,
            when_importing: crate::one_or_many::OneOrMany::Many(Vec::new()),
            when_importing_packages: Vec::new(),
        };

        assert_eq!(
            spec_dirs(&id, &spec(&["__tests__", "tests"])).expect("valid"),
            vec!["__tests__".to_owned(), "tests".to_owned()],
            "the names the author wrote are the names the rule gets"
        );
        assert_eq!(
            spec_dirs(&id, &spec(&["  __tests__  "])).expect("valid"),
            vec!["__tests__".to_owned()],
            "and they are trimmed"
        );
        assert!(
            spec_dirs(&id, &spec(&[])).expect("valid").is_empty(),
            "naming none is sibling-only, not an error"
        );
    }

    /// An entry that is a path asks the rule to reach a level deeper than it
    /// says. Accepting it would let a spec anywhere below satisfy the rule, and
    /// a `spec-pair` rule that reports nothing looks exactly like a repository
    /// that is fully tested. Issue #67.
    #[test]
    fn a_spec_dir_that_is_a_path_or_empty_is_refused() {
        let id = RuleId::new("r").expect("valid id");
        let spec = |dir: &str| SpecPairRule {
            id: id.clone(),
            level: Level::Error,
            why: None,
            decision: None,
            roots: crate::one_or_many::OneOrMany::One("src/*".to_owned()),
            subfolders: crate::one_or_many::OneOrMany::One(".".to_owned()),
            spec_markers: crate::one_or_many::OneOrMany::One("spec".to_owned()),
            spec_dirs: crate::one_or_many::OneOrMany::One(dir.to_owned()),
            ignore_files: crate::one_or_many::OneOrMany::Many(Vec::new()),
            require_non_empty_spec: false,
            skip_type_only: false,
            when_importing: crate::one_or_many::OneOrMany::Many(Vec::new()),
            when_importing_packages: Vec::new(),
        };

        for entry in ["__tests__/unit", "a\\b", "", "   "] {
            assert!(
                spec_dirs(&id, &spec(entry)).is_err(),
                "`{entry}` was accepted as a directory name"
            );
        }
    }

    /// Issue #50. The template form is what `naming.must_export` and
    /// `frontmatter.equals` accept, so reaching for it here is the obvious
    /// mistake -- and it used to compile, then report every governed file as
    /// missing a companion with braces in its name.
    #[test]
    fn a_companion_that_reaches_for_a_template_is_refused() {
        let id = RuleId::new("r").expect("valid id");

        let error = companion(&id, "../../meu/{{raw(dirname)}}.md").expect_err("refused");
        assert!(
            error.to_string().contains("literal"),
            "the message should say why: {error}"
        );
    }

    use archwarden_core::{ids::ModuleId, level::Level, path::RepoRelPath};
    use camino::Utf8PathBuf;

    fn merged(json: &str) -> MergedConfig {
        let config: Config = serde_json::from_str(json).expect("config should parse");
        let path = Utf8PathBuf::from("arch.config.json");
        MergedConfig {
            config,
            path: path.clone(),
            root: Utf8PathBuf::from("."),
            sources: vec![path],
        }
    }

    fn compile_json(json: &str) -> Result<CompiledConfig, CompileError> {
        compile(&merged(json))
    }

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// A module with paths of its own narrows every rule inside it.
    ///
    /// Issue #74: the fixture config in `xtask/src/preview.rs` said
    /// `packages/domain/src/*` in two rules and `packages/domain/**` in a
    /// boundary that forbade a module by glob. Moving the package meant
    /// editing four places, and missing one made a rule stop reaching with
    /// nothing reporting it.
    #[test]
    fn a_rule_inside_a_scoped_module_reaches_where_both_reach() {
        let compiled = compile_json(
            r#"{"version":0,"modules":[
                {"id":"domain","scope":"packages/domain/**","rules":[
                  {"type":"structure","id":"shape","level":"error",
                   "roots":"packages/domain/src/*","allowed_subfolders":["calcs"]}]}]}"#,
        )
        .expect("compiles");

        let rule = compiled.rules().next().expect("one rule");
        assert!(
            rule.scope
                .matches_dir(camino::Utf8Path::new("packages/domain/src/order"))
        );
        assert!(
            !rule
                .scope
                .matches_dir(camino::Utf8Path::new("packages/billing/src/order")),
            "the module's scope narrows it"
        );
    }

    /// A module without one keeps working exactly as before, which is what
    /// makes the field additive rather than a migration.
    #[test]
    fn a_module_with_no_scope_narrows_nothing() {
        let compiled = compile_json(
            r#"{"version":0,"modules":[
                {"id":"domain","rules":[
                  {"type":"structure","id":"shape","level":"error",
                   "roots":"packages/*/src","allowed_subfolders":["calcs"]}]}]}"#,
        )
        .expect("compiles");

        let rule = compiled.rules().next().expect("one rule");
        assert!(
            rule.scope
                .matches_dir(camino::Utf8Path::new("packages/billing/src"))
        );
    }

    /// A rule whose `roots` points outside its module reaches nothing. That is
    /// the cost of narrowing over refusing, it is silent, and `config doctor`
    /// is where it stops being silent.
    #[test]
    fn a_rule_pointing_outside_its_module_reaches_nothing() {
        let compiled = compile_json(
            r#"{"version":0,"modules":[
                {"id":"domain","scope":"packages/domain/**","rules":[
                  {"type":"structure","id":"stray","level":"error",
                   "roots":"apps/api/src/*","allowed_subfolders":["calcs"]}]}]}"#,
        )
        .expect("compiles");

        let rule = compiled.rules().next().expect("one rule");
        assert!(
            !rule
                .scope
                .matches_dir(camino::Utf8Path::new("apps/api/src/env"))
        );
    }

    /// A boundary names the module it is about, instead of re-describing it.
    ///
    /// The duplication issue #74 opens with: the fixture said
    /// `packages/domain/**` in a boundary rule and `packages/domain/src/*` in
    /// the rules of a module called `domain`, and forbade `infrastructure` —
    /// a declared module — by glob. Four places to edit, and nothing knowing
    /// they were the same thing.
    #[test]
    fn a_boundary_can_name_the_modules_it_is_about() {
        let compiled = compile_json(
            r#"{"version":0,
                "modules":[
                  {"id":"domain","scope":"packages/domain/**"},
                  {"id":"infrastructure","scope":"packages/infrastructure/**"}],
                "rules":[
                  {"type":"import-boundary","id":"sealed","level":"error",
                   "from_module":"domain","forbid_module":["infrastructure"]}]}"#,
        )
        .expect("compiles");

        let rule = compiled.rules().next().expect("one rule");
        assert!(
            rule.scope
                .matches_dir(camino::Utf8Path::new("packages/domain/src"))
        );
        assert!(
            !rule
                .scope
                .matches_dir(camino::Utf8Path::new("packages/infrastructure/src"))
        );
        assert!(
            matches!(&rule.kind, CompiledRuleKind::ImportBoundary { forbid, .. }
                     if forbid.is_match(camino::Utf8Path::new("packages/infrastructure/src/pdf/pdf.ts"))),
            "the named module became the forbidden paths"
        );
    }

    /// Naming a module that does not exist is refused. Silently forbidding
    /// nothing is the failure this whole feature is meant to remove.
    #[test]
    fn a_boundary_naming_a_module_that_does_not_exist_is_refused() {
        let error = compile_json(
            r#"{"version":0,
                "modules":[{"id":"domain","scope":"packages/domain/**"}],
                "rules":[
                  {"type":"import-boundary","id":"sealed","level":"error",
                   "from_module":"domain","forbid_module":["infra"]}]}"#,
        )
        .expect_err("no such module");

        assert!(
            matches!(&error, CompileError::UnknownModule { module, .. } if module.as_str() == "infra"),
            "{error:?}"
        );
    }

    /// And naming one that exists but declared no paths, which would forbid
    /// nothing just as quietly.
    #[test]
    fn a_boundary_naming_a_module_with_no_scope_is_refused() {
        let error = compile_json(
            r#"{"version":0,
                "modules":[{"id":"domain","scope":"packages/domain/**"},
                           {"id":"loose"}],
                "rules":[
                  {"type":"import-boundary","id":"sealed","level":"error",
                   "from_module":"domain","forbid_module":["loose"]}]}"#,
        )
        .expect_err("no scope");

        assert!(
            matches!(&error, CompileError::ModuleHasNoScope { module, .. } if module.as_str() == "loose"),
            "{error:?}"
        );
    }

    /// Saying it both ways on one rule is refused rather than resolved. Two
    /// spellings of the scope on one rule is the ambiguity that produces a
    /// rule enforcing something nobody meant, and unlike glob containment this
    /// one is decidable.
    #[test]
    fn a_boundary_may_not_say_its_scope_both_ways() {
        let error = compile_json(
            r#"{"version":0,
                "modules":[{"id":"domain","scope":"packages/domain/**"}],
                "rules":[
                  {"type":"import-boundary","id":"sealed","level":"error",
                   "from":"apps/**","from_module":"domain",
                   "forbid_import_from":["packages/infra/**"]}]}"#,
        )
        .expect_err("both ways");

        assert!(
            matches!(&error, CompileError::ScopeSaidTwice { rule, .. } if rule.as_str() == "sealed"),
            "{error:?}"
        );
    }

    /// The allowlist reaches the compiled rule, and `None` and empty stay
    /// different.
    ///
    /// `Some([])` would mean "nothing in this repository may be imported",
    /// which is the strictest rule in any config; `None` means the rule does
    /// not work by allowlist at all. A lowering that collapsed them would turn
    /// every ordinary boundary into the loudest one there is.
    #[test]
    fn an_allowlist_reaches_the_compiled_rule() {
        let config = compile_json(
            r#"{"version":0,"rules":[
                  {"type":"import-boundary","id":"api-only-libs","level":"error",
                   "from":"apps/api/**","only_import_from":["packages/orders/**"]}]}"#,
        )
        .expect("compiles");

        let rule = config.rules().next().expect("one rule");
        let CompiledRuleKind::ImportBoundary { allow, .. } = &rule.kind else {
            panic!("expected an import-boundary rule");
        };
        let allow = allow.as_ref().expect("the rule works by allowlist");
        assert!(allow.is_match(path("packages/orders/cart.ts").as_path()));
        assert!(!allow.is_match(path("packages/db/client.ts").as_path()));
    }

    /// And a named module becomes its paths, so nothing downstream knows the
    /// difference.
    #[test]
    fn an_allowlist_of_modules_becomes_those_modules_paths() {
        let config = compile_json(
            r#"{"version":0,
                "modules":[{"id":"orders-core","scope":"packages/orders/**"}],
                "rules":[
                  {"type":"import-boundary","id":"api-only-libs","level":"error",
                   "from":"apps/api/**","only_import_from_modules":["orders-core"]}]}"#,
        )
        .expect("compiles");

        let rule = config.rules().next().expect("one rule");
        let CompiledRuleKind::ImportBoundary { allow, .. } = &rule.kind else {
            panic!("expected an import-boundary rule");
        };
        let allow = allow.as_ref().expect("the rule works by allowlist");
        assert!(allow.is_match(path("packages/orders/cart.ts").as_path()));
        assert!(!allow.is_match(path("packages/db/client.ts").as_path()));
    }

    /// A kind is the union of every module wearing it, which is the point of
    /// the quantifier: the seventh library is permitted because it exists.
    #[test]
    fn an_allowlist_of_kinds_is_every_module_wearing_it() {
        let config = compile_json(
            r#"{"version":0,
                "modules":[
                  {"id":"orders-core","kind":"lib","scope":"packages/orders/**"},
                  {"id":"billing-core","kind":"lib","scope":"packages/billing/**"},
                  {"id":"api","kind":"app","scope":"apps/api/**"}],
                "rules":[
                  {"type":"import-boundary","id":"apps-only-libs","level":"error",
                   "from":"apps/api/**","only_import_from_kinds":["lib"]}]}"#,
        )
        .expect("compiles");

        let rule = config.rules().next().expect("one rule");
        let CompiledRuleKind::ImportBoundary { allow, .. } = &rule.kind else {
            panic!("expected an import-boundary rule");
        };
        let allow = allow.as_ref().expect("the rule works by allowlist");
        assert!(allow.is_match(path("packages/orders/cart.ts").as_path()));
        assert!(
            allow.is_match(path("packages/billing/invoice.ts").as_path()),
            "the second library is permitted without being named"
        );
        assert!(!allow.is_match(path("apps/api/handler.ts").as_path()));
    }

    /// A boundary that names no allowlist field has `None`, not an empty set.
    #[test]
    fn a_boundary_with_no_allowlist_field_does_not_work_by_allowlist() {
        let config = compile_json(
            r#"{"version":0,"rules":[
                  {"type":"import-boundary","id":"sealed","level":"error",
                   "from":"apps/api/**","forbid_import_from":["packages/db/**"]}]}"#,
        )
        .expect("compiles");

        let rule = config.rules().next().expect("one rule");
        let CompiledRuleKind::ImportBoundary { allow, .. } = &rule.kind else {
            panic!("expected an import-boundary rule");
        };
        assert!(allow.is_none());
    }

    /// Every way of saying the allowlist twice, refused.
    ///
    /// One test rather than three, because the property is one: two spellings
    /// of one set on one rule means an author has to be told which won.
    #[test]
    fn an_allowlist_may_not_be_said_two_ways() {
        let both = |extra: &str| {
            compile_json(&format!(
                r#"{{"version":0,
                    "modules":[
                      {{"id":"orders-core","kind":"lib","scope":"packages/orders/**"}}],
                    "rules":[
                      {{"type":"import-boundary","id":"sealed","level":"error",
                       "from":"apps/api/**",{extra}}}]}}"#
            ))
            .expect_err("said two ways")
        };

        for extra in [
            r#""only_import_from":["packages/orders/**"],"only_import_from_kinds":["lib"]"#,
            r#""only_import_from_modules":["orders-core"],"only_import_from_kinds":["lib"]"#,
            r#""only_import_from":["packages/orders/**"],"only_import_from_modules":["orders-core"]"#,
        ] {
            let error = both(extra);
            assert!(
                matches!(&error, CompileError::ScopeSaidTwice { rule, .. }
                    if rule.as_str() == "sealed"),
                "{extra}: {error:?}"
            );
        }
    }

    /// "Only these, except those" is two rules, and is refused as one.
    ///
    /// Three fields reach the same refusal, and each is checked: a denylist by
    /// glob, a denylist by module, and an exception. Missing any one of them
    /// leaves a config that compiles into a rule whose two halves contradict
    /// each other, with nothing saying which the engine honours.
    #[test]
    fn an_allowlist_may_not_be_combined_with_a_denylist() {
        let with = |extra: &str| {
            compile_json(&format!(
                r#"{{"version":0,
                    "modules":[{{"id":"infra","scope":"packages/infra/**"}}],
                    "rules":[
                      {{"type":"import-boundary","id":"sealed","level":"error",
                       "from":"apps/api/**",
                       "only_import_from":["packages/orders/**"],{extra}}}]}}"#
            ))
            .expect_err("an allowlist beside a denylist")
        };

        for (extra, expected) in [
            (
                r#""forbid_import_from":["packages/db/**"]"#,
                "forbid_import_from",
            ),
            (r#""forbid_module":["infra"]"#, "forbid_import_from"),
            (r#""except":["packages/orders/types/**"]"#, "except"),
        ] {
            let error = with(extra);
            assert!(
                matches!(&error, CompileError::AllowlistAndDenylist { rule, other }
                    if rule.as_str() == "sealed" && *other == expected),
                "{extra}: {error:?}"
            );
        }
    }

    /// `from_kind` compiles one group per module wearing the kind, not one
    /// group for their union.
    ///
    /// This is the whole of the self-import question. The scope of such a rule
    /// *is* the union, so asking "is the target in scope?" would exempt one app
    /// importing another — exactly what the rule forbids. Identity decides it,
    /// and only per-module groups carry identity.
    #[test]
    fn a_rule_about_a_kind_compiles_one_group_per_module() {
        let config = compile_json(
            r#"{"version":0,
                "modules":[
                  {"id":"api-orders","kind":"app","scope":"apps/api-orders/**"},
                  {"id":"api-billing","kind":"app","scope":"apps/api-billing/**"},
                  {"id":"orders-core","kind":"lib","scope":"packages/orders/**"}],
                "rules":[
                  {"type":"import-boundary","id":"assemblies-are-islands","level":"error",
                   "from_kind":"app","only_import_from_kinds":["lib"]}]}"#,
        )
        .expect("compiles");

        let rule = config.rules().next().expect("one rule");
        let CompiledRuleKind::ImportBoundary { groups, .. } = &rule.kind else {
            panic!("expected an import-boundary rule");
        };
        assert_eq!(groups.len(), 2, "one per app, not one for both");
        assert!(
            groups.iter().any(|group| {
                group.is_match(path("apps/api-orders/handler.ts").as_path())
                    && !group.is_match(path("apps/api-billing/handler.ts").as_path())
            }),
            "and each group is one app alone, or a sibling import would be \
             exempt"
        );
    }

    /// A boundary that is not about a kind has no groups, and the engine then
    /// answers the self-import question from the scope.
    #[test]
    fn a_boundary_not_about_a_kind_has_no_groups() {
        let config = compile_json(
            r#"{"version":0,"rules":[
                  {"type":"import-boundary","id":"sealed","level":"error",
                   "from":"apps/api/**","forbid_import_from":["packages/db/**"]}]}"#,
        )
        .expect("compiles");

        let rule = config.rules().next().expect("one rule");
        let CompiledRuleKind::ImportBoundary { groups, .. } = &rule.kind else {
            panic!("expected an import-boundary rule");
        };
        assert!(groups.is_empty());
    }

    /// A kind nothing wears is refused rather than compiled into a scope that
    /// selects nothing.
    ///
    /// A rule quantifying over an empty set governs nothing, silently — which
    /// is the failure the quantifier exists to remove, arriving through a typo.
    #[test]
    fn a_kind_no_module_wears_is_refused() {
        let error = compile_json(
            r#"{"version":0,
                "modules":[{"id":"api","kind":"app","scope":"apps/api/**"}],
                "rules":[
                  {"type":"import-boundary","id":"islands","level":"error",
                   "from_kind":"aap","only_import_from_kinds":["lib"]}]}"#,
        )
        .expect_err("no module wears `aap`");

        assert!(
            matches!(&error, CompileError::UnknownKind { rule, kind }
                if rule.as_str() == "islands" && kind == "aap"),
            "{error:?}"
        );
    }

    /// `from_kind` becomes a scope covering every module that wears it, and
    /// nothing else.
    #[test]
    fn a_kind_scope_covers_every_module_wearing_it() {
        let config = compile_json(
            r#"{"version":0,
                "modules":[
                  {"id":"api-orders","kind":"app","scope":"apps/api-orders/**"},
                  {"id":"api-billing","kind":"app","scope":"apps/api-billing/**"},
                  {"id":"orders-core","kind":"lib","scope":"packages/orders/**"}],
                "rules":[
                  {"type":"import-boundary","id":"islands","level":"error",
                   "from_kind":"app","forbid_import_from":["packages/db/**"]}]}"#,
        )
        .expect("compiles");

        let rule = config.rules().next().expect("one rule");
        assert!(
            rule.scope
                .contains_file(path("apps/api-orders/x.ts").as_path())
        );
        assert!(
            rule.scope
                .contains_file(path("apps/api-billing/x.ts").as_path())
        );
        assert!(
            !rule
                .scope
                .contains_file(path("packages/orders/x.ts").as_path()),
            "the libraries wear a different kind and are not importers here"
        );
    }

    /// Saying the scope as a kind *and* as globs is refused, like every other
    /// way of saying it twice.
    #[test]
    fn a_boundary_may_not_say_its_scope_as_a_kind_and_as_globs() {
        for extra in [r#""from":"apps/**""#, r#""from_module":"api-orders""#] {
            let error = compile_json(&format!(
                r#"{{"version":0,
                    "modules":[
                      {{"id":"api-orders","kind":"app","scope":"apps/api-orders/**"}}],
                    "rules":[
                      {{"type":"import-boundary","id":"islands","level":"error",
                       "from_kind":"app",{extra},
                       "forbid_import_from":["packages/db/**"]}}]}}"#
            ))
            .expect_err("said two ways");

            assert!(
                matches!(&error, CompileError::ScopeSaidTwice { rule, .. }
                    if rule.as_str() == "islands"),
                "{extra}: {error:?}"
            );
        }
    }

    /// The modules a config declares reach the compiled config, with the scope
    /// and kind each was given.
    ///
    /// `config doctor` and `config explain` answer from this list, so a
    /// lowering that dropped it would leave both commands reporting a config
    /// with no modules in it — while `check` went on enforcing their rules.
    #[test]
    fn the_declared_modules_reach_the_compiled_config() {
        let config = compile_json(
            r#"{"version":0,
                "modules":[
                  {"id":"api","kind":"app","scope":"apps/api/**"},
                  {"id":"loose"}],
                "rules":[
                  {"type":"import-boundary","id":"sealed","level":"error",
                   "from":"apps/api/**","forbid_import_from":["packages/db/**"]}]}"#,
        )
        .expect("compiles");

        let modules: Vec<_> = config.modules().collect();
        assert_eq!(modules.len(), 2);
        let api = modules
            .iter()
            .find(|module| module.id.as_str() == "api")
            .expect("`api` is declared");
        assert_eq!(api.kind.as_deref(), Some("app"));
        assert_eq!(
            api.scope
                .as_ref()
                .map(archwarden_core::scope::Scope::patterns),
            Some(&["apps/api/**".to_owned()][..])
        );

        let loose = modules
            .iter()
            .find(|module| module.id.as_str() == "loose")
            .expect("`loose` is declared");
        assert!(loose.kind.is_none());
        assert!(loose.scope.is_none(), "a module may have neither");
    }

    /// `only_import_from_packages` reaches the compiled rule, and stays `None`
    /// when the rule does not use it.
    ///
    /// The same `None`-is-not-empty distinction the path allowlist has, one
    /// axis over: an empty package allowlist would forbid every dependency in
    /// the repository.
    #[test]
    fn a_package_allowlist_is_absent_rather_than_empty_when_unused() {
        let with = compile_json(
            r#"{"version":0,"rules":[
                  {"type":"import-boundary","id":"only-zod","level":"error",
                   "from":"apps/api/**","only_import_from_packages":["zod"]}]}"#,
        )
        .expect("compiles");
        let CompiledRuleKind::ImportBoundary { allow_packages, .. } =
            &with.rules().next().expect("one rule").kind
        else {
            panic!("expected an import-boundary rule");
        };
        assert_eq!(allow_packages.as_deref(), Some(&["zod".to_owned()][..]));

        let without = compile_json(
            r#"{"version":0,"rules":[
                  {"type":"import-boundary","id":"sealed","level":"error",
                   "from":"apps/api/**","forbid_import_from":["packages/db/**"]}]}"#,
        )
        .expect("compiles");
        let CompiledRuleKind::ImportBoundary { allow_packages, .. } =
            &without.rules().next().expect("one rule").kind
        else {
            panic!("expected an import-boundary rule");
        };
        assert!(allow_packages.is_none());
    }

    /// `forbid_reaching` reaches the compiled rule as globs, or the engine
    /// answers `needs_graph` with `false` and the rule quietly enforces
    /// nothing.
    #[test]
    fn forbid_reaching_globs_reach_the_compiled_rule() {
        let config = compile_json(
            r#"{"version":0,"rules":[
                  {"type":"import-boundary","id":"ui-must-not-reach-db","level":"error",
                   "from":"packages/ui/**","forbid_reaching":["packages/db/**"]}]}"#,
        )
        .expect("compiles");

        let rule = config.rules().next().expect("one rule");
        let CompiledRuleKind::ImportBoundary {
            forbid_reaching, ..
        } = &rule.kind
        else {
            panic!("expected an import-boundary rule");
        };
        assert!(forbid_reaching.is_match(path("packages/db/client.ts").as_path()));
        assert!(!forbid_reaching.is_match(path("packages/orders/cart.ts").as_path()));
    }

    /// And a named module becomes its paths, so nothing downstream knows the
    /// difference — the same folding `forbid_module` does for the direct form.
    #[test]
    fn forbid_reaching_modules_becomes_that_modules_paths() {
        let config = compile_json(
            r#"{"version":0,
                "modules":[{"id":"persistence","scope":"packages/db/**"}],
                "rules":[
                  {"type":"import-boundary","id":"ui-must-not-reach-db","level":"error",
                   "from":"packages/ui/**","forbid_reaching_modules":["persistence"]}]}"#,
        )
        .expect("compiles");

        let rule = config.rules().next().expect("one rule");
        let CompiledRuleKind::ImportBoundary {
            forbid_reaching, ..
        } = &rule.kind
        else {
            panic!("expected an import-boundary rule");
        };
        assert!(forbid_reaching.is_match(path("packages/db/client.ts").as_path()));
        assert!(!forbid_reaching.is_match(path("packages/ui/button.tsx").as_path()));
    }

    /// Saying the reach both ways on one rule is refused, for the same reason
    /// saying the scope both ways is: two spellings of one set means somebody
    /// has to be told which one won.
    #[test]
    fn a_boundary_may_not_say_what_it_forbids_reaching_both_ways() {
        let error = compile_json(
            r#"{"version":0,
                "modules":[{"id":"persistence","scope":"packages/db/**"}],
                "rules":[
                  {"type":"import-boundary","id":"sealed","level":"error",
                   "from":"packages/ui/**",
                   "forbid_reaching":["packages/db/**"],
                   "forbid_reaching_modules":["persistence"]}]}"#,
        )
        .expect_err("both ways");

        assert!(
            matches!(&error, CompileError::ScopeSaidTwice { rule, one, other }
                if rule.as_str() == "sealed"
                    && *one == "forbid_reaching"
                    && *other == "forbid_reaching_modules"),
            "{error:?}"
        );
    }

    /// A boundary that says nothing about reach compiles to an empty set, and
    /// that emptiness is what keeps the run off the whole repository.
    #[test]
    fn a_boundary_silent_about_reach_compiles_to_an_empty_set() {
        let config = compile_json(
            r#"{"version":0,"rules":[
                  {"type":"import-boundary","id":"sealed","level":"error",
                   "from":"packages/ui/**","forbid_import_from":["packages/db/**"]}]}"#,
        )
        .expect("compiles");

        let rule = config.rules().next().expect("one rule");
        let CompiledRuleKind::ImportBoundary {
            forbid_reaching, ..
        } = &rule.kind
        else {
            panic!("expected an import-boundary rule");
        };
        assert!(forbid_reaching.is_empty());
    }

    /// And a boundary that says neither has no importers to be about.
    #[test]
    fn a_boundary_must_say_who_it_is_about() {
        let error = compile_json(
            r#"{"version":0,"rules":[
                  {"type":"import-boundary","id":"sealed","level":"error",
                   "forbid_import_from":["packages/infra/**"]}]}"#,
        )
        .expect_err("says neither");

        assert!(
            matches!(&error, CompileError::ScopeMissing { rule, .. } if rule.as_str() == "sealed"),
            "{error:?}"
        );
    }

    /// A scope that is not a glob is refused, and names the module rather than
    /// the rule: the rule is fine and the module is what has to be fixed.
    #[test]
    fn a_module_scope_that_is_not_a_glob_is_refused_by_module() {
        let error = compile_json(
            r#"{"version":0,"modules":[
                {"id":"domain","scope":"packages/[","rules":[]}]}"#,
        )
        .expect_err("not a glob");

        assert!(
            matches!(&error, CompileError::ModuleScope { module, .. } if module.as_str() == "domain"),
            "{error:?}"
        );
    }

    /// Extracts a `Pattern` error, or `None`. See the convention in
    /// CONTRIBUTING.md about not using `let ... else { panic!() }` in a test.
    fn pattern_error(error: &CompileError) -> Option<(&RuleId, &'static str)> {
        match error {
            CompileError::Pattern { rule, field, .. } => Some((rule, field)),
            _ => None,
        }
    }

    /// The `KindFilter` of the config's single naming rule, or `None`.
    fn only_naming_kind(compiled: &CompiledConfig) -> Option<&KindFilter> {
        match &compiled.rules().next()?.kind {
            CompiledRuleKind::Naming { kind, .. } => Some(kind),
            _ => None,
        }
    }

    /// Issue #46. A module is a bigger decision than any rule inside it —
    /// "why does `domain` exist and why is it sealed" is one sentence that
    /// explains eight rules — so both are carried, and neither stands in for
    /// the other.
    #[test]
    fn a_rules_reason_and_its_modules_both_reach_the_compiled_rule() {
        let compiled = compile_json(
            r#"{"version":0,"modules":[{
                 "id":"domain",
                 "why":"extracted so billing could depend on it without the API",
                 "rules":[{"type":"structure","id":"shape","level":"error",
                           "why":"entities are the only thing published",
                           "roots":"packages/domain/src/*",
                           "allowed_subfolders":["types"]}]}]}"#,
        )
        .expect("compiles");

        let rule = compiled.rules().next().expect("one rule");
        assert_eq!(
            rule.why.as_deref(),
            Some("entities are the only thing published")
        );
        assert_eq!(
            rule.module_why.as_deref(),
            Some("extracted so billing could depend on it without the API")
        );
    }

    /// A rule outside a module, and one whose author said nothing.
    #[test]
    fn a_rule_with_no_reason_carries_none() {
        let compiled = compile_json(
            r#"{"version":0,"rules":[{"type":"structure","id":"shape","level":"error",
                 "roots":"src/*","allowed_subfolders":["types"]}]}"#,
        )
        .expect("compiles");

        let rule = compiled.rules().next().expect("one rule");
        assert_eq!(rule.why, None);
        assert_eq!(rule.module_why, None);
    }

    /// Issue #100. The rule carries the reference; the prose stays in one
    /// place on the config, because N rules serve one decision and copying it
    /// onto each is N places for it to disagree with itself.
    #[test]
    fn a_rule_carries_the_decision_it_implements() {
        let compiled = compile_json(
            r#"{"version":0,
                "decisions":[{"id":"ADR-014",
                              "title":"The domain does not know about transport",
                              "why":"it is published, and a consumer must not inherit our client",
                              "link":"docs/adr/014.md"}],
                "rules":[{"type":"structure","id":"shape","level":"error",
                          "decision":"ADR-014",
                          "roots":"packages/domain/src/*",
                          "allowed_subfolders":["types"]}]}"#,
        )
        .expect("compiles");

        let rule = compiled.rules().next().expect("one rule");
        assert_eq!(
            rule.decision.as_ref().map(DecisionId::as_str),
            Some("ADR-014")
        );

        let decision = compiled
            .decision(rule.decision.as_ref().expect("names one"))
            .expect("the config declares it");
        assert_eq!(decision.title, "The domain does not know about transport");
        assert_eq!(decision.link.as_deref(), Some("docs/adr/014.md"));
        assert!(decision.status.is_accepted());
    }

    /// Every rule written before 0.21, unchanged.
    #[test]
    fn a_rule_naming_no_decision_carries_none() {
        let compiled = compile_json(
            r#"{"version":0,"rules":[{"type":"structure","id":"shape","level":"error",
                 "roots":"src/*","allowed_subfolders":["types"]}]}"#,
        )
        .expect("compiles");

        assert_eq!(compiled.rules().next().expect("one rule").decision, None);
        assert_eq!(compiled.decisions().count(), 0);
    }

    /// A reference to a decision nobody declared is a typo, and it is refused
    /// where `from_module` naming an undeclared module is refused: at compile,
    /// when the config loads, rather than in a command the user may never run.
    ///
    /// Not to be confused with a rule that names *no* decision, which is every
    /// existing config and stays perfectly valid.
    #[test]
    fn a_rule_pointing_at_an_undeclared_decision_is_refused() {
        let error = compile_json(
            r#"{"version":0,
                "decisions":[{"id":"ADR-014","title":"t"}],
                "rules":[{"type":"structure","id":"shape","level":"error",
                          "decision":"ADR-041","roots":"src/*",
                          "allowed_subfolders":[]}]}"#,
        )
        .expect_err("should refuse");

        let CompileError::UnknownDecision { rule, decision } = &error else {
            panic!("expected UnknownDecision, got {error:?}");
        };
        assert_eq!(rule.as_str(), "shape");
        assert_eq!(decision.as_str(), "ADR-041");
        assert!(
            error.to_string().contains("ADR-041"),
            "the message names the id that matched nothing: {error}"
        );
    }

    /// The three claims of a `metadata` rule reach the compiled kind, and the
    /// keys are names rather than patterns. Issue #104.
    #[test]
    fn a_metadata_rule_carries_its_keys_vocabularies_and_agreements() {
        let compiled = compile_json(
            r#"{"version":0,"rules":[{"type":"metadata","id":"payments-owned","level":"error",
                 "roots":"src/payments/**","require":["owner"],
                 "one_of":{"stability":["stable","experimental"]},
                 "equals":{"module":"{{raw(dirname)}}"}}]}"#,
        )
        .expect("compiles");

        let CompiledRuleKind::Metadata {
            require,
            one_of,
            equals,
        } = &compiled.rules().next().expect("one rule").kind
        else {
            panic!("expected a metadata rule");
        };

        assert_eq!(require, &["owner".to_owned()]);
        assert_eq!(
            one_of,
            &[(
                "stability".to_owned(),
                vec!["stable".to_owned(), "experimental".to_owned()]
            )]
        );
        assert_eq!(
            equals,
            &[("module".to_owned(), "{{raw(dirname)}}".to_owned())]
        );
    }

    /// A key the suppression grammar reaches first is unenforceable however a
    /// file is written: `// archwarden-allow: x` is a suppression and never a
    /// claim. Refused where the config compiles rather than left reporting an
    /// absence nobody can fix — the precedent `UnknownDecision` sets.
    #[test]
    fn a_metadata_rule_asking_for_an_unreachable_key_is_refused() {
        let error = compile_json(
            r#"{"version":0,"rules":[{"type":"metadata","id":"payments-owned","level":"error",
                 "roots":"src/payments/**","require":["allowance"]}]}"#,
        )
        .expect_err("should refuse");

        let CompileError::UnreachableMetadataKey { rule, key } = &error else {
            panic!("expected UnreachableMetadataKey, got {error:?}");
        };
        assert_eq!(rule.as_str(), "payments-owned");
        assert_eq!(key, "allowance");
        assert!(
            error.to_string().contains("archwarden-allow"),
            "the message says which grammar swallows it: {error}"
        );
    }

    /// Every clause that names a key is checked, not only `require`: a
    /// vocabulary about a key no file can declare is the same dead rule.
    #[test]
    fn an_unreachable_key_is_refused_wherever_it_is_named() {
        for clause in [
            r#""one_of":{"allow":["yes"]}"#,
            r#""equals":{"allow":"{{raw(dirname)}}"}"#,
        ] {
            let error = compile_json(&format!(
                r#"{{"version":0,"rules":[{{"type":"metadata","id":"r","level":"error",
                     "roots":"src/**",{clause}}}]}}"#
            ))
            .expect_err("should refuse");

            assert!(
                matches!(error, CompileError::UnreachableMetadataKey { .. }),
                "expected UnreachableMetadataKey for {clause}, got {error:?}"
            );
        }
    }

    /// A template naming a group nothing defines is refused here as it is for
    /// a document: a rule that quietly demanded the wrong value would be worse
    /// than one that would not load.
    #[test]
    fn a_metadata_agreement_may_name_only_the_directory() {
        let error = compile_json(
            r#"{"version":0,"rules":[{"type":"metadata","id":"r","level":"error",
                 "roots":"src/**","equals":{"module":"{{raw(basename)}}"}}]}"#,
        )
        .expect_err("should refuse");

        assert!(
            matches!(error, CompileError::Template { .. }),
            "got {error:?}"
        );
    }

    /// Issue #114. The half of an ADR that stops the losing option being
    /// proposed again, and the half a rule can never carry.
    #[test]
    fn a_decision_carries_what_it_rejected() {
        let compiled = compile_json(
            r#"{"version":0,
                "decisions":[{"id":"ADR-014","title":"hexagonal",
                  "alternatives":[
                    {"option":"TypeORM in the domain",
                     "why_not":"the schema starts dictating the model",
                     "refused_by":"no-orm-in-domain"},
                    {"option":"a generic repository",
                     "why_not":"it hides the queries worth reading"}]}],
                "rules":[{"type":"structure","id":"no-orm-in-domain","level":"error",
                          "roots":"src/*","allowed_subfolders":[]}]}"#,
        )
        .expect("compiles");

        let decision = compiled.decisions().next().expect("one decision");
        assert_eq!(decision.alternatives.len(), 2);
        assert_eq!(decision.alternatives[0].option, "TypeORM in the domain");
        assert_eq!(
            decision.alternatives[0].why_not,
            "the schema starts dictating the model"
        );
        assert_eq!(
            decision.alternatives[0]
                .refused_by
                .as_ref()
                .map(RuleId::as_str),
            Some("no-orm-in-domain")
        );
        assert_eq!(
            decision.alternatives[1].refused_by, None,
            "an option nothing refuses is written down and nothing stops it"
        );
    }

    /// A reference to a rule nobody wrote is a typo, and it is refused where a
    /// rule naming an undeclared module already is: at compile.
    #[test]
    fn an_alternative_refused_by_a_rule_that_does_not_exist_is_refused() {
        let error = compile_json(
            r#"{"version":0,
                "decisions":[{"id":"ADR-014","title":"hexagonal",
                  "alternatives":[{"option":"TypeORM","why_not":"no",
                                   "refused_by":"no-such-rule"}]}],
                "rules":[]}"#,
        )
        .expect_err("should refuse");

        let CompileError::UnknownRefusingRule {
            decision,
            option,
            rule,
        } = &error
        else {
            panic!("expected UnknownRefusingRule, got {error:?}");
        };
        assert_eq!(decision.as_str(), "ADR-014");
        assert_eq!(option, "TypeORM");
        assert_eq!(rule.as_str(), "no-such-rule");
    }

    /// Issue #115. The new decision knows what it replaces; the old one does
    /// not have to be edited to be replaced, and the reverse is computed.
    #[test]
    fn supersession_is_written_forward_and_read_both_ways() {
        let compiled = compile_json(
            r#"{"version":0,
                "decisions":[
                  {"id":"ADR-009","title":"the old way"},
                  {"id":"ADR-031","title":"the new way","supersedes":"ADR-009"}],
                "rules":[]}"#,
        )
        .expect("compiles");

        let decisions: Vec<_> = compiled.decisions().collect();
        assert_eq!(
            decisions[0].superseded_by,
            vec![DecisionId::new("ADR-031").expect("valid")]
        );
        assert_eq!(
            decisions[1].supersedes,
            vec![DecisionId::new("ADR-009").expect("valid")]
        );
        assert!(decisions[1].superseded_by.is_empty());
    }

    /// And the status comes with it. Somebody who writes `supersedes` and
    /// forgets to go and change the old decision's own status has a config
    /// that says two things — and disarms `superseded-decision-still-enforced`,
    /// which is the check with the most value here.
    #[test]
    fn a_superseded_decision_takes_the_status_without_repeating_it() {
        let compiled = compile_json(
            r#"{"version":0,
                "decisions":[
                  {"id":"ADR-009","title":"the old way"},
                  {"id":"ADR-031","title":"the new way","supersedes":"ADR-009"}],
                "rules":[]}"#,
        )
        .expect("compiles");

        let decisions: Vec<_> = compiled.decisions().collect();
        assert!(decisions[0].status.is_superseded(), "{:?}", decisions[0]);
        assert!(decisions[1].status.is_accepted());
    }

    /// Writing it out is fine; writing the opposite is a config saying two
    /// things, and it is refused rather than silently overridden.
    #[test]
    fn a_superseded_decision_that_calls_itself_accepted_is_refused() {
        let saying_both = compile_json(
            r#"{"version":0,
                "decisions":[
                  {"id":"ADR-009","title":"the old way","status":"accepted"},
                  {"id":"ADR-031","title":"the new way","supersedes":"ADR-009"}],
                "rules":[]}"#,
        )
        .expect_err("should refuse");

        let CompileError::StatusContradictsSupersession { decision, by, .. } = &saying_both else {
            panic!("expected StatusContradictsSupersession, got {saying_both:?}");
        };
        assert_eq!(decision.as_str(), "ADR-009");
        assert_eq!(by.as_str(), "ADR-031");

        compile_json(
            r#"{"version":0,
                "decisions":[
                  {"id":"ADR-009","title":"the old way","status":"superseded"},
                  {"id":"ADR-031","title":"the new way","supersedes":"ADR-009"}],
                "rules":[]}"#,
        )
        .expect("saying it out loud agrees with the edge, and is allowed");
    }

    /// A reference to a decision nobody declared, on the same argument as
    /// every other dangling reference here.
    #[test]
    fn superseding_a_decision_that_does_not_exist_is_refused() {
        let error = compile_json(
            r#"{"version":0,
                "decisions":[{"id":"ADR-031","title":"the new way","supersedes":"ADR-009"}],
                "rules":[]}"#,
        )
        .expect_err("should refuse");

        assert!(
            matches!(error, CompileError::UnknownSuperseded { .. }),
            "got {error:?}"
        );
    }

    /// A decision cannot replace itself, and two cannot replace each other:
    /// both leave a chain with no end, and every surface that draws one would
    /// walk it forever.
    #[test]
    fn a_supersession_cycle_is_refused() {
        for decisions in [
            r#"[{"id":"ADR-009","title":"itself","supersedes":"ADR-009"}]"#,
            r#"[{"id":"ADR-009","title":"a","supersedes":"ADR-031"},
                {"id":"ADR-031","title":"b","supersedes":"ADR-009"}]"#,
        ] {
            let error = compile_json(&format!(
                r#"{{"version":0,"decisions":{decisions},"rules":[]}}"#
            ))
            .expect_err("should refuse");

            assert!(
                matches!(error, CompileError::SupersessionCycle { .. }),
                "expected a cycle for {decisions}, got {error:?}"
            );
        }
    }

    /// The decisions survive compilation whether or not any rule points at
    /// them: `config doctor` has to be able to see an orphan, and the page
    /// lists what the architecture decided rather than what it enforces.
    #[test]
    fn decisions_reach_the_compiled_config_in_declaration_order() {
        let compiled = compile_json(
            r#"{"version":0,"decisions":[
                 {"id":"ADR-2","title":"second","status":"superseded"},
                 {"id":"ADR-1","title":"first"}],"rules":[]}"#,
        )
        .expect("compiles");

        let ids: Vec<&str> = compiled.decisions().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["ADR-2", "ADR-1"]);
        assert_eq!(
            compiled.decisions().next().expect("one").status,
            archwarden_core::compiled::DecisionStatus::Superseded
        );
    }

    /// Rewording a decision does not invalidate the findings cache.
    ///
    /// The same promise `why` makes one level down — prose is not part of a
    /// finding's identity — and here it falls out of where the prose lives:
    /// `rules_hash` serialises the *rules*, and the words are on the config.
    /// A team fixing a typo in a decision's title should not pay for a full
    /// re-check of the repository.
    #[test]
    fn rewording_a_decision_does_not_change_the_rules_hash() {
        let with = |title: &str| {
            compile_json(&format!(
                r#"{{"version":0,
                    "decisions":[{{"id":"ADR-014","title":"{title}"}}],
                    "rules":[{{"type":"structure","id":"shape","level":"error",
                               "decision":"ADR-014","roots":"src/*",
                               "allowed_subfolders":[]}}]}}"#
            ))
            .expect("compiles")
            .rules_hash()
        };

        assert_eq!(with("first wording"), with("second wording"));
    }

    /// But repointing a *rule* at a different decision does, because that is
    /// the rule changing.
    #[test]
    fn repointing_a_rule_at_another_decision_changes_the_rules_hash() {
        let with = |decision: &str| {
            compile_json(&format!(
                r#"{{"version":0,
                    "decisions":[{{"id":"ADR-1","title":"a"}},{{"id":"ADR-2","title":"b"}}],
                    "rules":[{{"type":"structure","id":"shape","level":"error",
                               "decision":"{decision}","roots":"src/*",
                               "allowed_subfolders":[]}}]}}"#
            ))
            .expect("compiles")
            .rules_hash()
        };

        assert_ne!(with("ADR-1"), with("ADR-2"));
    }

    /// A rule inside a module names its decision the same way a top-level one
    /// does. Decisions are top level only — a decision that spans modules is
    /// the common case — but the rules that serve them are wherever they are.
    #[test]
    fn a_rule_inside_a_module_may_name_a_top_level_decision() {
        let compiled = compile_json(
            r#"{"version":0,
                "decisions":[{"id":"ADR-014","title":"t"}],
                "modules":[{"id":"domain","rules":[
                  {"type":"structure","id":"shape","level":"error","decision":"ADR-014",
                   "roots":"src/*","allowed_subfolders":[]}]}]}"#,
        )
        .expect("compiles");

        assert_eq!(
            compiled
                .rules()
                .next()
                .expect("one rule")
                .decision
                .as_ref()
                .map(DecisionId::as_str),
            Some("ADR-014")
        );
    }

    /// A decision may not be declared inside a module, and the refusal is the
    /// parser's: `deny_unknown_fields` on `Module` means the key is not a
    /// thing a module has. One place to look for a decision, not two.
    #[test]
    fn a_module_may_not_declare_decisions() {
        assert!(
            serde_json::from_str::<crate::config::Config>(
                r#"{"version":0,"modules":[{"id":"m","decisions":[{"id":"d","title":"t"}]}]}"#
            )
            .is_err()
        );
    }

    /// The compiled annotations of the config's single naming rule.
    fn only_naming_annotation(compiled: &CompiledConfig) -> Option<&[String]> {
        match &compiled.rules().next()?.kind {
            CompiledRuleKind::Naming { annotation, .. } => Some(annotation),
            _ => None,
        }
    }

    fn tool_rule(must_export: &str) -> String {
        format!(
            r#"{{"version":0,"rules":[{{"type":"naming","id":"tools","level":"error",
               "roots":"src/tools","file_pattern":"^(?<tool>[a-z-]+)\\.tool\\.ts$",
               "must_export":{must_export}}}]}}"#
        )
    }

    #[test]
    fn an_annotation_reaches_the_compiled_rule() {
        let compiled = compile_json(&tool_rule(
            r#"{"kind":["const"],"name":"AGENT_TOOL","annotation":"{{pascal(tool)}}Module"}"#,
        ))
        .expect("compiles");

        assert_eq!(
            only_naming_annotation(&compiled).expect("is a naming rule"),
            ["{{pascal(tool)}}Module"]
        );
    }

    #[test]
    fn a_rule_without_an_annotation_compiles_to_an_empty_list() {
        let compiled = compile_json(&tool_rule(r#"{"kind":["const"],"name":"AGENT_TOOL"}"#))
            .expect("compiles");

        assert!(
            only_naming_annotation(&compiled)
                .expect("is a naming rule")
                .is_empty()
        );
    }

    /// The annotation is a template over the same groups the name is, so a
    /// group no pattern defines is the same config bug there -- and one that
    /// would otherwise surface only when a file happened to match.
    #[test]
    fn an_annotation_naming_an_unknown_group_is_refused() {
        let error = compile_json(&tool_rule(
            r#"{"kind":["const"],"name":"AGENT_TOOL","annotation":"{{pascal(entity)}}Module"}"#,
        ))
        .expect_err("refused");

        assert!(matches!(error, CompileError::Template { .. }), "{error:?}");
    }

    /// A function declares a return type, not an annotation, so a rule asking
    /// for both can never be satisfied by any file. Refused at compile time
    /// rather than left to `doctor`: `doctor` exits 0 and gives advice, and
    /// this is not advice -- it is a rule with no satisfying input, which looks
    /// exactly like a repository nobody has migrated yet.
    #[test]
    fn an_annotation_on_a_form_that_cannot_carry_one_is_refused() {
        let error = compile_json(&tool_rule(
            r#"{"kind":["function"],"name":"AGENT_TOOL","annotation":"AgentToolModule"}"#,
        ))
        .expect_err("refused");

        assert!(
            matches!(error, CompileError::UnannotatableKind { .. }),
            "{error:?}"
        );
    }

    /// The forms that do have an annotation position, each accepted. `arrow`
    /// is one: it only ever occurs with `const`, which is annotatable.
    #[test]
    fn every_annotatable_form_is_accepted_beside_an_annotation() {
        for kind in ["const", "let", "var", "arrow", "class", "any"] {
            let json = tool_rule(&format!(
                r#"{{"kind":["{kind}"],"name":"AGENT_TOOL","annotation":"AgentToolModule"}}"#
            ));
            assert!(compile_json(&json).is_ok(), "{kind}");
        }
    }

    #[test]
    fn a_full_config_compiles_into_matchable_rules() {
        let compiled = compile_json(
            r#"{
              "version": 0,
              "modules": [{"id":"domain","rules":[
                {"type":"structure","id":"shape","level":"error",
                 "roots":"packages/domain/src/*",
                 "allowed_subfolders":["types","calcs"]}
              ]}],
              "rules": [
                {"type":"import-boundary","id":"boundary","level":"error",
                 "from":"packages/domain/**",
                 "forbid_import_from":["packages/application/**"]}
              ]
            }"#,
        )
        .expect("compiles");

        assert_eq!(compiled.rule_count(), 2);
        assert_eq!(
            compiled
                .rules_for_file(&path("packages/domain/src/user/user.ts"))
                .count(),
            2,
            "both the structure rule and the boundary cover this file"
        );
    }

    /// The module label survives lowering, so a finding can still say which
    /// module it came from.
    #[test]
    fn a_rules_module_survives_compilation() {
        let compiled = compile_json(
            r#"{"version":0,"modules":[{"id":"domain","rules":[
                {"type":"structure","id":"s","level":"warning","roots":"x/*"}]}]}"#,
        )
        .expect("compiles");

        let rule = compiled.rules().next().expect("one rule");
        assert_eq!(rule.module.as_ref().map(ModuleId::as_str), Some("domain"));
        assert_eq!(rule.level, Level::Warning);
    }

    /// An invalid scope glob is caught here rather than at walk time, and the
    /// message says which rule.
    #[test]
    fn an_invalid_scope_glob_names_its_rule() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"structure","id":"bad-scope","level":"error","roots":"packages/[domain"}]}"#,
        )
        .expect_err("should fail");

        assert!(err.to_string().contains("bad-scope"), "{err}");
    }

    /// The D3 message reaches the user through this layer, with the rule and
    /// the field attached.
    #[test]
    fn an_unsupported_regex_construct_names_the_rule_and_the_field() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"structure","id":"no-lookahead","level":"error","roots":"x/*",
                 "filename_patterns":["^(?!.*spec).*\\.ts$"]}]}"#,
        )
        .expect_err("should fail");

        let (rule, field) = pattern_error(&err).expect("is a Pattern error");
        assert_eq!(rule.as_str(), "no-lookahead");
        assert_eq!(field, "filename_patterns");
        assert!(err.to_string().contains("negative lookahead"), "{err}");
    }

    #[test]
    fn a_naming_rules_file_pattern_is_compiled() {
        let compiled = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"n","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z-]+)\\.use-case\\.ts$",
                 "must_export":{"kind":"function","name":"{{pascal(name)}}"}}]}"#,
        )
        .expect("compiles");

        assert!(compiled.needs_parse());
        assert_eq!(compiled.rule_count(), 1);
    }

    /// `kind: ["function","arrow"]` is how a preset says "callable, either
    /// form", and it has to survive into the compiled filter.
    #[test]
    fn a_list_of_export_kinds_becomes_one_filter() {
        let compiled = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"n","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "must_export":{"kind":["function","arrow"],"name":"{{pascal(name)}}"}}]}"#,
        )
        .expect("compiles");

        let kind = only_naming_kind(&compiled).expect("is a naming rule");

        assert!(kind.accepts(ExportTags::only(ExportKind::Function)));
        assert!(kind.accepts(ExportTags::only(ExportKind::Arrow)));
        assert!(!kind.accepts(ExportTags::only(ExportKind::Class)));
    }

    #[test]
    fn the_any_kind_accepts_every_declaration_form() {
        let compiled = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"n","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "must_export":{"kind":"any","name":"{{pascal(name)}}"}}]}"#,
        )
        .expect("compiles");

        assert!(matches!(
            only_naming_kind(&compiled).expect("is a naming rule"),
            KindFilter::Any
        ));

        // The helper's negative branch: a structure rule is not a naming rule.
        let structural =
            compile_json(r#"{"version":0,"rules":[{"type":"structure","id":"s","level":"error","roots":"x/*"}]}"#)
                .expect("compiles");
        assert!(only_naming_kind(&structural).is_none());
    }

    /// A kind that is not one of the ten lists the valid ones, so the fix does
    /// not need the schema open beside it.
    #[test]
    fn an_unknown_export_kind_lists_the_valid_ones() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"n","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "must_export":{"kind":"callable","name":"X"}}]}"#,
        )
        .expect_err("should fail");

        let message = err.to_string();
        assert!(message.contains("callable"), "{message}");
        assert!(message.contains("function"), "{message}");
        assert!(message.contains("arrow"), "{message}");
        assert!(message.contains("`any`"), "{message}");
    }

    /// The check worth having: a template naming a group the pattern never
    /// defines would otherwise surface only when some file happened to match,
    /// which could be months later.
    #[test]
    fn a_template_referring_to_a_missing_capture_group_is_caught_at_compile_time() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"typo","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "must_export":{"kind":"function","name":"{{pascal(nome)}}"}}]}"#,
        )
        .expect_err("should fail");

        let message = err.to_string();
        assert!(message.contains("typo"), "{message}");
        assert!(message.contains("nome"), "{message}");
    }

    /// `signature_hint` is never verified against code, but it is still a
    /// template, so a typo in it is still caught.
    #[test]
    fn a_signature_hint_template_is_checked_too() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"hint","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "must_export":{"kind":"function","name":"{{pascal(name)}}",
                                "signature_hint":"(deps: {{pascal(missing)}}Deps)"}}]}"#,
        )
        .expect_err("should fail");

        assert!(err.to_string().contains("missing"), "{err}");
    }

    /// The rule from issue #14, through the wire format: a quarantined
    /// dependency and the one directory allowed to reach it.
    #[test]
    fn a_boundary_may_name_a_package_and_exempt_an_importer() {
        let compiled = compile_json(
            r#"{"version":0,"rules":[
                {"type":"import-boundary","id":"three-is-quarantined","level":"error",
                 "from":"src/**",
                 "forbid_import_from_packages":["three"],
                 "except_from":["src/scripts/three/**"]}]}"#,
        )
        .expect("compiles");

        let CompiledRuleKind::ImportBoundary {
            forbid_packages,
            except_from,
            forbid,
            ..
        } = &compiled.rules().next().expect("one rule").kind
        else {
            panic!("is an import-boundary rule");
        };
        assert_eq!(forbid_packages, &["three".to_owned()]);
        assert_eq!(except_from.patterns(), ["src/scripts/three/**".to_owned()]);
        assert!(
            forbid.is_empty(),
            "a package rule puts nothing in the path field"
        );
    }

    /// The rule from issue #16: the entity names the directory, the action
    /// names the file, and both reach the template.
    #[test]
    fn a_template_may_take_a_group_from_the_directory_pattern() {
        let compiled = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"repo-name","level":"error",
                 "roots":"src/Entities/*",
                 "file_pattern":"^(?<action>[a-z0-9-]+)\\.ts$",
                 "dir_pattern":"^(?<entity>[A-Za-z0-9]+)$",
                 "must_export":{"kind":"function",
                                "name":"{{pascal(entity)}}{{pascal(action)}}Repository"}}]}"#,
        )
        .expect("compiles");

        let CompiledRuleKind::Naming { dir_pattern, .. } =
            &compiled.rules().next().expect("one rule").kind
        else {
            panic!("is a naming rule");
        };
        assert_eq!(
            dir_pattern.as_ref().map(Pattern::as_str),
            Some("^(?<entity>[A-Za-z0-9]+)$")
        );
    }

    /// One namespace means one value per group, so a group both patterns
    /// define has no answer. Refused rather than resolved by precedence:
    /// silently preferring the filename would make the rule demand the wrong
    /// export on every file in the scope.
    #[test]
    fn a_capture_group_defined_by_both_patterns_is_refused() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"ambiguous","level":"error",
                 "roots":"src/Entities/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "dir_pattern":"^(?<name>[A-Za-z]+)$",
                 "must_export":{"kind":"function","name":"{{pascal(name)}}"}}]}"#,
        )
        .expect_err("should fail");

        let message = err.to_string();
        assert!(message.contains("ambiguous"), "names the rule: {message}");
        assert!(message.contains("`name`"), "names the group: {message}");
        assert!(
            message.contains("dir_pattern") && message.contains("file_pattern"),
            "names both fields, so the reader knows which to rename: {message}"
        );
    }

    /// And a broken `dir_pattern` is reported against its own field rather
    /// than against `file_pattern`, which is the whole point of passing the
    /// field name down.
    #[test]
    fn an_invalid_directory_pattern_names_its_field() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"broken","level":"error",
                 "roots":"src/Entities/*",
                 "file_pattern":"^(?<action>[a-z]+)\\.ts$",
                 "dir_pattern":"^[unclosed",
                 "must_export":{"kind":"function","name":"{{pascal(action)}}"}}]}"#,
        )
        .expect_err("should fail");

        assert!(err.to_string().contains("dir_pattern"), "{err}");
    }

    #[test]
    fn an_unknown_transform_in_a_template_is_caught() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"t","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "must_export":{"kind":"function","name":"{{pascalcase(name)}}"}}]}"#,
        )
        .expect_err("should fail");

        assert!(err.to_string().contains("pascalcase"), "{err}");
    }

    /// Every glob field on a boundary is compiled, and the message says which
    /// one was wrong -- a boundary has four.
    #[test]
    fn an_invalid_boundary_glob_names_the_field() {
        for (field, json) in [
            ("forbid_import_from", r#""forbid_import_from":["a/[b"]"#),
            ("must_import_from", r#""must_import_from":["a/[b"]"#),
            ("except", r#""except":["a/[b"]"#),
        ] {
            let err = compile_json(&format!(
                r#"{{"version":0,"rules":[
                    {{"type":"import-boundary","id":"b","level":"error","from":"x/**",{json}}}]}}"#
            ))
            .expect_err("should fail");

            assert!(err.to_string().contains(field), "{field}: {err}");
        }
    }

    #[test]
    fn an_invalid_spec_pair_ignore_glob_names_the_field() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"spec-pair","id":"s","level":"error","roots":"src/*",
                 "subfolders":".","ignore_files":["a/[b"]}]}"#,
        )
        .expect_err("should fail");

        assert!(err.to_string().contains("ignore_files"), "{err}");
    }

    #[test]
    fn an_invalid_ignore_or_skip_glob_is_reported_without_a_rule() {
        let err = compile_json(r#"{"version":0,"ignore":["a/[b"]}"#).expect_err("should fail");
        assert!(matches!(err, CompileError::Ignore { .. }), "{err:?}");

        let err = compile_json(r#"{"version":0,"skip_dirs":{"globs":["a/[b"]}}"#)
            .expect_err("should fail");
        assert!(matches!(err, CompileError::SkipDirs { .. }), "{err:?}");
    }

    /// `skip_dirs` reaches the compiled config intact, including the scope
    /// that decides whether the exemption is structural or total.
    #[test]
    fn skip_dirs_are_carried_through_with_their_scope() {
        let compiled =
            compile_json(r#"{"version":0,"skip_dirs":{"prefixes":["_"],"scope":"walk"}}"#)
                .expect("compiles");

        assert_eq!(compiled.skip_dirs().scope, SkipScope::Walk);
        assert!(compiled.skip_dirs().exempts(&path("src/_internal")));
    }

    /// The rules hash is what the `findings` cache key folds in, so a change
    /// to any rule must move it and an unrelated edit must not.
    #[test]
    fn the_rules_hash_tracks_the_rules_and_nothing_else() {
        let base = r#"{"version":0,"rules":[
            {"type":"structure","id":"a","level":"error","roots":"x/*"}]}"#;
        let same_rules_different_ignore = r#"{"version":0,"ignore":["**/dist/**"],"rules":[
            {"type":"structure","id":"a","level":"error","roots":"x/*"}]}"#;
        let changed_level = r#"{"version":0,"rules":[
            {"type":"structure","id":"a","level":"warning","roots":"x/*"}]}"#;

        let hash = |json: &str| compile_json(json).expect("compiles").rules_hash();

        assert_eq!(hash(base), hash(base), "deterministic");
        assert_eq!(
            hash(base),
            hash(same_rules_different_ignore),
            "ignore is not part of the rule set"
        );
        assert_ne!(
            hash(base),
            hash(changed_level),
            "a level change is a change"
        );
    }

    /// A config with no rules compiles to an empty set rather than failing.
    /// `archwarden init` writes one, and it should validate.
    #[test]
    fn an_empty_config_compiles_to_nothing() {
        let compiled = compile_json(r#"{"version":0}"#).expect("compiles");

        assert_eq!(compiled.rule_count(), 0);
        assert!(!compiled.needs_parse());
        assert_eq!(compiled.rules_for_file(&path("src/x.ts")).count(), 0);
    }

    /// `disable` is applied before lowering, so a disabled rule is never
    /// compiled and can never fire.
    #[test]
    fn a_disabled_rule_is_not_compiled() {
        let compiled = compile_json(
            r#"{"version":0,"disable":["off"],"rules":[
                {"type":"structure","id":"on","level":"error","roots":"x/*"},
                {"type":"structure","id":"off","level":"error","roots":"y/*"}]}"#,
        )
        .expect("compiles");

        let ids: Vec<_> = compiled.rules().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["on"]);
    }
}

#[cfg(test)]
mod import_filter_tests {
    use super::compile;
    use crate::extends::MergedConfig;

    fn compiled(rule: &str) -> archwarden_core::compiled::CompiledConfig {
        let source = format!(r#"{{"version":0,"rules":[{rule}]}}"#);
        let config =
            crate::discovery::parse(camino::Utf8Path::new("/repo/arch.config.json"), &source)
                .expect("parses");

        compile(&MergedConfig {
            config,
            path: camino::Utf8PathBuf::from("/repo/arch.config.json"),
            root: camino::Utf8PathBuf::from("/repo"),
            sources: Vec::new(),
        })
        .expect("compiles")
    }

    /// The filter survives compilation and matches what it was written to
    /// match. Decision 25.
    #[test]
    fn a_narrowed_rule_arrives_with_its_filter() {
        let config = compiled(
            r#"{"type":"presence","id":"p","level":"error","roots":["src/*"],
                "when_importing":"src/http/**","require":["contract.md"]}"#,
        );
        let rule = config.rules().next().expect("one rule");

        let filter = rule.imports.as_ref().expect("the filter is compiled");
        assert!(
            filter
                .paths
                .is_match(camino::Utf8Path::new("src/http/conn.ts"))
        );
        assert!(
            !filter
                .paths
                .is_match(camino::Utf8Path::new("src/db/pool.ts"))
        );
    }

    /// Packages alone are enough to narrow. Without this the two halves would
    /// have to be written together, which is not what either means.
    #[test]
    fn packages_alone_narrow_a_rule() {
        let config = compiled(
            r#"{"type":"presence","id":"p","level":"error","roots":["src/*"],
                "when_importing_packages":["zod"],"require":["contract.md"]}"#,
        );
        let rule = config.rules().next().expect("one rule");

        let filter = rule.imports.as_ref().expect("packages narrow too");
        assert_eq!(filter.packages, ["zod"]);
    }

    /// And a rule that names neither carries no filter at all. `None` rather
    /// than an empty one: "does not narrow" and "narrows to nothing" are
    /// different statements, and only one of them should cost a resolution
    /// pass.
    #[test]
    fn a_rule_that_names_neither_carries_no_filter() {
        let config = compiled(
            r#"{"type":"presence","id":"p","level":"error","roots":["src/*"],
                "require":["contract.md"]}"#,
        );

        assert!(config.rules().next().expect("one rule").imports.is_none());
    }

    /// A glob the engine refuses fails here, naming the field — rather than at
    /// the first file the rule is asked about.
    #[test]
    fn a_glob_that_will_not_compile_names_the_field() {
        let source = r#"{"version":0,"rules":[
            {"type":"presence","id":"p","level":"error","roots":["src/*"],
             "when_importing":"src/[","require":["contract.md"]}]}"#;
        let config =
            crate::discovery::parse(camino::Utf8Path::new("/repo/arch.config.json"), source)
                .expect("parses");

        let refusal = compile(&MergedConfig {
            config,
            path: camino::Utf8PathBuf::from("/repo/arch.config.json"),
            root: camino::Utf8PathBuf::from("/repo"),
            sources: Vec::new(),
        })
        .expect_err("an unparsable glob is refused");

        assert!(refusal.to_string().contains("when_importing"), "{refusal}");
    }
}
