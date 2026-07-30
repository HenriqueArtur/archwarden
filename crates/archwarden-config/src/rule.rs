//! The five rule shapes, as written in `arch.config.json`.
//!
//! These are *wire* types: a glob is a `String` here, and a regex is a
//! `String` too. Lowering them into the compiled types in `archwarden-core` is
//! a separate step, and it is what turns "this config might be valid" into
//! "this config is valid" — a compiled rule cannot exist unless every glob and
//! every regex in it parsed.
//!
//! See `docs/RULES.md` for semantics and `docs/CONFIG.md` for examples.

use archwarden_core::{ids::RuleId, level::Level};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::one_or_many::OneOrMany;

/// A list of glob or regex patterns, written as a string or an array.
pub type Patterns = OneOrMany<String>;

/// One rule, discriminated by `type`.
///
/// `import-boundary` is an ordinary rule like the rest. There is no `graph`
/// key: boundaries go through the same matcher and the same
/// `describe_expectation`, which is what keeps `describe` and `agent-guide` in
/// lockstep with the checker. See decision 14.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Rule {
    /// Which folders may exist, and which filenames.
    Structure(StructureRule),
    /// The filename dictates the exported symbol's name.
    Naming(NamingRule),
    /// Every unit file needs a spec sibling.
    SpecPair(SpecPairRule),
    /// Layer A may not import from layer B.
    ImportBoundary(ImportBoundaryRule),
    /// Files matching a pattern must call a given symbol.
    CallObligation(CallObligationRule),
    /// A file whose whole content is forwarding another module.
    NoPassthrough(NoPassthroughRule),
}

impl Rule {
    /// This rule's identifier.
    #[must_use]
    pub fn id(&self) -> &RuleId {
        match self {
            Self::Structure(r) => &r.id,
            Self::Naming(r) => &r.id,
            Self::SpecPair(r) => &r.id,
            Self::ImportBoundary(r) => &r.id,
            Self::CallObligation(r) => &r.id,
            Self::NoPassthrough(r) => &r.id,
        }
    }

    /// The severity of this rule's findings.
    #[must_use]
    pub fn level(&self) -> Level {
        match self {
            Self::Structure(r) => r.level,
            Self::Naming(r) => r.level,
            Self::SpecPair(r) => r.level,
            Self::ImportBoundary(r) => r.level,
            Self::CallObligation(r) => r.level,
            Self::NoPassthrough(r) => r.level,
        }
    }

    /// The rule's scope patterns.
    ///
    /// Named `roots` on four of the five and `from` on `import-boundary`,
    /// where it reads naturally against `forbid_import_from`. The semantics
    /// are identical, which is why they collapse to one accessor here.
    #[must_use]
    pub fn scope(&self) -> &Patterns {
        match self {
            Self::Structure(r) => &r.roots,
            Self::Naming(r) => &r.roots,
            Self::SpecPair(r) => &r.roots,
            Self::ImportBoundary(r) => &r.from,
            Self::CallObligation(r) => &r.roots,
            Self::NoPassthrough(r) => &r.roots,
        }
    }

    /// The discriminator, as written in the config.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Structure(_) => "structure",
            Self::Naming(_) => "naming",
            Self::SpecPair(_) => "spec-pair",
            Self::ImportBoundary(_) => "import-boundary",
            Self::CallObligation(_) => "call-obligation",
            Self::NoPassthrough(_) => "no-passthrough",
        }
    }
}

/// Which folders may exist under a scope, and which filenames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StructureRule {
    /// Stable identifier, unique across the config and its presets.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Subdirectory names that are permitted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_subfolders: Vec<String>,
    /// Subdirectory names that are permitted but reported as warnings,
    /// whatever `level` says. Naming a folder is more specific than the rule's
    /// blanket severity, and the more specific declaration wins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warn_subfolders: Vec<String>,
    /// Subdirectories that carry the same structural contract, recursively.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recurse_into: Vec<String>,
    /// Regexes every direct child file's name must match at least one of.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filename_patterns: Vec<String>,
}

/// The filename dictates the exported symbol's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NamingRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Regex over the filename, with a named capture group.
    pub file_pattern: String,
    /// The export the file must carry.
    pub must_export: MustExport,
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
    /// A signature shown by `scaffold`. **Never verified** — constraining the
    /// type of an export is type checking, which is `tsc`'s job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hint: Option<String>,
}

/// Every unit file under the scope needs a spec sibling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpecPairRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Subdirectories subject to the rule. `["."]` means the scope itself.
    pub subfolders: Patterns,
    /// What makes a filename a spec: `spec`, `test`, or both.
    ///
    /// A marker, not a whole suffix: the extension is taken from the source
    /// file, so `Component.tsx` wants `Component.spec.tsx` without anyone
    /// saying so. The default accepts both markers, which is what vitest and
    /// jest do, so the common project needs no configuration here at all.
    #[serde(default = "default_spec_markers")]
    pub spec_markers: Patterns,
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
}

fn default_spec_markers() -> Patterns {
    Patterns::Many(vec!["spec".to_owned(), "test".to_owned()])
}

/// Layer A may not import from layer B, or must import from layer C.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportBoundaryRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Directory globs selecting the importer. Same semantics as `roots`.
    pub from: Patterns,
    /// Globs matched against the *resolved* import path. Matching means the
    /// import is illegal.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub forbid_import_from: Patterns,
    /// Globs matched against the resolved import path. If none of the file's
    /// imports match, the file is illegal.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub must_import_from: Patterns,
    /// Exceptions, also matched against the resolved path.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub except: Patterns,
    /// Whether `import type` and inline `type` marks count.
    #[serde(default = "default_true")]
    pub include_type_only: bool,
}

fn default_true() -> bool {
    true
}

/// Files matching a pattern must call a given symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CallObligationRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Regex over the filename.
    pub file_pattern: String,
    /// The call the file must contain.
    pub must_call: MustCall,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A misspelled field is refused rather than ignored.
    ///
    /// Found the hard way in M7d: a config saying `allow` where the field is
    /// `allowed_subfolders` compiled to a `structure` rule that constrained
    /// nothing, `config validate` called it valid, and `check` reported a
    /// clean repository. A rule that silently enforces nothing is the worst
    /// possible failure for a linter -- it is indistinguishable from a rule
    /// that passes.
    #[test]
    fn a_misspelled_field_is_refused() {
        let error = serde_json::from_str::<Rule>(
            r#"{"type":"structure","id":"shape","level":"error",
                "roots":"src/*","allow":["types"]}"#,
        )
        .expect_err("`allow` is not a field");

        assert!(error.to_string().contains("allow"), "{error}");
    }

    /// Every rule kind refuses one, not just the first that happened to be
    /// tested. A gap here is a rule kind that can be silently disabled.
    #[test]
    fn every_rule_kind_refuses_an_unknown_field() {
        let cases = [
            r#"{"type":"structure","id":"a","level":"error","roots":"src/*","nope":1}"#,
            r#"{"type":"naming","id":"a","level":"error","roots":"src/*",
                "file_pattern":"^x$","must_export":{"name":"X","kind":"any"},"nope":1}"#,
            r#"{"type":"spec-pair","id":"a","level":"error","roots":"src/*",
                "subfolders":".","spec_markers":"spec","nope":1}"#,
            r#"{"type":"import-boundary","id":"a","level":"error","from":"src/**","nope":1}"#,
            r#"{"type":"call-obligation","id":"a","level":"error","roots":"src/*",
                "file_pattern":"^x$","must_call":{"symbol":"S","imported_from":"m"},"nope":1}"#,
        ];

        for case in cases {
            assert!(
                serde_json::from_str::<Rule>(case).is_err(),
                "accepted an unknown field: {case}"
            );
        }
    }

    /// The nested objects too, which is where a typo is easiest to make and
    /// hardest to notice.
    #[test]
    fn a_nested_object_refuses_an_unknown_field() {
        assert!(
            serde_json::from_str::<Rule>(
                r#"{"type":"naming","id":"a","level":"error","roots":"src/*",
                    "file_pattern":"^x$","must_export":{"name":"X","kind":"any","hint":"..."}}"#
            )
            .is_err(),
            "`hint` is not a field; `signature_hint` is"
        );
        assert!(
            serde_json::from_str::<Rule>(
                r#"{"type":"call-obligation","id":"a","level":"error","roots":"src/*",
                    "file_pattern":"^x$","must_call":{"symbol":"S","from":"m"}}"#
            )
            .is_err(),
            "`from` is not a field; `imported_from` is"
        );
    }

    /// And a correctly spelled config still parses, which is the half that
    /// would break if the attribute were put somewhere it does not belong.
    #[test]
    fn a_well_spelled_rule_still_parses() {
        let rule: Rule = serde_json::from_str(
            r#"{"type":"structure","id":"shape","level":"error",
                "roots":"src/*","allowed_subfolders":["types"]}"#,
        )
        .expect("parses");

        assert_eq!(rule.id().as_str(), "shape");
    }

    fn parse(json: &str) -> Rule {
        serde_json::from_str(json).expect("should deserialise")
    }

    /// Verbatim from docs/CONFIG.md. If this stops parsing, either the code or
    /// the documented example is wrong, and both matter.
    #[test]
    fn the_documented_structure_example_parses() {
        let rule = parse(
            r#"{
              "type": "structure",
              "id": "domain-entity-shape",
              "level": "error",
              "roots": ["packages/domain/src/*"],
              "allowed_subfolders": [
                "types", "calcs", "actions", "services",
                "mocks", "repositories", "const", "variants"
              ],
              "warn_subfolders": ["shared", "adapters"],
              "recurse_into": ["variants"]
            }"#,
        );

        let Rule::Structure(structure) = &rule else {
            panic!("expected a structure rule, got {}", rule.type_name());
        };
        assert_eq!(rule.id().as_str(), "domain-entity-shape");
        assert_eq!(rule.level(), Level::Error);
        assert_eq!(structure.allowed_subfolders.len(), 8);
        assert_eq!(structure.warn_subfolders, ["shared", "adapters"]);
        assert_eq!(structure.recurse_into, ["variants"]);
        assert!(structure.filename_patterns.is_empty());
    }

    /// Also verbatim from docs/CONFIG.md: the filename sub-mode of `structure`.
    #[test]
    fn the_documented_filename_example_parses() {
        let rule = parse(
            r#"{
              "type": "structure",
              "id": "api-route-filenames",
              "level": "error",
              "roots": ["apps/app/src/app/api/**"],
              "filename_patterns": [
                "^route\\.ts$",
                "^route\\.(get|post|put|patch|delete|options)\\.ts$",
                "^DOC\\.md$"
              ]
            }"#,
        );

        let Rule::Structure(structure) = &rule else {
            panic!("expected a structure rule");
        };
        assert_eq!(structure.filename_patterns.len(), 3);
        assert!(structure.allowed_subfolders.is_empty());
    }

    /// The `naming` example, with the scope corrected to `use-cases/*` when
    /// decision 4 fixed what a scope glob selects.
    #[test]
    fn the_documented_naming_example_parses() {
        let rule = parse(
            r#"{
              "type": "naming",
              "id": "usecase-factory-name",
              "level": "error",
              "roots": ["packages/application/src/use-cases/*"],
              "file_pattern": "^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
              "must_export": {
                "kind": "function",
                "name": "{{pascal(name)}}",
                "signature_hint": "(deps: {{pascal(name)}}Deps)"
              }
            }"#,
        );

        let Rule::Naming(naming) = &rule else {
            panic!("expected a naming rule");
        };
        assert_eq!(naming.must_export.name, "{{pascal(name)}}");
        assert_eq!(naming.must_export.kind.as_slice(), ["function"]);
        assert!(naming.must_export.signature_hint.is_some());
    }

    /// `kind` takes the same one-or-many treatment as every other list, so
    /// `["function", "arrow"]` -- "callable, either form" -- is expressible.
    #[test]
    fn an_export_kind_may_be_a_list() {
        let rule = parse(
            r#"{
              "type": "naming", "id": "n", "level": "error", "roots": "src/*",
              "file_pattern": "^(?<name>.+)\\.ts$",
              "must_export": { "kind": ["function", "arrow"], "name": "{{pascal(name)}}" }
            }"#,
        );
        let Rule::Naming(naming) = &rule else {
            panic!("expected a naming rule");
        };
        assert_eq!(naming.must_export.kind.as_slice(), ["function", "arrow"]);
        assert_eq!(naming.must_export.signature_hint, None);
    }

    #[test]
    fn the_documented_spec_pair_example_parses() {
        let rule = parse(
            r#"{
              "type": "spec-pair",
              "id": "domain-calcs-need-spec",
              "level": "error",
              "roots": ["packages/domain/src/*"],
              "subfolders": ["calcs", "services", "adapters"],
              "ignore_files": ["packages/domain/src/**/*.types.ts"]
            }"#,
        );

        let Rule::SpecPair(spec) = &rule else {
            panic!("expected a spec-pair rule");
        };
        assert_eq!(spec.subfolders.len(), 3);
        assert_eq!(spec.spec_markers.as_slice(), ["spec", "test"]);
        assert!(!spec.require_non_empty_spec, "defaults to off");
    }

    /// A spec-pair rule that omits everything optional still parses, and the
    /// defaults are the ones docs/RULES.md promises.
    #[test]
    fn spec_pair_defaults_match_the_documentation() {
        let rule = parse(
            r#"{
              "type": "spec-pair", "id": "s", "level": "error",
              "roots": "src/**", "subfolders": "."
            }"#,
        );
        let Rule::SpecPair(spec) = &rule else {
            panic!("expected a spec-pair rule");
        };
        assert_eq!(
            spec.spec_markers.as_slice(),
            ["spec", "test"],
            "both markers by default, as vitest and jest accept"
        );
        assert!(!spec.require_non_empty_spec);
        assert!(spec.ignore_files.is_empty());
    }

    /// Decision 14: a boundary is an ordinary rule with `type`, and its scope
    /// field is called `from`.
    #[test]
    fn the_documented_import_boundary_example_parses() {
        let rule = parse(
            r#"{
              "type": "import-boundary",
              "id": "ui-forbids-domain-direct",
              "level": "error",
              "from": "apps/**/src/**",
              "forbid_import_from": ["packages/domain/**"],
              "except": ["packages/domain/src/*/types/**"]
            }"#,
        );

        let Rule::ImportBoundary(boundary) = &rule else {
            panic!("expected an import-boundary rule");
        };
        assert_eq!(rule.type_name(), "import-boundary");
        assert_eq!(boundary.except.len(), 1);
        assert!(boundary.must_import_from.is_empty());
        assert!(
            boundary.include_type_only,
            "docs/RULES.md says type-only imports count unless opted out"
        );
    }

    /// `from` and `roots` are the same thing under two names, so one accessor
    /// serves both and the matcher never has to care which rule it holds.
    #[test]
    fn scope_reads_from_whichever_field_the_rule_uses() {
        let boundary = parse(
            r#"{"type":"import-boundary","id":"b","level":"error","from":"packages/domain/**"}"#,
        );
        let structure =
            parse(r#"{"type":"structure","id":"s","level":"error","roots":"packages/domain/**"}"#);

        assert_eq!(boundary.scope().as_slice(), ["packages/domain/**"]);
        assert_eq!(boundary.scope(), structure.scope());
    }

    #[test]
    fn the_documented_call_obligation_example_parses() {
        let rule = parse(
            r#"{
              "type": "call-obligation",
              "id": "non-get-routes-must-audit",
              "level": "error",
              "roots": ["apps/app/src/app/api/**"],
              "file_pattern": "^route\\.(post|put|patch|delete)\\.ts$",
              "must_call": {
                "symbol": "Event.save",
                "imported_from": "@flowmaatik/domain/event"
              }
            }"#,
        );

        let Rule::CallObligation(call) = &rule else {
            panic!("expected a call-obligation rule");
        };
        assert_eq!(call.must_call.symbol, "Event.save");
        assert_eq!(call.must_call.imported_from, "@flowmaatik/domain/event");
    }

    /// Every rule answers the same three questions whatever its type, which is
    /// what lets the matcher hold a heterogeneous list.
    #[test]
    fn every_rule_type_answers_id_level_and_scope() {
        let rules = [
            parse(r#"{"type":"structure","id":"a","level":"error","roots":"x/*"}"#),
            parse(
                r#"{"type":"naming","id":"b","level":"warning","roots":"x/*",
                    "file_pattern":"^(?<name>.+)$","must_export":{"kind":"any","name":"N"}}"#,
            ),
            parse(
                r#"{"type":"spec-pair","id":"c","level":"error","roots":"x/*","subfolders":"."}"#,
            ),
            parse(r#"{"type":"import-boundary","id":"d","level":"error","from":"x/*"}"#),
            parse(
                r#"{"type":"call-obligation","id":"e","level":"error","roots":"x/*",
                    "file_pattern":"^f$","must_call":{"symbol":"s","imported_from":"m"}}"#,
            ),
        ];

        let ids: Vec<_> = rules.iter().map(|r| r.id().as_str()).collect();
        assert_eq!(ids, ["a", "b", "c", "d", "e"]);

        let types: Vec<_> = rules.iter().map(Rule::type_name).collect();
        assert_eq!(
            types,
            [
                "structure",
                "naming",
                "spec-pair",
                "import-boundary",
                "call-obligation"
            ]
        );

        assert_eq!(rules[1].level(), Level::Warning);
        for rule in &rules {
            assert_eq!(rule.scope().as_slice(), ["x/*"]);
        }
    }

    /// An unknown discriminator names the valid ones. `graph` used to be a
    /// separate config key and is not any more, so somebody will try it.
    #[test]
    fn an_unknown_rule_type_is_rejected() {
        let err = serde_json::from_str::<Rule>(r#"{"type":"graph","id":"g","level":"error"}"#)
            .expect_err("should fail");
        let message = err.to_string();
        assert!(message.contains("structure"), "{message}");
        assert!(message.contains("import-boundary"), "{message}");
    }

    /// Severity is never inferred. Decision 1 puts the burden on the rule
    /// author to say up front whether a rule is a gate or a signpost.
    #[test]
    fn level_is_required() {
        assert!(
            serde_json::from_str::<Rule>(r#"{"type":"structure","id":"a","roots":"x/*"}"#).is_err()
        );
    }

    /// Ids are validated on the way in, so a bad one fails at load time rather
    /// than surfacing much later in a report.
    #[test]
    fn an_invalid_id_is_rejected_while_parsing() {
        let err = serde_json::from_str::<Rule>(
            r#"{"type":"structure","id":"bad id","level":"error","roots":"x"}"#,
        )
        .expect_err("should fail");
        assert!(err.to_string().contains("rule id"), "{err}");
    }

    /// Rules round-trip, which is what `agent-guide --format json` and the
    /// merged-config dump depend on.
    #[test]
    fn rules_round_trip_through_json() {
        let original = parse(
            r#"{"type":"import-boundary","id":"b","level":"error",
                "from":["a/**"],"forbid_import_from":["b/**"],"include_type_only":false}"#,
        );
        let json = serde_json::to_string(&original).expect("serialises");
        assert_eq!(
            serde_json::from_str::<Rule>(&json).expect("deserialises"),
            original
        );
        assert!(json.contains(r#""type":"import-boundary""#), "{json}");
    }
}

/// Which shapes of pure forwarding are refused, and where.
///
/// A file that only forwards another module is an indirection wearing the
/// name of a layer. The three shapes are all a way of holding a name and
/// adding nothing to it; see `docs/RULES.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoPassthroughRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Which shapes count. Defaults to all three.
    #[serde(default = "default_forms")]
    pub forms: Vec<PassthroughForm>,
    /// Files exempted, as globs.
    ///
    /// A legitimate re-export exists — a package's public API — and a rule
    /// without a way to say so is noise in the first repository that enables
    /// it. `allow_package_entrypoints` covers the common case without anyone
    /// writing a glob.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub except: Patterns,
    /// Whether a file reachable through a `package.json` `exports` entry is
    /// exempt. Default `true`.
    ///
    /// That file *is* the package's public API, and forwarding is what a
    /// public API is for. Without this the rule reports a package's entire
    /// surface the moment it is switched on.
    #[serde(default = "default_true")]
    pub allow_package_entrypoints: bool,
    /// Whether a file that forwards *some* of its exports and declares others
    /// is allowed. Default `true`.
    ///
    /// Set to `false` to hear about the shape that hides best: a file
    /// re-exporting six names from another module while declaring two of its
    /// own reads as a real module, and six of its eight exports are still an
    /// indirection its importers could skip.
    #[serde(default = "default_true")]
    pub allow_partial: bool,
}

/// One shape of pure forwarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PassthroughForm {
    /// `export { A } from './x'`, or an import followed by an export of the
    /// same name. A barrel file is this and nothing else.
    Reexport,
    /// `export const A = B` or `export type A = B`, where `B` was imported.
    Alias,
    /// A function whose whole body is `return g(<its own parameters>)`.
    Wrapper,
}

fn default_forms() -> Vec<PassthroughForm> {
    vec![
        PassthroughForm::Reexport,
        PassthroughForm::Alias,
        PassthroughForm::Wrapper,
    ]
}
