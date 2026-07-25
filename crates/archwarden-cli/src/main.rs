//! `archwarden` — command-line entry point.
//!
//! Deliberately thin. Everything that decides anything lives in the library
//! half of this crate, where it can be tested without spawning a process, and
//! where it is reachable by the coverage floor.

use std::process::ExitCode;

use archwarden_cli::{Cli, Output, exit::Exit};
use camino::Utf8PathBuf;
use clap::Parser;

fn main() -> ExitCode {
    let cli = Cli::parse();

    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();

    let Some(working_directory) = current_directory(&mut stderr) else {
        return Exit::ConfigProblem.into();
    };

    archwarden_cli::run(
        &cli,
        &working_directory,
        &mut Output {
            out: &mut stdout,
            err: &mut stderr,
        },
    )
    .into()
}

/// The working directory, as UTF-8.
///
/// archwarden works in UTF-8 paths throughout, so a directory that is not
/// valid UTF-8 is refused here rather than lossily converted into something
/// that would silently fail to match any glob.
fn current_directory(errors: &mut impl std::io::Write) -> Option<Utf8PathBuf> {
    let raw = match std::env::current_dir() {
        Ok(raw) => raw,
        Err(error) => {
            let _ = writeln!(errors, "cannot determine the working directory: {error}");
            return None;
        }
    };

    match Utf8PathBuf::from_path_buf(raw) {
        Ok(path) => Some(path),
        Err(raw) => {
            let _ = writeln!(
                errors,
                "the working directory is not valid UTF-8: {}",
                raw.display()
            );
            None
        }
    }
}
