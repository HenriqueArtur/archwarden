//! Turning one source argument into the list of moves it names.
//!
//! `impact <file> --to <file>` is one move and `--to` is where it goes. A
//! directory or a glob is a batch, and `--to` becomes relative to each match:
//!
//! ```text
//! archwarden impact 'packages/domain/src/*/shared' --to '../calcs'
//! ```
//!
//! is nine moves, one per entity, each landing beside where it started. A
//! refactor of an architecture is never one file, and spelling out nine
//! source-destination pairs by hand is where the mistakes come from.
//!
//! # Why `--to` is relative in a batch and absolute for one file
//!
//! Because they are different questions. Moving one file is "put this exactly
//! there", and any other reading would take away the ability to rename during
//! a move. Moving a set is "do the same thing to each of these", where a
//! single absolute destination could only mean a collision.
//!
//! The distinction is made by the source, not by a flag, and the source says
//! which it is: a glob character or an existing directory means a set.

use archwarden_core::{compiled::CompiledConfig, path::RepoRelPath};
use archwarden_engine::walk::RepoTree;

/// One move, as source and destination.
pub type Request = (RepoRelPath, RepoRelPath);

/// Every move `source --to destination` names.
///
/// # Errors
/// A message when a path cannot be read as repo-relative, or when a
/// destination relative to a match climbs out of the repository.
pub fn expand(
    root: &camino::Utf8Path,
    working_directory: &camino::Utf8Path,
    tree: &RepoTree,
    source: &str,
    destination: &str,
) -> Result<Vec<Request>, String> {
    let matched = match sources(root, working_directory, tree, source)? {
        // One file, named exactly: `--to` is the whole destination path, which
        // is what makes a rename during a move expressible at all.
        Sources::One(file) => {
            let to = crate::describe::repo_relative(root, working_directory, destination)?;
            return Ok(vec![(file, to)]);
        }
        Sources::Many(matched) => matched,
    };

    let mut requests = Vec::new();
    for (directory, file) in matched {
        let landing = relocate(&directory, destination)?;
        let name = file
            .file_name()
            .ok_or_else(|| format!("`{file}` has no filename"))?;
        let to = landing
            .join(name)
            .map_err(|error| format!("`{landing}/{name}`: {error}"))?;
        requests.push((file, to));
    }
    Ok(requests)
}

/// What a source argument named.
enum Sources {
    /// Exactly one file, named as a path.
    One(RepoRelPath),
    /// A set, as `(matched directory, file)` pairs. Possibly empty.
    ///
    /// The directory is carried alongside because `--to` is measured from it,
    /// not from each file: `'src/*/shared' --to '../calcs'` means "each
    /// `shared` becomes the `calcs` beside it", and measuring from a file two
    /// levels down inside one would land it somewhere nobody named.
    Many(Vec<(RepoRelPath, RepoRelPath)>),
}

fn sources(
    root: &camino::Utf8Path,
    working_directory: &camino::Utf8Path,
    tree: &RepoTree,
    source: &str,
) -> Result<Sources, String> {
    if crate::filter::looks_like_a_glob(source) {
        let set = archwarden_core::glob::PathSet::compile([source.to_owned()])
            .map_err(|error| error.to_string())?;

        let mut matched: Vec<(RepoRelPath, RepoRelPath)> = tree
            .files()
            .filter(|file| file.class == archwarden_core::path::FileClass::Source)
            .filter_map(|file| {
                let directory = matched_ancestor(&set, &file.path)?;
                Some((directory, file.path.clone()))
            })
            .collect();
        matched.sort();
        matched.dedup();
        return Ok(Sources::Many(matched));
    }

    let path = crate::describe::repo_relative(root, working_directory, source)?;
    if root.join(path.as_path()).is_dir() {
        let mut matched: Vec<(RepoRelPath, RepoRelPath)> = tree
            .files()
            .filter(|file| file.class == archwarden_core::path::FileClass::Source)
            .filter(|file| file.path.as_path().starts_with(path.as_path()))
            .map(|file| (path.clone(), file.path.clone()))
            .collect();
        matched.sort();
        return Ok(Sources::Many(matched));
    }

    Ok(Sources::One(path))
}

/// The nearest directory at or above `file` that the glob selects.
///
/// Nearest rather than outermost: a glob is written at the level the refactor
/// is about, and that is the level `--to` is measured from.
fn matched_ancestor(
    set: &archwarden_core::glob::PathSet,
    file: &RepoRelPath,
) -> Option<RepoRelPath> {
    let mut current = file.parent();
    while let Some(path) = current {
        if path.is_root() {
            return None;
        }
        if set.is_match(path.as_path()) {
            return Some(path);
        }
        current = path.parent();
    }
    None
}

/// Where a directory lands, given a destination written relative to it.
fn relocate(directory: &RepoRelPath, destination: &str) -> Result<RepoRelPath, String> {
    let mut parts: Vec<String> = directory
        .as_str()
        .split('/')
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    for part in destination.split('/').filter(|part| !part.is_empty()) {
        match part {
            "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(format!(
                        "`{destination}` climbs above the repository root from `{directory}`"
                    ));
                }
            }
            other => parts.push(other.to_owned()),
        }
    }

    RepoRelPath::new(parts.join("/")).map_err(|error| error.to_string())
}

/// The spec markers the configuration uses.
///
/// Taken from the `spec-pair` rules rather than hard-coded, so a move carries
/// the spec this repository actually writes. A configuration with no such rule
/// gets the same default the rule has.
#[must_use]
pub fn spec_markers(config: &CompiledConfig) -> Vec<String> {
    let mut markers: Vec<String> = config
        .rules()
        .filter_map(|rule| match &rule.kind {
            archwarden_core::compiled::CompiledRuleKind::SpecPair { spec_markers, .. } => {
                Some(spec_markers.clone())
            }
            _ => None,
        })
        .flatten()
        .collect();

    if markers.is_empty() {
        markers = vec!["spec".to_owned(), "test".to_owned()];
    }
    markers.sort();
    markers.dedup();
    markers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// The batch form's whole point: the destination is measured from the
    /// directory the glob matched, so `shared` becomes the `calcs` beside it.
    #[test]
    fn a_relative_destination_is_measured_from_the_matched_directory() {
        assert_eq!(
            relocate(&path("packages/domain/src/email/shared"), "../calcs")
                .expect("relocates")
                .as_str(),
            "packages/domain/src/email/calcs"
        );
    }

    /// And from the matched directory even for a file nested inside it. This
    /// is the case that made the distinction necessary:
    /// `feature/shared/consts/list-shared.ts` belongs in `feature/calcs`, and
    /// measuring from the file's own folder would land it in
    /// `feature/shared/calcs` — inside the very directory being emptied.
    #[test]
    fn a_file_nested_inside_the_match_still_lands_in_the_destination() {
        let set =
            archwarden_core::glob::PathSet::compile(["packages/domain/src/*/shared".to_owned()])
                .expect("valid glob");
        let matched = matched_ancestor(
            &set,
            &path("packages/domain/src/feature/shared/consts/list-shared.ts"),
        )
        .expect("matched");

        assert_eq!(matched.as_str(), "packages/domain/src/feature/shared");
        assert_eq!(
            relocate(&matched, "../calcs").expect("relocates").as_str(),
            "packages/domain/src/feature/calcs"
        );
    }

    /// Climbing out of the repository is refused rather than clamped: a
    /// destination outside it is a typo, and silently clamping would move
    /// files somewhere nobody named.
    #[test]
    fn a_destination_above_the_root_is_refused() {
        assert!(relocate(&path("src"), "../../../elsewhere").is_err());
    }

    /// The default when no rule says otherwise is the rule's own default,
    /// which is what vitest and jest both accept.
    #[test]
    fn the_markers_default_to_the_rules_own_default() {
        let config = CompiledConfig::new(
            Vec::new(),
            archwarden_core::glob::PathSet::default(),
            archwarden_core::compiled::SkipDirs::default(),
            archwarden_core::hash::ContentHash::of(b""),
        );

        assert_eq!(spec_markers(&config), ["spec", "test"]);
    }
}
