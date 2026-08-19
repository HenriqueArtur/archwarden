//! A rule's scope, the declared modules, and the hash of the rules.

use archwarden_core::{hash::ContentHash, ids::RuleId, scope::Scope};

use crate::config::Config;
use crate::rule::Rule;

use super::error::CompileError;

/// A rule's scope, from whichever field it used.
///
/// A boundary may say who it is about as globs (`from`) or as a module
/// (`from_module`), and exactly one of those is required. Both is refused
/// rather than resolved: two spellings of one scope on one rule is the
/// ambiguity that produces a rule enforcing something nobody meant, and unlike
/// glob containment this one is decidable at compile time.
pub(super) fn compile_scope(
    rule: &Rule,
    id: &RuleId,
    modules: &Modules,
    inside: Option<&archwarden_core::ids::ModuleId>,
) -> Result<Scope, CompileError> {
    let own = if let Rule::ImportBoundary(boundary) = rule {
        // A kind selects every module that wears it, which is the whole point:
        // the seventh assembly is governed because it exists, not because
        // somebody remembered. Issue #76.
        if let Some(kind) = &boundary.from_kind {
            if !boundary.from.is_empty() || boundary.from_module.is_some() {
                return Err(CompileError::ScopeSaidTwice {
                    rule: id.clone(),
                    one: "from_kind",
                    other: "from",
                });
            }
            let patterns = modules.paths_of_kind(id, kind)?;
            return Scope::compile(&patterns)
                .map_err(|source| CompileError::Scope {
                    rule: id.clone(),
                    source,
                })
                .map(|own| {
                    inside
                        .and_then(|m| modules.scopes.get(m))
                        .map_or(own.clone(), |outer| own.within(outer))
                });
        }
        match (boundary.from.is_empty(), boundary.from_module.as_ref()) {
            (false, Some(_)) => {
                return Err(CompileError::ScopeSaidTwice {
                    rule: id.clone(),
                    one: "from",
                    other: "from_module",
                });
            }
            (true, None) => {
                return Err(CompileError::ScopeMissing {
                    rule: id.clone(),
                    one: "from",
                    other: "from_module",
                });
            }
            (true, Some(named)) => Scope::compile(modules.paths_of(id, named)?),
            (false, None) => Scope::compile(rule.scope()),
        }
    } else {
        Scope::compile(rule.scope())
    }
    .map_err(|source| CompileError::Scope {
        rule: id.clone(),
        source,
    })?;

    // Narrowed, never replaced: a rule keeps its own scope and reaches where
    // the module it lives in also reaches. See `Scope::within` for why this is
    // not a refusal.
    Ok(inside
        .and_then(|id| modules.scopes.get(id))
        .map_or(own.clone(), |outer| own.within(outer)))
}

/// Hashes the effective rule set, for the `findings` cache key.
///
/// Derived from the merged config's serialised rules rather than from the
/// files on disk, so a preset reshuffle that produces the same rules does not
/// invalidate the cache, while any real change to a rule does.
pub(super) fn rules_hash(config: &Config) -> ContentHash {
    let rules: Vec<_> = config.rules().collect();
    let serialised = serde_json::to_vec(&rules).unwrap_or_default();
    ContentHash::of(&serialised)
}

/// The modules a config declares that have paths of their own.
///
/// Compiled once and consulted by every rule, rather than once per rule: a
/// module of nine rules would otherwise build the same globs nine times, and
/// a boundary naming a module needs one it does not live in.
pub(super) struct Modules {
    /// Scope by id, for narrowing a rule that lives inside one.
    pub(super) scopes: std::collections::BTreeMap<archwarden_core::ids::ModuleId, Scope>,
    /// The patterns as written, for a rule that names a module and needs its
    /// paths as a `PathSet` rather than as a scope.
    pub(super) patterns: std::collections::BTreeMap<archwarden_core::ids::ModuleId, Vec<String>>,
    /// Every id the config declares, including those with no scope: naming one
    /// of those is a different mistake from naming one that does not exist,
    /// and the two deserve different sentences.
    pub(super) declared: std::collections::BTreeSet<archwarden_core::ids::ModuleId>,
    /// What sort each module said it is, for rules that quantify over sorts.
    pub(super) kinds: std::collections::BTreeMap<archwarden_core::ids::ModuleId, String>,
}

impl Modules {
    pub(super) fn compile(config: &Config) -> Result<Self, CompileError> {
        let mut scopes = std::collections::BTreeMap::new();
        let mut patterns = std::collections::BTreeMap::new();
        let mut declared = std::collections::BTreeSet::new();
        let mut kinds = std::collections::BTreeMap::new();

        for module in &config.modules {
            declared.insert(module.id.clone());
            if let Some(kind) = &module.kind {
                kinds.insert(module.id.clone(), kind.clone());
            }
            if module.scope.is_empty() {
                continue;
            }
            let scope =
                Scope::compile(&module.scope).map_err(|source| CompileError::ModuleScope {
                    module: module.id.clone(),
                    source,
                })?;
            scopes.insert(module.id.clone(), scope);
            patterns.insert(
                module.id.clone(),
                module.scope.iter().map(ToOwned::to_owned).collect(),
            );
        }

        Ok(Self {
            scopes,
            patterns,
            declared,
            kinds,
        })
    }

    /// The paths every module of this sort is.
    ///
    /// A kind nothing wears is refused rather than compiled into a scope that
    /// selects nothing: a rule quantifying over an empty set governs nothing,
    /// silently, which is the failure the quantifier exists to remove.
    pub(super) fn paths_of_kind(
        &self,
        rule: &RuleId,
        kind: &str,
    ) -> Result<Vec<String>, CompileError> {
        let mut collected = Vec::new();
        for (id, worn) in &self.kinds {
            if worn != kind {
                continue;
            }
            collected.extend(self.paths_of(rule, id)?.iter().cloned());
        }

        if collected.is_empty() {
            return Err(CompileError::UnknownKind {
                rule: rule.clone(),
                kind: kind.to_owned(),
            });
        }
        Ok(collected)
    }

    /// The modules, as the rest of the run sees them.
    pub(super) fn compiled(&self) -> Vec<archwarden_core::compiled::CompiledModule> {
        self.declared
            .iter()
            .map(|id| archwarden_core::compiled::CompiledModule {
                id: id.clone(),
                scope: self.scopes.get(id).cloned(),
                kind: self.kinds.get(id).cloned(),
            })
            .collect()
    }

    /// The paths a named module is, or why it cannot answer.
    pub(super) fn paths_of(
        &self,
        rule: &RuleId,
        module: &archwarden_core::ids::ModuleId,
    ) -> Result<&[String], CompileError> {
        if !self.declared.contains(module) {
            return Err(CompileError::UnknownModule {
                rule: rule.clone(),
                module: module.clone(),
            });
        }
        self.patterns
            .get(module)
            .map(Vec::as_slice)
            .ok_or_else(|| CompileError::ModuleHasNoScope {
                rule: rule.clone(),
                module: module.clone(),
            })
    }
}
