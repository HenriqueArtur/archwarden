//! Walking the repository into a tree the rules can question.
//!
//! Rules never touch the filesystem. They are handed a [`RepoTree`] and ask it
//! what sits inside a directory, which is what keeps them deterministic and
//! testable without a disk. It is also what lets `describe` answer for a file
//! that does not exist: the same rule code runs against a path with no tree
//! entry behind it.

use std::collections::BTreeMap;

use archwarden_core::{compiled::CompiledConfig, path::RepoRelPath};
use camino::Utf8Path;

/// Why the walk could not complete.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WalkError {
    /// The root does not exist, or could not be read.
    #[error("cannot walk `{root}`")]
    Unwalkable {
        /// The root that was given.
        root: String,
        /// What the walker said.
        #[source]
        source: Box<ignore::Error>,
    },
}

/// What kind of file this is, as far as archwarden cares.
///
/// "Is this a spec?" is deliberately not here: a spec is whatever a rule's
/// `spec_suffix` says it is, and two rules in one config may disagree. Asking
/// the tree would force one answer on both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum FileClass {
    /// A JavaScript or TypeScript source file.
    Source,
    /// Anything else. Structure rules still see these, since a `filename_patterns`
    /// rule may well be about `DOC.md`.
    Other,
}

impl FileClass {
    /// Classifies by extension.
    #[must_use]
    pub fn of(name: &str) -> Self {
        let source = matches!(
            name.rsplit_once('.').map(|(_, extension)| extension),
            Some("ts" | "tsx" | "js" | "jsx" | "mts" | "cts" | "mjs" | "cjs")
        );

        if source { Self::Source } else { Self::Other }
    }
}

/// One file in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// The file's own name, including extension.
    pub name: String,
    /// Where it is, relative to the repository root.
    pub path: RepoRelPath,
    /// What kind of file it is.
    pub class: FileClass,
}

/// One directory's direct contents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Directory {
    /// Names of the directories immediately inside, sorted.
    pub subdirectories: Vec<String>,
    /// The files immediately inside, sorted by name.
    pub files: Vec<File>,
}

impl Directory {
    /// The names of the files immediately inside, for sibling checks.
    #[must_use]
    pub fn file_names(&self) -> Vec<String> {
        self.files.iter().map(|file| file.name.clone()).collect()
    }

    /// Whether a file with this exact name sits here.
    #[must_use]
    pub fn contains_file(&self, name: &str) -> bool {
        self.files.iter().any(|file| file.name == name)
    }
}

/// The repository, as a map from directory to its direct contents.
///
/// Ordered, because report output has to be byte-identical between runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoTree {
    directories: BTreeMap<RepoRelPath, Directory>,
}

impl RepoTree {
    /// What is directly inside `path`, if it is a directory in the tree.
    #[must_use]
    pub fn directory(&self, path: &RepoRelPath) -> Option<&Directory> {
        self.directories.get(path)
    }

    /// Every directory, root first, then in path order.
    pub fn directories(&self) -> impl Iterator<Item = (&RepoRelPath, &Directory)> {
        self.directories.iter()
    }

    /// Every file in the repository.
    ///
    /// Directory-major: all of one directory's files, then the next
    /// directory's. Deterministic, which is what a report needs; not globally
    /// path-sorted, which nothing needs.
    pub fn files(&self) -> impl Iterator<Item = &File> {
        self.directories.values().flat_map(|dir| dir.files.iter())
    }

    /// How many directories are in the tree.
    #[must_use]
    pub fn directory_count(&self) -> usize {
        self.directories.len()
    }
}

/// Walks `root` into a tree.
///
/// `.gitignore` is always honoured, plus the config's own `ignore` globs.
/// Hidden entries are skipped, which is what keeps `.git` and `node_modules`
/// caches out without anyone having to list them.
///
/// # Errors
/// [`WalkError::Unwalkable`] when the root cannot be read.
pub fn walk(root: &Utf8Path, config: &CompiledConfig) -> Result<RepoTree, WalkError> {
    let mut directories: BTreeMap<RepoRelPath, Directory> = BTreeMap::new();
    directories.insert(RepoRelPath::root(), Directory::default());

    let mut builder = ignore::WalkBuilder::new(root);
    builder
        // Honour `.gitignore` even outside a git repository: archwarden is
        // useful before `git init`, and a `.gitignore` still says what the
        // project considers noise.
        .require_git(false)
        .hidden(true)
        .parents(false);

    // Pruning rather than filtering afterwards. Two reasons: skipping a
    // directory has to skip everything under it, and an `ignore` of
    // `**/node_modules/**` should stop the walk at the boundary instead of
    // descending into a hundred thousand files only to discard each one.
    //
    // `filter_entry` demands a `'static` closure, so the two matchers are
    // cloned in rather than borrowed. Both are small.
    let pruned_root = root.to_owned();
    let ignore_globs = config.ignore_globs().clone();
    let skip_dirs = config.skip_dirs().clone();
    let removed_from_walk = skip_dirs.scope == archwarden_core::compiled::SkipScope::Walk;

    builder.filter_entry(move |entry| {
        let Ok(relative) = entry.path().strip_prefix(&pruned_root) else {
            return true;
        };
        let Some(relative) = Utf8Path::from_path(relative) else {
            return true;
        };
        let Ok(path) = RepoRelPath::new(relative.as_str()) else {
            return true;
        };
        if path.is_root() {
            return true;
        }

        if ignore_globs.is_match(path.as_path()) {
            return false;
        }

        let is_directory = entry.file_type().is_some_and(|kind| kind.is_dir());
        !(removed_from_walk && is_directory && skip_dirs.exempts(&path))
    });

    let walker = builder.build();

    for entry in walker {
        let entry = entry.map_err(|source| WalkError::Unwalkable {
            root: root.to_string(),
            source: Box::new(source),
        })?;

        // The first entry is the root itself, which is already in place.
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        let Some(relative) = Utf8Path::from_path(relative) else {
            // A non-UTF-8 path cannot be matched against any glob, so it could
            // never satisfy or violate a rule. Skipping is the only honest
            // thing to do with it.
            continue;
        };
        if relative.as_str().is_empty() {
            continue;
        }

        let Ok(path) = RepoRelPath::new(relative.as_str()) else {
            continue;
        };

        // Ignored and skipped entries were pruned by `filter_entry` above.
        let is_directory = entry.file_type().is_some_and(|kind| kind.is_dir());

        if is_directory {
            directories.entry(path.clone()).or_default();
        }

        let Some(parent) = path.parent() else {
            continue;
        };
        let Some(name) = path.file_name().map(ToOwned::to_owned) else {
            continue;
        };

        // A file whose parent was skipped has nowhere to go. `or_default`
        // would resurrect the skipped directory, so the entry is dropped.
        let Some(directory) = directories.get_mut(&parent) else {
            continue;
        };

        if is_directory {
            directory.subdirectories.push(name);
        } else {
            directory.files.push(File {
                name,
                path,
                class: FileClass::of(relative.file_name().unwrap_or_default()),
            });
        }
    }

    // `ignore` yields in an unspecified order; a report has to be identical
    // between runs.
    for directory in directories.values_mut() {
        directory.subdirectories.sort_unstable();
        directory.files.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    }

    Ok(RepoTree { directories })
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{compiled::SkipDirs, glob::PathSet, hash::ContentHash};
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

    fn config(ignore: &[&str], skip: SkipDirs) -> CompiledConfig {
        CompiledConfig::new(
            Vec::new(),
            PathSet::compile(ignore).expect("valid ignore"),
            skip,
            ContentHash::of(b""),
        )
    }

    fn plain() -> CompiledConfig {
        config(&[], SkipDirs::default())
    }

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn walked(entries: &[(&str, &str)], config: &CompiledConfig) -> RepoTree {
        let (guard, root) = tree_at(entries);
        let tree = walk(&root, config).expect("walks");
        drop(guard);
        tree
    }

    #[test]
    fn a_directory_reports_its_direct_children() {
        let tree = walked(
            &[
                ("packages/domain/src/user/user.ts", ""),
                ("packages/domain/src/user/types/id.ts", ""),
                ("packages/domain/src/invoice/invoice.ts", ""),
            ],
            &plain(),
        );

        let src = tree
            .directory(&path("packages/domain/src"))
            .expect("present");
        assert_eq!(src.subdirectories, ["invoice", "user"]);
        assert!(src.files.is_empty());

        let user = tree
            .directory(&path("packages/domain/src/user"))
            .expect("present");
        assert_eq!(user.subdirectories, ["types"]);
        assert_eq!(user.file_names(), ["user.ts"]);
    }

    /// The root is always in the tree, so a rule scoped to `.` has something
    /// to match against.
    #[test]
    fn the_repository_root_is_always_present() {
        let tree = walked(&[("package.json", "{}")], &plain());

        let root = tree.directory(&RepoRelPath::root()).expect("present");
        assert_eq!(root.file_names(), ["package.json"]);
        assert!(tree.directory_count() >= 1);
    }

    /// `.gitignore` is honoured even outside a git repository: archwarden is
    /// useful before `git init`, and the file still says what is noise.
    #[test]
    fn gitignore_is_honoured_without_a_git_repository() {
        let tree = walked(
            &[
                (".gitignore", "dist/\n*.generated.ts\n"),
                ("src/user.ts", ""),
                ("src/schema.generated.ts", ""),
                ("dist/bundle.js", ""),
            ],
            &plain(),
        );

        let files: Vec<_> = tree.files().map(|f| f.path.as_str()).collect();
        assert_eq!(files, ["src/user.ts"]);
        assert!(tree.directory(&path("dist")).is_none());
    }

    /// The config's own `ignore` stacks on top of `.gitignore`.
    #[test]
    fn config_ignore_globs_are_applied() {
        let tree = walked(
            &[
                ("src/user.ts", ""),
                ("src/generated/schema.ts", ""),
                ("vendor/lib.ts", ""),
            ],
            &config(&["**/generated/**", "vendor/**"], SkipDirs::default()),
        );

        let files: Vec<_> = tree.files().map(|f| f.path.as_str()).collect();
        assert_eq!(files, ["src/user.ts"]);
    }

    /// Hidden entries are skipped, which keeps `.git` out without anyone
    /// listing it.
    #[test]
    fn hidden_entries_are_skipped() {
        let tree = walked(
            &[
                ("src/user.ts", ""),
                (".git/config", ""),
                (".env", "SECRET=1"),
            ],
            &plain(),
        );

        let files: Vec<_> = tree.files().map(|f| f.path.as_str()).collect();
        assert_eq!(files, ["src/user.ts"]);
    }

    /// The default escape hatch is structural only: `_internal` stays in the
    /// tree, so its files remain visible to every non-structure rule. That is
    /// what closes the `mkdir _x && mv offender.ts _x/` bypass.
    #[test]
    fn the_default_escape_hatch_leaves_files_in_the_tree() {
        let tree = walked(
            &[("src/_internal/helper.ts", ""), ("src/user.ts", "")],
            &plain(),
        );

        // Directory-major order: `src`'s own files come before those of the
        // directories inside it.
        let files: Vec<_> = tree.files().map(|f| f.path.as_str()).collect();
        assert_eq!(files, ["src/user.ts", "src/_internal/helper.ts"]);
        assert!(tree.directory(&path("src/_internal")).is_some());
    }

    /// `scope: "walk"` is the opt-in that does remove them, and it removes the
    /// files inside too rather than leaving them orphaned.
    #[test]
    fn the_walk_scoped_escape_hatch_removes_the_directory_and_its_contents() {
        let skip = SkipDirs {
            prefixes: vec!["_".to_owned()],
            globs: PathSet::default(),
            scope: archwarden_core::compiled::SkipScope::Walk,
        };
        let tree = walked(
            &[
                ("src/_internal/helper.ts", ""),
                ("src/_internal/deep/more.ts", ""),
                ("src/user.ts", ""),
            ],
            &config(&[], skip),
        );

        let files: Vec<_> = tree.files().map(|f| f.path.as_str()).collect();
        assert_eq!(files, ["src/user.ts"]);
        assert!(tree.directory(&path("src/_internal")).is_none());
        assert!(tree.directory(&path("src/_internal/deep")).is_none());
        assert!(
            !tree
                .directory(&path("src"))
                .expect("present")
                .subdirectories
                .contains(&"_internal".to_owned())
        );
    }

    #[test]
    fn files_are_classified_by_extension() {
        let tree = walked(
            &[
                ("src/user.ts", ""),
                ("src/component.tsx", ""),
                ("src/legacy.mjs", ""),
                ("src/DOC.md", ""),
                ("src/data.json", ""),
                ("src/Makefile", ""),
            ],
            &plain(),
        );

        let src = tree.directory(&path("src")).expect("present");
        let classes: Vec<_> = src
            .files
            .iter()
            .map(|f| (f.name.as_str(), f.class))
            .collect();

        assert_eq!(
            classes,
            [
                ("DOC.md", FileClass::Other),
                ("Makefile", FileClass::Other),
                ("component.tsx", FileClass::Source),
                ("data.json", FileClass::Other),
                ("legacy.mjs", FileClass::Source),
                ("user.ts", FileClass::Source),
            ]
        );
    }

    /// A spec file is a source file. Whether it counts as "a spec" is a
    /// question only a rule's `spec_suffix` can answer, so the tree does not
    /// pretend to know.
    #[test]
    fn a_spec_file_is_classified_as_source() {
        assert_eq!(FileClass::of("user.spec.ts"), FileClass::Source);
        assert_eq!(FileClass::of("user.ts"), FileClass::Source);
        assert_eq!(FileClass::of("README.md"), FileClass::Other);
        assert_eq!(FileClass::of("no-extension"), FileClass::Other);
        assert_eq!(FileClass::of(""), FileClass::Other);
    }

    /// Report output has to be byte-identical between runs, so the tree is
    /// ordered rather than however the filesystem happened to yield.
    #[test]
    fn contents_are_ordered_deterministically() {
        let entries = [
            ("src/zebra.ts", ""),
            ("src/alpha.ts", ""),
            ("src/middle.ts", ""),
            ("src/zzz/x.ts", ""),
            ("src/aaa/y.ts", ""),
        ];

        let first = walked(&entries, &plain());
        let second = walked(&entries, &plain());

        let src = first.directory(&path("src")).expect("present");
        assert_eq!(src.file_names(), ["alpha.ts", "middle.ts", "zebra.ts"]);
        assert_eq!(src.subdirectories, ["aaa", "zzz"]);

        let names =
            |tree: &RepoTree| -> Vec<String> { tree.files().map(|f| f.path.to_string()).collect() };
        assert_eq!(names(&first), names(&second));
    }

    #[test]
    fn sibling_lookup_answers_by_exact_name() {
        let tree = walked(&[("src/user.ts", ""), ("src/user.spec.ts", "")], &plain());

        let src = tree.directory(&path("src")).expect("present");
        assert!(src.contains_file("user.spec.ts"));
        assert!(!src.contains_file("user.test.ts"));
    }

    /// An empty directory is still a directory, and a structure rule has to be
    /// able to say something about it.
    #[test]
    fn an_empty_directory_is_still_in_the_tree() {
        let (guard, root) = tree_at(&[("src/user.ts", "")]);
        std::fs::create_dir_all(root.join("src/empty")).expect("create dir");
        let tree = walk(&root, &plain()).expect("walks");
        drop(guard);

        let empty = tree.directory(&path("src/empty")).expect("present");
        assert!(empty.files.is_empty());
        assert!(empty.subdirectories.is_empty());
    }

    #[test]
    fn a_root_that_does_not_exist_is_an_error() {
        let (_guard, root) = tree_at(&[("src/user.ts", "")]);
        let missing = root.join("nowhere");

        assert!(matches!(
            walk(&missing, &plain()),
            Err(WalkError::Unwalkable { .. })
        ));
    }
}
