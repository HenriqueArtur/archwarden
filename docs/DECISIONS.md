# Decisions

Short ADR-style notes on the load-bearing choices behind archwarden.
Every entry has: context, decision, and the alternatives that were
weighed against it. New entries go at the top.

Format for each entry:

```
### N — Title
Status: accepted | superseded | proposed
Context: what forced this decision.
Decision: what we chose.
Alternatives: what we considered and why they lost.
Consequences: what this locks us into or unlocks.
```

---

### 13 — `--fix` stays out, and `spec-pair` is why
Status: accepted.
Context: a real repository put 37 warnings of "action without a spec"
on one screen, nearly all identical, and asked for a `--fix` that
would write the missing `.spec.ts` files. Decision 2 already deferred
`--fix` to a later version; this records why the most obviously
fixable rule is the one that must never have it.
Decision: no rule gets `--fix`. Not in v0, and `spec-pair` not ever
in the form proposed.

Rule by rule, the mechanical fix and what it costs:

- `structure` — move the file. Breaks every import of it.
- `naming` — rename the export, or the file. Either breaks callers.
- `import-boundary` — no mechanical fix exists; the answer is a
  design change.
- `call-obligation` — insert a call. That is editing behaviour, and
  a linter that writes statements into a function body has stopped
  being a linter.
- `spec-pair` — create an empty file. Mechanically trivial, and the
  reason for this entry.

`spec-pair` is the trap because the fix is easy and wrong. With
`require_non_empty_spec: false`, writing an empty spec turns a real
warning into a pass while changing nothing true about the repository:
the tests that were missing are still missing, and the linter now says
they are not. That is a tool manufacturing green. With
`require_non_empty_spec: true` the stub does not even help — it still
fails, so `--fix` would have done nothing but create files.

So the flag either lies or is useless, depending on a setting the user
did not think they were choosing between.
Alternatives:
- Emit a stub containing a failing test, so the suite goes red and the
  author is forced to look. Rejected twice over: archwarden would be
  writing test code in a framework's syntax it has no business
  knowing, and a linter that deliberately breaks your test suite is a
  linter people uninstall. It also inverts the contract — `check`
  reports, and the build failing afterwards would come from a file
  archwarden wrote.
- `--fix` restricted to `spec-pair` rules without
  `require_non_empty_spec`. Rejected: that is precisely the
  configuration where the stub is a lie, so the restriction selects
  for the dangerous case.
- `scaffold --write`, creating the shape for one named path on
  purpose. Not rejected, but not this: it is a create-time
  affordance, framed as "write the file I am about to write" rather
  than "fix the violation", and it earns its keep only if someone
  asks for it on its own terms.
Consequences: the pain that prompted the request is real and stays
unaddressed by this entry. It is not a fix problem — it is a debt
problem: a repository adopting archwarden inherits violations it has
not decided to fix yet, and every run reports them again.

The honest answer to that is a **baseline**: a committed record of
accepted findings, against which `check` reports only what is new.
It says out loud what a stub would have said silently, it is reviewable
in a pull request, and it does not require archwarden to write a single
byte of anyone's source. That is the feature to build when this pain
comes back, and `--changed` deliberately did not become a disguised
version of it.

Until then: `--summary` collapses the wall to one line per rule,
`--rules` isolates one, and `--changed` shows what a change touched.

---

### 12 — Dependency licences are a separate list from fixture licences
Status: accepted.
Context: decision 11 fixed an allowlist of MIT, Apache-2.0, BSD-2, BSD-3 and
0BSD. That list governs **fixture data imported into our test suite** — a
directory of fake `package.json` and `.ts` files representing a tricky
resolution scenario, copied with a `LICENSE-3RD-PARTY` marker. It has never
governed test *code*, which is always clean-room reimplemented and never
copied.
`cargo-deny` enforces an allowlist over something unrelated: the ~230 crates
downloaded from crates.io and compiled into the archwarden binary. Both are
"a list of acceptable licences", which makes them easy to confuse, and the
confusion is expensive because the two lists cannot be the same. Restricting
dependencies to decision 11's five licences makes archwarden unbuildable.
Decision: `deny.toml` carries its own allowlist for the dependency graph:
the five from decision 11 plus three that no Rust dependency tree can avoid.

- `Unicode-3.0` — `unicode-ident`, which sits under `proc-macro2` and `syn`
  and therefore under every `#[derive(...)]` in the language. Without it there
  is no `#[derive(Deserialize)]`, and no way to read `arch.config.json`.
- `ISC` — functionally identical to MIT, shorter text. Used by several small
  utility crates.
- `Apache-2.0 WITH LLVM-exception` — the terms of the Rust standard library
  itself. The exception removes an attribution requirement when distributing
  compiled binaries, so it is *more* permissive for us than plain Apache-2.0.

Copyleft is excluded from **both** lists, MPL-2.0 included.
Alternatives:
- One shared list for fixtures and dependencies. Rejected: archwarden does not
  compile under decision 11's five, so a shared list means either an
  unbuildable project or a fixture policy loosened for reasons that have
  nothing to do with fixtures.
- No dependency allowlist at all. Rejected: a copyleft crate could then be
  linked into the distributed binary without anyone noticing, which is a
  licensing problem users would inherit from us.
- Allow MPL-2.0 among dependencies. Rejected for the same reason decision 10
  rejected MPL for archwarden's own licence: file-level copyleft inside a
  statically linked binary is a question no user should have to answer.
Consequences: two allowlists exist and must not be conflated. The fixture list
stays strict and lives in decision 11 and `TESTING.md`. The dependency list
lives in `deny.toml` and is enforced on every CI run. Adding a licence to
either one is a deliberate change, not a fix for a red build.

### 11 — Testing strategy: clean-room reimplementation of prior tests
Status: accepted.
Context: prior tools in this space (dependency-cruiser, ESLint
plugin-import, enhanced-resolve, oxc_resolver) encode decades of
edge-case knowledge in their test suites. Discarding that would leave
archwarden blind to edge cases the ecosystem has already learned. Copying
tests verbatim carries the original licence and creates a maintenance
liability tied to another project's evolution.
Decision: three-tier testing (unit, integration, differential). Cases
inspired by other projects are reimplemented clean-room: read the
original test, describe the behaviour in prose, write our own test
against our own fixtures, and cite the origin in a comment. Verbatim
copies (even translated to Rust) are forbidden. Third-party fixtures may
be imported only under MIT/Apache-2.0/BSD/0BSD with a `LICENSE-3RD-PARTY`
attribution file. Differential tests run archwarden against
dependency-cruiser on real repos to catch divergences without importing
their code. See [`TESTING.md`](TESTING.md).
Alternatives:
- Vendor other projects' test suites and run them under our runner.
  Rejected: pulls in their dependencies, couples our CI speed to theirs,
  and breaks the "one binary" story.
- Write all tests from scratch with no reference. Rejected: reinvents
  edge cases the ecosystem already documented. Would take years to reach
  parity on tsconfig-paths and workspace resolution alone.
- Line-by-line ports of other suites into Rust. Rejected: derivative
  works carry the origin licence and create a permanent audit burden.
Consequences: every test inspired by a reference source has a citation
comment. Fixture directories imported from other projects have a
`LICENSE-3RD-PARTY` marker. Differential tests need dep-cruiser
installed on nightly CI. We accept slightly slower initial test
coverage in exchange for tests we fully own and can evolve.

### 10 — Dual licence: MIT OR Apache-2.0
Status: accepted.
Context: archwarden ships as both a CLI (used by JS/TS shops) and a
Rust crate (published to crates.io). Each ecosystem has different
conventions: JS/TS defaults to MIT; Rust defaults to `MIT OR Apache-2.0`
dual. archwarden must fit both without friction.
Decision: dual-licensed under MIT and Apache-2.0. Users pick either at
their option. Contributions are dual-licensed by default.
Alternatives:
- MIT only. Rejected: no patent grant. Corporate adopters who care
  about patent risk (Apache preferred) would have to negotiate.
- Apache-2.0 only. Rejected: some JS-side users see Apache as heavier
  and default to MIT-licensed alternatives without checking.
- MPL 2.0. Rejected: file-level copyleft is fine for dep-cruiser
  (a monolithic Node tool) but interacts awkwardly with a Cargo
  workspace where our crates might be embedded.
- AGPL. Rejected: makes no sense for a CLI linter — nobody hosts a
  linter as a service.
Consequences: `Cargo.toml` will declare `license = "MIT OR Apache-2.0"`.
Both `LICENSE-MIT` and `LICENSE-APACHE` ship in the repo root. Contributors
are informed via README that submissions are dual-licensed.

### 9 — archwarden is an informant, not only a gate
Status: accepted.
Context: coding agents that only meet archwarden after writing a file
waste an iteration every time a rule fires. The rules exist; the agent
just did not know them in advance. A pure-gate tool captures failures
but does not prevent them.
Decision: archwarden ships four integration layers from v0
([`AGENT-INTEGRATION.md`](AGENT-INTEGRATION.md)):
`describe` and `scaffold` for pre-write queries, `agent-guide` for a
`CLAUDE.md`-referenced rule digest, and `install-hooks` for harness-side
pre-write enforcement. `check` remains the gate; the other commands
prevent the write from being wrong in the first place.
Alternatives:
- Ship only `check` and `explain` in v0, defer agent commands. Rejected:
  agents are the primary source of the violations these rules exist to
  catch. Shipping the gate without the informant means the tool is
  measured on its worst loop (write, fail, retry) rather than its best
  loop (ask, write correctly).
- Rewrite the agent's output when it fails a rule. Rejected: crosses
  from linting into refactoring. Different tool, different scope.
Consequences: every rule kind must implement a `describe_expectation()`
method so `scaffold` and `agent-guide` stay in lockstep with the
checker. archwarden owns `.archwarden/` in the repo for generated
artefacts (`AGENT_RULES.md`, cache). It never edits the user's
`CLAUDE.md` or `AGENTS.md` — the user references the generated file
themselves.

### 8 — Rust from the start
Status: accepted.
Context: target repos include one with ~30k files growing at ~1k files
per month. Cold-run performance and warm-cache watch latency need to
stay flat as the file count grows.
Decision: implement archwarden in Rust from v0. Distribute as a native
binary.
Alternatives:
- TypeScript first, port hot paths later. Rejected: two implementations,
  double the maintenance, and the Node startup cost alone would exceed
  the whole target budget on warm caches.
- Go. Rejected: adequate performance, but the JS/TS parser and resolver
  ecosystem in Rust (`oxc_*`) is significantly more mature than in Go.
Consequences: contributors need Rust knowledge. End users need none —
the binary is downloaded, not built. CI must run a matrix of build
targets. This is a bigger up-front cost that pays back on every run.

### 7 — Own the import graph; do not depend on dependency-cruiser
Status: accepted.
Context: import boundaries are one of the five core rule categories.
The user wants archwarden to be the sole tool (paired only with Biome),
which rules out delegating the graph to dependency-cruiser.
Decision: implement the import graph inside archwarden, using
`oxc_parser` to parse and `oxc_resolver` to resolve.
Alternatives:
- Delegate to dependency-cruiser and just consume its output.
  Rejected: adds a Node/npm dependency to the pipeline and defeats the
  "one binary" story.
- Write our own resolver. Rejected: TypeScript resolution semantics
  (paths, exports, conditional exports, workspace resolution) took the
  Node ecosystem years to get right. `oxc_resolver` already handles it.
Consequences: archwarden is coupled to `oxc_resolver` correctness for
edge cases in monorepos with exotic configs. Mitigated by the resolver
trait (see decision 6).

### 6 — Resolver and parser behind traits
Status: accepted.
Context: `oxc_*` crates are young. Betting the project on one specific
implementation with no seam to replace it is risky. Additionally,
extending archwarden to another language later requires a swappable
parser.
Decision: rule engines depend on extracted `FileFacts` only. A
`Resolver` trait and a `Parser` trait sit between the parsing stage
and the rule engines. Default impls use `oxc_parser` and
`oxc_resolver`.
Alternatives:
- Call `oxc_*` directly from rule code. Rejected: couples every rule
  to the parser version and blocks language expansion.
- Full abstract-syntax-agnostic core (visit any AST via a generic
  visitor). Rejected: over-engineered for v0 when only JS/TS is
  targeted.
Consequences: adding a language means implementing the two traits;
rule code needs no changes. Swapping the resolver later requires no
rule changes either.

### 5 — Config format is JSON, not YAML or TOML or JS
Status: accepted.
Context: config is data that both humans and coding agents edit.
Decision: JSON with a published JSON Schema referenced via `$schema`.
Alternatives:
- YAML. Rejected: significant whitespace + type coercion (`no` → false,
  version strings interpreted as floats) is a bug source in a config
  meant to reduce ambiguity.
- TOML. Rejected: fine format, but nested arrays of objects (which
  archwarden configs are dominated by) are awkward in TOML.
- JS/TS config. Rejected: executable configs are a supply-chain and
  reproducibility problem; the whole point of archwarden is that
  configs are declarative artefacts, not code.
Consequences: schema autocomplete works in every mainstream editor
without a plugin. Comments are impossible in strict JSON — accepted;
users can use description fields inside rules if they need to
document intent.

### 4 — Config discovery walks up from CWD
Status: accepted.
Context: users will run archwarden from arbitrary subdirectories in
large monorepos. Requiring a `--config` flag every time is friction.
Decision: search for `arch.config.json` upward from the CWD until
found or the filesystem root is reached. The first match wins.
`--config` overrides.
Alternatives:
- Require `--config` always. Rejected: friction.
- Recursively find *all* configs under the CWD and analyse each
  scope. Rejected: makes reproducibility unclear (which config
  wins on overlaps?) and does not match how tools like git and
  biome behave.
Consequences: one config per repo is the intended model. Sub-config
files may be supported later via `extends`, but the root file is
always the entry point.

### 3 — Cache is a v0 requirement, not a v1 feature
Status: accepted.
Context: the target repo grows ~1k files per month. Non-incremental
tools cross a usability line somewhere between 10k and 100k files.
Decision: content-addressed on-disk cache from v0. Key includes both
file content hash and the hash of the rules that apply to the file.
Alternatives:
- Skip caching until users complain. Rejected: retrofitting cache
  invalidation into a code base that never assumed it is painful and
  bug-prone. Building cache correctness in from the start is cheaper
  than adding it later.
Consequences: `.archwarden/cache/` is a build artefact and must be
gitignored. Cache format is versioned; incompatible changes bump the
version and invalidate all entries.

### 2 — No auto-fix in v0
Status: accepted.
Context: architectural violations are usually not mechanically fixable.
Moving a file to a new folder can break imports; renaming an export
requires updating callers; splitting a use-case that has grown too big
is a design decision.
Decision: archwarden reports only. Biome handles what is safely
fixable in the code-style space; archwarden stays in the report-only
space.
Alternatives:
- Ship trivial fixes (naming coupling could suggest a rename).
  Deferred to v2 with `--fix`.
Consequences: `explain` becomes the primary interactive affordance:
"why is this wrong, and what should it look like instead?".

### 1 — Two levels only: error and warning
Status: accepted.
Context: linters that ship three or four severity levels tend to see
"info" and "hint" ignored entirely, and users lose confidence that
warnings matter.
Decision: `error` and `warning` are the only levels. Errors fail CI;
warnings do not.
Alternatives:
- Add `info`. Rejected: encourages dumping-ground rules.
- Errors only. Rejected: legitimate need to track technical debt
  without blocking CI (e.g., new spec-pair rule with existing
  offenders that will be resolved incrementally).
Consequences: rule authors must decide up front whether a rule is a
gate or a signpost. That decision is often a healthy conversation.
