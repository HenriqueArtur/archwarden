//! The import graph, for the questions a single file cannot answer.
//!
//! Every rule before this one was lexical, or asked about one file's own
//! imports. *"Does this end up depending on that?"* and *"is there a loop
//! here?"* are the first that need the whole shape, and `resolve.rs` says why
//! there was no index until now: one with no reader would be code nobody could
//! test. There are two readers now — issues #70 and #71 — and they are the
//! same traversal asked twice, which is why the graph is built once and both
//! are asked of it.
//!
//! # What it costs, and when
//!
//! Built from facts the run already has: every `ImportFact` carries where it
//! resolved. No file is read for it and nothing is resolved twice.
//!
//! It is built from [`FileEdges`] rather than from `FileFacts`, and that is
//! the point of the type: the run has to hold every file's edges at once,
//! because the graph needs all of them before any query can run, and holding
//! every file's *facts* to answer "who imports whom" would keep every export,
//! every call and every name in memory for data nothing reads.
//!
//! It is still O(edges) to build and O(edges) per query, so it is built only
//! when a rule asks — `RuleEngine::needs_graph`, gated the way `reads_files`
//! already gates the cache. dependency-cruiser's performance complaints are
//! this shape of whole-graph traversal, and a repository with no cycle rule
//! should not pay for one.

use std::collections::{BTreeMap, BTreeSet};

use crate::{facts::FileFacts, path::RepoRelPath};

/// One import that landed on a file in this repository.
///
/// `type_only` rides on the edge rather than being decided when the graph is
/// built, because the two rules that read the graph each carry their own
/// `include_type_only` and may disagree. Baking the flag in would mean one
/// graph per rule, and a graph costs a resolution pass over the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// Where the import landed.
    pub to: RepoRelPath,
    /// Whether it was written as `import type`, or marked `type` inline.
    pub type_only: bool,
}

/// What one file imports, reduced to what the graph needs.
///
/// The narrow shape is deliberate. See the note on memory at the top of this
/// module: this is what a run keeps for every file at once, so it holds paths
/// and a flag and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdges {
    /// The importing file.
    pub from: RepoRelPath,
    /// Every in-repo import it makes, in source order.
    pub to: Vec<Edge>,
}

impl FileEdges {
    /// Reduces one file's facts to its edges.
    ///
    /// An import that did not resolve is not an edge. A boundary rule cannot
    /// see one either — `Outcomes::unresolved` is where the run says so out
    /// loud — and inventing a destination would have the graph report a chain
    /// through a file nobody wrote.
    #[must_use]
    pub fn of(facts: &FileFacts) -> Self {
        Self {
            from: facts.path.clone(),
            to: facts
                .imports
                .iter()
                .filter_map(|import| {
                    Some(Edge {
                        to: import.resolved.clone()?,
                        type_only: import.type_only,
                    })
                })
                .collect(),
        }
    }
}

/// How far a search will walk before giving up.
///
/// A forty-file cycle is technically correct and useless: nobody reads it and
/// nobody can act on it. The limit is generous enough to catch the loops
/// people actually create and small enough that the answer stays a sentence.
pub const MAX_DEPTH: usize = 12;

/// Who imports whom, as resolved paths.
#[derive(Debug, Default)]
pub struct ImportGraph {
    /// Forward edges only. The reverse index `resolve.rs` declined to build is
    /// still not here: nothing asks "who imports this?", and an index with no
    /// reader is the thing that note refuses.
    edges: BTreeMap<RepoRelPath, Vec<Edge>>,
}

impl ImportGraph {
    /// Builds the graph from the edges of every file the run resolved.
    #[must_use]
    pub fn of(files: impl Iterator<Item = FileEdges>) -> Self {
        let mut edges: BTreeMap<RepoRelPath, Vec<Edge>> = BTreeMap::new();

        for file in files {
            for edge in file.to {
                // A file importing itself is not a cycle anybody means, and
                // reporting it would be reporting a typo as an architecture
                // fault. Filtered here rather than in `FileEdges::of`, so a
                // caller that builds edges by hand cannot reintroduce it.
                if edge.to == file.from {
                    continue;
                }
                edges.entry(file.from.clone()).or_default().push(edge);
            }
        }

        Self { edges }
    }

    /// What `from` imports directly.
    fn out(&self, from: &RepoRelPath) -> &[Edge] {
        self.edges.get(from).map_or(&[], Vec::as_slice)
    }

    /// The shortest loop that comes back to `from`, if there is one.
    ///
    /// Breadth-first, so the answer is the shortest: a two-file loop reported
    /// as itself rather than as whatever nine-file walk happened to be found
    /// first. The shortest is not always the one to fix, and it is always the
    /// one that can be read.
    ///
    /// The chain starts and ends at `from`, so `a -> b -> a` comes back as
    /// three entries. A reader needs both ends to see that it closed.
    ///
    /// `include_type_only` decides whether `import type` is followed. It is
    /// not an edge at runtime — the import is erased — so a loop made only of
    /// type imports cannot deadlock anything. It *is* one at compile time,
    /// which is why the choice is the caller's and matches the flag
    /// `import-boundary` already has.
    #[must_use]
    pub fn cycle_through(
        &self,
        from: &RepoRelPath,
        include_type_only: bool,
    ) -> Option<Vec<RepoRelPath>> {
        self.shortest(from, &|reached| reached == from, include_type_only)
            .map(|mut chain| {
                chain.insert(0, from.clone());
                chain
            })
    }

    /// The shortest chain from `from` to a file `target` accepts.
    ///
    /// The chain is the answer, not the fact. *"`apps/api` reaches
    /// `packages/db`"* is not actionable; *"`apps/api` → `packages/orders` →
    /// `packages/db`"* names the edge to cut. Issue #71.
    ///
    /// Direct imports are excluded: a rule about *transitive* reach is about
    /// what a boundary rule cannot already see, and reporting the direct edge
    /// here would report it twice.
    ///
    /// `include_type_only` is the caller's, for the same reason it is on
    /// [`cycle_through`](Self::cycle_through).
    #[must_use]
    pub fn reaches(
        &self,
        from: &RepoRelPath,
        target: &dyn Fn(&RepoRelPath) -> bool,
        include_type_only: bool,
    ) -> Option<Vec<RepoRelPath>> {
        let chain = self.shortest(from, &|reached| target(reached), include_type_only)?;
        if chain.len() < 2 {
            return None;
        }
        Some(
            std::iter::once(from.clone())
                .chain(chain)
                .collect::<Vec<_>>(),
        )
    }

    /// Breadth-first search, returning the chain of steps taken.
    ///
    /// The start is never reported as reached on step zero, which is what lets
    /// one implementation answer both questions: a cycle is "reach yourself"
    /// and reachability is "reach one of those", and only the predicate
    /// differs.
    fn shortest(
        &self,
        from: &RepoRelPath,
        accepts: &dyn Fn(&RepoRelPath) -> bool,
        include_type_only: bool,
    ) -> Option<Vec<RepoRelPath>> {
        let mut seen: BTreeSet<&RepoRelPath> = BTreeSet::new();
        let mut queue: std::collections::VecDeque<Vec<&RepoRelPath>> =
            std::collections::VecDeque::new();

        let followed = |edge: &Edge| include_type_only || !edge.type_only;

        for next in self.out(from).iter().filter(|edge| followed(edge)) {
            queue.push_back(vec![&next.to]);
        }

        while let Some(chain) = queue.pop_front() {
            let Some(here) = chain.last().copied() else {
                continue;
            };
            if accepts(here) {
                return Some(chain.into_iter().cloned().collect());
            }
            if chain.len() >= MAX_DEPTH || !seen.insert(here) {
                continue;
            }
            for next in self.out(here).iter().filter(|edge| followed(edge)) {
                let mut longer = chain.clone();
                longer.push(&next.to);
                queue.push_back(longer);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::{Edge, FileEdges, ImportGraph, MAX_DEPTH};
    use crate::{
        facts::{FileFacts, ImportFact, Span},
        hash::ContentHash,
        path::RepoRelPath,
    };

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// Files as `(path, [imports])`, every import resolved and not type-only.
    fn graph(files: &[(&str, &[&str])]) -> ImportGraph {
        ImportGraph::of(files.iter().map(|(from, imports)| {
            FileEdges {
                from: path(from),
                to: imports
                    .iter()
                    .map(|to| Edge {
                        to: path(to),
                        type_only: false,
                    })
                    .collect(),
            }
        }))
    }

    fn names(chain: Option<Vec<RepoRelPath>>) -> Vec<String> {
        chain
            .unwrap_or_default()
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect()
    }

    /// The two-file loop, reported with both ends so a reader can see it
    /// closed.
    #[test]
    fn a_loop_of_two_comes_back_naming_both_ends() {
        let graph = graph(&[("a.ts", &["b.ts"]), ("b.ts", &["a.ts"])]);

        assert_eq!(
            names(graph.cycle_through(&path("a.ts"), true)),
            ["a.ts", "b.ts", "a.ts"]
        );
    }

    /// Breadth-first, so a file that sits on a short loop and a long one is
    /// reported with the short one. The shortest is not always the one to fix
    /// and it is always the one somebody can read.
    #[test]
    fn the_shortest_loop_is_the_one_reported() {
        let graph = graph(&[
            ("a.ts", &["b.ts", "c.ts"]),
            ("b.ts", &["a.ts"]),
            ("c.ts", &["d.ts"]),
            ("d.ts", &["a.ts"]),
        ]);

        assert_eq!(
            names(graph.cycle_through(&path("a.ts"), true)),
            ["a.ts", "b.ts", "a.ts"]
        );
    }

    #[test]
    fn a_graph_with_no_loop_reports_none() {
        let graph = graph(&[("a.ts", &["b.ts"]), ("b.ts", &["c.ts"])]);

        assert!(graph.cycle_through(&path("a.ts"), true).is_none());
    }

    /// A file importing itself is a typo, not an architecture fault, and is
    /// not an edge at all.
    #[test]
    fn a_file_importing_itself_is_not_a_loop() {
        let graph = graph(&[("a.ts", &["a.ts"])]);

        assert!(graph.cycle_through(&path("a.ts"), true).is_none());
    }

    /// The chain is the answer. "`apps/api` reaches `packages/db`" is not
    /// actionable; the path through `packages/orders` names the edge to cut.
    #[test]
    fn reaching_something_comes_back_as_the_whole_chain() {
        let graph = graph(&[
            ("apps/api.ts", &["packages/orders.ts"]),
            ("packages/orders.ts", &["packages/db.ts"]),
        ]);

        assert_eq!(
            names(graph.reaches(
                &path("apps/api.ts"),
                &|p| p.as_str() == "packages/db.ts",
                true
            )),
            ["apps/api.ts", "packages/orders.ts", "packages/db.ts"]
        );
    }

    /// A direct import is not transitive reach. `import-boundary` already sees
    /// it, and reporting it here would report one fault twice.
    #[test]
    fn a_direct_import_is_not_reported_as_transitive_reach() {
        let graph = graph(&[("apps/api.ts", &["packages/db.ts"])]);

        assert!(
            graph
                .reaches(
                    &path("apps/api.ts"),
                    &|p| p.as_str() == "packages/db.ts",
                    true
                )
                .is_none()
        );
    }

    /// A chain longer than anybody would read is not walked. Reporting a
    /// forty-file path is technically correct and useless.
    #[test]
    fn a_chain_longer_than_the_limit_is_not_followed() {
        let files: Vec<(String, Vec<String>)> = (0..MAX_DEPTH + 5)
            .map(|i| (format!("f{i}.ts"), vec![format!("f{}.ts", i + 1)]))
            .collect();
        let borrowed: Vec<(&str, Vec<&str>)> = files
            .iter()
            .map(|(f, to)| (f.as_str(), to.iter().map(String::as_str).collect()))
            .collect();
        let as_slices: Vec<(&str, &[&str])> =
            borrowed.iter().map(|(f, to)| (*f, to.as_slice())).collect();

        let graph = graph(&as_slices);
        let far = format!("f{}.ts", MAX_DEPTH + 4);

        assert!(
            graph
                .reaches(&path("f0.ts"), &|p| p.as_str() == far, true)
                .is_none()
        );
    }

    /// A type-only edge cannot cause a runtime loop, so the caller decides
    /// whether it is an edge at all — the same choice `import-boundary`
    /// already offers.
    ///
    /// The choice is the *query's*, not the graph's, and one graph answers two
    /// callers who disagree. It has to be: `import-cycle` and
    /// `import-boundary` each carry their own `include_type_only`, and a graph
    /// that baked the flag in would have a run walk the whole repository twice
    /// to answer two questions about the same edges.
    #[test]
    fn one_graph_answers_callers_who_disagree_about_type_only_edges() {
        let graph = ImportGraph::of(
            [
                FileEdges {
                    from: path("a.ts"),
                    to: vec![Edge {
                        to: path("b.ts"),
                        type_only: true,
                    }],
                },
                FileEdges {
                    from: path("b.ts"),
                    to: vec![Edge {
                        to: path("a.ts"),
                        type_only: false,
                    }],
                },
            ]
            .into_iter(),
        );

        assert!(
            graph.cycle_through(&path("a.ts"), false).is_none(),
            "erased at runtime, so not a loop when the caller says so"
        );
        assert!(
            graph.cycle_through(&path("a.ts"), true).is_some(),
            "and a loop at compile time when they say the other thing"
        );
    }

    /// Excluding type-only edges means *not following them*, not following
    /// them instead.
    ///
    /// The test above cannot tell those apart: it has one loop, so a walk that
    /// followed the wrong edges finds nothing either way and reports `None`
    /// for the right answer by accident. Here `a.ts` sits on two loops, one
    /// made of `import type` and one not, and the caller who excluded type
    /// imports must come back with the *value* loop — naming which edges were
    /// walked rather than only how many.
    #[test]
    fn excluding_type_only_edges_follows_the_value_ones() {
        let graph = ImportGraph::of(
            [
                FileEdges {
                    from: path("a.ts"),
                    to: vec![
                        Edge {
                            to: path("b.ts"),
                            type_only: true,
                        },
                        Edge {
                            to: path("c.ts"),
                            type_only: false,
                        },
                    ],
                },
                FileEdges {
                    from: path("b.ts"),
                    to: vec![Edge {
                        to: path("a.ts"),
                        type_only: true,
                    }],
                },
                FileEdges {
                    from: path("c.ts"),
                    to: vec![Edge {
                        to: path("a.ts"),
                        type_only: false,
                    }],
                },
            ]
            .into_iter(),
        );

        assert_eq!(
            names(graph.cycle_through(&path("a.ts"), false)),
            ["a.ts", "c.ts", "a.ts"],
            "the loop through the `import type` edge is the one being skipped"
        );
    }

    /// The facts a run already has, reduced to what the graph needs.
    ///
    /// An import that did not resolve is not an edge: a boundary rule cannot
    /// see it either, and inventing a destination for it would have the graph
    /// report a chain through a file nobody wrote.
    #[test]
    fn only_imports_that_landed_somewhere_become_edges() {
        let mut facts = FileFacts::unparsed(path("a.ts"), ContentHash::of(b"source"));
        for (specifier, resolved, type_only) in [
            ("./b", Some(path("b.ts")), false),
            ("./types", Some(path("types.ts")), true),
            ("@org/never-installed", None, false),
        ] {
            facts.imports.push(ImportFact {
                specifier: specifier.to_owned(),
                resolved,
                type_only,
                names: Vec::new(),
                span: Span::new(0, 10),
            });
        }

        let edges = FileEdges::of(&facts);

        assert_eq!(edges.from, path("a.ts"));
        assert_eq!(
            edges.to,
            vec![
                Edge {
                    to: path("b.ts"),
                    type_only: false,
                },
                Edge {
                    to: path("types.ts"),
                    type_only: true,
                },
            ],
            "the unresolved specifier is not an edge, and the type-only one is \
             an edge the query decides about"
        );
    }
}
