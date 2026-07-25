//! The five rule engines: structure, naming, spec-pair, import-boundary,
//! call-obligation.
//!
//! Depends on `archwarden-core` **only** — never on the parser or the
//! resolver. Engines receive extracted facts and compiled rules, and return
//! findings.
//!
//! Every engine implements `describe_expectation()` alongside its check logic.
//! That is not optional: `scaffold` and `agent-guide` are built from it, so a
//! rule whose expectation is not describable does not compile. This is how
//! decision 9 stays true rather than aspirational.
//!
//! See `docs/RULES.md`.

// Modules document themselves with `//!`; see the note in archwarden-core.
pub mod structure;
