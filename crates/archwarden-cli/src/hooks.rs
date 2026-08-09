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
//! Three answers, in this order, and none of them needs configuring:
//!
//! 1. **`./node_modules/.bin/archwarden`, when it is there.** The relative path
//!    survives being committed, where an absolute one names this machine and
//!    this platform's package. It starts a process rather than a package
//!    manager, and — the reason to prefer it rather than merely allow it — it
//!    cannot reach the registry. `npx archwarden` with nothing installed
//!    locally *fetches* archwarden, so a project that dropped the dependency
//!    keeps a hook that works, quietly, at a version nobody chose.
//! 2. **`npx archwarden`, for a `package.json` with nothing installed yet.**
//!    The dependency may arrive later and `npx` finds it then. It also walks up
//!    for a hoisted install.
//! 3. **`archwarden`, otherwise.** No `package.json` means the binary came from
//!    a release archive and is on the PATH already; prefixing `npx` there would
//!    ask npm to fetch a package the user never installed.
//!
//! Detected rather than chosen, because a flag is a thing to get wrong and the
//! filesystem already knows the answer.
//!
//! # Editing a file that is not ours
//!
//! `.claude/settings.json` belongs to the user, and "belongs to" is meant
//! strictly: the `hooks` key is replaced inside their own bytes and every other
//! byte is left alone.
//!
//! It used to round-trip the document through `serde_json`, which produced
//! valid JSON and *a different file*. One repository groups about 180
//! permission entries into sections with blank lines; a serialiser cannot know
//! that and re-flowed the lot, so adding one hook arrived as a diff nobody
//! could review. `preserve_order` had already been reached for to stop keys
//! being alphabetised — the same problem, one size smaller, solved one case at
//! a time until the general answer was to stop rewriting the file.
//!
//! Everything here is still string-to-string so it can be tested without
//! touching a disk, and the edit is keyed on the command so a second run
//! updates rather than duplicates.

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
    // Installed: call it where it is. Relative, so the settings file stays
    // shareable; a process rather than a package manager; and it cannot reach
    // the registry, which is the part that matters — `npx archwarden` with
    // nothing installed locally *fetches* archwarden, so a project that
    // dropped the dependency keeps a working hook at a version nobody chose.
    //
    // The leading `./` is not decoration. Without it a shell with `.` off the
    // PATH — which is every shell — still resolves this, but the form with it
    // is unambiguous about naming a file rather than a command, and that is
    // what it is.
    if root.join(LOCAL_BINARY).is_file() {
        return format!("./{LOCAL_BINARY}");
    }
    // A `package.json` with nothing installed yet: the dependency may arrive
    // later and `npx` will find it then.
    if root.join("package.json").is_file() {
        return "npx archwarden".to_owned();
    }
    "archwarden".to_owned()
}

/// Where npm puts an installed archwarden, relative to the project root.
///
/// Forward slashes on every platform: this string goes into a shell command,
/// and Windows accepts them there. The extensionless name is the right one to
/// write on Windows too — `.bin` holds `archwarden.cmd` and `PATHEXT` is what
/// finds it.
const LOCAL_BINARY: &str = "node_modules/.bin/archwarden";

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
        // Unchanged means unchanged: hand back the bytes that came in rather
        // than a re-rendering of them.
        return Ok((
            settings.map_or_else(|| render(&root), str::to_owned),
            Outcome::AlreadyInstalled,
        ));
    }

    entries.push(json!({
        "matcher": MATCHER,
        "hooks": [{ "type": "command", "command": command }],
    }));

    Ok((written(settings, &root), Outcome::Installed))
}

/// Takes archwarden's hook back out, leaving everything else alone.
///
/// # Errors
/// A message naming the problem, when the file is not a JSON object.
pub fn remove(settings: Option<&str>) -> Result<(String, Outcome), String> {
    let mut root = parse(settings)?;

    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok((
            settings.map_or_else(|| render(&root), str::to_owned),
            Outcome::NotInstalled,
        ));
    };
    let Some(entries) = hooks.get_mut(EVENT).and_then(Value::as_array_mut) else {
        return Ok((
            settings.map_or_else(|| render(&root), str::to_owned),
            Outcome::NotInstalled,
        ));
    };

    let before = entries.len();
    entries.retain(|entry| !has_our_command(entry));
    if entries.len() == before {
        return Ok((
            settings.map_or_else(|| render(&root), str::to_owned),
            Outcome::NotInstalled,
        ));
    }

    // Leaving `"PreToolUse": []` behind would be litter in someone else's
    // file, and `"hooks": {}` after it more so.
    if entries.is_empty() {
        hooks.remove(EVENT);
    }
    if hooks.is_empty() {
        root.remove("hooks");
    }

    Ok((written(settings, &root), Outcome::Removed))
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

/// The contents to write: the user's file with one key changed, when that is
/// possible, and a fresh rendering when it is not.
///
/// A file that did not exist has no formatting to keep, so it is rendered. A
/// file that does gets [`weave`], and falls back to rendering only if the text
/// turns out to be a shape that cannot be edited in place — which loses the
/// user's layout and is still better than refusing to install.
fn written(settings: Option<&str>, root: &Map<String, Value>) -> String {
    let Some(source) = settings.filter(|text| !text.trim().is_empty()) else {
        return render(root);
    };

    weave(source, root.get("hooks")).unwrap_or_else(|| render(root))
}

/// Puts `hooks` back into the user's own text, touching nothing else.
///
/// `render` produces valid JSON and *a different file*: a serialiser has no
/// idea which blank lines were load-bearing. One repository groups about 180
/// permission entries into sections that way, and adding a hook re-flowed the
/// lot — a diff nobody can review, in a file a team maintains by hand.
///
/// So the whole document is left as bytes and only the `hooks` value is
/// replaced. `None` removes the key.
///
/// Returns `None` when the text is not a shape this can edit safely, and the
/// caller falls back to rendering the whole document — which is worse and
/// still correct.
fn weave(source: &str, hooks: Option<&Value>) -> Option<String> {
    let parsed = jsonc_parser::parse_to_ast(
        source,
        &jsonc_parser::CollectOptions::default(),
        &jsonc_parser::ParseOptions::default(),
    )
    .ok()?;

    let jsonc_parser::ast::Value::Object(root) = parsed.value.as_ref()? else {
        return None;
    };
    let existing = root
        .properties
        .iter()
        .find(|property| property.name.as_str() == "hooks");

    match (existing, hooks) {
        (Some(property), Some(value)) => {
            let range = value_range(&property.value);
            let indent = indent_of(source, property.range.start);
            let mut edited = String::with_capacity(source.len());
            edited.push_str(source.get(..range.0)?);
            edited.push_str(&reindented(value, &indent));
            edited.push_str(source.get(range.1..)?);
            Some(edited)
        }

        // The key goes away, and so does the comma that joined it to its
        // neighbour — otherwise the file is left with `,,` or a trailing one.
        (Some(property), None) => {
            let (start, end) = swallow_comma(source, property.range.start, property.range.end)?;
            let mut edited = String::with_capacity(source.len());
            edited.push_str(source.get(..start)?);
            edited.push_str(source.get(end..)?);
            Some(edited)
        }

        (None, Some(value)) => {
            // Before the closing brace, so the keys the user put first stay
            // first. A new key at the end is the smallest possible diff.
            let close = source.get(..root.range.end)?.rfind('}')?;
            let body = source.get(..close)?;
            let indent = root.properties.first().map_or_else(
                || "  ".to_owned(),
                |first| indent_of(source, first.range.start),
            );

            let trimmed = body.trim_end();
            let separator = if root.properties.is_empty() { "" } else { "," };
            let mut edited = String::with_capacity(source.len() + 64);
            edited.push_str(trimmed);
            edited.push_str(separator);
            edited.push('\n');
            edited.push_str(&indent);
            edited.push_str("\"hooks\": ");
            edited.push_str(&reindented(value, &indent));
            edited.push('\n');
            edited.push_str(source.get(close..)?);
            Some(edited)
        }

        (None, None) => Some(source.to_owned()),
    }
}

/// The byte range of a value node.
fn value_range(node: &jsonc_parser::ast::Value<'_>) -> (usize, usize) {
    use jsonc_parser::ast::Value as Node;
    let range = match node {
        Node::StringLit(literal) => literal.range,
        Node::NumberLit(literal) => literal.range,
        Node::BooleanLit(literal) => literal.range,
        Node::Object(object) => object.range,
        Node::Array(array) => array.range,
        Node::NullKeyword(keyword) => keyword.range,
    };
    (range.start, range.end)
}

/// The whitespace at the start of the line `offset` falls on.
///
/// The file's own indentation, whatever it is: a serialiser's two spaces would
/// be one more thing changed without being asked.
fn indent_of(source: &str, offset: usize) -> String {
    let line_start = source
        .get(..offset)
        .and_then(|before| before.rfind('\n').map(|index| index + 1))
        .unwrap_or(0);

    source
        .get(line_start..offset)
        .unwrap_or_default()
        .chars()
        .take_while(|character| character.is_whitespace())
        .collect()
}

/// The value, pretty-printed and shifted to sit at `indent`.
///
/// `to_string_pretty` writes as if the value were the whole document, so every
/// line after the first needs the surrounding depth added back.
fn reindented(value: &Value, indent: &str) -> String {
    let rendered = serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_owned());

    rendered
        .lines()
        .enumerate()
        .map(|(n, line)| {
            if n == 0 || line.is_empty() {
                line.to_owned()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Widens a property's range to take the comma that attached it, and the blank
/// line space it sat on.
///
/// The comma after it when there is one, the comma before it otherwise — a
/// last property has its comma on the left.
fn swallow_comma(source: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let after = source.get(end..)?;
    let trailing = after.len() - after.trim_start().len();
    if after.trim_start().starts_with(',') {
        let mut cut = end + trailing + 1;
        // And the newline it ended, so no blank line is left behind.
        if source.get(cut..)?.starts_with('\n') {
            cut += 1;
        }
        return Some((line_start_of(source, start), cut));
    }

    let before = source.get(..start)?;
    let comma = before.rfind(',')?;
    // Only if nothing but whitespace stands between: a comma further back
    // belongs to a different property.
    if !before.get(comma + 1..)?.trim().is_empty() {
        return None;
    }
    Some((comma, end))
}

/// Where the line containing `offset` begins.
///
/// Zero when there is no newline before it, not `offset`: a property on the
/// first line of a single-line file starts its line at the beginning of the
/// file. The wrong fallback was here until mutation testing pointed out that
/// nothing said what this should return.
fn line_start_of(source: &str, offset: usize) -> usize {
    source
        .get(..offset)
        .and_then(|before| before.rfind('\n').map(|index| index + 1))
        .unwrap_or(0)
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
            let path = root.join(entry);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create");
            }
            std::fs::write(path, "{}").expect("write");
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

    /// An installed archwarden is called where it is, not through `npx`.
    ///
    /// `npx` was chosen when the only alternatives were a bare command that
    /// does not resolve and an absolute path that names one machine. The
    /// installed binary is a third answer and a better one wherever it exists:
    /// the path is relative, so it survives being committed; it starts a
    /// process instead of a package manager; and it cannot reach the registry.
    ///
    /// That last one is the reason to prefer it rather than merely allow it.
    /// `npx archwarden` with nothing installed locally *fetches* archwarden —
    /// so a hook whose dev dependency was dropped keeps working, quietly, at a
    /// version nobody chose.
    #[test]
    fn an_installed_binary_is_called_where_it_lives() {
        let (_guard, root) = project(&["package.json", "node_modules/.bin/archwarden"]);

        assert_eq!(
            hook_command(&root),
            "./node_modules/.bin/archwarden hook claude-code"
        );
    }

    /// A `package.json` with nothing installed yet still gets `npx`: the
    /// dependency may arrive later, and `npx` finds it then. This is the case
    /// the previous rule was written for and it keeps its answer.
    #[test]
    fn a_node_project_with_nothing_installed_still_gets_npx() {
        let (_guard, root) = project(&["package.json"]);

        assert_eq!(hook_command(&root), "npx archwarden hook claude-code");
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

    /// The file's own indentation, not a serialiser's.
    ///
    /// Every one of these offsets edits a file somebody maintains by hand, and
    /// an off-by-one in them writes a `settings.json` that is subtly not what
    /// they had. Mutation testing asked for these by name: seventeen arithmetic
    /// and boundary mutants lived through the end-to-end tests, which assert
    /// what *survives* the edit and never look at the shape of what is added.
    #[test]
    fn the_indent_is_the_one_on_that_line() {
        assert_eq!(indent_of("{\n    \"a\": 1\n}", 6), "    ");
        assert_eq!(indent_of("{\n\t\"a\": 1\n}", 3), "\t");
        assert_eq!(indent_of("{\"a\": 1}", 1), "", "no line to indent from");
        assert_eq!(
            indent_of("{\n  \"a\": 1\n}", 4),
            "  ",
            "the newline itself is not indentation"
        );
    }

    /// A value is printed as if it were the whole document, so every line after
    /// the first needs the surrounding depth added back — and only those.
    #[test]
    fn a_value_is_shifted_to_the_depth_it_sits_at() {
        let value = json!({ "a": [1] });

        assert_eq!(
            reindented(&value, "  "),
            "{\n    \"a\": [\n      1\n    ]\n  }",
            "the first line is already in place and the rest are not"
        );
        assert_eq!(
            reindented(&json!({}), "    "),
            "{}",
            "a single line needs nothing"
        );
    }

    /// Where the line containing an offset begins.
    #[test]
    fn a_line_starts_after_the_newline_before_it() {
        assert_eq!(line_start_of("ab\ncd", 4), 3);
        assert_eq!(line_start_of("ab\ncd", 3), 3, "at the start of the line");
        assert_eq!(line_start_of("abcd", 2), 0, "the first line starts at zero");
    }

    /// The comma that joined the key to its neighbour goes with it.
    ///
    /// Leaving it produces `,,` or a trailing comma — either of which turns the
    /// user's settings file into something that will not parse, from a command
    /// whose whole promise is to leave it alone.
    #[test]
    fn removing_a_key_takes_the_comma_that_attached_it() {
        // `"a": 1, "b": 2` — taking `"a"` takes the comma after it.
        let source = "{\n  \"a\": 1,\n  \"b\": 2\n}";
        let (start, end) = swallow_comma(source, 4, 10).expect("range");
        assert_eq!(&source[start..end], "  \"a\": 1,\n");

        // A last property has its comma on the left.
        let (start, end) = swallow_comma(source, 14, 20).expect("range");
        assert_eq!(&source[start..end], ",\n  \"b\": 2");
    }

    /// A lone property has no comma either side, and nothing to swallow.
    #[test]
    fn removing_the_only_key_swallows_no_comma() {
        let source = "{\n  \"a\": 1\n}";
        assert_eq!(swallow_comma(source, 4, 10), None);
    }

    /// JSON allows whitespace before a comma, and somebody's file has it.
    ///
    /// The distance from the end of the value to the comma is counted, and
    /// every test above had that distance be zero — so the counting was
    /// exercised and never checked. Mutation testing is what noticed: two
    /// arithmetic mutants lived because `0` is `0` however you compute it.
    #[test]
    fn a_comma_set_apart_from_its_value_is_still_its_comma() {
        let source = "{\n  \"a\": 1  ,\n  \"b\": 2\n}";
        let (start, end) = swallow_comma(source, 4, 10).expect("range");
        assert_eq!(&source[start..end], "  \"a\": 1  ,\n");

        // And across a line break, which is a formatting nobody writes by hand
        // and a serialiser somewhere certainly does.
        let source = "{\n  \"a\": 1\n  ,\n  \"b\": 2\n}";
        let (start, end) = swallow_comma(source, 4, 10).expect("range");
        assert_eq!(&source[start..end], "  \"a\": 1\n  ,\n");
    }

    /// The added block sits at the depth the file's other keys sit at.
    ///
    /// Its *internal* nesting stays two-space, which is what a JSON serialiser
    /// and Claude Code both write. Re-flowing the new block into a file's own
    /// indent unit would be nicer and is not what this promises: the promise is
    /// that nothing already in the file moves.
    #[test]
    fn the_added_block_sits_at_the_files_own_depth() {
        let theirs = "{\n    \"model\": \"opus\"\n}";

        let written = installed(Some(theirs));

        assert!(
            written.contains("\n    \"hooks\": {"),
            "the key was not written at the depth its neighbours sit at:\n{written}"
        );
        assert!(
            written.contains("\n      \"PreToolUse\""),
            "the block's contents are not inside it:\n{written}"
        );
        assert!(
            written.contains("\n    \"model\": \"opus\","),
            "the key that was already there moved:\n{written}"
        );
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

    /// Every byte outside the `hooks` key is the byte the user wrote.
    ///
    /// Round-tripping the file through a serialiser produced valid JSON that
    /// was not their file: it dropped the blank lines grouping ~180 permission
    /// entries into readable sections, and re-indented whatever they had. A
    /// tool that reformats a hand-maintained file to add one entry is a tool
    /// whose diff nobody can review.
    ///
    /// Asserted on the text rather than on the parsed value, because a parsed
    /// value is exactly what cannot see the difference.
    #[test]
    fn nothing_outside_the_hooks_key_is_rewritten() {
        let theirs = "{\n  \"permissions\": {\n    \"allow\": [\n      \
             \"Bash(ls:*)\",\n\n      \"Bash(cat:*)\",\n\n\n      \"Read(//tmp/**)\"\n    ],\n\n\
             \n    \"deny\": [\"Bash(npx:*)\"]\n  },\n\n  \"model\":\"opus\"\n}";

        let written = installed(Some(theirs));

        let (before, _) = theirs.split_once("\n\n  \"model\"").expect("split");
        assert!(
            written.starts_with(before),
            "the text before `hooks` changed:\n{written}"
        );
        assert!(
            written.contains("\"Bash(ls:*)\",\n\n      \"Bash(cat:*)\",\n\n\n      \"Read"),
            "the blank lines grouping the entries were dropped:\n{written}"
        );
        assert!(
            written.contains("\"model\":\"opus\""),
            "a key the user wrote unspaced was reformatted:\n{written}"
        );
        assert_eq!(entries(&written).len(), 1, "and ours was added");
    }

    /// Taking it back out leaves the file as it was found, too.
    #[test]
    fn removing_leaves_the_rest_of_the_file_alone() {
        let theirs = "{\n  \"permissions\": {\n    \"allow\": [\n      \"Bash(ls:*)\",\n\n      \
             \"Bash(cat:*)\"\n    ]\n  },\n  \"model\":\"opus\"\n}";

        let installed_text = installed(Some(theirs));
        let (removed, outcome) = remove(Some(&installed_text)).expect("removes");

        assert_eq!(outcome, Outcome::Removed);
        assert!(
            removed.contains("\"Bash(ls:*)\",\n\n      \"Bash(cat:*)\""),
            "the blank line was dropped on the way out:\n{removed}"
        );
        assert!(
            removed.contains("\"model\":\"opus\""),
            "a key the user wrote unspaced was reformatted:\n{removed}"
        );
        assert!(
            !removed.contains("archwarden"),
            "our entry is still there:\n{removed}"
        );
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
