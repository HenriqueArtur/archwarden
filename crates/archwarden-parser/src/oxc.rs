//! The `oxc` front-end.
//!
//! Extraction is split the way `oxc` is: the module record answers the
//! *module-semantics* questions -- what is exported, under what name, is it a
//! default, does it come from elsewhere -- and the AST answers the
//! *declaration-form* question that decision 9 is about, where an arrow
//! function is not a function.
//!
//! Joining the two by local binding name is what makes `export { Local }`
//! work: the module record says `Local` is exported, and the AST says how
//! `Local` was declared, even though the two appear in different statements.

use std::collections::HashMap;

use archwarden_core::{
    facts::{CallFact, ExportFact, ExportKind, ExportTags, FileFacts, ImportFact, Span},
    hash::ContentHash,
    path::RepoRelPath,
    traits::Parser as ParserTrait,
};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Declaration, Expression, ImportExpression, Program, Statement, TSImportType,
    VariableDeclarationKind,
};
use oxc_ast_visit::Visit;
use oxc_span::{GetSpan, SourceType};

/// Why a file could not be parsed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ParseError {
    /// The extension is not one this front-end handles.
    #[error("`{path}` is not a JavaScript or TypeScript file")]
    UnsupportedExtension {
        /// The file.
        path: RepoRelPath,
    },

    /// The source does not parse.
    #[error("`{path}` does not parse: {message}")]
    Unparsable {
        /// The file.
        path: RepoRelPath,
        /// The first thing the parser objected to.
        message: String,
    },
}

/// The default JS/TS front-end.
#[derive(Debug, Clone, Copy, Default)]
pub struct OxcParser;

impl ParserTrait for OxcParser {
    type Error = ParseError;

    fn parse(
        &self,
        path: &RepoRelPath,
        source: &str,
        content_hash: ContentHash,
    ) -> Result<FileFacts, Self::Error> {
        let source_type = SourceType::from_path(path.as_path())
            .map_err(|_| ParseError::UnsupportedExtension { path: path.clone() })?;

        Self::parse_as(path, source, source_type, content_hash)
    }
}

impl OxcParser {
    /// Extracts facts from source whose kind the caller decides.
    ///
    /// `SourceType::from_path` answers for a file archwarden reads whole. A
    /// front-end for a format that *embeds* a module -- an `.astro` fence -- has
    /// a path the extension list rejects and a slice it already knows is
    /// TypeScript, so the decision moves to a parameter rather than being taken
    /// from a name that cannot answer. Issue #13.
    ///
    /// # Errors
    /// When the slice does not parse.
    pub fn parse_as(
        path: &RepoRelPath,
        source: &str,
        source_type: SourceType,
        content_hash: ContentHash,
    ) -> Result<FileFacts, ParseError> {
        let allocator = Allocator::default();
        let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();

        // `panicked` is the hard failure: oxc gave up and the tree is not
        // usable. Recoverable diagnostics leave a tree good enough to extract
        // from, and a linter that refused every file with a recoverable
        // complaint would be refusing files `tsc` accepts.
        if parsed.panicked {
            return Err(ParseError::Unparsable {
                path: path.clone(),
                message: parsed
                    .diagnostics
                    .first()
                    .map_or_else(|| "unknown parse failure".to_owned(), ToString::to_string),
            });
        }

        let declaration_tags = declaration_tags(&parsed.program);
        let declaration_annotations = declaration_annotations(&parsed.program, source);
        let forwarded = forwarded_bindings(&parsed.program);

        let (imports, has_opaque_import) = imports(&parsed.module_record, &parsed.program);

        Ok(FileFacts {
            path: path.clone(),
            content_hash,
            imports,
            exports: exports(
                &parsed.module_record,
                &declaration_tags,
                &declaration_annotations,
                &forwarded,
            ),
            calls: calls(&parsed.program),
            has_opaque_import,
        })
    }
}

/// Maps every top-level binding that is nothing but a forward of another one.
///
/// Three shapes, which are the three ways a file can hold a name and add
/// nothing to it:
///
/// - `const A = B` and `type A = B` — an alias. The name changed and nothing
///   else did.
/// - `function f(a, b) { return g(a, b); }` — a wrapper whose whole body is
///   one call, taking its own parameters in order.
///
/// Deliberately *syntactic*. "Same signature" in the type sense would need the
/// file on the other side and its types, which is cross-file analysis
/// `docs/RULES.md` keeps the file-local rules away from. A wrapper that
/// reorders arguments, drops one, or supplies a default is doing something,
/// and none of those match here.
fn forwarded_bindings(program: &Program<'_>) -> HashMap<String, String> {
    let mut forwards = HashMap::new();

    for statement in &program.body {
        let declaration = match statement {
            Statement::ExportNamedDeclaration(export) => export.declaration.as_ref(),
            other => other.as_declaration(),
        };
        let Some(declaration) = declaration else {
            continue;
        };
        record_forward(declaration, &mut forwards);
    }

    forwards
}

/// Records one declaration if it forwards another binding.
fn record_forward(declaration: &Declaration<'_>, forwards: &mut HashMap<String, String>) {
    match declaration {
        // `export const planToJson = planToJsonShared`
        Declaration::VariableDeclaration(variable) => {
            for declarator in &variable.declarations {
                let (Some(identifier), Some(Expression::Identifier(source))) =
                    (declarator.id.get_binding_identifier(), &declarator.init)
                else {
                    continue;
                };
                forwards.insert(identifier.name.to_string(), source.name.to_string());
            }
        }

        // `export type PlanJson = PlanJsonShared`, and only that: a type with
        // arguments (`Partial<X>`) or a union is a type being built, not a
        // name being changed.
        Declaration::TSTypeAliasDeclaration(alias) => {
            if let oxc_ast::ast::TSType::TSTypeReference(reference) = &alias.type_annotation
                && reference.type_arguments.is_none()
                && let oxc_ast::ast::TSTypeName::IdentifierReference(source) = &reference.type_name
            {
                forwards.insert(alias.id.name.to_string(), source.name.to_string());
            }
        }

        // `export function isFlowGraphInvalid(nodes, edges) {
        //    return isFlowGraphInvalidShared(nodes, edges);
        //  }`
        Declaration::FunctionDeclaration(function) => {
            let (Some(identifier), Some(body)) = (&function.id, &function.body) else {
                return;
            };
            if let Some(callee) = single_forwarding_return(body, &function.params) {
                forwards.insert(identifier.name.to_string(), callee);
            }
        }

        _ => {}
    }
}

/// The callee of a body that is exactly `return f(<own parameters, in order>)`.
fn single_forwarding_return(
    body: &oxc_ast::ast::FunctionBody<'_>,
    params: &oxc_ast::ast::FormalParameters<'_>,
) -> Option<String> {
    let [Statement::ReturnStatement(returned)] = body.statements.as_slice() else {
        return None;
    };
    let Some(Expression::CallExpression(call)) = &returned.argument else {
        return None;
    };
    let Expression::Identifier(callee) = &call.callee else {
        return None;
    };
    if call.arguments.len() != params.items.len() {
        return None;
    }

    // Its own parameters, in order. Anything else — a reordering, a literal, a
    // default supplied — is the file doing something, which is exactly what
    // this must not flag.
    for (argument, parameter) in call.arguments.iter().zip(&params.items) {
        let (Some(Expression::Identifier(passed)), Some(declared)) = (
            argument.as_expression(),
            parameter.pattern.get_binding_identifier(),
        ) else {
            return None;
        };
        if passed.name != declared.name {
            return None;
        }
    }

    Some(callee.name.to_string())
}

/// Maps every top-level binding to how it was declared.
///
/// Includes bindings that are not exported, because `export { Local }` refers
/// to one declared elsewhere in the file.
fn declaration_tags(program: &Program<'_>) -> HashMap<String, ExportTags> {
    let mut tags = HashMap::new();

    for statement in &program.body {
        match statement {
            Statement::ExportNamedDeclaration(export) => {
                if let Some(declaration) = &export.declaration {
                    record_declaration(declaration, &mut tags);
                }
            }
            Statement::ExportDefaultDeclaration(export) => {
                record_default(&export.declaration, &mut tags);
            }
            Statement::VariableDeclaration(_)
            | Statement::FunctionDeclaration(_)
            | Statement::ClassDeclaration(_)
            | Statement::TSTypeAliasDeclaration(_)
            | Statement::TSInterfaceDeclaration(_)
            | Statement::TSEnumDeclaration(_) => {
                if let Some(declaration) = statement.as_declaration() {
                    record_declaration(declaration, &mut tags);
                }
            }
            _ => {}
        }
    }

    tags
}

fn record_declaration(declaration: &Declaration<'_>, tags: &mut HashMap<String, ExportTags>) {
    match declaration {
        Declaration::VariableDeclaration(variable) => {
            let binding = match variable.kind {
                // `using` and `await using` are const bindings with a
                // disposal hook; for "how was this declared" they are const.
                // Listed rather than caught by a wildcard, so a variant added
                // to the language fails to compile here instead of silently
                // becoming a const.
                VariableDeclarationKind::Const
                | VariableDeclarationKind::Using
                | VariableDeclarationKind::AwaitUsing => ExportKind::Const,
                VariableDeclarationKind::Let => ExportKind::Let,
                VariableDeclarationKind::Var => ExportKind::Var,
            };

            for declarator in &variable.declarations {
                let Some(identifier) = declarator.id.get_binding_identifier() else {
                    continue;
                };

                let mut set = ExportTags::only(binding);
                // The distinction decision 9 exists for: an arrow function is
                // not a `function`, so a rule can require one and reject the
                // other.
                if matches!(
                    declarator.init,
                    Some(Expression::ArrowFunctionExpression(_))
                ) {
                    set = set.with(ExportKind::Arrow);
                }
                tags.insert(identifier.name.to_string(), set);
            }
        }

        Declaration::FunctionDeclaration(function) => {
            if let Some(identifier) = &function.id {
                tags.insert(
                    identifier.name.to_string(),
                    ExportTags::only(ExportKind::Function),
                );
            }
        }

        Declaration::ClassDeclaration(class) => {
            if let Some(identifier) = &class.id {
                tags.insert(
                    identifier.name.to_string(),
                    ExportTags::only(ExportKind::Class),
                );
            }
        }

        Declaration::TSTypeAliasDeclaration(alias) => {
            tags.insert(
                alias.id.name.to_string(),
                ExportTags::only(ExportKind::Type),
            );
        }

        Declaration::TSInterfaceDeclaration(interface) => {
            tags.insert(
                interface.id.name.to_string(),
                ExportTags::only(ExportKind::Interface),
            );
        }

        Declaration::TSEnumDeclaration(enumeration) => {
            tags.insert(
                enumeration.id.name.to_string(),
                ExportTags::only(ExportKind::Enum),
            );
        }

        _ => {}
    }
}

/// Maps every top-level binding to the types it writes down about itself.
///
/// A second pass over the same statements rather than another field threaded
/// through [`declaration_tags`]: that one answers "how was this declared", this
/// one answers "what does it claim to be", and the two are separately
/// interesting.
fn declaration_annotations(program: &Program<'_>, source: &str) -> HashMap<String, Vec<String>> {
    let mut annotations = HashMap::new();

    for statement in &program.body {
        let declaration = match statement {
            Statement::ExportNamedDeclaration(export) => export.declaration.as_ref(),
            Statement::VariableDeclaration(_) | Statement::ClassDeclaration(_) => {
                statement.as_declaration()
            }
            _ => None,
        };

        if let Some(declaration) = declaration {
            record_annotations(declaration, source, &mut annotations);
        }
    }

    annotations
}

/// Records the annotations of one declaration, for the forms that have any.
///
/// Two forms, because those are the two places TypeScript lets a declaration
/// name the contract it is written against: the annotation on a binding, and
/// a class's `implements` clauses. Everything else is left absent rather than
/// guessed at -- see [`ExportFact::annotations`].
fn record_annotations(
    declaration: &Declaration<'_>,
    source: &str,
    annotations: &mut HashMap<String, Vec<String>>,
) {
    match declaration {
        Declaration::VariableDeclaration(variable) => {
            for declarator in &variable.declarations {
                let (Some(identifier), Some(annotation)) = (
                    declarator.id.get_binding_identifier(),
                    &declarator.type_annotation,
                ) else {
                    continue;
                };

                // The `TSTypeAnnotation` span opens at the `:`; the type's own
                // span is what a config author writes into a rule.
                if let Some(text) = collapsed(source, annotation.type_annotation.span()) {
                    annotations.insert(identifier.name.to_string(), vec![text]);
                }
            }
        }

        Declaration::ClassDeclaration(class) => {
            let Some(identifier) = &class.id else {
                return;
            };

            let clauses: Vec<String> = class
                .implements
                .iter()
                .filter_map(|clause| collapsed(source, clause.span))
                .collect();

            if !clauses.is_empty() {
                annotations.insert(identifier.name.to_string(), clauses);
            }
        }

        _ => {}
    }
}

/// The source between two byte offsets, with runs of whitespace collapsed.
///
/// A type broken over three lines by a formatter and the same type on one line
/// are the same annotation, and a rule that disagreed with a formatter is a
/// rule nobody keeps. Collapsed rather than stripped because this text is
/// printed back at a user in a finding; the comparison that consumes it ignores
/// spacing entirely, on both sides.
fn collapsed(source: &str, span: oxc_span::Span) -> Option<String> {
    let text = source.get(span.start as usize..span.end as usize)?;

    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!joined.is_empty()).then_some(joined)
}

fn record_default(
    declaration: &oxc_ast::ast::ExportDefaultDeclarationKind<'_>,
    tags: &mut HashMap<String, ExportTags>,
) {
    use oxc_ast::ast::ExportDefaultDeclarationKind as Kind;

    match declaration {
        Kind::FunctionDeclaration(function) => {
            if let Some(identifier) = &function.id {
                tags.insert(
                    identifier.name.to_string(),
                    ExportTags::only(ExportKind::Function),
                );
            }
        }
        Kind::ClassDeclaration(class) => {
            if let Some(identifier) = &class.id {
                tags.insert(
                    identifier.name.to_string(),
                    ExportTags::only(ExportKind::Class),
                );
            }
        }
        _ => {}
    }
}

fn exports(
    record: &oxc_syntax::module_record::ModuleRecord<'_>,
    declaration_tags: &HashMap<String, ExportTags>,
    declaration_annotations: &HashMap<String, Vec<String>>,
    forwarded: &HashMap<String, String>,
) -> Vec<ExportFact> {
    let mut facts = Vec::new();

    for entry in &record.local_export_entries {
        let local = local_name(&entry.local_name);
        let exported = export_name(&entry.export_name);

        facts.push(ExportFact {
            tags: local
                .as_ref()
                .and_then(|name| declaration_tags.get(name).copied())
                .unwrap_or_else(ExportTags::none),
            // Keyed by the *local* name, like the tags: `export { Local as
            // Public }` annotates `Local`, and what the rule asks about is
            // `Public`.
            annotations: local
                .as_ref()
                .and_then(|name| declaration_annotations.get(name).cloned())
                .unwrap_or_default(),
            is_default: entry.export_name.is_default(),
            // Either the binding is an alias or a wrapper this file declared,
            // or `export { X }` names something the file never declared — in
            // which case `X` came in through an import and the file is holding
            // the name and nothing else.
            forwards: local.as_ref().and_then(|name| {
                forwarded
                    .get(name)
                    .cloned()
                    .or_else(|| (!declaration_tags.contains_key(name)).then(|| name.clone()))
            }),
            name: exported,
            reexport_from: None,
            span: span_of(entry.span),
        });
    }

    // `export { Foo } from './x'`: the name is exported here, but what it was
    // declared as lives in another file. Tagged `reexport` rather than
    // guessed, which is what lets the naming rule say so instead of failing
    // for the wrong reason.
    for entry in &record.indirect_export_entries {
        facts.push(ExportFact {
            name: export_name(&entry.export_name),
            tags: ExportTags::only(ExportKind::Reexport),
            // The declaration is in another file, so whether it annotates
            // anything is not knowable from here -- the same reason the kind
            // is `reexport` rather than guessed at.
            annotations: Vec::new(),
            is_default: entry.export_name.is_default(),
            reexport_from: entry
                .module_request
                .as_ref()
                .map(|request| request.name.to_string()),
            // Forwards by construction: the file holds the name and nothing
            // else about it.
            forwards: export_name(&entry.export_name),
            span: span_of(entry.span),
        });
    }

    facts
}

/// The name a symbol is exported *as*, which is what an importer writes.
///
/// A default has no such name: `import Whatever from './x'` binds whatever the
/// importer chose, which is why decision 9 says a default never satisfies a
/// named `must_export`.
fn export_name(name: &oxc_syntax::module_record::ExportExportName<'_>) -> Option<String> {
    match name {
        oxc_syntax::module_record::ExportExportName::Name(named) => Some(named.name.to_string()),
        _ => None,
    }
}

/// The local binding an export refers to, which is the key the AST's
/// declaration map is built on.
fn local_name(name: &oxc_syntax::module_record::ExportLocalName<'_>) -> Option<String> {
    match name {
        oxc_syntax::module_record::ExportLocalName::Name(named)
        | oxc_syntax::module_record::ExportLocalName::Default(named) => {
            Some(named.name.to_string())
        }
        oxc_syntax::module_record::ExportLocalName::Null => None,
    }
}

fn imports(
    record: &oxc_syntax::module_record::ModuleRecord<'_>,
    program: &Program<'_>,
) -> (Vec<ImportFact>, bool) {
    // Grouped by statement, so one `import { A, B } from './x'` is one fact
    // with two names rather than two facts naming the same module.
    let mut by_statement: Vec<ImportFact> = Vec::new();

    for entry in &record.import_entries {
        let specifier = entry.module_request.name.to_string();
        let span = span_of(entry.statement_span);
        let local = entry.local_name.name.to_string();

        if let Some(existing) = by_statement
            .iter_mut()
            .find(|fact| fact.span == span && fact.specifier == specifier)
        {
            existing.names.push(local);
            // A statement is type-only only if every name in it is.
            existing.type_only = existing.type_only && entry.is_type;
        } else {
            by_statement.push(ImportFact {
                specifier,
                resolved: None,
                type_only: entry.is_type,
                names: vec![local],
                span,
            });
        }
    }

    // A bare `import './side-effect'` has no entries, and a re-export's source
    // is an import for the purpose of the graph.
    for (specifier, requests) in &record.requested_modules {
        for request in requests {
            let span = span_of(request.statement_span);
            if by_statement
                .iter()
                .any(|fact| fact.span == span && fact.specifier == specifier.as_str())
            {
                continue;
            }
            by_statement.push(ImportFact {
                specifier: specifier.to_string(),
                resolved: None,
                type_only: request.is_type,
                names: Vec::new(),
                span,
            });
        }
    }

    // The module record covers module *syntax* only. A dynamic `import()` is
    // an ordinary call expression, so it is invisible there and has to come
    // off the AST -- which is how a boundary rule was bypassable by writing
    // `await import('@/domain/user')`. Found by the differential harness
    // against a real monorepo.
    let (dynamic, has_opaque_import) = dynamic_imports(program);
    by_statement.extend(dynamic);

    by_statement.sort_by_key(|fact| (fact.span.start, fact.span.end));
    (by_statement, has_opaque_import)
}

/// Every `import()` in the file whose specifier is written out, and whether
/// any was not.
fn dynamic_imports(program: &Program<'_>) -> (Vec<ImportFact>, bool) {
    let mut collector = DynamicImportCollector::default();
    collector.visit_program(program);
    (collector.imports, collector.opaque)
}

#[derive(Default)]
struct DynamicImportCollector {
    imports: Vec<ImportFact>,
    /// Whether an `import()` named something this cannot read.
    opaque: bool,
}

impl DynamicImportCollector {
    fn record(&mut self, specifier: &str, type_only: bool, span: oxc_span::Span) {
        self.imports.push(ImportFact {
            specifier: specifier.to_owned(),
            resolved: None,
            type_only,
            // A dynamic import binds nothing at the statement level: whatever
            // the caller destructures out of the promise is a separate
            // binding, and `call-obligation` matches call sites rather than
            // import names.
            names: Vec::new(),
            span: span_of(span),
        });
    }
}

impl<'a> Visit<'a> for DynamicImportCollector {
    fn visit_import_expression(&mut self, expression: &ImportExpression<'a>) {
        // Only a literal specifier. `import(name)` and
        // `import(`./locales/${name}`)` name no single module, and inventing
        // one would have a boundary rule report a path nobody wrote. The
        // run's `unresolved` tally is where a user learns it saw less than
        // everything.
        if let Expression::StringLiteral(literal) = &expression.source {
            self.record(literal.value.as_str(), false, expression.span);
        } else {
            // Recorded as a fact about the file rather than as an import,
            // because it is not one: it is the absence of one this cannot see.
            // A rule must not act on it; a caller asking who imports a file
            // must be told it exists.
            self.opaque = true;
        }
        oxc_ast_visit::walk::walk_import_expression(self, expression);
    }

    fn visit_ts_import_type(&mut self, import: &TSImportType<'a>) {
        // `import("./a").Actor` in a type position is a real dependency and an
        // erased one, so it is recorded and marked type-only. A rule with
        // `include_type_only: false` should not see it; one with the default
        // should.
        self.record(import.source.value.as_str(), true, import.span);
        oxc_ast_visit::walk::walk_ts_import_type(self, import);
    }
}

fn calls(program: &Program<'_>) -> Vec<CallFact> {
    let mut collector = CallCollector::default();
    collector.visit_program(program);
    collector.calls
}

#[derive(Default)]
struct CallCollector {
    calls: Vec<CallFact>,
}

impl<'a> Visit<'a> for CallCollector {
    fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
        if let Some(callee) = callee_path(&call.callee) {
            self.calls.push(CallFact {
                callee,
                span: span_of(call.span),
            });
        }
        oxc_ast_visit::walk::walk_call_expression(self, call);
    }
}

/// Renders a callee as a dotted path, when it is one.
///
/// `Event.save` and `logger.audit.write` both resolve; `expect(1).toBe` does
/// not, because its root is a call rather than a name. `docs/RULES.md` matches
/// `must_call.symbol` against exactly this shape, so anything that cannot be
/// written as a symbol path is not recorded rather than being recorded
/// half-formed.
fn callee_path(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::Identifier(identifier) => Some(identifier.name.to_string()),
        Expression::StaticMemberExpression(member) => {
            let object = callee_path(&member.object)?;
            Some(format!("{object}.{}", member.property.name))
        }
        _ => None,
    }
}

fn span_of(span: oxc_span::Span) -> Span {
    Span::new(span.start, span.end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::facts::KindFilter;

    fn parse(name: &str, source: &str) -> FileFacts {
        OxcParser
            .parse(
                &RepoRelPath::new(name).expect("valid path"),
                source,
                ContentHash::of(source.as_bytes()),
            )
            .expect("should parse")
    }

    fn tags_of(facts: &FileFacts, name: &str) -> ExportTags {
        facts
            .named_export(name)
            .unwrap_or_else(|| panic!("no export named {name}"))
            .tags
    }

    fn kinds(facts: &FileFacts, name: &str) -> Vec<&'static str> {
        tags_of(facts, name)
            .iter()
            .map(ExportKind::as_str)
            .collect()
    }

    /// The table in docs/RULES.md, checked against a real parser rather than
    /// against my reading of it.
    #[test]
    fn every_declaration_form_gets_its_documented_tags() {
        let facts = parse(
            "src/x.ts",
            r"
export function Fn() {}
export async function AsyncFn() {}
export function* GenFn() {}
export const Arrow = () => {};
export const AsyncArrow = async () => {};
export const FnExpr = function () {};
export const Value = 42;
export let Mutable = 1;
export var Ancient = 2;
export class Cls {}
export type Alias = string;
export interface Iface {}
export enum Enum { A }
",
        );

        assert_eq!(kinds(&facts, "Fn"), ["function"]);
        assert_eq!(kinds(&facts, "AsyncFn"), ["function"]);
        assert_eq!(kinds(&facts, "GenFn"), ["function"]);
        assert_eq!(kinds(&facts, "Arrow"), ["arrow", "const"]);
        assert_eq!(kinds(&facts, "AsyncArrow"), ["arrow", "const"]);
        assert_eq!(kinds(&facts, "FnExpr"), ["const"]);
        assert_eq!(kinds(&facts, "Value"), ["const"]);
        assert_eq!(kinds(&facts, "Mutable"), ["let"]);
        assert_eq!(kinds(&facts, "Ancient"), ["var"]);
        assert_eq!(kinds(&facts, "Cls"), ["class"]);
        assert_eq!(kinds(&facts, "Alias"), ["type"]);
        assert_eq!(kinds(&facts, "Iface"), ["interface"]);
        assert_eq!(kinds(&facts, "Enum"), ["enum"]);
    }

    /// The distinction the whole tag model exists for.
    #[test]
    fn an_arrow_is_not_a_function() {
        let facts = parse(
            "src/x.ts",
            "export function A() {}\nexport const B = () => {};",
        );

        let callable = KindFilter::OneOf(ExportTags::only(ExportKind::Function));
        assert!(callable.accepts(tags_of(&facts, "A")));
        assert!(
            !callable.accepts(tags_of(&facts, "B")),
            "an arrow must not satisfy `kind: function`"
        );
    }

    /// `export { Local }` refers to a declaration in another statement. The
    /// module record says it is exported; the AST says how it was declared;
    /// joining them by local name is what makes this work.
    #[test]
    fn a_separately_declared_export_keeps_its_declaration_form() {
        let facts = parse(
            "src/x.ts",
            r"
const Value = 42;
function Helper() {}
class Thing {}
export { Value, Helper, Thing };
",
        );

        assert_eq!(kinds(&facts, "Value"), ["const"]);
        assert_eq!(kinds(&facts, "Helper"), ["function"]);
        assert_eq!(kinds(&facts, "Thing"), ["class"]);
    }

    /// A rename keeps the declaration's form under the exported name, since
    /// that is the name an importer sees.
    #[test]
    fn a_renamed_export_is_recorded_under_its_exported_name() {
        let facts = parse(
            "src/x.ts",
            "function internalName() {}\nexport { internalName as PublicName };",
        );

        assert_eq!(kinds(&facts, "PublicName"), ["function"]);
        assert!(facts.named_export("internalName").is_none());
    }

    /// A default export is flagged as one and does not answer to a name
    /// lookup, because its local name does not bind the importer.
    #[test]
    fn a_default_export_is_flagged_and_unnamed() {
        let facts = parse("src/x.ts", "export default function Def() {}");

        assert_eq!(facts.exports.len(), 1);
        let export = facts.exports.first().expect("one export");
        assert!(export.is_default);
        assert!(facts.named_export("Def").is_none());
    }

    #[test]
    fn an_anonymous_default_export_parses() {
        let facts = parse("src/x.ts", "export default class {}");
        assert!(facts.exports.first().expect("one export").is_default);
    }

    /// A re-export is tagged `reexport` rather than guessed at, which is what
    /// lets the naming rule say "not determinable here" instead of failing for
    /// the wrong reason.
    #[test]
    fn a_reexport_is_tagged_as_one_and_names_its_source() {
        let facts = parse("src/x.ts", "export { Foo } from './other';");

        let export = facts.named_export("Foo").expect("Foo is exported");
        assert_eq!(export.tags, ExportTags::only(ExportKind::Reexport));
        assert_eq!(export.reexport_from.as_deref(), Some("./other"));
    }

    /// The fact issue #39 needs: whether the declaration wrote its type down.
    /// Not what that type resolves to -- the token, as the author typed it.
    #[test]
    fn an_annotated_binding_records_the_type_as_written() {
        let facts = parse(
            "src/x.ts",
            "export const AGENT_TOOL: AgentToolModule = { spec: {} };",
        );

        let export = facts.named_export("AGENT_TOOL").expect("is exported");
        assert_eq!(export.annotations, ["AgentToolModule"]);
    }

    /// The case the issue is about: `tsc` is green, archwarden is green, and
    /// the worker dies at boot because nothing ever submitted the object to a
    /// type. The absence has to be visible as an absence.
    #[test]
    fn an_unannotated_binding_records_nothing() {
        let facts = parse("src/x.ts", "export const AGENT_TOOL = { spec: {} };");

        let export = facts.named_export("AGENT_TOOL").expect("is exported");
        assert!(export.annotations.is_empty());
    }

    /// A class names its contract in `implements`, which is the same claim
    /// written where the language writes it. One entry per clause: a class
    /// implementing two interfaces satisfies a rule asking for either, and
    /// joining them into one string would make the rule for `B` fail.
    #[test]
    fn a_class_records_every_implements_clause() {
        let facts = parse(
            "src/x.ts",
            "export class Tool implements AgentToolModule, Disposable {}",
        );

        let export = facts.named_export("Tool").expect("is exported");
        assert_eq!(export.annotations, ["AgentToolModule", "Disposable"]);
    }

    /// A function declares a *return* type, which is a different claim. Reading
    /// it as an annotation would let `function AGENT_TOOL(): AgentToolModule`
    /// satisfy a rule that asked for a module object.
    #[test]
    fn a_functions_return_type_is_not_an_annotation() {
        let facts = parse(
            "src/x.ts",
            "export function AGENT_TOOL(): AgentToolModule {}",
        );

        let export = facts.named_export("AGENT_TOOL").expect("is exported");
        assert!(export.annotations.is_empty());
    }

    /// Whitespace is the formatter's business. The fact keeps a readable form
    /// -- it is printed back at a user in a finding -- and the comparison that
    /// uses it ignores spacing on both sides.
    #[test]
    fn an_annotation_broken_over_lines_is_collapsed_to_one() {
        let facts = parse(
            "src/x.ts",
            "export const T: AgentToolModule<\n  Input,\n  Output\n> = x;",
        );

        let export = facts.named_export("T").expect("is exported");
        assert_eq!(export.annotations, ["AgentToolModule< Input, Output >"]);
    }

    #[test]
    fn imports_are_grouped_by_statement() {
        let facts = parse(
            "src/x.ts",
            r"
import { A, B } from './ab';
import C from './c';
import './side-effect';
",
        );

        let specifiers: Vec<_> = facts.imports.iter().map(|i| i.specifier.as_str()).collect();
        assert_eq!(specifiers, ["./ab", "./c", "./side-effect"]);

        let ab = facts.imports.first().expect("first import");
        assert_eq!(ab.names, ["A", "B"], "one statement, one fact, two names");

        let side_effect = facts.imports.get(2).expect("third import");
        assert!(
            side_effect.names.is_empty(),
            "a side-effect import binds nothing"
        );
    }

    /// Boundary rules may opt out of type-only imports, so the distinction has
    /// to survive extraction.
    #[test]
    fn type_only_imports_are_marked() {
        let facts = parse(
            "src/x.ts",
            r"
import type { T } from './types';
import { V } from './values';
import { type U, W } from './mixed';
",
        );

        let by_specifier = |specifier: &str| {
            facts
                .imports
                .iter()
                .find(|i| i.specifier == specifier)
                .unwrap_or_else(|| panic!("no import of {specifier}"))
        };

        assert!(by_specifier("./types").type_only);
        assert!(!by_specifier("./values").type_only);
        assert!(
            !by_specifier("./mixed").type_only,
            "a statement is type-only only when every name in it is"
        );
    }

    /// A `import()` expression is a dependency. Found by the differential
    /// harness against a real monorepo, where a boundary rule could be
    /// bypassed -- without anyone trying to -- by lazy-loading a forbidden
    /// layer.
    #[test]
    fn a_dynamic_import_is_an_import() {
        let facts = parse(
            "src/x.ts",
            r"
export async function lazy() {
  const { mapReaction } = await import('./mappers/map-reaction');
  return mapReaction;
}
",
        );

        let specifiers: Vec<_> = facts.imports.iter().map(|i| i.specifier.as_str()).collect();
        assert_eq!(specifiers, ["./mappers/map-reaction"]);
    }

    /// TypeScript's `import("...")` in type position is the same dependency
    /// written differently, and dependency-cruiser counts it. So do we.
    #[test]
    fn a_dynamic_import_in_type_position_is_an_import() {
        let facts = parse(
            "src/x.ts",
            r#"
export interface Ctx {
  actor: import("../../actor/actor").Actor;
}
"#,
        );

        let import = facts.imports.first().expect("one import");
        assert_eq!(import.specifier, "../../actor/actor");
        assert!(
            import.type_only,
            "a type-position import is erased, so a rule that opted out of \
             type-only imports must not see it"
        );
    }

    /// A specifier that is not a literal cannot be resolved, and inventing one
    /// would be worse than omitting it: a boundary rule would report a path
    /// nobody wrote. It is left out, and the `unresolved` tally is where a
    /// user learns the run could not see everything.
    #[test]
    fn a_computed_dynamic_import_is_left_out() {
        let facts = parse(
            "src/x.ts",
            r"
export async function lazy(name: string) {
  const a = await import(name);
  const b = await import(`./locales/${name}.ts`);
  const c = await import('./known');
  return [a, b, c];
}
",
        );

        let specifiers: Vec<_> = facts.imports.iter().map(|i| i.specifier.as_str()).collect();
        assert_eq!(specifiers, ["./known"], "only the literal one");
    }

    /// A dynamic import brings no names into scope at the statement level --
    /// the destructuring is a separate binding -- and it is not type-only.
    #[test]
    fn a_dynamic_import_binds_no_names_and_is_not_type_only() {
        let facts = parse("src/x.ts", "export const p = import('./thing');");

        let import = facts.imports.first().expect("one import");
        assert!(import.names.is_empty());
        assert!(!import.type_only);
        assert!(import.resolved.is_none(), "resolution is a later pass");
    }

    /// Static and dynamic imports of the same module are one dependency each,
    /// and both survive. Ordering stays by position so a finding's span points
    /// at the statement a reader can find.
    #[test]
    fn static_and_dynamic_imports_coexist_in_source_order() {
        let facts = parse(
            "src/x.ts",
            r"
import { A } from './a';
export async function lazy() {
  const { B } = await import('./b');
  const { C } = await import('./a');
  return [A, B, C];
}
",
        );

        let specifiers: Vec<_> = facts.imports.iter().map(|i| i.specifier.as_str()).collect();
        assert_eq!(specifiers, ["./a", "./b", "./a"]);
    }

    /// `require()` is not `import()`. `CommonJS` resolution has its own rules
    /// and v0 does not claim to follow them; picking up the string here would
    /// promise a coverage the resolver does not have.
    #[test]
    fn a_require_call_is_not_picked_up() {
        let facts = parse("src/x.ts", "export const thing = require('./commonjs');");

        assert!(facts.imports.is_empty());
    }

    /// A re-export's source is an import as far as the graph is concerned.
    #[test]
    fn a_reexports_source_counts_as_an_import() {
        let facts = parse("src/x.ts", "export { Foo } from './other';");
        assert_eq!(
            facts
                .imports
                .iter()
                .map(|i| i.specifier.as_str())
                .collect::<Vec<_>>(),
            ["./other"]
        );
    }

    /// `call-obligation` matches a dotted symbol path, and docs/RULES.md names
    /// `logger.audit` as a valid one, so chains have to resolve fully.
    #[test]
    fn callees_are_recorded_as_dotted_paths() {
        let facts = parse(
            "src/x.ts",
            r"
Event.save({});
logger.audit.write('x');
plain();
",
        );

        let callees: Vec<_> = facts.calls.iter().map(|c| c.callee.as_str()).collect();
        assert_eq!(callees, ["Event.save", "logger.audit.write", "plain"]);
    }

    /// A callee whose root is not a name cannot be written as a symbol path,
    /// so it is left out rather than recorded half-formed as `?.toBe`.
    #[test]
    fn a_callee_that_is_not_a_symbol_path_is_not_recorded() {
        let facts = parse("src/x.ts", "expect(value).toBe(1);");

        let callees: Vec<_> = facts.calls.iter().map(|c| c.callee.as_str()).collect();
        assert_eq!(callees, ["expect"], "the outer chain is not nameable");
    }

    /// Calls are found wherever they are, not only at the top level. This is
    /// what `require_non_empty_spec` needs: an `it(...)` inside a `describe`.
    #[test]
    fn calls_are_found_at_any_depth() {
        let facts = parse(
            "src/x.spec.ts",
            r"
describe('a thing', () => {
  it('works', () => {});
  test('also', async () => { await Event.save({}); });
});
",
        );

        let callees: Vec<_> = facts.calls.iter().map(|c| c.callee.as_str()).collect();
        assert_eq!(callees, ["describe", "it", "test", "Event.save"]);
    }

    /// `.tsx` is a different grammar, not a different extension. Reading one
    /// as `.ts` fails, which is why the source type comes from the path.
    #[test]
    fn jsx_parses_in_a_tsx_file_and_not_in_a_ts_file() {
        let facts = parse("src/c.tsx", "export const C = () => <div />;");
        assert_eq!(kinds(&facts, "C"), ["arrow", "const"]);

        let as_ts = OxcParser.parse(
            &RepoRelPath::new("src/c.ts").expect("valid"),
            "export const C = () => <div />;",
            ContentHash::of(b""),
        );
        assert!(matches!(as_ts, Err(ParseError::Unparsable { .. })));
    }

    /// TypeScript archwarden will actually meet: decorators, generics,
    /// ambient declarations. A front-end that refused these would be refusing
    /// files `tsc` accepts.
    #[test]
    fn realistic_typescript_parses_without_complaint() {
        let facts = parse(
            "src/service.ts",
            r#"
@Injectable()
export class Service<T extends object> {
  constructor(private readonly value: T) {}
  async run(): Promise<void> { await Promise.resolve(); }
}
export const enum Flag { On = 1 }
declare module "external" { export const x: number; }
"#,
        );

        assert_eq!(kinds(&facts, "Service"), ["class"]);
    }

    #[test]
    fn a_file_that_does_not_parse_is_reported_with_its_path() {
        let error = OxcParser
            .parse(
                &RepoRelPath::new("src/broken.ts").expect("valid"),
                "export const = ;",
                ContentHash::of(b""),
            )
            .expect_err("should not parse");

        let message = error.to_string();
        assert!(message.contains("src/broken.ts"), "{message}");
        assert!(message.contains("does not parse"), "{message}");
    }

    #[test]
    fn a_file_with_no_javascript_extension_is_refused() {
        let error = OxcParser
            .parse(
                &RepoRelPath::new("README.md").expect("valid"),
                "# hello",
                ContentHash::of(b""),
            )
            .expect_err("should be refused");

        assert!(matches!(error, ParseError::UnsupportedExtension { .. }));
    }

    /// The facts carry the hash they were extracted at, which is what the
    /// cache will key them by.
    #[test]
    fn the_content_hash_is_carried_into_the_facts() {
        let source = "export const A = 1;";
        let facts = parse("src/x.ts", source);
        assert_eq!(facts.content_hash, ContentHash::of(source.as_bytes()));
        assert_eq!(facts.path.as_str(), "src/x.ts");
    }

    #[test]
    fn an_empty_file_yields_empty_facts() {
        let facts = parse("src/empty.ts", "");
        assert!(facts.imports.is_empty());
        assert!(facts.exports.is_empty());
        assert!(facts.calls.is_empty());
    }
}
