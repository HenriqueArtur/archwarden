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

        self.render(&self.name_template, name)
    }

    /// Renders a template against the filename's capture groups.
    fn render(&self, template: &str, filename: &str) -> Option<String> {
        template::render(template, |group| {
            self.file_pattern
                .capture(filename, group)
                .map(ToOwned::to_owned)
        })
        .ok()
    }

    /// The expectation for a file, with both templates rendered.
    ///
    /// `signature_hint` goes through the same rendering as the name. It is
    /// never verified, but it is *shown* -- by `scaffold`, and in a finding --
    /// and showing `{{pascal(name)}}` to a user is showing them our internals.
    fn expectation(&self, path: &RepoRelPath) -> Option<Expectation> {
        let filename = path.file_name()?;
        Some(Expectation::RequiredExport {
            kind: self.kind.clone(),
            name: self.required_name(path)?,
            signature_hint: self
                .signature_hint
                .as_ref()
                .and_then(|hint| self.render(hint, filename)),
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
}
