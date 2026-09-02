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
//! probes rather than cross-file state.
//!
//! What genuinely cannot run is a rule that reads a file this command could
//! not. That is reported, never dropped: "no findings" and "not checked" are
//! different answers, and a hook that conflated them would pass an
//! unparsable file. Correction C6.

use archwarden_core::{
    compiled::CompiledConfig,
    finding::Finding,
    path::{FileClass, RepoRelPath},
    traits::{DirectoryContext, Exists, FactsNeeded, FileContext, RuleEngine},
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
    /// Specifiers this file imports that nothing could resolve.
    ///
    /// The same failure as a skip, one level down: a boundary rule ran, and
    /// ran blind. Saying "is fine" about a file whose imports were never
    /// placed is the answer issue #18 is about, and a hook is where it costs
    /// most -- the import an agent just wrote is exactly the one nothing has
    /// seen yet.
    ///
    /// In source order, which is stable between runs and is where the reader
    /// will look.
    pub unresolved_imports: Vec<String>,
}

impl Single {
    /// The answer for a file no rule was asked about.
    ///
    /// An ignored path is not checked at all, exactly as in a full run: a hook
    /// that enforced rules `check` would not is worse than no hook.
    fn nothing(path: &RepoRelPath) -> Self {
        Self {
            path: path.clone(),
            findings: Vec::new(),
            skipped: Vec::new(),
            unresolved_imports: Vec::new(),
        }
    }
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
    /// The rule reads the whole repository's import graph, and this command
    /// sees one file.
    ///
    /// Distinct from every reason above, because nothing is wrong with the
    /// file and nothing is wrong with the rule: the *command* cannot answer
    /// this question, and the fix is to run `check`. Refusing is the whole
    /// point — a cycle rule handed no graph reports nothing, which is what a
    /// repository with no cycles reports, so silence here would be a hook
    /// approving a write it never examined. Issue #70.
    NeedsRepository,
    /// The file is source in a language this build has no front-end for.
    ///
    /// Distinct from [`NotSource`](Self::NotSource) because the answer to it is
    /// different: a `.json` under a rule about imports means the rule is
    /// pointed at the wrong thing, and a `.py` under the same rule means the
    /// rule is right and archwarden cannot read it. One is a config to fix, the
    /// other is a front-end that does not exist yet. Issue #44 opened this
    /// distinction; before it, both were silence.
    NoFrontEnd,
}

impl Reason {
    /// The stable slug, for JSON and for a caller branching on it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unreadable => "unreadable",
            Self::NotSource => "not-source",
            Self::NoFrontEnd => "no-front-end",
            Self::NeedsRepository => "needs-repository",
        }
    }

    /// One sentence for a human.
    #[must_use]
    pub fn explain(self) -> &'static str {
        match self {
            Self::Unreadable => "the file could not be read",
            Self::NotSource => "it is not a TypeScript or JavaScript file",
            Self::NeedsRepository => {
                "the rule reads the whole repository's import graph, which this \
                 command cannot build from one file -- run `check`"
            }
            Self::NoFrontEnd => {
                "it is source in a language this build has no front-end for, so \
                 the rule is right and archwarden cannot read the file"
            }
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
    check(root, config, path, None)
}

/// Checks what a pending write *would* leave at `path`.
///
/// The question a `PreToolUse` hook is actually asked. [`check_file`] answers
/// about the bytes on disk, which for a hook is the previous version of the
/// file — so a new file was never checked at all, an edit introducing a
/// violation was permitted, and an edit *fixing* one was refused while naming
/// a rule the pending write already satisfied. That last one has no way out
/// from inside an agent loop: it is told to fix the file and denied permission
/// to fix it. Issue #55.
///
/// Only this path's own facts come from `content`. Siblings, importers and
/// every directory listing still come from disk, because those are what the
/// write is *not* about and the harness does not send them.
#[must_use]
pub fn check_write(
    root: &Utf8Path,
    config: &CompiledConfig,
    path: &RepoRelPath,
    content: &str,
) -> Single {
    check(root, config, path, Some(content))
}

/// Whether the file's own facts put it in a rule's population.
///
/// Both narrowing axes, asked here for the same reason a full run asks them: a
/// write judged against a rule that would not have applied to it is the hook
/// and `check` disagreeing about one file. Decision 25, and issue #144 for the
/// second.
///
/// A file nobody could read is **out** of a narrowed population, and that is
/// the honest answer rather than a silent one: `unresolved_imports` carries
/// the specifiers nobody could place, naming the file and the specifier.
fn narrowed_in(
    rule: Option<&archwarden_core::compiled::CompiledRule>,
    facts: Option<&archwarden_core::facts::FileFacts>,
) -> bool {
    let Some(rule) = rule else {
        return true;
    };

    if let Some(filter) = rule.directives.as_ref()
        && !facts.is_some_and(|facts| filter.matches(facts))
    {
        return false;
    }
    if let Some(filter) = rule.imports.as_ref()
        && !facts.is_some_and(|facts| filter.matches(facts))
    {
        return false;
    }
    true
}

fn check(
    root: &Utf8Path,
    config: &CompiledConfig,
    path: &RepoRelPath,
    pending: Option<&str>,
) -> Single {
    let engines = archwarden_rules::engines_for(config);
    let mut findings = Vec::new();
    let mut skipped = Vec::new();
    let mut unresolved_imports = Vec::new();

    // An ignored path is not checked at all, exactly as in a full run. A hook
    // that enforced rules `check` would not is worse than no hook.
    if config.is_ignored(path) {
        return Single::nothing(path);
    }

    findings.extend(directory_findings(root, &engines, path));

    // Paired by position, which is what `engines_for` promises. A rule's import
    // filter lives on the rule, so the pre-write hook has to know the pairing
    // for the same reason the full run does — and it has to, or a write would
    // be judged here against rules `check` would not have applied to it.
    // Decision 25.
    let rules: Vec<&archwarden_core::compiled::CompiledRule> = config.rules().collect();
    let applicable: Vec<(usize, &Box<dyn RuleEngine>)> = engines
        .iter()
        .enumerate()
        .filter(|(_, engine)| engine.applies_to(path))
        .collect();
    let narrowed_by_imports = applicable
        .iter()
        .any(|(index, _)| rules.get(*index).is_some_and(|rule| rule.imports.is_some()));
    // Asked only of the rules that will actually be evaluated. A rule that
    // reads the graph is refused below whatever this command reads, so parsing
    // for it buys an answer nobody receives -- and `unresolved_imports` means
    // "a rule ran blind", which is not what happened when no rule ran at all.
    let needs_facts = applicable.iter().any(|(index, engine)| {
        !engine.needs_graph()
            && (engine.needs_facts() == FactsNeeded::Code
                    // A rule narrowed by imports has to read the file to find
                    // out whether it is about it at all.
                    || rules.get(*index).is_some_and(|rule| rule.imports.is_some()))
    });
    let class = path.file_name().map_or(FileClass::Other, FileClass::of);
    let is_source = class == FileClass::Source;

    // `is_source` is defence in depth, and `cargo-mutants` cannot kill it: the
    // parser refuses a non-source extension anyway, so dropping the guard
    // reaches the same answer through a wasted read. It stays because reading
    // a file to be told it is the wrong kind is work with no reason, and on a
    // binary that happened to match a `file_pattern` it is a large one. Second
    // instance of this shape; the first is the parser guard in `run.rs`.
    let facts = if needs_facts && is_source {
        // The pending text when there is one, the file otherwise. A write that
        // has not landed is still the thing being judged.
        let parsed = match pending {
            Some(content) => crate::run::facts_from(path, content),
            None => crate::run::facts_of(root, path),
        };
        match parsed {
            Ok(mut facts) => {
                // Resolution is what a boundary rule needs, and it is a
                // handful of filesystem probes rather than the cross-file
                // state the docs expected. Paid only when a rule asks.
                if narrowed_by_imports
                    || applicable
                        .iter()
                        .any(|(_, engine)| !engine.needs_graph() && engine.needs_resolution())
                {
                    let resolver = archwarden_resolver::imports::ImportResolver::new(root);
                    let outcomes = crate::resolve::resolve_imports(&resolver, &mut facts);
                    // The path in each pair is this file, which the caller
                    // already has.
                    unresolved_imports = outcomes
                        .unresolved_imports
                        .into_iter()
                        .map(|(_, specifier)| specifier)
                        .collect();
                }
                Some(facts)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    let siblings = listing(root, path.parent().as_ref(), path.file_name());
    for (index, engine) in applicable {
        // The second axis, asked here for the same reason it is asked in a full
        // run: a write judged against a rule that would not have applied to it
        // is the hook and `check` disagreeing about one file. Decision 25.
        if !narrowed_in(rules.get(index).copied(), facts.as_ref()) {
            continue;
        }

        // Refused before anything else is asked. A graph is the whole
        // repository's edges and this command has one file, so there is no
        // arrangement of what is on hand that would answer the question --
        // unlike an unreadable file, where the same rule over the same
        // repository would have decided. Checked first because it is a
        // property of the *rule*: a cycle rule pointed at a `.md` would
        // otherwise be reported as `NotSource`, which reads as "point it
        // somewhere else" when the fix is to run `check`.
        if engine.needs_graph() {
            skipped.push(Skipped {
                rule_id: engine.id().to_string(),
                reason: Reason::NeedsRepository,
            });
            continue;
        }
        // Reported whatever the class, unlike the full run: this command
        // answers "what happened to *this* file", and "nothing, it is not
        // source" is a real answer here. The full `check` counts only what
        // somebody lost, so a `.json` beside the code keeps `checks_skipped`
        // reachable at zero there. See `AGENTS.md`.
        if facts.is_none() && engine.needs_facts() != FactsNeeded::Nothing {
            skipped.push(Skipped {
                rule_id: engine.id().to_string(),
                reason: match class {
                    FileClass::Source => Reason::Unreadable,
                    FileClass::UnreadableSource => Reason::NoFrontEnd,
                    // `FileClass` is non_exhaustive; a class added later is a
                    // file this rule could not read, which is what the
                    // catch-all already says.
                    _ => Reason::NotSource,
                },
            });
            continue;
        }
        findings.extend(engine.check_file(FileContext {
            path,
            facts: facts.as_ref(),
            docs: None,
            siblings: &siblings,
            // No walk here -- this command exists to answer about one file
            // without one -- so the question goes to disk. `is_file` and not
            // `exists`: a rule looking for `notas.md` and finding a directory
            // of that name has not found its companion.
            exists: Exists::new(&|candidate| root.join(candidate.as_str()).is_file()),
            // One file, and a graph is the whole repository's edges. A rule
            // that needs one never reaches here: it is refused above, under
            // `Reason::NeedsRepository`, rather than handed `None` and left to
            // report the silence that means "no cycles".
            graph: None,
            // The hook judges one file the way `check` judges all of them, so
            // a deadline is measured here too, against today.
            as_of: archwarden_core::date::Date::today(),
        }));
    }

    findings.sort();
    findings.dedup();
    skipped.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));

    Single {
        path: path.clone(),
        findings,
        skipped,
        unresolved_imports,
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
            why: None,
            not_yet: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
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
            annotation: Vec::new(),
            signature_hint: None,
            ignore_files: archwarden_core::glob::PathSet::default(),
        }
    }

    fn structure() -> CompiledRuleKind {
        CompiledRuleKind::Structure {
            allowed_subfolders: Some(vec!["types".to_owned()]),
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
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
                        groups: Vec::new(),
                        allow: None,
                        allow_packages: None,
                        require: PathSet::default(),
                        forbid_packages: Vec::new(),
                        forbid_reaching: PathSet::default(),
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
        assert!(
            result.unresolved_imports.is_empty(),
            "every import was placed: {:?}",
            result.unresolved_imports
        );
    }

    /// A boundary rule that ran against an import nothing could place ran
    /// blind, and this command used to answer `is fine.` either way. That is
    /// the answer issue #18 is about, and a hook is where it costs most: the
    /// import an agent has just written is exactly the one nothing has seen.
    #[test]
    fn an_import_the_boundary_rule_could_not_place_is_named() {
        let boundary = || {
            config(
                vec![rule(
                    "domain-is-self-contained",
                    &["packages/domain/**"],
                    CompiledRuleKind::ImportBoundary {
                        forbid: PathSet::compile(["apps/**".to_owned()]).expect("valid globs"),
                        groups: Vec::new(),
                        allow: None,
                        allow_packages: None,
                        require: PathSet::default(),
                        forbid_packages: Vec::new(),
                        forbid_reaching: PathSet::default(),
                        except: PathSet::default(),
                        except_from: PathSet::default(),
                        include_type_only: true,
                    },
                )],
                &[],
            )
        };

        // `@Domain/*` is an alias declared in the app's `tsconfig`, which
        // archwarden does not read: inside the package it resolves to nothing,
        // and the boundary it violates is the one being introduced.
        let (guard, root) = tree_at(&[(
            "packages/domain/row.ts",
            "import type { Order } from '@Domain/Order/types';\nexport type Violation = Order;",
        )]);
        let blind = check_file(&root, &boundary(), &path("packages/domain/row.ts"));
        drop(guard);

        assert!(blind.findings.is_empty(), "{:?}", blind.findings);
        assert_eq!(
            blind.unresolved_imports,
            vec!["@Domain/Order/types".to_owned()],
        );

        // And an import that lands somewhere is not a blind spot.
        let (guard, root) = tree_at(&[
            (
                "packages/domain/row.ts",
                "import type { Order } from './order';\nexport type Violation = Order;",
            ),
            ("packages/domain/order.ts", "export type Order = 1;"),
        ]);
        let placed = check_file(&root, &boundary(), &path("packages/domain/row.ts"));
        drop(guard);

        assert!(
            placed.unresolved_imports.is_empty(),
            "{:?}",
            placed.unresolved_imports
        );
    }

    /// A configuration with no boundary rule resolves nothing, so it has no
    /// blind spot to report. Claiming one would send a reader after an import
    /// no rule was ever going to look at.
    #[test]
    fn a_configuration_that_resolves_nothing_reports_no_blind_spot() {
        let (guard, root) = tree_at(&[(
            "src/user/create-client.use-case.ts",
            "import { thing } from '@org/never-installed';\nexport function CreateClient() {}",
        )]);
        let result = check_file(
            &root,
            &config(vec![rule("usecase-name", &["src/*"], naming())], &[]),
            &path("src/user/create-client.use-case.ts"),
        );
        drop(guard);

        assert!(result.findings.is_empty(), "{:?}", result.findings);
        assert!(result.unresolved_imports.is_empty());
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
                        spec_dirs: Vec::new(),
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

    /// A rule that reads the import graph cannot be answered here, and is
    /// refused rather than passed.
    ///
    /// This command sees one file. A cycle is a property of the whole
    /// repository, so the honest answer is "I could not decide", and the
    /// dishonest one is silence — which is exactly what a cycle rule handed no
    /// graph would produce, and exactly what a repository with no cycles
    /// produces. A pre-write hook that let a file through on that basis would
    /// be reporting a clean write it never checked.
    #[test]
    fn a_rule_that_needs_the_whole_repository_is_refused_not_passed() {
        let (guard, root) = tree_at(&[(
            "src/user/thing.ts",
            "import { other } from './other';\nexport const thing = () => other();",
        )]);
        let result = check_file(
            &root,
            &config(
                vec![rule(
                    "no-cycles",
                    &["src/*"],
                    CompiledRuleKind::ImportCycle {
                        include_type_only: false,
                    },
                )],
                &[],
            ),
            &path("src/user/thing.ts"),
        );
        drop(guard);

        assert!(
            result.findings.is_empty(),
            "nothing was decided: {:?}",
            result.findings
        );
        assert_eq!(
            result.skipped,
            vec![Skipped {
                rule_id: "no-cycles".to_owned(),
                reason: Reason::NeedsRepository,
            }],
            "and the refusal names the rule, so a reader knows what went \
             unchecked"
        );
    }

    /// A rule that is going to be refused does not first make the hook read
    /// the file and probe the filesystem for every specifier in it.
    ///
    /// Two costs, one of them wrong rather than merely wasteful. The waste is
    /// a parse and a handful of resolver probes in the one command whose whole
    /// point is to answer inside a keystroke. The wrong part is
    /// `unresolved_imports`: that field means "a rule ran, and ran blind", and
    /// filling it for a rule that never ran reads as a check that went wrong
    /// instead of one that did not happen.
    #[test]
    fn a_rule_that_will_be_refused_does_not_make_the_hook_read_anything() {
        let (guard, root) = tree_at(&[(
            "src/user/thing.ts",
            "import { x } from '@org/never-installed';\nexport const thing = x;",
        )]);
        let result = check_file(
            &root,
            &config(
                vec![rule(
                    "no-cycles",
                    &["src/*"],
                    CompiledRuleKind::ImportCycle {
                        include_type_only: false,
                    },
                )],
                &[],
            ),
            &path("src/user/thing.ts"),
        );
        drop(guard);

        assert!(
            result.unresolved_imports.is_empty(),
            "no rule ran, so there is no blind spot to report: {:?}",
            result.unresolved_imports
        );
        assert_eq!(
            result.skipped.len(),
            1,
            "and the refusal is still the answer"
        );
    }

    /// The slug is part of the JSON a hook emits, so it is pinned here rather
    /// than left to whatever the enum happens to be renamed to.
    #[test]
    fn needing_the_repository_has_a_stable_slug() {
        assert_eq!(Reason::NeedsRepository.as_str(), "needs-repository");
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
                        spec_dirs: Vec::new(),
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
                        with_options: Vec::new(),
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
                        spec_dirs: Vec::new(),
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
    /// Issue #55. A `PreToolUse` hook is asked whether a write *would* be
    /// legal, and the answer was coming from the bytes already on disk — so it
    /// answered about the past.
    ///
    /// Three consequences, and the third has no way out from inside an agent
    /// loop: a new file was never checked, an edit introducing a violation was
    /// permitted, and an edit *fixing* one was refused, naming a rule the
    /// pending write already satisfied.
    #[test]
    fn a_pending_write_is_judged_by_what_it_would_leave_behind() {
        let (guard, root) = tree_at(&[(
            "src/user/create-client.use-case.ts",
            "export function Wrong() {}",
        )]);
        let config = config(vec![rule("usecase", &["src/*"], naming())], &[]);
        let target = path("src/user/create-client.use-case.ts");

        // On disk the export is wrong, and that is what `check_file` says.
        let on_disk = check_file(&root, &config, &target);
        assert!(!on_disk.findings.is_empty(), "the file on disk is wrong");

        // The write that fixes it is judged by its own content.
        let fixed = check_write(&root, &config, &target, "export function CreateClient() {}");
        drop(guard);

        assert!(
            fixed.findings.is_empty(),
            "the write that fixes the file was refused: {:?}",
            fixed.findings
        );
    }

    /// And a file that is not on disk at all is checked, rather than skipped
    /// for want of anything to read. This is the case a pre-write gate most
    /// exists for: every content rule sailed through on creation.
    #[test]
    fn a_file_that_does_not_exist_yet_is_checked_against_the_write() {
        let (guard, root) = tree_at(&[("src/user/other.ts", "export const a = 1;")]);
        let config = config(vec![rule("usecase", &["src/*"], naming())], &[]);
        let target = path("src/user/create-client.use-case.ts");

        let absent = check_file(&root, &config, &target);
        assert!(
            absent.findings.is_empty() && !absent.skipped.is_empty(),
            "nothing on disk means nothing to judge: {absent:?}"
        );

        let written = check_write(&root, &config, &target, "export function Wrong() {}");
        drop(guard);

        assert!(
            !written.findings.is_empty(),
            "a new file's content was not checked: {written:?}"
        );
        assert!(
            written.skipped.is_empty(),
            "it had the content, so nothing was skipped: {:?}",
            written.skipped
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
    use archwarden_core::path::RepoRelPath;
    use archwarden_core::pattern::Pattern;
    use archwarden_core::scope::Scope;
    use camino::Utf8PathBuf;

    /// A rule no file satisfies, so whether it *ran* is visible in the result.
    fn naming(narrowed: Option<&str>) -> CompiledConfig {
        CompiledConfig::new(
            vec![CompiledRule {
                id: RuleId::new("n").expect("valid id"),
                module: None,
                why: None,
                not_yet: None,
                module_why: None,
                decision: None,
                directives: None,
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
                    ignore_files: archwarden_core::glob::PathSet::default(),
                },
            }],
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b"narrowing"),
        )
    }

    fn repository() -> (tempfile::TempDir, Utf8PathBuf) {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("utf-8");
        std::fs::create_dir_all(root.join("src/http")).expect("create");
        std::fs::create_dir_all(root.join("src/orders")).expect("create");
        std::fs::write(root.join("src/http/connection.ts"), "export const c = 1;\n")
            .expect("write");
        std::fs::write(
            root.join("src/orders/update.ts"),
            "import { c } from '../http/connection';\nexport const u = () => c;\n",
        )
        .expect("write");
        std::fs::write(
            root.join("src/orders/monthly.ts"),
            "export const m = () => 1;\n",
        )
        .expect("write");
        (guard, root)
    }

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// Issue #144, on the pre-write surface. The hook applies the same third
    /// axis a full run does, for the reason decision 25 gave about the second:
    /// a write judged against a rule `check` would not have applied to it is
    /// the two surfaces disagreeing about one file.
    #[test]
    fn a_file_is_judged_by_a_directive_rule_only_when_it_declares_one() {
        let (_guard, root) = repository();
        std::fs::write(
            root.join("src/orders/client-view.ts"),
            "\"use client\";\nexport const v = 1;\n",
        )
        .expect("write");

        let by_directive = |declaring: &[&str], not_declaring: &[&str], file: &str| {
            let mut rules: Vec<CompiledRule> = naming(None).rules().cloned().collect();
            if let Some(rule) = rules.first_mut() {
                rule.directives = Some(archwarden_core::compiled::DirectiveFilter {
                    declaring: declaring.iter().map(|d| (*d).to_owned()).collect(),
                    not_declaring: not_declaring.iter().map(|d| (*d).to_owned()).collect(),
                });
            }
            let config = CompiledConfig::new(
                rules,
                PathSet::default(),
                SkipDirs::default(),
                ContentHash::of(b"directives"),
            );
            !super::check_file(&root, &config, &path(file))
                .findings
                .is_empty()
        };

        assert!(by_directive(
            &["use client"],
            &[],
            "src/orders/client-view.ts"
        ));
        assert!(!by_directive(&["use client"], &[], "src/orders/monthly.ts"));

        assert!(by_directive(&[], &["use client"], "src/orders/monthly.ts"));
        assert!(!by_directive(
            &[],
            &["use client"],
            "src/orders/client-view.ts"
        ));
    }

    /// The pre-write hook applies the same filter the full run does. A write
    /// judged here against a rule `check` would not have applied to it is the
    /// two surfaces disagreeing about one file — decision 22's lesson, on the
    /// axis decision 25 added.
    #[test]
    fn a_file_that_imports_what_the_rule_names_is_judged() {
        let (_guard, root) = repository();

        let judged = super::check_file(
            &root,
            &naming(Some("src/http/**")),
            &path("src/orders/update.ts"),
        );

        assert!(!judged.findings.is_empty(), "the rule applied: {judged:?}");
    }

    /// And a sibling that imports nothing named is not judged by it at all.
    #[test]
    fn a_file_that_does_not_is_left_alone() {
        let (_guard, root) = repository();

        let judged = super::check_file(
            &root,
            &naming(Some("src/http/**")),
            &path("src/orders/monthly.ts"),
        );

        assert!(
            judged.findings.is_empty(),
            "the rule was not about it: {judged:?}"
        );
    }

    /// The same rule without the narrowing judges both — which is what makes
    /// the pair above mean something.
    #[test]
    fn the_same_rule_unnarrowed_judges_both() {
        let (_guard, root) = repository();

        for file in ["src/orders/update.ts", "src/orders/monthly.ts"] {
            let judged = super::check_file(&root, &naming(None), &path(file));
            assert!(!judged.findings.is_empty(), "{file}: {judged:?}");
        }
    }
}
