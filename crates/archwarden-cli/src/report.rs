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
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    unimplemented_rules: &'a [String],
}

#[derive(Debug, Serialize)]
struct Summary {
    errors: usize,
    warnings: usize,
    files_scanned: usize,
    directories_scanned: usize,
}

impl Summary {
    fn of(report: &Report) -> Self {
        Self {
            errors: report.error_count(),
            warnings: report.warning_count(),
            files_scanned: report.files_scanned,
            directories_scanned: report.directories_scanned,
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
        unimplemented_rules: &report.unimplemented_rules,
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

fn render_text(report: &Report, out: &mut dyn std::io::Write) {
    for finding in &report.findings {
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

    for rule in &report.unimplemented_rules {
        let _ = writeln!(
            out,
            "note: rule `{rule}` was not checked — its kind is not implemented yet"
        );
    }

    let summary = Summary::of(report);
    let _ = writeln!(
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
fn describe_expectation(expectation: &Expectation) -> String {
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
            unimplemented_rules: Vec::new(),
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
            findings: Vec::new(),
            directories_scanned: 1,
            files_scanned: 1,
            unimplemented_rules: Vec::new(),
        };
        assert!(
            rendered(&singular, Format::Text).ends_with("1 file, 1 directory\n"),
            "{}",
            rendered(&singular, Format::Text)
        );
    }

    /// A rule that was not checked is said out loud. A clean-looking report
    /// that quietly skipped a rule is worse than no report.
    #[test]
    fn an_unchecked_rule_is_announced_in_both_formats() {
        let report = Report {
            findings: Vec::new(),
            directories_scanned: 1,
            files_scanned: 1,
            unimplemented_rules: vec!["future-rule".to_owned()],
        };

        let text = rendered(&report, Format::Text);
        assert!(text.contains("future-rule"), "{text}");
        assert!(text.contains("not implemented yet"), "{text}");

        let json = rendered(&report, Format::Json);
        assert!(json.contains("\"unimplemented_rules\""), "{json}");
        assert!(json.contains("future-rule"), "{json}");
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

        let first = &parsed["findings"][0];
        assert_eq!(first["rule_id"], "domain-entity-shape");
        assert_eq!(first["module_id"], "domain");
        assert_eq!(first["level"], "error");
        assert_eq!(first["path"], "packages/domain/src/user/wrong-folder");
        assert_eq!(first["observed"]["type"], "unexpected-subfolder");
        assert_eq!(first["expected"]["type"], "allowed-subfolders");
    }

    /// An empty `unimplemented_rules` is absent rather than an empty array, so
    /// the common report stays small.
    #[test]
    fn a_clean_json_report_omits_the_empty_list() {
        let json = rendered(&report(Vec::new()), Format::Json);
        assert!(!json.contains("unimplemented_rules"), "{json}");
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

    /// Lists read as prose rather than as a debug dump, at every length.
    #[test]
    fn lists_are_joined_as_english() {
        assert_eq!(join_or(&["a"], "none"), "`a`");
        assert_eq!(join_or(&["a", "b"], "none"), "`a` or `b`");
        assert_eq!(join_or(&["a", "b", "c"], "none"), "`a`, `b` or `c`");
        assert_eq!(join_or(&Vec::<String>::new(), "none"), "none");
    }
}
