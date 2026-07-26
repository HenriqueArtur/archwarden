// The logic that decides which binary to run. Everything else in the wrapper
// is `spawnSync`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import {
  binaryName,
  detectLibc,
  missingPackageMessage,
  packageFor,
  specifierFor,
  unsupportedMessage,
} from "../resolve.mjs";

test("every published platform maps to its package", () => {
  assert.equal(packageFor("darwin", "arm64", null), "@archwarden/cli-darwin-arm64");
  assert.equal(packageFor("darwin", "x64", null), "@archwarden/cli-darwin-x64");
  assert.equal(packageFor("win32", "x64", null), "@archwarden/cli-win32-x64");
  assert.equal(packageFor("linux", "x64", "glibc"), "@archwarden/cli-linux-x64");
  assert.equal(packageFor("linux", "arm64", "glibc"), "@archwarden/cli-linux-arm64");
  assert.equal(packageFor("linux", "x64", "musl"), "@archwarden/cli-linux-x64-musl");
  assert.equal(packageFor("linux", "arm64", "musl"), "@archwarden/cli-linux-arm64-musl");
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
      for (const libc of ["glibc", "musl"]) {
        const pkg = packageFor(platform, arch, libc);
        if (pkg) reachable.add(pkg);
      }
    }
  }

  assert.deepEqual([...reachable].sort(), declared);
});

test("musl only applies to linux", () => {
  // A `libc` of "musl" on macOS is a bug in the caller, not a platform. It
  // must not select a package that does not exist.
  assert.equal(packageFor("darwin", "arm64", "musl"), "@archwarden/cli-darwin-arm64");
  assert.equal(packageFor("win32", "x64", "musl"), "@archwarden/cli-win32-x64");
});

test("a platform we do not publish for gets null, not a guess", () => {
  for (const [platform, arch] of [
    ["freebsd", "x64"],
    ["linux", "ia32"],
    ["win32", "arm64"],
    ["darwin", "ppc64"],
  ]) {
    assert.equal(packageFor(platform, arch, "glibc"), null, `${platform}-${arch}`);
    assert.equal(specifierFor(platform, arch, "glibc"), null);
  }
});

test("musl is detected by the absence of a glibc version", () => {
  assert.equal(detectLibc({ header: { glibcVersionRuntime: "2.39" } }), "glibc");
  assert.equal(detectLibc({ header: {} }), "musl");
  // No report at all: assume glibc, which nearly every Linux is. A wrong
  // guess fails loudly when the package is missing, not silently.
  assert.equal(detectLibc(undefined), "glibc");
  assert.equal(detectLibc(null), "glibc");
});

test("the binary carries the extension its platform needs", () => {
  assert.equal(binaryName("win32"), "archwarden.exe");
  assert.equal(binaryName("linux"), "archwarden");
  assert.equal(binaryName("darwin"), "archwarden");
});

test("the specifier is the package plus the binary", () => {
  assert.equal(
    specifierFor("linux", "x64", "musl"),
    "@archwarden/cli-linux-x64-musl/archwarden",
  );
  assert.equal(
    specifierFor("win32", "x64", null),
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
