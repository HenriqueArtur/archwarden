//! JS/TS parsing and fact extraction, backed by `oxc_parser`.
//!
//! Exposes a `Parser` trait so the front-end can be replaced without touching
//! rule code (decision 6). Downstream crates never see an AST — only the
//! extracted `FileFacts` defined in `archwarden-core`.
//!
//! Adding another language means implementing this trait and populating the
//! same fact types. No rule engine changes.
