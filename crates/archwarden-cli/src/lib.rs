//! `archwarden`'s command line, as a library.
//!
//! The binary is a four-line shim over [`run`]. Everything that decides
//! anything lives here so it can be tested without spawning a process.

pub mod apply;
pub mod batch;
pub mod changed;
// The baseline and the filters are operations, not presentation: a committed
// record of accepted debt and a decision about what to print are both things
// MCP and an LSP have to make the same way `check` does. Issue #63 moved them
// to archwarden-api; re-exported here so `crate::baseline` and `crate::filter`
// still name them at the fifty-odd call sites that use them.
pub use archwarden_api::{baseline, filter};

pub mod coverage;
pub mod describe;
pub mod diagnostic;
pub mod doctor;
pub mod exit;
pub mod explain;
pub mod guide;
pub mod hook;
pub mod hooks;
pub mod html;
pub mod impact;
pub mod locate;
pub mod matrix;
pub mod orphans;
pub mod phrases;
pub mod report;
pub mod respecify;
pub mod scaffold;
pub mod schema;
pub mod verify;

// A type this passes through and a filename `init` writes, where this once
// reached for `compile`, `extends` and `PresetResolver` to assemble a
// configuration by hand. Issue #63 moved the assembly into archwarden-api;
// the shrinking import list is the boundary holding.
use archwarden_config::{discovery, extends::MergedConfig};
use camino::{Utf8Path, Utf8PathBuf};
use clap::{Parser, Subcommand};

use crate::{diagnostic::ConfigDiagnostic, exit::Exit, report::Format};

/// A fast, declarative architecture linter for TypeScript and JavaScript.
#[derive(Debug, Parser)]
#[command(name = "archwarden", version, about, long_about = None)]
pub struct Cli {
    /// Path to `arch.config.json`. Overrides the upward search.
    ///
    /// A config outside the repository needs `--root` beside it, because a
    /// config file's own directory is otherwise taken to be the repository.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<Utf8PathBuf>,

    /// The repository to analyse. Overrides where the config says to look.
    ///
    /// `--config` normally answers this too: globs resolve from the config
    /// file's directory. That is right for the config a repository carries and
    /// wrong for one kept anywhere else — which is the shape of the question
    /// "how many findings would this stricter rule produce?", asked without
    /// editing the file the project committed.
    #[arg(long, global = true, value_name = "PATH")]
    pub root: Option<Utf8PathBuf>,

    /// The command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Where a command reads its rules from, and what it reads them against.
///
/// Defined by [`archwarden_api`] and re-exported, not restated. It is an
/// argument to the operations, so the crate that owns the operations owns it;
/// a second copy here would be a type to keep in step for no gain.
pub use archwarden_api::Location;

impl Cli {
    /// Where this invocation says to look.
    #[must_use]
    pub fn location(&self) -> Location<'_> {
        Location {
            config: self.config.as_deref(),
            root: self.root.as_deref(),
        }
    }
}

/// What the filters look like together.
///
/// Shown under `check --help`, because a flag nobody finds is a flag nobody
/// uses, and the useful shapes are the combinations rather than any one flag.
const CHECK_EXAMPLES: &str = "\
Examples:
  # what rule is dominating this output?
  archwarden check --summary

  # only the errors; the warnings are known debt
  archwarden check --level error

  # I just touched domain
  archwarden check --paths 'packages/domain/**'

  # both, and counted rather than listed
  archwarden check --summary --level error --paths 'packages/domain/**'

  # one rule at a time, while fixing it
  archwarden check --rules domain-entity-shape

Filters change what is shown, never what is checked. The exit code is 0 when
nothing failed, 1 when a rule did, and 2 when archwarden could not run.";

/// What a move looks like, from asking to doing.
const IMPACT_EXAMPLES: &str = "\
Examples:
  # what would this cost? (the default -- nothing is written)
  archwarden impact packages/domain/src/id/shared/is-id-invalid-shared.ts \\
             --to  packages/domain/src/id/calcs/is-id-invalid.ts

  # do it: git mv, and every import specifier rewritten
  archwarden impact packages/domain/src/id/shared/is-id-invalid-shared.ts \\
             --to  packages/domain/src/id/calcs/is-id-invalid.ts --apply

  # a whole layer at once, with the destination relative to each match
  archwarden impact 'packages/domain/src/*/shared' --to '../calcs' --apply

The spec sibling travels with its unit file. The exported symbol does not get
renamed -- that would break callers this cannot see, and `check` reports the
mismatch afterwards, which is where it belongs.";

/// The top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check the repository against its rules.
    ///
    /// The filters below decide what is *printed*. Every rule runs, every
    /// finding is computed, and the exit code is the same with them and
    /// without them — which is what makes one safe to leave in a CI command.
    #[command(after_long_help = CHECK_EXAMPLES)]
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

        /// The language the page is written in.
        ///
        /// Defaults to the config's `language`, and to English when it says
        /// nothing. Only the page: the terminal, the JSON and the digest stay
        /// in English whatever this says — a CI log is pasted into an issue,
        /// searched for and read by an agent, and one whose language depends
        /// on who ran it is worse than one somebody has to translate.
        #[arg(long, value_enum)]
        lang: Option<crate::phrases::Language>,

        /// Also write a page for a human, at this path.
        ///
        /// A side artefact rather than a `--format`, because a browser cannot
        /// read a pipe: the terminal keeps its summary and its exit code, and
        /// the page is written beside them. Read-only, self-contained, and it
        /// fetches nothing — it renders from a CI artefact years later.
        #[arg(long, value_name = "PATH")]
        html: Option<String>,

        /// Parse every file from source, reading and writing nothing.
        ///
        /// The escape hatch for a suspected cache bug: if a run disagrees with
        /// `--no-cache`, the cache is wrong and that is worth a report.
        #[arg(long)]
        no_cache: bool,

        /// Print per-rule counts instead of every finding.
        ///
        /// The answer to "what rule is dominating this output?", which on a
        /// first migration is a question about hundreds of lines. Rules that
        /// found nothing keep their row: that they were evaluated is an
        /// answer too.
        #[arg(long)]
        summary: bool,

        /// Show only findings from these rules.
        ///
        /// Repeatable, and comma-separated. These two are the same:
        ///
        /// `--rules domain-entity-shape,actions-need-spec`
        ///
        /// `--rules domain-entity-shape --rules actions-need-spec`
        ///
        /// Every rule still runs and the exit code is unchanged. An id no
        /// rule has is an error, not an empty report.
        #[arg(long, value_name = "ID", value_delimiter = ',')]
        rules: Vec<String>,

        /// Show only findings under these paths.
        ///
        /// Globs, repeatable and comma-separated, matched against the
        /// finding's path:
        ///
        /// `--paths 'packages/domain/**'`
        ///
        /// `--paths 'packages/domain/**,packages/application/**'`
        #[arg(long, value_name = "GLOB", value_delimiter = ',')]
        paths: Vec<String>,

        /// Show only findings of this level.
        ///
        /// `--level error` is the one to reach for when the warnings are
        /// known debt. They are still evaluated, still counted in
        /// `--summary`, and still not what fails the build.
        #[arg(long, value_enum, value_name = "LEVEL")]
        level: Option<LevelFilter>,

        /// Show only findings in files that differ from this ref.
        ///
        /// Without a ref, the working tree against `HEAD` — what you have not
        /// committed. With one, everything this branch does:
        /// `--changed main`. Untracked files count; ignored ones do not.
        ///
        /// A filter like the rest. Every rule still runs over the whole
        /// repository and the exit code is unchanged, so this shows you your
        /// own regressions without hiding anyone else's from the build.
        #[arg(long, value_name = "REF", num_args = 0..=1,
              default_missing_value = crate::changed::DEFAULT_REF)]
        changed: Option<String>,

        /// What `--summary` counts by.
        ///
        /// `rule` answers "what is dominating this output". `path` answers
        /// "which part of the repository is furthest from the rules", which is
        /// the one that says where to start. Passing it implies `--summary`,
        /// since counting by area only makes sense as counts.
        ///
        /// The areas are the directories the rules' own scopes select, so a
        /// config saying `roots: packages/domain/src/*` gets one row per
        /// module without anyone choosing a depth.
        #[arg(long, value_enum, value_name = "AXIS")]
        by: Option<By>,

        /// Report every finding, including the ones the baseline accepts.
        ///
        /// "How bad is it really" is a fair question, and answering it should
        /// not mean deleting a committed file.
        #[arg(long)]
        no_baseline: bool,
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

        /// The language, for `--format html` only. See `check --lang`.
        #[arg(long, value_enum)]
        lang: Option<crate::phrases::Language>,

        /// Restrict the digest to rules that can fire under this directory.
        #[arg(long, value_name = "PATH")]
        scope: Option<String>,

        /// Restrict the digest to rules of these kinds.
        ///
        /// Repeatable and comma-separated. Composes with `--scope`, so
        /// `--scope packages/domain --kind import-boundary` answers "the
        /// import boundaries that affect this directory" in one question.
        ///
        /// A kind no rule type has is an error, not an empty digest.
        #[arg(long, value_name = "KIND", value_delimiter = ',')]
        kind: Vec<String>,
    },

    /// Answer a harness's pre-write question, reading the event from stdin.
    ///
    /// Installed by `install-hooks`; not usually run by hand.
    Hook {
        /// Which harness's protocol to speak.
        #[arg(value_enum)]
        harness: Harness,
    },

    /// Serve the operations over MCP, speaking JSON-RPC on stdin and stdout.
    ///
    /// Not usually run by hand: a client spawns it and speaks to its pipes.
    /// A `.mcp.json` at the repository root names the command, and `check_write`
    /// is the tool that earns it — the same judgement the pre-write hook makes,
    /// asked *before* the write instead of after.
    Mcp,

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

    /// Say what moving a file would change, without moving it.
    ///
    /// An editor moves a file and rewrites its imports, and says nothing about
    /// whether the destination is somewhere the architecture allows the file
    /// to be, or whether the move puts an existing import across a boundary.
    /// That half is this.
    ///
    /// Resolves the whole repository, which costs about what a `check` costs.
    #[command(after_long_help = IMPACT_EXAMPLES)]
    Impact {
        /// The file to move, or a directory or glob of files.
        ///
        /// A glob makes `--to` relative to each match, because a refactor of
        /// an architecture is never one file.
        #[arg(value_name = "PATH")]
        path: String,

        /// Where it would go.
        ///
        /// A full path when the source is one file. A relative one when the
        /// source is a directory or a glob, applied to each match:
        /// `--to ../calcs` moves every file up one and into `calcs`.
        #[arg(long, value_name = "PATH")]
        to: String,

        /// Carry the move out, instead of saying what it would cost.
        ///
        /// Moves the files with `git mv` so history follows them, and rewrites
        /// every import specifier that named them — including the ones written
        /// by package name, which an editor leaves alone.
        ///
        /// Refuses on a dirty working tree, because `git` is the undo. Refuses
        /// on a specifier it cannot recompute, because half a refactor is
        /// worse than none. Everything is worked out before a byte is written,
        /// so a refusal means nothing has happened.
        #[arg(long)]
        apply: bool,

        /// Proceed despite a dynamic import nothing can read.
        ///
        /// The only refusal a flag may override: whether such a file imports
        /// the target is unknowable, so this is a human saying they looked.
        /// The report prints the line to look at.
        #[arg(long, requires = "apply")]
        force: bool,

        /// How to render the answer.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },

    /// Say where each folder's importers come from.
    ///
    /// For every file: who imports it, and whether from inside the module it
    /// lives in, from outside it, or nobody. Aggregated by folder.
    ///
    /// The question it answers is whether a folder has a reason to exist. A
    /// folder used only from outside its module is a boundary drawn in the
    /// wrong place; one used only from inside should be private.
    ///
    /// Not unused-export detection — that is Knip's. The interest here is
    /// where the importers come from for the exports that *are* used.
    ///
    /// Resolves the whole repository, which costs about what a `check` costs.
    Orphans {
        /// Only folders under this path or glob.
        #[arg(value_name = "PATH")]
        path: Option<String>,

        /// List every file as well as the folder totals.
        #[arg(long)]
        by_file: bool,

        /// How to render the answer.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },

    /// Accept every finding this repository has right now.
    ///
    /// Writes `.archwarden/baseline.json`, which is meant to be committed.
    /// `check` then reports only findings that are not in it, so a repository
    /// adopting archwarden can gate on new violations from day one instead of
    /// on debt nobody has decided about yet.
    ///
    /// Unlike the filters on `check`, this changes the exit code. That is what
    /// it is for, and why it is a file in the repository rather than a flag: a
    /// line added to it is a decision, visible in a pull request.
    Baseline {
        /// Say what regenerating would change, and write nothing.
        ///
        /// A reviewer looking at a regenerated baseline has one question the
        /// count cannot answer: was debt paid, or was debt added? Accepting a
        /// new finding by accident is permanent and silent, which makes it
        /// the worst thing this file can do.
        ///
        /// Findings that only changed path are reported as moved rather than
        /// as a removal and an addition, so a refactor that shifted a
        /// directory stays one line instead of two per finding.
        #[arg(long)]
        dry_run: bool,
    },

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

    /// Prove that each rule bites: hand it a violation and see whether it
    /// fires.
    ///
    /// `explain` says what a rule *reaches*. This says whether it *catches*
    /// anything — a rule can be schema-valid, cover the right paths, appear in
    /// `explain` and still enforce nothing.
    ///
    /// Nothing is written to the repository. Exits non-zero when a rule was
    /// handed a violation and said nothing.
    ///
    /// It proves a rule fires on a violation of its own terms, and cannot know
    /// what you meant: a `forbid_import_from_packages` list missing an entry
    /// is a question about intent, and ticks here.
    VerifyRules {
        /// How to render the verdicts.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },

    /// Say which files no rule governs, grouped by directory.
    ///
    /// Every other question here is asked per rule. This one is asked per
    /// file, and it is the only one that can answer "what is nobody
    /// watching?" — a file no rule mentions appears in no rule's answer, and
    /// `check` reporting `0 errors` over it reads exactly like a file that
    /// satisfies everything.
    Coverage {
        /// How to render the report.
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

/// What `--summary` counts by, as a command-line value.
///
/// Here for the same reason [`LevelFilter`] is: this is the enum carrying
/// clap's `ValueEnum`, and `archwarden_api::present` takes its own
/// [`archwarden_api::Axis`]. One step between the word and the decision, in
/// the surface that has the word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum By {
    /// One row per rule: what is dominating this output.
    #[default]
    Rule,
    /// One row per area of the repository: where to start.
    Path,
}

impl By {
    /// The axis this names.
    #[must_use]
    pub fn axis(self) -> archwarden_api::Axis {
        match self {
            Self::Rule => archwarden_api::Axis::Rule,
            Self::Path => archwarden_api::Axis::Path,
        }
    }
}

/// Which level to show, as a command-line value.
///
/// Here rather than beside the filter it feeds, and for the reason that kept
/// it out of `archwarden-core` before: this is the enum with clap's
/// `ValueEnum` on it, and the crate holding the operations should no more
/// learn about a command line than the core should. `archwarden_api::filter`
/// takes the core's own [`archwarden_core::level::Level`]; this is the word
/// a user types, and [`LevelFilter::level`] is the one step between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LevelFilter {
    /// Only errors.
    Error,
    /// Only warnings.
    Warning,
}

impl LevelFilter {
    /// The severity this names.
    #[must_use]
    pub fn level(self) -> archwarden_core::level::Level {
        match self {
            Self::Error => archwarden_core::level::Level::Error,
            Self::Warning => archwarden_core::level::Level::Warning,
        }
    }
}

/// A harness archwarden can speak the hook protocol of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Harness {
    /// Claude Code's `PreToolUse` protocol.
    ClaudeCode,
}

/// Whether `impact` is asking or doing.
///
/// One value rather than two booleans in a row, which is the shape that lets a
/// call site pass them the wrong way round.
#[derive(Debug, Clone, Copy)]
pub struct Mode {
    /// Carry the move out rather than describe it.
    pub apply: bool,
    /// Proceed despite a dynamic import nothing can read.
    pub force: bool,
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
        } => check_one(cli.location(), working_directory, file, *format, output),
        Command::Check {
            format,
            html,
            lang,
            no_cache,
            summary,
            rules,
            paths,
            level,
            changed,
            no_baseline,
            by,
            ..
        } => check(
            cli.location(),
            working_directory,
            &CheckOptions {
                format: *format,
                html: html.as_deref(),
                language: *lang,
                no_cache: *no_cache,
                summary: *summary,
                rules,
                paths,
                changed: changed.as_deref(),
                level: *level,
                no_baseline: *no_baseline,
                by: *by,
            },
            output,
        ),
        Command::Describe { path, format } => {
            describe(cli.location(), working_directory, path, *format, output)
        }
        Command::Scaffold { path, format } => {
            scaffold(cli.location(), working_directory, path, *format, output)
        }
        Command::AgentGuide {
            format,
            lang,
            scope,
            kind,
        } => agent_guide(
            cli.location(),
            working_directory,
            *format,
            *lang,
            scope.as_deref(),
            kind,
            output,
        ),
        Command::Init => init(working_directory, output),
        Command::Baseline { dry_run } => {
            write_baseline(cli.location(), working_directory, *dry_run, output)
        }
        Command::Impact {
            path,
            to,
            apply,
            force,
            format,
        } => impact(
            cli.location(),
            working_directory,
            path,
            to,
            Mode {
                apply: *apply,
                force: *force,
            },
            *format,
            output,
        ),
        Command::Orphans {
            path,
            by_file,
            format,
        } => orphans(
            cli.location(),
            working_directory,
            path.as_deref(),
            *by_file,
            *format,
            output,
        ),
        Command::Hook { .. } | Command::Mcp | Command::InstallHooks { .. } => {
            run_harness(&cli.command, cli.location(), working_directory, output)
        }
        Command::Config { command } => {
            run_config(command, cli.location(), working_directory, output)
        }
    }
}

/// The harness family: the three commands a coding agent's tooling runs.
///
/// Its own function rather than three more arms, on the same argument
/// [`run_config`] is extracted under — and because these three belong together
/// for a second reason: they are the surfaces of `AGENT-INTEGRATION.md`, and a
/// fourth would go here rather than into a dispatch that is already long.
fn run_harness(
    command: &Command,
    location: Location<'_>,
    working_directory: &Utf8Path,
    output: &mut Output<'_>,
) -> Exit {
    match command {
        Command::Hook { harness } => hook(*harness, location, working_directory, output),
        Command::Mcp => mcp(working_directory, output),
        Command::InstallHooks {
            claude_code,
            remove,
        } => install_hooks(*claude_code, *remove, working_directory, output),
        // Unreachable by construction: `run` sends only the three above. A
        // match arm rather than a panic, because a command routed here by
        // mistake should do nothing rather than take the process down.
        _ => Exit::Clean,
    }
}

/// The `config` family: four questions about the configuration itself.
///
/// Its own function rather than a fourth arm, because the dispatch above is
/// long enough that a reader looking for one command should not have to scroll
/// past three sub-arms of another.
fn run_config(
    command: &ConfigCommand,
    location: Location<'_>,
    working_directory: &Utf8Path,
    output: &mut Output<'_>,
) -> Exit {
    match command {
        ConfigCommand::Validate => validate(location, working_directory, output),
        ConfigCommand::Doctor { format } => doctor(location, working_directory, *format, output),
        ConfigCommand::VerifyRules { format } => {
            verify_rules(location, working_directory, *format, output)
        }
        ConfigCommand::Coverage { format } => {
            coverage(location, working_directory, *format, output)
        }
        ConfigCommand::Explain { rule_id, format } => {
            explain(location, working_directory, rule_id, *format, output)
        }
    }
}

/// Loads, merges and compiles a configuration, rendering any failure.
///
/// The orchestration itself lives in [`archwarden_api`] and returns its
/// failures as values. What is left here is the half that is genuinely the
/// CLI's: turning one of those values into a miette report on stderr and exit
/// code 2. That split is issue #63 — before it, the two were one function, and
/// every surface that reports failure differently had to re-implement the path
/// to change the shape of an error rather than reuse it.
///
/// Keeps its tuple return so the eleven callers below are unchanged.
fn prepare(
    location: Location<'_>,
    working_directory: &Utf8Path,
    output: &mut Output<'_>,
) -> Result<(MergedConfig, archwarden_core::compiled::CompiledConfig), Exit> {
    let prepared = archwarden_api::prepare(location, working_directory).map_err(|error| {
        let report = miette::Report::new(ConfigDiagnostic::from_api_error(&error));
        let _ = writeln!(output.err, "{report:?}");
        Exit::ConfigProblem
    })?;

    Ok((prepared.merged, prepared.compiled))
}

/// Says what the rules require of one path.
///
/// Reads no file and parses nothing: every rule's `describe_expectation` is
/// purely lexical, which is what lets this answer about a path that does not
/// exist yet. Exit is clean even when nothing applies -- a query that found no
/// rules is not a failure, and an agent branching on the exit code should see
/// "your setup is wrong" only when it is.
fn describe(
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

    // A glob asks about an area rather than a path, which is a different
    // question with a different answer shape. Detected the same way `--paths`
    // does it, so one convention covers both.
    if crate::filter::looks_like_a_glob(argument) {
        return describe_many(
            &merged.root,
            working_directory,
            &compiled,
            argument,
            format,
            output,
        );
    }

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

    let applies = archwarden_api::describe::describe(&compiled, &path);
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

/// Says what moving a file would change.
fn impact(
    location: Location<'_>,
    working_directory: &Utf8Path,
    argument: &str,
    destination: &str,
    mode: Mode,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let Mode { apply, force } = mode;
    let (merged, compiled) = match prepare(location, working_directory, output) {
        Ok(prepared) => prepared,
        Err(exit) => return exit,
    };

    let tree = match walked(&merged.root, working_directory, &compiled, output) {
        Ok(tree) => tree,
        Err(exit) => return exit,
    };

    // A glob or a directory is a batch, and `--to` is then relative to each
    // match. One file keeps the original meaning: `--to` is where it goes.
    let requests = match crate::batch::expand(
        &merged.root,
        working_directory,
        &tree,
        argument,
        destination,
    ) {
        Ok(requests) => requests,
        Err(message) => {
            let _ = writeln!(output.err, "{message}");
            return Exit::ConfigProblem;
        }
    };

    if requests.is_empty() {
        // Never an empty report: a source matching nothing looks exactly like
        // a move with no consequences, which is the one wrong answer a reader
        // takes as good news. The same judgement `--rules` makes about an
        // unknown id.
        let _ = writeln!(output.err, "× `{argument}` matches no file.");
        return Exit::ConfigProblem;
    }

    if apply {
        return carry_out_moves(&merged.root, &compiled, &tree, &requests, force, output);
    }

    let sources: Vec<_> = requests.iter().map(|(from, _)| from.clone()).collect();
    let found =
        archwarden_engine::importers::importers_of_each(&merged.root, &compiled, &tree, &sources);

    let answers: Vec<crate::impact::Impact> = requests
        .iter()
        .map(|(from, to)| {
            let importers = found.get(from).cloned().unwrap_or_default();
            let relative = archwarden_engine::importers::relative_imports(&merged.root, from);
            crate::impact::impact(&compiled, from, to, &importers, relative)
        })
        .collect();

    crate::impact::render_all(&answers, format, output.out);
    Exit::Clean
}

/// Carries out a move, having said what it would do.
///
/// The plan is computed and validated in full before anything is written, so
/// every refusal below happens with the repository untouched.
fn carry_out_moves(
    root: &Utf8Path,
    compiled: &archwarden_core::compiled::CompiledConfig,
    tree: &archwarden_engine::walk::RepoTree,
    requests: &[(
        archwarden_core::path::RepoRelPath,
        archwarden_core::path::RepoRelPath,
    )],
    force: bool,
    output: &mut Output<'_>,
) -> Exit {
    let markers = crate::batch::spec_markers(compiled);
    let plan = crate::apply::plan(root, compiled, tree, requests, &markers);

    if !plan.is_actionable(force) {
        crate::apply::render_refusals(&plan, force, output.err);
        return Exit::ConfigProblem;
    }

    if let Err(message) = crate::apply::carry_out(root, &plan) {
        let _ = writeln!(output.err, "× {message}");
        return Exit::ConfigProblem;
    }

    crate::apply::render_done(&plan, output.out);
    Exit::Clean
}

/// Says where each folder's importers come from.
fn orphans(
    location: Location<'_>,
    working_directory: &Utf8Path,
    scope: Option<&str>,
    by_file: bool,
    format: Format,
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

    let index = archwarden_engine::importers::reverse_index(&merged.root, &compiled, &tree);
    let mut answer = crate::orphans::orphans(&compiled, &index, by_file);

    if let Some(scope) = scope {
        // The same matcher `--paths` uses, so a plain path selects it and
        // everything under it and a glob is used exactly as written. One
        // convention for narrowing, across every command that narrows.
        let set = match crate::filter::path_set(std::slice::from_ref(&scope.to_owned())) {
            Ok(set) => set,
            Err(message) => {
                let _ = writeln!(output.err, "{message}");
                return Exit::ConfigProblem;
            }
        };
        answer.retain(&set);

        if answer.folders.is_empty() {
            // Never an empty report for a scope that matched nothing: it would
            // read as a repository with no folders worth looking at.
            let _ = writeln!(output.err, "× `{scope}` matches no source file.");
            return Exit::ConfigProblem;
        }
    }

    crate::orphans::render(&answer, by_file, format, output.out);
    Exit::Clean
}

/// Accepts every finding this repository has right now.
///
/// Runs a full check and writes what it found. Deliberately not incremental:
/// a baseline that accepted only part of a run would be a promise the file
/// does not keep.
fn write_baseline(
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
        return report_baseline_changes(&merged.root, &path, &baseline, output);
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
fn report_baseline_changes(
    root: &Utf8Path,
    path: &Utf8Path,
    next: &crate::baseline::Baseline,
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

fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
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
fn ignore_the_cache(working_directory: &Utf8Path) -> bool {
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
fn hook(
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
fn stopped(location: Location<'_>, working_directory: &Utf8Path, output: &mut Output<'_>) -> Exit {
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

fn allow(output: &mut Output<'_>) -> Exit {
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
fn unable(output: &mut Output<'_>, reason: &str) -> Exit {
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
fn session_started(
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

/// Serves MCP until the client closes the pipe.
///
/// Everything this does is in `archwarden-mcp`, which cannot see this crate.
/// What is left here is the wiring a binary owns: buffering stdin, and turning
/// a client that went away into an exit code rather than a panic.
///
/// It exits clean when the pipe closes, because that is how a stdio server is
/// stopped — the client kills it at the end of the session, and reporting that
/// as a failure would put an error in the user's log every time they quit.
fn mcp(working_directory: &Utf8Path, output: &mut Output<'_>) -> Exit {
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
fn caveat(invocation: &str, in_container: bool, output: &mut Output<'_>) {
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
fn apply(
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

fn describe_mcp_outcome(outcome: crate::hooks::Outcome, config: &Utf8Path) -> String {
    match outcome {
        crate::hooks::Outcome::Installed => format!("installed the MCP server in {config}"),
        crate::hooks::Outcome::AlreadyInstalled => {
            format!("the MCP server is already in {config}")
        }
        crate::hooks::Outcome::Removed => format!("removed the MCP server from {config}"),
        crate::hooks::Outcome::NotInstalled => format!("no archwarden server was in {config}"),
    }
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
fn describe_many(
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
fn scaffold(
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
fn agent_guide(
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

    let guide = archwarden_api::guide::guide(&compiled, scope.as_ref(), kinds);
    // The flag wins over the config; the config over English. A repository
    // decides this once, and one run may want the other.
    let language = language.unwrap_or_else(|| crate::phrases::Language::of(merged.config.language));
    crate::guide::render(&guide, format, language, output.out);
    Exit::Clean
}

/// What `check` was asked to do.
///
/// A struct because the four filters plus the two switches are six arguments,
/// and six positional booleans and slices at a call site is a place transposed
/// arguments go to hide.
struct CheckOptions<'a> {
    format: Format,
    html: Option<&'a str>,
    language: Option<crate::phrases::Language>,
    no_cache: bool,
    summary: bool,
    rules: &'a [String],
    paths: &'a [String],
    changed: Option<&'a str>,
    level: Option<LevelFilter>,
    no_baseline: bool,
    by: Option<By>,
}

#[allow(
    clippy::too_many_lines,
    reason = "one command, in the order it happens: load, walk, run, filter \
              against the baseline, render, write the page, decide the exit \
              code. Splitting it would hide that the exit code is taken from \
              what the baseline did not accept and never from what was shown"
)]
fn check(
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

    crate::report::render(
        &crate::report::Rendered {
            root: &merged.root,
            report: &outcome,
            view: &presented.view,
            reasons: &crate::report::Reasons::of(&compiled),
            elapsed: started.elapsed(),
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
                let _ = writeln!(output.out, "page written to {destination}");
            }
            // Reported and not fatal: the gate already ran, and refusing its
            // exit code because a side artefact could not be written would let
            // a full disk turn a failing build green.
            Err(error) => {
                let _ = writeln!(output.err, "note: cannot write {destination}: {error}");
            }
        }
    }

    if let Some(baseline) = &baseline {
        report_standing(baseline, &outcome.findings, output);
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

/// How this run stands against the baseline.
///
/// Printed on every run that has one, deliberately. A baseline nobody is
/// reminded of is a suppression file, and the entries that no longer occur are
/// the only cheerful number archwarden has -- as well as the thing that stops
/// a stale entry hiding a violation that came back.
fn report_standing(
    baseline: &crate::baseline::Baseline,
    findings: &[archwarden_core::finding::Finding],
    output: &mut Output<'_>,
) {
    let standing = baseline.standing(findings);
    let _ = write!(output.out, "{} accepted", standing.accepted);

    if standing.gone > 0 {
        let _ = write!(
            output.out,
            ", {} no longer {} — run `archwarden baseline` to update",
            standing.gone,
            if standing.gone == 1 {
                "occurs"
            } else {
                "occur"
            }
        );
    }

    let _ = writeln!(output.out);
}

/// Walks the repository, rendering a refusal as this surface says it.
///
/// The walk and the refusal itself are [`archwarden_api::walk`], including why
/// the refusal is narrow. What is left here is the rendering — and the help,
/// which is the CLI's alone: it names `--root`, and a surface with no command
/// line needs a different sentence for the same fact.
fn walked(
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
fn verify_rules(
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
fn coverage(
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

fn doctor(
    location: Location<'_>,
    working_directory: &Utf8Path,
    format: Format,
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
    Exit::Clean
}

/// Shows what one rule reaches, and what it is reporting.
fn explain(
    location: Location<'_>,
    working_directory: &Utf8Path,
    rule_id: &str,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
    let Ok((merged, compiled)) = prepare(location, working_directory, output) else {
        return Exit::ConfigProblem;
    };

    let id = match archwarden_core::ids::RuleId::new(rule_id) {
        Ok(id) => id,
        Err(error) => {
            let _ = writeln!(output.err, "`{rule_id}` is not a rule id: {error}");
            return Exit::ConfigProblem;
        }
    };

    let tree = match walked(&merged.root, working_directory, &compiled, output) {
        Ok(tree) => tree,
        Err(exit) => return exit,
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

fn validate(location: Location<'_>, working_directory: &Utf8Path, output: &mut Output<'_>) -> Exit {
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

    /// Two rules of different levels over two packages, which is the smallest
    /// repository where every filter has something to do.
    const FILTERABLE: &str = r#"{"version":0,"rules":[
        {"type":"structure","id":"domain-shape","level":"error",
         "roots":"packages/domain/src/*","allowed_subfolders":["types"]},
        {"type":"structure","id":"app-shape","level":"warning",
         "roots":"packages/app/src/*","allowed_subfolders":["use-cases"]}]}"#;

    /// A tree that breaks both rules, once each.
    fn filterable() -> Vec<(&'static str, &'static str)> {
        vec![
            ("arch.config.json", FILTERABLE),
            (
                "packages/domain/src/order/handlers/a.ts",
                "export const a=1;",
            ),
            (
                "packages/app/src/billing/controllers/b.ts",
                "export const b=1;",
            ),
        ]
    }

    // --- baseline ---------------------------------------------------------

    /// The day-one problem: a repository adopting archwarden inherits
    /// violations nobody has decided about, so the build is red before anyone
    /// has done anything wrong.
    #[test]
    fn a_baseline_makes_inherited_debt_stop_failing_the_build() {
        let (guard, before) = run_in(&filterable(), &["check"]);
        assert_eq!(before.exit, Exit::Errors);
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");

        let written = run_at(&root, &["baseline"]);
        assert_eq!(written.exit, Exit::Clean);
        assert!(written.out.contains("2 findings"), "{}", written.out);

        let after = run_at(&root, &["check"]);
        assert_eq!(after.exit, Exit::Clean, "{}", after.out);
        assert!(after.out.contains("2 accepted"), "{}", after.out);
    }

    /// And a new violation still fails, which is the whole reason the previous
    /// test is not just `--level error` with extra steps.
    #[test]
    fn a_new_violation_fails_through_a_baseline() {
        let (guard, _) = run_in(&filterable(), &["check"]);
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        run_at(&root, &["baseline"]);

        std::fs::create_dir_all(guard.path().join("packages/domain/src/order/repositories"))
            .expect("create dirs");
        std::fs::write(
            guard
                .path()
                .join("packages/domain/src/order/repositories/a.ts"),
            "export const a = 1;",
        )
        .expect("write");

        let after = run_at(&root, &["check"]);

        assert_eq!(after.exit, Exit::Errors, "{}", after.out);
        assert!(after.out.contains("repositories"), "{}", after.out);
        assert!(
            !after.out.contains("handlers"),
            "the accepted one stays quiet: {}",
            after.out
        );
    }

    /// The ratchet. Fixing accepted debt is reported, and the entry named as
    /// removable -- without which reintroducing it later would be hidden by
    /// the stale entry.
    #[test]
    fn fixing_accepted_debt_is_reported() {
        let (guard, _) = run_in(&filterable(), &["check"]);
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        run_at(&root, &["baseline"]);

        std::fs::remove_dir_all(guard.path().join("packages/app/src/billing/controllers"))
            .expect("remove");

        let after = run_at(&root, &["check"]);

        assert_eq!(after.exit, Exit::Clean);
        assert!(after.out.contains("1 accepted"), "{}", after.out);
        assert!(after.out.contains("1 no longer occurs"), "{}", after.out);
        assert!(after.out.contains("archwarden baseline"), "{}", after.out);
    }

    /// The escape hatch. "How bad is it really" is a fair question and the
    /// answer must not require deleting a committed file.
    #[test]
    fn no_baseline_shows_everything_again() {
        let (guard, _) = run_in(&filterable(), &["check"]);
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        run_at(&root, &["baseline"]);

        let full = run_at(&root, &["check", "--no-baseline"]);

        assert_eq!(full.exit, Exit::Errors);
        assert!(full.out.contains("handlers"), "{}", full.out);
        assert!(!full.out.contains("accepted"), "{}", full.out);
    }

    /// The pre-write hook has to respect it too. An agent editing a legacy
    /// file would otherwise be blocked by debt that is not its own, and would
    /// have the hook uninstalled by lunchtime.
    ///
    /// Through `hook claude-code`, not through `check --file`. The first
    /// version of this test used the latter, passed, and the hook went on
    /// denying writes -- they are separate code paths, and testing the
    /// neighbour of the thing is not testing the thing.
    #[test]
    fn the_hook_respects_the_baseline() {
        let (guard, _) = run_in(&filterable(), &["check"]);
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        run_at(&root, &["baseline"]);

        // `check --file` too, since both answer the same question.
        let checked = run_at(
            &root,
            &["check", "--file", "packages/domain/src/order/handlers/a.ts"],
        );
        assert_eq!(checked.exit, Exit::Clean, "{}", checked.out);

        let event = format!(
            r#"{{"tool_name":"Write","tool_input":{{"file_path":"{root}/packages/domain/src/order/handlers/a.ts","content":"x"}}}}"#
        );
        let hooked = run_with(&root, &["hook", "claude-code"], &event);

        assert_eq!(
            hooked.out.trim(),
            "{}",
            "accepted debt does not block a write: {}",
            hooked.out
        );
    }

    /// A baseline on a clean repository is an empty one, not an error.
    #[test]
    fn a_clean_repository_writes_an_empty_baseline() {
        let (guard, result) = run_in(&[("arch.config.json", MINIMAL)], &["baseline"]);

        assert_eq!(result.exit, Exit::Clean);
        assert!(result.out.contains("nothing"), "{}", result.out);
        assert!(
            guard.path().join(".archwarden/baseline.json").exists(),
            "the file is still written, so `check` has something to read"
        );
    }

    /// The invariant the whole feature rests on. A filter narrows what is
    /// printed; if it could also narrow what fails, then `--rules` in a CI
    /// command would quietly turn a failing build green, and nobody would
    /// find out until something broke in production.
    #[test]
    fn no_filter_can_change_the_exit_code() {
        let (guard, unfiltered) = run_in(&filterable(), &["check"]);
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        assert_eq!(unfiltered.exit, Exit::Errors);

        for narrowing in [
            vec!["check", "--rules", "app-shape"],
            vec!["check", "--level", "warning"],
            vec!["check", "--paths", "packages/app/**"],
            vec!["check", "--summary", "--rules", "app-shape"],
            // The narrowest possible: a rule that fired, a path it did not
            // fire on, and the wrong level. Nothing survives to be printed.
            vec![
                "check",
                "--rules",
                "app-shape",
                "--paths",
                "packages/domain/**",
                "--level",
                "error",
            ],
        ] {
            let filtered = run_at(&root, &narrowing);
            assert_eq!(
                filtered.exit,
                Exit::Errors,
                "{narrowing:?} changed the gate:\n{}",
                filtered.out
            );
        }
    }

    /// And when a filter hides everything, the report says so rather than
    /// leaving `0 errors` next to exit 1 as a contradiction the reader cannot
    /// resolve.
    #[test]
    fn a_filter_that_hides_everything_admits_it() {
        let (_guard, result) = run_in(
            &filterable(),
            &["check", "--rules", "domain-shape", "--level", "warning"],
        );

        assert_eq!(result.exit, Exit::Errors);
        assert!(
            result.out.contains("0 errors, 0 warnings"),
            "{}",
            result.out
        );
        assert!(
            result.out.contains("note: 2 findings hidden"),
            "{}",
            result.out
        );
    }

    /// A mistyped rule id fails where the user is looking. Printing nothing
    /// and exiting 0 would be indistinguishable from a clean repository --
    /// the one wrong answer that reads as good news.
    #[test]
    fn an_unknown_rule_id_stops_the_run() {
        let (_guard, result) = run_in(&filterable(), &["check", "--rules", "domain-shpe"]);

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(
            result.err.contains("no rule is called `domain-shpe`"),
            "{}",
            result.err
        );
        assert!(result.err.contains("`app-shape`"), "{}", result.err);
    }

    #[test]
    fn a_malformed_glob_stops_the_run() {
        let (_guard, result) = run_in(&filterable(), &["check", "--paths", "packages/["]);

        assert_eq!(result.exit, Exit::ConfigProblem);
        assert!(result.err.contains("invalid glob"), "{}", result.err);
    }

    /// `--rules` names rules, so it narrows the rows. `--paths` and `--level`
    /// do not, so every rule keeps its row and answers with a zero -- which is
    /// the answer, and reads differently from a rule that is not there.
    #[test]
    fn only_naming_rules_narrows_the_breakdown() {
        let (guard, named) = run_in(
            &filterable(),
            &["check", "--summary", "--rules", "app-shape"],
        );
        assert!(named.out.contains("app-shape"), "{}", named.out);
        assert!(
            !named.out.contains("domain-shape"),
            "a rule the user did not name: {}",
            named.out
        );

        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("UTF-8");
        let by_path = run_at(&root, &["check", "--summary", "--paths", "packages/app/**"]);
        assert!(by_path.out.contains("app-shape"), "{}", by_path.out);
        assert!(
            by_path.out.contains("domain-shape  0"),
            "a rule that found nothing here still says so: {}",
            by_path.out
        );
    }

    /// `--summary` in JSON drops the findings array. A summary that still
    /// emitted every finding would give a piping user no size benefit, which
    /// is the whole reason to reach for the flag.
    #[test]
    fn a_json_summary_is_counts_without_findings() {
        let (_guard, result) = run_in(&filterable(), &["check", "--summary", "--format", "json"]);
        let parsed: serde_json::Value =
            serde_json::from_str(&result.out).expect("the report is JSON");

        assert!(parsed.get("findings").is_none(), "{}", result.out);
        assert_eq!(parsed["summary"]["by_rule"]["domain-shape"]["errors"], 1);
        assert_eq!(parsed["summary"]["by_rule"]["app-shape"]["warnings"], 1);
        assert_eq!(parsed["summary"]["errors"], 1);
    }

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

    /// The question `--root` exists for: how many findings would a stricter
    /// rule produce, asked without editing the file the project committed.
    ///
    /// Rule 2 of `AGENTS.md` forbids editing `arch.config.json` to make a
    /// check pass, and planning to *tighten* a rule needs exactly that edit to
    /// measure it. A config kept somewhere else answers without persisting
    /// anything — but only if archwarden can be told the repository is not
    /// where that config sits.
    #[test]
    fn a_config_outside_the_repository_analyses_it_when_root_says_where_it_is() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("UTF-8");
        std::fs::create_dir_all(root.join("repo/src/wrong")).expect("create dirs");
        std::fs::create_dir_all(root.join("elsewhere")).expect("create dirs");
        std::fs::write(root.join("repo/src/wrong/x.ts"), "export const x = 1;\n").expect("write");
        std::fs::write(
            root.join("elsewhere/stricter.json"),
            r#"{"version":0,"rules":[{"type":"structure","id":"shape","level":"error",
                "roots":"src","allowed_subfolders":["right"]}]}"#,
        )
        .expect("write");

        let result = run_at(
            &root.join("repo"),
            &[
                "check",
                "--config",
                "../elsewhere/stricter.json",
                "--root",
                ".",
                "--summary",
            ],
        );

        assert_eq!(result.exit, Exit::Errors, "{}{}", result.out, result.err);
        assert!(result.out.contains("shape"), "{}", result.out);
    }

    /// The same config without `--root`, which is the shape of the bug.
    ///
    /// The root falls back to the config file's own directory, which holds no
    /// source at all. Reporting that as a clean repository would answer "how
    /// many findings?" with zero — the one wrong answer a reader takes as good
    /// news. Exit 2, and the message names the flag that fixes it.
    #[test]
    fn a_config_outside_the_repository_refuses_rather_than_reporting_a_clean_run() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("UTF-8");
        std::fs::create_dir_all(root.join("repo/src")).expect("create dirs");
        std::fs::create_dir_all(root.join("elsewhere")).expect("create dirs");
        std::fs::write(root.join("repo/src/x.ts"), "export const x = 1;\n").expect("write");
        std::fs::write(root.join("elsewhere/stricter.json"), MINIMAL).expect("write");

        let result = run_at(
            &root.join("repo"),
            &["check", "--config", "../elsewhere/stricter.json"],
        );

        assert_eq!(result.exit, Exit::ConfigProblem, "{}", result.out);
        assert!(result.err.contains("--root"), "{}", result.err);
    }

    /// A repository with no source yet is not the same mistake. `init` writes
    /// a config into an empty directory, and the very next `check` must not
    /// tell the user their setup is broken.
    #[test]
    fn an_empty_repository_you_are_standing_in_is_still_checked() {
        let (_guard, result) = run_in(&[("arch.config.json", MINIMAL)], &["check"]);

        assert_eq!(result.exit, Exit::Clean, "{}", result.err);
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
        let cache = guard.path().join(archwarden_api::CACHE_DIRECTORY);
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
            !guard.path().join(archwarden_api::CACHE_DIRECTORY).exists(),
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
        let database = root
            .join(archwarden_api::CACHE_DIRECTORY)
            .join(archwarden_api::CACHE_FILE);
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
        assert!(
            result
                .out
                .contains("`packages/ui/button.tsx`: `@org/never-installed`"),
            "and which one, or the note is a blind spot of its own: {}",
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
        assert!(
            parsed["unresolved_imports"]
                .as_array()
                .is_some_and(Vec::is_empty),
            "and the blind spots, for the same reason: {}",
            result.out
        );
    }

    /// A boundary rule that ran against an import nothing could place ran
    /// blind, and this command answered `is fine.` either way -- which is what
    /// a pre-write hook asks it, about the import the agent has just written.
    /// Issue #18.
    #[test]
    fn check_file_names_an_import_the_boundary_rules_did_not_see() {
        let files = [
            (
                "arch.config.json",
                r#"{"version":0,"rules":[{
                    "type":"import-boundary","id":"domain-is-self-contained","level":"error",
                    "from":"packages/domain/**","forbid_import_from":"apps/**"}]}"#,
            ),
            (
                "packages/domain/row.ts",
                "import type { Order } from '@Domain/Order/types';\nexport type Violation = Order;",
            ),
        ];

        let (_guard, result) = run_in(&files, &["check", "--file", "packages/domain/row.ts"]);

        assert_eq!(result.exit, Exit::Clean, "nothing was found -- nor seen");
        assert!(
            result
                .out
                .contains("note: `@Domain/Order/types` did not resolve"),
            "{}",
            result.out
        );
        assert!(
            !result.out.contains("is fine"),
            "it is not fine, it is unseen: {}",
            result.out
        );

        let (_guard, json) = run_in(
            &files,
            &[
                "check",
                "--file",
                "packages/domain/row.ts",
                "--format",
                "json",
            ],
        );
        let parsed: serde_json::Value = serde_json::from_str(&json.out).expect("valid JSON");
        assert_eq!(parsed["unresolved_imports"][0], "@Domain/Order/types");
    }

    /// The question a reviewer has about a regenerated baseline, which the
    /// count could not answer: was debt paid, or was debt added? Issue #23.
    #[test]
    fn a_baseline_dry_run_says_what_would_change_and_writes_nothing() {
        let structure = r#"{"version":0,"rules":[{
            "type":"structure","id":"entity-shape","level":"error",
            "roots":["src/*"],"allowed_subfolders":["types"]}]}"#;

        let (guard, accepted) = run_in(
            &[
                ("arch.config.json", structure),
                ("src/order/handlers/a.ts", ""),
            ],
            &["baseline"],
        );
        assert_eq!(accepted.exit, Exit::Clean);
        let root = Utf8PathBuf::from_path_buf(guard.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");

        // Then break something else and ask what regenerating would do.
        std::fs::create_dir_all(root.join("src/billing/handlers")).expect("create dirs");
        std::fs::write(root.join("src/billing/handlers/b.ts"), "").expect("write");
        let dry = run_at(&root, &["baseline", "--dry-run"]);

        assert_eq!(dry.exit, Exit::Clean, "it reports, it does not gate");
        assert!(
            dry.out.contains("+ entity-shape src/billing/handlers"),
            "the addition is the line that matters: {}",
            dry.out
        );
        assert!(dry.out.contains("Nothing was written."), "{}", dry.out);
        assert!(
            dry.out
                .contains("would become debt this project has decided to carry"),
            "{}",
            dry.out
        );

        // And it wrote nothing: the committed file still accepts only the one.
        let on_disk = std::fs::read_to_string(root.join(crate::baseline::BASELINE_PATH))
            .expect("the baseline is still there");
        assert!(on_disk.contains("src/order/handlers"), "{on_disk}");
        assert!(
            !on_disk.contains("src/billing/handlers"),
            "a dry run that wrote would be the bug it exists to prevent: {on_disk}"
        );
        drop(guard);
    }

    /// `explain` answers about coverage; this answers about efficacy. The
    /// second rule here covers the right files, appears in `explain`, and its
    /// own `except` cancels the thing it forbids -- which reads exactly like a
    /// repository that satisfies it. Issue #24.
    #[test]
    fn verify_rules_fails_on_a_rule_that_enforces_nothing() {
        let (_guard, result) = run_in(
            &[
                (
                    "arch.config.json",
                    r#"{"version":0,"rules":[
                        {"type":"import-boundary","id":"domain-is-self-contained","level":"error",
                         "from":"packages/domain/**","forbid_import_from":["apps/**"]},
                        {"type":"import-boundary","id":"cancelled-by-its-own-except","level":"error",
                         "from":"packages/domain/**","forbid_import_from":["apps/**"],
                         "except":["apps/**"]}]}"#,
                ),
                ("packages/domain/order.ts", "export const x = 1;"),
                ("apps/api/src/env.ts", "export const e = 1;"),
            ],
            &["config", "verify-rules"],
        );

        assert_eq!(result.exit, Exit::Errors, "{}", result.out);
        assert!(
            result.out.contains("✓ domain-is-self-contained — fires on"),
            "{}",
            result.out
        );
        assert!(
            result
                .out
                .contains("✗ cancelled-by-its-own-except — silent on"),
            "{}",
            result.out
        );
        // Said on every run, clean or not: a wall of ticks that let a reader
        // conclude their config is sound would be this issue one level up.
        assert!(
            result.out.contains("It cannot"),
            "the limitation is stated: {}",
            result.out
        );
    }

    /// And a rule whose violation cannot be synthesised is reported as
    /// unchecked rather than left out. A partial answer that says which part
    /// is missing beats a confident one that is wrong.
    #[test]
    fn verify_rules_names_what_it_could_not_check() {
        let (_guard, result) = run_in(
            &[
                ("arch.config.json", NAMING),
                (
                    "src/user/create-client.use-case.ts",
                    "export function CreateClient() {}",
                ),
            ],
            &["config", "verify-rules", "--format", "json"],
        );

        assert_eq!(result.exit, Exit::Clean, "nothing was proven silent");
        let parsed: serde_json::Value = serde_json::from_str(&result.out).expect("valid JSON");
        assert_eq!(parsed[0]["verdict"], "unverified");
        assert!(
            parsed[0]["reason"]
                .as_str()
                .is_some_and(|why| why.contains("file_pattern")),
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
    /// Every unexpected shape lets the write through — and says that it did,
    /// which is the half this used to get wrong.
    ///
    /// Permitting in silence made a gate that could not run look exactly like
    /// one that ran and approved. The write still goes through; the difference
    /// is that somebody can tell.
    #[test]
    fn the_hook_never_blocks_because_of_its_own_trouble() {
        // A config with no rules is deliberately not here. That is a working
        // gate over an empty rule set: the write was examined and nothing
        // objected, which is the one thing `{}` is supposed to mean. Whether a
        // config should constrain something is `config doctor`'s question, and
        // asking it again on every write would be noise.
        let cases: [(&str, &str); 3] = [
            ("a broken config", r#"{"version": 0,,}"#),
            ("a config for a future version", r#"{"version": 99}"#),
            (
                "an uncompilable rule",
                r#"{"version":0,"rules":[{"type":"structure",
                "id":"a","level":"error","roots":"["}]}"#,
            ),
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
            assert!(
                !result.out.contains("\"permissionDecision\""),
                "{what} should not block the write: {}",
                result.out
            );
            assert!(
                result.out.contains("did not check this write"),
                "{what} permitted the write without saying it went unchecked: {}",
                result.out
            );
        }
    }

    /// The invariant issue #55 broke, stated where it can fail the build.
    ///
    /// Two surfaces read the same config and answer in different shapes —
    /// `validate` with a miette report and exit 2, the hook with JSON and exit
    /// 0. What they must never disagree about is the *question underneath*:
    /// whether this configuration can gate anything at all.
    ///
    /// They did disagree. The hook carried its own copy of the orchestration,
    /// because the shared one wrote to stderr and the hook cannot answer that
    /// way, and the copy had no version guard. `{"version": 99}` made
    /// `validate` exit 2 and the hook reply `{}` — a gate that had evaporated,
    /// reporting the same silence as a gate that examined the write and
    /// approved it.
    ///
    /// Neither the exact prose nor the exit code is asserted here. Those are
    /// each surface's own business and are pinned elsewhere. This is about the
    /// one thing they may not decide separately.
    #[test]
    fn the_hook_and_validate_never_disagree_about_whether_a_config_is_usable() {
        let unusable: [(&str, &str); 5] = [
            ("a syntax error", r#"{"version": 0,,}"#),
            ("a future version", r#"{"version": 99}"#),
            ("an unknown field", r#"{"version":0,"rulez":[]}"#),
            (
                "an unresolvable preset",
                r#"{"version":0,"extends":"@org/not-installed"}"#,
            ),
            (
                "an uncompilable rule",
                r#"{"version":0,"rules":[{"type":"structure",
                 "id":"a","level":"error","roots":"["}]}"#,
            ),
        ];

        for (what, config) in unusable {
            let dir = tempfile::tempdir().expect("temp dir");
            let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
                .expect("UTF-8");
            std::fs::write(root.join("arch.config.json"), config).expect("write");

            let validated = run_at(&root, &["config", "validate"]);
            let hooked = run_with(&root, &["hook", "claude-code"], WRITE_EVENT);

            assert_eq!(
                validated.exit,
                Exit::ConfigProblem,
                "{what}: validate should refuse it"
            );
            assert!(
                hooked.out.contains("did not check this write"),
                "{what}: validate refused this config and the hook gated a write with it \
                 anyway: {}",
                hooked.out
            );
        }
    }

    /// And the other direction, which is the half that makes the test above
    /// mean something: a config both accept is one the hook actually used.
    /// Without this, a hook that reported every write unchecked would pass.
    #[test]
    fn a_config_both_accept_is_one_the_hook_gated_with() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("UTF-8");
        std::fs::write(root.join("arch.config.json"), NAMING).expect("write");

        let validated = run_at(&root, &["config", "validate"]);
        let hooked = run_with(&root, &["hook", "claude-code"], WRITE_EVENT);

        assert_eq!(validated.exit, Exit::Clean);
        assert!(
            !hooked.out.contains("did not check this write"),
            "validate accepted this config and the hook refused to use it: {}",
            hooked.out
        );
    }

    /// A config that constrains nothing still *checked* the write.
    ///
    /// The line this whole change is drawn along: "I examined it and had no
    /// objection" stays silent, "I could not examine it" does not. An empty
    /// rule set is the first of those, however little it enforces — and
    /// `config doctor` is where the question of whether it should enforce
    /// something belongs.
    #[test]
    fn a_config_with_no_rules_examined_the_write_and_says_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("UTF-8");
        std::fs::write(root.join("arch.config.json"), r#"{"version":0}"#).expect("write");

        let result = run_with(&root, &["hook", "claude-code"], WRITE_EVENT);

        assert_eq!(result.exit, Exit::Clean);
        assert_eq!(result.out, "{}\n");
    }

    /// A payload naming no file is not this hook's business, and it says
    /// nothing at all about it.
    ///
    /// The only silence left. With a matcher broader than `Write|Edit|
    /// MultiEdit` this is every `Bash` and every `Read`, and a remark on each
    /// one is a hook somebody removes.
    #[test]
    fn the_hook_passes_over_a_tool_that_writes_nothing_in_silence() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("UTF-8");
        std::fs::write(root.join("arch.config.json"), NAMING).expect("write");

        let result = run_with(
            &root,
            &["hook", "claude-code"],
            r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#,
        );

        assert_eq!(result.exit, Exit::Clean);
        assert_eq!(result.out, "{}\n");
    }

    /// And an event it cannot read at all is the other answer.
    ///
    /// `echo 'not json' | archwarden hook claude-code` permitted in silence,
    /// so a misconfigured hook was indistinguishable from a working one.
    #[test]
    fn the_hook_says_so_when_it_cannot_read_the_event() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("UTF-8");
        std::fs::write(root.join("arch.config.json"), NAMING).expect("write");

        let result = run_with(&root, &["hook", "claude-code"], "not json");

        assert_eq!(result.exit, Exit::Clean, "it still must not block");
        assert!(
            result.out.contains("did not check this write"),
            "an unreadable event permitted in silence: {}",
            result.out
        );
    }

    /// A path that really is elsewhere is permitted, and named.
    ///
    /// The hook has nothing to say about a file outside the repository, and
    /// "nothing to say" is itself worth one sentence: a harness whose `cwd`
    /// lands somewhere unexpected would otherwise get a gate that reports
    /// success on every write it never looked at.
    #[test]
    fn the_hook_says_so_when_the_path_is_outside_the_repository() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("UTF-8");
        std::fs::write(root.join("arch.config.json"), NAMING).expect("write");

        let result = run_with(
            &root,
            &["hook", "claude-code"],
            r#"{"tool_input":{"file_path":"/elsewhere/entirely/a.ts"}}"#,
        );

        assert_eq!(result.exit, Exit::Clean);
        assert!(
            !result.out.contains("\"permissionDecision\""),
            "it must not block a write it has no opinion about: {}",
            result.out
        );
        assert!(
            result.out.contains("outside the repository"),
            "the reason was not carried: {}",
            result.out
        );
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
            !guard.path().join(archwarden_api::CACHE_DIRECTORY).exists(),
            "an empty cache is still a file someone has to gitignore"
        );
    }

    /// `--by` names an axis, and the two are not interchangeable: one counts
    /// by rule and answers "what is dominating this output", the other counts
    /// by area and answers "where do I start". Mapping both to the default
    /// would leave `--by path` silently answering the first question.
    #[test]
    fn each_by_value_names_its_own_axis() {
        assert_eq!(
            LevelFilter::Error.level(),
            archwarden_core::level::Level::Error
        );
        assert_eq!(
            LevelFilter::Warning.level(),
            archwarden_core::level::Level::Warning
        );
        assert_eq!(By::Rule.axis(), archwarden_api::Axis::Rule);
        assert_eq!(By::Path.axis(), archwarden_api::Axis::Path);
    }

    /// And end to end, because the mapping is only worth anything if the flag
    /// reaches it: `--by path` produces a table of directories, not of rules.
    #[test]
    fn counting_by_path_names_areas_rather_than_rules() {
        let (_guard, result) = run_in(&filterable(), &["check", "--by", "path"]);

        assert!(
            result.out.contains("packages/"),
            "expected areas, got: {}",
            result.out
        );
    }

    /// Hooks are read when a session starts, so a project that has just gained
    /// one has not gained it for the session that ran this. Said out loud,
    /// because the alternative is a user testing it, seeing nothing, and
    /// concluding the installer lied.
    #[test]
    fn installing_says_when_what_it_installed_takes_effect() {
        let (_guard, result) = run_in(
            &[("arch.config.json", r#"{"version":0}"#)],
            &["install-hooks", "--claude-code"],
        );

        assert!(result.out.contains("next session"), "{}", result.out);
    }

    /// And a second run says nothing about it. Nothing was installed, so
    /// there is nothing waiting for the next session.
    #[test]
    fn installing_twice_stops_promising_a_next_session() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");
        std::fs::write(root.join("arch.config.json"), r#"{"version":0}"#).expect("write");

        let first = run_at(&root, &["install-hooks", "--claude-code"]);
        assert!(first.out.contains("next session"), "{}", first.out);

        let again = run_at(&root, &["install-hooks", "--claude-code"]);
        assert!(
            !again.out.contains("next session"),
            "nothing was installed, so nothing is waiting: {}",
            again.out
        );
        assert!(again.out.contains("already"), "{}", again.out);
    }

    /// Removing never promises a next session either. There is nothing to
    /// start.
    #[test]
    fn removing_promises_nothing_about_the_next_session() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");
        std::fs::write(root.join("arch.config.json"), r#"{"version":0}"#).expect("write");

        run_at(&root, &["install-hooks", "--claude-code"]);
        let removed = run_at(&root, &["install-hooks", "--claude-code", "--remove"]);

        assert!(!removed.out.contains("next session"), "{}", removed.out);
        assert!(removed.out.contains("removed"), "{}", removed.out);
    }

    /// A project that has the hooks and not the server still gets the server,
    /// and is told the next session is when it matters. The two files are
    /// decided separately for exactly this: a shared flag would report
    /// "already installed" and leave the project without half of it.
    #[test]
    fn half_an_installation_is_completed_rather_than_reported_as_done() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");
        std::fs::write(root.join("arch.config.json"), r#"{"version":0}"#).expect("write");

        run_at(&root, &["install-hooks", "--claude-code"]);
        std::fs::remove_file(root.join(crate::hooks::MCP_CONFIG)).expect("take the server out");

        let again = run_at(&root, &["install-hooks", "--claude-code"]);

        assert!(
            root.join(crate::hooks::MCP_CONFIG).is_file(),
            "the server came back"
        );
        assert!(
            again.out.contains("installed the MCP server"),
            "{}",
            again.out
        );
        assert!(
            again.out.contains("already"),
            "and the hooks were left alone: {}",
            again.out
        );
        assert!(again.out.contains("next session"), "{}", again.out);
    }

    /// Each of the four outcomes reads as itself. A message that said
    /// "installed" after a removal would be the uninstall equivalent of a gate
    /// reporting it is on when it is not.
    #[test]
    fn the_server_outcomes_each_read_as_themselves() {
        let config = Utf8Path::new(".mcp.json");

        assert_eq!(
            describe_mcp_outcome(crate::hooks::Outcome::Installed, config),
            "installed the MCP server in .mcp.json"
        );
        assert_eq!(
            describe_mcp_outcome(crate::hooks::Outcome::AlreadyInstalled, config),
            "the MCP server is already in .mcp.json"
        );
        assert_eq!(
            describe_mcp_outcome(crate::hooks::Outcome::Removed, config),
            "removed the MCP server from .mcp.json"
        );
        assert_eq!(
            describe_mcp_outcome(crate::hooks::Outcome::NotInstalled, config),
            "no archwarden server was in .mcp.json"
        );
    }

    /// A client that closed the pipe is how a stdio server is stopped, and it
    /// exits clean. Reporting that as a failure would put an error in the
    /// user's log every time they quit their editor.
    #[test]
    fn a_client_that_went_away_is_not_a_failed_run() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");
        std::fs::write(root.join("arch.config.json"), r#"{"version":0}"#).expect("write");

        let cli = Cli::try_parse_from(["archwarden", "mcp"]).expect("arguments should parse");
        let mut stderr = Vec::new();
        let mut input = br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#.as_slice();

        let exit = run(
            &cli,
            &root,
            &mut Output {
                out: &mut Pipe {
                    kind: std::io::ErrorKind::BrokenPipe,
                },
                err: &mut stderr,
                input: &mut input,
            },
        );

        assert_eq!(exit, Exit::Clean);
        assert!(stderr.is_empty(), "and it says nothing about it");
    }

    /// Any other write failure is a real one, and is reported.
    #[test]
    fn a_write_that_failed_for_another_reason_is_reported() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");
        std::fs::write(root.join("arch.config.json"), r#"{"version":0}"#).expect("write");

        let cli = Cli::try_parse_from(["archwarden", "mcp"]).expect("arguments should parse");
        let mut stderr = Vec::new();
        let mut input = br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#.as_slice();

        let exit = run(
            &cli,
            &root,
            &mut Output {
                out: &mut Pipe {
                    kind: std::io::ErrorKind::PermissionDenied,
                },
                err: &mut stderr,
                input: &mut input,
            },
        );

        assert_eq!(exit, Exit::ConfigProblem);
        assert!(
            String::from_utf8_lossy(&stderr).contains("archwarden mcp"),
            "{}",
            String::from_utf8_lossy(&stderr)
        );
    }

    /// A sink that fails every write, for the two arms above.
    struct Pipe {
        kind: std::io::ErrorKind,
    }

    impl std::io::Write for Pipe {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(self.kind, "the client is gone"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::new(self.kind, "the client is gone"))
        }
    }

    /// Naming the command is the point, for the server as much as for the
    /// hooks: one that resolves to nothing fails silently, at somebody's next
    /// write rather than here. Only on the way in — a second run installed
    /// nothing and has no command to name, and a run that removed it has
    /// nothing to point at at all.
    #[test]
    fn the_server_command_is_named_when_it_is_installed_and_not_otherwise() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");
        std::fs::write(root.join("arch.config.json"), r#"{"version":0}"#).expect("write");

        let installed = run_at(&root, &["install-hooks", "--claude-code"]);
        assert!(
            installed.out.contains("archwarden mcp"),
            "the command a harness will run: {}",
            installed.out
        );

        let again = run_at(&root, &["install-hooks", "--claude-code"]);
        assert!(
            !again.out.contains("archwarden mcp"),
            "nothing was installed, so there is no command to name: {}",
            again.out
        );

        let removed = run_at(&root, &["install-hooks", "--claude-code", "--remove"]);
        assert!(
            !removed.out.contains("archwarden mcp"),
            "and nothing to point at after a removal: {}",
            removed.out
        );
    }

    /// Issue #93. The installer writes the command that works *where it ran*,
    /// and the harness runs it somewhere else — which is the same machine
    /// until it is not. Saying where the command has to be runnable from is
    /// the difference between a hook that fails loudly and one that is dead
    /// and says nothing.
    #[test]
    fn installing_says_where_the_command_has_to_be_runnable_from() {
        let (_guard, result) = run_in(
            &[
                ("arch.config.json", r#"{"version":0}"#),
                ("package.json", r#"{"name":"x"}"#),
            ],
            &["install-hooks", "--claude-code"],
        );

        assert!(
            result.out.contains("must be able to run"),
            "the caveat is the whole fix for #93: {}",
            result.out
        );
    }

    /// And it is said once, on the way in. A removal has no command to name
    /// and nothing to caveat.
    #[test]
    fn removing_says_nothing_about_where_a_command_runs() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");
        std::fs::write(root.join("arch.config.json"), r#"{"version":0}"#).expect("write");

        run_at(&root, &["install-hooks", "--claude-code"]);
        let removed = run_at(&root, &["install-hooks", "--claude-code", "--remove"]);

        assert!(
            !removed.out.contains("must be able to run"),
            "{}",
            removed.out
        );
    }

    /// The one case that can be recognised: a container, and a command that
    /// names a path inside it. Issue #93's setup exactly.
    #[test]
    fn a_relative_command_installed_from_inside_a_container_is_called_out() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut input = "".as_bytes();
        caveat(
            "./node_modules/.bin/archwarden",
            true,
            &mut Output {
                out: &mut out,
                err: &mut err,
                input: &mut input,
            },
        );

        let said = String::from_utf8(out).expect("utf-8");
        assert!(said.contains("must be able to run"), "{said}");
        assert!(said.contains("looks like a container"), "{said}");
        assert!(said.contains("./node_modules/.bin/archwarden"), "{said}");
    }

    /// The same command on a host is not called out. A warning that fired
    /// everywhere is a warning nobody reads, and the general sentence above
    /// already covers the case nothing can be known about.
    #[test]
    fn the_same_command_on_a_host_gets_only_the_general_sentence() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut input = "".as_bytes();
        caveat(
            "./node_modules/.bin/archwarden",
            false,
            &mut Output {
                out: &mut out,
                err: &mut err,
                input: &mut input,
            },
        );

        let said = String::from_utf8(out).expect("utf-8");
        assert!(said.contains("must be able to run"), "{said}");
        assert!(!said.contains("looks like a container"), "{said}");
    }

    /// And a command that is not a path is not called out either, container or
    /// not: `npx archwarden` and the bare command mean the same thing on both
    /// filesystems, so there is nothing to warn about. A relative path is the
    /// only invocation whose meaning depends on which one is reading it.
    #[test]
    fn a_command_that_is_not_a_path_means_the_same_on_both_sides() {
        for command in ["npx archwarden", "archwarden"] {
            let mut out = Vec::new();
            let mut err = Vec::new();
            let mut input = "".as_bytes();
            caveat(
                command,
                true,
                &mut Output {
                    out: &mut out,
                    err: &mut err,
                    input: &mut input,
                },
            );

            let said = String::from_utf8(out).expect("utf-8");
            assert!(
                !said.contains("looks like a container"),
                "{command}: {said}"
            );
        }
    }
}
