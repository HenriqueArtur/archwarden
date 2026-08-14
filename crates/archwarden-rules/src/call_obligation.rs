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
    facts::FileFacts,
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
        } = &rule.kind
        else {
            return None;
        };

        Some(Self::build(rule, file_pattern, symbol, imported_from))
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
    ) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            file_pattern: file_pattern.clone(),
            symbol: symbol.to_owned(),
            imported_from: imported_from.to_owned(),
        }
    }

    /// The name the symbol is rooted at.
    ///
    /// `Event.save` is called through the binding `Event`, so that is what the
    /// import has to provide. A bare `saveEvent` is its own root.
    fn root(&self) -> &str {
        self.symbol
            .split_once('.')
            .map_or(self.symbol.as_str(), |(root, _)| root)
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

    fn expectation(&self) -> Expectation {
        Expectation::RequiredCall {
            symbol: self.symbol.clone(),
            imported_from: self.imported_from.clone(),
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
            return Vec::new();
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
    use archwarden_core::traits::Exists;

    use super::*;
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
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["apps/app/src/app/api/**"]).expect("valid scope"),
            kind: CompiledRuleKind::CallObligation {
                file_pattern: Pattern::compile(r"^route\.(post|put|patch|delete)\.ts$")
                    .expect("valid pattern"),
                symbol: "Event.save".to_owned(),
                imported_from: "@flowmaatik/domain/event".to_owned(),
            },
        }
    }

    fn engine() -> CallObligationEngine {
        CallObligationEngine::from_rule(&rule()).expect("a call-obligation rule")
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
        })
    }

    const EVENT: &[(&str, &[&str], bool)] = &[("@flowmaatik/domain/event", &["Event"], false)];

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
