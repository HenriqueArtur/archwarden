//! The terminal format: findings, breakdowns, and the run summary.

use archwarden_api::describe::describe_observed;
use archwarden_api::present::Breakdown;
use archwarden_api::render::{Reasons, Rendered, Summary};
use archwarden_core::{finding::Finding, path::RepoRelPath};

use super::prose::{describe_expectation, render_unattempted_skips};
use super::{Format, Positions, renderer};

/// How long the run took, at a scale a reader can use.
///
/// Milliseconds below a second, one decimal of seconds below a minute, and
/// minutes above. `0ms` is never printed: a run that happened took *some*
/// time, and a reader seeing zero would reasonably conclude it did not.
pub(super) fn human_duration(elapsed: std::time::Duration) -> String {
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

/// One finding, in the shape a reader has learned to scan.
///
/// Shared by the full report and the single-file check, so a hook and a
/// commit-time run word the same finding identically.
/// The findings somebody allowed on purpose, and why.
///
/// A section of its own rather than a count, and never omitted when there is
/// one: a suppressed finding is not an absent finding. A run with forty of
/// them must not look like a clean run at a glance, which is the constraint
/// issue #72 is built on -- `// eslint-disable-next-line` with no explanation
/// is how debt becomes invisible. The reason is on the line for the same
/// reason it is mandatory in the marker.
pub(super) fn render_suppressed(
    suppressed: &[archwarden_engine::run::Suppressed],
    out: &mut dyn std::io::Write,
) {
    if suppressed.is_empty() {
        return;
    }

    let _ = writeln!(
        out,
        "\n{} {} allowed on purpose:\n",
        suppressed.len(),
        plural(suppressed.len(), "finding", "findings")
    );
    for allowed in suppressed {
        let _ = writeln!(
            out,
            "  {} · {} — {}",
            allowed.finding.path, allowed.finding.rule_id, allowed.reason
        );
    }
    let _ = writeln!(out);
}

pub(super) fn render_finding(
    finding: &Finding,
    at: &str,
    why: Option<&str>,
    decision: Option<&archwarden_core::compiled::CompiledDecision>,
    out: &mut dyn std::io::Write,
) {
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
    // Wrapped and indented under the finding rather than beside it: this is a
    // paragraph, and the two lines above are a diagnosis.
    if let Some(why) = why {
        let _ = writeln!(out, "        why: {why}");
    }
    // And the decision the rule serves, in the block every terminal surface
    // shares. Issue #100.
    if let Some(decision) = decision {
        let _ = write!(
            out,
            "{}",
            archwarden_api::describe::describe_decision_refusing(
                decision,
                "        ",
                decision.refusal_by(&finding.rule_id),
            )
        );
    }
    let _ = writeln!(out);
}

pub(super) fn render_breakdown(breakdown: &Breakdown, out: &mut dyn std::io::Write) {
    // A configuration with no rules has nothing to break down, and a blank
    // line above the totals would be the only thing `--summary` contributed.
    let rows: Vec<(&str, usize, usize)> = breakdown.rows().collect();
    if rows.is_empty() {
        return;
    }

    let id_width = rows.iter().map(|(id, _, _)| id.len()).max().unwrap_or(0);
    let count_width = rows
        .iter()
        .map(|(_, errors, warnings)| errors.max(warnings).to_string().len())
        .max()
        .unwrap_or(1);

    for &(rule_id, errors, warnings) in &rows {
        let (count, tail) = match (errors, warnings) {
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

        let _ = writeln!(out, "{rule_id:<id_width$}  {count:>count_width$}{tail}");
    }

    let _ = writeln!(out);
}

pub(super) fn render_text(rendered: &Rendered<'_>, out: &mut dyn std::io::Write) {
    let Rendered {
        root,
        report,
        view,
        reasons,
        elapsed,
        // Printed by `report_standing`, after this and after the `--html`
        // note, which is where a reader of the text format has always found
        // it. The JSON format carries the same number inside the document
        // instead. Named rather than elided so a field added to `Rendered`
        // still fails to compile here.
        standing: _,
        // The text format does not print it. A human at a terminal ran the
        // command a moment ago and knows what day it is; the JSON carries it
        // because a report read a week later does not.
        as_of: _,
    } = *rendered;

    if let Some(breakdown) = view.breakdown() {
        render_breakdown(breakdown, out);
    } else {
        let mut positions = Positions::default();
        // A repository with two hundred findings over six rules must not print
        // two hundred paragraphs. Six, at the point each rule first comes up,
        // is where a reader is already looking. Issue #46.
        let mut explained = std::collections::BTreeSet::new();
        // A second set, keyed by *decision* rather than by rule: a decision
        // serving six rules would otherwise print six identical blocks, which
        // is the once-per-rule economy above failing one level up.
        let mut decided = std::collections::BTreeSet::new();
        for finding in view.findings() {
            let at = positions.label(root, finding);
            let why = reasons
                .of_rule(&finding.rule_id)
                .filter(|_| explained.insert(finding.rule_id.as_str()));
            let decision = reasons
                .decision_of_rule(&finding.rule_id)
                .filter(|decision| decided.insert(decision.id.clone()));
            render_finding(finding, &at, why, decision, out);
        }
    }

    // Suppressions, above the unreadable files and below the findings. Never
    // omitted and never folded into a count: a suppressed finding is not an
    // absent finding, and a run with forty of them must not look like a clean
    // one at a glance. That is the constraint issue #72 is built on --
    // `// eslint-disable-next-line` with no explanation is how debt becomes
    // invisible, and the reason is on the line here for the same reason it is
    // mandatory in the marker.
    render_suppressed(&report.suppressed, out);

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

    // The other half of the same number. A skip on a file that *is* named above
    // is a bug to investigate; a skip on one that is not was never attempted --
    // a language this configuration did not ask archwarden to read, most often.
    // `1 skipped` could not tell those apart, and they are opposite decisions.
    // Issue #13.
    render_unattempted_skips(report, out);

    // Without this, `0 errors` beside exit 1 is a contradiction the reader
    // cannot resolve: the gate counts what was evaluated, and the line above
    // counts what was asked for.
    if view.hidden() > 0 {
        let _ = writeln!(
            out,
            "note: {} {} hidden by the filters given",
            view.hidden(),
            plural(view.hidden(), "finding", "findings")
        );
    }

    // An import a boundary rule could not place is an import it did not check.
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
        render_unresolved_imports(&report.imports.unresolved_imports, out);
    }

    let summary = Summary::of(rendered);
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

    // In the one line everybody reads, so a repository whose suppressions are
    // growing finds out without anybody running a second command. A number
    // that only ever goes up, visibly, is a number somebody eventually acts on.
    if summary.suppressed > 0 {
        let _ = write!(out, ", {} allowed", summary.suppressed);
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

/// How many files may be named under the unresolved-imports note.
///
/// The list is not always short. A repository whose dependencies are not
/// installed cannot place a single bare specifier, and a line per file would
/// push the findings the user came for off the screen. Ten is enough for the
/// case the note is for -- a package mid-extraction, whose aliases resolve
/// nowhere yet -- and the JSON carries every one for the CI job that gates on
/// them.
pub(super) const UNRESOLVED_FILES_SHOWN: usize = 10;

/// Which file wrote each import nothing could place, under the note counting
/// them.
///
/// Without this the note says an import is unprotected and gives the reader
/// nowhere to look; issue #18 found its own by deleting imports until the
/// count moved. Grouped by file rather than a line each, because one file
/// written against an alias that resolves nowhere usually has several.
pub(super) fn render_unresolved_imports(
    unresolved: &[(RepoRelPath, String)],
    out: &mut dyn std::io::Write,
) {
    // Sorted by the engine, so a file's imports arrive together.
    let mut by_file: Vec<(&RepoRelPath, Vec<&str>)> = Vec::new();
    for (path, specifier) in unresolved {
        match by_file.last_mut() {
            Some((last, specifiers)) if *last == path => specifiers.push(specifier),
            _ => by_file.push((path, vec![specifier.as_str()])),
        }
    }

    for (path, specifiers) in by_file.iter().take(UNRESOLVED_FILES_SHOWN) {
        let written: Vec<String> = specifiers
            .iter()
            .map(|specifier| format!("`{specifier}`"))
            .collect();
        let _ = writeln!(out, "      `{path}`: {}", written.join(", "));
    }

    // Saying how many were left out, rather than trailing off: a reader who
    // cannot tell whether the list ended or was cut has to check.
    if let Some(hidden) = by_file.len().checked_sub(UNRESOLVED_FILES_SHOWN)
        && hidden > 0
    {
        let _ = writeln!(
            out,
            "      … and {hidden} more {}, all of them under `--format json`",
            plural(hidden, "file", "files"),
        );
    }
}

/// Writes a single-file check in the requested format.
pub fn render_single(
    single: &archwarden_engine::single::Single,
    reasons: &Reasons,
    format: Format,
    out: &mut dyn std::io::Write,
) {
    renderer(format).render_single(single, reasons, out);
}

pub(super) fn render_single_text(
    single: &archwarden_engine::single::Single,
    reasons: &Reasons,
    out: &mut dyn std::io::Write,
) {
    let mut explained = std::collections::BTreeSet::new();
    let mut decided = std::collections::BTreeSet::new();
    for finding in &single.findings {
        let why = reasons
            .of_rule(&finding.rule_id)
            .filter(|_| explained.insert(finding.rule_id.as_str()));
        let decision = reasons
            .decision_of_rule(&finding.rule_id)
            .filter(|decision| decided.insert(decision.id.clone()));
        render_finding(
            finding,
            super::display_path(&finding.path),
            why,
            decision,
            out,
        );
    }

    for skipped in &single.skipped {
        let _ = writeln!(
            out,
            "note: rule `{}` was not checked — {}",
            skipped.rule_id,
            skipped.reason.explain()
        );
    }

    // A boundary rule that ran against an import nothing could place ran
    // blind, and `is fine.` is not what happened. Issue #18.
    for specifier in &single.unresolved_imports {
        let _ = writeln!(
            out,
            "note: `{specifier}` did not resolve, so boundary rules did not see it"
        );
    }

    if single.findings.is_empty()
        && single.skipped.is_empty()
        && single.unresolved_imports.is_empty()
    {
        let _ = writeln!(out, "{} is fine.", single.path);
    }
}

pub(super) fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}
