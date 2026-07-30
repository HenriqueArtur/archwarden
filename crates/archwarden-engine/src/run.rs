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
    traits::{FileContext, Parser as _},
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
    /// Checks that could not be made: one per rule that wanted a file whose
    /// facts were unavailable.
    ///
    /// Counted in *checks* rather than files, because one unreadable file that
    /// three rules wanted is three answers nobody got. `unreadable_files` names
    /// the file; this says how much was not decided because of it. Without the
    /// number, a report over a repository where nothing could be parsed reads
    /// as a repository with nothing wrong — the failure mode `check --file`
    /// already refuses through `skipped` (correction C6), and the full run
    /// used to allow.
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
    /// Where the imports went, by kind.
    ///
    /// All zero when no rule needed resolution. The `unresolved` count is the
    /// one that matters to a reader: a boundary rule cannot see an import it
    /// could not place, so a clean report over a repository whose dependencies
    /// are not installed means less than it looks like.
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
        .any(|engine| engine.needs_facts())
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

/// Runs every rule against the walked tree.
///
/// A configuration whose rules are all structural never reads a byte, cache or
/// no cache.
#[must_use]
pub fn check(run: Run<'_>) -> Report {
    let Run {
        root,
        config,
        tree,
        mut cache,
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

    // Built once per run, not once per file: `oxc_resolver` caches
    // `tsconfig` and `package.json` reads internally, and a fresh resolver
    // per file would throw that away thousands of times.
    let resolver = engines
        .iter()
        .any(|engine| engine.needs_resolution())
        .then(|| archwarden_resolver::imports::ImportResolver::new(root));

    for (path, directory) in tree.directories() {
        let file_names = directory.file_names();
        files_scanned += file_names.len();

        for engine in &engines {
            findings.extend(
                engine.check_directory(archwarden_core::traits::DirectoryContext {
                    path,
                    subdirectories: &directory.subdirectories,
                    files: &file_names,
                }),
            );
        }

        for file in &directory.files {
            let wanted_by: Vec<_> = engines
                .iter()
                .filter(|engine| engine.applies_to(&file.path))
                .collect();
            if wanted_by.is_empty() {
                continue;
            }

            // Read the file only if a rule that applies to it actually looks
            // inside. Deciding this per file rather than per run is what keeps
            // a mostly-structural configuration off the disk.
            let mut facts = if file.class == FileClass::Source
                && wanted_by.iter().any(|engine| engine.needs_facts())
            {
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
            if let (Some(resolver), Some(facts)) = (resolver.as_ref(), facts.as_mut()) {
                imports.absorb(crate::resolve::resolve_imports(resolver, facts));
            }

            for engine in wanted_by {
                // A rule that reads inside a file, handed no facts, cannot
                // decide anything -- it returns nothing, which is
                // indistinguishable from deciding the file is fine. Counted so
                // the report can say the difference out loud.
                if engine.needs_facts() && facts.is_none() {
                    checks_skipped += 1;
                    skipped_checks.push((engine.id().to_string(), file.path.clone()));
                }
                findings.extend(engine.check_file(FileContext {
                    path: &file.path,
                    facts: facts.as_ref(),
                    siblings: &file_names,
                }));
            }
        }
    }

    // Determinism is a design goal: the same inputs must produce byte-identical
    // output, or every snapshot test and CI diff becomes noise.
    findings.sort();
    unreadable_files.sort();

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
        if let Some(facts) = cache.facts(content) {
            return Ok((facts, Source::Cache));
        }

        let facts = parse(path, &source, content)?;
        cache.put_facts(content, &facts);
        return Ok((facts, Source::Parsed));
    }

    Ok((parse(path, &source, content)?, Source::Parsed))
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

fn parse(path: &RepoRelPath, source: &str, content: ContentHash) -> Result<FileFacts, String> {
    archwarden_parser::oxc::OxcParser
        .parse(path, source, content)
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
            allowed_subfolders: allowed.iter().map(|s| (*s).to_owned()).collect(),
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            filename_patterns: Vec::new(),
        }
    }

    fn boundary(forbid: &[&str], require: &[&str], except: &[&str]) -> CompiledRuleKind {
        let set = |patterns: &[&str]| {
            PathSet::compile(patterns.iter().map(|p| (*p).to_owned())).expect("valid globs")
        };
        CompiledRuleKind::ImportBoundary {
            forbid: set(forbid),
            require: set(require),
            except: set(except),
            include_type_only: true,
        }
    }

    fn naming() -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: archwarden_core::pattern::Pattern::compile(
                r"^(?<name>[a-z0-9-]+)\.use-case\.ts$",
            )
            .expect("valid pattern"),
            name_template: "{{pascal(name)}}".to_owned(),
            kind: archwarden_core::facts::KindFilter::OneOf(
                archwarden_core::facts::ExportTags::only(
                    archwarden_core::facts::ExportKind::Function,
                ),
            ),
            signature_hint: None,
        }
    }

    fn spec_pair() -> CompiledRuleKind {
        CompiledRuleKind::SpecPair {
            subfolders: vec![".".to_owned()],
            spec_markers: vec!["spec".to_owned(), "test".to_owned()],
            ignore_files: PathSet::default(),
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
    }

    /// `FileFacts` come from a TypeScript parser, so only a source file may
    /// ever be handed to one. A `.json` sitting in a folder a facts-needing
    /// rule governs is the case that would break it.
    ///
    /// Every engine today also refuses non-source files on its own, so the
    /// runner's guard is defence in depth and `cargo-mutants` cannot kill it.
    /// It stays because the invariant belongs at the one place that calls the
    /// parser, not spread across every rule that will ever exist. See M4 in
    /// `docs/PLAN-V0.md`.
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
        });
        drop(guard);

        assert!(report.skipped_checks.is_empty());
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
}
