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
};
use archwarden_engine::run::Report;
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

/// The JSON envelope.
#[derive(Debug, Serialize)]
struct JsonReport<'a> {
    version: u32,
    summary: Summary,
    findings: &'a [Finding],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unreadable_files: Vec<UnreadableFile<'a>>,
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
    /// Absent when no rule needed resolution, so a consumer can tell "no
    /// boundary rule ran" from "every import resolved".
    #[serde(skip_serializing_if = "Option::is_none")]
    imports: Option<Imports>,
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
    fn of(report: &Report) -> Self {
        Self {
            errors: report.error_count(),
            warnings: report.warning_count(),
            files_scanned: report.files_scanned,
            directories_scanned: report.directories_scanned,
            files_parsed: report.files_parsed,
            facts_reused: report.facts_reused,
            imports: (report.imports.total() > 0).then_some(Imports {
                in_repo: report.imports.in_repo,
                external: report.imports.external,
                builtin: report.imports.builtin,
                unresolved: report.imports.unresolved,
            }),
        }
    }
}

/// Writes a report in the requested format.
pub fn render(report: &Report, format: Format, out: &mut dyn std::io::Write) {
    match format {
        Format::Text => render_text(report, out),
        Format::Json => render_json(report, out),
    }
}

fn render_json(report: &Report, out: &mut dyn std::io::Write) {
    let envelope = JsonReport {
        version: REPORT_VERSION,
        summary: Summary::of(report),
        findings: &report.findings,
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
fn render_finding(finding: &Finding, out: &mut dyn std::io::Write) {
    let module = finding
        .module_id
        .as_ref()
        .map_or_else(|| "*".to_owned(), ToString::to_string);

    let _ = writeln!(
        out,
        "{:<7} {}\n        [{}] {} — {}",
        finding.level,
        finding.path,
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

fn render_text(report: &Report, out: &mut dyn std::io::Write) {
    for finding in &report.findings {
        render_finding(finding, out);
    }

    for (path, reason) in &report.unreadable_files {
        let _ = writeln!(out, "note: `{path}` was not checked — {reason}");
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

    let summary = Summary::of(report);
    let _ = write!(
        out,
        "{} {}, {} {} · {} {}, {} {}",
        summary.errors,
        plural(summary.errors, "error", "errors"),
        summary.warnings,
        plural(summary.warnings, "warning", "warnings"),
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

    let _ = writeln!(out);
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
        render_finding(finding, out);
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
fn describe_observed(observed: &Observed) -> String {
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
        Observed::SiblingMissing { path } => format!("`{path}` does not exist"),
        Observed::SpecIsEmpty { path } => format!("`{path}` contains no test cases"),
        Observed::ForbiddenImport {
            specifier,
            resolved,
        } => format!("imports `{specifier}`, which resolves to `{resolved}`"),
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

    fn rendered(report: &Report, format: Format) -> String {
        let mut out = Vec::new();
        render(report, format, &mut out);
        String::from_utf8(out).expect("output is UTF-8")
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
             1 error, 0 warnings · 34 files, 12 directories\n"
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
        assert_eq!(clean, "0 errors, 0 warnings · 34 files, 12 directories\n");

        let mixed = rendered(
            &report(vec![
                finding(Level::Error, None),
                finding(Level::Warning, None),
                finding(Level::Warning, None),
            ]),
            Format::Text,
        );
        assert!(
            mixed.ends_with("1 error, 2 warnings · 34 files, 12 directories\n"),
            "{mixed}"
        );

        let singular = Report {
            directories_scanned: 1,
            files_scanned: 1,
            ..report(Vec::new())
        };
        assert!(
            rendered(&singular, Format::Text).ends_with("1 file, 1 directory\n"),
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
        assert_eq!(text, "0 errors, 0 warnings · 34 files, 12 directories\n");
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
        assert!(cold.ends_with("· 34 parsed, 0 reused\n"), "{cold}");

        let warm = rendered(
            &Report {
                facts_reused: 34,
                ..report(Vec::new())
            },
            Format::Text,
        );
        assert!(warm.ends_with("· 0 parsed, 34 reused\n"), "{warm}");
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
