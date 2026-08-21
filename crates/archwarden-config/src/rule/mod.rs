//! The five rule shapes, as written in `arch.config.json`.
//!
//! These are *wire* types: a glob is a `String` here, and a regex is a
//! `String` too. Lowering them into the compiled types in `archwarden-core` is
//! a separate step, and it is what turns "this config might be valid" into
//! "this config is valid" — a compiled rule cannot exist unless every glob and
//! every regex in it parsed.
//!
//! See `docs/RULES.md` for semantics and `docs/CONFIG.md` for examples.

use archwarden_core::{
    ids::{DecisionId, RuleId},
    level::Level,
};
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
#[allow(
    clippy::large_enum_variant,
    reason = "456 bytes against 248 for the next largest, and the difference is \
              `import-boundary`, which has the most fields because it is the \
              rule with the most directions. This is a wire type: deserialised \
              once per run and lowered into `CompiledRule` immediately, held in \
              a Vec of a few dozen. A hundred-rule config is 45 KB. Boxing it \
              would buy that back and cost an indirection in every match and a \
              `Box::new` at every construction site, for memory nothing is \
              short of"
)]
pub enum Rule {
    /// Which folders may exist, and which filenames.
    Structure(StructureRule),
    /// The filename dictates the exported symbol's name.
    Naming(NamingRule),
    /// Every unit file needs a spec sibling.
    SpecPair(SpecPairRule),
    /// Layer A may not import from layer B.
    ImportBoundary(ImportBoundaryRule),
    /// No file in scope may sit on an import loop.
    ImportCycle(ImportCycleRule),
    /// Files matching a pattern must call a given symbol.
    CallObligation(CallObligationRule),
    /// Every name a call asks for is declared somewhere.
    CallMatchesExport(CallMatchesExportRule),
    /// A file whose whole content is forwarding another module.
    NoPassthrough(NoPassthroughRule),
    /// These files must exist in each governed directory.
    Presence(PresenceRule),
    /// A file of one kind must have a companion of another.
    Pair(PairRule),
    /// A document's frontmatter must carry these keys.
    Frontmatter(FrontmatterRule),
    /// What a file exposes, without saying anything about its name.
    ExportShape(ExportShapeRule),
    /// A directory that has stopped growing.
    Frozen(FrozenRule),
    /// A counterpart in a parallel tree.
    Mirror(MirrorRule),
    /// A capability only these files may reach.
    Chokepoint(ChokepointRule),
    /// What a file's header declares about itself.
    Metadata(MetadataRule),
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
            Self::ImportCycle(r) => &r.id,
            Self::CallObligation(r) => &r.id,
            Self::CallMatchesExport(r) => &r.id,
            Self::NoPassthrough(r) => &r.id,
            Self::Presence(r) => &r.id,
            Self::Pair(r) => &r.id,
            Self::Frontmatter(r) => &r.id,
            Self::ExportShape(r) => &r.id,
            Self::Frozen(r) => &r.id,
            Self::Mirror(r) => &r.id,
            Self::Chokepoint(r) => &r.id,
            Self::Metadata(r) => &r.id,
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
            Self::ImportCycle(r) => r.level,
            Self::CallObligation(r) => r.level,
            Self::CallMatchesExport(r) => r.level,
            Self::NoPassthrough(r) => r.level,
            Self::Presence(r) => r.level,
            Self::Pair(r) => r.level,
            Self::Frontmatter(r) => r.level,
            Self::ExportShape(r) => r.level,
            Self::Frozen(r) => r.level,
            Self::Mirror(r) => r.level,
            Self::Chokepoint(r) => r.level,
            Self::Metadata(r) => r.level,
        }
    }

    /// Why this rule exists, when its author said.
    #[must_use]
    pub fn why(&self) -> Option<&str> {
        match self {
            Self::Structure(r) => r.why.as_deref(),
            Self::Naming(r) => r.why.as_deref(),
            Self::SpecPair(r) => r.why.as_deref(),
            Self::ImportBoundary(r) => r.why.as_deref(),
            Self::ImportCycle(r) => r.why.as_deref(),
            Self::CallObligation(r) => r.why.as_deref(),
            Self::CallMatchesExport(r) => r.why.as_deref(),
            Self::NoPassthrough(r) => r.why.as_deref(),
            Self::Presence(r) => r.why.as_deref(),
            Self::Pair(r) => r.why.as_deref(),
            Self::Frontmatter(r) => r.why.as_deref(),
            Self::ExportShape(r) => r.why.as_deref(),
            Self::Frozen(r) => r.why.as_deref(),
            Self::Mirror(r) => r.why.as_deref(),
            Self::Chokepoint(r) => r.why.as_deref(),
            Self::Metadata(r) => r.why.as_deref(),
        }
    }

    /// The decision this rule implements, when it names one.
    ///
    /// Every kind has the field, and that is why issue #100 shipped first of
    /// its milestone: a kind landing after it carries the field from birth,
    /// where four kinds landing before it would each have been a retrofit.
    #[must_use]
    pub fn decision(&self) -> Option<&DecisionId> {
        match self {
            Self::Structure(r) => r.decision.as_ref(),
            Self::Naming(r) => r.decision.as_ref(),
            Self::SpecPair(r) => r.decision.as_ref(),
            Self::ImportBoundary(r) => r.decision.as_ref(),
            Self::ImportCycle(r) => r.decision.as_ref(),
            Self::CallObligation(r) => r.decision.as_ref(),
            Self::CallMatchesExport(r) => r.decision.as_ref(),
            Self::NoPassthrough(r) => r.decision.as_ref(),
            Self::Presence(r) => r.decision.as_ref(),
            Self::Pair(r) => r.decision.as_ref(),
            Self::Frontmatter(r) => r.decision.as_ref(),
            Self::ExportShape(r) => r.decision.as_ref(),
            Self::Frozen(r) => r.decision.as_ref(),
            Self::Mirror(r) => r.decision.as_ref(),
            Self::Chokepoint(r) => r.decision.as_ref(),
            Self::Metadata(r) => r.decision.as_ref(),
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
            Self::ImportCycle(r) => &r.roots,
            Self::CallObligation(r) => &r.roots,
            Self::CallMatchesExport(r) => &r.roots,
            Self::NoPassthrough(r) => &r.roots,
            Self::Presence(r) => &r.roots,
            Self::Pair(r) => &r.roots,
            Self::Frontmatter(r) => &r.roots,
            Self::ExportShape(r) => &r.roots,
            Self::Frozen(r) => &r.roots,
            Self::Mirror(r) => &r.roots,
            Self::Chokepoint(r) => &r.roots,
            Self::Metadata(r) => &r.roots,
        }
    }

    /// The import globs that narrow this rule's population, if any.
    ///
    /// Empty for every rule that does not ask, which is the ordinary case and
    /// the one that must stay free: a rule with nothing here never causes an
    /// import to be resolved. Decision 25.
    ///
    /// `import-boundary` has none and never will — it already chooses its
    /// importers with `from`, `from_module` and `from_kind`, and a second way
    /// to say the same thing is a second thing to get wrong.
    /// The directives that put a file in this rule's population, and the ones
    /// that keep it out.
    ///
    /// Only `import-boundary` asks. Issue #144 says why: its three motivating
    /// rules -- a `"use server"` module not imported by a `"use client"` one,
    /// a client component not reaching the database, a directory whose files
    /// agree on which side they are on -- are all import-boundary questions
    /// with one extra predicate. Another kind can gain the field the day it
    /// has a sentence that needs it.
    #[must_use]
    pub fn when_declaring(&self) -> (&Patterns, &Patterns) {
        const NONE: &Patterns = &OneOrMany::Many(Vec::new());
        match self {
            Self::ImportBoundary(r) => (&r.when_declaring, &r.when_not_declaring),
            _ => (NONE, NONE),
        }
    }

    /// Path globs that put a file in this rule's population.
    ///
    /// See [`ImportCycleRule::when_importing`]. Matched against where an
    /// import *lands*, so a rule carrying one costs a resolution pass over the
    /// files its scope reaches -- unlike [`when_declaring`](Self::when_declaring),
    /// which needs only the file parsed.
    #[must_use]
    pub fn when_importing(&self) -> &Patterns {
        // A rule that never asks. A `const` rather than a `Default::default()`
        // so the borrow outlives the match without a field to hold it.
        const NONE: &Patterns = &OneOrMany::Many(Vec::new());
        match self {
            Self::Structure(r) => &r.when_importing,
            Self::Naming(r) => &r.when_importing,
            Self::SpecPair(r) => &r.when_importing,
            Self::ImportBoundary(_) => NONE,
            Self::ImportCycle(r) => &r.when_importing,
            Self::CallObligation(r) => &r.when_importing,
            Self::CallMatchesExport(r) => &r.when_importing,
            Self::NoPassthrough(r) => &r.when_importing,
            Self::Presence(r) => &r.when_importing,
            Self::Pair(r) => &r.when_importing,
            Self::Frontmatter(r) => &r.when_importing,
            Self::ExportShape(r) => &r.when_importing,
            Self::Frozen(r) => &r.when_importing,
            Self::Mirror(r) => &r.when_importing,
            Self::Chokepoint(r) => &r.when_importing,
            Self::Metadata(r) => &r.when_importing,
        }
    }

    /// The package names that narrow this rule's population, if any.
    #[must_use]
    pub fn when_importing_packages(&self) -> &[String] {
        match self {
            Self::Structure(r) => &r.when_importing_packages,
            Self::Naming(r) => &r.when_importing_packages,
            Self::SpecPair(r) => &r.when_importing_packages,
            Self::ImportBoundary(_) => &[],
            Self::ImportCycle(r) => &r.when_importing_packages,
            Self::CallObligation(r) => &r.when_importing_packages,
            Self::CallMatchesExport(r) => &r.when_importing_packages,
            Self::NoPassthrough(r) => &r.when_importing_packages,
            Self::Presence(r) => &r.when_importing_packages,
            Self::Pair(r) => &r.when_importing_packages,
            Self::Frontmatter(r) => &r.when_importing_packages,
            Self::ExportShape(r) => &r.when_importing_packages,
            Self::Frozen(r) => &r.when_importing_packages,
            Self::Mirror(r) => &r.when_importing_packages,
            Self::Chokepoint(r) => &r.when_importing_packages,
            Self::Metadata(r) => &r.when_importing_packages,
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
            Self::ImportCycle(_) => "import-cycle",
            Self::CallObligation(_) => "call-obligation",
            Self::CallMatchesExport(_) => "call-matches-export",
            Self::NoPassthrough(_) => "no-passthrough",
            Self::Presence(_) => "presence",
            Self::Pair(_) => "pair",
            Self::Frontmatter(_) => "frontmatter",
            Self::ExportShape(_) => "export-shape",
            Self::Frozen(_) => "frozen",
            Self::Mirror(_) => "mirror",
            Self::Metadata(_) => "metadata",
            Self::Chokepoint(_) => "chokepoint",
        }
    }
}

mod call_matches_export;
mod call_obligation;
mod frontmatter;
mod import_boundary;
mod naming;
mod pair;
mod presence;
mod spec_pair;
mod structure;

pub use call_matches_export::CallMatchesExportRule;
pub use call_obligation::{CallObligationRule, MustCall, WithOptions};
pub use frontmatter::FrontmatterRule;
pub use import_boundary::ImportBoundaryRule;

use import_boundary::default_true;
pub use naming::{MustExport, NamingRule};
pub use pair::PairRule;
pub use presence::PresenceRule;
pub use spec_pair::SpecPairRule;
pub use structure::StructureRule;

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
        assert_eq!(
            structure
                .allowed_subfolders
                .as_ref()
                .expect("names a list")
                .len(),
            8
        );
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
        // Absent, not empty: this rule constrains filenames and says nothing
        // about the directories beside them.
        assert_eq!(structure.allowed_subfolders, None);
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

    /// Issue #44. The frontmatter is not documentation — it is the schema
    /// three scripts and one index page depend on, and nothing type-checks a
    /// markdown file.
    #[test]
    fn a_frontmatter_rule_names_keys_a_vocabulary_and_an_agreement() {
        let rule = parse(
            r#"{"type":"frontmatter","id":"projeto-frontmatter","level":"error",
                "roots":["projetos/*"],
                "file_pattern":"^projeto\\.md$",
                "require":["id","nivel","componentes"],
                "one_of":{"nivel":["1","2","3"]},
                "equals":{"id":"{{raw(dirname)}}"}}"#,
        );

        let Rule::Frontmatter(front) = &rule else {
            panic!("expected a frontmatter rule, got {}", rule.type_name());
        };
        assert_eq!(front.require.as_slice(), ["id", "nivel", "componentes"]);
        assert_eq!(front.one_of["nivel"].as_slice(), ["1", "2", "3"]);
        assert_eq!(front.equals["id"], "{{raw(dirname)}}");
        assert_eq!(rule.type_name(), "frontmatter");
    }

    /// Issue #45. `spec-pair` is the rule for this and its baked-in ignores
    /// exclude every file involved by construction; and `projeto.md` →
    /// `notas.md` is not a `<stem>.<marker>.<ext>` relationship at all, so
    /// nothing about that rule would have helped.
    #[test]
    fn a_pair_rule_names_its_companion_literally() {
        let rule = parse(
            r#"{"type":"pair","id":"licao-tem-notas","level":"error",
                "roots":["projetos/*"],
                "file_pattern":"^projeto\\.md$",
                "must_exist":"notas.md"}"#,
        );

        let Rule::Pair(pair) = &rule else {
            panic!("expected a pair rule, got {}", rule.type_name());
        };
        assert_eq!(pair.file_pattern, r"^projeto\.md$");
        assert_eq!(pair.must_exist, "notas.md");
        assert_eq!(rule.type_name(), "pair");
    }

    /// The other half of the issue: the companion may sit outside the
    /// directory. `sketch/semaforo.ino` needs the `projeto.md` one level up,
    /// and there is no directory-scoped rule that can say that.
    #[test]
    fn a_companion_may_leave_the_directory() {
        let rule = parse(
            r#"{"type":"pair","id":"sketch-tem-licao","level":"error",
                "roots":["projetos/*/sketch"],
                "file_pattern":"\\.ino$",
                "must_exist":"../projeto.md"}"#,
        );

        let Rule::Pair(pair) = &rule else {
            panic!("expected a pair rule");
        };
        assert_eq!(pair.must_exist, "../projeto.md");
    }

    /// Issue #42. A unit of work is incomplete until its companion files are
    /// there, and `filename_patterns` is a whitelist of what *may* exist —
    /// satisfied by an empty directory, which is the state this rule is about.
    #[test]
    fn a_presence_rule_lists_what_must_exist() {
        let rule = parse(
            r#"{"type":"presence","id":"licao-completa","level":"error",
                "roots":["projetos/*"],
                "require":["projeto.md","exercicios.md","notas.md"],
                "require_any":["\\.ino$"]}"#,
        );

        let Rule::Presence(presence) = &rule else {
            panic!("expected a presence rule, got {}", rule.type_name());
        };
        assert_eq!(
            presence.require.as_slice(),
            ["projeto.md", "exercicios.md", "notas.md"]
        );
        assert_eq!(presence.require_any.as_slice(), [r"\.ino$"]);
        assert_eq!(rule.type_name(), "presence");
    }

    /// Issue #46. Decision 5 chose JSON over YAML and JSON5, so a config has
    /// no comments and the reason a rule exists has nowhere to live. It ends
    /// up in a commit message or a wiki, neither of which is in front of
    /// anybody at the moment the rule fires.
    #[test]
    fn a_rule_may_say_why_it_exists() {
        let rule = parse(
            r#"{"type":"import-boundary","id":"domain-forbids-app","level":"error",
                "why":"domain is published as its own package and the app is not",
                "from":["packages/domain/**"],
                "forbid_import_from":["packages/app/**"]}"#,
        );

        assert_eq!(
            rule.why(),
            Some("domain is published as its own package and the app is not")
        );
    }

    /// Every kind, because a reason is not a property of one of them. A rule
    /// that could not say why would be the one nobody could argue with.
    #[test]
    fn every_rule_kind_can_say_why() {
        let cases = [
            r#"{"type":"structure","id":"r","level":"error","roots":"src","why":"w",
                "allowed_subfolders":[]}"#,
            r#"{"type":"naming","id":"r","level":"error","roots":"src","why":"w",
                "file_pattern":"^(?<n>.+)$","must_export":{"kind":"any","name":"{{pascal(n)}}"}}"#,
            r#"{"type":"spec-pair","id":"r","level":"error","roots":"src","why":"w",
                "subfolders":"."}"#,
            r#"{"type":"import-boundary","id":"r","level":"error","from":"src","why":"w",
                "forbid_import_from":["x/**"]}"#,
            r#"{"type":"call-obligation","id":"r","level":"error","roots":"src","why":"w",
                "file_pattern":"^x$","must_call":{"symbol":"s","imported_from":"m"}}"#,
            r#"{"type":"no-passthrough","id":"r","level":"error","roots":"src","why":"w"}"#,
        ];

        for json in cases {
            assert_eq!(parse(json).why(), Some("w"), "{json}");
        }
    }

    /// Every kind of every kind, and this list is deliberately the complete
    /// ten rather than the six above.
    ///
    /// Issue #100 was scheduled first of its milestone for exactly this: every
    /// rule kind that lands after it carries `decision` from birth, and one
    /// that shipped without it would be a retrofit. A kind added to `Rule` and
    /// not to this list fails `every_kind_carries_a_decision_field`, which
    /// counts the arms.
    #[test]
    fn every_rule_kind_can_name_the_decision_it_implements() {
        let cases = [
            r#"{"type":"structure","id":"r","level":"error","roots":"src","decision":"ADR-014",
                "allowed_subfolders":[]}"#,
            r#"{"type":"naming","id":"r","level":"error","roots":"src","decision":"ADR-014",
                "file_pattern":"^(?<n>.+)$","must_export":{"kind":"any","name":"{{pascal(n)}}"}}"#,
            r#"{"type":"spec-pair","id":"r","level":"error","roots":"src","decision":"ADR-014",
                "subfolders":"."}"#,
            r#"{"type":"import-boundary","id":"r","level":"error","from":"src","decision":"ADR-014",
                "forbid_import_from":["x/**"]}"#,
            r#"{"type":"import-cycle","id":"r","level":"error","roots":"src","decision":"ADR-014"}"#,
            r#"{"type":"call-obligation","id":"r","level":"error","roots":"src","decision":"ADR-014",
                "file_pattern":"^x$","must_call":{"symbol":"s","imported_from":"m"}}"#,
            r#"{"type":"no-passthrough","id":"r","level":"error","roots":"src","decision":"ADR-014"}"#,
            r#"{"type":"presence","id":"r","level":"error","roots":"src/*","decision":"ADR-014",
                "require":["x.md"]}"#,
            r#"{"type":"pair","id":"r","level":"error","roots":"src/*","decision":"ADR-014",
                "file_pattern":"^a\\.md$","must_exist":"b.md"}"#,
            r#"{"type":"frontmatter","id":"r","level":"error","roots":"src/*","decision":"ADR-014",
                "file_pattern":"^a\\.md$","require":["id"]}"#,
            r#"{"type":"export-shape","id":"r","level":"error","roots":"src","decision":"ADR-014",
                "forbid_default":true}"#,
        ];

        let mut kinds = std::collections::BTreeSet::new();
        for json in cases {
            let rule = parse(json);
            assert_eq!(
                rule.decision().map(DecisionId::as_str),
                Some("ADR-014"),
                "{json}"
            );
            kinds.insert(rule.type_name());
        }

        // The set, not the count of the list: a case duplicated while editing
        // would otherwise let a kind drop off the list unnoticed.
        assert_eq!(
            kinds.len(),
            11,
            "these are meant to be every kind archwarden has, one each: {kinds:?}"
        );
    }

    /// Issue #101, and the sketch from the issue verbatim. Three claims in one
    /// kind, none of which mentions a filename — which is the whole point,
    /// because saying any of them through `naming` meant inventing a naming
    /// claim you did not mean.
    #[test]
    fn the_documented_export_shape_example_parses() {
        let rule = parse(
            r#"{
              "type": "export-shape",
              "id": "use-cases-return-the-pattern",
              "level": "error",
              "roots": ["src/use-cases/*"],
              "forbid_default": true,
              "max_exports": 1,
              "must_return": ["^ResponsePattern<.+,.+>$", "^Result<.+>$"],
              "why": "a use case returns the pattern, it never throws"
            }"#,
        );

        let Rule::ExportShape(shape) = &rule else {
            panic!("expected an export-shape rule, got {}", rule.type_name());
        };
        assert!(shape.forbid_default);
        assert_eq!(shape.max_exports, Some(1));
        assert_eq!(
            shape.must_return.as_slice(),
            ["^ResponsePattern<.+,.+>$", "^Result<.+>$"]
        );
        assert_eq!(rule.type_name(), "export-shape");
    }

    /// Each claim stands alone. A rule that only forbids defaults says nothing
    /// about how many exports there are or what they return, and a config that
    /// asks for one of the three must not be given the other two by default.
    #[test]
    fn each_export_shape_claim_is_optional_and_absent_by_default() {
        let rule =
            parse(r#"{"type":"export-shape","id":"no-defaults","level":"error","roots":"src"}"#);

        let Rule::ExportShape(shape) = &rule else {
            panic!("expected an export-shape rule");
        };
        assert!(!shape.forbid_default);
        assert_eq!(shape.max_exports, None);
        assert!(shape.must_return.is_empty());
    }

    /// `must_return` takes the one-or-many treatment every glob field takes, so
    /// a single pattern needs no brackets.
    #[test]
    fn must_return_accepts_a_bare_string() {
        let rule = parse(
            r#"{"type":"export-shape","id":"r","level":"error","roots":"src",
                "must_return":"^Result<.+>$"}"#,
        );

        let Rule::ExportShape(shape) = &rule else {
            panic!("expected an export-shape rule");
        };
        assert_eq!(shape.must_return.as_slice(), ["^Result<.+>$"]);
    }

    /// A rule that names none is every rule written before 0.21, and it stays
    /// exactly as valid. `config doctor` is the only thing that mentions it,
    /// at `warning`, and `check` says nothing at all — a repository's build
    /// must not fail because its config is under-documented.
    #[test]
    fn a_rule_that_names_no_decision_is_still_a_rule() {
        let rule = parse(r#"{"type":"no-passthrough","id":"r","level":"error","roots":"src"}"#);
        assert_eq!(rule.decision(), None);
    }

    /// An id with a space in it is refused on the way in, like every other id,
    /// rather than becoming a reference nothing can resolve.
    #[test]
    fn a_decision_reference_is_validated_as_an_id() {
        let bad = serde_json::from_str::<Rule>(
            r#"{"type":"no-passthrough","id":"r","level":"error","roots":"src","decision":"ADR 14"}"#,
        )
        .expect_err("should reject");
        assert!(bad.to_string().contains("decision id"), "{bad}");
    }

    /// Issue #43. The regex-over-a-directory-name capability existed on
    /// `naming.dir_pattern` and was reachable only through a door that requires
    /// a TypeScript parse, so a repository with no `.ts` near its folders could
    /// not use it at all.
    #[test]
    fn subfolder_patterns_parse_beside_the_lists() {
        let rule = parse(
            r#"{"type":"structure","id":"licao-nome-da-pasta","level":"error",
                "roots":["projetos"],
                "subfolder_patterns":["^\\d{2}-[a-z0-9-]+$"]}"#,
        );
        let Rule::Structure(structure) = &rule else {
            panic!("expected a structure rule");
        };

        assert_eq!(structure.subfolder_patterns, [r"^\d{2}-[a-z0-9-]+$"]);
        assert_eq!(structure.allowed_subfolders, None);
    }

    /// Issue #40. `[]` is a list of what may exist holding nothing, and the
    /// rule that says "this directory is a leaf" has no other spelling. The
    /// field has to be an option for that to be sayable at all: with a plain
    /// `Vec` an omitted field and an empty one arrive identical, so giving `[]`
    /// a meaning would give it to every config that never mentioned subfolders.
    #[test]
    fn an_absent_allowed_subfolders_is_not_an_empty_one() {
        let absent = parse(
            r#"{"type":"structure","id":"s","level":"error","roots":"referencia",
                "filename_patterns":["^[a-z-]+\\.md$"]}"#,
        );
        let Rule::Structure(absent) = &absent else {
            panic!("expected a structure rule");
        };
        assert_eq!(absent.allowed_subfolders, None);

        let empty = parse(
            r#"{"type":"structure","id":"s","level":"error","roots":"referencia",
                "allowed_subfolders":[]}"#,
        );
        let Rule::Structure(empty) = &empty else {
            panic!("expected a structure rule");
        };
        assert_eq!(empty.allowed_subfolders, Some(Vec::new()));
    }

    /// The field issue #39 asks for, in the shape the issue writes it.
    #[test]
    fn an_annotation_parses_as_one_value_or_a_list() {
        let one = parse(
            r#"{
              "type": "naming", "id": "n", "level": "error", "roots": "src/*",
              "file_pattern": "^(?<tool>.+)\\.tool\\.ts$",
              "must_export": {
                "kind": ["const"], "name": "AGENT_TOOL",
                "annotation": "AgentToolModule"
              }
            }"#,
        );
        let Rule::Naming(naming) = &one else {
            panic!("expected a naming rule");
        };
        assert_eq!(
            naming
                .must_export
                .annotation
                .as_ref()
                .expect("an annotation")
                .as_slice(),
            ["AgentToolModule"]
        );

        let many = parse(
            r#"{
              "type": "naming", "id": "n", "level": "error", "roots": "src/*",
              "file_pattern": "^(?<tool>.+)\\.tool\\.ts$",
              "must_export": {
                "kind": ["const"], "name": "AGENT_TOOL",
                "annotation": ["AgentToolModule", "LegacyToolModule"]
              }
            }"#,
        );
        let Rule::Naming(naming) = &many else {
            panic!("expected a naming rule");
        };
        assert_eq!(
            naming
                .must_export
                .annotation
                .as_ref()
                .expect("an annotation")
                .as_slice(),
            ["AgentToolModule", "LegacyToolModule"]
        );
    }

    /// Every rule written before the field existed asks for no annotation, and
    /// keeps meaning exactly what it meant.
    #[test]
    fn a_rule_that_omits_the_annotation_asks_for_none() {
        let rule = parse(
            r#"{
              "type": "naming", "id": "n", "level": "error", "roots": "src/*",
              "file_pattern": "^(?<name>.+)\\.ts$",
              "must_export": { "kind": "any", "name": "{{pascal(name)}}" }
            }"#,
        );
        let Rule::Naming(naming) = &rule else {
            panic!("expected a naming rule");
        };
        assert_eq!(naming.must_export.annotation, None);
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

    /// Issue #71: the dependency nobody wrote down. `ui` does not import `db`,
    /// and it depends on it through `orders` anyway.
    #[test]
    fn a_boundary_can_forbid_reaching_as_well_as_importing() {
        let rule = parse(
            r#"{
              "type": "import-boundary",
              "id": "ui-must-not-reach-db",
              "level": "error",
              "from": "packages/ui/**",
              "forbid_reaching": ["packages/db/**"],
              "except": ["packages/db/types/**"]
            }"#,
        );

        let Rule::ImportBoundary(boundary) = &rule else {
            panic!("expected an import-boundary rule");
        };
        assert_eq!(boundary.forbid_reaching.len(), 1);
        assert!(
            boundary.forbid_import_from.is_empty(),
            "a rule may forbid reaching without forbidding the direct import, \
             which is the whole case this field exists for"
        );
        assert_eq!(boundary.except.len(), 1);
    }

    /// And it can name a module instead of repeating that module's globs, the
    /// way `forbid_module` does for the direct form.
    #[test]
    fn reaching_can_name_a_module() {
        let rule = parse(
            r#"{
              "type": "import-boundary", "id": "r", "level": "error",
              "from": "packages/ui/**", "forbid_reaching_modules": ["persistence"]
            }"#,
        );

        let Rule::ImportBoundary(boundary) = &rule else {
            panic!("expected an import-boundary rule");
        };
        assert_eq!(boundary.forbid_reaching_modules.len(), 1);
        assert!(boundary.forbid_reaching.is_empty());
    }

    /// A boundary that says nothing about reach parses as before, and the
    /// field is empty rather than absent-and-surprising. This is the case that
    /// keeps every rule already written as cheap as it was: an empty
    /// `forbid_reaching` is what tells the runner not to build a graph.
    #[test]
    fn a_boundary_that_says_nothing_about_reach_asks_for_nothing() {
        let rule = parse(
            r#"{"type":"import-boundary","id":"b","level":"error",
                "from":"packages/ui/**","forbid_import_from":["packages/domain/**"]}"#,
        );

        let Rule::ImportBoundary(boundary) = &rule else {
            panic!("expected an import-boundary rule");
        };
        assert!(boundary.forbid_reaching.is_empty());
        assert!(boundary.forbid_reaching_modules.is_empty());
    }

    /// `import-cycle` is written like every other rule, and its scope field is
    /// `roots` rather than `from`: it is not a rule about what may be
    /// imported, it is a rule about the files it governs.
    #[test]
    fn the_documented_import_cycle_example_parses() {
        let rule = parse(
            r#"{
              "type": "import-cycle",
              "id": "no-cycles",
              "level": "error",
              "roots": "packages/**"
            }"#,
        );

        let Rule::ImportCycle(cycle) = &rule else {
            panic!("expected an import-cycle rule");
        };
        assert_eq!(rule.type_name(), "import-cycle");
        assert_eq!(rule.id().as_str(), "no-cycles");
        assert_eq!(rule.level(), Level::Error);
        assert_eq!(rule.scope().len(), 1);
        assert!(
            cycle.include_type_only,
            "the same default `import-boundary` has, and the same field name: a \
             loop of type imports is a loop the compiler walks"
        );
    }

    /// And the opt-out, for a project that only cares about loops that exist
    /// at runtime.
    #[test]
    fn an_import_cycle_rule_can_ignore_type_only_loops() {
        let rule = parse(
            r#"{
              "type": "import-cycle", "id": "no-cycles", "level": "error",
              "roots": "packages/**", "include_type_only": false
            }"#,
        );

        let Rule::ImportCycle(cycle) = &rule else {
            panic!("expected an import-cycle rule");
        };
        assert!(!cycle.include_type_only);
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

/// A capability only these files may reach.
///
/// Every other `forbid_*` in this config is about an import. This is about a
/// **call**, which is exactly what is left over once `import-boundary` has cut
/// every capability that arrives through a specifier.
///
/// ```json
/// { "type": "chokepoint",
///   "id": "the-environment-is-read-once",
///   "level": "error",
///   "callee": ["process.env", "process.argv"],
///   "only_in": ["src/config/**"],
///   "decision": "ADR-022",
///   "why": "config read at startup, in one place, or it is read everywhere" }
/// ```
///
/// The sentences it exists for are ordinary ADR sentences: *only `src/config`
/// reads the environment*, *only `src/clock` knows what time it is*, *only the
/// composition root constructs adapters*, *nobody talks to the network outside
/// `src/http`*. What they have in common is that the capability is ambient --
/// `process.env`, `Date.now`, `fetch`, `localStorage` -- or is the project's
/// own symbol reached through an object imported legitimately somewhere else.
/// Neither has an edge in the graph to cut.
///
/// **Not a taint analysis.** It asks whether a name appears at a call site
/// inside a scope. It does not follow a value, so a capability passed as an
/// argument out of the chokepoint is invisible to it -- the same line
/// `docs/RULES.md` already draws beside `call-obligation`. Issue #118.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChokepointRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words. See [`StructureRule::why`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements, when it implements a declared one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// The callees this rule guards, as they appear at a call site.
    ///
    /// Matched exactly, or as a prefix at a dot: `process.env` guards
    /// `process.env` and `process.env.DATABASE_URL`, and does not guard
    /// `processing.env`. That is a change of dialect from `call-obligation`,
    /// which matches its symbol exactly -- and it is the right one here,
    /// because a chokepoint is about a capability rather than about one
    /// function, and `process.env.DATABASE_URL` is recorded as written.
    ///
    /// A construction is spelled the way the source spells it:
    /// `"new PostgresRepo"`. It is not a call and does not answer to
    /// `PostgresRepo`.
    #[serde(default)]
    pub callee: Vec<String>,
    /// The JSX elements this rule guards, as they appear in markup.
    ///
    /// ```json
    /// { "type": "chokepoint", "id": "only-the-primitives-layer-writes-markup",
    ///   "level": "error",
    ///   "roots": ["src/features/*"],
    ///   "renders": ["div", "span", "button"],
    ///   "only_in": ["src/ui/primitives/**"] }
    /// ```
    ///
    /// *"Nothing outside `features/checkout` renders `CheckoutForm`"* and *"a
    /// feature may not write raw markup, only composed components"* are the
    /// two sentences a design system needs, and neither is an import
    /// question: rendering and importing are different relationships.
    ///
    /// Matched exactly, and **not** by the dot-prefix rule `callee` uses.
    /// `Ui.Button` is one component, not a member of a `Ui` capability, so a
    /// rule naming `Ui` does not guard it. Issue #145.
    ///
    /// The case is JSX's own distinction: `div` is an intrinsic element and
    /// `Card` is a component in scope.
    #[serde(default)]
    pub renders: Vec<String>,
    /// Regex over the filename, narrowing the population further.
    ///
    /// `roots` selects directories; this selects the files in them. *"Only
    /// `*.server.ts` may call `fetch`"* is a sentence about a filename, and
    /// without this the rule could only be written about a folder. Optional:
    /// a rule that names none governs every file its scope reaches, which is
    /// every chokepoint written before the field existed. Issue #146.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_pattern: Option<String>,
    /// The module a guarded name has to come from.
    ///
    /// Two packages can export a `Ledger`, and a rule about *this* project's
    /// one should not fire on the other. Spelled and matched the way
    /// [`MustCall::imported_from`](crate::rule::MustCall::imported_from) is:
    /// against the specifier **as written**, so the rule needs no resolution.
    ///
    /// Optional, and absent is the right answer for an ambient capability:
    /// `process.env` is imported from nowhere and there is nothing to
    /// disambiguate. A rule that names one guards only the names the file
    /// actually took from there. Issue #146.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<String>,
    /// Directory globs this rule governs.
    ///
    /// Separate from [`only_in`](Self::only_in), and the separation is the
    /// point: `roots` is the population the question is asked of, `only_in` is
    /// the answer that is allowed. A test suite reads `process.env`
    /// legitimately, so a chokepoint that governed the whole repository by
    /// default would report the tests on its first run -- and a rule whose
    /// first run is wrong is one nobody keeps.
    pub roots: Patterns,
    /// The files allowed to reach them.
    ///
    /// An allowlist, and there is no `forbid` direction. `only_in` is the one
    /// that does not decay -- the argument #75 already made for
    /// `only_import_from`: a new file outside the chokepoint is reported the
    /// day it is written, where a forbid list has to be extended by whoever
    /// added the thing it should have forbidden.
    pub only_in: Patterns,
    /// Narrow this rule to the files that import something.
    ///
    /// Path globs, matched against where an import *lands*. See
    /// [`ImportCycleRule::when_importing`].
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// No file in scope may sit on an import loop.
///
/// The first rule whose question cannot be answered from one file, and the
/// reason a configuration carrying one costs a resolution pass over the whole
/// repository. See `docs/RULES.md`.
///
/// Deliberately no `ignored_circular_dependencies`. A cycle is a finding, and
/// `baseline` already accepts findings — per rule and per path, which is the
/// right granularity because every file on a loop is reported. Nx has such an
/// option because it has no baseline. Adding one here would be a second
/// mechanism for accepting a finding, and the two would disagree the first
/// time somebody used both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportCycleRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words. See [`StructureRule::why`].
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
    ///
    /// `roots` rather than `from`: `import-boundary` calls its scope `from`
    /// because it reads against `forbid_import_from`, and this rule forbids no
    /// destination. It is a rule about the files it governs, like the other
    /// four that spell it `roots`.
    ///
    /// It governs where a finding is *reported*, not what the graph is built
    /// from. The graph is always the whole repository, because a loop that
    /// leaves the scope and comes back is still a loop.
    pub roots: Patterns,
    /// Whether `import type` and inline `type` marks close a loop. Default
    /// `true`.
    ///
    /// Spelled and defaulted the same way `import-boundary` spells it. A type
    /// import is erased at runtime, so a loop made only of them cannot
    /// deadlock anything — and it is still a loop the compiler walks, which is
    /// why the default counts it and the opt-out exists for projects that only
    /// care about runtime.
    #[serde(default = "default_true")]
    pub include_type_only: bool,
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

/// What a file exposes, without saying anything about what it is called.
///
/// `naming` couples the export to the *filename*. Plenty of architectural
/// decisions are about the export alone — *"we do not use default exports"*,
/// *"one export per file"*, *"every use case returns the pattern"* — and none
/// of them mentions a name. Saying any of them through `naming` meant inventing
/// a naming claim you did not mean in order to make an export claim you did.
///
/// Three claims in one kind, because they are the same question asked three
/// ways: *what does this file expose?* Splitting them would be three kinds
/// sharing one scope, one `roots` and one `why`. Issue #101.
///
/// # The division of labour, which is the whole design
///
/// `must_return` requires that a function **declares** its return type. It
/// does not check that the body conforms — that is `tsc`'s job, and `tsc` does
/// it well. What `tsc` cannot do is *require that you annotate at all*: a
/// function returning `{ ok: true }` with no return type compiles perfectly.
///
/// **archwarden guarantees the pattern is declared; `tsc` guarantees the body
/// conforms.** Neither alone is the guarantee a team wants, and together they
/// are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportShapeRule {
    /// Stable identifier, unique across the config and its presets.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words. See [`StructureRule::why`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements. See [`StructureRule::decision`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Whether a default export is refused.
    ///
    /// `false` by default, so a rule that only wants to say something about
    /// return types says nothing about defaults.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub forbid_default: bool,
    /// The most exports a file may have.
    ///
    /// **Counts what exists at runtime.** `type` and `interface` exports do
    /// not count, and the default counts as one. A file exporting a function
    /// and the interface of its dependencies is idiomatic TypeScript, and a
    /// `max_exports: 1` that fired on it would be a rule nobody leaves on —
    /// which is the same argument `spec-pair.skip_type_only` already makes one
    /// rule over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_exports: Option<usize>,
    /// Return types an exported function may declare, as regexes.
    ///
    /// A **list**, and that is what settles the alias problem without imposing
    /// a convention. `type Result<T> = ResponsePattern<T, Error>` is the same
    /// type and a different string, so a team that has aliases lists them:
    ///
    /// ```json
    /// "must_return": ["^ResponsePattern<.+,.+>$", "^Result<.+>$"]
    /// ```
    ///
    /// A team that decides *"annotate with the canonical name"* writes one
    /// pattern and gets that convention enforced — which is itself an
    /// architectural decision, and now one the config states rather than
    /// implies.
    ///
    /// Matched **text against text**, on the same terms as `naming`'s
    /// annotations: no resolution, no inference, no assignability. Pair it with
    /// `import-boundary.must_import_from` to close the remaining hole, which is
    /// somebody declaring a local lookalike under the canonical name.
    ///
    /// Applies to the exports that *can* return something — a `function`
    /// declaration, or a function or arrow assigned to a binding. A callable
    /// that declares nothing is a finding, which is the point.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub must_return: Patterns,
    /// Narrow this rule to the files that import something. See decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// A directory that has stopped growing.
///
/// `import-boundary` can forbid **importing** something. Nothing could forbid
/// **adding** to it — and that is half of every migration ADR: *"the legacy
/// module is closed for extension; new work goes in `packages/core`"*.
///
/// # It is `baseline` pointed forward
///
/// Every file under `roots` is a finding. `baseline` accepts today's, by rule
/// and path, and reports tomorrow's. Nothing here remembers a date and nothing
/// reads `git`: archwarden answers from a working tree and a committed
/// baseline, which is what keeps it deterministic and keeps a shallow clone
/// working — a freeze that consulted history would answer differently in CI
/// than on a laptop.
///
/// It also turns `baseline` from a record of debt into a statement of intent,
/// which is a better thing for it to be. Issue #102.
///
/// # Two things it deliberately does not do
///
/// **It does not exempt a move.** `legacy/a.ts → legacy/sub/a.ts` is reported.
/// A module closed for extension is one that has stopped, and reshuffling it is
/// not stopping. When the move is deliberate, `archwarden baseline` accepts it
/// and leaves the change in a diff somebody reviews — which reads as one move
/// rather than a removal and an addition, because `baseline` already pairs
/// them. A move *out* is silent, which is the point of the freeze.
///
/// **It is about files, not exports.** *"No new exports in this file"* is a
/// real second claim and a much harder one: it needs the frozen set to be
/// per-symbol, and `baseline` accepts paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrozenRule {
    /// Stable identifier, unique across the config and its presets.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words. See [`StructureRule::why`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements. See [`StructureRule::decision`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// The directories that have stopped growing.
    pub roots: Patterns,
    /// Narrow this rule to the files that import something. See decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// A counterpart in a parallel tree, rather than a sibling.
///
/// `pair` and `spec-pair` both look in the *same directory*. Plenty of
/// conventions pair across parallel trees — *"every entity has a migration"*,
/// *"every route has a page in the docs"*, *"tests live in `test/`, mirroring
/// `src/`"* — and `pair` takes a sibling **name**, so there was no way to say
/// "the same path, elsewhere, transformed". Issue #103.
///
/// Two pieces that already existed, put together: `presence` proves a file is
/// on disk without parsing anything, and `naming` renders a path from capture
/// groups with transforms. A mirror is the second producing a path for the
/// first to check.
///
/// # One direction per rule
///
/// *"Every entity has a migration"* and *"every migration belongs to an
/// entity"* are two claims, and each deserves its own `why` — the first is
/// about completeness, the second about orphans. A flag would put two reasons
/// on one rule and make a reader work out which half fired.
///
/// # Why `pair` and `spec-pair` stay
///
/// They are the ergonomic forms: a bare sibling name, and a sibling with a
/// marker. Collapsing them into a template would make the common case wordier
/// to buy a generality most configs never use. The test is whether the
/// specialised forms are *shorter to write*, not whether they are expressible
/// — three kinds that are one kind wearing three names is how a format gets
/// heavy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MirrorRule {
    /// Stable identifier, unique across the config and its presets.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words. See [`StructureRule::why`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements. See [`StructureRule::decision`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs whose files this rule is about.
    pub roots: Patterns,
    /// Regex over the filename, with the capture groups the template uses.
    ///
    /// A file the pattern does not match is outside the population, the same
    /// way it is for `pair`.
    pub file_pattern: String,
    /// The counterpart's path, as a template rendered from repository root.
    ///
    /// The groups are `file_pattern`'s, plus two the path itself provides:
    ///
    /// - `dirname` — the immediate parent directory's name, which
    ///   `frontmatter.equals` already defines the same way;
    /// - `subpath` — the directory path from the rule's root down to the file,
    ///   which is what a mirror across a *nested* tree needs.
    ///   `src/a/b/x.ts` under `roots: ["src/**"]` gives `a/b`, so
    ///   `test/{{raw(subpath)}}/{{raw(name)}}.test.ts` is writable. Empty when
    ///   the file sits directly in the root, and the renderer collapses the
    ///   `//` that would leave.
    ///
    /// Only that the counterpart **exists** is checked. *"And it must contain a
    /// test case"* is `spec-pair`'s question and has an answer there already.
    pub must_exist: String,
    /// Narrow this rule to the files that import something. See decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// What a file's header declares about itself.
///
/// `frontmatter` asks a **document** to declare things about itself, and code
/// had no equivalent. Ownership, stability and lifecycle are ordinary ADR
/// content — *"every file under `payments/` declares an `@owner`"*, *"an
/// experimental export carries a removal date"* — and they are properties of a
/// file that no rule could ask about. Issue #104.
///
/// # The grammar
///
/// One key per line, in a comment above the file's first statement:
///
/// ```text
/// // archwarden-owner: payments-team
/// // archwarden-stability: experimental
/// ```
///
/// **Its own prefix, not a `JSDoc` tag.** `@internal` and `@deprecated` already
/// mean something to `tsc`, to editors and to `TypeDoc`, and a marker with two
/// readers eventually has two interpretations: the day somebody writes
/// `@internal` for the editor's benefit and archwarden reports a boundary
/// violation is the day the feature gets removed. It also puts these in the
/// same family as `archwarden-allow`, so a `grep` for `archwarden-` finds
/// everything this tool reads out of a comment. The cost is that it is uglier
/// than `@owner`, and that is the trade.
///
/// **One key per line**, rather than `archwarden: owner=x, stability=y`. Fewer
/// lines and a second grammar to parse, to validate and to explain when
/// somebody writes it wrong; this is the shape `archwarden-allow` already uses
/// and the shape a `sed` can find.
///
/// **The header only, in this version.** Above any export is far more useful
/// and far more work — it needs the marker bound to the declaration that
/// follows it. A marker written lower down is not ignored: it is reported as
/// misplaced, because telling an author who wrote `archwarden-owner` that the
/// file declares no owner is the one answer they cannot act on.
///
/// # The shape is `frontmatter`'s, deliberately
///
/// Two kinds asking the same question of two file formats should look the
/// same, and the document rule already settled the hard parts: values compare
/// as **text** with no type system, a value outside a closed vocabulary is
/// worse than an absent one, and `equals` can tie a value to the path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MetadataRule {
    /// Stable identifier, unique across the config and its presets.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words. See [`StructureRule::why`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements. See [`StructureRule::decision`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs whose files this rule is about.
    ///
    /// The whole population. There is no `file_pattern` here, which is
    /// decision 28's argument about `frozen`: a field decided before anybody
    /// asked for one, and addable later without breaking a config.
    pub roots: Patterns,
    /// Keys the header must declare.
    ///
    /// Names, written without the `archwarden-` prefix: `"owner"` is the key
    /// a file declares as `// archwarden-owner: payments-team`.
    ///
    /// A key beginning with `allow` is refused where the config compiles. The
    /// suppression grammar reaches that spelling first — `archwarden-allow:`
    /// is a suppression and never a claim — so no file could satisfy the rule
    /// however it was written.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub require: Patterns,
    /// The closed vocabulary a key's value must come from.
    ///
    /// The case that justifies the rule existing, on `frontmatter`'s
    /// reasoning: a missing key is at least an absence, and a value outside
    /// the vocabulary is *confidently wrong*. Values compare as text.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub one_of: std::collections::BTreeMap<String, Patterns>,
    /// A key whose value must equal a template rendered from the path.
    ///
    /// `{{raw(dirname)}}` is the name of the directory the file sits in, and
    /// it is the only group this template may name — the same group, defined
    /// the same way, as `frontmatter.equals`. The transforms come along:
    /// `{{kebab(dirname)}}` is spelled the same way here as there.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub equals: std::collections::BTreeMap<String, String>,
    /// Keys whose value is a date that must not have passed.
    ///
    /// `ISO YYYY-MM-DD`, and nothing else: a value that is not one is its own
    /// finding rather than a guess. Guessing which of two numbers is the month
    /// is how a deadline lands eleven months from where it was meant to.
    ///
    /// ```json
    /// { "require": ["remove-by"], "deadline": ["remove-by"] }
    /// ```
    ///
    /// The day compared against is the **run's**, not the clock's: `check`
    /// defaults it to today in UTC and `--as-of` pins it, so two machines
    /// given the same date give the same answer. That is what keeps decision
    /// 28's determinism while adding the one question that needs to know what
    /// day it is.
    ///
    /// A passed deadline fires at this rule's own `level`, like every other
    /// finding here. Whoever writes the deadline chooses: `error` if they mean
    /// it, `warning` while a migration is still running.
    ///
    /// It does not require the key. An absent one stays `require`'s to report,
    /// exactly as `one_of` already decides it. Issue #117.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub deadline: Patterns,
    /// Narrow this rule to the files that import something. See decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
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

#[cfg(test)]
mod narrowing_tests {
    use super::Rule;

    fn parsed(source: &str) -> Rule {
        serde_json::from_str(source).expect("the rule parses")
    }

    /// The second axis reaches the rule it was written on, whichever kind that
    /// is. Issue #98, decision 25.
    #[test]
    fn a_rule_carries_the_imports_it_was_narrowed_by() {
        let narrowed = parsed(
            r#"{"type":"call-obligation","id":"c","level":"error","roots":["src/*"],
                "file_pattern":"^x$","when_importing":"src/http/**",
                "when_importing_packages":["zod"],
                "must_call":{"symbol":"S","imported_from":"m"}}"#,
        );

        assert_eq!(narrowed.when_importing().as_slice(), ["src/http/**"]);
        assert_eq!(narrowed.when_importing_packages(), ["zod"]);
    }

    /// And a rule that names none carries none — which is what keeps every
    /// rule written before 0.20 as cheap as it was.
    #[test]
    fn a_rule_that_names_none_carries_none() {
        let plain = parsed(
            r#"{"type":"presence","id":"p","level":"error","roots":["src/*"],
                "require":["a.md"]}"#,
        );

        assert!(plain.when_importing().is_empty());
        assert!(plain.when_importing_packages().is_empty());
    }

    /// A directory rule carries it too: "some file inside imports X" is the
    /// reading decided for `presence` and `structure`.
    #[test]
    fn a_directory_rule_carries_it_as_well() {
        let narrowed = parsed(
            r#"{"type":"presence","id":"p","level":"error","roots":["src/*"],
                "when_importing":"src/db/**","require":["contract.md"]}"#,
        );

        assert_eq!(narrowed.when_importing().as_slice(), ["src/db/**"]);
    }

    /// `import-boundary` has none and never will: it already chooses its
    /// importers with `from`, `from_module` and `from_kind`, and a second way
    /// to say one thing is a second thing to get wrong.
    #[test]
    fn a_boundary_rule_never_narrows_by_import() {
        let boundary = parsed(
            r#"{"type":"import-boundary","id":"b","level":"error",
                "from":["src/**"],"forbid_import_from":["infra/**"]}"#,
        );

        assert!(boundary.when_importing().is_empty());
        assert!(boundary.when_importing_packages().is_empty());
    }
}
