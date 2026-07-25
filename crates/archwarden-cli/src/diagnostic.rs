//! Turning a [`LoadError`] into something a human can act on.
//!
//! `serde_json` reports "expected `,` or `}` at line 7 column 3", which is
//! accurate and unhelpful. miette renders the same fact as the offending line
//! with a caret under it, which is the difference between a user fixing their
//! config in ten seconds and opening the schema.

use archwarden_config::{discovery::LoadError, extends::ExtendsError};
use miette::{Diagnostic, NamedSource, SourceSpan};

/// A config problem, ready to render.
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("{message}")]
#[diagnostic(code(archwarden::config))]
pub struct ConfigDiagnostic {
    message: String,

    #[source_code]
    source_text: Option<NamedSource<String>>,

    #[label("here")]
    span: Option<SourceSpan>,

    #[help]
    help: Option<String>,
}

impl ConfigDiagnostic {
    /// Builds a diagnostic from a load failure.
    #[must_use]
    pub fn from_load_error(error: &LoadError) -> Self {
        match error {
            LoadError::NotFound { started_at } => Self {
                message: error.to_string(),
                source_text: None,
                span: None,
                help: Some(format!(
                    "run `archwarden init` in the repository root, or pass \
                     `--config <path>`. The search started at `{started_at}` \
                     and walked up to the filesystem root."
                )),
            },

            LoadError::Invalid {
                path,
                source_text,
                source,
            } => Self {
                message: format!("{path} is not a valid archwarden config: {source}"),
                source_text: Some(NamedSource::new(path.as_str(), source_text.clone())),
                span: byte_offset(source_text, source.line(), source.column())
                    .map(|offset| SourceSpan::new(offset.into(), 1)),
                help: Some(
                    "check the field against the schema at \
                     https://archwarden.dev/schema/v0.json"
                        .to_owned(),
                ),
            },

            // `LoadError` is non_exhaustive, so a new variant lands here and
            // gets the bare rendering rather than failing to compile. That is
            // the right default: the message is always correct, only the
            // span and the help are missing.
            _ => Self {
                message: error.to_string(),
                source_text: None,
                span: None,
                help: None,
            },
        }
    }
}

impl ConfigDiagnostic {
    /// Builds a diagnostic from a preset-merge failure.
    ///
    /// A preset problem that is really a load problem is delegated, so a
    /// syntax error inside a preset still gets the same caret a syntax error
    /// in the entry config does.
    #[must_use]
    pub fn from_extends_error(error: &ExtendsError) -> Self {
        match error {
            ExtendsError::Unloadable(inner) => Self::from_load_error(inner),

            ExtendsError::Cycle { .. } => Self {
                message: error.to_string(),
                source_text: None,
                span: None,
                help: Some(
                    "one of these files extends another that eventually \
                     extends it back; break the loop by inlining the shared \
                     rules into a preset neither of them extends"
                        .to_owned(),
                ),
            },

            ExtendsError::DuplicateRuleId { .. } => Self {
                message: error.to_string(),
                source_text: None,
                span: None,
                help: Some(
                    "rule ids must be unique across a config and every preset \
                     it extends, because `explain` and `disable` address rules \
                     by id. Rename one, or `disable` the inherited rule."
                        .to_owned(),
                ),
            },

            _ => Self {
                message: error.to_string(),
                source_text: None,
                span: None,
                help: None,
            },
        }
    }
}

/// Converts `serde_json`'s 1-based line and column into a byte offset.
///
/// Returns `None` when the position falls outside the text, which happens for
/// errors reported at EOF. A diagnostic without a span still renders; one with
/// a wrong span points the user at innocent code.
fn byte_offset(text: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }

    let mut offset = 0;
    for (index, current) in text.lines().enumerate() {
        if index + 1 == line {
            // A column past the end of the line is clamped rather than
            // dropped: the line is still the right place to point at.
            return Some(offset + (column - 1).min(current.len()));
        }
        // `lines()` strips the terminator; assume one byte for it. A `\r\n`
        // file shifts the caret by one column, which is close enough to be
        // useful and cheaper than re-scanning.
        offset += current.len() + 1;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    fn invalid(text: &str) -> LoadError {
        let source = serde_json::from_str::<archwarden_config::config::Config>(text)
            .expect_err("should not parse");
        LoadError::Invalid {
            path: Utf8PathBuf::from("arch.config.json"),
            source_text: text.to_owned(),
            source,
        }
    }

    #[test]
    fn the_first_line_starts_at_offset_zero() {
        assert_eq!(byte_offset("abc\ndef", 1, 1), Some(0));
        assert_eq!(byte_offset("abc\ndef", 1, 3), Some(2));
    }

    #[test]
    fn later_lines_account_for_the_terminator() {
        assert_eq!(byte_offset("abc\ndef", 2, 1), Some(4));
        assert_eq!(byte_offset("abc\ndef\nghi", 3, 2), Some(9));
    }

    /// A column past the end of its line still points at that line, because
    /// the line is the useful part. Pointing nowhere would be worse.
    #[test]
    fn a_column_past_the_end_of_a_line_is_clamped_to_it() {
        assert_eq!(byte_offset("abc\ndef", 1, 99), Some(3));
    }

    /// Better no caret than a caret under innocent code: an out-of-range
    /// position yields no span rather than a guessed one.
    #[test]
    fn an_out_of_range_position_yields_no_span() {
        assert_eq!(byte_offset("abc", 9, 1), None);
        assert_eq!(byte_offset("abc", 0, 1), None);
        assert_eq!(byte_offset("abc", 1, 0), None);
        assert_eq!(byte_offset("", 1, 1), None);
    }

    /// A missing config cannot show a span, but it can say what to do next.
    #[test]
    fn a_missing_config_offers_a_way_forward_instead_of_a_span() {
        let error = LoadError::NotFound {
            started_at: Utf8PathBuf::from("/repo/packages/app"),
        };
        let diagnostic = ConfigDiagnostic::from_load_error(&error);

        assert!(diagnostic.span.is_none());
        assert!(diagnostic.source_text.is_none());
        let help = diagnostic.help.as_deref().expect("has help");
        assert!(help.contains("archwarden init"), "{help}");
        assert!(help.contains("/repo/packages/app"), "{help}");
    }

    /// The whole point of miette here: a syntax error carries the file's text
    /// and a position inside it, so the caller can underline the mistake.
    #[test]
    fn a_syntax_error_carries_the_text_and_a_span() {
        let diagnostic = ConfigDiagnostic::from_load_error(&invalid("{\n  \"version\": 0,,\n}"));

        assert!(diagnostic.source_text.is_some());
        let span = diagnostic.span.expect("has a span");
        assert!(span.offset() > 0, "span points into the second line");
        assert!(diagnostic.to_string().contains("arch.config.json"));
    }

    /// A schema violation is not a syntax error, but it still has a position,
    /// so it still gets a caret.
    #[test]
    fn a_schema_violation_also_gets_a_span() {
        let diagnostic =
            ConfigDiagnostic::from_load_error(&invalid(r#"{"version": 0, "rules": "nope"}"#));

        assert!(diagnostic.span.is_some());
        assert!(diagnostic.help.is_some(), "points at the published schema");
    }

    /// An unreadable file has nothing to underline and nothing useful to
    /// suggest, so it says only what happened.
    #[test]
    fn an_unreadable_file_yields_a_bare_message() {
        let error = LoadError::Unreadable {
            path: Utf8PathBuf::from("arch.config.json"),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        };
        let diagnostic = ConfigDiagnostic::from_load_error(&error);

        assert!(diagnostic.span.is_none());
        assert!(diagnostic.help.is_none());
        assert!(diagnostic.to_string().contains("arch.config.json"));
    }
}
