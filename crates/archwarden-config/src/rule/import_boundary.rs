//! Which layer may reach which.

use archwarden_core::{
    ids::{DecisionId, ModuleId, RuleId},
    level::Level,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::one_or_many::OneOrMany;

use super::Patterns;

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

pub(super) fn default_true() -> bool {
    true
}
