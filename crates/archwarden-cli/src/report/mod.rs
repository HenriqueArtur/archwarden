//! Rendering a report.
//!
//! Two formats, one source. Text is for a human reading a terminal; JSON is
//! for an agent or another tool, and its shape is a contract -- it carries a
//! version so a consumer can tell when that contract changes.
//!
//! The prose in the text format is generated from the same `Observed` and
//! `Expectation` values the JSON carries, so the two can never describe a
//! finding differently.

use archwarden_core::{finding::Finding, path::RepoRelPath};
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

/// A path as a reader should see it, with the repository root spelled `.`.
///
/// `RepoRelPath` spells the root as the empty string, which is right for a
/// path and reads as a missing value in a report: a finding about the root
/// printed a blank where every other one prints a path. Only the human
/// renderings go through this — the JSON keeps the empty string, because that
/// is the contract and a consumer joining it onto a root would get `./.`.
/// Found shipping `presence.forbid`, whose first use is a lockfile at the
/// repository root. Issue #177.
pub(super) fn display_path(path: &archwarden_core::path::RepoRelPath) -> &str {
    if path.as_str().is_empty() {
        "."
    } else {
        path.as_str()
    }
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
            return display_path(&finding.path).to_owned();
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
            return display_path(&finding.path).to_owned();
        };

        let Some(before) = text.get(..span.start as usize) else {
            return display_path(&finding.path).to_owned();
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

        format!("{}:{line}:{column}", display_path(&finding.path))
    }
}

mod html;
mod prose;
mod text;

pub use html::html_page;
pub(crate) use prose::describe_expectation;
pub use text::render_single;

use text::{render_single_text, render_text};

#[cfg(test)]
mod tests {
    use super::text::{UNRESOLVED_FILES_SHOWN, human_duration};
    use super::*;
    use archwarden_core::finding::Expectation;
    use archwarden_core::{
        facts::{ExportKind, ExportTags, KindFilter},
        finding::Observed,
        ids::{ModuleId, RuleId},
        level::Level,
        path::RepoRelPath,
    };
    use archwarden_engine::run::Report;
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
            suppressed: Vec::new(),
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
                standing: None,
                as_of: archwarden_core::date::Date::EPOCH,
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
                standing: None,
                as_of: archwarden_core::date::Date::EPOCH,
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
                standing: None,
                as_of: archwarden_core::date::Date::EPOCH,
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

    /// A finding about the repository root reads as `.`, not as a blank.
    ///
    /// `RepoRelPath` spells the root as the empty string, which is right for a
    /// path and reads as a missing value in a report — the line said `error`
    /// and then nothing. It has been that way for every directory rule scoped
    /// to `.`; `presence.forbid` is what made it common, because a lockfile
    /// rule lives at the root. Issue #177.
    #[test]
    fn a_finding_about_the_repository_root_reads_as_a_dot() {
        let mut at_root = finding(Level::Error, None);
        at_root.path = path("");
        let text = rendered(&report(vec![at_root]), Format::Text);

        assert!(text.contains("error   .\n"), "{text}");

        // And only the human rendering: the JSON is a contract, and a consumer
        // joining `.` onto a root would build `./.`.
        assert_eq!(display_path(&path("")), ".");
        assert_eq!(display_path(&path("web/src")), "web/src");
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
    fn declared_metadata_reads_as_a_sentence() {
        let expected = describe_expectation(&Expectation::DeclaredMetadata {
            keys: vec!["owner".to_owned(), "stability".to_owned()],
            vocabularies: vec![(
                "stability".to_owned(),
                vec!["stable".to_owned(), "experimental".to_owned()],
            )],
            agreements: vec![("module".to_owned(), "payments".to_owned())],
        });

        assert_eq!(
            expected,
            "a header declaring `owner` and `stability`, \
             with `stability` one of `stable` or `experimental`, \
             and `module` equal to `payments`"
        );
    }

    /// Issue #168. An empty allowlist is how a `chokepoint` says *nobody here
    /// may*, and it used to render as `outside anywhere` -- the opposite of
    /// what the rule means, in the one string `describe`, `scaffold` and the
    /// pre-write hook all say.
    #[test]
    fn an_empty_allowlist_says_nobody_rather_than_outside_anywhere() {
        let nobody = describe_expectation(&Expectation::UsedOnlyIn {
            callee: vec!["Date.now".to_owned(), "console".to_owned()],
            renders: Vec::new(),
            only_in: Vec::new(),
        });

        assert_eq!(nobody, "no use of `Date.now` or `console` here at all");

        // And a rule that does name an allowlist still reads as it did.
        let somewhere = describe_expectation(&Expectation::UsedOnlyIn {
            callee: vec!["process.env".to_owned()],
            renders: Vec::new(),
            only_in: vec!["src/config/**".to_owned()],
        });

        assert_eq!(somewhere, "no use of `process.env` outside `src/config/**`");
    }

    /// A rule that only names keys gets one clause, and the sentence has to
    /// say where they go — a reader who has never met the marker cannot guess
    /// its spelling from `owner`.
    #[test]
    fn declared_metadata_says_where_the_claims_go() {
        let expected = describe_expectation(&Expectation::DeclaredMetadata {
            keys: vec!["owner".to_owned()],
            vocabularies: Vec::new(),
            agreements: Vec::new(),
        });

        assert_eq!(expected, "a header declaring `owner`");
    }

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

    /// Issue #100, and the economy is one step coarser than the reason's.
    ///
    /// A `why` is printed once per *rule*, because it belongs to one. A
    /// decision belongs to many rules by construction, so it is printed once
    /// per *decision* — a decision serving six rules would otherwise print six
    /// identical blocks in one report, which is the thing the once-per-rule
    /// rule was introduced to prevent, arriving one level up.
    #[test]
    fn a_decision_is_printed_once_per_decision_not_once_per_rule() {
        let other = Finding {
            rule_id: RuleId::new("domain-forbids-http").expect("valid"),
            ..finding(Level::Error, None)
        };
        let report = report(vec![
            finding(Level::Error, None),
            finding(Level::Error, None),
            other,
        ]);

        let adr = archwarden_core::compiled::CompiledDecision {
            scope: None,
            why_not_enforceable: None,
            id: archwarden_core::ids::DecisionId::new("ADR-014").expect("valid"),
            title: "The domain does not know about transport".to_owned(),
            why: Some("it is published".to_owned()),
            link: Some("docs/adr/014.md".to_owned()),
            status: archwarden_core::compiled::DecisionStatus::Accepted,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            alternatives: Vec::new(),
            not_yet: None,
        };
        let reasons = Reasons::default().deciding([
            ("domain-entity-shape", adr.clone()),
            ("domain-forbids-http", adr),
        ]);

        let text = rendered_with(&report, &reasons, Format::Text);

        assert_eq!(
            text.matches("decision: ADR-014").count(),
            1,
            "three findings over two rules serving one decision, one block: {text}"
        );
        assert!(text.contains("docs/adr/014.md"), "{text}");
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
            with_options: Vec::new(),
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

    /// The two expectations a graph rule carries.
    ///
    /// Both would otherwise fall through to the `non_exhaustive` arm and reach
    /// a reader as a Rust `Debug` dump, which is the failure that arm exists to
    /// soften rather than a place to leave a shipped variant.
    #[test]
    fn the_graph_rules_expectations_read_as_sentences() {
        let cycle = describe_expectation(&Expectation::NoImportCycle);
        assert_eq!(cycle, "no import cycle through it");

        let reach = describe_expectation(&Expectation::ForbiddenReach {
            patterns: vec!["packages/db/**".to_owned()],
            except: vec!["packages/db/types/**".to_owned()],
            include_type_only: true,
        });
        assert!(
            reach.contains("packages/db/**") && reach.contains("except"),
            "{reach}"
        );
        assert!(
            reach.contains("depend"),
            "the sentence has to say it is about depending rather than \
             importing, or it reads as a duplicate of `ForbiddenImport`: {reach}"
        );
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

    /// A suppressed finding is a line of its own and a number on the
    /// summary, never an absence.
    ///
    /// The constraint issue #72 is built on: a run with forty of them must
    /// not look like a clean run at a glance.
    #[test]
    fn suppressions_are_a_section_and_a_number_on_the_summary_line() {
        let make_report = report;
        let mut report = make_report(Vec::new());
        report.suppressed = vec![archwarden_engine::run::Suppressed {
            finding: finding(Level::Error, None),
            reason: "the vendor SDK ships no types, tracked in ARCH-412".to_owned(),
        }];

        let text = rendered(&report, Format::Text);

        assert!(
            text.contains("1 finding allowed on purpose"),
            "a suppressed finding is not an absent one: {text}"
        );
        assert!(
            text.contains("the vendor SDK ships no types, tracked in ARCH-412"),
            "and the reason is on the line, or this is `eslint-disable` again: \
             {text}"
        );
        assert!(
            text.contains("1 allowed"),
            "on the summary line too, so a repository whose suppressions are \
             growing finds out without running a second command: {text}"
        );

        let clean = rendered(&make_report(Vec::new()), Format::Text);
        assert!(
            !clean.contains("allowed"),
            "and a run with none says nothing about them: {clean}"
        );
    }
}
