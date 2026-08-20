//! The commands that write a file the user owns.

use camino::Utf8Path;

use crate::command::{Location, Output};
use crate::commands::check::walked;
use crate::commands::query::prepare;
use crate::exit::Exit;

/// Accepts every finding this repository has right now.
///
/// Runs a full check and writes what it found. Deliberately not incremental:
/// a baseline that accepted only part of a run would be a promise the file
/// does not keep.
pub(crate) fn write_baseline(
    location: Location<'_>,
    working_directory: &Utf8Path,
    dry_run: bool,
    output: &mut Output<'_>,
) -> Exit {
    let (merged, compiled) = match prepare(location, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    let tree = match walked(&merged.root, working_directory, &compiled, output) {
        Ok(tree) => tree,
        Err(exit) => return exit,
    };

    let evaluated = archwarden_api::evaluate(&archwarden_api::Evaluation {
        root: &merged.root,
        compiled: &compiled,
        tree: &tree,
        cache: archwarden_api::CachePolicy::Use,
        as_of: archwarden_core::date::Date::today(),
    });

    // Said out loud here too. This used to discard the flush failure with
    // `let _ = cache.flush()` while `check` reported it — two copies of one
    // orchestration disagreeing about whether a user hears that their next run
    // will be slow. Now the operation returns the note and both surfaces have
    // to decide, which is the decision this makes the same way.
    for note in &evaluated.notes {
        let _ = writeln!(output.err, "note: {note}");
    }
    let outcome = evaluated.report;

    let baseline = crate::baseline::Baseline::of(&outcome.findings);
    let path = merged.root.join(crate::baseline::BASELINE_PATH);

    if dry_run {
        return report_baseline_changes(&merged.root, &path, &baseline, &compiled, output);
    }

    if let Err(message) = baseline.write(&merged.root) {
        let _ = writeln!(output.err, "{message}");
        return Exit::ConfigProblem;
    }

    if baseline.is_empty() {
        // Still written, so `check` has something to read and the next person
        // does not wonder whether the command ran.
        let _ = writeln!(
            output.out,
            "wrote {path}, accepting nothing: this repository has no findings"
        );
    } else {
        let _ = writeln!(
            output.out,
            "wrote {path}, accepting {} {}",
            baseline.len(),
            if baseline.len() == 1 {
                "finding"
            } else {
                "findings"
            }
        );
        let _ = writeln!(
            output.out,
            "\nCommit it. Each line is debt this project has decided to carry,\n\
             and `check` will now fail only on findings that are not in it."
        );
    }

    Exit::Clean
}

/// Says what regenerating the baseline would change, and writes nothing.
///
/// The count the command printed before -- "accepting 106 findings" -- cannot
/// answer the question a reviewer has to ask: was debt paid, or was debt
/// added? Issue #23, whose author wrote a Python script twice in one session
/// to answer it, once to prove debt paid and once to prove a pure rename.
///
/// Exits clean whatever it finds. `check` is the gate, and it already fails on
/// a finding no baseline accepts; this answers "what would regenerating do",
/// which is a review question rather than a build one.
/// Writes one document per declared decision, or says what would change.
///
/// Exits clean whatever it finds, including under `--dry-run`. A document that
/// needs regenerating is not a violation of anything — it is a file out of
/// step with the config, which `config doctor` reports as advice. A team
/// adopting this incrementally must not get a red build for it. Issue #116.
pub(crate) fn write_decisions(
    location: Location<'_>,
    working_directory: &Utf8Path,
    dry_run: bool,
    output: &mut Output<'_>,
) -> Exit {
    let Ok((merged, compiled)) = prepare(location, working_directory, output) else {
        return Exit::ConfigProblem;
    };

    if compiled.decisions().count() == 0 {
        let _ = writeln!(
            output.out,
            "this configuration declares no decisions, so there is nothing to write."
        );
        return Exit::Clean;
    }

    let changes = crate::decisions::changes(&merged.root, &compiled);
    if changes.is_empty() {
        let _ = writeln!(
            output.out,
            "{} is up to date: {} {} unchanged. Nothing was written.",
            crate::decisions::DECISIONS_DIR,
            changes.unchanged.len(),
            plural(changes.unchanged.len(), "document is", "documents are"),
        );
        return Exit::Clean;
    }

    for path in &changes.created {
        let _ = writeln!(output.out, "  + {path}");
    }
    for path in &changes.updated {
        let _ = writeln!(output.out, "  ~ {path}");
    }

    if dry_run {
        let _ = writeln!(
            output.out,
            "\n{} {} would be written, {} updated. Nothing was written.",
            changes.created.len(),
            plural(changes.created.len(), "document", "documents"),
            changes.updated.len(),
        );
        return Exit::Clean;
    }

    for document in crate::decisions::documents(&merged.root, &compiled) {
        let path = merged.root.join(&document.path);
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            let _ = writeln!(output.err, "cannot create {parent}: {error}");
            return Exit::ConfigProblem;
        }
        if let Err(error) = std::fs::write(&path, &document.body) {
            let _ = writeln!(output.err, "cannot write {path}: {error}");
            return Exit::ConfigProblem;
        }
    }

    let _ = writeln!(
        output.out,
        "\nwrote {} {}, updated {}. The region between the `archwarden:yours` \
         markers was kept.",
        changes.created.len(),
        plural(changes.created.len(), "document", "documents"),
        changes.updated.len(),
    );
    Exit::Clean
}

/// The decision a rule implements, when it names one.
///
/// `None` for every rule written before 0.21 and for every rule whose author
/// has not said — which `config doctor` reports as `rule-without-a-decision`
/// and this stays silent about, because a line saying "against nothing" would
/// be noise on exactly the configurations that have the least to gain here.
pub(crate) fn decision_behind<'a>(
    config: &'a archwarden_core::compiled::CompiledConfig,
    rule_id: &str,
) -> Option<&'a archwarden_core::compiled::CompiledDecision> {
    let named = config
        .rules()
        .find(|rule| rule.id.as_str() == rule_id)?
        .decision
        .as_ref()?;

    config.decisions().find(|decision| &decision.id == named)
}

pub(crate) fn report_baseline_changes(
    root: &Utf8Path,
    path: &Utf8Path,
    next: &crate::baseline::Baseline,
    config: &archwarden_core::compiled::CompiledConfig,
    output: &mut Output<'_>,
) -> Exit {
    let committed = match crate::baseline::Baseline::load(root) {
        Ok(Some(committed)) => committed,
        // No baseline yet: everything this run found is what would be
        // accepted, which is the decision `archwarden baseline` is for and
        // exactly what someone adopting it should read first.
        Ok(None) => {
            let _ = writeln!(
                output.out,
                "no baseline yet. `archwarden baseline` would write {path}, accepting {} {}:\n",
                next.len(),
                plural(next.len(), "finding", "findings"),
            );
            for entry in next.entries() {
                let _ = writeln!(
                    output.out,
                    "  + {} {} — {}",
                    entry.rule, entry.path, entry.note
                );
            }
            return Exit::Clean;
        }
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    };

    let changes = committed.changes(next);
    if changes.is_empty() {
        let _ = writeln!(
            output.out,
            "{path} is up to date, accepting {} {}. Nothing was written.",
            committed.len(),
            plural(committed.len(), "finding", "findings"),
        );
        return Exit::Clean;
    }

    // Paid first. It is the only cheerful number archwarden prints, and a
    // reviewer who reads nothing else should read the additions last, where
    // they are still on screen.
    for entry in &changes.removed {
        let _ = writeln!(
            output.out,
            "  - {} {} — no longer occurs",
            entry.rule, entry.path
        );
    }
    for moved in &changes.moved {
        let _ = writeln!(
            output.out,
            "  ~ {} {} → {}",
            moved.from.rule, moved.from.path, moved.to.path
        );
    }
    for entry in &changes.added {
        let _ = writeln!(
            output.out,
            "  + {} {} — {}",
            entry.rule, entry.path, entry.note
        );
        // Under the addition and only the addition. A reviewer reading `+
        // entity-shape` has to know by heart which decision that rule serves,
        // and a reviewer who has to know it by heart is one who approves it.
        // The removals are the cheerful half and already read well; a second
        // line under each of them would double the good news to say nothing
        // new. Issue #113.
        if let Some(decision) = decision_behind(config, &entry.rule) {
            let _ = writeln!(
                output.out,
                "      against {} — {}",
                decision.id, decision.title
            );
        }
    }

    let _ = writeln!(
        output.out,
        "\n{path} would change: {} added, {} no longer occur, {} moved. Nothing was written.",
        changes.added.len(),
        changes.removed.len(),
        changes.moved.len(),
    );

    // The sentence the command exists for. An addition is a decision; the
    // other two are bookkeeping catching up with work already done.
    if changes.added.is_empty() {
        let _ = writeln!(
            output.out,
            "Nothing new would be accepted. Run `archwarden baseline` to apply."
        );
    } else {
        let _ = writeln!(
            output.out,
            "The {} marked `+` would become debt this project has decided to carry.\n\
             Fix them, or run `archwarden baseline` to accept them on purpose.",
            plural(changes.added.len(), "finding", "findings"),
        );
    }

    Exit::Clean
}

pub(crate) fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One is singular and everything else is not, zero included.
    ///
    /// Zero is asserted because it is the case English gets wrong and a
    /// condition written `!= 1` would too -- "0 findings" is plural, and a
    /// message reading "0 finding" is the kind of thing a reader notices and
    /// then stops trusting.
    #[test]
    fn only_one_is_singular() {
        assert_eq!(plural(1, "finding", "findings"), "finding");
        assert_eq!(plural(0, "finding", "findings"), "findings");
        assert_eq!(plural(2, "finding", "findings"), "findings");
    }
}
