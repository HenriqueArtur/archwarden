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
///
/// Like [`crate::finding::Finding`], and for the same reason, this is not
/// `#[non_exhaustive]`: a parser front-end in another crate has to build one,
/// and the attribute would make that impossible. The attribute stays on the
/// enums, which downstream code only reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The local binding this export is nothing but a forward of.
    ///
    /// Set when the file adds no behaviour between an import and this export:
    /// `export { X }` naming an imported `X`, `export const A = B`, or a
    /// one-line function whose whole body is `return g(...)` with its own
    /// parameters in order. `None` for anything that computes something.
    ///
    /// Whether the name is *imported* is not decided here — that needs the
    /// file's imports, which the `no-passthrough` rule has and a single
    /// export fact does not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forwards: Option<String>,
    /// The types this declaration writes down about itself, as written.
    ///
    /// One entry for a binding's annotation — `export const X: Foo = {}` gives
    /// `["Foo"]` — and one per clause for a class, since
    /// `export class X implements A, B` claims two contracts and satisfying
    /// either is satisfying one of them. Empty when the declaration annotates
    /// nothing, and empty for a function, which declares a *return* type — a
    /// different claim, carried in [`returns`](Self::returns).
    ///
    /// Text, not a type. Whitespace is collapsed to single spaces so a finding
    /// can print it back, and nothing else is done — no resolution, no
    /// inference, no assignability. What this supports is a rule asking whether
    /// a declaration *submits itself* to `tsc`'s judgement at all, which is the
    /// guarantee a discovery-based registry loses when the typed static
    /// registry it replaced goes away. Whether the annotated value really is
    /// of that type stays `tsc`'s question.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<String>,
    /// The return type this declaration writes down, as written.
    ///
    /// A field of its own rather than another entry in
    /// [`annotations`](Self::annotations), because the two are different
    /// claims: an annotation says *what this value is*, a return type says
    /// *what this call gives you*. A rule asking for one must not be satisfied
    /// by the other — `export const X: ResponsePattern<…> = () => {}` writes
    /// the pattern down about the wrong thing, and a single list could not tell
    /// the two apart. A declaration carrying both fills both.
    ///
    /// `None` for anything that is not callable, and — the case the rule exists
    /// for — for a callable that declared nothing. `tsc` checks what is
    /// annotated and cannot require that you annotate at all, so the absence is
    /// exactly what archwarden is placed to see. Issue #101.
    ///
    /// Text, on the same terms as `annotations`: collapsed whitespace, no
    /// resolution, no inference. An alias is a different string for the same
    /// type, and a rule that cares lists the aliases it accepts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<String>,
    /// Where it appears in the source.
    pub span: Span,
}

/// An import a file declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct CallFact {
    /// The callee as written at the call site, e.g. `Event.save`. Method
    /// chains are recorded verbatim and matched exactly.
    pub callee: String,
    /// Where it appears in the source.
    pub span: Span,
}

/// An `archwarden-allow` marker, as written in a comment.
///
/// The reason is not optional and there is no variant without one: a
/// suppression that hides itself is worse than the violation it hides, which
/// is the whole argument of issue #72. A comment with no reason after the
/// colon is not a marker and is not carried here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowanceFact {
    /// The rule it names, or `None` for every rule.
    pub rule_id: Option<String>,
    /// Why, in the author's words. Never empty.
    pub reason: String,
    /// The byte range of the line this marker governs — the one after it.
    ///
    /// Worked out by the front-end, where the source text is in hand, rather
    /// than carried as the comment's own position for the runner to resolve
    /// later. By then the text is gone: facts are cached and the file is not
    /// read again, so a marker that stored where *it* was could never find out
    /// what was under it.
    pub governs: Span,
}

impl AllowanceFact {
    /// The marker a comment's text spells, if it spells one.
    ///
    /// Two shapes, and the rule id is the only optional part:
    ///
    /// ```text
    /// // archwarden-allow: the vendor SDK has no types
    /// // archwarden-allow ui-forbids-domain: one screen, being deleted in Q3
    /// ```
    ///
    /// **No reason, no suppression.** A marker with nothing after the colon,
    /// or with only whitespace, is not a marker — it is a comment, and it
    /// suppresses nothing. That is the constraint the feature *is*: an
    /// unexplained suppression is how debt becomes invisible, and refusing to
    /// recognise one is cheaper than reporting it and hoping somebody looks.
    #[must_use]
    pub fn parse(text: &str, governs: Span) -> Option<Self> {
        let rest = text.trim_start().strip_prefix(MARKER)?;
        let (head, reason) = rest.split_once(':')?;

        let reason = reason.trim();
        if reason.is_empty() {
            return None;
        }

        let named = head.trim();
        // Anything between the marker and the colon is a rule id. Empty means
        // every rule; whitespace inside means the comment was prose that
        // happened to start with the word, and is not a marker.
        if named.chars().any(char::is_whitespace) {
            return None;
        }

        Some(Self {
            rule_id: (!named.is_empty()).then(|| named.to_owned()),
            reason: reason.to_owned(),
            governs,
        })
    }

    /// Whether this marker speaks for `rule` at `offset`.
    ///
    /// A marker with no rule id speaks for every rule on the line it governs.
    /// One that names a rule speaks for that rule alone, which is what lets a
    /// file carry an exception to one boundary without going quiet about the
    /// rest.
    #[must_use]
    pub fn covers(&self, offset: u32, rule: &str) -> bool {
        if self.governs.start > offset || offset >= self.governs.end {
            return false;
        }
        self.rule_id.as_deref().is_none_or(|named| named == rule)
    }
}

/// The word a suppression comment starts with.
pub const MARKER: &str = "archwarden-allow";

/// Everything archwarden knows about one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Suppression markers found in comments, in source order.
    ///
    /// Only the ones that parse as a marker; ordinary prose is not carried,
    /// because this is cached per file and a repository's comments are larger
    /// than its code. Issue #72.
    #[serde(default)]
    pub allowances: Vec<AllowanceFact>,
    /// Whether the file has a dynamic import naming no single module.
    ///
    /// `import(name)` and ``import(`./locales/${name}`)`` are recorded nowhere
    /// in `imports`, because inventing a path for them would have a boundary
    /// rule report one nobody wrote. That silence is right for a rule and
    /// wrong for anything asking "who imports this file" — the honest answer
    /// there is "these ones, and I cannot see inside that one".
    #[serde(default)]
    pub has_opaque_import: bool,
}

impl FileFacts {
    /// Moves every span forward by `offset` bytes.
    ///
    /// For a front-end that parses a *slice* of a file. An `.astro` module
    /// lives inside a `---` fence, so oxc's offsets are relative to the fence
    /// and every finding would point at the wrong line — which is worse than
    /// not reporting a position at all, because a wrong `path:line:column` is
    /// one a reader opens. Issue #13.
    ///
    /// Saturating: an offset that would overflow leaves the span where it is,
    /// which is wrong by a known amount rather than wrong by wrapping to the
    /// top of the file.
    pub fn shift_spans(&mut self, offset: u32) {
        let shift = |span: &mut Span| {
            span.start = span.start.saturating_add(offset);
            span.end = span.end.saturating_add(offset);
        };

        for import in &mut self.imports {
            shift(&mut import.span);
        }
        for export in &mut self.exports {
            shift(&mut export.span);
        }
        for call in &mut self.calls {
            shift(&mut call.span);
        }
    }

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
            allowances: Vec::new(),
            has_opaque_import: false,
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

    /// Issue #13. A front-end that parses a slice reports offsets into the
    /// slice, and a wrong `path:line:column` is worse than none: it is one a
    /// reader opens.
    #[test]
    fn shifting_moves_every_kind_of_span() {
        let mut facts = FileFacts::unparsed(path(), ContentHash::of(b""));
        facts.imports.push(ImportFact {
            specifier: "./x".to_owned(),
            resolved: None,
            type_only: false,
            names: Vec::new(),
            span: Span::new(0, 10),
        });
        facts.exports.push(ExportFact {
            span: Span::new(20, 30),
            ..export("X", ExportTags::only(ExportKind::Const))
        });
        facts.calls.push(CallFact {
            callee: "f".to_owned(),
            span: Span::new(40, 50),
        });

        facts.shift_spans(4);

        assert_eq!(facts.imports[0].span, Span::new(4, 14));
        assert_eq!(facts.exports[0].span, Span::new(24, 34));
        assert_eq!(facts.calls[0].span, Span::new(44, 54));
    }

    /// Saturating rather than wrapping: wrong by a known amount beats a span
    /// that jumps to the top of the file.
    #[test]
    fn an_offset_that_would_overflow_leaves_the_span_alone() {
        let mut facts = FileFacts::unparsed(path(), ContentHash::of(b""));
        facts.calls.push(CallFact {
            callee: "f".to_owned(),
            span: Span::new(u32::MAX - 1, u32::MAX),
        });

        facts.shift_spans(10);

        assert_eq!(facts.calls[0].span, Span::new(u32::MAX, u32::MAX));
    }

    fn export(name: &str, tags: ExportTags) -> ExportFact {
        ExportFact {
            name: Some(name.to_owned()),
            tags,
            is_default: false,
            reexport_from: None,
            forwards: None,
            annotations: Vec::new(),
            returns: None,
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

    /// The constraint the feature is: no reason, no suppression.
    ///
    /// `// eslint-disable-next-line` with no explanation is how debt becomes
    /// invisible, and a suppression that hides itself is worse than the
    /// violation it hides. Refusing to *recognise* an unexplained marker is
    /// cheaper than recognising it and hoping somebody reads a report.
    #[test]
    fn a_marker_without_a_reason_is_not_a_marker() {
        let span = Span::new(0, 10);

        assert!(AllowanceFact::parse("archwarden-allow:", span).is_none());
        assert!(AllowanceFact::parse("archwarden-allow:    ", span).is_none());
        assert!(
            AllowanceFact::parse("archwarden-allow", span).is_none(),
            "and no colon at all is a word somebody wrote, not a suppression"
        );
    }

    /// A marker speaks for the line it governs and for nobody else's rule.
    ///
    /// The two halves of what makes a suppression narrow: a byte outside the
    /// governed line is not covered, however close, and a marker naming one
    /// rule leaves every other rule reporting. Without the second, a file with
    /// one legitimate exception would go quiet about everything.
    #[test]
    fn a_marker_covers_its_own_line_and_its_own_rule() {
        let marker = AllowanceFact {
            rule_id: None,
            reason: "because".to_owned(),
            governs: Span::new(10, 20),
        };

        assert!(marker.covers(10, "any-rule"), "the first byte is inside");
        assert!(marker.covers(19, "any-rule"));
        assert!(!marker.covers(9, "any-rule"), "the byte before is not");
        assert!(
            !marker.covers(20, "any-rule"),
            "and neither is the one after the end"
        );

        let named = AllowanceFact {
            rule_id: Some("ui-forbids-domain".to_owned()),
            reason: "because".to_owned(),
            governs: Span::new(10, 20),
        };
        assert!(named.covers(12, "ui-forbids-domain"));
        assert!(
            !named.covers(12, "another-rule"),
            "one exception must not silence the rest of the file"
        );
    }

    /// A file with no markers carries none, which is what keeps the cache from
    /// growing for every repository that never uses the feature.
    #[test]
    fn a_file_without_markers_carries_none() {
        let facts = FileFacts::unparsed(
            RepoRelPath::new("src/a.ts").expect("valid"),
            ContentHash::of(b"source"),
        );

        assert!(facts.allowances.is_empty());
    }

    /// The two shapes, and the rule id is the only optional part.
    #[test]
    fn a_marker_may_name_one_rule_or_all_of_them() {
        let span = Span::new(0, 10);

        assert_eq!(
            AllowanceFact::parse("archwarden-allow: the vendor SDK has no types", span),
            Some(AllowanceFact {
                rule_id: None,
                reason: "the vendor SDK has no types".to_owned(),
                governs: span,
            })
        );
        assert_eq!(
            AllowanceFact::parse("archwarden-allow ui-forbids-domain: one screen", span),
            Some(AllowanceFact {
                rule_id: Some("ui-forbids-domain".to_owned()),
                reason: "one screen".to_owned(),
                governs: span,
            })
        );
    }

    /// Leading whitespace is the comment's, not the author's.
    #[test]
    fn the_comment_marker_may_be_indented() {
        let span = Span::new(0, 10);

        assert!(AllowanceFact::parse("   archwarden-allow: because", span).is_some());
    }

    /// Prose that happens to begin with the word is prose.
    ///
    /// "archwarden-allow rules are documented in ADR 7: see there" reads as a
    /// marker to a naive parser and is a sentence. Two words before the colon
    /// is what tells them apart, because a rule id has no spaces in it.
    #[test]
    fn a_sentence_that_starts_with_the_word_is_not_a_marker() {
        let span = Span::new(0, 10);

        assert!(
            AllowanceFact::parse(
                "archwarden-allow rules are documented in ADR 7: see there",
                span
            )
            .is_none()
        );
    }
}
