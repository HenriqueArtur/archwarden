//! Glob sets matched against whole paths.
//!
//! Distinct from [`crate::scope::Scope`], and deliberately so. A scope selects
//! *directories* and carries the `X/**` includes `X` normalisation that rule
//! scopes need. A [`PathSet`] matches a path as written, and is what
//! `forbid_import_from`, `except` and `ignore_files` use: those are matched
//! against a resolved file path, where "does this glob select the directory"
//! is not the question being asked.
//!
//! Keeping them apart means neither has to document which of the two
//! behaviours it has.

use camino::Utf8Path;
use globset::{GlobBuilder, GlobMatcher};

/// A glob in a path set that could not be compiled.
#[derive(Debug, thiserror::Error)]
#[error("invalid glob `{pattern}`")]
pub struct GlobError {
    /// The pattern as the user wrote it.
    pub pattern: String,
    #[source]
    source: globset::Error,
}

/// A set of globs matched against whole repository-relative paths.
#[derive(Debug, Clone, Default)]
pub struct PathSet {
    matchers: Vec<GlobMatcher>,
    patterns: Vec<String>,
}

impl PathSet {
    /// Compiles a set of globs.
    ///
    /// # Errors
    /// Returns the first pattern that is not a valid glob.
    pub fn compile<I, S>(patterns: I) -> Result<Self, GlobError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut matchers = Vec::new();
        let mut stored = Vec::new();

        for pattern in patterns {
            let raw = pattern.as_ref().to_owned();
            let normalised = raw.strip_prefix("./").unwrap_or(&raw).to_owned();

            // `literal_separator` keeps `*` inside one path component, so
            // `packages/*` and `packages/**` mean different things here just
            // as they do in a scope.
            let glob = GlobBuilder::new(&normalised)
                .literal_separator(true)
                .build()
                .map_err(|source| GlobError {
                    pattern: raw.clone(),
                    source,
                })?;

            matchers.push(glob.compile_matcher());
            stored.push(raw);
        }

        Ok(Self {
            matchers,
            patterns: stored,
        })
    }

    /// Whether any glob matches `path`.
    #[must_use]
    pub fn is_match(&self, path: &Utf8Path) -> bool {
        self.matchers.iter().any(|m| m.is_match(path.as_std_path()))
    }

    /// Whether the set holds no globs. An empty set matches nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.matchers.is_empty()
    }

    /// The patterns as written, for diagnostics.
    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(patterns: &[&str]) -> PathSet {
        PathSet::compile(patterns).expect("test globs must compile")
    }

    fn path(p: &str) -> &Utf8Path {
        Utf8Path::new(p)
    }

    /// The motivating case: a boundary forbidding imports from a layer, matched
    /// against the resolved path of a file inside it.
    #[test]
    fn a_recursive_glob_matches_a_file_anywhere_beneath_it() {
        let forbidden = set(&["packages/domain/**"]);

        assert!(forbidden.is_match(path("packages/domain/src/user/user.ts")));
        assert!(forbidden.is_match(path("packages/domain/index.ts")));
        assert!(!forbidden.is_match(path("packages/application/src/x.ts")));
    }

    /// `*` stops at a path boundary here exactly as it does in a scope, so the
    /// two spellings keep meaning different things.
    #[test]
    fn a_single_star_stays_within_one_component() {
        let shallow = set(&["packages/*"]);

        assert!(shallow.is_match(path("packages/domain")));
        assert!(!shallow.is_match(path("packages/domain/src/user.ts")));
    }

    /// The `except` case from docs/CONFIG.md: type-only imports from a
    /// specific shape inside an otherwise forbidden layer.
    #[test]
    fn an_exception_glob_matches_the_shape_it_names() {
        let except = set(&["packages/domain/src/*/types/**"]);

        assert!(except.is_match(path("packages/domain/src/user/types/user.ts")));
        assert!(!except.is_match(path("packages/domain/src/user/calcs/age.ts")));
    }

    /// Unlike a scope, a path set does *not* add `X` when given `X/**`. A file
    /// path is never a directory, so the normalisation would only blur what
    /// the pattern says.
    #[test]
    fn a_path_set_does_not_add_the_directory_a_recursive_glob_is_rooted_at() {
        let globs = set(&["packages/domain/**"]);
        assert!(!globs.is_match(path("packages/domain")));
    }

    #[test]
    fn several_globs_behave_as_a_union() {
        let globs = set(&["**/dist/**", "**/*.generated.ts"]);

        assert!(globs.is_match(path("apps/web/dist/index.js")));
        assert!(globs.is_match(path("packages/domain/src/schema.generated.ts")));
        assert!(!globs.is_match(path("packages/domain/src/user.ts")));
    }

    /// An empty set matches nothing, which is what makes an absent `except`
    /// or `ignore_files` behave as "no exceptions" without a special case at
    /// every call site.
    #[test]
    fn an_empty_set_matches_nothing() {
        let empty = PathSet::default();

        assert!(empty.is_empty());
        assert!(!empty.is_match(path("anything.ts")));
        assert!(empty.patterns().is_empty());

        let explicitly_empty = PathSet::compile(Vec::<String>::new()).expect("compiles");
        assert!(explicitly_empty.is_empty());
    }

    #[test]
    fn a_leading_dot_slash_is_ignored() {
        assert!(set(&["./src/**"]).is_match(path("src/main.ts")));
    }

    /// Patterns are reported as written, before normalisation, because that is
    /// what a diagnostic echoes back.
    #[test]
    fn patterns_are_reported_exactly_as_written() {
        let globs = set(&["./src/**", "**/dist/**"]);
        assert_eq!(globs.patterns(), ["./src/**", "**/dist/**"]);
        assert!(!globs.is_empty());
    }

    /// A compiled rule is cloned when the engine hands it to a worker, and
    /// printed when a diagnostic needs to say what it holds. Neither derive is
    /// decorative.
    #[test]
    fn a_path_set_clones_and_prints() {
        let globs = set(&["packages/domain/**"]);
        let copy = globs.clone();

        assert_eq!(copy.patterns(), globs.patterns());
        assert!(copy.is_match(path("packages/domain/src/user.ts")));
        assert!(format!("{globs:?}").contains("PathSet"));
    }

    #[test]
    fn an_invalid_glob_is_rejected_with_its_pattern() {
        let err = PathSet::compile(["packages/[domain"]).expect_err("should not compile");
        assert_eq!(err.pattern, "packages/[domain");
        assert!(err.to_string().contains("packages/[domain"), "{err}");
        assert!(
            format!("{err:?}").contains("GlobError"),
            "Debug is used by test harnesses"
        );
    }

    /// The underlying glob error stays reachable as a source. miette walks the
    /// chain when it renders, so an error that drops its cause silently loses
    /// the only sentence explaining *why* the glob was rejected.
    #[test]
    fn the_underlying_glob_error_is_kept_as_a_source() {
        use std::error::Error as _;

        let err = PathSet::compile(["packages/[domain"]).expect_err("should not compile");
        let cause = err.source().expect("keeps its cause");

        assert!(!cause.to_string().is_empty());
        assert_ne!(cause.to_string(), err.to_string(), "the cause adds detail");
    }
}
