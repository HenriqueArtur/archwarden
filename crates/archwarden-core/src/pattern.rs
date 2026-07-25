//! Filename patterns.
//!
//! archwarden matches filenames with Rust's `regex` crate, which guarantees
//! linear-time matching and therefore has no lookaround and no backreferences.
//! That trade is deliberate: archwarden runs inside pre-commit hooks and agent
//! pre-write hooks, where a catastrophically backtracking pattern would be a
//! denial of service on the user's own workflow. See decision 3.
//!
//! The cost lands on users porting a pattern from JavaScript, where lookahead
//! is ordinary. `regex` rejects those with a message about its own internals,
//! so this module recognises the construct first and says what to do instead.

use regex::Regex;

/// Why a pattern could not be compiled.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PatternError {
    /// The pattern uses a construct that a linear-time engine cannot support.
    #[error(
        "pattern `{pattern}` uses {construct}, which archwarden's regex engine \
         does not support. {suggestion}"
    )]
    Unsupported {
        /// The pattern as written.
        pattern: String,
        /// What was found, in words.
        construct: &'static str,
        /// What to do instead.
        suggestion: &'static str,
    },

    /// The pattern is not valid at all.
    #[error("pattern `{pattern}` is not a valid regular expression")]
    Invalid {
        /// The pattern as written.
        pattern: String,
        /// What the engine said.
        #[source]
        source: Box<regex::Error>,
    },
}

/// A compiled filename pattern.
///
/// Keeps the source text beside the compiled form, because every diagnostic
/// quotes the pattern the user wrote rather than a normalised version of it.
#[derive(Debug, Clone)]
pub struct Pattern {
    regex: Regex,
    source: String,
}

impl Pattern {
    /// Compiles a pattern.
    ///
    /// # Errors
    /// See [`PatternError`].
    pub fn compile(source: &str) -> Result<Self, PatternError> {
        if let Some((construct, suggestion)) = unsupported_construct(source) {
            return Err(PatternError::Unsupported {
                pattern: source.to_owned(),
                construct,
                suggestion,
            });
        }

        let regex = Regex::new(source).map_err(|error| PatternError::Invalid {
            pattern: source.to_owned(),
            source: Box::new(error),
        })?;

        Ok(Self {
            regex,
            source: source.to_owned(),
        })
    }

    /// Whether the pattern matches.
    #[must_use]
    pub fn is_match(&self, text: &str) -> bool {
        self.regex.is_match(text)
    }

    /// The named capture groups this pattern defines.
    #[must_use]
    pub fn capture_names(&self) -> Vec<&str> {
        self.regex.capture_names().flatten().collect()
    }

    /// Looks up a named capture from a match against `text`.
    #[must_use]
    pub fn capture<'t>(&self, text: &'t str, name: &str) -> Option<&'t str> {
        self.regex
            .captures(text)
            .and_then(|captures| captures.name(name))
            .map(|matched| matched.as_str())
    }

    /// The pattern as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.source
    }
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Eq for Pattern {}

impl std::fmt::Display for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(&self.source)
    }
}

/// Recognises the constructs a linear-time engine cannot have.
///
/// Done before handing the pattern to `regex` so the message names the
/// construct and offers a way round it, instead of reporting that the engine
/// failed to parse something.
fn unsupported_construct(source: &str) -> Option<(&'static str, &'static str)> {
    let bytes = source.as_bytes();

    for (index, window) in bytes.windows(3).enumerate() {
        // A `(` that is escaped is a literal parenthesis, not a group.
        if window.first() != Some(&b'(') || is_escaped(bytes, index) {
            continue;
        }

        match (window.get(1), window.get(2)) {
            (Some(b'?'), Some(b'=')) => {
                return Some((
                    "lookahead `(?=...)`",
                    "Express the requirement as a separate pattern in the list: \
                     a file matches the rule if it matches any one of them.",
                ));
            }
            (Some(b'?'), Some(b'!')) => {
                return Some((
                    "negative lookahead `(?!...)`",
                    "Match what is allowed rather than what is forbidden. \
                     `filename_patterns` already means \"must match one of \
                     these\", so listing the allowed shapes excludes the rest.",
                ));
            }
            // `(?<` is a named group unless the next character makes it a
            // lookbehind, so the two have to be told apart rather than the
            // prefix being rejected outright.
            (Some(b'?'), Some(b'<')) => match bytes.get(index + 3) {
                Some(b'=') => {
                    return Some((
                        "lookbehind `(?<=...)`",
                        "Include the preceding text in the pattern itself and \
                         capture only the part you need.",
                    ));
                }
                Some(b'!') => {
                    return Some((
                        "negative lookbehind `(?<!...)`",
                        "Match what is allowed rather than what is forbidden.",
                    ));
                }
                _ => {}
            },
            _ => {}
        }
    }

    if let Some(digit) = backreference(source) {
        return Some((
            match digit {
                1 => "backreference `\\1`",
                _ => "a backreference",
            },
            "Repeat the sub-pattern instead of referring back to it.",
        ));
    }

    None
}

/// Finds a `\N` backreference, ignoring `\\N` where the backslash is escaped.
fn backreference(source: &str) -> Option<u32> {
    let bytes = source.as_bytes();

    bytes.iter().enumerate().find_map(|(index, byte)| {
        if *byte != b'\\' || is_escaped(bytes, index) {
            return None;
        }
        bytes
            .get(index + 1)
            .filter(|next| next.is_ascii_digit() && **next != b'0')
            .map(|next| u32::from(next - b'0'))
    })
}

/// Whether the byte at `index` is preceded by an odd number of backslashes,
/// which is what makes it escaped rather than significant.
fn is_escaped(bytes: &[u8], index: usize) -> bool {
    bytes
        .iter()
        .take(index)
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(source: &str) -> Pattern {
        Pattern::compile(source).expect("should compile")
    }

    /// Destructures an `Unsupported` error, or returns `None`.
    ///
    /// Written as a function rather than as `let ... else { panic!() }` at
    /// every call site: that `panic!` arm never runs when the test passes, so
    /// it is dead code no run can reach. Here the `None` branch is reachable,
    /// and one test below reaches it.
    fn unsupported(error: &PatternError) -> Option<(&str, &str, &str)> {
        match error {
            PatternError::Unsupported {
                pattern,
                construct,
                suggestion,
            } => Some((pattern, construct, suggestion)),
            PatternError::Invalid { .. } => None,
        }
    }

    fn invalid(error: &PatternError) -> Option<&str> {
        match error {
            PatternError::Invalid { pattern, .. } => Some(pattern),
            PatternError::Unsupported { .. } => None,
        }
    }

    /// The pattern from docs/CONFIG.md, which is the shape almost every real
    /// rule uses.
    #[test]
    fn a_documented_filename_pattern_compiles_and_captures() {
        let pattern = compile(r"^(?<name>[a-z0-9-]+)\.use-case\.ts$");

        assert!(pattern.is_match("create-client.use-case.ts"));
        assert!(!pattern.is_match("create-client.use-case.spec.ts"));
        assert_eq!(
            pattern.capture("create-client.use-case.ts", "name"),
            Some("create-client")
        );
        assert_eq!(pattern.capture_names(), ["name"]);
    }

    #[test]
    fn a_capture_that_does_not_match_yields_nothing() {
        let pattern = compile(r"^(?<name>[a-z]+)\.ts$");
        assert_eq!(pattern.capture("NOT-MATCHING.ts", "name"), None);
        assert_eq!(pattern.capture("ok.ts", "missing"), None);
    }

    /// The construct a user porting from JavaScript reaches for first. The
    /// message has to name it and offer a way round, or the user is left with
    /// an engine-internals error.
    #[test]
    fn lookahead_is_rejected_by_name_with_a_suggestion() {
        let err = Pattern::compile(r"^(?=.*spec).*\.ts$").expect_err("should reject");
        let (pattern, construct, suggestion) = unsupported(&err).expect("is Unsupported");

        assert!(construct.contains("lookahead"), "{construct}");
        assert!(!suggestion.is_empty());
        assert_eq!(pattern, r"^(?=.*spec).*\.ts$");
    }

    /// Negative lookahead is the common way to write "not a spec file", so its
    /// suggestion points at the rule's own semantics rather than at regex.
    #[test]
    fn negative_lookahead_is_rejected_with_advice_about_the_rule() {
        let err = Pattern::compile(r"^(?!.*\.spec\.ts$).*\.ts$").expect_err("should reject");
        let (_, construct, suggestion) = unsupported(&err).expect("is Unsupported");

        assert!(construct.contains("negative lookahead"), "{construct}");
        assert!(suggestion.contains("filename_patterns"), "{suggestion}");
    }

    #[test]
    fn lookbehind_in_both_polarities_is_rejected() {
        for source in [r"(?<=foo)bar", r"(?<!foo)bar"] {
            let err = Pattern::compile(source).expect_err("should reject");
            let (_, construct, _) = unsupported(&err).expect("is Unsupported");
            assert!(construct.contains("lookbehind"), "{source}: {construct}");
        }
    }

    /// The distinction that makes the detector worth writing: `(?<name>` is a
    /// named group and must keep working, while `(?<=` is a lookbehind.
    #[test]
    fn a_named_group_is_not_mistaken_for_a_lookbehind() {
        let pattern = compile(r"^(?<name>[a-z]+)$");
        assert_eq!(pattern.capture("abc", "name"), Some("abc"));

        assert!(Pattern::compile(r"^(?<n>a)(?<m>b)$").is_ok());
    }

    #[test]
    fn a_backreference_is_rejected() {
        let err = Pattern::compile(r"^(a)\1$").expect_err("should reject");
        let (_, construct, _) = unsupported(&err).expect("is Unsupported");
        assert!(construct.contains("backreference"), "{construct}");

        let err = Pattern::compile(r"^(a)(b)\2$").expect_err("should reject");
        assert!(unsupported(&err).is_some());
    }

    /// `\d` is a digit class, not a backreference, and `\0` is a null byte.
    /// Rejecting either would break ordinary patterns.
    #[test]
    fn escape_sequences_that_are_not_backreferences_still_compile() {
        assert!(Pattern::compile(r"^\d+\.ts$").is_ok());
        assert!(Pattern::compile(r"^route\.(get|post)\.ts$").is_ok());
        assert!(Pattern::compile(r"^a\\b$").is_ok());
    }

    /// An escaped `(` is a literal parenthesis. Treating `\(?=` as a group
    /// would reject a pattern that is perfectly fine.
    #[test]
    fn an_escaped_parenthesis_is_not_a_group() {
        let pattern = compile(r"^foo\(\?=bar$");
        assert!(pattern.is_match("foo(?=bar"));
    }

    /// Likewise an escaped backslash followed by a digit is a literal
    /// backslash and a literal digit.
    #[test]
    fn an_escaped_backslash_before_a_digit_is_not_a_backreference() {
        assert!(Pattern::compile(r"^a\\1$").is_ok());
    }

    /// A pattern that is simply broken gets the engine's own message, since
    /// there is nothing more specific to say about it.
    #[test]
    fn a_malformed_pattern_is_reported_as_invalid() {
        let err = Pattern::compile(r"^[unclosed").expect_err("should reject");
        assert_eq!(invalid(&err), Some("^[unclosed"));
        assert!(err.to_string().contains("not a valid regular expression"));

        // The two classifications are mutually exclusive: a malformed pattern
        // is not also reported as an unsupported construct.
        assert!(unsupported(&err).is_none());
        let lookahead = Pattern::compile(r"(?=x)").expect_err("should reject");
        assert!(invalid(&lookahead).is_none());
    }

    /// Two patterns are the same when their source text is, which is what a
    /// config round-trip and a rules hash both rely on.
    #[test]
    fn patterns_compare_and_render_by_their_source() {
        let pattern = compile(r"^a\.ts$");
        assert_eq!(pattern, compile(r"^a\.ts$"));
        assert_ne!(pattern, compile(r"^b\.ts$"));
        assert_eq!(pattern.as_str(), r"^a\.ts$");
        assert_eq!(pattern.to_string(), r"^a\.ts$");
    }

    #[test]
    fn a_pattern_without_named_groups_reports_none() {
        assert!(compile(r"^route\.ts$").capture_names().is_empty());
    }
}
