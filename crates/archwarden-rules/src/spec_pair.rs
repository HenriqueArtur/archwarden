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
    traits::{FactsNeeded, FileContext, RuleEngine},
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
    /// Directory names beside a file where a spec also counts. Empty is
    /// sibling-only, which is every config written before issue #67.
    spec_dirs: Vec<String>,
    require_non_empty_spec: bool,
    skip_type_only: bool,
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
            spec_dirs,
            require_non_empty_spec,
            skip_type_only,
        } = &rule.kind
        else {
            return None;
        };

        Some(Self::build(
            rule,
            subfolders,
            spec_markers,
            ignore_files,
            spec_dirs,
            *require_non_empty_spec,
            *skip_type_only,
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
        subfolders: &[String],
        spec_markers: &[String],
        ignore_files: &PathSet,
        spec_dirs: &[String],
        require_non_empty_spec: bool,
        skip_type_only: bool,
    ) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            subfolders: subfolders.to_vec(),
            spec_markers: spec_markers.to_vec(),
            ignore_files: ignore_files.clone(),
            spec_dirs: spec_dirs.to_vec(),
            require_non_empty_spec,
            skip_type_only,
        }
    }

    /// Whether this rule governs `directory`.
    ///
    /// `subfolders: ["."]` means the scope-selected directory itself, and only
    /// its own files: naming `calcs` is how a project says *this* subtree is
    /// under the gate, and a recursive `.` would swallow `types` and every
    /// other folder it deliberately did not name.
    ///
    /// Any other entry names a directory relative to the scope-selected one,
    /// and covers it **and everything below it**. `calcs` covers
    /// `Entity/calcs/group/nested.ts`; `calcs/group` names the same subtree
    /// one level in.
    ///
    /// Both halves of that used to be false. The entry was compared against a
    /// directory's *name*, so only a direct child matched: a file one level
    /// deeper was silently outside the gate, and a nested entry like
    /// `calcs/group` could never equal a single component, so it was accepted,
    /// reported as valid, and matched nothing.
    ///
    /// The cost was measured. Two entities in one repository grouped their
    /// validation steps into a subfolder; one had thirteen files and thirteen
    /// specs, the other eleven files and no test at all, and neither the
    /// report nor the baseline had ever mentioned it. Same shape, same
    /// convention, one side quietly unguarded — which is what a TDD gate
    /// exists to make impossible. Issue #34.
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

        // Up to the nearest scope-selected ancestor, then ask whether the path
        // from it enters a named subfolder. Walking rather than checking the
        // parent alone is the whole fix: the file may sit any number of levels
        // below the folder that was named.
        let mut ancestor = directory.parent();
        while let Some(root) = ancestor {
            if self.scope.matches_dir(root.as_path())
                && let Ok(tail) = directory.as_path().strip_prefix(root.as_path())
                && self
                    .subfolders
                    .iter()
                    .any(|sub| sub != "." && !sub.is_empty() && tail.starts_with(sub))
            {
                return true;
            }
            ancestor = root.parent();
        }

        false
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

    /// Whether every export in a file is a `type` or an `interface`.
    ///
    /// The question `skip_type_only` asks. `enum` is deliberately not in the
    /// set: an enum exists at runtime and has behaviour a test can call, which
    /// is the whole distinction being drawn. Nor is a file with no exports at
    /// all — that is a file nobody imports rather than a contract, and it is
    /// not what this exemption is for.
    ///
    /// A re-export is not type-only either. Its real kind needs the file on
    /// the other side, which `RULES.md` keeps this rule away from, and
    /// guessing "probably a type" would exempt files on a coin flip.
    fn is_type_only(facts: &archwarden_core::facts::FileFacts) -> bool {
        use archwarden_core::facts::ExportKind;

        !facts.exports.is_empty()
            && facts.exports.iter().all(|export| {
                export.tags.contains(ExportKind::Type)
                    || export.tags.contains(ExportKind::Interface)
            })
    }

    /// The finding for a unit file with no spec beside it.
    ///
    /// Lives here rather than in `check_directory` because deciding it can
    /// need the file's exports, and a directory listing is only names. The
    /// inputs are otherwise the same: this file, and what else is in the
    /// folder.
    fn missing_sibling(
        &self,
        parent: &RepoRelPath,
        name: &str,
        ctx: FileContext<'_>,
    ) -> Vec<Finding> {
        if self.is_exempt(ctx.path, name) {
            return Vec::new();
        }

        // Before the sibling search, because a file with nothing to test needs
        // no sibling to be found. Facts absent means the file could not be
        // parsed, and the run counts that as a skipped check rather than
        // silently exempting it.
        if self.skip_type_only && ctx.facts.is_some_and(Self::is_type_only) {
            return Vec::new();
        }

        let Some((stem, _extension)) = Self::split(name) else {
            return Vec::new();
        };
        if ctx
            .siblings
            .iter()
            .any(|sibling| self.is_spec_for(sibling, stem))
        {
            return Vec::new();
        }

        let Some(spec_name) = self.spec_name_for(name) else {
            return Vec::new();
        };

        // A directory the rule named, one level down and no further. Asked of
        // the existence predicate rather than of `siblings`, which only carries
        // this directory's own listing.
        //
        // One level is the whole design: a reading that accepted a spec
        // anywhere below would let a rule report nothing while looking exactly
        // like a repository that is fully tested. Issue #67.
        if self.spec_dirs.iter().any(|directory| {
            parent
                .join(directory)
                .and_then(|inside| inside.join(&spec_name))
                .is_ok_and(|candidate| ctx.exists.at(&candidate))
        }) {
            return Vec::new();
        }
        let Ok(spec) = parent.join(&spec_name) else {
            return Vec::new();
        };

        vec![Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: ctx.path.clone(),
            span: None,
            observed: Observed::SiblingMissing { path: spec.clone() },
            expected: self.expectation(spec),
        }]
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

    fn needs_facts(&self) -> FactsNeeded {
        if self.require_non_empty_spec || self.skip_type_only {
            FactsNeeded::Code
        } else {
            FactsNeeded::Nothing
        }
    }

    /// Reports a spec that exists but contains no test cases.
    ///
    /// The finding is about the spec file, not the unit file: the unit file
    /// has its sibling, and what is missing is inside it. This is the flag
    /// that separates "a spec file exists" from "a spec was written".
    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        let Some(name) = ctx.path.file_name() else {
            return Vec::new();
        };
        let Some(parent) = ctx.path.parent() else {
            return Vec::new();
        };
        if !self.governs(&parent) {
            return Vec::new();
        }

        if !self.is_spec(name) {
            return self.missing_sibling(&parent, name, ctx);
        }
        if !self.require_non_empty_spec {
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
    use archwarden_core::traits::Exists;

    use super::*;
    use archwarden_core::facts::FileFacts;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }

    fn engine(scope: &[&str], subfolders: &[&str], ignore: &[&str]) -> SpecPairEngine {
        engine_with_spec_dirs_and_ignore(scope, subfolders, ignore, &[])
    }

    /// The same, with directories where a spec also counts. Issue #67.
    fn engine_with_spec_dirs(
        scope: &[&str],
        subfolders: &[&str],
        spec_dirs: &[&str],
    ) -> SpecPairEngine {
        engine_with_spec_dirs_and_ignore(scope, subfolders, &[], spec_dirs)
    }

    fn engine_with_spec_dirs_and_ignore(
        scope: &[&str],
        subfolders: &[&str],
        ignore: &[&str],
        spec_dirs: &[&str],
    ) -> SpecPairEngine {
        let rule = CompiledRule {
            id: RuleId::new("needs-spec").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(scope).expect("valid scope"),
            kind: CompiledRuleKind::SpecPair {
                subfolders: owned(subfolders),
                spec_markers: owned(&["spec", "test"]),
                ignore_files: PathSet::compile(ignore).expect("valid globs"),
                spec_dirs: owned(spec_dirs),
                require_non_empty_spec: false,
                skip_type_only: false,
            },
        };

        SpecPairEngine::from_rule(&rule).expect("is a spec-pair rule")
    }

    /// Every file in a directory, offered to the rule one at a time.
    ///
    /// The missing-spec finding used to come from `check_directory`, which saw
    /// the whole listing at once. It moved to `check_file` because deciding it
    /// can need the file's exports, which a listing of names does not carry.
    /// The inputs are the same either way — this file, and what else is in the
    /// folder — so the tests written against the old shape still hold.
    fn check(engine: &SpecPairEngine, directory: &str, files: &[&str]) -> Vec<Finding> {
        check_with(engine, directory, files, &[])
    }

    /// The same, with facts for named files.
    fn check_with(
        engine: &SpecPairEngine,
        directory: &str,
        files: &[&str],
        facts: &[(&str, &FileFacts)],
    ) -> Vec<Finding> {
        let directory = path(directory);
        let siblings = owned(files);

        files
            .iter()
            .flat_map(|name| {
                let file = directory.join(name).expect("valid path");
                let known = facts
                    .iter()
                    .find(|(named, _)| named == name)
                    .map(|(_, facts)| *facts);
                engine.check_file(FileContext {
                    path: &file,
                    facts: known,
                    docs: None,
                    siblings: &siblings,
                    exists: Exists::none(),
                    graph: None,
                })
            })
            .collect()
    }

    /// Facts for a file exporting one symbol of each given kind.
    fn exporting(kinds: &[archwarden_core::facts::ExportKind]) -> FileFacts {
        use archwarden_core::facts::{ExportFact, ExportTags, Span};

        FileFacts {
            path: path("x.ts"),
            content_hash: archwarden_core::hash::ContentHash::of(b""),
            imports: Vec::new(),
            exports: kinds
                .iter()
                .enumerate()
                .map(|(index, kind)| ExportFact {
                    name: Some(format!("Thing{index}")),
                    tags: ExportTags::only(*kind),
                    is_default: false,
                    reexport_from: None,
                    forwards: None,
                    annotations: Vec::new(),
                    span: Span::new(0, 0),
                })
                .collect(),
            calls: Vec::new(),
            allowances: Vec::new(),
            has_opaque_import: false,
        }
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

    /// A named subfolder is a directory inside a scope-selected one, and it
    /// covers everything below itself. Not the scope directory, and not a
    /// folder nobody named.
    ///
    /// The last assertion was `!governs(".../calcs/deep")` until issue #34.
    /// Grouping related calcs into a folder is an organisational choice, and
    /// it silently took them out of the gate — eleven validation functions in
    /// one repository had no test at all and had never been reported.
    #[test]
    fn a_named_subfolder_covers_everything_below_it() {
        let engine = engine(&["packages/domain/src/*"], &["calcs", "services"], &[]);

        assert!(engine.governs(&path("packages/domain/src/user/calcs")));
        assert!(engine.governs(&path("packages/domain/src/user/services")));
        assert!(
            engine.governs(&path("packages/domain/src/user/calcs/deep")),
            "a file does not leave the gate by being filed one level in"
        );
        assert!(engine.governs(&path("packages/domain/src/user/calcs/deep/deeper")));
        assert!(!engine.governs(&path("packages/domain/src/user")));
        assert!(!engine.governs(&path("packages/domain/src/user/types")));
        assert!(
            !engine.governs(&path("packages/domain/src/user/types/calcs")),
            "`calcs` is named relative to the scope directory, not found anywhere below it"
        );
    }

    /// The other half of the same gap: a nested path in `subfolders` used to
    /// be accepted by the schema, reported valid, and match nothing — because
    /// an entry was compared against a single directory *name*. It now names
    /// what it looks like it names. Issue #34.
    #[test]
    fn a_nested_subfolder_path_names_that_subtree() {
        let engine = engine(&["src/*"], &["calcs/group"], &[]);

        assert!(engine.governs(&path("src/Entity/calcs/group")));
        assert!(engine.governs(&path("src/Entity/calcs/group/deeper")));
        assert!(
            !engine.governs(&path("src/Entity/calcs")),
            "only the subtree that was named"
        );
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
            why: None,
            module_why: None,
            imports: None,
            level: Level::Warning,
            scope: Scope::compile(["src/*"]).expect("valid"),
            kind: CompiledRuleKind::SpecPair {
                subfolders: owned(["."].as_slice()),
                spec_markers: owned(&["test"]),
                ignore_files: PathSet::default(),
                spec_dirs: Vec::new(),
                require_non_empty_spec: false,
                skip_type_only: false,
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
            why: None,
            module_why: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid"),
            kind: CompiledRuleKind::SpecPair {
                subfolders: owned(["."].as_slice()),
                spec_markers: owned(&["spec"]),
                ignore_files: PathSet::default(),
                spec_dirs: Vec::new(),
                require_non_empty_spec: true,
                skip_type_only: false,
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
            why: None,
            module_why: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid"),
            kind: CompiledRuleKind::Structure {
                allowed_subfolders: Some(Vec::new()),
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: Vec::new(),
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
            why: None,
            module_why: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid"),
            kind: CompiledRuleKind::SpecPair {
                subfolders: owned(["."].as_slice()),
                spec_markers: owned(&["spec"]),
                ignore_files: PathSet::default(),
                spec_dirs: Vec::new(),
                require_non_empty_spec: true,
                skip_type_only: false,
            },
        };
        let strict = SpecPairEngine::from_rule(&rule).expect("is a spec-pair rule");

        assert!(strict.applies_to(&path("src/user/thing.spec.ts")));
        assert_eq!(strict.needs_facts(), FactsNeeded::Code);
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
            why: None,
            module_why: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid"),
            kind: CompiledRuleKind::SpecPair {
                subfolders: owned(["."].as_slice()),
                spec_markers: owned(&["spec"]),
                ignore_files: PathSet::default(),
                spec_dirs: Vec::new(),
                require_non_empty_spec: true,
                skip_type_only: false,
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
                docs: None,
                siblings: &[],
                exists: Exists::none(),
                graph: None,
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

    /// An engine with `skip_type_only` set.
    fn type_only_engine(scope: &[&str], subfolders: &[&str]) -> SpecPairEngine {
        let rule = CompiledRule {
            id: RuleId::new("needs-spec").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(scope).expect("valid scope"),
            kind: CompiledRuleKind::SpecPair {
                subfolders: owned(subfolders),
                spec_markers: owned(&["spec", "test"]),
                ignore_files: PathSet::default(),
                spec_dirs: Vec::new(),
                require_non_empty_spec: false,
                skip_type_only: true,
            },
        };

        SpecPairEngine::from_rule(&rule).expect("is a spec-pair rule")
    }

    /// The rule this flag exists for: a file that is nothing but a contract.
    ///
    /// Every `services/` and `adapters/` file in the domain layer of a real
    /// repository is this shape — `export interface LlmAdapter`, and no
    /// runtime export at all. A spec for one has nothing to call, so the spec
    /// that gets written to satisfy the rule tests a mock of the contract
    /// instead. `tsc` is what checks an interface, on every build.
    #[test]
    fn a_file_exporting_only_types_needs_no_spec() {
        use archwarden_core::facts::ExportKind;
        let engine = type_only_engine(&["packages/domain/src/*"], &["services"]);
        let facts = exporting(&[ExportKind::Interface, ExportKind::Type]);

        let findings = check_with(
            &engine,
            "packages/domain/src/cep/services",
            &["cep-lookup-service.ts"],
            &[("cep-lookup-service.ts", &facts)],
        );

        assert!(findings.is_empty(), "{findings:?}");
    }

    /// One runtime export is enough to bring the rule back. The flag is about
    /// files with nothing to test, not files with little to test.
    #[test]
    fn a_single_runtime_export_still_needs_a_spec() {
        use archwarden_core::facts::ExportKind;
        let engine = type_only_engine(&["packages/domain/src/*"], &["services"]);
        let facts = exporting(&[ExportKind::Interface, ExportKind::Function]);

        let findings = check_with(
            &engine,
            "packages/domain/src/cep/services",
            &["cep-lookup-service.ts"],
            &[("cep-lookup-service.ts", &facts)],
        );

        assert_eq!(offenders(&findings).len(), 1, "{findings:?}");
    }

    /// An `enum` exists at runtime. It has values a test can assert on, so it
    /// is not a contract in the sense this exemption means.
    #[test]
    fn an_enum_is_a_runtime_export() {
        use archwarden_core::facts::ExportKind;
        let engine = type_only_engine(&["src/*"], &["."]);
        let facts = exporting(&[ExportKind::Enum]);

        let findings = check_with(&engine, "src/user", &["kind.ts"], &[("kind.ts", &facts)]);

        assert_eq!(offenders(&findings).len(), 1, "{findings:?}");
    }

    /// A file exporting nothing is not a contract. It is a file nobody
    /// imports, and exempting it would turn the flag into a way to disappear
    /// from the rule by deleting the `export` keyword.
    #[test]
    fn a_file_with_no_exports_is_not_type_only() {
        let engine = type_only_engine(&["src/*"], &["."]);
        let facts = exporting(&[]);

        let findings = check_with(&engine, "src/user", &["thing.ts"], &[("thing.ts", &facts)]);

        assert_eq!(offenders(&findings).len(), 1, "{findings:?}");
    }

    /// Without the flag, exports are not consulted at all and a type-only file
    /// is reported exactly as it was before this existed.
    #[test]
    fn the_flag_is_off_by_default() {
        use archwarden_core::facts::ExportKind;
        let engine = engine(&["src/*"], &["."], &[]);
        let facts = exporting(&[ExportKind::Interface]);

        let findings = check_with(
            &engine,
            "src/user",
            &["contract.ts"],
            &[("contract.ts", &facts)],
        );

        assert_eq!(offenders(&findings).len(), 1, "{findings:?}");
    }

    /// Facts absent means the file would not parse. Exempting it would let an
    /// unparsable file slip the rule; the run counts it as a skipped check
    /// instead, which is what `checks_skipped` is for.
    #[test]
    fn a_file_that_could_not_be_parsed_is_not_exempted() {
        let engine = type_only_engine(&["src/*"], &["."]);

        let findings = check_with(&engine, "src/user", &["contract.ts"], &[]);

        assert_eq!(offenders(&findings).len(), 1, "{findings:?}");
    }

    /// The flag is what makes this rule read files at all. Without it the rule
    /// is still the cheap one that only looks at names.
    #[test]
    fn the_flag_is_what_opens_the_file() {
        assert_eq!(
            type_only_engine(&["src/*"], &["."]).needs_facts(),
            FactsNeeded::Code
        );
        assert_eq!(
            engine(&["src/*"], &["."], &[]).needs_facts(),
            FactsNeeded::Nothing
        );
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
    /// Issue #67. `spec-pair` accepted a sibling only, and a project keeping
    /// its specs in `__tests__` had a rule that reported every file.
    ///
    /// The directory is named by the author — `tests`, `__specs__`, whatever
    /// the project uses — and an empty list keeps the sibling-only behaviour
    /// every existing config has.
    #[test]
    fn a_spec_in_a_named_directory_satisfies_the_rule() {
        let engine = engine_with_spec_dirs(&["src/*"], &["."], &["__tests__"]);
        let there =
            |candidate: &RepoRelPath| candidate.as_str() == "src/user/__tests__/create.spec.ts";

        let findings = engine.check_file(FileContext {
            path: &path("src/user/create.ts"),
            facts: None,
            docs: None,
            siblings: &owned(&["create.ts"]),
            exists: Exists::new(&there),
            graph: None,
        });

        assert!(
            findings.is_empty(),
            "the spec is in the directory the rule names: {findings:?}"
        );
    }

    /// A directory the rule does not name is not a spec directory.
    ///
    /// This is the test that keeps the feature from being a way to switch the
    /// rule off. A permissive reading — a spec anywhere below counts — reports
    /// nothing and looks exactly like a repository that is fully tested.
    #[test]
    fn a_spec_in_a_directory_the_rule_did_not_name_does_not_count() {
        let engine = engine_with_spec_dirs(&["src/*"], &["."], &["__tests__"]);
        let elsewhere =
            |candidate: &RepoRelPath| candidate.as_str() == "src/user/spec/create.spec.ts";

        let findings = engine.check_file(FileContext {
            path: &path("src/user/create.ts"),
            facts: None,
            docs: None,
            siblings: &owned(&["create.ts"]),
            exists: Exists::new(&elsewhere),
            graph: None,
        });

        assert_eq!(findings.len(), 1, "`spec/` was never named: {findings:?}");
    }

    /// And it reaches one level, not the whole subtree. `__tests__/unit/` is a
    /// directory of its own, and naming `__tests__` did not name it.
    #[test]
    fn a_spec_nested_deeper_than_the_named_directory_does_not_count() {
        let engine = engine_with_spec_dirs(&["src/*"], &["."], &["__tests__"]);
        let deeper = |candidate: &RepoRelPath| {
            candidate.as_str() == "src/user/__tests__/unit/create.spec.ts"
        };

        let findings = engine.check_file(FileContext {
            path: &path("src/user/create.ts"),
            facts: None,
            docs: None,
            siblings: &owned(&["create.ts"]),
            exists: Exists::new(&deeper),
            graph: None,
        });

        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    /// A rule that names no directory behaves exactly as it did before, which
    /// is what keeps every config written until now working.
    #[test]
    fn naming_no_directory_keeps_the_sibling_only_rule() {
        let engine = engine_with_spec_dirs(&["src/*"], &["."], &[]);
        let anywhere = |_: &RepoRelPath| true;

        let findings = engine.check_file(FileContext {
            path: &path("src/user/create.ts"),
            facts: None,
            docs: None,
            siblings: &owned(&["create.ts"]),
            exists: Exists::new(&anywhere),
            graph: None,
        });

        assert_eq!(
            findings.len(),
            1,
            "with no directory named, only a sibling counts: {findings:?}"
        );
    }
}
