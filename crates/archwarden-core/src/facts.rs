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
    /// Rust's `fn`. Deliberately not `function`: decision 31 refuses to reuse
    /// a JavaScript spelling for a Rust form, because a rule copied between
    /// the two halves of one repository would then match under the wrong
    /// language instead of being told it does not apply.
    Fn,
    /// Rust's `struct`. Not `class`, for the same reason.
    Struct,
    /// Rust's `trait`. Not `interface`, for the same reason.
    Trait,
    /// Rust's `static`.
    Static,
    /// Rust's `mod`, when it is a declaration this file exports.
    Mod,
    /// Rust's `macro_rules!` and `macro`.
    Macro,
}

impl ExportKind {
    /// Every kind, for building error messages and for exhaustive tests.
    pub const ALL: [Self; 16] = [
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
        Self::Fn,
        Self::Struct,
        Self::Trait,
        Self::Static,
        Self::Mod,
        Self::Macro,
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
            Self::Fn => "fn",
            Self::Struct => "struct",
            Self::Trait => "trait",
            Self::Static => "static",
            Self::Mod => "mod",
            Self::Macro => "macro",
        }
    }

    /// Parses a kind by its config spelling.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }

    /// Whether a file in this language can declare an export of this form.
    ///
    /// Decision 31 split the vocabulary by language deliberately, and this is
    /// the other half of that: a rule asking for a `struct` over a `.ts` file
    /// is asking for something the language cannot spell, and the file is
    /// reported for exporting a `const` instead. That reads like a naming
    /// mistake and is a configuration mistake.
    ///
    /// Three forms are shared because they *are* the same thing in both
    /// languages, which is why they kept one spelling.
    ///
    /// Exhaustive on purpose: a kind added without an answer does not compile,
    /// and a language added to [`crate::path::Language`] fails here too.
    #[allow(
        clippy::match_same_arms,
        reason = "the arms answer for different languages and happen to agree. \
                  Merging them loses the exhaustiveness that is the point: a \
                  form added without an answer, or a language added to \
                  `Language`, has to fail to compile rather than default"
    )]
    #[must_use]
    pub fn produced_by(self, language: crate::path::Language) -> bool {
        use crate::path::Language;

        match (self, language) {
            // Shared: the same declaration under the same name.
            (Self::Const | Self::Type | Self::Enum, _) => true,
            // JavaScript and TypeScript, which Astro's fence is.
            (
                Self::Function
                | Self::Arrow
                | Self::Let
                | Self::Var
                | Self::Class
                | Self::Interface
                | Self::Reexport,
                Language::Ts | Language::Astro,
            ) => true,
            (
                Self::Function
                | Self::Arrow
                | Self::Let
                | Self::Var
                | Self::Class
                | Self::Interface
                | Self::Reexport,
                Language::Rust,
            ) => false,
            // Rust.
            (
                Self::Fn | Self::Struct | Self::Trait | Self::Static | Self::Mod | Self::Macro,
                Language::Rust,
            ) => true,
            (
                Self::Fn | Self::Struct | Self::Trait | Self::Static | Self::Mod | Self::Macro,
                Language::Ts | Language::Astro,
            ) => false,
        }
    }

    /// This kind's bit in an [`ExportTags`] set.
    ///
    /// `u32` since decision 31: ten JavaScript forms and six Rust ones is
    /// sixteen, which fits a `u16` exactly and leaves no room for the next
    /// language. The set is a field on a cached fact, so widening it is a
    /// format bump either way -- better one bump with room than two.
    fn bit(self) -> u32 {
        match self {
            Self::Function => 0x0001,
            Self::Arrow => 0x0002,
            Self::Const => 0x0004,
            Self::Let => 0x0008,
            Self::Var => 0x0010,
            Self::Class => 0x0020,
            Self::Type => 0x0040,
            Self::Interface => 0x0080,
            Self::Enum => 0x0100,
            Self::Reexport => 0x0200,
            Self::Fn => 0x0400,
            Self::Struct => 0x0800,
            Self::Trait => 0x1000,
            Self::Static => 0x2000,
            Self::Mod => 0x4000,
            Self::Macro => 0x8000,
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
pub struct ExportTags(u32);

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

/// How far an export is visible.
///
/// The second axis decision 31 separated from [`ExportKind`]. A form and a
/// visibility are orthogonal — `pub fn` and `pub(crate) fn` are the same form
/// and different exports; `pub fn` and `pub struct` are the same export and
/// different forms — and a single set holding both would make
/// `OneOf([function, pub])` sayable and meaningless.
///
/// **Only exported symbols are carried at all.** A Rust item with no `pub` is
/// not an [`ExportFact`], on the same terms as a JavaScript declaration with
/// no `export`. So this distinguishes *degrees* of exported, never exported
/// from private.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Visibility {
    /// Visible to anything that can name it: `pub`, and every JavaScript
    /// `export`.
    ///
    /// The default because it is the only one JavaScript has. A front-end that
    /// says nothing about visibility is describing a language where the
    /// question does not arise, and answering `Public` is what that means.
    #[default]
    Public,
    /// `pub(crate)`.
    Crate,
    /// `pub(super)`.
    Super,
    /// `pub(in path)`, whose reach is a module path this does not resolve.
    Restricted,
}

impl Visibility {
    /// Every visibility, for error messages and exhaustive tests.
    pub const ALL: [Self; 4] = [Self::Public, Self::Crate, Self::Super, Self::Restricted];

    /// The spelling a config uses.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Crate => "crate",
            Self::Super => "super",
            Self::Restricted => "restricted",
        }
    }

    /// Parses a visibility by its config spelling.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == name)
    }
}

impl std::fmt::Display for Visibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
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
    /// How far it is visible.
    ///
    /// `Public` for every JavaScript export, which is the only visibility that
    /// language has. Decision 31.
    #[serde(default)]
    pub visibility: Visibility,
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
    /// The string literals the call was given, in argument order.
    ///
    /// `None` in a position holds an argument that is not a string literal —
    /// a variable, an expression, a template with an interpolation in it. It
    /// is recorded as absent rather than guessed, on the same terms as
    /// [`FileFacts::has_opaque_import`]: inventing a value for something the
    /// reader cannot see makes a rule report an edge nobody wrote.
    ///
    /// Carried because some calls mean nothing without them. `invoke("greet")`
    /// has the callee `invoke` for every command in a Tauri application, and
    /// the string is the entire content; so does `t("checkout.title")` against
    /// a translation catalogue, and a feature flag key, and a job name.
    #[serde(default)]
    pub arguments: Vec<Option<String>>,
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

/// The word every comment archwarden reads starts with.
///
/// A grep for it finds everything this tool takes out of a comment, which is
/// half the reason the markers have a prefix of their own instead of borrowing
/// `JSDoc`'s `@`.
pub const PREFIX: &str = "archwarden-";

/// A claim a file makes about itself, as written in a header comment.
///
/// The frontmatter of code. `archwarden-owner: payments-team` is ownership,
/// stability or lifecycle written where it belongs — in the file it is about —
/// and it is a **claim**, never a suppression. Issue #104.
///
/// # Why it is not a `JSDoc` tag
///
/// `@internal` and `@deprecated` already mean something to `tsc`, to editors
/// and to `TypeDoc`, and a marker with two readers eventually has two
/// interpretations. The day somebody writes `@internal` for the editor's
/// benefit and archwarden reports a boundary violation is the day the feature
/// gets removed. The prefix costs an uglier line and buys one meaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataFact {
    /// The key, without the `archwarden-` prefix. Never empty.
    pub key: String,
    /// The value, as written, trimmed at both ends. Never empty.
    ///
    /// Text. There is no type system here, on the same terms as
    /// `frontmatter`: a vocabulary compares as text, `equals` compares as
    /// text, and a rule asking anything else is asking about the shape of a
    /// value, which is JSON Schema's question.
    pub value: String,
    /// Whether it was written in the file header.
    ///
    /// The header is everything before the first statement, and this version
    /// of the rule reads claims only from there — a file-level claim belongs
    /// at file level, and above-any-export needs the marker bound to the
    /// declaration under it, which is a position this does not have to solve.
    ///
    /// Worked out by the front-end, where the source text is in hand, for the
    /// same reason [`AllowanceFact::governs`] is: by the time a rule sees
    /// facts the text is gone, so a marker that stored only where *it* was
    /// could never find out what came before it.
    ///
    /// A marker below the header is carried anyway, and this is what makes it
    /// reportable. Dropping it would have archwarden say "this file declares
    /// no owner" about a file with `archwarden-owner` written in it.
    pub in_header: bool,
    /// Where the comment appears in the source.
    pub span: Span,
}

impl MetadataFact {
    /// The claim a comment's text spells, if it spells one.
    ///
    /// ```text
    /// // archwarden-owner: payments-team
    /// // archwarden-stability: experimental
    /// ```
    ///
    /// One key per line. `archwarden: owner=x, stability=y` is fewer lines and
    /// a second grammar to parse, to validate and to explain when somebody
    /// writes it wrong; this is the shape `archwarden-allow` already uses and
    /// the shape a `sed` can find.
    ///
    /// **No value, no claim**, on the same terms as a suppression with no
    /// reason. A key with nothing after the colon says nothing, and the rule
    /// that asked for the key reports it absent — which is the honest reading
    /// and the same next step.
    ///
    /// **A comment that spells a suppression is never a claim.** The two
    /// grammars share a prefix, so `archwarden-allow: because` would otherwise
    /// read as the key `allow` holding the word `because`. One comment has one
    /// meaning, and the suppression is the one that wins: that grammar has to
    /// stay small and boring, and this one gives way to it by construction
    /// rather than by keeping a list of words in step with it.
    #[must_use]
    pub fn parse(text: &str, in_header: bool, span: Span) -> Option<Self> {
        if AllowanceFact::parse(text, span).is_some() {
            return None;
        }

        let rest = text.trim_start().strip_prefix(PREFIX)?;
        let (key, value) = rest.split_once(':')?;

        let value = value.trim();
        // A key with whitespace inside it is prose that began with the word,
        // exactly as it is for a suppression; an empty one is a value with
        // nothing to hang on.
        if value.is_empty() || key.is_empty() || key.chars().any(char::is_whitespace) {
            return None;
        }

        Some(Self {
            key: key.to_owned(),
            value: value.to_owned(),
            in_header,
            span,
        })
    }

    /// Whether `archwarden-<key>:` could ever be read as this key.
    ///
    /// It cannot when the suppression grammar reaches the spelling first, which
    /// is every key beginning with `allow`. A rule asking for one of those
    /// would be unenforceable however the file was written, so the config that
    /// asks is refused where it compiles rather than left quietly reporting an
    /// absence nobody can fix.
    #[must_use]
    pub fn key_is_reachable(key: &str) -> bool {
        Self::parse(&format!("{PREFIX}{key}: value"), true, Span::new(0, 0)).is_some()
    }
}

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
    /// Claims the file makes about itself, in source order.
    ///
    /// Read out of the same pass over the comments the suppressions come from,
    /// and kept a fact of its own rather than a widening of them. They are both
    /// markers in comments and that is where the resemblance stops: a
    /// suppression changes what is reported, and a claim is something the file
    /// says. Merging them would put a grammar that can silence findings and a
    /// grammar that carries ownership in one parser. Issue #104.
    #[serde(default)]
    pub metadata: Vec<MetadataFact>,
    /// Whether the file has a dynamic import naming no single module.
    ///
    /// `import(name)` and ``import(`./locales/${name}`)`` are recorded nowhere
    /// in `imports`, because inventing a path for them would have a boundary
    /// rule report one nobody wrote. That silence is right for a rule and
    /// wrong for anything asking "who imports this file" — the honest answer
    /// there is "these ones, and I cannot see inside that one".
    #[serde(default)]
    pub has_opaque_import: bool,
    /// How many tests the file carries inside itself.
    ///
    /// Zero for a language whose tests are a sibling file, where the question
    /// is about the directory rather than about this file. Rust's unit tests
    /// are a `#[cfg(test)] mod tests` *inside* the unit, so `spec-pair` asks
    /// this of the file instead of looking beside it.
    ///
    /// A count rather than a flag, for the reason `require_non_empty_spec`
    /// refuses to count `describe`: an empty `#[cfg(test)] mod tests {}`
    /// satisfies the letter of the convention and tests nothing.
    #[serde(default)]
    pub inline_tests: usize,
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
        for claim in &mut self.metadata {
            shift(&mut claim.span);
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
            metadata: Vec::new(),
            has_opaque_import: false,
            inline_tests: 0,
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
            arguments: Vec::new(),
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
            arguments: Vec::new(),
            callee: "f".to_owned(),
            span: Span::new(u32::MAX - 1, u32::MAX),
        });

        facts.shift_spans(10);

        assert_eq!(facts.calls[0].span, Span::new(u32::MAX, u32::MAX));
    }

    /// A form and a visibility are two axes, and the vocabularies do not
    /// overlap.
    ///
    /// Decision 31's whole content, asserted as an invariant rather than
    /// described: no spelling means a form under one reading and a visibility
    /// under the other, so `must_export.kind: ["crate"]` is an unknown kind
    /// rather than a filter that quietly matches by visibility.
    #[test]
    fn no_spelling_is_both_a_form_and_a_visibility() {
        for kind in ExportKind::ALL {
            assert!(
                Visibility::parse(kind.as_str()).is_none(),
                "`{kind}` is a declaration form and parses as a visibility too"
            );
        }
        for visibility in Visibility::ALL {
            assert!(
                ExportKind::parse(visibility.as_str()).is_none(),
                "`{visibility}` is a visibility and parses as a form too"
            );
        }
    }

    /// Rust's forms are spelled Rust's way, and the JavaScript spellings do not
    /// reach them.
    ///
    /// The half of decision 31 that is a refusal. Reusing `function` for `fn`
    /// or `class` for `struct` reads better and makes a rule copied between
    /// the two halves of one repository match under the wrong language; under
    /// these spellings it matches nothing, and `doctor` says so.
    #[test]
    fn a_javascript_spelling_never_names_a_rust_form() {
        assert_eq!(ExportKind::parse("fn"), Some(ExportKind::Fn));
        assert_eq!(ExportKind::parse("struct"), Some(ExportKind::Struct));
        assert_eq!(ExportKind::parse("trait"), Some(ExportKind::Trait));

        assert_eq!(
            ExportKind::parse("function"),
            Some(ExportKind::Function),
            "and the JavaScript form still means itself"
        );
        assert_ne!(ExportKind::parse("function"), Some(ExportKind::Fn));
        assert_ne!(ExportKind::parse("class"), Some(ExportKind::Struct));
        assert_ne!(ExportKind::parse("interface"), Some(ExportKind::Trait));
    }

    /// Every kind has a bit of its own, and the set holds all sixteen at once.
    ///
    /// Asserted by counting rather than by listing: a bit copied from the line
    /// above -- which is how a `0x0400` becomes a second `0x0200` -- makes two
    /// kinds indistinguishable, and every test that names one of them passes.
    #[test]
    fn every_kind_has_a_bit_of_its_own() {
        let all = ExportKind::ALL
            .into_iter()
            .fold(ExportTags::none(), ExportTags::with);

        assert_eq!(
            all.iter().count(),
            ExportKind::ALL.len(),
            "a shared bit makes two kinds one"
        );
        for kind in ExportKind::ALL {
            assert!(all.contains(kind), "{kind}");
            assert!(
                ExportTags::only(kind).iter().eq([kind]),
                "{kind} alone is a set of one"
            );
        }
    }

    /// Every form belongs to a language, and the shared ones belong to both.
    ///
    /// The table decision 31 implies, asserted rather than described. A rule
    /// asking for a `struct` over a `.ts` file is asking for something the
    /// language cannot spell -- and without this the file is reported for
    /// exporting a `const`, which reads like a naming mistake and is a
    /// configuration mistake.
    #[test]
    fn every_form_says_which_languages_can_declare_it() {
        use crate::path::Language;

        for kind in [ExportKind::Const, ExportKind::Type, ExportKind::Enum] {
            assert!(kind.produced_by(Language::Ts), "{kind} is shared");
            assert!(kind.produced_by(Language::Rust), "{kind} is shared");
        }

        for kind in [
            ExportKind::Function,
            ExportKind::Arrow,
            ExportKind::Let,
            ExportKind::Var,
            ExportKind::Class,
            ExportKind::Interface,
            ExportKind::Reexport,
        ] {
            assert!(kind.produced_by(Language::Ts), "{kind}");
            assert!(kind.produced_by(Language::Astro), "{kind}: the fence is TS");
            assert!(!kind.produced_by(Language::Rust), "{kind} is not Rust");
        }

        for kind in [
            ExportKind::Fn,
            ExportKind::Struct,
            ExportKind::Trait,
            ExportKind::Static,
            ExportKind::Mod,
            ExportKind::Macro,
        ] {
            assert!(kind.produced_by(Language::Rust), "{kind}");
            assert!(!kind.produced_by(Language::Ts), "{kind} is not TypeScript");
            assert!(!kind.produced_by(Language::Astro), "{kind}");
        }
    }

    /// Every form is claimed by at least one language.
    ///
    /// A kind nobody can declare is a spelling a config may name and no file
    /// can satisfy -- the shape of a rule that enforces nothing while looking
    /// like it enforces something.
    #[test]
    fn no_form_is_orphaned() {
        use crate::path::Language;

        for kind in ExportKind::ALL {
            assert!(
                [Language::Ts, Language::Astro, Language::Rust]
                    .into_iter()
                    .any(|language| kind.produced_by(language)),
                "`{kind}` is a form no language in this build can declare"
            );
        }
    }

    /// A language with one visibility gets the one it has.
    #[test]
    fn public_is_the_default_because_javascript_has_no_other() {
        assert_eq!(Visibility::default(), Visibility::Public);
    }

    /// Each visibility has its own word, and the word round-trips.
    ///
    /// Named literally rather than derived, because every assertion that
    /// compares `as_str` to something else built from `as_str` -- a
    /// round-trip, a `Display` -- holds just as well when it returns the empty
    /// string for all four. These are the config spellings; if they change,
    /// someone's `arch.config.json` stops compiling and this is where they were
    /// promised.
    #[test]
    fn every_visibility_has_its_own_word() {
        assert_eq!(Visibility::Public.as_str(), "public");
        assert_eq!(Visibility::Crate.as_str(), "crate");
        assert_eq!(Visibility::Super.as_str(), "super");
        assert_eq!(Visibility::Restricted.as_str(), "restricted");

        for visibility in Visibility::ALL {
            assert_eq!(
                Visibility::parse(visibility.as_str()),
                Some(visibility),
                "the word a config writes is the one that reads back"
            );
        }
        assert_eq!(Visibility::parse("pub"), None, "the Rust keyword is not it");
        assert_eq!(Visibility::parse(""), None);
        assert_eq!(Visibility::parse("private"), None, "private is no export");
    }

    /// A visibility prints as the word a config writes, and pads like the
    /// kinds beside it.
    ///
    /// It reaches a report through `{}` -- a finding naming what it found --
    /// so the two spellings have to be one. A `Display` deriving its own text
    /// would drift from `as_str` the first time either changed.
    #[test]
    fn a_visibility_prints_the_word_a_config_writes() {
        for visibility in Visibility::ALL {
            assert_eq!(visibility.to_string(), visibility.as_str());
            assert_eq!(
                format!("{visibility:>10}"),
                format!("{:>10}", visibility.as_str()),
                "and honours the width, like ExportKind"
            );
        }
    }

    fn export(name: &str, tags: ExportTags) -> ExportFact {
        ExportFact {
            name: Some(name.to_owned()),
            tags,
            visibility: Visibility::Public,
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
            arguments: Vec::new(),
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

    fn header(text: &str) -> Option<MetadataFact> {
        MetadataFact::parse(text, true, Span::new(0, 10))
    }

    /// One key per line, and the value is the rest of it. Issue #104.
    #[test]
    fn a_marker_is_a_key_and_the_rest_of_the_line() {
        assert_eq!(
            header("archwarden-owner: payments-team"),
            Some(MetadataFact {
                key: "owner".to_owned(),
                value: "payments-team".to_owned(),
                in_header: true,
                span: Span::new(0, 10),
            })
        );
    }

    /// The value is trimmed at both ends and untouched in the middle: it is
    /// text, compared as text, and a team called `payments team` is a team.
    #[test]
    fn the_value_keeps_what_is_inside_it() {
        assert_eq!(
            header("   archwarden-owner:   payments team  ")
                .expect("a marker")
                .value,
            "payments team"
        );
    }

    /// Only the first colon separates. A value is free text and a ticket
    /// reference has one in it.
    #[test]
    fn a_value_may_contain_a_colon() {
        let marker = header("archwarden-ticket: JIRA: PAY-41").expect("a marker");

        assert_eq!(marker.key, "ticket");
        assert_eq!(marker.value, "JIRA: PAY-41");
    }

    /// Nothing after the colon is nothing said, on the same terms as a
    /// suppression with no reason: a key with no value is not a claim, and the
    /// rule that asked for the key reports it as absent.
    #[test]
    fn a_marker_without_a_value_is_not_a_marker() {
        assert!(header("archwarden-owner:").is_none());
        assert!(header("archwarden-owner:    ").is_none());
        assert!(
            header("archwarden-owner").is_none(),
            "and no colon at all is a word somebody wrote"
        );
        assert!(
            header("archwarden-: payments-team").is_none(),
            "nor is a value with no key to hang on"
        );
    }

    /// Prose that begins with the prefix is prose, told apart the way a
    /// suppression is: a key has no spaces in it.
    #[test]
    fn a_sentence_that_starts_with_the_prefix_is_not_a_marker() {
        assert!(header("archwarden-owner and the rest are read from here: see ADR-031").is_none());
    }

    /// A comment that spells a suppression is a suppression, never a claim.
    ///
    /// The two grammars share a prefix and `archwarden-allow: because` would
    /// otherwise read as the key `allow` holding the word `because`. One
    /// comment must have one meaning — the argument the issue makes against
    /// `JSDoc` — and the suppression is the one that wins, because that grammar
    /// has to stay small and boring.
    #[test]
    fn a_suppression_is_never_a_claim() {
        assert!(header("archwarden-allow: the vendor SDK has no types").is_none());
        assert!(header("archwarden-allow ui-forbids-domain: one screen").is_none());
        assert!(
            header("archwarden-allowance: 40 hours").is_none(),
            "including the shapes the suppression grammar reaches by accident"
        );
    }

    /// A marker below the header is carried, and says where it was.
    ///
    /// Reporting "this file declares no owner" about a file with the word
    /// `archwarden-owner` in it is the confidently-wrong failure this
    /// repository chases, so the fact has to survive the walk to be reported.
    #[test]
    fn a_marker_outside_the_header_is_carried_and_says_so() {
        let marker =
            MetadataFact::parse("archwarden-owner: payments-team", false, Span::new(80, 99))
                .expect("a marker");

        assert!(!marker.in_header);
        assert_eq!(marker.span, Span::new(80, 99));
    }

    /// The keys no file could ever carry are the ones the suppression grammar
    /// reaches first, and a config asking for one is refused where it compiles
    /// rather than left reporting an absence nobody can fix.
    #[test]
    fn a_key_the_suppression_grammar_swallows_is_unreachable() {
        assert!(MetadataFact::key_is_reachable("owner"));
        assert!(MetadataFact::key_is_reachable("stability"));
        assert!(MetadataFact::key_is_reachable("alignment"));

        assert!(!MetadataFact::key_is_reachable("allow"));
        assert!(
            !MetadataFact::key_is_reachable("allowance"),
            "and every key the suppression prefix reaches, not only that one"
        );
        assert!(!MetadataFact::key_is_reachable("allowed-owner"));

        assert!(
            !MetadataFact::key_is_reachable("two words"),
            "and so is a key no comment could spell"
        );
        assert!(!MetadataFact::key_is_reachable(""));
    }

    /// A file that uses none of this carries none of it, which is what keeps
    /// the cache from growing for every repository that never asks.
    #[test]
    fn a_file_without_metadata_carries_none() {
        let facts = FileFacts::unparsed(path(), ContentHash::of(b"source"));

        assert!(facts.metadata.is_empty());
    }

    /// An `.astro` module is parsed as a slice, so every span it carries is
    /// relative to the fence. A marker reported at the wrong line is one a
    /// reader opens. Issue #13's argument, applied to a new fact.
    #[test]
    fn shifting_moves_a_metadata_span_too() {
        let mut facts = FileFacts::unparsed(path(), ContentHash::of(b""));
        facts.metadata.push(MetadataFact {
            key: "owner".to_owned(),
            value: "payments-team".to_owned(),
            in_header: true,
            span: Span::new(4, 34),
        });

        facts.shift_spans(100);

        assert_eq!(facts.metadata[0].span, Span::new(104, 134));
    }

    #[test]
    fn metadata_round_trips_through_json() {
        let mut facts = FileFacts::unparsed(path(), ContentHash::of(b"source"));
        facts.metadata.push(MetadataFact {
            key: "stability".to_owned(),
            value: "experimental".to_owned(),
            in_header: false,
            span: Span::new(4, 34),
        });

        let json = serde_json::to_string(&facts).expect("serialises");
        assert_eq!(
            serde_json::from_str::<FileFacts>(&json).expect("deserialises"),
            facts
        );
    }
}
