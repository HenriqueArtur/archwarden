//! Present: what of a run to show, and what its exit code is.
//!
//! The last stage, and the one that has to hold a distinction no surface may
//! be trusted to remember on its own:
//!
//! > **The exit code comes from what the baseline did not accept, never from
//! > the view.**
//!
//! A filter narrows what is printed and nothing else. Otherwise `--rules` in a
//! CI command would quietly turn a failing build green, and an MCP client
//! asking for a summary of one rule would be told the repository is clean.
//! That rule used to be a comment beside four lines in `check`; here it is
//! [`Presented::fails_build`], which is the only way to ask.
//!
//! The baseline is applied first and *not* as a filter: what it accepts is
//! gone from the run entirely, exit code included. That is the one thing a
//! filter may never do, and the reason a baseline is a committed file rather
//! than a flag — see [`crate::baseline`].

use archwarden_core::{compiled::CompiledConfig, finding::Finding, path::RepoRelPath};
use archwarden_engine::run::Report;

use crate::{baseline::Baseline, filter::Filters};

/// What a summary counts by.
///
/// No `clap::ValueEnum` here: the word a user types is the surface's
/// vocabulary, and this crate no more learns about a command line than the
/// core does. `archwarden-cli` carries the enum with the derive on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Axis {
    /// One row per rule: what is dominating this output.
    #[default]
    Rule,
    /// One row per area of the repository: where to start.
    Path,
}

/// What of a report to show.
///
/// A run always evaluates every rule and computes every finding. This is the
/// part the user asked to look at, and nothing in it can change the exit code
/// — see [`crate::filter`].
#[derive(Debug)]
pub struct View<'a> {
    /// The findings that survived the filters, in the report's own order.
    findings: Vec<&'a Finding>,
    /// The per-rule counts, when `--summary` was asked for.
    breakdown: Option<Breakdown>,
    /// How many findings the filters removed.
    hidden: usize,
}
impl<'a> View<'a> {
    /// Everything, which is what an unfiltered run shows.
    #[must_use]
    pub fn everything(report: &'a Report) -> Self {
        Self {
            findings: report.findings.iter().collect(),
            breakdown: None,
            hidden: 0,
        }
    }

    /// A filtered listing.
    #[must_use]
    pub fn filtered(findings: &[&'a Finding], hidden: usize) -> Self {
        Self {
            findings: findings.to_vec(),
            breakdown: None,
            hidden,
        }
    }

    /// Counts instead of a listing.
    #[must_use]
    pub fn summarised(findings: &[&'a Finding], breakdown: Breakdown, hidden: usize) -> Self {
        Self {
            findings: findings.to_vec(),
            breakdown: Some(breakdown),
            hidden,
        }
    }

    /// How many of the shown findings are at this level.
    #[must_use]
    pub fn count(&self, level: archwarden_core::level::Level) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.level == level)
            .count()
    }

    /// The findings to show, in the report's own order.
    #[must_use]
    pub fn findings(&self) -> &[&'a Finding] {
        &self.findings
    }

    /// The counts, when the reader asked for counts rather than a listing.
    #[must_use]
    pub fn breakdown(&self) -> Option<&Breakdown> {
        self.breakdown.as_ref()
    }

    /// How many findings the filters removed.
    ///
    /// Reported by every format. Without it a narrowed run and a clean one
    /// print the same thing, and the reader cannot tell which they are
    /// looking at.
    #[must_use]
    pub fn hidden(&self) -> usize {
        self.hidden
    }
}

/// How many findings each rule produced.
///
/// Rows come from the configuration, counts from the findings being shown. A
/// rule that fired nothing keeps its row: that it was evaluated is an answer,
/// and a missing row reads as a rule someone disabled.
#[derive(Debug)]
pub struct Breakdown {
    rows: Vec<Row>,
}

#[derive(Debug)]
struct Row {
    /// A rule id, or a directory -- whichever axis the reader asked for. The
    /// table is the same either way; only what the first column names changes.
    rule_id: String,
    errors: usize,
    warnings: usize,
}

impl Breakdown {
    /// Counts `findings` against every rule in `ids`.
    ///
    /// Ordered worst-first: errors descending, then warnings descending, then
    /// by id. The same order the findings themselves are in — two orderings
    /// for one report is a thing someone eventually reports as a bug.
    #[must_use]
    pub fn over<I, S>(ids: I, findings: &[&Finding]) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut rows: Vec<Row> = ids
            .into_iter()
            .map(|id| {
                let id = id.as_ref();
                let mine = findings.iter().filter(|f| f.rule_id.as_str() == id);
                let (errors, warnings) =
                    mine.fold((0, 0), |(errors, warnings), finding| match finding.level {
                        archwarden_core::level::Level::Error => (errors + 1, warnings),
                        archwarden_core::level::Level::Warning => (errors, warnings + 1),
                    });
                Row {
                    rule_id: id.to_owned(),
                    errors,
                    warnings,
                }
            })
            .collect();

        rows.sort_by(|a, b| {
            b.errors
                .cmp(&a.errors)
                .then_with(|| b.warnings.cmp(&a.warnings))
                .then_with(|| a.rule_id.cmp(&b.rule_id))
        });

        Self { rows }
    }

    /// Counts `findings` by the area of the repository they are in.
    ///
    /// "Which rule dominates this output" and "which part of the repository is
    /// furthest from the rules" are different questions, and only the second
    /// says where to start a refactor.
    ///
    /// The areas are the directories the rules' own scopes select. Nothing
    /// here invents a depth: a config saying `roots: packages/domain/src/*`
    /// has already declared that `packages/domain/src/order` is a unit, and
    /// rolling a finding up to the nearest ancestor a scope matches is reading
    /// that back rather than guessing.
    ///
    /// Unlike the rule breakdown, only areas with findings get a row. That a
    /// rule was evaluated is an answer worth a zero; there is no comparable
    /// list of directories, and printing every clean one in a monorepo would
    /// bury the ones that are not.
    #[must_use]
    pub fn by_path(scopes: &[archwarden_core::scope::Scope], findings: &[&Finding]) -> Self {
        let mut counts: std::collections::BTreeMap<String, (usize, usize)> =
            std::collections::BTreeMap::new();

        for finding in findings {
            let area = area_of(scopes, &finding.path);
            let entry = counts.entry(area).or_default();
            match finding.level {
                archwarden_core::level::Level::Error => entry.0 += 1,
                archwarden_core::level::Level::Warning => entry.1 += 1,
            }
        }

        let mut rows: Vec<Row> = counts
            .into_iter()
            .map(|(rule_id, (errors, warnings))| Row {
                rule_id,
                errors,
                warnings,
            })
            .collect();

        rows.sort_by(|a, b| {
            b.errors
                .cmp(&a.errors)
                .then_with(|| b.warnings.cmp(&a.warnings))
                .then_with(|| a.rule_id.cmp(&b.rule_id))
        });

        Self { rows }
    }

    /// The rows, worst-first, as `(name, errors, warnings)`.
    ///
    /// Data rather than a rendered map. This used to hand back a
    /// `serde_json::Map` built for one format, which made every other format
    /// either reach through JSON or count again — and two things counting the
    /// same findings is two things that can disagree.
    pub fn rows(&self) -> impl Iterator<Item = (&str, usize, usize)> {
        self.rows
            .iter()
            .map(|row| (row.rule_id.as_str(), row.errors, row.warnings))
    }
}

/// The nearest ancestor of `path` that some rule's scope selects.
///
/// The path itself when a scope selects it, and the path itself again when
/// none does -- a finding outside every scope keeps its own name rather than
/// being dropped from a summary that claims to count everything, or filed
/// under a heading that means nothing.
fn area_of(scopes: &[archwarden_core::scope::Scope], path: &RepoRelPath) -> String {
    let mut candidate = Some(path.as_path());

    while let Some(directory) = candidate {
        if !directory.as_str().is_empty() && scopes.iter().any(|scope| scope.matches_dir(directory))
        {
            return directory.to_string();
        }
        candidate = directory.parent();
    }

    path.as_str().to_owned()
}

/// What of a run to show, and what it means for a build.
#[derive(Debug)]
pub struct Presented<'a> {
    /// Every finding the baseline did not accept.
    ///
    /// The exit code comes from here. Kept beside the view rather than folded
    /// into it because they answer different questions: the view is what a
    /// reader asked to look at, and this is what the run actually found.
    pub unaccepted: Vec<&'a Finding>,
    /// What to show.
    pub view: View<'a>,
}

impl Presented<'_> {
    /// Whether this run should fail a build.
    ///
    /// **From what the baseline did not accept, never from the view.** A
    /// filter narrows what is printed and nothing else — otherwise `--rules`
    /// in a CI command would quietly turn a failing build green, and an MCP
    /// client asking about one rule would be told the repository is clean.
    ///
    /// A method rather than a field, and on this rather than on the view,
    /// because that is what makes the rule impossible to get wrong from
    /// outside: there is no other way to ask.
    #[must_use]
    pub fn fails_build(&self) -> bool {
        self.unaccepted
            .iter()
            .any(|finding| finding.level.fails_build())
    }
}

/// How the reader asked to see the findings.
#[derive(Debug, Clone, Copy, Default)]
pub struct Shape {
    /// Count by this axis instead of listing. `None` lists.
    pub axis: Option<Axis>,
}

/// Present: applies the baseline, then the filters, then the shape.
///
/// The order is the whole content of this function and is not a preference.
/// The baseline goes first and not as a filter: what it accepts is gone from
/// the run entirely, exit code included. The filters go second and change only
/// what is printed. Anything that reversed the two would let a reading
/// preference decide whether a build passes.
#[must_use]
pub fn present<'a>(
    report: &'a Report,
    baseline: Option<&Baseline>,
    filters: &Filters,
    shape: Shape,
    compiled: &CompiledConfig,
) -> Presented<'a> {
    let unaccepted: Vec<&'a Finding> = report
        .findings
        .iter()
        .filter(|finding| baseline.is_none_or(|accepted| !accepted.accepts(finding)))
        .collect();

    let shown: Vec<&'a Finding> = unaccepted
        .iter()
        .copied()
        .filter(|finding| filters.keep(finding))
        .collect();
    let hidden = unaccepted.len() - shown.len();

    let view = match shape.axis {
        None => View::filtered(&shown, hidden),

        Some(Axis::Path) => {
            let scopes: Vec<archwarden_core::scope::Scope> =
                compiled.rules().map(|rule| rule.scope.clone()).collect();
            View::summarised(&shown, Breakdown::by_path(&scopes, &shown), hidden)
        }

        // Rows come from the config, not from what fired. `--rules` is the one
        // filter that names *rules*, so it is the one that narrows the rows;
        // the others leave every row in place, because "this rule found
        // nothing here" is the answer the reader wanted.
        Some(Axis::Rule) => {
            let ids: Vec<String> = filters.named_rules().map_or_else(
                || {
                    compiled
                        .rules()
                        .map(|rule| rule.id.as_str().to_owned())
                        .collect()
                },
                |named| named.iter().map(|id| id.as_str().to_owned()).collect(),
            );
            View::summarised(&shown, Breakdown::over(ids, &shown), hidden)
        }
    };

    Presented { unaccepted, view }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        finding::{Expectation, Observed},
        ids::RuleId,
        level::Level,
    };

    /// A breakdown's rows as names alone, which is what the ordering tests
    /// are about.
    fn ids(breakdown: &Breakdown) -> Vec<&str> {
        breakdown.rows().map(|(id, _, _)| id).collect()
    }

    /// And with their counts, for the tests that are about the numbers.
    fn rows(breakdown: &Breakdown) -> Vec<(&str, usize, usize)> {
        breakdown.rows().collect()
    }

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// One finding, of a rule, at a path. The observation itself is beside the
    /// point here: a breakdown counts by rule and by area and never looks
    /// inside one.
    fn from(rule_id: &str, level: Level, at: &str) -> Finding {
        Finding {
            rule_id: RuleId::new(rule_id).expect("valid"),
            module_id: None,
            level,
            path: path(at),
            span: None,
            observed: Observed::UnexpectedSubfolder {
                name: "handlers".to_owned(),
            },
            expected: Expectation::AllowedSubfolders {
                allowed: vec!["calcs".to_owned()],
                warn: Vec::new(),
                patterns: Vec::new(),
            },
        }
    }

    fn report(findings: Vec<Finding>) -> Report {
        Report {
            findings,
            directories_scanned: 12,
            files_scanned: 34,
            unreadable_files: Vec::new(),
            files_parsed: 0,
            checks_skipped: 0,
            skipped_checks: Vec::new(),
            facts_reused: 0,
            imports: archwarden_engine::resolve::Outcomes::default(),
            suppressed: Vec::new(),
        }
    }

    /// A breakdown over four rules, of which two fired.
    fn four_rules() -> (Report, Breakdown) {
        let findings = vec![
            from("domain-entity-shape", Level::Error, "packages/domain/a"),
            from("domain-entity-shape", Level::Error, "packages/domain/b"),
            from("actions-need-spec", Level::Warning, "packages/domain/c"),
        ];
        let report = report(findings);
        let breakdown = Breakdown::over(
            [
                "domain-entity-shape",
                "actions-need-spec",
                "calcs-need-spec",
                "variants-need-spec",
            ],
            &report.findings.iter().collect::<Vec<_>>(),
        );
        (report, breakdown)
    }

    /// The breakdown answers "what rule is dominating this output?", so the
    /// worst rule is the first line. Errors outrank warnings however many
    /// warnings there are: it is the same worst-first order the findings
    /// themselves are in, and two orderings for one report would be a bug
    /// someone eventually reports as one.
    #[test]
    fn the_breakdown_puts_the_worst_rule_first() {
        let (_report, breakdown) = four_rules();

        assert_eq!(
            ids(&breakdown),
            [
                "domain-entity-shape", // 2 errors
                "actions-need-spec",   // 1 warning
                "calcs-need-spec",     // nothing, and then by id
                "variants-need-spec",
            ]
        );
    }
    // --- the other axis --------------------------------------------------

    /// A scope selecting each module directory, as a real config does.
    fn module_scope() -> archwarden_core::scope::Scope {
        archwarden_core::scope::Scope::compile(["packages/domain/src/*"]).expect("valid scope")
    }

    /// "Which rule dominates" and "which part of the repository is furthest"
    /// are different questions. The second is the one that says where to start
    /// a refactor, and rolling up to the directories the *rules* select is what
    /// makes the rows mean something: the config already says what the units
    /// are, so nothing here has to invent a depth.
    #[test]
    fn findings_roll_up_to_the_directory_a_rule_scope_selects() {
        let findings = [
            from("shape", Level::Error, "packages/domain/src/order/handlers"),
            from("shape", Level::Error, "packages/domain/src/order/services"),
            from(
                "spec",
                Level::Warning,
                "packages/domain/src/invoice/calcs/a.ts",
            ),
        ];
        let shown: Vec<&Finding> = findings.iter().collect();

        let breakdown = Breakdown::by_path(&[module_scope()], &shown);

        assert_eq!(
            rows(&breakdown),
            [
                ("packages/domain/src/order", 2, 0),
                ("packages/domain/src/invoice", 0, 1),
            ]
        );
    }

    /// Worst first, then by path -- the same order the rule breakdown uses,
    /// because two orderings in one report is something someone eventually
    /// reports as a bug.
    #[test]
    fn the_path_breakdown_puts_the_worst_area_first() {
        let findings = [
            from("spec", Level::Warning, "packages/domain/src/aaa/x.ts"),
            from("shape", Level::Error, "packages/domain/src/zzz/handlers"),
        ];
        let shown: Vec<&Finding> = findings.iter().collect();

        assert_eq!(
            ids(&Breakdown::by_path(&[module_scope()], &shown)),
            ["packages/domain/src/zzz", "packages/domain/src/aaa"]
        );
    }

    /// A finding no scope reaches keeps its own path. Dropping it would lose a
    /// finding from a summary that claims to count everything, and inventing a
    /// parent for it would put it under a heading that means nothing.
    #[test]
    fn a_finding_outside_every_scope_stands_alone() {
        let findings = [from("shape", Level::Error, "scripts/build.ts")];
        let shown: Vec<&Finding> = findings.iter().collect();

        assert_eq!(
            rows(&Breakdown::by_path(&[module_scope()], &shown)),
            [("scripts/build.ts", 1, 0)]
        );
    }

    /// Only areas that have something to say. A rule breakdown lists every
    /// rule including the quiet ones -- that a rule was evaluated is an
    /// answer. There is no equivalent list of directories, and printing every
    /// clean directory in a monorepo would bury the ones that are not.
    #[test]
    fn the_path_breakdown_lists_only_what_fired() {
        assert!(ids(&Breakdown::by_path(&[module_scope()], &[])).is_empty());
    }
}

#[cfg(test)]
mod present_tests {
    use super::*;
    use archwarden_core::{
        compiled::{CompiledRule, CompiledRuleKind, SkipDirs},
        finding::{Expectation, Observed},
        hash::ContentHash,
        ids::RuleId,
        level::Level,
    };

    fn rule(id: &str, scope: &str) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Error,
            scope: archwarden_core::scope::Scope::compile([scope]).expect("valid scope"),
            kind: CompiledRuleKind::Structure {
                allowed_subfolders: Some(Vec::new()),
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: Vec::new(),
                filename_patterns: Vec::new(),
            },
        }
    }

    fn config(ids: &[&str]) -> CompiledConfig {
        CompiledConfig::new(
            ids.iter().map(|id| rule(id, "src/*")).collect(),
            archwarden_core::glob::PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b""),
        )
    }

    fn at(rule_id: &str, path: &str, level: Level) -> Finding {
        Finding {
            rule_id: RuleId::new(rule_id).expect("valid id"),
            module_id: None,
            level,
            path: RepoRelPath::new(path).expect("valid path"),
            span: None,
            observed: Observed::UnexpectedSubfolder {
                name: "handlers".to_owned(),
            },
            expected: Expectation::AllowedSubfolders {
                allowed: vec!["use-cases".to_owned()],
                warn: Vec::new(),
                patterns: Vec::new(),
            },
        }
    }

    fn run(findings: Vec<Finding>) -> Report {
        Report {
            findings,
            directories_scanned: 0,
            files_scanned: 0,
            unreadable_files: Vec::new(),
            files_parsed: 0,
            checks_skipped: 0,
            skipped_checks: Vec::new(),
            facts_reused: 0,
            imports: archwarden_engine::resolve::Outcomes::default(),
            suppressed: Vec::new(),
        }
    }

    fn only(rule_id: &str, config: &CompiledConfig) -> Filters {
        Filters::compile(
            crate::filter::Arguments {
                rules: std::slice::from_ref(&rule_id.to_owned()),
                ..crate::filter::Arguments::default()
            },
            config,
        )
        .expect("the rule exists")
    }

    /// **The invariant.** A filter narrows what is printed and may never touch
    /// the exit code — otherwise `--rules` in a CI command would quietly turn
    /// a failing build green, and an MCP client asking about one rule would be
    /// told the repository is clean.
    ///
    /// This used to be a comment beside four lines in `check`. A comment is
    /// obeyed by whoever read it, and MCP has not been written yet.
    #[test]
    fn a_filter_narrows_the_view_and_never_the_exit_code() {
        let config = config(&["shape", "spec"]);
        let report = run(vec![
            at("shape", "src/a", Level::Error),
            at("spec", "src/b", Level::Error),
        ]);

        let everything = present(
            &report,
            None,
            &Filters::default(),
            Shape::default(),
            &config,
        );
        let narrowed = present(
            &report,
            None,
            &only("spec", &config),
            Shape::default(),
            &config,
        );

        assert_eq!(everything.view.findings().len(), 2);
        assert_eq!(narrowed.view.findings().len(), 1, "the view narrows");
        assert_eq!(narrowed.view.hidden(), 1, "and says how much it hid");

        assert!(everything.fails_build());
        assert!(
            narrowed.fails_build(),
            "a filter turned a failing build green"
        );
    }

    /// The baseline is the one thing that may change the exit code, and it
    /// does not do it by filtering: what it accepts is gone from the run
    /// entirely. That is why it is a committed file a reviewer sees rather
    /// than a flag somebody adds to a command.
    #[test]
    fn what_the_baseline_accepted_leaves_the_run_entirely() {
        let config = config(&["shape"]);
        let findings = vec![at("shape", "src/a", Level::Error)];
        let report = run(findings.clone());
        let accepted = crate::baseline::Baseline::of(&findings);

        let presented = present(
            &report,
            Some(&accepted),
            &Filters::default(),
            Shape::default(),
            &config,
        );

        assert!(presented.unaccepted.is_empty());
        assert!(presented.view.findings().is_empty());
        assert_eq!(
            presented.view.hidden(),
            0,
            "accepted is not hidden, it is gone"
        );
        assert!(!presented.fails_build());
    }

    /// A warning is shown and does not gate. Decision 1.
    #[test]
    fn warnings_are_visible_and_do_not_fail_a_build() {
        let config = config(&["shape"]);
        let report = run(vec![at("shape", "src/a", Level::Warning)]);

        let presented = present(
            &report,
            None,
            &Filters::default(),
            Shape::default(),
            &config,
        );

        assert_eq!(presented.view.findings().len(), 1);
        assert!(!presented.fails_build());
    }

    /// No axis is a listing, and a listing has no breakdown to render.
    #[test]
    fn without_an_axis_the_view_is_a_listing() {
        let config = config(&["shape"]);
        let report = run(vec![at("shape", "src/a", Level::Error)]);

        let presented = present(
            &report,
            None,
            &Filters::default(),
            Shape::default(),
            &config,
        );

        assert!(presented.view.breakdown().is_none());
    }

    /// Rows come from the configuration, not from what fired: a rule that
    /// found nothing keeps its row, because that it was evaluated is an
    /// answer and a missing row reads as a rule somebody disabled.
    #[test]
    fn the_rule_axis_keeps_a_row_for_a_rule_that_found_nothing() {
        let config = config(&["shape", "spec"]);
        let report = run(vec![at("shape", "src/a", Level::Error)]);

        let presented = present(
            &report,
            None,
            &Filters::default(),
            Shape {
                axis: Some(Axis::Rule),
            },
            &config,
        );

        let rows: Vec<(&str, usize, usize)> = presented
            .view
            .breakdown()
            .expect("summarised")
            .rows()
            .collect();
        assert_eq!(rows, [("shape", 1, 0), ("spec", 0, 0)]);
    }

    /// Except when the reader named the rules. `--rules` is the one filter
    /// that is about *rules*, so it is the one that narrows the rows; the
    /// others leave every row in place.
    #[test]
    fn naming_rules_narrows_the_rows_as_well_as_the_findings() {
        let config = config(&["shape", "spec"]);
        let report = run(vec![
            at("shape", "src/a", Level::Error),
            at("spec", "src/b", Level::Error),
        ]);

        let presented = present(
            &report,
            None,
            &only("spec", &config),
            Shape {
                axis: Some(Axis::Rule),
            },
            &config,
        );

        let rows: Vec<&str> = presented
            .view
            .breakdown()
            .expect("summarised")
            .rows()
            .map(|(id, _, _)| id)
            .collect();
        assert_eq!(rows, ["spec"]);
    }

    /// The other axis answers a different question — not which rule dominates
    /// but which part of the repository is furthest from the rules, which is
    /// the one that says where to start.
    #[test]
    fn the_path_axis_counts_by_the_area_a_scope_selects() {
        let config = CompiledConfig::new(
            vec![rule("shape", "packages/*/src/*")],
            archwarden_core::glob::PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b""),
        );
        let report = run(vec![
            at("shape", "packages/domain/src/order/handlers", Level::Error),
            at("shape", "packages/domain/src/order/helpers", Level::Error),
            at("shape", "packages/web/src/cart/handlers", Level::Error),
        ]);

        let presented = present(
            &report,
            None,
            &Filters::default(),
            Shape {
                axis: Some(Axis::Path),
            },
            &config,
        );

        let rows: Vec<(&str, usize, usize)> = presented
            .view
            .breakdown()
            .expect("summarised")
            .rows()
            .collect();
        assert_eq!(
            rows,
            [
                ("packages/domain/src/order", 2, 0),
                ("packages/web/src/cart", 1, 0)
            ]
        );
    }

    /// Both at once, in the order that matters. The baseline accepts one, a
    /// filter hides another, and a third is left: `hidden` counts only the
    /// second, because the first never entered the run.
    #[test]
    fn the_baseline_runs_before_the_filters_and_the_counts_show_it() {
        let config = config(&["shape", "spec"]);
        let inherited = vec![at("shape", "src/old", Level::Error)];
        let report = run(vec![
            at("shape", "src/old", Level::Error),
            at("shape", "src/new", Level::Error),
            at("spec", "src/other", Level::Error),
        ]);
        let accepted = crate::baseline::Baseline::of(&inherited);

        let presented = present(
            &report,
            Some(&accepted),
            &only("spec", &config),
            Shape::default(),
            &config,
        );

        assert_eq!(presented.unaccepted.len(), 2, "the accepted one is gone");
        assert_eq!(presented.view.findings().len(), 1);
        assert_eq!(
            presented.view.hidden(),
            1,
            "hidden counts the filtered, not the accepted"
        );
        assert!(
            presented.fails_build(),
            "the filtered-out error still gates"
        );
    }

    /// `View::everything` is what a surface with nothing to narrow by uses —
    /// `check --file` answers about one file and has no filters at all.
    #[test]
    fn everything_shows_every_finding_and_hides_nothing() {
        let report = run(vec![
            at("shape", "src/a", Level::Error),
            at("shape", "src/b", Level::Warning),
        ]);

        let view = View::everything(&report);

        assert_eq!(view.findings().len(), 2);
        assert_eq!(view.hidden(), 0);
        assert_eq!(view.count(Level::Error), 1);
        assert_eq!(view.count(Level::Warning), 1);
        assert!(view.breakdown().is_none());
    }

    /// The ordering has three keys and the last two are only reached on a tie.
    /// Equal errors fall to warnings; equal warnings fall to the name. Without
    /// the last one two areas with identical counts would come out in whatever
    /// order the map happened to yield, and a summary that reshuffled between
    /// runs makes every diff of it unreadable.
    #[test]
    fn areas_with_equal_counts_are_ordered_by_name_and_stay_that_way() {
        let config = CompiledConfig::new(
            vec![rule("shape", "packages/*/src/*")],
            archwarden_core::glob::PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b""),
        );
        let report = run(vec![
            // `zed` and `alpha` tie on errors and on warnings, so only the
            // name can separate them. `beta` ties on errors alone.
            at("shape", "packages/zed/src/one/a", Level::Error),
            at("shape", "packages/alpha/src/one/a", Level::Error),
            at("shape", "packages/beta/src/one/a", Level::Error),
            at("shape", "packages/beta/src/one/b", Level::Warning),
        ]);

        let rows = |shape| -> Vec<String> {
            present(&report, None, &Filters::default(), shape, &config)
                .view
                .breakdown()
                .expect("summarised")
                .rows()
                .map(|(name, _, _)| name.to_owned())
                .collect()
        };

        let order = rows(Shape {
            axis: Some(Axis::Path),
        });
        assert_eq!(
            order,
            [
                "packages/beta/src/one",  // 1 error, 1 warning
                "packages/alpha/src/one", // 1 error, tie broken by name
                "packages/zed/src/one",
            ]
        );
        assert_eq!(
            order,
            rows(Shape {
                axis: Some(Axis::Path)
            }),
            "the same input has to produce the same order"
        );
    }

    /// The same three keys on the rule axis. Two rules that fired the same
    /// number of errors are separated by their warnings, and two that fired
    /// identically by their ids.
    #[test]
    fn rules_with_equal_counts_are_ordered_by_warnings_then_by_id() {
        let config = config(&["b-rule", "a-rule", "loud"]);
        let report = run(vec![
            at("loud", "src/a", Level::Error),
            at("loud", "src/b", Level::Warning),
            at("b-rule", "src/c", Level::Error),
            at("a-rule", "src/d", Level::Error),
        ]);

        let presented = present(
            &report,
            None,
            &Filters::default(),
            Shape {
                axis: Some(Axis::Rule),
            },
            &config,
        );

        let rows: Vec<&str> = presented
            .view
            .breakdown()
            .expect("summarised")
            .rows()
            .map(|(id, _, _)| id)
            .collect();
        assert_eq!(rows, ["loud", "a-rule", "b-rule"]);
    }
}
