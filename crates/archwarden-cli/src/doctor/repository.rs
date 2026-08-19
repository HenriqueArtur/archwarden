//! Checks that need the repository the config is pointed at.

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRule, CompiledRuleKind},
    level::Level,
};
use archwarden_engine::walk::RepoTree;
use camino::Utf8Path;

use super::Concern;
use super::{config::list, facts_covered, in_scope};

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
pub(super) fn rule_evaluates_nothing(
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
        level: Level::Warning,
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
pub(super) fn module_nobody_references(config: &CompiledConfig, concerns: &mut Vec<Concern>) {
    for module in config.modules() {
        if config
            .rules()
            .any(|rule| rule.module.as_ref() == Some(&module.id))
        {
            continue;
        }

        concerns.push(Concern {
            code: "module-nobody-references",
            level: Level::Warning,
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
pub(super) fn module_wearing_no_kind(config: &CompiledConfig, concerns: &mut Vec<Concern>) {
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
            level: Level::Warning,
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
pub(super) fn module_scope_matches_nothing(
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
        level: Level::Warning,
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
pub(super) fn rule_reaches_outside_its_module(
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
        level: Level::Warning,
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

pub(super) fn scope_matches_nothing(
    rule: &CompiledRule,
    tree: &RepoTree,
    concerns: &mut Vec<Concern>,
) {
    if tree
        .directories()
        .any(|(path, _)| rule.scope.matches_dir(path.as_path()))
    {
        return;
    }

    concerns.push(Concern {
        code: "scope-matches-nothing",
        level: Level::Warning,
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
pub(super) fn pattern_matches_nothing(
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
            level: Level::Warning,
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
pub(super) fn dir_pattern_matches_nothing(
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
        level: Level::Warning,
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
pub(super) fn symbol_never_imported(
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
        level: Level::Warning,
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
pub(super) fn only_a_default_export(
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
            level: Level::Warning,
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
