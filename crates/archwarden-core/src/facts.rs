//! What a parser extracts from one file.
//!
//! This is the seam that keeps rule engines independent of the front-end.
//! Rules never see an AST; they see [`FileFacts`]. Supporting another language
//! later means producing these same facts from a different parser, with no
//! change to any rule. See decision 6.

use serde::{Deserialize, Serialize};

use crate::{hash::ContentHash, path::RepoRelPath};

/// A byte range in the source file.
///
/// Byte offsets rather than line/column because that is what a parser hands
/// back cheaply; the reporter converts to line/column once, when rendering.
/// Carrying spans from the start avoids threading them through every fact type
/// later, which is the same reasoning that put serialisation on these types
/// before the cache exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Span {
    /// Byte offset of the first character.
    pub start: u32,
    /// Byte offset one past the last character.
    pub end: u32,
}

impl Span {
    /// Builds a span.
    #[must_use]
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }
}

/// How an exported symbol was declared.
///
/// An export carries a *set* of these, not one: `export const Foo = () => {}`
/// is both `Const` and `Arrow`. Deliberately, `Arrow` is not `Function`, so a
/// rule can require one form and reject the other. See `docs/RULES.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ExportKind {
    /// `export function Foo() {}`, including `async` and generators.
    Function,
    /// `export const Foo = () => {}`.
    Arrow,
    /// `export const Foo = ...`.
    Const,
    /// `export let Foo`.
    Let,
    /// `export var Foo`.
    Var,
    /// `export class Foo {}`.
    Class,
    /// `export type Foo = ...`.
    Type,
    /// `export interface Foo {}`.
    Interface,
    /// `export enum Foo {}`.
    Enum,
    /// `export { Foo } from './x'`, whose real kind needs cross-file analysis.
    Reexport,
}

impl ExportKind {
    /// Every kind, for building error messages and for exhaustive tests.
    pub const ALL: [Self; 10] = [
        Self::Function,
        Self::Arrow,
        Self::Const,
        Self::Let,
        Self::Var,
        Self::Class,
        Self::Type,
        Self::Interface,
        Self::Enum,
        Self::Reexport,
    ];

    /// The spelling used in a config's `must_export.kind`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Arrow => "arrow",
            Self::Const => "const",
            Self::Let => "let",
            Self::Var => "var",
            Self::Class => "class",
            Self::Type => "type",
            Self::Interface => "interface",
            Self::Enum => "enum",
            Self::Reexport => "reexport",
        }
    }

    /// Parses a kind by its config spelling.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }

    /// This kind's bit in an [`ExportTags`] set.
    fn bit(self) -> u16 {
        match self {
            Self::Function => 0x001,
            Self::Arrow => 0x002,
            Self::Const => 0x004,
            Self::Let => 0x008,
            Self::Var => 0x010,
            Self::Class => 0x020,
            Self::Type => 0x040,
            Self::Interface => 0x080,
            Self::Enum => 0x100,
            Self::Reexport => 0x200,
        }
    }
}

impl std::fmt::Display for ExportKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

/// The set of kinds one export carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "Vec<ExportKind>", into = "Vec<ExportKind>")]
pub struct ExportTags(u16);

impl ExportTags {
    /// The empty set.
    #[must_use]
    pub fn none() -> Self {
        Self(0)
    }

    /// A set holding exactly one kind.
    #[must_use]
    pub fn only(kind: ExportKind) -> Self {
        Self(kind.bit())
    }

    /// Adds a kind, returning the new set.
    #[must_use]
    pub fn with(self, kind: ExportKind) -> Self {
        Self(self.0 | kind.bit())
    }

    /// Whether this set holds `kind`.
    #[must_use]
    pub fn contains(self, kind: ExportKind) -> bool {
        self.0 & kind.bit() != 0
    }

    /// Whether the two sets share at least one kind.
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The kinds in the set, in declaration order.
    pub fn iter(self) -> impl Iterator<Item = ExportKind> {
        ExportKind::ALL
            .into_iter()
            .filter(move |k| self.contains(*k))
    }
}

impl FromIterator<ExportKind> for ExportTags {
    fn from_iter<I: IntoIterator<Item = ExportKind>>(iter: I) -> Self {
        iter.into_iter().fold(Self::none(), Self::with)
    }
}

impl From<Vec<ExportKind>> for ExportTags {
    fn from(kinds: Vec<ExportKind>) -> Self {
        kinds.into_iter().collect()
    }
}

impl From<ExportTags> for Vec<ExportKind> {
    fn from(tags: ExportTags) -> Self {
        tags.iter().collect()
    }
}

/// What a config's `must_export.kind` asks for.
///
/// `any` is a query, never a tag: no export is ever *declared* as `any`, so it
/// cannot live in [`ExportKind`] without making that enum mean two things.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum KindFilter {
    /// Matches any export, whatever its declaration form.
    Any,
    /// Matches an export carrying at least one of these kinds.
    OneOf(ExportTags),
}

impl KindFilter {
    /// Whether an export with `tags` satisfies this filter.
    #[must_use]
    pub fn accepts(&self, tags: ExportTags) -> bool {
        match self {
            Self::Any => true,
            Self::OneOf(wanted) => tags.intersects(*wanted),
        }
    }
}

/// A symbol a file exports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ExportFact {
    /// The exported name. `None` for an anonymous default export.
    pub name: Option<String>,
    /// How it was declared.
    pub tags: ExportTags,
    /// Whether this is the default export. A default never satisfies a named
    /// `must_export`, because its name does not bind the importer.
    pub is_default: bool,
    /// For `export { Foo } from './x'`, the specifier it came from.
    pub reexport_from: Option<String>,
    /// Where it appears in the source.
    pub span: Span,
}

/// An import a file declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ImportFact {
    /// The specifier exactly as written, e.g. `@/domain/user`.
    pub specifier: String,
    /// Where the specifier resolved to, once the resolver has run. `None`
    /// while facts are still being extracted, or when resolution failed.
    pub resolved: Option<RepoRelPath>,
    /// Whether this is `import type` or an inline `type` mark. Boundary rules
    /// may opt out of type-only imports.
    pub type_only: bool,
    /// The names brought into scope, for matching `call-obligation` symbols.
    pub names: Vec<String>,
    /// Where it appears in the source.
    pub span: Span,
}

/// A call expression found in a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CallFact {
    /// The callee as written at the call site, e.g. `Event.save`. Method
    /// chains are recorded verbatim and matched exactly.
    pub callee: String,
    /// Where it appears in the source.
    pub span: Span,
}

/// Everything archwarden knows about one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FileFacts {
    /// The file, relative to the repository root.
    pub path: RepoRelPath,
    /// Hash of the file's bytes. The `facts` cache table is keyed by this.
    pub content_hash: ContentHash,
    /// Imports, in source order.
    pub imports: Vec<ImportFact>,
    /// Exports, in source order.
    pub exports: Vec<ExportFact>,
    /// Call expressions, in source order.
    pub calls: Vec<CallFact>,
}

impl FileFacts {
    /// Facts for a file that has been hashed but not parsed. Used for files no
    /// rule needs to look inside, which is most of them on a structure-only
    /// run.
    #[must_use]
    pub fn unparsed(path: RepoRelPath, content_hash: ContentHash) -> Self {
        Self {
            path,
            content_hash,
            imports: Vec::new(),
            exports: Vec::new(),
            calls: Vec::new(),
        }
    }

    /// Finds an export by name, ignoring default exports.
    #[must_use]
    pub fn named_export(&self, name: &str) -> Option<&ExportFact> {
        self.exports
            .iter()
            .find(|e| !e.is_default && e.name.as_deref() == Some(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> RepoRelPath {
        RepoRelPath::new("packages/domain/src/user/user.ts").expect("valid")
    }

    fn export(name: &str, tags: ExportTags) -> ExportFact {
        ExportFact {
            name: Some(name.to_owned()),
            tags,
            is_default: false,
            reexport_from: None,
            span: Span::new(0, 1),
        }
    }

    /// The table in docs/RULES.md, as code. An arrow function is not a
    /// `function`; that distinction is the whole reason tags are a set.
    #[test]
    fn an_arrow_is_not_a_function() {
        let arrow = ExportTags::only(ExportKind::Const).with(ExportKind::Arrow);

        assert!(arrow.contains(ExportKind::Const));
        assert!(arrow.contains(ExportKind::Arrow));
        assert!(!arrow.contains(ExportKind::Function));

        let declared = ExportTags::only(ExportKind::Function);
        assert!(!declared.contains(ExportKind::Arrow));
        assert!(!declared.contains(ExportKind::Const));
    }

    #[test]
    fn tags_accumulate_without_disturbing_each_other() {
        let tags = ExportTags::none()
            .with(ExportKind::Const)
            .with(ExportKind::Arrow);

        assert_eq!(
            tags.iter().collect::<Vec<_>>(),
            [ExportKind::Arrow, ExportKind::Const]
        );
        assert!(!tags.is_empty());
        assert!(ExportTags::none().is_empty());
    }

    /// `with` is a set union, not a toggle. A parser that tags the same kind
    /// twice must end up with it once, not with it removed -- which is exactly
    /// what an XOR here would do, silently and only on the second call.
    #[test]
    fn adding_a_kind_that_is_already_present_changes_nothing() {
        let once = ExportTags::only(ExportKind::Const);
        assert_eq!(once.with(ExportKind::Const), once);
        assert!(once.with(ExportKind::Const).contains(ExportKind::Const));

        let mixed = ExportTags::only(ExportKind::Const)
            .with(ExportKind::Arrow)
            .with(ExportKind::Const);
        assert!(mixed.contains(ExportKind::Const));
        assert!(mixed.contains(ExportKind::Arrow));

        let collected: ExportTags = [ExportKind::Const, ExportKind::Const, ExportKind::Arrow]
            .into_iter()
            .collect();
        assert_eq!(collected, mixed);
    }

    /// Every kind needs its own bit. A collision would silently make two kinds
    /// interchangeable, which no test of a single kind would catch.
    #[test]
    fn every_kind_occupies_a_distinct_bit() {
        for a in ExportKind::ALL {
            for b in ExportKind::ALL {
                let only_a = ExportTags::only(a);
                assert_eq!(only_a.contains(b), a == b, "{a} and {b} share a bit");
            }
        }
    }

    #[test]
    fn intersects_finds_any_shared_kind() {
        let arrow_const = ExportTags::only(ExportKind::Const).with(ExportKind::Arrow);
        let callable = ExportTags::only(ExportKind::Function).with(ExportKind::Arrow);
        let types = ExportTags::only(ExportKind::Type).with(ExportKind::Interface);

        assert!(arrow_const.intersects(callable));
        assert!(!arrow_const.intersects(types));
        assert!(!ExportTags::none().intersects(callable));
    }

    /// `kind: ["function", "arrow"]` is how a preset says "callable, either
    /// form", and it must accept both while still rejecting a plain const.
    #[test]
    fn a_filter_matching_either_callable_form_accepts_both() {
        let filter =
            KindFilter::OneOf(ExportTags::only(ExportKind::Function).with(ExportKind::Arrow));

        assert!(filter.accepts(ExportTags::only(ExportKind::Function)));
        assert!(filter.accepts(ExportTags::only(ExportKind::Const).with(ExportKind::Arrow)));
        assert!(!filter.accepts(ExportTags::only(ExportKind::Const)));
        assert!(!filter.accepts(ExportTags::only(ExportKind::Class)));
    }

    /// `any` accepts whatever was declared, including a re-export whose real
    /// kind is unknowable without cross-file analysis.
    #[test]
    fn the_any_filter_accepts_every_declaration_form() {
        for kind in ExportKind::ALL {
            assert!(KindFilter::Any.accepts(ExportTags::only(kind)), "{kind}");
        }
        assert!(KindFilter::Any.accepts(ExportTags::none()));
    }

    /// A concrete kind must reject a re-export rather than guessing, which is
    /// what makes the "kind not determinable" diagnostic possible.
    #[test]
    fn a_concrete_filter_rejects_a_reexport() {
        let filter = KindFilter::OneOf(ExportTags::only(ExportKind::Function));
        assert!(!filter.accepts(ExportTags::only(ExportKind::Reexport)));
        assert!(KindFilter::Any.accepts(ExportTags::only(ExportKind::Reexport)));
    }

    #[test]
    fn kind_names_round_trip_through_parse() {
        for kind in ExportKind::ALL {
            assert_eq!(ExportKind::parse(kind.as_str()), Some(kind));
            assert_eq!(kind.to_string(), kind.as_str());
        }
        assert_eq!(ExportKind::parse("any"), None);
        assert_eq!(ExportKind::parse("nope"), None);
    }

    /// Tags serialise as a list of names, not an opaque integer, so a cache
    /// file stays readable and a bit-order change cannot silently reinterpret
    /// old entries.
    #[test]
    fn tags_are_a_list_of_names_on_the_wire() {
        let tags = ExportTags::only(ExportKind::Const).with(ExportKind::Arrow);
        let json = serde_json::to_string(&tags).expect("serialises");
        assert_eq!(json, r#"["arrow","const"]"#);

        let parsed: ExportTags = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(parsed, tags);
    }

    #[test]
    fn unparsed_facts_carry_the_hash_and_nothing_else() {
        let hash = ContentHash::of(b"whatever");
        let facts = FileFacts::unparsed(path(), hash);

        assert_eq!(facts.content_hash, hash);
        assert!(facts.imports.is_empty());
        assert!(facts.exports.is_empty());
        assert!(facts.calls.is_empty());
    }

    /// A default export never answers to a name lookup: its local name does
    /// not bind the importer, so a `naming` rule must not be satisfied by it.
    #[test]
    fn named_export_lookup_ignores_defaults() {
        let mut facts = FileFacts::unparsed(path(), ContentHash::of(b""));
        facts.exports.push(ExportFact {
            is_default: true,
            ..export("User", ExportTags::only(ExportKind::Function))
        });

        assert_eq!(facts.named_export("User"), None);

        facts
            .exports
            .push(export("User", ExportTags::only(ExportKind::Class)));
        let found = facts.named_export("User").expect("now present");
        assert!(found.tags.contains(ExportKind::Class));
    }

    #[test]
    fn facts_round_trip_through_json() {
        let mut facts = FileFacts::unparsed(path(), ContentHash::of(b"source"));
        facts
            .exports
            .push(export("User", ExportTags::only(ExportKind::Class)));
        facts.imports.push(ImportFact {
            specifier: "@/domain/user".to_owned(),
            resolved: Some(RepoRelPath::new("packages/domain/src/user.ts").expect("valid")),
            type_only: true,
            names: vec!["User".to_owned()],
            span: Span::new(0, 30),
        });
        facts.calls.push(CallFact {
            callee: "Event.save".to_owned(),
            span: Span::new(40, 52),
        });

        let json = serde_json::to_string(&facts).expect("serialises");
        let parsed: FileFacts = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(parsed, facts);
    }
}
