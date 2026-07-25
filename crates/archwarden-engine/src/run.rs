//! Running the rules over a walked tree.
//!
//! The pipeline's last stage, and deliberately dull: it owns no rule logic and
//! no filesystem access. It offers each directory to each engine and collects
//! what comes back, which is what keeps every interesting decision inside a
//! rule where it can be tested on its own.

use archwarden_core::{compiled::CompiledConfig, finding::Finding, level::Level};

use crate::walk::RepoTree;

/// What a run found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Every finding, worst-first then by path then by rule.
    pub findings: Vec<Finding>,
    /// How many directories were examined.
    pub directories_scanned: usize,
    /// How many files were examined.
    pub files_scanned: usize,
    /// Rules whose kind has no engine yet, by id.
    ///
    /// Reported rather than dropped: a run that silently skipped a rule would
    /// print a clean result the user has no reason to distrust.
    pub unimplemented_rules: Vec<String>,
}

impl Report {
    /// How many findings are at error level.
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.level.fails_build())
            .count()
    }

    /// How many findings are at warning level.
    #[must_use]
    pub fn warning_count(&self) -> usize {
        self.findings.len() - self.error_count()
    }

    /// Whether this run should fail a build.
    #[must_use]
    pub fn fails_build(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.level.fails_build())
    }
}

/// Runs every rule in `config` against `tree`.
#[must_use]
pub fn check(config: &CompiledConfig, tree: &RepoTree) -> Report {
    let (engines, unimplemented_rules) = archwarden_rules::engines_for(config);

    let mut findings = Vec::new();
    let mut files_scanned = 0;

    for (path, directory) in tree.directories() {
        let file_names = directory.file_names();
        files_scanned += file_names.len();

        for engine in &engines {
            findings.extend(
                engine.check_directory(archwarden_core::traits::DirectoryContext {
                    path,
                    subdirectories: &directory.subdirectories,
                    files: &file_names,
                }),
            );
        }
    }

    // Determinism is a design goal: the same inputs must produce byte-identical
    // output, or every snapshot test and CI diff becomes noise.
    findings.sort();

    Report {
        findings,
        directories_scanned: tree.directory_count(),
        files_scanned,
        unimplemented_rules,
    }
}

/// The level a report should be summarised at, for a caller choosing an exit
/// code without walking the findings itself.
#[must_use]
pub fn worst_level(report: &Report) -> Option<Level> {
    report.findings.iter().map(|finding| finding.level).max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::{CompiledRule, CompiledRuleKind, SkipDirs},
        glob::PathSet,
        hash::ContentHash,
        ids::{ModuleId, RuleId},
        scope::Scope,
    };
    use camino::Utf8PathBuf;

    fn tree_at(entries: &[(&str, &str)]) -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
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

    fn rule(
        id: &str,
        module: Option<&str>,
        scope: &[&str],
        kind: CompiledRuleKind,
    ) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: module.map(|m| ModuleId::new(m).expect("valid module")),
            level: Level::Error,
            scope: Scope::compile(scope).expect("valid scope"),
            kind,
        }
    }

    fn config(rules: Vec<CompiledRule>) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::default(),
            SkipDirs {
                prefixes: vec!["_".to_owned()],
                globs: PathSet::default(),
                scope: archwarden_core::compiled::SkipScope::Structure,
            },
            ContentHash::of(b""),
        )
    }

    fn structure(allowed: &[&str]) -> CompiledRuleKind {
        CompiledRuleKind::Structure {
            allowed_subfolders: allowed.iter().map(|s| (*s).to_owned()).collect(),
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            filename_patterns: Vec::new(),
        }
    }

    fn spec_pair() -> CompiledRuleKind {
        CompiledRuleKind::SpecPair {
            subfolders: vec![".".to_owned()],
            spec_markers: vec!["spec".to_owned(), "test".to_owned()],
            ignore_files: PathSet::default(),
            require_non_empty_spec: false,
        }
    }

    fn run(entries: &[(&str, &str)], config: &CompiledConfig) -> Report {
        let (guard, root) = tree_at(entries);
        let tree = crate::walk::walk(&root, config).expect("walks");
        let report = check(config, &tree);
        drop(guard);
        report
    }

    fn offenders(report: &Report) -> Vec<&str> {
        report.findings.iter().map(|f| f.path.as_str()).collect()
    }

    /// The whole pipeline, end to end: files on disk in, findings out.
    #[test]
    fn a_repository_is_walked_and_checked() {
        let report = run(
            &[
                ("packages/domain/src/user/types/id.ts", ""),
                ("packages/domain/src/user/wrong-folder/x.ts", ""),
                ("packages/domain/src/invoice/types/invoice.ts", ""),
            ],
            &config(vec![rule(
                "shape",
                Some("domain"),
                &["packages/domain/src/*"],
                structure(&["types", "calcs"]),
            )]),
        );

        assert_eq!(
            offenders(&report),
            ["packages/domain/src/user/wrong-folder"]
        );
        assert_eq!(report.error_count(), 1);
        assert_eq!(report.warning_count(), 0);
        assert!(report.fails_build());
        assert_eq!(
            report
                .findings
                .first()
                .expect("one")
                .module_id
                .as_ref()
                .map(ModuleId::as_str),
            Some("domain")
        );
    }

    /// Two rules of different kinds over one tree, which is the shape any real
    /// config has.
    #[test]
    fn rules_of_different_kinds_run_over_the_same_tree() {
        let report = run(
            &[
                ("src/user/types/id.ts", ""),
                ("src/user/user.ts", ""),
                ("src/user/nope/x.ts", ""),
            ],
            &config(vec![
                rule("shape", None, &["src/*"], structure(&["types"])),
                rule("needs-spec", None, &["src/*"], spec_pair()),
            ]),
        );

        assert_eq!(
            offenders(&report),
            ["src/user/nope", "src/user/user.ts"],
            "one finding from each rule"
        );
    }

    /// Determinism is a design goal. The same tree must produce byte-identical
    /// output, or every snapshot test becomes noise.
    #[test]
    fn findings_are_ordered_the_same_way_every_run() {
        let entries = [
            ("src/zebra/nope/x.ts", ""),
            ("src/alpha/nope/x.ts", ""),
            ("src/middle/nope/x.ts", ""),
        ];
        let config = config(vec![rule("shape", None, &["src/*"], structure(&["types"]))]);

        let first = run(&entries, &config);
        let second = run(&entries, &config);

        assert_eq!(first.findings, second.findings);
        assert_eq!(
            offenders(&first),
            ["src/alpha/nope", "src/middle/nope", "src/zebra/nope"]
        );
    }

    /// Errors sort before warnings, so the first thing a reader sees is the
    /// thing that blocks them.
    #[test]
    fn errors_sort_before_warnings() {
        let mut warning = rule("warn-rule", None, &["src/*"], structure(&["types"]));
        warning.level = Level::Warning;

        let report = run(
            &[("src/aaa/nope/x.ts", ""), ("src/zzz/nope/x.ts", "")],
            &config(vec![
                warning,
                rule("error-rule", None, &["src/*"], structure(&["types"])),
            ]),
        );

        let levels: Vec<_> = report.findings.iter().map(|f| f.level).collect();
        assert_eq!(
            levels,
            [Level::Error, Level::Error, Level::Warning, Level::Warning]
        );
        assert_eq!(report.error_count(), 2);
        assert_eq!(report.warning_count(), 2);
        assert_eq!(worst_level(&report), Some(Level::Error));
    }

    /// A run with only warnings does not fail a build. Decision 1: warnings
    /// track debt without blocking.
    #[test]
    fn warnings_alone_do_not_fail_the_build() {
        let mut warning = rule("warn-rule", None, &["src/*"], structure(&["types"]));
        warning.level = Level::Warning;

        let report = run(&[("src/user/nope/x.ts", "")], &config(vec![warning]));

        assert_eq!(report.warning_count(), 1);
        assert_eq!(report.error_count(), 0);
        assert!(!report.fails_build());
        assert_eq!(worst_level(&report), Some(Level::Warning));
    }

    #[test]
    fn a_clean_repository_reports_nothing() {
        let report = run(
            &[("src/user/types/id.ts", ""), ("src/user/user.spec.ts", "")],
            &config(vec![rule("shape", None, &["src/*"], structure(&["types"]))]),
        );

        assert!(report.findings.is_empty());
        assert!(!report.fails_build());
        assert_eq!(worst_level(&report), None);
    }

    /// The counts are what the summary line reports, so they have to mean what
    /// a reader assumes: how much was actually looked at.
    #[test]
    fn the_report_counts_what_was_examined() {
        let report = run(
            &[
                ("src/user/a.ts", ""),
                ("src/user/b.ts", ""),
                ("src/invoice/c.ts", ""),
                ("README.md", ""),
            ],
            &config(Vec::new()),
        );

        assert_eq!(report.files_scanned, 4);
        assert_eq!(
            report.directories_scanned, 4,
            "the root, src, src/user and src/invoice"
        );
    }

    /// A rule kind with no engine yet is named, so a caller can say what was
    /// not checked rather than presenting a clean run.
    #[test]
    fn a_rule_with_no_engine_is_named_in_the_report() {
        let report = run(
            &[("src/user/a.ts", "")],
            &config(vec![rule(
                "future-rule",
                None,
                &["src/*"],
                CompiledRuleKind::ImportBoundary {
                    forbid: PathSet::default(),
                    require: PathSet::default(),
                    except: PathSet::default(),
                    include_type_only: true,
                },
            )]),
        );

        assert_eq!(report.unimplemented_rules, ["future-rule"]);
        assert!(report.findings.is_empty());
    }

    /// The escape hatch reaches the run: a `_`-prefixed folder is invisible to
    /// the structure rule while its files stay in the tree.
    #[test]
    fn the_escape_hatch_survives_the_whole_pipeline() {
        let report = run(
            &[
                ("src/user/_internal/helper.ts", ""),
                ("src/user/nope/x.ts", ""),
            ],
            &config(vec![rule("shape", None, &["src/*"], structure(&["types"]))]),
        );

        assert_eq!(offenders(&report), ["src/user/nope"]);
        assert_eq!(report.files_scanned, 2, "the exempt file is still counted");
    }
}
