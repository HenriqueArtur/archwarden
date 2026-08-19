//! `cargo xtask linker` — the fast linker that is already installed.
//!
//! Measured on this workspace, relinking the `archwarden` binary after touching
//! one file: 6.47s with the default GNU linker, 1.02s with `rust-lld`. Four to
//! six times, on the step every `cargo build` and every `cargo nextest run`
//! ends with — and `nextest` links one test binary per crate.
//!
//! # Why a task and not a committed `.cargo/config.toml`
//!
//! Nothing has to be installed: `rust-lld` ships inside the toolchain
//! `rust-toolchain.toml` already pins. But the flag that reaches it on stable
//! needs an absolute path carrying both the sysroot and the host triple, and
//! neither is the same on two machines. It cannot be written into a committed
//! file.
//!
//! `-C linker-features=+lld` would be the portable spelling and is unstable on
//! 1.96, so it is out for a project that pins stable.
//!
//! The committable alternative — `-fuse-ld=lld` against a system lld — is
//! refused for two reasons. Every CI job would need `apt-get install lld`, and
//! issue #121 is what that costs here. And the gain is on *incremental*
//! relinks: CI builds cold, where compiling dominates and linking is a small
//! fraction. Committing it charges CI and returns nothing to it.

use std::path::{Path, PathBuf};

/// What to do with the flags once they are worked out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Write them to the file, after saying so.
    Write,
    /// Print them and change nothing.
    Print,
}

impl Mode {
    /// Reads the mode from the arguments after the task name.
    ///
    /// `--print` is the default, deliberately. The file this writes is the
    /// user's own global cargo config, outside the repository and shared by
    /// every project on the machine; a task that edits it because somebody
    /// typed the wrong word is worse than one that asks for a flag.
    pub(crate) fn parse(args: &[String]) -> Result<Self, String> {
        match args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            [] | ["--print"] => Ok(Self::Print),
            ["--write"] => Ok(Self::Write),
            other => Err(format!(
                "unknown argument `{}`; expected --print or --write",
                other.join(" ")
            )),
        }
    }
}

/// The host triple, read from `rustc -vV`.
///
/// Parsed rather than assumed: the triple names the directory the bundled
/// linker sits in, and guessing it wrong produces a `-B` pointing at nothing,
/// which fails at link time with a message about the linker rather than about
/// this task.
fn host_triple(banner: &str) -> Result<&str, String> {
    banner
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::trim)
        .filter(|triple| !triple.is_empty())
        .ok_or_else(|| "`rustc -vV` printed no `host:` line".to_owned())
}

/// Where the toolchain keeps the linkers it ships with.
fn bundled_linker_dir(sysroot: &Path, triple: &str) -> PathBuf {
    sysroot
        .join("lib")
        .join("rustlib")
        .join(triple)
        .join("bin")
        .join("gcc-ld")
}

/// The block to put in `~/.cargo/config.toml`.
///
/// Scoped to the host target rather than set globally: `[build] rustflags`
/// would apply when cross-compiling too, where this path is wrong.
fn snippet(linker_dir: &Path, triple: &str) -> String {
    format!(
        "[target.{triple}]\n\
         rustflags = [\n    \"-C\", \"link-arg=-B{}\",\n    \"-C\", \"link-arg=-fuse-ld=lld\",\n]\n",
        linker_dir.display()
    )
}

/// Whether the user's config already carries a block for this target.
///
/// Matched on the section header alone. Cargo takes the first `[target.<t>]`
/// it finds and ignores a second, so appending one to a file that has one
/// would report success and change nothing -- the failure this exists to
/// refuse.
fn already_configured(existing: &str, triple: &str) -> bool {
    let header = format!("[target.{triple}]");
    existing.lines().any(|line| line.trim() == header)
}

/// Works out the flags for this machine, and prints or writes them.
///
/// # Errors
/// When `rustc` cannot be run or its output cannot be read, when the toolchain
/// carries no bundled linker for the host, or when the config already has a
/// section for this target.
pub(crate) fn run(mode: Mode) -> Result<(), String> {
    let banner = rustc(&["-vV"])?;
    let triple = host_triple(&banner)?;
    let sysroot = PathBuf::from(rustc(&["--print", "sysroot"])?.trim());

    let linker_dir = bundled_linker_dir(&sysroot, triple);
    if !linker_dir.join("ld.lld").exists() {
        return Err(format!(
            "this toolchain carries no bundled linker at {}",
            linker_dir.display()
        ));
    }

    let block = snippet(&linker_dir, triple);
    match mode {
        Mode::Print => {
            println!("{block}");
            println!("# add that to ~/.cargo/config.toml, or run `cargo xtask linker --write`");
            Ok(())
        }
        Mode::Write => write_block(&block, triple),
    }
}

/// Runs `rustc` and returns what it said.
fn rustc(args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("rustc")
        .args(args)
        .output()
        .map_err(|error| format!("could not run `rustc {}`: {error}", args.join(" ")))?;

    String::from_utf8(output.stdout).map_err(|_| {
        format!(
            "`rustc {}` printed something that is not UTF-8",
            args.join(" ")
        )
    })
}

/// Appends the block to the user's global cargo config.
fn write_block(block: &str, triple: &str) -> Result<(), String> {
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
        .ok_or_else(|| "neither CARGO_HOME nor HOME is set".to_owned())?;

    let path = home.join("config.toml");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    if already_configured(&existing, triple) {
        return Err(format!(
            "{} already has a [target.{triple}] section; cargo takes the first, \n       so appending a second would change nothing. Edit it by hand.",
            path.display()
        ));
    }

    std::fs::create_dir_all(&home)
        .map_err(|error| format!("could not create {}: {error}", home.display()))?;

    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    std::fs::write(&path, format!("{existing}{separator}\n{block}"))
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;

    println!("wrote the [target.{triple}] block to {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_triple_is_read_from_the_version_banner() {
        let banner = "rustc 1.96.0 (ac68faa20 2026-05-25)\n\
                      binary: rustc\n\
                      host: aarch64-unknown-linux-gnu\n\
                      release: 1.96.0\n";

        assert_eq!(
            host_triple(banner).expect("a host line"),
            "aarch64-unknown-linux-gnu"
        );
    }

    /// Refused rather than guessed. A wrong triple makes a `-B` that points at
    /// nothing, and the failure surfaces at link time as a message about the
    /// linker rather than about this task.
    #[test]
    fn a_banner_with_no_host_line_is_refused() {
        assert!(host_triple("rustc 1.96.0\nbinary: rustc\n").is_err());
        assert!(
            host_triple("host: \n").is_err(),
            "and an empty one is not a triple"
        );
    }

    #[test]
    fn the_bundled_linker_sits_under_the_triple_it_is_for() {
        let dir = bundled_linker_dir(Path::new("/sysroot"), "aarch64-unknown-linux-gnu");

        assert_eq!(
            dir,
            Path::new("/sysroot/lib/rustlib/aarch64-unknown-linux-gnu/bin/gcc-ld"),
        );
    }

    /// The snippet is scoped to one target, carries the absolute path, and asks
    /// for `lld` by name. All three matter: a `[build]` section would apply
    /// when cross-compiling, a relative path would depend on the working
    /// directory, and without `-fuse-ld` the `-B` is just a search path nobody
    /// looks in.
    #[test]
    fn the_snippet_names_the_target_the_path_and_the_linker() {
        let text = snippet(Path::new("/sysroot/gcc-ld"), "x86_64-unknown-linux-gnu");

        assert!(
            text.starts_with("[target.x86_64-unknown-linux-gnu]"),
            "{text}"
        );
        assert!(text.contains("link-arg=-B/sysroot/gcc-ld"), "{text}");
        assert!(text.contains("link-arg=-fuse-ld=lld"), "{text}");
        assert!(!text.contains("[build]"), "never global: {text}");
    }

    /// Print is the default. The file this would write is the user's global
    /// cargo config, shared by every project on the machine.
    #[test]
    fn printing_is_the_default_and_writing_is_asked_for() {
        assert_eq!(Mode::parse(&[]), Ok(Mode::Print));
        assert_eq!(Mode::parse(&["--print".to_owned()]), Ok(Mode::Print));
        assert_eq!(Mode::parse(&["--write".to_owned()]), Ok(Mode::Write));
    }

    /// Appending a second `[target.<triple>]` section would give cargo two,
    /// and cargo takes the first -- so a second run would look like it worked
    /// and change nothing. Refusing is the honest answer.
    #[test]
    fn a_target_already_configured_is_recognised() {
        let existing = "[target.aarch64-unknown-linux-gnu]\nrustflags = [\"-C\", \"x\"]\n";

        assert!(already_configured(existing, "aarch64-unknown-linux-gnu"));
        assert!(
            !already_configured(existing, "x86_64-unknown-linux-gnu"),
            "another target is not this one"
        );
        assert!(
            !already_configured("", "aarch64-unknown-linux-gnu"),
            "an empty file has none"
        );
    }

    #[test]
    fn an_unknown_flag_is_refused_rather_than_treated_as_the_default() {
        let refusal = Mode::parse(&["--fast".to_owned()]).expect_err("not a flag");

        assert!(
            refusal.contains("--fast"),
            "it names what was typed: {refusal}"
        );
        assert!(refusal.contains("--write"), "and what was meant: {refusal}");
    }
}
