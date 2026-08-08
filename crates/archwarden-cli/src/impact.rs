//! `archwarden impact <from> --to <to>` — what a move would change.
//!
//! The question a refactor asks before it starts, and the one archwarden is
//! uniquely placed to answer: an editor moves a file and rewrites its imports,
//! but says nothing about whether the destination is somewhere the
//! architecture allows the file to be, or whether the move puts an existing
//! import across a boundary.
//!
//! It answers three things and admits a fourth:
//!
//! - which rules would start and stop applying,
//! - which files import it, and which of those imports would newly be
//!   forbidden,
//! - how many of its own relative imports would need rewriting,
//! - and which files contain a dynamic import nothing here can read.
//!
//! That last one is not decoration. `import(name)` names no module, so a file
//! containing one may or may not import the target and this cannot tell. A
//! report that left it out would be confident and wrong, which is worse than
//! incomplete and honest — and it is the precondition for a future `move` that
//! rewrites imports, which must refuse rather than half-do.
//!
//! # What this deliberately does not answer
//!
//! Whether the move breaks the TypeScript build. Relative imports are counted,
//! not resolved-after-the-fact, because `tsc` already answers that question
//! better than archwarden ever will. The half worth having here is the
//! architectural one nobody else answers.

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRuleKind},
    path::RepoRelPath,
};
use serde::Serialize;

/// The version of the `impact` JSON shape.
pub const IMPACT_VERSION: u32 = 0;

/// One importer, and whether the move would put its import out of bounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Importer {
    /// The file that imports the target.
    pub path: String,
    /// Rules that forbid the destination and did not forbid the source.
    ///
    /// Empty for an importer the move does not affect, which is most of them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub newly_forbidden_by: Vec<String>,
}

/// What moving a file would change.
#[derive(Debug, Clone, Serialize)]
pub struct Impact {
    version: u32,
    from: String,
    to: String,
    /// Rules that apply at the destination and not at the source.
    starts_applying: Vec<String>,
    /// Rules that apply at the source and not at the destination.
    stops_applying: Vec<String>,
    /// Every file that imports the target, worst first.
    importers: Vec<Importer>,
    /// How many of the file's own imports are relative and would move with it.
    relative_imports: usize,
    /// Files with a dynamic import this cannot read.
    opaque: Vec<String>,
}

impl Impact {
    /// Whether anything here needs a human to look at it.
    #[must_use]
    pub fn needs_attention(&self) -> bool {
        !self.opaque.is_empty()
            || self
                .importers
                .iter()
                .any(|importer| !importer.newly_forbidden_by.is_empty())
    }
}

/// Works out what moving `from` to `to` would change.
#[must_use]
pub fn impact(
    config: &CompiledConfig,
    from: &RepoRelPath,
    to: &RepoRelPath,
    importers: &archwarden_engine::importers::Importers,
    relative_imports: usize,
) -> Impact {
    let at = |path: &RepoRelPath| -> Vec<String> {
        crate::describe::describe(config, path)
            .into_iter()
            .map(|applies| applies.rule.id.as_str().to_owned())
            .collect()
    };
    let (here, there) = (at(from), at(to));

    let listed: Vec<Importer> = importers
        .direct
        .iter()
        .map(|importer| Importer {
            path: importer.path.as_str().to_owned(),
            newly_forbidden_by: newly_forbidden(config, &importer.path, from, to),
        })
        .collect();

    // Worst first: an importer the move breaks is what the reader came for,
    // and burying it under thirty that are fine would be the same mistake the
    // unfiltered report made.
    let mut importers_out = listed;
    importers_out.sort_by(|a, b| {
        b.newly_forbidden_by
            .len()
            .cmp(&a.newly_forbidden_by.len())
            .then_with(|| a.path.cmp(&b.path))
    });

    Impact {
        version: IMPACT_VERSION,
        from: from.as_str().to_owned(),
        to: to.as_str().to_owned(),
        starts_applying: there
            .iter()
            .filter(|id| !here.contains(id))
            .cloned()
            .collect(),
        stops_applying: here
            .iter()
            .filter(|id| !there.contains(id))
            .cloned()
            .collect(),
        importers: importers_out,
        relative_imports,
        opaque: importers
            .opaque
            .iter()
            .map(|path| path.as_str().to_owned())
            .collect(),
    }
}

/// Rules that would forbid `importer`'s import once the target moves.
///
/// Newly, not merely: a boundary that already forbids the import is a finding
/// `check` reports today and not something this move causes. Reporting it here
/// would put existing debt in a list of consequences.
fn newly_forbidden(
    config: &CompiledConfig,
    importer: &RepoRelPath,
    from: &RepoRelPath,
    to: &RepoRelPath,
) -> Vec<String> {
    config
        .rules()
        .filter(|rule| rule.applies_to_file(importer))
        .filter_map(|rule| {
            let CompiledRuleKind::ImportBoundary { forbid, except, .. } = &rule.kind else {
                return None;
            };
            let bans = |path: &RepoRelPath| {
                forbid.is_match(path.as_path()) && !except.is_match(path.as_path())
            };
            (bans(to) && !bans(from)).then(|| rule.id.as_str().to_owned())
        })
        .collect()
}

/// Writes the impact in the requested format.
pub fn render(impact: &Impact, format: crate::report::Format, out: &mut dyn std::io::Write) {
    match format {
        crate::report::Format::Json => match serde_json::to_string_pretty(impact) {
            Ok(json) => {
                let _ = writeln!(out, "{json}");
            }
            Err(error) => {
                let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
            }
        },
        crate::report::Format::Text => render_text(impact, out),
    }
}

/// Writes a batch of impacts, which for one file is the single report.
///
/// A batch is not a different shape, only more of the same one: a reader
/// planning nine moves wants each of them, and a summary that collapsed them
/// would hide the one that breaks something.
pub fn render_all(impacts: &[Impact], format: crate::report::Format, out: &mut dyn std::io::Write) {
    match (format, impacts) {
        (_, [one]) => render(one, format, out),
        (crate::report::Format::Json, many) => match serde_json::to_string_pretty(many) {
            Ok(json) => {
                let _ = writeln!(out, "{json}");
            }
            Err(error) => {
                let _ = writeln!(out, r#"{{"error":"cannot serialise: {error}"}}"#);
            }
        },
        (crate::report::Format::Text, many) => {
            let _ = writeln!(
                out,
                "{} files would move.\n",
                if many.is_empty() { 0 } else { many.len() }
            );
            for impact in many {
                render_text(impact, out);
            }
        }
    }
}

fn render_text(impact: &Impact, out: &mut dyn std::io::Write) {
    let _ = writeln!(out, "Moving `{}` to `{}`:\n", impact.from, impact.to);

    list(
        out,
        "Rules that would start applying",
        &impact.starts_applying,
    );
    list(
        out,
        "Rules that would stop applying",
        &impact.stops_applying,
    );

    let breaking = impact
        .importers
        .iter()
        .filter(|importer| !importer.newly_forbidden_by.is_empty())
        .count();

    if impact.importers.is_empty() {
        let _ = writeln!(out, "  Nothing imports it.\n");
    } else {
        let _ = writeln!(
            out,
            "  {} {} it, {} of which would newly cross a boundary:",
            impact.importers.len(),
            if impact.importers.len() == 1 {
                "file imports"
            } else {
                "files import"
            },
            breaking
        );
        for importer in &impact.importers {
            if importer.newly_forbidden_by.is_empty() {
                let _ = writeln!(out, "    {}", importer.path);
            } else {
                let _ = writeln!(
                    out,
                    "    {} — {}",
                    importer.path,
                    importer.newly_forbidden_by.join(", ")
                );
            }
        }
        let _ = writeln!(out);
    }

    if impact.relative_imports > 0 {
        let _ = writeln!(
            out,
            "  {} relative {} in the file itself would need rewriting.\n",
            impact.relative_imports,
            if impact.relative_imports == 1 {
                "import"
            } else {
                "imports"
            }
        );
    }

    // Last, and never omitted when it applies: it is the sentence that says
    // the rest of this report is incomplete.
    if !impact.opaque.is_empty() {
        let _ = writeln!(
            out,
            "  {} {} a dynamic import this cannot read. Check {} by hand:",
            impact.opaque.len(),
            if impact.opaque.len() == 1 {
                "file has"
            } else {
                "files have"
            },
            if impact.opaque.len() == 1 {
                "it"
            } else {
                "them"
            }
        );
        for path in &impact.opaque {
            let _ = writeln!(out, "    {path}");
        }
        let _ = writeln!(out);
    }
}

fn list(out: &mut dyn std::io::Write, heading: &str, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let _ = writeln!(out, "  {heading}:");
    for id in ids {
        let _ = writeln!(out, "    {id}");
    }
    let _ = writeln!(out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_core::{
        compiled::{CompiledRule, SkipDirs},
        glob::PathSet,
        hash::ContentHash,
        level::Level,
        scope::Scope,
    };
    use archwarden_engine::importers::Importers;

    fn path(p: &str) -> RepoRelPath {
        RepoRelPath::new(p).expect("valid path")
    }

    fn boundary(id: &str, from: &str, forbid: &str) -> CompiledRule {
        CompiledRule {
            id: archwarden_core::ids::RuleId::new(id).expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile([from]).expect("valid scope"),
            kind: CompiledRuleKind::ImportBoundary {
                forbid: PathSet::compile([forbid.to_owned()]).expect("valid glob"),
                require: PathSet::default(),
                forbid_packages: Vec::new(),
                except: PathSet::default(),
                except_from: PathSet::default(),
                include_type_only: true,
            },
        }
    }

    fn structure(id: &str, roots: &str) -> CompiledRule {
        CompiledRule {
            id: archwarden_core::ids::RuleId::new(id).expect("valid id"),
            module: None,
            level: Level::Error,
            scope: Scope::compile([roots]).expect("valid scope"),
            kind: CompiledRuleKind::Structure {
                allowed_subfolders: Some(vec!["types".to_owned()]),
                warn_subfolders: Vec::new(),
                recurse_into: Vec::new(),
                filename_patterns: Vec::new(),
            },
        }
    }

    fn config(rules: Vec<CompiledRule>) -> CompiledConfig {
        CompiledConfig::new(
            rules,
            PathSet::default(),
            SkipDirs::default(),
            ContentHash::of(b""),
        )
    }

    fn found(direct: &[&str], opaque: &[&str]) -> Importers {
        Importers {
            direct: direct
                .iter()
                .map(|p| archwarden_engine::importers::Importer {
                    path: path(p),
                    imports: Vec::new(),
                })
                .collect(),
            opaque: opaque.iter().map(|p| path(p)).collect(),
            unresolved_local: Vec::new(),
        }
    }

    /// The answer nobody else gives: this move puts an existing import across
    /// a boundary. An editor rewrites the specifier and says nothing.
    #[test]
    fn an_import_that_would_newly_cross_a_boundary_is_named() {
        let config = config(vec![boundary(
            "domain-forbids-app",
            "packages/domain/**",
            "packages/app/**",
        )]);

        let result = impact(
            &config,
            &path("packages/domain/src/order/x.ts"),
            &path("packages/app/src/order/x.ts"),
            &found(&["packages/domain/src/invoice/y.ts"], &[]),
            0,
        );

        assert_eq!(
            result.importers,
            [Importer {
                path: "packages/domain/src/invoice/y.ts".to_owned(),
                newly_forbidden_by: vec!["domain-forbids-app".to_owned()],
            }]
        );
        assert!(result.needs_attention());
    }

    /// Newly, not merely. A boundary that already forbids the import is debt
    /// `check` reports today, and putting it in a list of consequences would
    /// blame this move for something it did not do.
    #[test]
    fn a_boundary_that_was_already_crossed_is_not_a_consequence() {
        let config = config(vec![boundary(
            "domain-forbids-everything",
            "packages/domain/**",
            "packages/**",
        )]);

        let result = impact(
            &config,
            &path("packages/domain/src/order/x.ts"),
            &path("packages/app/src/order/x.ts"),
            &found(&["packages/domain/src/invoice/y.ts"], &[]),
            0,
        );

        assert_eq!(result.importers[0].newly_forbidden_by, Vec::<String>::new());
        assert!(!result.needs_attention());
    }

    /// The other half archwarden is uniquely placed to answer: the
    /// destination has different obligations.
    #[test]
    fn rules_that_start_and_stop_applying_are_listed() {
        let config = config(vec![
            structure("domain-shape", "packages/domain/src/*"),
            structure("app-shape", "packages/app/src/*"),
        ]);

        let result = impact(
            &config,
            &path("packages/domain/src/order"),
            &path("packages/app/src/order"),
            &found(&[], &[]),
            0,
        );

        assert_eq!(result.starts_applying, ["app-shape"]);
        assert_eq!(result.stops_applying, ["domain-shape"]);
    }

    /// An importer the move does not affect still appears -- moving a file
    /// touches every one of them, and a list of "who will have to be edited"
    /// is what someone plans a refactor from.
    #[test]
    fn every_importer_is_listed_worst_first() {
        let config = config(vec![boundary(
            "domain-forbids-app",
            "packages/domain/**",
            "packages/app/**",
        )]);

        let result = impact(
            &config,
            &path("packages/domain/src/order/x.ts"),
            &path("packages/app/src/order/x.ts"),
            &found(
                &[
                    "apps/web/a.ts",
                    "packages/domain/src/invoice/y.ts",
                    "apps/web/b.ts",
                ],
                &[],
            ),
            0,
        );

        assert_eq!(result.importers.len(), 3);
        assert_eq!(
            result.importers[0].path, "packages/domain/src/invoice/y.ts",
            "the one that breaks comes first"
        );
    }

    /// The sentence that says the rest of the report is incomplete.
    #[test]
    fn an_opaque_file_makes_the_answer_need_attention() {
        let result = impact(
            &config(Vec::new()),
            &path("src/a.ts"),
            &path("src/b/a.ts"),
            &found(&[], &["src/loader.ts"]),
            0,
        );

        assert_eq!(result.opaque, ["src/loader.ts"]);
        assert!(
            result.needs_attention(),
            "a blind spot is not a clean answer"
        );
    }

    /// A move nothing imports and no rule changes is the one that is safe, and
    /// says so plainly rather than printing empty headings.
    #[test]
    fn a_move_that_changes_nothing_reads_as_nothing() {
        let mut out = Vec::new();
        render(
            &impact(
                &config(Vec::new()),
                &path("src/a.ts"),
                &path("src/b/a.ts"),
                &found(&[], &[]),
                0,
            ),
            crate::report::Format::Text,
            &mut out,
        );
        let text = String::from_utf8(out).expect("output is UTF-8");

        assert_eq!(
            text,
            "Moving `src/a.ts` to `src/b/a.ts`:\n\n  Nothing imports it.\n\n"
        );
    }

    /// The whole report, as a reader sees it.
    #[test]
    fn the_text_format_reads_as_intended() {
        let config = config(vec![
            boundary(
                "domain-forbids-app",
                "packages/domain/**",
                "packages/app/**",
            ),
            structure("app-shape", "packages/app/src/*"),
        ]);
        let mut out = Vec::new();
        render(
            &impact(
                &config,
                &path("packages/domain/src/order/x.ts"),
                &path("packages/app/src/order/x.ts"),
                &found(&["packages/domain/src/invoice/y.ts"], &["src/loader.ts"]),
                2,
            ),
            crate::report::Format::Text,
            &mut out,
        );
        let text = String::from_utf8(out).expect("output is UTF-8");

        assert_eq!(
            text,
            "Moving `packages/domain/src/order/x.ts` to `packages/app/src/order/x.ts`:\n\
             \n\
             \x20 Rules that would stop applying:\n\
             \x20   domain-forbids-app\n\
             \n\
             \x20 1 file imports it, 1 of which would newly cross a boundary:\n\
             \x20   packages/domain/src/invoice/y.ts — domain-forbids-app\n\
             \n\
             \x20 2 relative imports in the file itself would need rewriting.\n\
             \n\
             \x20 1 file has a dynamic import this cannot read. Check it by hand:\n\
             \x20   src/loader.ts\n\
             \n"
        );
    }

    /// The JSON keeps every list, because a tool driving a refactor wants the
    /// paths and not the prose.
    #[test]
    fn the_json_carries_every_list() {
        let config = config(vec![boundary(
            "domain-forbids-app",
            "packages/domain/**",
            "packages/app/**",
        )]);
        let mut out = Vec::new();
        render(
            &impact(
                &config,
                &path("packages/domain/src/order/x.ts"),
                &path("packages/app/src/order/x.ts"),
                &found(&["packages/domain/src/invoice/y.ts"], &["src/loader.ts"]),
                2,
            ),
            crate::report::Format::Json,
            &mut out,
        );
        let parsed: serde_json::Value = serde_json::from_slice(&out).expect("valid JSON");

        assert_eq!(parsed["version"], 0);
        assert_eq!(parsed["from"], "packages/domain/src/order/x.ts");
        assert_eq!(parsed["to"], "packages/app/src/order/x.ts");
        assert_eq!(
            parsed["importers"][0]["newly_forbidden_by"][0],
            "domain-forbids-app"
        );
        assert_eq!(parsed["relative_imports"], 2);
        assert_eq!(parsed["opaque"][0], "src/loader.ts");
    }
}
