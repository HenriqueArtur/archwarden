//! Allowed subfolders, and the filenames a directory may hold.

use archwarden_core::{
    ids::{DecisionId, RuleId},
    level::Level,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Patterns;

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
