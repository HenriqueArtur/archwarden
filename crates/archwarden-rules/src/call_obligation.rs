//! The `call-obligation` rule: some files must actually do a thing.
//!
//! The rule no other tool in this space has. Everything else archwarden checks
//! is shape -- where a file sits, what it is called, what it exports, what it
//! reaches. This one is about behaviour: a route that mutates has to record an
//! audit event, and a file that merely *imports* the recorder has not done it.
//!
//! Two failures, deliberately distinguished. "You did not import `Event.save`"
//! and "you imported it and never called it" are different mistakes with
//! different fixes, and a rule that reported one sentence for both would make
//! the reader work out which.
//!
//! See `docs/RULES.md`.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    facts::{CallFact, FileFacts},
    finding::{Expectation, Finding, Observed},
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    pattern::Pattern,
    scope::Scope,
    traits::{FactsNeeded, FileContext, RuleEngine},
};

/// A compiled `call-obligation` rule.
#[derive(Debug, Clone)]
pub struct CallObligationEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    file_pattern: Pattern,
    symbol: String,
    imported_from: String,
    with_options: Vec<(String, Option<String>)>,
}

impl CallObligationEngine {
    /// Builds an engine from a compiled rule.
    ///
    /// Returns `None` for a rule of any other kind.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::CallObligation {
            file_pattern,
            symbol,
            imported_from,
            with_options,
        } = &rule.kind
        else {
            return None;
        };

        Some(Self::build(
            rule,
            file_pattern,
            symbol,
            imported_from,
            with_options,
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
        file_pattern: &Pattern,
        symbol: &str,
        imported_from: &str,
        with_options: &[(String, Option<String>)],
    ) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            file_pattern: file_pattern.clone(),
            symbol: symbol.to_owned(),
            imported_from: imported_from.to_owned(),
            with_options: with_options.to_vec(),
        }
    }

    /// The name the symbol is rooted at.
    ///
    /// `Event.save` is called through the binding `Event`, so that is what the
    /// import has to provide. A bare `saveEvent` is its own root.
    /// The name an import has to bind for the symbol to be reachable.
    ///
    /// `Event.save` needs `Event`, and `Event::save` needs `Event` too: the
    /// separator is the language's, and a rule names its symbol the way its own
    /// language spells it. Splitting on `.` alone read the whole of
    /// `Audit::record` as the root, so a Rust file importing `Audit` and
    /// calling `Audit::record` was reported for importing neither -- which the
    /// first end-to-end run of the Rust front-end found.
    ///
    /// `::` is tried first because `.` never appears before it in a path a rule
    /// would name, and trying `.` first would cut `a.b::c` in the wrong place.
    fn root(&self) -> &str {
        let symbol = self.symbol.as_str();
        symbol
            .split_once("::")
            .or_else(|| symbol.split_once('.'))
            .map_or(symbol, |(root, _)| root)
    }

    /// Whether the file imports the symbol from the module the rule names.
    ///
    /// Matched against the specifier *as written*, not against a resolved
    /// path: `imported_from` says which package a symbol comes from, and the
    /// package name is how a reader writes and recognises it. This is why the
    /// rule needs no resolution pass, unlike `import-boundary`.
    ///
    /// A type-only import does not count. A type cannot be called, so one
    /// would satisfy the import half of a rule whose whole point is the call.
    fn imports_the_symbol(&self, facts: &FileFacts) -> bool {
        facts.imports.iter().any(|import| {
            !import.type_only
                && import.specifier == self.imported_from
                && import.names.iter().any(|name| name == self.root())
        })
    }

    /// Whether the file calls the symbol anywhere.
    ///
    /// Anywhere, and not "on a path reachable from an export". A helper
    /// defined in this file *is* in this file, which is what the plan's
    /// acceptance criterion asks for, and `RULES.md` already declines to
    /// filter unreachable branches.
    fn calls_the_symbol(&self, facts: &FileFacts) -> bool {
        facts.calls.iter().any(|call| call.callee == self.symbol)
    }

    /// The first option the rule asks for that no single call carries.
    ///
    /// One call has to carry all of them. Two calls each carrying half is two
    /// calls, and the rule is a sentence about one -- `factory({ a })` beside
    /// `factory({ b })` is exactly the mixed suite issue #164 is about.
    ///
    /// A value the reader cannot see does not satisfy a rule that names one.
    /// The fact records it as absent rather than guessed, and treating absent
    /// as a match would have the rule pass on a call it cannot read.
    fn option_no_call_carries(&self, facts: &FileFacts) -> Option<&(String, Option<String>)> {
        let satisfying = |call: &&CallFact| {
            call.callee == self.symbol
                && self.with_options.iter().all(|(key, wanted)| {
                    call.options.iter().any(|option| {
                        option.key == *key
                            && match wanted {
                                Some(value) => option.value.as_deref() == Some(value.as_str()),
                                None => true,
                            }
                    })
                })
        };

        if facts.calls.iter().any(|call| satisfying(&call)) {
            return None;
        }

        // Nothing satisfies the whole set, so name the first one missing from
        // the call that came closest -- the reader has one call site to look
        // at and one key to add.
        let closest = facts
            .calls
            .iter()
            .filter(|call| call.callee == self.symbol)
            .max_by_key(|call| {
                self.with_options
                    .iter()
                    .filter(|(key, _)| call.options.iter().any(|option| option.key == *key))
                    .count()
            });

        self.with_options.iter().find(|(key, wanted)| {
            !closest.is_some_and(|call| {
                call.options.iter().any(|option| {
                    option.key == *key
                        && match wanted {
                            Some(value) => option.value.as_deref() == Some(value.as_str()),
                            None => true,
                        }
                })
            })
        })
    }

    fn expectation(&self) -> Expectation {
        Expectation::RequiredCall {
            symbol: self.symbol.clone(),
            imported_from: self.imported_from.clone(),
            with_options: self
                .with_options
                .iter()
                .map(|(key, value)| archwarden_core::finding::RequiredOption {
                    key: key.clone(),
                    value: value.clone(),
                })
                .collect(),
        }
    }

    fn finding(&self, path: &RepoRelPath, observed: Observed) -> Finding {
        Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: path.clone(),
            span: None,
            observed,
            expected: self.expectation(),
        }
    }
}

impl RuleEngine for CallObligationEngine {
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
            && path
                .file_name()
                .is_some_and(|name| self.file_pattern.is_match(name))
    }

    fn needs_facts(&self) -> FactsNeeded {
        FactsNeeded::Code
    }

    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        if !self.applies_to(ctx.path) {
            return Vec::new();
        }
        // No facts means no parser ran. Reporting "never called" would blame
        // the file for the run's own gap.
        let Some(facts) = ctx.facts else {
            return Vec::new();
        };

        // The import is checked first, and its failure ends the check: telling
        // someone their file never calls a symbol it never imported sends them
        // looking for a missing call site instead of a missing import.
        if !self.imports_the_symbol(facts) {
            return vec![self.finding(
                ctx.path,
                Observed::RequiredImportForCallMissing {
                    symbol: self.symbol.clone(),
                    module: self.imported_from.clone(),
                },
            )];
        }

        if self.calls_the_symbol(facts) {
            // The call is there. Whether it says what the rule asks it to say
            // is a different question, and a different place to send the
            // reader: a call site a few characters short, not a missing one.
            return match self.option_no_call_carries(facts) {
                Some((option, value)) => vec![self.finding(
                    ctx.path,
                    Observed::RequiredCallOptionMissing {
                        symbol: self.symbol.clone(),
                        option: option.clone(),
                        value: value.clone(),
                    },
                )],
                None => Vec::new(),
            };
        }

        vec![self.finding(
            ctx.path,
            Observed::RequiredCallMissing {
                symbol: self.symbol.clone(),
            },
        )]
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        if self.applies_to(path) {
            vec![self.expectation()]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use archwarden_core::{facts::CallOption, traits::Exists};

    use super::*;

    /// The root of a symbol is the name an import binds, in either language's
    /// spelling.
    ///
    /// Found by running the Rust front-end end to end: a file importing
    /// `Audit` and calling `Audit::record` was reported for importing neither,
    /// because splitting on `.` alone made the whole of `Audit::record` the
    /// root. Both separators are asserted, and so is a bare name -- a symbol
    /// with no separator is its own root, and an implementation that always
    /// split would return the empty string for it.
    #[test]
    fn a_symbols_root_is_read_in_either_languages_spelling() {
        let root_of = |symbol: &str| {
            CallObligationEngine {
                id: RuleId::new("r").expect("valid id"),
                module: None,
                level: Level::Error,
                scope: Scope::compile(["**"]).expect("valid scope"),
                file_pattern: Pattern::compile(".*").expect("valid pattern"),
                symbol: symbol.to_owned(),
                imported_from: "x".to_owned(),
                with_options: Vec::new(),
            }
            .root()
            .to_owned()
        };

        assert_eq!(root_of("Event.save"), "Event", "the JavaScript separator");
        assert_eq!(root_of("Audit::record"), "Audit", "and the Rust one");
        assert_eq!(root_of("record"), "record", "a bare name is its own root");
        assert_eq!(root_of("a::b::c"), "a", "the first segment, not the last");
    }
    use archwarden_core::{
        facts::{CallFact, ImportFact, Span},
        hash::ContentHash,
    };

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// The rule from `docs/CONFIG.md`: a mutating route must record an audit
    /// event.
    fn rule() -> CompiledRule {
        CompiledRule {
            id: RuleId::new("non-get-routes-must-audit").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["apps/app/src/app/api/**"]).expect("valid scope"),
            kind: CompiledRuleKind::CallObligation {
                file_pattern: Pattern::compile(r"^route\.(post|put|patch|delete)\.ts$")
                    .expect("valid pattern"),
                symbol: "Event.save".to_owned(),
                imported_from: "@flowmaatik/domain/event".to_owned(),
                with_options: Vec::new(),
            },
        }
    }

    fn engine() -> CallObligationEngine {
        CallObligationEngine::from_rule(&rule()).expect("a call-obligation rule")
    }

    /// The same rule, asking for options. Issue #164.
    fn engine_wanting(options: &[(&str, Option<&str>)]) -> CallObligationEngine {
        let mut rule = rule();
        if let CompiledRuleKind::CallObligation { with_options, .. } = &mut rule.kind {
            *with_options = options
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.map(str::to_owned)))
                .collect();
        }
        CallObligationEngine::from_rule(&rule).expect("a call-obligation rule")
    }

    /// Facts whose one call carries an options bag.
    fn facts_calling_with(callee: &str, options: &[(&str, Option<&str>)]) -> FileFacts {
        let mut facts = facts(EVENT, &[]);
        facts.calls.push(CallFact {
            arguments: Vec::new(),
            options: options
                .iter()
                .map(|(key, value)| CallOption {
                    key: (*key).to_owned(),
                    value: value.map(str::to_owned),
                })
                .collect(),
            callee: callee.to_owned(),
            span: Span::new(100, 120),
        });
        facts
    }

    /// Facts for a route file: `imports` as `(specifier, names, type_only)`,
    /// `calls` as callee paths.
    fn facts(imports: &[(&str, &[&str], bool)], calls: &[&str]) -> FileFacts {
        let mut facts = FileFacts::unparsed(
            path("apps/app/src/app/api/clients/route.post.ts"),
            ContentHash::of(b"source"),
        );
        for (specifier, names, type_only) in imports {
            facts.imports.push(ImportFact {
                specifier: (*specifier).to_owned(),
                resolved: None,
                type_only: *type_only,
                names: names.iter().map(|n| (*n).to_owned()).collect(),
                span: Span::new(0, 40),
            });
        }
        for callee in calls {
            facts.calls.push(CallFact {
                arguments: Vec::new(),
                options: Vec::new(),
                callee: (*callee).to_owned(),
                span: Span::new(100, 120),
            });
        }
        facts
    }

    fn check(engine: &CallObligationEngine, facts: &FileFacts) -> Vec<Finding> {
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

    const EVENT: &[(&str, &[&str], bool)] = &[("@flowmaatik/domain/event", &["Event"], false)];

    /// Issue #164, reported by a repository that found it the expensive way:
    /// of 215 files in a suite that was supposed to be entirely in-memory,
    /// five were not, and the only evidence was a run that took longer than it
    /// should. Same callee, same arity, opposite meaning -- the difference is
    /// an object key.
    #[test]
    fn a_call_without_the_option_the_rule_asks_for_is_reported() {
        let engine = engine_wanting(&[("PAY_IN_MEMORY", None)]);

        assert!(
            check(
                &engine,
                &facts_calling_with("Event.save", &[("PAY_IN_MEMORY", Some("all"))])
            )
            .is_empty(),
            "the key is there"
        );

        let missing = check(
            &engine,
            &facts_calling_with("Event.save", &[("cache", None)]),
        );
        assert_eq!(
            missing.first().map(|f| &f.observed),
            Some(&Observed::RequiredCallOptionMissing {
                symbol: "Event.save".to_owned(),
                option: "PAY_IN_MEMORY".to_owned(),
                value: None,
            }),
            "{missing:?}"
        );
    }

    /// Presence is a different question from value, and a rule that only
    /// wants presence must not have to name a value it does not care about.
    #[test]
    fn a_key_asked_for_by_presence_is_satisfied_by_any_value() {
        let engine = engine_wanting(&[("PAY_IN_MEMORY", None)]);

        for value in [Some("all"), Some("none"), None] {
            assert!(
                check(
                    &engine,
                    &facts_calling_with("Event.save", &[("PAY_IN_MEMORY", value)])
                )
                .is_empty(),
                "{value:?}"
            );
        }
    }

    /// And a rule that names one is not satisfied by the key alone -- including
    /// when the value is something the reader cannot see, which is absent
    /// rather than assumed to match.
    #[test]
    fn a_key_asked_for_by_value_is_not_satisfied_by_presence() {
        let engine = engine_wanting(&[("PAY_IN_MEMORY", Some("all"))]);

        assert!(
            check(
                &engine,
                &facts_calling_with("Event.save", &[("PAY_IN_MEMORY", Some("all"))])
            )
            .is_empty()
        );

        for wrong in [Some("none"), None] {
            let reported = check(
                &engine,
                &facts_calling_with("Event.save", &[("PAY_IN_MEMORY", wrong)]),
            );
            assert_eq!(
                reported.first().map(|f| &f.observed),
                Some(&Observed::RequiredCallOptionMissing {
                    symbol: "Event.save".to_owned(),
                    option: "PAY_IN_MEMORY".to_owned(),
                    value: Some("all".to_owned()),
                }),
                "{wrong:?}: {reported:?}"
            );
        }
    }

    /// One call has to carry all of them. Two calls each carrying half is two
    /// calls, and the rule is a sentence about one.
    #[test]
    fn the_options_have_to_meet_on_one_call() {
        let engine = engine_wanting(&[("PAY_IN_MEMORY", None), ("strict", Some("true"))]);

        let together = facts_calling_with(
            "Event.save",
            &[("PAY_IN_MEMORY", Some("all")), ("strict", Some("true"))],
        );
        assert!(check(&engine, &together).is_empty());

        let mut apart = facts_calling_with("Event.save", &[("PAY_IN_MEMORY", Some("all"))]);
        apart.calls.push(CallFact {
            arguments: Vec::new(),
            options: vec![CallOption::holding("strict", "true")],
            callee: "Event.save".to_owned(),
            span: Span::new(200, 220),
        });
        assert!(!check(&engine, &apart).is_empty(), "{apart:?}");
    }

    /// It has to be *this* callee carrying it. A different call in the same
    /// file passing the key is a different call, and a rule satisfied by one
    /// would pass every spec that happens to mention the word somewhere.
    #[test]
    fn another_callee_carrying_the_option_does_not_satisfy_it() {
        let engine = engine_wanting(&[("PAY_IN_MEMORY", None)]);

        let mut facts = facts_calling_with("Event.save", &[]);
        facts.calls.push(CallFact {
            arguments: Vec::new(),
            options: vec![CallOption::holding("PAY_IN_MEMORY", "all")],
            callee: "somethingElse".to_owned(),
            span: Span::new(300, 320),
        });

        let reported = check(&engine, &facts);
        assert_eq!(
            reported.first().map(|f| &f.observed),
            Some(&Observed::RequiredCallOptionMissing {
                symbol: "Event.save".to_owned(),
                option: "PAY_IN_MEMORY".to_owned(),
                value: None,
            }),
            "{reported:?}"
        );
    }

    /// When several calls to the symbol each fall short, the one named is the
    /// key missing from the call that came closest -- so the reader has one
    /// call site to open and one key to add, rather than a list to reconcile.
    #[test]
    fn the_key_named_is_the_one_missing_from_the_closest_call() {
        let engine = engine_wanting(&[("PAY_IN_MEMORY", None), ("strict", None)]);

        // The first call carries an option the rule never mentions, which is
        // no closer than carrying none: the count is of keys the rule asked
        // for, not of keys the call happens to have.
        let mut facts = facts_calling_with("Event.save", &[("cache", Some("false"))]);
        facts.calls.push(CallFact {
            arguments: Vec::new(),
            options: vec![CallOption::holding("PAY_IN_MEMORY", "all")],
            callee: "Event.save".to_owned(),
            span: Span::new(300, 320),
        });

        let reported = check(&engine, &facts);
        assert_eq!(
            reported.first().map(|f| &f.observed),
            Some(&Observed::RequiredCallOptionMissing {
                symbol: "Event.save".to_owned(),
                option: "strict".to_owned(),
                value: None,
            }),
            "the call holding half of what the rule asked for is the closest \
             one, and `strict` is what it is missing: {reported:?}"
        );
    }

    /// A rule that asks for no options is exactly the rule it was before, and
    /// a call carrying a bag it never mentioned satisfies it.
    #[test]
    fn a_rule_that_asks_for_no_options_is_unchanged() {
        assert!(
            check(
                &engine(),
                &facts_calling_with("Event.save", &[("anything", Some("at all"))])
            )
            .is_empty()
        );
    }

    /// The satisfied case: imported and called.
    #[test]
    fn an_imported_and_called_symbol_satisfies_the_rule() {
        assert!(check(&engine(), &facts(EVENT, &["Event.save"])).is_empty());
    }

    /// The failure the rule exists for: the recorder is on hand and never
    /// used. Importing something is not doing it.
    #[test]
    fn importing_without_calling_is_reported() {
        let findings = check(&engine(), &facts(EVENT, &["console.log"]));

        assert_eq!(findings.len(), 1);
        let finding = findings.first().expect("one finding");
        assert_eq!(
            finding.observed,
            Observed::RequiredCallMissing {
                symbol: "Event.save".to_owned()
            }
        );
        assert_eq!(
            finding.expected,
            Expectation::RequiredCall {
                symbol: "Event.save".to_owned(),
                imported_from: "@flowmaatik/domain/event".to_owned(),
                with_options: Vec::new(),
            }
        );
    }

    /// The other failure, kept distinct on purpose. Telling someone their file
    /// never calls a symbol it never imported sends them hunting for a missing
    /// call site instead of a missing import.
    #[test]
    fn a_missing_import_is_its_own_failure_and_stops_there() {
        let findings = check(&engine(), &facts(&[], &[]));

        assert_eq!(findings.len(), 1, "one failure, not two");
        assert_eq!(
            findings.first().map(|f| &f.observed),
            Some(&Observed::RequiredImportForCallMissing {
                symbol: "Event.save".to_owned(),
                module: "@flowmaatik/domain/event".to_owned(),
            })
        );
    }

    /// Same symbol name, different package. `imported_from` exists precisely
    /// to tell two `Event.save`s apart.
    #[test]
    fn the_same_symbol_from_another_module_does_not_count() {
        let findings = check(
            &engine(),
            &facts(&[("@other/analytics", &["Event"], false)], &["Event.save"]),
        );

        assert!(matches!(
            findings.first().map(|f| &f.observed),
            Some(Observed::RequiredImportForCallMissing { .. })
        ));
    }

    /// The module is right but the binding is not there, which is the shape of
    /// `import { Other } from '@flowmaatik/domain/event'`.
    #[test]
    fn the_right_module_without_the_binding_does_not_count() {
        let findings = check(
            &engine(),
            &facts(
                &[("@flowmaatik/domain/event", &["Other"], false)],
                &["Event.save"],
            ),
        );

        assert!(matches!(
            findings.first().map(|f| &f.observed),
            Some(Observed::RequiredImportForCallMissing { .. })
        ));
    }

    /// A type cannot be called, so a type-only import must not satisfy the
    /// half of the rule whose whole point is the call.
    #[test]
    fn a_type_only_import_does_not_satisfy_the_obligation() {
        let findings = check(
            &engine(),
            &facts(&[("@flowmaatik/domain/event", &["Event"], true)], &[]),
        );

        assert!(matches!(
            findings.first().map(|f| &f.observed),
            Some(Observed::RequiredImportForCallMissing { .. })
        ));
    }

    /// The plan's acceptance criterion: the export delegates to a helper in
    /// the same file, and the helper is what calls the symbol. The obligation
    /// is met, and a rule that demanded the call at the top level would fire
    /// on well-factored code.
    #[test]
    fn a_call_from_a_local_helper_satisfies_the_obligation() {
        // What the parser produces for:
        //     export async function POST() { return handle(); }
        //     async function handle() { Event.save(...); }
        let findings = check(&engine(), &facts(EVENT, &["handle", "Event.save"]));

        assert!(findings.is_empty());
    }

    /// A method chain is matched exactly, so `Event.save` is not satisfied by
    /// `Event.saveDraft` or by a bare `save`.
    #[test]
    fn a_method_chain_is_matched_exactly() {
        for callee in ["Event.saveDraft", "save", "Other.save", "Event.save.later"] {
            let findings = check(&engine(), &facts(EVENT, &[callee]));
            assert!(
                matches!(
                    findings.first().map(|f| &f.observed),
                    Some(Observed::RequiredCallMissing { .. })
                ),
                "`{callee}` should not satisfy `Event.save`"
            );
        }
    }

    /// A bare function has itself as its root, so `saveEvent` needs the import
    /// to bind `saveEvent`.
    #[test]
    fn a_bare_symbol_is_its_own_root() {
        let bare = CallObligationEngine::from_rule(&CompiledRule {
            kind: CompiledRuleKind::CallObligation {
                file_pattern: Pattern::compile(r"^route\.post\.ts$").expect("valid pattern"),
                symbol: "saveEvent".to_owned(),
                imported_from: "@flowmaatik/domain/event".to_owned(),
                with_options: Vec::new(),
            },
            ..rule()
        })
        .expect("a call-obligation rule");

        assert!(
            check(
                &bare,
                &facts(
                    &[("@flowmaatik/domain/event", &["saveEvent"], false)],
                    &["saveEvent"]
                )
            )
            .is_empty()
        );
        assert!(
            !check(
                &bare,
                &facts(
                    &[("@flowmaatik/domain/event", &["Event"], false)],
                    &["saveEvent"]
                )
            )
            .is_empty()
        );
    }

    /// The filename pattern is half the scope: a `route.get.ts` in the same
    /// folder is not a mutating route and the rule has nothing to say about
    /// it.
    #[test]
    fn a_file_the_pattern_does_not_match_is_not_checked() {
        let engine = engine();
        let get = path("apps/app/src/app/api/clients/route.get.ts");

        assert!(!engine.applies_to(&get));
        assert!(engine.describe_expectation(&get).is_empty());

        let mut facts = facts(&[], &[]);
        facts.path = get;
        assert!(check(&engine, &facts).is_empty());
    }

    /// And a matching filename outside the scope is equally not the rule's
    /// business.
    #[test]
    fn a_file_outside_the_scope_is_not_checked() {
        let engine = engine();
        let elsewhere = path("apps/blog/src/route.post.ts");

        assert!(!engine.applies_to(&elsewhere));

        let mut facts = facts(&[], &[]);
        facts.path = elsewhere;
        assert!(check(&engine, &facts).is_empty());
    }

    /// A file the parser never read has no calls to judge.
    #[test]
    fn a_file_without_facts_is_not_judged() {
        let findings = engine().check_file(FileContext {
            path: &path("apps/app/src/app/api/clients/route.post.ts"),
            facts: None,
            docs: None,
            siblings: &[],
            exists: Exists::none(),
            graph: None,
            as_of: archwarden_core::date::Date::EPOCH,
        });

        assert!(findings.is_empty());
    }

    /// `scaffold` and `agent-guide` are built from this, so the rule has to be
    /// able to say what it wants about a file that does not exist yet.
    #[test]
    fn the_expectation_is_describable_for_a_file_that_does_not_exist() {
        assert_eq!(
            engine().describe_expectation(&path("apps/app/src/app/api/new/route.put.ts")),
            vec![Expectation::RequiredCall {
                symbol: "Event.save".to_owned(),
                imported_from: "@flowmaatik/domain/event".to_owned(),
                with_options: Vec::new(),
            }]
        );
    }

    /// The rule reads inside a file, but never asks where an import lands --
    /// `imported_from` is matched against the specifier as written.
    #[test]
    fn the_rule_reads_facts_but_needs_no_resolution() {
        let engine = engine();

        assert_eq!(engine.needs_facts(), FactsNeeded::Code);
        assert!(!engine.needs_resolution());
        assert_eq!(engine.id().as_str(), "non-get-routes-must-audit");
        assert_eq!(engine.module(), None);
        assert_eq!(engine.level(), Level::Error);
    }

    /// A rule declared under a module carries it into the finding, which is
    /// the `[module]` a reader sees in the report.
    #[test]
    fn a_module_reaches_the_finding() {
        let module = ModuleId::new("api").expect("valid module");
        let engine = CallObligationEngine::from_rule(&CompiledRule {
            module: Some(module.clone()),
            ..rule()
        })
        .expect("a call-obligation rule");

        assert_eq!(engine.module(), Some(&module));

        let findings = check(&engine, &facts(&[], &[]));
        assert_eq!(
            findings.first().and_then(|f| f.module_id.as_ref()),
            Some(&module)
        );
    }

    /// A rule of another kind is not this engine's.
    #[test]
    fn a_rule_of_another_kind_builds_nothing() {
        assert!(
            CallObligationEngine::from_rule(&CompiledRule {
                kind: CompiledRuleKind::Structure {
                    allowed_subfolders: Some(Vec::new()),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
                ..rule()
            })
            .is_none()
        );
    }
}
