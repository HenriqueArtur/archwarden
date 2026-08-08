//! The `pair` rule: a file of one kind must have a companion of another.
//!
//! `spec-pair` is this rule for one specific pair and cannot be bent to any
//! other. Its default ignores exclude anything that is not a JS/TS source file,
//! by construction and for a good reason — a PNG needs no test — and its
//! companion is *derived*, `<stem>.<marker>.<ext>`, which is a good convention
//! for tests and generalises to nothing. Two fixed names in one directory is
//! what the rest of the world has.
//!
//! The difference from `presence` is the anchor. That rule asks about a
//! *directory*: these files must be here, whatever else is. This one asks about
//! a *file*: because this one exists, that one must too. So an empty directory
//! is a `presence` finding and not a `pair` one, and a companion may be named
//! relative to the file — including `../projeto.md`, which no directory-scoped
//! rule can reach. See `docs/RULES.md`.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    finding::{Expectation, Finding, Observed},
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    pattern::Pattern,
    scope::Scope,
    traits::{FileContext, RuleEngine},
};

/// A compiled `pair` rule.
#[derive(Debug, Clone)]
pub struct PairEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    file_pattern: Pattern,
    must_exist: String,
}

impl PairEngine {
    /// Builds an engine from a compiled rule.
    ///
    /// Returns `None` for a rule of any other kind.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::Pair {
            file_pattern,
            must_exist,
        } = &rule.kind
        else {
            return None;
        };

        Some(Self::build(rule, file_pattern, must_exist))
    }

    /// Builds an engine from a rule whose kind is already known.
    pub(crate) fn build(rule: &CompiledRule, file_pattern: &Pattern, must_exist: &str) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            file_pattern: file_pattern.clone(),
            must_exist: must_exist.to_owned(),
        }
    }

    /// Where this file's companion would be, if the rule reaches this file.
    ///
    /// Purely lexical: `dirname`, then the relative path resolved against it.
    /// No disk, which is what lets `describe` and `scaffold` answer for a file
    /// that has not been written yet.
    fn companion_of(&self, path: &RepoRelPath) -> Option<RepoRelPath> {
        if !self.scope.contains_file(path.as_path()) {
            return None;
        }
        if !self.file_pattern.is_match(path.file_name()?) {
            return None;
        }

        resolve(&path.parent()?, &self.must_exist)
    }

    fn expectation(companion: RepoRelPath) -> Expectation {
        Expectation::RequiredCompanion { path: companion }
    }
}

/// Resolves a relative path against a directory, honouring `..` and `.`.
///
/// `None` when it would climb above the repository root. A rule that reached
/// outside the repository would be asking about a file nothing here governs,
/// and answering "missing" for it would be a finding nobody could act on.
fn resolve(directory: &RepoRelPath, relative: &str) -> Option<RepoRelPath> {
    let mut segments: Vec<&str> = directory
        .as_str()
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();

    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }

    RepoRelPath::new(segments.join("/")).ok()
}

impl RuleEngine for PairEngine {
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
        self.companion_of(path).is_some()
    }

    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        let Some(companion) = self.companion_of(ctx.path) else {
            return Vec::new();
        };

        if ctx.exists.at(&companion) {
            return Vec::new();
        }

        vec![Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            // The file that needs the companion, not the companion. It is the
            // one that exists, and it is the one an editor can open.
            path: ctx.path.clone(),
            span: None,
            observed: Observed::CompanionMissing {
                path: companion.clone(),
            },
            expected: Self::expectation(companion),
        }]
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        self.companion_of(path)
            .map(Self::expectation)
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::traits::Exists;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn engine(scope: &[&str], file_pattern: &str, must_exist: &str) -> PairEngine {
        let rule = CompiledRule {
            id: RuleId::new("licao-tem-notas").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            level: Level::Error,
            scope: Scope::compile(scope).expect("valid scope"),
            kind: CompiledRuleKind::Pair {
                file_pattern: Pattern::compile(file_pattern).expect("valid pattern"),
                must_exist: must_exist.to_owned(),
            },
        };

        PairEngine::from_rule(&rule).expect("is a pair rule")
    }

    /// Checks `file` against a repository holding exactly `present`.
    fn check(engine: &PairEngine, file: &str, present: &[&str]) -> Vec<Finding> {
        let there: Vec<String> = present.iter().map(|p| (*p).to_owned()).collect();
        let exists = |candidate: &RepoRelPath| there.iter().any(|p| p == candidate.as_str());

        engine.check_file(FileContext {
            path: &path(file),
            facts: None,
            siblings: &[],
            exists: Exists::new(&exists),
        })
    }

    /// The rule issue #45 was filed with. The separation exists so a lesson can
    /// be rewritten without destroying what was written while doing it, and it
    /// only works if the notes file is there.
    #[test]
    fn a_file_whose_companion_is_beside_it_passes() {
        let engine = engine(&["projetos/*"], r"^projeto\.md$", "notas.md");

        assert!(
            check(
                &engine,
                "projetos/03-semaforo/projeto.md",
                &[
                    "projetos/03-semaforo/projeto.md",
                    "projetos/03-semaforo/notas.md",
                ],
            )
            .is_empty()
        );
    }

    /// The destructive case: with no `notas.md`, the next generated pass writes
    /// over the directory as if there were nothing to preserve, and the failure
    /// looks exactly like "I hadn't taken notes on that one yet".
    #[test]
    fn a_missing_companion_is_reported_on_the_file_that_needs_it() {
        let engine = engine(&["projetos/*"], r"^projeto\.md$", "notas.md");

        let findings = check(
            &engine,
            "projetos/03-semaforo/projeto.md",
            &["projetos/03-semaforo/projeto.md"],
        );

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].observed,
            Observed::CompanionMissing {
                path: path("projetos/03-semaforo/notas.md")
            }
        );
        // The file that needs it, which is the one an editor can open.
        assert_eq!(findings[0].path, path("projetos/03-semaforo/projeto.md"));
    }

    /// One direction, always. An orphan `notas.md` is a note taken before the
    /// lesson was written, which is fine and is not a finding.
    #[test]
    fn the_companion_never_needs_the_file_back() {
        let engine = engine(&["projetos/*"], r"^projeto\.md$", "notas.md");

        assert!(
            check(
                &engine,
                "projetos/03-semaforo/notas.md",
                &["projetos/03-semaforo/notas.md"],
            )
            .is_empty()
        );
        assert!(!engine.applies_to(&path("projetos/03-semaforo/notas.md")));
    }

    /// The half no directory-scoped rule can reach: a sketch needs the lesson
    /// one level up, and the sketch may be called anything.
    #[test]
    fn a_companion_may_sit_outside_the_directory() {
        let engine = engine(&["projetos/*/sketch"], r"\.ino$", "../projeto.md");

        assert!(
            check(
                &engine,
                "projetos/03-semaforo/sketch/semaforo.ino",
                &[
                    "projetos/03-semaforo/sketch/semaforo.ino",
                    "projetos/03-semaforo/projeto.md",
                ],
            )
            .is_empty()
        );

        let findings = check(
            &engine,
            "projetos/03-semaforo/sketch/semaforo.ino",
            &["projetos/03-semaforo/sketch/semaforo.ino"],
        );
        assert_eq!(
            findings.first().expect("one").observed,
            Observed::CompanionMissing {
                path: path("projetos/03-semaforo/projeto.md")
            }
        );
    }

    /// A companion that would climb above the repository root is a file this
    /// repository does not govern, so there is nothing to report about it.
    #[test]
    fn a_companion_above_the_root_puts_the_file_outside_the_rule() {
        let engine = engine(&["src"], r"\.md$", "../../elsewhere.md");

        assert!(!engine.applies_to(&path("src/x.md")));
        assert!(check(&engine, "src/x.md", &[]).is_empty());
    }

    /// The difference from `presence`, stated as a test: this rule is
    /// conditional. A directory with no `projeto.md` at all owes no `notas.md`,
    /// because nothing here is unpaired.
    #[test]
    fn a_directory_with_no_source_file_owes_nothing() {
        let engine = engine(&["projetos/*"], r"^projeto\.md$", "notas.md");

        assert!(check(&engine, "projetos/17-nova/rascunho.md", &[]).is_empty());
    }

    #[test]
    fn a_file_outside_the_scope_is_left_alone() {
        let engine = engine(&["projetos/*"], r"^projeto\.md$", "notas.md");
        assert!(check(&engine, "outra-coisa/projeto.md", &[]).is_empty());
    }

    /// Decision 9: the companion is nameable before either file exists, which
    /// is what puts `scaffold` in front of the writing.
    #[test]
    fn the_companion_is_describable_before_the_file_exists() {
        let engine = engine(&["projetos/*"], r"^projeto\.md$", "notas.md");

        assert_eq!(
            engine.describe_expectation(&path("projetos/17-nova/projeto.md")),
            [Expectation::RequiredCompanion {
                path: path("projetos/17-nova/notas.md")
            }]
        );
    }

    /// What `check` demands is what the informant advertises.
    #[test]
    fn what_check_demands_is_what_describe_expectation_advertises() {
        let engine = engine(&["projetos/*"], r"^projeto\.md$", "notas.md");
        let target = "projetos/03-semaforo/projeto.md";

        let findings = check(&engine, target, &[target]);
        let advertised = engine.describe_expectation(&path(target));

        assert_eq!(advertised.first(), Some(&findings[0].expected));
    }

    #[test]
    fn a_rule_of_another_kind_is_declined() {
        let presence = CompiledRule {
            id: RuleId::new("licao-completa").expect("valid"),
            module: None,
            why: None,
            module_why: None,
            level: Level::Error,
            scope: Scope::compile(["projetos/*"]).expect("valid"),
            kind: CompiledRuleKind::Presence {
                require: vec!["projeto.md".to_owned()],
                require_any: Vec::new(),
            },
        };

        assert!(PairEngine::from_rule(&presence).is_none());
    }

    #[test]
    fn the_engine_reports_its_identity() {
        let engine = engine(&["projetos/*"], r"^projeto\.md$", "notas.md");
        assert_eq!(engine.id().as_str(), "licao-tem-notas");
        assert_eq!(engine.module(), None);
        assert_eq!(engine.level(), Level::Error);
    }
}
