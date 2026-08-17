//! The `no-passthrough` rule: a file that adds nothing.
//!
//! Three shapes, all of them a way of holding a name and adding nothing to it:
//!
//! 1. **Re-export** — `export { A } from './x'`, or an import followed by an
//!    export of the same name. A barrel file is this and nothing else.
//! 2. **Alias** — `export const planToJson = planToJsonShared`, or
//!    `export type PlanJson = PlanJsonShared`. The name changed; nothing else
//!    did.
//! 3. **Wrapper** — a function whose whole body is `return g(a, b)` with its
//!    own parameters, in order.
//!
//! # Why it earns a rule
//!
//! These three are how a folder survives years looking like it has a purpose.
//! An importer reaching a `shared/` module through a `calcs/` file that only
//! forwards it cannot tell that the layer between them is empty, and neither
//! can a reader. A rule that says "this file adds nothing" ends the category —
//! and it is the only enforcement a "no barrel files" line in a `CLAUDE.md`
//! has ever had, since a barrel is case 1 with no exceptions.
//!
//! # Where it stays file-local
//!
//! "Same signature" in the type sense would need the file on the other side
//! and its types. `docs/RULES.md` section 2 keeps a file-local rule away from
//! following a re-export for exactly that reason, and this stays on the same
//! side of the line: the wrapper test is *syntactic*. A wrapper that reorders
//! arguments, drops one, or supplies a default is doing something, and none of
//! those match.
//!
//! # Why the exceptions are not optional
//!
//! Legitimate forwarding exists, and it is not rare: the file a package's
//! `exports` points at is a public API, and forwarding is what a public API is
//! for. `allow_package_entrypoints` covers that without anyone writing a glob,
//! because a rule that reported a package's entire surface the day it was
//! switched on is a rule nobody leaves on.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind, PassthroughForms},
    facts::{ExportKind, FileFacts},
    finding::{Expectation, Finding, Observed},
    glob::PathSet,
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    scope::Scope,
    traits::{FactsNeeded, FileContext, RuleEngine},
};

/// A compiled `no-passthrough` rule.
#[derive(Debug, Clone)]
pub struct NoPassthroughEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    forms: PassthroughForms,
    except: PathSet,
    allow_package_entrypoints: bool,
    allow_partial: bool,
}

impl NoPassthroughEngine {
    /// Builds an engine from a compiled rule.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::NoPassthrough {
            forms,
            except,
            allow_package_entrypoints,
            allow_partial,
        } = &rule.kind
        else {
            return None;
        };
        Some(Self::build(
            rule,
            *forms,
            except,
            *allow_package_entrypoints,
            *allow_partial,
        ))
    }

    /// Builds an engine from a rule whose kind is already known.
    pub(crate) fn build(
        rule: &CompiledRule,
        forms: PassthroughForms,
        except: &PathSet,
        allow_package_entrypoints: bool,
        allow_partial: bool,
    ) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            forms,
            except: except.clone(),
            allow_package_entrypoints,
            allow_partial,
        }
    }

    /// Which of a file's exports forward an imported binding, and by which
    /// shape.
    ///
    /// The parser records *that* an export forwards a local name. Deciding
    /// whether that name came from an import needs the file's imports, which
    /// is why the two halves meet here rather than in the parser.
    fn forwarding<'a>(&self, facts: &'a FileFacts) -> Vec<&'a str> {
        let imported: std::collections::BTreeSet<&str> = facts
            .imports
            .iter()
            .flat_map(|import| import.names.iter().map(String::as_str))
            .collect();

        facts
            .exports
            .iter()
            .filter(|export| {
                let Some(forwarded) = export.forwards.as_deref() else {
                    return false;
                };

                // `export { A } from './x'` never binds anything locally, so
                // the import set says nothing about it. It is a re-export by
                // construction.
                let is_indirect = export.reexport_from.is_some();
                if !is_indirect && !imported.contains(forwarded) {
                    return false;
                }

                if is_indirect || export.tags.contains(ExportKind::Reexport) {
                    self.forms.reexport
                } else if export.tags.contains(ExportKind::Function) {
                    self.forms.wrapper
                } else if export.tags.is_empty() {
                    // `export { A }` after `import { A }`: no local
                    // declaration, so no tags. A re-export written the long
                    // way, which is the shape the majority of real ones take.
                    self.forms.reexport
                } else {
                    self.forms.alias
                }
            })
            .filter_map(|export| export.name.as_deref())
            .collect()
    }
}

impl RuleEngine for NoPassthroughEngine {
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
        let Some(parent) = path.parent() else {
            return false;
        };
        if !self.scope.matches_dir(parent.as_path()) {
            return false;
        }
        if self.except.is_match(path.as_path()) {
            return false;
        }
        // A package entry point is a public API, and forwarding is what a
        // public API is for. Recognised by shape rather than by reading every
        // manifest here, because a rule engine consumes facts and does not
        // read the filesystem -- `config doctor` is where a manifest is read.
        !(self.allow_package_entrypoints && is_entry_point(path))
    }

    fn needs_facts(&self) -> FactsNeeded {
        FactsNeeded::Code
    }

    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        if !self.applies_to(ctx.path) {
            return Vec::new();
        }
        let Some(facts) = ctx.facts else {
            return Vec::new();
        };
        if facts.exports.is_empty() {
            return Vec::new();
        }

        let forwarding = self.forwarding(facts);
        if forwarding.is_empty() {
            return Vec::new();
        }

        // Whole-file by default. A file that forwards some of its exports and
        // declares others is a real module with an indirection inside it, and
        // reporting that as "adds nothing" would be false -- so it is opt-in.
        let whole_file = forwarding.len() == facts.exports.len();
        if !whole_file && self.allow_partial {
            return Vec::new();
        }

        let names: Vec<String> = forwarding.into_iter().map(ToOwned::to_owned).collect();
        vec![Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: ctx.path.clone(),
            span: None,
            observed: Observed::Passthrough {
                exports: names.clone(),
                whole_file,
            },
            expected: Expectation::NoPassthrough {
                forms: self.form_names(),
            },
        }]
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        if !self.applies_to(path) {
            return Vec::new();
        }
        vec![Expectation::NoPassthrough {
            forms: self.form_names(),
        }]
    }
}

impl NoPassthroughEngine {
    fn form_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        if self.forms.reexport {
            names.push("reexport".to_owned());
        }
        if self.forms.alias {
            names.push("alias".to_owned());
        }
        if self.forms.wrapper {
            names.push("wrapper".to_owned());
        }
        names
    }
}

/// Whether a path is the shape a package points its `exports` at.
///
/// An `index` file, or a file directly under the package root. Both are what a
/// manifest names, and neither is what a rule about indirection inside a layer
/// is aimed at.
fn is_entry_point(path: &RepoRelPath) -> bool {
    path.file_name().is_some_and(|name| {
        matches!(
            name,
            "index.ts" | "index.tsx" | "index.js" | "index.jsx" | "index.mts" | "index.cts"
        )
    })
}

#[cfg(test)]
mod tests {
    use archwarden_core::traits::Exists;

    use super::*;
    use archwarden_core::facts::{ExportFact, ExportTags, ImportFact, Span};

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn engine(forms: PassthroughForms, allow_partial: bool) -> NoPassthroughEngine {
        let rule = CompiledRule {
            id: RuleId::new("no-indirection").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            level: Level::Warning,
            scope: Scope::compile(["packages/domain/src/**"]).expect("valid scope"),
            kind: CompiledRuleKind::NoPassthrough {
                forms,
                except: PathSet::default(),
                allow_package_entrypoints: true,
                allow_partial,
            },
        };
        NoPassthroughEngine::from_rule(&rule).expect("is a no-passthrough rule")
    }

    fn all_forms() -> PassthroughForms {
        PassthroughForms {
            reexport: true,
            alias: true,
            wrapper: true,
        }
    }

    fn facts(path_str: &str, imports: &[&str], exports: Vec<ExportFact>) -> FileFacts {
        FileFacts {
            path: path(path_str),
            content_hash: archwarden_core::hash::ContentHash::of(b""),
            imports: vec![ImportFact {
                specifier: "../shared/x".to_owned(),
                resolved: None,
                type_only: false,
                names: imports.iter().map(|n| (*n).to_owned()).collect(),
                span: Span::new(0, 0),
            }],
            exports,
            calls: Vec::new(),
            allowances: Vec::new(),
            metadata: Vec::new(),
            has_opaque_import: false,
        }
    }

    fn export(name: &str, tags: ExportTags, forwards: Option<&str>) -> ExportFact {
        ExportFact {
            name: Some(name.to_owned()),
            tags,
            is_default: false,
            reexport_from: None,
            forwards: forwards.map(ToOwned::to_owned),
            annotations: Vec::new(),
            returns: None,
            span: Span::new(0, 0),
        }
    }

    fn check(engine: &NoPassthroughEngine, facts: &FileFacts) -> Vec<Finding> {
        engine.check_file(FileContext {
            path: &facts.path,
            facts: Some(facts),
            docs: None,
            siblings: &[],
            exists: Exists::none(),
            graph: None,
        })
    }

    /// Form 2, verbatim from a real repository:
    ///
    /// ```ts
    /// export type PlanJson = PlanJsonShared;
    /// export const planToJson = planToJsonShared;
    /// ```
    #[test]
    fn a_file_that_only_aliases_imported_names_is_reported() {
        let facts = facts(
            "packages/domain/src/plan/calcs/to-json.ts",
            &["PlanJsonShared", "planToJsonShared"],
            vec![
                export(
                    "PlanJson",
                    ExportTags::only(ExportKind::Type),
                    Some("PlanJsonShared"),
                ),
                export(
                    "planToJson",
                    ExportTags::only(ExportKind::Const),
                    Some("planToJsonShared"),
                ),
            ],
        );

        let findings = check(&engine(all_forms(), true), &facts);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(
            findings[0].observed,
            Observed::Passthrough {
                exports: vec!["PlanJson".to_owned(), "planToJson".to_owned()],
                whole_file: true,
            }
        );
    }

    /// Form 3, also verbatim: a one-line wrapper with the arguments passed
    /// straight through.
    #[test]
    fn a_wrapper_forwarding_its_own_parameters_is_reported() {
        let facts = facts(
            "packages/domain/src/flow/calcs/validate-flow-graph.ts",
            &["isFlowGraphInvalidShared"],
            vec![export(
                "isFlowGraphInvalid",
                ExportTags::only(ExportKind::Function),
                Some("isFlowGraphInvalidShared"),
            )],
        );

        assert_eq!(check(&engine(all_forms(), true), &facts).len(), 1);
    }

    /// A form the configuration turned off is not reported, which is what
    /// makes "no barrel files" expressible without also banning aliases.
    #[test]
    fn a_form_the_config_did_not_ask_for_is_not_reported() {
        let facts = facts(
            "packages/domain/src/plan/calcs/to-json.ts",
            &["planToJsonShared"],
            vec![export(
                "planToJson",
                ExportTags::only(ExportKind::Const),
                Some("planToJsonShared"),
            )],
        );

        let reexports_only = PassthroughForms {
            reexport: true,
            alias: false,
            wrapper: false,
        };
        assert!(check(&engine(reexports_only, true), &facts).is_empty());
    }

    /// `export * from './x'` is the barrel this rule exists for, and it was
    /// invisible until 0.22: the parser produced no fact at all, so the rule
    /// against a file that adds nothing of its own said nothing about the
    /// loudest form of exactly that. Issue #101.
    ///
    /// **This changes what an existing, unchanged config reports.** A
    /// repository with `no-passthrough` and star barrels gets findings on its
    /// first 0.22 run that 0.21 never produced. That is the defect being
    /// fixed, and `baseline` is the answer for anyone not paying the debt
    /// today.
    #[test]
    fn a_star_reexport_is_a_passthrough() {
        let facts = facts(
            "packages/domain/src/plan/barrel.ts",
            &[],
            vec![ExportFact {
                name: Some("*".to_owned()),
                tags: ExportTags::only(ExportKind::Reexport),
                is_default: false,
                reexport_from: Some("./other".to_owned()),
                forwards: Some("*".to_owned()),
                annotations: Vec::new(),
                returns: None,
                span: Span::new(0, 0),
            }],
        );

        let all_forms = PassthroughForms {
            reexport: true,
            alias: true,
            wrapper: true,
        };
        let findings = check(&engine(all_forms, true), &facts);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let Observed::Passthrough {
            exports,
            whole_file,
        } = &findings[0].observed
        else {
            panic!("expected a passthrough, got {:?}", findings[0].observed);
        };
        assert_eq!(exports, &["*".to_owned()]);
        assert!(
            whole_file,
            "a file that is only a star barrel is the whole-file case"
        );
    }

    /// And the exemption that decides how much this costs an existing
    /// repository: `allow_package_entrypoints` is on by default, and the star
    /// barrel is overwhelmingly written in a file called `index.ts`. Those were
    /// exempt before 0.22 and stay exempt — so the reporting change lands on
    /// star barrels under some *other* name, which is the narrower and more
    /// deliberate case.
    #[test]
    fn a_star_barrel_at_an_entry_point_stays_exempt() {
        let facts = facts(
            "packages/domain/src/plan/index.ts",
            &[],
            vec![ExportFact {
                name: Some("*".to_owned()),
                tags: ExportTags::only(ExportKind::Reexport),
                is_default: false,
                reexport_from: Some("./other".to_owned()),
                forwards: Some("*".to_owned()),
                annotations: Vec::new(),
                returns: None,
                span: Span::new(0, 0),
            }],
        );

        let all_forms = PassthroughForms {
            reexport: true,
            alias: true,
            wrapper: true,
        };
        assert!(
            check(&engine(all_forms, true), &facts).is_empty(),
            "a package entry point is a public API, and forwarding is what one is for"
        );
    }

    /// And it is a *re-export*, so a config that asked only for the other two
    /// forms is not suddenly given it. The form list is what a repository
    /// opted into, and this must not widen it.
    #[test]
    fn a_star_reexport_is_not_reported_when_reexports_were_not_asked_for() {
        let facts = facts(
            "packages/domain/src/plan/barrel.ts",
            &[],
            vec![ExportFact {
                name: Some("*".to_owned()),
                tags: ExportTags::only(ExportKind::Reexport),
                is_default: false,
                reexport_from: Some("./other".to_owned()),
                forwards: Some("*".to_owned()),
                annotations: Vec::new(),
                returns: None,
                span: Span::new(0, 0),
            }],
        );

        let others_only = PassthroughForms {
            reexport: false,
            alias: true,
            wrapper: true,
        };
        assert!(check(&engine(others_only, true), &facts).is_empty());
    }

    /// The partial case, and why it is opt-in.
    ///
    /// `feature/types/feature.ts` re-exports six names from another module and
    /// declares two interfaces of its own. It is not a file that adds nothing
    /// — saying so would be false — but six of its eight exports are still an
    /// indirection its importers could skip. Off by default; on when someone
    /// asks.
    #[test]
    fn a_partly_forwarding_file_is_reported_only_when_asked() {
        let mut exports: Vec<ExportFact> = (0..6)
            .map(|i| {
                let name = format!("FeatureItem{i}");
                export(&name, ExportTags::none(), Some(&name))
            })
            .collect();
        exports.push(export(
            "Feature",
            ExportTags::only(ExportKind::Interface),
            None,
        ));
        exports.push(export(
            "CreateFeatureInput",
            ExportTags::only(ExportKind::Interface),
            None,
        ));

        let imported: Vec<String> = (0..6).map(|i| format!("FeatureItem{i}")).collect();
        let names: Vec<&str> = imported.iter().map(String::as_str).collect();
        let facts = facts(
            "packages/domain/src/feature/types/feature.ts",
            &names,
            exports,
        );

        assert!(
            check(&engine(all_forms(), true), &facts).is_empty(),
            "a file that declares things of its own is not one that adds nothing"
        );

        let findings = check(&engine(all_forms(), false), &facts);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].observed,
            Observed::Passthrough {
                exports: (0..6).map(|i| format!("FeatureItem{i}")).collect(),
                whole_file: false,
            },
            "only the forwarded ones are named"
        );
    }

    /// A file that computes something is not a passthrough, however thin.
    #[test]
    fn a_file_that_declares_its_own_exports_is_left_alone() {
        let facts = facts(
            "packages/domain/src/plan/calcs/total.ts",
            &["rate"],
            vec![export(
                "total",
                ExportTags::only(ExportKind::Function),
                None,
            )],
        );

        assert!(check(&engine(all_forms(), false), &facts).is_empty());
    }

    /// An alias of a *local* binding is not a passthrough either. Nothing is
    /// being forwarded across a module boundary; the file is naming its own
    /// work twice.
    #[test]
    fn aliasing_a_local_binding_is_not_forwarding() {
        let facts = facts(
            "packages/domain/src/plan/calcs/x.ts",
            &["somethingElse"],
            vec![export(
                "publicName",
                ExportTags::only(ExportKind::Const),
                Some("privateName"),
            )],
        );

        assert!(
            check(&engine(all_forms(), false), &facts).is_empty(),
            "`privateName` was never imported"
        );
    }

    /// The exception that keeps the rule usable. A package's public API is a
    /// file whose whole job is forwarding, and a rule that reported every one
    /// of them the day it was switched on is a rule nobody leaves on.
    #[test]
    fn a_package_entry_point_is_exempt_by_default() {
        let facts = facts(
            "packages/domain/src/index.ts",
            &["A"],
            vec![export("A", ExportTags::none(), Some("A"))],
        );

        assert!(check(&engine(all_forms(), false), &facts).is_empty());
    }

    /// And the barrel bonus: turn the exemption off and an `index.ts` that
    /// only re-exports is case 1. That is the enforcement a "no barrel files"
    /// line in a CLAUDE.md has never had.
    #[test]
    fn a_barrel_is_reported_when_entry_points_are_not_exempt() {
        let rule = CompiledRule {
            id: RuleId::new("no-barrels").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["packages/domain/src/**"]).expect("valid scope"),
            kind: CompiledRuleKind::NoPassthrough {
                forms: PassthroughForms {
                    reexport: true,
                    alias: false,
                    wrapper: false,
                },
                except: PathSet::default(),
                allow_package_entrypoints: false,
                allow_partial: true,
            },
        };
        let engine = NoPassthroughEngine::from_rule(&rule).expect("builds");
        let facts = facts(
            "packages/domain/src/order/index.ts",
            &["A", "B"],
            vec![
                export("A", ExportTags::none(), Some("A")),
                export("B", ExportTags::none(), Some("B")),
            ],
        );

        assert_eq!(check(&engine, &facts).len(), 1);
    }

    /// A file with no exports at all forwards nothing. It is not a
    /// passthrough; it may not be anything.
    #[test]
    fn a_file_with_no_exports_is_not_a_passthrough() {
        let facts = facts("packages/domain/src/plan/calcs/x.ts", &["A"], Vec::new());
        assert!(check(&engine(all_forms(), false), &facts).is_empty());
    }
}
