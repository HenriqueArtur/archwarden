//! What you can put in an `arch.config.json`, answered before you write it.
//!
//! Issue #97. Every other command answers about **paths** or about **rules that
//! are already declared**. Nothing answered the question you have while writing
//! a rule: *what does this take?*
//!
//! The reported workaround is the point:
//!
//! ```text
//! node -e "const s=require('/app/node_modules/archwarden/schema/v0.json'); …"
//! ```
//!
//! Chasing `Rule.oneOf` for the variant whose `type.const` matches, then
//! following `$defs` to learn that `must_call` needs two fields and not one.
//! That is guess-and-check against an internal file, which is the loop
//! `AGENTS.md` tells agents not to run for code — *"never guess a convention;
//! two of these commands cost milliseconds and answer exactly"* — offered for
//! code and not for the config. **Over MCP it does not work at all**: a client
//! has no `node_modules` to read.
//!
//! # Generated, never written twice
//!
//! Everything here comes from `schemars` over the config types, in this
//! process. Not from `schema/v0.json`: a binary from a release archive has no
//! such file beside it, and a copy of the field list would be a second thing to
//! keep in step with the first — which is the failure this crate exists to
//! prevent one layer up.
//!
//! So the descriptions are the doc comments on the config types, and they are
//! right by construction. The one thing that is not generated is an example per
//! rule kind, and `every_example_parses` is what keeps those honest: an example
//! that stops being valid fails the build rather than misleading a reader.

use serde::Serialize;

/// One thing you can configure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// What to write: a top-level key, or the value of a rule's `type`.
    pub name: String,
    /// The first line of what it is for.
    pub summary: String,
    /// Its fields, required first.
    pub fields: Vec<Field>,
    /// A rule of this kind, ready to paste. `None` for a top-level key, whose
    /// shape is the field list itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
}

/// One field of one thing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Field {
    /// The key to write.
    pub name: String,
    /// What it means, from the type's own doc comment.
    pub description: String,
    /// Whether leaving it out is an error.
    pub required: bool,
    /// What it is when left out, when that is a value rather than "absent".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    /// The fields inside it, for a field that is an object of its own.
    ///
    /// One level, not a tree. `must_call` needing `symbol` **and**
    /// `imported_from` is the thing the report says it would have got wrong,
    /// and it is one `$ref` down; a reader who needs the level below that is
    /// past what a summary can carry.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inner: Vec<Field>,
}

/// Everything configurable, in the order it is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Options {
    /// The keys of the config object itself.
    ///
    /// Fields rather than entries, because that is what they are: a key of one
    /// object, with a description and a default. A rule kind is the other
    /// thing — a shape you write *inside* `rules`.
    pub top_level: Vec<Field>,
    /// The values a rule's `type` can take.
    pub kinds: Vec<Entry>,
}

/// What a name turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found<'a> {
    /// A key of the config object.
    Key(&'a Field),
    /// A value of a rule's `type`.
    Kind(&'a Entry),
}

impl Options {
    /// The one thing called `name`, whichever of the two it is.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<Found<'_>> {
        if let Some(key) = self.top_level.iter().find(|field| field.name == name) {
            return Some(Found::Key(key));
        }
        self.kinds
            .iter()
            .find(|entry| entry.name == name)
            .map(Found::Kind)
    }

    /// Every name, for a caller that got one wrong.
    ///
    /// Naming them is what turns a typo into a correction rather than into
    /// "this does not exist", which is the reading the report got from
    /// `agent-guide` and called out.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.top_level
            .iter()
            .map(|field| field.name.as_str())
            .chain(self.kinds.iter().map(|entry| entry.name.as_str()))
            .collect()
    }
}

/// Reads the configurable surface out of the config types themselves.
#[must_use]
pub fn options() -> Options {
    let schema = schemars::schema_for!(archwarden_config::config::Config);
    let root = serde_json::to_value(&schema).unwrap_or_default();
    let defs = root.get("$defs").cloned().unwrap_or_default();

    Options {
        top_level: top_level(&root, &defs),
        kinds: kinds(&defs),
    }
}

/// The config object's own keys.
fn top_level(root: &serde_json::Value, defs: &serde_json::Value) -> Vec<Field> {
    let required = required_of(root);

    let mut fields: Vec<Field> = properties(root)
        .into_iter()
        .map(|(name, spec)| Field {
            required: required.contains(&name),
            description: first_line(&spec),
            default: spec.get("default").cloned(),
            inner: inner_fields(&spec, defs),
            name,
        })
        .collect();

    // `$schema` last. It is the one key archwarden reads nothing from -- it is
    // there so an editor offers completion -- and it sorts first by name, so
    // the answer opened on the least important thing in it. Kept rather than
    // hidden: it is in the schema, and somebody will find it and ask.
    fields.sort_by_key(|field| field.name.starts_with('$'));
    fields
}

/// The values a rule's `type` can take, one entry each.
fn kinds(defs: &serde_json::Value) -> Vec<Entry> {
    let Some(variants) = defs
        .get("Rule")
        .and_then(|rule| rule.get("oneOf"))
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };

    variants
        .iter()
        .filter_map(|variant| {
            let name = variant
                .get("properties")?
                .get("type")?
                .get("const")?
                .as_str()?
                .to_owned();
            let required = required_of(variant);

            let mut fields: Vec<Field> = properties(variant)
                .into_iter()
                // `type` is the name of the entry, not a field to describe.
                .filter(|(field, _)| field != "type")
                .map(|(field, spec)| Field {
                    required: required.contains(&field),
                    description: first_line(&spec),
                    default: spec.get("default").cloned(),
                    inner: inner_fields(&spec, defs),
                    name: field,
                })
                .collect();
            // Required first: it is the half a reader has to get right, and the
            // report names it as the thing that would have been got wrong.
            fields.sort_by_key(|field| !field.required);

            Some(Entry {
                summary: first_line(variant),
                example: example_for(&name).map(ToOwned::to_owned),
                name,
                fields,
            })
        })
        .collect()
}

/// The fields inside a field that is an object of its own.
fn inner_fields(spec: &serde_json::Value, defs: &serde_json::Value) -> Vec<Field> {
    let Some(target) = referenced(spec, defs) else {
        return Vec::new();
    };
    let required = required_of(&target);

    properties(&target)
        .into_iter()
        .map(|(name, inner)| Field {
            required: required.contains(&name),
            description: first_line(&inner),
            default: inner.get("default").cloned(),
            inner: Vec::new(),
            name,
        })
        .collect()
}

/// What a `$ref` points at, when this spec is one.
fn referenced(spec: &serde_json::Value, defs: &serde_json::Value) -> Option<serde_json::Value> {
    let reference = spec.get("$ref")?.as_str()?;
    let name = reference.strip_prefix("#/$defs/")?;
    defs.get(name).cloned()
}

fn properties(spec: &serde_json::Value) -> Vec<(String, serde_json::Value)> {
    spec.get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|object| {
            object
                .iter()
                .map(|(name, spec)| (name.clone(), spec.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn required_of(spec: &serde_json::Value) -> Vec<String> {
    spec.get("required")
        .and_then(serde_json::Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(|name| name.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The first sentence of a description, which is what a list has room for.
fn first_line(spec: &serde_json::Value) -> String {
    spec.get("description")
        .and_then(serde_json::Value::as_str)
        .and_then(|text| text.lines().next())
        .unwrap_or_default()
        .to_owned()
}

/// One rule of each kind, ready to paste.
///
/// The only thing here that is written rather than generated, and the only
/// thing that can drift. `every_example_parses` is what stops it: each of these
/// goes through the real config parser, so an example that stops being valid
/// fails the build instead of teaching somebody the wrong shape.
fn example_for(kind: &str) -> Option<&'static str> {
    EXAMPLES
        .iter()
        .find(|(name, _)| *name == kind)
        .map(|(_, example)| *example)
}

/// Keyed by the `type` value, and checked against the parser in this module's
/// tests. Written to be pasted and edited rather than to be minimal: a reader
/// who has to invent an `id` and a `roots` before the example runs has been
/// given a template, not an answer.
const EXAMPLES: &[(&str, &str)] = &[
    (
        "structure",
        r#"{
  "type": "structure",
  "id": "domain-modules-hold-only-types",
  "level": "error",
  "roots": ["packages/domain/src/*"],
  "allowed_subfolders": ["types"]
}"#,
    ),
    (
        "naming",
        r#"{
  "type": "naming",
  "id": "use-cases-export-their-pascal-name",
  "level": "error",
  "roots": ["packages/application/src/use-cases/*"],
  "file_pattern": "^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
  "must_export": { "name": "{{pascal(name)}}", "kind": "function" }
}"#,
    ),
    (
        "spec-pair",
        r#"{
  "type": "spec-pair",
  "id": "use-cases-need-a-spec",
  "level": "error",
  "roots": ["packages/application/src/use-cases/*"],
  "subfolders": ["."]
}"#,
    ),
    (
        "pair",
        r#"{
  "type": "pair",
  "id": "every-project-has-notes",
  "level": "error",
  "roots": ["projetos/*"],
  "file_pattern": "^projeto\\.md$",
  "must_exist": "notas.md"
}"#,
    ),
    (
        "presence",
        r#"{
  "type": "presence",
  "id": "a-project-is-three-files",
  "level": "error",
  "roots": ["projetos/*"],
  "require": ["projeto.md", "exercicios.md", "diagram.json"]
}"#,
    ),
    (
        "frontmatter",
        r#"{
  "type": "frontmatter",
  "id": "docs-declare-their-status",
  "level": "error",
  "roots": ["docs/*"],
  "file_pattern": "\\.md$",
  "require": ["title", "status"],
  "one_of": { "status": ["draft", "accepted", "superseded"] }
}"#,
    ),
    (
        "import-boundary",
        r#"{
  "type": "import-boundary",
  "id": "domain-does-not-reach-infrastructure",
  "level": "error",
  "why": "domain is published and infrastructure is not",
  "from": ["packages/domain/**"],
  "forbid_import_from": ["packages/infrastructure/**"]
}"#,
    ),
    (
        "import-cycle",
        r#"{
  "type": "import-cycle",
  "id": "no-loops-in-domain",
  "level": "error",
  "roots": ["packages/domain/**"]
}"#,
    ),
    (
        "call-obligation",
        r#"{
  "type": "call-obligation",
  "id": "writes-go-through-the-request-helper",
  "level": "error",
  "roots": ["services/sunne-api/Entities/*"],
  "file_pattern": "^[a-z0-9-]+\\.ts$",
  "must_call": {
    "symbol": "SunneApiHttpRequest",
    "imported_from": "../../Http/request"
  }
}"#,
    ),
    (
        "no-passthrough",
        r#"{
  "type": "no-passthrough",
  "id": "a-file-adds-something-of-its-own",
  "level": "warning",
  "roots": ["packages/application/src/**"]
}"#,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The three things the report says cost the most time, asked of the kind
    /// it was about. Issue #97.
    #[test]
    fn the_three_things_that_cost_the_report_its_afternoon() {
        let read = options();
        let Some(Found::Kind(entry)) = read.find("call-obligation") else {
            panic!("a kind archwarden has is a kind it answers about");
        };

        // 1. Which fields are required, and which are not. The report says it
        //    would have sent `must_call` with only `symbol`.
        let must_call = entry
            .fields
            .iter()
            .find(|field| field.name == "must_call")
            .expect("the field the rule is about");
        assert!(must_call.required);
        let inner: Vec<&str> = must_call
            .inner
            .iter()
            .filter(|field| field.required)
            .map(|field| field.name.as_str())
            .collect();
        assert_eq!(inner, ["symbol", "imported_from"]);

        // 2. What a field actually means, in the words the type carries.
        let symbol = must_call
            .inner
            .iter()
            .find(|field| field.name == "symbol")
            .expect("named");
        assert!(
            symbol.description.contains("call site"),
            "{}",
            symbol.description
        );

        // 3. And an example that can be pasted.
        assert!(entry.example.is_some());
    }

    /// Defaults, which decide whether a field has to be written at all. The
    /// report names `no-passthrough.forms` as the one it wanted.
    #[test]
    fn a_field_with_a_default_says_what_it_is() {
        let read = options();
        let Some(Found::Kind(entry)) = read.find("no-passthrough") else {
            panic!("a kind");
        };
        let forms = entry
            .fields
            .iter()
            .find(|field| field.name == "forms")
            .expect("the field");

        assert!(!forms.required);
        assert_eq!(
            forms.default,
            Some(serde_json::json!(["reexport", "alias", "wrapper"]))
        );
    }

    /// Every kind archwarden has is answered about. A kind added later with no
    /// entry here would be one an agent is told does not exist — which is the
    /// shape of failure this whole issue is about.
    #[test]
    fn every_kind_the_tool_has_is_answered_about() {
        let read = options();

        let answered: Vec<&str> = read.kinds.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(answered.len(), 10, "{answered:?}");

        for kind in &answered {
            let Some(Found::Kind(entry)) = read.find(kind) else {
                panic!("{kind} is not found by name");
            };
            assert!(!entry.fields.is_empty(), "{kind} has no fields");
            assert!(
                entry.example.is_some(),
                "{kind} has no example to paste — add one to EXAMPLES"
            );
        }
    }

    /// The config's own keys, not only the rule kinds. The reported behaviour
    /// is reaching into `node_modules` for the schema, and `governance`,
    /// `extends` and `modules` send somebody there just as `call-obligation`
    /// does.
    #[test]
    fn the_configs_own_keys_are_answered_about_too() {
        let read = options();

        let named: Vec<&str> = read
            .top_level
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();

        for key in [
            "version",
            "modules",
            "rules",
            "governance",
            "extends",
            "ignore",
        ] {
            assert!(named.contains(&key), "{key} missing from {named:?}");
        }
        assert!(
            matches!(read.find("governance"), Some(Found::Key(field)) if !field.description.is_empty()),
            "and each says what it is for"
        );
        assert!(
            matches!(read.find("version"), Some(Found::Key(field)) if field.required),
            "`version` is the one key a config cannot leave out"
        );
    }

    /// The one thing here that is written rather than generated. An example
    /// that stops parsing is worse than no example: it teaches a shape the
    /// tool refuses.
    #[test]
    fn every_example_parses() {
        for (kind, example) in EXAMPLES {
            let config = format!(r#"{{"version":0,"rules":[{example}]}}"#);
            let parsed = archwarden_config::discovery::parse(
                camino::Utf8Path::new("/repo/arch.config.json"),
                &config,
            )
            .unwrap_or_else(|error| panic!("the `{kind}` example does not parse: {error}"));

            assert_eq!(parsed.rules.len(), 1, "{kind}");
        }
    }

    /// And every example is of the kind it is filed under, which a copy-paste
    /// slip would break silently.
    #[test]
    fn every_example_is_of_the_kind_it_is_filed_under() {
        for (kind, example) in EXAMPLES {
            let parsed: serde_json::Value =
                serde_json::from_str(example).expect("the example is JSON");
            assert_eq!(parsed["type"], *kind);
        }
    }

    /// A name nothing has comes back as nothing, and the caller has the list
    /// to say so with.
    #[test]
    fn a_name_nothing_has_is_answered_with_the_names_there_are() {
        let read = options();

        assert!(read.find("nao-existe").is_none());
        assert!(read.names().contains(&"call-obligation"));
        assert!(read.names().contains(&"governance"));
    }

    /// `type` is the entry's name, not a field somebody writes twice. Listing
    /// it would have a reader put `"type"` inside the rule they are already
    /// writing a `type` for.
    #[test]
    fn the_type_discriminant_is_not_listed_as_a_field() {
        let read = options();

        for entry in &read.kinds {
            assert!(
                !entry.fields.iter().any(|field| field.name == "type"),
                "{} lists `type`: {:?}",
                entry.name,
                entry.fields.iter().map(|f| &f.name).collect::<Vec<_>>()
            );
        }
    }

    /// The example a kind carries is that kind's, and it is real. An empty or
    /// wrong one is worse than none: it teaches a shape the tool refuses.
    #[test]
    fn the_example_a_kind_carries_is_its_own() {
        let read = options();

        for kind in ["call-obligation", "presence", "import-boundary"] {
            let Some(Found::Kind(entry)) = read.find(kind) else {
                panic!("{kind} is a kind archwarden has");
            };
            let example = entry.example.as_deref().expect("an example to paste");

            let parsed: serde_json::Value =
                serde_json::from_str(example).expect("the example is JSON");
            assert_eq!(parsed["type"], kind, "filed under the wrong kind");
            assert!(
                example.len() > 40,
                "and it is a rule, not a stub: {example}"
            );
        }
    }

    /// A name nothing has carries no example, rather than the first one that
    /// happened to be in the table.
    #[test]
    fn a_kind_nothing_has_carries_no_example() {
        assert_eq!(example_for("nao-existe"), None);
        assert!(example_for("presence").is_some());
    }

    /// Required fields come first in the list, not only in the summary line.
    /// A reader scanning the detail is deciding what to write, and the half
    /// they must get right should be the half they meet first.
    #[test]
    fn required_fields_are_listed_before_optional_ones() {
        let read = options();
        let Some(Found::Kind(entry)) = read.find("call-obligation") else {
            panic!("a kind");
        };

        let first_optional = entry.fields.iter().position(|field| !field.required);
        let last_required = entry.fields.iter().rposition(|field| field.required);

        assert!(
            matches!((first_optional, last_required), (Some(optional), Some(required)) if required < optional),
            "{:?}",
            entry
                .fields
                .iter()
                .map(|field| (&field.name, field.required))
                .collect::<Vec<_>>()
        );
    }
}
