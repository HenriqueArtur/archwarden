//! Repository automation, run as `cargo xtask <task>`.
//!
//! Holds jobs that need to be reproducible across contributors and CI without
//! adding a shell script per platform: JSON Schema generation from the config
//! types, release packaging, and differential-test orchestration.

#![allow(clippy::print_stdout, clippy::print_stderr)]

fn main() {
    println!("xtask: no tasks registered yet");
}
