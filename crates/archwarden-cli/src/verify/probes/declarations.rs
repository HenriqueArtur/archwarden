//! Probes for what a file says about itself.

use archwarden_core::{
    compiled::CompiledRule,
    facts::{FileFacts, Span},
    hash::ContentHash,
    traits::{Exists, FileContext, RuleEngine},
};
use archwarden_engine::walk::RepoTree;

use crate::verify::{PROBE, Verdict};

/// A file this rule covers, handed a header that cannot satisfy it.
///
/// Two plants in one, because a `metadata` rule can ask two different kinds of
/// question and a probe for one would tick for the other. Keys the rule only
/// *requires* are left undeclared — the headline case, and the same absence
/// `a_document_with_no_block` plants one rule over. Keys it asks a question
/// *about* are declared with a value that provably fails: a vocabulary is
/// refused by a string longer than every word in it, and an agreement by the
/// wanted value with a character stuck on the end.
pub(crate) fn a_file_declaring_the_wrong_thing(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
    require: &[String],
    one_of: &[(String, Vec<String>)],
    equals: &[(String, String)],
    deadline: &[String],
) -> Verdict {
    let Some(covered) = tree
        .directories()
        .flat_map(|(_, directory)| directory.files.iter())
        .map(|file| &file.path)
        .find(|path| engine.applies_to(path))
    else {
        return Verdict::Unverified {
            why: format!("no file in this repository is one `{}` is about", rule.id),
        };
    };

    let mut facts = FileFacts::unparsed(covered.clone(), ContentHash::of(PROBE.as_bytes()));
    let mut planted = false;

    for (key, accepted) in one_of {
        // Longer than every word in the vocabulary, so it is none of them.
        facts
            .metadata
            .push(claim(key, &format!("{}-", accepted.concat())));
        planted = true;
    }
    for (key, _) in equals {
        // An agreement's wanted value is rendered from the path, and working it
        // out here would be this module's own warning: a probe that
        // reimplements what it checks ticks when both are wrong the same way.
        // The same key declared twice violates the rule on its own terms and
        // needs to know nothing about templates.
        facts.metadata.push(claim(key, PROBE));
        facts.metadata.push(claim(key, PROBE));
        planted = true;
    }

    // A deadline is planted as a date already past. The probe is asked about
    // a fixed day, so this is the one plant that needs no invention at all:
    // 1970 is before every run.
    for key in deadline {
        facts.metadata.push(claim(key, "1970-01-01"));
        planted = true;
    }

    let on = if planted {
        format!("`{covered}` with a header this rule refuses")
    } else if require.is_empty() {
        return Verdict::Unverified {
            why: format!(
                "`{}` asks for no key, so there is nothing to plant",
                rule.id
            ),
        };
    } else {
        format!("`{covered}` declaring nothing about itself")
    };

    let findings = engine.check_file(FileContext {
        path: covered,
        facts: Some(&facts),
        docs: None,
        siblings: &[],
        exists: Exists::none(),
        graph: None,
        // The probe asks whether the rule bites *now*, so it answers for
        // today. A deadline planted in 1970 is past on every run there
        // has ever been.
        as_of: archwarden_core::date::Date::today(),
    });

    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}

/// One synthesised claim, in the header where the rule reads them.
pub(crate) fn claim(key: &str, value: &str) -> archwarden_core::facts::MetadataFact {
    archwarden_core::facts::MetadataFact {
        key: key.to_owned(),
        value: value.to_owned(),
        in_header: true,
        span: Span::new(0, 0),
    }
}

/// A document this rule covers, handed facts saying it has no block.
///
/// Absence is easy to synthesise, and this rule's own documentation says a
/// document with no frontmatter must be a finding rather than a skip -- so the
/// probe is the exact case the rule promises to catch.
pub(crate) fn a_document_with_no_block(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
) -> Verdict {
    let Some(covered) = tree
        .directories()
        .flat_map(|(_, directory)| directory.files.iter())
        .map(|file| &file.path)
        .find(|path| engine.applies_to(path))
    else {
        return Verdict::Unverified {
            why: format!(
                "no document in this repository is one `{}` is about",
                rule.id
            ),
        };
    };

    let docs = archwarden_core::docs::DocFacts {
        path: covered.clone(),
        content_hash: ContentHash::of(PROBE.as_bytes()),
        frontmatter: archwarden_core::docs::Frontmatter::Absent,
        headings: Vec::new(),
    };

    let findings = engine.check_file(FileContext {
        path: covered,
        facts: None,
        docs: Some(&docs),
        siblings: &[],
        exists: Exists::none(),
        graph: None,
        // The probe asks whether the rule bites *now*, so it answers for
        // today. A deadline planted in 1970 is past on every run there
        // has ever been.
        as_of: archwarden_core::date::Date::today(),
    });

    let on = format!("`{covered}` with no frontmatter block");
    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}

/// A file this rule covers, in a repository holding nothing else.
///
/// The probe is a real file -- one the rule says it applies to -- asked about
/// against an empty repository. Nothing has to be invented, unlike `naming`,
/// where a violating input is a filename and producing one means running a
/// regex backwards; here the violating input is the *absence* of the
/// companion, and absence is easy to synthesise.
pub(crate) fn a_file_with_no_companion(
    rule: &CompiledRule,
    engine: &dyn RuleEngine,
    tree: &RepoTree,
) -> Verdict {
    let Some(covered) = tree
        .directories()
        .flat_map(|(_, directory)| directory.files.iter())
        .map(|file| &file.path)
        .find(|path| engine.applies_to(path))
    else {
        return Verdict::Unverified {
            why: format!(
                "no file in this repository is one `{}` asks for a companion of",
                rule.id
            ),
        };
    };

    let findings = engine.check_file(FileContext {
        path: covered,
        facts: None,
        docs: None,
        siblings: &[],
        exists: Exists::none(),
        graph: None,
        // The probe asks whether the rule bites *now*, so it answers for
        // today. A deadline planted in 1970 is past on every run there
        // has ever been.
        as_of: archwarden_core::date::Date::today(),
    });

    let on = format!("`{covered}` with its companion missing");
    if findings.is_empty() {
        Verdict::Silent { on }
    } else {
        Verdict::Fires { on }
    }
}
