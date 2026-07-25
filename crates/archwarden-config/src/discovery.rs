//! Finding and reading `arch.config.json`.
//!
//! Discovery walks *up* from the working directory, the way `git` finds `.git`
//! and `biome` finds `biome.json`. The consequence is deliberate: running
//! archwarden inside a subpackage of a monorepo still analyses the whole
//! monorepo, through the root config. One config per repository is the
//! intended model. See decision 4.

use camino::{Utf8Path, Utf8PathBuf};

use crate::config::Config;

/// The file archwarden looks for.
pub const CONFIG_FILE_NAME: &str = "arch.config.json";

/// Why a config could not be found, read, or parsed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoadError {
    /// No `arch.config.json` between the starting directory and the root.
    #[error("no `{CONFIG_FILE_NAME}` found in `{started_at}` or any parent directory")]
    NotFound {
        /// Where the upward search began.
        started_at: Utf8PathBuf,
    },

    /// The file exists but could not be read.
    #[error("could not read `{path}`")]
    Unreadable {
        /// The file.
        path: Utf8PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The file was read but is not valid according to the schema.
    ///
    /// Carries the source text so the caller can render the offending span.
    #[error("`{path}` is not a valid archwarden config")]
    Invalid {
        /// The file.
        path: Utf8PathBuf,
        /// The file's contents, for span rendering.
        source_text: String,
        /// Where in the document the failure was, one step per level. Empty
        /// when the failure was at the root.
        ///
        /// Structured rather than a dotted string because a caller resolves it
        /// against the document to find the exact span, and a rendered path
        /// cannot be walked back: a key containing `.` or `[` would be
        /// indistinguishable from the punctuation that separates segments.
        ///
        /// This exists because `serde_json`'s own line and column are only
        /// trustworthy for syntax errors: for a schema violation the parser
        /// has usually moved past the offending value by the time it reports,
        /// so the position lands on the following token.
        segments: Vec<PathSegment>,
        /// What serde objected to.
        #[source]
        source: serde_json::Error,
    },

    /// A path was not valid UTF-8. archwarden works in UTF-8 paths throughout.
    #[error("`{path}` is not valid UTF-8")]
    NonUtf8Path {
        /// The path, lossily rendered.
        path: String,
    },
}

/// A config together with where it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConfig {
    /// The parsed config.
    pub config: Config,
    /// The config file itself.
    pub path: Utf8PathBuf,
    /// The directory globs resolve from: the config's `root` if it set one,
    /// otherwise the directory holding the config file.
    pub root: Utf8PathBuf,
}

/// Searches for `arch.config.json` from `start` upwards.
///
/// The first match wins, so the nearest config to the working directory is the
/// one used.
///
/// # Errors
/// [`LoadError::NotFound`] when the filesystem root is reached without a hit.
pub fn discover(start: &Utf8Path) -> Result<Utf8PathBuf, LoadError> {
    for directory in start.ancestors() {
        let candidate = directory.join(CONFIG_FILE_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(LoadError::NotFound {
        started_at: start.to_owned(),
    })
}

/// Parses config text, recording which field failed.
///
/// Goes through `serde_path_to_error` rather than calling `serde_json`
/// directly: on its own, a schema violation says only what was wrong, and the
/// position it reports points past the offending value. The path is what tells
/// the user *which* of thirty rules to look at.
///
/// # Errors
/// [`LoadError::Invalid`].
pub fn parse(path: &Utf8Path, source_text: &str) -> Result<Config, LoadError> {
    let deserializer = &mut serde_json::Deserializer::from_str(source_text);

    serde_path_to_error::deserialize(deserializer).map_err(|error| LoadError::Invalid {
        path: path.to_owned(),
        source_text: source_text.to_owned(),
        segments: segments_of(error.path()),
        source: error.into_inner(),
    })
}

/// One step of the way to a failure.
///
/// `serde_path_to_error` has richer segments than this; the two that survive
/// are the two a JSON document can be walked by. `Enum` and `Unknown` describe
/// Rust's shape rather than the file's -- an internally tagged enum has no
/// nesting in the document -- so following them would point at nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathSegment {
    /// An object member, by name.
    Key(String),
    /// An array element, by position.
    Index(usize),
}

fn segments_of(path: &serde_path_to_error::Path) -> Vec<PathSegment> {
    path.iter()
        .filter_map(|segment| match segment {
            serde_path_to_error::Segment::Seq { index } => Some(PathSegment::Index(*index)),
            serde_path_to_error::Segment::Map { key } => Some(PathSegment::Key(key.clone())),
            _ => None,
        })
        .collect()
}

/// A path rendered the way `serde_path_to_error` renders one, for a message.
#[must_use]
pub fn render_path(segments: &[PathSegment]) -> String {
    use std::fmt::Write as _;

    let mut rendered = String::new();
    for segment in segments {
        match segment {
            PathSegment::Key(key) => {
                if !rendered.is_empty() {
                    rendered.push('.');
                }
                rendered.push_str(key);
            }
            PathSegment::Index(index) => {
                let _ = write!(rendered, "[{index}]");
            }
        }
    }
    rendered
}

/// Reads and parses a config file.
///
/// # Errors
/// [`LoadError::Unreadable`] or [`LoadError::Invalid`].
pub fn load_file(path: &Utf8Path) -> Result<LoadedConfig, LoadError> {
    let source_text = std::fs::read_to_string(path).map_err(|source| LoadError::Unreadable {
        path: path.to_owned(),
        source,
    })?;

    let config = parse(path, &source_text)?;

    let containing_directory = path.parent().unwrap_or(Utf8Path::new("")).to_owned();
    let root = match &config.root {
        Some(declared) => containing_directory.join(declared),
        None => containing_directory,
    };

    Ok(LoadedConfig {
        config,
        path: path.to_owned(),
        root,
    })
}

/// Discovers and loads in one step.
///
/// # Errors
/// Any [`LoadError`].
pub fn load_from(start: &Utf8Path) -> Result<LoadedConfig, LoadError> {
    load_file(&discover(start)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"{"version": 0}"#;

    /// Builds a temporary directory tree and returns its UTF-8 root.
    fn tree(entries: &[(&str, &str)]) -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(
            // macOS hands back a symlinked /var path; canonicalising keeps the
            // assertions comparing like with like.
            dir.path().canonicalize().expect("canonicalise"),
        )
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

    #[test]
    fn a_config_in_the_starting_directory_is_found() {
        let (_guard, root) = tree(&[(CONFIG_FILE_NAME, MINIMAL)]);
        assert_eq!(discover(&root).expect("found"), root.join(CONFIG_FILE_NAME));
    }

    /// The monorepo case that decision 4 exists for: run from a subpackage,
    /// get the repository's config.
    #[test]
    fn the_search_walks_up_from_a_subdirectory() {
        let (_guard, root) = tree(&[
            (CONFIG_FILE_NAME, MINIMAL),
            ("packages/domain/src/user.ts", "export const a = 1;"),
        ]);

        let deep = root.join("packages/domain/src");
        assert_eq!(discover(&deep).expect("found"), root.join(CONFIG_FILE_NAME));
    }

    /// The first match going up wins, so a nested config shadows the one above
    /// it rather than the other way round.
    #[test]
    fn the_nearest_config_wins() {
        let (_guard, root) = tree(&[
            (CONFIG_FILE_NAME, MINIMAL),
            ("packages/app/arch.config.json", MINIMAL),
        ]);

        let nested = root.join("packages/app");
        assert_eq!(
            discover(&nested).expect("found"),
            nested.join(CONFIG_FILE_NAME)
        );
    }

    /// The error names where the search began, which is the one thing the user
    /// needs in order to understand why nothing was found.
    #[test]
    fn a_missing_config_reports_where_the_search_started() {
        let (_guard, root) = tree(&[("packages/app/src/x.ts", "")]);
        let deep = root.join("packages/app/src");

        let err = discover(&deep).expect_err("nothing to find");
        let LoadError::NotFound { started_at } = &err else {
            panic!("expected NotFound, got {err:?}");
        };
        assert_eq!(started_at, &deep);
        assert!(err.to_string().contains(CONFIG_FILE_NAME), "{err}");
    }

    /// A directory named `arch.config.json` is not a config. Without the
    /// file check the search would stop there and then fail confusingly on
    /// read.
    #[test]
    fn a_directory_with_the_config_name_is_not_a_config() {
        let (_guard, root) = tree(&[(CONFIG_FILE_NAME, MINIMAL)]);
        let nested = root.join("packages/app");
        std::fs::create_dir_all(nested.join(CONFIG_FILE_NAME)).expect("create dir");

        assert_eq!(
            discover(&nested).expect("found"),
            root.join(CONFIG_FILE_NAME)
        );
    }

    #[test]
    fn loading_yields_the_config_and_where_it_came_from() {
        let (_guard, root) = tree(&[(CONFIG_FILE_NAME, MINIMAL)]);
        let loaded = load_from(&root).expect("loads");

        assert_eq!(loaded.path, root.join(CONFIG_FILE_NAME));
        assert!(loaded.config.version_is_supported());
    }

    /// Globs resolve from the config file's directory by default, which is
    /// what makes a config portable between checkouts.
    #[test]
    fn root_defaults_to_the_config_files_directory() {
        let (_guard, root) = tree(&[(CONFIG_FILE_NAME, MINIMAL)]);
        assert_eq!(load_from(&root).expect("loads").root, root);
    }

    /// An explicit `root` is resolved relative to the config, not to the
    /// working directory, so where you invoke archwarden from cannot change
    /// what the globs mean.
    #[test]
    fn an_explicit_root_is_resolved_against_the_config_file() {
        let (_guard, root) =
            tree(&[("config/arch.config.json", r#"{"version": 0, "root": ".."}"#)]);

        let loaded = load_file(&root.join("config/arch.config.json")).expect("loads");
        assert_eq!(loaded.root, root.join("config").join(".."));
    }

    /// A malformed config carries its own text back, so the caller can point
    /// at the offending span rather than printing a bare parser message.
    #[test]
    fn a_malformed_config_reports_the_file_and_keeps_its_text() {
        let (_guard, root) = tree(&[(CONFIG_FILE_NAME, r#"{"version": 0,,}"#)]);

        let err = load_from(&root).expect_err("should fail");
        let LoadError::Invalid {
            path, source_text, ..
        } = &err
        else {
            panic!("expected Invalid, got {err:?}");
        };
        assert_eq!(path, &root.join(CONFIG_FILE_NAME));
        assert!(source_text.contains("version"), "text is carried back");
    }

    /// Returns the failure path of a config that should not parse.
    fn failing_path(source_text: &str) -> Vec<PathSegment> {
        match parse(Utf8Path::new("arch.config.json"), source_text) {
            Err(LoadError::Invalid { segments, .. }) => segments,
            other => panic!("expected an Invalid, got {other:?}"),
        }
    }

    /// The path is the point of routing parsing through `serde_path_to_error`.
    /// `serde_json`'s own position cannot be trusted for a schema violation, so
    /// "which of the thirty rules" has to come from somewhere else.
    #[test]
    fn a_schema_violation_records_which_field_failed() {
        let segments = failing_path(
            r#"{"version":0,"rules":[
                {"type":"structure","id":"fine","level":"error","roots":"a/*"},
                {"type":"structure","id":"bad id","level":"error","roots":"b/*"}]}"#,
        );

        assert_eq!(
            segments,
            [PathSegment::Key("rules".to_owned()), PathSegment::Index(1)]
        );
        assert_eq!(render_path(&segments), "rules[1]");
    }

    /// A failure at the document root has no useful path, and `.` is noise in
    /// a message rather than information.
    #[test]
    fn a_root_level_failure_records_an_empty_path() {
        let segments = failing_path("[]");

        assert!(segments.is_empty(), "got {segments:?}");
        assert!(render_path(&segments).is_empty());
    }

    /// The path is structured rather than a string because a caller walks it
    /// through the document to find a span, and a rendered path cannot be
    /// walked back: a key containing `.` or `[` is indistinguishable from the
    /// punctuation between segments.
    #[test]
    fn every_step_is_one_segment() {
        let segments =
            failing_path(r#"{"version":0,"modules":[{"id":"not a valid id","rules":[]}]}"#);

        assert_eq!(
            segments,
            [
                PathSegment::Key("modules".to_owned()),
                PathSegment::Index(0),
                PathSegment::Key("id".to_owned()),
            ],
            "a step is a step, and the punctuation between them is the \
             renderer's business"
        );
        assert_eq!(render_path(&segments), "modules[0].id");
    }

    /// **The path stops at the rule.** `Rule` is an internally tagged enum
    /// (`#[serde(tag = "type")]`), and serde deserialises one of those by
    /// buffering the object first and reading the variant out of the buffer.
    /// The path tracker cannot follow across that boundary, so a failure on a
    /// field *inside* a rule is reported at the rule.
    ///
    /// This is the second thing `tag = "type"` has cost: it also ruled out
    /// every non-self-describing cache format in M4. Both are worth it -- the
    /// tag is what makes the JSON report a contract an agent can read -- but
    /// the price is real and is recorded here so the next reader does not
    /// spend an afternoon looking for the bug.
    ///
    /// A caret still lands: on the whole rule for most failures, and on the
    /// exact word for an unknown field, whose name the message carries.
    #[test]
    fn a_failure_inside_a_rule_is_reported_at_the_rule() {
        assert_eq!(
            render_path(&failing_path(
                r#"{"version":0,"rules":[
                    {"type":"naming","id":"n","level":"error","roots":"a/*",
                     "file_pattern":"^x$","must_export":{"name":"X","kind":42}}]}"#,
            )),
            "rules[0]",
            "not `rules[0].must_export.kind`"
        );
        assert_eq!(
            render_path(&failing_path(
                r#"{"version":0,"rules":[
                    {"type":"structure","id":"a","level":"nope","roots":"a/*"}]}"#,
            )),
            "rules[0]"
        );
    }

    /// Outside a rule the path is as deep as the document, which is what makes
    /// the caret exact everywhere else.
    #[test]
    fn a_failure_outside_a_rule_keeps_every_step() {
        assert_eq!(
            render_path(&failing_path(
                r#"{"version":0,"skip_dirs":{"prefixes":"not an array"}}"#,
            )),
            "skip_dirs.prefixes"
        );
    }

    /// A config that parses as JSON but is not a config is still Invalid, not
    /// a panic or a default.
    #[test]
    fn valid_json_that_is_not_a_config_is_rejected() {
        let (_guard, root) = tree(&[(CONFIG_FILE_NAME, r#"{"rules": "not an array"}"#)]);
        assert!(matches!(load_from(&root), Err(LoadError::Invalid { .. })));
    }

    #[test]
    fn an_unreadable_file_is_distinguished_from_an_invalid_one() {
        let (_guard, root) = tree(&[("x.ts", "")]);
        let missing = root.join("does-not-exist.json");

        assert!(matches!(
            load_file(&missing),
            Err(LoadError::Unreadable { .. })
        ));
    }
}
