# Architecture

This document describes the internal shape of archwarden. It is a design
document, not a spec of shipped behaviour.

## Design goals

1. **Sub-second full runs on 100k-file repos**, sub-100ms watch-mode updates.
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
  archwarden-core/       # types, traits, rule engine interfaces
  archwarden-config/     # config schema, loading, validation, doctor
  archwarden-parser/     # oxc_parser wrapper, AST facts extraction
  archwarden-resolver/   # Resolver trait + oxc_resolver impl
  archwarden-rules/      # rule engines (one module per category)
  archwarden-cache/      # on-disk cache (content-hash keyed)
  archwarden-cli/        # binary crate, arg parsing, output formats
  archwarden-lsp/        # (post-v1) language server for editor integration
```

The rule engines depend on `archwarden-core` types only, never on the parser
or resolver directly. They receive already-extracted facts. This is the
seam that lets us swap the parser later.

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

Content-addressed, on-disk, one entry per file per config version.

- Cache key: `blake3(file_content) ++ blake3(effective_rules_for_file)`.
- Cache value: serialised `FileFacts` and the rule findings against that file.
- Invalidation: if the config hash changes, the whole cache is stale. If a
  file's content hash changes, only that file (and files whose findings
  depend on its exports) are recomputed.
- Storage: `.archwarden/cache/` at the repo root. Gitignored. Structured as
  a sled or redb database — small files, high read volume, cheap random
  access.
- Watch mode subscribes to filesystem events via `notify` and re-runs
  affected files only.

Cross-file dependencies (import boundaries) require a small reverse-index:
"if file A's exports changed, which files import A?" This index is rebuilt
lazily from the cache on startup.

## Concurrency

- File walk: parallel via `ignore::WalkBuilder` (uses `rayon` internally).
- Parsing and fact extraction: parallel via `rayon`, one task per file.
- Rule engines run against the fact set. Structure, naming, and spec-pair
  rules are file-local and trivially parallel. Import boundaries and call
  obligations need the assembled graph and run after fact extraction
  completes.
- Cache reads and writes: `dashmap` for in-memory sharing during a run,
  batch-flushed to disk at the end.

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
- `agent-guide` iterates every rule in the config and calls the same
  `describe_expectation()` per rule, formatted as markdown or JSON.

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
- LSP server. Watch mode covers the local-feedback need in v0.
- Auto-fix. archwarden reports; Biome fixes what it can fix. Auto-fixing
  architectural violations is rarely safe.
