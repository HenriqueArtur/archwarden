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
    /// The decisions the covered rules serve, in declaration order.
    ///
    /// Before the rules, and carrying the ids of the rules under each, because
    /// that is the shape of the answer: a digest that is a flat list of
    /// prohibitions is a list an agent works around, and one that says what
    /// was decided and what enforces it is one it can argue with. Issue #100.
    ///
    /// Narrowed with the rules. A guide restricted to `packages/domain` lists
    /// the decisions bearing on that directory — telling an agent asking about
    /// one folder that eleven decisions apply is the noise the filter exists
    /// to remove.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<GuideDecision<'a>>,
    /// Every rule it covers, in configuration order.
    pub rules: Vec<GuideRule<'a>>,
}

/// One decision, as the digest carries it.
#[derive(Debug, Serialize)]
pub struct GuideDecision<'a> {
    /// The reference, such as `ADR-014`.
    pub id: &'a str,
    /// What was decided, in one line.
    pub title: &'a str,
    /// Why, when the config said it here rather than only behind `link`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<&'a str>,
    /// Where it is written down.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<&'a str>,
    /// `accepted`, `proposed` or `superseded`. Always present: a consumer
    /// branches on it, unlike a terminal reader who only needs to hear about
    /// the unusual ones.
    pub status: &'a str,
    /// The rules serving it, in configuration order.
    ///
    /// Ids rather than the rules themselves: the rule is spelled out once, in
    /// `rules`, and a digest that carried it twice would be a digest that can
    /// disagree with itself. Empty is a real answer — a decision nobody
    /// enforces is a thing to know about an architecture, and `config doctor`
    /// is what calls it debt.
    pub rules: Vec<&'a str>,
    /// How many findings the baseline excuses against this decision's rules.
    ///
    /// `None` when no baseline was consulted, which is a different fact from a
    /// baseline that excuses nothing — the distinction `summary.imports`
    /// already draws for resolution.
    ///
    /// Counted off the file rather than off a run: this digest describes the
    /// architecture as declared and does not walk anything, and how much debt
    /// a decision carries is a property of what was committed. What that debt
    /// is *doing today* is `config explain`'s answer, and it costs a check.
    /// Issue #112.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excused: Option<usize>,
    /// The decisions this one replaced.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub supersedes: Vec<&'a str>,
    /// The decisions that replaced it. Computed, never written. Issue #115.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub superseded_by: Vec<&'a str>,
    /// What was considered and rejected, in the order it was written.
    ///
    /// The half that stops the losing option being proposed again — by the
    /// next person, or by an agent that reads this digest, complies, and
    /// helpfully suggests the thing that was already tried. Issue #114.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<GuideAlternative<'a>>,
}

/// One option a decision weighed and did not take, as the digest carries it.
#[derive(Debug, Serialize)]
pub struct GuideAlternative<'a> {
    /// The option, named as the team named it.
    pub option: &'a str,
    /// Why it lost.
    pub why_not: &'a str,
    /// The rule that refuses it today. Absent when nothing does, which is a
    /// true and useful thing for a reader to know.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused_by: Option<&'a str>,
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
    /// The decision it implements, by id.
    ///
    /// The id alone, because the prose is in `decisions` and a reader who
    /// starts at a rule needs to know *which* decision, not to have it
    /// repeated under every rule that serves it. Issue #100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<&'a str>,
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
pub const KINDS: [&str; 15] = [
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
    "call-matches-export",
    "export-shape",
    "frozen",
    "mirror",
    "metadata",
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
    baseline: Option<&crate::baseline::Baseline>,
) -> Guide<'a> {
    let rules: Vec<GuideRule<'a>> = covered(config, scope, kinds).collect();

    Guide {
        version: GUIDE_VERSION,
        scope: scope.map(RepoRelPath::as_str),
        kinds: kinds.iter().map(String::as_str).collect(),
        decisions: decisions_of(config, &rules, baseline),
        rules,
    }
}

/// The decisions the covered rules serve, with those rules under each.
///
/// Read off the *covered* rules rather than off the config, which is what
/// makes a scoped or kind-filtered guide carry the decisions bearing on what
/// was asked about and no others. A decision left serving none of them is
/// dropped; one the whole config declares and nobody enforces is kept, because
/// an unenforced decision is a fact about the architecture and a decision
/// whose rules are all out of scope is not.
fn decisions_of<'a>(
    config: &'a CompiledConfig,
    covered: &[GuideRule<'a>],
    baseline: Option<&crate::baseline::Baseline>,
) -> Vec<GuideDecision<'a>> {
    let excused_by_rule = baseline.map(crate::baseline::Baseline::excused_by_rule);

    config
        .decisions()
        .filter_map(|decision| {
            let serving: Vec<&'a str> = covered
                .iter()
                .filter(|rule| rule.decision == Some(decision.id.as_str()))
                .map(|rule| rule.id)
                .collect();

            let enforced_anywhere = config
                .rules()
                .any(|rule| rule.decision.as_ref() == Some(&decision.id));
            (!serving.is_empty() || !enforced_anywhere).then_some(GuideDecision {
                id: decision.id.as_str(),
                title: decision.title.as_str(),
                why: decision.why.as_deref(),
                link: decision.link.as_deref(),
                status: decision.status.as_str(),
                // Every rule the *config* names, not only the ones this digest
                // is scoped to: the debt a decision carries is not narrowed by
                // asking about one directory, and a number that shrank with
                // the question would be read as progress.
                excused: excused_by_rule.as_ref().map(|excused| {
                    config
                        .rules()
                        .filter(|rule| rule.decision.as_ref() == Some(&decision.id))
                        .filter_map(|rule| excused.get(rule.id.as_str()))
                        .sum()
                }),
                supersedes: decision
                    .supersedes
                    .iter()
                    .map(archwarden_core::ids::DecisionId::as_str)
                    .collect(),
                superseded_by: decision
                    .superseded_by
                    .iter()
                    .map(archwarden_core::ids::DecisionId::as_str)
                    .collect(),
                alternatives: decision
                    .alternatives
                    .iter()
                    .map(|alternative| GuideAlternative {
                        option: alternative.option.as_str(),
                        why_not: alternative.why_not.as_str(),
                        refused_by: alternative
                            .refused_by
                            .as_ref()
                            .map(archwarden_core::ids::RuleId::as_str),
                    })
                    .collect(),
                rules: serving,
            })
        })
        .collect()
}

/// The rules a guide covers, after both filters.
fn covered<'a>(
    config: &'a CompiledConfig,
    scope: Option<&'a RepoRelPath>,
    kinds: &'a [String],
) -> impl Iterator<Item = GuideRule<'a>> {
    config
        .rules()
        .filter(move |rule| kinds.is_empty() || kinds.iter().any(|k| k == rule.kind.type_name()))
        .filter(move |rule| scope.is_none_or(|prefix| reaches(rule, prefix)))
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
            decision: rule
                .decision
                .as_ref()
                .map(archwarden_core::ids::DecisionId::as_str),
        })
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
        CompiledRuleKind::CallMatchesExport {
            callee,
            argument,
            attribute,
            report_uncalled,
            ..
        } => {
            let held = attribute
                .as_ref()
                .map_or_else(String::new, |name| format!(", carrying `#[{name}]`"));
            let mut lines = vec![format!(
                "every `{callee}(...)` names, in argument {argument}, something \
                 declared in the rule's `declared_in` scope{held}"
            )];
            if *report_uncalled {
                lines.push(format!(
                    "and every such declaration is named by a `{callee}` somewhere"
                ));
            }
            lines
        }
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
            ignore_files: _,
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
        CompiledRuleKind::Metadata {
            require,
            one_of,
            equals,
            deadline,
        } => {
            let mut lines = vec![
                "claims go as `// archwarden-<key>: value` above the first statement".to_owned(),
            ];
            if !require.is_empty() {
                let quoted: Vec<String> = require.iter().map(|k| format!("`{k}`")).collect();
                lines.push(format!(
                    "files declare, in a header comment: {}",
                    quoted.join(", ")
                ));
            }
            for (key, accepted) in one_of {
                let quoted: Vec<String> = accepted.iter().map(|v| format!("`{v}`")).collect();
                lines.push(format!("`{key}` one of: {}", quoted.join(", ")));
            }
            for (key, template) in equals {
                lines.push(format!("`{key}` equal to `{template}`"));
            }
            for key in deadline {
                lines.push(format!(
                    "`{key}` is a date `YYYY-MM-DD` that has not passed"
                ));
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
        CompiledRuleKind::Frozen => vec![
            "has stopped growing: no file may be added under it".to_owned(),
            "what is here today is accepted by the baseline; a path it does not \
             carry is reported"
                .to_owned(),
        ],

        CompiledRuleKind::Mirror {
            file_pattern,
            must_exist,
        } => vec![format!(
            "every file matching {} must have a counterpart at {}",
            format!("`{}`", file_pattern.as_str()),
            format!("`{must_exist}`"),
        )],

        CompiledRuleKind::ExportShape(shape) => {
            let mut lines = Vec::new();
            if shape.forbid_default {
                lines.push("must not export a default".to_owned());
            }
            if let Some(limit) = shape.max_exports {
                lines.push(format!(
                    "must export at most {limit} {} (`type` and `interface` do not count)",
                    if limit == 1 { "symbol" } else { "symbols" }
                ));
            }
            if !shape.must_return.is_empty() {
                lines.push(format!(
                    "every exported function must declare a return type matching {}",
                    join(
                        &shape
                            .must_return
                            .iter()
                            .map(|pattern| pattern.as_str().to_owned())
                            .collect::<Vec<_>>()
                    )
                ));
            }
            lines
        }

        CompiledRuleKind::Chokepoint { callee, only_in } => vec![format!(
            "only files under {} may call {}",
            join(only_in.patterns()),
            join(callee),
        )],

        CompiledRuleKind::CallObligation {
            file_pattern,
            symbol,
            imported_from,
            with_options,
        } => {
            // One sentence about one call: appending rather than adding a
            // line, which would read as a second obligation.
            vec![format!(
                "files matching `{}` must call `{symbol}`, imported from \
                 `{imported_from}`{}",
                file_pattern.as_str(),
                passing(
                    with_options
                        .iter()
                        .map(|(key, value)| (key, value.as_ref()))
                )
            )]
        }
    }
}

/// The clause naming the options a call has to carry, or nothing.
///
/// Shared by the two sentences that say it -- the guide's and the report's --
/// because a rule worded one way in the instructions and another in the
/// failure is a rule read twice. Issue #164.
pub fn passing<'a>(options: impl Iterator<Item = (&'a String, Option<&'a String>)>) -> String {
    let named: Vec<String> = options
        .map(|(key, value)| match value {
            Some(value) => format!("`{key}: {value}`"),
            None => format!("`{key}`"),
        })
        .collect();

    if named.is_empty() {
        return String::new();
    }
    format!(", passing {}", named.join(" and "))
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
        compiled::{CompiledDecision, CompiledRule, DecisionStatus, SkipDirs},
        facts::{ExportTags, KindFilter},
        glob::PathSet,
        hash::ContentHash,
        ids::{DecisionId, ModuleId, RuleId},
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
            decision: None,
            imports: None,
            directives: None,
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
            ignore_files: archwarden_core::glob::PathSet::default(),
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
        guide(config, scope, &owned, None)
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
        serde_json::to_value(guide(config, scope, &[], None)).expect("serialises")
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

    /// Issue #104. The digest is what an agent has before it writes a file,
    /// and a claim it does not know to make is one nothing prompts it for.
    #[test]
    fn a_metadata_rule_lists_what_the_header_must_declare() {
        let config = config(vec![rule(
            "payments-owned",
            None,
            &["src/payments/**"],
            CompiledRuleKind::Metadata {
                require: vec!["owner".to_owned()],
                one_of: vec![(
                    "stability".to_owned(),
                    vec!["stable".to_owned(), "experimental".to_owned()],
                )],
                equals: vec![("module".to_owned(), "{{raw(dirname)}}".to_owned())],
                deadline: vec!["remove-by".to_owned()],
            },
        )]);

        let markdown = sentences(&config, None, &[]);

        assert!(
            markdown.contains("files declare, in a header comment: `owner`"),
            "{markdown}"
        );
        assert!(
            markdown.contains("`stability` one of: `stable`, `experimental`"),
            "{markdown}"
        );
        assert!(
            markdown.contains("`module` equal to `{{raw(dirname)}}`"),
            "{markdown}"
        );
    }

    /// The line an agent needs most, because it cannot guess the spelling: the
    /// marker is `// archwarden-<key>:` and it lives above the first statement.
    #[test]
    fn a_metadata_rule_says_where_the_claims_go() {
        let config = config(vec![rule(
            "payments-owned",
            None,
            &["src/payments/**"],
            CompiledRuleKind::Metadata {
                require: vec!["owner".to_owned()],
                one_of: Vec::new(),
                equals: Vec::new(),
                deadline: Vec::new(),
            },
        )]);

        assert!(
            sentences(&config, None, &[])
                .contains("as `// archwarden-<key>: value` above the first statement"),
            "{}",
            sentences(&config, None, &[])
        );
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
            ignore_files: archwarden_core::glob::PathSet::default(),
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
            CompiledRuleKind::CallMatchesExport {
                callee: "invoke".to_owned(),
                argument: 0,
                declared_in: Scope::compile(["src-tauri/src/**"]).expect("valid scope"),
                attribute: Some("tauri::command".to_owned()),
                report_uncalled: false,
            },
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
                with_options: Vec::new(),
            },
            CompiledRuleKind::ExportShape(archwarden_core::compiled::ExportShape {
                forbid_default: true,
                max_exports: Some(1),
                must_return: vec![
                    Pattern::compile(r"^ResponsePattern<.+,.+>$").expect("valid pattern"),
                ],
            }),
            CompiledRuleKind::Frozen,
            CompiledRuleKind::Mirror {
                file_pattern: Pattern::compile(r"^(?<name>[a-z-]+)\.ts$").expect("valid pattern"),
                must_exist: "migrations/{{raw(name)}}.sql".to_owned(),
            },
            CompiledRuleKind::Metadata {
                require: Vec::new(),
                one_of: Vec::new(),
                equals: Vec::new(),
                deadline: Vec::new(),
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

        assert_eq!(guide(&config, Some(&scope), &[], None).rules.len(), 1);
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

        assert_eq!(guide(&config, Some(&scope), &[], None).rules.len(), 1);
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
        let built = guide(&config, Some(&scope), &[], None);
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

        assert_eq!(guide(&config, None, &[], None).rules.len(), 2);
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
        assert_eq!(guide(&config, Some(&root), &[], None).rules.len(), 2);
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

    /// Issue #100. The digest is what an agent has instead of the config, and
    /// after this it carries the decisions with the rules that serve them
    /// under each — rather than a flat list of ids, which is a list of
    /// prohibitions and that is what an agent works around.
    #[test]
    fn the_digest_carries_the_decisions_with_their_rules_under_them() {
        let mut sealed = rule("domain-forbids-http", None, &["src/*"], naming());
        sealed.decision = Some(DecisionId::new("ADR-014").expect("valid"));
        let mut helper = rule("writes-go-through-the-helper", None, &["src/*"], naming());
        helper.decision = Some(DecisionId::new("ADR-014").expect("valid"));
        let loose = rule("unattached", None, &["src/*"], naming());

        let json = as_json(
            &config(vec![sealed, helper, loose]).with_decisions(vec![CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-014").expect("valid"),
                title: "The domain does not know about transport".to_owned(),
                why: Some("it is published".to_owned()),
                link: Some("docs/adr/014.md".to_owned()),
                status: DecisionStatus::Accepted,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                alternatives: Vec::new(),
            }]),
            None,
        );

        let decision = &json["decisions"][0];
        assert_eq!(decision["id"], "ADR-014");
        assert_eq!(
            decision["title"],
            "The domain does not know about transport"
        );
        assert_eq!(decision["why"], "it is published");
        assert_eq!(decision["link"], "docs/adr/014.md");
        assert_eq!(decision["status"], "accepted");
        assert_eq!(
            decision["rules"],
            serde_json::json!(["domain-forbids-http", "writes-go-through-the-helper"]),
            "the rules under it, in configuration order: {json}"
        );

        assert_eq!(
            json["rules"][0]["decision"], "ADR-014",
            "and each rule still names its own, so a reader who starts at a \
             rule is not sent to another section to find out: {json}"
        );
        assert!(
            json["rules"][2].get("decision").is_none(),
            "a rule that names none omits the key: {json}"
        );
    }

    /// The digest states each claim an `export-shape` rule makes, and only
    /// the ones it makes. A digest is what an agent has instead of the config,
    /// so a claim it omits is a claim the agent will break. Issue #101.
    #[test]
    fn an_export_shape_rule_states_each_claim_it_makes() {
        let all_three = CompiledRuleKind::ExportShape(archwarden_core::compiled::ExportShape {
            forbid_default: true,
            max_exports: Some(1),
            must_return: vec![
                Pattern::compile(r"^ResponsePattern<.+,.+>$").expect("valid pattern"),
                Pattern::compile(r"^Result<.+>$").expect("valid pattern"),
            ],
        });
        let markdown = sentences(
            &config(vec![rule("shape", None, &["src/*"], all_three)]),
            None,
            &[],
        );

        assert!(markdown.contains("must not export a default"), "{markdown}");
        assert!(
            markdown.contains("must export at most 1 symbol (`type` and `interface` do not count)"),
            "{markdown}"
        );
        assert!(
            markdown.contains("every exported function must declare a return type matching"),
            "{markdown}"
        );
        assert!(
            markdown.contains("^Result<.+>$"),
            "every pattern in the list, not only the first: {markdown}"
        );

        // The plural, which is the other half of the sentence.
        let two = CompiledRuleKind::ExportShape(archwarden_core::compiled::ExportShape {
            forbid_default: false,
            max_exports: Some(2),
            must_return: Vec::new(),
        });
        let plural = sentences(
            &config(vec![rule("shape", None, &["src/*"], two)]),
            None,
            &[],
        );
        assert!(plural.contains("must export at most 2 symbols"), "{plural}");
        assert!(
            !plural.contains("must not export a default"),
            "a claim the rule does not make is not stated: {plural}"
        );
    }

    /// A decision nobody enforces is still in the digest. The guide describes
    /// the architecture, and a decision with no rules is a real thing to know
    /// about it — `config doctor` is what calls it debt.
    #[test]
    fn a_decision_no_rule_serves_is_still_in_the_digest() {
        let json = as_json(
            &config(vec![rule("shape", None, &["src/*"], naming())]).with_decisions(vec![
                CompiledDecision {
                    scope: None,
                    why_not_enforceable: None,
                    id: DecisionId::new("ADR-020").expect("valid"),
                    title: "Nobody enforces this".to_owned(),
                    why: None,
                    link: None,
                    status: DecisionStatus::Accepted,
                    supersedes: Vec::new(),
                    superseded_by: Vec::new(),
                    alternatives: Vec::new(),
                },
            ]),
            None,
        );

        assert_eq!(json["decisions"][0]["id"], "ADR-020");
        assert_eq!(
            json["decisions"][0]["rules"],
            serde_json::json!([]),
            "an empty list, said out loud: {json}"
        );
    }

    /// Issue #112. The digest describes the architecture as declared, and the
    /// one thing it could never say is how much of it this repository still
    /// excuses. Counted off the committed file, because this walks nothing.
    #[test]
    fn a_decision_carries_the_debt_the_baseline_holds_against_it() {
        let mut serving = rule("shape", None, &["src/*"], naming());
        serving.decision = Some(DecisionId::new("ADR-014").expect("valid"));
        let mut also = rule("second", None, &["src/*"], naming());
        also.decision = Some(DecisionId::new("ADR-014").expect("valid"));
        let mut elsewhere = rule("other", None, &["src/*"], naming());
        elsewhere.decision = Some(DecisionId::new("ADR-031").expect("valid"));

        let config = config(vec![serving, also, elsewhere]).with_decisions(vec![
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-014").expect("valid"),
                title: "Carries debt".to_owned(),
                why: None,
                link: None,
                status: DecisionStatus::Accepted,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                alternatives: Vec::new(),
            },
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-031").expect("valid"),
                title: "Carries none".to_owned(),
                why: None,
                link: None,
                status: DecisionStatus::Accepted,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                alternatives: Vec::new(),
            },
        ]);

        let baseline: crate::baseline::Baseline = serde_json::from_value(serde_json::json!({
            "version": 0,
            "accepted": [
                { "rule": "shape", "path": "src/a", "note": "" },
                { "rule": "shape", "path": "src/b", "note": "" },
                { "rule": "second", "path": "src/c", "note": "" },
                { "rule": "unrelated", "path": "src/d", "note": "" },
            ],
        }))
        .expect("a baseline");

        let digest = guide(&config, None, &[], Some(&baseline));
        let carrying = digest
            .decisions
            .iter()
            .find(|decision| decision.id == "ADR-014")
            .expect("the decision with debt");
        let clean = digest
            .decisions
            .iter()
            .find(|decision| decision.id == "ADR-031")
            .expect("the decision without");

        assert_eq!(
            carrying.excused,
            Some(3),
            "both its rules are summed, and a rule serving nobody is not"
        );
        assert_eq!(
            clean.excused,
            Some(0),
            "a decision with no debt says zero rather than nothing: the \
             baseline was consulted and the answer is none"
        );
    }

    /// And a digest built without one says nothing about debt, which is a
    /// different fact from a baseline that excuses nothing.
    #[test]
    fn a_digest_with_no_baseline_omits_the_debt() {
        let mut serving = rule("shape", None, &["src/*"], naming());
        serving.decision = Some(DecisionId::new("ADR-014").expect("valid"));

        let json = as_json(
            &config(vec![serving]).with_decisions(vec![CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-014").expect("valid"),
                title: "Unmeasured".to_owned(),
                why: None,
                link: None,
                status: DecisionStatus::Accepted,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                alternatives: Vec::new(),
            }]),
            None,
        );

        assert!(
            json["decisions"][0].get("excused").is_none(),
            "absent, not zero: {json}"
        );
    }

    /// A config with no decisions omits the key, which is every config written
    /// before 0.21 and the shape their committed guides already have."""
    #[test]
    fn a_guide_with_no_decisions_omits_the_key() {
        let json = as_json(
            &config(vec![rule("shape", None, &["src/*"], naming())]),
            None,
        );

        assert!(json.get("decisions").is_none(), "{json}");
    }

    /// Narrowing the guide to a scope narrows the rules under each decision
    /// too, and drops a decision left serving none of them.
    ///
    /// The alternative — listing every decision whatever the scope — would
    /// tell an agent asking about one directory that eleven decisions bear on
    /// it, which is the noise the scope filter exists to remove.
    #[test]
    fn a_scoped_guide_carries_only_the_decisions_its_rules_serve() {
        let mut here = rule("here", None, &["src/*"], naming());
        here.decision = Some(DecisionId::new("ADR-014").expect("valid"));
        let mut elsewhere = rule("elsewhere", None, &["docs/*"], naming());
        elsewhere.decision = Some(DecisionId::new("ADR-020").expect("valid"));

        let scope = path("src");
        let json = as_json(
            &config(vec![here, elsewhere]).with_decisions(vec![
                CompiledDecision {
                    scope: None,
                    why_not_enforceable: None,
                    id: DecisionId::new("ADR-014").expect("valid"),
                    title: "reached".to_owned(),
                    why: None,
                    link: None,
                    status: DecisionStatus::Accepted,
                    supersedes: Vec::new(),
                    superseded_by: Vec::new(),
                    alternatives: Vec::new(),
                },
                CompiledDecision {
                    scope: None,
                    why_not_enforceable: None,
                    id: DecisionId::new("ADR-020").expect("valid"),
                    title: "not reached".to_owned(),
                    why: None,
                    link: None,
                    status: DecisionStatus::Accepted,
                    supersedes: Vec::new(),
                    superseded_by: Vec::new(),
                    alternatives: Vec::new(),
                },
            ]),
            Some(&scope),
        );

        let ids: Vec<&str> = json["decisions"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|d| d["id"].as_str().expect("an id"))
            .collect();
        assert_eq!(ids, ["ADR-014"], "{json}");
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
    /// Issue #118. The guide is what an agent reads before it writes, and a
    /// chokepoint is a rule it cannot infer from the file in front of it --
    /// nothing in `src/orders` says that `src/config` is the one place that
    /// reads the environment.
    #[test]
    fn a_chokepoint_names_the_capability_and_the_one_place_for_it() {
        let config = config(vec![rule(
            "the-environment-is-read-once",
            None,
            &["src/*"],
            CompiledRuleKind::Chokepoint {
                callee: vec!["process.env".to_owned(), "process.argv".to_owned()],
                only_in: Scope::compile(["src/config/**"]).expect("valid scope"),
            },
        )]);

        let markdown = sentences(&config, None, &[]);
        assert!(
            markdown.contains("only files under `src/config/**` may call"),
            "{markdown}"
        );
        assert!(markdown.contains("process.env"), "{markdown}");
        assert!(markdown.contains("process.argv"), "{markdown}");
    }

    /// Issue #164. An agent reads the guide before it writes, so a call whose
    /// options are missing from the sentence is a call it writes wrong -- and
    /// the same clause is what the failure says, because a rule worded one way
    /// in the instructions and another in the report is a rule read twice.
    #[test]
    fn a_call_obligations_options_are_in_the_sentence() {
        let config = config(vec![rule(
            "specs-run-in-memory",
            None,
            &["tests"],
            CompiledRuleKind::CallObligation {
                file_pattern: Pattern::compile(r"\.api\.spec\.ts$").expect("valid"),
                symbol: "FactoryMockDependencies".to_owned(),
                imported_from: "../test/factories".to_owned(),
                with_options: vec![
                    ("PAY_IN_MEMORY".to_owned(), None),
                    ("strict".to_owned(), Some("true".to_owned())),
                ],
            },
        )]);

        let markdown = sentences(&config, None, &[]);
        assert!(
            markdown.contains("passing `PAY_IN_MEMORY` and `strict: true`"),
            "{markdown}"
        );
    }

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
                    with_options: Vec::new(),
                },
            ),
        ]);

        let built = guide(&config, None, &[], None);
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
                    ignore_files: archwarden_core::glob::PathSet::default(),
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
        let digest = guide(&config, None, &[], None);
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
                    ignore_files: archwarden_core::glob::PathSet::default(),
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

    /// A call-matching rule states the callee, the argument and the marker.
    ///
    /// The digest is what an agent reads before writing code, and a rule about
    /// a seam it cannot see in either file is the one it most needs told. Both
    /// branches, because the second direction is a sentence of its own and a
    /// rule with it off must not claim it.
    #[test]
    fn a_call_matching_rule_states_the_seam_it_is_about() {
        let of = |report_uncalled| {
            sentences(
                &config(vec![rule(
                    "ipc",
                    None,
                    &["src/**"],
                    CompiledRuleKind::CallMatchesExport {
                        callee: "invoke".to_owned(),
                        argument: 0,
                        declared_in: Scope::compile(["src-tauri/src/**"]).expect("valid scope"),
                        attribute: Some("tauri::command".to_owned()),
                        report_uncalled,
                    },
                )]),
                None,
                &[],
            )
        };

        let quiet = of(false);
        assert!(quiet.contains("`invoke(...)`"), "{quiet}");
        assert!(quiet.contains("argument 0"), "{quiet}");
        assert!(quiet.contains("`#[tauri::command]`"), "{quiet}");
        assert!(
            !quiet.contains("named by a"),
            "the second direction is off and is not claimed: {quiet}"
        );

        let both = of(true);
        assert!(both.contains("named by a `invoke`"), "{both}");
    }

    /// A rule naming no attribute says nothing about one.
    #[test]
    fn a_call_matching_rule_with_no_attribute_claims_none() {
        let said = sentences(
            &config(vec![rule(
                "catalogue",
                None,
                &["src/**"],
                CompiledRuleKind::CallMatchesExport {
                    callee: "t".to_owned(),
                    argument: 0,
                    declared_in: Scope::compile(["strings/**"]).expect("valid scope"),
                    attribute: None,
                    report_uncalled: false,
                },
            )]),
            None,
            &[],
        );

        assert!(said.contains("`t(...)`"), "{said}");
        assert!(!said.contains("carrying"), "{said}");
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
