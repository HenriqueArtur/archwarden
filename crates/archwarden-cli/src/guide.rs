//! `archwarden agent-guide` — the rule set, written for an agent's context.
//!
//! Layer 3 of `AGENT-INTEGRATION.md`: a digest a harness already knows how to
//! read, referenced from `CLAUDE.md` or `AGENTS.md`. `describe` answers a
//! specific question about a specific path; this teaches the rules before any
//! question is asked.
//!
//! # Why this does not go through `describe_expectation`
//!
//! `ARCHITECTURE.md:252` says the guide "iterates every rule in the config and
//! calls the same `describe_expectation()` per rule". It cannot: that method
//! takes a path, and for good reason -- a `naming` rule's expectation contains
//! the *rendered* export name, which comes from the filename. A guide has no
//! filename, and inventing one would fill the digest with names derived from a
//! path nobody will ever create.
//!
//! So the guide renders the compiled configuration itself. That is the same
//! data the engines consume, so it cannot misstate a rule's globs, patterns or
//! templates; and the precise per-path answers remain `describe` and
//! `scaffold`, which do go through the expectation seam. Correction C12.

use std::fmt::Write as _;

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRule, CompiledRuleKind},
    facts::{ExportKind, KindFilter},
    path::RepoRelPath,
};
use serde::Serialize;

/// The version of the `agent-guide` JSON shape.
pub const GUIDE_VERSION: u32 = 0;

/// Every rule the guide covers, in configuration order.
#[derive(Debug, Serialize)]
pub struct Guide<'a> {
    version: u32,
    /// The scope the guide was restricted to, when it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<&'a str>,
    rules: Vec<GuideRule<'a>>,
}

#[derive(Debug, Serialize)]
struct GuideRule<'a> {
    id: &'a str,
    kind: &'static str,
    level: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    module: Option<&'a str>,
    /// Directory globs the rule governs.
    applies_to: &'a [String],
    /// One sentence per requirement, the same prose `describe` prints.
    requires: Vec<String>,
}

/// Every rule kind archwarden has, as written in a config and on the command
/// line.
///
/// Listed here rather than derived, because `CompiledRuleKind::type_name` maps
/// one way only. A test walks the enum through this list, so the two cannot
/// drift.
pub const KINDS: [&str; 5] = [
    "structure",
    "naming",
    "spec-pair",
    "import-boundary",
    "call-obligation",
];

/// Checks the kinds a caller asked for.
///
/// # Errors
/// A message naming the one archwarden does not have, and the ones it does.
/// Refused rather than answered with an empty digest: an agent handed one
/// would conclude the project has no rules of that kind, which is the wrong
/// lesson to draw from a typo.
pub fn guide_kinds(kinds: &[String]) -> Result<(), String> {
    for kind in kinds {
        if !KINDS.contains(&kind.as_str()) {
            let known: Vec<String> = KINDS.iter().map(|kind| format!("`{kind}`")).collect();
            return Err(format!(
                "no rule kind is called `{kind}`; there is {}",
                known.join(", ")
            ));
        }
    }
    Ok(())
}

/// Builds the guide, optionally restricted to rules that can fire under
/// `scope` and to the kinds named.
///
/// Both filters are AND, and an empty `kinds` means every kind: "the import
/// boundaries that affect this directory" is one question, not two.
#[must_use]
pub fn guide<'a>(
    config: &'a CompiledConfig,
    scope: Option<&'a RepoRelPath>,
    kinds: &[String],
) -> Guide<'a> {
    Guide {
        version: GUIDE_VERSION,
        scope: scope.map(RepoRelPath::as_str),
        rules: config
            .rules()
            .filter(|rule| kinds.is_empty() || kinds.iter().any(|k| k == rule.kind.type_name()))
            .filter(|rule| scope.is_none_or(|prefix| reaches(rule, prefix)))
            .map(|rule| GuideRule {
                id: rule.id.as_str(),
                kind: rule.kind.type_name(),
                level: rule.level.as_str(),
                module: rule
                    .module
                    .as_ref()
                    .map(archwarden_core::ids::ModuleId::as_str),
                applies_to: rule.scope.patterns(),
                requires: requirements(&rule.kind),
            })
            .collect(),
    }
}

/// Whether a rule could ever fire under `prefix`.
///
/// Two directions, because a user asking for "the guide for `packages/domain`"
/// means both. A rule scoped to `packages/**` governs that directory; a rule
/// scoped to `packages/domain/src/*` lives inside it. Either answer is yes.
fn reaches(rule: &CompiledRule, prefix: &RepoRelPath) -> bool {
    if rule.scope.matches_dir(prefix.as_path()) {
        return true;
    }

    let prefix = prefix.as_str();
    rule.scope.patterns().iter().any(|pattern| {
        prefix.is_empty() || pattern == prefix || pattern.starts_with(&format!("{prefix}/"))
    })
}

/// One sentence per thing the rule requires.
fn requirements(kind: &CompiledRuleKind) -> Vec<String> {
    match kind {
        CompiledRuleKind::Structure {
            allowed_subfolders,
            warn_subfolders,
            filename_patterns,
            ..
        } => {
            let mut lines = Vec::new();
            if !allowed_subfolders.is_empty() || !warn_subfolders.is_empty() {
                let mut line = format!("subfolders: {}", join(allowed_subfolders));
                if !warn_subfolders.is_empty() {
                    let _ = write!(line, "; allowed with a warning: {}", join(warn_subfolders));
                }
                lines.push(line);
            }
            if !filename_patterns.is_empty() {
                lines.push(format!(
                    "filenames must match: {}",
                    join(
                        &filename_patterns
                            .iter()
                            .map(|p| p.as_str().to_owned())
                            .collect::<Vec<_>>()
                    )
                ));
            }
            lines
        }
        CompiledRuleKind::Naming {
            file_pattern,
            name_template,
            kind,
            signature_hint,
        } => {
            let mut lines = vec![format!(
                "files matching `{}` must export `{name_template}`{}",
                file_pattern.as_str(),
                declared_as(kind),
            )];
            if let Some(hint) = signature_hint {
                lines.push(format!("suggested signature: `{hint}`"));
            }
            lines
        }
        CompiledRuleKind::SpecPair {
            subfolders,
            spec_markers,
            require_non_empty_spec,
            ..
        } => {
            let markers = spec_markers
                .iter()
                .map(|marker| format!(".{marker}."))
                .collect::<Vec<_>>();
            let mut line = format!(
                "every file in {} needs a sibling named with {}",
                join(subfolders),
                join(&markers)
            );
            if *require_non_empty_spec {
                line.push_str(", containing at least one test case");
            }
            vec![line]
        }
        CompiledRuleKind::ImportBoundary {
            forbid,
            require,
            except,
            include_type_only,
        } => {
            let mut lines = Vec::new();
            if !forbid.is_empty() {
                let mut line = format!("must not import from {}", join(forbid.patterns()));
                if !except.is_empty() {
                    let _ = write!(line, ", except {}", join(except.patterns()));
                }
                if !include_type_only {
                    line.push_str(" (type-only imports are exempt)");
                }
                lines.push(line);
            }
            if !require.is_empty() {
                lines.push(format!("must import from {}", join(require.patterns())));
            }
            lines
        }
        CompiledRuleKind::CallObligation {
            file_pattern,
            symbol,
            imported_from,
        } => vec![format!(
            "files matching `{}` must call `{symbol}`, imported from `{imported_from}`",
            file_pattern.as_str()
        )],
    }
}

fn declared_as(kind: &KindFilter) -> String {
    match kind {
        KindFilter::OneOf(tags) => {
            let kinds: Vec<String> = tags
                .iter()
                .map(|k| ExportKind::as_str(k).to_owned())
                .collect();
            format!(", declared as {}", join(&kinds))
        }
        // `Any`, and any filter added later: the rule asked for no particular
        // form, and naming one would teach the agent a constraint that is not
        // there.
        _ => String::new(),
    }
}

fn join(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| format!("`{item}`")).collect();
    match quoted.split_last() {
        None => "nothing".to_owned(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

/// How to render the guide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum GuideFormat {
    /// Grep-friendly headings, one section per rule.
    #[default]
    Markdown,
    /// The same content, as a versioned object.
    Json,
}

/// Writes the guide.
pub fn render(guide: &Guide<'_>, format: GuideFormat, out: &mut dyn std::io::Write) {
    match format {
        GuideFormat::Markdown => render_markdown(guide, out),
        GuideFormat::Json => render_json(guide, out),
    }
}

fn render_json(guide: &Guide<'_>, out: &mut dyn std::io::Write) {
    match serde_json::to_string_pretty(guide) {
        Ok(json) => {
            let _ = writeln!(out, "{json}");
        }
        Err(error) => {
            let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
        }
    }
}

fn render_markdown(guide: &Guide<'_>, out: &mut dyn std::io::Write) {
    let _ = writeln!(out, "# Architecture rules\n");

    // No timestamp, no version string, no host name. The output is committed
    // by some users and regenerated by others; anything that changed between
    // two identical configurations would show up as a diff nobody made.
    let _ = writeln!(
        out,
        "Generated by archwarden from the project's configuration. \
         Same configuration, same file.\n"
    );

    if guide.rules.is_empty() {
        let _ = writeln!(out, "No rules are configured.");
        return;
    }

    for rule in &guide.rules {
        let _ = writeln!(out, "## `{}` ({})\n", rule.id, rule.kind);

        let module = rule
            .module
            .map_or_else(String::new, |module| format!(" · module `{module}`"));
        let _ = writeln!(
            out,
            "- **Level**: {}{module}\n- **Applies to**: {}",
            rule.level,
            join(rule.applies_to)
        );

        for requirement in &rule.requires {
            let _ = writeln!(out, "- {requirement}");
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(
        out,
        "Ask `archwarden describe <path>` for what applies to one file, and \
         `archwarden scaffold <path>` for the shape it should have."
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::SkipDirs,
        facts::ExportTags,
        glob::PathSet,
        hash::ContentHash,
        ids::{ModuleId, RuleId},
        level::Level,
        pattern::Pattern,
        scope::Scope,
    };

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
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
            scope: Scope::compile(scope.iter().copied()).expect("valid scope"),
            kind,
        }
    }

    fn config(rules: Vec<CompiledRule>) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b"guide"),
        )
    }

    fn set(patterns: &[&str]) -> PathSet {
        PathSet::compile(patterns.iter().map(|p| (*p).to_owned())).expect("valid globs")
    }

    fn naming() -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile(r"^(?<name>[a-z0-9-]+)\.use-case\.ts$")
                .expect("valid pattern"),
            name_template: "{{pascal(name)}}".to_owned(),
            kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            signature_hint: Some("(deps: Deps): UseCase".to_owned()),
        }
    }

    fn rendered(
        config: &CompiledConfig,
        scope: Option<&RepoRelPath>,
        format: GuideFormat,
    ) -> String {
        rendered_of(config, scope, &[], format)
    }

    fn rendered_of(
        config: &CompiledConfig,
        scope: Option<&RepoRelPath>,
        kinds: &[&str],
        format: GuideFormat,
    ) -> String {
        let owned: Vec<String> = kinds.iter().map(|k| (*k).to_owned()).collect();
        let mut out = Vec::new();
        render(&guide(config, scope, &owned), format, &mut out);
        String::from_utf8(out).expect("output is UTF-8")
    }

    fn boundary() -> CompiledRuleKind {
        CompiledRuleKind::ImportBoundary {
            forbid: set(&["src/infra/**"]),
            require: PathSet::default(),
            except: PathSet::default(),
            include_type_only: true,
        }
    }

    fn mixed() -> CompiledConfig {
        config(vec![
            rule("usecase-name", None, &["src/*"], naming()),
            rule("no-infra", None, &["src/**"], boundary()),
            rule("also-no-infra", None, &["packages/**"], boundary()),
        ])
    }

    /// "Just the import boundaries that reach this directory" is a real
    /// question, and answering it by hand means reading past everything else.
    #[test]
    fn a_kind_narrows_the_digest_to_that_kind() {
        let markdown = rendered_of(&mixed(), None, &["import-boundary"], GuideFormat::Markdown);

        assert!(markdown.contains("no-infra"), "{markdown}");
        assert!(markdown.contains("also-no-infra"), "{markdown}");
        assert!(!markdown.contains("usecase-name"), "{markdown}");
    }

    /// Several kinds are a set, however they were written on the command line.
    #[test]
    fn several_kinds_are_a_set() {
        let markdown = rendered_of(
            &mixed(),
            None,
            &["import-boundary", "naming"],
            GuideFormat::Markdown,
        );

        assert!(markdown.contains("no-infra"), "{markdown}");
        assert!(markdown.contains("usecase-name"), "{markdown}");
    }

    /// With `--scope`, because the question that prompted this was "the import
    /// boundaries *that affect this directory*" -- one filter answers half of
    /// it.
    #[test]
    fn a_kind_composes_with_a_scope() {
        let markdown = rendered_of(
            &mixed(),
            Some(&path("src")),
            &["import-boundary"],
            GuideFormat::Markdown,
        );

        assert!(markdown.contains("no-infra"), "{markdown}");
        assert!(
            !markdown.contains("also-no-infra"),
            "scoped to packages: {markdown}"
        );
        assert!(!markdown.contains("usecase-name"), "{markdown}");
    }

    /// A kind no rule has is refused, not answered with an empty digest. An
    /// agent handed an empty guide would conclude the project has no rules of
    /// that kind, which is the wrong lesson to draw from a typo.
    ///
    /// `boundary` rather than a misspelling, because the likely mistake is a
    /// user writing the short name they say out loud.
    #[test]
    fn an_unknown_kind_is_refused_and_names_the_real_ones() {
        let message = guide_kinds(&["boundary".to_owned()]).expect_err("no such kind");

        assert!(message.contains("`boundary`"), "{message}");
        assert!(message.contains("`import-boundary`"), "{message}");
        assert!(message.contains("`spec-pair`"), "{message}");
    }

    /// Every kind archwarden has is accepted, so the list cannot drift from
    /// the enum it describes.
    #[test]
    fn every_kind_the_tool_has_is_accepted() {
        for kind in [
            "structure",
            "naming",
            "spec-pair",
            "import-boundary",
            "call-obligation",
        ] {
            guide_kinds(&[kind.to_owned()]).unwrap_or_else(|_| panic!("{kind} is a kind"));
        }
    }

    /// The guide describes a rule generically: the *template*, not a name
    /// rendered from a filename that does not exist.
    #[test]
    fn a_naming_rule_is_described_by_its_template() {
        let markdown = rendered(
            &config(vec![rule(
                "usecase-name",
                Some("app"),
                &["src/*"],
                naming(),
            )]),
            None,
            GuideFormat::Markdown,
        );

        assert!(markdown.contains("{{pascal(name)}}"), "{markdown}");
        assert!(markdown.contains("declared as `function`"), "{markdown}");
        assert!(
            markdown.contains("suggested signature: `(deps: Deps): UseCase`"),
            "{markdown}"
        );
        assert!(markdown.contains("module `app`"), "{markdown}");
    }

    /// The output is committed by some users and regenerated by others, so
    /// nothing that varies between two identical configurations may appear:
    /// no timestamp, no version string, no host name.
    #[test]
    fn the_same_configuration_gives_the_same_bytes() {
        let config = config(vec![
            rule("usecase-name", None, &["src/*"], naming()),
            rule(
                "boundary",
                None,
                &["src/**"],
                CompiledRuleKind::ImportBoundary {
                    forbid: set(&["src/infra/**"]),
                    require: PathSet::default(),
                    except: PathSet::default(),
                    include_type_only: true,
                },
            ),
        ]);

        assert_eq!(
            rendered(&config, None, GuideFormat::Markdown),
            rendered(&config, None, GuideFormat::Markdown)
        );
        assert!(
            !rendered(&config, None, GuideFormat::Markdown).contains("202"),
            "no date leaks in"
        );
    }

    /// Configuration order is preserved, so a diff of a committed guide
    /// follows the config rather than an internal ordering.
    #[test]
    fn rules_appear_in_configuration_order() {
        let markdown = rendered(
            &config(vec![
                rule("second", None, &["src/*"], naming()),
                rule(
                    "first",
                    None,
                    &["src/*"],
                    CompiledRuleKind::CallObligation {
                        file_pattern: Pattern::compile("^x$").expect("valid"),
                        symbol: "Event.save".to_owned(),
                        imported_from: "@org/domain/event".to_owned(),
                    },
                ),
            ]),
            None,
            GuideFormat::Markdown,
        );

        let second = markdown.find("`second`").expect("present");
        let first = markdown.find("`first`").expect("present");
        assert!(second < first, "config order, not alphabetical");
    }

    /// `--scope` keeps a rule that governs the directory asked about.
    #[test]
    fn a_rule_governing_the_scope_is_kept() {
        let config = config(vec![rule("wide", None, &["packages/**"], naming())]);
        let scope = path("packages/domain");

        assert_eq!(guide(&config, Some(&scope), &[]).rules.len(), 1);
    }

    /// And one that lives *inside* it, which is the other direction a user
    /// means by "the guide for this package".
    #[test]
    fn a_rule_inside_the_scope_is_kept() {
        let config = config(vec![rule(
            "inner",
            None,
            &["packages/domain/src/*"],
            naming(),
        )]);
        let scope = path("packages/domain");

        assert_eq!(guide(&config, Some(&scope), &[]).rules.len(), 1);
    }

    /// A rule that can never fire under the scope is left out -- that is the
    /// whole point of the flag.
    #[test]
    fn a_rule_elsewhere_is_dropped() {
        let config = config(vec![
            rule("here", None, &["packages/domain/**"], naming()),
            rule("elsewhere", None, &["apps/web/**"], naming()),
        ]);

        let scope = path("packages/domain");
        let built = guide(&config, Some(&scope), &[]);
        let ids: Vec<_> = built.rules.iter().map(|r| r.id).collect();
        assert_eq!(ids, ["here"]);
    }

    /// No scope means the whole configuration.
    #[test]
    fn no_scope_keeps_everything() {
        let config = config(vec![
            rule("here", None, &["packages/domain/**"], naming()),
            rule("elsewhere", None, &["apps/web/**"], naming()),
        ]);

        assert_eq!(guide(&config, None, &[]).rules.len(), 2);
    }

    /// A scope of the repository root keeps everything, which is what
    /// `--scope .` means and what any rule at the top level needs.
    #[test]
    fn the_root_as_a_scope_keeps_everything() {
        let config = config(vec![
            rule("here", None, &["packages/domain/**"], naming()),
            rule("elsewhere", None, &["apps/web/**"], naming()),
        ]);
        let root = path(".");

        assert!(root.is_root());
        assert_eq!(guide(&config, Some(&root), &[]).rules.len(), 2);
    }

    /// A structure rule may constrain only the warn list, or only filenames.
    /// Each half has to reach the guide on its own, or a rule would be listed
    /// with nothing under it.
    #[test]
    fn each_half_of_a_structure_rule_stands_alone() {
        let warn_only = rendered(
            &config(vec![rule(
                "shape",
                None,
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Vec::new(),
                    warn_subfolders: vec!["shared".to_owned()],
                    recurse_into: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )]),
            None,
            GuideFormat::Markdown,
        );
        assert!(warn_only.contains("subfolders: nothing"), "{warn_only}");
        assert!(
            warn_only.contains("allowed with a warning: `shared`"),
            "{warn_only}"
        );

        let names_only = rendered(
            &config(vec![rule(
                "shape",
                None,
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Vec::new(),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    filename_patterns: vec![Pattern::compile("^[a-z-]+\\.ts$").expect("valid")],
                },
            )]),
            None,
            GuideFormat::Markdown,
        );
        assert!(names_only.contains("filenames must match:"), "{names_only}");
        assert!(
            !names_only.contains("subfolders:"),
            "a rule that constrains no subfolders says nothing about them: {names_only}"
        );
    }

    /// A structure rule that constrains only its subfolders says nothing about
    /// filenames, for the same reason.
    #[test]
    fn a_structure_rule_without_filename_patterns_says_nothing_about_names() {
        let markdown = rendered(
            &config(vec![rule(
                "shape",
                None,
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: vec!["types".to_owned()],
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )]),
            None,
            GuideFormat::Markdown,
        );

        assert!(markdown.contains("subfolders: `types`"), "{markdown}");
        assert!(!markdown.contains("allowed with a warning"), "{markdown}");
        assert!(!markdown.contains("filenames must match"), "{markdown}");
    }

    /// An empty configuration says so rather than emitting a heading and
    /// nothing under it.
    #[test]
    fn an_empty_configuration_says_so() {
        let markdown = rendered(&config(Vec::new()), None, GuideFormat::Markdown);
        assert!(markdown.contains("No rules are configured."), "{markdown}");
    }

    /// The JSON is the machine-readable half, versioned like the others.
    #[test]
    fn the_json_shape_is_versioned() {
        let scope = path("src");
        let json = rendered(
            &config(vec![rule(
                "usecase-name",
                Some("app"),
                &["src/*"],
                naming(),
            )]),
            Some(&scope),
            GuideFormat::Json,
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(parsed["version"], 0);
        assert_eq!(parsed["scope"], "src");
        assert_eq!(parsed["rules"][0]["id"], "usecase-name");
        assert_eq!(parsed["rules"][0]["kind"], "naming");
        assert_eq!(parsed["rules"][0]["level"], "error");
        assert_eq!(parsed["rules"][0]["module"], "app");
        assert_eq!(parsed["rules"][0]["applies_to"][0], "src/*");
        assert!(
            parsed["rules"][0]["requires"][0]
                .as_str()
                .is_some_and(|line| line.contains("{{pascal(name)}}")),
            "{json}"
        );
    }

    /// An unrestricted guide omits the scope rather than sending null.
    #[test]
    fn an_unrestricted_guide_omits_the_scope() {
        let json = rendered(
            &config(vec![rule("usecase-name", None, &["src/*"], naming())]),
            None,
            GuideFormat::Json,
        );

        assert!(!json.contains("\"scope\""), "{json}");
    }

    /// Every rule kind states its requirement, because a guide that quietly
    /// omitted one would teach an incomplete rule set.
    #[test]
    fn every_rule_kind_states_its_requirement() {
        let config = config(vec![
            rule(
                "shape",
                None,
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: vec!["types".to_owned()],
                    warn_subfolders: vec!["shared".to_owned()],
                    recurse_into: Vec::new(),
                    filename_patterns: vec![Pattern::compile("^[a-z-]+\\.ts$").expect("valid")],
                },
            ),
            rule("name", None, &["src/*"], naming()),
            rule(
                "spec",
                None,
                &["src/*"],
                CompiledRuleKind::SpecPair {
                    subfolders: vec![".".to_owned()],
                    spec_markers: vec!["spec".to_owned(), "test".to_owned()],
                    ignore_files: PathSet::default(),
                    require_non_empty_spec: true,
                },
            ),
            rule(
                "boundary",
                None,
                &["src/**"],
                CompiledRuleKind::ImportBoundary {
                    forbid: set(&["src/infra/**"]),
                    require: set(&["src/telemetry/**"]),
                    except: set(&["src/infra/types/**"]),
                    include_type_only: false,
                },
            ),
            rule(
                "audit",
                None,
                &["src/*"],
                CompiledRuleKind::CallObligation {
                    file_pattern: Pattern::compile(r"^route\.post\.ts$").expect("valid"),
                    symbol: "Event.save".to_owned(),
                    imported_from: "@org/domain/event".to_owned(),
                },
            ),
        ]);

        let built = guide(&config, None, &[]);
        for entry in &built.rules {
            assert!(
                !entry.requires.is_empty(),
                "`{}` ({}) says nothing it requires",
                entry.id,
                entry.kind
            );
        }

        let markdown = rendered(&config, None, GuideFormat::Markdown);
        assert!(markdown.contains("subfolders: `types`"), "{markdown}");
        assert!(
            markdown.contains("allowed with a warning: `shared`"),
            "{markdown}"
        );
        assert!(markdown.contains("filenames must match:"), "{markdown}");
        assert!(
            markdown.contains("needs a sibling named with `.spec.` or `.test.`"),
            "{markdown}"
        );
        assert!(
            markdown.contains("containing at least one test case"),
            "{markdown}"
        );
        assert!(
            markdown.contains("must not import from `src/infra/**`, except `src/infra/types/**`"),
            "{markdown}"
        );
        assert!(
            markdown.contains("type-only imports are exempt"),
            "{markdown}"
        );
        assert!(
            markdown.contains("must import from `src/telemetry/**`"),
            "{markdown}"
        );
        assert!(
            markdown.contains("must call `Event.save`, imported from `@org/domain/event`"),
            "{markdown}"
        );
    }

    /// `kind: "any"` asks for no declaration form, so the guide must not
    /// invent one -- an agent taught a constraint that is not there writes to
    /// satisfy a rule nobody set.
    #[test]
    fn any_form_teaches_no_form() {
        let markdown = rendered(
            &config(vec![rule(
                "name",
                None,
                &["src/*"],
                CompiledRuleKind::Naming {
                    file_pattern: Pattern::compile("^(?<name>[a-z]+)\\.ts$").expect("valid"),
                    name_template: "{{pascal(name)}}".to_owned(),
                    kind: KindFilter::Any,
                    signature_hint: None,
                },
            )]),
            None,
            GuideFormat::Markdown,
        );

        assert!(
            markdown.contains("must export `{{pascal(name)}}`"),
            "{markdown}"
        );
        assert!(!markdown.contains("declared as"), "{markdown}");
    }

    /// The guide points at the commands that answer precisely, because a
    /// digest is a summary and an agent should know where to ask.
    #[test]
    fn the_guide_points_at_describe_and_scaffold() {
        let markdown = rendered(
            &config(vec![rule("name", None, &["src/*"], naming())]),
            None,
            GuideFormat::Markdown,
        );

        assert!(
            markdown.contains("archwarden describe <path>"),
            "{markdown}"
        );
        assert!(
            markdown.contains("archwarden scaffold <path>"),
            "{markdown}"
        );
    }
}
