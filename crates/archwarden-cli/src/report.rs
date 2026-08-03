//! Rendering a report.
//!
//! Two formats, one source. Text is for a human reading a terminal; JSON is
//! for an agent or another tool, and its shape is a contract -- it carries a
//! version so a consumer can tell when that contract changes.
//!
//! The prose in the text format is generated from the same `Observed` and
//! `Expectation` values the JSON carries, so the two can never describe a
//! finding differently.

use archwarden_core::{
    facts::ExportKind,
    finding::{Expectation, Finding, Observed},
    path::RepoRelPath,
};
use archwarden_engine::run::Report;
use camino::Utf8Path;
use serde::Serialize;

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

/// How to render a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Format {
    /// Grouped, human-readable text.
    #[default]
    Text,
    /// A stable, versioned JSON object.
    Json,
}

/// What `--summary` counts by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
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

    fn count(&self, level: archwarden_core::level::Level) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.level == level)
            .count()
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

#[derive(Debug, Serialize)]
struct Row {
    /// A rule id, or a directory -- whichever axis the reader asked for. The
    /// table is the same either way; only what the first column names changes.
    #[serde(skip)]
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

    /// The map the JSON carries, in the order the text uses.
    fn as_map(&self) -> serde_json::Map<String, serde_json::Value> {
        self.rows
            .iter()
            .map(|row| {
                (
                    row.rule_id.clone(),
                    serde_json::json!({ "errors": row.errors, "warnings": row.warnings }),
                )
            })
            .collect()
    }

    #[cfg(test)]
    fn ids(&self) -> Vec<&str> {
        self.rows.iter().map(|row| row.rule_id.as_str()).collect()
    }

    #[cfg(test)]
    fn rows_for_test(&self) -> Vec<(&str, usize, usize)> {
        self.rows
            .iter()
            .map(|row| (row.rule_id.as_str(), row.errors, row.warnings))
            .collect()
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

/// The JSON envelope.
#[derive(Debug, Serialize)]
struct JsonReport<'a> {
    version: u32,
    summary: Summary,
    /// Absent under `--summary`, which is the point of the flag: a summary
    /// that still emitted every finding would give a piping consumer no size
    /// benefit at all. Absence is opt-in — a consumer that never passes the
    /// flag sees the field it always saw.
    #[serde(skip_serializing_if = "Option::is_none")]
    findings: Option<&'a [&'a Finding]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unreadable_files: Vec<UnreadableFile<'a>>,
}

/// One check nobody could make.
#[derive(Debug, Serialize)]
struct SkippedCheck {
    rule_id: String,
    path: String,
}

#[derive(Debug, Serialize)]
struct UnreadableFile<'a> {
    path: &'a archwarden_core::path::RepoRelPath,
    reason: &'a str,
}

#[derive(Debug, Serialize)]
struct Summary {
    errors: usize,
    warnings: usize,
    files_scanned: usize,
    directories_scanned: usize,
    files_parsed: usize,
    facts_reused: usize,
    /// Checks no rule could make, because the file's facts were unavailable.
    ///
    /// Always present, including as zero. A consumer branching on it needs the
    /// field to exist; the text format leaves it out when it is zero, because
    /// a human reading `0 skipped` on every run only wonders why it is there.
    checks_skipped: usize,
    /// Which rule wanted which file, for every skipped check.
    ///
    /// The count on its own says a run decided less than it looks like and
    /// gives nobody a place to look. Absent when nothing was skipped, which is
    /// nearly always.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    skipped_checks: Vec<SkippedCheck>,
    /// How long the whole run took, in milliseconds.
    ///
    /// The raw number rather than the prose the text format prints: a consumer
    /// comparing two runs needs to subtract them.
    duration_ms: u128,
    /// How many findings the filters removed from this report.
    ///
    /// Zero unless a filter was given. A consumer comparing `errors` against
    /// the exit code needs this: the gate counts what was evaluated, and these
    /// counts describe what was asked for.
    #[serde(skip_serializing_if = "is_zero")]
    hidden: usize,
    /// Per-rule counts, present only under `--summary`.
    #[serde(skip_serializing_if = "Option::is_none")]
    by_rule: Option<serde_json::Map<String, serde_json::Value>>,
    /// Absent when no rule needed resolution, so a consumer can tell "no
    /// boundary rule ran" from "every import resolved".
    #[serde(skip_serializing_if = "Option::is_none")]
    imports: Option<Imports>,
}

/// Whether to leave `hidden` out of a report nothing was hidden from.
///
/// By reference because that is the signature `skip_serializing_if` calls,
/// not because a `usize` wants one.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's `skip_serializing_if` takes `&T`"
)]
fn is_zero(value: &usize) -> bool {
    *value == 0
}

/// Where a run's imports went.
#[derive(Debug, Serialize)]
struct Imports {
    in_repo: usize,
    external: usize,
    builtin: usize,
    unresolved: usize,
}

impl Summary {
    /// Counts come from the view, everything else from the report.
    ///
    /// Deliberately: the counts describe what the user asked to see, and the
    /// rest describes the run that happened. `hidden` is what keeps the two
    /// reconcilable.
    fn of(report: &Report, view: &View<'_>, elapsed: std::time::Duration) -> Self {
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
            hidden: view.hidden,
            by_rule: view.breakdown.as_ref().map(Breakdown::as_map),
            imports: (report.imports.total() > 0).then_some(Imports {
                in_repo: report.imports.in_repo,
                external: report.imports.external,
                builtin: report.imports.builtin,
                unresolved: report.imports.unresolved,
            }),
        }
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
    /// How long the whole run took -- config load, walk, check and cache
    /// flush. Passed in rather than measured here because wall-clock belongs
    /// to the invocation, and a test that could not fix it could not assert on
    /// the format.
    pub elapsed: std::time::Duration,
}

/// Writes a report in the requested format.
pub fn render(rendered: &Rendered<'_>, format: Format, out: &mut dyn std::io::Write) {
    match format {
        Format::Text => render_text(rendered, out),
        Format::Json => render_json(rendered, out),
    }
}

/// Turns a finding's byte offset into `line:column`, reading each file once.
///
/// The memo is one entry deep on purpose: findings are sorted by path, so the
/// repeats are consecutive, and holding every source file a report mentions
/// would be a lot of memory for a lookup that never comes back.
#[derive(Default)]
struct Positions {
    cached: Option<(RepoRelPath, Option<String>)>,
}

impl Positions {
    /// The path as a reader should see it: `path:line:column` when there is a
    /// position to give, and the bare path when there is not.
    ///
    /// A structure finding is about a directory and has no span; a span into a
    /// file that changed under us, or is gone, gives nothing. A position that
    /// is wrong is worse than none, because the reader follows it.
    fn label(&mut self, root: &Utf8Path, finding: &Finding) -> String {
        let Some(span) = finding.span else {
            return finding.path.to_string();
        };

        if self
            .cached
            .as_ref()
            .is_none_or(|(at, _)| *at != finding.path)
        {
            let text = std::fs::read_to_string(root.join(finding.path.as_path())).ok();
            self.cached = Some((finding.path.clone(), text));
        }
        let Some((_, Some(text))) = &self.cached else {
            return finding.path.to_string();
        };

        let Some(before) = text.get(..span.start as usize) else {
            return finding.path.to_string();
        };

        let line = before.matches('\n').count() + 1;
        // Characters, not bytes. An accent earlier on the line would otherwise
        // send an editor to the wrong column.
        let column = before
            .rsplit_once('\n')
            .map_or(before, |(_, last)| last)
            .chars()
            .count()
            + 1;

        format!("{}:{line}:{column}", finding.path)
    }
}

/// How long the run took, at a scale a reader can use.
///
/// Milliseconds below a second, one decimal of seconds below a minute, and
/// minutes above. `0ms` is never printed: a run that happened took *some*
/// time, and a reader seeing zero would reasonably conclude it did not.
fn human_duration(elapsed: std::time::Duration) -> String {
    let millis = elapsed.as_millis();

    if millis == 0 {
        return "<1ms".to_owned();
    }
    if millis < 1_000 {
        return format!("{millis}ms");
    }

    let seconds = elapsed.as_secs_f64();
    if seconds < 60.0 {
        return format!("{seconds:.1}s");
    }

    let whole = elapsed.as_secs();
    format!("{}m {}s", whole / 60, whole % 60)
}

fn render_json(rendered: &Rendered<'_>, out: &mut dyn std::io::Write) {
    let Rendered {
        report,
        view,
        elapsed,
        ..
    } = *rendered;
    let envelope = JsonReport {
        version: REPORT_VERSION,
        summary: Summary::of(report, view, elapsed),
        findings: view.breakdown.is_none().then_some(view.findings.as_slice()),
        unreadable_files: report
            .unreadable_files
            .iter()
            .map(|(path, reason)| UnreadableFile {
                path,
                reason: reason.as_str(),
            })
            .collect(),
    };

    // A report that cannot be serialised is a bug in these types, not
    // something a user can act on, so it is reported as itself rather than
    // silently producing nothing.
    match serde_json::to_string_pretty(&envelope) {
        Ok(json) => {
            let _ = writeln!(out, "{json}");
        }
        Err(error) => {
            let _ = writeln!(out, r#"{{"error":"cannot serialise report: {error}"}}"#);
        }
    }
}

/// One finding, in the shape a reader has learned to scan.
///
/// Shared by the full report and the single-file check, so a hook and a
/// commit-time run word the same finding identically.
fn render_finding(finding: &Finding, at: &str, out: &mut dyn std::io::Write) {
    let module = finding
        .module_id
        .as_ref()
        .map_or_else(|| "*".to_owned(), ToString::to_string);

    let _ = writeln!(
        out,
        "{:<7} {}\n        [{}] {} — {}",
        finding.level,
        at,
        module,
        finding.rule_id,
        describe_observed(&finding.observed),
    );
    let _ = writeln!(
        out,
        "        expected: {}",
        describe_expectation(&finding.expected)
    );
    let _ = writeln!(out);
}

/// The per-rule listing `--summary` prints in place of the findings.
///
/// The rule id sets the left column and the count is right-aligned inside it,
/// so the numbers read as a column rather than as prose to scan.
fn render_breakdown(breakdown: &Breakdown, out: &mut dyn std::io::Write) {
    // A configuration with no rules has nothing to break down, and a blank
    // line above the totals would be the only thing `--summary` contributed.
    if breakdown.rows.is_empty() {
        return;
    }

    let id_width = breakdown
        .rows
        .iter()
        .map(|row| row.rule_id.len())
        .max()
        .unwrap_or(0);
    let count_width = breakdown
        .rows
        .iter()
        .map(|row| row.errors.max(row.warnings).to_string().len())
        .max()
        .unwrap_or(1);

    for row in &breakdown.rows {
        let (count, tail) = match (row.errors, row.warnings) {
            (0, 0) => (0, String::new()),
            (errors, 0) => (errors, format!(" {}", plural(errors, "error", "errors"))),
            (0, warnings) => (
                warnings,
                format!(" {}", plural(warnings, "warning", "warnings")),
            ),
            (errors, warnings) => (
                errors,
                format!(
                    " {}, {warnings} {}",
                    plural(errors, "error", "errors"),
                    plural(warnings, "warning", "warnings")
                ),
            ),
        };

        let _ = writeln!(
            out,
            "{:<id_width$}  {count:>count_width$}{tail}",
            row.rule_id
        );
    }

    let _ = writeln!(out);
}

fn render_text(rendered: &Rendered<'_>, out: &mut dyn std::io::Write) {
    let Rendered {
        root,
        report,
        view,
        elapsed,
    } = *rendered;

    if let Some(breakdown) = &view.breakdown {
        render_breakdown(breakdown, out);
    } else {
        let mut positions = Positions::default();
        for finding in &view.findings {
            let at = positions.label(root, finding);
            render_finding(finding, &at, out);
        }
    }

    for (path, reason) in &report.unreadable_files {
        let _ = writeln!(out, "note: `{path}` was not checked — {reason}");

        // And which rules went unanswered because of it. `AGENTS.md` calls
        // `checks_skipped` "the number to watch" and tells a reader not to
        // report such a run as clean; a bare count leaves them nothing to act
        // on but stopping. The JSON has carried `rule_id` and `path` for a
        // while and the text output carried neither, so the only reader who
        // could answer "which ones, and where" was one already piping through
        // `jq`. Issue #12.
        //
        // Under the file rather than beside the count, because every counted
        // skip is a rule that wanted a file this loop is already naming: the
        // count only rises where `facts_for` failed, and that is the same
        // branch that pushes here.
        let rules: Vec<&str> = report
            .skipped_checks
            .iter()
            .filter(|(_, skipped)| skipped == path)
            .map(|(rule_id, _)| rule_id.as_str())
            .collect();
        if !rules.is_empty() {
            let _ = writeln!(
                out,
                "      {} {} skipped there: {}",
                rules.len(),
                plural(rules.len(), "check", "checks"),
                rules.join(", "),
            );
        }
    }

    // Without this, `0 errors` beside exit 1 is a contradiction the reader
    // cannot resolve: the gate counts what was evaluated, and the line above
    // counts what was asked for.
    if view.hidden > 0 {
        let _ = writeln!(
            out,
            "note: {} {} hidden by the filters given",
            view.hidden,
            plural(view.hidden, "finding", "findings")
        );
    }

    // An import a boundary rule could not place is an import it did not check.
    // Counted rather than listed: on a repository whose dependencies are not
    // installed this is every bare specifier, and a line each would bury the
    // findings the user came for.
    let unresolved = report.imports.unresolved;
    if unresolved > 0 {
        let _ = writeln!(
            out,
            "note: {unresolved} {} not resolve, so boundary rules did not see {}",
            if unresolved == 1 {
                "import could"
            } else {
                "imports could"
            },
            if unresolved == 1 { "it" } else { "them" },
        );
    }

    let summary = Summary::of(report, view, elapsed);
    let _ = write!(
        out,
        "{} {}, {} {}",
        summary.errors,
        plural(summary.errors, "error", "errors"),
        summary.warnings,
        plural(summary.warnings, "warning", "warnings"),
    );

    // Beside the counts rather than in a note below them, because it belongs to
    // the same sentence: this many problems found, and this many questions not
    // answered. Omitted when there is nothing to say, which is nearly always.
    if summary.checks_skipped > 0 {
        let _ = write!(out, ", {} skipped", summary.checks_skipped);
    }

    let _ = write!(
        out,
        " · {} {}, {} {}",
        summary.files_scanned,
        plural(summary.files_scanned, "file", "files"),
        summary.directories_scanned,
        plural(summary.directories_scanned, "directory", "directories"),
    );

    // Only when files were actually read. A structural run has nothing to say
    // here, and "0 parsed, 0 reused" would only invite the question of why.
    if summary.files_parsed + summary.facts_reused > 0 {
        let _ = write!(
            out,
            " · {} parsed, {} reused",
            summary.files_parsed, summary.facts_reused
        );
    }

    // Last, because it is the answer to a question asked after the others:
    // this many findings over this many files, and it took this long.
    let _ = writeln!(out, " · {}", human_duration(elapsed));
}

/// The JSON envelope for a single-file check.
#[derive(Debug, Serialize)]
struct JsonSingle<'a> {
    version: u32,
    path: &'a archwarden_core::path::RepoRelPath,
    findings: &'a [Finding],
    /// Always present, even when empty. A caller needs to see that the list is
    /// empty rather than infer it from absence -- that is the whole point of
    /// reporting skips (correction C6).
    skipped: Vec<JsonSkipped<'a>>,
}

#[derive(Debug, Serialize)]
struct JsonSkipped<'a> {
    rule_id: &'a str,
    reason: &'static str,
}

/// Writes a single-file check in the requested format.
pub fn render_single(
    single: &archwarden_engine::single::Single,
    format: Format,
    out: &mut dyn std::io::Write,
) {
    match format {
        Format::Text => render_single_text(single, out),
        Format::Json => render_single_json(single, out),
    }
}

fn render_single_json(single: &archwarden_engine::single::Single, out: &mut dyn std::io::Write) {
    let envelope = JsonSingle {
        version: REPORT_VERSION,
        path: &single.path,
        findings: &single.findings,
        skipped: single
            .skipped
            .iter()
            .map(|skipped| JsonSkipped {
                rule_id: skipped.rule_id.as_str(),
                reason: skipped.reason.as_str(),
            })
            .collect(),
    };

    match serde_json::to_string_pretty(&envelope) {
        Ok(json) => {
            let _ = writeln!(out, "{json}");
        }
        Err(error) => {
            let _ = writeln!(out, r#"{{"error":"cannot serialise report: {error}"}}"#);
        }
    }
}

fn render_single_text(single: &archwarden_engine::single::Single, out: &mut dyn std::io::Write) {
    for finding in &single.findings {
        render_finding(finding, finding.path.as_str(), out);
    }

    for skipped in &single.skipped {
        let _ = writeln!(
            out,
            "note: rule `{}` was not checked — {}",
            skipped.rule_id,
            skipped.reason.explain()
        );
    }

    if single.findings.is_empty() && single.skipped.is_empty() {
        let _ = writeln!(out, "{} is fine.", single.path);
    }
}

fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

/// One sentence for what was found.
///
/// Shared with the hook, so a blocked write and a failing `check` describe the
/// same problem in the same words.
pub(crate) fn describe_observed(observed: &Observed) -> String {
    match observed {
        Observed::UnexpectedSubfolder { name } => {
            format!("folder `{name}` is not allowed here")
        }
        Observed::DiscouragedSubfolder { name } => {
            format!("folder `{name}` is allowed for now, as documented debt")
        }
        Observed::UnexpectedFilename { name } => {
            format!("filename `{name}` matches none of the allowed patterns")
        }
        Observed::ExportMissing { name } => format!("no export named `{name}`"),
        Observed::ExportWrongKind { name, found } => {
            let kinds: Vec<_> = found.iter().map(ExportKind::as_str).collect();
            format!("`{name}` is declared as {}", join_or(&kinds, "nothing"))
        }
        Observed::OnlyDefaultExport => {
            "the only export is a default, whose name does not bind importers".to_owned()
        }
        Observed::ReexportOfUnknownKind { name, from } => {
            format!("`{name}` is re-exported from `{from}`, so its kind is not determinable here")
        }
        Observed::Passthrough {
            exports,
            whole_file,
        } => {
            let names = exports
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let forwards = if exports.len() == 1 {
                "only forwards"
            } else {
                "only forward"
            };
            if *whole_file {
                format!("adds nothing of its own: {names} {forwards} another module")
            } else {
                // A different sentence, because it is a different decision:
                // the file is real and part of it is an indirection.
                format!("{names} {forwards} another module; the rest of the file is its own")
            }
        }
        Observed::SiblingMissing { path } => format!("`{path}` does not exist"),
        Observed::SpecIsEmpty { path } => format!("`{path}` contains no test cases"),
        Observed::ForbiddenImport {
            specifier,
            resolved,
        } => format!("imports `{specifier}`, which resolves to `{resolved}`"),
        Observed::ForbiddenPackageImport { specifier, package } => {
            // Named separately only when they differ, because for a deep import
            // they do and reading "imports `three/examples/jsm/loaders/
            // GLTFLoader.js`" without being told the rule is about `three`
            // leaves the reader to work out which package they hit.
            //
            // `node:` is stripped from both first: `fs` is not *part of*
            // `node:fs`, it is the same module spelled the other way, and
            // saying otherwise reads as a bug in the rule.
            let bare = |name: &str| name.strip_prefix("node:").unwrap_or(name).to_owned();
            if bare(specifier) == bare(package) {
                format!("imports the package `{package}`")
            } else {
                format!("imports `{specifier}`, which is part of the package `{package}`")
            }
        }
        Observed::RequiredImportMissing => "no import satisfies the requirement".to_owned(),
        Observed::RequiredCallMissing { symbol } => {
            format!("`{symbol}` is imported but never called")
        }
        Observed::RequiredImportForCallMissing { symbol, module } => {
            format!("`{symbol}` is not imported from `{module}`")
        }
        // `Observed` is non_exhaustive; a variant added later says what it is
        // rather than failing to compile here.
        other => format!("{other:?}"),
    }
}

/// One sentence for what was required.
///
/// Shared with `describe`, which renders the same expectations for a file that
/// does not exist yet. One renderer, so the gate and the informant can never
/// word the same requirement differently -- decision 9.
pub(crate) fn describe_expectation(expectation: &Expectation) -> String {
    match expectation {
        Expectation::AllowedSubfolders { allowed, warn } => {
            let mut parts = vec![format!("one of {}", join_or(allowed, "no folders"))];
            if !warn.is_empty() {
                parts.push(format!("or {} as a warning", join_or(warn, "")));
            }
            parts.join(", ")
        }
        Expectation::FilenamePattern { patterns } => {
            format!("a name matching {}", join_or(patterns, "no pattern"))
        }
        Expectation::RequiredExport {
            name,
            signature_hint,
            ..
        } => signature_hint.as_ref().map_or_else(
            || format!("an export named `{name}`"),
            |hint| format!("an export named `{name}`, shaped like `{hint}`"),
        ),
        Expectation::NoPassthrough { forms } => {
            format!(
                "a file must add something of its own, not only {}",
                forms
                    .iter()
                    .map(|form| match form.as_str() {
                        "reexport" => "re-export another module",
                        "alias" => "rename an import",
                        "wrapper" => "wrap a call in a one-line function",
                        other => other,
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        Expectation::RequiredSibling {
            path,
            non_empty_spec,
        } => {
            if *non_empty_spec {
                format!("`{path}`, containing at least one test case")
            } else {
                format!("`{path}`")
            }
        }
        Expectation::ForbiddenImport {
            patterns, except, ..
        } => {
            let base = format!("no import from {}", join_or(patterns, "anywhere"));
            if except.is_empty() {
                base
            } else {
                format!("{base}, except {}", join_or(except, ""))
            }
        }
        Expectation::ForbiddenPackages {
            packages,
            except_from,
            ..
        } => {
            let base = format!("no import of {}", join_or(packages, "any package"));
            if except_from.is_empty() {
                base
            } else {
                format!("{base}, except from {}", join_or(except_from, ""))
            }
        }
        Expectation::RequiredImport { patterns } => {
            format!("an import from {}", join_or(patterns, "somewhere"))
        }
        Expectation::RequiredCall {
            symbol,
            imported_from,
        } => format!("a call to `{symbol}`, imported from `{imported_from}`"),
        other => format!("{other:?}"),
    }
}

/// Renders a list as `` `a`, `b` or `c` ``.
fn join_or(items: &[impl AsRef<str>], empty: &str) -> String {
    let quoted: Vec<String> = items
        .iter()
        .map(|item| format!("`{}`", item.as_ref()))
        .collect();

    match quoted.split_last() {
        None => empty.to_owned(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        facts::ExportTags,
        ids::{ModuleId, RuleId},
        level::Level,
        path::RepoRelPath,
    };
    use camino::Utf8PathBuf;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn finding(level: Level, module: Option<&str>) -> Finding {
        Finding {
            rule_id: RuleId::new("domain-entity-shape").expect("valid"),
            module_id: module.map(|m| ModuleId::new(m).expect("valid")),
            level,
            path: path("packages/domain/src/user/wrong-folder"),
            span: None,
            observed: Observed::UnexpectedSubfolder {
                name: "wrong-folder".to_owned(),
            },
            expected: Expectation::AllowedSubfolders {
                allowed: vec!["types".to_owned(), "calcs".to_owned()],
                warn: Vec::new(),
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
        }
    }

    fn outcomes(in_repo: usize, external: usize, builtin: usize, unresolved: usize) -> Report {
        Report {
            imports: archwarden_engine::resolve::Outcomes {
                in_repo,
                external,
                builtin,
                unresolved,
            },
            ..report(Vec::new())
        }
    }

    /// A fixed duration, so every assertion about the format stays exact.
    /// What the number is does not matter to any of them; that one is there
    /// does.
    const TOOK: std::time::Duration = std::time::Duration::from_millis(12);

    fn rendered(report: &Report, format: Format) -> String {
        rendered_after(report, format, TOOK)
    }

    /// Rendered against a real tree, for the cases that need one on disk.
    fn rendered_at(root: &Utf8Path, report: &Report, format: Format) -> String {
        let mut out = Vec::new();
        render(
            &Rendered {
                root,
                report,
                view: &View::everything(report),
                elapsed: TOOK,
            },
            format,
            &mut out,
        );
        String::from_utf8(out).expect("output is UTF-8")
    }

    fn rendered_after(report: &Report, format: Format, elapsed: std::time::Duration) -> String {
        rendered_view(report, &View::everything(report), format, elapsed)
    }

    fn rendered_view(
        report: &Report,
        view: &View<'_>,
        format: Format,
        elapsed: std::time::Duration,
    ) -> String {
        let mut out = Vec::new();
        render(
            &Rendered {
                // No tree on disk, so a span resolves to nothing and the bare
                // path is used -- which is what these assertions are about.
                root: Utf8Path::new("/nonexistent"),
                report,
                view,
                elapsed,
            },
            format,
            &mut out,
        );
        String::from_utf8(out).expect("output is UTF-8")
    }

    /// A finding of a named rule, at a named path.
    fn from(rule_id: &str, level: Level, at: &str) -> Finding {
        Finding {
            rule_id: RuleId::new(rule_id).expect("valid"),
            path: path(at),
            ..finding(level, None)
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
            breakdown.ids(),
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
            breakdown.rows_for_test(),
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
            Breakdown::by_path(&[module_scope()], &shown).ids(),
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
            Breakdown::by_path(&[module_scope()], &shown).rows_for_test(),
            [("scripts/build.ts", 1, 0)]
        );
    }

    /// Only areas that have something to say. A rule breakdown lists every
    /// rule including the quiet ones -- that a rule was evaluated is an
    /// answer. There is no equivalent list of directories, and printing every
    /// clean directory in a monorepo would bury the ones that are not.
    #[test]
    fn the_path_breakdown_lists_only_what_fired() {
        assert!(Breakdown::by_path(&[module_scope()], &[]).ids().is_empty());
    }

    /// It renders like the rule breakdown, because it is the same table with a
    /// different first column.
    #[test]
    fn the_path_breakdown_renders_as_a_table() {
        let findings = vec![
            from("shape", Level::Error, "packages/domain/src/order/handlers"),
            from(
                "spec",
                Level::Warning,
                "packages/domain/src/invoice/calcs/a.ts",
            ),
        ];
        let shown: Vec<&Finding> = findings.iter().collect();
        let report = report(findings.clone());

        let text = rendered_view(
            &report,
            &View::summarised(&shown, Breakdown::by_path(&[module_scope()], &shown), 0),
            Format::Text,
            TOOK,
        );

        assert_eq!(
            text,
            "packages/domain/src/order    1 error\n\
             packages/domain/src/invoice  1 warning\n\
             \n\
             1 error, 1 warning · 34 files, 12 directories · 12ms\n"
        );
    }

    /// A rule that fired nothing keeps its row. That it was evaluated is the
    /// answer to a question a reader is asking, and a missing row reads as a
    /// disabled rule.
    #[test]
    fn a_rule_with_no_findings_still_has_a_row() {
        let (report, breakdown) = four_rules();
        let text = rendered_view(
            &report,
            &View::summarised(&report.findings.iter().collect::<Vec<_>>(), breakdown, 0),
            Format::Text,
            TOOK,
        );

        assert_eq!(
            text,
            "domain-entity-shape  2 errors\n\
             actions-need-spec    1 warning\n\
             calcs-need-spec      0\n\
             variants-need-spec   0\n\
             \n\
             2 errors, 1 warning · 34 files, 12 directories · 12ms\n"
        );
    }

    /// `--summary` suppresses the per-finding listing. That is the whole
    /// point: on a first migration the listing is hundreds of lines.
    #[test]
    fn a_summary_does_not_list_the_findings() {
        let (report, breakdown) = four_rules();
        let text = rendered_view(
            &report,
            &View::summarised(&report.findings.iter().collect::<Vec<_>>(), breakdown, 0),
            Format::Text,
            TOOK,
        );

        assert!(!text.contains("expected:"), "{text}");
        assert!(!text.contains("packages/domain/a"), "{text}");
    }

    /// A rule with both is described with both, rather than the reader having
    /// to run it again the other way round.
    #[test]
    fn a_rule_with_errors_and_warnings_says_so() {
        let findings = vec![
            from("mixed", Level::Error, "a"),
            from("mixed", Level::Warning, "b"),
            from("mixed", Level::Warning, "c"),
        ];
        let report = report(findings);
        let breakdown = Breakdown::over(["mixed"], &report.findings.iter().collect::<Vec<_>>());

        assert_eq!(breakdown.rows_for_test(), [("mixed", 1, 2)]);
        let text = rendered_view(
            &report,
            &View::summarised(&report.findings.iter().collect::<Vec<_>>(), breakdown, 0),
            Format::Text,
            TOOK,
        );
        assert!(text.starts_with("mixed  1 error, 2 warnings\n"), "{text}");
    }

    /// The JSON half of `--summary`: a map beside the counts, and no
    /// `findings` array. Omitting it is the point -- a `--summary` that still
    /// emitted every finding would give a piping user no size benefit at all.
    #[test]
    fn a_json_summary_carries_the_breakdown_and_drops_the_findings() {
        let (report, breakdown) = four_rules();
        let json = rendered_view(
            &report,
            &View::summarised(&report.findings.iter().collect::<Vec<_>>(), breakdown, 0),
            Format::Json,
            TOOK,
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert!(parsed.get("findings").is_none(), "{json}");
        assert_eq!(
            parsed["summary"]["by_rule"]["domain-entity-shape"]["errors"],
            2
        );
        assert_eq!(
            parsed["summary"]["by_rule"]["domain-entity-shape"]["warnings"],
            0
        );
        assert_eq!(parsed["summary"]["by_rule"]["calcs-need-spec"]["errors"], 0);

        // The order the text uses, kept: a consumer that iterates the map gets
        // the worst rule first, like a reader does.
        let order: Vec<&str> = parsed["summary"]["by_rule"]
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(order[0], "domain-entity-shape");
    }

    /// Without `--summary` the findings array is there, as it always was. A
    /// consumer that never passes the flag sees no change at all.
    #[test]
    fn an_unfiltered_json_report_still_carries_its_findings() {
        let json = rendered(&report(vec![finding(Level::Error, None)]), Format::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert!(parsed["findings"].is_array(), "{json}");
        assert!(parsed["summary"].get("by_rule").is_none(), "{json}");
    }

    /// The line that stops "0 errors" and exit 1 from being a contradiction
    /// the reader cannot resolve. The gate saw findings the filter hid, and
    /// the report has to say so.
    #[test]
    fn hidden_findings_are_admitted() {
        let report = report(vec![
            from("shape", Level::Error, "a"),
            from("spec", Level::Error, "b"),
        ]);
        let shown: Vec<&Finding> = report.findings.iter().take(1).collect();

        let text = rendered_view(&report, &View::filtered(&shown, 1), Format::Text, TOOK);

        assert!(
            text.contains("note: 1 finding hidden by the filters given"),
            "{text}"
        );
    }

    // --- positions ---------------------------------------------------------

    /// A finding with a position gets one, in the form an editor and a
    /// terminal both linkify. Only `import-boundary` carries a span today, and
    /// it is the finding most worth jumping to: the offending import is one
    /// line in a file of many.
    #[test]
    fn a_finding_with_a_span_is_rendered_as_a_position() {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        std::fs::create_dir_all(root.join("src")).expect("create dirs");
        std::fs::write(
            root.join("src/a.ts"),
            "import { a } from './a';\nimport { Repo } from '@org/domain';\n",
        )
        .expect("write");

        // The second import starts at byte 25, which is line 2, column 1.
        let finding = Finding {
            path: path("src/a.ts"),
            span: Some(archwarden_core::facts::Span::new(25, 32)),
            ..finding(Level::Error, None)
        };

        let text = rendered_at(&root, &report(vec![finding]), Format::Text);

        assert!(text.contains("src/a.ts:2:1"), "{text}");
    }

    /// The column is counted in characters, not bytes. A file with an accent
    /// earlier on the line would otherwise send an editor to the wrong place
    /// -- which is worse than no link, because the reader trusts it.
    #[test]
    fn the_column_counts_characters_not_bytes() {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        std::fs::create_dir_all(root.join("src")).expect("create dirs");
        // `café` is five characters and six bytes.
        std::fs::write(root.join("src/a.ts"), "const café = 1; // x\n").expect("write");

        let finding = Finding {
            path: path("src/a.ts"),
            // The `x` is at byte 20, and 19 *characters* precede it, because
            // `é` is two bytes. Column 20, not 21.
            span: Some(archwarden_core::facts::Span::new(20, 21)),
            ..finding(Level::Error, None)
        };

        let text = rendered_at(&root, &report(vec![finding]), Format::Text);

        assert!(text.contains("src/a.ts:1:20"), "{text}");
        assert!(
            !text.contains("src/a.ts:1:21"),
            "counted bytes, not characters: {text}"
        );
    }

    /// A finding with no span keeps the bare path. A structure rule is about a
    /// directory, and `packages/domain/src/order:1:1` would be a link to a
    /// place that is not the problem.
    #[test]
    fn a_finding_without_a_span_keeps_its_bare_path() {
        let text = rendered(&report(vec![finding(Level::Error, None)]), Format::Text);

        assert!(
            text.contains("error   packages/domain/src/user/wrong-folder\n"),
            "{text}"
        );
        assert!(!text.contains(":1:1"), "{text}");
    }

    /// A span pointing past the end of a file that changed under us, or into a
    /// file that is no longer there, gets the bare path rather than a made-up
    /// position.
    #[test]
    fn an_unreadable_or_impossible_position_falls_back_to_the_path() {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        std::fs::create_dir_all(root.join("src")).expect("create dirs");
        std::fs::write(root.join("src/short.ts"), "const a = 1;\n").expect("write");

        for (file, start) in [("src/short.ts", 9_999), ("src/gone.ts", 0)] {
            let finding = Finding {
                path: path(file),
                span: Some(archwarden_core::facts::Span::new(start, start + 1)),
                ..finding(Level::Error, None)
            };

            let text = rendered_at(&root, &report(vec![finding]), Format::Text);
            assert!(text.contains(file), "{text}");
            assert!(!text.contains(&format!("{file}:")), "{text}");
        }
    }

    /// The JSON is untouched. It carries the byte span, which is what a tool
    /// wants; `line:col` is a rendering for a human and a terminal.
    #[test]
    fn the_json_still_carries_the_byte_span() {
        let finding = Finding {
            path: path("src/a.ts"),
            span: Some(archwarden_core::facts::Span::new(25, 32)),
            ..finding(Level::Error, None)
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&rendered(&report(vec![finding]), Format::Json))
                .expect("valid JSON");

        assert_eq!(parsed["findings"][0]["span"]["start"], 25);
        assert_eq!(parsed["findings"][0]["path"], "src/a.ts");
    }

    /// A check nobody could make belongs on the line a reader checks first.
    ///
    /// `check --file` has refused to drop one silently since C6. The full run
    /// named the unreadable file and stopped there, so a repository where
    /// nothing parsed reported as a repository with nothing wrong.
    #[test]
    fn checks_that_could_not_be_made_are_counted_on_the_summary_line() {
        let report = Report {
            checks_skipped: 3,
            ..report(Vec::new())
        };

        assert_eq!(
            rendered(&report, Format::Text),
            "0 errors, 0 warnings, 3 skipped · 34 files, 12 directories · 12ms\n"
        );
    }

    /// One reads as one.
    #[test]
    fn a_single_skipped_check_is_singular() {
        let report = Report {
            checks_skipped: 1,
            ..report(Vec::new())
        };

        assert!(
            rendered(&report, Format::Text).starts_with("0 errors, 0 warnings, 1 skipped ·"),
            "{}",
            rendered(&report, Format::Text)
        );
    }

    /// The count says a run decided less than it looks like. It does not say
    /// which decisions, and `AGENTS.md` asks the reader to act on it — so the
    /// only responses a bare number leaves are to ignore it or to stop.
    /// Issue #12.
    #[test]
    fn the_text_output_names_which_rules_were_skipped_and_where() {
        let report = Report {
            checks_skipped: 2,
            unreadable_files: vec![(path("src/broken.ts"), "unexpected token".to_owned())],
            skipped_checks: vec![
                (
                    "domain-forbids-infrastructure".to_owned(),
                    path("src/broken.ts"),
                ),
                ("usecase-export-name".to_owned(), path("src/broken.ts")),
            ],
            ..report(Vec::new())
        };

        let text = rendered(&report, Format::Text);

        assert!(
            text.contains(
                "      2 checks skipped there: domain-forbids-infrastructure, \
                 usecase-export-name\n"
            ),
            "both rules are named, under the file that cost them: {text}"
        );
    }

    /// One reads as one here too, and the count is the rules on this file
    /// rather than the run's total.
    #[test]
    fn a_lone_skipped_check_names_its_rule_in_the_singular() {
        let report = Report {
            checks_skipped: 1,
            unreadable_files: vec![(path("src/broken.ts"), "unexpected token".to_owned())],
            skipped_checks: vec![("usecase-export-name".to_owned(), path("src/broken.ts"))],
            ..report(Vec::new())
        };

        assert!(
            rendered(&report, Format::Text)
                .contains("      1 check skipped there: usecase-export-name\n"),
            "{}",
            rendered(&report, Format::Text)
        );
    }

    /// A file nobody could read that no rule wanted to look inside still says
    /// so, and says nothing more. The note is about the file; the line under it
    /// is about answers that were lost, and there were none.
    #[test]
    fn an_unreadable_file_no_rule_wanted_names_no_skipped_checks() {
        let report = Report {
            unreadable_files: vec![(path("src/broken.ts"), "unexpected token".to_owned())],
            ..report(Vec::new())
        };

        let text = rendered(&report, Format::Text);

        assert!(
            text.contains("note: `src/broken.ts` was not checked"),
            "{text}"
        );
        assert!(!text.contains("skipped there"), "{text}");
    }

    /// And a run that skipped nothing says nothing, because on almost every
    /// run there is nothing to say and a `0 skipped` would only invite the
    /// question of why it is there.
    #[test]
    fn a_run_that_skipped_nothing_does_not_mention_it() {
        let text = rendered(&report(Vec::new()), Format::Text);

        assert!(!text.contains("skipped"), "{text}");
    }

    /// The JSON carries it always, including as zero: a consumer branching on
    /// it needs the field to exist, and unlike a human it is not distracted by
    /// one.
    #[test]
    fn the_json_always_carries_the_skipped_count() {
        for skipped in [0, 3] {
            let report = Report {
                checks_skipped: skipped,
                ..report(Vec::new())
            };
            let parsed: serde_json::Value =
                serde_json::from_str(&rendered(&report, Format::Json)).expect("valid JSON");

            assert_eq!(parsed["summary"]["checks_skipped"], skipped);
        }
    }

    /// A configuration with no rules has nothing to break down, and a stray
    /// blank line above the totals would be the only thing `--summary` added.
    #[test]
    fn a_configuration_with_no_rules_summarises_to_nothing() {
        let report = report(Vec::new());
        let text = rendered_view(
            &report,
            &View::summarised(&[], Breakdown::over(Vec::<String>::new(), &[]), 0),
            Format::Text,
            TOOK,
        );

        assert_eq!(
            text,
            "0 errors, 0 warnings · 34 files, 12 directories · 12ms\n"
        );
    }

    /// And says nothing when nothing was hidden, including when filters were
    /// given and everything matched.
    #[test]
    fn nothing_hidden_is_not_mentioned() {
        let report = report(vec![from("shape", Level::Error, "a")]);
        let shown: Vec<&Finding> = report.findings.iter().collect();

        let text = rendered_view(&report, &View::filtered(&shown, 0), Format::Text, TOOK);

        assert!(!text.contains("hidden"), "{text}");
    }

    /// The counts describe what is shown, and the JSON says how many were
    /// not: a consumer comparing `errors` against the exit code needs the
    /// number, not the prose.
    #[test]
    fn the_json_reports_what_the_filters_hid() {
        let report = report(vec![
            from("shape", Level::Error, "a"),
            from("spec", Level::Warning, "b"),
        ]);
        let shown: Vec<&Finding> = report.findings.iter().take(1).collect();

        let json = rendered_view(&report, &View::filtered(&shown, 1), Format::Json, TOOK);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(parsed["summary"]["errors"], 1);
        assert_eq!(parsed["summary"]["warnings"], 0, "the warning was filtered");
        assert_eq!(parsed["summary"]["hidden"], 1);
    }

    /// A run that says nothing about how long it took invites the question,
    /// and a linter that claims to be fast should be the one answering it.
    #[test]
    fn the_summary_line_ends_with_how_long_it_took() {
        let text = rendered(&report(Vec::new()), Format::Text);

        assert_eq!(
            text,
            "0 errors, 0 warnings · 34 files, 12 directories · 12ms\n"
        );
    }

    /// The unit follows the magnitude. A linter reporting `0.012s` reads as
    /// slower than it is, and one reporting `312700ms` is unreadable.
    #[test]
    fn the_duration_is_written_at_a_scale_a_reader_can_use() {
        use std::time::Duration;

        for (elapsed, expected) in [
            (Duration::from_micros(400), "<1ms"),
            (Duration::from_millis(1), "1ms"),
            (Duration::from_millis(999), "999ms"),
            (Duration::from_secs(1), "1.0s"),
            (Duration::from_millis(1450), "1.4s"),
            (Duration::from_secs(59), "59.0s"),
            (Duration::from_mins(1), "1m 0s"),
            (Duration::from_secs(83), "1m 23s"),
            (Duration::from_hours(1), "60m 0s"),
        ] {
            assert_eq!(human_duration(elapsed), expected, "{elapsed:?}");
        }
    }

    /// Zero is not a plausible answer for a run that happened, so it is never
    /// printed as one. A user seeing `0ms` would reasonably conclude the run
    /// did not happen.
    #[test]
    fn a_run_too_fast_to_measure_still_says_something() {
        let text = rendered_after(
            &report(Vec::new()),
            Format::Text,
            std::time::Duration::from_nanos(1),
        );

        assert!(text.ends_with("· <1ms\n"), "{text}");
        assert!(!text.contains("0ms"), "{text}");
    }

    /// The machine-readable half gets the raw number, not the prose. A
    /// consumer comparing two runs needs to subtract them.
    #[test]
    fn the_json_carries_the_duration_as_a_number() {
        let parsed: serde_json::Value = serde_json::from_str(&rendered_after(
            &report(Vec::new()),
            Format::Json,
            std::time::Duration::from_millis(1450),
        ))
        .expect("valid JSON");

        assert_eq!(parsed["summary"]["duration_ms"], 1450);
    }

    /// The text a user actually reads. Written out by hand rather than
    /// accepted from a run, so the assertion is about what the format *should*
    /// be, not what it happens to be.
    #[test]
    fn the_text_format_reads_as_intended() {
        let text = rendered(
            &report(vec![finding(Level::Error, Some("domain"))]),
            Format::Text,
        );

        assert_eq!(
            text,
            "error   packages/domain/src/user/wrong-folder\n\
             \x20       [domain] domain-entity-shape — folder `wrong-folder` is not allowed here\n\
             \x20       expected: one of `types` or `calcs`\n\
             \n\
             1 error, 0 warnings · 34 files, 12 directories · 12ms\n"
        );
    }

    /// A rule with no module reports as `[*]`, which is how the config's
    /// top-level `rules` array shows up.
    #[test]
    fn a_rule_without_a_module_renders_as_a_star() {
        let text = rendered(&report(vec![finding(Level::Error, None)]), Format::Text);
        assert!(text.contains("[*] domain-entity-shape"), "{text}");
    }

    /// The summary is the line a reader checks first, so its counts and its
    /// grammar both have to be right.
    #[test]
    fn the_summary_counts_and_pluralises() {
        let clean = rendered(&report(Vec::new()), Format::Text);
        assert_eq!(
            clean,
            "0 errors, 0 warnings · 34 files, 12 directories · 12ms\n"
        );

        let mixed = rendered(
            &report(vec![
                finding(Level::Error, None),
                finding(Level::Warning, None),
                finding(Level::Warning, None),
            ]),
            Format::Text,
        );
        assert!(
            mixed.ends_with("1 error, 2 warnings · 34 files, 12 directories · 12ms\n"),
            "{mixed}"
        );

        let singular = Report {
            directories_scanned: 1,
            files_scanned: 1,
            ..report(Vec::new())
        };
        assert!(
            rendered(&singular, Format::Text).contains("1 file, 1 directory"),
            "{}",
            rendered(&singular, Format::Text)
        );
    }

    /// A file that was not checked is said out loud, in both formats. A
    /// clean-looking report that quietly skipped a file is worse than no
    /// report -- and this is now the only way a run can admit it saw less than
    /// everything, since every rule kind reaches an engine.
    #[test]
    fn an_unchecked_file_is_announced_in_both_formats() {
        let report = Report {
            unreadable_files: vec![(
                path("src/user/broken.ts"),
                "stream did not contain valid UTF-8".to_owned(),
            )],
            ..report(Vec::new())
        };

        let text = rendered(&report, Format::Text);
        assert!(text.contains("src/user/broken.ts"), "{text}");
        assert!(text.contains("was not checked"), "{text}");

        let json = rendered(&report, Format::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["unreadable_files"][0]["path"], "src/user/broken.ts");
        assert_eq!(
            parsed["unreadable_files"][0]["reason"],
            "stream did not contain valid UTF-8"
        );
    }

    /// The JSON shape is a contract with agents, so its envelope is asserted
    /// field by field rather than by eyeballing a dump.
    #[test]
    fn the_json_envelope_is_versioned_and_summarised() {
        let json = rendered(
            &report(vec![finding(Level::Error, Some("domain"))]),
            Format::Json,
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(parsed["version"], 0);
        assert_eq!(parsed["summary"]["errors"], 1);
        assert_eq!(parsed["summary"]["warnings"], 0);
        assert_eq!(parsed["summary"]["files_scanned"], 34);
        assert_eq!(parsed["summary"]["directories_scanned"], 12);
        assert_eq!(parsed["summary"]["files_parsed"], 0);
        assert_eq!(parsed["summary"]["facts_reused"], 0);

        let first = &parsed["findings"][0];
        assert_eq!(first["rule_id"], "domain-entity-shape");
        assert_eq!(first["module_id"], "domain");
        assert_eq!(first["level"], "error");
        assert_eq!(first["path"], "packages/domain/src/user/wrong-folder");
        assert_eq!(first["observed"]["type"], "unexpected-subfolder");
        assert_eq!(first["expected"]["type"], "allowed-subfolders");
    }

    /// An empty `unreadable_files` is absent rather than an empty array, so
    /// the common report stays small.
    #[test]
    fn a_clean_json_report_omits_the_empty_list() {
        let json = rendered(&report(Vec::new()), Format::Json);
        assert!(!json.contains("unreadable_files"), "{json}");
    }

    /// The prose comes from the same values the JSON carries, so the two can
    /// never describe one finding differently.
    #[test]
    fn every_observation_has_a_sentence() {
        let cases = [
            (
                Observed::UnexpectedFilename {
                    name: "helpers.ts".to_owned(),
                },
                "helpers.ts",
            ),
            (
                Observed::ExportMissing {
                    name: "Foo".to_owned(),
                },
                "no export named",
            ),
            (
                Observed::ExportWrongKind {
                    name: "Foo".to_owned(),
                    found: ExportTags::only(ExportKind::Const).with(ExportKind::Arrow),
                },
                "`arrow` or `const`",
            ),
            (Observed::OnlyDefaultExport, "does not bind importers"),
            (
                Observed::SiblingMissing {
                    path: path("a.spec.ts"),
                },
                "does not exist",
            ),
            (
                Observed::RequiredCallMissing {
                    symbol: "Event.save".to_owned(),
                },
                "never called",
            ),
        ];

        for (observed, expected_fragment) in cases {
            let sentence = describe_observed(&observed);
            assert!(
                sentence.contains(expected_fragment),
                "{observed:?} rendered as {sentence}"
            );
        }
    }

    #[test]
    fn every_expectation_has_a_sentence() {
        let sibling = describe_expectation(&Expectation::RequiredSibling {
            path: path("src/user/user.spec.ts"),
            non_empty_spec: true,
        });
        assert!(sibling.contains("at least one test case"), "{sibling}");

        let call = describe_expectation(&Expectation::RequiredCall {
            symbol: "Event.save".to_owned(),
            imported_from: "@org/domain".to_owned(),
        });
        assert!(call.contains("Event.save"), "{call}");
        assert!(call.contains("@org/domain"), "{call}");

        let forbidden = describe_expectation(&Expectation::ForbiddenImport {
            patterns: vec!["packages/domain/**".to_owned()],
            except: vec!["packages/domain/src/*/types/**".to_owned()],
            include_type_only: true,
        });
        assert!(forbidden.contains("except"), "{forbidden}");
    }

    /// A warn-listed folder is part of the expectation, so a reader can see
    /// why one folder is an error and another a warning.
    #[test]
    fn a_warn_list_appears_in_the_expectation() {
        let sentence = describe_expectation(&Expectation::AllowedSubfolders {
            allowed: vec!["types".to_owned()],
            warn: vec!["shared".to_owned()],
        });

        assert_eq!(sentence, "one of `types`, or `shared` as a warning");
    }

    /// A structural run reads no file, and its summary says nothing about a
    /// cache -- there is nothing to say. The common line stays short.
    #[test]
    fn a_run_that_read_nothing_says_nothing_about_the_cache() {
        let text = rendered(&report(Vec::new()), Format::Text);
        assert_eq!(
            text,
            "0 errors, 0 warnings · 34 files, 12 directories · 12ms\n"
        );
    }

    /// When files were read, the split between parsed and reused is shown.
    /// Otherwise a cache that silently stopped working is invisible until
    /// someone thinks to time two runs.
    #[test]
    fn a_run_that_read_files_reports_the_cache_split() {
        let cold = rendered(
            &Report {
                files_parsed: 34,
                ..report(Vec::new())
            },
            Format::Text,
        );
        assert!(cold.ends_with("· 34 parsed, 0 reused · 12ms\n"), "{cold}");

        let warm = rendered(
            &Report {
                facts_reused: 34,
                ..report(Vec::new())
            },
            Format::Text,
        );
        assert!(warm.ends_with("· 0 parsed, 34 reused · 12ms\n"), "{warm}");
    }

    /// The counts reach JSON too, where a tool can chart them.
    #[test]
    fn the_json_summary_carries_the_cache_split() {
        let json = rendered(
            &Report {
                files_parsed: 2,
                facts_reused: 32,
                ..report(Vec::new())
            },
            Format::Json,
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(parsed["summary"]["files_parsed"], 2);
        assert_eq!(parsed["summary"]["facts_reused"], 32);
    }

    /// An import a boundary rule could not place is an import it did not
    /// check. A clean report that stayed quiet about it would be lying about
    /// its own coverage, which is the same reason unreadable files are named.
    #[test]
    fn imports_that_did_not_resolve_are_announced() {
        let text = rendered(&outcomes(40, 12, 3, 7), Format::Text);
        assert!(text.contains("7 imports"), "{text}");
        assert!(text.contains("not resolve"), "{text}");

        let json = rendered(&outcomes(40, 12, 3, 7), Format::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["summary"]["imports"]["in_repo"], 40);
        assert_eq!(parsed["summary"]["imports"]["external"], 12);
        assert_eq!(parsed["summary"]["imports"]["builtin"], 3);
        assert_eq!(parsed["summary"]["imports"]["unresolved"], 7);
    }

    /// One is one. The note is read by someone deciding whether to trust the
    /// run, so its grammar has to be right.
    #[test]
    fn a_single_unresolved_import_reads_as_singular() {
        let text = rendered(&outcomes(1, 0, 0, 1), Format::Text);
        assert!(text.contains("1 import could not"), "{text}");
    }

    /// A run where everything resolved says nothing, and a run with no
    /// boundary rule resolved nothing at all -- neither should raise a note.
    #[test]
    fn a_run_with_nothing_unresolved_stays_quiet() {
        assert!(!rendered(&outcomes(40, 12, 3, 0), Format::Text).contains("resolve"));
        assert!(!rendered(&report(Vec::new()), Format::Text).contains("resolve"));
    }

    /// A run that resolved nothing carries no `imports` object at all, so a
    /// consumer can tell "no boundary rule" from "everything resolved".
    #[test]
    fn a_run_that_resolved_nothing_omits_the_import_summary() {
        let json = rendered(&report(Vec::new()), Format::Json);
        assert!(!json.contains("\"imports\""), "{json}");
    }

    /// Lists read as prose rather than as a debug dump, at every length.
    #[test]
    fn lists_are_joined_as_english() {
        assert_eq!(join_or(&["a"], "none"), "`a`");
        assert_eq!(join_or(&["a", "b"], "none"), "`a` or `b`");
        assert_eq!(join_or(&["a", "b", "c"], "none"), "`a`, `b` or `c`");
        assert_eq!(join_or(&Vec::<String>::new(), "none"), "none");
    }
}
