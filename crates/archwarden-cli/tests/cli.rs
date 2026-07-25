//! Tier 2: the real binary, spawned as a process.
//!
//! The unit tests in the library half call `run` directly, which is faster and
//! lets them assert on captured output. What they cannot check is that the
//! binary wires itself up correctly: that `main` reads the working directory,
//! that clap is reachable, and that the exit code actually leaves the process.
//! That is what these cover.

// clippy's `allow-*-in-tests` relaxations key off `#[cfg(test)]` modules and
// `#[test]` functions. The helpers below are neither -- they are plain
// functions in an integration-test crate -- so the relaxation is spelled out
// here instead. This whole file is test code.
#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::str::contains;

/// Builds a temporary repository and returns it. The guard must be held:
/// dropping it deletes the tree.
fn repo(entries: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");

    for (relative, contents) in entries {
        let path = dir.path().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create dirs");
        }
        std::fs::write(&path, contents).expect("write file");
    }

    dir
}

fn archwarden() -> Command {
    Command::cargo_bin("archwarden").expect("the binary is built")
}

const MINIMAL: &str = r#"{"version": 0}"#;

#[test]
fn a_valid_config_exits_zero() {
    let dir = repo(&[("arch.config.json", MINIMAL)]);

    archwarden()
        .current_dir(dir.path())
        .args(["config", "validate"])
        .assert()
        .success()
        .stdout(contains("is valid"));
}

/// The working directory is read by `main`, not passed in, so this is the only
/// place the upward search is exercised against a real process.
#[test]
fn discovery_walks_up_from_the_directory_the_process_was_spawned_in() {
    let dir = repo(&[
        ("arch.config.json", MINIMAL),
        ("packages/app/src/placeholder.ts", ""),
    ]);

    archwarden()
        .current_dir(dir.path().join("packages/app/src"))
        .args(["config", "validate"])
        .assert()
        .success()
        .stdout(contains("arch.config.json"));
}

/// Exit code 2 has to survive all the way out of the process, because that is
/// what a CI pipeline and an agent hook actually branch on.
#[test]
fn a_broken_config_exits_two() {
    let dir = repo(&[("arch.config.json", r#"{"version": 0,,}"#)]);

    archwarden()
        .current_dir(dir.path())
        .args(["config", "validate"])
        .assert()
        .code(2)
        .stderr(contains("arch.config.json"));
}

#[test]
fn a_missing_config_exits_two() {
    let dir = repo(&[("src/placeholder.ts", "")]);

    archwarden()
        .current_dir(dir.path())
        .args(["config", "validate"])
        .assert()
        .code(2)
        .stderr(contains("archwarden init"));
}

/// Presets are resolved by the real binary, through the real resolver, against
/// a real `node_modules`.
#[test]
fn a_package_preset_is_resolved_by_the_binary() {
    let dir = repo(&[
        (
            "node_modules/@org/preset/package.json",
            r#"{"name":"@org/preset","main":"preset.json"}"#,
        ),
        (
            "node_modules/@org/preset/preset.json",
            r#"{"version":0,"rules":[
                {"type":"structure","id":"from-preset","level":"error","roots":"x/*"}]}"#,
        ),
        (
            "arch.config.json",
            r#"{"version":0,"extends":"@org/preset"}"#,
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["config", "validate"])
        .assert()
        .success()
        .stdout(contains("1 rule"))
        .stdout(contains("extends:"));
}

#[test]
fn no_subcommand_is_a_usage_error() {
    archwarden().assert().failure().stderr(contains("Usage"));
}

#[test]
fn the_version_flag_reports_a_version() {
    archwarden()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains("archwarden"));
}

/// `--help` is the first thing a user runs, and clap will happily produce it
/// for a command tree that does not do what its help says. Pinning the
/// subcommand list here is cheap.
#[test]
fn help_lists_the_available_commands() {
    archwarden()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("config"))
        .stdout(contains("--config"));
}
