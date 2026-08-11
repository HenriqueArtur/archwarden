//! Resolving `extends` entries to files on disk.
//!
//! A `./`-prefixed entry is a path and needs nothing clever. Anything else is
//! an npm package name, and turning one of those into a file path is full Node
//! module resolution: walking up through `node_modules`, honouring `exports`
//! conditions, following pnpm's symlink layout, reading yarn `PnP`'s manifest.
//!
//! `oxc_resolver` already does all of that, and archwarden depends on it for
//! import resolution anyway (decision 7). Hand-rolling a second, worse copy
//! here would buy nothing.

use camino::{Utf8Path, Utf8PathBuf};

/// The `package.json` field a package may use to point at its preset.
///
/// Checked before `main`, so a package can ship a JavaScript entry point and
/// an archwarden preset without the two fighting over one field.
const PRESET_FIELD: &str = "archwarden";

/// Why a preset could not be resolved.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PresetError {
    /// The specifier did not resolve to anything.
    #[error("cannot resolve preset `{specifier}` from `{from}`")]
    Unresolved {
        /// The `extends` entry, as written.
        specifier: String,
        /// The directory resolution started from.
        from: Utf8PathBuf,
        /// What the resolver said. Boxed because `ResolveError` is large
        /// enough that carrying it inline would bloat every `Result` this
        /// module returns, including the successful ones.
        #[source]
        source: Box<oxc_resolver::ResolveError>,
    },

    /// It resolved, but not to a JSON file.
    #[error("preset `{specifier}` resolved to `{path}`, which is not a JSON file")]
    NotJson {
        /// The `extends` entry, as written.
        specifier: String,
        /// What it resolved to.
        path: Utf8PathBuf,
    },

    /// It resolved, but to a path that is not valid UTF-8.
    #[error("preset `{specifier}` resolved to a path that is not valid UTF-8")]
    NonUtf8Path {
        /// The `extends` entry, as written.
        specifier: String,
    },
}

/// Resolves `extends` entries.
#[derive(Debug)]
pub struct PresetResolver {
    inner: oxc_resolver::Resolver,
}

impl PresetResolver {
    /// Builds a resolver configured for preset lookup.
    #[must_use]
    pub fn new() -> Self {
        let options = oxc_resolver::ResolveOptions {
            // Only tried for specifiers written without an extension. A
            // `main` that already says `.js` bypasses this entirely, which is
            // why `resolve` checks the result rather than trusting the list.
            extensions: vec![".json".to_owned()],
            main_fields: vec![PRESET_FIELD.to_owned(), "main".to_owned()],
            main_files: vec!["index".to_owned()],
            condition_names: vec![PRESET_FIELD.to_owned(), "default".to_owned()],
            ..oxc_resolver::ResolveOptions::default()
        };

        Self {
            inner: oxc_resolver::Resolver::new(options),
        }
    }

    /// Resolves one `extends` entry, relative to the directory holding the
    /// config that declared it.
    ///
    /// # Errors
    /// [`PresetError`] when the specifier resolves to nothing, or to a path
    /// archwarden cannot represent.
    pub fn resolve(
        &self,
        from_directory: &Utf8Path,
        specifier: &str,
    ) -> Result<Utf8PathBuf, PresetError> {
        let resolution = self
            .inner
            .resolve(from_directory.as_std_path(), specifier)
            .map_err(|source| PresetError::Unresolved {
                specifier: specifier.to_owned(),
                from: from_directory.to_owned(),
                source: Box::new(source),
            })?;

        let path = Utf8PathBuf::from_path_buf(resolution.full_path()).map_err(|_| {
            PresetError::NonUtf8Path {
                specifier: specifier.to_owned(),
            }
        })?;

        // Config is data, not code (decision 5). A package whose entry point
        // is JavaScript must be refused here: the `extensions` list above only
        // governs specifiers written without one, so a `main` of `preset.js`
        // resolves happily and would otherwise be read as a config.
        if path.extension() != Some("json") {
            return Err(PresetError::NotJson {
                specifier: specifier.to_owned(),
                path,
            });
        }

        Ok(path)
    }
}

impl Default for PresetResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a temporary tree and returns its canonical UTF-8 root.
    fn tree(entries: &[(&str, &str)]) -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");

        for (relative, contents) in entries {
            let path = root.join(relative);
            // Every entry is `directory/file`, so the parent is never absent.
            // Written as an `expect` rather than an `if let` because the
            // negative arm no execution reaches is dead code that drags the
            // coverage floor down -- see the convention in CONTRIBUTING.md.
            std::fs::create_dir_all(path.parent().expect("a file has a parent"))
                .expect("create dirs");
            std::fs::write(&path, contents).expect("write file");
        }

        (dir, root)
    }

    const PRESET: &str = r#"{"version": 0}"#;

    /// The simple half: a relative path is just a path.
    #[test]
    fn a_relative_path_resolves_to_that_file() {
        let (_guard, root) = tree(&[("presets/base.json", PRESET)]);

        let resolved = PresetResolver::new()
            .resolve(&root, "./presets/base.json")
            .expect("resolves");

        assert_eq!(resolved, root.join("presets/base.json"));
    }

    /// The npm case, in the layout npm and yarn classic produce.
    #[test]
    fn a_package_name_resolves_through_node_modules() {
        let (_guard, root) = tree(&[
            (
                "node_modules/@myorg/arch-preset/package.json",
                r#"{"name": "@myorg/arch-preset", "main": "preset.json"}"#,
            ),
            ("node_modules/@myorg/arch-preset/preset.json", PRESET),
        ]);

        let resolved = PresetResolver::new()
            .resolve(&root, "@myorg/arch-preset")
            .expect("resolves");

        assert_eq!(
            resolved,
            root.join("node_modules/@myorg/arch-preset/preset.json")
        );
    }

    /// `node_modules` is searched upwards, which is how a package installed at
    /// the repository root is visible from inside a workspace member.
    #[test]
    fn node_modules_is_searched_upwards_from_a_subpackage() {
        let (_guard, root) = tree(&[
            (
                "node_modules/preset-pkg/package.json",
                r#"{"name": "preset-pkg", "main": "preset.json"}"#,
            ),
            ("node_modules/preset-pkg/preset.json", PRESET),
            ("packages/app/arch.config.json", PRESET),
        ]);

        let resolved = PresetResolver::new()
            .resolve(&root.join("packages/app"), "preset-pkg")
            .expect("resolves");

        assert_eq!(resolved, root.join("node_modules/preset-pkg/preset.json"));
    }

    /// A dedicated `archwarden` field wins over `main`, so a package can ship
    /// a JavaScript entry point and a preset without the two colliding.
    #[test]
    fn a_dedicated_archwarden_field_takes_precedence_over_main() {
        let (_guard, root) = tree(&[
            (
                "node_modules/dual/package.json",
                r#"{"name":"dual","main":"index.json","archwarden":"arch.json"}"#,
            ),
            ("node_modules/dual/index.json", r#"{"not":"a preset"}"#),
            ("node_modules/dual/arch.json", PRESET),
        ]);

        let resolved = PresetResolver::new()
            .resolve(&root, "dual")
            .expect("resolves");

        assert_eq!(resolved, root.join("node_modules/dual/arch.json"));
    }

    /// Modern packages use `exports` rather than `main`, and a subpath entry is
    /// a normal way to publish more than one preset from one package.
    #[test]
    fn an_exports_subpath_resolves() {
        let (_guard, root) = tree(&[
            (
                "node_modules/multi/package.json",
                r#"{"name":"multi","exports":{"./strict":"./presets/strict.json"}}"#,
            ),
            ("node_modules/multi/presets/strict.json", PRESET),
        ]);

        let resolved = PresetResolver::new()
            .resolve(&root, "multi/strict")
            .expect("resolves");

        assert_eq!(
            resolved,
            root.join("node_modules/multi/presets/strict.json")
        );
    }

    /// pnpm installs into a content-addressed store and links into
    /// `node_modules`. Resolution has to follow the link, which is the main
    /// reason this is not hand-rolled.
    #[cfg(unix)]
    #[test]
    fn a_pnpm_style_symlink_is_followed() {
        let (_guard, root) = tree(&[
            (
                ".pnpm/preset-pkg@1.0.0/node_modules/preset-pkg/package.json",
                r#"{"name":"preset-pkg","main":"preset.json"}"#,
            ),
            (
                ".pnpm/preset-pkg@1.0.0/node_modules/preset-pkg/preset.json",
                PRESET,
            ),
        ]);
        std::fs::create_dir_all(root.join("node_modules")).expect("create node_modules");
        std::os::unix::fs::symlink(
            root.join(".pnpm/preset-pkg@1.0.0/node_modules/preset-pkg"),
            root.join("node_modules/preset-pkg"),
        )
        .expect("symlink");

        let resolved = PresetResolver::new()
            .resolve(&root, "preset-pkg")
            .expect("resolves through the symlink");

        assert!(resolved.as_str().ends_with("preset.json"), "{resolved}");
        assert!(resolved.is_file(), "{resolved} should exist");
    }

    /// The error names both the specifier and where the search started, which
    /// together are what a user needs to work out whether they forgot to
    /// install the package or pointed at the wrong directory.
    #[test]
    fn an_unresolvable_preset_names_the_specifier_and_the_directory() {
        let (_guard, root) = tree(&[("arch.config.json", PRESET)]);

        let err = PresetResolver::new()
            .resolve(&root, "@myorg/never-installed")
            .expect_err("nothing to resolve");

        assert_eq!(
            err.to_string(),
            format!("cannot resolve preset `@myorg/never-installed` from `{root}`")
        );
    }

    /// A relative path that does not exist fails the same way a missing
    /// package does, rather than being reported later as a read error.
    #[test]
    fn a_missing_relative_preset_fails_at_resolution() {
        let (_guard, root) = tree(&[("arch.config.json", PRESET)]);

        let err = PresetResolver::new()
            .resolve(&root, "./nope.json")
            .expect_err("nothing to resolve");

        assert_eq!(
            err.to_string(),
            format!("cannot resolve preset `./nope.json` from `{root}`")
        );
    }

    /// `Default` and `new` are the same resolver. Worth pinning because they
    /// are two doors into one configuration, and a divergence would show up as
    /// presets resolving differently depending on which door a caller used.
    #[test]
    fn the_default_resolver_is_the_configured_one() {
        let (_guard, root) = tree(&[
            (
                "node_modules/preset-pkg/package.json",
                r#"{"name": "preset-pkg", "main": "preset.json"}"#,
            ),
            ("node_modules/preset-pkg/preset.json", PRESET),
        ]);

        assert_eq!(
            PresetResolver::default()
                .resolve(&root, "preset-pkg")
                .expect("the default resolver finds it"),
            root.join("node_modules/preset-pkg/preset.json")
        );
    }

    /// Only JSON. A preset that resolved to JavaScript would make the config
    /// executable, which decision 5 exists to prevent.
    #[test]
    fn a_javascript_entry_point_does_not_resolve() {
        let (_guard, root) = tree(&[
            (
                "node_modules/js-preset/package.json",
                r#"{"name":"js-preset","main":"preset.js"}"#,
            ),
            ("node_modules/js-preset/preset.js", "module.exports = {};"),
        ]);

        let err = PresetResolver::new()
            .resolve(&root, "js-preset")
            .expect_err("a JS entry point must not be accepted as a preset");

        assert_eq!(
            err.to_string(),
            format!(
                "preset `js-preset` resolved to `{}`, which is not a JSON file",
                root.join("node_modules/js-preset/preset.js")
            )
        );
    }
}
