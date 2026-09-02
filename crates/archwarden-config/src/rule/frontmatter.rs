//! A document's `---` block, and the keys in it.

use archwarden_core::{
    ids::{DecisionId, RuleId},
    level::Level,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::one_or_many::OneOrMany;

use super::Patterns;

/// A document's frontmatter must carry these keys.
///
/// The first rule that reads a file that is not code. The frontmatter of a
/// `.md` is often not documentation at all — it is the machine-readable half
/// of the document, and a missing or misspelled key fails *silently*: the
/// project with no `componentes` reports as needing none, and the lesson whose
/// `status` is outside the vocabulary drops out of the generated table with no
/// row and no error. Nothing type-checks a markdown file. Issue #44.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrontmatterRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words. The same field, with the same
    /// meaning, is on every rule kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements, when it implements a declared one.
    ///
    /// A plain foreign key into [`Config::decisions`](crate::config::Config::decisions),
    /// written here rather than as a list of rule ids on the decision: this is
    /// where the author already is, there is no second list to keep in step, a
    /// deleted rule leaves nothing dangling, and a new rule that forgets its
    /// decision is visible in the one place it exists. Naming a decision the
    /// config does not declare is refused at compile — a reference to nothing
    /// is a typo, not a style. Issue #100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Why this rule's scope is empty on purpose, when it is. The same field,
    /// with the same meaning, is on every rule kind — see `StructureRule`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_yet: Option<String>,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Regex over the filename of the documents this rule is about.
    pub file_pattern: String,
    /// Keys the block must carry.
    ///
    /// Ninety per cent of the value, and the whole of it that is about *names*.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub require: Patterns,
    /// The closed vocabulary a key's value must come from.
    ///
    /// The case that justifies the rule existing. A missing key is at least an
    /// absence; a value outside the vocabulary is *confidently wrong* — the
    /// generated table simply has no row for it — which is the same failure
    /// shape `must_export.annotation` exists for.
    ///
    /// Values are compared as text, so `"1"` here matches `nivel: 1` in the
    /// document. That is deliberate: it answers the question without archwarden
    /// growing a type system nothing else here needs.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub one_of: std::collections::BTreeMap<String, Patterns>,
    /// A key whose value must equal a template rendered from the path.
    ///
    /// `{{raw(dirname)}}` is the name of the directory the document sits in,
    /// and it is the only group a document template may name. The form is the
    /// one `naming` already uses, so the transforms come along:
    /// `{{kebab(dirname)}}` is spelled the same way here as there.
    ///
    /// This is the `naming` rule's question — a name agreeing with a path —
    /// asked of a file that has no exported symbol to ask it about.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub equals: std::collections::BTreeMap<String, String>,
    /// Narrow this rule to the files that import something.
    ///
    /// Path globs, matched against where an import *lands* — the same way an
    /// `import-boundary` matches. Without this a rule's population is where a
    /// file sits and what it is called; with it, what the file talks to.
    ///
    /// Leave it out and nothing changes, including the cost: a rule that does
    /// not ask never resolves an import. Issue #98, decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    ///
    /// Matched against the package a specifier belongs to, so `zod` covers
    /// `zod/v4` as it does everywhere else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}
