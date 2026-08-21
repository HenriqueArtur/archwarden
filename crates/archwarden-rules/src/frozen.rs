//! The `frozen` rule: a directory that has stopped growing.
//!
//! `import-boundary` can forbid **importing** something. Nothing could forbid
//! **adding** to it — and that is half of every migration ADR:
//!
//! > *"The legacy module is closed for extension. New code goes in
//! > `packages/core`."*
//!
//! # It is `baseline` pointed forward
//!
//! The engine is the smallest in this crate, and that is the whole idea rather
//! than a shortcut: **every file under the scope is a finding.** Which of them
//! are *accepted* is `baseline`'s to say, and `baseline` already accepts by
//! rule and path. So the rule adds no new machinery — it points the machinery
//! that records what a repository has accepted forward instead of back:
//!
//! > every file under these roots is a finding; today's are accepted;
//! > tomorrow's are not.
//!
//! It also turns `baseline` from a record of debt into a statement of intent,
//! which is a better thing for it to be. Issue #102.
//!
//! # What it never does
//!
//! **Read `git`.** archwarden answers from a working tree and a committed
//! baseline. A freeze that consulted history would answer differently in CI
//! than on a laptop, and would stop working in a shallow clone.
//!
//! **Exempt a move.** `legacy/a.ts → legacy/sub/a.ts` is reported: a module
//! closed for extension is one that has stopped, and reshuffling it is not
//! stopping. A move *out* is silent, which is the point of the freeze. When a
//! move within is deliberate, `archwarden baseline` accepts it — and the diff
//! reads as one move rather than a removal and an addition, because `baseline`
//! already pairs those.
//!
//! **Ask what kind of file it is.** A directory that has stopped growing has
//! stopped growing. `ignore` is for what is deliberately outside the
//! architecture, and `archwarden-allow` is the door for the one urgent
//! exception — written beside the file that needed it rather than argued in a
//! pull request.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    finding::{Expectation, Finding, Observed},
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    scope::Scope,
    traits::{FileContext, RuleEngine},
};

/// A compiled `frozen` rule.
#[derive(Debug, Clone)]
pub struct FrozenEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
}

impl FrozenEngine {
    /// Builds an engine from a compiled rule.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        matches!(rule.kind, CompiledRuleKind::Frozen).then(|| Self::build(rule))
    }

    /// Builds an engine from a rule whose kind is already known.
    pub(crate) fn build(rule: &CompiledRule) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
        }
    }
}

impl RuleEngine for FrozenEngine {
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

    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        if !self.applies_to(ctx.path) {
            return Vec::new();
        }

        vec![Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: ctx.path.clone(),
            span: None,
            observed: Observed::FileInFrozenTree,
            expected: Expectation::NoNewFiles,
        }]
    }

    /// Answerable about a file that does not exist yet, which is where this
    /// rule earns most of its keep: an agent about to create
    /// `packages/legacy/new-thing.ts` is told before it writes, rather than
    /// after CI runs.
    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        if self.applies_to(path) {
            vec![Expectation::NoNewFiles]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{facts::FileFacts, hash::ContentHash, traits::Exists};

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn engine(roots: &[&str]) -> FrozenEngine {
        let rule = CompiledRule {
            id: RuleId::new("legacy-is-closed").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Error,
            scope: Scope::compile(roots.iter().copied()).expect("valid scope"),
            kind: CompiledRuleKind::Frozen,
        };
        FrozenEngine::build(&rule)
    }

    fn check(engine: &FrozenEngine, at: &RepoRelPath) -> Vec<Finding> {
        let facts = FileFacts::unparsed(at.clone(), ContentHash::of(b"x"));
        engine.check_file(FileContext {
            path: at,
            facts: Some(&facts),
            docs: None,
            siblings: &[],
            exists: Exists::none(),
            graph: None,
            as_of: archwarden_core::date::Date::EPOCH,
        })
    }

    /// Every file under the scope is a finding. That is not a shortcut — it is
    /// the design: `baseline` decides which of them are accepted, and it
    /// already accepts by rule and path. Issue #102.
    #[test]
    fn every_file_under_the_scope_is_a_finding() {
        let found = check(
            &engine(&["packages/legacy/**"]),
            &path("packages/legacy/a.ts"),
        );

        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].observed, Observed::FileInFrozenTree);
        assert_eq!(found[0].expected, Expectation::NoNewFiles);
    }

    /// Whatever kind of file it is. A directory that has stopped growing has
    /// stopped growing, and `ignore` is where "deliberately outside the
    /// architecture" is said.
    #[test]
    fn it_does_not_ask_what_kind_of_file_it_is() {
        let frozen = engine(&["packages/legacy/**"]);

        for name in ["a.ts", "README.md", "fixture.json", "logo.png"] {
            let at = path(&format!("packages/legacy/{name}"));
            assert_eq!(check(&frozen, &at).len(), 1, "{name}");
        }
    }

    /// A file outside the freeze is not this rule's business, which is what
    /// makes a move *out* silent — and a move out is the point.
    #[test]
    fn a_file_outside_the_freeze_is_untouched() {
        let frozen = engine(&["packages/legacy/**"]);

        assert!(check(&frozen, &path("packages/core/a.ts")).is_empty());
        assert!(
            frozen
                .describe_expectation(&path("packages/core/a.ts"))
                .is_empty()
        );
    }

    /// A move *within* is reported, because a module closed for extension is
    /// one that has stopped and reshuffling it is not stopping. The engine
    /// cannot tell a move from a new file and does not try: both are a path
    /// nobody accepted, which is the whole of what `baseline` knows.
    #[test]
    fn a_move_within_the_freeze_is_a_new_path_like_any_other() {
        let frozen = engine(&["packages/legacy/**"]);

        assert_eq!(check(&frozen, &path("packages/legacy/sub/a.ts")).len(), 1);
    }

    /// Answerable before the file exists, which is where the rule earns most
    /// of its keep: the pre-write hook refuses the write rather than CI
    /// reporting it afterwards.
    #[test]
    fn it_answers_about_a_file_that_does_not_exist_yet() {
        let frozen = engine(&["packages/legacy/**"]);

        assert_eq!(
            frozen.describe_expectation(&path("packages/legacy/new-thing.ts")),
            [Expectation::NoNewFiles]
        );
    }

    /// The accessors every surface reads a finding through, and a rule of
    /// another kind builds no engine rather than one that reports nothing.
    #[test]
    fn an_engine_carries_its_module_and_refuses_another_kind() {
        let rule = CompiledRule {
            id: RuleId::new("legacy-is-closed").expect("valid id"),
            module: Some(ModuleId::new("legacy").expect("valid module")),
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Warning,
            scope: Scope::compile(["packages/legacy/**"]).expect("valid scope"),
            kind: CompiledRuleKind::Frozen,
        };

        let built = FrozenEngine::from_rule(&rule).expect("the kind matches");
        assert_eq!(built.module().map(ModuleId::as_str), Some("legacy"));
        assert_eq!(built.id().as_str(), "legacy-is-closed");
        assert_eq!(built.level(), Level::Warning);

        let other = CompiledRule {
            kind: CompiledRuleKind::ImportCycle {
                include_type_only: false,
            },
            ..rule
        };
        assert!(FrozenEngine::from_rule(&other).is_none());
    }

    /// A file at the repository root sits in the root directory, which `**`
    /// names like any other -- so a freeze over the whole tree covers it. The
    /// case worth pinning is that this is *answered* rather than panicked
    /// over, since the path has no parent component to speak of.
    #[test]
    fn a_file_at_the_repository_root_is_answered_about() {
        assert_eq!(check(&engine(&["**"]), &path("README.md")).len(), 1);
        assert!(check(&engine(&["packages/**"]), &path("README.md")).is_empty());
    }
}
