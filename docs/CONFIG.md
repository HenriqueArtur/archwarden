# Configuration

archwarden reads exactly one config file: `arch.config.json` at the repo root.

## Discovery

Running `archwarden <command>` walks up from the current working directory
looking for `arch.config.json`. The first one found wins. This mirrors how
`git` finds `.git` and how `biome` finds `biome.json`.

Consequence: in a monorepo, you can run archwarden from any subpackage and
it will still analyse the whole repo, using the root config as the source
of truth.

If you must override discovery, pass `--config path/to/config.json`.

### `--config` and `--root` are two questions

`--config` answers *where the rules are*. It also answers *what they are about*,
because globs resolve from the config file's own directory — which is right for
the config a repository carries, and wrong for one kept anywhere else.

`--root` separates them:

```bash
archwarden check --config ../experiments/stricter.json --root .
```

Without it, a config outside the repository would take its own directory to be
the repository, walk it, find no TypeScript and report a clean run — exit 0, no
findings, and the question answered with the one wrong answer a reader takes as
good news. So that case is **exit 2** with a message naming this flag.

The refusal is narrow: an empty root you are *standing in* is checked normally,
because a repository that has just run `archwarden init` is empty and the very
next `check` must not claim the setup is broken. What is never legitimate is an
empty root reached only through a config file's location.

This is what makes "how many findings would this stricter rule produce?"
answerable without editing the file the project committed — see
[Measuring a rule change](#measuring-a-rule-change) below.

## Format

JSON. Not YAML, not TOML, not JS. Reasoning is in [`DECISIONS.md`](DECISIONS.md).

Every file starts with a `$schema` reference so editors give autocomplete
and inline validation without a plugin:

```json
{
  "$schema": "https://raw.githubusercontent.com/HenriqueArtur/archwarden/main/schema/v0.json",
  "version": 0,
  "modules": [ ... ]
}
```

**Where archwarden is installed from npm, point at the copy on disk instead:**

```json
{
  "$schema": "./node_modules/archwarden/schema/v0.json"
}
```

`archwarden init` writes this form automatically when it finds an install. It
is the schema for the version in your lockfile, it works offline, and it cannot
describe a different build than the one you are running — a URL can only ever
serve one version, and it will not always be yours.

The schema itself is generated from the Rust types that parse the config, and
CI fails if the committed copy drifts from them (`cargo xtask check-schema`).
A field that exists in the parser but not in the schema is a field your editor
would refuse to complete.

## Top-level shape

```json
{
  "$schema": "https://raw.githubusercontent.com/HenriqueArtur/archwarden/main/schema/v0.json",
  "version": 0,

  "root": ".",

  "ignore": [
    "**/node_modules/**",
    "**/dist/**",
    "**/.next/**"
  ],

  "skip_dirs": {
    "prefixes": ["_"],
    "globs": [],
    "scope": "structure"
  },

  "modules": [
    { "id": "domain",       "rules": [ ... ] },
    { "id": "application",  "rules": [ ... ] },
    { "id": "api-routes",   "rules": [ ... ] }
  ],

  "rules": [ ... ]
}
```

- `root` — where to resolve globs from. Defaults to the config file's directory.
- `ignore` — extra ignore globs on top of `.gitignore` (which is always
  honoured). Ignore always wins over a rule's scope, however specific that
  scope is.
- `skip_dirs` — the `_`-prefix escape hatch, see [`RULES.md`](RULES.md).
- `modules` — logical groupings of rules. A "module" is just a name that
  scopes a set of rules to a set of paths. Naming things helps error
  reporting: findings show `[domain] packages/domain/src/user/wrong-folder/`.
- `rules` — rules that belong to no particular module, typically import
  boundaries (which are cross-module by nature). They report as `[*]`.

## Rule categories

Every rule has:

- `type` — discriminator (`structure`, `naming`, `spec-pair`, `import-boundary`, `call-obligation`).
- `id` — stable identifier used in output and in `explain`. Required, unique per config.
- `level` — `error` or `warning`.
- a **scope**: `roots` on every rule, except `import-boundary` where it is
  called `from`. Scope globs select **directories** — see
  [`RULES.md`](RULES.md) for what each rule then inspects inside them.

Every glob field accepts a single string or an array of strings:
`"roots": "src/**"` and `"roots": ["src/**"]` are the same.

### A note on regexes

Regex fields (`file_pattern`, `filename_patterns`) are matched with Rust's
`regex` engine, which guarantees linear-time matching. It does **not** support
lookahead, lookbehind, or backreferences — a deliberate trade, because
archwarden runs inside pre-commit hooks and agent pre-write hooks, where a
catastrophically backtracking pattern would be a denial of service on your own
workflow. Named capture groups (`(?<name>...)`) work normally.

`archwarden config validate` reports unsupported constructs with a message
saying so, rather than a raw engine error.

### Unknown fields are refused

A key archwarden does not recognise is an error, not something ignored:

```
× arch.config.json is not a valid archwarden config: at `rules[0]`:
  unknown field `allow`, expected one of `id`, `level`, `roots`,
  `allowed_subfolders`, `warn_subfolders`, `recurse_into`, `filename_patterns`
```

A misspelled key would otherwise compile to a rule that constrains nothing,
which `validate` would call valid and `check` would report as a clean
repository. A rule that silently enforces nothing is the worst failure a linter
has, because it is indistinguishable from a rule that passes.

The published JSON Schema says the same (`additionalProperties: false`), so an
editor with `$schema` wired up flags the typo before archwarden runs.

The cost is that a config written for a newer archwarden is **refused** by an
older one rather than degrading. That is the intended trade: a config file is
small, versioned by its `version` field, and a wrong guess about what a key
means is worse than an error.

The five rule types are specified in [`RULES.md`](RULES.md). This section
shows realistic examples for each.

### Structure rule

Ported from Flowmaatik's `check-structure.config.ts`:

```json
{
  "type": "structure",
  "id": "domain-entity-shape",
  "level": "error",
  "roots": ["packages/domain/src/*"],
  "allowed_subfolders": [
    "types", "calcs", "actions", "services",
    "mocks", "repositories", "const", "variants"
  ],
  "warn_subfolders": ["shared", "adapters"],
  "recurse_into": ["variants"]
}
```

### Filename rule

```json
{
  "type": "structure",
  "id": "api-route-filenames",
  "level": "error",
  "roots": ["apps/app/src/app/api/**"],
  "filename_patterns": [
    "^route\\.ts$",
    "^route\\.(get|post|put|patch|delete|options)\\.ts$",
    "^route\\.(get|post|put|patch|delete|options)\\.spec\\.ts$",
    "^route\\.(get|post|put|patch|delete|options)\\.factory\\.ts$",
    "^DOC\\.md$"
  ]
}
```

### Naming coupling

```json
{
  "type": "naming",
  "id": "usecase-factory-name",
  "level": "error",
  "roots": ["packages/application/src/use-cases/*"],
  "file_pattern": "^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
  "must_export": {
    "kind": "function",
    "name": "{{pascal(name)}}",
    "signature_hint": "(deps: {{pascal(name)}}Deps): UseCase<{{pascal(name)}}Input, {{pascal(name)}}Output>"
  }
}
```

Note the scope: `use-cases/*` selects each use-case *directory*, and
`file_pattern` then matches files directly inside it. `signature_hint` is
never verified — it only makes `scaffold` output realistic.

`{{pascal(name)}}` is a small templating helper: the named capture group
`name` from `file_pattern` gets fed to a case transformer. Supported:
`pascal`, `camel`, `kebab`, `snake`, `upper`, `lower`, `raw`.

### Spec pairing (TDD gate)

```json
{
  "type": "spec-pair",
  "id": "domain-calcs-need-spec",
  "level": "error",
  "roots": ["packages/domain/src/*"],
  "subfolders": ["calcs", "services", "adapters"],
  "spec_markers": ["spec", "test"],
  "ignore_files": [
    "packages/domain/src/nota-fiscal/variants/nfe/services/nfe-service.ts",
    "packages/domain/src/**/*.types.ts"
  ]
}
```

`ignore_files` takes globs, so both an exact path and a pattern work.

`spec_markers` defaults to `["spec", "test"]` and can usually be omitted: it
is what vitest and jest both accept. The extension is never configured — it
comes from the source file, so `Component.tsx` pairs with
`Component.spec.tsx`. See [`RULES.md`](RULES.md) for how a compound name like
`user.db.repository.ts` is handled.

Optional `require_non_empty_spec: true` fails on `.spec.ts` files that contain
no `it(...)` or `test(...)` calls — this is what enforces "spec written
first", not just "spec file exists". A `describe(...)` alone does not satisfy
it.

### Import boundary

An ordinary rule, with `from` as its scope field. Boundaries are cross-module,
so they normally live in the top-level `rules` array:

```json
{
  "rules": [
    {
      "type": "import-boundary",
      "id": "domain-forbids-application",
      "level": "error",
      "from": "packages/domain/**",
      "forbid_import_from": ["packages/application/**"]
    },
    {
      "type": "import-boundary",
      "id": "ui-forbids-domain-direct",
      "level": "error",
      "from": "apps/**/src/**",
      "forbid_import_from": ["packages/domain/**"],
      "except": ["packages/domain/src/*/types/**"]
    }
  ]
}
```

There is no `graph` key. Boundaries are rules like any other, so they go
through the same matcher, the same `describe_expectation()`, and show up in
`describe` and `agent-guide` with no special-casing — which is what keeps
those commands in lockstep with the checker (decision 9).

### Call obligation

The semantic rule that no lint plugin does well:

```json
{
  "type": "call-obligation",
  "id": "non-get-routes-must-audit",
  "level": "error",
  "roots": ["apps/app/src/app/api/**"],
  "file_pattern": "^route\\.(post|put|patch|delete)\\.ts$",
  "must_call": {
    "symbol": "Event.save",
    "imported_from": "@flowmaatik/domain/event"
  }
}
```

The obligation is satisfied when the call appears **anywhere in the file**.
That includes a local helper the export delegates to, which is the case this
rule has to get right — demanding the call at the top level would fire on
well-factored code.

It deliberately stops there. A file that calls `Event.save` only from a
function nothing reaches still passes, in the same way `RULES.md` declines to
filter calls inside `if (false)`: archwarden is a structural linter, not a
reachability analyser, and a rule that were sometimes right about dead code
would be harder to trust than one that is never asked.

Cross-file analysis is out of scope for v0 — the obligation must be satisfied
within the file itself.

## Presets

Presets let you share rule sets between projects.

```json
{
  "$schema": "https://raw.githubusercontent.com/HenriqueArtur/archwarden/main/schema/v0.json",
  "version": 0,
  "extends": ["@myorg/arch-preset-clean-arch"],
  "modules": [
    { "id": "project-specific", "rules": [ ... ] }
  ]
}
```

A preset is any published package whose entry point is a JSON file matching
the config schema. Local presets work too: `"extends": ["./presets/base.json"]`.

**Resolution.** A `./`-prefixed entry is a path. Anything else is an npm
package name, resolved with the same resolver archwarden uses for imports
(`oxc_resolver`), so npm, yarn classic, pnpm, and yarn PnP layouts all work
without special handling.

**Merging.**

- Arrays (`modules`, `rules`, `ignore`, `extends`) are concatenated.
- Scalars (`root`, `version`) — the local config wins over any preset.
- A preset declaring `root` is an error. A preset cannot know your repo
  layout, and silently relocating every glob in the config is not something
  a shared package should be able to do.
- Rule `id` collisions are an error caught by the doctor.

**Removing an inherited rule.** A top-level `disable` list drops rules that
came from a preset:

```json
{
  "extends": ["@myorg/arch-preset-clean-arch"],
  "disable": ["clean-arch-no-barrel-files"]
}
```

Without this, one unwanted rule makes a whole preset unusable. Disabling an
id that does not exist is a doctor error, so a typo fails loudly instead of
silently disabling nothing.

## Adopting archwarden in an existing repository

The first run on a repository that did not grow up with these rules reports
everything at once — on one real project, 32 errors and 46 warnings. That
leaves two bad choices: keep archwarden out of CI, where the rules rot, or put
it in and teach everyone to ignore red.

```bash
archwarden baseline     # writes .archwarden/baseline.json
git add .archwarden/baseline.json
```

`check` now reports only findings that are not in it. The build is green today
and fails at the next *new* violation.

**Commit the file.** Each line is debt the project has decided to carry, and a
line added in a pull request is a visible decision — reviewed like any other.
That is the whole difference between a baseline and a suppression file.

Every run says where it stands:

```
0 errors, 0 warnings · 3778 files, 1034 directories · 593ms
78 accepted, 12 no longer occur — run `archwarden baseline` to update
```

The second number is the ratchet. Fixing accepted debt is reported, and the
entries become removable — without which, fixing a violation and reintroducing
it later would be hidden by the stale entry.

`archwarden check --no-baseline` reports everything again, for when the
question is "how bad is it really".

**Unlike the filters below, a baseline changes the exit code.** That is what it
is for, and why it is a committed file rather than a flag.

### What counts as the same accepted finding

The rule and the path, and nothing else.

Not the level: promoting a rule from `warning` to `error` is the project
raising its own bar on debt it already acknowledged.

Not the detail: renaming a disallowed folder from `handlers` to `controllers`
is not a new violation, and treating it as one would make the file churn on
every rename. The cost is a case this deliberately does not catch — fixing a
violation and breaking differently *at the same path under the same rule* stays
accepted.

And no timestamp, so regenerating an unchanged repository produces no diff.
`git blame` on the file already says when each line arrived and who wrote it.

The pre-write hook respects the baseline too: an agent editing a legacy file is
not blocked by debt that is not its own.

## Filtering the report

Four flags on `check` decide what is **printed**. None of them decides what is
**checked**: every rule runs, every finding is computed, and the exit code is
identical with them and without. That is what makes one safe to leave in a
command that gates a build.

```bash
archwarden check --summary                        # per-rule counts, no listing
archwarden check --rules domain-entity-shape      # one rule's findings
archwarden check --paths 'packages/domain/**'     # one area of the repo
archwarden check --level error                    # warnings are known debt
archwarden check --changed                        # uncommitted work
archwarden check --changed main                   # everything this branch does
```

`--changed` asks git which files differ from a ref, defaulting to `HEAD`.
Untracked files count; gitignored ones do not. So do the directories those
files live in, because a `structure` finding names the directory rather than
the file that brought it into existence.

It is a filter like the rest, which is the point: decision 12 says `check`
covers the repository, and a `--changed` that narrowed what is *evaluated*
would let a pull request touching only `apps/web` pass with a regression
sitting in `packages/domain`. Here the build still fails; the report just shows
the part you asked about, and `hidden` says how much it left out.

For the same reason it is not "fail only on new violations". That is a
baseline — a committed record of accepted debt — and it is a different feature.

`--rules` and `--paths` are repeatable and comma-separated; `--rules a,b` and
`--rules a --rules b` are the same thing. All four compose with AND. `--paths`
matches against the finding's path through the same glob engine `ignore`,
`roots` and `forbid_import_from` use — there is only one matcher.

**An entry with no glob character in it is a path, not a pattern**, and selects
that path and everything under it. The path a reader has to hand is the one
they just copied out of a finding, and having to remember `/**` would turn
"look closer at this" into an empty report.

```bash
archwarden check --paths packages/domain/src/order     # that directory and below
archwarden check --paths 'packages/domain/src/*'       # exactly one level
```

An entry that *does* contain a glob is used exactly as written. Someone who
wrote `src/*` means one level, and widening it to `src/*/**` would be
archwarden overruling them.

`--summary` prints one row per rule, worst first: errors descending, then
warnings, then by id.

```
domain-entity-shape  3 errors
types-need-spec      3 errors
app-shape            1 error
calcs-need-spec      3 warnings

7 errors, 3 warnings · 8 files, 20 directories · 1ms
```

`--by path` counts the same findings by area of the repository instead, and
implies `--summary`:

```
packages/domain/src/invoice  2 errors, 1 warning
packages/domain/src/order    2 errors, 1 warning
packages/domain/src/client   1 error, 1 warning
```

The areas are the directories the rules' own scopes already select — a config
saying `roots: packages/domain/src/*` has declared that
`packages/domain/src/order` is a unit, so nothing here has to choose a depth. A
finding no scope reaches keeps its own path rather than being dropped or filed
under a heading that means nothing.

The two answer different questions. `--summary` says which rule is dominating
the output; `--by path` says which part of the repository is furthest from the
rules, which is the one that says where to start. Unlike the rule breakdown,
only areas with findings get a row: printing every clean directory in a
monorepo would bury the ones that are not.

A rule that found nothing keeps its row with a `0`. That it was evaluated is
an answer; a missing row would read as a rule someone disabled. `--rules`
narrows the rows — it is the one filter that names rules — while `--paths` and
`--level` leave every row in place.

In `--format json`, `--summary` adds a `by_rule` map beside the counts and
**omits the `findings` array**. A summary that still emitted every finding
would give a piping consumer no size benefit, which is most of the reason to
ask for one.

Two behaviours worth knowing:

- **The counts describe what you asked to see.** `0 errors` beside exit code 1
  is possible and correct: the gate counts what was evaluated. `summary.hidden`
  and a `note:` line in the text output say how many findings the filter
  removed, so the two are always reconcilable.
- **An unknown rule id is an error (exit 2)**, not an empty report — the same
  way `disable` and `config explain` refuse one. A filter that silently matched
  nothing would look exactly like a clean repository, which is the one wrong
  answer a user reads as good news.

## Before you move a file

```bash
archwarden impact packages/domain/src/order/calcs/total.ts \
           --to  packages/app/src/billing/total.ts
```

An editor moves a file and rewrites its imports. It says nothing about whether
the destination is somewhere the architecture allows the file to be, or whether
the move puts an existing import across a boundary. That half is this one, and
it is the half nothing else answers.

It reports which rules would start and stop applying, which files import the
target and which of those imports would *newly* be forbidden, how many of the
file's own relative imports would need rewriting — and which files contain a
dynamic import it cannot read.

That last one matters. `import(name)` names no single module, so archwarden
records nothing for it: right for a rule, which must not report a path nobody
wrote, and wrong for a question about who imports a file. Those files are
listed separately, because a confident answer with a hole in it is worse than
an incomplete one that says so.

"Newly" is doing work too. A boundary already being crossed is debt `check`
reports today, not a consequence of the move, and listing it here would blame
the move for something it did not do.

Relative imports are counted, not re-resolved: whether they still point
somewhere afterwards is a question `tsc` answers better.

Reading the import graph backwards means resolving the whole repository, so
this costs about what a `check` costs.

### Carrying it out

`--apply` does the move. Dry run stays the default, and this is a second,
explicit word.

```bash
archwarden impact packages/domain/src/id/shared/is-id-invalid-shared.ts \
           --to  packages/domain/src/id/calcs/is-id-invalid.ts --apply
```

Files move with `git mv`, so history follows them. Every import specifier that
named the file is rewritten — **including the ones written by package name**,
which is the half an editor cannot do: to an editor, `@org/domain/email/x` is a
package like `react`. In the repository this was built against, that is the
majority of imports.

The spec sibling travels with its unit file, and follows a rename:
`is-id-invalid-shared.spec.ts` becomes `is-id-invalid.spec.ts`. Leaving it
behind would break archwarden's own `spec-pair` rule.

A source directory the move empties is removed, because `structure` rules are
about directories and an emptied `shared/` would keep reporting the finding the
refactor was run to remove.

**The exported symbol is not renamed.** A file renamed mid-move keeps
`isIdInvalidShared`, and the output says so. Renaming an export breaks every
caller in a way this cannot see; `check` reports the mismatch afterwards, which
is where a `naming` rule belongs.

### A whole layer at once

A directory or a glob as the source makes `--to` relative to **each matched
directory**:

```bash
archwarden impact 'packages/domain/src/*/shared' --to '../calcs' --apply
```

Every `shared` becomes the `calcs` beside it. Files nested inside a match land
in the destination directly — `feature/shared/consts/list-shared.ts` goes to
`feature/calcs/`, not to `feature/shared/calcs/`. Two files landing on one path
is refused before anything is written.

One file keeps the other reading: `--to` is the whole destination path, which
is what makes renaming during a move expressible at all.

### What it refuses, and why a refusal is safe

Everything is computed and validated before a byte is written, so **a refusal
means nothing happened** — there is no state where half the imports are
rewritten.

| refusal | why |
|---|---|
| the working tree is dirty | `git` is the undo, and one that takes your own work with it is not one |
| not a git repository | same reason: no undo |
| a specifier this cannot recompute | a `tsconfig` path alias resolves through a map archwarden does not read; rewriting the rest would leave that one pointing at nothing |
| the destination exists, or two files land on one path | carrying it out would delete something |
| a dynamic import naming no module | whether that file imports the target is unknowable |

Only the last is overridable. `--force` is a human saying they looked, and the
report prints the file and the line to look at. The others produce a repository
that does not build, which is not a judgement a flag should be able to make.

## Measuring a rule change

Rule 2 of [`AGENTS.md`](AGENT-INTEGRATION.md) says not to edit `arch.config.json`
to make a check pass. Planning to *tighten* a rule needs the opposite of that
and looks identical from outside: you have to change the file to find out what
changes. A config kept somewhere else answers without persisting anything.

```bash
cp arch.config.json /tmp/stricter.json
# edit /tmp/stricter.json — drop `shared` from warn_subfolders
archwarden check --config /tmp/stricter.json --root . --summary
```

```
domain-entity-shape                        7 errors, 2 warnings
domain-actions-should-have-spec           37 warnings
domain-calcs-services-adapters-need-spec   0
domain-variants-calcs-services-need-spec   0
```

Seven errors, and `config explain` on the id says which paths. Nothing was
written, so there is no config change to remember to revert — which is the
difference between measuring a decision and making one.

`--root .` is not optional here; without it the run refuses. See
[`--config` and `--root` are two questions](#--config-and---root-are-two-questions).

## Does this folder have a reason to exist?

```bash
archwarden orphans                                  # every folder
archwarden orphans 'packages/domain/src/**/shared/**'
archwarden orphans packages/domain/src/order --by-file
```

For every file: who imports it, and whether from inside the module it lives in,
from outside it, or nobody. Aggregated by folder.

```
packages/domain/src/flow-node/shared/calcs     2 files   inside-only 0   outside-only 2   both 0   nobody 0
                                               → only used from outside its module — the boundary is drawn elsewhere
packages/domain/src/feature/shared/consts      1 file    inside-only 0   outside-only 0   both 1   nobody 0
```

Three shapes, three meanings:

- **Only from outside** — nothing in the module it sits in needs it. It belongs
  to its callers, not to its parent, and the boundary is drawn in the wrong
  place.
- **Only from inside** — part of how the module works rather than of what it
  offers. It should be private.
- **Nobody** — dead, or reached only through a dynamic import. Those files are
  listed at the end, because a folder above may be reached from one without
  showing it.

A folder that is a mix gets no verdict. That is a folder nobody has decided
about, and a sentence claiming otherwise would be the tool guessing.

**"Module" is the area the config already declares** — the same directories
`check --by path` counts by. A config with `roots: packages/domain/src/*` gets
one module per entity, so a `shared/` and a `calcs/` under the same entity are
*inside* each other. Nothing here picks a depth the config did not.

**Specs are left out of the graph, both ways.** A spec is an entry point for a
test runner, so counting it as an importee puts a phantom dead file in every
folder. Counting it as an *importer* is worse: a file's own spec sits in the
same module, so every file with a spec reads as used from inside and outside at
once. On one real repository that turned six `shared/` folders — every one of
them used only from other modules — into six folders marked "both".

**This is not Knip.** Knip finds exports nobody uses. The question here is where
the importers come from for the exports that *are* used. The "nobody" column
does overlap; the other two are what this exists for.

It resolves the whole repository, so it costs about what a `check` costs.

## Config validation commands

Three commands cover the config itself:

- `archwarden config validate` — schema-only. Fast. Fails on structural JSON errors.
- `archwarden config doctor` — semantic. Answers "does this config mean what
  you think?", where `validate` only answers "does it mean anything?".

  Three of the checks originally listed here — duplicate rule `id`s, `disable`
  naming a rule that does not exist, and a preset declaring `root` — are **hard
  errors** when the config loads, not doctor findings. That is strictly better:
  a typo fails where the user is looking, rather than in a command they may
  never run.

  Answerable from the config alone:
  - unreachable rules (scope fully covered by an `ignore` entry),
  - `skip_dirs.scope: "walk"` coexisting with `import-boundary` rules,
  - `spec-pair` targeting a subfolder the corresponding structure rule forbids,
  - a `signature_hint` written in a style the rule's `kind` does not accept.

  Answerable only against the repository:
  - regexes that never match any file,
  - scopes pointing to non-existent paths,
  - `call-obligation` naming a symbol that no file in scope imports,
  - files targeted by a `naming` rule that export only a default.

  Every finding carries a code, a sentence, and a fix. The command exits 0 even
  with findings: they are advice about a configuration, not findings about
  code, and a non-zero exit would put a deliberate choice into a CI gate.
- `archwarden config explain <rule-id>` — lists every path the rule currently
  covers and every one it currently flags, one line each. This is the compact
  answer to "which paths did that rule flag?" after a `--summary`.

`archwarden describe` asks the same question from the other end. Given a glob
rather than a path, it answers for every path that matches:

```
$ archwarden describe 'packages/domain/src/*'
Rules that apply under `packages/domain/src/*`:

  packages/domain/src/invoice  domain-entity-shape
  packages/domain/src/order    domain-entity-shape, calcs-need-spec

2 paths, 2 rules.
```

Only paths that exist, necessarily — a glob can match nothing else. Asking
about a single path still answers for one that does not exist yet, which is
most of what `describe` is for.

## Minimal config

The smallest useful config:

```json
{
  "$schema": "https://raw.githubusercontent.com/HenriqueArtur/archwarden/main/schema/v0.json",
  "version": 0,
  "modules": [
    {
      "id": "src",
      "rules": [
        {
          "type": "spec-pair",
          "id": "src-needs-spec",
          "level": "error",
          "roots": ["src/**"],
          "subfolders": ["."]
        }
      ]
    }
  ]
}
```
