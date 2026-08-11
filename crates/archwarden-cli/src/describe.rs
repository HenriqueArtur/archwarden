//! `archwarden describe <path>` — what applies here, before anything is written.
//!
//! The informant half of decision 9. `check` tells an agent what it got wrong
//! after the fact; this tells it what the rules are while there is still time
//! to follow them, for a path that need not exist yet.
//!
//! It reads no file and parses nothing. Every rule's `describe_expectation` is
//! purely lexical by contract, which is what makes this answerable about a
//! file nobody has created.

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRule},
    finding::Expectation,
    path::RepoRelPath,
};
use camino::Utf8Path;
use serde::Serialize;

/// The version of the `describe` JSON shape.
///
/// Separate from the report's version: an agent consuming one may never read
/// the other, and coupling them would force a bump on consumers of a contract
/// that did not change.
pub const DESCRIBE_VERSION: u32 = 0;

/// One rule that has something to say about a path.
pub struct Applies<'a> {
    /// The rule itself, for its id, kind, level and module.
    pub rule: &'a CompiledRule,
    /// What it requires of this path. Never empty -- a rule with nothing to
    /// say is not in the list.
    pub expectations: Vec<Expectation>,
}

/// Every rule that applies to `path`, in configuration order.
///
/// An ignored path yields nothing, which is the same answer `check` gives: an
/// `ignore` entry wins over any rule's scope.
#[must_use]
pub fn describe<'a>(config: &'a CompiledConfig, path: &RepoRelPath) -> Vec<Applies<'a>> {
    if config.is_ignored(path) {
        return Vec::new();
    }

    config
        .rules()
        .zip(archwarden_rules::engines_for(config))
        .filter_map(|(rule, engine)| {
            let expectations = engine.describe_expectation(path);
            (!expectations.is_empty()).then_some(Applies { rule, expectations })
        })
        .collect()
}

/// Turns a path as typed on the command line into a repository-relative one.
///
/// # Two readings of one relative path
///
/// Standing in `packages/domain`, `src/order/x.ts` means the file under
/// here — that is what `git diff` and an editor hand a developer. But every
/// path archwarden *prints* is repository-relative, so the one an agent copies
/// out of a report is `packages/domain/src/order/x.ts`, and reading that
/// against the working directory gives `packages/domain/packages/domain/...`.
///
/// That did not fail. It resolved to a path no rule selects and answered "no
/// rule applies", which reads exactly like "nothing constrains this file" —
/// the wrong answer that looks like good news.
///
/// So both readings are tried, in this order:
///
/// 1. Against the working directory, when that names something on disk. It is
///    the reading a developer means, and it wins whenever both are real.
/// 2. Against the repository root, when *that* names something on disk.
/// 3. Otherwise, whichever the argument's own shape indicates: a path already
///    beginning with where the user is standing is repository-relative, since
///    nobody nests `packages/domain` inside `packages/domain`.
/// 4. Failing all of that, against the working directory, as before.
///
/// Steps 1 and 2 touch the filesystem, which this function used to avoid.
/// Existence is the only evidence available about which reading was meant, and
/// steps 3 and 4 are what keep `describe` and `scaffold` answering about files
/// that do not exist yet — which is most of what they are for.
///
/// From the repository root both readings are the same question, so none of
/// this costs the common case anything.
///
/// # Errors
/// A message naming the path, when it falls outside the repository.
pub fn repo_relative(
    root: &Utf8Path,
    working_directory: &Utf8Path,
    argument: &str,
) -> Result<RepoRelPath, String> {
    let raw = Utf8Path::new(argument);

    let relative = if raw.is_absolute() {
        raw.strip_prefix(root)
            .map(Utf8Path::to_string)
            .or_else(|_| {
                same_directory_by_another_name(root, raw)
                    .ok_or_else(|| format!("`{argument}` is outside the repository at `{root}`"))
            })?
    } else {
        let inside = working_directory.strip_prefix(root).map_err(|_| {
            format!("the working directory `{working_directory}` is outside `{root}`")
        })?;

        let here = inside.join(raw).to_string();
        if inside.as_str().is_empty() {
            here
        } else {
            disambiguate(root, inside, raw, here)
        }
    };

    RepoRelPath::new(&relative)
        .map_err(|error| format!("`{argument}` is not a path inside the repository: {error}"))
}

/// The same path, when the text says otherwise and the filesystem disagrees.
///
/// A repository has more than one absolute path more often than it looks: a
/// symlinked checkout, a bind-mounted worktree, `/tmp` → `/private/tmp` on
/// macOS, a container whose mount path differs from the host's. A harness hands
/// over whichever spelling its own `cwd` resolved to, and comparing the two as
/// text says "outside the repository" about a file plainly inside it.
///
/// # Why the parent and not the whole path
///
/// A pre-write hook is asked *before* the write, so the file it names usually
/// does not exist and `canonicalize` on it would fail — the case this most
/// needs to work. The parent directory does exist, so that is what gets
/// resolved, and the file name is put back afterwards.
///
/// It also keeps a change nobody asked for from creeping in. Resolving the
/// whole path would follow a symlinked *file* to wherever it points, so a link
/// inside the repository aimed outside it would start being refused. That may
/// even be right, but it is a different question from this one and it should be
/// asked on its own.
///
/// Returns `None` when the path is genuinely somewhere else, which is still
/// most of the times this is reached.
fn same_directory_by_another_name(root: &Utf8Path, raw: &Utf8Path) -> Option<String> {
    let name = raw.file_name()?;
    let parent = raw.parent()?;

    let real_root = std::fs::canonicalize(root).ok()?;
    let real_parent = std::fs::canonicalize(parent).ok()?;

    let inside = real_parent.strip_prefix(&real_root).ok()?;

    let relative = Utf8Path::from_path(inside)?.join(name);
    Some(relative.to_string())
}

/// Picks between the two readings. See [`repo_relative`].
fn disambiguate(root: &Utf8Path, inside: &Utf8Path, raw: &Utf8Path, here: String) -> String {
    let there = raw.to_string();

    if root.join(&here).exists() {
        return here;
    }
    if root.join(&there).exists() {
        return there;
    }
    // Nothing on disk to go by, which is the case `describe` exists for. A
    // path that already carries the way here was written from the root.
    if raw.starts_with(inside) {
        return there;
    }
    here
}

/// The JSON envelope.
#[derive(Debug, Serialize)]
struct JsonDescribe<'a> {
    version: u32,
    path: &'a RepoRelPath,
    rules: Vec<JsonRule<'a>>,
}

#[derive(Debug, Serialize)]
struct JsonRule<'a> {
    id: &'a str,
    kind: &'static str,
    level: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    module: Option<&'a str>,
    /// Why the rule exists, when its author said. Issue #46: an agent that
    /// knows the rule and not the reason can comply and nothing else, which is
    /// how a config gets edited to make a check pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    why: Option<&'a str>,
    /// Why the module it belongs to exists. A separate answer, not a fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    module_why: Option<&'a str>,
    expectations: &'a [Expectation],
}

/// The JSON envelope for many paths at once.
///
/// A different shape from the one-path answer, because a different question
/// was asked. A consumer that passed a glob knows to expect it.
#[derive(Debug, Serialize)]
struct JsonScope<'a> {
    version: u32,
    scope: &'a str,
    paths: Vec<JsonDescribe<'a>>,
}

/// Writes what applies, in the requested format.
pub fn render(
    path: &RepoRelPath,
    applies: &[Applies<'_>],
    format: crate::report::Format,
    out: &mut dyn std::io::Write,
) {
    match format {
        crate::report::Format::Text => render_text(path, applies, out),
        crate::report::Format::Json => render_json(path, applies, out),
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
            let envelope = JsonScope {
                version: DESCRIBE_VERSION,
                scope,
                paths: answers
                    .iter()
                    .map(|(path, applies)| envelope_for(path, applies))
                    .collect(),
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

fn envelope_for<'a>(path: &'a RepoRelPath, applies: &'a [Applies<'a>]) -> JsonDescribe<'a> {
    JsonDescribe {
        version: DESCRIBE_VERSION,
        path,
        rules: applies
            .iter()
            .map(|entry| JsonRule {
                id: entry.rule.id.as_str(),
                kind: entry.rule.kind.type_name(),
                level: entry.rule.level.as_str(),
                module: entry
                    .rule
                    .module
                    .as_ref()
                    .map(archwarden_core::ids::ModuleId::as_str),
                why: entry.rule.why.as_deref(),
                module_why: entry.rule.module_why.as_deref(),
                expectations: &entry.expectations,
            })
            .collect(),
    }
}

fn render_json(path: &RepoRelPath, applies: &[Applies<'_>], out: &mut dyn std::io::Write) {
    let envelope = JsonDescribe {
        version: DESCRIBE_VERSION,
        path,
        rules: applies
            .iter()
            .map(|entry| JsonRule {
                id: entry.rule.id.as_str(),
                kind: entry.rule.kind.type_name(),
                level: entry.rule.level.as_str(),
                module: entry
                    .rule
                    .module
                    .as_ref()
                    .map(archwarden_core::ids::ModuleId::as_str),
                why: entry.rule.why.as_deref(),
                module_why: entry.rule.module_why.as_deref(),
                expectations: &entry.expectations,
            })
            .collect(),
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
    use archwarden_core::{
        compiled::{CompiledRuleKind, SkipDirs},
        facts::{ExportKind, ExportTags, KindFilter},
        glob::PathSet,
        hash::ContentHash,
        ids::{ModuleId, RuleId},
        level::Level,
        pattern::Pattern,
        scope::Scope,
    };
    use camino::Utf8PathBuf;

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

    fn config(rules: Vec<CompiledRule>, ignore: &[&str]) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::compile(ignore.iter().map(|g| (*g).to_owned())).expect("valid globs"),
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

    /// The reason the command exists: an agent about to write a file asks what
    /// is expected of it, and the file does not exist yet.
    /// Issue #46. `describe` is what an agent asks *before* writing, which is
    /// the moment a reason is worth most: knowing the rule is not knowing why,
    /// and a constraint that looks arbitrary is the one that gets worked
    /// around.
    #[test]
    fn a_rules_reason_reaches_the_json() {
        let mut governed = rule(
            "domain-forbids-app",
            Some("domain"),
            &["src/*"],
            CompiledRuleKind::Structure {
                allowed_subfolders: Some(vec!["types".to_owned()]),
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: Vec::new(),
                filename_patterns: Vec::new(),
            },
        );
        governed.why = Some("domain is published and the app is not".to_owned());
        governed.module_why = Some("extracted so billing could depend on it".to_owned());

        let text = rendered(
            &config(vec![governed], &[]),
            &path("src/user"),
            crate::report::Format::Json,
        );
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");

        assert_eq!(
            parsed["rules"][0]["why"],
            "domain is published and the app is not"
        );
        assert_eq!(
            parsed["rules"][0]["module_why"],
            "extracted so billing could depend on it"
        );
    }

    #[test]
    fn a_path_that_does_not_exist_still_has_rules() {
        let config = config(
            vec![
                rule("usecase-name", Some("app"), &["src/*"], naming()),
                rule("usecase-spec", None, &["src/*"], spec_pair()),
            ],
            &[],
        );
        let target = path("src/user/create-client.use-case.ts");

        let applies = describe(&config, &target);
        let ids: Vec<_> = applies.iter().map(|a| a.rule.id.as_str()).collect();

        assert_eq!(ids, ["usecase-name", "usecase-spec"]);
    }

    /// A rule whose scope covers the path but which has nothing to say about
    /// *this* file is not listed. "Applies" means "has a requirement", not
    /// "the glob matched".
    #[test]
    fn a_rule_with_nothing_to_say_is_not_listed() {
        let config = config(vec![rule("usecase-name", None, &["src/*"], naming())], &[]);

        assert!(describe(&config, &path("src/user/helper.ts")).is_empty());
    }

    /// An `ignore` entry wins over any rule's scope, and `describe` has to
    /// agree with `check` about that or an agent would be told to satisfy a
    /// rule that will never fire.
    #[test]
    fn an_ignored_path_has_no_rules() {
        let config = config(
            vec![rule("usecase-name", None, &["src/*"], naming())],
            &["src/legacy/**"],
        );

        assert!(
            describe(&config, &path("src/legacy/old.use-case.ts")).is_empty(),
            "ignore wins"
        );
        assert_eq!(
            describe(&config, &path("src/user/new.use-case.ts")).len(),
            1,
            "and only for the ignored subtree"
        );
    }

    /// Configuration order is preserved, so the answer reads in the order the
    /// user wrote their rules rather than in whatever order engines are built.
    #[test]
    fn rules_come_back_in_configuration_order() {
        let config = config(
            vec![
                rule("second", None, &["src/*"], spec_pair()),
                rule("first", None, &["src/*"], naming()),
            ],
            &[],
        );

        let applies = describe(&config, &path("src/user/create.use-case.ts"));
        let ids: Vec<_> = applies.iter().map(|a| a.rule.id.as_str()).collect();
        assert_eq!(ids, ["second", "first"]);
    }

    /// The prose is the same renderer `check` uses, so the informant and the
    /// gate cannot word one requirement differently.
    #[test]
    fn the_text_output_reads_as_intended() {
        let config = config(
            vec![rule("usecase-name", Some("app"), &["src/*"], naming())],
            &[],
        );

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
        let config = config(vec![rule("usecase-name", None, &["src/*"], naming())], &[]);

        assert_eq!(
            rendered(
                &config,
                &path("docs/README.md"),
                crate::report::Format::Text
            ),
            "No rule applies to `docs/README.md`.\n"
        );
    }

    /// The JSON is a contract with agents, so it is asserted field by field.
    #[test]
    fn the_json_shape_is_versioned_and_complete() {
        let config = config(
            vec![
                rule("usecase-name", Some("app"), &["src/*"], naming()),
                rule("usecase-spec", None, &["src/*"], spec_pair()),
            ],
            &[],
        );

        let json = rendered(
            &config,
            &path("src/user/create-client.use-case.ts"),
            crate::report::Format::Json,
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(parsed["version"], 0);
        assert_eq!(parsed["path"], "src/user/create-client.use-case.ts");

        let first = &parsed["rules"][0];
        assert_eq!(first["id"], "usecase-name");
        assert_eq!(first["kind"], "naming");
        assert_eq!(first["level"], "error");
        assert_eq!(first["module"], "app");
        assert_eq!(first["expectations"][0]["type"], "required-export");
        assert_eq!(first["expectations"][0]["name"], "CreateClient");

        let second = &parsed["rules"][1];
        assert_eq!(second["kind"], "spec-pair");
        assert!(second["module"].is_null(), "a top-level rule has none");
        assert_eq!(
            second["expectations"][0]["path"],
            "src/user/create-client.use-case.spec.ts"
        );
    }

    /// A rule with no module omits the field rather than sending `null`, so
    /// the common answer stays small.
    #[test]
    fn a_rule_without_a_module_omits_the_field() {
        let config = config(vec![rule("usecase-name", None, &["src/*"], naming())], &[]);
        let json = rendered(
            &config,
            &path("src/user/create.use-case.ts"),
            crate::report::Format::Json,
        );

        assert!(!json.contains("\"module\""), "{json}");
    }

    // --- path resolution -------------------------------------------------

    fn root() -> Utf8PathBuf {
        Utf8PathBuf::from("/repo")
    }

    /// The ordinary case: run from the root, name a path.
    #[test]
    fn a_relative_path_resolves_against_the_working_directory() {
        assert_eq!(
            repo_relative(&root(), &root(), "src/user/a.ts").expect("resolves"),
            path("src/user/a.ts")
        );
    }

    /// The case that makes this worth a function: the user is standing in a
    /// subdirectory, which is where anyone actually works.
    #[test]
    fn a_relative_path_from_a_subdirectory_is_still_repo_relative() {
        assert_eq!(
            repo_relative(&root(), &root().join("src/user"), "a.ts").expect("resolves"),
            path("src/user/a.ts")
        );
        assert_eq!(
            repo_relative(&root(), &root().join("src/user"), "../shared/b.ts").expect("resolves"),
            path("src/shared/b.ts")
        );
    }

    /// An absolute path is accepted, because a harness hook has one and should
    /// not have to make it relative first.
    #[test]
    fn an_absolute_path_inside_the_repository_resolves() {
        assert_eq!(
            repo_relative(&root(), &root(), "/repo/src/user/a.ts").expect("resolves"),
            path("src/user/a.ts")
        );
    }

    /// And one outside says so, naming both halves, rather than silently
    /// describing the wrong file.
    #[test]
    fn a_path_outside_the_repository_is_refused() {
        assert_eq!(
            repo_relative(&root(), &root(), "/elsewhere/a.ts").expect_err("outside"),
            "`/elsewhere/a.ts` is outside the repository at `/repo`"
        );
        assert_eq!(
            repo_relative(&root(), &root(), "../a.ts").expect_err("escapes"),
            "`../a.ts` is not a path inside the repository: `../a.ts` escapes the repository root"
        );
    }

    /// Two spellings of one directory are one directory.
    ///
    /// A symlinked checkout, a bind-mounted worktree, `/tmp` → `/private/tmp`
    /// on macOS, a container whose mount path differs from the host's: each
    /// gives a repository two absolute paths, and a harness hands over
    /// whichever one its own `cwd` resolved to. Comparing the two as text
    /// answers "outside the repository" about a file plainly inside it.
    ///
    /// Reported against 0.10.0, where the consequence was a pre-write hook
    /// that permitted every write on such a machine while reporting success.
    #[cfg(unix)]
    #[test]
    fn a_second_route_to_the_same_directory_is_the_same_directory() {
        let temporary = tempfile::tempdir().expect("temp dir");
        let real = temporary.path().join("real");
        std::fs::create_dir_all(real.join("src")).expect("create");
        std::fs::write(real.join("src/a.ts"), b"export const a = 1;").expect("write");

        let link = temporary.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        let root = Utf8PathBuf::from_path_buf(real).expect("utf-8");
        let through_link = Utf8PathBuf::from_path_buf(link)
            .expect("utf-8")
            .join("src/a.ts");

        assert_eq!(
            repo_relative(&root, &root, through_link.as_str()).expect("resolves"),
            path("src/a.ts")
        );
    }

    /// And the same when the file is not there yet, which is the case a
    /// pre-write hook is always in: it is asked before the write, so the path
    /// it is handed usually names nothing on disk.
    #[cfg(unix)]
    #[test]
    fn a_second_route_resolves_for_a_file_that_does_not_exist_yet() {
        let temporary = tempfile::tempdir().expect("temp dir");
        let real = temporary.path().join("real");
        std::fs::create_dir_all(real.join("src")).expect("create");

        let link = temporary.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        let root = Utf8PathBuf::from_path_buf(real).expect("utf-8");
        let through_link = Utf8PathBuf::from_path_buf(link)
            .expect("utf-8")
            .join("src/not-written-yet.ts");

        assert_eq!(
            repo_relative(&root, &root, through_link.as_str()).expect("resolves"),
            path("src/not-written-yet.ts")
        );
    }

    /// A path that is genuinely elsewhere still says so. The point is to stop
    /// mistaking one directory for two, not to stop refusing.
    #[cfg(unix)]
    #[test]
    fn a_path_that_is_really_outside_is_still_refused() {
        let temporary = tempfile::tempdir().expect("temp dir");
        let real = temporary.path().join("real");
        let other = temporary.path().join("other");
        std::fs::create_dir_all(&real).expect("create");
        std::fs::create_dir_all(&other).expect("create");
        std::fs::write(other.join("a.ts"), b"export const a = 1;").expect("write");

        let root = Utf8PathBuf::from_path_buf(real).expect("utf-8");
        let outside = Utf8PathBuf::from_path_buf(other)
            .expect("utf-8")
            .join("a.ts");

        assert!(
            repo_relative(&root, &root, outside.as_str()).is_err(),
            "a path in another directory was accepted"
        );
    }

    /// The repository root itself is a directory, and a structure rule has
    /// something to say about directories.
    #[test]
    fn the_root_is_addressable() {
        assert_eq!(
            repo_relative(&root(), &root(), ".").expect("resolves"),
            path("")
        );
    }

    // --- many paths at once ----------------------------------------------

    fn rendered_many(
        answers: &[(RepoRelPath, Vec<Applies<'_>>)],
        format: crate::report::Format,
    ) -> String {
        let mut out = Vec::new();
        render_many("packages/domain/src/*", answers, format, &mut out);
        String::from_utf8(out).expect("output is UTF-8")
    }

    fn config_of(rules: Vec<CompiledRule>) -> CompiledConfig {
        config(rules, &[])
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

    /// One line per path, which is the point: the alternative is scrolling
    /// past a block of three lines each.
    #[test]
    fn many_paths_render_one_line_each() {
        let config = config_of(vec![
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
        let config = config_of(vec![rule("shape", None, &["src/*"], structure())]);
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

    /// The JSON keeps every expectation, because an agent asking about an
    /// area still needs the detail it would have got asking one path at a
    /// time. Only the terminal wants it short.
    #[test]
    fn the_json_carries_the_full_answer_per_path() {
        let config = config_of(vec![rule(
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
        assert_eq!(parsed["paths"][0]["rules"][0]["id"], "shape");
        assert!(
            parsed["paths"][0]["rules"][0]["expectations"].is_array(),
            "the detail is there"
        );
        assert!(
            parsed.get("path").is_none(),
            "a different shape, because a different question was asked"
        );
    }

    // --- the two readings of one relative path ---------------------------

    /// A tree on disk, because existence is what tells the two readings apart.
    fn tree(entries: &[&str]) -> (tempfile::TempDir, Utf8PathBuf) {
        let guard = tempfile::tempdir().expect("temp dir");
        let root =
            Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("temp path is UTF-8");
        for entry in entries {
            let file = root.join(entry);
            std::fs::create_dir_all(file.parent().expect("a file has a parent"))
                .expect("create dirs");
            std::fs::write(&file, "export const a = 1;\n").expect("write");
        }
        (guard, root)
    }

    /// The defect this replaces. Every path archwarden prints is
    /// repository-relative, so the one an agent copies out of a report is too
    /// — and pasting it back while standing in a subdirectory used to resolve
    /// to `packages/domain/packages/domain/...`, which does not exist.
    ///
    /// It did not fail. It answered "no rule applies", which reads exactly
    /// like "nothing constrains this file".
    #[test]
    fn a_repository_relative_path_pasted_from_a_report_resolves() {
        let (_guard, root) = tree(&["packages/domain/src/order/calcs/x.ts"]);
        let inside = root.join("packages/domain");

        assert_eq!(
            repo_relative(&root, &inside, "packages/domain/src/order/calcs/x.ts")
                .expect("resolves"),
            path("packages/domain/src/order/calcs/x.ts")
        );
    }

    /// And the reading that was always right stays right. This is the path a
    /// developer has in hand from `git diff` or an editor, and it wins when
    /// both readings name something real.
    #[test]
    fn a_path_relative_to_where_you_stand_still_wins() {
        let (_guard, root) = tree(&[
            "packages/domain/src/order/calcs/x.ts",
            // The same relative path, real from the root as well. Whoever is
            // standing in `packages/domain` means theirs.
            "src/order/calcs/x.ts",
        ]);
        let inside = root.join("packages/domain");

        assert_eq!(
            repo_relative(&root, &inside, "src/order/calcs/x.ts").expect("resolves"),
            path("packages/domain/src/order/calcs/x.ts")
        );
    }

    /// A path only the root reading finds is the root reading's, even though
    /// it does not begin with where the user is standing.
    #[test]
    fn a_path_only_the_repository_reading_finds_is_taken() {
        let (_guard, root) = tree(&["src/shared/b.ts"]);
        let inside = root.join("packages/domain");
        std::fs::create_dir_all(&inside).expect("create dirs");

        assert_eq!(
            repo_relative(&root, &inside, "src/shared/b.ts").expect("resolves"),
            path("src/shared/b.ts")
        );
    }

    /// `describe` is asked about files that do not exist yet -- that is what
    /// it is for. With nothing on disk to go by, a path that already starts
    /// with where the user is standing is repository-relative: nobody nests
    /// `packages/domain` inside `packages/domain`.
    #[test]
    fn a_file_that_does_not_exist_yet_is_read_by_its_prefix() {
        let (_guard, root) = tree(&[]);
        let inside = root.join("packages/domain");
        std::fs::create_dir_all(&inside).expect("create dirs");

        assert_eq!(
            repo_relative(&root, &inside, "packages/domain/src/new/thing.ts").expect("resolves"),
            path("packages/domain/src/new/thing.ts")
        );
        // And one that does not carry the prefix is where the user is standing,
        // which is the older behaviour and the common case.
        assert_eq!(
            repo_relative(&root, &inside, "src/new/thing.ts").expect("resolves"),
            path("packages/domain/src/new/thing.ts")
        );
    }

    /// From the root the two readings are the same question, so nothing here
    /// costs the common case anything.
    #[test]
    fn from_the_root_there_is_only_one_reading() {
        let (_guard, root) = tree(&["src/user/a.ts"]);

        assert_eq!(
            repo_relative(&root, &root, "src/user/a.ts").expect("resolves"),
            path("src/user/a.ts")
        );
        assert_eq!(
            repo_relative(&root, &root, "src/nothing/here.ts").expect("resolves"),
            path("src/nothing/here.ts")
        );
    }

    /// A directory answers the same way a file does: `describe` and `scaffold`
    /// both take one, and a structure rule has more to say about a directory
    /// than about anything in it.
    #[test]
    fn a_directory_resolves_by_the_same_rules() {
        let (_guard, root) = tree(&["packages/domain/src/order/calcs/x.ts"]);
        let inside = root.join("packages/domain");

        assert_eq!(
            repo_relative(&root, &inside, "packages/domain/src/order").expect("resolves"),
            path("packages/domain/src/order")
        );
    }
}
