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
    /// Some rule must govern the file.
    ///
    /// Carries nothing, like [`NoImportCycle`](Self::NoImportCycle) and for a
    /// related reason: there is no shape to name. The requirement is that
    /// *any* rule claim the file, and which rule is the author's choice —
    /// naming one here would be this crate deciding somebody's architecture.
    GovernedBySomeRule,

    /// The file must not sit on an import loop.
    ///
    /// Carries nothing. Every other expectation names what the file should
    /// have looked like, and there is nothing to name here: the requirement is
    /// the absence of a shape, and the shape that broke it is in the
    /// [`Observed::ImportCycle`] beside it.
    NoImportCycle,

    /// Exports that are nothing but a forward of another module.
    NoPassthrough {
        /// The shapes the rule refuses.
        forms: Vec<String>,
    },

    /// Only these subdirectories may exist here.
    AllowedSubfolders {
        /// Names that are permitted.
        allowed: Vec<String>,
        /// Names that are permitted but reported as warnings.
        warn: Vec<String>,
        /// Regexes a name may match instead of being listed, as written in the
        /// config. Empty when the rule constrains names by enumeration only.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        patterns: Vec<String>,
    },
    /// Every file here must match one of these filename patterns.
    FilenamePattern {
        /// The patterns, as written in the config.
        patterns: Vec<String>,
    },
    /// This directory's *own* name is constrained by the rule governing its
    /// parent.
    ///
    /// The sibling of [`FilenamePattern`](Self::FilenamePattern), and it was
    /// missing. `filename_patterns` is attributed to the file it governs;
    /// `subfolder_patterns` was attributed only to the parent, so `describe`
    /// answered "no rule applies" about a folder `check` refuses and
    /// `scaffold` handed back a shape to build at a path that cannot pass.
    ///
    /// A path that does not exist yet is exactly where the name is still a
    /// choice, which is what `describe` is for.
    FolderName {
        /// Names that are permitted outright.
        allowed: Vec<String>,
        /// Names permitted but reported as warnings.
        warn: Vec<String>,
        /// Regexes the name may match instead of being listed.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        patterns: Vec<String>,
    },
    /// The file must export a symbol of this shape.
    RequiredExport {
        /// The declaration forms that satisfy the rule.
        kind: KindFilter,
        /// The required name, already rendered from the filename.
        name: String,
        /// The type annotations that satisfy the rule, any one of them, already
        /// rendered. Empty when the rule does not ask for one.
        ///
        /// Distinct from `signature_hint` on purpose. That field is a
        /// suggestion `scaffold` renders and `check` ignores, and code depends
        /// on that; this one is checked. Keeping the promise of each legible is
        /// worth the second field.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        annotation: Vec<String>,
        /// A free-form signature shown by `scaffold`. Never verified.
        signature_hint: Option<String>,
    },
    /// These files must exist in the directory.
    RequiredFiles {
        /// Filenames that must be there.
        names: Vec<String>,
        /// Regexes at least one file must match, one file per entry.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        patterns: Vec<String>,
    },
    /// A document's frontmatter must carry these keys.
    RequiredFrontmatter {
        /// Keys that must be there.
        keys: Vec<String>,
        /// The closed vocabulary a key's value must come from.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        vocabularies: Vec<(String, Vec<String>)>,
        /// A key whose value must equal this, already rendered from the path.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        agreements: Vec<(String, String)>,
    },
    /// A companion file must exist, named relative to this one.
    ///
    /// Distinct from [`RequiredSibling`](Self::RequiredSibling), which carries
    /// `spec-pair`'s `non_empty_spec` and means a derived `<stem>.<marker>`
    /// name. This one is literal and may sit outside the directory, and a
    /// consumer branching on the tag should be able to tell them apart.
    RequiredCompanion {
        /// Where it goes, resolved.
        path: RepoRelPath,
    },
    /// A sibling file must exist.
    RequiredSibling {
        /// The sibling's path.
        path: RepoRelPath,
        /// Whether it must contain at least one `it(...)` or `test(...)`.
        non_empty_spec: bool,
    },
    /// These imports are allowed, and nothing else in the repository is.
    ///
    /// The allowlist direction, and the reason it exists: a denylist decays.
    /// Every new package, app or directory is permitted by omission, and
    /// omission is invisible — the failure `CONFIG.md` names as the worst a
    /// linter has, arriving one import at a time. Issue #75.
    ///
    /// Governs edges inside this repository only. A dependency has its own
    /// axis, [`PermittedPackages`](Self::PermittedPackages), for the same
    /// reason forbidding one does.
    PermittedImports {
        /// Glob patterns matched against the resolved import path. Anything
        /// not matching is refused.
        patterns: Vec<String>,
        /// Whether `import type` counts.
        include_type_only: bool,
    },
    /// These packages are allowed, and no others are.
    PermittedPackages {
        /// Package names, matched as "this package, and anything under it".
        packages: Vec<String>,
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
    /// Depending on these, at any distance, is not allowed.
    ///
    /// Separate from [`ForbiddenImport`](Self::ForbiddenImport) because the
    /// two are satisfied by different work. A forbidden *import* is removed by
    /// editing the file the finding names; a forbidden *reach* usually is not,
    /// because the file that closes the chain may be several packages away and
    /// the fix is to cut an edge somewhere in the middle.
    ForbiddenReach {
        /// Glob patterns matched against every file reachable from this one.
        patterns: Vec<String>,
        /// Exceptions, also matched against the reached path.
        except: Vec<String>,
        /// Whether `import type` edges are followed.
        include_type_only: bool,
    },
    /// Imports of these packages are not allowed.
    ///
    /// Separate from [`ForbiddenImport`](Self::ForbiddenImport) because the
    /// thing matched is different in kind: a package name, not a repo-relative
    /// path. Under pnpm's store layout and yarn `PnP` a dependency has no path
    /// this repository could name, so a glob was never going to reach one.
    ForbiddenPackages {
        /// The package names, matched as "this package, and anything under it".
        packages: Vec<String>,
        /// Globs matched against the *importing* file, which is where an
        /// exception to a rule about a dependency naturally sits.
        except_from: Vec<String>,
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
    /// The file forwards other modules and adds nothing of its own.
    Passthrough {
        /// The exports that are forwards, in source order.
        exports: Vec<String>,
        /// Whether every export in the file is one.
        ///
        /// The difference between "this file adds nothing" and "part of this
        /// file adds nothing", which are different sentences and different
        /// decisions.
        whole_file: bool,
    },

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
    /// An export by the right name and kind, declaring no type at all.
    ///
    /// The state issue #39 is about: a declaration nothing submits to `tsc`,
    /// so nothing rejects it until the module is loaded.
    ExportMissingAnnotation {
        /// The name that was found.
        name: String,
    },
    /// An export annotated, but against another contract.
    ///
    /// Separate from [`Observed::ExportMissingAnnotation`] because the two are
    /// different sentences and different fixes — one is "write the type down",
    /// the other is "you wrote a different one" — the same reason
    /// [`Observed::ExportWrongKind`] is not [`Observed::ExportMissing`].
    ExportWrongAnnotation {
        /// The name that was found.
        name: String,
        /// The types the declaration does claim, as written. More than one for
        /// a class, which names a contract per `implements` clause.
        found: Vec<String>,
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
    /// A file the directory must hold is not there.
    ///
    /// The first observation about a path that does not exist. Every other one
    /// describes something a rule opened and disagreed with; this describes an
    /// absence, which is the failure nobody notices — nothing errors, nothing
    /// fails to build, and the gap is found by whoever needed the file.
    RequiredFileMissing {
        /// The name that was looked for.
        name: String,
    },
    /// No file in the directory matches a pattern one had to.
    ///
    /// Separate from [`Observed::RequiredFileMissing`] because there is no
    /// name to report: the requirement is a shape, and "create `\\.ino$`" is
    /// not an instruction anybody can follow.
    NoFileMatching {
        /// The pattern that found nothing, as written in the config.
        pattern: String,
    },
    /// The document has no frontmatter block at all.
    ///
    /// A finding rather than a skip: skipping would make *deleting the block*
    /// the way out of the rule, which is the argument `skip_type_only` already
    /// makes about deleting the `export` keyword.
    FrontmatterAbsent,
    /// There is a block and it is not a YAML mapping.
    ///
    /// Separate from [`Observed::FrontmatterAbsent`] because the next steps
    /// differ: one is "write the block", the other is "what you wrote is not
    /// YAML".
    FrontmatterMalformed {
        /// What the parser objected to.
        reason: String,
    },
    /// A key the block had to carry is not there.
    FrontmatterKeyMissing {
        /// The key that was looked for.
        key: String,
    },
    /// A key's value is outside the closed vocabulary the rule names.
    ///
    /// The confidently-wrong case, which is worse than an absence: a value
    /// outside the vocabulary drops the document out of whatever reads it, with
    /// no row and no error.
    FrontmatterValueOutsideVocabulary {
        /// The key.
        key: String,
        /// What was written there.
        found: String,
    },
    /// A key's value does not agree with what the path says it should be.
    FrontmatterValueDisagrees {
        /// The key.
        key: String,
        /// What was written there.
        found: String,
        /// What the path says it should be.
        wanted: String,
    },
    /// A key the rule asks a question about holds a list or a mapping.
    ///
    /// Distinct from being outside a vocabulary, because there is no value to
    /// compare: "fix the value" and "you wrote a list here" are different
    /// sentences.
    FrontmatterValueNotScalar {
        /// The key.
        key: String,
    },
    /// The companion this file needs is not there.
    CompanionMissing {
        /// The path that was looked for, resolved.
        path: RepoRelPath,
    },
    /// The required sibling is not there.
    SiblingMissing {
        /// The sibling that was looked for.
        path: RepoRelPath,
    },
    /// An import of a file this rule did not permit.
    ImportNotPermitted {
        /// The specifier, as written.
        specifier: String,
        /// Where it landed.
        resolved: RepoRelPath,
    },
    /// An import of a package this rule did not permit.
    PackageNotPermitted {
        /// The specifier, as written.
        specifier: String,
        /// The package it names.
        package: String,
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
    /// The file ends up depending on something the rule forbids, without
    /// importing it.
    ForbiddenReach {
        /// The chain, from this file to the forbidden destination.
        ///
        /// At least three entries, because a two-entry chain is a direct
        /// import and `ForbiddenImport` already reports that one. The middle
        /// of the chain is the actionable part: it names the edge that, cut,
        /// removes the dependency.
        chain: Vec<RepoRelPath>,
    },
    /// No rule governs the file.
    ///
    /// Carries nothing: the finding *is* the absence, and the path on the
    /// finding is the whole of what a reader needs. Anything else here would
    /// be a guess at which rule should have covered it.
    Ungoverned,

    /// The file sits on an import loop.
    ImportCycle {
        /// The loop, starting and ending at this file.
        ///
        /// Both ends are named so a reader can see that it closed: `a -> b ->
        /// a` is three entries, not two. The whole chain rather than the fact,
        /// because "this file is in a cycle" is not actionable and the chain
        /// names every edge that could be cut to break it.
        chain: Vec<RepoRelPath>,
    },
    /// An import of a package the rule forbids.
    ForbiddenPackageImport {
        /// The specifier as written, which may name a subpath.
        specifier: String,
        /// The forbidden package it names.
        package: String,
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
                annotation: Vec::new(),
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
                annotation: Vec::new(),
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
