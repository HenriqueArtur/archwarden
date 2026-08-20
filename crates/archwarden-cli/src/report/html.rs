//! The HTML pages, drawn on the shell `crate::html` provides.

use archwarden_core::finding::Finding;
use archwarden_engine::run::Report;

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
pub(super) fn html_map(
    matrix: &crate::matrix::Matrix,
    say: &dyn crate::phrases::Phrases,
    out: &mut Vec<u8>,
) {
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
pub(super) fn html_matrix(
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
pub(super) fn html_pressure(
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
pub(super) fn html_blindspots(
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
