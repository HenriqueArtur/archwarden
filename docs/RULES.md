# Rule categories

archwarden ships ten rule categories in v0. Each has narrow, well-defined
semantics. This document is the reference for what each rule can and cannot
express. Config syntax lives in [`CONFIG.md`](CONFIG.md).

Ordering: cheap and file-local first, expensive and graph-wide last. The
engine runs them in the same order for cache-friendly evaluation.

**Three of them need no parser at all.** `structure`, `presence` and `pair`
reason about names and paths on disk, so they work on a repository in any
language — or in none. `spec-pair` joins them unless `require_non_empty_spec`
or `skip_type_only` is set. The rules that do open a file are `naming`,
`call-obligation`, `no-passthrough`, `import-boundary` and `import-cycle` for
JS/TS, and `frontmatter` for markdown. See decision 19 for what a new language
costs.

**One of them reads more than the file in front of it.** `import-cycle`, and
`import-boundary` when it sets `forbid_reaching`, ask about the whole
repository's import graph — which is a real cost, stated in
[what a graph rule costs](#what-a-graph-rule-costs), and paid only by a
configuration that asks for it.

**Every one of them can name the decision it implements.** `decision` takes the
id of an entry in the config's top-level `decisions` block, and it is what turns
a denial from *"breaks `domain-forbids-http`"* into *"breaks ADR-014, and here is
why, and here is where it is written"*. It changes nothing about what the rule
checks. See [`CONFIG.md`](CONFIG.md#decisions--what-the-rules-are-for).

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
| `naming` | `dir_pattern` | the selected directory itself, by its own basename |
| `presence` | `require`, `require_any` | the direct child files, by name and by shape |
| `frontmatter` | `file_pattern` | the direct child documents, by basename; then the `---` block inside |
| `pair` | `file_pattern` | the direct child files, by basename; the companion may sit outside |
| `spec-pair` | `subfolders` | the listed subdirectories **and everything below them** (`"."` = the directory itself, its own files only), then files in them |
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

## Narrowing by what a file imports

`roots` selects by **where a file sits**. `when_importing` selects by **what it
talks to**, and the two are `AND`: a rule with both applies where its scope
reaches *and* the import matches.

```json
{
  "type": "call-obligation",
  "id": "writes-go-through-the-request-helper",
  "roots": ["services/api/Entities/*"],
  "when_importing": "services/api/Http/connection.ts",
  "must_call": { "symbol": "HttpRequest", "imported_from": "../../Http/request" }
}
```

Some obligations are about neither where a file is nor what it is called. In the
case this was built for, reads and writes are deliberate siblings — the
filenames say what the action *does*, not how it travels, because erasing the
transport from the contract was the point. Renaming the files would make the
rule expressible and would put back the fact the design spent a refactor
removing.

`when_importing_packages` is the sibling for package specifiers, matched the way
a boundary matches them, so `zod` covers `zod/v4`.

### What it costs, and when

**Nothing, unless a rule asks.** A rule that names no imports resolves nothing
and behaves exactly as it did. A rule that names them turns resolution on for
the files its scope reaches — and no further, so a narrowed rule over one
module does not make the rest of the repository pay.

| kind of rule | before | with `when_importing` |
|---|---|---|
| `naming`, `spec-pair`, `call-obligation`, `pair`, `frontmatter`, `no-passthrough` | parses each file | parses, and resolves its imports |
| `presence`, `structure` | reads a directory listing, opens nothing | parses and resolves **every file under its roots** |

That last row is the expensive one, and it is the price of the only reading
that means anything for a rule about a directory.

### For a directory rule it means "something in here"

`presence` and `structure` report about a **directory**, so *"this file imports
X"* has no reading there. A directory is in the population when **some file
inside it** matches:

```json
{ "type": "presence", "roots": ["src/*"],
  "when_importing": "src/db/**", "require": ["contract.md"] }
```

> Every directory with something that talks to the database carries a written
> contract.

`src/orders`, holding one file that imports `src/db/pool.ts`, is in. `src/reports`,
holding none, is out — and a rule that reported it would be reporting a
directory it was never about.

### An unresolved import is the sharp edge

A specifier nobody could place — a misconfigured alias, a dependency that is not
installed — leaves the narrowing undecided, and the file falls out of the
population. **The rule then does not apply at all**, which is a larger silence
than the one a boundary rule leaves: that one checked the imports it could
place.

Nothing new reports it, because something already does.
`summary.imports.unresolved_imports` names every specifier nobody placed, with
the file it is in:

```
note: `@Http/connection` did not resolve, so boundary rules did not see it
```

A run with entries there is a run whose narrowing may have been decided on
incomplete information. `archwarden config doctor` is where to start.

`import-boundary` has no `when_importing` and will not: it already chooses its
importers with `from`, `from_module` and `from_kind`.

See decision 25.

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

**An absent list and an empty one are different rules.** Omitting
`allowed_subfolders` says nothing about subfolders — the rule may still
constrain filenames, and every folder is permitted. Writing `[]` is a list of
what may exist holding nothing, so **no subfolder may exist**, which is how a
directory says it is a leaf:

```json
{
  "type": "structure",
  "id": "referencia-is-flat",
  "level": "error",
  "roots": ["referencia"],
  "allowed_subfolders": []
}
```

The two used to arrive identical and both did nothing, so the leaf rule was
unsayable and the config that tried to say it passed three commands in a row —
valid at `config validate`, silent at `config doctor`, skipped at `check`.
Issue #40.

A rule that names none of `allowed_subfolders`, `warn_subfolders`,
`subfolder_patterns` or `filename_patterns` constrains nothing at all, and
`config doctor` reports it as `rule-constrains-nothing`.

- **Subfolder patterns**. `filename_patterns` one entry over, for the other
  kind of directory entry: every direct child *directory* must match at least
  one regex. Enumeration works for a fixed vocabulary (`types`, `calcs`,
  `actions`) and cannot work for an open set where the shape is the rule —
  sixteen lesson folders named `NN-slug` with more arriving, and nobody
  listing them forever.

  ```json
  {
    "type": "structure",
    "id": "licao-nome-da-pasta",
    "level": "error",
    "roots": ["projetos"],
    "subfolder_patterns": ["^\\d{2}-[a-z0-9-]+$"]
  }
  ```

  It is a **union** with the two lists, the way `filename_patterns` is a union
  of its own regexes: a name passes if a list names it *or* a pattern matches
  it. So `allowed_subfolders: ["_template"]` beside the pattern above permits
  `_template` and every `NN-slug`.

  The lists are consulted **first**. A `warn_subfolders` entry whose name
  happens to have the right shape still warns — severity precedence above says
  the most specific declaration wins, and a name written out is more specific
  than a regex. Reading the patterns first would silence the one list that
  exists to be heard.

  **The constraint is reported to the folder as well as to its parent.**
  `describe projetos` says what may live inside; `describe projetos/03-servo`
  says what *that folder* may be called. They are one contract read from two
  sides, and until 0.12.0 only the parent was told — so `describe` answered
  "no rule applies" about a name `check` refuses, and `scaffold` returned a
  shape to build at a path that could never pass.

  A path with no extension is taken to be a folder, which is the evidence
  available about a path that does not exist yet. An extensionless *file* —
  `Makefile`, `LICENSE` — therefore hears one sentence that does not govern it.
  `check` is unaffected: it walked the tree and knows what is a directory.

  Purely lexical, like the rest of `structure`: no parse, no disk beyond the
  walk. `naming.dir_pattern` is the same matcher and reaches it only through
  `must_export`, which needs a TypeScript parse of a file inside — so a
  directory with no `.ts` near it could not use it at all. Issue #43.

**Recursion**. Some modules have nested modules of the same shape (e.g.,
"variants" of an entity). The `recurse_into` field lists **containers whose
children** are modules of the same shape, recursively.

Which directory becomes governed is the whole of it, and it is one level
deeper than the field's name suggests. With `recurse_into: ["variants"]`,
`user/variants/nfe` is governed and `user/variants` is not — the container
holds modules, it is not one. So `nfe` may be called anything, exactly as a
module selected by `roots` may: naming a container here promotes its children
from "unexpected subfolder" to "module", and a module's name is not this
rule's business.

That is a real decision, and it removes findings. Adding a namespace to
`recurse_into` cleared nineteen of them in one repository and looked like
modelling. `config explain <rule-id>` lists every directory a rule governs,
and is the way to check that the promotion is the one you meant.

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

**Wherever the directory sits.** A `_`-prefixed directory is exempt as a
subfolder of a governed directory *and* as a root a scope selects — the second
case being the one this hatch describes best, since a directory that is "not
itself part of the module structure" is usually a sibling of the modules
rather than a child of one.

Only the directory's own name is asked about, never an ancestor's, and that is
what makes a namespace expressible: `_Legacy` is exempt, so its nineteen
entities are not subfolders to complain about, while a rule with
`roots: ["packages/domain/_Legacy/*"]` governs each of them normally. To
silence a whole subtree instead, that is `skip_dirs.globs` with a `/**` — a
different request, and one worth making on purpose.

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
- `dir_pattern` — optional regex over the *name of the directory the file sits
  in*, contributing its own capture groups. See below.
- `must_export` — describes the required export:
  - `kind` — one tag or a list of tags (see table below).
  - `name` — templated from the capture groups.
  - `annotation` — optional. The type the export must be **annotated with**,
    templated from the same groups, as one value or a list meaning "any of".
    **Verified.** See below.
  - `signature_hint` — optional free-form string. **Never verified.** It exists
    so `scaffold` can show a realistic skeleton
    (`export function Foo(deps: FooDeps): UseCase<FooInput, FooOutput>`)
    rather than just a name. Constraining the actual type is type checking;
    use `tsc`.

**Case transformers available in templates**: `pascal`, `camel`, `kebab`,
`snake`, `upper`, `lower`, `raw`.

### When the directory is part of the name

Some conventions spell the export from both halves of the path. A per-entity
repository layer is the common shape: the entity names the folder, the action
names the file, and the export is the two joined.

```
Infrastructure/Repositories/Entities/
├── Order/
│   ├── fetch-by-id.ts   → export function OrderFetchByIdRepository
│   └── insert.ts        → export function OrderInsertRepository
└── Wallet/
    └── fetch-by-id.ts   → export function WalletFetchByIdRepository
```

`fetch-by-id.ts` may exist forty times over, and the entity prefix is what a
grep for the implementation lands on and what a stack trace names. `dir_pattern`
captures it:

```json
{
  "type": "naming",
  "id": "repository-action-export-name",
  "level": "error",
  "roots": ["src/Infrastructure/Repositories/Entities/*"],
  "dir_pattern": "^(?<entity>[A-Za-z0-9]+)$",
  "file_pattern": "^(?<action>[a-z0-9-]+)\\.ts$",
  "must_export": {
    "kind": "function",
    "name": "{{pascal(entity)}}{{pascal(action)}}Repository"
  }
}
```

Four things about it, each load-bearing:

- **It matches the last segment, not the path.** The scope glob has already
  chosen which directories are in play; what is offered to `dir_pattern` is
  `Order`, not `src/Infrastructure/Repositories/Entities/Order`. A pattern
  anchored with `^` and `$` — which is how anyone writes one — could not match
  the full path at all, so `config doctor` reports `dir-pattern-matches-nothing`
  rather than letting the rule quietly apply to nothing.
- **When set, it must match.** A file whose directory does not match is a file
  the rule is not about, exactly as with `file_pattern`. It is not a violation.
  A file with no directory at all — one at the repository root — is likewise
  outside a rule that asks about one.
- **One template namespace.** `{{pascal(entity)}}` and `{{pascal(action)}}` do
  not say which pattern each came from and do not have to. A group defined by
  *both* patterns is refused when the config compiles: it would have two values
  and no rule for choosing, and silently preferring one would make the rule
  demand the wrong export on every file in the scope.
- **Still purely lexical.** `dirname` and `basename` of a path archwarden
  already has. No parse, no resolution, no disk — so `describe` and `scaffold`
  keep answering for files that do not exist yet, and `check` and `describe`
  still share one matcher.

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

### When the export must write its type down

A registry built by discovery has no compile-time gate. Every
`tools/*.tool.ts` exports one symbol under a fixed name, a loader does
`readdir` plus `import()`, and nothing imports those files statically any
more. The name and the declaration form are already expressible; the shape is
not, and this is what it costs:

```ts
export const AGENT_TOOL = { spec: { name: "lookup_cep" } };   // ✓ archwarden ✓ tsc ✗ boot
```

`kind` is satisfied, the name is exact, `check` is green — and the worker dies
when the loader finds no `build`. The static registry that this replaced typed
its array, so the compiler rejected a malformed module. `readdir` plus
`import()` **removes that guarantee**, and the annotation on the export is the
only thing that restores it.

```json
{
  "type": "naming",
  "id": "agent-tools-export-contract",
  "level": "error",
  "roots": ["apps/worker/src/agent-tools/tools"],
  "file_pattern": "^(?<tool>[a-z0-9-]+)\\.tool\\.ts$",
  "must_export": {
    "kind": ["const"],
    "name": "AGENT_TOOL",
    "annotation": "AgentToolModule"
  }
}
```

`export const AGENT_TOOL: AgentToolModule = {...}` passes;
`export const AGENT_TOOL = {...}` does not.

Four things about it:

- **This is not type checking.** Nothing is resolved and nothing is inferred.
  The annotation is a token in the same declaration whose `kind` the rule
  already reads, and comparing it is the same class of work as comparing the
  name. A file annotating `AgentToolModule` over an object that is not one is
  `tsc`'s problem and stays that way. What the rule buys is that the
  declaration is *submitted to* `tsc`'s judgement at all — today a missing
  annotation means there is nothing for `tsc` to check against.
- **Where an annotation can live.** A binding writes it after the colon; a
  class writes it in `implements`, and one that implements several contracts
  satisfies a rule asking for any of them. A function declares a *return*
  type, which is a different claim — so `annotation` beside
  `kind: ["function"]` is a rule no file could satisfy, and the config is
  refused rather than left to flag every file forever.
- **Whitespace is not significant**, on either side. A type a formatter broke
  over three lines is the same type. Beyond that the comparison is exact:
  `AgentToolModule` does not match `AgentToolModule<In, Out>`, and a rule that
  accepts both says so with a list.
- **`scaffold` gets better, not just stricter.** The declaration it hands over
  becomes `export const AGENT_TOOL: AgentToolModule = /* ... */;` — a line that
  passes the rule, which is a promise `signature_hint` could never make.

**Cannot express**: whether the annotated value really is of that type, and
anything about the type itself ("a function returning `UseCase<X>`"). That is
type checking. Use `tsc` for that.

---

## 3. Presence

**What it enforces**: named files must exist in each governed directory.

**Scope**: directory-local. No parse, no resolve — a name against the walk.

**Shape**:

- `require` — filenames that must be there.
- `require_any` — regexes at least one file must match, **one file per entry**.

```json
{
  "type": "presence",
  "id": "licao-completa",
  "level": "error",
  "roots": ["projetos/*"],
  "require": ["projeto.md", "exercicios.md", "notas.md", "diagram.json"],
  "require_any": ["\\.ino$"]
}
```

**This is not the inverse of `filename_patterns`.** That field is a whitelist
of what *may* exist and is satisfied by an empty directory, which is exactly
the state this rule is about. A unit of work is incomplete until its companion
files are there, and the companion is what a hurried pass leaves out — nothing
errors, nothing fails to build, and the gap is found by whoever needed the
file. `spec-pair` is the same argument for one specific pair.

**`require` takes filenames, not paths.** An entry with a `/` is refused when
the config compiles. The same requirement is already sayable, by the rule that
is about that directory:

```json
{ "type": "presence", "id": "sketch-existe", "level": "error",
  "roots": ["projetos/*/sketch"], "require_any": ["\\.ino$"] }
```

One rule answering for one directory is what lets `describe` and `scaffold`
answer for a directory that does not exist yet, which is where this rule is
worth most: `archwarden scaffold projetos/17-nova` prints the filenames, and a
lesson gets started rather than corrected.

**One finding per missing entry**, not one per directory. Each is a separate
file to create — the shape `spec-pair` already reports a missing sibling in —
so a brand-new empty directory earns four findings for four absent files.
`--summary` is the answer to volume.

**Cannot express**: "this file needs *that* file", where the second is named
relative to the first rather than to the directory. That is a pairing question
rather than a presence one; see issue #45.

**Cannot express**: "this file needs *that* file", where the second is named
relative to the first rather than to the directory. That is `pair`, next.

---

## 4. Pairing

**What it enforces**: a file of one kind must have a companion of another.

**Scope**: file-local. No parse; a path against the walk.

**Shape**:

- `file_pattern` — regex over the filename of the file that *needs* a companion.
- `must_exist` — the companion, as a path relative to that file's directory.

```json
{
  "type": "pair",
  "id": "licao-tem-notas",
  "level": "error",
  "roots": ["projetos/*"],
  "file_pattern": "^projeto\\.md$",
  "must_exist": "notas.md"
}
```

**The difference from `presence` is the anchor.** `presence` asks about a
*directory* — these files must be here, whatever else is. `pair` asks about a
*file* — because this one exists, that one must too. So an empty directory is a
`presence` finding and not a `pair` one, and that is the whole of it: a lesson
that exists must have notes; a folder nobody has started owes nothing.

**The companion may leave the directory.** `../projeto.md` is the case this
rule exists for alongside the flat one — a sketch needs the lesson one level up,
the sketch may be called anything, and no directory-scoped rule can say that.
A path that would climb above the repository root puts the file outside the
rule rather than producing a finding nobody could act on.

**Literal, never derived.** `<stem>.<marker>.<ext>` is `spec-pair`'s idea; it
is a good convention for tests and generalises to nothing. Two fixed names in
one directory is what the rest of the world has.

**One direction, always.** The file matching `file_pattern` needs the
companion, never the reverse. An orphan `notas.md` is a note taken before the
lesson was written, which is fine and is not a finding. Write the second rule
if you mean both.

**Why not widen `spec-pair`.** Its default ignores exclude anything that is not
a JS/TS source file, by construction and for a good reason — a PNG needs no
test — and widening a rule until its name stops describing it is how a rule
stops being consultable. Issue #45.

---

## 5. Frontmatter

**What it enforces**: a document's YAML frontmatter carries the keys something
depends on.

**Scope**: file-local. Reads the document, not the code.

**Shape**:

- `file_pattern` — regex over the filename of the documents this is about.
- `require` — keys the block must carry.
- `one_of` — the closed vocabulary a key's value must come from.
- `equals` — a key whose value must equal a template rendered from the path.

```json
{
  "type": "frontmatter",
  "id": "projeto-frontmatter",
  "level": "error",
  "roots": ["projetos/*"],
  "file_pattern": "^projeto\\.md$",
  "require": ["id", "nivel", "componentes"],
  "one_of": { "nivel": ["1", "2", "3"] },
  "equals": { "id": "{{raw(dirname)}}" }
}
```

**The frontmatter is often not documentation.** It is the machine-readable half
of the document — the part three scripts and an index page depend on — and
nothing type-checks a markdown file. A `projeto.md` with no `componentes` does
not fail to load; it reports as a lesson that needs no components, so "which
projects use the DHT11?" returns an answer that is confidently short.

**`one_of` is the one that earns the rule.** A missing key is at least an
absence. A value *outside* the vocabulary is confidently wrong: `status:
concluido` where the vocabulary is `feito` drops the document out of the
generated table with no row and no error. Same failure shape as
`must_export.annotation`, one file format over.

**Values compare as text.** `"1"` in the config matches `nivel: 1` in the
document, and a quoted value matches an unquoted one. That answers the question
without archwarden growing a type system nothing else here needs.

**`equals` is the `naming` question**, asked of a file with no exported symbol
to ask it about: a name agreeing with a path. `{{raw(dirname)}}` is the name of
the directory the document sits in, and it is the only group a document
template may name. The form is `naming`'s, so the transforms come along —
`{{kebab(dirname)}}` is spelled the same way here as there.

**A document with no block is a finding, not a skip.** Skipping would make
*deleting the block* the way out of the rule, which is the argument
`skip_type_only` already makes about deleting the `export` keyword. A block
that is not YAML is a *different* finding, because "write the block" and "what
you wrote is not YAML" are different next steps.

**One dialect.** `---`-fenced YAML. TOML `+++` and JSON frontmatter are
guesses until somebody asks, and one dialect that is definitely right beats
three that are probably.

**Cannot express**: the shape of a value — `type`, `min_items`, a regex over a
value, a nested path. That is a document schema, JSON Schema is one, and the
line this rule keeps is the one every other rule here keeps: **archwarden
asserts names and vocabularies, never the shape of a value.** Nor referential
integrity between documents: "every `componentes[].id` exists in
`inventario.yml`" is a cross-file lookup into a file archwarden knows nothing
about, and `RULES.md` keeps cross-file questions out of file-local rules.

---

## 6. Spec pairing (TDD gate)

**What it enforces**: every unit file under configured subfolders must
have a `.spec.<ext>` sibling.

**Scope**: file-local. No parsing required unless `require_non_empty_spec`
is set.

**Shape**:

- `subfolders` — which folders under each selected directory are subject to
  the rule. Each entry covers that folder **and everything below it**, so
  `calcs` reaches `Entity/calcs/group/nested.ts`; grouping related files into
  a subfolder does not take them out of the gate. An entry is a path relative
  to the selected directory, so `calcs/group` names that subtree exactly.

  Use `["."]` to apply to the directory itself — that one is not recursive,
  because naming `calcs` is how a project says which subtree is under the
  gate, and a recursive `.` would swallow `types` and everything else it did
  not name.
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

### Where a spec may live

Beside the file, always. And in a directory the project names:

```json
{
  "type": "spec-pair",
  "id": "calcs-need-spec",
  "level": "error",
  "roots": ["src/*"],
  "subfolders": ["."],
  "spec_dirs": ["__tests__"]
}
```

`spec_dirs` is empty by default, which is sibling-only — what every config
written before this had, and what a project that says nothing keeps.

**One level, and no further.** A spec at `src/user/__tests__/create.spec.ts`
satisfies `src/user/create.ts`. One at `src/user/__tests__/unit/create.spec.ts`
does not, unless `unit` is named too. An entry with a path separator is refused
when the config compiles, with the reason.

That limit is the feature rather than a shortcut. A reading that accepted a
spec anywhere below would let a project satisfy the rule by putting one file
somewhere in the subtree — and the rule would report nothing and look exactly
like a repository that is fully tested, which is the failure `CONFIG.md` calls
the worst a linter has.

The name is the project's: `__tests__`, `tests`, `__specs__`, anything. The
`ROADMAP.md` this replaced leaned sibling-only *"because co-located test dirs
invite scattered test files"* — right about the risk, wrong about the remedy.
The risk comes from accepting *any* directory, not from the convention.

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

## 7. Import boundaries

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

**Three directions, not two.** `only_import_from` is an allowlist:

```json
{ "type": "import-boundary", "id": "api-depends-only-on-libs", "level": "error",
  "from_module": "api-orders", "only_import_from": ["packages/**"] }
```

Everything not named is refused, **including things that do not exist yet**.
That is the difference that matters: a denylist permits every new package, app
and directory by omission, and omission is invisible.

Three things sit outside an allowlist, and each is a decision rather than an
oversight:

| | |
|---|---|
| the rule's own scope | a file importing its neighbour is not what "only these" refuses |
| anything that did not resolve here | a builtin or a dependency has no repo path a glob could match |
| packages | `only_import_from_packages` is their axis, as `forbid_import_from_packages` is |

`only_import_from_modules` names modules instead of globs, the way
`forbid_module` does. And `only_import_from` is refused alongside
`forbid_import_from` or `except` on the same rule: "only these, except those"
is two rules, and two rules is what a reader can follow.

**Quantifying over a kind.** Give each module a `kind` and one rule covers
every module wearing it:

```json
"modules": [
  { "id": "api-orders",  "kind": "app", "scope": "apps/api-orders/**" },
  { "id": "api-billing", "kind": "app", "scope": "apps/api-billing/**" },
  { "id": "orders-core", "kind": "lib", "scope": "packages/orders/**" }
],
"rules": [
  { "type": "import-boundary", "id": "assemblies-are-islands", "level": "error",
    "from_kind": "app", "only_import_from_kinds": ["lib"] }
]
```

Six assemblies would otherwise be six rules of five entries each, and the
seventh means editing all six. Here the seventh is governed because it exists
with `kind: "app"`.

Written as an allowlist rather than `forbid_kind`, so a kind invented later is
refused rather than permitted by omission. **A module never fails this against
itself**: an app importing its own files is fine, and importing a sibling app
is not. Identity decides that, never the label. A kind no module wears is
refused when the config compiles, and `config doctor` reports a module wearing
none while other modules do.

**Naming a module instead of describing it.** When a module declares a `scope`
(see [CONFIG.md](CONFIG.md#modules-with-a-scope)), `from_module` and
`forbid_module` take module ids in place of globs:

```json
{ "type": "import-boundary", "id": "domain-is-sealed", "level": "error",
  "from_module": "domain", "forbid_module": ["infrastructure"] }
```

They become that module's paths when the config compiles, so everything below
applies unchanged. Saying it both ways on one rule — `from` *and* `from_module`,
or `forbid_import_from` *and* `forbid_module` — is refused, as is naming a
module that does not exist or one that declared no `scope`. Every one of those
would otherwise be a rule quietly governing nothing.

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

### Forbidding a dependency nobody wrote down

`forbid_reaching` is the transitive half of `forbid_import_from`:

```json
{
  "type": "import-boundary",
  "id": "ui-must-not-reach-db",
  "level": "error",
  "from": "packages/ui/**",
  "forbid_reaching": ["packages/db/**"],
  "except": ["packages/db/types/**"]
}
```

> `packages/ui` does not import `packages/db`. It imports `packages/orders`,
> which does — so a schema change in the database reaches the button component,
> and nothing in either file says so.

The finding carries the **whole chain**, `packages/ui/button.tsx →
packages/orders/cart.ts → packages/db/client.ts`, because *"ui reaches db"* is
not actionable and the middle of the chain is where the edit goes.

- **A direct import is not reported here.** That is `forbid_import_from`'s
  finding. A rule that set both would otherwise report one fault twice. Set
  both when you want both; they are different sentences.
- **`forbid_reaching_modules`** names a declared module instead of repeating its
  globs, the way `forbid_module` does for the direct form. Saying it both ways
  on one rule is refused when the config compiles.
- **`except` applies to both.** It names destinations the rule tolerates, and
  that means the same thing at the end of a chain as at the end of an edge.
- **Chains longer than twelve files are not followed**, for the reason
  `import-cycle` gives below.

**This field is the expensive one.** A rule that sets it makes the run parse and
resolve **every source file in the repository**, whatever `from` says — because
a chain that leaves the scope and comes back is still a chain, and a graph built
only from what the scope reaches would report a clean repository over a real
violation. See [what a graph rule costs](#what-a-graph-rule-costs). A boundary
rule that leaves `forbid_reaching` empty pays none of it.

### Forbidding a dependency

`forbid_import_from_packages` names packages rather than paths:

```json
{
  "type": "import-boundary",
  "id": "three-is-quarantined",
  "level": "error",
  "from": "src/**",
  "forbid_import_from_packages": ["three"],
  "except_from": ["src/scripts/three/**"]
}
```

> Only `src/scripts/three/**` may import `three`. Everywhere else the 3D module
> is reached through a dynamic `import()` behind an `IntersectionObserver`, so
> the cost never lands in the initial bundle.

That is an architecture rule in the same sense as the others — which layer may
reach which thing — and violating it is silent: nothing breaks, no test fails,
the page just gets slower and it is found weeks later in a Lighthouse report.

A separate field rather than a glob, because a dependency has no repo-relative
path. Matching `node_modules/three/**` would be a lie under pnpm's store layout
and does not exist at all under yarn PnP, so the rule names the **package**.

- **The package, and anything under it.** A rule naming `three` catches
  `three/examples/jsm/loaders/GLTFLoader.js`, which is the import that actually
  costs the bytes. A package that merely shares a prefix — `three-mesh-bvh` — is
  a different package and is left alone.
- **Builtins are one identity.** `node:fs` and `fs` are the same module; either
  spelling in the config matches either spelling in the source. "Nobody in
  `src/lib/**` imports `node:fs`" is expressible.
- **`except_from` is on the importing side.** `except` is about what is
  imported and cannot say "this directory may". They are separate fields because
  they exempt different ends of the same edge, and `except_from` exempts the
  importer from the whole rule.
- **An import that lands in this repository is a path, not a package.** A
  `tsconfig` alias spelling a local shim `three` is matched by
  `forbid_import_from`, never here. The two fields never both fire on one
  import.
- **It works before `install`.** An unresolved specifier is exactly what this
  reads, so a CI job that lints before installing dependencies still enforces
  the rule — unlike the path half, which needs resolution.

A separate field rather than a scheme prefix (`"pkg:three"`) inside
`forbid_import_from`, for the reason issue #14 gives: treating `three` as
*either* a path glob or a package name depending on what it happened to match is
the ambiguity that produces a rule enforcing nothing.

**Still cannot express**: transitivity *for packages*. `src/lib` importing
`src/scripts/three`, which imports `three`, is not flagged. `forbid_reaching`
covers the path half of the question and not this one: the graph is built from
imports that resolved **into this repository**, and a package has no node in it.

---

## 8. Import cycles

**What it enforces**: no file in scope sits on an import loop.

**Scope**: the whole repository's import graph.

```json
{
  "type": "import-cycle",
  "id": "no-cycles",
  "level": "error",
  "roots": "packages/**"
}
```

The first rule whose question cannot be answered from one file. Everything else
here reads the file in front of it — its name, its exports, its own imports —
and this one asks about the shape of the repository.

The finding carries the loop with **both ends**, `a.ts → b.ts → a.ts`, so a
reader can see that it closed.

- **The shortest loop is the one reported.** The search is breadth-first, so a
  file sitting on a two-file loop and a nine-file loop is reported with the
  two-file one. The shortest is not always the one to fix, and it is always the
  one somebody can read.
- **Chains longer than twelve files are not followed.** A forty-file loop is
  technically correct and useless: nobody reads it and nobody acts on it.
- **A file importing itself is not a loop.** That is a typo, and reporting it
  as an architecture fault helps nobody.
- **`include_type_only`** defaults to `true`, spelled the way
  `import-boundary` spells it. A loop of `import type` is erased at runtime and
  cannot deadlock anything; it is still a loop the compiler walks.

### Every file of the loop is reported

Once each, each carrying the loop as seen from itself.

A loop has no owner. dependency-cruiser reports the *closing edge*, which
depends on which file its walk happened to start from — so the same cycle moves
between runs and between machines, and the file blamed for it is an artefact of
traversal order. N files have to change, or N people have to agree not to, and
the report says N.

`baseline` accepts findings per rule and per path, so an accepted cycle is
accepted at that same N — you see how many files you signed off on.

**There is deliberately no `ignored_circular_dependencies`.** A cycle is a
finding and `baseline` already accepts findings. Nx has such an option because
it has no baseline. A second mechanism for accepting the same thing disagrees
with the first the day somebody uses both.

`config verify-rules` reports this kind as `unverified`: planting a violation
means writing two files that import each other and resolving both, which is the
resolver run inside a probe.

### What a graph rule costs

Both `import-cycle` and `import-boundary`'s `forbid_reaching` read the import
graph, and a graph is the whole repository's edges. A run carrying either one
**parses and resolves every source file**, whatever any rule's scope says.

Measured on a 10 000-file repository with 30 000 in-repo edges:

| configuration | wall clock | peak memory |
| --- | --- | --- |
| `import-boundary` over one module of forty | 0.01 s | 8 MB |
| `import-cycle` over the same one module | 0.22 s | 28 MB |
| `import-boundary` over everything | 0.12 s | 23 MB |
| the same, plus `import-cycle` | 0.23 s | 29 MB |

The run stops being proportional to the scope and becomes proportional to the
repository. Holding the edges is the small part — about 5 MB for 30 000 of them
— and the resolution pass is the rest.

This is the trade the rule exists to make: there is no cheaper way to answer
"is there a loop here?", and a graph built from less than the whole repository
answers it wrongly and quietly. A configuration with no graph rule pays nothing,
and neither does an `import-boundary` rule that leaves `forbid_reaching` empty.

`check --file` and the pre-write hook **refuse** these rules rather than
evaluating them, under the `needs-repository` skip reason: they see one file,
and a cycle rule with no graph reports nothing — which is what a repository with
no cycles reports.

---

## 9. Call obligations

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

## 10. No passthrough

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
