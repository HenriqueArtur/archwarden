//! The TDD gate: every unit file needs its spec.

use archwarden_core::{
    ids::{DecisionId, RuleId},
    level::Level,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::one_or_many::OneOrMany;

use super::Patterns;

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
    /// What makes a filename a spec — `spec`, `test`, `unit.spec`, anything
    /// the project writes. Not a closed vocabulary.
    ///
    /// A marker, not a whole suffix: the extension is taken from the source
    /// file, so `Component.tsx` wants `Component.spec.tsx` without anyone
    /// saying so. The default accepts `spec` and `test`, which is what vitest
    /// and jest do, so the common project needs no configuration here at all.
    ///
    /// **A marker may name more than one component.** `unit.spec` pairs
    /// `account-sanitize.ts` with `account-sanitize.unit.spec.ts`, which is
    /// what a repository distinguishing `*.unit.spec.ts` from `*.intg.spec.ts`
    /// needs — and it makes the rule *exact*, since an integration spec then
    /// does not satisfy a rule whose reason is about a unit test. Two markers
    /// where one ends the other are refused together: with `spec` and
    /// `unit.spec` both live, one file would answer for two units. Issue #174,
    /// decision 38.
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
