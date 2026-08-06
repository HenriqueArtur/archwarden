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

### 18 — `tsconfig.paths` is read per importer, and the maps are never merged
Status: accepted.
Context: issue #22 reported that the aliased half of a boundary rule
silently enforces nothing, and proposed reading `compilerOptions.paths`
into one repository-wide map. The premise was taken from this project's
own documentation, which said in five places that archwarden does not
read the alias map.

That documentation was wrong, and had been for as long as
`TsconfigDiscovery::Auto` has been set on the resolver. `paths` is read,
per importing file, and a boundary rule fires on an aliased import that
crosses it — `a_tsconfig_path_alias_resolves_to_the_same_file_as_the_relative_form`
has asserted so since v0. The cost of the wrong sentence was real: the
reporter duplicated their boundary by hand into
`forbid_import_from_packages`, their hand-written list was missing two
entries, and imports crossed the boundary with the build green. A false
claim of a blind spot produced an actual one.

What they hit is narrower than the docs led them to believe. Aliases are
resolved by TypeScript's own rule — the nearest `tsconfig.json` to the
file wins, whole — so an alias declared in an app's `tsconfig` does not
apply to a file in a package, and a bare `tsconfig.json` in a directory
takes the repository's aliases away from everything under it unless it
`extends` the one that declares them. Their files were mid-extraction:
physically in `packages/domain`, still compiled by the app's program.
Decision: keep per-importer discovery, and decline to merge every
`paths` map in a repository into one.
Alternatives:
- Merge all `paths` maps repository-wide, as the issue proposed.
  Rejected: `@/*` is the most common alias there is and means a
  different directory in every package — `each_package_gets_its_own_tsconfig`
  is that test. A merged map resolves one package's import into
  another package's source, and a boundary rule fed a *wrong* edge is
  worse than one fed no edge: `check` names the import it could not
  place (issue #18) and says nothing about the one it placed wrongly.
- Let `arch.config.json` declare the aliases. Rejected for the reason
  the issue itself gives: a second source of truth for the same fact,
  where an alias changed in the `tsconfig` and not mirrored here is a
  silent false green.
Consequences: an import that resolves under the build but not under the
file's own `tsconfig` is reported as unresolved and named, which is the
honest answer — under that file's compilation context it does not
resolve either. The fix is in the `tsconfig`: declare the path where the
file lives, or `extends` the config that does. Two tests pin the
behaviour so the documentation cannot drift away from it again.

---

### 17 — A boundary may name a package, in a field of its own
Status: accepted.
Context: `RULES.md` declared a prohibition on a dependency out of scope for
v0, and gave a real reason: globs are matched against repo-relative
paths, and an installed package does not have one. Issue #14 is the
motivating case — `three` is 150 KB gzipped against a page budget that is
otherwise a few KB, and the project's actual rule is "only
`src/scripts/three/**` may import it".

The argument that changed the answer is not the lint. Biome's
`noRestrictedImports` covers it and covers it well. It is that the rule
then lives in a second config file, and `describe` and `agent-guide` read
only the first — so an agent that consults archwarden before writing, which
is the whole premise of `AGENTS.md`, gets an incomplete answer and writes
the violation. A config that structurally cannot hold a project's rule
makes the agent-facing commands wrong, not merely incomplete.
Decision: `import-boundary` gains `forbid_import_from_packages`, matching
**package identity** rather than any path, plus `except_from` for the
importing side.

Four things fall out of "identity, not path", and each is load-bearing:

- The package **and everything under it**. `three/examples/jsm/loaders/
  GLTFLoader.js` is the import that costs the bytes; a rule that caught
  only the bare name would miss the case it exists for. `three-mesh-bvh`
  is a different package.
- `node:fs` and `fs` are one module and one identity.
- An import that resolves **into this repository** is a path, and
  `forbid_import_from` is its field. The two never both fire on one
  import, so a `tsconfig` alias spelling a local shim `three` is not
  caught by a rule about the dependency.
- Reading the specifier rather than the resolution means the rule holds on
  a repository whose dependencies are not installed. That is the opposite
  of the path half, which is blind there, and it is worth having: a CI job
  that lints before installing still enforces it.
Alternatives:
- **A glob against `node_modules/three/**`.** Rejected as a lie: under
  pnpm's store layout that path is a symlink into a content-addressed
  store, and under yarn PnP there is no such path at all. A rule that
  depends on a package manager's on-disk layout enforces nothing on half
  the ecosystem, silently.
- **A scheme prefix inside `forbid_import_from` (`"pkg:three"`).** One
  field and one mental model, which is genuinely attractive. Rejected on
  the same ground the issue raises: the day someone writes `"three"`
  without the prefix, it is silently a path glob that matches nothing —
  and a rule enforcing nothing is indistinguishable from a rule that
  passes. A separate field cannot be got wrong that way.
- **Reusing `except` for the importer side.** Rejected: `except` already
  means "and these imported paths are fine". Overloading it to sometimes
  mean the importer would make an existing rule's meaning depend on which
  other fields are present.
Consequences: transitivity is still declined, exactly as for the path
half — `src/lib` importing `src/scripts/three`, which imports `three`, is
not flagged. `RULES.md` declined reachability and this declines it the
same way, so the two halves of the rule stay one idea.

The rule reads the specifier rather than the resolution, so a dependency
reached through a `tsconfig` alias is spelled by the alias and not by the
package name this field matches. `check` names every import it could not
place (issue #18), which is where such a case shows up.

---

### 16 — `impact --apply` moves what you named; it never picks what to move
Status: accepted.
Context: decision 2 puts archwarden in the report-only space and decision
13 explains why the most obviously fixable rule must never have a fix.
`impact --apply` writes to the user's source tree, so it has to be
squared with both or it is those decisions being quietly reversed.

The pressure that produced it is real. Eliminating a `shared/` folder
across seven entities in one repository is 15 files, 29 import
specifiers, and 24 files edited. archwarden already knew every one of
those — it resolves the graph to answer `impact` — and reported them
correctly while helping with none of it. An editor does the relative
half of the rewrite and leaves the package-name half, which in that
repository is the majority.
Decision: `--apply` ships, under one rule: **archwarden carries out the
move the caller described, and never decides what to move.**

Concretely, and each of these is load-bearing:

- No finding suggests it. `check` never mentions it, and there is no
  `--fix`, no "apply all", no mode that moves more than the argument
  named.
- Dry run stays the default. `--apply` is a second, explicit word.
- The exported symbol is not renamed, even when the filename changes.
  Renaming an export breaks callers in a way this cannot see — decision
  13's argument about `naming` — so the output says the symbol was left
  alone and `check` reports the mismatch afterwards, which is where a
  `naming` rule belongs.

That is the same seam decision 13 left open for `scaffold --write`:
"write the file I am about to write" rather than "fix the violation".
The distinction is not the blast radius, it is who chose.
Alternatives:
- Leave it out and let editors do it. Rejected on measurement: an editor
  cannot rewrite `@org/domain/email/x`, because to it that is a package
  name like `react`. In the repository this was built against, 5481
  imports are written that way against 5690 relative ones, so "the
  editor handles it" is half a refactor.
- `--fix` on `structure` findings, moving a file into an allowed
  subfolder. Rejected, and this is the line: the rule knows the folder
  is wrong and not which of eight allowed folders is right. Picking one
  is a design decision, and a linter that makes it is guessing with
  write access.
- Warn on a dirty working tree instead of refusing. Rejected: `git` is
  the entire undo story here, and an undo that would take the user's
  uncommitted work with it is not one.
Consequences: everything is computed and validated before a byte is
written, so every refusal is total — there is no state where half the
imports are rewritten. A dynamic import naming no module blocks the
apply, because whether such a file imports the target is unknowable;
`--force` is the only refusal a flag may override, and the report prints
the file to look at first. Everything else — a specifier resolving
through a `tsconfig` alias, which is read forwards and cannot be written
backwards, a destination already occupied, two files landing on one path
— refuses outright, because forcing past one produces a repository that
does not build.

That promise was unconditional and, until issue #28, untrue in one case.
A file being moved that git does not track is refused by `git mv` — and
refused *during* the move, after the specifier rewrites are on disk, so
the repository was left with importers naming a module that had never
been created. The recovery the message offered, `git checkout .`, is
precisely what cannot restore an untracked file: the trigger and the
reason the advice fails are the same fact. Untrackedness now joins the
preconditions, asked in one `git ls-files` before anything is written.
The general lesson is the one the promise already implied: a question
answered by the tool performing the write is not a precondition, however
early in the write it happens to be asked.

The emptied source directory is removed. Not cosmetic: `structure` rules
are about directories, so an emptied `shared/` keeps reporting the exact
finding the refactor was run to remove. Measured before the fix: nine
warnings, unchanged, after every file in them had moved.

---

### 15 — Workspace packages resolve from their manifests, not from `node_modules`
Status: accepted.
Context: a monorepo imports itself by package name —
`@flowmaatik/domain/email/x`, not `../../domain/src/email/x`. Node answers
that specifier by looking in `node_modules`, where the package manager has
left a symlink. `oxc_resolver` does the same, correctly, and decision 7 says
not to hand-roll a second worse copy of Node resolution.

On a checkout that has not run an install, that whole half of the graph
resolves to nothing. Measured on a real pnpm monorepo with no
`node_modules`: **5481 imports by package name against 5690 relative ones**.
`impact` on one file found 2 importers where the true answer was 3; the
missing one was in another package and imported by alias.

What makes it a bug rather than a limitation is that nothing said so.
Import-boundary rules over those edges report nothing, which reads exactly
like a boundary that is satisfied. The tool was answering about the
relative half of a repository and presenting it as the whole.
Decision: build the alias map from what the repository itself declares —
every `package.json` with a `name`, and the `exports` field that says which
subpaths it offers and where they land. Feed it to `oxc_resolver` as
**`fallback`**, not `alias`.

`fallback` is consulted only after normal resolution has failed. A
repository that *has* installed its dependencies resolves exactly as it did
before, and an installed package always wins over our reconstruction of it.
The map fills a hole; it never overrules anything.
Alternatives:
- Read `pnpm-workspace.yaml` (and `workspaces`, and bun's equivalent) to
  learn which directories are members. Rejected: it puts a YAML parser in a
  binary that has no other use for one, and the answer is the same in every
  layout anyone writes. Taking every `package.json` in the walk at its word
  costs a package that exists on disk but is excluded from the workspace —
  and that one is only reachable by a specifier no file can be importing,
  since under Node it does not resolve and the repository would not build.
- Reimplement the `exports` subpath algorithm and resolve to a path
  directly. Rejected: that is decision 7 again. The `exports` patterns map
  onto `oxc_resolver`'s own wildcard alias form (`@org/domain/email/*` →
  `<dir>/src/email/*.ts`), so the matching stays in the resolver.
- Require an install. Rejected: it makes archwarden useless in the case it
  is most useful — CI that lints before it builds, and any checkout where
  the lockfile is the only thing that changed.
**Amendment, 0.5.1.** `fallback` is right and incomplete. It covers the
case where normal resolution *fails*; it says nothing about the case
where it succeeds and lands on a **copy** of a workspace package under
`node_modules`. pnpm with `node-linker=hoisted`, npm on a filesystem
without symlinks, a container volume, a partial install — all leave a
copy rather than a link, and a copy has `node_modules` in its path, so
`classify` called it somebody else's code and the file importing it
vanished from the graph.

The consequence was not a missing warning. `impact` reported two
importers where there were three, and `--apply` rewrote two, left the
third pointing at a file that had moved, and exited 0. Measured on the
same repository at the same commit with the same published binary: 29
specifiers rewritten with a symlink, 26 and three broken imports with a
copy.

So a resolved path under `node_modules` whose package name is one the
repository declares is mapped back to the source it was copied from —
when that source exists, and never otherwise. A dependency that merely
shares a name is still a dependency.

Consequences: resolution now depends on every local `package.json`, which
the `resolution_epoch` already hashes (decision 3's cache design listed
`package.json` for `exports` and workspaces), so no cache change was needed.

A repository that adds an `import-boundary` rule over an aliased edge will
now see findings it did not see before. That is the bug being fixed, not a
regression — but it is a behaviour change, and a project upgrading into it
should expect its first run to report more than the last one did.

---

### 14 — Linux binaries are static musl, and the floor is a decision
Status: accepted.
Context: archwarden 0.3.0 would not start on Debian 12, Ubuntu 22.04,
or any `node:` image. The binary required `GLIBC_2.39`, which only
Ubuntu 24.04 and newer have.

The cause was a feature: `--changed` was the first thing to put
`std::process::Command` into the shipped binary, and Rust's standard
library links its pidfd path along with it — `pidfd_spawnp` and
`pidfd_getpid`, both glibc 2.39. Nothing about the toolchain or the
runner changed; the code reached a corner of std that had a newer
floor.

Measuring the published binaries showed the deeper problem. 0.1.1 and
0.2.0 required 2.34, and 0.3.0 required 2.39 — nobody had ever chosen
either. The floor was whatever the build runner happened to have, and
it moved when a feature touched a new symbol. Running the published
0.2.0 in `debian:11` fails too, so "restore the old floor" was never
a fix, only a smaller version of the same bug.
Decision: the Linux packages carry **statically linked musl**
binaries, under the plain `linux-x64` and `linux-arm64` names, with no
`libc` field and no separate `-musl` packages. A static binary has no
floor to move.

The cost was measured rather than estimated, running the published
glibc and musl 0.3.0 binaries on the same machine over 8000 files that
are parsed and resolved: 55ms against 60ms cold, 51ms against 59ms
warm, and the musl binary is slightly *smaller*. About 10%, or 8ms —
against a failure mode that removes the tool from every common Linux
container.
Alternatives:
- Build on an old base image, or with `cargo-zigbuild` targeting a
  named glibc. Both work and both keep the performance. Rejected
  because they replace one floor with another: the next time std
  reaches a newer symbol the problem returns, and CI will not notice,
  because CI runs on the new runner. It is the same trap, deferred.
- Keep both, glibc as default and musl as fallback. Rejected: that is
  what 0.3.0 shipped, and the glibc package is the one every
  glibc-based machine installs — so the failure would remain the
  default experience.
- Avoid `std::process::Command`. Not viable: reading git's index by
  hand to answer `--changed` is not a trade anyone should make.
Consequences: Alpine and Debian 11 now run the same binary as Ubuntu
24.04, and there is no libc detection left in the wrapper. The release
archives for Linux are named `...-unknown-linux-musl`, which is what
someone downloading directly will see.

The floor being a decision is enforced, not documented: the release
workflow runs each Linux binary inside `debian:11` (glibc 2.31, older
than every current distribution) and `alpine` (no glibc at all) before
anything is published. Those are the two ways a Linux binary fails to
start.

That check is the real lesson. Every check archwarden had ran on a
machine shaped like the runner, so a binary that only worked on the
runner passed all of them — including a full local battery, on a
machine that happened to be Ubuntu 24.04 on aarch64. A test that
cannot fail on the developer's machine has to run somewhere that can.

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
