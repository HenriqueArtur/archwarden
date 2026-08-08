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

- **`must_export.annotation`: a `naming` rule can require the export to write
  its type down.** ([#39](https://github.com/HenriqueArtur/archwarden/issues/39))

  ```json
  "must_export": { "kind": ["const"], "name": "AGENT_TOOL", "annotation": "AgentToolModule" }
  ```

  `export const AGENT_TOOL: AgentToolModule = {...}` passes;
  `export const AGENT_TOOL = {...}` does not. A registry that moves from a
  typed static array to `readdir` plus `import()` loses its compile-time gate:
  the name and the declaration form are still expressible, the shape is not,
  and a module missing `build` is green in `check` and `tsc` and dies at boot.

  Not type checking. Nothing is resolved and nothing is inferred — the
  annotation is a token in the declaration whose `kind` the rule already reads.
  Whether the annotated value really is of that type stays `tsc`'s question;
  what this gates is whether the declaration is submitted to `tsc` at all.

  A binding writes the type after the colon and a class writes it in
  `implements`, so both are read; a class implementing several contracts
  satisfies a rule asking for any of them. Whitespace is not significant on
  either side. A list means "any of". `annotation` beside `kind: ["function"]`
  is a rule no file could satisfy — a function declares a *return* type, which
  is a different claim — so the config is refused rather than left to flag
  every file forever.

  `scaffold` renders the annotated declaration
  (`export const AGENT_TOOL: AgentToolModule = /* ... */;`), which is a line
  that passes — a promise `signature_hint` never made. `signature_hint` is
  unchanged and still never verified.

  Existing configs are unaffected: a rule that names no annotation ignores them
  exactly as before.

- **A `frontmatter` rule: a document's YAML block must carry these keys.**
  ([#44](https://github.com/HenriqueArtur/archwarden/issues/44))

  ```json
  { "type": "frontmatter", "id": "projeto-frontmatter", "level": "error",
    "roots": ["projetos/*"], "file_pattern": "^projeto\\.md$",
    "require": ["id", "nivel", "componentes"],
    "one_of": { "nivel": ["1", "2", "3"] },
    "equals": { "id": "{{raw(dirname)}}" } }
  ```

  The first rule that reads a file which is not code. A `.md`'s frontmatter is
  often not documentation at all — it is the machine-readable half of the
  document — and nothing type-checks a markdown file. A `projeto.md` with no
  `componentes` does not fail to load; it reports as a lesson that needs none.

  `one_of` is the clause that earns the rule. A missing key is an absence; a
  value *outside* the vocabulary is confidently wrong — `status: concluido`
  where the vocabulary is `feito` drops the document out of the generated table
  with no row and no error.

  `equals` is the `naming` question asked of a file with no exported symbol:
  a name agreeing with a path. Values compare as text, so `"1"` matches
  `nivel: 1`.

  Deliberately absent: `type`, `min_items`, nested paths, and anything about a
  value's shape. That is a document schema and JSON Schema is one. The line
  every rule here keeps is names and vocabularies, never shapes.

  A document with no block is a finding, not a skip — otherwise deleting the
  block would be the way out of the rule. A block that is not YAML is a
  different finding, because the next steps differ.

  `---`-fenced YAML only. Reading it takes `yaml-rust2`, the first parsing
  dependency that is not `oxc`: `status: "feito"  # done`, a flow mapping and
  an anchor are all things a line scanner reads wrong in silence.

- **A `pair` rule: a file of one kind must have a companion of another.**
  ([#45](https://github.com/HenriqueArtur/archwarden/issues/45))

  ```json
  { "type": "pair", "id": "licao-tem-notas", "level": "error",
    "roots": ["projetos/*"],
    "file_pattern": "^projeto\\.md$", "must_exist": "notas.md" }
  ```

  `spec-pair` is this rule for one specific pair and cannot be bent to any
  other: its default ignores exclude anything that is not a JS/TS source file,
  and its companion is *derived* — `<stem>.<marker>.<ext>` — which generalises
  to nothing. Two fixed names in one directory is what the rest of the world
  has.

  **The difference from `presence` is the anchor.** `presence` asks about a
  directory: these files must be here. `pair` asks about a file: because this
  one exists, that one must too. An empty directory is a `presence` finding and
  not a `pair` one.

  The companion may leave the directory — `../projeto.md`, for a sketch that
  needs the lesson one level up and may be called anything, which no
  directory-scoped rule can reach. One direction, always: an orphan companion is
  a note taken before the lesson was written, and is not a finding.

  `FileContext` gained an existence predicate for it, supplied by the caller the
  way `siblings` already is — `check` answers from the walk it has, `check
  --file` answers from disk because it has none, and a rule still never touches
  the filesystem itself.

- **A `presence` rule: these files must exist in each governed directory.**
  ([#42](https://github.com/HenriqueArtur/archwarden/issues/42))

  ```json
  { "type": "presence", "id": "licao-completa", "level": "error",
    "roots": ["projetos/*"],
    "require": ["projeto.md", "exercicios.md", "notas.md"],
    "require_any": ["\\.ino$"] }
  ```

  The rule kind `RULES.md` has been deferring by name. `filename_patterns` is a
  whitelist of what *may* exist and is satisfied by an empty directory, which is
  exactly the state this is about: a unit of work is incomplete until its
  companion files are there, nothing errors when one is missing, and the gap is
  found by whoever needed the file.

  The first rule that reasons about a path that is *not* there. It needs no
  parse and no resolution — a name against the walk.

  `require` takes **filenames, not paths**; an entry with a `/` is refused when
  the config compiles, and the same requirement is sayable by a second rule
  scoped one level down. One rule answering for one directory is what lets
  `describe` and `scaffold` answer for a directory that does not exist yet,
  which is where this rule is worth most: `scaffold projetos/17-nova` prints the
  filenames, and a unit of work gets started rather than corrected.

  One finding per missing entry, not one per directory — each is a separate file
  to create, which is how `spec-pair` reports a missing sibling too.

  It is also the cleanest rule `config verify-rules` has: a violation is a
  directory with no files in it, so nothing has to be invented.

- **`why`: a rule, or a module, can say why it exists.**
  ([#46](https://github.com/HenriqueArtur/archwarden/issues/46))

  ```json
  { "type": "import-boundary", "id": "domain-forbids-app", "level": "error",
    "why": "domain is published as its own package and the app is not",
    "from": ["packages/domain/**"], "forbid_import_from": ["packages/app/**"] }
  ```

  A finding said what the rule wanted and what the file did, and never why the
  rule exists. An agent reading one could comply, and that is all it could do —
  which is the failure mode `AGENTS.md` already had three bare prohibitions
  against ("do not edit `arch.config.json` to make a check pass", "a missing
  spec file means write the test", "exit 2 is not your problem to route
  around"). Each of those is a rule broken because the constraint looked
  arbitrary, and a reason is what makes a constraint non-arbitrary.

  There was nowhere to write one. The config is JSON, so it has no comments,
  and the reason lived in a commit message or a wiki — neither in front of
  anybody at the moment a rule fires.

  It surfaces in the pre-write hook's denial, in `describe` and `scaffold`, in
  `agent-guide`, in `config explain`, and beside a finding. In text a rule's
  reason prints **once per run, at its first finding**; in JSON every finding
  carries it. It is not part of a finding's identity, so rewording one never
  touches `.archwarden/baseline.json`.

  A module takes one too, as a separate answer rather than a fallback.

  `config doctor` reports `rules-without-a-reason` as a count, and only once at
  least one rule has a `why` — a project that never used the field has not
  adopted the practice.

- **`structure.subfolder_patterns`: a regex over directory names.**
  ([#43](https://github.com/HenriqueArtur/archwarden/issues/43))

  ```json
  "subfolder_patterns": ["^\\d{2}-[a-z0-9-]+$"]
  ```

  `filename_patterns` one field over, for the other kind of directory entry.
  `allowed_subfolders` constrains names by enumeration, which works for a fixed
  vocabulary and cannot work for an open set where the *shape* is the rule —
  sixteen lesson folders named `NN-slug` with more arriving, and nobody listing
  them forever.

  The same matcher already existed as `naming.dir_pattern` and was reachable
  only through `must_export`, which needs a TypeScript parse of a file inside;
  a directory with no `.ts` near it could not use it at all.

  A union with the two lists: a name passes if a list names it *or* a pattern
  matches it. The lists are read first, so a `warn_subfolders` entry whose name
  happens to have the right shape still warns — the most specific declaration
  wins, and a name written out is more specific than a regex.

  `describe`, `scaffold` and `agent-guide` all carry it, so the shape is
  answerable before the folder exists, which is where a folder-naming
  convention is cheap to follow.

### Fixed

- **Source in a language archwarden cannot read is counted, instead of passing
  in silence.**

  A `.py` under an `import-boundary` rule was classified `Other` — the class
  that exists so a PNG does not inflate `checks_skipped` — so the rule saw no
  imports, reported nothing and counted nothing. A rule enforcing nothing looks
  exactly like a repository that satisfies it.

  `FileClass` now has four answers, and a missing fact counts as a lost answer
  only when the file could have carried it: a boundary rule pointed at a `.md`
  lost nothing, and pointed at a `.py` lost everything. `check --file` gained a
  `no-front-end` reason distinct from `not-source` — one means the rule is
  pointed at the wrong thing, the other means the rule is right and archwarden
  cannot read the file.

- **`allowed_subfolders: []` now forbids every subfolder, instead of enforcing
  nothing.** ([#40](https://github.com/HenriqueArtur/archwarden/issues/40))

  **This can make an existing, unchanged config report differently** — but only
  one that wrote the empty list explicitly, which today enforces nothing and is
  therefore a config whose author meant something by it. A rule that *omits*
  the field is untouched, and that is every rule that constrains filenames
  only.

  The distinction is the fix: absent and `[]` used to arrive identical, so the
  literal reading — a list of what may exist, holding nothing — could not be
  given to one without giving it to the other. `allowed_subfolders` is now an
  option. Omitted, the rule says nothing about subfolders; `[]` permits none of
  them, which is how a directory says it is a leaf and was previously
  unsayable.

  `config doctor` gained `rule-constrains-nothing` for a `structure` rule that
  names no allowed subfolder, no warned subfolder and no filename pattern —
  the state that used to be valid at `validate`, silent at `doctor` and skipped
  at `check`, all three agreeing a rule was fine while it enforced nothing.

- **`config explain` no longer refers to `config doctor` for an answer `doctor`
  does not have.** ([#41](https://github.com/HenriqueArtur/archwarden/issues/41))

  "It covers nothing in this repository. Try `archwarden config doctor` for
  why." merged two different faults and sent users to a command that had
  nothing to say about one of them. They are now separate sentences:

  - the scope matched no path — *"Its scope matches no path in this
    repository"*, and the referral stays, because `doctor` does have that one;
  - the scope matched and the rule asks nothing of what it matched — *"It
    constrains nothing: its scope reaches 3 paths, and the rule has no
    requirement about any of them"*, said by `explain` itself. It is the
    command that decided, so it is the command that says why.

### Changed

- **A `naming` finding about an export that exists now carries a span**, so it
  prints as `path:line:column` and opens in an editor. Findings about an export
  that is *missing* still have none — there is no position to name.
- The facts cache format is at version 4. The first run after upgrading
  re-parses; nothing else changes.

## [0.9.2] — 2026-08-07

### Fixed

- **An aliased import that reaches its file through a directory `index.ts` is
  rewritten.** ([#36](https://github.com/HenriqueArtur/archwarden/issues/36),
  reopened)

  0.9.1 compared the specifier against the path with its extension and without
  it, and `@Infra/Ent/Card/types` reaching `Ent/Card/types/index.ts` is neither:
  the `*` captured from the specifier was one component shorter than the one
  captured from the path, so it never matched and every directory-index import
  refused. A repository that writes `Entities/Card/types/index.ts` has those
  everywhere.

  Three spellings reach one file — `types`, `types/index` and
  `types/index.ts` — and all three are now recognised, with the destination
  written in whichever form the author used.

  The reopened report blamed the tsconfig's *location*, since the working
  fixture had it at the repository root and the failing one did not. That was
  not it: a tsconfig in a subdirectory works, and 0.9.1 shipped a test for it.
  What the two runs actually differed by was the directory `index.ts` — which
  the report's own evidence pointed at, by quoting two different refusal
  messages.

## [0.9.1] — 2026-08-07

### Fixed

- **`impact --apply` rewrites an aliased specifier when the importer's own
  alias still covers the destination.**
  ([#36](https://github.com/HenriqueArtur/archwarden/issues/36))

  It refused any move whose importer reached the file through a `tsconfig`
  path alias. `paths` is not invertible in general — several patterns may
  reach one file — so the refusal was right in general and wrong for the case
  that dominates a rename: `@Lib/* → ./src/lib/*` covers both
  `src/lib/thing.ts` and `src/lib/renamed.ts`, so `@Lib/thing` → `@Lib/renamed`
  is determined.

  The entry that reaches the file being moved is the entry that produced the
  specifier, so re-running *that* pattern against the destination computes
  rather than chooses. Everything else still refuses: a destination outside
  what the alias covers, an entry that names one file rather than a subtree,
  or aliases that could not be read.

  An entry with no `*` needs no special case and gets none — `"@Env":
  ["./src/Env.ts"]` names one file, the destination is a different file, so it
  does not match. That case is the reason this reads the real map rather than
  transposing strings: a string-level guess would have rewritten `@Env` to
  `@Environment` and produced a repository that does not build.

  `extends` is followed only as far as the first config that declares `paths`.
  `oxc_resolver` merges `extends` properly for resolution and keeps that
  private; a config whose aliases arrive another way finds none here and the
  move refuses exactly as before.

- **A legitimate refusal no longer also reports itself as a bug in
  archwarden.**

  The guard for "an importer the dry run named got no edit" fired alongside
  the refusal that had just explained why, so a single unrewritable specifier
  produced two messages about one import — the second asking the reader to
  report a bug. The guard is for the *unexplained* case and now skips
  importers a refusal already accounts for.

## [0.9.0] — 2026-08-06

A minor for a bug fix, because it changes what an existing, unchanged config
reports — and in the direction of reporting files that were never checked.

### Changed

- **`spec-pair` covers files below a named subfolder, and a nested path in
  `subfolders` means what it looks like.**
  ([#34](https://github.com/HenriqueArtur/archwarden/issues/34))

  **This changes what an existing config reports.** A repository that groups
  related files into a folder under a named subfolder will gain findings on the
  next run. They are files the gate was never applied to.

  An entry was compared against a directory's *name*, so only a direct child
  was covered:

  - `subfolders: ["calcs"]` reached `Entity/calcs/direct.ts` and never
    `Entity/calcs/group/nested.ts`;
  - `subfolders: ["calcs/group"]` could not equal a single component, so it was
    accepted by the schema, reported valid by `config validate`, shown as
    unchanged coverage by `config explain`, and matched nothing.

  An entry now names a directory relative to the selected one and covers it and
  everything below it. `["."]` is unchanged and stays non-recursive — naming
  `calcs` is how a project says which subtree is under the gate, and a
  recursive `.` would swallow `types` and everything else it did not name.

  The cost of the old behaviour, measured in the reporting repository: two
  entities grouped their validation steps the same way, one had thirteen files
  and thirteen specs, the other eleven files and no test at all. Neither the
  report nor the baseline had ever mentioned it. Sixteen files sat in that
  blind spot.

  Run `archwarden baseline --dry-run` after upgrading to see what appears
  before deciding what to do about it.

### Fixed

- **The nightly job is gone; the differential tier runs on CI.**

  It had failed every night for at least six days with
  ``ARCHWARDEN_DIFF_REPO=`` is not a readable path``: an unset repository
  variable substitutes as the empty string, `env::var` succeeds with `""`, and
  the empty path cannot be canonicalised — while the test file promised a
  missing target "prints why it did nothing and passes". An empty value is now
  the same state as an unset one, with a test for it.

  The job moved into CI rather than being repaired in place, for the reason
  `docs/TESTING.md` already gives about mutation testing: a red job nobody
  opens is not a test, it is a habit of ignoring red. Nothing in this project
  runs on a schedule any more.

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

[Unreleased]: https://github.com/HenriqueArtur/archwarden/compare/v0.9.2...HEAD
[0.9.2]: https://github.com/HenriqueArtur/archwarden/compare/v0.9.1...v0.9.2
[0.9.1]: https://github.com/HenriqueArtur/archwarden/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.8.1...v0.9.0
[0.8.1]: https://github.com/HenriqueArtur/archwarden/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.5.1...v0.6.0
