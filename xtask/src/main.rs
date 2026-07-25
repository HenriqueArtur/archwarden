//! Repository automation, run as `cargo xtask <task>`.
//!
//! Holds jobs that need to be reproducible across contributors and CI without
//! adding a shell script per platform.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::{path::PathBuf, process::ExitCode};

/// Where the published schema lives, relative to the repository root.
const SCHEMA_PATH: &str = "schema/v0.json";

fn main() -> ExitCode {
    let task = std::env::args().nth(1);

    match task.as_deref() {
        Some("gen-schema") => run(gen_schema(Mode::Write)),
        Some("check-schema") => run(gen_schema(Mode::Check)),
        other => {
            if let Some(unknown) = other {
                eprintln!("unknown task `{unknown}`");
            }
            eprintln!("usage: cargo xtask <gen-schema|check-schema>");
            eprintln!();
            eprintln!("  gen-schema    write {SCHEMA_PATH} from the config types");
            eprintln!("  check-schema  fail if {SCHEMA_PATH} is out of date");
            ExitCode::FAILURE
        }
    }
}

fn run(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Write,
    Check,
}

/// Generates the JSON Schema editors pick up through `$schema`.
///
/// `Check` exists so CI can fail when the committed schema drifts from the
/// types. Without it, a field added to a rule would silently stop appearing in
/// editor completion, and nobody would notice until a user filed a bug.
fn gen_schema(mode: Mode) -> Result<(), String> {
    let schema = schemars::schema_for!(archwarden_config::config::Config);
    let mut rendered = serde_json::to_string_pretty(&schema)
        .map_err(|error| format!("cannot render the schema: {error}"))?;
    rendered.push('\n');

    let path = repository_root().join(SCHEMA_PATH);

    match mode {
        Mode::Write => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
            }
            std::fs::write(&path, &rendered)
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            println!("wrote {}", path.display());
            Ok(())
        }

        Mode::Check => {
            let committed = std::fs::read_to_string(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;

            if committed == rendered {
                println!("{SCHEMA_PATH} is up to date");
                Ok(())
            } else {
                Err(format!(
                    "{SCHEMA_PATH} is out of date; run `cargo xtask gen-schema`"
                ))
            }
        }
    }
}

/// The workspace root.
///
/// Derived from this crate's manifest directory rather than from the working
/// directory, so the task behaves the same wherever it is invoked from.
fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}
