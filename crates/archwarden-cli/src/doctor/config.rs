//! Checks a config can fail on its own, with no repository.

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRule, CompiledRuleKind, SkipScope},
    facts::{ExportKind, ExportTags, KindFilter},
    ids::RuleId,
    level::Level,
    path::Language,
};
use archwarden_engine::walk::RepoTree;

use super::Concern;
use super::decisions::count;

pub(super) fn frozen_with_nothing_accepted(
    rule: &CompiledRule,
    engine: &dyn archwarden_core::traits::RuleEngine,
    tree: &RepoTree,
    baseline: Option<&archwarden_api::baseline::Baseline>,
    concerns: &mut Vec<Concern>,
) {
    if !matches!(rule.kind, CompiledRuleKind::Frozen) {
        return;
    }

    let reached = tree
        .files()
        .filter(|file| engine.applies_to(&file.path))
        .count();
    if reached == 0 {
        // A scope that reaches nothing is `scope_matches_nothing`'s sentence,
        // and saying it twice in two voices is worse than saying it once.
        return;
    }

    let accepted = baseline.map_or(0, |baseline| {
        baseline
            .entries()
            .filter(|entry| entry.rule == rule.id.as_str())
            .count()
    });
    if accepted > 0 {
        return;
    }

    concerns.push(Concern {
        code: "frozen-with-nothing-accepted",
        level: Level::Warning,
        rule_id: Some(rule.id.clone()),
        path: None,
        message: format!(
            "`{}` freezes {} that the baseline accepts none of, so every one of \
             them is reported as new",
            rule.id,
            count(reached, "file"),
        ),
        fix: "run `archwarden baseline` to accept what is there today -- a \
              freeze is that file plus this rule, and without it the rule \
              reports the past rather than the future"
            .to_owned(),
    });
}

/// A rule that has nothing to enforce, whatever its scope reaches.
///
/// `unreachable_scope` is about a rule that cannot see anything;
/// this is about one that sees plenty and asks nothing of it. Both are
/// invisible from the outside, which is the whole reason this command exists:
/// a rule enforcing nothing looks exactly like a repository that satisfies it.
///
/// Only `structure` today, because it is the only kind whose fields are all
/// optional — every other kind carries its constraint in a field that must be
/// present for the config to parse at all.
pub(super) fn constrains_nothing(rule: &CompiledRule, concerns: &mut Vec<Concern>) {
    let CompiledRuleKind::Structure {
        allowed_subfolders,
        warn_subfolders,
        subfolder_patterns,
        filename_patterns,
        ..
    } = &rule.kind
    else {
        return;
    };

    // An *empty* `allowed_subfolders` is a constraint -- "no subfolder may
    // exist here" -- so what matters is whether the field was named at all.
    // Issue #40.
    if allowed_subfolders.is_some()
        || !warn_subfolders.is_empty()
        || !subfolder_patterns.is_empty()
        || !filename_patterns.is_empty()
    {
        return;
    }

    concerns.push(Concern {
        code: "rule-constrains-nothing",
        level: Level::Warning,
        rule_id: Some(rule.id.clone()),
        path: None,
        message: "it names no allowed subfolder, no warned subfolder and no \
                  pattern for a folder or a filename, so there is nothing it \
                  can report"
            .to_owned(),
        fix: "give it something to enforce — `allowed_subfolders: []` forbids \
              every subfolder — or drop the rule"
            .to_owned(),
    });
}

/// Decision 5: a `skip_dirs` exemption that reaches the walk hides files from
/// *every* rule, and a boundary rule that cannot see a file cannot see the
/// import that lands on it.
pub(super) fn walk_scope_with_boundaries(config: &CompiledConfig, concerns: &mut Vec<Concern>) {
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
        level: Level::Warning,
        rule_id: None,
        path: None,
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
pub(super) fn unreachable_scope(
    config: &CompiledConfig,
    rule: &CompiledRule,
    concerns: &mut Vec<Concern>,
) {
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
        level: Level::Warning,
        rule_id: Some(rule.id.clone()),
        path: None,
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
pub(super) fn spec_subfolder_not_allowed(
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
        // A structure rule that names no list says nothing about folders. An
        // *empty* list is not that: it permits none of them, so a spec folder
        // is one it does not allow. Issue #40.
        if allowed_subfolders.is_none() && warn_subfolders.is_empty() {
            continue;
        }
        if allowed_subfolders
            .iter()
            .flatten()
            .chain(warn_subfolders)
            .any(|allowed| allowed == subfolder)
        {
            continue;
        }

        concerns.push(Concern {
            code: "spec-folder-not-allowed",
            level: Level::Warning,
            rule_id: Some(rule.id.clone()),
            path: None,
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
pub(super) fn governing_structure<'a>(
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
pub(super) fn hint_disagrees_with_kind(rule: &CompiledRule, concerns: &mut Vec<Concern>) {
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
        level: Level::Warning,
        rule_id: Some(rule.id.clone()),
        path: None,
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
pub(super) fn requires_only(kind: &KindFilter, wanted: ExportKind) -> bool {
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
pub(super) fn covers(ignore: &str, scope: &str) -> bool {
    let Some(prefix) = ignore.strip_suffix("/**") else {
        return ignore == scope;
    };
    if prefix.contains(['*', '?', '[', '{']) {
        return false;
    }

    scope == prefix || scope.starts_with(&format!("{prefix}/"))
}

pub(super) fn list(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| format!("`{item}`")).collect();
    match quoted.split_last() {
        None => "nothing".to_owned(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// A `must_export.kind` no language this config reads can declare.
///
/// Decision 31 split the vocabulary by language and promised the other half of
/// it here: *"not applicable is reported, never silent"*. Without this a rule
/// asking for a `struct` over a `.ts` file reports the file for exporting a
/// `const`, which reads like a naming mistake and is a configuration one --
/// and a rule asking only for forms nobody enabled reports every file it
/// reaches, forever.
///
/// Two shapes, and only the second is unambiguous enough to name here. A rule
/// whose kinds are *all* unproducible is a rule that can never pass; one whose
/// kinds are a mix is a rule spanning two trees on purpose, which is what a
/// Tauri repository writes deliberately.
pub(super) fn kind_no_enabled_language_can_declare(
    config: &CompiledConfig,
    rule: &CompiledRule,
    concerns: &mut Vec<Concern>,
) {
    let CompiledRuleKind::Naming { kind, .. } = &rule.kind else {
        return;
    };
    let KindFilter::OneOf(wanted) = kind else {
        return;
    };

    let enabled = enabled_languages(config);
    let unreachable: Vec<ExportKind> = wanted
        .iter()
        .filter(|kind| !enabled.iter().any(|language| kind.produced_by(*language)))
        .collect();

    // A mix is deliberate: one rule over both halves of a repository names the
    // form each half spells. Only a rule that can never be satisfied is a
    // mistake this can be sure of.
    if unreachable.len() != wanted.iter().count() {
        return;
    }

    let named = unreachable
        .iter()
        .map(|kind| format!("`{kind}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let languages = enabled
        .iter()
        .map(|language| format!("`{}`", language_name(*language)))
        .collect::<Vec<_>>()
        .join(", ");

    concerns.push(Concern {
        code: "kind-no-language-declares",
        level: Level::Error,
        rule_id: Some(rule.id.clone()),
        path: None,
        message: format!(
            "its `must_export.kind` asks only for {named}, and no language this \
             config reads ({languages}) can declare one -- so every file the \
             rule reaches is reported, and none of them can be fixed"
        ),
        fix: "name a form the language spells, or add the language to \
              `languages`. `function` is TypeScript and `fn` is Rust; `const`, \
              `type` and `enum` are both"
            .to_owned(),
    });
}

/// The languages this configuration reads.
///
/// TypeScript is always read and needs no flag, which is what
/// `compiled::Languages` says of itself.
fn enabled_languages(config: &CompiledConfig) -> Vec<Language> {
    let mut languages = vec![Language::Ts];
    if config.languages().astro {
        languages.push(Language::Astro);
    }
    if config.languages().rust {
        languages.push(Language::Rust);
    }
    languages
}

/// The spelling `languages` uses for one.
fn language_name(language: Language) -> &'static str {
    // `Language` is `#[non_exhaustive]`, so a fallback is required. Naming the
    // three rather than deriving one keeps the spelling here the same as the
    // one `languages` accepts; a language added upstream reads as its debug
    // form until somebody names it, which is visible rather than wrong.
    match language {
        Language::Astro => "astro",
        Language::Rust => "rust",
        _ => "ts",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every language prints the word `languages` accepts.
    ///
    /// Tested directly because the concern that renders it cannot reach all
    /// three: it fires only when no enabled language declares the form, and
    /// TypeScript is always enabled -- so a message is only ever built while
    /// Rust is *off*. Mutation testing found the arm that way, and deleting it
    /// broke nothing.
    ///
    /// The words matter because the fix tells somebody to add one to
    /// `languages`, and a message naming `Rust` where the config wants `rust`
    /// sends them to a refusal.
    #[test]
    fn every_language_prints_the_word_a_config_writes() {
        assert_eq!(language_name(Language::Ts), "ts");
        assert_eq!(language_name(Language::Astro), "astro");
        assert_eq!(language_name(Language::Rust), "rust");
    }

    /// The set follows the config, and TypeScript is always in it.
    ///
    /// `compiled::Languages` says so of itself: a configuration that asked for
    /// nothing still means TypeScript.
    #[test]
    fn the_enabled_set_is_typescript_plus_whatever_was_asked_for() {
        use archwarden_core::{compiled::Languages, glob::PathSet, hash::ContentHash};

        let bare = CompiledConfig::new(
            Vec::new(),
            PathSet::default(),
            archwarden_core::compiled::SkipDirs::default(),
            ContentHash::of(b""),
        );
        assert_eq!(enabled_languages(&bare), vec![Language::Ts]);

        let both = bare.with_languages(Languages {
            astro: true,
            rust: true,
        });
        assert_eq!(
            enabled_languages(&both),
            vec![Language::Ts, Language::Astro, Language::Rust]
        );
    }
}
