//! Why a config could not be compiled.
//!
//! Split out of the compiler itself: this is the surface a user reads when
//! their config is refused, and it is a quarter of what `compile` used to be.

use archwarden_core::{
    glob::GlobError, ids::RuleId, pattern::PatternError, scope::ScopeError, template,
};

/// Why a config could not be compiled.
///
/// Every variant names the rule, because a config has many and a message that
/// does not say which one leaves the user searching.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CompileError {
    /// A rule takes the id `governance`, which findings about ungoverned
    /// files already report under.
    ///
    /// Refused rather than resolved, because the two would be indistinguishable
    /// in `arch.baseline.json` — which keys on rule id and path — so accepting
    /// one would silently accept the other.
    #[error(
        "rule `{rule}` takes the id `governance`, which `governance: closed` \
         already reports under; a baseline could not tell the two apart"
    )]
    ReservedRuleId {
        /// The rule.
        rule: RuleId,
    },

    /// A rule names a module the config never declared.
    #[error("rule `{rule}` names module `{module}`, which this config does not declare")]
    UnknownModule {
        /// The rule.
        rule: RuleId,
        /// The name it used.
        module: archwarden_core::ids::ModuleId,
    },

    /// A rule names a decision the config never declared.
    ///
    /// Refused here rather than reported by `config doctor`, on the precedent
    /// [`UnknownModule`](Self::UnknownModule) already sets: a reference to
    /// nothing is a typo, and a typo should fail when the config loads rather
    /// than in a separate command the user may never run. A rule that names
    /// *no* decision is a different thing entirely, and stays valid. Issue
    /// #100.
    #[error("rule `{rule}` names decision `{decision}`, which this config does not declare")]
    UnknownDecision {
        /// The rule.
        rule: RuleId,
        /// The reference that matched nothing.
        decision: archwarden_core::ids::DecisionId,
    },

    /// A decision supersedes one the config never declared.
    #[error("decision `{decision}` supersedes `{superseded}`, which this config does not declare")]
    UnknownSuperseded {
        /// The decision doing the replacing.
        decision: archwarden_core::ids::DecisionId,
        /// The reference that matched nothing.
        superseded: archwarden_core::ids::DecisionId,
    },

    /// A decision another one supersedes says it is something else.
    ///
    /// Refused rather than silently overridden: a field that can contradict
    /// the edge is a field that will, and the omission it protects against --
    /// writing `supersedes` and forgetting to edit the old decision -- is what
    /// disarms `superseded-decision-still-enforced`. Issue #115.
    #[error(
        "decision `{decision}` says it is `{status}`, and `{by}` supersedes it; \
         drop the `status` or drop the supersession"
    )]
    StatusContradictsSupersession {
        /// The decision that was replaced.
        decision: archwarden_core::ids::DecisionId,
        /// What it claimed to be.
        status: &'static str,
        /// What replaced it.
        by: archwarden_core::ids::DecisionId,
    },

    /// Supersession runs in a circle.
    #[error("supersession runs in a circle: {decisions}")]
    SupersessionCycle {
        /// The chain, with the id it returned to on both ends.
        decisions: String,
    },

    /// A rejected option names a rule the config never declared.
    ///
    /// The alternative points at a rule the author already wrote, and a
    /// reference to nothing is a typo -- refused where a rule naming an
    /// undeclared module already is. Issue #114.
    #[error(
        "decision `{decision}` says `{option}` is refused by rule `{rule}`, \
         which this config does not declare"
    )]
    UnknownRefusingRule {
        /// The decision.
        decision: archwarden_core::ids::DecisionId,
        /// The option it rejected.
        option: String,
        /// The rule that does not exist.
        rule: RuleId,
    },

    /// A `metadata` rule asks about a key no comment could ever spell.
    ///
    /// The suppression grammar reaches every key beginning with `allow`
    /// first — `// archwarden-allow: x` is a suppression and never a claim —
    /// so a rule asking for one would report the key absent from every file in
    /// its scope, for ever, with no edit that could satisfy it. Refused where
    /// the config compiles, on [`UnknownDecision`](Self::UnknownDecision)'s
    /// precedent: a rule that cannot be met is a typo, not a style.
    #[error(
        "rule `{rule}` asks for metadata key `{key}`, which no comment can \
         spell: `archwarden-{key}:` reads as an `archwarden-allow` suppression"
    )]
    UnreachableMetadataKey {
        /// The rule.
        rule: RuleId,
        /// The key nothing could declare.
        key: String,
    },

    /// A rule names a module that declared no paths.
    #[error("rule `{rule}` names module `{module}`, which declares no `scope`")]
    ModuleHasNoScope {
        /// The rule.
        rule: RuleId,
        /// The module with nothing to be.
        module: archwarden_core::ids::ModuleId,
    },

    /// A rule quantifies over a kind no module wears.
    #[error("rule `{rule}` is about kind `{kind}`, which no module with a `scope` declares")]
    UnknownKind {
        /// The rule.
        rule: RuleId,
        /// The label nothing wears.
        kind: String,
    },

    /// A rule says what it permits and what it forbids.
    #[error(
        "rule `{rule}` sets `only_import_from` and `{other}`; \
         \"only these, except those\" is two rules"
    )]
    AllowlistAndDenylist {
        /// The rule.
        rule: RuleId,
        /// The field that contradicts the allowlist.
        other: &'static str,
    },

    /// A rule says its scope twice, in two fields.
    #[error("rule `{rule}` sets both `{one}` and `{other}`; use one")]
    ScopeSaidTwice {
        /// The rule.
        rule: RuleId,
        /// The first field.
        one: &'static str,
        /// The second.
        other: &'static str,
    },

    /// A rule says its scope in neither field.
    #[error("rule `{rule}` sets neither `{one}` nor `{other}`; it governs nothing")]
    ScopeMissing {
        /// The rule.
        rule: RuleId,
        /// The first field it could have used.
        one: &'static str,
        /// The second.
        other: &'static str,
    },

    /// A module's scope glob is not valid.
    ///
    /// Named by module rather than by rule, because every rule inside it is
    /// fine and the module is the thing to fix. Issue #74.
    #[error("module `{module}`: {source}")]
    ModuleScope {
        /// The module.
        module: archwarden_core::ids::ModuleId,
        /// What went wrong.
        #[source]
        source: ScopeError,
    },

    /// A rule's scope glob is not valid.
    #[error("rule `{rule}`: {source}")]
    Scope {
        /// The rule.
        rule: RuleId,
        /// What went wrong.
        #[source]
        source: ScopeError,
    },

    /// A glob outside a scope is not valid.
    #[error("rule `{rule}`, field `{field}`: {source}")]
    Glob {
        /// The rule.
        rule: RuleId,
        /// Which field held the glob.
        field: &'static str,
        /// What went wrong.
        #[source]
        source: GlobError,
    },

    /// A filename pattern is not valid, or uses an unsupported construct.
    #[error("rule `{rule}`, field `{field}`: {source}")]
    Pattern {
        /// The rule.
        rule: RuleId,
        /// Which field held the pattern.
        field: &'static str,
        /// What went wrong.
        #[source]
        source: Box<PatternError>,
    },

    /// `must_export.kind` names something that is not a declaration form.
    #[error(
        "rule `{rule}`: `{name}` is not an export kind. \
         Valid kinds are {available}, or `any`."
    )]
    UnknownExportKind {
        /// The rule.
        rule: RuleId,
        /// The name as written.
        name: String,
        /// The valid names.
        available: String,
    },

    /// `must_export` asks for an annotation on a form that cannot carry one.
    ///
    /// A rule with no satisfying input, which is worse than a wrong rule: it
    /// looks exactly like a repository nobody has migrated yet, and every file
    /// under it is reported forever.
    #[error(
        "rule `{rule}`: `annotation` cannot be satisfied by an export declared \
         as {kinds}. Only a binding (`const`, `let`, `var`) or a `class`, \
         through its `implements` clause, writes a type down beside its name; \
         a function declares a *return* type, which is a different claim."
    )]
    UnannotatableKind {
        /// The rule.
        rule: RuleId,
        /// The forms the rule accepts, none of which can be annotated.
        kinds: String,
    },

    /// A `pair.must_exist` is absolute, or empty.
    #[error(
        "rule `{rule}`: `must_exist` is relative to the file that needs the \
         companion, and `{path}` is not a relative path. Write `notas.md`, or \
         `../projeto.md` to reach out of the directory."
    )]
    CompanionNotRelative {
        /// The rule.
        rule: RuleId,
        /// The path as written.
        path: String,
    },

    /// A `pair.must_exist` carries a template placeholder.
    #[error(
        "rule `{rule}`: `must_exist` is literal, and `{path}` reads as a \
         template. A `pair` rule asks for one named companion beside the file, \
         so `{{{{...}}}}` is not substituted here and would be hunted for as \
         part of the name. For a companion whose name varies with the file, \
         `naming.must_export` and `frontmatter.equals` are where templates \
         live; a fixed name is what this field takes."
    )]
    CompanionIsATemplate {
        /// The rule.
        rule: RuleId,
        /// The path as written.
        path: String,
    },

    /// A `presence.require` entry names a path rather than a file.
    #[error(
        "rule `{rule}`: `require` takes filenames, and `{entry}` is a path. \
         One rule answers for one directory, which is what lets `describe` \
         answer for a directory that does not exist yet. Scope a second rule \
         one level down instead."
    )]
    RequireIsAPath {
        /// The rule.
        rule: RuleId,
        /// The entry as written.
        entry: String,
    },

    /// A `spec-pair` `spec_dirs` entry is not a single directory name.
    #[error(
        "rule `{rule}`: `spec_dirs` takes directory names, and `{entry}` is a \
         path. A spec directory is one level beside the file — `__tests__`, not \
         `__tests__/unit` — because a rule that reached further would accept a \
         spec anywhere below and report nothing. Name the deeper directory as \
         its own entry if it is also a spec directory."
    )]
    SpecDirIsAPath {
        /// The rule.
        rule: RuleId,
        /// The entry as written.
        entry: String,
    },

    /// A `spec-pair` marker is not a single filename component.
    #[error(
        "rule `{rule}`: `{marker}` is not a spec marker. A marker is one \
         filename component such as `spec` or `test`; the extension comes \
         from the source file, so `Component.tsx` wants `Component.spec.tsx` \
         without being told."
    )]
    InvalidSpecMarker {
        /// The rule.
        rule: RuleId,
        /// The marker as written.
        marker: String,
    },

    /// A `must_export.name` template refers to a capture group that neither
    /// pattern on the rule defines.
    #[error("rule `{rule}`: {source}")]
    Template {
        /// The rule.
        rule: RuleId,
        /// What went wrong.
        #[source]
        source: template::TemplateError,
    },

    /// `file_pattern` and `dir_pattern` both define the same capture group.
    #[error(
        "rule `{rule}`: capture group `{group}` is defined by both \
         `file_pattern` and `dir_pattern`, so `{{{{...({group})}}}}` in the \
         template has two values and no rule for choosing between them. \
         Rename one of them."
    )]
    DuplicateCaptureGroup {
        /// The rule.
        rule: RuleId,
        /// The group both patterns define.
        group: String,
    },

    /// The top-level `ignore` list holds an invalid glob.
    #[error("`ignore`: {source}")]
    Ignore {
        /// What went wrong.
        #[source]
        source: GlobError,
    },

    /// `skip_dirs.globs` holds an invalid glob.
    #[error("`skip_dirs.globs`: {source}")]
    SkipDirs {
        /// What went wrong.
        #[source]
        source: GlobError,
    },
}
