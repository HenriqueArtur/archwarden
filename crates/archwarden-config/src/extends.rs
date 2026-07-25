//! Resolving `extends` and merging presets into one config.
//!
//! Merge rules, from `docs/CONFIG.md`:
//!
//! - arrays (`modules`, `rules`, `ignore`) are concatenated, presets first;
//! - scalars (`root`, `version`) come from the local config;
//! - a preset declaring `root` is an error, because it cannot know the layout
//!   of the repository including it;
//! - `disable` drops rules by id after everything is merged.

#[cfg(test)]
use archwarden_core::ids::ModuleId;
use archwarden_core::ids::RuleId;
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
    } = accumulator;

    // Scalars come from the entry config; only its lists were merged.
    merged.version = config.version;
    merged.root = config.root;
    merged.schema = config.schema;
    merged.skip_dirs = config.skip_dirs;

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

        self.merged.modules.extend(loaded.config.modules.clone());
        self.merged.rules.extend(loaded.config.rules.clone());
        self.merged.disable.extend(loaded.config.disable.clone());

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
            .map(|(_, r)| r.id().as_str())
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
            .map(|(_, r)| r.id().as_str())
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
            .map(|(_, r)| r.id().as_str())
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
        let (module, rule) = merged.config.rules().next().expect("one rule");
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
            .map(|(_, r)| r.id().as_str())
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
