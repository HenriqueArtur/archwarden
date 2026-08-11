//! `archwarden hook claude-code` — answering a harness's pre-write question.
//!
//! The command `install-hooks` puts in the harness's path. It reads the
//! `PreToolUse` event from stdin, finds what the tool is about to write, and
//! answers in the protocol Claude Code understands.
//!
//! # Two rules this obeys
//!
//! **It never blocks by failing.** A hook that crashed and took the user's
//! write with it would be worse than no hook, so every unexpected shape --
//! unreadable payload, a tool with no path, a broken config -- allows the
//! write and says why. Blocking is a decision expressed in the response, never
//! a side effect of something going wrong.
//!
//! **It says what to do, not just what is wrong.** A denial names the rule
//! *and the fix*: the message carries the same prose `check` prints,
//! expectation included.

use serde_json::{Value, json};

/// The event this command answers.
const EVENT: &str = "PreToolUse";

/// What the hook decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Let the write through, with nothing to say.
    Allow,
    /// Let the write through, but tell the user something.
    Note(String),
    /// Refuse the write, with the reason.
    Deny(String),
}

/// Which question the harness is asking.
///
/// One command answers both, dispatching on what it was sent. Two commands
/// would let a hook be wired to the wrong event, and a pre-write answer to a
/// stop event — or the reverse — is a hook that reports nothing while looking
/// installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A write is about to happen. Answer whether it would be legal.
    PreToolUse,
    /// The turn is over. Say what landed.
    ///
    /// The pre-write hook sees one write at a time and is structurally unable
    /// to judge a rule about a group — a `presence` rule makes every write in
    /// the group illegal until the whole group exists, so no order passes.
    /// This is where that class is caught. Issue #61.
    Stop,
    /// Something this build has no answer for.
    Other,
}

/// Which event the payload announces.
///
/// Absent means [`PreToolUse`](Event::PreToolUse): that is what every hook
/// installed before this sent, and a harness that stops sending the field must
/// not silently change what the hook does.
#[must_use]
pub fn event(payload: &str) -> Event {
    let Ok(parsed) = serde_json::from_str::<Value>(payload) else {
        return Event::PreToolUse;
    };

    match parsed.get("hook_event_name").and_then(Value::as_str) {
        None | Some(EVENT) => Event::PreToolUse,
        Some("Stop") => Event::Stop,
        Some(_) => Event::Other,
    }
}

/// What the payload had to say about a file.
///
/// Three answers rather than two, because [`NoFile`](Target::NoFile) and
/// [`Unreadable`](Target::Unreadable) were one `Option::None` once and are not
/// the same thing at all. One is "this tool writes nothing, carry on"; the
/// other is "I could not read what you sent me". Both let the write through,
/// and only the first should do it without saying a word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The path the tool is about to write.
    Path(String),
    /// A readable event for a tool that writes no file, which is most of them:
    /// the matcher narrows what arrives, but a harness is free to call the hook
    /// for anything and a hook that guessed would block writes it never
    /// understood.
    NoFile,
    /// Not an event this hook can read. A misconfigured harness, a protocol
    /// that moved, a wrapper sending something else — the hook cannot tell
    /// those apart, and every one of them means it is judging nothing.
    Unreadable,
}

/// Reads the payload the harness sent.
#[must_use]
pub fn target(payload: &str) -> Target {
    let Ok(event) = serde_json::from_str::<Value>(payload) else {
        return Target::Unreadable;
    };
    // An event is an object. A bare array or string is valid JSON and is not
    // this protocol, which is the same "I cannot read this" as a syntax error.
    if !event.is_object() {
        return Target::Unreadable;
    }

    event
        .get("tool_input")
        .and_then(|input| input.get("file_path"))
        .and_then(Value::as_str)
        .map_or(Target::NoFile, |path| Target::Path(path.to_owned()))
}

/// The text the write would leave at the target, reconstructed from the event.
///
/// A `PreToolUse` hook is asked whether a write *would* be legal, and the
/// answer used to come from the bytes already on disk — the previous version
/// of the file. So a new file was never checked at all, an edit introducing a
/// violation was permitted, and an edit *fixing* one was refused while naming
/// a rule the pending write already satisfied. That last one has no way out
/// from inside an agent loop. Issue #55.
///
/// `on_disk` is what is there now, which `Edit` and `MultiEdit` need because
/// they send a replacement rather than a document.
///
/// `None` when the event is a tool this cannot replay, or an edit that does not
/// apply. Both mean *"judge the file as it is"*, and the caller says so rather
/// than pretending it read the write.
#[must_use]
pub fn pending(payload: &str, on_disk: &str) -> Option<String> {
    let event: Value = serde_json::from_str(payload).ok()?;
    let input = event.get("tool_input")?;

    // A whole document, which is the easy case and the common one.
    if let Some(content) = input.get("content").and_then(Value::as_str) {
        return Some(content.to_owned());
    }

    match event.get("tool_name").and_then(Value::as_str) {
        Some("Edit") => apply(
            on_disk,
            input.get("old_string")?.as_str()?,
            input.get("new_string")?.as_str()?,
            input
                .get("replace_all")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
        Some("MultiEdit") => {
            let edits = input.get("edits")?.as_array()?;
            edits.iter().try_fold(on_disk.to_owned(), |text, edit| {
                apply(
                    &text,
                    edit.get("old_string")?.as_str()?,
                    edit.get("new_string")?.as_str()?,
                    edit.get("replace_all")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                )
            })
        }
        // A tool whose replacement this does not know how to replay. Guessing
        // would answer about a file the write never produces.
        _ => None,
    }
}

/// One replacement, or `None` when there is nothing to replace.
///
/// `None` rather than the unchanged text: the harness refuses an edit whose
/// `old_string` is not there, so judging the file as though it had applied
/// answers about a write that will not happen — and falling back to the file on
/// disk is the very bug this exists to fix, reached by another route.
///
/// The first occurrence is the occurrence: Claude Code requires `old_string` to
/// be unique in the file unless `replace_all` is set, and enforcing that a
/// second time here would refuse writes the harness accepts.
fn apply(text: &str, old: &str, new: &str, all: bool) -> Option<String> {
    if !text.contains(old) {
        return None;
    }
    Some(if all {
        text.replace(old, new)
    } else {
        text.replacen(old, new, 1)
    })
}

/// Renders a decision in Claude Code's hook protocol.
///
/// The deny shape is the one the official plugins emit:
/// `hookSpecificOutput.permissionDecision`, with the explanation in
/// `systemMessage`.
#[must_use]
pub fn respond(decision: &Decision) -> String {
    let response = match decision {
        Decision::Allow => json!({}),
        Decision::Note(message) => json!({ "systemMessage": message }),
        Decision::Deny(reason) => json!({
            "hookSpecificOutput": {
                "hookEventName": EVENT,
                "permissionDecision": "deny",
            },
            "systemMessage": reason,
        }),
    };

    // An empty object rather than an empty string even when there is nothing
    // to say: Claude Code logs "output does not start with {" for every tool
    // call otherwise, and a hook that fills someone's debug log is a hook they
    // uninstall.
    format!("{response}\n")
}

/// Whether this write is fixing the thing the finding is about.
///
/// A `presence` rule requiring several files makes every one of them illegal
/// until all of them exist: writing the first is refused for the absence of the
/// second, the second for the absence of the third, and no order passes. The
/// directory cannot be created at all. Issue #57.
///
/// The write is not what is wrong there. Writing `projeto.md` violates nothing
/// — the *directory* is incomplete, and it was incomplete before the write and
/// is less so after. Refusing it attributes a directory's fault to a file, and
/// refuses the write that improves the state, which is #55 one layer up.
///
/// So a write supplying one of the required files is **progress**, and passes
/// with a note. A write that supplies none of them leaves the directory exactly
/// as broken as it found it, and is refused as before — which is what keeps
/// this from being a way to switch `presence` off.
///
/// Only findings about a missing *required file* qualify. `spec-pair` has an
/// order that works — the spec first, which is what a TDD gate is for — and a
/// `structure` violation is caused by the write rather than pre-existing it.
#[must_use]
pub fn is_progress(finding: &archwarden_core::finding::Finding, written: &str) -> bool {
    use archwarden_core::finding::Expectation;

    let Expectation::RequiredFiles { names, patterns } = &finding.expected else {
        return false;
    };

    if names.iter().any(|name| name == written) {
        return true;
    }

    // Compiled with the engine that compiled the rule, so this cannot disagree
    // with `check` about whether a name satisfies the pattern.
    patterns.iter().any(|pattern| {
        archwarden_core::pattern::Pattern::compile(pattern)
            .is_ok_and(|compiled| compiled.is_match(written))
    })
}

/// What a write is still short of, when the write itself is fine.
///
/// A `presence` rule's finding is about the *directory*, and a write supplying
/// one of its required files is fixing that directory rather than breaking it.
/// Saying "would break these rules" about such a write is false — and it buries
/// the one thing worth saying, which is what to write next.
#[must_use]
pub fn still_needs(fixing: &[archwarden_core::finding::Finding]) -> String {
    use std::collections::BTreeSet;
    use std::fmt::Write as _;

    let mut wanted: BTreeSet<String> = BTreeSet::new();
    for finding in fixing {
        wanted.insert(crate::report::describe_observed(&finding.observed));
    }

    let mut message =
        String::from("archwarden: this write is fine, and the directory is not done yet.\n");
    for item in &wanted {
        let _ = writeln!(message, "\n  {item}");
    }
    message
}

/// The message the end of a turn carries: what landed that a rule objects to.
///
/// Different from [`explain`] in what it is for. That one is handed to an agent
/// about to be refused, and names one file and the shape it should have had.
/// This one is read after the fact, about a set of files, and its job is to be
/// short enough that somebody reads it.
///
/// Grouped by rule rather than by file: a `presence` rule that fired on four
/// directories is one thing to fix, and four lines saying the same thing is a
/// message people learn to skip.
#[must_use]
pub fn landed(
    findings: &[archwarden_core::finding::Finding],
    reasons: &crate::report::Reasons,
) -> String {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

    // The level is stored with the group rather than read back from its first
    // member: taking it from `first()` needs an `expect` for a case that cannot
    // happen, and a panic that cannot happen is still a panic in a hook.
    type Group<'a> = (
        archwarden_core::level::Level,
        Vec<&'a archwarden_core::finding::Finding>,
    );
    let mut by_rule: BTreeMap<&archwarden_core::ids::RuleId, Group<'_>> = BTreeMap::new();
    for finding in findings {
        by_rule
            .entry(&finding.rule_id)
            .or_insert_with(|| (finding.level, Vec::new()))
            .1
            .push(finding);
    }

    let mut message = format!(
        "archwarden: {} {} landed in this turn.\n",
        findings.len(),
        if findings.len() == 1 {
            "finding"
        } else {
            "findings"
        },
    );

    for (rule, (level, found)) in &by_rule {
        let _ = write!(message, "\n  [{level}] {rule}\n");

        // Each finding keeps its own observation. Grouping by rule and showing
        // only the first one printed the same sentence twice for a directory
        // missing two files, and never mentioned the second -- a shorter
        // message that leaves out what is wrong is not shorter, it is wrong.
        for finding in found {
            let _ = writeln!(
                message,
                "    {} — {}",
                finding.path,
                crate::report::describe_observed(&finding.observed),
            );
        }
        if let Some(why) = reasons.of_rule(rule) {
            let _ = writeln!(message, "  why: {why}");
        }
    }

    message
}

/// The message a blocked write carries: what broke, what was expected, and how
/// to ask for the whole shape.
///
/// `invocation` is how archwarden can be run from here — see
/// [`crate::hooks::invocation`]. It is a parameter rather than a constant
/// because this message is read by an agent, which will run what it is told
/// verbatim, and a bare `archwarden` is not a command in a repository where it
/// is a dev dependency.
#[must_use]
pub fn explain(
    single: &archwarden_engine::single::Single,
    reasons: &crate::report::Reasons,
    invocation: &str,
) -> String {
    use std::fmt::Write as _;

    let mut message = format!("archwarden: `{}` would break these rules.\n", single.path);

    for finding in &single.findings {
        let _ = write!(
            message,
            "\n  [{}] {} — {}\n  expected: {}\n",
            finding.level,
            finding.rule_id,
            crate::report::describe_observed(&finding.observed),
            crate::report::describe_expectation(&finding.expected),
        );
        // The highest-value place a reason can appear: this is the moment an
        // agent decides between complying and working around, and a denial
        // that says only what is forbidden is one it argues with. Issue #46.
        if let Some(why) = reasons.of_rule(&finding.rule_id) {
            let _ = writeln!(message, "  why: {why}");
        }
    }

    let _ = write!(
        message,
        "\nRun `{invocation} scaffold {}` for the shape it should have.\n",
        single.path
    );
    message
}

/// Why the hook formed no opinion about a write, in the words it says it in.
///
/// The hook cannot report a config problem the way `check` does — it must
/// answer in JSON and exit clean, where `check` writes a miette report to
/// stderr and exits 2. That difference used to be enough to make this surface
/// re-implement the whole of `prepare()` rather than reuse it, and the missing
/// version guard of issue #55 was in the copy.
///
/// This is what the difference costs now: one function, matching on the value
/// the shared operation returned. A stage added later cannot go unhandled here
/// — [`archwarden_api::Error`] is `non_exhaustive`, so it lands in the final
/// arm and the write is still reported as unchecked.
///
/// No sentence here ends in "so this write was not checked against any rule".
/// The caller already says that, and saying it twice in one line was what four
/// separately-written messages had drifted into.
#[must_use]
pub fn unexamined(error: &archwarden_api::Error) -> String {
    match error {
        archwarden_api::Error::Load(archwarden_config::discovery::LoadError::NotFound {
            ..
        }) => "no archwarden config was found from here".to_owned(),

        // Found, and unusable. Distinct from the arm above because the two
        // send a user to different places: one to `archwarden init`, the other
        // to the file they just edited.
        archwarden_api::Error::Load(_) => {
            "the config could not be read — `archwarden config validate` names the problem"
                .to_owned()
        }

        archwarden_api::Error::UnsupportedVersion {
            declared,
            understood,
            ..
        } => format!(
            "the config declares version {declared}, which this build does not understand \
             (it reads version {understood})"
        ),

        archwarden_api::Error::Extends(_) => {
            "the config could not be assembled (a preset it extends is missing, invalid, \
             or loops)"
                .to_owned()
        }

        archwarden_api::Error::Compile(_) => {
            "the config did not compile — `archwarden config validate` names the problem".to_owned()
        }

        _ => "the config could not be prepared".to_owned(),
    }
}

#[cfg(test)]
mod unexamined_tests {
    use super::unexamined;
    use archwarden_api::Error;
    use archwarden_config::discovery::LoadError;
    use camino::Utf8PathBuf;

    /// The sentence that used to be wrong. The hook rendered every load
    /// failure as "no config was found", so a user who had just introduced a
    /// syntax error was sent looking for a missing file — while the file sat
    /// there, found, broken, and named by the error the loader returned.
    ///
    /// It was not carelessness either: the prose was written by hand beside a
    /// re-implementation of the orchestration, because the real path wrote a
    /// miette report to stderr and the hook cannot answer that way. One enum
    /// with the distinction in it is what makes the right sentence available.
    #[test]
    fn a_config_that_is_there_and_broken_is_not_a_config_that_is_missing() {
        let broken = Error::Load(
            archwarden_config::discovery::parse(
                camino::Utf8Path::new("/repo/arch.config.json"),
                r#"{"version": 0,,}"#,
            )
            .expect_err("should not parse"),
        );

        assert_eq!(
            unexamined(&broken),
            "the config could not be read — `archwarden config validate` names the problem"
        );
    }

    #[test]
    fn a_config_that_really_is_missing_says_so() {
        let absent = Error::Load(LoadError::NotFound {
            started_at: Utf8PathBuf::from("/repo/packages/app"),
        });

        assert_eq!(
            unexamined(&absent),
            "no archwarden config was found from here"
        );
    }

    /// Issue #55's sentence. The guard it reports was missing from this
    /// surface entirely, because this surface had its own copy of the path.
    #[test]
    fn a_future_version_names_both_numbers() {
        let future = Error::UnsupportedVersion {
            path: Utf8PathBuf::from("/repo/arch.config.json"),
            declared: 99,
            understood: 0,
        };

        assert_eq!(
            unexamined(&future),
            "the config declares version 99, which this build does not understand \
             (it reads version 0)"
        );
    }

    /// A config that parsed and will not compile sends the reader to the
    /// command that names the offending rule, because the error itself is
    /// about a glob or a pattern and the hook has one line to spend.
    #[test]
    fn a_config_that_did_not_compile_names_the_command_that_explains_it() {
        let uncompilable = Error::Compile(archwarden_config::compile::CompileError::Pattern {
            rule: archwarden_core::ids::RuleId::new("lookahead").expect("valid id"),
            field: "file_pattern",
            source: Box::new(
                archwarden_core::pattern::Pattern::compile("^(?!test).*$")
                    .expect_err("a lookahead is not linear-time"),
            ),
        });

        assert_eq!(
            unexamined(&uncompilable),
            "the config did not compile — `archwarden config validate` names the problem"
        );
    }

    #[test]
    fn a_preset_problem_says_which_half_of_the_config_failed() {
        let unresolvable = Error::Extends(archwarden_config::extends::ExtendsError::Cycle {
            chain: vec![Utf8PathBuf::from("/repo/arch.config.json")],
        });

        assert_eq!(
            unexamined(&unresolvable),
            "the config could not be assembled (a preset it extends is missing, invalid, \
             or loops)"
        );
    }
}

#[cfg(test)]
mod tests {
    use archwarden_core::{
        facts::KindFilter,
        finding::{Expectation, Finding, Observed},
        ids::RuleId,
        level::Level,
        path::RepoRelPath,
    };

    use super::*;

    /// The payload shape a `Write` produces.
    const WRITE: &str = r#"{
        "session_id": "abc",
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": {
            "file_path": "/repo/src/user/create-client.use-case.ts",
            "content": "export const CreateClient = () => {};"
        }
    }"#;

    /// Issue #46's highest-value surface: the moment an agent decides between
    /// complying and working around. A denial that says only what is forbidden
    /// is a denial an agent argues with.
    #[test]
    fn a_denial_carries_the_reason_the_rule_exists() {
        let single = archwarden_engine::single::Single {
            path: RepoRelPath::new("src/user/create-client.use-case.ts").expect("valid"),
            findings: vec![Finding {
                rule_id: RuleId::new("usecase-name").expect("valid"),
                module_id: None,
                level: Level::Error,
                path: RepoRelPath::new("src/user/create-client.use-case.ts").expect("valid"),
                span: None,
                observed: Observed::ExportMissing {
                    name: "CreateClient".to_owned(),
                },
                expected: Expectation::RequiredExport {
                    kind: KindFilter::Any,
                    name: "CreateClient".to_owned(),
                    annotation: Vec::new(),
                    signature_hint: None,
                },
            }],
            skipped: Vec::new(),
            unresolved_imports: Vec::new(),
        };
        let reasons = crate::report::Reasons::from([(
            "usecase-name",
            "the registry imports these by name, not by path",
        )]);

        let message = explain(&single, &reasons, "npx archwarden");

        assert!(
            message.contains("why: the registry imports these by name, not by path"),
            "{message}"
        );
    }

    #[test]
    fn the_target_is_read_from_the_payload() {
        assert_eq!(
            target(WRITE),
            Target::Path("/repo/src/user/create-client.use-case.ts".to_owned())
        );
    }

    /// A tool that writes no file has no target, and a hook that guessed one
    /// would block writes it never understood.
    ///
    /// Silence is the right answer here and only here: with a broader matcher
    /// this is every `Bash`, every `Read`, every `Grep`. A message on each one
    /// is a hook somebody uninstalls by lunchtime.
    #[test]
    fn a_tool_that_writes_no_file_is_passed_over_in_silence() {
        for payload in [
            r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#,
            r#"{"tool_name":"Write"}"#,
            r#"{"tool_input":{"file_path":42}}"#,
        ] {
            assert_eq!(target(payload), Target::NoFile, "{payload}");
        }
    }

    /// And a payload that is not an event at all is a different answer.
    ///
    /// "This tool writes nothing" and "I could not read what you sent me" were
    /// the same `None` once, and both permitted in silence — so a misconfigured
    /// hook looked exactly like a working one. Only one of those two is safe to
    /// pass over without a word.
    #[test]
    fn an_unreadable_payload_is_not_the_same_as_nothing_to_do() {
        for payload in ["not json at all", "", "[1, 2, 3]", "\"a string\""] {
            assert_eq!(target(payload), Target::Unreadable, "{payload}");
        }
    }

    /// Nothing to say is still valid JSON. An empty stdout makes Claude Code
    /// log "output does not start with {" on every tool call, and a hook that
    /// fills someone's debug log is a hook they uninstall.
    #[test]
    fn allowing_emits_an_empty_object() {
        assert_eq!(respond(&Decision::Allow), "{}\n");
    }

    /// The deny shape the official plugins emit.
    #[test]
    fn denying_uses_the_documented_protocol() {
        let response = respond(&Decision::Deny("nope".to_owned()));
        let parsed: Value = serde_json::from_str(&response).expect("valid JSON");

        assert_eq!(parsed["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(parsed["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(parsed["systemMessage"], "nope");
    }

    /// A note is shown without blocking, which is what a warning-level finding
    /// deserves: decision 1 says warnings are visible and do not gate.
    #[test]
    fn a_note_is_shown_without_denying() {
        let response = respond(&Decision::Note("careful".to_owned()));
        let parsed: Value = serde_json::from_str(&response).expect("valid JSON");

        assert_eq!(parsed["systemMessage"], "careful");
        assert!(
            parsed.get("hookSpecificOutput").is_none(),
            "a note does not block: {response}"
        );
    }

    /// A denial identifies the rule *and the fix*, or an agent can only guess.
    #[test]
    fn the_message_names_the_rule_and_the_expectation() {
        use archwarden_core::{
            facts::{ExportKind, ExportTags, KindFilter},
            finding::{Expectation, Finding, Observed},
            ids::RuleId,
            level::Level,
            path::RepoRelPath,
        };

        let path = RepoRelPath::new("src/user/create-client.use-case.ts").expect("valid");
        let single = archwarden_engine::single::Single {
            path: path.clone(),
            findings: vec![Finding {
                rule_id: RuleId::new("usecase-name").expect("valid"),
                module_id: None,
                level: Level::Error,
                path,
                span: None,
                observed: Observed::ExportWrongKind {
                    name: "CreateClient".to_owned(),
                    found: ExportTags::only(ExportKind::Arrow),
                },
                expected: Expectation::RequiredExport {
                    kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
                    name: "CreateClient".to_owned(),
                    annotation: Vec::new(),
                    signature_hint: None,
                },
            }],
            skipped: Vec::new(),
            unresolved_imports: Vec::new(),
        };

        let message = explain(&single, &crate::report::Reasons::default(), "archwarden");

        assert!(message.contains("usecase-name"), "the rule: {message}");
        assert!(message.contains("CreateClient"), "the symbol: {message}");
        assert!(message.contains("expected:"), "the fix: {message}");
        assert!(
            message.contains("archwarden scaffold src/user/create-client.use-case.ts"),
            "where to get the whole shape: {message}"
        );
    }

    /// The suggestion has to be a command the reader can run. In a repository
    /// where archwarden is a dev dependency, a bare `archwarden` is not one —
    /// and this message is read by an agent, which will try it verbatim.
    #[test]
    fn the_suggestion_uses_the_invocation_it_is_given() {
        use archwarden_core::{
            facts::{ExportKind, ExportTags, KindFilter},
            finding::{Expectation, Finding, Observed},
            ids::RuleId,
            level::Level,
            path::RepoRelPath,
        };
        use archwarden_engine::single::Single;

        let path = RepoRelPath::new("src/user/create-client.use-case.ts").expect("valid");
        let single = Single {
            path: path.clone(),
            findings: vec![Finding {
                rule_id: RuleId::new("usecase-name").expect("valid"),
                module_id: None,
                level: Level::Error,
                path,
                span: None,
                observed: Observed::ExportMissing {
                    name: "CreateClient".to_owned(),
                },
                expected: Expectation::RequiredExport {
                    kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
                    name: "CreateClient".to_owned(),
                    annotation: Vec::new(),
                    signature_hint: None,
                },
            }],
            skipped: Vec::new(),
            unresolved_imports: Vec::new(),
        };

        let message = explain(
            &single,
            &crate::report::Reasons::default(),
            "npx archwarden",
        );

        assert!(
            message.contains("npx archwarden scaffold src/user/create-client.use-case.ts"),
            "{message}"
        );
    }
    /// A `Write` carries the whole file, and that is what gets judged.
    #[test]
    fn a_write_carries_its_own_content() {
        let event = r#"{"tool_name":"Write","tool_input":{
            "file_path":"/repo/a.ts","content":"export const A = 1;"}}"#;

        assert_eq!(
            pending(event, "on disk, and irrelevant"),
            Some("export const A = 1;".to_owned())
        );
    }

    /// An `Edit` carries a replacement, so the text has to be reconstructed
    /// from what is on disk. Claude Code requires `old_string` to be unique,
    /// so the first occurrence is the occurrence.
    #[test]
    fn an_edit_is_applied_to_what_is_there() {
        let event = r#"{"tool_name":"Edit","tool_input":{
            "file_path":"/repo/a.ts","old_string":"Wrong","new_string":"Right"}}"#;

        assert_eq!(
            pending(event, "export function Wrong() {}"),
            Some("export function Right() {}".to_owned())
        );
    }

    /// `replace_all` is a flag on the same tool, and ignoring it would judge a
    /// file the write never produces.
    #[test]
    fn an_edit_replacing_all_replaces_all() {
        let event = r#"{"tool_name":"Edit","tool_input":{
            "file_path":"/repo/a.ts","old_string":"a","new_string":"b",
            "replace_all":true}}"#;

        assert_eq!(pending(event, "a-a-a"), Some("b-b-b".to_owned()));
    }

    /// `MultiEdit` applies its edits in order, each to the result of the last.
    #[test]
    fn a_multi_edit_applies_its_edits_in_order() {
        let event = r#"{"tool_name":"MultiEdit","tool_input":{
            "file_path":"/repo/a.ts","edits":[
                {"old_string":"one","new_string":"two"},
                {"old_string":"two","new_string":"three"}]}}"#;

        assert_eq!(pending(event, "one"), Some("three".to_owned()));
    }

    /// An edit whose `old_string` is not there produces no text at all.
    ///
    /// `None` rather than the unchanged file: the harness will refuse this
    /// edit itself, and judging the file as if the edit had applied would
    /// answer about a write that is not going to happen. Falling back to disk
    /// is the old bug by another route.
    #[test]
    fn an_edit_that_does_not_apply_yields_nothing() {
        let event = r#"{"tool_name":"Edit","tool_input":{
            "file_path":"/repo/a.ts","old_string":"absent","new_string":"x"}}"#;

        assert_eq!(pending(event, "export function Wrong() {}"), None);
    }

    /// A tool this hook does not know how to replay yields nothing, and the
    /// caller falls back to the file on disk rather than guessing.
    #[test]
    fn an_unknown_tool_yields_no_pending_text() {
        let event = r#"{"tool_name":"NotebookEdit","tool_input":{
            "file_path":"/repo/a.ipynb","cell":"3"}}"#;

        assert_eq!(pending(event, "whatever"), None);
    }
    /// The harness says which event it is sending, and one command answers
    /// both. Issue #61.
    #[test]
    fn the_event_is_read_from_the_payload() {
        assert_eq!(event(WRITE), Event::PreToolUse);
        assert_eq!(
            event(r#"{"hook_event_name":"Stop","session_id":"abc"}"#),
            Event::Stop
        );
    }

    /// A payload with no event name is the pre-write one: that is what every
    /// installed hook sent before this existed, and a harness that stops
    /// sending the field must not silently switch behaviour.
    #[test]
    fn a_payload_with_no_event_name_is_the_pre_write_one() {
        assert_eq!(
            event(r#"{"tool_input":{"file_path":"/repo/a.ts"}}"#),
            Event::PreToolUse
        );
    }

    /// An event this build does not know is not guessed at. A harness that
    /// grows a new event would otherwise have it answered as though it were a
    /// write.
    #[test]
    fn an_event_this_build_does_not_know_is_named_as_such() {
        assert_eq!(event(r#"{"hook_event_name":"SessionStart"}"#), Event::Other);
    }
    /// Issue #57. A `presence` rule requiring several files makes every one of
    /// them illegal until all of them exist, so no write order passes and the
    /// directory cannot be created at all.
    ///
    /// The rigorous reading, and the one implemented: **a write passes while it
    /// is fixing the problem.** Judged by what the write does, not by the state
    /// it lands in — the same correction as #55, one layer up.
    #[test]
    fn a_write_that_supplies_a_required_file_is_progress() {
        let missing = |name: &str| Finding {
            rule_id: RuleId::new("tem-os-tres").expect("valid"),
            module_id: None,
            level: Level::Error,
            path: RepoRelPath::new("projetos/02-novo").expect("valid"),
            span: None,
            observed: Observed::RequiredFileMissing {
                name: name.to_owned(),
            },
            expected: Expectation::RequiredFiles {
                names: vec![
                    "projeto.md".to_owned(),
                    "exercicios.md".to_owned(),
                    "diagram.json".to_owned(),
                ],
                patterns: Vec::new(),
            },
        };

        assert!(
            is_progress(&missing("exercicios.md"), "projeto.md"),
            "writing one of the required files is fixing the directory"
        );
        assert!(
            is_progress(&missing("diagram.json"), "exercicios.md"),
            "and so is the second one"
        );
    }

    /// A write that ignores the problem is still refused. This is the half that
    /// keeps the relaxation from being a way to switch `presence` off.
    #[test]
    fn a_write_that_ignores_the_missing_files_is_not_progress() {
        let finding = Finding {
            rule_id: RuleId::new("tem-os-tres").expect("valid"),
            module_id: None,
            level: Level::Error,
            path: RepoRelPath::new("projetos/01-blink").expect("valid"),
            span: None,
            observed: Observed::RequiredFileMissing {
                name: "diagram.json".to_owned(),
            },
            expected: Expectation::RequiredFiles {
                names: vec!["projeto.md".to_owned(), "diagram.json".to_owned()],
                patterns: Vec::new(),
            },
        };

        assert!(
            !is_progress(&finding, "rascunho.md"),
            "a file the rule never asked for leaves the directory as broken as it was"
        );
    }

    /// A `require_any` entry is a regex, and a file matching one is progress
    /// the same way a named file is.
    #[test]
    fn a_write_matching_a_required_pattern_is_progress() {
        let finding = Finding {
            rule_id: RuleId::new("tem-um-ino").expect("valid"),
            module_id: None,
            level: Level::Error,
            path: RepoRelPath::new("projetos/02-novo/sketch").expect("valid"),
            span: None,
            observed: Observed::NoFileMatching {
                pattern: r"\.ino$".to_owned(),
            },
            expected: Expectation::RequiredFiles {
                names: Vec::new(),
                patterns: vec![r"\.ino$".to_owned()],
            },
        };

        assert!(is_progress(&finding, "sketch.ino"));
        assert!(!is_progress(&finding, "leiame.md"));
    }

    /// Every other rule keeps denying. `spec-pair` has an order that works —
    /// the spec first, which is the whole point of a TDD gate — and a
    /// `structure` violation is caused by the write rather than pre-existing
    /// it.
    #[test]
    fn a_finding_that_is_not_about_a_missing_file_is_never_progress() {
        let finding = Finding {
            rule_id: RuleId::new("usecase-name").expect("valid"),
            module_id: None,
            level: Level::Error,
            path: RepoRelPath::new("src/user/create.use-case.ts").expect("valid"),
            span: None,
            observed: Observed::ExportMissing {
                name: "Create".to_owned(),
            },
            expected: Expectation::RequiredExport {
                kind: KindFilter::Any,
                name: "Create".to_owned(),
                annotation: Vec::new(),
                signature_hint: None,
            },
        };

        assert!(!is_progress(&finding, "create.use-case.ts"));
    }
    /// The note a progress write carries says what is still missing, which is
    /// what the agent has to write next. "would break these rules" is false
    /// about a write that is fixing the directory. Issue #57.
    #[test]
    fn the_progress_note_names_what_is_still_missing() {
        let missing = |name: &str| Finding {
            rule_id: RuleId::new("tem-os-tres").expect("valid"),
            module_id: None,
            level: Level::Error,
            path: RepoRelPath::new("projetos/02-novo").expect("valid"),
            span: None,
            observed: Observed::RequiredFileMissing {
                name: name.to_owned(),
            },
            expected: Expectation::RequiredFiles {
                names: vec!["exercicios.md".to_owned()],
                patterns: Vec::new(),
            },
        };

        let message = still_needs(&[missing("exercicios.md"), missing("diagram.json")]);

        assert!(
            message.contains("this write is fine"),
            "it must not read as a complaint about the write: {message}"
        );
        assert!(message.contains("exercicios.md"), "{message}");
        assert!(message.contains("diagram.json"), "{message}");
        assert!(
            !message.contains("would break"),
            "the write breaks nothing: {message}"
        );
    }

    /// The same file named twice is said once. A directory missing one file
    /// under two rules would otherwise repeat itself.
    #[test]
    fn the_progress_note_does_not_repeat_itself() {
        let same = || Finding {
            rule_id: RuleId::new("tem-os-tres").expect("valid"),
            module_id: None,
            level: Level::Error,
            path: RepoRelPath::new("projetos/02-novo").expect("valid"),
            span: None,
            observed: Observed::RequiredFileMissing {
                name: "exercicios.md".to_owned(),
            },
            expected: Expectation::RequiredFiles {
                names: vec!["exercicios.md".to_owned()],
                patterns: Vec::new(),
            },
        };

        let message = still_needs(&[same(), same()]);

        assert_eq!(message.matches("exercicios.md").count(), 1, "{message}");
    }
}
