//! Running the rules over a walked tree.
//!
//! The pipeline's last stage, and deliberately dull: it owns no rule logic. It
//! offers each directory and each file to each engine and collects what comes
//! back, which is what keeps every interesting decision inside a rule where it
//! can be tested on its own.
//!
//! It does touch the filesystem, but only to read back a file some rule wants
//! to look inside. A configuration whose rules are all structural never reads
//! a byte.

use archwarden_cache::store::Cache;
use archwarden_core::{
    compiled::CompiledConfig,
    facts::FileFacts,
    finding::Finding,
    hash::ContentHash,
    level::Level,
    path::{FileClass, RepoRelPath},
    traits::{Exists, FactsNeeded, FileContext, Parser as _},
};
use camino::Utf8Path;

use crate::walk::RepoTree;

/// What a run found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Every finding, worst-first then by path then by rule.
    pub findings: Vec<Finding>,
    /// How many directories were examined.
    pub directories_scanned: usize,
    /// How many files were examined.
    pub files_scanned: usize,
    /// Files a rule wanted to read but could not, with why.
    ///
    /// Also reported rather than dropped, and for the same reason: a file that
    /// did not parse was not checked, and a clean report would be lying about
    /// it.
    pub unreadable_files: Vec<(RepoRelPath, String)>,
    /// How many files were parsed from source.
    pub files_parsed: usize,
    /// Checks that could not be made: one per rule that wanted a *source*
    /// file whose facts were unavailable.
    ///
    /// Counted in *checks* rather than files, because one unreadable file that
    /// three rules wanted is three answers nobody got. `unreadable_files` names
    /// the file; this says how much was not decided because of it. Without the
    /// number, a report over a repository where nothing could be parsed reads
    /// as a repository with nothing wrong — the failure mode `check --file`
    /// already refuses through `skipped` (correction C6), and the full run
    /// used to allow.
    ///
    /// A rule whose scope reaches a file that is not JavaScript or TypeScript
    /// does *not* count here. That is not a check nobody could make; it is a
    /// file the rule was never about, and `RULES.md` already declines to make
    /// anyone declare that a PNG needs no test. Counting it made the number
    /// unreachable for any repository that keeps a `DOC.md` beside its code.
    /// `check --file` still reports it, under `not-source`, because that
    /// command is answering "what happened to *this* file" and "nothing, it is
    /// not source" is a real answer there. Issue #15.
    pub checks_skipped: usize,
    /// Which rule wanted which file, for every skipped check.
    ///
    /// `checks_skipped` is the number `AGENTS.md` calls "the number to watch",
    /// and a number nobody can act on is not one. Sorted, so the report stays
    /// byte-identical between runs.
    pub skipped_checks: Vec<(String, RepoRelPath)>,
    /// How many files had their facts reused from the cache.
    ///
    /// Reported so a user can see the cache working -- and notice when it is
    /// not, which is otherwise invisible until someone times two runs.
    pub facts_reused: usize,
    /// Findings an `archwarden-allow` marker took out of the list, with the
    /// reason its author gave.
    ///
    /// **Never silently dropped.** A suppressed finding is not an absent
    /// finding: it is here, with its reason, in every format, and a run with
    /// forty of them must not look like a clean one at a glance. That is the
    /// whole of issue #72 — `// eslint-disable-next-line` with no explanation
    /// is how debt becomes invisible, and a suppression that hides itself is
    /// worse than the violation it hides.
    pub suppressed: Vec<Suppressed>,
    /// Where the imports went, by kind.
    ///
    /// All zero when no rule needed resolution. The `unresolved` count is the
    /// one that matters to a reader: a boundary rule cannot see an import it
    /// could not place, so a clean report over a repository whose dependencies
    /// are not installed means less than it looks like. Each of those is named
    /// in `unresolved_imports`, sorted, because the count alone left a reader
    /// nothing to open (issue #18).
    pub imports: crate::resolve::Outcomes,
}

impl Report {
    /// How many findings are at error level.
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.level.fails_build())
            .count()
    }

    /// How many findings are at warning level.
    #[must_use]
    pub fn warning_count(&self) -> usize {
        self.findings.len() - self.error_count()
    }

    /// Whether this run should fail a build.
    #[must_use]
    pub fn fails_build(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.level.fails_build())
    }
}
/// A finding somebody allowed on purpose, and why.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Suppressed {
    /// What would have been reported.
    pub finding: Finding,
    /// The author's words, from the marker. Never empty — a marker with no
    /// reason is not a marker. See
    /// [`AllowanceFact`](archwarden_core::facts::AllowanceFact).
    pub reason: String,
}

/// Everything one run needs.
///
/// A struct rather than four parameters, because the cache is optional and a
/// bare `Option<&mut Cache>` at a call site says nothing about what it is for.
pub struct Run<'a> {
    /// Where the tree was walked from. Needed only to read files back for the
    /// rules that look inside one.
    pub root: &'a Utf8Path,
    /// The compiled configuration.
    pub config: &'a CompiledConfig,
    /// The walked repository.
    pub tree: &'a RepoTree,
    /// The cache, when there is one. A run without it is correct and slower.
    pub cache: Option<&'a mut Cache>,
    /// The day this run answers for.
    ///
    /// Threaded to every rule through `FileContext::as_of` rather than read
    /// from a clock, so two machines given the same date give the same answer.
    /// Only `metadata.deadline` asks. Issue #117.
    pub as_of: archwarden_core::date::Date,
}

/// Whether any rule in this configuration has to look inside a file.
///
/// The caller uses this to decide whether opening a cache is worth it at all:
/// a purely structural configuration reads no bytes, so a cache would only
/// leave a file behind for someone to wonder about.
#[must_use]
pub fn reads_files(config: &CompiledConfig) -> bool {
    archwarden_rules::engines_for(config)
        .iter()
        .any(|engine| engine.needs_facts() != FactsNeeded::Nothing)
}

/// Whether any rule in this configuration asks where an import lands.
///
/// Resolution is a second cost on top of parsing: it probes the filesystem for
/// every specifier in every file the rule applies to. A run with no boundary
/// rule should not pay it.
#[must_use]
pub fn resolves_imports(config: &CompiledConfig) -> bool {
    archwarden_rules::engines_for(config)
        .iter()
        .any(|engine| engine.needs_resolution())
}

/// Whether any rule in this configuration reads the whole import graph.
///
/// The same shape as [`resolves_imports`] and a different question, and the
/// difference is what it costs. A boundary rule wants every specifier of *its
/// own* file placed, and pays once per file it covers. A cycle rule wants the
/// edges of every file at once — including files no scope reaches, because a
/// loop that leaves the scope and comes back is still a loop. So this one is
/// paid once per run, over the whole repository, whatever any scope says.
///
/// Measured on the 10,000-file benchmark, resolution is roughly three quarters
/// of a warm run. A configuration that answers `true` here is therefore about
/// four times the cost of one that does not, and one that answers `false` pays
/// nothing at all.
#[must_use]
pub fn needs_graph(config: &CompiledConfig) -> bool {
    archwarden_rules::engines_for(config)
        .iter()
        .any(|engine| engine.needs_graph())
}

/// The rule id an ungoverned file reports under.
///
/// A reserved name rather than `Option<RuleId>` on the finding: every other
/// finding names the rule that produced it, and `baseline` keys on that name,
/// so a finding with no rule would need both of those to grow a case for one
/// value. `config doctor` refuses a rule that takes this name, so the two can
/// never be confused in a baseline.
fn governance_id() -> archwarden_core::ids::RuleId {
    archwarden_core::ids::RuleId::new(archwarden_core::ids::GOVERNANCE_RULE_ID)
        .unwrap_or_else(|_| unreachable!("the reserved id is a valid rule id"))
}

/// A file a graph rule wanted, held until there is a graph to answer from.
///
/// Deliberately not every file: only the ones a rule with
/// [`needs_graph`](archwarden_core::traits::RuleEngine::needs_graph) applies
/// to. Every *other* file contributes its
/// [`FileEdges`](archwarden_core::graph::FileEdges) and nothing else — paths
/// and a flag, rather than the exports, calls and names that answering "who
/// imports whom" has no use for.
struct Deferred {
    path: RepoRelPath,
    facts: Option<FileFacts>,
    /// Which entry of the run's sibling lists belongs to this file, so a
    /// directory's names are cloned once rather than once per file in it.
    siblings: usize,
}

/// What one file could offer one rule.
///
/// A struct rather than four positional bools, which clippy refuses and is
/// right to: `looked_at(needed, true, false, true, false)` is a call nobody can
/// read, and this predicate's whole job is to be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WasRead {
    /// Nothing: the file did not parse, or no front-end reads it.
    Nothing,
    /// Imports, exports and calls are in hand.
    Code,
    /// Frontmatter is in hand.
    Document,
}

#[derive(Debug, Clone, Copy)]
struct Available {
    /// What the file yielded. An enum rather than two flags, because a file is
    /// of one class: the pair `(facts, docs)` had a fourth state that cannot
    /// happen and a reader had to work out that it could not.
    read: WasRead,
    /// The rule wants specifiers placed, not merely read.
    wants_edges: bool,
    /// A resolver in this build can place this language's specifiers.
    edges_can_be_placed: bool,
}

/// Whether a rule got the answer it came for, or is a check nobody could make.
///
/// A predicate rather than four lines inside the run loop, because the
/// interesting case cannot be reached from a fixture today and would otherwise
/// go unasserted until it could. Every readable language in this build is also
/// resolvable, so `needs_resolution && !resolvable` is unreachable end to end —
/// and it is the arm decision 19 is about, which is exactly why it is worth
/// pinning here before the language that reaches it arrives.
///
/// Reading is not enough where a rule asks for edges. A language whose parser
/// landed before its resolver yields facts with every `ImportFact::resolved` at
/// `None`, so an `import-boundary` rule over one of its files sees no edges,
/// reports nothing, and looks exactly like a file that crosses none. Decision
/// 19 requires a loud refusal instead, and a check counted as skipped is how
/// this stage refuses.
fn looked_at(needed: FactsNeeded, at: Available) -> bool {
    match needed {
        FactsNeeded::Nothing => true,
        FactsNeeded::Code => {
            at.read == WasRead::Code && (!at.wants_edges || at.edges_can_be_placed)
        }
        FactsNeeded::Document => at.read == WasRead::Document,
        // `FactsNeeded` is non_exhaustive; a kind added later has no front-end
        // here yet, and "did not look" is honest for it.
        _ => false,
    }
}

/// Runs every rule against the walked tree.
///
/// A configuration whose rules are all structural never reads a byte, cache or
/// no cache.
#[allow(
    clippy::too_many_lines,
    reason = "the run loop is one sequence: walk, offer directories, read what \
              a rule looks inside, offer files, count what nobody could \
              decide. Splitting it would put the counters behind a signature \
              and hide the one property that matters -- that every branch \
              which fails to produce facts also files a reason"
)]
#[must_use]
pub fn check(run: Run<'_>) -> Report {
    let Run {
        root,
        config,
        tree,
        mut cache,
        as_of,
    } = run;
    let engines = archwarden_rules::engines_for(config);

    let mut findings = Vec::new();
    let mut unreadable_files = Vec::new();
    let mut checks_skipped = 0;
    let mut skipped_checks: Vec<(String, RepoRelPath)> = Vec::new();
    let mut files_scanned = 0;
    let mut files_parsed = 0;
    let mut facts_reused = 0;
    let mut imports = crate::resolve::Outcomes::default();

    // Decided once, before anything is walked. `true` changes what the loop
    // below is allowed to skip: a graph needs the edges of every file, so the
    // per-file gating that keeps a scoped configuration off the disk has to be
    // suspended for this run. See `needs_graph`.
    let building_graph = engines.iter().any(|engine| engine.needs_graph());
    let mut edges: Vec<archwarden_core::graph::FileEdges> = Vec::new();
    let mut deferred: Vec<Deferred> = Vec::new();
    // Kept per file, and only for files that carry one, so a repository with
    // no suppressions holds nothing. Issue #72.
    let mut allowances: std::collections::BTreeMap<
        RepoRelPath,
        Vec<archwarden_core::facts::AllowanceFact>,
    > = std::collections::BTreeMap::new();
    let mut sibling_lists: Vec<Vec<String>> = Vec::new();

    // Built once per run, not once per file: `oxc_resolver` caches
    // `tsconfig` and `package.json` reads internally, and a fresh resolver
    // per file would throw that away thousands of times.
    //
    // `building_graph` forces one even if no engine declared
    // `needs_resolution`, because an edge is a *resolved* import and a graph
    // built without a resolver would be empty -- which a cycle rule reports as
    // "no cycles".
    // Paired by position, which is what `engines_for` promises and what
    // `describe` already relies on. A rule's import filter lives on the rule
    // and not on the engine, deliberately: it narrows a *population*, and no
    // rule kind should have to know that the axis exists. Decision 25.
    let rules: Vec<&archwarden_core::compiled::CompiledRule> = config.rules().collect();
    let narrowed_by_imports = rules.iter().any(|rule| rule.imports.is_some());

    let resolver = (building_graph
        || narrowed_by_imports
        || engines.iter().any(|engine| engine.needs_resolution()))
    .then(|| archwarden_resolver::imports::ImportResolver::new(root));

    for (path, directory) in tree.directories() {
        let file_names = directory.file_names();
        files_scanned += file_names.len();

        // Which rules a file in *this* directory put in the population. A
        // directory rule narrowed by imports asks "does something in here talk
        // to X?", and that cannot be answered before the files are read --
        // which is why the directory checks now run after the loop below
        // rather than before it. Findings are sorted afterwards, so nothing a
        // reader sees moved.
        let mut matched_here: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();

        // Which import-narrowed rules could be about *this* directory. A
        // directory rule's `applies_to` answers `false` for every file, by
        // design -- its findings are about the directory -- so it never appears
        // in a file's `wanted_by` and its files would never be read. This is
        // how they get read: the rule's own scope says which directories it is
        // about, and the files inside decide the rest.
        let narrowing_here: Vec<usize> = rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.imports.is_some() && rule.scope.matches_dir(path.as_path()))
            .map(|(index, _)| index)
            .collect();

        // Cloned at most once per directory, and only when a graph rule
        // actually holds a file in it.
        let mut siblings_index: Option<usize> = None;

        for file in &directory.files {
            let wanted_by: Vec<_> = engines
                .iter()
                .enumerate()
                .filter(|(_, engine)| engine.applies_to(&file.path))
                .collect();
            // A file nothing governs is normally not opened at all. While a
            // graph is being built it is opened anyway, if it is source: its
            // imports are edges, and a loop is made of edges from files whose
            // own scope has nothing to do with the rule that reports it.
            let feeds_graph = building_graph && reads_as_code(file.class, config.languages());
            // `narrowing_here` is the third reason to open a file nothing
            // appears to govern. A directory rule's `applies_to` answers
            // `false` for every file by design, so `wanted_by` is empty for one
            // — and its files are exactly what decides whether the directory is
            // in the population at all. Decision 25.
            if wanted_by.is_empty() && !feeds_graph && narrowing_here.is_empty() {
                continue;
            }

            // Read the file only if a rule that applies to it actually looks
            // inside. Deciding this per file rather than per run is what keeps
            // a mostly-structural configuration off the disk.
            // A document is read on the same terms: only when a rule that
            // applies to it looks inside one.
            let docs = if file.class == FileClass::Document
                && wanted_by
                    .iter()
                    .any(|(_, engine)| engine.needs_facts() == FactsNeeded::Document)
            {
                read_docs(
                    root,
                    &file.path,
                    cache.as_deref_mut(),
                    &mut Counters {
                        parsed: &mut files_parsed,
                        reused: &mut facts_reused,
                        unreadable: &mut unreadable_files,
                    },
                )
            } else {
                None
            };

            let mut facts = if reads_as_code(file.class, config.languages())
                && (feeds_graph
                    || !narrowing_here.is_empty()
                    || wanted_by.iter().any(|(index, engine)| {
                        engine.needs_facts() == FactsNeeded::Code
                                // A rule narrowed by imports has to read the
                                // file to find out whether it is about it.
                                || rules.get(*index).is_some_and(|rule| rule.imports.is_some())
                    })) {
                match facts_for(root, &file.path, cache.as_deref_mut()) {
                    Ok((facts, Source::Cache)) => {
                        facts_reused += 1;
                        Some(facts)
                    }
                    Ok((facts, Source::Parsed)) => {
                        files_parsed += 1;
                        Some(facts)
                    }
                    Err(message) => {
                        unreadable_files.push((file.path.clone(), message));
                        None
                    }
                }
            } else {
                None
            };

            // After the cache, never before: what is stored is the parser's
            // output, keyed by content alone. Resolution depends on files no
            // content hash covers -- `tsconfig`, lockfiles -- so caching a
            // resolved fact would need the epoch in the key and would serve
            // stale paths the day someone edits an alias.
            //
            // Asked of the rules that apply to *this* file, not of the run. A
            // global "some rule needs resolution" resolved every file that had
            // facts for any reason -- so a `naming` rule over `apps/**` paid
            // for resolving all of it because a boundary rule somewhere else
            // wanted resolution for one file.
            //
            // `feeds_graph` is the one case that overrides the per-file
            // question, and for the same reason it overrides the parse: an
            // unresolved import is not an edge, so a file whose imports were
            // never placed is a hole in the graph rather than a file the rule
            // was not about.
            if let (Some(resolver), Some(facts)) = (resolver.as_ref(), facts.as_mut())
                && (feeds_graph
                    || !narrowing_here.is_empty()
                    || wanted_by.iter().any(|(index, engine)| {
                        engine.needs_resolution()
                            || rules.get(*index).is_some_and(|rule| rule.imports.is_some())
                    }))
            {
                imports.absorb(crate::resolve::resolve_imports(resolver, facts));
            }

            if let Some(facts) = facts.as_ref()
                && !facts.allowances.is_empty()
            {
                allowances.insert(file.path.clone(), facts.allowances.clone());
            }

            // After resolution, never before: an edge is where an import
            // landed, and before this point nothing has landed anywhere.
            if let Some(facts) = facts.as_ref()
                && feeds_graph
            {
                edges.push(archwarden_core::graph::FileEdges::of(facts));
            }

            // Whether any rule that reads the graph wanted this file, so it can
            // be asked again once there is one.
            let mut held = false;

            // A directory rule asks whether *anything in here* talks to it, and
            // this is where "anything" is counted. Separate from the loop below
            // because that one is about rules that apply to the file, and a
            // directory rule never does.
            if let Some(facts) = facts.as_ref() {
                for index in &narrowing_here {
                    if rules
                        .get(*index)
                        .and_then(|rule| rule.imports.as_ref())
                        .is_some_and(|filter| filter.matches(facts))
                    {
                        matched_here.insert(*index);
                    }
                }
            }

            for (index, engine) in wanted_by {
                // The second axis. A rule narrowed by imports is about this
                // file only if the file's imports say so — and an import that
                // did not resolve cannot say, which is reported rather than
                // read as "no". Decision 25.
                if let Some(rule) = rules.get(index)
                    && let Some(filter) = rule.imports.as_ref()
                {
                    // Out of the population when the imports do not say so,
                    // and when there are no imports to read at all: a rule that
                    // narrows by import cannot decide about a file nobody
                    // could open. An unplaceable specifier is why that might be
                    // wrong, and the run already reports those by file and
                    // specifier.
                    let Some(facts) = facts.as_ref().filter(|facts| filter.matches(facts)) else {
                        continue;
                    };
                    let _ = facts;
                    matched_here.insert(index);
                }

                // A rule that reads inside a file, handed no facts, cannot
                // decide anything -- it returns nothing, which is
                // indistinguishable from deciding the file is fine. Counted so
                // the report can say the difference out loud.
                //
                // `is_source` is what makes the number mean something. A
                // boundary rule whose scope catches a `DOC.md` has no facts for
                // it either, but that is not an answer anybody lost: the rule
                // was never about that file, and archwarden classified it as
                // such before deciding to skip. Counting those pinned the
                // number at one per rule per documented layer, permanently and
                // with nothing to fix -- and `AGENTS.md` tells an agent a run
                // with skips must not be reported as clean, so a count that is
                // forever non-zero and forever benign teaches it to stop
                // reading the number, which is the state that instruction
                // exists to prevent. Issue #15.
                //
                // The skip a file with no parser deserves is a different
                // reason, not this one, and it is issue #13's to add.
                // The pair decides, not the class alone: a boundary rule
                // pointed at a `.md` wanted code facts from a file that could
                // never have them and lost nothing, while the same rule pointed
                // at a `.py` lost everything and used to say so nowhere.
                // Whether the rule got what it asked for. A rule that asks for
                // nothing always did; `yields` then answers `false` for it
                // anyway, and the two agreeing is what keeps a structural rule
                // out of the count.
                let needed = engine.needs_facts();
                let looked = looked_at(
                    needed,
                    Available {
                        read: if facts.is_some() {
                            WasRead::Code
                        } else if docs.is_some() {
                            WasRead::Document
                        } else {
                            WasRead::Nothing
                        },
                        wants_edges: engine.needs_resolution(),
                        edges_can_be_placed: FileClass::imports_can_be_resolved(&file.name),
                    },
                );
                if !looked && file.class.yields(needed) {
                    checks_skipped += 1;
                    skipped_checks.push((engine.id().to_string(), file.path.clone()));
                }
                // A rule that reads the graph cannot be answered yet: the
                // graph is not built until every file has been seen. Held back
                // rather than offered `graph: None`, because a cycle rule with
                // no graph reports nothing and nothing is what a repository
                // with no cycles reports. The accounting above still happens
                // here, where the file's class and facts are in hand.
                if engine.needs_graph() {
                    held = true;
                    continue;
                }
                findings.extend(engine.check_file(FileContext {
                    path: &file.path,
                    facts: facts.as_ref(),
                    docs: docs.as_ref(),
                    siblings: &file_names,
                    // The walk already knows the whole repository, so a rule
                    // asking about a path outside this directory costs a map
                    // lookup and no disk.
                    exists: Exists::new(&|candidate| tree.contains_file(candidate)),
                    graph: None,
                    as_of,
                }));
            }

            if held {
                let index = if let Some(index) = siblings_index {
                    index
                } else {
                    // Read before the push, so it *is* the index of what is
                    // about to be pushed. Taking the length afterwards and
                    // subtracting one is the same number and one arithmetic
                    // mistake away from a rule silently seeing another
                    // directory's siblings.
                    let index = sibling_lists.len();
                    sibling_lists.push(file_names.clone());
                    siblings_index = Some(index);
                    index
                };
                deferred.push(Deferred {
                    path: file.path.clone(),
                    facts,
                    siblings: index,
                });
            }
        }

        // After the files, not before: a directory rule narrowed by imports
        // asks whether anything in here talks to X, and nothing knew until the
        // loop above read them. A rule that does not narrow is unaffected and
        // runs exactly as it did.
        for (index, engine) in engines.iter().enumerate() {
            if let Some(rule) = rules.get(index)
                && rule.imports.is_some()
                && !matched_here.contains(&index)
            {
                continue;
            }

            findings.extend(
                engine.check_directory(archwarden_core::traits::DirectoryContext {
                    path,
                    subdirectories: &directory.subdirectories,
                    files: &file_names,
                }),
            );
        }
    }

    // A file no rule governs, when the configuration says the architecture is
    // closed. `CONFIG.md` calls a rule enforcing nothing the worst failure a
    // linter has; this is that sentence one level up, and until it existed a
    // clean report over an unwatched half of a tree was indistinguishable from
    // one over a tree that satisfied everything. Issue #60.
    //
    // Per file rather than per directory, deliberately. `baseline` accepts a
    // finding by rule *and path*, so a grouped finding would keep matching as
    // new ungoverned files appeared under it -- an escape hatch that silently
    // swallows tomorrow's debt, which is the shape this project keeps refusing.
    // `config coverage` is where the grouped view lives, and it is a report
    // rather than a record.
    if let Some(level) = config.governance() {
        findings.extend(
            tree.files()
                .filter(|file| !engines.iter().any(|engine| engine.applies_to(&file.path)))
                .map(|file| Finding {
                    rule_id: governance_id(),
                    module_id: None,
                    level,
                    path: file.path.clone(),
                    span: None,
                    observed: archwarden_core::finding::Observed::Ungoverned,
                    expected: archwarden_core::finding::Expectation::GovernedBySomeRule,
                }),
        );
    }

    // The second half of the run, and the only part that can see more than one
    // file at a time. Everything above produced edges; this turns them into a
    // graph and asks the rules that were waiting for one.
    if building_graph {
        let graph = archwarden_core::graph::ImportGraph::of(edges.into_iter());

        for held in &deferred {
            for engine in engines
                .iter()
                .filter(|engine| engine.needs_graph() && engine.applies_to(&held.path))
            {
                findings.extend(engine.check_file(FileContext {
                    path: &held.path,
                    facts: held.facts.as_ref(),
                    // No graph rule reads a document today. One that did would
                    // need its `DocFacts` held here the way its `FileFacts`
                    // are, and `needs_facts` is where it would say so.
                    docs: None,
                    siblings: sibling_lists.get(held.siblings).map_or(&[], Vec::as_slice),
                    exists: Exists::new(&|candidate| tree.contains_file(candidate)),
                    graph: Some(&graph),
                    as_of,
                }));
            }
        }
    }

    // Suppression, last: a marker takes a finding out of the list and puts it
    // in `suppressed` with the reason, rather than making it disappear. Issue
    // #72, and the constraint that makes the feature safe to have at all.
    //
    // Only findings with a span can be reached. A marker governs the line
    // after it, and `structure` reporting a folder that should not exist has
    // no line to sit above -- that limit is stated in `docs/CONFIG.md` rather
    // than left to be discovered.
    let mut suppressed: Vec<Suppressed> = Vec::new();
    findings.retain(|finding| {
        let Some(span) = finding.span else {
            return true;
        };
        let Some(marker) = allowances.get(&finding.path).and_then(|markers| {
            markers
                .iter()
                .find(|marker| marker.covers(span.start, finding.rule_id.as_str()))
        }) else {
            return true;
        };
        suppressed.push(Suppressed {
            finding: finding.clone(),
            reason: marker.reason.clone(),
        });
        false
    });

    // Determinism is a design goal: the same inputs must produce byte-identical
    // output, or every snapshot test and CI diff becomes noise. The blind spots
    // arrive in whatever order the walk reached the files, which is not one a
    // reader can predict; sorted, they group by file on their own.
    findings.sort();
    unreadable_files.sort();
    imports.unresolved_imports.sort();

    Report {
        findings,
        directories_scanned: tree.directory_count(),
        files_scanned,
        unreadable_files,
        checks_skipped,
        skipped_checks: {
            skipped_checks.sort();
            skipped_checks
        },
        files_parsed,
        facts_reused,
        imports,
        suppressed: {
            suppressed.sort();
            suppressed
        },
    }
}

/// Where a file's facts came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// Read and parsed on this run.
    Parsed,
    /// Reused from the cache, unchanged since it was stored.
    Cache,
}

/// Facts for one file, from the cache when its bytes have not changed.
///
/// The file is read either way -- that is how the content hash is computed --
/// so what the cache saves is the *parse*, which is the expensive half. Not
/// reading at all would need a stat-based pre-filter, which trades correctness
/// for speed in a way a gate should not.
fn facts_for(
    root: &Utf8Path,
    path: &RepoRelPath,
    cache: Option<&mut Cache>,
) -> Result<(FileFacts, Source), String> {
    let source =
        std::fs::read_to_string(root.join(path.as_path())).map_err(|error| error.to_string())?;
    let content = ContentHash::of(source.as_bytes());

    if let Some(cache) = cache {
        // The path is passed in, not read out: the entry is keyed by content
        // alone, so the file it was stamped from is not necessarily this one.
        // Issue #20.
        if let Some(facts) = cache.facts(content, path) {
            return Ok((facts, Source::Cache));
        }

        let facts = parse(path, &source, content)?;
        cache.put_facts(content, &facts);
        return Ok((facts, Source::Parsed));
    }

    Ok((parse(path, &source, content)?, Source::Parsed))
}

/// Where a read lands, so one call can report all three outcomes.
struct Counters<'a> {
    parsed: &'a mut usize,
    reused: &'a mut usize,
    unreadable: &'a mut Vec<(RepoRelPath, String)>,
}

/// Reads a document and files the outcome, or `None` when it could not be read.
fn read_docs(
    root: &Utf8Path,
    path: &RepoRelPath,
    cache: Option<&mut Cache>,
    counters: &mut Counters<'_>,
) -> Option<archwarden_core::docs::DocFacts> {
    match docs_for(root, path, cache) {
        Ok((docs, Source::Cache)) => {
            *counters.reused += 1;
            Some(docs)
        }
        Ok((docs, Source::Parsed)) => {
            *counters.parsed += 1;
            Some(docs)
        }
        Err(reason) => {
            counters.unreadable.push((path.clone(), reason));
            None
        }
    }
}

/// Whether this run may read a file of this class as code.
///
/// JS/TS always. Astro only when the configuration asked for it -- and when it
/// did not, the file produces no facts, which `yields` then turns into a
/// counted, named skip rather than a silent pass. Issue #13.
fn reads_as_code(class: FileClass, languages: archwarden_core::compiled::Languages) -> bool {
    match class {
        FileClass::Source => true,
        FileClass::Embedded => languages.astro,
        _ => false,
    }
}

/// Document facts for one file, from the cache when they are there.
///
/// The same shape as [`facts_for`], deliberately: the second front-end earns no
/// exception. Reading a document is infallible, so the `Result` a code parse
/// needs is absent here — a document that disappoints a rule does so through
/// its facts, not through an error.
fn docs_for(
    root: &Utf8Path,
    path: &RepoRelPath,
    cache: Option<&mut Cache>,
) -> Result<(archwarden_core::docs::DocFacts, Source), String> {
    let source =
        std::fs::read_to_string(root.join(path.as_path())).map_err(|error| error.to_string())?;
    let content = ContentHash::of(source.as_bytes());

    if let Some(cache) = cache {
        // Keyed by content alone, so the path is supplied rather than read
        // back. Same reasoning as `facts_for`; same bug if it is not. Issue #20.
        if let Some(docs) = cache.docs(content, path) {
            return Ok((docs, Source::Cache));
        }

        let docs = archwarden_parser::document::read(path, &source, content);
        cache.put_docs(content, &docs);
        return Ok((docs, Source::Parsed));
    }

    Ok((
        archwarden_parser::document::read(path, &source, content),
        Source::Parsed,
    ))
}

/// Facts for one file, read and parsed now.
///
/// The uncached path, exposed for `check --file`: a pre-write hook checks one
/// file, and opening a cache to read one entry costs more than the parse it
/// would save.
///
/// # Errors
/// A message naming what went wrong, when the file cannot be read or parsed.
pub fn facts_of(root: &Utf8Path, path: &RepoRelPath) -> Result<FileFacts, String> {
    let source =
        std::fs::read_to_string(root.join(path.as_path())).map_err(|error| error.to_string())?;
    let content = ContentHash::of(source.as_bytes());
    parse(path, &source, content)
}

/// The facts a source text would yield at `path`, without reading the disk.
///
/// What a pre-write hook needs: the write it is asked about has not landed, so
/// the bytes that matter are the ones in the event rather than the ones in the
/// working tree. See [`crate::single::check_write`].
///
/// # Errors
/// The parser's message, when the text does not parse.
pub fn facts_from(path: &RepoRelPath, source: &str) -> Result<FileFacts, String> {
    parse(path, source, ContentHash::of(source.as_bytes()))
}

/// Reads one file as code, through whichever front-end its class names.
///
/// The dispatch is by class rather than by extension: `FileClass` already
/// answers "what kind of file is this" from the name alone, and asking a second
/// time here is where the two would drift.
fn parse(path: &RepoRelPath, source: &str, content: ContentHash) -> Result<FileFacts, String> {
    let class = path.file_name().map_or(FileClass::Other, FileClass::of);

    match class {
        FileClass::Embedded => archwarden_parser::astro::parse(path, source, content),
        _ => archwarden_parser::oxc::OxcParser.parse(path, source, content),
    }
    .map_err(|error| error.to_string())
}

/// The level a report should be summarised at, for a caller choosing an exit
/// code without walking the findings itself.
#[must_use]
pub fn worst_level(report: &Report) -> Option<Level> {
    report.findings.iter().map(|finding| finding.level).max()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rule that reads nothing is always answered.
    #[test]
    fn a_rule_that_needs_no_facts_never_counts_as_skipped() {
        for read in [WasRead::Nothing, WasRead::Code, WasRead::Document] {
            for wants_edges in [true, false] {
                assert!(looked_at(
                    FactsNeeded::Nothing,
                    Available {
                        read,
                        wants_edges,
                        edges_can_be_placed: false
                    }
                ));
            }
        }
    }

    /// Code facts are needed, and a file that could not be parsed is a check
    /// nobody could make.
    #[test]
    fn a_code_rule_needs_facts_and_a_document_rule_needs_its_own() {
        assert!(looked_at(
            FactsNeeded::Code,
            Available {
                read: WasRead::Code,
                wants_edges: false,
                edges_can_be_placed: true
            }
        ));
        assert!(!looked_at(
            FactsNeeded::Code,
            Available {
                read: WasRead::Document,
                wants_edges: false,
                edges_can_be_placed: true
            }
        ));

        assert!(looked_at(
            FactsNeeded::Document,
            Available {
                read: WasRead::Document,
                wants_edges: false,
                edges_can_be_placed: true
            }
        ));
        assert!(
            !looked_at(
                FactsNeeded::Document,
                Available {
                    read: WasRead::Code,
                    wants_edges: false,
                    edges_can_be_placed: true
                }
            ),
            "code facts do not answer a document rule"
        );
    }

    /// The arm decision 19 is about, and the one no fixture can reach today.
    ///
    /// A rule asking for edges over a language whose parser landed before its
    /// resolver has facts and no resolved specifiers. It reports nothing, and
    /// nothing is what a file crossing no boundary reports -- so the check is
    /// counted rather than passed.
    ///
    /// All four combinations, because the condition is a disjunction: a test
    /// naming only the failing one passes while the rule stops being asked at
    /// all, and one naming only the passing ones passes while the refusal
    /// never fires.
    #[test]
    fn a_rule_wanting_edges_is_skipped_where_no_resolver_can_place_them() {
        assert!(
            !looked_at(
                FactsNeeded::Code,
                Available {
                    read: WasRead::Code,
                    wants_edges: true,
                    edges_can_be_placed: false
                }
            ),
            "wants edges, language has no resolver: the check was not made"
        );
        assert!(
            looked_at(
                FactsNeeded::Code,
                Available {
                    read: WasRead::Code,
                    wants_edges: true,
                    edges_can_be_placed: true
                }
            ),
            "wants edges and they can be placed"
        );
        assert!(
            looked_at(
                FactsNeeded::Code,
                Available {
                    read: WasRead::Code,
                    wants_edges: false,
                    edges_can_be_placed: false
                }
            ),
            "wants no edges, so an absent resolver costs it nothing"
        );
        assert!(
            looked_at(
                FactsNeeded::Code,
                Available {
                    read: WasRead::Code,
                    wants_edges: false,
                    edges_can_be_placed: true
                }
            ),
            "wants no edges and could have had them"
        );
    }

    /// Facts are still required first: an unresolvable language whose file did
    /// not parse is one skip, not an argument about resolvers.
    #[test]
    fn a_file_that_did_not_parse_is_skipped_whatever_the_rule_wanted() {
        for needs_resolution in [true, false] {
            for imports_resolvable in [true, false] {
                assert!(!looked_at(
                    FactsNeeded::Code,
                    Available {
                        read: WasRead::Nothing,
                        wants_edges: needs_resolution,
                        edges_can_be_placed: imports_resolvable
                    }
                ));
            }
        }
    }
    use archwarden_core::{
        compiled::{CompiledRule, CompiledRuleKind, SkipDirs},
        glob::PathSet,
        hash::ContentHash,
        ids::{ModuleId, RuleId},
        scope::Scope,
    };
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

    fn rule(
        id: &str,
        module: Option<&str>,
        scope: &[&str],
        kind: CompiledRuleKind,
    ) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: module.map(|m| ModuleId::new(m).expect("valid module")),
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(scope).expect("valid scope"),
            kind,
        }
    }

    fn config(rules: Vec<CompiledRule>) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::default(),
            SkipDirs {
                prefixes: vec!["_".to_owned()],
                globs: PathSet::default(),
                scope: archwarden_core::compiled::SkipScope::Structure,
            },
            ContentHash::of(b""),
        )
    }

    fn structure(allowed: &[&str]) -> CompiledRuleKind {
        CompiledRuleKind::Structure {
            allowed_subfolders: Some(allowed.iter().map(|s| (*s).to_owned()).collect()),
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
            filename_patterns: Vec::new(),
        }
    }

    fn boundary(forbid: &[&str], require: &[&str], except: &[&str]) -> CompiledRuleKind {
        let set = |patterns: &[&str]| {
            PathSet::compile(patterns.iter().map(|p| (*p).to_owned())).expect("valid globs")
        };
        CompiledRuleKind::ImportBoundary {
            forbid: set(forbid),
            groups: Vec::new(),
            allow: None,
            allow_packages: None,
            require: set(require),
            forbid_packages: Vec::new(),
            forbid_reaching: PathSet::default(),
            except: set(except),
            except_from: PathSet::default(),
            include_type_only: true,
        }
    }

    /// A boundary rule that forbids *reaching* rather than importing.
    fn reaching(forbid_reaching: &[&str]) -> CompiledRuleKind {
        let mut kind = boundary(&[], &[], &[]);
        let CompiledRuleKind::ImportBoundary {
            forbid_reaching: slot,
            ..
        } = &mut kind
        else {
            panic!("built as an import-boundary rule");
        };
        *slot =
            PathSet::compile(forbid_reaching.iter().map(|p| (*p).to_owned())).expect("valid globs");
        kind
    }

    fn naming() -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: archwarden_core::pattern::Pattern::compile(
                r"^(?<name>[a-z0-9-]+)\.use-case\.ts$",
            )
            .expect("valid pattern"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind: archwarden_core::facts::KindFilter::OneOf(
                archwarden_core::facts::ExportTags::only(
                    archwarden_core::facts::ExportKind::Function,
                ),
            ),
            annotation: Vec::new(),
            signature_hint: None,
        }
    }

    fn spec_pair() -> CompiledRuleKind {
        CompiledRuleKind::SpecPair {
            subfolders: vec![".".to_owned()],
            spec_markers: vec!["spec".to_owned(), "test".to_owned()],
            ignore_files: PathSet::default(),
            spec_dirs: Vec::new(),
            require_non_empty_spec: false,
            skip_type_only: false,
        }
    }

    fn run(entries: &[(&str, &str)], config: &CompiledConfig) -> Report {
        let (guard, root) = tree_at(entries);
        let tree = crate::walk::walk(&root, config).expect("walks");
        let report = check(Run {
            root: &root,
            config,
            tree: &tree,
            cache: None,
            as_of: archwarden_core::date::Date::EPOCH,
        });
        drop(guard);
        report
    }

    fn offenders(report: &Report) -> Vec<&str> {
        report.findings.iter().map(|f| f.path.as_str()).collect()
    }

    /// The whole pipeline, end to end: files on disk in, findings out.
    #[test]
    fn a_repository_is_walked_and_checked() {
        let report = run(
            &[
                ("packages/domain/src/user/types/id.ts", ""),
                ("packages/domain/src/user/wrong-folder/x.ts", ""),
                ("packages/domain/src/invoice/types/invoice.ts", ""),
            ],
            &config(vec![rule(
                "shape",
                Some("domain"),
                &["packages/domain/src/*"],
                structure(&["types", "calcs"]),
            )]),
        );

        assert_eq!(
            offenders(&report),
            ["packages/domain/src/user/wrong-folder"]
        );
        assert_eq!(report.error_count(), 1);
        assert_eq!(report.warning_count(), 0);
        assert!(report.fails_build());
        assert_eq!(
            report
                .findings
                .first()
                .expect("one")
                .module_id
                .as_ref()
                .map(ModuleId::as_str),
            Some("domain")
        );
    }

    /// Two rules of different kinds over one tree, which is the shape any real
    /// config has.
    #[test]
    fn rules_of_different_kinds_run_over_the_same_tree() {
        let report = run(
            &[
                ("src/user/types/id.ts", ""),
                ("src/user/user.ts", ""),
                ("src/user/nope/x.ts", ""),
            ],
            &config(vec![
                rule("shape", None, &["src/*"], structure(&["types"])),
                rule("needs-spec", None, &["src/*"], spec_pair()),
            ]),
        );

        assert_eq!(
            offenders(&report),
            ["src/user/nope", "src/user/user.ts"],
            "one finding from each rule"
        );
    }

    /// Determinism is a design goal. The same tree must produce byte-identical
    /// output, or every snapshot test becomes noise.
    #[test]
    fn findings_are_ordered_the_same_way_every_run() {
        let entries = [
            ("src/zebra/nope/x.ts", ""),
            ("src/alpha/nope/x.ts", ""),
            ("src/middle/nope/x.ts", ""),
        ];
        let config = config(vec![rule("shape", None, &["src/*"], structure(&["types"]))]);

        let first = run(&entries, &config);
        let second = run(&entries, &config);

        assert_eq!(first.findings, second.findings);
        assert_eq!(
            offenders(&first),
            ["src/alpha/nope", "src/middle/nope", "src/zebra/nope"]
        );
    }

    /// Errors sort before warnings, so the first thing a reader sees is the
    /// thing that blocks them.
    #[test]
    fn errors_sort_before_warnings() {
        let mut warning = rule("warn-rule", None, &["src/*"], structure(&["types"]));
        warning.level = Level::Warning;

        let report = run(
            &[("src/aaa/nope/x.ts", ""), ("src/zzz/nope/x.ts", "")],
            &config(vec![
                warning,
                rule("error-rule", None, &["src/*"], structure(&["types"])),
            ]),
        );

        let levels: Vec<_> = report.findings.iter().map(|f| f.level).collect();
        assert_eq!(
            levels,
            [Level::Error, Level::Error, Level::Warning, Level::Warning]
        );
        assert_eq!(report.error_count(), 2);
        assert_eq!(report.warning_count(), 2);
        assert_eq!(worst_level(&report), Some(Level::Error));
    }

    /// A run with only warnings does not fail a build. Decision 1: warnings
    /// track debt without blocking.
    #[test]
    fn warnings_alone_do_not_fail_the_build() {
        let mut warning = rule("warn-rule", None, &["src/*"], structure(&["types"]));
        warning.level = Level::Warning;

        let report = run(&[("src/user/nope/x.ts", "")], &config(vec![warning]));

        assert_eq!(report.warning_count(), 1);
        assert_eq!(report.error_count(), 0);
        assert!(!report.fails_build());
        assert_eq!(worst_level(&report), Some(Level::Warning));
    }

    #[test]
    fn a_clean_repository_reports_nothing() {
        let report = run(
            &[("src/user/types/id.ts", ""), ("src/user/user.spec.ts", "")],
            &config(vec![rule("shape", None, &["src/*"], structure(&["types"]))]),
        );

        assert!(report.findings.is_empty());
        assert!(!report.fails_build());
        assert_eq!(worst_level(&report), None);
    }

    /// The counts are what the summary line reports, so they have to mean what
    /// a reader assumes: how much was actually looked at.
    #[test]
    fn the_report_counts_what_was_examined() {
        let report = run(
            &[
                ("src/user/a.ts", ""),
                ("src/user/b.ts", ""),
                ("src/invoice/c.ts", ""),
                ("README.md", ""),
            ],
            &config(Vec::new()),
        );

        assert_eq!(report.files_scanned, 4);
        assert_eq!(
            report.directories_scanned, 4,
            "the root, src, src/user and src/invoice"
        );
    }

    /// Every rule kind v0 defines has an engine, so a run can no longer report
    /// one as unchecked. This test says so out loud rather than leaving the
    /// question open: it used `call-obligation` as its example of an
    /// unimplemented kind until M6 implemented it, and a test that encodes
    /// "not yet" has an expiry date.
    ///
    /// It cannot be reintroduced by accident either: `engines_for` matches
    /// `CompiledRuleKind` exhaustively, so a kind without an engine fails to
    /// compile rather than producing a rule nobody checks.
    #[test]
    fn every_rule_kind_reaches_an_engine() {
        let report = run(
            &[(
                "apps/api/route.post.ts",
                "import { Event } from '@org/domain/event';\nexport function POST() { Event.save(); }",
            )],
            &config(vec![rule(
                "must-audit",
                None,
                &["apps/*"],
                CompiledRuleKind::CallObligation {
                    file_pattern: archwarden_core::pattern::Pattern::compile(r"^route\.post\.ts$")
                        .expect("valid pattern"),
                    symbol: "Event.save".to_owned(),
                    imported_from: "@org/domain/event".to_owned(),
                },
            )]),
        );

        assert!(report.findings.is_empty(), "{:?}", report.findings);
        assert_eq!(report.files_parsed, 1, "the rule read the file");
    }

    /// The whole point of the rule, through the real parser: importing the
    /// recorder and never calling it.
    #[test]
    fn a_route_that_imports_without_calling_is_reported() {
        let report = run(
            &[(
                "apps/api/route.post.ts",
                "import { Event } from '@org/domain/event';\nexport function POST() { return Event; }",
            )],
            &config(vec![rule(
                "must-audit",
                None,
                &["apps/*"],
                CompiledRuleKind::CallObligation {
                    file_pattern: archwarden_core::pattern::Pattern::compile(r"^route\.post\.ts$")
                        .expect("valid pattern"),
                    symbol: "Event.save".to_owned(),
                    imported_from: "@org/domain/event".to_owned(),
                },
            )]),
        );

        assert_eq!(report.findings.len(), 1);
        assert_eq!(
            report.findings.first().map(|f| &f.observed),
            Some(&archwarden_core::finding::Observed::RequiredCallMissing {
                symbol: "Event.save".to_owned()
            })
        );
    }

    /// And the case the plan asks for: the export delegates to a helper in the
    /// same file, and the helper is what calls the symbol.
    #[test]
    fn a_call_from_a_local_helper_satisfies_the_obligation() {
        let report = run(
            &[(
                "apps/api/route.post.ts",
                "import { Event } from '@org/domain/event';\n\
                 export function POST() { return handle(); }\n\
                 function handle() { Event.save(); }",
            )],
            &config(vec![rule(
                "must-audit",
                None,
                &["apps/*"],
                CompiledRuleKind::CallObligation {
                    file_pattern: archwarden_core::pattern::Pattern::compile(r"^route\.post\.ts$")
                        .expect("valid pattern"),
                    symbol: "Event.save".to_owned(),
                    imported_from: "@org/domain/event".to_owned(),
                },
            )]),
        );

        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }

    /// The point of the cache: a second run over unchanged files reuses their
    /// facts instead of parsing again. Parsing is the expensive half, and it
    /// is what the cache buys back.
    #[test]
    fn a_second_run_reuses_facts_instead_of_parsing() {
        use archwarden_cache::store::Cache;

        let (guard, root) = tree_at(&[
            (
                "src/user/create-client.use-case.ts",
                "export const CreateClient = () => {};",
            ),
            (
                "src/user/delete-client.use-case.ts",
                "export function DeleteClient() {}",
            ),
        ]);
        let config = config(vec![rule("usecase-name", None, &["src/*"], naming())]);
        let tree = crate::walk::walk(&root, &config).expect("walks");
        let cache_path = root.join(".archwarden/cache/db.redb");

        let cold = {
            let mut cache = Cache::open(&cache_path).expect("opens");
            let report = check(Run {
                root: &root,
                config: &config,
                tree: &tree,
                cache: Some(&mut cache),
                as_of: archwarden_core::date::Date::EPOCH,
            });
            cache.flush().expect("flushes");
            report
        };

        assert_eq!(cold.files_parsed, 2);
        assert_eq!(cold.facts_reused, 0);

        let warm = {
            let mut cache = Cache::open(&cache_path).expect("reopens");
            check(Run {
                root: &root,
                config: &config,
                tree: &tree,
                cache: Some(&mut cache),
                as_of: archwarden_core::date::Date::EPOCH,
            })
        };

        assert_eq!(warm.files_parsed, 0, "nothing needed parsing again");
        assert_eq!(warm.facts_reused, 2);
        assert_eq!(
            warm.findings, cold.findings,
            "a warm run reports exactly what a cold one did"
        );
        drop(guard);
    }

    /// Two files with identical bytes are one cache entry, and the entry was
    /// handed back stamped with whichever file wrote it. `resolve_imports`
    /// reads that field to know which directory a relative specifier points
    /// from, so on a warm run one file's imports were resolved from another
    /// file's directory.
    ///
    /// The consequence is the worst a linter has. Here the violation is in
    /// `src/app`, the twin in `src/zzz` imports something that is not
    /// forbidden, and the cold run says so correctly -- then the warm run over
    /// an untouched tree reported nothing at all and exited clean. Reverse
    /// which twin is stored and the mirror happens: a finding against a file
    /// that imports nothing forbidden. Issue #20.
    #[test]
    fn a_warm_run_does_not_resolve_one_file_from_another_files_directory() {
        use archwarden_cache::store::Cache;

        // Same bytes, two directories, and `../domain/x` means a different
        // file from each of them.
        const TWIN: &str = "import { x } from '../domain/x';\nexport const use = x;";
        let (guard, root) = tree_at(&[
            ("src/domain/x.ts", "export const x = 1;"),
            ("src/zzz/domain/x.ts", "export const x = 2;"),
            ("src/app/thing.ts", TWIN),
            ("src/zzz/deep/thing.ts", TWIN),
        ]);
        let config = config(vec![rule(
            "nothing-imports-domain",
            None,
            &["src/**"],
            boundary(&["src/domain/**"], &[], &[]),
        )]);
        let tree = crate::walk::walk(&root, &config).expect("walks");
        let cache_path = root.join(".archwarden/cache/db.redb");

        let run_with_cache = || {
            let mut cache = Cache::open(&cache_path).expect("opens");
            let report = check(Run {
                root: &root,
                config: &config,
                tree: &tree,
                cache: Some(&mut cache),
                as_of: archwarden_core::date::Date::EPOCH,
            });
            cache.flush().expect("flushes");
            report
        };

        let cold = run_with_cache();
        let warm = run_with_cache();
        drop(guard);

        assert_eq!(
            warm.facts_reused, 4,
            "the cache was warm, or this proves nothing"
        );
        assert_eq!(
            cold.findings.len(),
            1,
            "one real violation, from `src/app`: {:?}",
            cold.findings
        );
        assert_eq!(
            cold.findings.first().map(|f| f.path.as_str()),
            Some("src/app/thing.ts")
        );
        assert_eq!(
            warm.findings, cold.findings,
            "and a warm run over an untouched tree says the same"
        );
    }

    /// A file that changed is parsed again, which is the half of the contract
    /// that matters: a cache that missed an edit would be worse than none.
    #[test]
    fn an_edited_file_is_parsed_again() {
        use archwarden_cache::store::Cache;

        let (guard, root) = tree_at(&[(
            "src/user/create-client.use-case.ts",
            "export function CreateClient() {}",
        )]);
        let config = config(vec![rule("usecase-name", None, &["src/*"], naming())]);
        let cache_path = root.join(".archwarden/cache/db.redb");

        let run_once = |root: &Utf8PathBuf| {
            let tree = crate::walk::walk(root, &config).expect("walks");
            let mut cache = Cache::open(&cache_path).expect("opens");
            let report = check(Run {
                root,
                config: &config,
                tree: &tree,
                cache: Some(&mut cache),
                as_of: archwarden_core::date::Date::EPOCH,
            });
            cache.flush().expect("flushes");
            report
        };

        let first = run_once(&root);
        assert!(first.findings.is_empty(), "the export is a function");

        std::fs::write(
            root.join("src/user/create-client.use-case.ts"),
            "export const CreateClient = () => {};",
        )
        .expect("edit");

        let second = run_once(&root);
        assert_eq!(second.files_parsed, 1, "the edit forced a re-parse");
        assert_eq!(second.facts_reused, 0);
        assert_eq!(second.findings.len(), 1, "and the new fault is reported");
        drop(guard);
    }

    /// A run without a cache is correct, just slower. Nothing about the result
    /// may depend on whether one was supplied.
    #[test]
    fn a_run_without_a_cache_reports_the_same_thing() {
        let entries = [(
            "src/user/create-client.use-case.ts",
            "export const CreateClient = () => {};",
        )];
        let config = config(vec![rule("usecase-name", None, &["src/*"], naming())]);

        let uncached = run(&entries, &config);
        assert_eq!(uncached.files_parsed, 1);
        assert_eq!(uncached.facts_reused, 0);
        assert_eq!(uncached.findings.len(), 1);
    }

    /// The end-to-end shape of a boundary rule: a real tree, a real resolver,
    /// an alias that has to be resolved before any glob can match.
    #[test]
    fn a_forbidden_import_is_found_through_a_tsconfig_alias() {
        let report = run(
            &[
                (
                    "tsconfig.json",
                    r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["packages/*"]}}}"#,
                ),
                (
                    "packages/ui/button.tsx",
                    "import { User } from '@/domain/user';\nexport const Button = () => User;",
                ),
                ("packages/domain/user.ts", "export const User = 1;"),
            ],
            &config(vec![rule(
                "ui-forbids-domain",
                None,
                &["packages/ui/**"],
                boundary(&["packages/domain/**"], &[], &[]),
            )]),
        );

        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        let finding = report.findings.first().expect("one finding");
        assert_eq!(finding.path.as_str(), "packages/ui/button.tsx");
        assert_eq!(
            finding.observed,
            archwarden_core::finding::Observed::ForbiddenImport {
                specifier: "@/domain/user".to_owned(),
                resolved: RepoRelPath::new("packages/domain/user.ts").expect("valid"),
            }
        );
        assert_eq!(
            report.imports,
            crate::resolve::Outcomes {
                in_repo: 1,
                ..crate::resolve::Outcomes::default()
            }
        );
    }

    /// A configuration with no boundary rule never resolves anything. Probing
    /// the filesystem for every specifier in every file is a cost a naming
    /// rule has no use for.
    #[test]
    fn a_configuration_without_a_boundary_rule_resolves_nothing() {
        let report = run(
            &[(
                "src/user/create-client.use-case.ts",
                "import { thing } from './nowhere';\nexport function CreateClient() {}",
            )],
            &config(vec![rule("usecase-name", None, &["src/*"], naming())]),
        );

        assert_eq!(report.files_parsed, 1, "it still parsed");
        assert_eq!(report.imports, crate::resolve::Outcomes::default());
    }

    /// An import nothing could resolve is counted. A boundary rule is blind to
    /// it, and a clean report that did not say so would be lying about what it
    /// checked.
    #[test]
    fn imports_that_did_not_resolve_are_counted() {
        let report = run(
            &[(
                "packages/ui/button.tsx",
                "import { x } from '@org/never-installed';\nimport y from 'node:fs';\nexport const B = 1;",
            )],
            &config(vec![rule(
                "ui-forbids-domain",
                None,
                &["packages/ui/**"],
                boundary(&["packages/domain/**"], &[], &[]),
            )]),
        );

        assert!(report.findings.is_empty(), "nothing matchable was imported");
        assert_eq!(report.imports.unresolved, 1);
        assert_eq!(report.imports.builtin, 1);
        assert_eq!(report.imports.in_repo, 0);
        assert_eq!(
            report.imports.unresolved_imports,
            vec![(
                RepoRelPath::new("packages/ui/button.tsx").expect("valid"),
                "@org/never-installed".to_owned()
            )],
            "and named, or the count is a blind spot nobody can find"
        );
    }

    /// The blind spots are sorted, like every other list in a report: the walk
    /// reaches files in an order nobody can predict, and a CI diff that moves
    /// between two identical runs is noise.
    #[test]
    fn the_unresolved_imports_of_a_run_are_sorted() {
        let report = run(
            &[
                (
                    "packages/ui/z-last.tsx",
                    "import { x } from '@Domain/Order';\nexport const Z = 1;",
                ),
                (
                    "packages/ui/a-first.tsx",
                    "import { y } from '@Shared/thing';\nimport { z } from '@Domain/Id';\nexport const A = 1;",
                ),
            ],
            &config(vec![rule(
                "ui-forbids-domain",
                None,
                &["packages/ui/**"],
                boundary(&["packages/domain/**"], &[], &[]),
            )]),
        );

        let named: Vec<(&str, &str)> = report
            .imports
            .unresolved_imports
            .iter()
            .map(|(path, specifier)| (path.as_str(), specifier.as_str()))
            .collect();
        assert_eq!(
            named,
            vec![
                ("packages/ui/a-first.tsx", "@Domain/Id"),
                ("packages/ui/a-first.tsx", "@Shared/thing"),
                ("packages/ui/z-last.tsx", "@Domain/Order"),
            ]
        );
    }

    /// `FileFacts` come from a TypeScript parser, so only a source file may
    /// ever be handed to one. A `.json` sitting in a folder a facts-needing
    /// rule governs is the case that would break it.
    ///
    /// Every engine today also refuses non-source files on its own, so the
    /// runner's guard is defence in depth and `cargo-mutants` cannot kill it.
    /// It stays because the invariant belongs at the one place that calls the
    /// parser, not spread across every rule that will ever exist.
    #[test]
    fn a_rule_that_needs_facts_still_does_not_parse_a_non_source_file() {
        let report = run(
            &[
                ("src/user/user.ts", "export class User {}"),
                ("src/user/user.spec.ts", "it('works', () => {});"),
                ("src/user/fixture.json", r#"{"name":"x"}"#),
            ],
            &config(vec![rule(
                "spec-pair",
                None,
                &["src/*"],
                CompiledRuleKind::SpecPair {
                    subfolders: vec![".".to_owned()],
                    spec_markers: vec!["spec".to_owned(), "test".to_owned()],
                    ignore_files: PathSet::default(),
                    spec_dirs: Vec::new(),
                    require_non_empty_spec: true,
                    skip_type_only: false,
                },
            )]),
        );

        assert_eq!(
            report.files_parsed, 2,
            "the two TypeScript files, and only those"
        );
        assert!(
            report.unreadable_files.is_empty(),
            "and nothing was attempted on the JSON: {:?}",
            report.unreadable_files
        );
        assert!(report.findings.is_empty());
    }

    /// A file a rule wanted to read but could not is named in the report. A
    /// clean-looking result that quietly skipped a file would be lying about
    /// what it checked.
    #[test]
    fn a_file_that_cannot_be_read_is_reported_not_dropped() {
        let (guard, root) = tree_at(&[(
            "src/user/create-client.use-case.ts",
            "export function CreateClient() {}",
        )]);
        // Latin-1 where UTF-8 was expected: a real thing to find in an old
        // repository, and not something a parser can be handed.
        std::fs::write(
            root.join("src/user/broken.use-case.ts"),
            [0x65, 0x78, 0x70, 0x6f, 0x72, 0x74, 0xff, 0xfe],
        )
        .expect("write file");

        let config = config(vec![rule("usecase-name", None, &["src/*"], naming())]);
        let tree = crate::walk::walk(&root, &config).expect("walks");
        let report = check(Run {
            root: &root,
            config: &config,
            tree: &tree,
            cache: None,
            as_of: archwarden_core::date::Date::EPOCH,
        });
        drop(guard);

        assert_eq!(report.unreadable_files.len(), 1);
        let (path, reason) = &report.unreadable_files[0];
        assert_eq!(path.as_str(), "src/user/broken.use-case.ts");
        assert!(!reason.is_empty(), "the reason is shown to the user");
        assert_eq!(
            report.files_parsed, 1,
            "the readable file was still checked"
        );
        assert!(report.findings.is_empty());

        // And the check that did not happen is counted. Naming the file is not
        // enough on its own: a reader has to work out which rules wanted it,
        // and "no findings" over a file nothing could evaluate is a clean
        // report that is not one.
        assert_eq!(report.checks_skipped, 1);
    }

    /// The number is checks, not files. One unreadable file that three rules
    /// wanted is three answers nobody got, and reporting `1` would understate
    /// it by exactly the amount that matters.
    #[test]
    fn a_skip_is_counted_once_per_rule_that_wanted_the_file() {
        let (guard, root) = tree_at(&[]);
        std::fs::create_dir_all(root.join("src/user")).expect("create dirs");
        std::fs::write(
            root.join("src/user/broken.use-case.ts"),
            [0x65, 0x78, 0x70, 0x6f, 0x72, 0x74, 0xff, 0xfe],
        )
        .expect("write file");

        let config = config(vec![
            rule("first", None, &["src/*"], naming()),
            rule("second", None, &["src/*"], naming()),
        ]);
        let tree = crate::walk::walk(&root, &config).expect("walks");
        let report = check(Run {
            root: &root,
            config: &config,
            tree: &tree,
            cache: None,
            as_of: archwarden_core::date::Date::EPOCH,
        });
        drop(guard);

        assert_eq!(report.unreadable_files.len(), 1, "one file");
        assert_eq!(report.checks_skipped, 2, "two rules wanted it");

        // The count on its own tells a reader the run decided less than it
        // looks like and gives them nowhere to look. `AGENTS.md` calls this
        // "the number to watch", which is only true if it can be acted on.
        assert_eq!(
            report
                .skipped_checks
                .iter()
                .map(|(rule, path)| format!("{rule} {path}"))
                .collect::<Vec<_>>(),
            [
                "first src/user/broken.use-case.ts",
                "second src/user/broken.use-case.ts",
            ],
            "each skip names the rule that wanted the file, and the file"
        );
    }

    /// And a clean run carries an empty list, not a missing one: a consumer
    /// branching on it needs the field to be there.
    #[test]
    fn a_clean_run_names_no_skipped_checks() {
        let (guard, root) = tree_at(&[("src/user/thing.ts", "export const thing = 1;\n")]);
        let config = config(vec![rule("shape", None, &["src/*"], structure(&["calcs"]))]);
        let tree = crate::walk::walk(&root, &config).expect("walks");
        let report = check(Run {
            root: &root,
            config: &config,
            tree: &tree,
            cache: None,
            as_of: archwarden_core::date::Date::EPOCH,
        });
        drop(guard);

        assert!(report.skipped_checks.is_empty());
    }

    /// A boundary rule whose scope reaches a `DOC.md` has no facts for it and
    /// never could. That is not an answer anybody lost, and counting it pinned
    /// the number at one per rule per documented layer -- permanently non-zero,
    /// permanently benign, and impossible to fix, because there is nothing
    /// wrong with the file. `AGENTS.md` says a run with skips must not be
    /// reported as clean, so such a count teaches a reader to ignore the one
    /// number it tells them to watch. Issue #15.
    /// Issue #44's other half. A `.py` under a boundary rule used to be
    /// `Other` — the class that exists so a PNG does not inflate the count — so
    /// the rule saw no imports, reported nothing, and counted nothing. A rule
    /// enforcing nothing looks exactly like a repository that satisfies it,
    /// which `CONFIG.md` calls the worst failure a linter has.
    ///
    /// Nothing here becomes readable. It becomes *countable*.
    #[test]
    fn source_this_build_cannot_read_is_a_check_nobody_could_make() {
        let (guard, root) = tree_at(&[
            ("src/domain/order.ts", "export const order = 1;\n"),
            (
                "src/domain/handler.py",
                "from src.infrastructure import db\n",
            ),
        ]);
        let config = config(vec![rule(
            "domain-forbids-infrastructure",
            None,
            &["src/*"],
            boundary(&["src/infrastructure/**"], &[], &[]),
        )]);
        let tree = crate::walk::walk(&root, &config).expect("walks");
        let report = check(Run {
            root: &root,
            config: &config,
            tree: &tree,
            cache: None,
            as_of: archwarden_core::date::Date::EPOCH,
        });
        drop(guard);

        assert_eq!(
            report.checks_skipped, 1,
            "the Python file has imports this build cannot read"
        );
        assert_eq!(
            report
                .skipped_checks
                .iter()
                .map(|(_, path)| path.as_str())
                .collect::<Vec<_>>(),
            ["src/domain/handler.py"],
            "and the run names it, so nobody has to guess"
        );
    }

    #[test]
    fn a_file_that_is_not_source_is_not_a_check_nobody_could_make() {
        let (guard, root) = tree_at(&[
            ("src/domain/order.ts", "export const order = 1;\n"),
            ("src/domain/DOC.md", "# The domain layer\n"),
        ]);
        let config = config(vec![rule(
            "domain-forbids-infrastructure",
            None,
            &["src/*"],
            boundary(&["src/infrastructure/**"], &[], &[]),
        )]);
        let tree = crate::walk::walk(&root, &config).expect("walks");
        let report = check(Run {
            root: &root,
            config: &config,
            tree: &tree,
            cache: None,
            as_of: archwarden_core::date::Date::EPOCH,
        });
        drop(guard);

        assert_eq!(
            report.checks_skipped, 0,
            "a markdown file is not a check nobody could make"
        );
        assert!(
            report.skipped_checks.is_empty(),
            "and nothing points at it to investigate: {:?}",
            report.skipped_checks
        );

        // The other half of the same `is_source`, one decision earlier: the
        // markdown is not read at all. Asserted because it is the difference
        // between "we knew not to ask" and "we asked and threw the answer
        // away" -- and because on a repository whose boundary scope covers a
        // directory of images, asking is the expensive half.
        assert_eq!(
            report.files_parsed, 1,
            "only the `.ts` was parsed; the `.md` was never opened"
        );
        assert!(
            report.unreadable_files.is_empty(),
            "and it is not reported as unreadable, because nothing read it: {:?}",
            report.unreadable_files
        );
    }

    /// The other half of the same line, and the half that must not move: a
    /// source file that would not parse is still a lost answer, and still
    /// counted. Without this the fix above could be "stop counting", which is
    /// the failure `checks_skipped` exists to prevent.
    ///
    /// Both files sit under one `import-boundary` rule on purpose. A boundary
    /// rule applies to everything in its scope, so the `.md` and the broken
    /// `.ts` both reach the decision and the assertion is about which of them
    /// is counted -- with a `naming` rule the `file_pattern` would filter the
    /// `.md` out first and this would pass without testing anything.
    #[test]
    fn a_source_file_that_will_not_parse_still_counts_beside_one_that_is_not_source() {
        let (guard, root) = tree_at(&[("src/user/DOC.md", "# Users\n")]);
        std::fs::write(
            root.join("src/user/broken.ts"),
            [0x65, 0x78, 0x70, 0x6f, 0x72, 0x74, 0xff, 0xfe],
        )
        .expect("write file");

        let config = config(vec![rule(
            "user-forbids-infrastructure",
            None,
            &["src/*"],
            boundary(&["src/infrastructure/**"], &[], &[]),
        )]);
        let tree = crate::walk::walk(&root, &config).expect("walks");
        let report = check(Run {
            root: &root,
            config: &config,
            tree: &tree,
            cache: None,
            as_of: archwarden_core::date::Date::EPOCH,
        });
        drop(guard);

        assert_eq!(
            report.checks_skipped, 1,
            "the unparsable `.ts` counts, the `.md` beside it does not"
        );
        assert_eq!(
            report
                .skipped_checks
                .iter()
                .map(|(rule, path)| format!("{rule} {path}"))
                .collect::<Vec<_>>(),
            ["user-forbids-infrastructure src/user/broken.ts"],
        );
    }

    /// A run where everything could be read skips nothing, so the common
    /// report says nothing about it.
    #[test]
    fn a_run_that_read_everything_skips_nothing() {
        let (guard, root) = tree_at(&[(
            "src/user/create-client.use-case.ts",
            "export function CreateClient() {}",
        )]);
        let config = config(vec![rule("usecase-name", None, &["src/*"], naming())]);
        let tree = crate::walk::walk(&root, &config).expect("walks");
        let report = check(Run {
            root: &root,
            config: &config,
            tree: &tree,
            cache: None,
            as_of: archwarden_core::date::Date::EPOCH,
        });
        drop(guard);

        assert_eq!(report.checks_skipped, 0);
    }

    /// The caller can tell, before walking anything, whether a cache would
    /// ever be consulted.
    #[test]
    fn a_configuration_says_whether_it_reads_files() {
        assert!(!reads_files(&config(vec![rule(
            "shape",
            None,
            &["src/*"],
            structure(&["types"]),
        )])));
        assert!(reads_files(&config(vec![rule(
            "usecase-name",
            None,
            &["src/*"],
            naming(),
        )])));
        assert!(!reads_files(&config(Vec::new())), "no rules, no reads");
    }

    /// A structure-only configuration reads no bytes, cache or no cache. On a
    /// large repository that is the difference between a walk and thirty
    /// thousand reads.
    #[test]
    fn a_structural_configuration_parses_nothing() {
        let report = run(
            &[("src/user/nope/x.ts", ""), ("src/user/types/y.ts", "")],
            &config(vec![rule("shape", None, &["src/*"], structure(&["types"]))]),
        );

        assert_eq!(report.files_parsed, 0);
        assert_eq!(report.facts_reused, 0);
        assert_eq!(report.findings.len(), 1, "and it still checks");
    }

    /// The escape hatch reaches the run: a `_`-prefixed folder is invisible to
    /// the structure rule while its files stay in the tree.
    #[test]
    fn the_escape_hatch_survives_the_whole_pipeline() {
        let report = run(
            &[
                ("src/user/_internal/helper.ts", ""),
                ("src/user/nope/x.ts", ""),
            ],
            &config(vec![rule("shape", None, &["src/*"], structure(&["types"]))]),
        );

        assert_eq!(offenders(&report), ["src/user/nope"]);
        assert_eq!(report.files_scanned, 2, "the exempt file is still counted");
    }
    /// A marker suppresses the finding on the line after it, and the
    /// suppression is reported rather than dropped.
    ///
    /// The whole argument of issue #72: `// eslint-disable-next-line` with no
    /// explanation is how debt becomes invisible, so a suppression here is
    /// never absent from a report — it is a line of its own, carrying the
    /// reason, and a run with forty of them must not look like a clean one.
    #[test]
    fn an_allowed_finding_is_moved_to_the_suppressed_list_with_its_reason() {
        let report = run(
            &[
                (
                    "packages/ui/button.tsx",
                    "// archwarden-allow: the vendor SDK has no types\n\
                     import { User } from '../domain/user';\n\
                     export const Button = () => User;",
                ),
                ("packages/domain/user.ts", "export const User = 1;"),
            ],
            &config(vec![rule(
                "ui-forbids-domain",
                None,
                &["packages/ui/**"],
                boundary(&["packages/domain/**"], &[], &[]),
            )]),
        );

        assert!(
            report.findings.is_empty(),
            "it is not reported as a violation: {:?}",
            report.findings
        );
        assert_eq!(report.suppressed.len(), 1, "{:?}", report.suppressed);
        assert_eq!(
            report.suppressed[0].reason, "the vendor SDK has no types",
            "and the reason travels with it, or the feature is \
             `eslint-disable` again"
        );
        assert_eq!(
            report.suppressed[0].finding.path.as_str(),
            "packages/ui/button.tsx"
        );
        assert!(!report.fails_build());
    }

    /// A marker one line too far up suppresses nothing.
    ///
    /// It governs the line *after* it, and only that one. A marker that
    /// reached further would be a file-scoped exception, which is what
    /// `baseline` is for and is a different promise: *this repository has this
    /// debt today* against *this line is a deliberate exception*.
    #[test]
    fn a_marker_governs_only_the_line_after_it() {
        let report = run(
            &[
                (
                    "packages/ui/button.tsx",
                    "// archwarden-allow: this is about the next line\n\
                     export const spacer = 1;\n\
                     import { User } from '../domain/user';\n\
                     export const Button = () => User;",
                ),
                ("packages/domain/user.ts", "export const User = 1;"),
            ],
            &config(vec![rule(
                "ui-forbids-domain",
                None,
                &["packages/ui/**"],
                boundary(&["packages/domain/**"], &[], &[]),
            )]),
        );

        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        assert!(report.suppressed.is_empty());
    }

    /// A marker naming a rule suppresses that rule and no other.
    #[test]
    fn a_marker_that_names_a_rule_leaves_the_others_alone() {
        let report = run(
            &[
                (
                    "packages/ui/button.tsx",
                    "// archwarden-allow someone-elses-rule: not this one\n\
                     import { User } from '../domain/user';\n\
                     export const Button = () => User;",
                ),
                ("packages/domain/user.ts", "export const User = 1;"),
            ],
            &config(vec![rule(
                "ui-forbids-domain",
                None,
                &["packages/ui/**"],
                boundary(&["packages/domain/**"], &[], &[]),
            )]),
        );

        assert_eq!(
            report.findings.len(),
            1,
            "the marker is about another rule: {:?}",
            report.findings
        );
    }

    /// `governance: closed` — a file no rule governs is a finding.
    ///
    /// `CONFIG.md` calls a rule enforcing nothing the worst failure a linter
    /// has, and this is that sentence one level up: a file no rule governs is
    /// indistinguishable from a file that satisfies every rule, and `check`
    /// reporting `0 errors` over it reads as though the architecture held.
    /// Issue #60.
    #[test]
    fn an_ungoverned_file_is_reported_when_the_architecture_is_closed() {
        let config = config(vec![rule("shape", None, &["src/*"], structure(&["types"]))])
            .with_governance(Some(Level::Error));

        let report = run(
            &[
                // `roots: src/*` selects `src/user`, so `structure` claims the
                // files directly in it -- `filename_patterns` is what it would
                // constrain them with. A file one level deeper is outside the
                // rule and is reported below, correctly.
                ("src/user/thing.ts", ""),
                ("scripts/build.ts", ""),
                ("scripts/deploy.ts", ""),
            ],
            &config,
        );

        assert_eq!(
            offenders(&report),
            ["scripts/build.ts", "scripts/deploy.ts"],
            "one per file, and the file `structure` claims is not among them: \
             {:?}",
            report.findings
        );
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.observed == archwarden_core::finding::Observed::Ungoverned),
            "{:?}",
            report.findings
        );
        assert!(report.fails_build());
    }

    /// Deeper than the rule reaches is still ungoverned, and that is the
    /// answer the report has to give.
    ///
    /// `roots: src/*` selects the direct children of `src` and claims the
    /// files in them. A file two levels down is outside it, and calling that
    /// governed because a rule mentions an ancestor is exactly the comfortable
    /// lie `governance: closed` exists to refuse.
    #[test]
    fn a_file_deeper_than_any_rule_reaches_is_ungoverned() {
        let report = run(
            &[("src/user/thing.ts", ""), ("src/user/types/id.ts", "")],
            &config(vec![rule("shape", None, &["src/*"], structure(&["types"]))])
                .with_governance(Some(Level::Error)),
        );

        assert_eq!(
            offenders(&report),
            ["src/user/types/id.ts"],
            "{:?}",
            report.findings
        );
    }

    /// The default, and the one that must not regress: a configuration that
    /// says nothing reports nothing new.
    ///
    /// A field defaulting the other way would turn every existing config into
    /// thousands of findings on upgrade, over code nobody touched.
    #[test]
    fn an_open_architecture_reports_no_ungoverned_files() {
        let report = run(
            &[("src/user/types/id.ts", ""), ("scripts/build.ts", "")],
            &config(vec![rule("shape", None, &["src/*"], structure(&["types"]))]),
        );

        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }

    /// The level is the configuration's, so a migration can turn this on and
    /// block nobody while it closes the gap.
    #[test]
    fn a_closed_architecture_reports_at_the_level_it_was_given() {
        let report = run(
            &[("scripts/build.ts", "")],
            &config(Vec::new()).with_governance(Some(Level::Warning)),
        );

        assert_eq!(report.warning_count(), 1);
        assert_eq!(report.error_count(), 0);
        assert!(
            !report.fails_build(),
            "which is the point of offering the level at all"
        );
    }

    /// `ignore` is the escape hatch, and gains a meaning it did not have:
    /// deliberately outside the architecture rather than merely unchecked.
    #[test]
    fn an_ignored_file_is_deliberately_outside_the_architecture() {
        let (guard, root) = tree_at(&[("scripts/build.ts", ""), ("src/a.ts", "")]);
        let config = CompiledConfig::new(
            Vec::new(),
            PathSet::compile(["scripts/**".to_owned()]).expect("valid globs"),
            SkipDirs::default(),
            ContentHash::of(b""),
        )
        .with_governance(Some(Level::Error));
        let tree = crate::walk::walk(&root, &config).expect("walks");
        let report = check(Run {
            root: &root,
            config: &config,
            tree: &tree,
            cache: None,
            as_of: archwarden_core::date::Date::EPOCH,
        });
        drop(guard);

        assert_eq!(
            offenders(&report),
            ["src/a.ts"],
            "the ignored file is a decision somebody wrote down: {:?}",
            report.findings
        );
    }

    /// The whole reason the graph is built from every file rather than from
    /// the ones a rule's scope reaches.
    ///
    /// The rule governs `apps/**`. The loop leaves it, passes through
    /// `packages/db`, and comes back. Nothing else in the configuration
    /// mentions `packages`, so under the per-file gating every other rule
    /// enjoys, that file is never parsed, never resolved, and contributes no
    /// edge — and the cycle rule reports a clean repository. A rule that
    /// enforces nothing looks exactly like a repository that satisfies it,
    /// which `CONFIG.md` calls the worst failure a linter has.
    #[test]
    fn a_loop_that_leaves_the_scope_and_comes_back_is_still_reported() {
        let report = run(
            &[
                (
                    "apps/api/handler.ts",
                    "import { save } from '../../packages/db/save';\n\
                     export const handle = () => save();",
                ),
                (
                    "packages/db/save.ts",
                    "import { handle } from '../../apps/api/handler';\n\
                     export const save = () => handle();",
                ),
            ],
            &config(vec![rule(
                "no-cycles",
                None,
                &["apps/**"],
                CompiledRuleKind::ImportCycle {
                    include_type_only: false,
                },
            )]),
        );

        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        let finding = report.findings.first().expect("one finding");
        assert_eq!(finding.path.as_str(), "apps/api/handler.ts");
        assert_eq!(
            finding.observed,
            archwarden_core::finding::Observed::ImportCycle {
                chain: vec![
                    RepoRelPath::new("apps/api/handler.ts").expect("valid"),
                    RepoRelPath::new("packages/db/save.ts").expect("valid"),
                    RepoRelPath::new("apps/api/handler.ts").expect("valid"),
                ],
            },
            "and the chain names the file outside the scope, because that is \
             the edge somebody has to cut"
        );
    }

    /// A rule that does not read the graph runs in the main loop and **not
    /// again** in the deferred pass, even when it covers a file a graph rule
    /// held back.
    ///
    /// The deferred pass walks the same files a second time. Anything that
    /// picked engines by "applies to this path" rather than by "reads the
    /// graph" would evaluate every ordinary rule twice, and a report that
    /// names one violation twice is one nobody trusts the counts in.
    #[test]
    fn a_rule_that_does_not_read_the_graph_is_not_run_twice() {
        let report = run(
            &[
                (
                    "src/a.ts",
                    "import { b } from './b';\nexport const a = () => b();",
                ),
                (
                    "src/b.ts",
                    "import { a } from './a';\nexport const b = () => a();",
                ),
            ],
            &config(vec![
                rule(
                    "no-cycles",
                    None,
                    &["src/**"],
                    CompiledRuleKind::ImportCycle {
                        include_type_only: false,
                    },
                ),
                // Covers exactly the same files, and reads no graph.
                rule(
                    "nothing-imports-nowhere",
                    None,
                    &["src/**"],
                    boundary(&["nowhere/**"], &["src/**"], &[]),
                ),
            ]),
        );

        let by_rule = |id: &str| {
            report
                .findings
                .iter()
                .filter(|finding| finding.rule_id.as_str() == id)
                .count()
        };

        assert_eq!(
            by_rule("no-cycles"),
            2,
            "both files of the loop: {:?}",
            report.findings
        );
        assert_eq!(
            by_rule("nothing-imports-nowhere"),
            0,
            "it is satisfied, and being satisfied twice is still zero -- the \
             count below is what would move: {:?}",
            report.findings
        );

        // The half that actually bites. A rule reporting a violation on a file
        // a graph rule also covers must report it once.
        let doubled = run(
            &[
                (
                    "src/a.ts",
                    "import { b } from './b';\nexport const a = () => b();",
                ),
                (
                    "src/b.ts",
                    "import { a } from './a';\nexport const b = () => a();",
                ),
            ],
            &config(vec![
                rule(
                    "no-cycles",
                    None,
                    &["src/**"],
                    CompiledRuleKind::ImportCycle {
                        include_type_only: false,
                    },
                ),
                rule(
                    "must-import-elsewhere",
                    None,
                    &["src/**"],
                    boundary(&[], &["packages/**"], &[]),
                ),
            ]),
        );

        assert_eq!(
            doubled
                .findings
                .iter()
                .filter(|finding| finding.rule_id.as_str() == "must-import-elsewhere")
                .count(),
            2,
            "one per file, not two per file: {:?}",
            doubled.findings
        );
    }

    /// Issue #71, through the whole pipeline: real files, a real resolver, and
    /// a dependency nobody wrote down.
    ///
    /// `apps/api` never mentions `packages/db`. It imports `packages/orders`,
    /// which does — and neither of those two files is in the rule's scope, so
    /// this is also the graph being built from more than the scope reaches.
    #[test]
    fn a_dependency_reached_through_another_package_is_found() {
        let report = run(
            &[
                (
                    "apps/api/handler.ts",
                    "import { place } from '../../packages/orders/place';\n\
                     export const handle = () => place();",
                ),
                (
                    "packages/orders/place.ts",
                    "import { save } from '../db/client';\n\
                     export const place = () => save();",
                ),
                ("packages/db/client.ts", "export const save = () => 1;"),
            ],
            &config(vec![rule(
                "api-must-not-reach-db",
                None,
                &["apps/**"],
                reaching(&["packages/db/**"]),
            )]),
        );

        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        let finding = report.findings.first().expect("one finding");
        assert_eq!(finding.path.as_str(), "apps/api/handler.ts");
        assert_eq!(
            finding.observed,
            archwarden_core::finding::Observed::ForbiddenReach {
                chain: vec![
                    RepoRelPath::new("apps/api/handler.ts").expect("valid"),
                    RepoRelPath::new("packages/orders/place.ts").expect("valid"),
                    RepoRelPath::new("packages/db/client.ts").expect("valid"),
                ],
            }
        );
    }

    /// And the direct import stays `forbid_import_from`'s finding. One fault,
    /// one finding: a rule that set both would otherwise report a direct
    /// import twice.
    #[test]
    fn a_direct_import_is_not_also_reported_as_reach() {
        let report = run(
            &[
                (
                    "apps/api/handler.ts",
                    "import { save } from '../../packages/db/client';\n\
                     export const handle = () => save();",
                ),
                ("packages/db/client.ts", "export const save = () => 1;"),
            ],
            &config(vec![rule(
                "api-must-not-reach-db",
                None,
                &["apps/**"],
                reaching(&["packages/db/**"]),
            )]),
        );

        assert!(
            report.findings.is_empty(),
            "the direct import is not this rule's finding: {:?}",
            report.findings
        );
    }

    /// Both ends of the same loop, when the scope covers both. A loop has no
    /// owner: N files have to change, and the report says N.
    #[test]
    fn every_file_of_a_loop_inside_the_scope_is_reported() {
        let report = run(
            &[
                (
                    "src/a.ts",
                    "import { b } from './b';\nexport const a = () => b();",
                ),
                (
                    "src/b.ts",
                    "import { a } from './a';\nexport const b = () => a();",
                ),
            ],
            &config(vec![rule(
                "no-cycles",
                None,
                &["src/**"],
                CompiledRuleKind::ImportCycle {
                    include_type_only: false,
                },
            )]),
        );

        assert_eq!(
            offenders(&report),
            ["src/a.ts", "src/b.ts"],
            "{:?}",
            report.findings
        );
    }

    /// The fast path, which is the point of gating the graph behind a
    /// question. A configuration with no graph rule must not start parsing
    /// files nothing asked about.
    #[test]
    fn a_configuration_without_a_graph_rule_parses_only_what_a_rule_covers() {
        let report = run(
            &[
                (
                    "src/user/create-client.use-case.ts",
                    "export function CreateClient() {}",
                ),
                ("elsewhere/untouched.ts", "export const x = 1;"),
            ],
            &config(vec![rule("usecase-name", None, &["src/*"], naming())]),
        );

        assert!(!needs_graph(&config(vec![rule(
            "usecase-name",
            None,
            &["src/*"],
            naming(),
        )])));
        assert_eq!(
            report.files_parsed, 1,
            "the file outside every scope was never opened"
        );
    }

    /// And the question is answerable before any walking happens, which is
    /// what lets the run decide once rather than per file.
    #[test]
    fn a_configuration_says_whether_it_needs_the_graph() {
        assert!(needs_graph(&config(vec![rule(
            "no-cycles",
            None,
            &["src/**"],
            CompiledRuleKind::ImportCycle {
                include_type_only: false,
            },
        )])));
        assert!(!needs_graph(&config(Vec::new())), "no rules, no graph");
    }

    /// Resolution is asked of the rules that apply to *this* file, not of the
    /// run as a whole.
    ///
    /// It used to be global: if any rule anywhere needed resolution, every file
    /// that had facts for any reason had its imports resolved. Measured on a
    /// real repository, adding a boundary rule governing **one file** cost
    /// about 0.2 s, because every file a `no-passthrough` rule covered was then
    /// resolved for nothing. Issue #79.
    #[test]
    fn a_file_no_resolving_rule_covers_is_not_resolved() {
        let (guard, root) = tree_at(&[
            ("apps/web/page.ts", "import { a } from './helper';\n"),
            ("apps/web/helper.ts", "export const a = 1;\n"),
            ("packages/domain/x.ts", "import { a } from './y';\n"),
            ("packages/domain/y.ts", "export const a = 1;\n"),
        ]);

        // A boundary rule over `packages/*` only, and a `no-passthrough` rule
        // over everything — so `apps/web` has facts and nothing that needs its
        // imports placed.
        let config = config(vec![
            rule(
                "domain-boundary",
                None,
                &["packages/*"],
                boundary(&["nowhere/**"], &[], &[]),
            ),
            rule(
                "no-barrels",
                None,
                &["apps/*"],
                CompiledRuleKind::NoPassthrough {
                    forms: archwarden_core::compiled::PassthroughForms {
                        reexport: true,
                        alias: true,
                        wrapper: true,
                    },
                    except: PathSet::default(),
                    allow_package_entrypoints: false,
                    allow_partial: false,
                },
            ),
        ]);

        let tree = crate::walk::walk(&root, &config).expect("walks");
        let report = check(Run {
            root: &root,
            config: &config,
            tree: &tree,
            cache: None,
            as_of: archwarden_core::date::Date::EPOCH,
        });
        drop(guard);

        assert_eq!(
            report.imports.in_repo, 1,
            "only the file a boundary rule covers should have been resolved"
        );
    }
}

#[cfg(test)]
mod narrowing_tests {
    use archwarden_core::compiled::{
        CompiledConfig, CompiledRule, CompiledRuleKind, ImportFilter, SkipDirs,
    };
    use archwarden_core::glob::PathSet;
    use archwarden_core::hash::ContentHash;
    use archwarden_core::ids::RuleId;
    use archwarden_core::level::Level;
    use archwarden_core::pattern::Pattern;
    use archwarden_core::scope::Scope;
    use camino::Utf8PathBuf;

    /// A `presence` rule over `src/*`, optionally narrowed by what the files
    /// inside import.
    fn presence(narrowed: Option<&str>) -> CompiledConfig {
        CompiledConfig::new(
            vec![CompiledRule {
                id: RuleId::new("p").expect("valid id"),
                module: None,
                why: None,
                module_why: None,
                decision: None,
                imports: narrowed.map(|glob| ImportFilter {
                    paths: PathSet::compile([glob.to_owned()]).expect("valid glob"),
                    packages: Vec::new(),
                }),
                level: Level::Error,
                scope: Scope::compile(["src/*"]).expect("valid scope"),
                kind: CompiledRuleKind::Presence {
                    require: vec!["contract.md".to_owned()],
                    require_any: Vec::new(),
                },
            }],
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b"narrowing"),
        )
    }

    /// A `naming` rule that no file satisfies, so that whether it *ran* is
    /// visible in the findings.
    fn naming(narrowed: Option<&str>) -> CompiledConfig {
        CompiledConfig::new(
            vec![CompiledRule {
                id: RuleId::new("n").expect("valid id"),
                module: None,
                why: None,
                module_why: None,
                decision: None,
                imports: narrowed.map(|glob| ImportFilter {
                    paths: PathSet::compile([glob.to_owned()]).expect("valid glob"),
                    packages: Vec::new(),
                }),
                level: Level::Error,
                scope: Scope::compile(["src/*"]).expect("valid scope"),
                kind: CompiledRuleKind::Naming {
                    file_pattern: Pattern::compile(r"^(?<n>[a-z-]+)\.ts$").expect("valid"),
                    dir_pattern: None,
                    name_template: "Nothing{{pascal(n)}}".to_owned(),
                    kind: archwarden_core::facts::KindFilter::Any,
                    annotation: Vec::new(),
                    signature_hint: None,
                },
            }],
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b"narrowing"),
        )
    }

    fn tree(files: &[(&str, &str)]) -> (tempfile::TempDir, Utf8PathBuf) {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("utf-8");
        for (name, contents) in files {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("create");
            std::fs::write(&path, contents).expect("write");
        }
        (guard, root)
    }

    fn run(root: &camino::Utf8Path, config: &CompiledConfig) -> crate::run::Report {
        let tree = crate::walk::walk(root, config).expect("walks");
        crate::run::check(crate::run::Run {
            root,
            config,
            tree: &tree,
            cache: None,
            as_of: archwarden_core::date::Date::today(),
        })
    }

    const TREE: &[(&str, &str)] = &[
        ("src/http/connection.ts", "export const conn = 1;\n"),
        (
            "src/orders/update.ts",
            "import { conn } from '../http/connection';\nexport const u = () => conn;\n",
        ),
        ("src/reports/monthly.ts", "export const m = () => 1;\n"),
    ];

    /// A rule narrowed by imports resolves, even though nothing else in the
    /// configuration asked for resolution. Without that the filter would be
    /// asked about imports nobody had placed and would answer "no" to
    /// everything. Decision 25.
    #[test]
    fn a_narrowed_rule_turns_resolution_on_by_itself() {
        let (_guard, root) = tree(TREE);

        let report = run(&root, &naming(Some("src/http/**")));

        assert!(
            report.imports.in_repo > 0,
            "resolution ran because a rule asked: {:?}",
            report.imports
        );
    }

    /// And a configuration that narrows nothing resolves nothing, which is
    /// what keeps every rule written before 0.20 as cheap as it was.
    #[test]
    fn a_configuration_that_narrows_nothing_resolves_nothing() {
        let (_guard, root) = tree(TREE);

        let report = run(&root, &naming(None));

        assert_eq!(
            report.imports.in_repo, 0,
            "nothing asked, so nothing was resolved: {:?}",
            report.imports
        );
    }

    /// A file rule narrowed by imports runs on the files that import, and not
    /// on the ones that do not.
    #[test]
    fn a_file_rule_runs_only_where_the_imports_say_so() {
        let (_guard, root) = tree(TREE);

        let flagged: Vec<String> = run(&root, &naming(Some("src/http/**")))
            .findings
            .iter()
            .map(|finding| finding.path.as_str().to_owned())
            .collect();

        assert_eq!(flagged, ["src/orders/update.ts"]);
    }

    /// A directory rule reports only the directories something inside talks
    /// to. The one that talks to nothing is not this rule's business, and a
    /// rule that reported it would be reporting a directory it was never
    /// about.
    #[test]
    fn a_directory_rule_reports_only_where_something_inside_matched() {
        let (_guard, root) = tree(TREE);

        let flagged: Vec<String> = run(&root, &presence(Some("src/http/connection.ts")))
            .findings
            .iter()
            .map(|finding| finding.path.as_str().to_owned())
            .collect();

        assert_eq!(flagged, ["src/orders"]);
    }

    /// And the same rule without the narrowing reports every directory its
    /// scope reaches — which is what makes the test above mean something.
    #[test]
    fn the_same_directory_rule_unnarrowed_reports_all_of_them() {
        let (_guard, root) = tree(TREE);

        let flagged: Vec<String> = run(&root, &presence(None))
            .findings
            .iter()
            .map(|finding| finding.path.as_str().to_owned())
            .collect();

        assert_eq!(flagged, ["src/http", "src/orders", "src/reports"]);
    }
}
