//! A capability only these files may reach.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    finding::{Expectation, Finding, Observed},
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    scope::Scope,
    traits::{FactsNeeded, FileContext, RuleEngine},
};

/// A compiled `chokepoint` rule.
#[derive(Debug, Clone)]
pub struct ChokepointEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    callee: Vec<String>,
    only_in: Scope,
}

impl ChokepointEngine {
    /// Builds an engine from a compiled rule.
    ///
    /// Returns `None` for a rule of any other kind.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::Chokepoint { callee, only_in } = &rule.kind else {
            return None;
        };

        Some(Self::build(rule, callee, only_in))
    }

    /// Builds an engine from a rule whose kind is already known.
    pub(crate) fn build(rule: &CompiledRule, callee: &[String], only_in: &Scope) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            callee: callee.to_vec(),
            only_in: only_in.clone(),
        }
    }

    /// Whether this callee is one the rule guards.
    ///
    /// Exact, or a prefix at a dot. `process.env` guards `process.env` and
    /// `process.env.DATABASE_URL`, and does not guard `processing.env` --
    /// the dot is what makes it a boundary between names rather than a
    /// boundary between characters.
    ///
    /// A change of dialect from `call-obligation`, which matches its symbol
    /// exactly. Deliberate: that rule names one function, and this one names a
    /// capability whose members are written as the source finds them.
    fn guards(&self, callee: &str) -> bool {
        self.callee.iter().any(|guarded| {
            callee == guarded
                || callee
                    .strip_prefix(guarded.as_str())
                    .is_some_and(|rest| rest.starts_with('.'))
        })
    }

    fn expectation(&self) -> Expectation {
        Expectation::UsedOnlyIn {
            callee: self.callee.clone(),
            only_in: self.only_in.patterns().to_vec(),
        }
    }
}

impl RuleEngine for ChokepointEngine {
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

    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        if !self.applies_to(ctx.path) {
            return Vec::new();
        }
        // A file inside the chokepoint is the file the capability is *for*.
        // Checked before the facts, because it is true whether or not the
        // parser ran.
        if self.only_in.contains_file(ctx.path.as_path()) {
            return Vec::new();
        }
        // No facts means no parser ran. Reporting a breach would blame the
        // file for the run's own gap; reporting nothing is the same answer a
        // file with no calls gives.
        let Some(facts) = ctx.facts else {
            return Vec::new();
        };

        // Calls *and* reads. `process.env.DATABASE_URL` is the capability
        // this rule was raised about and it is never a call site, so a rule
        // that looked only at calls would answer half the question it was
        // asked. `Date.now()` and `fetch()` are calls; `process.env` and
        // `localStorage` are reads; both are the same sentence to an author.
        let calls = facts
            .calls
            .iter()
            .map(|call| (call.callee.as_str(), call.span));
        let reads = facts
            .reads
            .iter()
            .map(|read| (read.path.as_str(), read.span));

        // One finding per site rather than one per file: the reader has to go
        // and look at each of them, and a count is not a location.
        // Sorted by where they are, not by which list they came from: a
        // reader works down the file, and calls before reads is an order that
        // means something only to this function.
        let mut sites: Vec<_> = calls
            .chain(reads)
            .filter(|(name, _)| self.guards(name))
            .collect();
        sites.sort_by_key(|(_, span)| span.start);

        sites
            .into_iter()
            .map(|(name, span)| Finding {
                rule_id: self.id.clone(),
                module_id: self.module.clone(),
                level: self.level,
                path: ctx.path.clone(),
                span: Some(span),
                observed: Observed::ChokepointBreached {
                    callee: name.to_owned(),
                },
                expected: self.expectation(),
            })
            .collect()
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        // Said to a file inside the chokepoint as well as outside it. Somebody
        // about to write in `src/config` should be told it is the one place
        // that may read the environment -- that is the half of the sentence
        // that explains the other half.
        if self.applies_to(path) {
            vec![self.expectation()]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use archwarden_core::{
        facts::{CallFact, FileFacts, ReadFact, Span},
        hash::ContentHash,
        ids::RuleId,
        traits::Exists,
    };

    use super::*;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn engine() -> ChokepointEngine {
        ChokepointEngine::from_rule(&CompiledRule {
            id: RuleId::new("the-environment-is-read-once").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Chokepoint {
                callee: vec!["process.env".to_owned(), "new PostgresRepo".to_owned()],
                only_in: Scope::compile(["src/config/**"]).expect("valid scope"),
            },
        })
        .expect("a chokepoint rule")
    }

    /// Facts for a file: `calls` as callee paths, `reads` as dotted names.
    fn facts(at: &str, calls: &[&str], reads: &[&str]) -> FileFacts {
        let mut facts = FileFacts::unparsed(path(at), ContentHash::of(b"source"));
        for (index, callee) in calls.iter().enumerate() {
            facts.calls.push(CallFact {
                callee: (*callee).to_owned(),
                arguments: Vec::new(),
                options: Vec::new(),
                #[expect(clippy::cast_possible_truncation, reason = "test spans are tiny")]
                span: Span::new(100 + index as u32 * 10, 110 + index as u32 * 10),
            });
        }
        for (index, name) in reads.iter().enumerate() {
            facts.reads.push(ReadFact {
                path: (*name).to_owned(),
                #[expect(clippy::cast_possible_truncation, reason = "test spans are tiny")]
                span: Span::new(index as u32 * 10, 5 + index as u32 * 10),
            });
        }
        facts
    }

    fn check(engine: &ChokepointEngine, facts: &FileFacts) -> Vec<Finding> {
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

    /// The sentence issue #118 was raised for: *only `src/config` reads the
    /// environment*. `process.env` has no import to forbid, so
    /// `import-boundary` cannot say it and nothing else could.
    #[test]
    fn a_guarded_name_outside_the_chokepoint_is_reported() {
        let outside = facts("src/orders/place.ts", &[], &["process.env.STRIPE_KEY"]);
        let reported = check(&engine(), &outside);

        assert_eq!(
            reported.first().map(|f| &f.observed),
            Some(&Observed::ChokepointBreached {
                callee: "process.env.STRIPE_KEY".to_owned(),
            }),
            "{reported:?}"
        );
        // The name as it appears *here*, not the pattern that matched it: the
        // reader has to go and find this line.
        assert!(reported[0].span.is_some(), "{reported:?}");
    }

    /// And inside it is the point of the rule, not a violation.
    #[test]
    fn the_file_the_capability_is_for_is_silent() {
        let inside = facts("src/config/env.ts", &[], &["process.env.DATABASE_URL"]);

        assert!(check(&engine(), &inside).is_empty());
    }

    /// Reads *and* calls. `process.env` is never a call site and `Date.now()`
    /// always is, and to an author they are one sentence.
    #[test]
    fn both_a_call_and_a_read_are_guarded() {
        let both = facts(
            "src/orders/place.ts",
            &["new PostgresRepo"],
            &["process.env"],
        );
        let reported = check(&engine(), &both);

        // One finding per site, ordered by where they are rather than by which
        // list they came from: a reader works down the file.
        assert_eq!(reported.len(), 2, "{reported:?}");
        assert_eq!(
            reported
                .iter()
                .map(|f| match &f.observed {
                    Observed::ChokepointBreached { callee } => callee.as_str(),
                    other => panic!("{other:?}"),
                })
                .collect::<Vec<_>>(),
            ["process.env", "new PostgresRepo"]
        );
    }

    /// The prefix is at a dot, which is what makes it a boundary between names
    /// rather than between characters.
    #[test]
    fn a_name_that_merely_starts_the_same_is_not_guarded() {
        let engine = engine();

        assert!(engine.guards("process.env"));
        assert!(engine.guards("process.env.DATABASE_URL"));
        assert!(!engine.guards("processing.env"));
        assert!(!engine.guards("process"));
        // A construction answers to the two words the source writes, and a
        // plain call to the same name does not.
        assert!(engine.guards("new PostgresRepo"));
        assert!(!engine.guards("PostgresRepo"));
    }

    /// Outside `roots` the rule says nothing. A test suite reads the
    /// environment legitimately, and a rule whose first run reports the tests
    /// is one nobody keeps.
    #[test]
    fn a_file_the_rule_does_not_govern_is_not_asked() {
        let elsewhere = facts("tests/place.spec.ts", &[], &["process.env.STRIPE_KEY"]);

        assert!(check(&engine(), &elsewhere).is_empty());
    }

    /// No facts means no parser ran. Reporting a breach would blame the file
    /// for the run's own gap.
    #[test]
    fn a_file_with_no_facts_is_not_reported() {
        let engine = engine();
        let findings = engine.check_file(FileContext {
            path: &path("src/orders/place.ts"),
            facts: None,
            docs: None,
            siblings: &[],
            exists: Exists::none(),
            graph: None,
            as_of: archwarden_core::date::Date::EPOCH,
        });

        assert!(findings.is_empty(), "{findings:?}");
    }

    /// Said to a file inside the chokepoint as well as outside it: somebody
    /// writing in `src/config` should be told it is the one place that may do
    /// this, which is the half of the sentence that explains the other half.
    /// The module a rule belongs to travels with its findings, which is what
    /// lets a report group by area rather than by rule.
    #[test]
    fn the_rules_module_is_carried() {
        let mut rule = CompiledRule {
            id: RuleId::new("r").expect("valid id"),
            module: Some(ModuleId::new("config").expect("valid module")),
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Chokepoint {
                callee: vec!["process.env".to_owned()],
                only_in: Scope::compile(["src/config/**"]).expect("valid scope"),
            },
        };
        let engine = ChokepointEngine::from_rule(&rule).expect("a chokepoint rule");
        assert_eq!(engine.module().map(ModuleId::as_str), Some("config"));

        let reported = check(
            &engine,
            &facts("src/orders/place.ts", &[], &["process.env"]),
        );
        assert_eq!(
            reported.first().and_then(|f| f.module_id.as_ref()),
            Some(&ModuleId::new("config").expect("valid module"))
        );

        // And a rule belonging to none carries none, rather than inventing one.
        rule.module = None;
        assert!(
            ChokepointEngine::from_rule(&rule)
                .expect("a chokepoint rule")
                .module()
                .is_none()
        );
    }

    #[test]
    fn the_expectation_reaches_both_sides() {
        let engine = engine();
        let expected = Expectation::UsedOnlyIn {
            callee: vec!["process.env".to_owned(), "new PostgresRepo".to_owned()],
            only_in: vec!["src/config/**".to_owned()],
        };

        assert_eq!(
            engine.describe_expectation(&path("src/config/env.ts")),
            std::slice::from_ref(&expected)
        );
        assert_eq!(
            engine.describe_expectation(&path("src/orders/place.ts")),
            std::slice::from_ref(&expected)
        );
        assert!(
            engine
                .describe_expectation(&path("tests/place.spec.ts"))
                .is_empty()
        );
    }
}
