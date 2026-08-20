//! The Rust front-end: `use`, `pub`, calls, and the markers in the comments.
//!
//! The third code front-end, behind the same seam as the other two: a path, a
//! source and a hash in, [`FileFacts`] out. No rule, command, cache or report
//! knows it exists.
//!
//! # Why a CST and not `syn`
//!
//! Decision 32, measured. `syn` keeps `///` — those become `#[doc]` attributes
//! — and discards every `//`, which is where `archwarden-allow:` and
//! `archwarden-<key>:` live. `ra_ap_syntax` keeps them with byte ranges, and
//! keeps them *correctly*: `let url = "https://example.com";` contains a `//`
//! and is not a comment, which is the line a hand-written scanner reads wrong.
//!
//! # Infallible, deliberately
//!
//! `parse` returns `FileFacts` rather than a `Result`. `ra_ap_syntax` answers
//! a malformed file with a tree *and* a list of what was wrong with it, so a
//! file somebody is mid-edit on still yields the facts it did carry — and
//! mid-edit is exactly when the pre-write hook runs. There is no state in which
//! this front-end has nothing to say.

use archwarden_core::{
    facts::{
        AllowanceFact, CallFact, ExportFact, ExportKind, ExportTags, FileFacts, ImportFact,
        MetadataFact, Span, Visibility,
    },
    hash::ContentHash,
    path::RepoRelPath,
};
use ra_ap_syntax::{
    AstNode, Edition, SourceFile, SyntaxKind, SyntaxNode, SyntaxToken, ast,
    ast::{HasModuleItem, HasName, HasVisibility},
};

/// Extracts facts from one Rust file.
#[must_use]
pub fn parse(path: &RepoRelPath, source: &str, content_hash: ContentHash) -> FileFacts {
    let tree = SourceFile::parse(source, Edition::CURRENT).tree();
    let syntax = tree.syntax();

    let header_ends = header_ends(&tree, source);

    let mut allowances = Vec::new();
    let mut metadata = Vec::new();
    for comment in comments(syntax) {
        let text = content(comment.text());
        let range = comment.text_range();
        let (start, end) = (offset(range.start()), offset(range.end()));

        if let Some(allowance) = AllowanceFact::parse(text, next_line(source, end)) {
            allowances.push(allowance);
        }
        if let Some(claim) = MetadataFact::parse(text, end <= header_ends, Span::new(start, end)) {
            metadata.push(claim);
        }
    }

    FileFacts {
        path: path.clone(),
        content_hash,
        imports: imports(&tree),
        exports: exports(&tree),
        calls: calls(syntax),
        allowances,
        metadata,
        // Rust has no `import(name)`. The nearest thing is a `use` a macro
        // generated, which is not written in the file and is not seen here —
        // and calling that opaque would mark most of a macro-heavy crate as
        // unreadable on the strength of something nobody wrote. Issue #135
        // records it as the open question it is.
        has_opaque_import: false,
    }
}

/// Every `use` in the file, one fact per name it binds.
///
/// `use serde::{Deserialize, Serialize}` is two facts rather than one holding
/// two names, and that is the difference from the JS front-end. There, a
/// specifier is a module and the names are what was taken out of it; here the
/// two cannot be told apart without resolving — `crate::domain::user` is a
/// module or a struct depending on what is on disk — so the honest record is
/// the path as written, once per binding.
///
/// A glob (`use a::b::*`) binds no name anybody wrote. It is carried with an
/// empty `names` rather than dropped: a boundary rule is about the reach, and
/// the reach is real even where the names are not.
fn imports(tree: &SourceFile) -> Vec<ImportFact> {
    let mut facts = Vec::new();
    for item in tree.items() {
        let ast::Item::Use(node) = item else { continue };
        let Some(root) = node.use_tree() else {
            continue;
        };
        let span = span_of(node.syntax());
        walk_use_tree(&root, "", span, &mut facts);
    }
    facts
}

/// One `use` tree, flattened into the bindings it makes.
fn walk_use_tree(tree: &ast::UseTree, prefix: &str, span: Span, facts: &mut Vec<ImportFact>) {
    let path = tree.path().map(|p| p.syntax().text().to_string());
    let full = match (prefix, path.as_deref()) {
        ("", Some(p)) => p.to_owned(),
        (before, Some(p)) => format!("{before}::{p}"),
        (before, None) => before.to_owned(),
    };

    if let Some(list) = tree.use_tree_list() {
        for child in list.use_trees() {
            walk_use_tree(&child, &full, span, facts);
        }
        return;
    }

    // `use a::b::*` names nothing; `use a::b as c` names `c`; `use a::{self}`
    // names `b`, which is already the last segment of the prefix.
    let names = if tree.star_token().is_some() {
        Vec::new()
    } else if let Some(rename) = tree.rename() {
        rename
            .name()
            .map(|n| n.text().to_string())
            .into_iter()
            .collect()
    } else {
        full.rsplit("::")
            .next()
            .filter(|last| *last != "self")
            .map(ToOwned::to_owned)
            .into_iter()
            .collect()
    };

    facts.push(ImportFact {
        specifier: full,
        // No Rust resolver exists yet, so nothing is placed. Decision 19 makes
        // a boundary rule over such a file a counted skip rather than a silent
        // pass, which `looked_at` in the engine enforces.
        resolved: None,
        // Rust has no `import type`. Every `use` brings a name into scope for
        // every purpose, so opting out of type-only imports opts out of
        // nothing here.
        type_only: false,
        names,
        span,
    });
}

/// Every item the file exports, in source order.
///
/// An item with no `pub` is not carried, on the same terms as a JavaScript
/// declaration with no `export`: it is not an export, and `ExportFact` is a
/// record of exports. Decision 31 makes the *degrees* of `pub` a field.
fn exports(tree: &SourceFile) -> Vec<ExportFact> {
    tree.items()
        .filter_map(|item| {
            let (kind, name, visibility) = match &item {
                ast::Item::Fn(node) => (ExportKind::Fn, node.name(), node.visibility()),
                ast::Item::Struct(node) => (ExportKind::Struct, node.name(), node.visibility()),
                ast::Item::Enum(node) => (ExportKind::Enum, node.name(), node.visibility()),
                ast::Item::Trait(node) => (ExportKind::Trait, node.name(), node.visibility()),
                ast::Item::TypeAlias(node) => (ExportKind::Type, node.name(), node.visibility()),
                ast::Item::Const(node) => (ExportKind::Const, node.name(), node.visibility()),
                ast::Item::Static(node) => (ExportKind::Static, node.name(), node.visibility()),
                ast::Item::Module(node) => (ExportKind::Mod, node.name(), node.visibility()),
                // A `macro_rules!` is not exported by `pub`. It is exported by
                // `#[macro_export]`, which makes it reachable from the crate
                // root -- so that attribute is its visibility, and there is
                // only the one degree.
                ast::Item::MacroRules(node) => {
                    return exported_macro(node);
                }
                _ => return None,
            };

            let visibility = visibility_of(visibility.as_ref())?;

            Some(ExportFact {
                name: name.map(|n| n.text().to_string()),
                tags: ExportTags::only(kind),
                visibility,
                // Rust has no default export. A name is how an importer binds
                // one, always.
                is_default: false,
                reexport_from: None,
                forwards: None,
                annotations: Vec::new(),
                returns: None,
                span: span_of(item.syntax()),
            })
        })
        .collect()
}

/// A `macro_rules!` carrying `#[macro_export]`, as an export fact.
///
/// `None` for one without it. Rust's macro visibility is an attribute rather
/// than a `pub`, and there is one degree of it: exported from the crate root,
/// or reachable only below where it was written.
fn exported_macro(node: &ast::MacroRules) -> Option<ExportFact> {
    use ra_ap_syntax::ast::HasAttrs as _;

    node.attrs()
        .filter_map(|attr| attr.path())
        .any(|path| path.syntax().text() == "macro_export")
        .then(|| ExportFact {
            name: node.name().map(|n| n.text().to_string()),
            tags: ExportTags::only(ExportKind::Macro),
            visibility: Visibility::Public,
            is_default: false,
            reexport_from: None,
            forwards: None,
            annotations: Vec::new(),
            returns: None,
            span: span_of(node.syntax()),
        })
}

/// How far an item is visible, or `None` when it is not exported at all.
///
/// Read from the text rather than from a typed accessor because that is what
/// the CST offers, and the four spellings are closed: anything else with a
/// `pub` in it is `pub(in path)` under another arrangement of spaces.
fn visibility_of(node: Option<&ast::Visibility>) -> Option<Visibility> {
    let text = node?.syntax().text().to_string();
    let inside = text.trim_start_matches("pub").trim();

    Some(match inside {
        "" => Visibility::Public,
        "(crate)" => Visibility::Crate,
        "(super)" => Visibility::Super,
        _ => Visibility::Restricted,
    })
}

/// Every call in the file, as written at the call site.
///
/// `Event::save(x)` is recorded as `Event::save`, which is the Rust spelling of
/// the `Event.save` the JS front-end already records — so one
/// `call-obligation` rule can be written against a repository holding both,
/// naming the symbol the way its own language spells it.
fn calls(syntax: &SyntaxNode) -> Vec<CallFact> {
    syntax
        .descendants()
        .filter_map(|node| {
            if let Some(call) = ast::CallExpr::cast(node.clone()) {
                let callee = call.expr()?;
                return Some(CallFact {
                    callee: callee.syntax().text().to_string(),
                    span: span_of(&node),
                });
            }

            let method = ast::MethodCallExpr::cast(node.clone())?;
            Some(CallFact {
                callee: method.name_ref()?.text().to_string(),
                span: span_of(&node),
            })
        })
        .collect()
}

/// A node's byte range, as the span every fact carries.
fn span_of(node: &SyntaxNode) -> Span {
    let range = node.text_range();
    Span::new(offset(range.start()), offset(range.end()))
}

/// A comment's text with its delimiters removed.
///
/// The marker grammars are written against the words inside a comment, not
/// against the comment: `AllowanceFact::parse` strips whitespace and looks for
/// `archwarden-`, so a leading `//` makes every marker unreadable. The JS
/// front-end gets this from oxc's `content_span`; Rust's tokens carry their
/// delimiters, so it is taken off here.
///
/// Every spelling Rust has, because a claim written above a `///` doc is still
/// a claim and refusing it for the second slash would be a rule nobody could
/// have predicted.
fn content(text: &str) -> &str {
    if let Some(rest) = text.strip_prefix("/*") {
        return rest
            .strip_suffix("*/")
            .unwrap_or(rest)
            .trim_start_matches(['*', '!']);
    }

    text.trim_start_matches('/').trim_start_matches('!')
}

/// Every comment in the file, in source order.
///
/// Including the ones inside items: a suppression governs the line under it
/// wherever that line is, and a claim is refused for being outside the header
/// rather than for being deep in the tree.
fn comments(syntax: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> + '_ {
    syntax
        .descendants_with_tokens()
        .filter_map(ra_ap_syntax::SyntaxElement::into_token)
        .filter(|token| token.kind() == SyntaxKind::COMMENT)
}

/// Where the file header stops: the start of the first item.
///
/// The same boundary the JS front-end draws, in Rust's terms. A claim is about
/// the file, so it is read from above the file's first declaration and nowhere
/// else — a licence block above the claims does not push them out, and a claim
/// under a `use` is out.
///
/// Inner attributes and `//!` docs are *not* items, so a file opening with
/// `#![forbid(unsafe_code)]` still has a header below it. That is the reading
/// a person predicts without knowing how the parser files them.
///
/// A file with no items at all is all header: nothing has ended it.
///
/// Taken from the item's first *non-trivia* token rather than from its range.
/// In a CST the comments above a declaration belong to it, so the range of a
/// `use` with a claim written above it starts at the claim -- and every claim
/// in the file would read as being below the first item, including the ones
/// above it. The first version of this did exactly that.
fn header_ends(tree: &SourceFile, source: &str) -> u32 {
    tree.items()
        .next()
        .and_then(|item| {
            item.syntax()
                .descendants_with_tokens()
                .filter_map(ra_ap_syntax::SyntaxElement::into_token)
                .find(|token| !token.kind().is_trivia())
                .map(|token| offset(token.text_range().start()))
        })
        .unwrap_or_else(|| u32::try_from(source.len()).unwrap_or(u32::MAX))
}

/// A `ra_ap_syntax` text position as the byte offset every fact carries.
fn offset(position: ra_ap_syntax::TextSize) -> u32 {
    position.into()
}

/// The byte range of the line beginning after `offset`.
///
/// Empty when the marker is the last thing in the file, which suppresses
/// nothing and is the honest answer: there is no next line to be about. The
/// same rule the JS front-end applies, so a suppression means one thing in a
/// repository holding both languages.
fn next_line(source: &str, offset: u32) -> Span {
    let from = offset as usize;
    let Some(newline) = source.get(from..).and_then(|rest| rest.find('\n')) else {
        return Span::new(offset, offset);
    };

    let start = offset + u32::try_from(newline).unwrap_or(0) + 1;
    let end = source
        .get(start as usize..)
        .and_then(|rest| rest.find('\n'))
        .map_or_else(
            || u32::try_from(source.len()).unwrap_or(u32::MAX),
            |len| start + u32::try_from(len).unwrap_or(0),
        );

    Span::new(start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn facts(source: &str) -> FileFacts {
        parse(
            &RepoRelPath::new("src/thing.rs").expect("a path"),
            source,
            ContentHash::of(source.as_bytes()),
        )
    }

    /// The markers are read out of `//` comments, which is the whole reason
    /// this front-end is a CST rather than `syn`.
    #[test]
    fn a_suppression_and_a_claim_are_read_from_comments() {
        let file = facts(
            "// archwarden-owner: payments-team\n\
             use std::fmt;\n\
             \n\
             // archwarden-allow some-rule: the reason\n\
             pub fn thing() {}\n",
        );

        assert_eq!(file.metadata.len(), 1, "{:?}", file.metadata);
        assert_eq!(file.metadata[0].key, "owner");
        assert_eq!(file.metadata[0].value, "payments-team");

        assert_eq!(file.allowances.len(), 1, "{:?}", file.allowances);
        assert_eq!(file.allowances[0].rule_id.as_deref(), Some("some-rule"));
        assert_eq!(file.allowances[0].reason, "the reason");
    }

    /// A `//` inside a string is not a comment.
    ///
    /// The line decision 32 was decided on. A scanner looking for `//` reads a
    /// suppression here that nobody wrote — and a suppression nobody wrote is
    /// worse than a violation, because it hides one silently.
    #[test]
    fn a_double_slash_inside_a_string_is_not_a_marker() {
        let file = facts(
            "pub fn urls() {\n\
             \x20   let a = \"// archwarden-allow fake: not a comment\";\n\
             \x20   let b = r#\"// archwarden-owner: also-fake\"#;\n\
             \x20   let c = '/';\n\
             }\n",
        );

        assert!(file.allowances.is_empty(), "{:?}", file.allowances);
        assert!(file.metadata.is_empty(), "{:?}", file.metadata);
    }

    /// A claim belongs in the header, and the header ends at the first item.
    #[test]
    fn a_claim_below_the_first_item_is_not_in_the_header() {
        let file = facts(
            "// archwarden-owner: above\n\
             use std::fmt;\n\
             // archwarden-owner: below\n\
             pub fn thing() {}\n",
        );

        assert_eq!(file.metadata.len(), 2, "both are read");
        assert!(file.metadata[0].in_header, "the one above the first item");
        assert!(!file.metadata[1].in_header, "and the one below it is not");
    }

    /// Inner attributes and `//!` docs do not end the header.
    ///
    /// They are not items, and a file opening with `#![forbid(unsafe_code)]`
    /// still has a header below it -- which is the reading somebody predicts
    /// without knowing how the parser files them.
    #[test]
    fn a_lint_attribute_above_a_claim_does_not_push_it_out_of_the_header() {
        let file = facts(
            "//! What this module is.\n\
             #![allow(clippy::pedantic)]\n\
             // archwarden-owner: payments-team\n\
             pub fn thing() {}\n",
        );

        assert_eq!(file.metadata.len(), 1);
        assert!(file.metadata[0].in_header, "{:?}", file.metadata[0]);
    }

    /// A file that will not parse still yields what it carried.
    ///
    /// The reason this function returns facts rather than a `Result`: the
    /// pre-write hook runs on a file somebody is in the middle of editing, and
    /// throwing away its readable half is the least useful thing to do with it.
    #[test]
    fn a_file_that_does_not_parse_still_yields_its_markers() {
        let file = facts(
            "// archwarden-owner: payments-team\n\
             pub fn broken( {\n",
        );

        assert_eq!(file.metadata.len(), 1, "{:?}", file.metadata);
        assert_eq!(file.metadata[0].value, "payments-team");
    }

    /// Nothing at all is not an error either.
    #[test]
    fn an_empty_file_is_facts_with_nothing_in_them() {
        let file = facts("");

        assert!(file.allowances.is_empty());
        assert!(file.metadata.is_empty());
        assert!(!file.has_opaque_import);
    }

    /// A suppression governs the line *after* it, and the span says which.
    ///
    /// The arithmetic is asserted rather than the behaviour, because the
    /// behaviour looks identical for every off-by-one: a span reaching one line
    /// too far silences a statement nobody meant to silence, and one reaching
    /// too little silences nothing while the marker sits there looking like it
    /// worked.
    #[test]
    fn a_suppression_governs_the_line_below_it() {
        let source = "// archwarden-allow r: why
let x = 1;
let y = 2;
";
        let file = facts(source);

        let governs = file.allowances[0].governs;
        assert_eq!(
            &source[governs.start as usize..governs.end as usize],
            "let x = 1;",
            "the line under the marker, and only it"
        );
    }

    /// A marker on the last line governs nothing, and says so with an empty
    /// span rather than by reaching past the end of the file.
    #[test]
    fn a_suppression_with_no_line_below_it_governs_nothing() {
        let file = facts(
            "pub fn thing() {}
// archwarden-allow r: why",
        );

        let governs = file.allowances[0].governs;
        assert_eq!(governs.start, governs.end, "empty, not out of bounds");
    }

    /// A block-comment marker with code after it on the same line still
    /// governs the *next* line.
    ///
    /// The only shape that distinguishes the arithmetic. A `//` marker always
    /// ends at its newline, so the distance to that newline is zero and every
    /// wrong operator lands on the right answer anyway; a `/* */` can be
    /// followed by more of the same line, and then it cannot.
    #[test]
    fn a_block_marker_with_code_after_it_still_governs_the_line_below() {
        let source = "/* archwarden-allow r: why */ let a = 1;\nlet x = 2;\n";
        let file = facts(source);

        let governs = file.allowances[0].governs;
        assert_eq!(
            &source[governs.start as usize..governs.end as usize],
            "let x = 2;",
            "the line below, not the rest of the marker's own line"
        );
    }

    /// The last line of a file with no trailing newline is still a line.
    #[test]
    fn a_suppression_governs_a_final_line_that_has_no_newline_after_it() {
        let source = "// archwarden-allow r: why
let x = 1;";
        let file = facts(source);

        let governs = file.allowances[0].governs;
        assert_eq!(
            &source[governs.start as usize..governs.end as usize],
            "let x = 1;"
        );
    }

    /// Ordinary prose is not carried. `FileFacts` is cached per file, and a
    /// repository's comments are larger than its code.
    #[test]
    fn a_comment_that_is_not_a_marker_is_not_a_fact() {
        let file = facts("// just explaining something\npub fn thing() {}\n");

        assert!(file.allowances.is_empty());
        assert!(file.metadata.is_empty());
    }
}

#[cfg(test)]
mod facts_tests {
    use super::tests::facts;
    use archwarden_core::facts::{ExportKind, ExportTags, Visibility};

    /// One fact per name a `use` binds, and the path as written.
    ///
    /// A braced group is several bindings, not one holding several names --
    /// which is where this parts company with the JS front-end. There a
    /// specifier is a module and the names came out of it; here the two cannot
    /// be told apart without resolving, since `crate::domain::user` is a module
    /// or a struct depending on what is on disk.
    #[test]
    fn a_use_is_one_fact_per_name_it_binds() {
        let file = facts(
            "use std::collections::BTreeMap;\n\
             use serde::{Deserialize, Serialize};\n\
             use crate::domain::user as u;\n",
        );

        let seen: Vec<(&str, Vec<&str>)> = file
            .imports
            .iter()
            .map(|import| {
                (
                    import.specifier.as_str(),
                    import.names.iter().map(String::as_str).collect(),
                )
            })
            .collect();

        assert_eq!(
            seen,
            vec![
                ("std::collections::BTreeMap", vec!["BTreeMap"]),
                ("serde::Deserialize", vec!["Deserialize"]),
                ("serde::Serialize", vec!["Serialize"]),
                ("crate::domain::user", vec!["u"]),
            ]
        );
    }

    /// A glob reaches without naming, and is carried for the reach.
    ///
    /// Dropping it would make `use crate::domain::*` invisible to a boundary
    /// rule -- the widest reach a file can have, recorded as no reach at all.
    #[test]
    fn a_glob_import_is_carried_with_no_names() {
        let file = facts("use crate::domain::*;\n");

        assert_eq!(file.imports.len(), 1);
        assert_eq!(file.imports[0].specifier, "crate::domain");
        assert!(file.imports[0].names.is_empty(), "a star names nobody");
    }

    /// `self` in a group is the module the group is under, not a name.
    #[test]
    fn self_in_a_group_binds_the_module_rather_than_a_name_of_its_own() {
        let file = facts("use crate::domain::user::{self, Thing};\n");

        let specifiers: Vec<&str> = file.imports.iter().map(|i| i.specifier.as_str()).collect();
        assert_eq!(
            specifiers,
            vec!["crate::domain::user::self", "crate::domain::user::Thing"]
        );
        assert!(file.imports[0].names.is_empty(), "`self` is not a new name");
        assert_eq!(file.imports[1].names, vec!["Thing".to_owned()]);
    }

    /// Nothing about a `use` is resolved, and nothing is type-only.
    ///
    /// Both are stated rather than left to default: an unresolved specifier is
    /// what makes a boundary rule a counted skip (decision 19), and Rust has no
    /// `import type`, so a rule opting out of type-only imports opts out of
    /// nothing.
    #[test]
    fn a_rust_import_is_unresolved_and_never_type_only() {
        let file = facts("use crate::domain::Thing;\n");

        assert!(file.imports[0].resolved.is_none());
        assert!(!file.imports[0].type_only);
    }

    /// Every `pub` item is an export, carrying its form and its reach apart.
    #[test]
    fn each_form_is_tagged_as_itself_and_carries_its_visibility() {
        let file = facts(
            "pub fn f() {}\n\
             pub struct S;\n\
             pub enum E { A }\n\
             pub trait T {}\n\
             pub type A = u8;\n\
             pub const C: u8 = 1;\n\
             pub static ST: u8 = 1;\n\
             pub mod m {}\n",
        );

        let seen: Vec<(&str, ExportTags)> = file
            .exports
            .iter()
            .map(|e| (e.name.as_deref().unwrap_or(""), e.tags))
            .collect();

        assert_eq!(
            seen,
            vec![
                ("f", ExportTags::only(ExportKind::Fn)),
                ("S", ExportTags::only(ExportKind::Struct)),
                ("E", ExportTags::only(ExportKind::Enum)),
                ("T", ExportTags::only(ExportKind::Trait)),
                ("A", ExportTags::only(ExportKind::Type)),
                ("C", ExportTags::only(ExportKind::Const)),
                ("ST", ExportTags::only(ExportKind::Static)),
                ("m", ExportTags::only(ExportKind::Mod)),
            ]
        );
        assert!(
            file.exports
                .iter()
                .all(|e| e.visibility == Visibility::Public),
            "a bare `pub` is public"
        );
        assert!(
            file.exports.iter().all(|e| !e.is_default),
            "Rust has no default export"
        );
    }

    /// The four degrees of exported, each on its own.
    ///
    /// Separately, because they are four arms of one match and a test naming
    /// only `pub` passes while the other three collapse into it -- after which
    /// a rule asking for a public API silently accepts a crate-private one.
    #[test]
    fn every_degree_of_pub_is_told_apart() {
        let file = facts(
            "pub fn a() {}\n\
             pub(crate) fn b() {}\n\
             pub(super) fn c() {}\n\
             pub(in crate::x) fn d() {}\n",
        );

        let seen: Vec<Visibility> = file.exports.iter().map(|e| e.visibility).collect();
        assert_eq!(
            seen,
            vec![
                Visibility::Public,
                Visibility::Crate,
                Visibility::Super,
                Visibility::Restricted,
            ]
        );
    }

    /// An item with no `pub` is not an export at all.
    #[test]
    fn a_private_item_is_no_export() {
        let file = facts(
            "fn hidden() {}\n\
             struct Hidden;\n\
             pub fn shown() {}\n",
        );

        assert_eq!(file.exports.len(), 1, "{:?}", file.exports);
        assert_eq!(file.exports[0].name.as_deref(), Some("shown"));
    }

    /// A macro is exported by an attribute, not by `pub`.
    ///
    /// Found by mutation testing, which deleted the match arm and nothing
    /// failed: the first version handed `macro_rules!` a `None` visibility, so
    /// the arm could never produce an export and was dead code that looked
    /// like support.
    #[test]
    fn a_macro_is_exported_by_its_attribute_rather_than_by_pub() {
        let file = facts(
            "#[macro_export]\n             macro_rules! shouted { () => {} }\n             macro_rules! quiet { () => {} }\n",
        );

        assert_eq!(file.exports.len(), 1, "{:?}", file.exports);
        assert_eq!(file.exports[0].name.as_deref(), Some("shouted"));
        assert_eq!(file.exports[0].tags, ExportTags::only(ExportKind::Macro));
        assert_eq!(file.exports[0].visibility, Visibility::Public);
    }

    /// An attribute that is not `macro_export` does not export one.
    #[test]
    fn another_attribute_on_a_macro_does_not_export_it() {
        let file = facts("#[allow(unused_macros)]\n             macro_rules! quiet { () => {} }\n");

        assert!(file.exports.is_empty(), "{:?}", file.exports);
    }

    /// A call is recorded as its path was written.
    ///
    /// `Event::save` is the Rust spelling of the `Event.save` the JS front-end
    /// records, so one `call-obligation` rule can be written against a
    /// repository holding both, naming the symbol the way its language spells
    /// it.
    #[test]
    fn a_call_is_recorded_as_written_including_its_path() {
        let file = facts(
            "pub fn f() {\n\
             \x20   Event::save(1);\n\
             \x20   crate::audit::record();\n\
             \x20   plain();\n\
             }\n",
        );

        let callees: Vec<&str> = file.calls.iter().map(|c| c.callee.as_str()).collect();
        assert_eq!(
            callees,
            vec!["Event::save", "crate::audit::record", "plain"]
        );
    }

    /// A method call is recorded by its name, without the receiver.
    ///
    /// The receiver is an expression rather than a path -- `self.repo.save()`
    /// and `thing().save()` are the same call to `save` -- so recording it
    /// would make the callee un-matchable by any rule.
    #[test]
    fn a_method_call_is_recorded_by_its_name() {
        let file = facts("pub fn f(x: T) { x.save(); }\n");

        let callees: Vec<&str> = file.calls.iter().map(|c| c.callee.as_str()).collect();
        assert_eq!(callees, vec!["save"]);
    }

    /// A call's span is the call, so a finding points at it.
    #[test]
    fn a_call_carries_the_span_of_the_call() {
        let source = "pub fn f() { Event::save(1); }\n";
        let file = facts(source);

        let span = file.calls[0].span;
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            "Event::save(1)"
        );
    }
}
