//! Checking one file, for a pre-write hook.
//!
//! Layer 4 of `AGENT-INTEGRATION.md`: a harness intercepts a write and asks
//! whether it would be legal. The answer has to arrive in the time a keystroke
//! can wait, so this reads the file and the directories on the way to it,
//! rather than walking the repository.
//!
//! # What it sees
//!
//! Everything the full run would say *about this path*. Two of the five rule
//! kinds report through `check_directory` -- a forbidden folder and a missing
//! spec are both facts about a directory's contents -- so those run too,
//! against one listing per ancestor, with the write the hook is asking about
//! folded in. Their findings are then filtered to the ones on this path's own
//! ancestry: an agent writing one file should not be handed its neighbour's
//! problems.
//!
//! `AGENT-INTEGRATION.md:180` expected boundary rules to be skipped on a cold
//! cache. They are not, and cannot be: a boundary rule is file-local once its
//! imports are resolved, and resolving them costs a handful of filesystem
//! probes rather than cross-file state. Correction C13 in `docs/PLAN-V0.md`
//! carries the measurement.
//!
//! What genuinely cannot run is a rule that reads a file this command could
//! not. That is reported, never dropped: "no findings" and "not checked" are
//! different answers, and a hook that conflated them would pass an
//! unparsable file. Correction C6.

use archwarden_core::{
    compiled::CompiledConfig,
    finding::Finding,
    path::{FileClass, RepoRelPath},
    traits::{DirectoryContext, FileContext, RuleEngine},
};
use camino::Utf8Path;

/// What checking one file found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Single {
    /// The file that was checked.
    pub path: RepoRelPath,
    /// Every finding about it, worst-first.
    pub findings: Vec<Finding>,
    /// Rules that apply here but could not be evaluated, and why.
    ///
    /// Never empty by accident. A silent skip would make the same write pass
    /// or fail depending on what the run happened to have available, which is
    /// exactly the determinism `ARCHITECTURE.md` is about. Correction C6.
    pub skipped: Vec<Skipped>,
}

/// One rule that applies but was not evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Which rule.
    pub rule_id: String,
    /// Why, as a stable slug a caller can branch on.
    pub reason: Reason,
}

/// Why a rule could not be evaluated for a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reason {
    /// The file could not be read or parsed, so no rule that looks inside it
    /// could run.
    Unreadable,
    /// The file is not TypeScript or JavaScript, so there are no facts to
    /// read from it.
    ///
    /// Distinct from [`Unreadable`](Self::Unreadable) because the fixes are
    /// opposite: one means the file is broken, the other means the rule is
    /// pointed at something it cannot be about.
    NotSource,
}

impl Reason {
    /// The stable slug, for JSON and for a caller branching on it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unreadable => "unreadable",
            Self::NotSource => "not-source",
        }
    }

    /// One sentence for a human.
    #[must_use]
    pub fn explain(self) -> &'static str {
        match self {
            Self::Unreadable => "the file could not be read",
            Self::NotSource => "it is not a TypeScript or JavaScript file",
        }
    }
}

/// Checks one file against every rule that applies to it.
///
/// `root` is the repository root; `path` is repository-relative and may not
/// exist, in which case rules that need its contents are reported as skipped
/// rather than passing quietly.
#[must_use]
pub fn check_file(root: &Utf8Path, config: &CompiledConfig, path: &RepoRelPath) -> Single {
    let engines = archwarden_rules::engines_for(config);
    let mut findings = Vec::new();
    let mut skipped = Vec::new();

    // An ignored path is not checked at all, exactly as in a full run. A hook
    // that enforced rules `check` would not is worse than no hook.
    if config.is_ignored(path) {
        return Single {
            path: path.clone(),
            findings,
            skipped,
        };
    }

    findings.extend(directory_findings(root, &engines, path));

    let applicable: Vec<&Box<dyn RuleEngine>> = engines
        .iter()
        .filter(|engine| engine.applies_to(path))
        .collect();

    let needs_facts = applicable.iter().any(|engine| engine.needs_facts());
    let is_source = path
        .file_name()
        .is_some_and(|name| FileClass::of(name) == FileClass::Source);

    // `is_source` is defence in depth, and `cargo-mutants` cannot kill it: the
    // parser refuses a non-source extension anyway, so dropping the guard
    // reaches the same answer through a wasted read. It stays because reading
    // a file to be told it is the wrong kind is work with no reason, and on a
    // binary that happened to match a `file_pattern` it is a large one. Second
    // instance of this shape -- see M4 in `docs/PLAN-V0.md` for the first.
    let facts = if needs_facts && is_source {
        match crate::run::facts_of(root, path) {
            Ok(mut facts) => {
                // Resolution is what a boundary rule needs, and it is a
                // handful of filesystem probes rather than the cross-file
                // state the docs expected. Paid only when a rule asks.
                if applicable.iter().any(|engine| engine.needs_resolution()) {
                    let resolver = archwarden_resolver::imports::ImportResolver::new(root);
                    let _ = crate::resolve::resolve_imports(&resolver, &mut facts);
                }
                Some(facts)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    let siblings = listing(root, path.parent().as_ref(), path.file_name());
    for engine in applicable {
        if engine.needs_facts() && facts.is_none() {
            skipped.push(Skipped {
                rule_id: engine.id().to_string(),
                reason: if is_source {
                    Reason::Unreadable
                } else {
                    Reason::NotSource
                },
            });
            continue;
        }
        findings.extend(engine.check_file(FileContext {
            path,
            facts: facts.as_ref(),
            siblings: &siblings,
        }));
    }

    findings.sort();
    findings.dedup();
    skipped.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));

    Single {
        path: path.clone(),
        findings,
        skipped,
    }
}

/// What the directory rules say about the path this write would create.
///
/// One listing per ancestor, with the write folded in: at the moment a hook
/// asks, neither the file nor the folders leading to it necessarily exist, and
/// a check against the tree as it stands would miss exactly the thing the hook
/// is for.
///
/// Findings are kept only when they are about this path or a directory on the
/// way to it. A neighbour's missing spec is a real finding and `check` will
/// report it; handing it to an agent writing an unrelated file is noise.
fn directory_findings(
    root: &Utf8Path,
    engines: &[Box<dyn RuleEngine>],
    path: &RepoRelPath,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (directory, next) in ancestors(path) {
        let (files, subdirectories) = match &next {
            // The next component is the file itself.
            Component::File(name) => (
                listing(root, Some(&directory), Some(name.as_str())),
                subdirectories(root, &directory, None),
            ),
            // The next component is a directory on the way down, which the
            // write may be about to create.
            Component::Directory(name) => (
                listing(root, Some(&directory), None),
                subdirectories(root, &directory, Some(name.as_str())),
            ),
        };

        for engine in engines {
            findings.extend(engine.check_directory(DirectoryContext {
                path: &directory,
                subdirectories: &subdirectories,
                files: &files,
            }));
        }
    }

    findings.retain(|finding| concerns(&finding.path, path));
    findings
}

/// The next component of `path` below a directory: either the file itself, or
/// a directory on the way to it.
enum Component {
    /// The target file's own name.
    File(String),
    /// A directory between here and the target.
    Directory(String),
}

/// Every directory from the repository root down to the file's parent, each
/// paired with the component of `path` that comes next.
fn ancestors(path: &RepoRelPath) -> Vec<(RepoRelPath, Component)> {
    let components: Vec<&str> = path
        .as_str()
        .split('/')
        .filter(|component| !component.is_empty())
        .collect();

    let mut pairs = Vec::new();
    let mut here = RepoRelPath::root();

    for (index, component) in components.iter().enumerate() {
        let last = index + 1 == components.len();
        let next = if last {
            Component::File((*component).to_owned())
        } else {
            Component::Directory((*component).to_owned())
        };
        pairs.push((here.clone(), next));

        if last {
            break;
        }
        let Ok(deeper) = here.join(component) else {
            break;
        };
        here = deeper;
    }

    pairs
}

/// A directory's file names, with `include` folded in whether or not it exists.
fn listing(root: &Utf8Path, directory: Option<&RepoRelPath>, include: Option<&str>) -> Vec<String> {
    let mut names = entries(root, directory, |kind| kind.is_file());
    if let Some(name) = include
        && !names.iter().any(|existing| existing == name)
    {
        names.push(name.to_owned());
    }
    names.sort();
    names
}

/// A directory's subdirectory names, with `include` folded in the same way.
fn subdirectories(root: &Utf8Path, directory: &RepoRelPath, include: Option<&str>) -> Vec<String> {
    let mut names = entries(root, Some(directory), |kind| kind.is_dir());
    if let Some(name) = include
        && !names.iter().any(|existing| existing == name)
    {
        names.push(name.to_owned());
    }
    names.sort();
    names
}

fn entries(
    root: &Utf8Path,
    directory: Option<&RepoRelPath>,
    keep: impl Fn(std::fs::FileType) -> bool,
) -> Vec<String> {
    let Some(directory) = directory else {
        return Vec::new();
    };
    // A directory that does not exist yet lists nothing, which is the honest
    // answer for a folder the write is about to create.
    let Ok(entries) = std::fs::read_dir(root.join(directory.as_path())) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(&keep))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

/// Whether a finding about `reported` is about `target` or a directory on the
/// way to it.
fn concerns(reported: &RepoRelPath, target: &RepoRelPath) -> bool {
    reported == target || reported.is_root() || target.as_str().starts_with(&format!("{reported}/"))
}

impl Single {
    /// Whether this file's findings should block a write.
    #[must_use]
    pub fn fails_build(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.level.fails_build())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::{CompiledRule, CompiledRuleKind, SkipDirs},
        facts::{ExportKind, ExportTags, KindFilter},
        glob::PathSet,
        hash::ContentHash,
        ids::RuleId,
        level::Level,
        pattern::Pattern,
        scope::Scope,
    };
    use camino::Utf8PathBuf;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn tree_at(entries: &[(&str, &str)]) -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");

        for (relative, contents) in entries {
            let file = root.join(relative);
            std::fs::create_dir_all(file.parent().expect("a file has a parent"))
                .expect("create dirs");
            std::fs::write(&file, contents).expect("write file");
        }

        (dir, root)
    }

    fn rule(id: &str, scope: &[&str], kind: CompiledRuleKind) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(scope.iter().copied()).expect("valid scope"),
            kind,
        }
    }

    fn config(rules: Vec<CompiledRule>, ignore: &[&str]) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::compile(ignore.iter().map(|g| (*g).to_owned())).expect("valid globs"),
            SkipDirs::default(),
            ContentHash::of(b"single"),
        )
    }

    fn naming() -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile(r"^(?<name>[a-z0-9-]+)\.use-case\.ts$")
                .expect("valid pattern"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            signature_hint: None,
        }
    }

    fn structure() -> CompiledRuleKind {
        CompiledRuleKind::Structure {
            allowed_subfolders: vec!["types".to_owned()],
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            filename_patterns: Vec::new(),
        }
    }

    /// The satisfied case, through a real parse.
    #[test]
    fn a_file_that_satisfies_its_rules_reports_nothing() {
        let (guard, root) = tree_at(&[(
            "src/user/create-client.use-case.ts",
            "export function CreateClient() {}",
        )]);
        let result = check_file(
            &root,
            &config(vec![rule("usecase-name", &["src/*"], naming())], &[]),
            &path("src/user/create-client.use-case.ts"),
        );
        drop(guard);

        assert!(result.findings.is_empty());
        assert!(result.skipped.is_empty());
        assert!(!result.fails_build());
    }

    /// And the failing one, which is what a hook blocks on.
    #[test]
    fn a_file_that_breaks_a_rule_says_so() {
        let (guard, root) = tree_at(&[(
            "src/user/create-client.use-case.ts",
            "export const CreateClient = () => {};",
        )]);
        let result = check_file(
            &root,
            &config(vec![rule("usecase-name", &["src/*"], naming())], &[]),
            &path("src/user/create-client.use-case.ts"),
        );
        drop(guard);

        assert_eq!(result.findings.len(), 1);
        assert!(result.fails_build());
        assert!(result.skipped.is_empty());
    }

    /// The correction the milestone exists for: a boundary rule is *not*
    /// skipped. It is file-local once its imports are resolved, and resolving
    /// them costs a handful of filesystem probes rather than a repository
    /// walk. `AGENT-INTEGRATION.md` said otherwise; C13 has the measurement.
    #[test]
    fn a_boundary_rule_runs_for_a_single_file() {
        let (guard, root) = tree_at(&[
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["packages/*"]}}}"#,
            ),
            (
                "packages/ui/button.tsx",
                "import { User } from '@/domain/user';\nexport const Button = () => User;",
            ),
            ("packages/domain/user.ts", "export const User = 1;"),
        ]);
        let result = check_file(
            &root,
            &config(
                vec![rule(
                    "ui-forbids-domain",
                    &["packages/ui/**"],
                    CompiledRuleKind::ImportBoundary {
                        forbid: PathSet::compile(["packages/domain/**".to_owned()])
                            .expect("valid globs"),
                        require: PathSet::default(),
                        forbid_packages: Vec::new(),
                        except: PathSet::default(),
                        except_from: PathSet::default(),
                        include_type_only: true,
                    },
                )],
                &[],
            ),
            &path("packages/ui/button.tsx"),
        );
        drop(guard);

        assert_eq!(result.findings.len(), 1, "{:?}", result.findings);
        assert!(
            result.skipped.is_empty(),
            "nothing was skipped: {:?}",
            result.skipped
        );
    }

    /// A `spec-pair` rule with `require_non_empty_spec` reads the spec itself,
    /// which is the file-level half of that rule. A file called `.spec.ts`
    /// with no test case in it satisfies the letter and defeats the point.
    #[test]
    fn an_empty_spec_is_caught() {
        let rule_with_contents = || {
            config(
                vec![rule(
                    "usecase-spec",
                    &["src/*"],
                    CompiledRuleKind::SpecPair {
                        subfolders: vec![".".to_owned()],
                        spec_markers: vec!["spec".to_owned()],
                        ignore_files: PathSet::default(),
                        require_non_empty_spec: true,
                        skip_type_only: false,
                    },
                )],
                &[],
            )
        };

        let (guard, root) = tree_at(&[
            ("src/user/user.ts", "export class User {}"),
            ("src/user/user.spec.ts", "describe('User', () => {});"),
        ]);
        let empty = check_file(&root, &rule_with_contents(), &path("src/user/user.spec.ts"));
        drop(guard);

        assert_eq!(empty.findings.len(), 1, "{:?}", empty.findings);

        let (guard, root) = tree_at(&[
            ("src/user/user.ts", "export class User {}"),
            ("src/user/user.spec.ts", "it('works', () => {});"),
        ]);
        let written = check_file(&root, &rule_with_contents(), &path("src/user/user.spec.ts"));
        drop(guard);

        assert!(written.findings.is_empty(), "{:?}", written.findings);
    }

    /// The failure a pre-write hook most needs to catch: the write creates a
    /// folder the structure rule forbids. Neither the file nor the folder
    /// exists when the hook asks, so a check against the tree as it stands
    /// would miss exactly this.
    #[test]
    fn a_write_that_would_create_a_forbidden_folder_is_caught() {
        let (guard, root) = tree_at(&[("src/user/types/user.ts", "")]);
        let result = check_file(
            &root,
            &config(vec![rule("shape", &["src/*"], structure())], &[]),
            &path("src/user/nope/x.ts"),
        );
        drop(guard);

        assert_eq!(result.findings.len(), 1, "{:?}", result.findings);
        assert!(result.fails_build());
        assert!(result.skipped.is_empty());
    }

    /// And the same write into an allowed folder is fine.
    #[test]
    fn a_write_into_an_allowed_folder_is_clean() {
        let (guard, root) = tree_at(&[("src/user/types/user.ts", "")]);
        let result = check_file(
            &root,
            &config(vec![rule("shape", &["src/*"], structure())], &[]),
            &path("src/user/types/address.ts"),
        );
        drop(guard);

        assert!(result.findings.is_empty(), "{:?}", result.findings);
    }

    /// A neighbour's problem is a real finding and `check` reports it. Handing
    /// it to an agent writing an unrelated file is noise, so it is filtered to
    /// this path's own ancestry.
    #[test]
    fn a_neighbours_problem_is_not_reported_here() {
        let (guard, root) = tree_at(&[("src/user/wrong/other.ts", "")]);
        let result = check_file(
            &root,
            &config(vec![rule("shape", &["src/*"], structure())], &[]),
            &path("src/user/types/address.ts"),
        );
        drop(guard);

        assert!(
            result.findings.is_empty(),
            "`wrong` is someone else's problem: {:?}",
            result.findings
        );
    }

    /// A file the parser could not read is not quietly passed. "No findings"
    /// and "not checked" are different answers, and a hook that conflated them
    /// would let an unparsable file through.
    #[test]
    fn an_unreadable_file_is_skipped_not_passed() {
        let (guard, root) = tree_at(&[("src/user/other.ts", "")]);
        // The file the rule is about is never created.
        let result = check_file(
            &root,
            &config(vec![rule("usecase-name", &["src/*"], naming())], &[]),
            &path("src/user/create-client.use-case.ts"),
        );
        drop(guard);

        assert!(result.findings.is_empty());
        assert_eq!(
            result.skipped,
            vec![Skipped {
                rule_id: "usecase-name".to_owned(),
                reason: Reason::Unreadable,
            }]
        );
    }

    /// An ignored path is not checked, exactly as in a full run. A hook that
    /// enforced rules `check` would not is worse than no hook.
    #[test]
    fn an_ignored_path_is_not_checked() {
        let (guard, root) = tree_at(&[(
            "src/legacy/create-client.use-case.ts",
            "export const CreateClient = () => {};",
        )]);
        let result = check_file(
            &root,
            &config(
                vec![rule("usecase-name", &["src/*"], naming())],
                &["src/legacy/**"],
            ),
            &path("src/legacy/create-client.use-case.ts"),
        );
        drop(guard);

        assert!(result.findings.is_empty());
        assert!(
            result.skipped.is_empty(),
            "nothing applies, nothing skipped"
        );
    }

    /// A rule that has nothing to do with this file is neither run nor
    /// reported. `skipped` means "applies but was not evaluated"; padding it
    /// with irrelevant rules would make the field useless.
    #[test]
    fn an_unrelated_rule_is_not_reported_as_skipped() {
        let (guard, root) = tree_at(&[("apps/web/page.tsx", "export const Page = 1;")]);
        let result = check_file(
            &root,
            &config(vec![rule("usecase-name", &["src/*"], naming())], &[]),
            &path("apps/web/page.tsx"),
        );
        drop(guard);

        assert!(result.findings.is_empty());
        assert!(result.skipped.is_empty());
    }

    /// A file that is not source is not parsed, so a rule needing facts is not
    /// claimed to have run.
    #[test]
    fn a_non_source_file_skips_the_rules_that_read_it() {
        let (guard, root) = tree_at(&[("src/user/data.json", "{}")]);
        let result = check_file(
            &root,
            &config(
                vec![rule(
                    "usecase-spec",
                    &["src/*"],
                    CompiledRuleKind::SpecPair {
                        subfolders: vec![".".to_owned()],
                        spec_markers: vec!["spec".to_owned()],
                        ignore_files: PathSet::default(),
                        require_non_empty_spec: true,
                        skip_type_only: false,
                    },
                )],
                &[],
            ),
            &path("src/user/data.json"),
        );
        drop(guard);

        // `spec-pair` exempts non-source files on its own, so it does not even
        // apply -- which is the right answer, and not a skip.
        assert!(result.findings.is_empty());
        assert!(result.skipped.is_empty());
    }

    /// A rule can be pointed at a file that is not source -- a
    /// `call-obligation` whose `file_pattern` matches a `.json` is the shape
    /// of it. There are no facts to read, and saying "could not be read" would
    /// send the user to fix a file that is fine. The rule is what is wrong.
    #[test]
    fn a_non_source_file_says_so_rather_than_claiming_it_is_broken() {
        let (guard, root) = tree_at(&[("src/user/data.json", r#"{"name":"x"}"#)]);
        let result = check_file(
            &root,
            &config(
                vec![rule(
                    "audit",
                    &["src/*"],
                    CompiledRuleKind::CallObligation {
                        file_pattern: Pattern::compile(r"^data\.json$").expect("valid pattern"),
                        symbol: "Event.save".to_owned(),
                        imported_from: "@org/domain/event".to_owned(),
                    },
                )],
                &[],
            ),
            &path("src/user/data.json"),
        );
        drop(guard);

        assert!(result.findings.is_empty());
        assert_eq!(
            result.skipped,
            vec![Skipped {
                rule_id: "audit".to_owned(),
                reason: Reason::NotSource,
            }]
        );
    }

    /// The reasons are stable slugs a caller can branch on, and sentences a
    /// human can read.
    #[test]
    fn every_reason_has_a_slug_and_a_sentence() {
        for reason in [Reason::Unreadable, Reason::NotSource] {
            assert!(
                !reason.as_str().contains(' '),
                "a slug is not a sentence: {}",
                reason.as_str()
            );
            assert!(reason.explain().contains(' '), "a sentence is not a slug");
        }

        assert_eq!(Reason::Unreadable.as_str(), "unreadable");
        assert_eq!(Reason::NotSource.as_str(), "not-source");
    }

    /// Findings and skips are both sorted, because a hook's output is diffed
    /// and compared by machines.
    #[test]
    fn the_answer_is_deterministic() {
        let entries = [(
            "src/user/create-client.use-case.ts",
            "export const CreateClient = () => {};",
        )];
        let config = config(
            vec![
                rule("z-shape", &["src/*"], structure()),
                rule("a-shape", &["src/*"], structure()),
                rule("usecase-name", &["src/*"], naming()),
            ],
            &[],
        );

        let (guard, root) = tree_at(&entries);
        let target = path("src/user/create-client.use-case.ts");
        let first = check_file(&root, &config, &target);
        let second = check_file(&root, &config, &target);
        drop(guard);

        assert_eq!(first, second);
    }

    /// The missing-spec check lives in `check_directory`, and it reaches a
    /// single file anyway: the one listing this command already reads is
    /// exactly what that check needs.
    #[test]
    fn a_missing_spec_is_caught_for_the_file_being_written() {
        let spec_pair = || {
            config(
                vec![rule(
                    "usecase-spec",
                    &["src/*"],
                    CompiledRuleKind::SpecPair {
                        subfolders: vec![".".to_owned()],
                        spec_markers: vec!["spec".to_owned()],
                        ignore_files: PathSet::default(),
                        require_non_empty_spec: false,
                        skip_type_only: false,
                    },
                )],
                &[],
            )
        };

        // Neither file exists yet: the hook is asked before the write.
        let (guard, root) = tree_at(&[("src/user/placeholder.md", "")]);
        let lonely = check_file(&root, &spec_pair(), &path("src/user/user.ts"));
        drop(guard);

        assert_eq!(lonely.findings.len(), 1, "{:?}", lonely.findings);
        assert_eq!(
            lonely.findings.first().map(|f| f.path.as_str()),
            Some("src/user/user.ts")
        );

        let (guard, root) = tree_at(&[("src/user/user.spec.ts", "it('works', () => {});")]);
        let paired = check_file(&root, &spec_pair(), &path("src/user/user.ts"));
        drop(guard);

        assert!(paired.findings.is_empty(), "{:?}", paired.findings);
    }
}
