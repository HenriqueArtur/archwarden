# archwarden

Fast, declarative architecture linter for TypeScript and JavaScript projects. Written in Rust.

archwarden enforces the rules your project already has but nobody remembers:
which folders may exist under a module, which files must be paired with a spec,
which layers may import which, and which files must call which functions.

It is a single binary. It reads one JSON config from your repo root. It runs
in milliseconds on caches. It is meant to be paired with [Biome](https://biomejs.dev/)
for formatting and code-style — archwarden does not overlap with Biome.

## Status

Design phase. No code yet. See [`docs/ROADMAP.md`](docs/ROADMAP.md).

## Why

Growing codebases accumulate architectural conventions faster than humans (and
coding agents) can remember them. The usual outcomes:

- A file lands in the wrong folder because nobody knew the folder scheme.
- A use-case ships without its `.spec.ts` sibling because the TDD rule is tribal.
- A `POST` route forgets to persist an audit event because the obligation was in a Notion doc.
- A UI component imports from the domain layer because nothing blocked it.

Existing tools cover parts of this. `dependency-cruiser` covers import graphs
well but is JS and slow on very large repos. ESLint boundaries plugins cover
imports at lint time but do not express structural or process rules. No single
tool covers filename-to-export coupling, structural TDD gates, and call
obligations together, with one config, at Rust speed.

archwarden aims to be that single tool, and to be equally usable by humans
and by coding agents.

## What it does

Five rule categories in v0:

1. **Structure rules** — allowed subfolders per module, filename regex, folder shape.
2. **Naming coupling** — filename dictates exported symbol name (`create-client.use-case.ts` must `export function CreateClient`).
3. **Spec pairing (TDD gate)** — every unit file under configured folders must have a `.spec.ts` sibling.
4. **Import boundaries** — layer A may not import from layer B; layer C must import from layer D.
5. **Call obligations** — files matching pattern X must contain a call to symbol Y (e.g., non-GET routes must call `Event.save`).

See [`docs/RULES.md`](docs/RULES.md) for semantics of each.

Beyond gating, archwarden is designed to be **queried by coding agents
before they write code**, not just consulted after. See
[`docs/AGENT-INTEGRATION.md`](docs/AGENT-INTEGRATION.md).

## What it does not do

- **Formatting and code style.** Use Biome.
- **Type checking.** Use `tsc --noEmit`.
- **Dead-code and unused-export analysis.** Use Knip.
- **Package version alignment in monorepos.** Use Syncpack or Manypkg.
- **Cyclomatic complexity, metrics, dashboards.** Out of scope.

archwarden intentionally has a narrow surface. Every rule it ships must be
something no other mainstream tool does well.

## Quick start (planned)

```bash
# install (planned distribution channels)
npm install -D @archwarden/cli
# or
cargo binstall archwarden
# or
brew install archwarden

# scaffold a config
archwarden init

# run the gate
archwarden check

# validate the config itself
archwarden config doctor

# ---- agent-facing commands (see docs/AGENT-INTEGRATION.md) ----

# "what rules apply to this path?" — call before writing a file
archwarden describe packages/application/src/use-cases/foo/foo.use-case.ts

# "what does a valid file at this path look like?" — minimal shape
archwarden scaffold packages/application/src/use-cases/foo/foo.use-case.ts

# generate a human-readable rules digest for CLAUDE.md / AGENTS.md
archwarden agent-guide --format markdown > .archwarden/AGENT_RULES.md

# install pre-write hooks for supported harnesses (Claude Code, Cursor, ...)
archwarden install-hooks --claude-code

# ---- diagnostics ----

# why is this file failing?
archwarden explain packages/application/src/use-cases/foo/foo.use-case.ts
```

## Config

One `arch.config.json` at the repo root. JSON with a published JSON Schema so
editors give autocomplete out of the box. No YAML. No JS/TS config files.

The config discovery walks up from the current working directory until it finds
`arch.config.json`, mirroring how `git` finds `.git`. Running archwarden inside
a subpackage of a monorepo therefore analyses the whole monorepo through the
root config.

See [`docs/CONFIG.md`](docs/CONFIG.md).

## Integration

- **Exit code** for CI gates (`0` clean, `1` errors, `2` config problem).
- **JSON output** (`--format json`) for coding agents and other tooling.
- **`explain` command** returns a human-readable reason for any offending file,
  so an agent can answer "why is this invalid?" without re-reading the rules.
- **`describe` / `scaffold`** let an agent ask what applies to a file *before*
  writing it, avoiding the write–fail–retry loop.
- **`agent-guide`** produces a markdown digest of every active rule, meant to
  be referenced from `CLAUDE.md` or `AGENTS.md`. Regenerated deterministically
  from the config.
- **`install-hooks`** wires archwarden into agent harnesses as a pre-write
  hook, so invalid writes are rejected at the source.

## Non-goals

- Being a general-purpose linter.
- Replacing Biome or ESLint entirely — archwarden covers structure and
  architecture, not code style.
- Supporting non-JS/TS languages in the core. The parser layer is pluggable
  (see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)), but shipping other
  languages is not on the v0/v1 roadmap.

## License

Dual-licensed under either of:

- MIT License ([`LICENSE-MIT`](LICENSE-MIT))
- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE))

at your option. This follows the Rust community convention.

Contributions submitted for inclusion in archwarden shall be dual-licensed
as above, without any additional terms or conditions.
