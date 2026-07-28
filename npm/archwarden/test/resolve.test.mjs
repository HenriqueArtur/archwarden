// The logic that decides which binary to run. Everything else in the wrapper
// is `spawnSync`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import {
  binaryName,
  missingPackageMessage,
  packageFor,
  specifierFor,
  unsupportedMessage,
} from "../resolve.mjs";

test("every published platform maps to its package", () => {
  assert.equal(packageFor("darwin", "arm64"), "@archwarden/cli-darwin-arm64");
  assert.equal(packageFor("darwin", "x64"), "@archwarden/cli-darwin-x64");
  assert.equal(packageFor("win32", "x64"), "@archwarden/cli-win32-x64");
  assert.equal(packageFor("linux", "x64"), "@archwarden/cli-linux-x64");
  assert.equal(packageFor("linux", "arm64"), "@archwarden/cli-linux-arm64");
});

test("one linux package per architecture, whatever C library is installed", () => {
  // The Linux binaries are statically linked against musl, so there is nothing
  // to detect and nothing to choose. 0.3.0 shipped a glibc build that required
  // 2.39 and would not start on Debian 12 — a floor nobody had chosen, moving
  // with whatever the runner had that month. A static binary has no floor.
  assert.equal(packageFor("linux", "arm64"), "@archwarden/cli-linux-arm64");
  assert.equal(packageFor("linux", "x64"), "@archwarden/cli-linux-x64");

  // And no `-musl` package is reachable, because there is no longer one to
  // reach: a name that resolved to nothing would fail at first run.
  for (const platform of ["darwin", "linux", "win32"]) {
    for (const arch of ["arm64", "x64"]) {
      const pkg = packageFor(platform, arch);
      assert.ok(!pkg?.endsWith("-musl"), `${platform}-${arch} -> ${pkg}`);
    }
  }
});

test("the map covers exactly what package.json declares", async () => {
  // The two lists are edited by hand in different files. If they drift, the
  // manager installs a package nothing looks for, or nothing looks for a
  // package that was never installed -- and both fail at first run.
  const manifest = JSON.parse(
    await readFile(new URL("../package.json", import.meta.url), "utf8"),
  );
  const declared = Object.keys(manifest.optionalDependencies).sort();

  const reachable = new Set();
  for (const platform of ["darwin", "linux", "win32"]) {
    for (const arch of ["arm64", "x64"]) {
      const pkg = packageFor(platform, arch);
      if (pkg) reachable.add(pkg);
    }
  }

  assert.deepEqual([...reachable].sort(), declared);
});

test("a platform we do not publish for gets null, not a guess", () => {
  for (const [platform, arch] of [
    ["freebsd", "x64"],
    ["linux", "ia32"],
    ["win32", "arm64"],
    ["darwin", "ppc64"],
  ]) {
    assert.equal(packageFor(platform, arch), null, `${platform}-${arch}`);
    assert.equal(specifierFor(platform, arch), null);
  }
});

test("the binary carries the extension its platform needs", () => {
  assert.equal(binaryName("win32"), "archwarden.exe");
  assert.equal(binaryName("linux"), "archwarden");
  assert.equal(binaryName("darwin"), "archwarden");
});

test("the specifier is the package plus the binary", () => {
  assert.equal(
    specifierFor("linux", "x64"),
    "@archwarden/cli-linux-x64/archwarden",
  );
  assert.equal(
    specifierFor("win32", "x64"),
    "@archwarden/cli-win32-x64/archwarden.exe",
  );
});

test("both failures say what to do about them", () => {
  // The two are different problems: one platform is not published, the other
  // was published and did not arrive. Telling someone to reinstall when no
  // binary exists for their machine wastes their afternoon.
  const unsupported = unsupportedMessage("freebsd", "x64");
  assert.match(unsupported, /no binary is published/);
  assert.match(unsupported, /cargo install/);

  const missing = missingPackageMessage("@archwarden/cli-linux-x64");
  assert.match(missing, /optional dependency/);
  assert.match(missing, /--no-optional/);
  assert.doesNotMatch(missing, /cargo/, "reinstalling is the fix, not building");
});
