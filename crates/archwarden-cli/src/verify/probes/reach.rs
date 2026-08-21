//! Probes for what a file exports and what it reaches.

use archwarden_core::{
    compiled::CompiledRule,
    facts::{ExportFact, ExportKind, ExportTags, FileFacts, ImportFact, Span, Visibility},
    hash::ContentHash,
    path::RepoRelPath,
    traits::{Exists, FileContext, RuleEngine},
};
use archwarden_engine::walk::RepoTree;

use crate::verify::probes::{a_directory_in_scope, a_file_in_scope, a_file_matching};
use crate::verify::{PROBE, Verdict};

/// A file whose exports break whichever claim this rule makes.
///
/// Every one of the three is plantable, which is unusual here: a `naming`
/// violation means running a regex backwards and a cycle means two files that
/// resolve against each other, but "has a default", "has one export too many"
/// and "declares no return type" are each one synthetic fact.
///
/// The probe breaks the *first* claim the rule makes, because a rule that fires
/// on any of them has been shown to fire. A rule making none of the three
/// constrains nothing, which is `config doctor`'s sentence rather than this
/// one's.
pub(crate) fn a_file_of_the_wrong_shape(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
    shape: &archwarden_core::compiled::ExportShape,
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
    let Ok(probe) = directory.join(&name) else {
        return Verdict::Unverified {
            why: format!("`{directory}` cannot hold a probe file"),
        };
    };
    if !engine.applies_to(&probe) {
        return Verdict::Unverified {
            why: format!(
                "the rule covers `{directory}` but not a file directly in it, so \
                 the probe has nowhere to sit"
            ),
        };
    }

    let exported = |name: Option<&str>, is_default: bool| ExportFact {
        attributes: Vec::new(),
        name: name.map(ToOwned::to_owned),
        tags: ExportTags::only(ExportKind::Function),
        visibility: Visibility::Public,
        is_default,
        reexport_from: None,
        forwards: None,
        annotations: Vec::new(),
        // No return type declared, which is the `must_return` violation and is
        // inert for the other two claims.
        returns: None,
        span: Span::new(0, 1),
    };

    let mut facts = FileFacts::unparsed(probe.clone(), ContentHash::of(PROBE.as_bytes()));
    let broke = if shape.forbid_default {
        facts.exports.push(exported(None, true));
        "a default export"
    } else if let Some(limit) = shape.max_exports {
        for index in 0..=limit {
            facts
                .exports
                .push(exported(Some(&format!("{PROBE}{index}")), false));
        }
        "one export more than the limit"
    } else if !shape.must_return.is_empty() {
        facts.exports.push(exported(Some("Probe"), false));
        "an exported function declaring no return type"
    } else {
        return Verdict::Unverified {
            why: "the rule makes none of the three claims, so there is nothing \
                  to break -- `config doctor` reports a rule that constrains \
                  nothing"
                .to_owned(),
        };
    };

    let findings = engine.check_file(FileContext {
        path: &probe,
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

    let on = format!("`{probe}` with {broke}");
    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}

/// A file this rule covers, importing something the rule forbids.
pub(crate) fn crossed_boundary(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
    forbid: &archwarden_core::glob::PathSet,
    forbid_packages: &[String],
    except_from: &archwarden_core::glob::PathSet,
) -> Verdict {
    let Some(importer) = a_file_in_scope(rule, engine, tree, except_from) else {
        return Verdict::Unverified {
            why: format!(
                "no source file in this repository is inside `{}` without being \
                 exempted by `except_from`",
                rule.scope.patterns().join("`, `")
            ),
        };
    };

    // The forbidden path half first: it is the half that needs resolution, and
    // so the half most likely to be enforcing nothing.
    let (import, on) = if forbid.is_empty() {
        let Some(package) = forbid_packages.first() else {
            return Verdict::Unverified {
                why: "the rule only requires an import, and a file that imports \
                      nothing is not a violation this can tell apart from a file \
                      the rule does not cover"
                    .to_owned(),
            };
        };
        (
            probe_import(package.clone(), None),
            format!("`{importer}` importing the package `{package}`"),
        )
    } else {
        let Some(target) = a_file_matching(forbid, tree) else {
            return Verdict::Unverified {
                why: format!(
                    "no file in this repository matches `{}`, so there is nothing \
                     for a probe to import",
                    forbid.patterns().join("`, `")
                ),
            };
        };
        (
            probe_import(format!("./{}", target.as_str()), Some(target.clone())),
            format!("`{importer}` importing `{target}`"),
        )
    };

    let mut facts = FileFacts::unparsed(importer.clone(), ContentHash::of(PROBE.as_bytes()));
    facts.imports.push(import);

    let findings = engine.check_file(FileContext {
        path: importer,
        facts: Some(&facts),
        docs: None,
        siblings: &[],
        exists: Exists::none(),
        graph: None,
        // The probe asks whether the rule bites *now*, so it answers for
        // today. A deadline planted in 1970 is past on every run there
        // has ever been.
        as_of: archwarden_core::date::Date::today(),
    });

    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}

/// An import as the parser would have recorded it, already resolved.
///
/// `type_only` is false: a rule with `include_type_only: false` still catches a
/// value import, so this is the probe that asks the least of the rule.
pub(crate) fn probe_import(specifier: String, resolved: Option<RepoRelPath>) -> ImportFact {
    ImportFact {
        specifier,
        resolved,
        type_only: false,
        names: Vec::new(),
        span: Span::new(0, 1),
    }
}

/// A file this rule covers, outside the chokepoint, calling what it guards.
///
/// Plantable where `forbid_reaching` is not: a breach is one file with one
/// call in it, not a chain that has to resolve against a second file.
///
/// The probe has to sit in a directory the rule covers and `only_in` does
/// *not*. A repository whose whole scope is inside the chokepoint has nowhere
/// to put one -- and that is a rule `config doctor` should be reporting rather
/// than a failure of this probe, so it is named as unverified with the reason.
pub(crate) fn a_call_from_outside_the_chokepoint(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
    callee: &[String],
    renders: &[String],
    only_in: &archwarden_core::scope::Scope,
) -> Verdict {
    // A call if the rule guards one, otherwise a render. Either proves the
    // rule bites; planting both would prove it twice. Issue #145.
    let rendered = callee.is_empty();
    let Some(guarded) = callee.first().or_else(|| renders.first()) else {
        return Verdict::Unverified {
            why: "the rule guards no callee and no element, so there is \
                  nothing to break -- `config doctor` reports a rule that \
                  constrains nothing"
                .to_owned(),
        };
    };

    let outside = tree
        .directories()
        .map(|(path, _)| path)
        .filter(|path| rule.scope.matches_dir(path.as_path()))
        .find(|path| !only_in.matches_dir(path.as_path()));

    let Some(directory) = outside else {
        return Verdict::Unverified {
            why: format!(
                "every directory this rule covers is inside `{}`, so there is \
                 nowhere outside the chokepoint to plant a call",
                only_in.patterns().join("`, `")
            ),
        };
    };

    let name = format!("{PROBE}.ts");
    let Ok(probe) = directory.join(&name) else {
        return Verdict::Unverified {
            why: format!("`{directory}` cannot hold a probe file"),
        };
    };

    let mut facts = FileFacts::unparsed(probe.clone(), ContentHash::of(PROBE.as_bytes()));
    if rendered {
        facts.renders.push(archwarden_core::facts::RenderFact {
            name: guarded.clone(),
            span: Span::new(0, 1),
        });
    } else {
        facts.calls.push(archwarden_core::facts::CallFact {
            callee: guarded.clone(),
            arguments: Vec::new(),
            options: Vec::new(),
            span: Span::new(0, 1),
        });
    }

    let findings = engine.check_file(FileContext {
        path: &probe,
        facts: Some(&facts),
        docs: None,
        siblings: std::slice::from_ref(&name),
        exists: Exists::none(),
        graph: None,
        as_of: archwarden_core::date::Date::today(),
    });

    let verb = if rendered { "rendering" } else { "calling" };
    let on = format!("`{probe}` {verb} `{guarded}`");
    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}

/// A file this rule already covers, with the export it demands taken away.
///
/// `naming` was the one kind whose bite this command could not demonstrate.
/// Inventing a filename that satisfies a `file_pattern` means running a regex
/// backwards, which archwarden's engine deliberately cannot do -- it is
/// linear-time and has no generator.
///
/// So the probe does not invent a name. It takes a file the rule *already*
/// matches out of the tree, and hands the engine facts for it with no exports
/// at all. The rule renders its own template against that real path, finds
/// nothing answering to it, and fires -- which is the demonstration.
///
/// Nothing is written and nothing is read: the path is real, the facts are
/// synthetic, and the file on disk is never opened.
///
/// It costs a rule that matches no file its verdict, which is honest: a
/// `naming` rule reaching nothing is `config doctor`'s `scope-matches-nothing`
/// rather than this command's business. Issue #154.
pub(crate) fn a_covered_file_without_its_export(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
) -> Verdict {
    let Some(covered) = a_file_in_scope(
        rule,
        engine,
        tree,
        &archwarden_core::glob::PathSet::default(),
    ) else {
        return Verdict::Unverified {
            why: format!(
                "no file this rule covers exists yet, so there is nothing to \
                 take an export away from -- `{}` selects the directories and \
                 the `file_pattern` the names in them",
                rule.scope.patterns().join("`, `")
            ),
        };
    };

    let path = covered.clone();
    let name = path.file_name().unwrap_or_default().to_owned();
    // Exports emptied rather than renamed: an invented name could collide with
    // whatever the template renders to, and a probe that accidentally passes
    // reports a rule as silent when it is not.
    let facts = FileFacts::unparsed(path.clone(), ContentHash::of(PROBE.as_bytes()));

    let findings = engine.check_file(FileContext {
        path: &path,
        facts: Some(&facts),
        docs: None,
        siblings: std::slice::from_ref(&name),
        exists: Exists::none(),
        graph: None,
        as_of: archwarden_core::date::Date::today(),
    });

    let on = format!("`{path}` exporting nothing");
    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}
