//! Severity. There are two levels and there will not be a third.
//!
//! Linters that ship `info` and `hint` tend to see those levels ignored
//! entirely, which then erodes confidence that warnings mean anything. Two
//! levels force rule authors to decide up front whether a rule is a gate or a
//! signpost. See decision 1.

use serde::{Deserialize, Serialize};

/// How seriously a finding is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Reported and visible, but the run still succeeds. For technical debt
    /// that must stay in sight until it is paid down.
    Warning,
    /// Fails the run. Blocks CI.
    Error,
}

impl Level {
    /// Whether a finding at this level should make the run fail.
    ///
    /// This is the single place that decides it, so exit codes and reporting
    /// can never disagree about what "error" means.
    #[must_use]
    pub fn fails_build(self) -> bool {
        matches!(self, Self::Error)
    }

    /// The spelling used in configs and in output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of having two levels: one blocks, one does not.
    #[test]
    fn only_error_fails_the_build() {
        assert!(Level::Error.fails_build());
        assert!(!Level::Warning.fails_build());
    }

    /// `Ord` exists so a report can be sorted worst-first without the caller
    /// inventing its own ranking.
    #[test]
    fn error_sorts_above_warning() {
        assert!(Level::Error > Level::Warning);

        let mut levels = [Level::Warning, Level::Error, Level::Warning];
        levels.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(levels, [Level::Error, Level::Warning, Level::Warning]);
    }

    /// Configs are written by hand, so the wire spelling is lowercase and has
    /// to stay that way: changing it silently breaks every existing config.
    #[test]
    fn the_wire_spelling_is_lowercase_in_both_directions() {
        assert_eq!(
            serde_json::to_string(&Level::Error).expect("serialises"),
            "\"error\""
        );
        assert_eq!(
            serde_json::to_string(&Level::Warning).expect("serialises"),
            "\"warning\""
        );

        let parsed: Level = serde_json::from_str("\"warning\"").expect("deserialises");
        assert_eq!(parsed, Level::Warning);
    }

    /// A capitalised level is a typo, not an alias. Accepting it would make
    /// configs that disagree with the schema still work, which is worse.
    #[test]
    fn a_capitalised_level_is_rejected() {
        assert!(serde_json::from_str::<Level>("\"Error\"").is_err());
        assert!(serde_json::from_str::<Level>("\"ERROR\"").is_err());
        assert!(serde_json::from_str::<Level>("\"info\"").is_err());
    }

    #[test]
    fn display_matches_the_wire_spelling() {
        assert_eq!(Level::Error.to_string(), "error");
        assert_eq!(Level::Warning.to_string(), "warning");
        assert_eq!(Level::Error.to_string(), Level::Error.as_str());
    }
}
