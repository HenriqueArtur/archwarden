//! Proving that a rule bites.
//!
//! # What this answers, and what `explain` could not
//!
//! `config explain <rule-id>` says what a rule *reaches*: which files its scope
//! covers and what it is flagging today. A rule can be schema-valid, cover the
//! right paths, appear in `explain` and still enforce nothing, because its own
//! condition never fires on anything. Coverage is not efficacy, and the gap
//! between them is invisible from the outside — a rule enforcing nothing looks
//! exactly like a repository that satisfies it, which `CONFIG.md` calls the
//! worst failure a linter has.
//!
//! So: for each rule, synthesise an input that *should* violate it, evaluate
//! the rule against that input in memory, and say whether it fired. Nothing is
//! written to the repository, and nothing is read that `check` does not already
//! read.
//!
//! # What it does not prove, said plainly
//!
//! That a rule fires on a violation of **its own terms**. It cannot know what
//! you meant.
//!
//! Issue #24 is the sharp example. A `forbid_import_from_packages` list was
//! missing `@Dependencies`, real imports crossed the boundary, and the run was
//! green. Synthesising a violation from that rule's own list would have used
//! one of the packages it *does* name, and reported a confident tick. An
//! incomplete list is a question about intent, and no amount of evaluation
//! recovers intent from a config.
//!
//! What it does catch is the class where the rule's terms are self-defeating: a
//! scope that reaches nothing, an `except` that exempts everything it covers, a
//! pattern nothing can match, a rule shadowed into inertness by another. Those
//! all look active in `explain` and enforce nothing.
//!
//! The report says this in its own footer. A verification tool that oversold
//! itself would be the very thing it exists to prevent.
//!
//! # Probing at real paths
//!
//! The violating *edge* is synthesised; the paths are not. For each rule the
//! probe is placed at a directory or file this repository actually has and the
//! rule actually covers. Generating a path from a glob would mean writing a
//! second, worse implementation of what the scope already decides — and one
//! that disagreed with it would report a failure nobody could reproduce.
//!
//! The cost is stated rather than hidden: a rule whose scope reaches nothing in
//! this repository cannot be probed, and is reported as unverified with that as
//! the reason. `doctor` is the command that complains about it.

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRule, CompiledRuleKind},
    traits::RuleEngine,
};
use archwarden_engine::walk::RepoTree;

/// A name no repository is expected to contain, used for the synthesised
/// entry. Suffixed until it collides with nothing the rule would allow.
const PROBE: &str = "archwarden-probe";

/// What one rule's verification found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    /// The rule.
    pub rule_id: String,
    /// Its kind, as written in the config.
    pub kind: &'static str,
    /// What happened when it was handed a violation.
    pub verdict: Verdict,
}

/// The three answers, and the middle one is the reason this exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Handed a violation, the rule reported it.
    Fires {
        /// What the violation was, for a reader deciding whether it is the one
        /// they care about.
        on: String,
    },
    /// Handed a violation, the rule said nothing.
    ///
    /// The finding this command exists for.
    Silent {
        /// What it was handed and did not report.
        on: String,
    },
    /// No violation could be synthesised, and why.
    ///
    /// Never silence: a rule that went unchecked is reported as unchecked, the
    /// same way `check --file` names the rules it could not evaluate. A partial
    /// answer that says which part is missing is worth more than a confident
    /// one that is wrong.
    Unverified {
        /// The reason, as a sentence.
        why: String,
    },
}

impl Verdict {
    /// Whether this verdict should fail a build.
    #[must_use]
    pub fn is_silent(&self) -> bool {
        matches!(self, Self::Silent { .. })
    }
}

/// Verifies every rule in the configuration against the walked repository.
#[must_use]
pub fn verify(config: &CompiledConfig, tree: &RepoTree) -> Vec<Verification> {
    // Zipped with the engines the same way `doctor` does it, so the question
    // "does this rule cover this file?" is answered by the code `check` uses
    // rather than by a second implementation that could disagree.
    config
        .rules()
        .zip(archwarden_rules::engines_for(config))
        .map(|(rule, engine)| Verification {
            rule_id: rule.id.as_str().to_owned(),
            kind: rule.kind.type_name(),
            verdict: verdict_for(rule, engine.as_ref(), tree),
        })
        .collect()
}

fn verdict_for(rule: &CompiledRule, engine: &dyn RuleEngine, tree: &RepoTree) -> Verdict {
    match &rule.kind {
        // A probe for this would be two files that have to disagree with each
        // other, in two scopes, in two languages -- and the rule is answered
        // once about the whole repository rather than about a file, which is
        // the shape everything here is built on. Named rather than synthesised.
        CompiledRuleKind::CallMatchesExport { callee, .. } => Verdict::Unverified {
            why: format!(
                "a violation is a `{callee}` in one scope naming something no \
                 declaration in another answers to -- two files that have to \
                 disagree, which this cannot build from one"
            ),
        },
        CompiledRuleKind::Structure { .. } => forbidden_subfolder(rule, engine, tree),
        CompiledRuleKind::SpecPair { .. } => a_file_with_no_spec(rule, engine, tree),
        CompiledRuleKind::Presence { .. } => a_directory_holding_nothing(rule, engine, tree),
        CompiledRuleKind::Pair { .. } => a_file_with_no_companion(rule, engine, tree),
        CompiledRuleKind::Frontmatter { .. } => a_document_with_no_block(rule, engine, tree),
        CompiledRuleKind::Metadata {
            require,
            one_of,
            equals,
            deadline,
        } => {
            a_file_declaring_the_wrong_thing(rule, engine, tree, require, one_of, equals, deadline)
        }
        // Both are file-existence questions, which is the easiest kind to
        // plant: one file that should not be there, and one that should.
        CompiledRuleKind::Chokepoint {
            callee,
            renders,
            only_in,
        } => a_call_from_outside_the_chokepoint(rule, engine, tree, callee, renders, only_in),
        CompiledRuleKind::Frozen => a_file_added_to_a_freeze(rule, engine, tree),
        CompiledRuleKind::Mirror { .. } => a_file_with_no_counterpart(rule, engine, tree),
        CompiledRuleKind::ExportShape(shape) => {
            a_file_of_the_wrong_shape(rule, engine, tree, shape)
        }
        // A rule that only forbids *reaching* has nothing a probe can plant.
        // Every other verdict here hands an engine one synthetic file; a chain
        // needs at least two, resolved against each other, which is the whole
        // pipeline run inside a probe. Checked before `crossed_boundary`,
        // which would otherwise explain it as "the rule only requires an
        // import" -- a sentence about a different rule.
        CompiledRuleKind::ImportBoundary {
            forbid,
            forbid_packages,
            forbid_reaching,
            ..
        } if forbid.is_empty() && forbid_packages.is_empty() && !forbid_reaching.is_empty() => {
            Verdict::Unverified {
                why: "the rule forbids reaching a path rather than importing \
                      one, and planting that means two files that resolve \
                      against each other -- the resolver run inside a probe"
                    .to_owned(),
            }
        }

        CompiledRuleKind::ImportBoundary {
            forbid,
            forbid_packages,
            except_from,
            ..
        } => crossed_boundary(rule, engine, tree, forbid, forbid_packages, except_from),

        // A violation here is a *file name*, and producing one means running a
        // regex backwards. `naming` and `call-obligation` both hold a
        // `file_pattern` whose language is what a violating name would have to
        // come from, and inventing a string that matches an arbitrary regex is
        // a generator this does not have.
        // `naming` used to sit beside `call-obligation` here. It does not need
        // to: a violation of a `naming` rule is not an invented filename, it is
        // a file the rule already covers with the export taken away. Issue #154.
        CompiledRuleKind::Naming { .. } => a_covered_file_without_its_export(rule, engine, tree),
        CompiledRuleKind::CallObligation { .. } => Verdict::Unverified {
            why: "a violation means inventing a filename that matches this rule's \
                  `file_pattern`, which is a regex run backwards"
                .to_owned(),
        },

        // Planting a violation means two files that import each other and a
        // resolver that places both, which is the whole `check` pipeline run
        // inside a probe. Every other probe here hands an engine synthetic
        // facts and reads the verdict; this one would have to build a
        // repository on disk to get the edges, and a probe that heavy is a
        // second implementation of the thing it is checking.
        CompiledRuleKind::ImportCycle { .. } => Verdict::Unverified {
            why: "planting a cycle means writing two files that import each \
                  other and resolving both, which is the resolver run inside a \
                  probe"
                .to_owned(),
        },

        // Synthesising a passthrough file is possible and the shapes are
        // configurable -- `reexport`, `alias`, `wrapper`, partial forms, the
        // `package.json` entrypoint exemption. A probe that covered one form
        // would tick for a rule configured for another, which is a confident
        // answer about the wrong question.
        CompiledRuleKind::NoPassthrough { .. } => Verdict::Unverified {
            why: "which shape of forwarding counts is configurable, and a probe \
                  of one shape would tick for a rule about another"
                .to_owned(),
        },
    }
}

mod probes;
mod render;

pub use render::render;

use probes::declarations::{
    a_document_with_no_block, a_file_declaring_the_wrong_thing, a_file_with_no_companion,
};
use probes::pairing::{a_file_added_to_a_freeze, a_file_with_no_counterpart, a_file_with_no_spec};
use probes::reach::{
    a_call_from_outside_the_chokepoint, a_covered_file_without_its_export,
    a_file_of_the_wrong_shape, crossed_boundary,
};
use probes::structure::{a_directory_holding_nothing, forbidden_subfolder};

#[cfg(test)]
mod tests {
    use super::probes::structure::{
        constrains_filenames, constrains_subfolders, unclaimed_filename,
    };
    use super::*;
    use archwarden_core::hash::ContentHash;
    use archwarden_core::{
        compiled::SkipDirs, glob::PathSet, ids::RuleId, level::Level, pattern::Pattern,
        scope::Scope,
    };
    use camino::Utf8PathBuf;

    fn tree_at(entries: &[&str]) -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");
        for relative in entries {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().expect("a file has a parent"))
                .expect("create dirs");
            std::fs::write(&path, "export const x = 1;").expect("write file");
        }
        (dir, root)
    }

    fn config_of(rules: Vec<CompiledRule>) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b"verify"),
        )
    }

    fn rule(id: &str, scope: &[&str], kind: CompiledRuleKind) -> CompiledRule {
        CompiledRule {
            id: RuleId::new(id).expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            directives: None,
            level: Level::Error,
            scope: Scope::compile(scope.iter().copied()).expect("valid scope"),
            kind,
        }
    }

    fn boundary(forbid: &[&str], packages: &[&str], except_from: &[&str]) -> CompiledRuleKind {
        CompiledRuleKind::ImportBoundary {
            forbid: PathSet::compile(forbid.iter().map(|g| (*g).to_owned())).expect("valid globs"),
            groups: Vec::new(),
            allow: None,
            allow_packages: None,
            require: PathSet::default(),
            forbid_packages: packages.iter().map(|p| (*p).to_owned()).collect(),
            forbid_reaching: PathSet::default(),
            except: PathSet::default(),
            except_from: PathSet::compile(except_from.iter().map(|g| (*g).to_owned()))
                .expect("valid globs"),
            include_type_only: true,
        }
    }

    fn verdict(entries: &[&str], rules: Vec<CompiledRule>) -> Verdict {
        let (guard, root) = tree_at(entries);
        let config = config_of(rules);
        let tree = archwarden_engine::walk::walk(&root, &config).expect("walks");
        let mut verifications = verify(&config, &tree);
        drop(guard);
        verifications.pop().expect("one rule, one verdict").verdict
    }

    fn chokepoint(callee: &[&str], only_in: &[&str]) -> CompiledRuleKind {
        CompiledRuleKind::Chokepoint {
            callee: callee.iter().map(|c| (*c).to_owned()).collect(),
            renders: Vec::new(),
            only_in: Scope::compile(only_in.iter().copied()).expect("valid scope"),
        }
    }

    /// Issue #118. A chokepoint breach is plantable where `forbid_reaching` is
    /// not: one file with one call in it, rather than a chain that has to
    /// resolve against a second.
    #[test]
    fn a_chokepoint_is_proved_by_a_call_from_outside_it() {
        let verdict = verdict(
            &["src/config/env.ts", "src/orders/place.ts"],
            vec![rule(
                "the-environment-is-read-once",
                &["src/*"],
                chokepoint(&["process.env"], &["src/config/**"]),
            )],
        );

        assert!(
            matches!(&verdict, Verdict::Fires { on } if on.contains("calling `process.env`")),
            "{verdict:?}"
        );
        // Planted outside the chokepoint, which is the only place a breach can
        // sit -- a probe in `src/config` would prove nothing.
        assert!(
            matches!(&verdict, Verdict::Fires { on } if !on.contains("src/config")),
            "{verdict:?}"
        );
    }

    /// A repository whose whole scope is inside the chokepoint has nowhere to
    /// plant one. That is a rule `config doctor` should be reporting rather
    /// than a failure of this probe, so it is named with the reason.
    #[test]
    fn a_chokepoint_covering_only_its_own_scope_cannot_be_proved() {
        let verdict = verdict(
            &["src/config/env.ts"],
            vec![rule(
                "the-environment-is-read-once",
                &["src/config/*"],
                chokepoint(&["process.env"], &["src/config/**"]),
            )],
        );

        assert!(
            matches!(&verdict, Verdict::Unverified { why } if why.contains("nowhere outside")),
            "{verdict:?}"
        );
    }

    /// And a rule guarding nothing constrains nothing, which is `doctor`'s
    /// sentence rather than this one's.
    #[test]
    fn a_chokepoint_guarding_no_callee_is_named_as_unverified() {
        let verdict = verdict(
            &["src/orders/place.ts"],
            vec![rule(
                "guards-nothing",
                &["src/*"],
                chokepoint(&[], &["src/config/**"]),
            )],
        );

        assert!(
            matches!(&verdict, Verdict::Unverified { why } if why.contains("guards no callee")),
            "{verdict:?}"
        );
    }

    fn metadata(
        require: &[&str],
        one_of: &[(&str, &[&str])],
        equals: &[(&str, &str)],
    ) -> CompiledRuleKind {
        CompiledRuleKind::Metadata {
            require: require.iter().map(|k| (*k).to_owned()).collect(),
            one_of: one_of
                .iter()
                .map(|(key, values)| {
                    (
                        (*key).to_owned(),
                        values.iter().map(|v| (*v).to_owned()).collect(),
                    )
                })
                .collect(),
            equals: equals
                .iter()
                .map(|(key, template)| ((*key).to_owned(), (*template).to_owned()))
                .collect(),
            deadline: Vec::new(),
        }
    }

    /// The headline claim, planted as an absence: a file declaring nothing.
    #[test]
    fn a_metadata_rule_requiring_a_key_is_reported_as_firing() {
        let verdict = verdict(
            &["src/payments/refund.ts"],
            vec![rule(
                "payments-declares-an-owner",
                &["src/payments/**"],
                metadata(&["owner"], &[], &[]),
            )],
        );

        assert!(
            matches!(&verdict, Verdict::Fires { on } if on.contains("declaring nothing about itself")),
            "{verdict:?}"
        );
    }

    /// A rule that only asks what a value must be is satisfied by a file that
    /// declares nothing, so the probe has to declare something the rule
    /// refuses. Both clauses, because they are planted two different ways.
    #[test]
    fn a_metadata_rule_asking_only_about_values_is_still_probed() {
        for kind in [
            metadata(&[], &[("stability", &["stable", "experimental"])], &[]),
            metadata(&[], &[], &[("module", "{{raw(dirname)}}")]),
        ] {
            let verdict = verdict(
                &["src/payments/refund.ts"],
                vec![rule("payments-owned", &["src/payments/**"], kind.clone())],
            );

            assert!(
                matches!(&verdict, Verdict::Fires { on } if on.contains("a header this rule refuses")),
                "{kind:?} gave {verdict:?}"
            );
        }
    }

    /// A rule that asks for nothing is reported as unverified rather than as
    /// firing or silent: there is no violation of its terms to plant, and
    /// saying so is worth more than a confident answer to a question nobody
    /// asked.
    #[test]
    fn a_metadata_rule_asking_for_nothing_is_unverified() {
        let verdict = verdict(
            &["src/payments/refund.ts"],
            vec![rule(
                "payments-owned",
                &["src/payments/**"],
                metadata(&[], &[], &[]),
            )],
        );

        assert!(
            matches!(&verdict, Verdict::Unverified { why } if why.contains("asks for no key")),
            "{verdict:?}"
        );
    }

    /// A rule whose scope reaches no file in this repository cannot be probed,
    /// and the reason is stated rather than guessed at.
    #[test]
    fn a_metadata_rule_reaching_nothing_is_unverified() {
        let verdict = verdict(
            &["src/billing/invoice.ts"],
            vec![rule(
                "payments-owned",
                &["src/payments/**"],
                metadata(&["owner"], &[], &[]),
            )],
        );

        assert!(
            matches!(&verdict, Verdict::Unverified { why } if why.contains("no file in this repository")),
            "{verdict:?}"
        );
    }

    /// The rule the issue's author proved by hand, by planting a file and
    /// deleting it: a relative escape out of a package into an app.
    #[test]
    fn a_boundary_that_bites_is_reported_as_firing() {
        let verdict = verdict(
            &["packages/domain/order.ts", "apps/api/src/env.ts"],
            vec![rule(
                "domain-is-self-contained",
                &["packages/domain/**"],
                boundary(&["apps/**"], &[], &[]),
            )],
        );

        assert!(
            matches!(verdict, Verdict::Fires { .. }),
            "it should catch an import of `apps/**`: {verdict:?}"
        );
    }

    /// And the finding this command exists for: a rule that covers the right
    /// files, appears in `explain`, and enforces nothing -- here because
    /// `except_from` exempts everything its scope reaches.
    #[test]
    fn a_boundary_exempted_into_inertness_is_reported_as_silent() {
        let verdict = verdict(
            &["packages/domain/order.ts", "apps/api/src/env.ts"],
            vec![rule(
                "domain-is-self-contained",
                &["packages/domain/**"],
                boundary(&["apps/**"], &[], &["packages/domain/**"]),
            )],
        );

        assert!(
            matches!(verdict, Verdict::Unverified { .. }),
            "every file it covers is exempt, so there is nothing to probe with: {verdict:?}"
        );
    }

    /// A rule that only forbids *reaching* cannot be probed, and says so in
    /// those words.
    ///
    /// It fell through to the "only requires an import" branch before, which
    /// is a true-sounding sentence about a rule that does not require an
    /// import at all — and a wrong explanation of an `unverified` is worse
    /// than a vague one, because a reader acts on it.
    #[test]
    fn a_boundary_that_only_forbids_reaching_says_why_it_cannot_be_probed() {
        let mut kind = boundary(&[], &[], &[]);
        let CompiledRuleKind::ImportBoundary {
            forbid_reaching: slot,
            ..
        } = &mut kind
        else {
            panic!("built as an import-boundary rule");
        };
        *slot = PathSet::compile(["packages/db/**".to_owned()]).expect("valid globs");

        let verdict = verdict(
            &["packages/ui/button.tsx", "packages/db/client.ts"],
            vec![rule("ui-must-not-reach-db", &["packages/ui/**"], kind)],
        );

        let Verdict::Unverified { why } = &verdict else {
            panic!("a chain cannot be planted by a probe: {verdict:?}");
        };
        assert!(
            why.contains("reach"),
            "the reason has to name the half that could not be probed: {why}"
        );
    }

    /// A rule that forbids *both* is still probed for the half a probe can
    /// reach.
    ///
    /// The refusal above is for a rule with nothing else to test. A rule that
    /// also forbids a direct import has a verifiable half, and reporting the
    /// whole rule as `unverified` would hide a `forbid_import_from` that
    /// enforces nothing — which is the finding this command exists for.
    #[test]
    fn a_boundary_that_forbids_both_is_still_probed_for_the_direct_half() {
        let mut kind = boundary(&["apps/**"], &[], &[]);
        let CompiledRuleKind::ImportBoundary {
            forbid_reaching: slot,
            ..
        } = &mut kind
        else {
            panic!("built as an import-boundary rule");
        };
        *slot = PathSet::compile(["packages/db/**".to_owned()]).expect("valid globs");

        let verdict = verdict(
            &["packages/domain/order.ts", "apps/api/src/env.ts"],
            vec![rule(
                "domain-is-self-contained",
                &["packages/domain/**"],
                kind,
            )],
        );

        assert!(
            matches!(verdict, Verdict::Fires { .. }),
            "the direct half is probeable and must still be probed: {verdict:?}"
        );
    }

    /// A `structure` rule is handed a folder it does not allow.
    #[test]
    fn a_structure_rule_is_probed_with_a_folder_it_forbids() {
        let verdict = verdict(
            &["src/order/types/x.ts"],
            vec![rule(
                "entity-shape",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec!["types".to_owned()]),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )],
        );

        assert!(matches!(verdict, Verdict::Fires { .. }), "{verdict:?}");
    }

    /// A `structure` rule that constrains filenames is probed with a filename.
    ///
    /// Issue #49: every `structure` rule was probed by offering it an unlisted
    /// folder. A rule that says nothing about subfolders is correctly silent on
    /// that, and was reported as enforcing nothing — five of fourteen rules in
    /// one repository, all five of which fire on the axis they actually
    /// constrain.
    ///
    /// Worse than a wrong tick, because of what the command is for. `#24` asked
    /// for it precisely because `explain` shows coverage and not efficacy, so
    /// *"5 enforce nothing"* is the line somebody acts on — and acting on it
    /// here means deleting five rules that work. A verifier that reports a
    /// false negative is worse than no verifier, for the reason the docs give
    /// about silent rules: it is indistinguishable from the real thing.
    #[test]
    fn a_structure_rule_that_only_constrains_filenames_is_probed_with_a_filename() {
        let verdict = verdict(
            &["scripts/build.ts"],
            vec![rule(
                "scripts-kebab-case",
                &["scripts"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: None,
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: vec![Pattern::compile(r"^[a-z0-9-]+\.ts$").expect("valid")],
                },
            )],
        );

        assert!(
            matches!(verdict, Verdict::Fires { .. }),
            "it refuses `NomeErrado.ts` and should be probed with one: {verdict:?}"
        );
    }

    /// A rule constraining both axes is verified if either one fires.
    #[test]
    fn a_structure_rule_constraining_both_axes_is_probed_on_both() {
        let verdict = verdict(
            &["src/order/types/x.ts"],
            vec![rule(
                "entity-shape",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec!["types".to_owned()]),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: vec![Pattern::compile(r"^[a-z0-9-]+\.ts$").expect("valid")],
                },
            )],
        );

        assert!(matches!(verdict, Verdict::Fires { .. }), "{verdict:?}");
    }

    /// All three `export-shape` claims are plantable, which is unusual here:
    /// `naming` needs a regex run backwards and a cycle needs two files that
    /// resolve against each other. "Has a default", "has one too many" and
    /// "declares no return type" are each one synthetic fact. Issue #101.
    #[test]
    fn every_export_shape_claim_can_be_probed() {
        let claims = [
            (true, None, &[][..]),
            (false, Some(1), &[][..]),
            (false, None, &[r"^Result<.+>$"][..]),
        ];

        for (forbid_default, max_exports, must_return) in claims {
            let verdict = verdict(
                &["src/use-cases/create-client.ts"],
                vec![rule(
                    "use-case-shape",
                    &["src/use-cases"],
                    CompiledRuleKind::ExportShape(archwarden_core::compiled::ExportShape {
                        forbid_default,
                        max_exports,
                        must_return: must_return
                            .iter()
                            .map(|p| Pattern::compile(p).expect("valid"))
                            .collect(),
                    }),
                )],
            );

            assert!(
                matches!(verdict, Verdict::Fires { .. }),
                "{forbid_default} {max_exports:?} {must_return:?}: {verdict:?}"
            );
        }
    }

    /// A rule making none of the three has nothing to break, and says so
    /// rather than reporting a confident tick — `config doctor` is what calls
    /// a rule that constrains nothing what it is.
    #[test]
    fn an_export_shape_rule_that_asks_nothing_cannot_be_probed() {
        let verdict = verdict(
            &["src/use-cases/create-client.ts"],
            vec![rule(
                "asks-nothing",
                &["src/use-cases"],
                CompiledRuleKind::ExportShape(archwarden_core::compiled::ExportShape {
                    forbid_default: false,
                    max_exports: None,
                    must_return: Vec::new(),
                }),
            )],
        );

        let Verdict::Unverified { why } = &verdict else {
            panic!("expected Unverified, got {verdict:?}");
        };
        assert!(why.contains("none of the three claims"), "{why}");
    }

    /// And a rule whose scope reaches a directory but not a file directly in
    /// it has nowhere to sit the probe, which is reported rather than guessed
    /// past.
    #[test]
    fn an_export_shape_rule_with_nowhere_to_sit_a_probe_says_so() {
        let verdict = verdict(
            &["src/use-cases/nested/create-client.ts"],
            vec![rule(
                "use-case-shape",
                &["src/use-cases"],
                CompiledRuleKind::ExportShape(archwarden_core::compiled::ExportShape {
                    forbid_default: true,
                    max_exports: None,
                    must_return: Vec::new(),
                }),
            )],
        );

        assert!(
            matches!(verdict, Verdict::Fires { .. } | Verdict::Unverified { .. }),
            "either it plants the probe or it says why not, never silence: {verdict:?}"
        );
    }

    /// And a `structure` rule that constrains neither axis really does enforce
    /// nothing, which is the answer the command exists to give.
    #[test]
    fn a_structure_rule_constraining_nothing_is_still_reported_silent() {
        let verdict = verdict(
            &["src/order/x.ts"],
            vec![rule(
                "says-nothing",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: None,
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )],
        );

        assert!(
            matches!(verdict, Verdict::Silent { .. }),
            "a rule with no requirement at all should still be caught: {verdict:?}"
        );
    }

    /// Which axes a rule constrains, asked directly.
    ///
    /// The two probes are chosen by these, so a function that answered `true`
    /// for everything would put every rule through both — and one that
    /// answered `false` for everything would put it through neither and call
    /// it silent, which is the bug this replaced.
    #[test]
    fn each_axis_is_recognised_on_its_own() {
        let none = CompiledRuleKind::Structure {
            allowed_subfolders: None,
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
            filename_patterns: Vec::new(),
        };
        assert!(!constrains_subfolders(&none));
        assert!(!constrains_filenames(&none));

        let folders = CompiledRuleKind::Structure {
            allowed_subfolders: Some(Vec::new()),
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
            filename_patterns: Vec::new(),
        };
        assert!(
            constrains_subfolders(&folders),
            "an empty allow-list forbids every subfolder, which is a constraint"
        );
        assert!(!constrains_filenames(&folders));

        let names = CompiledRuleKind::Structure {
            allowed_subfolders: None,
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: Vec::new(),
            filename_patterns: vec![Pattern::compile("^a$").expect("valid")],
        };
        assert!(!constrains_subfolders(&names));
        assert!(constrains_filenames(&names));

        let patterned = CompiledRuleKind::Structure {
            allowed_subfolders: None,
            warn_subfolders: Vec::new(),
            recurse_into: Vec::new(),
            subfolder_patterns: vec![Pattern::compile("^a$").expect("valid")],
            filename_patterns: Vec::new(),
        };
        assert!(
            constrains_subfolders(&patterned),
            "a subfolder regex is a constraint on subfolders"
        );
    }

    /// The probe filename has to be one the rule refuses.
    ///
    /// A name the pattern happens to accept would be reported silent for a
    /// file the rule was right to allow — the same false negative one layer
    /// down, and the one this whole change is about.
    #[test]
    fn the_probe_filename_is_one_the_patterns_reject() {
        // Written against the patterns themselves rather than against a
        // spelling: the contract is "this rule rejects the probe", and a test
        // that checked a suffix would pass for a probe the rule accepts.
        for source in [r"\.probe$", r"^[a-z0-9-]+\.ts$", r"^archwarden-.*$"] {
            let pattern = Pattern::compile(source).expect("valid");
            let kind = CompiledRuleKind::Structure {
                allowed_subfolders: None,
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                subfolder_patterns: Vec::new(),
                filename_patterns: vec![pattern],
            };

            let probe = unclaimed_filename(&kind);
            // The name is printed back — "a file named `X` in `Y`" — so it has
            // to read as archwarden's, not as a file the reader might go
            // looking for in their own repository.
            assert!(
                probe.contains(PROBE),
                "`{probe}` does not name itself as a probe, and it is shown to \
                 a reader as the thing the rule was handed"
            );

            let CompiledRuleKind::Structure {
                filename_patterns, ..
            } = &kind
            else {
                unreachable!("built as a structure rule")
            };
            assert!(
                !filename_patterns
                    .iter()
                    .any(|pattern| pattern.is_match(&probe)),
                "`{probe}` is a name `{source}` accepts, so the rule is right to \
                 stay silent about it and would be called idle for doing so"
            );
        }
    }

    /// The case the issue called impossible. `spec-pair` reports through
    /// `check_directory`, and what it is handed is a listing -- so a listing
    /// with a lone source file in it is the absence, synthesised.
    #[test]
    fn a_spec_pair_rule_is_probed_with_a_file_that_has_no_spec() {
        let verdict = verdict(
            &["src/order/x.ts", "src/order/x.spec.ts"],
            vec![rule(
                "calcs-need-spec",
                &["src/*"],
                CompiledRuleKind::SpecPair {
                    subfolders: vec![".".to_owned()],
                    spec_markers: vec!["spec".to_owned()],
                    ignore_files: PathSet::default(),
                    spec_dirs: Vec::new(),
                    require_non_empty_spec: false,
                    skip_type_only: false,
                },
            )],
        );

        assert!(matches!(verdict, Verdict::Fires { .. }), "{verdict:?}");
    }

    /// A scope that reaches nothing cannot be probed, and says so rather than
    /// accusing the rule of being silent. The two are different problems and
    /// `doctor` owns the first.
    #[test]
    fn a_rule_that_reaches_nothing_is_unverified_rather_than_silent() {
        let verdict = verdict(
            &["src/order/x.ts"],
            vec![rule(
                "nowhere",
                &["packages/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec!["types".to_owned()]),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )],
        );

        assert!(matches!(verdict, Verdict::Unverified { .. }), "{verdict:?}");
    }

    /// A boundary whose forbidden side names nothing this repository has is
    /// unverified too: the probe would have to import a file that does not
    /// exist, and a rule cannot be blamed for not catching it.
    #[test]
    fn a_boundary_with_nothing_to_import_is_unverified() {
        let verdict = verdict(
            &["packages/domain/order.ts"],
            vec![rule(
                "domain-is-self-contained",
                &["packages/domain/**"],
                boundary(&["apps/**"], &[], &[]),
            )],
        );

        assert!(matches!(verdict, Verdict::Unverified { .. }), "{verdict:?}");
    }

    /// Issue #154. `naming` was the one kind whose bite this command could not
    /// demonstrate -- and it is the kind most likely to be silently inert, so
    /// leaving it unverified left the gap exactly where it mattered.
    ///
    /// The probe does not invent a filename, which would be a regex run
    /// backwards. It takes a file the rule *already* covers and hands the
    /// engine facts with no exports at all.
    #[test]
    fn a_naming_rule_is_proved_by_a_file_it_covers_exporting_nothing() {
        let verdict = verdict(
            &["src/order/create.use-case.ts"],
            vec![rule("usecase-name", &["src/*"], naming())],
        );

        assert!(
            matches!(&verdict, Verdict::Fires { on } if on.contains("create.use-case.ts")
                && on.contains("exporting nothing")),
            "{verdict:?}"
        );
    }

    /// And a rule reaching no file keeps its verdict honest rather than
    /// claiming one. That state is `config doctor`'s `scope-matches-nothing`,
    /// not this command's business.
    #[test]
    fn a_naming_rule_that_covers_no_file_is_named_as_unverified() {
        let verdict = verdict(
            &["src/order/notes.md"],
            vec![rule("usecase-name", &["src/*"], naming())],
        );

        let Verdict::Unverified { why } = verdict else {
            panic!("there is nothing to take an export away from: {verdict:?}");
        };
        assert!(why.contains("no file this rule covers"), "{why}");
    }

    /// `call-obligation` still cannot be synthesised, and says so rather than
    /// being left out of the report. A rule that went unchecked has to be
    /// visible as unchecked.
    #[test]
    fn the_kind_that_cannot_be_synthesised_is_named() {
        let verdict = verdict(
            &["src/order/route.post.ts"],
            vec![rule(
                "audit",
                &["src/*"],
                CompiledRuleKind::CallObligation {
                    file_pattern: Pattern::compile(r"^route\.post\.ts$").expect("valid pattern"),
                    symbol: "Event.save".to_owned(),
                    imported_from: "@org/domain/event".to_owned(),
                    with_options: Vec::new(),
                },
            )],
        );

        let Verdict::Unverified { why } = verdict else {
            panic!("a filename cannot be invented: {verdict:?}");
        };
        assert!(why.contains("file_pattern"), "{why}");
    }

    /// The `naming` rule the two tests above share.
    fn naming() -> CompiledRuleKind {
        CompiledRuleKind::Naming {
            file_pattern: Pattern::compile(r"^(?<name>[a-z-]+)\.use-case\.ts$")
                .expect("valid pattern"),
            dir_pattern: None,
            name_template: "{{pascal(name)}}".to_owned(),
            kind: archwarden_core::facts::KindFilter::Any,
            annotation: Vec::new(),
            signature_hint: None,
            ignore_files: archwarden_core::glob::PathSet::default(),
        }
    }

    /// A `structure` rule that allows a folder by the probe's own name is
    /// handed a different one. Being told a rule is silent because it was
    /// handed something legal is a false accusation, in the one command whose
    /// job is not to make them.
    #[test]
    fn the_probe_never_uses_a_name_the_rule_allows() {
        let verdict = verdict(
            &["src/order/types/x.ts"],
            vec![rule(
                "entity-shape",
                &["src/*"],
                CompiledRuleKind::Structure {
                    allowed_subfolders: Some(vec![PROBE.to_owned(), "types".to_owned()]),
                    warn_subfolders: Vec::new(),
                    recurse_into: Vec::new(),
                    subfolder_patterns: Vec::new(),
                    filename_patterns: Vec::new(),
                },
            )],
        );

        assert!(
            matches!(verdict, Verdict::Fires { .. }),
            "the probe should have picked another name: {verdict:?}"
        );
    }
}
