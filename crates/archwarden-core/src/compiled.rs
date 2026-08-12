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

/// Which languages the configuration asked archwarden to read.
///
/// Carried rather than assumed, because a file in a language nobody asked for
/// is a *counted, named* skip and not a silent pass. See issue #13.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Languages {
    /// Astro components. JS/TS is always read and needs no flag: a
    /// configuration that asked for nothing still means TypeScript.
    pub astro: bool,
}

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
        ///
        /// `None` when the rule says nothing about subfolders; `Some([])` when
        /// it permits none of them. See `StructureRule::allowed_subfolders`.
        allowed_subfolders: Option<Vec<String>>,
        /// Names permitted but reported as warnings, whatever the rule's level.
        warn_subfolders: Vec<String>,
        /// Subdirectories carrying the same contract, recursively.
        recurse_into: Vec<String>,
        /// Regexes a direct child *directory*'s name may match instead of
        /// being named in `allowed_subfolders`.
        subfolder_patterns: Vec<Pattern>,
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
        /// The type annotations that satisfy the rule, any one of them, as
        /// templates over the same groups. Empty when the rule asks for none,
        /// which is every rule written before the field existed.
        annotation: Vec<String>,
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
        /// Directory names beside the file where a spec also counts. Empty is
        /// sibling-only. One level deep; see `SpecPairRule::spec_dirs`.
        spec_dirs: Vec<String>,
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
    /// No file in scope may sit on an import loop.
    ///
    /// The only kind whose question cannot be answered from one file, which is
    /// why the engine reads the import graph and why a configuration carrying
    /// one costs a resolution pass over the whole repository. See
    /// `RuleEngine::needs_graph`.
    ImportCycle {
        /// Whether `import type` closes a loop.
        ///
        /// A type import is erased at runtime, so a loop made only of them
        /// cannot deadlock anything — and it is still a loop the compiler
        /// walks. Spelled the same way `ImportBoundary` spells it, and read at
        /// query time, so one graph answers both.
        include_type_only: bool,
    },
    /// Layer A may not import from layer B, or must import from layer C.
    ImportBoundary {
        /// Resolved import paths that are illegal.
        forbid: PathSet,
        /// Resolved import paths at least one import must match.
        require: PathSet,
        /// Resolved import paths that are the *only* ones allowed.
        ///
        /// `None` means the rule does not work this way. An empty `PathSet`
        /// would mean "nothing is allowed", which is a different and much
        /// louder statement, so the two must not be the same value.
        allow: Option<PathSet>,
        /// Package names that are the only ones allowed. `None` as above.
        allow_packages: Option<Vec<String>>,
        /// The groups this rule quantifies over, one `PathSet` each.
        ///
        /// A rule about a *kind* covers every module wearing it, and its scope
        /// is their union — so "may this file import that one?" cannot be
        /// answered by asking whether the target is in scope: for
        /// `from_kind: "app"` forbidding other apps, every app is in scope and
        /// the union would exempt exactly the imports the rule exists to
        /// refuse.
        ///
        /// Kept apart so the exemption can be "the same group", which is what
        /// anybody means: an assembly may import itself and not its siblings.
        /// Identity decides it, never the label. Issue #76.
        groups: Vec<PathSet>,
        /// Package names that are illegal, matched as "this package, and
        /// anything under it".
        ///
        /// Kept as plain names rather than compiled globs: a dependency has no
        /// repo-relative path, and under pnpm's store layout or yarn `PnP` it may
        /// have no path this repository could name at all.
        forbid_packages: Vec<String>,
        /// Resolved import paths this file may not *end up* depending on,
        /// however many files away.
        ///
        /// Empty for almost every rule, and that emptiness is load-bearing:
        /// it is what `RuleEngine::needs_graph` answers from, and a graph
        /// costs a resolution pass over the whole repository. A boundary rule
        /// that does not ask about reach must stay as cheap as it was.
        ///
        /// Direct imports are `forbid`'s to report. This is about the
        /// dependency nobody wrote down. Issue #71.
        forbid_reaching: PathSet,
        /// Exceptions to `forbid`, and to `forbid_reaching`.
        ///
        /// One field for both because it means the same thing to each: a
        /// destination this rule tolerates. "May not reach `packages/db`,
        /// except `packages/db/types`" is the sentence somebody writes, and a
        /// second `except_reaching` would be a field whose only purpose is to
        /// be forgotten.
        except: PathSet,
        /// Importing files exempt from the whole rule.
        except_from: PathSet,
        /// Whether `import type` counts.
        include_type_only: bool,
    },
    /// Files that must exist in each governed directory.
    Presence {
        /// Filenames that must be there. Names, not paths — an entry with a
        /// separator is refused when the config compiles.
        require: Vec<String>,
        /// Regexes at least one file must match, one file per entry.
        require_any: Vec<Pattern>,
    },
    /// A file of one kind must have a companion of another.
    Pair {
        /// Regex over the filename of the file that needs a companion.
        file_pattern: Pattern,
        /// The companion, relative to the directory the file sits in. May
        /// start with `../`.
        must_exist: String,
    },
    /// A document's frontmatter must carry these keys.
    Frontmatter {
        /// Regex over the filename of the documents this rule is about.
        file_pattern: Pattern,
        /// Keys the block must carry.
        require: Vec<String>,
        /// The closed vocabulary a key's value must come from, as text.
        one_of: Vec<(String, Vec<String>)>,
        /// A key whose value must equal this template, rendered from the path.
        equals: Vec<(String, String)>,
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
            Self::ImportCycle { .. } => "import-cycle",
            Self::Presence { .. } => "presence",
            Self::Pair { .. } => "pair",
            Self::Frontmatter { .. } => "frontmatter",
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
            // The first three ask only whether a name is on disk.
            // `Frontmatter` does read a file -- but not through *this*
            // front-end, and this method answers only for that one.
            // `RuleEngine::needs_facts` is what says which front-end a rule
            // wants.
            Self::Structure { .. }
            | Self::Presence { .. }
            | Self::Pair { .. }
            | Self::Frontmatter { .. } => false,
            Self::SpecPair {
                require_non_empty_spec,
                skip_type_only,
                ..
            } => *require_non_empty_spec || *skip_type_only,
            Self::Naming { .. }
            | Self::ImportBoundary { .. }
            | Self::ImportCycle { .. }
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
    /// Why this rule exists, as its author wrote it.
    ///
    /// Prose, carried rather than interpreted. It is shown wherever a user or
    /// an agent meets the rule — the pre-write hook's denial, `describe`,
    /// `scaffold`, `agent-guide`, `config explain`, and beside a finding — and
    /// it never changes what the rule decides. Issue #46.
    pub why: Option<String>,
    /// Why the *module* this rule was declared under exists.
    ///
    /// A separate field, not a fallback: "why is `domain` sealed" explains
    /// eight rules at once and is not an answer to "why this one". Both are
    /// shown; neither stands in for the other.
    pub module_why: Option<String>,
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
    modules: Vec<CompiledModule>,
    ignore: PathSet,
    skip_dirs: SkipDirs,
    rules_hash: ContentHash,
    languages: Languages,
}

/// A module, as the rest of the run sees it.
#[derive(Debug, Clone)]
pub struct CompiledModule {
    /// The label.
    pub id: ModuleId,
    /// The paths it is, when it declared any.
    ///
    /// `None` is what a module has always been: a namespace for rules, with no
    /// paths of its own. Everything a scope unlocks — narrowing the rules
    /// inside it, being named by a boundary, being asked whether it reaches
    /// anything — is unavailable to those, deliberately, because inventing a
    /// scope for them would be guessing at the thing the field exists to state.
    pub scope: Option<Scope>,
    /// What sort of module it is, when it said.
    ///
    /// A module with no kind is outside every rule that quantifies over kinds
    /// — which is the omission problem the quantifier exists to remove,
    /// reappearing one level up. `config doctor` names them.
    pub kind: Option<String>,
}

impl CompiledConfig {
    /// Records which languages the configuration asked for.
    ///
    /// A builder step rather than a fifth parameter to `new`: every caller that
    /// does not care -- which is every test of a rule -- keeps the constructor
    /// it had, and the one that does says so in a line that names what it is
    /// setting.
    #[must_use]
    pub fn with_languages(mut self, languages: Languages) -> Self {
        self.languages = languages;
        self
    }

    /// Which languages this configuration asked archwarden to read.
    #[must_use]
    pub fn languages(&self) -> Languages {
        self.languages
    }

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
            modules: Vec::new(),
            ignore,
            skip_dirs,
            rules_hash,
            languages: Languages::default(),
        }
    }

    /// Records the modules the configuration declared.
    ///
    /// A builder step for the same reason `with_languages` is one: every test
    /// of a rule keeps the constructor it had, and the caller that cares says
    /// so on a line that names what it is setting.
    #[must_use]
    pub fn with_modules(mut self, modules: Vec<CompiledModule>) -> Self {
        self.modules = modules;
        self
    }

    /// Every module, in declaration order.
    ///
    /// Carried past compilation because two questions need them and neither
    /// is a rule's: whether a module reaches any file, and whether any rule
    /// references it. Both are `config doctor`'s, and neither could be asked
    /// while a module was only a namespace. Issue #74.
    pub fn modules(&self) -> impl Iterator<Item = &CompiledModule> {
        self.modules.iter()
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
            why: None,
            module_why: None,
            level: Level::Error,
            scope: Scope::compile(scope).expect("valid scope"),
            kind,
        }
    }

    fn structure() -> CompiledRuleKind {
        CompiledRuleKind::Structure {
            allowed_subfolders: Some(vec!["types".to_owned()]),
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
            filename_patterns: Vec::new(),
        }
    }

    fn naming() -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile(r"^(?<name>[a-z-]+)\.ts$").expect("valid"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            annotation: Vec::new(),
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

    /// The modules a config declared travel with it, because two questions
    /// need them and neither belongs to a rule: whether a module reaches any
    /// file, and whether anything references it. Issue #74.
    #[test]
    fn the_modules_a_configuration_declared_travel_with_it() {
        let declared = vec![
            CompiledModule {
                id: ModuleId::new("domain").expect("valid id"),
                kind: None,
                scope: Some(Scope::compile(["packages/domain/**"]).expect("valid scope")),
            },
            CompiledModule {
                id: ModuleId::new("loose").expect("valid id"),
                kind: None,
                scope: None,
            },
        ];

        let compiled = config(Vec::new(), &[]).with_modules(declared);

        let seen: Vec<&str> = compiled.modules().map(|m| m.id.as_str()).collect();
        assert_eq!(seen, ["domain", "loose"]);

        let domain = compiled.modules().next().expect("the first");
        assert!(
            domain
                .scope
                .as_ref()
                .is_some_and(|s| s.matches_dir(camino::Utf8Path::new("packages/domain/src"))),
        );
        assert!(
            compiled.modules().nth(1).is_some_and(|m| m.scope.is_none()),
            "a module with no paths is what a module has always been"
        );
    }

    /// And a configuration that declared none has none, rather than an
    /// invented empty module for the rules that belong to no module.
    #[test]
    fn a_configuration_with_no_modules_reports_none() {
        assert_eq!(config(Vec::new(), &[]).modules().count(), 0);
    }

    /// Which languages a configuration asked for travels with it, and a
    /// configuration that asked for nothing still means TypeScript.
    ///
    /// A builder step rather than a fifth constructor parameter, so every test
    /// of a rule keeps the constructor it had — which is why it needs a test of
    /// its own here.
    #[test]
    fn the_languages_a_config_asked_for_travel_with_it() {
        let bare = CompiledConfig::new(
            Vec::new(),
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b""),
        );
        assert!(!bare.languages().astro, "nobody asked for Astro");

        let asked = CompiledConfig::new(
            Vec::new(),
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b""),
        )
        .with_languages(Languages { astro: true });
        assert!(asked.languages().astro);
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
            spec_dirs: Vec::new(),
            require_non_empty_spec: false,
            skip_type_only: false,
        };
        let thorough = CompiledRuleKind::SpecPair {
            subfolders: vec![".".to_owned()],
            spec_markers: vec!["spec".to_owned()],
            ignore_files: PathSet::default(),
            spec_dirs: Vec::new(),
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
                spec_dirs: Vec::new(),
                require_non_empty_spec: false,
                skip_type_only: false,
            },
            CompiledRuleKind::ImportBoundary {
                forbid: PathSet::default(),
                groups: Vec::new(),
                allow: None,
                allow_packages: None,
                require: PathSet::default(),
                forbid_packages: Vec::new(),
                forbid_reaching: PathSet::default(),
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
