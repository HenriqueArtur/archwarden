# Contributing to archwarden

archwarden decides whether someone's CI passes and whether a coding agent's
write is accepted. That is the whole reason the bar here is where it is: a
false positive blocks legitimate work, and a false negative quietly erodes the
reason anyone installed the tool. Most of what follows is downstream of that
one sentence.

Read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) before your first change,
and [`docs/DECISIONS.md`](docs/DECISIONS.md) before proposing one that changes
what the tool *is*. A surprising number of "obvious" improvements were
considered and rejected on record, with the reasoning kept — which means
arguing against a decision is a normal thing to do here, as long as you argue
against the reason that is written down rather than around it.

## Getting set up

```bash
git clone https://github.com/HenriqueArtur/archwarden
cd archwarden
cargo xtask hooks     # points core.hooksPath at .githooks/
cargo build
```

`rust-toolchain.toml` pins rustc `1.96.0`, so rustup installs the right
compiler on first build and every contributor compiles with the same one.
There is no separate MSRV job because there is no second version in play.

### When the disk fills up

```bash
cargo xtask clean          # the caches that cost a rebuild nothing
cargo xtask clean --deps   # plus the compiled dependencies; next build is cold
cargo xtask clean --all    # what `cargo clean` does
```

`cargo clean` is all or nothing, and all is usually wrong: it takes the
compiled dependencies for the sake of space that was not the problem. The space
that *is* the problem is incremental compilation state, which grows without
bound and buys a few seconds per rebuild. Measured here once, `target` was
**59 GB** — 27 of it `debug/incremental` and 28 `debug/deps`. The default tier
takes the 27.

It also sweeps `cargo-mutants` build trees left in the temporary directory.
The pre-push hook builds one every push and removes it when it finishes; when
it is *killed* it does not, and each orphan is a whole build tree. One killed
run here was holding 62 GB.

`target/criterion` is never taken by any tier. Benchmark history is data, not
cache, and a benchmark that cannot be compared against its own past is a number
with nothing to say.

### Looking at the HTML reports

```bash
cargo xtask preview
```

Writes both pages, in both languages, into `target/preview/`, by building a
fixture repository and running the real binary against it. Open them.

A page is for a human and a human has to *look* at it: judging one by reading
its source is judging a drawing by reading its coordinates. Nothing there
builds a fake report — a preview assembled from hand-made data drifts from what
the tool emits, and a design signed off against a page the product does not
produce is worth less than nothing.

Re-run it after any change to a renderer, to the stylesheet, or to
`phrases.rs`.

`cargo xtask hooks` is not optional in spirit. It installs two hooks:

| Hook | Runs | Costs |
| --- | --- | --- |
| `pre-commit` | `cargo fmt --all --check`, `typos` | ~1.5s |
| `pre-push` | `cargo mutants --in-diff` | ~10s floor, then ~1s per mutant |

Both skip gracefully when their optional tool is not installed, and both honour
`--no-verify`. A hook is here to catch mistakes, not to take the decision away
from whoever is pushing.

The tools CI uses, none of which are required to build:

```bash
cargo install cargo-nextest cargo-llvm-cov cargo-deny cargo-machete
cargo install typos-cli cargo-mutants
```

`typos-cli` may not build on a small machine: its dictionary is one enormous
generated table, and `rustc` holds the whole thing in memory whatever the
optimisation level. On a 4 GB box it is killed by the OOM killer, at
`opt-level=0` too. Take the prebuilt binary instead — it is what CI does:

```bash
gh release download --repo crate-ci/typos --pattern '*aarch64-unknown-linux-musl.tar.gz'
```

Node 22 and `python3` are needed only for the distribution tests
(`npm/archwarden/test/`, `scripts/test_*.py`).

## Before you open a pull request

Run what CI runs. In order of how likely each is to catch you:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
cargo test --workspace --doc                    # nextest does not run doctests
cargo xtask check-schema
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
typos
cargo machete
cargo deny check
```

And, if you touched the distribution:

```bash
cd npm/archwarden && node --test "test/*.test.mjs"
python3 -m unittest discover -s scripts -p 'test_*.py'
```

`RUSTFLAGS: -D warnings` is set for the whole CI run, so a warning is a failed
build there even where it is not one locally.

## The rules that are not negotiable

These are enforced by the build rather than by review, which is deliberate —
review catches what a reviewer happens to look at.

**No `unsafe`.** `unsafe_code = "forbid"` in the workspace lints, and `forbid`
cannot be locally overridden the way `deny` can. archwarden has no reason to
reach for it.

**No panics in production paths.** `unwrap`, `expect`, `panic!`, `todo!`,
`unimplemented!` and slice indexing are `deny` at the workspace level.
archwarden runs as a CI gate, a pre-commit hook and an agent pre-write hook; in
all three a panic is worse than a reported error, because it produces a stack
trace where a diagnostic belongs and, in the hook case, interrupts a write with
nothing actionable. `clippy.toml` relaxes every one of these inside
`#[cfg(test)]` — `unwrap()` on a value the test just built is idiomatic and
reads better than the alternative. `dbg!` stays denied even there.

**Libraries do not print.** `print_stdout` and `print_stderr` are denied
outside the binary crates. Libraries return values; the CLI decides how to say
them.

**Coverage floors are floors.** `archwarden-core` is held at 99% lines / 100%
functions, everything else at 95% lines. Never lower one to make a red build
green. It is pure logic with no I/O, so an uncovered line there means either a
missing test or a branch no input can take — and both deserve to fail the
build. The line floor is 99 rather than 100 only because `cargo-llvm-cov`'s
summary reports one phantom miss inside macro-expanded code in `glob.rs` that
its own lcov, JSON and HTML reports all show as covered.

## Tests

[`docs/TESTING.md`](docs/TESTING.md) is the full strategy. The short version:

**Tier 1 — unit.** Next to the code, one behaviour per test, inline fixtures,
no filesystem when avoidable.

**Tier 2 — integration.** `crates/archwarden-cli/tests/`, driving the real CLI
against synthetic repos in `tempfile` directories. JSON output is snapshotted
with `insta`; reviewing that diff is part of reviewing the PR. Never a
checked-in `node_modules` — resolvers get synthetic `package.json` stubs.

**Tier 3 — differential.** archwarden against `dependency-cruiser` over the
same real repository, `cargo test -p archwarden-engine --features differential`.
Runs on PR CI like every other tier; target repos come from the
`ARCHWARDEN_DIFF_REPO` repository variable rather than being checked in, and
with none configured the test says why it did nothing and passes.

Write the test first. `cargo nextest` treats "zero tests ran" as an error,
which on this project is exactly right.

### Mutation testing, and how to read a survivor

The `pre-push` hook runs `cargo-mutants` over your diff. It is the only check
here that answers *"is this tested"* rather than *"does this run"* — coverage
says a guard executed and says nothing about whether deleting it would fail
anything.

It earned that slot the hard way. A refusal that stops `impact --apply` from
carrying out a move which would break a repository shipped with **no test at
all**, and so did four others. The suite was green. Coverage was green.
Flipping one boolean would have removed the protection silently.

Each survivor is an edit to your code that no test objected to. Either write
the test, or decide the mutant is harmless and say why, in a comment where the
next person will find it. `cargo-mutants` exits `2` when mutants survived and
something else when it could not run at all — a build failure, a timeout, the
linker being killed on a small machine. Only the first blocks; the second warns
and lets the push through, because a hook that stops you pushing when your
laptop ran out of memory is a hook you learn to bypass and never run again.

### Taking a test case from another project

Reference tools encode decades of edge-case knowledge and we want it. We do not
want their code. The process — read the original, describe the *behaviour* in
one prose sentence in our test file, reimplement from that description as if
the original did not exist, then cite the origin in a comment:

```rust
// Inspired by: dependency-cruiser test
//   test/extract/resolve/tsconfig-paths.spec.mjs::"resolves aliased import"
```

A line-by-line port is a derivative work and carries the original licence, even
translated into Rust. The accepted licences and the current source list are in
[`docs/TESTING.md`](docs/TESTING.md); adding a source means adding it there too.

## Commits

Conventional commits, lowercase subject, describing the behaviour rather than
the edit:

```
feat(resolver): resolve workspace packages without node_modules
fix(dist): static musl for Linux, and a floor that is a decision
test: move mutation testing from nightly to pre-push
```

The subject says what changed. **The body says why**, and it is the part that
matters — this repository's history is used as documentation, and several of
the commit bodies are better explanations than anything in `docs/`. If your
change has a measurement behind it, put the number in the body. If it was a
trade, name what lost.

Scopes in use: `resolver`, `parser`, `rules`, `engine`, `cli`, `config`,
`cache`, `impact`, `check`, `spec-pair`, `orphans`, `docs`, `dist`, `xtask`.

## What has to move with your change

The tool's surface is documented in several places, and a change that updates
one and not the others ships a lie:

- **`docs/RULES.md`** — semantics of a rule kind. Anything a config author
  needs to predict what a rule does.
- **`docs/CONFIG.md`** — the config surface itself, and refusals a user can
  hit.
- **`AGENTS.md`** — every agent-facing command with its *real* JSON output.
  This one ships inside the npm package, so a repository that installs
  archwarden reads it from `node_modules/archwarden/AGENTS.md` at the version
  it installed. Changing what a command prints and not changing this file
  breaks agents in the field.
- **`schema/v0.json`** — generated, never hand-edited. Run `cargo xtask
  gen-schema` and commit the result; CI fails on drift, because editors fetch
  the schema by URL and a missing field silently stops appearing in completion.
- **`CHANGELOG.md`** — under `Unreleased`, in the section that matches. See
  [`docs/RELEASING.md`](docs/RELEASING.md).
- **`docs/DECISIONS.md`** — see below.

### When to write a decision

Add an entry when the change locks the project into something, or deliberately
declines something a reasonable person would expect. New entries go at the top;
the next number is 17. The format is context, decision, alternatives weighed,
consequences — and the alternatives section is the load-bearing one, because
its job is to stop the same argument being had again in a year.

Not every change needs one. A bug fix does not. "We are not doing X, and here
is why" almost always does.

## Opening a pull request

Small and focused beats large and thorough. A PR that fixes one issue is one
review; a PR that fixes four is four reviews serialised behind the slowest one.

Say in the description what a reviewer should be suspicious of. If you left
something out, say that too — an explicit gap is a decision, and a silent one
is a bug someone else finds later.

If you are unsure whether an idea fits before writing it, open an issue first.
Design questions are cheaper in prose than in a diff.

## Reporting bugs

The issue templates ask for version, platform, a reproduction, and what you
ruled out. That last field is not busywork — the strongest reports this project
has received named the things they had already eliminated, and it is what makes
a bug fixable by someone who cannot reproduce it.

Security issues do not go in the issue tracker. See
[`SECURITY.md`](SECURITY.md).

## Licence

archwarden is dual-licensed under MIT or Apache-2.0, at the user's option,
following the Rust convention (decision 10). Contributions submitted for
inclusion are dual-licensed the same way, without any additional terms or
conditions. There is no CLA.
