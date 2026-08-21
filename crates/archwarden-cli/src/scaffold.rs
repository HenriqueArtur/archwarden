//! `archwarden scaffold <path>` — writing out the shape.
//!
//! The operation moved to [`archwarden_api::scaffold`] in 0.18, and what is
//! left here is this surface's half: the terminal prose, and the call that
//! turns the shared JSON envelope into bytes. Same split as
//! [`crate::describe`], for the same reason.

use archwarden_api::scaffold::{ImportConstraint, RequiredExport, Scaffold};
use archwarden_core::path::RepoRelPath;

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
    match serde_json::to_string_pretty(&archwarden_api::scaffold::envelope(path, shape)) {
        Ok(json) => {
            let _ = writeln!(out, "{json}");
        }
        Err(error) => {
            let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
        }
    }
}

/// Why this path's own last component is not one the rules permit, if it is
/// not.
///
/// Only the folder half is decided here. A filename is judged by regexes the
/// same way, but `scaffold` is asked about files that are about to be written
/// under a name the writer chose from the patterns it prints — and the folder
/// case is the one that arrived as a bug report, with `scaffold` describing a
/// shape to build at a path `check` rejects.
fn name_is_not_permitted(path: &RepoRelPath, shape: &Scaffold) -> Option<String> {
    let name = shape.folder_name.as_ref()?;
    let own = path.as_str().rsplit('/').next()?;

    if name.allowed.iter().any(|allowed| allowed == own) || name.warn.iter().any(|warn| warn == own)
    {
        return None;
    }
    // Recompiled with the engine that compiled it in the first place, so this
    // cannot disagree with `check` about whether a name matches. A second regex
    // implementation answering the same question differently would be a worse
    // bug than the one being fixed.
    if name.patterns.iter().any(|pattern| {
        archwarden_core::pattern::Pattern::compile(pattern)
            .is_ok_and(|compiled| compiled.is_match(own))
    }) {
        return None;
    }

    // Nothing is permitted at all, which is a rule saying this directory should
    // not be here rather than one saying it is misnamed.
    if name.allowed.is_empty() && name.warn.is_empty() && name.patterns.is_empty() {
        return Some(format!("`{own}` — its parent allows no subfolder at all"));
    }

    Some(format!("`{own}` is not one of the names allowed here"))
}

fn render_text(path: &RepoRelPath, shape: &Scaffold, out: &mut dyn std::io::Write) {
    if is_empty(shape) {
        let _ = writeln!(out, "No rule constrains `{path}`.");
        return;
    }

    // Before the shape, and deliberately: if the path's own name is not one the
    // rules permit, no shape built there can pass, and a reader who starts
    // building is following an answer to the wrong question. `scaffold` says
    // what to go and make, so leading with an unbuildable location is the one
    // thing it must not do.
    if let Some(refusal) = name_is_not_permitted(path, shape) {
        let _ = writeln!(out, "`{path}` is not a path these rules allow.\n");
        let _ = writeln!(out, "  {refusal}\n");
        let _ = writeln!(
            out,
            "Nothing built here can pass. Rename it first; the shape below is \
             what would be expected at a permitted name."
        );
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "Expected shape for `{path}`:");

    if let Some(name) = &shape.folder_name {
        let _ = writeln!(out, "\n  This folder's name must be:");
        for allowed in &name.allowed {
            let _ = writeln!(out, "    {allowed}");
        }
        for warn in &name.warn {
            let _ = writeln!(out, "    {warn} (allowed, reported as a warning)");
        }
        for pattern in &name.patterns {
            let _ = writeln!(out, "    any name matching {pattern}");
        }
        if name.allowed.is_empty() && name.warn.is_empty() && name.patterns.is_empty() {
            let _ = writeln!(out, "    (its parent allows no subfolder at all)");
        }
    }

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

    render_calls(shape, out);
}

/// The calls a file has to contain, and what they have to be given.
fn render_calls(shape: &Scaffold, out: &mut dyn std::io::Write) {
    if shape.call_obligations.is_empty() {
        let _ = writeln!(out, "\n  Required calls: none.");
        return;
    }

    let _ = writeln!(out, "\n  Required calls:");
    for obligation in &shape.call_obligations {
        let _ = writeln!(
            out,
            "    {} — imported from {}",
            obligation.symbol, obligation.imported_from
        );
        // Indented under the call rather than appended to it: this is what
        // somebody about to write the call has to type, and a line they can
        // copy beats a clause they have to parse. Issue #164.
        if !obligation.with_options.is_empty() {
            let _ = writeln!(
                out,
                "      passing {{ {} }}",
                obligation.with_options.join(", ")
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
        // A rule constraining only the folder's own name is the whole of #53,
        // and leaving it out here reproduced the reported bug in miniature:
        // `scaffold` answered "No rule constrains" about a path that rule
        // refuses. It was visible in a manual run and read past, because the
        // fixture beside it had a second rule that filled the shape.
        && shape.folder_name.is_none()
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
    use archwarden_api::scaffold::{AllowedSubfolders, scaffold};
    use archwarden_core::{
        compiled::{CompiledConfig, CompiledRule, CompiledRuleKind, SkipDirs},
        facts::{ExportKind, ExportTags, KindFilter},
        glob::PathSet,
        hash::ContentHash,
        ids::RuleId,
        level::Level,
        pattern::Pattern,
        scope::Scope,
    };

    const TARGET: &str = "src/user/create-client.use-case.ts";

    fn shape_of(rules: Vec<CompiledRule>) -> Scaffold {
        scaffold(&config(rules), &path(TARGET))
    }

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn rule(id: &str, scope: &[&str], kind: CompiledRuleKind) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
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
            ignore_files: archwarden_core::glob::PathSet::default(),
        }
    }

    fn spec_pair() -> CompiledRuleKind {
        CompiledRuleKind::SpecPair {
            subfolders: vec![".".to_owned()],
            spec_markers: vec!["spec".to_owned()],
            ignore_files: PathSet::default(),
            spec_dirs: Vec::new(),
            require_non_empty_spec: true,
            skip_type_only: false,
        }
    }

    fn boundary(forbid: &[&str], require: &[&str], except: &[&str]) -> CompiledRuleKind {
        CompiledRuleKind::ImportBoundary {
            forbid: set(forbid),
            groups: Vec::new(),
            allow: None,
            allow_packages: None,
            require: set(require),
            forbid_packages: Vec::new(),
            forbid_reaching: PathSet::default(),
            except: set(except),
            except_from: PathSet::default(),
            include_type_only: true,
        }
    }

    fn function_kind() -> KindFilter {
        KindFilter::OneOf(ExportTags::only(ExportKind::Function))
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
            ignore_files: archwarden_core::glob::PathSet::default(),
        }
    }

    fn rendered(rules: Vec<CompiledRule>, format: crate::report::Format) -> String {
        let target = path(TARGET);
        let shape = scaffold(&config(rules), &target);
        let mut out = Vec::new();
        render(&target, &shape, format, &mut out);
        String::from_utf8(out).expect("output is UTF-8")
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

    /// And the options the call has to be given, on their own line under it.
    /// Issue #164: this is what somebody about to write the call has to type,
    /// and a line they can copy beats a clause they have to parse.
    #[test]
    fn a_call_obligations_options_are_printed_under_it() {
        let text = rendered(
            vec![rule(
                "specs-run-in-memory",
                &["src/*"],
                CompiledRuleKind::CallObligation {
                    file_pattern: Pattern::compile(r"^create-client\.use-case\.ts$")
                        .expect("valid pattern"),
                    symbol: "FactoryMockDependencies".to_owned(),
                    imported_from: "../test/factories".to_owned(),
                    with_options: vec![
                        ("PAY_IN_MEMORY".to_owned(), None),
                        ("strict".to_owned(), Some("true".to_owned())),
                    ],
                },
            )],
            crate::report::Format::Text,
        );

        assert!(
            text.contains(
                "    FactoryMockDependencies — imported from ../test/factories\n\
                 \x20     passing { PAY_IN_MEMORY, strict: true }\n"
            ),
            "{text}"
        );
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
                    groups: Vec::new(),
                    allow: None,
                    allow_packages: None,
                    require: PathSet::default(),
                    forbid_packages: Vec::new(),
                    forbid_reaching: PathSet::default(),
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

    /// The literal lists, which the pattern tests never reached.
    ///
    /// A rule may permit folders by name, by regex, or by both, and the three
    /// answers are different sentences. Mutation testing found every one of
    /// these branches untested: the pattern path was exercised end to end and
    /// the `allowed_subfolders` path was not.
    #[test]
    fn a_name_on_the_allowed_list_is_permitted() {
        let shape = Scaffold {
            folder_name: Some(AllowedSubfolders {
                allowed: vec!["sketch".to_owned(), "minha-solucao".to_owned()],
                warn: Vec::new(),
                patterns: Vec::new(),
            }),
            ..Scaffold::default()
        };

        assert_eq!(
            name_is_not_permitted(&path("projetos/01/sketch"), &shape),
            None
        );
        assert!(
            name_is_not_permitted(&path("projetos/01/outra"), &shape)
                .is_some_and(|why| why.contains("not one of the names allowed here")),
            "a name off the list should be refused"
        );
    }

    /// A warn-listed name is permitted. It reports as a warning when the folder
    /// exists, and `scaffold` must not tell someone the path is impossible when
    /// the project has said it is merely discouraged.
    #[test]
    fn a_warn_listed_name_is_permitted() {
        let shape = Scaffold {
            folder_name: Some(AllowedSubfolders {
                allowed: vec!["sketch".to_owned()],
                warn: vec!["rascunho".to_owned()],
                patterns: Vec::new(),
            }),
            ..Scaffold::default()
        };

        assert_eq!(
            name_is_not_permitted(&path("projetos/01/rascunho"), &shape),
            None
        );
    }

    /// A parent that permits no subfolder at all is a different message: the
    /// folder is not misnamed, it should not be there.
    #[test]
    fn a_parent_that_allows_no_subfolder_says_that_instead() {
        let shape = Scaffold {
            folder_name: Some(AllowedSubfolders {
                allowed: Vec::new(),
                warn: Vec::new(),
                patterns: Vec::new(),
            }),
            ..Scaffold::default()
        };

        assert!(
            name_is_not_permitted(&path("src/user/anything"), &shape)
                .is_some_and(|why| why.contains("allows no subfolder at all")),
            "a rule forbidding every subfolder should say so"
        );
    }

    /// And a shape with no folder constraint refuses nothing.
    #[test]
    fn a_shape_that_says_nothing_about_folder_names_refuses_nothing() {
        assert_eq!(
            name_is_not_permitted(&path("src/user/anything"), &Scaffold::default()),
            None
        );
    }

    /// The rendered line for a parent that permits no subfolder.
    ///
    /// Without it the section prints its heading and then nothing, which reads
    /// as a rule that forgot to say what it wanted rather than one saying this
    /// folder should not exist.
    #[test]
    fn a_folder_section_with_nothing_permitted_still_says_something() {
        let target = path("projetos/qualquer");
        let shape = Scaffold {
            folder_name: Some(AllowedSubfolders {
                allowed: Vec::new(),
                warn: Vec::new(),
                patterns: Vec::new(),
            }),
            ..Scaffold::default()
        };

        let mut out = Vec::new();
        render_text(&target, &shape, &mut out);
        let text = String::from_utf8(out).expect("UTF-8");

        assert!(
            text.contains("its parent allows no subfolder at all"),
            "the section was left empty:\n{text}"
        );
    }

    /// And a folder section that *does* permit something does not print it.
    #[test]
    fn a_folder_section_with_names_does_not_claim_none_are_allowed() {
        let target = path("projetos/01-blink");
        let shape = Scaffold {
            folder_name: Some(AllowedSubfolders {
                allowed: vec!["01-blink".to_owned()],
                warn: Vec::new(),
                patterns: Vec::new(),
            }),
            ..Scaffold::default()
        };

        let mut out = Vec::new();
        render_text(&target, &shape, &mut out);
        let text = String::from_utf8(out).expect("UTF-8");

        assert!(
            !text.contains("allows no subfolder at all"),
            "it claimed nothing is allowed while listing a name:\n{text}"
        );
        assert!(text.contains("01-blink"), "{text}");
    }
}
