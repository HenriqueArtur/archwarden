//! A resolver that answers from a table instead of a filesystem.
//!
//! The graph stage has to be testable without building a `node_modules`, a
//! `tsconfig` and a symlink farm for every case. This is the fixture the
//! architecture note asks for: a resolver whose whole behaviour is a list of
//! answers written out in the test.
//!
//! It is not a simulation of Node resolution and must never grow into one. The
//! moment a test needs *resolution* to be right rather than *the graph*, it
//! belongs in `imports.rs` against a real tree.

use std::collections::HashMap;

use archwarden_core::{
    path::RepoRelPath,
    traits::{Resolved, Resolver},
};

/// Why an in-memory resolution failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MemoryError {
    /// No answer was recorded for this pair.
    #[error("no answer recorded for `{specifier}` from `{importer}`")]
    Unrecorded {
        /// The specifier, as written.
        specifier: String,
        /// The file that imported it.
        importer: RepoRelPath,
    },
}

/// A resolver backed by a table of answers.
///
/// Answers are looked up by importer *and* specifier, then by specifier alone.
/// The second lookup is what keeps a fixture short: a package name resolves to
/// the same place from everywhere, and repeating the importer for each of
/// twenty files would bury the case being tested.
#[derive(Debug, Default)]
pub struct InMemoryResolver {
    by_pair: HashMap<(RepoRelPath, String), Resolved>,
    by_specifier: HashMap<String, Resolved>,
}

impl InMemoryResolver {
    /// An empty table, in which nothing resolves.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an answer for one specifier, whoever imports it.
    #[must_use]
    pub fn with(mut self, specifier: &str, resolved: Resolved) -> Self {
        self.by_specifier.insert(specifier.to_owned(), resolved);
        self
    }

    /// Records an answer for one specifier written in one specific file.
    ///
    /// Needed for relative specifiers, where `./user` means a different file
    /// in every directory.
    #[must_use]
    pub fn with_from(
        mut self,
        importer: &RepoRelPath,
        specifier: &str,
        resolved: Resolved,
    ) -> Self {
        self.by_pair
            .insert((importer.clone(), specifier.to_owned()), resolved);
        self
    }
}

impl Resolver for InMemoryResolver {
    type Error = MemoryError;

    fn resolve(&self, importer: &RepoRelPath, specifier: &str) -> Result<Resolved, MemoryError> {
        // The specific answer wins: a fixture that records both means the pair
        // is the exception it went to the trouble of writing down.
        self.by_pair
            .get(&(importer.clone(), specifier.to_owned()))
            .or_else(|| self.by_specifier.get(specifier))
            .cloned()
            .ok_or_else(|| MemoryError::Unrecorded {
                specifier: specifier.to_owned(),
                importer: importer.clone(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn in_repo(p: &str) -> Resolved {
        Resolved::InRepo(path(p))
    }

    /// The short form: one answer, good from anywhere. This is what a fixture
    /// for a package name looks like.
    #[test]
    fn an_answer_recorded_by_specifier_holds_for_every_importer() {
        let resolver =
            InMemoryResolver::new().with("@org/domain", in_repo("packages/domain/src/index.ts"));

        for importer in ["src/a.ts", "packages/ui/src/deep/b.ts"] {
            assert_eq!(
                resolver
                    .resolve(&path(importer), "@org/domain")
                    .expect("recorded"),
                in_repo("packages/domain/src/index.ts")
            );
        }
    }

    /// Relative specifiers mean different files in different directories, so
    /// the pair form has to exist.
    #[test]
    fn an_answer_recorded_for_a_pair_is_specific_to_that_importer() {
        let resolver = InMemoryResolver::new()
            .with_from(&path("src/a/one.ts"), "./x", in_repo("src/a/x.ts"))
            .with_from(&path("src/b/two.ts"), "./x", in_repo("src/b/x.ts"));

        assert_eq!(
            resolver.resolve(&path("src/a/one.ts"), "./x").expect("a"),
            in_repo("src/a/x.ts")
        );
        assert_eq!(
            resolver.resolve(&path("src/b/two.ts"), "./x").expect("b"),
            in_repo("src/b/x.ts")
        );
    }

    /// A fixture that bothered to write the pair down meant it as an
    /// exception, so the pair wins over the general answer.
    #[test]
    fn the_specific_answer_beats_the_general_one() {
        let resolver = InMemoryResolver::new()
            .with("@org/domain", in_repo("packages/domain/src/index.ts"))
            .with_from(
                &path("packages/legacy/src/a.ts"),
                "@org/domain",
                in_repo("packages/domain/src/legacy.ts"),
            );

        assert_eq!(
            resolver
                .resolve(&path("packages/legacy/src/a.ts"), "@org/domain")
                .expect("the exception"),
            in_repo("packages/domain/src/legacy.ts")
        );
        assert_eq!(
            resolver
                .resolve(&path("packages/app/src/a.ts"), "@org/domain")
                .expect("the rule"),
            in_repo("packages/domain/src/index.ts")
        );
    }

    /// Externals and builtins are answers too: a boundary rule that forbids
    /// `node:fs` has to be testable without a Node installation.
    #[test]
    fn externals_and_builtins_are_recordable() {
        let resolver = InMemoryResolver::new()
            .with("node:fs", Resolved::Builtin("fs".to_owned()))
            .with(
                "lodash",
                Resolved::External("/tmp/node_modules/lodash/index.js".into()),
            );

        assert_eq!(
            resolver.resolve(&path("src/a.ts"), "node:fs").expect("fs"),
            Resolved::Builtin("fs".to_owned())
        );
        assert_eq!(
            resolver
                .resolve(&path("src/a.ts"), "lodash")
                .expect("lodash"),
            Resolved::External("/tmp/node_modules/lodash/index.js".into())
        );
    }

    /// A specifier nobody wrote down is an error naming both halves, so a
    /// forgotten fixture line reads as itself rather than as a rule bug.
    #[test]
    fn an_unrecorded_specifier_says_which_one_it_was() {
        let error = InMemoryResolver::new()
            .resolve(&path("src/a.ts"), "@org/nothing")
            .expect_err("nothing recorded");

        assert_eq!(
            error.to_string(),
            "no answer recorded for `@org/nothing` from `src/a.ts`"
        );
    }
}
