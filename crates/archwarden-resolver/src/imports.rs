//! Resolving import specifiers to files.
//!
//! A boundary rule matches globs against where an import *lands*, not against
//! what was written. `@/domain/user` and `../../domain/user` are the same edge
//! in the graph, and only a resolver can say so.
//!
//! Everything hard about this -- `tsconfig.paths`, `exports` conditions, the
//! `.js`-means-`.ts` convention, pnpm's symlink layout, workspace links --
//! `oxc_resolver` already does (decision 7). What this module owns is the
//! configuration for TypeScript source and the classification of the answer:
//! inside the repository, a dependency, or a runtime builtin.

use archwarden_core::{
    path::RepoRelPath,
    traits::{Resolved, Resolver},
};
use camino::{Utf8Path, Utf8PathBuf};

/// Extensions tried for a specifier written without one.
///
/// TypeScript before JavaScript: in a repository that ships both `user.ts` and
/// a compiled `user.js`, the source is the file a rule is about.
/// `.astro` is last and is never guessed at: Astro requires the extension to
/// be written, so this entry only ever matches an import that already said it.
/// Without it, `import Layout from './Base.astro'` resolves to nothing and a
/// boundary rule sees an unresolved specifier instead of an in-repo path.
/// Issue #13.
const EXTENSIONS: [&str; 10] = [
    ".ts", ".tsx", ".mts", ".cts", ".d.ts", ".js", ".jsx", ".mjs", ".cjs", ".astro",
];

/// Fields consulted in a dependency's `package.json`, best first.
///
/// `types` first because a dependency's type declarations are what a
/// TypeScript file actually imports from; `module` before `main` because the
/// ESM entry point is the one a bundler picks.
const MAIN_FIELDS: [&str; 3] = ["types", "module", "main"];

/// `exports` conditions, in the order a TypeScript ESM build would apply them.
///
/// `node` is here because a package may offer *only* platform conditions.
/// `bwip-js` maps `.` to `browser`, `electron`, `react-native` and `node` with
/// no `default` at all, so a resolver applying none of them matches nothing --
/// and because `exports` is present there is no legitimate fall back to `main`.
/// The package is installed, Node runs it and `tsc` type-checks it; only
/// archwarden called it unresolved, which put a boundary rule's blind spot on a
/// dependency that was never a blind spot. Issue #21.
///
/// `node` rather than `browser` because that is what archwarden itself runs
/// under, and because the alternative would be to guess at a repository's
/// target from nothing.
const CONDITIONS: [&str; 5] = ["types", "node", "import", "require", "default"];

/// The directory whose presence makes a resolved file a dependency rather than
/// part of the repository.
const DEPENDENCY_DIRECTORY: &str = "node_modules";

/// Why an import could not be resolved.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ImportError {
    /// The specifier did not resolve to anything.
    #[error("cannot resolve `{specifier}` from `{importer}`")]
    Unresolved {
        /// The specifier, as written in the source.
        specifier: String,
        /// The file that imported it.
        importer: RepoRelPath,
        /// What the resolver said. Boxed because `ResolveError` is large
        /// enough to bloat every `Result` this module returns, successful ones
        /// included.
        #[source]
        source: Box<oxc_resolver::ResolveError>,
    },

    /// It resolved, but to a path archwarden cannot represent.
    #[error("`{specifier}` from `{importer}` resolved to a path that is not valid UTF-8")]
    NonUtf8Path {
        /// The specifier, as written in the source.
        specifier: String,
        /// The file that imported it.
        importer: RepoRelPath,
    },
}

/// Resolves import specifiers against a repository.
#[derive(Debug)]
pub struct ImportResolver {
    inner: oxc_resolver::ResolverGeneric<crate::listing::Listings>,
    root: Utf8PathBuf,
    workspace: crate::workspace::Workspace,
}

impl ImportResolver {
    /// Builds a resolver for TypeScript and JavaScript source under `root`.
    ///
    /// `tsconfig` discovery is automatic: the nearest one to the importing file
    /// wins, which is what a monorepo where every package has its own
    /// `compilerOptions.paths` needs.
    ///
    /// The repository's own packages are read from their manifests and passed
    /// as `fallback`, so `@org/domain/email/x` resolves on a machine that has
    /// not run an install. `fallback` and not `alias`: it is consulted only
    /// after normal resolution fails, so an installed package always wins over
    /// the reconstruction of it. See [`crate::workspace`].
    ///
    /// The other half of that bargain is in `classify`: when normal resolution
    /// succeeds and lands on a *copy* of one of those packages under
    /// `node_modules`, the answer is mapped back to the source. Winning is
    /// right for a dependency and wrong for an artefact made from a file in
    /// the repository.
    #[must_use]
    pub fn new(root: &Utf8Path) -> Self {
        let workspace = crate::workspace::Workspace::discover(root);
        let options = oxc_resolver::ResolveOptions {
            fallback: workspace.fallback(root),
            extensions: EXTENSIONS.iter().map(|e| (*e).to_owned()).collect(),
            main_fields: MAIN_FIELDS.iter().map(|f| (*f).to_owned()).collect(),
            condition_names: CONDITIONS.iter().map(|c| (*c).to_owned()).collect(),
            // TypeScript's ESM convention: `./user.js` in a `.ts` file means
            // `./user.ts`. Without this, half a NodeNext repository resolves to
            // nothing.
            extension_alias: vec![
                (
                    ".js".to_owned(),
                    vec![".ts".to_owned(), ".tsx".to_owned(), ".js".to_owned()],
                ),
                (
                    ".mjs".to_owned(),
                    vec![".mts".to_owned(), ".mjs".to_owned()],
                ),
                (
                    ".cjs".to_owned(),
                    vec![".cts".to_owned(), ".cjs".to_owned()],
                ),
            ],
            // A builtin comes back as a distinguishable error rather than as
            // "unresolved", which is the only way to tell `node:fs` from a
            // dependency someone forgot to install.
            builtin_modules: true,
            tsconfig: Some(oxc_resolver::TsconfigDiscovery::Auto),
            ..oxc_resolver::ResolveOptions::default()
        };

        Self {
            // `ResolverGeneric<Listings>` rather than `Resolver`, which is
            // `ResolverGeneric<FileSystemOs>`. The only difference is that a
            // name no directory holds is answered from a listing instead of a
            // `statx` that returns nothing — over half of resolution's calls,
            // and the whole of the 10x a shared mount costs. Issue #82.
            inner: oxc_resolver::ResolverGeneric::new_with_file_system(
                crate::listing::Listings::default(),
                options,
            ),
            root: root.to_owned(),
            workspace,
        }
    }

    /// The repository's own packages, as their manifests describe them.
    ///
    /// Exposed because rewriting an import needs to know whether a specifier
    /// names a local package — one whose specifier a move changes — or an
    /// outside dependency it must leave alone.
    #[must_use]
    pub fn workspace(&self) -> &crate::workspace::Workspace {
        &self.workspace
    }

    /// The repository file a copied workspace package under `node_modules`
    /// was made from.
    ///
    /// `node_modules/@org/domain/src/email/x.ts` is `packages/domain/src/email/x.ts`
    /// when `@org/domain` is a package this repository declares: a copy mirrors
    /// the directory it was copied from, so the tail after the package name
    /// maps across unchanged.
    ///
    /// Why it matters, measured: with the copy classified as external, the file
    /// importing it disappears from the graph. `impact` then reports two
    /// importers where there are three, and `--apply` rewrites two of them and
    /// leaves the third pointing at a file that has moved. Same repository,
    /// same commit, same binary -- 29 specifiers rewritten with a symlink, 26
    /// and three broken imports with a copy.
    ///
    /// Only when the source is actually there. A dependency that merely shares
    /// a name with a local package is still a dependency, and inventing a path
    /// for it would be worse than calling it external.
    fn back_to_source(&self, relative: &Utf8Path) -> Option<RepoRelPath> {
        let parts: Vec<&str> = relative.as_str().split('/').collect();
        let start = parts
            .iter()
            .rposition(|part| *part == DEPENDENCY_DIRECTORY)?
            + 1;
        let inside = parts.get(start..)?;

        // `@scope/name` is two segments, a bare name is one. Longest first, so
        // a scoped package is not mistaken for an unscoped one.
        for width in [2, 1] {
            let Some(name) = inside.get(..width).map(|segments| segments.join("/")) else {
                continue;
            };
            let Some(package) = self
                .workspace
                .packages()
                .iter()
                .find(|package| package.name == name)
            else {
                continue;
            };

            let tail = inside.get(width..)?.join("/");
            let source = package.directory.join(&tail);
            if !self.root.join(&source).is_file() {
                return None;
            }
            return RepoRelPath::new(source.as_str()).ok();
        }

        None
    }

    /// Decides what a resolved absolute path is, from archwarden's point of
    /// view.
    ///
    /// A file under the root is part of the repository *unless* it sits in a
    /// `node_modules`, which is where a workspace link stops being interesting
    /// and a dependency starts. Symlinks are followed first, so a workspace
    /// package linked into `node_modules` classifies by where it really lives
    /// -- which is the whole point in a monorepo.
    ///
    /// A *copy* is the case that rule gets wrong, and it is not exotic: pnpm
    /// with `node-linker=hoisted`, npm on a filesystem without symlinks, a
    /// container volume, a partial install. The copy is an artefact of the
    /// installer and the file it was made from is right there in the
    /// repository -- so it is mapped back rather than written off as somebody
    /// else's code. See [`Self::back_to_source`].
    fn classify(&self, path: Utf8PathBuf) -> Resolved {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return Resolved::External(path);
        };

        if relative
            .components()
            .any(|component| component.as_str() == DEPENDENCY_DIRECTORY)
        {
            return self
                .back_to_source(relative)
                .map_or(Resolved::External(path), Resolved::InRepo);
        }

        RepoRelPath::new(relative.as_str()).map_or(Resolved::External(path), Resolved::InRepo)
    }
}

impl Resolver for ImportResolver {
    type Error = ImportError;

    fn resolve(&self, importer: &RepoRelPath, specifier: &str) -> Result<Resolved, ImportError> {
        // `resolve_file`, not `resolve`: automatic `tsconfig` discovery only
        // works from a file path, because the config that applies is the
        // nearest one *above the importer* -- which in a monorepo is a
        // different file for every package.
        let file = self.root.join(importer.as_path());

        let resolution = match self.inner.resolve_file(file.as_std_path(), specifier) {
            Ok(resolution) => resolution,
            // Not a failure: a builtin has no file, and saying so is the
            // answer. A boundary rule that forbids `node:fs` needs to see it.
            Err(oxc_resolver::ResolveError::Builtin { resolved, .. }) => {
                return Ok(Resolved::Builtin(resolved));
            }
            Err(source) => {
                return Err(ImportError::Unresolved {
                    specifier: specifier.to_owned(),
                    importer: importer.clone(),
                    source: Box::new(source),
                });
            }
        };

        let path = Utf8PathBuf::from_path_buf(resolution.into_path_buf()).map_err(|_| {
            ImportError::NonUtf8Path {
                specifier: specifier.to_owned(),
                importer: importer.clone(),
            }
        })?;

        Ok(self.classify(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a temporary repository and returns its canonical UTF-8 root.
    fn repo(entries: &[(&str, &str)]) -> (tempfile::TempDir, Utf8PathBuf) {
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

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// Resolves `specifier` as written in `importer`, and says in one line
    /// where it landed *and* how it was classified.
    ///
    /// A string rather than the value itself, because the classification is
    /// half of what these tests are about and an assertion that spells it out
    /// reads as the behaviour: `"in-repo src/user.ts"`, `"builtin fs"`.
    /// Absolute paths are made relative to the repository so an assertion does
    /// not contain a temporary directory name.
    fn landed(entries: &[(&str, &str)], importer: &str, specifier: &str) -> String {
        let (guard, root) = repo(entries);
        let described = describe(
            &root,
            ImportResolver::new(&root).resolve(&path(importer), specifier),
        );
        drop(guard);
        described
    }

    fn describe(root: &Utf8Path, resolved: Result<Resolved, ImportError>) -> String {
        match resolved {
            Ok(Resolved::InRepo(landed)) => format!("in-repo {landed}"),
            Ok(Resolved::External(landed)) => {
                format!("external {}", landed.strip_prefix(root).unwrap_or(&landed))
            }
            Ok(Resolved::Builtin(name)) => format!("builtin {name}"),
            // `Resolved` is non_exhaustive; a variant added later says what it
            // is rather than failing to compile here.
            Ok(other) => format!("{other:?}"),
            Err(error) => format!("error {error}"),
        }
    }

    const TS: &str = "export const value = 1;";

    /// The plain case, and the reason the resolver exists: a rule matches
    /// globs against where an import lands, not against what was written.
    #[test]
    fn a_relative_specifier_lands_on_a_repository_file() {
        assert_eq!(
            landed(
                &[
                    ("src/user/create.ts", "import { value } from '../shared/x';"),
                    ("src/shared/x.ts", TS),
                ],
                "src/user/create.ts",
                "../shared/x",
            ),
            "in-repo src/shared/x.ts"
        );
    }

    /// TypeScript source is written without extensions. Trying `.ts` before
    /// `.js` matters in a repository that ships both.
    #[test]
    fn typescript_wins_over_compiled_javascript() {
        assert_eq!(
            landed(
                &[
                    ("src/app.ts", ""),
                    ("src/user.ts", TS),
                    ("src/user.js", "exports.value = 1;"),
                ],
                "src/app.ts",
                "./user",
            ),
            "in-repo src/user.ts"
        );
    }

    /// The `NodeNext` convention: a `.ts` file importing `./user.js` means
    /// `./user.ts`. Without the extension alias, half of a modern repository
    /// resolves to nothing.
    #[test]
    fn a_js_specifier_finds_the_ts_source() {
        assert_eq!(
            landed(
                &[("src/app.ts", ""), ("src/user.ts", TS)],
                "src/app.ts",
                "./user.js",
            ),
            "in-repo src/user.ts"
        );
    }

    /// A directory import lands on its `index`, which is how a folder-as-module
    /// is spelled everywhere in the ecosystem.
    #[test]
    fn a_directory_lands_on_its_index() {
        assert_eq!(
            landed(
                &[("src/app.ts", ""), ("src/user/index.ts", TS)],
                "src/app.ts",
                "./user",
            ),
            "in-repo src/user/index.ts"
        );
    }

    /// `tsconfig.paths` is the whole reason an alias and a relative path have
    /// to become the same edge. This is the case a boundary rule cannot see
    /// without a resolver.
    #[test]
    fn a_tsconfig_path_alias_resolves_to_the_same_file_as_the_relative_form() {
        let entries = [
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
            ),
            ("src/user/create.ts", ""),
            ("src/domain/user.ts", TS),
        ];

        assert_eq!(
            landed(&entries, "src/user/create.ts", "@/domain/user"),
            "in-repo src/domain/user.ts"
        );
        assert_eq!(
            landed(&entries, "src/user/create.ts", "../domain/user"),
            "in-repo src/domain/user.ts"
        );
    }

    /// A monorepo gives each package its own `tsconfig`, and the same alias
    /// means different things in each. Discovery is per importer for exactly
    /// this reason.
    #[test]
    fn each_package_gets_its_own_tsconfig() {
        let entries = [
            (
                "packages/app/tsconfig.json",
                r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
            ),
            ("packages/app/src/main.ts", ""),
            ("packages/app/src/thing.ts", TS),
            (
                "packages/api/tsconfig.json",
                r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["lib/*"]}}}"#,
            ),
            ("packages/api/src/main.ts", ""),
            ("packages/api/lib/thing.ts", TS),
        ];

        assert_eq!(
            landed(&entries, "packages/app/src/main.ts", "@/thing"),
            "in-repo packages/app/src/thing.ts"
        );
        assert_eq!(
            landed(&entries, "packages/api/src/main.ts", "@/thing"),
            "in-repo packages/api/lib/thing.ts"
        );
    }

    /// The other side of per-importer discovery: an alias declared by a
    /// `tsconfig` that does not govern the importing file does not apply to it.
    ///
    /// This is what issue #22 hit while extracting a package out of an app.
    /// The files sit in `packages/domain` and still write `@Domain/*`, which
    /// only `apps/api/tsconfig.json` declares. `tsc` resolves it because those
    /// files are still in the app's program; archwarden asks the `tsconfig`
    /// that governs the file on disk, and that one has never heard of it.
    ///
    /// Not a bug to fix by merging every `paths` map in the repository into
    /// one. `each_package_gets_its_own_tsconfig` above is why: `@/*` is the
    /// most common alias there is, it means a different directory in each
    /// package, and a merged map would resolve one package's import into
    /// another's source. A boundary rule fed a wrong edge is worse than one
    /// fed no edge, because `check` now names the import it could not place
    /// (issue #18) and says nothing about the one it placed wrongly.
    #[test]
    fn an_alias_from_a_tsconfig_that_does_not_govern_the_file_does_not_apply() {
        let entries = [
            (
                "apps/api/tsconfig.json",
                r#"{"compilerOptions":{"baseUrl":".","paths":{"@Domain/*":["src/Domain/*"]}}}"#,
            ),
            ("apps/api/src/Domain/order.ts", TS),
            ("packages/domain/row.ts", ""),
        ];

        assert!(
            landed(&entries, "packages/domain/row.ts", "@Domain/order").starts_with("error"),
            "the app's alias is not the package's"
        );
        assert_eq!(
            landed(&entries, "apps/api/src/main.ts", "@Domain/order"),
            "in-repo apps/api/src/Domain/order.ts",
            "and inside the app it resolves, because there it is declared"
        );
    }

    /// A `tsconfig` with no `paths` shadows an ancestor that has them, unless
    /// it extends it. The nearest one wins whole, which is TypeScript's own
    /// rule and the trap in it: adding a bare `tsconfig.json` to a package
    /// silently takes the repository's aliases away from every file under it.
    #[test]
    fn a_nearer_tsconfig_shadows_an_ancestors_paths_unless_it_extends_it() {
        fn root_declares(package_tsconfig: &str) -> [(&str, &str); 4] {
            [
                (
                    "tsconfig.json",
                    r#"{"compilerOptions":{"baseUrl":".","paths":{"@Domain/*":["apps/api/src/Domain/*"]}}}"#,
                ),
                ("apps/api/src/Domain/order.ts", TS),
                ("packages/domain/tsconfig.json", package_tsconfig),
                ("packages/domain/row.ts", ""),
            ]
        }

        assert!(
            landed(
                &root_declares(r#"{"compilerOptions":{"strict":true}}"#),
                "packages/domain/row.ts",
                "@Domain/order",
            )
            .starts_with("error"),
            "a bare `tsconfig.json` takes the ancestor's aliases away"
        );

        assert_eq!(
            landed(
                &root_declares(r#"{"extends":"../../tsconfig.json"}"#),
                "packages/domain/row.ts",
                "@Domain/order",
            ),
            "in-repo apps/api/src/Domain/order.ts",
            "and `extends` gives them back, which is how a monorepo writes it"
        );
    }

    /// A workspace package is linked into `node_modules`, but it is source in
    /// this repository. Following the link is what lets a boundary rule written
    /// against `packages/domain/**` see an import of `@org/domain`.
    #[cfg(unix)]
    #[test]
    fn a_workspace_package_resolves_to_its_source_not_its_link() {
        let (guard, root) = repo(&[
            ("packages/app/src/main.ts", ""),
            (
                "packages/domain/package.json",
                r#"{"name":"@org/domain","types":"src/index.ts"}"#,
            ),
            ("packages/domain/src/index.ts", TS),
        ]);
        std::fs::create_dir_all(root.join("node_modules/@org")).expect("create dirs");
        std::os::unix::fs::symlink(
            root.join("packages/domain"),
            root.join("node_modules/@org/domain"),
        )
        .expect("symlink");

        let resolved = describe(
            &root,
            ImportResolver::new(&root).resolve(&path("packages/app/src/main.ts"), "@org/domain"),
        );
        drop(guard);

        assert_eq!(resolved, "in-repo packages/domain/src/index.ts");
    }

    /// The same import, on a machine that has not run an install.
    ///
    /// This is the half of a monorepo's import graph that used to disappear:
    /// no `node_modules`, so Node's own answer is "nothing", and every
    /// boundary rule about this edge silently passed. The subpath pattern in
    /// `exports` says where it really lands, and that is read from the
    /// manifest rather than from the symlink a package manager would have
    /// left. See `crate::workspace`.
    #[test]
    fn a_workspace_package_resolves_with_no_node_modules_at_all() {
        let (guard, root) = repo(&[
            ("packages/app/src/main.ts", ""),
            (
                "packages/domain/package.json",
                r#"{"name":"@org/domain","exports":{"./email/*":"./src/email/*.ts"}}"#,
            ),
            ("packages/domain/src/email/is-invalid.ts", TS),
        ]);

        let resolved = describe(
            &root,
            ImportResolver::new(&root).resolve(
                &path("packages/app/src/main.ts"),
                "@org/domain/email/is-invalid",
            ),
        );
        drop(guard);

        assert_eq!(resolved, "in-repo packages/domain/src/email/is-invalid.ts");
    }

    /// Issue #169. At one level a specifier can name a file or a directory,
    /// and the only `exports` map that satisfies both Node and `tsc` is the
    /// array -- Node's fallback list, tried in order.
    ///
    /// Reading only the first member left a real monorepo with 2073 of 20843
    /// imports unresolved while `check` exited 0: the boundary rules were
    /// blind to a tenth of the repository and nothing said so loudly enough to
    /// fail a build.
    #[test]
    fn every_target_of_an_exports_array_is_tried_in_order() {
        let (guard, root) = repo(&[
            ("apps/api/src/main.ts", ""),
            (
                "packages/application/package.json",
                r#"{"name":"@org/application","type":"module","exports":{
                    "./*.ts":"./src/*.ts",
                    "./*":["./src/*.ts","./src/*/index.ts"]}}"#,
            ),
            ("packages/application/src/Order/queue-send.ts", TS),
            ("packages/application/src/Order/create/index.ts", TS),
        ]);

        let resolve = |specifier: &str| {
            describe(
                &root,
                ImportResolver::new(&root).resolve(&path("apps/api/src/main.ts"), specifier),
            )
        };

        // The first target: a file-shaped specifier, which always worked.
        assert_eq!(
            resolve("@org/application/Order/queue-send"),
            "in-repo packages/application/src/Order/queue-send.ts"
        );
        // The second: a directory-shaped one, which is the whole bug.
        assert_eq!(
            resolve("@org/application/Order/create"),
            "in-repo packages/application/src/Order/create/index.ts"
        );
        // And a specifier no target reaches is still unresolved. The list
        // widens what resolves; it does not make everything resolve.
        let missing = resolve("@org/application/Order/nothing-here");
        assert!(missing.contains("cannot resolve"), "{missing}");
        drop(guard);
    }

    /// A subpath `exports` does not cover stays unresolved.
    ///
    /// The map fills the hole `node_modules` would have filled; it does not
    /// widen it. An import Node refuses must be refused here too, or a
    /// boundary rule would be evaluated against a path nobody can import.
    #[test]
    fn a_subpath_outside_the_exports_map_does_not_resolve() {
        let (guard, root) = repo(&[
            ("packages/app/src/main.ts", ""),
            (
                "packages/domain/package.json",
                r#"{"name":"@org/domain","exports":{"./email/*":"./src/email/*.ts"}}"#,
            ),
            ("packages/domain/src/secret/internal.ts", TS),
        ]);

        let resolved = ImportResolver::new(&root).resolve(
            &path("packages/app/src/main.ts"),
            "@org/domain/secret/internal",
        );
        drop(guard);

        assert!(resolved.is_err(), "{resolved:?}");
    }

    /// An installed package wins over the reconstruction of it. The map is a
    /// `fallback`, consulted only once normal resolution has failed, so a
    /// repository that *has* installed its dependencies resolves exactly as it
    /// did before this existed.
    #[cfg(unix)]
    #[test]
    fn an_installed_package_is_not_overruled_by_the_workspace_map() {
        let (guard, root) = repo(&[
            ("packages/app/src/main.ts", ""),
            (
                "packages/domain/package.json",
                r#"{"name":"@org/domain","exports":{"./x":"./src/from-manifest.ts"}}"#,
            ),
            ("packages/domain/src/from-manifest.ts", TS),
            ("packages/domain/src/from-link.ts", TS),
        ]);
        // The installed copy exports the same subpath at a different file. If
        // the fallback were an `alias`, it would run first and win.
        std::fs::write(
            root.join("packages/domain/package.json"),
            r#"{"name":"@org/domain","exports":{"./x":"./src/from-link.ts"}}"#,
        )
        .expect("write manifest");
        std::fs::create_dir_all(root.join("node_modules/@org")).expect("create dirs");
        std::os::unix::fs::symlink(
            root.join("packages/domain"),
            root.join("node_modules/@org/domain"),
        )
        .expect("symlink");

        let resolved = describe(
            &root,
            ImportResolver::new(&root).resolve(&path("packages/app/src/main.ts"), "@org/domain/x"),
        );
        drop(guard);

        assert_eq!(resolved, "in-repo packages/domain/src/from-link.ts");
    }

    /// A workspace package *copied* into `node_modules` is still the
    /// repository's own file.
    ///
    /// This is the bug that shipped in 0.5.0. pnpm with
    /// `node-linker=hoisted`, npm on a filesystem without symlinks, a
    /// container volume, a partial install — all leave a copy rather than a
    /// link, and a copy has `node_modules` in its path. Classified as a
    /// dependency, the file importing it vanishes from the graph: `impact`
    /// reported two importers where there were three, and `--apply` rewrote
    /// two and left the third pointing at a file that had moved. Exit 0.
    ///
    /// Measured on a real monorepo, same commit and same binary: 29 specifiers
    /// rewritten with a symlink, 26 and three broken imports with a copy.
    #[test]
    fn a_workspace_package_copied_into_node_modules_is_still_ours() {
        let (guard, root) = repo(&[
            ("apps/web/src/main.ts", ""),
            (
                "packages/domain/package.json",
                r#"{"name":"@org/domain","exports":{"./email/*":"./src/email/*.ts"}}"#,
            ),
            ("packages/domain/src/email/is-invalid.ts", TS),
            // The copy, byte for byte what an installer leaves behind.
            (
                "node_modules/@org/domain/package.json",
                r#"{"name":"@org/domain","exports":{"./email/*":"./src/email/*.ts"}}"#,
            ),
            ("node_modules/@org/domain/src/email/is-invalid.ts", TS),
        ]);

        let resolved = describe(
            &root,
            ImportResolver::new(&root).resolve(
                &path("apps/web/src/main.ts"),
                "@org/domain/email/is-invalid",
            ),
        );
        drop(guard);

        assert_eq!(
            resolved, "in-repo packages/domain/src/email/is-invalid.ts",
            "a copy of our own package is our own file, not somebody else's code"
        );
    }

    /// And a dependency that merely shares a name with nothing local stays a
    /// dependency. The mapping only fires for a package this repository
    /// declares, and only when the source is really there.
    #[test]
    fn a_dependency_is_not_mapped_into_the_repository() {
        let (guard, root) = repo(&[
            ("apps/web/src/main.ts", ""),
            (
                "node_modules/@org/domain/package.json",
                r#"{"name":"@org/domain","types":"src/index.d.ts"}"#,
            ),
            (
                "node_modules/@org/domain/src/index.d.ts",
                "export const x: number;",
            ),
        ]);

        let resolved = describe(
            &root,
            ImportResolver::new(&root).resolve(&path("apps/web/src/main.ts"), "@org/domain"),
        );
        drop(guard);

        assert_eq!(resolved, "external node_modules/@org/domain/src/index.d.ts");
    }

    /// A real dependency is not part of the repository, so no boundary glob
    /// should ever match it as a path.
    #[test]
    fn an_installed_dependency_is_external() {
        let (guard, root) = repo(&[
            ("src/app.ts", ""),
            (
                "node_modules/lodash/package.json",
                r#"{"name":"lodash","main":"index.js"}"#,
            ),
            ("node_modules/lodash/index.js", "module.exports = {};"),
        ]);

        let resolved = describe(
            &root,
            ImportResolver::new(&root).resolve(&path("src/app.ts"), "lodash"),
        );
        drop(guard);

        assert_eq!(resolved, "external node_modules/lodash/index.js");
    }

    /// A package whose `exports` offers only platform conditions and no
    /// `default`. Applying none of them matches nothing, and `exports` being
    /// present blocks the fall back to `main`, so an installed dependency was
    /// reported as an import nothing could place -- with the note sending the
    /// reader to run `install`, which was already done.
    ///
    /// This is `bwip-js@4.11.2`'s manifest, trimmed to the shape that matters.
    /// Issue #21.
    #[test]
    fn a_dependency_with_only_platform_conditions_resolves() {
        let (guard, root) = repo(&[
            ("src/barcode.ts", ""),
            (
                "node_modules/bwip-js/package.json",
                r#"{
                    "name": "bwip-js",
                    "main": "./dist/bwip-js-node.js",
                    "exports": {
                        ".": {
                            "browser": { "import": "./dist/bwip-js-browser.mjs" },
                            "react-native": { "default": "./dist/bwip-js-rn.js" },
                            "node": {
                                "types": "./dist/bwip-js-node.d.ts",
                                "import": "./dist/bwip-js-node.mjs",
                                "require": "./dist/bwip-js-node.js"
                            }
                        }
                    }
                }"#,
            ),
            ("node_modules/bwip-js/dist/bwip-js-node.d.ts", "export {};"),
            ("node_modules/bwip-js/dist/bwip-js-node.mjs", "export {};"),
        ]);

        let resolved = describe(
            &root,
            ImportResolver::new(&root).resolve(&path("src/barcode.ts"), "bwip-js"),
        );
        drop(guard);

        assert_eq!(
            resolved, "external node_modules/bwip-js/dist/bwip-js-node.d.ts",
            "installed, and a dependency rather than a blind spot"
        );
    }

    /// A builtin has no file at all. Reporting it as unresolved would make a
    /// rule that forbids `node:fs` impossible to write, and would drown a real
    /// missing dependency in noise.
    #[test]
    fn a_node_builtin_is_reported_as_a_builtin() {
        let (guard, root) = repo(&[("src/app.ts", "")]);
        let resolver = ImportResolver::new(&root);

        let prefixed = describe(&root, resolver.resolve(&path("src/app.ts"), "node:fs"));
        let bare = describe(&root, resolver.resolve(&path("src/app.ts"), "fs"));
        drop(guard);

        // Both forms normalise to the prefixed name, so a rule that forbids
        // `node:fs` catches the bare `fs` too without saying so twice.
        assert_eq!(prefixed, "builtin node:fs");
        assert_eq!(bare, "builtin node:fs");
    }

    /// The error names the specifier and the file that wrote it, which together
    /// are what a user needs to find the line.
    #[test]
    fn an_unresolvable_specifier_names_the_specifier_and_the_importer() {
        let (guard, root) = repo(&[("src/app.ts", "")]);
        let described = describe(
            &root,
            ImportResolver::new(&root).resolve(&path("src/app.ts"), "./nowhere"),
        );
        drop(guard);

        assert_eq!(
            described,
            "error cannot resolve `./nowhere` from `src/app.ts`"
        );
    }

    /// A file above the repository root is external even though no
    /// `node_modules` is involved: it is not a file any rule can be about.
    #[test]
    fn a_file_outside_the_root_is_external() {
        let (guard, outer) = repo(&[("shared/x.ts", TS), ("repo/src/app.ts", "")]);
        let root = outer.join("repo");

        let resolved = describe(
            &root,
            ImportResolver::new(&root).resolve(&path("src/app.ts"), "../../shared/x"),
        );
        drop(guard);

        assert_eq!(resolved, format!("external {}", outer.join("shared/x.ts")));
    }

    /// A dependency's type declarations are what a TypeScript file imports
    /// from, so `types` is consulted before `main`.
    #[test]
    fn a_dependency_resolves_through_its_types_field() {
        let (guard, root) = repo(&[
            ("src/app.ts", ""),
            (
                "node_modules/dep/package.json",
                r#"{"name":"dep","main":"dist/index.js","types":"dist/index.d.ts"}"#,
            ),
            ("node_modules/dep/dist/index.js", "module.exports = {};"),
            (
                "node_modules/dep/dist/index.d.ts",
                "export const x: number;",
            ),
        ]);

        let resolved = describe(
            &root,
            ImportResolver::new(&root).resolve(&path("src/app.ts"), "dep"),
        );
        drop(guard);

        assert_eq!(resolved, "external node_modules/dep/dist/index.d.ts");
    }

    /// An import written from a file at the repository root resolves against
    /// the root itself, rather than falling over on a missing parent.
    #[test]
    fn a_file_at_the_root_resolves_against_the_root() {
        assert_eq!(
            landed(&[("main.ts", ""), ("helper.ts", TS)], "main.ts", "./helper"),
            "in-repo helper.ts"
        );
    }
}
