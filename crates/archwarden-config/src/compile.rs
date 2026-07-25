//! Lowering a merged config into compiled rules.
//!
//! This is where "your config might be valid" becomes "your config is valid".
//! Every glob is built, every regex is compiled, every template is checked
//! against the capture groups its pattern actually defines. What comes out
//! cannot be invalid, so no rule engine ever re-checks any of it.

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRule, CompiledRuleKind, SkipDirs, SkipScope},
    facts::{ExportKind, ExportTags, KindFilter},
    glob::{GlobError, PathSet},
    hash::ContentHash,
    ids::RuleId,
    pattern::{Pattern, PatternError},
    scope::{Scope, ScopeError},
    template,
};

use crate::{
    config::{self, Config},
    extends::MergedConfig,
    rule::{MustExport, Rule, SpecPairRule},
};

/// The spelling `must_export.kind` uses for "any declaration form".
const KIND_ANY: &str = "any";

/// Why a config could not be compiled.
///
/// Every variant names the rule, because a config has many and a message that
/// does not say which one leaves the user searching.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CompileError {
    /// A rule's scope glob is not valid.
    #[error("rule `{rule}`: {source}")]
    Scope {
        /// The rule.
        rule: RuleId,
        /// What went wrong.
        #[source]
        source: ScopeError,
    },

    /// A glob outside a scope is not valid.
    #[error("rule `{rule}`, field `{field}`: {source}")]
    Glob {
        /// The rule.
        rule: RuleId,
        /// Which field held the glob.
        field: &'static str,
        /// What went wrong.
        #[source]
        source: GlobError,
    },

    /// A filename pattern is not valid, or uses an unsupported construct.
    #[error("rule `{rule}`, field `{field}`: {source}")]
    Pattern {
        /// The rule.
        rule: RuleId,
        /// Which field held the pattern.
        field: &'static str,
        /// What went wrong.
        #[source]
        source: Box<PatternError>,
    },

    /// `must_export.kind` names something that is not a declaration form.
    #[error(
        "rule `{rule}`: `{name}` is not an export kind. \
         Valid kinds are {available}, or `any`."
    )]
    UnknownExportKind {
        /// The rule.
        rule: RuleId,
        /// The name as written.
        name: String,
        /// The valid names.
        available: String,
    },

    /// A `spec-pair` marker is not a single filename component.
    #[error(
        "rule `{rule}`: `{marker}` is not a spec marker. A marker is one \
         filename component such as `spec` or `test`; the extension comes \
         from the source file, so `Component.tsx` wants `Component.spec.tsx` \
         without being told."
    )]
    InvalidSpecMarker {
        /// The rule.
        rule: RuleId,
        /// The marker as written.
        marker: String,
    },

    /// A `must_export.name` template refers to a capture group that its
    /// `file_pattern` does not define.
    #[error("rule `{rule}`: {source}")]
    Template {
        /// The rule.
        rule: RuleId,
        /// What went wrong.
        #[source]
        source: template::TemplateError,
    },

    /// The top-level `ignore` list holds an invalid glob.
    #[error("`ignore`: {source}")]
    Ignore {
        /// What went wrong.
        #[source]
        source: GlobError,
    },

    /// `skip_dirs.globs` holds an invalid glob.
    #[error("`skip_dirs.globs`: {source}")]
    SkipDirs {
        /// What went wrong.
        #[source]
        source: GlobError,
    },
}

/// Compiles a merged config.
///
/// # Errors
/// See [`CompileError`].
pub fn compile(merged: &MergedConfig) -> Result<CompiledConfig, CompileError> {
    let config = &merged.config;

    let mut rules = Vec::new();
    for (module, rule) in config.rules() {
        rules.push(compile_rule(rule, module.cloned())?);
    }

    let ignore =
        PathSet::compile(&config.ignore).map_err(|source| CompileError::Ignore { source })?;

    let skip_dirs = SkipDirs {
        prefixes: config.skip_dirs.prefixes.clone(),
        globs: PathSet::compile(&config.skip_dirs.globs)
            .map_err(|source| CompileError::SkipDirs { source })?,
        scope: match config.skip_dirs.scope {
            config::SkipScope::Structure => SkipScope::Structure,
            config::SkipScope::Walk => SkipScope::Walk,
        },
    };

    Ok(CompiledConfig::new(
        rules,
        ignore,
        skip_dirs,
        rules_hash(config),
    ))
}

/// Hashes the effective rule set, for the `findings` cache key.
///
/// Derived from the merged config's serialised rules rather than from the
/// files on disk, so a preset reshuffle that produces the same rules does not
/// invalidate the cache, while any real change to a rule does.
fn rules_hash(config: &Config) -> ContentHash {
    let rules: Vec<_> = config.rules().collect();
    let serialised = serde_json::to_vec(&rules).unwrap_or_default();
    ContentHash::of(&serialised)
}

fn compile_rule(
    rule: &Rule,
    module: Option<archwarden_core::ids::ModuleId>,
) -> Result<CompiledRule, CompileError> {
    let id = rule.id().clone();

    let scope = Scope::compile(rule.scope()).map_err(|source| CompileError::Scope {
        rule: id.clone(),
        source,
    })?;

    let kind = match rule {
        Rule::Structure(r) => CompiledRuleKind::Structure {
            allowed_subfolders: r.allowed_subfolders.clone(),
            warn_subfolders: r.warn_subfolders.clone(),
            recurse_into: r.recurse_into.clone(),
            filename_patterns: r
                .filename_patterns
                .iter()
                .map(|p| pattern(&id, "filename_patterns", p))
                .collect::<Result<_, _>>()?,
        },

        Rule::Naming(r) => {
            let file_pattern = pattern(&id, "file_pattern", &r.file_pattern)?;
            check_template(&id, &file_pattern, &r.must_export)?;

            CompiledRuleKind::Naming {
                kind: export_kind(&id, &r.must_export)?,
                name_template: r.must_export.name.clone(),
                signature_hint: r.must_export.signature_hint.clone(),
                file_pattern,
            }
        }

        Rule::SpecPair(r) => CompiledRuleKind::SpecPair {
            subfolders: r.subfolders.iter().cloned().collect(),
            spec_markers: spec_markers(&id, r)?,
            ignore_files: globs(&id, "ignore_files", &r.ignore_files)?,
            require_non_empty_spec: r.require_non_empty_spec,
        },

        Rule::ImportBoundary(r) => CompiledRuleKind::ImportBoundary {
            forbid: globs(&id, "forbid_import_from", &r.forbid_import_from)?,
            require: globs(&id, "must_import_from", &r.must_import_from)?,
            except: globs(&id, "except", &r.except)?,
            include_type_only: r.include_type_only,
        },

        Rule::CallObligation(r) => CompiledRuleKind::CallObligation {
            file_pattern: pattern(&id, "file_pattern", &r.file_pattern)?,
            symbol: r.must_call.symbol.clone(),
            imported_from: r.must_call.imported_from.clone(),
        },
    };

    Ok(CompiledRule {
        id,
        module,
        level: rule.level(),
        scope,
        kind,
    })
}

fn pattern(rule: &RuleId, field: &'static str, source: &str) -> Result<Pattern, CompileError> {
    Pattern::compile(source).map_err(|error| CompileError::Pattern {
        rule: rule.clone(),
        field,
        source: Box::new(error),
    })
}

fn globs<'a, I>(rule: &RuleId, field: &'static str, patterns: I) -> Result<PathSet, CompileError>
where
    I: IntoIterator<Item = &'a String>,
{
    PathSet::compile(patterns).map_err(|source| CompileError::Glob {
        rule: rule.clone(),
        field,
        source,
    })
}

/// Validates the `spec-pair` markers.
///
/// A marker is one filename component -- `spec`, `test` -- and the extension
/// is taken from the source file. A marker carrying a dot or an extension is
/// almost always someone writing the old whole-suffix form, and guessing what
/// they meant would be worse than saying so.
fn spec_markers(rule: &RuleId, spec: &SpecPairRule) -> Result<Vec<String>, CompileError> {
    let mut markers = Vec::new();

    for marker in &spec.spec_markers {
        let trimmed = marker.trim_start_matches('.');
        if trimmed.is_empty() || trimmed.contains('.') {
            return Err(CompileError::InvalidSpecMarker {
                rule: rule.clone(),
                marker: marker.clone(),
            });
        }
        markers.push(trimmed.to_owned());
    }

    Ok(markers)
}

fn export_kind(rule: &RuleId, must_export: &MustExport) -> Result<KindFilter, CompileError> {
    let mut tags = ExportTags::none();

    for name in &must_export.kind {
        if name == KIND_ANY {
            return Ok(KindFilter::Any);
        }

        let kind = ExportKind::parse(name).ok_or_else(|| CompileError::UnknownExportKind {
            rule: rule.clone(),
            name: name.clone(),
            available: ExportKind::ALL.map(ExportKind::as_str).join(", "),
        })?;

        tags = tags.with(kind);
    }

    Ok(KindFilter::OneOf(tags))
}

/// Renders the export-name template against the pattern's capture groups.
///
/// A rule whose template names a group its `file_pattern` never defines is a
/// config bug that would otherwise surface only when a file happened to match,
/// which could be months later or never.
fn check_template(
    rule: &RuleId,
    file_pattern: &Pattern,
    must_export: &MustExport,
) -> Result<(), CompileError> {
    let available = file_pattern.capture_names();
    let lookup = |group: &str| {
        available
            .contains(&group)
            // The value is irrelevant: only whether the group exists is being
            // checked here.
            .then(|| "placeholder".to_owned())
    };

    for text in [Some(&must_export.name), must_export.signature_hint.as_ref()]
        .into_iter()
        .flatten()
    {
        template::render(text, lookup).map_err(|source| CompileError::Template {
            rule: rule.clone(),
            source,
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{ids::ModuleId, level::Level, path::RepoRelPath};
    use camino::Utf8PathBuf;

    fn merged(json: &str) -> MergedConfig {
        let config: Config = serde_json::from_str(json).expect("config should parse");
        let path = Utf8PathBuf::from("arch.config.json");
        MergedConfig {
            config,
            path: path.clone(),
            root: Utf8PathBuf::from("."),
            sources: vec![path],
        }
    }

    fn compile_json(json: &str) -> Result<CompiledConfig, CompileError> {
        compile(&merged(json))
    }

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// Extracts a `Pattern` error, or `None`. See the convention note in
    /// docs/PLAN-V0.md about not using `let ... else { panic!() }` here.
    fn pattern_error(error: &CompileError) -> Option<(&RuleId, &'static str)> {
        match error {
            CompileError::Pattern { rule, field, .. } => Some((rule, field)),
            _ => None,
        }
    }

    /// The `KindFilter` of the config's single naming rule, or `None`.
    fn only_naming_kind(compiled: &CompiledConfig) -> Option<&KindFilter> {
        match &compiled.rules().next()?.kind {
            CompiledRuleKind::Naming { kind, .. } => Some(kind),
            _ => None,
        }
    }

    #[test]
    fn a_full_config_compiles_into_matchable_rules() {
        let compiled = compile_json(
            r#"{
              "version": 0,
              "modules": [{"id":"domain","rules":[
                {"type":"structure","id":"shape","level":"error",
                 "roots":"packages/domain/src/*",
                 "allowed_subfolders":["types","calcs"]}
              ]}],
              "rules": [
                {"type":"import-boundary","id":"boundary","level":"error",
                 "from":"packages/domain/**",
                 "forbid_import_from":["packages/application/**"]}
              ]
            }"#,
        )
        .expect("compiles");

        assert_eq!(compiled.rule_count(), 2);
        assert_eq!(
            compiled
                .rules_for_file(&path("packages/domain/src/user/user.ts"))
                .count(),
            2,
            "both the structure rule and the boundary cover this file"
        );
    }

    /// The module label survives lowering, so a finding can still say which
    /// module it came from.
    #[test]
    fn a_rules_module_survives_compilation() {
        let compiled = compile_json(
            r#"{"version":0,"modules":[{"id":"domain","rules":[
                {"type":"structure","id":"s","level":"warning","roots":"x/*"}]}]}"#,
        )
        .expect("compiles");

        let rule = compiled.rules().next().expect("one rule");
        assert_eq!(rule.module.as_ref().map(ModuleId::as_str), Some("domain"));
        assert_eq!(rule.level, Level::Warning);
    }

    /// An invalid scope glob is caught here rather than at walk time, and the
    /// message says which rule.
    #[test]
    fn an_invalid_scope_glob_names_its_rule() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"structure","id":"bad-scope","level":"error","roots":"packages/[domain"}]}"#,
        )
        .expect_err("should fail");

        assert!(err.to_string().contains("bad-scope"), "{err}");
    }

    /// The D3 message reaches the user through this layer, with the rule and
    /// the field attached.
    #[test]
    fn an_unsupported_regex_construct_names_the_rule_and_the_field() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"structure","id":"no-lookahead","level":"error","roots":"x/*",
                 "filename_patterns":["^(?!.*spec).*\\.ts$"]}]}"#,
        )
        .expect_err("should fail");

        let (rule, field) = pattern_error(&err).expect("is a Pattern error");
        assert_eq!(rule.as_str(), "no-lookahead");
        assert_eq!(field, "filename_patterns");
        assert!(err.to_string().contains("negative lookahead"), "{err}");
    }

    #[test]
    fn a_naming_rules_file_pattern_is_compiled() {
        let compiled = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"n","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z-]+)\\.use-case\\.ts$",
                 "must_export":{"kind":"function","name":"{{pascal(name)}}"}}]}"#,
        )
        .expect("compiles");

        assert!(compiled.needs_parse());
        assert_eq!(compiled.rule_count(), 1);
    }

    /// `kind: ["function","arrow"]` is how a preset says "callable, either
    /// form", and it has to survive into the compiled filter.
    #[test]
    fn a_list_of_export_kinds_becomes_one_filter() {
        let compiled = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"n","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "must_export":{"kind":["function","arrow"],"name":"{{pascal(name)}}"}}]}"#,
        )
        .expect("compiles");

        let kind = only_naming_kind(&compiled).expect("is a naming rule");

        assert!(kind.accepts(ExportTags::only(ExportKind::Function)));
        assert!(kind.accepts(ExportTags::only(ExportKind::Arrow)));
        assert!(!kind.accepts(ExportTags::only(ExportKind::Class)));
    }

    #[test]
    fn the_any_kind_accepts_every_declaration_form() {
        let compiled = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"n","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "must_export":{"kind":"any","name":"{{pascal(name)}}"}}]}"#,
        )
        .expect("compiles");

        assert!(matches!(
            only_naming_kind(&compiled).expect("is a naming rule"),
            KindFilter::Any
        ));

        // The helper's negative branch: a structure rule is not a naming rule.
        let structural =
            compile_json(r#"{"version":0,"rules":[{"type":"structure","id":"s","level":"error","roots":"x/*"}]}"#)
                .expect("compiles");
        assert!(only_naming_kind(&structural).is_none());
    }

    /// A kind that is not one of the ten lists the valid ones, so the fix does
    /// not need the schema open beside it.
    #[test]
    fn an_unknown_export_kind_lists_the_valid_ones() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"n","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "must_export":{"kind":"callable","name":"X"}}]}"#,
        )
        .expect_err("should fail");

        let message = err.to_string();
        assert!(message.contains("callable"), "{message}");
        assert!(message.contains("function"), "{message}");
        assert!(message.contains("arrow"), "{message}");
        assert!(message.contains("`any`"), "{message}");
    }

    /// The check worth having: a template naming a group the pattern never
    /// defines would otherwise surface only when some file happened to match,
    /// which could be months later.
    #[test]
    fn a_template_referring_to_a_missing_capture_group_is_caught_at_compile_time() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"typo","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "must_export":{"kind":"function","name":"{{pascal(nome)}}"}}]}"#,
        )
        .expect_err("should fail");

        let message = err.to_string();
        assert!(message.contains("typo"), "{message}");
        assert!(message.contains("nome"), "{message}");
    }

    /// `signature_hint` is never verified against code, but it is still a
    /// template, so a typo in it is still caught.
    #[test]
    fn a_signature_hint_template_is_checked_too() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"hint","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "must_export":{"kind":"function","name":"{{pascal(name)}}",
                                "signature_hint":"(deps: {{pascal(missing)}}Deps)"}}]}"#,
        )
        .expect_err("should fail");

        assert!(err.to_string().contains("missing"), "{err}");
    }

    #[test]
    fn an_unknown_transform_in_a_template_is_caught() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"t","level":"error","roots":"src/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "must_export":{"kind":"function","name":"{{pascalcase(name)}}"}}]}"#,
        )
        .expect_err("should fail");

        assert!(err.to_string().contains("pascalcase"), "{err}");
    }

    /// Every glob field on a boundary is compiled, and the message says which
    /// one was wrong -- a boundary has four.
    #[test]
    fn an_invalid_boundary_glob_names_the_field() {
        for (field, json) in [
            ("forbid_import_from", r#""forbid_import_from":["a/[b"]"#),
            ("must_import_from", r#""must_import_from":["a/[b"]"#),
            ("except", r#""except":["a/[b"]"#),
        ] {
            let err = compile_json(&format!(
                r#"{{"version":0,"rules":[
                    {{"type":"import-boundary","id":"b","level":"error","from":"x/**",{json}}}]}}"#
            ))
            .expect_err("should fail");

            assert!(err.to_string().contains(field), "{field}: {err}");
        }
    }

    #[test]
    fn an_invalid_spec_pair_ignore_glob_names_the_field() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"spec-pair","id":"s","level":"error","roots":"src/*",
                 "subfolders":".","ignore_files":["a/[b"]}]}"#,
        )
        .expect_err("should fail");

        assert!(err.to_string().contains("ignore_files"), "{err}");
    }

    #[test]
    fn an_invalid_ignore_or_skip_glob_is_reported_without_a_rule() {
        let err = compile_json(r#"{"version":0,"ignore":["a/[b"]}"#).expect_err("should fail");
        assert!(matches!(err, CompileError::Ignore { .. }), "{err:?}");

        let err = compile_json(r#"{"version":0,"skip_dirs":{"globs":["a/[b"]}}"#)
            .expect_err("should fail");
        assert!(matches!(err, CompileError::SkipDirs { .. }), "{err:?}");
    }

    /// `skip_dirs` reaches the compiled config intact, including the scope
    /// that decides whether the exemption is structural or total.
    #[test]
    fn skip_dirs_are_carried_through_with_their_scope() {
        let compiled =
            compile_json(r#"{"version":0,"skip_dirs":{"prefixes":["_"],"scope":"walk"}}"#)
                .expect("compiles");

        assert_eq!(compiled.skip_dirs().scope, SkipScope::Walk);
        assert!(compiled.skip_dirs().exempts(&path("src/_internal")));
    }

    /// The rules hash is what the `findings` cache key folds in, so a change
    /// to any rule must move it and an unrelated edit must not.
    #[test]
    fn the_rules_hash_tracks_the_rules_and_nothing_else() {
        let base = r#"{"version":0,"rules":[
            {"type":"structure","id":"a","level":"error","roots":"x/*"}]}"#;
        let same_rules_different_ignore = r#"{"version":0,"ignore":["**/dist/**"],"rules":[
            {"type":"structure","id":"a","level":"error","roots":"x/*"}]}"#;
        let changed_level = r#"{"version":0,"rules":[
            {"type":"structure","id":"a","level":"warning","roots":"x/*"}]}"#;

        let hash = |json: &str| compile_json(json).expect("compiles").rules_hash();

        assert_eq!(hash(base), hash(base), "deterministic");
        assert_eq!(
            hash(base),
            hash(same_rules_different_ignore),
            "ignore is not part of the rule set"
        );
        assert_ne!(
            hash(base),
            hash(changed_level),
            "a level change is a change"
        );
    }

    /// A config with no rules compiles to an empty set rather than failing.
    /// `archwarden init` writes one, and it should validate.
    #[test]
    fn an_empty_config_compiles_to_nothing() {
        let compiled = compile_json(r#"{"version":0}"#).expect("compiles");

        assert_eq!(compiled.rule_count(), 0);
        assert!(!compiled.needs_parse());
        assert_eq!(compiled.rules_for_file(&path("src/x.ts")).count(), 0);
    }

    /// `disable` is applied before lowering, so a disabled rule is never
    /// compiled and can never fire.
    #[test]
    fn a_disabled_rule_is_not_compiled() {
        let compiled = compile_json(
            r#"{"version":0,"disable":["off"],"rules":[
                {"type":"structure","id":"on","level":"error","roots":"x/*"},
                {"type":"structure","id":"off","level":"error","roots":"y/*"}]}"#,
        )
        .expect("compiles");

        let ids: Vec<_> = compiled.rules().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["on"]);
    }
}
