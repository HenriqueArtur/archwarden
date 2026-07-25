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
//! See `docs/ARCHITECTURE.md`.
