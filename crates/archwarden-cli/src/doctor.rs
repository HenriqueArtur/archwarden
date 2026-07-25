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
    compiled::{CompiledConfig, CompiledRule, CompiledRuleKind, SkipScope},
    facts::{ExportKind, ExportTags, KindFilter},
    ids::RuleId,
};
use serde::Serialize;

/// The version of the `doctor` JSON shape.
pub const DOCTOR_VERSION: u32 = 0;

/// Something worth telling the user about their configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Concern {
    /// A stable slug, so a user can grep for one and a tool can branch on it.
    pub code: &'static str,
    /// The rule it is about, when it is about one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<RuleId>,
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

    for rule in config.rules() {
        unreachable_scope(config, rule, &mut concerns);
        spec_subfolder_not_allowed(config, rule, &mut concerns);
        hint_disagrees_with_kind(rule, &mut concerns);
    }

    concerns
}

/// Decision 5: a `skip_dirs` exemption that reaches the walk hides files from
/// *every* rule, and a boundary rule that cannot see a file cannot see the
/// import that lands on it.
fn walk_scope_with_boundaries(config: &CompiledConfig, concerns: &mut Vec<Concern>) {
    if config.skip_dirs().scope != SkipScope::Walk {
        return;
    }

    let boundaries: Vec<&RuleId> = config
        .rules()
        .filter(|rule| matches!(rule.kind, CompiledRuleKind::ImportBoundary { .. }))
        .map(|rule| &rule.id)
        .collect();

    if boundaries.is_empty() {
        return;
    }

    concerns.push(Concern {
        code: "walk-skip-hides-imports",
        rule_id: None,
        message: format!(
            "`skip_dirs.scope` is `walk`, so skipped folders are invisible to \
             the whole run -- including to {}, which {} about imports that \
             land in them",
            list(
                &boundaries
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            ),
            if boundaries.len() == 1 { "asks" } else { "ask" },
        ),
        fix: "use `skip_dirs.scope: \"structure\"` so the folders are exempt \
              from the structure rule but still walked, or narrow \
              `skip_dirs.prefixes`"
            .to_owned(),
    });
}

/// Decision 6: a scope inside an `ignore` entry can never fire, and a rule
/// that never fires reads exactly like a rule that passes.
fn unreachable_scope(config: &CompiledConfig, rule: &CompiledRule, concerns: &mut Vec<Concern>) {
    let patterns = rule.scope.patterns();
    if patterns.is_empty() {
        return;
    }

    let covering: Vec<String> = config
        .ignore_globs()
        .patterns()
        .iter()
        .filter(|ignore| patterns.iter().all(|scope| covers(ignore, scope)))
        .cloned()
        .collect();

    if covering.is_empty() {
        return;
    }

    concerns.push(Concern {
        code: "unreachable-scope",
        rule_id: Some(rule.id.clone()),
        message: format!(
            "every path this rule covers is excluded by {}, so it can never \
             report anything",
            list(&covering)
        ),
        fix: "narrow the `ignore` entry, or drop the rule".to_owned(),
    });
}

/// A `spec-pair` looking in a folder the structure rule for the same scope
/// does not allow: the folder cannot exist, so the rule watches nothing.
fn spec_subfolder_not_allowed(
    config: &CompiledConfig,
    rule: &CompiledRule,
    concerns: &mut Vec<Concern>,
) {
    let CompiledRuleKind::SpecPair { subfolders, .. } = &rule.kind else {
        return;
    };

    for subfolder in subfolders {
        // `.` is the scope directory itself, which no structure rule governs
        // the existence of.
        if subfolder == "." || subfolder.is_empty() {
            continue;
        }

        let Some(structure) = governing_structure(config, rule) else {
            continue;
        };
        let CompiledRuleKind::Structure {
            allowed_subfolders,
            warn_subfolders,
            ..
        } = &structure.kind
        else {
            continue;
        };
        // A structure rule that names no folders constrains none of them.
        if allowed_subfolders.is_empty() && warn_subfolders.is_empty() {
            continue;
        }
        if allowed_subfolders
            .iter()
            .chain(warn_subfolders)
            .any(|allowed| allowed == subfolder)
        {
            continue;
        }

        concerns.push(Concern {
            code: "spec-folder-not-allowed",
            rule_id: Some(rule.id.clone()),
            message: format!(
                "it looks for specs in `{subfolder}`, which `{}` does not \
                 allow as a subfolder, so that folder cannot exist",
                structure.id
            ),
            fix: format!(
                "add `{subfolder}` to `{}`'s `allowed_subfolders`, or point \
                 this rule at a folder that is allowed",
                structure.id
            ),
        });
    }
}

/// A structure rule governing the same scope as `rule`.
fn governing_structure<'a>(
    config: &'a CompiledConfig,
    rule: &CompiledRule,
) -> Option<&'a CompiledRule> {
    config.rules().find(|candidate| {
        matches!(candidate.kind, CompiledRuleKind::Structure { .. })
            && candidate.scope.patterns() == rule.scope.patterns()
    })
}

/// The M7b finding: `scaffold` writes the hint straight after the declaration
/// keyword, so a hint in one style under a rule demanding another emits a line
/// that does not compile.
fn hint_disagrees_with_kind(rule: &CompiledRule, concerns: &mut Vec<Concern>) {
    let CompiledRuleKind::Naming {
        kind,
        signature_hint: Some(hint),
        ..
    } = &rule.kind
    else {
        return;
    };

    // Only the unambiguous case. A hint is free text archwarden never
    // verifies, and a doctor that guessed at prose would cry wolf.
    if !requires_only(kind, ExportKind::Function) || !hint.contains("=>") {
        return;
    }

    concerns.push(Concern {
        code: "hint-disagrees-with-kind",
        rule_id: Some(rule.id.clone()),
        message: format!(
            "its `signature_hint` is written as an arrow (`{hint}`) but the \
             rule requires a `function`, so `archwarden scaffold` emits a \
             declaration that does not compile"
        ),
        fix: "write the hint as a call signature -- `(deps: Deps): Result` \
              rather than `(deps: Deps) => Result` -- or allow `arrow` in \
              `must_export.kind`"
            .to_owned(),
    });
}

/// Whether a filter accepts exactly one declaration form, and it is `wanted`.
fn requires_only(kind: &KindFilter, wanted: ExportKind) -> bool {
    match kind {
        KindFilter::OneOf(tags) => *tags == ExportTags::only(wanted),
        _ => false,
    }
}

/// Whether an `ignore` glob swallows a scope glob whole.
///
/// Deliberately conservative: it answers yes only when the ignore pattern is a
/// `**` under a literal prefix that also prefixes the scope. Glob containment
/// is undecidable in general, and a doctor that guessed would report rules
/// that work.
fn covers(ignore: &str, scope: &str) -> bool {
    let Some(prefix) = ignore.strip_suffix("/**") else {
        return ignore == scope;
    };
    if prefix.contains(['*', '?', '[', '{']) {
        return false;
    }

    scope == prefix || scope.starts_with(&format!("{prefix}/"))
}

fn list(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| format!("`{item}`")).collect();
    match quoted.split_last() {
        None => "nothing".to_owned(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// The JSON envelope.
#[derive(Debug, Serialize)]
struct JsonDoctor<'a> {
    version: u32,
    /// Always present, even when empty: a caller needs to see that the list is
    /// empty rather than infer it from absence.
    concerns: &'a [Concern],
}

/// Writes the diagnosis.
pub fn render(concerns: &[Concern], format: crate::report::Format, out: &mut dyn std::io::Write) {
    match format {
        crate::report::Format::Text => render_text(concerns, out),
        crate::report::Format::Json => render_json(concerns, out),
    }
}

fn render_json(concerns: &[Concern], out: &mut dyn std::io::Write) {
    match serde_json::to_string_pretty(&JsonDoctor {
        version: DOCTOR_VERSION,
        concerns,
    }) {
        Ok(json) => {
            let _ = writeln!(out, "{json}");
        }
        Err(error) => {
            let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
        }
    }
}

fn render_text(concerns: &[Concern], out: &mut dyn std::io::Write) {
    if concerns.is_empty() {
        let _ = writeln!(out, "No concerns.");
        return;
    }

    for concern in concerns {
        let subject = concern
            .rule_id
            .as_ref()
            .map_or_else(|| "config".to_owned(), ToString::to_string);
        let _ = writeln!(out, "{} [{}]\n  {}", subject, concern.code, concern.message);
        let _ = writeln!(out, "  fix: {}\n", concern.fix);
    }

    let _ = writeln!(
        out,
        "{} {}",
        concerns.len(),
        if concerns.len() == 1 {
            "concern"
        } else {
            "concerns"
        }
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::SkipDirs, glob::PathSet, hash::ContentHash, level::Level, pattern::Pattern,
        scope::Scope,
    };

    fn rule(id: &str, scope: &[&str], kind: CompiledRuleKind) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: None,
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
            require: PathSet::default(),
            except: PathSet::default(),
            include_type_only: true,
        }
    }

    fn naming(hint: Option<&str>, kind: KindFilter) -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile("^(?<name>[a-z]+)\\.ts$").expect("valid"),
            name_template: "{{pascal(name)}}".to_owned(),
            kind,
            signature_hint: hint.map(str::to_owned),
        }
    }

    fn structure(allowed: &[&str], warn: &[&str]) -> CompiledRuleKind {
        CompiledRuleKind::Structure {
            allowed_subfolders: allowed.iter().map(|s| (*s).to_owned()).collect(),
            warn_subfolders: warn.iter().map(|s| (*s).to_owned()).collect(),
            recurse_into: Vec::new(),
            filename_patterns: Vec::new(),
        }
    }

    fn spec_pair(subfolders: &[&str]) -> CompiledRuleKind {
        CompiledRuleKind::SpecPair {
            subfolders: subfolders.iter().map(|s| (*s).to_owned()).collect(),
            spec_markers: vec!["spec".to_owned()],
            ignore_files: PathSet::default(),
            require_non_empty_spec: false,
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

    /// A structure rule that names no folders constrains none of them.
    #[test]
    fn a_structure_rule_that_allows_nothing_constrains_nothing() {
        let open = config(vec![
            rule("shape", &["src/*"], structure(&[], &[])),
            rule("spec", &["src/*"], spec_pair(&["usecases"])),
        ]);

        assert!(examine(&open).is_empty());
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
