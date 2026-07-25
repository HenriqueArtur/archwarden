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
        .stdout(contains("check"))
        .stdout(contains("describe"))
        .stdout(contains("scaffold"))
        .stdout(contains("agent-guide"))
        .stdout(contains("install-hooks"))
        .stdout(contains("init"))
        .stdout(contains("config"))
        .stdout(contains("--config"));
}

/// Layer 2 of `docs/AGENT-INTEGRATION.md`, through the real process: an agent
/// asks what applies to a path it is about to create. The file is not there,
/// and neither is its directory.
#[test]
fn describe_answers_through_the_binary_for_a_file_that_does_not_exist() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,"rules":[{
            "type":"naming","id":"usecase-name","level":"error","roots":"src/*",
            "file_pattern":"^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
            "must_export":{"name":"{{pascal(name)}}","kind":"function"}}]}"#,
    )]);

    archwarden()
        .current_dir(dir.path())
        .args([
            "describe",
            "src/user/create-client.use-case.ts",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(contains(r#""id": "usecase-name""#))
        .stdout(contains(r#""name": "CreateClient""#));
}

/// Layer 2's second call, through the real process.
#[test]
fn scaffold_answers_through_the_binary() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,"rules":[
            {"type":"naming","id":"usecase-name","level":"error","roots":"src/*",
             "file_pattern":"^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
             "must_export":{"name":"{{pascal(name)}}","kind":"function",
                            "signature_hint":"(deps: Deps): UseCase"}},
            {"type":"spec-pair","id":"usecase-spec","level":"error","roots":"src/*",
             "subfolders":".","spec_markers":"spec","require_non_empty_spec":true}]}"#,
    )]);

    archwarden()
        .current_dir(dir.path())
        .args(["scaffold", "src/user/create-client.use-case.ts"])
        .assert()
        .success()
        .stdout(contains(
            "export function CreateClient(deps: Deps): UseCase",
        ))
        .stdout(contains("src/user/create-client.use-case.spec.ts"));
}

/// Layer 4 through the real process, on the write a hook most needs to stop:
/// the file does not exist yet, and neither does the folder it would create.
#[test]
fn check_file_stops_a_write_that_would_create_a_forbidden_folder() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[{
                "type":"structure","id":"entity-shape","level":"error",
                "roots":"src/*","allowed_subfolders":["types","calcs"]}]}"#,
        ),
        ("src/user/types/user.ts", ""),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["check", "--file", "src/user/helpers/thing.ts"])
        .assert()
        .code(1)
        .stdout(contains("helpers"));

    archwarden()
        .current_dir(dir.path())
        .args(["check", "--file", "src/user/types/address.ts"])
        .assert()
        .success()
        .stdout(contains("is fine"));
}

/// The whole of `AGENT-INTEGRATION.md`'s recommended setup, through the real
/// process: init, install the hook, and have the hook refuse a bad write.
#[test]
fn the_recommended_setup_works_end_to_end() {
    let dir = repo(&[]);

    archwarden()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success()
        .stdout(contains("wrote"));

    std::fs::write(
        dir.path().join("arch.config.json"),
        r#"{"version":0,"rules":[{
            "type":"naming","id":"usecase-name","level":"error","roots":"src/*",
            "file_pattern":"^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
            "must_export":{"name":"{{pascal(name)}}","kind":"function"}}]}"#,
    )
    .expect("write a real config");

    archwarden()
        .current_dir(dir.path())
        .args(["install-hooks", "--claude-code"])
        .assert()
        .success()
        .stdout(contains("installed"));

    let settings = std::fs::read_to_string(dir.path().join(".claude/settings.json"))
        .expect("the hook was written");
    assert!(
        settings.contains("archwarden hook claude-code"),
        "{settings}"
    );

    std::fs::create_dir_all(dir.path().join("src/user")).expect("create dirs");
    std::fs::write(
        dir.path().join("src/user/create-client.use-case.ts"),
        "export const CreateClient = () => {};",
    )
    .expect("write the offending file");

    archwarden()
        .current_dir(dir.path())
        .args(["hook", "claude-code"])
        .write_stdin(
            r#"{"hook_event_name":"PreToolUse","tool_name":"Write",
                "tool_input":{"file_path":"src/user/create-client.use-case.ts"}}"#,
        )
        .assert()
        // The hook never fails: blocking is carried in the response.
        .success()
        .stdout(contains(r#""permissionDecision":"deny""#))
        .stdout(contains("usecase-name"))
        .stdout(contains("archwarden scaffold"));
}

/// Layer 3, redirected the way `AGENT-INTEGRATION.md` shows it: the guide goes
/// to stdout, and the user chooses where it lands.
#[test]
fn agent_guide_writes_a_digest_to_stdout() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,"rules":[
            {"type":"naming","id":"usecase-name","level":"error","roots":"src/*",
             "file_pattern":"^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
             "must_export":{"name":"{{pascal(name)}}","kind":"function"}}]}"#,
    )]);

    archwarden()
        .current_dir(dir.path())
        .arg("agent-guide")
        .assert()
        .success()
        .stdout(contains("# Architecture rules"))
        .stdout(contains("`usecase-name` (naming)"))
        .stdout(contains("archwarden describe <path>"));
}

/// The working directory is read by `main`, so a relative path typed from a
/// subdirectory has to resolve the way the user means it. This is the only
/// place that is exercised against a real process.
#[test]
fn describe_resolves_a_relative_path_from_a_subdirectory() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[{
                "type":"naming","id":"usecase-name","level":"error","roots":"src/*",
                "file_pattern":"^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
                "must_export":{"name":"{{pascal(name)}}","kind":"function"}}]}"#,
        ),
        ("src/user/placeholder.ts", ""),
    ]);

    archwarden()
        .current_dir(dir.path().join("src/user"))
        .args(["describe", "create-client.use-case.ts"])
        .assert()
        .success()
        .stdout(contains("src/user/create-client.use-case.ts"))
        .stdout(contains("CreateClient"));
}

/// The repository shape the `check` tests share: a domain entity with one
/// disallowed folder, one folder on the warn list, and one file missing its
/// spec.
fn repo_with_violations() -> tempfile::TempDir {
    repo(&[
        (
            "arch.config.json",
            r#"{
              "version": 0,
              "modules": [{"id":"domain","rules":[
                {"type":"structure","id":"domain-entity-shape","level":"error",
                 "roots":["packages/domain/src/*"],
                 "allowed_subfolders":["types","calcs"],
                 "warn_subfolders":["shared"]},
                {"type":"spec-pair","id":"calcs-need-spec","level":"error",
                 "roots":["packages/domain/src/*"],"subfolders":["calcs"]}
              ]}]
            }"#,
        ),
        ("packages/domain/src/user/types/id.ts", ""),
        ("packages/domain/src/user/calcs/age.ts", ""),
        ("packages/domain/src/user/shared/util.ts", ""),
        ("packages/domain/src/user/wrong-folder/x.ts", ""),
    ])
}

/// Findings at error level exit 1, which is what a CI gate branches on.
#[test]
fn a_repository_with_errors_exits_one() {
    let dir = repo_with_violations();

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stdout(contains("wrong-folder"))
        .stdout(contains("age.spec.ts"));
}

/// A clean repository exits 0 and says what it looked at, so a passing run is
/// distinguishable from a run that examined nothing.
#[test]
fn a_clean_repository_exits_zero_and_reports_what_it_scanned() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"structure","id":"shape","level":"error",
                 "roots":["src/*"],"allowed_subfolders":["types"]}]}"#,
        ),
        ("src/user/types/id.ts", ""),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .success()
        .stdout(contains("0 errors, 0 warnings"))
        .stdout(contains("files"));
}

/// Decision 1: warnings are visible but do not block. A run whose worst
/// finding is a warning still exits 0.
#[test]
fn warnings_alone_exit_zero() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"structure","id":"shape","level":"error",
                 "roots":["src/*"],"allowed_subfolders":["types"],
                 "warn_subfolders":["shared"]}]}"#,
        ),
        ("src/user/types/id.ts", ""),
        ("src/user/shared/util.ts", ""),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .success()
        .stdout(contains("1 warning"))
        .stdout(contains("documented debt"));
}

/// A broken config is exit 2 even from `check`, so a pipeline can still tell
/// "your setup is wrong" from "your code is wrong".
#[test]
fn checking_with_a_broken_config_exits_two() {
    let dir = repo(&[("arch.config.json", r#"{"version": 0,,}"#)]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(2);
}

/// The JSON shape is a contract with agents and other tools. Asserted field by
/// field rather than eyeballed, and pinned at the top level by its version.
#[test]
fn the_json_report_has_the_documented_shape() {
    let dir = repo_with_violations();

    let output = archwarden()
        .current_dir(dir.path())
        .args(["check", "--format", "json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();

    let parsed: serde_json::Value =
        serde_json::from_slice(&output).expect("the report is valid JSON");

    assert_eq!(parsed["version"], 0);
    assert_eq!(parsed["summary"]["errors"], 2);
    assert_eq!(parsed["summary"]["warnings"], 1);

    let findings = parsed["findings"].as_array().expect("findings is an array");
    assert_eq!(findings.len(), 3);

    // Worst first, then by path: the two errors precede the warning.
    let levels: Vec<_> = findings.iter().map(|f| f["level"].as_str()).collect();
    assert_eq!(levels, [Some("error"), Some("error"), Some("warning")]);

    let first = &findings[0];
    assert_eq!(first["rule_id"], "calcs-need-spec");
    assert_eq!(first["module_id"], "domain");
    assert_eq!(first["observed"]["type"], "sibling-missing");
    assert_eq!(first["expected"]["type"], "required-sibling");
}

/// The same repository checked twice must produce byte-identical output, or
/// snapshot tests and CI diffs become noise. This is design goal 3.
#[test]
fn two_runs_over_one_repository_agree_byte_for_byte() {
    let dir = repo_with_violations();

    let run = || {
        archwarden()
            .current_dir(dir.path())
            .args(["check", "--format", "json"])
            .assert()
            .code(1)
            .get_output()
            .stdout
            .clone()
    };

    assert_eq!(run(), run());
}
