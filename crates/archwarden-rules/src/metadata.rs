//! The `metadata` rule: a file's header must declare these keys about itself.
//!
//! The frontmatter of code. `frontmatter` asks a document to declare what it
//! is; ownership, stability and lifecycle are ordinary ADR content and are
//! properties of a **source** file that no rule could ask about. See
//! `docs/RULES.md`. Issue #104.
//!
//! # The line this rule keeps
//!
//! `frontmatter`'s, deliberately and to the letter: `require` is names,
//! `one_of` is a closed vocabulary whose members are names, `equals` is a name
//! agreeing with a path. Values compare as **text**, with no type system. Two
//! kinds asking the same question of two file formats should look the same,
//! and the document rule already settled the hard parts.
//!
//! # Where a claim may be written
//!
//! In the header — everything above the file's first statement — and nowhere
//! else, in this version. Above any export is far more useful and far more
//! work: it needs the marker bound to the declaration that follows it, which
//! is a position a suppression never has to solve because it applies to the
//! next line. What it does *not* do is ignore a marker written lower down:
//! that one is reported as misplaced, because an author who wrote
//! `archwarden-owner` and is told the file declares no owner has been given
//! the one answer they cannot act on.

use std::collections::{BTreeMap, BTreeSet};

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    facts::{FileFacts, Span},
    finding::{Expectation, Finding, Observed},
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    scope::Scope,
    template,
    traits::{FactsNeeded, FileContext, RuleEngine},
};

/// A compiled `metadata` rule.
#[derive(Debug, Clone)]
pub struct MetadataEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    require: Vec<String>,
    one_of: Vec<(String, Vec<String>)>,
    equals: Vec<(String, String)>,
    deadline: Vec<String>,
}

impl MetadataEngine {
    /// Builds an engine from a compiled rule.
    ///
    /// Returns `None` for a rule of any other kind.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::Metadata {
            require,
            one_of,
            equals,
            deadline,
        } = &rule.kind
        else {
            return None;
        };

        Some(Self::build(rule, require, one_of, equals, deadline))
    }

    /// Builds an engine from a rule whose kind is already known.
    pub(crate) fn build(
        rule: &CompiledRule,
        require: &[String],
        one_of: &[(String, Vec<String>)],
        equals: &[(String, String)],
        deadline: &[String],
    ) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            require: require.to_vec(),
            one_of: one_of.to_vec(),
            equals: equals.to_vec(),
            deadline: deadline.to_vec(),
        }
    }

    /// Whether this rule is about `path`.
    ///
    /// The scope and nothing else. A `file_pattern` would be a field decided
    /// before anybody asked for one — the argument decision 28 makes about
    /// `frozen` — and it can be added later without breaking a config.
    fn covers(&self, path: &RepoRelPath) -> bool {
        self.scope.contains_file(path.as_path())
    }

    /// The value `equals` demands here, with `{{raw(dirname)}}` rendered.
    ///
    /// The directory's own name, not its path, for the reason
    /// `frontmatter.equals` and `naming.dir_pattern` both use it: the claim is
    /// about the module a file sits in, and a template that carried the whole
    /// path would agree with nothing anybody writes by hand.
    fn rendered(template: &str, path: &RepoRelPath) -> Option<String> {
        let directory = path.parent()?.file_name()?.to_owned();

        template::render(template, |group| {
            (group == "dirname").then(|| directory.clone())
        })
        .ok()
    }

    fn expectation(&self, path: &RepoRelPath) -> Expectation {
        Expectation::DeclaredMetadata {
            keys: self.require.clone(),
            vocabularies: self.one_of.clone(),
            agreements: self
                .equals
                .iter()
                .filter_map(|(key, template)| Some((key.clone(), Self::rendered(template, path)?)))
                .collect(),
        }
    }

    fn finding(&self, path: &RepoRelPath, observed: Observed, span: Option<Span>) -> Finding {
        Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: path.clone(),
            span,
            observed,
            expected: self.expectation(path),
        }
    }

    /// Every key this rule asks a question about, in the order it asks them.
    fn keys(&self) -> Vec<&str> {
        let mut seen = BTreeSet::new();

        self.require
            .iter()
            .map(String::as_str)
            .chain(self.one_of.iter().map(|(key, _)| key.as_str()))
            .chain(self.equals.iter().map(|(key, _)| key.as_str()))
            .chain(self.deadline.iter().map(String::as_str))
            .filter(|key| seen.insert(*key))
            .collect()
    }

    /// What is wrong with what the file declares.
    ///
    /// Two passes, and the order is the point. The first asks whether each key
    /// is *settled* — declared once, in the header — and the second asks what
    /// its value says. A key declared twice or written below the header has no
    /// single value to judge, so the questions about its value are not asked:
    /// two findings for one edit is noise, and the second would have to pick a
    /// value to be about.
    fn faults(
        &self,
        path: &RepoRelPath,
        facts: &FileFacts,
        as_of: archwarden_core::date::Date,
    ) -> Vec<(Observed, Option<Span>)> {
        let mut declared: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let mut below: BTreeMap<&str, Span> = BTreeMap::new();

        for claim in &facts.metadata {
            if claim.in_header {
                declared
                    .entry(claim.key.as_str())
                    .or_default()
                    .push(claim.value.as_str());
            } else {
                below.entry(claim.key.as_str()).or_insert(claim.span);
            }
        }

        let mut faults = Vec::new();
        let mut unsettled = BTreeSet::new();

        for key in self.keys() {
            match declared.get(key) {
                Some(values) if values.len() > 1 => {
                    unsettled.insert(key);
                    faults.push((
                        Observed::MetadataDeclaredTwice {
                            key: key.to_owned(),
                            found: values.iter().map(|value| (*value).to_owned()).collect(),
                        },
                        None,
                    ));
                }
                Some(_) => {}
                // Only when the header says nothing: a claim in the header is
                // the claim, and a comment further down is a comment.
                None => {
                    if let Some(span) = below.get(key) {
                        unsettled.insert(key);
                        faults.push((
                            Observed::MetadataOutsideHeader {
                                key: key.to_owned(),
                            },
                            Some(*span),
                        ));
                    }
                }
            }
        }

        let value = |key: &str| -> Option<&str> {
            match declared.get(key)?.as_slice() {
                [only] => Some(only),
                _ => None,
            }
        };

        for key in &self.require {
            if unsettled.contains(key.as_str()) || declared.contains_key(key.as_str()) {
                continue;
            }
            faults.push((Observed::MetadataMissing { key: key.clone() }, None));
        }

        for (key, vocabulary) in &self.one_of {
            // A key that is absent is reported by `require`, if the rule asked
            // for it. Reporting it twice would be two findings for one edit.
            let Some(written) = value(key) else {
                continue;
            };
            if !vocabulary.iter().any(|allowed| allowed == written) {
                faults.push((
                    Observed::MetadataOutsideVocabulary {
                        key: key.clone(),
                        found: written.to_owned(),
                    },
                    None,
                ));
            }
        }

        // Dates last, because the two questions before it are about whether
        // there *is* a value: a key declared twice has no single date to be
        // due, and one written below the header is not read at all.
        for key in &self.deadline {
            if let Some(written) = value(key) {
                faults.extend(overdue(key, written, as_of));
            }
        }

        for (key, template) in &self.equals {
            let (Some(written), Some(wanted)) = (value(key), Self::rendered(template, path)) else {
                continue;
            };
            if written != wanted {
                faults.push((
                    Observed::MetadataDisagrees {
                        key: key.clone(),
                        found: written.to_owned(),
                        wanted,
                    },
                    None,
                ));
            }
        }

        faults
    }
}

/// What a key that should hold a date says, when it does not hold one or holds
/// one that has passed.
///
/// Its own function rather than a fourth arm inside `faults`: the three passes
/// above ask whether a value *is settled*, and this one asks what the value
/// means. Splitting on that line is what keeps each readable.
fn overdue(
    key: &str,
    written: &str,
    as_of: archwarden_core::date::Date,
) -> Option<(Observed, Option<Span>)> {
    let Some(due) = archwarden_core::date::Date::parse(written) else {
        return Some((
            Observed::MetadataNotADate {
                key: key.to_owned(),
                found: written.to_owned(),
            },
            None,
        ));
    };

    // The day it falls due is met, not missed. A rule that fired on the date
    // itself would fire a day early for everybody.
    let days = as_of.days_since(due);
    (days > 0).then(|| {
        (
            Observed::MetadataDeadlinePassed {
                key: key.to_owned(),
                was: due.to_string(),
                days,
            },
            None,
        )
    })
}

impl RuleEngine for MetadataEngine {
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
        FactsNeeded::Code
    }

    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        if !self.covers(ctx.path) {
            return Vec::new();
        }
        // No facts means no front-end read it, which the run counts and names.
        // Reporting a missing key here would be accusing a file nobody opened.
        let Some(facts) = ctx.facts else {
            return Vec::new();
        };

        self.faults(ctx.path, facts, ctx.as_of)
            .into_iter()
            .map(|(observed, span)| self.finding(ctx.path, observed, span))
            .collect()
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
    use archwarden_core::{
        facts::{FileFacts, MetadataFact, Span},
        hash::ContentHash,
        traits::Exists,
    };

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }

    fn deadline_engine(deadline: &[&str]) -> MetadataEngine {
        let rule = CompiledRule {
            id: RuleId::new("experiments-expire").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Error,
            scope: Scope::compile(["src/payments/**"]).expect("valid scope"),
            kind: CompiledRuleKind::Metadata {
                require: Vec::new(),
                one_of: Vec::new(),
                equals: Vec::new(),
                deadline: owned(deadline),
            },
        };

        MetadataEngine::from_rule(&rule).expect("is a metadata rule")
    }

    fn engine(
        require: &[&str],
        one_of: &[(&str, &[&str])],
        equals: &[(&str, &str)],
    ) -> MetadataEngine {
        let rule = CompiledRule {
            id: RuleId::new("payments-declare-an-owner").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Error,
            scope: Scope::compile(["src/payments/**"]).expect("valid scope"),
            kind: CompiledRuleKind::Metadata {
                require: owned(require),
                one_of: one_of
                    .iter()
                    .map(|(key, values)| ((*key).to_owned(), owned(values)))
                    .collect(),
                equals: equals
                    .iter()
                    .map(|(key, template)| ((*key).to_owned(), (*template).to_owned()))
                    .collect(),
                deadline: Vec::new(),
            },
        };

        MetadataEngine::from_rule(&rule).expect("is a metadata rule")
    }

    /// Facts as the JS/TS front-end would produce them, keyed by hand so a rule
    /// test does not depend on oxc.
    fn facts_with(claims: &[(&str, &str, bool)]) -> FileFacts {
        let mut facts = FileFacts::unparsed(path("src/payments/refund.ts"), ContentHash::of(b""));
        for (index, (key, value, in_header)) in claims.iter().enumerate() {
            let at = u32::try_from(index).expect("small") * 40;
            facts.metadata.push(MetadataFact {
                key: (*key).to_owned(),
                value: (*value).to_owned(),
                in_header: *in_header,
                span: Span::new(at, at + 30),
            });
        }
        facts
    }

    fn header(claims: &[(&str, &str)]) -> FileFacts {
        facts_with(
            &claims
                .iter()
                .map(|(key, value)| (*key, *value, true))
                .collect::<Vec<_>>(),
        )
    }

    fn check(engine: &MetadataEngine, facts: &FileFacts) -> Vec<Finding> {
        engine.check_file(FileContext {
            path: &facts.path,
            facts: Some(facts),
            docs: None,
            siblings: &[],
            exists: Exists::none(),
            graph: None,
            as_of: archwarden_core::date::Date::EPOCH,
        })
    }

    fn observed(engine: &MetadataEngine, facts: &FileFacts) -> Vec<Observed> {
        check(engine, facts)
            .into_iter()
            .map(|finding| finding.observed)
            .collect()
    }

    /// The line the issue was filed with: every file under `payments/`
    /// declares an owner.
    #[test]
    fn a_header_carrying_every_required_key_passes() {
        let engine = engine(&["owner", "stability"], &[], &[]);
        let facts = header(&[("owner", "payments-team"), ("stability", "stable")]);

        assert!(check(&engine, &facts).is_empty());
    }

    /// A missing key is named on its own, so the fix is one key rather than
    /// "write a header".
    #[test]
    fn a_missing_key_is_named_on_its_own() {
        let engine = engine(&["owner", "stability"], &[], &[]);

        assert_eq!(
            observed(&engine, &header(&[("owner", "payments-team")])),
            [Observed::MetadataMissing {
                key: "stability".to_owned()
            }]
        );
    }

    /// A file that declares nothing reports one fault per key asked of it, and
    /// not one fault about the absent header: there is no block to be absent.
    #[test]
    fn a_file_that_declares_nothing_reports_every_key() {
        let engine = engine(&["owner", "stability"], &[], &[]);

        assert_eq!(
            observed(&engine, &header(&[])),
            [
                Observed::MetadataMissing {
                    key: "owner".to_owned()
                },
                Observed::MetadataMissing {
                    key: "stability".to_owned()
                },
            ]
        );
    }

    /// The confidently-wrong case, `frontmatter`'s argument asked of code: a
    /// value outside the vocabulary is worse than an absent one, because
    /// whatever reads it gets an answer that means nothing.
    #[test]
    fn a_value_outside_the_vocabulary_is_reported_with_what_was_written() {
        let engine = engine(
            &[],
            &[("stability", &["stable", "experimental", "deprecated"])],
            &[],
        );

        assert_eq!(
            observed(&engine, &header(&[("stability", "wip")])),
            [Observed::MetadataOutsideVocabulary {
                key: "stability".to_owned(),
                found: "wip".to_owned(),
            }]
        );
    }

    /// A key `one_of` asks about and `require` does not is not reported twice:
    /// its absence is one edit, and two findings for one edit is noise. The
    /// argument `frontmatter` already makes.
    #[test]
    fn a_vocabulary_says_nothing_about_a_key_that_is_not_there() {
        let engine = engine(&[], &[("stability", &["stable"])], &[]);

        assert!(check(&engine, &header(&[])).is_empty());
    }

    /// `naming`'s question — a name agreeing with a path — asked of a file
    /// through what it says about itself rather than what it exports.
    #[test]
    fn a_key_can_be_pinned_to_the_directory_name() {
        let engine = engine(&[], &[], &[("module", "{{raw(dirname)}}")]);

        assert!(check(&engine, &header(&[("module", "payments")])).is_empty());

        assert_eq!(
            observed(&engine, &header(&[("module", "billing")])),
            [Observed::MetadataDisagrees {
                key: "module".to_owned(),
                found: "billing".to_owned(),
                wanted: "payments".to_owned(),
            }]
        );
    }

    /// The decision the milestone turned on: a marker below the first
    /// statement is reported as misplaced, never as absent. Saying "this file
    /// declares no owner" about a file with `archwarden-owner` written in it
    /// is the confidently-wrong failure arriving from the other direction.
    #[test]
    fn a_key_declared_below_the_header_is_misplaced_rather_than_missing() {
        let engine = engine(&["owner"], &[], &[]);
        let facts = facts_with(&[("owner", "payments-team", false)]);

        assert_eq!(
            observed(&engine, &facts),
            [Observed::MetadataOutsideHeader {
                key: "owner".to_owned()
            }]
        );
    }

    /// And it points at the marker, because "somewhere in this file" is not
    /// an instruction anybody can follow.
    #[test]
    fn a_misplaced_key_is_reported_where_it_was_written() {
        let engine = engine(&["owner"], &[], &[]);
        let facts = facts_with(&[("stability", "stable", true), ("owner", "team", false)]);

        assert_eq!(
            check(&engine, &facts).first().expect("one").span,
            Some(Span::new(40, 70))
        );
    }

    /// A misplaced key is reported by a rule that only asks about its value.
    /// An absent key means a file that does not participate; a misplaced one
    /// means a file that tried, and the vocabulary check silently did not run.
    #[test]
    fn a_misplaced_key_is_reported_even_where_absence_would_be_silent() {
        let engine = engine(&[], &[("stability", &["stable"])], &[]);
        let facts = facts_with(&[("stability", "stable", false)]);

        assert_eq!(
            observed(&engine, &facts),
            [Observed::MetadataOutsideHeader {
                key: "stability".to_owned()
            }]
        );
    }

    /// A key the rule never asks about is nobody's business, wherever it sits.
    /// The rule reports the claims it is about, not every comment in the file.
    #[test]
    fn a_key_no_rule_asks_about_is_left_alone() {
        let engine = engine(&["owner"], &[], &[]);
        let facts = facts_with(&[
            ("owner", "payments-team", true),
            ("ticket", "PAY-41", false),
        ]);

        assert!(check(&engine, &facts).is_empty());
    }

    /// Two claims about one thing, reported rather than resolved. Picking a
    /// winner in silence makes which one wins something an author has to know
    /// by heart, and hides the correction behind the line it replaced.
    #[test]
    fn a_key_declared_twice_is_reported_with_both_values() {
        let engine = engine(&["owner"], &[], &[]);
        let facts = header(&[("owner", "payments-team"), ("owner", "billing-team")]);

        assert_eq!(
            observed(&engine, &facts),
            [Observed::MetadataDeclaredTwice {
                key: "owner".to_owned(),
                found: owned(&["payments-team", "billing-team"]),
            }]
        );
    }

    /// And the questions about its value wait: which value the vocabulary
    /// would judge is exactly what has not been settled.
    #[test]
    fn a_doubled_key_is_not_also_judged_against_its_vocabulary() {
        let engine = engine(&["stability"], &[("stability", &["stable"])], &[]);
        let facts = header(&[("stability", "stable"), ("stability", "wip")]);

        assert_eq!(
            observed(&engine, &facts),
            [Observed::MetadataDeclaredTwice {
                key: "stability".to_owned(),
                found: owned(&["stable", "wip"]),
            }]
        );
    }

    /// A key doubled below the header is misplaced, and the doubling is not
    /// the interesting half: moving it up is the fix either way.
    #[test]
    fn a_key_doubled_below_the_header_is_reported_as_misplaced() {
        let engine = engine(&["owner"], &[], &[]);
        let facts = facts_with(&[("owner", "a", false), ("owner", "b", false)]);

        assert_eq!(
            observed(&engine, &facts),
            [Observed::MetadataOutsideHeader {
                key: "owner".to_owned()
            }]
        );
    }

    /// A key in the header and again below it is settled: the header is where
    /// claims are read from, and the one underneath is a comment.
    #[test]
    fn a_key_in_the_header_is_not_disturbed_by_one_below_it() {
        let engine = engine(&["owner"], &[], &[]);
        let facts = facts_with(&[("owner", "payments-team", true), ("owner", "other", false)]);

        assert!(check(&engine, &facts).is_empty());
    }

    fn on(day: &str, engine: &MetadataEngine, facts: &FileFacts) -> Vec<Observed> {
        engine
            .check_file(FileContext {
                path: &facts.path,
                facts: Some(facts),
                docs: None,
                siblings: &[],
                exists: Exists::none(),
                graph: None,
                as_of: archwarden_core::date::Date::parse(day).expect("a date"),
            })
            .into_iter()
            .map(|finding| finding.observed)
            .collect()
    }

    /// Issue #117. `metadata` could record a removal date and nothing compared
    /// it to anything — the difference between a migration and a wish.
    #[test]
    fn a_deadline_that_has_passed_is_reported_with_how_long_ago() {
        let engine = deadline_engine(&["remove-by"]);
        let facts = header(&[("remove-by", "2026-12-01")]);

        assert_eq!(
            on("2027-01-15", &engine, &facts),
            [Observed::MetadataDeadlinePassed {
                key: "remove-by".to_owned(),
                was: "2026-12-01".to_owned(),
                days: 45,
            }]
        );
    }

    /// The day it falls due is not yet past. A deadline of *today* is met, and
    /// a rule that fired on it would fire a day early for everybody.
    #[test]
    fn a_deadline_is_met_on_the_day_itself_and_before_it() {
        let engine = deadline_engine(&["remove-by"]);
        let facts = header(&[("remove-by", "2026-12-01")]);

        assert!(
            on("2026-12-01", &engine, &facts).is_empty(),
            "the day itself"
        );
        assert!(on("2026-11-30", &engine, &facts).is_empty(), "and before");
        assert_eq!(on("2026-12-02", &engine, &facts).len(), 1, "and after");
    }

    /// A value that is not a date is its own finding rather than a guess.
    /// `01/12/2026` read as a date would put the deadline eleven months out.
    #[test]
    fn a_value_that_is_not_a_date_says_so_rather_than_being_guessed_at() {
        let engine = deadline_engine(&["remove-by"]);

        assert_eq!(
            on(
                "2027-01-15",
                &engine,
                &header(&[("remove-by", "01/12/2026")])
            ),
            [Observed::MetadataNotADate {
                key: "remove-by".to_owned(),
                found: "01/12/2026".to_owned(),
            }]
        );
    }

    /// A key nobody declared is `require`'s to report, exactly as `one_of`
    /// already decides it: two findings for one edit is noise.
    #[test]
    fn a_deadline_says_nothing_about_a_key_that_is_not_there() {
        assert!(on("2027-01-15", &deadline_engine(&["remove-by"]), &header(&[])).is_empty());
    }

    /// And a doubled key is not judged, for the reason a doubled key is never
    /// judged: which of the two dates would be the deadline is exactly what
    /// has not been settled.
    #[test]
    fn a_doubled_deadline_is_not_compared_to_anything() {
        let facts = header(&[("remove-by", "2020-01-01"), ("remove-by", "2030-01-01")]);

        assert_eq!(
            on("2027-01-15", &deadline_engine(&["remove-by"]), &facts),
            [Observed::MetadataDeclaredTwice {
                key: "remove-by".to_owned(),
                found: owned(&["2020-01-01", "2030-01-01"]),
            }]
        );
    }

    /// Without facts nobody opened the file, and the run counts that.
    /// Reporting a missing key here would be accusing a file nobody read.
    #[test]
    fn a_file_that_was_never_parsed_is_not_accused() {
        let engine = engine(&["owner"], &[], &[]);
        let target = path("src/payments/refund.ts");

        let findings = engine.check_file(FileContext {
            path: &target,
            facts: None,
            docs: None,
            siblings: &[],
            exists: Exists::none(),
            graph: None,
            as_of: archwarden_core::date::Date::EPOCH,
        });

        assert!(findings.is_empty());
        assert!(engine.applies_to(&target), "the rule does apply to it");
    }

    #[test]
    fn a_file_outside_the_scope_is_left_alone() {
        let engine = engine(&["owner"], &[], &[]);
        let mut facts = header(&[]);
        facts.path = path("src/billing/invoice.ts");

        assert!(check(&engine, &facts).is_empty());
        assert!(!engine.applies_to(&facts.path));
    }

    /// It reads code, and says so — which is what keeps a `.md` under this
    /// rule from being counted as a check nobody could make.
    #[test]
    fn it_asks_for_code_facts() {
        assert_eq!(
            engine(&["owner"], &[], &[]).needs_facts(),
            FactsNeeded::Code
        );
    }

    /// Decision 9: what `check` demands is what `scaffold` advertises, with
    /// the template rendered — showing a user `{{raw(dirname)}}` is showing
    /// them our internals.
    #[test]
    fn the_contract_is_describable_before_the_file_exists() {
        let engine = engine(
            &["owner"],
            &[("stability", &["stable", "experimental"])],
            &[("module", "{{raw(dirname)}}")],
        );

        assert_eq!(
            engine.describe_expectation(&path("src/payments/refund.ts")),
            [Expectation::DeclaredMetadata {
                keys: owned(&["owner"]),
                vocabularies: vec![("stability".to_owned(), owned(&["stable", "experimental"]))],
                agreements: vec![("module".to_owned(), "payments".to_owned())],
            }]
        );
    }

    #[test]
    fn a_rule_of_another_kind_is_declined() {
        let frozen = CompiledRule {
            id: RuleId::new("legacy-frozen").expect("valid"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Error,
            scope: Scope::compile(["src/payments/**"]).expect("valid"),
            kind: CompiledRuleKind::Frozen,
        };

        assert!(MetadataEngine::from_rule(&frozen).is_none());
    }

    /// A rule declared inside a module carries it, which is what puts its
    /// findings under that module in the report and lets the module's own `why`
    /// travel with them.
    #[test]
    fn the_engine_reports_its_identity() {
        let engine = engine(&["owner"], &[], &[]);

        assert_eq!(engine.id().as_str(), "payments-declare-an-owner");
        assert_eq!(engine.module(), None);
        assert_eq!(engine.level(), Level::Error);

        let in_a_module = CompiledRule {
            id: RuleId::new("payments-declare-an-owner").expect("valid id"),
            module: Some(ModuleId::new("payments").expect("valid module")),
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Warning,
            scope: Scope::compile(["src/payments/**"]).expect("valid scope"),
            kind: CompiledRuleKind::Metadata {
                require: owned(&["owner"]),
                one_of: Vec::new(),
                equals: Vec::new(),
                deadline: Vec::new(),
            },
        };

        let owned_by = MetadataEngine::from_rule(&in_a_module).expect("is a metadata rule");
        assert_eq!(owned_by.module().map(ModuleId::as_str), Some("payments"));
        assert_eq!(owned_by.level(), Level::Warning);
    }
}
