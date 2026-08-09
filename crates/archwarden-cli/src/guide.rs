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
    /// Why the rule exists, when its author said. A digest without them is a
    /// list of prohibitions, which is what an agent works around. Issue #46.
    #[serde(skip_serializing_if = "Option::is_none")]
    why: Option<&'a str>,
    /// Why the module it belongs to exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    module_why: Option<&'a str>,
}

/// Every rule kind archwarden has, as written in a config and on the command
/// line.
///
/// Listed here rather than derived, because `CompiledRuleKind::type_name` maps
/// one way only. A test walks the enum through this list, so the two cannot
/// drift.
pub const KINDS: [&str; 8] = [
    "structure",
    "naming",
    "spec-pair",
    "presence",
    "pair",
    "frontmatter",
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
                why: rule.why.as_deref(),
                module_why: rule.module_why.as_deref(),
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
///
/// One arm per rule kind, each a handful of lines. Splitting it would put the
/// arms somewhere the exhaustive `match` no longer names them, which is the
/// property that makes a kind added without a sentence fail to compile.
#[allow(clippy::too_many_lines, reason = "one arm per rule kind; see above")]
fn requirements(kind: &CompiledRuleKind) -> Vec<String> {
    match kind {
        CompiledRuleKind::Structure {
            allowed_subfolders,
            warn_subfolders,
            subfolder_patterns,
            filename_patterns,
            ..
        } => {
            let mut lines = Vec::new();
            if allowed_subfolders.is_some() || !warn_subfolders.is_empty() {
                let allowed = allowed_subfolders.as_deref().unwrap_or_default();
                let mut line = format!("subfolders: {}", join(allowed));
                if !warn_subfolders.is_empty() {
                    let _ = write!(line, "; allowed with a warning: {}", join(warn_subfolders));
                }
                lines.push(line);
            }
            if !subfolder_patterns.is_empty() {
                lines.push(format!(
                    "subfolder names must match: {}",
                    join(
                        &subfolder_patterns
                            .iter()
                            .map(|p| p.as_str().to_owned())
                            .collect::<Vec<_>>()
                    )
                ));
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
        CompiledRuleKind::NoPassthrough {
            forms,
            allow_package_entrypoints,
            allow_partial,
            ..
        } => {
            let mut shapes = Vec::new();
            if forms.reexport {
                shapes.push("re-exporting it");
            }
            if forms.alias {
                shapes.push("aliasing it");
            }
            if forms.wrapper {
                shapes.push("wrapping it in a one-line function");
            }
            let mut lines = vec![format!(
                "a file must add something of its own: not only {}",
                join(&shapes.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>()),
            )];
            if *allow_package_entrypoints {
                lines.push(
                    "a package entry point is exempt: forwarding is what a public API is for"
                        .to_owned(),
                );
            }
            if !*allow_partial {
                lines.push("forwarding some exports while declaring others counts too".to_owned());
            }
            lines
        }
        CompiledRuleKind::Naming {
            file_pattern,
            dir_pattern,
            name_template,
            kind,
            annotation,
            signature_hint,
        } => {
            // The directory half belongs in the same sentence, not in a note
            // under it. An agent reading "files matching `^(?<action>...)$`
            // must export `{{pascal(entity)}}{{pascal(action)}}`" without being
            // told where `entity` comes from cannot produce the name, and this
            // digest is what it has instead of the config.
            //
            // The annotation belongs in that sentence too, and not in a note
            // under it beside the hint: one of those two is enforced and the
            // other is advice, and a digest that lists them together teaches an
            // agent to treat both as optional.
            let mut lines = vec![format!(
                "files matching `{}`{} must export `{name_template}`{}{}",
                file_pattern.as_str(),
                dir_pattern.as_ref().map_or_else(String::new, |pattern| {
                    format!(", in a directory matching `{}`", pattern.as_str())
                }),
                declared_as(kind),
                annotated_as(annotation),
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
            forbid_packages,
            except,
            except_from,
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
            // The dependency half, in the same voice. An agent about to reach
            // for `three` has to learn it here or not at all: this digest is
            // what it has instead of the config, and a rule the digest omits is
            // a rule it will violate.
            if !forbid_packages.is_empty() {
                let mut line = format!(
                    "must not import the package {} (nor anything under it)",
                    join(forbid_packages)
                );
                if !except_from.is_empty() {
                    let _ = write!(line, "; only {} may", join(except_from.patterns()));
                }
                lines.push(line);
            }
            if !require.is_empty() {
                lines.push(format!("must import from {}", join(require.patterns())));
            }
            lines
        }
        CompiledRuleKind::Frontmatter {
            file_pattern,
            require,
            one_of,
            equals,
        } => {
            let mut lines = vec![format!(
                "documents matching `{}` need frontmatter",
                file_pattern.as_str()
            )];
            if !require.is_empty() {
                let quoted: Vec<String> = require.iter().map(|k| format!("`{k}`")).collect();
                lines.push(format!("carrying: {}", quoted.join(", ")));
            }
            for (key, accepted) in one_of {
                let quoted: Vec<String> = accepted.iter().map(|v| format!("`{v}`")).collect();
                lines.push(format!("`{key}` one of: {}", quoted.join(", ")));
            }
            for (key, template) in equals {
                lines.push(format!("`{key}` equal to `{template}`"));
            }
            lines
        }
        CompiledRuleKind::Pair {
            file_pattern,
            must_exist,
        } => vec![format!(
            "files matching `{}` need `{must_exist}` beside them",
            file_pattern.as_str()
        )],
        CompiledRuleKind::Presence {
            require,
            require_any,
        } => {
            let mut lines = Vec::new();
            if !require.is_empty() {
                // Comma-joined, not `join`: that helper reads "a or b", and
                // every one of these is required.
                let quoted: Vec<String> = require.iter().map(|n| format!("`{n}`")).collect();
                lines.push(format!("must contain: {}", quoted.join(", ")));
            }
            if !require_any.is_empty() {
                lines.push(format!(
                    "at least one file matching: {}",
                    join(
                        &require_any
                            .iter()
                            .map(|p| p.as_str().to_owned())
                            .collect::<Vec<_>>()
                    )
                ));
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

/// The clause naming the type the export must write down, if the rule asks.
///
/// Empty for a rule that asks for none, which keeps the sentence of every rule
/// written before this field existed byte-identical -- `agent-guide` is
/// documented as deterministic and safe to commit, so an unrelated rule
/// growing a clause would show up as a diff in a repository nobody touched.
fn annotated_as(annotation: &[String]) -> String {
    if annotation.is_empty() {
        return String::new();
    }

    format!(", annotated {}", join(annotation))
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
    /// The same content, as a page for somebody about to change the
    /// architecture rather than to satisfy it.
    ///
    /// Config-only, like the other two: this renders what the rules *declare*,
    /// so it stays byte-stable and safe to commit. What the architecture
    /// currently *is* — which walls are being crossed, and how often — needs
    /// the repository, and lives in `check --html`.
    Html,
}

/// Writes the guide.
pub fn render(
    guide: &Guide<'_>,
    format: GuideFormat,
    language: crate::phrases::Language,
    out: &mut dyn std::io::Write,
) {
    match format {
        // Markdown and JSON stay English whatever the language is. One is a
        // digest an agent reads and the other is a contract; see `phrases`.
        GuideFormat::Markdown => render_markdown(guide, out),
        GuideFormat::Json => render_json(guide, out),
        GuideFormat::Html => render_html(guide, language, out),
    }
}

/// The digest as a page: the map, the walls, and the reasons.
///
/// Grouped by module rather than listed by rule, because the reader is holding
/// a mental map of the repository and not a list of ids. Rules that belong to
/// no module come last, under their own heading — import boundaries usually
/// are, and they are the walls between the modules above.
#[allow(
    clippy::too_many_lines,
    reason = "one page, written in the order it is read. Splitting it by \
              section would put the shape of the document behind four \
              signatures, and the shape is the thing under review"
)]
fn render_html(
    guide: &Guide<'_>,
    language: crate::phrases::Language,
    out: &mut dyn std::io::Write,
) {
    use crate::html::{close, code, escape, open, prose, section};

    let say = language.phrases();
    open(say.guide_title(), language, out);

    let modules = grouped_by_module(guide);
    let unattached = guide
        .rules
        .iter()
        .filter(|rule| rule.module.is_none())
        .count();
    let unexplained = guide.rules.iter().filter(|rule| rule.why.is_none()).count();

    // Masthead. The counts are the ones the digest already carries; nothing is
    // derived that `agent-guide --format json` does not also say.
    let _ = write!(
        out,
        "<header class=\"masthead\">\n\
         <div class=\"stamp\">{}</div>\n\
         <h1>{}</h1>\n\
         <div class=\"tallies\">\n\
         <div class=\"tally\"><span class=\"n\">{}</span><span class=\"k\">{}</span></div>\n\
         <div class=\"tally\"><span class=\"n\">{}</span><span class=\"k\">{}</span></div>\n\
         <div class=\"tally\"><span class=\"n\">{unattached}</span><span class=\"k\">{}</span></div>\n\
         <div class=\"tally{}\"><span class=\"n\">{unexplained}</span><span class=\"k\">{}</span></div>\n\
         </div>\n</header>\n",
        escape(say.guide_stamp()),
        escape(&say.guide_heading(guide.rules.len(), modules.len())),
        guide.rules.len(),
        escape(say.tally_rules()),
        modules.len(),
        escape(say.tally_modules()),
        escape(say.tally_cross_module()),
        // The class comes before the label because the format string puts it
        // there: `<div class="tally{}">…<span class="k">{}</span>`. Getting
        // this pair the wrong way round printed `is-accepted` as a label,
        // which is a bug a reader sees and a test does not.
        if unexplained > 0 { " is-accepted" } else { "" },
        escape(say.tally_no_reason()),
    );

    let _ = write!(
        out,
        "{}",
        section(say.map_eyebrow(), say.map_heading(), say.map_lede())
    );

    let _ = writeln!(out, "<div class=\"modules\">\n");
    for (module, rules) in &modules {
        let module_why = rules.iter().find_map(|rule| rule.module_why);
        let _ = write!(
            out,
            "<div class=\"module\">\n<span class=\"name\">{}</span>\n\
             <span class=\"counts\">{}</span>",
            escape(module),
            escape(&say.rules(rules.len())),
        );
        if let Some(why) = module_why {
            let _ = writeln!(out, "<p class=\"why\">{}</p>", escape(why));
        } else {
            let _ = writeln!(
                out,
                "<p class=\"why is-absent\">{}</p>",
                escape(say.no_reason_recorded())
            );
        }
        let _ = writeln!(out, "</div>\n");
    }
    let _ = writeln!(out, "</div>\n</section>");

    let _ = write!(
        out,
        "{}",
        section(say.rules_eyebrow(), say.rules_heading(), say.rules_lede())
    );

    let _ = writeln!(out, "<div class=\"walls\">\n");
    for rule in &guide.rules {
        let _ = write!(
            out,
            "<article class=\"rule\">\n\
             <span class=\"id\">{}</span>\n\
             <span class=\"severity {2}\">{2}</span>\n\
             <span class=\"kind\">{}</span>",
            escape(rule.id),
            escape(rule.kind),
            escape(rule.level),
        );
        let _ = write!(
            out,
            "<ul>\n<li>{}</li>",
            say.applies_to(
                &rule
                    .applies_to
                    .iter()
                    .map(|glob| code(glob))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        );
        for requirement in &rule.requires {
            let _ = writeln!(out, "<li>{}</li>", prose(requirement));
        }
        let _ = writeln!(out, "</ul>");
        if let Some(why) = rule.why {
            let _ = writeln!(out, "<p class=\"why\">{}</p>", escape(why));
        }
        let _ = writeln!(out, "</article>\n");
    }
    let _ = writeln!(out, "</div>\n</section>");

    let _ = write!(
        out,
        "<footer><span>archwarden {}</span>\n\
         <span>{} <code>archwarden check --html</code></span>\n\
         <span>{}</span></footer>",
        escape(env!("CARGO_PKG_VERSION")),
        escape(say.guide_footer()),
        escape(say.read_only()),
    );

    close(out);
}

/// The rules of each module, in configuration order, modules in name order.
fn grouped_by_module<'a>(guide: &'a Guide<'a>) -> Vec<(&'a str, Vec<&'a GuideRule<'a>>)> {
    let mut by_module: std::collections::BTreeMap<&str, Vec<&GuideRule<'_>>> =
        std::collections::BTreeMap::new();

    for rule in &guide.rules {
        // A rule declared at the top level belongs to no module and reports as
        // `[*]` everywhere else, so it is grouped under the same name here.
        by_module
            .entry(rule.module.unwrap_or("*"))
            .or_default()
            .push(rule);
    }

    by_module.into_iter().collect()
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
        // Last, and on its own line: the requirements are what to do, this is
        // why. A digest of prohibitions with no reasons is what an agent works
        // around. Issue #46.
        if let Some(why) = rule.why {
            let _ = writeln!(out, "- **Why**: {why}");
        }
        if let Some(why) = rule.module_why {
            let _ = writeln!(out, "- **Why this module**: {why}");
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
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            annotation: Vec::new(),
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
        render(
            &guide(config, scope, &owned),
            format,
            crate::phrases::Language::En,
            &mut out,
        );
        String::from_utf8(out).expect("output is UTF-8")
    }

    fn boundary() -> CompiledRuleKind {
        CompiledRuleKind::ImportBoundary {
            forbid: set(&["src/infra/**"]),
            require: PathSet::default(),
            forbid_packages: Vec::new(),
            except: PathSet::default(),
            except_from: PathSet::default(),
            include_type_only: true,
        }
    }

    /// A boundary about a dependency rather than a layer.
    fn package_boundary(except_from: &[&str]) -> CompiledRuleKind {
        CompiledRuleKind::ImportBoundary {
            forbid: PathSet::default(),
            require: PathSet::default(),
            forbid_packages: vec!["three".to_owned()],
            except: PathSet::default(),
            except_from: set(except_from),
            include_type_only: true,
        }
    }

    /// The digest is what an agent has instead of the config, so a rule it
    /// omits is a rule the agent will violate. Issue #14's angle: the rule
    /// used to live in a second config file archwarden never read.
    #[test]
    fn the_guide_names_a_forbidden_package_and_who_may_import_it() {
        let quarantined = config(vec![rule(
            "three-is-quarantined",
            None,
            &["src/**"],
            package_boundary(&["src/scripts/three/**"]),
        )]);
        let markdown = rendered(&quarantined, None, GuideFormat::Markdown);

        assert!(
            markdown.contains("must not import the package `three` (nor anything under it)"),
            "{markdown}"
        );
        assert!(
            markdown.contains("only `src/scripts/three/**` may"),
            "the one directory allowed is the half an agent needs most: {markdown}"
        );

        // And with nothing exempt, the sentence stops rather than trailing off
        // into an empty list.
        let everywhere = config(vec![rule(
            "no-three",
            None,
            &["src/**"],
            package_boundary(&[]),
        )]);
        let markdown = rendered(&everywhere, None, GuideFormat::Markdown);
        assert!(markdown.contains("(nor anything under it)"), "{markdown}");
        assert!(!markdown.contains("only "), "{markdown}");
    }

    /// Every label on the page is a label, and not a CSS class that landed in
    /// the wrong hole.
    ///
    /// The masthead interleaves counts, classes and labels in one format
    /// string, and one pair the wrong way round printed `is-accepted` where a
    /// reader expected a word. It compiled, and nothing else noticed.
    #[test]
    fn no_tally_label_is_a_class_name() {
        let html = rendered(
            &config(vec![rule("usecase-name", None, &["src/*"], naming())]),
            None,
            GuideFormat::Html,
        );

        for label in html.split("class=\"k\">").skip(1) {
            let label = label.split('<').next().unwrap_or_default();
            assert!(
                !label.contains("is-"),
                "`{label}` is a class, not a label: {html}"
            );
            assert!(!label.trim().is_empty(), "an empty label: {html}");
        }
    }

    /// The digest is what an agent has *instead of* the config, so a
    /// requirement missing from it is a requirement the agent will break and
    /// then be told about. The annotation is checked, so it belongs in the
    /// sentence rather than under it as a suggestion.
    /// The human rendering of the same digest. `describe` and the JSON are for
    /// an agent; this is for somebody about to change the architecture, who
    /// wants the walls and the reasons in one place.
    #[test]
    fn the_html_page_carries_the_modules_the_walls_and_the_reasons() {
        let mut boundary_rule = rule(
            "domain-forbids-app",
            Some("domain"),
            &["packages/domain/**"],
            boundary(),
        );
        boundary_rule.why =
            Some("domain is published as its own package and the app is not".to_owned());
        boundary_rule.module_why = Some("extracted so billing could depend on it".to_owned());

        let html = rendered(&config(vec![boundary_rule]), None, GuideFormat::Html);

        assert!(html.starts_with("<!doctype html>"), "{html}");
        assert!(html.contains("domain-forbids-app"), "the rule id");
        assert!(html.contains("packages/domain/**"), "the scope it governs");
        assert!(
            html.contains("domain is published as its own package"),
            "the reason, which is the whole point"
        );
        assert!(
            html.contains("extracted so billing could depend on it"),
            "the module's own reason"
        );
    }

    /// The page is a rendering of the digest, and the digest is documented as
    /// byte-stable and safe to commit. The page has to be too, or a repository
    /// that keeps one gets a diff every run.
    #[test]
    fn the_html_page_is_byte_stable() {
        let config = config(vec![rule("usecase-name", None, &["src/*"], naming())]);

        assert_eq!(
            rendered(&config, None, GuideFormat::Html),
            rendered(&config, None, GuideFormat::Html)
        );
    }

    /// A reason is prose somebody wrote, and the likeliest string in the whole
    /// config to contain a character that would close the element it sits in.
    #[test]
    fn a_reason_that_looks_like_markup_does_not_break_the_page() {
        let mut reasoned = rule("layout-rule", None, &["src/*"], naming());
        reasoned.why = Some("every page renders inside <Layout />".to_owned());

        let html = rendered(&config(vec![reasoned]), None, GuideFormat::Html);

        assert!(html.contains("&lt;Layout /&gt;"), "{html}");
        assert!(!html.contains("<Layout />"), "it got out of its element");
    }

    /// Issue #44. The digest is what an agent has before it writes a document,
    /// and the frontmatter is the half a human never reads.
    #[test]
    fn a_frontmatter_rule_lists_its_keys_and_vocabularies() {
        let config = config(vec![rule(
            "projeto-frontmatter",
            None,
            &["projetos/*"],
            CompiledRuleKind::Frontmatter {
                file_pattern: Pattern::compile(r"^projeto\.md$").expect("valid"),
                require: vec!["id".to_owned(), "nivel".to_owned()],
                one_of: vec![("nivel".to_owned(), vec!["1".to_owned(), "2".to_owned()])],
                equals: vec![("id".to_owned(), "{{raw(dirname)}}".to_owned())],
            },
        )]);

        let markdown = rendered(&config, None, GuideFormat::Markdown);

        assert!(
            markdown.contains(r"documents matching `^projeto\.md$` need frontmatter"),
            "{markdown}"
        );
        assert!(markdown.contains("carrying: `id`, `nivel`"), "{markdown}");
        assert!(markdown.contains("`nivel` one of: `1`, `2`"), "{markdown}");
        assert!(
            markdown.contains("`id` equal to `{{raw(dirname)}}`"),
            "{markdown}"
        );
    }

    /// Issue #45. The digest has to say which of the two files needs the
    /// other, because the rule is one-directional and an agent that got it
    /// backwards would create the wrong file.
    #[test]
    fn a_pair_rule_says_which_file_needs_which() {
        let config = config(vec![rule(
            "licao-tem-notas",
            None,
            &["projetos/*"],
            CompiledRuleKind::Pair {
                file_pattern: Pattern::compile(r"^projeto\.md$").expect("valid"),
                must_exist: "notas.md".to_owned(),
            },
        )]);

        let markdown = rendered(&config, None, GuideFormat::Markdown);

        assert!(
            markdown.contains(r"files matching `^projeto\.md$` need `notas.md` beside them"),
            "{markdown}"
        );
    }

    /// Issue #42. The digest teaches the rules before a question is asked, and
    /// "a lesson directory has these four files" is the one an agent most needs
    /// before creating one.
    #[test]
    fn a_presence_rule_lists_what_must_exist() {
        let config = config(vec![rule(
            "licao-completa",
            None,
            &["projetos/*"],
            CompiledRuleKind::Presence {
                require: vec!["projeto.md".to_owned(), "notas.md".to_owned()],
                require_any: vec![Pattern::compile(r"\.ino$").expect("valid")],
            },
        )]);

        let markdown = rendered(&config, None, GuideFormat::Markdown);

        assert!(
            markdown.contains("must contain: `projeto.md`, `notas.md`"),
            "{markdown}"
        );
        assert!(
            markdown.contains(r"at least one file matching: `\.ino$`"),
            "{markdown}"
        );
    }

    /// Issue #46. The digest is a list of prohibitions without them, and a
    /// list of prohibitions is what an agent works around.
    #[test]
    fn a_rules_reason_is_part_of_the_digest() {
        let mut reasoned = rule("usecase-name", None, &["src/*"], naming());
        reasoned.why = Some("the loader finds these by readdir".to_owned());
        let config = config(vec![reasoned]);

        let markdown = rendered(&config, None, GuideFormat::Markdown);

        assert!(
            markdown.contains("**Why**: the loader finds these by readdir"),
            "{markdown}"
        );
    }

    /// A digest is what an agent has instead of the config, and "the folder
    /// name has to look like this" is exactly what it needs before creating
    /// one.
    #[test]
    fn a_subfolder_pattern_appears_in_the_digest() {
        let by_shape = config(vec![rule(
            "licao-nome-da-pasta",
            None,
            &["projetos"],
            CompiledRuleKind::Structure {
                allowed_subfolders: None,
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: vec![Pattern::compile(r"^\d{2}-[a-z0-9-]+$").expect("valid")],
                filename_patterns: Vec::new(),
            },
        )]);

        let markdown = rendered(&by_shape, None, GuideFormat::Markdown);

        assert!(
            markdown.contains(r"subfolder names must match: `^\d{2}-[a-z0-9-]+$`"),
            "{markdown}"
        );
    }

    #[test]
    fn a_required_annotation_is_part_of_the_sentence() {
        let annotated = CompiledRuleKind::Naming {
            file_pattern: Pattern::compile(r"^(?<tool>[a-z0-9-]+)\.tool\.ts$").expect("valid"),
            dir_pattern: None,
            name_template: "AGENT_TOOL".to_owned(),
            kind: KindFilter::OneOf(ExportTags::only(ExportKind::Const)),
            annotation: vec!["AgentToolModule".to_owned()],
            signature_hint: None,
        };
        let config = config(vec![rule("tools", None, &["src/*"], annotated)]);

        let markdown = rendered(&config, None, GuideFormat::Markdown);

        assert!(
            markdown.contains("annotated `AgentToolModule`"),
            "{markdown}"
        );
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
                    forbid_packages: Vec::new(),
                    except: PathSet::default(),
                    except_from: PathSet::default(),
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
                    allowed_subfolders: Some(Vec::new()),
                    warn_subfolders: vec!["shared".to_owned()],
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
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
                    // Absent, not empty: after issue #40 an empty list is a
                    // constraint -- "no subfolder may exist here" -- and the
                    // digest has to say so.
                    allowed_subfolders: None,
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
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
                    allowed_subfolders: Some(vec!["types".to_owned()]),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
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
                    allowed_subfolders: Some(vec!["types".to_owned()]),
                    warn_subfolders: vec!["shared".to_owned()],
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
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
                    skip_type_only: false,
                },
            ),
            rule(
                "boundary",
                None,
                &["src/**"],
                CompiledRuleKind::ImportBoundary {
                    forbid: set(&["src/infra/**"]),
                    require: set(&["src/telemetry/**"]),
                    forbid_packages: Vec::new(),
                    except: set(&["src/infra/types/**"]),
                    except_from: PathSet::default(),
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
                    dir_pattern: None,
                    name_template: "{{pascal(name)}}".to_owned(),
                    kind: KindFilter::Any,
                    annotation: Vec::new(),
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
