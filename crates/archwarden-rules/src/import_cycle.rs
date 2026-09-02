//! The `import-cycle` rule: no file in scope may sit on an import loop.
//!
//! The first rule that cannot be answered from one file. Every other rule here
//! reads the file in front of it — its name, its exports, its own imports —
//! and this one asks a question about the shape of the repository, so it reads
//! [`ImportGraph`](archwarden_core::graph::ImportGraph) instead. See
//! `docs/RULES.md`.
//!
//! # Why every file in the loop is reported
//!
//! A loop has no owner. dependency-cruiser reports the *closing* edge, which
//! depends on which file the walk happened to start from, so the same cycle
//! moves between runs and between machines. This rule reports the finding on
//! every file of the loop its scope covers, once each, carrying the whole
//! chain — because that is the honest count: N files have to change, or N
//! people have to agree not to. `baseline` accepts findings per rule and per
//! path, so an accepted cycle is accepted at the same N.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    finding::{Expectation, Finding, Observed},
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    scope::Scope,
    traits::{FactsNeeded, FileContext, RuleEngine},
};

/// A compiled `import-cycle` rule.
#[derive(Debug, Clone)]
pub struct ImportCycleEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    include_type_only: bool,
}

impl ImportCycleEngine {
    /// Builds an engine from a compiled rule.
    ///
    /// Returns `None` for a rule of any other kind.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::ImportCycle { include_type_only } = &rule.kind else {
            return None;
        };

        Some(Self::build(rule, *include_type_only))
    }

    /// Builds an engine from a rule whose kind is already known.
    pub(crate) fn build(rule: &CompiledRule, include_type_only: bool) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            include_type_only,
        }
    }

    fn expectation() -> Expectation {
        Expectation::NoImportCycle
    }

    fn finding(&self, path: &RepoRelPath, chain: Vec<RepoRelPath>) -> Finding {
        Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: path.clone(),
            // No span. The loop is not at a place in this file: it is the
            // whole chain, and pointing at one `import` line would name one
            // edge of several as the guilty one.
            span: None,
            observed: Observed::ImportCycle { chain },
            expected: Self::expectation(),
        }
    }
}

impl RuleEngine for ImportCycleEngine {
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

    fn needs_facts(&self) -> FactsNeeded {
        FactsNeeded::Code
    }

    fn needs_resolution(&self) -> bool {
        true
    }

    fn needs_graph(&self) -> bool {
        true
    }

    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        if !self.applies_to(ctx.path) {
            return Vec::new();
        }
        // A rule that needs the graph is never handed `None` by a driver that
        // can build one; a driver that cannot refuses the rule outright and
        // says so. Reporting nothing here would be reporting "no cycles",
        // which is the one answer this rule must never give by accident.
        let Some(graph) = ctx.graph else {
            return Vec::new();
        };

        graph
            .cycle_through(ctx.path, self.include_type_only)
            .map(|chain| vec![self.finding(ctx.path, chain)])
            .unwrap_or_default()
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        if self.applies_to(path) {
            vec![Self::expectation()]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        graph::{Edge, FileEdges, ImportGraph},
        traits::Exists,
    };

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn rule(scope: &[&str], include_type_only: bool) -> CompiledRule {
        CompiledRule {
            id: RuleId::new("no-cycles").expect("valid id"),
            module: None,
            why: None,
            not_yet: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Error,
            scope: Scope::compile(scope).expect("valid scope"),
            kind: CompiledRuleKind::ImportCycle { include_type_only },
        }
    }

    /// Files as `(path, [imports])`, every edge resolved and not type-only.
    fn graph(files: &[(&str, &[&str])]) -> ImportGraph {
        ImportGraph::of(files.iter().map(|(from, imports)| {
            FileEdges {
                from: path(from),
                to: imports
                    .iter()
                    .map(|to| Edge {
                        to: path(to),
                        type_only: false,
                    })
                    .collect(),
            }
        }))
    }

    fn check(engine: &ImportCycleEngine, at: &str, graph: &ImportGraph) -> Vec<Finding> {
        engine.check_file(FileContext {
            path: &path(at),
            facts: None,
            docs: None,
            siblings: &[],
            exists: Exists::none(),
            graph: Some(graph),
            as_of: archwarden_core::date::Date::EPOCH,
        })
    }

    fn chain_of(finding: &Finding) -> Vec<&str> {
        match &finding.observed {
            Observed::ImportCycle { chain } => chain.iter().map(RepoRelPath::as_str).collect(),
            other => panic!("expected a cycle, got {other:?}"),
        }
    }

    /// The finding carries the whole chain, because the chain is the answer:
    /// "`src/a.ts` is in a cycle" is not actionable, and the loop through
    /// `src/b.ts` names the edge to cut.
    #[test]
    fn a_file_on_a_loop_is_reported_with_the_whole_chain() {
        let engine =
            ImportCycleEngine::from_rule(&rule(&["src/**"], false)).expect("the kind matches");
        let graph = graph(&[("src/a.ts", &["src/b.ts"]), ("src/b.ts", &["src/a.ts"])]);

        let findings = check(&engine, "src/a.ts", &graph);

        assert_eq!(findings.len(), 1);
        assert_eq!(chain_of(&findings[0]), ["src/a.ts", "src/b.ts", "src/a.ts"]);
        assert_eq!(findings[0].path.as_str(), "src/a.ts");
        assert_eq!(findings[0].expected, Expectation::NoImportCycle);
    }

    /// A loop has no owner, so every file on it that the scope covers is
    /// reported — once each, each carrying the loop as seen from itself. N
    /// files have to change, and a report naming one of them would be picking
    /// the guilty party by walk order.
    #[test]
    fn every_file_on_the_loop_is_reported_once_from_its_own_side() {
        let engine =
            ImportCycleEngine::from_rule(&rule(&["src/**"], false)).expect("the kind matches");
        let graph = graph(&[("src/a.ts", &["src/b.ts"]), ("src/b.ts", &["src/a.ts"])]);

        assert_eq!(
            chain_of(&check(&engine, "src/b.ts", &graph)[0]),
            ["src/b.ts", "src/a.ts", "src/b.ts"],
            "the same loop, named from where the reader is standing"
        );
    }

    /// A file outside the scope is not reported, even when it is on a loop the
    /// graph can see. The graph is whole-repository on purpose — a loop that
    /// leaves the scope and comes back is still a loop — and the *finding* is
    /// what the scope governs.
    #[test]
    fn a_file_outside_the_scope_is_not_reported_though_the_graph_sees_it() {
        let engine =
            ImportCycleEngine::from_rule(&rule(&["apps/**"], false)).expect("the kind matches");
        let graph = graph(&[
            ("apps/api.ts", &["packages/db.ts"]),
            ("packages/db.ts", &["apps/api.ts"]),
        ]);

        assert_eq!(
            check(&engine, "apps/api.ts", &graph).len(),
            1,
            "the in-scope end of the loop is reported"
        );
        assert!(
            check(&engine, "packages/db.ts", &graph).is_empty(),
            "and the out-of-scope end is not, though the loop runs through it"
        );
    }

    #[test]
    fn a_file_on_no_loop_is_not_reported() {
        let engine =
            ImportCycleEngine::from_rule(&rule(&["src/**"], false)).expect("the kind matches");
        let graph = graph(&[("src/a.ts", &["src/b.ts"]), ("src/b.ts", &["src/c.ts"])]);

        assert!(check(&engine, "src/a.ts", &graph).is_empty());
    }

    /// `include_type_only` reaches the query rather than the graph, so the
    /// same graph answers a rule that counts type imports and one that does
    /// not.
    #[test]
    fn a_loop_of_type_imports_is_the_rules_choice() {
        let type_only = ImportGraph::of(
            [
                FileEdges {
                    from: path("src/a.ts"),
                    to: vec![Edge {
                        to: path("src/b.ts"),
                        type_only: true,
                    }],
                },
                FileEdges {
                    from: path("src/b.ts"),
                    to: vec![Edge {
                        to: path("src/a.ts"),
                        type_only: false,
                    }],
                },
            ]
            .into_iter(),
        );

        assert!(
            check(
                &ImportCycleEngine::from_rule(&rule(&["src/**"], false)).expect("kind"),
                "src/a.ts",
                &type_only
            )
            .is_empty(),
            "erased at runtime, so not a loop when the rule says so"
        );
        assert_eq!(
            check(
                &ImportCycleEngine::from_rule(&rule(&["src/**"], true)).expect("kind"),
                "src/a.ts",
                &type_only
            )
            .len(),
            1,
            "and a loop at compile time when it says the other thing"
        );
    }

    /// The rule is built from its own kind and nothing else.
    #[test]
    fn a_rule_of_another_kind_does_not_build_this_engine() {
        let mut other = rule(&["src/**"], false);
        other.kind = CompiledRuleKind::Presence {
            require: Vec::new(),
            require_any: Vec::new(),
            forbid: Vec::new(),
        };

        assert!(ImportCycleEngine::from_rule(&other).is_none());
    }

    /// Decision 9: whatever `check` demands is what `describe_expectation`
    /// advertises, so `scaffold` never tells an agent to write something the
    /// gate then rejects.
    #[test]
    fn what_check_demands_is_what_describe_expectation_advertises() {
        let engine =
            ImportCycleEngine::from_rule(&rule(&["src/**"], false)).expect("the kind matches");
        let graph = graph(&[("src/a.ts", &["src/b.ts"]), ("src/b.ts", &["src/a.ts"])]);

        let demanded = &check(&engine, "src/a.ts", &graph)[0].expected;
        let advertised = engine.describe_expectation(&path("src/a.ts"));

        assert_eq!(advertised, vec![demanded.clone()]);
        assert!(
            engine
                .describe_expectation(&path("elsewhere/a.ts"))
                .is_empty(),
            "and it advertises nothing where it does not apply"
        );
    }

    /// The three questions the runner asks before it does any work. This rule
    /// is the only one that answers `true` to the third, and that answer costs
    /// a resolution pass over the whole repository.
    #[test]
    fn the_rule_declares_what_it_costs() {
        let engine =
            ImportCycleEngine::from_rule(&rule(&["src/**"], false)).expect("the kind matches");

        assert_eq!(engine.needs_facts(), FactsNeeded::Code);
        assert!(engine.needs_resolution());
        assert!(engine.needs_graph());
        assert!(!engine.answers_for_directories());
        assert_eq!(engine.id().as_str(), "no-cycles");
        assert_eq!(engine.module(), None);
        assert_eq!(engine.level(), Level::Error);
    }

    /// A rule declared inside a module carries that module onto its findings.
    ///
    /// The report groups by module and `config explain` answers per module, so
    /// a finding that lost it is one nobody can trace back to the boundary it
    /// belongs to. Asserted on a finding rather than only on the accessor,
    /// because the accessor returning the right thing is not the property that
    /// matters.
    #[test]
    fn a_rule_inside_a_module_stamps_its_findings_with_it() {
        let mut rule = rule(&["src/**"], false);
        rule.module = Some(ModuleId::new("domain").expect("valid module"));
        let engine = ImportCycleEngine::from_rule(&rule).expect("the kind matches");
        let graph = graph(&[("src/a.ts", &["src/b.ts"]), ("src/b.ts", &["src/a.ts"])]);

        assert_eq!(engine.module().map(ModuleId::as_str), Some("domain"));
        assert_eq!(
            check(&engine, "src/a.ts", &graph)[0]
                .module_id
                .as_ref()
                .map(ModuleId::as_str),
            Some("domain")
        );
    }

    /// A driver that cannot build a graph must refuse this rule rather than
    /// let it run. If one ever hands it `None`, the rule reports nothing —
    /// and this test exists to say that silence is a driver's bug, not this
    /// rule's answer.
    #[test]
    fn without_a_graph_the_rule_decides_nothing() {
        let engine =
            ImportCycleEngine::from_rule(&rule(&["src/**"], false)).expect("the kind matches");

        assert!(
            engine
                .check_file(FileContext {
                    path: &path("src/a.ts"),
                    facts: None,
                    docs: None,
                    siblings: &[],
                    exists: Exists::none(),
                    graph: None,
                    as_of: archwarden_core::date::Date::EPOCH,
                })
                .is_empty()
        );
    }
}
