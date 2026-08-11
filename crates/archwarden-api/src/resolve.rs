//! Resolve and Load: from "where do I look" to a compiled configuration.
//!
//! Two stages, separately callable. **Resolve** answers *which* configuration
//! this invocation means — discovery or an explicit path, the version guard,
//! and the `extends` chain folded flat. **Load** turns that into rules: every
//! glob built, every pattern compiled, every export template checked against
//! the capture groups its pattern defines.
//!
//! They are separate because not every caller wants both. `config validate`
//! reports how many rules a config declares and which presets contributed,
//! and none of that needs them compiled.

use archwarden_config::{compile, config::SCHEMA_VERSION, discovery, extends};
use archwarden_core::compiled::CompiledConfig;
use archwarden_resolver::preset::PresetResolver;
use camino::Utf8Path;

use crate::Error;

/// Where an invocation reads its rules from, and what it reads them against.
///
/// Two questions `--config` used to answer at once. Separating them is what
/// lets a configuration live outside the repository it describes.
#[derive(Debug, Clone, Copy)]
pub struct Location<'a> {
    /// An explicit config path, or `None` to search upwards from the working
    /// directory.
    pub config: Option<&'a Utf8Path>,
    /// An explicit repository root, or `None` to take the config's answer.
    pub root: Option<&'a Utf8Path>,
}

/// A configuration, resolved and compiled.
///
/// Named rather than a tuple because it crosses a crate boundary: a caller
/// unpacking `(a, b)` has to remember which is which, and MCP will name these
/// in a schema.
#[derive(Debug)]
pub struct Prepared {
    /// The configuration as declared, with its presets folded in.
    pub merged: extends::MergedConfig,
    /// The same configuration, lowered into matchers.
    pub compiled: CompiledConfig,
}

/// Resolve: finds the configuration this invocation means, and folds it flat.
///
/// The version guard sits between loading and merging, and the order is not
/// arbitrary. A version this build cannot interpret means it cannot be trusted
/// to read the file at all — presets included — so refusing before resolving
/// them is both cheaper and the only way the user hears about the real problem
/// first.
///
/// # Errors
/// [`Error::Load`], [`Error::UnsupportedVersion`], [`Error::Extends`].
pub fn resolve(
    location: Location<'_>,
    working_directory: &Utf8Path,
) -> Result<extends::MergedConfig, Error> {
    let loaded = locate(location, working_directory)?;

    if !loaded.config.version_is_supported() {
        return Err(Error::UnsupportedVersion {
            path: loaded.path,
            declared: loaded.config.version,
            understood: SCHEMA_VERSION,
        });
    }

    Ok(extends::merge(loaded, &PresetResolver::new())?)
}

/// Load: lowers a resolved configuration into matchers.
///
/// # Errors
/// [`Error::Compile`].
pub fn load(merged: &extends::MergedConfig) -> Result<CompiledConfig, Error> {
    Ok(compile::compile(merged)?)
}

/// Resolve then Load, which is what almost every caller wants.
///
/// # Errors
/// Any of [`Error`]'s configuration variants.
pub fn prepare(location: Location<'_>, working_directory: &Utf8Path) -> Result<Prepared, Error> {
    let merged = resolve(location, working_directory)?;
    let compiled = load(&merged)?;

    Ok(Prepared { merged, compiled })
}

/// Reads the config file, either from an explicit path or by searching upwards.
///
/// A relative path resolves against the `working_directory` the caller passed
/// rather than against the process's own, so nothing here depends on ambient
/// state: a test and a shell behave identically, and one process can serve two
/// repositories.
///
/// Both the config path and the root are joined unconditionally. Each used to
/// be guarded by an `is_absolute` test that decided nothing: `Path::join`
/// already replaces the base when what it is given is rooted, on every
/// platform. `cargo-mutants` replaced the first guard with `false` and no test
/// noticed — which is what a branch with no observable effect looks like, and
/// the reason both are gone rather than excluded from mutation testing.
///
/// The behaviour they appeared to protect is still pinned, by
/// `an_absolute_config_path_is_taken_as_it_stands` and
/// `an_absolute_root_is_taken_as_it_stands`. Those tests are why deleting them
/// is safe rather than merely tidy.
fn locate(
    location: Location<'_>,
    working_directory: &Utf8Path,
) -> Result<discovery::LoadedConfig, discovery::LoadError> {
    let mut loaded = match location.config {
        Some(path) => discovery::load_file(&working_directory.join(path)),
        None => discovery::load_from(working_directory),
    }?;

    // Last, so it beats both the config's own `root` and the default. Someone
    // who passed `--root` is answering the question those two guess at.
    if let Some(root) = location.root {
        loaded.root = working_directory.join(root);
    }

    Ok(loaded)
}

#[cfg(test)]
mod tests {
    use crate::{Error, Location, prepare};
    use camino::{Utf8Path, Utf8PathBuf};

    /// Writes a repository and hands back its root, keeping the `TempDir`
    /// alive in the caller so the directory outlives the assertions.
    fn repository(files: &[(&str, &str)]) -> (tempfile::TempDir, Utf8PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();

        for (name, contents) in files {
            let path = root.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, contents).unwrap();
        }

        (directory, root)
    }

    /// Discovery, the version guard, `extends` and compilation, with nothing
    /// written anywhere: the whole point of the crate in one call.
    fn prepared_at(root: &Utf8Path) -> Result<crate::Prepared, Error> {
        prepare(
            Location {
                config: None,
                root: None,
            },
            root,
        )
    }

    const A_RULE: &str = r#"{"version":0,"rules":[
        {"type":"naming","id":"usecase-name","level":"error","roots":"src/*",
         "file_pattern":"^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
         "must_export":{"name":"{{pascal(name)}}","kind":"function"}}]}"#;

    #[test]
    fn a_configuration_that_is_good_arrives_compiled() {
        let (_directory, root) = repository(&[("arch.config.json", A_RULE)]);

        let prepared = prepared_at(&root).unwrap();

        assert_eq!(prepared.merged.root, root);
        assert_eq!(prepared.compiled.rules().count(), 1);
    }

    /// Issue #55's shape, as a value. The version this build cannot interpret
    /// used to be a sentence written to stderr from inside the orchestration,
    /// which is exactly why the hook could not reuse the path and re-wrote it
    /// without this guard: a future config parsed into a config with no rules,
    /// compiled, matched nothing, and permitted every write.
    ///
    /// Now it is returned. A surface that answers in JSON renders it as JSON.
    #[test]
    fn an_unsupported_version_is_returned_rather_than_written() {
        let (_directory, root) =
            repository(&[("arch.config.json", r#"{"version": 99, "rules": []}"#)]);

        let error = prepared_at(&root).err();

        assert!(matches!(
            error,
            Some(Error::UnsupportedVersion {
                declared: 99,
                understood: 0,
                ..
            })
        ));
    }

    /// The sentence a user reads, pinned. Asserting on the whole message
    /// rather than on the variant is what keeps a copy edit from silently
    /// changing what three surfaces say.
    #[test]
    fn the_unsupported_version_message_names_both_numbers_and_the_file() {
        let (_directory, root) =
            repository(&[("arch.config.json", r#"{"version": 99, "rules": []}"#)]);

        assert_eq!(
            prepared_at(&root).err().map(|error| error.to_string()),
            Some(format!(
                "`{}` declares version 99, but this build understands version 0",
                root.join("arch.config.json")
            ))
        );
    }

    /// The order matters and nothing else pins it: a config from a future
    /// version must be refused *before* its `extends` chain is resolved.
    /// Otherwise the first thing a user hears about a config this build cannot
    /// read is that one of its presets is missing, which sends them to fix the
    /// wrong file.
    #[test]
    fn the_version_is_checked_before_presets_are_resolved() {
        let (_directory, root) = repository(&[(
            "arch.config.json",
            r#"{"version": 99, "extends": ["./nowhere.json"], "rules": []}"#,
        )]);

        assert!(matches!(
            prepared_at(&root).err(),
            Some(Error::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn no_configuration_anywhere_is_a_load_error() {
        let (_directory, root) = repository(&[]);

        assert!(matches!(prepared_at(&root).err(), Some(Error::Load(_))));
    }

    #[test]
    fn a_preset_that_does_not_resolve_is_an_extends_error() {
        let (_directory, root) = repository(&[(
            "arch.config.json",
            r#"{"version": 0, "extends": ["./nowhere.json"], "rules": []}"#,
        )]);

        assert!(matches!(prepared_at(&root).err(), Some(Error::Extends(_))));
    }

    /// Compilation is what makes "the JSON parsed" mean something: every glob
    /// is built and every pattern compiled. A pattern the linear-time engine
    /// refuses fails here rather than at the first file it is asked about.
    #[test]
    fn a_pattern_the_engine_refuses_is_a_compile_error() {
        let (_directory, root) = repository(&[(
            "arch.config.json",
            r#"{"version":0,"rules":[
                {"type":"naming","id":"lookahead","level":"error","roots":"src/*",
                 "file_pattern":"^(?!test)(?<name>[a-z]+)\\.ts$",
                 "must_export":{"name":"{{pascal(name)}}","kind":"function"}}]}"#,
        )]);

        assert!(matches!(prepared_at(&root).err(), Some(Error::Compile(_))));
    }

    /// A relative `--config` resolves against the working directory the caller
    /// passed, never against the process's own. Nothing here reads ambient
    /// state, which is what lets a test and a shell behave identically — and
    /// what lets an MCP server serve two repositories from one process.
    #[test]
    fn a_relative_config_path_resolves_against_the_working_directory() {
        let (_directory, root) = repository(&[("config/strict.json", A_RULE)]);

        let prepared = prepare(
            Location {
                config: Some(Utf8Path::new("config/strict.json")),
                root: None,
            },
            &root,
        )
        .unwrap();

        assert_eq!(prepared.merged.path, root.join("config/strict.json"));
    }

    /// `--root` beats both the config's own `root` and the default, because
    /// someone who passed it is answering the question those two guess at.
    #[test]
    fn an_explicit_root_beats_the_one_the_config_chose() {
        let (_directory, root) = repository(&[(
            "arch.config.json",
            r#"{"version": 0, "root": "src", "rules": []}"#,
        )]);
        let elsewhere = root.join("packages/web");
        std::fs::create_dir_all(&elsewhere).unwrap();

        let prepared = prepare(
            Location {
                config: None,
                root: Some(Utf8Path::new("packages/web")),
            },
            &root,
        )
        .unwrap();

        assert_eq!(prepared.merged.root, elsewhere);
    }

    /// An absolute `--root` is taken as given rather than joined onto the
    /// working directory, which would produce a path that exists nowhere.
    #[test]
    fn an_absolute_root_is_taken_as_it_stands() {
        let (_directory, root) = repository(&[("arch.config.json", A_RULE)]);
        let (_other, elsewhere) = repository(&[]);

        let prepared = prepare(
            Location {
                config: None,
                root: Some(&elsewhere),
            },
            &root,
        )
        .unwrap();

        assert_eq!(prepared.merged.root, elsewhere);
    }

    /// An absolute `--config` likewise, which is the shape `--config
    /// /tmp/stricter.json` takes when asking what a rule would find.
    #[test]
    fn an_absolute_config_path_is_taken_as_it_stands() {
        let (_directory, root) = repository(&[("arch.config.json", A_RULE)]);
        let (_other, elsewhere) = repository(&[("stricter.json", A_RULE)]);
        let config = elsewhere.join("stricter.json");

        let prepared = prepare(
            Location {
                config: Some(&config),
                root: None,
            },
            &root,
        )
        .unwrap();

        assert_eq!(prepared.merged.path, config);
    }

    /// The two stages are separately callable, which is the property a surface
    /// that only wants the merged config depends on — `config validate` says
    /// how many rules are active without ever needing them compiled.
    #[test]
    fn resolve_answers_without_compiling_and_load_finishes_the_job() {
        let (_directory, root) = repository(&[("arch.config.json", A_RULE)]);

        let merged = crate::resolve(
            Location {
                config: None,
                root: None,
            },
            &root,
        )
        .unwrap();
        assert_eq!(merged.config.rules().count(), 1);

        let compiled = crate::load(&merged).unwrap();
        assert_eq!(compiled.rules().count(), 1);
    }
}
