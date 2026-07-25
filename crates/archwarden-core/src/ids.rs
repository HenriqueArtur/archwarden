//! Stable identifiers for rules and modules.
//!
//! These are newtypes rather than `String` because they travel together
//! through the whole pipeline and are trivially swappable at a call site. The
//! compiler catching that is cheaper than a test catching it.
//!
//! Both are also user-facing: a rule id is typed on the command line
//! (`archwarden explain <rule-id>`) and inside a config (`disable`). That is
//! why the character set is restricted — an id with a space in it would need
//! quoting in every context it appears.

use serde::{Deserialize, Serialize};

/// Why an identifier was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IdError {
    /// The identifier was empty or only whitespace.
    #[error("{kind} id must not be empty")]
    Empty {
        /// `rule` or `module`.
        kind: &'static str,
    },
    /// The identifier contained a character outside the allowed set.
    #[error(
        "{kind} id `{id}` contains `{character}`; \
         allowed characters are letters, digits, `-`, `_`, `.` and `/`"
    )]
    InvalidCharacter {
        /// `rule` or `module`.
        kind: &'static str,
        /// The identifier as written.
        id: String,
        /// The first offending character.
        character: char,
    },
}

fn validate(kind: &'static str, raw: &str) -> Result<String, IdError> {
    if raw.trim().is_empty() {
        return Err(IdError::Empty { kind });
    }

    if let Some(character) = raw.chars().find(|c| !is_allowed(*c)) {
        return Err(IdError::InvalidCharacter {
            kind,
            id: raw.to_owned(),
            character,
        });
    }

    Ok(raw.to_owned())
}

/// Characters an identifier may contain.
///
/// `/` is allowed so a preset can namespace its rules (`clean-arch/no-barrel`)
/// without colliding with the ids a project defines itself.
///
/// Alphanumeric is Unicode-wide rather than ASCII-only: an accented id is
/// perfectly typeable and needs no shell quoting. What is excluded is
/// punctuation and whitespace, which do.
fn is_allowed(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/')
}

macro_rules! id_newtype {
    ($name:ident, $kind:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Validates and wraps an identifier.
            ///
            /// # Errors
            /// See [`IdError`].
            pub fn new(raw: impl AsRef<str>) -> Result<Self, IdError> {
                validate($kind, raw.as_ref()).map(Self)
            }

            /// The identifier as written.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(d)?;
                Self::new(raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

id_newtype!(
    RuleId,
    "rule",
    "A rule's stable identifier, unique across a config and its presets."
);
id_newtype!(
    ModuleId,
    "module",
    "A module's identifier. Modules are labels for grouping output, not scopes."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_kebab_id_is_accepted() {
        let id = RuleId::new("domain-entity-shape").expect("valid");
        assert_eq!(id.as_str(), "domain-entity-shape");
        assert_eq!(id.to_string(), "domain-entity-shape");
    }

    /// A preset namespaces its rules with `/` so they cannot collide with the
    /// ids a project defines for itself.
    #[test]
    fn a_preset_may_namespace_with_a_slash() {
        assert!(RuleId::new("clean-arch/no-barrel-files").is_ok());
        assert!(RuleId::new("v2.no_barrel").is_ok());
    }

    #[test]
    fn an_empty_id_is_rejected() {
        assert_eq!(RuleId::new(""), Err(IdError::Empty { kind: "rule" }));
        assert_eq!(ModuleId::new("   "), Err(IdError::Empty { kind: "module" }));
    }

    /// An id is typed on the command line and written in `disable`. A space
    /// would need quoting everywhere it appears, so it is rejected at the
    /// boundary instead.
    #[test]
    fn whitespace_inside_an_id_is_rejected() {
        let err = RuleId::new("domain entity").expect_err("should reject");
        assert_eq!(
            err,
            IdError::InvalidCharacter {
                kind: "rule",
                id: "domain entity".to_owned(),
                character: ' ',
            }
        );
    }

    #[test]
    fn shell_and_glob_metacharacters_are_rejected() {
        for bad in ["rule*", "rule?", "rule$x", "rule;drop", "rule\"q", "rule'q"] {
            assert!(RuleId::new(bad).is_err(), "{bad} should be rejected");
        }
    }

    /// The error names the offending character, so the fix is obvious without
    /// consulting the schema.
    #[test]
    fn the_error_names_the_first_offending_character() {
        let err = RuleId::new("ok-then*bad?").expect_err("should reject");
        let IdError::InvalidCharacter { character, .. } = err else {
            panic!("expected InvalidCharacter, got {err:?}");
        };
        assert_eq!(character, '*');
    }

    /// The two kinds report themselves distinctly, so a config error says
    /// which list the bad id came from.
    #[test]
    fn the_error_distinguishes_rules_from_modules() {
        let rule = RuleId::new("bad id").expect_err("rejects");
        let module = ModuleId::new("bad id").expect_err("rejects");
        assert!(rule.to_string().starts_with("rule id"), "{rule}");
        assert!(module.to_string().starts_with("module id"), "{module}");
    }

    /// Ids are `transparent` on the wire: a config writes a plain string, not
    /// a wrapper object.
    #[test]
    fn ids_are_plain_strings_on_the_wire() {
        let id = RuleId::new("a-rule").expect("valid");
        assert_eq!(
            serde_json::to_string(&id).expect("serialises"),
            "\"a-rule\""
        );

        let parsed: RuleId = serde_json::from_str("\"a-rule\"").expect("deserialises");
        assert_eq!(parsed, id);
    }

    /// Validation runs on the way in from JSON too, so a bad id in a config
    /// fails at load time rather than surfacing much later.
    #[test]
    fn deserialising_an_invalid_id_fails() {
        let err = serde_json::from_str::<RuleId>("\"bad id\"").expect_err("should fail");
        assert!(err.to_string().contains("rule id"), "{err}");
    }
}
