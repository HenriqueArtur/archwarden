//! Whether a sentence has already been said here.
//!
//! Issue #162. `alternatives` records what was rejected and why it lost, and
//! `config explain` ends with "Do not propose it again." -- which it can only
//! say to somebody who already knows the decision's id. The person about to
//! propose the losing option is, by definition, not that person, and they will
//! name it differently from whoever rejected it: "single layer", "monolith",
//! "one package" and "just put it together" are the same option under four
//! names.
//!
//! Two measurements decide the whole design.
//!
//! **The corpus is tiny.** Twenty-eight decisions with three alternatives each
//! is a hundred short strings. A linear scan is microseconds, and every
//! expensive retrieval technique exists for corpora this is not.
//!
//! **The errors are asymmetric.** A false negative means the rejected option
//! gets proposed again, which is the exact failure `alternatives` exists to
//! prevent; a false positive costs two seconds of reading.
//!
//! Together: **do not rank, do not miss.** Every candidate that matches, in
//! declaration order, with no score. The question was never "which is most
//! similar", it was "is there anything similar" -- and that removes the reason
//! to reach for anything that answers with a float.

use archwarden_core::compiled::{CompiledConfig, CompiledDecision};

/// How one query token reached one candidate token.
///
/// Carried rather than collapsed into a yes, on the same terms as a finding's
/// `observed`: a reader adjusts the query by seeing the reason, where a number
/// they cannot inspect leaves them guessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum How {
    /// The same token.
    Exact,
    /// The query token is a prefix of the candidate's, or the reverse.
    ///
    /// A minimum length, because three characters prefix half a language.
    /// Chosen over a stemmer: a stemmer is a per-language dependency whose
    /// output nobody can predict, while a minimum is one documented integer
    /// that reaches `camada`/`camadas` and `package`/`packages` just the same.
    Prefix,
    /// They differ by at most this many single-character edits.
    ///
    /// Capped rather than scored. A trigram similarity gives a float, a float
    /// needs a threshold, and a threshold is ranking wearing a different hat.
    /// A cap answers yes or no and its failure is legible -- "differs by one
    /// character" is something a reader can act on.
    Edits(usize),
}

/// One query token reaching one candidate token, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    /// The token from the query.
    pub query: String,
    /// The token from the candidate text.
    pub candidate: String,
    /// Which layer matched.
    pub how: How,
}

/// Which part of a decision a hit is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    /// The decision's title.
    Title,
    /// The decision's `why`.
    Why,
    /// A rejected option's name, by index.
    Option(usize),
    /// A rejected option's argument against it, by index.
    WhyNot(usize),
}

impl Where {
    /// The field path, as `config explain` would spell it.
    #[must_use]
    pub fn path(&self) -> String {
        match self {
            Self::Title => "title".to_owned(),
            Self::Why => "why".to_owned(),
            Self::Option(index) => format!("alternatives[{index}].option"),
            Self::WhyNot(index) => format!("alternatives[{index}].why_not"),
        }
    }
}

/// One place a query reached, and why.
#[derive(Debug, Clone)]
pub struct Hit<'a> {
    /// The decision it is in.
    pub decision: &'a CompiledDecision,
    /// Which of its fields.
    pub at: Where,
    /// The text of that field.
    pub text: String,
    /// Every token pair that matched, in query order.
    pub reasons: Vec<Reason>,
}

/// Words carrying no argument, dropped from both sides.
///
/// A committed list rather than a library, so behaviour is identical on every
/// machine with nothing configured. Short on purpose: a long stopword list
/// starts removing words that are the whole point of a phrase.
///
/// Both languages in one list rather than one per language. A word that
/// carries no argument in English carries none in a Portuguese sentence
/// either, and the alternative is asking a query which language it is in --
/// which is a question the person typing it should not have to answer.
const STOPWORDS: &[&str] = &[
    "a", "an", "ao", "aos", "and", "are", "as", "at", "be", "by", "com", "como", "da", "das", "de",
    "do", "dos", "e", "em", "for", "from", "in", "into", "is", "it", "its", "na", "nas", "no",
    "nos", "of", "on", "or", "os", "ou", "para", "por", "que", "se", "sem", "that", "the", "then",
    "there", "this", "to", "um", "uma", "was", "with",
];

/// The shortest prefix that counts as one.
///
/// Four, and it is one documented integer rather than a stemmer's opinion.
const PREFIX_MINIMUM: usize = 4;

/// Folds a character to its unaccented form.
///
/// A table rather than Unicode normalisation, because the two languages this
/// serves are Latin alphabets and a full normalisation crate is a dependency
/// far wider than the need. It fails safe: a character with no entry is kept
/// as it is, so at worst two spellings do not meet.
///
/// Not optional for a bilingual repository -- without it `única` and `unica`
/// are different strings, and nobody types the accent into a query.
fn fold(character: char) -> char {
    match character {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

/// The tokens of a phrase: lowercased, unaccented, split on anything else.
///
/// Both sides go through this, so a query and a candidate are compared as the
/// same kind of thing. Stopwords are dropped last, after folding, so `não` and
/// `nao` are the same word to the list.
#[must_use]
pub fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .map(|character| {
            let folded = fold(character);
            if folded.is_alphanumeric() {
                folded
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .filter(|word| !STOPWORDS.contains(word))
        .map(str::to_owned)
        .collect()
}

/// Levenshtein distance, abandoned once it passes `cap`.
///
/// The cap is not an optimisation: it is the answer. Nothing here wants to
/// know that two words differ by nine edits.
fn within(left: &str, right: &str, cap: usize) -> Option<usize> {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    if left.len().abs_diff(right.len()) > cap {
        return None;
    }

    let mut previous: Vec<usize> = (0..=right.len()).collect();

    for (i, l) in left.iter().enumerate() {
        let mut current = Vec::with_capacity(right.len() + 1);
        current.push(i + 1);
        for (j, r) in right.iter().enumerate() {
            let deletion = previous
                .get(j + 1)
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(1);
            let insertion = current
                .last()
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(1);
            let substitution = previous
                .get(j)
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(usize::from(l != r));
            current.push(deletion.min(insertion).min(substitution));
        }
        // Nothing on this row is within the cap, and a row never improves on
        // the one above it, so no later row can be either.
        if current.iter().min().copied().unwrap_or(usize::MAX) > cap {
            return None;
        }
        previous = current;
    }

    previous.last().copied().filter(|distance| *distance <= cap)
}

/// The edits a token of this length is allowed to differ by.
///
/// None below four: at three characters one edit reaches a third of the
/// language.
fn cap_for(length: usize) -> usize {
    match length {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    }
}

/// How one query token reaches one candidate token, if it does.
#[must_use]
pub fn reach(query: &str, candidate: &str) -> Option<How> {
    if query == candidate {
        return Some(How::Exact);
    }

    let shorter = query.chars().count().min(candidate.chars().count());
    if shorter >= PREFIX_MINIMUM && (query.starts_with(candidate) || candidate.starts_with(query)) {
        return Some(How::Prefix);
    }

    let cap = cap_for(query.chars().count().max(candidate.chars().count()));
    if cap == 0 {
        return None;
    }
    within(query, candidate, cap).map(How::Edits)
}

/// Every query token that reaches something in this text.
///
/// Empty when nothing does, which is what makes a candidate a miss.
#[must_use]
pub fn reasons(query: &[String], text: &str) -> Vec<Reason> {
    let candidates = tokens(text);
    let mut found = Vec::new();
    for term in query {
        for candidate in &candidates {
            if let Some(how) = reach(term, candidate) {
                found.push(Reason {
                    query: term.clone(),
                    candidate: candidate.clone(),
                    how,
                });
                break;
            }
        }
    }
    found
}

/// Every place in the configuration a query reaches, in declaration order.
///
/// No ranking and no top-N: with a hundred candidates, returning eight instead
/// of three is free, and recall is the only thing that matters here.
#[must_use]
pub fn search<'a>(config: &'a CompiledConfig, terms: &str) -> Vec<Hit<'a>> {
    let query = tokens(terms);
    if query.is_empty() {
        return Vec::new();
    }

    let mut hits = Vec::new();
    for decision in config.decisions() {
        let mut fields: Vec<(Where, String)> = vec![(Where::Title, decision.title.clone())];
        if let Some(why) = &decision.why {
            fields.push((Where::Why, why.clone()));
        }
        for (index, alternative) in decision.alternatives.iter().enumerate() {
            fields.push((Where::Option(index), alternative.option.clone()));
            fields.push((Where::WhyNot(index), alternative.why_not.clone()));
        }

        for (at, text) in fields {
            let reasons = reasons(&query, &text);
            if !reasons.is_empty() {
                hits.push(Hit {
                    decision,
                    at,
                    text,
                    reasons,
                });
            }
        }
    }
    hits
}

/// The search, as JSON.
///
/// One shape for both surfaces. `decisions find --format json` and the MCP
/// `decisions_find` tool answer the same question, and two renderings of one
/// answer are two renderings that drift.
#[must_use]
pub fn similar_json(config: &CompiledConfig, terms: &str) -> serde_json::Value {
    let hits = search(config, terms);
    serde_json::json!({
        "query": terms,
        "hits": hits.iter().map(|hit| serde_json::json!({
            "decision": hit.decision.id.as_str(),
            "title": hit.decision.title,
            "at": hit.at.path(),
            "text": hit.text,
            "link": hit.decision.link,
            "reasons": hit.reasons.iter().map(|reason| serde_json::json!({
                "query": reason.query,
                "candidate": reason.candidate,
                "how": match reason.how {
                    How::Exact => "exact".to_owned(),
                    How::Prefix => "prefix".to_owned(),
                    How::Edits(distance) => format!("edits:{distance}"),
                },
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// Two places in the configuration saying what looks like the same thing.
#[derive(Debug, Clone)]
pub struct Duplicate<'a> {
    /// The decision declared first.
    pub earlier: &'a CompiledDecision,
    /// Which of its name-bearing fields.
    pub earlier_at: Where,
    /// The decision declared after it.
    pub later: &'a CompiledDecision,
    /// Which of its name-bearing fields.
    pub later_at: Where,
    /// The text they share, as the later one spells it.
    pub text: String,
    /// Why they matched.
    pub reasons: Vec<Reason>,
}

/// The fields of a decision that *name* something.
///
/// Titles and rejected options only -- not `why` or `why_not`. Prose shares
/// vocabulary constantly, and two decisions both saying "the domain would
/// import the transport" are agreeing, not duplicating.
fn names(decision: &CompiledDecision) -> Vec<(Where, String)> {
    let mut fields = vec![(Where::Title, decision.title.clone())];
    for (index, alternative) in decision.alternatives.iter().enumerate() {
        fields.push((Where::Option(index), alternative.option.clone()));
    }
    fields
}

/// Whether these two names are the same statement under two spellings.
///
/// Every token of the shorter side has to be reached, which is a far tighter
/// question than [`search`] asks -- and deliberately so. `search` answers a
/// person who typed a query and will read what comes back, where a false
/// positive costs two seconds. This answers inside `config doctor`, which
/// lives in a gate, and a gate that cries wolf is one somebody turns off. So
/// the pull is recall-first and the push is precision-first, and the two use
/// the same three layers to stay explainable.
fn same_statement(left: &str, right: &str) -> Option<Vec<Reason>> {
    let (left_tokens, right_tokens) = (tokens(left), tokens(right));
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return None;
    }

    let (shorter, longer) = if left_tokens.len() <= right_tokens.len() {
        (&left_tokens, right)
    } else {
        (&right_tokens, left)
    };

    let found = reasons(shorter, longer);
    (found.len() == shorter.len()).then_some(found)
}

/// Every pair of decisions that appear to say the same thing.
///
/// Each unordered pair once, in declaration order, so the report reads as a
/// list of places to look rather than as the same finding twice.
#[must_use]
pub fn duplicates(config: &CompiledConfig) -> Vec<Duplicate<'_>> {
    let decisions: Vec<&CompiledDecision> = config.decisions().collect();
    let mut found = Vec::new();

    for (index, earlier) in decisions.iter().enumerate() {
        for later in decisions.iter().skip(index + 1) {
            for (earlier_at, earlier_text) in names(earlier) {
                for (later_at, later_text) in names(later) {
                    if let Some(reasons) = same_statement(&earlier_text, &later_text) {
                        found.push(Duplicate {
                            earlier,
                            earlier_at: earlier_at.clone(),
                            later,
                            later_at,
                            text: later_text,
                            reasons,
                        });
                    }
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use archwarden_core::{
        compiled::{CompiledAlternative, CompiledDecision, DecisionStatus, SkipDirs},
        glob::PathSet,
        hash::ContentHash,
        ids::DecisionId,
    };

    use super::*;

    /// Folding is not optional for a bilingual repository: nobody types the
    /// accent into a query, and without this `única` and `unica` are two
    /// different words.
    #[test]
    fn tokens_are_lowercased_unaccented_and_stripped_of_stopwords() {
        assert_eq!(
            tokens("A Única Camada, e não duas"),
            ["unica", "camada", "nao", "duas"]
        );
        // Punctuation is a separator, not a character.
        assert_eq!(
            tokens("pub/sub, message-broker"),
            ["pub", "sub", "message", "broker"]
        );
        // A phrase of nothing but stopwords has no tokens, which is what makes
        // an empty query answer with nothing rather than with everything.
        assert!(tokens("the and of").is_empty());
        // The whole table, because a letter that folds and a letter that does
        // not are the same silent failure: two spellings that never meet.
        assert_eq!(
            tokens("ámanhã êxito índice órfão último çedilha ñandu"),
            [
                "amanha", "exito", "indice", "orfao", "ultimo", "cedilha", "nandu"
            ]
        );
        assert_eq!(tokens("àèìòùâôûäëïöüåÿ"), ["aeiouaouaeiouay"]);
    }

    /// The three layers, and the reason each exists.
    #[test]
    fn a_token_reaches_another_by_exactness_prefix_or_a_capped_edit() {
        assert_eq!(reach("layer", "layer"), Some(How::Exact));

        // Prefix, where a stemmer would be reached for: plural and singular,
        // in either language, without a per-language dependency.
        assert_eq!(reach("camada", "camadas"), Some(How::Prefix));
        assert_eq!(reach("package", "packages"), Some(How::Prefix));
        // And below the minimum it does not fire -- three characters prefix
        // half a language.
        assert_eq!(reach("mon", "monolith"), None);

        // A near-miss, answered with how far off it is rather than with a
        // score.
        assert_eq!(reach("monolitico", "monolitica"), Some(How::Edits(1)));
        // Short words get no allowance at all: at three characters one edit
        // reaches a third of the language.
        assert_eq!(reach("cat", "car"), None);
        // And a long one gets two, not more. Real words on both sides, in
        // both tests: a deliberate misspelling here would be a misspelling the
        // repository's own spell-check has to be taught to accept.
        assert_eq!(reach("comparador", "compilador"), Some(How::Edits(2)));
        assert_eq!(reach("comparador", "carregador"), None);
        assert_eq!(reach("layer", "broker"), None);
    }

    /// The distance itself, asserted directly. `reach` answers the three
    /// layers together and would hide an arithmetic slip inside the last one.
    #[test]
    fn the_distance_is_counted_and_abandoned_at_the_cap() {
        assert_eq!(within("layer", "layer", 2), Some(0));
        assert_eq!(within("layer", "lager", 2), Some(1));
        assert_eq!(within("camada", "camisa", 2), Some(2));
        assert_eq!(within("camada", "camisa", 1), None);

        // A length difference of exactly the cap is still reachable: it is the
        // *distance* that is capped, and each missing character costs one.
        assert_eq!(within("camada", "camadas", 1), Some(1));
        assert_eq!(within("camada", "camadinha", 2), None);
        assert_eq!(within("pacote", "pacotes", 1), Some(1));
        // Characters dropped from the *front* of the longer side cost the same
        // as any others. The first column of the matrix is what says so, and
        // an off-by-one there makes a prefix free -- so `empacote` would reach
        // `pacote` as though it were one edit away.
        assert_eq!(within("empacote", "pacote", 2), Some(2));
        assert_eq!(within("empacote", "pacote", 1), None);
    }

    /// The allowance a length earns, and the middle band is the one that has
    /// to exist: without it a five-letter word gets two edits, and `layer`
    /// reaches `lager` as though they were the same word.
    #[test]
    fn a_short_word_earns_less_allowance_than_a_long_one() {
        assert_eq!(cap_for(3), 0);
        assert_eq!(cap_for(4), 1);
        assert_eq!(cap_for(7), 1);
        assert_eq!(cap_for(8), 2);

        // Read through `reach`, which is where it matters: two real words of
        // six letters, two edits apart, are two words.
        assert_eq!(reach("camada", "camisa"), None);
        assert_eq!(reach("players", "layer"), None);
        assert_eq!(reach("camada", "camara"), Some(How::Edits(1)));
    }

    fn decision(title: &str, alternatives: Vec<CompiledAlternative>) -> CompiledDecision {
        CompiledDecision {
            scope: None,
            why_not_enforceable: None,
            id: DecisionId::new("ADR-001").expect("valid"),
            title: title.to_owned(),
            why: None,
            link: None,
            status: DecisionStatus::Accepted,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            alternatives,
        }
    }

    fn config(decisions: Vec<CompiledDecision>) -> CompiledConfig {
        CompiledConfig::new(
            Vec::new(),
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b"similar"),
        )
        .with_decisions(decisions)
    }

    fn rejected(option: &str, why_not: &str) -> CompiledAlternative {
        CompiledAlternative {
            option: option.to_owned(),
            why_not: why_not.to_owned(),
            refused_by: None,
        }
    }

    /// The line this exists for: *has this already been rejected?*, asked by
    /// somebody who does not know the decision's id and names the option
    /// differently from whoever rejected it.
    #[test]
    fn a_rejected_option_is_found_under_a_name_nobody_wrote() {
        let config = config(vec![decision(
            "four layers plus System",
            vec![rejected(
                "a single layer",
                "the domain would import the transport",
            )],
        )]);

        let hits = search(&config, "camada unica? single layers");
        let at: Vec<String> = hits.iter().map(|hit| hit.at.path()).collect();

        assert_eq!(at, ["title", "alternatives[0].option"], "{hits:?}");
        // And it says why, because a reader adjusts the query by seeing the
        // reason rather than by trusting a number.
        let option = hits.last().expect("the option");
        assert!(
            option.reasons.iter().any(|reason| reason.query == "single"
                && reason.candidate == "single"
                && reason.how == How::Exact),
            "{option:?}"
        );
        assert!(
            option.reasons.iter().any(|reason| reason.query == "layers"
                && reason.candidate == "layer"
                && reason.how == How::Prefix),
            "{option:?}"
        );
    }

    /// Every field, so an argument recorded only in `why_not` is reachable.
    /// The half of an ADR that stops the option being proposed again is often
    /// the argument, not the name.
    #[test]
    fn the_argument_against_an_option_is_searched_too() {
        let mut adr = decision(
            "the broker is Pub/Sub",
            vec![rejected(
                "RabbitMQ",
                "nobody here has run one in production",
            )],
        );
        adr.why = Some("the platform already bills for it".to_owned());

        let config = config(vec![adr]);

        let at: Vec<String> = search(&config, "production")
            .iter()
            .map(|hit| hit.at.path())
            .collect();
        assert_eq!(at, ["alternatives[0].why_not"]);

        let at: Vec<String> = search(&config, "platform")
            .iter()
            .map(|hit| hit.at.path())
            .collect();
        assert_eq!(at, ["why"]);
    }

    /// No ranking and no top-N: with a hundred candidates, returning eight
    /// instead of three is free, and a false negative is the failure this
    /// exists to prevent.
    #[test]
    fn every_match_is_returned_in_declaration_order() {
        let config = config(vec![
            decision("layers", vec![rejected("one layer", "it grows")]),
            decision("layer boundaries", Vec::new()),
        ]);

        let at: Vec<String> = search(&config, "layer")
            .iter()
            .map(|hit| format!("{}:{}", hit.decision.title, hit.at.path()))
            .collect();
        assert_eq!(
            at,
            [
                "layers:title",
                "layers:alternatives[0].option",
                "layer boundaries:title",
            ]
        );
    }

    /// The push half. `search` answers a person who will read what comes back,
    /// so it is recall-first; this answers inside `config doctor`, which lives
    /// in a gate, and a gate that cries wolf is one somebody turns off.
    #[test]
    fn two_decisions_saying_the_same_thing_are_paired() {
        let mut first = decision(
            "Four layers plus System",
            vec![rejected(
                "a single layer",
                "the domain would import the transport",
            )],
        );
        first.id = DecisionId::new("ADR-001").expect("valid");
        let mut second = decision(
            "Deployment shape",
            vec![rejected("one layer, single", "we tried it")],
        );
        second.id = DecisionId::new("ADR-002").expect("valid");

        let paired = config(vec![first, second]);
        let found = duplicates(&paired);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].earlier.id.as_str(), "ADR-001");
        assert_eq!(found[0].earlier_at, Where::Option(0));
        assert_eq!(found[0].later.id.as_str(), "ADR-002");
        assert_eq!(found[0].text, "one layer, single");
    }

    /// Sharing a word is not saying the same thing. Every token of the shorter
    /// phrase has to be reached -- and prose is not compared at all, because
    /// two decisions both saying "the domain would import the transport" are
    /// agreeing rather than duplicating.
    #[test]
    fn sharing_a_word_or_an_argument_is_not_a_duplicate() {
        let mut layers = decision("Four layers plus System", Vec::new());
        layers.id = DecisionId::new("ADR-001").expect("valid");
        let mut packages = decision(
            "One package per bounded context",
            vec![rejected(
                "one package",
                "the boundaries stop being enforceable",
            )],
        );
        packages.id = DecisionId::new("ADR-002").expect("valid");

        assert!(duplicates(&config(vec![layers, packages])).is_empty());

        // The same argument, word for word, under two different options.
        let mut one = decision("A", vec![rejected("RabbitMQ", "one more thing to operate")]);
        one.id = DecisionId::new("ADR-003").expect("valid");
        let mut two = decision("B", vec![rejected("Kafka", "one more thing to operate")]);
        two.id = DecisionId::new("ADR-004").expect("valid");

        assert!(duplicates(&config(vec![one, two])).is_empty());
    }

    /// The JSON both surfaces answer with. One shape, because `decisions find
    /// --format json` and the MCP `decisions_find` tool answer the same
    /// question, and two renderings of one answer are two that drift.
    #[test]
    fn the_json_carries_the_reason_and_never_a_score() {
        let config = config(vec![decision(
            "Four layers plus System",
            vec![rejected("a monolith", "we tried it in 2021")],
        )]);

        let json = similar_json(&config, "monolit");
        assert_eq!(json["query"], "monolit");
        assert_eq!(json["hits"][0]["decision"], "ADR-001");
        assert_eq!(json["hits"][0]["at"], "alternatives[0].option");
        assert_eq!(json["hits"][0]["text"], "a monolith");
        assert_eq!(json["hits"][0]["reasons"][0]["how"], "prefix");
        assert_eq!(json["hits"][0]["reasons"][0]["candidate"], "monolith");

        // The two other layers, spelled so a client can branch on them.
        assert_eq!(
            similar_json(&config, "monolith")["hits"][0]["reasons"][0]["how"],
            "exact"
        );
        // `monolitos`, the Portuguese plural, is two edits from the English
        // spelling and reaches it -- which is the case this layer exists for
        // in a repository whose decisions are written in both.
        assert_eq!(
            similar_json(&config, "monolitos")["hits"][0]["reasons"][0]["how"],
            "edits:2"
        );

        // Nothing found is an empty list, not an absent key: a client should
        // not have to tell "no answer" from "no hits".
        assert_eq!(
            similar_json(&config, "graphql")["hits"].as_array(),
            Some(&vec![])
        );
    }

    /// A query of nothing but stopwords reaches nothing, rather than reaching
    /// everything -- which is what a matcher with no tokens would do.
    #[test]
    fn a_query_with_no_tokens_finds_nothing() {
        let config = config(vec![decision("layers", Vec::new())]);

        assert!(search(&config, "the and of").is_empty());
        assert!(search(&config, "").is_empty());
    }
}
