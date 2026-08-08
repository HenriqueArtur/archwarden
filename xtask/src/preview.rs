//! `cargo xtask preview` — the HTML reports, against a repository built to
//! show every one of them.
//!
//! The pages are for a human, and a human has to *look* at them to say whether
//! they work. Judging a page by reading its source is judging a drawing by
//! reading its coordinates, so this exists to put the real output in a browser
//! in one command.
//!
//! # It runs the real binary
//!
//! Nothing here builds a fake `Report` or a fake `Guide`. It writes a
//! repository, then runs `archwarden` against it exactly as a user would. A
//! preview assembled from hand-made data would drift from what the tool emits
//! and would be worth less than nothing — a design signed off against a page
//! the product does not produce.
//!
//! The fixture is deliberately loud: twelve modules, several walls, walls being
//! crossed, debt already accepted, an orphaned folder, a file that will not
//! parse, an import nothing can resolve, an `.astro` page and a `.md` with
//! frontmatter. Every section of every page has something in it, and the module
//! count is past the ten where the matrix has to stop being comfortable.

use std::{path::Path, process::Command};

/// Where the fixture and the pages are written.
///
/// Under `target/`, because it is a build artefact: `cargo clean` should take
/// it, and nothing here should ever be committed.
const PREVIEW: &str = "target/preview";

/// Builds the fixture, runs the real binary, and reports where the pages are.
pub(crate) fn run(root: &Path) -> Result<(), String> {
    let preview = root.join(PREVIEW);
    let repo = preview.join("repo");

    // Removed rather than merged: a stale file from a previous shape of the
    // fixture would show up in a page and be read as a bug in the renderer.
    let _ = std::fs::remove_dir_all(&preview);
    write_fixture(&repo)?;

    let guide = preview.join("guide.html");
    let report = preview.join("check.html");

    archwarden(
        root,
        &repo,
        &["agent-guide", "--format", "html"],
        Some(&guide),
    )?;
    archwarden(root, &repo, &["check", "--html", path_of(&report)?], None)?;

    println!("preview written:");
    println!("  {}", path_of(&guide)?);
    println!("  {}", path_of(&report)?);
    println!("\nopen them in a browser; re-run this after any change to the renderer.");
    Ok(())
}

/// Runs the binary against the fixture, optionally capturing stdout to a file.
fn archwarden(
    root: &Path,
    repo: &Path,
    args: &[&str],
    stdout: Option<&Path>,
) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());

    let mut command = Command::new(cargo);
    command
        .current_dir(root)
        .args(["run", "--quiet", "--bin", "archwarden", "--"])
        .args(args)
        .arg("--config")
        .arg(repo.join("arch.config.json"));

    let output = command
        .output()
        .map_err(|error| format!("cannot run archwarden: {error}"))?;

    // A build failure exits 101 and writes an empty file, which then reads as
    // a renderer bug. Stopping here is the difference between "the page is
    // blank" and "the crate does not compile".
    if output.status.code().is_some_and(|code| code > 2) {
        return Err(format!(
            "archwarden did not run:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Exit 1 is expected and wanted: the fixture violates its own rules on
    // purpose, and a preview of a clean repository would show empty sections.
    // Exit 2 is the tool failing to run, and that is worth stopping for.
    if output.status.code() == Some(2) {
        return Err(format!(
            "archwarden could not run:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    if let Some(destination) = stdout {
        std::fs::write(destination, &output.stdout)
            .map_err(|error| format!("cannot write {}: {error}", destination.display()))?;
    }

    Ok(())
}

fn path_of(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("`{}` is not UTF-8", path.display()))
}

/// Writes every file of the fixture repository.
fn write_fixture(repo: &Path) -> Result<(), String> {
    for (relative, contents) in FIXTURE {
        let path = repo.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        std::fs::write(&path, contents)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    Ok(())
}

/// A repository that exercises every section of both pages.
///
/// Written out rather than generated, because a reader comparing a page against
/// the repository that produced it needs to be able to read the repository.
const FIXTURE: &[(&str, &str)] = &[
    ("arch.config.json", CONFIG),
    (".archwarden/baseline.json", BASELINE),
    // domain — the module under most pressure
    (
        "packages/domain/src/order/calcs/total.ts",
        "import { clock } from '../../../../infrastructure/src/clock/clock';\n\
      export function total() { return clock(); }\n",
    ),
    (
        "packages/domain/src/order/calcs/total.spec.ts",
        "it('totals', () => {});\n",
    ),
    (
        "packages/domain/src/order/types/order.ts",
        "export type Order = { id: string };\n",
    ),
    (
        "packages/domain/src/invoice/actions/issue.ts",
        "import { pdf } from '../../../../infrastructure/src/pdf/pdf';\n\
      export function issue() { return pdf(); }\n",
    ),
    (
        "packages/domain/src/invoice/calcs/sum.ts",
        "export function sum() { return 1; }\n",
    ),
    (
        "packages/domain/src/billing/const/limits.ts",
        "import { env } from '../../../../../apps/api/src/env';\nexport const limits = env;\n",
    ),
    // a folder only used from outside its own module
    (
        "packages/domain/src/flow-node/shared/calcs/score.ts",
        "export function score() { return 1; }\n",
    ),
    // application
    (
        "packages/application/src/use-cases/refund/refund-order.use-case.ts",
        "export function RefundOrder() {}\n",
    ),
    (
        "packages/application/src/use-cases/refund/refund-order.use-case.spec.ts",
        "it('refunds', () => {});\n",
    ),
    // infrastructure
    (
        "packages/infrastructure/src/clock/clock.ts",
        "export const clock = () => 0;\n",
    ),
    (
        "packages/infrastructure/src/pdf/pdf.ts",
        "export const pdf = () => '';\n",
    ),
    (
        "packages/infrastructure/src/mailer/types/message.ts",
        "export type Message = { to: string };\n",
    ),
    // api — crosses a wall three times
    ("apps/api/src/env.ts", "export const env = { limit: 10 };\n"),
    (
        "apps/api/src/routes/health/route.ts",
        "import { clock } from '../../../../../packages/infrastructure/src/clock/clock';\n\
      export function route() { return clock(); }\n",
    ),
    (
        "apps/api/src/routes/export/route.ts",
        "import { generated } from '@acme/generated/schema';\nexport function route() { return generated; }\n",
    ),
    // a file that will not parse, so `checks_skipped` has something in it
    (
        "packages/shared/src/legacy/report.ts",
        "export const broken = {{{;\n",
    ),
    (
        "packages/shared/src/util/id.ts",
        "export const id = () => '1';\n",
    ),
    // the other two front-ends, so the page shows a repository that is not
    // only TypeScript
    (
        "apps/site/src/pages/blog.astro",
        "---\nimport { Order } from '../../../../packages/domain/src/order/types/order';\n---\n\n<div />\n",
    ),
    (
        "projetos/03-semaforo/projeto.md",
        "---\nid: semaforo\nnivel: 9\n---\n\n# Semáforo\n",
    ),
    ("projetos/03-semaforo/notas.md", "# Notas\n"),
];

const CONFIG: &str = r#"{
  "version": 0,
  "languages": ["ts", "astro"],
  "modules": [
    {
      "id": "domain",
      "why": "Extracted from the monolith in 2025-11 so billing could depend on it without depending on the API. Every rule under it defends that one property.",
      "rules": [
        {
          "type": "structure", "id": "domain-entity-shape", "level": "error",
          "why": "An entity that grows a folder nobody named is one nobody can find.",
          "roots": "packages/domain/src/*",
          "allowed_subfolders": ["types", "calcs", "actions", "const", "shared"]
        },
        {
          "type": "spec-pair", "id": "calcs-need-spec", "level": "warning",
          "why": "A calculation with no test is a rule nobody can change.",
          "roots": "packages/domain/src/*", "subfolders": "calcs"
        }
      ]
    },
    {
      "id": "application",
      "why": "Use cases are the only thing the API is allowed to call, so the transport can be replaced without touching a decision.",
      "rules": [
        {
          "type": "naming", "id": "usecase-name", "level": "error",
          "why": "A stack trace should name the use case, not the file.",
          "roots": "packages/application/src/use-cases/*",
          "file_pattern": "^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
          "must_export": { "kind": "function", "name": "{{pascal(name)}}" }
        }
      ]
    },
    {
      "id": "infrastructure",
      "why": "Everything that talks to the outside world, swapped per environment. Nothing above it may name a driver.",
      "rules": [
        {
          "type": "structure", "id": "infra-shape", "level": "warning",
          "roots": "packages/infrastructure/src/*",
          "allowed_subfolders": ["types"]
        }
      ]
    },
    {
      "id": "site",
      "why": "The public site. Its pages may read the domain's types and nothing else.",
      "rules": [
        {
          "type": "presence", "id": "page-needs-layout", "level": "warning",
          "why": "A page with no layout renders without the site chrome.",
          "roots": "apps/site/src/pages", "require": ["_layout.astro"]
        }
      ]
    },
    {
      "id": "licoes",
      "why": "The generated index and three scripts read the frontmatter of every lesson.",
      "rules": [
        {
          "type": "frontmatter", "id": "projeto-frontmatter", "level": "error",
          "why": "A lesson whose nivel is outside the vocabulary drops out of the index with no row and no error.",
          "roots": "projetos/*", "file_pattern": "^projeto\\.md$",
          "require": ["id", "nivel", "componentes"],
          "one_of": { "nivel": ["1", "2", "3"] },
          "equals": { "id": "{{raw(dirname)}}" }
        },
        {
          "type": "pair", "id": "licao-tem-notas", "level": "error",
          "why": "The notes file is the one an agent may read and must never write; a lesson without one gets regenerated over.",
          "roots": "projetos/*", "file_pattern": "^projeto\\.md$", "must_exist": "notas.md"
        }
      ]
    }
  ],
  "rules": [
    {
      "type": "import-boundary", "id": "domain-forbids-infrastructure", "level": "error",
      "why": "The published @acme/domain package cannot resolve a driver at build time, so an import here makes the artefact unbuildable outside this repository.",
      "from": "packages/domain/**", "forbid_import_from": ["packages/infrastructure/**"]
    },
    {
      "type": "import-boundary", "id": "domain-forbids-app", "level": "error",
      "why": "domain is published as its own package and the app is not.",
      "from": "packages/domain/**", "forbid_import_from": ["apps/**"]
    },
    {
      "type": "import-boundary", "id": "api-through-use-cases", "level": "error",
      "why": "A route that reaches a driver directly is a decision nothing can test without HTTP.",
      "from": "apps/api/**", "forbid_import_from": ["packages/infrastructure/**"]
    },
    {
      "type": "no-passthrough", "id": "no-barrel-files", "level": "warning",
      "why": "A file that only forwards another is a file every reader has to open twice.",
      "roots": "packages/**"
    }
  ]
}
"#;

/// Debt accepted when archwarden was adopted, so the page has something in its
/// accepted column and the reader can see the difference between "crossing
/// now" and "already forgiven".
const BASELINE: &str = r#"{
  "version": 0,
  "accepted": [
    { "rule": "domain-forbids-infrastructure",
      "path": "packages/domain/src/invoice/actions/issue.ts",
      "note": "imports `@acme/infra/pdf`, which resolves to `packages/infrastructure/src/pdf/pdf.ts`" },
    { "rule": "infra-shape",
      "path": "packages/infrastructure/src/clock",
      "note": "folder `clock` is not allowed here" },
    { "rule": "infra-shape",
      "path": "packages/infrastructure/src/pdf",
      "note": "folder `pdf` is not allowed here" }
  ]
}
"#;
