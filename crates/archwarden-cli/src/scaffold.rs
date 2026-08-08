//! `archwarden scaffold <path>` — the smallest shape that would satisfy the rules.
//!
//! `describe` answers rule by rule. An agent about to write a file does not
//! think rule by rule: it wants one list of exports, one list of siblings, one
//! list of import constraints. This is the same expectations, transposed.
//!
//! Not a code generator. It emits structural requirements, and the signature
//! it shows comes from `signature_hint`, which the config author writes and
//! archwarden never verifies (`RULES.md`).

use archwarden_core::{
    compiled::CompiledConfig,
    facts::{ExportKind, KindFilter},
    finding::Expectation,
    path::RepoRelPath,
};
use serde::Serialize;

/// The version of the `scaffold` JSON shape.
pub const SCAFFOLD_VERSION: u32 = 0;

/// Everything the rules require of one path, grouped by what it is about.
#[derive(Debug, Default, Serialize)]
pub struct Scaffold {
    /// Exports the file must carry.
    pub required_exports: Vec<RequiredExport>,
    /// Files that must exist beside it.
    pub required_siblings: Vec<RequiredSibling>,
    /// Import globs it may not reach, one entry per glob.
    pub forbidden_imports: Vec<ImportConstraint>,
    /// Import globs at least one of its imports must match.
    pub required_imports: Vec<ImportConstraint>,
    /// Symbols it must call.
    pub call_obligations: Vec<CallObligation>,
    /// Patterns its filename must match.
    ///
    /// Not in the shape `AGENT-INTEGRATION.md` sketched. Left out, an agent
    /// scaffolding a path whose *name* is already wrong would be told
    /// everything except the thing it has to fix first. Correction C11.
    pub filename_patterns: Vec<String>,
    /// When asked about a directory: what may live inside it.
    pub allowed_subfolders: Option<AllowedSubfolders>,
    /// When asked about a directory: what must live inside it.
    ///
    /// Always present, even when empty, unlike `allowed_subfolders`. That one
    /// distinguishes "nothing constrains this directory" from "these folders
    /// are allowed"; here an empty list already says what there is to say, and
    /// a consumer reading two lists should not have to unwrap one of them.
    #[serde(default)]
    pub required_files: RequiredFiles,
}

/// What a directory must contain.
#[derive(Debug, Default, Serialize)]
pub struct RequiredFiles {
    /// Filenames that must be there.
    pub names: Vec<String>,
    /// Regexes at least one file must match, one file per entry.
    pub patterns: Vec<String>,
}

/// One export the file must carry.
#[derive(Debug, Serialize)]
pub struct RequiredExport {
    /// The name, already rendered from the filename.
    pub name: String,
    /// The declaration forms that satisfy the rule, best first. Empty means
    /// any form will do.
    pub kinds: Vec<&'static str>,
    /// The type annotations that satisfy the rule, any one of them. Absent
    /// when the rule asks for none.
    ///
    /// Beside `signature_hint` rather than instead of it: this one is checked
    /// and that one is not, and collapsing them would make the weaker promise
    /// look like the stronger one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub annotation: Vec<String>,
    /// The signature the config author wrote. Never verified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_hint: Option<String>,
}

/// One file that must exist beside this one.
#[derive(Debug, Serialize)]
pub struct RequiredSibling {
    /// Where it goes.
    pub path: RepoRelPath,
    /// Extra conditions on its contents, as stable slugs.
    pub constraints: Vec<&'static str>,
}

/// One import glob, and the exceptions carved out of it.
#[derive(Debug, Serialize)]
pub struct ImportConstraint {
    /// The glob, matched against the resolved import path.
    pub pattern: String,
    /// Exceptions to it.
    pub except: Vec<String>,
    /// Whether `import type` counts against it.
    pub include_type_only: bool,
}

/// One symbol the file must call.
#[derive(Debug, Serialize)]
pub struct CallObligation {
    /// The callee as it appears at a call site.
    pub symbol: String,
    /// The module it must be imported from.
    pub imported_from: String,
}

/// What may live inside a directory.
#[derive(Debug, Serialize)]
pub struct AllowedSubfolders {
    /// Names that are permitted.
    pub allowed: Vec<String>,
    /// Names permitted but reported as warnings.
    pub warn: Vec<String>,
    /// Regexes a name may match instead of being listed. Absent when the rule
    /// constrains names by enumeration only.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<String>,
}

/// Transposes every expectation that applies to `path` into one shape.
///
/// Built on `describe`, not beside it: two walks of the same rules could
/// disagree, and then an agent following `scaffold` would fail `check`.
#[must_use]
pub fn scaffold(config: &CompiledConfig, path: &RepoRelPath) -> Scaffold {
    let mut shape = Scaffold::default();

    for applies in crate::describe::describe(config, path) {
        for expectation in applies.expectations {
            absorb(&mut shape, expectation);
        }
    }

    shape
}

fn absorb(shape: &mut Scaffold, expectation: Expectation) {
    match expectation {
        Expectation::RequiredExport {
            kind,
            name,
            annotation,
            signature_hint,
        } => shape.required_exports.push(RequiredExport {
            name,
            kinds: kinds_of(&kind),
            annotation,
            signature_hint,
        }),
        Expectation::RequiredSibling {
            path,
            non_empty_spec,
        } => shape.required_siblings.push(RequiredSibling {
            path,
            constraints: if non_empty_spec {
                vec!["non-empty-spec"]
            } else {
                Vec::new()
            },
        }),
        // One entry per glob rather than per rule: an agent asks "may I import
        // this?" about one path at a time, and a list it has to unpack first
        // is a list it can get wrong.
        Expectation::ForbiddenImport {
            patterns,
            except,
            include_type_only,
        } => shape
            .forbidden_imports
            .extend(patterns.into_iter().map(|pattern| ImportConstraint {
                pattern,
                except: except.clone(),
                include_type_only,
            })),
        // A forbidden package belongs in the same list as a forbidden path: for
        // someone about to write the file the two are one instruction, "do not
        // import this". The pattern slot carries the package name, which is
        // what the rule matches on and what the writer needs to see.
        Expectation::ForbiddenPackages {
            packages,
            except_from,
            include_type_only,
        } => shape
            .forbidden_imports
            .extend(packages.into_iter().map(|package| ImportConstraint {
                pattern: package,
                except: except_from.clone(),
                include_type_only,
            })),
        Expectation::RequiredImport { patterns } => {
            shape
                .required_imports
                .extend(patterns.into_iter().map(|pattern| ImportConstraint {
                    pattern,
                    except: Vec::new(),
                    include_type_only: true,
                }));
        }
        Expectation::RequiredCall {
            symbol,
            imported_from,
        } => shape.call_obligations.push(CallObligation {
            symbol,
            imported_from,
        }),
        Expectation::FilenamePattern { patterns } => shape.filename_patterns.extend(patterns),
        Expectation::RequiredFiles { names, patterns } => {
            shape.required_files.names.extend(names);
            shape.required_files.patterns.extend(patterns);
        }
        Expectation::AllowedSubfolders {
            allowed,
            warn,
            patterns,
        } => {
            shape.allowed_subfolders = Some(AllowedSubfolders {
                allowed,
                warn,
                patterns,
            });
        }
        // `Expectation` is non_exhaustive. A variant added later is not
        // something this shape can place, and guessing would be worse than the
        // omission -- `describe` still reports it in full.
        _ => {}
    }
}

/// The declaration forms a filter accepts, best first.
///
/// Empty for `Any`, which is how the renderer knows not to claim a form the
/// rule never asked for.
fn kinds_of(kind: &KindFilter) -> Vec<&'static str> {
    match kind {
        KindFilter::OneOf(tags) => tags.iter().map(ExportKind::as_str).collect(),
        // `Any`, and any filter added later: claiming a declaration form the
        // rule never asked for would have an agent write the wrong one.
        _ => Vec::new(),
    }
}

/// The JSON envelope.
#[derive(Debug, Serialize)]
struct JsonScaffold<'a> {
    version: u32,
    path: &'a RepoRelPath,
    #[serde(flatten)]
    shape: &'a Scaffold,
}

/// Writes the shape in the requested format.
pub fn render(
    path: &RepoRelPath,
    shape: &Scaffold,
    format: crate::report::Format,
    out: &mut dyn std::io::Write,
) {
    match format {
        crate::report::Format::Text => render_text(path, shape, out),
        crate::report::Format::Json => render_json(path, shape, out),
    }
}

fn render_json(path: &RepoRelPath, shape: &Scaffold, out: &mut dyn std::io::Write) {
    match serde_json::to_string_pretty(&JsonScaffold {
        version: SCAFFOLD_VERSION,
        path,
        shape,
    }) {
        Ok(json) => {
            let _ = writeln!(out, "{json}");
        }
        Err(error) => {
            let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
        }
    }
}

fn render_text(path: &RepoRelPath, shape: &Scaffold, out: &mut dyn std::io::Write) {
    if is_empty(shape) {
        let _ = writeln!(out, "No rule constrains `{path}`.");
        return;
    }

    let _ = writeln!(out, "Expected shape for `{path}`:");

    if !shape.filename_patterns.is_empty() {
        let _ = writeln!(out, "\n  Filename must match:");
        for pattern in &shape.filename_patterns {
            let _ = writeln!(out, "    {pattern}");
        }
    }

    if let Some(subfolders) = &shape.allowed_subfolders {
        let _ = writeln!(out, "\n  Subfolders allowed here:");
        for name in &subfolders.allowed {
            let _ = writeln!(out, "    {name}");
        }
        for name in &subfolders.warn {
            let _ = writeln!(out, "    {name} (allowed, reported as a warning)");
        }
        for pattern in &subfolders.patterns {
            let _ = writeln!(out, "    any name matching {pattern}");
        }
    }

    if !shape.required_files.names.is_empty() || !shape.required_files.patterns.is_empty() {
        let _ = writeln!(out, "\n  Files that must exist here:");
        for name in &shape.required_files.names {
            let _ = writeln!(out, "    {name}");
        }
        for pattern in &shape.required_files.patterns {
            let _ = writeln!(out, "    a file matching {pattern}");
        }
    }

    if !shape.required_exports.is_empty() {
        let _ = writeln!(out, "\n  Required exports:");
        for export in &shape.required_exports {
            let _ = writeln!(out, "    {}", declaration(export));
            if export.kinds.len() > 1 {
                let _ = writeln!(out, "      (or declared as {})", export.kinds.join(", "));
            }
        }
    }

    if !shape.required_siblings.is_empty() {
        let _ = writeln!(out, "\n  Required sibling files:");
        for sibling in &shape.required_siblings {
            let _ = writeln!(out, "    {}", sibling.path);
            if sibling.constraints.contains(&"non-empty-spec") {
                let _ = writeln!(
                    out,
                    "      (must contain at least one it(...) or test(...) call)"
                );
            }
        }
    }

    if !shape.forbidden_imports.is_empty() || !shape.required_imports.is_empty() {
        let _ = writeln!(out, "\n  Import constraints:");
        for constraint in &shape.forbidden_imports {
            let _ = writeln!(out, "    forbidden: {}", describe_constraint(constraint));
        }
        for constraint in &shape.required_imports {
            let _ = writeln!(out, "    required:  {}", constraint.pattern);
        }
    }

    if shape.call_obligations.is_empty() {
        let _ = writeln!(out, "\n  Required calls: none.");
    } else {
        let _ = writeln!(out, "\n  Required calls:");
        for obligation in &shape.call_obligations {
            let _ = writeln!(
                out,
                "    {} — imported from {}",
                obligation.symbol, obligation.imported_from
            );
        }
    }
}

fn is_empty(shape: &Scaffold) -> bool {
    shape.required_exports.is_empty()
        && shape.required_siblings.is_empty()
        && shape.forbidden_imports.is_empty()
        && shape.required_imports.is_empty()
        && shape.call_obligations.is_empty()
        && shape.filename_patterns.is_empty()
        && shape.allowed_subfolders.is_none()
        && shape.required_files.names.is_empty()
        && shape.required_files.patterns.is_empty()
}

/// The declaration line an agent can paste.
///
/// The keyword is the rule's first accepted form. An `annotation` outranks a
/// `signature_hint` in the same position, because it is the one of the two the
/// checker will hold the file to: a line built from it is a line that passes,
/// which is a promise a hint archwarden never verifies cannot make.
fn declaration(export: &RequiredExport) -> String {
    let keyword = export.kinds.first().copied().unwrap_or("const");
    let name = &export.name;

    if let Some(annotation) = export.annotation.first() {
        // A class names its contract in `implements`; every other form writes
        // it after a colon.
        return if keyword == "class" {
            format!("export class {name} implements {annotation} {{ ... }}")
        } else {
            format!("export {keyword} {name}: {annotation} = /* ... */;")
        };
    }

    match export.signature_hint.as_deref() {
        Some(hint) => format!("export {keyword} {name}{hint}"),
        None if keyword == "function" => format!("export function {name}(/* ... */) {{ ... }}"),
        None if keyword == "class" => format!("export class {name} {{ ... }}"),
        None => format!("export {keyword} {name} = /* ... */;"),
    }
}

fn describe_constraint(constraint: &ImportConstraint) -> String {
    use std::fmt::Write as _;

    let mut line = constraint.pattern.clone();
    if !constraint.except.is_empty() {
        let _ = write!(line, " (except {})", constraint.except.join(", "));
    }
    if !constraint.include_type_only {
        line.push_str(" (type-only imports are exempt)");
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::{CompiledRule, CompiledRuleKind, SkipDirs},
        facts::ExportTags,
        glob::PathSet,
        hash::ContentHash,
        ids::RuleId,
        level::Level,
        pattern::Pattern,
        scope::Scope,
    };

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn rule(id: &str, scope: &[&str], kind: CompiledRuleKind) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            level: Level::Error,
            scope: Scope::compile(scope.iter().copied()).expect("valid scope"),
            kind,
        }
    }

    fn config(rules: Vec<CompiledRule>) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b"scaffold"),
        )
    }

    fn set(patterns: &[&str]) -> PathSet {
        PathSet::compile(patterns.iter().map(|p| (*p).to_owned())).expect("valid globs")
    }

    fn naming(hint: Option<&str>, kind: KindFilter) -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile(r"^(?<name>[a-z0-9-]+)\.use-case\.ts$")
                .expect("valid pattern"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind,
            annotation: Vec::new(),
            signature_hint: hint.map(str::to_owned),
        }
    }

    fn spec_pair() -> CompiledRuleKind {
        CompiledRuleKind::SpecPair {
            subfolders: vec![".".to_owned()],
            spec_markers: vec!["spec".to_owned()],
            ignore_files: PathSet::default(),
            require_non_empty_spec: true,
            skip_type_only: false,
        }
    }

    fn boundary(forbid: &[&str], require: &[&str], except: &[&str]) -> CompiledRuleKind {
        CompiledRuleKind::ImportBoundary {
            forbid: set(forbid),
            require: set(require),
            forbid_packages: Vec::new(),
            except: set(except),
            except_from: PathSet::default(),
            include_type_only: true,
        }
    }

    fn function_kind() -> KindFilter {
        KindFilter::OneOf(ExportTags::only(ExportKind::Function))
    }

    const TARGET: &str = "src/user/create-client.use-case.ts";

    fn shape_of(rules: Vec<CompiledRule>) -> Scaffold {
        scaffold(&config(rules), &path(TARGET))
    }

    fn rendered(rules: Vec<CompiledRule>, format: crate::report::Format) -> String {
        let target = path(TARGET);
        let shape = scaffold(&config(rules), &target);
        let mut out = Vec::new();
        render(&target, &shape, format, &mut out);
        String::from_utf8(out).expect("output is UTF-8")
    }

    /// The transposition, which is the whole job: three rules become one list
    /// of exports, one of siblings, one of import constraints.
    #[test]
    fn expectations_from_several_rules_land_in_one_shape() {
        let shape = shape_of(vec![
            rule("name", &["src/*"], naming(None, function_kind())),
            rule("spec", &["src/*"], spec_pair()),
            rule(
                "boundary",
                &["src/**"],
                boundary(
                    &["src/infra/**"],
                    &["src/telemetry/**"],
                    &["src/infra/types/**"],
                ),
            ),
        ]);

        assert_eq!(shape.required_exports.len(), 1);
        assert_eq!(
            shape.required_exports.first().map(|e| e.name.as_str()),
            Some("CreateClient")
        );
        assert_eq!(
            shape.required_siblings.first().map(|s| s.path.as_str()),
            Some("src/user/create-client.use-case.spec.ts")
        );
        assert_eq!(
            shape
                .required_siblings
                .first()
                .map(|s| s.constraints.as_slice()),
            Some(["non-empty-spec"].as_slice())
        );
        assert_eq!(
            shape.forbidden_imports.first().map(|c| c.pattern.as_str()),
            Some("src/infra/**")
        );
        assert_eq!(
            shape.forbidden_imports.first().map(|c| c.except.as_slice()),
            Some(["src/infra/types/**".to_owned()].as_slice())
        );
        assert_eq!(
            shape.required_imports.first().map(|c| c.pattern.as_str()),
            Some("src/telemetry/**")
        );
    }

    /// A forbidden package sits in the same list as a forbidden path. For
    /// someone about to write the file the two are one instruction — "do not
    /// import this" — and splitting them would make an agent consult two lists
    /// to answer one question.
    #[test]
    fn a_forbidden_package_lands_beside_the_forbidden_paths() {
        let shape = shape_of(vec![rule(
            "three-is-quarantined",
            &["src/**"],
            CompiledRuleKind::ImportBoundary {
                forbid: set(&["src/infra/**"]),
                require: PathSet::default(),
                forbid_packages: vec!["three".to_owned()],
                except: PathSet::default(),
                except_from: set(&["src/scripts/three/**"]),
                include_type_only: true,
            },
        )]);

        let patterns: Vec<_> = shape
            .forbidden_imports
            .iter()
            .map(|c| c.pattern.as_str())
            .collect();
        assert_eq!(
            patterns,
            ["src/infra/**", "three"],
            "the package name is what the rule matches on, so it is what is shown"
        );
        assert_eq!(
            shape.forbidden_imports.last().map(|c| c.except.as_slice()),
            Some(["src/scripts/three/**".to_owned()].as_slice()),
            "and the one directory allowed travels with it"
        );
    }

    /// One entry per glob, not per rule. An agent asks "may I import this?"
    /// about one path at a time, and a list it has to unpack first is a list
    /// it can get wrong.
    #[test]
    fn a_rule_with_several_globs_becomes_several_entries() {
        let shape = shape_of(vec![rule(
            "boundary",
            &["src/**"],
            boundary(&["src/infra/**", "src/db/**"], &[], &["src/infra/types/**"]),
        )]);

        let patterns: Vec<_> = shape
            .forbidden_imports
            .iter()
            .map(|c| c.pattern.as_str())
            .collect();
        assert_eq!(patterns, ["src/infra/**", "src/db/**"]);
        assert!(
            shape.forbidden_imports.iter().all(|c| !c.except.is_empty()),
            "the exception travels with every glob it applies to"
        );
    }

    /// `scaffold` is built on `describe`, so `ignore` wins here too. An agent
    /// told to satisfy a rule that will never fire is worse off than one told
    /// nothing.
    #[test]
    fn an_ignored_path_is_unconstrained() {
        let config = CompiledConfig::new(
            vec![rule("name", &["src/*"], naming(None, function_kind()))],
            set(&["src/legacy/**"]),
            SkipDirs::default(),
            ContentHash::of(b"scaffold"),
        );

        let shape = scaffold(&config, &path("src/legacy/old.use-case.ts"));
        assert!(shape.required_exports.is_empty());
    }

    /// The line an agent can paste, with the author's hint used as the
    /// signature it is documented to be.
    #[test]
    fn the_declaration_uses_the_signature_hint() {
        let text = rendered(
            vec![rule(
                "name",
                &["src/*"],
                naming(Some("(deps: Deps): UseCase<In, Out>"), function_kind()),
            )],
            crate::report::Format::Text,
        );

        assert!(
            text.contains("export function CreateClient(deps: Deps): UseCase<In, Out>"),
            "{text}"
        );
    }

    /// Issue #42 names this as the command that makes the rule worth having:
    /// `scaffold projetos/17-nova` printing the filenames is how a unit of
    /// work gets started, which puts archwarden before the writing rather than
    /// after it.
    #[test]
    fn a_presence_rule_lists_the_files_to_create() {
        let shape = scaffold(
            &config(vec![rule(
                "licao-completa",
                &["projetos/*"],
                CompiledRuleKind::Presence {
                    require: vec!["projeto.md".to_owned(), "notas.md".to_owned()],
                    require_any: vec![Pattern::compile(r"\.ino$").expect("valid")],
                },
            )]),
            &path("projetos/17-nova"),
        );

        assert_eq!(
            shape.required_files.names,
            ["projeto.md".to_owned(), "notas.md".to_owned()]
        );
        assert_eq!(shape.required_files.patterns, [r"\.ino$".to_owned()]);
    }

    /// A naming rule that fixes both the name and the annotation, which is the
    /// shape a discovery-based registry needs (issue #39).
    fn annotated_naming(annotation: &[&str], kind: KindFilter) -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile(r"^(?<name>[a-z0-9-]+)\.use-case\.ts$")
                .expect("valid pattern"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind,
            annotation: annotation.iter().map(|a| (*a).to_owned()).collect(),
            signature_hint: None,
        }
    }

    /// The payoff of `annotation` being *checked* rather than suggested: the
    /// line `scaffold` hands over compiles into a file that passes. A
    /// `signature_hint` could never promise that.
    #[test]
    fn the_declaration_carries_the_required_annotation() {
        let text = rendered(
            vec![rule(
                "contract",
                &["src/*"],
                annotated_naming(
                    &["UseCaseModule"],
                    KindFilter::OneOf(ExportTags::only(ExportKind::Const)),
                ),
            )],
            crate::report::Format::Text,
        );

        assert!(
            text.contains("export const CreateClient: UseCaseModule = /* ... */;"),
            "{text}"
        );
    }

    /// A class writes the same contract in `implements`, and the skeleton has
    /// to be the shape that satisfies the rule, not a `const` with a class
    /// keyword in front of it.
    #[test]
    fn an_annotated_class_is_rendered_with_its_implements_clause() {
        let text = rendered(
            vec![rule(
                "contract",
                &["src/*"],
                annotated_naming(
                    &["UseCaseModule"],
                    KindFilter::OneOf(ExportTags::only(ExportKind::Class)),
                ),
            )],
            crate::report::Format::Text,
        );

        assert!(
            text.contains("export class CreateClient implements UseCaseModule { ... }"),
            "{text}"
        );
    }

    /// An agent reads the JSON, so the requirement has to be in it -- and
    /// beside `signature_hint`, not instead of it, because the two make
    /// different promises.
    #[test]
    fn the_required_annotation_reaches_the_json() {
        let text = rendered(
            vec![rule(
                "contract",
                &["src/*"],
                annotated_naming(
                    &["UseCaseModule", "LegacyUseCase"],
                    KindFilter::OneOf(ExportTags::only(ExportKind::Const)),
                ),
            )],
            crate::report::Format::Json,
        );
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");

        assert_eq!(
            parsed["required_exports"][0]["annotation"],
            serde_json::json!(["UseCaseModule", "LegacyUseCase"])
        );
    }

    /// A rule that asks for no annotation must not grow an empty field in the
    /// output an agent parses.
    #[test]
    fn a_rule_without_an_annotation_says_nothing_about_one() {
        let text = rendered(
            vec![rule("name", &["src/*"], naming(None, function_kind()))],
            crate::report::Format::Json,
        );
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");

        assert!(parsed["required_exports"][0].get("annotation").is_none());
    }

    /// Without a hint there is still something to write, and it says which
    /// form the rule wants.
    #[test]
    fn a_declaration_without_a_hint_is_still_usable() {
        let text = rendered(
            vec![rule("name", &["src/*"], naming(None, function_kind()))],
            crate::report::Format::Text,
        );

        assert!(
            text.contains("export function CreateClient(/* ... */) { ... }"),
            "{text}"
        );
    }

    /// A rule that accepts several forms shows one and names the rest, rather
    /// than picking silently.
    #[test]
    fn several_accepted_forms_are_all_named() {
        let text = rendered(
            vec![rule(
                "name",
                &["src/*"],
                naming(
                    None,
                    KindFilter::OneOf(
                        ExportTags::only(ExportKind::Function).with(ExportKind::Arrow),
                    ),
                ),
            )],
            crate::report::Format::Text,
        );

        assert!(text.contains("export function CreateClient"), "{text}");
        assert!(text.contains("or declared as"), "{text}");
        assert!(text.contains("arrow"), "{text}");
    }

    /// `kind: "any"` asks for no particular form, so the scaffold must not
    /// claim one. It falls back to `const` for something writable and says
    /// nothing about alternatives.
    #[test]
    fn any_form_claims_none() {
        let shape = shape_of(vec![rule(
            "name",
            &["src/*"],
            naming(None, KindFilter::Any),
        )]);

        assert!(
            shape
                .required_exports
                .first()
                .is_some_and(|e| e.kinds.is_empty()),
            "no form is claimed"
        );

        let text = rendered(
            vec![rule("name", &["src/*"], naming(None, KindFilter::Any))],
            crate::report::Format::Text,
        );
        assert!(text.contains("export const CreateClient"), "{text}");
        assert!(!text.contains("or declared as"), "{text}");
    }

    /// A path nothing constrains says so, rather than printing an empty form.
    #[test]
    fn an_unconstrained_path_says_so() {
        let target = path("docs/README.md");
        let shape = scaffold(
            &config(vec![rule(
                "name",
                &["src/*"],
                naming(None, function_kind()),
            )]),
            &target,
        );
        let mut out = Vec::new();
        render(&target, &shape, crate::report::Format::Text, &mut out);

        assert_eq!(
            String::from_utf8(out).expect("UTF-8"),
            "No rule constrains `docs/README.md`.\n"
        );
    }

    /// "No required calls" is stated rather than left out: an agent reading a
    /// shape needs to know the list is empty, not absent.
    #[test]
    fn having_no_call_obligations_is_stated() {
        let text = rendered(
            vec![rule("name", &["src/*"], naming(None, function_kind()))],
            crate::report::Format::Text,
        );

        assert!(text.contains("Required calls: none."), "{text}");
    }

    /// The whole text format, written out by hand so the assertion is about
    /// what it should be rather than what it happens to be.
    #[test]
    fn the_text_format_reads_as_intended() {
        let text = rendered(
            vec![
                rule(
                    "name",
                    &["src/*"],
                    naming(Some("(deps: Deps): UseCase"), function_kind()),
                ),
                rule("spec", &["src/*"], spec_pair()),
                rule(
                    "boundary",
                    &["src/**"],
                    boundary(&["src/infra/**"], &[], &["src/infra/types/**"]),
                ),
            ],
            crate::report::Format::Text,
        );

        assert_eq!(
            text,
            "Expected shape for `src/user/create-client.use-case.ts`:\n\
             \n\
             \x20 Required exports:\n\
             \x20   export function CreateClient(deps: Deps): UseCase\n\
             \n\
             \x20 Required sibling files:\n\
             \x20   src/user/create-client.use-case.spec.ts\n\
             \x20     (must contain at least one it(...) or test(...) call)\n\
             \n\
             \x20 Import constraints:\n\
             \x20   forbidden: src/infra/** (except src/infra/types/**)\n\
             \n\
             \x20 Required calls: none.\n"
        );
    }

    /// The JSON is the contract an agent should consume.
    #[test]
    fn the_json_shape_is_versioned_and_flat() {
        let json = rendered(
            vec![
                rule(
                    "name",
                    &["src/*"],
                    naming(Some("(deps: Deps): UseCase"), function_kind()),
                ),
                rule("spec", &["src/*"], spec_pair()),
                rule(
                    "boundary",
                    &["src/**"],
                    boundary(&["src/infra/**"], &["src/telemetry/**"], &[]),
                ),
            ],
            crate::report::Format::Json,
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(parsed["version"], 0);
        assert_eq!(parsed["path"], TARGET);
        assert_eq!(parsed["required_exports"][0]["name"], "CreateClient");
        assert_eq!(parsed["required_exports"][0]["kinds"][0], "function");
        assert_eq!(
            parsed["required_exports"][0]["signature_hint"],
            "(deps: Deps): UseCase"
        );
        assert_eq!(
            parsed["required_siblings"][0]["constraints"][0],
            "non-empty-spec"
        );
        assert_eq!(parsed["forbidden_imports"][0]["pattern"], "src/infra/**");
        assert_eq!(parsed["required_imports"][0]["pattern"], "src/telemetry/**");
        assert!(
            parsed["call_obligations"]
                .as_array()
                .is_some_and(Vec::is_empty),
            "empty rather than absent: an agent needs to know the list is empty"
        );
    }

    /// A `call-obligation` reaches the shape, and says where the symbol has to
    /// come from -- half the requirement is the import.
    #[test]
    fn a_call_obligation_names_its_module() {
        let shape = shape_of(vec![rule(
            "audit",
            &["src/*"],
            CompiledRuleKind::CallObligation {
                file_pattern: Pattern::compile(r"^create-client\.use-case\.ts$")
                    .expect("valid pattern"),
                symbol: "Event.save".to_owned(),
                imported_from: "@org/domain/event".to_owned(),
            },
        )]);

        assert_eq!(
            shape.call_obligations.first().map(|c| c.symbol.as_str()),
            Some("Event.save")
        );
        assert_eq!(
            shape
                .call_obligations
                .first()
                .map(|c| c.imported_from.as_str()),
            Some("@org/domain/event")
        );
    }

    /// Correction C11: a filename constraint reaches the shape. Without it, an
    /// agent scaffolding a path whose name is already wrong would be told
    /// everything except the thing it has to fix first.
    #[test]
    fn a_filename_constraint_reaches_the_shape() {
        let target = path("src/user/helpers.ts");
        let shape = scaffold(
            &config(vec![rule(
                "shape",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(Vec::new()),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: vec![
                        Pattern::compile(r"^[a-z-]+\.use-case\.ts$").expect("valid"),
                    ],
                },
            )]),
            &target,
        );

        assert_eq!(shape.filename_patterns, [r"^[a-z-]+\.use-case\.ts$"]);

        let mut out = Vec::new();
        render(&target, &shape, crate::report::Format::Text, &mut out);
        let text = String::from_utf8(out).expect("UTF-8");
        assert!(text.contains("Filename must match:"), "{text}");
    }

    /// Asked about a directory, the shape says what may live in it. `describe`
    /// already answers for directories, and a scaffold that dropped the answer
    /// would be less useful than the command it is built on.
    #[test]
    fn a_directory_gets_its_allowed_subfolders() {
        let target = path("src/user");
        let shape = scaffold(
            &config(vec![rule(
                "shape",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec!["types".to_owned(), "calcs".to_owned()]),
                    warn_subfolders: vec!["shared".to_owned()],
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )]),
            &target,
        );

        let subfolders = shape.allowed_subfolders.as_ref().expect("a directory rule");
        assert_eq!(subfolders.allowed, ["types", "calcs"]);
        assert_eq!(subfolders.warn, ["shared"]);

        let mut out = Vec::new();
        render(&target, &shape, crate::report::Format::Text, &mut out);
        let text = String::from_utf8(out).expect("UTF-8");
        assert!(text.contains("Subfolders allowed here:"), "{text}");
        assert!(
            text.contains("shared (allowed, reported as a warning)"),
            "{text}"
        );
    }

    /// A boundary rule that exempts type-only imports says so, because "you
    /// may not import this" and "you may import its types" are different
    /// instructions.
    #[test]
    fn a_type_only_exemption_is_stated() {
        let target = path(TARGET);
        let shape = scaffold(
            &config(vec![rule(
                "boundary",
                &["src/**"],
                CompiledRuleKind::ImportBoundary {
                    forbid: set(&["src/infra/**"]),
                    require: PathSet::default(),
                    forbid_packages: Vec::new(),
                    except: PathSet::default(),
                    except_from: PathSet::default(),
                    include_type_only: false,
                },
            )]),
            &target,
        );

        let mut out = Vec::new();
        render(&target, &shape, crate::report::Format::Text, &mut out);
        let text = String::from_utf8(out).expect("UTF-8");
        assert!(text.contains("type-only imports are exempt"), "{text}");
    }
}
