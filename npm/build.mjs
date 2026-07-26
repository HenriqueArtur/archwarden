#!/usr/bin/env node
// Builds the npm packages from the release archives.
//
// Eight packages: the one people install, and seven that carry a binary each.
// The seven are generated rather than kept in the tree, because their only
// contents are a manifest and a file the release already produced, and a
// checked-in copy is a copy that goes stale.
//
//     build.mjs <dist> <out> <version>
//
// `dist` holds the release artifacts, each `archwarden-<version>-<target>` a
// directory extracted from its archive.
import { mkdir, readFile, writeFile, copyFile, chmod, rm } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));

/** Every platform package: what it is called, and what it is for. */
export const PLATFORMS = [
  { pkg: "cli-darwin-arm64", target: "aarch64-apple-darwin", os: "darwin", cpu: "arm64" },
  { pkg: "cli-darwin-x64", target: "x86_64-apple-darwin", os: "darwin", cpu: "x64" },
  { pkg: "cli-linux-arm64", target: "aarch64-unknown-linux-gnu", os: "linux", cpu: "arm64", libc: "glibc" },
  { pkg: "cli-linux-arm64-musl", target: "aarch64-unknown-linux-musl", os: "linux", cpu: "arm64", libc: "musl" },
  { pkg: "cli-linux-x64", target: "x86_64-unknown-linux-gnu", os: "linux", cpu: "x64", libc: "glibc" },
  { pkg: "cli-linux-x64-musl", target: "x86_64-unknown-linux-musl", os: "linux", cpu: "x64", libc: "musl" },
  { pkg: "cli-win32-x64", target: "x86_64-pc-windows-msvc", os: "win32", cpu: "x64" },
];

/** The manifest for one platform package. */
export function manifestFor(platform, version) {
  const binary = platform.os === "win32" ? "archwarden.exe" : "archwarden";
  return {
    name: `@archwarden/${platform.pkg}`,
    version,
    description: `The archwarden binary for ${platform.os}-${platform.cpu}${
      platform.libc === "musl" ? " (musl)" : ""
    }.`,
    license: "MIT OR Apache-2.0",
    repository: {
      type: "git",
      url: "git+https://github.com/HenriqueArtur/archwarden.git",
    },
    homepage: "https://github.com/HenriqueArtur/archwarden",
    // `os`, `cpu` and `libc` are how the package manager knows to skip this
    // one. Without them every machine downloads all seven.
    os: [platform.os],
    cpu: [platform.cpu],
    ...(platform.libc ? { libc: [platform.libc] } : {}),
    files: [binary],
    engines: { node: ">=18" },
  };
}

/**
 * Files copied from the repository root into the main package.
 *
 * Both are read by someone who never sees this repository: the README is the
 * package's page on npm, and `AGENTS.md` is what an agent is pointed at from
 * inside `node_modules`. They travel with the version that produced them.
 */
const FROM_ROOT = ["README.md", "AGENTS.md"];

/** Builds all eight packages. Exported so a test can run it. */
export async function build(dist, out, version) {
  await rm(out, { recursive: true, force: true });

  // The package people install, copied rather than generated: it is hand
  // written and reviewed, and only its version moves.
  const mainOut = join(out, "archwarden");
  await mkdir(join(mainOut, "bin"), { recursive: true });
  const main = JSON.parse(
    await readFile(join(here, "archwarden", "package.json"), "utf8"),
  );
  main.version = version;
  for (const platform of PLATFORMS) {
    main.optionalDependencies[`@archwarden/${platform.pkg}`] = version;
  }
  await writeFile(
    join(mainOut, "package.json"),
    `${JSON.stringify(main, null, 2)}\n`,
  );
  for (const file of ["resolve.mjs", "bin/archwarden.mjs"]) {
    await copyFile(join(here, "archwarden", file), join(mainOut, file));
  }
  for (const file of FROM_ROOT) {
    await copyFile(join(here, "..", file), join(mainOut, file));
  }
  await chmod(join(mainOut, "bin", "archwarden.mjs"), 0o755);

  for (const platform of PLATFORMS) {
    const manifest = manifestFor(platform, version);
    const dir = join(out, platform.pkg);
    await mkdir(dir, { recursive: true });
    await writeFile(
      join(dir, "package.json"),
      `${JSON.stringify(manifest, null, 2)}\n`,
    );

    const [binary] = manifest.files;
    await copyFile(
      join(dist, `archwarden-${version}-${platform.target}`, binary),
      join(dir, binary),
    );
    await chmod(join(dir, binary), 0o755);
  }

  console.log(`built ${PLATFORMS.length + 1} packages in ${out}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [dist, out, version] = process.argv.slice(2);
  if (!dist || !out || !version) {
    throw new Error("usage: build.mjs <dist> <out> <version>");
  }
  await build(dist, out, version);
}
