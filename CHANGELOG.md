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

## [0.8.1] — 2026-08-06

### Fixed

- **`impact <dir> --to … --apply` renames the directory instead of flattening
  it.** ([#32](https://github.com/HenriqueArtur/archwarden/issues/32))

  **This changes what an existing command does, and the old behaviour lost
  directory structure.** Every file landed directly in the destination and the
  levels between were gone:

  ```console
  $ archwarden impact src/Group --to '../Renamed' --apply
    src/Group/A/alpha.ts → src/Renamed/alpha.ts     # `A/` gone
    src/Group/B/beta.ts  → src/Renamed/beta.ts      # `B/` gone
  ```

  Silent, with exit 0, whenever no two basenames collided. Where they did the
  collision guard refused — which is how it stayed hidden, and is itself the
  bug stated plainly: two files in different directories have no business
  landing on one path. On a real 19-entity namespace, 93 source files mapped to
  57 destinations.

  The path below the match now comes along, so `src/Group/A/alpha.ts` lands at
  `src/Renamed/A/alpha.ts`.

  **The glob form changed with it**, and this is worth reading if you use it. A
  file nested inside a match keeps its nesting: with
  `'src/*/shared' --to '../calcs'`, a file at `order/shared/calcs/total.ts`
  now lands at `order/calcs/calcs/total.ts` where it used to land at
  `order/calcs/total.ts`. The doubled name looks odd and is the honest answer —
  the file was in `shared/calcs/`, and `shared` is becoming `calcs`. Collapsing
  that level is a guess about intent, and it is the same guess that flattened
  the namespace above. The dry run prints every destination, so an unwanted one
  is visible before `--apply`.

## [0.8.0] — 2026-08-06

Nine reported issues. Read the first two entries before upgrading: both change
what an existing, unchanged config reports — the first towards reporting more,
the second towards reporting less.

### Changed

- **A warm run no longer resolves one file's imports from another file's
  directory.** ([#20](https://github.com/HenriqueArtur/archwarden/issues/20))

  **This changes what an existing config reports.** Repositories that contain
  two files with identical bytes — barrel files, re-export stubs, `export {};`
  — may see boundary findings on the next run that 0.7.0 did not report, and
  may stop seeing findings that were never real. Both directions are the same
  fix.

  The fact cache is keyed by content alone, so identical files share one entry,
  and the entry carried the path of whichever file was stamped first.
  `resolve_imports` reads that path to know which directory a relative
  specifier points from. Measured on a five-file repository:

  ```console
  $ archwarden check     # cold cache
  1 error, 0 warnings · 4 parsed, 0 reused

  $ archwarden check     # warm cache, nothing touched on disk
  0 errors, 0 warnings · 0 parsed, 4 reused
  ```

  A real boundary violation disappeared, and two runs over an unchanged tree
  disagreed. Reverse which twin is stored and the mirror happens: a finding
  against a file that imports nothing forbidden.

  `Cache::facts` now takes the path it is being asked about and stamps the
  answer with it. No cache format bump — existing caches heal on read rather
  than being thrown away.

- **The `_` escape hatch exempts a directory that is itself a rule root.**
  ([#30](https://github.com/HenriqueArtur/archwarden/issues/30))

  **This changes what an existing config reports.** A `structure` rule whose
  scope selects a `_`-prefixed directory stops reporting its subfolders. If
  your repository has such a directory and you were relying on those findings,
  they will disappear — check with `config explain <rule-id>`, which lists
  every directory a rule governs.

  `RULES.md` documents the hatch without qualifying it by position, and it was
  consulted for subfolders only: `_` worked on a child of a root and not on a
  root, which is the case the sentence describes best. It was easy to believe
  it worked — the reporter's `_database` held only allowed names, so the rule
  had nothing to say about it either way, and their own documentation recorded
  the exemption as fact.

  Only the directory's own name is asked about, never an ancestor's, so a rule
  rooted *below* an exempt directory still fires. That is what makes a
  namespace expressible: `_Legacy` is exempt, and a rule rooted at
  `_Legacy/*` governs each of its entities normally. Silencing a whole subtree
  is still `skip_dirs.globs` with a `/**`.

- **`impact --apply` refuses a move of a file git does not track**, instead of
  discovering it mid-move.
  ([#28](https://github.com/HenriqueArtur/archwarden/issues/28))

  `git mv` refuses an untracked file *during* the move, after the specifier
  rewrites are on disk. That left importers naming a module that had never
  been created, against decision 16's unconditional promise that a refusal
  means nothing happened — and the recovery the message offered,
  `git checkout .`, is the one thing that cannot restore an untracked file.

  The question is now asked in one `git ls-files` before a byte is written.

- **An installed package whose `exports` offers only platform conditions
  resolves.** ([#21](https://github.com/HenriqueArtur/archwarden/issues/21))

  `bwip-js` maps `.` to `browser`, `electron`, `react-native` and `node`, with
  no `default`, so nothing matched and `exports` blocked the fall back to
  `main`. An installed dependency was reported as an import no boundary rule
  could see. `node` joins the conditions; such imports move from `unresolved`
  to `external` in `summary.imports`.

- **`impact --apply` computes a destination specifier for a package with no
  `exports`.** ([#27](https://github.com/HenriqueArtur/archwarden/issues/27))

  A package without `exports` exports everything, so the new specifier is the
  destination's path under the package root. It used to refuse and suggest
  adding an `exports` map — which no map can do for a package resolving
  subpaths through `index.ts`, and which changes what every consumer may
  import. Whichever of `thing`, `thing/index` and `thing/index.ts` the author
  wrote is what comes back.

### Added

- **`archwarden config verify-rules`** — proves a rule bites, where `explain`
  only shows what it reaches.
  ([#24](https://github.com/HenriqueArtur/archwarden/issues/24))

  ```
  ✓ domain-is-self-contained — fires on `packages/domain/order.ts` importing `apps/api/env.ts`
  ✗ cancelled-by-its-own-except — silent on the same import
  ? usecase-name — not verified: a violation means inventing a filename that
    matches this rule's `file_pattern`, which is a regex run backwards
  ```

  Each rule is handed a synthesised violation of its own terms; nothing is
  written to the repository. Exits non-zero on `✗`, so it belongs in CI beside
  `check`.

  What it cannot do is printed on every run: it proves a rule fires on a
  violation of *its own terms* and cannot know what you meant — a
  `forbid_import_from_packages` list missing an entry is a question about
  intent, and a rule with that hole ticks here. Rules whose violation cannot
  be synthesised are reported as `?` with the reason rather than left out.

- **`archwarden baseline --dry-run`** — says what regenerating would change,
  and writes nothing.
  ([#23](https://github.com/HenriqueArtur/archwarden/issues/23))

  ```
    - domain-needs-spec   apps/api/src/order.ts — no longer occurs
    ~ domain-entity-shape apps/api/src/Domain/user → packages/domain/user
    + domain-forbids-outer apps/api/src/billing.ts — imports `@Infrastructure/Auth`

  .archwarden/baseline.json would change: 1 added, 1 no longer occur, 1 moved.
  ```

  Only `+` is a decision. A finding that only changed path is reported as
  moved — but a prefix mapping must explain **two** pairs before it counts as
  one, so a fix and a new finding that happen to share a folder name are never
  laundered into a move.

### Fixed

- **`tsconfig.paths` is read, and the documentation said in five places that
  it was not.** ([#22](https://github.com/HenriqueArtur/archwarden/issues/22))

  No behaviour change: aliases have resolved since `TsconfigDiscovery::Auto`
  was set, and a boundary rule fires on an aliased import that crosses it. The
  false claim cost a real repository real work — believing the blind spot, its
  author duplicated a boundary by hand into `forbid_import_from_packages`, the
  hand-written list was missing two entries, and imports crossed the boundary
  with the build green.

  Aliases apply by TypeScript's own rule: the nearest `tsconfig.json` to the
  file wins, whole. So an app's alias does not apply to a file in a package,
  and a bare `tsconfig.json` takes the repository's aliases away from
  everything under it unless it `extends` the config that declares them. Two
  tests pin both cases; decision 18 records why the maps are not merged
  repository-wide.

- **`recurse_into` names a container, not a contract.**
  ([#29](https://github.com/HenriqueArtur/archwarden/issues/29))

  No behaviour change: the recursion runs, and a directory inside a container
  is held to the folder list. The description — "subdirectories that carry the
  same structural contract, recursively" — reads as *the contract applies
  inside this folder*, and means the opposite one level down: the container is
  not governed, its children are, and a child's name is not this rule's
  business.

  So naming a container **removes** findings. One repository added a namespace
  holding nineteen entities, cleared nineteen findings in a single run, and
  read it as having modelled the namespace. `config explain <rule-id>` lists
  every directory a rule governs, which is where that decision is visible.

## [0.7.0] — 2026-08-05

### Added

- **`check` names every import that did not resolve**, where it used to report
  only how many.
  ([#18](https://github.com/HenriqueArtur/archwarden/issues/18))

  ```
  note: 2 imports could not resolve, so boundary rules did not see them
        `packages/domain/row.ts`: `@Domain/Order/id`, `@Domain/Order/types`
  0 errors, 0 warnings · 4153 files, 1268 directories · 820ms
  ```

  A boundary rule matches globs against where an import *lands*, so one that
  landed nowhere was never checked — and the note that said so gave the reader
  nothing to open. The person who reported it found theirs by deleting imports
  until the count moved, in a repository of four thousand files.

  It bites hardest where the rule matters most: extracting a package out of an
  app, where imports still written with the app's `tsconfig` aliases resolve to
  nothing and are precisely the ones that cross the boundary being introduced.

  `summary.imports.unresolved_imports` carries every `{path, specifier}` pair,
  for a CI job gating on "no import escapes the rules". The text names the
  first ten files and says how many it left out — a repository whose
  dependencies are not installed cannot place a single bare specifier, and a
  line each would bury the findings the run was for.

  `check --file` reports the same thing for one file, under
  `unresolved_imports`, and no longer answers `is fine.` about a file whose
  imports nothing could see. That is the shape a pre-write hook asks in: the
  import an agent has just written is exactly the one nothing has seen yet.

  Unchanged: `tsconfig` path aliases are still not read
  ([`CONFIG.md`](docs/CONFIG.md)), and an unresolved import is still a note
  rather than a finding. It is the statement that no rule could tell, not a
  rule saying no.

### Changed

- **`check --file` no longer prints `is fine.` for a file whose imports did not
  resolve.** No finding changes and no exit code changes — a run that passed on
  0.6.0 passes on 0.7.0. What changes is one line of text, for the one case
  where it was not true: a boundary rule ran against that file, and ran blind.
  Anything matching on that string, rather than on the exit code or the JSON,
  will see the difference.

## [0.6.0] — 2026-08-03

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

- **A refusal from `impact --apply` now names the file to go and fix.** It had
  one sentence for four different causes, and the sentence it used said
  `tsconfig` path alias. A repository whose `exports` map does not reach the
  destination — the realistic case, and the one issue #11 was filed from — was
  told to look in the wrong file.

  The four causes are now distinct: a `tsconfig` alias this does not read, an
  `exports` map that reaches no subpath at the destination, a file leaving the
  package its specifier names, and an importer at the repository root. Each one
  names a different thing to open.

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

[Unreleased]: https://github.com/HenriqueArtur/archwarden/compare/v0.8.1...HEAD
[0.8.1]: https://github.com/HenriqueArtur/archwarden/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.5.1...v0.6.0
