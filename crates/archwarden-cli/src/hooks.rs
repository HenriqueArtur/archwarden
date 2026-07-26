//! `archwarden install-hooks` — wiring archwarden into a harness.
//!
//! Layer 4 of `AGENT-INTEGRATION.md` needs two halves: something that answers
//! "would this write be legal?", which is `check --file`, and something that
//! puts the question in the harness's path. This is the second half.
//!
//! # What the hook actually receives
//!
//! `AGENT-INTEGRATION.md:168` described the installed command as
//! `archwarden check --file $CLAUDE_FILE_PATH`. Claude Code does not pass the
//! path in the environment: a hook is handed the event as JSON on stdin, with
//! the write's target under `tool_input.file_path`. Correction C15.
//!
//! So the installed command is `archwarden hook claude-code`, which reads that
//! payload itself. One binary and no shell quoting, rather than a one-liner
//! that would need `jq` the user may not have.
//!
//! # Naming a command the harness can actually run
//!
//! That bare command was wrong for the way archwarden is installed. As a dev
//! dependency it lives in `node_modules/.bin`, which is on the PATH of a
//! `package.json` script and of nothing else — and a harness runs a hook as
//! its own process, not through npm. The hook would have failed with "command
//! not found" on every write.
//!
//! So a project with a `package.json` gets `npx archwarden hook claude-code`.
//! `npx` resolves `node_modules/.bin` itself, walks up for a hoisted install,
//! and works the same on Windows, where `.bin` holds `.cmd` shims a bare path
//! would miss. An absolute path to the binary would have been faster and
//! unshareable: `.claude/settings.json` is committed, and that path names this
//! machine and this platform's package.
//!
//! A project with no `package.json` did not install archwarden from npm, so
//! the binary is on the PATH already and the bare command is right. Prefixing
//! `npx` there would ask npm to fetch a package the user never installed.
//!
//! # Editing a file that is not ours
//!
//! `.claude/settings.json` belongs to the user. Everything here is
//! string-to-string so it can be tested without touching a disk, the edit is
//! keyed on the command so a second run updates rather than duplicates, and
//! `serde_json` carries the `preserve_order` feature so a round-trip does not
//! alphabetise keys the user wrote in an order they chose.

use serde_json::{Map, Value, json};

/// Where Claude Code keeps its project settings.
pub const CLAUDE_SETTINGS: &str = ".claude/settings.json";

/// The command, and the key archwarden recognises its own entry by.
///
/// Every form it installs ends in this, so matching on it as a substring
/// recognises the `npx` one, a path someone wrote by hand, and the bare
/// command alike. An entry it failed to recognise would be duplicated on the
/// next run, and every write checked twice.
pub const HOOK_COMMAND: &str = "archwarden hook claude-code";

/// How to invoke archwarden from a process the harness starts, rather than
/// from an npm script.
///
/// See the module docs for why a node project gets `npx` and nothing else
/// does. Used for the installed command and for the commands archwarden
/// suggests in its own messages: telling an agent to run something it cannot
/// run is the same defect, one layer further out.
#[must_use]
pub fn invocation(root: &camino::Utf8Path) -> String {
    if root.join("package.json").is_file() {
        return "npx archwarden".to_owned();
    }
    "archwarden".to_owned()
}

/// The command to install in `root`.
#[must_use]
pub fn hook_command(root: &camino::Utf8Path) -> String {
    format!("{} hook claude-code", invocation(root))
}

/// The tools whose writes are worth intercepting.
const MATCHER: &str = "Write|Edit|MultiEdit";

/// The event archwarden hooks into.
const EVENT: &str = "PreToolUse";

/// What an install or removal did, for a message the user can trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The hook was not there and now is.
    Installed,
    /// It was already there, unchanged.
    AlreadyInstalled,
    /// It was there and is gone.
    Removed,
    /// There was nothing to remove.
    NotInstalled,
}

/// Adds archwarden's pre-write hook to `settings`, or reports it was already
/// there.
///
/// `settings` is the file's current contents, or `None` when there is no file
/// yet. `command` is what to install, from [`hook_command`]. Returns the
/// contents to write, and what happened.
///
/// # Errors
/// A message naming the problem, when the file is not a JSON object.
pub fn install(settings: Option<&str>, command: &str) -> Result<(String, Outcome), String> {
    let mut root = parse(settings)?;

    let hooks = object_at(&mut root, "hooks")?;
    let entries = array_at(hooks, EVENT)?;

    if entries.iter().any(has_our_command) {
        return Ok((render(&root), Outcome::AlreadyInstalled));
    }

    entries.push(json!({
        "matcher": MATCHER,
        "hooks": [{ "type": "command", "command": command }],
    }));

    Ok((render(&root), Outcome::Installed))
}

/// Takes archwarden's hook back out, leaving everything else alone.
///
/// # Errors
/// A message naming the problem, when the file is not a JSON object.
pub fn remove(settings: Option<&str>) -> Result<(String, Outcome), String> {
    let mut root = parse(settings)?;

    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok((render(&root), Outcome::NotInstalled));
    };
    let Some(entries) = hooks.get_mut(EVENT).and_then(Value::as_array_mut) else {
        return Ok((render(&root), Outcome::NotInstalled));
    };

    let before = entries.len();
    entries.retain(|entry| !has_our_command(entry));
    if entries.len() == before {
        return Ok((render(&root), Outcome::NotInstalled));
    }

    // Leaving `"PreToolUse": []` behind would be litter in someone else's
    // file, and `"hooks": {}` after it more so.
    if entries.is_empty() {
        hooks.remove(EVENT);
    }
    if hooks.is_empty() {
        root.remove("hooks");
    }

    Ok((render(&root), Outcome::Removed))
}

/// Whether an entry is one archwarden installed.
///
/// Matched on the command rather than on the whole block, so a user who
/// changed the matcher or added a timeout keeps their edit through an
/// upgrade instead of getting a second copy.
fn has_our_command(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.contains(HOOK_COMMAND))
            })
        })
}

fn parse(settings: Option<&str>) -> Result<Map<String, Value>, String> {
    let Some(settings) = settings else {
        return Ok(Map::new());
    };
    // An empty file is a file someone created and left; treating it as `{}` is
    // kinder than refusing to install because of a stray `touch`.
    if settings.trim().is_empty() {
        return Ok(Map::new());
    }

    match serde_json::from_str::<Value>(settings) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(format!("`{CLAUDE_SETTINGS}` is not a JSON object")),
        Err(error) => Err(format!("`{CLAUDE_SETTINGS}` is not valid JSON: {error}")),
    }
}

fn object_at<'a>(
    root: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, String> {
    let slot = root.entry(key.to_owned()).or_insert_with(|| json!({}));
    slot.as_object_mut()
        .ok_or_else(|| format!("`{key}` in `{CLAUDE_SETTINGS}` is not an object"))
}

fn array_at<'a>(
    parent: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Vec<Value>, String> {
    let slot = parent.entry(key.to_owned()).or_insert_with(|| json!([]));
    slot.as_array_mut()
        .ok_or_else(|| format!("`hooks.{key}` in `{CLAUDE_SETTINGS}` is not an array"))
}

/// Two-space JSON with a trailing newline, which is what the file already
/// looks like and what an editor will not immediately reformat.
fn render(root: &Map<String, Value>) -> String {
    let mut rendered =
        serde_json::to_string_pretty(&Value::Object(root.clone())).unwrap_or_else(|_| "{}".into());
    rendered.push('\n');
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(settings: Option<&str>) -> String {
        install(settings, HOOK_COMMAND).expect("installs").0
    }

    /// A directory holding whatever entries the case needs.
    fn project(entries: &[&str]) -> (tempfile::TempDir, camino::Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf())
            .expect("temp path is UTF-8");
        for entry in entries {
            std::fs::write(root.join(entry), "{}").expect("write");
        }
        (dir, root)
    }

    /// The bug this replaces: as a dev dependency archwarden is in
    /// `node_modules/.bin`, which is on the PATH of a `package.json` script
    /// and of nothing else. A harness runs the hook as its own process, so a
    /// bare `archwarden` is a command that does not exist.
    #[test]
    fn a_node_project_gets_a_command_that_resolves() {
        let (_guard, root) = project(&["package.json"]);

        assert_eq!(hook_command(&root), "npx archwarden hook claude-code");
    }

    /// Without a `package.json` there is no `node_modules/.bin` to reach, so
    /// the binary came from a release archive and is on the PATH already.
    /// Prefixing `npx` there would ask npm to fetch a package this user never
    /// installed.
    #[test]
    fn a_project_with_no_package_json_calls_the_binary_directly() {
        let (_guard, root) = project(&[]);

        assert_eq!(hook_command(&root), HOOK_COMMAND);
    }

    /// Both forms have to be recognised as ours, or the next `install-hooks`
    /// adds a second entry and every write is checked twice.
    #[test]
    fn every_form_of_the_command_is_recognised_as_ours() {
        for command in [
            HOOK_COMMAND,
            "npx archwarden hook claude-code",
            "pnpm exec archwarden hook claude-code",
            "./node_modules/.bin/archwarden hook claude-code",
        ] {
            let settings = format!(
                r#"{{"hooks":{{"PreToolUse":[{{"matcher":"Write","hooks":[
                    {{"type":"command","command":"{command}"}}]}}]}}}}"#
            );

            let (_, outcome) = install(Some(&settings), HOOK_COMMAND).expect("installs");
            assert_eq!(outcome, Outcome::AlreadyInstalled, "{command}");

            let (_, outcome) = remove(Some(&settings)).expect("removes");
            assert_eq!(outcome, Outcome::Removed, "{command}");
        }
    }

    /// What `install` writes is what it was handed. The probe decides the
    /// form; this module does not second-guess it.
    #[test]
    fn the_command_handed_in_is_the_command_written() {
        let (written, _) = install(None, "npx archwarden hook claude-code").expect("installs");

        assert_eq!(
            entries(&written)[0]["hooks"][0]["command"],
            "npx archwarden hook claude-code"
        );
    }

    fn entries(settings: &str) -> Vec<Value> {
        let root: Value = serde_json::from_str(settings).expect("valid JSON");
        root["hooks"][EVENT].as_array().cloned().unwrap_or_default()
    }

    /// The first install, into a project that has no settings file at all.
    #[test]
    fn installing_into_nothing_creates_the_hook() {
        let (written, outcome) = install(None, HOOK_COMMAND).expect("installs");

        assert_eq!(outcome, Outcome::Installed);
        let hooks = entries(&written);
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0]["matcher"], MATCHER);
        assert_eq!(hooks[0]["hooks"][0]["type"], "command");
        assert_eq!(hooks[0]["hooks"][0]["command"], HOOK_COMMAND);
    }

    /// Idempotence, which the doc asks for by name: a second run must not
    /// leave two copies for the harness to run twice.
    #[test]
    fn installing_twice_changes_nothing() {
        let once = installed(None);
        let (twice, outcome) = install(Some(&once), HOOK_COMMAND).expect("installs");

        assert_eq!(outcome, Outcome::AlreadyInstalled);
        assert_eq!(twice, once, "byte-identical");
        assert_eq!(entries(&twice).len(), 1);
    }

    /// The file belongs to the user. Their other settings, their other hooks,
    /// and the order they wrote their keys in all survive.
    #[test]
    fn everything_the_user_wrote_survives() {
        let theirs = r#"{
  "model": "opus",
  "hooks": {
    "SessionStart": [
      {
        "matcher": "",
        "hooks": [{ "type": "command", "command": "their-tool session-start" }]
      }
    ]
  },
  "env": { "ZZZ": "1", "AAA": "2" }
}"#;

        let written = installed(Some(theirs));
        let root: Value = serde_json::from_str(&written).expect("valid JSON");

        assert_eq!(root["model"], "opus");
        assert_eq!(
            root["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "their-tool session-start"
        );
        assert_eq!(entries(&written).len(), 1, "ours was added");

        // `preserve_order` is why this holds. Without it a round-trip
        // alphabetises every object, which is a large diff in a file the user
        // did not ask us to reformat.
        let keys: Vec<_> = root.as_object().expect("object").keys().collect();
        assert_eq!(keys, ["model", "hooks", "env"]);
        let env: Vec<_> = root["env"].as_object().expect("object").keys().collect();
        assert_eq!(env, ["ZZZ", "AAA"]);
    }

    /// A `PreToolUse` hook the user already had is left alone, and ours joins
    /// it rather than replacing it.
    #[test]
    fn an_existing_pre_tool_use_hook_is_kept() {
        let theirs = r#"{"hooks":{"PreToolUse":[
            {"matcher":"Bash","hooks":[{"type":"command","command":"their-guard"}]}]}}"#;

        let written = installed(Some(theirs));
        let hooks = entries(&written);

        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks[0]["hooks"][0]["command"], "their-guard");
        assert_eq!(hooks[1]["hooks"][0]["command"], HOOK_COMMAND);
    }

    /// A user who narrowed the matcher or added a timeout keeps their edit
    /// through a re-run. Recognising our entry by the command rather than by
    /// the whole block is what makes that work.
    #[test]
    fn a_users_edit_to_our_entry_is_not_undone() {
        let edited = r#"{"hooks":{"PreToolUse":[
            {"matcher":"Write","hooks":[
                {"type":"command","command":"archwarden hook claude-code","timeout":5}]}]}}"#;

        let (written, outcome) = install(Some(edited), HOOK_COMMAND).expect("installs");

        assert_eq!(outcome, Outcome::AlreadyInstalled);
        let hooks = entries(&written);
        assert_eq!(hooks.len(), 1, "not duplicated");
        assert_eq!(
            hooks[0]["matcher"], "Write",
            "their narrower matcher stands"
        );
        assert_eq!(hooks[0]["hooks"][0]["timeout"], 5, "their timeout stands");
    }

    /// Uninstall takes ours out and leaves theirs.
    #[test]
    fn removing_takes_only_our_entry() {
        let theirs = r#"{"hooks":{"PreToolUse":[
            {"matcher":"Bash","hooks":[{"type":"command","command":"their-guard"}]}]}}"#;
        let both = installed(Some(theirs));

        let (written, outcome) = remove(Some(&both)).expect("removes");

        assert_eq!(outcome, Outcome::Removed);
        let hooks = entries(&written);
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0]["hooks"][0]["command"], "their-guard");
    }

    /// Removing the last one takes the empty scaffolding with it. Leaving
    /// `"PreToolUse": []` behind is litter in someone else's file.
    #[test]
    fn removing_the_last_hook_leaves_no_empty_husk() {
        let only_ours = installed(None);
        let (written, outcome) = remove(Some(&only_ours)).expect("removes");

        assert_eq!(outcome, Outcome::Removed);
        assert_eq!(written.trim(), "{}", "{written}");
    }

    /// Removing what is not there is not an error, so `--remove` is safe to
    /// run twice and safe to put in a teardown script.
    #[test]
    fn removing_what_is_not_there_is_not_a_failure() {
        for settings in [
            None,
            Some("{}"),
            Some(r#"{"model":"opus"}"#),
            Some(r#"{"hooks":{}}"#),
            Some(r#"{"hooks":{"PreToolUse":[]}}"#),
            Some(r#"{"hooks":{"SessionStart":[{"hooks":[{"command":"x"}]}]}}"#),
        ] {
            let (_, outcome) = remove(settings).expect("does not fail");
            assert_eq!(outcome, Outcome::NotInstalled, "{settings:?}");
        }
    }

    /// A settings file that is not a JSON object is refused rather than
    /// overwritten. Replacing a file we cannot read would destroy work.
    #[test]
    fn a_settings_file_we_cannot_read_is_refused() {
        assert_eq!(
            install(Some("[1, 2]"), HOOK_COMMAND).expect_err("not an object"),
            "`.claude/settings.json` is not a JSON object"
        );
        assert!(
            install(Some("{ oops"), HOOK_COMMAND)
                .expect_err("not JSON")
                .contains("is not valid JSON"),
        );
    }

    /// A `hooks` key of the wrong shape is named rather than replaced, for the
    /// same reason.
    #[test]
    fn a_hooks_key_of_the_wrong_shape_is_named() {
        assert_eq!(
            install(Some(r#"{"hooks":"none"}"#), HOOK_COMMAND).expect_err("not an object"),
            "`hooks` in `.claude/settings.json` is not an object"
        );
        assert_eq!(
            install(Some(r#"{"hooks":{"PreToolUse":{}}}"#), HOOK_COMMAND)
                .expect_err("not an array"),
            "`hooks.PreToolUse` in `.claude/settings.json` is not an array"
        );
    }

    /// An empty file is one someone created and left. Refusing to install
    /// because of a stray `touch` would be unkind.
    #[test]
    fn an_empty_file_is_treated_as_empty_settings() {
        let (written, outcome) = install(Some("   \n"), HOOK_COMMAND).expect("installs");

        assert_eq!(outcome, Outcome::Installed);
        assert_eq!(entries(&written).len(), 1);
    }

    /// The file ends with a newline, like every other file in a repository.
    #[test]
    fn the_written_file_ends_with_a_newline() {
        assert!(installed(None).ends_with("}\n"));
    }

    /// The matcher covers the tools that write files, and only those. A hook
    /// on every tool would run archwarden on `Bash` and `Read` for nothing.
    #[test]
    fn the_matcher_covers_the_writing_tools() {
        for tool in ["Write", "Edit", "MultiEdit"] {
            assert!(MATCHER.contains(tool), "{tool} is a writing tool");
        }
        assert!(
            !MATCHER.contains("Bash"),
            "a hook on Bash would run for nothing"
        );
    }
}
