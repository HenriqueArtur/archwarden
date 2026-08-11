//! The resolution epoch.
//!
//! Import-boundary findings depend on how a specifier resolves, and that
//! depends on files no rule mentions: `tsconfig.json` for path aliases,
//! `package.json` for `exports` and workspaces, the lockfile for what is
//! installed. None of those appear in a file's content hash or in the rules
//! hash, so without a third component the `findings` cache would serve stale
//! answers after a `tsconfig.paths` change -- correct-looking, and wrong.
//!
//! Found when a `tsconfig.paths` change left the cache serving answers that
//! looked right.

use archwarden_core::{hash::ContentHash, path::RepoRelPath};

use crate::walk::RepoTree;

/// Filenames whose contents change how a specifier resolves.
///
/// Matched on the whole name, except `tsconfig` which is matched by prefix so
/// `tsconfig.base.json` and `tsconfig.build.json` are caught too.
const RESOLUTION_FILES: [&str; 5] = [
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
];

const TSCONFIG_PREFIX: &str = "tsconfig";

/// Whether a filename is one resolution depends on.
///
/// Compared case-sensitively on purpose: every tool that reads these files
/// spells them in lower case, and a `TSCONFIG.JSON` that some editor produced
/// would not be read by TypeScript either.
#[must_use]
#[allow(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "matching a filename, not a path; see above"
)]
pub fn affects_resolution(name: &str) -> bool {
    if RESOLUTION_FILES.contains(&name) {
        return true;
    }
    name.starts_with(TSCONFIG_PREFIX) && name.ends_with(".json")
}

/// Every file in the tree that resolution depends on, in path order.
///
/// Ordered because the epoch is a hash of the sequence, and a hash that
/// depended on directory-iteration order would change for no reason.
#[must_use]
pub fn resolution_files(tree: &RepoTree) -> Vec<&RepoRelPath> {
    let mut paths: Vec<&RepoRelPath> = tree
        .files()
        .filter(|file| affects_resolution(&file.name))
        .map(|file| &file.path)
        .collect();

    paths.sort();
    paths
}

/// Hashes everything resolution depends on.
///
/// A file that cannot be read contributes its path and a marker rather than
/// being skipped: a `tsconfig.json` that became unreadable *is* a change in
/// how things resolve, and skipping it would leave the epoch unchanged.
#[must_use]
pub fn resolution_epoch(root: &camino::Utf8Path, tree: &RepoTree) -> ContentHash {
    let parts: Vec<ContentHash> = resolution_files(tree)
        .into_iter()
        .flat_map(|path| {
            let contents = std::fs::read(root.join(path.as_path()))
                .unwrap_or_else(|_| b"<unreadable>".to_vec());
            [
                ContentHash::of(path.as_str().as_bytes()),
                ContentHash::of(&contents),
            ]
        })
        .collect();

    ContentHash::combine(&parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{compiled::CompiledConfig, compiled::SkipDirs, glob::PathSet};
    use camino::Utf8PathBuf;

    fn tree_at(entries: &[(&str, &str)]) -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");

        for (relative, contents) in entries {
            let path = root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create dirs");
            }
            std::fs::write(&path, contents).expect("write file");
        }

        (dir, root)
    }

    fn config() -> CompiledConfig {
        CompiledConfig::new(
            Vec::new(),
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b""),
        )
    }

    fn epoch_of(entries: &[(&str, &str)]) -> ContentHash {
        let (guard, root) = tree_at(entries);
        let tree = crate::walk::walk(&root, &config()).expect("walks");
        let epoch = resolution_epoch(&root, &tree);
        drop(guard);
        epoch
    }

    /// The files whose contents decide where a specifier lands.
    #[test]
    fn the_files_resolution_depends_on_are_recognised() {
        for name in [
            "package.json",
            "tsconfig.json",
            "tsconfig.base.json",
            "tsconfig.build.json",
            "pnpm-lock.yaml",
            "package-lock.json",
            "yarn.lock",
            "bun.lock",
        ] {
            assert!(affects_resolution(name), "{name}");
        }
    }

    /// Everything else does not. A source file's changes are already covered
    /// by its own content hash.
    #[test]
    fn ordinary_files_do_not_affect_resolution() {
        for name in [
            "user.ts",
            "README.md",
            "tsconfig.md",
            "my-package.json",
            "vite.config.ts",
            "",
        ] {
            assert!(!affects_resolution(name), "{name}");
        }
    }

    /// The whole reason C4 exists: change `tsconfig.paths` and the epoch has
    /// to move, or a warm run serves findings computed under the old aliases.
    #[test]
    fn changing_a_tsconfig_moves_the_epoch() {
        let before = epoch_of(&[
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"paths":{"@/*":["src/*"]}}}"#,
            ),
            ("src/user.ts", "export const a = 1;"),
        ]);
        let after = epoch_of(&[
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"paths":{"@/*":["lib/*"]}}}"#,
            ),
            ("src/user.ts", "export const a = 1;"),
        ]);

        assert_ne!(before, after);
    }

    #[test]
    fn changing_a_lockfile_moves_the_epoch() {
        let before = epoch_of(&[("pnpm-lock.yaml", "lockfileVersion: 9\n")]);
        let after = epoch_of(&[("pnpm-lock.yaml", "lockfileVersion: 9\npackages: {}\n")]);

        assert_ne!(before, after);
    }

    /// A source file changing must *not* move the epoch: its own content hash
    /// already covers it, and moving the epoch would invalidate every finding
    /// in the repository for one edit.
    #[test]
    fn changing_a_source_file_leaves_the_epoch_alone() {
        let before = epoch_of(&[
            ("package.json", r#"{"name":"x"}"#),
            ("src/user.ts", "export const a = 1;"),
        ]);
        let after = epoch_of(&[
            ("package.json", r#"{"name":"x"}"#),
            ("src/user.ts", "export const a = 2;"),
        ]);

        assert_eq!(before, after);
    }

    /// Adding a `tsconfig.json` where there was none changes resolution, so it
    /// changes the epoch even though no existing file was touched.
    #[test]
    fn adding_a_resolution_file_moves_the_epoch() {
        let before = epoch_of(&[("src/user.ts", "")]);
        let after = epoch_of(&[("src/user.ts", ""), ("tsconfig.json", "{}")]);

        assert_ne!(before, after);
    }

    /// A workspace has several. All of them count, because any one can change
    /// where a specifier lands.
    #[test]
    fn every_package_json_in_a_workspace_counts() {
        let before = epoch_of(&[
            ("package.json", r#"{"workspaces":["packages/*"]}"#),
            ("packages/domain/package.json", r#"{"name":"@org/domain"}"#),
        ]);
        let after = epoch_of(&[
            ("package.json", r#"{"workspaces":["packages/*"]}"#),
            (
                "packages/domain/package.json",
                r#"{"name":"@org/domain","exports":{".":"./src/index.ts"}}"#,
            ),
        ]);

        assert_ne!(before, after);
    }

    /// The epoch is a hash of a sequence, so the sequence is sorted. One that
    /// depended on directory-iteration order would change for no reason and
    /// throw the cache away at random.
    #[test]
    fn the_epoch_is_stable_across_runs() {
        let entries = [
            ("package.json", r#"{"name":"x"}"#),
            ("tsconfig.json", "{}"),
            ("packages/b/package.json", r#"{"name":"b"}"#),
            ("packages/a/package.json", r#"{"name":"a"}"#),
        ];

        assert_eq!(epoch_of(&entries), epoch_of(&entries));

        let (_guard, root) = tree_at(&entries);
        let tree = crate::walk::walk(&root, &config()).expect("walks");
        let files: Vec<_> = resolution_files(&tree)
            .into_iter()
            .map(RepoRelPath::as_str)
            .collect();
        assert_eq!(
            files,
            [
                "package.json",
                "packages/a/package.json",
                "packages/b/package.json",
                "tsconfig.json",
            ]
        );
    }

    /// A repository with none of these files still has an epoch, so the
    /// findings key does not need a special case for it.
    #[test]
    fn a_repository_with_no_resolution_files_still_has_an_epoch() {
        let epoch = epoch_of(&[("src/user.ts", "")]);
        assert_eq!(epoch, epoch_of(&[("src/user.ts", "")]));
    }

    /// A `tsconfig.json` that cannot be read *is* a change in how things
    /// resolve. Skipping it would leave the epoch where it was and serve
    /// findings computed under aliases nobody can read any more.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_resolution_file_still_moves_the_epoch() {
        use std::os::unix::fs::PermissionsExt as _;

        let readable = epoch_of(&[("tsconfig.json", "{}"), ("src/user.ts", "")]);
        let absent = epoch_of(&[("src/user.ts", "")]);

        let (guard, root) = tree_at(&[("tsconfig.json", "{}"), ("src/user.ts", "")]);
        let denied = root.join("tsconfig.json");
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        let tree = crate::walk::walk(&root, &config()).expect("walks");
        let unreadable = resolution_epoch(&root, &tree);
        // Restored so the temporary directory can be removed.
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o644))
            .expect("chmod back");
        drop(guard);

        assert_ne!(unreadable, absent, "the file is there, and that counts");
        assert_ne!(
            unreadable, readable,
            "but its contents are not what they were"
        );
    }

    /// Two files swapping contents must not produce the same epoch, which is
    /// why each contributes its path as well as its bytes.
    #[test]
    fn contents_are_bound_to_their_paths() {
        let one = epoch_of(&[
            ("packages/a/package.json", r#"{"name":"a"}"#),
            ("packages/b/package.json", r#"{"name":"b"}"#),
        ]);
        let swapped = epoch_of(&[
            ("packages/a/package.json", r#"{"name":"b"}"#),
            ("packages/b/package.json", r#"{"name":"a"}"#),
        ]);

        assert_ne!(one, swapped);
    }
}
