//! `archwarden config coverage` — which files no rule governs.
//!
//! `CONFIG.md` names the worst failure a linter has:
//!
//! > a rule enforcing nothing is indistinguishable from a repository that
//! > satisfies it
//!
//! This is that sentence one level up, and nothing answered it until now:
//!
//! > **a file no rule governs is indistinguishable from a file that satisfies
//! > every rule**
//!
//! `check` prints `0 errors`, and that reads as *the architecture holds* when
//! it may mean *half the tree was never looked at*. Issue #59.
//!
//! # What the other commands answer, and why none of them is this
//!
//! Every existing question is asked **per rule**: `config doctor` says a rule
//! is broken or reaches nothing, `config verify-rules` says a rule does not
//! bite, `config explain` says what a rule covers. None of them can be asked
//! *"what is nobody watching?"*, because that is a question about **files**,
//! and a file nothing mentions appears in no rule's answer.
//!
//! # Governed means a rule would evaluate the file
//!
//! Decided by [`RuleEngine::applies_to`](archwarden_core::traits::RuleEngine::applies_to),
//! which is the same code `check` uses to pick the rules for a file — so this
//! report cannot disagree with the checker about what is covered, which a
//! second implementation eventually would.
//!
//! Issue #60 leaves it open whether "governed" should instead mean *inside a
//! directory some scope selects*, and the two differ for exactly one rule
//! kind. `presence` governs a *directory* — these files must exist here — and
//! its `applies_to` returns `false` for every path, deliberately and with a
//! comment saying so. That is the right answer here: a `presence` rule does
//! not object to a file you add, so a file dropped into a directory only a
//! `presence` rule governs really is unwatched. Counting it as covered would
//! be this report telling a comfortable lie in the one place it exists to
//! refuse one.
//!
//! `structure` is what makes the distinction subtle rather than academic, and
//! it comes out right on its own: it answers for directories *and* claims the
//! files inside one it governs, because `filename_patterns` constrains them.
//!
//! # Grouped by directory, because per file it is unactionable
//!
//! Nobody writes a rule per file. A thousand paths is a wall of text and one
//! directory is one rule to write, so the report collapses each **shallowest
//! wholly-ungoverned directory** into a single line and leaves partly-governed
//! directories reporting themselves, where somebody can open one and see both
//! kinds.

use std::collections::BTreeMap;

use archwarden_core::{compiled::CompiledConfig, path::RepoRelPath};
use archwarden_engine::walk::RepoTree;

/// What no rule is watching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    /// Every file the walk reached.
    pub files: usize,
    /// Where the ungoverned ones are, worst first.
    pub groups: Vec<Group>,
}

impl Coverage {
    /// How many files no rule would evaluate.
    #[must_use]
    pub fn ungoverned(&self) -> usize {
        self.groups.iter().map(|group| group.files).sum()
    }

    /// How many at least one rule would evaluate.
    #[must_use]
    pub fn governed(&self) -> usize {
        self.files.saturating_sub(self.ungoverned())
    }

    /// Whether everything is watched by something.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.groups.is_empty()
    }
}

/// One directory's worth of ungoverned files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The directory, repository-relative.
    pub directory: RepoRelPath,
    /// How many ungoverned files it accounts for.
    pub files: usize,
    /// Whether *everything* below it is ungoverned, so one rule here covers
    /// the lot.
    ///
    /// The difference between "write a rule here" and "write a rule here and
    /// then check what it caught", which is the difference between a line
    /// somebody can act on and one that misleads them.
    pub whole_subtree: bool,
}

/// Counts what no rule governs, grouped by directory.
#[must_use]
pub fn examine(config: &CompiledConfig, tree: &RepoTree) -> Coverage {
    let engines = archwarden_rules::engines_for(config);
    let files: Vec<(RepoRelPath, bool)> = tree
        .files()
        .map(|file| {
            let governed = engines.iter().any(|engine| engine.applies_to(&file.path));
            (file.path.clone(), governed)
        })
        .collect();

    Coverage {
        files: files.len(),
        groups: group(&files),
    }
}

/// Turns `(path, governed)` into the lines a reader acts on.
///
/// Split from [`examine`] so the grouping can be tested against paths written
/// down in a test rather than against a repository somebody has to build on
/// disk first — the shapes that matter here are "all of it", "some of it" and
/// "nested", and each is three lines to state and a directory tree to create.
pub(crate) fn group(files: &[(RepoRelPath, bool)]) -> Vec<Group> {
    let totals = counted(files.iter().map(|(path, _)| path));
    let missing = counted(
        files
            .iter()
            .filter(|(_, governed)| !governed)
            .map(|(path, _)| path),
    );

    let whole = |directory: &RepoRelPath| {
        missing.get(directory).copied().unwrap_or_default()
            == totals.get(directory).copied().unwrap_or_default()
    };

    // The shallowest wholly-ungoverned directories. A parent that is also
    // whole already accounts for its children, so only the topmost is a line.
    let mut groups: Vec<Group> = missing
        .iter()
        .filter(|(directory, _)| whole(directory))
        .filter(|(directory, _)| !directory.parent().is_some_and(|parent| whole(&parent)))
        .map(|(directory, files)| Group {
            directory: directory.clone(),
            files: *files,
            whole_subtree: true,
        })
        .collect();

    // Everything a whole subtree did not account for, reported where the files
    // actually are. Rolling these up would say "write a rule here" about a
    // directory that already has one.
    let mut leftovers: BTreeMap<RepoRelPath, usize> = BTreeMap::new();
    for (path, _) in files.iter().filter(|(_, governed)| !governed) {
        if groups.iter().any(|group| under(path, &group.directory)) {
            continue;
        }
        if let Some(directory) = path.parent() {
            *leftovers.entry(directory).or_default() += 1;
        }
    }
    groups.extend(leftovers.into_iter().map(|(directory, files)| Group {
        directory,
        files,
        whole_subtree: false,
    }));

    // Worst first, because the first line is the one rule worth writing today.
    groups.sort_by(|a, b| {
        b.files
            .cmp(&a.files)
            .then_with(|| a.directory.cmp(&b.directory))
    });
    groups
}

/// How many of `files` sit at or below each directory.
fn counted<'a>(files: impl Iterator<Item = &'a RepoRelPath>) -> BTreeMap<RepoRelPath, usize> {
    let mut counts: BTreeMap<RepoRelPath, usize> = BTreeMap::new();

    for file in files {
        let mut directory = file.parent();
        while let Some(here) = directory {
            *counts.entry(here.clone()).or_default() += 1;
            if here.as_str().is_empty() {
                break;
            }
            directory = here.parent();
        }
    }

    counts
}

/// Whether `file` is at or below `directory`.
///
/// Compared by path segment rather than by string prefix, which would have
/// `apps/admin-legacy/x.ts` counted as living under `apps/admin`.
fn under(file: &RepoRelPath, directory: &RepoRelPath) -> bool {
    if directory.as_str().is_empty() {
        return true;
    }
    file.as_str()
        .strip_prefix(directory.as_str())
        .is_some_and(|rest| rest.starts_with('/'))
}

/// The version of the `config coverage` JSON shape.
pub const COVERAGE_VERSION: u32 = 0;

#[derive(serde::Serialize)]
struct JsonCoverage<'a> {
    version: u32,
    files: usize,
    governed: usize,
    ungoverned: usize,
    groups: Vec<JsonGroup<'a>>,
}

#[derive(serde::Serialize)]
struct JsonGroup<'a> {
    directory: &'a str,
    files: usize,
    whole_subtree: bool,
}

/// Renders the report.
pub fn render(coverage: &Coverage, format: crate::report::Format, out: &mut dyn std::io::Write) {
    match format {
        crate::report::Format::Text => render_text(coverage, out),
        crate::report::Format::Json => render_json(coverage, out),
    }
}

fn render_json(coverage: &Coverage, out: &mut dyn std::io::Write) {
    let json = serde_json::to_string_pretty(&JsonCoverage {
        version: COVERAGE_VERSION,
        files: coverage.files,
        governed: coverage.governed(),
        ungoverned: coverage.ungoverned(),
        groups: coverage
            .groups
            .iter()
            .map(|group| JsonGroup {
                directory: group.directory.as_str(),
                files: group.files,
                whole_subtree: group.whole_subtree,
            })
            .collect(),
    });

    match json {
        Ok(json) => {
            let _ = writeln!(out, "{json}");
        }
        Err(error) => {
            let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
        }
    }
}

fn render_text(coverage: &Coverage, out: &mut dyn std::io::Write) {
    if coverage.is_complete() {
        let _ = writeln!(
            out,
            "every one of the {} files is governed by some rule.",
            coverage.files
        );
        return;
    }

    let _ = writeln!(
        out,
        "{} of {} files are governed by no rule
",
        coverage.ungoverned(),
        coverage.files
    );

    // The glob a reader would paste into `roots`, rather than the bare
    // directory: a whole subtree is `dir/**` and a mixed one is `dir/*`,
    // because in the second case the files below it are somebody else's.
    //
    // Rendered before the width is taken rather than the width being guessed
    // from the directory plus a constant. The constant was `+ 3`, which is
    // right for `/**` and one too many for `/*`, and there is no arithmetic
    // here to be right or wrong about now.
    let globs: Vec<String> = coverage
        .groups
        .iter()
        .map(|group| {
            format!(
                "{}/{}",
                group.directory,
                if group.whole_subtree { "**" } else { "*" }
            )
        })
        .collect();
    let width = globs.iter().map(String::len).max().unwrap_or_default();

    for (group, glob) in coverage.groups.iter().zip(&globs) {
        let _ = writeln!(
            out,
            "  {glob:<width$}  {} {}",
            group.files,
            if group.files == 1 { "file" } else { "files" }
        );
    }

    let _ = writeln!(
        out,
        "
A `**` line is one rule away from covered. A `*` line already has a \
         rule beside it,
so look at what the two would each catch."
    );
}

#[cfg(test)]
mod tests {
    use super::{Group, group, under};
    use archwarden_core::path::RepoRelPath;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// `(path, governed)`, as `examine` produces it.
    fn files(entries: &[(&str, bool)]) -> Vec<(RepoRelPath, bool)> {
        entries
            .iter()
            .map(|(p, governed)| (path(p), *governed))
            .collect()
    }

    fn rendered(entries: &[(&str, bool)]) -> String {
        let files = files(entries);
        let coverage = super::Coverage {
            files: files.len(),
            groups: group(&files),
        };
        let mut out = Vec::new();
        super::render(&coverage, crate::report::Format::Text, &mut out);
        String::from_utf8(out).expect("output is UTF-8")
    }

    fn lines(groups: &[Group]) -> Vec<(String, usize, bool)> {
        groups
            .iter()
            .map(|g| (g.directory.as_str().to_owned(), g.files, g.whole_subtree))
            .collect()
    }

    /// The two numbers a reader compares, and they have to add up.
    ///
    /// `governed` is the one people quote, and it is derived: a `governed`
    /// that did not come from `files - ungoverned` would let the report say
    /// "1 843 of 2 800" over totals that never summed.
    #[test]
    fn the_counts_agree_with_the_groups() {
        let entries = files(&[
            ("legacy/a.ts", false),
            ("legacy/b.ts", false),
            ("src/watched.ts", true),
        ]);
        let coverage = super::Coverage {
            files: entries.len(),
            groups: group(&entries),
        };

        assert_eq!(coverage.files, 3);
        assert_eq!(coverage.ungoverned(), 2);
        assert_eq!(coverage.governed(), 1);
        assert_eq!(
            coverage.governed() + coverage.ungoverned(),
            coverage.files,
            "the halves have to be the whole, or the headline is arithmetic \
             nobody can check"
        );
        assert!(!coverage.is_complete());

        let clean = super::Coverage {
            files: 2,
            groups: Vec::new(),
        };
        assert_eq!(clean.ungoverned(), 0);
        assert_eq!(clean.governed(), 2);
        assert!(clean.is_complete());
    }

    /// The JSON carries the same numbers as the prose, and the glob shape as a
    /// flag rather than as punctuation somebody has to parse back out.
    #[test]
    fn the_json_says_what_the_text_says() {
        let entries = files(&[
            ("legacy/a.ts", false),
            ("legacy/b.ts", false),
            ("src/watched.ts", true),
            ("src/stray.ts", false),
        ]);
        let coverage = super::Coverage {
            files: entries.len(),
            groups: group(&entries),
        };
        let mut out = Vec::new();
        super::render(&coverage, crate::report::Format::Json, &mut out);
        let json: serde_json::Value = serde_json::from_slice(&out)
            .expect("it is JSON, and a report nothing can parse is not one");

        assert_eq!(json["version"], super::COVERAGE_VERSION);
        assert_eq!(json["files"], 4);
        assert_eq!(json["governed"], 1);
        assert_eq!(json["ungoverned"], 3);

        let groups = json["groups"].as_array().expect("an array");
        assert_eq!(groups.len(), 2, "{groups:?}");
        assert_eq!(groups[0]["directory"], "legacy");
        assert_eq!(groups[0]["files"], 2);
        assert_eq!(groups[0]["whole_subtree"], true);
        assert_eq!(groups[1]["directory"], "src");
        assert_eq!(groups[1]["whole_subtree"], false);
    }

    /// The column the counts sit in is wide enough for the longest glob.
    ///
    /// Not cosmetic: the width is computed from the directory plus the `/**`
    /// the line actually prints, and getting it short runs the name into the
    /// number.
    #[test]
    fn the_counts_line_up_past_the_longest_directory() {
        let out = rendered(&[
            ("a/x.ts", false),
            ("packages/something-long/y.ts", false),
            // So the collapse stops at `something-long` rather than climbing
            // to `packages`, which is what makes this a *long* name.
            ("packages/orders/kept.ts", true),
            ("src/kept.ts", true),
        ]);

        let columns: Vec<usize> = out
            .lines()
            // The group lines, which are the indented ones. The headline also
            // says "files" and is not in the column.
            .filter(|line| line.starts_with("  ") && line.contains("/**"))
            .filter_map(|line| line.find(" file"))
            .collect();
        assert_eq!(columns.len(), 2, "{out}");
        assert_eq!(
            columns[0], columns[1],
            "the counts are in one column, whatever the directory is called:\n{out}"
        );
        assert!(
            out.contains("packages/something-long/**"),
            "and the long name is not truncated: {out}"
        );
    }

    /// The report reads as English, including for the directory holding one
    /// file. "1 files" is the kind of thing a reader stops on.
    #[test]
    fn the_lines_read_as_sentences() {
        let one = rendered(&[("legacy/only.ts", false), ("src/a.ts", true)]);
        assert!(
            one.contains("1 of 2 files are governed by no rule"),
            "{one}"
        );
        assert!(one.contains("1 file\n"), "singular, not `1 files`: {one}");

        let clean = rendered(&[("src/a.ts", true), ("src/b.ts", true)]);
        assert_eq!(
            clean.trim(),
            "every one of the 2 files is governed by some rule.",
            "and a covered repository is told so plainly rather than shown an \
             empty list"
        );
    }

    /// A repository every rule reaches has nothing to report.
    #[test]
    fn a_fully_governed_repository_reports_nothing() {
        assert!(group(&files(&[("src/a.ts", true), ("src/b.ts", true)])).is_empty());
    }

    /// A wholly ungoverned subtree is one line, however deep it goes.
    ///
    /// The whole point of grouping: four hundred paths is a wall of text
    /// nobody reads, and one directory is one rule to write.
    #[test]
    fn a_wholly_ungoverned_subtree_collapses_to_its_shallowest_directory() {
        let groups = group(&files(&[
            ("packages/legacy/a.ts", false),
            ("packages/legacy/deep/b.ts", false),
            ("packages/legacy/deep/deeper/c.ts", false),
            // A governed sibling, so the collapse has somewhere to stop.
            ("packages/orders/cart.ts", true),
            ("src/app.ts", true),
        ]));

        assert_eq!(lines(&groups), [("packages/legacy".to_owned(), 3, true)]);
    }

    /// And it climbs as far as it truly can.
    ///
    /// With nothing governed under `packages`, the honest line is `packages`
    /// rather than each of its children: the reader is being told where one
    /// rule would go, and naming the children would have them write three.
    #[test]
    fn the_collapse_climbs_past_a_directory_with_no_governed_files_at_all() {
        let groups = group(&files(&[
            ("packages/legacy/a.ts", false),
            ("packages/other/b.ts", false),
            ("src/app.ts", true),
        ]));

        assert_eq!(lines(&groups), [("packages".to_owned(), 2, true)]);
    }

    /// A directory holding both kinds reports itself, and is not rolled up
    /// into its parent.
    ///
    /// Rolling it up would say "write a rule here" about a directory that
    /// already has one, which is worse than saying nothing: the reader writes
    /// a second rule over the first and the number does not move.
    #[test]
    fn a_partly_governed_directory_reports_itself() {
        let groups = group(&files(&[
            ("src/watched.ts", true),
            ("src/unwatched.ts", false),
            ("src/also-unwatched.ts", false),
        ]));

        assert_eq!(lines(&groups), [("src".to_owned(), 2, false)]);
    }

    /// The two shapes side by side, worst first.
    ///
    /// Ordering is by count, because the first line is the one rule worth
    /// writing today.
    #[test]
    fn the_biggest_gap_is_reported_first() {
        let groups = group(&files(&[
            ("packages/legacy/a.ts", false),
            ("packages/legacy/b.ts", false),
            ("packages/legacy/c.ts", false),
            ("packages/orders/cart.ts", true),
            ("apps/admin/kept.ts", true),
            ("apps/admin/stray.ts", false),
        ]));

        assert_eq!(
            lines(&groups),
            [
                ("packages/legacy".to_owned(), 3, true),
                ("apps/admin".to_owned(), 1, false),
            ]
        );
    }

    /// A whole subtree inside a partly governed one is still its own line, and
    /// is not counted twice.
    #[test]
    fn a_whole_subtree_under_a_mixed_parent_is_named_once() {
        let groups = group(&files(&[
            ("apps/admin/kept.ts", true),
            ("apps/admin/stray.ts", false),
            ("apps/admin/screens/a.ts", false),
            ("apps/admin/screens/b.ts", false),
        ]));

        assert_eq!(
            lines(&groups),
            [
                ("apps/admin/screens".to_owned(), 2, true),
                ("apps/admin".to_owned(), 1, false),
            ],
            "the screens are one rule; the stray file is a different decision"
        );
        assert_eq!(
            groups.iter().map(|g| g.files).sum::<usize>(),
            3,
            "and three ungoverned files are reported as three, not five"
        );
    }

    /// Nothing governed at all collapses to the root, which is the honest
    /// answer for a repository with no rules: there is one thing to fix and it
    /// is the config.
    #[test]
    fn a_repository_with_no_rules_at_all_collapses_to_one_line() {
        let groups = group(&files(&[
            ("src/a.ts", false),
            ("apps/b.ts", false),
            ("README.md", false),
        ]));

        assert_eq!(groups.len(), 1, "{groups:?}");
        assert_eq!(groups[0].files, 3);
        assert!(groups[0].whole_subtree);
    }

    /// Containment is by path segment, not by string prefix.
    ///
    /// `apps/admin-legacy` starts with `apps/admin` and is a different
    /// directory. Getting this wrong would silently fold one team's files into
    /// another team's line.
    #[test]
    fn a_directory_that_merely_shares_a_prefix_is_not_inside_it() {
        assert!(under(&path("apps/admin/x.ts"), &path("apps/admin")));
        assert!(under(&path("apps/admin/deep/x.ts"), &path("apps/admin")));
        assert!(!under(&path("apps/admin-legacy/x.ts"), &path("apps/admin")));
        assert!(!under(&path("apps/admin.ts"), &path("apps/admin")));
    }
}
