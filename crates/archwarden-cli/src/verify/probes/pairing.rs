//! Probes for a file that should have a companion.

use archwarden_core::{
    compiled::CompiledRule,
    facts::{ExportFact, ExportKind, ExportTags, FileFacts, Span, Visibility},
    hash::ContentHash,
    traits::{Exists, FileContext, RuleEngine},
};
use archwarden_engine::walk::RepoTree;

use crate::verify::probes::a_directory_in_scope;
use crate::verify::{PROBE, Verdict};

/// A source file this rule covers, with no spec beside it.
///
/// The issue expected this one to be impossible -- "the violation is the
/// *absence* of a file, which cannot be synthesised as a file at all". It can.
/// The rule is offered one file at a time together with what else is in the
/// folder, so a file whose only sibling is itself *is* the absence, and no
/// spec has to exist anywhere for the rule to be asked about it.
///
/// The probe carries a function export because `skip_type_only` exempts a file
/// with nothing at runtime to test, and a probe that tripped over that
/// exemption would report a working rule as silent.
pub(crate) fn a_file_with_no_spec(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
) -> Verdict {
    let Some(directory) = a_directory_in_scope(rule, tree) else {
        return Verdict::Unverified {
            why: format!(
                "no directory in this repository is inside `{}`",
                rule.scope.patterns().join("`, `")
            ),
        };
    };

    let name = format!("{PROBE}.ts");
    let Ok(lonely) = directory.join(&name) else {
        return Verdict::Unverified {
            why: format!("`{directory}` cannot hold a probe file"),
        };
    };

    if !engine.applies_to(&lonely) {
        return Verdict::Unverified {
            why: format!(
                "the rule covers `{directory}` but not a file directly in it, so \
                 the probe has nowhere to sit"
            ),
        };
    }

    let mut facts = FileFacts::unparsed(lonely.clone(), ContentHash::of(PROBE.as_bytes()));
    facts.exports.push(ExportFact {
        name: Some("Probe".to_owned()),
        tags: ExportTags::only(ExportKind::Function),
        visibility: Visibility::Public,
        is_default: false,
        reexport_from: None,
        forwards: None,
        annotations: Vec::new(),
        returns: None,
        span: Span::new(0, 1),
    });

    let findings = engine.check_file(FileContext {
        path: &lonely,
        facts: Some(&facts),
        docs: None,
        siblings: std::slice::from_ref(&name),
        exists: Exists::none(),
        graph: None,
        // The probe asks whether the rule bites *now*, so it answers for
        // today. A deadline planted in 1970 is past on every run there
        // has ever been.
        as_of: archwarden_core::date::Date::today(),
    });

    let on = format!("`{lonely}` with no spec beside it");
    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}

/// A file added under a freeze.
///
/// The probe is the whole rule: every file under the scope is a finding, so
/// planting one is planting a path. What `baseline` would do with it is not
/// this command's question — `verify-rules` asks whether the rule *bites*, and
/// an accepted finding is a finding that fired.
pub(crate) fn a_file_added_to_a_freeze(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
) -> Verdict {
    let Some(directory) = a_directory_in_scope(rule, tree) else {
        return Verdict::Unverified {
            why: format!(
                "no directory in this repository is inside `{}`",
                rule.scope.patterns().join("`, `")
            ),
        };
    };

    let name = format!("{PROBE}.ts");
    let Ok(added) = directory.join(&name) else {
        return Verdict::Unverified {
            why: format!("`{directory}` cannot hold a probe file"),
        };
    };

    let facts = FileFacts::unparsed(added.clone(), ContentHash::of(PROBE.as_bytes()));
    let findings = engine.check_file(FileContext {
        path: &added,
        facts: Some(&facts),
        docs: None,
        siblings: std::slice::from_ref(&name),
        exists: Exists::none(),
        graph: None,
        // The probe asks whether the rule bites *now*, so it answers for
        // today. A deadline planted in 1970 is past on every run there
        // has ever been.
        as_of: archwarden_core::date::Date::today(),
    });

    let on = format!("`{added}`, a file added under the freeze");
    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}

/// A file this rule covers whose counterpart is not on disk.
///
/// `Exists::none()` is the whole probe: the counterpart the template names is
/// absent by construction, which is exactly the violation.
pub(crate) fn a_file_with_no_counterpart(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
) -> Verdict {
    let Some(covered) = tree
        .files()
        .map(|file| &file.path)
        .find(|path| engine.applies_to(path))
    else {
        return Verdict::Unverified {
            why: format!(
                "no file in this repository matches this rule's `file_pattern` \
                 inside `{}`",
                rule.scope.patterns().join("`, `")
            ),
        };
    };

    let findings = engine.check_file(FileContext {
        path: covered,
        facts: None,
        docs: None,
        siblings: &[],
        // Nothing exists, so the counterpart certainly does not.
        exists: Exists::none(),
        graph: None,
        // The probe asks whether the rule bites *now*, so it answers for
        // today. A deadline planted in 1970 is past on every run there
        // has ever been.
        as_of: archwarden_core::date::Date::today(),
    });

    let on = format!("`{covered}`, whose counterpart is not on disk");
    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}
