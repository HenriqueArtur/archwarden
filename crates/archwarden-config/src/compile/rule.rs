//! One rule of the wire format, lowered into one compiled rule.

use archwarden_core::compiled::{CompiledRule, CompiledRuleKind};

use crate::rule::Rule;

use super::{
    boundary::{forbidden_paths, importer_groups, permitted_paths, reaching_paths},
    error::CompileError,
    fields::{
        annotation, check_document_template, check_template, companion, export_kind, globs,
        import_filter, pattern, reachable_key, require_name, spec_dirs, spec_markers,
    },
    scope::{Modules, compile_scope},
};

#[allow(
    clippy::too_many_lines,
    reason = "one arm per rule kind, each a literal. Splitting it would put the \
              arms somewhere the exhaustive match no longer names them, which \
              is what makes a kind added without lowering fail to build"
)]
pub(super) fn compile_rule(
    rule: &Rule,
    module: Option<archwarden_core::ids::ModuleId>,
    module_why: Option<String>,
    modules: &Modules,
    inside: Option<&archwarden_core::ids::ModuleId>,
) -> Result<CompiledRule, CompileError> {
    let id = rule.id().clone();

    let scope = compile_scope(rule, &id, modules, inside)?;

    let kind = match rule {
        Rule::Structure(r) => CompiledRuleKind::Structure {
            allowed_subfolders: r.allowed_subfolders.clone(),
            warn_subfolders: r.warn_subfolders.clone(),
            recurse_into: r.recurse_into.clone(),
            subfolder_patterns: r
                .subfolder_patterns
                .iter()
                .map(|p| pattern(&id, "subfolder_patterns", p))
                .collect::<Result<_, _>>()?,
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
            spec_dirs: spec_dirs(&id, r)?,
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

        Rule::ImportCycle(r) => CompiledRuleKind::ImportCycle {
            include_type_only: r.include_type_only,
        },

        Rule::ImportBoundary(r) => CompiledRuleKind::ImportBoundary {
            // A named module becomes its paths here, so nothing downstream
            // knows the difference: the engine sees the `PathSet` it always
            // saw, and the config says `infrastructure` instead of repeating
            // that module's globs. Issue #74.
            forbid: forbidden_paths(&id, r, modules)?,
            allow: permitted_paths(&id, r, modules)?,
            groups: importer_groups(&id, r, modules)?,
            allow_packages: (!r.only_import_from_packages.is_empty())
                .then(|| r.only_import_from_packages.iter().cloned().collect()),
            require: globs(&id, "must_import_from", &r.must_import_from)?,
            forbid_packages: r.forbid_import_from_packages.iter().cloned().collect(),
            forbid_reaching: reaching_paths(&id, r, modules)?,
            except: globs(&id, "except", &r.except)?,
            except_from: globs(&id, "except_from", &r.except_from)?,
            include_type_only: r.include_type_only,
        },

        Rule::Presence(r) => CompiledRuleKind::Presence {
            require: r
                .require
                .iter()
                .map(|name| require_name(&id, name))
                .collect::<Result<_, _>>()?,
            require_any: r
                .require_any
                .iter()
                .map(|p| pattern(&id, "require_any", p))
                .collect::<Result<_, _>>()?,
        },

        Rule::Pair(r) => CompiledRuleKind::Pair {
            file_pattern: pattern(&id, "file_pattern", &r.file_pattern)?,
            must_exist: companion(&id, &r.must_exist)?,
        },

        Rule::Frontmatter(r) => CompiledRuleKind::Frontmatter {
            file_pattern: pattern(&id, "file_pattern", &r.file_pattern)?,
            require: r.require.iter().cloned().collect(),
            one_of: r
                .one_of
                .iter()
                .map(|(key, values)| (key.clone(), values.iter().cloned().collect()))
                .collect(),
            equals: r
                .equals
                .iter()
                .map(|(key, template)| {
                    check_document_template(&id, template)?;
                    Ok((key.clone(), template.clone()))
                })
                .collect::<Result<_, CompileError>>()?,
        },

        Rule::Metadata(r) => CompiledRuleKind::Metadata {
            require: r
                .require
                .iter()
                .map(|key| reachable_key(&id, key))
                .collect::<Result<_, CompileError>>()?,
            one_of: r
                .one_of
                .iter()
                .map(|(key, values)| {
                    Ok((
                        reachable_key(&id, key)?,
                        values.iter().cloned().collect::<Vec<_>>(),
                    ))
                })
                .collect::<Result<_, CompileError>>()?,
            equals: r
                .equals
                .iter()
                .map(|(key, template)| {
                    check_document_template(&id, template)?;
                    Ok((reachable_key(&id, key)?, template.clone()))
                })
                .collect::<Result<_, CompileError>>()?,
            deadline: r
                .deadline
                .iter()
                .map(|key| reachable_key(&id, key))
                .collect::<Result<_, CompileError>>()?,
        },

        Rule::Frozen(_) => CompiledRuleKind::Frozen,

        Rule::Mirror(r) => CompiledRuleKind::Mirror {
            file_pattern: pattern(&id, "file_pattern", &r.file_pattern)?,
            must_exist: r.must_exist.clone(),
        },

        Rule::ExportShape(r) => {
            CompiledRuleKind::ExportShape(archwarden_core::compiled::ExportShape {
                forbid_default: r.forbid_default,
                max_exports: r.max_exports,
                must_return: r
                    .must_return
                    .iter()
                    .map(|p| pattern(&id, "must_return", p))
                    .collect::<Result<_, _>>()?,
            })
        }

        Rule::CallObligation(r) => CompiledRuleKind::CallObligation {
            file_pattern: pattern(&id, "file_pattern", &r.file_pattern)?,
            symbol: r.must_call.symbol.clone(),
            imported_from: r.must_call.imported_from.clone(),
        },
    };

    // The second axis, and only when the rule asks for it. `None` rather than an
    // empty filter: "does not narrow" and "narrows to nothing" are different
    // statements, and only one of them should cost a resolution pass.
    let imports = import_filter(&id, rule)?;

    Ok(CompiledRule {
        id,
        module,
        imports,
        why: rule.why().map(ToOwned::to_owned),
        module_why,
        decision: rule.decision().cloned(),
        level: rule.level(),
        scope,
        kind,
    })
}
