//! `archwarden config doctor` — catching a configuration that parses and is
//! still wrong.
//!
//! `config validate` answers "does this file mean anything?". This answers the
//! harder question: "does it mean what you think?". A rule that parses, loads,
//! and then never fires is the failure mode this exists for, because it is
//! indistinguishable from a rule that passes.
//!
//! # What is not here, because it is already an error
//!
//! `CONFIG.md:333` lists duplicate rule ids, `disable` naming a rule that does
//! not exist, and a preset declaring `root` among the doctor's checks. All
//! three are hard errors at merge time (M1), which is strictly better: a typo
//! fails when the config loads rather than in a separate command the user may
//! never run. Correction C16.

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRule},
    facts::FileFacts,
    ids::RuleId,
    level::Level,
    path::RepoRelPath,
};
use archwarden_engine::walk::RepoTree;
use camino::Utf8Path;
use serde::Serialize;

/// The version of the `doctor` JSON shape.
pub const DOCTOR_VERSION: u32 = 0;

/// Something worth telling the user about their configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Concern {
    /// A stable slug, so a user can grep for one and a tool can branch on it.
    pub code: &'static str,
    /// How loud it is.
    ///
    /// The `doctor` had no notion of severity until 0.21 — sixteen checks in
    /// one flat list, every one of them advice. Issue #100 needed two of its
    /// three to stay advice and one to be a contradiction, so this exists.
    ///
    /// **Everything that came before is `warning`**, and stays that way. Some
    /// of those checks arguably deserve `error`; promoting them is a review of
    /// sixteen checks that belongs to whichever release is about them, not to
    /// one that is about decisions. Assigning them all the level they already
    /// had in practice is the change that adds a field and changes nothing.
    ///
    /// It does not reach the exit code. `doctor` is advice and `check` is the
    /// gate — the same line issue #100 draws when it keeps `check` silent
    /// about a rule that names no decision.
    pub level: Level,
    /// The rule it is about, when it is about one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<RuleId>,
    /// The file it is about, when it is about one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<RepoRelPath>,
    /// What is wrong, in a sentence.
    pub message: String,
    /// What to do about it.
    pub fix: String,
}

/// Every concern the configuration raises, in configuration order.
///
/// Reads no file: these are the checks answerable from the config alone.
#[must_use]
pub fn examine(config: &CompiledConfig) -> Vec<Concern> {
    let mut concerns = Vec::new();

    walk_scope_with_boundaries(config, &mut concerns);

    reasons_left_unsaid(config, &mut concerns);

    module_nobody_references(config, &mut concerns);

    module_wearing_no_kind(config, &mut concerns);

    decisions_left_unsaid(config, &mut concerns);

    superseded_but_still_enforced(config, &mut concerns);

    decision_nobody_enforces(config, &mut concerns);
    decision_may_duplicate(config, &mut concerns);
    unenforceable_but_a_rule_keeps_it(config, &mut concerns);

    for rule in config.rules() {
        unreachable_scope(config, rule, &mut concerns);
        constrains_nothing(rule, &mut concerns);
        spec_subfolder_not_allowed(config, rule, &mut concerns);
        hint_disagrees_with_kind(rule, &mut concerns);
        kind_no_enabled_language_can_declare(config, rule, &mut concerns);
    }

    concerns
}

mod config;
mod decisions;
mod render;
mod repository;

pub use render::render;

use config::{
    constrains_nothing, frozen_with_nothing_accepted, hint_disagrees_with_kind,
    kind_no_enabled_language_can_declare, spec_subfolder_not_allowed, unreachable_scope,
    walk_scope_with_boundaries,
};
use decisions::{
    decision_documents_out_of_date, decision_may_duplicate, decision_nobody_enforces,
    decision_scope_matches_nothing, decisions_left_unsaid, reasons_left_unsaid,
    superseded_but_still_enforced, unenforceable_but_a_rule_keeps_it,
};
use repository::{
    module_nobody_references, module_scope_matches_nothing, module_wearing_no_kind,
    only_a_default_export, pattern_matches_nothing, rule_evaluates_nothing,
    rule_reaches_outside_its_module, scope_matches_nothing, symbol_never_imported,
};

/// The concerns that need the repository, not just the configuration.
///
/// The slow half of the doctor, and the reason `CONFIG.md` calls the command
/// slower than `validate`: it walks, and parses the files the rules that ask
/// about contents apply to.
#[must_use]
pub fn examine_repository(
    root: &Utf8Path,
    config: &CompiledConfig,
    tree: &RepoTree,
) -> Vec<Concern> {
    let mut concerns = Vec::new();

    // Zipped with the engines, so applicability is asked of the same code
    // `check` asks. Re-deriving "does this rule cover this file?" here would
    // be a second implementation, and the doctor would eventually disagree
    // with the checker about which files a rule is even about.
    for module in config.modules() {
        module_scope_matches_nothing(module, tree, &mut concerns);
    }
    // Outside the loop above: a decision's scope is nothing to do with a
    // module, and a repository declaring decisions and no modules is the
    // ordinary case for one adopting them first.
    decision_scope_matches_nothing(config, tree, &mut concerns);

    let baseline = archwarden_api::baseline::Baseline::load(root)
        .ok()
        .flatten();

    decision_documents_out_of_date(root, config, &mut concerns);

    for (rule, engine) in config.rules().zip(archwarden_rules::engines_for(config)) {
        frozen_with_nothing_accepted(
            rule,
            engine.as_ref(),
            tree,
            baseline.as_ref(),
            &mut concerns,
        );
        scope_matches_nothing(rule, tree, &mut concerns);
        rule_reaches_outside_its_module(config, rule, tree, &mut concerns);
        pattern_matches_nothing(config, rule, tree, &mut concerns);
        symbol_never_imported(root, config, rule, engine.as_ref(), tree, &mut concerns);
        only_a_default_export(root, config, rule, engine.as_ref(), tree, &mut concerns);
        // Last, and it defers to everything above: reaching no file is what a
        // rule with a pattern matching nothing *does*, and saying it twice
        // makes one mistake look like two.
        rule_evaluates_nothing(rule, engine.as_ref(), tree, &mut concerns);
    }

    concerns
}

/// Every file the rule applies to.
fn in_scope<'a>(
    config: &'a CompiledConfig,
    rule: &'a CompiledRule,
    tree: &'a RepoTree,
) -> impl Iterator<Item = &'a archwarden_engine::walk::File> {
    tree.files().filter(move |file| {
        !config.is_ignored(&file.path) && rule.scope.contains_file(file.path.as_path())
    })
}

/// The facts of every source file the rule actually covers.
///
/// Applicability comes from the engine, not from the scope alone: a
/// `call-obligation` scoped to `src/*` with a `file_pattern` of
/// `^route\.post\.ts$` covers the POST routes and nothing else, and counting
/// the rest would have the doctor report a rule for files it never looks at.
///
/// Files that will not parse are skipped rather than reported: `check` says so
/// already, and the doctor is here to talk about the configuration.
fn facts_covered<'a>(
    root: &'a Utf8Path,
    config: &'a CompiledConfig,
    engine: &'a dyn archwarden_core::traits::RuleEngine,
    tree: &'a RepoTree,
) -> impl Iterator<Item = FileFacts> + 'a {
    tree.files()
        .filter(move |file| {
            !config.is_ignored(&file.path)
                && file.class == archwarden_core::path::FileClass::Source
                && engine.applies_to(&file.path)
        })
        .filter_map(|file| archwarden_engine::run::facts_of(root, &file.path).ok())
}

#[cfg(test)]
mod tests {
    use archwarden_core::compiled::CompiledAlternative;

    use super::*;

    /// A rule asking only for forms nobody enabled can never pass.
    ///
    /// Decision 31's other half. Without it a rule wanting a `struct` over a
    /// `.ts` file reports the file for exporting a `const`, which reads like a
    /// naming mistake and is a configuration one.
    #[test]
    fn a_kind_no_enabled_language_declares_is_a_concern() {
        let rule = rule(
            "rust-shaped",
            &["src/*"],
            CompiledRuleKind::Naming {
                file_pattern: Pattern::compile("^(?<name>.+)$").expect("valid"),
                dir_pattern: None,
                name_template: "{{pascal(name)}}".to_owned(),
                kind: KindFilter::OneOf(ExportTags::only(ExportKind::Struct)),
                annotation: Vec::new(),
                signature_hint: None,
                ignore_files: archwarden_core::glob::PathSet::default(),
            },
        );

        let concerns = examine(&config(vec![rule]));

        assert_eq!(concerns.len(), 1, "{concerns:?}");
        assert_eq!(concerns[0].code, "kind-no-language-declares");
        assert!(
            concerns[0].message.contains("`struct`"),
            "it names the form"
        );
        assert!(concerns[0].message.contains("`ts`"), "and what is enabled");
    }

    /// The concern names every language the config reads, not only the default.
    ///
    /// Found by mutation testing: deleting the `astro` and `rust` arms of the
    /// message broke nothing, because the only test enabled neither. A message
    /// that named the wrong set would send somebody to add a language they
    /// already have.
    #[test]
    fn the_concern_names_the_languages_the_config_actually_reads() {
        let rule = rule(
            "rust-shaped-again",
            &["src/*"],
            CompiledRuleKind::Naming {
                file_pattern: Pattern::compile("^(?<name>.+)$").expect("valid"),
                dir_pattern: None,
                name_template: "{{pascal(name)}}".to_owned(),
                kind: KindFilter::OneOf(ExportTags::only(ExportKind::Struct)),
                annotation: Vec::new(),
                signature_hint: None,
                ignore_files: archwarden_core::glob::PathSet::default(),
            },
        );

        // Astro on and Rust off: `struct` is still unreachable, and the
        // message has two languages to name rather than one.
        let compiled = config(vec![rule]).with_languages(archwarden_core::compiled::Languages {
            astro: true,
            rust: false,
        });
        let concerns = examine(&compiled);

        assert_eq!(concerns.len(), 1, "{concerns:?}");
        let message = &concerns[0].message;
        assert!(message.contains("`ts`"), "{message}");
        assert!(message.contains("`astro`"), "{message}");
        assert!(
            !message.contains("`rust`"),
            "and does not name one nobody turned on: {message}"
        );
    }

    /// A rule naming one form per language is a rule spanning both trees on
    /// purpose, and is left alone.
    ///
    /// The case a Tauri repository writes deliberately: `["const", "struct"]`
    /// over `src/**` and `src-tauri/src/**`, each half satisfying the form its
    /// own language spells. Reporting it would make the honest way to write
    /// one rule for two languages look like a mistake.
    #[test]
    fn a_kind_naming_a_form_from_each_language_is_not_a_concern() {
        let rule = rule(
            "both-trees",
            &["src/*"],
            CompiledRuleKind::Naming {
                file_pattern: Pattern::compile("^(?<name>.+)$").expect("valid"),
                dir_pattern: None,
                name_template: "{{pascal(name)}}".to_owned(),
                kind: KindFilter::OneOf(
                    ExportTags::only(ExportKind::Struct).with(ExportKind::Const),
                ),
                annotation: Vec::new(),
                signature_hint: None,
                ignore_files: archwarden_core::glob::PathSet::default(),
            },
        );

        assert!(examine(&config(vec![rule])).is_empty());
    }

    /// A rule asking for any form at all is asking for nothing unreachable.
    #[test]
    fn a_rule_naming_no_kind_is_not_a_concern() {
        let rule = rule(
            "any",
            &["src/*"],
            CompiledRuleKind::Naming {
                file_pattern: Pattern::compile("^(?<name>.+)$").expect("valid"),
                dir_pattern: None,
                name_template: "{{pascal(name)}}".to_owned(),
                kind: KindFilter::Any,
                annotation: Vec::new(),
                signature_hint: None,
                ignore_files: archwarden_core::glob::PathSet::default(),
            },
        );

        assert!(examine(&config(vec![rule])).is_empty());
    }
    use archwarden_core::compiled::{CompiledRuleKind, SkipScope};
    use archwarden_core::facts::{ExportKind, ExportTags, KindFilter};
    use archwarden_core::{
        compiled::{CompiledDecision, DecisionStatus, SkipDirs},
        glob::PathSet,
        hash::ContentHash,
        ids::DecisionId,
        level::Level,
        pattern::Pattern,
        scope::Scope,
    };

    fn rule(id: &str, scope: &[&str], kind: CompiledRuleKind) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(scope.iter().copied()).expect("valid scope"),
            kind,
        }
    }

    fn config(rules: Vec<CompiledRule>) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b"doctor"),
        )
    }

    fn with_ignore(rules: Vec<CompiledRule>, ignore: &[&str]) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::compile(ignore.iter().map(|g| (*g).to_owned())).expect("valid globs"),
            SkipDirs::default(),
            ContentHash::of(b"doctor"),
        )
    }

    fn with_skip(rules: Vec<CompiledRule>, scope: SkipScope) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::default(),
            SkipDirs {
                prefixes: vec!["_".to_owned()],
                globs: PathSet::default(),
                scope,
            },
            ContentHash::of(b"doctor"),
        )
    }

    fn boundary() -> CompiledRuleKind {
        CompiledRuleKind::ImportBoundary {
            forbid: PathSet::compile(["src/infra/**".to_owned()]).expect("valid globs"),
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

    fn naming(hint: Option<&str>, kind: KindFilter) -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile("^(?<name>[a-z]+)\\.ts$").expect("valid"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind,
            annotation: Vec::new(),
            signature_hint: hint.map(str::to_owned),
            ignore_files: archwarden_core::glob::PathSet::default(),
        }
    }

    /// A `naming` rule that spells its export from the directory as well, with
    /// the directory pattern varying per test.
    fn entity_naming(dir_pattern: &str) -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile("^(?<action>[a-z]+)\\.ts$").expect("valid"),
            dir_pattern: Some(Pattern::compile(dir_pattern).expect("valid")),
            name_template: "{{pascal(entity)}}{{pascal(action)}}".to_owned(),
            kind: KindFilter::Any,
            annotation: Vec::new(),
            signature_hint: None,
            ignore_files: archwarden_core::glob::PathSet::default(),
        }
    }

    fn structure(allowed: &[&str], warn: &[&str]) -> CompiledRuleKind {
        CompiledRuleKind::Structure {
            allowed_subfolders: Some(allowed.iter().map(|s| (*s).to_owned()).collect()),
            warn_subfolders: warn.iter().map(|s| (*s).to_owned()).collect(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
            filename_patterns: Vec::new(),
        }
    }

    fn spec_pair(subfolders: &[&str]) -> CompiledRuleKind {
        CompiledRuleKind::SpecPair {
            subfolders: subfolders.iter().map(|s| (*s).to_owned()).collect(),
            spec_markers: vec!["spec".to_owned()],
            ignore_files: PathSet::default(),
            spec_dirs: Vec::new(),
            require_non_empty_spec: false,
            skip_type_only: false,
        }
    }

    fn codes(config: &CompiledConfig) -> Vec<&'static str> {
        examine(config).into_iter().map(|c| c.code).collect()
    }

    /// A configuration with nothing wrong says nothing.
    #[test]
    fn a_sound_configuration_raises_nothing() {
        let sound = config(vec![
            rule("shape", &["src/*"], structure(&["types"], &[])),
            rule("spec", &["src/*"], spec_pair(&["types"])),
            rule(
                "name",
                &["src/*"],
                naming(
                    Some("(deps: Deps): Result"),
                    KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
                ),
            ),
        ]);

        assert!(examine(&sound).is_empty(), "{:?}", examine(&sound));
    }

    /// Decision 5: a walk-scoped skip hides files from every rule, and a
    /// boundary rule that cannot see a file cannot see the import landing on
    /// it.
    #[test]
    fn a_walk_scoped_skip_beside_a_boundary_rule_is_reported() {
        let risky = with_skip(
            vec![rule("boundary", &["src/**"], boundary())],
            SkipScope::Walk,
        );

        assert_eq!(codes(&risky), ["walk-skip-hides-imports"]);
        let concern = examine(&risky).remove(0);
        assert!(concern.message.contains("boundary"), "{concern:?}");
        assert!(
            concern.fix.contains("structure"),
            "the fix names the setting"
        );
        assert_eq!(
            concern.rule_id, None,
            "it is about the config, not one rule"
        );
    }

    /// The same skip without a boundary rule is fine: that is what
    /// `scope: "walk"` is for.
    #[test]
    fn a_walk_scoped_skip_alone_is_fine() {
        let fine = with_skip(
            vec![rule("shape", &["src/*"], structure(&["types"], &[]))],
            SkipScope::Walk,
        );

        assert!(examine(&fine).is_empty());
    }

    /// And the structure-scoped skip is fine beside a boundary rule, which is
    /// the arrangement the fix suggests.
    #[test]
    fn a_structure_scoped_skip_is_fine_beside_a_boundary_rule() {
        let fine = with_skip(
            vec![rule("boundary", &["src/**"], boundary())],
            SkipScope::Structure,
        );

        assert!(examine(&fine).is_empty());
    }

    /// Decision 6: a rule whose scope is inside an `ignore` entry can never
    /// fire, and a rule that never fires reads exactly like one that passes.
    #[test]
    fn a_scope_swallowed_by_ignore_is_reported() {
        let unreachable = with_ignore(
            vec![rule(
                "legacy",
                &["src/legacy/*"],
                structure(&["types"], &[]),
            )],
            &["src/legacy/**"],
        );

        assert_eq!(codes(&unreachable), ["unreachable-scope"]);
        let concern = examine(&unreachable).remove(0);
        assert_eq!(concern.rule_id.as_ref().map(RuleId::as_str), Some("legacy"));
        assert!(concern.message.contains("src/legacy/**"), "{concern:?}");
    }

    /// A rule only partly covered still fires for the rest, so it is not
    /// reported. The check errs towards silence: a doctor that cried wolf
    /// about working rules is a doctor nobody runs.
    #[test]
    fn a_scope_only_partly_ignored_is_not_reported() {
        let partial = with_ignore(
            vec![rule(
                "wide",
                &["src/legacy/*", "src/current/*"],
                structure(&["types"], &[]),
            )],
            &["src/legacy/**"],
        );

        assert!(examine(&partial).is_empty(), "{:?}", examine(&partial));
    }

    /// Containment is only claimed where it is certain. A glob in the ignore
    /// prefix could match anything, so nothing is concluded from it.
    #[test]
    fn a_glob_in_the_ignore_prefix_concludes_nothing() {
        let uncertain = with_ignore(
            vec![rule("r", &["src/legacy/*"], structure(&["types"], &[]))],
            &["src/*/**"],
        );

        assert!(examine(&uncertain).is_empty());
    }

    /// An exact ignore of the scope counts too.
    #[test]
    fn an_exact_ignore_of_the_scope_counts() {
        let exact = with_ignore(
            vec![rule("r", &["src/legacy"], structure(&["types"], &[]))],
            &["src/legacy"],
        );

        assert_eq!(codes(&exact), ["unreachable-scope"]);
    }

    /// A `spec-pair` watching a folder the structure rule forbids watches a
    /// folder that cannot exist.
    #[test]
    fn a_spec_folder_the_structure_rule_forbids_is_reported() {
        let mismatched = config(vec![
            rule("shape", &["src/*"], structure(&["types", "calcs"], &[])),
            rule("spec", &["src/*"], spec_pair(&["usecases"])),
        ]);

        assert_eq!(codes(&mismatched), ["spec-folder-not-allowed"]);
        let concern = examine(&mismatched).remove(0);
        assert!(concern.message.contains("usecases"), "{concern:?}");
        assert!(concern.message.contains("shape"), "it names the other rule");
    }

    /// A folder on the warn list exists, it is just discouraged. The spec rule
    /// pointed at it is not wrong.
    #[test]
    fn a_spec_folder_on_the_warn_list_is_fine() {
        let fine = config(vec![
            rule("shape", &["src/*"], structure(&["types"], &["shared"])),
            rule("spec", &["src/*"], spec_pair(&["shared"])),
        ]);

        assert!(examine(&fine).is_empty());
    }

    /// `.` is the scope directory itself, which no structure rule governs the
    /// existence of.
    #[test]
    fn the_scope_itself_is_never_reported() {
        let fine = config(vec![
            rule("shape", &["src/*"], structure(&["types"], &[])),
            rule("spec", &["src/*"], spec_pair(&["."])),
        ]);

        assert!(examine(&fine).is_empty());
    }

    /// With no structure rule over the same scope there is nothing to
    /// contradict, so nothing is claimed.
    #[test]
    fn a_spec_folder_with_no_structure_rule_is_not_reported() {
        let alone = config(vec![rule("spec", &["src/*"], spec_pair(&["usecases"]))]);

        assert!(examine(&alone).is_empty());
    }

    /// Issue #41's other half. `explain` says it itself now, but this is the
    /// command that audits a configuration, and a rule with nothing to enforce
    /// is exactly what it is for — it is invisible from the outside, since a
    /// rule enforcing nothing looks like a repository that satisfies it.
    #[test]
    fn a_structure_rule_with_no_constraint_at_all_is_a_concern() {
        let toothless = config(vec![rule(
            "toothless",
            &["src/*"],
            CompiledRuleKind::Structure {
                allowed_subfolders: None,
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: Vec::new(),
                filename_patterns: Vec::new(),
            },
        )]);

        let concerns = examine(&toothless);
        assert_eq!(concerns.len(), 1, "{concerns:?}");
        assert_eq!(
            concerns.first().expect("one").code,
            "rule-constrains-nothing"
        );
    }

    /// Issue #168, the half found by reading `verify-rules` output. It refuses
    /// a chokepoint with no callee and says *"`config doctor` reports a rule
    /// that constrains nothing"* — and `doctor` said `No concerns`. A promise
    /// one command makes about another has to be kept by the other.
    ///
    /// `chokepoint` is the second kind whose fields are all optional, which is
    /// what this check was written for.
    #[test]
    fn a_chokepoint_guarding_no_callee_constrains_nothing() {
        let toothless = config(vec![rule(
            "guards-nothing",
            &["src/*"],
            CompiledRuleKind::Chokepoint {
                callee: Vec::new(),
                only_in: Scope::compile(["src/config/**"]).expect("valid scope"),
            },
        )]);

        let concerns = examine(&toothless);
        let found = concerns
            .iter()
            .find(|c| c.code == "rule-constrains-nothing")
            .unwrap_or_else(|| panic!("{concerns:?}"));
        assert_eq!(
            found.rule_id.as_ref().map(RuleId::as_str),
            Some("guards-nothing")
        );

        // One callee is enough. An empty `only_in` is *not* the same thing --
        // it is the rule "nobody here may", which is a constraint.
        let guarding = config(vec![rule(
            "the-domain-is-testable",
            &["src/*"],
            CompiledRuleKind::Chokepoint {
                callee: vec!["Date.now".to_owned()],
                only_in: Scope::compile(std::iter::empty::<&str>()).expect("valid scope"),
            },
        )]);

        assert!(
            examine(&guarding)
                .iter()
                .all(|c| c.code != "rule-constrains-nothing"),
            "{:?}",
            examine(&guarding)
        );
    }

    /// Each of the three fields on its own is something to enforce, and an
    /// empty `allowed_subfolders` counts — after issue #40 it is the rule
    /// "no subfolder may exist here".
    #[test]
    fn any_one_constraint_is_enough_to_have_nothing_to_report() {
        let cases = [
            (Some(Vec::new()), Vec::new(), Vec::new()),
            (Some(vec!["types".to_owned()]), Vec::new(), Vec::new()),
            (None, vec!["legacy".to_owned()], Vec::new()),
            (
                None,
                Vec::new(),
                vec![Pattern::compile("^[a-z-]+\\.ts$").expect("valid")],
            ),
        ];

        for (allowed, warn, filenames) in cases {
            let configured = config(vec![rule(
                "shape",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: allowed.clone(),
                    warn_subfolders: warn.clone(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: filenames.clone(),
                },
            )]);

            assert!(
                examine(&configured).is_empty(),
                "{allowed:?} {warn:?} {}",
                filenames.len()
            );
        }
    }

    /// Counted, not listed: a config with forty rules and no `why` anywhere
    /// would bury every other concern, and burying them is the same as not
    /// reporting them.
    #[test]
    fn rules_without_a_reason_are_counted_once() {
        let mut reasoned = rule("shape", &["src/*"], structure(&["types"], &[]));
        reasoned.why = Some("entities are the only thing published".to_owned());
        let mixed = config(vec![
            reasoned,
            rule("second", &["src/*"], structure(&["types"], &[])),
            rule("third", &["src/*"], structure(&["types"], &[])),
        ]);

        let concerns = examine(&mixed);
        let counted: Vec<&Concern> = concerns
            .iter()
            .filter(|c| c.code == "rules-without-a-reason")
            .collect();

        assert_eq!(counted.len(), 1, "{concerns:?}");
        assert!(counted[0].message.contains("1 of 3"), "{:?}", counted[0]);
        assert!(counted[0].message.contains("2 do not"), "{:?}", counted[0]);
    }

    /// A project that has never used the field has not adopted the practice,
    /// and nagging it about a convention it never chose is how a command that
    /// gives advice becomes one people stop running.
    #[test]
    fn a_config_that_never_says_why_is_not_nagged() {
        let silent = config(vec![
            rule("shape", &["src/*"], structure(&["types"], &[])),
            rule("second", &["src/*"], structure(&["types"], &[])),
        ]);

        assert!(
            examine(&silent)
                .iter()
                .all(|c| c.code != "rules-without-a-reason"),
            "{:?}",
            examine(&silent)
        );
    }

    /// And a config where every rule says why has nothing to be told either.
    #[test]
    fn a_config_that_always_says_why_is_not_nagged() {
        let mut first = rule("shape", &["src/*"], structure(&["types"], &[]));
        first.why = Some("a".to_owned());
        let mut second = rule("second", &["src/*"], structure(&["types"], &[]));
        second.why = Some("b".to_owned());

        let complete = config(vec![first, second]);

        assert!(
            examine(&complete)
                .iter()
                .all(|c| c.code != "rules-without-a-reason"),
            "{:?}",
            examine(&complete)
        );
    }

    /// Every concern carries a level, and one of them is louder than the rest.
    ///
    /// The `doctor` had no notion of severity until 0.21: sixteen checks in a
    /// flat list, all equally advisory. Issue #100 needs two of its three to be
    /// advice and one to be a contradiction, so the field exists — but the
    /// sixteen that came before stay `warning`, because that is what every one
    /// of them has always been, and quietly promoting some of them would
    /// change what an existing user sees over a release that is about
    /// something else.
    #[test]
    fn a_concern_is_a_warning_unless_it_is_a_contradiction() {
        let contradictory = config(vec![serving("shape", "ADR-014")])
            .with_decisions(vec![adr("ADR-014", DecisionStatus::Superseded)]);

        let concerns = examine(&contradictory);
        let superseded = concerns
            .iter()
            .find(|c| c.code == "superseded-decision-still-enforced")
            .unwrap_or_else(|| panic!("{concerns:?}"));
        assert_eq!(superseded.level, Level::Error);

        assert!(
            examine(&with_ignore(
                vec![rule(
                    "legacy",
                    &["src/legacy/*"],
                    structure(&["types"], &[])
                )],
                &["src/legacy/**"],
            ))
            .iter()
            .all(|c| c.level == Level::Warning),
            "the checks that came before 0.21 are unchanged"
        );
    }

    /// It reaches the text, where a reader needs to tell the two apart, and the
    /// JSON, where a consumer branches on it.
    #[test]
    fn the_level_reaches_both_renderings() {
        let contradictory = config(vec![serving("shape", "ADR-014")])
            .with_decisions(vec![adr("ADR-014", DecisionStatus::Superseded)]);
        let concerns = examine(&contradictory);

        let mut text = Vec::new();
        render(&concerns, crate::report::Format::Text, &mut text);
        let text = String::from_utf8(text).expect("UTF-8");
        assert!(text.contains("error"), "{text}");

        let mut json = Vec::new();
        render(&concerns, crate::report::Format::Json, &mut json);
        let parsed: serde_json::Value = serde_json::from_slice(&json).expect("valid JSON");
        assert!(
            parsed["concerns"]
                .as_array()
                .expect("a list")
                .iter()
                .any(|c| c["level"] == "error"),
            "{parsed}"
        );
    }

    /// Issue #102. A `frozen` rule reports every file under it, which is the
    /// design — the baseline holds the accepted set. Turn one on without
    /// running `archwarden baseline` and the first `check` is a wall of errors
    /// about the past. `check` still reports them, honestly; this is where the
    /// missing second step is named, with the command to run.
    #[test]
    fn a_freeze_the_baseline_accepts_nothing_of_is_reported() {
        let (concerns, _guard) = repository_concerns(
            &["packages/legacy/a.ts", "packages/legacy/b.ts"],
            "legacy-is-closed",
            None,
        );

        let found = concerns
            .iter()
            .find(|c| c.code == "frozen-with-nothing-accepted")
            .unwrap_or_else(|| panic!("{concerns:?}"));

        assert_eq!(found.level, Level::Warning);
        assert!(found.message.contains("2 files"), "{found:?}");
        assert!(found.fix.contains("archwarden baseline"), "{found:?}");
    }

    /// And it goes quiet once the baseline carries the rule, which is the
    /// whole adoption path: two steps, and the second one said out loud until
    /// it is taken.
    #[test]
    fn a_freeze_the_baseline_accepts_is_not_reported() {
        let (concerns, _guard) = repository_concerns(
            &["packages/legacy/a.ts"],
            "legacy-is-closed",
            Some("legacy-is-closed"),
        );

        assert!(
            concerns
                .iter()
                .all(|c| c.code != "frozen-with-nothing-accepted"),
            "{concerns:?}"
        );
    }

    /// A freeze whose scope reaches no file is `scope-matches-nothing`'s
    /// sentence. Saying it twice in two voices is worse than saying it once.
    #[test]
    fn a_freeze_over_nothing_is_left_to_the_other_check() {
        let (concerns, _guard) = repository_concerns(&["src/a.ts"], "legacy-is-closed", None);

        assert!(
            concerns
                .iter()
                .all(|c| c.code != "frozen-with-nothing-accepted"),
            "{concerns:?}"
        );
    }

    /// A repository with the named files, a `frozen` rule over
    /// `packages/legacy/**`, and optionally a baseline accepting one entry
    /// under `accepts`.
    fn repository_concerns(
        files: &[&str],
        rule_id: &str,
        accepts: Option<&str>,
    ) -> (Vec<Concern>, tempfile::TempDir) {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8PathBuf::from_path_buf(guard.path().to_path_buf())
            .expect("temp path is UTF-8");

        for file in files {
            let at = root.join(file);
            std::fs::create_dir_all(at.parent().expect("a file has a parent")).expect("dirs");
            std::fs::write(&at, "export const x = 1;\n").expect("write");
        }
        if let Some(rule) = accepts {
            std::fs::create_dir_all(root.join(".archwarden")).expect("dirs");
            std::fs::write(
                root.join(archwarden_api::baseline::BASELINE_PATH),
                format!(
                    r#"{{"version":0,"accepted":[{{"rule":"{rule}","path":"{}","note":"pre-existing"}}]}}"#,
                    files[0]
                ),
            )
            .expect("write");
        }

        let frozen = CompiledRule {
            id: RuleId::new(rule_id).expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["packages/legacy/**"]).expect("valid scope"),
            kind: CompiledRuleKind::Frozen,
        };
        let compiled = config(vec![frozen]);
        let tree = archwarden_engine::walk::walk(&root, &compiled).expect("walks");

        (examine_repository(&root, &compiled, &tree), guard)
    }

    /// A decision, for the checks below.
    fn adr(id: &str, status: DecisionStatus) -> CompiledDecision {
        CompiledDecision {
            scope: None,
            why_not_enforceable: None,
            id: DecisionId::new(id).expect("valid id"),
            title: "A wall".to_owned(),
            why: None,
            link: None,
            status,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            alternatives: Vec::new(),
        }
    }

    /// A rule pointing at a decision.
    fn serving(name: &str, decision: &str) -> CompiledRule {
        let mut rule = rule(name, &["src/*"], structure(&["types"], &[]));
        rule.decision = Some(DecisionId::new(decision).expect("valid id"));
        rule
    }

    /// Issue #100. `check` says nothing about an undeclared rule — a
    /// repository's build must not fail because its config is
    /// under-documented, and a gate that failed for that is one people turn
    /// off. `doctor` is where it belongs, counted once, exactly as
    /// `rules-without-a-reason` is.
    #[test]
    fn rules_without_a_decision_are_counted_once() {
        let mixed = config(vec![
            serving("shape", "ADR-014"),
            rule("second", &["src/*"], structure(&["types"], &[])),
            rule("third", &["src/*"], structure(&["types"], &[])),
        ])
        .with_decisions(vec![adr("ADR-014", DecisionStatus::Accepted)]);

        let concerns = examine(&mixed);
        let counted: Vec<&Concern> = concerns
            .iter()
            .filter(|c| c.code == "rule-without-a-decision")
            .collect();

        assert_eq!(counted.len(), 1, "{concerns:?}");
        assert!(counted[0].message.contains("1 of 3"), "{:?}", counted[0]);
        assert!(counted[0].message.contains("2 do not"), "{:?}", counted[0]);
    }

    /// And only once at least one rule names one. Every existing configuration
    /// has zero decisions on the day this ships, and a tool that greets them
    /// with a complaint is one they stop running.
    #[test]
    fn a_config_that_names_no_decisions_at_all_is_not_nagged() {
        let silent = config(vec![
            rule("shape", &["src/*"], structure(&["types"], &[])),
            rule("second", &["src/*"], structure(&["types"], &[])),
        ]);

        assert!(
            examine(&silent)
                .iter()
                .all(|c| c.code != "rule-without-a-decision"),
            "{:?}",
            examine(&silent)
        );
    }

    /// A config where every rule names one has nothing to be told either.
    #[test]
    fn a_config_where_every_rule_decides_is_not_nagged() {
        let complete = config(vec![
            serving("shape", "ADR-014"),
            serving("second", "ADR-014"),
        ])
        .with_decisions(vec![adr("ADR-014", DecisionStatus::Accepted)]);

        assert!(
            examine(&complete)
                .iter()
                .all(|c| c.code != "rule-without-a-decision"),
            "{:?}",
            examine(&complete)
        );
    }

    /// Issue #115. Now that supersession is an edge, this check can name what
    /// replaced the decision — and that is what turns it from a contradiction
    /// into an instruction.
    ///
    /// A second check was written for this and deleted: *"the new decision has
    /// no rules while the old one still does"* fires under exactly the
    /// condition this one does, so it was one mistake reported in two voices.
    /// One check, saying more, is the version that survives.
    #[test]
    fn a_supersession_the_rules_did_not_follow_names_what_replaced_it() {
        let config = config(vec![serving("old-rule", "ADR-009")]).with_decisions(vec![
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-009").expect("valid"),
                title: "the old way".to_owned(),
                why: None,
                link: None,
                status: archwarden_core::compiled::DecisionStatus::Superseded,
                supersedes: Vec::new(),
                superseded_by: vec![DecisionId::new("ADR-031").expect("valid")],
                alternatives: Vec::new(),
            },
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-031").expect("valid"),
                title: "the new way".to_owned(),
                why: None,
                link: None,
                status: archwarden_core::compiled::DecisionStatus::Accepted,
                supersedes: vec![DecisionId::new("ADR-009").expect("valid")],
                superseded_by: Vec::new(),
                alternatives: Vec::new(),
            },
        ]);

        let found = examine(&config)
            .into_iter()
            .find(|c| c.code == "superseded-decision-still-enforced")
            .expect("the renamed decision is reported");

        assert!(
            found.message.contains("superseded by `ADR-031`"),
            "not just that it was replaced, but by what: {found:?}"
        );
        assert!(found.message.contains("old-rule"), "{found:?}");
        assert!(
            found.fix.contains("point those rules at `ADR-031`"),
            "and the fix is an instruction rather than a dilemma: {found:?}"
        );
    }

    /// And it goes quiet the moment the rules follow, which is what finishing
    /// the replacement means.
    #[test]
    fn a_supersession_the_rules_followed_is_not_reported() {
        let config = config(vec![serving("new-rule", "ADR-031")]).with_decisions(vec![
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-009").expect("valid"),
                title: "the old way".to_owned(),
                why: None,
                link: None,
                status: archwarden_core::compiled::DecisionStatus::Superseded,
                supersedes: Vec::new(),
                superseded_by: vec![DecisionId::new("ADR-031").expect("valid")],
                alternatives: Vec::new(),
            },
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-031").expect("valid"),
                title: "the new way".to_owned(),
                why: None,
                link: None,
                status: archwarden_core::compiled::DecisionStatus::Accepted,
                supersedes: vec![DecisionId::new("ADR-009").expect("valid")],
                superseded_by: Vec::new(),
                alternatives: Vec::new(),
            },
        ]);

        assert!(
            examine(&config)
                .iter()
                .all(|c| c.code != "superseded-decision-still-enforced"),
            "{:?}",
            examine(&config)
        );
    }

    /// The check most worth having, and the reason `status` is not decoration:
    /// a decision recorded as replaced, with rules still enforcing it, is a
    /// config saying two things at once.
    #[test]
    fn a_superseded_decision_whose_rules_still_fire_is_an_error() {
        let contradictory = config(vec![serving("shape", "ADR-014")])
            .with_decisions(vec![adr("ADR-014", DecisionStatus::Superseded)]);

        let concerns = examine(&contradictory);
        let found = concerns
            .iter()
            .find(|c| c.code == "superseded-decision-still-enforced")
            .unwrap_or_else(|| panic!("{concerns:?}"));

        assert!(found.message.contains("ADR-014"), "{found:?}");
        assert!(
            found.message.contains("shape"),
            "the rule is named: {found:?}"
        );
    }

    /// The plurals, in both sentences that carry a count. A message reading
    /// "2 rule still enforces it" is the kind of thing a reader stops trusting
    /// the rest of.
    #[test]
    fn the_counts_read_as_english_in_both_numbers() {
        let two = config(vec![
            serving("shape", "ADR-014"),
            serving("sealed", "ADR-014"),
        ])
        .with_decisions(vec![
            adr("ADR-014", DecisionStatus::Superseded),
            adr("ADR-020", DecisionStatus::Accepted),
            adr("ADR-021", DecisionStatus::Accepted),
        ]);
        let concerns = examine(&two);

        let superseded = concerns
            .iter()
            .find(|c| c.code == "superseded-decision-still-enforced")
            .unwrap_or_else(|| panic!("{concerns:?}"));
        assert!(
            superseded.message.contains("2 rules still enforce it"),
            "{superseded:?}"
        );

        let orphans = concerns
            .iter()
            .find(|c| c.code == "decision-nobody-enforces")
            .unwrap_or_else(|| panic!("{concerns:?}"));
        assert!(
            orphans.message.contains("2 decisions are declared"),
            "{orphans:?}"
        );

        // And the singular, which is the branch the plural rule exists for.
        let one = config(vec![serving("shape", "ADR-014")]).with_decisions(vec![
            adr("ADR-014", DecisionStatus::Superseded),
            adr("ADR-020", DecisionStatus::Accepted),
        ]);
        let concerns = examine(&one);
        assert!(
            concerns
                .iter()
                .any(|c| c.message.contains("1 rule still enforces it")),
            "{concerns:?}"
        );
        assert!(
            concerns
                .iter()
                .any(|c| c.message.contains("1 decision is declared")),
            "{concerns:?}"
        );
    }

    /// The claim and a rule, said at once. Issue #160: a decision carrying
    /// `why_not_enforceable` is asserting that no rule *can* keep it, and a
    /// rule pointing at it says the opposite. One of the two is stale and only
    /// the author knows which.
    #[test]
    fn a_decision_claiming_nothing_can_keep_it_while_a_rule_does_is_reported() {
        let mut unenforceable = adr("ADR-014", DecisionStatus::Accepted);
        unenforceable.why_not_enforceable = Some("it is about tone in reviews".to_owned());

        let contradictory = config(vec![serving("shape", "ADR-014")]).with_decisions(vec![
            unenforceable,
            adr("ADR-020", DecisionStatus::Accepted),
        ]);

        let concerns = examine(&contradictory);
        let found = concerns
            .iter()
            .find(|c| c.code == "unenforceable-but-a-rule-keeps-it")
            .unwrap_or_else(|| panic!("{concerns:?}"));

        assert_eq!(found.level, Level::Error, "{found:?}");
        assert!(found.message.contains("ADR-014"), "{found:?}");
        assert!(
            found.message.contains("shape"),
            "the rule is named: {found:?}"
        );
    }

    /// The half of that check that is easy to lose: it must be the rules
    /// pointing at *this* decision that are counted. A config where every
    /// decision has rules and only one claims unenforceability would report
    /// every time if the match were dropped.
    #[test]
    fn a_rule_serving_a_different_decision_does_not_trigger_the_claim() {
        let mut unenforceable = adr("ADR-014", DecisionStatus::Accepted);
        unenforceable.why_not_enforceable = Some("it is about tone in reviews".to_owned());

        let unrelated = config(vec![serving("shape", "ADR-020")]).with_decisions(vec![
            unenforceable,
            adr("ADR-020", DecisionStatus::Accepted),
        ]);

        let concerns = examine(&unrelated);
        assert!(
            concerns
                .iter()
                .all(|c| c.code != "unenforceable-but-a-rule-keeps-it"),
            "{concerns:?}"
        );
    }

    /// And the claim on its own is the whole point of #160 -- it is how a
    /// decision stops being nagged about by `decision-nobody-enforces`.
    #[test]
    fn the_claim_silences_the_orphan_report_and_says_nothing_else() {
        let mut unenforceable = adr("ADR-020", DecisionStatus::Accepted);
        unenforceable.why_not_enforceable = Some("no parser sees a code review".to_owned());

        let declared = config(vec![serving("shape", "ADR-014")]).with_decisions(vec![
            adr("ADR-014", DecisionStatus::Accepted),
            unenforceable,
        ]);

        let concerns = examine(&declared);
        assert!(
            concerns.iter().all(|c| {
                c.code != "decision-nobody-enforces"
                    && c.code != "unenforceable-but-a-rule-keeps-it"
            }),
            "{concerns:?}"
        );
    }

    /// Issue #162, the push half. `config explain` ends with "Do not propose
    /// it again", and it can only say that to somebody who already knows the
    /// id -- which the person about to propose the losing option is not. This
    /// catches the duplicate at the moment it is written.
    #[test]
    fn two_decisions_rejecting_the_same_option_are_reported() {
        let mut first = adr("ADR-014", DecisionStatus::Accepted);
        first.alternatives = vec![CompiledAlternative {
            option: "a single layer".to_owned(),
            why_not: "the domain would import the transport".to_owned(),
            refused_by: None,
        }];
        let mut second = adr("ADR-020", DecisionStatus::Accepted);
        second.alternatives = vec![CompiledAlternative {
            option: "one layer, single".to_owned(),
            why_not: "we tried it".to_owned(),
            refused_by: None,
        }];

        let concerns = examine(&config(Vec::new()).with_decisions(vec![first, second]));
        let found = concerns
            .iter()
            .find(|c| c.code == "decision-may-duplicate")
            .unwrap_or_else(|| panic!("{concerns:?}"));

        assert_eq!(found.level, Level::Warning, "{found:?}");
        assert!(found.message.contains("ADR-020"), "{found:?}");
        assert!(found.message.contains("ADR-014"), "{found:?}");
    }

    /// Superseding is the sanctioned way to say the same thing twice: the
    /// later decision is *about* the earlier one. Reporting the pair would
    /// punish recording the succession, which is the record #114 exists for.
    #[test]
    fn a_superseding_decision_repeating_its_predecessor_is_not_reported() {
        let mut old = adr("ADR-014", DecisionStatus::Superseded);
        old.alternatives = vec![CompiledAlternative {
            option: "a single layer".to_owned(),
            why_not: "the domain would import the transport".to_owned(),
            refused_by: None,
        }];
        old.superseded_by = vec![DecisionId::new("ADR-020").expect("valid")];
        let mut new = adr("ADR-020", DecisionStatus::Accepted);
        new.alternatives = vec![CompiledAlternative {
            option: "a single layer".to_owned(),
            why_not: "still true".to_owned(),
            refused_by: None,
        }];
        new.supersedes = vec![DecisionId::new("ADR-014").expect("valid")];

        // Both orders, because "earlier" here is declaration order and a
        // config is free to list the superseding decision first. Only one of
        // the two directions is true for a given pair, so a check that
        // demanded both would report every succession written that way.
        let concerns = examine(&config(Vec::new()).with_decisions(vec![old.clone(), new.clone()]));
        assert!(
            concerns.iter().all(|c| c.code != "decision-may-duplicate"),
            "{concerns:?}"
        );

        let mut only_forward = new.clone();
        only_forward.supersedes = vec![DecisionId::new("ADR-014").expect("valid")];
        let mut silent = old.clone();
        silent.superseded_by = Vec::new();
        let reversed =
            examine(&config(Vec::new()).with_decisions(vec![only_forward, silent.clone()]));
        assert!(
            reversed.iter().all(|c| c.code != "decision-may-duplicate"),
            "declared out of order: {reversed:?}"
        );

        // And with the succession recorded nowhere, the pair is reported --
        // which is what the exemption is an exemption from.
        let mut orphan = new;
        orphan.supersedes = Vec::new();
        let unrecorded = examine(&config(Vec::new()).with_decisions(vec![silent, orphan]));
        assert!(
            unrecorded
                .iter()
                .any(|c| c.code == "decision-may-duplicate"),
            "{unrecorded:?}"
        );
    }

    /// And two decisions that merely share vocabulary are not duplicates. The
    /// concern lives in a gate, and a gate that cries wolf is one somebody
    /// turns off -- which is why the push is stricter than the pull.
    #[test]
    fn two_decisions_sharing_a_word_are_left_alone() {
        let mut layers = adr("ADR-014", DecisionStatus::Accepted);
        layers.title = "Four layers plus System".to_owned();
        let mut packages = adr("ADR-020", DecisionStatus::Accepted);
        packages.title = "One package per bounded context".to_owned();
        packages.alternatives = vec![CompiledAlternative {
            option: "one package".to_owned(),
            why_not: "the boundaries stop being enforceable".to_owned(),
            refused_by: None,
        }];

        let concerns = examine(&config(Vec::new()).with_decisions(vec![layers, packages]));
        assert!(
            concerns.iter().all(|c| c.code != "decision-may-duplicate"),
            "{concerns:?}"
        );
    }

    /// A superseded decision nothing enforces is a tidy record of history, not
    /// a contradiction. Reporting it would punish keeping the record.
    #[test]
    fn a_superseded_decision_nothing_enforces_is_fine() {
        let historical = config(vec![rule("shape", &["src/*"], structure(&["types"], &[]))])
            .with_decisions(vec![adr("ADR-014", DecisionStatus::Superseded)]);

        assert!(
            examine(&historical)
                .iter()
                .all(|c| c.code != "superseded-decision-still-enforced"),
            "{:?}",
            examine(&historical)
        );
    }

    /// `proposed` is silent, deliberately: a decision under trial with rules
    /// already running is how one is trialled, and reporting it would nag the
    /// practice this whole feature is trying to encourage.
    #[test]
    fn a_proposed_decision_with_rules_is_not_reported() {
        let trialled = config(vec![serving("shape", "ADR-014")])
            .with_decisions(vec![adr("ADR-014", DecisionStatus::Proposed)]);

        assert!(
            examine(&trialled)
                .iter()
                .all(|c| c.code != "superseded-decision-still-enforced"),
            "{:?}",
            examine(&trialled)
        );
    }

    /// The mirror of `module-nobody-references`: an opinion with a name that
    /// nothing keeps. A preset ships decisions, `disable` takes their rules
    /// away, and what is left is a config describing an architecture it does
    /// not enforce.
    #[test]
    fn a_decision_no_rule_implements_is_reported() {
        let orphaned = config(vec![serving("shape", "ADR-014")]).with_decisions(vec![
            adr("ADR-014", DecisionStatus::Accepted),
            adr("ADR-020", DecisionStatus::Accepted),
        ]);

        let concerns = examine(&orphaned);
        let found = concerns
            .iter()
            .find(|c| c.code == "decision-nobody-enforces")
            .unwrap_or_else(|| panic!("{concerns:?}"));

        assert!(found.message.contains("ADR-020"), "{found:?}");
        assert!(
            !found.message.contains("ADR-014"),
            "the one that is enforced is not named: {found:?}"
        );
    }

    /// A rule that constrains subfolder names by shape has plenty to enforce,
    /// and names none of the three fields this check started out looking at.
    #[test]
    fn a_subfolder_pattern_alone_is_something_to_enforce() {
        let by_shape = config(vec![rule(
            "licao-nome-da-pasta",
            &["projetos"],
            CompiledRuleKind::Structure {
                allowed_subfolders: None,
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: vec![Pattern::compile(r"^\d{2}-[a-z0-9-]+$").expect("valid")],
                filename_patterns: Vec::new(),
            },
        )]);

        assert!(examine(&by_shape).is_empty(), "{:?}", examine(&by_shape));
    }

    /// A structure rule that names no list says nothing about folders, so it
    /// cannot contradict a `spec-pair` rule that looks in one.
    #[test]
    fn a_structure_rule_that_names_no_list_constrains_nothing() {
        let open = config(vec![
            rule(
                "shape",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: None,
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            ),
            rule("spec", &["src/*"], spec_pair(&["usecases"])),
        ]);

        // It does earn `rule-constrains-nothing`, which is a different
        // complaint about the same rule; what it must not earn is a claim that
        // it contradicts the spec folder.
        assert!(
            examine(&open)
                .iter()
                .all(|concern| concern.code != "spec-folder-not-allowed"),
            "{:?}",
            examine(&open)
        );
    }

    /// An empty list is the opposite of that: it permits no subfolder at all,
    /// so a `spec-pair` rule looking for specs in `usecases` is looking
    /// somewhere the structure rule forbids. Issue #40 — the case that used to
    /// be indistinguishable from the one above.
    #[test]
    fn an_empty_allowed_list_contradicts_a_spec_folder() {
        let closed = config(vec![
            rule("shape", &["src/*"], structure(&[], &[])),
            rule("spec", &["src/*"], spec_pair(&["usecases"])),
        ]);

        let concerns = examine(&closed);
        assert_eq!(concerns.len(), 1, "{concerns:?}");
        assert_eq!(
            concerns.first().expect("one").code,
            "spec-folder-not-allowed"
        );
    }

    /// The structure rule that matters is the one over the *same* scope. With
    /// several rules on one scope, the first one found must not be mistaken
    /// for it.
    #[test]
    fn the_structure_rule_is_matched_by_scope_not_by_position() {
        let ordered = config(vec![
            rule(
                "name",
                &["src/*"],
                naming(
                    None,
                    KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
                ),
            ),
            rule("shape", &["src/*"], structure(&["types"], &[])),
            rule("spec", &["src/*"], spec_pair(&["usecases"])),
        ]);

        assert_eq!(
            codes(&ordered),
            ["spec-folder-not-allowed"],
            "the naming rule sitting first must not stand in for the structure rule"
        );
    }

    /// And a structure rule over a *different* scope says nothing about this
    /// one. Reading it as governing would invent a concern about a
    /// configuration that is correct.
    #[test]
    fn a_structure_rule_elsewhere_is_not_borrowed() {
        let unrelated = config(vec![
            rule("other-shape", &["packages/*"], structure(&["types"], &[])),
            rule("spec", &["src/*"], spec_pair(&["usecases"])),
        ]);

        assert!(examine(&unrelated).is_empty(), "{:?}", examine(&unrelated));
    }

    /// The M7b finding: `scaffold` writes the hint straight after the keyword,
    /// so an arrow hint under a `function` rule emits a line that does not
    /// compile.
    #[test]
    fn an_arrow_hint_under_a_function_rule_is_reported() {
        let mismatched = config(vec![rule(
            "name",
            &["src/*"],
            naming(
                Some("(deps: Deps) => Result"),
                KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            ),
        )]);

        assert_eq!(codes(&mismatched), ["hint-disagrees-with-kind"]);
        let concern = examine(&mismatched).remove(0);
        assert!(concern.message.contains("does not compile"), "{concern:?}");
        assert!(concern.fix.contains("call signature"), "{concern:?}");
    }

    /// The same hint under a rule that accepts arrows is exactly right.
    #[test]
    fn an_arrow_hint_under_an_arrow_rule_is_fine() {
        let fine = config(vec![rule(
            "name",
            &["src/*"],
            naming(
                Some("(deps: Deps) => Result"),
                KindFilter::OneOf(ExportTags::only(ExportKind::Arrow)),
            ),
        )]);

        assert!(examine(&fine).is_empty());
    }

    /// A rule accepting either form is not wrong whichever way the hint reads,
    /// so nothing is claimed.
    #[test]
    fn a_rule_accepting_either_form_is_not_second_guessed() {
        let permissive = config(vec![rule(
            "name",
            &["src/*"],
            naming(
                Some("(deps: Deps) => Result"),
                KindFilter::OneOf(ExportTags::only(ExportKind::Function).with(ExportKind::Arrow)),
            ),
        )]);

        assert!(examine(&permissive).is_empty());
    }

    /// No hint, nothing to disagree with.
    #[test]
    fn a_rule_without_a_hint_is_not_reported() {
        let bare = config(vec![rule(
            "name",
            &["src/*"],
            naming(
                None,
                KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            ),
        )]);

        assert!(examine(&bare).is_empty());
    }

    // --- checks that need the repository --------------------------------

    fn tree_at(entries: &[(&str, &str)]) -> (tempfile::TempDir, camino::Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root =
            camino::Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
                .expect("temp path is UTF-8");

        for (relative, contents) in entries {
            let file = root.join(relative);
            std::fs::create_dir_all(file.parent().expect("a file has a parent"))
                .expect("create dirs");
            std::fs::write(&file, contents).expect("write file");
        }

        (dir, root)
    }

    fn repository_codes(entries: &[(&str, &str)], config: &CompiledConfig) -> Vec<&'static str> {
        let (guard, root) = tree_at(entries);
        let tree = archwarden_engine::walk::walk(&root, config).expect("walks");
        let codes = examine_repository(&root, config, &tree)
            .into_iter()
            .map(|c| c.code)
            .collect();
        drop(guard);
        codes
    }

    fn module(id: &str, scope: Option<&str>) -> archwarden_core::compiled::CompiledModule {
        archwarden_core::compiled::CompiledModule {
            id: archwarden_core::ids::ModuleId::new(id).expect("valid id"),
            kind: None,
            scope: scope.map(|s| Scope::compile([s]).expect("valid scope")),
        }
    }

    fn in_module(mut rule: CompiledRule, id: &str) -> CompiledRule {
        rule.module = Some(archwarden_core::ids::ModuleId::new(id).expect("valid id"));
        rule
    }

    /// A module wearing no kind, where kinds are used, is outside every rule
    /// that quantifies over them. Silently — which is the omission the
    /// quantifier was written to remove, arriving through the config instead.
    #[test]
    fn a_module_with_no_kind_is_reported_when_kinds_are_used() {
        let mut app = module("api", Some("apps/api/**"));
        app.kind = Some("app".to_owned());
        let config = config(Vec::new())
            .with_modules(vec![app, module("orders", Some("packages/orders/**"))]);

        let codes: Vec<&str> = examine(&config).into_iter().map(|c| c.code).collect();
        assert!(codes.contains(&"module-wears-no-kind"), "{codes:?}");
    }

    /// And a config that never uses kinds is not missing them.
    #[test]
    fn a_config_that_uses_no_kinds_is_not_told_to() {
        let config = config(Vec::new()).with_modules(vec![module("orders", Some("packages/**"))]);

        let codes: Vec<&str> = examine(&config).into_iter().map(|c| c.code).collect();
        assert!(!codes.contains(&"module-wears-no-kind"), "{codes:?}");
    }

    /// The module *wearing* a kind is not reported for not wearing one.
    ///
    /// The test above asserts the concern appears and cannot see which module
    /// it is about, so a check that reported every module — including the one
    /// that is fine — would satisfy it. `doctor` telling somebody to give a
    /// `kind` to a module that has one is how a reader learns to stop reading
    /// it.
    #[test]
    fn only_the_module_missing_a_kind_is_named() {
        let mut app = module("api", Some("apps/api/**"));
        app.kind = Some("app".to_owned());
        let config = config(Vec::new())
            .with_modules(vec![app, module("orders", Some("packages/orders/**"))]);

        let named: Vec<String> = examine(&config)
            .into_iter()
            .filter(|concern| concern.code == "module-wears-no-kind")
            .map(|concern| concern.message)
            .collect();

        assert_eq!(named.len(), 1, "{named:?}");
        assert!(named[0].contains("`orders`"), "{named:?}");
    }

    /// A module nothing references is a name somebody wrote down and a
    /// constraint nobody wrote.
    #[test]
    fn a_module_nothing_references_is_reported() {
        let config = config(Vec::new()).with_modules(vec![module("orders", Some("packages/**"))]);

        let named: Vec<String> = examine(&config)
            .into_iter()
            .filter(|concern| concern.code == "module-nobody-references")
            .map(|concern| concern.message)
            .collect();

        assert_eq!(named.len(), 1, "{named:?}");
        assert!(named[0].contains("`orders`"), "{named:?}");
    }

    /// And a module that holds a rule references itself.
    ///
    /// The half that makes the check usable: without it, every module in every
    /// config is reported, and a concern that fires on the correct state is one
    /// nobody can act on.
    #[test]
    fn a_module_that_holds_a_rule_is_not_reported_as_unreferenced() {
        let config = config(vec![in_module(
            rule("shape", &["packages/*"], structure(&["types"], &[])),
            "orders",
        )])
        .with_modules(vec![module("orders", Some("packages/**"))]);

        let codes = codes(&config);
        assert!(
            !codes.contains(&"module-nobody-references"),
            "the rule inside it is the reference: {codes:?}"
        );
    }

    /// A module that reaches nothing takes every rule inside it down with it,
    /// because each is narrowed to the intersection and the intersection of
    /// anything with nothing is nothing. One typo, nine silent rules, and a
    /// config that still looks right.
    #[test]
    fn a_module_scope_matching_no_directory_is_reported() {
        let config = config(vec![rule("shape", &["src/*"], structure(&["calcs"], &[]))])
            .with_modules(vec![module("domain", Some("packages/domian/**"))]);

        assert!(
            repository_codes(&[("src/a/b.ts", "export const a = 1;\n")], &config)
                .contains(&"module-scope-matches-nothing"),
        );
    }

    /// And one that reaches something is not reported, which is the half that
    /// makes the test above mean anything.
    #[test]
    fn a_module_that_reaches_a_directory_is_not_reported() {
        let config = config(vec![rule("shape", &["src/*"], structure(&["calcs"], &[]))])
            .with_modules(vec![module("domain", Some("src/**"))]);

        assert!(
            !repository_codes(&[("src/a/b.ts", "export const a = 1;\n")], &config)
                .contains(&"module-scope-matches-nothing"),
        );
    }

    /// A rule inside a module, pointing outside it, reaches nothing. This is
    /// the cost of narrowing over refusing, and this check is where it stops
    /// being silent.
    #[test]
    fn a_rule_pointing_outside_its_module_is_reported() {
        let inside = Scope::compile(["packages/domain/**"]).expect("valid");
        let mut stray = in_module(
            rule("stray", &["apps/*"], structure(&["calcs"], &[])),
            "domain",
        );
        stray.scope = stray.scope.within(&inside);

        let config =
            config(vec![stray]).with_modules(vec![module("domain", Some("packages/domain/**"))]);

        assert!(
            repository_codes(
                &[
                    ("apps/api/env.ts", "export const e = 1;\n"),
                    ("packages/domain/src/a.ts", "export const a = 1;\n"),
                ],
                &config,
            )
            .contains(&"rule-reaches-outside-its-module"),
        );
    }

    /// A rule whose scope is empty for its own reasons is *not* reported by
    /// this check: `scope-matches-nothing` already says so, and one mistake
    /// reported twice reads as two.
    #[test]
    fn a_rule_whose_own_scope_is_empty_is_not_blamed_on_its_module() {
        let inside = Scope::compile(["packages/domain/**"]).expect("valid");
        let mut stray = in_module(
            rule("stray", &["nowhere/*"], structure(&["calcs"], &[])),
            "domain",
        );
        stray.scope = stray.scope.within(&inside);

        let config =
            config(vec![stray]).with_modules(vec![module("domain", Some("packages/domain/**"))]);

        assert!(
            !repository_codes(
                &[("packages/domain/src/a.ts", "export const a = 1;\n")],
                &config,
            )
            .contains(&"rule-reaches-outside-its-module"),
        );
    }

    /// A scope naming a directory that is not there: usually a typo, sometimes
    /// a folder someone renamed and a rule nobody updated.
    #[test]
    fn a_scope_matching_no_directory_is_reported() {
        assert_eq!(
            repository_codes(
                &[("src/user/user.ts", "export class User {}")],
                &config(vec![rule(
                    "shape",
                    &["packages/*"],
                    structure(&["types"], &[])
                )]),
            ),
            ["scope-matches-nothing"]
        );
    }

    /// A scope that does match is not reported, even when nothing inside it
    /// interests the rule.
    #[test]
    fn a_scope_that_matches_is_not_reported() {
        let codes = repository_codes(
            &[("src/user/user.ts", "export class User {}")],
            &config(vec![rule("shape", &["src/*"], structure(&["types"], &[]))]),
        );

        assert!(!codes.contains(&"scope-matches-nothing"), "{codes:?}");
    }

    /// A regex that matches nothing in the rule's own scope: the rule loads,
    /// applies to a real directory, and still never looks at a file.
    #[test]
    fn a_pattern_matching_no_file_is_reported() {
        assert_eq!(
            repository_codes(
                // The pattern wants a lowercase `.ts`; this directory has
                // neither.
                &[("src/user/User.tsx", "export class User {}")],
                &config(vec![rule(
                    "name",
                    &["src/*"],
                    naming(None, KindFilter::Any)
                )]),
            ),
            ["pattern-matches-nothing"]
        );
    }

    /// And one that does match is quiet.
    #[test]
    fn a_pattern_that_matches_is_not_reported() {
        let codes = repository_codes(
            &[("src/user/thing.ts", "export const Thing = 1;")],
            &config(vec![rule(
                "name",
                &["src/*"],
                naming(None, KindFilter::Any),
            )]),
        );

        assert!(!codes.contains(&"pattern-matches-nothing"), "{codes:?}");
    }

    /// The same silent failure one level up. The mistake this catches is
    /// writing `dir_pattern` against the whole path when only the last segment
    /// is offered — and a rule whose directory pattern matches nothing applies
    /// to no file at all, which `CONFIG.md` calls the worst failure a linter
    /// has, because it is indistinguishable from a rule that passes.
    #[test]
    fn a_directory_pattern_matching_nothing_is_reported() {
        let codes = repository_codes(
            &[("src/Order/insert.ts", "export function OrderInsert() {}")],
            &config(vec![rule(
                "repo-name",
                &["src/*"],
                entity_naming(r"^src/(?<entity>[A-Za-z]+)$"),
            )]),
        );

        assert!(
            codes.contains(&"dir-pattern-matches-nothing"),
            "a pattern anchored against the path matches no directory name: {codes:?}"
        );
    }

    /// And the same rule written against the segment is quiet.
    #[test]
    fn a_directory_pattern_that_matches_is_not_reported() {
        let codes = repository_codes(
            &[("src/Order/insert.ts", "export function OrderInsert() {}")],
            &config(vec![rule(
                "repo-name",
                &["src/*"],
                entity_naming(r"^(?<entity>[A-Za-z]+)$"),
            )]),
        );

        assert!(!codes.contains(&"dir-pattern-matches-nothing"), "{codes:?}");
    }

    /// A `structure` rule's `filename_patterns` are regexes too, and one that
    /// matches nothing means the rule reports every file in the folder.
    #[test]
    fn a_structure_filename_pattern_matching_nothing_is_reported() {
        assert_eq!(
            repository_codes(
                &[("src/user/user.ts", "export class User {}")],
                &config(vec![rule(
                    "shape",
                    &["src/*"],
                    CompiledRuleKind::Structure {
                        allowed_subfolders: Some(Vec::new()),
                        warn_subfolders: Vec::new(),
                        recurse_into: Vec::new(),
                        subfolder_patterns: Vec::new(),
                        filename_patterns: vec![
                            Pattern::compile(r"^[a-z-]+\.use-case\.ts$").expect("valid"),
                        ],
                    },
                )]),
            ),
            ["pattern-matches-nothing"]
        );
    }

    /// A pattern is judged against the rule's own scope. One that matches a
    /// file somewhere else in the repository has still matched nothing here,
    /// and the rule still never looks at anything.
    #[test]
    fn a_pattern_is_judged_against_its_own_scope() {
        assert_eq!(
            repository_codes(
                &[
                    ("src/user/User.tsx", "export class User {}"),
                    // Matches the pattern, but lives outside `src/*`.
                    ("apps/web/thing.ts", "export const Thing = 1;"),
                ],
                &config(vec![rule(
                    "name",
                    &["src/*"],
                    naming(None, KindFilter::Any)
                )]),
            ),
            ["pattern-matches-nothing"]
        );
    }

    /// A file outside the rule's scope is not the rule's business, even when
    /// it would raise a concern if it were.
    #[test]
    fn a_file_outside_the_scope_is_not_examined() {
        let codes = repository_codes(
            &[
                ("src/user/thing.ts", "export const Thing = 1;"),
                ("apps/web/other.ts", "export default class {}"),
            ],
            &config(vec![rule(
                "name",
                &["src/*"],
                naming(None, KindFilter::Any),
            )]),
        );

        assert!(
            !codes.contains(&"only-a-default-export"),
            "`apps/web/other.ts` is outside `src/*`: {codes:?}"
        );
    }

    /// With nothing in scope there is nothing to conclude about a module
    /// nobody imports -- `pattern-matches-nothing` is the concern that
    /// applies, and saying both would send the user chasing two problems.
    #[test]
    fn an_empty_scope_concludes_nothing_about_the_module() {
        let codes = repository_codes(
            &[("src/api/route.get.ts", "export function GET() {}")],
            &config(vec![rule(
                "audit",
                &["src/*"],
                CompiledRuleKind::CallObligation {
                    file_pattern: Pattern::compile(r"^route\.post\.ts$").expect("valid"),
                    symbol: "Event.save".to_owned(),
                    imported_from: "@org/domain/event".to_owned(),
                    with_options: Vec::new(),
                },
            )]),
        );

        assert_eq!(codes, ["pattern-matches-nothing"]);
    }

    /// A `call-obligation` naming a module nothing in scope imports. One file
    /// missing the import is a finding `check` already reports; *no* file
    /// having it means the config's module name is probably wrong.
    #[test]
    fn a_module_nothing_imports_is_reported_as_a_likely_typo() {
        let obligation = |module: &str| {
            config(vec![rule(
                "audit",
                &["src/*"],
                CompiledRuleKind::CallObligation {
                    file_pattern: Pattern::compile(r"^route\.post\.ts$").expect("valid"),
                    symbol: "Event.save".to_owned(),
                    imported_from: module.to_owned(),
                    with_options: Vec::new(),
                },
            )])
        };
        let files = [(
            "src/api/route.post.ts",
            "import { Event } from '@org/domain/event';\nexport function POST() {}",
        )];

        assert_eq!(
            repository_codes(&files, &obligation("@org/domain/evnt")),
            ["symbol-never-imported"],
            "the misspelled module is the config's fault"
        );
        assert!(
            !repository_codes(&files, &obligation("@org/domain/event"))
                .contains(&"symbol-never-imported"),
            "the right module is not reported"
        );
    }

    /// One file having it is enough: then the missing calls are the code's
    /// problem, and `check` is the command that says so.
    #[test]
    fn one_file_importing_it_settles_the_question() {
        let codes = repository_codes(
            &[
                (
                    "src/api/route.post.ts",
                    "import { Event } from '@org/domain/event';\nexport function POST() {}",
                ),
                ("src/api/route.put.ts", "export function PUT() {}"),
            ],
            &config(vec![rule(
                "audit",
                &["src/*"],
                CompiledRuleKind::CallObligation {
                    file_pattern: Pattern::compile(r"^route\.(post|put)\.ts$").expect("valid"),
                    symbol: "Event.save".to_owned(),
                    imported_from: "@org/domain/event".to_owned(),
                    with_options: Vec::new(),
                },
            )]),
        );

        assert!(!codes.contains(&"symbol-never-imported"), "{codes:?}");
    }

    /// Issue #161. What #74 gave a module, one level over: a scoped decision
    /// whose paths are gone reaches nobody through `describe` while still
    /// reading in the config as though it governs something.
    #[test]
    fn a_decision_scoped_to_nowhere_is_reported() {
        let (guard, root) = tree_at(&[("src/user/thing.ts", "export class Thing {}")]);
        let mut moved = adr("ADR-014", DecisionStatus::Accepted);
        moved.scope = Some(Scope::compile(["packages/gone/**"]).expect("valid scope"));
        let mut present = adr("ADR-020", DecisionStatus::Accepted);
        present.scope = Some(Scope::compile(["src/**"]).expect("valid scope"));

        let config = config(vec![serving("shape", "ADR-014")]).with_decisions(vec![
            moved,
            present,
            adr("ADR-021", DecisionStatus::Accepted),
        ]);
        let tree = archwarden_engine::walk::walk(&root, &config).expect("walks");
        let concerns = examine_repository(&root, &config, &tree);
        drop(guard);

        let found: Vec<_> = concerns
            .iter()
            .filter(|c| c.code == "decision-scope-matches-nothing")
            .collect();

        assert_eq!(found.len(), 1, "{concerns:?}");
        assert!(found[0].message.contains("ADR-014"), "{found:?}");
        // A warning, not an error: the decision is still written down and
        // still true. What it lost is the way it arrives unprompted.
        assert_eq!(found[0].level, Level::Warning, "{found:?}");
    }

    /// Decision 9: a default export's name does not bind the importer, so a
    /// rule asking for a named export can never be satisfied by a file that
    /// has only one.
    #[test]
    fn a_file_with_only_a_default_export_is_reported() {
        let (guard, root) = tree_at(&[("src/user/thing.ts", "export default class {}")]);
        let config = config(vec![rule(
            "name",
            &["src/*"],
            naming(None, KindFilter::Any),
        )]);
        let tree = archwarden_engine::walk::walk(&root, &config).expect("walks");
        let concerns = examine_repository(&root, &config, &tree);
        drop(guard);

        let default_export: Vec<_> = concerns
            .iter()
            .filter(|c| c.code == "only-a-default-export")
            .collect();
        assert_eq!(default_export.len(), 1, "{concerns:?}");
        assert_eq!(
            default_export[0].path.as_ref().map(RepoRelPath::as_str),
            Some("src/user/thing.ts"),
            "the file is named, because that is what the user has to open"
        );
    }

    /// A file with a named export beside the default is fine: the rule has
    /// something to match.
    #[test]
    fn a_named_export_beside_a_default_is_fine() {
        let codes = repository_codes(
            &[(
                "src/user/thing.ts",
                "export const Thing = 1;\nexport default Thing;",
            )],
            &config(vec![rule(
                "name",
                &["src/*"],
                naming(None, KindFilter::Any),
            )]),
        );

        assert!(!codes.contains(&"only-a-default-export"), "{codes:?}");
    }

    /// An ignored file is not the rule's business, so it raises nothing.
    #[test]
    fn an_ignored_file_is_not_examined() {
        let config = CompiledConfig::new(
            vec![rule("name", &["src/*"], naming(None, KindFilter::Any))],
            PathSet::compile(["src/user/thing.ts".to_owned()]).expect("valid globs"),
            SkipDirs::default(),
            ContentHash::of(b"doctor"),
        );
        let (guard, root) = tree_at(&[("src/user/thing.ts", "export default class {}")]);
        let tree = archwarden_engine::walk::walk(&root, &config).expect("walks");
        let concerns = examine_repository(&root, &config, &tree);
        drop(guard);

        assert!(
            !concerns.iter().any(|c| c.code == "only-a-default-export"),
            "{concerns:?}"
        );
    }

    fn rendered(config: &CompiledConfig, format: crate::report::Format) -> String {
        let mut out = Vec::new();
        render(&examine(config), format, &mut out);
        String::from_utf8(out).expect("output is UTF-8")
    }

    /// Every concern carries both halves: what is wrong and what to do.
    #[test]
    fn the_text_output_gives_the_problem_and_the_fix() {
        let text = rendered(
            &with_ignore(
                vec![rule(
                    "legacy",
                    &["src/legacy/*"],
                    structure(&["types"], &[]),
                )],
                &["src/legacy/**"],
            ),
            crate::report::Format::Text,
        );

        assert!(text.contains("legacy [unreachable-scope]"), "{text}");
        assert!(text.contains("fix:"), "{text}");
        assert!(text.ends_with("1 concern\n"), "{text}");
    }

    /// A clean configuration says so rather than printing nothing.
    #[test]
    fn a_clean_configuration_says_so() {
        assert_eq!(
            rendered(&config(Vec::new()), crate::report::Format::Text),
            "No concerns.\n"
        );
    }

    /// The JSON is versioned, and the list is present even when empty.
    #[test]
    fn the_json_shape_is_versioned_and_always_carries_the_list() {
        let clean = rendered(&config(Vec::new()), crate::report::Format::Json);
        let parsed: serde_json::Value = serde_json::from_str(&clean).expect("valid JSON");
        assert_eq!(parsed["version"], 0);
        assert!(parsed["concerns"].as_array().is_some_and(Vec::is_empty));

        let json = rendered(
            &with_ignore(
                vec![rule(
                    "legacy",
                    &["src/legacy/*"],
                    structure(&["types"], &[]),
                )],
                &["src/legacy/**"],
            ),
            crate::report::Format::Json,
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["concerns"][0]["code"], "unreachable-scope");
        assert_eq!(parsed["concerns"][0]["rule_id"], "legacy");
        assert!(parsed["concerns"][0]["fix"].is_string());
    }

    /// A concern about the config as a whole has no rule id, and omits the
    /// field rather than sending null.
    #[test]
    fn a_config_wide_concern_omits_the_rule_id() {
        let json = rendered(
            &with_skip(vec![rule("b", &["src/**"], boundary())], SkipScope::Walk),
            crate::report::Format::Json,
        );

        assert!(!json.contains("\"rule_id\""), "{json}");
    }

    /// Plural grammar, because the count is the line a reader checks.
    #[test]
    fn the_count_is_pluralised() {
        let two = with_ignore(
            vec![
                rule("a", &["src/legacy/*"], structure(&["types"], &[])),
                rule("b", &["src/legacy/deep/*"], structure(&["types"], &[])),
            ],
            &["src/legacy/**"],
        );

        assert!(
            rendered(&two, crate::report::Format::Text).ends_with("2 concerns\n"),
            "{}",
            rendered(&two, crate::report::Format::Text)
        );
    }
}
