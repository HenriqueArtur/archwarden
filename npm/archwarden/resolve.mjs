// Which platform package holds this machine's binary.
//
// No postinstall, and nothing downloaded at install time: the seven binaries
// are published as separate packages, each declaring the `os`, `cpu` and
// `libc` it is for, and listed here as optional dependencies. The package
// manager installs the one that matches and skips the rest.
//
// That is not a style preference. pnpm 10 blocks dependencies' install scripts
// by default, so a package that fetched its binary in a postinstall installs
// *silently without a binary* -- the failure arrives later, at first run, in
// someone else's terminal. Optional dependencies need no permission from
// anyone.

/** The platform package for a machine, or null if none is published. */
export function packageFor(platform, arch, libc) {
  const suffix = platform === "linux" && libc === "musl" ? "-musl" : "";
  const key = `${platform}-${arch}${suffix}`;

  // Written out rather than assembled, so an unpublished combination is
  // `undefined` here instead of a name that resolves to nothing later.
  return (
    {
      "darwin-arm64": "@archwarden/cli-darwin-arm64",
      "darwin-x64": "@archwarden/cli-darwin-x64",
      "linux-arm64": "@archwarden/cli-linux-arm64",
      "linux-arm64-musl": "@archwarden/cli-linux-arm64-musl",
      "linux-x64": "@archwarden/cli-linux-x64",
      "linux-x64-musl": "@archwarden/cli-linux-x64-musl",
      "win32-x64": "@archwarden/cli-win32-x64",
    }[key] ?? null
  );
}

/** The binary's name inside its package. */
export function binaryName(platform) {
  return platform === "win32" ? "archwarden.exe" : "archwarden";
}

/** The specifier to hand `require.resolve`. */
export function specifierFor(platform, arch, libc) {
  const pkg = packageFor(platform, arch, libc);
  return pkg && `${pkg}/${binaryName(platform)}`;
}

/**
 * Which C library this Linux uses.
 *
 * `process.report` rather than shelling out to `ldd`: it costs no subprocess,
 * and a hook that runs on every file write should not spawn one to answer a
 * question that does not change. Bun and Node both carry the field.
 */
export function detectLibc(report) {
  const header = report?.header;
  if (!header) return "glibc";
  // A runtime built against musl reports no glibc version at all.
  return header.glibcVersionRuntime ? "glibc" : "musl";
}

/** What to tell someone whose machine has no published binary. */
export function unsupportedMessage(platform, arch) {
  return (
    `archwarden: no binary is published for ${platform}-${arch}.\n` +
    "Build from source with `cargo install --git " +
    "https://github.com/HenriqueArtur/archwarden archwarden-cli`, or open an " +
    "issue asking for this platform."
  );
}

/** What to tell someone whose platform package did not get installed. */
export function missingPackageMessage(pkg) {
  return (
    `archwarden: \`${pkg}\` is not installed.\n` +
    "It is an optional dependency, so this usually means the install ran " +
    "with optional dependencies disabled, or the lockfile was written on a " +
    "different platform. Reinstall without `--no-optional`."
  );
}
