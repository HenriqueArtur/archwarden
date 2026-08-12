//! `cargo xtask mutants` — mutation testing on what this branch changes.
//!
//! `docs/TESTING.md` explains why `cargo-mutants` runs on push rather than
//! nightly: a report nobody opens is a survivor list nobody reads. That
//! argument is still right, and it has a gap this module exists to close.
//!
//! # The gap
//!
//! The hook runs at push. A branch nobody pushes never runs it, and nothing
//! between a commit and a push says a word. Milestone 0.16 was written over
//! five commits without one push, and the survivors accumulated in silence:
//! twenty-eight of them, over four issues, discovered in the middle of a
//! release. The whole compile-layer lowering of `only_import_from` had no
//! test; both of `doctor`'s module checks could be deleted with the suite
//! green.
//!
//! None of that was hard to fix. It was hard to *see*, and it was hard to see
//! for eight minutes of accumulated diff instead of the thirteen seconds any
//! one of those commits would have cost.
//!
//! # What this does about it
//!
//! Two entry points over one implementation, so the number you are told and
//! the number that blocks you cannot disagree:
//!
//! - [`pending`] lists the mutants in the current diff **without running
//!   them**. Measured at **1.0 s for 224 mutants**, which is why `cargo xtask
//!   ci` can end with a line naming the count and still cost what it did.
//! - [`run`] runs them, and is what the pre-push hook calls.
//!
//! The gate stays at push, where `TESTING.md` put it. What is new is that the
//! number is visible before you get there, growing, in the output of a command
//! already run many times a day.
//!
//! # Why not a step of `cargo xtask ci`
//!
//! Measured on this repository: `cargo xtask ci` is 73.7 s. Running the
//! mutants of one ordinary commit adds about 65% to that, and of an
//! accumulated branch about 570%. A gate people run less often because it got
//! slower is a gate that catches less, which is the failure this module is
//! about, arriving from the other side.
//!
//! There is a second reason, found the hard way while releasing 0.16.0. `git`
//! opens its connection to the remote *before* running `pre-push`, so a hook
//! that takes fifteen minutes has the server close the connection under it —
//! the push fails after every check has passed. Heavy verification in the
//! common path does not just cost time; past a point it breaks the thing it
//! is attached to.

use std::path::Path;
use std::process::Command;

/// The branch a diff is taken against when nothing else says otherwise.
const DEFAULT_BRANCH: &str = "main";

/// What `cargo-mutants` had to say, once the noise is separated from the
/// verdict.
///
/// The distinction is the whole of `TESTING.md`'s "only survivors block":
/// exit 2 is a statement about your tests, and a linker killed on a small
/// machine is not. A hook that treated them alike is one somebody bypasses
/// once and never comes back from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Every mutant was caught.
    Clean {
        /// How many ran.
        tested: usize,
    },
    /// Mutants lived. This blocks.
    Survivors {
        /// One line each, as `cargo-mutants` names them.
        missed: Vec<String>,
        /// Whether the run also ended abnormally, so the list may be short.
        interrupted: bool,
    },
    /// It could not form an opinion, and why. Advisory.
    Inconclusive {
        /// What to tell whoever is waiting.
        why: String,
    },
}

/// Decides what a run means from its exit code and its survivor list.
///
/// **The survivor list wins over the exit code, always.** The hook used to
/// read the code alone and let 190 survivors past: the run was interrupted
/// *after* printing them, and an interruption is not exit 2. A run that found
/// something found it, however it ended — the honest caveat is that there may
/// be more, and [`Verdict::Survivors::interrupted`] carries it.
pub(crate) fn verdict_of(code: Option<i32>, missed: Vec<String>, tested: usize) -> Verdict {
    if !missed.is_empty() {
        return Verdict::Survivors {
            interrupted: code != Some(2),
            missed,
        };
    }

    match code {
        Some(0) => Verdict::Clean { tested },
        // Exit 2 with nothing in the list is a contradiction, and the honest
        // reading is that the list did not survive the run rather than that
        // there was nothing in it.
        Some(2) => Verdict::Inconclusive {
            why: "it reported survivors and left no list of them".to_owned(),
        },
        Some(code) => Verdict::Inconclusive {
            why: format!(
                "it exited {code} without forming an opinion -- a build failure, \
                 a timeout, or the linker being killed"
            ),
        },
        None => Verdict::Inconclusive {
            why: "it was killed by a signal".to_owned(),
        },
    }
}

/// The line `cargo xtask ci` ends with.
///
/// Written for whoever reads it next, which on this project is as often an
/// agent as a person: it names the count and the exact command, so acting on
/// it needs nothing recalled and nothing looked up. A number with no command
/// beside it is a number people learn to scroll past.
pub(crate) fn advice(pending: &Result<usize, String>) -> String {
    match pending {
        Ok(0) => "mutants: nothing to test on this diff.".to_owned(),
        Ok(1) => "mutants: 1 to test on this diff, not run here — `cargo xtask mutants`".to_owned(),
        Ok(count) => {
            format!("mutants: {count} to test on this diff, not run here — `cargo xtask mutants`")
        }
        Err(why) => format!("mutants: not counted ({why})"),
    }
}

/// The base named by `--since <ref>`, if the arguments name one.
///
/// A flag rather than a positional, because the pre-push hook may have nothing
/// to pass: a branch the remote has never seen has no "since", and the honest
/// answer there is the merge base. `--since` with nothing after it is that
/// same case rather than an error — the hook builds the argument from a shell
/// variable, and an empty variable must not turn a push into a usage message.
pub(crate) fn since_from(arguments: &[String]) -> Option<String> {
    let at = arguments
        .iter()
        .position(|argument| argument == "--since")?;
    arguments
        .get(at + 1)
        .filter(|reference| !reference.trim().is_empty())
        .cloned()
}

/// How many mutants the current diff would produce, without running any.
///
/// `--list` parses the changed lines and prints what it *would* mutate, which
/// costs a second where running them costs minutes. That gap is what lets the
/// count be free enough to print every time.
pub(crate) fn pending(root: &Path) -> Result<usize, String> {
    let base = base(root, None)?;
    let diff = diff_against(root, &base)?;
    if diff.trim().is_empty() {
        return Ok(0);
    }

    let path = write_diff(root, &diff)?;
    let listed = Command::new("cargo")
        .args(["mutants", "--list", "--in-diff"])
        .arg(&path)
        .args(["--output", &scratch(root).to_string_lossy()])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cargo-mutants could not run: {error}"))?;

    if !listed.status.success() {
        return Err(String::from_utf8_lossy(&listed.stderr)
            .lines()
            .last()
            .unwrap_or("cargo-mutants could not list")
            .trim()
            .to_owned());
    }

    Ok(count_listed(&String::from_utf8_lossy(&listed.stdout)))
}

/// One mutant per non-empty line, which is the format `--list` prints.
pub(crate) fn count_listed(stdout: &str) -> usize {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

/// The commit a diff is taken against.
///
/// Three answers, in order:
///
/// 1. **`--since <ref>`**, which the pre-push hook passes as the sha the remote
///    already has. That makes a second push to an existing branch test only
///    what it adds, and it is not an optimisation for its own sake — a hook
///    that takes eight minutes has the remote close the SSH connection under
///    it, so the push fails *after* every check passed. Measured while
///    releasing 0.16.0. Slow enough breaks the thing it is attached to.
/// 2. **`GITHUB_BASE_REF`**, when a pull request set it.
/// 3. **The merge base with the default branch** — everything this branch
///    added, which is what the count in `cargo xtask ci` reports and what
///    somebody running this by hand means.
///
/// Never `HEAD`: accumulating across commits is the failure being prevented,
/// and a base of `HEAD` cannot see it.
fn base(root: &Path, since: Option<&str>) -> Result<String, String> {
    if let Some(reference) = since {
        return Ok(reference.to_owned());
    }
    if let Some(reference) = pull_request_base(std::env::var("GITHUB_BASE_REF").ok().as_deref()) {
        return Ok(reference);
    }

    let merge_base = Command::new("git")
        .args(["merge-base", "HEAD", DEFAULT_BRANCH])
        .current_dir(root)
        .output()
        .map_err(|error| format!("git could not run: {error}"))?;

    if !merge_base.status.success() {
        return Err(format!("no merge base with `{DEFAULT_BRANCH}`"));
    }

    Ok(String::from_utf8_lossy(&merge_base.stdout)
        .trim()
        .to_owned())
}

/// The base a pull request names, if it names one.
///
/// Split out so it can be tested without writing to the process environment,
/// which this project's lints refuse. The empty case is the one that matters:
/// `GITHUB_BASE_REF` is set and empty for a workflow that is not a pull
/// request, and taking that literally would diff against `origin/`, which
/// resolves to nothing and reports no mutants at all.
fn pull_request_base(reference: Option<&str>) -> Option<String> {
    let named = reference?.trim();
    (!named.is_empty()).then(|| format!("origin/{named}"))
}

/// Everything this branch changed in Rust, working tree included.
///
/// Committed *and* uncommitted, because a survivor in code you have not
/// committed yet is one you would rather hear about before you commit it —
/// and because `cargo xtask ci` runs against the working tree, so a count
/// taken from `HEAD` would describe a different repository than the gates
/// beside it just checked.
fn diff_against(root: &Path, base: &str) -> Result<String, String> {
    let diff = Command::new("git")
        .args(["diff", base, "--", "*.rs"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("git could not run: {error}"))?;

    if !diff.status.success() {
        return Err("git could not produce the diff".to_owned());
    }

    let mut patch = String::from_utf8_lossy(&diff.stdout).into_owned();
    for file in untracked_sources(root)? {
        patch.push_str(&added_whole(root, &file)?);
    }
    Ok(patch)
}

/// Rust files git has never been told about.
///
/// `git diff` cannot see them — they are in no tree and no index — so a module
/// written and not yet added produces *no mutants at all*. That is the wrong
/// way round: a brand new file is exactly where untested code comes from, and
/// it would be the one thing the count never mentioned.
///
/// Found rather than fixed with `git add -N`, which would do the job and would
/// also write to the index of somebody who only asked for a number.
fn untracked_sources(root: &Path) -> Result<Vec<String>, String> {
    let listed = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard", "--", "*.rs"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("git could not run: {error}"))?;

    if !listed.status.success() {
        return Err("git could not list untracked files".to_owned());
    }

    Ok(String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect())
}

/// One untracked file, as a patch that adds all of it.
///
/// `--no-index` makes git diff two paths it does not track, and `/dev/null` on
/// the left is how a patch spells "this file is new". It exits 1 when the two
/// differ, which here is every time and is not an error.
fn added_whole(root: &Path, file: &str) -> Result<String, String> {
    let diff = Command::new("git")
        .args(["diff", "--no-index", "--", "/dev/null", file])
        .current_dir(root)
        .output()
        .map_err(|error| format!("git could not run: {error}"))?;

    Ok(String::from_utf8_lossy(&diff.stdout).into_owned())
}

fn scratch(root: &Path) -> std::path::PathBuf {
    root.join("target").join("xtask-mutants")
}

fn write_diff(root: &Path, diff: &str) -> Result<std::path::PathBuf, String> {
    let directory = scratch(root);
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join("in-diff.patch");
    std::fs::write(&path, diff).map_err(|error| error.to_string())?;
    Ok(path)
}

/// Runs the mutants of the current diff, and reports what they mean.
///
/// # Errors
/// When mutants survived. Every other outcome is advisory and returns `Ok`,
/// which is `TESTING.md`'s "only survivors block" and the reason this is a
/// hook people keep.
pub(crate) fn run(root: &Path, since: Option<&str>) -> Result<(), String> {
    if !found_in(&std::env::var("PATH").unwrap_or_default(), "cargo-mutants") {
        println!("mutants: skipped -- cargo install cargo-mutants");
        return Ok(());
    }

    let base = base(root, since)?;
    let diff = diff_against(root, &base)?;
    if diff.trim().is_empty() {
        println!("mutants: nothing to test on this diff.");
        return Ok(());
    }

    let path = write_diff(root, &diff)?;
    let output = scratch(root);
    println!("mutants: testing what this branch changed...");

    let status = Command::new("cargo")
        .args(["mutants", "--in-diff"])
        .arg(&path)
        .args(["--output", &output.to_string_lossy()])
        .current_dir(root)
        .status()
        .map_err(|error| format!("cargo-mutants could not run: {error}"))?;

    let missed = missed_from(&output);
    let tested = missed.len();

    report(&verdict_of(status.code(), missed, tested))
}

/// Where `cargo-mutants` puts its report inside the directory it is given.
///
/// `--output <dir>` names the *parent*: the run's files land in
/// `<dir>/mutants.out`. Getting that wrong reads as an empty survivor list,
/// which is why [`verdict_of`] refuses to call exit 2 with no list a clean
/// run — it caught exactly this while the module was being written.
const REPORT_DIR: &str = "mutants.out";

/// The survivor list `cargo-mutants` leaves behind, read from disk rather than
/// from the exit code. See [`verdict_of`].
fn missed_from(output: &Path) -> Vec<String> {
    std::fs::read_to_string(output.join(REPORT_DIR).join("missed.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

fn report(verdict: &Verdict) -> Result<(), String> {
    match verdict {
        Verdict::Clean { .. } => {
            println!("mutants: every one caught.");
            Ok(())
        }
        Verdict::Inconclusive { why } => {
            println!("mutants: no opinion -- {why}");
            Ok(())
        }
        Verdict::Survivors {
            missed,
            interrupted,
        } => {
            eprintln!("\n{} mutant(s) survived:\n", missed.len());
            for line in missed {
                eprintln!("    {line}");
            }
            eprintln!(
                "\n  Each one is a behaviour with no test. Write the test, or say\n  \
                 why the mutant is harmless."
            );
            if *interrupted {
                eprintln!(
                    "\n  The run also ended abnormally, so there may be more than\n  \
                     these."
                );
            }
            Err(format!("{} mutant(s) survived", missed.len()))
        }
    }
}

/// Whether `binary` sits in one of the directories `paths` names.
///
/// Takes the `PATH` rather than reading it, so it can be tested without
/// writing to the process environment — which this project's lints refuse and
/// which would be a mutation shared between tests anyway. It decides whether a
/// missing `cargo-mutants` is a skip or a failed push: "always yes" blocks
/// every machine without the tool, and "always no" silently stops checking on
/// every machine with it.
fn found_in(paths: &str, binary: &str) -> bool {
    std::env::split_paths(paths).any(|directory| directory.join(binary).is_file())
}

#[cfg(test)]
mod tests {
    use super::{Verdict, advice, count_listed, since_from, verdict_of};

    /// A clean run is a clean run.
    #[test]
    fn everything_caught_is_clean() {
        assert_eq!(
            verdict_of(Some(0), Vec::new(), 12),
            Verdict::Clean { tested: 12 }
        );
    }

    /// The one that shipped 190 survivors: the run was interrupted *after*
    /// printing them, and an interruption is not exit 2. The list decides,
    /// and the caveat that there may be more rides with the verdict.
    #[test]
    fn a_survivor_list_blocks_whatever_the_exit_code_was() {
        let missed = vec!["src/a.rs:1:1: replace f with ()".to_owned()];

        assert_eq!(
            verdict_of(Some(2), missed.clone(), 1),
            Verdict::Survivors {
                missed: missed.clone(),
                interrupted: false,
            },
            "the ordinary way to find survivors"
        );
        assert_eq!(
            verdict_of(None, missed.clone(), 1),
            Verdict::Survivors {
                missed: missed.clone(),
                interrupted: true,
            },
            "killed by a signal, and it still found one"
        );
        assert_eq!(
            verdict_of(Some(101), missed.clone(), 1),
            Verdict::Survivors {
                missed,
                interrupted: true,
            },
            "and a build failure after the list was written"
        );
    }

    /// A tool that could not form an opinion is advisory, not a block.
    ///
    /// `TESTING.md`'s argument, and it is about people rather than about
    /// correctness: somebody whose laptop runs out of memory linking a test
    /// binary must not be unable to push. They would reach for `--no-verify`
    /// once and never come back.
    #[test]
    fn a_run_that_could_not_decide_does_not_block() {
        for code in [Some(1), Some(101), None] {
            assert!(
                matches!(
                    verdict_of(code, Vec::new(), 0),
                    Verdict::Inconclusive { .. }
                ),
                "{code:?} said nothing about the tests"
            );
        }
    }

    /// Exit 2 with an empty list is a contradiction, and the honest reading is
    /// that the list was lost rather than that it was empty. Reporting clean
    /// there is how survivors get past.
    #[test]
    fn survivors_reported_without_a_list_are_not_clean() {
        let verdict = verdict_of(Some(2), Vec::new(), 0);

        assert!(
            matches!(&verdict, Verdict::Inconclusive { why } if why.contains("left no list")),
            "{verdict:?}"
        );
        assert!(
            !matches!(verdict, Verdict::Clean { .. }),
            "a clean verdict here is the failure this arm exists for"
        );
    }

    /// The line names the count *and* the command.
    ///
    /// Whoever reads it next is as often an agent as a person, and a number
    /// with no command beside it is one they scroll past. This is the whole of
    /// the guarantee: the count is visible while it is still small.
    #[test]
    fn the_advice_names_the_count_and_the_command() {
        let many = advice(&Ok(224));
        assert!(many.contains("224"), "{many}");
        assert!(many.contains("cargo xtask mutants"), "{many}");
        assert!(
            many.contains("not run here"),
            "it must not read as though they were tested: {many}"
        );

        assert!(
            advice(&Ok(1)).contains(" 1 to test"),
            "singular reads right"
        );

        let none = advice(&Ok(0));
        assert_eq!(none, "mutants: nothing to test on this diff.");
        assert!(
            !none.contains("cargo xtask mutants"),
            "nothing to do, so nothing to suggest: {none}"
        );
    }

    /// And when it cannot be counted, it says so rather than saying zero.
    ///
    /// Zero and "I could not tell" are the same output to a reader and
    /// opposite facts, which is the confusion every other counter in this
    /// project is written to avoid.
    #[test]
    fn a_count_that_could_not_be_taken_does_not_read_as_zero() {
        let line = advice(&Err("no merge base with `main`".to_owned()));

        assert!(line.contains("not counted"), "{line}");
        assert!(line.contains("no merge base"), "{line}");
        assert_ne!(line, advice(&Ok(0)));
    }

    /// `--since <ref>` names the base, and everything else means "the merge
    /// base".
    ///
    /// The empty case is the one that matters: the pre-push hook builds this
    /// argument from a shell variable that is empty for a branch the remote
    /// has never seen. An empty `--since` must fall back rather than become a
    /// usage error, or the first push of every new branch fails on its
    /// arguments.
    #[test]
    fn the_base_is_read_from_the_arguments_or_left_to_the_merge_base() {
        let owned =
            |args: &[&str]| -> Vec<String> { args.iter().map(|a| (*a).to_owned()).collect() };

        assert_eq!(
            since_from(&owned(&["--since", "abc123"])),
            Some("abc123".to_owned())
        );
        assert_eq!(
            since_from(&owned(&["--other", "x", "--since", "abc123"])),
            Some("abc123".to_owned()),
            "it is a flag, not a position"
        );
        assert_eq!(since_from(&owned(&[])), None);
        assert_eq!(
            since_from(&owned(&["--since"])),
            None,
            "a flag with nothing after it is the merge base, not an error"
        );
        assert_eq!(
            since_from(&owned(&["--since", "  "])),
            None,
            "and neither is an empty shell variable"
        );
    }

    /// A repository with one commit, for the git-shaped half of this module.
    ///
    /// Real `git`, because everything below is about what `git` actually
    /// prints: a fake would be a second implementation of the thing being
    /// checked, and it is the thing being checked that keeps surprising us.
    fn repository() -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().expect("temp dir");
        let root = directory.path().to_path_buf();
        let git = |args: &[&str]| {
            let done = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .expect("git runs");
            assert!(done.status.success(), "git {args:?}");
        };

        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "T"]);
        std::fs::write(root.join("kept.rs"), "pub fn kept() -> u8 { 1 }\n").expect("write");
        git(&["add", "-A"]);
        git(&["commit", "-qm", "first"]);

        (directory, root)
    }

    /// The base is the merge base when nothing names one, and `--since` when
    /// something does.
    #[test]
    fn the_base_is_the_merge_base_unless_a_reference_is_given() {
        let (guard, root) = repository();
        let head = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&root)
                .output()
                .expect("git runs")
                .stdout,
        )
        .expect("utf-8");

        assert_eq!(
            super::base(&root, None).expect("there is a merge base"),
            head.trim(),
            "one commit, so the merge base with `main` is that commit"
        );
        assert_eq!(
            super::base(&root, Some("abc123")).expect("given"),
            "abc123",
            "and an explicit reference is taken as given"
        );
        drop(guard);
    }

    /// A file git has never been told about still produces mutants.
    ///
    /// `git diff` cannot see an untracked file, so a module written and not
    /// yet added would produce *no mutants at all* — and a brand new file is
    /// exactly where untested code comes from. This is the one hole that would
    /// have made the count reassuring and wrong.
    #[test]
    fn a_file_git_has_never_seen_is_still_in_the_diff() {
        let (guard, root) = repository();
        std::fs::write(root.join("brand-new.rs"), "pub fn fresh() -> u8 { 2 }\n").expect("write");

        assert_eq!(
            super::untracked_sources(&root).expect("git lists them"),
            vec!["brand-new.rs".to_owned()]
        );

        let base = super::base(&root, None).expect("merge base");
        let diff = super::diff_against(&root, &base).expect("diff");
        assert!(diff.contains("brand-new.rs"), "{diff}");
        assert!(
            diff.contains("fn fresh"),
            "the whole file is added, not just its name: {diff}"
        );
        drop(guard);
    }

    /// And an edit to a tracked file is in it too, uncommitted.
    ///
    /// `cargo xtask ci` checks the working tree, so a count taken from `HEAD`
    /// would describe a different repository than the gates beside it just
    /// looked at.
    #[test]
    fn an_uncommitted_edit_is_in_the_diff() {
        let (guard, root) = repository();
        std::fs::write(root.join("kept.rs"), "pub fn kept() -> u8 { 99 }\n").expect("write");

        let base = super::base(&root, None).expect("merge base");
        let diff = super::diff_against(&root, &base).expect("diff");

        assert!(diff.contains("kept.rs"), "{diff}");
        assert!(diff.contains("99"), "{diff}");
        drop(guard);
    }

    /// A repository nobody has touched since the base produces no diff, which
    /// is what lets `run` say "nothing to test" instead of building the world.
    #[test]
    fn an_untouched_repository_diffs_to_nothing() {
        let (guard, root) = repository();
        let base = super::base(&root, None).expect("merge base");

        assert!(
            super::diff_against(&root, &base)
                .expect("diff")
                .trim()
                .is_empty()
        );
        drop(guard);
    }

    /// The survivor list is read from where `cargo-mutants` actually writes
    /// it: `<output>/mutants.out/missed.txt`, not `<output>/missed.txt`.
    ///
    /// Getting that wrong reads as "no survivors", which is the one wrong
    /// answer this module must never give. It happened while writing it, and
    /// only `verdict_of` refusing to call exit 2 with an empty list a clean
    /// run turned it up.
    #[test]
    fn the_survivor_list_is_read_from_where_it_is_written() {
        let directory = tempfile::tempdir().expect("temp dir");
        let output = directory.path();
        let report = output.join(super::REPORT_DIR);
        std::fs::create_dir_all(&report).expect("dirs");
        std::fs::write(
            report.join("missed.txt"),
            "a.rs:1:1: delete !\n\nb.rs:2:2: replace x\n",
        )
        .expect("write");

        assert_eq!(
            super::missed_from(output),
            vec![
                "a.rs:1:1: delete !".to_owned(),
                "b.rs:2:2: replace x".to_owned()
            ],
            "blank lines are not survivors"
        );

        let empty = tempfile::tempdir().expect("temp dir");
        assert!(
            super::missed_from(empty.path()).is_empty(),
            "no report at all is no survivors, not a panic"
        );
    }

    /// Survivors are the only verdict that fails, and the failure names how
    /// many.
    #[test]
    fn only_survivors_make_the_report_fail() {
        assert!(super::report(&Verdict::Clean { tested: 3 }).is_ok());
        assert!(
            super::report(&Verdict::Inconclusive {
                why: "the linker was killed".to_owned()
            })
            .is_ok(),
            "it could not form an opinion, so it does not get one"
        );

        let failed = super::report(&Verdict::Survivors {
            missed: vec!["a.rs:1:1: delete !".to_owned()],
            interrupted: false,
        })
        .expect_err("survivors block");
        assert!(failed.contains('1'), "{failed}");
    }

    /// Where the scratch diff and the report live, under `target/` so a clean
    /// takes them and no `.gitignore` has to learn about them.
    #[test]
    fn the_working_files_live_under_target() {
        let (guard, root) = repository();
        let written = super::write_diff(&root, "diff --git a/x b/x\n").expect("writes");

        assert!(written.starts_with(root.join("target")), "{written:?}");
        assert_eq!(
            std::fs::read_to_string(&written).expect("readable"),
            "diff --git a/x b/x\n"
        );
        drop(guard);
    }

    /// A workflow that is not a pull request sets `GITHUB_BASE_REF` to the
    /// empty string, and taking that literally diffs against `origin/`.
    ///
    /// Which resolves to nothing, reports no mutants, and reads exactly like a
    /// clean branch. The fallback to the merge base is the whole point.
    #[test]
    fn an_empty_pull_request_base_falls_through() {
        assert_eq!(
            super::pull_request_base(Some("main")),
            Some("origin/main".to_owned())
        );
        assert_eq!(
            super::pull_request_base(Some("  main  ")),
            Some("origin/main".to_owned()),
            "whatever whitespace the runner put around it"
        );
        assert_eq!(super::pull_request_base(Some("")), None);
        assert_eq!(super::pull_request_base(Some("   ")), None);
        assert_eq!(super::pull_request_base(None), None);
    }

    /// A binary is found where the shell would look for it, and not
    /// elsewhere.
    ///
    /// This decides whether a missing `cargo-mutants` is a skip or a failed
    /// push, so "always yes" would block every machine without the tool and
    /// "always no" would silently stop checking on every machine with it.
    #[test]
    fn a_binary_is_found_where_the_shell_would_look_for_it() {
        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::write(directory.path().join("pretend-tool"), "#!/bin/sh\n").expect("write");
        let elsewhere = tempfile::tempdir().expect("temp dir");

        let paths = |dirs: &[&std::path::Path]| {
            std::env::join_paths(dirs.iter().map(|d| d.to_path_buf()))
                .expect("joinable")
                .into_string()
                .expect("utf-8")
        };

        assert!(super::found_in(
            &paths(&[elsewhere.path(), directory.path()]),
            "pretend-tool"
        ));
        assert!(!super::found_in(
            &paths(&[elsewhere.path()]),
            "pretend-tool"
        ));
        assert!(
            !super::found_in("", "pretend-tool"),
            "an empty PATH finds nothing"
        );
    }

    /// One mutant per line, blanks ignored.
    #[test]
    fn the_listing_is_counted_by_line() {
        assert_eq!(count_listed(""), 0);
        assert_eq!(count_listed("\n  \n"), 0);
        assert_eq!(
            count_listed("src/a.rs:1:1: replace f with ()\nsrc/b.rs:2:2: delete !\n"),
            2
        );
    }
}
