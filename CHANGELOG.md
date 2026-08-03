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

---

[Unreleased]: https://github.com/HenriqueArtur/archwarden/compare/v0.5.1...HEAD
