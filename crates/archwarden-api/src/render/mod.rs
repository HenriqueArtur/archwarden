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
/// [`Finding`](archwarden_core::finding::Finding), deliberately. A `why` is
/// prose about a *rule*; a finding is about a file,
/// and copying the prose onto every one of them would put it in the baseline's
/// path -- `.archwarden/baseline.json` must not churn because somebody
/// reworded a sentence -- and would make every rule engine take a field it
/// never reads. Issue #46.
/// It also carries the *decision* each rule implements, resolved from the
/// config's `decisions` block. The two travel together because every surface
/// that shows one shows the other in the same breath — a denial says why the
/// rule exists and which decision it serves — and two places resolving that
/// would be two places to get it wrong. Issue #100.
#[derive(Debug, Default)]
pub struct Reasons {
    /// Why each rule exists, by rule id.
    why: std::collections::BTreeMap<String, String>,
    /// Which decision each rule implements, in configuration order.
    ///
    /// A list rather than a map, unlike `why` beside it, and the difference is
    /// what is asked of each. `why` is only ever looked up by rule id. This is
    /// also read *backwards* — the rules serving a decision — and that answer
    /// is shown to a person, so it comes out in the order they wrote their
    /// rules rather than sorted by id. A few dozen entries scanned at render
    /// time.
    implements: Vec<(String, archwarden_core::ids::DecisionId)>,
    /// Every decision the config declared, in declaration order — including
    /// the ones no rule points at, which is a state `config doctor` reports
    /// and the guide page shows.
    decisions: Vec<archwarden_core::compiled::CompiledDecision>,
}
impl Reasons {
    /// Reads the reasons and decisions a compiled configuration carries.
    #[must_use]
    pub fn of(config: &archwarden_core::compiled::CompiledConfig) -> Self {
        Self {
            why: config
                .rules()
                .filter_map(|rule| {
                    rule.why
                        .as_ref()
                        .map(|why| (rule.id.as_str().to_owned(), why.clone()))
                })
                .collect(),
            implements: config
                .rules()
                .filter_map(|rule| {
                    rule.decision
                        .as_ref()
                        .map(|decision| (rule.id.as_str().to_owned(), decision.clone()))
                })
                .collect(),
            decisions: config.decisions().cloned().collect(),
        }
    }

    /// Why this rule exists, when its author said.
    #[must_use]
    pub fn of_rule(&self, rule: &archwarden_core::ids::RuleId) -> Option<&str> {
        self.why.get(rule.as_str()).map(String::as_str)
    }

    /// The decision this rule implements, when it names one.
    ///
    /// Resolved rather than stored per rule: N rules serve one decision, and
    /// copying the prose onto each would give it N places to disagree with
    /// itself.
    #[must_use]
    pub fn decision_of_rule(
        &self,
        rule: &archwarden_core::ids::RuleId,
    ) -> Option<&archwarden_core::compiled::CompiledDecision> {
        let named = self
            .implements
            .iter()
            .find(|(id, _)| id == rule.as_str())
            .map(|(_, named)| named)?;
        self.decisions.iter().find(|decision| &decision.id == named)
    }

    /// Every decision the configuration declared, in declaration order.
    pub fn decisions(&self) -> impl Iterator<Item = &archwarden_core::compiled::CompiledDecision> {
        self.decisions.iter()
    }

    /// Every rule that implements a given decision, in configuration order.
    ///
    /// The reverse of the foreign key, computed rather than stored — which is
    /// the point of issue #100's shape. `config explain <decision-id>` asks
    /// this, and so does the doctor's check for a superseded decision whose
    /// rules still fire.
    pub fn rules_implementing(
        &self,
        decision: &archwarden_core::ids::DecisionId,
    ) -> impl Iterator<Item = &str> {
        self.implements
            .iter()
            .filter(move |(_, named)| named == decision)
            .map(|(rule, _)| rule.as_str())
    }
}

/// Rules and their reasons, spelled out.
///
/// For tests and for a caller that has reasons from somewhere other than a
/// compiled config. Here rather than in a surface because `Reasons` holds its
/// maps privately, and the orphan rule puts the impl with the type.
impl<const N: usize> From<[(&str, &str); N]> for Reasons {
    fn from(pairs: [(&str, &str); N]) -> Self {
        Self {
            why: pairs
                .into_iter()
                .map(|(rule, why)| (rule.to_owned(), why.to_owned()))
                .collect(),
            implements: Vec::new(),
            decisions: Vec::new(),
        }
    }
}

impl Reasons {
    /// The decisions each rule implements, spelled out.
    ///
    /// A builder step rather than a second `From`, because the two are set
    /// independently: a config may carry reasons, decisions, both or neither,
    /// and a constructor taking both would make every test that wants one
    /// write the other as an empty array.
    #[must_use]
    pub fn deciding<const N: usize>(
        mut self,
        pairs: [(&str, archwarden_core::compiled::CompiledDecision); N],
    ) -> Self {
        for (rule, decision) in pairs {
            self.implements.push((rule.to_owned(), decision.id.clone()));
            if !self.decisions.iter().any(|seen| seen.id == decision.id) {
                self.decisions.push(decision);
            }
        }
        self
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
    /// How this run stands against the baseline, when there is one.
    ///
    /// `None` means no baseline was loaded, which is a different fact from a
    /// baseline that accepted nothing -- the distinction
    /// [`Summary::imports`](Summary::imports) already draws for resolution.
    ///
    /// It reaches the renderer rather than being printed after it because the
    /// JSON document is the whole of stdout: a line written past the closing
    /// brace is trailing text, and it is what issue #110 was filed about. The
    /// text format still prints its own sentence, from the same number.
    pub standing: Option<crate::baseline::Standing>,
    /// The day this run answered for, carried into `summary.as_of`.
    pub as_of: archwarden_core::date::Date,
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
    /// How many findings an `archwarden-allow` marker took out of the list.
    ///
    /// On the summary line, and never only in a section below it: a number
    /// that only ever goes up, visibly, is a number somebody eventually acts
    /// on. Issue #72.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub suppressed: usize,
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
    /// The day this run answered for.
    ///
    /// Always present. A report read a week later is not reproducible unless
    /// it says which day it was answered for — a deadline finding is a fact
    /// about a date, and without this a consumer cannot tell a report that is
    /// stale from a repository that regressed. Issue #117.
    pub as_of: String,
    /// How this run stands against the baseline. Absent when there is none.
    ///
    /// In the document rather than on a line after it, which is what issue
    /// #110 was filed about, and in the document rather than on stderr, which
    /// is the argument `suppressed` already makes one field up: a number that
    /// only ever goes up, visibly, is a number somebody eventually acts on.
    /// `gone` is the other half and the cheerful one -- accepted entries that
    /// no longer occur -- and a baseline nobody is reminded of is a suppression
    /// file. Sending that to a stream CI throws away would fix the parse and
    /// lose the point.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<crate::baseline::Standing>,
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
    pub fn of(rendered: &Rendered<'_>) -> Self {
        let Rendered {
            report,
            view,
            elapsed,
            ref standing,
            as_of,
            ..
        } = *rendered;

        Self {
            errors: view.count(archwarden_core::level::Level::Error),
            warnings: view.count(archwarden_core::level::Level::Warning),
            files_scanned: report.files_scanned,
            directories_scanned: report.directories_scanned,
            files_parsed: report.files_parsed,
            facts_reused: report.facts_reused,
            checks_skipped: report.checks_skipped,
            suppressed: report.suppressed.len(),
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
            as_of: as_of.to_string(),
            baseline: standing.clone(),
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
        compiled::{
            CompiledConfig, CompiledDecision, CompiledRule, CompiledRuleKind, DecisionStatus,
            SkipDirs,
        },
        hash::ContentHash,
        ids::{DecisionId, RuleId},
        level::Level,
    };

    fn rule(id: &str, why: Option<&str>) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: None,
            why: why.map(ToOwned::to_owned),
            not_yet: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
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

    fn decided(id: &str, decision: &str) -> CompiledRule {
        let mut rule = rule(id, None);
        rule.decision = Some(DecisionId::new(decision).expect("valid id"));
        rule
    }

    fn adr(id: &str) -> CompiledDecision {
        CompiledDecision {
            scope: None,
            why_not_enforceable: None,
            id: DecisionId::new(id).expect("valid id"),
            title: "The domain does not know about transport".to_owned(),
            why: Some("it is published, and a consumer must not inherit our client".to_owned()),
            link: Some("docs/adr/014.md".to_owned()),
            status: DecisionStatus::Accepted,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            alternatives: Vec::new(),
            not_yet: None,
        }
    }

    fn config(rules: Vec<CompiledRule>, decisions: Vec<CompiledDecision>) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            archwarden_core::glob::PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b""),
        )
        .with_decisions(decisions)
    }

    /// The other half of the same lookup, and the reason it lives here: every
    /// surface that shows a rule's reason shows the decision it implements in
    /// the same breath, and two places resolving that would be two places to
    /// get it wrong. Issue #100.
    #[test]
    fn the_decision_a_rule_implements_is_resolved_from_the_config() {
        let reasons = Reasons::of(&config(
            vec![decided("shape", "ADR-014"), rule("spec", None)],
            vec![adr("ADR-014")],
        ));

        let found = reasons
            .decision_of_rule(&RuleId::new("shape").expect("valid"))
            .expect("the rule names one and the config declares it");
        assert_eq!(found.id.as_str(), "ADR-014");
        assert_eq!(found.title, "The domain does not know about transport");
        assert_eq!(found.link.as_deref(), Some("docs/adr/014.md"));

        assert!(
            reasons
                .decision_of_rule(&RuleId::new("spec").expect("valid"))
                .is_none(),
            "a rule that names no decision has none, and that is not an error"
        );
    }

    /// One decision, many rules — which is the whole reason the prose lives on
    /// the config and the rules carry a reference. Both resolve to the same
    /// words, and there is one place to edit them.
    #[test]
    fn two_rules_serving_one_decision_resolve_to_the_same_prose() {
        let reasons = Reasons::of(&config(
            vec![decided("shape", "ADR-014"), decided("sealed", "ADR-014")],
            vec![adr("ADR-014")],
        ));

        // By id: a compiled decision holds a built `Scope` and so has no
        // equality, exactly as `CompiledRule` has none for the same reason.
        assert_eq!(
            reasons
                .decision_of_rule(&RuleId::new("shape").expect("valid"))
                .map(|decision| decision.id.as_str()),
            reasons
                .decision_of_rule(&RuleId::new("sealed").expect("valid"))
                .map(|decision| decision.id.as_str())
        );
    }

    /// Every decision the config declared, whether or not a rule points at it.
    /// The guide page lists what the architecture *decided*, and `config
    /// doctor` has to be able to see an orphan.
    #[test]
    fn every_declared_decision_is_reachable_even_unenforced() {
        let reasons = Reasons::of(&config(
            vec![decided("shape", "ADR-014")],
            vec![adr("ADR-014"), adr("ADR-020")],
        ));

        let ids: Vec<&str> = reasons.decisions().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["ADR-014", "ADR-020"], "declaration order is kept");
    }

    /// A config with no decisions is every config written before 0.21, and it
    /// answers the new question with nothing rather than with an error.
    #[test]
    fn a_config_with_no_decisions_answers_none() {
        let reasons = Reasons::of(&config(vec![rule("shape", Some("because"))], Vec::new()));

        assert!(
            reasons
                .decision_of_rule(&RuleId::new("shape").expect("valid"))
                .is_none()
        );
        assert_eq!(reasons.decisions().count(), 0);
        assert_eq!(
            reasons.of_rule(&RuleId::new("shape").expect("valid")),
            Some("because"),
            "and the reason it did carry is untouched"
        );
    }

    /// Two rules pointing at one decision keep one copy of the prose, which is
    /// the whole reason the prose lives on the config.
    #[test]
    fn one_decision_named_twice_is_stored_once() {
        let reasons =
            Reasons::default().deciding([("shape", adr("ADR-014")), ("sealed", adr("ADR-014"))]);

        assert_eq!(reasons.decisions().count(), 1);
        assert_eq!(
            reasons
                .rules_implementing(&DecisionId::new("ADR-014").expect("valid"))
                .count(),
            2,
            "and both rules still resolve to it"
        );

        let two =
            Reasons::default().deciding([("shape", adr("ADR-014")), ("other", adr("ADR-020"))]);
        assert_eq!(two.decisions().count(), 2, "two distinct ones stay two");
    }

    /// The foreign key read backwards, which is what `config explain
    /// <decision-id>` and the doctor's superseded check both ask. Computed
    /// rather than stored: that is what makes the decision block carry no list
    /// of rules to keep in step.
    #[test]
    fn the_rules_serving_a_decision_are_found_by_reading_the_key_backwards() {
        let reasons = Reasons::of(&config(
            vec![
                decided("shape", "ADR-014"),
                rule("spec", None),
                decided("sealed", "ADR-014"),
                decided("other", "ADR-020"),
            ],
            vec![adr("ADR-014"), adr("ADR-020")],
        ));

        let serving: Vec<&str> = reasons
            .rules_implementing(&DecisionId::new("ADR-014").expect("valid"))
            .collect();
        assert_eq!(
            serving,
            ["shape", "sealed"],
            "the order the rules were written in, not their ids sorted"
        );

        assert_eq!(
            reasons
                .rules_implementing(&DecisionId::new("ADR-099").expect("valid"))
                .count(),
            0,
            "a decision nothing serves answers with nothing, not with everything"
        );
    }

    /// Spelled out, for the tests of every surface downstream.
    #[test]
    fn decisions_can_be_written_out_directly() {
        let reasons = Reasons::from([("shape", "because")]).deciding([("shape", adr("ADR-014"))]);

        assert_eq!(
            reasons
                .decision_of_rule(&RuleId::new("shape").expect("valid"))
                .map(|d| d.id.as_str()),
            Some("ADR-014")
        );
        assert_eq!(
            reasons.of_rule(&RuleId::new("shape").expect("valid")),
            Some("because"),
            "the two are set independently and neither clears the other"
        );
    }
}
