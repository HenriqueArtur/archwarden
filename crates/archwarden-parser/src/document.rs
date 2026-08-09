//! The document front-end: markdown, read for its frontmatter.
//!
//! The second front-end, and the one that makes "front-end" mean something.
//! It shares nothing with `oxc` except the seam: a path, a source, a hash in,
//! facts out.
//!
//! # What it reads, and what it refuses to
//!
//! The `---`-fenced block at the top of the file, parsed as YAML, top-level
//! keys only. Not the body, not nested paths, not the shape of any value —
//! `docs/RULES.md` states the line, and it is the same one every other rule
//! keeps: archwarden asserts names and vocabularies, never the shape of a
//! value. A document schema is a different tool and JSON Schema is it.
//!
//! # Why a YAML crate and not a line scanner
//!
//! `status: feito` looks line-oriented until it is `status: "feito"  # done`,
//! or a flow mapping, or an anchor, or a value on the next line. A scanner
//! reads those wrong *in silence*, which is the one failure this tool exists
//! to refuse. Finding the fence is trivial and is done here; reading what is
//! inside it is not, and is not.

use archwarden_core::{
    docs::{DocFacts, DocValue, Frontmatter},
    hash::ContentHash,
    path::RepoRelPath,
};

/// The delimiter that opens and closes a frontmatter block.
const FENCE: &str = "---";

/// Reads one document's facts.
///
/// Infallible on purpose: every way a document can disappoint a rule is a
/// *fact* about it, not an error. A file with no block, and a file whose block
/// is not YAML, are two different findings — and making either an error here
/// would put them in `unreadable_files`, where the only available reading is
/// "this file is broken".
#[must_use]
pub fn read(path: &RepoRelPath, source: &str, content_hash: ContentHash) -> DocFacts {
    DocFacts {
        path: path.clone(),
        content_hash,
        frontmatter: frontmatter(source),
        // Headings arrive with the rule that asks for them, and that rule needs
        // a markdown parser: `# comment` inside a fenced code block is not a
        // heading, and a line scanner cannot tell. Empty is honest until then.
        headings: Vec::new(),
    }
}

/// Extracts and parses the fenced block at the top of the file.
fn frontmatter(source: &str) -> Frontmatter {
    let Some(block) = fenced_block(source) else {
        return Frontmatter::Absent;
    };

    let loaded = match yaml_rust2::YamlLoader::load_from_str(block) {
        Ok(documents) => documents,
        Err(error) => {
            return Frontmatter::Malformed {
                reason: error.to_string(),
            };
        }
    };

    // An empty block is a mapping with no keys, not a malformed one: the
    // author opened and closed a block and wrote nothing in it, and every
    // `require` entry is then honestly missing.
    let Some(document) = loaded.first() else {
        return Frontmatter::Present(std::collections::BTreeMap::new());
    };

    document.as_hash().map_or_else(
        || Frontmatter::Malformed {
            reason: "the block is not a mapping of keys to values".to_owned(),
        },
        |hash| Frontmatter::Present(entries(hash)),
    )
}

/// The text between the opening and closing fence, if there is a block.
///
/// The opening fence must be the *first* line: a `---` in the middle of a
/// document is a horizontal rule, and reading one as frontmatter would invent
/// a block the author never wrote.
fn fenced_block(source: &str) -> Option<&str> {
    let rest = source.strip_prefix(FENCE)?;
    // `---` and nothing else on the line. `----` is a rule, not a fence.
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))?;

    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == FENCE {
            return Some(&rest[..offset]);
        }
        offset += line.len();
    }

    // An opening fence with no closing one. Treated as no block: the author
    // wrote something that is not a frontmatter block, and guessing where it
    // ends would be inventing the block for them.
    None
}

/// Top-level keys, with each value reduced to what a rule may ask about.
fn entries(hash: &yaml_rust2::yaml::Hash) -> std::collections::BTreeMap<String, DocValue> {
    hash.iter()
        .filter_map(|(key, value)| Some((key.as_str()?.to_owned(), classify(value))))
        .collect()
}

/// Reduces a YAML value to presence, and to its text when it is a scalar.
///
/// A scalar is kept as *text* rather than typed. `one_of: [1, 2, 3]` in a
/// config and `nivel: 1` in a document are the same question asked in two
/// notations, and rendering both to `"1"` answers it without archwarden
/// growing a type system it has no other use for.
fn classify(value: &yaml_rust2::Yaml) -> DocValue {
    use yaml_rust2::Yaml;

    match value {
        // `String` and `Real` are the same arm by coincidence of both
        // carrying their text already; they are kept apart because they are
        // different YAML kinds and merging them would read as an equivalence
        // this makes no claim about.
        Yaml::String(text) | Yaml::Real(text) => DocValue::Scalar(text.clone()),
        Yaml::Integer(number) => DocValue::Scalar(number.to_string()),
        Yaml::Boolean(flag) => DocValue::Scalar(flag.to_string()),
        Yaml::Array(_) => DocValue::List,
        Yaml::Hash(_) => DocValue::Map,
        // `key:` with nothing after it, an alias, or anything a future YAML
        // revision adds: present, and nothing this rule kind asserts about.
        _ => DocValue::Empty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(source: &str) -> DocFacts {
        read(
            &RepoRelPath::new("projetos/03-semaforo/projeto.md").expect("valid"),
            source,
            ContentHash::of(source.as_bytes()),
        )
    }

    fn keys(source: &str) -> std::collections::BTreeMap<String, DocValue> {
        match facts(source).frontmatter {
            Frontmatter::Present(keys) => keys,
            other => panic!("expected a block, got {other:?}"),
        }
    }

    /// The block issue #44 was filed with.
    #[test]
    fn a_fenced_block_yields_its_top_level_keys() {
        let keys = keys(
            "---\nid: 03-semaforo\nnivel: 1\ncomponentes:\n  - { id: led, qtd: 1 }\n---\n\n# Semáforo\n",
        );

        assert_eq!(
            keys.get("id"),
            Some(&DocValue::Scalar("03-semaforo".to_owned()))
        );
        assert_eq!(keys.get("nivel"), Some(&DocValue::Scalar("1".to_owned())));
        assert_eq!(keys.get("componentes"), Some(&DocValue::List));
    }

    /// A number in the document and a number in the config are the same
    /// question in two notations. Rendering both to text answers it without a
    /// type system archwarden has no other use for.
    #[test]
    fn scalars_are_kept_as_text_whatever_yaml_called_them() {
        let keys = keys("---\ninteiro: 1\ntexto: feito\nbooleano: true\nreal: 1.5\n---\n");

        assert_eq!(keys["inteiro"], DocValue::Scalar("1".to_owned()));
        assert_eq!(keys["texto"], DocValue::Scalar("feito".to_owned()));
        assert_eq!(keys["booleano"], DocValue::Scalar("true".to_owned()));
        assert_eq!(keys["real"], DocValue::Scalar("1.5".to_owned()));
    }

    /// The case a line scanner gets wrong, and the reason this takes a real
    /// parser: quoted value, trailing comment, and a flow mapping.
    #[test]
    fn a_value_a_line_scanner_would_misread_is_read_correctly() {
        let keys = keys("---\nstatus: \"feito\"  # concluído ontem\npinos: { 23: led }\n---\n");

        assert_eq!(keys["status"], DocValue::Scalar("feito".to_owned()));
        assert_eq!(keys["pinos"], DocValue::Map);
    }

    /// A file with no block at all is a fact, not an error. The rule decides
    /// what to do about it, and `RULES.md` says it is a finding — otherwise
    /// deleting the block would be the way out of the rule.
    #[test]
    fn a_file_with_no_block_says_so() {
        assert_eq!(
            facts("# Semáforo\n\nTexto.\n").frontmatter,
            Frontmatter::Absent
        );
    }

    /// A `---` in the middle of a document is a horizontal rule. Reading one as
    /// frontmatter would invent a block the author never wrote.
    #[test]
    fn a_fence_that_is_not_the_first_line_is_a_horizontal_rule() {
        assert_eq!(
            facts("# Semáforo\n\n---\n\nOutra seção.\n").frontmatter,
            Frontmatter::Absent
        );
    }

    /// An opening fence with no closing one is not a block either: guessing
    /// where it ends would be inventing the block for the author.
    #[test]
    fn an_unclosed_fence_is_not_a_block() {
        assert_eq!(
            facts("---\nid: 03-semaforo\n\n# Semáforo\n").frontmatter,
            Frontmatter::Absent
        );
    }

    /// Malformed is its own answer, distinct from absent, because the fixes are
    /// opposite: one means write the block, the other means the block you wrote
    /// is not YAML.
    #[test]
    fn a_block_that_is_not_yaml_is_malformed_rather_than_absent() {
        let malformed = facts("---\nid: [unclosed\n---\n").frontmatter;

        assert!(
            matches!(malformed, Frontmatter::Malformed { .. }),
            "{malformed:?}"
        );
    }

    /// A block that parses but is a list, not a mapping, has no keys to ask
    /// about — and saying "absent" would be wrong, since the author clearly
    /// wrote one.
    #[test]
    fn a_block_that_is_not_a_mapping_is_malformed() {
        let malformed = facts("---\n- um\n- dois\n---\n").frontmatter;

        assert!(
            matches!(malformed, Frontmatter::Malformed { .. }),
            "{malformed:?}"
        );
    }

    /// Headings are foreseen and not yet read: the rule that wants them needs a
    /// markdown parser, because `# comment` inside a fenced code block is not a
    /// heading and a line scanner cannot tell. Empty is the honest answer.
    #[test]
    fn headings_are_not_read_yet() {
        assert!(facts("---\nid: x\n---\n\n# Semáforo\n").headings.is_empty());
    }

    #[test]
    fn the_content_hash_is_carried_into_the_facts() {
        let source = "---\nid: x\n---\n";
        assert_eq!(
            facts(source).content_hash,
            ContentHash::of(source.as_bytes())
        );
    }
}
