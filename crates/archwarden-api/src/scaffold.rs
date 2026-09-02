//! `scaffold` — the smallest shape that would satisfy the rules at a path.
//!
//! [`crate::describe`] answers rule by rule. An agent about to write a file
//! does not think rule by rule: it wants one list of exports, one list of
//! siblings, one list of import constraints. This is the same expectations,
//! transposed.
//!
//! Not a code generator. It emits structural requirements, and the signature
//! it shows comes from `signature_hint`, which the config author writes and
//! archwarden never verifies (`RULES.md`).
//!
//! It lived in `archwarden-cli` until 0.18. The JSON it produces is a
//! documented contract with agents, carrying a version of its own, and a
//! second surface assembling that shape for itself would be a second
//! implementation of the contract — which is what decision 20 draws this
//! boundary to prevent.

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
    /// When asked about a directory: what *this* directory may be called.
    ///
    /// The same argument as [`filename_patterns`](Self::filename_patterns),
    /// which was correction C11: an agent scaffolding a path whose name is
    /// already wrong would be told everything except the thing it has to fix
    /// first. That argument was made for files and not carried to folders, so
    /// `scaffold` handed back a shape to build at a path `check` refuses —
    /// and following the answer produced a directory that failed on the next
    /// run.
    pub folder_name: Option<AllowedSubfolders>,
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
    /// When asked about a directory: what may **not** live inside it.
    ///
    /// Beside `forbidden_imports` rather than inside `required_files`: a
    /// consumer acting on that one creates what it names, and a list it must
    /// not create cannot travel there. Always present and often empty, on the
    /// same grounds `required_files` is. Issue #177.
    #[serde(default)]
    pub forbidden_files: Vec<String>,
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
    /// Options the call has to be given, when the rule asks for any.
    ///
    /// Carried rather than dropped: an agent writing the call from this shape
    /// without them writes one the rule then refuses, which is the failure
    /// `scaffold` exists to prevent. Rendered as `key` or `key: value`,
    /// the way it would be written. Issue #164.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub with_options: Vec<String>,
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

/// One required call, with its options rendered the way they are written.
///
/// Its own function because `absorb` is a long match and this arm has a body:
/// an agent writing the call from this shape without the options writes one
/// the rule then refuses, which is the failure `scaffold` exists to prevent.
fn obligation(
    symbol: String,
    imported_from: String,
    with_options: Vec<archwarden_core::finding::RequiredOption>,
) -> CallObligation {
    CallObligation {
        symbol,
        imported_from,
        with_options: with_options
            .into_iter()
            .map(|option| match option.value {
                Some(value) => format!("{}: {value}", option.key),
                None => option.key,
            })
            .collect(),
    }
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
            with_options,
        } => shape
            .call_obligations
            .push(obligation(symbol, imported_from, with_options)),
        Expectation::FilenamePattern { patterns } => shape.filename_patterns.extend(patterns),
        // Into the same list as a spec sibling: for someone about to write the
        // file the two are one instruction, "create this too", and the reason
        // one of them is derived and the other literal is not their problem.
        Expectation::RequiredCompanion { path } => shape.required_siblings.push(RequiredSibling {
            path,
            constraints: Vec::new(),
        }),
        Expectation::RequiredFiles { names, patterns } => {
            shape.required_files.names.extend(names);
            shape.required_files.patterns.extend(patterns);
        }
        Expectation::ForbiddenFiles { names } => shape.forbidden_files.extend(names),
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
        Expectation::FolderName {
            allowed,
            warn,
            patterns,
        } => {
            shape.folder_name = Some(AllowedSubfolders {
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
///
/// Public, and built here rather than in a renderer, for the reason
/// [`crate::render`] gives about the report: a shape a program consumes is a
/// contract, and every surface has to emit the same one.
#[derive(Debug, Serialize)]
pub struct JsonScaffold<'a> {
    /// The shape's version, [`SCAFFOLD_VERSION`].
    pub version: u32,
    /// The path asked about.
    pub path: &'a RepoRelPath,
    /// The shape itself, flattened into the envelope.
    #[serde(flatten)]
    pub shape: &'a Scaffold,
}

/// The JSON answer for one path.
#[must_use]
pub fn envelope<'a>(path: &'a RepoRelPath, shape: &'a Scaffold) -> JsonScaffold<'a> {
    JsonScaffold {
        version: SCAFFOLD_VERSION,
        path,
        shape,
    }
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
            not_yet: None,
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

    const TARGET: &str = "src/user/create-client.use-case.ts";

    fn shape_of(rules: Vec<CompiledRule>) -> Scaffold {
        scaffold(&config(rules), &path(TARGET))
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

    /// The JSON an agent receives, as text.
    fn rendered_json(rules: Vec<CompiledRule>) -> String {
        let target = path(TARGET);
        serde_json::to_string_pretty(&envelope(&target, &scaffold(&config(rules), &target)))
            .expect("serialises")
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
                groups: Vec::new(),
                allow: None,
                allow_packages: None,
                require: PathSet::default(),
                forbid_packages: vec!["three".to_owned()],
                forbid_reaching: PathSet::default(),
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

    /// Issue #45. `scaffold projetos/17-nova/projeto.md` naming `notas.md` is
    /// what stops the pair being half-written in the first place -- the whole
    /// failure is that nobody notices the missing half.
    #[test]
    fn a_pair_rule_lists_the_companion_to_create() {
        let shape = scaffold(
            &config(vec![rule(
                "licao-tem-notas",
                &["projetos/*"],
                CompiledRuleKind::Pair {
                    file_pattern: Pattern::compile(r"^projeto\.md$").expect("valid"),
                    must_exist: "notas.md".to_owned(),
                },
            )]),
            &path("projetos/17-nova/projeto.md"),
        );

        assert_eq!(
            shape
                .required_siblings
                .iter()
                .map(|sibling| sibling.path.as_str())
                .collect::<Vec<_>>(),
            ["projetos/17-nova/notas.md"]
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
                    forbid: Vec::new(),
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

    /// An agent reads the JSON, so the requirement has to be in it -- and
    /// beside `signature_hint`, not instead of it, because the two make
    /// different promises.
    #[test]
    fn the_required_annotation_reaches_the_json() {
        let text = rendered_json(vec![rule(
            "contract",
            &["src/*"],
            annotated_naming(
                &["UseCaseModule", "LegacyUseCase"],
                KindFilter::OneOf(ExportTags::only(ExportKind::Const)),
            ),
        )]);
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
        let text = rendered_json(vec![rule(
            "name",
            &["src/*"],
            naming(None, function_kind()),
        )]);
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");

        assert!(parsed["required_exports"][0].get("annotation").is_none());
    }

    /// The JSON is the contract an agent should consume.
    #[test]
    fn the_json_shape_is_versioned_and_flat() {
        let json = rendered_json(vec![
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
        ]);
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
                with_options: Vec::new(),
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

    /// And the options it has to be given. Issue #164: an agent writing the
    /// call from this shape without them writes one the rule then refuses,
    /// which is the failure `scaffold` exists to prevent.
    #[test]
    fn a_call_obligations_options_reach_the_shape() {
        let shape = shape_of(vec![rule(
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
        )]);

        // Rendered the way they are written, so the line can be copied rather
        // than translated.
        assert_eq!(
            shape
                .call_obligations
                .first()
                .map(|c| c.with_options.as_slice()),
            Some(["PAY_IN_MEMORY".to_owned(), "strict: true".to_owned()].as_slice())
        );
    }

    /// A filename constraint reaches the shape, because an agent scaffolding a
    /// path whose *name* is already wrong is told everything except the thing
    /// it has to fix first.
    #[test]
    fn a_filename_pattern_reaches_the_shape() {
        let shape = shape_of(vec![rule(
            "shape",
            &["src/*"],
            CompiledRuleKind::Structure {
                allowed_subfolders: None,
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: Vec::new(),
                filename_patterns: vec![Pattern::compile(r"^[a-z-]+\.ts$").expect("valid")],
            },
        )]);

        assert_eq!(shape.filename_patterns, [r"^[a-z-]+\.ts$"]);
    }

    /// A companion lands in the same list a spec sibling does: for someone
    /// about to write the file the two are one instruction, "create this too",
    /// and which of them is derived is not their problem.
    #[test]
    fn a_companion_lands_beside_the_siblings_without_constraints() {
        let shape = scaffold(
            &config(vec![rule(
                "tem-notas",
                &["projetos/*"],
                CompiledRuleKind::Pair {
                    file_pattern: Pattern::compile(r"^projeto\.md$").expect("valid"),
                    must_exist: "notas.md".to_owned(),
                },
            )]),
            &path("projetos/17-nova/projeto.md"),
        );

        let companion = shape
            .required_siblings
            .first()
            .expect("the companion is listed");
        assert!(
            companion.path.as_str().ends_with("notas.md"),
            "{:?}",
            companion.path
        );
        assert!(
            companion.constraints.is_empty(),
            "a companion carries no spec constraint"
        );
    }

    /// A directory gets both halves of what must be in it: the names, and the
    /// pattern at least one file has to match.
    #[test]
    fn a_directory_gets_the_files_that_must_exist_in_it() {
        let shape = scaffold(
            &config(vec![rule(
                "licao-completa",
                &["projetos/*"],
                CompiledRuleKind::Presence {
                    require: vec!["projeto.md".to_owned()],
                    require_any: vec![Pattern::compile(r"\.ino$").expect("valid")],
                    forbid: Vec::new(),
                },
            )]),
            &path("projetos/17-nova"),
        );

        assert_eq!(shape.required_files.names, ["projeto.md"]);
        assert_eq!(shape.required_files.patterns, [r"\.ino$"]);
    }

    /// Asked about a directory, the shape carries the subfolders allowed
    /// inside it — which is what `scaffold` answers that `describe` does not.
    #[test]
    fn a_directory_gets_its_allowed_subfolders() {
        let shape = scaffold(
            &config(vec![rule(
                "shape",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec!["types".to_owned()]),
                    warn_subfolders: vec!["shared".to_owned()],
                    recurse_into: Vec::new(),
                    subfolder_patterns: vec![Pattern::compile("^v[0-9]+$").expect("valid")],
                    filename_patterns: Vec::new(),
                },
            )]),
            &path("src/user"),
        );

        let subfolders = shape
            .allowed_subfolders
            .as_ref()
            .expect("a directory has them");
        assert_eq!(subfolders.allowed, ["types"]);
        assert_eq!(subfolders.warn, ["shared"]);
        assert_eq!(subfolders.patterns, ["^v[0-9]+$"]);
    }

    /// And the folder's *own* name, which is a different question from what
    /// may sit inside it. Issue #53: `scaffold` used to describe a shape to
    /// build at a path `check` rejects.
    #[test]
    fn a_directory_gets_the_names_its_parent_permits() {
        let shape = scaffold(
            &config(vec![rule(
                "shape",
                &["projetos/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec!["sketch".to_owned()]),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )]),
            &path("projetos/01-blink/qualquer"),
        );

        let own = shape
            .folder_name
            .as_ref()
            .expect("its parent constrains it");
        assert_eq!(own.allowed, ["sketch"]);
    }
}
