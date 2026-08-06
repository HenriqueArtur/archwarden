//! Rewriting one import specifier for a file that moved.
//!
//! The mechanical half of a move, and the half an editor already does — badly,
//! in a monorepo. An editor rewrites the relative specifiers it can see and
//! leaves `@org/domain/email/x` alone, because to it that is a package name
//! like `react`. In a repository where half the imports are written that way,
//! that is half a refactor.
//!
//! Everything here is a pure function of strings and paths. It touches no
//! file, so every case below is a unit test rather than a fixture — which
//! matters, because the cases are the whole feature and getting one wrong
//! silently produces a repository that does not build.
//!
//! # What it refuses
//!
//! A specifier it cannot recompute comes back as [`Rewrite::Unknown`], and the
//! caller must refuse the whole move rather than rewrite the rest — a
//! half-rewritten repository is worse than one that was never touched.
//!
//! It carries [`Unknown`], which says *which* of four reasons, because they are
//! four different files to go and look at: a `tsconfig` whose `paths` this does
//! not read, a `package.json` whose `exports` does not reach the destination,
//! the destination package's manifest, or a file at the repository root. They
//! were one message until issue #11 turned up a repository that hit the
//! `exports` case and was told to check its `tsconfig`.

use archwarden_core::path::RepoRelPath;
use archwarden_resolver::workspace::{Package, Workspace};

/// What should replace a specifier, or why nothing can.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rewrite {
    /// The specifier still means the same file. Nothing to write.
    Unchanged,
    /// Replace it with this.
    To(String),
    /// Nothing can, and here is which of the reasons it is.
    ///
    /// The caller must refuse either way, so carrying the reason changes no
    /// decision — it changes what the refusal *says*, and that is the whole
    /// point. These four are four different files to go and look at, and a
    /// message that names the wrong one costs more time than the check saves.
    Unknown(Unknown),
}

/// Why a specifier could not be recomputed.
///
/// Four causes, and they were one message until issue #11 turned up a
/// repository that hit the fourth and was told to check the third.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unknown {
    /// The specifier resolves into the repository through a `tsconfig` path
    /// alias, which is read forwards and cannot be written backwards.
    ///
    /// It is not relative and names no workspace package, yet it reached a file
    /// here, so something else mapped it. `compilerOptions.paths` is that
    /// something in every case seen so far — and archwarden does read it, which
    /// is how this variant is reached at all.
    ///
    /// What it cannot do is invert the map. Given a destination, which alias to
    /// write is not a question `paths` answers: several patterns may reach the
    /// same file, and none may reach the new location. Guessing produces an
    /// import that does not compile.
    PathAlias,
    /// The importing file is at the repository root, so a relative specifier
    /// has no directory to be measured from.
    NoImporterDirectory,
    /// The target is leaving the package whose name the specifier uses.
    ///
    /// Which package it should name instead is a question about the
    /// destination's manifest rather than this one's, and guessing would
    /// produce an import that does not resolve.
    LeavesThePackage,
    /// The package's `exports` covers no subpath that reaches the destination.
    ///
    /// The move is legal and the file lands somewhere real; there is simply no
    /// specifier the package's own manifest would let an importer write for it.
    /// Reached when `exports` is absent, or its patterns do not match where the
    /// file is going.
    NotExported,
}

impl Unknown {
    /// What to look at, in one sentence.
    ///
    /// Written as an instruction rather than a diagnosis: whoever reads this is
    /// mid-refactor and wants the next step, not a classification.
    ///
    /// Wrapped with the two-space continuation the rest of the refusals use, so
    /// a long reason does not run off the side of a terminal the others fit in.
    #[must_use]
    pub fn explain(self) -> &'static str {
        match self {
            Self::PathAlias => {
                "it resolves through a `tsconfig` path alias, and which alias\n  \
                 to write for the new location is not something that map says"
            }
            Self::NoImporterDirectory => {
                "the importing file is at the repository root, so a relative\n  \
                 specifier has no directory to be measured from"
            }
            Self::LeavesThePackage => {
                "the file is leaving the package that specifier names, and\n  \
                 which package offers it next is a question about the\n  \
                 destination's `package.json`"
            }
            Self::NotExported => {
                "the package's `exports` reaches no subpath at the destination,\n  \
                 so there is no specifier an importer could write for it --\n  \
                 add one, or land the file where `exports` already reaches"
            }
        }
    }
}

/// The new specifier for one import, after a move.
///
/// Two things can have changed, independently, and the answer depends on both:
///
/// - **the importing file moved**, so a relative specifier now measures a
///   different distance;
/// - **the imported file moved**, so a specifier that names it has to name it
///   somewhere else.
///
/// A batch move makes both happen at once — a file in the moved set that
/// imports another file in the moved set — so they are one question here
/// rather than two functions a caller has to pick between.
///
/// `new_home` is where the importing file will live, and `new_target` where
/// the imported one will. `target_moved` says whether the second actually
/// changed, which is what decides a non-relative specifier: a package name
/// means the same thing from anywhere, so it survives the importer moving and
/// must be recomputed when the target moves.
///
/// [`Rewrite::Unknown`] is a refusal the caller must honour by abandoning the
/// whole move: leaving such a specifier alone would silently point it at a file
/// that is no longer there. The [`Unknown`] it carries says which of the four
/// reasons it is, and each one names a different thing to go and fix.
#[must_use]
pub fn respecify(
    specifier: &str,
    new_home: &RepoRelPath,
    new_target: &RepoRelPath,
    target_moved: bool,
    workspace: &Workspace,
) -> Rewrite {
    if specifier.starts_with('.') {
        return relative(specifier, new_home, new_target);
    }
    if !target_moved {
        // A package name, a builtin and an alias all mean the same thing from
        // anywhere in the repository, so the importer moving does not touch
        // them. The exception — a move across a package boundary, where a
        // different `tsconfig` applies — is reported by the caller rather than
        // guessed at here.
        return Rewrite::Unchanged;
    }
    match workspace.package_for(specifier) {
        Some(package) => by_package(specifier, package, new_target),
        None => Rewrite::Unknown(Unknown::PathAlias),
    }
}

/// A relative specifier, recomputed from the importer's directory.
fn relative(specifier: &str, importer: &RepoRelPath, to: &RepoRelPath) -> Rewrite {
    let Some(directory) = importer.parent() else {
        return Rewrite::Unknown(Unknown::NoImporterDirectory);
    };

    // The extension the author wrote, kept. A repository on TypeScript's ESM
    // convention writes `./user.js` and means `./user.ts`; rewriting that to
    // `./user` would resolve under a bundler and break under `node --strip
    // types`. Whatever suffix was there before goes back.
    let suffix = written_extension(specifier);
    let stem = strip_extension(to.as_str());
    let target = format!("{stem}{suffix}");

    let rewritten = relative_path(directory.as_str(), &target);
    if rewritten == specifier {
        Rewrite::Unchanged
    } else {
        Rewrite::To(rewritten)
    }
}

/// The extension the author wrote on a specifier, if any.
///
/// Only a suffix that is plausibly one: a specifier ending in `.js` has an
/// extension, and `../v1.2/thing` does not. The list is the one the resolver
/// tries, which is what makes the two agree about what an extension is.
fn written_extension(specifier: &str) -> &'static str {
    const EXTENSIONS: [&str; 9] = [
        ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".json",
    ];
    EXTENSIONS
        .into_iter()
        .find(|extension| specifier.ends_with(extension))
        .unwrap_or("")
}

/// A path with its final extension removed.
fn strip_extension(path: &str) -> &str {
    match path.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.contains('/') => stem,
        _ => path,
    }
}

/// The `./`-anchored path from `directory` to `target`, both repo-relative.
///
/// Always anchored: a bare `user` is a package specifier to every resolver in
/// the ecosystem, so a sibling has to be written `./user`.
fn relative_path(directory: &str, target: &str) -> String {
    let from: Vec<&str> = directory.split('/').filter(|s| !s.is_empty()).collect();
    let into: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();

    let shared = from.iter().zip(&into).take_while(|(a, b)| a == b).count();

    let mut parts: Vec<&str> = vec![".."; from.len().saturating_sub(shared)];
    parts.extend(into.iter().skip(shared));

    match parts.first() {
        Some(&"..") => parts.join("/"),
        _ => format!("./{}", parts.join("/")),
    }
}

/// A specifier written against a workspace package, recomputed through the
/// package's own `exports` map.
///
/// The map is read backwards here: forwards it says `./email/*` lands at
/// `./src/email/*.ts`, and the question is which subpath now lands at the
/// file's new home. A file moved out of every exported subpath has no
/// specifier that reaches it, which is [`Rewrite::Unknown`] — the move is
/// legal and the import is not, and only a human can decide which to change.
fn by_package(specifier: &str, package: &Package, to: &RepoRelPath) -> Rewrite {
    let Ok(inside) = to.as_path().strip_prefix(package.directory.as_path()) else {
        // Moved out of the package. The specifier has to name a different
        // package, and which one is a question about the destination's
        // manifest rather than this one's.
        return Rewrite::Unknown(Unknown::LeavesThePackage);
    };
    let inside = inside.as_str();

    for (subpath, target) in &package.subpaths {
        let Some(subpath) = subpath.strip_prefix("./") else {
            continue;
        };
        let target = target.trim_start_matches("./");

        let Some((prefix, suffix)) = target.split_once('*') else {
            // A literal subpath: it names one file, and it names this one or
            // it does not.
            if target == inside {
                let rewritten = format!("{}/{subpath}", package.name);
                return settle(specifier, rewritten);
            }
            continue;
        };
        let Some(star) = inside
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(suffix))
        else {
            continue;
        };

        let tail = subpath.replacen('*', star, 1);
        return settle(specifier, format!("{}/{tail}", package.name));
    }

    Rewrite::Unknown(Unknown::NotExported)
}

fn settle(specifier: &str, rewritten: String) -> Rewrite {
    if rewritten == specifier {
        Rewrite::Unchanged
    } else {
        Rewrite::To(rewritten)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// A workspace with one package shaped like the real one this was built
    /// against: every entity is its own `exports` subpath pattern.
    fn domain() -> Workspace {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("UTF-8");
        std::fs::create_dir_all(root.join("packages/domain")).expect("dirs");
        std::fs::write(
            root.join("packages/domain/package.json"),
            r#"{"name":"@flowmaatik/domain","exports":{
                 "./email/*":"./src/email/*.ts",
                 "./id/*":"./src/id/*.ts",
                 "./feature/*":"./src/feature/*.ts"}}"#,
        )
        .expect("write");
        let workspace = Workspace::discover(&root);
        drop(dir);
        workspace
    }

    /// An import that resolves to the moved file, read from an importer that
    /// itself stayed put.
    fn rewrite(specifier: &str, importer: &str, to: &str) -> Rewrite {
        respecify(specifier, &path(importer), &path(to), true, &domain())
    }

    /// One of the moved file's own imports: the importer moved, the target
    /// did not.
    fn from_moved(specifier: &str, new_home: &str, target: &str) -> Rewrite {
        respecify(specifier, &path(new_home), &path(target), false, &domain())
    }

    /// Case 1, and the majority of a real monorepo's imports: written by
    /// package name, not relatively. An editor leaves this one alone because
    /// it cannot tell it from `react`.
    #[test]
    fn a_package_specifier_follows_the_file_through_the_exports_map() {
        assert_eq!(
            rewrite(
                "@flowmaatik/domain/email/shared/is-email-invalid-shared",
                "apps/app/src/admin/use-new-admin-user-page.ts",
                "packages/domain/src/email/calcs/is-email-invalid.ts",
            ),
            Rewrite::To("@flowmaatik/domain/email/calcs/is-email-invalid".to_owned())
        );
    }

    /// Case 2: relative, within one entity.
    #[test]
    fn a_relative_specifier_is_recomputed_from_the_importers_directory() {
        assert_eq!(
            rewrite(
                "../shared/types/feature-shared",
                "packages/domain/src/feature/types/feature.ts",
                "packages/domain/src/feature/types/feature-shared.ts",
            ),
            Rewrite::To("./feature-shared".to_owned())
        );
    }

    /// Case 3, read from the other side: the moved file's own imports. The
    /// importer is the file at its *new* home, and what changes is the
    /// distance from there to something that did not move.
    ///
    /// `organization/shared/calcs/slugify-tag.ts` imports
    /// `../../calcs/validate-tag`. Moved to `organization/calcs/`, that is
    /// `./validate-tag`.
    #[test]
    fn the_moved_files_own_relative_imports_are_recomputed_from_its_new_home() {
        assert_eq!(
            from_moved(
                "../../calcs/validate-tag",
                "packages/domain/src/organization/calcs/slugify-tag.ts",
                "packages/domain/src/organization/calcs/validate-tag.ts",
            ),
            Rewrite::To("./validate-tag".to_owned())
        );
    }

    /// A sibling is `./x`, never a bare `x`: every resolver in the ecosystem
    /// reads the second as a package name.
    #[test]
    fn a_sibling_keeps_its_leading_dot_slash() {
        assert_eq!(
            rewrite("../calcs/thing", "src/a/b/importer.ts", "src/a/b/thing.ts"),
            Rewrite::To("./thing".to_owned())
        );
    }

    /// TypeScript's ESM convention: `./user.js` in a `.ts` file means
    /// `./user.ts`. Dropping the suffix resolves under a bundler and breaks
    /// under Node, so whatever the author wrote goes back.
    #[test]
    fn an_explicit_extension_survives_the_rewrite() {
        assert_eq!(
            rewrite("../shared/thing.js", "src/a/importer.ts", "src/b/thing.ts"),
            Rewrite::To("../b/thing.js".to_owned())
        );
    }

    /// A specifier already pointing at the destination is left alone, so a
    /// rewrite never produces a diff that changes nothing.
    #[test]
    fn a_specifier_that_already_points_at_the_destination_is_unchanged() {
        assert_eq!(
            rewrite("./thing", "src/a/importer.ts", "src/a/thing.ts"),
            Rewrite::Unchanged
        );
    }

    /// The moved file keeps every specifier that does not point at something
    /// relative to it: a package name, a builtin and an alias all mean the
    /// same thing from anywhere.
    #[test]
    fn the_moved_files_non_relative_imports_are_left_alone() {
        for specifier in ["react", "node:fs", "@types/node", "@Components/button"] {
            assert_eq!(
                from_moved(specifier, "src/b/thing.ts", "src/other.ts"),
                Rewrite::Unchanged,
                "{specifier}"
            );
        }
    }

    /// The refusal that keeps a half-rewritten repository from happening.
    ///
    /// This specifier *resolved to the file being moved* — that is the
    /// contract of `for_importer` — so leaving it alone would leave it
    /// pointing at a file that is no longer there. A `tsconfig` path alias
    /// reaches this: it resolves through `compilerOptions.paths`, a map this
    /// does not read. Refusing the whole move is the only safe answer.
    #[test]
    fn an_alias_that_resolved_into_the_repository_is_refused_rather_than_guessed() {
        for specifier in ["@Components/button", "~/components/button", "#internal/x"] {
            assert_eq!(
                rewrite(specifier, "src/a.ts", "src/b/thing.ts"),
                Rewrite::Unknown(Unknown::PathAlias),
                "{specifier}"
            );
        }
    }

    /// A file moved outside every exported subpath has no specifier that
    /// reaches it. Inventing one would write an import that does not resolve.
    #[test]
    fn a_move_out_of_the_exports_map_is_refused() {
        assert_eq!(
            rewrite(
                "@flowmaatik/domain/email/shared/x",
                "apps/app/src/main.ts",
                "packages/domain/src/internal/x.ts",
            ),
            Rewrite::Unknown(Unknown::NotExported),
            "the file lands somewhere real; `exports` simply offers no \
             specifier that reaches it"
        );
    }

    /// And a move out of the package entirely: which package now offers it is
    /// a question about the destination's manifest, not this one's.
    #[test]
    fn a_move_to_another_package_is_refused() {
        assert_eq!(
            rewrite(
                "@flowmaatik/domain/email/shared/x",
                "apps/app/src/main.ts",
                "packages/system/src/x.ts",
            ),
            Rewrite::Unknown(Unknown::LeavesThePackage)
        );
    }

    /// Case 5's mechanical half: the destination carries a different
    /// filename, and the specifier follows it. Renaming the exported symbol
    /// is a separate decision and this does not make it.
    #[test]
    fn a_rename_in_the_middle_of_a_move_follows_the_filename() {
        assert_eq!(
            rewrite(
                "@flowmaatik/domain/id/shared/is-id-invalid-shared",
                "packages/domain/src/user/calcs/is-user-create-data-invalid.ts",
                "packages/domain/src/id/calcs/is-id-invalid.ts",
            ),
            Rewrite::To("@flowmaatik/domain/id/calcs/is-id-invalid".to_owned())
        );
    }

    /// Up and over, which is the shape of a move between entities.
    #[test]
    fn a_path_that_goes_up_before_it_goes_down_keeps_its_dot_dots() {
        assert_eq!(
            rewrite(
                "./old",
                "packages/domain/src/a/x.ts",
                "packages/domain/src/b/c/old.ts"
            ),
            Rewrite::To("../b/c/old".to_owned())
        );
    }

    /// Each reason has to send the reader somewhere different, or splitting the
    /// enum bought nothing. The assertion is on the file each sentence names,
    /// because that is what the reader goes and opens.
    #[test]
    fn every_reason_names_the_thing_to_go_and_look_at() {
        for (reason, must_name) in [
            (Unknown::PathAlias, "tsconfig"),
            (Unknown::NoImporterDirectory, "repository root"),
            (Unknown::LeavesThePackage, "package.json"),
            (Unknown::NotExported, "exports"),
        ] {
            let sentence = reason.explain();
            assert!(
                sentence.contains(must_name),
                "{reason:?} should point at `{must_name}`: {sentence}"
            );
        }

        // And the two that are most easily confused stay distinguishable: the
        // repository this came from hit `NotExported` and was told to check its
        // `tsconfig`. Issue #11.
        assert_ne!(Unknown::NotExported.explain(), Unknown::PathAlias.explain());
        assert!(
            !Unknown::NotExported.explain().contains("tsconfig"),
            "an `exports` problem must not send anyone to the `tsconfig`"
        );
    }
}
