//! Filename to exported symbol, and the shape of that export.

use archwarden_core::{
    ids::{DecisionId, RuleId},
    level::Level,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Patterns;
use crate::one_or_many::OneOrMany;

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
    /// Why this rule's scope is empty on purpose, when it is. The same field,
    /// with the same meaning, is on every rule kind — see `StructureRule`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_yet: Option<String>,
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
    /// Files this rule does not ask about.
    ///
    /// Repo-relative globs, spelled the way [`SpecPairRule::ignore_files`]
    /// spells them. Separate from the top-level `ignore`, which hides a file
    /// from *every* rule -- so a repository wanting one rule to skip a file
    /// and a `metadata` or `structure` rule to still see it had to choose
    /// between the two. Issue #153.
    ///
    /// Barrels do not belong here. `mod.rs`, `lib.rs`, `main.rs` and
    /// `index.ts` are exempt by construction: nobody should have to declare
    /// that a module declaration exports no symbol of its own.
    ///
    /// [`SpecPairRule::ignore_files`]: crate::rule::SpecPairRule::ignore_files
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub ignore_files: Patterns,
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
