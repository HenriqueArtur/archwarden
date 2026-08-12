//! Rendering a report.
//!
//! Two formats, one source. Text is for a human reading a terminal; JSON is
//! for an agent or another tool, and its shape is a contract -- it carries a
//! version so a consumer can tell when that contract changes.
//!
//! The prose in the text format is generated from the same `Observed` and
//! `Expectation` values the JSON carries, so the two can never describe a
//! finding differently.

use std::fmt::Write as _;

use archwarden_core::{
    finding::{Expectation, Finding},
    path::RepoRelPath,
};
use archwarden_engine::run::Report;
use camino::Utf8Path;

// The sentence a finding is described by, and the comma rule it is built
// with. Both moved to archwarden-api in issue #63: the baseline writes
// `describe_observed`'s output into a committed file, so it is part of a
// format rather than terminal output — and a format cannot live in a
// renderer that the operations would have to reach back into.
//
// Re-exported, not just imported: `crate::report::describe_observed` is what
// four modules already call it, and renaming a path at five call sites is a
// diff about nothing.
pub use archwarden_api::describe::{describe_observed, join_or};

// What of a run to show is a decision, not a rendering: which findings the
// baseline left, which the filters kept, and whether the reader asked for a
// listing or for counts. All three are answers MCP and an LSP need in exactly
// the form `check` gets them, so `View` and `Breakdown` moved to
// archwarden-api with the rest of Present. What stayed here is the part that
// turns one into bytes.
pub use archwarden_api::present::{Breakdown, View};

// The JSON report and everything it needs. MCP has to emit the same object
// `check --format json` does, so the contract lives where the operations are
// rather than in a surface a server would have to depend on. What stayed here
// is the human text and the HTML page: both implement `Renderer` from this
// side, which is what the trait is for.
pub use archwarden_api::render::{REPORT_VERSION, Reasons, Rendered, Renderer, Summary};

/// How to render a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Format {
    /// Grouped, human-readable text.
    #[default]
    Text,
    /// A stable, versioned JSON object.
    Json,
}

/// The human-readable format: grouped prose for somebody at a terminal.
///
/// A [`Renderer`] like the others. It stays in this crate rather than in
/// `archwarden-api` because it is this surface's own — it resolves a finding's
/// byte offset into `line:column` by reading source files off disk, which is a
/// thing to do for a terminal and not a thing an MCP response wants.
#[derive(Debug, Clone, Copy, Default)]
pub struct Text;

impl Renderer for Text {
    fn render(&self, rendered: &Rendered<'_>, out: &mut dyn std::io::Write) {
        render_text(rendered, out);
    }

    fn render_single(
        &self,
        single: &archwarden_engine::single::Single,
        reasons: &Reasons,
        out: &mut dyn std::io::Write,
    ) {
        render_single_text(single, reasons, out);
    }
}

/// The renderer a format names.
///
/// The one place the choice is made. SARIF (#64) is one more implementation
/// and one more arm here — not a fourth branch in `render`, another in
/// `render_single`, and a third wherever the page is written.
fn renderer(format: Format) -> &'static dyn Renderer {
    match format {
        Format::Text => &Text,
        Format::Json => &archwarden_api::render::Json,
    }
}

/// Writes a report in the requested format.
///
/// One place decides which renderer, so SARIF (#64) is one more
/// implementation and one more arm here rather than a branch in several
/// functions. That is what the trait bought.
pub fn render(rendered: &Rendered<'_>, format: Format, out: &mut dyn std::io::Write) {
    renderer(format).render(rendered, out);
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

/// One finding, in the shape a reader has learned to scan.
///
/// Shared by the full report and the single-file check, so a hook and a
/// commit-time run word the same finding identically.
fn render_finding(finding: &Finding, at: &str, why: Option<&str>, out: &mut dyn std::io::Write) {
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
    let _ = writeln!(out);
}

fn render_breakdown(breakdown: &Breakdown, out: &mut dyn std::io::Write) {
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

fn render_text(rendered: &Rendered<'_>, out: &mut dyn std::io::Write) {
    let Rendered {
        root,
        report,
        view,
        reasons,
        elapsed,
    } = *rendered;

    if let Some(breakdown) = view.breakdown() {
        render_breakdown(breakdown, out);
    } else {
        let mut positions = Positions::default();
        // A repository with two hundred findings over six rules must not print
        // two hundred paragraphs. Six, at the point each rule first comes up,
        // is where a reader is already looking. Issue #46.
        let mut explained = std::collections::BTreeSet::new();
        for finding in view.findings() {
            let at = positions.label(root, finding);
            let why = reasons
                .of_rule(&finding.rule_id)
                .filter(|_| explained.insert(finding.rule_id.as_str()));
            render_finding(finding, &at, why, out);
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

/// How many files may be named under the unresolved-imports note.
///
/// The list is not always short. A repository whose dependencies are not
/// installed cannot place a single bare specifier, and a line per file would
/// push the findings the user came for off the screen. Ten is enough for the
/// case the note is for -- a package mid-extraction, whose aliases resolve
/// nowhere yet -- and the JSON carries every one for the CI job that gates on
/// them.
const UNRESOLVED_FILES_SHOWN: usize = 10;

/// Which file wrote each import nothing could place, under the note counting
/// them.
///
/// Without this the note says an import is unprotected and gives the reader
/// nowhere to look; issue #18 found its own by deleting imports until the
/// count moved. Grouped by file rather than a line each, because one file
/// written against an alias that resolves nowhere usually has several.
fn render_unresolved_imports(unresolved: &[(RepoRelPath, String)], out: &mut dyn std::io::Write) {
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

fn render_single_text(
    single: &archwarden_engine::single::Single,
    reasons: &Reasons,
    out: &mut dyn std::io::Write,
) {
    let mut explained = std::collections::BTreeSet::new();
    for finding in &single.findings {
        let why = reasons
            .of_rule(&finding.rule_id)
            .filter(|_| explained.insert(finding.rule_id.as_str()));
        render_finding(finding, finding.path.as_str(), why, out);
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

fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

/// One sentence for what was required.
///
/// Shared with `describe`, which renders the same expectations for a file that
/// does not exist yet. One renderer, so the gate and the informant can never
/// word the same requirement differently -- decision 9.
/// The run as a page: the map, the walls, the pressure on them, and the
/// blind spots.
///
/// Ordered for somebody about to *change* the architecture rather than to
/// satisfy it. That reader is not asking "is it clean" -- they are asking where
/// reality is pushing against the design, so the pressure is grouped by **wall**
/// and not by file. A wall crossed eleven times is a question about the wall.
///
/// Accepted debt is given the same weight as a current error, and that is
/// deliberate: it is where somebody already decided the design was losing, and
/// today it is buried in `.archwarden/baseline.json` where nobody reads it.
#[must_use]
pub fn html_page(
    config: &archwarden_core::compiled::CompiledConfig,
    tree: &archwarden_engine::walk::RepoTree,
    report: &Report,
    shown: &[&Finding],
    baseline: Option<&crate::baseline::Baseline>,
    language: crate::phrases::Language,
) -> String {
    use std::io::Write as _;

    use crate::html::{close, escape, open};
    use crate::matrix::{Cell, Matrix};

    let matrix = Matrix::of(config, tree, &report.findings);
    let accepted = baseline.map_or(0, |baseline| baseline.entries().count());

    let mut out: Vec<u8> = Vec::new();
    let say = language.phrases();
    open(say.report_title(), language, &mut out);

    let crossed = matrix
        .rows
        .iter()
        .flatten()
        .filter(|cell| matches!(cell, Cell::Crossed(_)))
        .count();
    let walls = matrix
        .rows
        .iter()
        .flatten()
        .filter(|cell| matches!(cell, Cell::Forbidden | Cell::Crossed(_)))
        .count();

    let _ = write!(
        out,
        "<header class=\"masthead\">\n\
         <div class=\"stamp\">{}</div>\n\
         <h1>{}</h1>\n\
         <div class=\"tallies\">\n\
         <div class=\"tally\"><span class=\"n\">{}</span><span class=\"k\">{}</span></div>\n\
         <div class=\"tally\"><span class=\"n\">{}</span><span class=\"k\">{}</span></div>\n\
         <div class=\"tally{}\"><span class=\"n\">{}</span><span class=\"k\">{}</span></div>\n\
         <div class=\"tally\"><span class=\"n\">{}</span><span class=\"k\">{}</span></div>\n\
         <div class=\"tally{}\"><span class=\"n\">{accepted}</span><span class=\"k\">{}</span></div>\n\
         <div class=\"tally{}\"><span class=\"n\">{}</span><span class=\"k\">{}</span></div>\n\
         </div>\n</header>\n",
        escape(say.report_stamp()),
        escape(&say.report_heading(matrix.modules.len(), walls, crossed)),
        report.files_scanned,
        escape(say.tally_files()),
        config.rule_count(),
        escape(say.tally_rules()),
        if report.error_count() > 0 {
            " is-crossed"
        } else {
            ""
        },
        shown.iter().filter(|f| f.level.fails_build()).count(),
        escape(say.tally_errors()),
        shown.iter().filter(|f| !f.level.fails_build()).count(),
        escape(say.tally_warnings()),
        if accepted > 0 { " is-accepted" } else { "" },
        escape(say.tally_accepted()),
        if report.checks_skipped > 0 {
            " is-accepted"
        } else {
            ""
        },
        report.checks_skipped,
        escape(say.tally_undecided()),
    );

    html_map(&matrix, say, &mut out);
    html_matrix(&matrix, say, &mut out);
    html_pressure(&matrix, say, &mut out);
    html_blindspots(report, accepted, say, &mut out);

    let _ = write!(
        out,
        "<footer><span>archwarden {}</span>\n\
         <span>{}</span>\n\
         <span>{} · {} <code>archwarden check --html</code></span>\n\
         </footer>\n",
        escape(env!("CARGO_PKG_VERSION")),
        escape(&say.scanned(report.files_scanned, report.directories_scanned)),
        escape(say.read_only()),
        escape(say.regenerate_with()),
    );

    close(&mut out);
    String::from_utf8(out).unwrap_or_default()
}

/// The modules, with what the config says they are for.
fn html_map(matrix: &crate::matrix::Matrix, say: &dyn crate::phrases::Phrases, out: &mut Vec<u8>) {
    use std::io::Write as _;

    use crate::html::{code, escape, section};

    let _ = writeln!(
        out,
        "{}<div class=\"modules\">",
        section(say.map_eyebrow(), say.map_heading(), say.map_lede())
    );

    for module in &matrix.modules {
        let counts = if module.errors == 0 && module.warnings == 0 {
            say.clean().to_owned()
        } else {
            let mut parts = Vec::new();
            if module.errors > 0 {
                parts.push(format!(
                    "<span class=\"hot\">{}</span>",
                    escape(&say.errors(module.errors))
                ));
            }
            if module.warnings > 0 {
                parts.push(escape(&say.warnings(module.warnings)));
            }
            parts.join(" · ")
        };

        let _ = write!(
            out,
            "<div class=\"module\">\n<span class=\"name\">{}</span>\n\
             <span class=\"counts\">{} · {counts}</span>\n\
             <span class=\"scope\">{}</span>\n",
            escape(&module.id),
            escape(&say.files(module.files)),
            module
                .scopes
                .iter()
                .map(|glob| code(glob))
                .collect::<Vec<_>>()
                .join(" "),
        );
        match &module.why {
            Some(why) => {
                let _ = writeln!(out, "<p class=\"why\">{}</p>", escape(why));
            }
            None => {
                let _ = writeln!(
                    out,
                    "<p class=\"why is-absent\">{}</p>",
                    escape(say.no_reason_recorded())
                );
            }
        }
        let _ = writeln!(out, "</div>\n");
    }

    let _ = writeln!(out, "</div>\n</section>");
}

/// The grid. Rows are numbered and columns carry only the number, which is what
/// keeps it readable past ten modules -- a name in every column header costs
/// about a hundred pixels each and a repository with twenty of them would
/// scroll sideways before the first cell.
fn html_matrix(
    matrix: &crate::matrix::Matrix,
    say: &dyn crate::phrases::Phrases,
    out: &mut Vec<u8>,
) {
    use std::io::Write as _;

    use crate::html::{escape, section};
    use crate::matrix::Cell;

    let _ = write!(
        out,
        "{}<div class=\"plate\">\n<table class=\"matrix\">\n<thead>\n<tr>\n\
         <th class=\"corner\"></th>",
        section(say.walls_eyebrow(), say.walls_heading(), say.walls_lede())
    );

    for (index, _) in matrix.modules.iter().enumerate() {
        let _ = writeln!(out, "<th scope=\"col\">{}</th>\n", index + 1);
    }
    let _ = writeln!(out, "</tr>\n</thead>\n<tbody>");

    for (index, (module, row)) in matrix.modules.iter().zip(&matrix.rows).enumerate() {
        let _ = write!(
            out,
            "<tr>\n<th scope=\"row\"><span class=\"n\">{}</span> {}</th>",
            index + 1,
            escape(&module.id)
        );
        for cell in row {
            let _ = match cell {
                Cell::Self_ => writeln!(out, "<td><span class=\"cell self\">—</span></td>"),
                Cell::Allowed => writeln!(out, "<td><span class=\"cell\"></span></td>"),
                Cell::Forbidden => {
                    writeln!(out, "<td><span class=\"cell forbidden\"></span></td>")
                }
                Cell::Crossed(n) => {
                    writeln!(out, "<td><span class=\"cell crossed\">{n}</span></td>")
                }
            };
        }
        let _ = writeln!(out, "</tr>\n");
    }

    let _ = write!(
        out,
        "</tbody>\n</table>\n</div>\n\
         <div class=\"legend\">\n\
         <span><i class=\"swatch\"></i> {}</span>\n\
         <span><i class=\"swatch forbidden\"></i> {}</span>\n\
         <span><i class=\"swatch crossed\"></i> {}</span>\n\
         </div>\n</section>",
        escape(say.legend_allowed()),
        escape(say.legend_forbidden()),
        escape(say.legend_crossed()),
    );
}

/// The walls, worst first, each with what is going through it.
fn html_pressure(
    matrix: &crate::matrix::Matrix,
    say: &dyn crate::phrases::Phrases,
    out: &mut Vec<u8>,
) {
    use std::io::Write as _;

    use crate::html::{code, escape, section};

    if matrix.walls.is_empty() {
        return;
    }

    let _ = writeln!(
        out,
        "{}<div class=\"walls\">",
        section(
            say.pressure_eyebrow(),
            say.pressure_heading(),
            say.pressure_lede()
        )
    );

    for wall in &matrix.walls {
        let crossings = wall.crossings.len();
        let _ = write!(
            out,
            "<article class=\"wall\">\n<header>\n\
             <span class=\"edge\">{} ↛ {}</span>\n\
             <span class=\"rule-id\">{}</span>\n<span class=\"pills\">",
            escape(&wall.from),
            escape(&wall.to),
            escape(&wall.rule_id),
        );
        let _ = if crossings == 0 {
            write!(
                out,
                "<span class=\"pill quiet\">{}</span>",
                escape(say.holding())
            )
        } else {
            write!(
                out,
                "<span class=\"pill now\">{}</span>",
                escape(&say.crossing_now(crossings))
            )
        };
        let _ = writeln!(out, "</span>\n</header>");

        if let Some(why) = &wall.why {
            let _ = writeln!(out, "<p class=\"why\">{}</p>\n", escape(why));
        }

        if crossings == 0 {
            let _ = write!(
                out,
                "<ul class=\"crossings\"><li>{}</li></ul>\n</article>",
                escape(say.nothing_crosses())
            );
            continue;
        }

        // Folded past five, and folded by `<details>` rather than truncated:
        // the count stays on the summary line, so nothing is hidden and the
        // page does not become a wall of text. No script, and it prints open if
        // the reader opens it.
        let folded = crossings > 5;
        if folded {
            let _ = writeln!(
                out,
                "<details><summary>{}</summary>",
                escape(&say.imports(crossings))
            );
        }
        let _ = writeln!(out, "<ul class=\"crossings\">");
        for (importer, specifier) in &wall.crossings {
            let _ = writeln!(
                out,
                "<li><span class=\"file\">{}</span> → {}</li>",
                escape(importer.as_str()),
                code(specifier)
            );
        }
        let _ = writeln!(out, "</ul>");
        if folded {
            let _ = writeln!(out, "</details>");
        }
        let _ = writeln!(out, "</article>");
    }

    let _ = writeln!(out, "</div>\n</section>");
}

/// What the run could not decide.
///
/// Bordered and coloured rather than tucked into a footer, on purpose. A page
/// that hid these would be worse than the JSON: it would look more trustworthy
/// while knowing less.
fn html_blindspots(
    report: &Report,
    accepted: usize,
    say: &dyn crate::phrases::Phrases,
    out: &mut Vec<u8>,
) {
    use std::io::Write as _;

    use crate::html::{code, escape, prose};

    let mut notes: Vec<String> = Vec::new();

    for (_, reason) in &report.unreadable_files {
        // The reason is the parser's own sentence and already names the file
        // in backticks, so the path is not repeated -- `prose` turns those into
        // elements, and printing both left the same path twice, once as an
        // element and once as punctuation.
        notes.push(format!(
            "<strong>{}</strong> {}",
            escape(say.not_read()),
            prose(reason)
        ));
    }
    if report.checks_skipped > 0 {
        notes.push(format!(
            "<strong>{}</strong> {}",
            escape(&say.checks_nobody_could_make(report.checks_skipped)),
            report
                .skipped_checks
                .iter()
                .map(|(rule, path)| format!("{} on {}", code(rule), code(path.as_str())))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if report.imports.unresolved > 0 {
        notes.push(format!(
            "<strong>{}</strong>",
            escape(&say.unresolved_imports(report.imports.unresolved)),
        ));
    }
    if accepted > 0 {
        notes.push(format!(
            "<strong>{}</strong>",
            escape(&say.accepted_in_baseline(accepted)),
        ));
    }

    if notes.is_empty() {
        return;
    }

    let _ = write!(
        out,
        "<section>\n<div class=\"blindspots\">\n\
         <h2>{}</h2>\n\
         <p class=\"lede\">{}</p>\n<ul>\n",
        escape(say.blindspots_heading()),
        escape(say.blindspots_lede()),
    );
    for note in notes {
        let _ = writeln!(out, "<li>{note}</li>");
    }
    let _ = writeln!(out, "</ul>\n</div>\n</section>");
}

/// Skips on files the unreadable-file notes above do not account for.
///
/// The other half of the same number. A skip on a file that *is* named above is
/// a bug to investigate; a skip on one that is not was never attempted -- a
/// language this configuration did not ask archwarden to read, most often.
/// `1 skipped` could not tell those apart, and they are opposite decisions.
/// Issue #13.
fn render_unattempted_skips(report: &Report, out: &mut dyn std::io::Write) {
    let mut by_path: std::collections::BTreeMap<&RepoRelPath, Vec<&str>> =
        std::collections::BTreeMap::new();

    for (rule, path) in &report.skipped_checks {
        let named_above = report
            .unreadable_files
            .iter()
            .any(|(unreadable, _)| unreadable == path);
        if !named_above {
            by_path.entry(path).or_default().push(rule.as_str());
        }
    }

    for (path, rules) in by_path {
        let _ = writeln!(
            out,
            "note: `{path}` was not read, so {} {} skipped there: {}",
            rules.len(),
            plural(rules.len(), "check was", "checks were"),
            rules.join(", "),
        );
    }
}

/// What a document's frontmatter must carry.
///
/// Three clauses, in the order the rule reads them: the keys, then the closed
/// vocabularies, then the agreements with the path. Each is skipped when the
/// rule did not ask for it, so a rule that only names keys gets one clause.
fn describe_frontmatter(
    keys: &[String],
    vocabularies: &[(String, Vec<String>)],
    agreements: &[(String, String)],
) -> String {
    let mut parts = Vec::new();

    if !keys.is_empty() {
        let quoted: Vec<&str> = keys.iter().map(String::as_str).collect();
        parts.push(format!("frontmatter carrying {}", join_and(&quoted)));
    }
    for (key, accepted) in vocabularies {
        let quoted: Vec<&str> = accepted.iter().map(String::as_str).collect();
        parts.push(format!(
            "with `{key}` one of {}",
            join_or(&quoted, "nothing")
        ));
    }
    for (key, wanted) in agreements {
        parts.push(format!("and `{key}` equal to `{wanted}`"));
    }

    parts.join(", ")
}

/// `a`, `b` and `c` — for a list where every entry is required.
fn join_and(items: &[&str]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| format!("`{item}`")).collect();
    match quoted.split_last() {
        None => "nothing".to_owned(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// What must live inside a directory, by name and by shape.
///
/// "and", never "or": every entry is required, and the `join_or` this module
/// uses everywhere else would say the opposite of the rule.
fn describe_required_files(names: &[String], patterns: &[String]) -> String {
    let mut parts = Vec::new();

    if !names.is_empty() {
        let quoted: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
        parts.push(match quoted.split_last() {
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
            None => String::new(),
        });
    }

    for pattern in patterns {
        let clause = format!("a file matching `{pattern}`");
        parts.push(if parts.is_empty() {
            clause
        } else {
            format!("and {clause}")
        });
    }

    parts.join(", ")
}

/// What may live inside a directory, by name and by shape.
///
/// Split out of [`describe_expectation`] because it is the one arm with three
/// clauses to compose, and because "one of no folders" -- the sentence a rule
/// constraining by shape alone used to get -- describes the opposite of that
/// rule. The enumeration clause appears only when there is an enumeration.
fn describe_subfolders(allowed: &[String], warn: &[String], patterns: &[String]) -> String {
    let mut parts = Vec::new();

    if !allowed.is_empty() || patterns.is_empty() {
        parts.push(format!("one of {}", join_or(allowed, "no folders")));
    }
    if !warn.is_empty() {
        parts.push(format!("or {} as a warning", join_or(warn, "")));
    }
    if !patterns.is_empty() {
        // "a folder name", not "a name". The same sentence served
        // `filename_patterns` and `subfolder_patterns`, which are siblings
        // constraining different kinds of entry, so a reader could not tell
        // which was meant. `check` had always distinguished them — "folder
        // `x` is not allowed here" — and `describe` had not.
        let clause = format!("a folder name matching {}", join_or(patterns, ""));
        parts.push(if parts.is_empty() {
            clause
        } else {
            format!("or {clause}")
        });
    }

    parts.join(", ")
}

/// What a directory's own name may be, said to that directory.
///
/// The same lists as [`describe_subfolders`], in the second person: that one
/// tells a parent what may appear inside it, this one tells a child what it
/// may be called.
fn describe_folder_name(allowed: &[String], warn: &[String], patterns: &[String]) -> String {
    let mut parts = Vec::new();

    if !allowed.is_empty() {
        parts.push(format!("named one of {}", join_or(allowed, "")));
    }
    if !warn.is_empty() {
        parts.push(format!("or {} as a warning", join_or(warn, "")));
    }
    if !patterns.is_empty() {
        let clause = format!("a folder name matching {}", join_or(patterns, ""));
        parts.push(if parts.is_empty() {
            clause
        } else {
            format!("or {clause}")
        });
    }

    if parts.is_empty() {
        // A parent that permits no subfolder at all. Said plainly, because the
        // alternative reads as a rule with nothing to say.
        return "no folder here at all: its parent allows none".to_owned();
    }

    parts.join(", ")
}

#[allow(
    clippy::too_many_lines,
    reason = "the same table as `describe_observed`, from the other side: one \
              arm per expectation, and the two have to read as one sentence \
              when a finding puts them together"
)]
pub(crate) fn describe_expectation(expectation: &Expectation) -> String {
    match expectation {
        Expectation::AllowedSubfolders {
            allowed,
            warn,
            patterns,
        } => describe_subfolders(allowed, warn, patterns),
        Expectation::RequiredFiles { names, patterns } => describe_required_files(names, patterns),
        Expectation::RequiredCompanion { path } => format!("`{path}` beside it"),
        Expectation::RequiredFrontmatter {
            keys,
            vocabularies,
            agreements,
        } => describe_frontmatter(keys, vocabularies, agreements),
        Expectation::FilenamePattern { patterns } => {
            format!("a file name matching {}", join_or(patterns, "no pattern"))
        }
        Expectation::FolderName {
            allowed,
            warn,
            patterns,
        } => describe_folder_name(allowed, warn, patterns),
        Expectation::RequiredExport {
            name,
            annotation,
            signature_hint,
            ..
        } => {
            let mut sentence = format!("an export named `{name}`");
            // The checked clause first, and the suggestion after it. A reader
            // acting on one of the two should reach the enforced one first.
            if !annotation.is_empty() {
                let accepted: Vec<&str> = annotation.iter().map(String::as_str).collect();
                let _ = write!(
                    sentence,
                    ", annotated {}",
                    join_or(&accepted, "no type at all")
                );
            }
            if let Some(hint) = signature_hint {
                let _ = write!(sentence, ", shaped like `{hint}`");
            }
            sentence
        }
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
        Expectation::PermittedImports { patterns, .. } => format!(
            "imports only from {} (its own files always, packages separately)",
            join_or(patterns, "nowhere")
        ),
        Expectation::PermittedPackages { packages } => {
            format!("imports only these packages: {}", join_or(packages, "none"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        facts::{ExportKind, ExportTags, KindFilter},
        finding::Observed,
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
        }
    }

    fn outcomes(in_repo: usize, external: usize, builtin: usize, unresolved: usize) -> Report {
        Report {
            imports: archwarden_engine::resolve::Outcomes {
                in_repo,
                external,
                builtin,
                unresolved,
                unresolved_imports: Vec::new(),
            },
            ..report(Vec::new())
        }
    }

    /// A run whose only news is that these imports were never placed, sorted
    /// as the engine hands them over.
    fn blind_spots(unresolved: &[(&str, &str)]) -> Report {
        Report {
            imports: archwarden_engine::resolve::Outcomes {
                unresolved: unresolved.len(),
                unresolved_imports: unresolved
                    .iter()
                    .map(|(file, specifier)| (path(file), (*specifier).to_owned()))
                    .collect(),
                ..archwarden_engine::resolve::Outcomes::default()
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

    /// Rendered with the standing reasons a configuration carries.
    fn rendered_with(report: &Report, reasons: &Reasons, format: Format) -> String {
        let mut out = Vec::new();
        render(
            &Rendered {
                root: Utf8Path::new("."),
                report,
                view: &View::everything(report),
                reasons,
                elapsed: TOOK,
            },
            format,
            &mut out,
        );
        String::from_utf8(out).expect("output is UTF-8")
    }

    /// Rendered against a real tree, for the cases that need one on disk.
    fn rendered_at(root: &Utf8Path, report: &Report, format: Format) -> String {
        let mut out = Vec::new();
        render(
            &Rendered {
                root,
                report,
                view: &View::everything(report),
                reasons: &Reasons::default(),
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
                reasons: &Reasons::default(),
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

    /// Issue #13's reporting half. `1 skipped` is indistinguishable from a
    /// skip on an unreadable file and one on a rule nobody could evaluate, and
    /// the two mean opposite things: one is a bug to investigate, the other is
    /// a decision the project has not made — a language the config never asked
    /// archwarden to read.
    ///
    /// The unreadable case already names its file. This is the other one, which
    /// named nothing.
    #[test]
    fn a_skip_with_no_unreadable_file_still_names_what_and_where() {
        let mut report = report(Vec::new());
        report.checks_skipped = 1;
        report.skipped_checks = vec![(
            "pages-forbid-domain".to_owned(),
            path("src/pages/blog.astro"),
        )];

        let text = rendered(&report, Format::Text);

        assert!(text.contains("src/pages/blog.astro"), "{text}");
        assert!(text.contains("pages-forbid-domain"), "{text}");
    }

    /// And a skip the unreadable-file note already accounts for is not printed
    /// twice.
    #[test]
    fn a_skip_under_an_unreadable_file_is_named_once() {
        let mut report = report(Vec::new());
        report.unreadable_files = vec![(path("src/broken.ts"), "unexpected token".to_owned())];
        report.checks_skipped = 1;
        report.skipped_checks = vec![("usecase-name".to_owned(), path("src/broken.ts"))];

        let text = rendered(&report, Format::Text);

        assert_eq!(text.matches("src/broken.ts").count(), 1, "{text}");
    }

    /// What the rule wants, for someone who has not read the config.
    #[test]
    fn a_required_frontmatter_reads_as_a_sentence() {
        let expected = describe_expectation(&Expectation::RequiredFrontmatter {
            keys: vec!["id".to_owned(), "nivel".to_owned()],
            vocabularies: vec![("nivel".to_owned(), vec!["1".to_owned(), "2".to_owned()])],
            agreements: vec![("id".to_owned(), "03-semaforo".to_owned())],
        });

        assert_eq!(
            expected,
            "frontmatter carrying `id` and `nivel`, \
             with `nivel` one of `1` or `2`, and `id` equal to `03-semaforo`"
        );
    }

    /// Issue #46. A finding says what the rule wanted and what the file did,
    /// and never why the rule exists. An agent reading one can comply and that
    /// is all it can do -- which is how a config gets edited to make a check
    /// pass.
    ///
    /// Once per rule, at its first occurrence. A repository with two hundred
    /// findings over six rules must not print two hundred paragraphs; six, in
    /// the place a reader is already looking, is the whole design constraint.
    #[test]
    fn a_rules_reason_is_printed_once_at_its_first_finding() {
        let report = report(vec![
            finding(Level::Error, None),
            finding(Level::Error, None),
        ]);
        let reasons = Reasons::from([(
            "domain-entity-shape",
            "domain is published as its own package and the app is not",
        )]);

        let text = rendered_with(&report, &reasons, Format::Text);

        assert_eq!(
            text.matches("why: domain is published").count(),
            1,
            "one paragraph per rule, not per finding: {text}"
        );
    }

    /// A rule whose author said nothing prints nothing extra, which is every
    /// rule in every config written before the field existed.
    #[test]
    fn a_rule_with_no_reason_adds_no_line() {
        let report = report(vec![finding(Level::Error, None)]);

        let text = rendered_with(&report, &Reasons::default(), Format::Text);

        assert!(!text.contains("why:"), "{text}");
    }

    /// What a folder may be *called*, said to that folder.
    ///
    /// The mirror of `describe_subfolders`, and its three shapes are three
    /// sentences. Mutation testing found each branch untested: the pattern
    /// path was exercised through the CLI and the literal lists were not.
    #[test]
    fn a_folder_name_reads_in_the_second_person() {
        let by_list = describe_expectation(&Expectation::FolderName {
            allowed: vec!["sketch".to_owned(), "minha-solucao".to_owned()],
            warn: Vec::new(),
            patterns: Vec::new(),
        });
        assert_eq!(by_list, "named one of `sketch` or `minha-solucao`");

        let with_warning = describe_expectation(&Expectation::FolderName {
            allowed: vec!["sketch".to_owned()],
            warn: vec!["rascunho".to_owned()],
            patterns: Vec::new(),
        });
        assert_eq!(
            with_warning,
            "named one of `sketch`, or `rascunho` as a warning"
        );

        let by_shape = describe_expectation(&Expectation::FolderName {
            allowed: Vec::new(),
            warn: Vec::new(),
            patterns: vec![r"^\d{2}-[a-z0-9-]+$".to_owned()],
        });
        assert_eq!(by_shape, r"a folder name matching `^\d{2}-[a-z0-9-]+$`");
    }

    /// A parent permitting no subfolder at all: the folder is not misnamed,
    /// it should not be there. Said plainly, because an empty list of
    /// permitted names otherwise reads as a rule with nothing to say.
    #[test]
    fn a_folder_whose_parent_permits_none_is_told_that() {
        let sentence = describe_expectation(&Expectation::FolderName {
            allowed: Vec::new(),
            warn: Vec::new(),
            patterns: Vec::new(),
        });

        assert_eq!(sentence, "no folder here at all: its parent allows none");
    }

    /// Issue #43: a rule whose subfolder names are a *shape* rather than a
    /// list. "one of no folders" was the old sentence for that, which describes
    /// the opposite of the rule.
    #[test]
    fn a_subfolder_pattern_reads_as_a_shape_not_an_empty_list() {
        let by_shape = describe_expectation(&Expectation::AllowedSubfolders {
            allowed: Vec::new(),
            warn: Vec::new(),
            patterns: vec![r"^\d{2}-[a-z0-9-]+$".to_owned()],
        });
        // "a folder name", not "a name": `filename_patterns` and
        // `subfolder_patterns` are siblings over different kinds of entry, and
        // one sentence for both left a reader unable to tell which. Issue #53.
        assert_eq!(by_shape, r"a folder name matching `^\d{2}-[a-z0-9-]+$`");

        let both = describe_expectation(&Expectation::AllowedSubfolders {
            allowed: vec!["_template".to_owned()],
            warn: Vec::new(),
            patterns: vec![r"^\d{2}-[a-z0-9-]+$".to_owned()],
        });
        assert_eq!(
            both,
            r"one of `_template`, or a folder name matching `^\d{2}-[a-z0-9-]+$`"
        );
    }

    /// What the rule wants, worded for someone who has not read the config.
    #[test]
    fn a_required_annotation_reads_as_a_sentence() {
        let expected = describe_expectation(&Expectation::RequiredExport {
            kind: KindFilter::OneOf(ExportTags::only(ExportKind::Const)),
            name: "AGENT_TOOL".to_owned(),
            annotation: vec!["AgentToolModule".to_owned()],
            signature_hint: None,
        });

        assert_eq!(
            expected,
            "an export named `AGENT_TOOL`, annotated `AgentToolModule`"
        );
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

        let packages = describe_expectation(&Expectation::ForbiddenPackages {
            packages: vec!["three".to_owned()],
            except_from: vec!["src/scripts/three/**".to_owned()],
            include_type_only: true,
        });
        assert!(packages.contains("three"), "{packages}");
        assert!(
            packages.contains("except from"),
            "the exemption is on the importing side, and the sentence says so: \
             {packages}"
        );

        let no_exemption = describe_expectation(&Expectation::ForbiddenPackages {
            packages: vec!["three".to_owned()],
            except_from: Vec::new(),
            include_type_only: true,
        });
        assert_eq!(no_exemption, "no import of `three`");
    }

    /// A warn-listed folder is part of the expectation, so a reader can see
    /// why one folder is an error and another a warning.
    #[test]
    fn a_warn_list_appears_in_the_expectation() {
        let sentence = describe_expectation(&Expectation::AllowedSubfolders {
            allowed: vec!["types".to_owned()],
            warn: vec!["shared".to_owned()],
            patterns: Vec::new(),
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

    /// And the count alone is not enough. It says an import is unprotected and
    /// leaves the reader nowhere to look; issue #18 found its own by deleting
    /// imports until the number moved.
    #[test]
    fn the_text_output_names_each_import_that_did_not_resolve() {
        let text = rendered(
            &blind_spots(&[
                ("packages/domain/row.ts", "@Domain/Order/id"),
                ("packages/domain/row.ts", "@Domain/Order/types"),
                ("packages/domain/seed.ts", "@Shared/clock"),
            ]),
            Format::Text,
        );

        assert!(text.contains("3 imports could not resolve"), "{text}");
        assert!(
            text.contains(
                "      `packages/domain/row.ts`: `@Domain/Order/id`, `@Domain/Order/types`\n"
            ),
            "one line per file, however many it wrote: {text}"
        );
        assert!(
            text.contains("      `packages/domain/seed.ts`: `@Shared/clock`\n"),
            "{text}"
        );
    }

    /// A repository whose dependencies are not installed cannot place a single
    /// bare specifier, and a line per file would push the findings the user
    /// came for off the screen. Cut, and said to be cut -- a reader who cannot
    /// tell whether the list ended or was truncated has to go and check.
    #[test]
    fn a_wall_of_unresolved_imports_is_cut_and_says_so() {
        let files: Vec<(String, &str)> = (0..14)
            .map(|n| (format!("src/file-{n:02}.ts"), "react"))
            .collect();
        let text = rendered(
            &blind_spots(
                &files
                    .iter()
                    .map(|(file, specifier)| (file.as_str(), *specifier))
                    .collect::<Vec<_>>(),
            ),
            Format::Text,
        );

        assert!(text.contains("`src/file-09.ts`: `react`"), "{text}");
        assert!(
            !text.contains("`src/file-10.ts`"),
            "the eleventh file is past the cut: {text}"
        );
        assert!(
            text.contains("      … and 4 more files, all of them under `--format json`\n"),
            "{text}"
        );
    }

    /// Exactly at the cut there is nothing left out, and saying "and 0 more"
    /// would send a reader looking for a list that is already complete.
    #[test]
    fn a_list_that_fits_is_not_announced_as_cut() {
        let files: Vec<String> = (0..UNRESOLVED_FILES_SHOWN)
            .map(|n| format!("src/file-{n:02}.ts"))
            .collect();
        let text = rendered(
            &blind_spots(
                &files
                    .iter()
                    .map(|file| (file.as_str(), "react"))
                    .collect::<Vec<_>>(),
            ),
            Format::Text,
        );

        assert!(text.contains("`src/file-09.ts`: `react`"), "{text}");
        assert!(!text.contains("more files"), "{text}");
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

    /// Lists read as prose rather than as a debug dump, at every length.
    #[test]
    fn lists_are_joined_as_english() {
        assert_eq!(join_or(&["a"], "none"), "`a`");
        assert_eq!(join_or(&["a", "b"], "none"), "`a` or `b`");
        assert_eq!(join_or(&["a", "b", "c"], "none"), "`a`, `b` or `c`");
        assert_eq!(join_or(&Vec::<String>::new(), "none"), "none");
    }
    /// Issue #45. The finding is on the file that needs the companion, so the
    /// sentence has to name the companion rather than repeat the file.
    #[test]
    fn a_missing_companion_reads_as_a_sentence() {
        assert_eq!(
            describe_observed(&Observed::CompanionMissing {
                path: path("projetos/03-semaforo/notas.md")
            }),
            "`projetos/03-semaforo/notas.md` does not exist"
        );
        assert_eq!(
            describe_expectation(&Expectation::RequiredCompanion {
                path: path("projetos/03-semaforo/notas.md")
            }),
            "`projetos/03-semaforo/notas.md` beside it"
        );
    }
    /// Issue #42. The first observation about a path that is *not* there, so
    /// the sentence has to read as an absence rather than as a disagreement
    /// with something on disk.
    #[test]
    fn a_missing_required_file_reads_as_a_sentence() {
        assert_eq!(
            describe_observed(&Observed::RequiredFileMissing {
                name: "notas.md".to_owned()
            }),
            "`notas.md` is not here"
        );
        assert_eq!(
            describe_observed(&Observed::NoFileMatching {
                pattern: r"\.ino$".to_owned()
            }),
            r"no file here matches `\.ino$`"
        );
        assert_eq!(
            describe_expectation(&Expectation::RequiredFiles {
                names: vec!["projeto.md".to_owned(), "notas.md".to_owned()],
                patterns: vec![r"\.ino$".to_owned()],
            }),
            r"`projeto.md` and `notas.md`, and a file matching `\.ino$`"
        );
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

        assert_eq!(breakdown.rows().collect::<Vec<_>>(), [("mixed", 1, 2)]);
        let text = rendered_view(
            &report,
            &View::summarised(&report.findings.iter().collect::<Vec<_>>(), breakdown, 0),
            Format::Text,
            TOOK,
        );
        assert!(text.starts_with("mixed  1 error, 2 warnings\n"), "{text}");
    }

    /// A scope selecting each module directory, as a real config does.
    fn module_scope() -> archwarden_core::scope::Scope {
        archwarden_core::scope::Scope::compile(["packages/domain/src/*"]).expect("valid scope")
    }

    /// A breakdown over four rules, of which two fired.
    ///
    /// The counting is `archwarden_api::present`'s and is tested there. What
    /// these need it for is a table with something in it to render.
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
    /// Every one of them in the JSON, where the text shows the first few: a CI
    /// job gating on "no import escapes the boundary rules" reads the whole
    /// list, and nothing is scrolling past it.
    #[test]
    fn the_json_carries_every_import_that_did_not_resolve() {
        let json = rendered(
            &blind_spots(&[
                ("packages/domain/row.ts", "@Domain/Order/id"),
                ("packages/domain/seed.ts", "@Shared/clock"),
            ]),
            Format::Json,
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        let named = &parsed["summary"]["imports"]["unresolved_imports"];
        assert_eq!(named[0]["path"], "packages/domain/row.ts");
        assert_eq!(named[0]["specifier"], "@Domain/Order/id");
        assert_eq!(named[1]["path"], "packages/domain/seed.ts");
        assert_eq!(named[1]["specifier"], "@Shared/clock");
        assert_eq!(
            named.as_array().map(Vec::len),
            Some(2),
            "as many as the count: {json}"
        );
    }
}
