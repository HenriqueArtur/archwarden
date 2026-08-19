//! What `cargo xtask` does with the word after it.
//!
//! The tasks themselves are tested in their own modules, against throwaway
//! directories. This is about the dispatch above them, and it runs the real
//! binary because that is the only way to observe what `main` returns.
//!
//! Mutation testing asked for it: `main` replaced by `Default::default()` —
//! a program that exits successfully and does nothing — survived the entire
//! suite. Every task was covered and the thing that chooses between them was
//! not, so `cargo xtask check-schema` reporting success while checking nothing
//! would have looked exactly like a passing build.
//!
//! Only read-only tasks are run here. `clean` deletes and `preview` writes, and
//! a test suite that invokes either against the repository it is running in is
//! a worse bug than the one it is guarding.

use std::process::Command;

/// The binary this crate builds, given to us by cargo.
fn xtask() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
}

/// A word nobody implemented has to fail. Silently doing nothing is how a
/// contributor runs a task they misspelled, sees no complaint, and believes it
/// ran.
#[test]
fn an_unknown_task_is_refused() {
    // Not a near-miss of a real word: the spell checker reads test sources
    // too, and offers a correction for a typo written on purpose.
    let status = xtask().arg("cheque-schema").status().expect("runs");

    assert!(
        !status.success(),
        "an unknown task reported success; every misspelling would look like work"
    );
}

/// Likewise no word at all: the usage text is the answer, and it is not a
/// successful run.
#[test]
fn no_task_at_all_is_refused() {
    let status = xtask().status().expect("runs");

    assert!(!status.success(), "bare `cargo xtask` reported success");
}

/// And a real task actually reaches its task.
///
/// `check-schema` is the one to use: it reads the committed schema, compares,
/// and writes nothing. It is also load-bearing — CI runs it — so a dispatch
/// that stopped routing it would break the schema gate silently.
#[test]
fn a_known_task_runs_and_reports_what_it_found() {
    let status = xtask().arg("check-schema").status().expect("runs");

    assert!(
        status.success(),
        "`check-schema` failed; either the dispatch is broken or \
         schema/v0.json is out of date (`cargo xtask gen-schema`)"
    );
}

/// `linker` reaches the toolchain and prints flags that name it.
///
/// Read-only, which is why it belongs here: without `--write` it touches no
/// file. It is also the only way to observe that the task runs `rustc` at all
/// -- a `rustc` that returned nothing would leave no host triple to name, and
/// a `run` that returned `Ok(())` without doing anything would print nothing.
/// Mutation testing asked for both.
#[test]
fn the_linker_task_prints_flags_for_this_machine() {
    let output = xtask().arg("linker").output().expect("runs");

    assert!(output.status.success(), "`linker` failed");

    let printed = String::from_utf8(output.stdout).expect("UTF-8");
    assert!(printed.contains("[target."), "it names a target: {printed}");
    assert!(
        printed.contains("fuse-ld=lld"),
        "and asks for the bundled linker: {printed}"
    );
    assert!(
        printed.contains("gcc-ld"),
        "by the path the toolchain keeps it at: {printed}"
    );
}

/// A flag a task does not understand is refused rather than ignored.
///
/// `clean` is safe to ask this of: the argument is rejected before anything is
/// removed, which is the property being asserted.
#[test]
fn a_task_refuses_a_flag_it_does_not_know() {
    let status = xtask()
        .args(["clean", "--everything"])
        .status()
        .expect("runs");

    assert!(
        !status.success(),
        "an unknown flag was accepted by a task that deletes"
    );
}
