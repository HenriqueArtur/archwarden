//! Workspace packages, resolved without `node_modules`.
//!
//! A monorepo imports itself by package name: `@flowmaatik/domain/email/x`,
//! not `../../domain/src/email/x`. Node answers that specifier by looking in
//! `node_modules`, where the package manager has left a symlink — so on a
//! machine that has run `pnpm install`, `oxc_resolver` needs nothing from this
//! module.
//!
//! On one that has not, half the import graph disappears. Measured on a real
//! pnpm monorepo with no install: 5481 imports by package name against 5690
//! relative ones. Every question archwarden answers from the graph —
//! `import-boundary` findings, `impact`'s list of importers — is then quietly
//! answering about the relative half only. Quietly is the problem: a boundary
//! rule reports nothing and reads as satisfied.
//!
//! So the map is built from what the repository itself declares: every
//! `package.json` with a `name`, and the `exports` that says which subpaths it
//! offers and where they land. That is the same information the package
//! manager used to build the symlink, read from the source rather than from
//! the artefact.
//!
//! # Why `fallback` and not `alias`
//!
//! These entries go into [`oxc_resolver::ResolveOptions::fallback`], which is
//! consulted only after normal resolution has failed. A repository that *has*
//! installed its dependencies resolves exactly as it did before this module
//! existed, and an installed package always wins over our reconstruction of
//! it. The map fills a hole; it does not overrule anything.
//!
//! # What this deliberately does not do
//!
//! It does not read `pnpm-workspace.yaml`, `workspaces` in the root
//! `package.json`, or any other manifest of *which* directories are members.
//! Every `package.json` in the walk is taken at its word. Two reasons: the
//! answer is the same in every layout anyone actually writes, and the
//! alternative is a YAML parser in a binary that has no other use for one.
//!
//! The cost is a package that exists on disk but is excluded from the
//! workspace. It would resolve here and not under Node — but only for a
//! specifier no file can be importing, since under Node that import does not
//! resolve at all and the repository would not build.

use camino::{Utf8Path, Utf8PathBuf};
use oxc_resolver::{Alias, AliasValue};

/// Directory names never descended into when looking for packages.
///
/// `node_modules` first and for the obvious reason: a package found in there
/// is the artefact this module exists to work without, and every one of its
/// own dependencies would be found under it too.
const SKIP: [&str; 7] = [
    "node_modules",
    ".git",
    "dist",
    "build",
    ".next",
    "coverage",
    "target",
];

/// How deep the search for `package.json` goes below the root.
///
/// `services/repository/firebase` is three, which is the deepest layout in the
/// repositories this was measured against. Five leaves room without turning a
/// mistaken root into a walk of somebody's whole home directory.
const MAX_DEPTH: usize = 5;

/// `exports` conditions, best first.
///
/// The same order and the same reasoning as `imports.rs`: a TypeScript file
/// imports a dependency's declarations, and the ESM entry point is the one a
/// bundler picks.
const CONDITIONS: [&str; 4] = ["types", "import", "require", "default"];

/// One local package, as its own `package.json` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The `name` field. A `package.json` without one is not a package.
    pub name: String,
    /// The directory the manifest sits in, relative to the repository root.
    pub directory: Utf8PathBuf,
    /// Subpath patterns from `exports`, as `(subpath, target)` pairs where
    /// both may contain a single `*`. Empty when the package declares no
    /// `exports`, which means every file in it is importable by path.
    pub subpaths: Vec<(String, String)>,
}

/// Every package the repository declares locally.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Workspace {
    packages: Vec<Package>,
}

impl Workspace {
    /// Finds every local package under `root`.
    ///
    /// Never fails: a manifest that will not parse is a manifest this cannot
    /// learn from, and refusing to resolve the other forty packages because of
    /// it would be the wrong trade. `check` is where a broken file is
    /// reported.
    #[must_use]
    pub fn discover(root: &Utf8Path) -> Self {
        let mut packages = Vec::new();
        collect(root, Utf8Path::new(""), 0, &mut packages);

        // Longest name first, so `@org/domain-testing` is matched before
        // `@org/domain` — otherwise the shorter prefix entry would claim
        // `@org/domain-testing/x` and resolve it inside the wrong package.
        packages.sort_by(|a, b| {
            b.name
                .len()
                .cmp(&a.name.len())
                .then_with(|| a.name.cmp(&b.name))
        });
        Self { packages }
    }

    /// The packages found, longest name first.
    #[must_use]
    pub fn packages(&self) -> &[Package] {
        &self.packages
    }

    /// Whether a specifier names one of these packages.
    ///
    /// Answers the question `impact --apply` asks of every import it is about
    /// to rewrite: is this specifier one archwarden understands the shape of,
    /// or an outside dependency it must leave alone?
    #[must_use]
    pub fn package_for(&self, specifier: &str) -> Option<&Package> {
        self.packages.iter().find(|package| {
            specifier
                .strip_prefix(package.name.as_str())
                .is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
        })
    }

    /// The map, in the form `oxc_resolver` takes.
    ///
    /// Absolute, because an alias value is resolved as written rather than
    /// against the importer.
    #[must_use]
    pub fn fallback(&self, root: &Utf8Path) -> Alias {
        let mut alias: Alias = Vec::new();

        for package in &self.packages {
            let directory = root.join(&package.directory);

            for (subpath, target) in &package.subpaths {
                let Some(key) = key_for(&package.name, subpath) else {
                    continue;
                };
                let value = directory.join(target.trim_start_matches("./"));
                alias.push((key, vec![AliasValue::Path(value.into_string())]));
            }

            // Last, and always: a package with no `exports` is importable by
            // path, and one with `exports` still needs somewhere for a
            // specifier its patterns did not cover to land — where it will
            // fail, which is the honest answer rather than a wrong one.
            alias.push((
                package.name.clone(),
                vec![AliasValue::Path(directory.into_string())],
            ));
        }

        alias
    }
}

/// The alias key for one `exports` subpath.
///
/// `"."` is the package itself and gets `$`, which is how `oxc_resolver`
/// spells an exact match — without it the entry would also claim every
/// subpath and shadow the patterns declared beside it.
fn key_for(name: &str, subpath: &str) -> Option<String> {
    if subpath == "." {
        return Some(format!("{name}$"));
    }
    let tail = subpath.strip_prefix("./")?;
    Some(format!("{name}/{tail}"))
}

/// Reads every `package.json` below `directory`, depth-first.
fn collect(root: &Utf8Path, relative: &Utf8Path, depth: usize, found: &mut Vec<Package>) {
    if let Some(package) = read_manifest(&root.join(relative).join("package.json"), relative) {
        found.push(package);
    }

    if depth >= MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root.join(relative)) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with('.') || SKIP.contains(&name.as_str()) {
            continue;
        }
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            collect(root, &relative.join(name), depth + 1, found);
        }
    }
}

/// Parses one `package.json` into a [`Package`], if it is one.
fn read_manifest(manifest: &Utf8Path, directory: &Utf8Path) -> Option<Package> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let name = json.get("name")?.as_str()?.to_owned();

    Some(Package {
        name,
        directory: directory.to_owned(),
        subpaths: subpaths(json.get("exports")),
    })
}

/// Flattens an `exports` field into `(subpath, target)` pairs.
///
/// The three shapes Node allows at the top level: a bare string (the package
/// itself), a map of conditions (still the package itself), and a map of
/// subpaths. Telling the last two apart is what the `.`-prefix test does —
/// which is Node's own rule, not a heuristic.
fn subpaths(exports: Option<&serde_json::Value>) -> Vec<(String, String)> {
    let Some(exports) = exports else {
        return Vec::new();
    };

    match exports {
        serde_json::Value::String(target) => vec![(".".to_owned(), target.clone())],
        serde_json::Value::Object(map) => {
            let is_subpath_map = map.keys().any(|key| key.starts_with('.'));
            if !is_subpath_map {
                return condition(exports)
                    .map(|target| vec![(".".to_owned(), target)])
                    .unwrap_or_default();
            }
            map.iter()
                .filter(|(key, _)| key.starts_with('.'))
                .filter_map(|(key, value)| Some((key.clone(), condition(value)?)))
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Picks the target a TypeScript build would take from one `exports` entry.
///
/// A string is itself; a conditions object is its best matching condition,
/// recursively; an array is its first member that yields one. `null` means
/// the subpath is deliberately not exported, and yields nothing.
fn condition(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(target) => Some(target.clone()),
        serde_json::Value::Array(members) => members.iter().find_map(condition),
        serde_json::Value::Object(map) => CONDITIONS
            .iter()
            .find_map(|name| map.get(*name).and_then(condition)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(entries: &[(&str, &str)]) -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");

        for (relative, contents) in entries {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().expect("a file has a parent"))
                .expect("create dirs");
            std::fs::write(&path, contents).expect("write file");
        }

        (dir, root)
    }

    fn discovered(entries: &[(&str, &str)]) -> Vec<Package> {
        let (guard, root) = repo(entries);
        let found = Workspace::discover(&root);
        drop(guard);
        found.packages().to_vec()
    }

    /// The shape the whole module exists for: a package name and the
    /// subpath pattern that says where its files really live.
    #[test]
    fn a_package_with_subpath_exports_is_read() {
        let found = discovered(&[(
            "packages/domain/package.json",
            r#"{"name":"@org/domain","exports":{"./email/*":"./src/email/*.ts"}}"#,
        )]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "@org/domain");
        assert_eq!(found[0].directory, "packages/domain");
        assert_eq!(
            found[0].subpaths,
            [("./email/*".to_owned(), "./src/email/*.ts".to_owned())]
        );
    }

    /// `node_modules` is the artefact this module exists to work without. A
    /// package found inside it would shadow the local one it is a copy of.
    #[test]
    fn a_package_inside_node_modules_is_not_a_workspace_package() {
        let found = discovered(&[
            ("packages/domain/package.json", r#"{"name":"@org/domain"}"#),
            (
                "node_modules/@org/domain/package.json",
                r#"{"name":"@org/domain"}"#,
            ),
        ]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].directory, "packages/domain");
    }

    /// Longest first, or `@org/domain` claims `@org/domain-testing/x` and
    /// resolves it inside the wrong package.
    #[test]
    fn a_longer_name_is_matched_before_the_prefix_of_it() {
        let workspace = {
            let (guard, root) = repo(&[
                ("packages/domain/package.json", r#"{"name":"@org/domain"}"#),
                (
                    "packages/domain-testing/package.json",
                    r#"{"name":"@org/domain-testing"}"#,
                ),
            ]);
            let found = Workspace::discover(&root);
            drop(guard);
            found
        };

        assert_eq!(
            workspace
                .package_for("@org/domain-testing/helpers")
                .map(|package| package.name.as_str()),
            Some("@org/domain-testing")
        );
    }

    /// A name that merely starts the same is not a match: `@org/domainx` is
    /// a different package, and only a `/` or the end of the string separates
    /// a package from its subpath.
    #[test]
    fn a_name_that_is_only_a_prefix_is_not_a_match() {
        let (guard, root) = repo(&[("packages/domain/package.json", r#"{"name":"@org/domain"}"#)]);
        let workspace = Workspace::discover(&root);
        drop(guard);

        assert!(workspace.package_for("@org/domainx").is_none());
        assert!(workspace.package_for("@org/domain").is_some());
        assert!(workspace.package_for("@org/domain/email/x").is_some());
    }

    /// `exports` as a bare string is the package itself.
    #[test]
    fn a_string_exports_is_the_package_root() {
        let found = discovered(&[(
            "packages/x/package.json",
            r#"{"name":"x","exports":"./src/index.ts"}"#,
        )]);

        assert_eq!(
            found[0].subpaths,
            [(".".to_owned(), "./src/index.ts".to_owned())]
        );
    }

    /// A conditions object with no `.` key is the package itself too, and the
    /// condition order decides which target. `types` before `import` because
    /// a TypeScript file imports declarations.
    #[test]
    fn a_conditions_object_picks_the_typescript_target() {
        let found = discovered(&[(
            "packages/x/package.json",
            r#"{"name":"x","exports":{"import":"./dist/x.js","types":"./src/x.ts"}}"#,
        )]);

        assert_eq!(
            found[0].subpaths,
            [(".".to_owned(), "./src/x.ts".to_owned())]
        );
    }

    /// Nested conditions, which is what a package with both a subpath map and
    /// per-subpath conditions looks like.
    #[test]
    fn conditions_nested_under_a_subpath_are_flattened() {
        let found = discovered(&[(
            "packages/x/package.json",
            r#"{"name":"x","exports":{"./a":{"types":"./src/a.ts","default":"./dist/a.js"}}}"#,
        )]);

        assert_eq!(
            found[0].subpaths,
            [("./a".to_owned(), "./src/a.ts".to_owned())]
        );
    }

    /// `"./private": null` is a subpath deliberately not exported. Inventing a
    /// target for it would resolve an import Node refuses.
    #[test]
    fn a_null_target_exports_nothing() {
        let found = discovered(&[(
            "packages/x/package.json",
            r#"{"name":"x","exports":{"./a":"./src/a.ts","./private":null}}"#,
        )]);

        assert_eq!(
            found[0].subpaths,
            [("./a".to_owned(), "./src/a.ts".to_owned())]
        );
    }

    /// A manifest with no `name` is configuration, not a package -- the root
    /// `package.json` of most monorepos is exactly this.
    #[test]
    fn a_manifest_without_a_name_is_not_a_package() {
        let found = discovered(&[(
            "package.json",
            r#"{"private":true,"devDependencies":{"x":"1"}}"#,
        )]);

        assert!(found.is_empty());
    }

    /// A manifest that will not parse is one this cannot learn from, and
    /// refusing to resolve the rest of the repository because of it would be
    /// the wrong trade.
    #[test]
    fn a_broken_manifest_does_not_stop_the_search() {
        let found = discovered(&[
            ("packages/broken/package.json", "{ not json"),
            ("packages/x/package.json", r#"{"name":"x"}"#),
        ]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "x");
    }

    /// The map as the resolver takes it: the exact entry for `.`, the wildcard
    /// for a subpath pattern, and the bare directory last.
    #[test]
    fn the_fallback_map_carries_exact_wildcard_and_directory_entries() {
        let (guard, root) = repo(&[(
            "packages/domain/package.json",
            r#"{"name":"@org/domain","exports":{".":"./src/index.ts","./email/*":"./src/email/*.ts"}}"#,
        )]);
        let alias = Workspace::discover(&root).fallback(&root);
        let keys: Vec<&str> = alias.iter().map(|(key, _)| key.as_str()).collect();
        drop(guard);

        assert_eq!(
            keys,
            ["@org/domain$", "@org/domain/email/*", "@org/domain"],
            "the exact and wildcard entries come before the directory catch-all"
        );
    }
}
