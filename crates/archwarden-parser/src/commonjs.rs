//! `CommonJS`, read off the AST.
//!
//! `oxc`'s module record covers module *syntax* and `CommonJS` has none: a
//! `require` is a call and an export is an assignment. So both halves come
//! from the tree, and both land in the facts an ES module already fills --
//! [`ImportFact`] and [`ExportFact`] -- which is what keeps every rule engine
//! unaware that a second module system exists. Decision 41.
//!
//! What is read is what is *written out*. `require(name)` names no module and
//! is recorded as an opaque import, on the terms a computed `import()` already
//! is; `exports[key] = …` and `Object.defineProperty(exports, …)` name no
//! export a reader can see, and are left absent rather than guessed at.

use std::collections::{HashMap, HashSet};

use archwarden_core::facts::{ExportFact, ExportKind, ExportTags, ImportFact, Visibility};
use oxc_ast::ast::{
    Argument, AssignmentExpression, AssignmentTarget, CallExpression, Expression,
    ObjectPropertyKind, Program, PropertyKind, Statement, StaticMemberExpression,
    TSImportEqualsDeclaration, TSModuleReference, VariableDeclarator,
};
use oxc_ast_visit::Visit;
use oxc_syntax::operator::AssignmentOperator;

use crate::oxc::{collapsed, return_type_of, span_of};

/// What the ES-module pass already knows about the file's top-level bindings.
///
/// `module.exports = { Local }` refers to a declaration made elsewhere in the
/// file, exactly as `export { Local }` does, so the answer to "how was `Local`
/// declared" is the same map and is borrowed rather than rebuilt.
#[derive(Clone, Copy)]
pub(crate) struct Declarations<'m> {
    /// How each binding was declared.
    pub tags: &'m HashMap<String, ExportTags>,
    /// The types each binding writes down about itself.
    pub annotations: &'m HashMap<String, Vec<String>>,
    /// The return type each callable binding declares.
    pub returns: &'m HashMap<String, String>,
    /// The bindings that are nothing but a forward of another.
    pub forwarded: &'m HashMap<String, String>,
}

/// Whether a call is to the free function `require`.
///
/// By name, with no scope analysis: a file that binds its own `require` --
/// `createRequire(import.meta.url)` is the usual way -- is loading modules
/// with it just the same. `schema.require(...)` is a method and is not this.
fn is_require(call: &CallExpression<'_>) -> bool {
    matches!(&call.callee, Expression::Identifier(callee) if callee.name == "require")
}

/// The module a `require` names, when it wrote one out.
fn specifier<'b>(call: &'b CallExpression<'_>) -> Option<&'b str> {
    match call.arguments.first() {
        Some(Argument::StringLiteral(literal)) => Some(literal.value.as_str()),
        _ => None,
    }
}

/// The `require` an expression is, and nothing else.
fn direct_require<'a, 'b>(expression: &'b Expression<'a>) -> Option<&'b CallExpression<'a>> {
    match expression {
        Expression::CallExpression(call) if is_require(call) => Some(call),
        _ => None,
    }
}

/// The `require` an expression reads out of: `require('./x')` itself, or
/// `require('./x').a.b`.
fn require_behind<'a, 'b>(expression: &'b Expression<'a>) -> Option<&'b CallExpression<'a>> {
    let mut current = expression;
    while let Expression::StaticMemberExpression(member) = current {
        current = &member.object;
    }
    direct_require(current)
}

/// Every `require` in the file whose specifier is written out, and whether
/// any was not.
pub(crate) fn requires(program: &Program<'_>) -> (Vec<ImportFact>, bool) {
    let mut collector = RequireCollector::default();
    collector.visit_program(program);
    (collector.imports, collector.opaque)
}

#[derive(Default)]
struct RequireCollector {
    imports: Vec<ImportFact>,
    /// Whether a `require` named something this cannot read.
    opaque: bool,
    /// The calls a declarator already recorded, with the names it binds.
    ///
    /// The walk descends from a declarator into its initialiser, so the call
    /// arrives at `visit_call_expression` as well. Marked on the way in,
    /// checked on the way down -- the same arrangement `CallCollector` has for
    /// a callee.
    bound: HashSet<(u32, u32)>,
}

impl RequireCollector {
    fn record(&mut self, call: &CallExpression<'_>, names: Vec<String>) {
        // Only a literal specifier, on the terms `import()` is held to:
        // `require(name)` names no single module, and inventing one would have
        // a boundary rule report a path nobody wrote.
        let Some(specifier) = specifier(call) else {
            self.opaque = true;
            return;
        };
        self.imports.push(ImportFact {
            specifier: specifier.to_owned(),
            resolved: None,
            // A `require` is a call. It runs, whatever is done with the result.
            type_only: false,
            names,
            span: span_of(call.span),
        });
    }
}

impl<'a> Visit<'a> for RequireCollector {
    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        // `const x = require('./x')`, `const { a, b: c } = require('./x')` and
        // `const a = require('./x').a` are the three ways `CommonJS` spells an
        // import that binds a name, and the name is the local one -- which is
        // what `import { b as c }` records too.
        if let Some(call) = declarator.init.as_ref().and_then(require_behind) {
            let names = declarator
                .id
                .get_binding_identifiers()
                .iter()
                .map(|identifier| identifier.name.to_string())
                .collect();
            self.bound.insert((call.span.start, call.span.end));
            self.record(call, names);
        }
        oxc_ast_visit::walk::walk_variable_declarator(self, declarator);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if is_require(call) && !self.bound.contains(&(call.span.start, call.span.end)) {
            self.record(call, Vec::new());
        }
        oxc_ast_visit::walk::walk_call_expression(self, call);
    }

    fn visit_ts_import_equals_declaration(&mut self, import: &TSImportEqualsDeclaration<'a>) {
        // `import fs = require('fs')`. `import A = N.Inner` is an alias for
        // something already in scope and loads nothing.
        if let TSModuleReference::ExternalModuleReference(external) = &import.module_reference {
            self.imports.push(ImportFact {
                specifier: external.expression.value.to_string(),
                resolved: None,
                type_only: import.import_kind.is_type(),
                names: vec![import.id.name.to_string()],
                span: span_of(import.span),
            });
        }
        oxc_ast_visit::walk::walk_ts_import_equals_declaration(self, import);
    }
}

/// Everything the file exports the `CommonJS` way, in source order.
///
/// Top-level statements only, as `export` is. An assignment inside a function
/// or behind a condition may never run, and a `must_export` satisfied by one
/// that does not is a rule enforcing nothing.
pub(crate) fn exports(
    program: &Program<'_>,
    source: &str,
    declared: Declarations<'_>,
) -> Vec<ExportFact> {
    let reader = ExportReader {
        source,
        declared,
        required: required_bindings(program),
    };
    let mut facts = Vec::new();

    for statement in &program.body {
        match statement {
            Statement::ExpressionStatement(expression) => {
                if let Expression::AssignmentExpression(assignment) = &expression.expression {
                    reader.assignment(assignment, &mut facts);
                }
            }
            // TypeScript's `export = X` is `module.exports = X` with a keyword.
            Statement::TSExportAssignment(assignment) => {
                reader.whole(&assignment.expression, assignment.span, &mut facts);
            }
            _ => {}
        }
    }

    facts
}

/// The top-level names a `require` binds.
///
/// A `const` holding a `require` is a top-level declaration as far as
/// [`Declarations::tags`] can tell, and it is an import as far as the file is
/// concerned. Exporting one is `import { A }` followed by `export { A }`, so
/// the export side has to know which names these are.
fn required_bindings(program: &Program<'_>) -> HashSet<String> {
    let mut names = HashSet::new();

    for statement in &program.body {
        // `import x = require()` needs no entry: the declaration map has
        // never heard of it, so it already reads as a name the file did not
        // declare.
        let Statement::VariableDeclaration(variable) = statement else {
            continue;
        };
        for declarator in &variable.declarations {
            if declarator.init.as_ref().and_then(require_behind).is_some() {
                names.extend(
                    declarator
                        .id
                        .get_binding_identifiers()
                        .iter()
                        .map(|identifier| identifier.name.to_string()),
                );
            }
        }
    }

    names
}

/// What an assignment's left-hand side says about the module's exports.
enum Target<'b> {
    /// `module.exports`: everything the module exports, replaced at once.
    Whole,
    /// `exports.name` or `module.exports.name`.
    Named(&'b str),
}

/// Whether a member expression is `module.exports`.
fn is_module_exports(member: &StaticMemberExpression<'_>) -> bool {
    member.property.name == "exports"
        && matches!(&member.object, Expression::Identifier(object) if object.name == "module")
}

fn target_of<'b>(target: &'b AssignmentTarget<'_>) -> Option<Target<'b>> {
    // Static members only. `exports[key] = …` names no export a reader can
    // see, and a bare `exports = …` rebinds a local and exports nothing.
    let AssignmentTarget::StaticMemberExpression(member) = target else {
        return None;
    };
    if is_module_exports(member) {
        return Some(Target::Whole);
    }

    let on_the_export_object = match &member.object {
        Expression::Identifier(object) => object.name == "exports",
        Expression::StaticMemberExpression(object) => is_module_exports(object),
        _ => false,
    };
    on_the_export_object.then(|| Target::Named(member.property.name.as_str()))
}

/// The part of an [`ExportFact`] that comes from the value being exported.
struct Shape {
    tags: ExportTags,
    annotations: Vec<String>,
    returns: Option<String>,
    reexport_from: Option<String>,
    forwards: Option<String>,
}

impl Shape {
    fn of(tags: ExportTags) -> Self {
        Self {
            tags,
            annotations: Vec::new(),
            returns: None,
            reexport_from: None,
            forwards: None,
        }
    }
}

struct ExportReader<'s, 'm> {
    source: &'s str,
    declared: Declarations<'m>,
    required: HashSet<String>,
}

impl ExportReader<'_, '_> {
    /// One assignment statement, which may be a chain: `exports.a = exports.b
    /// = value` exports both, and `exports = module.exports = value` is the
    /// idiom that keeps the alias pointing at the new object.
    fn assignment(&self, assignment: &AssignmentExpression<'_>, facts: &mut Vec<ExportFact>) {
        let mut targets = Vec::new();
        let mut current = assignment;
        let value = loop {
            // `exports.count += 1` changes something already exported and
            // declares nothing.
            if current.operator != AssignmentOperator::Assign {
                return;
            }
            targets.push(&current.left);
            match &current.right {
                Expression::AssignmentExpression(next) => current = next,
                value => break value,
            }
        };

        for target in targets {
            match target_of(target) {
                Some(Target::Whole) => self.whole(value, assignment.span, facts),
                Some(Target::Named(name)) => {
                    facts.push(self.fact(Some(name), value, assignment.span));
                }
                None => {}
            }
        }
    }

    /// `module.exports = value`.
    fn whole(&self, value: &Expression<'_>, span: oxc_span::Span, facts: &mut Vec<ExportFact>) {
        // `module.exports = require('./x')` is `export * from './x'`.
        if let Some(from) = direct_require(value).and_then(specifier) {
            facts.push(star(from, span));
            return;
        }

        // Anything that is not an object literal is what an importer receives
        // under a name of its own choosing, which is what a default is.
        let Expression::ObjectExpression(object) = value else {
            facts.push(self.fact(None, value, span));
            return;
        };

        // The object literal is the dominant idiom and the one `const { a } =
        // require()` consumes by name, so each property is a named export.
        for property in &object.properties {
            match property {
                ObjectPropertyKind::ObjectProperty(property) => {
                    // A computed key names nothing a reader can see.
                    let Some(name) = property.key.static_name() else {
                        continue;
                    };
                    let mut fact = self.fact(Some(&name), &property.value, property.span);
                    // A getter is written as a function and is not one: the
                    // importer is handed whatever it returns.
                    if property.kind != PropertyKind::Init {
                        fact.tags = ExportTags::only(ExportKind::Const);
                        fact.returns = None;
                    }
                    facts.push(fact);
                }
                // `{ ...require('./a') }` is the barrel. A spread of anything
                // else is a set nobody here can enumerate *and* nobody here
                // can name the source of, so it is absent.
                ObjectPropertyKind::SpreadProperty(spread) => {
                    if let Some(from) = direct_require(&spread.argument).and_then(specifier) {
                        facts.push(star(from, spread.span));
                    }
                }
            }
        }
    }

    /// One export: `name` for `exports.name`, `None` for a default.
    fn fact(&self, name: Option<&str>, value: &Expression<'_>, span: oxc_span::Span) -> ExportFact {
        let shape = self.shape(name, value);

        ExportFact {
            name: name.map(str::to_owned),
            tags: shape.tags,
            attributes: Vec::new(),
            visibility: Visibility::Public,
            is_default: name.is_none(),
            reexport_from: shape.reexport_from,
            forwards: shape.forwards,
            annotations: shape.annotations,
            returns: shape.returns,
            span: span_of(span),
        }
    }

    /// How the exported value was declared, in the vocabulary an ES module's
    /// export already uses.
    ///
    /// `exports.x = …` is how `CommonJS` spells `export const x = …`, so a
    /// named export of a plain value is a `const` -- a rule written against
    /// one module system has to get the same answer from the other. A default
    /// has no binding of its own, as `export default 42` has none.
    fn shape(&self, name: Option<&str>, value: &Expression<'_>) -> Shape {
        let binding = if name.is_some() {
            ExportTags::only(ExportKind::Const)
        } else {
            ExportTags::none()
        };

        // `exports.user = require('./user')` and `exports.save =
        // require('./event').save`: the file holds the name and nothing else
        // about it, which is `export { save } from './event'`.
        if let Some(from) = require_behind(value).and_then(specifier) {
            return Shape {
                reexport_from: Some(from.to_owned()),
                forwards: name.map(str::to_owned),
                ..Shape::of(ExportTags::only(ExportKind::Reexport))
            };
        }

        match value {
            Expression::Identifier(local) => self.local(local.name.as_str()),
            Expression::FunctionExpression(_) => Shape {
                returns: self.returns_of(value),
                ..Shape::of(ExportTags::only(ExportKind::Function))
            },
            Expression::ArrowFunctionExpression(_) => Shape {
                returns: self.returns_of(value),
                ..Shape::of(binding.with(ExportKind::Arrow))
            },
            Expression::ClassExpression(_) => Shape::of(ExportTags::only(ExportKind::Class)),
            _ => Shape::of(binding),
        }
    }

    /// An export that names a binding: `exports.save = save`.
    fn local(&self, local: &str) -> Shape {
        // A binding holding a `require` is declared here only in the sense
        // that the word `const` is in front of it. It is an import, and an ES
        // module's import has no entry in the declaration map at all.
        let tags = self
            .declared
            .tags
            .get(local)
            .filter(|_| !self.required.contains(local));

        Shape {
            tags: tags.copied().unwrap_or_else(ExportTags::none),
            annotations: self
                .declared
                .annotations
                .get(local)
                .cloned()
                .unwrap_or_default(),
            returns: self.declared.returns.get(local).cloned(),
            reexport_from: None,
            // The rule `export { X }` follows: an alias or a wrapper this file
            // declared forwards what it names, and a name the file never
            // declared came in from somewhere else.
            forwards: self
                .declared
                .forwarded
                .get(local)
                .cloned()
                .or_else(|| tags.is_none().then(|| local.to_owned())),
        }
    }

    /// The return type a function or arrow expression writes down.
    fn returns_of(&self, value: &Expression<'_>) -> Option<String> {
        use oxc_span::GetSpan as _;

        let annotation = return_type_of(value)?;
        collapsed(self.source, annotation.type_annotation.span())
    }
}

/// `export * from './x'`, written as an assignment or a spread.
///
/// Named `*` for the reason the ES-module form is: it exports a set nobody
/// here can enumerate, and a fact with no name is dropped by every rule that
/// reports one.
fn star(from: &str, span: oxc_span::Span) -> ExportFact {
    ExportFact {
        name: Some("*".to_owned()),
        tags: ExportTags::only(ExportKind::Reexport),
        attributes: Vec::new(),
        visibility: Visibility::Public,
        is_default: false,
        reexport_from: Some(from.to_owned()),
        forwards: Some("*".to_owned()),
        annotations: Vec::new(),
        returns: None,
        span: span_of(span),
    }
}

#[cfg(test)]
mod tests {
    use archwarden_core::{
        facts::{ExportFact, ExportKind, FileFacts},
        hash::ContentHash,
        path::RepoRelPath,
        traits::Parser as _,
    };

    use crate::oxc::OxcParser;

    fn parse(name: &str, source: &str) -> FileFacts {
        OxcParser
            .parse(
                &RepoRelPath::new(name).expect("valid path"),
                source,
                ContentHash::of(source.as_bytes()),
            )
            .expect("should parse")
    }

    fn specifiers(facts: &FileFacts) -> Vec<&str> {
        facts.imports.iter().map(|i| i.specifier.as_str()).collect()
    }

    fn named<'a>(facts: &'a FileFacts, name: &str) -> &'a ExportFact {
        facts
            .exports
            .iter()
            .find(|export| export.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no export named `{name}` in {:?}", facts.exports))
    }

    fn kinds(export: &ExportFact) -> Vec<ExportKind> {
        export.tags.iter().collect()
    }

    // ---- require ----------------------------------------------------------

    /// The whole point: a boundary rule over a `CommonJS` layer saw no edges
    /// at all, and a rule that sees nothing reports a clean repository.
    #[test]
    fn a_require_call_is_an_import() {
        let facts = parse("src/x.js", "const user = require('./domain/user');");

        assert_eq!(specifiers(&facts), ["./domain/user"]);
        let import = facts.imports.first().expect("one import");
        assert!(!import.type_only, "a require always runs");
        assert!(import.resolved.is_none(), "resolution is a later pass");
    }

    /// `call-obligation` and `chokepoint` ask whether a file imports a symbol
    /// *by name* from a module, so the binding has to be the name.
    #[test]
    fn a_require_bound_to_a_name_brings_that_name_into_scope() {
        let facts = parse("src/x.js", "const logger = require('./logger');");

        assert_eq!(facts.imports.first().expect("one").names, ["logger"]);
    }

    /// Destructuring is how `CommonJS` spells a named import, and the local
    /// name is the one that binds -- `import { save as persist }` on the same
    /// terms.
    #[test]
    fn a_destructured_require_brings_each_local_name_into_scope() {
        let facts = parse(
            "src/x.js",
            "const { find, save: persist, ...rest } = require('./event');",
        );

        assert_eq!(
            facts.imports.first().expect("one").names,
            ["find", "persist", "rest"]
        );
    }

    /// `const save = require('./event').save` is the same import written
    /// without braces.
    #[test]
    fn a_member_of_a_require_binds_the_name_it_is_assigned_to() {
        let facts = parse("src/x.js", "const save = require('./event').save;");

        let import = facts.imports.first().expect("one import");
        assert_eq!(import.specifier, "./event");
        assert_eq!(import.names, ["save"]);
    }

    /// A require that is not the whole of a declaration still loads the
    /// module; it just binds nothing.
    #[test]
    fn a_require_outside_a_declaration_is_an_import_that_binds_nothing() {
        let facts = parse(
            "src/x.js",
            "require('./polyfill');\nrun(require('./config'));",
        );

        assert_eq!(specifiers(&facts), ["./polyfill", "./config"]);
        assert!(facts.imports.iter().all(|import| import.names.is_empty()));
    }

    /// One declaration is one import: the declarator records it with its
    /// names, and the walk that then reaches the call must not record it
    /// again.
    #[test]
    fn a_bound_require_is_recorded_once() {
        let facts = parse("src/x.js", "const a = require('./a');");

        assert_eq!(facts.imports.len(), 1);
    }

    /// A lazy require inside a function is how a forbidden layer gets loaded
    /// without anyone trying to hide it -- the same finding that made
    /// `import()` an import.
    #[test]
    fn a_require_inside_a_function_is_an_import() {
        let facts = parse(
            "src/x.js",
            "function lazy() {\n  const db = require('./infra/db');\n  return db;\n}",
        );

        assert_eq!(specifiers(&facts), ["./infra/db"]);
        assert_eq!(facts.imports.first().expect("one").names, ["db"]);
    }

    /// On the terms a computed `import()` is held to: a module nobody wrote
    /// out is not invented, and the file says it has one.
    #[test]
    fn a_computed_require_is_left_out_and_marks_the_file_opaque() {
        let facts = parse(
            "src/x.js",
            "const plugin = require(name);\nconst known = require('./known');",
        );

        assert_eq!(specifiers(&facts), ["./known"], "only the literal one");
        assert!(facts.has_opaque_import);
    }

    #[test]
    fn a_file_whose_requires_are_all_literal_is_not_opaque() {
        let facts = parse("src/x.js", "const known = require('./known');");

        assert!(!facts.has_opaque_import);
    }

    /// A method that happens to be called `require` is somebody's API, not
    /// the module loader.
    #[test]
    fn a_method_called_require_is_not_a_require() {
        let facts = parse(
            "src/x.js",
            "schema.require('./name');\nschema.require(field);",
        );

        assert!(facts.imports.is_empty());
        assert!(!facts.has_opaque_import);
    }

    /// Every other call in the file is somebody else's function, whatever it
    /// was given.
    #[test]
    fn a_call_to_another_function_is_not_a_require() {
        let facts = parse("src/x.js", "load('./name');\nload(name);");

        assert!(facts.imports.is_empty());
        assert!(!facts.has_opaque_import);
    }

    /// The span is the call, which is the thing a reader can find; the order
    /// is by position, the way every other import is ordered.
    #[test]
    fn requires_and_imports_coexist_in_source_order() {
        let source =
            "import { A } from './a';\nconst b = require('./b');\nconst c = import('./c');";
        let facts = parse("src/x.ts", source);

        assert_eq!(specifiers(&facts), ["./a", "./b", "./c"]);
        let require = facts.imports.get(1).expect("the require");
        assert_eq!(
            source.get(require.span.start as usize..require.span.end as usize),
            Some("require('./b')")
        );
    }

    /// TypeScript's own spelling of a `CommonJS` import. The module record
    /// does not carry it, so without this a `.cts` file imported nothing.
    #[test]
    fn a_typescript_import_equals_is_an_import_binding_its_name() {
        let facts = parse("src/x.cts", "import fs = require('./fs-layer');");

        let import = facts.imports.first().expect("one import");
        assert_eq!(import.specifier, "./fs-layer");
        assert_eq!(import.names, ["fs"]);
        assert!(!import.type_only);
    }

    #[test]
    fn a_type_only_import_equals_is_type_only() {
        let facts = parse("src/x.cts", "import type T = require('./types');");

        assert!(facts.imports.first().expect("one import").type_only);
    }

    /// `import A = Namespace.Inner` is an alias for something already in
    /// scope and loads no module.
    #[test]
    fn an_import_equals_naming_a_namespace_is_not_an_import() {
        let facts = parse(
            "src/x.ts",
            "namespace N { export const a = 1; }\nimport a = N.a;\nexport const b = a;",
        );

        assert!(facts.imports.is_empty());
    }

    // ---- exports.name = … -------------------------------------------------

    /// `exports.x = …` is how `CommonJS` spells `export const x = …`, and a
    /// rule written for one has to get the same answer from the other.
    #[test]
    fn an_exports_property_is_a_named_export() {
        let facts = parse("src/x.js", "exports.LIMIT = 10;");

        let export = named(&facts, "LIMIT");
        assert!(!export.is_default);
        assert_eq!(kinds(export), [ExportKind::Const]);
        assert!(export.reexport_from.is_none());
        assert!(export.forwards.is_none());
    }

    #[test]
    fn a_module_exports_property_is_a_named_export() {
        let facts = parse("src/x.js", "module.exports.LIMIT = 10;");

        assert_eq!(kinds(named(&facts, "LIMIT")), [ExportKind::Const]);
    }

    /// Decision 9's distinction survives the module system: a function
    /// expression is a `function`, an arrow is not.
    #[test]
    fn an_exported_function_expression_is_a_function_and_an_arrow_is_an_arrow() {
        let facts = parse(
            "src/x.js",
            "exports.plain = function () {};\nexports.arrow = () => {};\nexports.Model = class {};",
        );

        assert_eq!(kinds(named(&facts, "plain")), [ExportKind::Function]);
        assert_eq!(
            kinds(named(&facts, "arrow")),
            [ExportKind::Arrow, ExportKind::Const]
        );
        assert_eq!(kinds(named(&facts, "Model")), [ExportKind::Class]);
    }

    /// `exports.save = save` exports a declaration made elsewhere in the
    /// file, which is `export { save }` -- so the form is the declaration's.
    #[test]
    fn an_exported_local_binding_carries_how_it_was_declared() {
        let facts = parse(
            "src/x.ts",
            "function save(): Promise<void> {}\nconst repo: Repository = {};\nexports.save = save;\nexports.repo = repo;",
        );

        let save = named(&facts, "save");
        assert_eq!(kinds(save), [ExportKind::Function]);
        assert_eq!(save.returns.as_deref(), Some("Promise<void>"));
        assert!(save.forwards.is_none(), "declared here, so not a forward");

        let repo = named(&facts, "repo");
        assert_eq!(kinds(repo), [ExportKind::Const]);
        assert_eq!(repo.annotations, ["Repository"]);
    }

    /// The return type written on the expression is the declaration's own.
    #[test]
    fn an_exported_function_expression_carries_its_return_type() {
        let facts = parse(
            "src/x.ts",
            "exports.load = (): Promise<User> => fetchUser();\nexports.bare = () => 1;\nexports.plain = function (): Result {};",
        );

        assert_eq!(named(&facts, "plain").returns.as_deref(), Some("Result"));
        assert_eq!(
            named(&facts, "load").returns.as_deref(),
            Some("Promise<User>")
        );
        assert!(named(&facts, "bare").returns.is_none());
    }

    /// The span is the assignment a reader can find.
    #[test]
    fn a_named_export_points_at_its_assignment() {
        let source = "const a = 1;\nexports.a = a;";
        let facts = parse("src/x.js", source);

        let span = named(&facts, "a").span;
        assert_eq!(
            source.get(span.start as usize..span.end as usize),
            Some("exports.a = a")
        );
    }

    /// `exports.count += 1` mutates something already exported; it declares
    /// nothing.
    #[test]
    fn a_compound_assignment_is_not_an_export() {
        let facts = parse("src/x.js", "exports.count += 1;\nmodule.exports ||= {};");

        assert!(facts.exports.is_empty());
    }

    /// `other.exports.x` and `exports.a.b` are property writes on something
    /// that is not the module's export object.
    #[test]
    fn a_write_to_something_that_is_not_the_export_object_is_not_an_export() {
        let facts = parse(
            "src/x.js",
            "other.exports.x = 1;\nexports.a.b = 2;\nconfig.exports = {};\nexports = {};\nmodule.id = 3;",
        );

        assert!(facts.exports.is_empty(), "{:?}", facts.exports);
    }

    /// Only the top level, as with `export`: an assignment inside a function
    /// may never run, and a rule must not be satisfied by one that does not.
    #[test]
    fn an_assignment_inside_a_function_is_not_an_export() {
        let facts = parse("src/x.js", "function later() {\n  exports.late = 1;\n}");

        assert!(facts.exports.is_empty());
    }

    /// `exports.a = exports.b = value` exports both, and every target gets
    /// the value at the end of the chain.
    #[test]
    fn a_chained_assignment_exports_every_target() {
        let facts = parse("src/x.js", "exports.a = exports.b = function () {};");

        assert_eq!(kinds(named(&facts, "a")), [ExportKind::Function]);
        assert_eq!(kinds(named(&facts, "b")), [ExportKind::Function]);
    }

    // ---- module.exports = … -----------------------------------------------

    /// The dominant idiom, and the one `const { a } = require()` consumes by
    /// name -- so each property is a named export.
    #[test]
    fn an_exported_object_literal_exports_each_property_by_name() {
        let facts = parse(
            "src/x.js",
            "function find() {}\nconst save = () => {};\nmodule.exports = { find, save, LIMIT: 10, 'quoted': 1, load() {} };",
        );

        assert_eq!(kinds(named(&facts, "find")), [ExportKind::Function]);
        assert_eq!(
            kinds(named(&facts, "save")),
            [ExportKind::Arrow, ExportKind::Const]
        );
        assert_eq!(kinds(named(&facts, "LIMIT")), [ExportKind::Const]);
        assert_eq!(kinds(named(&facts, "quoted")), [ExportKind::Const]);
        assert_eq!(kinds(named(&facts, "load")), [ExportKind::Function]);
        assert!(facts.exports.iter().all(|export| !export.is_default));
        assert_eq!(facts.exports.len(), 5);
    }

    /// A computed key names nothing a reader can see.
    #[test]
    fn a_computed_key_in_an_exported_object_is_left_out() {
        let facts = parse("src/x.js", "module.exports = { [name]: 1, kept: 2 };");

        assert_eq!(facts.exports.len(), 1);
        assert_eq!(named(&facts, "kept").name.as_deref(), Some("kept"));
    }

    /// A getter is a value computed on read, not a function a caller is
    /// handed.
    #[test]
    fn a_getter_in_an_exported_object_is_not_a_function() {
        let facts = parse("src/x.js", "module.exports = { get lazy() { return 1; } };");

        assert_eq!(kinds(named(&facts, "lazy")), [ExportKind::Const]);
    }

    /// A property points at itself, not at the whole object, so a finding
    /// lands on the line that has the name in it.
    #[test]
    fn a_property_export_points_at_the_property() {
        let source = "module.exports = {\n  LIMIT: 10,\n};";
        let facts = parse("src/x.js", source);

        let span = named(&facts, "LIMIT").span;
        assert_eq!(
            source.get(span.start as usize..span.end as usize),
            Some("LIMIT: 10")
        );
    }

    /// Anything else assigned whole is what the importer receives under a
    /// name of its own choosing: a default, which never satisfies a named
    /// `must_export`.
    #[test]
    fn a_whole_module_exports_that_is_not_an_object_is_a_default() {
        let facts = parse("src/x.js", "module.exports = function createApp() {};");

        let [export] = facts.exports.as_slice() else {
            panic!("one export, got {:?}", facts.exports);
        };
        assert!(export.is_default);
        assert!(export.name.is_none());
        assert_eq!(kinds(export), [ExportKind::Function]);
    }

    #[test]
    fn a_default_carries_the_form_of_what_it_names() {
        let facts = parse(
            "src/x.js",
            "class UserService {}\nmodule.exports = UserService;",
        );

        let export = facts.exports.first().expect("one export");
        assert!(export.is_default);
        assert_eq!(kinds(export), [ExportKind::Class]);
    }

    /// A value with no declaration form has none, as `export default 42` has
    /// none; an arrow is still an arrow.
    #[test]
    fn a_default_that_is_a_bare_value_has_no_form_and_an_arrow_is_an_arrow() {
        let value = parse("src/a.js", "module.exports = createApp();");
        assert!(value.exports.first().expect("one").tags.is_empty());

        let arrow = parse("src/b.js", "module.exports = () => {};");
        assert_eq!(
            kinds(arrow.exports.first().expect("one")),
            [ExportKind::Arrow]
        );
    }

    /// The classic `exports = module.exports = X`. The bare `exports` is a
    /// rebinding of a local and exports nothing by itself.
    #[test]
    fn the_exports_alias_idiom_is_one_default() {
        let facts = parse("src/x.js", "exports = module.exports = function () {};");

        assert_eq!(facts.exports.len(), 1);
        assert!(facts.exports.first().expect("one").is_default);
    }

    /// TypeScript's `export =` is `module.exports =` with a keyword.
    #[test]
    fn a_typescript_export_assignment_is_a_default() {
        let facts = parse("src/x.cts", "class Client {}\nexport = Client;");

        let export = facts.exports.first().expect("one export");
        assert!(export.is_default);
        assert_eq!(kinds(export), [ExportKind::Class]);
    }

    #[test]
    fn a_default_and_named_properties_coexist() {
        let facts = parse(
            "src/x.js",
            "module.exports = function main() {};\nmodule.exports.helper = () => {};",
        );

        assert_eq!(facts.exports.len(), 2);
        assert!(facts.exports.first().expect("first").is_default);
        assert!(!named(&facts, "helper").is_default);
    }

    // ---- forwarding -------------------------------------------------------

    /// `module.exports = require('./x')` is `export * from './x'`: the file
    /// exports a set nobody here can enumerate, and adds nothing to it.
    #[test]
    fn a_whole_module_exports_of_a_require_is_a_star_reexport() {
        let facts = parse("src/index.js", "module.exports = require('./impl');");

        let export = named(&facts, "*");
        assert_eq!(kinds(export), [ExportKind::Reexport]);
        assert_eq!(export.reexport_from.as_deref(), Some("./impl"));
        assert_eq!(export.forwards.as_deref(), Some("*"));
        assert!(!export.is_default);
        assert_eq!(specifiers(&facts), ["./impl"], "and it is still an import");
    }

    /// A call to anything else returns a value this file computed, and
    /// re-exports nothing -- whatever string it was handed.
    #[test]
    fn a_call_that_is_not_a_require_is_never_a_reexport() {
        let facts = parse(
            "src/x.js",
            "module.exports = load('./impl');\nexports.user = load('./user');",
        );

        let whole = facts.exports.first().expect("the default");
        assert!(whole.is_default);
        assert!(whole.reexport_from.is_none());

        let user = named(&facts, "user");
        assert_eq!(kinds(user), [ExportKind::Const]);
        assert!(user.reexport_from.is_none());
    }

    /// The barrel: `{ ...require('./a'), b: require('./b') }`.
    #[test]
    fn a_spread_require_in_an_exported_object_is_a_star_reexport() {
        let facts = parse(
            "src/index.js",
            "module.exports = { ...require('./a'), ...local, own: 1 };",
        );

        assert_eq!(named(&facts, "*").reexport_from.as_deref(), Some("./a"));
        assert_eq!(facts.exports.len(), 2, "a spread of a local names nothing");
    }

    /// A member of a require is one thing out of a module, not the module:
    /// spreading it, or assigning it whole, re-exports no set.
    #[test]
    fn a_member_of_a_require_is_never_a_star_reexport() {
        let spread = parse(
            "src/a.js",
            "module.exports = { ...require('./a').helpers };",
        );
        assert!(spread.exports.is_empty(), "{:?}", spread.exports);

        let whole = parse("src/b.js", "module.exports = require('./b').client;");
        let export = whole.exports.first().expect("one export");
        assert!(export.is_default);
        assert_eq!(kinds(export), [ExportKind::Reexport]);
        assert_eq!(export.reexport_from.as_deref(), Some("./b"));
    }

    /// `exports.user = require('./user')` and `exports.save =
    /// require('./event').save` hold a name and nothing else, which is what
    /// `export { save } from './event'` is.
    #[test]
    fn a_named_export_of_a_require_is_a_reexport() {
        let facts = parse(
            "src/index.js",
            "exports.user = require('./user');\nexports.save = require('./event').save;",
        );

        let user = named(&facts, "user");
        assert_eq!(kinds(user), [ExportKind::Reexport]);
        assert_eq!(user.reexport_from.as_deref(), Some("./user"));
        assert_eq!(user.forwards.as_deref(), Some("user"));

        let save = named(&facts, "save");
        assert_eq!(save.reexport_from.as_deref(), Some("./event"));
        assert_eq!(save.forwards.as_deref(), Some("save"));
    }

    /// `const { A } = require('./a'); module.exports = { A }` is a re-export
    /// written the long way. An ES module's version has no local declaration
    /// and so no tags, which is the shape `no-passthrough` reads as one; a
    /// `const` holding a `require` is the same file and must look the same.
    #[test]
    fn exporting_a_required_binding_is_a_forward_with_no_form_of_its_own() {
        let facts = parse(
            "src/index.js",
            "const { A } = require('./a');\nconst b = require('./b');\nmodule.exports = { A, renamed: b };",
        );

        let a = named(&facts, "A");
        assert!(a.tags.is_empty());
        assert_eq!(a.forwards.as_deref(), Some("A"));
        assert!(a.reexport_from.is_none());

        let renamed = named(&facts, "renamed");
        assert!(renamed.tags.is_empty());
        assert_eq!(renamed.forwards.as_deref(), Some("b"));
    }

    /// An `import x = require()` binding is a required binding too.
    #[test]
    fn exporting_an_import_equals_binding_is_a_forward() {
        let facts = parse(
            "src/index.cts",
            "import impl = require('./impl');\nexports.impl = impl;",
        );

        let export = named(&facts, "impl");
        assert!(export.tags.is_empty());
        assert_eq!(export.forwards.as_deref(), Some("impl"));
    }

    /// `const A = B; exports.A = A` is the alias `export const A = B` is.
    #[test]
    fn exporting_an_alias_forwards_what_the_alias_names() {
        let facts = parse(
            "src/x.js",
            "const { shared } = require('./shared');\nconst mine = shared;\nexports.mine = mine;",
        );

        let export = named(&facts, "mine");
        assert_eq!(export.forwards.as_deref(), Some("shared"));
        assert_eq!(kinds(export), [ExportKind::Const]);
    }

    /// A name the file never declared came from somewhere else, and the rule
    /// that has the imports decides whether that somewhere is an import.
    #[test]
    fn exporting_a_name_the_file_never_declared_forwards_that_name() {
        let facts = parse("src/x.js", "exports.env = process;");

        let export = named(&facts, "env");
        assert_eq!(export.forwards.as_deref(), Some("process"));
        assert!(export.tags.is_empty());
    }

    /// A require inside a function binds a name in that function, not at the
    /// top level, so a top-level declaration of the same name is the file's
    /// own.
    #[test]
    fn a_require_bound_inside_a_function_does_not_make_a_top_level_name_required() {
        let facts = parse(
            "src/x.js",
            "function load() {\n  const helper = require('./helper');\n  return helper;\n}\nfunction helper() {}\nexports.helper = helper;",
        );

        let export = named(&facts, "helper");
        assert_eq!(kinds(export), [ExportKind::Function]);
        assert!(export.forwards.is_none());
    }

    /// An ES module's exports are untouched by any of this.
    #[test]
    fn an_es_module_exports_what_it_did_before() {
        let facts = parse(
            "src/x.ts",
            "export const a = 1;\nexport default function () {}",
        );

        assert_eq!(facts.exports.len(), 2);
    }
}
