//! One synthetic violation per rule kind.
//!
//! Each probe builds the smallest thing its rule should refuse, so `verify` can
//! prove the rule fires rather than assuming it would.

pub(crate) mod declarations;
pub(crate) mod pairing;
pub(crate) mod reach;
pub(crate) mod structure;

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    path::{FileClass, RepoRelPath},
    traits::RuleEngine,
};
use archwarden_engine::walk::RepoTree;

use crate::verify::PROBE;

/// A directory this rule's scope covers.
pub(crate) fn a_directory_in_scope<'a>(
    rule: &CompiledRule,
    tree: &'a RepoTree,
) -> Option<&'a RepoRelPath> {
    tree.directories()
        .map(|(path, _)| path)
        .find(|path| rule.scope.matches_dir(path.as_path()))
}

/// A source file this rule applies to and does not exempt.
pub(crate) fn a_file_in_scope<'a>(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &'a RepoTree,
    except_from: &archwarden_core::glob::PathSet,
) -> Option<&'a RepoRelPath> {
    let _ = rule;
    tree.files()
        .filter(|file| file.class == FileClass::Source)
        .map(|file| &file.path)
        .find(|path| engine.applies_to(path) && !except_from.is_match(path.as_path()))
}

/// A file in this repository that the given globs match.
pub(crate) fn a_file_matching<'a>(
    forbid: &archwarden_core::glob::PathSet,
    tree: &'a RepoTree,
) -> Option<&'a RepoRelPath> {
    tree.files()
        .filter(|file| file.class == FileClass::Source)
        .map(|file| &file.path)
        .find(|path| forbid.is_match(path.as_path()))
}

/// A folder name this rule does not already permit.
///
/// A `structure` rule that happened to allow a folder called
/// `archwarden-probe` would be handed something legal and reported as silent,
/// which is a false accusation in the one command whose job is not to make
/// them.
pub(crate) fn unclaimed_name(kind: &CompiledRuleKind) -> String {
    let CompiledRuleKind::Structure {
        allowed_subfolders,
        warn_subfolders,
        recurse_into,
        ..
    } = kind
    else {
        return PROBE.to_owned();
    };

    let claimed = |name: &str| {
        allowed_subfolders
            .iter()
            .flatten()
            .any(|other| other == name)
            || warn_subfolders.iter().any(|other| other == name)
            || recurse_into.iter().any(|other| other == name)
    };

    let mut name = PROBE.to_owned();
    let mut suffix = 2;
    while claimed(&name) {
        name = format!("{PROBE}-{suffix}");
        suffix += 1;
    }
    name
}
