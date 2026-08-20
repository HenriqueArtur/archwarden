//! The three seams: parsing, resolution, and rule evaluation.
//!
//! Rule engines depend on this crate alone. They receive already-extracted
//! facts and already-resolved paths, so replacing the parser or the resolver
//! never touches rule code. See decision 6.

use camino::Utf8PathBuf;

use crate::{
    facts::FileFacts,
    finding::{Expectation, Finding},
    hash::ContentHash,
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
};

/// Turns source text into facts.
///
/// `Send + Sync` because fact extraction runs one task per file under rayon.
pub trait Parser: Send + Sync {
    /// The error this parser produces.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Extracts facts from one file's source.
    ///
    /// # Errors
    /// When the source cannot be parsed.
    fn parse(
        &self,
        path: &RepoRelPath,
        source: &str,
        content_hash: ContentHash,
    ) -> Result<FileFacts, Self::Error>;
}

/// Where an import specifier ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Resolved {
    /// A file inside the repository. Boundary rules match globs against this.
    InRepo(RepoRelPath),
    /// A real file outside the repository: a dependency in `node_modules`, or
    /// a package in a pnpm store. Preset resolution for `extends` lands here,
    /// which is why this variant exists rather than everything being
    /// repository-relative.
    External(Utf8PathBuf),
    /// A runtime builtin such as `node:fs`, which has no file at all.
    Builtin(String),
}

/// Turns a specifier into a path.
///
/// Two production uses and one test use: resolving a preset package name for
/// `extends`, resolving import specifiers for the boundary rules, and an
/// in-memory implementation so graph rules can be tested without a filesystem.
pub trait Resolver: Send + Sync {
    /// The error this resolver produces.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Resolves `specifier` as written inside `importer`.
    ///
    /// # Errors
    /// When the specifier cannot be resolved.
    fn resolve(&self, importer: &RepoRelPath, specifier: &str) -> Result<Resolved, Self::Error>;
}

/// Everything a file-local rule needs to reach a verdict.
#[derive(Debug, Clone, Copy)]
pub struct FileContext<'a> {
    /// The file under test.
    pub path: &'a RepoRelPath,
    /// Its facts, once a parser has run. `None` on a walk-only pass, which is
    /// what a structure-only configuration does.
    pub facts: Option<&'a FileFacts>,
    /// Its document facts, for the rules that read a `.md` rather than code.
    ///
    /// A second field rather than an enum, because the two are produced by
    /// different front-ends from different files and no rule wants both. A
    /// rule declares which it reads through
    /// [`needs_facts`](RuleEngine::needs_facts), so the one it did not ask for
    /// being `None` is never a surprise it has to handle.
    pub docs: Option<&'a crate::docs::DocFacts>,
    /// Names of the entries sitting beside it, for sibling checks. Supplied by
    /// the walk so a rule never touches the filesystem itself -- which is what
    /// keeps rules deterministic and cheap to test.
    pub siblings: &'a [String],
    /// Whether a path anywhere in the repository exists.
    ///
    /// `siblings` answers for the directory the file is in, and a rule whose
    /// companion may sit outside it -- `../projeto.md` -- has nothing to ask.
    /// Supplied by the caller for the same reason `siblings` is: `check`
    /// answers from the walk it already has, `check --file` answers from disk
    /// because it has no walk, and a rule still never touches the filesystem
    /// itself.
    pub exists: Exists<'a>,
    /// Who imports whom, for the rules that ask about more than one file.
    ///
    /// `None` for every rule that did not ask, which is almost all of them:
    /// the graph costs a resolution pass over the whole repository, so it is
    /// built only when [`needs_graph`](RuleEngine::needs_graph) says a rule
    /// reads it.
    ///
    /// A rule that *did* ask is never handed `None`. That is the invariant the
    /// runner keeps rather than a shape the type enforces, and it is the
    /// reason the runner holds such rules back from the main loop instead of
    /// offering them a graph it has not built yet: a cycle rule handed an
    /// empty graph reports nothing, and nothing is exactly what a repository
    /// with no cycles reports. `docs/CONFIG.md` calls that the worst failure a
    /// linter has. A driver that cannot build a graph — `check --file`, which
    /// sees one file — refuses such a rule and says so, rather than letting it
    /// pass quietly.
    pub graph: Option<&'a crate::graph::ImportGraph>,
    /// The day this run is answering for.
    ///
    /// Supplied by the caller like `siblings` and `exists`, and for the same
    /// reason: a rule never reads a clock, so two machines given the same date
    /// give the same answer. That is the determinism decision 28 defended when
    /// it refused to read `git`, kept while adding the one question that needs
    /// to know what day it is.
    ///
    /// `check` defaults it to today in UTC and `--as-of` pins it. A surface
    /// with no run behind it passes [`Date::EPOCH`](crate::date::Date::EPOCH),
    /// which is a real date rather than a placeholder — nothing has to handle
    /// an absent one. Issue #117.
    pub as_of: crate::date::Date,
}

/// Which facts a rule needs read out of a file, if any.
///
/// `bool` was enough while one front-end existed. With two, "needs facts" is
/// ambiguous in the one place it matters: whether an absent fact is an answer
/// somebody lost. A boundary rule pointed at a `.md` wanted *code* facts from a
/// file that could never have them, and counting that as a missed check would
/// pin `checks_skipped` above zero in every repository that keeps documentation
/// beside its code. See [`FileClass::yields`](crate::path::FileClass::yields).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FactsNeeded {
    /// Nothing is read; the rule reasons about names and paths.
    Nothing,
    /// Imports, exports and calls — what a JS/TS front-end produces.
    Code,
    /// Frontmatter and headings — what a document front-end produces.
    Document,
}

/// Whether a path exists, asked of whoever is driving the check.
///
/// A closure rather than a listing, because the two callers know it two
/// different ways and neither can hand over the other's.
#[derive(Clone, Copy)]
pub struct Exists<'a>(&'a dyn Fn(&RepoRelPath) -> bool);

impl<'a> Exists<'a> {
    /// Wraps a predicate.
    #[must_use]
    pub fn new(predicate: &'a dyn Fn(&RepoRelPath) -> bool) -> Self {
        Self(predicate)
    }

    /// Whether `path` is in the repository.
    #[must_use]
    pub fn at(&self, path: &RepoRelPath) -> bool {
        (self.0)(path)
    }
}

impl Exists<'static> {
    /// A repository holding nothing, for a caller with no answer to give.
    ///
    /// Used by tests and by any driver that cannot see beyond the file it was
    /// handed. A rule asking about a path it gets `false` for reports the path
    /// as missing, which is the honest reading of "I cannot see it".
    #[must_use]
    pub fn none() -> Self {
        Self(&|_| false)
    }
}

impl std::fmt::Debug for Exists<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Exists(..)")
    }
}

/// Everything a directory-local rule needs to reach a verdict.
///
/// Separate from [`FileContext`] because two of the five rule kinds ask about
/// a *directory* -- which folders may exist here, does every file here have a
/// spec sibling -- and handing them a file would mean each one recovering the
/// directory and its contents for itself.
#[derive(Debug, Clone, Copy)]
pub struct DirectoryContext<'a> {
    /// The directory under test.
    pub path: &'a RepoRelPath,
    /// Names of the directories immediately inside, sorted.
    pub subdirectories: &'a [String],
    /// Names of the files immediately inside, sorted.
    pub files: &'a [String],
}

/// One rule, evaluated against a directory or a file.
///
/// The two halves of this trait are the point of it. The `check_*` methods are
/// the gate; `describe_expectation` is the informant that `scaffold` and
/// `agent-guide` are built from. Requiring both on the same trait means a rule
/// whose expectation cannot be described does not compile, so the informant
/// can never drift from what the checker actually enforces. See decision 9.
///
/// Both `check_*` methods default to reporting nothing, so a rule implements
/// only the one its category is about. A rule that implements neither is
/// inert, which `config doctor` can and should notice.
pub trait RuleEngine: Send + Sync {
    /// This rule's stable identifier.
    fn id(&self) -> &RuleId;

    /// The module this rule was declared under, if any.
    fn module(&self) -> Option<&ModuleId>;

    /// The severity of findings this rule produces.
    fn level(&self) -> Level;

    /// Whether the rule has anything to say about a file at `path`.
    ///
    /// Must be purely lexical: `describe` and the pre-write hook call this for
    /// files that do not exist yet.
    fn applies_to(&self, path: &RepoRelPath) -> bool;

    /// Which facts this rule reads out of a file, if any.
    ///
    /// The runner uses this to decide what to parse. A structure-only
    /// configuration should never open a source file, and on a large
    /// repository that is the difference between a walk and thirty thousand
    /// reads.
    ///
    /// It also decides whether a missing fact was an answer somebody lost:
    /// paired with the file's class, "wanted code facts from a `.py`" is a
    /// counted skip and "wanted code facts from a `.md`" is not.
    fn needs_facts(&self) -> FactsNeeded {
        FactsNeeded::Nothing
    }

    /// Whether this rule reads *where a file's imports land*.
    ///
    /// Separate from [`needs_facts`](Self::needs_facts) because resolution is
    /// a second cost on top of parsing: it probes the filesystem for every
    /// specifier in every file it applies to. A naming rule reads inside a
    /// file and never asks where its imports go, and should not pay for it.
    fn needs_resolution(&self) -> bool {
        false
    }

    /// Whether this rule reads the *whole repository's* import graph.
    ///
    /// A third question rather than a stronger `needs_resolution`, because the
    /// two are paid at different times and in different amounts. A boundary
    /// rule wants every specifier of *its own* file placed, once per file it
    /// covers. A cycle rule wants the edges of every file at once — including
    /// files no rule's scope reaches, because a loop that leaves the scope and
    /// comes back is still a loop, and one built from a partial graph would be
    /// invisible.
    ///
    /// So this is the expensive answer: `true` here makes the run parse and
    /// resolve every source file in the repository, whatever any scope says.
    /// Measured on the 10,000-file benchmark, resolution is about three
    /// quarters of a warm run, so a configuration that turns this on is
    /// roughly four times the cost of one that does not. It buys the only
    /// answer there is to "is there a loop here?", and a configuration with no
    /// such rule pays none of it.
    fn needs_graph(&self) -> bool {
        false
    }

    /// Whether this rule's findings are about a directory rather than about
    /// the files in it.
    ///
    /// Asked by `config doctor`, which reports a rule that reaches no file as
    /// idle — a good question for a rule about files and a meaningless one for
    /// a rule about directories, where reaching no file is the ordinary state.
    ///
    /// A method rather than a match on rule kinds, because the match was the
    /// bug: `structure` was exempt by name, `presence` arrived later answering
    /// the same way and was not, and `doctor` called every `presence` rule idle
    /// while `check` was firing it. Whoever writes the third one has to answer
    /// this, and the compiler will not remind them — but a default of `false`
    /// is the safe way to be wrong, since it only ever costs a concern that can
    /// be read and dismissed.
    fn answers_for_directories(&self) -> bool {
        false
    }

    /// Evaluates the rule against one directory.
    fn check_directory(&self, ctx: DirectoryContext<'_>) -> Vec<Finding> {
        let _ = ctx;
        Vec::new()
    }

    /// Evaluates the rule against one file.
    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        let _ = ctx;
        Vec::new()
    }

    /// What the rule requires of `path`, whether or not the file exists.
    ///
    /// Called by `scaffold` and `agent-guide`. Returns empty when the rule
    /// does not apply.
    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{ExportKind, ExportTags, KindFilter};

    /// A rule that requires one named export, standing in for the real
    /// `naming` engine so the trait's shape can be exercised before that
    /// engine exists.
    struct RequiresExport {
        id: RuleId,
        name: String,
    }

    impl RuleEngine for RequiresExport {
        fn id(&self) -> &RuleId {
            &self.id
        }

        fn module(&self) -> Option<&ModuleId> {
            None
        }

        fn level(&self) -> Level {
            Level::Error
        }

        fn applies_to(&self, path: &RepoRelPath) -> bool {
            path.as_str().ends_with(".use-case.ts")
        }

        fn needs_facts(&self) -> FactsNeeded {
            FactsNeeded::Code
        }

        fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
            if !self.applies_to(ctx.path) {
                return Vec::new();
            }
            if ctx
                .facts
                .is_some_and(|facts| facts.named_export(&self.name).is_some())
            {
                return Vec::new();
            }
            vec![Finding {
                rule_id: self.id.clone(),
                module_id: None,
                level: self.level(),
                path: ctx.path.clone(),
                span: None,
                observed: crate::finding::Observed::ExportMissing {
                    name: self.name.clone(),
                },
                expected: self.expectation(),
            }]
        }

        fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
            if self.applies_to(path) {
                vec![self.expectation()]
            } else {
                Vec::new()
            }
        }
    }

    impl RequiresExport {
        fn expectation(&self) -> Expectation {
            Expectation::RequiredExport {
                kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
                name: self.name.clone(),
                annotation: Vec::new(),
                signature_hint: None,
            }
        }
    }

    fn engine() -> RequiresExport {
        RequiresExport {
            id: RuleId::new("usecase-factory-name").expect("valid"),
            name: "Foo".to_owned(),
        }
    }

    fn facts_for(path: &str) -> FileFacts {
        FileFacts::unparsed(
            RepoRelPath::new(path).expect("valid"),
            ContentHash::of(b"source"),
        )
    }

    /// The trait is object-safe in practice: the engine list is heterogeneous,
    /// so it has to be storable behind a trait object.
    #[test]
    fn rule_engines_are_usable_as_trait_objects() {
        let engines: Vec<Box<dyn RuleEngine>> = vec![Box::new(engine())];
        assert_eq!(engines.len(), 1);
        assert_eq!(engines[0].id().as_str(), "usecase-factory-name");
        assert_eq!(engines[0].level(), Level::Error);
        assert_eq!(engines[0].module(), None);
        assert_eq!(
            engines[0].needs_facts(),
            FactsNeeded::Code,
            "it reads exports"
        );
        assert!(
            !engines[0].needs_resolution(),
            "but it never asks where an import goes"
        );
        assert!(
            !engines[0].needs_graph(),
            "and it certainly does not want the whole repository's edges"
        );
    }

    /// A rule that reads the graph is handed it, and one that does not is not.
    ///
    /// The `Option` is not a convenience: a run that cannot build a graph must
    /// not hand a rule an empty one, because a cycle rule over an empty graph
    /// reports silence, and silence is indistinguishable from a repository
    /// with no cycles. The runner keeps such a rule out of the loop entirely;
    /// this is the shape that lets it.
    #[test]
    fn a_context_carries_the_graph_for_the_rules_that_read_it() {
        let facts = facts_for("packages/app/src/a.ts");
        let graph = crate::graph::ImportGraph::of(
            [crate::graph::FileEdges {
                from: facts.path.clone(),
                to: vec![crate::graph::Edge {
                    to: RepoRelPath::new("packages/app/src/b.ts").expect("valid"),
                    type_only: false,
                }],
            }]
            .into_iter(),
        );

        let ctx = FileContext {
            path: &facts.path,
            facts: Some(&facts),
            docs: None,
            siblings: &[],
            exists: Exists::none(),
            graph: Some(&graph),
            as_of: crate::date::Date::EPOCH,
        };

        assert!(
            ctx.graph
                .expect("the rule asked for it")
                .reaches(
                    &facts.path,
                    &|p| p.as_str() == "packages/app/src/b.ts",
                    true
                )
                .is_none(),
            "a direct import is not transitive reach, and the context is what \
             lets a rule ask at all"
        );
    }

    /// `applies_to` answers for a path with no file behind it, which is what
    /// `describe` and the pre-write hook depend on.
    #[test]
    fn applicability_is_answerable_for_a_file_that_does_not_exist() {
        let engine = engine();
        let hypothetical =
            RepoRelPath::new("packages/app/src/nope/foo.use-case.ts").expect("valid");

        assert!(engine.applies_to(&hypothetical));
        assert!(!engine.applies_to(&RepoRelPath::new("packages/app/src/foo.ts").expect("valid")));
    }

    /// The invariant decision 9 exists to protect: whatever `check` demands is
    /// exactly what `describe_expectation` advertises. If these two could
    /// diverge, `scaffold` would tell an agent to write something the gate
    /// then rejects.
    #[test]
    fn what_check_demands_is_what_describe_expectation_advertises() {
        let engine = engine();
        let facts = facts_for("packages/app/src/foo/foo.use-case.ts");
        let findings = engine.check_file(FileContext {
            path: &facts.path,
            facts: Some(&facts),
            docs: None,
            siblings: &[],
            exists: Exists::none(),
            graph: None,
            as_of: crate::date::Date::EPOCH,
        });

        let demanded = &findings
            .first()
            .expect("file has no exports, so it fails")
            .expected;
        let advertised = engine.describe_expectation(&facts.path);

        assert_eq!(advertised.len(), 1);
        assert_eq!(advertised.first(), Some(demanded));
    }

    /// The predicate a caller supplies, asked and answered.
    ///
    /// Covered here rather than left to the crates that use it: this crate's
    /// gate is 100% of functions, and a seam whose own crate never exercises it
    /// is one whose contract nobody stated.
    #[test]
    fn an_existence_predicate_answers_for_the_path_it_is_given() {
        let there = RepoRelPath::new("packages/domain/src/user.ts").expect("valid");
        let predicate = |candidate: &RepoRelPath| candidate == &there;
        let exists = Exists::new(&predicate);

        assert!(exists.at(&there));
        assert!(!exists.at(&RepoRelPath::new("packages/domain/src/order.ts").expect("valid")));
    }

    /// A repository holding nothing, for a caller with no answer to give. A
    /// rule asking about a path it gets `false` for reports the path as
    /// missing, which is the honest reading of "I cannot see it".
    #[test]
    fn a_caller_with_no_answer_says_nothing_is_there() {
        assert!(!Exists::none().at(&RepoRelPath::new("anything.ts").expect("valid")));
    }

    /// It is `Copy` and it is `Debug`, because it rides inside a context that
    /// is both, and a manual `Debug` that panicked or printed a pointer would
    /// be found by whoever debugged a rule at three in the morning.
    #[test]
    fn the_predicate_can_be_copied_and_printed() {
        let exists = Exists::none();
        let copy = exists;

        assert!(!copy.at(&RepoRelPath::new("anything.ts").expect("valid")));
        assert_eq!(format!("{exists:?}"), "Exists(..)");
    }

    /// The path every file in a clean repository takes: the rule applies, the
    /// file satisfies it, and nothing is reported.
    #[test]
    fn a_satisfied_rule_reports_nothing() {
        let engine = engine();
        let mut facts = facts_for("packages/app/src/foo/foo.use-case.ts");
        facts.exports.push(crate::facts::ExportFact {
            name: Some("Foo".to_owned()),
            tags: ExportTags::only(ExportKind::Function),
            visibility: crate::facts::Visibility::Public,
            is_default: false,
            reexport_from: None,
            forwards: None,
            annotations: Vec::new(),
            returns: None,
            span: crate::facts::Span::new(0, 10),
        });

        assert!(engine.applies_to(&facts.path));
        assert!(
            engine
                .check_file(FileContext {
                    path: &facts.path,
                    facts: Some(&facts),
                    docs: None,
                    siblings: &[],
                    exists: Exists::none(),
                    graph: None,
                    as_of: crate::date::Date::EPOCH,
                })
                .is_empty()
        );
    }

    /// A directory-oriented rule, standing in for the real `structure` engine.
    /// Its purpose here is to be the mirror of `RequiresExport`: one
    /// implements `check_file` and the other `check_directory`, so both
    /// defaults get exercised.
    struct ForbidsSubfolder {
        id: RuleId,
    }

    impl RuleEngine for ForbidsSubfolder {
        fn id(&self) -> &RuleId {
            &self.id
        }
        fn module(&self) -> Option<&ModuleId> {
            None
        }
        fn level(&self) -> Level {
            Level::Error
        }
        fn applies_to(&self, _path: &RepoRelPath) -> bool {
            true
        }

        fn check_directory(&self, ctx: DirectoryContext<'_>) -> Vec<Finding> {
            ctx.subdirectories
                .iter()
                .filter(|name| name.as_str() == "forbidden")
                .filter_map(|name| {
                    Some(Finding {
                        rule_id: self.id.clone(),
                        module_id: None,
                        level: Level::Error,
                        path: ctx.path.join(name).ok()?,
                        span: None,
                        observed: crate::finding::Observed::UnexpectedSubfolder {
                            name: name.clone(),
                        },
                        expected: Expectation::AllowedSubfolders {
                            allowed: Vec::new(),
                            warn: Vec::new(),
                            patterns: Vec::new(),
                        },
                    })
                })
                .collect()
        }

        fn describe_expectation(&self, _path: &RepoRelPath) -> Vec<Expectation> {
            vec![Expectation::AllowedSubfolders {
                allowed: Vec::new(),
                warn: Vec::new(),
                patterns: Vec::new(),
            }]
        }
    }

    /// A rule implements only the `check_*` method its category is about. The
    /// other defaults to reporting nothing, so neither kind has to guess about
    /// the shape it was not written for.
    #[test]
    fn the_unimplemented_half_of_the_trait_reports_nothing() {
        let path = RepoRelPath::new("packages/app/src/foo").expect("valid");
        let subdirectories = ["forbidden".to_owned()];
        let files = ["anything.ts".to_owned()];

        // A file-oriented rule, asked about a directory.
        assert!(
            engine()
                .check_directory(DirectoryContext {
                    path: &path,
                    subdirectories: &subdirectories,
                    files: &files,
                })
                .is_empty()
        );

        // A directory-oriented rule, asked about a file.
        let directory_rule = ForbidsSubfolder {
            id: RuleId::new("no-forbidden-folder").expect("valid"),
        };
        let facts = facts_for("packages/app/src/foo/bar.ts");
        assert!(
            directory_rule
                .check_file(FileContext {
                    path: &facts.path,
                    facts: Some(&facts),
                    docs: None,
                    siblings: &[],
                    exists: Exists::none(),
                    graph: None,
                    as_of: crate::date::Date::EPOCH,
                })
                .is_empty()
        );

        // And it does report when asked about what it *is* written for, so the
        // assertions above are not passing because the rule is inert.
        let reported = directory_rule.check_directory(DirectoryContext {
            path: &path,
            subdirectories: &subdirectories,
            files: &files,
        });
        assert_eq!(reported.len(), 1);
        assert_eq!(
            reported.first().expect("one").path.as_str(),
            "packages/app/src/foo/forbidden"
        );

        // The rest of the trait answers too. A double whose identity methods
        // are never called is a double nobody has checked is well formed.
        assert_eq!(directory_rule.id().as_str(), "no-forbidden-folder");
        assert_eq!(directory_rule.module(), None);
        assert_eq!(directory_rule.level(), Level::Error);
        assert!(directory_rule.applies_to(&path));
        assert_eq!(directory_rule.describe_expectation(&path).len(), 1);
        assert_eq!(
            directory_rule.needs_facts(),
            FactsNeeded::Nothing,
            "a directory rule reads names, not contents"
        );
        assert!(!directory_rule.needs_resolution());
        assert!(!directory_rule.needs_graph());

        // The default, which is the safe way to be wrong: a rule that does not
        // answer this is treated as a rule about files, so `config doctor` may
        // report a concern that can be read and dismissed rather than staying
        // quiet about one that matters. The two rule kinds whose findings are
        // about directories override it, and `doctor` calling every `presence`
        // rule idle is what happened while that question was a match on names.
        assert!(
            !directory_rule.answers_for_directories(),
            "the default is `false`, whatever this double checks"
        );
    }

    #[test]
    fn a_rule_says_nothing_about_a_file_outside_its_scope() {
        let engine = engine();
        let facts = facts_for("packages/app/src/helper.ts");

        assert!(
            engine
                .check_file(FileContext {
                    path: &facts.path,
                    facts: Some(&facts),
                    docs: None,
                    siblings: &[],
                    exists: Exists::none(),
                    graph: None,
                    as_of: crate::date::Date::EPOCH,
                })
                .is_empty()
        );
        assert!(engine.describe_expectation(&facts.path).is_empty());
    }

    /// Siblings arrive from the walk rather than being read by the rule, so a
    /// rule stays deterministic and testable without a filesystem.
    #[test]
    fn a_rule_receives_its_siblings_rather_than_reading_the_disk() {
        let facts = facts_for("packages/app/src/foo/foo.use-case.ts");
        let siblings = [
            "foo.use-case.ts".to_owned(),
            "foo.use-case.spec.ts".to_owned(),
        ];
        let ctx = FileContext {
            path: &facts.path,
            facts: Some(&facts),
            docs: None,
            siblings: &siblings,
            exists: Exists::none(),
            graph: None,
            as_of: crate::date::Date::EPOCH,
        };

        assert_eq!(ctx.siblings.len(), 2);
        assert!(ctx.siblings.iter().any(|s| s.ends_with(".spec.ts")));
    }
}
