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
