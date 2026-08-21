//! Resolving `extends` and merging presets into one config.
//!
//! Merge rules, from `docs/CONFIG.md`:
//!
//! - arrays (`modules`, `rules`, `decisions`, `ignore`) are concatenated,
//!   presets first;
//! - scalars (`root`, `version`) come from the local config;
//! - a preset declaring `root` is an error, because it cannot know the layout
//!   of the repository including it;
//! - `disable` drops rules by id after everything is merged.

#[cfg(test)]
use archwarden_core::ids::ModuleId;
use archwarden_core::ids::{DecisionId, RuleId};
use archwarden_resolver::preset::{PresetError, PresetResolver};
use camino::{Utf8Path, Utf8PathBuf};

use crate::{
    config::Config,
    discovery::{LoadError, LoadedConfig, load_file},
};

/// Why a config and its presets could not be combined.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExtendsError {
    /// An `extends` entry did not resolve.
    #[error(transparent)]
    Unresolvable(#[from] PresetError),

    /// A preset resolved but could not be read or parsed.
    #[error(transparent)]
    Unloadable(#[from] LoadError),

    /// The `extends` graph contains a cycle.
    #[error("`extends` forms a cycle: {}", render_cycle(chain))]
    Cycle {
        /// The chain of config files, from the entry point back to the repeat.
        chain: Vec<Utf8PathBuf>,
    },

    /// A preset declared `root`.
    #[error(
        "preset `{path}` declares `root`, which only the repository's own \
         config may set"
    )]
    PresetDeclaresRoot {
        /// The offending preset.
        path: Utf8PathBuf,
    },

    /// Two rules share an id.
    #[error("rule id `{id}` is declared twice, in `{first}` and in `{second}`")]
    DuplicateRuleId {
        /// The repeated id.
        id: RuleId,
        /// Where it was first seen.
        first: Utf8PathBuf,
        /// Where it was seen again.
        second: Utf8PathBuf,
    },

    /// Two decisions share an id.
    ///
    /// Refused for the reason two rules with one id are refused: an id that
    /// names two things names neither, and a rule pointing at it would be
    /// pointing at whichever the merge happened to keep. Issue #100.
    #[error("decision id `{id}` is declared twice, in `{first}` and in `{second}`")]
    DuplicateDecisionId {
        /// The repeated id.
        id: DecisionId,
        /// Where it was first seen.
        first: Utf8PathBuf,
        /// Where it was seen again.
        second: Utf8PathBuf,
    },

    /// One id is a rule in one place and a decision in another.
    ///
    /// The two are one namespace because `config explain` takes either. An
    /// argument that could mean a rule or a decision, and does mean both, is a
    /// command that has to pick one and be wrong half the time — so the
    /// collision is refused here, where both files can be named, rather than
    /// resolved by a precedence rule nobody would remember. Issue #100.
    #[error(
        "`{id}` is a rule in `{rule_at}` and a decision in `{decision_at}`; \
         `config explain` takes either, so one id may not be both"
    )]
    IdIsBothRuleAndDecision {
        /// The id that names two things.
        id: String,
        /// Where the rule is declared.
        rule_at: Utf8PathBuf,
        /// Where the decision is declared.
        decision_at: Utf8PathBuf,
    },

    /// `disable` names a rule that does not exist.
    #[error("`disable` names rule `{id}`, which no config or preset declares")]
    DisableUnknownRule {
        /// The id that matched nothing.
        id: RuleId,
    },
}

fn render_cycle(chain: &[Utf8PathBuf]) -> String {
    chain
        .iter()
        .map(|path| path.as_str())
        .collect::<Vec<_>>()
        .join(" → ")
}

/// A config with every preset folded in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedConfig {
    /// The combined config.
    pub config: Config,
    /// The entry config file.
    pub path: Utf8PathBuf,
    /// Where globs resolve from.
    pub root: Utf8PathBuf,
    /// Every file that contributed, presets first, entry config last.
    ///
    /// `explain` shows this so a user can see where a rule came from, and the
    /// cache hashes it so editing a preset invalidates findings.
    pub sources: Vec<Utf8PathBuf>,
}

/// Folds a loaded config's `extends` chain into a single config.
///
/// # Errors
/// See [`ExtendsError`].
pub fn merge(entry: LoadedConfig, resolver: &PresetResolver) -> Result<MergedConfig, ExtendsError> {
    let mut accumulator = Accumulator::default();
    let mut visiting = Vec::new();

    accumulator.absorb(&entry, &mut visiting, resolver, true)?;

    let LoadedConfig { config, path, root } = entry;
    let Accumulator {
        mut merged,
        sources,
        origins,
        decision_origins,
    } = accumulator;

    // Scalars come from the entry config; only its lists were merged.
    //
    // Destructured rather than assigned field by field, and that is the whole
    // point of the shape: a scalar added to `Config` and not named here used
    // to compile and silently keep the *default* for every config in the
    // world. `governance` shipped that way and reported nothing until an
    // end-to-end run turned it up. Now a new field fails to build until
    // somebody decides which side it comes from.
    let Config {
        version,
        root: config_root,
        schema,
        skip_dirs,
        // Unioned by `absorb` above rather than taken from one side, which is
        // why it is bound and dropped here instead of assigned below. It read
        // the other way until issue #158: a preset cannot know whether the
        // project including it has any `.astro`, but a preset whose every rule
        // is about `.rs` knows exactly what it needs -- and could not ask for
        // it. Decision 35 records which half of that argument won.
        languages: _,
        language,
        // Likewise, and more so: closing the architecture says every file in
        // *this* repository is somebody's responsibility, and a preset cannot
        // know what is in a tree it has never seen. A preset that could turn
        // it on would fail a build on files its author never heard of.
        governance,
        // The lists, already folded across the whole chain by `absorb`.
        extends: _,
        ignore: _,
        modules: _,
        rules: _,
        disable: _,
        // A list like the rules, and for the same reason: a preset that ships
        // decisions is shipping opinions with names, which is what makes one
        // worth adopting rather than copying. Two with one id is refused in
        // `absorb`, where both files can be named. Issue #100.
        decisions: _,
    } = config;

    merged.version = version;
    merged.root = config_root;
    merged.schema = schema;
    merged.skip_dirs = skip_dirs;
    merged.language = language;
    merged.governance = governance;

    check_namespaces_do_not_collide(&origins, &decision_origins)?;
    check_disable_targets(&merged, &origins)?;

    Ok(MergedConfig {
        config: merged,
        path,
        root,
        sources,
    })
}

/// Accumulates presets depth-first, in declaration order.
#[derive(Debug, Default)]
struct Accumulator {
    merged: Config,
    sources: Vec<Utf8PathBuf>,
    /// Which file each rule id came from, for the duplicate-id message.
    origins: Vec<(RuleId, Utf8PathBuf)>,
    /// The same for decision ids, kept apart from the rule ids so a collision
    /// between the two lists can say which side each came from.
    decision_origins: Vec<(DecisionId, Utf8PathBuf)>,
}

impl Accumulator {
    fn absorb(
        &mut self,
        loaded: &LoadedConfig,
        visiting: &mut Vec<Utf8PathBuf>,
        resolver: &PresetResolver,
        is_entry: bool,
    ) -> Result<(), ExtendsError> {
        if visiting.contains(&loaded.path) {
            let mut chain = visiting.clone();
            chain.push(loaded.path.clone());
            return Err(ExtendsError::Cycle { chain });
        }

        if !is_entry && loaded.config.root.is_some() {
            return Err(ExtendsError::PresetDeclaresRoot {
                path: loaded.path.clone(),
            });
        }

        visiting.push(loaded.path.clone());

        // Depth-first, in declaration order: a preset's own presets land
        // before it, and later entries override earlier ones by appearing
        // later in the merged lists.
        let containing_directory = loaded.path.parent().unwrap_or(Utf8Path::new("")).to_owned();
        for specifier in &loaded.config.extends {
            let preset_path = resolver.resolve(&containing_directory, specifier)?;
            let preset = load_file(&preset_path)?;
            self.absorb(&preset, visiting, resolver, false)?;
        }

        self.take_lists_from(loaded)?;
        self.sources.push(loaded.path.clone());

        visiting.pop();
        Ok(())
    }

    fn take_lists_from(&mut self, loaded: &LoadedConfig) -> Result<(), ExtendsError> {
        for (id, _) in loaded
            .config
            .modules
            .iter()
            .flat_map(|m| m.rules.iter().map(|r| (r.id(), &m.id)))
        {
            self.remember(id, &loaded.path)?;
        }
        for rule in &loaded.config.rules {
            self.remember(rule.id(), &loaded.path)?;
        }
        for decision in &loaded.config.decisions {
            self.remember_decision(&decision.id, &loaded.path)?;
        }

        self.merged.modules.extend(loaded.config.modules.clone());
        self.merged.rules.extend(loaded.config.rules.clone());
        self.merged
            .decisions
            .extend(loaded.config.decisions.clone());
        self.merged.disable.extend(loaded.config.disable.clone());

        // A union with every preset in the chain, unlike the scalars below.
        // A preset that ships Rust rules and cannot turn Rust on is a preset
        // that does nothing on adoption day -- silently, as a clean run with a
        // skip note, which is the failure decision 31 named. And it is a set:
        // extending a Rust preset and an Astro one means both, and there is no
        // way to spell a conflict between two members. Issue #158, decision 35.
        for language in &loaded.config.languages {
            if !self.merged.languages.contains(language) {
                self.merged.languages.push(*language);
            }
        }

        let mut ignore = std::mem::take(&mut self.merged.ignore).into_vec();
        ignore.extend(loaded.config.ignore.iter().cloned());
        self.merged.ignore = ignore.into();

        Ok(())
    }

    fn remember(&mut self, id: &RuleId, path: &Utf8Path) -> Result<(), ExtendsError> {
        if let Some((_, first)) = self.origins.iter().find(|(seen, _)| seen == id) {
            return Err(ExtendsError::DuplicateRuleId {
                id: id.clone(),
                first: first.clone(),
                second: path.to_owned(),
            });
        }
        self.origins.push((id.clone(), path.to_owned()));
        Ok(())
    }

    fn remember_decision(&mut self, id: &DecisionId, path: &Utf8Path) -> Result<(), ExtendsError> {
        if let Some((_, first)) = self.decision_origins.iter().find(|(seen, _)| seen == id) {
            return Err(ExtendsError::DuplicateDecisionId {
                id: id.clone(),
                first: first.clone(),
                second: path.to_owned(),
            });
        }
        self.decision_origins.push((id.clone(), path.to_owned()));
        Ok(())
    }
}

/// Rejects an id that is a rule in one place and a decision in another.
///
/// Checked after the whole chain is absorbed rather than as each file lands,
/// because the two lists fill in file order and a rule may be declared before
/// the decision it collides with, or after it. One pass over both at the end
/// sees every pair whichever way round they arrived.
fn check_namespaces_do_not_collide(
    rules: &[(RuleId, Utf8PathBuf)],
    decisions: &[(DecisionId, Utf8PathBuf)],
) -> Result<(), ExtendsError> {
    for (rule, rule_at) in rules {
        if let Some((decision, decision_at)) = decisions
            .iter()
            .find(|(decision, _)| decision.as_str() == rule.as_str())
        {
            return Err(ExtendsError::IdIsBothRuleAndDecision {
                id: decision.as_str().to_owned(),
                rule_at: rule_at.clone(),
                decision_at: decision_at.clone(),
            });
        }
    }
    Ok(())
}

/// Rejects `disable` entries that match nothing.
///
/// A typo here silently disables no rule, which is the failure mode that makes
/// a user believe a rule is off when it is not.
fn check_disable_targets(
    merged: &Config,
    origins: &[(RuleId, Utf8PathBuf)],
) -> Result<(), ExtendsError> {
    for id in &merged.disable {
        if !origins.iter().any(|(seen, _)| seen == id) {
            return Err(ExtendsError::DisableUnknownRule { id: id.clone() });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::load_from;

    fn tree(entries: &[(&str, &str)]) -> (tempfile::TempDir, Utf8PathBuf) {
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

    fn merge_at(root: &Utf8Path) -> Result<MergedConfig, ExtendsError> {
        merge(
            load_from(root).expect("entry config loads"),
            &PresetResolver::new(),
        )
    }

    fn rule(id: &str, roots: &str) -> String {
        format!(r#"{{"type":"structure","id":"{id}","level":"error","roots":"{roots}"}}"#)
    }

    /// A scalar the entry config sets survives the merge.
    ///
    /// The bug this pins shipped: `governance` was added to `Config` and not
    /// to the hand-written list of scalars copied here, so every config in the
    /// world kept the *default* and `governance: closed` reported nothing at
    /// all — the exact silence the setting exists to break. The destructuring
    /// above is what stops the next field going the same way; this is what
    /// says the current one arrived.
    #[test]
    fn a_scalar_the_entry_config_sets_survives_the_merge() {
        let (_guard, root) = tree(&[(
            "arch.config.json",
            r#"{"version":0,"governance":"closed","languages":["ts","astro"],"rules":[]}"#,
        )]);

        let merged = merge_at(&root).expect("merges");

        assert_eq!(
            merged.config.governance.level(),
            Some(archwarden_core::level::Level::Error),
            "the setting reached the merged config"
        );
        assert!(
            merged
                .config
                .languages
                .contains(&crate::config::Language::Astro),
            "and so did its neighbour, which is the shape being protected"
        );
    }

    /// Issue #158. A preset that ships Rust rules and cannot turn Rust on is
    /// a preset that does nothing on adoption day -- and does it *silently*,
    /// as a clean run with a skip note. `docs/CONFIG.md` calls a rule that
    /// enforces nothing while looking like it enforces something the worst
    /// failure a linter has, and this manufactured one.
    #[test]
    fn a_preset_can_turn_on_the_language_its_rules_are_about() {
        let (_guard, root) = tree(&[
            (
                "arch.config.json",
                r#"{"version":0,"extends":"./preset.json","rules":[]}"#,
            ),
            ("preset.json", r#"{"version":0,"languages":["rust"]}"#),
        ]);

        let merged = merge_at(&root).expect("merges");

        assert!(
            merged
                .config
                .languages
                .contains(&crate::config::Language::Rust),
            "the preset's rules are about Rust and could not read a `.rs` file"
        );
    }

    /// A union, not a replacement. Somebody extending a Rust preset and an
    /// Astro one means both, and there is no way to spell a conflict between
    /// two members of a set.
    #[test]
    fn the_languages_of_every_preset_and_the_entry_config_are_unioned() {
        let (_guard, root) = tree(&[
            (
                "arch.config.json",
                r#"{"version":0,"extends":["./rust.json","./astro.json"],
                    "languages":["ts"],"rules":[]}"#,
            ),
            ("rust.json", r#"{"version":0,"languages":["rust"]}"#),
            ("astro.json", r#"{"version":0,"languages":["astro"]}"#),
        ]);

        let merged = merge_at(&root).expect("merges");
        let mut asked: Vec<String> = merged
            .config
            .languages
            .iter()
            .map(|language| format!("{language:?}"))
            .collect();
        asked.sort();

        assert_eq!(asked, ["Astro", "Rust", "Ts"], "{asked:?}");
    }

    /// And naming one twice asks for it once. A set, spelled as a list.
    #[test]
    fn a_language_named_by_two_presets_is_asked_for_once() {
        let (_guard, root) = tree(&[
            (
                "arch.config.json",
                r#"{"version":0,"extends":["./a.json","./b.json"],
                    "languages":["rust"],"rules":[]}"#,
            ),
            ("a.json", r#"{"version":0,"languages":["rust"]}"#),
            ("b.json", r#"{"version":0,"languages":["rust"]}"#),
        ]);

        let merged = merge_at(&root).expect("merges");

        assert_eq!(
            merged
                .config
                .languages
                .iter()
                .filter(|l| **l == crate::config::Language::Rust)
                .count(),
            1
        );
    }

    /// A preset may not close the architecture of a repository it has never
    /// seen.
    ///
    /// The same reasoning that stops a preset setting `root`, one step
    /// stronger: closing it says every file *here* is somebody's
    /// responsibility, and a shared package cannot know what is in this tree.
    /// A preset that could turn it on would fail a build over files its author
    /// never heard of.
    #[test]
    fn a_preset_cannot_close_the_architecture() {
        let (_guard, root) = tree(&[
            (
                "arch.config.json",
                r#"{"version":0,"extends":"./preset.json","rules":[]}"#,
            ),
            ("preset.json", r#"{"version":0,"governance":"closed"}"#),
        ]);

        let merged = merge_at(&root).expect("merges");

        assert_eq!(
            merged.config.governance.level(),
            None,
            "the entry config said nothing, so the architecture is open"
        );
    }

    #[test]
    fn a_config_without_presets_is_returned_as_it_is() {
        let (_guard, root) = tree(&[(
            "arch.config.json",
            &format!(r#"{{"version":0,"rules":[{}]}}"#, rule("a", "x/*")),
        )]);

        let merged = merge_at(&root).expect("merges");
        assert_eq!(merged.config.rules.len(), 1);
        assert_eq!(merged.sources, [root.join("arch.config.json")]);
    }

    /// The motivating case: a preset supplies rules and the local config adds
    /// its own.
    #[test]
    fn preset_rules_and_local_rules_are_concatenated() {
        let (_guard, root) = tree(&[
            (
                "presets/base.json",
                &format!(
                    r#"{{"version":0,"rules":[{}]}}"#,
                    rule("from-preset", "p/*")
                ),
            ),
            (
                "arch.config.json",
                &format!(
                    r#"{{"version":0,"extends":"./presets/base.json","rules":[{}]}}"#,
                    rule("local", "l/*")
                ),
            ),
        ]);

        let merged = merge_at(&root).expect("merges");
        let ids: Vec<_> = merged
            .config
            .rules()
            .map(|(_, _, r)| r.id().as_str())
            .collect();

        assert_eq!(ids, ["from-preset", "local"], "presets come first");
        assert_eq!(
            merged.sources,
            [
                root.join("presets/base.json"),
                root.join("arch.config.json")
            ]
        );
    }

    /// Presets may extend presets. Depth-first order means the deepest
    /// ancestor's rules land first.
    #[test]
    fn presets_may_themselves_extend_presets() {
        let (_guard, root) = tree(&[
            (
                "presets/deep.json",
                &format!(r#"{{"version":0,"rules":[{}]}}"#, rule("deep", "d/*")),
            ),
            (
                "presets/mid.json",
                &format!(
                    r#"{{"version":0,"extends":"./deep.json","rules":[{}]}}"#,
                    rule("mid", "m/*")
                ),
            ),
            (
                "arch.config.json",
                &format!(
                    r#"{{"version":0,"extends":"./presets/mid.json","rules":[{}]}}"#,
                    rule("local", "l/*")
                ),
            ),
        ]);

        let merged = merge_at(&root).expect("merges");
        let ids: Vec<_> = merged
            .config
            .rules()
            .map(|(_, _, r)| r.id().as_str())
            .collect();
        assert_eq!(ids, ["deep", "mid", "local"]);
    }

    /// Without cycle detection this recurses until the stack runs out, and the
    /// user gets a crash instead of a diagnostic.
    #[test]
    fn a_cycle_is_reported_rather_than_recursed_into() {
        let (_guard, root) = tree(&[
            (
                "arch.config.json",
                r#"{"version":0,"extends":"./presets/a.json"}"#,
            ),
            ("presets/a.json", r#"{"version":0,"extends":"./b.json"}"#),
            (
                "presets/b.json",
                r#"{"version":0,"extends":"../arch.config.json"}"#,
            ),
        ]);

        let err = merge_at(&root).expect_err("should detect the cycle");
        let ExtendsError::Cycle { chain } = &err else {
            panic!("expected Cycle, got {err:?}");
        };

        assert_eq!(chain.len(), 4, "entry, a, b, and back to entry");
        assert_eq!(chain.first(), chain.last());
        assert!(err.to_string().contains(" → "), "{err}");
    }

    /// A preset that extends itself is the shortest possible cycle.
    #[test]
    fn a_self_referential_preset_is_a_cycle() {
        let (_guard, root) = tree(&[(
            "arch.config.json",
            r#"{"version":0,"extends":"./arch.config.json"}"#,
        )]);

        assert!(matches!(merge_at(&root), Err(ExtendsError::Cycle { .. })));
    }

    /// A preset cannot know the layout of the repository including it, so
    /// silently relocating every glob is not something it may do.
    #[test]
    fn a_preset_declaring_root_is_refused() {
        let (_guard, root) = tree(&[
            (
                "presets/base.json",
                r#"{"version":0,"root":"../elsewhere"}"#,
            ),
            (
                "arch.config.json",
                r#"{"version":0,"extends":"./presets/base.json"}"#,
            ),
        ]);

        let err = merge_at(&root).expect_err("should refuse");
        let ExtendsError::PresetDeclaresRoot { path } = &err else {
            panic!("expected PresetDeclaresRoot, got {err:?}");
        };
        assert_eq!(path, &root.join("presets/base.json"));
    }

    /// The entry config may set `root`; only presets may not.
    #[test]
    fn the_entry_config_may_declare_root() {
        let (_guard, root) = tree(&[("arch.config.json", r#"{"version":0,"root":"."}"#)]);
        assert_eq!(
            merge_at(&root).expect("merges").config.root.as_deref(),
            Some(".")
        );
    }

    /// Two rules sharing an id would make `explain <id>` and `disable`
    /// ambiguous, so the collision is refused and both files are named.
    #[test]
    fn a_rule_id_declared_twice_names_both_files() {
        let (_guard, root) = tree(&[
            (
                "presets/base.json",
                &format!(r#"{{"version":0,"rules":[{}]}}"#, rule("shared", "p/*")),
            ),
            (
                "arch.config.json",
                &format!(
                    r#"{{"version":0,"extends":"./presets/base.json","rules":[{}]}}"#,
                    rule("shared", "l/*")
                ),
            ),
        ]);

        let err = merge_at(&root).expect_err("should refuse");
        let ExtendsError::DuplicateRuleId { id, first, second } = &err else {
            panic!("expected DuplicateRuleId, got {err:?}");
        };
        assert_eq!(id.as_str(), "shared");
        assert_eq!(first, &root.join("presets/base.json"));
        assert_eq!(second, &root.join("arch.config.json"));
    }

    /// A preset that ships decisions is shipping opinions with names, which is
    /// what makes one worth adopting rather than copying. They fold the way
    /// rules fold: concatenated, presets first.
    #[test]
    fn a_preset_may_ship_decisions_and_they_merge() {
        let (_guard, root) = tree(&[
            (
                "presets/base.json",
                r#"{"version":0,"decisions":[
                     {"id":"PRESET-1","title":"the preset's opinion","link":"docs/a.md"}],
                   "rules":[]}"#,
            ),
            (
                "arch.config.json",
                r#"{"version":0,"extends":"./presets/base.json",
                    "decisions":[{"id":"LOCAL-1","title":"ours"}],"rules":[]}"#,
            ),
        ]);

        let merged = merge_at(&root).expect("merges");
        let ids: Vec<&str> = merged
            .config
            .decisions
            .iter()
            .map(|d| d.id.as_str())
            .collect();

        assert_eq!(ids, ["PRESET-1", "LOCAL-1"], "presets first, like rules");
        assert_eq!(
            merged.config.decisions[0].link.as_deref(),
            Some("docs/a.md"),
            "the prose travelled with it"
        );
    }

    /// Two decisions with one id is refused for the same reason two rules
    /// with one id already are: an id that names two things names neither.
    /// Both files are named, because the fix is in one of them.
    #[test]
    fn a_decision_id_declared_twice_names_both_files() {
        let (_guard, root) = tree(&[
            (
                "presets/base.json",
                r#"{"version":0,"decisions":[{"id":"ADR-014","title":"theirs"}],"rules":[]}"#,
            ),
            (
                "arch.config.json",
                r#"{"version":0,"extends":"./presets/base.json",
                    "decisions":[{"id":"ADR-014","title":"ours"}],"rules":[]}"#,
            ),
        ]);

        let err = merge_at(&root).expect_err("should refuse");
        let ExtendsError::DuplicateDecisionId { id, first, second } = &err else {
            panic!("expected DuplicateDecisionId, got {err:?}");
        };
        assert_eq!(id.as_str(), "ADR-014");
        assert_eq!(first, &root.join("presets/base.json"));
        assert_eq!(second, &root.join("arch.config.json"));
    }

    /// Within one file too, where it is a plain copy-paste.
    #[test]
    fn a_decision_id_repeated_within_one_file_is_refused() {
        let (_guard, root) = tree(&[(
            "arch.config.json",
            r#"{"version":0,"decisions":[
                 {"id":"same","title":"a"},{"id":"same","title":"b"}],"rules":[]}"#,
        )]);

        assert!(matches!(
            merge_at(&root),
            Err(ExtendsError::DuplicateDecisionId { .. })
        ));
    }

    /// The namespaces are one namespace, because `config explain` takes
    /// either. An argument that could mean a rule or a decision, and does
    /// mean both, is a command that has to choose one and be wrong half the
    /// time — so the collision is refused where both can be named.
    #[test]
    fn an_id_cannot_be_a_rule_and_a_decision_at_once() {
        let (_guard, root) = tree(&[(
            "arch.config.json",
            &format!(
                r#"{{"version":0,"decisions":[{{"id":"sealed","title":"a"}}],"rules":[{}]}}"#,
                rule("sealed", "a/*")
            ),
        )]);

        let err = merge_at(&root).expect_err("should refuse");
        assert!(
            matches!(err, ExtendsError::IdIsBothRuleAndDecision { .. }),
            "got {err:?}"
        );
        assert!(
            err.to_string().contains("sealed"),
            "the message names the id: {err}"
        );
    }

    /// And the collision is caught across files as well as within one, which
    /// is the case a preset creates: a project defining a rule called
    /// `ADR-014` after adopting a preset that decided `ADR-014`.
    #[test]
    fn the_two_namespaces_collide_across_a_preset_too() {
        let (_guard, root) = tree(&[
            (
                "presets/base.json",
                r#"{"version":0,"decisions":[{"id":"ADR-014","title":"theirs"}],"rules":[]}"#,
            ),
            (
                "arch.config.json",
                &format!(
                    r#"{{"version":0,"extends":"./presets/base.json","rules":[{}]}}"#,
                    rule("ADR-014", "a/*")
                ),
            ),
        ]);

        assert!(matches!(
            merge_at(&root),
            Err(ExtendsError::IdIsBothRuleAndDecision { .. })
        ));
    }

    /// Collisions inside one file count too, not only across presets.
    #[test]
    fn a_rule_id_repeated_within_one_file_is_refused() {
        let (_guard, root) = tree(&[(
            "arch.config.json",
            &format!(
                r#"{{"version":0,"rules":[{},{}]}}"#,
                rule("same", "a/*"),
                rule("same", "b/*")
            ),
        )]);

        assert!(matches!(
            merge_at(&root),
            Err(ExtendsError::DuplicateRuleId { .. })
        ));
    }

    /// The point of `disable`: one unwanted rule must not make a whole preset
    /// unusable.
    #[test]
    fn disable_drops_an_inherited_rule() {
        let (_guard, root) = tree(&[
            (
                "presets/base.json",
                &format!(
                    r#"{{"version":0,"rules":[{},{}]}}"#,
                    rule("keep", "k/*"),
                    rule("drop", "d/*")
                ),
            ),
            (
                "arch.config.json",
                r#"{"version":0,"extends":"./presets/base.json","disable":["drop"]}"#,
            ),
        ]);

        let merged = merge_at(&root).expect("merges");
        let ids: Vec<_> = merged
            .config
            .rules()
            .map(|(_, _, r)| r.id().as_str())
            .collect();
        assert_eq!(ids, ["keep"]);
    }

    /// A typo in `disable` would silently disable nothing, leaving the user
    /// convinced a rule is off when it is not. It fails loudly instead.
    #[test]
    fn disabling_a_rule_that_does_not_exist_is_refused() {
        let (_guard, root) = tree(&[(
            "arch.config.json",
            &format!(
                r#"{{"version":0,"disable":["typoed"],"rules":[{}]}}"#,
                rule("real", "r/*")
            ),
        )]);

        let err = merge_at(&root).expect_err("should refuse");
        let ExtendsError::DisableUnknownRule { id } = &err else {
            panic!("expected DisableUnknownRule, got {err:?}");
        };
        assert_eq!(id.as_str(), "typoed");
    }

    /// `ignore` accumulates across presets: a preset excluding `dist` and a
    /// project excluding `generated` should end up excluding both.
    #[test]
    fn ignore_lists_accumulate_across_presets() {
        let (_guard, root) = tree(&[
            (
                "presets/base.json",
                r#"{"version":0,"ignore":["**/dist/**"]}"#,
            ),
            (
                "arch.config.json",
                r#"{"version":0,"extends":"./presets/base.json","ignore":["**/generated/**"]}"#,
            ),
        ]);

        let merged = merge_at(&root).expect("merges");
        assert_eq!(
            merged.config.ignore.as_slice(),
            ["**/dist/**", "**/generated/**"]
        );
    }

    /// Modules survive the merge with their labels, so a finding from a
    /// preset's rule still reports which module it belonged to.
    #[test]
    fn modules_keep_their_labels_through_a_merge() {
        let (_guard, root) = tree(&[
            (
                "presets/base.json",
                &format!(
                    r#"{{"version":0,"modules":[{{"id":"domain","rules":[{}]}}]}}"#,
                    rule("preset-rule", "p/*")
                ),
            ),
            (
                "arch.config.json",
                r#"{"version":0,"extends":"./presets/base.json"}"#,
            ),
        ]);

        let merged = merge_at(&root).expect("merges");
        let (module, _, rule) = merged.config.rules().next().expect("one rule");
        assert_eq!(module.map(ModuleId::as_str), Some("domain"));
        assert_eq!(rule.id().as_str(), "preset-rule");
    }

    /// An npm-style preset goes through the resolver, not through path
    /// handling, and lands in `sources` like any other contributor.
    #[test]
    fn a_package_preset_is_resolved_and_recorded() {
        let (_guard, root) = tree(&[
            (
                "node_modules/@org/preset/package.json",
                r#"{"name":"@org/preset","main":"preset.json"}"#,
            ),
            (
                "node_modules/@org/preset/preset.json",
                &format!(r#"{{"version":0,"rules":[{}]}}"#, rule("from-npm", "n/*")),
            ),
            (
                "arch.config.json",
                r#"{"version":0,"extends":"@org/preset"}"#,
            ),
        ]);

        let merged = merge_at(&root).expect("merges");
        let ids: Vec<_> = merged
            .config
            .rules()
            .map(|(_, _, r)| r.id().as_str())
            .collect();
        assert_eq!(ids, ["from-npm"]);
        assert!(
            merged.sources[0].as_str().contains("node_modules"),
            "{:?}",
            merged.sources
        );
    }

    #[test]
    fn an_unresolvable_preset_is_reported_as_such() {
        let (_guard, root) = tree(&[(
            "arch.config.json",
            r#"{"version":0,"extends":"@org/never-installed"}"#,
        )]);

        assert!(matches!(
            merge_at(&root),
            Err(ExtendsError::Unresolvable(_))
        ));
    }

    /// A preset that resolves but does not parse is a load failure, not a
    /// resolution one: the distinction tells the user whether to check their
    /// install or the preset's contents.
    #[test]
    fn a_malformed_preset_is_reported_as_a_load_failure() {
        let (_guard, root) = tree(&[
            ("presets/broken.json", "{ not json"),
            (
                "arch.config.json",
                r#"{"version":0,"extends":"./presets/broken.json"}"#,
            ),
        ]);

        assert!(matches!(merge_at(&root), Err(ExtendsError::Unloadable(_))));
    }
}
