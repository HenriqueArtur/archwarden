//! The operations behind archwarden's surfaces.
//!
//! The CLI, the agent hook, and later MCP and an LSP server all ask the same
//! questions of a repository. Before this crate existed they each assembled
//! the answer themselves, and the assembly was entangled with how one of them
//! reports failure: `prepare()` in the CLI wrote a miette report to stderr and
//! returned an exit code, so any surface that reports errors differently had
//! to re-implement the path rather than reuse it. Two of them did — the
//! pre-write hook and the end-of-turn hook — and the missing version guard
//! that shipped as issue #55 was in one of the copies.
//!
//! So the rule this crate exists to enforce, and the one to break before
//! anything else here is worth reading:
//!
//! > **Nothing in `archwarden-api` writes, and no function here takes a
//! > writer.** Every failure is a value the caller renders.
//!
//! The workspace already denies `print_stdout` and `print_stderr` outside the
//! binaries, but that lint never caught `prepare()`, which wrote through a
//! `&mut dyn Write` it was handed. The enforcement here is structural instead:
//! this crate does not depend on `archwarden-cli`, and no signature in it
//! mentions an output sink. A surface turns [`Error`] into stderr and an exit
//! code, or into a `systemMessage`, or into a JSON-RPC error, and nobody
//! re-walks the path to change the shape of a failure.
//!
//! # The stages
//!
//! ```text
//! Resolve  →  Load  →  Walk  →  Evaluate  →  Present
//! (config)   (rules)   (tree)   (findings)    (view)
//! ```
//!
//! Naming them is worth it even where there is only one implementation: it is
//! what lets a future surface say *"the LSP reuses through Evaluate and brings
//! its own Present"* instead of negotiating the boundary from scratch.

pub mod error;
pub mod evaluate;
pub mod resolve;
pub mod walk;

pub use error::Error;
pub use evaluate::{
    CACHE_DIRECTORY, CACHE_FILE, CachePolicy, Evaluated, Evaluation, Note, cache_path, evaluate,
};
pub use resolve::{Location, Prepared, load, prepare, resolve};
pub use walk::walk;
