# Working on archwarden

A map for an agent working **on** this repository.

Not to be confused with [`AGENTS.md`](AGENTS.md), which is about **using**
archwarden in somebody else's repository and ships inside the npm package.
Different audience, different file. If you are here to change this project,
this is your file.

The documentation is good and there is a lot of it — around 50 KB across
`docs/`. This page exists so you know which part to read before you start,
instead of finding out afterwards.

## Read this before you change behaviour

**[`docs/DECISIONS.md`](docs/DECISIONS.md) is numbered, and most of what looks
arbitrary in this codebase is in it.** Before changing how something behaves,
find the decision that governs it and read the *Alternatives* section. That
section exists to stop an argument being had twice, and re-deriving a rejected
option from scratch is the failure it was written to prevent.

If no decision governs your change, you may be creating one. See
"What has to move with your change" below.

The ones that come up most:

| Decision | What it settles |
|---|---|
| 30 | A claim in a comment is a fact; `metadata` reads the file header |
| 22 | Operations belong in `archwarden-api` — the ones every surface asks, not the ones `check` needs |
| 21 | A graph rule reads the whole repository, and says what that costs |
| 20 | Nothing in `archwarden-api` writes, and no function in it takes a writer |
| 19 | **What a new language front-end costs.** Read before touching the parser |
| 13 | `--fix` stays out, and `spec-pair` is why |
| 6 | Resolver and parser sit behind traits |
| 3 | The cache is a v0 requirement; a format bump invalidates everything |

## The crate graph, and the one rule about it

```
core ← config ← engine ← api ← mcp ← cli
 ↑       ↑        ↑       ↑
 └── parser, resolver, rules, cache
```

Full version in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md). **Never invert
an arrow.** `mcp` cannot see `cli`, which is what makes an answer taken from
the wrong place fail to compile rather than fail in the field.

Two seams carry most of the design:

**Wire types vs compiled types.** `archwarden-config` owns the *wire* format —
structs with `Deserialize` and `JsonSchema`, where a glob is a `String`. It
lowers them into `archwarden-core`'s *compiled* types, where a glob is a built
`GlobSet` and a pattern is a compiled `Regex`. A `CompiledRule` cannot exist
with an invalid glob, so nothing downstream has to ask. This is why
`archwarden-rules` depends on `core` alone.

**Parser and resolver behind traits.** Rule engines never see an AST and never
call the resolver. They receive extracted facts (`FileFacts`, `DocFacts`).
Adding a language means implementing the trait and populating the same facts —
decision 19 prices exactly what that costs.

## Not negotiable, and enforced by the build

Full list in [`CONTRIBUTING.md`](CONTRIBUTING.md#the-rules-that-are-not-negotiable).
The ones you will hit:

- **No `unsafe`.** `forbid`, not `deny` — it cannot be locally overridden.
- **No panics in production paths.** `unwrap`, `expect`, `panic!`, `todo!`,
  `unimplemented!` and slice indexing are `deny` at the workspace level.
  `clippy.toml` relaxes all of them inside `#[cfg(test)]`; `dbg!` stays denied
  even there.
- **Libraries do not print.** `print_stdout` / `print_stderr` are denied
  outside the binary crates.
- **Coverage floors are floors.** `core` and `api` at 99/100 and 99/99,
  everything else 95% lines. Never lower one to make a red build green.

An `#[allow]` needs a `reason = "..."` that carries the argument. There are
about ten `clippy::too_many_lines` allows in the tree and every one names why
splitting would cost more than it buys — usually that an exhaustive `match`
over a closed enum is a table, and scattering its arms loses the compiler check
that makes "new rule kind without an engine" fail to build. Match that bar or
do not add the allow.

## Tests

**Test first. Watch it fail. Then implement.** This is not a preference here;
`cargo-mutants` runs on push and a survivor blocks it.

- Unit tests are **inline `#[cfg(test)]` modules**, in the same file as the
  code. That is Rust's convention and this project keeps it. Roughly 53% of
  every source file is its own tests — expect it when you open one.
- One behaviour per test. The name is a sentence:
  `a_tsconfig_path_alias_resolves_to_the_same_file_as_the_relative_form`.
- Full strategy, including the three tiers and the reference-source policy, is
  [`docs/TESTING.md`](docs/TESTING.md).

Running them:

```bash
cargo nextest run -p archwarden-rules      # one crate, seconds
cargo nextest run --workspace --all-features   # 2000+ tests
cargo xtask ci                              # every gate CI runs
```

`cargo xtask ci` treats a check whose tool is not installed as a **failure**,
not a skip. That rule exists because three pull requests in a row went red on
checks that had reported "skipped" locally in a message that read like a pass.

## What has to move with your change

A change that updates one of these and not the others ships a lie. Full list
in [`CONTRIBUTING.md`](CONTRIBUTING.md#what-has-to-move-with-your-change).

- **`docs/RULES.md`** — semantics of a rule kind.
- **`docs/CONFIG.md`** — the config surface and the refusals a user can hit.
- **`AGENTS.md`** — every agent-facing command with its *real* JSON output.
  This ships in the npm package; changing what a command prints without
  changing this breaks agents in the field.
- **`schema/v0.json`** — generated. `cargo xtask gen-schema` and commit it.
  Never hand-edit; CI fails on drift.
- **`CHANGELOG.md`** — under `Unreleased`.
- **`docs/DECISIONS.md`** — when the change locks something in, or declines
  something a reasonable person would expect. New entries at the top; the next
  number is one past the highest already there.

Commits are conventional, lowercase subject, describing the behaviour rather
than the edit.

## Hooks

Installed by `cargo xtask hooks`, which points `core.hooksPath` at
`.githooks/`.

- **pre-commit** — `cargo fmt --check` and `typos`. About 1.5s, deliberately.
- **pre-push** — `cargo xtask ci`, then `cargo mutants --in-diff`. Slow on
  purpose; safety is worth the wait.

Both are skippable with `--no-verify`, and that is deliberate: a hook catches
mistakes, it does not take the decision away from whoever is pushing.

## Traps

- **`AGENTS.md` is not about this repo.** It is the shipped product surface.
- **A rule's `roots` glob selects directories, never files.** `docs/RULES.md`
  opens with why: it lets `describe` answer for a file that does not exist yet,
  with no disk access.
- **`FileClass::UnreadableSource` is not a hole, it is a plug.** A `.py` under
  an import rule is a *counted, named* skip. A rule enforcing nothing must
  never look like a repository that satisfies it — `docs/CONFIG.md` calls that
  the worst failure a linter has, and it is the argument behind several
  decisions.
- **The cache has two tables with different keys.** Facts are keyed by content
  hash; findings by content hash + rules hash + resolution epoch. A config edit
  must not throw away parse results.
- **Long functions here are usually exhaustive matches.** Check for an
  `#[allow]` with a reason before deciding one is too long.
