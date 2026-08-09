//! `archwarden config explain <rule-id>` — one rule, and what it is doing.
//!
//! `describe` answers "what applies to this path?". This answers the other
//! direction: "what does this rule reach, and what is it reporting?". It is
//! the command for a user who wrote a rule and cannot tell whether it is doing
//! anything, and for an agent that has been handed a rule id in a finding and
//! wants to see the shape of it.
//!
//! The flagged list comes from a real run rather than from a second
//! evaluation, so what it shows is what `check` reports, by construction.

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRule},
    finding::Finding,
    ids::RuleId,
    path::RepoRelPath,
};
use archwarden_engine::walk::RepoTree;
use camino::Utf8Path;
use serde::Serialize;

/// The version of the `explain` JSON shape.
pub const EXPLAIN_VERSION: u32 = 0;

/// What one rule reaches, and what it reports.
pub struct Explanation<'a> {
    /// The rule itself.
    pub rule: &'a CompiledRule,
    /// Every path it has a requirement about, in walk order.
    pub covers: Vec<RepoRelPath>,
    /// How many paths its **scope** selects, requirement or not.
    ///
    /// The difference between the two numbers is the whole diagnosis when
    /// `covers` is empty: a glob that matched nothing and a rule that matched
    /// directories and has nothing to say about them are different faults with
    /// different fixes, and one sentence for both sent users to `config
    /// doctor` for an answer only one of the two has. Issue #41.
    pub scope_reaches: usize,
    /// Every finding it is currently producing.
    pub flags: Vec<Finding>,
}

/// Explains one rule, or says which ids exist.
///
/// # Errors
/// A message listing the configured ids, when `wanted` is not one of them. A
/// typo in a rule id is the likeliest way to reach this, and the list is the
/// answer to it.
pub fn explain<'a>(
    root: &Utf8Path,
    config: &'a CompiledConfig,
    tree: &RepoTree,
    wanted: &RuleId,
) -> Result<Explanation<'a>, String> {
    let Some((rule, engine)) = config
        .rules()
        .zip(archwarden_rules::engines_for(config))
        .find(|(rule, _)| &rule.id == wanted)
    else {
        let ids: Vec<String> = config
            .rules()
            .map(|rule| format!("`{}`", rule.id))
            .collect();
        return Err(if ids.is_empty() {
            format!("no rule is called `{wanted}`; this configuration has no rules")
        } else {
            format!(
                "no rule is called `{wanted}`; the configured rules are {}",
                ids.join(", ")
            )
        });
    };

    // "Covers" means "has a requirement about", which is the same definition
    // `describe` uses. A rule whose scope matches a path it has nothing to say
    // about is not covering it, and listing it here would tell a user their
    // rule reaches further than it does.
    let mut covers = Vec::new();
    let mut scope_reaches = 0;
    for (path, directory) in tree.directories() {
        if !config.is_ignored(path) && rule.scope.matches_dir(path.as_path()) {
            scope_reaches += 1;
        }
        if !config.is_ignored(path) && !engine.describe_expectation(path).is_empty() {
            covers.push(path.clone());
        }
        for file in &directory.files {
            if !config.is_ignored(&file.path) && !engine.describe_expectation(&file.path).is_empty()
            {
                covers.push(file.path.clone());
            }
        }
    }
    covers.sort();

    // From a real run, not a second evaluation: what this shows is what
    // `check` reports, and the two cannot drift.
    let report = archwarden_engine::run::check(archwarden_engine::run::Run {
        root,
        config,
        tree,
        cache: None,
    });
    let flags = report
        .findings
        .into_iter()
        .filter(|finding| &finding.rule_id == wanted)
        .collect();

    Ok(Explanation {
        rule,
        covers,
        scope_reaches,
        flags,
    })
}

/// The JSON envelope.
#[derive(Debug, Serialize)]
struct JsonExplain<'a> {
    version: u32,
    id: &'a str,
    kind: &'static str,
    level: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    module: Option<&'a str>,
    applies_to: &'a [String],
    covers: &'a [RepoRelPath],
    flags: &'a [Finding],
}

/// Writes the explanation.
pub fn render(
    explanation: &Explanation<'_>,
    format: crate::report::Format,
    out: &mut dyn std::io::Write,
) {
    match format {
        crate::report::Format::Text => render_text(explanation, out),
        crate::report::Format::Json => render_json(explanation, out),
    }
}

fn render_json(explanation: &Explanation<'_>, out: &mut dyn std::io::Write) {
    let rule = explanation.rule;
    let envelope = JsonExplain {
        version: EXPLAIN_VERSION,
        id: rule.id.as_str(),
        kind: rule.kind.type_name(),
        level: rule.level.as_str(),
        module: rule
            .module
            .as_ref()
            .map(archwarden_core::ids::ModuleId::as_str),
        applies_to: rule.scope.patterns(),
        covers: &explanation.covers,
        flags: &explanation.flags,
    };

    match serde_json::to_string_pretty(&envelope) {
        Ok(json) => {
            let _ = writeln!(out, "{json}");
        }
        Err(error) => {
            let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
        }
    }
}

fn render_text(explanation: &Explanation<'_>, out: &mut dyn std::io::Write) {
    let rule = explanation.rule;
    let module = rule
        .module
        .as_ref()
        .map_or_else(String::new, |module| format!(" [{module}]"));

    let _ = writeln!(
        out,
        "{} ({}) — {}{module}\n  applies to: {}",
        rule.id,
        rule.kind.type_name(),
        rule.level,
        rule.scope.patterns().join(", "),
    );

    // Under the header, before anything about coverage: a user reading this
    // has a rule that surprised them, and "why does this exist" is the
    // question underneath the one they typed. Issue #46.
    if let Some(why) = &rule.why {
        let _ = writeln!(out, "  why: {why}");
    }
    if let Some(why) = &rule.module_why {
        let _ = writeln!(out, "  module: {why}");
    }

    // Said out loud rather than shown as an empty list. A rule covering
    // nothing is the thing a user runs this command to find out -- and which
    // of the two ways it can happen is the answer, so the two get different
    // sentences and only one of them refers anybody anywhere. Issue #41.
    if explanation.covers.is_empty() {
        let _ = if explanation.scope_reaches == 0 {
            writeln!(
                out,
                "\n  Its scope matches no path in this repository.\n  \
                 Try `archwarden config doctor` for why."
            )
        } else {
            let paths = if explanation.scope_reaches == 1 {
                "1 path".to_owned()
            } else {
                format!("{} paths", explanation.scope_reaches)
            };
            writeln!(
                out,
                "\n  It constrains nothing: its scope reaches {paths}, and the \
                 rule has no requirement about any of them.\n  \
                 Give it something to enforce, or delete it."
            )
        };
        return;
    }

    let _ = writeln!(
        out,
        "\n  Covers {}:",
        count(explanation.covers.len(), "path")
    );
    for path in &explanation.covers {
        let _ = writeln!(out, "    {path}");
    }

    if explanation.flags.is_empty() {
        let _ = writeln!(out, "\n  Flags nothing.");
        return;
    }

    let _ = writeln!(out, "\n  Flags {}:", count(explanation.flags.len(), "path"));
    for finding in &explanation.flags {
        let _ = writeln!(
            out,
            "    {} — {}",
            finding.path,
            crate::report::describe_observed(&finding.observed)
        );
    }
}

fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::{CompiledRuleKind, SkipDirs},
        facts::{ExportKind, ExportTags, KindFilter},
        glob::PathSet,
        hash::ContentHash,
        level::Level,
        pattern::Pattern,
        scope::Scope,
    };
    use camino::Utf8PathBuf;

    fn id(raw: &str) -> RuleId {
        RuleId::new(raw).expect("valid id")
    }

    fn rule(name: &str, scope: &[&str], kind: CompiledRuleKind) -> CompiledRule {
        CompiledRule {
            id: id(name),
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
            ContentHash::of(b"explain"),
        )
    }

    fn naming() -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile(r"^(?<name>[a-z0-9-]+)\.use-case\.ts$")
                .expect("valid pattern"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            annotation: Vec::new(),
            signature_hint: None,
        }
    }

    fn tree_at(entries: &[(&str, &str)]) -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");

        for (relative, contents) in entries {
            let file = root.join(relative);
            std::fs::create_dir_all(file.parent().expect("a file has a parent"))
                .expect("create dirs");
            std::fs::write(&file, contents).expect("write file");
        }

        (dir, root)
    }

    /// Runs `explain` over a temporary repository and renders it.
    fn rendered(
        entries: &[(&str, &str)],
        config: &CompiledConfig,
        wanted: &str,
        format: crate::report::Format,
    ) -> Result<String, String> {
        let (guard, root) = tree_at(entries);
        let tree = archwarden_engine::walk::walk(&root, config).expect("walks");
        let result = explain(&root, config, &tree, &id(wanted)).map(|explanation| {
            let mut out = Vec::new();
            render(&explanation, format, &mut out);
            String::from_utf8(out).expect("output is UTF-8")
        });
        drop(guard);
        result
    }

    const FILES: [(&str, &str); 3] = [
        (
            "src/user/create-client.use-case.ts",
            "export const CreateClient = () => {};",
        ),
        (
            "src/user/delete-client.use-case.ts",
            "export function DeleteClient() {}",
        ),
        ("src/user/helper.ts", "export const helper = 1;"),
    ];

    /// The two questions the command answers: what does this rule reach, and
    /// what is it reporting?
    #[test]
    fn it_lists_what_the_rule_covers_and_what_it_flags() {
        let (guard, root) = tree_at(&FILES);
        let config = config(vec![rule("usecase-name", &["src/*"], naming())]);
        let tree = archwarden_engine::walk::walk(&root, &config).expect("walks");
        let explanation = explain(&root, &config, &tree, &id("usecase-name")).expect("explains");
        drop(guard);

        let covered: Vec<_> = explanation.covers.iter().map(RepoRelPath::as_str).collect();
        assert_eq!(
            covered,
            [
                "src/user/create-client.use-case.ts",
                "src/user/delete-client.use-case.ts"
            ],
            "`helper.ts` does not match the pattern, so the rule does not cover it"
        );

        assert_eq!(explanation.flags.len(), 1);
        assert_eq!(
            explanation.flags[0].path.as_str(),
            "src/user/create-client.use-case.ts",
            "the arrow is the one that breaks the rule"
        );
    }

    /// "Covers" means "has a requirement about", the same definition
    /// `describe` uses. A rule whose scope matches a file it has nothing to
    /// say about is not covering it, and saying otherwise would tell a user
    /// their rule reaches further than it does.
    #[test]
    fn covering_means_having_a_requirement() {
        let text = rendered(
            &FILES,
            &config(vec![rule("usecase-name", &["src/*"], naming())]),
            "usecase-name",
            crate::report::Format::Text,
        )
        .expect("explains");

        assert!(!text.contains("helper.ts"), "{text}");
    }

    /// What it shows is what `check` reports, because it comes from a real
    /// run: another rule's findings are not this rule's.
    #[test]
    fn another_rules_findings_are_not_borrowed() {
        let (guard, root) = tree_at(&FILES);
        let config = config(vec![
            rule("usecase-name", &["src/*"], naming()),
            rule(
                "shape",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec!["types".to_owned()]),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            ),
        ]);
        let tree = archwarden_engine::walk::walk(&root, &config).expect("walks");
        let explanation = explain(&root, &config, &tree, &id("usecase-name")).expect("explains");
        drop(guard);

        assert!(
            explanation
                .flags
                .iter()
                .all(|finding| finding.rule_id.as_str() == "usecase-name"),
            "{:?}",
            explanation.flags
        );
    }

    /// A directory rule covers directories, which is what it has requirements
    /// about — both the directory whose contents it constrains and the ones
    /// whose *names* it constrains.
    ///
    /// `src/user` is covered because the rule says what may live inside it;
    /// `src/user/types` because the rule says its name must be one of the
    /// allowed ones. It satisfies that today, and being governed is not the
    /// same as being in breach. Issue #53 was the other side of this: the
    /// child was governed by `check` and invisible to `describe`.
    #[test]
    fn a_directory_rule_covers_directories() {
        let (guard, root) = tree_at(&[("src/user/types/user.ts", "")]);
        let config = config(vec![rule(
            "shape",
            &["src/*"],
            CompiledRuleKind::Structure {
                allowed_subfolders: Some(vec!["types".to_owned()]),
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: Vec::new(),
                filename_patterns: Vec::new(),
            },
        )]);
        let tree = archwarden_engine::walk::walk(&root, &config).expect("walks");
        let explanation = explain(&root, &config, &tree, &id("shape")).expect("explains");
        drop(guard);

        let covered: Vec<_> = explanation.covers.iter().map(RepoRelPath::as_str).collect();
        assert_eq!(covered, ["src/user", "src/user/types"]);
    }

    /// A rule whose scope reaches no path says so, and points at the command
    /// that explains why. This is the case a user runs `explain` to discover,
    /// and the one `doctor` does have a concern for.
    #[test]
    fn a_rule_whose_scope_reaches_nothing_says_so_and_refers_on() {
        let text = rendered(
            &FILES,
            &config(vec![rule("elsewhere", &["packages/*"], naming())]),
            "elsewhere",
            crate::report::Format::Text,
        )
        .expect("explains");

        assert!(
            text.contains("matches no path in this repository"),
            "{text}"
        );
        assert!(
            text.contains("config doctor"),
            "it points at the why: {text}"
        );
    }

    /// Issue #46. `explain` answers "what is this rule doing", and a reason is
    /// half of that answer -- it is the command a user reaches for when a rule
    /// surprises them, which is exactly when "why does this exist" is the
    /// question.
    #[test]
    fn a_rules_reason_is_printed_with_it() {
        let mut reasoned = rule("shape", &["src/*"], naming());
        reasoned.why = Some("the loader finds these by readdir".to_owned());
        reasoned.module_why = Some("domain is published on its own".to_owned());

        let text = rendered(
            &FILES,
            &config(vec![reasoned]),
            "shape",
            crate::report::Format::Text,
        )
        .expect("explains");

        assert!(
            text.contains("why: the loader finds these by readdir"),
            "{text}"
        );
        assert!(
            text.contains("module: domain is published on its own"),
            "{text}"
        );
    }

    /// Issue #41. The scope matched; what is empty is the rule's own set of
    /// constraints, not the set of paths it reaches. Merging the two into "it
    /// covers nothing" and referring to `doctor` sent a user to a command that
    /// had nothing to say — at exactly the moment they had been told the tool
    /// knew the answer.
    ///
    /// `explain` is the command that decided the rule constrains nothing, so
    /// it is the command that says why.
    #[test]
    fn a_rule_that_reaches_paths_and_constrains_none_of_them_says_why_itself() {
        let text = rendered(
            &FILES,
            &config(vec![rule(
                "toothless",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: None,
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )]),
            "toothless",
            crate::report::Format::Text,
        )
        .expect("explains");

        assert!(text.contains("constrains nothing"), "{text}");
        assert!(
            text.contains("its scope reaches 1 path"),
            "the scope matched, and saying so is the diagnosis: {text}"
        );
        assert!(
            !text.contains("config doctor"),
            "no round trip to a command that would repeat this: {text}"
        );
    }

    /// A rule that covers files and is happy with all of them says that too.
    #[test]
    fn a_rule_that_flags_nothing_says_so() {
        let text = rendered(
            &[(
                "src/user/delete-client.use-case.ts",
                "export function DeleteClient() {}",
            )],
            &config(vec![rule("usecase-name", &["src/*"], naming())]),
            "usecase-name",
            crate::report::Format::Text,
        )
        .expect("explains");

        assert!(text.contains("Covers 1 path"), "{text}");
        assert!(text.contains("Flags nothing."), "{text}");
    }

    /// Naming a rule that is not there -- a typo, or the rule's *kind*
    /// mistaken for its id -- is the likeliest way to reach this error, and
    /// the list of real ids is the answer to it.
    #[test]
    fn an_unknown_id_lists_the_real_ones() {
        let error = rendered(
            &FILES,
            &config(vec![
                rule("usecase-name", &["src/*"], naming()),
                rule("other", &["src/*"], naming()),
            ]),
            "usecase-naming",
            crate::report::Format::Text,
        )
        .expect_err("no such rule");

        assert_eq!(
            error,
            "no rule is called `usecase-naming`; the configured rules are \
             `usecase-name`, `other`"
        );
    }

    /// And a configuration with no rules says that instead of listing nothing.
    #[test]
    fn an_empty_configuration_says_it_has_no_rules() {
        let error = rendered(
            &FILES,
            &config(Vec::new()),
            "anything",
            crate::report::Format::Text,
        )
        .expect_err("no such rule");

        assert_eq!(
            error,
            "no rule is called `anything`; this configuration has no rules"
        );
    }

    /// The header is what an agent reads first: id, kind, level, scope.
    #[test]
    fn the_header_carries_the_rule_itself() {
        let text = rendered(
            &FILES,
            &config(vec![rule("usecase-name", &["src/*"], naming())]),
            "usecase-name",
            crate::report::Format::Text,
        )
        .expect("explains");

        assert!(text.starts_with("usecase-name (naming) — error"), "{text}");
        assert!(text.contains("applies to: src/*"), "{text}");
    }

    /// The JSON an agent consumes, versioned like the other commands.
    #[test]
    fn the_json_shape_is_versioned() {
        let json = rendered(
            &FILES,
            &config(vec![rule("usecase-name", &["src/*"], naming())]),
            "usecase-name",
            crate::report::Format::Json,
        )
        .expect("explains");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(parsed["version"], 0);
        assert_eq!(parsed["id"], "usecase-name");
        assert_eq!(parsed["kind"], "naming");
        assert_eq!(parsed["level"], "error");
        assert_eq!(parsed["applies_to"][0], "src/*");
        assert_eq!(parsed["covers"][0], "src/user/create-client.use-case.ts");
        assert_eq!(parsed["flags"][0]["rule_id"], "usecase-name");
        assert!(parsed["module"].is_null(), "absent rather than null");
    }

    /// Singular and plural, because the count is the line a reader checks.
    #[test]
    fn the_counts_are_pluralised() {
        assert_eq!(count(1, "path"), "1 path");
        assert_eq!(count(2, "path"), "2 paths");
        assert_eq!(count(0, "path"), "0 paths");
    }
}
