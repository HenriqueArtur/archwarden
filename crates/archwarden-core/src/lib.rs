//! Shared types, traits, and matching primitives for archwarden.
//!
//! This crate has **no internal dependencies**. Everything else in the
//! workspace depends on it, and that is what lets `archwarden-rules` consume
//! only extracted facts while staying independent of the parser and resolver.
//!
//! It also owns the *compiled* side of the configuration. Where
//! `archwarden-config` deserialises a glob as a `String`, this crate holds the
//! built `GlobSet`; a compiled rule cannot be constructed unless its globs and
//! regexes are valid, so no downstream code ever has to re-check them.
//!
//! # Layout
//!
//! - [`compiled`] — rules with every glob and regex already compiled
//! - [`facts`] — what a parser extracts from one file
//! - [`finding`] — what a rule reports, and why
//! - [`glob`] — glob sets matched against whole paths
//! - [`graph`] — who imports whom, for the rules one file cannot answer
//! - [`hash`] — content hashing, the basis of both cache keys
//! - [`ids`] — stable identifiers for rules and modules
//! - [`level`] — severity, of which there are exactly two
//! - [`path`] — repository-relative paths
//! - [`pattern`] — filename regexes, and the constructs we refuse
//! - [`scope`] — the `roots` / `from` directory matcher
//! - [`template`] — the `{{pascal(name)}}` mini-template
//! - [`traits`] — the parser, resolver and rule-engine seams
//!
//! See `docs/ARCHITECTURE.md`.

// Each module documents itself with an inner `//!` block. Do not add `///`
// doc comments here as well: rustdoc concatenates the two and then resolves
// every intra-doc link in *this* scope, so a `[`FileFacts`]` written inside
// `facts` would fail to resolve.
pub mod compiled;
pub mod docs;
pub mod facts;
pub mod finding;
pub mod glob;
pub mod graph;
pub mod hash;
pub mod ids;
pub mod level;
pub mod path;
pub mod pattern;
pub mod scope;
pub mod template;
pub mod traits;
