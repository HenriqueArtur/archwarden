//! Which files a branch or a working tree has touched.
//!
//! `--changed` is a *filter*, exactly like `--paths`: every rule still runs
//! over the whole repository and the exit code still comes from the whole
//! report. This is decision 12 — the scope of `check` is the repository — and
//! it is not negotiable at the level of a flag.
//!
//! The distinction matters because the obvious reading is the dangerous one. A
//! `--changed` that narrowed what is *evaluated* would let a pull request
//! touching only `apps/web` pass while a regression sits in
//! `packages/domain`, and the gate would say nothing. As a filter, that cannot
//! happen: the build still fails, the report just shows the part the reader
//! asked about, and the `hidden` count says how much it left out.
//!
//! What it does not do, therefore, is "fail only on new violations". That is a
//! baseline — a committed record of accepted debt — and it is an honest
//! feature worth building on its own terms. It is not this one, and pretending
//! otherwise would be the quiet way to lose decision 12.

use camino::Utf8Path;

/// The default comparison: the working tree against the current commit.
///
/// `--changed` alone means "what I have not committed yet", which is the
/// question a developer asks before committing. `--changed main` means "what
/// this branch does", because a two-dot diff against a ref covers both the
/// commits on top of it and the work still uncommitted.
pub const DEFAULT_REF: &str = "HEAD";

/// Every file that differs from `reference`, repository-relative.
///
/// # Errors
/// A message naming what went wrong: git missing, not a repository, or a ref
/// that does not exist. Refused rather than treated as "nothing changed",
/// which would silently show an empty report.
pub fn changed_files(root: &Utf8Path, reference: &str) -> Result<Vec<String>, String> {
    // `--relative` makes git report paths from `root` rather than from the
    // repository root. Those differ whenever `arch.config.json` does not sit
    // beside `.git`, and findings are relative to the config.
    let diff = git(
        root,
        &["diff", "--name-only", "--relative", reference],
        "compare against",
    )?;

    // A file that does not exist in git yet is exactly the file most worth
    // checking, so untracked ones are included. `--exclude-standard` honours
    // `.gitignore`, without which this would return every build artefact.
    let untracked = git(
        root,
        &["ls-files", "--others", "--exclude-standard"],
        "list untracked files in",
    )?;

    Ok(collect(&diff, &untracked))
}

/// Merges git's two answers into one sorted, deduplicated list, with the
/// directories those files live in.
///
/// # Why the directories
///
/// git names *files*. A `structure` finding names the *directory* that should
/// not exist — so adding `packages/app/src/billing/services/z.ts` produces a
/// finding on `packages/app/src/billing/services`, which is not a path git
/// mentioned. Filtering on the file alone hid the violation the change had
/// just caused, which is the one thing `--changed` exists to show. Found by
/// running it, not by reading it.
///
/// Every ancestor, because one new file can create a whole chain of
/// directories and a rule may be about any link in it. The cost is that a
/// pre-existing finding on a directory you happened to add a file to also
/// shows — which is honest: it is on the path you touched. Erring the other
/// way would hide your own regression, and that is not a trade worth making.
///
/// Split out from the process call so it is testable without a repository:
/// what git prints is stable, and what this does with it is where a mistake
/// would hide.
#[must_use]
fn collect(diff: &str, untracked: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();

    for line in diff.lines().chain(untracked.lines()) {
        let path = line.trim();
        if path.is_empty() {
            continue;
        }
        paths.push(path.to_owned());

        // The repository root is never added: an empty path would match
        // everything and turn the filter off.
        let mut ancestor = camino::Utf8Path::new(path).parent();
        while let Some(directory) = ancestor {
            if directory.as_str().is_empty() {
                break;
            }
            paths.push(directory.to_string());
            ancestor = directory.parent();
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

fn git(root: &Utf8Path, args: &[&str], doing: &str) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root.as_std_path())
        .args(args)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;

    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        let message = message.trim();
        return Err(format!("cannot {doing} `{root}`: {message}"));
    }

    String::from_utf8(output.stdout).map_err(|_| "git printed a path that is not UTF-8".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ordinary case: two commands, one list.
    #[test]
    fn both_lists_are_merged() {
        let paths = collect("src/a.ts\nsrc/b.ts\n", "src/c.ts\n");

        assert_eq!(paths, ["src", "src/a.ts", "src/b.ts", "src/c.ts"]);
    }

    /// A file that is both modified and reported untracked -- git can say both
    /// during a rename -- is one file, not two. A duplicate would count twice
    /// in `hidden`.
    #[test]
    fn a_path_in_both_lists_appears_once() {
        assert_eq!(collect("src/a.ts\n", "src/a.ts\n"), ["src", "src/a.ts"]);
    }

    /// Sorted, because the report is deterministic and a filter built from an
    /// unordered list would make its own diagnostics wander.
    #[test]
    fn the_list_is_sorted() {
        assert_eq!(collect("z.ts\na.ts\n", "m.ts\n"), ["a.ts", "m.ts", "z.ts"]);
    }

    /// Nothing changed is an empty list, not an error. It is a real state --
    /// a clean working tree -- and the caller shows an empty report for it.
    #[test]
    fn nothing_changed_is_an_empty_list() {
        assert!(collect("", "").is_empty());
        assert!(collect("\n", "  \n").is_empty());
    }

    /// The defect this was written to fix, found by running it.
    ///
    /// git names *files*. A structure finding names the *directory* that
    /// should not exist. Adding `packages/app/src/billing/services/z.ts` makes
    /// `packages/app/src/billing/services` a new and possibly forbidden
    /// directory -- and matching on the file alone hid the violation the
    /// change had just caused, which is the one thing `--changed` exists to
    /// show.
    #[test]
    fn a_new_file_carries_the_directories_it_created() {
        let paths = collect("packages/app/src/billing/services/z.ts\n", "");

        assert!(paths.contains(&"packages/app/src/billing/services/z.ts".to_owned()));
        assert!(
            paths.contains(&"packages/app/src/billing/services".to_owned()),
            "the directory the file created: {paths:?}"
        );
        assert!(paths.contains(&"packages/app".to_owned()), "{paths:?}");
    }

    /// Every ancestor, because creating one file can create a whole chain of
    /// directories, and a rule may be about any link in it.
    #[test]
    fn every_ancestor_is_included_but_not_the_root() {
        let paths = collect("a/b/c.ts\n", "");

        assert_eq!(paths, ["a", "a/b", "a/b/c.ts"]);
    }

    /// A file at the top level has no ancestor to add, and must not
    /// contribute an empty path -- which would match everything.
    #[test]
    fn a_top_level_file_adds_nothing() {
        assert_eq!(collect("package.json\n", ""), ["package.json"]);
    }

    /// Against a real repository, which is the only way to know the flags are
    /// the ones git actually has.
    #[test]
    fn a_real_repository_answers() {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8PathBuf::from_path_buf(guard.path().to_path_buf())
            .expect("temp path is UTF-8");

        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(root.as_std_path())
                .args(args)
                .output()
                .expect("git runs")
        };

        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::create_dir_all(root.join("src")).expect("create dirs");
        std::fs::write(root.join("src/committed.ts"), "export const a = 1;\n").expect("write");
        run(&["add", "-A"]);
        run(&["commit", "-qm", "first"]);

        // Nothing has moved since that commit.
        assert!(
            changed_files(&root, "HEAD").expect("answers").is_empty(),
            "a clean tree has changed nothing"
        );

        // One edited, one brand new, one ignored.
        std::fs::write(root.join("src/committed.ts"), "export const a = 2;\n").expect("write");
        std::fs::write(root.join("src/fresh.ts"), "export const b = 1;\n").expect("write");
        std::fs::write(root.join(".gitignore"), "ignored.ts\n").expect("write");
        std::fs::write(root.join("ignored.ts"), "export const c = 1;\n").expect("write");

        let changed = changed_files(&root, "HEAD").expect("answers");

        assert!(
            changed.contains(&"src/committed.ts".to_owned()),
            "{changed:?}"
        );
        assert!(
            changed.contains(&"src/fresh.ts".to_owned()),
            "an untracked file is the one most worth checking: {changed:?}"
        );
        assert!(
            !changed.contains(&"ignored.ts".to_owned()),
            "gitignored stays out: {changed:?}"
        );
    }

    /// A ref that does not exist says so, rather than reporting that nothing
    /// changed -- which would show an empty report and look like good news.
    #[test]
    fn a_ref_that_does_not_exist_is_refused() {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8PathBuf::from_path_buf(guard.path().to_path_buf())
            .expect("temp path is UTF-8");
        std::process::Command::new("git")
            .arg("-C")
            .arg(root.as_std_path())
            .args(["init", "-q"])
            .output()
            .expect("git runs");

        let message = changed_files(&root, "no-such-branch").expect_err("no such ref");

        assert!(message.contains("cannot compare against"), "{message}");
    }

    /// And a directory that is not a repository at all.
    #[test]
    fn a_directory_that_is_not_a_repository_is_refused() {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8PathBuf::from_path_buf(guard.path().to_path_buf())
            .expect("temp path is UTF-8");

        let message = changed_files(&root, "HEAD").expect_err("not a repository");

        assert!(message.contains("cannot compare against"), "{message}");
    }
}
