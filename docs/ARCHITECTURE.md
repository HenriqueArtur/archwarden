# Architecture

This document describes the internal shape of archwarden. It is a design
document, not a spec of shipped behaviour.

## Design goals

1. **Sub-second full runs on 100k-file repos.** Sub-100 ms incremental updates
   once watch mode lands in v1.
2. **Cache-first**: incremental analysis is a requirement, not an optimisation.
3. **Deterministic**: same inputs produce the same output and the same exit code.
4. **Swappable parser and resolver**: the JS/TS front-end must be replaceable
   without touching rule engines. See "Resolver abstraction" below.
5. **Zero toolchain for end users**: distributed as a single native binary,
   installable through npm, cargo-binstall, or homebrew.
6. **Agent-friendly**: JSON output and dedicated commands (`explain`,
   `describe`, `scaffold`, `agent-guide`) so coding agents can query the
   ruleset before writing code, not only after. See
   [`AGENT-INTEGRATION.md`](AGENT-INTEGRATION.md).

## Pipeline

```
config load
   │
   ▼
walk (parallel, respects .gitignore)
   │
   ▼
cache probe ──► hit ──► reuse findings
   │
   ▼ miss
parse (oxc_parser)
   │
   ▼
resolve imports (Resolver trait, default = oxc_resolver)
   │
   ▼
extract facts (imports, exports, calls, file metadata)
   │
   ▼
run rule engines (structure, naming, spec-pair, imports, calls)
   │
   ▼
persist cache
   │
   ▼
report (text, JSON, or LSP diagnostics)
```

Each stage is a boundary. Stages communicate through owned data structures,
never through shared mutable state. This makes stages independently testable
and independently swappable.

## Modules and crates

Planned Cargo workspace layout:

```
crates/
  archwarden-core/       # types, traits, path matcher, case transforms
  archwarden-config/     # config schema, loading, extends, validation, doctor
  archwarden-parser/     # oxc_parser wrapper, AST facts extraction
  archwarden-resolver/   # Resolver trait + oxc_resolver impl
  archwarden-rules/      # rule engines (one module per category)
  archwarden-cache/      # on-disk cache (redb, content-hash keyed)
  archwarden-engine/     # pipeline orchestration: walk → parse → resolve → rules
  archwarden-api/        # the operations every surface goes through
  archwarden-cli/        # binary crate, arg parsing, terminal and HTML output
                         # archwarden-mcp/ and archwarden-lsp/ arrive later
                         # and depend on api, not on cli
```

Dependency direction:

```
core   ← config ← engine ← api ← cli
  ↑        ↑        ↑        ↑
  └── parser, resolver, rules, cache
```

`archwarden-core` has no internal dependencies. Everything else depends on it.

`archwarden-engine` exists so that the pipeline is not owned by the binary
crate: `archwarden-lsp` needs the same pipeline, and depending on a binary
crate to get it would be backwards.

`archwarden-api` exists for the layer above that, and issue #63 is the
argument. The engine runs the rules; assembling a configuration, walking the
repository, applying a baseline and deciding what to show were all in
`archwarden-cli` — entangled with how *that* surface reports failure.
`prepare()` wrote a miette report to stderr and returned an exit code, so any
surface that reports failure differently could not reuse it. Two did not: the
pre-write hook answers in JSON and exits clean, the end-of-turn hook says
nothing, and both re-implemented the path. The version guard was missing from
one of the copies, which shipped as issue #55 — a config from a future version
parsed into one with no rules, compiled, matched nothing, and permitted every
write.

So the rule the crate exists to enforce, and the one to break before anything
else in it is worth reading:

> **Nothing in `archwarden-api` writes, and no function in it takes a writer.**
> Every failure is a value the caller renders.

The workspace already denies `print_stdout` and `print_stderr` outside the
binaries, and that lint never caught `prepare()`, which wrote through a
`&mut dyn Write` it was handed. The enforcement here is structural instead: the
crate does not depend on `archwarden-cli`, and no signature in it mentions an
output sink. The one exception is `render`, where a `Renderer` writes only
where the caller pointed it and reports no failure by writing.

Its stages are named — `Resolve → Load → Walk → Evaluate → Present` — even
where there is only one implementation of each, because that is what lets a
future surface say *"the LSP reuses through Evaluate and brings its own
Present"* instead of negotiating the boundary from scratch.

`archwarden-config` depends on `archwarden-resolver` because `extends`
accepts npm package names (`"@myorg/arch-preset"`), and turning one into a
file path is full Node module resolution — `node_modules` lookup, `exports`
conditions, pnpm symlinks, yarn PnP. `oxc_resolver` already does this
correctly and there is no reason to hand-roll a second, worse copy. The
dependency is acyclic (`config → resolver → core`).

The rule engines depend on `archwarden-core` types only, never on the parser
or resolver directly. They receive already-extracted facts. This is the
seam that lets us swap the parser later.

### Wire types vs compiled types

`archwarden-config` owns the *wire format*: the structs that carry
`Deserialize` and `JsonSchema`, where a glob is a `String`. It lowers them
into `archwarden-core`'s *compiled* types, where a glob is a built `GlobSet`
and a pattern is a compiled `Regex`.

This is not boilerplate — it is compilation that has to happen anyway, and it
buys a real invariant: a `CompiledRule` cannot exist unless its globs and
regexes are valid, so no downstream code ever has to ask. It is also why
`archwarden-rules` can depend on `core` alone, exactly as claimed above.

## Resolver abstraction

The default resolver is `oxc_resolver`. It is behind a trait so it can be
replaced.

```rust
pub trait Resolver: Send + Sync {
    fn resolve(
        &self,
        importer: &Path,
        specifier: &str,
    ) -> Result<ResolvedImport, ResolveError>;
}

pub struct OxcResolver { /* ... */ }
impl Resolver for OxcResolver { /* ... */ }
```

Reasons to keep this pluggable:

- `oxc_resolver` is young. If it stops being maintained, or if a specific
  edge case cannot be handled, we can drop in a different impl.
- Users with exotic build systems (Bazel, custom bundlers) may need custom
  resolution.
- Testing rules that depend on the graph is easier with an in-memory
  resolver fixture.

Rule engines never call the resolver themselves. They consume `ResolvedImport`
values that the fact-extraction stage already populated. This means changing
the resolver never requires changing rule code.

## Parser abstraction

Same principle as the resolver. `archwarden-parser` exposes a `Parser` trait
whose default impl uses `oxc_parser`. Fact extraction is done in this crate;
downstream code sees only extracted facts:

```rust
pub struct FileFacts {
    pub path: PathBuf,
    pub imports: Vec<ImportFact>,
    pub exports: Vec<ExportFact>,
    pub calls: Vec<CallFact>,
    pub content_hash: Hash,
}
```

If we later need to support another language (say, Python), the shape is
already there: a `PythonParser: Parser` implementation feeding the same
`FileFacts`. Adding new fact kinds means extending the struct — no rule
engine needs to know where the facts came from.

## Cache design

Content-addressed, on-disk, at `.archwarden/cache/` in the repo root.
Gitignored. Stored in a **redb** database — small records, high read volume,
cheap random access. Random access matters specifically for
`check --file`: the Layer 4 pre-write hook spawns one process per agent write
and must read a single entry out of tens of thousands inside a ~20 ms budget,
without deserialising the rest.

### Two tables, two keys

Facts and findings have different invalidation triggers, so they are keyed
separately. Storing both under one combined key would throw away every parse
result in the repo whenever a single rule changes.

| Table | Key | Value |
|---|---|---|
| `facts` | `blake3(file_content)` | serialised `FileFacts` |
| `findings` | `blake3(content_hash ++ rules_hash ++ resolution_epoch)` | findings for that file |

- `rules_hash` — hash of the effective rules for that file, so a config edit
  invalidates only the findings, not the parse.
- `resolution_epoch` — hash of every `tsconfig*.json`, `package.json`, and
  lockfile in the repo. Import-boundary findings depend on how specifiers
  resolve, and resolution depends on those files. Without this component,
  changing `tsconfig.paths` and running warm serves stale boundary findings.

### Invalidation

- Cache format version bumps invalidate everything (decision 3).
- A changed file content hash recomputes that file's facts and findings, plus
  the findings of files whose boundary rules depend on its resolved path.
- A changed `resolution_epoch` invalidates findings only; facts survive.

Cross-file dependencies (import boundaries) require a small reverse-index:
"if file A's exports changed, which files import A?" This index is rebuilt
lazily from the cache on startup.

A watch mode would subscribe to filesystem events via `notify` and re-run
only the affected files against this same cache. It is not built, and
currently not planned: the LSP covers the editor loop it was meant for, and
whether it is needed at all depends on a warm-run measurement nobody has
taken. Tracked in the issues.

## Concurrency

- File walk: parallel via `ignore::WalkBuilder`. Note that `ignore` runs its
  own thread pool (crossbeam channels over std threads), *not* rayon. The walk
  pool and the rayon pool are separate and must be sized together, or they
  oversubscribe the machine.
- Parsing and fact extraction: parallel via `rayon`, one task per file.
- Rule engines run against the fact set. Almost all of them are file-local and
  trivially parallel — including import boundaries and call obligations, which
  need their own file's imports *resolved* but nothing from any other file.
  The exceptions are the two graph rules, `import-cycle` and an
  `import-boundary` that sets `forbid_reaching`: those need every file's edges
  before any of them can be answered, so the run holds them back, builds one
  `ImportGraph` when the walk is done, and asks them then. A configuration
  carrying one also suspends the per-file gating below — see decision 21 for
  what that costs.
- The pipeline is a `par_iter().map().collect()` over the file list: each
  stage takes owned inputs and returns owned outputs, so no stage needs
  shared mutable state. Cache writes are collected and batch-flushed once at
  the end of the run.

## Config loading

- Discovery walks up from the CWD until a file named `arch.config.json` is
  found or the filesystem root is reached.
- The config is parsed by `serde_json` against the schema types in
  `archwarden-config`.
- Schema is generated at build time by `schemars` and published so editors
  pick it up via the `$schema` field.
- After parsing, the doctor pass runs (see `docs/CONFIG.md`) to catch
  regexes that never match, roots that point to non-existent paths, and
  cross-rule inconsistencies.

## Output formats

- Default: human-readable text with grouped findings and file paths.
- `--format json`: stable JSON schema, one object per finding, versioned.
- `--format markdown`: used by `agent-guide` to produce grep-friendly
  rule digests for `CLAUDE.md` / `AGENTS.md` context.
- `--format sarif`: (post-v1) SARIF 2.1.0 for GitHub code scanning.
- LSP diagnostics: (post-v1) via `archwarden-lsp`.

## Agent-facing commands share the check pipeline

`describe`, `scaffold`, and `agent-guide` are not a parallel implementation.
They reuse the same rule-matching engine used by `check`:

- `describe <path>` runs the "which rules apply to this path?" phase for
  the given path (a query the matcher already performs internally per
  file) and formats the result. No parsing needed.
- `scaffold <path>` runs the same query and then translates each matched
  rule into its "minimal satisfying shape" description. This is a pure
  function of the rule definition — every rule kind implements a
  `describe_expectation()` method that scaffold consumes.
- `agent-guide` iterates every rule in the config and renders the compiled
  rule itself — its scope globs, filename patterns, name template, import
  globs — as markdown or JSON.

  It does **not** go through `describe_expectation()`, and cannot: that method
  takes a path, deliberately, because a `naming` rule's expectation carries the
  *rendered* export name and the name comes from the filename. A guide has no
  filename. Inventing one would fill the digest with names derived from a path
  nobody will create.

  The property this section is about survives anyway. The compiled rule is the
  same value the engines consume, so the guide cannot misstate a rule's globs,
  patterns or templates; and the precise per-path answers are `describe` and
  `scaffold`, which do go through the expectation seam.

This means adding a new rule kind requires implementing both the check
logic and the expectation description in the same place, which keeps
`scaffold`/`agent-guide` output in lockstep with what the checker
actually enforces. There is no risk of drift between "what the docs say
the rule requires" and "what the checker enforces" because both come
from the same code.

## Distribution

- Native binaries built in CI for macOS (x86_64, aarch64), Linux
  (x86_64, aarch64, musl), and Windows (x86_64).
- npm package `@archwarden/cli` acts as a shim that downloads the correct
  binary at install time, following the pattern used by esbuild and Biome.
- `cargo binstall archwarden` fetches the same binaries.
- Homebrew formula for macOS/Linux.

## What is explicitly excluded from v0

- Custom rules written by users. All v0 rules are built in. A plugin API
  is a v2 topic — the internal rule engine interface must stabilise first.
- Non-JS/TS languages. The parser trait exists to allow it later, but no
  other language ships in v0.
- Watch mode and the LSP server. Both are v1. In v0 the local-feedback need
  is covered by `check` on a warm cache, which is fast enough for a
  `pre-commit` hook.
- Auto-fix. archwarden reports; Biome fixes what it can fix. Auto-fixing
  architectural violations is rarely safe.
