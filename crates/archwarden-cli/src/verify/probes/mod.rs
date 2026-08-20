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

#[cfg(test)]
mod tests {
    use super::*;

    fn structure(allowed: Option<&[&str]>, warn: &[&str], recurse: &[&str]) -> CompiledRuleKind {
        let own = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        CompiledRuleKind::Structure {
            allowed_subfolders: allowed.map(own),
            warn_subfolders: own(warn),
            recurse_into: own(recurse),
            subfolder_patterns: Vec::new(),
            filename_patterns: Vec::new(),
        }
    }

    /// The plain probe, when the rule claims nothing that collides with it.
    ///
    /// Asserted as an equality rather than "is not empty": a probe named
    /// something else would still be unclaimed and would still be wrong, since
    /// the marker is what a reader recognises in the report.
    #[test]
    fn an_unclaimed_probe_keeps_its_plain_name() {
        assert_eq!(unclaimed_name(&structure(None, &[], &[])), PROBE);
        assert_eq!(
            unclaimed_name(&structure(Some(&["calcs"]), &[], &[])),
            PROBE
        );
    }

    /// A rule of another kind has no folder names to collide with.
    #[test]
    fn a_rule_that_is_not_about_folders_gets_the_plain_probe() {
        assert_eq!(unclaimed_name(&CompiledRuleKind::Frozen), PROBE);
    }

    /// Each of the three lists is checked, and each on its own.
    ///
    /// A rule that happened to permit the probe would be handed something legal
    /// and reported as enforcing nothing -- a false accusation in the one
    /// command whose job is not to make them. Three separate assertions because
    /// the check is three `||`ed clauses, and one test covering the first would
    /// pass while the other two stopped being consulted.
    #[test]
    fn every_list_the_rule_claims_is_avoided() {
        assert_eq!(
            unclaimed_name(&structure(Some(&[PROBE]), &[], &[])),
            "archwarden-probe-2"
        );
        assert_eq!(
            unclaimed_name(&structure(None, &[PROBE], &[])),
            "archwarden-probe-2"
        );
        assert_eq!(
            unclaimed_name(&structure(None, &[], &[PROBE])),
            "archwarden-probe-2"
        );
    }

    /// The search walks forward until it finds one nobody claimed.
    ///
    /// Exactly two claimed names, and the answer is the third. The count is
    /// chosen so the step is pinned rather than merely exercised: claiming
    /// three would let a suffix that doubled -- 2, 4 -- land on the same answer
    /// as one that counted, and the test would pass while the walk was wrong.
    #[test]
    fn the_suffix_climbs_until_the_name_is_free() {
        assert_eq!(
            unclaimed_name(&structure(Some(&[PROBE, "archwarden-probe-2"]), &[], &[])),
            "archwarden-probe-3",
        );
    }
}
