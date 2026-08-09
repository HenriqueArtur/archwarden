//! Which module may import which, as a grid.
//!
//! The one picture only archwarden can draw. An import graph is what a dozen
//! tools already produce; *intent overlaid with reality* — the walls, and the
//! arrows going through them — needs both halves, and only this tool has both.
//!
//! # It infers nothing
//!
//! A cell is decided by asking the **same matchers the engines use** against the
//! directories the walk actually found: the boundary rule's own `from` scope and
//! its own `forbid` globs. Comparing globs against globs would have been
//! cheaper and would have been a second implementation of what a boundary means
//! — and a matrix that quietly disagreed with `check` is worse than no matrix.
//!
//! Crossings are counted from the findings of that same run, so a number in a
//! cell is a finding a reader can go and look at.
//!
//! # Why a grid rather than a graph
//!
//! A node-link diagram cannot show the pair with *no* edge, and "may `domain`
//! import `shared`?" is a question about a pair. A grid answers every pair,
//! including the ones nothing connects, and it needs no layout engine — so the
//! page stays a single file with no script in it.

use archwarden_core::{
    compiled::{CompiledConfig, CompiledRuleKind},
    finding::{Finding, Observed},
    path::RepoRelPath,
};
use archwarden_engine::walk::RepoTree;

/// What one module may do to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// The module itself.
    Self_,
    /// No rule forbids it.
    Allowed,
    /// A wall. The design working, not a problem.
    Forbidden,
    /// A wall, and this many imports are going through it right now.
    Crossed(usize),
}

/// One module, as the grid knows it.
#[derive(Debug, Clone)]
pub struct Module {
    /// The id from the config.
    pub id: String,
    /// Why it exists, when its author said.
    pub why: Option<String>,
    /// The globs its rules govern, for the reader to recognise it by.
    pub scopes: Vec<String>,
    /// How many files sit inside it.
    pub files: usize,
    /// Findings reported against those files.
    pub errors: usize,
    /// Warnings reported against those files.
    pub warnings: usize,
}

/// The grid, plus the modules that label it.
#[derive(Debug, Clone)]
pub struct Matrix {
    /// In configuration order, which is the order a reader wrote them in.
    pub modules: Vec<Module>,
    /// `rows[importer][imported]`.
    pub rows: Vec<Vec<Cell>>,
    /// Which rule draws each wall, for the legend under the plate.
    pub walls: Vec<Wall>,
}

/// A wall, with what is happening to it.
#[derive(Debug, Clone)]
pub struct Wall {
    /// The rule that draws it.
    pub rule_id: String,
    /// Why it exists.
    pub why: Option<String>,
    /// `domain`, the importer side.
    pub from: String,
    /// `infrastructure`, the imported side.
    pub to: String,
    /// The imports going through it now, importer first.
    pub crossings: Vec<(RepoRelPath, String)>,
}

impl Matrix {
    /// Builds the grid for a walked repository.
    #[must_use]
    pub fn of(config: &CompiledConfig, tree: &RepoTree, findings: &[Finding]) -> Self {
        let modules = modules_of(config, tree, findings);
        let boundaries = boundaries_of(config);

        let mut rows = Vec::with_capacity(modules.len());
        let mut walls: Vec<Wall> = Vec::new();

        for (row, importer) in modules.iter().enumerate() {
            let mut cells = Vec::with_capacity(modules.len());

            for (column, imported) in modules.iter().enumerate() {
                if row == column {
                    cells.push(Cell::Self_);
                    continue;
                }

                let Some(boundary) = boundaries
                    .iter()
                    .find(|boundary| boundary.forbids(&importer.id, &imported.id, tree, config))
                else {
                    cells.push(Cell::Allowed);
                    continue;
                };

                let crossings = crossings_between(findings, importer, imported, tree, config);
                cells.push(if crossings.is_empty() {
                    Cell::Forbidden
                } else {
                    Cell::Crossed(crossings.len())
                });

                walls.push(Wall {
                    rule_id: boundary.id.clone(),
                    why: boundary.why.clone(),
                    from: importer.id.clone(),
                    to: imported.id.clone(),
                    crossings,
                });
            }

            rows.push(cells);
        }

        // Worst first: a wall being crossed is what the reader came for, and a
        // wall that is holding is context.
        walls.sort_by_key(|wall| {
            (
                std::cmp::Reverse(wall.crossings.len()),
                wall.rule_id.clone(),
            )
        });

        Self {
            modules,
            rows,
            walls,
        }
    }
}

/// A boundary rule, reduced to the question the grid asks it.
struct Boundary {
    id: String,
    why: Option<String>,
    scope: archwarden_core::scope::Scope,
    forbid: archwarden_core::glob::PathSet,
}

impl Boundary {
    /// Whether this rule forbids `from` importing `to`.
    ///
    /// Asked of real paths, through the rule's own matchers. A module the walk
    /// found no file in cannot answer, and is treated as unwalled rather than
    /// as walled — claiming a wall nobody could verify is the wrong way to be
    /// wrong on a page somebody plans a refactor from.
    fn forbids(&self, from: &str, to: &str, tree: &RepoTree, config: &CompiledConfig) -> bool {
        let importing = files_of(from, tree, config).into_iter().any(|file| {
            file.parent()
                .is_some_and(|dir| self.scope.matches_dir(dir.as_path()))
        });

        let imported = files_of(to, tree, config)
            .into_iter()
            .any(|file| self.forbid.is_match(file.as_path()));

        importing && imported
    }
}

fn boundaries_of(config: &CompiledConfig) -> Vec<Boundary> {
    config
        .rules()
        .filter_map(|rule| {
            let CompiledRuleKind::ImportBoundary { forbid, .. } = &rule.kind else {
                return None;
            };
            Some(Boundary {
                id: rule.id.as_str().to_owned(),
                why: rule.why.clone(),
                scope: rule.scope.clone(),
                forbid: forbid.clone(),
            })
        })
        .collect()
}

/// The modules a configuration declares, in the order it declares them.
fn modules_of(config: &CompiledConfig, tree: &RepoTree, findings: &[Finding]) -> Vec<Module> {
    let mut order: Vec<String> = Vec::new();
    let mut scopes: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut whys: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();

    for rule in config.rules() {
        let Some(module) = rule.module.as_ref() else {
            continue;
        };
        let id = module.as_str().to_owned();
        if !order.contains(&id) {
            order.push(id.clone());
        }
        let patterns = scopes.entry(id.clone()).or_default();
        for pattern in rule.scope.patterns() {
            if !patterns.contains(pattern) {
                patterns.push(pattern.clone());
            }
        }
        if let Some(why) = &rule.module_why {
            whys.entry(id).or_insert_with(|| why.clone());
        }
    }

    order
        .into_iter()
        .map(|id| {
            let files = files_of(&id, tree, config);
            let (errors, warnings) = counts_for(&files, findings);
            Module {
                why: whys.get(&id).cloned(),
                scopes: scopes.get(&id).cloned().unwrap_or_default(),
                files: files.len(),
                errors,
                warnings,
                id,
            }
        })
        .collect()
}

/// Every file the walk found inside a module, and inside anything under it.
///
/// A module is a *subtree*, not a level. `roots: "packages/domain/src/*"`
/// selects the entity directories, and the files that make an entity live one
/// or two levels below them -- so taking only the files directly inside a
/// selected directory finds almost nothing, which is what the first version of
/// this did: every cell came back allowed and the grid was empty and confident.
///
/// The rules themselves do not need this, because each one is handed the
/// directory it is about. The grid does, because it is asking "what is inside
/// this module" rather than "does this rule apply here".
fn files_of(module: &str, tree: &RepoTree, config: &CompiledConfig) -> Vec<RepoRelPath> {
    let scopes: Vec<&archwarden_core::scope::Scope> = config
        .rules()
        .filter(|rule| rule.module.as_ref().is_some_and(|id| id.as_str() == module))
        .map(|rule| &rule.scope)
        .collect();

    let roots: Vec<RepoRelPath> = tree
        .directories()
        .filter(|(path, _)| scopes.iter().any(|scope| scope.matches_dir(path.as_path())))
        .map(|(path, _)| path.clone())
        .collect();

    tree.directories()
        .filter(|(path, _)| {
            roots
                .iter()
                .any(|root| path == &root || under(path.as_str(), root.as_str()))
        })
        .flat_map(|(_, directory)| directory.files.iter().map(|file| file.path.clone()))
        .collect()
}

/// Whether `path` sits under `root`, by whole segments.
///
/// `starts_with` alone would put `packages/domain-legacy` inside
/// `packages/domain`.
fn under(path: &str, root: &str) -> bool {
    path.strip_prefix(root)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn counts_for(files: &[RepoRelPath], findings: &[Finding]) -> (usize, usize) {
    findings
        .iter()
        .filter(|finding| files.iter().any(|file| file == &finding.path))
        .fold((0, 0), |(errors, warnings), finding| {
            if finding.level.fails_build() {
                (errors + 1, warnings)
            } else {
                (errors, warnings + 1)
            }
        })
}

/// The imports crossing from one module into another, from the run's findings.
fn crossings_between(
    findings: &[Finding],
    from: &Module,
    to: &Module,
    tree: &RepoTree,
    config: &CompiledConfig,
) -> Vec<(RepoRelPath, String)> {
    let importing = files_of(&from.id, tree, config);
    let imported = files_of(&to.id, tree, config);

    findings
        .iter()
        .filter_map(|finding| {
            let Observed::ForbiddenImport {
                specifier,
                resolved,
            } = &finding.observed
            else {
                return None;
            };
            let crosses = importing.iter().any(|file| file == &finding.path)
                && imported.iter().any(|file| file == resolved);

            crosses.then(|| (finding.path.clone(), specifier.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use archwarden_config::{compile::compile, discovery::load_from, extends::merge};
    use archwarden_resolver::preset::PresetResolver;
    use camino::Utf8PathBuf;

    /// Builds a repository on disk, compiles its config, and runs a real check.
    ///
    /// The whole point of the grid is that it agrees with `check`, so a test
    /// that fed it hand-made findings would be testing the wrong thing.
    fn against(entries: &[(&str, &str)]) -> (tempfile::TempDir, Matrix) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().expect("canonicalise"))
            .expect("temp path is UTF-8");

        for (relative, contents) in entries {
            let file = root.join(relative);
            std::fs::create_dir_all(file.parent().expect("a file has a parent"))
                .expect("create dirs");
            std::fs::write(&file, contents).expect("write file");
        }

        let loaded = load_from(&root).expect("the config loads");
        let merged = merge(loaded, &PresetResolver::new()).expect("merges");
        let config = compile(&merged).expect("compiles");
        let tree = archwarden_engine::walk::walk(&root, &config).expect("walks");
        let report = archwarden_engine::run::check(archwarden_engine::run::Run {
            root: &root,
            config: &config,
            tree: &tree,
            cache: None,
        });

        let matrix = Matrix::of(&config, &tree, &report.findings);
        (dir, matrix)
    }

    const CONFIG: &str = r#"{
      "version": 0,
      "modules": [
        {"id":"domain","rules":[
          {"type":"structure","id":"domain-shape","level":"error",
           "roots":"packages/domain/src/*","allowed_subfolders":["calcs","types"]}]},
        {"id":"infra","rules":[
          {"type":"structure","id":"infra-shape","level":"warning",
           "roots":"packages/infra/src/*","allowed_subfolders":["clock"]}]}
      ],
      "rules": [
        {"type":"import-boundary","id":"domain-forbids-infra","level":"error",
         "why":"the published package cannot resolve a driver",
         "from":"packages/domain/**","forbid_import_from":["packages/infra/**"]}
      ]
    }"#;

    /// A module is a *subtree*, not a level. `roots: "packages/domain/src/*"`
    /// selects the entity directories and the files live one or two levels
    /// below them, so taking only the files directly inside a selected
    /// directory finds almost nothing.
    ///
    /// The first version of this did exactly that: every cell came back
    /// allowed, and the grid was empty and confident — which on a page somebody
    /// plans a refactor from is the worst way to be wrong.
    #[test]
    fn a_modules_files_are_the_whole_subtree_under_its_scopes() {
        let (_guard, matrix) = against(&[
            ("arch.config.json", CONFIG),
            (
                "packages/domain/src/order/calcs/total.ts",
                "export const total = 1;\n",
            ),
            (
                "packages/infra/src/clock/clock.ts",
                "export const clock = 1;\n",
            ),
        ]);

        let domain = matrix
            .modules
            .iter()
            .find(|module| module.id == "domain")
            .expect("domain is a module");

        assert_eq!(
            domain.files, 1,
            "the file sits two levels below the scope and is still the module's"
        );
    }

    /// A wall is drawn by asking the rule's own matchers, so the grid cannot
    /// disagree with `check` about where one is.
    #[test]
    fn a_boundary_rule_draws_a_wall_between_the_two_modules() {
        let (_guard, matrix) = against(&[
            ("arch.config.json", CONFIG),
            (
                "packages/domain/src/order/calcs/total.ts",
                "export const total = 1;\n",
            ),
            (
                "packages/infra/src/clock/clock.ts",
                "export const clock = 1;\n",
            ),
        ]);

        let domain = index_of(&matrix, "domain");
        let infra = index_of(&matrix, "infra");

        assert_eq!(cell(&matrix, domain, infra), Cell::Forbidden);
        assert_eq!(
            cell(&matrix, infra, domain),
            Cell::Allowed,
            "the rule is one-directional and so is the grid"
        );
        assert_eq!(cell(&matrix, domain, domain), Cell::Self_);
    }

    /// And a crossing is counted from the findings of that same run, so a
    /// number in a cell is a finding a reader can go and look at.
    #[test]
    fn an_import_through_the_wall_is_counted_in_the_cell() {
        let (_guard, matrix) = against(&[
            ("arch.config.json", CONFIG),
            (
                "packages/domain/src/order/calcs/total.ts",
                "import { clock } from '../../../../infra/src/clock/clock';\nexport const total = clock;\n",
            ),
            (
                "packages/infra/src/clock/clock.ts",
                "export const clock = 1;\n",
            ),
        ]);

        let domain = index_of(&matrix, "domain");
        let infra = index_of(&matrix, "infra");

        assert_eq!(cell(&matrix, domain, infra), Cell::Crossed(1));

        let wall = matrix
            .walls
            .iter()
            .find(|wall| wall.from == "domain" && wall.to == "infra")
            .expect("the wall is listed");
        assert_eq!(wall.rule_id, "domain-forbids-infra");
        assert_eq!(
            wall.why.as_deref(),
            Some("the published package cannot resolve a driver"),
            "the reason travels with the wall"
        );
        assert_eq!(wall.crossings.len(), 1);
    }

    /// A module the walk found no file in cannot answer whether a rule reaches
    /// it, and is left unwalled rather than walled. Claiming a wall nobody
    /// could verify is the wrong way to be wrong here.
    #[test]
    fn a_module_with_no_files_is_not_given_a_wall() {
        let (_guard, matrix) = against(&[
            ("arch.config.json", CONFIG),
            (
                "packages/domain/src/order/calcs/total.ts",
                "export const total = 1;\n",
            ),
        ]);

        let domain = index_of(&matrix, "domain");
        let infra = index_of(&matrix, "infra");

        assert_eq!(cell(&matrix, domain, infra), Cell::Allowed);
    }

    /// `starts_with` alone would put `packages/domain-legacy` inside
    /// `packages/domain`.
    #[test]
    fn a_sibling_whose_name_begins_the_same_is_not_inside() {
        assert!(under("packages/domain/src", "packages/domain"));
        assert!(!under("packages/domain-legacy/src", "packages/domain"));
        assert!(!under("packages/domain", "packages/domain"));
    }

    fn index_of(matrix: &Matrix, id: &str) -> usize {
        matrix
            .modules
            .iter()
            .position(|module| module.id == id)
            .expect("the module is in the grid")
    }

    fn cell(matrix: &Matrix, row: usize, column: usize) -> Cell {
        *matrix
            .rows
            .get(row)
            .and_then(|cells| cells.get(column))
            .expect("the grid is square")
    }
}
