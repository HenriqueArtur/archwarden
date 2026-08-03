# Changelog

Notable changes to archwarden, in the format of
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versions before `0.6.0` are not reconstructed here. Their history is in the
[GitHub releases](https://github.com/HenriqueArtur/archwarden/releases) and, in
more detail than the releases carry, in the body of each `chore(release)`
commit — `git log --grep '^chore(release)'`.

archwarden is pre-1.0: minor versions carry new behaviour, patch versions carry
fixes, and neither is a stability promise. The distinction that matters more
than semver is called out explicitly below whenever it applies — **a change
that makes an existing, unchanged config report differently** is the one that
breaks someone's build, and it is listed first in its release with a note
saying so.

## [Unreleased]

### Added

- **`naming` rules can spell the export name from the directory as well as the
  filename**, through a new optional `dir_pattern` whose capture groups join
  `file_pattern`'s in the same template.
  ([#16](https://github.com/HenriqueArtur/archwarden/issues/16))

  ```json
  {
    "type": "naming",
    "roots": ["src/Infrastructure/Repositories/Entities/*"],
    "dir_pattern": "^(?<entity>[A-Za-z0-9]+)$",
    "file_pattern": "^(?<action>[a-z0-9-]+)\\.ts$",
    "must_export": {
      "kind": "function",
      "name": "{{pascal(entity)}}{{pascal(action)}}Repository"
    }
  }
  ```

  This is the per-entity repository shape, where `fetch-by-id.ts` exists once
  per entity and the entity prefix is what a stack trace names. Previously the
  closest expressible rule asked for `FetchByIdRepository` and was wrong on
  every file it touched.

  `dir_pattern` matches the *name* of the directory the file sits in, not the
  path to it. When set, it must match, exactly as `file_pattern` must. A group
  defined by both patterns is refused at compile time rather than resolved by
  precedence. The rule stays purely lexical, so `describe` and `scaffold` keep
  answering for files that do not exist yet.

- **`import-boundary` can forbid a dependency**, through
  `forbid_import_from_packages` and an importer-side `except_from`.
  ([#14](https://github.com/HenriqueArtur/archwarden/issues/14), decision 17)

  ```json
  {
    "type": "import-boundary",
    "id": "three-is-quarantined",
    "level": "error",
    "from": "src/**",
    "forbid_import_from_packages": ["three"],
    "except_from": ["src/scripts/three/**"]
  }
  ```

  `RULES.md` declared this out of scope for v0 because a dependency has no
  repo-relative path for a glob to match. It still has none — the field names
  the *package*, which is why it is correct under pnpm's store layout and under
  yarn PnP, where `node_modules/three/**` is a lie or does not exist.

  Matched as "the package and anything under it", so
  `three/examples/jsm/loaders/GLTFLoader.js` is caught and `three-mesh-bvh` is
  not. `node:fs` and `fs` are one identity. An import that resolves into the
  repository is a path and is matched by `forbid_import_from` instead, never
  both. Transitivity is still declined.

  It reads the specifier rather than the resolution, so unlike the path half it
  still fires on a repository whose dependencies are not installed.

- `config doctor` reports **`dir-pattern-matches-nothing`** when a `dir_pattern`
  matches no directory in its scope — the mistake being writing it against the
  whole path — because such a rule applies to no file and is indistinguishable
  from one that passes.

### Changed

- **`checks_skipped` no longer counts files that are not JavaScript or
  TypeScript.** A `DOC.md` inside an `import-boundary` scope used to produce one
  skipped check per rule, forever, so a repository that documents its layers
  could never report a run the way `AGENTS.md` asks — the number was pinned
  above zero with nothing to fix. It now counts only rules that wanted a
  *source* file whose facts were unavailable, which is the case a reader can act
  on. `check --file` is unchanged and still reports these under `not-source`.
  ([#15](https://github.com/HenriqueArtur/archwarden/issues/15))

  This changes what an existing, unchanged config reports: `checks_skipped` will
  drop, in some repositories to zero. No finding changes, and the exit code
  cannot change.

  One consequence worth naming: an `.astro`, `.vue` or `.svelte` file in a rule's
  scope was counted under the same reason as a `DOC.md`, and now is not counted
  either. That signal was already misleading — it said "a check was skipped"
  where the honest statement is "no parser exists for this extension" — and
  giving it its own reason is
  [#13](https://github.com/HenriqueArtur/archwarden/issues/13).

- **The text output names which rules were skipped, and where.** Previously it
  printed the count alone, so the only reader who could answer "which ones?" was
  one already piping through `jq`.
  ([#12](https://github.com/HenriqueArtur/archwarden/issues/12))

  ```
  note: `src/user/broken.ts` was not checked — unexpected token
        2 checks skipped there: calcs-need-spec, domain-forbids-infrastructure
  1 error, 0 warnings, 2 skipped · 3 files, 3 directories · 1 parsed · 2ms
  ```

### Fixed

- **`impact --apply` no longer reports success after leaving imports pointing at
  a file it just deleted.**
  ([#11](https://github.com/HenriqueArtur/archwarden/issues/11))

  A file that imports a moving package by name — `@org/domain/thing` rather than
  `../thing` — is invisible to archwarden when that package does not resolve.
  Nothing listed it as an importer, so nothing rewrote it and nothing refused:
  the move went through, printed the files it *had* rewritten, and exited `0`
  over a repository that no longer type-checks. The existing guard could not
  catch it, because it asks whether every *known* importer was rewritten and
  this was a file that was never known.

  `--apply` now refuses when an import names a package the move is taking files
  out of and archwarden cannot place it. The refusal is total, as every refusal
  here is, and `--force` does not override it — `--force` was in the command
  that produced the broken repository.

  The usual cause is a workspace that is not installed, so the message says so.
  An unresolved import to a real dependency does *not* refuse: `react` names no
  package in the repository, and no move could change what it means.

  Reproduced from two `exports` shapes archwarden reads differently from the
  bundler (`"./*/*": "./src/*/*.ts"`, and a package with no `exports` at all); a
  clone before `install` reaches the same state.

---

[Unreleased]: https://github.com/HenriqueArtur/archwarden/compare/v0.5.1...HEAD
