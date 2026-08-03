//! Carrying out a move, once `impact` has said what it costs.
//!
//! Dry run stays the default. `--apply` is the explicit second step, and this
//! module is what it runs.
//!
//! # Why this is not `--fix`
//!
//! Decision 2 keeps archwarden in the report-only space, and decision 13 says
//! why the most obviously fixable rule must never have a fix: `--fix` picks
//! the correction, and for `spec-pair` the correction it would pick is a lie.
//!
//! Nothing here picks anything. The caller names a source and a destination,
//! and this carries out the move they described — the same relationship
//! decision 13 left open for `scaffold --write`, "write the file I am about to
//! write" rather than "fix the violation". No finding suggests it, `check`
//! never mentions it, and there is no mode that moves more than what was
//! named. That boundary is the whole licence for this module to exist; see
//! decision 16.
//!
//! # Atomicity
//!
//! Every edit and every move is computed and validated before a single byte is
//! written. A refusal is therefore total: nothing has happened yet. That is
//! also why a dirty working tree is refused outright rather than warned about
//! — `git` is the undo, and an undo that would take uncommitted work with it
//! is not one.

use archwarden_core::path::RepoRelPath;
use camino::Utf8Path;

use crate::respecify::{Rewrite, respecify};

/// One file changing place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    /// Where it is.
    pub from: RepoRelPath,
    /// Where it goes.
    pub to: RepoRelPath,
    /// Whether this move was asked for, or is a spec travelling with its unit
    /// file. Reported separately because a caller who did not mention the spec
    /// should be told it is coming along.
    pub is_spec_sibling: bool,
}

/// One replacement inside one file: a byte range and what goes in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    /// Byte offset of the first character of the old specifier's quotes.
    pub start: usize,
    /// Byte offset one past the last character.
    pub end: usize,
    /// The specifier as written, without quotes.
    pub was: String,
    /// What replaces it, without quotes.
    pub now: String,
}

/// One file whose import specifiers change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// The file to rewrite.
    pub path: RepoRelPath,
    /// The replacements, in source order.
    pub replacements: Vec<Replacement>,
}

/// Why a move cannot be carried out.
///
/// Every one of these is total: the plan is validated before anything is
/// written, so a refusal means nothing has changed on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The working tree has uncommitted changes.
    DirtyTree(Vec<String>),
    /// There is no git repository here.
    NotAGitRepository(String),
    /// A file contains a dynamic import naming no module, so whether it
    /// imports the target is unknowable.
    Opaque(Vec<RepoRelPath>),
    /// A specifier resolves to the target and cannot be recomputed.
    UnreadableSpecifier {
        /// The importing file.
        importer: RepoRelPath,
        /// The specifier as written.
        specifier: String,
        /// Which of the four reasons, so the message can name the right file.
        why: crate::respecify::Unknown,
    },
    /// The specifier could not be located in the file's bytes.
    ///
    /// The parser reports the span of the whole statement; the quoted
    /// specifier is found inside it. When it is not there, the file on disk is
    /// not the file that was parsed, and editing by offset would corrupt it.
    SpecifierNotFound {
        /// The importing file.
        importer: RepoRelPath,
        /// The specifier as written.
        specifier: String,
    },
    /// Something is already at the destination.
    DestinationOccupied(RepoRelPath),
    /// Two sources want the same destination.
    Collision {
        /// Where they both land.
        destination: RepoRelPath,
        /// The sources, in path order.
        sources: Vec<RepoRelPath>,
    },
    /// The source is not there.
    SourceMissing(RepoRelPath),
    /// A file the dry run named as an importer got no edit.
    ///
    /// Nothing is supposed to reach this: a specifier that cannot be
    /// recomputed refuses on its own. It exists because the failure it guards
    /// is the worst one this module has — a repository that compiles nowhere,
    /// reported as success, found by whoever runs `tsc` next. A guard for a
    /// state that "cannot happen" is exactly the guard worth having when the
    /// cost of being wrong is silent.
    ImporterNotRewritten {
        /// The file that imports the target and was not rewritten.
        importer: RepoRelPath,
        /// What it imports.
        target: RepoRelPath,
    },
    /// A file could not be read.
    Unreadable(RepoRelPath, String),
    /// An import naming a package something is moving out of did not resolve.
    ///
    /// The same blind spot as [`Opaque`](Self::Opaque), reached the other way.
    /// A dynamic import names no module; this names one that should be in the
    /// repository and was not found, so whether it points at a moving file is
    /// equally unknowable — and unlike a dynamic import, it looks like an
    /// ordinary specifier, so nothing about it invites suspicion.
    ///
    /// The state that produces it is a workspace whose packages do not resolve:
    /// a clone before `install`, a package manager layout with no
    /// `node_modules/<scope>/<pkg>`, or an `exports` map whose patterns
    /// archwarden reads differently from the bundler. In all three the move can
    /// still be carried out — after the imports resolve.
    UnresolvedLocalImport {
        /// The file that wrote the specifier.
        importer: RepoRelPath,
        /// The specifier as written.
        specifier: String,
    },
}

impl Refusal {
    /// Whether `--force` may override this.
    ///
    /// Only the dynamic-import blind spot. Everything else is a fact about the
    /// filesystem or an edit this cannot compute, and forcing past one of
    /// those would produce a repository that does not build — which is not a
    /// judgement a flag should be able to make.
    #[must_use]
    pub fn is_forceable(&self) -> bool {
        matches!(self, Self::Opaque(_))
    }
}

/// Everything a move would do, before any of it is done.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Files changing place, in source order.
    pub moves: Vec<Move>,
    /// Files whose specifiers change, in path order.
    pub edits: Vec<Edit>,
    /// Why this cannot proceed. Empty means it can.
    pub refusals: Vec<Refusal>,
    /// Symbols the move leaves alone, named so nobody assumes otherwise.
    ///
    /// A file renamed mid-move keeps its exported symbol: renaming it would
    /// break every caller in a way this cannot see, which is decision 13's
    /// argument about `naming`. `check` reports the mismatch afterwards, and
    /// that is the right place for it.
    pub untouched_symbols: Vec<String>,
    /// Whether any move crosses from one workspace package into another.
    ///
    /// A different `tsconfig` applies at the destination, so a path alias in
    /// the moved file can mean something else there — which no specifier
    /// rewrite can decide and a reader has to check.
    pub crosses_packages: bool,
}

impl Plan {
    /// Whether the plan may be carried out.
    #[must_use]
    pub fn is_actionable(&self, force: bool) -> bool {
        if self.refusals.is_empty() {
            return true;
        }
        force && self.refusals.iter().all(Refusal::is_forceable)
    }
}

/// Locates the quoted specifier inside a statement's byte range.
///
/// The parser records the span of the whole `import … from '…'`, not of the
/// literal, and adding a second span to `FileFacts` would invalidate every
/// cached parse in every repository. The specifier is a string literal, so
/// within one import statement the exact quoted text occurs once — searching
/// for it is unambiguous, and *not* finding it is a refusal rather than a
/// guess.
#[must_use]
pub fn locate(
    source: &str,
    statement: std::ops::Range<usize>,
    specifier: &str,
) -> Option<Replacement> {
    let end = statement.end.min(source.len());
    let text = source.get(statement.start..end)?;

    for quote in ['\'', '"', '`'] {
        let needle = format!("{quote}{specifier}{quote}");
        if let Some(offset) = text.find(&needle) {
            let start = statement.start + offset + quote.len_utf8();
            return Some(Replacement {
                start,
                end: start + specifier.len(),
                was: specifier.to_owned(),
                now: String::new(),
            });
        }
    }
    None
}

/// Applies a file's replacements to its text.
///
/// Back to front, so an earlier replacement never shifts a later one's
/// offsets.
#[must_use]
pub fn rewritten(source: &str, replacements: &[Replacement]) -> String {
    let mut ordered: Vec<&Replacement> = replacements.iter().collect();
    ordered.sort_by_key(|r| std::cmp::Reverse(r.start));

    let mut out = source.to_owned();
    for replacement in ordered {
        if replacement.end <= out.len() {
            out.replace_range(replacement.start..replacement.end, &replacement.now);
        }
    }
    out
}

/// The spec file that travels with a unit file, if one exists.
///
/// Using the same stem-and-marker rules `spec-pair` uses, because a move that
/// left the spec behind would break archwarden's own rule — and a second
/// notion of what a spec is called would drift from the first.
#[must_use]
pub fn spec_sibling(
    root: &Utf8Path,
    file: &RepoRelPath,
    markers: &[String],
) -> Option<(RepoRelPath, String)> {
    let name = file.file_name()?;
    let parent = file.parent()?;
    let (stem, extension) = name.rsplit_once('.')?;

    for marker in markers {
        for candidate_extension in ["ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs"] {
            let candidate = format!("{stem}.{marker}.{candidate_extension}");
            let path = parent.join(&candidate).ok()?;
            if root.join(path.as_path()).is_file() {
                return Some((path, format!(".{marker}.{candidate_extension}")));
            }
        }
    }
    let _ = extension;
    None
}

/// The destination for a spec, given where its unit file lands.
#[must_use]
pub fn spec_destination(to: &RepoRelPath, suffix: &str) -> Option<RepoRelPath> {
    let name = to.file_name()?;
    let (stem, _extension) = name.rsplit_once('.')?;
    to.parent()?.join(&format!("{stem}{suffix}")).ok()
}

/// Whether the working tree is clean enough to be an undo.
///
/// # Errors
/// A message when git is missing or this is not a repository.
pub fn working_tree_state(root: &Utf8Path) -> Result<Vec<String>, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root.as_std_path())
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;

    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(message.trim().to_owned());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect())
}

/// Moves a file with `git mv`, so history follows it.
///
/// # Errors
/// Whatever git said.
pub fn git_move(root: &Utf8Path, from: &RepoRelPath, to: &RepoRelPath) -> Result<(), String> {
    if let Some(parent) = to.parent()
        && !parent.is_root()
    {
        std::fs::create_dir_all(root.join(parent.as_path()))
            .map_err(|error| format!("cannot create `{parent}`: {error}"))?;
    }

    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root.as_std_path())
        .args(["mv", from.as_str(), to.as_str()])
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;

    if output.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
}

/// Every move, as a lookup from where a file is to where it goes.
pub type Moves = std::collections::BTreeMap<RepoRelPath, RepoRelPath>;

/// Works out the replacements one file needs, given every move happening.
///
/// `file` is the file being rewritten, wherever it ends up. Both halves of the
/// question are answered here: an import that points at something in `moves`
/// has to name it somewhere else, and every relative import has to be
/// remeasured if `file` itself is in `moves`. A batch move makes both true of
/// the same file, which is why this is one pass rather than two.
///
/// A specifier that cannot be recomputed is pushed to `refusals`, not skipped:
/// it resolved to a file that is moving, so leaving it alone would leave it
/// pointing at nothing.
pub fn replacements_for(
    source: &str,
    file: &RepoRelPath,
    facts: &archwarden_core::facts::FileFacts,
    moves: &Moves,
    workspace: &archwarden_resolver::workspace::Workspace,
    refusals: &mut Vec<Refusal>,
) -> Vec<Replacement> {
    let new_home = moves.get(file).unwrap_or(file);
    let mut replacements = Vec::new();

    for import in &facts.imports {
        let Some(target) = import.resolved.as_ref() else {
            // An unresolved specifier points at nothing archwarden can see, so
            // no move changes what it means. `check` is where an import that
            // does not resolve is reported.
            continue;
        };
        let moved_target = moves.get(target);
        let new_target = moved_target.unwrap_or(target);

        if moved_target.is_none() && new_home == file {
            // Neither end moved. Most imports in most files.
            continue;
        }

        let now = match respecify(
            &import.specifier,
            new_home,
            new_target,
            moved_target.is_some(),
            workspace,
        ) {
            Rewrite::Unchanged => continue,
            Rewrite::To(now) => now,
            Rewrite::Unknown(why) => {
                refusals.push(Refusal::UnreadableSpecifier {
                    importer: file.clone(),
                    specifier: import.specifier.clone(),
                    why,
                });
                continue;
            }
        };

        let range = import.span.start as usize..import.span.end as usize;
        match locate(source, range, &import.specifier) {
            Some(mut replacement) => {
                replacement.now = now;
                replacements.push(replacement);
            }
            None => refusals.push(Refusal::SpecifierNotFound {
                importer: file.clone(),
                specifier: import.specifier.clone(),
            }),
        }
    }

    replacements.sort_by_key(|r| r.start);
    replacements
}

/// Everything a set of moves would do, worked out before any of it is done.
///
/// The order is the order of the refusals: git first, because a dirty tree
/// makes every other answer moot; then the filesystem; then the edits, which
/// are the expensive part and the part a refusal above makes pointless.
#[must_use]
pub fn plan(
    root: &Utf8Path,
    config: &archwarden_core::compiled::CompiledConfig,
    tree: &archwarden_engine::walk::RepoTree,
    requests: &[(RepoRelPath, RepoRelPath)],
    spec_markers: &[String],
) -> Plan {
    let mut plan = Plan::default();

    match working_tree_state(root) {
        Ok(dirty) if !dirty.is_empty() => plan.refusals.push(Refusal::DirtyTree(dirty)),
        Ok(_) => {}
        Err(message) => plan.refusals.push(Refusal::NotAGitRepository(message)),
    }

    // The spec travels with its unit file, or the move breaks archwarden's own
    // `spec-pair` rule. Added before validation so a spec landing on something
    // is caught like any other collision.
    for (from, to) in requests {
        plan.moves.push(Move {
            from: from.clone(),
            to: to.clone(),
            is_spec_sibling: false,
        });
        if let Some((spec, suffix)) = spec_sibling(root, from, spec_markers)
            && let Some(destination) = spec_destination(to, &suffix)
        {
            plan.moves.push(Move {
                from: spec,
                to: destination,
                is_spec_sibling: true,
            });
        }
    }

    // A batch whose source glob already swept the specs in would name each of
    // them twice: once as a file that matched, once as a sibling travelling
    // with its unit file. The same move written twice is one move, and
    // leaving both in would trip the collision check on a file against
    // itself. The request wins over the sibling, because it is what the
    // caller wrote.
    let mut seen = std::collections::BTreeSet::new();
    plan.moves.retain(|entry| seen.insert(entry.from.clone()));

    validate(root, &plan.moves, &mut plan.refusals);

    let moves: Moves = plan
        .moves
        .iter()
        .map(|m| (m.from.clone(), m.to.clone()))
        .collect();

    let workspace = archwarden_resolver::workspace::Workspace::discover(root);
    plan.crosses_packages = crosses_packages(&plan.moves, &workspace);

    // A renamed file keeps its exported symbol. Said out loud, because the
    // rename is the moment somebody assumes otherwise — and renaming it would
    // break every caller in a way this cannot see, which is decision 13's
    // argument about `naming` rules.
    plan.untouched_symbols = renamed_stems(&plan.moves);

    let targets: Vec<RepoRelPath> = plan.moves.iter().map(|m| m.from.clone()).collect();
    let found = archwarden_engine::importers::importers_of_each(root, config, tree, &targets);

    if let Some(opaque) = found.values().next()
        && !opaque.opaque.is_empty()
    {
        plan.refusals.push(Refusal::Opaque(opaque.opaque.clone()));
    }

    if let Some(entry) = found.values().next() {
        plan.refusals.extend(blind_spots(
            &entry.unresolved_local,
            &plan.moves,
            &workspace,
        ));
    }

    // Every file that needs rewriting: the importers of anything moving, plus
    // the moving files themselves, whose own relative imports are measured
    // from somewhere new.
    let mut to_rewrite: std::collections::BTreeSet<RepoRelPath> = found
        .values()
        .flat_map(|importers| importers.direct.iter().map(|i| i.path.clone()))
        .collect();
    to_rewrite.extend(plan.moves.iter().map(|m| m.from.clone()));

    let resolver = archwarden_resolver::imports::ImportResolver::new(root);

    for path in to_rewrite {
        let (source, facts) =
            match archwarden_engine::importers::resolved_facts(root, &path, &resolver) {
                Ok(read) => read,
                Err(message) => {
                    plan.refusals
                        .push(Refusal::Unreadable(path.clone(), message));
                    continue;
                }
            };

        let replacements = replacements_for(
            &source,
            &path,
            &facts,
            &moves,
            &workspace,
            &mut plan.refusals,
        );
        if !replacements.is_empty() {
            plan.edits.push(Edit { path, replacements });
        }
    }

    plan.edits.sort_by(|a, b| a.path.cmp(&b.path));

    // The invariant, checked rather than trusted: every file the dry run named
    // as an importer must have come out of the loop above with an edit.
    //
    // Nothing above is supposed to be able to break it — a specifier that
    // cannot be recomputed already pushes a refusal. But "supposed to" is what
    // this module cannot afford: the failure it would hide is a repository
    // that compiles nowhere, reported as success with exit 0, and found by
    // whoever runs `tsc` next. The check costs one set difference against a
    // list that is already in hand.
    let edited: std::collections::BTreeSet<&RepoRelPath> =
        plan.edits.iter().map(|edit| &edit.path).collect();
    let moving: std::collections::BTreeSet<&RepoRelPath> =
        plan.moves.iter().map(|entry| &entry.from).collect();

    for (target, importers) in &found {
        for importer in &importers.direct {
            // A moving file needs no edit of its own unless it has relative
            // imports, and an importer that is itself the target's spec moves
            // with it. Both are already handled; what must never happen is a
            // file that stays put, imports something that moved, and was not
            // rewritten.
            if edited.contains(&importer.path) || moving.contains(&importer.path) {
                continue;
            }
            plan.refusals.push(Refusal::ImporterNotRewritten {
                importer: importer.path.clone(),
                target: target.clone(),
            });
        }
    }

    plan
}

/// Whether any move leaves the workspace package it started in.
///
/// Worth reporting on its own: a different `tsconfig` applies at the
/// destination, so a path alias in the moved file can mean something else
/// there — which no specifier rewrite can decide and a reader has to check.
fn crosses_packages(moves: &[Move], workspace: &archwarden_resolver::workspace::Workspace) -> bool {
    let package_of = |path: &RepoRelPath| {
        workspace
            .packages()
            .iter()
            .find(|package| path.as_path().starts_with(package.directory.as_path()))
            .map(|package| package.name.as_str())
    };
    moves
        .iter()
        .any(|m| package_of(&m.from) != package_of(&m.to))
}

/// Imports archwarden cannot place, in packages this move is taking files out
/// of.
///
/// An import that names a workspace package and did not resolve is one nothing
/// can locate, so nothing can say whether it points at a file that is about to
/// move. The importer never appears among the known importers either, which is
/// why the `ImporterNotRewritten` guard below never saw it: the guard asks
/// whether every *known* importer was rewritten, and this is a file that was
/// never known. That is how a move came to report success over a repository
/// that no longer builds. Issue #11.
///
/// Narrowed to the packages something is moving out of, and the narrowing is
/// the whole difference between a guard and an obstacle. A repository before
/// `install` has thousands of unresolved imports to real dependencies, and
/// `react` failing to resolve costs this move no accuracy at all — no move
/// could ever change what it means.
fn blind_spots(
    unresolved: &[(RepoRelPath, String)],
    moves: &[Move],
    workspace: &archwarden_resolver::workspace::Workspace,
) -> Vec<Refusal> {
    let package_of = |path: &RepoRelPath| {
        workspace
            .packages()
            .iter()
            .find(|package| path.as_path().starts_with(package.directory.as_path()))
            .map(|package| package.name.as_str())
    };
    let moving: std::collections::BTreeSet<&str> =
        moves.iter().filter_map(|m| package_of(&m.from)).collect();

    unresolved
        .iter()
        .filter(|(_, specifier)| {
            workspace
                .package_for(specifier)
                .is_some_and(|package| moving.contains(package.name.as_str()))
        })
        .map(|(importer, specifier)| Refusal::UnresolvedLocalImport {
            importer: importer.clone(),
            specifier: specifier.clone(),
        })
        .collect()
}

/// Checks every move against the filesystem and against the others.
fn validate(root: &Utf8Path, moves: &[Move], refusals: &mut Vec<Refusal>) {
    let mut destinations: std::collections::BTreeMap<&RepoRelPath, Vec<RepoRelPath>> =
        std::collections::BTreeMap::new();

    for entry in moves {
        if !root.join(entry.from.as_path()).is_file() {
            refusals.push(Refusal::SourceMissing(entry.from.clone()));
        }
        if root.join(entry.to.as_path()).exists() {
            refusals.push(Refusal::DestinationOccupied(entry.to.clone()));
        }
        destinations
            .entry(&entry.to)
            .or_default()
            .push(entry.from.clone());
    }

    // Two files landing on one path. Caught before anything is written,
    // because carrying it out would silently delete one of them.
    for (destination, mut sources) in destinations {
        if sources.len() > 1 {
            sources.sort();
            refusals.push(Refusal::Collision {
                destination: destination.clone(),
                sources,
            });
        }
    }
}

/// The stems of files whose name changes during the move.
///
/// A move that keeps the filename cannot have renamed a symbol, so it is not
/// worth a sentence. One that changes it almost certainly should have.
fn renamed_stems(moves: &[Move]) -> Vec<String> {
    let mut renamed: Vec<String> = moves
        .iter()
        .filter(|entry| !entry.is_spec_sibling)
        .filter_map(|entry| {
            let (before, after) = (entry.from.file_name()?, entry.to.file_name()?);
            (before != after).then(|| format!("{before} → {after}"))
        })
        .collect();
    renamed.sort();
    renamed.dedup();
    renamed
}

/// Carries out a validated plan.
///
/// Edits first, then the moves. Every new file body is built in memory and
/// checked before a byte is written, so the window in which a failure can
/// leave half a refactor behind is one filesystem call wide — and a clean
/// working tree was a precondition, so `git checkout .` is the whole undo.
///
/// # Errors
/// A message naming what failed and what to run to get back.
pub fn carry_out(root: &Utf8Path, plan: &Plan) -> Result<(), String> {
    let mut bodies = Vec::with_capacity(plan.edits.len());
    for edit in &plan.edits {
        let path = root.join(edit.path.as_path());
        let source = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read `{}`: {error}", edit.path))?;
        bodies.push((path, rewritten(&source, &edit.replacements)));
    }

    for (path, body) in bodies {
        std::fs::write(&path, body).map_err(|error| {
            format!("cannot write `{path}`: {error}\n  nothing was moved; `git checkout .` restores the edits already made")
        })?;
    }

    for entry in &plan.moves {
        git_move(root, &entry.from, &entry.to).map_err(|message| {
            format!(
                "cannot move `{}` to `{}`: {message}\n  \
                 run `git checkout .` and `git status` — some files may already have moved",
                entry.from, entry.to
            )
        })?;
    }

    prune_emptied(root, &plan.moves);
    Ok(())
}

/// Removes source directories the move left empty.
///
/// `git mv` moves files, and git does not track directories — so emptying a
/// folder leaves the folder. That is not a cosmetic leftover: `structure`
/// rules are *about* directories, so an emptied `shared/` keeps reporting
/// exactly the finding the refactor was carried out to remove. Measured on a
/// real repository: nine warnings, unchanged, after every file in them had
/// moved.
///
/// Upwards while empty, because emptying `shared/calcs` empties `shared`.
/// Never the repository root, and never a directory that still holds anything
/// — including a file the walk ignores, since removing one would delete
/// something nobody asked about.
fn prune_emptied(root: &Utf8Path, moves: &[Move]) {
    let mut directories: Vec<RepoRelPath> = moves.iter().filter_map(|m| m.from.parent()).collect();
    directories.sort_by_key(|path| std::cmp::Reverse(path.as_str().len()));
    directories.dedup();

    for directory in directories {
        let mut current = Some(directory);
        while let Some(path) = current {
            if path.is_root() {
                break;
            }
            let absolute = root.join(path.as_path());
            let is_empty =
                std::fs::read_dir(&absolute).is_ok_and(|mut entries| entries.next().is_none());
            if !is_empty || std::fs::remove_dir(&absolute).is_err() {
                break;
            }
            current = path.parent();
        }
    }
}

/// Says why nothing happened, and what would let it.
///
/// One arm per refusal, each a sentence. Splitting it would put the arms
/// somewhere the exhaustive `match` no longer names them, which is what makes
/// a refusal added without a sentence fail to compile.
#[allow(clippy::too_many_lines, reason = "one arm per refusal; see above")]
///
/// Every line here is about a repository that is exactly as it was: the plan
/// is validated in full before a byte is written, so there is nothing to
/// undo and nothing to warn about having half-done.
pub fn render_refusals(plan: &Plan, force: bool, out: &mut dyn std::io::Write) {
    let _ = writeln!(out, "× nothing was moved.\n");

    for refusal in &plan.refusals {
        match refusal {
            Refusal::DirtyTree(entries) => {
                let _ = writeln!(
                    out,
                    "  The working tree has uncommitted changes. `git` is the undo for this,\n  \
                     and an undo that would take your own work with it is not one. Commit or\n  \
                     stash first:"
                );
                for entry in entries.iter().take(10) {
                    let _ = writeln!(out, "    {entry}");
                }
                if entries.len() > 10 {
                    let _ = writeln!(out, "    … and {} more", entries.len() - 10);
                }
            }
            Refusal::NotAGitRepository(message) => {
                let _ = writeln!(
                    out,
                    "  This is not a git repository ({message}).\n  \
                     A move rewrites files across the tree, and without git there is no undo."
                );
            }
            Refusal::Opaque(paths) => {
                let _ = writeln!(
                    out,
                    "  {} {} a dynamic import naming no module, so whether {} imports what is\n  \
                     moving cannot be known:",
                    paths.len(),
                    if paths.len() == 1 {
                        "file has"
                    } else {
                        "files have"
                    },
                    if paths.len() == 1 { "it" } else { "they" },
                );
                for path in paths {
                    let _ = writeln!(out, "    {path}");
                }
                if !force {
                    let _ = writeln!(
                        out,
                        "\n  Look at {}, then pass `--force` to say so.",
                        if paths.len() == 1 { "it" } else { "them" }
                    );
                }
            }
            Refusal::UnreadableSpecifier {
                importer,
                specifier,
                why,
            } => {
                let _ = writeln!(
                    out,
                    "  `{importer}` imports `{specifier}`, and no new specifier could be\n  \
                     worked out for it:\n  \
                     {}.\n  \
                     Rewriting the rest and leaving this one would produce a repository\n  \
                     that does not build.",
                    why.explain()
                );
            }
            Refusal::SpecifierNotFound {
                importer,
                specifier,
            } => {
                let _ = writeln!(
                    out,
                    "  `{specifier}` is not where `{importer}` was parsed to have it. The file\n  \
                     on disk changed while this was running."
                );
            }
            Refusal::DestinationOccupied(path) => {
                let _ = writeln!(out, "  `{path}` already exists.");
            }
            Refusal::Collision {
                destination,
                sources,
            } => {
                let _ = writeln!(
                    out,
                    "  {} files would land on `{destination}`:",
                    sources.len()
                );
                for source in sources {
                    let _ = writeln!(out, "    {source}");
                }
            }
            Refusal::SourceMissing(path) => {
                let _ = writeln!(out, "  `{path}` is not there.");
            }
            Refusal::ImporterNotRewritten { importer, target } => {
                let _ = writeln!(
                    out,
                    "  `{importer}` imports `{target}`, which is moving, and no rewrite was\n  \
                     worked out for it. Applying the rest would leave that import pointing at\n  \
                     nothing. This is a bug in archwarden — please report it with the command\n  \
                     you ran."
                );
            }
            Refusal::UnresolvedLocalImport {
                importer,
                specifier,
            } => {
                let _ = writeln!(
                    out,
                    "  `{importer}` imports `{specifier}`, which names a package this move is\n  \
                     taking files out of — and it does not resolve to a file here. Whether it\n  \
                     points at one of them cannot be known, so rewriting the rest would leave\n  \
                     this one pointing at nothing.\n  \
                     Usually the workspace is not installed: run your package manager's install\n  \
                     and try again. If it is installed, the package's `exports` map does not\n  \
                     cover this subpath the way the bundler resolves it."
                );
            }
            Refusal::Unreadable(path, message) => {
                let _ = writeln!(out, "  `{path}` cannot be read: {message}");
            }
        }
        let _ = writeln!(out);
    }
}

/// Says what was done.
pub fn render_done(plan: &Plan, out: &mut dyn std::io::Write) {
    let asked: Vec<&Move> = plan.moves.iter().filter(|m| !m.is_spec_sibling).collect();
    let specs = plan.moves.len() - asked.len();

    let _ = writeln!(
        out,
        "Moved {} {}{}:\n",
        asked.len(),
        if asked.len() == 1 { "file" } else { "files" },
        if specs == 0 {
            String::new()
        } else {
            format!(
                ", and {specs} spec {} with {}",
                if specs == 1 { "sibling" } else { "siblings" },
                if specs == 1 { "it" } else { "them" }
            )
        }
    );
    for entry in &plan.moves {
        let _ = writeln!(out, "  {} → {}", entry.from, entry.to);
    }

    let replacements: usize = plan.edits.iter().map(|e| e.replacements.len()).sum();
    let _ = writeln!(
        out,
        "\n{replacements} import {} rewritten across {} {}.",
        if replacements == 1 {
            "specifier"
        } else {
            "specifiers"
        },
        plan.edits.len(),
        if plan.edits.len() == 1 {
            "file"
        } else {
            "files"
        }
    );

    // Said every time it applies, because the rename is the moment somebody
    // assumes the symbol came with it. It did not, and `check` will say so.
    if !plan.untouched_symbols.is_empty() {
        let _ = writeln!(
            out,
            "\nThe filename changed and the exported symbol did not:"
        );
        for renamed in &plan.untouched_symbols {
            let _ = writeln!(out, "  {renamed}");
        }
        let _ = writeln!(
            out,
            "  Renaming an export breaks every caller, which this cannot see. Run `check` —\n  \
             a `naming` rule will say whether the project wants them to match."
        );
    }

    if plan.crosses_packages {
        let _ = writeln!(
            out,
            "\nA move crossed a package boundary, so a different `tsconfig` applies at the\n\
             destination. Any path alias in the moved files may mean something else there;\n\
             `tsc` is what answers that."
        );
    }

    let _ = writeln!(out, "\n`git status` shows it; `git checkout .` undoes it.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// The parser gives a statement span; the edit needs the literal. Finding
    /// it inside the statement is what bridges the two without adding a field
    /// to `FileFacts` and invalidating every cached parse in every repository.
    #[test]
    fn a_specifier_is_located_inside_its_statement() {
        let source = "import { a } from '../shared/x';\n";
        let found = locate(source, 0..32, "../shared/x").expect("located");

        assert_eq!(&source[found.start..found.end], "../shared/x");
    }

    /// Double quotes, which is what a repository formatted by Biome uses.
    #[test]
    fn double_quotes_are_found_too() {
        let source = "import type { A } from \"@org/domain/email/x\";\n";
        let found = locate(source, 0..source.len(), "@org/domain/email/x").expect("located");

        assert_eq!(&source[found.start..found.end], "@org/domain/email/x");
    }

    /// A statement whose bytes no longer hold the specifier means the file on
    /// disk is not the file that was parsed. Editing by offset would corrupt
    /// it, so this returns nothing and the caller refuses.
    #[test]
    fn a_specifier_that_is_not_there_is_not_invented() {
        assert!(locate("import { a } from './y';\n", 0..24, "./x").is_none());
    }

    /// Back to front, so the first replacement does not shift the second.
    #[test]
    fn several_replacements_in_one_file_all_land() {
        let source = "import { a } from './one';\nimport { b } from './two';\n";
        let replacements = vec![
            locate(source, 0..26, "./one")
                .map(|mut r| {
                    r.now = "../moved/one".to_owned();
                    r
                })
                .expect("first"),
            locate(source, 27..53, "./two")
                .map(|mut r| {
                    r.now = "../moved/two".to_owned();
                    r
                })
                .expect("second"),
        ];

        assert_eq!(
            rewritten(source, &replacements),
            "import { a } from '../moved/one';\nimport { b } from '../moved/two';\n"
        );
    }

    /// Only the specifier changes. The quotes, the names, the `type` mark and
    /// the semicolon are the author's and stay exactly as written.
    #[test]
    fn nothing_but_the_specifier_is_touched() {
        let source = "import type { A, B } from \"../shared/types/feature-shared\";\n";
        let mut found =
            locate(source, 0..source.len(), "../shared/types/feature-shared").expect("located");
        found.now = "./feature-shared".to_owned();

        assert_eq!(
            rewritten(source, &[found]),
            "import type { A, B } from \"./feature-shared\";\n"
        );
    }

    /// The blind spot is the only refusal a flag may override. Everything else
    /// is a fact about the filesystem or an edit this cannot compute, and
    /// forcing past one of those produces a repository that does not build.
    #[test]
    fn only_the_dynamic_import_blind_spot_is_forceable() {
        assert!(Refusal::Opaque(vec![path("src/loader.ts")]).is_forceable());
        assert!(!Refusal::DirtyTree(vec!["M src/a.ts".to_owned()]).is_forceable());
        assert!(!Refusal::DestinationOccupied(path("src/b.ts")).is_forceable());
        assert!(
            !Refusal::UnreadableSpecifier {
                importer: path("src/a.ts"),
                specifier: "@Components/x".to_owned(),
                why: crate::respecify::Unknown::PathAlias,
            }
            .is_forceable()
        );
        // The one issue #11 added. `--force` was in the command that produced
        // the broken repository, so a refusal a flag can wave through would
        // have changed nothing about that outcome.
        assert!(
            !Refusal::UnresolvedLocalImport {
                importer: path("apps/app/src/a.ts"),
                specifier: "@scope/domain/email/shared/x".to_owned(),
            }
            .is_forceable()
        );
    }

    /// A file leaving its package is the case the warning exists for: a
    /// different `tsconfig` applies at the destination, so a path alias in the
    /// moved file can mean something else there.
    ///
    /// Untested until `cargo-mutants` could reach it. The computation predates
    /// this test and lived inline inside `plan`, where a mutant had nothing to
    /// replace; extracting it made the gap visible, and returning a constant
    /// `true` or `false` broke nothing.
    #[test]
    fn a_move_out_of_a_package_is_what_crosses_a_boundary() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8Path::from_path(dir.path()).expect("utf-8");
        std::fs::create_dir_all(root.join("packages/app")).expect("dirs");
        std::fs::write(
            root.join("packages/app/package.json"),
            r#"{"name":"@org/app"}"#,
        )
        .expect("write");
        let workspace = workspace_at(root);

        let moved = |from: &str, to: &str| Move {
            from: path(from),
            to: path(to),
            is_spec_sibling: false,
        };

        assert!(
            crosses_packages(
                &[moved(
                    "packages/domain/src/email/a.ts",
                    "packages/app/src/a.ts"
                )],
                &workspace
            ),
            "domain to app is two packages"
        );
        assert!(
            !crosses_packages(
                &[moved(
                    "packages/domain/src/email/a.ts",
                    "packages/domain/src/calcs/a.ts"
                )],
                &workspace
            ),
            "and moving within one is not, however far the file travels"
        );
        assert!(
            !crosses_packages(&[], &workspace),
            "nothing moving crosses nothing"
        );
    }

    /// The refusal has to name the file, the specifier, and what to do — it
    /// fires in the state where a workspace is not installed, which is a state
    /// the reader can fix in one command and will not guess at from "cannot
    /// resolve".
    #[test]
    fn the_unresolved_local_import_refusal_says_what_to_do_about_it() {
        let plan = Plan {
            refusals: vec![Refusal::UnresolvedLocalImport {
                importer: path("apps/app/src/a.ts"),
                specifier: "@scope/domain/email/shared/x".to_owned(),
            }],
            ..Plan::default()
        };
        let mut out = Vec::new();
        render_refusals(&plan, false, &mut out);
        let text = String::from_utf8(out).expect("utf-8");

        assert!(text.contains("nothing was moved"), "{text}");
        assert!(text.contains("apps/app/src/a.ts"), "{text}");
        assert!(text.contains("@scope/domain/email/shared/x"), "{text}");
        assert!(text.contains("install"), "the usual cause, named: {text}");
        assert!(text.contains("exports"), "and the other one: {text}");
    }

    /// The guard must never become forceable, and this test exists to make
    /// adding it to `is_forceable` a failing change rather than a quiet one.
    ///
    /// It fires when a file the dry run named as an importer came out of the
    /// plan with no edit — a state nothing is supposed to reach. A `--force`
    /// that got past it would carry out exactly the move it exists to stop:
    /// most imports rewritten, one left pointing at a file that moved, exit 0.
    /// That shipped once, in 0.5.0, for a different reason.
    #[test]
    fn the_importer_guard_can_never_be_forced() {
        let refusal = Refusal::ImporterNotRewritten {
            importer: path("apps/web/src/main.ts"),
            target: path("packages/domain/src/email/x.ts"),
        };
        assert!(!refusal.is_forceable());

        let plan = Plan {
            refusals: vec![refusal],
            ..Plan::default()
        };
        assert!(!plan.is_actionable(false));
        assert!(
            !plan.is_actionable(true),
            "no flag may carry out a move that would leave an import broken"
        );
    }

    /// And it says which file and which target, because the pair is the whole
    /// diagnosis — the bug report that produced this guard cost two rounds of
    /// investigation for want of exactly those two paths.
    #[test]
    fn the_importer_guard_names_the_file_and_the_target() {
        let plan = Plan {
            refusals: vec![Refusal::ImporterNotRewritten {
                importer: path("apps/web/src/main.ts"),
                target: path("packages/domain/src/email/x.ts"),
            }],
            ..Plan::default()
        };

        let mut out = Vec::new();
        render_refusals(&plan, true, &mut out);
        let text = String::from_utf8(out).expect("UTF-8");

        assert!(text.contains("nothing was moved"), "{text}");
        assert!(text.contains("apps/web/src/main.ts"), "{text}");
        assert!(text.contains("packages/domain/src/email/x.ts"), "{text}");
        assert!(
            text.contains("bug in archwarden"),
            "a state that cannot happen should say so rather than read as user error: {text}"
        );
    }

    /// A plan with an overridable refusal is actionable only with the flag,
    /// and one with any other refusal is not actionable at all.
    #[test]
    fn force_covers_the_blind_spot_and_nothing_else() {
        let opaque = Plan {
            refusals: vec![Refusal::Opaque(vec![path("src/loader.ts")])],
            ..Plan::default()
        };
        assert!(!opaque.is_actionable(false));
        assert!(opaque.is_actionable(true));

        let dirty = Plan {
            refusals: vec![Refusal::DirtyTree(vec!["M x".to_owned()])],
            ..Plan::default()
        };
        assert!(!dirty.is_actionable(true), "a dirty tree is never forced");
    }

    /// A workspace with one package, for the specifier tests below.
    fn workspace_at(root: &camino::Utf8Path) -> archwarden_resolver::workspace::Workspace {
        std::fs::create_dir_all(root.join("packages/domain")).expect("dirs");
        std::fs::write(
            root.join("packages/domain/package.json"),
            r#"{"name":"@org/domain","exports":{"./email/*":"./src/email/*.ts"}}"#,
        )
        .expect("write");
        archwarden_resolver::workspace::Workspace::discover(root)
    }

    fn facts_with(path_str: &str, imports: &[(&str, &str)]) -> archwarden_core::facts::FileFacts {
        use archwarden_core::facts::{ImportFact, Span};

        let mut offset = 0u32;
        archwarden_core::facts::FileFacts {
            path: path(path_str),
            content_hash: archwarden_core::hash::ContentHash::of(b""),
            imports: imports
                .iter()
                .map(|(specifier, resolved)| {
                    let line = format!("import {{ a }} from '{specifier}';\n");
                    let width = u32::try_from(line.len()).expect("a line fits in u32");
                    let span = Span::new(offset, offset + width);
                    offset += width;
                    ImportFact {
                        specifier: (*specifier).to_owned(),
                        resolved: Some(path(resolved)),
                        type_only: false,
                        names: vec!["a".to_owned()],
                        span,
                    }
                })
                .collect(),
            exports: Vec::new(),
            calls: Vec::new(),
            has_opaque_import: false,
        }
    }

    fn source_for(imports: &[(&str, &str)]) -> String {
        use std::fmt::Write as _;

        imports
            .iter()
            .fold(String::new(), |mut text, (specifier, _)| {
                let _ = writeln!(text, "import {{ a }} from '{specifier}';");
                text
            })
    }

    /// The case a batch move exists to get right, and the reason the rewriter
    /// asks one question instead of two: a file that moves *and* imports
    /// another file that moves. Both ends changed, and the answer depends on
    /// both.
    #[test]
    fn a_moved_file_importing_another_moved_file_is_measured_from_both_new_homes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root =
            camino::Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
                .expect("UTF-8");
        let workspace = workspace_at(&root);

        // `a.ts` sits in `shared/` and imports `b.ts` two levels up. Both move
        // into `calcs/`, where they become siblings.
        let imports = [("../../calcs/b", "packages/domain/src/order/calcs/b.ts")];
        let facts = facts_with("packages/domain/src/order/shared/calcs/a.ts", &imports);
        let source = source_for(&imports);

        let moves: Moves = [
            (
                path("packages/domain/src/order/shared/calcs/a.ts"),
                path("packages/domain/src/order/calcs/a.ts"),
            ),
            (
                path("packages/domain/src/order/calcs/b.ts"),
                path("packages/domain/src/order/calcs/b.ts"),
            ),
        ]
        .into_iter()
        .collect();

        let mut refusals = Vec::new();
        let replacements = replacements_for(
            &source,
            &path("packages/domain/src/order/shared/calcs/a.ts"),
            &facts,
            &moves,
            &workspace,
            &mut refusals,
        );
        drop(dir);

        assert!(refusals.is_empty(), "{refusals:?}");
        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].now, "./b", "siblings after the move");
    }

    /// An import neither end of which moved is left alone, which is most
    /// imports in most files and the reason a batch does not rewrite the world.
    #[test]
    fn an_import_untouched_by_the_move_produces_no_replacement() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root =
            camino::Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
                .expect("UTF-8");
        let workspace = workspace_at(&root);

        let imports = [("../types/thing", "packages/domain/src/order/types/thing.ts")];
        let facts = facts_with("packages/domain/src/order/calcs/x.ts", &imports);
        let source = source_for(&imports);

        let mut refusals = Vec::new();
        let replacements = replacements_for(
            &source,
            &path("packages/domain/src/order/calcs/x.ts"),
            &facts,
            &Moves::new(),
            &workspace,
            &mut refusals,
        );
        drop(dir);

        assert!(replacements.is_empty());
        assert!(refusals.is_empty());
    }

    /// A specifier that resolves into the repository through a map this does
    /// not read refuses, rather than being skipped. Skipping it is what leaves
    /// an import pointing at a file that moved.
    #[test]
    fn a_specifier_that_cannot_be_recomputed_refuses() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root =
            camino::Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
                .expect("UTF-8");
        let workspace = workspace_at(&root);

        // A `tsconfig` path alias: it resolved to the moving file, and nothing
        // here can work out what it should say instead.
        let imports = [("@Components/email", "packages/domain/src/email/x.ts")];
        let facts = facts_with("apps/web/src/main.ts", &imports);
        let source = source_for(&imports);

        let moves: Moves = [(
            path("packages/domain/src/email/x.ts"),
            path("packages/domain/src/email/calcs/x.ts"),
        )]
        .into_iter()
        .collect();

        let mut refusals = Vec::new();
        let replacements = replacements_for(
            &source,
            &path("apps/web/src/main.ts"),
            &facts,
            &moves,
            &workspace,
            &mut refusals,
        );
        drop(dir);

        assert!(replacements.is_empty());
        assert_eq!(
            refusals,
            [Refusal::UnreadableSpecifier {
                importer: path("apps/web/src/main.ts"),
                specifier: "@Components/email".to_owned(),
                why: crate::respecify::Unknown::PathAlias,
            }]
        );
    }

    /// The file on disk is not the file that was parsed. Editing by offset
    /// would corrupt it, so it refuses.
    #[test]
    fn a_specifier_missing_from_the_bytes_refuses() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root =
            camino::Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
                .expect("UTF-8");
        let workspace = workspace_at(&root);

        let imports = [("../old/x", "packages/domain/src/order/old/x.ts")];
        let facts = facts_with("packages/domain/src/order/calcs/y.ts", &imports);

        let moves: Moves = [(
            path("packages/domain/src/order/old/x.ts"),
            path("packages/domain/src/order/calcs/x.ts"),
        )]
        .into_iter()
        .collect();

        let mut refusals = Vec::new();
        let replacements = replacements_for(
            // Someone edited the file between the parse and this call.
            "import { a } from './something-else';\n",
            &path("packages/domain/src/order/calcs/y.ts"),
            &facts,
            &moves,
            &workspace,
            &mut refusals,
        );
        drop(dir);

        assert!(replacements.is_empty());
        assert!(
            matches!(refusals.as_slice(), [Refusal::SpecifierNotFound { .. }]),
            "{refusals:?}"
        );
    }

    /// Two files landing on one path. Caught before anything is written,
    /// because carrying it out would silently delete one of them -- which a
    /// batch move with a flattening destination can produce from a glob that
    /// looks perfectly reasonable.
    #[test]
    fn two_files_landing_on_one_path_are_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root =
            camino::Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
                .expect("UTF-8");
        std::fs::create_dir_all(root.join("src/a")).expect("dirs");
        std::fs::create_dir_all(root.join("src/b")).expect("dirs");
        std::fs::write(root.join("src/a/thing.ts"), "").expect("write");
        std::fs::write(root.join("src/b/thing.ts"), "").expect("write");

        let moves = vec![
            Move {
                from: path("src/a/thing.ts"),
                to: path("src/thing.ts"),
                is_spec_sibling: false,
            },
            Move {
                from: path("src/b/thing.ts"),
                to: path("src/thing.ts"),
                is_spec_sibling: false,
            },
        ];
        let mut refusals = Vec::new();
        validate(&root, &moves, &mut refusals);
        drop(dir);

        assert_eq!(
            refusals,
            [Refusal::Collision {
                destination: path("src/thing.ts"),
                sources: vec![path("src/a/thing.ts"), path("src/b/thing.ts")],
            }]
        );
    }

    /// A destination that already exists, and a source that is not there.
    /// Both would destroy something if carried out.
    #[test]
    fn an_occupied_destination_and_a_missing_source_are_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root =
            camino::Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
                .expect("UTF-8");
        std::fs::create_dir_all(root.join("src")).expect("dirs");
        std::fs::write(root.join("src/here.ts"), "").expect("write");

        let moves = vec![Move {
            from: path("src/gone.ts"),
            to: path("src/here.ts"),
            is_spec_sibling: false,
        }];
        let mut refusals = Vec::new();
        validate(&root, &moves, &mut refusals);
        drop(dir);

        assert!(
            refusals.contains(&Refusal::SourceMissing(path("src/gone.ts"))),
            "{refusals:?}"
        );
        assert!(
            refusals.contains(&Refusal::DestinationOccupied(path("src/here.ts"))),
            "{refusals:?}"
        );
    }

    /// Case 4: the spec travels with its unit file, or the move breaks
    /// archwarden's own `spec-pair` rule.
    #[test]
    fn a_spec_sibling_is_found_and_follows_the_rename() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root =
            camino::Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
                .expect("UTF-8");
        std::fs::create_dir_all(root.join("src/id/shared")).expect("dirs");
        std::fs::write(root.join("src/id/shared/is-id-invalid-shared.ts"), "").expect("write");
        std::fs::write(root.join("src/id/shared/is-id-invalid-shared.spec.ts"), "").expect("write");

        let markers = vec!["spec".to_owned(), "test".to_owned()];
        let (spec, suffix) = spec_sibling(
            &root,
            &path("src/id/shared/is-id-invalid-shared.ts"),
            &markers,
        )
        .expect("spec found");

        assert_eq!(spec.as_str(), "src/id/shared/is-id-invalid-shared.spec.ts");
        assert_eq!(
            spec_destination(&path("src/id/calcs/is-id-invalid.ts"), &suffix)
                .expect("destination")
                .as_str(),
            "src/id/calcs/is-id-invalid.spec.ts",
            "the spec follows the rename, not just the folder"
        );
    }

    /// A unit file with no spec beside it moves alone, without inventing one.
    #[test]
    fn a_file_with_no_spec_moves_alone() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root =
            camino::Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
                .expect("UTF-8");
        std::fs::create_dir_all(root.join("src")).expect("dirs");
        std::fs::write(root.join("src/thing.ts"), "").expect("write");

        assert!(spec_sibling(&root, &path("src/thing.ts"), &["spec".to_owned()]).is_none());
    }
}
