# archwarden

Fast, declarative architecture linter for TypeScript and JavaScript projects. Written in Rust.

archwarden enforces the rules your project already has but nobody remembers:
which folders may exist under a module, which files must be paired with a spec,
which layers may import which, and which files must call which functions.

It is a single binary. It reads one JSON config from your repo root. It runs
in milliseconds on caches. It is meant to be paired with [Biome](https://biomejs.dev/)
for formatting and code-style — archwarden does not overlap with Biome.

## Install

archwarden is a dev dependency, pinned per repository like Biome — not a
globally installed tool.

```bash
pnpm add -D archwarden     # or: npm i -D archwarden / bun add -d archwarden
```

The package carries no binary of its own. It declares one optional dependency
per platform, and your package manager downloads the single one your machine
needs. There is no postinstall script and nothing to compile.

```json
{
  "scripts": {
    "check:arch": "archwarden check"
  }
}
```

Then `pnpm check:arch`. Outside a script, use `pnpm exec archwarden` /
`npx archwarden`.

Prebuilt binaries for macOS, Linux and Windows are attached to every
[release](https://github.com/HenriqueArtur/archwarden/releases), with `.sha256`
files beside them.

The Linux binaries are statically linked against musl, so they run on any
distribution — Alpine, Debian 11, an Ubuntu 24.04 runner — with no glibc
version to match. That is [decision 14](docs/DECISIONS.md), and the release
workflow proves it by running each one inside `debian:11` and `alpine` before
publishing.

## Status

v0, in development. See [`docs/ROADMAP.md`](docs/ROADMAP.md).

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
before they write code**, not just consulted after.

## For coding agents

**[`AGENTS.md`](AGENTS.md)** is written for the agent, not about it: the
ask-before-you-write loop, every command with its real JSON output, the exit
codes, and what each rule kind wants. It ships inside the package, so a
repository that installs archwarden has it at
`node_modules/archwarden/AGENTS.md`, matched to the version it installed.

Point your agent at it, or paste it into `CLAUDE.md` / your own `AGENTS.md`.
For the design behind the integration, see
[`docs/AGENT-INTEGRATION.md`](docs/AGENT-INTEGRATION.md).

## What it does not do

- **Formatting and code style.** Use Biome.
- **Type checking.** Use `tsc --noEmit`.
- **Dead-code and unused-export analysis.** Use Knip.
- **Package version alignment in monorepos.** Use Syncpack or Manypkg.
- **Cyclomatic complexity, metrics, dashboards.** Out of scope.

archwarden intentionally has a narrow surface. Every rule it ships must be
something no other mainstream tool does well.

## Quick start

```bash
# scaffold a config
npx archwarden init

# run the gate
npx archwarden check

# validate the config itself
npx archwarden config validate      # schema only, fast
npx archwarden config doctor        # semantic: does it mean what you think?

# ---- agent-facing commands (see AGENTS.md) ----

# "what rules apply to this path?" — call before writing a file
npx archwarden describe packages/application/src/use-cases/foo/foo.use-case.ts

# "what does a valid file at this path look like?" — minimal shape
npx archwarden scaffold packages/application/src/use-cases/foo/foo.use-case.ts

# verify one file, without walking the repository
npx archwarden check --file packages/application/src/use-cases/foo/foo.use-case.ts

# generate a rules digest for CLAUDE.md / AGENTS.md
npx archwarden agent-guide > .archwarden/AGENT_RULES.md

# install pre-write hooks for supported harnesses
npx archwarden install-hooks --claude-code

# ---- adopting it in an existing repo ----

# accept today's findings, so the build gates on new ones
npx archwarden baseline

# ---- filtering a large report ----

# what rule is dominating this output?
npx archwarden check --summary

# only the errors; the warnings are known debt
npx archwarden check --level error

# only the part of the repo I touched
npx archwarden check --paths 'packages/domain/**'

# ---- refactoring ----

# what would moving this file change?
npx archwarden impact packages/domain/src/order/x.ts --to packages/app/src/order/x.ts

# ---- diagnostics ----

# what does this rule reach, and what is it flagging?
npx archwarden config explain usecase-export-name
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
- **`config explain <rule-id>`** lists every path a rule covers and every one it
  flags, so "why is this invalid?" is answerable without re-reading the config.
- **`describe` / `scaffold`** let an agent ask what applies to a file *before*
  writing it, avoiding the write–fail–retry loop.
- **`check --file`** verifies one file without walking the repository, and
  reports the rules it could not evaluate rather than dropping them.
- **`--summary` / `--rules` / `--paths` / `--level` / `--changed`** narrow what a
  report prints without narrowing what it checks. The exit code is the same with
  them and without, so a filter is safe in a command that gates a build.
- **`impact <path> --to <path>`** says what a move would change before you make
  it: which rules start and stop applying, which files import it, and which of
  those imports would newly cross a boundary. An editor rewrites the specifiers
  and says nothing about the architecture; this is the other half.
- **`baseline`** is the opposite and says so: a committed record of findings the
  project has decided to accept, so a repository adopting archwarden gates on
  new violations from day one instead of on debt nobody has decided about. It
  changes the exit code, which is why it is a reviewed file and not a flag.
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
