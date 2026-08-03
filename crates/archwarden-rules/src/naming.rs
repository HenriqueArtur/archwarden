//! The `naming` rule: the filename dictates the exported symbol's name.
//!
//! The first rule that needs a parser. Everything before it reasons about
//! names on disk; this one opens the file to see what it exports.
//!
//! See `docs/RULES.md`.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    facts::{ExportFact, ExportKind, KindFilter},
    finding::{Expectation, Finding, Observed},
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    pattern::Pattern,
    scope::Scope,
    template,
    traits::{FileContext, RuleEngine},
};

/// A compiled `naming` rule.
#[derive(Debug, Clone)]
pub struct NamingEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    file_pattern: Pattern,
    dir_pattern: Option<Pattern>,
    name_template: String,
    kind: KindFilter,
    signature_hint: Option<String>,
}

impl NamingEngine {
    /// Builds an engine from a compiled rule.
    ///
    /// Returns `None` for a rule of any other kind.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::Naming {
            file_pattern,
            dir_pattern,
            name_template,
            kind,
            signature_hint,
        } = &rule.kind
        else {
            return None;
        };

        Some(Self::build(
            rule,
            file_pattern,
            dir_pattern.as_ref(),
            name_template,
            kind,
            signature_hint.as_deref(),
        ))
    }

    /// Builds an engine from a rule whose kind is already known.
    ///
    /// Infallible, and that is the point: `engines_for` matches every
    /// `CompiledRuleKind` exhaustively and calls the matching constructor, so
    /// a kind added without an engine fails to compile. There is no runtime
    /// state in which a rule goes unchecked, which is why a run has nothing to
    /// report as unimplemented.
    pub(crate) fn build(
        rule: &CompiledRule,
        file_pattern: &Pattern,
        dir_pattern: Option<&Pattern>,
        name_template: &str,
        kind: &KindFilter,
        signature_hint: Option<&str>,
    ) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            file_pattern: file_pattern.clone(),
            dir_pattern: dir_pattern.cloned(),
            name_template: name_template.to_owned(),
            kind: kind.clone(),
            signature_hint: signature_hint.map(str::to_owned),
        }
    }

    /// The export name this file must carry, if the rule applies to it.
    ///
    /// `None` when the rule does not apply, or when the template cannot be
    /// rendered -- which `config validate` already refuses, so by the time a
    /// check runs it means the filename matched without supplying a group.
    fn required_name(&self, path: &RepoRelPath) -> Option<String> {
        if !self.scope.contains_file(path.as_path()) {
            return None;
        }
        let name = path.file_name()?;
        if !self.file_pattern.is_match(name) {
            return None;
        }
        if !self.directory_matches(path) {
            return None;
        }

        self.render(&self.name_template, path)
    }

    /// The name of the directory the file sits in, for `dir_pattern` to read.
    ///
    /// The last segment rather than the whole path: the scope glob has already
    /// chosen which directories are in play, and what a rule wants to capture
    /// out of `.../Entities/Order/insert.ts` is `Order`. A pattern anchored
    /// with `^` and `$` — which is how anyone writes one — could not match the
    /// full path at all.
    fn directory_name(path: &RepoRelPath) -> Option<String> {
        path.parent()?.file_name().map(ToOwned::to_owned)
    }

    /// Whether `dir_pattern` is satisfied, which a rule without one always is.
    ///
    /// A file directly at the repository root has no directory to offer, so a
    /// rule that asks about one does not apply to it. That is the same answer
    /// `file_pattern` gives to a filename it does not match: not a violation,
    /// just not this rule's business.
    fn directory_matches(&self, path: &RepoRelPath) -> bool {
        let Some(pattern) = self.dir_pattern.as_ref() else {
            return true;
        };
        Self::directory_name(path).is_some_and(|directory| pattern.is_match(&directory))
    }

    /// Renders a template against the capture groups of both patterns.
    ///
    /// One namespace, deliberately: `{{pascal(entity)}}{{pascal(action)}}` does
    /// not say which pattern each group came from, and it should not have to.
    /// A group defined by both patterns is refused when the config is compiled,
    /// so the order these are tried in cannot decide an answer.
    fn render(&self, template: &str, path: &RepoRelPath) -> Option<String> {
        let filename = path.file_name()?;
        let directory = Self::directory_name(path);

        template::render(template, |group| {
            self.file_pattern
                .capture(filename, group)
                .map(ToOwned::to_owned)
                .or_else(|| {
                    let pattern = self.dir_pattern.as_ref()?;
                    pattern
                        .capture(directory.as_deref()?, group)
                        .map(ToOwned::to_owned)
                })
        })
        .ok()
    }

    /// The expectation for a file, with both templates rendered.
    ///
    /// `signature_hint` goes through the same rendering as the name. It is
    /// never verified, but it is *shown* -- by `scaffold`, and in a finding --
    /// and showing `{{pascal(name)}}` to a user is showing them our internals.
    fn expectation(&self, path: &RepoRelPath) -> Option<Expectation> {
        Some(Expectation::RequiredExport {
            kind: self.kind.clone(),
            name: self.required_name(path)?,
            signature_hint: self
                .signature_hint
                .as_ref()
                .and_then(|hint| self.render(hint, path)),
        })
    }

    fn finding(&self, path: &RepoRelPath, observed: Observed, expected: Expectation) -> Finding {
        Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: path.clone(),
            span: None,
            observed,
            expected,
        }
    }

    /// What is wrong with the export the file does carry, if anything.
    fn fault(&self, required: &str, export: &ExportFact) -> Option<Observed> {
        if self.kind.accepts(export.tags) {
            return None;
        }

        // A re-export's declaration form lives in another file. Saying so is
        // more useful than reporting the wrong kind, which would send the
        // reader looking for a declaration that is not there.
        if export.tags.contains(ExportKind::Reexport) {
            return Some(Observed::ReexportOfUnknownKind {
                name: required.to_owned(),
                from: export
                    .reexport_from
                    .clone()
                    .unwrap_or_else(|| "another module".to_owned()),
            });
        }

        Some(Observed::ExportWrongKind {
            name: required.to_owned(),
            found: export.tags,
        })
    }
}

impl RuleEngine for NamingEngine {
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
        self.required_name(path).is_some()
    }

    fn needs_facts(&self) -> bool {
        true
    }

    fn check_file(&self, ctx: FileContext<'_>) -> Vec<Finding> {
        let Some(required) = self.required_name(ctx.path) else {
            return Vec::new();
        };
        // No facts means no parser ran. Reporting "no such export" would be
        // accusing a file nobody read.
        let Some(facts) = ctx.facts else {
            return Vec::new();
        };

        let Some(expected) = self.expectation(ctx.path) else {
            return Vec::new();
        };

        let Some(export) = facts.named_export(&required) else {
            // A default export is not a named one: its local name does not
            // bind the importer, so it can never satisfy this rule. Saying
            // that is more useful than "no such export" when the file plainly
            // exports something.
            let observed = if facts.exports.iter().all(|export| export.is_default)
                && !facts.exports.is_empty()
            {
                Observed::OnlyDefaultExport
            } else {
                Observed::ExportMissing { name: required }
            };
            return vec![self.finding(ctx.path, observed, expected)];
        };

        self.fault(&required, export)
            .map(|observed| self.finding(ctx.path, observed, expected))
            .into_iter()
            .collect()
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        self.expectation(path).into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        facts::{ExportTags, FileFacts, Span},
        hash::ContentHash,
    };

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn engine_with(
        scope: &[&str],
        file_pattern: &str,
        name_template: &str,
        kind: KindFilter,
        signature_hint: Option<&str>,
    ) -> NamingEngine {
        let rule = CompiledRule {
            id: RuleId::new("usecase-factory-name").expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(scope).expect("valid scope"),
            kind: CompiledRuleKind::Naming {
                file_pattern: Pattern::compile(file_pattern).expect("valid pattern"),
                dir_pattern: None,
                name_template: name_template.to_owned(),
                kind,
                signature_hint: signature_hint.map(ToOwned::to_owned),
            },
        };

        NamingEngine::from_rule(&rule).expect("is a naming rule")
    }

    /// The rule from docs/CONFIG.md: `<name>.use-case.ts` must export
    /// `Pascal(<name>)` as a function.
    fn engine() -> NamingEngine {
        engine_with(
            &["packages/application/src/use-cases/*"],
            r"^(?<name>[a-z0-9-]+)\.use-case\.ts$",
            "{{pascal(name)}}",
            KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            None,
        )
    }

    /// The rule from issue #16, which needs both halves of the path: the
    /// entity names the directory, the action names the file, and the export
    /// is spelled from the two.
    fn entity_engine() -> NamingEngine {
        let rule = CompiledRule {
            id: RuleId::new("repository-action-export-name").expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(["src/Repositories/Entities/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Naming {
                file_pattern: Pattern::compile(r"^(?<action>[a-z0-9-]+)\.ts$")
                    .expect("valid pattern"),
                dir_pattern: Some(
                    Pattern::compile(r"^(?<entity>[A-Za-z0-9]+)$").expect("valid pattern"),
                ),
                name_template: "{{pascal(entity)}}{{pascal(action)}}Repository".to_owned(),
                kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
                signature_hint: None,
            },
        };

        NamingEngine::from_rule(&rule).expect("is a naming rule")
    }

    fn facts_with(file: &str, exports: Vec<ExportFact>) -> FileFacts {
        let mut facts = FileFacts::unparsed(path(file), ContentHash::of(b""));
        facts.exports = exports;
        facts
    }

    fn export(name: &str, tags: ExportTags) -> ExportFact {
        ExportFact {
            name: Some(name.to_owned()),
            tags,
            is_default: false,
            reexport_from: None,
            forwards: None,
            span: Span::new(0, 1),
        }
    }

    fn check(engine: &NamingEngine, facts: &FileFacts) -> Vec<Finding> {
        engine.check_file(FileContext {
            path: &facts.path,
            facts: Some(facts),
            siblings: &[],
        })
    }

    const USE_CASE: &str = "packages/application/src/use-cases/foo/create-client.use-case.ts";

    #[test]
    fn a_file_exporting_the_required_symbol_passes() {
        let facts = facts_with(
            USE_CASE,
            vec![export(
                "CreateClient",
                ExportTags::only(ExportKind::Function),
            )],
        );

        assert!(check(&engine(), &facts).is_empty());
    }

    #[test]
    fn a_missing_export_is_reported_by_the_name_it_should_have() {
        let facts = facts_with(
            USE_CASE,
            vec![export(
                "SomethingElse",
                ExportTags::only(ExportKind::Function),
            )],
        );

        let findings = check(&engine(), &facts);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings.first().expect("one").observed,
            Observed::ExportMissing {
                name: "CreateClient".to_owned()
            }
        );
    }

    /// The distinction decision 9 exists for, reaching all the way from the
    /// parser to a finding.
    #[test]
    fn an_arrow_does_not_satisfy_a_function_requirement() {
        let facts = facts_with(
            USE_CASE,
            vec![export(
                "CreateClient",
                ExportTags::only(ExportKind::Const).with(ExportKind::Arrow),
            )],
        );

        let findings = check(&engine(), &facts);
        assert_eq!(
            findings.first().expect("one").observed,
            Observed::ExportWrongKind {
                name: "CreateClient".to_owned(),
                found: ExportTags::only(ExportKind::Const).with(ExportKind::Arrow),
            }
        );
    }

    /// `kind: ["function", "arrow"]` is how a preset says "callable, either
    /// form".
    #[test]
    fn a_filter_accepting_either_callable_form_accepts_both() {
        let engine = engine_with(
            &["packages/application/src/use-cases/*"],
            r"^(?<name>[a-z0-9-]+)\.use-case\.ts$",
            "{{pascal(name)}}",
            KindFilter::OneOf(ExportTags::only(ExportKind::Function).with(ExportKind::Arrow)),
            None,
        );

        for tags in [
            ExportTags::only(ExportKind::Function),
            ExportTags::only(ExportKind::Const).with(ExportKind::Arrow),
        ] {
            assert!(
                check(
                    &engine,
                    &facts_with(USE_CASE, vec![export("CreateClient", tags)])
                )
                .is_empty()
            );
        }
    }

    /// A default export's local name does not bind the importer, so it can
    /// never satisfy a named requirement. Saying that beats "no such export"
    /// when the file plainly exports something.
    #[test]
    fn a_file_exporting_only_a_default_is_told_why_that_cannot_satisfy_the_rule() {
        let facts = facts_with(
            USE_CASE,
            vec![ExportFact {
                is_default: true,
                name: None,
                ..export("ignored", ExportTags::only(ExportKind::Function))
            }],
        );

        let findings = check(&engine(), &facts);
        assert_eq!(
            findings.first().expect("one").observed,
            Observed::OnlyDefaultExport
        );
    }

    /// A file with no exports at all gets the plain message: there is nothing
    /// to explain away.
    #[test]
    fn a_file_with_no_exports_is_told_the_export_is_missing() {
        let facts = facts_with(USE_CASE, Vec::new());

        assert_eq!(
            check(&engine(), &facts).first().expect("one").observed,
            Observed::ExportMissing {
                name: "CreateClient".to_owned()
            }
        );
    }

    /// A re-export's declaration form lives in another file. Saying so is more
    /// useful than reporting the wrong kind, which would send a reader looking
    /// for a declaration that is not there.
    #[test]
    fn a_reexport_is_reported_as_not_determinable_rather_than_wrong() {
        let facts = facts_with(
            USE_CASE,
            vec![ExportFact {
                reexport_from: Some("./internal".to_owned()),
                forwards: None,
                ..export("CreateClient", ExportTags::only(ExportKind::Reexport))
            }],
        );

        assert_eq!(
            check(&engine(), &facts).first().expect("one").observed,
            Observed::ReexportOfUnknownKind {
                name: "CreateClient".to_owned(),
                from: "./internal".to_owned(),
            }
        );
    }

    /// `kind: "any"` is how a config says "I only care about the name", and it
    /// accepts a re-export because the name is all it asked about.
    #[test]
    fn the_any_filter_accepts_a_reexport() {
        let engine = engine_with(
            &["packages/application/src/use-cases/*"],
            r"^(?<name>[a-z0-9-]+)\.use-case\.ts$",
            "{{pascal(name)}}",
            KindFilter::Any,
            None,
        );
        let facts = facts_with(
            USE_CASE,
            vec![export(
                "CreateClient",
                ExportTags::only(ExportKind::Reexport),
            )],
        );

        assert!(check(&engine, &facts).is_empty());
    }

    /// A file the pattern does not match is none of the rule's business, even
    /// inside the scope.
    #[test]
    fn a_file_the_pattern_does_not_match_is_left_alone() {
        let facts = facts_with(
            "packages/application/src/use-cases/foo/helpers.ts",
            Vec::new(),
        );

        assert!(check(&engine(), &facts).is_empty());
        assert!(!engine().applies_to(&facts.path));
    }

    #[test]
    fn a_file_outside_the_scope_is_left_alone() {
        let facts = facts_with("elsewhere/create-client.use-case.ts", Vec::new());
        assert!(check(&engine(), &facts).is_empty());
    }

    /// Without facts no parser ran. Reporting a missing export would be
    /// accusing a file nobody read.
    #[test]
    fn a_file_that_was_never_parsed_is_not_accused() {
        let engine = engine();
        let path = path(USE_CASE);

        let findings = engine.check_file(FileContext {
            path: &path,
            facts: None,
            siblings: &[],
        });

        assert!(findings.is_empty());
        assert!(engine.applies_to(&path), "the rule does apply to it");
    }

    /// Decision 9 as an assertion: what the checker demands is what the
    /// informant advertises.
    #[test]
    fn what_check_demands_is_what_describe_expectation_advertises() {
        let engine = engine();
        let facts = facts_with(USE_CASE, Vec::new());

        let findings = check(&engine, &facts);
        let demanded = &findings.first().expect("one finding").expected;
        let advertised = engine.describe_expectation(&path(USE_CASE));

        assert_eq!(advertised.len(), 1);
        assert_eq!(advertised.first(), Some(demanded));
    }

    /// `scaffold` answers for a file nobody has written, which is the whole
    /// point of the informant: the agent learns the required name before
    /// writing, not after failing. The hint is rendered too -- showing a user
    /// `{{pascal(name)}}` would be showing them our internals.
    #[test]
    fn the_required_export_is_describable_before_the_file_exists() {
        let engine = engine_with(
            &["packages/application/src/use-cases/*"],
            r"^(?<name>[a-z0-9-]+)\.use-case\.ts$",
            "{{pascal(name)}}",
            KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            Some("(deps: {{pascal(name)}}Deps)"),
        );

        let expectations = engine.describe_expectation(&path(
            "packages/application/src/use-cases/bar/never-written.use-case.ts",
        ));

        assert_eq!(
            expectations,
            [Expectation::RequiredExport {
                kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
                name: "NeverWritten".to_owned(),
                signature_hint: Some("(deps: NeverWrittenDeps)".to_owned()),
            }]
        );
    }

    /// The hint reaches a *finding*, not only `scaffold`, so it has to be
    /// rendered there too. It shipped showing `{{pascal(name)}}` verbatim,
    /// which is showing a user our internals.
    #[test]
    fn the_signature_hint_is_rendered_in_a_finding() {
        let engine = engine_with(
            &["packages/application/src/use-cases/*"],
            r"^(?<name>[a-z0-9-]+)\.use-case\.ts$",
            "{{pascal(name)}}",
            KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
            Some("(deps: {{pascal(name)}}Deps)"),
        );
        let facts = facts_with(USE_CASE, Vec::new());

        let findings = check(&engine, &facts);
        assert_eq!(
            findings.first().expect("one").expected,
            Expectation::RequiredExport {
                kind: KindFilter::OneOf(ExportTags::only(ExportKind::Function)),
                name: "CreateClient".to_owned(),
                signature_hint: Some("(deps: CreateClientDeps)".to_owned()),
            }
        );
    }

    /// The template runs over the filename's capture groups, so the required
    /// name follows the file rather than being fixed.
    #[test]
    fn the_required_name_is_derived_from_the_filename() {
        let engine = engine();

        for (file, expected) in [
            ("create-client.use-case.ts", "CreateClient"),
            ("delete-invoice-line.use-case.ts", "DeleteInvoiceLine"),
            ("x.use-case.ts", "X"),
        ] {
            let facts = facts_with(
                &format!("packages/application/src/use-cases/foo/{file}"),
                Vec::new(),
            );
            assert_eq!(
                check(&engine, &facts).first().expect("one").observed,
                Observed::ExportMissing {
                    name: expected.to_owned()
                },
                "{file}"
            );
        }
    }

    /// Other exports are ignored: the rule enforces presence of the required
    /// one, not exclusivity.
    #[test]
    fn other_exports_alongside_the_required_one_are_ignored() {
        let facts = facts_with(
            USE_CASE,
            vec![
                export("Helper", ExportTags::only(ExportKind::Const)),
                export("CreateClient", ExportTags::only(ExportKind::Function)),
                export("Deps", ExportTags::only(ExportKind::Interface)),
            ],
        );

        assert!(check(&engine(), &facts).is_empty());
    }

    #[test]
    fn a_rule_of_another_kind_is_declined() {
        let structure = CompiledRule {
            id: RuleId::new("shape").expect("valid"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(["src/*"]).expect("valid"),
            kind: CompiledRuleKind::Structure {
                allowed_subfolders: Vec::new(),
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                filename_patterns: Vec::new(),
            },
        };

        assert!(NamingEngine::from_rule(&structure).is_none());
    }

    #[test]
    fn the_engine_reports_its_identity() {
        let engine = engine();
        assert_eq!(engine.id().as_str(), "usecase-factory-name");
        assert_eq!(engine.module(), None);
        assert_eq!(engine.level(), Level::Error);
    }

    /// The case issue #16 was filed with. `fetch-by-id.ts` exists forty times
    /// over in that repository, and what makes each one findable is the entity
    /// its directory names. Without this the closest expressible rule asks for
    /// `FetchByIdRepository` and is wrong on all 310 files -- not finding
    /// drift, disagreeing with the convention, which is the state where a rule
    /// gets deleted.
    #[test]
    fn an_export_name_can_be_spelled_from_the_directory_and_the_filename() {
        let engine = entity_engine();

        for (file, expected) in [
            (
                "src/Repositories/Entities/Order/fetch-by-id.ts",
                "OrderFetchByIdRepository",
            ),
            (
                "src/Repositories/Entities/Order/fetch-many-for-replay.ts",
                "OrderFetchManyForReplayRepository",
            ),
            (
                "src/Repositories/Entities/Wallet/apply-balance-delta.ts",
                "WalletApplyBalanceDeltaRepository",
            ),
        ] {
            let facts = facts_with(
                file,
                vec![export(expected, ExportTags::only(ExportKind::Function))],
            );
            assert!(
                check(&engine, &facts).is_empty(),
                "{file} exports {expected} and should pass"
            );

            // And the same file under the wrong name is reported by the name it
            // should have had, which is the half a reader acts on.
            let wrong = facts_with(
                file,
                vec![export("FetchById", ExportTags::only(ExportKind::Function))],
            );
            let findings = check(&engine, &wrong);
            assert_eq!(findings.len(), 1, "{file}");
            assert_eq!(
                findings[0].observed,
                Observed::ExportMissing {
                    name: expected.to_owned()
                },
                "{file}"
            );
        }
    }

    /// A directory the pattern does not match is a directory the rule is not
    /// about. Not a violation -- the same answer `file_pattern` gives to a
    /// filename it does not match.
    #[test]
    fn a_directory_that_does_not_match_puts_the_file_outside_the_rule() {
        let engine = entity_engine();
        // `_shared` fails `^[A-Za-z0-9]+$` on the underscore.
        let facts = facts_with(
            "src/Repositories/Entities/_shared/fetch-by-id.ts",
            vec![export("Anything", ExportTags::only(ExportKind::Function))],
        );

        assert!(!engine.applies_to(&facts.path));
        assert!(check(&engine, &facts).is_empty());
    }

    /// `dir_pattern` may be a filter and nothing more -- "only files in a
    /// directory named like an entity are subject to this rule", with the
    /// export name coming from the filename alone.
    ///
    /// This is the case that gives the match its own reason to exist. When the
    /// template *does* use a directory group, a non-matching directory would
    /// fail to render anyway and the file would fall out of the rule by
    /// accident; `cargo-mutants` found that by replacing the match with `true`
    /// and breaking nothing. Here nothing else would catch it, and dropping the
    /// guard would put every file in every sibling directory under the rule.
    #[test]
    fn a_directory_pattern_whose_groups_the_template_ignores_still_filters() {
        let rule = CompiledRule {
            id: RuleId::new("entities-only").expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(["src/Entities/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Naming {
                file_pattern: Pattern::compile(r"^(?<action>[a-z-]+)\.ts$").expect("valid"),
                dir_pattern: Some(Pattern::compile(r"^[A-Z][A-Za-z0-9]*$").expect("valid")),
                name_template: "{{pascal(action)}}".to_owned(),
                kind: KindFilter::Any,
                signature_hint: None,
            },
        };
        let engine = NamingEngine::from_rule(&rule).expect("is a naming rule");

        assert!(
            engine.applies_to(&path("src/Entities/Order/insert.ts")),
            "`Order` is an entity directory"
        );
        assert!(
            !engine.applies_to(&path("src/Entities/_helpers/insert.ts")),
            "`_helpers` is not, and the template would render happily without \
             the match -- it never asks the directory anything"
        );
    }

    /// A file at the repository root has no directory to offer, so a rule that
    /// asks about one does not reach it. Reached through `describe`, which is
    /// the command an agent calls before writing a file anywhere.
    #[test]
    fn a_file_with_no_directory_is_outside_a_rule_that_asks_about_one() {
        let rule = CompiledRule {
            id: RuleId::new("root-level").expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(["."]).expect("valid scope"),
            kind: CompiledRuleKind::Naming {
                file_pattern: Pattern::compile(r"^(?<action>[a-z]+)\.ts$").expect("valid"),
                dir_pattern: Some(Pattern::compile(r"^(?<entity>[A-Za-z]+)$").expect("valid")),
                name_template: "{{pascal(entity)}}{{pascal(action)}}".to_owned(),
                kind: KindFilter::Any,
                signature_hint: None,
            },
        };
        let engine = NamingEngine::from_rule(&rule).expect("is a naming rule");

        assert!(!engine.applies_to(&path("index.ts")));
        assert!(engine.describe_expectation(&path("index.ts")).is_empty());
    }

    /// The directory groups reach `signature_hint` too. It is never verified,
    /// but `scaffold` prints it, and printing `{{pascal(entity)}}` at someone
    /// is showing them our internals.
    #[test]
    fn the_signature_hint_sees_the_directory_groups_as_well() {
        let rule = CompiledRule {
            id: RuleId::new("hinted").expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile(["src/Entities/*"]).expect("valid scope"),
            kind: CompiledRuleKind::Naming {
                file_pattern: Pattern::compile(r"^(?<action>[a-z-]+)\.ts$").expect("valid"),
                dir_pattern: Some(Pattern::compile(r"^(?<entity>[A-Za-z]+)$").expect("valid")),
                name_template: "{{pascal(entity)}}{{pascal(action)}}".to_owned(),
                kind: KindFilter::Any,
                signature_hint: Some(
                    "function {{pascal(entity)}}{{pascal(action)}}(input: {{pascal(entity)}}): void"
                        .to_owned(),
                ),
            },
        };
        let engine = NamingEngine::from_rule(&rule).expect("is a naming rule");

        let [expectation] = engine
            .describe_expectation(&path("src/Entities/Order/insert.ts"))
            .try_into()
            .expect("one expectation");
        let Expectation::RequiredExport {
            name,
            signature_hint,
            ..
        } = expectation
        else {
            panic!("naming describes a required export");
        };

        assert_eq!(name, "OrderInsert");
        assert_eq!(
            signature_hint.as_deref(),
            Some("function OrderInsert(input: Order): void")
        );
    }
}
