//! `archwarden describe <path>` — writing out what applies here.
//!
//! The operation moved to [`archwarden_api::describe`] in 0.18, and what is
//! left here is this surface's half of it: the terminal prose, and the call
//! that turns the shared JSON envelope into bytes.
//!
//! The split is the one [`archwarden_api::render`] already draws for the
//! report. A shape a program consumes is a contract, and MCP has to emit the
//! one `describe --format json` does — so the envelope is built once, in the
//! crate both surfaces depend on. The text is this surface's own: it resolves
//! expectations into sentences through `report::describe_expectation`,
//! for a reader with a terminal.

use archwarden_api::describe::Applies;
use archwarden_core::path::RepoRelPath;

/// Writes what applies, in the requested format.
pub fn render(
    path: &RepoRelPath,
    applies: &[Applies<'_>],
    format: crate::report::Format,
    out: &mut dyn std::io::Write,
) {
    match format {
        crate::report::Format::Text => render_text(path, applies, out),
        crate::report::Format::Json => {
            write_json(&archwarden_api::describe::envelope(path, applies), out);
        }
    }
}

/// Writes what applies across many paths.
///
/// The terminal gets one line per path: the whole point of asking about an
/// area is not to scroll past a block each. The JSON keeps every expectation,
/// because an agent asking about an area still needs the detail it would have
/// got asking one path at a time.
pub fn render_many(
    scope: &str,
    answers: &[(RepoRelPath, Vec<Applies<'_>>)],
    format: crate::report::Format,
    out: &mut dyn std::io::Write,
) {
    match format {
        crate::report::Format::Text => render_many_text(scope, answers, out),
        crate::report::Format::Json => {
            write_json(
                &archwarden_api::describe::envelope_many(scope, answers),
                out,
            );
        }
    }
}

/// Serialises one of the shared envelopes.
///
/// One function for both shapes, because the failure sentence is the same and
/// two copies of it are two copies that drift.
fn write_json(envelope: &impl serde::Serialize, out: &mut dyn std::io::Write) {
    match serde_json::to_string_pretty(envelope) {
        Ok(json) => {
            let _ = writeln!(out, "{json}");
        }
        Err(error) => {
            let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
        }
    }
}

fn render_many_text(
    scope: &str,
    answers: &[(RepoRelPath, Vec<Applies<'_>>)],
    out: &mut dyn std::io::Write,
) {
    // A glob that matched nothing is said out loud. An empty list would read
    // as "every path here is unconstrained", which is a different answer.
    if answers.is_empty() {
        let _ = writeln!(out, "Nothing matches `{scope}`.");
        return;
    }

    let width = answers
        .iter()
        .map(|(path, _)| path.as_str().len())
        .max()
        .unwrap_or(0);

    let _ = writeln!(out, "Rules that apply under `{scope}`:\n");

    let mut distinct: Vec<&str> = Vec::new();
    for (path, applies) in answers {
        let ids: Vec<&str> = applies.iter().map(|entry| entry.rule.id.as_str()).collect();
        for id in &ids {
            if !distinct.contains(id) {
                distinct.push(id);
            }
        }
        // An em dash rather than a blank: a path nothing constrains keeps its
        // line, because dropping it would read as the glob not matching it.
        let listed = if ids.is_empty() {
            "—".to_owned()
        } else {
            ids.join(", ")
        };
        let _ = writeln!(out, "  {:<width$}  {listed}", path.as_str());
    }

    let _ = writeln!(
        out,
        "\n{} {}, {} {}.",
        answers.len(),
        if answers.len() == 1 { "path" } else { "paths" },
        distinct.len(),
        if distinct.len() == 1 { "rule" } else { "rules" },
    );
}

fn render_text(path: &RepoRelPath, applies: &[Applies<'_>], out: &mut dyn std::io::Write) {
    // Said plainly rather than left as an empty list. "No rule applies" is a
    // useful answer for an agent deciding whether to ask again, and an empty
    // response reads like the command failed.
    if applies.is_empty() {
        let _ = writeln!(out, "No rule applies to `{path}`.");
        return;
    }

    let _ = writeln!(out, "Rules that apply to `{path}`:");
    for entry in applies {
        let _ = writeln!(out);
        let module = entry
            .rule
            .module
            .as_ref()
            .map_or_else(String::new, |module| format!(" [{module}]"));
        let _ = writeln!(
            out,
            "  [{}] {} ({}){module}",
            entry.rule.level,
            entry.rule.id,
            entry.rule.kind.type_name(),
        );
        // Before the expectations, because it is why they are what they are.
        // Issue #46.
        if let Some(why) = &entry.rule.why {
            let _ = writeln!(out, "    why: {why}");
        }
        for expectation in &entry.expectations {
            let _ = writeln!(
                out,
                "    {}",
                crate::report::describe_expectation(expectation)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_api::describe::describe;
    use archwarden_core::{
        compiled::{CompiledConfig, CompiledRule, CompiledRuleKind, SkipDirs},
        facts::{ExportKind, ExportTags, KindFilter},
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
            imports: None,
            level: Level::Error,
            scope: Scope::compile(scope.iter().copied()).expect("valid scope"),
            kind,
        }
    }

    fn naming() -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile(r"^(?<name>[a-z0-9-]+)\.use-case\.ts$")
                .expect("valid pattern"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            annotation: Vec::new(),
            signature_hint: Some("(deps: Deps) => UseCase".to_owned()),
        }
    }

    fn structure() -> CompiledRuleKind {
        CompiledRuleKind::Structure {
            allowed_subfolders: Some(vec!["types".to_owned()]),
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
            filename_patterns: Vec::new(),
        }
    }

    fn config(rules: Vec<CompiledRule>) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b"describe"),
        )
    }

    fn rendered(
        config: &CompiledConfig,
        target: &RepoRelPath,
        format: crate::report::Format,
    ) -> String {
        let mut out = Vec::new();
        render(target, &describe(config, target), format, &mut out);
        String::from_utf8(out).expect("output is UTF-8")
    }

    fn rendered_many(
        answers: &[(RepoRelPath, Vec<Applies<'_>>)],
        format: crate::report::Format,
    ) -> String {
        let mut out = Vec::new();
        render_many("packages/domain/src/*", answers, format, &mut out);
        String::from_utf8(out).expect("output is UTF-8")
    }

    /// The prose is the same renderer `check` uses, so the informant and the
    /// gate cannot word one requirement differently.
    #[test]
    fn the_text_output_reads_as_intended() {
        let config = config(vec![rule(
            "usecase-name",
            Some("app"),
            &["src/*"],
            naming(),
        )]);

        assert_eq!(
            rendered(
                &config,
                &path("src/user/create-client.use-case.ts"),
                crate::report::Format::Text
            ),
            "Rules that apply to `src/user/create-client.use-case.ts`:\n\
             \n\
             \x20 [error] usecase-name (naming) [app]\n\
             \x20   an export named `CreateClient`, shaped like `(deps: Deps) => UseCase`\n"
        );
    }

    /// "Nothing applies" is an answer, and a useful one for an agent deciding
    /// whether to ask again. An empty response would read like a failure.
    #[test]
    fn nothing_applying_is_said_out_loud() {
        let config = config(vec![rule("usecase-name", None, &["src/*"], naming())]);

        assert_eq!(
            rendered(
                &config,
                &path("docs/README.md"),
                crate::report::Format::Text
            ),
            "No rule applies to `docs/README.md`.\n"
        );
    }

    /// A rule's reason is printed before its expectations, because it is why
    /// they are what they are. Issue #46.
    #[test]
    fn a_rules_reason_is_printed_above_what_it_requires() {
        let mut governed = rule("usecase-name", None, &["src/*"], naming());
        governed.why = Some("the factory name is the public API".to_owned());

        let text = rendered(
            &config(vec![governed]),
            &path("src/user/create-client.use-case.ts"),
            crate::report::Format::Text,
        );

        let why = text.find("why:").expect("the reason is printed");
        let expectation = text.find("an export named").expect("and the expectation");
        assert!(why < expectation, "{text}");
    }

    /// The renderer hands back the shared envelope's bytes rather than
    /// assembling a shape of its own, so `describe --format json` and an MCP
    /// tool cannot answer the same question differently.
    #[test]
    fn the_json_is_the_shared_envelope() {
        let config = config(vec![rule(
            "usecase-name",
            Some("app"),
            &["src/*"],
            naming(),
        )]);
        let target = path("src/user/create-client.use-case.ts");

        let written: serde_json::Value =
            serde_json::from_str(&rendered(&config, &target, crate::report::Format::Json))
                .expect("valid JSON");

        let envelope = serde_json::to_value(archwarden_api::describe::envelope(
            &target,
            &describe(&config, &target),
        ))
        .expect("serialises");

        assert_eq!(written, envelope);
    }

    /// One line per path, which is the point: the alternative is scrolling
    /// past a block of three lines each.
    #[test]
    fn many_paths_render_one_line_each() {
        let config = config(vec![
            rule("shape", None, &["packages/domain/src/*"], structure()),
            rule("names", None, &["packages/domain/src/*"], naming()),
        ]);
        let answers: Vec<_> = ["packages/domain/src/invoice", "packages/domain/src/order"]
            .iter()
            .map(|p| {
                let path = path(p);
                let applies = describe(&config, &path);
                (path, applies)
            })
            .collect();

        let text = rendered_many(&answers, crate::report::Format::Text);

        assert_eq!(
            text,
            "Rules that apply under `packages/domain/src/*`:\n\
             \n\
             \x20 packages/domain/src/invoice  shape\n\
             \x20 packages/domain/src/order    shape\n\
             \n\
             2 paths, 1 rule.\n"
        );
    }

    /// A path nothing constrains keeps its line, saying so. Dropping it would
    /// make a reader think the glob did not match it.
    #[test]
    fn a_path_with_no_rules_still_has_a_line() {
        let config = config(vec![rule("shape", None, &["src/*"], structure())]);
        let path = path("packages/other");
        let answers = vec![(path.clone(), describe(&config, &path))];

        let text = rendered_many(&answers, crate::report::Format::Text);

        assert!(text.contains("packages/other  —"), "{text}");
        assert!(text.contains("1 path, 0 rules."), "{text}");
    }

    /// A glob matching nothing says so, rather than printing an empty list
    /// that reads like every path is unconstrained.
    #[test]
    fn a_glob_that_matches_nothing_says_so() {
        let text = rendered_many(&[], crate::report::Format::Text);

        assert_eq!(text, "Nothing matches `packages/domain/src/*`.\n");
    }

    /// The many-path JSON is the shared envelope too, and a different shape
    /// from the one-path answer because a different question was asked.
    #[test]
    fn the_many_path_json_is_the_shared_envelope() {
        let config = config(vec![rule(
            "shape",
            None,
            &["packages/domain/src/*"],
            structure(),
        )]);
        let path = path("packages/domain/src/invoice");
        let answers = vec![(path.clone(), describe(&config, &path))];

        let parsed: serde_json::Value =
            serde_json::from_str(&rendered_many(&answers, crate::report::Format::Json))
                .expect("valid JSON");

        assert_eq!(parsed["scope"], "packages/domain/src/*");
        assert_eq!(parsed["paths"][0]["path"], "packages/domain/src/invoice");
        assert!(
            parsed.get("path").is_none(),
            "a different shape, because a different question was asked"
        );
    }
}
