//! The `structure` rule: which folders may exist, and which filenames.
//!
//! Two sub-modes that share a scope. `allowed_subfolders` constrains the
//! directories immediately inside; `filename_patterns` constrains the files.
//! A rule may use either or both. See `docs/RULES.md`.

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

/// How deep `recurse_into` will follow before giving up.
///
/// A guard, not a feature. Repository trees are shallow, and a bound means a
/// pathological path can never turn the check into a long walk.
const MAX_RECURSION_DEPTH: usize = 64;

/// A compiled `structure` rule.
#[derive(Debug, Clone)]
pub struct StructureEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    allowed_subfolders: Vec<String>,
    warn_subfolders: Vec<String>,
    recurse_into: Vec<String>,
    filename_patterns: Vec<Pattern>,
    skip_dirs: SkipDirs,
}

impl StructureEngine {
    /// Builds an engine from a compiled rule.
    ///
    /// Returns `None` for a rule of any other kind, which is what lets the
    /// runner offer every rule to every engine constructor and keep the ones
    /// that stick.
    ///
    /// `skip_dirs` comes from the configuration rather than the rule because
    /// the escape hatch is repository-wide, and it is passed only to this
    /// engine because decision 5 scopes it to `structure` rules alone.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule, skip_dirs: SkipDirs) -> Option<Self> {
        let CompiledRuleKind::Structure {
            allowed_subfolders,
            warn_subfolders,
            recurse_into,
            filename_patterns,
        } = &rule.kind
        else {
            return None;
        };

        Some(Self::build(
            rule,
            allowed_subfolders,
            warn_subfolders,
            recurse_into,
            filename_patterns,
            skip_dirs,
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
        allowed_subfolders: &[String],
        warn_subfolders: &[String],
        recurse_into: &[String],
        filename_patterns: &[Pattern],
        skip_dirs: SkipDirs,
    ) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            allowed_subfolders: allowed_subfolders.to_vec(),
            warn_subfolders: warn_subfolders.to_vec(),
            recurse_into: recurse_into.to_vec(),
            filename_patterns: filename_patterns.to_vec(),
            skip_dirs,
        }
    }

    /// Whether this rule governs `directory`.
    ///
    /// Either the scope selects it outright, or it was reached through a
    /// `recurse_into` container. The recursive case is what lets one rule
    /// describe an entity *and* the variants of that entity, which have the
    /// same shape one level further down.
    ///
    /// Note which directory is governed: given `recurse_into: ["variants"]`,
    /// it is `user/variants/nfe` and **not** `user/variants` itself. The
    /// container holds entities; it is not one.
    #[must_use]
    pub fn governs(&self, directory: &RepoRelPath) -> bool {
        if self.scope.matches_dir(directory.as_path()) {
            return true;
        }
        if self.recurse_into.is_empty() {
            return false;
        }

        let mut current = directory.clone();
        for _ in 0..MAX_RECURSION_DEPTH {
            let Some(container) = current.parent() else {
                return false;
            };
            let Some(container_name) = container.file_name() else {
                return false;
            };
            if !self.recurse_into.iter().any(|name| name == container_name) {
                return false;
            }

            let Some(grandparent) = container.parent() else {
                return false;
            };
            if self.scope.matches_dir(grandparent.as_path()) {
                return true;
            }
            current = grandparent;
        }

        false
    }

    /// What a subdirectory's name earns: a level, and what to say about it.
    ///
    /// `None` means allowed. Naming a folder in `warn_subfolders` is a more
    /// specific declaration than the rule's blanket `level`, and the more
    /// specific one wins -- so a warn-listed folder reports as a warning even
    /// under an `error` rule. See decision 6.
    ///
    /// The two cases also carry different observations, because "not allowed
    /// here" printed beside `warning` reads as a contradiction.
    fn verdict_for(&self, name: &str) -> Option<(Level, Observed)> {
        if self.allowed_subfolders.iter().any(|a| a == name) {
            return None;
        }
        if self.warn_subfolders.iter().any(|w| w == name) {
            return Some((
                Level::Warning,
                Observed::DiscouragedSubfolder {
                    name: name.to_owned(),
                },
            ));
        }
        Some((
            self.level,
            Observed::UnexpectedSubfolder {
                name: name.to_owned(),
            },
        ))
    }

    fn finding(&self, path: RepoRelPath, level: Level, observed: Observed) -> Finding {
        Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level,
            path,
            span: None,
            observed,
            expected: self.subfolder_expectation(),
        }
    }

    fn subfolder_expectation(&self) -> Expectation {
        Expectation::AllowedSubfolders {
            allowed: self.allowed_subfolders.clone(),
            warn: self.warn_subfolders.clone(),
        }
    }

    fn filename_expectation(&self) -> Expectation {
        Expectation::FilenamePattern {
            patterns: self
                .filename_patterns
                .iter()
                .map(|p| p.as_str().to_owned())
                .collect(),
        }
    }
}

impl RuleEngine for StructureEngine {
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
        // A file is in scope when the directory holding it is governed, which
        // is what `filename_patterns` constrains.
        path.parent().is_some_and(|parent| self.governs(&parent))
    }

    fn check_directory(&self, ctx: DirectoryContext<'_>) -> Vec<Finding> {
        if !self.governs(ctx.path) {
            return Vec::new();
        }

        let mut findings = Vec::new();

        if !self.allowed_subfolders.is_empty() || !self.warn_subfolders.is_empty() {
            for name in ctx.subdirectories {
                let Ok(subdirectory) = ctx.path.join(name) else {
                    continue;
                };

                // The escape hatch: a `_`-prefixed directory is invisible to
                // structure rules and to nothing else, so its files stay in
                // the import graph. See decision 5.
                if self.skip_dirs.exempts(&subdirectory) {
                    continue;
                }

                if let Some((level, observed)) = self.verdict_for(name) {
                    findings.push(self.finding(subdirectory, level, observed));
                }
            }
        }

        if !self.filename_patterns.is_empty() {
            for name in ctx.files {
                if self.filename_patterns.iter().any(|p| p.is_match(name)) {
                    continue;
                }
                let Ok(file) = ctx.path.join(name) else {
                    continue;
                };
                findings.push(Finding {
                    rule_id: self.id.clone(),
                    module_id: self.module.clone(),
                    level: self.level,
                    path: file,
                    span: None,
                    observed: Observed::UnexpectedFilename { name: name.clone() },
                    expected: self.filename_expectation(),
                });
            }
        }

        findings
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        let mut expectations = Vec::new();

        // Asked about a directory: what may live inside it.
        if self.governs(path)
            && (!self.allowed_subfolders.is_empty() || !self.warn_subfolders.is_empty())
        {
            expectations.push(self.subfolder_expectation());
        }

        // Asked about a file: what its name must look like.
        if !self.filename_patterns.is_empty() && self.applies_to(path) {
            expectations.push(self.filename_expectation());
        }

        expectations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn patterns(sources: &[&str]) -> Vec<Pattern> {
        sources
            .iter()
            .map(|s| Pattern::compile(s).expect("valid pattern"))
            .collect()
    }

    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }

    fn rule(scope: &[&str], kind: CompiledRuleKind) -> CompiledRule {
        CompiledRule {
            id: RuleId::new("shape").expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(scope).expect("valid scope"),
            kind,
        }
    }

    fn engine(
        scope: &[&str],
        allowed: &[&str],
        warn: &[&str],
        recurse: &[&str],
        filenames: &[&str],
    ) -> StructureEngine {
        let rule = rule(
            scope,
            CompiledRuleKind::Structure {
                allowed_subfolders: owned(allowed),
                warn_subfolders: owned(warn),
                recurse_into: owned(recurse),
                filename_patterns: patterns(filenames),
            },
        );

        StructureEngine::from_rule(
            &rule,
            SkipDirs {
                prefixes: vec!["_".to_owned()],
                globs: archwarden_core::glob::PathSet::default(),
                scope: archwarden_core::compiled::SkipScope::Structure,
            },
        )
        .expect("is a structure rule")
    }

    /// The runner offers every rule to every engine constructor and keeps what
    /// sticks, so a constructor has to decline what is not its kind.
    #[test]
    fn a_rule_of_another_kind_is_declined() {
        let naming = rule(
            &["src/*"],
            CompiledRuleKind::CallObligation {
                file_pattern: Pattern::compile("^x$").expect("valid"),
                symbol: "Event.save".to_owned(),
                imported_from: "@org/domain".to_owned(),
            },
        );

        assert!(StructureEngine::from_rule(&naming, SkipDirs::default()).is_none());
    }

    fn check(
        engine: &StructureEngine,
        directory: &str,
        subdirectories: &[&str],
        files: &[&str],
    ) -> Vec<Finding> {
        let path = path(directory);
        let subdirectories: Vec<String> = subdirectories.iter().map(|s| (*s).to_owned()).collect();
        let files: Vec<String> = files.iter().map(|s| (*s).to_owned()).collect();

        engine.check_directory(DirectoryContext {
            path: &path,
            subdirectories: &subdirectories,
            files: &files,
        })
    }

    /// The Flowmaatik rule this milestone exists to replace: a fixed set of
    /// folders under each domain entity.
    #[test]
    fn a_subfolder_outside_the_allowed_list_is_reported() {
        let engine = engine(
            &["packages/domain/src/*"],
            &["types", "calcs", "services"],
            &[],
            &[],
            &[],
        );

        let findings = check(
            &engine,
            "packages/domain/src/user",
            &["types", "calcs", "wrong-folder"],
            &[],
        );

        assert_eq!(findings.len(), 1);
        let finding = findings.first().expect("one finding");
        assert_eq!(
            finding.path.as_str(),
            "packages/domain/src/user/wrong-folder"
        );
        assert_eq!(finding.level, Level::Error);
        assert_eq!(
            finding.observed,
            Observed::UnexpectedSubfolder {
                name: "wrong-folder".to_owned()
            }
        );
    }

    /// Decision 6 as code: naming a folder explicitly is more specific than the
    /// rule's blanket level, so a warn-listed folder reports as a warning even
    /// under an `error` rule.
    #[test]
    fn a_warn_listed_subfolder_reports_as_a_warning_under_an_error_rule() {
        let engine = engine(
            &["packages/domain/src/*"],
            &["types"],
            &["shared", "adapters"],
            &[],
            &[],
        );

        let findings = check(
            &engine,
            "packages/domain/src/user",
            &["types", "shared", "nope"],
            &[],
        );

        let by_name: Vec<_> = findings
            .iter()
            .map(|f| (f.path.file_name().unwrap_or_default().to_owned(), f.level))
            .collect();

        assert_eq!(
            by_name,
            [
                ("shared".to_owned(), Level::Warning),
                ("nope".to_owned(), Level::Error),
            ]
        );
        assert_eq!(engine.level(), Level::Error, "the rule itself is an error");

        // The two cases say different things. "Not allowed here" printed
        // beside `warning` would read as a contradiction.
        let observations: Vec<_> = findings.iter().map(|f| &f.observed).collect();
        assert!(matches!(
            observations.first(),
            Some(Observed::DiscouragedSubfolder { .. })
        ));
        assert!(matches!(
            observations.get(1),
            Some(Observed::UnexpectedSubfolder { .. })
        ));
    }

    /// The escape hatch is structural only, so a `_`-prefixed folder is
    /// invisible here while its files stay in the tree for every other rule.
    #[test]
    fn an_underscore_prefixed_subfolder_is_exempt() {
        let engine = engine(&["src/*"], &["types"], &[], &[], &[]);

        let findings = check(&engine, "src/user", &["types", "_internal", "nope"], &[]);

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings.first().expect("one").path.as_str(),
            "src/user/nope"
        );
    }

    /// `recurse_into` governs what is *inside* the container, not the
    /// container. `variants/` holds entities; it is not one, so its own
    /// children are free-form names while *their* children are constrained.
    #[test]
    fn recursion_governs_the_contents_of_a_container_not_the_container() {
        let engine = engine(
            &["packages/domain/src/*"],
            &["types", "calcs", "variants"],
            &[],
            &["variants"],
            &[],
        );

        assert!(engine.governs(&path("packages/domain/src/user")));
        assert!(
            engine.governs(&path("packages/domain/src/user/variants/nfe")),
            "an entity inside the container is governed"
        );
        assert!(
            !engine.governs(&path("packages/domain/src/user/variants")),
            "the container itself is not"
        );
        assert!(!engine.governs(&path("packages/domain/src/user/types")));
    }

    /// The consequence of the rule above, stated on its own because it is what
    /// a reader gets wrong: naming a container in `recurse_into` *stops* its
    /// children's names being checked. They were subfolders of a governed
    /// directory and are now entities, and an entity's name is no more this
    /// rule's business than one selected by `roots`.
    ///
    /// It therefore removes findings. One repository added a namespace holding
    /// nineteen entities and cleared nineteen of them in a single run, which
    /// read as modelling the namespace and was in fact a decision about what
    /// those nineteen directories *are*. Issue #29. `config explain` lists
    /// every directory a rule governs, which is where that decision is
    /// visible.
    #[test]
    fn a_container_stops_its_own_children_being_name_checked() {
        let contract = |recurse: &[&str]| engine(&["src/*"], &["types", "Sub"], &[], recurse, &[]);

        // Without it, a directory inside `Sub` is a subfolder like any other,
        // and `Sub` is governed only if the scope selects it -- which here it
        // does not, so nothing is said either way.
        let promoted = contract(&["Sub"]);
        assert!(
            !promoted.governs(&path("src/Entity/Sub")),
            "the container is not governed, so the names of its children are \
             nobody's to check"
        );
        assert!(
            promoted.governs(&path("src/Entity/Sub/anything-at-all")),
            "and each of those children is an entity in its own right"
        );

        // Which is where the enforcement went: one level further down.
        let findings = check(
            &promoted,
            "src/Entity/Sub/anything-at-all",
            &["types", "wrong"],
            &[],
        );
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(
            findings.first().expect("one").path.as_str(),
            "src/Entity/Sub/anything-at-all/wrong"
        );
    }

    #[test]
    fn a_recursed_entity_is_held_to_the_same_folder_list() {
        let engine = engine(
            &["packages/domain/src/*"],
            &["types", "calcs", "variants"],
            &[],
            &["variants"],
            &[],
        );

        let findings = check(
            &engine,
            "packages/domain/src/user/variants/nfe",
            &["types", "wrong"],
            &[],
        );

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings.first().expect("one").path.as_str(),
            "packages/domain/src/user/variants/nfe/wrong"
        );
    }

    /// Without `recurse_into`, depth is not inherited: only what the scope
    /// glob selects is governed.
    #[test]
    fn without_recursion_only_the_scope_governs() {
        let engine = engine(&["packages/domain/src/*"], &["types"], &[], &[], &[]);

        assert!(engine.governs(&path("packages/domain/src/user")));
        assert!(!engine.governs(&path("packages/domain/src/user/variants/nfe")));
        assert!(
            check(
                &engine,
                "packages/domain/src/user/variants/nfe",
                &["wrong"],
                &[]
            )
            .is_empty()
        );
    }

    /// The filename sub-mode, from docs/CONFIG.md's API-route example.
    #[test]
    fn a_filename_matching_no_pattern_is_reported() {
        let engine = engine(
            &["apps/app/src/app/api/**"],
            &[],
            &[],
            &[],
            &[r"^route\.ts$", r"^route\.(get|post)\.ts$", r"^DOC\.md$"],
        );

        let findings = check(
            &engine,
            "apps/app/src/app/api/users",
            &[],
            &["route.ts", "route.post.ts", "DOC.md", "helpers.ts"],
        );

        assert_eq!(findings.len(), 1);
        let finding = findings.first().expect("one finding");
        assert_eq!(
            finding.observed,
            Observed::UnexpectedFilename {
                name: "helpers.ts".to_owned()
            }
        );
        assert_eq!(
            finding.path.as_str(),
            "apps/app/src/app/api/users/helpers.ts"
        );
    }

    /// One rule may use both sub-modes, and both report.
    #[test]
    fn both_sub_modes_can_fire_from_one_rule() {
        let engine = engine(&["src/*"], &["types"], &[], &[], &[r"^index\.ts$"]);

        let findings = check(&engine, "src/user", &["nope"], &["index.ts", "other.ts"]);

        assert_eq!(findings.len(), 2);
        assert!(
            findings
                .iter()
                .any(|f| matches!(f.observed, Observed::UnexpectedSubfolder { .. }))
        );
        assert!(
            findings
                .iter()
                .any(|f| matches!(f.observed, Observed::UnexpectedFilename { .. }))
        );
    }

    /// A rule with neither list reports nothing rather than rejecting
    /// everything. An empty `allowed_subfolders` means "not checking folders",
    /// not "no folders allowed".
    #[test]
    fn an_empty_rule_reports_nothing() {
        let engine = engine(&["src/*"], &[], &[], &[], &[]);
        assert!(check(&engine, "src/user", &["anything"], &["anything.ts"]).is_empty());
    }

    #[test]
    fn a_directory_outside_the_scope_is_left_alone() {
        let engine = engine(&["packages/domain/src/*"], &["types"], &[], &[], &[]);
        assert!(check(&engine, "packages/application/src/foo", &["nope"], &[]).is_empty());
    }

    /// Decision 9 as an assertion: what the checker demands is what the
    /// informant advertises.
    #[test]
    fn what_check_demands_is_what_describe_expectation_advertises() {
        let engine = engine(&["src/*"], &["types"], &["shared"], &[], &[]);

        let findings = check(&engine, "src/user", &["nope"], &[]);
        let demanded = &findings.first().expect("one finding").expected;

        let advertised = engine.describe_expectation(&path("src/user"));
        assert_eq!(advertised.len(), 1);
        assert_eq!(advertised.first(), Some(demanded));
    }

    /// `describe` is asked about paths that do not exist. A rule with filename
    /// patterns has something to say about a file that has never been written.
    #[test]
    fn a_filename_expectation_is_describable_for_a_file_that_does_not_exist() {
        let engine = engine(&["src/*"], &[], &[], &[], &[r"^route\.ts$"]);

        let expectations = engine.describe_expectation(&path("src/user/not-written-yet.ts"));
        assert_eq!(
            expectations,
            [Expectation::FilenamePattern {
                patterns: vec![r"^route\.ts$".to_owned()]
            }]
        );

        assert!(
            engine
                .describe_expectation(&path("elsewhere/file.ts"))
                .is_empty()
        );
    }

    /// A rule that constrains folders has nothing to say about a file's name,
    /// and vice versa. `scaffold` should not invent an expectation.
    #[test]
    fn each_sub_mode_describes_only_what_it_constrains() {
        let folders = engine(&["src/*"], &["types"], &[], &[], &[]);
        assert!(
            folders
                .describe_expectation(&path("src/user/file.ts"))
                .is_empty(),
            "a folder rule says nothing about a filename"
        );
        assert_eq!(folders.describe_expectation(&path("src/user")).len(), 1);
    }

    #[test]
    fn the_engine_reports_its_identity() {
        let engine = engine(&["src/*"], &["types"], &[], &[], &[]);
        assert_eq!(engine.id().as_str(), "shape");
        assert_eq!(engine.module(), None);
        assert_eq!(engine.level(), Level::Error);
        assert!(engine.applies_to(&path("src/user/file.ts")));
        assert!(!engine.applies_to(&path("elsewhere/file.ts")));
    }

    /// A module label reaches the finding, so a report can group by it.
    #[test]
    fn a_findings_module_comes_from_the_rule() {
        let mut engine = engine(&["src/*"], &["types"], &[], &[], &[]);
        engine.module = Some(ModuleId::new("domain").expect("valid"));

        let findings = check(&engine, "src/user", &["nope"], &[]);
        assert_eq!(
            findings
                .first()
                .expect("one")
                .module_id
                .as_ref()
                .map(ModuleId::as_str),
            Some("domain")
        );
    }
}
