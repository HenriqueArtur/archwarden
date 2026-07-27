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
