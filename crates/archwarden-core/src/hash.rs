//! Content hashing, the basis of both cache keys.
//!
//! `facts` is keyed by content hash alone; `findings` is keyed by content hash
//! combined with a rules hash and a resolution epoch. Keeping the combination
//! in one place means the two keys can never drift apart in how they are
//! built. See `docs/ARCHITECTURE.md`.

use serde::{Deserialize, Serialize};

/// The number of bytes in a hash. blake3 produces 32.
const HASH_LEN: usize = 32;

/// A blake3 hash of some content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ContentHash([u8; HASH_LEN]);

/// Why a hash could not be parsed from text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HashError {
    /// The text was not 64 hexadecimal characters.
    #[error("`{text}` is not a {expected}-character hex hash")]
    BadLength {
        /// The text as given.
        text: String,
        /// How many characters were expected.
        expected: usize,
    },
    /// The text contained a character that is not a hex digit.
    #[error("`{text}` contains non-hexadecimal character `{character}`")]
    NotHex {
        /// The text as given.
        text: String,
        /// The first offending character.
        character: char,
    },
}

impl ContentHash {
    /// Hashes a byte slice.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }

    /// Folds several hashes into one, for the composite `findings` key.
    ///
    /// Order matters: this is a hash *of the sequence*, not of an unordered
    /// set. Two different orderings must not collide, or two different rule
    /// configurations could share a cache entry.
    #[must_use]
    pub fn combine(parts: &[Self]) -> Self {
        let mut hasher = blake3::Hasher::new();
        for part in parts {
            hasher.update(&part.0);
        }
        Self(*hasher.finalize().as_bytes())
    }

    /// The hash as lowercase hexadecimal.
    #[must_use]
    pub fn to_hex(&self) -> String {
        use std::fmt::Write as _;

        self.0
            .iter()
            .fold(String::with_capacity(HASH_LEN * 2), |mut acc, byte| {
                // Writing into a String is infallible, so the result is
                // discarded rather than unwrapped: this crate denies unwrap.
                let _ = write!(acc, "{byte:02x}");
                acc
            })
    }

    /// Parses a hash from lowercase hexadecimal.
    ///
    /// # Errors
    /// See [`HashError`].
    pub fn parse_hex(text: &str) -> Result<Self, HashError> {
        if text.len() != HASH_LEN * 2 {
            return Err(HashError::BadLength {
                text: text.to_owned(),
                expected: HASH_LEN * 2,
            });
        }

        if let Some(character) = text.chars().find(|c| !c.is_ascii_hexdigit()) {
            return Err(HashError::NotHex {
                text: text.to_owned(),
                character,
            });
        }

        let mut bytes = [0u8; HASH_LEN];
        for (slot, pair) in bytes.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
            let pair = std::str::from_utf8(pair).map_err(|_| HashError::NotHex {
                text: text.to_owned(),
                character: char::REPLACEMENT_CHARACTER,
            })?;
            *slot = u8::from_str_radix(pair, 16).map_err(|_| HashError::NotHex {
                text: text.to_owned(),
                character: char::REPLACEMENT_CHARACTER,
            })?;
        }

        Ok(Self(bytes))
    }

    /// The raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; HASH_LEN] {
        &self.0
    }
}

impl std::fmt::Display for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl TryFrom<String> for ContentHash {
    type Error = HashError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse_hex(&value)
    }
}

impl From<ContentHash> for String {
    fn from(value: ContentHash) -> Self {
        value.to_hex()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_content_hashes_identically() {
        assert_eq!(
            ContentHash::of(b"export function Foo() {}"),
            ContentHash::of(b"export function Foo() {}")
        );
    }

    /// The entire cache rests on this: if a one-character edit did not change
    /// the hash, a stale entry would be served.
    #[test]
    fn a_single_byte_change_changes_the_hash() {
        assert_ne!(
            ContentHash::of(b"export function Foo() {}"),
            ContentHash::of(b"export function Fooo() {}")
        );
        assert_ne!(ContentHash::of(b""), ContentHash::of(b" "));
    }

    #[test]
    fn hex_is_sixty_four_lowercase_characters() {
        let hex = ContentHash::of(b"anything").to_hex();
        assert_eq!(hex.len(), 64);
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "{hex}"
        );
    }

    #[test]
    fn hex_round_trips() {
        let hash = ContentHash::of(b"some file content");
        let parsed = ContentHash::parse_hex(&hash.to_hex()).expect("round trips");
        assert_eq!(parsed, hash);
        assert_eq!(hash.to_string(), hash.to_hex());
    }

    #[test]
    fn parsing_rejects_wrong_length_and_non_hex() {
        let err = ContentHash::parse_hex("abc").expect_err("too short");
        let HashError::BadLength { expected, text } = err else {
            panic!("expected BadLength, got {err:?}");
        };
        // The message tells the user the width to aim for, so the number has
        // to be the real one rather than merely present.
        assert_eq!(expected, 64);
        assert_eq!(text, "abc");

        let long = "z".repeat(64);
        assert!(matches!(
            ContentHash::parse_hex(&long),
            Err(HashError::NotHex { character: 'z', .. })
        ));
    }

    /// The `findings` key is built by combining content, rules and resolution
    /// hashes. If the fold ignored order, swapping two components would
    /// produce the same key and serve the wrong cached findings.
    #[test]
    fn combining_is_sensitive_to_order() {
        let a = ContentHash::of(b"a");
        let b = ContentHash::of(b"b");
        assert_ne!(ContentHash::combine(&[a, b]), ContentHash::combine(&[b, a]));
    }

    #[test]
    fn combining_is_deterministic_and_depends_on_every_part() {
        let a = ContentHash::of(b"content");
        let b = ContentHash::of(b"rules");
        let c = ContentHash::of(b"epoch");

        assert_eq!(
            ContentHash::combine(&[a, b, c]),
            ContentHash::combine(&[a, b, c])
        );

        // Changing any one component must change the result, or a config edit
        // could go unnoticed by the cache.
        let other = ContentHash::of(b"other rules");
        assert_ne!(
            ContentHash::combine(&[a, b, c]),
            ContentHash::combine(&[a, other, c])
        );
        assert_ne!(
            ContentHash::combine(&[a, b, c]),
            ContentHash::combine(&[a, b])
        );
    }

    #[test]
    fn hashes_are_hex_strings_on_the_wire() {
        let hash = ContentHash::of(b"content");
        let json = serde_json::to_string(&hash).expect("serialises");
        assert_eq!(json, format!("\"{}\"", hash.to_hex()));

        let parsed: ContentHash = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(parsed, hash);
        assert!(serde_json::from_str::<ContentHash>("\"nope\"").is_err());
    }

    /// Pins both the width and the algorithm. Asserting only "32 bytes, not
    /// all zero" would accept any digest at all, including a constant.
    #[test]
    fn as_bytes_exposes_the_real_blake3_digest() {
        let hash = ContentHash::of(b"content");
        assert_eq!(hash.as_bytes().len(), 32);
        assert_eq!(hash.as_bytes(), blake3::hash(b"content").as_bytes());
        assert_ne!(hash.as_bytes(), blake3::hash(b"other").as_bytes());
    }
}
