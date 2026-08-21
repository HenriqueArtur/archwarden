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
    ids::{DecisionId, ModuleId, RuleId},
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
    /// Rust. Off unless the config named it, so a `src-tauri/` beside a `src/`
    /// is a counted skip rather than a tree held to rules written for the
    /// other half of the repository.
    pub rust: bool,
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
        /// Files this rule does not ask about.
        ///
        /// Repo-relative globs, spelled the way `spec-pair` spells them.
        /// Separate from the top-level `ignore`, which hides a file from
        /// *every* rule -- so a repository wanting one rule to skip a file and
        /// another to see it had to choose. Issue #153.
        ///
        /// Barrels are not in here. `mod.rs` and `index.ts` are exempt by
        /// construction, because nobody should have to declare that a module
        /// declaration exports nothing.
        ignore_files: PathSet,
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
    /// A file's header must declare these keys about itself.
    ///
    /// `frontmatter`'s three claims, asked of code. The values live in
    /// `archwarden-<key>:` comments above the first statement, and the shape is
    /// deliberately the document rule's: two kinds asking the same question of
    /// two file formats should look the same. Issue #104.
    Metadata {
        /// Keys the header must declare.
        require: Vec<String>,
        /// The closed vocabulary a key's value must come from, as text.
        one_of: Vec<(String, Vec<String>)>,
        /// A key whose value must equal this template, rendered from the path.
        equals: Vec<(String, String)>,
        /// Keys whose value is an ISO date that must not have passed.
        ///
        /// Compared against the run's day, never a clock: a rule that read the
        /// time would answer differently in CI than on a laptop, which is the
        /// determinism decision 28 defended. Issue #117.
        deadline: Vec<String>,
    },
    /// Files matching a pattern must call a symbol.
    CallObligation {
        /// Regex over the filename.
        file_pattern: Pattern,
        /// The callee, as it appears at a call site.
        symbol: String,
        /// The module the symbol must come from.
        imported_from: String,
        /// Options the call must carry.
        ///
        /// A key with `None` asks only that it be there. Empty means the rule
        /// does not ask, which is what every rule written before #164 does.
        with_options: Vec<(String, Option<String>)>,
    },
    /// A capability that may only be reached from one place.
    ///
    /// Every `forbid_*` field in the config is about an import. This is about
    /// a *call*, which is what is left over once `import-boundary` has cut
    /// every capability that arrives through a specifier: `process.env`,
    /// `Date.now`, `fetch`, `localStorage`, and the project's own symbols
    /// reached through an object imported legitimately somewhere else. Those
    /// have no edge in the graph to cut. Issue #118.
    ///
    /// An allowlist and no forbid direction. `only_in` is the one that does
    /// not decay -- a new file outside the chokepoint is reported the day it
    /// is written, where a `forbid` list would have to be extended by whoever
    /// added the thing it should have forbidden.
    Chokepoint {
        /// The callees, as they appear at a call site.
        callee: Vec<String>,
        /// The JSX elements, as they appear in markup.
        ///
        /// A render is a use, which is what this rule guards -- and it is a
        /// *different* use from a call. `<Card />` compiles to one, and a rule
        /// naming `Card` under `callee` must not start matching markup.
        /// Issue #145.
        renders: Vec<String>,
        /// The files allowed to reach them.
        only_in: Scope,
    },
    /// Two vocabularies that have to agree: every name called here is
    /// declared there, and every name declared there is called.
    ///
    /// The seam a Tauri application is joined by. `invoke("save_document")`
    /// and `#[tauri::command] fn save_document` are the same edge and there is
    /// no import between them -- the coupling is a string on one side and an
    /// attribute on the other, in different languages, checked by nothing until
    /// a user clicks the button.
    ///
    /// Deliberately not a `tauri` rule. A framework in the engine is a
    /// framework the engine has to keep up with; the shape is general and
    /// Tauri is its first instance. `t("checkout.title")` against a catalogue
    /// and a feature-flag key are the same question.
    CallMatchesExport {
        /// The callee whose argument names something, e.g. `invoke`.
        callee: String,
        /// Which argument holds the name. Zero-based.
        argument: usize,
        /// Where the declarations live.
        declared_in: Scope,
        /// The attribute a declaration carries to be one, e.g.
        /// `tauri::command`. `None` accepts any export in scope.
        attribute: Option<String>,
        /// Whether a declaration nobody calls is reported.
        ///
        /// Off by default: a command called only from a language archwarden
        /// does not read is not dead, and reporting it would be confident about
        /// something the run cannot see.
        report_uncalled: bool,
    },
    /// What a file exposes, said without reference to what it is called.
    ExportShape(ExportShape),
    /// A directory that has stopped growing.
    ///
    /// It carries no field of its own, and that is the design: every file
    /// under the scope is a finding, and which of them are *accepted* is
    /// `baseline`'s to say. The rule points the machinery that already records
    /// what a repository has accepted forward instead of back. Issue #102.
    Frozen,
    /// A counterpart in a parallel tree.
    Mirror {
        /// Regex over the filename, carrying the groups the template uses.
        file_pattern: Pattern,
        /// The counterpart's path, as a template rendered from repository
        /// root. See `MirrorRule::must_exist` for the groups it may name.
        must_exist: String,
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
            Self::Chokepoint { .. } => "chokepoint",
            Self::Presence { .. } => "presence",
            Self::Pair { .. } => "pair",
            Self::Frontmatter { .. } => "frontmatter",
            Self::Metadata { .. } => "metadata",
            Self::CallObligation { .. } => "call-obligation",
            Self::CallMatchesExport { .. } => "call-matches-export",
            Self::ExportShape(_) => "export-shape",
            Self::Frozen => "frozen",
            Self::Mirror { .. } => "mirror",
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
            // `Frozen` and `Mirror` join the first three: one asks whether a
            // path is in the scope, the other whether a path is on disk, and
            // neither opens a file.
            Self::Structure { .. }
            | Self::Presence { .. }
            | Self::Pair { .. }
            | Self::Frozen
            | Self::Mirror { .. }
            | Self::Frontmatter { .. } => false,
            Self::SpecPair {
                require_non_empty_spec,
                skip_type_only,
                ..
            } => *require_non_empty_spec || *skip_type_only,
            // `CallMatchesExport` joins them because both its sides are facts:
            // the calls in one file and the exports in another, and every file
            // in either scope has to be read for the two to be compared.
            Self::CallMatchesExport { .. }
            | Self::Naming { .. }
            | Self::ImportBoundary { .. }
            | Self::ImportCycle { .. }
            | Self::Chokepoint { .. }
            | Self::CallObligation { .. }
            | Self::NoPassthrough { .. }
            // Every one of its three claims is about the exports, which is
            // the one thing here that cannot be read off a directory listing.
            | Self::ExportShape(_)
            // The claims are in the comments, and the comments come out of
            // the same pass as everything else this front-end reads.
            | Self::Metadata { .. } => true,
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

/// Which files a rule narrows itself to by what they declare.
///
/// Cheaper than [`ImportFilter`] and answered from the same facts: a directive
/// is at the top of the file and needs no resolution. Issue #144.
#[derive(Debug, Clone)]
pub struct DirectiveFilter {
    /// Directives that put a file in the population.
    ///
    /// Any one of them is enough: a file declaring `"use client"` is a client
    /// component whatever else it says.
    pub declaring: Vec<String>,
    /// Directives that take a file out of it.
    ///
    /// Both directions exist because React needs both sentences. *"A client
    /// component may not import the database"* narrows by what a file
    /// declares; *"a server component may not call a hook"* narrows by what it
    /// does **not** -- a server component is spelled by the absence of
    /// `"use client"`, and there is no directive that says so.
    pub not_declaring: Vec<String>,
}

impl DirectiveFilter {
    /// Whether this file's directives put it in the population.
    ///
    /// Both halves must hold. A rule naming neither is not built at all --
    /// that is `None` on the rule, not an empty filter here.
    #[must_use]
    pub fn matches(&self, facts: &crate::facts::FileFacts) -> bool {
        let declares = |wanted: &String| facts.directives.contains(wanted);

        (self.declaring.is_empty() || self.declaring.iter().any(declares))
            && !self.not_declaring.iter().any(declares)
    }
}

/// Which files a rule narrows itself to by what they import.
///
/// Both halves are matched the way `import-boundary` already matches: paths
/// against where an import lands, packages against the package a specifier
/// belongs to. A file passes when **either** matches — they are two spellings
/// of "talks to this", not two conditions to satisfy at once.
#[derive(Debug, Clone)]
pub struct ImportFilter {
    /// Resolved import paths that put a file in the population.
    pub paths: crate::glob::PathSet,
    /// Package names that do the same.
    pub packages: Vec<String>,
}

impl ImportFilter {
    /// Whether this file's imports put it in the population.
    ///
    /// **An import that did not resolve cannot answer**, and this cannot tell
    /// one from an external package: both reach here with `resolved: None`,
    /// and only the resolver knows which was which. So the run reports them
    /// where it already does — `summary.imports.unresolved_imports`, naming
    /// the file and the specifier — which is the same answer a boundary rule
    /// gets for the same situation, and a better one than a count.
    ///
    /// It matters more here than there. A boundary rule with an unresolved
    /// import checked the others; a rule *narrowed* by one may not have
    /// applied at all. `RULES.md` says so beside the field.
    #[must_use]
    pub fn matches(&self, facts: &crate::facts::FileFacts) -> bool {
        facts.imports.iter().any(|import| {
            import
                .resolved
                .as_ref()
                .is_some_and(|resolved| self.paths.is_match(resolved.as_path()))
                || self
                    .packages
                    .iter()
                    .any(|package| package_of(&import.specifier) == *package)
        })
    }
}

/// The package a specifier belongs to, so `zod` covers `zod/v4`.
///
/// Scoped packages keep two segments: `@org/pkg/deep` is `@org/pkg`. The same
/// rule `import-boundary` applies, spelled here rather than reached for across
/// a crate boundary that points the wrong way.
#[must_use]
fn package_of(specifier: &str) -> String {
    let bare = specifier.strip_prefix("node:").unwrap_or(specifier);
    let segments: Vec<&str> = bare.split('/').collect();

    if bare.starts_with('@') {
        return segments
            .iter()
            .take(2)
            .copied()
            .collect::<Vec<_>>()
            .join("/");
    }
    segments
        .first()
        .map_or_else(String::new, |first| (*first).to_owned())
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
    /// The decision this rule implements, when it names one.
    ///
    /// The *reference*, not the prose — unlike [`why`](Self::why), which is
    /// copied onto every rule that carries it. One decision is served by many
    /// rules, so the prose lives once on the
    /// [`CompiledConfig`](CompiledConfig::decisions) and every surface looks
    /// it up. Copying it here would put the same paragraph on eight rules and
    /// give it eight places to be edited.
    ///
    /// Guaranteed to name a decision the config declares: a reference to
    /// nothing is refused at compile. Issue #100.
    pub decision: Option<DecisionId>,
    /// Which files this rule narrows itself to by what they import.
    ///
    /// A second axis beside [`scope`](Self::scope), and the difference is what
    /// each can see. A scope is lexical: where the file sits, what it is
    /// called, answerable before anything is read. This one is about where the
    /// file's imports *land*, which is knowable only after they are resolved —
    /// so a rule that carries one costs a resolution pass over the files its
    /// scope reaches, and a rule that does not costs nothing.
    ///
    /// `None` is "this rule does not ask", which is every rule written before
    /// 0.20 and most rules after it. An **empty** filter would be a different
    /// and much louder statement — "narrow me to the files that import
    /// nothing" — so the two must not be the same value. Decision 25.
    pub imports: Option<ImportFilter>,
    /// Narrows this rule to the files that declare a directive, or that do not.
    ///
    /// A third axis, and the cheapest of the three. A scope is lexical and
    /// costs nothing; [`imports`](Self::imports) needs an import to have been
    /// *resolved*, which is a pass over the files the scope reaches; this
    /// needs only the file parsed, which every rule asking for code facts
    /// already pays for.
    ///
    /// `None` is "this rule does not ask", on the same terms as `imports`.
    /// Issue #144.
    pub directives: Option<DirectiveFilter>,
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
    decisions: Vec<CompiledDecision>,
    ignore: PathSet,
    skip_dirs: SkipDirs,
    rules_hash: ContentHash,
    languages: Languages,
    /// What an ungoverned file reports as, or `None` for an open
    /// architecture. See [`CompiledConfig::governance`].
    governance: Option<Level>,
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

/// What a file exposes, with nothing said about what it is called.
///
/// Three claims in one kind because they are one question — *what does this
/// file expose?* — and a rule asking any of them wants the same `roots` and
/// the same `why`. Issue #101.
#[derive(Debug, Clone)]
pub struct ExportShape {
    /// Whether a default export is refused.
    pub forbid_default: bool,
    /// The most exports a file may have, counting only what exists at runtime.
    pub max_exports: Option<usize>,
    /// Return types an exported callable may declare.
    ///
    /// Empty when the rule does not ask. A list rather than one pattern,
    /// because an alias is the same type under a different string and a team
    /// that has aliases lists them — which leaves *"annotate with the
    /// canonical name"* available as a one-pattern rule, said out loud rather
    /// than implied.
    pub must_return: Vec<Pattern>,
}

/// A decision the architecture rests on, as the rest of the run sees it.
///
/// Prose, carried and never interpreted, exactly like a rule's `why` — with
/// one difference that decides where it lives: many rules serve one decision.
/// So the rules carry the *reference* and this carries the words, once.
///
/// Issue #100: a rule id in a denial is a thing to satisfy; a decision with a
/// link is a thing to understand or to argue with.
/// `PartialEq` and not `Eq`: a compiled `Scope` holds built `GlobSet`s, which
/// have no meaningful total equality. Nothing compares two decisions for
/// identity; the derive is here because the tests compare fields.
#[derive(Debug, Clone)]
pub struct CompiledDecision {
    /// The reference, such as `ADR-014`.
    pub id: DecisionId,
    /// What was decided, in one line. Always present.
    pub title: String,
    /// Why, when the author wrote it here rather than only behind `link`.
    pub why: Option<String>,
    /// Where it is written down. Carried verbatim, never resolved — archwarden
    /// does not check that a wiki page exists, and a linter that refused the
    /// reference would push people to omit it.
    pub link: Option<String>,
    /// Whether it still holds.
    ///
    /// Inferred rather than only read: a decision another one supersedes is
    /// superseded, and a config that wrote `accepted` there is refused where
    /// it compiles. Issue #115.
    pub status: DecisionStatus,
    /// The decisions this one replaced, in the order they were named.
    pub supersedes: Vec<DecisionId>,
    /// The decisions that replaced this one.
    ///
    /// Computed from everyone else's `supersedes`, never written: the new
    /// decision knows what it replaces, and the old one does not have to be
    /// edited to be replaced. Decision 26's foreign key, one level over.
    pub superseded_by: Vec<DecisionId>,
    /// What was considered and rejected, in the order it was written.
    ///
    /// The half of an ADR that stops the losing option being proposed again,
    /// and the half a rule can never carry: a rule says what is refused, and
    /// this says what was *weighed* and why it lost. Issue #114.
    pub alternatives: Vec<CompiledAlternative>,
    /// Why no rule can keep this, when the author claimed none can.
    ///
    /// `Some` is the claim and its argument together: the wire format refuses
    /// the claim without one, so a compiled decision carrying this is one
    /// somebody wrote a reason for. Issue #160.
    pub why_not_enforceable: Option<String>,
    /// Where it applies, compiled.
    ///
    /// Empty is a decision that says nothing about where — which is every
    /// decision written before #161 and every one whose author left it out.
    /// `describe` then finds it through the rules that name it, or not at all.
    pub scope: Option<Scope>,
}

/// One option a decision considered and did not take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledAlternative {
    /// The option, named as the team named it.
    pub option: String,
    /// Why it lost. Never empty: an option with no argument against it is a
    /// name nobody can disagree with, and the argument is the whole point.
    pub why_not: String,
    /// The rule that refuses this option today, when one does.
    ///
    /// A reference to a rule the author already wrote, never a rule generated
    /// from here. `baseline` keys on rule ids, and an id derived from this
    /// prose would orphan accepted debt the day somebody reworded the
    /// sentence. `None` means the option is written down and nothing stops
    /// anybody taking it — which is a true and useful thing for a page to say.
    pub refused_by: Option<RuleId>,
}

impl CompiledDecision {
    /// The option this rule is what refuses, when this decision named it.
    ///
    /// Scanned rather than indexed: a decision has a handful of alternatives
    /// and this is asked once per finding that already carries a decision.
    #[must_use]
    pub fn refusal_by(&self, rule: &RuleId) -> Option<&CompiledAlternative> {
        self.alternatives
            .iter()
            .find(|alternative| alternative.refused_by.as_ref() == Some(rule))
    }
}

/// Whether a decision still holds.
///
/// Mirrors the wire enum in `archwarden-config`, the way [`SkipScope`] does:
/// this crate owns the compiled result and cannot see the format it was
/// written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DecisionStatus {
    /// It holds. The default, and what a decision that says nothing means.
    #[default]
    Accepted,
    /// Written down, not yet settled. Reported by nothing — a decision under
    /// trial with rules already running is how one is trialled.
    Proposed,
    /// Replaced. Rules still enforcing it are a config saying two things at
    /// once, which `config doctor` reports as an error.
    Superseded,
}

impl DecisionStatus {
    /// The word a surface prints, which is the word the config wrote.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Proposed => "proposed",
            Self::Superseded => "superseded",
        }
    }

    /// Whether this is the default.
    #[must_use]
    pub fn is_accepted(self) -> bool {
        matches!(self, Self::Accepted)
    }

    /// Whether this decision has been replaced.
    ///
    /// The one status that is checked: a superseded decision whose rules still
    /// fire is a config saying two things at once. Issue #115 made it
    /// inferrable from an edge, so this is asked of far more decisions than it
    /// used to be.
    #[must_use]
    pub fn is_superseded(self) -> bool {
        matches!(self, Self::Superseded)
    }
}

impl std::fmt::Display for DecisionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
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

    /// Records that this configuration closes the architecture.
    ///
    /// A builder step like `with_languages`, so every test of a rule keeps the
    /// constructor it had.
    #[must_use]
    pub fn with_governance(mut self, level: Option<Level>) -> Self {
        self.governance = level;
        self
    }

    /// What an ungoverned file reports as, when this configuration reports one.
    ///
    /// `None` is an open architecture, which is every configuration that does
    /// not ask. Carrying the *level* rather than a flag is what keeps "does it
    /// report" and "how loudly" from being two questions that can disagree.
    #[must_use]
    pub fn governance(&self) -> Option<Level> {
        self.governance
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
            decisions: Vec::new(),
            ignore,
            skip_dirs,
            rules_hash,
            languages: Languages::default(),
            governance: None,
        }
    }

    /// Records the decisions the configuration declared.
    ///
    /// A builder step like `with_modules`, so every test of a rule keeps the
    /// constructor it had.
    #[must_use]
    pub fn with_decisions(mut self, decisions: Vec<CompiledDecision>) -> Self {
        self.decisions = decisions;
        self
    }

    /// Every decision, in declaration order.
    ///
    /// Carried whether or not any rule points at one: `config doctor` has to
    /// be able to see a decision nobody enforces, and the guide page lists
    /// what the architecture *decided*, not only what it checks.
    pub fn decisions(&self) -> impl Iterator<Item = &CompiledDecision> {
        self.decisions.iter()
    }

    /// The decision an id names.
    ///
    /// Every `CompiledRule::decision` resolves here, because a reference to a
    /// decision the config does not declare is refused at compile. A caller
    /// still gets an `Option` rather than a panic: this is also the lookup
    /// `config explain` makes with a string a user typed.
    #[must_use]
    pub fn decision(&self, id: &DecisionId) -> Option<&CompiledDecision> {
        self.decisions.iter().find(|decision| &decision.id == id)
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
            decision: None,
            imports: None,
            directives: None,
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
            ignore_files: crate::glob::PathSet::default(),
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
    /// The new kind names itself and says it must be read.
    ///
    /// Both halves of a `call-matches-export` are facts -- the calls in one
    /// file and the exports in another -- so every file in either scope is
    /// opened. Asserted because `needs_parse` is what keeps a structural
    /// configuration off the disk, and a kind answering it wrongly either
    /// reads a repository it did not have to or reports nothing at all.
    #[test]
    fn a_call_matching_rule_names_itself_and_has_to_be_read() {
        let kind = CompiledRuleKind::CallMatchesExport {
            callee: "invoke".to_owned(),
            argument: 0,
            declared_in: Scope::compile(["backend/**"]).expect("valid scope"),
            attribute: Some("tauri::command".to_owned()),
            report_uncalled: false,
        };

        assert_eq!(kind.type_name(), "call-matches-export");
        assert!(kind.needs_parse());
    }

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
        .with_languages(Languages {
            astro: true,
            rust: false,
        });
        assert!(asked.languages().astro);
        assert!(
            !asked.languages().rust,
            "a language nobody named stays off, whatever else was asked for"
        );
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
                with_options: Vec::new(),
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

    /// An open architecture is the default, and closing it carries the level
    /// rather than a flag.
    ///
    /// The level travels with the decision because "does it report" and "how
    /// loudly" are one question in the config and would be two here — and two
    /// that could disagree, which is how a gate ends up firing at a level
    /// nobody chose.
    #[test]
    fn an_architecture_is_open_until_a_configuration_closes_it() {
        let open = CompiledConfig::new(
            Vec::new(),
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b""),
        );
        assert_eq!(
            open.governance(),
            None,
            "every configuration written before the field, and every one that \
             does not ask"
        );

        for level in [Level::Error, Level::Warning] {
            assert_eq!(
                CompiledConfig::new(
                    Vec::new(),
                    PathSet::default(),
                    SkipDirs::default(),
                    ContentHash::of(b""),
                )
                .with_governance(Some(level))
                .governance(),
                Some(level)
            );
        }
    }
    fn decision(id: &str, status: DecisionStatus) -> CompiledDecision {
        CompiledDecision {
            scope: None,
            why_not_enforceable: None,
            id: DecisionId::new(id).expect("valid id"),
            title: "The domain does not know about transport".to_owned(),
            why: None,
            link: None,
            status,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            alternatives: Vec::new(),
        }
    }

    /// A configuration carries its decisions past compilation, whether or not
    /// any rule points at one: `config doctor` has to be able to see an
    /// orphan, and the guide page lists what the architecture decided rather
    /// than only what it checks.
    #[test]
    fn a_config_carries_its_decisions_in_declaration_order() {
        let config = CompiledConfig::new(
            Vec::new(),
            PathSet::default(),
            SkipDirs::default(),
            crate::hash::ContentHash::of(b""),
        )
        .with_decisions(vec![
            decision("ADR-014", DecisionStatus::Accepted),
            decision("ADR-007", DecisionStatus::Superseded),
        ]);

        let ids: Vec<&str> = config.decisions().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["ADR-014", "ADR-007"]);
    }

    /// The lookup every surface makes. It returns an `Option` rather than
    /// panicking even though a compiled rule's reference is guaranteed to
    /// resolve, because this is also how `config explain` looks up a string a
    /// user typed.
    #[test]
    fn a_decision_is_found_by_id_and_a_stranger_is_not() {
        let config = CompiledConfig::new(
            Vec::new(),
            PathSet::default(),
            SkipDirs::default(),
            crate::hash::ContentHash::of(b""),
        )
        .with_decisions(vec![decision("ADR-014", DecisionStatus::Accepted)]);

        assert_eq!(
            config
                .decision(&DecisionId::new("ADR-014").expect("valid"))
                .map(|d| d.title.as_str()),
            Some("The domain does not know about transport")
        );
        assert!(
            config
                .decision(&DecisionId::new("ADR-041").expect("valid"))
                .is_none()
        );
    }

    /// A configuration that declares none answers with nothing rather than
    /// with an error, which is every configuration written before 0.21.
    #[test]
    fn a_config_with_no_decisions_answers_none() {
        let config = CompiledConfig::new(
            Vec::new(),
            PathSet::default(),
            SkipDirs::default(),
            crate::hash::ContentHash::of(b""),
        );

        assert_eq!(config.decisions().count(), 0);
        assert!(
            config
                .decision(&DecisionId::new("ADR-014").expect("valid"))
                .is_none()
        );
    }

    /// The three words, which are the words the config wrote. They are stable
    /// identifiers in every JSON shape, so they are asserted rather than
    /// derived.
    #[test]
    fn a_status_prints_the_word_the_config_used() {
        assert_eq!(DecisionStatus::Accepted.as_str(), "accepted");
        assert_eq!(DecisionStatus::Proposed.as_str(), "proposed");
        assert_eq!(DecisionStatus::Superseded.as_str(), "superseded");

        // Through `Display`, which is what the terminal surfaces format with.
        assert_eq!(DecisionStatus::Superseded.to_string(), "superseded");
        assert_eq!(format!("{:>11}", DecisionStatus::Proposed), "   proposed");
    }

    /// Only one of the three is the default, and every surface asks this to
    /// decide whether to say the word out loud at all.
    #[test]
    fn only_accepted_is_the_default() {
        assert!(DecisionStatus::Accepted.is_accepted());
        assert!(!DecisionStatus::Proposed.is_accepted());
        assert!(!DecisionStatus::Superseded.is_accepted());
        assert_eq!(DecisionStatus::default(), DecisionStatus::Accepted);
    }

    /// A rule carries the reference, not the prose. The two travel separately
    /// because many rules serve one decision.
    #[test]
    fn a_rule_carries_the_reference_and_the_config_carries_the_words() {
        let mut governed = rule("shape", &["src/*"], structure());
        governed.decision = Some(DecisionId::new("ADR-014").expect("valid"));

        let config = CompiledConfig::new(
            vec![governed],
            PathSet::default(),
            SkipDirs::default(),
            crate::hash::ContentHash::of(b""),
        )
        .with_decisions(vec![decision("ADR-014", DecisionStatus::Accepted)]);

        let named = config
            .rules()
            .next()
            .expect("one rule")
            .decision
            .as_ref()
            .expect("it names one");
        assert_eq!(
            config.decision(named).map(|d| d.status),
            Some(DecisionStatus::Accepted)
        );
    }
    /// Issue #114. A rule refuses at most one rejected option, and the lookup
    /// is what puts the sentence *"this was already tried"* under a finding.
    #[test]
    fn a_decision_knows_which_option_a_rule_refuses() {
        let decision = CompiledDecision {
            scope: None,
            why_not_enforceable: None,
            id: DecisionId::new("ADR-031").expect("valid"),
            title: "the new way".to_owned(),
            why: None,
            link: None,
            status: DecisionStatus::Accepted,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            alternatives: vec![
                CompiledAlternative {
                    option: "an HTTP client in the domain".to_owned(),
                    why_not: "a consumer would inherit our transport".to_owned(),
                    refused_by: Some(RuleId::new("domain-forbids-http").expect("valid")),
                },
                CompiledAlternative {
                    option: "a shared kernel".to_owned(),
                    why_not: "it becomes the place everything goes".to_owned(),
                    refused_by: None,
                },
            ],
        };

        assert_eq!(
            decision
                .refusal_by(&RuleId::new("domain-forbids-http").expect("valid"))
                .map(|alternative| alternative.option.as_str()),
            Some("an HTTP client in the domain")
        );
        assert!(
            decision
                .refusal_by(&RuleId::new("some-other-rule").expect("valid"))
                .is_none(),
            "a rule that refuses no rejected option gets the block it always got"
        );
    }

    /// The one status that is checked, and the reason it is not decoration: a
    /// superseded decision whose rules still fire is a config saying two
    /// things at once.
    #[test]
    fn a_status_says_whether_it_was_replaced() {
        assert!(DecisionStatus::Superseded.is_superseded());
        assert!(!DecisionStatus::Accepted.is_superseded());
        assert!(!DecisionStatus::Proposed.is_superseded());
    }
}

#[cfg(test)]
mod import_filter_tests {
    use super::{DirectiveFilter, ImportFilter, package_of};
    use crate::facts::{FileFacts, ImportFact, Span};
    use crate::hash::ContentHash;
    use crate::path::RepoRelPath;

    fn importing(specifiers: &[(&str, Option<&str>)]) -> FileFacts {
        FileFacts {
            inline_tests: 0,
            path: RepoRelPath::new("src/a.ts").expect("valid"),
            content_hash: ContentHash::of(b"x"),
            exports: Vec::new(),
            calls: Vec::new(),
            reads: Vec::new(),
            callables: 0,
            directives: Vec::new(),
            renders: Vec::new(),
            imports: specifiers
                .iter()
                .map(|(specifier, resolved)| ImportFact {
                    specifier: (*specifier).to_owned(),
                    resolved: resolved.map(|path| RepoRelPath::new(path).expect("valid")),
                    type_only: false,
                    names: Vec::new(),
                    span: Span::new(0, 1),
                })
                .collect(),
            allowances: Vec::new(),
            metadata: Vec::new(),
            has_opaque_import: false,
        }
    }

    /// Issue #144. React Server Components draw the sharpest architectural
    /// boundary in the modern JavaScript ecosystem, and it is a directive
    /// rather than a path -- so a rule that could only narrow by where a file
    /// sits could not say either half of it.
    #[test]
    fn a_directive_filter_narrows_in_both_directions() {
        let declaring = |directives: &[&str]| {
            let mut facts = importing(&[]);
            facts.directives = directives.iter().map(|d| (*d).to_owned()).collect();
            facts
        };

        // A client component: what it declares puts it in.
        let client = DirectiveFilter {
            declaring: vec!["use client".to_owned()],
            not_declaring: Vec::new(),
        };
        assert!(client.matches(&declaring(&["use client"])));
        // Any one of them is enough, whatever else the file says.
        assert!(client.matches(&declaring(&["use strict", "use client"])));
        assert!(!client.matches(&declaring(&["use server"])));
        assert!(!client.matches(&declaring(&[])));

        // A server component is spelled by the *absence* of `use client`.
        // There is no directive that says so, which is why both directions
        // have to exist.
        let server = DirectiveFilter {
            declaring: Vec::new(),
            not_declaring: vec!["use client".to_owned()],
        };
        assert!(server.matches(&declaring(&[])));
        assert!(server.matches(&declaring(&["use strict"])));
        assert!(!server.matches(&declaring(&["use client"])));

        // Both halves together, and both must hold.
        let both = DirectiveFilter {
            declaring: vec!["use server".to_owned()],
            not_declaring: vec!["use client".to_owned()],
        };
        assert!(both.matches(&declaring(&["use server"])));
        assert!(!both.matches(&declaring(&["use server", "use client"])));
        assert!(!both.matches(&declaring(&["use strict"])));
    }

    fn filter(paths: &[&str], packages: &[&str]) -> ImportFilter {
        ImportFilter {
            paths: crate::glob::PathSet::compile(paths.iter().map(|p| (*p).to_owned()))
                .expect("valid globs"),
            packages: packages.iter().map(|p| (*p).to_owned()).collect(),
        }
    }

    /// The reported case: a file is in the population because of where one of
    /// its imports landed, not because of where it sits.
    #[test]
    fn a_file_is_in_when_an_import_lands_on_a_named_path() {
        let write = importing(&[("../http/connection", Some("src/http/connection.ts"))]);

        assert!(filter(&["src/http/**"], &[]).matches(&write));
        assert!(!filter(&["src/db/**"], &[]).matches(&write));
    }

    /// And a sibling that imports nothing is out, which is the half `roots`
    /// alone could never express.
    #[test]
    fn a_file_that_imports_nothing_named_is_out() {
        let read = importing(&[]);

        assert!(!filter(&["src/http/**"], &[]).matches(&read));
    }

    /// Packages are the other spelling of "talks to this", and either matching
    /// is enough — they are not two conditions to satisfy at once.
    #[test]
    fn either_half_puts_a_file_in() {
        let uses_zod = importing(&[("zod/v4", None)]);

        assert!(filter(&[], &["zod"]).matches(&uses_zod));
        assert!(
            filter(&["src/nowhere/**"], &["zod"]).matches(&uses_zod),
            "the package half alone is enough"
        );
    }

    /// An import nothing could place cannot put a file in. It cannot keep it
    /// out honestly either, which is why the run reports every unplaced
    /// specifier rather than this pretending to know. Decision 25.
    #[test]
    fn an_import_nobody_placed_does_not_match_a_path() {
        let unplaced = importing(&[("@Http/connection", None)]);

        assert!(!filter(&["src/http/**"], &[]).matches(&unplaced));
    }

    /// A package is what a specifier belongs to, so a deep import counts as
    /// the package it came from — the same rule a boundary applies.
    #[test]
    fn a_package_is_what_a_specifier_belongs_to() {
        assert_eq!(package_of("zod"), "zod");
        assert_eq!(package_of("zod/v4"), "zod");
        assert_eq!(package_of("@org/pkg"), "@org/pkg");
        assert_eq!(package_of("@org/pkg/deep/thing"), "@org/pkg");
        // `fs` is not part of `node:fs`; it is the same module spelled the
        // other way, which is how the boundary rules already read it.
        assert_eq!(package_of("node:fs"), "fs");
        assert_eq!(package_of(""), "");
    }
}
