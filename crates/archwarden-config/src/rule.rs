//! The five rule shapes, as written in `arch.config.json`.
//!
//! These are *wire* types: a glob is a `String` here, and a regex is a
//! `String` too. Lowering them into the compiled types in `archwarden-core` is
//! a separate step, and it is what turns "this config might be valid" into
//! "this config is valid" — a compiled rule cannot exist unless every glob and
//! every regex in it parsed.
//!
//! See `docs/RULES.md` for semantics and `docs/CONFIG.md` for examples.

use archwarden_core::{ids::RuleId, level::Level};
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
pub enum Rule {
    /// Which folders may exist, and which filenames.
    Structure(StructureRule),
    /// The filename dictates the exported symbol's name.
    Naming(NamingRule),
    /// Every unit file needs a spec sibling.
    SpecPair(SpecPairRule),
    /// Layer A may not import from layer B.
    ImportBoundary(ImportBoundaryRule),
    /// Files matching a pattern must call a given symbol.
    CallObligation(CallObligationRule),
    /// A file whose whole content is forwarding another module.
    NoPassthrough(NoPassthroughRule),
    /// These files must exist in each governed directory.
    Presence(PresenceRule),
    /// A file of one kind must have a companion of another.
    Pair(PairRule),
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
            Self::CallObligation(r) => &r.id,
            Self::NoPassthrough(r) => &r.id,
            Self::Presence(r) => &r.id,
            Self::Pair(r) => &r.id,
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
            Self::CallObligation(r) => r.level,
            Self::NoPassthrough(r) => r.level,
            Self::Presence(r) => r.level,
            Self::Pair(r) => r.level,
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
            Self::CallObligation(r) => r.why.as_deref(),
            Self::NoPassthrough(r) => r.why.as_deref(),
            Self::Presence(r) => r.why.as_deref(),
            Self::Pair(r) => r.why.as_deref(),
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
            Self::CallObligation(r) => &r.roots,
            Self::NoPassthrough(r) => &r.roots,
            Self::Presence(r) => &r.roots,
            Self::Pair(r) => &r.roots,
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
            Self::CallObligation(_) => "call-obligation",
            Self::NoPassthrough(_) => "no-passthrough",
            Self::Presence(_) => "presence",
            Self::Pair(_) => "pair",
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
    /// Directory globs selecting the importer. Same semantics as `roots`.
    pub from: Patterns,
    /// Globs matched against the *resolved* import path. Matching means the
    /// import is illegal.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub forbid_import_from: Patterns,
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
    /// Exceptions, also matched against the resolved path.
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
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Regex over the filename.
    pub file_pattern: String,
    /// The call the file must contain.
    pub must_call: MustCall,
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
