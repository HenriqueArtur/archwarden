//! The page shell every HTML report is drawn on.
//!
//! archwarden's JSON is a contract with agents and its text output is a gate.
//! Neither is what somebody about to *change* an architecture wants to read:
//! that reader is asking where reality is pushing against the design, and gets
//! there today by running four commands and holding the results in their head.
//!
//! # What this is not
//!
//! It computes nothing. Every number and every sentence on a page comes from
//! the same `CompiledConfig` and `Report` the other renderers use — a page that
//! derived anything of its own would disagree with `check` one day, which is
//! the argument decision 9 already makes about `describe_expectation`.
//!
//! It is also read-only and self-contained: no script, no network, no font
//! request. A page that needs the internet to render is one that will not
//! render from a CI artefact in two years.
//!
//! # Why the drawing language
//!
//! Straight corners, hairline rules, hatching. The subject is walls between
//! parts of a repository, and a drafting plate says that better than a
//! dashboard does. One decision carries most of the weight: **a forbidden edge
//! is drawn, not alarmed.** A wall is the design working, so it is hatched and
//! colourless; colour is spent only on a wall being *crossed*. Hatching also
//! means the two states differ by texture and not only by hue, which is what
//! makes the plate readable to someone who cannot tell them apart by colour.

use std::fmt::Write as _;

/// The stylesheet, inlined because a page that fetches one is a page that
/// stops rendering the day the network is not there.
///
/// Tokens are redefined three times on purpose. `prefers-color-scheme` carries
/// the reader's system preference; a viewer's own toggle stamps `data-theme` on
/// the root and has to win in both directions, which a media query alone cannot
/// do.
const STYLE: &str = include_str!("html/report.css");

/// Opens the document: title, style, and the sheet the sections sit on.
pub(crate) fn open(title: &str, out: &mut dyn std::io::Write) {
    let _ = write!(
        out,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<style>\n{STYLE}</style>\n</head>\n<body>\n\
         <div class=\"sheet\">\n",
        escape(title)
    );
}

/// Closes the document.
pub(crate) fn close(out: &mut dyn std::io::Write) {
    let _ = writeln!(out, "</div>\n</body>\n</html>");
}

/// A section heading, with the eyebrow that names what kind of thing it is.
pub(crate) fn section(eyebrow: &str, heading: &str, lede: &str) -> String {
    let mut html = String::new();
    let _ = write!(
        &mut html,
        "<section>\n<div class=\"eyebrow\">{}</div>\n<h2>{}</h2>\n",
        escape(eyebrow),
        escape(heading)
    );
    if !lede.is_empty() {
        let _ = writeln!(&mut html, "<p class=\"lede\">{}</p>", escape(lede));
    }
    html
}

/// Escapes text for HTML.
///
/// Every string on a page is untrusted: a rule id, a glob, and above all a
/// `why`, which is prose an author wrote and may contain anything. A `why`
/// mentioning `<Layout />` must read as `<Layout />` and must not close the
/// paragraph it is in.
///
/// Both quote forms are escaped, not only the ones an attribute needs, because
/// the same helper is used in attribute and text position and a helper that is
/// safe in one place only is one that will be used in the other.
#[must_use]
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }

    out
}

/// Escapes a sentence, turning its backtick spans into `<code>`.
///
/// The digest and the findings write their prose once and it is rendered as
/// text, as JSON and as a page. That prose uses markdown's backticks around an
/// identifier, which a page has to turn into an element — otherwise a rule
/// reads "must not import from \`packages/infra/**\`", and the punctuation is
/// the only thing on the page that looks unfinished.
///
/// Not a markdown renderer, and deliberately: one span form, no emphasis, no
/// links. Everything else in the sentence is escaped, including what is inside
/// a span, because a glob is untrusted text like any other.
///
/// A backtick with no partner stays a backtick. Closing the span for the author
/// would put the rest of the sentence in a code font and hide where the
/// mistake is.
#[must_use]
pub(crate) fn prose(text: &str) -> String {
    let pieces: Vec<&str> = text.split('`').collect();

    // An even number of pieces means an odd number of backticks: the last one
    // opens a span nothing closes. Only that one is left as a character —
    // bailing on the whole sentence would turn every *matched* span in it back
    // into punctuation, which is a worse answer to a smaller mistake.
    let unclosed = pieces.len().is_multiple_of(2);
    let last = pieces.len().saturating_sub(1);

    pieces
        .iter()
        .enumerate()
        .map(|(index, piece)| {
            if unclosed && index == last {
                format!("`{}", escape(piece))
            } else if index.is_multiple_of(2) {
                escape(piece)
            } else {
                code(piece)
            }
        })
        .collect()
}

/// Escapes text and wraps it in `<code>`, for an identifier or a glob.
#[must_use]
pub(crate) fn code(text: &str) -> String {
    format!("<code>{}</code>", escape(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(f: impl Fn(&mut dyn std::io::Write)) -> String {
        let mut out = Vec::new();
        f(&mut out);
        String::from_utf8(out).expect("output is UTF-8")
    }

    /// A `why` is prose an author wrote, and the most likely thing to contain a
    /// character that would close the element it sits in. A page that broke on
    /// one would break on exactly the field this whole feature exists to show.
    #[test]
    fn prose_that_looks_like_markup_stays_prose() {
        assert_eq!(
            escape("<Layout /> & \"the app\"'s env"),
            "&lt;Layout /&gt; &amp; &quot;the app&quot;&#39;s env"
        );
    }

    /// Escaped in attribute position too, which is where a bare quote would let
    /// text out of the attribute and into the tag.
    #[test]
    fn both_quote_forms_are_escaped() {
        assert!(!escape(r#"a "b" 'c'"#).contains('"'));
        assert!(!escape(r#"a "b" 'c'"#).contains('\''));
    }

    /// The digest's sentences are written once and rendered three ways, so they
    /// carry markdown's backticks. On a page those came out as literal
    /// backticks -- a rule reading "must not import from
    /// \`packages/infra/**\`", which is prose wearing punctuation it does not
    /// need and the one thing on the page that looked unfinished.
    #[test]
    fn a_backtick_span_becomes_code() {
        assert_eq!(
            prose("must not import from `packages/infra/**`"),
            "must not import from <code>packages/infra/**</code>"
        );
    }

    /// What is inside the span is still escaped: a glob is untrusted text like
    /// any other, and `<` inside backticks would otherwise open an element.
    #[test]
    fn what_is_inside_a_span_is_still_escaped() {
        assert_eq!(
            prose("exports `<Layout>`"),
            "exports <code>&lt;Layout&gt;</code>"
        );
    }

    /// An odd number of backticks is prose that happens to contain one. Closing
    /// the span for the author would put the rest of the sentence in a code
    /// font and hide where the mistake is.
    #[test]
    fn an_unmatched_backtick_stays_a_backtick() {
        assert_eq!(prose("a ` b"), "a ` b");
        assert_eq!(prose("`a` and ` b"), "<code>a</code> and ` b");
    }

    /// The document has to be one a browser will render as HTML rather than
    /// display as text, and one that renders offline.
    #[test]
    fn the_document_declares_itself_and_carries_its_own_style() {
        let html = rendered(|out| {
            open("archwarden", out);
            close(out);
        });

        assert!(html.starts_with("<!doctype html>"), "{html}");
        assert!(html.contains("<style>"), "no inlined style");
        assert!(!html.contains("http://"), "nothing is fetched: {html}");
        assert!(!html.contains("https://"), "nothing is fetched: {html}");
        assert!(!html.contains("<script"), "read-only: no script");
        assert!(html.trim_end().ends_with("</html>"));
    }

    /// The title reaches the tab, escaped like everything else.
    #[test]
    fn the_title_is_escaped_too() {
        let html = rendered(|out| open("a <b> repo", out));

        assert!(html.contains("<title>a &lt;b&gt; repo</title>"), "{html}");
    }

    /// Every class the renderers emit has a rule in the stylesheet.
    ///
    /// The failure this catches shipped once: the markup was renamed from
    /// `wall` to `rule` and the stylesheet was not, so a whole section rendered
    /// as unstyled text — and it looked like a *renderer* bug, because the HTML
    /// was correct. A page whose CSS silently does not cover it is the one
    /// defect a reader cannot report accurately.
    #[test]
    fn every_class_the_pages_emit_is_styled() {
        for class in [
            "sheet",
            "masthead",
            "stamp",
            "tallies",
            "tally",
            "eyebrow",
            "lede",
            "modules",
            "module",
            "counts",
            "plate",
            "matrix",
            "cell",
            "legend",
            "swatch",
            "walls",
            "wall",
            "edge",
            "rule-id",
            "pills",
            "pill",
            "crossings",
            "blindspots",
            "rules",
            "rule",
            "id",
            "kind",
            "severity",
        ] {
            assert!(
                STYLE.contains(&format!(".{class}")),
                "`{class}` is emitted and has no rule in the stylesheet"
            );
        }
    }

    /// Both themes are defined, and the viewer's own toggle wins over the
    /// system preference in both directions -- a media query alone cannot do
    /// that.
    #[test]
    fn both_themes_are_defined_and_the_toggle_overrides_the_system() {
        assert!(STYLE.contains("prefers-color-scheme: dark"));
        assert!(STYLE.contains(r#":root[data-theme="dark"]"#));
        assert!(STYLE.contains(r#":root[data-theme="light"]"#));
    }
}
