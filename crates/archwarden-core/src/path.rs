//! Repository-relative paths.
//!
//! Every path that crosses a stage boundary in archwarden is relative to the
//! repository root, uses `/` as its separator, and is normalised. Encoding
//! that in a type rather than a convention matters more here than in most
//! projects: rules match globs against paths, and a glob written with `/` in a
//! config silently fails to match a path carrying `\` on Windows. That bug
//! would show up as "the rule just doesn't fire", which is the worst possible
//! failure mode for a linter.

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

/// Why a path could not be made repository-relative.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PathError {
    /// The path was absolute. Callers must strip the repository root first.
    #[error("`{path}` is absolute; paths must be relative to the repository root")]
    Absolute {
        /// The path as given.
        path: String,
    },
    /// The path climbed above the repository root with `..`.
    #[error("`{path}` escapes the repository root")]
    EscapesRoot {
        /// The path as given.
        path: String,
    },
}

/// A normalised, repository-relative, `/`-separated path.
///
/// The repository root itself is the empty path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RepoRelPath(Utf8PathBuf);

impl RepoRelPath {
    /// Normalises and validates a path.
    ///
    /// Normalisation is lexical and never touches the filesystem: `.` and
    /// empty components are dropped, `..` is resolved against what precedes
    /// it, and `\` becomes `/`.
    ///
    /// # Errors
    /// See [`PathError`].
    pub fn new(raw: impl AsRef<str>) -> Result<Self, PathError> {
        let raw = raw.as_ref();
        let unified = raw.replace('\\', "/");

        if is_absolute(&unified) {
            return Err(PathError::Absolute {
                path: raw.to_owned(),
            });
        }

        let mut components: Vec<&str> = Vec::new();
        for component in unified.split('/') {
            match component {
                "" | "." => {}
                ".." => {
                    // Popping past the start would name something outside the
                    // repository, which this type cannot represent. Clamping
                    // to the root instead would turn a caller's bug into a
                    // silently wrong path.
                    if components.pop().is_none() {
                        return Err(PathError::EscapesRoot {
                            path: raw.to_owned(),
                        });
                    }
                }
                other => components.push(other),
            }
        }

        Ok(Self(Utf8PathBuf::from(components.join("/"))))
    }

    /// The repository root.
    #[must_use]
    pub fn root() -> Self {
        Self(Utf8PathBuf::new())
    }

    /// Whether this is the repository root.
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.0.as_str().is_empty()
    }

    /// Borrows the underlying path.
    #[must_use]
    pub fn as_path(&self) -> &Utf8Path {
        &self.0
    }

    /// The path as a `/`-separated string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The containing directory, or `None` for the root.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        if self.is_root() {
            return None;
        }
        Some(Self(
            self.0.parent().unwrap_or(Utf8Path::new("")).to_owned(),
        ))
    }

    /// Appends a component.
    ///
    /// Fallible because `name` is untrusted text like any other: a caller
    /// could pass `..`, and clamping that to the root would be the silent
    /// wrong answer this type exists to avoid. For the entry names a directory
    /// listing produces it never fails.
    ///
    /// # Errors
    /// See [`PathError`].
    pub fn join(&self, name: &str) -> Result<Self, PathError> {
        if self.is_root() {
            return Self::new(name);
        }
        Self::new(format!("{}/{name}", self.0))
    }

    /// The final component, or `None` for the root.
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.0.file_name()
    }
}

/// Recognises the three shapes of absolute path archwarden can meet: POSIX
/// (`/x`), a Windows drive (`C:/x`), and a UNC share (`//server/share`, which
/// has already been unified from `\\server\share`).
fn is_absolute(unified: &str) -> bool {
    if unified.starts_with('/') {
        return true;
    }
    let mut chars = unified.chars();
    matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(letter), Some(':'), Some('/')) if letter.is_ascii_alphabetic()
    )
}

impl std::fmt::Display for RepoRelPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<String> for RepoRelPath {
    type Error = PathError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<RepoRelPath> for String {
    fn from(value: RepoRelPath) -> Self {
        value.0.into_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(raw: &str) -> RepoRelPath {
        RepoRelPath::new(raw).expect("should be valid")
    }

    #[test]
    fn an_ordinary_relative_path_survives_unchanged() {
        assert_eq!(
            p("packages/domain/src/user.ts").as_str(),
            "packages/domain/src/user.ts"
        );
    }

    /// A config glob is written with `/`. A path carrying `\` would silently
    /// fail to match it, and the symptom would be a rule that never fires.
    #[test]
    fn backslashes_are_normalised_to_forward_slashes() {
        assert_eq!(p(r"packages\domain\src").as_str(), "packages/domain/src");
    }

    #[test]
    fn redundant_components_are_removed() {
        assert_eq!(
            p("./packages//domain/./src/").as_str(),
            "packages/domain/src"
        );
    }

    /// `..` that stays inside the repository is resolved rather than rejected:
    /// it is legal, just badly written.
    #[test]
    fn a_dotdot_that_stays_inside_the_repo_is_resolved() {
        assert_eq!(
            p("packages/domain/../application").as_str(),
            "packages/application"
        );
        assert_eq!(p("a/b/c/../../d").as_str(), "a/d");
    }

    /// Climbing above the root is not something a repository-relative path can
    /// express, so it is an error rather than a clamp to the root. Clamping
    /// would turn a caller's bug into a silently wrong path.
    #[test]
    fn a_dotdot_that_escapes_the_root_is_rejected() {
        assert_eq!(
            RepoRelPath::new("../outside"),
            Err(PathError::EscapesRoot {
                path: "../outside".to_owned()
            })
        );
        assert!(RepoRelPath::new("packages/../../outside").is_err());
    }

    #[test]
    fn an_absolute_path_is_rejected() {
        assert_eq!(
            RepoRelPath::new("/etc/passwd"),
            Err(PathError::Absolute {
                path: "/etc/passwd".to_owned()
            })
        );
    }

    /// Windows drive letters and UNC paths are absolute too, and archwarden
    /// ships a Windows binary.
    #[test]
    fn windows_absolute_paths_are_rejected() {
        assert!(RepoRelPath::new(r"C:\repo\src").is_err());
        assert!(RepoRelPath::new("C:/repo/src").is_err());
        assert!(RepoRelPath::new(r"\\server\share\file.ts").is_err());
    }

    /// The root is the empty path. `.` and `""` are both spellings of it.
    #[test]
    fn the_root_is_the_empty_path() {
        assert!(RepoRelPath::root().is_root());
        assert!(p(".").is_root());
        assert!(p("").is_root());
        assert_eq!(p("./").as_str(), "");
        assert!(!p("src").is_root());
    }

    /// Walking up from a top-level file lands on the root, not on `None`.
    /// Only the root itself has no parent.
    #[test]
    fn parent_walks_up_to_the_root_and_then_stops() {
        let file = p("packages/domain/user.ts");

        let dir = file.parent().expect("has a parent");
        assert_eq!(dir.as_str(), "packages/domain");

        let up = dir.parent().expect("has a parent");
        assert_eq!(up.as_str(), "packages");

        let root = up.parent().expect("has a parent");
        assert!(root.is_root());

        assert_eq!(root.parent(), None);
    }

    /// `Display` is how a path reaches a finding message and the text report,
    /// so it has to be the normalised form and not something else.
    #[test]
    fn display_is_the_normalised_path() {
        assert_eq!(
            p("packages/domain/user.ts").to_string(),
            "packages/domain/user.ts"
        );
        assert_eq!(p(r".\packages\domain\").to_string(), "packages/domain");
        assert_eq!(RepoRelPath::root().to_string(), "");
    }

    /// `as_path` is what hands the path to `globset`, which works in
    /// `Utf8Path` rather than `&str`. It must expose the normalised form, not
    /// whatever was passed in.
    #[test]
    fn as_path_exposes_the_normalised_path() {
        let path = p("./packages//domain/./src/");
        assert_eq!(path.as_path(), Utf8Path::new("packages/domain/src"));
        assert_eq!(path.as_path().as_str(), path.as_str());
        assert_eq!(RepoRelPath::root().as_path(), Utf8Path::new(""));
    }

    /// Joining is how a rule names the child it is reporting on, so it has to
    /// agree with what the walk produced.
    #[test]
    fn joining_appends_a_component() {
        assert_eq!(
            p("packages/domain/src")
                .join("user")
                .expect("valid")
                .as_str(),
            "packages/domain/src/user"
        );
        assert_eq!(
            RepoRelPath::root()
                .join("package.json")
                .expect("valid")
                .as_str(),
            "package.json"
        );
    }

    /// A joined name goes through the same normalisation as any other input,
    /// so a caller cannot smuggle an escape past the type.
    #[test]
    fn joining_a_name_that_escapes_the_root_is_refused() {
        assert!(RepoRelPath::root().join("..").is_err());
        assert!(p("src").join("../..").is_err());
        assert_eq!(
            p("src/user").join("..").expect("stays inside").as_str(),
            "src"
        );
    }

    #[test]
    fn file_name_is_the_final_component() {
        assert_eq!(p("packages/domain/user.ts").file_name(), Some("user.ts"));
        assert_eq!(RepoRelPath::root().file_name(), None);
    }

    /// Paths are plain strings on the wire, and normalisation runs on the way
    /// in, so a cache written by one platform is readable by another.
    #[test]
    fn paths_round_trip_through_json_normalised() {
        let path = p("packages/domain/src/user.ts");
        let json = serde_json::to_string(&path).expect("serialises");
        assert_eq!(json, "\"packages/domain/src/user.ts\"");

        let parsed: RepoRelPath =
            serde_json::from_str(r#""./packages/domain/src/user.ts""#).expect("deserialises");
        assert_eq!(parsed, path);
    }

    #[test]
    fn deserialising_an_escaping_path_fails() {
        assert!(serde_json::from_str::<RepoRelPath>(r#""../outside""#).is_err());
    }

    /// Ordering is lexical on the normalised string, which is what makes
    /// report output stable across runs and platforms.
    #[test]
    fn paths_sort_lexically_after_normalisation() {
        let mut paths = [p("b/x.ts"), p("a/z.ts"), p("./a/a.ts")];
        paths.sort();
        let sorted: Vec<_> = paths.iter().map(RepoRelPath::as_str).collect();
        assert_eq!(sorted, ["a/a.ts", "a/z.ts", "b/x.ts"]);
    }
}
