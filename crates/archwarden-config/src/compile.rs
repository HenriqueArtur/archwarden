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

    /// `must_export` asks for an annotation on a form that cannot carry one.
    ///
    /// A rule with no satisfying input, which is worse than a wrong rule: it
    /// looks exactly like a repository nobody has migrated yet, and every file
    /// under it is reported forever.
    #[error(
        "rule `{rule}`: `annotation` cannot be satisfied by an export declared \
         as {kinds}. Only a binding (`const`, `let`, `var`) or a `class`, \
         through its `implements` clause, writes a type down beside its name; \
         a function declares a *return* type, which is a different claim."
    )]
    UnannotatableKind {
        /// The rule.
        rule: RuleId,
        /// The forms the rule accepts, none of which can be annotated.
        kinds: String,
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

    /// A `must_export.name` template refers to a capture group that neither
    /// pattern on the rule defines.
    #[error("rule `{rule}`: {source}")]
    Template {
        /// The rule.
        rule: RuleId,
        /// What went wrong.
        #[source]
        source: template::TemplateError,
    },

    /// `file_pattern` and `dir_pattern` both define the same capture group.
    #[error(
        "rule `{rule}`: capture group `{group}` is defined by both \
         `file_pattern` and `dir_pattern`, so `{{{{...({group})}}}}` in the \
         template has two values and no rule for choosing between them. \
         Rename one of them."
    )]
    DuplicateCaptureGroup {
        /// The rule.
        rule: RuleId,
        /// The group both patterns define.
        group: String,
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
            let dir_pattern = r
                .dir_pattern
                .as_deref()
                .map(|source| pattern(&id, "dir_pattern", source))
                .transpose()?;
            check_template(&id, &file_pattern, dir_pattern.as_ref(), &r.must_export)?;

            let kind = export_kind(&id, &r.must_export)?;
            let annotation = annotation(&id, &kind, &r.must_export)?;

            CompiledRuleKind::Naming {
                kind,
                name_template: r.must_export.name.clone(),
                annotation,
                signature_hint: r.must_export.signature_hint.clone(),
                file_pattern,
                dir_pattern,
            }
        }

        Rule::SpecPair(r) => CompiledRuleKind::SpecPair {
            subfolders: r.subfolders.iter().cloned().collect(),
            spec_markers: spec_markers(&id, r)?,
            ignore_files: globs(&id, "ignore_files", &r.ignore_files)?,
            require_non_empty_spec: r.require_non_empty_spec,
            skip_type_only: r.skip_type_only,
        },

        Rule::NoPassthrough(r) => CompiledRuleKind::NoPassthrough {
            forms: archwarden_core::compiled::PassthroughForms {
                reexport: r.forms.contains(&crate::rule::PassthroughForm::Reexport),
                alias: r.forms.contains(&crate::rule::PassthroughForm::Alias),
                wrapper: r.forms.contains(&crate::rule::PassthroughForm::Wrapper),
            },
            except: globs(&id, "except", &r.except)?,
            allow_package_entrypoints: r.allow_package_entrypoints,
            allow_partial: r.allow_partial,
        },

        Rule::ImportBoundary(r) => CompiledRuleKind::ImportBoundary {
            forbid: globs(&id, "forbid_import_from", &r.forbid_import_from)?,
            require: globs(&id, "must_import_from", &r.must_import_from)?,
            forbid_packages: r.forbid_import_from_packages.iter().cloned().collect(),
            except: globs(&id, "except", &r.except)?,
            except_from: globs(&id, "except_from", &r.except_from)?,
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

/// The forms that have somewhere to write a type down beside the name.
///
/// A binding takes an annotation after the colon; a class names its contracts
/// in `implements`. A function has a *return* type, an interface and a type
/// alias *are* the type, an enum declares one, and a re-export's declaration is
/// in another file — none of those is a place this rule could read.
const ANNOTATABLE: [ExportKind; 5] = [
    ExportKind::Const,
    ExportKind::Let,
    ExportKind::Var,
    ExportKind::Arrow,
    ExportKind::Class,
];

/// The required annotations, refusing a rule no file could satisfy.
///
/// `kind: "any"` passes: it accepts the annotatable forms among everything
/// else, so a file that satisfies the rule exists.
fn annotation(
    rule: &RuleId,
    kind: &KindFilter,
    must_export: &MustExport,
) -> Result<Vec<String>, CompileError> {
    let Some(annotation) = must_export.annotation.as_ref() else {
        return Ok(Vec::new());
    };

    if !ANNOTATABLE
        .iter()
        .any(|form| kind.accepts(ExportTags::only(*form)))
    {
        return Err(CompileError::UnannotatableKind {
            rule: rule.clone(),
            kinds: must_export
                .kind
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", "),
        });
    }

    Ok(annotation.iter().cloned().collect())
}

/// Renders the export-name template against both patterns' capture groups.
///
/// A rule whose template names a group no pattern defines is a config bug that
/// would otherwise surface only when a file happened to match, which could be
/// months later or never.
fn check_template(
    rule: &RuleId,
    file_pattern: &Pattern,
    dir_pattern: Option<&Pattern>,
    must_export: &MustExport,
) -> Result<(), CompileError> {
    let from_file = file_pattern.capture_names();
    let from_dir = dir_pattern.map(Pattern::capture_names).unwrap_or_default();

    // Refused rather than resolved by precedence. The two patterns share one
    // template namespace so that `{{pascal(entity)}}{{pascal(action)}}` reads
    // as one name rather than as two sources spliced together -- and the price
    // of that is that a group defined twice has no answer. Picking the
    // filename's silently would make the rule demand the wrong export on every
    // file in the scope, which is the state where a `naming` rule gets deleted
    // rather than fixed.
    if let Some(group) = from_file.iter().find(|group| from_dir.contains(group)) {
        return Err(CompileError::DuplicateCaptureGroup {
            rule: rule.clone(),
            group: (*group).to_owned(),
        });
    }

    let lookup = |group: &str| {
        (from_file.contains(&group) || from_dir.contains(&group))
            // The value is irrelevant: only whether the group exists is being
            // checked here.
            .then(|| "placeholder".to_owned())
    };

    let annotations = must_export
        .annotation
        .iter()
        .flat_map(|patterns| patterns.iter());

    for text in [Some(&must_export.name), must_export.signature_hint.as_ref()]
        .into_iter()
        .flatten()
        .chain(annotations)
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

    /// The compiled annotations of the config's single naming rule.
    fn only_naming_annotation(compiled: &CompiledConfig) -> Option<&[String]> {
        match &compiled.rules().next()?.kind {
            CompiledRuleKind::Naming { annotation, .. } => Some(annotation),
            _ => None,
        }
    }

    fn tool_rule(must_export: &str) -> String {
        format!(
            r#"{{"version":0,"rules":[{{"type":"naming","id":"tools","level":"error",
               "roots":"src/tools","file_pattern":"^(?<tool>[a-z-]+)\\.tool\\.ts$",
               "must_export":{must_export}}}]}}"#
        )
    }

    #[test]
    fn an_annotation_reaches_the_compiled_rule() {
        let compiled = compile_json(&tool_rule(
            r#"{"kind":["const"],"name":"AGENT_TOOL","annotation":"{{pascal(tool)}}Module"}"#,
        ))
        .expect("compiles");

        assert_eq!(
            only_naming_annotation(&compiled).expect("is a naming rule"),
            ["{{pascal(tool)}}Module"]
        );
    }

    #[test]
    fn a_rule_without_an_annotation_compiles_to_an_empty_list() {
        let compiled = compile_json(&tool_rule(r#"{"kind":["const"],"name":"AGENT_TOOL"}"#))
            .expect("compiles");

        assert!(
            only_naming_annotation(&compiled)
                .expect("is a naming rule")
                .is_empty()
        );
    }

    /// The annotation is a template over the same groups the name is, so a
    /// group no pattern defines is the same config bug there -- and one that
    /// would otherwise surface only when a file happened to match.
    #[test]
    fn an_annotation_naming_an_unknown_group_is_refused() {
        let error = compile_json(&tool_rule(
            r#"{"kind":["const"],"name":"AGENT_TOOL","annotation":"{{pascal(entity)}}Module"}"#,
        ))
        .expect_err("refused");

        assert!(matches!(error, CompileError::Template { .. }), "{error:?}");
    }

    /// A function declares a return type, not an annotation, so a rule asking
    /// for both can never be satisfied by any file. Refused at compile time
    /// rather than left to `doctor`: `doctor` exits 0 and gives advice, and
    /// this is not advice -- it is a rule with no satisfying input, which looks
    /// exactly like a repository nobody has migrated yet.
    #[test]
    fn an_annotation_on_a_form_that_cannot_carry_one_is_refused() {
        let error = compile_json(&tool_rule(
            r#"{"kind":["function"],"name":"AGENT_TOOL","annotation":"AgentToolModule"}"#,
        ))
        .expect_err("refused");

        assert!(
            matches!(error, CompileError::UnannotatableKind { .. }),
            "{error:?}"
        );
    }

    /// The forms that do have an annotation position, each accepted. `arrow`
    /// is one: it only ever occurs with `const`, which is annotatable.
    #[test]
    fn every_annotatable_form_is_accepted_beside_an_annotation() {
        for kind in ["const", "let", "var", "arrow", "class", "any"] {
            let json = tool_rule(&format!(
                r#"{{"kind":["{kind}"],"name":"AGENT_TOOL","annotation":"AgentToolModule"}}"#
            ));
            assert!(compile_json(&json).is_ok(), "{kind}");
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

    /// The rule from issue #14, through the wire format: a quarantined
    /// dependency and the one directory allowed to reach it.
    #[test]
    fn a_boundary_may_name_a_package_and_exempt_an_importer() {
        let compiled = compile_json(
            r#"{"version":0,"rules":[
                {"type":"import-boundary","id":"three-is-quarantined","level":"error",
                 "from":"src/**",
                 "forbid_import_from_packages":["three"],
                 "except_from":["src/scripts/three/**"]}]}"#,
        )
        .expect("compiles");

        let CompiledRuleKind::ImportBoundary {
            forbid_packages,
            except_from,
            forbid,
            ..
        } = &compiled.rules().next().expect("one rule").kind
        else {
            panic!("is an import-boundary rule");
        };
        assert_eq!(forbid_packages, &["three".to_owned()]);
        assert_eq!(except_from.patterns(), ["src/scripts/three/**".to_owned()]);
        assert!(
            forbid.is_empty(),
            "a package rule puts nothing in the path field"
        );
    }

    /// The rule from issue #16: the entity names the directory, the action
    /// names the file, and both reach the template.
    #[test]
    fn a_template_may_take_a_group_from_the_directory_pattern() {
        let compiled = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"repo-name","level":"error",
                 "roots":"src/Entities/*",
                 "file_pattern":"^(?<action>[a-z0-9-]+)\\.ts$",
                 "dir_pattern":"^(?<entity>[A-Za-z0-9]+)$",
                 "must_export":{"kind":"function",
                                "name":"{{pascal(entity)}}{{pascal(action)}}Repository"}}]}"#,
        )
        .expect("compiles");

        let CompiledRuleKind::Naming { dir_pattern, .. } =
            &compiled.rules().next().expect("one rule").kind
        else {
            panic!("is a naming rule");
        };
        assert_eq!(
            dir_pattern.as_ref().map(Pattern::as_str),
            Some("^(?<entity>[A-Za-z0-9]+)$")
        );
    }

    /// One namespace means one value per group, so a group both patterns
    /// define has no answer. Refused rather than resolved by precedence:
    /// silently preferring the filename would make the rule demand the wrong
    /// export on every file in the scope.
    #[test]
    fn a_capture_group_defined_by_both_patterns_is_refused() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"ambiguous","level":"error",
                 "roots":"src/Entities/*",
                 "file_pattern":"^(?<name>[a-z]+)\\.ts$",
                 "dir_pattern":"^(?<name>[A-Za-z]+)$",
                 "must_export":{"kind":"function","name":"{{pascal(name)}}"}}]}"#,
        )
        .expect_err("should fail");

        let message = err.to_string();
        assert!(message.contains("ambiguous"), "names the rule: {message}");
        assert!(message.contains("`name`"), "names the group: {message}");
        assert!(
            message.contains("dir_pattern") && message.contains("file_pattern"),
            "names both fields, so the reader knows which to rename: {message}"
        );
    }

    /// And a broken `dir_pattern` is reported against its own field rather
    /// than against `file_pattern`, which is the whole point of passing the
    /// field name down.
    #[test]
    fn an_invalid_directory_pattern_names_its_field() {
        let err = compile_json(
            r#"{"version":0,"rules":[
                {"type":"naming","id":"broken","level":"error",
                 "roots":"src/Entities/*",
                 "file_pattern":"^(?<action>[a-z]+)\\.ts$",
                 "dir_pattern":"^[unclosed",
                 "must_export":{"kind":"function","name":"{{pascal(action)}}"}}]}"#,
        )
        .expect_err("should fail");

        assert!(err.to_string().contains("dir_pattern"), "{err}");
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
