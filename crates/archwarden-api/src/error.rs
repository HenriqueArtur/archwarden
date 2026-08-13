//! What an operation returns when it cannot answer.
//!
//! One enum, several renderings. The CLI turns a variant into a miette report
//! with a caret under the offending line and exit code 2; the pre-write hook
//! turns the same variant into a sentence in a `systemMessage` and exits
//! clean; MCP will turn it into a JSON-RPC error. None of them re-walks the
//! path to change the shape of a failure, which is the whole argument of
//! issue #63.
//!
//! The variants wrap the domain errors rather than flattening them into
//! strings, because the CLI still needs `LoadError::Invalid`'s source text and
//! byte offsets to draw that caret. A boundary that loses them would trade one
//! duplication for a worse diagnostic.

use archwarden_config::{compile::CompileError, discovery::LoadError, extends::ExtendsError};
use camino::Utf8PathBuf;

/// A reason an operation could not answer.
///
/// `non_exhaustive` because surfaces match on this and a new stage will add a
/// variant. A surface that has not been taught the new one keeps compiling and
/// falls through to its generic rendering, which is always the correct message
/// and only ever misses the extra help.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The config could not be found, read, or parsed.
    #[error(transparent)]
    Load(#[from] LoadError),

    /// The config declares a schema version this build does not understand.
    ///
    /// Its own variant rather than a `LoadError`, because it is not a problem
    /// with the file: the file is fine and this binary is the wrong reader of
    /// it. That distinction is the one issue #55 turned on. A future version
    /// deserialises cleanly into a config with no rules the current build
    /// recognises, so every downstream stage succeeds and the gate evaporates
    /// rather than failing. Nothing but this check catches it.
    #[error(
        "`{path}` declares version {declared}, but this build understands version {understood}"
    )]
    UnsupportedVersion {
        /// The config file that declared it.
        path: Utf8PathBuf,
        /// The version the file asked for.
        declared: u32,
        /// The version this build reads.
        understood: u32,
    },

    /// A preset in the `extends` chain is missing, invalid, or loops.
    #[error(transparent)]
    Extends(#[from] ExtendsError),

    /// A glob, pattern or template in the config did not compile.
    #[error(transparent)]
    Compile(#[from] CompileError),

    /// The repository could not be read.
    #[error(transparent)]
    Walk(#[from] archwarden_engine::walk::WalkError),

    /// The root holds no source, and is not where the caller is standing.
    ///
    /// A clean run over the wrong directory, which reads as good news and is
    /// not news at all. See [`crate::walk()`] for why the refusal is this narrow
    /// rather than "the root is empty".
    #[error("`{root}` holds no JavaScript or TypeScript, and is not where you are standing")]
    RootHoldsNoSource {
        /// The root that was walked.
        root: Utf8PathBuf,
    },
}

impl Error {
    /// One line saying why nothing could be judged, for a surface with one
    /// line to spend.
    ///
    /// Not the `Display` above, which is the CLI's: that one carries a path
    /// and reads under a caret. This one is written for a reader who is about
    /// to be told *and so this was not checked* — a coding agent through the
    /// pre-write hook, or through MCP — and it names what to go and do.
    ///
    /// It lives here because both of those surfaces need it and neither may
    /// have the other's copy. The pre-write hook wrote its own once, and the
    /// four sentences in it had drifted into repeating the caller's clause; an
    /// MCP server writing a fifth would be the same defect with a new name.
    ///
    /// Every arm ends without punctuation and without saying what follows from
    /// it. The caller says that, and says it once.
    #[must_use]
    pub fn unreadable(&self) -> String {
        match self {
            Self::Load(LoadError::NotFound { .. }) => {
                "no archwarden config was found from here".to_owned()
            }

            // Found, and unusable. Distinct from the arm above because the two
            // send a user to different places: one to `archwarden init`, the
            // other to the file they just edited.
            Self::Load(_) => {
                "the config could not be read — `archwarden config validate` names the problem"
                    .to_owned()
            }

            Self::UnsupportedVersion {
                declared,
                understood,
                ..
            } => format!(
                "the config declares version {declared}, which this build does not understand \
                 (it reads version {understood})"
            ),

            Self::Extends(_) => {
                "the config could not be assembled (a preset it extends is missing, invalid, \
                 or loops)"
                    .to_owned()
            }

            Self::Compile(_) => {
                "the config did not compile — `archwarden config validate` names the problem"
                    .to_owned()
            }

            // `Error` is `non_exhaustive`. A stage added later lands here and
            // the surface still reports that it judged nothing, which is the
            // answer that matters.
            _ => "the config could not be prepared".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sentence a surface with one line to spend says, arm by arm.
    ///
    /// These lived beside a re-implementation of the loading path in the
    /// pre-write hook until 0.18, which is how issue #55 happened: the copy
    /// was missing the version guard, and four separately-written messages had
    /// drifted into repeating the caller's own clause. They are asserted here,
    /// where the sentences are, so a second surface cannot describe one broken
    /// config differently.
    #[test]
    fn a_config_that_is_there_and_broken_is_not_a_config_that_is_missing() {
        let broken = Error::Load(
            archwarden_config::discovery::parse(
                camino::Utf8Path::new("/repo/arch.config.json"),
                r#"{"version": 0,,}"#,
            )
            .expect_err("should not parse"),
        );

        assert_eq!(
            broken.unreadable(),
            "the config could not be read — `archwarden config validate` names the problem"
        );
    }

    /// A different place to send the reader: `archwarden init`, not the file
    /// they just edited.
    #[test]
    fn a_config_that_really_is_missing_says_so() {
        let absent = Error::Load(LoadError::NotFound {
            started_at: Utf8PathBuf::from("/repo/packages/app"),
        });

        assert_eq!(
            absent.unreadable(),
            "no archwarden config was found from here"
        );
    }

    /// Issue #55's sentence, and the one this whole guard exists for. Both
    /// numbers, because "unsupported version" without them tells a reader
    /// nothing about which half to change.
    #[test]
    fn a_future_version_names_both_numbers() {
        let future = Error::UnsupportedVersion {
            path: Utf8PathBuf::from("/repo/arch.config.json"),
            declared: 99,
            understood: 0,
        };

        assert_eq!(
            future.unreadable(),
            "the config declares version 99, which this build does not understand \
             (it reads version 0)"
        );
    }

    #[test]
    fn a_preset_problem_says_which_half_of_the_config_failed() {
        let unresolvable = Error::Extends(ExtendsError::Cycle {
            chain: vec![Utf8PathBuf::from("/repo/arch.config.json")],
        });

        assert_eq!(
            unresolvable.unreadable(),
            "the config could not be assembled (a preset it extends is missing, invalid, \
             or loops)"
        );
    }

    /// A config that parsed and will not compile sends the reader to the
    /// command that names the offending rule: the error itself is about a glob
    /// or a pattern, and this has one line to spend.
    #[test]
    fn a_config_that_did_not_compile_names_the_command_that_explains_it() {
        let uncompilable = Error::Compile(CompileError::Pattern {
            rule: archwarden_core::ids::RuleId::new("lookahead").expect("valid id"),
            field: "file_pattern",
            source: Box::new(
                archwarden_core::pattern::Pattern::compile("^(?!test).*$")
                    .expect_err("a lookahead is not linear-time"),
            ),
        });

        assert_eq!(
            uncompilable.unreadable(),
            "the config did not compile — `archwarden config validate` names the problem"
        );
    }

    /// A stage added later lands in the final arm and the surface still reports
    /// that it judged nothing, which is the answer that matters. `Error` is
    /// `non_exhaustive` precisely so this cannot become a compile error that
    /// somebody fixes by guessing.
    #[test]
    fn a_variant_with_no_sentence_of_its_own_still_says_something() {
        let walked = Error::RootHoldsNoSource {
            root: Utf8PathBuf::from("/repo"),
        };

        assert_eq!(walked.unreadable(), "the config could not be prepared");
    }

    /// No sentence ends in "so this was not checked". The caller says that,
    /// and saying it twice in one line is what the four hand-written copies
    /// had drifted into.
    #[test]
    fn no_sentence_repeats_what_the_caller_already_says() {
        let every = [
            Error::Load(LoadError::NotFound {
                started_at: Utf8PathBuf::from("/repo"),
            }),
            Error::UnsupportedVersion {
                path: Utf8PathBuf::from("/repo/arch.config.json"),
                declared: 99,
                understood: 0,
            },
            Error::RootHoldsNoSource {
                root: Utf8PathBuf::from("/repo"),
            },
        ];

        for error in &every {
            let said = error.unreadable();
            assert!(!said.is_empty(), "every variant has a sentence");
            assert!(!said.ends_with('.'), "the caller punctuates it: {said}");
            assert!(
                !said.contains("not checked"),
                "the caller already says that: {said}"
            );
        }
    }
}
