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
  PR CI beside every other tier. With no target configured the test says why
  it did nothing and passes, so the job costs a compile.
- **Divergence handling**: when archwarden and dep-cruiser disagree, the
  reviewer decides whether (a) archwarden is wrong (fix it), (b)
  dep-cruiser is wrong (record as known divergence with rationale in
  `tests/differential/known-divergences.md`), or (c) the config on either
  side is ambiguous (fix the config).
- **Purpose**: gives us confidence that we cover the edge cases years of
  dep-cruiser use has surfaced, without importing their code.

## Nothing here runs on a schedule

There is no nightly job, and the reason is the same one that moved mutation
testing onto the push hook below.

The differential tier *was* a nightly. It failed every night for at least six
days — an unset repository variable substitutes as the empty string, so the
target path was `""` and could not be canonicalised — while the test file
promised that a missing target "prints why it did nothing and passes". Nobody
opened the report. A red job nobody reads is not a test; it is a habit of
ignoring red, and it costs more than the job was ever worth.

So every tier runs where the person who caused the failure is looking: on the
pull request, or on the push that would carry it.

## Mutation testing runs on push, not nightly

`cargo-mutants` injects bugs and checks whether the suite notices. It is the
only thing here that answers **"is this tested?"** rather than "does this run":
line coverage says a guard executed, and says nothing about whether deleting it
would fail anything.

It moved out of the nightly job for a reason that is worth writing down. A
refusal that stops `impact --apply` from carrying out a move which would break
a repository shipped with **no test at all**, and so did four of the others.
The suite was green. Coverage was green. Flipping one boolean would have
removed the protection and nothing would have said so. A survivor list would
have named every one of them — and a nightly report nobody opens is a survivor
list nobody reads.

```
.githooks/pre-push   cargo mutants --in-diff <what you are pushing>
```

Two choices in that line, both deliberate:

- **Push, not commit.** Measured on one commit of 120 changed lines: **3
  mutants, 13 seconds** — about ten of those the unmutated build, paid once,
  and about a second each mutant. The floor is ten seconds however small the
  change, which is well past what the `pre-commit` hook beside it tolerates,
  and it argues correctly that a hook costing more than a couple of seconds is
  one people bypass out of reflex.

  The shape of the cost is the argument, not the size of it. Fixed overhead per
  run means fifteen commits cost fifteen builds and one push costs one.
- **The diff, not the workspace.** `--workspace` mutates the whole project —
  thousands of mutants at about a second each. `--in-diff` mutates only the
  lines being pushed, so the cost tracks the change. New code is where untested
  code comes from.

The scope widened in the move: the nightly job mutated `archwarden-core`,
`archwarden-config` and `archwarden-rules`, and left `archwarden-cli` alone —
which is where `impact --apply` lives, and where the untested refusals were.
Scoping by diff covers every crate without anyone maintaining a list.

**Only survivors block.** `cargo-mutants` exits 2 when mutants lived and
something else when it could not run — a build failure, a timeout, the linker
being killed on a small machine. The first is a statement about your tests; the
second is not, so it warns and lets the push through.

That distinction is the whole difference between a hook people keep and one
they learn to bypass. Someone whose laptop runs out of memory linking a test
binary must not be unable to push; they would reach for `--no-verify` once and
never come back. Mutation testing is advisory exactly when it could not form an
opinion, which is the honest reading of "the tool did not finish".

Not installed is not a failure either: the hook says so and carries on, the
same policy `pre-commit` applies to `typos`. `git push --no-verify` skips it,
which is the point — a hook catches mistakes, it does not take the decision
away from whoever is pushing.

**What it has not been measured at.** A whole branch's accumulated diff — 76
mutants over 1537 lines — has not been run to completion here: this machine has
3 GB of RAM and the linker is killed during the unmutated build, which is the
case the warn-and-continue branch above exists for. At the measured second per
mutant that would be a minute and a half, but that is arithmetic rather than a
measurement, and the only numbers stated as fact above are the ones observed.

**Reading a survivor.** Each line is an edit to your code that no test
objected to. Either write the test, or decide the mutant is harmless and say
why — a `verdict` returning `Some("xyzzy")` that nothing catches means no test
asserts the sentence, which may be fine for prose and is not fine for a
refusal.

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
- **The larger sibling project** — runs on CI when configured, like every
  other tier. Not committed as snapshot (too large, moves too fast);
  instead, the job asserts that the output structure is well-formed and the
  run finishes under the performance budget declared in `ROADMAP.md`.

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
