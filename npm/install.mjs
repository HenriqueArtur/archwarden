#!/usr/bin/env node
// Downloads the archwarden binary for this machine.
//
// A postinstall rather than one package per platform: the matrix is seven
// targets, and seven published packages is seven things to get out of step
// with a release. One package that fetches the right archive has one version
// number to be wrong.
//
// It never fails the install. `npm install` running in CI on a platform we do
// not ship for should not take the whole install down; the shim says what
// happened and `archwarden` reports it again if anyone runs it.
import { createWriteStream } from "node:fs";
import { chmod, mkdir, rm, readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { pipeline } from "node:stream/promises";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));

/** The release target triple for this machine, or null if we ship none. */
export function targetFor(platform, arch, libc) {
  const key = `${platform}-${arch}`;
  switch (key) {
    case "darwin-arm64":
      return "aarch64-apple-darwin";
    case "darwin-x64":
      return "x86_64-apple-darwin";
    case "win32-x64":
      return "x86_64-pc-windows-msvc";
    case "linux-x64":
      return libc === "musl"
        ? "x86_64-unknown-linux-musl"
        : "x86_64-unknown-linux-gnu";
    case "linux-arm64":
      return libc === "musl"
        ? "aarch64-unknown-linux-musl"
        : "aarch64-unknown-linux-gnu";
    default:
      return null;
  }
}

/** Which C library this Linux uses. Glibc reports itself; musl does not. */
export function detectLibc(report = process.report?.getReport?.()) {
  const header = report?.header;
  if (!header) return "glibc";
  if (header.glibcVersionRuntime) return "glibc";
  // A Node built against musl reports no glibc version at all.
  return "musl";
}

/** Where a release archive lives, given the pieces of a release. */
export function archiveUrl(repository, version, target) {
  const suffix = target.includes("windows") ? "zip" : "tar.gz";
  return `${repository}/releases/download/v${version}/archwarden-${version}-${target}.${suffix}`;
}

/** The path inside the archive that holds the binary. */
export function binaryPath(version, target) {
  const name = target.includes("windows") ? "archwarden.exe" : "archwarden";
  return `archwarden-${version}-${target}/${name}`;
}

async function main() {
  const manifest = JSON.parse(
    await readFile(join(here, "package.json"), "utf8"),
  );
  const version = manifest.version;
  const repository = manifest.homepage;

  const target = targetFor(
    process.platform,
    process.arch,
    process.platform === "linux" ? detectLibc() : null,
  );
  if (!target) {
    console.warn(
      `archwarden: no binary is published for ${process.platform}-${process.arch}. ` +
        `Install with \`cargo install archwarden-cli\` instead.`,
    );
    return;
  }

  const url = archiveUrl(repository, version, target);
  const scratch = join(tmpdir(), `archwarden-${version}-${process.pid}`);
  await mkdir(scratch, { recursive: true });

  try {
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`${response.status} ${response.statusText} for ${url}`);
    }

    const archive = join(scratch, target.includes("windows") ? "a.zip" : "a.tar.gz");
    await pipeline(response.body, createWriteStream(archive));

    // The release publishes a checksum beside every archive. Verifying it is
    // the difference between "we downloaded something" and "we downloaded the
    // thing that was released".
    const checksums = await fetch(`${url.replace(/\.(tar\.gz|zip)$/, "")}.sha256`);
    if (checksums.ok) {
      const expected = (await checksums.text()).trim().split(/\s+/)[0];
      const actual = createHash("sha256")
        .update(await readFile(archive))
        .digest("hex");
      if (expected !== actual) {
        throw new Error(`checksum mismatch for ${url}`);
      }
    }

    await mkdir(join(here, "bin"), { recursive: true });
    if (target.includes("windows")) {
      execFileSync("tar", ["xf", archive, "-C", scratch]);
    } else {
      execFileSync("tar", ["xzf", archive, "-C", scratch]);
    }

    const extracted = join(scratch, binaryPath(version, target));
    const installed = join(here, "bin", target.includes("windows") ? "archwarden.exe" : "archwarden");
    await pipeline(
      (await import("node:fs")).createReadStream(extracted),
      createWriteStream(installed),
    );
    await chmod(installed, 0o755);
  } catch (error) {
    console.warn(`archwarden: could not install the binary — ${error.message}`);
    console.warn("archwarden: run `cargo install archwarden-cli`, or report this.");
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await main();
}
