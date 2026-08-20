//! A file that must actually call something.

use archwarden_core::{
    ids::{DecisionId, RuleId},
    level::Level,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Patterns;

/// Files matching a pattern must call a given symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CallObligationRule {
    /// Stable identifier.
    pub id: RuleId,
    /// Severity.
    pub level: Level,
    /// Why this rule exists, in the author's words.
    ///
    /// The config already says *what* a rule does, in a form that cannot drift
    /// from what it enforces; a prose restatement of that would be a second
    /// source of truth going stale. The reason cannot drift, because nothing
    /// else records it — decision 5 chose JSON, so there are no comments, and
    /// a commit message is not in front of anybody at the moment a rule fires.
    ///
    /// Shown by the pre-write hook when it denies a write, by `describe`,
    /// `scaffold`, `agent-guide` and `config explain`, and beside a finding.
    /// Not a message override: `observed` and `expected` remain the whole
    /// diagnosis, and a `why` restating them will contradict them the day the
    /// rule changes. Issue #46.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The decision this rule implements, when it implements a declared one.
    ///
    /// A plain foreign key into [`Config::decisions`](crate::config::Config::decisions),
    /// written here rather than as a list of rule ids on the decision: this is
    /// where the author already is, there is no second list to keep in step, a
    /// deleted rule leaves nothing dangling, and a new rule that forgets its
    /// decision is visible in the one place it exists. Naming a decision the
    /// config does not declare is refused at compile — a reference to nothing
    /// is a typo, not a style. Issue #100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionId>,
    /// Directory globs this rule applies to.
    pub roots: Patterns,
    /// Regex over the filename.
    pub file_pattern: String,
    /// The call the file must contain.
    pub must_call: MustCall,
    /// Narrow this rule to the files that import something.
    ///
    /// Path globs, matched against where an import *lands* — the same way an
    /// `import-boundary` matches. Without this a rule's population is where a
    /// file sits and what it is called; with it, what the file talks to.
    ///
    /// Leave it out and nothing changes, including the cost: a rule that does
    /// not ask never resolves an import. Issue #98, decision 25.
    #[serde(default, skip_serializing_if = "Patterns::is_empty")]
    pub when_importing: Patterns,
    /// The same, for packages rather than paths.
    ///
    /// Matched against the package a specifier belongs to, so `zod` covers
    /// `zod/v4` as it does everywhere else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when_importing_packages: Vec<String>,
}

/// The call a `call-obligation` rule requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MustCall {
    /// The callee as it appears at a call site, e.g. `Event.save`. Method
    /// chains are matched exactly.
    pub symbol: String,
    /// The module the symbol must be imported from, which disambiguates
    /// same-named functions from different packages.
    pub imported_from: String,
    /// Options the call must carry, when the call alone is not the statement.
    ///
    /// See [`WithOptions`]. Leave it out and the rule is exactly what it was:
    /// the symbol is imported and called, and what it is given is not asked
    /// about.
    #[serde(default, skip_serializing_if = "WithOptions::is_empty")]
    pub with_options: WithOptions,
}

/// The options a required call must carry.
///
/// An options bag is how TypeScript spells the argument whose presence changes
/// what a call does. `FactoryMockDependencies(ENV, { PAY_IN_MEMORY: "all" })`
/// and `FactoryMockDependencies()` are the same callee at the same arity and
/// opposite meanings -- one runs against in-memory twins, the other starts a
/// container -- and nothing in the file says which. Issue #164.
///
/// Two spellings, because presence and value are different questions:
///
/// ```json
/// "with_options": ["PAY_IN_MEMORY"]
/// "with_options": { "PAY_IN_MEMORY": "all" }
/// ```
///
/// A list asks only that the key be there, which is the case this was built
/// for: the value never varies, and a rule made to name one would be naming a
/// thing it does not care about. A map asks for the value too, rendered as
/// written -- `"all"`, `false`, `3`.
///
/// A sequence and a map are told apart by their JSON type before any field is
/// read, so this is not the kind of untagged union that reports "data did not
/// match any variant" at the wrong line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum WithOptions {
    /// Keys that must be present, whatever they hold.
    Present(Vec<String>),
    /// Keys that must be present holding exactly this.
    Holding(std::collections::BTreeMap<String, String>),
}

impl WithOptions {
    /// Whether the rule asks for no options at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Present(keys) => keys.is_empty(),
            Self::Holding(pairs) => pairs.is_empty(),
        }
    }

    /// The pairs as the engine wants them: a key, and a value when one is
    /// asked for.
    #[must_use]
    pub fn pairs(&self) -> Vec<(String, Option<String>)> {
        match self {
            Self::Present(keys) => keys.iter().map(|key| (key.clone(), None)).collect(),
            Self::Holding(pairs) => pairs
                .iter()
                .map(|(key, value)| (key.clone(), Some(value.clone())))
                .collect(),
        }
    }
}

impl Default for WithOptions {
    fn default() -> Self {
        Self::Present(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rule that asks for no options serialises without the field, so a
    /// config written back out is the config that was read. Every rule
    /// authored before #164 is this case, and a `"with_options": []` appearing
    /// in all of them would be a diff nobody asked for.
    #[test]
    fn an_empty_ask_is_left_out_of_the_serialised_rule() {
        let bare = MustCall {
            symbol: "factory".to_owned(),
            imported_from: "m".to_owned(),
            with_options: WithOptions::default(),
        };
        assert!(bare.with_options.is_empty());
        assert_eq!(
            serde_json::to_string(&bare).expect("serialises"),
            r#"{"symbol":"factory","imported_from":"m"}"#
        );

        let asking = MustCall {
            with_options: WithOptions::Present(vec!["PAY_IN_MEMORY".to_owned()]),
            ..bare
        };
        assert!(!asking.with_options.is_empty());
        assert_eq!(
            serde_json::to_string(&asking).expect("serialises"),
            r#"{"symbol":"factory","imported_from":"m","with_options":["PAY_IN_MEMORY"]}"#
        );
    }

    /// And an empty map is empty too -- the same statement in the other
    /// spelling, which `is_empty` has to answer the same way.
    #[test]
    fn an_empty_map_asks_for_nothing_either() {
        assert!(WithOptions::Holding(std::collections::BTreeMap::new()).is_empty());
        assert!(
            !WithOptions::Holding(
                [("PAY_IN_MEMORY".to_owned(), "all".to_owned())]
                    .into_iter()
                    .collect()
            )
            .is_empty()
        );
    }
}
