//! The `call-matches-export` rule: two vocabularies that have to agree.
//!
//! The seam a Tauri application is joined by, and the first rule here that no
//! single file can answer. `invoke("save_document")` in the webview and
//! `#[tauri::command] fn save_document` in the backend are the same edge, and
//! there is **no import between them** — the coupling is a string on one side
//! and an attribute on the other, in different languages, checked by nothing
//! until somebody clicks the button.
//!
//! # Not a Tauri rule
//!
//! A framework in the engine is a framework the engine has to keep up with,
//! and archwarden's rules are framework-agnostic everywhere else. The shape is
//! general: a callee whose argument names something, a scope where the
//! declarations live, and an attribute marking one. `t("checkout.title")`
//! against a translation catalogue is the same question, and so is a feature
//! flag key.
//!
//! # Two directions, and only one of them is safe by default
//!
//! A call naming nothing is unambiguous: the name is not there, and a typo or
//! a rename on the other side is the cause.
//!
//! A declaration nobody calls is not. archwarden reads the languages it has
//! front-ends for, and a command called from one it does not read looks
//! identical to a command nobody calls. So `report_uncalled` is off unless a
//! configuration turns it on, and `docs/RULES.md` says why.

use archwarden_core::{
    compiled::{CompiledRule, CompiledRuleKind},
    facts::FileFacts,
    finding::{Expectation, Finding, Observed},
    ids::{ModuleId, RuleId},
    level::Level,
    path::RepoRelPath,
    scope::Scope,
    traits::{FactsNeeded, RepositoryContext, RuleEngine},
};

/// A compiled `call-matches-export` rule.
#[derive(Debug, Clone)]
pub struct CallMatchesExportEngine {
    id: RuleId,
    module: Option<ModuleId>,
    level: Level,
    scope: Scope,
    callee: String,
    argument: usize,
    declared_in: Scope,
    attribute: Option<String>,
    report_uncalled: bool,
}

impl CallMatchesExportEngine {
    /// Builds an engine from a compiled rule.
    ///
    /// Returns `None` for a rule of any other kind.
    #[must_use]
    pub fn from_rule(rule: &CompiledRule) -> Option<Self> {
        let CompiledRuleKind::CallMatchesExport {
            callee,
            argument,
            declared_in,
            attribute,
            report_uncalled,
        } = &rule.kind
        else {
            return None;
        };

        Some(Self::build(
            rule,
            callee,
            *argument,
            declared_in,
            attribute.as_deref(),
            *report_uncalled,
        ))
    }

    /// Builds an engine from a rule whose kind is already known.
    #[must_use]
    pub(crate) fn build(
        rule: &CompiledRule,
        callee: &str,
        argument: usize,
        declared_in: &Scope,
        attribute: Option<&str>,
        report_uncalled: bool,
    ) -> Self {
        Self {
            id: rule.id.clone(),
            module: rule.module.clone(),
            level: rule.level,
            scope: rule.scope.clone(),
            callee: callee.to_owned(),
            argument,
            declared_in: declared_in.clone(),
            attribute: attribute.map(ToOwned::to_owned),
            report_uncalled,
        }
    }

    /// Whether a file sits in the scope the calls are read from.
    fn calls_here(&self, path: &RepoRelPath) -> bool {
        self.scope.contains_file(path.as_path())
    }

    /// Whether a file sits in the scope the declarations live in.
    fn declares_here(&self, path: &RepoRelPath) -> bool {
        self.declared_in.contains_file(path.as_path())
    }

    /// The names this file declares, by the rule's terms.
    ///
    /// An export with no name declares nothing anybody can call for, so it is
    /// not one — the same reading `naming` gives an anonymous default.
    fn declared_by<'a>(&'a self, facts: &'a FileFacts) -> impl Iterator<Item = &'a str> + 'a {
        facts
            .exports
            .iter()
            .filter(move |export| match &self.attribute {
                Some(wanted) => export.attributes.iter().any(|held| held == wanted),
                None => true,
            })
            .filter_map(|export| export.name.as_deref())
    }

    /// The names this file's calls ask for, with the span of the call.
    ///
    /// A call whose naming argument is not a string literal is skipped rather
    /// than guessed at. `invoke(command)` names something the reader cannot
    /// see, and reporting it as naming nothing would be reporting a variable
    /// as a typo.
    fn named_by<'a>(
        &'a self,
        facts: &'a FileFacts,
    ) -> impl Iterator<Item = (&'a str, archwarden_core::facts::Span)> + 'a {
        facts
            .calls
            .iter()
            .filter(move |call| call.callee == self.callee)
            .filter_map(move |call| {
                let named = call.arguments.get(self.argument)?.as_deref()?;
                Some((named, call.span))
            })
    }

    fn dangling(
        &self,
        path: &RepoRelPath,
        named: &str,
        span: archwarden_core::facts::Span,
    ) -> Finding {
        Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: path.clone(),
            span: Some(span),
            observed: Observed::CallNamesNothing {
                callee: self.callee.clone(),
                named: named.to_owned(),
            },
            expected: Expectation::DeclaredName {
                named: named.to_owned(),
                attribute: self.attribute.clone(),
            },
        }
    }

    fn uncalled(&self, path: &RepoRelPath, named: &str) -> Finding {
        Finding {
            rule_id: self.id.clone(),
            module_id: self.module.clone(),
            level: self.level,
            path: path.clone(),
            span: None,
            observed: Observed::NothingCallsIt {
                named: named.to_owned(),
            },
            expected: Expectation::CallNaming {
                named: named.to_owned(),
                callee: self.callee.clone(),
            },
        }
    }
}

impl RuleEngine for CallMatchesExportEngine {
    fn id(&self) -> &RuleId {
        &self.id
    }

    fn module(&self) -> Option<&ModuleId> {
        self.module.as_ref()
    }

    fn level(&self) -> Level {
        self.level
    }

    /// No file *individually* satisfies or violates this rule, so nothing is
    /// reported against a path by the per-file pass.
    ///
    /// `describe` and the pre-write hook answer through this, and the honest
    /// answer is that writing one file cannot break an agreement between two
    /// vocabularies -- the other half is somewhere else, and may not exist yet.
    fn applies_to(&self, path: &RepoRelPath) -> bool {
        let _ = path;
        false
    }

    fn describe_expectation(&self, path: &RepoRelPath) -> Vec<Expectation> {
        let _ = path;
        Vec::new()
    }

    fn needs_facts(&self) -> FactsNeeded {
        FactsNeeded::Code
    }

    fn needs_repository(&self) -> bool {
        true
    }

    /// Both directions, from one pass over the files.
    ///
    /// The declared set is built first and whole, because a call in the first
    /// file may name a command declared in the last one — answering per file
    /// in walk order would report every forward reference as dangling.
    fn check_repository(&self, ctx: RepositoryContext<'_>) -> Vec<Finding> {
        let mut declared: std::collections::BTreeMap<&str, &RepoRelPath> =
            std::collections::BTreeMap::new();
        for (path, facts) in ctx.files {
            if self.declares_here(path) {
                for name in self.declared_by(facts) {
                    declared.entry(name).or_insert(path);
                }
            }
        }

        let mut findings = Vec::new();
        let mut called: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();

        for (path, facts) in ctx.files {
            if !self.calls_here(path) {
                continue;
            }
            for (named, span) in self.named_by(facts) {
                called.insert(named);
                if !declared.contains_key(named) {
                    findings.push(self.dangling(path, named, span));
                }
            }
        }

        if self.report_uncalled {
            for (named, path) in &declared {
                if !called.contains(named) {
                    findings.push(self.uncalled(path, named));
                }
            }
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::CompiledRuleKind,
        facts::{CallFact, ExportFact, ExportKind, ExportTags, Span, Visibility},
        hash::ContentHash,
    };

    fn engine(attribute: Option<&str>, report_uncalled: bool) -> CallMatchesExportEngine {
        let rule = CompiledRule {
            id: RuleId::new("ipc").expect("valid id"),
            module: None,
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["src/**"]).expect("valid scope"),
            kind: CompiledRuleKind::Frozen,
        };

        CallMatchesExportEngine::build(
            &rule,
            "invoke",
            0,
            &Scope::compile(["backend/**"]).expect("valid scope"),
            attribute,
            report_uncalled,
        )
    }

    fn calling(path: &str, named: &[Option<&str>]) -> (RepoRelPath, FileFacts) {
        let path = RepoRelPath::new(path).expect("a path");
        let mut facts = FileFacts::unparsed(path.clone(), ContentHash::of(b""));
        facts.calls = named
            .iter()
            .map(|name| CallFact {
                callee: "invoke".to_owned(),
                arguments: vec![name.map(ToOwned::to_owned)],
                options: Vec::new(),
                span: Span::new(0, 1),
            })
            .collect();
        (path, facts)
    }

    fn declaring(path: &str, names: &[(&str, &[&str])]) -> (RepoRelPath, FileFacts) {
        let path = RepoRelPath::new(path).expect("a path");
        let mut facts = FileFacts::unparsed(path.clone(), ContentHash::of(b""));
        facts.exports = names
            .iter()
            .map(|(name, attributes)| ExportFact {
                name: Some((*name).to_owned()),
                tags: ExportTags::only(ExportKind::Fn),
                attributes: attributes.iter().map(|a| (*a).to_owned()).collect(),
                visibility: Visibility::Public,
                is_default: false,
                reexport_from: None,
                forwards: None,
                annotations: Vec::new(),
                returns: None,
                span: Span::new(0, 1),
            })
            .collect();
        (path, facts)
    }

    fn judge(engine: &CallMatchesExportEngine, files: &[(RepoRelPath, FileFacts)]) -> Vec<Finding> {
        engine.check_repository(RepositoryContext { files })
    }

    /// A name declared anywhere in the other scope satisfies a call, and a name
    /// declared nowhere does not.
    ///
    /// The whole rule. There is no import between the two halves, and until
    /// this nothing compared them.
    #[test]
    fn a_call_naming_nothing_is_reported_and_one_naming_something_is_not() {
        let engine = engine(Some("tauri::command"), false);
        let files = vec![
            calling(
                "src/api.ts",
                &[Some("save_document"), Some("purge_document")],
            ),
            declaring(
                "backend/commands.rs",
                &[("save_document", &["tauri::command"])],
            ),
        ];

        let findings = judge(&engine, &files);

        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(matches!(
            &findings[0].observed,
            Observed::CallNamesNothing { named, .. } if named == "purge_document"
        ));
        assert_eq!(
            findings[0].path.as_str(),
            "src/api.ts",
            "reported where it is called"
        );
    }

    /// The declared set is built whole before any call is judged.
    ///
    /// A call in the first file may name something declared in the last one.
    /// Answering in walk order would report every forward reference as
    /// dangling, which on a real repository is most of them.
    #[test]
    fn a_call_is_satisfied_by_a_declaration_the_walk_reaches_later() {
        let engine = engine(Some("tauri::command"), false);
        let files = vec![
            calling("src/api.ts", &[Some("save_document")]),
            declaring(
                "backend/z_last.rs",
                &[("save_document", &["tauri::command"])],
            ),
        ];

        assert!(judge(&engine, &files).is_empty(), "order does not decide");
    }

    /// An export without the attribute is not a declaration.
    ///
    /// A `pub fn` beside a command is an ordinary function, and counting it
    /// would make the rule accept a call naming something no framework will
    /// ever dispatch.
    #[test]
    fn an_export_without_the_attribute_does_not_declare_anything() {
        let engine = engine(Some("tauri::command"), false);
        let files = vec![
            calling("src/api.ts", &[Some("helper")]),
            declaring("backend/commands.rs", &[("helper", &[])]),
        ];

        assert_eq!(
            judge(&engine, &files).len(),
            1,
            "the attribute is the marker"
        );
    }

    /// Naming no attribute accepts every export in scope.
    ///
    /// What a translation catalogue wants, and what a command surface does
    /// not.
    #[test]
    fn a_rule_naming_no_attribute_accepts_any_export_in_scope() {
        let engine = engine(None, false);
        let files = vec![
            calling("src/api.ts", &[Some("checkout.title")]),
            declaring("backend/strings.rs", &[("checkout.title", &[])]),
        ];

        assert!(judge(&engine, &files).is_empty());
    }

    /// A call whose name is not a literal is skipped, not reported.
    ///
    /// `invoke(command)` names something the reader cannot see. Reporting it as
    /// naming nothing would report a variable as a typo -- the same argument
    /// `has_opaque_import` makes about a dynamic import.
    #[test]
    fn a_call_named_by_a_variable_is_not_reported() {
        let engine = engine(Some("tauri::command"), false);
        let files = vec![
            calling("src/api.ts", &[None]),
            declaring(
                "backend/commands.rs",
                &[("save_document", &["tauri::command"])],
            ),
        ];

        assert!(judge(&engine, &files).is_empty());
    }

    /// A declaration nobody calls is reported only when asked for.
    ///
    /// The asymmetry is the point. archwarden reads the languages it has
    /// front-ends for, and a command called from one it does not read looks
    /// exactly like a command nobody calls -- so the default cannot be to
    /// report it.
    #[test]
    fn an_uncalled_declaration_is_reported_only_when_the_rule_asks() {
        let files = vec![
            calling("src/api.ts", &[Some("save_document")]),
            declaring(
                "backend/commands.rs",
                &[
                    ("save_document", &["tauri::command"]),
                    ("delete_document", &["tauri::command"]),
                ],
            ),
        ];

        assert!(
            judge(&engine(Some("tauri::command"), false), &files).is_empty(),
            "silent by default"
        );

        let findings = judge(&engine(Some("tauri::command"), true), &files);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(matches!(
            &findings[0].observed,
            Observed::NothingCallsIt { named } if named == "delete_document"
        ));
        assert_eq!(
            findings[0].path.as_str(),
            "backend/commands.rs",
            "reported where it is declared"
        );
    }

    /// Calls outside the scope are not read, and declarations outside theirs do
    /// not count.
    ///
    /// Two scopes, asserted separately: one rule with both collapsed into one
    /// would accept a repository calling and declaring in the same place, which
    /// is not the seam this is about.
    #[test]
    fn each_scope_is_honoured_on_its_own() {
        let engine = engine(Some("tauri::command"), false);

        let outside_call = vec![
            calling("elsewhere/api.ts", &[Some("nothing_declares_this")]),
            declaring("backend/commands.rs", &[("x", &["tauri::command"])]),
        ];
        assert!(
            judge(&engine, &outside_call).is_empty(),
            "a call outside the scope is not this rule's business"
        );

        let outside_declaration = vec![
            calling("src/api.ts", &[Some("save_document")]),
            declaring(
                "elsewhere/commands.rs",
                &[("save_document", &["tauri::command"])],
            ),
        ];
        assert_eq!(
            judge(&engine, &outside_declaration).len(),
            1,
            "a declaration outside `declared_in` declares nothing here"
        );
    }

    /// A rule of this kind is recognised, and one of another kind is not.
    ///
    /// `from_rule` is the path every surface but the run takes, and returning
    /// `None` for a rule that *is* one would make the rule invisible to
    /// `describe`, `explain` and the guide while `check` still fired it.
    #[test]
    fn a_rule_of_this_kind_is_recognised_and_another_is_not() {
        let mut rule = CompiledRule {
            id: RuleId::new("ipc").expect("valid id"),
            module: Some(ModuleId::new("webview").expect("valid id")),
            why: None,
            module_why: None,
            decision: None,
            imports: None,
            level: Level::Error,
            scope: Scope::compile(["src/**"]).expect("valid scope"),
            kind: CompiledRuleKind::CallMatchesExport {
                callee: "invoke".to_owned(),
                argument: 2,
                declared_in: Scope::compile(["backend/**"]).expect("valid scope"),
                attribute: Some("tauri::command".to_owned()),
                report_uncalled: true,
            },
        };

        let engine = CallMatchesExportEngine::from_rule(&rule).expect("is one");
        assert_eq!(engine.id().as_str(), "ipc");
        assert_eq!(
            engine.module().map(ModuleId::as_str),
            Some("webview"),
            "the module it was declared in reaches the finding"
        );
        assert_eq!(engine.argument, 2, "the field is carried, not defaulted");
        assert!(engine.report_uncalled);

        rule.kind = CompiledRuleKind::Frozen;
        assert!(
            CallMatchesExportEngine::from_rule(&rule).is_none(),
            "and another kind is not one"
        );
    }

    /// The rule is answered once about everything, and claims no file.
    #[test]
    fn it_asks_about_the_repository_and_claims_no_file() {
        let engine = engine(None, false);

        assert!(engine.needs_repository());
        assert!(!engine.needs_graph(), "it needs no resolution at all");
        assert!(!engine.applies_to(&RepoRelPath::new("src/api.ts").expect("a path")));
    }
}
