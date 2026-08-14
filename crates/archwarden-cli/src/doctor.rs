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
    facts::{ExportKind, ExportTags, FileFacts, KindFilter},
    ids::RuleId,
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

    for rule in config.rules() {
        unreachable_scope(config, rule, &mut concerns);
        constrains_nothing(rule, &mut concerns);
        spec_subfolder_not_allowed(config, rule, &mut concerns);
        hint_disagrees_with_kind(rule, &mut concerns);
    }

    concerns
}

/// Rules that do not say why they exist, counted rather than listed.
///
/// One line, not one per rule: a config with forty rules and no `why` anywhere
/// would otherwise bury every other concern this command has, and burying them
/// is the same as not reporting them.
///
/// And only once at least one rule *does* say why. A project that has never
/// used the field has not adopted the practice, and nagging it about a
/// convention it never chose is how a command that gives advice becomes one
/// people stop running. Once one rule carries a reason, the ones that do not
/// are an inconsistency worth naming. Issue #46.
fn reasons_left_unsaid(config: &CompiledConfig, concerns: &mut Vec<Concern>) {
    let (with, without): (Vec<_>, Vec<_>) = config.rules().partition(|rule| rule.why.is_some());

    if with.is_empty() || without.is_empty() {
        return;
    }

    concerns.push(Concern {
        code: "rules-without-a-reason",
        rule_id: None,
        path: None,
        message: format!(
            "{} of {} rules say why they exist; {} {} not",
            with.len(),
            with.len() + without.len(),
            without.len(),
            if without.len() == 1 { "does" } else { "do" },
        ),
        fix: "add `why` to them, or accept the gap -- a rule whose reason is \
              nowhere is one a reader can only obey"
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
fn constrains_nothing(rule: &CompiledRule, concerns: &mut Vec<Concern>) {
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

    for (rule, engine) in config.rules().zip(archwarden_rules::engines_for(config)) {
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

/// A rule whose scope matches directories and whose engine sees no file in any
/// of them.
///
/// The gap `scope_matches_nothing` leaves, and the one that costs most,
/// because the config looks right. `roots: "packages/domain/src/*"` selects
/// the 49 entity directories exactly as documented — and if every entity keeps
/// its code in `calcs/`, `types/`, `actions/` with nothing loose at the top,
/// a rule about files evaluates none of them and reports silence.
///
/// Silence is indistinguishable from a clean repository, which is the failure
/// `CONFIG.md` calls the worst a linter has. `doctor` promises to answer "does
/// this config mean what you think?", and a rule reaching zero files is
/// exactly a rule that does not.
///
/// A rule whose findings are about directories is exempt: having no files to
/// inspect is its ordinary state rather than a symptom. That used to be a match
/// on `structure` by name, and `presence` arrived answering the same way and
/// was not on the list — so `doctor` called every `presence` rule idle while
/// `check` was firing it on the same repository, and the fix it suggested
/// (widen the scope) would have turned a working rule into a wall of false
/// errors. The engine is asked now, via `answers_for_directories`.
///
/// So is any rule another concern has already explained — a `file_pattern`
/// matching nothing reaches no file *because* of that, and reporting both would
/// make one mistake look like two.
fn rule_evaluates_nothing(
    rule: &CompiledRule,
    engine: &dyn archwarden_core::traits::RuleEngine,
    tree: &RepoTree,
    concerns: &mut Vec<Concern>,
) {
    if engine.answers_for_directories() {
        return;
    }
    // Only when the scope did match something: a scope matching no directory
    // at all is already reported, and two concerns about one typo is noise.
    if !tree
        .directories()
        .any(|(path, _)| rule.scope.matches_dir(path.as_path()))
    {
        return;
    }
    if tree.files().any(|file| engine.applies_to(&file.path)) {
        return;
    }
    // A more specific concern about this rule has already said why. This one
    // is the catch-all for a silence nothing else explained.
    if concerns
        .iter()
        .any(|concern| concern.rule_id.as_ref() == Some(&rule.id))
    {
        return;
    }

    concerns.push(Concern {
        code: "rule-evaluates-nothing",
        rule_id: Some(rule.id.clone()),
        path: None,
        message: format!(
            "{} matches directories, but no file inside them is subject to this rule",
            list(rule.scope.patterns())
        ),
        fix: concat!(
            "a scope selects directories and the rule then inspects the files directly ",
            "inside them -- `src/*` reaches files one level down, not two. If the files ",
            "live in subfolders, the scope wants `src/**`"
        )
        .to_owned(),
    });
}

/// A scope naming a directory that is not there. Usually a typo, occasionally
/// a folder someone renamed and a rule nobody updated.
/// A module every rule declares and none of them mentions.
///
/// Free once a module has paths of its own (issue #74), and worth having: a
/// module nothing references is a name somebody wrote down and a constraint
/// nobody wrote. It is the shape of `governance: closed` (#60) asked about the
/// config rather than about the tree.
///
/// A module that holds rules references itself, so only the empty declarations
/// and the ones no boundary names are reported.
fn module_nobody_references(config: &CompiledConfig, concerns: &mut Vec<Concern>) {
    for module in config.modules() {
        if config
            .rules()
            .any(|rule| rule.module.as_ref() == Some(&module.id))
        {
            continue;
        }

        concerns.push(Concern {
            code: "module-nobody-references",
            rule_id: None,
            path: None,
            message: format!("module `{}` holds no rules and none names it", module.id),
            fix: "give it a rule, name it from an `import-boundary` with \
                  `from_module` or `forbid_module`, or delete the declaration"
                .to_owned(),
        });
    }
}

/// A module with no `kind`, in a config where rules quantify over kinds.
///
/// The omission problem one level up, and the reason it is worth a check: a
/// rule saying "an assembly may not import another assembly" governs every
/// module wearing `app`, and a module wearing nothing is outside it. Silently.
/// The seventh assembly declared without a `kind` is exactly the case the
/// quantifier was written to stop, arriving through the config instead of
/// through the rule.
///
/// Only reported when something quantifies: a config that never uses kinds is
/// not missing them.
fn module_wearing_no_kind(config: &CompiledConfig, concerns: &mut Vec<Concern>) {
    let quantifies = config.modules().any(|module| module.kind.is_some());
    if !quantifies {
        return;
    }

    for module in config.modules() {
        if module.kind.is_some() || module.scope.is_none() {
            continue;
        }

        concerns.push(Concern {
            code: "module-wears-no-kind",
            rule_id: None,
            path: None,
            message: format!(
                "module `{}` declares no `kind`, and this config has rules about kinds",
                module.id
            ),
            fix: "give it one, so a rule quantifying over kinds covers it. A \
                  module outside every such rule is governed by nothing they say"
                .to_owned(),
        });
    }
}

/// A module whose `scope` selects no directory in the repository.
///
/// The module-level twin of `scope-matches-nothing`, and it costs more,
/// because a module that reaches nothing takes every rule inside it down with
/// it: they are narrowed to the intersection, and the intersection of anything
/// with nothing is nothing. One typo in one field, and nine rules go quiet
/// with the config still looking right.
fn module_scope_matches_nothing(
    module: &archwarden_core::compiled::CompiledModule,
    tree: &RepoTree,
    concerns: &mut Vec<Concern>,
) {
    let Some(scope) = &module.scope else {
        return;
    };
    if tree
        .directories()
        .any(|(path, _)| scope.matches_dir(path.as_path()))
    {
        return;
    }

    concerns.push(Concern {
        code: "module-scope-matches-nothing",
        rule_id: None,
        path: None,
        message: format!(
            "module `{}` selects no directory in the repository: {}",
            module.id,
            list(scope.patterns())
        ),
        fix: "every rule inside a module is narrowed to it, so a module that \
              reaches nothing silences all of them. Check the glob against the \
              tree"
            .to_owned(),
    });
}

/// A rule whose own scope points outside the module it lives in.
///
/// The cost of narrowing rather than refusing (`Scope::within`). A rule that
/// keeps `roots: "apps/**"` inside a module scoped to `packages/domain/**`
/// reaches nothing, and nothing about the config looks wrong. Refusing when it
/// compiles is not available — whether one glob contains another is not a
/// question `globset` answers — so it is asked here, against a tree, where it
/// is a fact rather than a guess.
fn rule_reaches_outside_its_module(
    config: &CompiledConfig,
    rule: &CompiledRule,
    tree: &RepoTree,
    concerns: &mut Vec<Concern>,
) {
    let Some(module_id) = &rule.module else {
        return;
    };
    let Some(module) = config.modules().find(|m| &m.id == module_id) else {
        return;
    };
    let Some(scope) = &module.scope else {
        return;
    };
    // Its own patterns reach somewhere, and the narrowed scope reaches
    // nowhere: the module is what removed it.
    let own = archwarden_core::scope::Scope::compile(rule.scope.patterns());
    let Ok(own) = own else { return };

    let alone = tree
        .directories()
        .any(|(path, _)| own.matches_dir(path.as_path()));
    let narrowed = tree
        .directories()
        .any(|(path, _)| rule.scope.matches_dir(path.as_path()));

    if !alone || narrowed {
        return;
    }

    concerns.push(Concern {
        code: "rule-reaches-outside-its-module",
        rule_id: Some(rule.id.clone()),
        path: None,
        message: format!(
            "{} matches directories the repository has, and none of them is inside \
             module `{module_id}` ({})",
            list(rule.scope.patterns()),
            list(scope.patterns())
        ),
        fix: "a rule inside a module reaches where both reach, so this one \
              reaches nothing. Widen the module's `scope`, narrow the rule's \
              `roots`, or move the rule out of the module"
            .to_owned(),
    });
}

fn scope_matches_nothing(rule: &CompiledRule, tree: &RepoTree, concerns: &mut Vec<Concern>) {
    if tree
        .directories()
        .any(|(path, _)| rule.scope.matches_dir(path.as_path()))
    {
        return;
    }

    concerns.push(Concern {
        code: "scope-matches-nothing",
        rule_id: Some(rule.id.clone()),
        path: None,
        message: format!(
            "no directory in the repository matches {}",
            list(rule.scope.patterns())
        ),
        fix: "check the glob against the tree -- a scope selects directories, \
              so `src/*` means the folders inside `src`, not the files"
            .to_owned(),
    });
}

/// A filename regex that matches nothing in the rule's own scope. The rule
/// loads, applies to a real directory, and still never looks at a file.
fn pattern_matches_nothing(
    config: &CompiledConfig,
    rule: &CompiledRule,
    tree: &RepoTree,
    concerns: &mut Vec<Concern>,
) {
    let patterns: Vec<&archwarden_core::pattern::Pattern> = match &rule.kind {
        CompiledRuleKind::Naming { file_pattern, .. }
        | CompiledRuleKind::CallObligation { file_pattern, .. } => vec![file_pattern],
        CompiledRuleKind::Structure {
            filename_patterns, ..
        } => filename_patterns.iter().collect(),
        _ => Vec::new(),
    };

    for pattern in patterns {
        if in_scope(config, rule, tree).any(|file| pattern.is_match(&file.name)) {
            continue;
        }

        concerns.push(Concern {
            code: "pattern-matches-nothing",
            rule_id: Some(rule.id.clone()),
            path: None,
            message: format!(
                "`{}` matches no file in {}",
                pattern.as_str(),
                list(rule.scope.patterns())
            ),
            fix: "check the regex against a real filename -- it is matched \
                  against the name alone, not the path"
                .to_owned(),
        });
    }

    dir_pattern_matches_nothing(config, rule, tree, concerns);
}

/// The same silent failure, one level up: a `dir_pattern` that matches no
/// directory in scope stops the rule applying to anything at all.
///
/// Worth its own check rather than folding into the loop above, because the
/// text a reader needs is different. A `file_pattern` that matches nothing is
/// usually a regex written against a path; a `dir_pattern` that matches nothing
/// is usually one written against the whole path when only the last segment is
/// offered, and saying "filename" to someone debugging a directory regex sends
/// them the wrong way.
fn dir_pattern_matches_nothing(
    config: &CompiledConfig,
    rule: &CompiledRule,
    tree: &RepoTree,
    concerns: &mut Vec<Concern>,
) {
    let CompiledRuleKind::Naming {
        dir_pattern: Some(pattern),
        ..
    } = &rule.kind
    else {
        return;
    };

    let matches = in_scope(config, rule, tree).any(|file| {
        file.path
            .parent()
            .and_then(|parent| parent.file_name().map(ToOwned::to_owned))
            .is_some_and(|directory| pattern.is_match(&directory))
    });
    if matches {
        return;
    }

    concerns.push(Concern {
        code: "dir-pattern-matches-nothing",
        rule_id: Some(rule.id.clone()),
        path: None,
        message: format!(
            "`{}` matches no directory in {}, so the rule applies to no file",
            pattern.as_str(),
            list(rule.scope.patterns())
        ),
        fix: "`dir_pattern` is matched against the name of the directory the \
              file sits in -- `Order`, not `src/entities/Order`"
            .to_owned(),
    });
}

/// A `call-obligation` naming a module nothing in scope imports.
///
/// One file missing the import is a finding `check` already reports. *No* file
/// having it is a different claim: the module name in the config is probably
/// wrong, and every file in scope is about to be reported for a typo.
fn symbol_never_imported(
    root: &Utf8Path,
    config: &CompiledConfig,
    rule: &CompiledRule,
    engine: &dyn archwarden_core::traits::RuleEngine,
    tree: &RepoTree,
    concerns: &mut Vec<Concern>,
) {
    let CompiledRuleKind::CallObligation {
        symbol,
        imported_from,
        ..
    } = &rule.kind
    else {
        return;
    };

    // A flag rather than a count: the question is "did this rule cover
    // anything?", and a counter invites arithmetic nobody needs.
    let mut covered_something = false;
    for facts in facts_covered(root, config, engine, tree) {
        covered_something = true;
        if facts
            .imports
            .iter()
            .any(|import| &import.specifier == imported_from)
        {
            return;
        }
    }

    // Nothing to conclude when the rule covers nothing; `scope-matches-nothing`
    // and `pattern-matches-nothing` are the concerns that apply, and saying
    // all three would send the user chasing three problems that are one.
    if !covered_something {
        return;
    }

    concerns.push(Concern {
        code: "symbol-never-imported",
        rule_id: Some(rule.id.clone()),
        path: None,
        message: format!(
            "no file this rule covers imports from `{imported_from}`, so every \
             one of them will be reported for missing `{symbol}`"
        ),
        fix: format!("check `{imported_from}` against how the code spells it"),
    });
}

/// Decision 9: a `naming` rule asks for a named export, and a file with only a
/// default export can never satisfy one -- a default's local name does not
/// bind the importer.
fn only_a_default_export(
    root: &Utf8Path,
    config: &CompiledConfig,
    rule: &CompiledRule,
    engine: &dyn archwarden_core::traits::RuleEngine,
    tree: &RepoTree,
    concerns: &mut Vec<Concern>,
) {
    if !matches!(rule.kind, CompiledRuleKind::Naming { .. }) {
        return;
    }

    for facts in facts_covered(root, config, engine, tree) {
        if facts.exports.is_empty() || facts.exports.iter().any(|export| !export.is_default) {
            continue;
        }

        concerns.push(Concern {
            code: "only-a-default-export",
            rule_id: Some(rule.id.clone()),
            path: Some(facts.path.clone()),
            message: "it exports only a default, whose name does not bind the \
                      importer, so a rule asking for a named export can never \
                      be satisfied here"
                .to_owned(),
            fix: "export the symbol by name, or take this file out of the \
                  rule's scope"
                .to_owned(),
        });
    }
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
        let subject = match (&concern.rule_id, &concern.path) {
            (Some(rule), Some(path)) => format!("{rule} · {path}"),
            (Some(rule), None) => rule.to_string(),
            (None, _) => "config".to_owned(),
        };
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
            why: None,
            module_why: None,
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
                },
            )]),
        );

        assert!(!codes.contains(&"symbol-never-imported"), "{codes:?}");
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
