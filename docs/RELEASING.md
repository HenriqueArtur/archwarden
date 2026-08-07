# Releasing

Cutting a release is a tag push. Everything after that is
[`.github/workflows/release.yml`](../.github/workflows/release.yml): five
binaries built on runners of their own architecture, each one checked before it
is published, then a GitHub release and six npm packages.

This document exists because the part that is *not* automated — deciding the
version and getting it into every place that states it — was tribal knowledge
recorded only in the body of `chore(release)` commits. A release that reaches
CI with the version bumped in two of three places builds five binaries, gets
caught by the version guard, and wastes twenty minutes.

## What the version number means

Pre-1.0, so the semver contract is the loose one: minor for anything new, patch
for fixes. The line that actually matters here is different, and it is not the
one semver draws:

> **Does this change what an existing, unchanged config reports?**

If yes, it belongs in a minor even when it is a bug fix, and the release commit
and changelog must say so in plain words. 0.5.0 is the example. Workspace
resolution made imports written by package name visible to the graph — on the
target repository that was 5481 edges the previous version simply could not
see. Every project with an `import-boundary` rule over any of them got findings
on its first 0.5.0 run that 0.4.0 never reported. That is the bug being fixed,
not a regression, and `baseline` is the answer for anyone not paying that debt
today. Someone reading only the release title would have found out by having
their build break.

## Before the tag

### 1. The changelog

Move everything under `## [Unreleased]` into a new version section with today's
date. If a `Changed` entry alters what an existing config reports, it goes
first and says so.

### 2. The version, in three places

```bash
NEW=0.6.0
```

**`Cargo.toml`, `[workspace.package]`** — the single source of truth for the
eight crates:

```toml
[workspace.package]
version = "0.6.0"
```

**`Cargo.toml`, `[workspace.dependencies]`** — the seven internal crates each
pin a version alongside their path. A path dependency without a version cannot
be published to crates.io, and these must move together:

```toml
archwarden-cache = { path = "crates/archwarden-cache", version = "0.6.0" }
archwarden-config = { path = "crates/archwarden-config", version = "0.6.0" }
archwarden-core = { path = "crates/archwarden-core", version = "0.6.0" }
archwarden-engine = { path = "crates/archwarden-engine", version = "0.6.0" }
archwarden-parser = { path = "crates/archwarden-parser", version = "0.6.0" }
archwarden-resolver = { path = "crates/archwarden-resolver", version = "0.6.0" }
archwarden-rules = { path = "crates/archwarden-rules", version = "0.6.0" }
```

**`npm/archwarden/package.json`** — the `version` field, and only that field:

```json
{ "version": "0.6.0" }
```

Then rebuild so `Cargo.lock` picks the new version up:

```bash
cargo build --locked || cargo build
cargo xtask check-schema
```

`Cargo.lock` is part of the release commit. `--locked` is what the release
workflow builds with, so a lockfile that does not match the manifest fails
there rather than here.

### What you do *not* bump

`optionalDependencies` in `npm/archwarden/package.json` is generated.
`npm/build.mjs` overwrites all five entries with the version being released, so
the values checked into the tree are inert — they currently read `0.1.1` and
that is harmless. Editing them by hand achieves nothing and will be overwritten.

### 3. The cache format, if facts changed

`FORMAT_VERSION` in `crates/archwarden-cache/src/store.rs` is currently `3`.
Bump it in the same release whenever the shape of what is cached changes —
`FileFacts` gaining a field is the usual trigger. A cache written by an older
format is then discarded rather than misread, which is what decision 3 built
the version for. Say it in the changelog: users lose one warm cache and pay one
cold run.

### 4. The release commit

```
chore(release): 0.6.0
```

The body is the release's own explanation, and it is read later. It should
carry:

- what kind of release this is, in one line;
- anything that changes what an existing config reports, first and unmissable;
- the cache format bump, if there was one;
- confirmation that the three places moved and the built binary reports the new
  version.

The bodies on `ba1fbce` (0.5.0) and `b2cdc33` (0.5.1) are the models.

### 5. A dry run, when you want one

The release workflow accepts a `workflow_dispatch` with a `tag` input. Run it
**from a branch** and it builds every target and runs every check, then stops:
both the `publish` and `npm` jobs are gated on
`startsWith(github.ref, 'refs/tags/v')`, and a dispatch from `main` gives
`refs/heads/main`. Nothing is published and nothing is tagged.

That is the way to test a change to the workflow itself, and it is cheap
insurance before a release that touches distribution.

**The ref is what decides, not the event.** This paragraph used to say a
dispatch "never satisfies" that gate, and that is wrong: `gh workflow run
release.yml --ref v0.8.0 -f tag=v0.8.0` dispatches *on the tag*, so
`github.ref` is `refs/tags/v0.8.0` and both jobs run and publish exactly as a
tag push would.

Which is a real escape hatch, and 0.8.0 needed it. GitHub Actions was in a
major incident with webhook delivery throttled to about 15%, so pushing the
tag created no run at all — nine re-pushes over half an hour, none of which
fired. A dispatch goes through the API rather than webhook delivery, and it
started immediately.

**Reach for it only when the tag push produced no run.** Push the tag, wait a
minute, and look:

```bash
gh run list --workflow=release.yml --limit 1
```

A run for the tag means the ordinary path worked and there is nothing to do.
Dispatching *as well* gets you two runs publishing one version: the first
wins, the second fails on npm's duplicate-version rejection, and the red run
stays in the history for whoever looks at it next to worry about. That
happened to 0.9.1, on a day when Actions had already recovered and the
dispatch was not needed at all.

Stop any retry loop before dispatching, so two runs cannot publish the same
version.

Stopping the loop is not enough on its own, and 0.8.0 showed why: a throttled
webhook is *delayed*, not dropped. Three of the nine re-pushed tags arrived
twenty minutes after the dispatch had already published, and each started a
release run of its own. What happened is worth knowing, because it is the
answer to "how bad is this":

- the GitHub release step ran again and updated the existing release with the
  same artefacts — 10 assets, unchanged, original publish timestamp intact;
- the npm step failed with `E403 — You cannot publish over the previously
  published versions`, which is npm refusing exactly as it should.

So the damage is three red runs in the history and nothing else. Do not delete
the tag to tidy it up: the release is published and the tag is what it points
at. If you want the runs gone, they can be deleted from the Actions history —
but leaving them is more honest.

## The tag

```bash
git tag -a v0.6.0 -m "archwarden 0.6.0"
git push origin main
git push origin v0.6.0
```

The tag is what triggers everything. Push it after `main`, never before.

## What the workflow does, and what it will catch

### Build — five targets, five runners

| Target | Runner | Notes |
| --- | --- | --- |
| `aarch64-apple-darwin` | `macos-latest` | native |
| `x86_64-apple-darwin` | `macos-latest` | the one real cross-compile |
| `x86_64-unknown-linux-musl` | `ubuntu-latest` | static musl |
| `aarch64-unknown-linux-musl` | `ubuntu-24.04-arm` | static musl |
| `x86_64-pc-windows-msvc` | `windows-latest` | zip rather than tar |

Each builds on a runner of its own architecture rather than cross-compiling,
because `blake3` compiles C for its SIMD paths and a cross-compile would need a
C cross-toolchain per target. Linux is musl only, and that is decision 14: a
glibc build links against whatever the runner has, which made 0.3.0 require
glibc 2.39 and refuse to start on Debian 12 — a floor nobody chose.

Two gates run before anything is packaged:

**The version guard.** Every target except the cross-compiled
`x86_64-apple-darwin` runs its own binary and compares `--version` against the
tag. A workspace nobody bumped would otherwise ship as the new version with the
old binary inside, silently, because the archive is named from the tag and
`build.mjs` finds it by that name. This is the check that catches step 2 going
wrong, and it fires after the builds — hence doing step 2 carefully.

**The old-libc check.** Both musl binaries are piped into `debian:11` and
`alpine:3.19` and run there. Debian 11 is glibc 2.31, older than Debian 12,
Ubuntu 22.04 and every `node:` image, so passing there covers all of them;
Alpine has no glibc at all. Between them they are the two ways a Linux binary
fails to start. This check did not exist for 0.3.0, and the reason it did not
is worth remembering: every check archwarden had ran on a machine shaped like
the runner, so a binary that only worked on the runner passed all of them.

Then each target packages `archwarden` plus `README.md` and both licences into
`archwarden-<version>-<target>.tar.gz` (or `.zip` on Windows), and writes a
`.sha256` beside it via `scripts/checksum.py`.

### Publish — the GitHub release

`softprops/action-gh-release` with `generate_release_notes: true`, uploading
every archive and checksum. Downloads use `download-artifact@v8`, which fails
on a hash mismatch rather than warning — the right behaviour when those bytes
are about to become the binaries inside six published npm packages.

### npm — six packages, in one order

One wrapper (`archwarden`) and five carrying a binary each
(`@archwarden/cli-darwin-arm64`, `-darwin-x64`, `-linux-arm64`, `-linux-x64`,
`-win32-x64`). The platform packages are generated by `npm/build.mjs` rather
than checked in, since their only contents are a manifest and a file the
release already produced.

**Platform packages publish first, then the wrapper.** The wrapper's
`optionalDependencies` name them at an exact version, so publishing it first
leaves a window in which installing archwarden resolves to no binary at all.

No package has a postinstall script, anywhere. pnpm 10 blocks dependencies'
install scripts by default, so a package that downloaded its own binary would
install silently and fail at first run in someone else's terminal. The `os` and
`cpu` fields on each platform manifest are how the package manager knows to
skip the four that are not yours.

If `NPM_TOKEN` is not set the job warns and publishes nothing; the GitHub
release still exists. That is a recoverable state — set the secret and re-run
the job.

## After the tag

```bash
npm view archwarden version
npm view archwarden optionalDependencies
```

Both should read the new version. Then install it somewhere real and run it —
the release checked that the binary starts on an old libc, not that it still
lints anything.

## If it goes wrong

**Caught before publish** (version guard, container check, a build failure).
Nothing was published. Delete the tag, fix, tag again:

```bash
git push --delete origin v0.6.0
git tag -d v0.6.0
```

**Caught after publish.** Do not delete a published npm version and do not
retag. Both break anyone who already installed it, and npm will not let you
republish the same version anyway. Ship a patch. This is the cheap direction:
0.5.1 exists because a second pair of eyes found four things in 0.5.0, and that
is a normal thing for a release to be.

**The npm job failed but the release succeeded.** Re-run the failed job. It
downloads the same artifacts and is safe to repeat, right up until a package
was published — at which point npm rejects the duplicate and the run fails on
a version that is already live. Check `npm view` before re-running.
