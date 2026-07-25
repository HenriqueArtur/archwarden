//! Pipeline orchestration: walk, cache probe, parse, resolve, run rules,
//! persist, report.
//!
//! This crate exists so the pipeline is not owned by the binary. The v1
//! language server needs the same pipeline, and depending on a binary crate to
//! reach it would be backwards.
//!
//! Each stage takes owned inputs and returns owned outputs. No stage shares
//! mutable state with another, which is what makes them independently testable
//! and independently replaceable.
