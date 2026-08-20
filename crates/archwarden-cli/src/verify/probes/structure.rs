//! Probes for the shape of a directory.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    traits::{DirectoryContext, RuleEngine},
};
use archwarden_engine::walk::RepoTree;

use crate::verify::probes::{a_directory_in_scope, unclaimed_name};
use crate::verify::{PROBE, Verdict};

/// A directory this rule covers, emptied.
///
/// The cleanest synthesis of the six: a rule that asks for files is violated
/// by a directory with none, and nothing has to be invented -- unlike `naming`,
/// where a violating input is a filename and producing one means running a
/// regex backwards.
pub(crate) fn a_directory_holding_nothing(
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

    let findings = engine.check_directory(DirectoryContext {
        path: directory,
        subdirectories: &[],
        files: &[],
    });

    let on = format!("`{directory}` holding none of the files it requires");
    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}

/// A directory this rule covers, offered a violation of each axis it
/// constrains.
///
/// A `structure` rule constrains two independent things: which subfolders a
/// directory may hold, and what its files may be called. It may constrain
/// either, both, or — the case the command exists to catch — neither.
///
/// Probing only the subfolder axis reported every filename-only rule as
/// enforcing nothing, which is a false negative on the one line a reader acts
/// on: *"5 enforce nothing"* invites deleting five rules that work. So each
/// axis the rule actually constrains gets a probe, and the rule is verified if
/// any of them fires. Only a rule that constrains neither is silent, and that
/// one really does enforce nothing.
pub(crate) fn forbidden_subfolder(
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

    let mut attempted = Vec::new();

    if constrains_subfolders(&rule.kind) {
        let probe = unclaimed_name(&rule.kind);
        let on = format!("an unlisted `{probe}/` folder in `{directory}`");
        let findings = engine.check_directory(DirectoryContext {
            path: directory,
            subdirectories: std::slice::from_ref(&probe),
            files: &[],
        });
        if !findings.is_empty() {
            return Verdict::Fires { on };
        }
        attempted.push(on);
    }

    if constrains_filenames(&rule.kind) {
        // A name no `filename_patterns` regex in this repository accepts: the
        // probe marker, with capitals and an extension the patterns are written
        // against. `unclaimed_filename` checks it rather than assuming.
        let probe = unclaimed_filename(&rule.kind);
        let on = format!("a file named `{probe}` in `{directory}`");
        let findings = engine.check_directory(DirectoryContext {
            path: directory,
            subdirectories: &[],
            files: std::slice::from_ref(&probe),
        });
        if !findings.is_empty() {
            return Verdict::Fires { on };
        }
        attempted.push(on);
    }

    // Neither axis is constrained: the rule asks nothing of the directories it
    // covers, which is exactly the state this command was written to name.
    if attempted.is_empty() {
        return Verdict::Silent {
            on: format!("`{directory}`, which it constrains in no way at all"),
        };
    }

    Verdict::Silent {
        on: attempted.join(", and "),
    }
}

/// Whether the rule says anything about which subfolders may be there.
pub(crate) fn constrains_subfolders(kind: &CompiledRuleKind) -> bool {
    matches!(
        kind,
        CompiledRuleKind::Structure {
            allowed_subfolders: Some(_),
            ..
        }
    ) || matches!(
        kind,
        CompiledRuleKind::Structure {
            subfolder_patterns,
            ..
        } if !subfolder_patterns.is_empty()
    )
}

/// Whether the rule says anything about what the files may be called.
pub(crate) fn constrains_filenames(kind: &CompiledRuleKind) -> bool {
    matches!(
        kind,
        CompiledRuleKind::Structure {
            filename_patterns,
            ..
        } if !filename_patterns.is_empty()
    )
}

/// A filename none of the rule's patterns accept.
///
/// Tried rather than assumed: a rule whose pattern happens to accept the probe
/// would be reported silent for a name it was right to allow, which is the
/// same false negative one layer down.
pub(crate) fn unclaimed_filename(kind: &CompiledRuleKind) -> String {
    let CompiledRuleKind::Structure {
        filename_patterns, ..
    } = kind
    else {
        return PROBE.to_owned();
    };

    // Capitals and an unlikely extension, because the patterns these rules
    // carry are overwhelmingly lower-case-with-dashes over a known suffix.
    for candidate in [
        format!("{PROBE}-INVALID-Name.probe"),
        format!("{PROBE}-INVALID-Name"),
        format!("__{PROBE}__"),
    ] {
        if !filename_patterns
            .iter()
            .any(|pattern| pattern.is_match(&candidate))
        {
            return candidate;
        }
    }

    format!("{PROBE}-INVALID-Name.probe")
}
