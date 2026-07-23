# Roadmap

This is a design-phase document. Nothing is shipped. Milestones are ordered
by dependency, not by calendar dates.

## Guiding principle

Every version must be usable on its own. archwarden should never be in a
state where "the useful part comes in the next release". A user who
adopts v0 gets real value; v1 makes it faster and broader; v2 opens it up.

## v0 — Structural core

Goal: replace the hand-rolled `check-structure.ts` / `lint-naming.ts`
scripts in Flowmaatik and equivalents in the larger sibling project.

Scope:

- CLI (gate): `check`, `check --file`, `init`, `explain`, `config validate`,
  `config doctor`, `config explain`.
- CLI (agent-facing): `describe`, `scaffold`, `agent-guide`, `install-hooks`.
  See [`AGENT-INTEGRATION.md`](AGENT-INTEGRATION.md).
- Rule categories 1, 2, 3 fully implemented (structure, naming, spec-pair).
- Rule categories 4, 5 implemented with the constraints in
  [`RULES.md`](RULES.md).
- Every rule kind implements `describe_expectation()` so `scaffold` and
  `agent-guide` stay in lockstep with the checker.
- Config loading with discovery, `extends` for presets, JSON schema
  published.
- Text, JSON, and markdown output formats.
- On-disk cache, content-hash keyed. No watch mode yet.
- `install-hooks --claude-code` ships in v0. Other harnesses follow their
  own hook-API availability.
- Distribution: prebuilt binaries in GitHub Releases; npm shim package;
  cargo-binstall support.

Non-goals for v0:

- Watch mode. `check` on a warm cache is expected to be fast enough for
  the local `pre-commit` loop.
- LSP.
- SARIF output.
- Auto-fix of anything.
- User-defined rules.

Success criteria:

- Full-repo check on Flowmaatik (~2.8k TS files) completes in under
  500 ms cold, under 100 ms warm.
- Full-repo check on the larger sibling project (~30k files, growing at
  ~1k/month) completes in under 5 s cold, under 500 ms warm.
- Zero regressions in the rules currently enforced by the Flowmaatik
  scripts.
- `describe` and `scaffold` return in under 50 ms on a warm cache.
- With `install-hooks --claude-code` active, an agent attempting to
  create an invalid file is blocked by the pre-write hook and receives
  a message identifying the rule and the fix.

## v1 — Local feedback loop

Goal: get archwarden out of CI-only usage and into the editor loop, so
violations are caught while the code is being typed.

Scope:

- `archwarden watch` — filesystem watcher with sub-100 ms recompute on
  changed files.
- LSP server (`archwarden-lsp`) exposing rule findings as diagnostics.
  Integrations with VS Code and Zed to start.
- SARIF output for GitHub code scanning.
- Better `explain` output: include the rule definition, the file's
  observed state, and the specific expectation that failed.
- Preset ecosystem: publish `@archwarden/preset-clean-arch`,
  `@archwarden/preset-hexagonal`, `@archwarden/preset-nextjs-app-router`
  as starting points.

Success criteria:

- Editor diagnostic updates within 100 ms of file save on a 30k-file repo.
- At least one third-party preset published (i.e., proof someone outside
  the maintainer group finds the preset format usable).

## v2 — Extensibility

Goal: let users express rules archwarden does not ship.

Scope:

- Plugin API for custom rules. Two candidate implementations to evaluate:
  - WASM plugins with a stable ABI (safe, portable, slow to iterate on).
  - Native dylib plugins (fast, unsafe, harder to distribute).
- Cross-file call-graph analysis for `call-obligation` rules.
- Additional parser front-ends (Python, Go) if driven by user demand.
- `--fix` for a small set of mechanically safe fixes (renaming imports
  to match a `naming` rule; moving a file into an allowed subfolder).

Non-goals for v2:

- General static analysis. archwarden is not a compiler and not a taint
  tracker. Rules that need program semantics belong in other tools.

Success criteria:

- A user can write a custom rule, package it, and distribute it via npm.
- The plugin API remains ABI-stable across at least two archwarden
  patch releases.

## Not on the roadmap

Things people will ask for but I do not intend to build:

- **Code formatting**. Biome and Prettier do this well.
- **Type checking**. That's `tsc`.
- **Unused-export detection**. Knip does this.
- **Complexity metrics**. Out of scope.
- **A web dashboard**. archwarden's output is JSON; build your own if
  you need one.
- **Rules in JS/TS config files**. Config is data. Executable configs
  are a bug source and a security concern. If a rule cannot be
  expressed declaratively, it belongs in the plugin API (v2).

## Open design questions

Tracked here so I do not forget them:

1. **Cross-file spec pairing**. Should a co-located `__tests__/` directory
   satisfy `spec-pair`, or only sibling files? Leaning: sibling-only,
   because co-located test dirs invite scattered test files.
2. **Rule composition**. Should users be able to say "rule A applies
   unless rule B applies to the same file"? Real use case unclear.
   Defer to v2.
3. **Config-in-package.json**. Biome and Prettier accept keys inside
   `package.json`. Leaning against: archwarden config is longer than
   theirs and bloats `package.json`.
4. **Migration tooling**. Should `init` inspect an existing repo and
   propose a starter config? Nice for adoption. Post-v0.
