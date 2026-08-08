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
use predicates::prelude::PredicateBooleanExt as _;
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

/// Issue #39, end to end: thirteen tool modules found by `readdir` and
/// `import()`, one of which forgot its annotation. Every layer is real here —
/// the parser reads the annotation off the declaration, the rule compares it,
/// and the report names the file and the position. `tsc` is green on both
/// files; the difference is that only one of them submitted itself to `tsc`.
#[test]
fn a_discovered_module_missing_its_annotation_fails_the_check() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"naming","id":"agent-tools-export-contract","level":"error",
                 "roots":"src/tools",
                 "file_pattern":"^(?<tool>[a-z0-9-]+)\\.tool\\.ts$",
                 "must_export":{"kind":["const"],"name":"AGENT_TOOL",
                                "annotation":"AgentToolModule"}}]}"#,
        ),
        (
            "src/tools/lookup-cep.tool.ts",
            "export const AGENT_TOOL = { spec: { name: 'lookup_cep' } };\n",
        ),
        (
            "src/tools/send-email.tool.ts",
            "import type { AgentToolModule } from '../types';\n\
             export const AGENT_TOOL: AgentToolModule = { spec: {}, build: () => {} };\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stdout(contains("lookup-cep.tool.ts"))
        .stdout(contains("`AGENT_TOOL` declares no type of its own"))
        .stdout(contains("annotated `AgentToolModule`"))
        // The one that wrote the type down is not mentioned at all.
        .stdout(contains("send-email.tool.ts").not());
}

/// The other half of decision 9: the shape is answerable before the file
/// exists, and the line it hands over is the line that passes the rule above.
#[test]
fn scaffold_hands_over_the_annotated_declaration() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,"rules":[
            {"type":"naming","id":"agent-tools-export-contract","level":"error",
             "roots":"src/tools",
             "file_pattern":"^(?<tool>[a-z0-9-]+)\\.tool\\.ts$",
             "must_export":{"kind":["const"],"name":"AGENT_TOOL",
                            "annotation":"AgentToolModule"}}]}"#,
    )]);

    archwarden()
        .current_dir(dir.path())
        .args(["scaffold", "src/tools/lookup-cep.tool.ts"])
        .assert()
        .success()
        .stdout(contains(
            "export const AGENT_TOOL: AgentToolModule = /* ... */;",
        ));
}

/// Issue #40, the reporter's repository reduced: a directory that is a leaf by
/// design, said the only way the config can say it. This used to be valid at
/// `config validate`, silent at `config doctor` and skipped at `check` — three
/// commands agreeing that a rule was fine while it enforced nothing.
#[test]
fn an_empty_allowed_subfolders_forbids_every_subfolder() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"structure","id":"referencia-sem-subpasta","level":"error",
                 "roots":["referencia"],"allowed_subfolders":[]}]}"#,
        ),
        ("referencia/nota.md", "# nota\n"),
        ("referencia/subpasta-que-nao-deveria-existir/x.md", "# x\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stdout(contains("subpasta-que-nao-deveria-existir"));
}

/// The other half of the same distinction, and the one that must not change:
/// a rule that constrains filenames and never mentions subfolders is unchanged
/// by all of it.
#[test]
fn a_rule_that_never_mentions_subfolders_still_allows_them() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"structure","id":"referencia-so-md","level":"error",
                 "roots":["referencia"],"filename_patterns":["^[a-z-]+\\.md$"]}]}"#,
        ),
        ("referencia/nota.md", "# nota\n"),
        ("referencia/qualquer-subpasta/x.md", "# x\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .success();
}

/// Issue #43. Lesson folders are `NN-slug` and the two digits are the sort key
/// for a generated index, so `semaforo` and `03_semaforo` break it silently.
/// The regex-over-a-directory-name matcher existed on `naming.dir_pattern` and
/// was reachable only through a door that requires a TypeScript parse — and
/// there is no TypeScript anywhere near these folders.
#[test]
fn a_subfolder_pattern_constrains_directory_names_without_any_typescript() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"structure","id":"licao-nome-da-pasta","level":"error",
                 "roots":["projetos"],
                 "subfolder_patterns":["^\\d{2}-[a-z0-9-]+$"]}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
        ("projetos/semaforo/projeto.md", "# semaforo\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stdout(contains("projetos/semaforo"))
        .stdout(contains("projetos/01-blink").not());
}

/// And the half that pays: the answer arrives before the folder is created,
/// which is where a naming convention is cheap to follow.
#[test]
fn scaffold_names_the_shape_a_subfolder_must_have() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"structure","id":"licao-nome-da-pasta","level":"error",
                 "roots":["projetos"],
                 "subfolder_patterns":["^\\d{2}-[a-z0-9-]+$"]}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["scaffold", "projetos"])
        .assert()
        .success()
        .stdout(contains(r"any name matching ^\d{2}-[a-z0-9-]+$"));
}

/// Issue #41. `explain` used to end a "covers nothing" report by referring to
/// `config doctor`, which then said nothing about that rule — a dead end at
/// exactly the moment a user had been told the tool knew the answer.
#[test]
fn explain_says_why_a_rule_constrains_nothing_instead_of_referring_on() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"structure","id":"toothless","level":"error",
                 "roots":["referencia"]}]}"#,
        ),
        ("referencia/nota.md", "# nota\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["config", "explain", "toothless"])
        .assert()
        .success()
        .stdout(contains("constrains nothing"))
        .stdout(contains("config doctor").not());

    // And the command that audits configurations does have it, so the class is
    // visible from there too.
    archwarden()
        .current_dir(dir.path())
        .args(["config", "doctor"])
        .assert()
        .success()
        .stdout(contains("rule-constrains-nothing"));
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
///
/// Everything but `duration_ms`, which is wall-clock and cannot be identical
/// between two runs. It is blanked rather than the test being weakened,
/// because "identical apart from one named field" is a much stronger claim
/// than "the fields I remembered to compare are equal" -- a field added later
/// and left non-deterministic would fail this, which is the point.
#[test]
fn two_runs_over_one_repository_agree_byte_for_byte() {
    let dir = repo_with_violations();

    let run = || {
        let stdout = archwarden()
            .current_dir(dir.path())
            .args(["check", "--format", "json"])
            .assert()
            .code(1)
            .get_output()
            .stdout
            .clone();

        let mut parsed: serde_json::Value =
            serde_json::from_slice(&stdout).expect("the report is JSON");
        let duration = parsed["summary"]["duration_ms"].take();
        assert!(duration.is_number(), "the run reported how long it took");
        parsed
    };

    assert_eq!(run(), run());
}

/// A repository with a real git history, since `--apply` refuses without one.
fn git_repo(entries: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = repo(entries);
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .output()
            .expect("git runs");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "test@example.invalid"]);
    git(&["config", "user.name", "test"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "initial"]);
    dir
}

/// The workspace layout `--apply` has to get right: an importer that names the
/// moved file by package, not relatively. An editor rewrites the relative half
/// of a monorepo and leaves this one pointing at nothing.
fn workspace() -> [(&'static str, &'static str); 5] {
    [
        ("arch.config.json", MINIMAL),
        (
            "packages/domain/package.json",
            r#"{"name":"@org/domain","exports":{"./id/*":"./src/id/*.ts"}}"#,
        ),
        (
            "packages/domain/src/id/shared/is-id-invalid-shared.ts",
            "export function isIdInvalidShared(id: string) {\n  return id === '';\n}\n",
        ),
        (
            "packages/domain/src/id/shared/is-id-invalid-shared.spec.ts",
            "import { isIdInvalidShared } from './is-id-invalid-shared';\nit('works', () => {});\n",
        ),
        (
            "apps/web/package.json",
            r#"{"name":"@org/web","dependencies":{"@org/domain":"workspace:*"}}"#,
        ),
    ]
}

#[test]
fn apply_moves_the_file_and_rewrites_a_package_specifier() {
    let dir = git_repo(&workspace());
    std::fs::write(
        dir.path().join("apps/web/use-it.ts"),
        "import { isIdInvalidShared } from \"@org/domain/id/shared/is-id-invalid-shared\";\n\
         export const check = isIdInvalidShared;\n",
    )
    .expect("write");
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["add", "-A"])
        .output()
        .expect("git runs");
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["commit", "-qm", "importer"])
        .output()
        .expect("git runs");

    archwarden()
        .current_dir(dir.path())
        .args([
            "impact",
            "packages/domain/src/id/shared/is-id-invalid-shared.ts",
            "--to",
            "packages/domain/src/id/calcs/is-id-invalid.ts",
            "--apply",
        ])
        .assert()
        .success()
        .stdout(contains("Moved 1 file, and 1 spec sibling with it"))
        // Case 5: the filename changed and the symbol did not, said out loud
        // rather than left for the reader to discover.
        .stdout(contains(
            "The filename changed and the exported symbol did not",
        ));

    let importer = std::fs::read_to_string(dir.path().join("apps/web/use-it.ts")).expect("read");
    assert!(
        importer.contains("\"@org/domain/id/calcs/is-id-invalid\""),
        "the package specifier followed the file: {importer}"
    );

    assert!(
        dir.path()
            .join("packages/domain/src/id/calcs/is-id-invalid.spec.ts")
            .is_file(),
        "the spec travelled and followed the rename"
    );
    assert!(
        !dir.path().join("packages/domain/src/id/shared").exists(),
        "the emptied source directory is gone, or a structure rule keeps reporting it"
    );
}

/// Issue #11, end to end. The importer lives in another package and names it
/// by package name; the package's `exports` do not cover that subpath the way
/// the bundler resolves it, so archwarden cannot place the specifier.
///
/// What used to happen: the specifier resolved to nothing, so the file was not
/// an importer, so nothing rewrote it and nothing refused. The move went
/// through, printed a success line, exited `0`, and left an import pointing at
/// a path that had just been deleted. `AGENTS.md` promises the opposite —
/// "a refusal means nothing happened, everything is validated before a byte is
/// written" — and the promise held; there was simply no refusal.
///
/// `--force` is in the command because it was in the one that produced the
/// broken repository. It must not help.
#[test]
fn apply_refuses_when_an_importer_names_a_package_it_cannot_place() {
    let dir = git_repo(&[
        ("arch.config.json", MINIMAL),
        // `./id/*` covers `id/shared/x`; `./*/*/*` does not cover it the way
        // this reads patterns, so the specifier below lands nowhere.
        (
            "packages/domain/package.json",
            r#"{"name":"@org/domain","exports":{"./*/*/*":"./src/*/*/*.ts"}}"#,
        ),
        (
            "packages/domain/src/id/shared/is-id-invalid-shared.ts",
            "export function isIdInvalidShared(id: string) {\n  return id === '';\n}\n",
        ),
        (
            "apps/web/package.json",
            r#"{"name":"@org/web","dependencies":{"@org/domain":"workspace:*"}}"#,
        ),
        (
            "apps/web/use-it.ts",
            "import { isIdInvalidShared } from \"@org/domain/id/shared/is-id-invalid-shared\";\n\
             export const check = isIdInvalidShared;\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args([
            "impact",
            "packages/domain/src/id/shared/is-id-invalid-shared.ts",
            "--to",
            "packages/domain/src/id/calcs/is-id-invalid.ts",
            "--apply",
            "--force",
        ])
        .assert()
        .code(2)
        .stderr(contains("nothing was moved"))
        .stderr(contains("apps/web/use-it.ts"))
        .stderr(contains("install"));

    assert!(
        dir.path()
            .join("packages/domain/src/id/shared/is-id-invalid-shared.ts")
            .is_file(),
        "the refusal is total: the source is where it was"
    );
    let importer = std::fs::read_to_string(dir.path().join("apps/web/use-it.ts")).expect("read");
    assert!(
        importer.contains("\"@org/domain/id/shared/is-id-invalid-shared\""),
        "and the import still points at a file that still exists: {importer}"
    );
}

/// The same shape, resolving. A workspace archwarden *can* place is not made
/// harder by the guard above — which is the half that decides whether the
/// guard is a protection or an obstacle.
#[test]
fn apply_is_untouched_when_the_package_specifier_resolves() {
    let dir = git_repo(&[
        ("arch.config.json", MINIMAL),
        (
            "packages/domain/package.json",
            r#"{"name":"@org/domain","exports":{"./id/*":"./src/id/*.ts"}}"#,
        ),
        (
            "packages/domain/src/id/shared/is-id-invalid-shared.ts",
            "export function isIdInvalidShared(id: string) {\n  return id === '';\n}\n",
        ),
        (
            "apps/web/package.json",
            r#"{"name":"@org/web","dependencies":{"@org/domain":"workspace:*"}}"#,
        ),
        (
            // An uninstalled real dependency beside the workspace one. It does
            // not resolve either, and it must not block anything: a repository
            // before `install` has thousands of these, and no move could ever
            // change what `react` means.
            "apps/web/use-it.ts",
            "import React from \"react\";\n\
             import { isIdInvalidShared } from \"@org/domain/id/shared/is-id-invalid-shared\";\n\
             export const check = [React, isIdInvalidShared];\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args([
            "impact",
            "packages/domain/src/id/shared/is-id-invalid-shared.ts",
            "--to",
            "packages/domain/src/id/calcs/is-id-invalid.ts",
            "--apply",
        ])
        .assert()
        .success();

    let importer = std::fs::read_to_string(dir.path().join("apps/web/use-it.ts")).expect("read");
    assert!(
        importer.contains("\"@org/domain/id/calcs/is-id-invalid\""),
        "the package specifier followed the file: {importer}"
    );
    assert!(
        importer.contains("\"react\""),
        "and the dependency was left alone: {importer}"
    );
}

/// `git` is the undo, so an undo that would take uncommitted work with it is
/// refused. Nothing is written, which is why the refusal can be total.
#[test]
fn apply_refuses_a_dirty_working_tree_and_changes_nothing() {
    let dir = git_repo(&workspace());
    std::fs::write(
        dir.path()
            .join("packages/domain/src/id/shared/is-id-invalid-shared.ts"),
        "export function isIdInvalidShared() { return true; }\n",
    )
    .expect("dirty it");

    archwarden()
        .current_dir(dir.path())
        .args([
            "impact",
            "packages/domain/src/id/shared/is-id-invalid-shared.ts",
            "--to",
            "packages/domain/src/id/calcs/is-id-invalid.ts",
            "--apply",
        ])
        .assert()
        .code(2)
        .stderr(contains("nothing was moved"))
        .stderr(contains("uncommitted changes"));

    assert!(
        dir.path()
            .join("packages/domain/src/id/shared/is-id-invalid-shared.ts")
            .is_file(),
        "the refusal is total"
    );
}

/// Dry run is the default. Asking must never write.
#[test]
fn impact_without_apply_writes_nothing() {
    let dir = git_repo(&workspace());

    archwarden()
        .current_dir(dir.path())
        .args([
            "impact",
            "packages/domain/src/id/shared/is-id-invalid-shared.ts",
            "--to",
            "packages/domain/src/id/calcs/is-id-invalid.ts",
        ])
        .assert()
        .success()
        .stdout(contains("Moving"));

    assert!(
        dir.path()
            .join("packages/domain/src/id/shared/is-id-invalid-shared.ts")
            .is_file(),
        "the default said what it would do and did nothing"
    );
}

/// A source matching nothing is exit 2, never an empty report -- the same
/// judgement `--rules` makes about an unknown id, and for the same reason: a
/// move with no consequences and a glob that hit nothing must not print alike.
#[test]
fn a_source_matching_nothing_is_refused() {
    let dir = git_repo(&workspace());

    archwarden()
        .current_dir(dir.path())
        .args([
            "impact",
            "packages/domain/src/*/nowhere",
            "--to",
            "../calcs",
        ])
        .assert()
        .code(2)
        .stderr(contains("matches no file"));
}

/// `check` writes a multi-megabyte binary database inside the repository, so
/// `init` says so in `.gitignore` rather than leaving it for the user to find
/// in `git status`.
#[test]
fn init_ignores_the_cache_but_not_the_baseline() {
    let dir = repo(&[]);

    archwarden()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success();

    let ignored = std::fs::read_to_string(dir.path().join(".gitignore")).expect("written");
    assert!(ignored.contains(".archwarden/cache/"), "{ignored}");
    assert!(
        !ignored.lines().any(|line| line.trim() == ".archwarden/"),
        "the baseline beside the cache is meant to be committed: {ignored}"
    );
}

/// An existing `.gitignore` is appended to, not replaced, and a repository
/// that already covers the cache is left alone.
#[test]
fn init_does_not_duplicate_an_ignore_that_is_already_there() {
    let dir = repo(&[(".gitignore", "node_modules/\n.archwarden/cache/\n")]);

    archwarden()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success();

    let ignored = std::fs::read_to_string(dir.path().join(".gitignore")).expect("read");
    assert_eq!(
        ignored.matches(".archwarden/cache/").count(),
        1,
        "{ignored}"
    );
    assert!(
        ignored.contains("node_modules/"),
        "the file was appended to"
    );
}

/// A rule whose scope matches directories and reaches no file inside them.
///
/// `roots: "packages/domain/src/*"` selects the entity directories exactly as
/// documented; if every entity keeps its code one level further down, a rule
/// about files evaluates none of them and reports silence — indistinguishable
/// from a clean repository. `doctor` exists to answer "does this config mean
/// what you think?", so this is precisely its question.
#[test]
fn doctor_reports_a_rule_that_reaches_no_file() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[{"type":"no-passthrough","id":"np","level":"warning",
                "roots":["packages/domain/src/*"]}]}"#,
        ),
        (
            "packages/domain/src/order/calcs/total.ts",
            "export const a = 1;\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["config", "doctor"])
        .assert()
        .success()
        .stdout(contains("rule-evaluates-nothing"))
        .stdout(contains("no file inside them is subject to this rule"));
}

/// And the same config with a scope that does reach the files says nothing.
#[test]
fn doctor_is_quiet_when_the_rule_reaches_files() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[{"type":"no-passthrough","id":"np","level":"warning",
                "roots":["packages/domain/**"]}]}"#,
        ),
        (
            "packages/domain/src/order/calcs/total.ts",
            "export const a = 1;\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["config", "doctor"])
        .assert()
        .success()
        .stdout(contains("No concerns"));
}

/// The batch form, end to end: a glob as the source, `--to` measured from each
/// matched directory, and every specifier that named any of the moved files
/// rewritten in one go.
///
/// The single-file tests above cannot catch what this does: `--to` resolved
/// from the wrong directory, a spec swept in twice by the glob and colliding
/// with itself, or a moved file whose own imports point at another moved file.
#[test]
fn a_batch_move_relocates_every_match_and_rewrites_across_packages() {
    let dir = git_repo(&[
        ("arch.config.json", MINIMAL),
        (
            "packages/domain/package.json",
            r#"{"name":"@org/domain","exports":{"./order/*":"./src/order/*.ts","./user/*":"./src/user/*.ts"}}"#,
        ),
        // Two entities, each with a `shared/` to collapse. `order`'s lives one
        // level deeper, which is where a destination measured from the file
        // rather than the match goes wrong.
        (
            "packages/domain/src/order/shared/calcs/total.ts",
            "export const total = 1;\n",
        ),
        (
            "packages/domain/src/order/shared/calcs/total.spec.ts",
            "import { total } from './total';\nit('works', () => {});\n",
        ),
        (
            "packages/domain/src/user/shared/name.ts",
            "export const name = 'x';\n",
        ),
        // An importer in another package, by package name — the half an editor
        // cannot do.
        (
            "apps/web/package.json",
            r#"{"name":"@org/web","dependencies":{"@org/domain":"workspace:*"}}"#,
        ),
        (
            "apps/web/src/main.ts",
            "import { total } from \"@org/domain/order/shared/calcs/total\";\n             import { name } from \"@org/domain/user/shared/name\";\n             export const both = [total, name];\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args([
            "impact",
            "packages/domain/src/*/shared",
            "--to",
            "../calcs",
            "--apply",
        ])
        .assert()
        .success()
        .stdout(contains("Moved"));

    // Measured from the matched directory, and the path *below* the match
    // comes along: `order/shared/calcs/total.ts` lands in
    // `order/calcs/calcs/`. Not `order/shared/calcs/`, which would be inside
    // the very folder being emptied — and not `order/calcs/`, which is what
    // this test asserted until issue #32.
    //
    // The doubled `calcs` looks odd and is the honest answer: the file was in
    // `shared/calcs/`, `shared` is becoming `calcs`, so it is in `calcs/calcs/`.
    // Collapsing the level is a guess about what the author meant, and the same
    // guess flattened a 19-entity namespace into one directory — 93 files onto
    // 57 paths. A move relocates what it is pointed at and changes nothing else
    // about it; the dry run prints every destination, so a surprising one is
    // visible before `--apply`.
    for landed in [
        "packages/domain/src/order/calcs/calcs/total.ts",
        "packages/domain/src/order/calcs/calcs/total.spec.ts",
        "packages/domain/src/user/calcs/name.ts",
    ] {
        assert!(dir.path().join(landed).is_file(), "{landed} did not land");
    }

    // `structure` rules are about directories, so an emptied `shared/` would
    // keep reporting the finding the move was run to remove.
    assert!(
        !dir.path().join("packages/domain/src/order/shared").exists(),
        "the emptied source directory is gone"
    );
    assert!(!dir.path().join("packages/domain/src/user/shared").exists());

    let importer = std::fs::read_to_string(dir.path().join("apps/web/src/main.ts")).expect("read");
    assert!(
        importer.contains("\"@org/domain/order/calcs/calcs/total\""),
        "{importer}"
    );
    assert!(
        importer.contains("\"@org/domain/user/calcs/name\""),
        "{importer}"
    );

    // The spec matched the glob on its own *and* travels with its unit file.
    // Named twice, it must be moved once rather than colliding with itself.
    let spec = std::fs::read_to_string(
        dir.path()
            .join("packages/domain/src/order/calcs/calcs/total.spec.ts"),
    )
    .expect("read");
    assert!(
        spec.contains("'./total'"),
        "the spec still finds it: {spec}"
    );
}

/// `--force` is the one refusal a flag may override, and it has to actually
/// carry the move out — a flag that refuses anyway is a flag nobody trusts.
#[test]
fn force_carries_the_move_past_a_dynamic_import_nothing_can_read() {
    let dir = git_repo(&[
        ("arch.config.json", MINIMAL),
        (
            "packages/domain/package.json",
            r#"{"name":"@org/domain","exports":{"./id/*":"./src/id/*.ts"}}"#,
        ),
        (
            "packages/domain/src/id/shared/is-invalid.ts",
            "export const a = 1;\n",
        ),
        // Names no module, so whether it imports the target is unknowable.
        (
            "scripts/load.ts",
            "export async function load(name: string) { return import(name); }\n",
        ),
    ]);

    let move_it = |force: bool| {
        let mut args = vec![
            "impact",
            "packages/domain/src/id/shared/is-invalid.ts",
            "--to",
            "packages/domain/src/id/calcs/is-invalid.ts",
            "--apply",
        ];
        if force {
            args.push("--force");
        }
        archwarden().current_dir(dir.path()).args(args).assert()
    };

    move_it(false)
        .code(2)
        .stderr(contains("nothing was moved"))
        .stderr(contains("scripts/load.ts"))
        .stderr(contains("--force"));
    assert!(
        dir.path()
            .join("packages/domain/src/id/shared/is-invalid.ts")
            .is_file(),
        "the refusal is total"
    );

    move_it(true).success();
    assert!(
        dir.path()
            .join("packages/domain/src/id/calcs/is-invalid.ts")
            .is_file(),
        "and the flag actually carries it out"
    );
}

/// An aliased importer no longer blocks a move when the importer's own alias
/// still covers the destination, and the refusal it used to produce no longer
/// also asks the reader to report a bug. Issue #36.
#[test]
fn a_move_under_an_alias_rewrites_through_that_alias() {
    let dir = git_repo(&[
        ("arch.config.json", MINIMAL),
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@Lib/*":["./src/lib/*"]}}}"#,
        ),
        ("src/lib/thing.ts", "export const THING = 1;\n"),
        (
            "src/app/via-alias.ts",
            "import { THING } from \"@Lib/thing\";\n",
        ),
        (
            "src/app/via-relative.ts",
            "import { THING } from \"../lib/thing\";\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args([
            "impact",
            "src/lib/thing.ts",
            "--to",
            "src/lib/renamed.ts",
            "--apply",
        ])
        .assert()
        .success();

    let aliased = std::fs::read_to_string(dir.path().join("src/app/via-alias.ts")).expect("read");
    assert!(aliased.contains("\"@Lib/renamed\""), "{aliased}");
    let relative =
        std::fs::read_to_string(dir.path().join("src/app/via-relative.ts")).expect("read");
    assert!(relative.contains("\"../lib/renamed\""), "{relative}");
}

/// And a destination outside what the alias covers still refuses -- with one
/// message, not two. The second used to say "This is a bug in archwarden"
/// about a refusal the reader had just been given the reason for.
#[test]
fn a_move_out_of_an_alias_refuses_once() {
    let dir = git_repo(&[
        ("arch.config.json", MINIMAL),
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@Lib/*":["./src/lib/*"]}}}"#,
        ),
        ("src/lib/thing.ts", "export const THING = 1;\n"),
        ("src/other/keep.ts", "export const KEEP = 1;\n"),
        (
            "src/app/via-alias.ts",
            "import { THING } from \"@Lib/thing\";\n",
        ),
    ]);

    let assert = archwarden()
        .current_dir(dir.path())
        .args([
            "impact",
            "src/lib/thing.ts",
            "--to",
            "src/other/thing.ts",
            "--apply",
        ])
        .assert()
        .failure();
    // Refusals go to stderr; the exit code is the gate and this is the reason.
    let out = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    assert!(out.contains("nothing was moved"), "{out}");
    assert!(out.contains("path alias"), "the reason is named: {out}");
    assert!(
        !out.contains("This is a bug in archwarden"),
        "the refusal explained itself; the guard is for the unexplained: {out}"
    );

    // And nothing was written.
    let importer = std::fs::read_to_string(dir.path().join("src/app/via-alias.ts")).expect("read");
    assert!(importer.contains("\"@Lib/thing\""), "{importer}");
}

/// The reopening of #36: an aliased import that reaches its file through a
/// directory `index.ts`, with the tsconfig in a subdirectory rather than at
/// the archwarden root. Both halves of the layout every monorepo has.
#[test]
fn an_aliased_directory_index_import_is_rewritten() {
    let dir = git_repo(&[
        ("arch.config.json", MINIMAL),
        (
            "apps/api/tsconfig.json",
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@Infra/*":["./src/Infra/*"]}}}"#,
        ),
        (
            "apps/api/src/Infra/Ent/Card/types/index.ts",
            "export type Card = { id: string };\n",
        ),
        (
            "apps/api/src/Seeds/data.ts",
            "import type { Card } from \"@Infra/Ent/Card/types\";\nexport type X = Card;\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args([
            "impact",
            "apps/api/src/Infra/Ent/Card",
            "--to",
            "../CardProbe",
            "--apply",
        ])
        .assert()
        .success();

    let importer =
        std::fs::read_to_string(dir.path().join("apps/api/src/Seeds/data.ts")).expect("read");
    assert!(
        importer.contains("\"@Infra/Ent/CardProbe/types\""),
        "the form the author wrote, pointing at the new place: {importer}"
    );
    assert!(
        dir.path()
            .join("apps/api/src/Infra/Ent/CardProbe/types/index.ts")
            .is_file()
    );
}
