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

use std::fmt::Write as _;

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRule},
    finding::Finding,
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

/// What one decision was, and what is keeping it.
///
/// The other question `config explain` answers, and issue #100 argues it is
/// the one people actually ask: not *what does this rule do* but *why is this
/// like this*. A document answers the first half of that and cannot answer the
/// second — whether the decision is still being kept — because only a run
/// knows.
pub struct DecisionExplanation<'a> {
    /// The decision itself, with its prose.
    pub decision: &'a archwarden_core::compiled::CompiledDecision,
    /// The rules implementing it, in configuration order, with what each is
    /// currently flagging. Empty is a real answer and is said out loud.
    pub rules: Vec<RuleUnderDecision<'a>>,
    /// Accepted entries of this decision's rules that no longer occur.
    ///
    /// The cheerful half of the ratchet, and the reason it is here rather than
    /// only in `check`: somebody asking about one decision is asking whether
    /// it is being kept, and debt paid against it is the answer they hoped
    /// for. Issue #112.
    pub paid: usize,
}

/// One rule serving a decision, and how it is doing.
pub struct RuleUnderDecision<'a> {
    /// The rule.
    pub rule: &'a CompiledRule,
    /// How many findings it is currently producing.
    ///
    /// A count rather than the findings: the question here is whether the
    /// decision is holding, and `config explain <rule-id>` is one command away
    /// for the detail.
    pub flags: usize,
    /// How many of those the baseline already excuses.
    ///
    /// Broken down per rule because the total on its own is a number nobody
    /// can act on: two rules serving one decision are two different debts, and
    /// only one of them is usually the one worth paying first. Issue #112.
    pub excused: usize,
}

/// Either answer the command can give.
///
/// One argument, two namespaces, and they are kept apart at compile time — an
/// id may not be both a rule and a decision — so this never has to guess which
/// was meant. Issue #100.
pub enum Explained<'a> {
    /// The argument named a rule.
    Rule(Explanation<'a>),
    /// The argument named a decision.
    Decision(DecisionExplanation<'a>),
}

#[cfg(test)]
impl<'a> Explained<'a> {
    /// The rule answer, for a test that asked about a rule id.
    ///
    /// Test-only: production code matches both arms, because the argument can
    /// be either and a surface that assumed one would panic on the other.
    fn into_rule(self) -> Explanation<'a> {
        match self {
            Self::Rule(rule) => rule,
            Self::Decision(decision) => {
                panic!("expected a rule, got decision `{}`", decision.decision.id)
            }
        }
    }
}

/// Explains one rule or one decision, or says which ids exist.
///
/// # Errors
/// A message listing the configured rule ids *and* decision ids, when `wanted`
/// is neither. A typo is the likeliest way to reach this, and a user who
/// mistyped does not know which of the two lists they meant.
pub fn explain<'a>(
    root: &Utf8Path,
    config: &'a CompiledConfig,
    tree: &RepoTree,
    wanted: &str,
) -> Result<Explained<'a>, String> {
    let Some((rule, engine)) = config
        .rules()
        .zip(archwarden_rules::engines_for(config))
        .find(|(rule, _)| rule.id.as_str() == wanted)
    else {
        if let Some(decision) = config.decisions().find(|d| d.id.as_str() == wanted) {
            return Ok(Explained::Decision(explain_decision(
                root, config, tree, decision,
            )));
        }
        return Err(nothing_is_called(config, wanted));
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
        as_of: archwarden_core::date::Date::today(),
    });
    let flags = report
        .findings
        .into_iter()
        .filter(|finding| finding.rule_id.as_str() == wanted)
        .collect();

    Ok(Explained::Rule(Explanation {
        rule,
        covers,
        scope_reaches,
        flags,
    }))
}

/// The message for an id that is neither a rule nor a decision.
///
/// Both lists, always. A user who mistyped `ADR-041` does not know whether
/// they got a rule id or a decision id wrong, and a message naming only one of
/// the two teaches them the other does not exist.
fn nothing_is_called(config: &CompiledConfig, wanted: &str) -> String {
    let rules: Vec<String> = config
        .rules()
        .map(|rule| format!("`{}`", rule.id))
        .collect();
    let decisions: Vec<String> = config
        .decisions()
        .map(|decision| format!("`{}`", decision.id))
        .collect();

    match (rules.is_empty(), decisions.is_empty()) {
        (true, true) => format!(
            "nothing is called `{wanted}`; this configuration has no rules and no decisions"
        ),
        (false, true) => format!(
            "nothing is called `{wanted}`; the configured rules are {}",
            rules.join(", ")
        ),
        (true, false) => format!(
            "nothing is called `{wanted}`; the declared decisions are {}",
            decisions.join(", ")
        ),
        (false, false) => format!(
            "nothing is called `{wanted}`; the configured rules are {}, and the \
             declared decisions are {}",
            rules.join(", "),
            decisions.join(", ")
        ),
    }
}

/// What a decision is, and what is keeping it.
///
/// The counts come from one real run, the same way the rule answer's do, so
/// what this says is what `check` reports rather than a second evaluation that
/// could disagree with it.
fn explain_decision<'a>(
    root: &Utf8Path,
    config: &'a CompiledConfig,
    tree: &RepoTree,
    decision: &'a archwarden_core::compiled::CompiledDecision,
) -> DecisionExplanation<'a> {
    let report = archwarden_engine::run::check(archwarden_engine::run::Run {
        root,
        config,
        tree,
        cache: None,
        as_of: archwarden_core::date::Date::today(),
    });

    // Read here rather than passed in: this command already opens the
    // repository, and a decision explained without the debt against it is the
    // half-answer issue #112 was filed about. A baseline that will not parse
    // leaves the rest of the answer standing, on `report_standing`'s
    // precedent -- the question asked was about a decision, not about a file.
    let baseline = archwarden_api::baseline::Baseline::load(root)
        .ok()
        .flatten();

    let rules: Vec<RuleUnderDecision<'a>> = config
        .rules()
        .filter(|rule| rule.decision.as_ref() == Some(&decision.id))
        .map(|rule| {
            let flagged: Vec<_> = report
                .findings
                .iter()
                .filter(|finding| finding.rule_id == rule.id)
                .collect();
            RuleUnderDecision {
                excused: baseline.as_ref().map_or(0, |baseline| {
                    flagged
                        .iter()
                        .filter(|finding| baseline.accepts(finding))
                        .count()
                }),
                flags: flagged.len(),
                rule,
            }
        })
        .collect();

    let paid = baseline.as_ref().map_or(0, |baseline| {
        baseline
            .standing(&report.findings, config)
            .by_decision
            .get(decision.id.as_str())
            .map_or(0, |standing| standing.gone)
    });

    DecisionExplanation {
        decision,
        rules,
        paid,
    }
}

/// The JSON envelope.
#[derive(Debug, Serialize)]
struct JsonExplain<'a> {
    version: u32,
    /// Which of the two questions this answers.
    ///
    /// Said out loud rather than left for a consumer to infer from which keys
    /// are present: one argument can return either shape, and a program that
    /// had to tell them apart by probing would get it wrong on the day a field
    /// is added. Issue #100.
    explains: &'static str,
    id: &'a str,
    kind: &'static str,
    level: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    module: Option<&'a str>,
    applies_to: &'a [String],
    covers: &'a [RepoRelPath],
    flags: &'a [Finding],
}

/// The JSON envelope for a decision.
#[derive(Debug, Serialize)]
struct JsonDecision<'a> {
    version: u32,
    explains: &'static str,
    id: &'a str,
    title: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    why: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    link: Option<&'a str>,
    status: &'a str,
    rules: Vec<JsonRuleUnder<'a>>,
    /// How much of what this decision's rules flag is excused by the baseline.
    ///
    /// The number that says whether the decision is real. `excused == the sum
    /// of the rules' flags` is a decision that has never refused anything, and
    /// a consumer can compute that verdict from what is here. Issue #112.
    excused: usize,
    /// Accepted entries of this decision's rules that no longer occur.
    paid: usize,
}

/// One rule under a decision, as JSON.
#[derive(Debug, Serialize)]
struct JsonRuleUnder<'a> {
    id: &'a str,
    kind: &'static str,
    level: &'a str,
    flags: usize,
    /// How many of `flags` the baseline already excuses. Always present,
    /// including as zero: a consumer comparing the two needs the field to
    /// exist, and a repository with no baseline is a real answer of zero.
    excused: usize,
}

/// Writes the explanation.
pub fn render(
    explained: &Explained<'_>,
    format: crate::report::Format,
    out: &mut dyn std::io::Write,
) {
    match (explained, format) {
        (Explained::Rule(rule), crate::report::Format::Text) => render_text(rule, out),
        (Explained::Rule(rule), crate::report::Format::Json) => render_json(rule, out),
        (Explained::Decision(decision), crate::report::Format::Text) => {
            render_decision_text(decision, out);
        }
        (Explained::Decision(decision), crate::report::Format::Json) => {
            render_decision_json(decision, out);
        }
    }
}

/// Serialises an envelope, or says why not.
///
/// One function for both shapes, because the failure sentence is the same and
/// two copies of it are two copies that drift.
fn write_json(envelope: &impl Serialize, out: &mut dyn std::io::Write) {
    match serde_json::to_string_pretty(envelope) {
        Ok(json) => {
            let _ = writeln!(out, "{json}");
        }
        Err(error) => {
            let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
        }
    }
}

fn render_decision_json(explanation: &DecisionExplanation<'_>, out: &mut dyn std::io::Write) {
    let decision = explanation.decision;
    write_json(
        &JsonDecision {
            version: EXPLAIN_VERSION,
            explains: "decision",
            id: decision.id.as_str(),
            title: decision.title.as_str(),
            why: decision.why.as_deref(),
            link: decision.link.as_deref(),
            status: decision.status.as_str(),
            rules: explanation
                .rules
                .iter()
                .map(|under| JsonRuleUnder {
                    id: under.rule.id.as_str(),
                    kind: under.rule.kind.type_name(),
                    level: under.rule.level.as_str(),
                    flags: under.flags,
                    excused: under.excused,
                })
                .collect(),
            excused: explanation.rules.iter().map(|under| under.excused).sum(),
            paid: explanation.paid,
        },
        out,
    );
}

fn render_decision_text(explanation: &DecisionExplanation<'_>, out: &mut dyn std::io::Write) {
    let decision = explanation.decision;

    // The status is on the header rather than tucked below, because the one
    // reading it needs most — a decision that was replaced, with rules still
    // enforcing it — is the one `config doctor` calls an error, and this must
    // not be quieter than that.
    let status = match (
        decision.status.is_accepted(),
        decision.superseded_by.first(),
    ) {
        (true, _) => String::new(),
        // A reader told to stop trusting a decision needs to be told where to
        // go instead, which is the one thing the flag alone could not say.
        // Issue #115.
        (false, Some(by)) => format!(" (superseded by {by})"),
        (false, None) => format!(" ({})", decision.status),
    };
    let _ = writeln!(out, "{}{status} — {}", decision.id, decision.title);
    if !decision.supersedes.is_empty() {
        let named: Vec<String> = decision
            .supersedes
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        let _ = writeln!(out, "  replaces {}", named.join(", "));
    }
    if let Some(why) = &decision.why {
        let _ = writeln!(out, "  {why}");
    }
    if let Some(link) = &decision.link {
        let _ = writeln!(out, "  written down in {link}");
    }

    // Said out loud. A decision nobody enforces is exactly what somebody runs
    // this command to find out, and an empty list does not tell them.
    if explanation.rules.is_empty() {
        let _ = writeln!(
            out,
            "\n  No rule implements it. This decision is written down and not \
             enforced.\n  `archwarden config doctor` reports it."
        );
        return;
    }

    let _ = writeln!(
        out,
        "\n  Implemented by {}:",
        count(explanation.rules.len(), "rule")
    );
    for under in &explanation.rules {
        let mut flags = if under.flags == 0 {
            "flags nothing".to_owned()
        } else {
            format!("flags {}", count(under.flags, "path"))
        };
        // Beside the count rather than under it: "flags 68 paths" and "68 of
        // them are excused" are one fact, and a reader who stops at the first
        // half has been told this rule is doing something it is not.
        if under.excused > 0 {
            let _ = write!(flags, ", {} excused", under.excused);
        }
        let _ = writeln!(
            out,
            "    [{}] {} ({}) — {flags}",
            under.rule.level,
            under.rule.id,
            under.rule.kind.type_name(),
        );
    }

    // What was weighed and lost, before the verdict about what is holding: a
    // reader deciding whether to reopen this needs the argument that closed it
    // first. Issue #114.
    if !decision.alternatives.is_empty() {
        let _ = writeln!(out, "\n  Considered and rejected:");
        for alternative in &decision.alternatives {
            let refused = match &alternative.refused_by {
                Some(rule) => format!(" — refused by `{rule}`"),
                // Said out loud. An option nothing stops is the one somebody
                // takes, and a blank here would read as "this is handled".
                None => " — nothing refuses it".to_owned(),
            };
            let _ = writeln!(out, "    {}{refused}", alternative.option);
            let _ = writeln!(out, "      {}", alternative.why_not);
        }
    }

    // The half a document cannot answer: not what was decided, but whether it
    // is still being kept.
    let total: usize = explanation.rules.iter().map(|under| under.flags).sum();
    let excused: usize = explanation.rules.iter().map(|under| under.excused).sum();

    let _ = match (total, excused) {
        (0, _) => writeln!(out, "\n  Nothing in this repository breaks it."),
        // Every path it flags is already accepted. The decision is written
        // down, implemented, and has never refused a single change -- which
        // reads as *kept* on every surface that stops at the rule list, and is
        // the failure issue #112 was filed about.
        (total, excused) if excused == total => writeln!(
            out,
            "\n  {} {} it, and the baseline excuses {}.\n  \
             It has never refused anything.",
            count(total, "path"),
            if total == 1 { "breaks" } else { "break" },
            if total == 1 { "it" } else { "all of them" },
        ),
        (total, 0) => writeln!(out, "\n  {} currently breaks it.", count(total, "path")),
        (total, excused) => writeln!(
            out,
            "\n  {} {} it: {excused} excused by the baseline, {} not.",
            count(total, "path"),
            if total == 1 { "breaks" } else { "break" },
            total - excused,
        ),
    };

    // Last, because it is the only good news here and the reader should leave
    // on it when there is any.
    if explanation.paid > 0 {
        let _ = writeln!(
            out,
            "  {} no longer {} — run `archwarden baseline` to update.",
            count(explanation.paid, "accepted entry"),
            if explanation.paid == 1 {
                "occurs"
            } else {
                "occur"
            },
        );
    }
}

fn render_json(explanation: &Explanation<'_>, out: &mut dyn std::io::Write) {
    let rule = explanation.rule;
    let envelope = JsonExplain {
        version: EXPLAIN_VERSION,
        explains: "rule",
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

    write_json(&envelope, out);
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
        compiled::{CompiledDecision, CompiledRuleKind, DecisionStatus, SkipDirs},
        facts::{ExportKind, ExportTags, KindFilter},
        glob::PathSet,
        hash::ContentHash,
        ids::DecisionId,
        ids::RuleId,
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
            ignore_files: archwarden_core::glob::PathSet::default(),
        }
    }

    fn structure() -> CompiledRuleKind {
        CompiledRuleKind::Structure {
            allowed_subfolders: Some(Vec::new()),
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
            filename_patterns: Vec::new(),
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
        let result = explain(&root, config, &tree, wanted).map(|explanation| {
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

    /// A configuration where one decision is served by two rules, one of them
    /// currently firing.
    fn decided() -> CompiledConfig {
        let mut named = rule("usecase-name", &["src/*"], naming());
        named.decision = Some(DecisionId::new("ADR-014").expect("valid"));
        let mut shape = rule("usecase-shape", &["src/*"], structure());
        shape.decision = Some(DecisionId::new("ADR-014").expect("valid"));

        config(vec![named, shape]).with_decisions(vec![CompiledDecision {
            scope: None,
            why_not_enforceable: None,
            id: DecisionId::new("ADR-014").expect("valid"),
            title: "The registry resolves use cases by name".to_owned(),
            why: Some("the loader reads the directory and imports by filename".to_owned()),
            link: Some("docs/adr/014.md".to_owned()),
            status: DecisionStatus::Accepted,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            alternatives: Vec::new(),
        }])
    }

    /// Issue #100. `config explain` was the command for "what does this rule
    /// do"; the question people actually ask is "why is this like this", and
    /// that one is answered by a decision. So the argument takes either, and
    /// the two namespaces are kept apart at compile time so it never has to
    /// guess which was meant.
    #[test]
    fn a_decision_id_explains_the_decision_and_what_serves_it() {
        let text =
            rendered(&FILES, &decided(), "ADR-014", crate::report::Format::Text).expect("explains");

        assert!(
            text.contains("ADR-014 — The registry resolves use cases by name"),
            "{text}"
        );
        assert!(
            text.contains("the loader reads the directory and imports by filename"),
            "{text}"
        );
        assert!(text.contains("docs/adr/014.md"), "{text}");
        assert!(
            text.contains("usecase-name") && text.contains("usecase-shape"),
            "both rules that serve it are named: {text}"
        );
    }

    /// And it says whether the decision is currently holding, which is the
    /// half a document cannot answer. A decision whose rules all pass is being
    /// kept; one with findings against it is being broken right now.
    #[test]
    fn explaining_a_decision_says_whether_it_is_holding() {
        let text =
            rendered(&FILES, &decided(), "ADR-014", crate::report::Format::Text).expect("explains");

        assert!(
            text.contains("flags"),
            "the rules under it report what they are flagging: {text}"
        );
    }

    /// A baseline accepting everything the decision's rules flag.
    ///
    /// Written into the tree the harness builds, because that is where
    /// `Baseline::load` looks — the same file a repository commits.
    fn excusing_everything() -> (&'static str, String) {
        (
            ".archwarden/baseline.json",
            serde_json::json!({
                "version": 0,
                "accepted": [
                    { "rule": "usecase-name", "path": "src/user/create-client.use-case.ts", "note": "" },
                ],
            })
            .to_string(),
        )
    }

    fn with_baseline(files: &[(&str, &str)], baseline: &(&str, String)) -> Vec<(String, String)> {
        let mut entries: Vec<(String, String)> = files
            .iter()
            .map(|(path, body)| ((*path).to_owned(), (*body).to_owned()))
            .collect();
        entries.push((baseline.0.to_owned(), baseline.1.clone()));
        entries
    }

    fn rendered_owned(
        entries: &[(String, String)],
        config: &CompiledConfig,
        wanted: &str,
        format: crate::report::Format,
    ) -> Result<String, String> {
        let borrowed: Vec<(&str, &str)> = entries
            .iter()
            .map(|(path, body)| (path.as_str(), body.as_str()))
            .collect();
        rendered(&borrowed, config, wanted, format)
    }

    /// Issue #112. A decision can be accepted, be named by two rules, and be
    /// one this repository has never kept — because everything it flags is in
    /// the baseline. Both halves already existed and had never been joined.
    #[test]
    fn a_decision_whose_debt_is_all_excused_says_it_has_never_refused_anything() {
        let entries = with_baseline(&FILES, &excusing_everything());
        let text = rendered_owned(&entries, &decided(), "ADR-014", crate::report::Format::Text)
            .expect("explains");

        assert!(
            text.contains("the baseline excuses it"),
            "the debt is named rather than counted as a violation: {text}"
        );
        assert!(
            text.contains("has never refused anything"),
            "and the verdict is said out loud: {text}"
        );
    }

    /// The same numbers without a baseline: nothing is excused, and the
    /// verdict does not fire on a decision that is genuinely being broken.
    #[test]
    fn a_decision_with_no_baseline_is_not_accused_of_being_on_paper() {
        let text =
            rendered(&FILES, &decided(), "ADR-014", crate::report::Format::Text).expect("explains");

        assert!(!text.contains("excused"), "{text}");
        assert!(!text.contains("never refused anything"), "{text}");
        assert!(text.contains("currently breaks it"), "{text}");
    }

    /// And the rule lines carry their own share, because "87 excused" over two
    /// rules is a number nobody can act on until it is broken down.
    #[test]
    fn each_rule_says_how_much_of_the_debt_it_carries() {
        let entries = with_baseline(&FILES, &excusing_everything());
        let text = rendered_owned(&entries, &decided(), "ADR-014", crate::report::Format::Text)
            .expect("explains");

        assert!(
            text.contains("usecase-name") && text.contains("1 excused"),
            "{text}"
        );
    }

    /// A configuration where one decision replaced another and rejected two
    /// options, one of which a rule refuses.
    fn with_history() -> CompiledConfig {
        let mut named = rule("usecase-name", &["src/*"], naming());
        named.decision = Some(DecisionId::new("ADR-031").expect("valid"));

        config(vec![named]).with_decisions(vec![
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-009").expect("valid"),
                title: "The old way".to_owned(),
                why: None,
                link: None,
                status: archwarden_core::compiled::DecisionStatus::Superseded,
                supersedes: Vec::new(),
                superseded_by: vec![DecisionId::new("ADR-031").expect("valid")],
                alternatives: Vec::new(),
            },
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-031").expect("valid"),
                title: "The registry resolves use cases by name".to_owned(),
                why: None,
                link: None,
                status: DecisionStatus::Accepted,
                supersedes: vec![DecisionId::new("ADR-009").expect("valid")],
                superseded_by: Vec::new(),
                alternatives: vec![
                    archwarden_core::compiled::CompiledAlternative {
                        option: "a static registry".to_owned(),
                        why_not: "every use case had to be added to it by hand".to_owned(),
                        refused_by: Some(RuleId::new("usecase-name").expect("valid")),
                    },
                    archwarden_core::compiled::CompiledAlternative {
                        option: "decorators".to_owned(),
                        why_not: "they need a runtime nobody wanted".to_owned(),
                        refused_by: None,
                    },
                ],
            },
        ])
    }

    /// Issue #114. The argument that closed the question, before the verdict
    /// about whether it is holding: a reader deciding whether to reopen this
    /// needs to know what was already weighed.
    #[test]
    fn a_decision_lists_what_it_considered_and_rejected() {
        let text = rendered(
            &FILES,
            &with_history(),
            "ADR-031",
            crate::report::Format::Text,
        )
        .expect("explains");

        assert!(text.contains("Considered and rejected:"), "{text}");
        assert!(
            text.contains("a static registry — refused by `usecase-name`"),
            "{text}"
        );
        assert!(
            text.contains("every use case had to be added to it by hand"),
            "{text}"
        );
        assert!(
            text.contains("decorators — nothing refuses it"),
            "an option nothing stops is the one somebody takes: {text}"
        );
    }

    /// Issue #115. Both directions of the chain, because a reader arrives from
    /// either end: told to stop trusting one, or asking what a new one changed.
    #[test]
    fn a_decision_says_what_it_replaced_and_what_replaced_it() {
        let new = rendered(
            &FILES,
            &with_history(),
            "ADR-031",
            crate::report::Format::Text,
        )
        .expect("explains");
        assert!(new.contains("replaces ADR-009"), "{new}");

        let old = rendered(
            &FILES,
            &with_history(),
            "ADR-009",
            crate::report::Format::Text,
        )
        .expect("explains");
        assert!(
            old.contains("ADR-009 (superseded by ADR-031)"),
            "not just that it was replaced, but by what: {old}"
        );
    }

    /// The cheerful half, and the reason it is here at all: somebody asking
    /// about a decision is asking whether it is being kept, and debt paid
    /// against it is the answer they hoped for.
    #[test]
    fn a_decision_says_when_debt_against_it_was_paid() {
        let baseline = (
            ".archwarden/baseline.json",
            serde_json::json!({
                "version": 0,
                "accepted": [
                    { "rule": "usecase-name", "path": "src/user/create-client.use-case.ts",
                      "note": "" },
                    { "rule": "usecase-name", "path": "src/user/gone.use-case.ts", "note": "" },
                ],
            })
            .to_string(),
        );
        let entries = with_baseline(&FILES, &baseline);
        let text = rendered_owned(&entries, &decided(), "ADR-014", crate::report::Format::Text)
            .expect("explains");

        assert!(
            text.contains("1 accepted entry no longer occurs"),
            "the entry for a file that is gone is named: {text}"
        );
    }

    /// And a decision with nothing paid says nothing, rather than printing a
    /// zero on every run for every decision.
    #[test]
    fn a_decision_with_nothing_paid_says_nothing_about_it() {
        let entries = with_baseline(&FILES, &excusing_everything());
        let text = rendered_owned(&entries, &decided(), "ADR-014", crate::report::Format::Text)
            .expect("explains");

        assert!(!text.contains("no longer occur"), "{text}");
    }

    /// The JSON half carries both numbers per rule and the decision's own
    /// standing, so a consumer can compute the same verdict."""
    #[test]
    fn the_decision_json_carries_the_excused_debt() {
        let entries = with_baseline(&FILES, &excusing_everything());
        let json = rendered_owned(&entries, &decided(), "ADR-014", crate::report::Format::Json)
            .expect("explains");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        let shape = parsed["rules"]
            .as_array()
            .expect("rules")
            .iter()
            .find(|rule| rule["id"] == "usecase-name")
            .expect("the rule that fires");
        assert_eq!(shape["excused"], 1);
        assert_eq!(parsed["excused"], 1);
        assert_eq!(parsed["paid"], 0);
    }

    /// The JSON half, versioned like the rule shape and distinguishable from
    /// it: a consumer that asked about an id and got back an answer of the
    /// other kind must be able to tell.
    #[test]
    fn the_decision_json_says_what_kind_of_answer_it_is() {
        let json =
            rendered(&FILES, &decided(), "ADR-014", crate::report::Format::Json).expect("explains");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(parsed["version"], 0);
        assert_eq!(parsed["explains"], "decision");
        assert_eq!(parsed["id"], "ADR-014");
        assert_eq!(parsed["status"], "accepted");
        assert_eq!(parsed["link"], "docs/adr/014.md");
        assert_eq!(parsed["rules"][0]["id"], "usecase-name");
        assert!(parsed["rules"][0]["flags"].is_number(), "{json}");
    }

    /// And the rule answer says so too, rather than leaving a consumer to
    /// tell the two apart by which keys are present.
    #[test]
    fn the_rule_json_says_what_kind_of_answer_it_is() {
        let json = rendered(
            &FILES,
            &config(vec![rule("usecase-name", &["src/*"], naming())]),
            "usecase-name",
            crate::report::Format::Json,
        )
        .expect("explains");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(parsed["explains"], "rule");
        assert_eq!(parsed["id"], "usecase-name");
    }

    /// An id that is neither is refused with both lists, because a user who
    /// mistyped one does not know which of the two they got wrong.
    #[test]
    fn an_unknown_id_lists_the_rules_and_the_decisions() {
        let error = rendered(&FILES, &decided(), "ADR-041", crate::report::Format::Text)
            .expect_err("refuses");

        assert!(error.contains("ADR-041"), "{error}");
        assert!(error.contains("usecase-name"), "{error}");
        assert!(error.contains("ADR-014"), "{error}");
    }

    /// Explaining one decision lists the rules serving *that* decision and no
    /// others, which is the foreign key read backwards and the thing most
    /// worth getting wrong quietly.
    #[test]
    fn explaining_a_decision_lists_only_the_rules_that_serve_it() {
        let mut mine = rule("usecase-name", &["src/*"], naming());
        mine.decision = Some(DecisionId::new("ADR-014").expect("valid"));
        let mut theirs = rule("usecase-shape", &["src/*"], structure());
        theirs.decision = Some(DecisionId::new("ADR-020").expect("valid"));
        let loose = rule("unattached", &["src/*"], structure());

        let config = config(vec![mine, theirs, loose]).with_decisions(vec![
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-014").expect("valid"),
                title: "mine".to_owned(),
                why: None,
                link: None,
                status: DecisionStatus::Accepted,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                alternatives: Vec::new(),
            },
            CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-020").expect("valid"),
                title: "theirs".to_owned(),
                why: None,
                link: None,
                status: DecisionStatus::Accepted,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                alternatives: Vec::new(),
            },
        ]);

        let text =
            rendered(&FILES, &config, "ADR-014", crate::report::Format::Text).expect("explains");

        assert!(text.contains("usecase-name"), "{text}");
        assert!(
            !text.contains("usecase-shape"),
            "the other decision's: {text}"
        );
        assert!(!text.contains("unattached"), "and one serving none: {text}");
        assert!(
            text.contains("Implemented by 1 rule:"),
            "singular, and counted: {text}"
        );
    }

    /// A decision whose rules all pass says so, and one being broken says how
    /// many paths. The difference is the half a document cannot answer.
    #[test]
    fn explaining_a_decision_distinguishes_holding_from_broken() {
        let broken =
            rendered(&FILES, &decided(), "ADR-014", crate::report::Format::Text).expect("explains");
        // The count is asserted *on its rule's line*, not merely present
        // somewhere in the output: `decided()` puts two rules under one
        // decision, and a count attributed to the wrong one reads perfectly
        // and sends a reader to edit the rule that is passing.
        assert!(
            broken.contains("usecase-name (naming) — flags 1 path"),
            "{broken}"
        );
        assert!(
            broken.contains("usecase-shape (structure) — flags nothing"),
            "{broken}"
        );
        assert!(broken.contains("1 path currently breaks it."), "{broken}");
        assert!(
            !broken.contains("Nothing in this repository breaks it"),
            "{broken}"
        );

        let mut clean_rule = rule("nothing-to-say", &["docs/*"], structure());
        clean_rule.decision = Some(DecisionId::new("ADR-014").expect("valid"));
        let clean = config(vec![clean_rule]).with_decisions(vec![CompiledDecision {
            scope: None,
            why_not_enforceable: None,
            id: DecisionId::new("ADR-014").expect("valid"),
            title: "kept".to_owned(),
            why: None,
            link: None,
            status: DecisionStatus::Accepted,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            alternatives: Vec::new(),
        }]);

        let held =
            rendered(&FILES, &clean, "ADR-014", crate::report::Format::Text).expect("explains");
        assert!(held.contains("flags nothing"), "{held}");
        assert!(
            held.contains("Nothing in this repository breaks it."),
            "{held}"
        );
    }

    /// An accepted decision does not announce that it is accepted. Every
    /// explanation in a healthy repository would carry the word, which is how
    /// a line stops being read.
    #[test]
    fn explaining_an_accepted_decision_does_not_say_accepted() {
        let text =
            rendered(&FILES, &decided(), "ADR-014", crate::report::Format::Text).expect("explains");

        assert!(!text.contains("accepted"), "{text}");
    }

    /// A decision nobody enforces explains itself and says so. It is a
    /// legitimate question to ask about one — that is how you find out.
    #[test]
    fn explaining_a_decision_nothing_serves_says_nothing_serves_it() {
        let config = config(vec![rule("usecase-name", &["src/*"], naming())]).with_decisions(vec![
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

        let text =
            rendered(&FILES, &config, "ADR-020", crate::report::Format::Text).expect("explains");

        assert!(text.contains("ADR-020"), "{text}");
        assert!(
            text.contains("No rule implements it"),
            "the debt is said out loud: {text}"
        );
    }

    /// A superseded decision whose rules still fire is the state `config
    /// doctor` calls an error, and explaining it must not be quieter than
    /// that: the status is on the header where it cannot be missed.
    #[test]
    fn explaining_a_superseded_decision_leads_with_the_status() {
        let mut named = rule("usecase-name", &["src/*"], naming());
        named.decision = Some(DecisionId::new("ADR-014").expect("valid"));
        let config = config(vec![named]).with_decisions(vec![CompiledDecision {
            scope: None,
            why_not_enforceable: None,
            id: DecisionId::new("ADR-014").expect("valid"),
            title: "Replaced".to_owned(),
            why: None,
            link: None,
            status: DecisionStatus::Superseded,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            alternatives: Vec::new(),
        }]);

        let text =
            rendered(&FILES, &config, "ADR-014", crate::report::Format::Text).expect("explains");

        assert!(text.contains("superseded"), "{text}");
    }

    /// The two questions the command answers: what does this rule reach, and
    /// what is it reporting?
    #[test]
    fn it_lists_what_the_rule_covers_and_what_it_flags() {
        let (guard, root) = tree_at(&FILES);
        let config = config(vec![rule("usecase-name", &["src/*"], naming())]);
        let tree = archwarden_engine::walk::walk(&root, &config).expect("walks");
        let explanation = explain(&root, &config, &tree, "usecase-name")
            .expect("explains")
            .into_rule();
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
        let explanation = explain(&root, &config, &tree, "usecase-name")
            .expect("explains")
            .into_rule();
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
        let explanation = explain(&root, &config, &tree, "shape")
            .expect("explains")
            .into_rule();
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
    ///
    /// "Nothing is called" rather than "no rule is called", since 0.21: the
    /// argument takes a decision id too, and a message that named only rules
    /// would teach a user the other namespace does not exist. A config with no
    /// decisions still lists only its rules, because there is nothing else to
    /// list. Issue #100.
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
            "nothing is called `usecase-naming`; the configured rules are \
             `usecase-name`, `other`"
        );
    }

    /// And a configuration with neither says that instead of listing nothing.
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
            "nothing is called `anything`; this configuration has no rules and \
             no decisions"
        );
    }

    /// A config with decisions and no rules is a real state — a team writing
    /// its decisions down before enforcing any of them — and the message lists
    /// what there is.
    #[test]
    fn a_configuration_with_only_decisions_lists_those() {
        let error = rendered(
            &FILES,
            &config(Vec::new()).with_decisions(vec![CompiledDecision {
                scope: None,
                why_not_enforceable: None,
                id: DecisionId::new("ADR-1").expect("valid"),
                title: "t".to_owned(),
                why: None,
                link: None,
                status: DecisionStatus::Accepted,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                alternatives: Vec::new(),
            }]),
            "ADR-2",
            crate::report::Format::Text,
        )
        .expect_err("no such id");

        assert_eq!(
            error,
            "nothing is called `ADR-2`; the declared decisions are `ADR-1`"
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
