//! The path sets an `import-boundary` rule is built from.

use archwarden_core::{glob::PathSet, ids::RuleId};

use super::fields::globs;

use super::{error::CompileError, scope::Modules};

/// The paths a boundary forbids, from whichever field it used.
///
/// `forbid_module` and `forbid_import_from` are refused together, on the same
/// argument as `from` and `from_module`: one rule, one way of saying what it
/// is about. Neither is fine — a boundary may forbid nothing and require
/// something instead, which `must_import_from` is for.
pub(super) fn forbidden_paths(
    id: &RuleId,
    rule: &crate::rule::ImportBoundaryRule,
    modules: &Modules,
) -> Result<PathSet, CompileError> {
    if !rule.forbid_module.is_empty() && !rule.forbid_import_from.is_empty() {
        return Err(CompileError::ScopeSaidTwice {
            rule: id.clone(),
            one: "forbid_import_from",
            other: "forbid_module",
        });
    }

    if rule.forbid_module.is_empty() {
        return globs(id, "forbid_import_from", &rule.forbid_import_from);
    }

    let mut patterns: Vec<String> = Vec::new();
    for named in &rule.forbid_module {
        patterns.extend(modules.paths_of(id, named)?.iter().cloned());
    }
    PathSet::compile(&patterns).map_err(|source| CompileError::Glob {
        rule: id.clone(),
        field: "forbid_module",
        source,
    })
}

/// The paths a boundary rule refuses to let its files *end up* depending on.
///
/// The same shape as [`forbidden_paths`], and the same refusal when both the
/// globs and the modules are given: two ways to fill one set means an author
/// has to be told which one won, and a rule nobody can predict is worse than a
/// rule that will not compile.
pub(super) fn reaching_paths(
    id: &RuleId,
    rule: &crate::rule::ImportBoundaryRule,
    modules: &Modules,
) -> Result<PathSet, CompileError> {
    if !rule.forbid_reaching_modules.is_empty() && !rule.forbid_reaching.is_empty() {
        return Err(CompileError::ScopeSaidTwice {
            rule: id.clone(),
            one: "forbid_reaching",
            other: "forbid_reaching_modules",
        });
    }

    if rule.forbid_reaching_modules.is_empty() {
        return globs(id, "forbid_reaching", &rule.forbid_reaching);
    }

    let mut patterns: Vec<String> = Vec::new();
    for named in &rule.forbid_reaching_modules {
        patterns.extend(modules.paths_of(id, named)?.iter().cloned());
    }
    PathSet::compile(&patterns).map_err(|source| CompileError::Glob {
        rule: id.clone(),
        field: "forbid_reaching_modules",
        source,
    })
}

/// The groups a boundary's importers fall into, one per module it covers.
///
/// One group for a rule about one module or one set of globs, and one *per
/// module* for a rule about a kind. That distinction is the whole of the
/// self-import question: an assembly may import its own files and not its
/// siblings', and only per-module groups can tell those apart.
pub(super) fn importer_groups(
    id: &RuleId,
    rule: &crate::rule::ImportBoundaryRule,
    modules: &Modules,
) -> Result<Vec<PathSet>, CompileError> {
    let Some(kind) = &rule.from_kind else {
        return Ok(Vec::new());
    };

    let mut groups = Vec::new();
    for (module, worn) in &modules.kinds {
        if worn != kind {
            continue;
        }
        let paths = modules.paths_of(id, module)?;
        groups.push(
            PathSet::compile(paths).map_err(|source| CompileError::Glob {
                rule: id.clone(),
                field: "from_kind",
                source,
            })?,
        );
    }
    Ok(groups)
}

/// The paths a boundary permits, when it works that way at all.
///
/// `None` when neither allowlist field is set, and that is not the same as an
/// empty set: empty would mean "nothing in this repository may be imported",
/// which is a far louder statement than "this rule does not work by allowlist".
///
/// Refused alongside `forbid_import_from`: "only these, except those" reads as
/// one sentence and is two rules, and two rules is what a reader can follow.
/// `except` is refused too — it shields against a prohibition, and an
/// exception to a *permission* is as meaningless as `RULES.md` already says an
/// exception to a requirement is.
pub(super) fn permitted_paths(
    id: &RuleId,
    rule: &crate::rule::ImportBoundaryRule,
    modules: &Modules,
) -> Result<Option<PathSet>, CompileError> {
    let by_glob = !rule.only_import_from.is_empty();
    let by_module = !rule.only_import_from_modules.is_empty();
    let by_kind = !rule.only_import_from_kinds.is_empty();

    if by_kind {
        if by_glob || by_module {
            return Err(CompileError::ScopeSaidTwice {
                rule: id.clone(),
                one: "only_import_from_kinds",
                other: "only_import_from",
            });
        }
        let mut patterns = Vec::new();
        for kind in &rule.only_import_from_kinds {
            patterns.extend(modules.paths_of_kind(id, kind)?);
        }
        return PathSet::compile(&patterns)
            .map(Some)
            .map_err(|source| CompileError::Glob {
                rule: id.clone(),
                field: "only_import_from_kinds",
                source,
            });
    }

    if by_glob && by_module {
        return Err(CompileError::ScopeSaidTwice {
            rule: id.clone(),
            one: "only_import_from",
            other: "only_import_from_modules",
        });
    }
    if !by_glob && !by_module {
        return Ok(None);
    }
    if !rule.forbid_import_from.is_empty() || !rule.forbid_module.is_empty() {
        return Err(CompileError::AllowlistAndDenylist {
            rule: id.clone(),
            other: "forbid_import_from",
        });
    }
    if !rule.except.is_empty() {
        return Err(CompileError::AllowlistAndDenylist {
            rule: id.clone(),
            other: "except",
        });
    }

    let patterns: Vec<String> = if by_module {
        let mut collected = Vec::new();
        for named in &rule.only_import_from_modules {
            collected.extend(modules.paths_of(id, named)?.iter().cloned());
        }
        collected
    } else {
        rule.only_import_from.iter().cloned().collect()
    };

    PathSet::compile(&patterns)
        .map(Some)
        .map_err(|source| CompileError::Glob {
            rule: id.clone(),
            field: "only_import_from",
            source,
        })
}
