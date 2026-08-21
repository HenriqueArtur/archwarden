//! `check` and the commands that inspect a ruleset.

use archwarden_config::extends::MergedConfig;
use camino::Utf8Path;

use crate::command::{By, LevelFilter, Location, Output};
use crate::commands::query::prepare;
use crate::{diagnostic::ConfigDiagnostic, exit::Exit, report::Format};

/// What `check` was asked to do.
///
/// A struct because the four filters plus the two switches are six arguments,
/// and six positional booleans and slices at a call site is a place transposed
/// arguments go to hide.
pub(crate) struct CheckOptions<'a> {
    pub(crate) format: Format,
    pub(crate) html: Option<&'a str>,
    pub(crate) language: Option<crate::phrases::Language>,
    pub(crate) no_cache: bool,
    pub(crate) summary: bool,
    pub(crate) rules: &'a [String],
    pub(crate) paths: &'a [String],
    pub(crate) changed: Option<&'a str>,
    pub(crate) level: Option<LevelFilter>,
    pub(crate) no_baseline: bool,
    pub(crate) by: Option<By>,
    pub(crate) as_of: Option<&'a str>,
}

#[allow(
    clippy::too_many_lines,
    reason = "one command, in the order it happens: load, walk, run, filter \
              against the baseline, render, write the page, decide the exit \
              code. Splitting it would hide that the exit code is taken from \
              what the baseline did not accept and never from what was shown"
)]
pub(crate) fn check(
    location: Location<'_>,
    working_directory: &Utf8Path,
    options: &CheckOptions<'_>,
    output: &mut Output<'_>,
) -> Exit {
    // From here rather than from `main`: argument parsing is not the run, and
    // a number that moved with clap's work would not be the one a user is
    // comparing between two invocations.
    let started = std::time::Instant::now();

    let (merged, compiled) = match prepare(location, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    // Read before the walk, so a broken baseline costs a message rather than a
    // full run the user then has to repeat.
    let baseline = if options.no_baseline {
        None
    } else {
        match crate::baseline::Baseline::load(&merged.root) {
            Ok(baseline) => baseline,
            Err(message) => {
                let _ = writeln!(output.err, "{message}");
                return Exit::ConfigProblem;
            }
        }
    };

    // Asked of git before the walk, so a bad ref costs a message rather than a
    // full run the user then has to repeat.
    let changed = match options.changed {
        Some(reference) => match crate::changed::changed_files(&merged.root, reference) {
            Ok(paths) => Some(paths),
            Err(message) => {
                let _ = writeln!(output.err, "{message}");
                return Exit::ConfigProblem;
            }
        },
        None => None,
    };

    // Before the walk too, so a mistyped rule id costs the same.
    let filters = match crate::filter::Filters::compile(
        crate::filter::Arguments {
            rules: options.rules,
            paths: options.paths,
            changed,
            // The one step from the word a user typed to the severity the
            // filter matches on.
            level: options.level.map(LevelFilter::level),
        },
        &compiled,
    ) {
        Ok(filters) => filters,
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    };

    // Refused before the walk, so a date nobody can read costs a message
    // rather than a run. A lenient parser here would be the worst outcome the
    // feature has: `01/12/2026` read as a date puts the deadline eleven months
    // from where it was meant to be.
    let as_of = match options
        .as_of
        .map(|written| (written, archwarden_core::date::Date::parse(written)))
    {
        None => archwarden_core::date::Date::today(),
        Some((_, Some(date))) => date,
        Some((written, None)) => {
            let _ = writeln!(
                output.err,
                "`--as-of {written}` is not a date; write it as `YYYY-MM-DD`"
            );
            return Exit::ConfigProblem;
        }
    };

    let tree = match walked(&merged.root, working_directory, &compiled, output) {
        Ok(tree) => tree,
        Err(exit) => return exit,
    };

    let evaluated = archwarden_api::evaluate(&archwarden_api::Evaluation {
        root: &merged.root,
        compiled: &compiled,
        tree: &tree,
        cache: if options.no_cache {
            archwarden_api::CachePolicy::Ignore
        } else {
            archwarden_api::CachePolicy::Use
        },
        as_of,
    });

    // A cache that would not open, or would not persist, costs the next run
    // its speed and nothing else — so it is a note on stderr rather than a
    // failure. Which of those it is, the operation decided; that it is worth
    // saying out loud, this decides.
    for note in &evaluated.notes {
        let _ = writeln!(output.err, "note: {note}");
    }
    let outcome = evaluated.report;

    // The baseline, then the filters, then the shape. The order is not a
    // preference and no surface decides it: `archwarden_api::present` does,
    // because reversing the first two would let a reading preference decide
    // whether a build passes.
    let presented = archwarden_api::present(
        &outcome,
        baseline.as_ref(),
        &filters,
        archwarden_api::Shape {
            // `--by` implies `--summary`: counting by area only means anything
            // as counts, and making someone pass both to say one thing is
            // friction with no reading behind it.
            axis: options
                .by
                .map(By::axis)
                .or(options.summary.then_some(archwarden_api::Axis::Rule)),
        },
        &compiled,
    );

    // Worked out before the report is written rather than after it, because in
    // `--format json` it travels *inside* the document. Issue #110.
    let standing = baseline
        .as_ref()
        .map(|baseline| baseline.standing(&outcome.findings, &compiled));

    crate::report::render(
        &crate::report::Rendered {
            root: &merged.root,
            report: &outcome,
            view: &presented.view,
            reasons: &crate::report::Reasons::of(&compiled),
            elapsed: started.elapsed(),
            standing: standing.clone(),
            as_of,
        },
        options.format,
        output.out,
    );

    if let Some(destination) = options.html {
        let page = crate::report::html_page(
            &compiled,
            &tree,
            &outcome,
            &presented.unaccepted,
            baseline.as_ref(),
            options
                .language
                .unwrap_or_else(|| crate::phrases::Language::of(merged.config.language)),
        );
        match std::fs::write(destination, page) {
            Ok(()) => {
                let _ = writeln!(
                    output.aside(options.format),
                    "page written to {destination}"
                );
            }
            // Reported and not fatal: the gate already ran, and refusing its
            // exit code because a side artefact could not be written would let
            // a full disk turn a failing build green.
            Err(error) => {
                let _ = writeln!(output.err, "note: cannot write {destination}: {error}");
            }
        }
    }

    // The prose form, for somebody at a terminal. `--format json` said it
    // inside the document, under `summary.baseline`, and saying it twice would
    // be the trailing text again.
    if let Some(standing) = &standing
        && options.format == crate::report::Format::Text
    {
        report_standing(standing, output.out);
    }

    // One question, asked of the thing that knows the rule. `fails_build` reads
    // what the baseline did not accept and never the view, and there is no
    // other way to ask it -- which is what stops a surface deciding for itself
    // that a narrowed run is a passing one.
    if presented.fails_build() {
        Exit::Errors
    } else {
        Exit::Clean
    }
}

/// How this run stands against the baseline, as a sentence.
///
/// Written on every text run that has one, deliberately. A baseline nobody is
/// reminded of is a suppression file, and the entries that no longer occur are
/// the only cheerful number archwarden has -- as well as the thing that stops
/// a stale entry hiding a violation that came back.
///
/// The number, not the sentence, is what `--format json` carries, under
/// `summary.baseline`. Two renderings of one fact rather than one rendering on
/// two streams: a document that ends and then keeps writing is not a document,
/// which is what issue #110 was filed about.
pub(crate) fn report_standing(
    standing: &archwarden_api::baseline::Standing,
    out: &mut dyn std::io::Write,
) {
    let _ = write!(out, "{} accepted", standing.accepted);

    if standing.gone > 0 {
        let _ = write!(
            out,
            ", {} no longer {} — run `archwarden baseline` to update",
            standing.gone,
            if standing.gone == 1 {
                "occurs"
            } else {
                "occur"
            }
        );
    }

    let _ = writeln!(out);
}

/// Walks the repository, rendering a refusal as this surface says it.
///
/// The walk and the refusal itself are [`archwarden_api::walk`], including why
/// the refusal is narrow. What is left here is the rendering — and the help,
/// which is the CLI's alone: it names `--root`, and a surface with no command
/// line needs a different sentence for the same fact.
pub(crate) fn walked(
    root: &Utf8Path,
    working_directory: &Utf8Path,
    compiled: &archwarden_core::compiled::CompiledConfig,
    output: &mut Output<'_>,
) -> Result<archwarden_engine::walk::RepoTree, Exit> {
    archwarden_api::walk(root, working_directory, compiled).map_err(|error| {
        let report = miette::Report::new(ConfigDiagnostic::from_api_error(&error));
        let _ = writeln!(output.err, "{report:?}");
        Exit::ConfigProblem
    })
}

/// Looks for a configuration that parses and is still wrong.
///
/// Exits clean even with concerns. They are advice about a configuration, not
/// findings about code, and a non-zero exit would put them in a CI gate where
/// a deliberate choice would start failing builds.
/// Hands every rule a violation and reports which ones did not notice.
///
/// A rule that enforces nothing is indistinguishable from a repository that
/// satisfies it, and `explain` cannot tell them apart: it answers about
/// coverage, and this answers about efficacy. Issue #24, whose author settled
/// the question by planting a file with three escapes in it, running `check`,
/// and deleting it again.
///
/// Needs the walked tree, because the probe is placed at a path this repository
/// actually has. See [`crate::verify`] for why that is not a glob generator.
pub(crate) fn verify_rules(
    location: Location<'_>,
    working_directory: &Utf8Path,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let Ok((merged, compiled)) = prepare(location, working_directory, output) else {
        return Exit::ConfigProblem;
    };

    let tree = match walked(&merged.root, working_directory, &compiled, output) {
        Ok(tree) => tree,
        Err(exit) => return exit,
    };

    let verifications = crate::verify::verify(&compiled, &tree);
    crate::verify::render(&verifications, format, output.out);

    if verifications
        .iter()
        .any(|verification| verification.verdict.is_silent())
    {
        Exit::Errors
    } else {
        Exit::Clean
    }
}

/// `config coverage` — which files no rule governs.
///
/// Reports and does not fail: issue #59 says the number is worth having on its
/// own, and nobody should be asked to enable a gate before they can see what
/// it would cost. The gate is `governance: closed`, issue #60.
pub(crate) fn coverage(
    location: Location<'_>,
    working_directory: &Utf8Path,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let Ok((merged, compiled)) = prepare(location, working_directory, output) else {
        return Exit::ConfigProblem;
    };

    let tree = match archwarden_engine::walk::walk(&merged.root, &compiled) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = writeln!(output.err, "the repository could not be walked — {error}");
            return Exit::ConfigProblem;
        }
    };

    crate::coverage::render(
        &crate::coverage::examine(&compiled, &tree),
        format,
        output.out,
    );
    Exit::Clean
}

pub(crate) fn doctor(
    location: Location<'_>,
    working_directory: &Utf8Path,
    format: Format,
    strict: bool,
    output: &mut Output<'_>,
) -> Exit {
    let Ok((merged, compiled)) = prepare(location, working_directory, output) else {
        return Exit::ConfigProblem;
    };

    let mut concerns = crate::doctor::examine(&compiled);

    // The slow half. A tree that will not walk is a problem the user needs to
    // hear about, but it does not invalidate what the config alone already
    // said, so the answer so far is still printed.
    match archwarden_engine::walk::walk(&merged.root, &compiled) {
        Ok(tree) => {
            concerns.extend(crate::doctor::examine_repository(
                &merged.root,
                &compiled,
                &tree,
            ));
        }
        Err(error) => {
            let _ = writeln!(
                output.err,
                "note: the repository could not be walked, so only the \
                 configuration was examined — {error}"
            );
        }
    }

    crate::doctor::render(&concerns, format, output.out);

    // A command that never fails guards nothing. Printing the word `error` and
    // returning success is the incoherence issue #166 reported: the word is a
    // promise, and a pipeline that ran this and passed had been told about a
    // problem and could not act on it.
    //
    // `ConfigProblem` rather than `Errors`, and that distinction is the whole
    // reason the two codes exist: everything here is a statement about the
    // configuration, not about the code the configuration governs.
    let blocking = concerns
        .iter()
        .any(|concern| strict || concern.level == archwarden_core::level::Level::Error);

    if blocking {
        Exit::ConfigProblem
    } else {
        Exit::Clean
    }
}

/// Shows what one rule reaches, and what it is reporting.
pub(crate) fn explain(
    location: Location<'_>,
    working_directory: &Utf8Path,
    rule_id: &str,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let Ok((merged, compiled)) = prepare(location, working_directory, output) else {
        return Exit::ConfigProblem;
    };

    // Validated once, as a rule id, and that covers both namespaces: a
    // decision id takes the same character set, deliberately, so that one
    // argument accepting either needs one rule about what may be typed.
    if let Err(error) = archwarden_core::ids::RuleId::new(rule_id) {
        let _ = writeln!(
            output.err,
            "`{rule_id}` is not a rule or decision id: {error}"
        );
        return Exit::ConfigProblem;
    }

    let tree = match walked(&merged.root, working_directory, &compiled, output) {
        Ok(tree) => tree,
        Err(exit) => return exit,
    };

    match crate::explain::explain(&merged.root, &compiled, &tree, rule_id) {
        Ok(explanation) => {
            crate::explain::render(&explanation, format, output.out);
            Exit::Clean
        }
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            Exit::ConfigProblem
        }
    }
}

pub(crate) fn validate(
    location: Location<'_>,
    working_directory: &Utf8Path,
    output: &mut Output<'_>,
) -> Exit {
    match prepare(location, working_directory, output) {
        Ok((merged, compiled)) => {
            report_valid(&merged, compiled.rule_count(), output);
            Exit::Clean
        }
        Err(exit) => exit,
    }
}

/// Says what was loaded, and from where.
///
/// The rule count and the preset list are the cheapest way for a user to
/// notice that a preset did not load, or that `disable` removed more than
/// they meant.
pub(crate) fn report_valid(merged: &MergedConfig, rules: usize, output: &mut Output<'_>) {
    let _ = writeln!(
        output.out,
        "{} is valid ({} rule{})",
        merged.path,
        rules,
        if rules == 1 { "" } else { "s" }
    );

    if merged.sources.len() > 1 {
        let _ = writeln!(output.out, "  extends:");
        for source in merged.sources.iter().filter(|s| **s != merged.path) {
            let _ = writeln!(output.out, "    {source}");
        }
    }

    // Which languages are read, once a preset can add one. A preset that turns
    // a language on turns on *reading files*, and that is a cost the adopter
    // should be able to see rather than infer from a run getting slower.
    // Issue #158 asked for this in the same breath as the union.
    //
    // Printed only when a preset is involved: a repository whose own config
    // names its languages is being told what it just wrote.
    if merged.sources.len() > 1 {
        // Named by the enum's own crate, where the match is exhaustive: this
        // one cannot be, because `Language` is `#[non_exhaustive]` and a
        // wildcard here would print a list quietly missing a new language.
        let mut asked: Vec<&'static str> = Vec::new();
        for language in &merged.config.languages {
            let name = language.as_str();
            if !asked.contains(&name) {
                asked.push(name);
            }
        }
        // Sorted rather than in merge order, so reordering `extends` does not
        // reword this line.
        asked.sort_unstable();
        let _ = writeln!(output.out, "  reads: {}", asked.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standing_of(accepted: usize, gone: usize) -> String {
        let standing = archwarden_api::baseline::Standing {
            accepted,
            gone,
            by_decision: std::collections::BTreeMap::new(),
        };
        let mut out = Vec::new();
        report_standing(&standing, &mut out);
        String::from_utf8(out).expect("UTF-8")
    }

    /// A baseline with nothing stale says only what it accepted.
    ///
    /// The advice to re-run `archwarden baseline` is the whole value of the
    /// second clause, and printing it when there is nothing to update trains
    /// the reader to ignore it.
    #[test]
    fn a_baseline_that_is_current_does_not_ask_to_be_updated() {
        let text = standing_of(3, 0);

        assert!(text.contains("3 accepted"), "{text}");
        assert!(!text.contains("no longer"), "{text}");
        assert!(!text.contains("archwarden baseline"), "{text}");
    }

    /// One stale entry is singular, and more than one is not.
    #[test]
    fn entries_that_no_longer_occur_are_counted_and_agree_with_their_verb() {
        let one = standing_of(3, 1);
        assert!(one.contains("1 no longer occurs"), "{one}");
        assert!(one.contains("archwarden baseline"), "{one}");

        let several = standing_of(3, 2);
        assert!(several.contains("2 no longer occur"), "{several}");
    }
}
