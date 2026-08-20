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
    facts::{AllowanceFact, FileFacts, MetadataFact, Span},
    hash::ContentHash,
    path::RepoRelPath,
};
use ra_ap_syntax::{
    AstNode, Edition, SourceFile, SyntaxKind, SyntaxNode, SyntaxToken, ast::HasModuleItem,
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
        imports: Vec::new(),
        exports: Vec::new(),
        calls: Vec::new(),
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

    fn facts(source: &str) -> FileFacts {
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
