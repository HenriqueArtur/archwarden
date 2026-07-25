//! The five rule engines: structure, naming, spec-pair, import-boundary,
//! call-obligation.
//!
//! Depends on `archwarden-core` **only** — never on the parser or the
//! resolver. Engines receive extracted facts and compiled rules, and return
//! findings.
//!
//! Every engine implements `describe_expectation()` alongside its check logic.
//! That is not optional: `scaffold` and `agent-guide` are built from it, so a
//! rule whose expectation is not describable does not compile. This is how
//! decision 9 stays true rather than aspirational.
//!
//! See `docs/RULES.md`.

// Modules document themselves with `//!`; see the note in archwarden-core.
pub mod naming;
pub mod spec_pair;
pub mod structure;

use archwarden_core::{compiled::CompiledConfig, traits::RuleEngine};

/// Builds an engine for every rule in a compiled config.
///
/// Each rule is offered to each engine constructor and the one that recognises
/// its kind takes it. A rule of a kind no engine has been written for yet is
/// skipped rather than dropped silently -- it is returned in the second half
/// of the pair so a caller can say so.
///
/// Declaration order is preserved, which is what makes a report's ordering
/// follow the config rather than the order engines happen to be tried in.
#[must_use]
pub fn engines_for(config: &CompiledConfig) -> (Vec<Box<dyn RuleEngine>>, Vec<String>) {
    let mut engines: Vec<Box<dyn RuleEngine>> = Vec::new();
    let mut unimplemented = Vec::new();

    for rule in config.rules() {
        if let Some(engine) =
            structure::StructureEngine::from_rule(rule, config.skip_dirs().clone())
        {
            engines.push(Box::new(engine));
        } else if let Some(engine) = spec_pair::SpecPairEngine::from_rule(rule) {
            engines.push(Box::new(engine));
        } else if let Some(engine) = naming::NamingEngine::from_rule(rule) {
            engines.push(Box::new(engine));
        } else {
            unimplemented.push(rule.id.to_string());
        }
    }

    (engines, unimplemented)
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::{CompiledRule, CompiledRuleKind, SkipDirs},
        glob::PathSet,
        hash::ContentHash,
        ids::RuleId,
        level::Level,
        pattern::Pattern,
        scope::Scope,
    };

    fn rule(id: &str, kind: CompiledRuleKind) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid scope"),
            kind,
        }
    }

    fn config(rules: Vec<CompiledRule>) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b""),
        )
    }

    fn structure_rule() -> CompiledRuleKind {
        CompiledRuleKind::Structure {
            allowed_subfolders: vec!["types".to_owned()],
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            filename_patterns: Vec::new(),
        }
    }

    fn spec_pair_rule() -> CompiledRuleKind {
        CompiledRuleKind::SpecPair {
            subfolders: vec![".".to_owned()],
            spec_markers: vec!["spec".to_owned()],
            ignore_files: PathSet::default(),
            require_non_empty_spec: false,
        }
    }

    /// Declaration order survives, so a report follows the config rather than
    /// the order the engine constructors happen to be tried in.
    #[test]
    fn engines_are_built_in_declaration_order() {
        let config = config(vec![
            rule("spec-first", spec_pair_rule()),
            rule("structure-second", structure_rule()),
        ]);

        let (engines, unimplemented) = engines_for(&config);

        let ids: Vec<_> = engines.iter().map(|e| e.id().as_str().to_owned()).collect();
        assert_eq!(ids, ["spec-first", "structure-second"]);
        assert!(unimplemented.is_empty());
    }

    /// A rule kind with no engine yet is named rather than dropped, so the
    /// caller can tell the user what was not checked instead of quietly
    /// reporting a clean run.
    #[test]
    fn a_rule_kind_without_an_engine_is_reported_not_dropped() {
        let config = config(vec![
            rule("has-engine", structure_rule()),
            rule(
                "no-engine-yet",
                CompiledRuleKind::CallObligation {
                    file_pattern: Pattern::compile("^x$").expect("valid"),
                    symbol: "Event.save".to_owned(),
                    imported_from: "@org/domain".to_owned(),
                },
            ),
        ]);

        let (engines, unimplemented) = engines_for(&config);

        assert_eq!(engines.len(), 1);
        assert_eq!(unimplemented, ["no-engine-yet"]);
    }

    #[test]
    fn an_empty_config_builds_no_engines() {
        let (engines, unimplemented) = engines_for(&config(Vec::new()));
        assert!(engines.is_empty());
        assert!(unimplemented.is_empty());
    }
}
