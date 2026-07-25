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
  "$schema": "https://archwarden.dev/schema/v0.json",
  "version": 0,
  "modules": [ ... ]
}
```

## Top-level shape

```json
{
  "$schema": "https://archwarden.dev/schema/v0.json",
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

The engine performs an AST call-graph walk within the file (following local
function definitions) to check that at least one reachable path from the
top-level export calls `Event.save`. Cross-file call-graph analysis is out
of scope for v0 — the obligation must be satisfied within the file itself.

## Presets

Presets let you share rule sets between projects.

```json
{
  "$schema": "https://archwarden.dev/schema/v0.json",
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

## Config validation commands

Three commands cover the config itself:

- `archwarden config validate` — schema-only. Fast. Fails on structural JSON errors.
- `archwarden config doctor` — semantic. Slower. Reports:
  - regexes that never match any file in the repo,
  - scopes pointing to non-existent paths,
  - `spec-pair` targeting a subfolder not present in the corresponding structure rule,
  - `call-obligation` naming a symbol that no file in scope imports (likely typo),
  - duplicate rule `id`s across the config and presets,
  - unreachable rules (scope fully covered by an `ignore` entry),
  - `disable` naming a rule id that does not exist,
  - a preset declaring `root`,
  - `skip_dirs.scope: "walk"` coexisting with `import-boundary` rules,
  - files targeted by a `naming` rule that export only a default.
- `archwarden config explain <rule-id>` — lists every file the rule currently
  covers and, if applicable, every file it currently flags. Useful for a
  coding agent that needs to understand a rule before writing code.

## Minimal config

The smallest useful config:

```json
{
  "$schema": "https://archwarden.dev/schema/v0.json",
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
