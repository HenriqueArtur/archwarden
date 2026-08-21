//! `archwarden agent-guide` — writing the digest out.
//!
//! The digest moved to [`archwarden_api::guide`] in 0.18. What is left here is
//! the three renderings and the flag that picks between them: markdown for an
//! agent's context file, JSON for a program, and a page for somebody about to
//! change the architecture rather than to satisfy it.
//!
//! `GuideFormat` stays here because it carries `clap::ValueEnum`, and the page
//! stays because it carries the phrase tables the config's `language` selects.
//! Both are this surface's own, on the argument decision 20 already made about
//! `LevelFilter`.

use archwarden_api::guide::{Guide, GuideRule, join};

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

/// What to say when the digest has nothing in it.
///
/// Three different states, and they were one sentence until 0.20. A repository
/// with nine rules and none of the kind you asked about was told "No rules are
/// configured", which is false and reads as *this kind does not exist* — the
/// reading issue #97 reported getting. Two answers that differ have to differ
/// out loud; it is the same distinction the pre-write hook draws between "I
/// have no objection" and "I could not tell".
fn nothing_to_show(guide: &Guide<'_>) -> String {
    // Narrowed to a kind, and it is a kind archwarden has -- an unknown one is
    // refused before a digest is built. So the honest sentence is that you have
    // none of it, and the next question is what one would take.
    if let Some(first) = guide.kinds.first() {
        let named = archwarden_api::describe::join_or(&guide.kinds, "any kind");
        let under = guide
            .scope
            .map_or_else(String::new, |scope| format!(" under `{scope}`"));
        return format!(
            "No {named} rules are declared{under}.\n\nRun `archwarden config options \
             {first}` for what one takes."
        );
    }

    match guide.scope {
        Some(scope) => format!("No rules reach `{scope}`."),
        None => "No rules are configured.".to_owned(),
    }
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
         {}</div>\n</header>\n",
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
        // Only when there are any. A fifth tally reading `0 decisions` would
        // appear in every committed page on upgrade — a diff nobody made, over
        // a feature they have not adopted. Issue #100.
        if guide.decisions.is_empty() {
            String::new()
        } else {
            format!(
                "<div class=\"tally\"><span class=\"n\">{}</span><span class=\"k\">{}</span></div>\n",
                guide.decisions.len(),
                escape(say.tally_decisions()),
            )
        },
    );

    // What was decided, before the map and before the walls: those two are
    // how the decisions are kept, and a page that opened with them would be
    // the rule table this section exists to replace. Issue #100.
    if !guide.decisions.is_empty() {
        let _ = write!(
            out,
            "{}",
            section(
                say.decisions_eyebrow(),
                say.decisions_heading(),
                say.decisions_lede()
            )
        );

        let _ = writeln!(out, "<div class=\"decisions\">\n");
        for decision in &guide.decisions {
            // The chain, not just the flag: a reader told to stop trusting a
            // decision needs to be told where to go instead. Issue #115.
            let status = match (decision.status, decision.superseded_by.first()) {
                ("accepted", _) => String::new(),
                (_, Some(by)) => format!(
                    "\n<span class=\"status\">{}</span>",
                    escape(&say.superseded_by(by))
                ),
                (other, None) => format!(
                    "\n<span class=\"status\">{}</span>",
                    escape(&say.decision_status(other))
                ),
            };
            let _ = write!(
                out,
                "<article class=\"decision\">\n\
                 <span class=\"id\">{}</span>{status}\n\
                 <h3>{}</h3>",
                escape(decision.id),
                escape(decision.title),
            );
            if let Some(why) = decision.why {
                let _ = writeln!(out, "<p class=\"why\">{}</p>", escape(why));
            }
            if let Some(link) = decision.link {
                let _ = writeln!(
                    out,
                    "<p class=\"link\">{} {}</p>",
                    escape(say.written_down_in()),
                    code(link)
                );
            }
            // The measurement, above the rule list rather than below it: a
            // reader who stops at "Enforced by two rules" has been told this
            // decision is kept, and the number is what says whether it is.
            // Absent when no baseline was read, and absent at zero -- a
            // decision carrying no debt has nothing to say here, and "0
            // excused" on every card is a column of noise. Issue #112.
            if !decision.supersedes.is_empty() {
                let _ = writeln!(
                    out,
                    "<p class=\"replaces\">{}</p>",
                    escape(&say.replaces(&decision.supersedes.join(", ")))
                );
            }
            if let Some(excused) = decision.excused.filter(|excused| *excused > 0) {
                let _ = writeln!(
                    out,
                    "<p class=\"excused\">{}</p>",
                    escape(&say.debt_against(excused))
                );
            }
            // The debt, in words. An empty list here is the one thing on this
            // page worth stopping at, and a blank does not stop anybody.
            if decision.rules.is_empty() {
                let _ = writeln!(
                    out,
                    "<p class=\"enforced is-absent\">{}</p>",
                    escape(say.enforced_by_nothing())
                );
            } else {
                let _ = writeln!(
                    out,
                    "<p class=\"enforced\">{}</p>",
                    say.enforced_by(
                        &decision
                            .rules
                            .iter()
                            .map(|id| code(id))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                );
            }
            // What was weighed and lost, and which half of it has teeth. The
            // page's most honest line: an option with a rule is mechanically
            // refused, one without is written down and nothing stops anybody
            // taking it. Issue #114.
            if !decision.alternatives.is_empty() {
                let _ = writeln!(
                    out,
                    "<p class=\"rejected-k\">{}</p>\n<ul class=\"rejected\">",
                    escape(say.rejected_heading())
                );
                for alternative in &decision.alternatives {
                    let refused = match alternative.refused_by {
                        Some(rule) => (say.refused_by(&code(rule)), "is-refused"),
                        None => (escape(say.refused_by_nothing()), "is-absent"),
                    };
                    let _ = writeln!(
                        out,
                        "<li><span class=\"opt\">{}</span>                          <span class=\"tag {}\">{}</span>\n                         <span class=\"whynot\">{}</span></li>",
                        escape(alternative.option),
                        refused.1,
                        refused.0,
                        escape(alternative.why_not),
                    );
                }
                let _ = writeln!(out, "</ul>");
            }
            let _ = writeln!(out, "</article>\n");
        }
        let _ = writeln!(out, "</div>\n</section>");
    }

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
        let _ = writeln!(out, "{}", nothing_to_show(guide));
        return;
    }

    // The decisions first, with the rules that serve each under it. A digest
    // that opens with a flat list of prohibitions is a list an agent works
    // around; one that opens with what was decided and what enforces it is one
    // it can argue with. Issue #100.
    if !guide.decisions.is_empty() {
        let _ = writeln!(out, "## Decisions\n");
        for decision in &guide.decisions {
            let status = if decision.status == "accepted" {
                String::new()
            } else {
                format!(" _({})_", decision.status)
            };
            let _ = writeln!(out, "### {}{status} — {}\n", decision.id, decision.title);
            if let Some(why) = decision.why {
                let _ = writeln!(out, "{why}\n");
            }
            if let Some(link) = decision.link {
                let _ = writeln!(out, "- **Written down in**: {link}");
            }
            // The digest is what an agent has in context before it changes
            // anything, and a decision carrying debt is one it must not add
            // to. Absent at zero and absent with no baseline, for the reason
            // the page leaves it out: a line saying nothing is a line read
            // every time. Issue #112.
            if let Some(entries) = decision.excused.filter(|entries| *entries > 0) {
                let _ = writeln!(out, "- **Baseline entries against it**: {entries}");
            }
            if !decision.supersedes.is_empty() {
                let _ = writeln!(out, "- **Replaces**: {}", decision.supersedes.join(", "));
            }
            if !decision.superseded_by.is_empty() {
                let _ = writeln!(
                    out,
                    "- **Superseded by**: {}",
                    decision.superseded_by.join(", ")
                );
            }
            // The options this decision already weighed, so an agent reading
            // the digest does not propose one of them back. Issue #114.
            if !decision.alternatives.is_empty() {
                let _ = writeln!(out, "\n**Considered and rejected**:\n");
                for alternative in &decision.alternatives {
                    let refused = match alternative.refused_by {
                        Some(rule) => format!("refused by `{rule}`"),
                        None => "nothing refuses it".to_owned(),
                    };
                    let _ = writeln!(
                        out,
                        "- **{}** ({refused}) — {}",
                        alternative.option, alternative.why_not
                    );
                }
                let _ = writeln!(out);
            }
            // Said out loud when it is empty, because a decision nothing
            // enforces is the thing worth noticing here.
            let _ = writeln!(
                out,
                "- **Enforced by**: {}\n",
                if decision.rules.is_empty() {
                    "nothing".to_owned()
                } else {
                    decision
                        .rules
                        .iter()
                        .map(|id| format!("`{id}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
        }
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
        // The id only: the prose is under `## Decisions` above, and repeating
        // it under each of eight rules is eight places for it to be edited.
        if let Some(decision) = rule.decision {
            let _ = writeln!(out, "- **Decision**: {decision}");
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
    use archwarden_api::guide::guide;
    use archwarden_core::{
        compiled::{
            CompiledConfig, CompiledDecision, CompiledRule, CompiledRuleKind, DecisionStatus,
            SkipDirs,
        },
        facts::{ExportKind, ExportTags, KindFilter},
        glob::PathSet,
        hash::ContentHash,
        ids::{DecisionId, ModuleId, RuleId},
        level::Level,
        path::RepoRelPath,
        pattern::Pattern,
        scope::Scope,
    };

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
            ignore_files: archwarden_core::glob::PathSet::default(),
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
        rendered_with_baseline(config, scope, kinds, format, None)
    }

    fn rendered_with_baseline(
        config: &CompiledConfig,
        scope: Option<&RepoRelPath>,
        kinds: &[&str],
        format: GuideFormat,
        baseline: Option<&archwarden_api::baseline::Baseline>,
    ) -> String {
        let owned: Vec<String> = kinds.iter().map(|k| (*k).to_owned()).collect();
        let mut out = Vec::new();
        render(
            &guide(config, scope, &owned, baseline),
            format,
            crate::phrases::Language::En,
            &mut out,
        );
        String::from_utf8(out).expect("output is UTF-8")
    }

    fn boundary() -> CompiledRuleKind {
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
            include_type_only: true,
        }
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
                    groups: Vec::new(),
                    allow: None,
                    allow_packages: None,
                    require: PathSet::default(),
                    forbid_packages: Vec::new(),
                    forbid_reaching: PathSet::default(),
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

    /// An empty configuration says so rather than emitting a heading and
    /// nothing under it.
    #[test]
    fn an_empty_configuration_says_so() {
        let markdown = rendered(&config(Vec::new()), None, GuideFormat::Markdown);
        assert!(markdown.contains("No rules are configured."), "{markdown}");
    }

    /// Issue #97's smaller half. A repository with rules, asked about a kind it
    /// has none of, was told it had no rules at all — which is false, and reads
    /// as "this kind does not exist" rather than "you have none".
    #[test]
    fn a_kind_with_none_declared_is_not_an_empty_repository() {
        let configured = config(vec![rule("usecase-name", None, &["src/*"], naming())]);

        let markdown = rendered_of(
            &configured,
            None,
            &["call-obligation"],
            GuideFormat::Markdown,
        );

        assert!(
            markdown.contains("No `call-obligation` rules are declared."),
            "{markdown}"
        );
        assert!(
            !markdown.contains("No rules are configured"),
            "the false half is gone: {markdown}"
        );
        // And it says where to learn the shape, which is the question somebody
        // asking this is about to have.
        assert!(
            markdown.contains("config options call-obligation"),
            "{markdown}"
        );
    }

    /// A scope that reaches nothing is its own answer again, and not the same
    /// one as an empty repository.
    #[test]
    fn a_scope_that_reaches_nothing_says_that_rather_than_nothing_exists() {
        let configured = config(vec![rule("usecase-name", None, &["src/*"], naming())]);
        let elsewhere = RepoRelPath::new("packages/outro").expect("valid path");

        let markdown = rendered(&configured, Some(&elsewhere), GuideFormat::Markdown);

        assert!(
            markdown.contains("No rules reach `packages/outro`."),
            "{markdown}"
        );
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

    /// Issue #100's shape for the page: the architecture as decisions, not as
    /// a rule table. The decisions come before the module map, because they
    /// are the statement the map and the walls are both serving.
    #[test]
    fn the_page_leads_with_the_decisions() {
        let page = rendered(&decided_config(), None, GuideFormat::Html);

        assert!(
            page.contains("The decisions, and what enforces them"),
            "{page}"
        );
        assert!(page.contains("ADR-014"), "{page}");
        assert!(
            page.contains("The domain does not know about transport"),
            "{page}"
        );
        assert!(page.contains("docs/adr/014.md"), "{page}");
        assert!(
            page.contains("domain-forbids-http"),
            "the rules under it are named: {page}"
        );
        assert!(
            page.contains("<span class=\"k\">decisions</span>"),
            "and the masthead counts them: {page}"
        );

        // The rendered eyebrow, not the bare phrase: the stylesheet carries
        // `/* ---- the map ---- */` as a section comment, so searching for the
        // words alone finds the CSS rather than the page.
        let decisions = page
            .find("<div class=\"eyebrow\">what was decided</div>")
            .expect("the decisions section");
        let map = page
            .find("<div class=\"eyebrow\">the map</div>")
            .expect("the module map");
        assert!(decisions < map, "the decisions come first: {page}");
    }

    /// Issue #112. A decision named by rules that report nothing reads as kept
    /// on this page, and a repository that excuses all of it has never kept it.
    /// The count is what turns the card from a claim into a measurement.
    #[test]
    fn the_page_says_how_much_of_a_decision_is_still_excused() {
        let baseline: archwarden_api::baseline::Baseline =
            serde_json::from_value(serde_json::json!({
                "version": 0,
                "accepted": [
                    { "rule": "domain-forbids-http", "path": "packages/domain/a.ts", "note": "" },
                    { "rule": "domain-forbids-http", "path": "packages/domain/b.ts", "note": "" },
                ],
            }))
            .expect("a baseline");

        let page = rendered_with_baseline(
            &decided_config(),
            None,
            &[],
            GuideFormat::Html,
            Some(&baseline),
        );

        assert!(
            page.contains("<p class=\"excused\">The baseline carries 2 entries against it.</p>"),
            "{page}"
        );
    }

    /// The digest an agent is handed carries it too: a decision with debt is
    /// one it must not add to, and that is the most actionable thing on the
    /// card.
    #[test]
    fn the_digest_carries_the_debt_a_decision_holds() {
        let baseline: archwarden_api::baseline::Baseline =
            serde_json::from_value(serde_json::json!({
                "version": 0,
                "accepted": [
                    { "rule": "domain-forbids-http", "path": "packages/domain/a.ts", "note": "" },
                ],
            }))
            .expect("a baseline");

        let markdown = rendered_with_baseline(
            &decided_config(),
            None,
            &[],
            GuideFormat::Markdown,
            Some(&baseline),
        );

        assert!(
            markdown.contains("- **Baseline entries against it**: 1"),
            "{markdown}"
        );
    }

    /// A config whose decision replaced another and rejected two options, one
    /// of which a rule refuses.
    fn with_history() -> CompiledConfig {
        let mut serving = rule(
            "domain-forbids-http",
            None,
            &["packages/domain/**"],
            boundary(),
        );
        serving.decision = Some(DecisionId::new("ADR-031").expect("valid"));

        config(vec![serving]).with_decisions(vec![
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-009").expect("valid"),
                title: "The old way".to_owned(),
                why: None,
                link: None,
                status: DecisionStatus::Superseded,
                supersedes: Vec::new(),
                superseded_by: vec![DecisionId::new("ADR-031").expect("valid")],
                alternatives: Vec::new(),
            },
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-031").expect("valid"),
                title: "The domain does not know about transport".to_owned(),
                why: None,
                link: None,
                status: DecisionStatus::Accepted,
                supersedes: vec![DecisionId::new("ADR-009").expect("valid")],
                superseded_by: Vec::new(),
                alternatives: vec![
                    archwarden_core::compiled::CompiledAlternative {
                        option: "an HTTP client in the domain".to_owned(),
                        why_not: "a consumer would inherit our transport".to_owned(),
                        refused_by: Some(RuleId::new("domain-forbids-http").expect("valid")),
                    },
                    archwarden_core::compiled::CompiledAlternative {
                        option: "a shared kernel".to_owned(),
                        why_not: "it becomes the place everything goes".to_owned(),
                        refused_by: None,
                    },
                ],
            },
        ])
    }

    /// Issue #114. The page's most honest line: which of the rejected options
    /// has teeth, and which is only written down.
    #[test]
    fn the_page_separates_a_rejection_with_teeth_from_one_without() {
        let page = rendered(&with_history(), None, GuideFormat::Html);

        assert!(page.contains("Considered and rejected"), "{page}");
        assert!(
            page.contains(
                r#"<span class="tag is-refused">refused by <code>domain-forbids-http</code></span>"#
            ),
            "{page}"
        );
        assert!(
            page.contains(r#"<span class="tag is-absent">nothing refuses it</span>"#),
            "an option nothing stops is the one somebody takes: {page}"
        );
        assert!(
            page.contains("a consumer would inherit our transport"),
            "the argument travels with the option: {page}"
        );
    }

    /// Issue #115. The chain, both ways, because a reader arrives from either
    /// end.
    #[test]
    fn the_page_draws_the_supersession_chain() {
        let page = rendered(&with_history(), None, GuideFormat::Html);

        assert!(
            page.contains(r#"<span class="status">superseded by ADR-031</span>"#),
            "not just that it was replaced, but by what: {page}"
        );
        assert!(
            page.contains(r#"<p class="replaces">Replaces ADR-009</p>"#),
            "{page}"
        );
    }

    /// And the digest an agent reads carries both, so it does not propose an
    /// option this architecture already weighed and turned down.
    #[test]
    fn the_digest_carries_what_was_rejected_and_what_was_replaced() {
        let markdown = rendered(&with_history(), None, GuideFormat::Markdown);

        assert!(
            markdown.contains(
                "- **an HTTP client in the domain** (refused by `domain-forbids-http`) \
                 — a consumer would inherit our transport"
            ),
            "{markdown}"
        );
        assert!(
            markdown.contains("- **a shared kernel** (nothing refuses it)"),
            "{markdown}"
        );
        assert!(markdown.contains("- **Replaces**: ADR-009"), "{markdown}");
        assert!(
            markdown.contains("- **Superseded by**: ADR-031"),
            "{markdown}"
        );
    }

    /// A decision the baseline carries nothing against says nothing, rather
    /// than printing "0 entries" on every card of every clean repository.
    #[test]
    fn a_decision_with_no_debt_says_nothing_about_it() {
        let baseline: archwarden_api::baseline::Baseline =
            serde_json::from_value(serde_json::json!({
                "version": 0,
                "accepted": [
                    { "rule": "some-other-rule", "path": "packages/app/a.ts", "note": "" },
                ],
            }))
            .expect("a baseline");

        for format in [GuideFormat::Html, GuideFormat::Markdown] {
            let rendered =
                rendered_with_baseline(&decided_config(), None, &[], format, Some(&baseline));
            assert!(
                !rendered.contains("<p class=\"excused\">")
                    && !rendered.contains("**Baseline entries against it**"),
                "a consulted baseline that holds nothing says nothing: {rendered}"
            );
        }
    }

    /// And a page rendered without one says nothing about it, rather than
    /// showing a zero that reads as "this decision is clean"."""
    ///
    /// Asserted against the rendered element rather than the word: the
    /// stylesheet carries `.decision .excused` and a section comment that says
    /// it, so searching for the word finds the CSS on every page.
    #[test]
    fn a_page_with_no_baseline_says_nothing_about_excused_debt() {
        let page = rendered(&decided_config(), None, GuideFormat::Html);

        assert!(!page.contains("<p class=\"excused\">"), "{page}");
    }

    /// A decision nothing enforces is the state the page exists to make
    /// visible, so it is said in words rather than left as an empty list.
    #[test]
    fn the_page_says_when_nothing_enforces_a_decision() {
        let config = config(vec![rule("shape", None, &["src/*"], naming())]).with_decisions(vec![
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-020").expect("valid"),
                title: "Nobody enforces this".to_owned(),
                why: None,
                link: None,
                status: DecisionStatus::Accepted,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                alternatives: Vec::new(),
            },
        ]);

        assert!(
            rendered(&config, None, GuideFormat::Html).contains("Nothing enforces this."),
            "the page names the debt rather than showing a blank"
        );
    }

    /// A repository with no decisions gets the page it got before 0.21: no
    /// empty section, and no tally counting zero.
    #[test]
    fn a_page_with_no_decisions_grows_no_section() {
        let page = rendered(
            &config(vec![rule("shape", None, &["src/*"], naming())]),
            None,
            GuideFormat::Html,
        );

        // The rendered markup, not the words: the stylesheet always carries
        // `.decisions`, because one sheet serves both pages.
        assert!(
            !page.contains("<div class=\"eyebrow\">what was decided</div>"),
            "{page}"
        );
        assert!(!page.contains("<div class=\"decisions\">"), "{page}");
        assert!(
            !page.contains("<span class=\"k\">decisions</span>"),
            "no fifth tally reading zero: {page}"
        );
    }

    /// The page is the one surface a decision's status is translated on,
    /// because the page is prose. The JSON keeps the English slug.
    #[test]
    fn the_page_translates_a_status_and_the_digest_does_not() {
        let config = config(vec![rule("shape", None, &["src/*"], naming())]).with_decisions(vec![
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-020").expect("valid"),
                title: "Substituída".to_owned(),
                why: None,
                link: None,
                status: DecisionStatus::Superseded,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                alternatives: Vec::new(),
            },
        ]);

        let mut page = Vec::new();
        render(
            &guide(&config, None, &[], None),
            GuideFormat::Html,
            crate::phrases::Language::PtBr,
            &mut page,
        );
        let page = String::from_utf8(page).expect("UTF-8");
        assert!(page.contains("substituída"), "{page}");

        assert!(
            rendered(&config, None, GuideFormat::Json).contains("\"superseded\""),
            "the slug is a stable identifier and stays English in the JSON"
        );
    }

    /// Issue #100. The digest opens with what was decided, and the rules that
    /// serve each are named under it — the ids, because the rule is spelled
    /// out once further down and a digest carrying it twice can disagree with
    /// itself.
    #[test]
    fn the_digest_opens_with_the_decisions_and_what_enforces_them() {
        let markdown = rendered(&decided_config(), None, GuideFormat::Markdown);

        assert!(
            markdown.contains("### ADR-014 — The domain does not know about transport"),
            "{markdown}"
        );
        assert!(markdown.contains("docs/adr/014.md"), "{markdown}");
        assert!(
            markdown.contains("**Enforced by**: `domain-forbids-http`"),
            "{markdown}"
        );
        assert!(
            markdown.contains("**Decision**: ADR-014"),
            "and the rule points back at it: {markdown}"
        );

        let decisions = markdown.find("## Decisions").expect("the section");
        let rules = markdown
            .find("## `domain-forbids-http`")
            .expect("and the rules");
        assert!(decisions < rules, "decisions first: {markdown}");
    }

    /// A decision nothing enforces says so in the word `nothing`, rather than
    /// with an empty line a reader skims past. It is the state worth noticing.
    #[test]
    fn a_decision_nothing_enforces_says_nothing_out_loud() {
        let config = config(vec![rule("shape", None, &["src/*"], naming())]).with_decisions(vec![
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-020").expect("valid"),
                title: "Nobody enforces this".to_owned(),
                why: None,
                link: None,
                status: DecisionStatus::Accepted,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                alternatives: Vec::new(),
            },
        ]);

        let markdown = rendered(&config, None, GuideFormat::Markdown);

        assert!(markdown.contains("**Enforced by**: nothing"), "{markdown}");
    }

    /// A config with no decisions renders exactly the digest it rendered
    /// before 0.21 — which matters more here than anywhere: these files are
    /// committed, and a heading appearing in every repository's guide on
    /// upgrade is a diff nobody made.
    #[test]
    fn a_digest_with_no_decisions_grows_no_section() {
        let markdown = rendered(
            &config(vec![rule("shape", None, &["src/*"], naming())]),
            None,
            GuideFormat::Markdown,
        );

        assert!(!markdown.contains("Decisions"), "{markdown}");
        assert!(!markdown.contains("Decision"), "{markdown}");
    }

    /// A status other than `accepted` is said; `accepted` is not. Every
    /// decision in a healthy repository would otherwise carry the word.
    #[test]
    fn the_digest_says_a_status_only_when_it_is_not_accepted() {
        let markdown = rendered(&decided_config(), None, GuideFormat::Markdown);
        assert!(!markdown.contains("accepted"), "{markdown}");

        let superseded =
            config(vec![rule("shape", None, &["src/*"], naming())]).with_decisions(vec![
                CompiledDecision {
                    scope: None,
                    why_not_enforceable: None,
                    id: DecisionId::new("ADR-020").expect("valid"),
                    title: "Replaced".to_owned(),
                    why: None,
                    link: None,
                    status: DecisionStatus::Superseded,
                    supersedes: Vec::new(),
                    superseded_by: Vec::new(),
                    alternatives: Vec::new(),
                },
            ]);
        assert!(
            rendered(&superseded, None, GuideFormat::Markdown).contains("_(superseded)_"),
            "a decision that no longer holds is said out loud"
        );
    }

    /// A configuration with one decision and one rule serving it.
    fn decided_config() -> CompiledConfig {
        let mut sealed = rule("domain-forbids-http", None, &["src/*"], naming());
        sealed.decision = Some(DecisionId::new("ADR-014").expect("valid"));

        config(vec![sealed]).with_decisions(vec![CompiledDecision {
            scope: None,
            why_not_enforceable: None,
            id: DecisionId::new("ADR-014").expect("valid"),
            title: "The domain does not know about transport".to_owned(),
            why: Some("it is published, and a consumer must not inherit our client".to_owned()),
            link: Some("docs/adr/014.md".to_owned()),
            status: DecisionStatus::Accepted,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            alternatives: Vec::new(),
        }])
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
                        with_options: Vec::new(),
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
}
