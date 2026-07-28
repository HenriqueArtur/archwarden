//! Who imports a file.
//!
//! Every other question archwarden answers reads the import graph forwards:
//! this file imports those. "What breaks if I move this" is the only one that
//! needs it backwards, and there is no stored inverse — so it is built by
//! resolving the repository once, which costs about what a `check` costs.
//!
//! That was worth measuring before building. A `check` over 3778 files takes
//! about 600ms, and this asks the same work of the same files. For a question
//! asked a few times during a refactor, that is not expensive; the reason
//! there is no cached inverse is not cost but correctness — resolution depends
//! on `tsconfig` and lockfiles, which no content hash covers, so a stored one
//! would serve stale paths the day someone edits an alias.
//!
//! # What it cannot see
//!
//! `import(name)` and ``import(`./locales/${name}`)`` name no single module,
//! so the parser records nothing for them, and neither does this. A caller
//! about to rewrite imports has to be told that rather than left to find out:
//! [`Importers::opaque`] lists the files that contain one.

use archwarden_core::{
    compiled::CompiledConfig,
    path::{FileClass, RepoRelPath},
    traits::Parser as _,
};
use camino::Utf8Path;

use crate::walk::RepoTree;

/// Which files import a target, and which files nobody can be sure about.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Importers {
    /// Files with an import that resolves to the target, in path order.
    pub direct: Vec<RepoRelPath>,
    /// Files containing a dynamic import archwarden cannot read.
    ///
    /// Not an error and not a finding: a statement that the answer above is
    /// incomplete for these, and that a human has to look. Reporting them is
    /// the difference between a tool that is honest about its blind spot and
    /// one that quietly hands over a wrong answer.
    pub opaque: Vec<RepoRelPath>,
}

/// Every file that imports `target`.
///
/// Resolves the whole repository, because the answer cannot be known from any
/// smaller part of it.
#[must_use]
pub fn importers_of(
    root: &Utf8Path,
    config: &CompiledConfig,
    tree: &RepoTree,
    target: &RepoRelPath,
) -> Importers {
    let resolver = archwarden_resolver::imports::ImportResolver::new(root);
    let parser = archwarden_parser::oxc::OxcParser;

    let mut found = Importers::default();

    for file in tree.files() {
        if file.class != FileClass::Source || config.is_ignored(&file.path) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(root.join(file.path.as_path())) else {
            continue;
        };
        let hash = archwarden_core::hash::ContentHash::of(source.as_bytes());
        let Ok(mut facts) = parser.parse(&file.path, &source, hash) else {
            continue;
        };

        if facts.has_opaque_import {
            found.opaque.push(file.path.clone());
        }

        crate::resolve::resolve_imports(&resolver, &mut facts);
        if facts
            .imports
            .iter()
            .any(|import| import.resolved.as_ref() == Some(target))
        {
            found.direct.push(file.path.clone());
        }
    }

    // Determinism is a design goal, and the walk's order is not one a reader
    // can predict.
    found.direct.sort();
    found.opaque.sort();
    found
}

/// How many of a file's own imports are written relative to it.
///
/// Counted rather than resolved after a hypothetical move, because whether
/// they still point somewhere is a question `tsc` answers better than
/// archwarden ever will. The number is here so a reader knows the size of the
/// mechanical half of a move.
#[must_use]
pub fn relative_imports(root: &Utf8Path, path: &RepoRelPath) -> usize {
    let Ok(source) = std::fs::read_to_string(root.join(path.as_path())) else {
        return 0;
    };
    let hash = archwarden_core::hash::ContentHash::of(source.as_bytes());
    let Ok(facts) = archwarden_parser::oxc::OxcParser.parse(path, &source, hash) else {
        return 0;
    };

    facts
        .imports
        .iter()
        .filter(|import| import.specifier.starts_with('.'))
        .count()
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
            std::fs::create_dir_all(path.parent().expect("a file has a parent"))
                .expect("create dirs");
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

    fn importers(entries: &[(&str, &str)], target: &str) -> Importers {
        let (guard, root) = tree_at(entries);
        let config = config();
        let tree = crate::walk::walk(&root, &config).expect("walks");
        let found = importers_of(
            &root,
            &config,
            &tree,
            &RepoRelPath::new(target).expect("valid path"),
        );
        drop(guard);
        found
    }

    /// The question the module exists for, read backwards.
    #[test]
    fn a_files_importers_are_found() {
        let found = importers(
            &[
                ("src/target.ts", "export const value = 1;\n"),
                ("src/a.ts", "import { value } from './target';\n"),
                ("src/b.ts", "import { value } from './target';\n"),
                ("src/unrelated.ts", "export const other = 2;\n"),
            ],
            "src/target.ts",
        );

        assert_eq!(
            found.direct,
            [
                RepoRelPath::new("src/a.ts").expect("valid"),
                RepoRelPath::new("src/b.ts").expect("valid"),
            ]
        );
    }

    /// Through a directory index, which is how most of a repository imports
    /// most of the rest of it. A textual search for the filename would miss
    /// every one of these.
    #[test]
    fn an_import_through_an_index_counts() {
        let found = importers(
            &[
                ("src/thing/index.ts", "export const value = 1;\n"),
                ("src/a.ts", "import { value } from './thing';\n"),
            ],
            "src/thing/index.ts",
        );

        assert_eq!(found.direct.len(), 1, "{:?}", found.direct);
    }

    /// A type-only import is still an import: moving the file breaks it just
    /// the same, whatever a boundary rule may have decided to ignore.
    #[test]
    fn a_type_only_import_counts() {
        let found = importers(
            &[
                ("src/target.ts", "export type Value = number;\n"),
                ("src/a.ts", "import type { Value } from './target';\n"),
            ],
            "src/target.ts",
        );

        assert_eq!(found.direct.len(), 1);
    }

    /// Nobody importing it is a real answer, and the one that makes a move
    /// safe.
    #[test]
    fn a_file_nobody_imports_has_no_importers() {
        let found = importers(
            &[
                ("src/target.ts", "export const value = 1;\n"),
                ("src/a.ts", "export const other = 2;\n"),
            ],
            "src/target.ts",
        );

        assert!(found.direct.is_empty());
        assert!(found.opaque.is_empty());
    }

    /// The blind spot, said out loud. `import(name)` names no module, so this
    /// file may or may not import the target and nothing here can tell —
    /// which a caller about to rewrite imports has to know.
    #[test]
    fn a_file_with_a_dynamic_import_is_reported_as_opaque() {
        let found = importers(
            &[
                ("src/target.ts", "export const value = 1;\n"),
                (
                    "src/loader.ts",
                    "export async function load(name: string) { return import(name); }\n",
                ),
            ],
            "src/target.ts",
        );

        assert!(found.direct.is_empty());
        assert_eq!(
            found.opaque,
            [RepoRelPath::new("src/loader.ts").expect("valid")]
        );
    }

    /// A literal dynamic import is not opaque: it names one module, and the
    /// parser records it like any other.
    #[test]
    fn a_literal_dynamic_import_is_an_ordinary_one() {
        let found = importers(
            &[
                ("src/target.ts", "export const value = 1;\n"),
                (
                    "src/a.ts",
                    "export const load = () => import('./target');\n",
                ),
            ],
            "src/target.ts",
        );

        assert_eq!(found.direct.len(), 1);
        assert!(found.opaque.is_empty(), "{:?}", found.opaque);
    }

    /// An ignored file is not an importer for this purpose either: `check`
    /// does not report it, and a move is not blocked by it.
    #[test]
    fn an_ignored_file_is_not_counted() {
        let (guard, root) = tree_at(&[
            ("src/target.ts", "export const value = 1;\n"),
            ("vendor/a.ts", "import { value } from '../src/target';\n"),
        ]);
        let config = CompiledConfig::new(
            Vec::new(),
            PathSet::compile(["vendor/**".to_owned()]).expect("valid glob"),
            SkipDirs::default(),
            ContentHash::of(b""),
        );
        let tree = crate::walk::walk(&root, &config).expect("walks");
        let found = importers_of(
            &root,
            &config,
            &tree,
            &RepoRelPath::new("src/target.ts").expect("valid"),
        );
        drop(guard);

        assert!(found.direct.is_empty(), "{:?}", found.direct);
    }

    /// Only the relative ones: a package specifier means the same thing from
    /// anywhere in the repository, and a move does not touch it.
    #[test]
    fn only_relative_imports_are_counted() {
        let (guard, root) = tree_at(&[
            ("src/target.ts", "export const value = 1;\n"),
            (
                "src/a.ts",
                "import { a } from './target';\n\
                 import { b } from '../src/target';\n\
                 import { c } from '@org/domain';\n\
                 import { d } from 'react';\n",
            ),
        ]);
        let count = relative_imports(&root, &RepoRelPath::new("src/a.ts").expect("valid"));
        drop(guard);

        assert_eq!(count, 2);
    }

    /// A file that is not there has no imports to rewrite, and asking about
    /// one is not an error -- `impact` answers about a path that may not exist
    /// yet, like `describe` does.
    #[test]
    fn a_file_that_is_not_there_counts_nothing() {
        let (guard, root) = tree_at(&[("src/a.ts", "export const a = 1;\n")]);
        let count = relative_imports(&root, &RepoRelPath::new("src/gone.ts").expect("valid"));
        drop(guard);

        assert_eq!(count, 0);
    }

    /// A file that will not parse is passed over rather than stopping the
    /// answer. It is reported by `check`, which is where a broken file
    /// belongs; refusing to answer a refactoring question because some
    /// unrelated file is malformed would be the wrong trade.
    #[test]
    fn a_file_that_will_not_parse_does_not_stop_the_search() {
        let (guard, root) = tree_at(&[
            ("src/target.ts", "export const value = 1;\n"),
            ("src/a.ts", "import { value } from './target';\n"),
        ]);
        std::fs::write(root.join("src/broken.ts"), [0x65, 0x78, 0xff, 0xfe]).expect("write");
        let config = config();
        let tree = crate::walk::walk(&root, &config).expect("walks");
        let found = importers_of(
            &root,
            &config,
            &tree,
            &RepoRelPath::new("src/target.ts").expect("valid"),
        );
        drop(guard);

        assert_eq!(
            found.direct.len(),
            1,
            "the readable importer is still found"
        );
    }
}
