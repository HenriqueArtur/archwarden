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

  "modules": [
    { "id": "domain",       "rules": [ ... ] },
    { "id": "application",  "rules": [ ... ] },
    { "id": "api-routes",   "rules": [ ... ] }
  ],

  "graph": {
    "boundaries": [ ... ]
  }
}
```

- `root` — where to resolve globs from. Defaults to the config file's directory.
- `ignore` — extra ignore globs on top of `.gitignore` (which is always honoured).
- `modules` — logical groupings of rules. A "module" is just a name that
  scopes a set of rules to a set of paths. Naming things helps error
  reporting: findings show `[domain] packages/domain/src/user/wrong-folder/`.
- `graph` — cross-module concerns: import boundaries between layers.

## Rule categories

Every rule has:

- `type` — discriminator (`structure`, `naming`, `spec-pair`, `import-boundary`, `call-obligation`).
- `id` — stable identifier used in output and in `explain`. Required, unique per config.
- `level` — `error` or `warning`.
- `roots` — glob(s) the rule applies to.

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
  "roots": ["packages/application/src/use-cases/*/*"],
  "file_pattern": "^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
  "must_export": {
    "kind": "function",
    "name": "{{pascal(name)}}"
  }
}
```

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
  "spec_suffix": ".spec.ts",
  "ignore_files": [
    "packages/domain/src/nota-fiscal/variants/nfe/services/nfe-service.ts"
  ]
}
```

Optional `require_non_empty_spec: true` fails on `.spec.ts` files that contain
no `it(...)` or `test(...)` calls — this is what enforces "spec written
first", not just "spec file exists".

### Import boundary

```json
{
  "graph": {
    "boundaries": [
      {
        "id": "domain-forbids-application",
        "level": "error",
        "from": "packages/domain/**",
        "forbid_import_from": ["packages/application/**"]
      },
      {
        "id": "ui-forbids-domain-direct",
        "level": "error",
        "from": "apps/**/src/**",
        "forbid_import_from": ["packages/domain/**"],
        "except": ["packages/domain/src/*/types/**"]
      }
    ]
  }
}
```

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
Merging is shallow at the top level; arrays are concatenated; rule `id`
collisions are an error caught by the doctor.

## Config validation commands

Three commands cover the config itself:

- `archwarden config validate` — schema-only. Fast. Fails on structural JSON errors.
- `archwarden config doctor` — semantic. Slower. Reports:
  - regexes that never match any file in the repo,
  - roots pointing to non-existent paths,
  - `spec-pair` targeting a subfolder not present in the corresponding structure rule,
  - `call-obligation` naming a symbol that no file in `roots` imports (likely typo),
  - duplicate rule `id`s across the config and presets,
  - unreachable rules (root fully covered by an earlier ignore).
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
          "subfolders": ["."],
          "spec_suffix": ".spec.ts"
        }
      ]
    }
  ]
}
```
