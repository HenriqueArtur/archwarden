//! Turning a [`LoadError`] into something a human can act on.
//!
//! `serde_json` reports "expected `,` or `}` at line 7 column 3", which is
//! accurate and unhelpful. miette renders the same fact as the offending line
//! with a caret under it, which is the difference between a user fixing their
//! config in ten seconds and opening the schema.

use archwarden_config::{compile::CompileError, discovery::LoadError, extends::ExtendsError};
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
                segments,
                source,
            } => {
                // Two ways to point at the problem, and they are not
                // interchangeable. A syntax error's own position is exact --
                // the parser stopped at the offending byte. A schema
                // violation's is not: serde has already read past the value,
                // so its number lands on the next token and accuses innocent
                // code. For those, the path is walked through a span-keeping
                // parse of the same text instead.
                let reason = strip_position(&source.to_string());
                let syntax = matches!(
                    source.classify(),
                    serde_json::error::Category::Syntax | serde_json::error::Category::Eof
                );

                let span = if syntax {
                    byte_offset(source_text, source.line(), source.column())
                        .map(|offset| SourceSpan::new(offset.into(), 1))
                } else {
                    // For "unknown field `x`" serde names the containing
                    // object, because the field is not part of the struct it
                    // was building. The word the user misspelt is right
                    // there, so the caret goes on it; failing that, on the
                    // object, which is still the right neighbourhood.
                    crate::locate::unknown_field(&reason)
                        .and_then(|field| crate::locate::locate_key(source_text, segments, field))
                        .or_else(|| crate::locate::locate(source_text, segments))
                        .map(|found| SourceSpan::new(found.start.into(), found.len()))
                };

                // The path stays in the message even when the caret lands.
                // A caret shows *where*; `rules[1].roots` says *what*, and a
                // user reading a failure in CI has only the text.
                let pointer = archwarden_config::discovery::render_path(segments);
                let located = if pointer.is_empty() {
                    reason
                } else {
                    format!("at `{pointer}`: {reason}")
                };

                Self {
                    message: format!("{path} is not a valid archwarden config: {located}"),
                    source_text: span
                        .is_some()
                        .then(|| NamedSource::new(path.as_str(), source_text.clone())),
                    span,
                    help: Some(format!(
                        "check the field against the schema at {}",
                        crate::schema::SCHEMA_URL
                    )),
                }
            }

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

    /// Builds a diagnostic from a compilation failure.
    ///
    /// These carry no span: by this point the config has been merged from
    /// several files and the offending value no longer has one position. Every
    /// `CompileError` names its rule instead, which is what a user searches by.
    #[must_use]
    pub fn from_compile_error(error: &CompileError) -> Self {
        Self {
            message: error.to_string(),
            source_text: None,
            span: None,
            help: match error {
                CompileError::Pattern { .. } => Some(
                    "archwarden matches filenames with a linear-time regex \
                     engine, which is what keeps a pathological pattern from \
                     hanging a pre-commit hook. See docs/CONFIG.md."
                        .to_owned(),
                ),
                _ => None,
            },
        }
    }

    /// Builds a diagnostic from anything an operation returned.
    ///
    /// The single entry point the CLI uses, and the reason the boundary in
    /// issue #63 costs nothing in diagnostic quality: the wrapped variants
    /// delegate to the constructors above, which still hold the source text
    /// and the byte offsets they need to draw a caret.
    ///
    /// [`archwarden_api::Error`] is `non_exhaustive`, so a stage added later
    /// lands in the final arm and gets the bare rendering rather than failing
    /// to compile. The message is always correct there; only the help is
    /// missing.
    #[must_use]
    pub fn from_api_error(error: &archwarden_api::Error) -> Self {
        match error {
            archwarden_api::Error::Load(inner) => Self::from_load_error(inner),
            archwarden_api::Error::Extends(inner) => Self::from_extends_error(inner),
            archwarden_api::Error::Compile(inner) => Self::from_compile_error(inner),

            // The one config problem whose fix is not in the config. Saying
            // "upgrade" first matters: a user reading a complaint about their
            // own file's version number will otherwise edit the number, which
            // makes this build read a config written for a schema it does not
            // have — the exact silence issue #55 was.
            archwarden_api::Error::UnsupportedVersion { understood, .. } => Self {
                message: error.to_string(),
                source_text: None,
                span: None,
                help: Some(format!(
                    "upgrade archwarden to a build that reads this version. \
                     Lowering the file's `version` to {understood} would make \
                     this build parse it, and silently ignore everything the \
                     newer schema added."
                )),
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

/// Removes the ` at line N column M` suffix `serde_json` appends to every
/// message.
///
/// The position is either redundant, because the caret shows it, or wrong,
/// because a schema violation is reported after the parser has moved past the
/// offending value. Neither case wants it in the sentence.
fn strip_position(message: &str) -> String {
    message
        .rfind(" at line ")
        .map_or_else(|| message.to_owned(), |cut| message[..cut].to_owned())
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

    /// Builds a real `LoadError::Invalid`, through the same parser the loader
    /// uses, so the pointer and the error category are the genuine ones.
    fn invalid(text: &str) -> LoadError {
        archwarden_config::discovery::parse(camino::Utf8Path::new("arch.config.json"), text)
            .expect_err("should not parse")
    }

    #[test]
    fn the_trailing_position_is_removed_from_a_message() {
        assert_eq!(
            strip_position("missing field `level` at line 3 column 41"),
            "missing field `level`"
        );
        assert_eq!(
            strip_position("rule id `a b` contains ` ` at line 6 column 3"),
            "rule id `a b` contains ` `"
        );
    }

    /// A message that mentions a line for its own reasons keeps everything up
    /// to the *last* suffix, so only `serde_json`'s own addition is removed.
    #[test]
    fn only_the_trailing_position_is_removed() {
        assert_eq!(
            strip_position("bad value at line 2 of the file at line 9 column 1"),
            "bad value at line 2 of the file"
        );
        assert_eq!(strip_position("no position here"), "no position here");
        assert_eq!(strip_position(""), "");
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

    /// The version refusal is the one error the orchestration used to write
    /// itself, in a bare sentence with no help, because it had no variant to
    /// be. It has one now, so it renders like every other config problem —
    /// and can finally say the thing the user needs, which is that the fix is
    /// on this side rather than in their file.
    #[test]
    fn an_unsupported_version_says_the_fix_is_on_this_side() {
        let diagnostic =
            ConfigDiagnostic::from_api_error(&archwarden_api::Error::UnsupportedVersion {
                path: Utf8PathBuf::from("/repo/arch.config.json"),
                declared: 99,
                understood: 0,
            });

        assert_eq!(
            diagnostic.message,
            "`/repo/arch.config.json` declares version 99, but this build understands version 0"
        );
        assert!(diagnostic.span.is_none());
        let help = diagnostic.help.as_deref().expect("has help");
        assert!(help.contains("upgrade archwarden"), "{help}");
    }

    /// The three wrapped variants delegate, so a syntax error inside a preset
    /// still gets the caret it got before the boundary existed. This is the
    /// property that stops "errors as values" costing a diagnostic.
    #[test]
    fn a_wrapped_load_error_keeps_the_span_it_would_have_had() {
        let error = invalid(r#"{"version": 0,,}"#);
        let direct = ConfigDiagnostic::from_load_error(&error);
        let through = ConfigDiagnostic::from_api_error(&archwarden_api::Error::Load(error));

        assert_eq!(direct.message, through.message);
        assert_eq!(direct.span, through.span);
        assert!(through.span.is_some());
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

    /// A schema violation now gets a caret too, and it lands on the value the
    /// path names rather than on wherever serde's parser had reached.
    ///
    /// This is the M8d change. Before it, a schema violation got no caret at
    /// all, because `serde_json`'s position has moved past the offending value
    /// by the time serde objects, and a caret on the following token accuses
    /// innocent code.
    #[test]
    fn a_schema_violation_gets_a_caret_on_the_named_value() {
        let source_text = r#"{"version": 0, "rules": "nope"}"#;
        let diagnostic = ConfigDiagnostic::from_load_error(&invalid(source_text));

        let span = diagnostic.span.expect("a caret");
        assert_eq!(
            source_text.get(span.offset()..span.offset() + span.len()),
            Some(r#""nope""#),
            "on the value the path names"
        );
        assert!(
            diagnostic.source_text.is_some(),
            "with the file to render it"
        );
        assert!(diagnostic.to_string().contains("rules"), "{diagnostic}");
        assert!(diagnostic.help.is_some(), "points at the published schema");
    }

    /// An unknown field is reported at the object that contains it, because
    /// the field is not part of the struct serde was building. The caret goes
    /// on the misspelt word anyway -- it is right there in the message.
    #[test]
    fn an_unknown_field_gets_a_caret_on_the_word_itself() {
        let source_text = r#"{"version":0,"rules":[
            {"type":"structure","id":"a","level":"error","roots":"a/*"},
            {"type":"structure","id":"b","level":"error","roots":"b/*","allow":["types"]}]}"#;
        let diagnostic = ConfigDiagnostic::from_load_error(&invalid(source_text));

        let span = diagnostic.span.expect("a caret");
        assert_eq!(
            source_text.get(span.offset()..span.offset() + span.len()),
            Some(r#""allow""#),
            "on the word the user misspelt, in the second rule"
        );
    }

    /// A failure inside a rule is reported at the rule: `Rule` is an
    /// internally tagged enum, and serde loses the path across the buffer it
    /// deserialises one through. The caret still lands on the right rule out
    /// of many, which is the question a user is actually asking.
    #[test]
    fn a_failure_inside_a_rule_puts_the_caret_on_that_rule() {
        let source_text = r#"{"version":0,"rules":[
            {"type":"structure","id":"fine","level":"error","roots":"a/*"},
            {"type":"structure","id":"b","level":"nope","roots":"b/*"}]}"#;
        let diagnostic = ConfigDiagnostic::from_load_error(&invalid(source_text));

        let span = diagnostic.span.expect("a caret");
        let pointed_at = source_text
            .get(span.offset()..span.offset() + span.len())
            .expect("a real span");

        assert!(
            pointed_at.contains(r#""nope""#),
            "the broken rule: {pointed_at}"
        );
        assert!(
            !pointed_at.contains(r#""fine""#),
            "not the innocent one: {pointed_at}"
        );
    }

    /// A broken preset gets the same treatment as a broken entry config: the
    /// failure is delegated rather than re-described, so a caret lands inside
    /// the preset file and the message names *that* file.
    ///
    /// Without the delegation a user whose preset has a typo would be told
    /// only "a preset could not be loaded", with nothing to open.
    #[test]
    fn a_broken_preset_is_delegated_and_keeps_its_caret() {
        let inner = invalid(r#"{"version": 0, "rules": "nope"}"#);
        let direct = ConfigDiagnostic::from_load_error(&inner);
        let through_preset = ConfigDiagnostic::from_extends_error(&ExtendsError::Unloadable(inner));

        assert_eq!(through_preset.to_string(), direct.to_string());
        assert_eq!(through_preset.span, direct.span);
        assert!(through_preset.span.is_some(), "the caret survives the hop");
    }

    /// A document that will not parse as JSON at all keeps the syntax error's
    /// own position, which for a syntax error *is* exact.
    #[test]
    fn a_syntax_error_keeps_its_own_position() {
        let diagnostic = ConfigDiagnostic::from_load_error(&invalid(r#"{"version": 0,,}"#));

        assert!(diagnostic.span.is_some(), "the parser stopped at the byte");
        assert!(diagnostic.source_text.is_some());
    }

    /// The path is what makes a schema violation readable when the caret
    /// cannot be: it survives a copy-paste into a chat window, and it is all a
    /// CI log has.
    #[test]
    fn the_path_names_the_offending_rule_by_index() {
        let diagnostic = ConfigDiagnostic::from_load_error(&invalid(
            r#"{"version":0,"rules":[
                {"type":"structure","id":"fine","level":"error","roots":"a/*"},
                {"type":"structure","id":"bad id","level":"error","roots":"b/*"}]}"#,
        ));

        let message = diagnostic.to_string();
        assert!(message.contains("rules[1]"), "{message}");
        assert!(message.contains("bad id"), "{message}");

        // The path stays in the message even now that a caret lands. A caret
        // shows *where*; `rules[1]` says *what*, and a user reading a CI log
        // has only the text.
        assert!(diagnostic.span.is_some(), "and the caret too");
    }

    /// A failure at the root has no useful path, and an empty one is not
    /// printed rather than rendering as a bare `.`.
    #[test]
    fn a_root_level_failure_prints_no_path() {
        let diagnostic = ConfigDiagnostic::from_load_error(&invalid("[]"));

        let message = diagnostic.to_string();
        assert!(!message.contains("at `"), "{message}");
        assert!(!message.contains('`'), "{message}");
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
