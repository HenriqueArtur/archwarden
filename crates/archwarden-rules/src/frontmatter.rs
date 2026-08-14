//! The `frontmatter` rule: a document's YAML block must carry these keys.
//!
//! The first rule that reads a file which is not code, and the reason there is
//! a second front-end. See `docs/RULES.md`.
//!
//! # The line this rule keeps
//!
//! It asserts **names and vocabularies, never the shape of a value.** `require`
//! is names. `equals` is a name agreeing with a path, which is the `naming`
//! rule's question asked of a file with no exported symbol to ask it about.
//! `one_of` is a closed vocabulary, whose members are themselves names.
//!
//! What is deliberately absent is `type` and `min_items`. Those are the shape
//! of a value, they are the first two rungs of a ladder with no natural stop,
//! and JSON Schema is already at the top of it. Every other rule here keeps the
//! same line — `must_export.annotation` asserts that a token is *present*, not
//! what the type means.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    docs::{DocValue, Frontmatter},
    finding::{Expectation, Finding, Observed},
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    pattern::Pattern,
    scope::Scope,
    template,
    traits::{FactsNeeded, FileContext, RuleEngine},
};

/// A compiled `frontmatter` rule.
#[derive(Debug, Clone)]
pub struct FrontmatterEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    file_pattern: Pattern,
    require: Vec<String>,
    one_of: Vec<(String, Vec<String>)>,
    equals: Vec<(String, String)>,
}

impl FrontmatterEngine {
    /// Builds an engine from a compiled rule.
    ///
    /// Returns `None` for a rule of any other kind.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::Frontmatter {
            file_pattern,
            require,
            one_of,
            equals,
        } = &rule.kind
        else {
            return None;
        };

        Some(Self::build(rule, file_pattern, require, one_of, equals))
    }

    /// Builds an engine from a rule whose kind is already known.
    pub(crate) fn build(
        rule: &CompiledRule,
        file_pattern: &Pattern,
        require: &[String],
        one_of: &[(String, Vec<String>)],
        equals: &[(String, String)],
    ) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            file_pattern: file_pattern.clone(),
            require: require.to_vec(),
            one_of: one_of.to_vec(),
            equals: equals.to_vec(),
        }
    }

    /// Whether this rule is about `path`.
    fn covers(&self, path: &RepoRelPath) -> bool {
        self.scope.contains_file(path.as_path())
            && path
                .file_name()
                .is_some_and(|name| self.file_pattern.is_match(name))
    }

    /// The value `equals` demands here, with `{{raw(dirname)}}` rendered.
    ///
    /// The directory's own name, not its path: what a document says its `id`
    /// is agrees with `03-semaforo`, never with `projetos/03-semaforo`. Same
    /// reasoning as `naming.dir_pattern`, which matches the last segment.
    fn rendered(template: &str, path: &RepoRelPath) -> Option<String> {
        let directory = path.parent()?.file_name()?.to_owned();

        template::render(template, |group| {
            (group == "dirname").then(|| directory.clone())
        })
        .ok()
    }

    fn expectation(&self, path: &RepoRelPath) -> Expectation {
        Expectation::RequiredFrontmatter {
            keys: self.require.clone(),
            vocabularies: self.one_of.clone(),
            agreements: self
                .equals
                .iter()
                .filter_map(|(key, template)| Some((key.clone(), Self::rendered(template, path)?)))
                .collect(),
        }
    }

    fn finding(&self, path: &RepoRelPath, observed: Observed) -> Finding {
        Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: path.clone(),
            // The block has a position in the file, and reporting it would be
            // better. `DocFacts` does not carry one yet: the fence extractor
            // returns text, not a span, and inventing an offset here would be
            // pointing at a place nobody measured.
            span: None,
            observed,
            expected: self.expectation(path),
        }
    }

    /// What is wrong with a block that is there and parsed.
    fn faults(
        &self,
        path: &RepoRelPath,
        keys: &std::collections::BTreeMap<String, DocValue>,
    ) -> Vec<Observed> {
        let mut faults = Vec::new();

        for key in &self.require {
            if !keys.contains_key(key) {
                faults.push(Observed::FrontmatterKeyMissing { key: key.clone() });
            }
        }

        for (key, vocabulary) in &self.one_of {
            // A key that is absent is reported by `require`, if the rule asked
            // for it. Reporting it twice would be two findings for one edit.
            let Some(value) = keys.get(key) else {
                continue;
            };
            let DocValue::Scalar(written) = value else {
                faults.push(Observed::FrontmatterValueNotScalar { key: key.clone() });
                continue;
            };
            if !vocabulary.contains(written) {
                faults.push(Observed::FrontmatterValueOutsideVocabulary {
                    key: key.clone(),
                    found: written.clone(),
                });
            }
        }

        for (key, template) in &self.equals {
            let (Some(value), Some(wanted)) = (keys.get(key), Self::rendered(template, path))
            else {
                continue;
            };
            let DocValue::Scalar(written) = value else {
                faults.push(Observed::FrontmatterValueNotScalar { key: key.clone() });
                continue;
            };
            if written != &wanted {
                faults.push(Observed::FrontmatterValueDisagrees {
                    key: key.clone(),
                    found: written.clone(),
                    wanted,
                });
            }
        }

        faults
    }
}

impl RuleEngine for FrontmatterEngine {
    fn id(&self) -> &RuleId {
        &self.id
    }

    fn module(&self) -> Option<&ModuleId> {
        self.module.as_ref()
    }

    fn level(&self) -> Level {
        self.level
    }

    fn applies_to(&self, path: &RepoRelPath) -> bool {
        self.covers(path)
    }

    fn needs_facts(&self) -> FactsNeeded {
        FactsNeeded::Document
    }

    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        if !self.covers(ctx.path) {
            return Vec::new();
        }
        // No document facts means no front-end read it, which the run counts
        // and names. Reporting a missing key here would be accusing a file
        // nobody opened.
        let Some(docs) = ctx.docs else {
            return Vec::new();
        };

        match &docs.frontmatter {
            // A file with no block is a finding, not a skip. Skipping would
            // make *deleting the block* the way out of the rule, which is the
            // argument `skip_type_only` already makes about deleting the
            // `export` keyword.
            Frontmatter::Absent => vec![self.finding(ctx.path, Observed::FrontmatterAbsent)],
            Frontmatter::Malformed { reason } => vec![self.finding(
                ctx.path,
                Observed::FrontmatterMalformed {
                    reason: reason.clone(),
                },
            )],
            Frontmatter::Present(keys) => self
                .faults(ctx.path, keys)
                .into_iter()
                .map(|observed| self.finding(ctx.path, observed))
                .collect(),
            // `Frontmatter` is non_exhaustive; a state added later is one this
            // rule has no question for yet, and inventing a finding about it
            // would be accusing a document of something nobody defined.
            _ => Vec::new(),
        }
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        if !self.covers(path) {
            return Vec::new();
        }
        vec![self.expectation(path)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{docs::DocFacts, hash::ContentHash, traits::Exists};

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }

    fn engine(
        require: &[&str],
        one_of: &[(&str, &[&str])],
        equals: &[(&str, &str)],
    ) -> FrontmatterEngine {
        let rule = CompiledRule {
            id: RuleId::new("projeto-frontmatter").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["projetos/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Frontmatter {
                file_pattern: Pattern::compile(r"^projeto\.md$").expect("valid pattern"),
                require: owned(require),
                one_of: one_of
                    .iter()
                    .map(|(key, values)| ((*key).to_owned(), owned(values)))
                    .collect(),
                equals: equals
                    .iter()
                    .map(|(key, template)| ((*key).to_owned(), (*template).to_owned()))
                    .collect(),
            },
        };

        FrontmatterEngine::from_rule(&rule).expect("is a frontmatter rule")
    }

    /// Facts as the document front-end would produce them, keyed by hand so a
    /// rule test does not depend on the YAML crate.
    fn docs_with(frontmatter: Frontmatter) -> DocFacts {
        DocFacts {
            path: path("projetos/03-semaforo/projeto.md"),
            content_hash: ContentHash::of(b""),
            frontmatter,
            headings: Vec::new(),
        }
    }

    fn present(pairs: &[(&str, DocValue)]) -> DocFacts {
        docs_with(Frontmatter::Present(
            pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        ))
    }

    fn scalar(text: &str) -> DocValue {
        DocValue::Scalar(text.to_owned())
    }

    fn check(engine: &FrontmatterEngine, docs: &DocFacts) -> Vec<Finding> {
        engine.check_file(FileContext {
            path: &docs.path,
            facts: None,
            docs: Some(docs),
            siblings: &[],
            exists: Exists::none(),
            graph: None,
        })
    }

    /// The block issue #44 was filed with, complete.
    #[test]
    fn a_block_carrying_every_required_key_passes() {
        let engine = engine(&["id", "nivel", "componentes"], &[], &[]);
        let docs = present(&[
            ("id", scalar("03-semaforo")),
            ("nivel", scalar("1")),
            ("componentes", DocValue::List),
        ]);

        assert!(check(&engine, &docs).is_empty());
    }

    /// A `projeto.md` with no `componentes` does not fail to load. It reports
    /// as a lesson that needs no components, so "which projects use the DHT11?"
    /// returns an answer that is confidently short.
    #[test]
    fn a_missing_key_is_named_on_its_own() {
        let engine = engine(&["id", "nivel", "componentes"], &[], &[]);
        let docs = present(&[("id", scalar("03-semaforo"))]);

        let findings = check(&engine, &docs);
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.observed.clone())
                .collect::<Vec<_>>(),
            [
                Observed::FrontmatterKeyMissing {
                    key: "nivel".to_owned()
                },
                Observed::FrontmatterKeyMissing {
                    key: "componentes".to_owned()
                },
            ]
        );
    }

    /// The case that justifies the rule. `status: concluido` where the
    /// vocabulary is `feito` drops the lesson out of the progress table with no
    /// row and no error — confidently wrong rather than merely absent.
    #[test]
    fn a_value_outside_the_vocabulary_is_reported_with_what_was_written() {
        let engine = engine(&[], &[("status", &["feito", "fazendo", "parado"])], &[]);
        let docs = present(&[("status", scalar("concluido"))]);

        let findings = check(&engine, &docs);
        assert_eq!(
            findings.first().expect("one").observed,
            Observed::FrontmatterValueOutsideVocabulary {
                key: "status".to_owned(),
                found: "concluido".to_owned(),
            }
        );
    }

    /// A number in the document and a number in the config are one question in
    /// two notations.
    #[test]
    fn a_vocabulary_compares_as_text_whatever_yaml_called_the_value() {
        let engine = engine(&[], &[("nivel", &["1", "2", "3"])], &[]);

        assert!(check(&engine, &present(&[("nivel", scalar("2"))])).is_empty());
        assert_eq!(check(&engine, &present(&[("nivel", scalar("9"))])).len(), 1);
    }

    /// A key `one_of` asks about and `require` does not is not reported twice:
    /// its absence is one edit, and two findings for one edit is noise.
    #[test]
    fn a_vocabulary_says_nothing_about_a_key_that_is_not_there() {
        let engine = engine(&[], &[("status", &["feito"])], &[]);

        assert!(check(&engine, &present(&[])).is_empty());
    }

    /// The `naming` rule's question, asked of a file with no exported symbol:
    /// a name agreeing with a path.
    #[test]
    fn a_key_can_be_pinned_to_the_directory_name() {
        let engine = engine(&[], &[], &[("id", "{{raw(dirname)}}")]);

        assert!(check(&engine, &present(&[("id", scalar("03-semaforo"))])).is_empty());

        let findings = check(&engine, &present(&[("id", scalar("semaforo"))]));
        assert_eq!(
            findings.first().expect("one").observed,
            Observed::FrontmatterValueDisagrees {
                key: "id".to_owned(),
                found: "semaforo".to_owned(),
                wanted: "03-semaforo".to_owned(),
            }
        );
    }

    /// A list where a scalar was asked about is not "outside the vocabulary" —
    /// there is no value to compare. Saying so is the difference between "fix
    /// the value" and "you wrote a list here".
    #[test]
    fn a_non_scalar_where_a_value_was_asked_about_says_so() {
        let engine = engine(&[], &[("nivel", &["1"])], &[]);

        assert_eq!(
            check(&engine, &present(&[("nivel", DocValue::List)]))
                .first()
                .expect("one")
                .observed,
            Observed::FrontmatterValueNotScalar {
                key: "nivel".to_owned()
            }
        );
    }

    /// A file with no block is a finding, not a skip. Skipping would make
    /// deleting the block the way out of the rule — the argument
    /// `skip_type_only` already makes about deleting the `export` keyword.
    #[test]
    fn a_document_with_no_block_is_reported_rather_than_skipped() {
        let engine = engine(&["id"], &[], &[]);

        assert_eq!(
            check(&engine, &docs_with(Frontmatter::Absent))
                .first()
                .expect("one")
                .observed,
            Observed::FrontmatterAbsent
        );
    }

    /// Malformed is its own finding: "write the block" and "the block you wrote
    /// is not YAML" are different next steps.
    #[test]
    fn a_block_that_is_not_yaml_is_its_own_finding() {
        let engine = engine(&["id"], &[], &[]);
        let docs = docs_with(Frontmatter::Malformed {
            reason: "mapping values are not allowed in this context".to_owned(),
        });

        assert_eq!(
            check(&engine, &docs).first().expect("one").observed,
            Observed::FrontmatterMalformed {
                reason: "mapping values are not allowed in this context".to_owned()
            }
        );
    }

    /// Without document facts nobody opened the file, and the run counts that.
    /// Reporting a missing key here would be accusing a file nobody read.
    #[test]
    fn a_document_that_was_never_read_is_not_accused() {
        let engine = engine(&["id"], &[], &[]);
        let target = path("projetos/03-semaforo/projeto.md");

        let findings = engine.check_file(FileContext {
            path: &target,
            facts: None,
            docs: None,
            siblings: &[],
            exists: Exists::none(),
            graph: None,
        });

        assert!(findings.is_empty());
        assert!(engine.applies_to(&target), "the rule does apply to it");
    }

    #[test]
    fn a_document_the_pattern_does_not_match_is_left_alone() {
        let engine = engine(&["id"], &[], &[]);
        let docs = DocFacts {
            path: path("projetos/03-semaforo/notas.md"),
            ..docs_with(Frontmatter::Absent)
        };

        assert!(check(&engine, &docs).is_empty());
    }

    /// It reads a document, and says so — which is what keeps a `.py` under
    /// this rule from being counted as a check nobody could make.
    #[test]
    fn it_asks_for_document_facts() {
        assert_eq!(
            engine(&["id"], &[], &[]).needs_facts(),
            FactsNeeded::Document
        );
    }

    /// Decision 9: what `check` demands is what `scaffold` advertises, with the
    /// template rendered — showing a user `{{raw(dirname)}}` is showing them our
    /// internals.
    #[test]
    fn the_contract_is_describable_before_the_document_exists() {
        let engine = engine(
            &["id"],
            &[("nivel", &["1", "2"])],
            &[("id", "{{raw(dirname)}}")],
        );

        assert_eq!(
            engine.describe_expectation(&path("projetos/17-nova/projeto.md")),
            [Expectation::RequiredFrontmatter {
                keys: owned(&["id"]),
                vocabularies: vec![("nivel".to_owned(), owned(&["1", "2"]))],
                agreements: vec![("id".to_owned(), "17-nova".to_owned())],
            }]
        );
    }

    #[test]
    fn a_rule_of_another_kind_is_declined() {
        let presence = CompiledRule {
            id: RuleId::new("licao-completa").expect("valid"),
            module: None,
            why: None,
            module_why: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["projetos/*"]).expect("valid"),
            kind: CompiledRuleKind::Presence {
                require: vec!["projeto.md".to_owned()],
                require_any: Vec::new(),
            },
        };

        assert!(FrontmatterEngine::from_rule(&presence).is_none());
    }

    #[test]
    fn the_engine_reports_its_identity() {
        let engine = engine(&["id"], &[], &[]);
        assert_eq!(engine.id().as_str(), "projeto-frontmatter");
        assert_eq!(engine.module(), None);
        assert_eq!(engine.level(), Level::Error);
    }
}
