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

/// A repository with one commit, so `HEAD` names something.
///
/// The stop hook asks git what changed since `HEAD`, which is the turn's work
/// unless the agent committed midway.
fn git_init(root: &std::path::Path) {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git runs");
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    run(&["add", "arch.config.json"]);
    run(&["commit", "-qm", "config"]);
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

/// The page speaks the language it was asked for; the terminal does not.
///
/// A CI log is pasted into an issue, searched for and read by an agent —
/// `AGENTS.md` teaches one to read that output — so a log whose language
/// depends on who ran it is worse than one somebody has to translate.
#[test]
fn only_the_page_is_translated() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"modules":[{"id":"domain",
                "rules":[{"type":"structure","id":"domain-shape","level":"error",
                 "roots":"packages/domain/src/*","allowed_subfolders":["calcs"]}]}]}"#,
        ),
        (
            "packages/domain/src/order/nope/x.ts",
            "export const x = 1;\n",
        ),
    ]);
    let page = dir.path().join("relatorio.html");

    archwarden()
        .current_dir(dir.path())
        .args([
            "check",
            "--lang",
            "pt-br",
            "--html",
            page.to_str().expect("utf-8"),
        ])
        .assert()
        .code(1)
        // The terminal is English whatever the page is.
        //
        // Asserted on the English summary line rather than on the absence of a
        // translated one. The obvious negative assertion is a trap twice over:
        // the Portuguese word for an error is a substring of the English one,
        // so it passes for the wrong reason — and writing it here would put
        // Portuguese in a file the spell checker reads.
        .stdout(contains("1 error, 0 warnings"));

    let html = std::fs::read_to_string(&page).expect("the page was written");
    assert!(html.contains(r#"<html lang="pt-BR">"#), "{html}");
    assert!(html.contains("O que a config governa"), "{html}");
    // The grid always renders; the pressure section only exists where a
    // boundary rule does, and this fixture has none.
    assert!(html.contains("Quem pode importar quem"), "{html}");
    assert!(
        !html.contains("What the config governs"),
        "no English left over: {html}"
    );
}

/// A repository decides its language once, in the config. Nobody should have
/// to remember a flag to read their own report.
#[test]
fn the_config_can_choose_the_language() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"language":"pt-br","rules":[
                {"type":"structure","id":"shape","level":"error",
                 "roots":"src","allowed_subfolders":[]}]}"#,
        ),
        ("src/nope/x.ts", "export const x = 1;\n"),
    ]);
    let page = dir.path().join("relatorio.html");

    archwarden()
        .current_dir(dir.path())
        .args(["check", "--html", page.to_str().expect("utf-8")])
        .assert()
        .code(1);

    let html = std::fs::read_to_string(&page).expect("the page was written");
    assert!(html.contains(r#"<html lang="pt-BR">"#), "{html}");
}

/// And the flag wins over it, for the one run that wants the other.
#[test]
fn the_flag_overrides_the_configs_language() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"language":"pt-br","rules":[
                {"type":"structure","id":"shape","level":"error",
                 "roots":"src","allowed_subfolders":[]}]}"#,
        ),
        ("src/nope/x.ts", "export const x = 1;\n"),
    ]);
    let page = dir.path().join("report.html");

    archwarden()
        .current_dir(dir.path())
        .args([
            "check",
            "--lang",
            "en",
            "--html",
            page.to_str().expect("utf-8"),
        ])
        .assert()
        .code(1);

    let html = std::fs::read_to_string(&page).expect("the page was written");
    assert!(html.contains(r#"<html lang="en">"#), "{html}");
}

/// The digest keeps its language too: markdown is a CLI output and JSON is a
/// contract, so `--lang` reaches neither.
#[test]
fn the_markdown_digest_is_english_whatever_the_language_is() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,"rules":[{"type":"structure","id":"shape","level":"error",
            "roots":"src","allowed_subfolders":[]}]}"#,
    )]);

    archwarden()
        .current_dir(dir.path())
        .args(["agent-guide", "--lang", "pt-br"])
        .assert()
        .success()
        .stdout(contains("Architecture rules"))
        .stdout(contains("Regras de arquitetura").not());
}

/// The page is a side artefact, not a rendering: the terminal keeps its summary
/// and its exit code, and the file is written beside them. A browser cannot
/// read a pipe, so a `--format` that had to be redirected would be the wrong
/// shape for this.
#[test]
fn check_writes_a_page_without_changing_what_the_terminal_says() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"modules":[{"id":"domain","why":"published on its own",
                "rules":[{"type":"structure","id":"domain-shape","level":"error",
                 "roots":"packages/domain/src/*","allowed_subfolders":["calcs"]}]}]}"#,
        ),
        (
            "packages/domain/src/order/nope/x.ts",
            "export const x = 1;
",
        ),
    ]);
    let page = dir.path().join("report.html");

    archwarden()
        .current_dir(dir.path())
        .args(["check", "--html", page.to_str().expect("utf-8")])
        .assert()
        // The gate is untouched: a side artefact never decides an exit code.
        .code(1)
        .stdout(contains("1 error"))
        .stdout(contains("page written to"));

    let html = std::fs::read_to_string(&page).expect("the page was written");
    assert!(html.starts_with("<!doctype html>"), "{html}");
    assert!(html.contains("published on its own"), "the reason travels");
    assert!(!html.contains("<script"), "read-only");
    assert!(!html.contains("https://"), "nothing is fetched");
}

/// A page that could not be written is reported and does not fail the run.
/// Letting a full disk turn a failing build green would be the worst possible
/// trade for a side artefact.
#[test]
fn a_page_that_cannot_be_written_does_not_change_the_exit_code() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[{"type":"structure","id":"shape","level":"error",
                "roots":"src","allowed_subfolders":[]}]}"#,
        ),
        (
            "src/nope/x.ts",
            "export const x = 1;
",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["check", "--html", "no/such/directory/report.html"])
        .assert()
        .code(1)
        .stderr(contains("cannot write"));
}

/// Issue #13, the half that is a bug on its own. An Astro repository with a
/// boundary rule got exit 0 while every page imported the domain directly, and
/// nothing in the output said the rule had not been evaluated for those files.
///
/// It is loud now even without opting in — which is the point: a user who never
/// read about the feature still finds out.
#[test]
fn astro_files_are_a_named_skip_until_the_config_asks_for_them() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"import-boundary","id":"pages-forbid-domain","level":"error",
                 "from":["src/**"],"forbid_import_from":["src/domain/**"]}]}"#,
        ),
        ("src/domain/post.ts", "export const post = 1;\n"),
        (
            "src/pages/blog.astro",
            "---\nimport { post } from '../domain/post';\n---\n\n<div />\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .success()
        .stdout(contains("skipped"))
        .stdout(contains("src/pages/blog.astro"));
}

/// And with the opt-in, the boundary is actually held. The import lives in the
/// `---` fence, which is where essentially every import in an Astro page is.
#[test]
fn an_astro_page_crossing_a_boundary_is_reported_once_astro_is_enabled() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"languages":["ts","astro"],"rules":[
                {"type":"import-boundary","id":"pages-forbid-domain","level":"error",
                 "from":["src/**"],"forbid_import_from":["src/domain/**"]}]}"#,
        ),
        ("src/domain/post.ts", "export const post = 1;\n"),
        (
            "src/pages/blog.astro",
            "---\nimport { post } from '../domain/post';\n---\n\n<div />\n",
        ),
        ("src/pages/sobre.astro", "<h1>Sobre</h1>\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stdout(contains("src/pages/blog.astro"))
        // A markup-only page has no imports, and is not a skip either.
        .stdout(contains("sobre.astro").not())
        .stdout(contains("skipped").not());
}

/// The rule issue #13 says earns its keep, and it falls out of reading the
/// fence: an Astro page has no named component export, but it does export
/// `getStaticPaths`.
#[test]
fn a_naming_rule_can_ask_an_astro_page_for_get_static_paths() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"languages":["ts","astro"],"rules":[
                {"type":"naming","id":"pages-are-static","level":"error",
                 "roots":["src/pages/blog"],
                 "file_pattern":"^\\[.+\\]\\.astro$",
                 "must_export":{"kind":["function"],"name":"getStaticPaths"}}]}"#,
        ),
        (
            "src/pages/blog/[slug].astro",
            "---\nconst x = 1;\n---\n<div />\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stdout(contains("no export named `getStaticPaths`"));
}

/// Issue #44, end to end and through every layer that is new: the walk
/// classifies a `.md` as a document, the document front-end finds the fence and
/// parses the block, the cache stores it in its own table, and the rule asks it
/// questions.
///
/// The frontmatter here is not documentation. It is the schema three scripts
/// read, and nothing else in this repository type-checks a markdown file.
#[test]
fn a_document_whose_frontmatter_is_wrong_fails_the_check() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"frontmatter","id":"projeto-frontmatter","level":"error",
                 "why":"three scripts and the generated index read this block",
                 "roots":["projetos/*"],
                 "file_pattern":"^projeto\\.md$",
                 "require":["id","nivel","componentes"],
                 "one_of":{"nivel":["1","2","3"]},
                 "equals":{"id":"{{raw(dirname)}}"}}]}"#,
        ),
        (
            "projetos/01-blink/projeto.md",
            "---\nid: 01-blink\nnivel: 1\ncomponentes:\n  - { id: led, qtd: 1 }\n---\n\n# Blink\n",
        ),
        (
            "projetos/03-semaforo/projeto.md",
            "---\nid: semaforo\nnivel: 9\n---\n\n# Semáforo\n",
        ),
        ("projetos/07-oled/projeto.md", "# OLED\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        // The key that is simply absent.
        .stdout(contains("carries no `componentes`"))
        // The value outside the vocabulary — quoted back, because it is almost
        // always a spelling.
        .stdout(contains("`nivel` is `9`"))
        // The value that disagrees with the path.
        .stdout(contains(
            "`id` is `semaforo`, and the path says `03-semaforo`",
        ))
        // And no block at all is a finding, not a skip.
        .stdout(contains("has no frontmatter block"))
        // The complete document is not mentioned.
        .stdout(contains("01-blink").not())
        // The reason travels with it.
        .stdout(contains("why: three scripts and the generated index"));
}

/// Issue #104, end to end and through every layer that is new: the JS/TS
/// front-end reads a second kind of marker out of the comments it already
/// scans, the header boundary is worked out where the source text is, and the
/// rule asks `frontmatter`'s three questions of a `.ts` file.
///
/// Five faults in five files, because they are five different edits.
#[test]
fn a_file_whose_header_declares_the_wrong_thing_fails_the_check() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"metadata","id":"payments-declares-an-owner","level":"error",
                 "why":"a module without an owner is a module nobody reviews",
                 "roots":["src/payments/**"],
                 "require":["owner"],
                 "one_of":{"stability":["stable","experimental","deprecated"]},
                 "equals":{"module":"{{raw(dirname)}}"}}]}"#,
        ),
        (
            "src/payments/refund.ts",
            "// Copyright 2026\n\
             // archwarden-owner: payments-team\n\
             // archwarden-stability: stable\n\
             // archwarden-module: payments\n\
             export const refund = () => 1;\n",
        ),
        ("src/payments/charge.ts", "export const charge = () => 1;\n"),
        (
            "src/payments/void.ts",
            "// archwarden-owner: payments-team\n\
             // archwarden-stability: wip\n\
             export const cancel = () => 1;\n",
        ),
        (
            "src/payments/settle.ts",
            "// archwarden-owner: payments-team\n\
             // archwarden-module: billing\n\
             export const settle = () => 1;\n",
        ),
        (
            "src/payments/capture.ts",
            "import { charge } from './charge';\n\
             // archwarden-owner: payments-team\n\
             export const capture = () => charge();\n",
        ),
        (
            "src/payments/dispute.ts",
            "// archwarden-owner: payments-team\n\
             // archwarden-owner: risk-team\n\
             export const dispute = () => 1;\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        // The key that is simply absent.
        .stdout(contains("declares no `owner` in its header"))
        // The value outside the vocabulary — quoted back, because it is almost
        // always a spelling.
        .stdout(contains("`stability` is `wip`"))
        // The value that disagrees with the path.
        .stdout(contains(
            "`module` is `billing`, and the path says `payments`",
        ))
        // The decision the milestone turned on: a marker that is there, in the
        // wrong place, is never reported as absent.
        .stdout(contains(
            "declares `owner` below the first statement, where it is not read",
        ))
        // Two claims about one thing, with both quoted back.
        .stdout(contains(
            "declares `owner` twice, as `payments-team` and `risk-team`",
        ))
        // A licence block above the claims does not push them out of the header.
        .stdout(contains("refund.ts").not())
        // The reason travels with it.
        .stdout(contains("why: a module without an owner"));
}

/// A suppression is a suppression. The two grammars share a prefix, and the
/// day `archwarden-allow` starts reading as a claim about a key called `allow`
/// is the day one comment has two meanings.
#[test]
fn a_suppression_is_not_read_as_a_claim() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"metadata","id":"payments-declares-an-owner","level":"error",
                 "roots":["src/payments/**"],
                 "require":["owner"]}]}"#,
        ),
        (
            "src/payments/refund.ts",
            "// archwarden-allow: the owner of this file is being decided\n\
             export const refund = () => 1;\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stdout(contains("declares no `owner` in its header"));
}

/// A key no comment could ever spell is refused where the config loads, rather
/// than reporting every file in its scope for ever.
#[test]
fn a_metadata_rule_asking_for_an_unreachable_key_does_not_load() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,"rules":[
            {"type":"metadata","id":"payments-owned","level":"error",
             "roots":["src/**"],"require":["allow"]}]}"#,
    )]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .failure()
        .stderr(contains("archwarden-allow"));
}

/// A number in the document and a number in the config are one question in two
/// notations, and a quoted value is the same value.
#[test]
fn a_scalar_matches_however_yaml_spelled_it() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"frontmatter","id":"notas-status","level":"error",
                 "roots":["projetos/*"],
                 "file_pattern":"^notas\\.md$",
                 "one_of":{"status":["feito","fazendo"],"nivel":["1"]}}]}"#,
        ),
        (
            "projetos/01-blink/notas.md",
            "---\nstatus: \"feito\"  # concluído ontem\nnivel: 1\n---\n\n# Notas\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .success();
}

/// Issue #45, end to end. The separation exists so a lesson can be rewritten
/// without destroying what was written while doing it, and it only works if the
/// notes file is *there* — a directory with no `notas.md` is one a regeneration
/// writes over, and the failure looks exactly like "I hadn't taken notes yet".
#[test]
fn a_file_whose_companion_is_missing_fails_the_check() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"pair","id":"licao-tem-notas","level":"error",
                 "roots":["projetos/*"],
                 "file_pattern":"^projeto\\.md$","must_exist":"notas.md"}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
        ("projetos/01-blink/notas.md", "# notas\n"),
        ("projetos/03-semaforo/projeto.md", "# semaforo\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stdout(contains("projetos/03-semaforo/notas.md` does not exist"))
        .stdout(contains("01-blink").not());
}

/// One direction, always. An orphan companion is a note taken before the
/// lesson was written, which is fine.
#[test]
fn an_orphan_companion_is_not_a_finding() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"pair","id":"licao-tem-notas","level":"error",
                 "roots":["projetos/*"],
                 "file_pattern":"^projeto\\.md$","must_exist":"notas.md"}]}"#,
        ),
        ("projetos/09-adiantada/notas.md", "# ideias\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .success();
}

/// The half no directory-scoped rule can reach: the sketch needs the lesson
/// one level up, and the sketch may be called anything.
#[test]
fn a_companion_outside_the_directory_is_found() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"pair","id":"sketch-tem-licao","level":"error",
                 "roots":["projetos/*/sketch"],
                 "file_pattern":"\\.ino$","must_exist":"../projeto.md"}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
        ("projetos/01-blink/sketch/blink.ino", "void setup() {}\n"),
        (
            "projetos/03-semaforo/sketch/semaforo.ino",
            "void setup() {}\n",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stdout(contains("projetos/03-semaforo/projeto.md` does not exist"))
        .stdout(contains("01-blink").not());
}

/// Issue #42, end to end. A lesson missing `exercicios.md` still renders,
/// still commits, still shows up in the index — and is found weeks later by
/// the person who reaches the end of it. There is no build here at all, so
/// nothing else was ever going to catch it.
#[test]
fn a_directory_missing_a_required_file_fails_the_check() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"presence","id":"licao-completa","level":"error",
                 "roots":["projetos/*"],
                 "require":["projeto.md","exercicios.md","notas.md"]}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
        ("projetos/01-blink/exercicios.md", "# ex\n"),
        ("projetos/01-blink/notas.md", "# notas\n"),
        ("projetos/03-semaforo/projeto.md", "# semaforo\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stdout(contains("`exercicios.md` is not here"))
        .stdout(contains("`notas.md` is not here"))
        // The complete lesson is not mentioned.
        .stdout(contains("01-blink").not());
}

/// The half the issue asks for by name: the filenames arrive *before* the
/// directory does, which is how a unit of work gets started.
#[test]
fn scaffold_lists_the_files_a_new_directory_must_have() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"presence","id":"licao-completa","level":"error",
                 "roots":["projetos/*"],
                 "require":["projeto.md","notas.md"],
                 "require_any":["\\.ino$"]}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["scaffold", "projetos/17-nova"])
        .assert()
        .success()
        .stdout(contains("Files that must exist here:"))
        .stdout(contains("projeto.md"))
        .stdout(contains("notas.md"))
        .stdout(contains(r"a file matching \.ino$"));
}

/// `require` takes filenames. A path is refused with the rule that says it
/// instead, rather than silently reaching into a subdirectory or silently not.
#[test]
fn a_require_entry_that_is_a_path_is_refused() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,"rules":[
            {"type":"presence","id":"licao-completa","level":"error",
             "roots":["projetos/*"],"require":["sketch/sketch.ino"]}]}"#,
    )]);

    archwarden()
        .current_dir(dir.path())
        .args(["config", "validate"])
        .assert()
        .code(2)
        .stderr(contains("takes filenames"));
}

/// `must_exist` is literal, and a config that reaches for a template is told
/// so rather than hunting for a file with braces in its name.
///
/// Issue #50: the template form is what `naming` and `frontmatter.equals`
/// accept, so writing one here is the obvious mistake. It validated, ran, and
/// reported every governed file as missing `meu/{{raw(dirname)}}.md` —
/// sixteen confident findings about a file nothing could create.
#[test]
fn a_must_exist_that_reaches_for_a_template_is_refused() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,"rules":[
            {"type":"pair","id":"projeto-tem-nota","level":"error",
             "roots":["projetos/*"],"file_pattern":"^projeto\\.md$",
             "must_exist":"../../meu/{{raw(dirname)}}.md"}]}"#,
    )]);

    archwarden()
        .current_dir(dir.path())
        .args(["config", "validate"])
        .assert()
        .code(2)
        .stderr(contains("literal"));
}

/// Issue #53. `describe` answered "no rule applies" about a folder name that
/// `check` refuses, and `scaffold` handed back a shape to build there.
///
/// `describe --help` says *"what the rules require of a path, which need not
/// exist yet"* — and the path that does not exist yet is precisely where the
/// name is still a choice. Answering after the folder is created is answering
/// too late.
#[test]
fn a_folder_is_told_that_its_own_name_is_constrained() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"structure","id":"projeto-nome-de-pasta","level":"error",
                 "roots":["projetos"],
                 "subfolder_patterns":["^\\d{2}-[a-z0-9]+(-[a-z0-9]+)*$"]}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["describe", "projetos/sensor-sem-numero"])
        .assert()
        .success()
        .stdout(contains("projeto-nome-de-pasta"))
        .stdout(contains("a folder name matching"));
}

/// And `scaffold` leads with the fact that nothing built there can pass,
/// before listing the shape.
///
/// Its whole answer is a thing to go and build, so an unbuildable location has
/// to come first. Correction C11 made this argument for filenames — *"an agent
/// scaffolding a path whose name is already wrong would be told everything
/// except the thing it has to fix first"* — and it was never carried to
/// folders.
#[test]
fn scaffold_leads_with_a_path_that_cannot_pass() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"structure","id":"projeto-nome-de-pasta","level":"error",
                 "roots":["projetos"],
                 "subfolder_patterns":["^\\d{2}-[a-z0-9]+(-[a-z0-9]+)*$"]},
                {"type":"presence","id":"projeto-tem-os-tres","level":"error",
                 "roots":["projetos/*"],"require":["projeto.md"]}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["scaffold", "projetos/sensor-sem-numero"])
        .assert()
        .success()
        .stdout(contains("is not a path these rules allow"))
        .stdout(contains("Nothing built here can pass"));
}

/// A name the rules do permit gets the shape and no refusal.
#[test]
fn scaffold_does_not_refuse_a_name_the_rules_allow() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"structure","id":"projeto-nome-de-pasta","level":"error",
                 "roots":["projetos"],
                 "subfolder_patterns":["^\\d{2}-[a-z0-9]+(-[a-z0-9]+)*$"]},
                {"type":"presence","id":"projeto-tem-os-tres","level":"error",
                 "roots":["projetos/*"],"require":["projeto.md"]}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["scaffold", "projetos/02-sensor"])
        .assert()
        .success()
        .stdout(contains("Expected shape for"))
        .stdout(contains("projeto.md"));
}

/// Issue #55, through the real process: the hook judges the write, not the
/// file.
///
/// Three failures in one, and the third had no way out from inside an agent
/// loop — it was told to fix the file and refused permission to fix it, with a
/// message naming a rule the pending write already satisfied.
#[test]
fn the_hook_judges_the_write_and_not_the_file_on_disk() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"naming","id":"usecase-name","level":"error",
                 "roots":["src/*"],
                 "file_pattern":"^(?<n>[a-z0-9-]+)\\.use-case\\.ts$",
                 "must_export":{"kind":["function"],"name":"{{pascal(n)}}"}}]}"#,
        ),
        ("src/user/other.ts", "export const a = 1;\n"),
    ]);
    let target = dir.path().join("src/user/create-client.use-case.ts");
    let target = target.to_str().expect("utf-8");

    // A new file, nothing on disk. This is the case a pre-write gate most
    // exists for, and every content rule used to sail through it.
    archwarden()
        .current_dir(dir.path())
        .args(["hook", "claude-code"])
        .write_stdin(format!(
            r#"{{"tool_name":"Write","tool_input":{{"file_path":"{target}",
               "content":"export function Wrong() {{}}"}}}}"#
        ))
        .assert()
        .success()
        .stdout(contains("permissionDecision"));

    // And the write that satisfies the rule is permitted, with the violating
    // version still on disk — the deadlock.
    std::fs::write(target, "export function Wrong() {}\n").expect("write");
    archwarden()
        .current_dir(dir.path())
        .args(["hook", "claude-code"])
        .write_stdin(format!(
            r#"{{"tool_name":"Write","tool_input":{{"file_path":"{target}",
               "content":"export function CreateClient() {{}}"}}}}"#
        ))
        .assert()
        .success()
        .stdout("{}\n");
}

/// An `Edit` sends a replacement rather than a document, so the result has to
/// be reconstructed before it can be judged.
#[test]
fn the_hook_replays_an_edit_before_judging_it() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"naming","id":"usecase-name","level":"error",
                 "roots":["src/*"],
                 "file_pattern":"^(?<n>[a-z0-9-]+)\\.use-case\\.ts$",
                 "must_export":{"kind":["function"],"name":"{{pascal(n)}}"}}]}"#,
        ),
        (
            "src/user/create-client.use-case.ts",
            "export function Wrong() {}\n",
        ),
    ]);
    let target = dir.path().join("src/user/create-client.use-case.ts");
    let target = target.to_str().expect("utf-8");

    archwarden()
        .current_dir(dir.path())
        .args(["hook", "claude-code"])
        .write_stdin(format!(
            r#"{{"tool_name":"Edit","tool_input":{{"file_path":"{target}",
               "old_string":"Wrong","new_string":"CreateClient"}}}}"#
        ))
        .assert()
        .success()
        .stdout("{}\n");
}

/// `spec_dirs` names a directory beside the file, one level. A path asks for
/// the rule to reach further than it says, and reaching further is how a
/// `spec-pair` rule stops reporting and starts looking like a fully-tested
/// repository. Issue #67.
#[test]
fn a_spec_dir_that_is_a_path_is_refused() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,"rules":[
            {"type":"spec-pair","id":"needs-spec","level":"error",
             "roots":["src/*"],"subfolders":["."],
             "spec_dirs":["__tests__/unit"]}]}"#,
    )]);

    archwarden()
        .current_dir(dir.path())
        .args(["config", "validate"])
        .assert()
        .code(2)
        .stderr(contains("directory names"));
}

/// And the whole thing end to end: a spec in the named directory satisfies the
/// rule, and the same spec one level deeper does not.
#[test]
fn a_spec_in_a_named_directory_satisfies_the_rule_through_the_cli() {
    const CONFIG: &str = r#"{"version":0,"rules":[
        {"type":"spec-pair","id":"needs-spec","level":"error",
         "roots":["src/*"],"subfolders":["."],"spec_dirs":["__tests__"]}]}"#;

    let named = repo(&[
        ("arch.config.json", CONFIG),
        ("src/user/create.ts", "export function create() {}\n"),
        (
            "src/user/__tests__/create.spec.ts",
            "it('works', () => {});\n",
        ),
    ]);
    archwarden()
        .current_dir(named.path())
        .args(["check"])
        .assert()
        .success();

    let deeper = repo(&[
        ("arch.config.json", CONFIG),
        ("src/user/create.ts", "export function create() {}\n"),
        (
            "src/user/__tests__/unit/create.spec.ts",
            "it('works', () => {});\n",
        ),
    ]);
    archwarden()
        .current_dir(deeper.path())
        .args(["check"])
        .assert()
        .code(1)
        .stdout(contains("needs-spec"));
}

/// Issue #57. A `presence` rule of several files made every one of them
/// illegal until all of them existed — no write order passed, and the
/// directory could not be created at all.
///
/// A write supplying one of the required files is fixing the directory, not
/// breaking it. The whole creation sequence goes through, and each write is
/// told what is still missing.
#[test]
fn a_module_can_be_created_one_file_at_a_time() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,"rules":[
            {"type":"presence","id":"tem-os-tres","level":"error",
             "roots":["projetos/*"],
             "require":["projeto.md","exercicios.md","diagram.json"]}]}"#,
    )]);
    std::fs::create_dir_all(dir.path().join("projetos/02-novo")).expect("mkdir");

    for name in ["projeto.md", "exercicios.md", "diagram.json"] {
        let target = dir.path().join("projetos/02-novo").join(name);
        let target = target.to_str().expect("utf-8");

        archwarden()
            .current_dir(dir.path())
            .args(["hook", "claude-code"])
            .write_stdin(format!(
                r#"{{"tool_name":"Write","tool_input":{{"file_path":"{target}","content":"x"}}}}"#
            ))
            .assert()
            .success()
            .stdout(contains("permissionDecision").not());

        // Allowed *and* told what is still missing. Silence here would let the
        // agent believe the directory was done.
        if name != "diagram.json" {
            archwarden()
                .current_dir(dir.path())
                .args(["hook", "claude-code"])
                .write_stdin(format!(
                    r#"{{"tool_name":"Write","tool_input":{{"file_path":"{target}","content":"x"}}}}"#
                ))
                .assert()
                .stdout(contains("not done yet"));
        }

        std::fs::write(target, "x").expect("write");
    }
}

/// And the half that keeps this from being a way to switch `presence` off: a
/// write that supplies none of the required files leaves the directory exactly
/// as broken as it found it, and is refused.
#[test]
fn a_write_that_ignores_the_missing_files_is_still_refused() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"presence","id":"tem-os-tres","level":"error",
                 "roots":["projetos/*"],
                 "require":["projeto.md","exercicios.md","diagram.json"]}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
    ]);
    let target = dir.path().join("projetos/01-blink/rascunho.md");
    let target = target.to_str().expect("utf-8");

    archwarden()
        .current_dir(dir.path())
        .args(["hook", "claude-code"])
        .write_stdin(format!(
            r#"{{"tool_name":"Write","tool_input":{{"file_path":"{target}","content":"x"}}}}"#
        ))
        .assert()
        .success()
        .stdout(contains("permissionDecision"));
}

/// Issue #61, and the relief for #57: the class of rule the pre-write hook
/// cannot judge is caught once the writes have landed.
///
/// A `presence` rule requiring three files makes every one of the three
/// illegal until the other two exist, so no write order passes and the module
/// cannot be created at all. At the end of the turn the group is there to be
/// judged, and what is missing is a fact rather than a prediction.
#[test]
fn the_stop_hook_reports_what_landed() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"presence","id":"tem-os-tres","level":"error",
                 "roots":["projetos/*"],
                 "require":["projeto.md","exercicios.md","diagram.json"]}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
    ]);
    git_init(dir.path());

    archwarden()
        .current_dir(dir.path())
        .args(["hook", "claude-code"])
        .write_stdin(r#"{"hook_event_name":"Stop","session_id":"abc"}"#)
        .assert()
        .success()
        .stdout(contains("landed in this turn"))
        .stdout(contains("exercicios.md"));
}

/// A finding the project already accepted is not reported at the end of a
/// turn either. `baseline` is debt the repository decided to carry, and a hook
/// that read it out every turn would be a hook somebody removes.
#[test]
fn the_stop_hook_honours_the_baseline() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"presence","id":"tem-os-tres","level":"error",
                 "roots":["projetos/*"],"require":["projeto.md","exercicios.md"]}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
        (
            ".archwarden/baseline.json",
            r#"{"version":0,"accepted":[
                {"rule":"tem-os-tres","path":"projetos/01-blink",
                 "note":"pre-existing"}]}"#,
        ),
    ]);
    git_init(dir.path());

    archwarden()
        .current_dir(dir.path())
        .args(["hook", "claude-code"])
        .write_stdin(r#"{"hook_event_name":"Stop"}"#)
        .assert()
        .success()
        .stdout("{}\n");
}

/// **The whole thesis of issue #102, end to end.** A `frozen` rule adds no
/// machinery of its own: it points `baseline` — which already records what a
/// repository has accepted, by rule and path — forward instead of back.
///
/// Three states in one test, because the value is in how they compose and no
/// unit test spans `baseline` and `check`:
///
/// - what is there when the freeze is declared is accepted, and `check` is
///   clean;
/// - a file added afterwards is reported;
/// - a file moved *out* is silent, which is the point of the freeze.
#[test]
fn a_freeze_accepts_what_is_there_and_reports_what_arrives() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"frozen","id":"legacy-is-closed","level":"error",
                 "roots":["packages/legacy/**"]}]}"#,
        ),
        ("packages/legacy/a.ts", "export const a = 1;\n"),
        ("packages/core/keep.ts", "export const keep = 1;\n"),
        (
            ".archwarden/baseline.json",
            r#"{"version":0,"accepted":[
                {"rule":"legacy-is-closed","path":"packages/legacy/a.ts",
                 "note":"here when the freeze was declared"}]}"#,
        ),
    ]);
    git_init(dir.path());

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .success();

    // A file added to the frozen tree is a path nobody accepted.
    std::fs::write(
        dir.path().join("packages/legacy/novo.ts"),
        "export const novo = 1;\n",
    )
    .expect("write");

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .failure()
        .stdout(predicates::str::contains("packages/legacy/novo.ts"));

    // Moved out, and the freeze has nothing to say: leaving is the point.
    std::fs::rename(
        dir.path().join("packages/legacy/novo.ts"),
        dir.path().join("packages/core/novo.ts"),
    )
    .expect("move");

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .success();
}

/// A turn that broke nothing says nothing. A hook that spoke every turn is one
/// somebody removes.
#[test]
fn the_stop_hook_is_silent_when_nothing_landed() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"presence","id":"tem-os-tres","level":"error",
                 "roots":["projetos/*"],"require":["projeto.md"]}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
    ]);
    git_init(dir.path());

    archwarden()
        .current_dir(dir.path())
        .args(["hook", "claude-code"])
        .write_stdin(r#"{"hook_event_name":"Stop"}"#)
        .assert()
        .success()
        .stdout("{}\n");
}

/// Issue #46, through the real process. A finding says what the rule wanted
/// and what the file did, and used to never say why the rule exists — so an
/// agent reading one could comply and nothing else, which is how a config gets
/// edited to make a check pass.
///
/// Once per rule, at its first occurrence: two findings, one paragraph.
#[test]
fn a_rules_reason_is_printed_once_beside_its_findings() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"structure","id":"referencia-sem-subpasta","level":"error",
                 "why":"the generated index reads this folder flat; a nested one is invisible to it",
                 "roots":["referencia"],"allowed_subfolders":[]}]}"#,
        ),
        ("referencia/uma/x.md", "# x\n"),
        ("referencia/outra/y.md", "# y\n"),
    ]);

    let output = archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).expect("output is UTF-8");

    assert_eq!(
        text.matches("why: the generated index reads this folder flat")
            .count(),
        1,
        "one paragraph per rule, not per finding: {text}"
    );
}

/// And it reaches the commands an agent asks *before* writing, which is where
/// a reason is worth most — a constraint that looks arbitrary is the one that
/// gets worked around.
#[test]
fn a_rules_reason_reaches_describe_and_the_guide() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"modules":[{
                 "id":"referencia",
                 "why":"it is the only part of this repository another project consumes",
                 "rules":[{"type":"structure","id":"referencia-sem-subpasta","level":"error",
                           "why":"the generated index reads this folder flat",
                           "roots":["referencia"],"allowed_subfolders":[]}]}]}"#,
        ),
        ("referencia/x.md", "# x\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["describe", "referencia", "--format", "json"])
        .assert()
        .success()
        .stdout(contains("the generated index reads this folder flat"))
        .stdout(contains("another project consumes"));

    archwarden()
        .current_dir(dir.path())
        .arg("agent-guide")
        .assert()
        .success()
        .stdout(contains(
            "**Why**: the generated index reads this folder flat",
        ));
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

/// Issue #112, end to end and through every surface it touches: a decision
/// that is accepted, named by a rule, reporting nothing, and never kept —
/// because everything it flags is in the baseline.
///
/// The three surfaces are asserted together deliberately. The value is in them
/// agreeing, and no unit test spans `baseline`, `explain` and the report.
#[test]
fn the_debt_a_decision_carries_is_named_on_every_surface() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,
                "decisions":[{"id":"ADR-014","title":"entities are flat",
                              "why":"a nested entity is a module nobody named"}],
                "rules":[{"type":"structure","id":"entity-shape","level":"error",
                          "decision":"ADR-014","roots":["src/*"],
                          "allowed_subfolders":["types"]}]}"#,
        ),
        ("src/order/handlers/a.ts", ""),
        ("src/billing/types/b.ts", ""),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("baseline")
        .assert()
        .success();

    // 1. The report, where the number is machine-readable and normalised
    //    under the baseline it belongs to.
    let stdout = archwarden()
        .current_dir(dir.path())
        .args(["check", "--format", "json"])
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let parsed: serde_json::Value = serde_json::from_slice(&stdout).expect("valid JSON");
    assert_eq!(
        parsed["summary"]["baseline"]["by_decision"]["ADR-014"]["accepted"],
        1
    );
    assert_eq!(
        parsed["summary"]["baseline"]["by_decision"]["ADR-014"]["gone"],
        0
    );

    // 2. `config explain`, where somebody asking about the decision is told
    //    the thing the rule list cannot say.
    archwarden()
        .current_dir(dir.path())
        .args(["config", "explain", "ADR-014"])
        .assert()
        .success()
        .stdout(contains("1 excused"))
        .stdout(contains("It has never refused anything."));

    // 3. The page, which until now could only say what was declared.
    archwarden()
        .current_dir(dir.path())
        .args(["agent-guide", "--format", "html"])
        .assert()
        .success()
        .stdout(contains(
            r#"<p class="excused">The baseline carries 1 entry against it.</p>"#,
        ));
}

/// Issue #116, end to end: the document archwarden writes, the region it never
/// Issue #144, end to end. React Server Components draw the sharpest
/// architectural boundary in the modern JavaScript ecosystem, and it is a
/// directive rather than a path -- two files in the same directory, importing
/// the same module, on opposite sides of it.
#[test]
fn a_rule_can_narrow_by_what_a_file_declares_about_itself() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"import-boundary","id":"a-client-component-cannot-reach-the-database",
                 "level":"error","from":["app/*"],
                 "when_declaring":["use client"],
                 "forbid_import_from":["src/db/**"]},
                {"type":"import-boundary","id":"a-server-component-cannot-use-a-browser-package",
                 "level":"error","from":["app/*"],
                 "when_not_declaring":["use client"],
                 "forbid_import_from_packages":["react-dom"]}]}"#,
        ),
        ("src/db/client.ts", "export const db = 1;"),
        // A client component reaching the database: a credential in the bundle.
        (
            "app/dashboard/chart.tsx",
            "\"use client\";\nimport { db } from \"../../src/db/client\";\n",
        ),
        // A server component reaching a browser-only package. Same directory,
        // and nothing about where it sits tells the two apart.
        (
            "app/dashboard/page.tsx",
            "import { createPortal } from \"react-dom\";\n",
        ),
        // And a client component using that package is exactly right.
        (
            "app/dashboard/modal.tsx",
            "\"use client\";\nimport { createPortal } from \"react-dom\";\n",
        ),
    ]);

    // Read as findings rather than as text: `react-dom` is not installed in
    // this fixture, so every file that imports it appears in the unresolved
    // note -- and a rule not firing is the thing being asserted.
    let out = archwarden()
        .current_dir(dir.path())
        .args(["check", "--format", "json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&out).expect("valid JSON");
    let reported: Vec<String> = report["findings"]
        .as_array()
        .expect("an array")
        .iter()
        .map(|finding| {
            format!(
                "{} {}",
                finding["rule_id"].as_str().unwrap_or_default(),
                finding["path"].as_str().unwrap_or_default()
            )
        })
        .collect();

    assert_eq!(
        reported,
        [
            "a-client-component-cannot-reach-the-database app/dashboard/chart.tsx",
            "a-server-component-cannot-use-a-browser-package app/dashboard/page.tsx",
        ],
        "a client component may use a browser package, and only these two are wrong"
    );
}

/// Issue #166, reported from a repository that put `config doctor` in its
/// gate and watched it pass green for two commits with a stale decision
/// document hanging off it.
///
/// A command that never fails guards nothing. Printing the word `error` and
/// returning success is the incoherence: the word is a promise.
#[test]
fn config_doctor_fails_on_what_it_calls_an_error() {
    // A decision claiming nothing can keep it, with a rule that does: the
    // config says two things at once, which `doctor` reports as an error.
    let contradictory = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,
            "decisions":[{"id":"ADR-014","title":"A wall","enforcement":"none",
                          "why_not_enforceable":"no parser sees a review"}],
            "rules":[{"type":"presence","id":"has-a-readme","level":"error",
                      "decision":"ADR-014",
                      "roots":["src/*"],"require":["README.md"]}]}"#,
        ),
        ("src/api/README.md", "# api"),
    ]);

    archwarden()
        .current_dir(contradictory.path())
        .args(["config", "doctor"])
        .assert()
        // Two, not one: this is "your config is wrong", which is a different
        // thing from "your code violates your config".
        .code(2)
        .stdout(contains("unenforceable-but-a-rule-keeps-it"));

    // A warning alone is still clean, so a repository is not failed for
    // something archwarden itself calls a warning.
    let warned = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,
            "decisions":[{"id":"ADR-014","title":"A wall"}],
            "rules":[{"type":"presence","id":"has-a-readme","level":"error",
                      "roots":["src/*"],"require":["README.md"]}]}"#,
        ),
        ("src/api/README.md", "# api"),
    ]);

    archwarden()
        .current_dir(warned.path())
        .args(["config", "doctor"])
        .assert()
        .success()
        .stdout(contains("decision-nobody-enforces"));

    // And `--strict` is for a gate that wants every concern to block, which
    // is what the reporter's own pipeline meant.
    archwarden()
        .current_dir(warned.path())
        .args(["config", "doctor", "--strict"])
        .assert()
        .code(2)
        .stdout(contains("decision-nobody-enforces"));

    // A clean config is clean either way.
    let clean = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[{"type":"presence","id":"has-a-readme",
            "level":"error","roots":["src/*"],"require":["README.md"]}]}"#,
        ),
        ("src/api/README.md", "# api"),
    ]);

    archwarden()
        .current_dir(clean.path())
        .args(["config", "doctor", "--strict"])
        .assert()
        .success()
        .stdout(contains("No concerns"));
}

/// Issue #162. The line archwarden could not say: *has this already been
/// rejected?* -- asked by somebody who does not know the decision's id and
/// names the option differently from whoever rejected it.
///
/// End to end rather than as a unit test, because the value is in the whole
/// path: a bilingual config on disk, a query with no accents typed by someone
/// who never read it, and an answer that says why it matched.
#[test]
fn a_rejected_option_is_found_under_a_name_nobody_wrote() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,
            "decisions":[{"id":"ADR-001","title":"Quatro camadas, mais o System",
                          "alternatives":[{"option":"uma única camada",
                                           "why_not":"o domínio importaria o transporte"}]}],
            "rules":[]}"#,
    )]);

    archwarden()
        .current_dir(dir.path())
        .args(["decisions", "find", "camada", "unica"])
        .assert()
        .success()
        .stdout(contains("ADR-001 — Quatro camadas, mais o System"))
        .stdout(contains("alternatives[0].option"))
        // And why it matched, which is what a reader adjusts the query by.
        .stdout(contains("`camada` prefix of `camadas`"))
        .stdout(contains("`unica` exact"))
        .stdout(contains("2 places mention"));

    // One reads as one. A count sentence that says "1 places mention" is the
    // kind of thing a reader stops trusting the rest of.
    archwarden()
        .current_dir(dir.path())
        .args(["decisions", "find", "transporte"])
        .assert()
        .success()
        .stdout(contains("1 place mentions"));

    // The same answer as data, in the shape the MCP tool answers with.
    let json = archwarden()
        .current_dir(dir.path())
        .args(["decisions", "find", "transporte", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let parsed: serde_json::Value =
        serde_json::from_slice(&json).expect("the JSON format emits JSON");
    assert_eq!(parsed["query"], "transporte");
    assert_eq!(parsed["hits"][0]["decision"], "ADR-001");
    assert_eq!(parsed["hits"][0]["at"], "alternatives[0].why_not");
    assert_eq!(parsed["hits"][0]["reasons"][0]["how"], "exact");

    // Nothing found is an answer, not a failure: a command somebody runs to
    // ask a question must not fail them for asking.
    archwarden()
        .current_dir(dir.path())
        .args(["decisions", "find", "graphql"])
        .assert()
        .success()
        .stdout(contains("Nothing here has been said about `graphql`"));

    // And `decisions` with no subcommand still writes, which is what every
    // script calling it already does.
    archwarden()
        .current_dir(dir.path())
        .arg("decisions")
        .assert()
        .success()
        .stdout(contains("wrote 1 document"));
}

/// rewrites, and the drift `config doctor` reports when the config moves on.
///
/// Asserted as one sequence because the value is in the three agreeing, and no
/// unit test spans a command, a hand edit, and the doctor.
#[test]
fn a_decision_document_is_written_kept_and_reported_when_it_falls_behind() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,
            "decisions":[{"id":"ADR-031","title":"the domain does not know about transport",
                          "why":"it is published",
                          "alternatives":[{"option":"an HTTP client in the domain",
                                           "why_not":"a consumer would inherit our transport"}]}],
            "rules":[]}"#,
    )]);

    // 1. It writes one document per decision, and says so.
    archwarden()
        .current_dir(dir.path())
        .arg("decisions")
        .assert()
        .success()
        .stdout(contains(".archwarden/decisions/ADR-031.md"))
        .stdout(contains("wrote 1 document"));

    let path = dir.path().join(".archwarden/decisions/ADR-031.md");
    let written = std::fs::read_to_string(&path).expect("the document is there");
    assert!(
        written.contains("# the domain does not know about transport"),
        "{written}"
    );
    assert!(written.contains("> it is published"), "{written}");
    assert!(
        written.contains("- **an HTTP client in the domain** (nothing refuses it)"),
        "{written}"
    );

    // 2. Running it again changes nothing, which is what makes it safe to
    //    leave in a script.
    archwarden()
        .current_dir(dir.path())
        .arg("decisions")
        .assert()
        .success()
        .stdout(contains("up to date"));

    // 3. What a person writes between the markers survives a regeneration.
    let edited = written.replace(
        "## Context",
        "## Context\n\nThree services shared the order model.",
    );
    std::fs::write(&path, edited).expect("write");
    archwarden()
        .current_dir(dir.path())
        .arg("decisions")
        .assert()
        .success();
    let again = std::fs::read_to_string(&path).expect("still there");
    assert!(
        again.contains("Three services shared the order model."),
        "regenerating must never eat what a person wrote: {again}"
    );

    // 4. And when the config moves on, the doctor says so — as advice, with a
    //    clean exit, because a document needing regeneration is not a
    //    violation of anything.
    std::fs::write(
        dir.path().join("arch.config.json"),
        r#"{"version":0,
            "decisions":[{"id":"ADR-031","title":"a different title now"}],
            "rules":[]}"#,
    )
    .expect("write");

    archwarden()
        .current_dir(dir.path())
        .args(["config", "doctor"])
        .assert()
        .code(0)
        .stdout(contains("decision-document-out-of-date"))
        .stdout(contains("archwarden decisions"));
}

/// And `--dry-run` says what would change and writes nothing, the shape
/// `baseline --dry-run` already has.
#[test]
fn a_decisions_dry_run_writes_nothing() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,"decisions":[{"id":"ADR-1","title":"a wall"}],"rules":[]}"#,
    )]);

    archwarden()
        .current_dir(dir.path())
        .args(["decisions", "--dry-run"])
        .assert()
        .success()
        .stdout(contains("Nothing was written."));

    assert!(
        !dir.path().join(".archwarden/decisions/ADR-1.md").exists(),
        "a dry run that wrote would be the bug it exists to prevent"
    );
}

/// A configuration with no decisions is every configuration written before
/// 0.21, and the command says so rather than writing an empty directory.
#[test]
fn a_config_with_no_decisions_writes_no_documents() {
    let dir = repo(&[("arch.config.json", r#"{"version":0,"rules":[]}"#)]);

    archwarden()
        .current_dir(dir.path())
        .arg("decisions")
        .assert()
        .success()
        .stdout(contains("declares no decisions"));

    assert!(!dir.path().join(".archwarden/decisions").exists());
}

/// Issue #117, end to end: a date a file wrote down, compared against the day
/// the run was given. `metadata` could record a removal date since 0.24 and
/// nothing compared it to anything — the difference between a migration and a
/// wish.
#[test]
fn a_deadline_is_measured_against_the_day_the_run_was_given() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"metadata","id":"experiments-expire","level":"error",
                 "why":"an experiment with no end is a feature nobody decided to keep",
                 "roots":["src/**"],"require":["remove-by"],"deadline":["remove-by"]}]}"#,
        ),
        (
            "src/payments/beta-checkout.ts",
            "// archwarden-remove-by: 2026-12-01
export const beta = () => 1;
",
        ),
    ]);

    // Before the date, it holds — and the same repository, the same config.
    archwarden()
        .current_dir(dir.path())
        .args(["check", "--as-of", "2026-11-30"])
        .assert()
        .success();

    // The day itself is met, not missed: a rule that fired here would fire a
    // day early for everybody.
    archwarden()
        .current_dir(dir.path())
        .args(["check", "--as-of", "2026-12-01"])
        .assert()
        .success();

    // And after it, with how long ago — a deadline that slipped yesterday and
    // one that slipped a year ago are different conversations.
    archwarden()
        .current_dir(dir.path())
        .args(["check", "--as-of", "2027-01-15"])
        .assert()
        .code(1)
        .stdout(contains("`remove-by` was `2026-12-01`, 45 days ago"))
        .stdout(contains("why: an experiment with no end"));
}

/// A value that is not a date is its own finding rather than a guess.
/// `01/12/2026` read leniently is eleven months from where it was meant to be.
#[test]
fn a_deadline_that_is_not_a_date_says_so() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"metadata","id":"experiments-expire","level":"error",
                 "roots":["src/**"],"deadline":["remove-by"]}]}"#,
        ),
        (
            "src/a.ts",
            "// archwarden-remove-by: 01/12/2026
export const a = 1;
",
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["check", "--as-of", "2027-01-15"])
        .assert()
        .code(1)
        .stdout(contains(
            "`remove-by` is `01/12/2026`, which is not a date — write `YYYY-MM-DD`",
        ));
}

/// And the flag itself is refused before the walk, for the same reason: a
/// lenient `--as-of` would answer confidently for the wrong day.
#[test]
fn an_as_of_that_is_not_a_date_is_refused_before_the_run() {
    let dir = repo(&[("arch.config.json", r#"{"version":0,"rules":[]}"#)]);

    archwarden()
        .current_dir(dir.path())
        .args(["check", "--as-of", "next tuesday"])
        .assert()
        .code(2)
        .stderr(contains("is not a date; write it as `YYYY-MM-DD`"));
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

/// Issue #110. In `--format json`, stdout is the document and nothing else.
///
/// The baseline standing and the `--html` note were both written to stdout
/// after the object, unconditionally, so a repository with a baseline — which
/// is every repository that adopted archwarden after its code existed — handed
/// every consumer a document with trailing text after it. `AGENTS.md` tells an
/// agent to use this format instead of parsing the prose, so the path the
/// documentation calls the tool path was the broken one.
///
/// Both writers are asserted here, and the standing is asserted *inside* the
/// document: moving it to stderr would have fixed the parse and lost the number
/// the baseline exists to keep in front of somebody.
#[test]
fn the_json_report_is_the_whole_of_stdout() {
    let dir = repo_with_violations();

    archwarden()
        .current_dir(dir.path())
        .arg("baseline")
        .assert()
        .success();

    let run = archwarden()
        .current_dir(dir.path())
        .args(["check", "--format", "json", "--html", "page.html"])
        .assert()
        .code(0);
    let output = run.get_output();

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is the document and nothing else");

    assert_eq!(parsed["summary"]["baseline"]["accepted"], 3);
    assert_eq!(
        parsed["summary"]["baseline"]["gone"], 0,
        "present at zero: a consumer branching on the ratchet needs the field"
    );

    // The note about the side artefact is still made, on the stream that is
    // not the contract.
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8");
    assert!(stderr.contains("page written to page.html"), "{stderr}");
}

/// And the text format is untouched: the standing still reads as a sentence
/// under the counts, which is where somebody at a terminal is reminded that a
/// baseline nobody looks at is a suppression file.
#[test]
fn the_text_report_still_says_where_the_run_stands() {
    let dir = repo_with_violations();

    archwarden()
        .current_dir(dir.path())
        .arg("baseline")
        .assert()
        .success();

    archwarden()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(0)
        .stdout(contains("3 accepted"));
}

/// A run with no baseline carries no `baseline` key at all, so a consumer can
/// tell "this repository accepts nothing" from "nothing is accepted" — the
/// distinction `summary.imports` already draws for resolution.
#[test]
fn a_run_without_a_baseline_says_nothing_about_one() {
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

    assert!(parsed["summary"].get("baseline").is_none(), "{parsed}");
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

/// A `presence` rule answers for the directory, so no file being subject to it
/// is its ordinary state and not a symptom.
///
/// Issue #51: `doctor` reported `rule-evaluates-nothing` for every `presence`
/// rule in a config, while `check` fired those same rules on the same state.
/// Two commands in one binary disagreeing about whether a rule does anything.
///
/// The advice made it worse than the false positive did. Widening `roots` from
/// `projetos/*` to `projetos/**` as suggested would ask every subdirectory of
/// every lesson to hold the three required files — turning a working rule into
/// sixteen false errors.
#[test]
fn doctor_does_not_call_a_directory_rule_idle() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"presence","id":"projeto-tem-os-tres","level":"error",
                 "roots":["projetos/*"],
                 "require":["projeto.md","exercicios.md","diagram.json"]}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
        ("projetos/01-blink/exercicios.md", "# exercicios\n"),
        ("projetos/01-blink/diagram.json", "{}\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["config", "doctor"])
        .assert()
        .success()
        .stdout(contains("No concerns."));
}

/// And it still bites, which is the other half of the same claim.
#[test]
fn a_directory_rule_doctor_stays_quiet_about_still_fires() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"presence","id":"projeto-tem-os-tres","level":"error",
                 "roots":["projetos/*"],
                 "require":["projeto.md","exercicios.md","diagram.json"]}]}"#,
        ),
        ("projetos/01-blink/projeto.md", "# blink\n"),
        ("projetos/01-blink/diagram.json", "{}\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["check"])
        .assert()
        .code(1)
        .stdout(contains("exercicios.md"));
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

// ---------------------------------------------------------------------------
// The drift guard
// ---------------------------------------------------------------------------
//
// Issue #55 was not a bug in the version guard. The guard was correct and the
// pre-write hook never reached it, because that surface had grown a copy of the
// loading path — it had to, since the shared `prepare()` answered by writing a
// miette report to stderr and a hook must answer in JSON and exit clean. A
// config from a future version parsed into one with no rules, compiled, matched
// nothing, and permitted every write. The gate did not fail; it evaporated.
//
// Decision 20 removed the reason to copy the path. Nothing yet checks that no
// surface copies it anyway, and milestone 0.18 adds three more surfaces.
//
// The unit tests on `unexamined()` cannot close this: a surface with its own
// loading path would never call it, and every one of those tests would stay
// green while the gate evaporated. Only driving each surface end to end, from
// the outside, against a repository whose config this build cannot read, can
// tell the difference.
//
// Each surface is tested in a pair. The version-0 half proves the surface does
// the thing at all; the version-99 half proves it stops. Without the first, the
// second passes for a surface that never worked.

/// One rule, in one module, at whatever version the caller names.
///
/// A `presence` rule because it fires on a directory rather than on a file's
/// contents, so the same config is answerable by every surface here: the
/// pre-write hook judges a write into the directory, the stop hook judges what
/// landed, and the session hook only ever reads the module's own declaration.
fn governed_at(version: u32) -> String {
    format!(
        r#"{{"version":{version},
            "modules":[{{"id":"projetos",
              "why":"one exercise per folder, and all three files in it",
              "scope":["projetos/*"],
              "rules":[{{"type":"presence","id":"tem-os-tres","level":"error",
                "roots":["projetos/*"],
                "require":["projeto.md","exercicios.md","diagram.json"]}}]}}]}}"#
    )
}

/// The pre-write hook, both halves.
///
/// This is issue #55's exact shape, asserted from outside the process for the
/// first time. The write below supplies none of the three required files, so
/// under a config this build understands it is refused.
#[test]
fn the_pre_write_hook_stops_at_a_config_from_a_future_version() {
    let readable = repo(&[("arch.config.json", &governed_at(0))]);
    let target = readable.path().join("projetos/01-blink/notas.md");
    let payload = format!(
        r#"{{"tool_name":"Write","tool_input":{{"file_path":"{}","content":"notas\n"}}}}"#,
        target.to_str().expect("utf-8")
    );

    archwarden()
        .current_dir(readable.path())
        .args(["hook", "claude-code"])
        .write_stdin(payload.clone())
        .assert()
        .success()
        .stdout(contains("permissionDecision"));

    // The same write, the same rule, one digit different in the config.
    let future = repo(&[("arch.config.json", &governed_at(99))]);
    let target = future.path().join("projetos/01-blink/notas.md");
    let payload = format!(
        r#"{{"tool_name":"Write","tool_input":{{"file_path":"{}","content":"notas\n"}}}}"#,
        target.to_str().expect("utf-8")
    );

    archwarden()
        .current_dir(future.path())
        .args(["hook", "claude-code"])
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(contains("archwarden did not check this write"))
        .stdout(contains("version 99"))
        // The half that matters: it must not have formed an opinion either way.
        .stdout(contains("permissionDecision").not());
}

/// The stop hook, both halves.
///
/// Silence is this surface's correct answer to a config it cannot read, which
/// makes it the surface where a missing guard is hardest to see: a build with
/// no guard is silent too, having compiled a config it did not understand into
/// rules that found nothing. The version-0 half is what makes the silence mean
/// something — it fixes that this repository, this config and this turn do
/// produce a report.
#[test]
fn the_stop_hook_stops_at_a_config_from_a_future_version() {
    let readable = git_repo(&[
        ("arch.config.json", &governed_at(0)),
        ("projetos/01-blink/projeto.md", "# blink\n"),
    ]);
    std::fs::write(
        readable.path().join("projetos/01-blink/outro.md"),
        "# outro\n",
    )
    .expect("write");

    archwarden()
        .current_dir(readable.path())
        .args(["hook", "claude-code"])
        .write_stdin(r#"{"hook_event_name":"Stop","session_id":"abc"}"#)
        .assert()
        .success()
        .stdout(contains("exercicios.md"));

    let future = git_repo(&[
        ("arch.config.json", &governed_at(99)),
        ("projetos/01-blink/projeto.md", "# blink\n"),
    ]);
    std::fs::write(
        future.path().join("projetos/01-blink/outro.md"),
        "# outro\n",
    )
    .expect("write");

    archwarden()
        .current_dir(future.path())
        .args(["hook", "claude-code"])
        .write_stdin(r#"{"hook_event_name":"Stop","session_id":"abc"}"#)
        .assert()
        .success()
        .stdout(contains("exercicios.md").not());
}

/// The session hook, both halves. Issue #66.
///
/// The module map is a pointer rather than the guide, so what proves it ran is
/// the module's own id and the sentence its author wrote about it.
#[test]
fn the_session_start_hook_stops_at_a_config_from_a_future_version() {
    let readable = repo(&[("arch.config.json", &governed_at(0))]);

    archwarden()
        .current_dir(readable.path())
        .args(["hook", "claude-code"])
        .write_stdin(r#"{"hook_event_name":"SessionStart","source":"compact"}"#)
        .assert()
        .success()
        .stdout(contains("additionalContext"))
        .stdout(contains("projetos"))
        .stdout(contains("one exercise per folder"));

    let future = repo(&[("arch.config.json", &governed_at(99))]);

    archwarden()
        .current_dir(future.path())
        .args(["hook", "claude-code"])
        .write_stdin(r#"{"hook_event_name":"SessionStart","source":"compact"}"#)
        .assert()
        .success()
        // No map, and not in silence either: a session told nothing is a
        // session that cannot tell "no rules here" from "archwarden is broken".
        .stdout(contains("one exercise per folder").not())
        .stdout(contains("version 99"));
}

// ---------------------------------------------------------------------------
// MCP, over stdio
// ---------------------------------------------------------------------------
//
// Issue #65. Not HTTP: the client spawns the binary and speaks JSON-RPC over
// its pipes. No port, no daemon, nothing listening.
//
// The tools are the operations that already exist, and the server owns none of
// them — which is what `archwarden-mcp` depending on `archwarden-api` and never
// on `archwarden-cli` is there to make structural rather than reviewed.

/// One JSON-RPC request per line, which is what the stdio transport is.
///
/// Each request is re-serialised compactly on the way in. The transport is
/// line-delimited and the requests below are written across several lines to
/// stay readable, which are two things that cannot both be true of the same
/// bytes.
fn rpc(dir: &std::path::Path, requests: &[&str]) -> Vec<serde_json::Value> {
    let line_delimited: Vec<String> = requests
        .iter()
        .map(|request| {
            serde_json::from_str::<serde_json::Value>(request)
                .expect("each request is valid JSON")
                .to_string()
        })
        .collect();

    let output = archwarden()
        .current_dir(dir)
        .args(["mcp"])
        .write_stdin(format!("{}\n", line_delimited.join("\n")))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    String::from_utf8(output)
        .expect("utf-8")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("each line is one JSON-RPC message"))
        .collect()
}

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;

/// The handshake, and that the server names itself and its version.
#[test]
fn the_mcp_server_initialises_over_stdio() {
    let dir = repo(&[("arch.config.json", &governed_at(0))]);

    let replies = rpc(dir.path(), &[INITIALIZE]);

    assert_eq!(replies[0]["jsonrpc"], "2.0");
    assert_eq!(replies[0]["id"], 1);
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "archwarden");
    assert!(
        replies[0]["result"]["capabilities"]["tools"].is_object(),
        "it offers tools: {}",
        replies[0]
    );
}

/// The tools are the operations, and `check_write` is the one that earns the
/// server: it exists today and is reachable only through the hook, which means
/// only reactively — the agent writes, and is denied.
#[test]
fn the_tools_are_the_operations_that_already_exist() {
    let dir = repo(&[("arch.config.json", &governed_at(0))]);

    let replies = rpc(
        dir.path(),
        &[
            INITIALIZE,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        ],
    );

    let names: Vec<&str> = replies[1]["result"]["tools"]
        .as_array()
        .expect("a list of tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();

    assert!(names.contains(&"check_write"), "{names:?}");
    assert!(names.contains(&"describe"), "{names:?}");
    assert!(names.contains(&"scaffold"), "{names:?}");

    for tool in replies[1]["result"]["tools"].as_array().expect("a list") {
        assert!(
            tool["inputSchema"]["type"] == "object",
            "every tool declares its arguments: {tool}"
        );
    }
}

/// The question the server exists to answer: *would this content pass?*, asked
/// before the write rather than after it.
#[test]
fn check_write_answers_before_the_write_rather_than_after() {
    let dir = repo(&[("arch.config.json", &governed_at(0))]);

    let replies = rpc(
        dir.path(),
        &[
            INITIALIZE,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"check_write",
               "arguments":{"path":"projetos/01-blink/notas.md","content":"notas\n"}}}"#,
        ],
    );

    let text = replies[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("the answer is text");
    let answer: serde_json::Value = serde_json::from_str(text).expect("carrying JSON");

    assert_eq!(
        answer["refused"], true,
        "this write supplies none of the three required files: {answer}"
    );
    assert!(
        answer["findings"].as_array().is_some_and(|f| !f.is_empty()),
        "and says which rule: {answer}"
    );
    // Nothing was written. The whole point is that the agent can ask first.
    assert!(!dir.path().join("projetos/01-blink/notas.md").exists());
}

/// And the same tool permits a write that is fixing the directory rather than
/// breaking it — because it goes through the same operation the hook does.
/// Two surfaces answering one question differently is this milestone's risk.
#[test]
fn check_write_and_the_pre_write_hook_agree() {
    let dir = repo(&[("arch.config.json", &governed_at(0))]);
    let target = dir.path().join("projetos/01-blink/projeto.md");

    let replies = rpc(
        dir.path(),
        &[
            INITIALIZE,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"check_write",
               "arguments":{"path":"projetos/01-blink/projeto.md","content":"blink\n"}}}"#,
        ],
    );
    let text = replies[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("the answer is text");
    let answer: serde_json::Value = serde_json::from_str(text).expect("carrying JSON");

    assert_eq!(
        answer["refused"], false,
        "a write supplying a required file is progress: {answer}"
    );

    // The hook, asked the same question about the same write, permits it too.
    archwarden()
        .current_dir(dir.path())
        .args(["hook", "claude-code"])
        .write_stdin(format!(
            r#"{{"tool_name":"Write","tool_input":{{"file_path":"{}","content":"blink\n"}}}}"#,
            target.to_str().expect("utf-8")
        ))
        .assert()
        .success()
        .stdout(contains("permissionDecision").not());
}

/// `describe` through MCP is the same envelope `describe --format json` emits.
/// Asserted against each other rather than against a copy of the shape, which
/// is the only way the assertion cannot drift with them.
#[test]
fn describe_through_mcp_is_the_envelope_the_command_prints() {
    let dir = repo(&[("arch.config.json", &governed_at(0))]);

    let replies = rpc(
        dir.path(),
        &[
            INITIALIZE,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"describe",
               "arguments":{"path":"projetos/01-blink"}}}"#,
        ],
    );
    let through_mcp: serde_json::Value = serde_json::from_str(
        replies[1]["result"]["content"][0]["text"]
            .as_str()
            .expect("the answer is text"),
    )
    .expect("carrying JSON");

    let printed = archwarden()
        .current_dir(dir.path())
        .args(["describe", "projetos/01-blink", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let through_cli: serde_json::Value =
        serde_json::from_slice(&printed).expect("the command prints JSON");

    assert_eq!(through_mcp, through_cli);
}

/// The server is long-lived, so it must re-read the configuration on each call
/// rather than cache it at startup — or it answers from a config the user has
/// since edited. That is issue #55 again, in a new place.
#[test]
fn the_server_re_reads_the_config_on_every_call() {
    let dir = repo(&[("arch.config.json", &governed_at(0))]);
    let loosened = dir.path().join("arch.config.json");

    // Two calls in one session, with the config rewritten between them. The
    // second must be answered against what is on disk *now*.
    let output = archwarden()
        .current_dir(dir.path())
        .args(["mcp"])
        .write_stdin(format!(
            "{INITIALIZE}\n{}\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"describe","arguments":{"path":"projetos/01-blink"}}}"#
        ))
        .assert()
        .success();
    let first = String::from_utf8(output.get_output().stdout.clone()).expect("utf-8");
    assert!(first.contains("tem-os-tres"), "{first}");

    std::fs::write(&loosened, r#"{"version":0,"rules":[]}"#).expect("rewrite the config");

    let output = archwarden()
        .current_dir(dir.path())
        .args(["mcp"])
        .write_stdin(format!(
            "{INITIALIZE}\n{}\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"describe","arguments":{"path":"projetos/01-blink"}}}"#
        ))
        .assert()
        .success();
    let second = String::from_utf8(output.get_output().stdout.clone()).expect("utf-8");
    assert!(
        !second.contains("tem-os-tres"),
        "answered from a config the user has since edited: {second}"
    );
}

/// The drift guard's fifth surface. A config this build cannot read must not
/// be answered from — an MCP server that compiled a future config into no
/// rules would tell an agent every write is fine.
#[test]
fn every_mcp_tool_stops_at_a_config_from_a_future_version() {
    let dir = repo(&[("arch.config.json", &governed_at(99))]);

    let replies = rpc(
        dir.path(),
        &[
            INITIALIZE,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"check_write",
               "arguments":{"path":"projetos/01-blink/notas.md","content":"notas\n"}}}"#,
        ],
    );

    assert_eq!(
        replies[1]["result"]["isError"], true,
        "a question it cannot answer is an error, not an answer: {}",
        replies[1]
    );
    let text = replies[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("the answer is text");
    assert!(text.contains("version 99"), "{text}");
}

/// A method this build has never heard of is refused in the protocol's own
/// terms rather than by dying, which would take the client's session with it.
#[test]
fn an_unknown_method_is_a_json_rpc_error_and_not_a_crash() {
    let dir = repo(&[("arch.config.json", &governed_at(0))]);

    let replies = rpc(
        dir.path(),
        &[
            INITIALIZE,
            r#"{"jsonrpc":"2.0","id":2,"method":"nothing/here"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
        ],
    );

    assert_eq!(replies[1]["error"]["code"], -32601, "method not found");
    assert!(
        replies[2]["result"]["tools"].is_array(),
        "and the server is still answering afterwards"
    );
}

/// A notification carries no `id` and takes no reply. Answering one is a
/// protocol violation that some clients treat as fatal.
///
/// What follows `notifications/initialized` is not a reply to it: it is this
/// server asking the client where the repository is, which is a request of its
/// own with an id of its own. Issue #93, decision 24.
#[test]
fn a_notification_is_not_answered() {
    let dir = repo(&[("arch.config.json", &governed_at(0))]);

    let replies = rpc(
        dir.path(),
        &[
            INITIALIZE,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
        ],
    );

    assert_eq!(replies.len(), 3, "{replies:?}");
    assert_eq!(replies[0]["id"], 1, "the handshake");
    assert!(
        replies[1].get("id").is_some() && replies[1]["method"] == "roots/list",
        "a question, not a reply: {}",
        replies[1]
    );
    assert_eq!(replies[2]["id"], 3, "and the request after it");
}

/// The whole of issue #93 through MCP, from outside the process: the client's
/// path, our root, one file, and a verdict instead of a shrug.
#[test]
fn a_client_that_names_the_repository_differently_is_still_answered() {
    let dir = repo(&[("arch.config.json", &governed_at(0))]);
    std::fs::create_dir_all(dir.path().join("projetos/01-blink")).expect("create");

    let replies = rpc(
        dir.path(),
        &[
            INITIALIZE,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":-1,"result":{"roots":[{"uri":"file:///home/dev/projeto"}]}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"check_write",
               "arguments":{"path":"/home/dev/projeto/projetos/01-blink/notas.md","content":"notas"}}}"#,
        ],
    );

    let answered = replies.last().expect("an answer");
    assert_ne!(
        answered["result"]["isError"], true,
        "it answered instead of shrugging: {answered}"
    );
    let judged: serde_json::Value = serde_json::from_str(
        answered["result"]["content"][0]["text"]
            .as_str()
            .expect("text"),
    )
    .expect("carrying JSON");

    assert_eq!(judged["path"], "projetos/01-blink/notas.md");
    assert_eq!(judged["refused"], true, "{judged}");
}

/// The MCP server needs a `.mcp.json` naming the command, and the installer is
/// where a user already goes to wire archwarden into Claude Code.
///
/// Committable, so it travels with the project — and it names the same
/// `./node_modules/.bin/archwarden` the pre-write hook resolves, because MCP
/// adds no new installation requirement.
#[test]
fn install_hooks_writes_a_committable_mcp_json() {
    let dir = repo(&[("arch.config.json", MINIMAL)]);

    archwarden()
        .current_dir(dir.path())
        .args(["install-hooks", "--claude-code"])
        .assert()
        .success();

    let written = std::fs::read_to_string(dir.path().join(".mcp.json")).expect(".mcp.json exists");
    let parsed: serde_json::Value = serde_json::from_str(&written).expect("valid JSON");

    assert_eq!(parsed["mcpServers"]["archwarden"]["command"], "archwarden");
    assert_eq!(parsed["mcpServers"]["archwarden"]["args"][0], "mcp");
}

/// A second run changes nothing, so it does not appear in `git status` for
/// nothing — the same promise the settings half already makes.
#[test]
fn installing_the_mcp_server_twice_changes_nothing() {
    let dir = repo(&[("arch.config.json", MINIMAL)]);
    let mcp_json = dir.path().join(".mcp.json");

    archwarden()
        .current_dir(dir.path())
        .args(["install-hooks", "--claude-code"])
        .assert()
        .success();
    let first = std::fs::read_to_string(&mcp_json).expect("written");

    archwarden()
        .current_dir(dir.path())
        .args(["install-hooks", "--claude-code"])
        .assert()
        .success();

    assert_eq!(
        first,
        std::fs::read_to_string(&mcp_json).expect("still there")
    );
}

/// And it is the user's file: another server declared beside ours survives
/// both installing and removing.
#[test]
fn another_mcp_server_in_the_file_is_left_alone() {
    let dir = repo(&[
        ("arch.config.json", MINIMAL),
        (
            ".mcp.json",
            r#"{"mcpServers":{"theirs":{"command":"their-server","args":[]}}}"#,
        ),
    ]);

    archwarden()
        .current_dir(dir.path())
        .args(["install-hooks", "--claude-code"])
        .assert()
        .success();

    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join(".mcp.json")).expect("read"))
            .expect("valid JSON");
    assert_eq!(parsed["mcpServers"]["theirs"]["command"], "their-server");
    assert!(parsed["mcpServers"]["archwarden"].is_object());

    archwarden()
        .current_dir(dir.path())
        .args(["install-hooks", "--claude-code", "--remove"])
        .assert()
        .success();

    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join(".mcp.json")).expect("read"))
            .expect("valid JSON");
    assert_eq!(parsed["mcpServers"]["theirs"]["command"], "their-server");
    assert!(
        parsed["mcpServers"].get("archwarden").is_none(),
        "ours is gone and theirs is not: {parsed}"
    );
}

/// The session hook is installed with **no matcher**, and that is the whole
/// decision.
///
/// `SessionStart` fires with a `source`, and a matcher is compared against it:
/// this build's Claude Code takes `startup`, `resume`, `clear`, `compact` and
/// `fork`. An entry naming three of them installs cleanly and silently covers
/// half the sessions — which is this project's recurring shape of failure, and
/// exactly what issue #66 warns about.
///
/// An omitted matcher cannot miss one, including a source added after this was
/// written.
#[test]
fn the_session_hook_is_installed_without_a_matcher() {
    let dir = repo(&[("arch.config.json", MINIMAL)]);

    archwarden()
        .current_dir(dir.path())
        .args(["install-hooks", "--claude-code"])
        .assert()
        .success();

    let settings: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join(".claude/settings.json")).expect("read"),
    )
    .expect("valid JSON");

    let entries = settings["hooks"]["SessionStart"]
        .as_array()
        .expect("a SessionStart entry was installed");
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0].get("matcher").is_none(),
        "a matcher is a way to miss a source: {}",
        entries[0]
    );
    assert!(
        entries[0]["hooks"][0]["command"]
            .as_str()
            .is_some_and(|command| command.ends_with("hook claude-code")),
        "the same command, dispatching on the event it is sent: {}",
        entries[0]
    );
}

/// And it comes back out with the rest.
#[test]
fn removing_takes_the_session_hook_out_too() {
    let dir = repo(&[("arch.config.json", MINIMAL)]);

    archwarden()
        .current_dir(dir.path())
        .args(["install-hooks", "--claude-code"])
        .assert()
        .success();
    archwarden()
        .current_dir(dir.path())
        .args(["install-hooks", "--claude-code", "--remove"])
        .assert()
        .success();

    let settings: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join(".claude/settings.json")).expect("read"),
    )
    .expect("valid JSON");

    assert!(
        settings.get("hooks").is_none(),
        "reporting `removed` while a hook of ours keeps running is the uninstall \
         equivalent of a gate that says it is on and is not: {settings}"
    );
}

/// Hooks are read when a session starts, so installing one mid-session does
/// nothing until the next. Said out loud, because the alternative is a user
/// testing it, seeing nothing, and concluding the installer lied.
#[test]
fn the_installer_says_when_what_it_installed_takes_effect() {
    let dir = repo(&[("arch.config.json", MINIMAL)]);

    archwarden()
        .current_dir(dir.path())
        .args(["install-hooks", "--claude-code"])
        .assert()
        .success()
        .stdout(contains("next session"));
}

/// End to end, in the shape a real session receives it: the module map arrives
/// in `additionalContext`, naming the modules and the sentence their author
/// wrote, and pointing at the commands that answer the rest.
#[test]
fn a_session_is_handed_the_module_map_and_the_commands() {
    let dir = repo(&[(
        "arch.config.json",
        r#"{"version":0,"modules":[
            {"id":"domain","scope":["packages/domain/**"],
             "why":"published, so it may not reach into the app",
             "rules":[{"type":"structure","id":"domain-shape","level":"error",
                       "roots":["packages/domain/src/*"],"allowed_subfolders":["types"]}]}]}"#,
    )]);

    let output = archwarden()
        .current_dir(dir.path())
        .args(["hook", "claude-code"])
        .write_stdin(r#"{"hook_event_name":"SessionStart","source":"startup"}"#)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let reply: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(
        reply["hookSpecificOutput"]["hookEventName"], "SessionStart",
        "{reply}"
    );

    let context = reply["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("the map reaches the model");
    assert!(context.contains("domain"), "{context}");
    assert!(
        context.contains("published, so it may not reach into the app"),
        "{context}"
    );
    assert!(context.contains("describe <path>"), "{context}");
    assert!(
        !context.contains("allowed_subfolders"),
        "a pointer, not the guide: {context}"
    );
}

/// A repository whose config governs nothing gets nothing. Announcing a gate
/// that is not there is worse than saying nothing, and it is the one case
/// where silence is the honest answer rather than the ambiguous one.
#[test]
fn a_session_in_an_ungoverned_repository_is_told_nothing() {
    let dir = repo(&[("arch.config.json", MINIMAL)]);

    archwarden()
        .current_dir(dir.path())
        .args(["hook", "claude-code"])
        .write_stdin(r#"{"hook_event_name":"SessionStart","source":"startup"}"#)
        .assert()
        .success()
        .stdout("{}\n");
}

// ---------------------------------------------------------------------------
// One rule, two ways to choose its files
// ---------------------------------------------------------------------------
//
// Issue #98, decided in 25. A rule's population was where a file sits and what
// it is called. Some obligations are about neither: "every write goes through
// the request helper" is about what the file *talks to*, and in the reported
// repository reads and writes are deliberate siblings whose names say what the
// action does and not how it travels.

/// The reported case, end to end. Two sibling files, one importing the HTTP
/// connection and one not, and a rule that must catch exactly the first.
#[test]
fn a_rule_can_choose_its_files_by_what_they_import() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"call-obligation","id":"writes-go-through-the-helper","level":"error",
                 "roots":["src/entities/*"],"file_pattern":"^[a-z-]+\\.ts$",
                 "when_importing":"src/http/connection.ts",
                 "must_call":{"symbol":"HttpRequest","imported_from":"../../http/request"}}]}"#,
        ),
        ("src/http/connection.ts", "export const conn = 1;\n"),
        ("src/http/request.ts", "export function HttpRequest() {}\n"),
        // A write: it reaches the connection, and forgets the helper.
        (
            "src/entities/consumer-unit/update.ts",
            "import { conn } from '../../http/connection';\nexport const update = () => conn;\n",
        ),
        // A read: same shape of name, same kind of place, and it must not be
        // obliged — this is the half that `roots` alone could never express.
        (
            "src/entities/system-user/find-by-email.ts",
            "export const findByEmail = () => 1;\n",
        ),
    ]);

    let output = archwarden()
        .current_dir(dir.path())
        .args(["check", "--format", "json"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).expect("JSON");

    let flagged: Vec<&str> = report["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter_map(|finding| finding["path"].as_str())
        .collect();

    assert_eq!(
        flagged,
        ["src/entities/consumer-unit/update.ts"],
        "the write is obliged and the read is not: {report}"
    );
}

/// And a rule that names no imports is untouched — including in what it costs.
/// Every rule written before 0.20 is one of these.
#[test]
fn a_rule_that_names_no_imports_behaves_exactly_as_before() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"call-obligation","id":"everything-calls-it","level":"error",
                 "roots":["src/entities/*"],"file_pattern":"^[a-z-]+\\.ts$",
                 "must_call":{"symbol":"HttpRequest","imported_from":"../../http/request"}}]}"#,
        ),
        (
            "src/entities/consumer-unit/update.ts",
            "export const update = () => 1;\n",
        ),
        (
            "src/entities/system-user/find-by-email.ts",
            "export const findByEmail = () => 1;\n",
        ),
    ]);

    let output = archwarden()
        .current_dir(dir.path())
        .args(["check", "--format", "json"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).expect("JSON");

    assert_eq!(
        report["findings"].as_array().map(Vec::len),
        Some(2),
        "both files, as before: {report}"
    );
}

/// A directory rule asks whether *something in here* talks to it, which is the
/// only reading of the axis that means anything for a rule about a directory.
#[test]
fn a_directory_rule_asks_whether_anything_inside_imports_it() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"presence","id":"talkers-write-a-contract","level":"error",
                 "roots":["src/*"],
                 "when_importing":"src/http/connection.ts",
                 "require":["contract.md"]}]}"#,
        ),
        ("src/http/connection.ts", "export const conn = 1;\n"),
        // Talks to it, and has no contract: reported.
        (
            "src/orders/update.ts",
            "import { conn } from '../http/connection';\nexport const update = () => conn;\n",
        ),
        // Talks to nothing, and has no contract either: not this rule's
        // business, which is the whole point of narrowing.
        (
            "src/reports/monthly.ts",
            "export const monthly = () => 1;\n",
        ),
    ]);

    let output = archwarden()
        .current_dir(dir.path())
        .args(["check", "--format", "json"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).expect("JSON");

    let flagged: Vec<&str> = report["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter_map(|finding| finding["path"].as_str())
        .collect();

    assert_eq!(flagged, ["src/orders"], "{report}");
}

/// The trap. An import nothing could place is not evidence that a file is out
/// of the population — it is evidence that nobody knows, and a rule that
/// quietly stopped applying because an alias is misconfigured is a gate that
/// evaporated. It is counted as a skipped check rather than read as "no".
#[test]
fn an_import_that_did_not_resolve_is_reported_rather_than_read_as_no() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"call-obligation","id":"writes-go-through-the-helper","level":"error",
                 "roots":["src/entities/*"],"file_pattern":"^[a-z-]+\\.ts$",
                 "when_importing":"src/http/connection.ts",
                 "must_call":{"symbol":"HttpRequest","imported_from":"../../http/request"}}]}"#,
        ),
        // An alias nothing can place: no tsconfig declares `@Http`.
        (
            "src/entities/consumer-unit/update.ts",
            "import { conn } from '@Http/connection';\nexport const update = () => conn;\n",
        ),
    ]);

    let output = archwarden()
        .current_dir(dir.path())
        .args(["check", "--format", "json"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).expect("JSON");

    // Reported where a boundary rule's blind spot is already reported, naming
    // the file and the specifier. This cannot tell an unplaceable alias from an
    // external package — both arrive with nothing resolved — so it says which
    // imports nobody placed and lets a reader see that the narrowing may have
    // been decided on incomplete information.
    let unresolved = &report["summary"]["imports"]["unresolved_imports"];
    assert_eq!(
        unresolved[0]["specifier"], "@Http/connection",
        "a rule that could not tell must leave the reason visible: {report}"
    );
    assert_eq!(
        unresolved[0]["path"], "src/entities/consumer-unit/update.ts",
        "{report}"
    );
}

/// The pre-write hook and `check` must agree about which files a narrowed rule
/// is even about. A write judged here against a rule `check` would not have
/// applied to it is the two surfaces disagreeing about one file — which is the
/// failure the whole 0.18 milestone was built to make impossible.
#[test]
fn the_hook_and_check_agree_about_a_rule_narrowed_by_imports() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"call-obligation","id":"writes-go-through-the-helper","level":"error",
                 "roots":["src/entities/*"],"file_pattern":"^[a-z-]+\\.ts$",
                 "when_importing":"src/http/connection.ts",
                 "must_call":{"symbol":"HttpRequest","imported_from":"../../http/request"}}]}"#,
        ),
        ("src/http/connection.ts", "export const conn = 1;\n"),
        ("src/http/request.ts", "export function HttpRequest() {}\n"),
    ]);

    // The read: it does not reach the connection, so nothing obliges it. The
    // hook must permit it.
    let read = dir.path().join("src/entities/system-user/find-by-email.ts");
    std::fs::create_dir_all(read.parent().expect("a parent")).expect("create");
    archwarden()
        .current_dir(dir.path())
        .args(["hook", "claude-code"])
        .write_stdin(format!(
            r#"{{"tool_name":"Write","tool_input":{{"file_path":"{}",
               "content":"export const findByEmail = () => 1;"}}}}"#,
            read.to_str().expect("utf-8")
        ))
        .assert()
        .success()
        .stdout("{}\n");

    // The write: it reaches the connection and forgets the helper, so the same
    // rule that ignored the file above refuses this one.
    let write = dir.path().join("src/entities/consumer-unit/update.ts");
    std::fs::create_dir_all(write.parent().expect("a parent")).expect("create");
    archwarden()
        .current_dir(dir.path())
        .args(["hook", "claude-code"])
        .write_stdin(format!(
            r#"{{"tool_name":"Write","tool_input":{{"file_path":"{}",
               "content":"import {{ conn }} from '../../http/connection';\nexport const update = () => conn;"}}}}"#,
            write.to_str().expect("utf-8")
        ))
        .assert()
        .success()
        .stdout(contains("permissionDecision"));
}

/// The blind-spots section is what the run could not decide, and it appears
/// only when there is something to say.
#[test]
fn the_blind_spots_section_appears_only_when_the_run_missed_something() {
    let clean = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[{"type":"structure","id":"shape","level":"error",
                "roots":"src/*","allowed_subfolders":["calcs"]}]}"#,
        ),
        ("src/a/calcs/x.ts", "export const x = 1;\n"),
    ]);
    let page = clean.path().join("clean.html");
    archwarden()
        .current_dir(clean.path())
        .args(["check", "--html", page.to_str().expect("utf-8")])
        .assert()
        .success();
    let html = std::fs::read_to_string(&page).expect("the page was written");
    // Each note is asserted absent on its own. A single assertion on the
    // section heading would pass while any one of them printed a zero, which
    // is the shape of every mutant this file has produced.
    assert!(
        !html.contains("nobody could make"),
        "no check was skipped: {html}"
    );
    assert!(
        !html.contains("could not be resolved"),
        "every import resolved: {html}"
    );
    assert!(
        !html.contains("accepted in the baseline"),
        "nothing was accepted into a baseline: {html}"
    );
    assert!(
        !html.contains("What this run did not decide"),
        "and with no note at all the section does not render: {html}"
    );

    // A boundary rule over a language with no front-end: the import cannot be
    // read, so the check cannot be made, and the page has to say so.
    let blind = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[{"type":"import-boundary","id":"b","level":"error",
                "from":"src/**","forbid_import_from":["vendor/**"]}]}"#,
        ),
        ("src/a.py", "import vendor.thing\n"),
        ("vendor/thing.py", "x = 1\n"),
    ]);
    let page = blind.path().join("blind.html");
    archwarden()
        .current_dir(blind.path())
        .args(["check", "--html", page.to_str().expect("utf-8")])
        .assert()
        .success();
    let html = std::fs::read_to_string(&page).expect("the page was written");
    assert!(
        html.contains("What this run did not decide"),
        "a skipped check is named: {html}"
    );
}

/// A wall exists between two declared modules, and the config for one is the
/// smallest thing that produces it: a module's scope comes from the rules
/// inside it, so a module declared with no rules contributes nothing and the
/// section never renders.
const WALLED: &str = r#"{"version":0,
    "modules":[
      {"id":"ui","rules":[
        {"type":"import-boundary","id":"ui-not-domain","level":"error",
         "from":"src/ui/**","forbid_import_from":["src/domain/**"]}]},
      {"id":"domain","rules":[
        {"type":"structure","id":"domain-shape","level":"error",
         "roots":"src/domain/*","allowed_subfolders":["parts"]}]}]}"#;

/// A wall nothing crosses says so; a wall something crosses counts it.
///
/// The two branches of `html_pressure`. A page that drew a crossed wall the
/// same as a held one would be worse than no page, because it would look like
/// it had checked.
#[test]
fn the_pressure_section_distinguishes_a_held_wall_from_a_crossed_one() {
    let held = repo(&[
        ("arch.config.json", WALLED),
        ("src/ui/a.ts", "export const a = 1;\n"),
        ("src/domain/thing/parts/x.ts", "export const x = 1;\n"),
    ]);
    let page = held.path().join("held.html");
    archwarden()
        .current_dir(held.path())
        .args(["check", "--html", page.to_str().expect("utf-8")])
        .assert()
        .success();

    let html = std::fs::read_to_string(&page).expect("the page was written");
    assert!(html.contains("The walls under pressure"), "{html}");
    assert!(html.contains(r#"pill quiet">holding"#), "{html}");
    assert!(html.contains("Nothing crosses this today."), "{html}");
    assert!(!html.contains("crossing now"), "{html}");

    let crossed = repo(&[
        ("arch.config.json", WALLED),
        (
            "src/ui/a.ts",
            "import { x } from '../domain/thing/parts/x';\nexport const a = x;\n",
        ),
        ("src/domain/thing/parts/x.ts", "export const x = 1;\n"),
    ]);
    let page = crossed.path().join("crossed.html");
    archwarden()
        .current_dir(crossed.path())
        .args(["check", "--html", page.to_str().expect("utf-8")])
        .assert()
        .code(1);

    let html = std::fs::read_to_string(&page).expect("the page was written");
    assert!(html.contains(r#"pill now">1 crossing now"#), "{html}");
    assert!(!html.contains("Nothing crosses this today."), "{html}");
}

/// Past five crossings the list folds, and the count moves onto the summary
/// line so that folding hides nothing.
#[test]
fn a_wall_crossed_more_than_five_times_folds_and_keeps_its_count() {
    let import = "import { x } from '../domain/thing/parts/x';\n";
    let mut files: Vec<(String, String)> = vec![
        ("arch.config.json".to_owned(), WALLED.to_owned()),
        (
            "src/domain/thing/parts/x.ts".to_owned(),
            "export const x = 1;\n".to_owned(),
        ),
    ];
    for n in 0..6 {
        files.push((
            format!("src/ui/f{n}.ts"),
            format!("{import}export const a{n} = x;\n"),
        ));
    }
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, body)| (path.as_str(), body.as_str()))
        .collect();

    let dir = repo(&borrowed);
    let page = dir.path().join("folded.html");
    archwarden()
        .current_dir(dir.path())
        .args(["check", "--html", page.to_str().expect("utf-8")])
        .assert()
        .code(1);

    let html = std::fs::read_to_string(&page).expect("the page was written");
    // `<details>` alone appears in the stylesheet; the summary is the body.
    assert!(
        html.contains("<details><summary>6 imports"),
        "six crossings fold, and the count survives folding: {html}"
    );
    assert!(html.contains(r#"pill now">6 crossing now"#), "{html}");
}

/// Five is not past five. The boundary is asserted from below as well, because
/// `>` and `>=` render the same page for every count except this one.
#[test]
fn a_wall_crossed_exactly_five_times_does_not_fold() {
    let import = "import { x } from '../domain/thing/parts/x';\n";
    let mut files: Vec<(String, String)> = vec![
        ("arch.config.json".to_owned(), WALLED.to_owned()),
        (
            "src/domain/thing/parts/x.ts".to_owned(),
            "export const x = 1;\n".to_owned(),
        ),
    ];
    for n in 0..5 {
        files.push((
            format!("src/ui/f{n}.ts"),
            format!("{import}export const a{n} = x;\n"),
        ));
    }
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, body)| (path.as_str(), body.as_str()))
        .collect();

    let dir = repo(&borrowed);
    let page = dir.path().join("five.html");
    archwarden()
        .current_dir(dir.path())
        .args(["check", "--html", page.to_str().expect("utf-8")])
        .assert()
        .code(1);

    let html = std::fs::read_to_string(&page).expect("the page was written");
    assert!(html.contains(r#"pill now">5 crossing now"#), "{html}");
    assert!(
        !html.contains("<details><summary>"),
        "five crossings are still a list: {html}"
    );
}

/// An import that resolves nowhere is a blind spot, and the page names it.
///
/// Separate from the skipped-check note beside it: a check nobody could make
/// and an import nobody could place are different admissions, and a page that
/// printed one for the other would be telling the reader the wrong thing.
#[test]
fn an_unresolved_import_is_named_among_the_blind_spots() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[{"type":"import-boundary","id":"b","level":"error",
                "from":"src/**","forbid_import_from":["vendor/**"]}]}"#,
        ),
        (
            "src/a.ts",
            "import { y } from './nowhere-at-all';\nexport const a = y;\n",
        ),
    ]);
    let page = dir.path().join("unresolved.html");
    archwarden()
        .current_dir(dir.path())
        .args(["check", "--html", page.to_str().expect("utf-8")])
        .assert()
        .success();

    let html = std::fs::read_to_string(&page).expect("the page was written");
    assert!(html.contains("What this run did not decide"), "{html}");
    assert!(
        html.contains("could not be resolved"),
        "the import nobody could place is named: {html}"
    );
}

/// Debt accepted into a baseline is a blind spot too, and the page counts it.
///
/// A page that showed a green run over an accepted violation without saying
/// how many were accepted would be the most flattering thing this tool could
/// print, and the least true.
#[test]
fn what_a_baseline_accepted_is_counted_among_the_blind_spots() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,"rules":[{"type":"spec-pair","id":"s","level":"error",
                "roots":"src/*","subfolders":["."]}]}"#,
        ),
        ("src/a/x.ts", "export const x = 1;\n"),
    ]);

    archwarden()
        .current_dir(dir.path())
        .arg("baseline")
        .assert()
        .success();

    let page = dir.path().join("accepted.html");
    archwarden()
        .current_dir(dir.path())
        .args(["check", "--html", page.to_str().expect("utf-8")])
        .assert()
        .success();

    let html = std::fs::read_to_string(&page).expect("the page was written");
    assert!(html.contains("What this run did not decide"), "{html}");
    assert!(
        html.contains("accepted in the baseline"),
        "the accepted debt is counted: {html}"
    );
}

/// The map counts errors and warnings apart, and says `clean` only when there
/// is neither.
///
/// Three modules and three shapes, because the branch is `errors == 0 &&
/// warnings == 0` and one fixture cannot exercise it. `spec-pair` rather than
/// `structure`: a module's counts are gathered from findings on its *files*,
/// and a structure finding is about a directory, so it lands nowhere here —
/// which is why the obvious fixture reports every module clean.
#[test]
fn the_map_counts_errors_and_warnings_apart() {
    let dir = repo(&[
        (
            "arch.config.json",
            r#"{"version":0,
                "modules":[
                  {"id":"strict","rules":[{"type":"spec-pair","id":"s","level":"error",
                    "roots":"src/strict/*","subfolders":["."]}]},
                  {"id":"lenient","rules":[{"type":"spec-pair","id":"l","level":"warning",
                    "roots":"src/lenient/*","subfolders":["."]}]},
                  {"id":"clean","rules":[{"type":"spec-pair","id":"c","level":"error",
                    "roots":"src/clean/*","subfolders":["."]}]}]}"#,
        ),
        ("src/strict/a/x.ts", "export const x = 1;\n"),
        ("src/lenient/a/x.ts", "export const x = 1;\n"),
        ("src/clean/a/x.ts", "export const x = 1;\n"),
        ("src/clean/a/x.spec.ts", "it('x', () => {});\n"),
    ]);
    let page = dir.path().join("map.html");

    archwarden()
        .current_dir(dir.path())
        .args(["check", "--html", page.to_str().expect("utf-8")])
        .assert()
        .code(1);

    let html = std::fs::read_to_string(&page).expect("the page was written");
    assert!(
        html.contains(r#"<span class="counts">1 file · <span class="hot">1 error</span></span>"#),
        "an error is marked hot, and a module with no warnings says nothing \
         about warnings: {html}"
    );
    assert!(
        html.contains(r#"<span class="counts">1 file · 1 warning</span>"#),
        "a warning is counted and is not hot: {html}"
    );
    assert!(
        html.contains(r#"<span class="counts">2 files · clean</span>"#),
        "neither is `clean`: {html}"
    );
}
