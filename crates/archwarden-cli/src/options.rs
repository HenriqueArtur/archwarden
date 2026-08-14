//! `archwarden config options` — writing out what can be configured.
//!
//! The operation is [`archwarden_api::options`]; this is the surface's half.
//! Same split as [`crate::describe`]: the shape a program consumes is built
//! once in the crate both surfaces depend on, and the prose for a terminal is
//! this one's own.
//!
//! # It answers with no configuration
//!
//! Every other `config` subcommand reads an `arch.config.json` first. This one
//! must not: the moment somebody needs it is *before* there is one, or while
//! the one they have is the thing being changed. Refusing to say what a
//! `presence` rule takes because the file they are fixing does not parse would
//! be the tool withholding the answer to the question at exactly the moment it
//! was asked.

use archwarden_api::options::{Entry, Field, Found};

use crate::Output;
use crate::exit::Exit;
use crate::report::Format;

/// Answers about one name, or lists them all.
pub fn run(name: Option<&str>, format: Format, output: &mut Output<'_>) -> Exit {
    let options = archwarden_api::options::options();

    let Some(name) = name else {
        match format {
            Format::Text => list(&options, output),
            Format::Json => write_json(&options, output),
        }
        return Exit::Clean;
    };

    match options.find(name) {
        Some(Found::Key(field)) => match format {
            Format::Text => key(field, output),
            Format::Json => write_json(field, output),
        },
        Some(Found::Kind(entry)) => match format {
            Format::Text => kind(entry, output),
            Format::Json => write_json(entry, output),
        },
        // Named, and the names it could have been. A typo answered with
        // "unknown" is one somebody retypes; answered with the list, it is one
        // they fix. `agent-guide` already does this for kinds.
        None => {
            let _ = writeln!(
                output.err,
                "nothing configurable is called `{name}`; there is {}",
                archwarden_api::describe::join_or(&options.names(), "nothing")
            );
            return Exit::ConfigProblem;
        }
    }

    Exit::Clean
}

fn write_json(value: &impl serde::Serialize, output: &mut Output<'_>) {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            let _ = writeln!(output.out, "{json}");
        }
        Err(error) => {
            let _ = writeln!(output.out, r#"{{"error":"cannot serialise: {error}"}}"#);
        }
    }
}

/// The two lists, and how to ask about one of them.
fn list(options: &archwarden_api::options::Options, output: &mut Output<'_>) {
    let _ = writeln!(output.out, "What an `arch.config.json` can carry.\n");

    let _ = writeln!(output.out, "Keys of the config itself:\n");
    let width = options
        .top_level
        .iter()
        .map(|field| field.name.len())
        .max()
        .unwrap_or(0);
    for field in &options.top_level {
        let required = if field.required { "  (required)" } else { "" };
        let _ = writeln!(
            output.out,
            "  {:<width$}  {}{required}",
            field.name, field.description
        );
    }

    let _ = writeln!(output.out, "\nValues a rule's `type` can take:\n");
    let width = options
        .kinds
        .iter()
        .map(|entry| entry.name.len())
        .max()
        .unwrap_or(0);
    for entry in &options.kinds {
        let _ = writeln!(output.out, "  {:<width$}  {}", entry.name, entry.summary);
    }

    let _ = writeln!(
        output.out,
        "\nAsk about one: `archwarden config options <name>`"
    );
}

/// One key of the config object.
fn key(field: &Field, output: &mut Output<'_>) {
    let _ = writeln!(output.out, "{} — {}\n", field.name, field.description);

    if field.required {
        let _ = writeln!(output.out, "  required.");
    }
    if let Some(default) = &field.default {
        let _ = writeln!(output.out, "  defaults to {default}");
    }
    if !field.inner.is_empty() {
        let _ = writeln!(output.out, "\n  It carries:");
        write_fields(&field.inner, "    ", output);
    }
}

/// One rule kind, which is what the report was asking about.
fn kind(entry: &Entry, output: &mut Output<'_>) {
    let _ = writeln!(output.out, "{} — {}\n", entry.name, entry.summary);

    // Required and optional named as two lists before the detail, because
    // "which of these must I write?" is the question asked first and the one
    // the report says it would have got wrong.
    let named = |wanted: bool| {
        entry
            .fields
            .iter()
            .filter(|field| field.required == wanted)
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let required = named(true);
    let optional = named(false);
    if !required.is_empty() {
        let _ = writeln!(output.out, "  required: {required}");
    }
    if !optional.is_empty() {
        let _ = writeln!(output.out, "  optional: {optional}");
    }

    let _ = writeln!(output.out);
    write_fields(&entry.fields, "  ", output);

    if let Some(example) = &entry.example {
        let _ = writeln!(output.out, "\nExample:\n");
        for line in example.lines() {
            let _ = writeln!(output.out, "  {line}");
        }
    }
}

/// One group of fields, with the descriptions in a column.
///
/// Aligned per group rather than across the whole answer: the fields inside
/// `must_call` are a list of their own, and padding them to the width of the
/// list above would push them off the edge for nothing.
///
/// A trailing `?` marks optional. A column that said "optional" in words would
/// be wider than most of the names it qualifies.
fn write_fields(fields: &[Field], indent: &str, output: &mut Output<'_>) {
    let width = fields
        .iter()
        .map(|field| field.name.len() + usize::from(!field.required))
        .max()
        .unwrap_or(0);

    for field in fields {
        let name = format!("{}{}", field.name, if field.required { "" } else { "?" });
        let _ = writeln!(output.out, "{indent}{name:<width$}  {}", field.description);
        if let Some(default) = &field.default {
            let _ = writeln!(output.out, "{indent}{:<width$}  defaults to {default}", "");
        }
        if !field.inner.is_empty() {
            write_fields(&field.inner, &format!("{indent}  "), output);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Captured {
        out: String,
        err: String,
        exit: Exit,
    }

    fn answered(name: Option<&str>, format: Format) -> Captured {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut input = "".as_bytes();
        let exit = run(
            name,
            format,
            &mut Output {
                out: &mut out,
                err: &mut err,
                input: &mut input,
            },
        );

        Captured {
            out: String::from_utf8(out).expect("utf-8"),
            err: String::from_utf8(err).expect("utf-8"),
            exit,
        }
    }

    /// The list names both halves of what a config can carry, and says how to
    /// ask about one. Reaching into `node_modules` for `schema/v0.json` is the
    /// behaviour this replaces, and a list that named only rule kinds would
    /// leave `governance` and `extends` sending somebody back there.
    #[test]
    fn the_list_carries_the_keys_and_the_kinds_and_says_how_to_ask() {
        let said = answered(None, Format::Text);

        assert_eq!(said.exit, Exit::Clean);
        for expected in ["governance", "modules", "call-obligation", "presence"] {
            assert!(
                said.out.contains(expected),
                "{expected} missing: {}",
                said.out
            );
        }
        assert!(said.out.contains("config options <name>"), "{}", said.out);
    }

    /// `version` is the one key a config cannot leave out, and the list says
    /// so where a reader is choosing what to write.
    #[test]
    fn the_list_marks_what_cannot_be_left_out() {
        let said = answered(None, Format::Text);

        let version = said
            .out
            .lines()
            .find(|line| line.trim_start().starts_with("version "))
            .expect("the key is listed");
        assert!(version.contains("(required)"), "{version}");
    }

    /// One kind, which is the question issue #97 was actually asking. Both
    /// lists come before the detail, because "which of these must I write?" is
    /// asked first and is the half the report says it would have got wrong.
    #[test]
    fn one_kind_names_its_required_fields_first() {
        let said = answered(Some("call-obligation"), Format::Text);

        assert_eq!(said.exit, Exit::Clean);
        assert!(said.out.contains("required: "), "{}", said.out);
        assert!(said.out.contains("optional: why"), "{}", said.out);

        let required = said.out.find("required:").expect("named");
        let detail = said.out.find("must_call").expect("described");
        assert!(required < detail, "the summary comes first: {}", said.out);
    }

    /// The field a reader would have got wrong, with both of its own fields
    /// and the sentence that says what one means.
    #[test]
    fn a_nested_field_carries_the_fields_inside_it() {
        let said = answered(Some("call-obligation"), Format::Text);

        assert!(said.out.contains("symbol"), "{}", said.out);
        assert!(said.out.contains("imported_from"), "{}", said.out);
        assert!(said.out.contains("call site"), "the meaning: {}", said.out);
    }

    /// Optional is marked on the field itself, not only in the summary above
    /// it — a reader scanning the detail should not have to scroll back.
    #[test]
    fn optional_fields_are_marked_where_they_are_described() {
        let said = answered(Some("call-obligation"), Format::Text);

        assert!(said.out.contains("why?"), "{}", said.out);
        assert!(
            !said.out.contains("roots?"),
            "and a required one is not marked: {}",
            said.out
        );
    }

    /// A default decides whether a field has to be written at all, so it is
    /// printed beside the field rather than left to the schema.
    #[test]
    fn a_default_is_printed_beside_the_field_it_belongs_to() {
        let said = answered(Some("no-passthrough"), Format::Text);

        assert!(said.out.contains("defaults to"), "{}", said.out);
        assert!(said.out.contains("reexport"), "{}", said.out);
    }

    /// A rule to paste, because the shape is easier to copy than to assemble.
    #[test]
    fn a_kind_carries_a_rule_to_paste() {
        let said = answered(Some("presence"), Format::Text);

        assert!(said.out.contains("Example:"), "{}", said.out);
        assert!(said.out.contains("\"type\": \"presence\""), "{}", said.out);
    }

    /// A key of the config object is a different shape from a rule kind, and
    /// says the things a key has: whether it is required, and what it carries.
    #[test]
    fn one_key_of_the_config_is_answered_as_a_key() {
        let said = answered(Some("governance"), Format::Text);

        assert_eq!(said.exit, Exit::Clean);
        assert!(said.out.starts_with("governance — "), "{}", said.out);
        assert!(
            !said.out.contains("Example:"),
            "a key's shape is its field list: {}",
            said.out
        );
    }

    /// A key that cannot be left out says so.
    #[test]
    fn a_required_key_says_it_is_required() {
        let said = answered(Some("version"), Format::Text);

        assert!(said.out.contains("required."), "{}", said.out);
    }

    /// JSON for a program, and it is the shared value rather than a second
    /// shape assembled here.
    #[test]
    fn the_json_is_the_shared_value() {
        let listed = answered(None, Format::Json);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).expect("valid JSON");
        assert!(parsed["top_level"].is_array());
        assert!(parsed["kinds"].is_array());

        let one = answered(Some("call-obligation"), Format::Json);
        let parsed: serde_json::Value = serde_json::from_str(&one.out).expect("valid JSON");
        assert_eq!(parsed["name"], "call-obligation");
        assert!(parsed["fields"].is_array());
    }

    /// A name nothing has is refused, and the names it could have been are
    /// listed. A typo answered with "unknown" is one somebody retypes;
    /// answered with the list, it is one they fix.
    #[test]
    fn a_name_nothing_has_is_refused_with_the_names_there_are() {
        let said = answered(Some("call-obligations"), Format::Text);

        assert_eq!(said.exit, Exit::ConfigProblem);
        assert!(said.err.contains("call-obligations"), "{}", said.err);
        assert!(said.err.contains("call-obligation"), "{}", said.err);
        assert!(said.err.contains("governance"), "keys too: {}", said.err);
        assert!(said.out.is_empty(), "nothing on stdout: {}", said.out);
    }

    /// A key that is an object of its own says what it carries. `skip_dirs` is
    /// three fields deep and a reader told only its name is back where they
    /// started.
    #[test]
    fn a_key_that_carries_fields_lists_them() {
        let said = answered(Some("skip_dirs"), Format::Text);

        assert!(said.out.contains("It carries:"), "{}", said.out);
        assert!(
            said.out
                .lines()
                .filter(|line| line.starts_with("    "))
                .count()
                >= 3,
            "{}",
            said.out
        );
    }

    /// The descriptions line up in a column. Not decoration: a list of ten
    /// fields whose text starts at ten different places is read one line at a
    /// time instead of scanned, which is what a reference is for.
    #[test]
    fn the_descriptions_line_up_in_a_column() {
        let said = answered(Some("call-obligation"), Format::Text);

        // The top-level fields of the kind: two spaces in, and not the nested
        // ones, which are a list of their own and aligned among themselves.
        let columns: Vec<usize> = said
            .out
            .lines()
            .filter(|line| line.starts_with("  ") && !line.starts_with("   "))
            .filter_map(|line| {
                let named = line.trim_start();
                // The two summary lines are `required: …` and `optional: …`,
                // which are prose rather than a name and a description.
                let (name, rest) = named.split_once("  ")?;
                // Where the description actually begins, past the padding.
                (!name.ends_with(':') && !rest.trim_start().is_empty())
                    .then(|| line.len() - rest.trim_start().len())
            })
            .collect();

        assert!(columns.len() >= 4, "{}", said.out);
        assert!(
            columns.iter().all(|column| *column == columns[0]),
            "the descriptions start at {columns:?}:\n{}",
            said.out
        );
    }
}
