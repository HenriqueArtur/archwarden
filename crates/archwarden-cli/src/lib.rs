//! `archwarden`'s command line, as a library.
//!
//! The binary is a four-line shim over [`run`]. Everything that decides
//! anything lives here so it can be tested without spawning a process.

pub mod diagnostic;
pub mod exit;

use archwarden_config::discovery::{self, LoadedConfig};
use camino::{Utf8Path, Utf8PathBuf};
use clap::{Parser, Subcommand};

use crate::{diagnostic::ConfigDiagnostic, exit::Exit};

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
}

/// Runs a parsed command line.
///
/// Never returns an error: every failure is rendered to `output.err` and
/// reported as an [`Exit`], because a linter's exit code is its primary
/// interface and a stray `Err` bubbling to `main` would bypass it.
pub fn run(cli: &Cli, working_directory: &Utf8Path, output: &mut Output<'_>) -> Exit {
    match &cli.command {
        Command::Config { command } => match command {
            ConfigCommand::Validate => validate(cli.config.as_deref(), working_directory, output),
        },
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

fn validate(
    explicit: Option<&Utf8Path>,
    working_directory: &Utf8Path,
    output: &mut Output<'_>,
) -> Exit {
    let loaded = match load(explicit, working_directory) {
        Ok(loaded) => loaded,
        Err(error) => {
            let report = miette::Report::new(ConfigDiagnostic::from_load_error(&error));
            let _ = writeln!(output.err, "{report:?}");
            return Exit::ConfigProblem;
        }
    };

    if !loaded.config.version_is_supported() {
        let _ = writeln!(
            output.err,
            "{}: config declares version {}, but this build understands version {}",
            loaded.path,
            loaded.config.version,
            archwarden_config::config::SCHEMA_VERSION,
        );
        return Exit::ConfigProblem;
    }

    let rules = loaded.config.rules().count();
    let _ = writeln!(
        output.out,
        "{} is valid ({} rule{})",
        loaded.path,
        rules,
        if rules == 1 { "" } else { "s" }
    );

    Exit::Clean
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

        let mut command_line = vec!["archwarden"];
        command_line.extend_from_slice(args);
        let cli = Cli::try_parse_from(command_line).expect("arguments should parse");

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run(
            &cli,
            &root,
            &mut Output {
                out: &mut stdout,
                err: &mut stderr,
            },
        );

        let captured = Captured {
            out: String::from_utf8(stdout).expect("stdout is UTF-8"),
            err: String::from_utf8(stderr).expect("stderr is UTF-8"),
            exit,
        };
        (dir, captured)
    }

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
        let exit = run(
            &cli,
            &nested,
            &mut Output {
                out: &mut stdout,
                err: &mut stderr,
            },
        );

        assert_eq!(exit, Exit::Clean);
        assert!(String::from_utf8_lossy(&stdout).contains("arch.config.json"));
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
}
