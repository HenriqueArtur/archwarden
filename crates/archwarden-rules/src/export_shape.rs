//! The `export-shape` rule: what a file exposes, without its name.
//!
//! `naming` couples the export to the *filename*. Plenty of architectural
//! decisions are about the export alone:
//!
//! > *"We do not use default exports."*
//! > *"One export per file."*
//! > *"Every exported function in `use-cases/` returns `ResponsePattern<R, E>`."*
//!
//! None of them mentions a filename, and until 0.22 the only way to say any of
//! them was inside a `naming` rule — which demands a name template, so you had
//! to invent a naming claim you did not mean in order to make an export claim
//! you did. Issue #101.
//!
//! # The division of labour, which is the whole design
//!
//! `must_return` requires that a function **declares** its return type. It does
//! not check that the body conforms: that is `tsc`'s job and `tsc` is good at
//! it. What `tsc` cannot do is *require that you annotate at all* — a function
//! returning `{ ok: true }` with no return type compiles perfectly.
//!
//! **archwarden guarantees the pattern is declared; `tsc` guarantees the body
//! conforms.** Neither alone is the guarantee a team wants; together they are.
//!
//! # Where it stays out
//!
//! Inspecting the returned object literal in the AST. Early returns, ternaries,
//! delegation to a helper, spreads — a rule right about most files and silently
//! wrong about the rest is worse than no rule, because it is read as a
//! guarantee. `docs/RULES.md` already draws this line for `call-obligation`.
//!
//! # The hole this leaves, said out loud
//!
//! `must_return` matches the annotation **as text**, so an alias defeats it:
//! `type Result<T> = ResponsePattern<T, Error>` is the same type and a
//! different string. The field takes a *list* for exactly that reason — a team
//! with aliases lists them, and a team that writes one pattern has chosen
//! *"annotate with the canonical name"* as a convention the config now states.
//!
//! What closes it completely is pairing this with
//! `import-boundary.must_import_from`: the annotation must be the canonical
//! name, imported from the module that owns it. Without that, somebody declares
//! a local lookalike and every check passes.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind, ExportShape},
    facts::{ExportFact, ExportKind, FileFacts},
    finding::{Expectation, Finding, Observed},
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    pattern::Pattern,
    scope::Scope,
    traits::{FactsNeeded, FileContext, RuleEngine},
};

/// A compiled `export-shape` rule.
#[derive(Debug, Clone)]
pub struct ExportShapeEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    shape: ExportShape,
}

impl ExportShapeEngine {
    /// Builds an engine from a compiled rule.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::ExportShape(shape) = &rule.kind else {
            return None;
        };
        Some(Self::build(rule, shape))
    }

    /// Builds an engine from a rule whose kind is already known.
    pub(crate) fn build(rule: &CompiledRule, shape: &ExportShape) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            shape: shape.clone(),
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

/// Whether an export exists at runtime.
///
/// `type` and `interface` are erased, so they are not counted by
/// `max_exports`. A file exporting a function and the interface of its
/// dependencies is idiomatic TypeScript, and a `max_exports: 1` that fired on
/// it would be a rule nobody leaves on — the same argument
/// `spec-pair.skip_type_only` already makes one rule over.
fn exists_at_runtime(export: &ExportFact) -> bool {
    !(export.tags.contains(ExportKind::Type) || export.tags.contains(ExportKind::Interface))
}

/// Whether an export is something that can declare a return type.
///
/// A `function` declaration, or a function or arrow assigned to a binding.
/// Anything else has no return position, so `must_return` has nothing to say
/// about it and says nothing — rather than reporting every exported constant
/// as missing an annotation it could not have.
///
/// A re-export is excluded too: what it was declared as lives in another file,
/// which is the same reason its kind is `reexport` rather than guessed at.
fn is_callable(export: &ExportFact) -> bool {
    !export.tags.contains(ExportKind::Reexport)
        && (export.tags.contains(ExportKind::Function) || export.tags.contains(ExportKind::Arrow))
}

/// The label an export is reported under.
///
/// A default export may be anonymous, and `default` is what an importer writes
/// to reach it either way.
fn label(export: &ExportFact) -> String {
    export.name.clone().unwrap_or_else(|| "default".to_owned())
}

impl RuleEngine for ExportShapeEngine {
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
        path.parent()
            .is_some_and(|parent| self.scope.matches_dir(parent.as_path()))
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

        let mut findings = Vec::new();
        self.check_default(ctx.path, facts, &mut findings);
        self.check_count(ctx.path, facts, &mut findings);
        self.check_returns(ctx.path, facts, &mut findings);
        findings
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        if !self.applies_to(path) {
            return Vec::new();
        }

        let mut expectations = Vec::new();
        if self.shape.forbid_default {
            expectations.push(Expectation::NoDefaultExport);
        }
        if let Some(limit) = self.shape.max_exports {
            expectations.push(Expectation::AtMostExports { limit });
        }
        if !self.shape.must_return.is_empty() {
            expectations.push(Expectation::RequiredReturnType {
                patterns: self
                    .shape
                    .must_return
                    .iter()
                    .map(Pattern::as_str)
                    .map(ToOwned::to_owned)
                    .collect(),
            });
        }
        expectations
    }
}

impl ExportShapeEngine {
    fn check_default(&self, path: &RepoRelPath, facts: &FileFacts, out: &mut Vec<Finding>) {
        if !self.shape.forbid_default {
            return;
        }
        if let Some(default) = facts.exports.iter().find(|export| export.is_default) {
            out.push(self.finding(
                path,
                Observed::DefaultExportPresent {
                    name: default.name.clone(),
                },
                Expectation::NoDefaultExport,
            ));
        }
    }

    fn check_count(&self, path: &RepoRelPath, facts: &FileFacts, out: &mut Vec<Finding>) {
        let Some(limit) = self.shape.max_exports else {
            return;
        };

        let names: Vec<String> = facts
            .exports
            .iter()
            .filter(|export| exists_at_runtime(export))
            .map(label)
            .collect();

        if names.len() > limit {
            out.push(self.finding(
                path,
                Observed::TooManyExports { names, limit },
                Expectation::AtMostExports { limit },
            ));
        }
    }

    fn check_returns(&self, path: &RepoRelPath, facts: &FileFacts, out: &mut Vec<Finding>) {
        if self.shape.must_return.is_empty() {
            return;
        }

        let expected = || Expectation::RequiredReturnType {
            patterns: self
                .shape
                .must_return
                .iter()
                .map(Pattern::as_str)
                .map(ToOwned::to_owned)
                .collect(),
        };

        for export in facts.exports.iter().filter(|export| is_callable(export)) {
            match &export.returns {
                None => out.push(self.finding(
                    path,
                    Observed::ExportMissingReturnType {
                        name: label(export),
                    },
                    expected(),
                )),
                Some(declared) => {
                    if !self
                        .shape
                        .must_return
                        .iter()
                        .any(|pattern| pattern.is_match(declared))
                    {
                        out.push(self.finding(
                            path,
                            Observed::ExportWrongReturnType {
                                name: label(export),
                                found: declared.clone(),
                            },
                            expected(),
                        ));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::ExportShape,
        facts::{ExportTags, Span},
        hash::ContentHash,
        traits::Exists,
    };

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn engine(shape: &ExportShape) -> ExportShapeEngine {
        let rule = CompiledRule {
            id: RuleId::new("use-case-shape").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["src/use-cases"]).expect("valid scope"),
            kind: CompiledRuleKind::ExportShape(shape.clone()),
        };
        ExportShapeEngine::build(&rule, shape)
    }

    fn shape(
        forbid_default: bool,
        max_exports: Option<usize>,
        must_return: &[&str],
    ) -> ExportShape {
        ExportShape {
            forbid_default,
            max_exports,
            must_return: must_return
                .iter()
                .map(|p| Pattern::compile(p).expect("valid pattern"))
                .collect(),
        }
    }

    fn exported(name: Option<&str>, tags: ExportTags, returns: Option<&str>) -> ExportFact {
        ExportFact {
            visibility: archwarden_core::facts::Visibility::Public,
            name: name.map(ToOwned::to_owned),
            tags,
            is_default: name.is_none(),
            reexport_from: None,
            forwards: None,
            annotations: Vec::new(),
            returns: returns.map(ToOwned::to_owned),
            span: Span::new(0, 1),
        }
    }

    fn facts(exports: Vec<ExportFact>) -> FileFacts {
        let mut facts = FileFacts::unparsed(
            path("src/use-cases/create-client.ts"),
            ContentHash::of(b"x"),
        );
        facts.exports = exports;
        facts
    }

    fn check(engine: &ExportShapeEngine, facts: &FileFacts) -> Vec<Finding> {
        engine.check_file(FileContext {
            path: &facts.path,
            facts: Some(facts),
            docs: None,
            siblings: &[],
            exists: Exists::none(),
            graph: None,
            as_of: archwarden_core::date::Date::EPOCH,
        })
    }

    fn function(name: &str, returns: Option<&str>) -> ExportFact {
        exported(Some(name), ExportTags::only(ExportKind::Function), returns)
    }

    /// *"We do not use default exports."* One sentence, no filename in it, and
    /// inexpressible until 0.22 without inventing a `naming` claim. Issue #101.
    #[test]
    fn a_default_export_is_reported_when_the_rule_forbids_one() {
        let found = check(
            &engine(&shape(true, None, &[])),
            &facts(vec![exported(
                None,
                ExportTags::only(ExportKind::Function),
                None,
            )]),
        );

        assert_eq!(found.len(), 1, "{found:?}");
        assert!(matches!(
            found[0].observed,
            Observed::DefaultExportPresent { .. }
        ));
        assert_eq!(found[0].expected, Expectation::NoDefaultExport);
    }

    /// And a file with no default is silent, whatever else it exports.
    #[test]
    fn a_file_with_no_default_says_nothing() {
        let found = check(
            &engine(&shape(true, None, &[])),
            &facts(vec![function("CreateClient", None)]),
        );

        assert!(found.is_empty(), "{found:?}");
    }

    /// *"One export per file."* The count is of what exists at **runtime**: a
    /// file exporting a function and the interface of its dependencies is
    /// idiomatic TypeScript, and a limit that counted the interface would be a
    /// rule nobody leaves on.
    #[test]
    fn type_only_exports_do_not_count_towards_the_limit() {
        let one_runtime_export = facts(vec![
            function("CreateClient", None),
            exported(
                "CreateClientDeps".into(),
                ExportTags::only(ExportKind::Interface),
                None,
            ),
            exported(
                "CreateClientInput".into(),
                ExportTags::only(ExportKind::Type),
                None,
            ),
        ]);

        assert!(
            check(&engine(&shape(false, Some(1), &[])), &one_runtime_export).is_empty(),
            "three exports, one of them at runtime"
        );
    }

    /// Two runtime exports against a limit of one is the finding, and it names
    /// them — the fix is deciding which one leaves.
    #[test]
    fn too_many_runtime_exports_are_reported_by_name() {
        let found = check(
            &engine(&shape(false, Some(1), &[])),
            &facts(vec![
                function("CreateClient", None),
                function("CreateClientHelper", None),
            ]),
        );

        assert_eq!(found.len(), 1, "{found:?}");
        let Observed::TooManyExports { names, limit } = &found[0].observed else {
            panic!("expected TooManyExports, got {:?}", found[0].observed);
        };
        assert_eq!(names, &["CreateClient", "CreateClientHelper"]);
        assert_eq!(*limit, 1);
    }

    /// A default counts as one of them: it exists at runtime and an importer
    /// reaches it.
    #[test]
    fn a_default_counts_towards_the_limit() {
        let found = check(
            &engine(&shape(false, Some(1), &[])),
            &facts(vec![
                function("CreateClient", None),
                exported(None, ExportTags::only(ExportKind::Function), None),
            ]),
        );

        assert_eq!(found.len(), 1, "{found:?}");
        let Observed::TooManyExports { names, .. } = &found[0].observed else {
            panic!("expected TooManyExports");
        };
        assert_eq!(
            names,
            &["CreateClient", "default"],
            "the anonymous default is named as an importer would reach it"
        );
    }

    /// **The case that motivated the issue.** `tsc` is green: a function
    /// returning `{ ok: true }` with no return type compiles perfectly. So the
    /// absence is what archwarden reports, and it is a different finding from
    /// declaring the wrong thing.
    #[test]
    fn a_callable_declaring_no_return_type_is_the_finding() {
        let found = check(
            &engine(&shape(false, None, &["^ResponsePattern<.+,.+>$"])),
            &facts(vec![function("CreateClient", None)]),
        );

        assert_eq!(found.len(), 1, "{found:?}");
        assert!(matches!(
            &found[0].observed,
            Observed::ExportMissingReturnType { name } if name == "CreateClient"
        ));
    }

    /// Declaring something the rule does not accept is a different sentence
    /// and a different fix: one is "write the type down", the other is "you
    /// wrote a different one".
    #[test]
    fn a_callable_declaring_the_wrong_return_type_is_a_different_finding() {
        let found = check(
            &engine(&shape(false, None, &["^ResponsePattern<.+,.+>$"])),
            &facts(vec![function("CreateClient", Some("Promise<Client>"))]),
        );

        assert_eq!(found.len(), 1, "{found:?}");
        let Observed::ExportWrongReturnType {
            name,
            found: declared,
        } = &found[0].observed
        else {
            panic!(
                "expected ExportWrongReturnType, got {:?}",
                found[0].observed
            );
        };
        assert_eq!(name, "CreateClient");
        assert_eq!(declared, "Promise<Client>");
    }

    /// **The list is what settles the alias problem.** `type Result<T> =
    /// ResponsePattern<T, Error>` is the same type and a different string, and
    /// matching is text against text. A team with aliases lists them; a team
    /// that writes one pattern has chosen "annotate with the canonical name"
    /// as a convention the config now states rather than implies.
    #[test]
    fn any_pattern_in_the_list_satisfies_the_rule() {
        let rule = engine(&shape(
            false,
            None,
            &["^ResponsePattern<.+,.+>$", "^Result<.+>$"],
        ));

        for declared in ["ResponsePattern<Client, Error>", "Result<Client>"] {
            assert!(
                check(
                    &rule,
                    &facts(vec![function("CreateClient", Some(declared))])
                )
                .is_empty(),
                "{declared} should satisfy the rule"
            );
        }
    }

    /// An export that cannot return anything has nothing to declare, and is
    /// left alone rather than reported for an annotation it could not have.
    #[test]
    fn a_non_callable_export_is_not_asked_for_a_return_type() {
        let found = check(
            &engine(&shape(false, None, &["^Result<.+>$"])),
            &facts(vec![
                exported("CONFIG".into(), ExportTags::only(ExportKind::Const), None),
                exported("Deps".into(), ExportTags::only(ExportKind::Interface), None),
                exported("Tool".into(), ExportTags::only(ExportKind::Class), None),
            ]),
        );

        assert!(found.is_empty(), "{found:?}");
    }

    /// A re-export declared its return type in another file, which is the same
    /// reason its kind is `reexport` rather than guessed at. Reporting it here
    /// would be reporting a file for what a different file does.
    #[test]
    fn a_reexport_is_not_asked_for_a_return_type() {
        let mut reexported = exported(
            "CreateClient".into(),
            ExportTags::only(ExportKind::Reexport),
            None,
        );
        reexported.reexport_from = Some("./create-client".to_owned());

        assert!(
            check(
                &engine(&shape(false, None, &["^Result<.+>$"])),
                &facts(vec![reexported])
            )
            .is_empty()
        );
    }

    /// An arrow assigned to a const is how most use cases are written, and it
    /// is as callable as a `function`.
    #[test]
    fn an_arrow_is_asked_for_a_return_type_too() {
        let arrow = exported(
            "CreateClient".into(),
            ExportTags::only(ExportKind::Const).with(ExportKind::Arrow),
            None,
        );

        let found = check(
            &engine(&shape(false, None, &["^Result<.+>$"])),
            &facts(vec![arrow]),
        );
        assert_eq!(found.len(), 1, "{found:?}");
    }

    /// The three claims are independent: a rule asking one says nothing about
    /// the other two, and a rule asking all three reports each separately.
    #[test]
    fn the_three_claims_are_independent() {
        let quiet = engine(&shape(false, None, &[]));
        assert!(
            check(
                &quiet,
                &facts(vec![
                    exported(None, ExportTags::only(ExportKind::Function), None),
                    function("A", None),
                    function("B", None),
                ])
            )
            .is_empty(),
            "a rule that asks nothing reports nothing"
        );

        let all_three = engine(&shape(true, Some(1), &["^Result<.+>$"]));
        let found = check(
            &all_three,
            &facts(vec![
                exported(None, ExportTags::only(ExportKind::Function), None),
                function("A", None),
            ]),
        );
        assert_eq!(
            found.len(),
            4,
            "one default, one over the limit, two returns: {found:?}"
        );
    }

    /// `describe` and `scaffold` ask what a rule wants *before* a file exists,
    /// so the expectations have to be answerable without facts. One per claim
    /// the rule makes, and none for a claim it does not.
    #[test]
    fn the_expectations_are_one_per_claim() {
        let target = path("src/use-cases/create-client.ts");

        assert!(
            engine(&shape(false, None, &[]))
                .describe_expectation(&target)
                .is_empty()
        );

        let all_three =
            engine(&shape(true, Some(1), &["^Result<.+>$"])).describe_expectation(&target);
        assert_eq!(all_three.len(), 3, "{all_three:?}");
        assert!(all_three.contains(&Expectation::NoDefaultExport));
        assert!(all_three.contains(&Expectation::AtMostExports { limit: 1 }));
        assert!(all_three.contains(&Expectation::RequiredReturnType {
            patterns: vec!["^Result<.+>$".to_owned()],
        }));
    }

    /// The two accessors every surface reads a finding through. `module` is
    /// what puts `[domain]` in front of a finding, and an engine built from a
    /// rule of the wrong kind is `None` rather than a panic.
    #[test]
    fn an_engine_carries_its_module_and_refuses_another_kind() {
        let rule = CompiledRule {
            id: RuleId::new("use-case-shape").expect("valid id"),
            module: Some(ModuleId::new("use-cases").expect("valid module")),
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["src/use-cases"]).expect("valid scope"),
            kind: CompiledRuleKind::ExportShape(shape(true, None, &[])),
        };

        let built = ExportShapeEngine::from_rule(&rule).expect("the kind matches");
        assert_eq!(
            built.module().map(ModuleId::as_str),
            Some("use-cases"),
            "a finding from this rule reports under its module"
        );
        assert_eq!(built.id().as_str(), "use-case-shape");
        assert_eq!(built.level(), Level::Error);

        let other = CompiledRule {
            kind: CompiledRuleKind::ImportCycle {
                include_type_only: false,
            },
            ..rule
        };
        assert!(
            ExportShapeEngine::from_rule(&other).is_none(),
            "a rule of another kind builds no engine, rather than one that does nothing"
        );
    }

    /// A file outside the scope is not this rule's business, before or after
    /// it exists.
    #[test]
    fn a_file_outside_the_scope_is_untouched() {
        let rule = engine(&shape(true, Some(1), &["^Result<.+>$"]));
        let elsewhere = path("src/adapters/http.ts");

        assert!(rule.describe_expectation(&elsewhere).is_empty());

        let mut outside = facts(vec![exported(
            None,
            ExportTags::only(ExportKind::Function),
            None,
        )]);
        outside.path = elsewhere;
        assert!(check(&rule, &outside).is_empty());
    }
}
