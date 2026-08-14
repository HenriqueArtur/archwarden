//! archwarden as an MCP server: JSON-RPC over stdio, and nothing of its own.
//!
//! # Why this is a crate and not a module
//!
//! Issue #65 called MCP *"the proof the boundary is in the right place: if it
//! needs anything not in `archwarden-api`, it is not."* Building it proved the
//! opposite of what decision 20 claimed — `describe`, `scaffold`, the digest
//! and the whole of what a pre-write check *means* all lived in
//! `archwarden-cli`, because the CLI was the only surface asking. They moved,
//! and this crate is what keeps them moved: it depends on `archwarden-api` and
//! it cannot see `archwarden-cli`, so a question answered from the wrong place
//! does not compile rather than passing review.
//!
//! The binary is still one. `archwarden-cli` depends on this and dispatches
//! `archwarden mcp` into [`serve`], because issue #65 is explicit that MCP adds
//! **no new installation requirement**: a committable `.mcp.json` names the
//! same `./node_modules/.bin/archwarden` the pre-write hook already resolves.
//!
//! # The transport
//!
//! Not HTTP. The client spawns the binary and speaks JSON-RPC 2.0 over its
//! pipes, one message per line. No port, no daemon, nothing listening — which
//! is also why there is no async runtime here: a loop over stdin serving one
//! request at a time is what the transport is.
//!
//! # Two rules it obeys
//!
//! **It re-reads the configuration on every call.** A long-lived process that
//! prepared its rules at startup would answer from a config the user has since
//! edited, and be confidently wrong for the rest of the session. That is issue
//! #55 in a new place, and the cost is one config load per call against a
//! process that is otherwise idle.
//!
//! **It never dies on a bad message.** An unknown method, an unparsable line, a
//! call with the wrong arguments — each is an error *in the protocol*, because
//! a server that exited would take the client's session with it and the user
//! would learn about it as tools silently disappearing.

use archwarden_api::{Location, describe, scaffold, single};
use camino::Utf8Path;
use serde_json::{Value, json};

/// The MCP protocol version this speaks.
///
/// Echoed back at `initialize` rather than negotiated: this server has one
/// shape, and a client that wants another is better told plainly what it got.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// JSON-RPC's own code for a method that does not exist.
const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC's own code for a message that is not a valid request.
const INVALID_REQUEST: i64 = -32600;
/// JSON-RPC's own code for a message that is not valid JSON.
const PARSE_ERROR: i64 = -32700;

/// The id this server uses for the one request it makes of its own.
///
/// A server that asks the client something has to recognise the answer coming
/// back, and JSON-RPC correlates by id. One fixed id is enough because there
/// is one question and it is never in flight twice — a second `roots/list`
/// replaces the first answer, which is what the client's `listChanged`
/// notification is asking for anyway.
///
/// Far from anything a client would pick for its own requests: ids are its
/// namespace and ours, and collisions there are silent.
const ROOTS_REQUEST_ID: i64 = -1;

/// What the client says about where the repository is.
///
/// A client on the host and this server inside a container disagree about the
/// repository's absolute path and agree about everything inside it. Every hook
/// payload carries `cwd` for the same reason; MCP has no such field, and has
/// something better — the client *declares* its roots and answers `roots/list`.
/// Measured against Claude Code 2.1.231, which advertises
/// `roots: { listChanged: true }`. Decision 24.
#[derive(Debug, Default)]
pub struct Session {
    /// Where the client says the repository is, once it has said.
    seen_as: Option<camino::Utf8PathBuf>,
}

impl Session {
    /// A session that has not asked yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Where the client says the repository is.
    #[must_use]
    pub fn seen_as(&self) -> Option<&Utf8Path> {
        self.seen_as.as_deref()
    }
}

/// The path a `file://` URI names.
///
/// Roots arrive as URIs, and a path is what everything downstream takes.
/// Percent escapes are decoded because a repository under a directory with a
/// space in it is an ordinary thing and an undecoded `%20` would name nothing.
#[must_use]
fn path_of(uri: &str) -> Option<camino::Utf8PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    let mut decoded = String::with_capacity(encoded.len());
    let mut bytes = encoded.bytes();

    while let Some(byte) = bytes.next() {
        if byte != b'%' {
            decoded.push(byte as char);
            continue;
        }
        let hex: String = bytes.by_ref().take(2).map(|b| b as char).collect();
        if let Ok(value) = u8::from_str_radix(&hex, 16) {
            decoded.push(value as char);
        } else {
            // A stray `%` is not an escape. Keeping it is closer to what the
            // client meant than dropping the rest of the path.
            decoded.push('%');
            decoded.push_str(&hex);
        }
    }

    (!decoded.is_empty()).then(|| camino::Utf8PathBuf::from(decoded))
}

/// Serves one client, reading requests until its input ends.
///
/// Returns when stdin closes, which is how a stdio server is stopped: the
/// client kills the pipe when the session ends.
///
/// # Errors
/// Only a write that failed, which means the client is gone.
pub fn serve(
    input: &mut dyn std::io::BufRead,
    output: &mut dyn std::io::Write,
    working_directory: &Utf8Path,
) -> std::io::Result<()> {
    let mut session = Session::new();
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        if line.trim().is_empty() {
            continue;
        }

        // Zero, one or two lines out. A notification takes no reply and may
        // still make this server ask something of its own, which is why this
        // is a list rather than an `Option`.
        for written in exchange(&mut session, &line, working_directory) {
            writeln!(output, "{written}")?;
            output.flush()?;
        }
    }
}

/// One message in, everything it produces out.
///
/// Separate from [`handle`] because a server that only ever answers cannot ask
/// the client where the repository is — and that question is the whole of
/// decision 24 on this surface.
#[must_use]
pub fn exchange(session: &mut Session, message: &str, working_directory: &Utf8Path) -> Vec<String> {
    // Our own answer coming back. Absorbed before anything else looks at it:
    // it carries an id and no method, which every other path would read as a
    // malformed request.
    if took_roots(session, message) {
        return Vec::new();
    }

    let mut written = Vec::new();
    if let Some(reply) = handle_in(session, message, working_directory) {
        written.push(reply);
    }

    // Asked once the client is ready, and again whenever it says its roots
    // moved. `listChanged` is the protocol's own way of keeping this current,
    // which is why nothing here caches a config the same way.
    if asks_for_roots(message) {
        written.push(rendered(&json!({
            "jsonrpc": "2.0",
            "id": ROOTS_REQUEST_ID,
            "method": "roots/list",
        })));
    }

    written
}

/// Whether this message means the client is ready to be asked.
fn asks_for_roots(message: &str) -> bool {
    let Ok(parsed) = serde_json::from_str::<Value>(message) else {
        return false;
    };

    matches!(
        parsed.get("method").and_then(Value::as_str),
        Some("notifications/initialized" | "notifications/roots/list_changed")
    )
}

/// Takes in the client's answer, and says whether this message was it.
///
/// A client that answers with **no** roots is still answering, and what was
/// known stops being known: keeping a stale root would translate against a
/// topology the client has left, which is worse than having none.
fn took_roots(session: &mut Session, message: &str) -> bool {
    let Some(listed) = ours(message) else {
        return false;
    };

    // The first root. A client may declare several — a multi-root workspace —
    // and only one of them can be the repository this server was started in.
    // Choosing the first is what the protocol's ordering offers; a wrong one
    // fails the ancestor test in `repo_relative` and refuses, rather than
    // translating into the wrong project.
    session.seen_as = listed
        .iter()
        .filter_map(|root| root.get("uri")?.as_str())
        .find_map(path_of);
    true
}

/// The roots in this message, when it is the answer to the one question this
/// server asks.
fn ours(message: &str) -> Option<Vec<Value>> {
    let parsed: Value = serde_json::from_str(message).ok()?;
    if parsed.get("id")?.as_i64()? != ROOTS_REQUEST_ID {
        return None;
    }

    parsed.get("result")?.get("roots")?.as_array().cloned()
}

/// Answers one message, or nothing when it was a notification.
///
/// A notification carries no `id` and takes no reply; answering one is a
/// protocol violation that some clients treat as fatal.
#[must_use]
pub fn handle(message: &str, working_directory: &Utf8Path) -> Option<String> {
    handle_in(&Session::new(), message, working_directory)
}

/// The same, against a session that may know where the client stands.
#[must_use]
fn handle_in(session: &Session, message: &str, working_directory: &Utf8Path) -> Option<String> {
    let parsed: Value = match serde_json::from_str(message) {
        Ok(parsed) => parsed,
        // No id to answer with, so the error goes out against a null one,
        // which is what JSON-RPC says to do when the request could not be read.
        Err(error) => {
            return Some(rendered(&failure(
                &Value::Null,
                PARSE_ERROR,
                &format!("the message is not JSON: {error}"),
            )));
        }
    };

    let id = parsed.get("id").cloned();
    let method = parsed.get("method").and_then(Value::as_str).unwrap_or("");

    // No id: a notification. `notifications/initialized` is the one every
    // client sends, and the correct answer to all of them is silence.
    let id = id?;

    let reply = match method {
        "initialize" => success(&id, &initialize()),
        "tools/list" => success(&id, &json!({ "tools": tools() })),
        "tools/call" => call(session, &id, parsed.get("params"), working_directory),
        "ping" => success(&id, &json!({})),
        "" => failure(&id, INVALID_REQUEST, "the message names no method"),
        other => failure(
            &id,
            METHOD_NOT_FOUND,
            &format!("`{other}` is not a method this server has"),
        ),
    };

    Some(rendered(&reply))
}

fn rendered(reply: &Value) -> String {
    // A reply that will not serialise is a bug rather than an input, and
    // answering nothing would hang the client. The fallback is a valid message
    // saying so.
    serde_json::to_string(reply).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"the reply could not be serialised"}}"#
            .to_owned()
    })
}

fn initialize() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        // Tools only. This server reads a repository and answers; it has no
        // resources to list and no prompts to offer, and claiming either would
        // have clients call methods that answer nothing.
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "archwarden",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

/// The tools, which are the operations and nothing else.
///
/// Issue #65 draws the line: *anything that is not already an operation is out
/// of scope. If MCP needs a capability `archwarden-api` does not have, that is
/// a signal about the boundary, not a reason to add it here.*
fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "check_write",
            "description":
                "Ask whether writing this content at this path would satisfy the \
                 repository's architecture rules. Answers before the write, so nothing \
                 is created. This is the same judgement the pre-write hook makes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file the write would land at. It need not exist yet.",
                    },
                    "content": {
                        "type": "string",
                        "description": "The text the write would leave in it.",
                    },
                },
                "required": ["path", "content"],
            },
        }),
        json!({
            "name": "describe",
            "description":
                "List every rule that applies to a path, with what it requires and why \
                 it exists. The path need not exist yet.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The file or directory to ask about." },
                },
                "required": ["path"],
            },
        }),
        json!({
            "name": "config_options",
            "description":
                "What an arch.config.json can carry: the config's own keys, and the ten \
                 values a rule's `type` can take, each with its required fields, what they \
                 mean, their defaults, and a rule to paste. Ask this before writing or \
                 changing a rule rather than guessing at the shape.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description":
                            "One key or rule kind, such as `call-obligation` or \
                             `governance`. Omit for the list of everything.",
                    },
                },
                "required": [],
            },
        }),
        json!({
            "name": "scaffold",
            "description":
                "The smallest shape that would satisfy the rules at a path: required \
                 exports, sibling files that must exist, and import constraints. \
                 Structural requirements only, never a working file body.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The file or directory to shape." },
                },
                "required": ["path"],
            },
        }),
    ]
}

/// Runs one tool call.
///
/// Every arm loads the configuration first and separately, which is the
/// re-reading rule: one call's answer must not depend on what a previous call
/// happened to find on disk.
fn call(
    session: &Session,
    id: &Value,
    params: Option<&Value>,
    working_directory: &Utf8Path,
) -> Value {
    let Some(name) = params.and_then(|p| p.get("name")).and_then(Value::as_str) else {
        return failure(id, INVALID_REQUEST, "the call names no tool");
    };
    let arguments = params.and_then(|p| p.get("arguments"));

    // Answered before a configuration is loaded, and that is deliberate: the
    // moment this is asked is *before* there is one, or while the one there is
    // is the thing being changed. Every other tool here needs a repository;
    // this one is about the ones you could write. Issue #97.
    if name == "config_options" {
        return configurable(id, arguments);
    }

    let Some(path) = arguments
        .and_then(|a| a.get("path"))
        .and_then(Value::as_str)
    else {
        return tool_error(id, &format!("`{name}` needs a `path`"));
    };

    // Read here, on every call, and never cached. See the module docs.
    let prepared = match archwarden_api::prepare(
        Location {
            config: None,
            root: None,
        },
        working_directory,
    ) {
        Ok(prepared) => prepared,
        Err(error) => return tool_error(id, &error.unreadable()),
    };

    // Where the client says the repository is, when it has said. A client on
    // the host and this server inside a container name one file two ways, and
    // until 0.19 the second answered "outside the repository" about it.
    // Decision 24.
    let repo_relative = match describe::repo_relative(
        &prepared.merged.root,
        working_directory,
        session.seen_as(),
        path,
    ) {
        Ok(path) => path,
        Err(reason) => return tool_error(id, &reason),
    };

    let answer = match name {
        "describe" => serde_json::to_value(describe::envelope(
            &repo_relative,
            &describe::describe(&prepared.compiled, &repo_relative),
        )),
        "scaffold" => serde_json::to_value(scaffold::envelope(
            &repo_relative,
            &scaffold::scaffold(&prepared.compiled, &repo_relative),
        )),
        "check_write" => {
            let Some(content) = arguments
                .and_then(|a| a.get("content"))
                .and_then(Value::as_str)
            else {
                return tool_error(id, "`check_write` needs a `content`");
            };
            Ok(judged(&archwarden_api::single::check(
                &prepared.merged.root,
                &prepared.compiled,
                &repo_relative,
                Some(content),
            )))
        }
        other => {
            return failure(
                id,
                METHOD_NOT_FOUND,
                &format!("`{other}` is not a tool this server has"),
            );
        }
    };

    match answer {
        Ok(value) => success(id, &text_content(&value.to_string(), false)),
        Err(error) => tool_error(id, &format!("the answer could not be serialised: {error}")),
    }
}

/// What an `arch.config.json` can carry.
///
/// The surface where the report's workaround does not exist: an MCP client has
/// no `node_modules` to read `schema/v0.json` out of, so before this there was
/// no way to learn a rule's shape at all.
fn configurable(id: &Value, arguments: Option<&Value>) -> Value {
    let options = archwarden_api::options::options();

    let Some(name) = arguments
        .and_then(|a| a.get("name"))
        .and_then(Value::as_str)
    else {
        return match serde_json::to_value(&options) {
            Ok(value) => success(id, &text_content(&value.to_string(), false)),
            Err(error) => tool_error(id, &format!("the answer could not be serialised: {error}")),
        };
    };

    let answer = match options.find(name) {
        Some(archwarden_api::options::Found::Key(field)) => serde_json::to_value(field),
        Some(archwarden_api::options::Found::Kind(entry)) => serde_json::to_value(entry),
        // Named, and the names it could have been. A model handed "unknown"
        // retries the same word; handed the list, it picks the right one.
        None => {
            return tool_error(
                id,
                &format!(
                    "nothing configurable is called `{name}`; there is {}",
                    archwarden_api::describe::join_or(&options.names(), "nothing")
                ),
            );
        }
    };

    match answer {
        Ok(value) => success(id, &text_content(&value.to_string(), false)),
        Err(error) => tool_error(id, &format!("the answer could not be serialised: {error}")),
    }
}

/// What `check_write` answers, as a value.
///
/// `refused` is the whole point and comes first: an agent asking *would this
/// pass?* wants a yes or a no, and the findings are why. Progress is reported
/// separately and never refuses, exactly as the hook reports it — a write
/// supplying one of a directory's required files is fixing it, not breaking it.
fn judged(checked: &single::Checked) -> Value {
    json!({
        "refused": checked.refuses(),
        "path": checked.single.path,
        "findings": checked
            .single
            .findings
            .iter()
            .map(finding)
            .collect::<Vec<_>>(),
        "fixing": checked.fixing.iter().map(finding).collect::<Vec<_>>(),
        // Present even when empty, for the reason `check --file` gives: a
        // caller has to see the list is empty rather than infer it from
        // absence, and a rule that could not run is not a rule that passed.
        "skipped": checked
            .single
            .skipped
            .iter()
            .map(|skipped| json!({
                "rule_id": skipped.rule_id,
                "reason": skipped.reason.as_str(),
            }))
            .collect::<Vec<_>>(),
        "unresolved_imports": checked.single.unresolved_imports,
    })
}

fn finding(finding: &archwarden_core::finding::Finding) -> Value {
    json!({
        "rule_id": finding.rule_id.as_str(),
        "level": finding.level.as_str(),
        "path": finding.path,
        "said": describe::describe_observed(&finding.observed),
    })
}

fn success(id: &Value, result: &Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn failure(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// A tool that could not answer.
///
/// An `isError` result rather than a JSON-RPC error, which is the protocol's
/// own distinction: the call was well-formed and the *tool* has something to
/// say. A client shows this to the model, which is what has to know that
/// nothing was checked — a JSON-RPC error is a transport fault the model
/// never sees.
fn tool_error(id: &Value, message: &str) -> Value {
    success(
        id,
        &text_content(&format!("archwarden could not answer: {message}."), true),
    )
}

fn text_content(text: &str, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    /// A repository with one rule, which is enough for every tool to have
    /// something to say.
    fn repository() -> (tempfile::TempDir, Utf8PathBuf) {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("utf-8");
        std::fs::write(
            root.join("arch.config.json"),
            r#"{"version":0,"rules":[
                {"type":"presence","id":"tem-os-tres","level":"error",
                 "roots":["projetos/*"],
                 "require":["projeto.md","exercicios.md","diagram.json"]}]}"#,
        )
        .expect("write the config");
        (guard, root)
    }

    fn answer(message: &str, root: &Utf8Path) -> Value {
        serde_json::from_str(&handle(message, root).expect("a request is answered"))
            .expect("the reply is JSON")
    }

    /// The text a tool call carries, parsed back. Every tool answers with one
    /// text block holding JSON, because that is what an MCP client hands a
    /// model.
    fn tool_text(reply: &Value) -> &str {
        reply["result"]["content"][0]["text"]
            .as_str()
            .expect("one text block")
    }

    /// A line that is not JSON gets a JSON-RPC parse error against a null id,
    /// which is what the protocol says to do when the request cannot be read
    /// well enough to know what to answer.
    #[test]
    fn a_line_that_is_not_json_is_a_parse_error_and_not_a_crash() {
        let (_guard, root) = repository();

        let reply = answer("{not json at all", &root);

        assert_eq!(reply["error"]["code"], PARSE_ERROR);
        assert!(reply["id"].is_null());
    }

    /// A well-formed message that names no method is a different fault from an
    /// unknown one, and says so.
    #[test]
    fn a_message_with_no_method_is_an_invalid_request() {
        let (_guard, root) = repository();

        let reply = answer(r#"{"jsonrpc":"2.0","id":1}"#, &root);

        assert_eq!(reply["error"]["code"], INVALID_REQUEST);
    }

    /// `ping` is the protocol's own liveness check and has to answer.
    #[test]
    fn ping_is_answered() {
        let (_guard, root) = repository();

        let reply = answer(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#, &root);

        assert_eq!(reply["id"], 7);
        assert!(reply["result"].is_object());
    }

    /// A notification takes no reply at all. Answering one is a protocol
    /// violation some clients treat as fatal.
    #[test]
    fn a_notification_is_answered_with_nothing() {
        let (_guard, root) = repository();

        assert!(
            handle(
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                &root
            )
            .is_none()
        );
    }

    /// A tool this build does not have is a method-not-found, not a silent
    /// empty answer: an agent handed nothing would conclude the question has
    /// no answer rather than that it asked the wrong one.
    #[test]
    fn an_unknown_tool_is_refused_by_name() {
        let (_guard, root) = repository();

        let reply = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"invent","arguments":{"path":"a"}}}"#,
            &root,
        );

        assert_eq!(reply["error"]["code"], METHOD_NOT_FOUND);
        assert!(
            reply["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("invent")),
            "{reply}"
        );
    }

    /// A call naming no tool at all.
    #[test]
    fn a_call_with_no_tool_named_is_an_invalid_request() {
        let (_guard, root) = repository();

        let reply = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"arguments":{}}}"#,
            &root,
        );

        assert_eq!(reply["error"]["code"], INVALID_REQUEST);
    }

    /// A tool called without the argument it needs is an `isError` result
    /// rather than a JSON-RPC error: the call was well-formed and the tool has
    /// something to say, which the model needs to see. A transport error it
    /// never sees.
    #[test]
    fn a_tool_called_without_its_arguments_says_so_where_the_model_can_see_it() {
        let (_guard, root) = repository();

        let missing_path = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"describe","arguments":{}}}"#,
            &root,
        );
        assert_eq!(missing_path["result"]["isError"], true);
        assert!(tool_text(&missing_path).contains("needs a `path`"));

        let missing_content = answer(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"check_write","arguments":{"path":"a.ts"}}}"#,
            &root,
        );
        assert_eq!(missing_content["result"]["isError"], true);
        assert!(tool_text(&missing_content).contains("needs a `content`"));
    }

    /// `scaffold` answers through the same envelope the command prints.
    #[test]
    fn scaffold_answers_the_shared_envelope() {
        let (_guard, root) = repository();

        let reply = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"scaffold","arguments":{"path":"projetos/01-blink"}}}"#,
            &root,
        );
        let shape: Value = serde_json::from_str(tool_text(&reply)).expect("carrying JSON");

        assert_eq!(shape["version"], 0);
        assert_eq!(shape["path"], "projetos/01-blink");
        assert!(
            shape["required_files"]["names"]
                .as_array()
                .is_some_and(|names| names.len() == 3),
            "{shape}"
        );
    }

    /// A path outside the repository is refused where the model can see it,
    /// rather than answered about some other file.
    #[test]
    fn a_path_outside_the_repository_is_refused() {
        let (_guard, root) = repository();

        let reply = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"describe","arguments":{"path":"/elsewhere/a.ts"}}}"#,
            &root,
        );

        assert_eq!(reply["result"]["isError"], true);
        assert!(tool_text(&reply).contains("outside the repository"));
    }

    /// No config at all is its own sentence, because it sends the reader to
    /// `archwarden init` rather than to a file they would not find.
    #[test]
    fn a_repository_with_no_config_says_that_and_not_something_else() {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("utf-8");

        let reply = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"describe","arguments":{"path":"a.ts"}}}"#,
            &root,
        );

        assert_eq!(reply["result"]["isError"], true);
        assert!(
            tool_text(&reply).contains("no archwarden config was found"),
            "{}",
            tool_text(&reply)
        );
    }

    /// `check_write` reports what it could not evaluate, always — a caller has
    /// to see the list is empty rather than infer it from absence, and a rule
    /// that could not run is not a rule that passed.
    #[test]
    fn check_write_always_carries_its_skipped_and_unresolved_lists() {
        let (_guard, root) = repository();

        let reply = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"check_write","arguments":{"path":"projetos/01-blink/projeto.md","content":"blink"}}}"#,
            &root,
        );
        let judged: Value = serde_json::from_str(tool_text(&reply)).expect("carrying JSON");

        assert!(judged["skipped"].is_array(), "{judged}");
        assert!(judged["unresolved_imports"].is_array(), "{judged}");
        assert_eq!(
            judged["refused"], false,
            "a write supplying a required file is progress: {judged}"
        );
        assert!(
            judged["fixing"].as_array().is_some_and(|f| !f.is_empty()),
            "and what is still missing is reported: {judged}"
        );
    }

    /// The whole loop, over a pipe: two requests in, two replies out, and it
    /// returns when the input ends rather than waiting for a client that has
    /// gone.
    #[test]
    fn the_loop_answers_until_its_input_ends() {
        let (_guard, root) = repository();
        let requests = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            "\n",
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
        );
        let mut input = std::io::BufReader::new(requests.as_bytes());
        let mut written = Vec::new();

        serve(&mut input, &mut written, &root).expect("the loop runs to the end of its input");

        let replies: Vec<Value> = String::from_utf8(written)
            .expect("utf-8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("one message per line"))
            .collect();

        // Three: the two requests, and the one question this server asks of
        // its own once the client says it is ready. The blank line is silent,
        // and so is the notification itself — what follows it is not a reply
        // to it.
        assert_eq!(replies.len(), 3, "{replies:?}");
        assert_eq!(replies[0]["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(
            replies[1]["method"], "roots/list",
            "it asks where the client thinks the repository is"
        );
        assert!(replies[2]["result"]["tools"].is_array());
    }

    /// The id this server asks with is negative, and that is the property
    /// rather than the number.
    ///
    /// Ids are a shared namespace: the client numbers its requests from zero
    /// upward, and a server asking with `1` would eventually be handed the
    /// client's own reply to its own request `1` and take it for an answer.
    /// A collision there is silent, which is why this is asserted against the
    /// literal rather than against the constant beside it.
    #[test]
    fn the_id_this_server_asks_with_cannot_collide_with_the_clients() {
        // Negative, and asserted as the literal: a client numbers its own
        // requests upward from zero, so nothing it sends can wear this.
        assert_eq!(ROOTS_REQUEST_ID, -1);
    }

    /// The codes are JSON-RPC's own, and a client branches on the number.
    ///
    /// Asserted against the literals rather than against the constants beside
    /// them: comparing a constant to itself proves the comparison, not the
    /// value, and a sign dropped from one of these turns "method not found"
    /// into a code no client has ever heard of.
    #[test]
    fn the_error_codes_are_the_ones_json_rpc_defines() {
        assert_eq!(METHOD_NOT_FOUND, -32601);
        assert_eq!(INVALID_REQUEST, -32600);
        assert_eq!(PARSE_ERROR, -32700);
    }

    /// The tools are the operations, and the list is the contract a client
    /// reads once at startup. A tool missing from it is a tool that does not
    /// exist as far as any agent is concerned.
    #[test]
    fn every_operation_is_offered_with_a_schema_for_its_arguments() {
        let offered = tools();

        let names: Vec<&str> = offered
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(
            names,
            ["check_write", "describe", "config_options", "scaffold"]
        );

        for tool in &offered {
            assert_eq!(
                tool["inputSchema"]["type"], "object",
                "a tool with no argument schema is one a client cannot call: {tool}"
            );
            assert!(
                tool["description"]
                    .as_str()
                    .is_some_and(|said| said.len() > 40),
                "the description is what decides whether a model reaches for it: {tool}"
            );
            // `config_options` is the one tool that takes nothing, because it
            // is about the configurations you could write rather than about a
            // repository. Everything else needs to be told what to look at.
            let takes_nothing = tool["name"] == "config_options";
            assert_eq!(
                tool["inputSchema"]["required"]
                    .as_array()
                    .is_some_and(Vec::is_empty),
                takes_nothing,
                "{tool}"
            );
        }
    }

    /// A finding says which rule, how seriously, where, and what was found —
    /// and the prose is the same sentence every other surface says, so a
    /// blocked write and a failing `check` cannot describe one problem
    /// differently.
    #[test]
    fn a_finding_carries_the_rule_the_level_the_path_and_the_sentence() {
        let (_guard, root) = repository();

        let reply = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"check_write","arguments":{"path":"projetos/01-blink/qualquer.md","content":"nada"}}}"#,
            &root,
        );
        let judged: Value = serde_json::from_str(tool_text(&reply)).expect("carrying JSON");

        let finding = &judged["findings"][0];
        assert_eq!(finding["rule_id"], "tem-os-tres");
        assert_eq!(finding["level"], "error");
        assert_eq!(finding["path"], "projetos/01-blink");
        // The same sentence `check` prints and the pre-write hook says when it
        // denies, generated from the same `Observed` value the JSON carries —
        // so the two can never describe one finding differently.
        assert_eq!(finding["said"], "`projeto.md` is not here");
    }

    // --- one repository, two roots --------------------------------------

    /// A `file://` URI is what a root arrives as, and a path is what
    /// everything downstream takes.
    #[test]
    fn a_root_uri_becomes_the_path_it_names() {
        assert_eq!(
            path_of("file:///home/dev/projeto").as_deref(),
            Some(camino::Utf8Path::new("/home/dev/projeto"))
        );
        // A repository under a directory with a space in it is an ordinary
        // thing, and an undecoded `%20` names nothing.
        assert_eq!(
            path_of("file:///home/dev/meus%20projetos/app").as_deref(),
            Some(camino::Utf8Path::new("/home/dev/meus projetos/app"))
        );
        // A stray `%` is not an escape. Keeping it beats dropping the rest.
        assert_eq!(
            path_of("file:///home/50%25/app").as_deref(),
            Some(camino::Utf8Path::new("/home/50%/app"))
        );
        assert_eq!(path_of("https://example.com/x"), None);
        assert_eq!(path_of("file://"), None);
    }

    /// The client is asked once it says it is ready, and again whenever it
    /// says its roots moved — which is what it advertises `listChanged` for.
    #[test]
    fn the_client_is_asked_where_the_repository_is() {
        let (_guard, root) = repository();
        let mut session = Session::new();

        for notification in [
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/roots/list_changed"}"#,
        ] {
            let written = exchange(&mut session, notification, &root);

            assert_eq!(written.len(), 1, "a notification takes no reply of its own");
            let asked: Value = serde_json::from_str(&written[0]).expect("JSON");
            assert_eq!(asked["method"], "roots/list");
            assert_eq!(asked["id"], ROOTS_REQUEST_ID);
        }
    }

    /// And the answer is absorbed rather than answered. It carries an id and
    /// no method, which every other path would read as a malformed request.
    #[test]
    fn the_answer_is_taken_in_and_not_replied_to() {
        let (_guard, root) = repository();
        let mut session = Session::new();

        let written = exchange(
            &mut session,
            &format!(
                r#"{{"jsonrpc":"2.0","id":{ROOTS_REQUEST_ID},"result":{{"roots":[{{"uri":"file:///home/dev/projeto"}}]}}}}"#
            ),
            &root,
        );

        assert!(written.is_empty(), "{written:?}");
        assert_eq!(
            session.seen_as(),
            Some(camino::Utf8Path::new("/home/dev/projeto"))
        );
    }

    /// A client that answers with no roots is answering, and what was known
    /// stops being known. Keeping a stale root would be worse than having
    /// none: it would translate against a topology the client has left.
    #[test]
    fn an_answer_with_no_roots_clears_what_was_known() {
        let (_guard, root) = repository();
        let mut session = Session::new();

        let _ = exchange(
            &mut session,
            &format!(
                r#"{{"jsonrpc":"2.0","id":{ROOTS_REQUEST_ID},"result":{{"roots":[{{"uri":"file:///home/dev/projeto"}}]}}}}"#
            ),
            &root,
        );
        assert!(session.seen_as().is_some());

        let _ = exchange(
            &mut session,
            &format!(r#"{{"jsonrpc":"2.0","id":{ROOTS_REQUEST_ID},"result":{{"roots":[]}}}}"#),
            &root,
        );

        assert_eq!(session.seen_as(), None);
    }

    /// A reply of the client's own, carrying an id that is not ours, is not
    /// mistaken for the answer to our question.
    #[test]
    fn somebody_elses_reply_is_not_our_answer() {
        let (_guard, root) = repository();
        let mut session = Session::new();

        let _ = exchange(
            &mut session,
            r#"{"jsonrpc":"2.0","id":7,"result":{"roots":[{"uri":"file:///nao/e/nosso"}]}}"#,
            &root,
        );

        assert_eq!(session.seen_as(), None);
    }

    /// The whole point, end to end: the client's path, our root, one file, and
    /// a verdict instead of a shrug. This is issue #93 through MCP.
    #[test]
    fn a_tool_called_with_the_clients_path_is_answered_about_our_file() {
        let (_guard, root) = repository();
        std::fs::create_dir_all(root.join("projetos/01-blink")).expect("create");
        let mut session = Session::new();

        let _ = exchange(
            &mut session,
            &format!(
                r#"{{"jsonrpc":"2.0","id":{ROOTS_REQUEST_ID},"result":{{"roots":[{{"uri":"file:///home/dev/projeto"}}]}}}}"#
            ),
            &root,
        );

        let written = exchange(
            &mut session,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"check_write","arguments":{"path":"/home/dev/projeto/projetos/01-blink/qualquer.md","content":"nada"}}}"#,
            &root,
        );

        let reply: Value = serde_json::from_str(&written[0]).expect("JSON");
        assert_ne!(
            reply["result"]["isError"], true,
            "it answered instead of shrugging: {reply}"
        );
        let judged: Value = serde_json::from_str(tool_text(&reply)).expect("carrying JSON");
        assert_eq!(judged["path"], "projetos/01-blink/qualquer.md");
        assert_eq!(judged["refused"], true);
    }

    /// And a client whose root is somewhere else entirely is refused, naming
    /// both roots. The guard is decision 24's, and it is what keeps a
    /// translation from being a guess.
    #[test]
    fn a_path_from_another_project_is_refused_and_names_both_roots() {
        let (_guard, root) = repository();
        let mut session = Session::new();

        let _ = exchange(
            &mut session,
            &format!(
                r#"{{"jsonrpc":"2.0","id":{ROOTS_REQUEST_ID},"result":{{"roots":[{{"uri":"file:///home/dev/outro"}}]}}}}"#
            ),
            &root,
        );

        let written = exchange(
            &mut session,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"describe","arguments":{"path":"/home/dev/outro/servico/interno/y.ts"}}}"#,
            &root,
        );

        let reply: Value = serde_json::from_str(&written[0]).expect("JSON");
        assert_eq!(reply["result"]["isError"], true);
        let said = tool_text(&reply);
        assert!(said.contains("/home/dev/outro"), "{said}");
        assert!(said.contains("where the caller says"), "{said}");
    }

    /// The surface where the reported workaround does not exist: an MCP client
    /// has no `node_modules` to read `schema/v0.json` out of, so before 0.20
    /// there was no way to learn a rule's shape at all. Issue #97.
    #[test]
    fn the_configurable_surface_is_answered_over_mcp() {
        let (_guard, root) = repository();

        let reply = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"config_options","arguments":{"name":"call-obligation"}}}"#,
            &root,
        );
        let entry: Value = serde_json::from_str(tool_text(&reply)).expect("carrying JSON");

        assert_eq!(entry["name"], "call-obligation");
        let required: Vec<&str> = entry["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .filter(|field| field["required"] == true)
            .filter_map(|field| field["name"].as_str())
            .collect();
        assert!(required.contains(&"must_call"), "{entry}");
        assert!(entry["example"].is_string(), "a rule to paste: {entry}");
    }

    /// And with no name, the whole surface — the config's own keys as well as
    /// the rule kinds, because `governance` and `extends` send somebody into
    /// `node_modules` exactly as a rule kind does.
    #[test]
    fn with_no_name_it_answers_the_whole_surface() {
        let (_guard, root) = repository();

        let reply = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"config_options","arguments":{}}}"#,
            &root,
        );
        let all: Value = serde_json::from_str(tool_text(&reply)).expect("carrying JSON");

        assert!(
            all["top_level"]
                .as_array()
                .is_some_and(|keys| keys.len() > 8)
        );
        assert_eq!(all["kinds"].as_array().map(Vec::len), Some(10));
    }

    /// It answers with no configuration at all, which is the moment it is
    /// asked: before there is one, or while the one there is is being changed.
    #[test]
    fn it_answers_a_repository_with_no_configuration() {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(guard.path().to_path_buf()).expect("utf-8");

        let reply = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"config_options","arguments":{"name":"presence"}}}"#,
            &root,
        );

        assert_ne!(
            reply["result"]["isError"], true,
            "no config is not a reason to withhold this: {reply}"
        );
    }

    /// A name nothing has is refused with the names it could have been. A
    /// model handed "unknown" retries the same word.
    #[test]
    fn a_name_nothing_has_is_answered_with_the_names_there_are() {
        let (_guard, root) = repository();

        let reply = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"config_options","arguments":{"name":"call-obligations"}}}"#,
            &root,
        );

        assert_eq!(reply["result"]["isError"], true);
        let said = tool_text(&reply);
        assert!(said.contains("call-obligation"), "{said}");
        assert!(said.contains("governance"), "{said}");
    }
}
