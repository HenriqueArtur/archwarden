//! Deciding which findings get printed.
//!
//! The distinction this module exists to hold: **rules always run, findings
//! are always computed, filters only decide what is shown.** Nothing here
//! reaches the engine. A filtered run and an unfiltered one evaluate exactly
//! the same thing and exit exactly the same way — which is what makes it safe
//! to put a filter in a CI command without wondering whether the gate got
//! narrower.
//!
//! # Why an unknown rule id is an error
//!
//! `--rules typo-here` could reasonably print nothing and exit 0. That is the
//! worst outcome available: a filter that matches nothing looks exactly like a
//! repository with no findings. The config commands already refuse an unknown
//! id -- `disable` and `config explain` both do -- and this follows them.

use archwarden_core::{
    compiled::CompiledConfig, finding::Finding, glob::PathSet, ids::RuleId, level::Level,
};

/// Which level to show, as a command-line value.
///
/// A CLI-side enum because `Level` lives in the core, which does not know
/// about clap and should not learn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LevelFilter {
    /// Only errors.
    Error,
    /// Only warnings.
    Warning,
}

impl LevelFilter {
    fn level(self) -> Level {
        match self {
            Self::Error => Level::Error,
            Self::Warning => Level::Warning,
        }
    }
}

/// What the user asked to see.
///
/// Every field absent means "everything", which is what an unfiltered run is.
#[derive(Debug, Default)]
pub struct Filters {
    rules: Option<Vec<RuleId>>,
    paths: Option<PathSet>,
    /// The files `--changed` named.
    ///
    /// Its own field rather than more entries in `paths`, and the distinction
    /// is load-bearing: `--changed` on a clean working tree yields an *empty*
    /// list, which folded into `paths` would be indistinguishable from no
    /// `--paths` at all. The filter would switch off and show everything --
    /// the opposite of what was asked.
    changed: Option<PathSet>,
    level: Option<Level>,
}

/// The filters as written on the command line.
///
/// A struct because there are four of them and they are all lists of strings,
/// which at a call site is where a transposition hides.
#[derive(Debug, Default)]
pub struct Arguments<'a> {
    /// `--rules`.
    pub rules: &'a [String],
    /// `--paths`.
    pub paths: &'a [String],
    /// The files `--changed` found, when it was given. `Some(empty)` means it
    /// was asked for and nothing had changed.
    pub changed: Option<Vec<String>>,
    /// `--level`.
    pub level: Option<LevelFilter>,
}

impl Filters {
    /// Builds the filters, refusing anything that cannot mean what it says.
    ///
    /// # Errors
    /// A message naming the problem: a rule id no rule has, or a glob that is
    /// not one.
    pub fn compile(arguments: Arguments<'_>, config: &CompiledConfig) -> Result<Self, String> {
        let Arguments {
            rules,
            paths,
            changed,
            level,
        } = arguments;

        let compiled_rules = if rules.is_empty() {
            None
        } else {
            Some(resolve_ids(rules, config)?)
        };

        let compiled_paths = if paths.is_empty() {
            None
        } else {
            Some(compile_paths(paths)?)
        };

        // Matched exactly, never widened. `collect` already enumerates every
        // directory a change touched, so adding `/**` to each would turn the
        // ancestor `packages` into `packages/**` and drag in every finding in
        // the repository -- which is what a first run of this actually did.
        let compiled_changed = changed
            .map(|paths| PathSet::compile(&paths).map_err(|error| error.to_string()))
            .transpose()?;

        Ok(Self {
            rules: compiled_rules,
            paths: compiled_paths,
            changed: compiled_changed,
            level: level.map(LevelFilter::level),
        })
    }

    /// Whether anything is being filtered at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_none()
            && self.paths.is_none()
            && self.changed.is_none()
            && self.level.is_none()
    }

    /// The rule ids the user named, if any.
    ///
    /// The breakdown uses this: naming rules is the one filter that says which
    /// *rules* the reader cares about, so it is the one that narrows the rows.
    #[must_use]
    pub fn named_rules(&self) -> Option<&[RuleId]> {
        self.rules.as_deref()
    }

    /// Whether a finding survives every filter. All of them, not any.
    #[must_use]
    pub fn keep(&self, finding: &Finding) -> bool {
        if let Some(rules) = &self.rules
            && !rules.contains(&finding.rule_id)
        {
            return false;
        }
        if let Some(paths) = &self.paths
            && !paths.is_match(finding.path.as_path())
        {
            return false;
        }
        if let Some(changed) = &self.changed
            && !changed.is_match(finding.path.as_path())
        {
            return false;
        }
        if let Some(level) = self.level
            && finding.level != level
        {
            return false;
        }
        true
    }
}

/// Compiles a list of `--paths`-style entries into one set.
///
/// Public because `orphans` narrows the same way `check --paths` does, and one
/// convention for narrowing is worth more than a second matcher that almost
/// agrees with the first.
///
/// # Errors
/// A message when a pattern will not compile.
pub fn path_set(paths: &[String]) -> Result<PathSet, String> {
    compile_paths(paths)
}

/// Compiles a list of `--paths`-style entries into one set.
fn compile_paths(paths: &[String]) -> Result<PathSet, String> {
    let expanded: Vec<String> = paths.iter().flat_map(|path| expand(path)).collect();
    PathSet::compile(&expanded).map_err(|error| error.to_string())
}

/// Whether a string is a pattern rather than a path.
///
/// One convention, used by `--paths` and by `describe`, so a user who learns
/// it once has learned it everywhere.
#[must_use]
pub fn looks_like_a_glob(pattern: &str) -> bool {
    const GLOB_CHARS: [char; 5] = ['*', '?', '[', ']', '{'];
    pattern.contains(GLOB_CHARS)
}

/// What a `--paths` entry has to match.
///
/// A glob is left exactly as written: someone who wrote `src/*` means one
/// level, and quietly widening it to `src/*/**` would be archwarden overruling
/// them.
///
/// A plain path is not a glob and is not treated as one. It selects that path
/// and everything under it, because the path a user has to hand is the one
/// they copied out of a finding, and making them remember to append `/**`
/// turns "look closer at this" into an empty report — which reads like the
/// problem went away.
fn expand(pattern: &str) -> Vec<String> {
    if looks_like_a_glob(pattern) {
        return vec![pattern.to_owned()];
    }

    let trimmed = pattern.trim_end_matches('/');
    // Both, so a structure finding -- which names the directory itself, not a
    // file in it -- is kept alongside everything inside.
    vec![trimmed.to_owned(), format!("{trimmed}/**")]
}

/// Turns the ids the user wrote into ids the config has, or says which one it
/// does not.
fn resolve_ids(wanted: &[String], config: &CompiledConfig) -> Result<Vec<RuleId>, String> {
    let known: Vec<&RuleId> = config.rules().map(|rule| &rule.id).collect();

    wanted
        .iter()
        .map(|id| {
            known
                .iter()
                .find(|known| known.as_str() == id)
                .map(|known| (*known).clone())
                .ok_or_else(|| unknown(id, &known))
        })
        .collect()
}

/// The message for an id no rule has.
///
/// Lists what there is when the list is short enough to read. A user who
/// mistyped one id out of four wants to see the four; a user with sixty rules
/// wants `config validate`, not a wall.
fn unknown(id: &str, known: &[&RuleId]) -> String {
    const LISTABLE: usize = 12;

    if known.is_empty() {
        return format!("no rule is called `{id}`; this configuration has no rules");
    }
    if known.len() <= LISTABLE {
        let names: Vec<&str> = known.iter().map(|id| id.as_str()).collect();
        return format!("no rule is called `{id}`; there is {}", list(&names));
    }
    format!(
        "no rule is called `{id}`; this configuration has {} rules",
        known.len()
    )
}

/// `a`, `a` or `b`, `a`, `b` or `c` -- the phrasing the rest of the reports
/// use for a list of alternatives.
fn list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [only] => format!("`{only}`"),
        [rest @ .., last] => {
            let head: Vec<String> = rest.iter().map(|name| format!("`{name}`")).collect();
            format!("{} or `{last}`", head.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::{CompiledRule, CompiledRuleKind, SkipDirs},
        finding::{Expectation, Observed},
        hash::ContentHash,
        path::RepoRelPath,
        scope::Scope,
    };

    fn rule(id: &str) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Structure {
                allowed_subfolders: Some(Vec::new()),
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: Vec::new(),
                filename_patterns: Vec::new(),
            },
        }
    }

    fn config(ids: &[&str]) -> CompiledConfig {
        CompiledConfig::new(
            ids.iter().map(|id| rule(id)).collect(),
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b""),
        )
    }

    fn finding(rule_id: &str, path: &str, level: Level) -> Finding {
        Finding {
            rule_id: RuleId::new(rule_id).expect("valid id"),
            module_id: None,
            level,
            path: RepoRelPath::new(path).expect("valid path"),
            span: None,
            observed: Observed::UnexpectedSubfolder {
                name: "handlers".to_owned(),
            },
            expected: Expectation::AllowedSubfolders {
                allowed: vec!["use-cases".to_owned()],
                warn: Vec::new(),
                patterns: Vec::new(),
            },
        }
    }

    fn compile(rules: &[&str], paths: &[&str], level: Option<LevelFilter>) -> Filters {
        let owned_rules: Vec<String> = rules.iter().map(|id| (*id).to_owned()).collect();
        let owned_paths: Vec<String> = paths.iter().map(|glob| (*glob).to_owned()).collect();
        Filters::compile(
            Arguments {
                rules: &owned_rules,
                paths: &owned_paths,
                changed: None,
                level,
            },
            &config(&["shape", "spec", "boundary"]),
        )
        .expect("compiles")
    }

    /// No flags is no filtering, and that has to be the cheap path: it is
    /// every run nobody asked to narrow.
    #[test]
    fn nothing_asked_for_keeps_everything() {
        let filters = compile(&[], &[], None);

        assert!(filters.is_empty());
        assert!(filters.keep(&finding("shape", "src/a/b.ts", Level::Error)));
        assert!(filters.keep(&finding("spec", "lib/c.ts", Level::Warning)));
        assert_eq!(filters.named_rules(), None);
    }

    #[test]
    fn a_named_rule_keeps_only_its_findings() {
        let filters = compile(&["shape"], &[], None);

        assert!(filters.keep(&finding("shape", "src/a.ts", Level::Error)));
        assert!(!filters.keep(&finding("spec", "src/a.ts", Level::Error)));
    }

    /// Several ids, however they were written. `--rules a,b` and
    /// `--rules a --rules b` reach here identically, because clap flattens
    /// both into one list.
    #[test]
    fn several_named_rules_are_a_set() {
        let filters = compile(&["shape", "boundary"], &[], None);

        assert!(filters.keep(&finding("shape", "src/a.ts", Level::Error)));
        assert!(filters.keep(&finding("boundary", "src/a.ts", Level::Error)));
        assert!(!filters.keep(&finding("spec", "src/a.ts", Level::Error)));
    }

    #[test]
    fn a_glob_keeps_only_findings_under_it() {
        let filters = compile(&[], &["packages/domain/**"], None);

        assert!(filters.keep(&finding("shape", "packages/domain/src/a.ts", Level::Error)));
        assert!(!filters.keep(&finding("shape", "packages/app/src/a.ts", Level::Error)));
    }

    /// A path with no glob in it is a path, and it selects what is under it.
    ///
    /// The one a user has to hand is the one they just copied out of a
    /// finding. Requiring them to remember `/**` turns "look closer at this"
    /// into an empty report, which reads like the problem went away.
    #[test]
    fn a_plain_path_selects_what_is_under_it() {
        let filters = compile(&[], &["packages/domain/src/order"], None);

        assert!(filters.keep(&finding(
            "shape",
            "packages/domain/src/order/calcs/a.ts",
            Level::Error
        )));
        // And the directory itself, which is what a structure finding names.
        assert!(filters.keep(&finding("shape", "packages/domain/src/order", Level::Error)));

        assert!(!filters.keep(&finding(
            "shape",
            "packages/domain/src/invoice/calcs/a.ts",
            Level::Error
        )));
    }

    /// A file path selects that file, and nothing that merely starts with its
    /// name. `order.ts` must not drag in `order.spec.ts`.
    #[test]
    fn a_plain_file_path_is_not_a_prefix_of_its_neighbours() {
        let filters = compile(&[], &["packages/domain/src/order.ts"], None);

        assert!(filters.keep(&finding(
            "shape",
            "packages/domain/src/order.ts",
            Level::Error
        )));
        assert!(!filters.keep(&finding(
            "shape",
            "packages/domain/src/order.spec.ts",
            Level::Error
        )));
    }

    /// The dangerous case, and the reason `changed` is its own field rather
    /// than more entries in `paths`.
    ///
    /// `--changed` on a clean working tree yields an empty list. Folded into
    /// `paths`, an empty list is indistinguishable from "no `--paths` was
    /// given", so the filter would switch off and the report would show
    /// everything — the opposite of what was asked, and it would look like the
    /// flag simply did not apply.
    #[test]
    fn changed_with_nothing_changed_shows_nothing() {
        let filters = Filters::compile(
            Arguments {
                changed: Some(Vec::new()),
                ..Arguments::default()
            },
            &config(&["shape"]),
        )
        .expect("compiles");

        assert!(!filters.is_empty(), "a filter was asked for");
        assert!(!filters.keep(&finding("shape", "src/a.ts", Level::Error)));
    }

    /// A changed file is kept, and so is anything under it -- a directory git
    /// reports as touched is a directory a structure finding can be about.
    #[test]
    fn changed_keeps_the_files_git_named() {
        let filters = Filters::compile(
            Arguments {
                changed: Some(vec!["packages/domain/src/order/a.ts".to_owned()]),
                ..Arguments::default()
            },
            &config(&["shape"]),
        )
        .expect("compiles");

        assert!(filters.keep(&finding(
            "shape",
            "packages/domain/src/order/a.ts",
            Level::Error
        )));
        assert!(!filters.keep(&finding(
            "shape",
            "packages/domain/src/order/b.ts",
            Level::Error
        )));
    }

    /// The second defect a first run found: `--changed` entries are matched
    /// exactly, never widened.
    ///
    /// The changed list already names every directory a change touched, so
    /// treating each as a prefix turns the ancestor `packages` into
    /// `packages/**` — and the report shows every finding in the repository
    /// while claiming to show only what you touched.
    #[test]
    fn a_changed_directory_does_not_drag_in_its_whole_subtree() {
        let filters = Filters::compile(
            Arguments {
                // What `collect` produces for one file under `packages/app`.
                changed: Some(vec![
                    "packages".to_owned(),
                    "packages/app".to_owned(),
                    "packages/app/services".to_owned(),
                    "packages/app/services/z.ts".to_owned(),
                ]),
                ..Arguments::default()
            },
            &config(&["shape"]),
        )
        .expect("compiles");

        assert!(filters.keep(&finding("shape", "packages/app/services", Level::Error)));
        assert!(
            !filters.keep(&finding(
                "shape",
                "packages/domain/src/order/handlers",
                Level::Error
            )),
            "a directory nobody touched"
        );
    }

    /// And it ANDs with the others, like every filter does.
    #[test]
    fn changed_composes_with_the_other_filters() {
        let filters = Filters::compile(
            Arguments {
                rules: &["shape".to_owned()],
                changed: Some(vec!["src/a.ts".to_owned(), "src/b.ts".to_owned()]),
                ..Arguments::default()
            },
            &config(&["shape", "spec"]),
        )
        .expect("compiles");

        assert!(filters.keep(&finding("shape", "src/a.ts", Level::Error)));
        assert!(!filters.keep(&finding("spec", "src/a.ts", Level::Error)));
        assert!(!filters.keep(&finding("shape", "src/c.ts", Level::Error)));
    }

    /// A pattern with a glob character in it is left exactly as written. A
    /// user who wrote `src/*` means one level, and quietly turning it into
    /// `src/*/**` would be archwarden overruling them.
    #[test]
    fn a_pattern_with_a_glob_is_left_alone() {
        let filters = compile(&[], &["packages/*"], None);

        assert!(filters.keep(&finding("shape", "packages/domain", Level::Error)));
        assert!(
            !filters.keep(&finding("shape", "packages/domain/src/a.ts", Level::Error)),
            "`packages/*` is one level, and stays one level"
        );
    }

    /// Any of the globs, not all of them -- a path cannot be under two
    /// packages at once, so `all` would match nothing.
    #[test]
    fn several_globs_are_an_or() {
        let filters = compile(&[], &["packages/domain/**", "packages/app/**"], None);

        assert!(filters.keep(&finding("shape", "packages/domain/a.ts", Level::Error)));
        assert!(filters.keep(&finding("shape", "packages/app/a.ts", Level::Error)));
        assert!(!filters.keep(&finding("shape", "packages/ui/a.ts", Level::Error)));
    }

    #[test]
    fn a_level_keeps_only_that_level() {
        let errors = compile(&[], &[], Some(LevelFilter::Error));
        assert!(errors.keep(&finding("shape", "src/a.ts", Level::Error)));
        assert!(!errors.keep(&finding("shape", "src/a.ts", Level::Warning)));

        let warnings = compile(&[], &[], Some(LevelFilter::Warning));
        assert!(!warnings.keep(&finding("shape", "src/a.ts", Level::Error)));
        assert!(warnings.keep(&finding("shape", "src/a.ts", Level::Warning)));
    }

    /// Filters compose with AND. A finding has to survive every one of them,
    /// which is what makes `--rules X --paths Y` mean what a reader assumes.
    #[test]
    fn filters_compose_with_and() {
        let filters = compile(
            &["shape"],
            &["packages/domain/**"],
            Some(LevelFilter::Error),
        );

        assert!(filters.keep(&finding("shape", "packages/domain/a.ts", Level::Error)));

        for wrong in [
            finding("spec", "packages/domain/a.ts", Level::Error),
            finding("shape", "packages/app/a.ts", Level::Error),
            finding("shape", "packages/domain/a.ts", Level::Warning),
        ] {
            assert!(!filters.keep(&wrong), "{wrong:?}");
        }
    }

    /// The whole reason this refuses rather than shrugging: a filter matching
    /// nothing is indistinguishable from a clean repository, and the user
    /// would read the second as good news.
    #[test]
    fn an_unknown_rule_id_is_refused_and_says_what_there_is() {
        let message = Filters::compile(
            Arguments {
                rules: &["shpe".to_owned()],
                ..Arguments::default()
            },
            &config(&["shape", "spec"]),
        )
        .expect_err("no such rule");

        assert_eq!(
            message,
            "no rule is called `shpe`; there is `shape` or `spec`"
        );
    }

    /// With many rules, listing them all would be a wall. The count is what a
    /// user can act on.
    #[test]
    fn with_many_rules_the_message_counts_instead_of_listing() {
        let many: Vec<String> = (0..20).map(|index| format!("rule-{index}")).collect();
        let ids: Vec<&str> = many.iter().map(String::as_str).collect();

        let message = Filters::compile(
            Arguments {
                rules: &["nope".to_owned()],
                ..Arguments::default()
            },
            &config(&ids),
        )
        .expect_err("no such rule");

        assert_eq!(
            message,
            "no rule is called `nope`; this configuration has 20 rules"
        );
    }

    /// A configuration with no rules at all gets a message that says so,
    /// rather than one ending in an empty list.
    #[test]
    fn an_empty_configuration_says_it_has_no_rules() {
        let message = Filters::compile(
            Arguments {
                rules: &["anything".to_owned()],
                ..Arguments::default()
            },
            &config(&[]),
        )
        .expect_err("no such rule");

        assert_eq!(
            message,
            "no rule is called `anything`; this configuration has no rules"
        );
    }

    /// A malformed glob is refused with the message the config's own glob
    /// fields use, because it is the same compiler behind both.
    #[test]
    fn a_malformed_glob_is_refused() {
        let message = Filters::compile(
            Arguments {
                paths: &["packages/[".to_owned()],
                ..Arguments::default()
            },
            &config(&["shape"]),
        )
        .expect_err("not a glob");

        assert!(message.contains("packages/["), "{message}");
        assert!(message.contains("invalid glob"), "{message}");
    }
}
