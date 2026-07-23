# Testing strategy

archwarden's correctness bar is high: it is a gate that decides whether
CI passes and whether coding agents' writes are accepted. A false positive
blocks legitimate work; a false negative erodes trust in the tool. This
document defines how we get there.

## Three tiers

Tests are organised in three layers, from fastest and most local to
slowest and most integrative.

### Tier 1 — Unit tests

Per-rule, per-function. Live next to the code (`#[cfg(test)]` modules or
`tests/` files in the same crate).

- **Scope**: one behaviour per test.
- **Fixtures**: inline strings or small in-memory structures. No
  filesystem when avoidable.
- **Speed**: every unit test suite runs in under 5 seconds total.
- **Coverage target**: every branch of every rule kind hits at least one
  unit test.

### Tier 2 — Integration tests

Run archwarden's CLI against synthetic repos assembled in `tempfile`
directories. Live in `crates/archwarden-cli/tests/`.

- **Scope**: end-to-end for a single scenario. "Given this repo layout
  and this config, `archwarden check` returns this exit code and this
  JSON output."
- **Fixtures**: small hand-crafted repos, one per scenario, generated
  in-test. Never checked-in `node_modules` — resolvers use synthetic
  `package.json` stubs.
- **Snapshots**: JSON output is snapshotted with `insta`. Diff review is
  part of PR review.
- **Speed**: full integration suite under 60 seconds.

### Tier 3 — Differential tests

Run archwarden and a reference tool (initially `dependency-cruiser`) over
the same real repository, compare graph output, fail on divergence.

- **Scope**: import-boundary rules only. The other rule categories have
  no equivalent tool to differentiate against.
- **Targets**: Flowmaatik (~2.8k TS files) and the larger sibling project
  (~30k files). Repos are configured via env vars, not checked in.
- **When it runs**: manual (`cargo test --features differential`), and on
  a nightly CI job. Not on the fast PR CI.
- **Divergence handling**: when archwarden and dep-cruiser disagree, the
  reviewer decides whether (a) archwarden is wrong (fix it), (b)
  dep-cruiser is wrong (record as known divergence with rationale in
  `tests/differential/known-divergences.md`), or (c) the config on either
  side is ambiguous (fix the config).
- **Purpose**: gives us confidence that we cover the edge cases years of
  dep-cruiser use has surfaced, without importing their code.

## Reference-inspired test policy

archwarden's rule surface overlaps with prior tools — most notably
dependency-cruiser, ESLint plugin-import, oxc_resolver, and
enhanced-resolve. Their test suites encode decades of accumulated
edge-case knowledge. We want that knowledge; we do not want to copy their
code.

### Clean-room reimplementation

The process for adopting a case from another project's test suite:

1. **Read** the original test. Understand the *behaviour* being exercised
   (e.g., "resolver should resolve `@/foo` to `src/foo.ts` when
   `tsconfig.paths` maps `@/*` → `src/*`").
2. **Describe** the case in one prose sentence in the archwarden test
   file. This forces you to understand it, not just translate it.
3. **Reimplement** the test in Rust with our own fixture data. Do not
   translate the source test line-by-line — start from the description
   and write the test as if the reference did not exist.
4. **Cite** the origin in a comment for auditability:
   ```rust
   // Inspired by: dependency-cruiser test
   //   test/extract/resolve/tsconfig-paths.spec.mjs::"resolves aliased import"
   ```

The result is a test whose *idea* comes from elsewhere but whose *expression*
is ours. This is legally clean and creates a test suite we can evolve
independently.

### What is explicitly disallowed

- **Copying test source verbatim** — even translated to Rust, a line-by-line
  port is a derivative work and carries the original licence.
- **Copying fixture files without licence audit** — every reused fixture
  directory needs a `LICENSE-3RD-PARTY` at its root citing origin and
  original licence. Only MIT, Apache-2.0, BSD-2, BSD-3, and 0BSD are
  accepted.
- **Vendoring another tool's test binary or runner** — we do not run
  their Node/Rust tests inside our CI. Differential tests (Tier 3) run
  the reference tool as a subprocess against a shared target, and diff
  the output.

### Priority list of reference sources

Ordered by value to archwarden:

1. **oxc_resolver test suite** — indirectly covered: since we depend on
   the crate, its tests run when it builds. Upgrade PRs must confirm the
   crate's own tests still pass.
2. **enhanced-resolve fixtures** — MIT licensed. Node resolution ground
   truth for a decade. Fixtures may be imported with proper
   `LICENSE-3RD-PARTY` attribution; the assertions around them are ours.
3. **dependency-cruiser** — MIT. Primary source for TS/JS graph edge
   cases (tsconfig paths, project references, workspace resolution,
   circular deps). Reimplementation only, no code copy.
4. **ESLint plugin-import** — MIT. Second source for the same territory
   as dep-cruiser; useful for cross-checking. Reimplementation only.
5. **eslint-plugin-boundaries** — MIT. Source for layer-boundary rule
   corner cases (`except` clauses, glob interactions).
6. **Steiger** — MIT. Rust-native FSD linter. Worth studying for
   Rust-idiomatic ways to express layer rules.

Adding a new reference source requires:

- Confirming its licence is on the accepted list.
- Adding an entry to `docs/TESTING.md` under this section.
- Recording the source in the citation comment of every test inspired
  by it.

## Real-repo snapshot tests

archwarden is validated against real repositories in CI, not only
synthetic fixtures:

- **Flowmaatik** — the origin project. Runs on every PR. Snapshot of
  `archwarden check --format json` output is committed under
  `tests/snapshots/flowmaatik.json`. Any change to the snapshot requires
  reviewer approval — that is how we notice unintended rule behaviour
  changes.
- **The larger sibling project** — runs nightly. Not committed as
  snapshot (too large, moves too fast); instead, the job asserts that
  the output structure is well-formed and the run finishes under the
  performance budget declared in `ROADMAP.md`.

Both repos are pulled by CI as read-only references. No changes ever
flow from archwarden CI back to them.

## Property-based tests

For the file-walk and config-loading layers, property tests
(`proptest` crate) generate random configs and repo layouts to shake out
crashes and edge cases:

- Config loading must never panic; malformed input yields a structured
  error.
- File walk must terminate on any legal directory structure, including
  symlink cycles.
- Rule matcher must be commutative on rules with disjoint scopes.

These are complements to unit tests, not replacements.

## Performance regression tests

Success criteria in `ROADMAP.md` are numeric. Enforce them:

- A `benches/` directory using `criterion` measures `check`, `describe`,
  and `scaffold` on Flowmaatik and a fixture 30k-file synthetic repo.
- CI compares against the previous release's baseline. Regressions
  greater than 20% fail the build.
- Benchmarks are informational, not deterministic gates — variance on
  shared CI can trigger false alarms. A regression triggers investigation,
  not an automatic revert.

## Test data hygiene

- No secrets, credentials, or personal data in fixtures — ever.
- No copyrighted code (song lyrics, copyrighted samples) in fixtures.
- Fixture files that mimic real code should be trivially fake
  (`function foo() { return 1; }`) to make it obvious they are not
  derived from anywhere.
- `tests/fixtures/*/LICENSE-3RD-PARTY` when a fixture set was imported
  from another project's test suite (see policy above).

## What we do not test

- **Third-party crate behaviour** — we test our integration with
  `oxc_parser` and `oxc_resolver`, not their internals. Their tests
  cover that.
- **The Rust standard library** — filesystem, threading, hashing all
  assumed correct.
- **Editor integrations** — LSP behaviour (v1) is tested via LSP protocol
  fixtures, not by driving VS Code or Zed. Actual editor QA is manual.
