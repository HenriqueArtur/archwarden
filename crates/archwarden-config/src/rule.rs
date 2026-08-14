//! The five rule shapes, as written in `arch.config.json`.
//!
//! These are *wire* types: a glob is a `String` here, and a regex is a
//! `String` too. Lowering them into the compiled types in `archwarden-core` is
//! a separate step, and it is what turns "this config might be valid" into
//! "this config is valid" — a compiled rule cannot exist unless every glob and
//! every regex in it parsed.
//!
//! See `docs/RULES.md` for semantics and `docs/CONFIG.md` for examples.

use archwarden_core::{
    ids::{DecisionId, ModuleId, RuleId},
    level::Level,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::one_or_many::OneOrMany;

/// A list of glob or regex patterns, written as a string or an array.
pub type Patterns = OneOrMany<String>;

/// One rule, discriminated by `type`.
///
/// `import-boundary` is an ordinary rule like the rest. There is no `graph`
/// key: boundaries go through the same matcher and the same
/// `describe_expectation`, which is what keeps `describe` and `agent-guide` in
/// lockstep with the checker. See decision 14.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[allow(
    clippy::large_enum_variant,
    reason = "456 bytes against 248 for the next largest, and the difference is \
              `import-boundary`, which has the most fields because it is the \
              rule with the most directions. This is a wire type: deserialised \
              once per run and lowered into `CompiledRule` immediately, held in \
              a Vec of a few dozen. A hundred-rule config is 45 KB. Boxing it \
              would buy that back and cost an indirection in every match and a \
              `Box::new` at every construction site, for memory nothing is \
              short of"
)]
pub enum Rule {
    /// Which folders may exist, and which filenames.
    Structure(StructureRule),
    /// The filename dictates the exported symbol's name.
    Naming(NamingRule),
    /// Every unit file needs a spec sibling.
    SpecPair(SpecPairRule),
    /// Layer A may not import from layer B.
    ImportBoundary(ImportBoundaryRule),
    /// No file in scope may sit on an import loop.
    ImportCycle(ImportCycleRule),
    /// Files matching a pattern must call a given symbol.
    CallObligation(CallObligationRule),
    /// A file whose whole content is forwarding another module.
    NoPassthrough(NoPassthroughRule),
    /// These files must exist in each governed directory.
    Presence(PresenceRule),
    /// A file of one kind must have a companion of another.
    Pair(PairRule),
    /// A document's frontmatter must carry these keys.
    Frontmatter(FrontmatterRule),
    /// What a file exposes, without saying anything about its name.
    ExportShape(ExportShapeRule),
}

impl Rule {
    /// This rule's identifier.
    #[must_use]
    pub fn id(&self) -> &RuleId {
        match self {
            Self::Structure(r) => &r.id,
            Self::Naming(r) => &r.id,
            Self::SpecPair(r) => &r.id,
            Self::ImportBoundary(r) => &r.id,
            Self::ImportCycle(r) => &r.id,
            Self::CallObligation(r) => &r.id,
            Self::NoPassthrough(r) => &r.id,
            Self::Presence(r) => &r.id,
            Self::Pair(r) => &r.id,
            Self::Frontmatter(r) => &r.id,
            Self::ExportShape(r) => &r.id,
        }
    }

    /// The severity of this rule's findings.
    #[must_use]
    pub fn level(&self) -> Level {
        match self {
            Self::Structure(r) => r.level,
            Self::Naming(r) => r.level,
            Self::SpecPair(r) => r.level,
            Self::ImportBoundary(r) => r.level,
            Self::ImportCycle(r) => r.level,
            Self::CallObligation(r) => r.level,
            Self::NoPassthrough(r) => r.level,
            Self::Presence(r) => r.level,
            Self::Pair(r) => r.level,
            Self::Frontmatter(r) => r.level,
            Self::ExportShape(r) => r.level,
        }
    }

    /// Why this rule exists, when its author said.
    #[must_use]
    pub fn why(&self) -> Option<&str> {
        match self {
            Self::Structure(r) => r.why.as_deref(),
            Self::Naming(r) => r.why.as_deref(),
            Self::SpecPair(r) => r.why.as_deref(),
            Self::ImportBoundary(r) => r.why.as_deref(),
            Self::ImportCycle(r) => r.why.as_deref(),
            Self::CallObligation(r) => r.why.as_deref(),
            Self::NoPassthrough(r) => r.why.as_deref(),
            Self::Presence(r) => r.why.as_deref(),
            Self::Pair(r) => r.why.as_deref(),
            Self::Frontmatter(r) => r.why.as_deref(),
            Self::ExportShape(r) => r.why.as_deref(),
        }
    }

    /// The decision this rule implements, when it names one.
    ///
    /// Every kind has the field, and that is why issue #100 shipped first of
    /// its milestone: a kind landing after it carries the field from birth,
    /// where four kinds landing before it would each have been a retrofit.
    #[must_use]
    pub fn decision(&self) -> Option<&DecisionId> {
        match self {
            Self::Structure(r) => r.decision.as_ref(),
            Self::Naming(r) => r.decision.as_ref(),
            Self::SpecPair(r) => r.decision.as_ref(),
            Self::ImportBoundary(r) => r.decision.as_ref(),
            Self::ImportCycle(r) => r.decision.as_ref(),
            Self::CallObligation(r) => r.decision.as_ref(),
            Self::NoPassthrough(r) => r.decision.as_ref(),
            Self::Presence(r) => r.decision.as_ref(),
            Self::Pair(r) => r.decision.as_ref(),
            Self::Frontmatter(r) => r.decision.as_ref(),
            Self::ExportShape(r) => r.decision.as_ref(),
        }
    }

    /// The rule's scope patterns.
    ///
    /// Named `roots` on four of the five and `from` on `import-boundary`,
    /// where it reads naturally against `forbid_import_from`. The semantics
    /// are identical, which is why they collapse to one accessor here.
    #[must_use]
    pub fn scope(&self) -> &Patterns {
        match self {
            Self::Structure(r) => &r.roots,
            Self::Naming(r) => &r.roots,
            Self::SpecPair(r) => &r.roots,
            Self::ImportBoundary(r) => &r.from,
            Self::ImportCycle(r) => &r.roots,
            Self::CallObligation(r) => &r.roots,
            Self::NoPassthrough(r) => &r.roots,
            Self::Presence(r) => &r.roots,
            Self::Pair(r) => &r.roots,
            Self::Frontmatter(r) => &r.roots,
            Self::ExportShape(r) => &r.roots,
        }
    }

    /// The import globs that narrow this rule's population, if any.
    ///
    /// Empty for every rule that does not ask, which is the ordinary case and
    /// the one that must stay free: a rule with nothing here never causes an
    /// import to be resolved. Decision 25.
    ///
    /// `import-boundary` has none and never will — it already chooses its
    /// importers with `from`, `from_module` and `from_kind`, and a second way
    /// to say the same thing is a second thing to get wrong.
    #[must_use]
    pub fn when_importing(&self) -> &Patterns {
        // A rule that never asks. A `const` rather than a `Default::default()`
        // so the borrow outlives the match without a field to hold it.
        const NONE: &Patterns = &OneOrMany::Many(Vec::new());
        match self {
            Self::Structure(r) => &r.when_importing,
            Self::Naming(r) => &r.when_importing,
            Self::SpecPair(r) => &r.when_importing,
            Self::ImportBoundary(_) => NONE,
            Self::ImportCycle(r) => &r.when_importing,
            Self::CallObligation(r) => &r.when_importing,
            Self::NoPassthrough(r) => &r.when_importing,
            Self::Presence(r) => &r.when_importing,
            Self::Pair(r) => &r.when_importing,
            Self::Frontmatter(r) => &r.when_importing,
            Self::ExportShape(r) => &r.when_importing,
        }
    }

    /// The package names that narrow this rule's population, if any.
    #[must_use]
    pub fn when_importing_packages(&self) -> &[String] {
        match self {
            Self::Structure(r) => &r.when_importing_packages,
            Self::Naming(r) => &r.when_importing_packages,
            Self::SpecPair(r) => &r.when_importing_packages,
            Self::ImportBoundary(_) => &[],
            Self::ImportCycle(r) => &r.when_importing_packages,
            Self::CallObligation(r) => &r.when_importing_packages,
            Self::NoPassthrough(r) => &r.when_importing_packages,
            Self::Presence(r) => &r.when_importing_packages,
            Self::Pair(r) => &r.when_importing_packages,
            Self::Frontmatter(r) => &r.when_importing_packages,
            Self::ExportShape(r) => &r.when_importing_packages,
        }
    }

    /// The discriminator, as written in the config.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Structure(_) => "structure",
            Self::Naming(_) => "naming",
            Self::SpecPair(_) => "spec-pair",
            Self::ImportBoundary(_) => "import-boundary",
            Self::ImportCycle(_) => "import-cycle",
            Self::CallObligation(_) => "call-obligation",
            Self::NoPassthrough(_) => "no-passthrough",
            Self::Presence(_) => "presence",
            Self::Pair(_) => "pair",
            Self::Frontmatter(_) => "frontmatter",
            Self::ExportShape(_) => "export-shape",
        }
    }
}

/// Which folders may exist under a scope, and which filenames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StructureRule {
    /// Stable identifier, unique across the config and its presets.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words.
    ///
    /// The config already says *what* a rule does, in a form that cannot drift
    /// from what it enforces; a prose restatement of that would be a second
    /// source of truth going stale. The reason cannot drift, because nothing
    /// else records it — decision 5 chose JSON, so there are no comments, and
    /// a commit message is not in front of anybody at the moment a rule fires.
    ///
    /// Shown by the pre-write hook when it denies a write, by `describe`,
    /// `scaffold`, `agent-guide` and `config explain`, and beside a finding.
    /// Not a message override: `observed` and `expected` remain the whole
    /// diagnosis, and a `why` restating them will contradict them the day the
    /// rule changes. Issue #46.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements, when it implements a declared one.
    ///
    /// A plain foreign key into [`Config::decisions`](crate::config::Config::decisions),
    /// written here rather than as a list of rule ids on the decision: this is
    /// where the author already is, there is no second list to keep in step, a
    /// deleted rule leaves nothing dangling, and a new rule that forgets its
    /// decision is visible in the one place it exists. Naming a decision the
    /// config does not declare is refused at compile — a reference to nothing
    /// is a typo, not a style. Issue #100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Subdirectory names that are permitted.
    ///
    /// An **option, not a list**, because absent and `[]` are different rules
    /// and a plain `Vec` cannot tell them apart. Absent means the rule says
    /// nothing about subfolders — it may still constrain filenames. `[]` is a
    /// list of what may exist holding nothing, so no subfolder is permitted,
    /// which is how "this directory is a leaf" is said. Issue #40, where the
    /// empty list validated, ran, and enforced nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_subfolders: Option<Vec<String>>,
    /// Subdirectory names that are permitted but reported as warnings,
    /// whatever `level` says. Naming a folder is more specific than the rule's
    /// blanket severity, and the more specific declaration wins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warn_subfolders: Vec<String>,
    /// Containers whose *children* carry this rule's contract, recursively.
    ///
    /// The container itself is not governed and its children's names are not
    /// checked — they are modules in their own right, and a module's name is
    /// no more constrained here than one selected by `roots`. Given
    /// `recurse_into: ["variants"]`, the governed directory is
    /// `user/variants/nfe`, and `nfe` may be called anything.
    ///
    /// This description used to read "subdirectories that carry the same
    /// structural contract, recursively", which one reader took to mean the
    /// contract applies *inside* the named folder. Adding it to a namespace
    /// holding nineteen modules cleared nineteen findings and read as
    /// modelling; what it did was promote those nineteen directories from
    /// "unexpected subfolder" to "module", which is a real decision and was
    /// not the one they thought they were making. Issue #29.
    ///
    /// `config explain <rule-id>` lists every directory a rule governs, which
    /// is the answer to "did this mean what I think" for exactly this field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recurse_into: Vec<String>,
    /// Regexes a direct child *directory*'s name may match instead of being
    /// named in `allowed_subfolders`.
    ///
    /// `filename_patterns` one entry over, for the other kind of directory
    /// entry. `allowed_subfolders` constrains names by enumeration, which works
    /// for a fixed vocabulary (`types`, `calcs`, `actions`) and cannot work for
    /// an open set where the *shape* is the rule — sixteen lesson folders named
    /// `NN-slug` and more arriving. Issue #43.
    ///
    /// A union with the two lists, the way `filename_patterns` is a union of
    /// its own regexes: a name is permitted if a list names it *or* a pattern
    /// matches it. The lists are consulted first, so a `warn_subfolders` entry
    /// whose name happens to have the right shape still warns — the most
    /// specific declaration wins, and a name written out is more specific than
    /// a regex.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subfolder_patterns: Vec<String>,
    /// Regexes every direct child file's name must match at least one of.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filename_patterns: Vec<String>,
    /// Narrow this rule to the files that import something.
    ///
    /// Path globs, matched against where an import *lands* — the same way an
    /// `import-boundary` matches. Without this a rule's population is where a
    /// file sits and what it is called; with it, what the file talks to.
    ///
    /// Leave it out and nothing changes, including the cost: a rule that does
    /// not ask never resolves an import. Issue #98, decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    ///
    /// Matched against the package a specifier belongs to, so `zod` covers
    /// `zod/v4` as it does everywhere else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// Files that must exist in each governed directory.
///
/// `structure.filename_patterns` is a whitelist of what *may* exist, and the
/// two are not each other's inverse — a `filename_patterns` rule is satisfied
/// by an empty directory, which is exactly the state this one is about. A unit
/// of work is incomplete until its companion files are there, and the
/// companion is what a hurried pass leaves out. Issue #42.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresenceRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words. See [`StructureRule::why`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements, when it implements a declared one.
    ///
    /// A plain foreign key into [`Config::decisions`](crate::config::Config::decisions),
    /// written here rather than as a list of rule ids on the decision: this is
    /// where the author already is, there is no second list to keep in step, a
    /// deleted rule leaves nothing dangling, and a new rule that forgets its
    /// decision is visible in the one place it exists. Naming a decision the
    /// config does not declare is refused at compile — a reference to nothing
    /// is a typo, not a style. Issue #100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Filenames that must exist directly inside each governed directory.
    ///
    /// **Names, not paths.** An entry with a `/` is refused when the config
    /// compiles, and the message says what to write instead: a second rule
    /// scoped one level down. `roots: ["projetos/*/sketch"]` with
    /// `require: ["sketch.ino"]` is the same requirement, said by the rule that
    /// is about that directory — and it keeps one rule answering for one
    /// directory's contract, which is what makes `describe` able to answer for
    /// a directory that does not exist yet.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub require: Patterns,
    /// Regexes at least one file in each governed directory must match.
    ///
    /// For "there has to be a sketch and I do not care what it is called".
    /// One entry, one requirement: two regexes mean two files must be found,
    /// one for each.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub require_any: Patterns,
    /// Narrow this rule to the files that import something.
    ///
    /// Path globs, matched against where an import *lands* — the same way an
    /// `import-boundary` matches. Without this a rule's population is where a
    /// file sits and what it is called; with it, what the file talks to.
    ///
    /// Leave it out and nothing changes, including the cost: a rule that does
    /// not ask never resolves an import. Issue #98, decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    ///
    /// Matched against the package a specifier belongs to, so `zod` covers
    /// `zod/v4` as it does everywhere else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// A file of one kind must have a companion of another.
///
/// `spec-pair` is this rule for one specific pair, and cannot be bent to any
/// other: its default ignores exclude anything that is not a JS/TS source file,
/// and its companion is *derived* — `<stem>.<marker>.<ext>` — which is a good
/// convention for tests and generalises to nothing. Two fixed names in one
/// directory is the common case everywhere else. Issue #45.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PairRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words. See [`StructureRule::why`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements, when it implements a declared one.
    ///
    /// A plain foreign key into [`Config::decisions`](crate::config::Config::decisions),
    /// written here rather than as a list of rule ids on the decision: this is
    /// where the author already is, there is no second list to keep in step, a
    /// deleted rule leaves nothing dangling, and a new rule that forgets its
    /// decision is visible in the one place it exists. Naming a decision the
    /// config does not declare is refused at compile — a reference to nothing
    /// is a typo, not a style. Issue #100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Regex over the filename of the file that *needs* a companion.
    pub file_pattern: String,
    /// The companion, as a path relative to the directory the file sits in.
    ///
    /// **Literal, never derived.** `<stem>.<marker>.<ext>` is `spec-pair`'s
    /// idea and does not generalise; two fixed names in one directory is what
    /// the rest of the world has.
    ///
    /// **May leave the directory.** `../projeto.md` is the case this rule
    /// exists for alongside the flat one — a sketch needs the lesson one level
    /// up, and no directory-scoped rule can say that. `presence` refuses paths
    /// for exactly the opposite reason: it answers for a directory, and this
    /// one answers for a file, which is what gives it an anchor to be relative
    /// to.
    ///
    /// **One direction, always.** This rule says the file matching
    /// `file_pattern` needs the companion, and never the reverse — an orphan
    /// `notas.md` is a note taken before the lesson was written, which is fine.
    pub must_exist: String,
    /// Narrow this rule to the files that import something.
    ///
    /// Path globs, matched against where an import *lands* — the same way an
    /// `import-boundary` matches. Without this a rule's population is where a
    /// file sits and what it is called; with it, what the file talks to.
    ///
    /// Leave it out and nothing changes, including the cost: a rule that does
    /// not ask never resolves an import. Issue #98, decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    ///
    /// Matched against the package a specifier belongs to, so `zod` covers
    /// `zod/v4` as it does everywhere else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// A document's frontmatter must carry these keys.
///
/// The first rule that reads a file that is not code. The frontmatter of a
/// `.md` is often not documentation at all — it is the machine-readable half
/// of the document, and a missing or misspelled key fails *silently*: the
/// project with no `componentes` reports as needing none, and the lesson whose
/// `status` is outside the vocabulary drops out of the generated table with no
/// row and no error. Nothing type-checks a markdown file. Issue #44.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrontmatterRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words. See [`StructureRule::why`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements, when it implements a declared one.
    ///
    /// A plain foreign key into [`Config::decisions`](crate::config::Config::decisions),
    /// written here rather than as a list of rule ids on the decision: this is
    /// where the author already is, there is no second list to keep in step, a
    /// deleted rule leaves nothing dangling, and a new rule that forgets its
    /// decision is visible in the one place it exists. Naming a decision the
    /// config does not declare is refused at compile — a reference to nothing
    /// is a typo, not a style. Issue #100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Regex over the filename of the documents this rule is about.
    pub file_pattern: String,
    /// Keys the block must carry.
    ///
    /// Ninety per cent of the value, and the whole of it that is about *names*.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub require: Patterns,
    /// The closed vocabulary a key's value must come from.
    ///
    /// The case that justifies the rule existing. A missing key is at least an
    /// absence; a value outside the vocabulary is *confidently wrong* — the
    /// generated table simply has no row for it — which is the same failure
    /// shape `must_export.annotation` exists for.
    ///
    /// Values are compared as text, so `"1"` here matches `nivel: 1` in the
    /// document. That is deliberate: it answers the question without archwarden
    /// growing a type system nothing else here needs.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub one_of: std::collections::BTreeMap<String, Patterns>,
    /// A key whose value must equal a template rendered from the path.
    ///
    /// `{{raw(dirname)}}` is the name of the directory the document sits in,
    /// and it is the only group a document template may name. The form is the
    /// one `naming` already uses, so the transforms come along:
    /// `{{kebab(dirname)}}` is spelled the same way here as there.
    ///
    /// This is the `naming` rule's question — a name agreeing with a path —
    /// asked of a file that has no exported symbol to ask it about.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub equals: std::collections::BTreeMap<String, String>,
    /// Narrow this rule to the files that import something.
    ///
    /// Path globs, matched against where an import *lands* — the same way an
    /// `import-boundary` matches. Without this a rule's population is where a
    /// file sits and what it is called; with it, what the file talks to.
    ///
    /// Leave it out and nothing changes, including the cost: a rule that does
    /// not ask never resolves an import. Issue #98, decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    ///
    /// Matched against the package a specifier belongs to, so `zod` covers
    /// `zod/v4` as it does everywhere else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// The filename dictates the exported symbol's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NamingRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words.
    ///
    /// The config already says *what* a rule does, in a form that cannot drift
    /// from what it enforces; a prose restatement of that would be a second
    /// source of truth going stale. The reason cannot drift, because nothing
    /// else records it — decision 5 chose JSON, so there are no comments, and
    /// a commit message is not in front of anybody at the moment a rule fires.
    ///
    /// Shown by the pre-write hook when it denies a write, by `describe`,
    /// `scaffold`, `agent-guide` and `config explain`, and beside a finding.
    /// Not a message override: `observed` and `expected` remain the whole
    /// diagnosis, and a `why` restating them will contradict them the day the
    /// rule changes. Issue #46.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements, when it implements a declared one.
    ///
    /// A plain foreign key into [`Config::decisions`](crate::config::Config::decisions),
    /// written here rather than as a list of rule ids on the decision: this is
    /// where the author already is, there is no second list to keep in step, a
    /// deleted rule leaves nothing dangling, and a new rule that forgets its
    /// decision is visible in the one place it exists. Naming a decision the
    /// config does not declare is refused at compile — a reference to nothing
    /// is a typo, not a style. Issue #100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Regex over the filename, with a named capture group.
    pub file_pattern: String,
    /// Regex over the name of the directory the file sits in, with named
    /// capture groups that join `file_pattern`'s in the template.
    ///
    /// For the convention where the entity is the folder and the action is the
    /// file — `Order/fetch-by-id.ts` exporting `OrderFetchByIdRepository` — the
    /// export name is spelled from both halves of the path, and `file_pattern`
    /// alone can only see one of them.
    ///
    /// Matched against the *last segment* of the directory, not the whole path:
    /// under `roots: ".../Entities/*"` the file `.../Entities/Order/insert.ts`
    /// offers `Order`. When set, it must match, exactly as `file_pattern` must
    /// — a file whose directory does not match is a file the rule is not about.
    ///
    /// Stays purely lexical: `dirname` and `basename` of a path archwarden
    /// already has, with no parse, no resolution and no disk access, so
    /// `describe` and `scaffold` keep answering for files that do not exist yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir_pattern: Option<String>,
    /// The export the file must carry.
    pub must_export: MustExport,
    /// Narrow this rule to the files that import something.
    ///
    /// Path globs, matched against where an import *lands* — the same way an
    /// `import-boundary` matches. Without this a rule's population is where a
    /// file sits and what it is called; with it, what the file talks to.
    ///
    /// Leave it out and nothing changes, including the cost: a rule that does
    /// not ask never resolves an import. Issue #98, decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    ///
    /// Matched against the package a specifier belongs to, so `zod` covers
    /// `zod/v4` as it does everywhere else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// The export a `naming` rule requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MustExport {
    /// Declaration forms that satisfy the rule: one name, a list of names, or
    /// `"any"`. See the table in `docs/RULES.md`.
    pub kind: Patterns,
    /// The required name, as a template over `file_pattern`'s capture groups.
    pub name: String,
    /// The type the export must be annotated with, as a template over the same
    /// groups. One value, or several meaning "any of".
    ///
    /// **Checked**, and the one field here that is — which is why it is not
    /// spelled into `signature_hint`. That field is documented as a suggestion
    /// `scaffold` renders and `check` ignores, and code depends on that; a
    /// separate field keeps the promise of each legible.
    ///
    /// Still not type checking. Nothing is resolved and nothing is inferred:
    /// the annotation is a token in the same declaration whose `kind` this rule
    /// already reads, and comparing it is the same class of work as comparing
    /// the name. A file annotating `AgentToolModule` over an object that is not
    /// one is `tsc`'s problem and stays that way. What this buys is that the
    /// declaration is *submitted to* `tsc`'s judgement at all — the guarantee a
    /// registry loses when it moves from a typed array to `readdir` and
    /// `import()`. Issue #39.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotation: Option<Patterns>,
    /// A signature shown by `scaffold`. **Never verified** — constraining the
    /// type of an export is type checking, which is `tsc`'s job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hint: Option<String>,
}

/// Every unit file under the scope needs a spec sibling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpecPairRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words.
    ///
    /// The config already says *what* a rule does, in a form that cannot drift
    /// from what it enforces; a prose restatement of that would be a second
    /// source of truth going stale. The reason cannot drift, because nothing
    /// else records it — decision 5 chose JSON, so there are no comments, and
    /// a commit message is not in front of anybody at the moment a rule fires.
    ///
    /// Shown by the pre-write hook when it denies a write, by `describe`,
    /// `scaffold`, `agent-guide` and `config explain`, and beside a finding.
    /// Not a message override: `observed` and `expected` remain the whole
    /// diagnosis, and a `why` restating them will contradict them the day the
    /// rule changes. Issue #46.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements, when it implements a declared one.
    ///
    /// A plain foreign key into [`Config::decisions`](crate::config::Config::decisions),
    /// written here rather than as a list of rule ids on the decision: this is
    /// where the author already is, there is no second list to keep in step, a
    /// deleted rule leaves nothing dangling, and a new rule that forgets its
    /// decision is visible in the one place it exists. Naming a decision the
    /// config does not declare is refused at compile — a reference to nothing
    /// is a typo, not a style. Issue #100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Subdirectories subject to the rule, each covering everything below it.
    ///
    /// An entry names a directory relative to a `roots`-selected one, so
    /// `calcs` covers `Entity/calcs/group/nested.ts` as well as
    /// `Entity/calcs/direct.ts`, and a nested entry like `calcs/group` names
    /// that subtree exactly.
    ///
    /// `["."]` means the scope directory itself and only its own files —
    /// deliberately not recursive, since naming `calcs` is how a project says
    /// which subtree is under the gate, and a recursive `.` would swallow
    /// `types` and everything else it did not name.
    ///
    /// Entries used to be compared against a single directory *name*, so only
    /// a direct child was covered and a nested path matched nothing while
    /// validating cleanly. Grouping related files into a folder took them out
    /// of the gate in silence — eleven validation functions in one repository
    /// had no test at all and had never appeared in a report. Issue #34.
    pub subfolders: Patterns,
    /// What makes a filename a spec: `spec`, `test`, or both.
    ///
    /// A marker, not a whole suffix: the extension is taken from the source
    /// file, so `Component.tsx` wants `Component.spec.tsx` without anyone
    /// saying so. The default accepts both markers, which is what vitest and
    /// jest do, so the common project needs no configuration here at all.
    #[serde(default = "default_spec_markers")]
    pub spec_markers: Patterns,
    /// Directories, beside the file, where a spec also counts.
    ///
    /// Empty by default, which is sibling-only — what every config written
    /// before this had, and what a project that says nothing keeps.
    ///
    /// A name, not a glob: `__tests__`, `tests`, `__specs__`, whatever the
    /// project uses. A spec at `<dir>/<named>/x.spec.ts` satisfies `<dir>/x.ts`
    /// and reaches exactly one level — `__tests__/unit/x.spec.ts` does not
    /// count unless `unit` is named too.
    ///
    /// The depth limit is the feature. A reading that accepted a spec anywhere
    /// below would make the rule report nothing and look exactly like a
    /// repository that is fully tested, which is the failure `CONFIG.md` calls
    /// the worst a linter has. Issue #67.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub spec_dirs: Patterns,
    /// Globs exempted from the rule.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub ignore_files: Patterns,
    /// Whether the spec must contain at least one `it(...)` or `test(...)`.
    /// A bare `describe(...)` does not count: an empty describe block
    /// satisfies the letter of the rule while defeating its purpose.
    #[serde(default)]
    pub require_non_empty_spec: bool,
    /// Whether a file whose exports are all `type` or `interface` is exempt.
    ///
    /// A file with no runtime export has nothing a test could call. Demanding
    /// a spec for one produces work that reduces no risk — and the spec that
    /// gets written to satisfy the rule tests a mock of the contract rather
    /// than the contract, because there is nothing else to test. `tsc` is the
    /// tool that checks an interface, and it checks it on every build.
    ///
    /// `enum` is a runtime export and does not count as type-only. A file with
    /// no exports at all does not either: that is a file with no callers, not
    /// a contract, and the rule has something to say about it.
    ///
    /// Costs a parse. `spec-pair` otherwise reads no file, so a rule that sets
    /// this reads every file in its scope — the same trade
    /// `require_non_empty_spec` makes.
    #[serde(default)]
    pub skip_type_only: bool,
    /// Narrow this rule to the files that import something.
    ///
    /// Path globs, matched against where an import *lands* — the same way an
    /// `import-boundary` matches. Without this a rule's population is where a
    /// file sits and what it is called; with it, what the file talks to.
    ///
    /// Leave it out and nothing changes, including the cost: a rule that does
    /// not ask never resolves an import. Issue #98, decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    ///
    /// Matched against the package a specifier belongs to, so `zod` covers
    /// `zod/v4` as it does everywhere else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

fn default_spec_markers() -> Patterns {
    Patterns::Many(vec!["spec".to_owned(), "test".to_owned()])
}

/// Layer A may not import from layer B, or must import from layer C.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportBoundaryRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words.
    ///
    /// The config already says *what* a rule does, in a form that cannot drift
    /// from what it enforces; a prose restatement of that would be a second
    /// source of truth going stale. The reason cannot drift, because nothing
    /// else records it — decision 5 chose JSON, so there are no comments, and
    /// a commit message is not in front of anybody at the moment a rule fires.
    ///
    /// Shown by the pre-write hook when it denies a write, by `describe`,
    /// `scaffold`, `agent-guide` and `config explain`, and beside a finding.
    /// Not a message override: `observed` and `expected` remain the whole
    /// diagnosis, and a `why` restating them will contradict them the day the
    /// rule changes. Issue #46.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements, when it implements a declared one.
    ///
    /// A plain foreign key into [`Config::decisions`](crate::config::Config::decisions),
    /// written here rather than as a list of rule ids on the decision: this is
    /// where the author already is, there is no second list to keep in step, a
    /// deleted rule leaves nothing dangling, and a new rule that forgets its
    /// decision is visible in the one place it exists. Naming a decision the
    /// config does not declare is refused at compile — a reference to nothing
    /// is a typo, not a style. Issue #100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs selecting the importer. Same semantics as `roots`.
    ///
    /// Exactly one of this and [`from_module`](Self::from_module) is required.
    /// Saying it both ways is refused when the config compiles: two spellings
    /// of one scope on one rule is the ambiguity that produces a rule
    /// enforcing something nobody meant.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub from: Patterns,
    /// The module the importers are, instead of the globs they match.
    ///
    /// A module that declared a `scope` has paths already; naming it here
    /// stops a boundary from re-describing them. Move the package and one
    /// place changes instead of two, and nothing silently stops reaching.
    /// Issue #74.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_module: Option<ModuleId>,
    /// Globs matched against the *resolved* import path. Matching means the
    /// import is illegal.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub forbid_import_from: Patterns,
    /// Globs matched against the *resolved* import path. Anything not matching
    /// is illegal.
    ///
    /// The allowlist direction. A denylist decays: every new package, app or
    /// directory is permitted by omission, and omission is invisible. This one
    /// refuses things that do not exist yet, which is the whole point.
    ///
    /// **Governs edges inside this repository only.** A builtin, a dependency
    /// and an import nothing could resolve have no repo-relative path a glob
    /// could match; `only_import_from_packages` is the field for those, for the
    /// same reason `forbid_import_from_packages` is separate from
    /// `forbid_import_from`. And a file importing its own neighbour is always
    /// permitted: an import resolving inside the rule's own `from` is not
    /// something "only these" was ever meant to refuse. Issue #75.
    ///
    /// Refused alongside `forbid_import_from` on one rule: "only these, except
    /// those" is expressible as two rules and clearer as two.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub only_import_from: Patterns,
    /// Modules whose files may be imported, and no others.
    ///
    /// `only_import_from` with the paths written for you, the way
    /// `forbid_module` is for `forbid_import_from`.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub only_import_from_modules: OneOrMany<ModuleId>,
    /// The sort of module the importers are, instead of naming each one.
    ///
    /// `from_kind: "app"` selects every module that said `kind: "app"`, so the
    /// seventh assembly is governed because it exists rather than because
    /// somebody remembered to add it. Issue #76.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_kind: Option<String>,
    /// The sorts of module those importers may import from, and no others.
    ///
    /// An allowlist rather than `forbid_kind`, deliberately: a `kind` invented
    /// later is refused rather than permitted by omission, which is the same
    /// argument [`only_import_from`](Self::only_import_from) rests on.
    ///
    /// A module never fails this against itself. `from_kind: "app"` permitting
    /// only `lib` must not stop an app importing its own files — identity
    /// decides that, not the label.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub only_import_from_kinds: OneOrMany<String>,
    /// Package names this file may import, and no others.
    ///
    /// The package axis of `only_import_from`. Absent means packages are not
    /// governed by this rule at all, which is what keeps `only_import_from`
    /// from tripping on every dependency in the manifest.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub only_import_from_packages: Patterns,
    /// Modules whose files may not be imported, instead of the globs they are.
    ///
    /// Folded into `forbid_import_from` when the config compiles, so nothing
    /// downstream knows the difference — but the config says `infrastructure`
    /// where it used to repeat that module's paths. Issue #74.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub forbid_module: OneOrMany<ModuleId>,
    /// Globs matched against the resolved import path. If none of the file's
    /// imports match, the file is illegal.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub must_import_from: Patterns,
    /// Package names this file may not import. Matching means the import is
    /// illegal.
    ///
    /// A package name, not a glob: `"three"` forbids `three` and everything
    /// under it, so `three/examples/jsm/loaders/GLTFLoader.js` does not sail
    /// past. `node:fs` and `fs` are the same module and either spelling matches
    /// both.
    ///
    /// A separate field rather than a scheme prefix inside `forbid_import_from`
    /// on purpose: treating `three` as *either* a path glob or a package name
    /// depending on what it happened to match is the ambiguity that produces a
    /// rule enforcing nothing.
    ///
    /// An import that resolves to a file in this repository is never matched
    /// here, however it is spelled — that is a path, and `forbid_import_from`
    /// is the field for it.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub forbid_import_from_packages: Patterns,
    /// Globs matched against every file this one ends up depending on,
    /// however many imports away. Matching means the dependency is illegal.
    ///
    /// The rule `forbid_import_from` cannot express: `packages/ui` does not
    /// import `packages/db`, and it depends on it through `packages/orders`
    /// anyway. A *direct* import is not reported here — that is
    /// `forbid_import_from`'s finding, and reporting it twice would make one
    /// fault look like two.
    ///
    /// **This field is the expensive one.** A rule that sets it makes the run
    /// parse and resolve every source file in the repository, whatever any
    /// scope says, because a chain that leaves the scope and comes back is
    /// still a chain. Measured on a 10,000-file repository, that is about
    /// twenty times the wall clock of the same rule without it. A boundary
    /// rule that leaves it empty pays none of that. Issue #71.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub forbid_reaching: Patterns,
    /// Modules that may not be reached, instead of the globs they are.
    ///
    /// What `forbid_module` is to `forbid_import_from`. Folded into
    /// `forbid_reaching` when the config compiles, and refused alongside it on
    /// one rule for the same reason: two ways to fill one set is a rule whose
    /// author has to be told which one won.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub forbid_reaching_modules: OneOrMany<ModuleId>,
    /// Exceptions, also matched against the resolved path.
    ///
    /// Applies to `forbid_import_from` and to `forbid_reaching` alike: both
    /// name destinations, and "may not reach `packages/db`, except
    /// `packages/db/types`" is one sentence rather than two fields.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub except: Patterns,
    /// Globs matched against the *importing* file, exempting it from the whole
    /// rule.
    ///
    /// `except` is about what is imported; this is about who imports it, which
    /// is where an exception to a rule about a dependency naturally sits —
    /// "only `src/scripts/three/**` may import `three`" is one forbid and one
    /// exempt importer.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub except_from: Patterns,
    /// Whether `import type` and inline `type` marks count.
    #[serde(default = "default_true")]
    pub include_type_only: bool,
}

fn default_true() -> bool {
    true
}

/// Files matching a pattern must call a given symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CallObligationRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words.
    ///
    /// The config already says *what* a rule does, in a form that cannot drift
    /// from what it enforces; a prose restatement of that would be a second
    /// source of truth going stale. The reason cannot drift, because nothing
    /// else records it — decision 5 chose JSON, so there are no comments, and
    /// a commit message is not in front of anybody at the moment a rule fires.
    ///
    /// Shown by the pre-write hook when it denies a write, by `describe`,
    /// `scaffold`, `agent-guide` and `config explain`, and beside a finding.
    /// Not a message override: `observed` and `expected` remain the whole
    /// diagnosis, and a `why` restating them will contradict them the day the
    /// rule changes. Issue #46.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements, when it implements a declared one.
    ///
    /// A plain foreign key into [`Config::decisions`](crate::config::Config::decisions),
    /// written here rather than as a list of rule ids on the decision: this is
    /// where the author already is, there is no second list to keep in step, a
    /// deleted rule leaves nothing dangling, and a new rule that forgets its
    /// decision is visible in the one place it exists. Naming a decision the
    /// config does not declare is refused at compile — a reference to nothing
    /// is a typo, not a style. Issue #100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Regex over the filename.
    pub file_pattern: String,
    /// The call the file must contain.
    pub must_call: MustCall,
    /// Narrow this rule to the files that import something.
    ///
    /// Path globs, matched against where an import *lands* — the same way an
    /// `import-boundary` matches. Without this a rule's population is where a
    /// file sits and what it is called; with it, what the file talks to.
    ///
    /// Leave it out and nothing changes, including the cost: a rule that does
    /// not ask never resolves an import. Issue #98, decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    ///
    /// Matched against the package a specifier belongs to, so `zod` covers
    /// `zod/v4` as it does everywhere else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// The call a `call-obligation` rule requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MustCall {
    /// The callee as it appears at a call site, e.g. `Event.save`. Method
    /// chains are matched exactly.
    pub symbol: String,
    /// The module the symbol must be imported from, which disambiguates
    /// same-named functions from different packages.
    pub imported_from: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A misspelled field is refused rather than ignored.
    ///
    /// Found the hard way in M7d: a config saying `allow` where the field is
    /// `allowed_subfolders` compiled to a `structure` rule that constrained
    /// nothing, `config validate` called it valid, and `check` reported a
    /// clean repository. A rule that silently enforces nothing is the worst
    /// possible failure for a linter -- it is indistinguishable from a rule
    /// that passes.
    #[test]
    fn a_misspelled_field_is_refused() {
        let error = serde_json::from_str::<Rule>(
            r#"{"type":"structure","id":"shape","level":"error",
                "roots":"src/*","allow":["types"]}"#,
        )
        .expect_err("`allow` is not a field");

        assert!(error.to_string().contains("allow"), "{error}");
    }

    /// Every rule kind refuses one, not just the first that happened to be
    /// tested. A gap here is a rule kind that can be silently disabled.
    #[test]
    fn every_rule_kind_refuses_an_unknown_field() {
        let cases = [
            r#"{"type":"structure","id":"a","level":"error","roots":"src/*","nope":1}"#,
            r#"{"type":"naming","id":"a","level":"error","roots":"src/*",
                "file_pattern":"^x$","must_export":{"name":"X","kind":"any"},"nope":1}"#,
            r#"{"type":"spec-pair","id":"a","level":"error","roots":"src/*",
                "subfolders":".","spec_markers":"spec","nope":1}"#,
            r#"{"type":"import-boundary","id":"a","level":"error","from":"src/**","nope":1}"#,
            r#"{"type":"call-obligation","id":"a","level":"error","roots":"src/*",
                "file_pattern":"^x$","must_call":{"symbol":"S","imported_from":"m"},"nope":1}"#,
        ];

        for case in cases {
            assert!(
                serde_json::from_str::<Rule>(case).is_err(),
                "accepted an unknown field: {case}"
            );
        }
    }

    /// The nested objects too, which is where a typo is easiest to make and
    /// hardest to notice.
    #[test]
    fn a_nested_object_refuses_an_unknown_field() {
        assert!(
            serde_json::from_str::<Rule>(
                r#"{"type":"naming","id":"a","level":"error","roots":"src/*",
                    "file_pattern":"^x$","must_export":{"name":"X","kind":"any","hint":"..."}}"#
            )
            .is_err(),
            "`hint` is not a field; `signature_hint` is"
        );
        assert!(
            serde_json::from_str::<Rule>(
                r#"{"type":"call-obligation","id":"a","level":"error","roots":"src/*",
                    "file_pattern":"^x$","must_call":{"symbol":"S","from":"m"}}"#
            )
            .is_err(),
            "`from` is not a field; `imported_from` is"
        );
    }

    /// And a correctly spelled config still parses, which is the half that
    /// would break if the attribute were put somewhere it does not belong.
    #[test]
    fn a_well_spelled_rule_still_parses() {
        let rule: Rule = serde_json::from_str(
            r#"{"type":"structure","id":"shape","level":"error",
                "roots":"src/*","allowed_subfolders":["types"]}"#,
        )
        .expect("parses");

        assert_eq!(rule.id().as_str(), "shape");
    }

    fn parse(json: &str) -> Rule {
        serde_json::from_str(json).expect("should deserialise")
    }

    /// Verbatim from docs/CONFIG.md. If this stops parsing, either the code or
    /// the documented example is wrong, and both matter.
    #[test]
    fn the_documented_structure_example_parses() {
        let rule = parse(
            r#"{
              "type": "structure",
              "id": "domain-entity-shape",
              "level": "error",
              "roots": ["packages/domain/src/*"],
              "allowed_subfolders": [
                "types", "calcs", "actions", "services",
                "mocks", "repositories", "const", "variants"
              ],
              "warn_subfolders": ["shared", "adapters"],
              "recurse_into": ["variants"]
            }"#,
        );

        let Rule::Structure(structure) = &rule else {
            panic!("expected a structure rule, got {}", rule.type_name());
        };
        assert_eq!(rule.id().as_str(), "domain-entity-shape");
        assert_eq!(rule.level(), Level::Error);
        assert_eq!(
            structure
                .allowed_subfolders
                .as_ref()
                .expect("names a list")
                .len(),
            8
        );
        assert_eq!(structure.warn_subfolders, ["shared", "adapters"]);
        assert_eq!(structure.recurse_into, ["variants"]);
        assert!(structure.filename_patterns.is_empty());
    }

    /// Also verbatim from docs/CONFIG.md: the filename sub-mode of `structure`.
    #[test]
    fn the_documented_filename_example_parses() {
        let rule = parse(
            r#"{
              "type": "structure",
              "id": "api-route-filenames",
              "level": "error",
              "roots": ["apps/app/src/app/api/**"],
              "filename_patterns": [
                "^route\\.ts$",
                "^route\\.(get|post|put|patch|delete|options)\\.ts$",
                "^DOC\\.md$"
              ]
            }"#,
        );

        let Rule::Structure(structure) = &rule else {
            panic!("expected a structure rule");
        };
        assert_eq!(structure.filename_patterns.len(), 3);
        // Absent, not empty: this rule constrains filenames and says nothing
        // about the directories beside them.
        assert_eq!(structure.allowed_subfolders, None);
    }

    /// The `naming` example, with the scope corrected to `use-cases/*` when
    /// decision 4 fixed what a scope glob selects.
    #[test]
    fn the_documented_naming_example_parses() {
        let rule = parse(
            r#"{
              "type": "naming",
              "id": "usecase-factory-name",
              "level": "error",
              "roots": ["packages/application/src/use-cases/*"],
              "file_pattern": "^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
              "must_export": {
                "kind": "function",
                "name": "{{pascal(name)}}",
                "signature_hint": "(deps: {{pascal(name)}}Deps)"
              }
            }"#,
        );

        let Rule::Naming(naming) = &rule else {
            panic!("expected a naming rule");
        };
        assert_eq!(naming.must_export.name, "{{pascal(name)}}");
        assert_eq!(naming.must_export.kind.as_slice(), ["function"]);
        assert!(naming.must_export.signature_hint.is_some());
    }

    /// `kind` takes the same one-or-many treatment as every other list, so
    /// `["function", "arrow"]` -- "callable, either form" -- is expressible.
    #[test]
    fn an_export_kind_may_be_a_list() {
        let rule = parse(
            r#"{
              "type": "naming", "id": "n", "level": "error", "roots": "src/*",
              "file_pattern": "^(?<name>.+)\\.ts$",
              "must_export": { "kind": ["function", "arrow"], "name": "{{pascal(name)}}" }
            }"#,
        );
        let Rule::Naming(naming) = &rule else {
            panic!("expected a naming rule");
        };
        assert_eq!(naming.must_export.kind.as_slice(), ["function", "arrow"]);
        assert_eq!(naming.must_export.signature_hint, None);
    }

    /// Issue #44. The frontmatter is not documentation — it is the schema
    /// three scripts and one index page depend on, and nothing type-checks a
    /// markdown file.
    #[test]
    fn a_frontmatter_rule_names_keys_a_vocabulary_and_an_agreement() {
        let rule = parse(
            r#"{"type":"frontmatter","id":"projeto-frontmatter","level":"error",
                "roots":["projetos/*"],
                "file_pattern":"^projeto\\.md$",
                "require":["id","nivel","componentes"],
                "one_of":{"nivel":["1","2","3"]},
                "equals":{"id":"{{raw(dirname)}}"}}"#,
        );

        let Rule::Frontmatter(front) = &rule else {
            panic!("expected a frontmatter rule, got {}", rule.type_name());
        };
        assert_eq!(front.require.as_slice(), ["id", "nivel", "componentes"]);
        assert_eq!(front.one_of["nivel"].as_slice(), ["1", "2", "3"]);
        assert_eq!(front.equals["id"], "{{raw(dirname)}}");
        assert_eq!(rule.type_name(), "frontmatter");
    }

    /// Issue #45. `spec-pair` is the rule for this and its baked-in ignores
    /// exclude every file involved by construction; and `projeto.md` →
    /// `notas.md` is not a `<stem>.<marker>.<ext>` relationship at all, so
    /// nothing about that rule would have helped.
    #[test]
    fn a_pair_rule_names_its_companion_literally() {
        let rule = parse(
            r#"{"type":"pair","id":"licao-tem-notas","level":"error",
                "roots":["projetos/*"],
                "file_pattern":"^projeto\\.md$",
                "must_exist":"notas.md"}"#,
        );

        let Rule::Pair(pair) = &rule else {
            panic!("expected a pair rule, got {}", rule.type_name());
        };
        assert_eq!(pair.file_pattern, r"^projeto\.md$");
        assert_eq!(pair.must_exist, "notas.md");
        assert_eq!(rule.type_name(), "pair");
    }

    /// The other half of the issue: the companion may sit outside the
    /// directory. `sketch/semaforo.ino` needs the `projeto.md` one level up,
    /// and there is no directory-scoped rule that can say that.
    #[test]
    fn a_companion_may_leave_the_directory() {
        let rule = parse(
            r#"{"type":"pair","id":"sketch-tem-licao","level":"error",
                "roots":["projetos/*/sketch"],
                "file_pattern":"\\.ino$",
                "must_exist":"../projeto.md"}"#,
        );

        let Rule::Pair(pair) = &rule else {
            panic!("expected a pair rule");
        };
        assert_eq!(pair.must_exist, "../projeto.md");
    }

    /// Issue #42. A unit of work is incomplete until its companion files are
    /// there, and `filename_patterns` is a whitelist of what *may* exist —
    /// satisfied by an empty directory, which is the state this rule is about.
    #[test]
    fn a_presence_rule_lists_what_must_exist() {
        let rule = parse(
            r#"{"type":"presence","id":"licao-completa","level":"error",
                "roots":["projetos/*"],
                "require":["projeto.md","exercicios.md","notas.md"],
                "require_any":["\\.ino$"]}"#,
        );

        let Rule::Presence(presence) = &rule else {
            panic!("expected a presence rule, got {}", rule.type_name());
        };
        assert_eq!(
            presence.require.as_slice(),
            ["projeto.md", "exercicios.md", "notas.md"]
        );
        assert_eq!(presence.require_any.as_slice(), [r"\.ino$"]);
        assert_eq!(rule.type_name(), "presence");
    }

    /// Issue #46. Decision 5 chose JSON over YAML and JSON5, so a config has
    /// no comments and the reason a rule exists has nowhere to live. It ends
    /// up in a commit message or a wiki, neither of which is in front of
    /// anybody at the moment the rule fires.
    #[test]
    fn a_rule_may_say_why_it_exists() {
        let rule = parse(
            r#"{"type":"import-boundary","id":"domain-forbids-app","level":"error",
                "why":"domain is published as its own package and the app is not",
                "from":["packages/domain/**"],
                "forbid_import_from":["packages/app/**"]}"#,
        );

        assert_eq!(
            rule.why(),
            Some("domain is published as its own package and the app is not")
        );
    }

    /// Every kind, because a reason is not a property of one of them. A rule
    /// that could not say why would be the one nobody could argue with.
    #[test]
    fn every_rule_kind_can_say_why() {
        let cases = [
            r#"{"type":"structure","id":"r","level":"error","roots":"src","why":"w",
                "allowed_subfolders":[]}"#,
            r#"{"type":"naming","id":"r","level":"error","roots":"src","why":"w",
                "file_pattern":"^(?<n>.+)$","must_export":{"kind":"any","name":"{{pascal(n)}}"}}"#,
            r#"{"type":"spec-pair","id":"r","level":"error","roots":"src","why":"w",
                "subfolders":"."}"#,
            r#"{"type":"import-boundary","id":"r","level":"error","from":"src","why":"w",
                "forbid_import_from":["x/**"]}"#,
            r#"{"type":"call-obligation","id":"r","level":"error","roots":"src","why":"w",
                "file_pattern":"^x$","must_call":{"symbol":"s","imported_from":"m"}}"#,
            r#"{"type":"no-passthrough","id":"r","level":"error","roots":"src","why":"w"}"#,
        ];

        for json in cases {
            assert_eq!(parse(json).why(), Some("w"), "{json}");
        }
    }

    /// Every kind of every kind, and this list is deliberately the complete
    /// ten rather than the six above.
    ///
    /// Issue #100 was scheduled first of its milestone for exactly this: every
    /// rule kind that lands after it carries `decision` from birth, and one
    /// that shipped without it would be a retrofit. A kind added to `Rule` and
    /// not to this list fails `every_kind_carries_a_decision_field`, which
    /// counts the arms.
    #[test]
    fn every_rule_kind_can_name_the_decision_it_implements() {
        let cases = [
            r#"{"type":"structure","id":"r","level":"error","roots":"src","decision":"ADR-014",
                "allowed_subfolders":[]}"#,
            r#"{"type":"naming","id":"r","level":"error","roots":"src","decision":"ADR-014",
                "file_pattern":"^(?<n>.+)$","must_export":{"kind":"any","name":"{{pascal(n)}}"}}"#,
            r#"{"type":"spec-pair","id":"r","level":"error","roots":"src","decision":"ADR-014",
                "subfolders":"."}"#,
            r#"{"type":"import-boundary","id":"r","level":"error","from":"src","decision":"ADR-014",
                "forbid_import_from":["x/**"]}"#,
            r#"{"type":"import-cycle","id":"r","level":"error","roots":"src","decision":"ADR-014"}"#,
            r#"{"type":"call-obligation","id":"r","level":"error","roots":"src","decision":"ADR-014",
                "file_pattern":"^x$","must_call":{"symbol":"s","imported_from":"m"}}"#,
            r#"{"type":"no-passthrough","id":"r","level":"error","roots":"src","decision":"ADR-014"}"#,
            r#"{"type":"presence","id":"r","level":"error","roots":"src/*","decision":"ADR-014",
                "require":["x.md"]}"#,
            r#"{"type":"pair","id":"r","level":"error","roots":"src/*","decision":"ADR-014",
                "file_pattern":"^a\\.md$","must_exist":"b.md"}"#,
            r#"{"type":"frontmatter","id":"r","level":"error","roots":"src/*","decision":"ADR-014",
                "file_pattern":"^a\\.md$","require":["id"]}"#,
            r#"{"type":"export-shape","id":"r","level":"error","roots":"src","decision":"ADR-014",
                "forbid_default":true}"#,
        ];

        let mut kinds = std::collections::BTreeSet::new();
        for json in cases {
            let rule = parse(json);
            assert_eq!(
                rule.decision().map(DecisionId::as_str),
                Some("ADR-014"),
                "{json}"
            );
            kinds.insert(rule.type_name());
        }

        // The set, not the count of the list: a case duplicated while editing
        // would otherwise let a kind drop off the list unnoticed.
        assert_eq!(
            kinds.len(),
            11,
            "these are meant to be every kind archwarden has, one each: {kinds:?}"
        );
    }

    /// Issue #101, and the sketch from the issue verbatim. Three claims in one
    /// kind, none of which mentions a filename — which is the whole point,
    /// because saying any of them through `naming` meant inventing a naming
    /// claim you did not mean.
    #[test]
    fn the_documented_export_shape_example_parses() {
        let rule = parse(
            r#"{
              "type": "export-shape",
              "id": "use-cases-return-the-pattern",
              "level": "error",
              "roots": ["src/use-cases/*"],
              "forbid_default": true,
              "max_exports": 1,
              "must_return": ["^ResponsePattern<.+,.+>$", "^Result<.+>$"],
              "why": "a use case returns the pattern, it never throws"
            }"#,
        );

        let Rule::ExportShape(shape) = &rule else {
            panic!("expected an export-shape rule, got {}", rule.type_name());
        };
        assert!(shape.forbid_default);
        assert_eq!(shape.max_exports, Some(1));
        assert_eq!(
            shape.must_return.as_slice(),
            ["^ResponsePattern<.+,.+>$", "^Result<.+>$"]
        );
        assert_eq!(rule.type_name(), "export-shape");
    }

    /// Each claim stands alone. A rule that only forbids defaults says nothing
    /// about how many exports there are or what they return, and a config that
    /// asks for one of the three must not be given the other two by default.
    #[test]
    fn each_export_shape_claim_is_optional_and_absent_by_default() {
        let rule =
            parse(r#"{"type":"export-shape","id":"no-defaults","level":"error","roots":"src"}"#);

        let Rule::ExportShape(shape) = &rule else {
            panic!("expected an export-shape rule");
        };
        assert!(!shape.forbid_default);
        assert_eq!(shape.max_exports, None);
        assert!(shape.must_return.is_empty());
    }

    /// `must_return` takes the one-or-many treatment every glob field takes, so
    /// a single pattern needs no brackets.
    #[test]
    fn must_return_accepts_a_bare_string() {
        let rule = parse(
            r#"{"type":"export-shape","id":"r","level":"error","roots":"src",
                "must_return":"^Result<.+>$"}"#,
        );

        let Rule::ExportShape(shape) = &rule else {
            panic!("expected an export-shape rule");
        };
        assert_eq!(shape.must_return.as_slice(), ["^Result<.+>$"]);
    }

    /// A rule that names none is every rule written before 0.21, and it stays
    /// exactly as valid. `config doctor` is the only thing that mentions it,
    /// at `warning`, and `check` says nothing at all — a repository's build
    /// must not fail because its config is under-documented.
    #[test]
    fn a_rule_that_names_no_decision_is_still_a_rule() {
        let rule = parse(r#"{"type":"no-passthrough","id":"r","level":"error","roots":"src"}"#);
        assert_eq!(rule.decision(), None);
    }

    /// An id with a space in it is refused on the way in, like every other id,
    /// rather than becoming a reference nothing can resolve.
    #[test]
    fn a_decision_reference_is_validated_as_an_id() {
        let bad = serde_json::from_str::<Rule>(
            r#"{"type":"no-passthrough","id":"r","level":"error","roots":"src","decision":"ADR 14"}"#,
        )
        .expect_err("should reject");
        assert!(bad.to_string().contains("decision id"), "{bad}");
    }

    /// Issue #43. The regex-over-a-directory-name capability existed on
    /// `naming.dir_pattern` and was reachable only through a door that requires
    /// a TypeScript parse, so a repository with no `.ts` near its folders could
    /// not use it at all.
    #[test]
    fn subfolder_patterns_parse_beside_the_lists() {
        let rule = parse(
            r#"{"type":"structure","id":"licao-nome-da-pasta","level":"error",
                "roots":["projetos"],
                "subfolder_patterns":["^\\d{2}-[a-z0-9-]+$"]}"#,
        );
        let Rule::Structure(structure) = &rule else {
            panic!("expected a structure rule");
        };

        assert_eq!(structure.subfolder_patterns, [r"^\d{2}-[a-z0-9-]+$"]);
        assert_eq!(structure.allowed_subfolders, None);
    }

    /// Issue #40. `[]` is a list of what may exist holding nothing, and the
    /// rule that says "this directory is a leaf" has no other spelling. The
    /// field has to be an option for that to be sayable at all: with a plain
    /// `Vec` an omitted field and an empty one arrive identical, so giving `[]`
    /// a meaning would give it to every config that never mentioned subfolders.
    #[test]
    fn an_absent_allowed_subfolders_is_not_an_empty_one() {
        let absent = parse(
            r#"{"type":"structure","id":"s","level":"error","roots":"referencia",
                "filename_patterns":["^[a-z-]+\\.md$"]}"#,
        );
        let Rule::Structure(absent) = &absent else {
            panic!("expected a structure rule");
        };
        assert_eq!(absent.allowed_subfolders, None);

        let empty = parse(
            r#"{"type":"structure","id":"s","level":"error","roots":"referencia",
                "allowed_subfolders":[]}"#,
        );
        let Rule::Structure(empty) = &empty else {
            panic!("expected a structure rule");
        };
        assert_eq!(empty.allowed_subfolders, Some(Vec::new()));
    }

    /// The field issue #39 asks for, in the shape the issue writes it.
    #[test]
    fn an_annotation_parses_as_one_value_or_a_list() {
        let one = parse(
            r#"{
              "type": "naming", "id": "n", "level": "error", "roots": "src/*",
              "file_pattern": "^(?<tool>.+)\\.tool\\.ts$",
              "must_export": {
                "kind": ["const"], "name": "AGENT_TOOL",
                "annotation": "AgentToolModule"
              }
            }"#,
        );
        let Rule::Naming(naming) = &one else {
            panic!("expected a naming rule");
        };
        assert_eq!(
            naming
                .must_export
                .annotation
                .as_ref()
                .expect("an annotation")
                .as_slice(),
            ["AgentToolModule"]
        );

        let many = parse(
            r#"{
              "type": "naming", "id": "n", "level": "error", "roots": "src/*",
              "file_pattern": "^(?<tool>.+)\\.tool\\.ts$",
              "must_export": {
                "kind": ["const"], "name": "AGENT_TOOL",
                "annotation": ["AgentToolModule", "LegacyToolModule"]
              }
            }"#,
        );
        let Rule::Naming(naming) = &many else {
            panic!("expected a naming rule");
        };
        assert_eq!(
            naming
                .must_export
                .annotation
                .as_ref()
                .expect("an annotation")
                .as_slice(),
            ["AgentToolModule", "LegacyToolModule"]
        );
    }

    /// Every rule written before the field existed asks for no annotation, and
    /// keeps meaning exactly what it meant.
    #[test]
    fn a_rule_that_omits_the_annotation_asks_for_none() {
        let rule = parse(
            r#"{
              "type": "naming", "id": "n", "level": "error", "roots": "src/*",
              "file_pattern": "^(?<name>.+)\\.ts$",
              "must_export": { "kind": "any", "name": "{{pascal(name)}}" }
            }"#,
        );
        let Rule::Naming(naming) = &rule else {
            panic!("expected a naming rule");
        };
        assert_eq!(naming.must_export.annotation, None);
    }

    #[test]
    fn the_documented_spec_pair_example_parses() {
        let rule = parse(
            r#"{
              "type": "spec-pair",
              "id": "domain-calcs-need-spec",
              "level": "error",
              "roots": ["packages/domain/src/*"],
              "subfolders": ["calcs", "services", "adapters"],
              "ignore_files": ["packages/domain/src/**/*.types.ts"]
            }"#,
        );

        let Rule::SpecPair(spec) = &rule else {
            panic!("expected a spec-pair rule");
        };
        assert_eq!(spec.subfolders.len(), 3);
        assert_eq!(spec.spec_markers.as_slice(), ["spec", "test"]);
        assert!(!spec.require_non_empty_spec, "defaults to off");
    }

    /// A spec-pair rule that omits everything optional still parses, and the
    /// defaults are the ones docs/RULES.md promises.
    #[test]
    fn spec_pair_defaults_match_the_documentation() {
        let rule = parse(
            r#"{
              "type": "spec-pair", "id": "s", "level": "error",
              "roots": "src/**", "subfolders": "."
            }"#,
        );
        let Rule::SpecPair(spec) = &rule else {
            panic!("expected a spec-pair rule");
        };
        assert_eq!(
            spec.spec_markers.as_slice(),
            ["spec", "test"],
            "both markers by default, as vitest and jest accept"
        );
        assert!(!spec.require_non_empty_spec);
        assert!(spec.ignore_files.is_empty());
    }

    /// Decision 14: a boundary is an ordinary rule with `type`, and its scope
    /// field is called `from`.
    #[test]
    fn the_documented_import_boundary_example_parses() {
        let rule = parse(
            r#"{
              "type": "import-boundary",
              "id": "ui-forbids-domain-direct",
              "level": "error",
              "from": "apps/**/src/**",
              "forbid_import_from": ["packages/domain/**"],
              "except": ["packages/domain/src/*/types/**"]
            }"#,
        );

        let Rule::ImportBoundary(boundary) = &rule else {
            panic!("expected an import-boundary rule");
        };
        assert_eq!(rule.type_name(), "import-boundary");
        assert_eq!(boundary.except.len(), 1);
        assert!(boundary.must_import_from.is_empty());
        assert!(
            boundary.include_type_only,
            "docs/RULES.md says type-only imports count unless opted out"
        );
    }

    /// Issue #71: the dependency nobody wrote down. `ui` does not import `db`,
    /// and it depends on it through `orders` anyway.
    #[test]
    fn a_boundary_can_forbid_reaching_as_well_as_importing() {
        let rule = parse(
            r#"{
              "type": "import-boundary",
              "id": "ui-must-not-reach-db",
              "level": "error",
              "from": "packages/ui/**",
              "forbid_reaching": ["packages/db/**"],
              "except": ["packages/db/types/**"]
            }"#,
        );

        let Rule::ImportBoundary(boundary) = &rule else {
            panic!("expected an import-boundary rule");
        };
        assert_eq!(boundary.forbid_reaching.len(), 1);
        assert!(
            boundary.forbid_import_from.is_empty(),
            "a rule may forbid reaching without forbidding the direct import, \
             which is the whole case this field exists for"
        );
        assert_eq!(boundary.except.len(), 1);
    }

    /// And it can name a module instead of repeating that module's globs, the
    /// way `forbid_module` does for the direct form.
    #[test]
    fn reaching_can_name_a_module() {
        let rule = parse(
            r#"{
              "type": "import-boundary", "id": "r", "level": "error",
              "from": "packages/ui/**", "forbid_reaching_modules": ["persistence"]
            }"#,
        );

        let Rule::ImportBoundary(boundary) = &rule else {
            panic!("expected an import-boundary rule");
        };
        assert_eq!(boundary.forbid_reaching_modules.len(), 1);
        assert!(boundary.forbid_reaching.is_empty());
    }

    /// A boundary that says nothing about reach parses as before, and the
    /// field is empty rather than absent-and-surprising. This is the case that
    /// keeps every rule already written as cheap as it was: an empty
    /// `forbid_reaching` is what tells the runner not to build a graph.
    #[test]
    fn a_boundary_that_says_nothing_about_reach_asks_for_nothing() {
        let rule = parse(
            r#"{"type":"import-boundary","id":"b","level":"error",
                "from":"packages/ui/**","forbid_import_from":["packages/domain/**"]}"#,
        );

        let Rule::ImportBoundary(boundary) = &rule else {
            panic!("expected an import-boundary rule");
        };
        assert!(boundary.forbid_reaching.is_empty());
        assert!(boundary.forbid_reaching_modules.is_empty());
    }

    /// `import-cycle` is written like every other rule, and its scope field is
    /// `roots` rather than `from`: it is not a rule about what may be
    /// imported, it is a rule about the files it governs.
    #[test]
    fn the_documented_import_cycle_example_parses() {
        let rule = parse(
            r#"{
              "type": "import-cycle",
              "id": "no-cycles",
              "level": "error",
              "roots": "packages/**"
            }"#,
        );

        let Rule::ImportCycle(cycle) = &rule else {
            panic!("expected an import-cycle rule");
        };
        assert_eq!(rule.type_name(), "import-cycle");
        assert_eq!(rule.id().as_str(), "no-cycles");
        assert_eq!(rule.level(), Level::Error);
        assert_eq!(rule.scope().len(), 1);
        assert!(
            cycle.include_type_only,
            "the same default `import-boundary` has, and the same field name: a \
             loop of type imports is a loop the compiler walks"
        );
    }

    /// And the opt-out, for a project that only cares about loops that exist
    /// at runtime.
    #[test]
    fn an_import_cycle_rule_can_ignore_type_only_loops() {
        let rule = parse(
            r#"{
              "type": "import-cycle", "id": "no-cycles", "level": "error",
              "roots": "packages/**", "include_type_only": false
            }"#,
        );

        let Rule::ImportCycle(cycle) = &rule else {
            panic!("expected an import-cycle rule");
        };
        assert!(!cycle.include_type_only);
    }

    /// `from` and `roots` are the same thing under two names, so one accessor
    /// serves both and the matcher never has to care which rule it holds.
    #[test]
    fn scope_reads_from_whichever_field_the_rule_uses() {
        let boundary = parse(
            r#"{"type":"import-boundary","id":"b","level":"error","from":"packages/domain/**"}"#,
        );
        let structure =
            parse(r#"{"type":"structure","id":"s","level":"error","roots":"packages/domain/**"}"#);

        assert_eq!(boundary.scope().as_slice(), ["packages/domain/**"]);
        assert_eq!(boundary.scope(), structure.scope());
    }

    #[test]
    fn the_documented_call_obligation_example_parses() {
        let rule = parse(
            r#"{
              "type": "call-obligation",
              "id": "non-get-routes-must-audit",
              "level": "error",
              "roots": ["apps/app/src/app/api/**"],
              "file_pattern": "^route\\.(post|put|patch|delete)\\.ts$",
              "must_call": {
                "symbol": "Event.save",
                "imported_from": "@flowmaatik/domain/event"
              }
            }"#,
        );

        let Rule::CallObligation(call) = &rule else {
            panic!("expected a call-obligation rule");
        };
        assert_eq!(call.must_call.symbol, "Event.save");
        assert_eq!(call.must_call.imported_from, "@flowmaatik/domain/event");
    }

    /// Every rule answers the same three questions whatever its type, which is
    /// what lets the matcher hold a heterogeneous list.
    #[test]
    fn every_rule_type_answers_id_level_and_scope() {
        let rules = [
            parse(r#"{"type":"structure","id":"a","level":"error","roots":"x/*"}"#),
            parse(
                r#"{"type":"naming","id":"b","level":"warning","roots":"x/*",
                    "file_pattern":"^(?<name>.+)$","must_export":{"kind":"any","name":"N"}}"#,
            ),
            parse(
                r#"{"type":"spec-pair","id":"c","level":"error","roots":"x/*","subfolders":"."}"#,
            ),
            parse(r#"{"type":"import-boundary","id":"d","level":"error","from":"x/*"}"#),
            parse(
                r#"{"type":"call-obligation","id":"e","level":"error","roots":"x/*",
                    "file_pattern":"^f$","must_call":{"symbol":"s","imported_from":"m"}}"#,
            ),
        ];

        let ids: Vec<_> = rules.iter().map(|r| r.id().as_str()).collect();
        assert_eq!(ids, ["a", "b", "c", "d", "e"]);

        let types: Vec<_> = rules.iter().map(Rule::type_name).collect();
        assert_eq!(
            types,
            [
                "structure",
                "naming",
                "spec-pair",
                "import-boundary",
                "call-obligation"
            ]
        );

        assert_eq!(rules[1].level(), Level::Warning);
        for rule in &rules {
            assert_eq!(rule.scope().as_slice(), ["x/*"]);
        }
    }

    /// An unknown discriminator names the valid ones. `graph` used to be a
    /// separate config key and is not any more, so somebody will try it.
    #[test]
    fn an_unknown_rule_type_is_rejected() {
        let err = serde_json::from_str::<Rule>(r#"{"type":"graph","id":"g","level":"error"}"#)
            .expect_err("should fail");
        let message = err.to_string();
        assert!(message.contains("structure"), "{message}");
        assert!(message.contains("import-boundary"), "{message}");
    }

    /// Severity is never inferred. Decision 1 puts the burden on the rule
    /// author to say up front whether a rule is a gate or a signpost.
    #[test]
    fn level_is_required() {
        assert!(
            serde_json::from_str::<Rule>(r#"{"type":"structure","id":"a","roots":"x/*"}"#).is_err()
        );
    }

    /// Ids are validated on the way in, so a bad one fails at load time rather
    /// than surfacing much later in a report.
    #[test]
    fn an_invalid_id_is_rejected_while_parsing() {
        let err = serde_json::from_str::<Rule>(
            r#"{"type":"structure","id":"bad id","level":"error","roots":"x"}"#,
        )
        .expect_err("should fail");
        assert!(err.to_string().contains("rule id"), "{err}");
    }

    /// Rules round-trip, which is what `agent-guide --format json` and the
    /// merged-config dump depend on.
    #[test]
    fn rules_round_trip_through_json() {
        let original = parse(
            r#"{"type":"import-boundary","id":"b","level":"error",
                "from":["a/**"],"forbid_import_from":["b/**"],"include_type_only":false}"#,
        );
        let json = serde_json::to_string(&original).expect("serialises");
        assert_eq!(
            serde_json::from_str::<Rule>(&json).expect("deserialises"),
            original
        );
        assert!(json.contains(r#""type":"import-boundary""#), "{json}");
    }
}

/// No file in scope may sit on an import loop.
///
/// The first rule whose question cannot be answered from one file, and the
/// reason a configuration carrying one costs a resolution pass over the whole
/// repository. See `docs/RULES.md`.
///
/// Deliberately no `ignored_circular_dependencies`. A cycle is a finding, and
/// `baseline` already accepts findings — per rule and per path, which is the
/// right granularity because every file on a loop is reported. Nx has such an
/// option because it has no baseline. Adding one here would be a second
/// mechanism for accepting a finding, and the two would disagree the first
/// time somebody used both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportCycleRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words. See [`StructureRule::why`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements, when it implements a declared one.
    ///
    /// A plain foreign key into [`Config::decisions`](crate::config::Config::decisions),
    /// written here rather than as a list of rule ids on the decision: this is
    /// where the author already is, there is no second list to keep in step, a
    /// deleted rule leaves nothing dangling, and a new rule that forgets its
    /// decision is visible in the one place it exists. Naming a decision the
    /// config does not declare is refused at compile — a reference to nothing
    /// is a typo, not a style. Issue #100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs this rule applies to.
    ///
    /// `roots` rather than `from`: `import-boundary` calls its scope `from`
    /// because it reads against `forbid_import_from`, and this rule forbids no
    /// destination. It is a rule about the files it governs, like the other
    /// four that spell it `roots`.
    ///
    /// It governs where a finding is *reported*, not what the graph is built
    /// from. The graph is always the whole repository, because a loop that
    /// leaves the scope and comes back is still a loop.
    pub roots: Patterns,
    /// Whether `import type` and inline `type` marks close a loop. Default
    /// `true`.
    ///
    /// Spelled and defaulted the same way `import-boundary` spells it. A type
    /// import is erased at runtime, so a loop made only of them cannot
    /// deadlock anything — and it is still a loop the compiler walks, which is
    /// why the default counts it and the opt-out exists for projects that only
    /// care about runtime.
    #[serde(default = "default_true")]
    pub include_type_only: bool,
    /// Narrow this rule to the files that import something.
    ///
    /// Path globs, matched against where an import *lands* — the same way an
    /// `import-boundary` matches. Without this a rule's population is where a
    /// file sits and what it is called; with it, what the file talks to.
    ///
    /// Leave it out and nothing changes, including the cost: a rule that does
    /// not ask never resolves an import. Issue #98, decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    ///
    /// Matched against the package a specifier belongs to, so `zod` covers
    /// `zod/v4` as it does everywhere else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// Which shapes of pure forwarding are refused, and where.
///
/// A file that only forwards another module is an indirection wearing the
/// name of a layer. The three shapes are all a way of holding a name and
/// adding nothing to it; see `docs/RULES.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoPassthroughRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words.
    ///
    /// The config already says *what* a rule does, in a form that cannot drift
    /// from what it enforces; a prose restatement of that would be a second
    /// source of truth going stale. The reason cannot drift, because nothing
    /// else records it — decision 5 chose JSON, so there are no comments, and
    /// a commit message is not in front of anybody at the moment a rule fires.
    ///
    /// Shown by the pre-write hook when it denies a write, by `describe`,
    /// `scaffold`, `agent-guide` and `config explain`, and beside a finding.
    /// Not a message override: `observed` and `expected` remain the whole
    /// diagnosis, and a `why` restating them will contradict them the day the
    /// rule changes. Issue #46.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements, when it implements a declared one.
    ///
    /// A plain foreign key into [`Config::decisions`](crate::config::Config::decisions),
    /// written here rather than as a list of rule ids on the decision: this is
    /// where the author already is, there is no second list to keep in step, a
    /// deleted rule leaves nothing dangling, and a new rule that forgets its
    /// decision is visible in the one place it exists. Naming a decision the
    /// config does not declare is refused at compile — a reference to nothing
    /// is a typo, not a style. Issue #100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Which shapes count. Defaults to all three.
    #[serde(default = "default_forms")]
    pub forms: Vec<PassthroughForm>,
    /// Files exempted, as globs.
    ///
    /// A legitimate re-export exists — a package's public API — and a rule
    /// without a way to say so is noise in the first repository that enables
    /// it. `allow_package_entrypoints` covers the common case without anyone
    /// writing a glob.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub except: Patterns,
    /// Whether a file reachable through a `package.json` `exports` entry is
    /// exempt. Default `true`.
    ///
    /// That file *is* the package's public API, and forwarding is what a
    /// public API is for. Without this the rule reports a package's entire
    /// surface the moment it is switched on.
    #[serde(default = "default_true")]
    pub allow_package_entrypoints: bool,
    /// Whether a file that forwards *some* of its exports and declares others
    /// is allowed. Default `true`.
    ///
    /// Set to `false` to hear about the shape that hides best: a file
    /// re-exporting six names from another module while declaring two of its
    /// own reads as a real module, and six of its eight exports are still an
    /// indirection its importers could skip.
    #[serde(default = "default_true")]
    pub allow_partial: bool,
    /// Narrow this rule to the files that import something.
    ///
    /// Path globs, matched against where an import *lands* — the same way an
    /// `import-boundary` matches. Without this a rule's population is where a
    /// file sits and what it is called; with it, what the file talks to.
    ///
    /// Leave it out and nothing changes, including the cost: a rule that does
    /// not ask never resolves an import. Issue #98, decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    ///
    /// Matched against the package a specifier belongs to, so `zod` covers
    /// `zod/v4` as it does everywhere else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// What a file exposes, without saying anything about what it is called.
///
/// `naming` couples the export to the *filename*. Plenty of architectural
/// decisions are about the export alone — *"we do not use default exports"*,
/// *"one export per file"*, *"every use case returns the pattern"* — and none
/// of them mentions a name. Saying any of them through `naming` meant inventing
/// a naming claim you did not mean in order to make an export claim you did.
///
/// Three claims in one kind, because they are the same question asked three
/// ways: *what does this file expose?* Splitting them would be three kinds
/// sharing one scope, one `roots` and one `why`. Issue #101.
///
/// # The division of labour, which is the whole design
///
/// `must_return` requires that a function **declares** its return type. It
/// does not check that the body conforms — that is `tsc`'s job, and `tsc` does
/// it well. What `tsc` cannot do is *require that you annotate at all*: a
/// function returning `{ ok: true }` with no return type compiles perfectly.
///
/// **archwarden guarantees the pattern is declared; `tsc` guarantees the body
/// conforms.** Neither alone is the guarantee a team wants, and together they
/// are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportShapeRule {
    /// Stable identifier, unique across the config and its presets.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words. See [`StructureRule::why`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements. See [`StructureRule::decision`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Whether a default export is refused.
    ///
    /// `false` by default, so a rule that only wants to say something about
    /// return types says nothing about defaults.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub forbid_default: bool,
    /// The most exports a file may have.
    ///
    /// **Counts what exists at runtime.** `type` and `interface` exports do
    /// not count, and the default counts as one. A file exporting a function
    /// and the interface of its dependencies is idiomatic TypeScript, and a
    /// `max_exports: 1` that fired on it would be a rule nobody leaves on —
    /// which is the same argument `spec-pair.skip_type_only` already makes one
    /// rule over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_exports: Option<usize>,
    /// Return types an exported function may declare, as regexes.
    ///
    /// A **list**, and that is what settles the alias problem without imposing
    /// a convention. `type Result<T> = ResponsePattern<T, Error>` is the same
    /// type and a different string, so a team that has aliases lists them:
    ///
    /// ```json
    /// "must_return": ["^ResponsePattern<.+,.+>$", "^Result<.+>$"]
    /// ```
    ///
    /// A team that decides *"annotate with the canonical name"* writes one
    /// pattern and gets that convention enforced — which is itself an
    /// architectural decision, and now one the config states rather than
    /// implies.
    ///
    /// Matched **text against text**, on the same terms as `naming`'s
    /// annotations: no resolution, no inference, no assignability. Pair it with
    /// `import-boundary.must_import_from` to close the remaining hole, which is
    /// somebody declaring a local lookalike under the canonical name.
    ///
    /// Applies to the exports that *can* return something — a `function`
    /// declaration, or a function or arrow assigned to a binding. A callable
    /// that declares nothing is a finding, which is the point.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub must_return: Patterns,
    /// Narrow this rule to the files that import something. See decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// One shape of pure forwarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PassthroughForm {
    /// `export { A } from './x'`, or an import followed by an export of the
    /// same name. A barrel file is this and nothing else.
    Reexport,
    /// `export const A = B` or `export type A = B`, where `B` was imported.
    Alias,
    /// A function whose whole body is `return g(<its own parameters>)`.
    Wrapper,
}

fn default_forms() -> Vec<PassthroughForm> {
    vec![
        PassthroughForm::Reexport,
        PassthroughForm::Alias,
        PassthroughForm::Wrapper,
    ]
}

#[cfg(test)]
mod narrowing_tests {
    use super::Rule;

    fn parsed(source: &str) -> Rule {
        serde_json::from_str(source).expect("the rule parses")
    }

    /// The second axis reaches the rule it was written on, whichever kind that
    /// is. Issue #98, decision 25.
    #[test]
    fn a_rule_carries_the_imports_it_was_narrowed_by() {
        let narrowed = parsed(
            r#"{"type":"call-obligation","id":"c","level":"error","roots":["src/*"],
                "file_pattern":"^x$","when_importing":"src/http/**",
                "when_importing_packages":["zod"],
                "must_call":{"symbol":"S","imported_from":"m"}}"#,
        );

        assert_eq!(narrowed.when_importing().as_slice(), ["src/http/**"]);
        assert_eq!(narrowed.when_importing_packages(), ["zod"]);
    }

    /// And a rule that names none carries none — which is what keeps every
    /// rule written before 0.20 as cheap as it was.
    #[test]
    fn a_rule_that_names_none_carries_none() {
        let plain = parsed(
            r#"{"type":"presence","id":"p","level":"error","roots":["src/*"],
                "require":["a.md"]}"#,
        );

        assert!(plain.when_importing().is_empty());
        assert!(plain.when_importing_packages().is_empty());
    }

    /// A directory rule carries it too: "some file inside imports X" is the
    /// reading decided for `presence` and `structure`.
    #[test]
    fn a_directory_rule_carries_it_as_well() {
        let narrowed = parsed(
            r#"{"type":"presence","id":"p","level":"error","roots":["src/*"],
                "when_importing":"src/db/**","require":["contract.md"]}"#,
        );

        assert_eq!(narrowed.when_importing().as_slice(), ["src/db/**"]);
    }

    /// `import-boundary` has none and never will: it already chooses its
    /// importers with `from`, `from_module` and `from_kind`, and a second way
    /// to say one thing is a second thing to get wrong.
    #[test]
    fn a_boundary_rule_never_narrows_by_import() {
        let boundary = parsed(
            r#"{"type":"import-boundary","id":"b","level":"error",
                "from":["src/**"],"forbid_import_from":["infra/**"]}"#,
        );

        assert!(boundary.when_importing().is_empty());
        assert!(boundary.when_importing_packages().is_empty());
    }
}
