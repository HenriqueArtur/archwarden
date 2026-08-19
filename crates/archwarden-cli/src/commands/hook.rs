//! `init`, and the hooks that judge a write.

use archwarden_config::discovery;
use camino::Utf8Path;

use crate::command::{Harness, Location, Output};
use crate::commands::query::starter;
use crate::exit::Exit;

/// Writes a starter configuration, if there is not one already.
pub(crate) fn init(working_directory: &Utf8Path, output: &mut Output<'_>) -> Exit {
    let path = working_directory.join(discovery::CONFIG_FILE_NAME);

    // Never overwrites. A config is hand-written and often long, and a command
    // that replaced one would be a command nobody runs twice on purpose.
    if path.exists() {
        let _ = writeln!(output.err, "`{path}` already exists; nothing was written");
        return Exit::ConfigProblem;
    }

    if let Err(error) = std::fs::write(&path, starter(&crate::schema::reference(working_directory)))
    {
        let _ = writeln!(output.err, "cannot write `{path}`: {error}");
        return Exit::ConfigProblem;
    }

    let ignored = ignore_the_cache(working_directory);

    let _ = writeln!(
        output.out,
        "wrote {path}{}\n\n\
         Next: add a rule, then\n\
         \x20 archwarden config validate      check it parses\n\
         \x20 archwarden describe <path>      see what applies to a file\n\
         \x20 archwarden install-hooks --claude-code   block invalid writes",
        if ignored {
            "\nadded `.archwarden/cache/` to .gitignore"
        } else {
            ""
        }
    );
    Exit::Clean
}

/// Adds `.archwarden/cache/` to `.gitignore`, if it is not covered already.
///
/// `check` writes a multi-megabyte binary database inside the repository, and
/// a tool that leaves its own build artefact for the user to discover in
/// `git status` is a tool being rude with their diff.
///
/// **`.archwarden/cache/`, never `.archwarden/`.** The baseline lives beside
/// the cache and is meant to be committed — it is a record of accepted debt,
/// reviewed in a pull request, and ignoring it would quietly undo the one
/// feature whose whole point is being visible in version control.
///
/// Returns whether a line was added. Never fails the command: an unwritable
/// `.gitignore` is the user's business, and `init` succeeding is about the
/// config.
pub(crate) fn ignore_the_cache(working_directory: &Utf8Path) -> bool {
    const ENTRY: &str = ".archwarden/cache/";

    let path = working_directory.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    if existing
        .lines()
        .any(|line| matches!(line.trim(), ENTRY | ".archwarden/cache" | ".archwarden/"))
    {
        return false;
    }

    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let addition = format!(
        "{separator}\n# archwarden's parse cache. The baseline beside it is meant to be committed.\n{ENTRY}\n"
    );

    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.as_std_path())
        .and_then(|mut file| std::io::Write::write_all(&mut file, addition.as_bytes()))
        .is_ok()
}

/// Answers a harness's question, whichever one it asked.
///
/// Always exits clean. A hook that blocked because *it* failed would be worse
/// than no hook, so every unexpected shape allows the write and says why;
/// blocking is a decision carried in the response, never a side effect of
/// something going wrong.
pub(crate) fn hook(
    harness: Harness,
    location: Location<'_>,
    working_directory: &Utf8Path,
    output: &mut Output<'_>,
) -> Exit {
    let Harness::ClaudeCode = harness;

    let mut payload = String::new();
    if std::io::Read::read_to_string(output.input, &mut payload).is_err() {
        return unable(output, "the hook event could not be read from stdin");
    }

    // One command, dispatching on what it was sent. Two commands would let a
    // hook be wired to the wrong event, and an answer to the wrong question is
    // a hook that reports nothing while looking installed.
    match crate::hook::event(&payload) {
        crate::hook::Event::PreToolUse => {}
        crate::hook::Event::Stop => {
            return stopped(location, working_directory, output);
        }
        crate::hook::Event::SessionStart => {
            return session_started(location, working_directory, output);
        }
        // Not guessed at. A harness that grows an event this build has never
        // seen gets silence rather than a pre-write answer to a question that
        // was not one.
        crate::hook::Event::Other => return allow(output),
    }

    let argument = match crate::hook::target(&payload) {
        crate::hook::Target::Path(path) => path,
        // The one silence that is correct: most tools write no file, and a
        // word about each would be a hook nobody keeps.
        crate::hook::Target::NoFile => return allow(output),
        crate::hook::Target::Unreadable => {
            return unable(
                output,
                "the hook event was not in a shape archwarden could read",
            );
        }
    };

    // The same operation `check` and `config validate` run, and that is the
    // whole of issue #63. This used to be four steps written out again here,
    // because the shared `prepare()` reported failure by writing a miette
    // report to stderr and returning exit 2, and a hook must answer in JSON
    // and exit clean. So the difference in how a failure is *said* forced the
    // path to be duplicated — and the copy was missing the version guard,
    // which shipped as issue #55: a config from a future version parsed into
    // one with no rules, compiled, matched nothing, and permitted every write.
    // The gate did not fail; it evaporated.
    //
    // Now the operation returns its failure and this decides how to say it. A
    // broken or absent configuration is the user's problem to fix at their own
    // pace, not a reason to stop them writing a file. It is a reason to say
    // so: a gate that permits in silence is indistinguishable from one that
    // examined the write and approved it.
    let archwarden_api::Prepared { merged, compiled } =
        match archwarden_api::prepare(location, working_directory) {
            Ok(prepared) => prepared,
            Err(error) => return unable(output, &crate::hook::unexamined(&error)),
        };

    // The harness's own root, from the payload it sent. When it differs from
    // ours the two are one repository through two mounts, and until 0.19 this
    // answered "outside the repository" about a file plainly inside it —
    // which is every write in a container-only project. Issue #93.
    let seen_as = crate::hook::seen_as(&payload);
    let path = match archwarden_api::describe::repo_relative(
        &merged.root,
        working_directory,
        seen_as.as_deref(),
        &argument,
    ) {
        Ok(path) => path,
        // `repo_relative` resolves a second route to the same directory, so
        // reaching here means the path really is somewhere else. Which is a
        // fine thing for a write to be — and the hook still has to say that it
        // formed no opinion, rather than nodding.
        Err(reason) => return unable(output, &reason),
    };

    // The write, not the file. A `PreToolUse` hook is asked whether something
    // that has not happened would be legal, and answering from disk answers
    // about the previous version — so a new file went unchecked, and an edit
    // that *fixed* a violation was refused for the violation it was fixing.
    // Issue #55.
    //
    // The disk is still read, because `Edit` sends a replacement rather than a
    // document and the result has to be reconstructed. A file that is not there
    // reads as empty, which is the case this most exists for.
    let on_disk = std::fs::read_to_string(merged.root.join(path.as_str())).unwrap_or_default();

    // Everything from here to the decision is [`archwarden_api::single::check`]
    // — the engine, the baseline, and the split between what this write breaks
    // and what it is fixing. It was written out here while the hook was the
    // only surface asking. MCP asks the same question, and a server that ran
    // the engine without the other two would refuse a write this permits.
    //
    // Reconstructing the text stays here: replaying an `Edit` is the harness's
    // protocol, not an operation. A tool this cannot replay yields `None`, and
    // judging the file as it stands is the honest answer to that.
    let checked = archwarden_api::single::check(
        &merged.root,
        &compiled,
        &path,
        crate::hook::pending(&payload, &on_disk).as_deref(),
    );
    let archwarden_api::single::Checked { single, fixing } = &checked;

    // Probed at the config root rather than the working directory: that is
    // where `node_modules` sits in a monorepo, and where the harness will be
    // when it runs what this message suggests.
    let invocation = crate::hooks::invocation(&merged.root);
    let reasons = crate::report::Reasons::of(&compiled);

    let decision = if checked.refuses() {
        crate::hook::Decision::Deny(crate::hook::explain(single, &reasons, &invocation))
    } else if single.findings.is_empty() && fixing.is_empty() {
        crate::hook::Decision::Allow
    } else if single.findings.is_empty() {
        // Only progress. "Would break these rules" is false about a write that
        // is fixing the directory, and it buries the useful half -- what is
        // still missing is what the agent has to write next.
        crate::hook::Decision::Note(crate::hook::still_needs(fixing))
    } else {
        // Decision 1: warnings are visible and do not gate.
        crate::hook::Decision::Note(crate::hook::explain(single, &reasons, &invocation))
    };

    let _ = write!(output.out, "{}", crate::hook::respond(&decision));
    Exit::Clean
}

/// Answers the end of a turn: what landed, now that it has all landed.
///
/// The pre-write hook sees one write at a time, and some rules are only
/// decidable once a group of writes exists. A `presence` rule requiring three
/// files makes every one of the three illegal until the other two are there,
/// so there is no order that passes and the module cannot be created at all.
/// Issue #57 is that; this is where the class is caught instead.
///
/// **Reports, never blocks.** The writes have already happened, so refusing
/// them is not on offer — and a `Stop` hook that kept the agent going would be
/// a loop waiting for a reason to start.
///
/// Scoped to what changed against `HEAD`, plus untracked files, which is the
/// work of the turn unless the agent committed midway. A full run would take
/// seconds on a large repository and say the same thing about files nobody
/// touched.
pub(crate) fn stopped(
    location: Location<'_>,
    working_directory: &Utf8Path,
    output: &mut Output<'_>,
) -> Exit {
    // The third surface, and the third shape a failure takes: silence.
    //
    // Unlike the pre-write hook, saying nothing here is honest. There, silence
    // is indistinguishable from approving a write; here nothing was gated, so
    // a message on every turn about a config the user has not written yet is
    // noise they would remove the hook to stop.
    //
    // That this is one `else` rather than four is the point of issue #63. It
    // was four, and every one of them was an opportunity to leave a guard out.
    let Ok(archwarden_api::Prepared { merged, compiled }) =
        archwarden_api::prepare(location, working_directory)
    else {
        return allow(output);
    };

    let Ok(changed) = crate::changed::changed_files(&merged.root, "HEAD") else {
        // No git, a fresh repository with no commits, a detached state. None of
        // those is the user's problem at the end of a turn.
        return allow(output);
    };
    if changed.is_empty() {
        return allow(output);
    }

    let baseline = crate::baseline::Baseline::load(&merged.root).ok().flatten();

    let mut findings = Vec::new();
    for path in &changed {
        let Ok(path) = archwarden_core::path::RepoRelPath::new(path) else {
            continue;
        };
        let single = archwarden_engine::single::check_file(&merged.root, &compiled, &path);
        findings.extend(
            single
                .findings
                .into_iter()
                .filter(|finding| baseline.as_ref().is_none_or(|b| !b.accepts(finding))),
        );
    }

    if findings.is_empty() {
        return allow(output);
    }

    let reasons = crate::report::Reasons::of(&compiled);
    let _ = write!(
        output.out,
        "{}",
        crate::hook::respond(&crate::hook::Decision::Note(crate::hook::landed(
            &findings, &reasons,
        )))
    );
    Exit::Clean
}

pub(crate) fn allow(output: &mut Output<'_>) -> Exit {
    let _ = write!(
        output.out,
        "{}",
        crate::hook::respond(&crate::hook::Decision::Allow)
    );
    Exit::Clean
}

/// Permits the write, and says that it was permitted unexamined.
///
/// The distinction this exists for: *"I have no objection"* and *"I could not
/// tell"* are different answers, and only the first is safe to ignore. Both
/// used to be `{}`.
///
/// A gate that cannot judge a write and permits it in silence is
/// indistinguishable from one that judged it and approved — which is the
/// property `verify-rules` exists to refuse for rules, one layer up and with
/// nothing checking it. On a machine where every write took this path, the only
/// symptom was CI failing later on files a pre-write gate was installed to
/// refuse.
///
/// Still permits. A hook that blocked because *it* could not do its job would
/// be worse than no hook.
pub(crate) fn unable(output: &mut Output<'_>, reason: &str) -> Exit {
    let _ = write!(
        output.out,
        "{}",
        crate::hook::respond(&crate::hook::Decision::Note(format!(
            "archwarden did not check this write: {reason}."
        )))
    );
    Exit::Clean
}

/// Puts the module map into a starting session's context.
///
/// Issue #66. Layer 3 of `AGENT-INTEGRATION.md` depended on the user
/// referencing `.archwarden/AGENT_RULES.md` from their `CLAUDE.md` by hand;
/// this puts a pointer there without being asked.
///
/// **A pointer, not the guide.** The full digest costs context in every
/// session, including the ones touching no governed file, and a long block is
/// the first thing compaction drops — which is the moment this exists for.
///
/// It fires on every source, `compact` included, because `install-hooks`
/// writes the entry with no matcher. Nothing here reads the source: whichever
/// way the session arrived, it arrived without the rules in it.
///
/// A configuration it cannot read is reported to the *user* and never injected.
/// Silence would be the third answer this project keeps refusing — a session
/// with no rules in context is indistinguishable from a repository with no
/// rules, which is the sentence `CONFIG.md` calls the worst failure a linter
/// has.
pub(crate) fn session_started(
    location: Location<'_>,
    working_directory: &Utf8Path,
    output: &mut Output<'_>,
) -> Exit {
    let prepared = match archwarden_api::prepare(location, working_directory) {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = write!(
                output.out,
                "{}",
                crate::hook::session(None, Some(&error.unreadable()))
            );
            return Exit::Clean;
        }
    };

    let map = archwarden_api::map::map(&prepared.merged.config, &prepared.compiled);

    // A repository whose config governs nothing gets nothing. Announcing a
    // gate that is not there is worse than saying nothing, and this is the one
    // case where silence is the honest answer rather than the ambiguous one.
    if map.is_empty() {
        let _ = write!(output.out, "{}", crate::hook::session(None, None));
        return Exit::Clean;
    }

    let invocation = crate::hooks::invocation(&prepared.merged.root);
    let _ = write!(
        output.out,
        "{}",
        crate::hook::session(Some(&archwarden_api::map::render(&map, &invocation)), None)
    );
    Exit::Clean
}
