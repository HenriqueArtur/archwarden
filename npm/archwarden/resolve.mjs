// Which platform package holds this machine's binary.
//
// No postinstall, and nothing downloaded at install time: the five binaries
// are published as separate packages, each declaring the `os` and `cpu` it is
// for, and listed here as optional dependencies. The package manager installs
// the one that matches and skips the rest.
//
// One package per Linux architecture, with no `libc` distinction, because the
// Linux binaries are statically linked against musl and have no C library to
// distinguish. 0.3.0 shipped a glibc build requiring 2.39 that would not start
// on Debian 12 — a floor nobody had chosen, which moved with whatever the
// build runner happened to have. See decision 14.
//
// That is not a style preference. pnpm 10 blocks dependencies' install scripts
// by default, so a package that fetched its binary in a postinstall installs
// *silently without a binary* -- the failure arrives later, at first run, in
// someone else's terminal. Optional dependencies need no permission from
// anyone.

/** The platform package for a machine, or null if none is published. */
export function packageFor(platform, arch) {
  // Written out rather than assembled, so an unpublished combination is
  // `undefined` here instead of a name that resolves to nothing later.
  return (
    {
      "darwin-arm64": "@archwarden/cli-darwin-arm64",
      "darwin-x64": "@archwarden/cli-darwin-x64",
      "linux-arm64": "@archwarden/cli-linux-arm64",
      "linux-x64": "@archwarden/cli-linux-x64",
      "win32-x64": "@archwarden/cli-win32-x64",
    }[`${platform}-${arch}`] ?? null
  );
}

/** The binary's name inside its package. */
export function binaryName(platform) {
  return platform === "win32" ? "archwarden.exe" : "archwarden";
}

/** The specifier to hand `require.resolve`. */
export function specifierFor(platform, arch) {
  const pkg = packageFor(platform, arch);
  return pkg && `${pkg}/${binaryName(platform)}`;
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
