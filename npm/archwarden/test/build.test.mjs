// The generator for the seven platform packages.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { build, PLATFORMS, manifestFor } from "../../build.mjs";
import { packageFor } from "../resolve.mjs";

/** A release directory with a stand-in binary for every target. */
async function fakeRelease(version) {
  const dist = await mkdtemp(join(tmpdir(), "archwarden-dist-"));
  for (const platform of PLATFORMS) {
    const dir = join(dist, `archwarden-${version}-${platform.target}`);
    await mkdir(dir, { recursive: true });
    const [binary] = manifestFor(platform, version).files;
    await writeFile(join(dir, binary), "not really a binary");
  }
  return dist;
}

test("every platform the wrapper looks for is one this builds", async () => {
  // Three lists, in three files, edited by hand: the resolver's map, the
  // manifest's optionalDependencies, and this. Any two drifting means a
  // machine installs nothing, or looks for nothing.
  const built = PLATFORMS.map((p) => `@archwarden/${p.pkg}`).sort();
  const manifest = JSON.parse(
    await readFile(new URL("../package.json", import.meta.url), "utf8"),
  );

  assert.deepEqual(built, Object.keys(manifest.optionalDependencies).sort());

  for (const platform of PLATFORMS) {
    const libc = platform.libc ?? null;
    assert.equal(
      packageFor(platform.os, platform.cpu, libc),
      `@archwarden/${platform.pkg}`,
      `the resolver finds ${platform.pkg}`,
    );
  }
});

test("each package declares what it is for", () => {
  // Without `os`/`cpu`/`libc` a package manager installs all seven on every
  // machine: 30 MB to use 4.
  for (const platform of PLATFORMS) {
    const manifest = manifestFor(platform, "1.2.3");

    assert.deepEqual(manifest.os, [platform.os], platform.pkg);
    assert.deepEqual(manifest.cpu, [platform.cpu], platform.pkg);
    assert.equal(manifest.version, "1.2.3");
    if (platform.os === "linux") {
      assert.deepEqual(manifest.libc, [platform.libc], `${platform.pkg} names its libc`);
    } else {
      assert.equal(manifest.libc, undefined, `${platform.pkg} has no libc to name`);
    }
  }
});

test("only the binary is published", () => {
  // A platform package with a stray file is a platform package someone will
  // eventually import from.
  for (const platform of PLATFORMS) {
    const manifest = manifestFor(platform, "1.2.3");
    assert.equal(manifest.files.length, 1, platform.pkg);
    assert.equal(
      manifest.files[0],
      platform.os === "win32" ? "archwarden.exe" : "archwarden",
    );
  }
});

test("the main package ships exactly what its manifest promises", async () => {
  // `files` is a promise made in one file and kept in another. A name added
  // to it without a copy in `build.mjs` publishes a manifest pointing at
  // nothing; a copy without the name publishes a file nobody receives. The
  // README went missing that way once already.
  const dist = await fakeRelease("1.2.3");
  const out = await mkdtemp(join(tmpdir(), "archwarden-out-"));
  try {
    await build(dist, out, "1.2.3");

    const manifest = JSON.parse(
      await readFile(join(out, "archwarden", "package.json"), "utf8"),
    );

    // Two entries are directories; the rest are files. Reading something
    // inside either tells us it arrived, which is the whole question.
    const INSIDE = { bin: "bin/archwarden.mjs", schema: "schema/v0.json" };

    for (const entry of manifest.files) {
      const path = join(out, "archwarden", INSIDE[entry] ?? entry);
      const contents = await readFile(path, "utf8");
      assert.ok(contents.length > 0, `${entry} arrived and is not empty`);
    }
  } finally {
    await rm(dist, { recursive: true, force: true });
    await rm(out, { recursive: true, force: true });
  }
});

test("the schema travels with the package, where init expects it", async () => {
  // `init` writes `./node_modules/archwarden/schema/v0.json` into the config
  // it generates when archwarden is installed. That path is this package's
  // name plus this layout — three facts in two languages, and a config
  // pointing at a file that is not there gives an editor nothing and says
  // nothing about why.
  const manifest = JSON.parse(
    await readFile(new URL("../package.json", import.meta.url), "utf8"),
  );
  assert.equal(manifest.name, "archwarden");
  assert.ok(manifest.files.includes("schema"), "the schema is published");

  const dist = await fakeRelease("1.2.3");
  const out = await mkdtemp(join(tmpdir(), "archwarden-out-"));
  try {
    await build(dist, out, "1.2.3");

    const shipped = await readFile(
      join(out, "archwarden", "schema", "v0.json"),
      "utf8",
    );
    const source = await readFile(
      new URL("../../../schema/v0.json", import.meta.url),
      "utf8",
    );

    assert.equal(shipped, source, "shipped verbatim, not regenerated");
    // It has to be a schema, not just a file that arrived.
    assert.equal(JSON.parse(shipped).title, "Config");
  } finally {
    await rm(dist, { recursive: true, force: true });
    await rm(out, { recursive: true, force: true });
  }
});

test("the agent instructions travel with the package", async () => {
  // The file an agent is pointed at. Shipping it is the point: a repository
  // that depends on archwarden gets the instructions in `node_modules`,
  // matched to the version it installed.
  const manifest = JSON.parse(
    await readFile(new URL("../package.json", import.meta.url), "utf8"),
  );
  assert.ok(manifest.files.includes("AGENTS.md"), "AGENTS.md is published");

  const source = await readFile(new URL("../../../AGENTS.md", import.meta.url), "utf8");
  // Every command an agent is told to run has to exist. A doc that names a
  // command the binary does not have is worse than no doc: the agent will
  // try it, get exit 2, and improvise.
  for (const command of ["describe", "scaffold", "check --file", "agent-guide"]) {
    assert.ok(source.includes(command), `AGENTS.md documents \`${command}\``);
  }
});

test("both linux libcs are covered, for both architectures", () => {
  // The gap that would be easiest to leave: an Alpine CI image on arm64.
  const linux = PLATFORMS.filter((p) => p.os === "linux");
  assert.deepEqual(
    linux.map((p) => `${p.cpu}-${p.libc}`).sort(),
    ["arm64-glibc", "arm64-musl", "x64-glibc", "x64-musl"],
  );
});
