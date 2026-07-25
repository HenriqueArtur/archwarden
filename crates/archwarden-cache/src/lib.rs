//! Content-addressed on-disk cache, stored in `redb` under
//! `.archwarden/cache/`.
//!
//! Two tables with different invalidation triggers:
//!
//! - `facts`, keyed by content hash alone,
//! - `findings`, keyed by content hash, rules hash, and resolution epoch.
//!
//! Keeping them apart means editing one rule in the config does not throw away
//! every parse result in the repository.
//!
//! The resolution epoch covers `tsconfig*.json`, `package.json`, and lockfiles:
//! import-boundary findings depend on how specifiers resolve, so without it a
//! warm run would serve stale findings after a `tsconfig.paths` change.
//!
//! See `docs/ARCHITECTURE.md`.
