//! The `spec-pair` rule: the TDD gate.
//!
//! Every unit file under the configured subfolders must have a spec sibling.
//! Cheap by design -- it reads no file, only the names the walk already
//! collected -- unless `require_non_empty_spec` is set, which is what makes
//! the difference between "a spec file exists" and "a spec was written".
//!
//! See `docs/RULES.md`.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    finding::{Expectation, Finding, Observed},
    glob::PathSet,
    ids::{ModuleId, RuleId},
    level::Level,
    path::{FileClass, RepoRelPath},
    scope::Scope,
    traits::{DirectoryContext, FileContext, RuleEngine},
};

/// Barrel files, which re-export and hold no behaviour of their own.
///
/// Baked in rather than configurable, as `docs/RULES.md` specifies. A rule
/// that made you list these would make every config longer for no choice
/// anyone wants to make differently.
const ALWAYS_EXEMPT: [&str; 4] = ["index.ts", "index.tsx", "index.js", "index.jsx"];

/// A compiled `spec-pair` rule.
#[derive(Debug, Clone)]
pub struct SpecPairEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    subfolders: Vec<String>,
    spec_markers: Vec<String>,
    ignore_files: PathSet,
    require_non_empty_spec: bool,
}

impl SpecPairEngine {
    /// Builds an engine from a compiled rule.
    ///
    /// Returns `None` for a rule of any other kind.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::SpecPair {
            subfolders,
            spec_markers,
            ignore_files,
            require_non_empty_spec,
        } = &rule.kind
        else {
            return None;
        };

        Some(Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            subfolders: subfolders.clone(),
            spec_markers: spec_markers.clone(),
            ignore_files: ignore_files.clone(),
            require_non_empty_spec: *require_non_empty_spec,
        })
    }

    /// Whether this rule governs `directory`.
    ///
    /// `subfolders: ["."]` means the scope-selected directory itself; any
    /// other entry means a directory of that name directly inside one.
    #[must_use]
    pub fn governs(&self, directory: &RepoRelPath) -> bool {
        if self
            .subfolders
            .iter()
            .any(|sub| sub == "." || sub.is_empty())
            && self.scope.matches_dir(directory.as_path())
        {
            return true;
        }

        let Some(name) = directory.file_name() else {
            return false;
        };
        if !self.subfolders.iter().any(|sub| sub == name) {
            return false;
        }

        directory
            .parent()
            .is_some_and(|parent| self.scope.matches_dir(parent.as_path()))
    }

    /// Splits a filename into its stem and extension.
    ///
    /// The split is at the *last* dot, which is what makes a compound name
    /// like `user.db.repository.ts` work: the stem keeps every component but
    /// the extension, so the spec beside it is
    /// `user.db.repository.spec.ts`.
    fn split(name: &str) -> Option<(&str, &str)> {
        name.rsplit_once('.')
            .filter(|(stem, extension)| !stem.is_empty() && !extension.is_empty())
    }

    /// The spec file `name` would need, using the first marker.
    ///
    /// The extension is the source file's own, so a `.tsx` component asks for
    /// a `.tsx` spec. Which marker is a project's preference; which extension
    /// is not.
    fn spec_name_for(&self, name: &str) -> Option<String> {
        let (stem, extension) = Self::split(name)?;
        let marker = self.spec_markers.first()?;
        Some(format!("{stem}.{marker}.{extension}"))
    }

    /// Whether `name` is itself a spec, for any marker.
    ///
    /// The marker has to be the last stem component: `user.spec.ts` is a spec,
    /// and `user.spec.helper.ts` is a helper that happens to mention one.
    fn is_spec(&self, name: &str) -> bool {
        let Some((stem, _extension)) = Self::split(name) else {
            return false;
        };

        self.spec_markers.iter().any(|marker| {
            // `spec.ts` with no stem before it is a spec too -- both vitest and
            // jest treat the leading `<name>.` as optional.
            stem == marker || stem.ends_with(&format!(".{marker}"))
        })
    }

    /// Whether `candidate` is a spec for the unit file with stem `stem`.
    ///
    /// Any source extension is accepted, not just the unit file's own: some
    /// projects test a `.tsx` component with a `.ts` spec, and refusing that
    /// would be a false positive on a file that plainly exists.
    fn is_spec_for(&self, candidate: &str, stem: &str) -> bool {
        let Some((candidate_stem, _extension)) = Self::split(candidate) else {
            return false;
        };
        if FileClass::of(candidate) != FileClass::Source {
            return false;
        }

        self.spec_markers
            .iter()
            .any(|marker| candidate_stem == format!("{stem}.{marker}"))
    }

    /// Whether a file is exempt before any sibling is looked for.
    fn is_exempt(&self, path: &RepoRelPath, name: &str) -> bool {
        // A spec is not its own unit file.
        if self.is_spec(name) {
            return true;
        }
        // Only source files carry behaviour worth testing. This is what keeps
        // the rule off `DOC.md`, `package.json` and images without anyone
        // listing them.
        if FileClass::of(name) != FileClass::Source {
            return true;
        }
        if ALWAYS_EXEMPT.contains(&name) {
            return true;
        }
        self.ignore_files.is_match(path.as_path())
    }

    fn expectation(&self, spec: RepoRelPath) -> Expectation {
        Expectation::RequiredSibling {
            path: spec,
            non_empty_spec: self.require_non_empty_spec,
        }
    }
}

impl RuleEngine for SpecPairEngine {
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
        let Some(parent) = path.parent() else {
            return false;
        };
        let Some(name) = path.file_name() else {
            return false;
        };
        if !self.governs(&parent) {
            return false;
        }

        // A spec is exempt from needing a spec of its own, but when
        // `require_non_empty_spec` is set the rule has something to say about
        // the spec itself -- and a rule that claimed not to apply would never
        // be offered the file.
        if self.require_non_empty_spec && self.is_spec(name) {
            return true;
        }

        !self.is_exempt(path, name)
    }

    fn needs_facts(&self) -> bool {
        self.require_non_empty_spec
    }

    /// Reports a spec that exists but contains no test cases.
    ///
    /// The finding is about the spec file, not the unit file: the unit file
    /// has its sibling, and what is missing is inside it. This is the flag
    /// that separates "a spec file exists" from "a spec was written".
    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        if !self.require_non_empty_spec {
            return Vec::new();
        }
        let Some(name) = ctx.path.file_name() else {
            return Vec::new();
        };
        if !self.is_spec(name) {
            return Vec::new();
        }
        let Some(parent) = ctx.path.parent() else {
            return Vec::new();
        };
        if !self.governs(&parent) {
            return Vec::new();
        }
        let Some(facts) = ctx.facts else {
            return Vec::new();
        };

        // `describe` deliberately does not count. An empty describe block
        // satisfies the letter of the rule while defeating its entire purpose.
        if facts
            .calls
            .iter()
            .any(|call| matches!(call.callee.as_str(), "it" | "test"))
        {
            return Vec::new();
        }

        vec![Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: ctx.path.clone(),
            span: None,
            observed: Observed::SpecIsEmpty {
                path: ctx.path.clone(),
            },
            expected: self.expectation(ctx.path.clone()),
        }]
    }

    fn check_directory(&self, ctx: DirectoryContext<'_>) -> Vec<Finding> {
        if !self.governs(ctx.path) {
            return Vec::new();
        }

        ctx.files
            .iter()
            .filter_map(|name| {
                let file = ctx.path.join(name).ok()?;
                if self.is_exempt(&file, name) {
                    return None;
                }

                let (stem, _extension) = Self::split(name)?;
                if ctx
                    .files
                    .iter()
                    .any(|sibling| self.is_spec_for(sibling, stem))
                {
                    return None;
                }

                let spec = ctx.path.join(&self.spec_name_for(name)?).ok()?;
                Some(Finding {
                    rule_id: self.id.clone(),
                    module_id: self.module.clone(),
                    level: self.level,
                    path: file,
                    span: None,
                    observed: Observed::SiblingMissing { path: spec.clone() },
                    expected: self.expectation(spec),
                })
            })
            .collect()
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        if !self.applies_to(path) {
            return Vec::new();
        }

        let Some(name) = path.file_name() else {
            return Vec::new();
        };
        let Some(parent) = path.parent() else {
            return Vec::new();
        };
        let Some(spec_name) = self.spec_name_for(name) else {
            return Vec::new();
        };
        let Ok(spec) = parent.join(&spec_name) else {
            return Vec::new();
        };

        vec![self.expectation(spec)]
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

    fn engine(scope: &[&str], subfolders: &[&str], ignore: &[&str]) -> SpecPairEngine {
        let rule = CompiledRule {
            id: RuleId::new("needs-spec").expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(scope).expect("valid scope"),
            kind: CompiledRuleKind::SpecPair {
                subfolders: owned(subfolders),
                spec_markers: owned(&["spec", "test"]),
                ignore_files: PathSet::compile(ignore).expect("valid globs"),
                require_non_empty_spec: false,
            },
        };

        SpecPairEngine::from_rule(&rule).expect("is a spec-pair rule")
    }

    fn check(engine: &SpecPairEngine, directory: &str, files: &[&str]) -> Vec<Finding> {
        let path = path(directory);
        let files = owned(files);
        engine.check_directory(DirectoryContext {
            path: &path,
            subdirectories: &[],
            files: &files,
        })
    }

    fn offenders(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.path.as_str()).collect()
    }

    /// The rule this milestone exists to replace: a unit file with no spec
    /// beside it.
    #[test]
    fn a_file_without_its_spec_is_reported() {
        let engine = engine(&["packages/domain/src/*"], &["calcs"], &[]);

        let findings = check(
            &engine,
            "packages/domain/src/user/calcs",
            &["compute-age.ts", "compute-age.spec.ts", "format-name.ts"],
        );

        assert_eq!(
            offenders(&findings),
            ["packages/domain/src/user/calcs/format-name.ts"]
        );

        let finding = findings.first().expect("one finding");
        assert_eq!(
            finding.observed,
            Observed::SiblingMissing {
                path: path("packages/domain/src/user/calcs/format-name.spec.ts"),
            }
        );
        assert_eq!(finding.level, Level::Error);
    }

    /// `subfolders: ["."]` puts the scope-selected directory itself under the
    /// rule, which is what the minimal config in docs/CONFIG.md does.
    #[test]
    fn a_dot_subfolder_means_the_scope_directory_itself() {
        let engine = engine(&["src/**"], &["."], &[]);

        assert!(engine.governs(&path("src")));
        assert!(engine.governs(&path("src/deep/nested")));
        assert_eq!(
            offenders(&check(&engine, "src/user", &["user.ts"])),
            ["src/user/user.ts"]
        );
    }

    /// A named subfolder is a directory of that name directly inside a
    /// scope-selected one -- not the scope directory, and not deeper.
    #[test]
    fn a_named_subfolder_is_one_level_inside_the_scope() {
        let engine = engine(&["packages/domain/src/*"], &["calcs", "services"], &[]);

        assert!(engine.governs(&path("packages/domain/src/user/calcs")));
        assert!(engine.governs(&path("packages/domain/src/user/services")));
        assert!(!engine.governs(&path("packages/domain/src/user")));
        assert!(!engine.governs(&path("packages/domain/src/user/types")));
        assert!(!engine.governs(&path("packages/domain/src/user/calcs/deep")));
    }

    /// A spec is not its own unit file, or every spec would demand a spec.
    #[test]
    fn a_spec_file_does_not_need_a_spec_of_its_own() {
        let engine = engine(&["src/*"], &["."], &[]);
        assert!(check(&engine, "src/user", &["user.spec.ts"]).is_empty());
    }

    /// Barrel files re-export and hold no behaviour. Baked in, as
    /// docs/RULES.md specifies.
    #[test]
    fn index_files_are_exempt_without_being_configured() {
        let engine = engine(&["src/*"], &["."], &[]);
        assert!(
            check(
                &engine,
                "src/user",
                &["index.ts", "index.tsx", "index.js", "index.jsx"]
            )
            .is_empty()
        );
    }

    /// Only source files carry behaviour worth testing. This is what keeps the
    /// rule off documentation, data and assets without anyone listing them --
    /// and it makes the `DOC.md` and `README.md` entries docs/RULES.md names
    /// as baked-in exemptions redundant.
    #[test]
    fn files_that_are_not_source_are_exempt() {
        let engine = engine(&["src/*"], &["."], &[]);
        assert!(
            check(
                &engine,
                "src/user",
                &[
                    "DOC.md",
                    "README.md",
                    "fixtures.json",
                    "logo.png",
                    "Makefile"
                ]
            )
            .is_empty()
        );
    }

    /// `ignore_files` takes globs rather than exact paths, so a whole shape of
    /// file can be exempted at once.
    #[test]
    fn ignore_globs_exempt_by_pattern() {
        let engine = engine(
            &["src/*"],
            &["."],
            &["src/**/*.types.ts", "src/user/legacy.ts"],
        );

        let findings = check(
            &engine,
            "src/user",
            &["user.types.ts", "legacy.ts", "real.ts"],
        );

        assert_eq!(offenders(&findings), ["src/user/real.ts"]);
    }

    /// The extension follows the source file. A `.tsx` component asking for a
    /// `.ts` spec was the bug this rule shipped with, enshrined in a test that
    /// asserted the wrong answer.
    #[test]
    fn the_suggested_spec_keeps_the_source_extension() {
        let engine = engine(&["src/*"], &["."], &[]);

        assert_eq!(
            engine.spec_name_for("user.ts").as_deref(),
            Some("user.spec.ts")
        );
        assert_eq!(
            engine.spec_name_for("Component.tsx").as_deref(),
            Some("Component.spec.tsx")
        );
        assert_eq!(
            engine.spec_name_for("legacy.mjs").as_deref(),
            Some("legacy.spec.mjs")
        );
        assert_eq!(engine.spec_name_for("no-extension"), None);
    }

    /// Compound names are the real shape in the target repository:
    /// `*.db.repository.ts`, `*.use-case.ts`. The split is at the last dot, so
    /// every component but the extension stays in the stem.
    #[test]
    fn a_compound_name_keeps_every_component_but_the_extension() {
        let engine = engine(&["src/*"], &["."], &[]);

        assert_eq!(
            engine.spec_name_for("user.db.repository.ts").as_deref(),
            Some("user.db.repository.spec.ts")
        );
        assert_eq!(
            engine.spec_name_for("create-client.use-case.ts").as_deref(),
            Some("create-client.use-case.spec.ts")
        );

        // And it is satisfied by the file that name denotes.
        assert!(
            check(
                &engine,
                "src/user",
                &["user.db.repository.ts", "user.db.repository.spec.ts"]
            )
            .is_empty()
        );
        assert_eq!(
            offenders(&check(&engine, "src/user", &["user.db.repository.ts"])),
            ["src/user/user.db.repository.ts"]
        );
    }

    /// Both markers by default, because vitest and jest both accept either and
    /// a repository mid-migration has both on disk.
    #[test]
    fn either_marker_satisfies_the_rule() {
        let engine = engine(&["src/*"], &["."], &[]);

        assert!(check(&engine, "src/user", &["a.ts", "a.spec.ts"]).is_empty());
        assert!(check(&engine, "src/user", &["b.ts", "b.test.ts"]).is_empty());
        assert_eq!(
            offenders(&check(&engine, "src/user", &["c.ts", "c.helper.ts"])),
            ["src/user/c.ts", "src/user/c.helper.ts"]
        );
    }

    /// Sub-decision from the review: a `.tsx` component may be tested by a
    /// `.ts` spec. Refusing that would be a false positive on a file that
    /// plainly exists.
    #[test]
    fn any_source_extension_satisfies_an_existing_spec() {
        let engine = engine(&["src/*"], &["."], &[]);

        assert!(check(&engine, "src/user", &["Component.tsx", "Component.spec.ts"]).is_empty());
        assert!(
            check(
                &engine,
                "src/user",
                &["Component.tsx", "Component.test.tsx"]
            )
            .is_empty()
        );

        // But a non-source file with the right name is not a spec.
        assert_eq!(
            offenders(&check(&engine, "src/user", &["a.ts", "a.spec.md"])),
            ["src/user/a.ts"]
        );
    }

    /// The marker has to be the last stem component. A helper that merely
    /// mentions one is a unit file like any other.
    #[test]
    fn a_marker_elsewhere_in_the_name_does_not_make_a_spec() {
        let engine = engine(&["src/*"], &["."], &[]);

        assert!(engine.is_spec("user.spec.ts"));
        assert!(engine.is_spec("user.db.repository.test.ts"));
        assert!(
            engine.is_spec("spec.ts"),
            "vitest and jest accept a bare marker"
        );
        assert!(!engine.is_spec("user.spec.helper.ts"));
        assert!(!engine.is_spec("specification.ts"));
        assert!(!engine.is_spec("user.ts"));
    }

    /// Restricting the markers is how a project says "we use `.test`, and a
    /// stray `.spec.ts` is not a test". Both directions have to follow, or
    /// every one of its tests would demand a test.
    #[test]
    fn a_restricted_marker_list_is_used_in_both_directions() {
        let rule = CompiledRule {
            id: RuleId::new("needs-test").expect("valid"),
            module: None,
            level: Level::Warning,
            scope: Scope::compile(["src/*"]).expect("valid"),
            kind: CompiledRuleKind::SpecPair {
                subfolders: owned(["."].as_slice()),
                spec_markers: owned(&["test"]),
                ignore_files: PathSet::default(),
                require_non_empty_spec: false,
            },
        };
        let engine = SpecPairEngine::from_rule(&rule).expect("is a spec-pair rule");

        assert!(check(&engine, "src/user", &["a.ts", "a.test.ts"]).is_empty());

        // `.spec.ts` is no longer a spec here, so it is a unit file that needs
        // its own test -- and it does not satisfy `b.ts`.
        let findings = check(&engine, "src/user", &["b.ts", "b.spec.ts"]);
        assert_eq!(
            offenders(&findings),
            ["src/user/b.ts", "src/user/b.spec.ts"]
        );
        assert_eq!(findings.first().expect("one").level, Level::Warning);
        assert_eq!(engine.spec_name_for("a.ts").as_deref(), Some("a.test.ts"));
    }

    #[test]
    fn a_directory_outside_the_scope_is_left_alone() {
        let engine = engine(&["packages/domain/src/*"], &["calcs"], &[]);
        assert!(check(&engine, "packages/application/src/foo/calcs", &["x.ts"]).is_empty());
    }

    /// Decision 9 as an assertion: what the checker demands is what the
    /// informant advertises.
    #[test]
    fn what_check_demands_is_what_describe_expectation_advertises() {
        let engine = engine(&["src/*"], &["."], &[]);

        let findings = check(&engine, "src/user", &["thing.ts"]);
        let demanded = &findings.first().expect("one finding").expected;

        let advertised = engine.describe_expectation(&path("src/user/thing.ts"));
        assert_eq!(advertised.len(), 1);
        assert_eq!(advertised.first(), Some(demanded));
    }

    /// The informant answers for a file nobody has written yet, which is what
    /// `scaffold` is for: it tells an agent the spec it will also need.
    #[test]
    fn the_required_spec_is_describable_before_the_file_exists() {
        let engine = engine(&["packages/application/src/use-cases/*"], &["."], &[]);

        let expectations = engine.describe_expectation(&path(
            "packages/application/src/use-cases/foo/foo.use-case.ts",
        ));

        assert_eq!(
            expectations,
            [Expectation::RequiredSibling {
                path: path("packages/application/src/use-cases/foo/foo.use-case.spec.ts"),
                non_empty_spec: false,
            }]
        );
    }

    /// An exempt file has no expectation to advertise, so `scaffold` does not
    /// tell an agent to write a spec for a barrel file.
    #[test]
    fn an_exempt_file_advertises_nothing() {
        let engine = engine(&["src/*"], &["."], &["src/**/*.types.ts"]);

        for name in ["index.ts", "user.spec.ts", "user.types.ts", "DOC.md"] {
            assert!(
                engine
                    .describe_expectation(&path(&format!("src/user/{name}")))
                    .is_empty(),
                "{name} should advertise nothing"
            );
        }
        assert!(
            engine
                .describe_expectation(&path("elsewhere/thing.ts"))
                .is_empty()
        );
    }

    /// `require_non_empty_spec` reaches the expectation even before the check
    /// that reads the spec exists, so `scaffold` already tells an agent the
    /// spec must contain a test.
    #[test]
    fn the_non_empty_requirement_is_carried_into_the_expectation() {
        let rule = CompiledRule {
            id: RuleId::new("tdd-gate").expect("valid"),
            module: Some(ModuleId::new("domain").expect("valid")),
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid"),
            kind: CompiledRuleKind::SpecPair {
                subfolders: owned(["."].as_slice()),
                spec_markers: owned(&["spec"]),
                ignore_files: PathSet::default(),
                require_non_empty_spec: true,
            },
        };
        let engine = SpecPairEngine::from_rule(&rule).expect("is a spec-pair rule");

        assert_eq!(
            engine.describe_expectation(&path("src/user/thing.ts")),
            [Expectation::RequiredSibling {
                path: path("src/user/thing.spec.ts"),
                non_empty_spec: true,
            }]
        );

        let findings = check(&engine, "src/user", &["thing.ts"]);
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

    #[test]
    fn a_rule_of_another_kind_is_declined() {
        let structure = CompiledRule {
            id: RuleId::new("shape").expect("valid"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid"),
            kind: CompiledRuleKind::Structure {
                allowed_subfolders: Vec::new(),
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                filename_patterns: Vec::new(),
            },
        };

        assert!(SpecPairEngine::from_rule(&structure).is_none());
    }

    /// The runner only offers a file to a rule that says it applies. A spec is
    /// exempt from needing a spec of its own, so a rule that stopped there
    /// would never be handed the spec -- and the non-empty check would never
    /// run. It shipped that way, and only running it against a real repository
    /// showed it.
    #[test]
    fn a_spec_is_in_scope_for_the_non_empty_check_despite_being_exempt() {
        let lenient = engine(&["src/*"], &["."], &[]);
        assert!(
            !lenient.applies_to(&path("src/user/thing.spec.ts")),
            "without the flag, a spec is simply exempt"
        );

        let rule = CompiledRule {
            id: RuleId::new("tdd-gate").expect("valid"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid"),
            kind: CompiledRuleKind::SpecPair {
                subfolders: owned(["."].as_slice()),
                spec_markers: owned(&["spec"]),
                ignore_files: PathSet::default(),
                require_non_empty_spec: true,
            },
        };
        let strict = SpecPairEngine::from_rule(&rule).expect("is a spec-pair rule");

        assert!(strict.applies_to(&path("src/user/thing.spec.ts")));
        assert!(strict.needs_facts());
    }

    /// The flag's whole purpose: a spec with a `describe` and no test cases
    /// satisfies "a file exists" and defeats what the rule is for.
    #[test]
    fn a_spec_with_no_test_cases_is_reported() {
        use archwarden_core::{
            facts::{CallFact, FileFacts, Span},
            hash::ContentHash,
        };

        let rule = CompiledRule {
            id: RuleId::new("tdd-gate").expect("valid"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid"),
            kind: CompiledRuleKind::SpecPair {
                subfolders: owned(["."].as_slice()),
                spec_markers: owned(&["spec"]),
                ignore_files: PathSet::default(),
                require_non_empty_spec: true,
            },
        };
        let engine = SpecPairEngine::from_rule(&rule).expect("is a spec-pair rule");

        let check_spec = |callees: &[&str]| {
            let spec = path("src/user/thing.spec.ts");
            let mut facts = FileFacts::unparsed(spec.clone(), ContentHash::of(b""));
            facts.calls = callees
                .iter()
                .map(|callee| CallFact {
                    callee: (*callee).to_owned(),
                    span: Span::new(0, 1),
                })
                .collect();

            engine.check_file(FileContext {
                path: &spec,
                facts: Some(&facts),
                siblings: &[],
            })
        };

        assert!(check_spec(&["it"]).is_empty());
        assert!(check_spec(&["describe", "test"]).is_empty());

        let findings = check_spec(&["describe"]);
        assert_eq!(
            findings.first().expect("one").observed,
            Observed::SpecIsEmpty {
                path: path("src/user/thing.spec.ts")
            },
            "a describe alone satisfies the letter and defeats the purpose"
        );
        assert_eq!(check_spec(&[]).len(), 1, "no calls at all is empty too");
    }

    #[test]
    fn the_engine_reports_its_identity() {
        let engine = engine(&["src/*"], &["."], &[]);
        assert_eq!(engine.id().as_str(), "needs-spec");
        assert_eq!(engine.module(), None);
        assert_eq!(engine.level(), Level::Error);
        assert!(engine.applies_to(&path("src/user/thing.ts")));
        assert!(!engine.applies_to(&path("src/user/index.ts")));
        assert!(!engine.applies_to(&RepoRelPath::root()));
    }
}
