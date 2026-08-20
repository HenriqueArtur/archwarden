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

/// What kind of file a name denotes, as far as archwarden cares.
///
/// Derived from the name alone, which is why it lives beside [`RepoRelPath`]
/// rather than in `facts`: nothing is parsed to work it out.
///
/// "Is this a spec?" is deliberately not one of these. A spec is whatever a
/// rule's `spec_markers` say it is, and two rules in one config may disagree,
/// so answering here would force one of them on the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum FileClass {
    /// A JavaScript or TypeScript source file.
    Source,
    /// A document a rule can look inside: markdown, for its frontmatter.
    ///
    /// Not `Source`, because the facts it yields are a different kind and the
    /// front-end that reads it is a different one. Not `Other`, because a rule
    /// that wanted to look inside one and could not has lost an answer.
    Document,
    /// Source whose code is embedded in another file format.
    ///
    /// An `.astro` file is a TypeScript module inside a `---` fence with a
    /// template around it. It yields the same facts a `.ts` file does and is
    /// not the same *kind of thing*: `spec-pair` reads `Source` as "a unit that
    /// needs a test", and the spec for `Card.astro` is `Card.spec.ts`, never
    /// `Card.spec.astro`. `.vue` and `.svelte` are the same shape when they
    /// arrive.
    Embedded,
    /// Source in a language this build has no front-end for.
    ///
    /// The class that closes a hole rather than opening a door. A `.py` under
    /// an `import-boundary` rule used to be `Other` — the class that exists so
    /// a PNG does not inflate `checks_skipped` — so the rule saw no imports,
    /// reported nothing, and counted nothing. A rule enforcing nothing looks
    /// exactly like a repository that satisfies it, which `CONFIG.md` calls the
    /// worst failure a linter has. Nothing here becomes readable; it becomes
    /// *countable*.
    UnreadableSource,
    /// Anything else. Structure rules still see these, since a
    /// `filename_patterns` rule may well be about a `.json` or an image.
    Other,
}

impl FileClass {
    /// Extensions a front-end in this build can read.
    const SOURCE: [&'static str; 8] = ["ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs"];

    /// Extensions the document front-end can read.
    const DOCUMENT: [&'static str; 2] = ["md", "markdown"];

    /// Extensions whose code lives inside another format.
    const EMBEDDED: [&'static str; 1] = ["astro"];

    /// Extensions that are somebody's source and nobody's here.
    ///
    /// A heuristic, and deliberately a short one. Its only job is to turn
    /// silence into a named skip, so the worst a wrong entry does is report a
    /// check nobody could make — which is a sentence, not a false finding.
    ///
    /// A language gaining a front-end moves its extension up to `SOURCE`, and
    /// [`FileClass::pairs_with_sibling_spec`] is the question that has to be
    /// answered in the same commit. This sentence used to end "and nothing
    /// else changes", which was false: `spec-pair` reads `Source` as "a unit
    /// that needs a test beside it", so a language whose tests do not live
    /// beside it would start failing that rule in every configuration that
    /// already had one.
    const UNREADABLE_SOURCE: [&'static str; 12] = [
        "py", "go", "rs", "rb", "java", "kt", "kts", "php", "cs", "swift", "scala", "ex",
    ];

    /// Whether a unit of this file's language is tested by a sibling file.
    ///
    /// `spec-pair` asks this rather than `class == Source`, and the difference
    /// is the whole point. `Source` means "a front-end in this build can read
    /// it"; this means "and the test for it is `<stem>.<marker>.<ext>` next to
    /// it", which is a claim about the language's conventions rather than
    /// about archwarden's.
    ///
    /// JavaScript and TypeScript answer yes. Rust would answer no — its unit
    /// tests live in a `#[cfg(test)]` module inside the file — and a language
    /// that answers no is *skipped* by `spec-pair`, not failed by it.
    ///
    /// `.astro` answers yes too, and is worth stating: it is `Embedded` rather
    /// than `Source`, and its spec is `Card.spec.ts` rather than
    /// `Card.spec.astro` -- so the sibling exists under another extension, and
    /// the pairing this asks about holds.
    ///
    /// Written as a match rather than a second list beside `SOURCE`, so a
    /// language added to one and forgotten in the other cannot compile into a
    /// silent answer.
    #[must_use]
    pub fn pairs_with_sibling_spec(name: &str) -> bool {
        let Some((_, extension)) = name.rsplit_once('.') else {
            return false;
        };

        matches!(
            extension,
            "ts" | "tsx" | "js" | "jsx" | "mts" | "cts" | "mjs" | "cjs" | "astro"
        )
    }

    /// Whether this build can place a specifier written in this file's
    /// language.
    ///
    /// A second question `FileClass::Source` used to answer by accident.
    /// `Source` means a front-end can *read* the file; this means a resolver
    /// can turn what it read into a path — and decision 19 says the two arrive
    /// separately on purpose, because the parser is one function and the
    /// resolver is the expensive half.
    ///
    /// A language answering no has facts with every `ImportFact::resolved` at
    /// `None`. An `import-boundary` or `import-cycle` rule over such a file
    /// therefore sees no edges, reports nothing, and looks exactly like a file
    /// that crosses no boundary. Decision 19 requires the opposite: such a rule
    /// is a **loud refusal**, never a silent pass, which here means the check
    /// is counted and named rather than quietly passing.
    ///
    /// Exhaustive by extension for the same reason as
    /// [`Self::pairs_with_sibling_spec`]: a language added to `SOURCE` and
    /// forgotten here cannot compile into a silent answer.
    #[must_use]
    pub fn imports_can_be_resolved(name: &str) -> bool {
        let Some((_, extension)) = name.rsplit_once('.') else {
            return false;
        };

        matches!(
            extension,
            "ts" | "tsx" | "js" | "jsx" | "mts" | "cts" | "mjs" | "cjs" | "astro"
        )
    }

    /// Classifies by extension.
    #[must_use]
    pub fn of(name: &str) -> Self {
        let Some((_, extension)) = name.rsplit_once('.') else {
            return Self::Other;
        };

        if Self::SOURCE.contains(&extension) {
            Self::Source
        } else if Self::EMBEDDED.contains(&extension) {
            Self::Embedded
        } else if Self::DOCUMENT.contains(&extension) {
            Self::Document
        } else if Self::UNREADABLE_SOURCE.contains(&extension) {
            Self::UnreadableSource
        } else {
            Self::Other
        }
    }

    /// Whether a file of this kind could ever carry the facts a rule wants.
    ///
    /// The pair, not the class alone, is what decides whether an absent fact is
    /// an answer somebody lost. A boundary rule pointed at a `.md` has lost
    /// nothing — markdown has no imports — and counting that would pin
    /// `checks_skipped` above zero in every repository that keeps documentation
    /// beside its code, which teaches a reader to ignore the one number the
    /// docs tell them to watch. Pointed at a `.py` it has lost everything, and
    /// that is the case that used to pass in silence.
    #[must_use]
    pub fn yields(self, needed: crate::traits::FactsNeeded) -> bool {
        use crate::traits::FactsNeeded;

        match (self, needed) {
            // Source this build cannot read still *has* imports and exports.
            // That is the whole point of the class.
            (Self::Source | Self::Embedded | Self::UnreadableSource, FactsNeeded::Code)
            | (Self::Document, FactsNeeded::Document) => true,
            _ => false,
        }
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
        f.pad(self.as_str())
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

    /// A spec file is a source file. Whether it counts as "a spec" is a
    /// question only a rule's `spec_markers` can answer.
    /// Every readable source extension answers the sibling-spec question.
    ///
    /// The tripwire for decision 31's successor. `spec-pair` reads `Source` as
    /// "a unit that needs a test beside it", and that reading is only true
    /// while every language in `SOURCE` tests by sibling. Rust does not -- its
    /// unit tests are a `#[cfg(test)]` module inside the file -- so moving
    /// `rs` into `SOURCE` without teaching `spec-pair` would make every
    /// existing `spec-pair` rule start demanding `create_client.spec.rs`.
    ///
    /// This fails the moment `SOURCE` grows an extension the match below does
    /// not name, which is the commit where that decision has to be taken
    /// rather than the release where somebody notices.
    #[test]
    fn every_source_extension_says_whether_its_tests_sit_beside_it() {
        for extension in FileClass::SOURCE {
            let name = format!("unit.{extension}");
            assert!(
                FileClass::pairs_with_sibling_spec(&name),
                "`{extension}` is readable source and nothing says where its \
                 tests live. Answer it in `pairs_with_sibling_spec`: `true` if \
                 the test is `<stem>.<marker>.{extension}` beside the file, \
                 `false` if the language tests some other way -- and if it is \
                 `false`, `spec-pair` skips the language rather than failing it."
            );
        }
    }

    /// Every readable source extension says whether its imports can be placed.
    ///
    /// The second tripwire, and decision 19 is what it guards. A language whose
    /// parser lands before its resolver has facts whose every `resolved` is
    /// `None`; a boundary rule over one of its files then sees no edges and
    /// reports nothing, which is indistinguishable from a file that crosses
    /// nothing. Decision 19 requires a loud refusal instead.
    ///
    /// So moving an extension into `SOURCE` has to answer this in the same
    /// commit, and the answer for a language with a parser and no resolver is
    /// `false` -- which makes the check a counted, named skip.
    #[test]
    fn every_source_extension_says_whether_its_imports_can_be_placed() {
        for extension in FileClass::SOURCE {
            let name = format!("unit.{extension}");
            assert!(
                FileClass::imports_can_be_resolved(&name),
                "`{extension}` is readable source and nothing says whether a \
                 resolver can place its specifiers. Answer it in \
                 `imports_can_be_resolved`: `false` for a language whose parser \
                 landed before its resolver, which makes a boundary rule over \
                 it a counted skip rather than a silent pass."
            );
        }
    }

    /// A language with no front-end answers no to both questions.
    ///
    /// Both, because they are the two halves of decision 19 and a language
    /// arrives at them separately: a parser makes the first `true` and a
    /// resolver makes the second. Asserting only the classification would let
    /// either answer default to yes for a language this build cannot read at
    /// all, which is the loudest version of the silence both exist to refuse.
    #[test]
    fn a_language_nobody_reads_answers_no_to_both_questions() {
        for name in ["main.rs", "app.py", "server.go", "Thing.java"] {
            assert!(!FileClass::pairs_with_sibling_spec(name), "{name}");
            assert!(!FileClass::imports_can_be_resolved(name), "{name}");
            assert_eq!(FileClass::of(name), FileClass::UnreadableSource, "{name}");
        }
    }

    /// A document, an asset and a file with no extension are neither.
    #[test]
    fn only_source_pairs_with_a_spec_or_resolves_anything() {
        for name in ["DOC.md", "package.json", "logo.png", "Makefile"] {
            assert!(!FileClass::pairs_with_sibling_spec(name), "{name}");
            assert!(!FileClass::imports_can_be_resolved(name), "{name}");
        }
    }

    #[test]
    fn files_are_classified_by_extension() {
        for name in [
            "user.ts",
            "user.spec.ts",
            "c.tsx",
            "m.mjs",
            "c.cts",
            "old.js",
        ] {
            assert_eq!(FileClass::of(name), FileClass::Source, "{name}");
        }
        for name in ["data.json", "logo.png", "no-extension", ""] {
            assert_eq!(FileClass::of(name), FileClass::Other, "{name}");
        }
    }

    /// A markdown file is neither of the two the class used to have. It is not
    /// JS/TS, and it is not "nothing anybody could read" either — a
    /// `frontmatter` rule opens it. Issue #44.
    #[test]
    fn a_document_is_its_own_class() {
        for name in ["README.md", "DOC.md", "projeto.md", "notes.markdown"] {
            assert_eq!(FileClass::of(name), FileClass::Document, "{name}");
        }
    }

    /// Issue #13. An `.astro` file is source, and its code is not the whole
    /// file — the module lives inside a `---` fence, with a template around it.
    ///
    /// A class of its own rather than `Source`, and the reason is `spec-pair`:
    /// that rule reads `FileClass::of(name) == Source` as "this is a unit that
    /// needs a test", and the spec for `Card.astro` is `Card.spec.ts`, never
    /// `Card.spec.astro`. Calling it `Source` would demand a file nobody
    /// writes.
    #[test]
    fn source_embedded_in_another_format_is_its_own_class() {
        for name in ["index.astro", "Card.astro"] {
            assert_eq!(FileClass::of(name), FileClass::Embedded, "{name}");
        }
    }

    /// It still yields code facts: the fence is a TypeScript module, and it is
    /// where every import in an Astro page lives.
    #[test]
    fn an_embedded_source_file_yields_code_facts() {
        use crate::traits::FactsNeeded;

        assert!(FileClass::Embedded.yields(FactsNeeded::Code));
        assert!(!FileClass::Embedded.yields(FactsNeeded::Document));
    }

    /// The gap this closes: a `.py` under a rule that needs facts used to be
    /// `Other`, which is the class that exists so a PNG does not inflate
    /// `checks_skipped`. So the rule saw nothing, reported nothing, and counted
    /// nothing — a rule enforcing nothing, indistinguishable from a repository
    /// that satisfies it, which is the failure `CONFIG.md` names as the worst a
    /// linter has.
    ///
    /// A heuristic, and only ever used to turn silence into a named skip: the
    /// worst a wrong answer here does is report a check nobody could make.
    #[test]
    fn source_in_a_language_with_no_front_end_says_so() {
        for name in [
            "main.py",
            "server.go",
            "lib.rs",
            "app.rb",
            "Main.java",
            "index.php",
        ] {
            assert_eq!(FileClass::of(name), FileClass::UnreadableSource, "{name}");
        }
    }

    /// What counts as a lost answer is a *pair*: the facts a rule wanted, and
    /// whether this kind of file could ever have had them.
    ///
    /// A boundary rule pointed at a `.md` has lost nothing — markdown has no
    /// imports. Pointed at a `.py` it has lost everything, and that is the case
    /// that used to pass in silence.
    #[test]
    fn a_class_yields_only_the_facts_its_kind_of_file_can_have() {
        use crate::traits::FactsNeeded;

        assert!(FileClass::Source.yields(FactsNeeded::Code));
        assert!(!FileClass::Source.yields(FactsNeeded::Document));

        assert!(FileClass::Document.yields(FactsNeeded::Document));
        assert!(
            !FileClass::Document.yields(FactsNeeded::Code),
            "markdown has no imports, so a boundary rule lost nothing"
        );

        assert!(
            FileClass::UnreadableSource.yields(FactsNeeded::Code),
            "it has imports and exports; this build cannot read them"
        );
        assert!(!FileClass::UnreadableSource.yields(FactsNeeded::Document));

        for needed in [FactsNeeded::Code, FactsNeeded::Document] {
            assert!(!FileClass::Other.yields(needed));
        }
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
