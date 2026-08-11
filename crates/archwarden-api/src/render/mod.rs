//! Turning a run into bytes, in whatever shape the reader is.
//!
//! One trait, one implementation per shape. `report::render` used to be a
//! two-arm match, with the HTML page reached by a separate path entirely —
//! and SARIF (#64) would have been a fourth branch in several places rather
//! than one more implementation. That is the test the project already applies
//! to its seams: `Parser` is a trait because there are three front-ends,
//! `RuleEngine` because there are nine. This one has earned it.
//!
//! # What lives here and what does not
//!
//! The machine-readable shapes, because a shape a program consumes is a
//! contract and MCP has to emit the same one `check --format json` does — an
//! MCP server reaching into `archwarden-cli` for the report format would be a
//! dependency pointing the wrong way.
//!
//! Human text and the HTML page stay in `archwarden-cli`. They are that
//! surface's own: the text renderer resolves byte offsets into `line:column`
//! by reading source files for a terminal, and the page carries the phrase
//! tables the config's `language` selects. Both implement [`Renderer`] from
//! there, which is what the trait is for.

use archwarden_engine::run::Report;
use camino::Utf8Path;
use serde::Serialize;

use crate::present::{Breakdown, View};

pub mod json;

pub use json::Json;

/// One shape a run can be written in.
///
/// `&dyn` at the call site rather than a generic: a surface picks a renderer
/// from a flag at runtime, so monomorphising over the choice would buy
/// nothing and force every caller to name the type it chose.
pub trait Renderer {
    /// Writes the run.
    ///
    /// Returns nothing, and takes the sink rather than producing bytes,
    /// because a report over a large repository is large and a caller
    /// streaming it to stdout should not have to hold it first.
    ///
    /// **This is the one place in `archwarden-api` that writes**, and it
    /// writes only where the caller pointed it. Nothing here reaches a
    /// terminal on its own, and no failure is reported by writing — a
    /// renderer that cannot render says so in the shape it renders.
    fn render(&self, rendered: &Rendered<'_>, out: &mut dyn std::io::Write);

    /// Writes a single-file check.
    ///
    /// A second method rather than a second trait, because `check --file`,
    /// the pre-write hook and an editor all ask about one file and every
    /// format has to answer them. The shapes differ from a whole-repository
    /// report — this one carries `skipped` and the imports nothing could
    /// resolve, which a full run reports in its summary instead — so it is a
    /// shape of its own and not a report with one finding in it.
    fn render_single(
        &self,
        single: &archwarden_engine::single::Single,
        reasons: &Reasons,
        out: &mut dyn std::io::Write,
    );
}

/// The version of the JSON report shape.
///
/// Bumped when a consumer would have to change to keep reading it. Adding a
/// field is not a bump; removing or repurposing one is.
///
/// Not bumped when `unimplemented_rules` was removed in M6, and the exception
/// is worth stating: the field was omitted from every clean report anyway, it
/// could only ever appear in a state no released build could reach, and
/// archwarden has not been released. Version 0 is still the first shape any
/// consumer will see. A field removal after release is a bump.
///
/// Not bumped for `summary.duration_ms` either: a consumer that ignores it
/// reads the report exactly as before.
pub const REPORT_VERSION: u32 = 0;
/// The standing reason behind each rule, by rule id.
///
/// Looked up when a report is rendered rather than carried on a
/// [`Finding`](archwarden_core::finding::Finding),
/// deliberately. A `why` is prose about a *rule*; a finding is about a file,
/// and copying the prose onto every one of them would put it in the baseline's
/// path -- `.archwarden/baseline.json` must not churn because somebody
/// reworded a sentence -- and would make every rule engine take a field it
/// never reads. Issue #46.
#[derive(Debug, Default)]
pub struct Reasons(std::collections::BTreeMap<String, String>);
impl Reasons {
    /// Reads the reasons a compiled configuration carries.
    #[must_use]
    pub fn of(config: &archwarden_core::compiled::CompiledConfig) -> Self {
        Self(
            config
                .rules()
                .filter_map(|rule| {
                    rule.why
                        .as_ref()
                        .map(|why| (rule.id.as_str().to_owned(), why.clone()))
                })
                .collect(),
        )
    }

    /// Why this rule exists, when its author said.
    #[must_use]
    pub fn of_rule(&self, rule: &archwarden_core::ids::RuleId) -> Option<&str> {
        self.0.get(rule.as_str()).map(String::as_str)
    }
}

/// Rules and their reasons, spelled out.
///
/// For tests and for a caller that has reasons from somewhere other than a
/// compiled config. Here rather than in a surface because `Reasons` holds its
/// map privately, and the orphan rule puts the impl with the type.
impl<const N: usize> From<[(&str, &str); N]> for Reasons {
    fn from(pairs: [(&str, &str); N]) -> Self {
        Self(
            pairs
                .into_iter()
                .map(|(rule, why)| (rule.to_owned(), why.to_owned()))
                .collect(),
        )
    }
}

/// Everything the renderer needs about one run.
///
/// A struct because four of these arrived one at a time, and four positional
/// references of similar type at a call site is where a transposition hides.
pub struct Rendered<'a> {
    /// The repository, for turning a finding's byte span into a position.
    pub root: &'a Utf8Path,
    /// What the run found.
    pub report: &'a Report,
    /// What of it to show.
    pub view: &'a View<'a>,
    /// Why each rule exists, for the line under its first finding.
    pub reasons: &'a Reasons,
    /// How long the whole run took -- config load, walk, check and cache
    /// flush. Passed in rather than measured here because wall-clock belongs
    /// to the invocation, and a test that could not fix it could not assert on
    /// the format.
    pub elapsed: std::time::Duration,
}

/// One check nobody could make.
#[derive(Debug, Serialize)]
pub struct SkippedCheck {
    /// The rule that wanted to decide.
    pub rule_id: String,
    /// The file whose facts were unavailable.
    pub path: String,
}

/// The counts a run reports.
///
/// Read by every renderer and not only by JSON: the text format prints the
/// same numbers in a sentence, and two things counting the same findings is
/// two things that can disagree. That is why this is here rather than inside
/// one format's module.
#[derive(Debug, Serialize)]
pub struct Summary {
    /// Findings at `error` level, among those being shown.
    pub errors: usize,
    /// Findings at `warning` level, among those being shown.
    pub warnings: usize,
    /// How many files the walk examined.
    pub files_scanned: usize,
    /// How many directories the walk examined.
    pub directories_scanned: usize,
    /// How many files were parsed from source rather than read from cache.
    pub files_parsed: usize,
    /// How many files had their facts reused from the cache.
    pub facts_reused: usize,
    /// Checks no rule could make, because the file's facts were unavailable.
    ///
    /// Always present, including as zero. A consumer branching on it needs the
    /// field to exist; the text format leaves it out when it is zero, because
    /// a human reading `0 skipped` on every run only wonders why it is there.
    pub checks_skipped: usize,
    /// Which rule wanted which file, for every skipped check.
    ///
    /// The count on its own says a run decided less than it looks like and
    /// gives nobody a place to look. Absent when nothing was skipped, which is
    /// nearly always.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped_checks: Vec<SkippedCheck>,
    /// How long the whole run took, in milliseconds.
    ///
    /// The raw number rather than the prose the text format prints: a consumer
    /// comparing two runs needs to subtract them.
    pub duration_ms: u128,
    /// How many findings the filters removed from this report.
    ///
    /// Zero unless a filter was given. A consumer comparing `errors` against
    /// the exit code needs this: the gate counts what was evaluated, and these
    /// counts describe what was asked for.
    #[serde(skip_serializing_if = "is_zero")]
    pub hidden: usize,
    /// Per-rule counts, present only under `--summary`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_rule: Option<serde_json::Map<String, serde_json::Value>>,
    /// Absent when no rule needed resolution, so a consumer can tell "no
    /// boundary rule ran" from "every import resolved".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imports: Option<Imports>,
}
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's `skip_serializing_if` takes `&T`"
)]
fn is_zero(value: &usize) -> bool {
    *value == 0
}
#[allow(
    clippy::struct_field_names,
    reason = "`unresolved_imports` is a JSON key a consumer reads, not a name this struct is free to shorten"
)]
/// Where a run's imports went.
#[derive(Debug, Serialize)]
pub struct Imports {
    /// Imports that landed on a file in this repository.
    pub in_repo: usize,
    /// Imports that landed in a dependency.
    pub external: usize,
    /// Imports of a Node built-in.
    pub builtin: usize,
    /// Imports nothing could place, so no boundary rule saw them.
    pub unresolved: usize,
    /// Which file wrote each import that did not resolve.
    ///
    /// Every one of them, where the text format shows the first few: a CI job
    /// gating on "no import escapes the boundary rules" needs the whole list,
    /// and nothing is scrolling past it. Absent when everything resolved.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unresolved_imports: Vec<UnresolvedImport>,
}
/// One import no boundary rule could see.
#[derive(Debug, Serialize)]
pub struct UnresolvedImport {
    /// The file the import was written in.
    pub path: String,
    /// What it asked for.
    pub specifier: String,
}
impl Summary {
    /// Counts come from the view, everything else from the report.
    ///
    /// Deliberately: the counts describe what the user asked to see, and the
    /// rest describes the run that happened. `hidden` is what keeps the two
    /// reconcilable.
    #[must_use]
    pub fn of(report: &Report, view: &View<'_>, elapsed: std::time::Duration) -> Self {
        Self {
            errors: view.count(archwarden_core::level::Level::Error),
            warnings: view.count(archwarden_core::level::Level::Warning),
            files_scanned: report.files_scanned,
            directories_scanned: report.directories_scanned,
            files_parsed: report.files_parsed,
            facts_reused: report.facts_reused,
            checks_skipped: report.checks_skipped,
            skipped_checks: report
                .skipped_checks
                .iter()
                .map(|(rule_id, path)| SkippedCheck {
                    rule_id: rule_id.clone(),
                    path: path.as_str().to_owned(),
                })
                .collect(),
            duration_ms: elapsed.as_millis(),
            hidden: view.hidden(),
            by_rule: view.breakdown().map(breakdown_as_map),
            imports: (report.imports.total() > 0).then_some(Imports {
                in_repo: report.imports.in_repo,
                external: report.imports.external,
                builtin: report.imports.builtin,
                unresolved: report.imports.unresolved,
                unresolved_imports: report
                    .imports
                    .unresolved_imports
                    .iter()
                    .map(|(path, specifier)| UnresolvedImport {
                        path: path.as_str().to_owned(),
                        specifier: specifier.clone(),
                    })
                    .collect(),
            }),
        }
    }
}

/// The breakdown as a map of names to counts.
///
/// Built here rather than by `Breakdown` itself. The rows are data — a name
/// and two counts — and this map is one shape of them. `Breakdown::rows()`
/// hands the same rows to the text renderer, so the two cannot count
/// differently, and SARIF will get them without going through JSON to do it.
fn breakdown_as_map(breakdown: &Breakdown) -> serde_json::Map<String, serde_json::Value> {
    breakdown
        .rows()
        .map(|(rule_id, errors, warnings)| {
            (
                rule_id.to_owned(),
                serde_json::json!({ "errors": errors, "warnings": warnings }),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::{CompiledConfig, CompiledRule, CompiledRuleKind, SkipDirs},
        hash::ContentHash,
        ids::RuleId,
        level::Level,
    };

    fn rule(id: &str, why: Option<&str>) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: None,
            why: why.map(ToOwned::to_owned),
            module_why: None,
            level: Level::Error,
            scope: archwarden_core::scope::Scope::compile(["src/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Structure {
                allowed_subfolders: Some(Vec::new()),
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: Vec::new(),
                filename_patterns: Vec::new(),
            },
        }
    }

    /// A rule's standing reason is read off the compiled config rather than
    /// carried on every finding: a `why` is prose about a *rule*, and copying
    /// it onto each finding would put it in the baseline file's path, where
    /// rewording a sentence would churn a committed diff. Issue #46.
    #[test]
    fn the_reasons_are_the_ones_the_configuration_gave() {
        let config = CompiledConfig::new(
            vec![
                rule("shape", Some("the domain must not know about HTTP")),
                rule("spec", None),
            ],
            archwarden_core::glob::PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b""),
        );

        let reasons = Reasons::of(&config);

        assert_eq!(
            reasons.of_rule(&RuleId::new("shape").expect("valid")),
            Some("the domain must not know about HTTP")
        );
        assert_eq!(reasons.of_rule(&RuleId::new("spec").expect("valid")), None);
        assert_eq!(
            reasons.of_rule(&RuleId::new("absent").expect("valid")),
            None,
            "a rule the config never declared has no reason either"
        );
    }

    /// The same map, spelled out. For a caller whose reasons come from
    /// somewhere other than a compiled config, and for tests.
    #[test]
    fn reasons_can_be_written_out_directly() {
        let reasons = Reasons::from([("shape", "because")]);

        assert_eq!(
            reasons.of_rule(&RuleId::new("shape").expect("valid")),
            Some("because")
        );
    }
}
