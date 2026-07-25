//! A field that accepts either one value or a list of them.
//!
//! Every glob field in a config takes this shape. `"roots": "src/**"` and
//! `"roots": ["src/**"]` mean the same thing, because a config is written by
//! hand and by agents, and forcing a one-element array on the common case is
//! friction with no payoff. See `docs/CONFIG.md`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One `T`, or several.
///
/// Serialises back to whichever shape it was written in, so a config that
/// round-trips through archwarden does not gain array brackets it never had.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    /// A single value, written bare.
    One(T),
    /// Several values, written as an array.
    Many(Vec<T>),
}

impl<T> OneOrMany<T> {
    /// Borrows the values as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        match self {
            // `from_ref` views the single value as a one-element slice, so
            // both variants expose the same shape without allocating.
            Self::One(value) => std::slice::from_ref(value),
            Self::Many(values) => values,
        }
    }

    /// Consumes this into a `Vec`.
    #[must_use]
    pub fn into_vec(self) -> Vec<T> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }

    /// How many values there are.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::One(_) => 1,
            Self::Many(values) => values.len(),
        }
    }

    /// Whether there are no values. Only an explicit empty array is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterates the values.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.as_slice().iter()
    }
}

impl<T> Default for OneOrMany<T> {
    fn default() -> Self {
        Self::Many(Vec::new())
    }
}

impl<T> From<Vec<T>> for OneOrMany<T> {
    fn from(values: Vec<T>) -> Self {
        Self::Many(values)
    }
}

impl<'a, T> IntoIterator for &'a OneOrMany<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Globs = OneOrMany<String>;

    fn parse(json: &str) -> Globs {
        serde_json::from_str(json).expect("should deserialise")
    }

    /// The motivating case: a single glob written bare, without brackets.
    #[test]
    fn a_bare_string_is_accepted_as_one_value() {
        let globs = parse(r#""src/**""#);
        assert_eq!(globs.len(), 1);
        assert_eq!(globs.iter().collect::<Vec<_>>(), ["src/**"]);
    }

    #[test]
    fn an_array_is_accepted_as_many_values() {
        let globs = parse(r#"["src/**", "apps/*"]"#);
        assert_eq!(globs.len(), 2);
        assert_eq!(globs.iter().collect::<Vec<_>>(), ["src/**", "apps/*"]);
    }

    /// The two spellings of a single value must be indistinguishable once
    /// parsed, or a rule would behave differently depending on how its author
    /// happened to write it.
    #[test]
    fn both_spellings_of_one_value_yield_the_same_values() {
        let bare = parse(r#""src/**""#);
        let wrapped = parse(r#"["src/**"]"#);
        assert_eq!(bare.into_vec(), wrapped.into_vec());
    }

    /// An explicit empty array is the only way to mean "no values". A bare
    /// string always means one.
    #[test]
    fn only_an_explicit_empty_array_is_empty() {
        let empty = parse("[]");
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        assert!(!parse(r#""""#).is_empty());
        assert_eq!(parse(r#""""#).len(), 1);
    }

    #[test]
    fn the_default_is_no_values() {
        assert!(Globs::default().is_empty());
        assert_eq!(Globs::default().into_vec(), Vec::<String>::new());
    }

    /// Round-tripping preserves the spelling, so archwarden never rewrites a
    /// bare string into a one-element array in a config it read.
    #[test]
    fn serialising_preserves_the_original_spelling() {
        assert_eq!(
            serde_json::to_string(&parse(r#""src/**""#)).expect("serialises"),
            r#""src/**""#
        );
        assert_eq!(
            serde_json::to_string(&parse(r#"["src/**"]"#)).expect("serialises"),
            r#"["src/**"]"#
        );
    }

    #[test]
    fn a_vec_converts_in_directly() {
        let globs: Globs = vec!["a".to_owned(), "b".to_owned()].into();
        assert_eq!(globs.len(), 2);
    }

    #[test]
    fn borrowing_iterates_without_consuming() {
        let globs = parse(r#"["a", "b"]"#);
        let collected: Vec<_> = (&globs).into_iter().map(String::as_str).collect();
        assert_eq!(collected, ["a", "b"]);
        assert_eq!(globs.len(), 2, "still usable after iterating");
    }

    /// A number where a glob belongs is a config bug, not something to coerce.
    #[test]
    fn a_wrong_inner_type_is_rejected() {
        assert!(serde_json::from_str::<Globs>("42").is_err());
        assert!(serde_json::from_str::<Globs>("[1, 2]").is_err());
    }
}
