//! Reading `compilerOptions.paths` backwards, for one narrow question.
//!
//! Resolution reads the alias map forwards and `oxc_resolver` owns that
//! (decision 7). This module exists for the other direction, and only for the
//! part of it that has an answer: **a file moved, and the importer's own alias
//! still covers where it landed.**
//!
//! # Why the general question has no answer
//!
//! `paths` is not invertible. Several patterns may map onto overlapping
//! targets, so a path can be spelled by more than one alias and archwarden
//! would be choosing rather than computing. That is why `impact --apply`
//! refuses an aliased specifier, and it stays the right refusal.
//!
//! # Why this one does
//!
//! The pattern that produced the specifier is not a guess: it is the entry
//! whose target reaches the file being moved. Re-running *that* pattern
//! against the destination either matches or does not, and when it matches the
//! new specifier is determined.
//!
//! An entry with no `*` falls out correctly without a special case: it names
//! one file, the destination is a different file, so it does not match and the
//! move refuses. That case matters -- `"@Env": ["./src/Env.ts"]` is a real
//! shape, and a string-level guess would happily rewrite it to `@Environment`
//! and produce a repository that does not build.
//!
//! # What it does not read
//!
//! `extends`, beyond following the chain to the first config that declares
//! `paths`. `oxc_resolver` merges `extends` properly for resolution and keeps
//! that machinery private; reimplementing the merge here to answer a
//! best-effort question would be a second implementation of something that
//! already exists, and one that could disagree with it. A config whose aliases
//! arrive some other way finds no entries here, and the move refuses exactly
//! as it did before.

use archwarden_core::path::RepoRelPath;
use camino::{Utf8Path, Utf8PathBuf};

/// Source extensions a specifier may leave off.
const EXTENSIONS: [&str; 8] = ["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

/// How far up the `extends` chain to look before giving up.
///
/// A guard rather than a feature: a cycle in `extends` is a broken config, and
/// this is not the command that reports it.
const MAX_EXTENDS_DEPTH: usize = 16;

/// The path aliases governing one file, as `(pattern, target)` pairs.
///
/// Both sides are repository-relative and may contain a single `*`, which is
/// all TypeScript allows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathAliases {
    entries: Vec<(String, String)>,
}

impl PathAliases {
    /// The aliases declared by the nearest `tsconfig.json` above `file`.
    ///
    /// Nearest wins whole, which is TypeScript's own rule and the one
    /// resolution already follows. A config that declares no `paths` hands the
    /// question to what it extends, and one that declares them stops the
    /// search -- a nearer config is a deliberate override.
    #[must_use]
    pub fn governing(root: &Utf8Path, file: &RepoRelPath) -> Self {
        let mut directory = file.parent();
        while let Some(here) = directory {
            let candidate = root.join(here.as_path()).join("tsconfig.json");
            if candidate.is_file()
                && let Some(aliases) = Self::read(root, &candidate, 0)
            {
                return aliases;
            }
            if here.is_root() {
                break;
            }
            directory = here.parent();
        }
        Self::default()
    }

    /// Reads one config, following `extends` while it declares no `paths`.
    fn read(root: &Utf8Path, path: &Utf8Path, depth: usize) -> Option<Self> {
        if depth > MAX_EXTENDS_DEPTH {
            return None;
        }
        let text = std::fs::read_to_string(path).ok()?;
        let config =
            oxc_resolver::TsConfig::parse(depth == 0, path.as_std_path(), path.as_std_path(), text)
                .ok()?;

        let directory = path.parent()?;
        if let Some(paths) = config.compiler_options.paths.as_ref()
            && !paths.is_empty()
        {
            // `paths` are anchored at `baseUrl` when there is one and at the
            // config's own directory otherwise. `oxc_resolver` computes the
            // same thing into a field it keeps private, so it is recomputed
            // here from the two public halves rather than guessed at.
            let base = config.compiler_options.base_url.as_ref().map_or_else(
                || directory.to_owned(),
                |base_url| {
                    Utf8PathBuf::from_path_buf(directory.as_std_path().join(base_url))
                        .unwrap_or_else(|_| directory.to_owned())
                },
            );

            let mut entries = Vec::new();
            for (pattern, targets) in paths {
                for target in targets {
                    let Some(target) = target.to_str() else {
                        continue;
                    };
                    if let Some(relative) = repo_relative(root, &base, target) {
                        entries.push((pattern.clone(), relative));
                    }
                }
            }
            return Some(Self { entries });
        }

        // No `paths` of its own: ask what it extends. Only the single form,
        // because the multiple form is a merge and merging is what this
        // module declines to reimplement.
        let Some(oxc_resolver::ExtendsField::Single(parent)) = config.extends.as_ref() else {
            return None;
        };
        if !parent.starts_with('.') {
            // A package name. Resolving it is `node_modules` work, and a
            // best-effort answer is not worth reaching into another package
            // for.
            return None;
        }
        let parent_path = normalise(&directory.join(parent));
        let parent_path = if parent_path.extension().is_some() {
            parent_path
        } else {
            parent_path.join("tsconfig.json")
        };
        Self::read(root, &parent_path, depth + 1)
    }

    /// The specifier that names `now`, if the alias spelling `specifier` for
    /// `was` still reaches it.
    ///
    /// `None` means refuse, which is every case this cannot answer: no entry
    /// reaches the moved file, the entry names one file rather than a subtree,
    /// or the destination has left what the alias covers.
    #[must_use]
    pub fn rewrite(&self, specifier: &str, was: &RepoRelPath, now: &RepoRelPath) -> Option<String> {
        // The specifier may or may not spell the extension. Whichever form the
        // author used, the destination is written the same way.
        for spelled_out in [false, true] {
            let was_written = written(was, spelled_out);
            let now_written = written(now, spelled_out);

            for (pattern, target) in &self.entries {
                // Only a wildcard entry can name a file it was not written
                // for. An exact one names the file it names, and the
                // destination is a different file.
                let Match::Star(star) = capture(pattern, specifier) else {
                    continue;
                };
                // Is this the entry that reaches the file being moved? An
                // entry that does not is an entry about something else.
                if capture(target, &was_written) != Match::Star(star) {
                    continue;
                }
                // Re-run that same entry against the destination.
                let Match::Star(landed) = capture(target, &now_written) else {
                    continue;
                };
                return Some(pattern.replacen('*', &landed, 1));
            }
        }
        None
    }

    /// Whether any alias was found at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A path as a specifier would spell it: with its extension, or without.
fn written(path: &RepoRelPath, spelled_out: bool) -> String {
    if spelled_out {
        return path.as_str().to_owned();
    }
    path.as_str()
        .rsplit_once('.')
        .filter(|(_, extension)| EXTENSIONS.contains(extension))
        .map_or_else(|| path.as_str().to_owned(), |(stem, _)| stem.to_owned())
}

/// What happened when a pattern was held against a string.
///
/// Three answers, and collapsing any two of them is how an entry with no `*`
/// would get treated as a wildcard -- which would rewrite `@Env` to
/// `@Environment` and produce a repository that does not build.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Match {
    /// The pattern does not match.
    No,
    /// A pattern with no `*`, matching the one string it names.
    Exact,
    /// A pattern with a `*`, and what it stood for.
    Star(String),
}

/// Holds `pattern` against `text`.
fn capture(pattern: &str, text: &str) -> Match {
    match pattern.split_once('*') {
        None => {
            if pattern == text {
                Match::Exact
            } else {
                Match::No
            }
        }
        Some((prefix, suffix)) => text
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(suffix))
            .map_or(Match::No, |star| Match::Star(star.to_owned())),
    }
}

/// A `paths` target, as a repository-relative pattern.
fn repo_relative(root: &Utf8Path, base: &Utf8Path, target: &str) -> Option<String> {
    let absolute = normalise(&base.join(target));
    let root = normalise(root);
    Some(absolute.strip_prefix(&root).ok()?.as_str().to_owned())
}

/// `.` and `..` removed, without touching the filesystem.
///
/// `canonicalize` is wrong here: a `paths` target names files that need not
/// exist, and one of them is the destination of a move that has not happened.
fn normalise(path: &Utf8Path) -> Utf8PathBuf {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.as_str().split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if path.as_str().starts_with('/') {
        Utf8PathBuf::from(format!("/{joined}"))
    } else {
        Utf8PathBuf::from(joined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn aliases(entries: &[(&str, &str)]) -> PathAliases {
        PathAliases {
            entries: entries
                .iter()
                .map(|(pattern, target)| ((*pattern).to_owned(), (*target).to_owned()))
                .collect(),
        }
    }

    /// The case that dominates a rename: the alias the importer already writes
    /// still covers the destination, so the new specifier is determined rather
    /// than chosen. Issue #36.
    #[test]
    fn an_alias_that_still_covers_the_destination_is_rewritten() {
        let aliases = aliases(&[("@Lib/*", "src/lib/*")]);

        assert_eq!(
            aliases.rewrite(
                "@Lib/thing",
                &path("src/lib/thing.ts"),
                &path("src/lib/renamed.ts")
            ),
            Some("@Lib/renamed".to_owned())
        );
    }

    /// Deeper, and with the alias covering several levels -- the shape of a
    /// real `@Infrastructure/*` move.
    #[test]
    fn the_alias_may_cover_more_than_one_level() {
        let aliases = aliases(&[("@Infrastructure/*", "src/Infrastructure/*")]);

        assert_eq!(
            aliases.rewrite(
                "@Infrastructure/Repositories/Pay/Entities/Cards/find",
                &path("src/Infrastructure/Repositories/Pay/Entities/Cards/find.ts"),
                &path("src/Infrastructure/Repositories/Pay/Entities/Card/find.ts"),
            ),
            Some("@Infrastructure/Repositories/Pay/Entities/Card/find".to_owned())
        );
    }

    /// A destination outside what the alias covers has no specifier through
    /// it, and refusing is the whole reason the general question is refused.
    #[test]
    fn a_destination_outside_the_alias_is_refused() {
        let aliases = aliases(&[("@Lib/*", "src/lib/*")]);

        assert_eq!(
            aliases.rewrite(
                "@Lib/thing",
                &path("src/lib/thing.ts"),
                &path("src/other/thing.ts")
            ),
            None
        );
    }

    /// An entry with no `*` names one file. The destination is a different
    /// file, so the entry does not reach it -- and a string-level guess would
    /// have rewritten `@Env` to `@Environment` and produced a repository that
    /// does not build.
    #[test]
    fn an_exact_entry_is_never_treated_as_a_wildcard() {
        let aliases = aliases(&[("@Env", "src/Env.ts")]);

        assert_eq!(
            aliases.rewrite("@Env", &path("src/Env.ts"), &path("src/Environment.ts")),
            None
        );
    }

    /// The entry has to be the one that reaches the moved file. Another
    /// pattern that happens to match the specifier's shape is about something
    /// else.
    #[test]
    fn an_entry_that_does_not_reach_the_moved_file_is_ignored() {
        let aliases = aliases(&[("@Lib/*", "src/other/*"), ("@Lib/*", "src/lib/*")]);

        assert_eq!(
            aliases.rewrite(
                "@Lib/thing",
                &path("src/lib/thing.ts"),
                &path("src/lib/renamed.ts")
            ),
            Some("@Lib/renamed".to_owned()),
            "the second entry is the one that reaches it"
        );
    }

    /// Whatever the author wrote comes back: a specifier that spells the
    /// extension keeps it, the same rule the relative half follows.
    #[test]
    fn a_specifier_that_spells_the_extension_keeps_it() {
        let aliases = aliases(&[("@Lib/*", "src/lib/*")]);

        assert_eq!(
            aliases.rewrite(
                "@Lib/thing.ts",
                &path("src/lib/thing.ts"),
                &path("src/lib/renamed.ts")
            ),
            Some("@Lib/renamed.ts".to_owned())
        );
    }

    /// No aliases at all is the ordinary state, and it refuses without
    /// pretending to have looked.
    #[test]
    fn no_aliases_rewrites_nothing() {
        let aliases = PathAliases::default();

        assert!(aliases.is_empty());
        assert_eq!(
            aliases.rewrite(
                "@Lib/thing",
                &path("src/lib/thing.ts"),
                &path("src/lib/renamed.ts")
            ),
            None
        );
    }

    /// The three answers are three, and telling `Exact` from `Star` is what
    /// keeps an entry naming one file from being treated as a subtree.
    #[test]
    fn a_pattern_says_which_of_the_three_it_is() {
        assert_eq!(
            capture("@Lib/*", "@Lib/thing"),
            Match::Star("thing".to_owned())
        );
        assert_eq!(capture("@Env", "@Env"), Match::Exact);
        assert_eq!(capture("@Env", "@Environment"), Match::No);
        assert_eq!(capture("@Lib/*", "@Other/thing"), Match::No);
        assert_eq!(
            capture("@Lib/*.js", "@Lib/thing.js"),
            Match::Star("thing".to_owned()),
            "a `*` in the middle keeps both sides"
        );
    }

    fn tree_at(entries: &[(&str, &str)]) -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");
        for (relative, contents) in entries {
            let file = root.join(relative);
            std::fs::create_dir_all(file.parent().expect("a file has a parent"))
                .expect("create dirs");
            std::fs::write(&file, contents).expect("write file");
        }
        (dir, root)
    }

    /// Read off a real `tsconfig.json`, which is the half everything above
    /// takes as given: the `./` in a target, and the repository-relative form
    /// the rewrite works in.
    #[test]
    fn aliases_are_read_from_the_tsconfig_on_disk() {
        let (guard, root) = tree_at(&[
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"baseUrl":".","paths":{"@Lib/*":["./src/lib/*"]}}}"#,
            ),
            ("src/app/use.ts", ""),
        ]);
        let aliases = PathAliases::governing(&root, &path("src/app/use.ts"));
        drop(guard);

        assert!(!aliases.is_empty(), "the config declares one");
        assert_eq!(
            aliases.rewrite(
                "@Lib/thing",
                &path("src/lib/thing.ts"),
                &path("src/lib/renamed.ts")
            ),
            Some("@Lib/renamed".to_owned())
        );
    }

    /// `baseUrl` moves what a target is measured from, and getting that wrong
    /// would produce entries matching nothing.
    #[test]
    fn a_base_url_anchors_the_targets() {
        let (guard, root) = tree_at(&[
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"baseUrl":"./src","paths":{"@Lib/*":["lib/*"]}}}"#,
            ),
            ("src/app/use.ts", ""),
        ]);
        let aliases = PathAliases::governing(&root, &path("src/app/use.ts"));
        drop(guard);

        assert_eq!(
            aliases.rewrite(
                "@Lib/thing",
                &path("src/lib/thing.ts"),
                &path("src/lib/renamed.ts")
            ),
            Some("@Lib/renamed".to_owned()),
            "`lib/*` under `baseUrl: ./src` is `src/lib/*`"
        );
    }

    /// The nearest config wins whole, which is TypeScript's own rule and the
    /// one resolution follows.
    #[test]
    fn the_nearest_tsconfig_wins() {
        let (guard, root) = tree_at(&[
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"paths":{"@Lib/*":["./src/lib/*"]}}}"#,
            ),
            (
                "packages/thing/tsconfig.json",
                r#"{"compilerOptions":{"paths":{"@Lib/*":["./inner/*"]}}}"#,
            ),
            ("packages/thing/use.ts", ""),
        ]);
        let aliases = PathAliases::governing(&root, &path("packages/thing/use.ts"));
        drop(guard);

        assert_eq!(
            aliases.rewrite(
                "@Lib/thing",
                &path("packages/thing/inner/thing.ts"),
                &path("packages/thing/inner/renamed.ts"),
            ),
            Some("@Lib/renamed".to_owned()),
            "the package's own map, not the repository's"
        );
    }

    /// A config with no `paths` of its own asks what it extends, which is how
    /// a monorepo package usually gets them.
    #[test]
    fn extends_is_followed_until_something_declares_paths() {
        let (guard, root) = tree_at(&[
            (
                "tsconfig.base.json",
                r#"{"compilerOptions":{"paths":{"@Lib/*":["./src/lib/*"]}}}"#,
            ),
            (
                "tsconfig.json",
                r#"{"extends":"./tsconfig.base.json","compilerOptions":{"strict":true}}"#,
            ),
            ("src/app/use.ts", ""),
        ]);
        let aliases = PathAliases::governing(&root, &path("src/app/use.ts"));
        drop(guard);

        assert_eq!(
            aliases.rewrite(
                "@Lib/thing",
                &path("src/lib/thing.ts"),
                &path("src/lib/renamed.ts")
            ),
            Some("@Lib/renamed".to_owned())
        );
    }

    /// An `extends` naming a package is not followed. Reaching into
    /// `node_modules` for a best-effort answer is not worth it, and refusing
    /// is what the move did before this existed.
    #[test]
    fn an_extends_naming_a_package_is_not_followed() {
        let (guard, root) = tree_at(&[
            (
                "tsconfig.json",
                r#"{"extends":"@tsconfig/node20/tsconfig.json"}"#,
            ),
            ("src/app/use.ts", ""),
        ]);
        let aliases = PathAliases::governing(&root, &path("src/app/use.ts"));
        drop(guard);

        assert!(aliases.is_empty());
    }

    /// No `tsconfig.json` above the file at all: the ordinary state of a plain
    /// JavaScript repository, and not an error.
    #[test]
    fn a_repository_with_no_tsconfig_has_no_aliases() {
        let (guard, root) = tree_at(&[("src/app/use.ts", "")]);
        let aliases = PathAliases::governing(&root, &path("src/app/use.ts"));
        drop(guard);

        assert!(aliases.is_empty());
        assert_eq!(
            aliases.rewrite(
                "@Lib/thing",
                &path("src/lib/thing.ts"),
                &path("src/lib/renamed.ts")
            ),
            None
        );
    }

    /// A target outside the repository is dropped rather than kept as
    /// something no repository-relative path could match.
    #[test]
    fn a_target_outside_the_repository_is_dropped() {
        let (guard, root) = tree_at(&[
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"paths":{"@Out/*":["../elsewhere/*"]}}}"#,
            ),
            ("src/app/use.ts", ""),
        ]);
        let aliases = PathAliases::governing(&root, &path("src/app/use.ts"));
        drop(guard);

        assert!(aliases.is_empty(), "nothing here is under it");
    }

    /// An `extends` cycle terminates. A config that extends itself is broken
    /// and this is not the command that reports it, but "broken config" must
    /// not mean "hangs" -- the depth bound is the only thing between the two,
    /// and without it this recursion never returns.
    #[test]
    fn an_extends_cycle_does_not_loop_forever() {
        let (guard, root) = tree_at(&[
            ("tsconfig.json", r#"{"extends":"./tsconfig.base.json"}"#),
            ("tsconfig.base.json", r#"{"extends":"./tsconfig.json"}"#),
            ("src/app/use.ts", ""),
        ]);
        let aliases = PathAliases::governing(&root, &path("src/app/use.ts"));
        drop(guard);

        assert!(aliases.is_empty());
    }

    /// A config that will not parse is not a crash and not a guess: no
    /// aliases, and the move refuses. `check` reports the broken file.
    #[test]
    fn an_unparsable_tsconfig_yields_no_aliases() {
        let (guard, root) = tree_at(&[("tsconfig.json", "{ oops"), ("src/app/use.ts", "")]);
        let aliases = PathAliases::governing(&root, &path("src/app/use.ts"));
        drop(guard);

        assert!(aliases.is_empty());
    }

    /// `.` and `..` are removed without asking the filesystem, because a
    /// `paths` target names files that need not exist -- one of them is the
    /// destination of a move that has not happened.
    #[test]
    fn dots_are_removed_without_touching_the_filesystem() {
        assert_eq!(normalise(Utf8Path::new("a/./b/../c")).as_str(), "a/c");
        assert_eq!(normalise(Utf8Path::new("/a/b/../c")).as_str(), "/a/c");
        assert_eq!(normalise(Utf8Path::new("a//b")).as_str(), "a/b");
    }
}
