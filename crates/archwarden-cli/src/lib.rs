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
pub mod decisions;
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
pub mod options;
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
use camino::Utf8Path;

use crate::exit::Exit;

pub mod command;
mod commands;

pub use command::*;

use crate::commands::{
    agent::{agent_guide, check_one, install_hooks, mcp, scaffold},
    check::{CheckOptions, check, coverage, doctor, explain, validate, verify_rules},
    hook::{hook, init},
    query::{describe, impact, orphans},
    write::{write_baseline, write_decisions},
};

/// Runs a parsed command line.
///
/// Never returns an error: every failure is rendered to `output.err` and
/// reported as an [`Exit`], because a linter's exit code is its primary
/// interface and a stray `Err` bubbling to `main` would bypass it.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per subcommand, each a call; splitting it would put the \
              arms somewhere the exhaustive match no longer names them, which \
              is the property that makes a command added without a dispatch \
              fail to build"
)]
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
            as_of,
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
                as_of: as_of.as_deref(),
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
        // `--dry-run` is about writing, and `find` writes nothing, so it has
        // nothing to say here.
        Command::Decisions {
            dry_run: _,
            command: Some(DecisionsCommand::Find { terms, format }),
        } => crate::commands::find::find_decisions(
            cli.location(),
            working_directory,
            terms,
            *format,
            output,
        ),
        Command::Decisions {
            dry_run,
            command: None,
        } => write_decisions(cli.location(), working_directory, *dry_run, output),
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
        // Answered without a configuration, and that is the point: the moment
        // you need this is before there is one to read, or while the one you
        // have is the thing being changed.
        ConfigCommand::Options { name, format } => {
            crate::options::run(name.as_deref(), *format, output)
        }
        ConfigCommand::Validate => validate(location, working_directory, output),
        ConfigCommand::Doctor { format, strict } => {
            doctor(location, working_directory, *format, *strict, output)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::agent::{caveat, describe_mcp_outcome};
    use camino::Utf8PathBuf;
    use clap::Parser;

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

    /// Issue #158. A preset that turns a language on turns on *reading files*,
    /// which is a cost the adopter should be able to see rather than infer
    /// from a run getting slower.
    #[test]
    fn a_preset_that_turns_a_language_on_says_so() {
        let (_guard, result) = run_in(
            &[
                (
                    "presets/rust.json",
                    r#"{"version":0,"languages":["rust"],"rules":[
                        {"type":"structure","id":"from-preset","level":"error","roots":"p/*"}]}"#,
                ),
                (
                    "arch.config.json",
                    r#"{"version":0,"extends":"./presets/rust.json","rules":[]}"#,
                ),
            ],
            &["config", "validate"],
        );

        assert_eq!(result.exit, Exit::Clean);
        // Both: the union, not the preset's half of it. Sorted, so reordering
        // `extends` does not reword the line.
        assert!(result.out.contains("reads: rust, ts"), "{}", result.out);
    }

    /// And a config with no presets is not told what it just wrote.
    #[test]
    fn a_config_with_no_presets_is_not_told_what_it_reads() {
        let (_guard, result) = run_in(&[("arch.config.json", MINIMAL)], &["config", "validate"]);
        assert!(!result.out.contains("reads:"), "{}", result.out);
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

    /// Issue #113. A reviewer reading `+ entity-shape src/billing/handlers`
    /// has to know by heart which decision that rule serves — and a reviewer
    /// who has to know it by heart is a reviewer who approves it. Naming the
    /// decision is what turns a diff line into a sentence somebody answers for.
    #[test]
    fn a_dry_run_names_the_decision_the_debt_is_added_against() {
        let decided = r#"{"version":0,
            "decisions":[{"id":"ADR-014","title":"entities are flat"}],
            "rules":[{
              "type":"structure","id":"entity-shape","level":"error",
              "decision":"ADR-014",
              "roots":["src/*"],"allowed_subfolders":["types"]}]}"#;

        let (guard, accepted) = run_in(
            &[("arch.config.json", decided), ("src/order/types/a.ts", "")],
            &["baseline"],
        );
        assert_eq!(accepted.exit, Exit::Clean);
        let root = Utf8PathBuf::from_path_buf(guard.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");

        std::fs::create_dir_all(root.join("src/billing/handlers")).expect("create dirs");
        std::fs::write(root.join("src/billing/handlers/b.ts"), "").expect("write");
        let dry = run_at(&root, &["baseline", "--dry-run"]);

        assert!(
            dry.out.contains("against ADR-014 — entities are flat"),
            "the addition says what it is debt against: {}",
            dry.out
        );
        drop(guard);
    }

    /// A rule that names no decision adds debt against nothing, and the line
    /// stays exactly as it was. Every configuration written before 0.21 is
    /// this one, and none of them should grow a line saying so.
    #[test]
    fn a_dry_run_says_nothing_extra_when_a_rule_serves_no_decision() {
        let structure = r#"{"version":0,"rules":[{
            "type":"structure","id":"entity-shape","level":"error",
            "roots":["src/*"],"allowed_subfolders":["types"]}]}"#;

        let (guard, accepted) = run_in(
            &[
                ("arch.config.json", structure),
                ("src/order/types/a.ts", ""),
            ],
            &["baseline"],
        );
        assert_eq!(accepted.exit, Exit::Clean);
        let root = Utf8PathBuf::from_path_buf(guard.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");

        std::fs::create_dir_all(root.join("src/billing/handlers")).expect("create dirs");
        std::fs::write(root.join("src/billing/handlers/b.ts"), "").expect("write");
        let dry = run_at(&root, &["baseline", "--dry-run"]);

        assert!(dry.out.contains("+ entity-shape"), "{}", dry.out);
        assert!(!dry.out.contains("against"), "{}", dry.out);
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
