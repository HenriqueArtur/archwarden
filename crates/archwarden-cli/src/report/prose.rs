//! One sentence per observation, and one per expectation.
//!
//! The two are the same table read from opposite sides, which is why they sit
//! together: a finding puts them in one sentence and they have to agree.

use std::fmt::Write as _;

use archwarden_api::describe::join_or;
use archwarden_core::{finding::Expectation, path::RepoRelPath};
use archwarden_engine::run::Report;

use super::text::plural;

/// Skips on files the unreadable-file notes above do not account for.
///
/// The other half of the same number. A skip on a file that *is* named above is
/// a bug to investigate; a skip on one that is not was never attempted -- a
/// language this configuration did not ask archwarden to read, most often.
/// `1 skipped` could not tell those apart, and they are opposite decisions.
/// Issue #13.
pub(super) fn render_unattempted_skips(report: &Report, out: &mut dyn std::io::Write) {
    let mut by_path: std::collections::BTreeMap<&RepoRelPath, Vec<&str>> =
        std::collections::BTreeMap::new();

    for (rule, path) in &report.skipped_checks {
        let named_above = report
            .unreadable_files
            .iter()
            .any(|(unreadable, _)| unreadable == path);
        if !named_above {
            by_path.entry(path).or_default().push(rule.as_str());
        }
    }

    for (path, rules) in by_path {
        let _ = writeln!(
            out,
            "note: `{path}` was not read, so {} {} skipped there: {}",
            rules.len(),
            plural(rules.len(), "check was", "checks were"),
            rules.join(", "),
        );
    }
}

/// What a file must declare about itself, in a block or in a header.
///
/// Three clauses, in the order the rule reads them: the keys, then the closed
/// vocabularies, then the agreements with the path. Each is skipped when the
/// rule did not ask for it, so a rule that only names keys gets one clause.
///
/// `frontmatter` and `metadata` share every clause but the first, because they
/// are the same three questions asked of two file formats. Only the lead
/// differs, and it has to: a reader following a finding about a `.ts` file must
/// not be sent looking for a YAML block.
pub(super) fn describe_declarations(
    lead: &str,
    keys: &[String],
    vocabularies: &[(String, Vec<String>)],
    agreements: &[(String, String)],
) -> String {
    let mut parts = Vec::new();

    if !keys.is_empty() {
        let quoted: Vec<&str> = keys.iter().map(String::as_str).collect();
        parts.push(format!("{lead} {}", join_and(&quoted)));
    }
    for (key, accepted) in vocabularies {
        let quoted: Vec<&str> = accepted.iter().map(String::as_str).collect();
        parts.push(format!(
            "with `{key}` one of {}",
            join_or(&quoted, "nothing")
        ));
    }
    for (key, wanted) in agreements {
        parts.push(format!("and `{key}` equal to `{wanted}`"));
    }

    parts.join(", ")
}

/// `a`, `b` and `c` — for a list where every entry is required.
pub(super) fn join_and(items: &[&str]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| format!("`{item}`")).collect();
    match quoted.split_last() {
        None => "nothing".to_owned(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// What must live inside a directory, by name and by shape.
///
/// "and", never "or": every entry is required, and the `join_or` this module
/// uses everywhere else would say the opposite of the rule.
pub(super) fn describe_required_files(names: &[String], patterns: &[String]) -> String {
    let mut parts = Vec::new();

    if !names.is_empty() {
        let quoted: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
        parts.push(match quoted.split_last() {
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
            None => String::new(),
        });
    }

    for pattern in patterns {
        let clause = format!("a file matching `{pattern}`");
        parts.push(if parts.is_empty() {
            clause
        } else {
            format!("and {clause}")
        });
    }

    parts.join(", ")
}

/// What may live inside a directory, by name and by shape.
///
/// Split out of [`describe_expectation`] because it is the one arm with three
/// clauses to compose, and because "one of no folders" -- the sentence a rule
/// constraining by shape alone used to get -- describes the opposite of that
/// rule. The enumeration clause appears only when there is an enumeration.
pub(super) fn describe_subfolders(
    allowed: &[String],
    warn: &[String],
    patterns: &[String],
) -> String {
    let mut parts = Vec::new();

    if !allowed.is_empty() || patterns.is_empty() {
        parts.push(format!("one of {}", join_or(allowed, "no folders")));
    }
    if !warn.is_empty() {
        parts.push(format!("or {} as a warning", join_or(warn, "")));
    }
    if !patterns.is_empty() {
        // "a folder name", not "a name". The same sentence served
        // `filename_patterns` and `subfolder_patterns`, which are siblings
        // constraining different kinds of entry, so a reader could not tell
        // which was meant. `check` had always distinguished them — "folder
        // `x` is not allowed here" — and `describe` had not.
        let clause = format!("a folder name matching {}", join_or(patterns, ""));
        parts.push(if parts.is_empty() {
            clause
        } else {
            format!("or {clause}")
        });
    }

    parts.join(", ")
}

/// What a directory's own name may be, said to that directory.
///
/// The same lists as [`describe_subfolders`], in the second person: that one
/// tells a parent what may appear inside it, this one tells a child what it
/// may be called.
pub(super) fn describe_folder_name(
    allowed: &[String],
    warn: &[String],
    patterns: &[String],
) -> String {
    let mut parts = Vec::new();

    if !allowed.is_empty() {
        parts.push(format!("named one of {}", join_or(allowed, "")));
    }
    if !warn.is_empty() {
        parts.push(format!("or {} as a warning", join_or(warn, "")));
    }
    if !patterns.is_empty() {
        let clause = format!("a folder name matching {}", join_or(patterns, ""));
        parts.push(if parts.is_empty() {
            clause
        } else {
            format!("or {clause}")
        });
    }

    if parts.is_empty() {
        // A parent that permits no subfolder at all. Said plainly, because the
        // alternative reads as a rule with nothing to say.
        return "no folder here at all: its parent allows none".to_owned();
    }

    parts.join(", ")
}

#[allow(
    clippy::too_many_lines,
    reason = "the same table as `describe_observed`, from the other side: one \
              arm per expectation, and the two have to read as one sentence \
              when a finding puts them together"
)]
pub(crate) fn describe_expectation(expectation: &Expectation) -> String {
    match expectation {
        Expectation::AllowedSubfolders {
            allowed,
            warn,
            patterns,
        } => describe_subfolders(allowed, warn, patterns),
        Expectation::RequiredFiles { names, patterns } => describe_required_files(names, patterns),
        // The `expected:` line beside a forbidden file. It names the whole
        // list rather than the one file found, because the reader's question
        // after "delete this" is "what else may not be here". Issue #177.
        // `join_or` quotes each entry itself; quoting here too was two pairs
        // of backticks around every name.
        Expectation::ForbiddenFiles { names } => format!(
            "none of {}",
            archwarden_api::describe::join_or(names, "nothing")
        ),
        Expectation::RequiredCompanion { path } => format!("`{path}` beside it"),
        Expectation::RequiredFrontmatter {
            keys,
            vocabularies,
            agreements,
        } => describe_declarations("frontmatter carrying", keys, vocabularies, agreements),
        Expectation::DeclaredMetadata {
            keys,
            vocabularies,
            agreements,
        } => describe_declarations("a header declaring", keys, vocabularies, agreements),
        Expectation::FilenamePattern { patterns } => {
            format!("a file name matching {}", join_or(patterns, "no pattern"))
        }
        Expectation::FolderName {
            allowed,
            warn,
            patterns,
        } => describe_folder_name(allowed, warn, patterns),
        Expectation::RequiredExport {
            name,
            annotation,
            signature_hint,
            ..
        } => {
            let mut sentence = format!("an export named `{name}`");
            // The checked clause first, and the suggestion after it. A reader
            // acting on one of the two should reach the enforced one first.
            if !annotation.is_empty() {
                let accepted: Vec<&str> = annotation.iter().map(String::as_str).collect();
                let _ = write!(
                    sentence,
                    ", annotated {}",
                    join_or(&accepted, "no type at all")
                );
            }
            if let Some(hint) = signature_hint {
                let _ = write!(sentence, ", shaped like `{hint}`");
            }
            sentence
        }
        Expectation::NoNewFiles => {
            "no file to be added here -- what is present today is accepted by \
             `.archwarden/baseline.json`, and a path it does not carry is new"
                .to_owned()
        }
        Expectation::RequiredCounterpart { path } => {
            format!("a counterpart at `{path}`")
        }
        Expectation::NoDefaultExport => {
            "no default export -- an importer names a default itself, so the \
             name it is reached by is not the one it was given"
                .to_owned()
        }
        Expectation::AtMostExports { limit } => format!(
            "at most {limit} {} exported, not counting `type` and `interface`",
            if *limit == 1 { "symbol" } else { "symbols" }
        ),
        Expectation::RequiredReturnType { patterns } => format!(
            "every exported function to declare a return type matching {}",
            patterns
                .iter()
                .map(|pattern| format!("`{pattern}`"))
                .collect::<Vec<_>>()
                .join(" or ")
        ),
        Expectation::NoPassthrough { forms } => {
            format!(
                "a file must add something of its own, not only {}",
                forms
                    .iter()
                    .map(|form| match form.as_str() {
                        "reexport" => "re-export another module",
                        "alias" => "rename an import",
                        "wrapper" => "wrap a call in a one-line function",
                        other => other,
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        Expectation::DeclaredName { named, attribute } => attribute.as_ref().map_or_else(
            || format!("a declaration named `{named}`"),
            |held| format!("a declaration named `{named}`, carrying `#[{held}]`"),
        ),
        Expectation::CallNaming { named, callee } => {
            format!("a `{callee}` naming `{named}` somewhere in scope")
        }
        Expectation::RequiredSibling {
            path,
            non_empty_spec,
        } => {
            if *non_empty_spec {
                format!("`{path}`, containing at least one test case")
            } else {
                format!("`{path}`")
            }
        }
        Expectation::PermittedImports { patterns, .. } => format!(
            "imports only from {} (its own files always, packages separately)",
            join_or(patterns, "nowhere")
        ),
        Expectation::PermittedPackages { packages } => {
            format!("imports only these packages: {}", join_or(packages, "none"))
        }
        Expectation::ForbiddenImport {
            patterns, except, ..
        } => {
            let base = format!("no import from {}", join_or(patterns, "anywhere"));
            if except.is_empty() {
                base
            } else {
                format!("{base}, except {}", join_or(except, ""))
            }
        }
        Expectation::GovernedBySomeRule => {
            "some rule to govern it, or an `ignore` entry saying it is outside \
             the architecture on purpose"
                .to_owned()
        }
        Expectation::NoImportCycle => "no import cycle through it".to_owned(),
        // "depend on" rather than "import from", so that a report carrying both
        // this and a `ForbiddenImport` does not read as the same sentence
        // twice. They are different obligations and one edit rarely satisfies
        // both.
        Expectation::ForbiddenReach {
            patterns, except, ..
        } => {
            let base = format!(
                "not to depend on {}, at any distance",
                join_or(patterns, "anything")
            );
            if except.is_empty() {
                base
            } else {
                format!("{base}, except {}", join_or(except, ""))
            }
        }
        Expectation::ForbiddenPackages {
            packages,
            except_from,
            ..
        } => {
            let base = format!("no import of {}", join_or(packages, "any package"));
            if except_from.is_empty() {
                base
            } else {
                format!("{base}, except from {}", join_or(except_from, ""))
            }
        }
        Expectation::RequiredImport { patterns } => {
            format!("an import from {}", join_or(patterns, "somewhere"))
        }
        // An empty allowlist is a sentence of its own, not a list with a
        // fallback word in it. `join_or(only_in, "anywhere")` read `outside
        // anywhere`, which is the opposite of what the rule means -- and this
        // is the string `describe`, `scaffold` and the pre-write hook all say,
        // so an agent was being told the reverse. Issue #168.
        // Two clauses when the rule makes two statements. A call and a render
        // are different relationships, and "no use of `Card`" would send a
        // reader looking for a call site that is not there. Issue #145.
        Expectation::UsedOnlyIn {
            callee,
            renders,
            only_in,
        } => {
            let mut said = Vec::new();
            if !callee.is_empty() {
                said.push(format!("no use of {}", join_or(callee, "anything")));
            }
            if !renders.is_empty() {
                said.push(format!("no render of {}", join_or(renders, "anything")));
            }
            if said.is_empty() {
                said.push("no use of anything".to_owned());
            }
            let guarded = said.join(" and ");

            if only_in.is_empty() {
                format!("{guarded} here at all")
            } else {
                format!("{guarded} outside {}", join_or(only_in, "anywhere"))
            }
        }
        Expectation::RequiredCall {
            symbol,
            imported_from,
            with_options,
        } => format!(
            "a call to `{symbol}`, imported from `{imported_from}`{}",
            archwarden_api::guide::passing(
                with_options
                    .iter()
                    .map(|option| (&option.key, option.value.as_ref()))
            )
        ),
        other => format!("{other:?}"),
    }
}
