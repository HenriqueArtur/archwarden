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
pub mod call_obligation;
pub mod frontmatter;
pub mod import_boundary;
pub mod import_cycle;
pub mod naming;
pub mod no_passthrough;
pub mod pair;
pub mod presence;
pub mod spec_pair;
pub mod structure;

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRuleKind},
    traits::RuleEngine,
};

/// Builds an engine for every rule in the configuration.
///
/// The `match` is exhaustive on purpose. A kind added to `CompiledRuleKind`
/// without an engine fails to compile here, so "a rule this build cannot
/// check" is not a state a run can be in and nothing has to be reported as
/// unchecked. This is the same trade as `CompiledRule` itself: make the bad
/// state unrepresentable rather than detect it later.
///
/// Declaration order is preserved, which is what makes a report's ordering
/// follow the config rather than the order engines happen to be tried in.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "one arm per rule kind; splitting it would put the arms somewhere \
              the exhaustive match no longer names them, which is the property \
              that makes a kind added without an engine fail to build"
)]
pub fn engines_for(config: &CompiledConfig) -> Vec<Box<dyn RuleEngine>> {
    config
        .rules()
        .map(|rule| -> Box<dyn RuleEngine> {
            match &rule.kind {
                CompiledRuleKind::Structure {
                    allowed_subfolders,
                    warn_subfolders,
                    recurse_into,
                    subfolder_patterns,
                    filename_patterns,
                } => Box::new(structure::StructureEngine::build(
                    rule,
                    allowed_subfolders.as_ref(),
                    warn_subfolders,
                    recurse_into,
                    subfolder_patterns,
                    filename_patterns,
                    config.skip_dirs().clone(),
                )),
                CompiledRuleKind::NoPassthrough {
                    forms,
                    except,
                    allow_package_entrypoints,
                    allow_partial,
                } => Box::new(no_passthrough::NoPassthroughEngine::build(
                    rule,
                    *forms,
                    except,
                    *allow_package_entrypoints,
                    *allow_partial,
                )),
                CompiledRuleKind::SpecPair {
                    subfolders,
                    spec_markers,
                    ignore_files,
                    spec_dirs,
                    require_non_empty_spec,
                    skip_type_only,
                } => Box::new(spec_pair::SpecPairEngine::build(
                    rule,
                    subfolders,
                    spec_markers,
                    ignore_files,
                    spec_dirs,
                    *require_non_empty_spec,
                    *skip_type_only,
                )),
                CompiledRuleKind::Naming {
                    file_pattern,
                    dir_pattern,
                    name_template,
                    kind,
                    annotation,
                    signature_hint,
                } => Box::new(naming::NamingEngine::build(
                    rule,
                    file_pattern,
                    dir_pattern.as_ref(),
                    name_template,
                    kind,
                    annotation,
                    signature_hint.as_deref(),
                )),
                CompiledRuleKind::ImportBoundary {
                    forbid,
                    require,
                    allow,
                    allow_packages,
                    groups,
                    forbid_packages,
                    forbid_reaching,
                    except,
                    except_from,
                    include_type_only,
                } => Box::new(import_boundary::ImportBoundaryEngine::build(
                    rule,
                    forbid,
                    require,
                    allow.clone(),
                    allow_packages.clone(),
                    groups.clone(),
                    forbid_packages,
                    forbid_reaching,
                    except,
                    except_from,
                    *include_type_only,
                )),
                CompiledRuleKind::ImportCycle { include_type_only } => Box::new(
                    import_cycle::ImportCycleEngine::build(rule, *include_type_only),
                ),
                CompiledRuleKind::Frontmatter {
                    file_pattern,
                    require,
                    one_of,
                    equals,
                } => Box::new(frontmatter::FrontmatterEngine::build(
                    rule,
                    file_pattern,
                    require,
                    one_of,
                    equals,
                )),
                CompiledRuleKind::Pair {
                    file_pattern,
                    must_exist,
                } => Box::new(pair::PairEngine::build(rule, file_pattern, must_exist)),
                CompiledRuleKind::Presence {
                    require,
                    require_any,
                } => Box::new(presence::PresenceEngine::build(
                    rule,
                    require,
                    require_any,
                    config.skip_dirs().clone(),
                )),
                CompiledRuleKind::CallObligation {
                    file_pattern,
                    symbol,
                    imported_from,
                } => Box::new(call_obligation::CallObligationEngine::build(
                    rule,
                    file_pattern,
                    symbol,
                    imported_from,
                )),
            }
        })
        .collect()
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
            why: None,
            module_why: None,
            imports: None,
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
            allowed_subfolders: Some(vec!["types".to_owned()]),
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
            filename_patterns: Vec::new(),
        }
    }

    fn spec_pair_rule() -> CompiledRuleKind {
        CompiledRuleKind::SpecPair {
            subfolders: vec![".".to_owned()],
            spec_markers: vec!["spec".to_owned()],
            ignore_files: PathSet::default(),
            spec_dirs: Vec::new(),
            require_non_empty_spec: false,
            skip_type_only: false,
        }
    }

    fn naming_rule() -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile("^(?<name>[a-z]+)\\.ts$").expect("valid"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind: archwarden_core::facts::KindFilter::Any,
            annotation: Vec::new(),
            signature_hint: None,
        }
    }

    fn call_obligation_rule() -> CompiledRuleKind {
        CompiledRuleKind::CallObligation {
            file_pattern: Pattern::compile(r"^route\\.post\\.ts$").expect("valid"),
            symbol: "Event.save".to_owned(),
            imported_from: "@org/domain/event".to_owned(),
        }
    }

    fn import_boundary_rule() -> CompiledRuleKind {
        CompiledRuleKind::ImportBoundary {
            forbid: PathSet::compile(["packages/domain/**".to_owned()]).expect("valid"),
            groups: Vec::new(),
            allow: None,
            allow_packages: None,
            require: PathSet::default(),
            forbid_packages: Vec::new(),
            forbid_reaching: PathSet::default(),
            except: PathSet::default(),
            except_from: PathSet::default(),
            include_type_only: true,
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

        let engines = engines_for(&config);

        let ids: Vec<_> = engines.iter().map(|e| e.id().as_str().to_owned()).collect();
        assert_eq!(ids, ["spec-first", "structure-second"]);
    }

    /// Every rule kind v0 defines now has an engine, so there is nothing left
    /// to report as unimplemented.
    ///
    /// The reporting path is not dead -- it is what a kind added to
    /// `CompiledRuleKind` before its engine would take, and a run that quietly
    /// skipped it would print a clean result nobody has reason to distrust.
    /// It is covered by `run.rs`'s report test rather than here, because
    /// there is no longer a real kind to stand in for one.
    #[test]
    fn nothing_is_left_unimplemented() {
        let config = config(vec![
            rule("structure", structure_rule()),
            rule("spec-pair", spec_pair_rule()),
            rule("naming", naming_rule()),
            rule("import-boundary", import_boundary_rule()),
            rule("call-obligation", call_obligation_rule()),
        ]);

        let engines = engines_for(&config);

        assert_eq!(engines.len(), 5);
    }

    /// Every rule kind that has an engine gets one. The list is the contract
    /// `config doctor` and the unimplemented-rule note are both built on, so
    /// an engine that exists but is never reached would be invisible.
    #[test]
    fn every_implemented_kind_is_reachable() {
        let config = config(vec![
            rule("structure", structure_rule()),
            rule("spec-pair", spec_pair_rule()),
            rule("naming", naming_rule()),
            rule("import-boundary", import_boundary_rule()),
        ]);

        let engines = engines_for(&config);

        let ids: Vec<_> = engines.iter().map(|e| e.id().as_str().to_owned()).collect();
        assert_eq!(ids, ["structure", "spec-pair", "naming", "import-boundary"]);
    }

    /// Only the boundary rule pays for resolution, which is what keeps a
    /// naming-only run off the filesystem a second time.
    #[test]
    fn only_the_boundary_rule_asks_for_resolution() {
        let config = config(vec![
            rule("naming", naming_rule()),
            rule("import-boundary", import_boundary_rule()),
        ]);

        let engines = engines_for(&config);
        let wants: Vec<_> = engines.iter().map(|e| e.needs_resolution()).collect();

        assert_eq!(wants, [false, true]);
    }

    #[test]
    fn an_empty_config_builds_no_engines() {
        assert!(engines_for(&config(Vec::new())).is_empty());
    }
}
