//! Rules with every glob and every regex already compiled.
//!
//! This is the other half of "parse, don't validate": a [`CompiledRule`]
//! cannot be constructed unless its scope globs and its filename patterns all
//! parsed, so nothing downstream ever asks whether a pattern is valid. Turning
//! a config into these values *is* what validating it means.
//!
//! Lowering lives in `archwarden-config`, which owns the wire format. This
//! module owns only the result.

use crate::{
    facts::KindFilter,
    glob::PathSet,
    hash::ContentHash,
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    pattern::Pattern,
    scope::Scope,
};

/// How far a `skip_dirs` exemption reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename_all = "lowercase"))]
#[non_exhaustive]
pub enum SkipScope {
    /// Exempt from `structure` rules only. Files inside are still parsed and
    /// still enter the import graph.
    #[default]
    Structure,
    /// Removed from the walk entirely, and therefore invisible to every rule.
    Walk,
}

/// The compiled `_`-prefix escape hatch.
#[derive(Debug, Clone, Default)]
pub struct SkipDirs {
    /// Directory name prefixes. Empty disables the escape hatch.
    pub prefixes: Vec<String>,
    /// Globs, for what a prefix cannot express.
    pub globs: PathSet,
    /// How far the exemption reaches.
    pub scope: SkipScope,
}

impl SkipDirs {
    /// Whether a directory is exempt.
    ///
    /// Takes the directory's own name and its full path, because a prefix
    /// applies to the name while a glob applies to the path.
    #[must_use]
    pub fn exempts(&self, directory: &RepoRelPath) -> bool {
        let named = directory.file_name().is_some_and(|name| {
            self.prefixes
                .iter()
                .any(|p| !p.is_empty() && name.starts_with(p))
        });

        named || self.globs.is_match(directory.as_path())
    }
}

/// What a compiled rule requires, by category.
///
/// Deliberately **not** `#[non_exhaustive]`, unlike [`Observed`] and
/// [`Expectation`](crate::finding::Expectation). Those two are matched by
/// downstream code that must keep compiling when a variant appears;
/// `archwarden-rules` matches this one exhaustively *on purpose*, so that a
/// kind added without an engine fails to build. The eight crates version in
/// lockstep and there is no independent downstream, so the attribute would buy
/// nothing and would cost that guarantee.
///
/// [`Observed`]: crate::finding::Observed
#[derive(Debug, Clone)]
pub enum CompiledRuleKind {
    /// Which subdirectories may exist, and which filenames.
    Structure {
        /// Subdirectory names that are permitted.
        allowed_subfolders: Vec<String>,
        /// Names permitted but reported as warnings, whatever the rule's level.
        warn_subfolders: Vec<String>,
        /// Subdirectories carrying the same contract, recursively.
        recurse_into: Vec<String>,
        /// Every direct child file must match one of these.
        filename_patterns: Vec<Pattern>,
    },
    /// The filename dictates the exported symbol's name.
    Naming {
        /// Regex over the filename, with the capture groups the template uses.
        file_pattern: Pattern,
        /// Regex over the name of the containing directory, contributing its
        /// own capture groups to the same template.
        ///
        /// `None` for the common rule, whose export name is spelled from the
        /// filename alone.
        dir_pattern: Option<Pattern>,
        /// The required name, as a template over those groups.
        name_template: String,
        /// Which declaration forms satisfy the rule.
        kind: KindFilter,
        /// A signature shown by `scaffold`. Never verified.
        signature_hint: Option<String>,
    },
    /// Every unit file needs a spec sibling.
    SpecPair {
        /// Subdirectories subject to the rule. `.` means the scope itself.
        subfolders: Vec<String>,
        /// The markers that make a filename a spec: `spec`, `test`, or both.
        ///
        /// A marker, not a whole suffix. The extension comes from the source
        /// file, because `Component.tsx` wanting `Component.spec.tsx` is
        /// mechanical rather than a preference anyone configures. Which marker
        /// a project uses *is* a preference, and vitest and jest both accept
        /// either.
        spec_markers: Vec<String>,
        /// Files exempted from the rule.
        ignore_files: PathSet,
        /// Whether the spec must contain at least one `it` or `test` call.
        require_non_empty_spec: bool,
        /// Whether a file whose exports are all `type` or `interface` is
        /// exempt. A file with no runtime export has nothing to test, and the
        /// spec written to satisfy the rule tests a mock rather than the
        /// contract. See `docs/RULES.md`.
        skip_type_only: bool,
    },
    /// A file whose whole content is forwarding another module.
    NoPassthrough {
        /// Which shapes of forwarding count.
        forms: PassthroughForms,
        /// Files exempted, as globs.
        except: PathSet,
        /// Whether a file a `package.json` `exports` entry points at is exempt.
        allow_package_entrypoints: bool,
        /// Whether a file that forwards some exports and declares others is
        /// allowed.
        allow_partial: bool,
    },
    /// Layer A may not import from layer B, or must import from layer C.
    ImportBoundary {
        /// Resolved import paths that are illegal.
        forbid: PathSet,
        /// Resolved import paths at least one import must match.
        require: PathSet,
        /// Package names that are illegal, matched as "this package, and
        /// anything under it".
        ///
        /// Kept as plain names rather than compiled globs: a dependency has no
        /// repo-relative path, and under pnpm's store layout or yarn `PnP` it may
        /// have no path this repository could name at all.
        forbid_packages: Vec<String>,
        /// Exceptions to `forbid`.
        except: PathSet,
        /// Importing files exempt from the whole rule.
        except_from: PathSet,
        /// Whether `import type` counts.
        include_type_only: bool,
    },
    /// Files matching a pattern must call a symbol.
    CallObligation {
        /// Regex over the filename.
        file_pattern: Pattern,
        /// The callee, as it appears at a call site.
        symbol: String,
        /// The module the symbol must come from.
        imported_from: String,
    },
}

impl CompiledRuleKind {
    /// The discriminator, as written in the config.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Structure { .. } => "structure",
            Self::Naming { .. } => "naming",
            Self::SpecPair { .. } => "spec-pair",
            Self::NoPassthrough { .. } => "no-passthrough",
            Self::ImportBoundary { .. } => "import-boundary",
            Self::CallObligation { .. } => "call-obligation",
        }
    }

    /// Whether evaluating this rule needs the file to have been parsed.
    ///
    /// The walk uses this to avoid parsing files no rule looks inside, which
    /// on a structure-only run is most of them.
    #[must_use]
    pub fn needs_parse(&self) -> bool {
        match self {
            Self::Structure { .. } => false,
            Self::SpecPair {
                require_non_empty_spec,
                skip_type_only,
                ..
            } => *require_non_empty_spec || *skip_type_only,
            Self::Naming { .. }
            | Self::ImportBoundary { .. }
            | Self::CallObligation { .. }
            | Self::NoPassthrough { .. } => true,
        }
    }
}

/// Which shapes of pure forwarding a `no-passthrough` rule refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassthroughForms {
    /// `export { A } from './x'`, or an import followed by an export of it.
    pub reexport: bool,
    /// `export const A = B`, `export type A = B`.
    pub alias: bool,
    /// A function whose whole body is `return g(<its own parameters>)`.
    pub wrapper: bool,
}

/// One rule, ready to evaluate.
#[derive(Debug, Clone)]
pub struct CompiledRule {
    /// Stable identifier.
    pub id: RuleId,
    /// The module it was declared under, if any.
    pub module: Option<ModuleId>,
    /// Severity of its findings.
    pub level: Level,
    /// The directories it applies to.
    pub scope: Scope,
    /// What it requires.
    pub kind: CompiledRuleKind,
}

impl CompiledRule {
    /// Whether this rule has anything to say about `path`.
    ///
    /// Purely lexical, and that is load-bearing: `describe` and the pre-write
    /// hook ask this about files that do not exist yet.
    #[must_use]
    pub fn applies_to_file(&self, path: &RepoRelPath) -> bool {
        self.scope.contains_file(path.as_path())
    }

    /// Whether this rule has anything to say about `directory`.
    #[must_use]
    pub fn applies_to_directory(&self, directory: &RepoRelPath) -> bool {
        self.scope.matches_dir(directory.as_path())
    }
}

/// A config with everything compiled.
#[derive(Debug, Clone)]
pub struct CompiledConfig {
    rules: Vec<CompiledRule>,
    ignore: PathSet,
    skip_dirs: SkipDirs,
    rules_hash: ContentHash,
}

impl CompiledConfig {
    /// Builds a compiled config.
    #[must_use]
    pub fn new(
        rules: Vec<CompiledRule>,
        ignore: PathSet,
        skip_dirs: SkipDirs,
        rules_hash: ContentHash,
    ) -> Self {
        Self {
            rules,
            ignore,
            skip_dirs,
            rules_hash,
        }
    }

    /// Every rule, in declaration order.
    pub fn rules(&self) -> impl Iterator<Item = &CompiledRule> {
        self.rules.iter()
    }

    /// How many rules are active.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// The rules that apply to a file.
    ///
    /// Ignored paths yield nothing: an `ignore` entry wins over any rule's
    /// scope, however specific that scope is. A kill-switch that can be
    /// overridden by accident is not one.
    pub fn rules_for_file(&self, path: &RepoRelPath) -> impl Iterator<Item = &CompiledRule> {
        let ignored = self.is_ignored(path);
        self.rules
            .iter()
            .filter(move |rule| !ignored && rule.applies_to_file(path))
    }

    /// Whether a path is excluded from analysis entirely.
    #[must_use]
    pub fn is_ignored(&self, path: &RepoRelPath) -> bool {
        self.ignore.is_match(path.as_path())
    }

    /// The compiled `ignore` globs.
    ///
    /// Exposed so the walk can clone them into its pruning closure, which the
    /// walker requires to be `'static`.
    #[must_use]
    pub fn ignore_globs(&self) -> &PathSet {
        &self.ignore
    }

    /// The escape-hatch configuration.
    #[must_use]
    pub fn skip_dirs(&self) -> &SkipDirs {
        &self.skip_dirs
    }

    /// A hash of the effective rule set, for the `findings` cache key.
    #[must_use]
    pub fn rules_hash(&self) -> ContentHash {
        self.rules_hash
    }

    /// Whether any rule requires parsing.
    ///
    /// A run whose rules are all structural never needs a parser at all.
    #[must_use]
    pub fn needs_parse(&self) -> bool {
        self.rules.iter().any(|rule| rule.kind.needs_parse())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{ExportKind, ExportTags};

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn rule(id: &str, scope: &[&str], kind: CompiledRuleKind) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(scope).expect("valid scope"),
            kind,
        }
    }

    fn structure() -> CompiledRuleKind {
        CompiledRuleKind::Structure {
            allowed_subfolders: vec!["types".to_owned()],
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            filename_patterns: Vec::new(),
        }
    }

    fn naming() -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile(r"^(?<name>[a-z-]+)\.ts$").expect("valid"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            signature_hint: None,
        }
    }

    fn config(rules: Vec<CompiledRule>, ignore: &[&str]) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::compile(ignore).expect("valid ignore"),
            SkipDirs::default(),
            ContentHash::of(b"rules"),
        )
    }

    #[test]
    fn a_rule_applies_to_files_inside_its_scope() {
        let rule = rule("r", &["packages/domain/src/*"], structure());

        assert!(rule.applies_to_file(&path("packages/domain/src/user/user.ts")));
        assert!(!rule.applies_to_file(&path("packages/application/src/x/y.ts")));
        assert!(rule.applies_to_directory(&path("packages/domain/src/user")));
        assert!(!rule.applies_to_directory(&path("packages/domain/src")));
    }

    /// The matcher must answer for a file that does not exist, because that is
    /// what `describe` and the pre-write hook ask about.
    #[test]
    fn the_matcher_answers_for_a_file_that_does_not_exist() {
        let config = config(vec![rule("r", &["src/*"], structure())], &[]);
        let hypothetical = path("src/never-written/foo.ts");

        assert_eq!(config.rules_for_file(&hypothetical).count(), 1);
    }

    /// An ignore entry beats any scope, however specific. Decision 6.
    #[test]
    fn an_ignored_path_matches_no_rule_however_specific_the_scope() {
        let config = config(
            vec![rule("r", &["packages/domain/src/generated/*"], structure())],
            &["**/generated/**"],
        );
        let ignored = path("packages/domain/src/generated/deep/x.ts");

        assert!(config.is_ignored(&ignored));
        assert_eq!(config.rules_for_file(&ignored).count(), 0);
    }

    /// The walk clones these into its pruning closure rather than asking
    /// `is_ignored` per entry, so that an `ignore` of `**/node_modules/**`
    /// stops the walk at the boundary instead of descending into it.
    #[test]
    fn the_ignore_globs_are_reachable_for_the_walk_to_prune_with() {
        let config = config(vec![], &["**/node_modules/**"]);

        let globs = config.ignore_globs();
        assert_eq!(globs.patterns(), ["**/node_modules/**"]);
        assert!(globs.is_match(path("packages/app/node_modules/x/index.js").as_path()));
        assert!(!globs.is_match(path("packages/app/src/x.ts").as_path()));
    }

    #[test]
    fn rules_are_reported_in_declaration_order() {
        let config = config(
            vec![
                rule("first", &["src/*"], structure()),
                rule("second", &["src/*"], naming()),
            ],
            &[],
        );

        let ids: Vec<_> = config.rules().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["first", "second"]);
        assert_eq!(config.rule_count(), 2);
    }

    /// A run whose rules are all structural never needs a parser. Knowing that
    /// up front is what keeps a structure-only check off the AST entirely.
    #[test]
    fn a_structure_only_config_needs_no_parser() {
        let structural = config(vec![rule("s", &["src/*"], structure())], &[]);
        assert!(!structural.needs_parse());

        let with_naming = config(
            vec![
                rule("s", &["src/*"], structure()),
                rule("n", &["src/*"], naming()),
            ],
            &[],
        );
        assert!(with_naming.needs_parse());
    }

    /// `spec-pair` is the one rule whose parsing need depends on a field
    /// rather than on its category: only `require_non_empty_spec` opens the
    /// file.
    #[test]
    fn spec_pair_needs_a_parser_only_when_it_inspects_the_spec() {
        let cheap = CompiledRuleKind::SpecPair {
            subfolders: vec![".".to_owned()],
            spec_markers: vec!["spec".to_owned()],
            ignore_files: PathSet::default(),
            require_non_empty_spec: false,
            skip_type_only: false,
        };
        let thorough = CompiledRuleKind::SpecPair {
            subfolders: vec![".".to_owned()],
            spec_markers: vec!["spec".to_owned()],
            ignore_files: PathSet::default(),
            require_non_empty_spec: true,
            skip_type_only: false,
        };

        assert!(!cheap.needs_parse());
        assert!(thorough.needs_parse());
    }

    #[test]
    fn every_kind_reports_its_config_spelling() {
        let kinds = [
            structure(),
            naming(),
            CompiledRuleKind::SpecPair {
                subfolders: Vec::new(),
                spec_markers: vec!["spec".to_owned()],
                ignore_files: PathSet::default(),
                require_non_empty_spec: false,
                skip_type_only: false,
            },
            CompiledRuleKind::ImportBoundary {
                forbid: PathSet::default(),
                require: PathSet::default(),
                forbid_packages: Vec::new(),
                except: PathSet::default(),
                except_from: PathSet::default(),
                include_type_only: true,
            },
            CompiledRuleKind::CallObligation {
                file_pattern: Pattern::compile("^x$").expect("valid"),
                symbol: "Event.save".to_owned(),
                imported_from: "@org/domain".to_owned(),
            },
        ];

        let names: Vec<_> = kinds.iter().map(CompiledRuleKind::type_name).collect();
        assert_eq!(
            names,
            [
                "structure",
                "naming",
                "spec-pair",
                "import-boundary",
                "call-obligation"
            ]
        );

        // Boundaries and call obligations both need resolved imports.
        assert!(kinds[3].needs_parse());
        assert!(kinds[4].needs_parse());
    }

    /// The default escape hatch exempts `_`-prefixed directories, and only by
    /// name -- a directory merely *containing* an underscore is not exempt.
    #[test]
    fn the_escape_hatch_matches_a_prefix_on_the_directory_name() {
        let skip = SkipDirs {
            prefixes: vec!["_".to_owned()],
            globs: PathSet::default(),
            scope: SkipScope::Structure,
        };

        assert!(skip.exempts(&path("packages/domain/src/_internal")));
        assert!(!skip.exempts(&path("packages/domain/src/my_helpers")));
        assert!(!skip.exempts(&path("packages/domain/src/user")));
    }

    /// An empty prefix would match every directory, which would silently
    /// disable every structure rule in the repository.
    #[test]
    fn an_empty_prefix_exempts_nothing() {
        let skip = SkipDirs {
            prefixes: vec![String::new()],
            globs: PathSet::default(),
            scope: SkipScope::Structure,
        };

        assert!(!skip.exempts(&path("packages/domain/src/user")));
    }

    #[test]
    fn the_escape_hatch_also_takes_globs() {
        let skip = SkipDirs {
            prefixes: Vec::new(),
            globs: PathSet::compile(["**/__generated__"]).expect("valid"),
            scope: SkipScope::Walk,
        };

        assert!(skip.exempts(&path("packages/domain/src/__generated__")));
        assert!(!skip.exempts(&path("packages/domain/src/user")));
        assert_eq!(skip.scope, SkipScope::Walk);
    }

    #[test]
    fn an_empty_escape_hatch_exempts_nothing() {
        let empty = SkipDirs::default();
        assert!(!empty.exempts(&path("packages/domain/src/_internal")));
        assert_eq!(empty.scope, SkipScope::Structure);
    }

    /// The rules hash is what the `findings` cache key folds in, so it has to
    /// survive compilation intact.
    #[test]
    fn the_rules_hash_is_carried_through() {
        let hash = ContentHash::of(b"the effective rules");
        let config = CompiledConfig::new(Vec::new(), PathSet::default(), SkipDirs::default(), hash);

        assert_eq!(config.rules_hash(), hash);
        assert_eq!(config.rule_count(), 0);
        assert!(!config.needs_parse());
        assert!(config.skip_dirs().prefixes.is_empty());
    }

    /// A compiled rule is handed to workers by value, so it has to clone, and
    /// it is printed when a diagnostic needs to say what it holds.
    #[test]
    fn a_compiled_config_clones_and_prints() {
        let config = config(vec![rule("r", &["src/*"], naming())], &[]);
        let copy = config.clone();

        assert_eq!(copy.rule_count(), config.rule_count());
        assert!(format!("{config:?}").contains("CompiledConfig"));
        assert!(format!("{:?}", SkipScope::Walk).contains("Walk"));
    }
}
