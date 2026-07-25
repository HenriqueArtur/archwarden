//! `archwarden` — command-line entry point.
//!
//! Argument parsing and output formatting only. All real work happens in
//! `archwarden-engine`; this crate decides how to render the result as text,
//! JSON, or markdown, and which exit code to return:
//! `0` clean, `1` findings at error level, `2` configuration problem.

// The binary is the one place that talks to the terminal. Libraries return
// values; see the workspace lint table.
#![allow(clippy::print_stdout, clippy::print_stderr)]

fn main() {
    println!(
        "archwarden {} (skeleton, no commands yet)",
        env!("CARGO_PKG_VERSION")
    );
}
