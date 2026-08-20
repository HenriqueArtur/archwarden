//! The top-level shape of `arch.config.json`.

use archwarden_core::{
    ids::{DecisionId, ModuleId, RuleId},
    level::Level,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{one_or_many::OneOrMany, rule::Rule};

/// The config schema version this crate understands.
pub const SCHEMA_VERSION: u32 = 0;

/// A parsed `arch.config.json`, before `extends` is resolved and before globs
/// and regexes are compiled.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The `$schema` URL. Present so editors offer completion; archwarden
    /// itself ignores it.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// Config format version.
    pub version: u32,

    /// Where globs resolve from. Defaults to the config file's directory.
    ///
    /// A preset may not set this: it cannot know the layout of the repository
    /// including it, and silently relocating every glob in a config is not
    /// something a shared package should be able to do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,

    /// Presets to inherit from. A `./`-prefixed entry is a path; anything else
    /// is an npm package name.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub extends: OneOrMany<String>,

    /// Extra ignore globs, on top of `.gitignore`, which is always honoured.
    ///
    /// Ignore always wins over a rule's scope, however specific that scope is:
    /// a kill-switch that can be overridden by accident is not one.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub ignore: OneOrMany<String>,

    /// The language the HTML pages are written in.
    ///
    /// A repository decides this once, the way it decides its rules — the
    /// people reading a report of *this* project read it in one language, and
    /// putting that in the config means nobody has to remember a flag.
    /// `--lang` still wins, for the one run that wants the other.
    ///
    /// Reaches the pages and nothing else: the terminal, the JSON and the
    /// markdown digest are English whatever this says. See
    /// `crate::config::PageLanguage`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<PageLanguage>,

    /// Which languages this repository asks archwarden to read.
    ///
    /// Defaults to `["ts"]`, which is JavaScript and TypeScript together —
    /// every config written before this field meant exactly that, and still
    /// does.
    ///
    /// **Opt-in on purpose, and not because of cost.** A repository with no
    /// `.astro` file pays nothing for an Astro front-end either way. What the
    /// field buys is that widening what archwarden governs is a decision
    /// written in the config, rather than one that arrives with a dependency
    /// upgrade — and the un-opted state is loud rather than silent: a file in a
    /// language this config did not ask for is a *counted, named* skip, so a
    /// user who never read about the feature still finds out. Issue #13.
    #[serde(default = "default_languages", skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<Language>,

    /// Whether a file no rule governs is a finding.
    ///
    /// Absent means `open`, which is every config written before this field
    /// and still means what it meant.
    #[serde(default, skip_serializing_if = "Governance::is_open")]
    pub governance: Governance,

    /// The `_`-prefix escape hatch.
    #[serde(default)]
    pub skip_dirs: SkipDirs,

    /// The decisions this configuration enforces, as prose.
    ///
    /// A rule says *why* it exists; this says *what decision it implements*,
    /// which is the difference between a config that enforces an architecture
    /// and one that describes it. The block carries only the prose — the link
    /// between the two is written on the rule, in [`Rule::decision`], because
    /// a foreign key pointing the other way is a second list to keep in step.
    /// Issue #100.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<Decision>,

    /// Rules grouped under a label, which is what findings report in brackets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<Module>,

    /// Rules belonging to no module, typically import boundaries, which are
    /// cross-module by nature. They report as `[*]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,

    /// Rule ids inherited from a preset that this config drops.
    ///
    /// Without this, one unwanted rule makes a whole preset unusable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disable: Vec<RuleId>,
}

/// A decision the architecture rests on, and the rules enforce.
///
/// Prose and nothing else. What it does *not* carry is the list of rules that
/// serve it: a rule names its decision in [`Rule::decision`], which is where
/// the author already is when they write the rule, and which leaves nothing
/// dangling when a rule is deleted. Issue #100 weighed both directions and
/// this is the one where a new rule that forgets its decision is visible in
/// the one place it exists, rather than absent from a list nobody re-reads.
///
/// Deliberately not a place to restate what the rules enforce. `CONFIG.md`
/// already argues that a prose restatement of a check is a second source of
/// truth going stale — this explains the *choice*, and the rules remain the
/// only statement of what is enforced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    /// The reference the team already uses for it, such as `ADR-014`.
    ///
    /// Unique across the config, its presets, *and* its rule ids: `config
    /// explain` takes either, and an id that names two things names neither.
    pub id: DecisionId,
    /// What was decided, in one line.
    ///
    /// Required, unlike everything else here. The id is a reference and this
    /// is the sentence a denial says out loud — a decision carrying only an id
    /// would leave the hook's message exactly as opaque as the rule id it was
    /// meant to replace.
    pub title: String,
    /// Why it was decided that way, when the author said it here.
    ///
    /// Optional because `link` is often the better answer: a decision whose
    /// reasoning is three paragraphs belongs in the document, and duplicating
    /// its first sentence here is the drift this field exists to avoid. One of
    /// the two is what a reader needs; neither is required, because a team
    /// adopting this incrementally starts with titles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// Where it is written down: a path, a URL, a ticket.
    ///
    /// Carried verbatim and never resolved. archwarden does not check that it
    /// exists — a decision recorded in a wiki this process cannot reach is
    /// still recorded, and a linter that refused the reference would push
    /// people to omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// Whether it still holds.
    ///
    /// Not decoration, and the reason this field exists: a `superseded`
    /// decision whose rules still fire is a config saying two things at once,
    /// and `config doctor` reports it as an error. See
    /// [`DecisionStatus`].
    ///
    /// Optional so *unset* can be told from *explicitly accepted*, which is
    /// what lets a decision another one supersedes be refused for calling
    /// itself accepted rather than silently overridden. Absent means
    /// `accepted`, as it always did. Issue #115.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<DecisionStatus>,
    /// The decisions this one replaces.
    ///
    /// Written on the **new** decision, which is where the author already is:
    /// the old one does not have to be edited to be replaced, there is no
    /// second list to keep in step, and the reverse — what replaced this — is
    /// computed. Decision 26's argument for `rule.decision`, one level over.
    ///
    /// A decision named here is `superseded`, and does not repeat it. Naming
    /// a decision the config does not declare, naming itself, or closing a
    /// cycle are all refused where the config compiles. Issue #115.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub supersedes: OneOrMany<DecisionId>,
    /// What was considered and rejected, and why it lost.
    ///
    /// The half of an ADR that stops the losing option being proposed again —
    /// by the next person, or by an agent that reads the rules, complies, and
    /// helpfully suggests the thing that was already tried. A rule says what is
    /// refused; this says what was *weighed*. Issue #114.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<Alternative>,
    /// Whether any rule can keep this decision.
    ///
    /// `config doctor` reports a decision no rule implements, because a
    /// decision written down and unenforced is usually one somebody meant to
    /// enforce. Some are not: *"Pub/Sub is the message broker"*,
    /// *"money amounts are decimal, never float"* — real decisions, written
    /// down, and not a shape any rule here can hold.
    ///
    /// Without this the config punishes a repository for declaring everything
    /// it decided, and the honest way to keep it quiet is to leave decisions
    /// out — which is the opposite of what the feature is for. Issue #160.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enforcement: Option<Enforcement>,
    /// Why no rule can keep it, when `enforcement` says none can.
    ///
    /// Required with that claim, and refused without it. `Alternative::why_not`
    /// makes the same argument and it is the same argument: *"an option with
    /// no argument against it is a name nobody can disagree with"*. Unenforced
    /// with no reason is the button everybody presses, and it makes the config
    /// quieter and less true at once.
    ///
    /// A claim, not a note. `doctor` reports a decision that carries this and
    /// *does* have a rule, because then the sentence is wrong and the config
    /// says two things. Issue #160.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why_not_enforceable: Option<String>,
    /// Where this decision applies, as directory globs.
    ///
    /// The same shape as [`Module::scope`], for the same reason #74 gave it to
    /// a module: without it the decision is only a label, and nothing can bring
    /// it to the person standing in the paths it governs.
    ///
    /// An enforced decision already has an implicit scope through the roots of
    /// the rules that name it. This is what an *unenforced* one has instead —
    /// which is why it matters most for the decisions `enforcement: "none"`
    /// describes: there is no gate that will catch their violation later, so
    /// arriving unprompted is the only way they arrive at all. Issue #161.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub scope: OneOrMany<String>,
}

/// Whether a decision is one any rule can keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Enforcement {
    /// No rule can express this decision, and `why_not_enforceable` says why.
    ///
    /// One variant, deliberately. The absent case already means "a rule should
    /// keep this", which is what `decision-nobody-enforces` reports on, so a
    /// second spelling of the default would be two ways to say one thing. A
    /// value naming *how* it is enforced would be a third source of truth
    /// beside the rule and its `decision` link.
    None,
}

/// One option a decision considered and did not take.
///
/// # It points at a rule; it does not become one
///
/// `refused_by` names a rule the author already wrote. It was measured the
/// other way first: a generated refusal needs a rule id, `baseline` keys on
/// rule ids, and every way of deriving one is unstable — a slug of `option`
/// breaks when the sentence is reworded, an index breaks when the list is
/// reordered. Either way, accepted debt is orphaned in silence.
///
/// What the reference buys is the page's most honest line: an alternative with
/// a rule is mechanically refused, one without it is written down and nothing
/// stops anybody taking it, and the two are told apart by whether the field is
/// filled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Alternative {
    /// The option, named as the team named it.
    pub option: String,
    /// Why it lost.
    ///
    /// Required. An option with no argument against it is a name nobody can
    /// disagree with, and the argument is the whole reason to write the option
    /// down — the same shape `archwarden-allow` takes, where a marker with no
    /// reason is not a marker.
    pub why_not: String,
    /// The rule that refuses this option today, when one does.
    ///
    /// Refused at compile if no rule has that id, where a rule naming an
    /// undeclared module already is. Whether that rule also names this
    /// decision is left to the author: a constraint there would be a field
    /// decided before anybody asked for one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused_by: Option<RuleId>,
}

/// Whether a decision still holds.
///
/// The three `docs/DECISIONS.md` already declares, so the config of this tool
/// can describe the ADRs of this tool. Only one of them is checked:
/// `superseded` with rules still enforcing it. `proposed` is deliberately
/// silent — a decision under trial with rules already running is how one is
/// trialled, and reporting it would nag the practice this feature is trying to
/// encourage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum DecisionStatus {
    /// It holds. The default, and what a decision that says nothing means.
    #[default]
    Accepted,
    /// Written down, not yet settled. Reported by nothing.
    Proposed,
    /// Replaced. Rules still enforcing it are an error in `config doctor`.
    Superseded,
}

impl DecisionStatus {
    /// Whether this is the default, for `skip_serializing_if`.
    #[must_use]
    pub fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted)
    }
}

/// A named group of rules.
///
/// A module is a label, not a scope: it exists so a finding can say
/// `[domain] packages/domain/src/user/wrong-folder`. Each rule still carries
/// its own scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Module {
    /// The label.
    pub id: ModuleId,
    /// The paths this module is.
    ///
    /// Optional, so every config written before this keeps working: a module
    /// with no scope is what a module has always been, a namespace for rules.
    ///
    /// With one, the module stops being only a label. A rule inside it reaches
    /// where its own `roots` and this agree, a boundary elsewhere can name the
    /// module instead of re-describing it by glob, and `config doctor` can ask
    /// two questions it could not ask before: whether a module reaches
    /// anything, and whether any rule references it. Issue #74.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub scope: OneOrMany<String>,
    /// What sort of module this is, for rules that quantify over sorts.
    ///
    /// One label, not a list, and the reduction is the design. Nx carries a
    /// list per project because it combines two independent axes — layer and
    /// bounded context. That buys composition across dimensions and costs a
    /// second vocabulary, an extra indirection when a rule does not fire, and
    /// a place to declare tags.
    ///
    /// One axis is what the case needs: assembly versus piece. If a second
    /// real one appears — context, ownership — that is where the conversation
    /// resumes, with a repository that cannot be expressed rather than with a
    /// comparison to another tool. Issue #76.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Why this module exists, in the author's words.
    ///
    /// A module is a bigger decision than any rule inside it — one sentence
    /// explaining why `domain` is sealed explains every rule under it — so it
    /// gets its own, and a rule's `why` never substitutes for it. Issue #46.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The rules in it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
}

/// Whether every file must be governed by some rule.
///
/// `CONFIG.md` calls a rule enforcing nothing the worst failure a linter has,
/// and a file no rule governs is that sentence one level up: indistinguishable
/// from a file that satisfies everything. `config coverage` reports the gap;
/// this turns it into findings. Issue #60.
///
/// Written either way:
///
/// ```json
/// { "governance": "closed" }
/// { "governance": { "mode": "closed", "level": "warning" } }
/// ```
///
/// The shorthand is `error`, because a gate that does not fail a build is a
/// report. The long form exists for the migration the report is for: a
/// repository with two thousand ungoverned files can turn this on as a
/// `warning` today, see the number in CI without blocking anyone, and close it
/// over time. `baseline` is the other way to do that and produces a
/// two-thousand-entry committed file; both are honest and they suit different
/// repositories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Governance {
    /// `"open"` or `"closed"`, taking the default level.
    Mode(GovernanceMode),
    /// The mode with a level of its own.
    Detailed {
        /// Open or closed.
        mode: GovernanceMode,
        /// What an ungoverned file reports as. Defaults to `error`.
        #[serde(default = "default_governance_level")]
        level: Level,
    },
}

/// Whether the architecture is closed to files no rule mentions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum GovernanceMode {
    /// A file no rule governs is not reported. The default, and what every
    /// config written before this field means.
    #[default]
    Open,
    /// A file no rule governs is a finding, and `ignore` is the escape hatch —
    /// which gains a meaning it did not have: **deliberately outside the
    /// architecture**, rather than merely unchecked.
    Closed,
}

fn default_governance_level() -> Level {
    Level::Error
}

impl Default for Governance {
    fn default() -> Self {
        Self::Mode(GovernanceMode::Open)
    }
}

impl Governance {
    /// Whether this configuration reports ungoverned files, and at what level.
    ///
    /// `None` is open. Returning the level rather than a bool is what keeps
    /// the two questions — *does it report* and *how loudly* — from being
    /// asked separately and answered inconsistently.
    #[must_use]
    pub fn level(self) -> Option<Level> {
        match self {
            Self::Mode(GovernanceMode::Open)
            | Self::Detailed {
                mode: GovernanceMode::Open,
                ..
            } => None,
            Self::Mode(GovernanceMode::Closed) => Some(default_governance_level()),
            Self::Detailed {
                mode: GovernanceMode::Closed,
                level,
            } => Some(level),
        }
    }

    /// Whether this is the default, for `skip_serializing_if`.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.level().is_none()
    }
}

/// A language the HTML pages can be written in.
///
/// Separate from [`Language`], which is about *source* archwarden reads. These
/// two would be a confusing single field: one says what the tool can parse and
/// the other says what a person wants to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum PageLanguage {
    /// English.
    En,
    /// Brazilian Portuguese.
    PtBr,
}

/// A language archwarden has a front-end for.
///
/// Markdown is absent on purpose: a `frontmatter` rule names the documents it
/// is about, so asking for it in two places would let them disagree. This list
/// is for languages whose files would otherwise be read as *code* by a rule
/// that never named them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Language {
    /// JavaScript and TypeScript, which are one front-end.
    Ts,
    /// Astro components: the TypeScript module inside the `---` fence.
    Astro,
    /// Rust, read by its own front-end.
    ///
    /// Asked for rather than assumed, on the same terms as `astro`: a
    /// repository with a `src-tauri/` beside its `src/` has `.rs` files whose
    /// author never asked archwarden to have an opinion about them, and
    /// decision 31 turns "held to a rule nobody chose" into a named skip.
    Rust,
}

impl Language {
    /// The name as it is written in a config.
    ///
    /// Lives here rather than at the call site because the enum is
    /// `#[non_exhaustive]`: a match in another crate needs a wildcard arm and
    /// would print a list quietly missing a new language. This one is
    /// exhaustive, so adding a variant fails to build until somebody names it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ts => "ts",
            Self::Astro => "astro",
            Self::Rust => "rust",
        }
    }
}

fn default_languages() -> Vec<Language> {
    vec![Language::Ts]
}

/// Which directories are exempt, and from what.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkipDirs {
    /// Directory name prefixes. Empty disables the escape hatch.
    #[serde(default = "default_skip_prefixes")]
    pub prefixes: Vec<String>,
    /// Globs, for cases a prefix cannot express.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub globs: Vec<String>,
    /// How far the exemption reaches.
    #[serde(default)]
    pub scope: SkipScope,
}

fn default_skip_prefixes() -> Vec<String> {
    vec!["_".to_owned()]
}

impl Default for SkipDirs {
    fn default() -> Self {
        Self {
            prefixes: default_skip_prefixes(),
            globs: Vec::new(),
            scope: SkipScope::default(),
        }
    }
}

/// How far a `skip_dirs` exemption reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SkipScope {
    /// Exempt from `structure` rules only. Files inside are still parsed and
    /// still enter the import graph.
    #[default]
    Structure,
    /// Removed from the walk entirely, and therefore invisible to every rule.
    ///
    /// Rarely what you want: it turns `mkdir _x && mv offender.ts _x/` into a
    /// way to bypass any import boundary. `config doctor` warns when this is
    /// combined with `import-boundary` rules.
    Walk,
}

impl Config {
    /// Every rule in the config, paired with the module it came from.
    ///
    /// Top-level rules yield `None`. Rules named in `disable` are skipped, so
    /// no caller has to remember to apply it.
    pub fn rules(&self) -> impl Iterator<Item = (Option<&ModuleId>, Option<&str>, &Rule)> {
        let from_modules = self.modules.iter().flat_map(|m| {
            m.rules
                .iter()
                .map(move |r| (Some(&m.id), m.why.as_deref(), r))
        });
        let top_level = self.rules.iter().map(|r| (None, None, r));

        from_modules
            .chain(top_level)
            .filter(|(_, _, rule)| !self.disable.contains(rule.id()))
    }

    /// Whether this config's `version` is one this build understands.
    #[must_use]
    pub fn version_is_supported(&self) -> bool {
        self.version == SCHEMA_VERSION
    }
}

#[cfg(test)]
mod tests {
    /// The name `config validate` prints is the name a config can be written
    /// with. Issue #158 made these visible: a language a preset turned on is
    /// reported back, and reporting a spelling nobody can type would send the
    /// reader to write something the parser then refuses.
    ///
    /// Asserted by round-tripping through serde rather than by repeating the
    /// literals, so the two cannot drift while both still look right.
    #[test]
    fn the_name_a_language_prints_is_the_name_a_config_writes() {
        for language in [Language::Ts, Language::Astro, Language::Rust] {
            let written = language.as_str();
            let read: Language = serde_json::from_str(&format!("\"{written}\""))
                .unwrap_or_else(|error| panic!("`{written}` is not a language: {error}"));

            assert_eq!(read, language, "`{written}` reads back as something else");
        }

        // And they are distinct, which a constant returned for every variant
        // would not be.
        assert_eq!(Language::Ts.as_str(), "ts");
        assert_ne!(Language::Rust.as_str(), Language::Astro.as_str());
    }

    use super::*;

    /// The same protection as on the rules, at the top level and in every
    /// nested object. A misspelled key here disables a setting rather than a
    /// rule, which is quieter still.
    #[test]
    fn an_unknown_field_is_refused_at_every_level() {
        let cases = [
            r#"{"version":0,"rulez":[]}"#,
            r#"{"version":0,"modules":[{"id":"m","rulez":[]}]}"#,
            r#"{"version":0,"skip_dirs":{"prefix":["_"]}}"#,
        ];

        for case in cases {
            assert!(
                serde_json::from_str::<Config>(case).is_err(),
                "accepted an unknown field: {case}"
            );
        }
    }

    /// And every documented key still parses, including the ones with a
    /// `serde` rename, which is what would break if the attribute were applied
    /// carelessly.
    #[test]
    fn a_well_spelled_config_still_parses() {
        let config: Config = serde_json::from_str(
            r#"{
                "$schema": "https://raw.githubusercontent.com/HenriqueArtur/archwarden/main/schema/v0.json",
                "version": 0,
                "extends": ["@org/preset"],
                "ignore": ["dist/**"],
                "skip_dirs": {"prefixes": ["_"], "scope": "structure"},
                "modules": [{"id": "domain", "rules": []}],
                "rules": []
            }"#,
        )
        .expect("parses");

        assert_eq!(config.version, 0);
    }

    use archwarden_core::level::Level;

    fn parse(json: &str) -> Config {
        serde_json::from_str(json).expect("should deserialise")
    }

    /// The minimal useful config, verbatim from docs/CONFIG.md.
    #[test]
    fn the_documented_minimal_config_parses() {
        let config = parse(
            r#"{
              "$schema": "https://raw.githubusercontent.com/HenriqueArtur/archwarden/main/schema/v0.json",
              "version": 0,
              "modules": [
                {
                  "id": "src",
                  "rules": [
                    {
                      "type": "spec-pair",
                      "id": "src-needs-spec",
                      "level": "error",
                      "roots": ["src/**"],
                      "subfolders": ["."]
                    }
                  ]
                }
              ]
            }"#,
        );

        assert!(config.version_is_supported());
        assert_eq!(
            config.schema.as_deref(),
            Some("https://raw.githubusercontent.com/HenriqueArtur/archwarden/main/schema/v0.json")
        );

        let rules: Vec<_> = config.rules().collect();
        assert_eq!(rules.len(), 1);
        let (module, _, rule) = &rules[0];
        assert_eq!(module.map(ModuleId::as_str), Some("src"));
        assert_eq!(rule.id().as_str(), "src-needs-spec");
    }

    /// Rules live in two places, and the iterator flattens both. Module rules
    /// come first so report grouping is stable.
    #[test]
    fn rules_come_from_modules_and_from_the_top_level() {
        let config = parse(
            r#"{
              "version": 0,
              "modules": [
                { "id": "domain", "rules": [
                  {"type":"structure","id":"in-module","level":"error","roots":"x/*"}
                ]}
              ],
              "rules": [
                {"type":"import-boundary","id":"top-level","level":"error","from":"y/*"}
              ]
            }"#,
        );

        let collected: Vec<_> = config
            .rules()
            .map(|(m, _, r)| (m.map(ModuleId::as_str), r.id().as_str()))
            .collect();

        assert_eq!(
            collected,
            [(Some("domain"), "in-module"), (None, "top-level")]
        );
    }

    /// `disable` is applied by the iterator, so a caller cannot forget it and
    /// accidentally run a rule the config dropped.
    #[test]
    fn disabled_rules_never_reach_a_caller() {
        let config = parse(
            r#"{
              "version": 0,
              "disable": ["unwanted"],
              "rules": [
                {"type":"structure","id":"kept","level":"error","roots":"x/*"},
                {"type":"structure","id":"unwanted","level":"error","roots":"y/*"}
              ]
            }"#,
        );

        let ids: Vec<_> = config.rules().map(|(_, _, r)| r.id().as_str()).collect();
        assert_eq!(ids, ["kept"]);
    }

    /// The escape hatch defaults to `_`, exempting those directories from
    /// structure rules only -- files inside stay in the import graph.
    #[test]
    fn skip_dirs_defaults_to_underscore_and_structure_scope() {
        let config = parse(r#"{"version": 0}"#);
        assert_eq!(config.skip_dirs.prefixes, ["_"]);
        assert!(config.skip_dirs.globs.is_empty());
        assert_eq!(config.skip_dirs.scope, SkipScope::Structure);
    }

    #[test]
    fn skip_dirs_can_be_widened_to_the_whole_walk_or_switched_off() {
        let widened = parse(r#"{"version":0,"skip_dirs":{"scope":"walk"}}"#);
        assert_eq!(widened.skip_dirs.scope, SkipScope::Walk);
        assert_eq!(
            widened.skip_dirs.prefixes,
            ["_"],
            "unset fields keep defaults"
        );

        let off = parse(r#"{"version":0,"skip_dirs":{"prefixes":[]}}"#);
        assert!(off.skip_dirs.prefixes.is_empty());
        assert_eq!(off.skip_dirs.scope, SkipScope::Structure);
    }

    /// Everything except `version` is optional, so a config can start tiny and
    /// grow.
    #[test]
    fn only_version_is_required() {
        let config = parse(r#"{"version": 0}"#);
        assert!(config.rules().next().is_none());
        assert!(config.modules.is_empty());
        assert!(config.extends.is_empty());
        assert!(config.ignore.is_empty());
        assert_eq!(config.root, None);

        assert!(serde_json::from_str::<Config>("{}").is_err());
    }

    /// A future config format must not be silently misread by an old binary.
    #[test]
    fn an_unknown_version_is_recognised_as_unsupported() {
        assert!(!parse(r#"{"version": 1}"#).version_is_supported());
        assert!(parse(r#"{"version": 0}"#).version_is_supported());
    }

    /// `ignore` and `extends` take the same one-or-many treatment as rule
    /// scopes, so a single entry needs no brackets.
    #[test]
    fn list_fields_accept_a_bare_string() {
        let config = parse(r#"{"version":0,"ignore":"**/dist/**","extends":"./base.json"}"#);
        assert_eq!(config.ignore.as_slice(), ["**/dist/**"]);
        assert_eq!(config.extends.as_slice(), ["./base.json"]);
    }

    /// A config round-trips, which is what a merged-config dump depends on.
    /// Unset optional fields stay absent rather than reappearing as nulls and
    /// empty arrays.
    #[test]
    fn a_config_round_trips_without_gaining_empty_fields() {
        let original = parse(
            r#"{"version":0,"rules":[
            {"type":"structure","id":"a","level":"warning","roots":"x/*"}]}"#,
        );
        let json = serde_json::to_string(&original).expect("serialises");

        assert_eq!(
            serde_json::from_str::<Config>(&json).expect("parses"),
            original
        );
        assert!(!json.contains("modules"), "{json}");
        assert!(!json.contains("disable"), "{json}");
        assert!(!json.contains("$schema"), "{json}");

        let (module, _, rule) = original.rules().next().expect("one rule");
        assert_eq!(module, None);
        assert_eq!(rule.level(), Level::Warning);
    }
}

#[cfg(test)]
mod decision_tests {
    use super::{Config, Decision, DecisionStatus};
    use archwarden_core::ids::DecisionId;

    fn parse(json: &str) -> Config {
        serde_json::from_str(json).expect("should deserialise")
    }

    /// The whole block, verbatim from issue #100.
    #[test]
    fn a_decision_carries_its_prose() {
        let config = parse(
            r#"{
              "version": 0,
              "decisions": [
                {
                  "id": "ADR-014",
                  "title": "The domain does not know about transport",
                  "why": "It is published, and a consumer must not inherit our HTTP client.",
                  "link": "docs/adr/014-domain-transport.md",
                  "status": "accepted"
                }
              ]
            }"#,
        );

        let decision = &config.decisions[0];
        assert_eq!(decision.id.as_str(), "ADR-014");
        assert_eq!(decision.title, "The domain does not know about transport");
        assert!(decision.why.as_deref().is_some_and(|w| w.contains("HTTP")));
        assert_eq!(
            decision.link.as_deref(),
            Some("docs/adr/014-domain-transport.md")
        );
        assert_eq!(
            decision.status,
            Some(DecisionStatus::Accepted),
            "written out, which is different from unset"
        );
    }

    /// A decision that says nothing about its status is one that holds. The
    /// default has to be this way round: a field defaulting to `proposed`
    /// would make every decision written without it something the doctor is
    /// entitled to complain about.
    #[test]
    fn a_decision_that_says_nothing_is_accepted() {
        let config = parse(r#"{"version":0,"decisions":[{"id":"ADR-1","title":"A wall"}]}"#);

        assert_eq!(
            config.decisions[0].status, None,
            "unset, which is what `accepted` means and what compile resolves it to"
        );
        assert_eq!(config.decisions[0].why, None);
        assert_eq!(config.decisions[0].link, None);
    }

    /// The three the repository's own `docs/DECISIONS.md` declares, so the
    /// config of this tool can describe the ADRs of this tool.
    #[test]
    fn the_three_statuses_are_the_ones_the_adr_format_already_has() {
        for (written, expected) in [
            ("accepted", DecisionStatus::Accepted),
            ("proposed", DecisionStatus::Proposed),
            ("superseded", DecisionStatus::Superseded),
        ] {
            let config = parse(&format!(
                r#"{{"version":0,"decisions":[{{"id":"d","title":"t","status":"{written}"}}]}}"#
            ));
            assert_eq!(config.decisions[0].status, Some(expected), "{written}");
        }

        assert!(
            serde_json::from_str::<Config>(
                r#"{"version":0,"decisions":[{"id":"d","title":"t","status":"rejected"}]}"#
            )
            .is_err(),
            "a status nothing means is refused rather than read as accepted"
        );
    }

    /// A decision needs a title. The id is a reference and the title is the
    /// sentence a denial says out loud, so a decision carrying only an id
    /// would make the hook's message the thing it replaced.
    #[test]
    fn a_decision_without_a_title_is_refused() {
        assert!(
            serde_json::from_str::<Config>(r#"{"version":0,"decisions":[{"id":"d"}]}"#).is_err()
        );
    }

    /// The same protection the rest of the config has: a misspelled key here
    /// silently drops the prose the whole feature exists to carry.
    #[test]
    fn an_unknown_field_in_a_decision_is_refused() {
        assert!(
            serde_json::from_str::<Config>(
                r#"{"version":0,"decisions":[{"id":"d","title":"t","rules":["r"]}]}"#
            )
            .is_err(),
            "`rules` on a decision is the shape issue #100 rejected: the rule points at \
             the decision, not the other way round"
        );
    }

    /// A config that declares none does not gain an empty list, which is what
    /// a merged-config dump and `config explain` depend on.
    #[test]
    fn a_config_with_no_decisions_does_not_grow_the_key() {
        let config = parse(r#"{"version":0}"#);
        assert!(config.decisions.is_empty());

        let json = serde_json::to_string(&config).expect("serialises");
        assert!(!json.contains("decisions"), "{json}");
    }

    /// And one that declares them round-trips, prose and all.
    #[test]
    fn decisions_round_trip() {
        let original = parse(
            r#"{"version":0,"decisions":[
              {"id":"ADR-1","title":"t","why":"w","link":"l","status":"superseded"}]}"#,
        );
        let json = serde_json::to_string(&original).expect("serialises");

        assert_eq!(
            serde_json::from_str::<Config>(&json).expect("parses"),
            original
        );
    }

    /// The lookup every surface downstream makes.
    #[test]
    fn a_decision_is_found_by_id() {
        let config = parse(
            r#"{"version":0,"decisions":[
              {"id":"ADR-1","title":"one"},{"id":"ADR-2","title":"two"}]}"#,
        );

        let wanted = DecisionId::new("ADR-2").expect("valid");
        assert_eq!(
            config
                .decisions
                .iter()
                .find(|d| d.id == wanted)
                .map(|d| d.title.as_str()),
            Some("two")
        );
    }

    /// `Decision` is constructible from the outside, which the tests of every
    /// surface downstream rely on.
    #[test]
    fn a_decision_can_be_built_in_code() {
        let decision = Decision {
            enforcement: None,
            scope: crate::one_or_many::OneOrMany::Many(Vec::new()),
            why_not_enforceable: None,
            id: DecisionId::new("ADR-9").expect("valid"),
            title: "built".to_owned(),
            why: None,
            link: None,
            status: Some(DecisionStatus::Proposed),
            supersedes: crate::one_or_many::OneOrMany::default(),
            alternatives: Vec::new(),
        };
        assert_eq!(decision.status, Some(DecisionStatus::Proposed));
    }
}

#[cfg(test)]
mod governance_tests {
    use super::{Config, Governance, GovernanceMode};
    use archwarden_core::level::Level;

    fn parse(json: &str) -> Config {
        serde_json::from_str(json).expect("should deserialise")
    }

    /// Every config written before this field means `open`, and still does.
    ///
    /// The one that must not regress: a field arriving with a default of
    /// `closed` would turn every existing configuration into thousands of
    /// findings on upgrade, over code nobody touched.
    #[test]
    fn a_config_that_says_nothing_is_open() {
        let config = parse(r#"{"version":0,"rules":[]}"#);

        assert_eq!(config.governance, Governance::default());
        assert_eq!(config.governance.level(), None);
        assert!(config.governance.is_open());
    }

    /// The shorthand is `error`, because a gate that does not fail a build is
    /// a report, and the report is `config coverage`.
    #[test]
    fn the_shorthand_closes_at_error() {
        let config = parse(r#"{"version":0,"governance":"closed","rules":[]}"#);

        assert_eq!(config.governance.level(), Some(Level::Error));
        assert!(!config.governance.is_open());
    }

    /// And the long form exists for the migration the report is for: turn it
    /// on today, see the number in CI, block nobody.
    #[test]
    fn the_long_form_carries_a_level_of_its_own() {
        let config =
            parse(r#"{"version":0,"governance":{"mode":"closed","level":"warning"},"rules":[]}"#);

        assert_eq!(config.governance.level(), Some(Level::Warning));
    }

    /// The long form defaults to the shorthand's level rather than to
    /// something quieter, so writing the mode out longhand never weakens it.
    #[test]
    fn spelling_the_mode_out_does_not_change_what_it_means() {
        let long = parse(r#"{"version":0,"governance":{"mode":"closed"},"rules":[]}"#);
        let short = parse(r#"{"version":0,"governance":"closed","rules":[]}"#);

        assert_eq!(long.governance.level(), short.governance.level());
    }

    /// `open` said out loud is still open, at either spelling, and a level
    /// beside it changes nothing — there is nothing to report.
    #[test]
    fn open_reports_nothing_however_it_is_written() {
        assert_eq!(
            parse(r#"{"version":0,"governance":"open","rules":[]}"#)
                .governance
                .level(),
            None
        );
        assert_eq!(
            parse(r#"{"version":0,"governance":{"mode":"open","level":"error"},"rules":[]}"#)
                .governance
                .level(),
            None,
            "a level on an open architecture is not a quiet gate, it is no gate"
        );
    }

    /// It survives a round trip, which is what `config explain` and the
    /// merged-config output depend on.
    #[test]
    fn it_round_trips() {
        for original in [
            Governance::Mode(GovernanceMode::Closed),
            Governance::Detailed {
                mode: GovernanceMode::Closed,
                level: Level::Warning,
            },
        ] {
            let json = serde_json::to_string(&original).expect("serialises");
            assert_eq!(
                serde_json::from_str::<Governance>(&json).expect("deserialises"),
                original,
                "{json}"
            );
        }
    }
}
