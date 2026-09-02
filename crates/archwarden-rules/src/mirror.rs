//! The `mirror` rule: a counterpart in a parallel tree, not a sibling.
//!
//! `spec-pair` and `pair` answer *"this file needs a companion"* — and both
//! look in the same directory. Plenty of conventions pair across **parallel
//! trees**:
//!
//! > *"Every entity has a migration."*
//! > *"Every route has a page in the docs."*
//! > *"Tests live in `test/`, mirroring `src/`."*
//!
//! `pair` takes a sibling **name**, so there was no way to say *"the same
//! path, elsewhere, transformed"*. Issue #103.
//!
//! # Two pieces that already existed
//!
//! `presence` proves a file is on disk without parsing anything, and `naming`
//! renders a path from capture groups with transforms. A mirror is the second
//! producing a path for the first to check — no new fact, no parse, path
//! arithmetic and an existence check.
//!
//! # One direction per rule
//!
//! *"Every entity has a migration"* and *"every migration belongs to an
//! entity"* are two claims, and each deserves its own `why`: the first is about
//! completeness, the second about orphans. A flag would put two reasons on one
//! rule and make a reader work out which half fired. So the config says both
//! things out loud, as two rules.
//!
//! # What the template may name
//!
//! `file_pattern`'s capture groups, plus two the path itself provides:
//!
//! - `dirname` — the immediate parent's *name*, which `frontmatter.equals`
//!   already defines the same way;
//! - `subpath` — the directory path from the rule's root down to the file,
//!   which is what a mirror across a nested tree needs and which `dirname`
//!   cannot carry.
//!
//! # And what it never asks
//!
//! Whether the counterpart has anything in it. *"And it must contain a test
//! case"* is `spec-pair`'s question, and it has an answer there already.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    finding::{Expectation, Finding, Observed},
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    pattern::Pattern,
    scope::Scope,
    template,
    traits::{FileContext, RuleEngine},
};

/// A compiled `mirror` rule.
#[derive(Debug, Clone)]
pub struct MirrorEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    file_pattern: Pattern,
    must_exist: String,
}

impl MirrorEngine {
    /// Builds an engine from a compiled rule.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::Mirror {
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

    /// Whether this file is in the rule's population.
    fn covers(&self, path: &RepoRelPath) -> bool {
        self.scope.contains_file(path.as_path())
            && path
                .file_name()
                .is_some_and(|name| self.file_pattern.is_match(name))
    }

    /// The counterpart's path, rendered from this file.
    ///
    /// `None` when the template names a group the pattern does not capture,
    /// which is a config error the `naming` rule reports the same way.
    fn counterpart(&self, path: &RepoRelPath) -> Option<String> {
        let name = path.file_name()?;
        let dirname = path
            .parent()
            .and_then(|parent| parent.file_name().map(ToOwned::to_owned));
        let subpath = self.subpath_of(path);

        let rendered = template::render(&self.must_exist, |group| match group {
            "dirname" => dirname.clone(),
            "subpath" => Some(subpath.clone()),
            other => self
                .file_pattern
                .capture(name, other)
                .map(ToOwned::to_owned),
        })
        .ok()?;

        // An empty `subpath` leaves `test//x.ts`, which names nothing on any
        // filesystem. Collapsed here rather than made the config author's
        // problem: the same template has to work for a file at the root of the
        // mirrored tree and one three directories down, which is the whole
        // reason the group exists.
        Some(collapse_separators(&rendered))
    }

    /// The directory path from the rule's root down to this file.
    ///
    /// Empty when the file sits directly in a root. Computed against the scope
    /// that selected the file rather than against a configured prefix: the
    /// scope is what already decides which files this rule is about, and a
    /// second way of saying where the tree starts is a second thing to get
    /// wrong.
    fn subpath_of(&self, path: &RepoRelPath) -> String {
        let Some(parent) = path.parent() else {
            return String::new();
        };

        // The longest ancestor the scope names as a *root* is where the
        // mirrored tree begins; what is left below it is the subpath.
        let directory = parent.as_str();
        self.scope
            .patterns()
            .iter()
            .filter_map(|glob| glob.split(['*', '?']).next())
            .map(|prefix| prefix.trim_end_matches('/'))
            .filter(|prefix| !prefix.is_empty())
            .filter_map(|prefix| {
                directory
                    .strip_prefix(prefix)
                    .map(|rest| rest.trim_start_matches('/').to_owned())
            })
            .min_by_key(String::len)
            .unwrap_or_else(|| directory.to_owned())
    }
}

/// `a//b` → `a/b`, and a leading or trailing separator dropped.
fn collapse_separators(path: &str) -> String {
    let joined: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    joined.join("/")
}

impl RuleEngine for MirrorEngine {
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
        self.covers(path)
    }

    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        if !self.covers(ctx.path) {
            return Vec::new();
        }
        let Some(counterpart) = self.counterpart(ctx.path) else {
            return Vec::new();
        };

        let Ok(wanted) = RepoRelPath::new(&counterpart) else {
            return Vec::new();
        };
        if ctx.exists.at(&wanted) {
            return Vec::new();
        }

        vec![Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: ctx.path.clone(),
            span: None,
            observed: Observed::CounterpartMissing {
                expected: counterpart.clone(),
            },
            expected: Expectation::RequiredCounterpart { path: counterpart },
        }]
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        if !self.covers(path) {
            return Vec::new();
        }
        self.counterpart(path)
            .map(|path| vec![Expectation::RequiredCounterpart { path }])
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::traits::Exists;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn engine(roots: &[&str], file_pattern: &str, must_exist: &str) -> MirrorEngine {
        let rule = CompiledRule {
            id: RuleId::new("entities-have-migrations").expect("valid id"),
            module: None,
            why: None,
            not_yet: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Error,
            scope: Scope::compile(roots.iter().copied()).expect("valid scope"),
            kind: CompiledRuleKind::Mirror {
                file_pattern: Pattern::compile(file_pattern).expect("valid pattern"),
                must_exist: must_exist.to_owned(),
            },
        };
        MirrorEngine::from_rule(&rule).expect("the kind matches")
    }

    /// Checks a file against a repository holding exactly `present`.
    fn check(engine: &MirrorEngine, at: &str, present: &[&str]) -> Vec<Finding> {
        let owned: Vec<String> = present.iter().map(|p| (*p).to_owned()).collect();
        let predicate =
            move |candidate: &RepoRelPath| owned.iter().any(|p| p == candidate.as_str());
        let target = path(at);
        engine.check_file(FileContext {
            path: &target,
            facts: None,
            docs: None,
            siblings: &[],
            exists: Exists::new(&predicate),
            graph: None,
            as_of: archwarden_core::date::Date::EPOCH,
        })
    }

    /// The issue's first example, verbatim: an entity needs a migration in a
    /// parallel tree, which `pair` could not say because it takes a sibling
    /// name. Issue #103.
    #[test]
    fn a_counterpart_in_a_parallel_tree_is_found() {
        let rule = engine(
            &["src/entities"],
            r"^(?<name>[a-z-]+)\.ts$",
            "migrations/{{raw(name)}}.sql",
        );

        assert!(
            check(
                &rule,
                "src/entities/user.ts",
                &["src/entities/user.ts", "migrations/user.sql"]
            )
            .is_empty()
        );
    }

    /// And its absence is the finding, naming the path that should exist —
    /// which is what makes it actionable rather than a puzzle.
    #[test]
    fn a_missing_counterpart_names_the_path_it_wanted() {
        let rule = engine(
            &["src/entities"],
            r"^(?<name>[a-z-]+)\.ts$",
            "migrations/{{raw(name)}}.sql",
        );

        let found = check(&rule, "src/entities/user.ts", &["src/entities/user.ts"]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            found[0].observed,
            Observed::CounterpartMissing {
                expected: "migrations/user.sql".to_owned(),
            }
        );
    }

    /// **The group the issue left open**, and the line it names in its own
    /// heading: tests in a parallel tree, mirroring a nested source tree.
    /// `dirname` gives `b` and cannot carry `a/b`.
    #[test]
    fn subpath_carries_the_whole_directory_path_below_the_root() {
        let rule = engine(
            &["src/**"],
            r"^(?<name>[a-z-]+)\.ts$",
            "test/{{raw(subpath)}}/{{raw(name)}}.test.ts",
        );

        assert!(
            check(
                &rule,
                "src/a/b/x.ts",
                &["src/a/b/x.ts", "test/a/b/x.test.ts"]
            )
            .is_empty()
        );

        let found = check(&rule, "src/a/b/x.ts", &["src/a/b/x.ts"]);
        assert_eq!(
            found[0].observed,
            Observed::CounterpartMissing {
                expected: "test/a/b/x.test.ts".to_owned(),
            }
        );
    }

    /// A file directly in the root has an empty `subpath`, and the template
    /// must still render a usable path rather than `test//x.test.ts`.
    #[test]
    fn an_empty_subpath_does_not_leave_a_double_separator() {
        let rule = engine(
            &["src/**"],
            r"^(?<name>[a-z-]+)\.ts$",
            "test/{{raw(subpath)}}/{{raw(name)}}.test.ts",
        );

        let found = check(&rule, "src/x.ts", &["src/x.ts"]);
        assert_eq!(
            found[0].observed,
            Observed::CounterpartMissing {
                expected: "test/x.test.ts".to_owned(),
            }
        );
    }

    /// `dirname` is the immediate parent's *name*, as `frontmatter.equals`
    /// already defines it — the two groups are different questions and a rule
    /// may want either.
    #[test]
    fn dirname_is_the_parents_name_and_subpath_is_the_path() {
        let by_name = engine(
            &["src/**"],
            r"^(?<name>[a-z-]+)\.ts$",
            "docs/{{raw(dirname)}}.md",
        );

        let found = check(&by_name, "src/a/b/x.ts", &["src/a/b/x.ts"]);
        assert_eq!(
            found[0].observed,
            Observed::CounterpartMissing {
                expected: "docs/b.md".to_owned(),
            }
        );
    }

    /// The transforms `naming` already has work here too: the renderer is the
    /// same one, which is what made this rule small.
    #[test]
    fn a_transform_applies_to_a_captured_group() {
        let rule = engine(
            &["src/entities"],
            r"^(?<name>[a-z-]+)\.ts$",
            "docs/{{pascal(name)}}.md",
        );

        let found = check(&rule, "src/entities/user-account.ts", &[]);
        assert_eq!(
            found[0].observed,
            Observed::CounterpartMissing {
                expected: "docs/UserAccount.md".to_owned(),
            }
        );
    }

    /// A file the pattern does not match is outside the population, the same
    /// way it is for `pair` — not a finding, and not an expectation either.
    #[test]
    fn a_file_the_pattern_does_not_match_is_outside_the_rule() {
        let rule = engine(
            &["src/entities"],
            r"^(?<name>[a-z-]+)\.ts$",
            "migrations/{{raw(name)}}.sql",
        );

        assert!(check(&rule, "src/entities/README.md", &[]).is_empty());
        assert!(
            rule.describe_expectation(&path("src/entities/README.md"))
                .is_empty()
        );
    }

    /// Both halves of the population, tested apart. A file the pattern
    /// matches *outside* the scope is not this rule's business, and a file
    /// inside the scope the pattern does not match is not either — and only
    /// asserting the second leaves the first free to be wrong, because a
    /// template rendered from a name that does not match produces nothing
    /// anyway and hides it.
    #[test]
    fn the_population_is_the_scope_and_the_pattern_together() {
        let rule = engine(
            &["src/entities"],
            r"^(?<name>[a-z-]+)\.ts$",
            "migrations/{{raw(name)}}.sql",
        );

        // Matches the pattern, wrong tree.
        assert!(!rule.applies_to(&path("other/user.ts")));
        assert!(check(&rule, "other/user.ts", &[]).is_empty());
        assert!(rule.describe_expectation(&path("other/user.ts")).is_empty());

        // Right tree, wrong name.
        assert!(!rule.applies_to(&path("src/entities/README.md")));

        // Both, which is the only combination in the population.
        assert!(rule.applies_to(&path("src/entities/user.ts")));
    }

    /// The other direction is a second rule, which is the decision: an orphan
    /// migration is a different claim with a different reason, and one rule
    /// with a flag would put two `why`s on one line.
    #[test]
    fn the_other_direction_is_an_ordinary_second_rule() {
        let orphans = engine(
            &["migrations"],
            r"^(?<name>[a-z-]+)\.sql$",
            "src/entities/{{raw(name)}}.ts",
        );

        let found = check(&orphans, "migrations/ghost.sql", &["migrations/ghost.sql"]);
        assert_eq!(
            found[0].observed,
            Observed::CounterpartMissing {
                expected: "src/entities/ghost.ts".to_owned(),
            }
        );
    }

    /// Answerable before the file exists, so the pre-write hook can say what
    /// else the write will owe.
    #[test]
    fn it_answers_about_a_file_that_does_not_exist_yet() {
        let rule = engine(
            &["src/entities"],
            r"^(?<name>[a-z-]+)\.ts$",
            "migrations/{{raw(name)}}.sql",
        );

        assert_eq!(
            rule.describe_expectation(&path("src/entities/invoice.ts")),
            [Expectation::RequiredCounterpart {
                path: "migrations/invoice.sql".to_owned(),
            }]
        );
    }

    /// A template naming a group nothing captures renders nothing, and the
    /// rule reports nothing rather than inventing a path. `config doctor` is
    /// where a template that names the wrong group is complained about.
    #[test]
    fn a_template_naming_an_unknown_group_reports_nothing() {
        let rule = engine(
            &["src/entities"],
            r"^(?<name>[a-z-]+)\.ts$",
            "migrations/{{raw(missing)}}.sql",
        );

        assert!(check(&rule, "src/entities/user.ts", &[]).is_empty());
        assert!(
            rule.describe_expectation(&path("src/entities/user.ts"))
                .is_empty()
        );
    }

    /// The accessors every surface reads a finding through, and a rule of
    /// another kind builds no engine.
    #[test]
    fn an_engine_carries_its_module_and_refuses_another_kind() {
        let rule = CompiledRule {
            id: RuleId::new("entities-have-migrations").expect("valid id"),
            module: Some(ModuleId::new("entities").expect("valid module")),
            why: None,
            not_yet: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Warning,
            scope: Scope::compile(["src/entities"]).expect("valid scope"),
            kind: CompiledRuleKind::Mirror {
                file_pattern: Pattern::compile(r"^(?<name>[a-z-]+)\.ts$").expect("valid"),
                must_exist: "migrations/{{raw(name)}}.sql".to_owned(),
            },
        };

        let built = MirrorEngine::from_rule(&rule).expect("the kind matches");
        assert_eq!(built.module().map(ModuleId::as_str), Some("entities"));
        assert_eq!(built.id().as_str(), "entities-have-migrations");
        assert_eq!(built.level(), Level::Warning);

        let other = CompiledRule {
            kind: CompiledRuleKind::Frozen,
            ..rule
        };
        assert!(MirrorEngine::from_rule(&other).is_none());
    }
}
