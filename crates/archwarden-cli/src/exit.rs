//! Exit codes.
//!
//! These are a public contract: CI pipelines and agent hooks branch on them,
//! so the mapping lives in one place and is tested rather than being spelled
//! out at each return site.

/// What archwarden tells its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Exit {
    /// Nothing to report, or only warnings.
    Clean,
    /// At least one finding at `error` level. Blocks CI.
    Errors,
    /// The config could not be found, read, or understood. Distinct from
    /// `Errors` on purpose: "your config is broken" and "your code violates
    /// your config" call for different reactions from a pipeline.
    ConfigProblem,
}

impl Exit {
    /// The numeric code.
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Clean => 0,
            Self::Errors => 1,
            Self::ConfigProblem => 2,
        }
    }
}

impl From<Exit> for std::process::ExitCode {
    fn from(exit: Exit) -> Self {
        Self::from(exit.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers README.md promises. Changing one breaks every pipeline that
    /// branches on it, so they are pinned here rather than left implicit.
    #[test]
    fn the_documented_codes_are_zero_one_and_two() {
        assert_eq!(Exit::Clean.code(), 0);
        assert_eq!(Exit::Errors.code(), 1);
        assert_eq!(Exit::ConfigProblem.code(), 2);
    }

    /// A broken config and a failing check must not look the same to a
    /// pipeline: one means "fix your setup", the other "fix your code".
    #[test]
    fn a_config_problem_is_distinguishable_from_findings() {
        assert_ne!(Exit::ConfigProblem.code(), Exit::Errors.code());
        assert_ne!(Exit::ConfigProblem, Exit::Errors);
    }

    #[test]
    fn conversion_to_a_process_code_preserves_the_number() {
        for exit in [Exit::Clean, Exit::Errors, Exit::ConfigProblem] {
            let converted = std::process::ExitCode::from(exit);
            assert_eq!(
                format!("{converted:?}"),
                format!("{:?}", std::process::ExitCode::from(exit.code()))
            );
        }
    }
}
