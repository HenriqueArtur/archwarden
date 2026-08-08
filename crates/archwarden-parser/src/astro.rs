//! The `.astro` front-end: the module inside the fence.
//!
//! The third front-end, and the cheapest — it owns no parser. An `.astro` file
//! opens with a `---`-fenced block that is a plain TypeScript module, and that
//! is where essentially every import in an Astro page lives. So this finds the
//! fence and hands the slice to `oxc`.
//!
//! # What it does not read, and says so
//!
//! The template (`{expr}`, `<Card client:visible />`) and inline `<script>`
//! tags are a second and third module region, and neither is read here. Issue
//! #13 calls this stage 1 and argues it is worth shipping alone; the ninety per
//! cent case is the fence. What matters is that the gap is *stated* rather than
//! discovered: a dynamic `import()` inside a template expression is invisible
//! to a boundary rule, exactly as one written `import(name)` is in a `.ts`
//! file, and `has_opaque_import` is how that has always been reported.
//!
//! # Finding the closing fence
//!
//! A line that is exactly `---`. A `---` alone on a line inside a template
//! literal would close it early, which is the one case a line scan gets wrong.
//! So a slice that does not parse is retried against the next candidate fence
//! before the failure is reported: the tokenizer's opinion, bought without a
//! tokenizer.

use archwarden_core::{facts::FileFacts, hash::ContentHash, path::RepoRelPath};
use oxc_span::SourceType;

use crate::oxc::{OxcParser, ParseError};

/// The delimiter that opens and closes the frontmatter fence.
const FENCE: &str = "---";

/// Extracts facts from an `.astro` file's frontmatter.
///
/// # Errors
/// When the fence is there and does not parse as TypeScript.
pub fn parse(
    path: &RepoRelPath,
    source: &str,
    content_hash: ContentHash,
) -> Result<FileFacts, ParseError> {
    let Some(candidates) = fences(source) else {
        // No fence is not an error. A `.astro` file may be markup alone, and it
        // then has no imports, no exports and no calls -- which is a fact about
        // it, not a failure to read it.
        return Ok(FileFacts::unparsed(path.clone(), content_hash));
    };

    let mut first_failure = None;
    for (start, end) in candidates {
        match OxcParser::parse_as(path, &source[start..end], SourceType::ts(), content_hash) {
            Ok(mut facts) => {
                // oxc measured the slice; the report is about the file.
                facts.shift_spans(u32::try_from(start).unwrap_or(u32::MAX));
                return Ok(facts);
            }
            Err(error) => first_failure = first_failure.or(Some(error)),
        }
    }

    Err(first_failure.unwrap_or_else(|| ParseError::Unparsable {
        path: path.clone(),
        message: "the frontmatter fence is not a TypeScript module".to_owned(),
    }))
}

/// Every `(start, end)` the frontmatter could span, nearest fence first.
///
/// More than one, because a line that is exactly `---` inside a template
/// literal is indistinguishable from a closing fence without tokenising. The
/// caller tries them in order, so the common case costs one parse and the
/// pathological one costs a second.
fn fences(source: &str) -> Option<Vec<(usize, usize)>> {
    // The opening fence is the first thing in the file, after a BOM and any
    // leading blank lines. A `---` further down is markup, not a fence.
    let body = source.strip_prefix('\u{feff}').unwrap_or(source);
    let leading = body.len() - body.trim_start().len();
    let rest = body[leading..].strip_prefix(FENCE)?;
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))?;

    let offset = source.len() - rest.len();
    let mut candidates = Vec::new();
    let mut cursor = 0;

    for line in rest.split_inclusive('\n') {
        if line.trim_end() == FENCE {
            candidates.push((offset, offset + cursor));
        }
        cursor += line.len();
    }

    (!candidates.is_empty()).then_some(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(source: &str) -> FileFacts {
        parse(
            &RepoRelPath::new("src/pages/blog.astro").expect("valid"),
            source,
            ContentHash::of(source.as_bytes()),
        )
        .expect("parses")
    }

    fn specifiers(source: &str) -> Vec<String> {
        facts(source)
            .imports
            .iter()
            .map(|import| import.specifier.clone())
            .collect()
    }

    /// The file from issue #13, cut to its fence. Pages, layouts and components
    /// are where the imports actually happen, and those are all `.astro`.
    #[test]
    fn imports_in_the_fence_are_read() {
        let specifiers = specifiers(
            "---\n\
             import Layout from '../layouts/Base.astro';\n\
             import { getPosts } from '../lib/posts';\n\
             ---\n\
             \n\
             <Layout title=\"Blog\" />\n",
        );

        assert_eq!(specifiers, ["../layouts/Base.astro", "../lib/posts"]);
    }

    /// The rule the issue says earns its keep: an Astro page has no named
    /// component export, but it does export `getStaticPaths` and `prerender`,
    /// and "every file under `src/pages/blog/**` exports `getStaticPaths`" is a
    /// rule that falls out of reading the fence.
    #[test]
    fn the_exports_astro_does_allow_are_read() {
        let facts = facts(
            "---\n\
             export const prerender = true;\n\
             export async function getStaticPaths() { return []; }\n\
             ---\n\
             <div />\n",
        );

        let names: Vec<&str> = facts
            .exports
            .iter()
            .filter_map(|export| export.name.as_deref())
            .collect();
        assert_eq!(names, ["prerender", "getStaticPaths"]);
    }

    /// Spans are the whole reason this is not just "parse the slice". A wrong
    /// `path:line:column` is worse than none, because a reader opens it.
    #[test]
    fn spans_point_into_the_file_and_not_into_the_slice() {
        let source = "---\nimport X from './x';\n---\n";
        let facts = facts(source);

        let span = facts.imports.first().expect("one import").span;
        let at = &source[span.start as usize..span.end as usize];
        assert!(at.contains("./x"), "the span landed on `{at}`");
    }

    /// A markup-only component is not a parse failure. It has no imports, no
    /// exports and no calls, which is a fact about it.
    #[test]
    fn a_file_with_no_fence_yields_empty_facts() {
        let facts = facts("<h1>Sobre</h1>\n");

        assert!(facts.imports.is_empty());
        assert!(facts.exports.is_empty());
        assert!(facts.calls.is_empty());
    }

    /// An empty fence is a fence, and yields nothing.
    #[test]
    fn an_empty_fence_yields_empty_facts() {
        assert!(facts("---\n---\n<div />\n").imports.is_empty());
    }

    /// The case a line scan gets wrong on its own: a line that is exactly `---`
    /// inside a template literal is not the closing fence, and the difference
    /// is only visible to something that tokenises. Retrying the next candidate
    /// buys that opinion without a tokeniser.
    #[test]
    fn a_fence_inside_a_template_literal_does_not_close_it() {
        let specifiers = specifiers(
            "---\n\
             const rule = `\n\
             ---\n\
             `;\n\
             import X from './x';\n\
             ---\n\
             <div />\n",
        );

        assert_eq!(specifiers, ["./x"]);
    }

    /// A fence that does not parse under any candidate is reported as
    /// unparsable, with the first failure rather than the last -- the first is
    /// the one whose message is about the code the author wrote.
    #[test]
    fn a_fence_that_is_not_typescript_is_a_parse_error() {
        let broken = parse(
            &RepoRelPath::new("src/pages/blog.astro").expect("valid"),
            "---\nimport { from './x';\n---\n<div />\n",
            ContentHash::of(b""),
        );

        assert!(broken.is_err());
    }

    /// CRLF and a byte order mark are what an editor on Windows produces, and
    /// neither is a reason to read a file as markup-only.
    #[test]
    fn a_crlf_file_with_a_byte_order_mark_still_has_its_fence_found() {
        let specifiers = specifiers("\u{feff}---\r\nimport X from './x';\r\n---\r\n<div />\r\n");

        assert_eq!(specifiers, ["./x"]);
    }

    /// A `---` further down is markup — a horizontal rule in an MDX-ish body,
    /// or a value in a template. Reading one as a fence would invent a module.
    #[test]
    fn a_fence_that_is_not_at_the_top_is_markup() {
        let facts = facts("<h1>Sobre</h1>\n\n---\n\nimport X from './x';\n---\n");

        assert!(facts.imports.is_empty());
    }

    #[test]
    fn the_content_hash_is_carried_into_the_facts() {
        let source = "---\nimport X from './x';\n---\n";
        assert_eq!(
            facts(source).content_hash,
            ContentHash::of(source.as_bytes())
        );
    }
}
