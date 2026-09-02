//! Files that must exist inside a directory.

use archwarden_core::{
    ids::{DecisionId, RuleId},
    level::Level,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::one_or_many::OneOrMany;

use super::Patterns;

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
    /// Filenames that must **not** exist directly inside each governed
    /// directory.
    ///
    /// Names, not paths, on the same terms as `require` and refused the same
    /// way — and the only thing this rule reports for *existing*.
    ///
    /// The case it was added for is a lockfile: one package manager per
    /// repository is a decision every monorepo makes and nothing enforces, and
    /// `bun.lock` yes / `package-lock.json` no is one named file at a known
    /// path rather than a pattern over a folder's children.
    ///
    /// **Not `structure.filename_patterns`.** That field is a whitelist every
    /// child must match, so saying "not these three" there means enumerating
    /// everything else in a repository root — a list that is wrong the day
    /// somebody adds a file. Issue #177, decision 39.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub forbid: Patterns,
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
