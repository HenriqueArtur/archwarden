//! Turning specifiers into paths.
//!
//! A boundary rule matches globs against where an import *lands*. The parser
//! only sees what was written, so something has to close the gap, and this is
//! it: one pass over a file's imports, filling in the resolved path.
//!
//! Deliberately not a graph, still. The *forward* graph now exists —
//! [`archwarden_core::graph::ImportGraph`], built by `run.rs` from what this
//! pass fills in, for the two rules that ask about more than one file. It is
//! built there rather than here because it is built only when a rule asks, and
//! this pass runs whenever any boundary rule does.
//!
//! The **reverse** index `ARCHITECTURE.md:195` describes — "if file A changed,
//! who imports A?" — is what is still absent. It exists to invalidate a cache
//! incrementally, and v0 has no watch mode and re-checks the whole repository
//! every run. Building an index with no reader would be code nobody can test
//! against a requirement.

use archwarden_core::{
    facts::FileFacts,
    path::RepoRelPath,
    traits::{Resolved, Resolver},
};

/// What became of a set of imports.
///
/// The three kinds that resolved are counted and nothing more: a reader who
/// wants to know which file imports `lodash` has `grep`. The ones that did not
/// resolve are also named, because those are the imports no boundary rule
/// could see, and a count of blind spots is not something anyone can act on --
/// the only way left to find them is to delete imports until the number moves.
/// Issue #18.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcomes {
    /// Landed on a file in this repository. The only kind a v0 boundary rule
    /// can match, because its globs are repo-relative paths.
    pub in_repo: usize,
    /// Landed on a real file outside the repository: an installed dependency.
    pub external: usize,
    /// A runtime builtin such as `node:fs`, which has no file at all.
    pub builtin: usize,
    /// Did not resolve. Boundary rules cannot see these, which is worth
    /// saying out loud when it happens.
    pub unresolved: usize,
    /// Which file wrote which specifier, for every import counted in
    /// [`unresolved`](Self::unresolved).
    ///
    /// One entry per counted import rather than per distinct pair, so the two
    /// can never disagree: a reader told "3 imports" and shown two lines would
    /// be right to wonder where the third went. A file that writes the same
    /// unresolvable specifier twice is two imports and appears twice.
    ///
    /// The renderer is what decides how many of these a human should be shown.
    /// On a repository whose dependencies are not installed this is every bare
    /// specifier in it, which is a real list and a useless wall of text.
    pub unresolved_imports: Vec<(RepoRelPath, String)>,
}

impl Outcomes {
    /// Folds another tally into this one.
    pub fn absorb(&mut self, mut other: Self) {
        self.in_repo += other.in_repo;
        self.external += other.external;
        self.builtin += other.builtin;
        self.unresolved += other.unresolved;
        self.unresolved_imports
            .append(&mut other.unresolved_imports);
    }

    /// How many imports were counted in total.
    #[must_use]
    pub fn total(&self) -> usize {
        self.in_repo + self.external + self.builtin + self.unresolved
    }
}

/// Fills in where each of `facts`'s imports lands, and says what happened.
///
/// Only in-repo results reach [`ImportFact::resolved`](archwarden_core::facts::ImportFact::resolved):
/// a rule's globs are repo-relative paths, so a dependency and a builtin have
/// nothing a glob could match. They are counted rather than stored, which is
/// what keeps "resolved to a dependency" distinguishable from "did not
/// resolve" without teaching the fact type about either.
pub fn resolve_imports<R: Resolver>(resolver: &R, facts: &mut FileFacts) -> Outcomes {
    let mut outcomes = Outcomes::default();

    for import in &mut facts.imports {
        match resolver.resolve(&facts.path, &import.specifier) {
            Ok(Resolved::InRepo(path)) => {
                import.resolved = Some(path);
                outcomes.in_repo += 1;
            }
            Ok(Resolved::External(_)) => outcomes.external += 1,
            Ok(Resolved::Builtin(_)) => outcomes.builtin += 1,
            // A variant added later is not something this pass can pretend to
            // understand, so it counts as unresolved rather than as a match.
            Ok(_) | Err(_) => {
                outcomes.unresolved += 1;
                outcomes
                    .unresolved_imports
                    .push((facts.path.clone(), import.specifier.clone()));
            }
        }
    }

    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        facts::{ImportFact, Span},
        hash::ContentHash,
        path::RepoRelPath,
    };
    use archwarden_resolver::memory::InMemoryResolver;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// Facts for `src/app.ts` importing each of `specifiers`, as the parser
    /// would hand them over: written down, not yet resolved.
    fn facts(specifiers: &[&str]) -> FileFacts {
        let mut facts = FileFacts::unparsed(path("src/app.ts"), ContentHash::of(b"source"));
        for specifier in specifiers {
            facts.imports.push(ImportFact {
                specifier: (*specifier).to_owned(),
                resolved: None,
                type_only: false,
                names: Vec::new(),
                span: Span::new(0, 10),
            });
        }
        facts
    }

    /// The whole point: a rule matches globs against where an import landed,
    /// and until this pass runs there is nowhere for it to have landed.
    #[test]
    fn an_in_repo_import_gets_its_path_filled_in() {
        let resolver = InMemoryResolver::new().with(
            "@/domain/user",
            Resolved::InRepo(path("src/domain/user.ts")),
        );
        let mut facts = facts(&["@/domain/user"]);

        let outcomes = resolve_imports(&resolver, &mut facts);

        assert_eq!(
            facts.imports.first().and_then(|i| i.resolved.as_ref()),
            Some(&path("src/domain/user.ts"))
        );
        assert_eq!(
            outcomes,
            Outcomes {
                in_repo: 1,
                ..Outcomes::default()
            }
        );
    }

    /// An installed dependency has a path, but not one any repo-relative glob
    /// could match. It is counted, not stored -- which is what keeps it
    /// distinguishable from an import that failed to resolve.
    #[test]
    fn a_dependency_is_counted_rather_than_stored() {
        let resolver = InMemoryResolver::new().with(
            "lodash",
            Resolved::External("/repo/node_modules/lodash/index.js".into()),
        );
        let mut facts = facts(&["lodash"]);

        let outcomes = resolve_imports(&resolver, &mut facts);

        assert!(facts.imports.first().is_some_and(|i| i.resolved.is_none()));
        assert_eq!(
            outcomes,
            Outcomes {
                external: 1,
                ..Outcomes::default()
            }
        );
    }

    /// A builtin has no file at all, and is counted apart from a dependency so
    /// a future rule about `node:fs` has somewhere to start.
    #[test]
    fn a_builtin_is_counted_on_its_own() {
        let resolver =
            InMemoryResolver::new().with("node:fs", Resolved::Builtin("node:fs".to_owned()));
        let mut facts = facts(&["node:fs"]);

        assert_eq!(
            resolve_imports(&resolver, &mut facts),
            Outcomes {
                builtin: 1,
                ..Outcomes::default()
            }
        );
    }

    /// An import nothing could resolve is the case a boundary rule is blind
    /// to, so it is counted separately and reported.
    #[test]
    fn an_unresolvable_import_is_counted_as_such() {
        let mut facts = facts(&["@org/never-installed"]);

        assert_eq!(
            resolve_imports(&InMemoryResolver::new(), &mut facts),
            Outcomes {
                unresolved: 1,
                unresolved_imports: vec![(path("src/app.ts"), "@org/never-installed".to_owned())],
                ..Outcomes::default()
            }
        );
        assert!(facts.imports.first().is_some_and(|i| i.resolved.is_none()));
    }

    /// A `import type` still resolves. Whether it counts is the rule's
    /// decision (`include_type_only`), not this pass's.
    #[test]
    fn a_type_only_import_resolves_like_any_other() {
        let resolver =
            InMemoryResolver::new().with("./types", Resolved::InRepo(path("src/types.ts")));
        let mut facts = facts(&["./types"]);
        if let Some(import) = facts.imports.first_mut() {
            import.type_only = true;
        }

        resolve_imports(&resolver, &mut facts);

        assert_eq!(
            facts.imports.first().and_then(|i| i.resolved.as_ref()),
            Some(&path("src/types.ts"))
        );
    }

    /// One file's imports usually land in more than one place, and the tally
    /// has to keep them apart.
    #[test]
    fn a_mixed_file_is_tallied_by_kind() {
        let resolver = InMemoryResolver::new()
            .with(
                "@/domain/user",
                Resolved::InRepo(path("src/domain/user.ts")),
            )
            .with("./local", Resolved::InRepo(path("src/local.ts")))
            .with(
                "lodash",
                Resolved::External("/repo/node_modules/lodash/index.js".into()),
            )
            .with("node:fs", Resolved::Builtin("node:fs".to_owned()));
        let mut facts = facts(&[
            "@/domain/user",
            "./local",
            "lodash",
            "node:fs",
            "@org/never-installed",
        ]);

        let outcomes = resolve_imports(&resolver, &mut facts);

        assert_eq!(
            outcomes,
            Outcomes {
                in_repo: 2,
                external: 1,
                builtin: 1,
                unresolved: 1,
                unresolved_imports: vec![(path("src/app.ts"), "@org/never-installed".to_owned())],
            }
        );
        assert_eq!(outcomes.total(), 5);
    }

    /// A file that imports nothing costs nothing and says so.
    #[test]
    fn a_file_with_no_imports_tallies_zero() {
        let mut facts = facts(&[]);
        let outcomes = resolve_imports(&InMemoryResolver::new(), &mut facts);

        assert_eq!(outcomes, Outcomes::default());
        assert_eq!(outcomes.total(), 0);
    }

    /// The run-level tally is the sum of the per-file ones.
    #[test]
    fn tallies_add_up_across_files() {
        let mut total = Outcomes::default();
        total.absorb(Outcomes {
            in_repo: 2,
            external: 1,
            builtin: 0,
            unresolved: 3,
            unresolved_imports: vec![
                (path("src/a.ts"), "@one".to_owned()),
                (path("src/a.ts"), "@two".to_owned()),
                (path("src/a.ts"), "@three".to_owned()),
            ],
        });
        total.absorb(Outcomes {
            in_repo: 5,
            external: 0,
            builtin: 4,
            unresolved: 0,
            unresolved_imports: Vec::new(),
        });

        assert_eq!(
            total,
            Outcomes {
                in_repo: 7,
                external: 1,
                builtin: 4,
                unresolved: 3,
                unresolved_imports: vec![
                    (path("src/a.ts"), "@one".to_owned()),
                    (path("src/a.ts"), "@two".to_owned()),
                    (path("src/a.ts"), "@three".to_owned()),
                ],
            }
        );
        assert_eq!(total.total(), 15);
    }

    /// The blind spot is named, not only counted. A boundary rule cannot see
    /// an import that did not resolve, and until this list existed the only
    /// way to find out which import that was, in a repository of four
    /// thousand files, was to delete imports until the count moved. Issue #18.
    #[test]
    fn every_unresolved_import_is_named_with_the_file_that_wrote_it() {
        let resolver =
            InMemoryResolver::new().with("./local", Resolved::InRepo(path("src/local.ts")));
        let mut facts = facts(&["@Domain/Order/types", "./local", "@Domain/Order/id"]);

        let outcomes = resolve_imports(&resolver, &mut facts);

        assert_eq!(
            outcomes.unresolved_imports,
            vec![
                (path("src/app.ts"), "@Domain/Order/types".to_owned()),
                (path("src/app.ts"), "@Domain/Order/id".to_owned()),
            ],
            "the file that wrote it, and what it wrote"
        );
        assert_eq!(
            outcomes.unresolved_imports.len(),
            outcomes.unresolved,
            "a reader shown fewer lines than the count would ask where the rest went"
        );
    }

    /// Two imports of the same unresolvable specifier are two imports. Folding
    /// them into one entry would leave the list disagreeing with the count it
    /// is there to explain.
    #[test]
    fn the_same_unresolved_specifier_twice_is_named_twice() {
        let mut facts = facts(&["@org/never-installed", "@org/never-installed"]);

        let outcomes = resolve_imports(&InMemoryResolver::new(), &mut facts);

        assert_eq!(outcomes.unresolved, 2);
        assert_eq!(outcomes.unresolved_imports.len(), 2);
    }

    /// Resolution is per importer: the same specifier written in two files can
    /// land in two places, and the pass must pass the importer along.
    #[test]
    fn the_importer_is_what_a_relative_specifier_resolves_against() {
        let resolver = InMemoryResolver::new()
            .with_from(
                &path("src/app.ts"),
                "./x",
                Resolved::InRepo(path("src/x.ts")),
            )
            .with_from(
                &path("src/deep/app.ts"),
                "./x",
                Resolved::InRepo(path("src/deep/x.ts")),
            );

        let mut shallow = facts(&["./x"]);
        resolve_imports(&resolver, &mut shallow);
        assert_eq!(
            shallow.imports.first().and_then(|i| i.resolved.as_ref()),
            Some(&path("src/x.ts"))
        );

        let mut deep = FileFacts::unparsed(path("src/deep/app.ts"), ContentHash::of(b"source"));
        deep.imports = facts(&["./x"]).imports;
        resolve_imports(&resolver, &mut deep);
        assert_eq!(
            deep.imports.first().and_then(|i| i.resolved.as_ref()),
            Some(&path("src/deep/x.ts"))
        );
    }
}
