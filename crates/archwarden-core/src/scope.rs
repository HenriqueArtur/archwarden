//! Path scopes: the `roots` field carried by every rule, and `from` on import
//! boundaries.
//!
//! A scope glob always selects **directories**. Each rule kind then decides
//! what it inspects inside a selected directory. See `docs/RULES.md`.

use camino::Utf8Path;
use globset::{GlobBuilder, GlobMatcher};

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
///
/// Patterns are held as individually compiled matchers rather than as a
/// `GlobSet`. A `GlobSet` amortises one candidate construction across many
/// patterns, which is the right trade for a gitignore-sized list; a rule's
/// scope is one or two patterns, where the two are equivalent. What the
/// individual matchers buy is that `Glob::compile_matcher` is infallible,
/// so `compile` has exactly one failure mode -- a pattern that does not
/// parse -- instead of a second, unreachable one from building the set.
#[derive(Debug, Clone)]
pub struct Scope {
    matchers: Vec<GlobMatcher>,
    patterns: Vec<String>,
    /// Whether the repository root itself is selected. Tracked separately
    /// because the root is the empty path, which is not a glob any engine
    /// matches naturally.
    matches_root: bool,
    /// A second scope every match must also satisfy.
    ///
    /// The module a rule lives in, when that module declared paths of its own
    /// (issue #74). Held rather than merged into `matchers` because the two
    /// are a conjunction and the matchers are a disjunction: a rule scoped to
    /// `a/*` or `b/*`, inside a module scoped to `a/**`, reaches `a/*` alone,
    /// and no single list of globs says that.
    within: Option<Box<Scope>>,
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
        let mut matchers = Vec::new();
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

            add_glob(&mut matchers, normalised, &raw)?;

            // `X/**` selects everything beneath X. Users also expect X itself,
            // which globset does not give us, so it is added explicitly. A
            // bare `**` means the whole repository, root included.
            match normalised.strip_suffix("/**") {
                Some(prefix) if !prefix.is_empty() => add_glob(&mut matchers, prefix, &raw)?,
                Some(_) => matches_root = true,
                None => {
                    if normalised == "**" {
                        matches_root = true;
                    }
                }
            }

            stored.push(raw);
        }

        Ok(Self {
            matchers,
            patterns: stored,
            matches_root,
            within: None,
        })
    }

    /// Whether `dir` is selected by this scope. `dir` is repository-relative;
    /// the repository root itself is the empty path.
    #[must_use]
    pub fn matches_dir(&self, dir: &Utf8Path) -> bool {
        if let Some(outer) = &self.within
            && !outer.matches_dir(dir)
        {
            return false;
        }

        let as_str = dir.as_str();
        if as_str.is_empty() || as_str == "." {
            return self.matches_root;
        }
        self.matchers.iter().any(|m| m.is_match(dir.as_std_path()))
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
    ///
    /// The rule's own, never the narrowing. A finding names what the rule was
    /// written to govern; the module it sits in is a separate sentence and is
    /// reported as one.
    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// This scope, narrowed to what `outer` also reaches.
    ///
    /// Narrowing rather than replacing, and not refusing: a rule inside a
    /// module with paths of its own keeps its `roots`, and reaches where both
    /// agree. A rule pointing outside its module therefore reaches nothing,
    /// which is silent — so `config doctor` names it. Refusing at compile time
    /// would be louder and cannot be done: whether one glob contains another
    /// is not a question `globset` answers, and the only honest test is
    /// against a tree that has already been walked. Issue #74.
    #[must_use]
    pub fn within(&self, outer: &Self) -> Self {
        Self {
            matchers: self.matchers.clone(),
            patterns: self.patterns.clone(),
            matches_root: self.matches_root,
            within: Some(Box::new(match &self.within {
                // Already narrowed once: narrow the narrowing, so a module
                // nested in a module composes rather than replacing.
                Some(inner) => inner.within(outer),
                None => outer.clone(),
            })),
        }
    }
}

/// Strips the `./` and trailing `/` noise that hand-written configs collect,
/// and collapses every spelling of "the repository root" to the empty string.
fn normalise(pattern: &str) -> &str {
    let trimmed = pattern.strip_prefix("./").unwrap_or(pattern);
    let trimmed = trimmed.trim_end_matches('/');
    if trimmed == "." { "" } else { trimmed }
}

fn add_glob(matchers: &mut Vec<GlobMatcher>, pattern: &str, raw: &str) -> Result<(), ScopeError> {
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
    matchers.push(glob.compile_matcher());
    Ok(())
}

#[cfg(test)]
mod narrowing_tests {
    use super::Scope;
    use camino::Utf8Path;

    /// A rule inside a module reaches where both agree, and nowhere else.
    ///
    /// Issue #74. A module gains paths of its own, and a rule inside it keeps
    /// its `roots` — so the two have to combine, and narrowing is the safe
    /// combination: a module that says "this is my area" cannot have a rule
    /// inside it governing somewhere else.
    #[test]
    fn a_narrowed_scope_reaches_only_where_both_reach() {
        let rule = Scope::compile(["packages/domain/src/*"]).expect("valid");
        let module = Scope::compile(["packages/domain/**"]).expect("valid");

        let narrowed = rule.within(&module);

        assert!(narrowed.matches_dir(Utf8Path::new("packages/domain/src/order")));
        assert!(
            !narrowed.matches_dir(Utf8Path::new("packages/billing/src/order")),
            "outside the module"
        );
    }

    /// The case the `doctor` check exists for: a rule whose `roots` points
    /// somewhere its module does not reach selects nothing at all. Silent
    /// emptiness is the failure this project keeps refusing, which is why
    /// `config doctor` names it — but the semantics here stay narrowing,
    /// because deciding whether one glob contains another is not something
    /// `globset` can answer.
    #[test]
    fn a_rule_reaching_outside_its_module_reaches_nothing() {
        let rule = Scope::compile(["apps/api/src/*"]).expect("valid");
        let module = Scope::compile(["packages/domain/**"]).expect("valid");

        let narrowed = rule.within(&module);

        assert!(!narrowed.matches_dir(Utf8Path::new("apps/api/src/env")));
        assert!(!narrowed.matches_dir(Utf8Path::new("packages/domain/src")));
    }

    /// Narrowing keeps the rule's own patterns for diagnostics: a finding says
    /// what the rule was written to govern, not the intersection nobody typed.
    #[test]
    fn the_patterns_reported_are_the_ones_the_rule_was_written_with() {
        let rule = Scope::compile(["packages/domain/src/*"]).expect("valid");
        let module = Scope::compile(["packages/domain/**"]).expect("valid");

        assert_eq!(rule.within(&module).patterns(), ["packages/domain/src/*"]);
    }

    /// A rule at the repository root, narrowed by a module that does not
    /// include the root, stops matching it. The root is tracked separately
    /// from the matchers and would otherwise slip past the narrowing.
    #[test]
    fn the_root_is_narrowed_like_anything_else() {
        let rule = Scope::compile(["."]).expect("valid");
        let module = Scope::compile(["packages/domain/**"]).expect("valid");

        assert!(rule.matches_dir(Utf8Path::new(".")));
        assert!(!rule.within(&module).matches_dir(Utf8Path::new(".")));
    }
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
