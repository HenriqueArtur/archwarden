//! `cargo xtask ci` — everything the workflow checks, before the workflow does.
//!
//! # Why this exists
//!
//! Three checks failed on a pull request in a row that had passed everything
//! locally: `typos` read a translation file, the coverage floor caught a
//! function nothing called, and mutation testing found a table of untested
//! strings. None of the three *could* have been caught here, because none of
//! the tools was installed — and every one of them was skipped with a cheerful
//! one-line message that read like a pass.
//!
//! So the rule this task exists to enforce is one line:
//!
//! **A check that cannot run is a failure, not a skip.**
//!
//! A skipped check and a passing check look identical in a summary, which is
//! the same fault `docs/CONFIG.md` names as the worst a linter has — a rule
//! that enforces nothing is indistinguishable from a repository that satisfies
//! it. It applies to the linter's own tooling.
//!
//! # Why the list cannot drift
//!
//! [`tests::every_step_the_workflow_runs_is_accounted_for`] reads
//! `.github/workflows/ci.yml` and fails if a command there is missing from
//! [`STEPS`], or if a command here is no longer in the workflow. Adding a job
//! to CI without adding it here breaks the build that adds it.
//!
//! A step CI runs that this task does not is written down as
//! [`Role::NotAGate`] with its reason, so the list stays complete even where it
//! is deliberately not run.

use std::{
    path::Path,
    process::{Command, Stdio},
};

/// A tool a step needs, and the one command that installs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tool {
    /// What to look for on `PATH`.
    binary: &'static str,
    /// What to run when it is not there.
    install: &'static str,
}

const NEXTEST: Tool = Tool {
    binary: "cargo-nextest",
    install: "cargo install cargo-nextest --locked",
};
const LLVM_COV: Tool = Tool {
    binary: "cargo-llvm-cov",
    install: "cargo install cargo-llvm-cov --locked",
};
const DENY: Tool = Tool {
    binary: "cargo-deny",
    install: "cargo install cargo-deny --locked",
};
const MACHETE: Tool = Tool {
    binary: "cargo-machete",
    install: "cargo install cargo-machete --locked",
};
// Not `cargo install typos-cli`, which is the obvious line and is killed by
// the OOM killer on a small machine: the dictionary is one enormous generated
// table and `rustc` holds all of it, at `opt-level=0` too. CONTRIBUTING.md has
// carried that warning for a while; the install line beside it did not, and a
// contributor reads the line they are handed.
const TYPOS: Tool = Tool {
    binary: "typos",
    install: "gh release download --repo crate-ci/typos --pattern '*<arch>-unknown-linux-musl.tar.gz'",
};
const NODE: Tool = Tool {
    binary: "node",
    install: "install Node 22 (the npm wrapper is tested with it)",
};
const PYTHON: Tool = Tool {
    binary: "python3",
    install: "install Python 3 (the release scripts are tested with it)",
};

/// Whether this task runs a workflow step, or says why it does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// Run it here, exactly as the workflow runs it.
    Gate,
    /// The workflow runs it and this task does not, for the reason given.
    NotAGate(&'static str),
}

/// One step of the workflow.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Step {
    /// The command, character for character as `ci.yml` writes it.
    ///
    /// Copied rather than paraphrased so the drift test can compare them, and
    /// so a floor changed in one place fails until it is changed in both.
    command: &'static str,
    /// Where to run it, relative to the repository root.
    dir: Option<&'static str>,
    /// What must be on `PATH` first.
    needs: Option<Tool>,
    /// Gate, or not, and why.
    role: Role,
}

/// Every step `.github/workflows/ci.yml` runs, in the order it runs them.
///
/// The order is the workflow's, not fastest-first: a contributor reading a
/// failure here should find the same job name in the same place on GitHub.
pub(crate) const STEPS: &[Step] = &[
    // 🎨 format, clippy, docs
    Step {
        command: "cargo fmt --all --check",
        dir: None,
        needs: None,
        role: Role::Gate,
    },
    Step {
        command: "cargo clippy --workspace --all-targets --all-features -- -D warnings",
        dir: None,
        needs: None,
        role: Role::Gate,
    },
    Step {
        command: "cargo doc --workspace --no-deps --all-features",
        dir: None,
        needs: None,
        role: Role::Gate,
    },
    Step {
        command: "cargo xtask check-schema",
        dir: None,
        needs: None,
        role: Role::Gate,
    },
    // 🧪 tests
    Step {
        command: "cargo nextest run --workspace --all-features",
        dir: None,
        needs: Some(NEXTEST),
        role: Role::Gate,
    },
    Step {
        command: "cargo test --workspace --doc",
        dir: None,
        needs: None,
        role: Role::Gate,
    },
    // 📊 coverage. The floors are repeated here character for character, and
    // the drift test is what keeps them equal to the workflow's.
    Step {
        command: "cargo llvm-cov -p archwarden-core --all-features --fail-under-lines 99 \
                  --fail-under-functions 100",
        dir: None,
        needs: Some(LLVM_COV),
        role: Role::Gate,
    },
    Step {
        command: "cargo llvm-cov --workspace --all-features --ignore-filename-regex 'xtask/' \
                  --fail-under-lines 95",
        dir: None,
        needs: Some(LLVM_COV),
        role: Role::Gate,
    },
    // ⚖️ licences and advisories
    Step {
        command: "cargo deny check",
        dir: None,
        needs: Some(DENY),
        role: Role::Gate,
    },
    // 🧹 unused deps and typos
    Step {
        command: "cargo machete",
        dir: None,
        needs: Some(MACHETE),
        role: Role::Gate,
    },
    Step {
        command: "typos",
        dir: None,
        needs: Some(TYPOS),
        role: Role::Gate,
    },
    // 📦 the install routes
    Step {
        command: "node --test \"test/*.test.mjs\"",
        dir: Some("npm/archwarden"),
        needs: Some(NODE),
        role: Role::Gate,
    },
    Step {
        command: "python3 -m unittest discover -s scripts -p 'test_*.py'",
        dir: None,
        needs: Some(PYTHON),
        role: Role::Gate,
    },
    // 🔮 latest stable (advisory)
    Step {
        command: "cargo test --workspace --all-features",
        dir: None,
        needs: None,
        role: Role::NotAGate(
            "the advisory job, and it is this workspace's tests on a toolchain \
             this machine does not have pinned. `cargo nextest run` above is the \
             same suite on the toolchain we ship.",
        ),
    },
    // 🔬 differential vs dependency-cruiser
    Step {
        command: "npm install --no-audit --no-fund dependency-cruiser typescript@5",
        dir: None,
        needs: None,
        role: Role::NotAGate(
            "setup for the differential job, run inside a private target \
             repository that only CI is configured with.",
        ),
    },
    Step {
        command: "cargo test -p archwarden-engine --features differential -- --nocapture",
        dir: None,
        needs: None,
        role: Role::NotAGate(
            "reads ARCHWARDEN_DIFF_REPO, and with none set it says why it did \
             nothing and passes. Run it by hand with the variable set.",
        ),
    },
];

/// What cargo tells a program about the package it is running, and what a gate
/// must not inherit.
///
/// This task is launched as `cargo xtask ci`, which is `cargo run`, so every
/// one of these is set — and describes *`xtask`*, not the gate being run. A
/// child that reads them is being told it is part of a build it has nothing to
/// do with.
///
/// It is not hypothetical. `cargo machete` read the inherited `CARGO_PKG_NAME`
/// and took its own subcommand name as a directory to analyse:
///
/// ```text
/// Analyzing dependencies of crates in machete...
/// machete: IO error for operation on machete: No such file or directory
/// ```
///
/// The gate passed when the binary was run directly and failed through the
/// alias, which is the shape of every bug in this file so far: the answer must
/// not depend on what spawned us. CI runs each of these from a clean shell, and
/// so does this.
///
/// `CARGO`, `CARGO_HOME`, `PATH` and the `RUSTUP_*` family are deliberately
/// kept — they say where the toolchain is, not what is being built.
const CARGO_INJECTED: &[&str] = &[
    "CARGO_MANIFEST_DIR",
    "CARGO_MANIFEST_PATH",
    "CARGO_CRATE_NAME",
    "CARGO_BIN_NAME",
    "CARGO_PRIMARY_PACKAGE",
    "CARGO_TARGET_TMPDIR",
    "CARGO_PKG_NAME",
    "CARGO_PKG_VERSION",
    "CARGO_PKG_VERSION_MAJOR",
    "CARGO_PKG_VERSION_MINOR",
    "CARGO_PKG_VERSION_PATCH",
    "CARGO_PKG_VERSION_PRE",
    "CARGO_PKG_AUTHORS",
    "CARGO_PKG_DESCRIPTION",
    "CARGO_PKG_HOMEPAGE",
    "CARGO_PKG_REPOSITORY",
    "CARGO_PKG_LICENSE",
    "CARGO_PKG_LICENSE_FILE",
    "CARGO_PKG_RUST_VERSION",
    "CARGO_PKG_README",
];

/// What a caller asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Run the gates.
    Run,
    /// Print what is missing and how to get it, and run nothing.
    Doctor,
}

impl Mode {
    /// Reads the flag a caller passed.
    pub(crate) fn parse(args: &[String]) -> Result<Self, String> {
        match args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            [] => Ok(Self::Run),
            ["--doctor"] => Ok(Self::Doctor),
            other => Err(format!(
                "unknown argument {other:?}; usage: cargo xtask ci [--doctor]"
            )),
        }
    }
}

/// Runs every gate, and reports every failure rather than the first.
///
/// All of them, because a contributor who fixes one thing and pushes into the
/// next failure learns the same lesson twice. The workflow reports its jobs
/// together and so does this.
pub(crate) fn run(root: &Path, mode: Mode) -> Result<(), String> {
    let gates: Vec<&Step> = STEPS
        .iter()
        .filter(|step| step.role == Role::Gate)
        .collect();

    let missing: Vec<Tool> = gates
        .iter()
        .filter_map(|step| step.needs)
        .filter(|tool| !on_path(tool.binary))
        .fold(Vec::new(), |mut seen, tool| {
            if !seen.contains(&tool) {
                seen.push(tool);
            }
            seen
        });

    if mode == Mode::Doctor {
        doctor(&gates, &missing);
        return Ok(());
    }

    // Before anything runs, and fatal. A check that cannot run is a failure:
    // the alternative is a green summary that means "we did not look".
    if !missing.is_empty() {
        eprintln!("{} of these gates cannot run here:\n", missing.len());
        for tool in &missing {
            eprintln!("  {:<16} {}", tool.binary, tool.install);
        }
        eprintln!("\nInstall them and run this again. Every one of them is a gate");
        eprintln!("that would otherwise fail on CI, minutes after you pushed.");
        return Err(format!(
            "{} tool{} missing",
            missing.len(),
            if missing.len() == 1 { "" } else { "s" }
        ));
    }

    let mut failed = Vec::new();
    for (n, step) in gates.iter().enumerate() {
        println!("[{}/{}] {}", n + 1, gates.len(), step.command);
        if !step.succeeds(root) {
            failed.push(step.command);
            println!("       FAILED");
        }
    }

    if failed.is_empty() {
        println!("\nall {} gates pass.", gates.len());
        return Ok(());
    }

    eprintln!("\n{} of {} gates failed:", failed.len(), gates.len());
    for command in &failed {
        eprintln!("  {command}");
    }
    Err("re-run the command above on its own to read its output".to_owned())
}

/// Says what is here and what is not, and runs nothing.
///
/// Separate from a run because a contributor with nothing installed should be
/// able to see the whole list at once rather than one tool per attempt.
fn doctor(gates: &[&Step], missing: &[Tool]) {
    println!("{} gates, needing:\n", gates.len());
    for tool in gates
        .iter()
        .filter_map(|step| step.needs)
        .fold(Vec::new(), |mut seen, tool| {
            if !seen.contains(&tool) {
                seen.push(tool);
            }
            seen
        })
    {
        let mark = if on_path(tool.binary) { "ok" } else { "--" };
        println!("  {mark:<4}{:<16} {}", tool.binary, tool.install);
    }

    if missing.is_empty() {
        println!("\nnothing missing.");
    } else {
        println!("\n{} missing. Their gates cannot run here.", missing.len());
    }
}

impl Step {
    /// Runs it where CI runs it, and says only whether it passed.
    ///
    /// Output is inherited rather than captured: a contributor wants the
    /// compiler's own diagnostics, in colour, as they happen.
    fn succeeds(&self, root: &Path) -> bool {
        self.spawned(root)
            .is_some_and(|mut command| command.status().is_ok_and(|status| status.success()))
    }

    /// The command as it will be run, or `None` if there is nothing to run.
    ///
    /// Separate from running it so a test can inspect the environment the
    /// child is handed, which is the part that went wrong.
    fn spawned(&self, root: &Path) -> Option<Command> {
        let mut parts = self.words();
        let program = parts.next()?;

        let mut command = Command::new(program);
        command
            .args(parts)
            .current_dir(
                self.dir
                    .map_or_else(|| root.to_owned(), |dir| root.join(dir)),
            )
            .stdin(Stdio::null());

        for variable in CARGO_INJECTED {
            command.env_remove(variable);
        }

        Some(command)
    }

    /// The command split into words, with the quotes a shell would remove.
    ///
    /// There is no shell here — one fewer thing between the command written
    /// down and the command that runs — so the two quoted arguments in the list
    /// are unwrapped by hand.
    fn words(&self) -> impl Iterator<Item = &str> {
        self.command
            .split_whitespace()
            .map(|word| word.trim_matches(['"', '\'']))
    }
}

/// Whether a binary is on `PATH`.
///
/// A file lookup rather than a `--version` call, and the difference is not
/// stylistic. Probing a cargo subcommand with `cargo machete --version`
/// reported it missing when this task was itself run under `cargo run`, and
/// found it when the same binary was run directly — so the first thing the
/// gate did was fail on the machine that had the tool installed.
///
/// A tool is present if it is where the shell would look for it. That is the
/// question `command -v` answers, it is the one the hooks beside this asked
/// before it, and it cannot be told a different story by whatever spawned us.
fn on_path(binary: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| found_in(binary, &path))
}

/// The search itself, given the `PATH` to search.
///
/// Split from [`on_path`] so a test can hand it a directory it built. The
/// workspace forbids `unsafe`, and `std::env::set_var` is unsafe from the 2024
/// edition — which is the right rule and made this the right shape.
fn found_in(binary: &str, path: &std::ffi::OsStr) -> bool {
    std::env::split_paths(path).any(|dir| {
        let candidate = dir.join(binary);
        candidate.is_file() && executable(&candidate)
    })
}

/// Whether the file has an execute bit set for anybody.
///
/// Unix only, which is what CI and every contributor's hook run on. On another
/// platform being on `PATH` is the whole answer.
#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(_path: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `run:` in the workflow, with folded blocks joined and whitespace
    /// collapsed, so a command wrapped across lines compares equal to the same
    /// command written on one.
    fn workflow_commands(yaml: &str) -> Vec<String> {
        let lines: Vec<&str> = yaml.lines().collect();
        let mut found = Vec::new();

        for (n, line) in lines.iter().enumerate() {
            let indent = line.len() - line.trim_start().len();
            let trimmed = line.trim_start();
            let Some(rest) = trimmed
                .strip_prefix("- run:")
                .or_else(|| trimmed.strip_prefix("run:"))
            else {
                continue;
            };

            let rest = rest.trim();
            if rest == ">" || rest == ">-" || rest == "|" {
                let mut block = String::new();
                for following in &lines[n + 1..] {
                    let deeper = following.len() - following.trim_start().len();
                    if following.trim().is_empty() || deeper <= indent {
                        break;
                    }
                    block.push(' ');
                    block.push_str(following.trim());
                }
                found.push(block.split_whitespace().collect::<Vec<_>>().join(" "));
            } else {
                found.push(rest.split_whitespace().collect::<Vec<_>>().join(" "));
            }
        }

        found
    }

    /// The one that stops this list from rotting.
    ///
    /// A job added to CI and not here is a check that only ever fails on
    /// GitHub, which is the whole reason this task was written. A step here
    /// that CI no longer runs is a contributor waiting on something nobody
    /// requires.
    #[test]
    fn every_step_the_workflow_runs_is_accounted_for() {
        let yaml = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join(".github/workflows/ci.yml"),
        )
        .expect("the workflow is in the repository");

        let workflow = workflow_commands(&yaml);
        let ours: Vec<String> = STEPS
            .iter()
            .map(|step| {
                step.command
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();

        for command in &workflow {
            assert!(
                ours.contains(command),
                "ci.yml runs `{command}` and xtask/src/ci.rs does not list it.\n\
                 Add it to STEPS as a gate, or as NotAGate with the reason."
            );
        }
        for command in &ours {
            assert!(
                workflow.contains(command),
                "STEPS lists `{command}` and ci.yml no longer runs it"
            );
        }
    }

    /// The parser earns its own test: it is the thing the drift test trusts,
    /// and a parser that quietly finds nothing would make that test pass
    /// forever.
    #[test]
    fn folded_and_inline_commands_read_the_same() {
        let yaml = "\
jobs:
  a:
    steps:
      - run: cargo fmt --all --check
      - run: >
          cargo llvm-cov -p archwarden-core
          --fail-under-lines 99
      - name: with a name
        run: node --test \"test/*.test.mjs\"
";
        assert_eq!(
            workflow_commands(yaml),
            vec![
                "cargo fmt --all --check",
                "cargo llvm-cov -p archwarden-core --fail-under-lines 99",
                "node --test \"test/*.test.mjs\"",
            ]
        );
    }

    /// A step that is not run is a decision, and a decision with no reason is
    /// indistinguishable from an omission. The same rule `why` exists for.
    #[test]
    fn a_step_that_is_not_a_gate_says_why() {
        for step in STEPS {
            if let Role::NotAGate(reason) = step.role {
                assert!(
                    reason.len() > 30,
                    "`{}` is skipped without saying why",
                    step.command
                );
            }
        }
    }

    /// The quoted arguments survive having no shell to unquote them.
    #[test]
    fn quotes_are_removed_the_way_a_shell_would() {
        let node = STEPS
            .iter()
            .find(|step| step.command.starts_with("node"))
            .expect("the npm wrapper is tested");

        assert_eq!(
            node.words().collect::<Vec<_>>(),
            vec!["node", "--test", "test/*.test.mjs"]
        );
    }

    #[test]
    fn the_flags_are_read_and_a_typo_is_refused() {
        assert_eq!(Mode::parse(&[]), Ok(Mode::Run));
        assert_eq!(Mode::parse(&["--doctor".to_owned()]), Ok(Mode::Doctor));
        assert!(Mode::parse(&["--docter".to_owned()]).is_err());
    }

    /// The answer must not depend on what spawned this. It did once: a cargo
    /// subcommand probed with `cargo <sub> --version` was reported missing
    /// under `cargo run` and found when run directly, so the gate's first act
    /// was to fail on a machine that had the tool.
    #[test]
    fn a_tool_is_found_where_the_shell_would_look_for_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let tool = dir.path().join("archwarden-not-a-real-tool");
        std::fs::write(&tool, "#!/bin/sh\nexit 1\n").expect("write");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }

        let path = std::env::join_paths([dir.path().to_owned()]).expect("join");

        // It exits non-zero when called. A `--version` probe would call it and
        // conclude it was not installed.
        assert!(
            found_in("archwarden-not-a-real-tool", &path),
            "a tool on PATH was not found"
        );
        assert!(
            !found_in("archwarden-definitely-not-here", &path),
            "a tool that is not on PATH was reported present"
        );
    }

    /// A gate must run the way it would from a shell, and `cargo run` hands
    /// its child a description of `xtask` that the child has no business
    /// believing. `cargo machete` believed it and analysed a directory named
    /// after its own subcommand.
    #[test]
    fn a_gate_is_not_told_it_is_part_of_this_build() {
        let step = STEPS
            .iter()
            .find(|step| step.command == "cargo machete")
            .expect("machete is a gate");

        let command = step.spawned(Path::new("/repo")).expect("a command");
        let removed: Vec<&str> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .filter_map(|(key, _)| key.to_str())
            .collect();

        assert!(
            removed.contains(&"CARGO_PKG_NAME"),
            "the variable that broke `cargo machete` is still inherited"
        );
        assert!(removed.contains(&"CARGO_MANIFEST_DIR"));
        assert!(
            !removed.contains(&"PATH"),
            "PATH is how the gate finds its own tools"
        );
    }

    /// Where CI runs it is where this runs it. The npm wrapper's tests are the
    /// only step with an answer other than the repository root, and they fail
    /// to find their own fixtures anywhere else.
    #[test]
    fn a_step_runs_in_the_directory_the_workflow_gives_it() {
        let node = STEPS
            .iter()
            .find(|step| step.command.starts_with("node"))
            .expect("the npm wrapper is tested");
        let fmt = STEPS
            .iter()
            .find(|step| step.command == "cargo fmt --all --check")
            .expect("fmt is a gate");

        assert_eq!(
            node.spawned(Path::new("/repo"))
                .expect("a command")
                .get_current_dir(),
            Some(Path::new("/repo/npm/archwarden"))
        );
        assert_eq!(
            fmt.spawned(Path::new("/repo"))
                .expect("a command")
                .get_current_dir(),
            Some(Path::new("/repo"))
        );
    }

    /// A directory of that name is not a tool of that name.
    #[test]
    fn a_directory_is_not_a_tool() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("typos")).expect("mkdir");
        let path = std::env::join_paths([dir.path().to_owned()]).expect("join");

        assert!(!found_in("typos", &path));
    }

    /// A file that is there and cannot be run is not a tool either. It is the
    /// half-finished download, and reporting it present would send a
    /// contributor to a gate that fails for a reason nothing explains.
    #[cfg(unix)]
    #[test]
    fn a_file_without_its_execute_bit_is_not_a_tool() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("typos"), "not really").expect("write");
        let path = std::env::join_paths([dir.path().to_owned()]).expect("join");

        assert!(!found_in("typos", &path));
    }
}
