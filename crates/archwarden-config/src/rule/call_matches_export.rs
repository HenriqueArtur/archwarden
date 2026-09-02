//! Two vocabularies that have to agree, across files and across languages.

use archwarden_core::{
    ids::{DecisionId, ModuleId, RuleId},
    level::Level,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::one_or_many::OneOrMany;

use super::Patterns;

/// Every name a call asks for is declared somewhere, and optionally the
/// reverse.
///
/// The seam a Tauri application is joined by: `invoke("save_document")` in the
/// webview and `#[tauri::command] fn save_document` in the backend are the
/// same edge, with no import between them. Deliberately not a `tauri` rule —
/// `t("checkout.title")` against a translation catalogue is the same question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CallMatchesExportRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words. The same field, with the
    /// same meaning, is on every rule kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Why this rule's scope is empty on purpose, when it is. The same field,
    /// with the same meaning, is on every rule kind — see `StructureRule`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_yet: Option<String>,
    /// Where the calls are read from.
    pub roots: Patterns,
    /// The callee whose argument names something, e.g. `invoke`.
    pub callee: String,
    /// Which argument holds the name. Zero-based; the first by default,
    /// because every framework that does this puts it first.
    #[serde(default)]
    pub argument: usize,
    /// Where the declarations live.
    pub declared_in: Patterns,
    /// The attribute a declaration carries to be one, e.g. `tauri::command`.
    ///
    /// Written without the brackets. Omitted, every named export in
    /// `declared_in` counts — which is what a translation catalogue wants and
    /// what a command surface does not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribute: Option<String>,
    /// Whether a declaration nobody calls is reported.
    ///
    /// Off by default, and the asymmetry is deliberate. A call naming nothing
    /// is unambiguous. A declaration nobody calls is not: archwarden reads the
    /// languages it has front-ends for, and a command called from one it does
    /// not read looks exactly like a command nobody calls.
    #[serde(default)]
    pub report_uncalled: bool,
    /// Narrow the rule to files importing these.
    #[serde(default, skip_serializing_if = "OneOrMany::is_empty")]
    pub when_importing: Patterns,
    /// Narrow the rule to files importing these packages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
    /// The module this rule belongs to, when it was declared inside one.
    #[serde(skip)]
    pub module: Option<ModuleId>,
}
