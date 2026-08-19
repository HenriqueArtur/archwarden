//! The agent-facing surface: MCP, scaffolding, guides, moves.

use camino::Utf8Path;

use crate::command::{Location, Output};
use crate::commands::{check::walked, query::prepare};
use crate::{exit::Exit, report::Format};

/// Serves MCP until the client closes the pipe.
///
/// Everything this does is in `archwarden-mcp`, which cannot see this crate.
/// What is left here is the wiring a binary owns: buffering stdin, and turning
/// a client that went away into an exit code rather than a panic.
///
/// It exits clean when the pipe closes, because that is how a stdio server is
/// stopped — the client kills it at the end of the session, and reporting that
/// as a failure would put an error in the user's log every time they quit.
pub(crate) fn mcp(working_directory: &Utf8Path, output: &mut Output<'_>) -> Exit {
    let mut input = std::io::BufReader::new(&mut *output.input);

    match archwarden_mcp::serve(&mut input, output.out, working_directory) {
        Ok(()) => Exit::Clean,
        // A broken pipe is the client going away mid-write, which is the same
        // ending by another route.
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Exit::Clean,
        Err(error) => {
            let _ = writeln!(output.err, "archwarden mcp: {error}");
            Exit::ConfigProblem
        }
    }
}

/// Wires archwarden into a harness, or takes it back out.
pub(crate) fn install_hooks(
    claude_code: bool,
    remove: bool,
    working_directory: &Utf8Path,
    output: &mut Output<'_>,
) -> Exit {
    if !claude_code {
        let _ = writeln!(
            output.err,
            "say which harness: `--claude-code` is the only one so far"
        );
        return Exit::ConfigProblem;
    }

    let settings = working_directory.join(crate::hooks::CLAUDE_SETTINGS);
    let command = crate::hooks::hook_command(working_directory);

    let hooks = {
        let current = std::fs::read_to_string(&settings).ok();
        let edited = if remove {
            crate::hooks::remove(current.as_deref())
        } else {
            crate::hooks::install(current.as_deref(), &command)
        };
        match apply(&settings, edited, output) {
            Ok(outcome) => outcome,
            Err(exit) => return exit,
        }
    };

    // The second file, decided on its own. Sharing a flag would let a
    // half-installed project report "already installed" and never gain the
    // server -- the same defect `install` avoids by deciding each event
    // separately.
    let mcp_config = working_directory.join(crate::hooks::MCP_CONFIG);
    let invocation = crate::hooks::invocation(working_directory);
    let mcp = {
        let current = std::fs::read_to_string(&mcp_config).ok();
        let edited = if remove {
            crate::hooks::remove_mcp(current.as_deref())
        } else {
            crate::hooks::install_mcp(current.as_deref(), &invocation)
        };
        match apply(&mcp_config, edited, output) {
            Ok(outcome) => outcome,
            Err(exit) => return exit,
        }
    };

    let _ = writeln!(output.out, "{}", describe_outcome(hooks, &settings));
    // Naming the command is the point: a hook that resolves to nothing fails
    // silently, at someone else's next write rather than here. Only on the
    // way in — after a removal there is no command to name.
    if hooks == crate::hooks::Outcome::Installed {
        let _ = writeln!(output.out, "  {command}");
    }

    let _ = writeln!(output.out, "{}", describe_mcp_outcome(mcp, &mcp_config));
    if mcp == crate::hooks::Outcome::Installed {
        let _ = writeln!(output.out, "  {invocation} mcp");
    }

    // Hooks are read when a session starts, so a project that just gained one
    // has not gained it for the session that ran this. Said out loud because
    // the alternative is a user testing it, seeing nothing, and concluding the
    // installer lied.
    if !remove
        && (hooks == crate::hooks::Outcome::Installed || mcp == crate::hooks::Outcome::Installed)
    {
        let _ = writeln!(
            output.out,
            "\nBoth take effect in the next session: hooks and MCP servers are read at startup."
        );
        caveat(
            &invocation,
            crate::hooks::in_container(
                std::path::Path::new(crate::hooks::CONTAINER_MARKER),
                std::path::Path::new(crate::hooks::CONTAINER_CGROUP),
            ),
            output,
        );
    }

    Exit::Clean
}

/// Says where the installed command has to be runnable from, and when that is
/// unlikely to be here.
///
/// Issue #93. The command written is the one that works **where this ran**, and
/// the harness runs it somewhere else — which is the same machine until it is
/// not. A project whose dependencies live only inside a container installs
/// `./node_modules/.bin/archwarden` and hands it to a harness on the host,
/// where that path does not exist. The hook then fails on every write, and the
/// only symptom is a message that says archwarden did not check it.
///
/// Nothing here can fix that: the installer cannot know what the harness's
/// machine can run. It can stop being silent, which is the half the report
/// asked for — *"nothing in the output hints that the command may not be
/// executable from where the harness will call it"*.
pub(crate) fn caveat(invocation: &str, in_container: bool, output: &mut Output<'_>) {
    let _ = writeln!(
        output.out,
        "\nThe harness must be able to run that command itself, from the repository \
         root. It runs hooks and MCP servers as its own process, not through npm."
    );

    // The one case that can be recognised, said sharply rather than left in
    // the general sentence above. A relative path is the only invocation whose
    // meaning depends on which filesystem is reading it.
    if in_container && invocation.starts_with("./") {
        let _ = writeln!(
            output.out,
            "\nThis looks like a container, and `{invocation}` names a path inside it. \
             If your harness runs on the host, it cannot reach that — point it at a \
             wrapper that runs archwarden where the dependencies are."
        );
    }
}

/// Writes one edited file, or reports why it could not.
///
/// Nothing changed means nothing written: rewriting a file to the same bytes
/// still shows up as a modification in an editor and in `git status`.
pub(crate) fn apply(
    path: &Utf8Path,
    edited: Result<(String, crate::hooks::Outcome), String>,
    output: &mut Output<'_>,
) -> Result<crate::hooks::Outcome, Exit> {
    let (contents, outcome) = match edited {
        Ok(edited) => edited,
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Err(Exit::ConfigProblem);
        }
    };

    if matches!(
        outcome,
        crate::hooks::Outcome::AlreadyInstalled | crate::hooks::Outcome::NotInstalled
    ) {
        return Ok(outcome);
    }

    if let Some(parent) = path.parent()
        && !parent.as_str().is_empty()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        let _ = writeln!(output.err, "cannot create `{parent}`: {error}");
        return Err(Exit::ConfigProblem);
    }
    if let Err(error) = std::fs::write(path, contents) {
        let _ = writeln!(output.err, "cannot write `{path}`: {error}");
        return Err(Exit::ConfigProblem);
    }

    Ok(outcome)
}

pub(crate) fn describe_mcp_outcome(outcome: crate::hooks::Outcome, config: &Utf8Path) -> String {
    match outcome {
        crate::hooks::Outcome::Installed => format!("installed the MCP server in {config}"),
        crate::hooks::Outcome::AlreadyInstalled => {
            format!("the MCP server is already in {config}")
        }
        crate::hooks::Outcome::Removed => format!("removed the MCP server from {config}"),
        crate::hooks::Outcome::NotInstalled => format!("no archwarden server was in {config}"),
    }
}

pub(crate) fn describe_outcome(outcome: crate::hooks::Outcome, settings: &Utf8Path) -> String {
    match outcome {
        crate::hooks::Outcome::Installed => {
            format!("installed the pre-write hook in {settings}")
        }
        crate::hooks::Outcome::AlreadyInstalled => {
            format!("the pre-write hook is already in {settings}")
        }
        crate::hooks::Outcome::Removed => {
            format!("removed the pre-write hook from {settings}")
        }
        crate::hooks::Outcome::NotInstalled => {
            format!("no archwarden hook was in {settings}")
        }
    }
}

/// Checks one file, for a pre-write hook.
///
/// Exits with findings the same way a full run does, so a harness can block on
/// the exit code without parsing anything.
pub(crate) fn check_one(
    location: Location<'_>,
    working_directory: &Utf8Path,
    argument: &str,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let (merged, compiled) = match prepare(location, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    let path = match archwarden_api::describe::repo_relative(
        &merged.root,
        working_directory,
        None,
        argument,
    ) {
        Ok(path) => path,
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    };

    let mut single = archwarden_engine::single::check_file(&merged.root, &compiled, &path);
    // A pre-write hook that blocked an agent on debt the project already
    // accepted would be uninstalled by lunchtime.
    match crate::baseline::Baseline::load(&merged.root) {
        Ok(Some(baseline)) => single.findings.retain(|finding| !baseline.accepts(finding)),
        Ok(None) => {}
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    }
    crate::report::render_single(
        &single,
        &crate::report::Reasons::of(&compiled),
        format,
        output.out,
    );

    if single.fails_build() {
        Exit::Errors
    } else {
        Exit::Clean
    }
}

/// Answers about every path a glob matches.
///
/// Only paths that exist, necessarily: a glob can match nothing else. That is
/// the one thing this cannot do that single-path `describe` can, and it is
/// worth stating because answering about a file nobody has created is most of
/// what `describe` is for.
pub(crate) fn describe_many(
    root: &Utf8Path,
    working_directory: &Utf8Path,
    compiled: &archwarden_core::compiled::CompiledConfig,
    glob: &str,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let set = match archwarden_core::glob::PathSet::compile([glob.to_owned()]) {
        Ok(set) => set,
        Err(error) => {
            let _ = writeln!(output.err, "{error}");
            return Exit::ConfigProblem;
        }
    };

    let tree = match walked(root, working_directory, compiled, output) {
        Ok(tree) => tree,
        Err(exit) => return exit,
    };

    // Directories and files both, because a rule can be about either and the
    // user does not have to know which before asking.
    let mut matched: Vec<archwarden_core::path::RepoRelPath> = tree
        .directories()
        .map(|(path, _)| path.clone())
        .chain(tree.files().map(|file| file.path.clone()))
        .filter(|path| set.is_match(path.as_path()))
        .collect();
    matched.sort();
    matched.dedup();

    let answers: Vec<_> = matched
        .into_iter()
        .map(|path| {
            let applies = archwarden_api::describe::describe(compiled, &path);
            (path, applies)
        })
        .collect();

    crate::describe::render_many(glob, &answers, format, output.out);
    Exit::Clean
}

/// Shows the smallest shape that would satisfy the rules at one path.
///
/// Shares `describe`'s path resolution and config loading, and is built on its
/// answer, so the two commands cannot disagree about what applies.
pub(crate) fn scaffold(
    location: Location<'_>,
    working_directory: &Utf8Path,
    argument: &str,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let (merged, compiled) = match prepare(location, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    let path = match archwarden_api::describe::repo_relative(
        &merged.root,
        working_directory,
        None,
        argument,
    ) {
        Ok(path) => path,
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    };

    let shape = archwarden_api::scaffold::scaffold(&compiled, &path);
    crate::scaffold::render(&path, &shape, format, output.out);
    Exit::Clean
}

/// Emits the rule set as a digest for an agent's context.
///
/// Writes to stdout rather than to a file: `AGENT-INTEGRATION.md` shows it
/// redirected into `.archwarden/AGENT_RULES.md`, and a command that chose the
/// destination itself would be a command that writes where the user did not
/// ask.
pub(crate) fn agent_guide(
    location: Location<'_>,
    working_directory: &Utf8Path,
    format: crate::guide::GuideFormat,
    language: Option<crate::phrases::Language>,
    scope: Option<&str>,
    kinds: &[String],
    output: &mut Output<'_>,
) -> Exit {
    if let Err(message) = archwarden_api::guide::guide_kinds(kinds) {
        let _ = writeln!(output.err, "{message}");
        return Exit::ConfigProblem;
    }

    let (merged, compiled) = match prepare(location, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    let scope = match scope
        .map(|scope| {
            archwarden_api::describe::repo_relative(&merged.root, working_directory, None, scope)
        })
        .transpose()
    {
        Ok(scope) => scope,
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    };

    // The digest describes the architecture as declared, and the one thing it
    // could never say is how much of it this repository still excuses. The
    // baseline is a committed file: reading it costs no walk, and a broken one
    // costs the count rather than the answer. Issue #112.
    let baseline = archwarden_api::baseline::Baseline::load(&merged.root)
        .ok()
        .flatten();
    let guide = archwarden_api::guide::guide(&compiled, scope.as_ref(), kinds, baseline.as_ref());
    // The flag wins over the config; the config over English. A repository
    // decides this once, and one run may want the other.
    let language = language.unwrap_or_else(|| crate::phrases::Language::of(merged.config.language));
    crate::guide::render(&guide, format, language, output.out);
    Exit::Clean
}
