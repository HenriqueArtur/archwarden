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
    /// Names of the entries sitting beside it, for sibling checks. Supplied by
    /// the walk so a rule never touches the filesystem itself -- which is what
    /// keeps rules deterministic and cheap to test.
    pub siblings: &'a [String],
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
            siblings: &[],
        });

        let demanded = &findings
            .first()
            .expect("file has no exports, so it fails")
            .expected;
        let advertised = engine.describe_expectation(&facts.path);

        assert_eq!(advertised.len(), 1);
        assert_eq!(advertised.first(), Some(demanded));
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
            is_default: false,
            reexport_from: None,
            span: crate::facts::Span::new(0, 10),
        });

        assert!(engine.applies_to(&facts.path));
        assert!(
            engine
                .check_file(FileContext {
                    path: &facts.path,
                    facts: Some(&facts),
                    siblings: &[]
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
                        },
                    })
                })
                .collect()
        }

        fn describe_expectation(&self, _path: &RepoRelPath) -> Vec<Expectation> {
            vec![Expectation::AllowedSubfolders {
                allowed: Vec::new(),
                warn: Vec::new(),
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
                    siblings: &[],
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
                    siblings: &[]
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
            siblings: &siblings,
        };

        assert_eq!(ctx.siblings.len(), 2);
        assert!(ctx.siblings.iter().any(|s| s.ends_with(".spec.ts")));
    }
}
