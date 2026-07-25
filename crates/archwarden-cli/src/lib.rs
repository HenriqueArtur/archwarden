//! `archwarden`'s command line, as a library.
//!
//! The binary is a four-line shim over [`run`]. Everything that decides
//! anything lives here so it can be tested without spawning a process.

pub mod diagnostic;
pub mod exit;
pub mod report;

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
        /// How to render the report.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
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
        Command::Check { format } => {
            check(cli.config.as_deref(), working_directory, *format, output)
        }
        Command::Config { command } => match command {
            ConfigCommand::Validate => validate(cli.config.as_deref(), working_directory, output),
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

fn check(
    explicit: Option<&Utf8Path>,
    working_directory: &Utf8Path,
    format: Format,
    output: &mut Output<'_>,
) -> Exit {
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

    let outcome = archwarden_engine::run::check(&compiled, &tree);
    crate::report::render(&outcome, format, output.out);

    if outcome.fails_build() {
        Exit::Errors
    } else {
        Exit::Clean
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
}
