//! Configuration for archwarden: schema, discovery, `extends`, validation.
//!
//! Owns the *wire format* — the types carrying `Deserialize` and `JsonSchema`
//! — and lowers them into the compiled types in `archwarden-core`.
//!
//! Discovery walks up from the current directory to find `arch.config.json`,
//! mirroring how `git` finds `.git` (decision 4).
//!
//! This crate depends on `archwarden-resolver` because `extends` accepts npm
//! package names, and turning one into a file path is full Node module
//! resolution. See `docs/CONFIG.md`.

// Modules document themselves with `//!`; see the note in archwarden-core.
pub mod config;
pub mod discovery;
pub mod one_or_many;
pub mod rule;
