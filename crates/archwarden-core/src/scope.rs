//! Path scopes: the `roots` field carried by every rule, and `from` on import
//! boundaries.
//!
//! A scope glob always selects **directories**. Each rule kind then decides
//! what it inspects inside a selected directory. See `docs/RULES.md`.

use camino::Utf8Path;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

/// A scope pattern that could not be compiled.
#[derive(Debug, thiserror::Error)]
#[error("invalid scope glob `{pattern}`")]
pub struct ScopeError {
    /// The pattern as the user wrote it.
    pub pattern: String,
    #[source]
    source: globset::Error,
}

/// A compiled set of directory globs.
///
/// Matching is purely lexical: no path is ever touched on disk. That is what
/// lets `describe` and the pre-write hook answer "which rules apply here?" for
/// files that do not exist yet.
#[derive(Debug, Clone)]
pub struct Scope {
    set: GlobSet,
    patterns: Vec<String>,
    /// Whether the repository root itself is selected. Tracked separately
    /// because the root is the empty path, which is not a glob any engine
    /// matches naturally.
    matches_root: bool,
}

impl Scope {
    /// Compiles a set of scope patterns.
    ///
    /// # Errors
    /// Returns the first pattern that is not a valid glob.
    pub fn compile<I, S>(patterns: I) -> Result<Self, ScopeError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut builder = GlobSetBuilder::new();
        let mut stored = Vec::new();
        let mut matches_root = false;

        for pattern in patterns {
            let raw = pattern.as_ref().to_owned();
            let normalised = normalise(&raw);

            if normalised.is_empty() {
                matches_root = true;
                stored.push(raw);
                continue;
            }

            add_glob(&mut builder, normalised, &raw)?;

            // `X/**` selects everything beneath X. Users also expect X itself,
            // which globset does not give us, so it is added explicitly. A
            // bare `**` means the whole repository, root included.
            match normalised.strip_suffix("/**") {
                Some(prefix) if !prefix.is_empty() => add_glob(&mut builder, prefix, &raw)?,
                Some(_) => matches_root = true,
                None => {
                    if normalised == "**" {
                        matches_root = true;
                    }
                }
            }

            stored.push(raw);
        }

        let set = builder.build().map_err(|source| ScopeError {
            pattern: stored.join(", "),
            source,
        })?;

        Ok(Self {
            set,
            patterns: stored,
            matches_root,
        })
    }

    /// Whether `dir` is selected by this scope. `dir` is repository-relative;
    /// the repository root itself is the empty path.
    #[must_use]
    pub fn matches_dir(&self, dir: &Utf8Path) -> bool {
        let as_str = dir.as_str();
        if as_str.is_empty() || as_str == "." {
            return self.matches_root;
        }
        self.set.is_match(dir.as_std_path())
    }

    /// Whether `file` sits directly inside a selected directory.
    ///
    /// The file's own name is irrelevant here. Filtering by filename is each
    /// rule kind's job; the scope only answers "is this the right directory?".
    #[must_use]
    pub fn contains_file(&self, file: &Utf8Path) -> bool {
        file.parent().is_some_and(|parent| self.matches_dir(parent))
    }

    /// The patterns as written, for diagnostics.
    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }
}

/// Strips the `./` and trailing `/` noise that hand-written configs collect,
/// and collapses every spelling of "the repository root" to the empty string.
fn normalise(pattern: &str) -> &str {
    let trimmed = pattern.strip_prefix("./").unwrap_or(pattern);
    let trimmed = trimmed.trim_end_matches('/');
    if trimmed == "." { "" } else { trimmed }
}

fn add_glob(builder: &mut GlobSetBuilder, pattern: &str, raw: &str) -> Result<(), ScopeError> {
    // `literal_separator` is what makes `*` stop at a path boundary. Without
    // it, `src/*` would also select `src/a/b`, and the distinction between `*`
    // and `**` that docs/RULES.md promises would not exist.
    let glob = GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map_err(|source| ScopeError {
            pattern: raw.to_owned(),
            source,
        })?;
    builder.add(glob);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(patterns: &[&str]) -> Scope {
        Scope::compile(patterns).expect("test patterns must compile")
    }

    fn dir(p: &str) -> &Utf8Path {
        Utf8Path::new(p)
    }

    /// A single `*` selects the direct child directories of its parent, and
    /// neither the parent itself nor anything deeper.
    #[test]
    fn single_star_selects_only_direct_children() {
        let s = scope(&["packages/domain/src/*"]);

        assert!(s.matches_dir(dir("packages/domain/src/user")));
        assert!(s.matches_dir(dir("packages/domain/src/invoice")));

        assert!(!s.matches_dir(dir("packages/domain/src")));
        assert!(!s.matches_dir(dir("packages/domain/src/user/calcs")));
    }

    /// `X/**` selects everything beneath `X` *and* `X` itself. globset does not
    /// do the latter on its own; users expect it, so we normalise it in.
    #[test]
    fn double_star_includes_the_directory_it_is_rooted_at() {
        let s = scope(&["apps/api/**"]);

        assert!(s.matches_dir(dir("apps/api")));
        assert!(s.matches_dir(dir("apps/api/users")));
        assert!(s.matches_dir(dir("apps/api/users/[id]")));

        assert!(!s.matches_dir(dir("apps")));
        assert!(!s.matches_dir(dir("apps/web")));
    }

    /// A file is in scope when its parent directory is, whatever the file is
    /// called. Filename filtering is each rule's own job, not the scope's.
    #[test]
    fn a_file_is_in_scope_when_its_parent_directory_is() {
        let s = scope(&["packages/domain/src/*"]);

        assert!(s.contains_file(dir("packages/domain/src/user/user.ts")));
        assert!(s.contains_file(dir("packages/domain/src/user/anything.md")));

        // Two levels down: the parent is `user/calcs`, which the scope does
        // not select.
        assert!(!s.contains_file(dir("packages/domain/src/user/calcs/age.ts")));
    }

    /// The repository root is the empty path, and `.` is how a config names it.
    #[test]
    fn the_repository_root_is_addressable_as_dot() {
        let s = scope(&["."]);

        assert!(s.matches_dir(dir("")));
        assert!(s.contains_file(dir("package.json")));

        assert!(!s.matches_dir(dir("src")));
        assert!(!s.contains_file(dir("src/main.ts")));
    }

    /// Scope matching never touches the filesystem, so a path that does not
    /// exist matches exactly like one that does. This is what `describe` and
    /// the pre-write hook depend on.
    #[test]
    fn matching_is_lexical_and_needs_no_file_on_disk() {
        let s = scope(&["packages/application/src/use-cases/*"]);

        assert!(s.contains_file(dir(
            "packages/application/src/use-cases/does-not-exist-yet/foo.use-case.ts"
        )));
    }

    /// Several patterns behave as a union.
    #[test]
    fn multiple_patterns_are_a_union() {
        let s = scope(&["packages/domain/**", "apps/web/src/*"]);

        assert!(s.matches_dir(dir("packages/domain/src/user")));
        assert!(s.matches_dir(dir("apps/web/src/components")));
        assert!(!s.matches_dir(dir("apps/api/src/routes")));
    }

    /// A malformed glob is rejected when the config is compiled, not when a
    /// file happens to be walked, and the error names the offending pattern.
    #[test]
    fn an_invalid_glob_is_rejected_at_compile_time() {
        let err = Scope::compile(["packages/[domain"]).expect_err("should not compile");
        assert_eq!(err.pattern, "packages/[domain");
    }

    /// A leading `./` is noise from hand-written configs and means the same
    /// thing without it.
    #[test]
    fn a_leading_dot_slash_is_ignored() {
        let s = scope(&["./src/*"]);
        assert!(s.matches_dir(dir("src/components")));
    }

    /// A bare `**` is how a config says "the entire repository". It has to
    /// include the root, which is the empty path and therefore not something
    /// the glob engine reaches on its own.
    #[test]
    fn a_bare_double_star_selects_the_whole_repository_including_the_root() {
        let s = scope(&["**"]);

        assert!(s.matches_dir(dir("")));
        assert!(s.matches_dir(dir("packages")));
        assert!(s.matches_dir(dir("packages/domain/src/user")));
        assert!(s.contains_file(dir("package.json")));
        assert!(s.contains_file(dir("packages/domain/src/user/user.ts")));
    }

    /// `/**` is an odd but legal spelling of the same thing, and it takes a
    /// different branch than a bare `**`.
    #[test]
    fn a_rooted_double_star_also_reaches_the_root() {
        let s = scope(&["/**"]);
        assert!(s.matches_dir(dir("")));
    }

    /// Patterns are kept verbatim, before normalisation, because they are what
    /// gets shown back to the user in `explain` and `config doctor`. A
    /// diagnostic that echoes a rewritten pattern is worse than none.
    #[test]
    fn patterns_are_reported_exactly_as_written() {
        let s = scope(&["./src/*", "apps/api/**"]);
        assert_eq!(s.patterns(), ["./src/*", "apps/api/**"]);

        assert!(
            Scope::compile(Vec::<String>::new())
                .expect("empty compiles")
                .patterns()
                .is_empty()
        );
    }
}
