//! The JSON report: a stable, versioned object a program can consume.
//!
//! Its shape is a contract. [`super::REPORT_VERSION`] says which one, so a
//! consumer can tell when it changes, and the prose in the text format is
//! generated from the same `Observed` and `Expectation` values this carries —
//! so the two can never describe one finding differently.

use archwarden_core::finding::Finding;
use serde::Serialize;

use super::{REPORT_VERSION, Reasons, Rendered, Renderer, Summary};

/// Writes a run as the versioned JSON object.
///
/// In `archwarden-api` rather than in a surface, because MCP has to emit the
/// same object `check --format json` does. A server reaching into
/// `archwarden-cli` for the report format would be a dependency pointing the
/// wrong way, and two implementations of one contract would be two things that
/// drift.
#[derive(Debug, Clone, Copy, Default)]
pub struct Json;

impl Renderer for Json {
    fn render(&self, rendered: &Rendered<'_>, out: &mut dyn std::io::Write) {
        let Rendered {
            report,
            view,
            reasons,
            ..
        } = *rendered;
        let envelope = JsonReport {
            version: REPORT_VERSION,
            summary: Summary::of(rendered),
            findings: view.breakdown().is_none().then(|| {
                view.findings()
                    .iter()
                    .map(|finding| JsonFinding::new(finding, reasons))
                    .collect()
            }),
            decisions: reasons
                .decisions()
                .map(crate::describe::JsonDecision::of)
                .collect(),
            unreadable_files: report
                .unreadable_files
                .iter()
                .map(|(path, reason)| UnreadableFile {
                    path,
                    reason: reason.as_str(),
                })
                .collect(),
        };

        let _ = writeln!(out, "{}", written(&envelope));
    }

    /// Writes a single-file check as the versioned JSON object.
    ///
    /// A second shape rather than a second renderer: `check --file` answers
    /// about one file, and its object carries `skipped` and
    /// `unresolved_imports` that a whole-repository report has no place for.
    /// Both are this format's contract, so both live with it.
    fn render_single(
        &self,
        single: &archwarden_engine::single::Single,
        reasons: &Reasons,
        out: &mut dyn std::io::Write,
    ) {
        let envelope = JsonSingle {
            version: REPORT_VERSION,
            path: &single.path,
            findings: single
                .findings
                .iter()
                .map(|finding| JsonFinding::new(finding, reasons))
                .collect(),
            decisions: referenced_by(&single.findings, reasons),
            skipped: single
                .skipped
                .iter()
                .map(|skipped| JsonSkipped {
                    rule_id: skipped.rule_id.as_str(),
                    reason: skipped.reason.as_str(),
                })
                .collect(),
            unresolved_imports: &single.unresolved_imports,
        };

        let _ = writeln!(out, "{}", written(&envelope));
    }
}

#[derive(Debug, Serialize)]
struct JsonReport<'a> {
    version: u32,
    summary: Summary,
    /// Absent under `--summary`, which is the point of the flag: a summary
    /// that still emitted every finding would give a piping consumer no size
    /// benefit at all. Absence is opt-in — a consumer that never passes the
    /// flag sees the field it always saw.
    #[serde(skip_serializing_if = "Option::is_none")]
    findings: Option<Vec<JsonFinding<'a>>>,
    /// The decisions the configuration declares, with their prose, once.
    ///
    /// Every decision, not only the ones with findings against them: a
    /// consumer charting a report wants to say "eleven decisions, two being
    /// broken", and it cannot say that from the ones that failed. Absent when
    /// the config declares none. Issue #100.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    decisions: Vec<crate::describe::JsonDecision<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unreadable_files: Vec<UnreadableFile<'a>>,
}

/// A finding, plus the standing reason its rule exists.
///
/// `why` is flattened in beside the finding's own fields rather than nested,
/// because a consumer reading a finding is reading one object. It is absent
/// when the rule's author said nothing, which is every rule in every config
/// written before the field existed.
#[derive(Debug, Serialize)]
struct JsonFinding<'a> {
    #[serde(flatten)]
    finding: &'a Finding,
    #[serde(skip_serializing_if = "Option::is_none")]
    why: Option<&'a str>,
    /// The decision this finding's rule implements, by id.
    ///
    /// The id, where `why` above is the whole string, and the asymmetry is
    /// deliberate. A reason belongs to one rule, so repeating it per finding
    /// costs one copy per finding *of that rule*. A decision belongs to many
    /// rules by construction — that is the argument for the prose living on
    /// the config at all — so repeating the block would put the same paragraph
    /// on every finding of every rule serving it. The prose is in the report's
    /// `decisions`, once. Issue #100.
    #[serde(skip_serializing_if = "Option::is_none")]
    decision: Option<&'a str>,
}
impl<'a> JsonFinding<'a> {
    fn new(finding: &'a Finding, reasons: &'a Reasons) -> Self {
        Self {
            finding,
            why: reasons.of_rule(&finding.rule_id),
            decision: reasons
                .decision_of_rule(&finding.rule_id)
                .map(|decision| decision.id.as_str()),
        }
    }
}

/// The decisions a set of findings names, each once, in the order met.
///
/// Deduplicated because that is the whole shape of the thing: many rules serve
/// one decision, so two findings from two rules routinely name the same one.
#[must_use]
pub fn referenced_by<'a>(
    findings: &[Finding],
    reasons: &'a Reasons,
) -> Vec<crate::describe::JsonDecision<'a>> {
    let mut seen: Vec<crate::describe::JsonDecision<'a>> = Vec::new();
    for finding in findings {
        if let Some(decision) = reasons.decision_of_rule(&finding.rule_id)
            && !seen.iter().any(|kept| kept.id == decision.id.as_str())
        {
            seen.push(crate::describe::JsonDecision::of(decision));
        }
    }
    seen
}

#[derive(Debug, Serialize)]
struct UnreadableFile<'a> {
    path: &'a archwarden_core::path::RepoRelPath,
    reason: &'a str,
}

/// The JSON envelope for a single-file check.
#[derive(Debug, Serialize)]
struct JsonSingle<'a> {
    version: u32,
    path: &'a archwarden_core::path::RepoRelPath,
    findings: Vec<JsonFinding<'a>>,
    /// The decisions the findings above reference, with their prose.
    ///
    /// Only those, where the whole-repository report carries every declared
    /// decision — because the questions differ. A consumer charting a run
    /// wants to say "eleven decisions, two being broken"; a caller asking
    /// about one file wants to know what *this* write breaks, and eleven
    /// paragraphs about decisions it does not touch is the noise a pre-write
    /// answer cannot afford. Issue #100.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    decisions: Vec<crate::describe::JsonDecision<'a>>,
    /// Always present, even when empty. A caller needs to see that the list is
    /// empty rather than infer it from absence -- that is the whole point of
    /// reporting skips (correction C6).
    skipped: Vec<JsonSkipped<'a>>,
    /// Imports of this file that nothing could resolve, so no boundary rule
    /// saw them.
    ///
    /// Always present for the same reason as `skipped`: a caller has to be
    /// able to tell "this file has no blind spot" from "this build does not
    /// report them". Issue #18.
    unresolved_imports: &'a [String],
}

#[derive(Debug, Serialize)]
struct JsonSkipped<'a> {
    rule_id: &'a str,
    reason: &'static str,
}

/// A value as pretty JSON, or an object saying why not.
///
/// A report that cannot be serialised is a bug in these types, not something a
/// user can act on — so it is reported as itself rather than silently
/// producing nothing. A consumer parsing the output finds an object either
/// way, which is the one thing it can rely on.
///
/// Its own function because the failing arm is not reachable through either
/// envelope: both are built from types that always serialise. Left inline it
/// would be four lines nothing exercises, in the crate held to the highest
/// floor. Extracted, the mapping is pinned on a value that genuinely fails.
fn written(value: &impl Serialize) -> String {
    match serde_json::to_string_pretty(value) {
        Ok(json) => json,
        Err(error) => format!(r#"{{"error":"cannot serialise report: {error}"}}"#),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::present::{Breakdown, View};
    use archwarden_core::{
        finding::{Expectation, Finding, Observed},
        ids::{ModuleId, RuleId},
        level::Level,
        path::RepoRelPath,
    };
    use archwarden_engine::run::Report;
    use camino::Utf8Path;

    const TOOK: std::time::Duration = std::time::Duration::from_millis(12);

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

    /// A breakdown over four rules, of which two fired.
    fn four_rules() -> (Report, Breakdown) {
        let at = |rule: &str, level, path_at| Finding {
            rule_id: RuleId::new(rule).expect("valid"),
            path: path(path_at),
            ..finding(level, None)
        };
        let findings = vec![
            at("domain-entity-shape", Level::Error, "packages/domain/a"),
            at("domain-entity-shape", Level::Error, "packages/domain/b"),
            at("actions-need-spec", Level::Warning, "packages/domain/c"),
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

    fn rendered_view(report: &Report, view: &View<'_>) -> String {
        let mut out = Vec::new();
        Json.render(
            &Rendered {
                root: Utf8Path::new("."),
                report,
                view,
                reasons: &Reasons::default(),
                elapsed: TOOK,
                standing: None,
                as_of: archwarden_core::date::Date::EPOCH,
            },
            &mut out,
        );
        String::from_utf8(out).expect("UTF-8")
    }

    fn rendered_after(report: &Report, elapsed: std::time::Duration) -> String {
        let mut out = Vec::new();
        Json.render(
            &Rendered {
                root: Utf8Path::new("."),
                report,
                view: &View::everything(report),
                reasons: &Reasons::default(),
                elapsed,
                standing: None,
                as_of: archwarden_core::date::Date::EPOCH,
            },
            &mut out,
        );
        String::from_utf8(out).expect("UTF-8")
    }

    fn rendered(report: &Report) -> String {
        rendered_after(report, TOOK)
    }

    fn rendered_with(report: &Report, reasons: &Reasons) -> String {
        let mut out = Vec::new();
        Json.render(
            &Rendered {
                root: Utf8Path::new("."),
                report,
                view: &View::everything(report),
                reasons,
                elapsed: TOOK,
                standing: None,
                as_of: archwarden_core::date::Date::EPOCH,
            },
            &mut out,
        );
        String::from_utf8(out).expect("UTF-8")
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
        let json = rendered(&report(vec![finding(Level::Error, None)]));
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert!(parsed["findings"].is_array(), "{json}");
        assert!(parsed["summary"].get("by_rule").is_none(), "{json}");
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
            serde_json::from_str(&rendered(&report(vec![finding]))).expect("valid JSON");

        assert_eq!(parsed["findings"][0]["span"]["start"], 25);
        assert_eq!(parsed["findings"][0]["path"], "src/a.ts");
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
                serde_json::from_str(&rendered(&report)).expect("valid JSON");

            assert_eq!(parsed["summary"]["checks_skipped"], skipped);
        }
    }
    /// The counts describe what is shown, and the JSON says how many were
    /// not: a consumer comparing `errors` against the exit code needs the
    /// number, not the prose.
    #[test]
    fn the_json_reports_what_the_filters_hid() {
        let report = report(vec![
            Finding {
                rule_id: RuleId::new("shape").expect("valid"),
                path: path("a"),
                ..finding(Level::Error, None)
            },
            Finding {
                rule_id: RuleId::new("spec").expect("valid"),
                path: path("b"),
                ..finding(Level::Warning, None)
            },
        ]);
        let shown: Vec<&Finding> = report.findings.iter().take(1).collect();

        let json = rendered_view(&report, &View::filtered(&shown, 1));
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(parsed["summary"]["errors"], 1);
        assert_eq!(parsed["summary"]["warnings"], 0, "the warning was filtered");
        assert_eq!(parsed["summary"]["hidden"], 1);
    }
    /// The machine-readable half gets the raw number, not the prose. A
    /// consumer comparing two runs needs to subtract them.
    #[test]
    fn the_json_carries_the_duration_as_a_number() {
        let parsed: serde_json::Value = serde_json::from_str(&rendered_after(
            &report(Vec::new()),
            std::time::Duration::from_millis(1450),
        ))
        .expect("valid JSON");

        assert_eq!(parsed["summary"]["duration_ms"], 1450);
    }
    /// The JSON shape is a contract with agents, so its envelope is asserted
    /// field by field rather than by eyeballing a dump.
    #[test]
    fn the_json_envelope_is_versioned_and_summarised() {
        let json = rendered(&report(vec![finding(Level::Error, Some("domain"))]));
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
        let json = rendered(&report(Vec::new()));
        assert!(!json.contains("unreadable_files"), "{json}");
    }
    /// A consumer renders it itself, so every finding carries it -- the
    /// once-per-rule economy is a property of the text output, not of the data.
    #[test]
    fn every_finding_carries_the_reason_in_json() {
        let report = report(vec![
            finding(Level::Error, None),
            finding(Level::Error, None),
        ]);
        let reasons = Reasons::from([("domain-entity-shape", "it is published")]);

        let text = rendered_with(&report, &reasons);
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");

        assert_eq!(parsed["findings"][0]["why"], "it is published");
        assert_eq!(parsed["findings"][1]["why"], "it is published");
    }
    /// The decision is **normalised**, and that is the one place this shape
    /// deliberately differs from `why` beside it.
    ///
    /// A reason belongs to one rule, so repeating it per finding costs one
    /// copy per finding of that rule. A decision belongs to many rules by
    /// construction — that is the whole argument for the prose living on the
    /// config — so repeating the block would put the same paragraph on every
    /// finding of eight rules. The findings name it by id and the report
    /// carries the prose once. Issue #100.
    #[test]
    fn a_finding_names_its_decision_and_the_report_carries_the_prose_once() {
        let report = report(vec![
            finding(Level::Error, None),
            finding(Level::Error, None),
        ]);
        let reasons = Reasons::from([("domain-entity-shape", "it is published")]).deciding([(
            "domain-entity-shape",
            archwarden_core::compiled::CompiledDecision {
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
            },
        )]);

        let parsed: serde_json::Value =
            serde_json::from_str(&rendered_with(&report, &reasons)).expect("valid JSON");

        assert_eq!(parsed["findings"][0]["decision"], "ADR-014");
        assert_eq!(parsed["findings"][1]["decision"], "ADR-014");

        let decisions = parsed["decisions"].as_array().expect("a list");
        assert_eq!(decisions.len(), 1, "the prose appears once: {parsed}");
        assert_eq!(decisions[0]["id"], "ADR-014");
        assert_eq!(
            decisions[0]["title"],
            "The domain does not know about transport"
        );
        assert_eq!(decisions[0]["link"], "docs/adr/014.md");
        assert_eq!(decisions[0]["status"], "accepted");
    }

    /// The single-file shape carries only the decisions its findings name —
    /// a caller asking about one file wants to know what *this* write breaks,
    /// and a paragraph about every decision in the repository is noise a
    /// pre-write answer cannot afford. Each named one appears once, however
    /// many findings reach it.
    #[test]
    fn a_single_file_check_carries_the_decisions_its_findings_name() {
        let other = Finding {
            rule_id: archwarden_core::ids::RuleId::new("domain-forbids-http").expect("valid"),
            ..finding(Level::Error, None)
        };
        let single = archwarden_engine::single::Single {
            path: path("src/user/create.use-case.ts"),
            findings: vec![finding(Level::Error, None), other],
            skipped: Vec::new(),
            unresolved_imports: Vec::new(),
        };

        let adr = archwarden_core::compiled::CompiledDecision {
            scope: None,
            why_not_enforceable: None,
            id: archwarden_core::ids::DecisionId::new("ADR-014").expect("valid"),
            title: "The domain does not know about transport".to_owned(),
            why: None,
            link: Some("docs/adr/014.md".to_owned()),
            status: archwarden_core::compiled::DecisionStatus::Accepted,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            alternatives: Vec::new(),
        };
        // Two rules, one decision, plus a third decision nothing here names.
        let reasons = Reasons::default().deciding([
            ("domain-entity-shape", adr.clone()),
            ("domain-forbids-http", adr),
            (
                "untouched",
                archwarden_core::compiled::CompiledDecision {
                    scope: None,
                    why_not_enforceable: None,
                    id: archwarden_core::ids::DecisionId::new("ADR-020").expect("valid"),
                    title: "Not about this file".to_owned(),
                    why: None,
                    link: None,
                    status: archwarden_core::compiled::DecisionStatus::Accepted,
                    supersedes: Vec::new(),
                    superseded_by: Vec::new(),
                    alternatives: Vec::new(),
                },
            ),
        ]);

        let mut out = Vec::new();
        Json.render_single(&single, &reasons, &mut out);
        let parsed: serde_json::Value =
            serde_json::from_str(&String::from_utf8(out).expect("UTF-8")).expect("valid JSON");

        let decisions = parsed["decisions"].as_array().expect("a list");
        assert_eq!(decisions.len(), 1, "once, and only the named one: {parsed}");
        assert_eq!(decisions[0]["id"], "ADR-014");
        assert_eq!(decisions[0]["link"], "docs/adr/014.md");
        assert_eq!(parsed["findings"][1]["decision"], "ADR-014");
    }

    /// And a single-file check over a config with no decisions omits the key,
    /// which is what a pre-write hook has always received.
    #[test]
    fn a_single_file_check_with_no_decisions_omits_the_key() {
        let single = archwarden_engine::single::Single {
            path: path("src/user/create.use-case.ts"),
            findings: vec![finding(Level::Error, None)],
            skipped: Vec::new(),
            unresolved_imports: Vec::new(),
        };

        let mut out = Vec::new();
        Json.render_single(&single, &Reasons::default(), &mut out);
        let json = String::from_utf8(out).expect("UTF-8");

        assert!(!json.contains("decision"), "{json}");
    }

    /// A run over a config with no decisions produces the report it produced
    /// before 0.21, key for key.
    #[test]
    fn a_report_with_no_decisions_grows_no_key() {
        let json = rendered_with(
            &report(vec![finding(Level::Error, None)]),
            &Reasons::from([("domain-entity-shape", "it is published")]),
        );

        assert!(!json.contains("decision"), "{json}");
    }

    /// The counts reach JSON too, where a tool can chart them.
    #[test]
    fn the_json_summary_carries_the_cache_split() {
        let json = rendered(&Report {
            files_parsed: 2,
            facts_reused: 32,
            ..report(Vec::new())
        });
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(parsed["summary"]["files_parsed"], 2);
        assert_eq!(parsed["summary"]["facts_reused"], 32);
    }
    /// A run that resolved nothing carries no `imports` object at all, so a
    /// consumer can tell "no boundary rule" from "everything resolved".
    #[test]
    fn a_run_that_resolved_nothing_omits_the_import_summary() {
        let json = rendered(&report(Vec::new()));
        assert!(!json.contains("\"imports\""), "{json}");
    }

    /// The single-file shape, which `check --file`, the pre-write hook and an
    /// editor all consume. `skipped` and `unresolved_imports` are present even
    /// when empty, and that is the whole point of them: a caller has to be
    /// able to tell "this file has no blind spot" from "this build does not
    /// report them". Correction C6 and issue #18.
    #[test]
    fn a_single_file_check_always_carries_its_skips_and_blind_spots() {
        let single = archwarden_engine::single::Single {
            path: path("src/user/create.use-case.ts"),
            findings: vec![finding(Level::Error, None)],
            skipped: Vec::new(),
            unresolved_imports: Vec::new(),
        };

        let mut out = Vec::new();
        Json.render_single(&single, &Reasons::default(), &mut out);
        let parsed: serde_json::Value =
            serde_json::from_str(&String::from_utf8(out).expect("UTF-8")).expect("valid JSON");

        assert_eq!(parsed["version"], REPORT_VERSION);
        assert_eq!(parsed["path"], "src/user/create.use-case.ts");
        assert_eq!(parsed["findings"].as_array().map(Vec::len), Some(1));
        assert!(parsed["skipped"].is_array(), "present even when empty");
        assert!(
            parsed["unresolved_imports"].is_array(),
            "present even when empty"
        );
    }

    /// And when there is something to report, it carries the slug a caller
    /// branches on rather than the sentence a human reads.
    #[test]
    fn a_skip_is_reported_by_its_slug_and_the_rule_that_wanted_it() {
        let single = archwarden_engine::single::Single {
            path: path("src/notes.md"),
            findings: Vec::new(),
            skipped: vec![archwarden_engine::single::Skipped {
                rule_id: "usecase-name".to_owned(),
                reason: archwarden_engine::single::Reason::NotSource,
            }],
            unresolved_imports: vec!["@app/nowhere".to_owned()],
        };

        let mut out = Vec::new();
        Json.render_single(&single, &Reasons::default(), &mut out);
        let parsed: serde_json::Value =
            serde_json::from_str(&String::from_utf8(out).expect("UTF-8")).expect("valid JSON");

        assert_eq!(parsed["skipped"][0]["rule_id"], "usecase-name");
        assert_eq!(parsed["skipped"][0]["reason"], "not-source");
        assert_eq!(parsed["unresolved_imports"][0], "@app/nowhere");
    }

    /// A file a rule wanted and could not read is named in the report rather
    /// than dropped: a clean report about a file nothing parsed would be
    /// lying about it.
    #[test]
    fn a_file_that_could_not_be_read_is_named() {
        let mut report = report(Vec::new());
        report.unreadable_files = vec![(path("src/broken.ts"), "unexpected token".to_owned())];

        let parsed: serde_json::Value =
            serde_json::from_str(&rendered(&report)).expect("valid JSON");

        assert_eq!(parsed["unreadable_files"][0]["path"], "src/broken.ts");
        assert_eq!(parsed["unreadable_files"][0]["reason"], "unexpected token");
    }

    /// The count of checks nobody could make is a number nobody can act on
    /// without the list beside it, so the list is there too.
    #[test]
    fn a_skipped_check_is_named_as_well_as_counted() {
        let mut report = report(Vec::new());
        report.checks_skipped = 1;
        report.skipped_checks = vec![("usecase-name".to_owned(), path("src/broken.ts"))];

        let parsed: serde_json::Value =
            serde_json::from_str(&rendered(&report)).expect("valid JSON");

        assert_eq!(parsed["summary"]["checks_skipped"], 1);
        assert_eq!(
            parsed["summary"]["skipped_checks"][0]["rule_id"],
            "usecase-name"
        );
        assert_eq!(
            parsed["summary"]["skipped_checks"][0]["path"],
            "src/broken.ts"
        );
    }

    /// A value that cannot be serialised comes back as an object saying so,
    /// not as nothing. A consumer parsing the output finds an object either
    /// way, which is the one thing it can rely on.
    ///
    /// Neither envelope can reach this — both are built from types that always
    /// serialise — so the mapping is asserted on a value that genuinely fails:
    /// JSON has no way to write a map whose keys are not strings.
    #[test]
    fn a_value_that_cannot_be_serialised_says_so_in_the_shape_it_promised() {
        let unwritable: std::collections::BTreeMap<(u8, u8), u8> =
            [((1, 2), 3)].into_iter().collect();

        let rendered = written(&unwritable);

        let parsed: serde_json::Value =
            serde_json::from_str(&rendered).expect("still an object: {rendered}");
        assert!(
            parsed["error"]
                .as_str()
                .expect("an error string")
                .starts_with("cannot serialise report: "),
            "{rendered}"
        );
    }

    /// And a value that can is written as itself.
    #[test]
    fn a_value_that_can_be_serialised_is_written_as_itself() {
        assert_eq!(written(&42_u8), "42");
    }

    /// Every import nothing could place, not just the count. A CI job gating
    /// on "no import escapes the boundary rules" needs the whole list — a
    /// boundary rule cannot see an import it could not resolve, so a clean
    /// report over a repository whose dependencies are not installed means
    /// less than it looks like. Issue #18.
    #[test]
    fn the_imports_that_did_not_resolve_are_named_one_by_one() {
        let mut report = report(Vec::new());
        report.imports = archwarden_engine::resolve::Outcomes {
            in_repo: 40,
            external: 12,
            builtin: 3,
            unresolved: 2,
            unresolved_imports: vec![
                (
                    path("packages/domain/row.ts"),
                    "@Domain/Order/id".to_owned(),
                ),
                (path("packages/domain/seed.ts"), "@Shared/clock".to_owned()),
            ],
        };

        let parsed: serde_json::Value =
            serde_json::from_str(&rendered(&report)).expect("valid JSON");

        let named = &parsed["summary"]["imports"]["unresolved_imports"];
        assert_eq!(named[0]["path"], "packages/domain/row.ts");
        assert_eq!(named[0]["specifier"], "@Domain/Order/id");
        assert_eq!(named[1]["path"], "packages/domain/seed.ts");
        assert_eq!(
            named.as_array().map(Vec::len),
            Some(2),
            "as many as the count says"
        );
    }

    /// `hidden` is absent when nothing was hidden, not zero.
    ///
    /// A consumer reading a report from an unfiltered run should see the
    /// object it always saw; a field that appeared on every run as `0` is one
    /// more thing to explain, and the flag that produces it is the exception
    /// rather than the rule. When a filter *did* hide something the number is
    /// there, because a consumer comparing `errors` against the exit code
    /// needs it.
    #[test]
    fn nothing_hidden_leaves_the_field_out_and_something_hidden_puts_it_in() {
        let report = report(vec![
            finding(Level::Error, None),
            finding(Level::Warning, None),
        ]);

        let unfiltered: serde_json::Value =
            serde_json::from_str(&rendered(&report)).expect("valid JSON");
        assert!(
            unfiltered["summary"].get("hidden").is_none(),
            "{unfiltered}"
        );

        let shown: Vec<&Finding> = report.findings.iter().take(1).collect();
        let filtered: serde_json::Value =
            serde_json::from_str(&rendered_view(&report, &View::filtered(&shown, 1)))
                .expect("valid JSON");
        assert_eq!(filtered["summary"]["hidden"], 1);
    }
}
