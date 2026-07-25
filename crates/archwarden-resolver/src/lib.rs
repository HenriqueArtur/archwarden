//! Module resolution, backed by `oxc_resolver`.
//!
//! Exposes a `Resolver` trait with two production uses and one test use:
//!
//! - resolving a preset package name for `extends` (used by
//!   `archwarden-config`),
//! - resolving import specifiers to canonical repo-relative paths, honouring
//!   `tsconfig` paths, `exports` conditions and workspaces (used by the
//!   import-boundary rules),
//! - an in-memory implementation for fixtures, so graph rules can be tested
//!   without a filesystem.
//!
//! Rule engines never call a resolver. They consume already-resolved paths, so
//! swapping the implementation never touches rule code (decision 6).
