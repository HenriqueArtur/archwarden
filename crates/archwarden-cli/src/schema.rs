//! Which schema an `arch.config.json` should point at.
//!
//! The `$schema` field is the one line in a starter config that earns its
//! place: archwarden ignores it, but an editor reads it and gives completion
//! and an error on a misspelled key before archwarden ever runs.
//!
//! # Two answers, and why
//!
//! Where archwarden is installed from npm, the schema for *the version that
//! is installed* is already on disk, under `node_modules/archwarden/`. A
//! relative reference to it is better than a URL in every way that matters: it
//! works with no network, it cannot serve a schema for a different version
//! than the binary being run, and it moves with the lockfile.
//!
//! Everywhere else the reference has to be a URL, and it points at the raw
//! file in the repository rather than at `archwarden.dev`, which does not
//! resolve. A `$schema` that 404s is worse than none: the editor gives no
//! completion and no reason why.

use camino::Utf8Path;

/// The schema, as published.
///
/// Deliberately the raw file rather than a vanity domain. The domain can come
/// later; a promise in a generated config file cannot wait for it.
pub const SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/HenriqueArtur/archwarden/main/schema/v0.json";

/// Where the schema sits inside the installed package.
const IN_PACKAGE: &str = "node_modules/archwarden/schema/v0.json";

/// What to write as `$schema` for a config at `root`.
///
/// A `./`-prefixed relative reference when archwarden is installed here, and
/// the published URL otherwise. Editors resolve a relative `$schema` against
/// the file that declares it, which is why the answer is relative to `root`
/// rather than absolute: an absolute path would name this machine, and
/// `arch.config.json` is committed.
#[must_use]
pub fn reference(root: &Utf8Path) -> String {
    if root.join(IN_PACKAGE).is_file() {
        return format!("./{IN_PACKAGE}");
    }
    SCHEMA_URL.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory with the package's schema in place, as an install leaves it.
    fn installed() -> (tempfile::TempDir, camino::Utf8PathBuf) {
        let (guard, root) = empty();
        let directory = root.join("node_modules/archwarden/schema");
        std::fs::create_dir_all(&directory).expect("create dirs");
        std::fs::write(directory.join("v0.json"), "{}").expect("write");
        (guard, root)
    }

    fn empty() -> (tempfile::TempDir, camino::Utf8PathBuf) {
        let guard = tempfile::tempdir().expect("temp dir");
        let root = camino::Utf8PathBuf::from_path_buf(guard.path().to_path_buf())
            .expect("temp path is UTF-8");
        (guard, root)
    }

    /// The point of shipping the schema: the installed version answers for
    /// itself, with no network and no chance of describing a different build.
    #[test]
    fn an_installed_package_is_referenced_by_path() {
        let (_guard, root) = installed();

        assert_eq!(reference(&root), "./node_modules/archwarden/schema/v0.json");
    }

    /// A relative reference, not an absolute one. `arch.config.json` is
    /// committed, and an absolute path names this machine.
    #[test]
    fn the_reference_is_relative() {
        let (_guard, root) = installed();
        let reference = reference(&root);

        assert!(reference.starts_with("./"), "{reference}");
        assert!(
            !reference.contains(root.as_str()),
            "no absolute path: {reference}"
        );
    }

    /// Without an install there is nothing on disk to point at.
    #[test]
    fn without_an_install_the_published_url_is_used() {
        let (_guard, root) = empty();

        assert_eq!(reference(&root), SCHEMA_URL);
    }

    /// A `node_modules/archwarden` without the schema in it is an older
    /// version, published before the schema shipped. Pointing at a file that
    /// is not there would give the editor nothing and say nothing about why.
    #[test]
    fn a_package_without_the_schema_falls_back_to_the_url() {
        let (_guard, root) = empty();
        std::fs::create_dir_all(root.join("node_modules/archwarden")).expect("create dirs");

        assert_eq!(reference(&root), SCHEMA_URL);
    }

    /// The URL has to be one that answers. `archwarden.dev` did not resolve,
    /// which is the defect this replaces -- so this pins the host rather than
    /// just the shape.
    #[test]
    fn the_published_url_points_at_the_file_in_the_repository() {
        assert!(
            SCHEMA_URL.starts_with("https://raw.githubusercontent.com/HenriqueArtur/archwarden/"),
            "{SCHEMA_URL}"
        );
        assert!(SCHEMA_URL.ends_with("/schema/v0.json"), "{SCHEMA_URL}");
    }
}
