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
    renders: Vec<String>,
    file_pattern: Option<archwarden_core::pattern::Pattern>,
    imported_from: Option<String>,
    only_in: Scope,
}

/// The fields of a `chokepoint` rule, as `engines_for` destructured them.
///
/// A struct rather than six arguments, the shape this workspace reaches for
/// when a signature outgrows a reader.
pub(crate) struct ChokepointFields<'a> {
    /// The callees it guards.
    pub callee: &'a [String],
    /// The JSX elements it guards.
    pub renders: &'a [String],
    /// The filenames it governs, when it narrows by one.
    pub file_pattern: Option<&'a archwarden_core::pattern::Pattern>,
    /// The module a guarded name has to come from, when it names one.
    pub imported_from: Option<&'a str>,
    /// The files allowed to reach them.
    pub only_in: &'a Scope,
}

impl ChokepointEngine {
    /// Builds an engine from a compiled rule.
    ///
    /// Returns `None` for a rule of any other kind.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::Chokepoint {
            callee,
            renders,
            file_pattern,
            imported_from,
            only_in,
        } = &rule.kind
        else {
            return None;
        };

        Some(Self::build(
            rule,
            &ChokepointFields {
                callee,
                renders,
                file_pattern: file_pattern.as_ref(),
                imported_from: imported_from.as_deref(),
                only_in,
            },
        ))
    }

    /// Builds an engine from a rule whose kind is already known.
    pub(crate) fn build(rule: &CompiledRule, fields: &ChokepointFields<'_>) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            callee: fields.callee.to_vec(),
            renders: fields.renders.to_vec(),
            file_pattern: fields.file_pattern.cloned(),
            imported_from: fields.imported_from.map(str::to_owned),
            only_in: fields.only_in.clone(),
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

    /// Whether the file took this name from the module the rule names.
    ///
    /// `Ledger.post` is reached through the binding `Ledger`, so that is what
    /// the import has to provide -- the same root a `call-obligation` looks
    /// for, and `Ui.Button` and `a::b` split the same way. Matched against the
    /// specifier **as written**, which is why this costs no resolution.
    ///
    /// A rule that names no module asks nothing: an ambient capability like
    /// `process.env` is imported from nowhere and there is nothing to
    /// disambiguate. Issue #146.
    fn came_from_the_named_module(
        &self,
        name: &str,
        facts: &archwarden_core::facts::FileFacts,
    ) -> bool {
        let Some(module) = self.imported_from.as_deref() else {
            return true;
        };
        let root = name
            .split_once("::")
            .or_else(|| name.split_once('.'))
            .map_or(name, |(root, _)| root);

        facts.imports.iter().any(|import| {
            import.specifier == module && import.names.iter().any(|bound| bound == root)
        })
    }

    fn expectation(&self) -> Expectation {
        Expectation::UsedOnlyIn {
            callee: self.callee.clone(),
            renders: self.renders.clone(),
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
            // `roots` selects directories; this selects the files in them.
            // *"Only `*.server.ts` may call `fetch`"* is a sentence about a
            // filename. Issue #146.
            && self.file_pattern.as_ref().is_none_or(|wanted| {
                path.file_name().is_some_and(|name| wanted.is_match(name))
            })
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
            .filter(|(name, _)| self.guards(name) && self.came_from_the_named_module(name, facts))
            .map(|(name, span)| (name, span, false))
            // A render is a use and a *different* one: `<Card />` compiles to
            // a call, and matching it against `callee` would make a rule about
            // a capability start firing on markup. Matched exactly, because
            // `Ui.Button` is one component rather than a member of a `Ui`
            // capability. Issue #145.
            .chain(
                facts
                    .renders
                    .iter()
                    .filter(|render| {
                        self.renders.contains(&render.name)
                            && self.came_from_the_named_module(&render.name, facts)
                    })
                    .map(|render| (render.name.as_str(), render.span, true)),
            )
            .collect();
        sites.sort_by_key(|(_, span, _)| span.start);

        sites
            .into_iter()
            .map(|(name, span, rendered)| Finding {
                rule_id: self.id.clone(),
                module_id: self.module.clone(),
                level: self.level,
                path: ctx.path.clone(),
                span: Some(span),
                observed: Observed::ChokepointBreached {
                    callee: name.to_owned(),
                    rendered,
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

    /// An engine guarding JSX elements rather than calls. Issue #145.
    fn rendering_engine(renders: &[&str], only_in: &[&str]) -> ChokepointEngine {
        ChokepointEngine::from_rule(&CompiledRule {
            id: RuleId::new("only-checkout-renders-its-form").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Error,
            scope: Scope::compile(["src/features/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Chokepoint {
                callee: Vec::new(),
                renders: renders.iter().map(|r| (*r).to_owned()).collect(),
                file_pattern: None,
                imported_from: None,
                only_in: Scope::compile(only_in.iter().copied()).expect("valid scope"),
            },
        })
        .expect("a chokepoint rule")
    }

    /// Facts for a file that renders these elements.
    fn rendering(at: &str, elements: &[&str]) -> FileFacts {
        let mut facts = FileFacts::unparsed(path(at), ContentHash::of(b"source"));
        for (index, name) in elements.iter().enumerate() {
            facts.renders.push(archwarden_core::facts::RenderFact {
                name: (*name).to_owned(),
                #[expect(clippy::cast_possible_truncation, reason = "test spans are tiny")]
                span: Span::new(index as u32 * 10, 5 + index as u32 * 10),
            });
        }
        facts
    }

    fn engine() -> ChokepointEngine {
        ChokepointEngine::from_rule(&CompiledRule {
            id: RuleId::new("the-environment-is-read-once").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Chokepoint {
                callee: vec!["process.env".to_owned(), "new PostgresRepo".to_owned()],
                renders: Vec::new(),
                file_pattern: None,
                imported_from: None,
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
                rendered: false,
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
                    Observed::ChokepointBreached { callee, .. } => callee.as_str(),
                    other => panic!("{other:?}"),
                })
                .collect::<Vec<_>>(),
            ["process.env", "new PostgresRepo"]
        );
    }

    /// Issue #146. Two packages can export a `Ledger`, and a rule about
    /// *this* project's one should not fire on the other. Matched against the
    /// specifier as written, the way `call-obligation` matches it, so the rule
    /// needs no resolution.
    #[test]
    fn a_guarded_name_from_another_module_is_left_alone() {
        let engine = ChokepointEngine::from_rule(&CompiledRule {
            id: RuleId::new("only-the-ledger-posts").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Chokepoint {
                callee: vec!["Ledger.post".to_owned()],
                renders: Vec::new(),
                file_pattern: None,
                imported_from: Some("@org/accounting".to_owned()),
                only_in: Scope::compile(["src/accounting/**"]).expect("valid scope"),
            },
        })
        .expect("a chokepoint rule");

        let importing = |specifier: &str| {
            let mut facts = facts("src/orders/place.ts", &["Ledger.post"], &[]);
            facts.imports.push(archwarden_core::facts::ImportFact {
                specifier: specifier.to_owned(),
                resolved: None,
                type_only: false,
                names: vec!["Ledger".to_owned()],
                span: Span::new(0, 10),
            });
            facts
        };

        assert!(!check(&engine, &importing("@org/accounting")).is_empty());
        assert!(
            check(&engine, &importing("@other/ledger")).is_empty(),
            "a different package's `Ledger` is a different `Ledger`"
        );
        // And a file that imports it from nowhere is not guarded either: the
        // rule named where the name comes from, and this one did not come from
        // there.
        assert!(
            check(
                &engine,
                &facts("src/orders/place.ts", &["Ledger.post"], &[])
            )
            .is_empty()
        );
    }

    /// Issue #146. `roots` selects directories; `file_pattern` selects the
    /// files in them. *"Only `*.server.ts` may call `fetch`"* is a sentence
    /// about a filename.
    #[test]
    fn a_file_pattern_narrows_the_population_further() {
        let engine = ChokepointEngine::from_rule(&CompiledRule {
            id: RuleId::new("only-a-server-file-fetches").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Chokepoint {
                callee: vec!["fetch".to_owned()],
                renders: Vec::new(),
                file_pattern: Some(
                    archwarden_core::pattern::Pattern::compile(r"\.client\.ts$")
                        .expect("valid pattern"),
                ),
                imported_from: None,
                only_in: Scope::compile(std::iter::empty::<&str>()).expect("valid scope"),
            },
        })
        .expect("a chokepoint rule");

        assert!(!check(&engine, &facts("src/orders/a.client.ts", &["fetch"], &[])).is_empty());
        assert!(
            check(&engine, &facts("src/orders/a.server.ts", &["fetch"], &[])).is_empty(),
            "the rule does not govern this file at all"
        );
    }

    /// Issue #145. *"Nothing outside `features/checkout` renders
    /// `CheckoutForm`"* is not an import question: rendering and importing are
    /// different relationships, and a component reached through a barrel or
    /// passed as a prop comes apart from its import exactly where it matters.
    #[test]
    fn an_element_rendered_outside_its_chokepoint_is_reported() {
        let engine = rendering_engine(&["CheckoutForm"], &["src/features/checkout/**"]);

        let elsewhere = rendering("src/features/orders/page.tsx", &["div", "CheckoutForm"]);
        let reported = check(&engine, &elsewhere);

        assert_eq!(
            reported.first().map(|f| &f.observed),
            Some(&Observed::ChokepointBreached {
                callee: "CheckoutForm".to_owned(),
                rendered: true,
            }),
            "{reported:?}"
        );
        // `renders`, not `reaches`: the reader is being sent to markup, and a
        // sentence about a call site would send them looking for one that is
        // not there.
        assert!(reported[0].span.is_some());

        // And the feature that owns it renders it freely.
        let owner = rendering("src/features/checkout/page.tsx", &["CheckoutForm"]);
        assert!(check(&engine, &owner).is_empty());
    }

    /// A render is a *different* use from a call. `<Card />` compiles to one,
    /// and a rule about a capability must not start firing on markup.
    #[test]
    fn a_call_and_a_render_are_guarded_separately() {
        let by_render = rendering_engine(&["Card"], &["src/features/checkout/**"]);
        let mut called = facts("src/features/orders/page.tsx", &["Card"], &[]);
        called.renders.clear();
        assert!(
            check(&by_render, &called).is_empty(),
            "a rule guarding a render does not guard a call of the same name"
        );

        let by_call = engine();
        let rendered = rendering("src/orders/place.ts", &["process.env"]);
        assert!(
            check(&by_call, &rendered).is_empty(),
            "and the reverse: a rule guarding a call does not guard markup"
        );
    }

    /// Matched exactly, and **not** by the dot-prefix rule `callee` uses.
    /// `Ui.Button` is one component, not a member of a `Ui` capability.
    #[test]
    fn an_element_name_is_matched_whole() {
        let engine = rendering_engine(&["Ui"], &["src/features/checkout/**"]);
        let nested = rendering("src/features/orders/page.tsx", &["Ui.Button"]);

        assert!(check(&engine, &nested).is_empty(), "{nested:?}");

        let exact = rendering_engine(&["Ui.Button"], &["src/features/checkout/**"]);
        assert!(!check(&exact, &nested).is_empty());
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
            directives: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Chokepoint {
                callee: vec!["process.env".to_owned()],
                renders: Vec::new(),
                file_pattern: None,
                imported_from: None,
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
            renders: Vec::new(),
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
