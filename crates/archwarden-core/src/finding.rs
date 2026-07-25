//! What a rule reports, and why.
//!
//! A finding carries the expectation and the observation as **structured
//! values**, not as a pre-rendered message. That costs a little now and buys
//! three things: `explain` can show the rule definition beside the observed
//! state without re-deriving either, `--format json` has a stable shape for
//! agents to consume, and the richer `explain` planned for v1 is a rendering
//! change rather than a rewrite of every rule.

use serde::{Deserialize, Serialize};

use crate::{
    facts::{ExportTags, KindFilter, Span},
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
};

/// What a rule requires of a file.
///
/// Every variant is also what `scaffold` prints and what `agent-guide`
/// documents, which is why rule engines build these rather than strings: one
/// value serves the checker, the informant, and the JSON output.
///
/// The discriminator is `type`, not `kind`, because `kind` is already the
/// documented field name for an export's declaration form in the `scaffold`
/// JSON (`docs/AGENT-INTEGRATION.md`). `type` also matches how the config
/// spells its own rule discriminator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Expectation {
    /// Only these subdirectories may exist here.
    AllowedSubfolders {
        /// Names that are permitted.
        allowed: Vec<String>,
        /// Names that are permitted but reported as warnings.
        warn: Vec<String>,
    },
    /// Every file here must match one of these filename patterns.
    FilenamePattern {
        /// The patterns, as written in the config.
        patterns: Vec<String>,
    },
    /// The file must export a symbol of this shape.
    RequiredExport {
        /// The declaration forms that satisfy the rule.
        kind: KindFilter,
        /// The required name, already rendered from the filename.
        name: String,
        /// A free-form signature shown by `scaffold`. Never verified.
        signature_hint: Option<String>,
    },
    /// A sibling file must exist.
    RequiredSibling {
        /// The sibling's path.
        path: RepoRelPath,
        /// Whether it must contain at least one `it(...)` or `test(...)`.
        non_empty_spec: bool,
    },
    /// Imports matching these patterns are not allowed.
    ForbiddenImport {
        /// Glob patterns matched against the resolved import path.
        patterns: Vec<String>,
        /// Exceptions, also matched against the resolved path.
        except: Vec<String>,
        /// Whether `import type` counts.
        include_type_only: bool,
    },
    /// At least one import must match these patterns.
    RequiredImport {
        /// Glob patterns matched against the resolved import path.
        patterns: Vec<String>,
    },
    /// The file must call this symbol.
    RequiredCall {
        /// The callee as it appears at a call site, e.g. `Event.save`.
        symbol: String,
        /// The module the symbol must be imported from.
        imported_from: String,
    },
}

/// What the file actually looked like.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Observed {
    /// A subdirectory that is on neither list.
    UnexpectedSubfolder {
        /// The directory's name.
        name: String,
    },
    /// A subdirectory on `warn_subfolders`: permitted, but documented debt.
    ///
    /// Distinct from [`Observed::UnexpectedSubfolder`] because the two need
    /// different sentences. "Not allowed here" under a `warning` reads as a
    /// contradiction, and a reader is right to wonder which half to believe.
    DiscouragedSubfolder {
        /// The directory's name.
        name: String,
    },
    /// A filename matching none of the rule's patterns.
    UnexpectedFilename {
        /// The file's name.
        name: String,
    },
    /// No export by the required name.
    ExportMissing {
        /// The name that was looked for.
        name: String,
    },
    /// An export by the right name, declared the wrong way.
    ExportWrongKind {
        /// The name that was found.
        name: String,
        /// How it was actually declared.
        found: ExportTags,
    },
    /// The only export is a default, whose name does not bind importers.
    OnlyDefaultExport,
    /// A re-export, whose kind cannot be determined without cross-file work.
    ReexportOfUnknownKind {
        /// The name that was found.
        name: String,
        /// Where it is re-exported from.
        from: String,
    },
    /// The required sibling is not there.
    SiblingMissing {
        /// The sibling that was looked for.
        path: RepoRelPath,
    },
    /// The sibling exists but contains no test cases.
    SpecIsEmpty {
        /// The spec file.
        path: RepoRelPath,
    },
    /// An import that the rule forbids.
    ForbiddenImport {
        /// The specifier as written.
        specifier: String,
        /// Where it resolved to.
        resolved: RepoRelPath,
    },
    /// No import satisfied a `must_import_from`.
    RequiredImportMissing,
    /// The symbol is imported but never called.
    RequiredCallMissing {
        /// The callee that was looked for.
        symbol: String,
    },
    /// The symbol is not even imported, so it certainly is not called.
    RequiredImportForCallMissing {
        /// The callee that was looked for.
        symbol: String,
        /// The module it should come from.
        module: String,
    },
}

/// One rule's verdict on one file.
///
/// Not `#[non_exhaustive]`, unlike [`Observed`] and [`Expectation`]. Those two
/// are matched on by downstream code, which is what the attribute protects; a
/// finding is *built* by every rule engine, and the attribute would make that
/// impossible from another crate. Adding a field here is a breaking change,
/// and should be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Which rule fired.
    pub rule_id: RuleId,
    /// The module the rule belongs to, if any. Rules declared at the top level
    /// have none and report as `[*]`.
    pub module_id: Option<ModuleId>,
    /// How seriously to take it.
    pub level: Level,
    /// The offending file or directory.
    pub path: RepoRelPath,
    /// Where in the file, when the rule looked inside one.
    pub span: Option<Span>,
    /// What was found.
    pub observed: Observed,
    /// What was required.
    pub expected: Expectation,
}

impl Finding {
    /// The key a report sorts by.
    ///
    /// Sorting is worst-first, then by path, then by rule id. Determinism is a
    /// design goal: the same inputs must produce byte-identical output, or
    /// snapshot tests and CI diffs become noise.
    #[must_use]
    pub fn sort_key(&self) -> (std::cmp::Reverse<Level>, &str, &str) {
        (
            std::cmp::Reverse(self.level),
            self.path.as_str(),
            self.rule_id.as_str(),
        )
    }
}

impl PartialOrd for Finding {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Finding {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.sort_key().cmp(&other.sort_key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::ExportKind;

    fn rule(id: &str) -> RuleId {
        RuleId::new(id).expect("valid id")
    }

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn finding(id: &str, level: Level, p: &str) -> Finding {
        Finding {
            rule_id: rule(id),
            module_id: None,
            level,
            path: path(p),
            span: None,
            observed: Observed::OnlyDefaultExport,
            expected: Expectation::RequiredExport {
                kind: KindFilter::Any,
                name: "Foo".to_owned(),
                signature_hint: None,
            },
        }
    }

    /// Reports are sorted worst-first, then by path, then by rule. Determinism
    /// is a design goal: unstable ordering turns every snapshot test and CI
    /// diff into noise.
    #[test]
    fn findings_sort_errors_first_then_by_path_then_by_rule() {
        let mut findings = [
            finding("z-rule", Level::Warning, "a/first.ts"),
            finding("b-rule", Level::Error, "b/second.ts"),
            finding("a-rule", Level::Error, "b/second.ts"),
            finding("m-rule", Level::Error, "a/first.ts"),
        ];
        findings.sort();

        let order: Vec<_> = findings
            .iter()
            .map(|f| (f.level, f.path.as_str(), f.rule_id.as_str()))
            .collect();

        assert_eq!(
            order,
            [
                (Level::Error, "a/first.ts", "m-rule"),
                (Level::Error, "b/second.ts", "a-rule"),
                (Level::Error, "b/second.ts", "b-rule"),
                (Level::Warning, "a/first.ts", "z-rule"),
            ]
        );
    }

    #[test]
    fn sorting_is_stable_across_runs() {
        let build = || {
            let mut f = vec![
                finding("b", Level::Error, "x.ts"),
                finding("a", Level::Warning, "x.ts"),
                finding("c", Level::Error, "a.ts"),
            ];
            f.sort();
            f
        };
        assert_eq!(build(), build());
    }

    /// The JSON shape is a contract with agents and with `--format json`
    /// consumers. Variants are externally tagged with an explicit `kind`, so
    /// adding a variant later cannot reshape the existing ones.
    #[test]
    fn expectations_carry_an_explicit_type_tag() {
        let expectation = Expectation::RequiredSibling {
            path: path("src/user.spec.ts"),
            non_empty_spec: true,
        };
        let json = serde_json::to_string(&expectation).expect("serialises");
        assert_eq!(
            json,
            r#"{"type":"required-sibling","path":"src/user.spec.ts","non_empty_spec":true}"#
        );

        let parsed: Expectation = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(parsed, expectation);
    }

    #[test]
    fn observations_carry_an_explicit_type_tag() {
        let observed = Observed::ExportWrongKind {
            name: "CreateClient".to_owned(),
            found: ExportTags::only(ExportKind::Const).with(ExportKind::Arrow),
        };
        let json = serde_json::to_string(&observed).expect("serialises");
        assert_eq!(
            json,
            r#"{"type":"export-wrong-kind","name":"CreateClient","found":["arrow","const"]}"#
        );

        let parsed: Observed = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(parsed, observed);
    }

    /// A finding holds the expectation and the observation side by side, which
    /// is what lets `explain` answer "what was required, and what was there?"
    /// without re-running the rule.
    #[test]
    fn a_finding_round_trips_with_both_halves_intact() {
        let original = Finding {
            rule_id: rule("usecase-factory-name"),
            module_id: Some(ModuleId::new("application").expect("valid")),
            level: Level::Error,
            path: path("packages/application/src/use-cases/foo/foo.use-case.ts"),
            span: Some(Span::new(0, 42)),
            observed: Observed::ExportMissing {
                name: "Foo".to_owned(),
            },
            expected: Expectation::RequiredExport {
                kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
                name: "Foo".to_owned(),
                signature_hint: Some("(deps: FooDeps) => UseCase".to_owned()),
            },
        };

        let json = serde_json::to_string(&original).expect("serialises");
        let parsed: Finding = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(parsed, original);
    }

    /// A rule with no module reports as belonging to none, which the reporter
    /// renders as `[*]`. Import boundaries are the usual case.
    #[test]
    fn a_finding_may_have_no_module() {
        let f = finding("domain-forbids-application", Level::Error, "x.ts");
        assert_eq!(f.module_id, None);

        let json = serde_json::to_string(&f).expect("serialises");
        assert!(json.contains(r#""module_id":null"#), "{json}");
    }
}
