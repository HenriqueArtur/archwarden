//! Tier 3: archwarden's import graph against `dependency-cruiser`'s.
//!
//! Decision 7 says archwarden owns its graph rather than delegating to
//! dependency-cruiser. This is the price of that decision: a repository is
//! cruised by both tools and the two answers are diffed, so the edge cases a
//! decade of dependency-cruiser use has surfaced show up as failures here
//! instead of as a user's bug report.
//!
//! Nothing is imported from dependency-cruiser. It is run as a subprocess and
//! its JSON is read, which is the reimplementation-only policy in
//! `docs/TESTING.md`.
//!
//! # Running it
//!
//! ```text
//! ARCHWARDEN_DIFF_REPO=/path/to/repo \
//!   cargo test -p archwarden-engine --features differential
//! ```
//!
//! The repository must have `dependency-cruiser` and TypeScript 5 installed
//! (they are how a real project runs it), or `ARCHWARDEN_DEPCRUISE` must point
//! at a `depcruise` binary. Target repositories are configured by environment
//! and never checked in.
//!
//! Without `ARCHWARDEN_DIFF_REPO` the test prints why it did nothing and
//! passes. A differential test cannot invent a repository to differentiate
//! against, and failing for a missing one would just teach people to ignore
//! it.

#![cfg(feature = "differential")]
// clippy's `allow-*-in-tests` relaxations key off `#[cfg(test)]` modules and
// `#[test]` functions. The helpers below are neither -- they are plain
// functions in an integration-test crate -- so the relaxation is spelled out
// here instead. This whole file is test code: a panic here *is* the failure
// being reported, and `value["key"]` on a `serde_json::Value` is how that API
// reads.
#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::print_stdout
)]

use std::collections::{BTreeMap, BTreeSet};

use archwarden_core::{
    compiled::{CompiledConfig, SkipDirs},
    glob::PathSet,
    hash::ContentHash,
    path::{FileClass, RepoRelPath},
    traits::{Parser as _, Resolver as _},
};
use archwarden_engine::walk;
use archwarden_resolver::imports::ImportResolver;
use camino::{Utf8Path, Utf8PathBuf};

/// The repository to cruise. Absent means "nothing to do".
const REPO: &str = "ARCHWARDEN_DIFF_REPO";
/// An explicit `depcruise` binary, when it is not the repository's own.
const BINARY: &str = "ARCHWARDEN_DEPCRUISE";
/// Directories to cruise, comma-separated. Defaults to the repository root.
const DIRS: &str = "ARCHWARDEN_DIFF_DIRS";

/// One edge: who imported what, and where it landed.
type Edges = BTreeMap<(String, String), String>;

/// What one cruise said.
struct Cruise {
    /// Edges dependency-cruiser placed.
    edges: Edges,
    /// Pairs it saw and admitted it could not place.
    ///
    /// Kept apart because they are an admission of ignorance, not an assertion
    /// of absence. Diffing against one produces noise, not signal.
    unresolved: BTreeSet<(String, String)>,
}

/// An unset target and one set to the empty string are the same state, and
/// the second is the one CI produces.
#[test]
fn an_unconfigured_target_is_not_a_failure() {
    assert_eq!(configured(None), None);
    assert_eq!(
        configured(Some(String::new())),
        None,
        "GitHub's substitution"
    );
    assert_eq!(configured(Some("   ".to_owned())), None);
    assert_eq!(
        configured(Some("/repos/target".to_owned())),
        Some("/repos/target".to_owned())
    );
}

#[test]
fn archwarden_and_dependency_cruiser_agree_about_the_graph() {
    let Some(repo) = target() else {
        println!("skipped: set {REPO} to a repository to cruise");
        return;
    };

    let cruise = dependency_cruiser(&repo);
    let theirs = cruise.edges;
    let ours = archwarden(&repo);
    println!(
        "dependency-cruiser: {} edges ({} unresolved) · archwarden: {} edges",
        theirs.len(),
        cruise.unresolved.len(),
        ours.len()
    );
    assert!(
        !theirs.is_empty(),
        "dependency-cruiser found no edges in {repo}; the target is probably \
         misconfigured, and an empty comparison would pass for the wrong reason"
    );

    let known = known_divergences();
    let mut divergences = Vec::new();

    for (edge, resolved) in &theirs {
        match ours.get(edge) {
            Some(landed) if landed == resolved => {}
            Some(landed) => divergences.push(format!(
                "`{}` -> `{}`: archwarden says `{landed}`, dependency-cruiser says `{resolved}`",
                edge.0, edge.1
            )),
            None => divergences.push(format!(
                "`{}` -> `{}`: dependency-cruiser resolved it to `{resolved}`, archwarden did not see it",
                edge.0, edge.1
            )),
        }
    }

    for (edge, resolved) in &ours {
        if theirs.contains_key(edge) {
            continue;
        }
        // dependency-cruiser saw this specifier and said it could not place
        // it. That is not evidence archwarden is wrong -- it is a place
        // archwarden resolves more than the reference does, and treating an
        // admission of ignorance as a contradiction would make the harness
        // fail for being better.
        if cruise.unresolved.contains(edge) {
            println!(
                "  more than the reference: `{}` -> `{}` = `{resolved}` (dependency-cruiser could not resolve it)",
                edge.0, edge.1
            );
            continue;
        }
        divergences.push(format!(
            "`{}` -> `{}`: archwarden resolved it to `{resolved}`, dependency-cruiser did not see it",
            edge.0, edge.1
        ));
    }

    let unexplained: Vec<_> = divergences
        .iter()
        .filter(|line| !known.iter().any(|entry| line.starts_with(entry)))
        .collect();

    assert!(
        unexplained.is_empty(),
        "{} unexplained divergence(s):\n{}\n\nDecide which side is right. If it \
         is archwarden, record it in tests/differential/known-divergences.md \
         with the reasoning.",
        unexplained.len(),
        unexplained
            .iter()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The repository to cruise, canonicalised.
fn target() -> Option<Utf8PathBuf> {
    let raw = configured(std::env::var(REPO).ok())?;
    let path = std::path::Path::new(&raw)
        .canonicalize()
        .unwrap_or_else(|error| panic!("{REPO}=`{raw}` is not a readable path: {error}"));

    Some(Utf8PathBuf::from_path_buf(path).expect("the repository path is UTF-8"))
}

/// A configured value, where empty means *not* configured.
///
/// GitHub Actions substitutes an unset repository variable as the empty
/// string, so a workflow that passes one through sets this variable to the
/// empty string rather than leaving it unset. `env::var` then succeeds, the
/// empty path fails to canonicalise, and the job panicked saying the path was
/// not readable -- every night, for the six days anybody has records of, while
/// the file above promised that a missing target "prints why it did nothing
/// and passes".
///
/// Which is the argument for running this on CI rather than nightly, in one
/// sentence: nobody opened the report.
fn configured(raw: Option<String>) -> Option<String> {
    raw.map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Runs `depcruise` and reads the edges out of its JSON.
fn dependency_cruiser(repo: &Utf8Path) -> Cruise {
    let binary = std::env::var(BINARY).map_or_else(
        |_| repo.join("node_modules/.bin/depcruise"),
        Utf8PathBuf::from,
    );
    assert!(
        binary.is_file(),
        "no depcruise at `{binary}`. Install dependency-cruiser in the target \
         repository, or point {BINARY} at one"
    );

    let dirs = std::env::var(DIRS).unwrap_or_else(|_| ".".to_owned());
    let mut command = std::process::Command::new(binary.as_std_path());
    command
        .current_dir(repo.as_std_path())
        .args(["--no-config", "--output-type", "json"]);
    if repo.join("tsconfig.json").is_file() {
        command.args(["--ts-config", "tsconfig.json"]);
    }
    // Without this, a `import type` is invisible to dependency-cruiser and
    // every type-only edge would look like a divergence.
    command.arg("--ts-pre-compilation-deps");
    command.args(dirs.split(','));

    let output = command.output().expect("depcruise runs");
    assert!(
        output.status.success(),
        "depcruise failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    parse_cruise(&String::from_utf8(output.stdout).expect("depcruise emits UTF-8"))
}

/// Pulls `(importer, specifier) -> resolved` out of a cruise result.
///
/// Filtered to in-repository edges; the reasoning for every filter is in
/// `tests/differential/known-divergences.md`.
fn parse_cruise(json: &str) -> Cruise {
    let report: serde_json::Value = serde_json::from_str(json).expect("depcruise emits JSON");
    let modules = report["modules"].as_array().expect("a modules array");

    let mut edges = Edges::new();
    let mut unresolved = BTreeSet::new();
    for module in modules {
        let Some(source) = module["source"].as_str() else {
            continue;
        };
        if source.contains("node_modules") || source.starts_with("..") {
            continue;
        }
        for dependency in module["dependencies"].as_array().into_iter().flatten() {
            let kinds: BTreeSet<&str> = dependency["dependencyTypes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .collect();

            let Some(specifier) = dependency["module"].as_str() else {
                continue;
            };
            if dependency["couldNotResolve"].as_bool() == Some(true) {
                unresolved.insert((source.to_owned(), specifier.to_owned()));
                continue;
            }

            // `local` is dependency-cruiser's word for "landed on a file in
            // this project". Everything else -- a package, a builtin -- has no
            // repo-relative path, and archwarden's side is filtered the same
            // way.
            if !kinds.contains("local") {
                continue;
            }

            let Some(resolved) = dependency["resolved"].as_str() else {
                continue;
            };
            if resolved.contains("node_modules") || resolved.starts_with("..") {
                continue;
            }

            edges.insert(
                (source.to_owned(), specifier.to_owned()),
                resolved.to_owned(),
            );
        }
    }
    Cruise { edges, unresolved }
}

/// Walks, parses and resolves the repository the way a run would.
fn archwarden(repo: &Utf8Path) -> Edges {
    let config = CompiledConfig::new(
        Vec::new(),
        PathSet::default(),
        SkipDirs::default(),
        ContentHash::of(b"differential"),
    );
    let tree = walk::walk(repo, &config).expect("the repository walks");
    let resolver = ImportResolver::new(repo);

    let mut edges = Edges::new();
    for (_, directory) in tree.directories() {
        for file in &directory.files {
            if file.class != FileClass::Source {
                continue;
            }
            for (specifier, resolved) in imports_of(repo, &file.path, &resolver) {
                edges.insert((file.path.as_str().to_owned(), specifier), resolved);
            }
        }
    }
    edges
}

/// One file's in-repository import edges.
fn imports_of(
    repo: &Utf8Path,
    path: &RepoRelPath,
    resolver: &ImportResolver,
) -> Vec<(String, String)> {
    let Ok(source) = std::fs::read_to_string(repo.join(path.as_path())) else {
        return Vec::new();
    };
    let content = ContentHash::of(source.as_bytes());
    let Ok(facts) = archwarden_parser::oxc::OxcParser.parse(path, &source, content) else {
        return Vec::new();
    };

    facts
        .imports
        .iter()
        .filter_map(|import| {
            match resolver.resolve(path, &import.specifier) {
                Ok(archwarden_core::traits::Resolved::InRepo(landed)) => {
                    Some((import.specifier.clone(), landed.as_str().to_owned()))
                }
                // Everything else has no repo-relative path, and the cruise
                // side is filtered the same way.
                _ => None,
            }
        })
        .collect()
}

/// The divergence headings recorded as decided.
///
/// The markdown is the source of truth: a list in code would drift from the
/// rationale beside it, and the rationale is the only reason the file exists.
fn known_divergences() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/differential/known-divergences.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));

    text.lines()
        .filter_map(|line| line.strip_prefix("### "))
        .filter(|heading| heading.contains("` -> `"))
        .map(str::to_owned)
        .collect()
}
