//! The `presence` rule: these files must exist in each governed directory.
//!
//! The first rule that reasons about paths that are *not* there. Every other
//! rule opens what exists and asks whether it is right; this one asks whether
//! something is missing, which is the failure nobody notices — a lesson with
//! no exercises still renders, still commits, still appears in the index, and
//! is found weeks later by the person who reaches the end of it.
//!
//! `structure.filename_patterns` is the whitelist half and is not the inverse
//! of this: it is satisfied by an empty directory. See `docs/RULES.md`.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind, SkipDirs},
    finding::{Expectation, Finding, Observed},
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    pattern::Pattern,
    scope::Scope,
    traits::{DirectoryContext, RuleEngine},
};

/// A compiled `presence` rule.
#[derive(Debug, Clone)]
pub struct PresenceEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    require: Vec<String>,
    require_any: Vec<Pattern>,
    skip_dirs: SkipDirs,
}

impl PresenceEngine {
    /// Builds an engine from a compiled rule.
    ///
    /// Returns `None` for a rule of any other kind.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule, skip_dirs: SkipDirs) -> Option<Self> {
        let CompiledRuleKind::Presence {
            require,
            require_any,
        } = &rule.kind
        else {
            return None;
        };

        Some(Self::build(rule, require, require_any, skip_dirs))
    }

    /// Builds an engine from a rule whose kind is already known.
    pub(crate) fn build(
        rule: &CompiledRule,
        require: &[String],
        require_any: &[Pattern],
        skip_dirs: SkipDirs,
    ) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            require: require.to_vec(),
            require_any: require_any.to_vec(),
            skip_dirs,
        }
    }

    /// Whether this rule governs `directory`.
    ///
    /// The `_`-prefixed escape hatch is honoured here as it is for `structure`:
    /// a directory nobody considers part of the module layout is not one that
    /// owes it a set of files.
    fn governs(&self, directory: &RepoRelPath) -> bool {
        !self.skip_dirs.exempts(directory) && self.scope.matches_dir(directory.as_path())
    }

    fn expectation(&self) -> Expectation {
        Expectation::RequiredFiles {
            names: self.require.clone(),
            patterns: self
                .require_any
                .iter()
                .map(|pattern| pattern.as_str().to_owned())
                .collect(),
        }
    }

    fn finding(&self, path: &RepoRelPath, observed: Observed) -> Finding {
        Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: path.clone(),
            // The finding is about a file that is not there, so there is no
            // position to name. The path is the directory, which is the thing
            // that exists and the thing that is incomplete.
            span: None,
            observed,
            expected: self.expectation(),
        }
    }
}

impl RuleEngine for PresenceEngine {
    fn id(&self) -> &RuleId {
        &self.id
    }

    fn module(&self) -> Option<&ModuleId> {
        self.module.as_ref()
    }

    fn level(&self) -> Level {
        self.level
    }

    fn applies_to(&self, _path: &RepoRelPath) -> bool {
        // A directory rule. Nothing it says is about one file, and claiming a
        // file would have `describe <file>` answer with a requirement the
        // reader cannot act on from there.
        false
    }

    fn answers_for_directories(&self) -> bool {
        // Which is the other half of `applies_to` returning false, and the
        // half `config doctor` needs: a rule reaching no file is a symptom for
        // a rule about files and the ordinary state for this one.
        true
    }

    fn check_directory(&self, ctx: DirectoryContext<'_>) -> Vec<Finding> {
        if !self.governs(ctx.path) {
            return Vec::new();
        }

        let mut findings = Vec::new();

        // One finding per missing entry, not one per directory. Each is a
        // separate file to create, which is the shape `spec-pair` already
        // reports a missing sibling in -- and a new, empty directory earning
        // four findings is four things that are genuinely absent. `--summary`
        // is the answer to volume; a merged finding would be the answer to
        // nothing.
        for name in &self.require {
            if !ctx.files.iter().any(|file| file == name) {
                findings.push(self.finding(
                    ctx.path,
                    Observed::RequiredFileMissing { name: name.clone() },
                ));
            }
        }

        for pattern in &self.require_any {
            if !ctx.files.iter().any(|file| pattern.is_match(file)) {
                findings.push(self.finding(
                    ctx.path,
                    Observed::NoFileMatching {
                        pattern: pattern.as_str().to_owned(),
                    },
                ));
            }
        }

        findings
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        // The command that makes this rule worth having: `scaffold
        // projetos/17-nova` printing the four filenames is how a unit of work
        // gets started, which puts archwarden before the writing rather than
        // after it.
        if !self.governs(path) || (self.require.is_empty() && self.require_any.is_empty()) {
            return Vec::new();
        }

        vec![self.expectation()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }

    fn engine(scope: &[&str], require: &[&str], require_any: &[&str]) -> PresenceEngine {
        let rule = CompiledRule {
            id: RuleId::new("licao-completa").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            level: Level::Error,
            scope: Scope::compile(scope).expect("valid scope"),
            kind: CompiledRuleKind::Presence {
                require: owned(require),
                require_any: require_any
                    .iter()
                    .map(|p| Pattern::compile(p).expect("valid pattern"))
                    .collect(),
            },
        };

        PresenceEngine::from_rule(&rule, SkipDirs::default()).expect("is a presence rule")
    }

    fn check(engine: &PresenceEngine, directory: &str, files: &[&str]) -> Vec<Finding> {
        engine.check_directory(DirectoryContext {
            path: &path(directory),
            subdirectories: &[],
            files: &owned(files),
        })
    }

    /// The rule issue #42 was filed with: a lesson is only a lesson if it has
    /// all of its parts.
    #[test]
    fn a_directory_holding_every_required_file_passes() {
        let engine = engine(
            &["projetos/*"],
            &["projeto.md", "exercicios.md", "notas.md"],
            &[],
        );

        assert!(
            check(
                &engine,
                "projetos/03-semaforo",
                &["projeto.md", "exercicios.md", "notas.md", "extra.md"],
            )
            .is_empty()
        );
    }

    /// One finding per missing file. Each is a separate thing to create, which
    /// is how `spec-pair` reports a missing sibling too.
    #[test]
    fn every_missing_file_is_named_on_its_own() {
        let engine = engine(
            &["projetos/*"],
            &["projeto.md", "exercicios.md", "notas.md"],
            &[],
        );

        let findings = check(&engine, "projetos/03-semaforo", &["projeto.md"]);

        assert_eq!(findings.len(), 2);
        assert_eq!(
            findings[0].observed,
            Observed::RequiredFileMissing {
                name: "exercicios.md".to_owned()
            }
        );
        assert_eq!(
            findings[1].observed,
            Observed::RequiredFileMissing {
                name: "notas.md".to_owned()
            }
        );
        // The directory is what exists and what is incomplete.
        assert_eq!(findings[0].path, path("projetos/03-semaforo"));
    }

    /// The destructive case the issue is really about: the notes file is the
    /// one an agent may read and must never write, so a directory without one
    /// is a directory the next generated pass writes over as if there were
    /// nothing to preserve.
    #[test]
    fn a_missing_companion_is_reported_even_when_the_rest_is_there() {
        let engine = engine(&["projetos/*"], &["projeto.md", "notas.md"], &[]);

        let findings = check(&engine, "projetos/03-semaforo", &["projeto.md"]);

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].observed,
            Observed::RequiredFileMissing {
                name: "notas.md".to_owned()
            }
        );
    }

    /// "There has to be a sketch and I do not care what it is called."
    #[test]
    fn a_pattern_is_satisfied_by_any_one_file_that_matches() {
        let engine = engine(&["projetos/*/sketch"], &[], &[r"\.ino$"]);

        assert!(check(&engine, "projetos/03-semaforo/sketch", &["semaforo.ino"]).is_empty());

        let findings = check(&engine, "projetos/03-semaforo/sketch", &["readme.md"]);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].observed,
            Observed::NoFileMatching {
                pattern: r"\.ino$".to_owned()
            }
        );
    }

    /// Two regexes are two requirements, not one satisfied twice.
    #[test]
    fn each_pattern_needs_a_file_of_its_own() {
        let engine = engine(&["projetos/*"], &[], &[r"\.ino$", r"\.json$"]);

        let findings = check(&engine, "projetos/03-semaforo", &["semaforo.ino"]);

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].observed,
            Observed::NoFileMatching {
                pattern: r"\.json$".to_owned()
            }
        );
    }

    /// A brand-new directory is missing everything, and says so. Four findings
    /// for four absent files is four things to do, and `--summary` is the
    /// answer to volume.
    #[test]
    fn an_empty_directory_is_missing_everything() {
        let engine = engine(
            &["projetos/*"],
            &["projeto.md", "exercicios.md", "notas.md"],
            &[r"\.ino$"],
        );

        assert_eq!(check(&engine, "projetos/17-nova", &[]).len(), 4);
    }

    #[test]
    fn a_directory_outside_the_scope_is_left_alone() {
        let engine = engine(&["projetos/*"], &["projeto.md"], &[]);
        assert!(check(&engine, "outra-coisa/qualquer", &[]).is_empty());
    }

    /// The escape hatch reaches here for the same reason it reaches
    /// `structure`: a directory nobody considers part of the layout does not
    /// owe it a set of files.
    #[test]
    fn an_exempt_directory_owes_nothing() {
        let rule = CompiledRule {
            id: RuleId::new("licao-completa").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            level: Level::Error,
            scope: Scope::compile(["projetos/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Presence {
                require: owned(&["projeto.md"]),
                require_any: Vec::new(),
            },
        };
        let engine = PresenceEngine::from_rule(
            &rule,
            SkipDirs {
                prefixes: vec!["_".to_owned()],
                globs: archwarden_core::glob::PathSet::default(),
                scope: archwarden_core::compiled::SkipScope::Structure,
            },
        )
        .expect("is a presence rule");

        assert!(check(&engine, "projetos/_template", &[]).is_empty());
    }

    /// Decision 9, and the half the issue asks for by name: `scaffold
    /// projetos/17-nova` printing the filenames is how a lesson gets started.
    #[test]
    fn the_required_files_are_describable_before_the_directory_exists() {
        let engine = engine(&["projetos/*"], &["projeto.md", "notas.md"], &[r"\.ino$"]);

        assert_eq!(
            engine.describe_expectation(&path("projetos/17-nova")),
            [Expectation::RequiredFiles {
                names: owned(&["projeto.md", "notas.md"]),
                patterns: owned(&[r"\.ino$"]),
            }]
        );
        assert!(
            engine
                .describe_expectation(&path("outra-coisa/qualquer"))
                .is_empty()
        );
    }

    /// What `check` demands is what the informant advertises.
    #[test]
    fn what_check_demands_is_what_describe_expectation_advertises() {
        let engine = engine(&["projetos/*"], &["projeto.md"], &[]);

        let findings = check(&engine, "projetos/03-semaforo", &[]);
        let advertised = engine.describe_expectation(&path("projetos/03-semaforo"));

        assert_eq!(advertised.first(), Some(&findings[0].expected));
    }

    /// A directory rule claims no file: `describe <file>` answering with a
    /// requirement about the folder would be an answer the reader cannot act
    /// on from where they are standing.
    #[test]
    fn no_file_is_claimed_by_this_rule() {
        let engine = engine(&["projetos/*"], &["projeto.md"], &[]);

        assert!(!engine.applies_to(&path("projetos/03-semaforo/projeto.md")));
    }

    #[test]
    fn a_rule_of_another_kind_is_declined() {
        let structure = CompiledRule {
            id: RuleId::new("shape").expect("valid"),
            module: None,
            why: None,
            module_why: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid"),
            kind: CompiledRuleKind::Structure {
                allowed_subfolders: None,
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: Vec::new(),
                filename_patterns: Vec::new(),
            },
        };

        assert!(PresenceEngine::from_rule(&structure, SkipDirs::default()).is_none());
    }

    #[test]
    fn the_engine_reports_its_identity() {
        let engine = engine(&["projetos/*"], &["projeto.md"], &[]);
        assert_eq!(engine.id().as_str(), "licao-completa");
        assert_eq!(engine.module(), None);
        assert_eq!(engine.level(), Level::Error);
    }
    /// A `presence` rule answers for the directory, and `config doctor` needs
    /// to know it. While this was a match on rule names in `doctor`, every
    /// `presence` rule was reported as evaluating nothing -- with a suggested
    /// fix that would have turned a working rule into a wall of false errors.
    #[test]
    fn it_answers_for_a_directory_rather_than_for_the_files_in_it() {
        let engine = engine(&["projetos/*"], &["projeto.md"], &[]);

        assert!(engine.answers_for_directories());
    }
}
