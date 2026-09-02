//! A file that must be accompanied by another.

use archwarden_core::{
    ids::{DecisionId, RuleId},
    level::Level,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Patterns;

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
    /// Why this rule exists, in the author's words. The same field, with the same
    /// meaning, is on every rule kind.
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
    /// Why this rule's scope is empty on purpose, when it is. The same field,
    /// with the same meaning, is on every rule kind — see `StructureRule`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_yet: Option<String>,
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
