// The programmatic binding: an architecture claim living in the test suite
// that a team already runs.
//
// These run against the real binary, built by `cargo build`, because a binding
// over a subprocess that never spawned one has tested nothing. `ARCHWARDEN_BIN`
// points at it; with no binary there the suite says so and skips, rather than
// failing for a reason that is not about this code.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, mkdir, rm, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { existsSync } from "node:fs";

import { check, REPORT_VERSION, ArchwardenError } from "../index.mjs";

const BINARY =
  process.env.ARCHWARDEN_BIN ??
  join(import.meta.dirname, "../../../target/debug/archwarden");

const built = existsSync(BINARY);
const needsBinary = {
  skip: built ? false : `no binary at ${BINARY} — run \`cargo build\` first`,
};

/// A repository with one module and one rule that bites.
async function repository(config) {
  const root = await mkdtemp(join(tmpdir(), "archwarden-binding-"));
  await writeFile(join(root, "arch.config.json"), config);
  await mkdir(join(root, "projetos/01-blink"), { recursive: true });
  await writeFile(join(root, "projetos/01-blink/projeto.md"), "# blink\n");
  return root;
}

const GOVERNED = JSON.stringify({
  version: 0,
  modules: [
    {
      id: "projetos",
      scope: ["projetos/*"],
      why: "one exercise per folder, and all three files in it",
      rules: [
        {
          type: "presence",
          id: "tem-os-tres",
          level: "error",
          roots: ["projetos/*"],
          require: ["projeto.md", "exercicios.md", "diagram.json"],
        },
      ],
    },
  ],
});

test("findings come back for the test framework to assert on", needsBinary, async () => {
  const root = await repository(GOVERNED);
  try {
    const report = await check({ cwd: root, binary: BINARY });

    assert.ok(Array.isArray(report.findings), "an array to assert against");
    assert.ok(report.findings.length > 0, "this repository is missing two files");
    assert.equal(report.findings[0].rule_id, "tem-os-tres");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("a repository that satisfies its rules comes back empty", needsBinary, async () => {
  const root = await repository(GOVERNED);
  try {
    await writeFile(join(root, "projetos/01-blink/exercicios.md"), "# ex\n");
    await writeFile(join(root, "projetos/01-blink/diagram.json"), "{}\n");

    const report = await check({ cwd: root, binary: BINARY });

    // The shape the issue asks for: `expect(findings).toEqual([])`.
    assert.deepEqual(report.findings, []);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("the repository's own config is the source of truth, filtered", needsBinary, async () => {
  const root = await repository(GOVERNED);
  try {
    // A rule id that exists: the test asserts a subset of the config rather
    // than restating rules inline, which would be a second place they live.
    const named = await check({ cwd: root, binary: BINARY, rules: ["tem-os-tres"] });
    assert.ok(named.findings.length > 0);

    // And a path filter, for a test about one area.
    const elsewhere = await check({
      cwd: root,
      binary: BINARY,
      paths: ["nada/**"],
    });
    assert.deepEqual(elsewhere.findings, []);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("a rule id no rule has is an error, not an empty result", needsBinary, async () => {
  const root = await repository(GOVERNED);
  try {
    await assert.rejects(
      () => check({ cwd: root, binary: BINARY, rules: ["nao-existe"] }),
      (error) => {
        assert.ok(error instanceof ArchwardenError);
        // The failure this exists to prevent: a typo in a rule id that comes
        // back as "no findings" is a test that passes for the wrong reason and
        // goes on passing after the rule is deleted.
        assert.match(error.message, /nao-existe/);
        return true;
      },
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("a config this build cannot read is an error and never an empty report", needsBinary, async () => {
  const root = await repository(JSON.stringify({ version: 99, rules: [] }));
  try {
    await assert.rejects(
      () => check({ cwd: root, binary: BINARY }),
      (error) => {
        assert.ok(error instanceof ArchwardenError);
        assert.match(error.message, /version 99/);
        return true;
      },
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("no config at all is an error rather than a clean report", needsBinary, async () => {
  const root = await mkdtemp(join(tmpdir(), "archwarden-binding-"));
  try {
    await assert.rejects(() => check({ cwd: root, binary: BINARY }), ArchwardenError);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("a report shape this binding does not understand is refused", needsBinary, async () => {
  const root = await repository(GOVERNED);
  try {
    const report = await check({ cwd: root, binary: BINARY });
    // The guard that keeps this binding from reading a future report as though
    // it were this one. Issue #55 is the same defect one layer down: a version
    // nobody checked, parsed into something that looked fine and was not.
    assert.equal(report.version, REPORT_VERSION);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("the summary comes back beside the findings", needsBinary, async () => {
  const root = await repository(GOVERNED);
  try {
    const report = await check({ cwd: root, binary: BINARY });

    assert.equal(typeof report.summary.errors, "number");
    assert.equal(typeof report.summary.warnings, "number");
    assert.equal(report.summary.errors, report.findings.length);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("a binary that is not there says so plainly", async () => {
  await assert.rejects(
    () => check({ cwd: process.cwd(), binary: "/nowhere/archwarden" }),
    (error) => {
      assert.ok(error instanceof ArchwardenError);
      assert.match(error.message, /nowhere/);
      return true;
    },
  );
});

test("every export is typed, and every declared type is exported", async () => {
  // A `.d.ts` nobody verifies is worse than none: a TypeScript suite would be
  // told a function exists that does not, or miss one that does. There is no
  // compiler in this package's gates, so the two files are checked against
  // each other instead.
  const [source, types] = await Promise.all([
    readFile(join(import.meta.dirname, "../index.mjs"), "utf8"),
    readFile(join(import.meta.dirname, "../index.d.ts"), "utf8"),
  ]);

  const exported = [...source.matchAll(/^export (?:async function|function|class|const) (\w+)/gm)]
    .map((match) => match[1])
    .sort();
  assert.deepEqual(exported, ["ArchwardenError", "REPORT_VERSION", "check"]);

  for (const name of exported) {
    assert.match(
      types,
      new RegExp(`export (?:declare )?(?:function|class|const) ${name}\\b`),
      `${name} is exported by index.mjs and not declared in index.d.ts`,
    );
  }
});

test("the package ships what it says it exports", async () => {
  const manifest = JSON.parse(
    await readFile(join(import.meta.dirname, "../package.json"), "utf8"),
  );

  // A file left out of `files` is one that is not in the published tarball,
  // and an `exports` entry pointing at it fails at the consumer's first import
  // rather than here.
  for (const path of [manifest.exports["."].default, manifest.exports["."].types]) {
    const name = path.replace("./", "");
    assert.ok(manifest.files.includes(name), `${name} is exported and not published`);
    assert.ok(
      existsSync(join(import.meta.dirname, "..", name)),
      `${name} is published and not there`,
    );
  }
});
