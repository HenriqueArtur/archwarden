# Rule categories

archwarden ships five rule categories in v0. Each has narrow, well-defined
semantics. This document is the reference for what each rule can and cannot
express. Config syntax lives in [`CONFIG.md`](CONFIG.md).

Ordering: cheap and file-local first, expensive and graph-wide last. The
engine runs them in the same order for cache-friendly evaluation.

---

## 1. Structure

**What it enforces**: which folders may exist under a module, and (optionally)
which filenames may exist inside a folder.

**Scope**: file-local. No cross-file reasoning. Runs on the walk output.

**Two sub-modes**:

- **Allowed subfolders**. Given a set of module roots (e.g., every direct
  child of `packages/domain/src/`), only listed subfolder names may exist.
  Extras are errors. A separate list may mark subfolders as warnings
  (documented technical debt).
- **Filename patterns**. Given a root, every file inside must match at
  least one of a set of regexes. Non-matching files are errors.

**Recursion**. Some modules have nested modules of the same shape (e.g.,
"variants" of an entity). The `recurse_into` field lists subfolders that
carry the same structural contract as the parent, recursively.

**Escape hatch**. Directories prefixed with `_` are skipped by the walk.
Convention borrowed from Next.js; used for internal helpers that are not
themselves part of the module structure.

**Cannot express**: relative ordering of files, "at least one file of
kind X must exist" — those belong to a future `presence` rule and are
not in v0.

---

## 2. Naming coupling

**What it enforces**: the exported symbol name inside a file must be
derivable from the filename by a case transform.

**Scope**: file-local. Needs parse (to inspect exports) but not resolve.

**Shape**:

- `file_pattern` — regex with a named capture group (typically `name`).
- `must_export` — describes the required export: kind (`function`, `const`,
  `class`, `type`, `interface`), name (templated from the capture group).

**Case transformers available in templates**: `pascal`, `camel`, `kebab`,
`snake`, `upper`, `lower`, `raw`.

**Multiple exports**. If a file has other exports besides the required
one, they are ignored. The rule enforces presence and correctness of the
required export, not exclusivity.

**Cannot express**: constraints on the *type* of the export (e.g.,
"function returning `UseCase<X>`"). That is type checking, not
architecture linting. Use `tsc` for that.

---

## 3. Spec pairing (TDD gate)

**What it enforces**: every unit file under configured subfolders must
have a `.spec.<ext>` sibling.

**Scope**: file-local. No parsing required unless `require_non_empty_spec`
is set.

**Shape**:

- `subfolders` — which folders under each root are subject to the rule.
  Use `["."]` to apply to the root itself.
- `spec_suffix` — the expected sibling suffix (default `.spec.ts`).
- `ignore_files` — exact repo-relative paths exempted (usually type-only
  files with no runtime behaviour to test).
- `require_non_empty_spec` (bool, default `false`) — if true, the spec
  file must contain at least one `it(...)`, `test(...)`, or `describe(...)`
  call. This is the "TDD gate" flag: it prevents empty stubs from
  satisfying the rule.

**Default ignores** (baked in, not configurable):
- Files ending in the configured `spec_suffix` (obviously).
- `index.ts`, `index.tsx`.
- `DOC.md`, `README.md`.

**Cannot express**: test-first ordering (that the spec was written before
the impl). Git history could tell us, but making that a gate would be
noisy and unreliable. `require_non_empty_spec` is the practical proxy.

---

## 4. Import boundaries

**What it enforces**: layer A may not import from layer B; or, layer C
must import from layer D.

**Scope**: graph. Requires parse + resolve.

**Two directions**:

- **Forbid** — `from` matches importer path, `forbid_import_from` matches
  resolved import path. If both match, the import is illegal.
- **Require** — `from` matches importer path, `must_import_from` matches
  resolved import path. If `from` matches but no import satisfies the
  requirement, the file is illegal.

**Exceptions**. Each rule accepts `except`, a list of globs matched against
the *resolved* import path. Common use: "UI may not import domain, except
type-only imports from `domain/*/types/**`".

**Path matching semantics**. Globs are applied against the repo-relative
resolved path (never against the specifier string). This means aliases,
`tsconfig` paths, and workspace symlinks are resolved before matching —
there is exactly one canonical path per import, and rules operate on it.

**Type-only imports**. TypeScript's `import type` and inline `type` marks
are extracted separately. Rules may opt in with `include_type_only: false`
(default `true`).

**Cannot express**: transitive prohibitions ("X may not reach Y through
any chain"). That is a graph reachability question and belongs to a future
rule if there is demand.

---

## 5. Call obligations

**What it enforces**: files matching a pattern must contain at least one
call to a specific imported symbol.

**Scope**: file-local AST call-graph, plus resolved import to identify the
symbol source.

**Shape**:

- `file_pattern` — regex over filename.
- `must_call.symbol` — the callable, as it would appear at the call site
  (`Event.save`, `saveEvent`, `logger.audit`). Method chains are matched
  exactly.
- `must_call.imported_from` — the module the symbol must be imported from.
  This disambiguates same-named functions from different packages.

**How the check works**:

1. Resolve the file's imports. Confirm the target symbol is imported from
   the required module. If not: fail immediately with a clear reason
   ("expected import missing").
2. Walk the AST collecting call expressions. If at least one matches the
   symbol shape, the rule passes.
3. Basic reachability: calls inside `if (false)` or unreachable branches
   are not filtered out. archwarden is a structural linter, not a taint
   analyser.

**Cross-file call graphs**. Not in v0. If the required call is delegated
to a helper in another file, the rule will not follow through. Workaround:
scope the rule to a folder shape where the call is expected to be direct
(e.g., route handlers, not the helpers they use).

**Cannot express**: ordering of calls, argument values, conditional
obligations ("call `Event.save` only when the body has X"). Those cross
into program analysis territory and are out of scope.

---

## Rule interaction and evaluation order

Rules run in this order for cache friendliness:

1. Structure (walk only, no parse)
2. Naming coupling (parse required for exports)
3. Spec pairing (walk only, unless `require_non_empty_spec` — then parse)
4. Import boundaries (parse + resolve; needs full graph)
5. Call obligations (parse; needs resolved imports to match `imported_from`)

Each rule reports its own findings independently. A file may be flagged by
multiple rules — the report groups findings by file and by rule id.

## Levels

- `error` — non-zero exit code, blocks CI.
- `warning` — reported, exit code stays zero. Used for documented technical
  debt that must remain visible until paid down.

There is no `info` or `hint`. Two levels are enough; more encourages
warnings to be ignored.
