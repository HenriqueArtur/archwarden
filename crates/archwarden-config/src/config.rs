//! The top-level shape of `arch.config.json`.

use archwarden_core::ids::{ModuleId, RuleId};
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

    /// The `_`-prefix escape hatch.
    #[serde(default)]
    pub skip_dirs: SkipDirs,

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
