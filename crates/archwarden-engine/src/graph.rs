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
//! It is still O(edges) to build and O(edges) per query, so it is built only
//! when a rule asks — `RuleEngine::needs_graph`, gated the way `reads_files`
//! already gates the cache. dependency-cruiser's performance complaints are
//! this shape of whole-graph traversal, and a repository with no cycle rule
//! should not pay for one.

use std::collections::{BTreeMap, BTreeSet};

use archwarden_core::{facts::FileFacts, path::RepoRelPath};

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
    edges: BTreeMap<RepoRelPath, Vec<RepoRelPath>>,
}

impl ImportGraph {
    /// Builds the graph from the facts of every file the run parsed.
    ///
    /// `include_type_only` decides whether `import type` is an edge. It is not
    /// one at runtime — the import is erased — so a cycle made only of type
    /// imports cannot deadlock anything. It *is* one at compile time, which is
    /// why the choice is the caller's and matches the flag `import-boundary`
    /// already has.
    #[must_use]
    pub fn of<'a>(files: impl Iterator<Item = &'a FileFacts>, include_type_only: bool) -> Self {
        let mut edges: BTreeMap<RepoRelPath, Vec<RepoRelPath>> = BTreeMap::new();

        for facts in files {
            for import in &facts.imports {
                if import.type_only && !include_type_only {
                    continue;
                }
                let Some(resolved) = &import.resolved else {
                    continue;
                };
                // A file importing itself is not a cycle anybody means, and
                // reporting it would be reporting a typo as an architecture
                // fault.
                if resolved == &facts.path {
                    continue;
                }
                edges
                    .entry(facts.path.clone())
                    .or_default()
                    .push(resolved.clone());
            }
        }

        Self { edges }
    }

    /// What `from` imports directly.
    fn out(&self, from: &RepoRelPath) -> &[RepoRelPath] {
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
    #[must_use]
    pub fn cycle_through(&self, from: &RepoRelPath) -> Option<Vec<RepoRelPath>> {
        self.shortest(from, &|reached| reached == from)
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
    #[must_use]
    pub fn reaches(
        &self,
        from: &RepoRelPath,
        target: &dyn Fn(&RepoRelPath) -> bool,
    ) -> Option<Vec<RepoRelPath>> {
        let chain = self.shortest(from, &|reached| target(reached))?;
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
    ) -> Option<Vec<RepoRelPath>> {
        let mut seen: BTreeSet<&RepoRelPath> = BTreeSet::new();
        let mut queue: std::collections::VecDeque<Vec<&RepoRelPath>> =
            std::collections::VecDeque::new();

        for next in self.out(from) {
            queue.push_back(vec![next]);
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
            for next in self.out(here) {
                let mut longer = chain.clone();
                longer.push(next);
                queue.push_back(longer);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::{ImportGraph, MAX_DEPTH};
    use archwarden_core::{
        facts::{FileFacts, ImportFact, Span},
        hash::ContentHash,
        path::RepoRelPath,
    };

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// Files as `(path, [imports])`, every import resolved and not type-only.
    fn graph(files: &[(&str, &[&str])]) -> ImportGraph {
        let facts: Vec<FileFacts> = files
            .iter()
            .map(|(from, imports)| {
                let mut facts = FileFacts::unparsed(path(from), ContentHash::of(b"source"));
                for (offset, to) in imports.iter().enumerate() {
                    let start = u32::try_from(offset).expect("few imports") * 100;
                    facts.imports.push(ImportFact {
                        specifier: (*to).to_owned(),
                        resolved: Some(path(to)),
                        type_only: false,
                        names: Vec::new(),
                        span: Span::new(start, start + 10),
                    });
                }
                facts
            })
            .collect();

        ImportGraph::of(facts.iter(), true)
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
            names(graph.cycle_through(&path("a.ts"))),
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
            names(graph.cycle_through(&path("a.ts"))),
            ["a.ts", "b.ts", "a.ts"]
        );
    }

    #[test]
    fn a_graph_with_no_loop_reports_none() {
        let graph = graph(&[("a.ts", &["b.ts"]), ("b.ts", &["c.ts"])]);

        assert!(graph.cycle_through(&path("a.ts")).is_none());
    }

    /// A file importing itself is a typo, not an architecture fault, and is
    /// not an edge at all.
    #[test]
    fn a_file_importing_itself_is_not_a_loop() {
        let graph = graph(&[("a.ts", &["a.ts"])]);

        assert!(graph.cycle_through(&path("a.ts")).is_none());
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
            names(graph.reaches(&path("apps/api.ts"), &|p| p.as_str() == "packages/db.ts")),
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
                .reaches(&path("apps/api.ts"), &|p| p.as_str() == "packages/db.ts")
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
                .reaches(&path("f0.ts"), &|p| p.as_str() == far)
                .is_none()
        );
    }

    /// A type-only edge cannot cause a runtime loop, so the caller decides
    /// whether it is an edge at all — the same choice `import-boundary`
    /// already offers.
    #[test]
    fn type_only_edges_are_the_callers_choice() {
        let mut facts = FileFacts::unparsed(path("a.ts"), ContentHash::of(b"source"));
        facts.imports.push(ImportFact {
            specifier: "b.ts".to_owned(),
            resolved: Some(path("b.ts")),
            type_only: true,
            names: Vec::new(),
            span: Span::new(0, 10),
        });
        let mut back = FileFacts::unparsed(path("b.ts"), ContentHash::of(b"source"));
        back.imports.push(ImportFact {
            specifier: "a.ts".to_owned(),
            resolved: Some(path("a.ts")),
            type_only: false,
            names: Vec::new(),
            span: Span::new(0, 10),
        });
        let files = [facts, back];

        assert!(
            ImportGraph::of(files.iter(), false)
                .cycle_through(&path("a.ts"))
                .is_none(),
            "erased at runtime, so not a loop when the caller says so"
        );
        assert!(
            ImportGraph::of(files.iter(), true)
                .cycle_through(&path("a.ts"))
                .is_some(),
            "and a loop at compile time when they say the other thing"
        );
    }
}
