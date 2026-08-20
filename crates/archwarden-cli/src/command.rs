//! The command line as clap sees it.
//!
//! Every subcommand, flag and value type -- the surface, with no behaviour.
//! What each one *does* is `crate::commands`; which one to do is `crate::run`.

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};

use crate::report::Format;

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

        /// The day to answer for, as `YYYY-MM-DD`. Defaults to today, in UTC.
        ///
        /// Only `metadata.deadline` reads it, and it is a flag rather than a
        /// clock so two machines given the same date give the same answer —
        /// the determinism decision 28 defended when it refused to read `git`.
        ///
        /// It is also the warning window, with no field for one: a second,
        /// non-gating run that asks about the future says what is about to
        /// break.
        ///
        ///   archwarden check                     # the gate
        ///   archwarden check --as-of 2026-09-02  # what breaks in a fortnight
        #[arg(long, value_name = "DATE")]
        as_of: Option<String>,
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

    /// Write the decision documents, one per declared decision.
    ///
    /// Writes `.archwarden/decisions/<id>.md`, which is meant to be committed.
    /// Everything the config knows is generated; one marked region in each
    /// file belongs to whoever opens it, and regenerating never rewrites that.
    ///
    /// This is not two owners. The config stays the truth for what is
    /// enforced, and the document is a rendering of it with room for the three
    /// paragraphs JSON has no place for. `config doctor` reports a document
    /// that no longer matches the config it came from. Issue #116.
    Decisions {
        /// Say what writing would change, and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Ask a question of the decisions instead of writing them.
        #[command(subcommand)]
        command: Option<DecisionsCommand>,
    },

    /// Inspect the configuration itself.
    Config {
        /// Which config command to run.
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

/// `archwarden decisions ...`
#[derive(Debug, Subcommand)]
pub enum DecisionsCommand {
    /// Has this already been rejected?
    ///
    /// Searches every declared decision -- its title, its reason, and every
    /// alternative it rejected together with the argument against it -- for
    /// anything the terms reach. The person about to propose a losing option
    /// does not know the decision's id, and will name the option differently
    /// from whoever rejected it: "single layer", "monolith" and "one package"
    /// are the same option under three names. Issue #162.
    ///
    /// Every match, in declaration order, with no score. The question is not
    /// which is most similar, it is whether there is anything similar -- and
    /// a false negative here is the failure `alternatives` exists to prevent,
    /// while a false positive costs two seconds of reading.
    Find {
        /// The words to look for. Accents and case are ignored.
        #[arg(required = true, num_args = 1..)]
        terms: Vec<String>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = crate::report::Format::Text)]
        format: crate::report::Format,
    },
}

/// `archwarden config ...`
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Say what can go in an `arch.config.json`, before you write it.
    ///
    /// Every other question here is about a configuration that exists. This
    /// one is about the ones you could write: the config's own keys, and the
    /// ten values a rule's `type` can take, each with its required fields,
    /// what they mean, their defaults, and a rule to paste.
    ///
    /// Read out of archwarden's own types, so it cannot describe a shape the
    /// binary would refuse. Issue #97.
    Options {
        /// One key or rule kind, instead of the list.
        #[arg(value_name = "NAME")]
        name: Option<String>,

        /// How to render the answer.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },

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

impl Output<'_> {
    /// Where something that is not the report goes.
    ///
    /// In `--format json`, stdout is the document and nothing else: a note
    /// written past the closing brace is trailing text, and `JSON.parse` of a
    /// whole-repository report was broken for every repository that has a
    /// baseline. An aside goes to stderr there, which is where the failure
    /// half of the same write already went. Issue #110.
    ///
    /// In the text format nothing moves. The report *is* prose, and a note
    /// beside it is part of what somebody at a terminal reads.
    pub(crate) fn aside(&mut self, format: crate::report::Format) -> &mut dyn std::io::Write {
        match format {
            crate::report::Format::Json => &mut *self.err,
            crate::report::Format::Text => &mut *self.out,
        }
    }
}
