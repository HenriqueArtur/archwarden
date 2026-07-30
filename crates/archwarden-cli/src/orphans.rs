//! `archwarden orphans` — which direction a folder's imports come from.
//!
//! For every file: who imports it, and from where. From inside the module it
//! lives in, from outside it, or nobody. Aggregated by folder.
//!
//! # The question it answers
//!
//! **Does this folder have a reason to exist?** — which is the question at the
//! centre of any refactor of a layer, and one nothing else asks.
//!
//! - A folder whose files are imported **only from outside** is a boundary
//!   drawn in the wrong place: nothing in the module it sits in needs it, so
//!   it belongs to its callers rather than to its parent.
//! - A folder whose files are imported **only from inside** is a folder that
//!   should be private. It is part of how the module works, not of what it
//!   offers.
//! - A folder whose files are imported **by nobody** is dead, or reached only
//!   through a dynamic import nothing here can read.
//!
//! The answer took nine hand-written greps the last time somebody needed it,
//! and the graph is already resolved — the information existed and was not
//! exposed.
//!
//! # This is not Knip
//!
//! Knip finds exports nobody uses. The interest here is **where the importers
//! come from** for the exports that *are* used, which is a different question
//! with a different answer.
//!
//! One column does overlap: a file nothing imports is a file Knip would also
//! report. The other two are the ones this exists for, and they are the ones
//! that say whether a folder is a boundary, a private detail, or a mistake.
//!
//! # Specs are left out of the graph, both ways
//!
//! A spec is an entry point for a test runner: nothing imports it, by design.
//! Counting it as an importee puts a phantom dead file in every folder in the
//! repository.
//!
//! Counting it as an *importer* is worse, and it is the one that had to be
//! measured to be believed. A file's own spec sits in the same module as the
//! file, so it lands in the "inside" column — and every file with a spec then
//! reads as used from inside *and* outside, which is the answer that says
//! nothing. On a real repository that turned six `shared/` folders, all of
//! them used only by other modules, into six folders marked "both".
//!
//! So a spec is neither. It is not architecture; it is how the architecture is
//! tested.

use archwarden_core::{compiled::CompiledConfig, path::RepoRelPath, scope::Scope};
use serde::Serialize;

/// The version of the `orphans` JSON shape.
pub const ORPHANS_VERSION: u32 = 0;

/// Where a file's importers sit, relative to the module the file is in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Direction {
    /// Importers inside the same module.
    pub inside: usize,
    /// Importers outside it.
    pub outside: usize,
}

impl Direction {
    /// Whether nobody imports the file at all.
    #[must_use]
    pub fn is_unimported(self) -> bool {
        self.inside == 0 && self.outside == 0
    }
}

/// One file, and where its importers come from.
#[derive(Debug, Clone, Serialize)]
pub struct FileRow {
    /// The file.
    pub path: String,
    /// The module it belongs to.
    pub module: String,
    /// Importers, by direction.
    #[serde(flatten)]
    pub direction: Direction,
}

/// One folder, and where its files' importers come from.
#[derive(Debug, Clone, Serialize)]
pub struct FolderRow {
    /// The folder.
    pub path: String,
    /// How many source files are in it, directly.
    pub files: usize,
    /// Files imported only from inside their module.
    pub inside_only: usize,
    /// Files imported only from outside it.
    pub outside_only: usize,
    /// Files imported from both.
    pub both: usize,
    /// Files nobody imports.
    pub unimported: usize,
    /// What the shape of the folder says about it, or `None` when it says
    /// nothing worth a sentence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<&'static str>,
}

/// The whole answer.
#[derive(Debug, Clone, Serialize)]
pub struct Orphans {
    version: u32,
    /// Folders, worst first.
    pub folders: Vec<FolderRow>,
    /// Files, in path order. Only under `--by file`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileRow>>,
    /// Files with a dynamic import nothing can read.
    ///
    /// The same admission `impact` makes, and for the same reason: `import(name)`
    /// names no module, so a file containing one may import anything. A folder
    /// reported as unimported may simply be reached this way.
    pub opaque: Vec<String>,
}

impl Orphans {
    /// Keeps only what is under `scope`.
    ///
    /// Applied after the fact rather than before, deliberately: "inside" and
    /// "outside" are about the module a file belongs to, and narrowing the
    /// graph first would call every importer outside the scope an outsider —
    /// which would make the answer depend on what you asked about.
    pub fn retain(&mut self, scope: &archwarden_core::glob::PathSet) {
        self.folders
            .retain(|folder| scope.is_match(camino::Utf8Path::new(&folder.path)));
        if let Some(files) = &mut self.files {
            files.retain(|file| scope.is_match(camino::Utf8Path::new(&file.path)));
        }
    }
}

/// Works out where every file's importers sit.
///
/// `modules` are the directories the configuration's own rule scopes select —
/// the same areas `check --by path` counts by, so nothing here has to choose a
/// depth the config did not already declare.
#[must_use]
pub fn orphans(
    config: &CompiledConfig,
    index: &archwarden_engine::importers::ReverseIndex,
    by_file: bool,
) -> Orphans {
    let scopes: Vec<Scope> = config.rules().map(|rule| rule.scope.clone()).collect();
    let markers = crate::batch::spec_markers(config);

    let mut files: Vec<FileRow> = Vec::new();
    for (path, importers) in index.entries() {
        if is_spec(path, &markers) {
            continue;
        }
        let module = module_of(&scopes, path);
        let mut direction = Direction::default();
        for importer in importers {
            if is_spec(importer, &markers) {
                continue;
            }
            if module_of(&scopes, importer) == module {
                direction.inside += 1;
            } else {
                direction.outside += 1;
            }
        }
        files.push(FileRow {
            path: path.as_str().to_owned(),
            module,
            direction,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    Orphans {
        version: ORPHANS_VERSION,
        folders: fold(&files),
        files: by_file.then_some(files),
        opaque: index
            .opaque()
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect(),
    }
}

/// Whether a path is a spec, by the markers the configuration uses.
///
/// The marker has to be the last stem component, which is the same rule
/// `spec-pair` applies: `user.spec.ts` is a spec and `user.spec.helper.ts` is a
/// helper that happens to mention one.
fn is_spec(path: &RepoRelPath, markers: &[String]) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let Some((stem, _extension)) = name.rsplit_once('.') else {
        return false;
    };
    markers
        .iter()
        .any(|marker| stem == marker || stem.ends_with(&format!(".{marker}")))
}

/// The nearest ancestor of `path` that some rule's scope selects.
///
/// The same rule `check --by path` uses, deliberately: a config saying
/// `roots: packages/domain/src/*` has already declared that
/// `packages/domain/src/order` is a unit, and inventing a second notion of
/// "module" here would let the two disagree about the same repository.
///
/// A file no scope reaches falls back to its own directory, which is an
/// honest answer rather than a heading that means nothing.
fn module_of(scopes: &[Scope], path: &RepoRelPath) -> String {
    let mut candidate = path.parent();

    while let Some(directory) = candidate {
        if !directory.as_str().is_empty()
            && scopes
                .iter()
                .any(|scope| scope.matches_dir(directory.as_path()))
        {
            return directory.as_str().to_owned();
        }
        candidate = directory.parent();
    }

    path.parent()
        .map_or_else(|| path.as_str().to_owned(), |p| p.as_str().to_owned())
}

/// Groups files by the folder they sit in directly.
fn fold(files: &[FileRow]) -> Vec<FolderRow> {
    let mut by_folder: std::collections::BTreeMap<String, FolderRow> =
        std::collections::BTreeMap::new();

    for file in files {
        let folder = file
            .path
            .rsplit_once('/')
            .map_or_else(|| ".".to_owned(), |(directory, _)| directory.to_owned());

        let row = by_folder.entry(folder.clone()).or_insert(FolderRow {
            path: folder,
            files: 0,
            inside_only: 0,
            outside_only: 0,
            both: 0,
            unimported: 0,
            verdict: None,
        });

        row.files += 1;
        match (file.direction.inside, file.direction.outside) {
            (0, 0) => row.unimported += 1,
            (0, _) => row.outside_only += 1,
            (_, 0) => row.inside_only += 1,
            _ => row.both += 1,
        }
    }

    let mut folders: Vec<FolderRow> = by_folder.into_values().collect();
    for folder in &mut folders {
        folder.verdict = verdict(folder);
    }

    // Worst first, where "worst" is "most likely to be the folder you are
    // looking for": dead files, then a boundary in the wrong place, then a
    // folder that should be private.
    folders.sort_by(|a, b| {
        b.unimported
            .cmp(&a.unimported)
            .then_with(|| b.outside_only.cmp(&a.outside_only))
            .then_with(|| a.path.cmp(&b.path))
    });
    folders
}

/// What a folder's shape says about it.
///
/// Only when every file agrees. A folder that is half one thing and half
/// another is a folder nobody has decided about, and a sentence claiming
/// otherwise would be the tool guessing.
fn verdict(folder: &FolderRow) -> Option<&'static str> {
    if folder.files == 0 {
        return None;
    }
    if folder.unimported == folder.files {
        return Some("nothing imports any of it");
    }
    if folder.outside_only == folder.files {
        return Some("only used from outside its module — the boundary is drawn elsewhere");
    }
    if folder.inside_only == folder.files {
        return Some("only used from inside its module — this could be private");
    }
    None
}

/// Writes the answer in the requested format.
pub fn render(
    orphans: &Orphans,
    by_file: bool,
    format: crate::report::Format,
    out: &mut dyn std::io::Write,
) {
    match format {
        crate::report::Format::Json => match serde_json::to_string_pretty(orphans) {
            Ok(json) => {
                let _ = writeln!(out, "{json}");
            }
            Err(error) => {
                let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
            }
        },
        crate::report::Format::Text => render_text(orphans, by_file, out),
    }
}

fn render_text(orphans: &Orphans, by_file: bool, out: &mut dyn std::io::Write) {
    if orphans.folders.is_empty() {
        let _ = writeln!(out, "No source files here.");
        return;
    }

    if by_file && let Some(files) = &orphans.files {
        {
            let width = files.iter().map(|f| f.path.len()).max().unwrap_or(0);
            for file in files {
                let _ = writeln!(
                    out,
                    "{:width$}  inside {}  outside {}",
                    file.path, file.direction.inside, file.direction.outside
                );
            }
            let _ = writeln!(out);
        }
    }

    let width = orphans
        .folders
        .iter()
        .map(|f| f.path.len())
        .max()
        .unwrap_or(0);
    for folder in &orphans.folders {
        let _ = writeln!(
            out,
            "{:width$}  {} {}   inside-only {}   outside-only {}   both {}   nobody {}",
            folder.path,
            folder.files,
            if folder.files == 1 { "file " } else { "files" },
            folder.inside_only,
            folder.outside_only,
            folder.both,
            folder.unimported,
        );
        if let Some(verdict) = folder.verdict {
            let _ = writeln!(out, "{:width$}  → {verdict}", "");
        }
    }

    // Last, and never omitted: it is the sentence that says a folder listed
    // above as unimported may not be.
    if !orphans.opaque.is_empty() {
        let _ = writeln!(
            out,
            "\n{} {} a dynamic import this cannot read, so a folder above may be reached\nfrom {} without showing it:",
            orphans.opaque.len(),
            if orphans.opaque.len() == 1 {
                "file has"
            } else {
                "files have"
            },
            if orphans.opaque.len() == 1 {
                "it"
            } else {
                "them"
            },
        );
        for path in &orphans.opaque {
            let _ = writeln!(out, "  {path}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{compiled::SkipDirs, glob::PathSet, hash::ContentHash};
    use archwarden_engine::importers::ReverseIndex;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    /// A config whose scopes make each entity under `src` a module, which is
    /// the layout the areas are meant to come from.
    fn config() -> CompiledConfig {
        let rule = archwarden_core::compiled::CompiledRule {
            id: archwarden_core::ids::RuleId::new("shape").expect("valid id"),
            module: None,
            level: archwarden_core::level::Level::Error,
            scope: Scope::compile(["packages/domain/src/*"]).expect("valid scope"),
            kind: archwarden_core::compiled::CompiledRuleKind::Structure {
                allowed_subfolders: Vec::new(),
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                filename_patterns: Vec::new(),
            },
        };
        CompiledConfig::new(
            vec![rule],
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b""),
        )
    }

    fn index(entries: &[(&str, &[&str])], opaque: &[&str]) -> ReverseIndex {
        ReverseIndex::from_pairs(
            entries
                .iter()
                .map(|(file, importers)| (path(file), importers.iter().map(|i| path(i)).collect()))
                .collect(),
            opaque.iter().map(|p| path(p)).collect(),
        )
    }

    /// The verdict a badly-drawn boundary gets. Every file in the folder is
    /// used only by other modules, so nothing in the module it sits in needs
    /// it — it belongs to its callers.
    #[test]
    fn a_folder_used_only_from_outside_is_named_as_a_misplaced_boundary() {
        let found = orphans(
            &config(),
            &index(
                &[(
                    "packages/domain/src/email/shared/is-email-invalid.ts",
                    &["packages/domain/src/user/calcs/x.ts", "apps/web/y.ts"],
                )],
                &[],
            ),
            false,
        );

        let folder = &found.folders[0];
        assert_eq!(folder.outside_only, 1);
        assert_eq!(folder.inside_only, 0);
        assert_eq!(
            folder.verdict,
            Some("only used from outside its module — the boundary is drawn elsewhere")
        );
    }

    /// The mirror verdict: everything in the folder is used only by its own
    /// module, so it is a private detail wearing a public name.
    #[test]
    fn a_folder_used_only_from_inside_is_named_as_private() {
        let found = orphans(
            &config(),
            &index(
                &[(
                    "packages/domain/src/email/calcs/normalise.ts",
                    &["packages/domain/src/email/actions/send.ts"],
                )],
                &[],
            ),
            false,
        );

        assert_eq!(
            found.folders[0].verdict,
            Some("only used from inside its module — this could be private")
        );
    }

    /// The one column Knip would also report, kept because a refactor needs
    /// all three and splitting them across two tools helps nobody.
    #[test]
    fn a_file_nobody_imports_is_counted_as_such() {
        let found = orphans(
            &config(),
            &index(&[("packages/domain/src/email/calcs/dead.ts", &[])], &[]),
            false,
        );

        assert_eq!(found.folders[0].unimported, 1);
        assert_eq!(found.folders[0].verdict, Some("nothing imports any of it"));
    }

    /// A folder half one thing and half another is a folder nobody has decided
    /// about. A sentence claiming otherwise would be the tool guessing.
    #[test]
    fn a_mixed_folder_gets_no_verdict() {
        let found = orphans(
            &config(),
            &index(
                &[
                    (
                        "packages/domain/src/email/calcs/a.ts",
                        &["packages/domain/src/email/actions/x.ts"],
                    ),
                    ("packages/domain/src/email/calcs/b.ts", &["apps/web/y.ts"]),
                ],
                &[],
            ),
            false,
        );

        assert_eq!(found.folders[0].files, 2);
        assert_eq!(found.folders[0].inside_only, 1);
        assert_eq!(found.folders[0].outside_only, 1);
        assert_eq!(found.folders[0].verdict, None);
    }

    /// "Inside" means the module, not the folder. A `shared/` and a `calcs/`
    /// under the same entity are one module, which is what makes the
    /// inside/outside split say something about the boundary rather than
    /// about the directory tree.
    #[test]
    fn a_sibling_folder_in_the_same_module_counts_as_inside() {
        let found = orphans(
            &config(),
            &index(
                &[(
                    "packages/domain/src/organization/shared/calcs/slugify-tag.ts",
                    &["packages/domain/src/organization/calcs/validate-tag.ts"],
                )],
                &[],
            ),
            true,
        );

        let files = found.files.expect("by file");
        assert_eq!(files[0].module, "packages/domain/src/organization");
        assert_eq!(files[0].direction.inside, 1);
        assert_eq!(files[0].direction.outside, 0);
    }

    /// The blind spot, said out loud. A folder listed as unimported may be
    /// reached through a dynamic import nothing here can read.
    #[test]
    fn the_dynamic_import_blind_spot_is_reported() {
        let found = orphans(
            &config(),
            &index(
                &[("packages/domain/src/email/calcs/x.ts", &[])],
                &["scripts/loader.ts"],
            ),
            false,
        );

        assert_eq!(found.opaque, ["scripts/loader.ts"]);

        let mut out = Vec::new();
        render(&found, false, crate::report::Format::Text, &mut out);
        let text = String::from_utf8(out).expect("UTF-8");
        assert!(text.contains("scripts/loader.ts"), "{text}");
        assert!(text.contains("dynamic import"), "{text}");
    }

    /// The measurement that forced specs out of the graph.
    ///
    /// A file's own spec sits in the same module as the file, so counting it
    /// puts every file with a spec in the "inside" column — and a folder used
    /// only by other modules then reads as "both", which is the answer that
    /// says nothing. On a real repository that turned six `shared/` folders,
    /// every one of them used only from outside, into six folders marked
    /// "both".
    #[test]
    fn a_files_own_spec_does_not_make_it_look_used_from_inside() {
        let found = orphans(
            &config(),
            &index(
                &[
                    (
                        "packages/domain/src/email/shared/is-email-invalid.ts",
                        &[
                            "packages/domain/src/email/shared/is-email-invalid.spec.ts",
                            "packages/domain/src/user/calcs/x.ts",
                        ],
                    ),
                    (
                        "packages/domain/src/email/shared/is-email-invalid.spec.ts",
                        &[],
                    ),
                ],
                &[],
            ),
            true,
        );

        assert_eq!(
            found.folders[0].outside_only, 1,
            "the spec is not an importer: {:?}",
            found.folders[0]
        );
        assert_eq!(found.folders[0].files, 1, "the spec is not a row either");
        assert_eq!(
            found.folders[0].verdict,
            Some("only used from outside its module — the boundary is drawn elsewhere")
        );
        assert_eq!(
            found.files.expect("by file").len(),
            1,
            "a spec is an entry point for a runner, not a file nobody imports"
        );
    }

    /// Worst first: dead files, then a misplaced boundary, then the rest.
    #[test]
    fn folders_are_ordered_worst_first() {
        let found = orphans(
            &config(),
            &index(
                &[
                    (
                        "packages/domain/src/a/calcs/used.ts",
                        &["packages/domain/src/a/actions/x.ts"],
                    ),
                    ("packages/domain/src/b/calcs/dead.ts", &[]),
                    (
                        "packages/domain/src/c/calcs/external.ts",
                        &["apps/web/y.ts"],
                    ),
                ],
                &[],
            ),
            false,
        );

        let order: Vec<&str> = found.folders.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            order,
            [
                "packages/domain/src/b/calcs",
                "packages/domain/src/c/calcs",
                "packages/domain/src/a/calcs",
            ]
        );
    }
}
