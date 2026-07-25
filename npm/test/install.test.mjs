// Tier 2 for the npm shim: the parts that decide *what* to download, and the
// wrapper's contract with its caller.
import { test } from "node:test";
import assert from "node:assert/strict";
import { targetFor, detectLibc, archiveUrl, binaryPath } from "../install.mjs";

test("every platform we publish for maps to its triple", () => {
  assert.equal(targetFor("darwin", "arm64", null), "aarch64-apple-darwin");
  assert.equal(targetFor("darwin", "x64", null), "x86_64-apple-darwin");
  assert.equal(targetFor("win32", "x64", null), "x86_64-pc-windows-msvc");
  assert.equal(targetFor("linux", "x64", "glibc"), "x86_64-unknown-linux-gnu");
  assert.equal(targetFor("linux", "arm64", "glibc"), "aarch64-unknown-linux-gnu");
  assert.equal(targetFor("linux", "x64", "musl"), "x86_64-unknown-linux-musl");
  assert.equal(targetFor("linux", "arm64", "musl"), "aarch64-unknown-linux-musl");
});

test("a platform we do not publish for gets null, not a guess", () => {
  // Downloading the wrong binary is worse than saying we have none: the
  // failure moves from install time to the first run, with no explanation.
  assert.equal(targetFor("freebsd", "x64", null), null);
  assert.equal(targetFor("linux", "ia32", "glibc"), null);
  assert.equal(targetFor("win32", "arm64", null), null);
});

test("musl is detected by the absence of a glibc version", () => {
  assert.equal(detectLibc({ header: { glibcVersionRuntime: "2.39" } }), "glibc");
  assert.equal(detectLibc({ header: {} }), "musl");
  // No report at all: assume glibc, which is what nearly every Linux is. A
  // wrong guess here fails loudly at download time rather than silently.
  assert.equal(detectLibc(undefined), "glibc");
});

test("the archive url matches what the release workflow publishes", () => {
  const repo = "https://github.com/HenriqueArtur/archwarden";
  assert.equal(
    archiveUrl(repo, "1.2.3", "aarch64-apple-darwin"),
    `${repo}/releases/download/v1.2.3/archwarden-1.2.3-aarch64-apple-darwin.tar.gz`,
  );
  assert.equal(
    archiveUrl(repo, "1.2.3", "x86_64-pc-windows-msvc"),
    `${repo}/releases/download/v1.2.3/archwarden-1.2.3-x86_64-pc-windows-msvc.zip`,
  );
});

test("the path inside the archive matches how it is packaged", () => {
  assert.equal(
    binaryPath("1.2.3", "x86_64-unknown-linux-gnu"),
    "archwarden-1.2.3-x86_64-unknown-linux-gnu/archwarden",
  );
  assert.equal(
    binaryPath("1.2.3", "x86_64-pc-windows-msvc"),
    "archwarden-1.2.3-x86_64-pc-windows-msvc/archwarden.exe",
  );
});
