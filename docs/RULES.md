# Rule categories

archwarden ships six rule categories in v0. Each has narrow, well-defined
semantics. This document is the reference for what each rule can and cannot
express. Config syntax lives in [`CONFIG.md`](CONFIG.md).

Ordering: cheap and file-local first, expensive and graph-wide last. The
engine runs them in the same order for cache-friendly evaluation.

---

## Scope: how `roots` selects what a rule sees

Every rule declares a scope. **A scope glob always selects directories**,
never files. Each rule kind then declares what it inspects inside a selected
directory:

| Rule | Field | Inspects, inside each selected directory |
|---|---|---|
| `structure` | `allowed_subfolders`, `warn_subfolders` | the direct child directories |
| `structure` | `filename_patterns` | the direct child files, by basename |
| `naming` | `file_pattern` | the direct child files, by basename |
| `spec-pair` | `subfolders` | the listed subdirectories (`"."` = the directory itself), then files in them |
| `call-obligation` | `file_pattern` | the direct child files, by basename |
| `import-boundary` | *(scope only)* | every file in the directory is a candidate importer |
| `no-passthrough` | *(scope only)* | every direct child file's exports |

Recursion lives entirely in the glob. `apps/api/*` selects only the direct
child directories of `apps/api`; `apps/api/**` selects every directory under
it, recursively. `X/**` also selects `X` itself.

The field is named `roots` (an array) on every rule except `import-boundary`,
where it is named `from` because that reads naturally against
`forbid_import_from`. The semantics are identical — only the name differs.
Every glob field accepts either a single string or an array of strings.

**Why directories and not files.** `describe <path>` and the Layer 4 pre-write
hook must answer "which rules apply to this file?" for files that *do not
exist yet*. With this rule the answer is purely lexical and touches no disk:
split the path into `dirname` and `basename`, match `dirname` against the
scope, match `basename` against the rule's pattern. If scope meant different
things for different rule kinds, the matcher would need a branch per kind —
and that branch is exactly where `check` and `describe` would drift apart,
which decision 9 exists to prevent.

## Severity precedence

Within a single rule, **the most specific declaration wins**. Specificity is
decided by, in order: (1) the number of literal path segments in the glob,
(2) the length of the literal prefix, (3) declaration order — later wins.

This is what makes `warn_subfolders` work: naming a folder explicitly is more
specific than the rule's blanket `level`, so entries in `warn_subfolders`
report as `warning` regardless of the rule's `level`, and `level` applies to
folders in neither list. Still only two levels, as decision 1 requires.

Two boundaries on this:

- **It does not apply across rules.** Two rules matching the same file
  produce two independent findings with their own levels. A narrowly-scoped
  `warning` rule never downgrades a broadly-scoped `error` rule — otherwise
  adding a rule could silently weaken an existing gate.
- **`ignore` always wins over scope**, however deep the scope glob is. An
  ignore entry is a kill-switch, and a kill-switch that can be overridden by
  accident is not one. `config doctor` reports rules made unreachable this way.

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

**Escape hatch**. Directories prefixed with `_` are exempt from structure
rules. Convention borrowed from Next.js; used for internal helpers that are
not themselves part of the module structure. Configurable at the top level:

```json
"skip_dirs": {
  "prefixes": ["_"],
  "globs": [],
  "scope": "structure"
}
```

`scope: "structure"` (the default) exempts the directory from `structure`
rules only. Files inside it are still parsed, still enter the import graph,
and are still subject to every other rule.

`scope: "walk"` removes them from the walk entirely, making them invisible to
everything. This is available but rarely what you want: it turns
`mkdir _x && mv offender.ts _x/` into a way to bypass any import boundary.
`config doctor` warns when `scope: "walk"` coexists with `import-boundary`
rules.

Setting `prefixes: []` disables the escape hatch.

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
- `must_export` — describes the required export:
  - `kind` — one tag or a list of tags (see table below).
  - `name` — templated from the capture group.
  - `signature_hint` — optional free-form string. **Never verified.** It exists
    so `scaffold` can show a realistic skeleton
    (`export function Foo(deps: FooDeps): UseCase<FooInput, FooOutput>`)
    rather than just a name. Constraining the actual type is type checking;
    use `tsc`.

**Case transformers available in templates**: `pascal`, `camel`, `kebab`,
`snake`, `upper`, `lower`, `raw`.

**Export kinds**. Each export in a file is tagged. `kind` matches if the
export carries **any** of the listed tags. Note that an arrow function is not
a `function` — the declaration form is what is tagged, deliberately, so a rule
can require one and not the other.

| Source form | Tags | Matches `kind: "function"`? |
|---|---|---|
| `export function Foo() {}` | `function` | ✅ |
| `export async function Foo() {}` | `function` | ✅ |
| `export function* Foo() {}` | `function` | ✅ |
| `export const Foo = () => {}` | `const`, `arrow` | ❌ |
| `export const Foo = async () => {}` | `const`, `arrow` | ❌ |
| `export const Foo = function () {}` | `const` | ❌ |
| `export const Foo = 42` | `const` | ❌ |
| `export let Foo` / `export var Foo` | `let` / `var` | ❌ |
| `export class Foo {}` | `class` | ❌ |
| `export type Foo = ...` | `type` | ❌ |
| `export interface Foo {}` | `interface` | ❌ |
| `export enum Foo {}` | `enum` | ❌ |
| `const Foo = ...; export { Foo }` | tags of the local declaration | depends |
| `export { Foo } from './x'` | `reexport` | ❌ (see below) |

Valid tags: `function`, `arrow`, `const`, `let`, `var`, `class`, `type`,
`interface`, `enum`, `reexport`, `any`.

Use `kind: ["function", "arrow"]` to mean "callable, either form" — a good
default for presets.

**Default exports do not satisfy a named `must_export`.** A default export's
local name does not bind the importer (`import Whatever from './foo'`), and
this rule exists to couple filename to the symbol importers actually see.
`config doctor` warns when a file targeted by a `naming` rule exports only a
default.

**Re-exports** match `kind: "any"` or `kind: "reexport"` and fail any concrete
kind, with a specific reason: *"symbol is re-exported from './x'; kind is not
determinable without cross-file analysis"*. Following the re-export would make
a file-local rule cross-file, which is not what this rule is.

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

- `subfolders` — which folders under each selected directory are subject to
  the rule. Use `["."]` to apply to the directory itself.
- `spec_markers` — what makes a filename a spec. Default `["spec", "test"]`.
- `ignore_files` — repo-relative **globs** exempted (usually type-only files
  with no runtime behaviour to test). Globs, not exact paths, for consistency
  with every other path field in the config.
- `require_non_empty_spec` (bool, default `false`) — if true, the spec
  file must contain at least one `it(...)` or `test(...)` call. This is the
  "TDD gate" flag: it prevents empty stubs from satisfying the rule.
  **`describe(...)` does not count** — an empty `describe` block satisfies the
  letter of the rule while defeating its entire purpose.
- `skip_type_only` (bool, default `false`) — if true, a file whose exports are
  all `type` or `interface` needs no spec. See below.

### `skip_type_only`: files with nothing to test

A file with no runtime export has nothing a test can call. The rule still
demands a spec for one, and the spec that gets written to satisfy it tests a
mock of the contract rather than the contract — because there is nothing else
to test. That is work which reduces no risk, and `tsc` already checks an
interface on every build.

Three boundaries, each drawn where it is for a reason:

- **`enum` is a runtime export.** It has values a test can assert on, so a file
  exporting one is not a contract in the sense this exemption means.
- **A file with no exports at all is not type-only.** That is a file nobody
  imports, not a contract — and exempting it would make deleting the `export`
  keyword a way out of the rule.
- **A re-export is not type-only either.** Its real kind needs the file on the
  other side, which section 2 keeps this rule away from; guessing would exempt
  files on a coin flip.

**It costs a parse.** `spec-pair` otherwise reads no file at all, so a rule that
sets this reads every file in its scope — the same trade `require_non_empty_spec`
makes. On a repository of 3778 files with the flag on one rule, that is 137
files parsed that were not before.

What it replaces is a hand-maintained `ignore_files` list. On one real
repository the five entries in it were all interface-only service and adapter
files, and the flag removes all five while reporting exactly what the list
reported.

### How a spec is named

A spec is `<stem>.<marker>.<extension>`, and the parts come from different
places on purpose:

- the **stem** is everything before the file's last dot, so a compound name
  survives intact: `user.db.repository.ts` pairs with
  `user.db.repository.spec.ts`, and `create-client.use-case.ts` with
  `create-client.use-case.spec.ts`;
- the **marker** is a project's preference, and `spec_markers` configures it.
  The default accepts both, which is what vitest (`**/*.{test,spec}.?(c|m)[jt]s?(x)`)
  and jest (`**/?(*.)+(spec|test).[jt]s?(x)`) do, so the common project
  configures nothing;
- the **extension** is the source file's own and is not configurable, because
  `Component.tsx` wanting `Component.spec.tsx` is mechanical rather than a
  choice anyone makes differently.

Two asymmetries, both deliberate:

- **Recognising** an existing spec accepts any JS/TS extension, so a `.tsx`
  component tested by a `.ts` spec is satisfied. Refusing it would be a false
  positive on a file that plainly exists.
- **Suggesting** a missing spec uses the first marker and the source's own
  extension, so `scaffold` gives one deterministic answer rather than a list.

The marker must be the last stem component. `user.spec.ts` is a spec;
`user.spec.helper.ts` is a helper that happens to mention one. A bare
`spec.ts` counts, matching both runners' optional `<name>.` prefix.

**Default ignores** (baked in, not configurable):
- Files that are themselves specs.
- `index.ts`, `index.tsx`, `index.js`, `index.jsx` — barrel files re-export and
  hold no behaviour of their own.
- Anything that is not a JS/TS source file. This covers `DOC.md`, `README.md`,
  `package.json`, images and everything else in one rule, rather than naming
  them: nobody should have to declare that a PNG needs no test.

**Cannot express**: test-first ordering (that the spec was written before
the impl). Git history could tell us, but making that a gate would be
noisy and unreliable. `require_non_empty_spec` is the practical proxy.

---

## 4. Import boundaries

**What it enforces**: layer A may not import from layer B; or, layer C
must import from layer D.

**Scope**: graph. Requires parse + resolve.

**Shape**: an import boundary is an ordinary rule with
`type: "import-boundary"`. Its scope field is named `from` rather than `roots`
(see "Scope" above); the semantics are the same.

Because a boundary is cross-module by nature — "domain must not import
application" belongs to neither layer — boundaries usually live in the
top-level `rules` array rather than inside a `modules[].rules`. Both are
accepted; module membership is only a label for output.

**Two directions**:

- **Forbid** — `from` selects the importer, `forbid_import_from` matches the
  resolved import path. If both match, the import is illegal.
- **Require** — `from` selects the importer, `must_import_from` matches the
  resolved import path. If `from` matches but no import satisfies the
  requirement, the file is illegal.

**Exceptions**. Each rule accepts `except`, a list of globs matched against
the *resolved* import path. Common use: "UI may not import domain, except
type-only imports from `domain/*/types/**`".

`except` shields against `forbid_import_from` only. A rule that both requires
and forbids reads as "must reach A, must not reach B, and here are the corners
of B that are allowed" — an exception to a *requirement* would be a
requirement nobody has to meet.

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

**Also cannot express, in v0**: a prohibition on a *dependency* or a runtime
builtin — "the UI may not import `lodash` directly", "nobody imports
`node:fs`". Globs are matched against repo-relative paths, and neither an
installed package nor a builtin has one. The run counts them separately, so
the information is there when a rule for it is designed.

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

## 6. No passthrough

**What it enforces**: a file must add something of its own. A file whose whole
content is forwarding another module is an indirection wearing the name of a
layer.

**Scope**: file-local. Needs parse (to inspect exports and their initialisers),
not resolve.

**Three shapes**, configured with `forms` (default: all three):

| form | looks like |
|---|---|
| `reexport` | `export { A } from './x'`, or `import { A }` followed by `export { A }` |
| `alias` | `export const planToJson = planToJsonShared`, `export type PlanJson = PlanJsonShared` |
| `wrapper` | `export function f(a, b) { return g(a, b); }` |

**Why it earns a rule.** These three are how a folder survives years looking
like it has a purpose. An importer reaching a `shared/` module through a
`calcs/` file that only forwards it cannot tell the layer between them is
empty, and neither can a reader.

**A barrel file is case 1**, so `"no barrel files"` — a line many projects
carry in a `CLAUDE.md` with no enforcement at all — is
`forms: ["reexport"]` with `allow_package_entrypoints: false`. It is a
sub-case, not a kind of its own: two implementations of the same predicate can
disagree, and decision 9 exists to stop that.

**The wrapper test is syntactic.** "Same signature" in the type sense needs the
file on the other side and its types, which section 2 keeps file-local rules
away from. A wrapper that reorders arguments, drops one, or supplies a default
is doing something, and none of those match.

### The exceptions are not optional

Legitimate forwarding exists. The file a package's `exports` points at *is* a
public API, and forwarding is what a public API is for.
`allow_package_entrypoints` (default `true`) exempts it without anyone writing
a glob, and `except` takes globs for the rest. A rule that reported a package's
entire surface the day it was switched on is a rule nobody leaves on.

### `allow_partial`

Default `true`: only a file where **every** export is a forward is reported.

Set it to `false` to hear about the shape that hides best — a file
re-exporting six names from another module while declaring two of its own. That
file is not one that adds nothing, and saying so would be false; but six of its
eight exports are still an indirection its importers could skip. The finding
names exactly those six.

Measured on one real repository: **4 findings** with the default, **26** with
`allow_partial: false`. Both numbers are true and they answer different
questions, which is why it is a flag and not a default.

**Cannot express**: whether the forwarded module is the right one to import
instead. That is what `impact` is for.

---

## Rule interaction and evaluation order

Rules run in this order for cache friendliness:

1. Structure (walk only, no parse)
2. Naming coupling (parse required for exports)
2b. No passthrough (parse required for exports and their initialisers)
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
