//! Finding the exact bytes a config error is about.
//!
//! `serde_json` reports where its *parser* had reached, which for a schema
//! violation is past the offending value: by the time serde knows a field is
//! missing it has read the closing brace, and a caret there accuses the next
//! rule of the previous one's mistake. Only a syntax error can be pointed at
//! with that number, which is why the diagnostic layer used to show a caret
//! for one error kind out of four.
//!
//! So the document is parsed a second time, into an AST that keeps byte
//! ranges, and the path `serde_path_to_error` collected is walked through it.
//! Two parses of a config file is nothing -- it happens once, on the way to
//! printing an error -- and the result is a caret on the member the user has
//! to change.

use archwarden_config::discovery::PathSegment;
use jsonc_parser::ast::{Object, ObjectPropName, Value};

/// A byte range in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// First byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
}

impl Span {
    /// The length in bytes, for miette.
    #[must_use]
    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether the span covers nothing.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
}

/// The span of whatever `segments` names, or `None` when the document does not
/// have that shape.
///
/// `None` rather than a guess: a caret in the wrong place is worse than none,
/// because the user trusts it. That is the whole reason this module exists.
#[must_use]
pub fn locate(source_text: &str, segments: &[PathSegment]) -> Option<Span> {
    let parsed = jsonc_parser::parse_to_ast(
        source_text,
        &jsonc_parser::CollectOptions::default(),
        &jsonc_parser::ParseOptions::default(),
    )
    .ok()?;

    let mut node = parsed.value.as_ref()?;
    for segment in segments {
        node = step(node, segment)?;
    }

    Some(span_of(node))
}

/// The span of one member's *name* inside whatever `segments` names.
///
/// For "unknown field `allow`" serde reports the containing object as the
/// path, because the field is not part of the struct it was building. The
/// object is the right neighbourhood and the wrong precision: the user wants
/// the caret on the word they misspelt.
#[must_use]
pub fn locate_key(source_text: &str, segments: &[PathSegment], key: &str) -> Option<Span> {
    let parsed = jsonc_parser::parse_to_ast(
        source_text,
        &jsonc_parser::CollectOptions::default(),
        &jsonc_parser::ParseOptions::default(),
    )
    .ok()?;

    let mut node = parsed.value.as_ref()?;
    for segment in segments {
        node = step(node, segment)?;
    }

    let Value::Object(object) = node else {
        return None;
    };
    key_span(object, key)
}

/// The field name in a serde "unknown field `x`" message.
///
/// Reading our own dependency's prose is not lovely, but the alternative is a
/// caret on the whole object when the exact word is right there. The format
/// has been stable for serde's lifetime, and a change to it makes this return
/// `None` rather than point somewhere wrong.
#[must_use]
pub fn unknown_field(message: &str) -> Option<&str> {
    let rest = message.strip_prefix("unknown field `")?;
    rest.split_once('`').map(|(field, _)| field)
}

fn step<'a>(node: &'a Value<'a>, segment: &PathSegment) -> Option<&'a Value<'a>> {
    match (node, segment) {
        (Value::Object(object), PathSegment::Key(key)) => {
            object.get(key.as_str()).map(|property| &property.value)
        }
        (Value::Array(array), PathSegment::Index(index)) => array.elements.get(*index),
        _ => None,
    }
}

fn key_span(object: &Object<'_>, key: &str) -> Option<Span> {
    object
        .properties
        .iter()
        .find(|property| property.name.as_str() == key)
        .map(|property| {
            let range = match &property.name {
                ObjectPropName::String(literal) => literal.range,
                ObjectPropName::Word(literal) => literal.range,
            };
            Span {
                start: range.start,
                end: range.end,
            }
        })
}

fn span_of(node: &Value<'_>) -> Span {
    let range = match node {
        Value::StringLit(literal) => literal.range,
        Value::NumberLit(literal) => literal.range,
        Value::BooleanLit(literal) => literal.range,
        Value::Object(object) => object.range,
        Value::Array(array) => array.range,
        Value::NullKeyword(keyword) => keyword.range,
    };

    Span {
        start: range.start,
        end: range.end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"{
  "version": 0,
  "rules": [
    { "type": "structure", "id": "first", "level": "error", "roots": "src/*" },
    { "type": "naming", "id": "second", "level": "error", "roots": ["a", "b"] }
  ]
}"#;

    fn key(name: &str) -> PathSegment {
        PathSegment::Key(name.to_owned())
    }

    fn text_at(span: Span) -> &'static str {
        CONFIG.get(span.start..span.end).expect("a real span")
    }

    /// The root, which is what an empty path names.
    #[test]
    fn an_empty_path_is_the_whole_document() {
        let span = locate(CONFIG, &[]).expect("the root");
        assert_eq!(span.start, 0);
        assert_eq!(span.end, CONFIG.len());
    }

    /// A top-level member.
    #[test]
    fn a_key_finds_its_value() {
        assert_eq!(
            text_at(locate(CONFIG, &[key("version")]).expect("found")),
            "0"
        );
    }

    /// The case the whole module exists for: one rule out of several. A caret
    /// on the wrong rule is the failure this replaces.
    #[test]
    fn an_index_finds_the_right_element() {
        let span = locate(CONFIG, &[key("rules"), PathSegment::Index(1)]).expect("found");
        let found = text_at(span);

        assert!(found.starts_with('{') && found.ends_with('}'), "{found}");
        assert!(found.contains("\"second\""), "{found}");
        assert!(
            !found.contains("\"first\""),
            "the other rule is not it: {found}"
        );
    }

    /// And down to a field inside it.
    #[test]
    fn a_path_walks_all_the_way_down() {
        let span =
            locate(CONFIG, &[key("rules"), PathSegment::Index(1), key("roots")]).expect("found");

        assert_eq!(text_at(span), r#"["a", "b"]"#);
    }

    /// A path the document does not have gets no span. A caret in the wrong
    /// place is worse than none, because the user trusts it.
    #[test]
    fn a_path_that_does_not_fit_gets_nothing() {
        for segments in [
            vec![key("nope")],
            vec![key("rules"), PathSegment::Index(9)],
            vec![key("version"), key("deeper")],
            vec![key("rules"), key("not-an-index")],
            vec![PathSegment::Index(0)],
        ] {
            assert_eq!(locate(CONFIG, &segments), None, "{segments:?}");
        }
    }

    /// A document that will not parse has no spans to give, and the syntax
    /// error already has a trustworthy position of its own.
    #[test]
    fn a_broken_document_gets_nothing() {
        assert_eq!(locate("{ oops", &[key("version")]), None);
        assert_eq!(locate("", &[]), None);
    }

    /// For an unknown field, serde names the containing object. The caret goes
    /// on the misspelt word inside it.
    #[test]
    fn a_key_span_lands_on_the_name_not_the_value() {
        let span =
            locate_key(CONFIG, &[key("rules"), PathSegment::Index(0)], "roots").expect("found");

        assert_eq!(text_at(span), r#""roots""#);
    }

    /// A key that is not there gets nothing, and the caller falls back to the
    /// object.
    #[test]
    fn a_key_that_is_not_there_gets_nothing() {
        assert_eq!(
            locate_key(CONFIG, &[key("rules"), PathSegment::Index(0)], "absent"),
            None
        );
        assert_eq!(
            locate_key(CONFIG, &[key("version")], "anything"),
            None,
            "a number has no members"
        );
    }

    /// The field name is read out of serde's message, and a message of another
    /// shape yields nothing rather than a guess.
    #[test]
    fn the_unknown_field_is_read_from_the_message() {
        assert_eq!(
            unknown_field("unknown field `allow`, expected one of `id`, `level`"),
            Some("allow")
        );
        assert_eq!(unknown_field("missing field `roots`"), None);
        assert_eq!(unknown_field("unknown field `unterminated"), None);
        assert_eq!(unknown_field(""), None);
    }

    /// Every value kind carries a range, so a caret can land on any of them.
    #[test]
    fn every_value_kind_has_a_span() {
        let document = r#"{"s":"x","n":1,"b":true,"o":{},"a":[],"z":null}"#;

        for (name, expected) in [
            ("s", "\"x\""),
            ("n", "1"),
            ("b", "true"),
            ("o", "{}"),
            ("a", "[]"),
            ("z", "null"),
        ] {
            let span = locate(document, &[key(name)]).expect("found");
            assert_eq!(
                document.get(span.start..span.end),
                Some(expected),
                "for `{name}`"
            );
        }
    }

    /// A span is measured in bytes, so a document with multi-byte characters
    /// before the target still points at the right place.
    #[test]
    fn spans_are_byte_offsets() {
        let document = r#"{"café":"au lait","target":42}"#;
        let span = locate(document, &[key("target")]).expect("found");

        assert_eq!(document.get(span.start..span.end), Some("42"));
    }

    /// The length is what miette wants, and an empty span is a span that
    /// points at nothing.
    #[test]
    fn a_span_reports_its_length() {
        let span = locate(CONFIG, &[key("version")]).expect("found");

        assert_eq!(span.len(), 1);
        assert!(!span.is_empty());
        assert!(Span { start: 4, end: 4 }.is_empty());
    }
}
