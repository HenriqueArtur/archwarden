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
//! **It says what to do, not just what is wrong.** `ROADMAP.md:57` asks for a
//! message that identifies the rule *and the fix*. The message carries the
//! same prose `check` prints, expectation included.

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

/// The path the tool is about to write, when the payload names one.
///
/// `None` for a tool that writes no file, which is most of them: the matcher
/// narrows the event, but a harness is free to call the hook for anything and
/// a hook that guessed would block writes it never understood.
#[must_use]
pub fn target(payload: &str) -> Option<String> {
    let event: Value = serde_json::from_str(payload).ok()?;
    event
        .get("tool_input")?
        .get("file_path")?
        .as_str()
        .map(str::to_owned)
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

/// The message a blocked write carries: what broke, what was expected, and how
/// to ask for the whole shape.
///
/// `invocation` is how archwarden can be run from here — see
/// [`crate::hooks::invocation`]. It is a parameter rather than a constant
/// because this message is read by an agent, which will run what it is told
/// verbatim, and a bare `archwarden` is not a command in a repository where it
/// is a dev dependency.
#[must_use]
pub fn explain(single: &archwarden_engine::single::Single, invocation: &str) -> String {
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
    }

    let _ = write!(
        message,
        "\nRun `{invocation} scaffold {}` for the shape it should have.\n",
        single.path
    );
    message
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn the_target_is_read_from_the_payload() {
        assert_eq!(
            target(WRITE).as_deref(),
            Some("/repo/src/user/create-client.use-case.ts")
        );
    }

    /// A tool that writes no file has no target, and a hook that guessed one
    /// would block writes it never understood.
    #[test]
    fn a_payload_without_a_path_has_no_target() {
        for payload in [
            r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#,
            r#"{"tool_name":"Write"}"#,
            r#"{"tool_input":{"file_path":42}}"#,
            "not json at all",
            "",
        ] {
            assert_eq!(target(payload), None, "{payload}");
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

    /// `ROADMAP.md:57` asks for a message identifying the rule *and the fix*.
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
                    signature_hint: None,
                },
            }],
            skipped: Vec::new(),
            unresolved_imports: Vec::new(),
        };

        let message = explain(&single, "archwarden");

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
                    signature_hint: None,
                },
            }],
            skipped: Vec::new(),
            unresolved_imports: Vec::new(),
        };

        let message = explain(&single, "npx archwarden");

        assert!(
            message.contains("npx archwarden scaffold src/user/create-client.use-case.ts"),
            "{message}"
        );
    }
}
