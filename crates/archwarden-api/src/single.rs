//! Judging one write, all the way to what a surface has to say about it.
//!
//! `archwarden_engine::single` answers the narrow question — what do the rules
//! find about this path? Two things always happen to that answer before anyone
//! reports it, and both are decisions rather than presentation:
//!
//! 1. **The baseline is applied.** Debt the repository already accepted is not
//!    this write's fault, and a gate that refused it would be uninstalled the
//!    same day.
//! 2. **Findings this write is fixing are separated from the ones it breaks.**
//!    A `presence` rule's finding is about a *directory*, and a write supplying
//!    one of its required files leaves the directory less broken than it found
//!    it. Refusing that made a rule of several files unsatisfiable in any
//!    order: the first write refused for the absence of the second, the second
//!    for the third, and the directory could not be created at all. Issue #57.
//!
//! Both used to live in the pre-write hook, which was the only surface asking.
//! MCP asks the same question — that is the tool issue #65 says earns it — and
//! a server that ran the engine without these two would answer *"this write is
//! illegal"* about a write the hook permits. Two surfaces, two answers, same
//! repository: the drift this crate exists to make impossible.
//!
//! What stays outside is genuinely the surface's: reconstructing an `Edit` into
//! the text it would leave is the harness's protocol, and turning [`Checked`]
//! into a denial or a JSON-RPC result is how each surface speaks.

use archwarden_core::{compiled::CompiledConfig, finding::Finding, path::RepoRelPath};
use archwarden_engine::single::Single;
use camino::Utf8Path;

/// What the rules say about one write, split by what the write is doing.
#[derive(Debug, Clone)]
pub struct Checked {
    /// Findings this write breaks, with the baseline's already removed.
    ///
    /// This is what decides whether a write is refused.
    pub single: Single,
    /// Findings this write is progress against, in the order they were found.
    ///
    /// Not violations. They are reported — what is still missing is what the
    /// agent has to write next — and they never gate.
    pub fixing: Vec<Finding>,
}

impl Checked {
    /// Whether what remains is bad enough to refuse the write.
    ///
    /// Reads [`Checked::single`] only: a finding this write is fixing has
    /// never gated anything, whatever its level.
    #[must_use]
    pub fn refuses(&self) -> bool {
        self.single.fails_build()
    }
}

/// Checks one path, as it would be after this write.
///
/// `content` is the text the write would leave. `None` judges the file as it
/// stands, which is the honest answer for a tool whose write cannot be
/// replayed — and is what every surface did before the write itself could be
/// seen.
///
/// A baseline that will not load allows the write rather than refusing it,
/// like every other failure on this path: a configuration problem is the
/// user's to fix at their own pace, not a reason to stop them writing a file.
#[must_use]
pub fn check(
    root: &Utf8Path,
    config: &CompiledConfig,
    path: &RepoRelPath,
    content: Option<&str>,
) -> Checked {
    let mut single = match content {
        Some(content) => archwarden_engine::single::check_write(root, config, path, content),
        None => archwarden_engine::single::check_file(root, config, path),
    };

    if let Ok(Some(baseline)) = crate::baseline::Baseline::load(root) {
        single.findings.retain(|finding| !baseline.accepts(finding));
    }

    let mut fixing = Vec::new();
    if let Some(name) = path.file_name() {
        single.findings.retain(|finding| {
            let progress = is_progress(finding, name);
            if progress {
                fixing.push(finding.clone());
            }
            !progress
        });
    }

    Checked { single, fixing }
}

/// Whether a finding is one this write is fixing rather than causing.
///
/// True only for a directory's required files, and only when the file being
/// written is one of them. A file the rule never asked for leaves the directory
/// as broken as it found it — that half is what keeps this from being a way to
/// switch `presence` off.
#[must_use]
pub fn is_progress(finding: &Finding, written: &str) -> bool {
    use archwarden_core::finding::Expectation;

    let Expectation::RequiredFiles { names, patterns } = &finding.expected else {
        return false;
    };

    if names.iter().any(|name| name == written) {
        return true;
    }

    // Compiled with the engine that compiled the rule, so this cannot disagree
    // with `check` about whether a name satisfies the pattern.
    patterns.iter().any(|pattern| {
        archwarden_core::pattern::Pattern::compile(pattern)
            .is_ok_and(|compiled| compiled.is_match(written))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::{CompiledRule, CompiledRuleKind, SkipDirs},
        glob::PathSet,
        hash::ContentHash,
        ids::RuleId,
        level::Level,
        scope::Scope,
    };
    use archwarden_core::{
        facts::KindFilter,
        finding::{Expectation, Observed},
    };
    use camino::Utf8PathBuf;

    /// A directory that must carry three files, which is the shape issue #57
    /// is about: no order of writes passes if each is judged alone.
    fn presence_config() -> CompiledConfig {
        CompiledConfig::new(
            vec![CompiledRule {
                id: RuleId::new("tem-os-tres").expect("valid id"),
                module: None,
                why: None,
                module_why: None,
                level: Level::Error,
                scope: Scope::compile(["projetos/*"]).expect("valid scope"),
                kind: CompiledRuleKind::Presence {
                    require: vec![
                        "projeto.md".to_owned(),
                        "exercicios.md".to_owned(),
                        "diagram.json".to_owned(),
                    ],
                    require_any: Vec::new(),
                },
            }],
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b"single"),
        )
    }

    fn repository() -> (tempfile::TempDir, Utf8PathBuf) {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("utf-8");
        std::fs::create_dir_all(root.join("projetos/01-blink")).expect("create");
        (guard, root)
    }

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// The write supplies one of the three, so the directory is less broken
    /// than it was. It is reported and it does not refuse.
    #[test]
    fn a_write_supplying_a_required_file_is_progress_and_never_refuses() {
        let (_guard, root) = repository();

        let checked = check(
            &root,
            &presence_config(),
            &path("projetos/01-blink/projeto.md"),
            Some("# blink\n"),
        );

        assert!(
            !checked.refuses(),
            "a write that is fixing must not be denied"
        );
        assert!(
            !checked.fixing.is_empty(),
            "and what is still missing has to be reported"
        );
    }

    /// And a write supplying none of them leaves the directory exactly as
    /// broken, so it is refused. This is the half that keeps the rule above
    /// from being a way to switch `presence` off.
    #[test]
    fn a_write_the_rule_never_asked_for_is_still_refused() {
        let (_guard, root) = repository();

        let checked = check(
            &root,
            &presence_config(),
            &path("projetos/01-blink/qualquer.md"),
            Some("# qualquer\n"),
        );

        assert!(checked.refuses(), "the directory is no less broken");
        assert!(checked.fixing.is_empty());
    }

    /// Debt the repository already accepted is not this write's fault. Without
    /// this, an agent asked to edit a legacy file is refused for something it
    /// did not do, and the hook is uninstalled by lunchtime.
    #[test]
    fn a_finding_the_baseline_accepts_does_not_refuse_the_write() {
        let (_guard, root) = repository();
        let config = presence_config();
        let target = path("projetos/01-blink/qualquer.md");

        let before = check(&root, &config, &target, Some("# qualquer\n"));
        assert!(
            before.refuses(),
            "refused before the baseline says otherwise"
        );

        crate::baseline::Baseline::of(&before.single.findings)
            .write(&root)
            .expect("the baseline is written");

        let after = check(&root, &config, &target, Some("# qualquer\n"));
        assert!(
            !after.refuses(),
            "the same write, against a baseline that accepts it"
        );
    }

    /// `None` judges the file as it stands, which is what a tool whose write
    /// cannot be replayed gets.
    #[test]
    fn no_pending_content_judges_what_is_on_disk() {
        let (_guard, root) = repository();
        std::fs::write(root.join("projetos/01-blink/projeto.md"), "# blink\n").expect("write");

        let checked = check(
            &root,
            &presence_config(),
            &path("projetos/01-blink/projeto.md"),
            None,
        );

        assert!(!checked.refuses());
    }

    /// Issue #57. A `presence` rule requiring several files makes every one of
    /// them illegal until all of them exist, so no write order passes and the
    /// directory cannot be created at all.
    ///
    /// The rigorous reading, and the one implemented: **a write passes while it
    /// is fixing the problem.** Judged by what the write does, not by the state
    /// it lands in — the same correction as #55, one layer up.
    #[test]
    fn a_write_that_supplies_a_required_file_is_progress() {
        let missing = |name: &str| Finding {
            rule_id: RuleId::new("tem-os-tres").expect("valid"),
            module_id: None,
            level: Level::Error,
            path: RepoRelPath::new("projetos/02-novo").expect("valid"),
            span: None,
            observed: Observed::RequiredFileMissing {
                name: name.to_owned(),
            },
            expected: Expectation::RequiredFiles {
                names: vec![
                    "projeto.md".to_owned(),
                    "exercicios.md".to_owned(),
                    "diagram.json".to_owned(),
                ],
                patterns: Vec::new(),
            },
        };

        assert!(
            is_progress(&missing("exercicios.md"), "projeto.md"),
            "writing one of the required files is fixing the directory"
        );
        assert!(
            is_progress(&missing("diagram.json"), "exercicios.md"),
            "and so is the second one"
        );
    }

    /// A write that ignores the problem is still refused. This is the half that
    /// keeps the relaxation from being a way to switch `presence` off.
    #[test]
    fn a_write_that_ignores_the_missing_files_is_not_progress() {
        let finding = Finding {
            rule_id: RuleId::new("tem-os-tres").expect("valid"),
            module_id: None,
            level: Level::Error,
            path: RepoRelPath::new("projetos/01-blink").expect("valid"),
            span: None,
            observed: Observed::RequiredFileMissing {
                name: "diagram.json".to_owned(),
            },
            expected: Expectation::RequiredFiles {
                names: vec!["projeto.md".to_owned(), "diagram.json".to_owned()],
                patterns: Vec::new(),
            },
        };

        assert!(
            !is_progress(&finding, "rascunho.md"),
            "a file the rule never asked for leaves the directory as broken as it was"
        );
    }

    /// A `require_any` entry is a regex, and a file matching one is progress
    /// the same way a named file is.
    #[test]
    fn a_write_matching_a_required_pattern_is_progress() {
        let finding = Finding {
            rule_id: RuleId::new("tem-um-ino").expect("valid"),
            module_id: None,
            level: Level::Error,
            path: RepoRelPath::new("projetos/02-novo/sketch").expect("valid"),
            span: None,
            observed: Observed::NoFileMatching {
                pattern: r"\.ino$".to_owned(),
            },
            expected: Expectation::RequiredFiles {
                names: Vec::new(),
                patterns: vec![r"\.ino$".to_owned()],
            },
        };

        assert!(is_progress(&finding, "sketch.ino"));
        assert!(!is_progress(&finding, "leiame.md"));
    }

    /// Every other rule keeps denying. `spec-pair` has an order that works —
    /// the spec first, which is the whole point of a TDD gate — and a
    /// `structure` violation is caused by the write rather than pre-existing
    /// it.
    #[test]
    fn a_finding_that_is_not_about_a_missing_file_is_never_progress() {
        let finding = Finding {
            rule_id: RuleId::new("usecase-name").expect("valid"),
            module_id: None,
            level: Level::Error,
            path: RepoRelPath::new("src/user/create.use-case.ts").expect("valid"),
            span: None,
            observed: Observed::ExportMissing {
                name: "Create".to_owned(),
            },
            expected: Expectation::RequiredExport {
                kind: KindFilter::Any,
                name: "Create".to_owned(),
                annotation: Vec::new(),
                signature_hint: None,
            },
        };

        assert!(!is_progress(&finding, "create.use-case.ts"));
    }
}
