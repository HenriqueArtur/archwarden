//! A file that must actually call something.

use archwarden_core::{
    ids::{DecisionId, RuleId},
    level::Level,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Patterns;

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
