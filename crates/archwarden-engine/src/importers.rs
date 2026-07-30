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

/// One importing file, and the imports in it that reach the target.
///
/// The specifiers are what a rewrite edits, and there can be more than one in
/// a file: `import type { A }` and `import { b }` from the same module are two
/// statements, and a move has to touch both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Importer {
    /// The importing file.
    pub path: RepoRelPath,
    /// The imports in it that resolve to the target, in source order.
    pub imports: Vec<archwarden_core::facts::ImportFact>,
}

/// Which files import a target, and which files nobody can be sure about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Importers {
    /// Files with an import that resolves to the target, in path order.
    pub direct: Vec<Importer>,
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
    importers_of_each(root, config, tree, std::slice::from_ref(target))
        .remove(target)
        .unwrap_or_default()
}

/// The same question asked about several targets at once.
///
/// One pass, not one per target. A batch move of nine files would otherwise
/// resolve the repository nine times — about seven seconds on a repository
/// where a `check` takes two hundred milliseconds — for an answer that comes
/// out of the same single traversal.
///
/// Every target gets an entry, including one nothing imports: an empty list is
/// the answer that makes a move safe, and a missing key would be
/// indistinguishable from a target nobody asked about.
#[must_use]
pub fn importers_of_each(
    root: &Utf8Path,
    config: &CompiledConfig,
    tree: &RepoTree,
    targets: &[RepoRelPath],
) -> std::collections::BTreeMap<RepoRelPath, Importers> {
    let resolver = archwarden_resolver::imports::ImportResolver::new(root);
    let parser = archwarden_parser::oxc::OxcParser;

    let wanted: std::collections::BTreeSet<&RepoRelPath> = targets.iter().collect();
    let mut found: std::collections::BTreeMap<RepoRelPath, Importers> = targets
        .iter()
        .map(|target| (target.clone(), Importers::default()))
        .collect();
    let mut opaque = Vec::new();

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
            opaque.push(file.path.clone());
        }

        crate::resolve::resolve_imports(&resolver, &mut facts);

        // Grouped by which target each import reaches, so one traversal
        // answers for every target at once.
        let mut by_target: std::collections::BTreeMap<&RepoRelPath, Vec<_>> =
            std::collections::BTreeMap::new();
        for import in &facts.imports {
            let Some(landed) = import.resolved.as_ref() else {
                continue;
            };
            if let Some(target) = wanted.get(landed) {
                by_target.entry(target).or_default().push(import.clone());
            }
        }

        for (target, imports) in by_target {
            if let Some(entry) = found.get_mut(target) {
                entry.direct.push(Importer {
                    path: file.path.clone(),
                    imports,
                });
            }
        }
    }

    // Determinism is a design goal, and the walk's order is not one a reader
    // can predict.
    opaque.sort();
    for entry in found.values_mut() {
        entry.direct.sort_by(|a, b| a.path.cmp(&b.path));
        entry.opaque.clone_from(&opaque);
    }
    found
}

/// Every in-repository import edge, read backwards.
///
/// [`importers_of_each`] answers about a known list of targets.
/// This answers about all of them at once, which is what a question about
/// *every* folder needs — and it is the same single traversal, so asking about
/// the whole repository costs what asking about one file costs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReverseIndex {
    entries: std::collections::BTreeMap<RepoRelPath, Vec<RepoRelPath>>,
    opaque: Vec<RepoRelPath>,
}

impl ReverseIndex {
    /// Builds one directly, for tests.
    #[must_use]
    pub fn from_pairs(
        entries: std::collections::BTreeMap<RepoRelPath, Vec<RepoRelPath>>,
        opaque: Vec<RepoRelPath>,
    ) -> Self {
        Self { entries, opaque }
    }

    /// Every file, with the files that import it.
    ///
    /// Every source file in the walk has an entry, including one nothing
    /// imports: an empty list is the answer the question is largely about, and
    /// a missing key would be indistinguishable from a file nobody looked at.
    pub fn entries(&self) -> impl Iterator<Item = (&RepoRelPath, &[RepoRelPath])> {
        self.entries
            .iter()
            .map(|(path, importers)| (path, importers.as_slice()))
    }

    /// Files containing a dynamic import naming no module.
    #[must_use]
    pub fn opaque(&self) -> &[RepoRelPath] {
        &self.opaque
    }
}

/// Reads every import in the repository backwards, in one pass.
#[must_use]
pub fn reverse_index(root: &Utf8Path, config: &CompiledConfig, tree: &RepoTree) -> ReverseIndex {
    let resolver = archwarden_resolver::imports::ImportResolver::new(root);
    let parser = archwarden_parser::oxc::OxcParser;

    let mut index = ReverseIndex::default();

    // Seeded with every source file, so a file nothing imports is present with
    // an empty list rather than absent. That row is most of the point.
    for file in tree.files() {
        if file.class == FileClass::Source && !config.is_ignored(&file.path) {
            index.entries.entry(file.path.clone()).or_default();
        }
    }

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
            index.opaque.push(file.path.clone());
        }

        crate::resolve::resolve_imports(&resolver, &mut facts);
        for import in &facts.imports {
            let Some(landed) = import.resolved.as_ref() else {
                continue;
            };
            // A file importing itself is not an importer of it. It happens
            // through an index re-export, and counting it would make a folder
            // look used by its own module when nothing outside it is.
            if landed == &file.path {
                continue;
            }
            if let Some(importers) = index.entries.get_mut(landed) {
                importers.push(file.path.clone());
            }
        }
    }

    index.opaque.sort();
    for importers in index.entries.values_mut() {
        importers.sort();
        importers.dedup();
    }
    index
}

/// One file's text and its imports, resolved.
///
/// The pipeline reads, parses and resolves in three places already; this is
/// the fourth caller's version of it, kept here because `archwarden-cli` has
/// no business depending on the parser directly — the crate graph in
/// `docs/ARCHITECTURE.md` puts the front-end behind the engine.
///
/// The text comes back with the facts because a caller rewriting a specifier
/// needs the exact bytes the spans were measured against, and re-reading could
/// get a different file.
///
/// # Errors
/// A message naming what went wrong, for a caller that must refuse rather than
/// carry on with a file it could not read.
pub fn resolved_facts(
    root: &Utf8Path,
    path: &RepoRelPath,
    resolver: &archwarden_resolver::imports::ImportResolver,
) -> Result<(String, archwarden_core::facts::FileFacts), String> {
    use archwarden_core::traits::Parser as _;

    let source =
        std::fs::read_to_string(root.join(path.as_path())).map_err(|error| error.to_string())?;
    let hash = archwarden_core::hash::ContentHash::of(source.as_bytes());
    let mut facts = archwarden_parser::oxc::OxcParser
        .parse(path, &source, hash)
        .map_err(|_| "will not parse".to_owned())?;

    crate::resolve::resolve_imports(resolver, &mut facts);
    Ok((source, facts))
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

    /// Just the importing paths, for assertions that are not about specifiers.
    fn paths(found: &Importers) -> Vec<RepoRelPath> {
        found.direct.iter().map(|i| i.path.clone()).collect()
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
            paths(&found),
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
