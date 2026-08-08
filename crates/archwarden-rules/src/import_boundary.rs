//! The `import-boundary` rule: which layer may reach which.
//!
//! The first rule that is about a relationship rather than a file. Everything
//! before it can be decided from one file's name and contents; this one needs
//! to know where an import *lands*, which is why the resolution pass exists.
//!
//! Globs are matched against the resolved repo-relative path and never against
//! the specifier string. That is what makes `@/domain/user` and
//! `../../domain/user` the same edge -- there is exactly one canonical path per
//! import, and a rule is written against it.
//!
//! See `docs/RULES.md`.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    facts::ImportFact,
    finding::{Expectation, Finding, Observed},
    glob::PathSet,
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    scope::Scope,
    traits::{FileContext, RuleEngine},
};

/// A compiled `import-boundary` rule.
#[derive(Debug, Clone)]
pub struct ImportBoundaryEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    forbid: PathSet,
    require: PathSet,
    forbid_packages: Vec<String>,
    except: PathSet,
    except_from: PathSet,
    include_type_only: bool,
}

impl ImportBoundaryEngine {
    /// Builds an engine from a compiled rule.
    ///
    /// Returns `None` for a rule of any other kind.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::ImportBoundary {
            forbid,
            require,
            forbid_packages,
            except,
            except_from,
            include_type_only,
        } = &rule.kind
        else {
            return None;
        };

        Some(Self::build(
            rule,
            forbid,
            require,
            forbid_packages,
            except,
            except_from,
            *include_type_only,
        ))
    }

    /// Builds an engine from a rule whose kind is already known.
    ///
    /// Infallible, and that is the point: `engines_for` matches every
    /// `CompiledRuleKind` exhaustively and calls the matching constructor, so
    /// a kind added without an engine fails to compile. There is no runtime
    /// state in which a rule goes unchecked, which is why a run has nothing to
    /// report as unimplemented.
    pub(crate) fn build(
        rule: &CompiledRule,
        forbid: &PathSet,
        require: &PathSet,
        forbid_packages: &[String],
        except: &PathSet,
        except_from: &PathSet,
        include_type_only: bool,
    ) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            forbid: forbid.clone(),
            require: require.clone(),
            forbid_packages: forbid_packages.to_vec(),
            except: except.clone(),
            except_from: except_from.clone(),
            include_type_only,
        }
    }

    /// Whether this import is one the rule looks at.
    ///
    /// A type-only import is invisible when the rule opted out of them, and an
    /// import that did not resolve to a file in this repository has no path a
    /// glob could match.
    fn visible<'a>(&self, import: &'a ImportFact) -> Option<&'a RepoRelPath> {
        if import.type_only && !self.include_type_only {
            return None;
        }
        import.resolved.as_ref()
    }

    /// Whether `resolved` is one this rule forbids.
    ///
    /// `except` only shields against `forbid`. A rule that both requires and
    /// forbids reads as "must reach A, must not reach B, and here are the
    /// corners of B that are allowed" -- an exception to a requirement would
    /// be a requirement nobody has to meet.
    fn is_forbidden(&self, resolved: &RepoRelPath) -> bool {
        self.forbid.is_match(resolved.as_path()) && !self.except.is_match(resolved.as_path())
    }

    /// Which forbidden package this specifier names, if any.
    ///
    /// Matched as "the package, and anything under it", so a rule naming
    /// `three` catches `three/examples/jsm/loaders/GLTFLoader.js` — otherwise
    /// the deep import, which is the one that actually costs the bytes, would
    /// be the one that sails past.
    ///
    /// `node:fs` and `fs` are the same module, so both spellings are stripped
    /// on both sides and either one in the config matches either one in the
    /// source.
    ///
    /// A relative specifier is never a package, whatever it is called.
    fn forbidden_package(&self, specifier: &str) -> Option<&str> {
        if specifier.starts_with('.') {
            return None;
        }
        let bare = specifier.strip_prefix("node:").unwrap_or(specifier);

        self.forbid_packages
            .iter()
            .find(|package| {
                let package = package.strip_prefix("node:").unwrap_or(package);
                bare.strip_prefix(package)
                    .is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
            })
            .map(String::as_str)
    }

    fn forbidden_packages_expectation(&self) -> Expectation {
        Expectation::ForbiddenPackages {
            packages: self.forbid_packages.clone(),
            except_from: self.except_from.patterns().to_vec(),
            include_type_only: self.include_type_only,
        }
    }

    fn forbidden_expectation(&self) -> Expectation {
        Expectation::ForbiddenImport {
            patterns: self.forbid.patterns().to_vec(),
            except: self.except.patterns().to_vec(),
            include_type_only: self.include_type_only,
        }
    }

    fn required_expectation(&self) -> Expectation {
        Expectation::RequiredImport {
            patterns: self.require.patterns().to_vec(),
        }
    }

    fn finding(&self, path: &RepoRelPath, observed: Observed, expected: Expectation) -> Finding {
        Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: path.clone(),
            span: None,
            observed,
            expected,
        }
    }
}

impl RuleEngine for ImportBoundaryEngine {
    fn id(&self) -> &RuleId {
        &self.id
    }

    fn module(&self) -> Option<&ModuleId> {
        self.module.as_ref()
    }

    fn level(&self) -> Level {
        self.level
    }

    fn applies_to(&self, path: &RepoRelPath) -> bool {
        self.scope.contains_file(path.as_path())
    }

    fn needs_facts(&self) -> bool {
        true
    }

    fn needs_resolution(&self) -> bool {
        true
    }

    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        if !self.applies_to(ctx.path) {
            return Vec::new();
        }
        // No facts means no parser ran. Reporting "no import satisfies the
        // requirement" would be blaming the file for the run's own gap.
        let Some(facts) = ctx.facts else {
            return Vec::new();
        };

        // The importer is exempt from the whole rule. `except` is about what is
        // imported and cannot express this: "only `src/scripts/three/**` may
        // import `three`" is one forbid and one exempt importer, and the
        // exemption is on the side that does the importing.
        if self.except_from.is_match(ctx.path.as_path()) {
            return Vec::new();
        }

        let mut findings = Vec::new();
        let mut satisfies_requirement = false;

        for import in &facts.imports {
            if import.type_only && !self.include_type_only {
                continue;
            }

            // A package rule and a path rule ask about different imports, and
            // never about the same one. An import that landed on a file here is
            // a path — whatever its specifier looks like, including a
            // `tsconfig` alias that spells a local shim `three`. Everything
            // else names a package: a dependency, a builtin, or one that did
            // not resolve, which on a repository before `install` is most of
            // them and is exactly where this rule still has to work.
            if import.resolved.is_none()
                && let Some(package) = self.forbidden_package(&import.specifier)
            {
                findings.push(Finding {
                    span: Some(import.span),
                    ..self.finding(
                        ctx.path,
                        Observed::ForbiddenPackageImport {
                            specifier: import.specifier.clone(),
                            package: package.to_owned(),
                        },
                        self.forbidden_packages_expectation(),
                    )
                });
            }

            let Some(resolved) = self.visible(import) else {
                continue;
            };

            if self.is_forbidden(resolved) {
                findings.push(Finding {
                    span: Some(import.span),
                    ..self.finding(
                        ctx.path,
                        Observed::ForbiddenImport {
                            specifier: import.specifier.clone(),
                            resolved: resolved.clone(),
                        },
                        self.forbidden_expectation(),
                    )
                });
            }

            satisfies_requirement |= self.require.is_match(resolved.as_path());
        }

        // A rule with no `must_import_from` requires nothing, and an empty
        // `PathSet` matches nothing -- so the flag would be false for every
        // file and every file would be reported.
        if !self.require.is_empty() && !satisfies_requirement {
            findings.push(self.finding(
                ctx.path,
                Observed::RequiredImportMissing,
                self.required_expectation(),
            ));
        }

        findings
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        // Same exemption `check_file` applies, and it has to be the same or
        // `describe` would tell an agent about a rule that will not fire —
        // which is the write-fail-retry loop the command exists to avoid.
        if !self.applies_to(path) || self.except_from.is_match(path.as_path()) {
            return Vec::new();
        }

        let mut expectations = Vec::new();
        if !self.forbid.is_empty() {
            expectations.push(self.forbidden_expectation());
        }
        if !self.forbid_packages.is_empty() {
            expectations.push(self.forbidden_packages_expectation());
        }
        if !self.require.is_empty() {
            expectations.push(self.required_expectation());
        }
        expectations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        facts::{FileFacts, Span},
        hash::ContentHash,
    };

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn set(patterns: &[&str]) -> PathSet {
        PathSet::compile(patterns.iter().map(|p| (*p).to_owned())).expect("valid globs")
    }

    /// A rule over the whole `packages/ui` subtree, configured by the caller.
    ///
    /// `**` and not `*`: a scope glob selects *directories* (decision 4), so
    /// `packages/ui/*` would cover `packages/ui/button` but not `packages/ui`
    /// itself. A boundary is about a package, not about one level of it.
    fn rule(forbid: &[&str], require: &[&str], except: &[&str], type_only: bool) -> CompiledRule {
        CompiledRule {
            id: RuleId::new("ui-forbids-domain").expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(["packages/ui/**"]).expect("valid scope"),
            kind: CompiledRuleKind::ImportBoundary {
                forbid: set(forbid),
                require: set(require),
                forbid_packages: Vec::new(),
                except: set(except),
                except_from: PathSet::default(),
                include_type_only: type_only,
            },
        }
    }

    fn engine(
        forbid: &[&str],
        require: &[&str],
        except: &[&str],
        type_only: bool,
    ) -> ImportBoundaryEngine {
        ImportBoundaryEngine::from_rule(&rule(forbid, require, except, type_only))
            .expect("an import-boundary rule")
    }

    /// Facts for a file whose imports are `(specifier, resolved, type_only)`.
    /// `resolved` of `None` stands for a dependency, a builtin, or an import
    /// that did not resolve -- all three of which have no repo path.
    fn facts(file: &str, imports: &[(&str, Option<&str>, bool)]) -> FileFacts {
        let mut facts = FileFacts::unparsed(path(file), ContentHash::of(b"source"));
        for (offset, (specifier, resolved, type_only)) in imports.iter().enumerate() {
            let start = u32::try_from(offset).expect("few imports") * 100;
            facts.imports.push(ImportFact {
                specifier: (*specifier).to_owned(),
                resolved: resolved.map(path),
                type_only: *type_only,
                names: Vec::new(),
                span: Span::new(start, start + 40),
            });
        }
        facts
    }

    fn check(engine: &ImportBoundaryEngine, facts: &FileFacts) -> Vec<Finding> {
        engine.check_file(FileContext {
            path: &facts.path,
            facts: Some(facts),
            siblings: &[],
        })
    }

    /// The rule the whole feature exists for: a UI file reaching into domain.
    #[test]
    fn a_forbidden_import_is_reported_with_both_the_specifier_and_the_path() {
        let engine = engine(&["packages/domain/**"], &[], &[], true);
        let findings = check(
            &engine,
            &facts(
                "packages/ui/button/button.tsx",
                &[(
                    "@/domain/user",
                    Some("packages/domain/src/user/user.entity.ts"),
                    false,
                )],
            ),
        );

        assert_eq!(findings.len(), 1);
        let finding = findings.first().expect("one finding");
        assert_eq!(
            finding.observed,
            Observed::ForbiddenImport {
                specifier: "@/domain/user".to_owned(),
                resolved: path("packages/domain/src/user/user.entity.ts"),
            }
        );
        assert_eq!(
            finding.expected,
            Expectation::ForbiddenImport {
                patterns: vec!["packages/domain/**".to_owned()],
                except: Vec::new(),
                include_type_only: true,
            }
        );
        assert_eq!(finding.path, path("packages/ui/button/button.tsx"));
        assert_eq!(finding.level, Level::Error);
    }

    /// Both halves of the finding matter: the specifier is what the user has
    /// to go and delete, and the resolved path is why the rule fired. An alias
    /// makes them look nothing alike.
    #[test]
    fn the_specifier_is_reported_as_written_not_as_resolved() {
        let engine = engine(&["packages/domain/**"], &[], &[], true);
        let findings = check(
            &engine,
            &facts(
                "packages/ui/a.ts",
                &[("@/domain/user", Some("packages/domain/src/user.ts"), false)],
            ),
        );

        assert!(matches!(
            findings.first().map(|f| &f.observed),
            Some(Observed::ForbiddenImport { specifier, .. }) if specifier == "@/domain/user"
        ));
    }

    /// The finding points at the import statement, which is the one thing in
    /// the file the user has to look at.
    #[test]
    fn a_forbidden_import_carries_the_span_of_its_statement() {
        let engine = engine(&["packages/domain/**"], &[], &[], true);
        let findings = check(
            &engine,
            &facts(
                "packages/ui/a.ts",
                &[
                    ("./sibling", Some("packages/ui/sibling.ts"), false),
                    ("@/domain/user", Some("packages/domain/src/user.ts"), false),
                ],
            ),
        );

        assert_eq!(
            findings.first().and_then(|f| f.span),
            Some(Span::new(100, 140)),
            "the second import, not the first"
        );
    }

    /// An import that lands outside the forbidden set is simply fine.
    #[test]
    fn an_allowed_import_produces_nothing() {
        let engine = engine(&["packages/domain/**"], &[], &[], true);
        let findings = check(
            &engine,
            &facts(
                "packages/ui/a.ts",
                &[("./sibling", Some("packages/ui/sibling.ts"), false)],
            ),
        );

        assert!(findings.is_empty());
    }

    /// The documented use of `except`: the layer is closed, but its types are
    /// open. Without this, every boundary rule would be all-or-nothing.
    #[test]
    fn an_exception_shields_a_corner_of_the_forbidden_set() {
        let engine = engine(
            &["packages/domain/**"],
            &[],
            &["packages/domain/src/*/types/**"],
            true,
        );
        let findings = check(
            &engine,
            &facts(
                "packages/ui/a.ts",
                &[
                    (
                        "@/domain/user/types",
                        Some("packages/domain/src/user/types/user.dto.ts"),
                        false,
                    ),
                    (
                        "@/domain/user",
                        Some("packages/domain/src/user/user.entity.ts"),
                        false,
                    ),
                ],
            ),
        );

        assert_eq!(findings.len(), 1, "only the entity, not the type");
        assert!(matches!(
            findings.first().map(|f| &f.observed),
            Some(Observed::ForbiddenImport { resolved, .. })
                if resolved.as_str().ends_with("user.entity.ts")
        ));
    }

    /// The exception is part of what the user is told, so they can see why one
    /// import fired and another did not.
    #[test]
    fn the_exception_appears_in_the_expectation() {
        let engine = engine(
            &["packages/domain/**"],
            &[],
            &["packages/domain/src/*/types/**"],
            true,
        );

        assert_eq!(
            engine.describe_expectation(&path("packages/ui/a.ts")),
            vec![Expectation::ForbiddenImport {
                patterns: vec!["packages/domain/**".to_owned()],
                except: vec!["packages/domain/src/*/types/**".to_owned()],
                include_type_only: true,
            }]
        );
    }

    /// `include_type_only: false` is how a rule says "a type dependency is not
    /// a real dependency", which is the usual reading in TypeScript.
    #[test]
    fn a_type_only_import_is_invisible_when_the_rule_opted_out() {
        let file = facts(
            "packages/ui/a.ts",
            &[("@/domain/user", Some("packages/domain/src/user.ts"), true)],
        );

        assert!(
            check(&engine(&["packages/domain/**"], &[], &[], false), &file).is_empty(),
            "opted out"
        );
        assert_eq!(
            check(&engine(&["packages/domain/**"], &[], &[], true), &file).len(),
            1,
            "opted in, which is the default"
        );
    }

    /// The other direction: a file in scope that reaches nothing it must.
    #[test]
    fn a_missing_required_import_is_reported() {
        let engine = engine(&[], &["packages/telemetry/**"], &[], true);
        let findings = check(
            &engine,
            &facts(
                "packages/ui/a.ts",
                &[("./sibling", Some("packages/ui/sibling.ts"), false)],
            ),
        );

        assert_eq!(findings.len(), 1);
        let finding = findings.first().expect("one finding");
        assert_eq!(finding.observed, Observed::RequiredImportMissing);
        assert_eq!(
            finding.expected,
            Expectation::RequiredImport {
                patterns: vec!["packages/telemetry/**".to_owned()],
            }
        );
        assert_eq!(finding.span, None, "nothing in the file to point at");
    }

    /// One import satisfying the requirement is enough, whichever it is.
    #[test]
    fn one_satisfying_import_is_enough() {
        let engine = engine(&[], &["packages/telemetry/**"], &[], true);
        let findings = check(
            &engine,
            &facts(
                "packages/ui/a.ts",
                &[
                    ("./sibling", Some("packages/ui/sibling.ts"), false),
                    (
                        "@/telemetry",
                        Some("packages/telemetry/src/index.ts"),
                        false,
                    ),
                ],
            ),
        );

        assert!(findings.is_empty());
    }

    /// A rule with no `must_import_from` requires nothing. The empty glob set
    /// matches nothing, so a naive check would report every file in scope.
    #[test]
    fn a_rule_without_a_requirement_never_reports_one_missing() {
        let engine = engine(&["packages/domain/**"], &[], &[], true);
        let findings = check(
            &engine,
            &facts(
                "packages/ui/a.ts",
                &[("./x", Some("packages/ui/x.ts"), false)],
            ),
        );

        assert!(findings.is_empty());
        assert_eq!(
            engine.describe_expectation(&path("packages/ui/a.ts")).len(),
            1,
            "and it says only the one thing it enforces"
        );
    }

    /// Both directions in one rule, both reported.
    #[test]
    fn a_rule_can_forbid_and_require_at_once() {
        let engine = engine(
            &["packages/domain/**"],
            &["packages/telemetry/**"],
            &[],
            true,
        );
        let findings = check(
            &engine,
            &facts(
                "packages/ui/a.ts",
                &[("@/domain/user", Some("packages/domain/src/user.ts"), false)],
            ),
        );

        assert_eq!(findings.len(), 2);
        assert_eq!(
            engine.describe_expectation(&path("packages/ui/a.ts")).len(),
            2
        );
    }

    /// A dependency, a builtin and an unresolvable specifier all arrive with
    /// no path. A rule whose globs are repo-relative has nothing to say about
    /// any of them, and must not guess from the specifier string.
    #[test]
    fn an_import_with_no_repository_path_is_not_matched() {
        let engine = engine(&["**/domain/**"], &[], &[], true);
        let findings = check(
            &engine,
            &facts(
                "packages/ui/a.ts",
                &[
                    ("lodash", None, false),
                    ("node:fs", None, false),
                    ("@org/domain", None, false),
                ],
            ),
        );

        assert!(findings.is_empty());
    }

    /// Scope is the importer's directory, so a file outside it is not the
    /// rule's business however it imports.
    #[test]
    fn a_file_outside_the_scope_is_not_checked() {
        let engine = engine(&["packages/domain/**"], &[], &[], true);
        let outsider = facts(
            "packages/api/a.ts",
            &[("@/domain/user", Some("packages/domain/src/user.ts"), false)],
        );

        assert!(!engine.applies_to(&outsider.path));
        assert!(check(&engine, &outsider).is_empty());
        assert!(
            engine
                .describe_expectation(&path("packages/api/a.ts"))
                .is_empty()
        );
    }

    /// A file the parser never read has no imports to judge. Reporting a
    /// missing required import would blame the file for the run's own gap.
    #[test]
    fn a_file_without_facts_is_not_judged() {
        let engine = engine(&[], &["packages/telemetry/**"], &[], true);
        let findings = engine.check_file(FileContext {
            path: &path("packages/ui/a.ts"),
            facts: None,
            siblings: &[],
        });

        assert!(findings.is_empty());
    }

    /// The rule reads inside a file *and* needs to know where its imports go.
    /// Both flags exist because they cost different things.
    #[test]
    fn the_rule_declares_both_costs() {
        let engine = engine(&["packages/domain/**"], &[], &[], true);

        assert!(engine.needs_facts());
        assert!(engine.needs_resolution());
        assert_eq!(engine.id().as_str(), "ui-forbids-domain");
        assert_eq!(engine.module(), None);
        assert_eq!(engine.level(), Level::Error);
    }

    /// A boundary usually lives in the top-level `rules`, because "domain must
    /// not import application" belongs to neither layer. It may still be
    /// declared under a module, and then the module has to reach the finding:
    /// it is the `[domain]` a reader sees in the report.
    #[test]
    fn a_boundary_declared_under_a_module_carries_it_into_the_finding() {
        let module = ModuleId::new("ui").expect("valid module");
        let engine = ImportBoundaryEngine::from_rule(&CompiledRule {
            module: Some(module.clone()),
            ..rule(&["packages/domain/**"], &[], &[], true)
        })
        .expect("an import-boundary rule");

        assert_eq!(engine.module(), Some(&module));

        let findings = check(
            &engine,
            &facts(
                "packages/ui/a.ts",
                &[("@/domain/user", Some("packages/domain/src/user.ts"), false)],
            ),
        );
        assert_eq!(
            findings.first().and_then(|f| f.module_id.as_ref()),
            Some(&module)
        );
    }

    /// A rule of another kind is not this engine's.
    #[test]
    fn a_rule_of_another_kind_builds_nothing() {
        let structure = CompiledRule {
            id: RuleId::new("shape").expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(["packages/ui/**"]).expect("valid scope"),
            kind: CompiledRuleKind::Structure {
                allowed_subfolders: Some(Vec::new()),
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: Vec::new(),
                filename_patterns: Vec::new(),
            },
        };

        assert!(ImportBoundaryEngine::from_rule(&structure).is_none());
    }

    /// A rule that quarantines a dependency: `three` may be imported from one
    /// directory and nowhere else.
    fn quarantine(packages: &[&str], except_from: &[&str]) -> ImportBoundaryEngine {
        let rule = CompiledRule {
            id: RuleId::new("three-is-quarantined").expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(["src/**"]).expect("valid scope"),
            kind: CompiledRuleKind::ImportBoundary {
                forbid: PathSet::default(),
                require: PathSet::default(),
                forbid_packages: packages.iter().map(|p| (*p).to_owned()).collect(),
                except: PathSet::default(),
                except_from: PathSet::compile(
                    except_from
                        .iter()
                        .map(|p| (*p).to_owned())
                        .collect::<Vec<_>>(),
                )
                .expect("valid globs"),
                include_type_only: true,
            },
        };
        ImportBoundaryEngine::from_rule(&rule).expect("an import-boundary rule")
    }

    /// Issue #14's case. Violating it is silent — nothing breaks, no test
    /// fails, the page just gets slower and it is found weeks later in a
    /// Lighthouse report — which is exactly the kind of rule archwarden is for.
    #[test]
    fn an_import_of_a_forbidden_package_is_reported() {
        let engine = quarantine(&["three"], &["src/scripts/three/**"]);
        let findings = check(
            &engine,
            &facts("src/pages/home.ts", &[("three", None, false)]),
        );

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].observed,
            Observed::ForbiddenPackageImport {
                specifier: "three".to_owned(),
                package: "three".to_owned(),
            }
        );
        assert_eq!(
            findings[0].span,
            Some(Span::new(0, 40)),
            "the finding carries the import's span, or the caret has nothing to \
             point at and the reader has to search the file"
        );
    }

    /// The deep import is the one that actually costs the bytes, so it is the
    /// one that must not sail past. A rule naming `three` covers everything
    /// under it.
    #[test]
    fn a_subpath_of_a_forbidden_package_is_the_same_package() {
        let engine = quarantine(&["three"], &[]);
        let findings = check(
            &engine,
            &facts(
                "src/pages/home.ts",
                &[("three/examples/jsm/loaders/GLTFLoader.js", None, false)],
            ),
        );

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].observed,
            Observed::ForbiddenPackageImport {
                specifier: "three/examples/jsm/loaders/GLTFLoader.js".to_owned(),
                package: "three".to_owned(),
            }
        );
    }

    /// And a package whose name merely starts the same way is a different
    /// package. Without the boundary test, `three-mesh-bvh` would be caught by
    /// a rule about `three`, and a rule that reports things nobody asked about
    /// is one that gets deleted.
    #[test]
    fn a_package_that_only_shares_a_prefix_is_left_alone() {
        let engine = quarantine(&["three"], &[]);

        assert!(
            check(
                &engine,
                &facts("src/pages/home.ts", &[("three-mesh-bvh", None, false)])
            )
            .is_empty()
        );
    }

    /// The exemption is on the importing side, which is where an exception to a
    /// rule about a dependency naturally sits: one forbid, one directory that
    /// may.
    #[test]
    fn the_directory_allowed_to_import_it_is_exempt() {
        let engine = quarantine(&["three"], &["src/scripts/three/**"]);

        assert!(
            check(
                &engine,
                &facts("src/scripts/three/scene.ts", &[("three", None, false)])
            )
            .is_empty(),
            "the quarantine directory is the one place it is allowed"
        );
        assert!(
            engine
                .describe_expectation(&path("src/scripts/three/scene.ts"))
                .is_empty(),
            "and `describe` says the same, or an agent is told about a rule \
             that will not fire"
        );
    }

    /// `node:fs` and `fs` are the same module, so either spelling in the config
    /// matches either spelling in the source. Four combinations, all of them
    /// the same import.
    #[test]
    fn a_builtin_matches_whichever_way_either_side_spells_it() {
        for configured in ["fs", "node:fs"] {
            for written in ["fs", "node:fs"] {
                let engine = quarantine(&[configured], &[]);
                assert_eq!(
                    check(&engine, &facts("src/lib/a.ts", &[(written, None, false)])).len(),
                    1,
                    "config `{configured}` should match source `{written}`"
                );
            }
        }
    }

    /// An import that landed on a file in this repository is a path, whatever
    /// its specifier looks like. A `tsconfig` alias spelling a local shim
    /// `three` is the case: `forbid_import_from` is the field for it, and
    /// reporting it here would be the rule firing on the wrong thing.
    #[test]
    fn a_specifier_that_resolved_into_the_repository_is_a_path_not_a_package() {
        let engine = quarantine(&["three"], &[]);

        assert!(
            check(
                &engine,
                &facts(
                    "src/pages/home.ts",
                    &[("three", Some("src/shims/three.ts"), false)]
                )
            )
            .is_empty()
        );
    }

    /// A relative specifier is never a package, however it is named.
    #[test]
    fn a_relative_specifier_is_never_a_package() {
        let engine = quarantine(&["three"], &[]);

        assert!(
            check(
                &engine,
                &facts("src/pages/home.ts", &[("./three", None, false)])
            )
            .is_empty()
        );
    }

    /// The rule holds on a repository whose dependencies are not installed,
    /// which is the state a CI job that lints before installing is in. An
    /// unresolved import is exactly what this rule reads, so nothing is lost.
    #[test]
    fn the_rule_still_fires_when_nothing_resolves() {
        let engine = quarantine(&["three"], &["src/scripts/three/**"]);
        let findings = check(
            &engine,
            &facts(
                "src/pages/home.ts",
                &[("three", None, false), ("react", None, false)],
            ),
        );

        assert_eq!(findings.len(), 1, "`react` is not forbidden: {findings:?}");
    }

    /// `include_type_only` governs this half too. `import type { Vector3 }`
    /// costs no bytes at runtime, which is the whole reason a bundle-budget
    /// rule would opt out of them.
    #[test]
    fn a_type_only_import_obeys_the_same_opt_out() {
        let mut rule = quarantine(&["three"], &[]);
        let type_only = facts("src/pages/home.ts", &[("three", None, true)]);

        assert_eq!(check(&rule, &type_only).len(), 1, "counted by default");

        rule.include_type_only = false;
        assert!(
            check(&rule, &type_only).is_empty(),
            "and exempt when the rule opted out"
        );
    }
}
