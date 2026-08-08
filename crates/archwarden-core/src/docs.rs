//! What a front-end extracts from one *document*.
//!
//! The second kind of facts, and the reason there is a plural. [`FileFacts`]
//! is imports, exports and calls — the questions a JS/TS front-end answers. A
//! markdown file has none of those, and forcing its frontmatter into that
//! struct would make it mean two unrelated things and hand every rule engine a
//! field it never reads.
//!
//! Named for *documents* rather than for frontmatter on purpose. A rule that
//! asks about a document's sections is the same shape of question — a document
//! is a tree of named sections the way a directory is a tree of named files —
//! and it should cost a rule and nothing else. So [`DocFacts::headings`] is
//! here, empty, waiting for the rule that consumes it.
//!
//! [`FileFacts`]: crate::facts::FileFacts

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{facts::Span, hash::ContentHash, path::RepoRelPath};

/// Everything archwarden knows about one document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocFacts {
    /// The file, relative to the repository root.
    pub path: RepoRelPath,
    /// Hash of the file's bytes, which the cache is keyed by.
    pub content_hash: ContentHash,
    /// The frontmatter block, in one of its three states.
    pub frontmatter: Frontmatter,
    /// Headings, in document order.
    ///
    /// Always empty today. The field exists so that the rule which wants it is
    /// a rule and nothing else — no second facts type, no second front-end, no
    /// second place in the cache.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headings: Vec<Heading>,
}

/// A document's frontmatter block.
///
/// Three states, not two, because the three have three different fixes.
/// Absent means write the block; malformed means the block you wrote is not
/// YAML; present means ask it questions. Collapsing the first two would leave a
/// reader with "something is wrong with your frontmatter" and no next step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Frontmatter {
    /// There is no `---`-fenced block at the top of the file.
    Absent,
    /// There is one, and it is not a YAML mapping.
    Malformed {
        /// What the parser objected to.
        reason: String,
    },
    /// The top-level keys, in name order.
    Present(BTreeMap<String, DocValue>),
}

/// What a rule may ask about one frontmatter value.
///
/// Deliberately not a YAML value. archwarden asserts names and vocabularies,
/// never the shape of a value — so a list is *present* and its contents are
/// nobody's business here. The line is stated in `docs/RULES.md`, and it is the
/// same one `must_export` keeps about a type annotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum DocValue {
    /// A scalar, kept as the text it renders to.
    ///
    /// Text and not a type: `one_of: [1, 2, 3]` in a config and `nivel: 1` in a
    /// document are the same question in two notations, and rendering both to
    /// `"1"` answers it without a type system archwarden has no other use for.
    Scalar(String),
    /// A list. Present; its contents are not modelled.
    List,
    /// A nested mapping. Present; its contents are not modelled.
    Map,
    /// A key written with nothing after it.
    Empty,
}

/// A heading in a document.
///
/// Nothing produces one yet. See [`DocFacts::headings`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    /// How many `#` it carries: 1 through 6.
    pub level: u8,
    /// The text after the marker, trimmed.
    pub text: String,
    /// Where it appears in the source.
    pub span: Span,
}

impl DocFacts {
    /// The keys the block carries, or none when there is no usable block.
    ///
    /// A rule asking "is `id` present" gets `false` for a document with no
    /// block *and* for one whose block is not YAML — but it should not report
    /// the same thing about them, which is why the state is still reachable
    /// through [`DocFacts::frontmatter`].
    #[must_use]
    pub fn keys(&self) -> Option<&BTreeMap<String, DocValue>> {
        match &self.frontmatter {
            Frontmatter::Present(keys) => Some(keys),
            _ => None,
        }
    }
}
