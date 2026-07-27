//! `archwarden`'s command line, as a library.
//!
//! The binary is a four-line shim over [`run`]. Everything that decides
//! anything lives here so it can be tested without spawning a process.

pub mod describe;
pub mod diagnostic;
pub mod doctor;
pub mod exit;
pub mod explain;
pub mod guide;
pub mod hook;
pub mod hooks;
pub mod locate;
pub mod report;
pub mod scaffold;
pub mod schema;

use archwarden_cache::store::Cache;
use archwarden_config::{
    compile,
    discovery::{self, LoadedConfig},
    extends::{self, MergedConfig},
};
use archwarden_resolver::preset::PresetResolver;
use camino::{Utf8Path, Utf8PathBuf};
use clap::{Parser, Subcommand};

use crate::{diagnostic::ConfigDiagnostic, exit::Exit, report::Format};

/// A fast, declarative architecture linter for TypeScript and JavaScript.
#[derive(Debug, Parser)]
#[command(name = "archwarden", version, about, long_about = None)]
pub struct Cli {
    /// Path to `arch.config.json`. Overrides the upward search.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<Utf8PathBuf>,

    /// The command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check the repository against its rules.
    Check {
        /// Check one file instead of the repository.
        ///
        /// For a pre-write hook: reads the file and the directories on the way
        /// to it, rather than walking the repository. Rules it could not
        /// evaluate are reported, never dropped.
        #[arg(long, value_name = "PATH")]
        file: Option<String>,

        /// How to render the report.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,

        /// Parse every file from source, reading and writing nothing.
        ///
        /// The escape hatch for a suspected cache bug: if a run disagrees with
        /// `--no-cache`, the cache is wrong and that is worth a report.
        #[arg(long)]
        no_cache: bool,
    },

    /// Say what the rules require of a path, which need not exist yet.
    ///
    /// The informant half of decision 9: an agent asks before it writes,
    /// rather than being told after.
    Describe {
        /// The file or directory to ask about, relative to the working
        /// directory or absolute.
        #[arg(value_name = "PATH")]
        path: String,

        /// How to render the answer.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },

    /// Show the smallest shape that would satisfy the rules at a path.
    ///
    /// `describe` answers rule by rule; this transposes the same answer into
    /// one list of exports, one of siblings, one of import constraints.
    Scaffold {
        /// The file or directory to shape, relative to the working directory
        /// or absolute.
        #[arg(value_name = "PATH")]
        path: String,

        /// How to render the shape.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },

    /// Emit the rule set as a digest for an agent's context.
    ///
    /// Layer 3 of `AGENT-INTEGRATION.md`. Deterministic: the same
    /// configuration produces the same bytes, so the output can be committed
    /// or regenerated without either choice creating noise.
    AgentGuide {
        /// How to render the digest.
        #[arg(long, value_enum, default_value_t = crate::guide::GuideFormat::Markdown)]
        format: crate::guide::GuideFormat,

        /// Restrict the digest to rules that can fire under this directory.
        #[arg(long, value_name = "PATH")]
        scope: Option<String>,
    },

    /// Answer a harness's pre-write question, reading the event from stdin.
    ///
    /// Installed by `install-hooks`; not usually run by hand.
    Hook {
        /// Which harness's protocol to speak.
        #[arg(value_enum)]
        harness: Harness,
    },

    /// Wire archwarden into a harness as a pre-write hook.
    InstallHooks {
        /// Install for Claude Code, in `.claude/settings.json`.
        #[arg(long)]
        claude_code: bool,

        /// Take the hook back out instead of putting it in.
        #[arg(long)]
        remove: bool,
    },

    /// Write a starter configuration.
    Init,

    /// Inspect the configuration itself.
    Config {
        /// Which config command to run.
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

/// `archwarden config ...`
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Check that the config parses and matches the schema. Fast; no files are
    /// walked. For the semantic checks, use `config doctor`.
    Validate,

    /// Look for a configuration that parses and is still wrong.
    ///
    /// A rule that loads and then never fires is indistinguishable from a rule
    /// that passes, which is what this exists to catch.
    Doctor {
        /// How to render the diagnosis.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },

    /// Show what one rule reaches, and what it is reporting.
    Explain {
        /// The rule's id, as written in the config.
        #[arg(value_name = "RULE-ID")]
        rule_id: String,

        /// How to render the explanation.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
}

/// A harness archwarden can speak the hook protocol of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Harness {
    /// Claude Code's `PreToolUse` protocol.
    ClaudeCode,
}

/// Where a command writes its output.
///
/// Passing these in rather than printing directly is what lets a test assert
/// on what a command said, instead of only on its exit code.
pub struct Output<'a> {
    /// Normal output.
    pub out: &'a mut dyn std::io::Write,
    /// Diagnostics.
    pub err: &'a mut dyn std::io::Write,
    /// Where a command reads from, for the one that is handed a payload.
    pub input: &'a mut dyn std::io::Read,
}

/// Runs a parsed command line.
///
/// Never returns an error: every failure is rendered to `output.err` and
/// reported as an [`Exit`], because a linter's exit code is its primary
/// interface and a stray `Err` bubbling to `main` would bypass it.
pub fn run(cli: &Cli, working_directory: &Utf8Path, output: &mut Output<'_>) -> Exit {
    match &cli.command {
        Command::Check {
            file: Some(file),
            format,
            ..
        } => check_one(
            cli.config.as_deref(),
            working_directory,
            file,
            *format,
            output,
        ),
        Command::Check {
            format, no_cache, ..
        } => check(
            cli.config.as_deref(),
            working_directory,
            *format,
            *no_cache,
            output,
        ),
        Command::Describe { path, format } => describe(
            cli.config.as_deref(),
            working_directory,
            path,
            *format,
            output,
        ),
        Command::Scaffold { path, format } => scaffold(
            cli.config.as_deref(),
            working_directory,
            path,
            *format,
            output,
        ),
        Command::AgentGuide { format, scope } => agent_guide(
            cli.config.as_deref(),
            working_directory,
            *format,
            scope.as_deref(),
            output,
        ),
        Command::Init => init(working_directory, output),
        Command::Hook { harness } => {
            hook(*harness, cli.config.as_deref(), working_directory, output)
        }
        Command::InstallHooks {
            claude_code,
            remove,
        } => install_hooks(*claude_code, *remove, working_directory, output),
        Command::Config { command } => match command {
            ConfigCommand::Validate => validate(cli.config.as_deref(), working_directory, output),
            ConfigCommand::Doctor { format } => {
                doctor(cli.config.as_deref(), working_directory, *format, output)
            }
            ConfigCommand::Explain { rule_id, format } => explain(
                cli.config.as_deref(),
                working_directory,
                rule_id,
                *format,
                output,
            ),
        },
    }
}

/// Loads, merges and compiles a configuration, rendering any failure.
///
/// Shared by `check` and `config validate` so the two can never disagree about
/// whether a configuration is usable.
fn prepare(
    explicit: Option<&Utf8Path>,
    working_directory: &Utf8Path,
    output: &mut Output<'_>,
) -> Result<(MergedConfig, archwarden_core::compiled::CompiledConfig), Exit> {
    let loaded = load(explicit, working_directory).map_err(|error| {
        let report = miette::Report::new(ConfigDiagnostic::from_load_error(&error));
        let _ = writeln!(output.err, "{report:?}");
        Exit::ConfigProblem
    })?;

    // Checked before merging: an unsupported version means this build cannot
    // be trusted to interpret the file at all, presets included.
    if !loaded.config.version_is_supported() {
        let _ = writeln!(
            output.err,
            "{}: config declares version {}, but this build understands version {}",
            loaded.path,
            loaded.config.version,
            archwarden_config::config::SCHEMA_VERSION,
        );
        return Err(Exit::ConfigProblem);
    }

    let merged = extends::merge(loaded, &PresetResolver::new()).map_err(|error| {
        let report = miette::Report::new(ConfigDiagnostic::from_extends_error(&error));
        let _ = writeln!(output.err, "{report:?}");
        Exit::ConfigProblem
    })?;

    // Compiling is what makes validation mean something beyond "the JSON
    // parsed": every glob is built, every regex is compiled, and every export
    // template is checked against the capture groups its pattern defines.
    let compiled = compile::compile(&merged).map_err(|error| {
        let report = miette::Report::new(ConfigDiagnostic::from_compile_error(&error));
        let _ = writeln!(output.err, "{report:?}");
        Exit::ConfigProblem
    })?;

    Ok((merged, compiled))
}

/// Says what the rules require of one path.
///
/// Reads no file and parses nothing: every rule's `describe_expectation` is
/// purely lexical, which is what lets this answer about a path that does not
/// exist yet. Exit is clean even when nothing applies -- a query that found no
/// rules is not a failure, and an agent branching on the exit code should see
/// "your setup is wrong" only when it is.
fn describe(
    explicit: Option<&Utf8Path>,
    working_directory: &Utf8Path,
    argument: &str,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let (merged, compiled) = match prepare(explicit, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    let path = match crate::describe::repo_relative(&merged.root, working_directory, argument) {
        Ok(path) => path,
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    };

    let applies = crate::describe::describe(&compiled, &path);
    crate::describe::render(&path, &applies, format, output.out);
    Exit::Clean
}

/// The starter configuration `init` writes.
///
/// No rules. A generated rule is a rule nobody chose, and a linter that starts
/// by reporting things the user never asked for is a linter they turn off. The
/// `$schema` line is the part that earns its place: an editor picks it up and
/// gives completion and, since M7d.1, an error on a misspelled key -- which is
/// why what it points at is decided by [`crate::schema::reference`] rather
/// than being a constant, and why it must be a reference that answers.
fn starter(reference: &str) -> String {
    format!(
        r#"{{
  "$schema": "{reference}",
  "version": 0,
  "rules": []
}}
"#
    )
}

/// Writes a starter configuration, if there is not one already.
fn init(working_directory: &Utf8Path, output: &mut Output<'_>) -> Exit {
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

    let _ = writeln!(
        output.out,
        "wrote {path}\n\n\
         Next: add a rule, then\n\
         \x20 archwarden config validate      check it parses\n\
         \x20 archwarden describe <path>      see what applies to a file\n\
         \x20 archwarden install-hooks --claude-code   block invalid writes"
    );
    Exit::Clean
}

/// Answers a harness's pre-write question.
///
/// Always exits clean. A hook that blocked because *it* failed would be worse
/// than no hook, so every unexpected shape allows the write and says why;
/// blocking is a decision carried in the response, never a side effect of
/// something going wrong.
fn hook(
    harness: Harness,
    explicit: Option<&Utf8Path>,
    working_directory: &Utf8Path,
    output: &mut Output<'_>,
) -> Exit {
    let Harness::ClaudeCode = harness;

    let mut payload = String::new();
    if std::io::Read::read_to_string(output.input, &mut payload).is_err() {
        return allow(output);
    }
    let Some(argument) = crate::hook::target(&payload) else {
        return allow(output);
    };

    // A broken or absent configuration is the user's problem to fix at their
    // own pace, not a reason to stop them writing a file.
    let Ok(loaded) = load(explicit, working_directory) else {
        return allow(output);
    };
    let Ok(merged) = extends::merge(loaded, &PresetResolver::new()) else {
        return allow(output);
    };
    let Ok(compiled) = compile::compile(&merged) else {
        return allow(output);
    };
    let Ok(path) = crate::describe::repo_relative(&merged.root, working_directory, &argument)
    else {
        return allow(output);
    };

    let single = archwarden_engine::single::check_file(&merged.root, &compiled, &path);
    // Probed at the config root rather than the working directory: that is
    // where `node_modules` sits in a monorepo, and where the harness will be
    // when it runs what this message suggests.
    let invocation = crate::hooks::invocation(&merged.root);
    let decision = if single.fails_build() {
        crate::hook::Decision::Deny(crate::hook::explain(&single, &invocation))
    } else if single.findings.is_empty() {
        crate::hook::Decision::Allow
    } else {
        // Decision 1: warnings are visible and do not gate.
        crate::hook::Decision::Note(crate::hook::explain(&single, &invocation))
    };

    let _ = write!(output.out, "{}", crate::hook::respond(&decision));
    Exit::Clean
}

fn allow(output: &mut Output<'_>) -> Exit {
    let _ = write!(
        output.out,
        "{}",
        crate::hook::respond(&crate::hook::Decision::Allow)
    );
    Exit::Clean
}

/// Wires archwarden into a harness, or takes it back out.
fn install_hooks(
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
    let current = std::fs::read_to_string(&settings).ok();

    let command = crate::hooks::hook_command(working_directory);
    let edited = if remove {
        crate::hooks::remove(current.as_deref())
    } else {
        crate::hooks::install(current.as_deref(), &command)
    };

    let (contents, outcome) = match edited {
        Ok(edited) => edited,
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    };

    // Nothing changed, nothing is written: rewriting a file to the same bytes
    // still shows up as a modification in an editor and in `git status`.
    if matches!(
        outcome,
        crate::hooks::Outcome::AlreadyInstalled | crate::hooks::Outcome::NotInstalled
    ) {
        let _ = writeln!(output.out, "{}", describe_outcome(outcome, &settings));
        return Exit::Clean;
    }

    if let Some(parent) = settings.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        let _ = writeln!(output.err, "cannot create `{parent}`: {error}");
        return Exit::ConfigProblem;
    }
    if let Err(error) = std::fs::write(&settings, contents) {
        let _ = writeln!(output.err, "cannot write `{settings}`: {error}");
        return Exit::ConfigProblem;
    }

    let _ = writeln!(output.out, "{}", describe_outcome(outcome, &settings));
    // Naming the command is the point: a hook that resolves to nothing fails
    // silently, at someone else's next write rather than here. Only on the
    // way in — after a removal there is no command to name.
    if outcome == crate::hooks::Outcome::Installed {
        let _ = writeln!(output.out, "  {command}");
    }
    Exit::Clean
}

fn describe_outcome(outcome: crate::hooks::Outcome, settings: &Utf8Path) -> String {
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
fn check_one(
    explicit: Option<&Utf8Path>,
    working_directory: &Utf8Path,
    argument: &str,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let (merged, compiled) = match prepare(explicit, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    let path = match crate::describe::repo_relative(&merged.root, working_directory, argument) {
        Ok(path) => path,
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    };

    let single = archwarden_engine::single::check_file(&merged.root, &compiled, &path);
    crate::report::render_single(&single, format, output.out);

    if single.fails_build() {
        Exit::Errors
    } else {
        Exit::Clean
    }
}

/// Shows the smallest shape that would satisfy the rules at one path.
///
/// Shares `describe`'s path resolution and config loading, and is built on its
/// answer, so the two commands cannot disagree about what applies.
fn scaffold(
    explicit: Option<&Utf8Path>,
    working_directory: &Utf8Path,
    argument: &str,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let (merged, compiled) = match prepare(explicit, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    let path = match crate::describe::repo_relative(&merged.root, working_directory, argument) {
        Ok(path) => path,
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    };

    let shape = crate::scaffold::scaffold(&compiled, &path);
    crate::scaffold::render(&path, &shape, format, output.out);
    Exit::Clean
}

/// Emits the rule set as a digest for an agent's context.
///
/// Writes to stdout rather than to a file: `AGENT-INTEGRATION.md` shows it
/// redirected into `.archwarden/AGENT_RULES.md`, and a command that chose the
/// destination itself would be a command that writes where the user did not
/// ask.
fn agent_guide(
    explicit: Option<&Utf8Path>,
    working_directory: &Utf8Path,
    format: crate::guide::GuideFormat,
    scope: Option<&str>,
    output: &mut Output<'_>,
) -> Exit {
    let (merged, compiled) = match prepare(explicit, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    let scope = match scope
        .map(|scope| crate::describe::repo_relative(&merged.root, working_directory, scope))
        .transpose()
    {
        Ok(scope) => scope,
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    };

    let guide = crate::guide::guide(&compiled, scope.as_ref());
    crate::guide::render(&guide, format, output.out);
    Exit::Clean
}

/// archwarden's own directory in the repository, and the cache inside it.
///
/// Decision 4 in `DECISIONS.md`: archwarden owns `.archwarden/` for generated
/// artefacts and never writes anywhere else in the user's tree.
const CACHE_DIRECTORY: &str = ".archwarden/cache";

/// The database file itself. Its format version lives inside it, so this name
/// does not change when the shape does.
const CACHE_FILE: &str = "cache.redb";

fn check(
    explicit: Option<&Utf8Path>,
    working_directory: &Utf8Path,
    format: Format,
    no_cache: bool,
    output: &mut Output<'_>,
) -> Exit {
    // From here rather than from `main`: argument parsing is not the run, and
    // a number that moved with clap's work would not be the one a user is
    // comparing between two invocations.
    let started = std::time::Instant::now();

    let (merged, compiled) = match prepare(explicit, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    let tree = match archwarden_engine::walk::walk(&merged.root, &compiled) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = writeln!(output.err, "{error}");
            return Exit::ConfigProblem;
        }
    };

    // Opened only when a rule will actually look inside a file. A purely
    // structural configuration reads no bytes, and a cache it never consults
    // would just be a file someone has to wonder about.
    let mut cache = if no_cache || !archwarden_engine::run::reads_files(&compiled) {
        None
    } else {
        open_cache(&merged.root, output)
    };

    let outcome = archwarden_engine::run::check(archwarden_engine::run::Run {
        root: &merged.root,
        config: &compiled,
        tree: &tree,
        cache: cache.as_mut(),
    });

    // A cache that did not persist costs the next run its speed and nothing
    // else, so it is a note on stderr rather than a failure.
    if let Some(cache) = cache.as_mut()
        && let Err(error) = cache.flush()
    {
        let _ = writeln!(output.err, "note: the cache was not written — {error}");
    }

    crate::report::render(&outcome, format, started.elapsed(), output.out);

    if outcome.fails_build() {
        Exit::Errors
    } else {
        Exit::Clean
    }
}

/// Opens the repository's cache, or explains why it is running without one.
///
/// A cache is a rebuildable artefact. Refusing to lint because one is damaged
/// would be the wrong trade, so a failure here degrades the run instead of
/// ending it.
fn open_cache(root: &Utf8Path, output: &mut Output<'_>) -> Option<Cache> {
    match Cache::open(&root.join(CACHE_DIRECTORY).join(CACHE_FILE)) {
        Ok(cache) => Some(cache),
        Err(error) => {
            let _ = writeln!(output.err, "note: running without a cache — {error}");
            None
        }
    }
}

/// Loads the config, either from an explicit path or by searching upwards.
///
/// A relative `--config` resolves against the working directory rather than
/// against the process's own, so nothing here depends on ambient state and
/// `run` behaves identically in a test and in a shell.
fn load(
    explicit: Option<&Utf8Path>,
    working_directory: &Utf8Path,
) -> Result<LoadedConfig, discovery::LoadError> {
    match explicit {
        Some(path) if path.is_absolute() => discovery::load_file(path),
        Some(path) => discovery::load_file(&working_directory.join(path)),
        None => discovery::load_from(working_directory),
    }
}

/// Looks for a configuration that parses and is still wrong.
///
/// Exits clean even with concerns. They are advice about a configuration, not
/// findings about code, and a non-zero exit would put them in a CI gate where
/// a deliberate choice would start failing builds.
fn doctor(
    explicit: Option<&Utf8Path>,
    working_directory: &Utf8Path,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let Ok((merged, compiled)) = prepare(explicit, working_directory, output) else {
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
    Exit::Clean
}

/// Shows what one rule reaches, and what it is reporting.
fn explain(
    explicit: Option<&Utf8Path>,
    working_directory: &Utf8Path,
    rule_id: &str,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let Ok((merged, compiled)) = prepare(explicit, working_directory, output) else {
        return Exit::ConfigProblem;
    };

    let id = match archwarden_core::ids::RuleId::new(rule_id) {
        Ok(id) => id,
        Err(error) => {
            let _ = writeln!(output.err, "`{rule_id}` is not a rule id: {error}");
            return Exit::ConfigProblem;
        }
    };

    let tree = match archwarden_engine::walk::walk(&merged.root, &compiled) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = writeln!(output.err, "{error}");
            return Exit::ConfigProblem;
        }
    };

    match crate::explain::explain(&merged.root, &compiled, &tree, &id) {
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

fn validate(
    explicit: Option<&Utf8Path>,
    working_directory: &Utf8Path,
    output: &mut Output<'_>,
) -> Exit {
    match prepare(explicit, working_directory, output) {
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
fn report_valid(merged: &MergedConfig, rules: usize, output: &mut Output<'_>) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Captured {
        out: String,
        err: String,
        exit: Exit,
    }

    /// Runs a command line against a temporary repository and captures both
    /// streams, so assertions can be about what the user sees.
    fn run_in(files: &[(&str, &str)], args: &[&str]) -> (tempfile::TempDir, Captured) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");

        for (relative, contents) in files {
            let path = root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create dirs");
            }
            std::fs::write(&path, contents).expect("write file");
        }

        let captured = run_at(&root, args);
        (dir, captured)
    }

    /// Runs against an existing tree, so a test can run twice over one
    /// repository -- which is the only way to observe a cache at all.
    fn run_at(root: &Utf8Path, args: &[&str]) -> Captured {
        run_with(root, args, "")
    }

    /// Runs with something on stdin, for the hook.
    fn run_with(root: &Utf8Path, args: &[&str], input: &str) -> Captured {
        let mut command_line = vec!["archwarden"];
        command_line.extend_from_slice(args);
        let cli = Cli::try_parse_from(command_line).expect("arguments should parse");

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut input = input.as_bytes();
        let exit = run(
            &cli,
            root,
            &mut Output {
                out: &mut stdout,
                err: &mut stderr,
                input: &mut input,
            },
        );

        Captured {
            out: String::from_utf8(stdout).expect("stdout is UTF-8"),
            err: String::from_utf8(stderr).expect("stderr is UTF-8"),
            exit,
        }
    }

    /// The summary counts, pulled back out of a JSON report.
    fn cache_split(captured: &Captured) -> (u64, u64) {
        let parsed: serde_json::Value =
            serde_json::from_str(&captured.out).expect("stdout is a JSON report");
        (
            parsed["summary"]["files_parsed"]
                .as_u64()
                .expect("files_parsed"),
            parsed["summary"]["facts_reused"]
                .as_u64()
                .expect("facts_reused"),
        )
    }

    /// A configuration whose one rule has to look inside files.
    const NAMING: &str = r#"{"version":0,"rules":[{
        "type":"naming","id":"usecase-name","level":"error","roots":"src/*",
        "file_pattern":"^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
        "must_export":{"name":"{{pascal(name)}}","kind":"function"}}]}"#;

    const MINIMAL: &str = r#"{"version": 0}"#;

    #[test]
    fn validating_a_good_config_is_clean_and_says_so() {
        let (_guard, result) = run_in(&[("arch.config.json", MINIMAL)], &["config", "validate"]);

        assert_eq!(result.exit, Exit::Clean);
        assert!(result.out.contains("is valid"), "{}", result.out);
        assert!(result.err.is_empty(), "{}", result.err);
    }

    /// The count is part of the message because it is the cheapest way for a
    /// user to notice that a preset did not load, or that `disable` removed
    /// more than they meant.
    #[test]
    fn validation_reports_how_many_rules_are_active() {
        let (_guard, result) = run_in(
            &[(
                "arch.config.json",
                r#"{"version":0,"rules":[
                    {"type":"structure","id":"a","level":"error","roots":"x/*"},
                    {"type":"structure","id":"b","level":"error","roots":"y/*"}]}"#,
            )],
            &["config", "validate"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(result.out.contains("2 rules"), "{}", result.out);
    }

    #[test]
    fn the_rule_count_is_singular_for_one_rule() {
        let (_guard, result) = run_in(
            &[(
                "arch.config.json",
                r#"{"version":0,"rules":[
                    {"type":"structure","id":"a","level":"error","roots":"x/*"}]}"#,
            )],
            &["config", "validate"],
        );

        assert!(result.out.contains("1 rule)"), "{}", result.out);
    }

    /// A broken config exits 2, not 1: a pipeline should be able to tell
    /// "your setup is wrong" from "your code is wrong".
    #[test]
    fn a_malformed_config_is_a_config_problem_not_a_finding() {
        let (_guard, result) = run_in(
            &[("arch.config.json", r#"{"version": 0,,}"#)],
            &["config", "validate"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.out.is_empty(), "nothing is claimed to be valid");
        assert!(result.err.contains("arch.config.json"), "{}", result.err);
    }

    /// The diagnostic shows the offending line, which is the reason miette is
    /// a dependency at all.
    #[test]
    fn a_syntax_error_is_rendered_with_the_offending_source() {
        let (_guard, result) = run_in(
            &[(
                "arch.config.json",
                "{\n  \"version\": 0,\n  \"rules\": \"not an array\"\n}",
            )],
            &["config", "validate"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("not an array"), "{}", result.err);
    }

    #[test]
    fn a_missing_config_is_a_config_problem_and_suggests_init() {
        let (_guard, result) = run_in(&[("src/x.ts", "")], &["config", "validate"]);

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("archwarden init"), "{}", result.err);
    }

    /// An old binary must refuse a newer config rather than misreading it.
    #[test]
    fn an_unsupported_version_is_refused_by_number() {
        let (_guard, result) = run_in(
            &[("arch.config.json", r#"{"version": 99}"#)],
            &["config", "validate"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("99"), "{}", result.err);
        assert!(result.err.contains("version 0"), "{}", result.err);
    }

    /// `--config` skips the upward search, which is how a caller escapes the
    /// one-config-per-repository model when they have to.
    #[test]
    fn an_explicit_config_path_bypasses_discovery() {
        let (_guard, result) = run_in(
            &[("elsewhere/other.json", MINIMAL)],
            &["config", "validate", "--config", "elsewhere/other.json"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(result.out.contains("other.json"), "{}", result.out);
    }

    /// Running from a subdirectory finds the repository's config, which is the
    /// monorepo behaviour decision 4 exists for.
    #[test]
    fn discovery_walks_up_from_the_working_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("UTF-8");
        std::fs::write(root.join("arch.config.json"), MINIMAL).expect("write");
        let nested = root.join("packages/app/src");
        std::fs::create_dir_all(&nested).expect("create dirs");

        let cli = Cli::try_parse_from(["archwarden", "config", "validate"]).expect("parses");
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut input = std::io::empty();
        let exit = run(
            &cli,
            &nested,
            &mut Output {
                out: &mut stdout,
                err: &mut stderr,
                input: &mut input,
            },
        );

        assert_eq!(exit, Exit::Clean);
        assert!(String::from_utf8_lossy(&stdout).contains("arch.config.json"));
    }

    /// A preset's rules count towards the total, and the files that
    /// contributed are listed. Seeing the preset in the output is how a user
    /// notices it loaded from where they expected.
    #[test]
    fn validation_folds_in_presets_and_names_them() {
        let (_guard, result) = run_in(
            &[
                (
                    "presets/base.json",
                    r#"{"version":0,"rules":[
                        {"type":"structure","id":"from-preset","level":"error","roots":"p/*"}]}"#,
                ),
                (
                    "arch.config.json",
                    r#"{"version":0,"extends":"./presets/base.json","rules":[
                        {"type":"structure","id":"local","level":"error","roots":"l/*"}]}"#,
                ),
            ],
            &["config", "validate"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(result.out.contains("2 rules"), "{}", result.out);
        assert!(result.out.contains("extends:"), "{}", result.out);
        assert!(result.out.contains("presets/base.json"), "{}", result.out);
    }

    /// A config with no presets says nothing about them, rather than printing
    /// an empty section.
    #[test]
    fn a_config_without_presets_says_nothing_about_them() {
        let (_guard, result) = run_in(&[("arch.config.json", MINIMAL)], &["config", "validate"]);
        assert!(!result.out.contains("extends:"), "{}", result.out);
    }

    /// A cycle would otherwise recurse until the stack ran out. The user gets
    /// a diagnostic and a way out instead of a crash.
    #[test]
    fn an_extends_cycle_is_a_config_problem_with_a_way_out() {
        let (_guard, result) = run_in(
            &[
                ("arch.config.json", r#"{"version":0,"extends":"./a.json"}"#),
                ("a.json", r#"{"version":0,"extends":"./arch.config.json"}"#),
            ],
            &["config", "validate"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("cycle"), "{}", result.err);
        assert!(result.err.contains("break the loop"), "{}", result.err);
    }

    /// Two rules with one id make `explain` and `disable` ambiguous, so it is
    /// refused with both filenames and a suggestion.
    #[test]
    fn a_duplicate_rule_id_across_a_preset_is_refused() {
        let (_guard, result) = run_in(
            &[
                (
                    "presets/base.json",
                    r#"{"version":0,"rules":[
                        {"type":"structure","id":"clash","level":"error","roots":"p/*"}]}"#,
                ),
                (
                    "arch.config.json",
                    r#"{"version":0,"extends":"./presets/base.json","rules":[
                        {"type":"structure","id":"clash","level":"error","roots":"l/*"}]}"#,
                ),
            ],
            &["config", "validate"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("clash"), "{}", result.err);
        assert!(result.err.contains("disable"), "{}", result.err);
    }

    /// A missing preset is a config problem, not a silent partial load.
    #[test]
    fn an_unresolvable_preset_is_a_config_problem() {
        let (_guard, result) = run_in(
            &[(
                "arch.config.json",
                r#"{"version":0,"extends":"@org/not-installed"}"#,
            )],
            &["config", "validate"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("@org/not-installed"), "{}", result.err);
    }

    /// The version check runs before presets are touched: an unsupported
    /// version means this build cannot be trusted to read the file at all.
    #[test]
    fn an_unsupported_version_is_refused_before_presets_are_resolved() {
        let (_guard, result) = run_in(
            &[(
                "arch.config.json",
                r#"{"version":99,"extends":"@org/not-installed"}"#,
            )],
            &["config", "validate"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("99"), "{}", result.err);
        assert!(
            !result.err.contains("not-installed"),
            "should not have tried to resolve: {}",
            result.err
        );
    }

    /// The point of compiling during `validate`: a pattern that no engine can
    /// run is a config problem, and the user hears about it now rather than
    /// the first time a matching file appears.
    #[test]
    fn an_unsupported_regex_construct_fails_validation() {
        let (_guard, result) = run_in(
            &[(
                "arch.config.json",
                r#"{"version":0,"rules":[
                    {"type":"structure","id":"no-lookahead","level":"error","roots":"src/*",
                     "filename_patterns":["^(?!.*\\.spec\\.ts$).*\\.ts$"]}]}"#,
            )],
            &["config", "validate"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("no-lookahead"), "{}", result.err);
        assert!(result.err.contains("negative lookahead"), "{}", result.err);
        assert!(result.err.contains("linear-time"), "{}", result.err);
    }

    /// A template naming a capture group its pattern never defines would
    /// otherwise stay silent until some file happened to match.
    #[test]
    fn a_template_with_a_missing_capture_group_fails_validation() {
        let (_guard, result) = run_in(
            &[(
                "arch.config.json",
                r#"{"version":0,"rules":[
                    {"type":"naming","id":"typo","level":"error","roots":"src/*",
                     "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                     "must_export":{"kind":"function","name":"{{pascal(nome)}}"}}]}"#,
            )],
            &["config", "validate"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("typo"), "{}", result.err);
        assert!(result.err.contains("nome"), "{}", result.err);
    }

    /// An invalid glob in a rule's scope is caught before any file is walked.
    #[test]
    fn an_invalid_scope_glob_fails_validation() {
        let (_guard, result) = run_in(
            &[(
                "arch.config.json",
                r#"{"version":0,"rules":[
                    {"type":"structure","id":"bad","level":"error","roots":"packages/[domain"}]}"#,
            )],
            &["config", "validate"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("bad"), "{}", result.err);
    }

    /// clap's own contract, worth pinning: the parser is built from these
    /// types and a mistake in them is not otherwise visible until runtime.
    #[test]
    fn the_command_line_grammar_is_what_it_claims() {
        use clap::CommandFactory;
        Cli::command().debug_assert();

        assert!(
            Cli::try_parse_from(["archwarden"]).is_err(),
            "needs a subcommand"
        );
        assert!(
            Cli::try_parse_from(["archwarden", "config"]).is_err(),
            "needs a sub-subcommand"
        );
        assert!(Cli::try_parse_from(["archwarden", "nope"]).is_err());
        assert!(Cli::try_parse_from(["archwarden", "config", "validate"]).is_ok());
    }

    /// The cache lives in `.archwarden/cache/`, which is archwarden's own
    /// directory in the repository and the one `.gitignore` covers.
    #[test]
    fn checking_writes_a_cache_under_the_repository() {
        let (guard, result) = run_in(
            &[
                ("arch.config.json", NAMING),
                (
                    "src/user/create-client.use-case.ts",
                    "export function CreateClient() {}",
                ),
            ],
            &["check"],
        );

        assert_eq!(result.exit, Exit::Clean);
        let cache = guard.path().join(CACHE_DIRECTORY);
        assert!(cache.is_dir(), "no cache at {}", cache.display());
    }

    /// The point of the whole milestone: the second run over an unchanged
    /// repository parses nothing.
    #[test]
    fn a_second_check_reuses_the_cached_facts() {
        let (guard, cold) = run_in(
            &[
                ("arch.config.json", NAMING),
                (
                    "src/user/create-client.use-case.ts",
                    "export function CreateClient() {}",
                ),
                (
                    "src/user/delete-client.use-case.ts",
                    "export function DeleteClient() {}",
                ),
            ],
            &["check", "--format", "json"],
        );
        assert_eq!(cache_split(&cold), (2, 0));

        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        let warm = run_at(&root, &["check", "--format", "json"]);

        assert_eq!(cache_split(&warm), (0, 2));

        // The counts differ by design -- that is the point of the cache. What
        // may never differ is the answer, or the scope it was computed over.
        let (cold, warm): (serde_json::Value, serde_json::Value) = (
            serde_json::from_str(&cold.out).expect("JSON"),
            serde_json::from_str(&warm.out).expect("JSON"),
        );
        assert_eq!(warm["findings"], cold["findings"]);
        assert_eq!(
            warm["summary"]["files_scanned"],
            cold["summary"]["files_scanned"]
        );

        // And the cache is not itself a file to be checked: it lives in a
        // dotted directory precisely so the walk never sees it.
        assert_eq!(cold["summary"]["files_scanned"], 3);
    }

    /// `--no-cache` is the escape hatch for a suspected cache bug, so it has to
    /// both skip reading and skip writing.
    #[test]
    fn no_cache_neither_reads_nor_writes() {
        let files = [
            ("arch.config.json", NAMING),
            (
                "src/user/create-client.use-case.ts",
                "export function CreateClient() {}",
            ),
        ];
        let (guard, first) = run_in(&files, &["check", "--format", "json", "--no-cache"]);
        assert_eq!(cache_split(&first), (1, 0));
        assert!(
            !guard.path().join(CACHE_DIRECTORY).exists(),
            "nothing should have been written"
        );

        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        let second = run_at(&root, &["check", "--format", "json", "--no-cache"]);
        assert_eq!(cache_split(&second), (1, 0), "and nothing was read back");
    }

    /// An edited file is parsed again. A cache that missed an edit would be
    /// worse than no cache at all.
    #[test]
    fn editing_a_file_forces_it_to_be_parsed_again() {
        let (guard, first) = run_in(
            &[
                ("arch.config.json", NAMING),
                (
                    "src/user/create-client.use-case.ts",
                    "export function CreateClient() {}",
                ),
            ],
            &["check", "--format", "json"],
        );
        assert_eq!(first.exit, Exit::Clean);

        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        std::fs::write(
            root.join("src/user/create-client.use-case.ts"),
            "export const CreateClient = () => {};",
        )
        .expect("edit");

        let second = run_at(&root, &["check", "--format", "json"]);
        assert_eq!(cache_split(&second), (1, 0));
        assert_eq!(second.exit, Exit::Errors, "the new fault is reported");
    }

    /// A cache that cannot be opened must not stop the run: it is a rebuildable
    /// artefact, and refusing to lint because of one would be the wrong trade.
    #[test]
    fn an_unusable_cache_is_a_note_not_a_failure() {
        let (guard, first) = run_in(
            &[
                ("arch.config.json", NAMING),
                (
                    "src/user/create-client.use-case.ts",
                    "export function CreateClient() {}",
                ),
            ],
            &["check", "--format", "json"],
        );
        assert_eq!(first.exit, Exit::Clean);

        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        // A directory where the database file should be: unopenable, and not
        // something the cache may delete its way out of.
        let database = root.join(CACHE_DIRECTORY).join(CACHE_FILE);
        std::fs::remove_file(&database).expect("remove the database");
        std::fs::create_dir(&database).expect("put a directory in its place");

        let second = run_at(&root, &["check", "--format", "json"]);

        assert_eq!(second.exit, Exit::Clean, "the run still happened");
        assert_eq!(cache_split(&second), (1, 0), "and it parsed for itself");
        assert!(second.err.contains("cache"), "{}", second.err);
        drop(guard);
    }

    /// A boundary rule, through the real binary path: config, walk, parse,
    /// resolve, check, render. The alias is what makes this worth a test at
    /// this level -- nothing below the resolver could tell that
    /// `@/domain/user` is the file the rule forbids.
    #[test]
    fn a_forbidden_import_is_reported_end_to_end() {
        let (guard, result) = run_in(
            &[
                (
                    "arch.config.json",
                    r#"{"version":0,"rules":[{
                        "type":"import-boundary","id":"ui-forbids-domain","level":"error",
                        "from":"packages/ui/**","forbid_import_from":"packages/domain/**"}]}"#,
                ),
                (
                    "tsconfig.json",
                    r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["packages/*"]}}}"#,
                ),
                (
                    "packages/ui/button.tsx",
                    "import { User } from '@/domain/user';\nexport const Button = () => User;",
                ),
                ("packages/domain/user.ts", "export const User = 1;"),
            ],
            &["check"],
        );

        assert_eq!(result.exit, Exit::Errors, "{}", result.out);
        assert!(
            result.out.contains("packages/ui/button.tsx"),
            "{}",
            result.out
        );
        assert!(
            result
                .out
                .contains("imports `@/domain/user`, which resolves to"),
            "{}",
            result.out
        );
        assert!(
            result.out.contains("packages/domain/user.ts"),
            "the resolved path is shown: {}",
            result.out
        );
        drop(guard);
    }

    /// The other half of the same run: an import nothing could place is said
    /// out loud, because a boundary rule did not check it.
    #[test]
    fn unresolved_imports_are_noted_in_the_report() {
        let (_guard, result) = run_in(
            &[
                (
                    "arch.config.json",
                    r#"{"version":0,"rules":[{
                        "type":"import-boundary","id":"ui-forbids-domain","level":"error",
                        "from":"packages/ui/**","forbid_import_from":"packages/domain/**"}]}"#,
                ),
                (
                    "packages/ui/button.tsx",
                    "import { x } from '@org/never-installed';\nexport const B = x;",
                ),
            ],
            &["check"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(
            result.out.contains("1 import could not resolve"),
            "{}",
            result.out
        );
    }

    /// The command an agent calls before it writes. The file does not exist,
    /// and that is the point.
    #[test]
    fn describe_answers_about_a_file_that_does_not_exist() {
        let (_guard, result) = run_in(
            &[("arch.config.json", NAMING)],
            &["describe", "src/user/create-client.use-case.ts"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(
            result.out.contains("usecase-name (naming)"),
            "{}",
            result.out
        );
        assert!(
            result.out.contains("an export named `CreateClient`"),
            "{}",
            result.out
        );
    }

    /// The JSON an agent should actually consume.
    #[test]
    fn describe_emits_a_versioned_json_shape() {
        let (_guard, result) = run_in(
            &[("arch.config.json", NAMING)],
            &[
                "describe",
                "src/user/create-client.use-case.ts",
                "--format",
                "json",
            ],
        );

        let parsed: serde_json::Value = serde_json::from_str(&result.out).expect("valid JSON");
        assert_eq!(parsed["version"], 0);
        assert_eq!(parsed["path"], "src/user/create-client.use-case.ts");
        assert_eq!(parsed["rules"][0]["id"], "usecase-name");
        assert_eq!(
            parsed["rules"][0]["expectations"][0]["name"],
            "CreateClient"
        );
    }

    /// Nothing applying is a clean answer, not a failure: an agent branching
    /// on the exit code should see a non-zero one only when its setup is
    /// wrong.
    #[test]
    fn describe_exits_clean_when_no_rule_applies() {
        let (_guard, result) = run_in(&[("arch.config.json", NAMING)], &["describe", "README.md"]);

        assert_eq!(result.exit, Exit::Clean);
        assert!(result.out.contains("No rule applies"), "{}", result.out);
    }

    /// A path outside the repository is refused rather than silently
    /// described as something else.
    #[test]
    fn describe_refuses_a_path_outside_the_repository() {
        let (_guard, result) = run_in(&[("arch.config.json", NAMING)], &["describe", "../a.ts"]);

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.out.is_empty(), "nothing is described");
        assert!(result.err.contains("../a.ts"), "{}", result.err);
    }

    /// A broken config is still exit 2 here, so an agent can tell "your setup
    /// is wrong" from "nothing applies".
    #[test]
    fn describing_with_a_broken_config_exits_two() {
        let (_guard, result) = run_in(
            &[("arch.config.json", r#"{"version": 0,,}"#)],
            &["describe", "src/a.ts"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem);
    }

    /// The other half of Layer 2: having asked what applies, the agent asks
    /// what to write.
    #[test]
    fn scaffold_shows_what_to_write() {
        let (_guard, result) = run_in(
            &[("arch.config.json", NAMING)],
            &["scaffold", "src/user/create-client.use-case.ts"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(
            result.out.contains("export function CreateClient"),
            "{}",
            result.out
        );
    }

    /// And the JSON an agent should consume, which `describe` and `scaffold`
    /// version separately.
    #[test]
    fn scaffold_emits_a_versioned_json_shape() {
        let (_guard, result) = run_in(
            &[("arch.config.json", NAMING)],
            &[
                "scaffold",
                "src/user/create-client.use-case.ts",
                "--format",
                "json",
            ],
        );

        let parsed: serde_json::Value = serde_json::from_str(&result.out).expect("valid JSON");
        assert_eq!(parsed["version"], 0);
        assert_eq!(parsed["required_exports"][0]["name"], "CreateClient");
    }

    /// Layer 3: the digest a harness reads from `CLAUDE.md`.
    #[test]
    fn agent_guide_emits_the_rule_set() {
        let (_guard, result) = run_in(&[("arch.config.json", NAMING)], &["agent-guide"]);

        assert_eq!(result.exit, Exit::Clean);
        assert!(
            result.out.contains("# Architecture rules"),
            "{}",
            result.out
        );
        assert!(result.out.contains("`usecase-name`"), "{}", result.out);
        assert!(result.out.contains("{{pascal(name)}}"), "{}", result.out);
    }

    /// Committed by some users, regenerated by others. Two runs of the same
    /// configuration must not differ, or one choice creates diffs for the
    /// other.
    #[test]
    fn agent_guide_is_byte_identical_across_runs() {
        let files = [("arch.config.json", NAMING)];
        let (_a, first) = run_in(&files, &["agent-guide"]);
        let (_b, second) = run_in(&files, &["agent-guide"]);

        assert_eq!(first.out, second.out);
    }

    /// `--scope` restricts the digest, so a large monorepo can hand one
    /// package's agent only that package's rules.
    #[test]
    fn agent_guide_can_be_restricted_to_a_scope() {
        let config = r#"{"version":0,"rules":[
            {"type":"structure","id":"domain-shape","level":"error",
             "roots":"packages/domain/*","allowed_subfolders":["types"]},
            {"type":"structure","id":"web-shape","level":"error",
             "roots":"apps/web/*","allowed_subfolders":["components"]}]}"#;

        let (_guard, result) = run_in(
            &[("arch.config.json", config)],
            &["agent-guide", "--scope", "packages/domain"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(result.out.contains("domain-shape"), "{}", result.out);
        assert!(!result.out.contains("web-shape"), "{}", result.out);
    }

    /// Layer 4: the hook asks about one file and blocks on the exit code.
    #[test]
    fn check_file_reports_one_file_and_exits_one() {
        let (_guard, result) = run_in(
            &[
                ("arch.config.json", NAMING),
                (
                    "src/user/create-client.use-case.ts",
                    "export const CreateClient = () => {};",
                ),
                ("src/user/other.use-case.ts", "export const Other = 1;"),
            ],
            &["check", "--file", "src/user/create-client.use-case.ts"],
        );

        assert_eq!(result.exit, Exit::Errors);
        assert!(
            result.out.contains("src/user/create-client.use-case.ts"),
            "{}",
            result.out
        );
        assert!(
            !result.out.contains("other.use-case.ts"),
            "the neighbour is not this write's problem: {}",
            result.out
        );
    }

    /// A clean file exits zero and says so, so a hook can tell "checked and
    /// fine" from "nothing happened".
    #[test]
    fn check_file_says_when_a_file_is_fine() {
        let (_guard, result) = run_in(
            &[
                ("arch.config.json", NAMING),
                (
                    "src/user/create-client.use-case.ts",
                    "export function CreateClient() {}",
                ),
            ],
            &["check", "--file", "src/user/create-client.use-case.ts"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(result.out.contains("is fine"), "{}", result.out);
    }

    /// Correction C6: a rule that could not be evaluated is named. A file that
    /// does not exist cannot be parsed, and passing quietly would let a hook
    /// wave through a write it never checked.
    #[test]
    fn check_file_reports_what_it_could_not_check() {
        let (_guard, result) = run_in(
            &[("arch.config.json", NAMING)],
            &[
                "check",
                "--file",
                "src/user/create-client.use-case.ts",
                "--format",
                "json",
            ],
        );

        assert_eq!(result.exit, Exit::Clean, "nothing was found");
        let parsed: serde_json::Value = serde_json::from_str(&result.out).expect("valid JSON");
        assert_eq!(parsed["skipped"][0]["rule_id"], "usecase-name");
        assert_eq!(parsed["skipped"][0]["reason"], "unreadable");
    }

    /// `skipped` is present even when empty: a caller has to see the list is
    /// empty rather than infer it from absence.
    #[test]
    fn check_file_always_carries_the_skipped_list() {
        let (_guard, result) = run_in(
            &[
                ("arch.config.json", NAMING),
                (
                    "src/user/create-client.use-case.ts",
                    "export function CreateClient() {}",
                ),
            ],
            &[
                "check",
                "--file",
                "src/user/create-client.use-case.ts",
                "--format",
                "json",
            ],
        );

        let parsed: serde_json::Value = serde_json::from_str(&result.out).expect("valid JSON");
        assert!(
            parsed["skipped"].as_array().is_some_and(Vec::is_empty),
            "{}",
            result.out
        );
    }

    const WRITE_EVENT: &str = r#"{
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": { "file_path": "src/user/create-client.use-case.ts" }
    }"#;

    /// The whole point of Layer 4: an invalid write is refused, with a message
    /// naming the rule and the fix.
    #[test]
    fn the_hook_denies_a_write_that_would_break_a_rule() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("UTF-8");
        std::fs::write(root.join("arch.config.json"), NAMING).expect("write");
        std::fs::create_dir_all(root.join("src/user")).expect("create dirs");
        std::fs::write(
            root.join("src/user/create-client.use-case.ts"),
            "export const CreateClient = () => {};",
        )
        .expect("write");

        let result = run_with(&root, &["hook", "claude-code"], WRITE_EVENT);

        assert_eq!(result.exit, Exit::Clean, "the hook itself did not fail");
        let parsed: serde_json::Value = serde_json::from_str(&result.out).expect("valid JSON");
        assert_eq!(
            parsed["hookSpecificOutput"]["permissionDecision"], "deny",
            "{}",
            result.out
        );
        let message = parsed["systemMessage"].as_str().expect("a message");
        assert!(message.contains("usecase-name"), "{message}");
        assert!(message.contains("expected:"), "{message}");
    }

    /// A legal write is allowed, quietly.
    #[test]
    fn the_hook_allows_a_write_that_is_fine() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("UTF-8");
        std::fs::write(root.join("arch.config.json"), NAMING).expect("write");
        std::fs::create_dir_all(root.join("src/user")).expect("create dirs");
        std::fs::write(
            root.join("src/user/create-client.use-case.ts"),
            "export function CreateClient() {}",
        )
        .expect("write");

        let result = run_with(&root, &["hook", "claude-code"], WRITE_EVENT);

        assert_eq!(result.exit, Exit::Clean);
        assert_eq!(result.out, "{}\n");
    }

    /// A hook that blocked because *it* failed would be worse than no hook.
    /// Every unexpected shape lets the write through.
    #[test]
    fn the_hook_never_blocks_because_of_its_own_trouble() {
        let cases: [(&str, &str); 4] = [
            ("a broken config", r#"{"version": 0,,}"#),
            ("a config for a future version", r#"{"version": 99}"#),
            (
                "an uncompilable rule",
                r#"{"version":0,"rules":[{"type":"structure",
                "id":"a","level":"error","roots":"["}]}"#,
            ),
            ("no rules at all", r#"{"version":0}"#),
        ];

        for (what, config) in cases {
            let (_guard, result) = {
                let dir = tempfile::tempdir().expect("temp dir");
                let root =
                    Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
                        .expect("UTF-8");
                std::fs::write(root.join("arch.config.json"), config).expect("write");
                let result = run_with(&root, &["hook", "claude-code"], WRITE_EVENT);
                (dir, result)
            };

            assert_eq!(result.exit, Exit::Clean, "{what}");
            assert_eq!(result.out, "{}\n", "{what} should allow the write");
        }
    }

    /// A payload naming no file is not this hook's business.
    #[test]
    fn the_hook_allows_a_tool_that_writes_nothing() {
        let (_guard, result) = run_in(&[("arch.config.json", NAMING)], &["hook", "claude-code"]);

        assert_eq!(result.exit, Exit::Clean);
        assert_eq!(result.out, "{}\n");
    }

    /// `install-hooks` writes the settings file, and says what it did.
    #[test]
    fn install_hooks_writes_the_settings_file() {
        let (guard, result) = run_in(
            &[("arch.config.json", MINIMAL)],
            &["install-hooks", "--claude-code"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(
            result.out.contains("installed the pre-write hook"),
            "{}",
            result.out
        );

        let settings = guard.path().join(crate::hooks::CLAUDE_SETTINGS);
        let written = std::fs::read_to_string(&settings).expect("the file exists");
        assert!(written.contains(crate::hooks::HOOK_COMMAND), "{written}");
    }

    /// A node project gets a command a harness can run, and is told which one.
    ///
    /// The bare `archwarden` was wrong here: as a dev dependency it is in
    /// `node_modules/.bin`, which is on the PATH of a `package.json` script
    /// and of nothing else. The message names the command because a hook that
    /// resolves to nothing fails silently, at someone else's next write.
    #[test]
    fn install_hooks_in_a_node_project_installs_a_command_that_resolves() {
        let (guard, result) = run_in(
            &[
                ("arch.config.json", MINIMAL),
                ("package.json", r#"{"name":"app"}"#),
            ],
            &["install-hooks", "--claude-code"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(
            result.out.contains("npx archwarden hook claude-code"),
            "{}",
            result.out
        );

        let written = std::fs::read_to_string(guard.path().join(crate::hooks::CLAUDE_SETTINGS))
            .expect("the file exists");
        assert!(
            written.contains("npx archwarden hook claude-code"),
            "{written}"
        );
    }

    /// Idempotent, and it does not touch the file when there is nothing to
    /// change: rewriting to the same bytes still shows up in `git status`.
    #[test]
    fn install_hooks_run_twice_leaves_the_file_alone() {
        let (guard, first) = run_in(
            &[("arch.config.json", MINIMAL)],
            &["install-hooks", "--claude-code"],
        );
        assert_eq!(first.exit, Exit::Clean);

        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        let settings = root.join(crate::hooks::CLAUDE_SETTINGS);
        let before = std::fs::metadata(&settings)
            .and_then(|m| m.modified())
            .expect("mtime");

        let second = run_at(&root, &["install-hooks", "--claude-code"]);

        assert!(second.out.contains("already in"), "{}", second.out);
        let after = std::fs::metadata(&settings)
            .and_then(|m| m.modified())
            .expect("mtime");
        assert_eq!(before, after, "the file was not rewritten");
    }

    /// And uninstall, which the doc asks for by name.
    #[test]
    fn install_hooks_can_take_the_hook_back_out() {
        let (guard, _) = run_in(
            &[("arch.config.json", MINIMAL)],
            &["install-hooks", "--claude-code"],
        );
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");

        let removed = run_at(&root, &["install-hooks", "--claude-code", "--remove"]);
        assert_eq!(removed.exit, Exit::Clean);
        assert!(removed.out.contains("removed"), "{}", removed.out);
        assert!(
            !removed.out.contains("hook claude-code"),
            "a removal has no command to name: {}",
            removed.out
        );

        let written = std::fs::read_to_string(root.join(crate::hooks::CLAUDE_SETTINGS))
            .expect("the file is still there");
        assert!(!written.contains(crate::hooks::HOOK_COMMAND), "{written}");

        let again = run_at(&root, &["install-hooks", "--claude-code", "--remove"]);
        assert!(again.out.contains("no archwarden hook"), "{}", again.out);
    }

    /// Naming no harness is a usage error, not a silent no-op.
    #[test]
    fn install_hooks_needs_a_harness() {
        let (_guard, result) = run_in(&[("arch.config.json", MINIMAL)], &["install-hooks"]);

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("--claude-code"), "{}", result.err);
    }

    /// The first command a new user runs.
    #[test]
    fn init_writes_a_starter_config() {
        let (guard, result) = run_in(&[], &["init"]);

        assert_eq!(result.exit, Exit::Clean);
        assert!(result.out.contains("wrote"), "{}", result.out);

        let written = std::fs::read_to_string(guard.path().join("arch.config.json"))
            .expect("the file exists");
        assert!(written.contains("$schema"), "{written}");

        // It has to be a config archwarden itself accepts, or the first thing
        // a new user sees is an error from the tool that wrote the file.
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        let validated = run_at(&root, &["config", "validate"]);
        assert_eq!(validated.exit, Exit::Clean, "{}", validated.err);
    }

    /// Where archwarden is installed, the config points at the schema on
    /// disk. It is the schema for the version that is installed, it works with
    /// no network, and it cannot describe a different build than the one being
    /// run.
    #[test]
    fn init_points_at_the_installed_schema_when_there_is_one() {
        let (guard, _) = run_in(
            &[("node_modules/archwarden/schema/v0.json", "{}")],
            &["init"],
        );

        let written = std::fs::read_to_string(guard.path().join("arch.config.json"))
            .expect("the file exists");

        assert!(
            written.contains(r#""$schema": "./node_modules/archwarden/schema/v0.json""#),
            "{written}"
        );
    }

    /// And falls back to the published URL, which has to be one that answers:
    /// `archwarden.dev` never resolved, so every config written before this
    /// pointed at nothing.
    #[test]
    fn init_falls_back_to_the_published_url() {
        let (guard, _) = run_in(&[], &["init"]);

        let written = std::fs::read_to_string(guard.path().join("arch.config.json"))
            .expect("the file exists");

        assert!(written.contains(crate::schema::SCHEMA_URL), "{written}");
        assert!(!written.contains("archwarden.dev"), "{written}");
    }

    /// It never overwrites. A config is hand-written and often long, and a
    /// command that replaced one would be a command nobody runs twice on
    /// purpose.
    #[test]
    fn init_refuses_to_overwrite() {
        let (guard, result) = run_in(&[("arch.config.json", NAMING)], &["init"]);

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("already exists"), "{}", result.err);

        let kept =
            std::fs::read_to_string(guard.path().join("arch.config.json")).expect("still there");
        assert_eq!(kept, NAMING, "the user's config is untouched");
    }

    /// The doctor through the command line, on the mistake that started it:
    /// a rule inside an `ignore` entry that can never fire.
    #[test]
    fn doctor_reports_a_rule_that_can_never_fire() {
        let (_guard, result) = run_in(
            &[(
                "arch.config.json",
                r#"{"version":0,"ignore":"src/legacy/**","rules":[{
                    "type":"structure","id":"legacy-shape","level":"error",
                    "roots":"src/legacy/*","allowed_subfolders":["types"]}]}"#,
            )],
            &["config", "doctor"],
        );

        assert_eq!(result.exit, Exit::Clean, "advice is not a gate");
        assert!(result.out.contains("unreachable-scope"), "{}", result.out);
        assert!(result.out.contains("fix:"), "{}", result.out);
    }

    /// A sound configuration over a matching repository says so.
    #[test]
    fn doctor_is_quiet_about_a_sound_configuration() {
        let (_guard, result) = run_in(
            &[
                ("arch.config.json", NAMING),
                (
                    "src/user/create-client.use-case.ts",
                    "export function CreateClient() {}",
                ),
            ],
            &["config", "doctor"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(result.out.contains("No concerns"), "{}", result.out);
    }

    /// The repository half, through the command line: decision 9's warning
    /// about a file that exports only a default.
    #[test]
    fn doctor_examines_the_repository_too() {
        let (_guard, result) = run_in(
            &[
                ("arch.config.json", NAMING),
                (
                    "src/user/create-client.use-case.ts",
                    "export default function () {}",
                ),
            ],
            &["config", "doctor"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(
            result.out.contains("only-a-default-export"),
            "{}",
            result.out
        );
        assert!(
            result.out.contains("src/user/create-client.use-case.ts"),
            "the file is named: {}",
            result.out
        );
    }

    /// A rule pointed at a directory that is not there is the commonest
    /// configuration mistake, and the one `validate` cannot see.
    #[test]
    fn doctor_reports_a_scope_that_matches_nothing() {
        let (_guard, result) = run_in(
            &[("arch.config.json", NAMING), ("README.md", "")],
            &["config", "doctor"],
        );

        assert!(
            result.out.contains("scope-matches-nothing"),
            "{}",
            result.out
        );
    }

    /// A broken config is still exit 2 here: the doctor cannot advise on a
    /// file it could not read.
    #[test]
    fn doctor_on_a_broken_config_exits_two() {
        let (_guard, result) = run_in(
            &[("arch.config.json", r#"{"version": 0,,}"#)],
            &["config", "doctor"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem);
    }

    /// The JSON an agent or a CI script would read.
    #[test]
    fn doctor_emits_a_versioned_json_shape() {
        let (_guard, result) = run_in(
            &[(
                "arch.config.json",
                r#"{"version":0,"ignore":"src/legacy/**","rules":[{
                    "type":"structure","id":"legacy-shape","level":"error",
                    "roots":"src/legacy/*","allowed_subfolders":["types"]}]}"#,
            )],
            &["config", "doctor", "--format", "json"],
        );

        let parsed: serde_json::Value = serde_json::from_str(&result.out).expect("valid JSON");
        assert_eq!(parsed["version"], 0);
        assert_eq!(parsed["concerns"][0]["code"], "unreachable-scope");
        assert_eq!(parsed["concerns"][0]["rule_id"], "legacy-shape");
    }

    /// The command a user runs when a rule is not doing what they expected.
    #[test]
    fn explain_shows_what_a_rule_reaches_and_reports() {
        let (_guard, result) = run_in(
            &[
                ("arch.config.json", NAMING),
                (
                    "src/user/create-client.use-case.ts",
                    "export const CreateClient = () => {};",
                ),
                ("src/user/helper.ts", "export const helper = 1;"),
            ],
            &["config", "explain", "usecase-name"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(
            result.out.contains("usecase-name (naming)"),
            "{}",
            result.out
        );
        assert!(
            result.out.contains("src/user/create-client.use-case.ts"),
            "{}",
            result.out
        );
        assert!(
            !result.out.contains("helper.ts"),
            "the rule does not cover it: {}",
            result.out
        );
    }

    /// A typo in the id is the likeliest way to get this wrong, so the answer
    /// is the list of real ids.
    #[test]
    fn explain_lists_the_real_ids_for_an_unknown_one() {
        let (_guard, result) = run_in(
            &[("arch.config.json", NAMING)],
            &["config", "explain", "usecase-naming"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("usecase-name"), "{}", result.err);
    }

    /// A structural configuration reads no bytes, so it has nothing to cache
    /// and must not pay for opening one.
    #[test]
    fn a_structural_configuration_writes_no_cache() {
        let (guard, result) = run_in(
            &[
                (
                    "arch.config.json",
                    r#"{"version":0,"rules":[{"type":"structure","id":"shape",
                       "level":"error","roots":"src/*","allowed_subfolders":["types"]}]}"#,
                ),
                ("src/user/types/user.ts", ""),
            ],
            &["check", "--format", "json"],
        );

        assert_eq!(result.exit, Exit::Clean);
        assert_eq!(cache_split(&result), (0, 0));
        assert!(
            !guard.path().join(CACHE_DIRECTORY).exists(),
            "an empty cache is still a file someone has to gitignore"
        );
    }
}
