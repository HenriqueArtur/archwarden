# archwarden, for coding agents

archwarden is an architecture linter. It answers **where a file may go, what it
must export, what it must be paired with, what it may import, and what it must
call** — all from one `arch.config.json` at the repository root.

You are the audience for this file. Ask before you write, instead of writing
and being corrected.

## The loop

```
describe <path>   →  write the file  →  check --file <path>
```

1. **`describe`** — before creating or moving a file, ask what applies to that
   path. It does not need to exist yet.
2. **`scaffold`** — when you want the answer as a shape rather than as a list
   of rules: what to export, what siblings to create, what not to import.
3. **`check --file`** — after writing, verify that one file without walking the
   repository.

Never guess a convention. Two of these commands cost milliseconds and answer
exactly.

## Running it

archwarden is a dev dependency, so it lives in `node_modules/.bin` rather than
on `PATH`:

```bash
npx archwarden describe src/foo.ts        # npm
pnpm exec archwarden describe src/foo.ts  # pnpm
bunx archwarden describe src/foo.ts       # bun
```

Inside a `package.json` script, plain `archwarden` works — `node_modules/.bin`
is on the PATH there. Everywhere else, use one of the above.

If there is no `arch.config.json` anywhere up the tree, every command exits
**2**. That means the project does not use archwarden; do not create a config
unless you were asked to.

## Exit codes

| code | meaning | what to do |
|---|---|---|
| `0` | clean | proceed |
| `1` | rules were broken | fix the file; the finding says how |
| `2` | archwarden could not run | config missing, unreadable or invalid — report it, do not work around it |

Exit 1 is the answer to a question. Exit 2 is a broken tool. Never treat them
alike, and never suppress either.

## Commands

Every command takes `--format json`, which is versioned and stable. Use it.

Under `--format json`, **stdout is the document and nothing else**. Notes about
side artefacts go to stderr, so `JSON.parse` of stdout is always safe. In the
text format they stay where a reader expects them, beside the report.

### `describe <path>` — what applies here

```bash
npx archwarden describe packages/domain/src/order/calcs/discount.ts --format json
```

```json
{
  "version": 0,
  "path": "packages/domain/src/order/calcs/discount.ts",
  "rules": [
    {
      "id": "calcs-need-spec",
      "kind": "spec-pair",
      "level": "error",
      "module": "domain",
      "expectations": [
        {
          "type": "required-sibling",
          "path": "packages/domain/src/order/calcs/discount.spec.ts",
          "non_empty_spec": false
        }
      ]
    },
    {
      "id": "domain-forbids-app",
      "kind": "import-boundary",
      "level": "error",
      "expectations": [
        {
          "type": "forbidden-import",
          "patterns": ["packages/app/**"],
          "except": [],
          "include_type_only": true
        }
      ]
    }
  ]
}
```

An empty `rules` array means nothing constrains that path. That is an answer,
not a failure.

**Paths are read either way.** From inside `packages/domain`, both
`src/order/x.ts` and `packages/domain/src/order/x.ts` reach the same file — the
first is what an editor or `git diff` hands you, the second is what every
archwarden report prints. Absolute paths work too. When both readings name
something real, the one relative to where you are standing wins. Check the
`path` field in the reply: it is what was actually resolved.

### `scaffold <path>` — the shape it should have

```bash
npx archwarden scaffold packages/app/src/use-cases/refund-order/refund-order.use-case.ts --format json
```

```json
{
  "version": 0,
  "path": "packages/app/src/use-cases/refund-order/refund-order.use-case.ts",
  "required_exports": [
    {
      "name": "RefundOrder",
      "kinds": ["function"],
      "signature_hint": "(deps: RefundOrderDeps): Promise<void>"
    }
  ],
  "required_siblings": [],
  "forbidden_imports": [],
  "required_imports": [],
  "call_obligations": [],
  "filename_patterns": [],
  "allowed_subfolders": null
}
```

`required_exports[].name` is **exact**: the filename dictates it. Write
`export function RefundOrder`, not `RefundOrderUseCase`.

`signature_hint` is a suggestion — archwarden never verifies it. Follow it
anyway; it is the project's own house style.

`annotation` is not a suggestion. When it is present, the export must carry
that type as written — `export const AGENT_TOOL: AgentToolModule = {...}`,
never `export const AGENT_TOOL = {...}` — and `check` fails on a file that
leaves it off. Several entries mean any one of them will do. Write the
declaration line `scaffold` gives you and it passes.

Pass a **directory** to ask what may exist inside it. `allowed_subfolders` is
`null` when nothing constrains the directory, and otherwise names both lists:

```json
"allowed_subfolders": { "allowed": ["use-cases"], "warn": [] }
```

A name in `warn` is permitted but discouraged — prefer one from `allowed`.

### `check --file <path>` — verify one file

```bash
npx archwarden check --file packages/domain/src/order/calcs/total.ts --format json
```

```json
{
  "version": 0,
  "path": "packages/domain/src/order/calcs/total.ts",
  "findings": [
    {
      "rule_id": "calcs-need-spec",
      "module_id": "domain",
      "level": "error",
      "path": "packages/domain/src/order/calcs/total.ts",
      "span": null,
      "observed": {
        "type": "sibling-missing",
        "path": "packages/domain/src/order/calcs/total.spec.ts"
      },
      "expected": {
        "type": "required-sibling",
        "path": "packages/domain/src/order/calcs/total.spec.ts",
        "non_empty_spec": false
      }
    }
  ],
  "skipped": []
}
```

**On a `.rs` file the same rule points at the file itself**, because Rust's
unit tests live inside it:

```json
{
  "rule_id": "rust/every-unit-carries-its-tests",
  "level": "error",
  "path": "crates/thing/src/untested.rs",
  "observed": { "type": "spec-is-empty", "path": "crates/thing/src/untested.rs" },
  "expected": {
    "type": "required-sibling",
    "path": "crates/thing/src/untested.rs",
    "non_empty_spec": true
  }
}
```

`expected.path` equal to `path` is the signal: the file to edit is the one
reported, and the fix is a `#[cfg(test)] mod tests` with at least one `#[test]`
in it. Creating `untested.spec.rs` satisfies nothing.

**Read `skipped`.** A rule archwarden could not evaluate from one file appears
there rather than being dropped in silence. A non-empty `skipped` means the
answer is partial; run the full `check` before claiming the change is clean.

### `check` — the whole repository

The gate. Run it before saying you are done.

```json
{
  "version": 0,
  "summary": {
    "errors": 1,
    "warnings": 0,
    "files_scanned": 4,
    "directories_scanned": 11,
    "files_parsed": 0,
    "facts_reused": 3,
    "checks_skipped": 0,
    "duration_ms": 1
  },
  "findings": [ ... ]
}
```

The text format ends with the same numbers on one line, how long it took last:

```
1 error, 0 warnings · 4 files, 11 directories · 0 parsed, 3 reused · 1ms
```

**`checks_skipped` is the number to watch.** It counts checks nobody could
make — one per rule that wanted a source file whose facts were unavailable,
which in practice means the file would not parse. It appears in the text line
only when it is not zero, and the run names every one of them:

```
note: `src/user/broken.ts` was not checked — unexpected token
      2 checks skipped there: calcs-need-spec, domain-forbids-infrastructure
1 error, 0 warnings, 2 skipped · 3 files, 3 directories · 1 parsed · 2ms
```

A run with skips is a run that decided less than it looks like. Do not report
it as clean.

`summary.skipped_checks` carries the same pairs, for a consumer rather than a
reader:

```json
"skipped_checks": [{ "rule_id": "calcs-need-spec", "path": "src/user/broken.ts" }]
```

Usually one file that will not parse, wanted by every rule that reads inside a
file. Fix the file, or find out why it does not parse.

**Zero is reachable, and that is the point.** A rule whose scope also covers a
`DOC.md`, a `package.json` or an image does not skip a check on it — those are
not answers anybody lost, they are files the rule was never about, and counting
them would pin the number above zero for any repository that keeps
documentation beside its code. If that were so, this instruction would teach
you to ignore the one number it asks you to watch. `check --file` still reports
them, under `not-source`, because that command answers "what happened to *this*
file" and "nothing, it is not source" is a real answer there.

**An import that did not resolve is the other kind of blind spot.** A boundary
rule matches globs against where an import *lands*, so one that landed nowhere
was never checked. The run says how many, and names them:

```
note: 2 imports could not resolve, so boundary rules did not see them
      `packages/domain/row.ts`: `@Domain/Order/id`, `@Domain/Order/types`
0 errors, 0 warnings · 4153 files, 1268 directories · 820ms
```

`summary.imports.unresolved_imports` carries every pair for a consumer; the
text names the first ten files and says how many it left out.

Usually one of two things. A dependency that is not installed — run `install`
and the note goes away. Or an alias no `tsconfig` *governing that file*
declares.

`compilerOptions.paths` **is** read, per importer and by TypeScript's own rule:
the nearest `tsconfig.json` to the file wins, whole. So an alias declared in
another package's `tsconfig` does not apply here, and a bare `tsconfig.json`
in a directory takes the repository's aliases away from every file under it
unless it `extends` the one that declares them. That is the usual cause, and
the fix is in the `tsconfig`, not in archwarden. Neither case fails the build
on its own.

`files_parsed` and `facts_reused` are the cache working. A warm run parses
nothing and is not much faster in wall clock — it still reads and hashes every
file, because that is the only honest way to know one did not change. The cache
saves the parse, not the read.

`--no-cache` re-parses everything. Use it only if you suspect a stale result;
a run that disagrees with `--no-cache` is a bug worth reporting.

#### The baseline

A repository may carry `.archwarden/baseline.json` — a committed record of
findings the project has decided to accept, usually written when archwarden was
first adopted. `check` reports only what is *not* in it, and the summary says
how many are accepted:

```
0 errors, 0 warnings · 3778 files, 1034 directories · 593ms
78 accepted, 12 no longer occur — run `archwarden baseline` to update
```

In JSON the same two numbers are in the document, under `summary.baseline`,
and the key is absent when the repository has no baseline at all:

```json
"baseline": {
  "accepted": 78,
  "gone": 12,
  "by_decision": { "ADR-014": { "accepted": 68, "gone": 9 } }
}
```

`gone` is the one to read. Accepted entries that no longer occur are debt that
has actually been paid — and a stale entry is one that could hide a violation
that came back.

`by_decision` attributes both numbers to the decision the rule serves. A
decision whose rules report nothing today and whose entire debt is here is one
this repository has written down and never kept; `config explain <decision-id>`
says so in words. Debt from a rule that names no decision is in the totals and
in no entry here.

**A decision may say what it already rejected.** When a finding or a denial
carries one, read it — it is the option you were about to propose:

```
decision: ADR-031 — the domain does not know about transport
  `an HTTP client in the domain` was considered and rejected:
    a consumer would inherit our transport, and the retry policy with it
```

Do not propose it again. `config explain <decision-id>` lists every rejected
option, and says which ones a rule actually refuses and which are only written
down.

**A deadline is measured against the run's day, not the clock.** `check
--as-of YYYY-MM-DD` answers for any day, and `summary.as_of` says which one a
report answered for. If you need to know what is about to expire, ask about the
future rather than waiting for it to break.

**`.archwarden/decisions/*.md` is generated.** Everything outside the
`archwarden:yours` markers comes from `arch.config.json` and will be
overwritten — change the config, then run `archwarden decisions`. Inside the
markers is a person's prose; never rewrite it.

**Do not add to a decision's debt.** If your change would put a new entry under
one, `archwarden baseline --dry-run` names it:

```
+ domain-forbids-http packages/domain/src/order/repo.ts — imports `axios`
    against ADR-014 — the domain does not know infrastructure
```

**Do not add to it.** Writing `archwarden baseline` accepts every finding in the
repository, which silently forgives whatever you just broke. It is a decision
for a human, made once, reviewed in a pull request. If your change fails
`check`, fix the change.

`12 no longer occur` means someone fixed accepted debt — the entries are
removable, and regenerating is the only time that command is right to run.

**`archwarden baseline --dry-run` says what regenerating would change and
writes nothing.** Use it to answer the only question that matters about a
regenerated baseline — was debt paid, or was debt added:

```
  - domain-needs-spec  apps/api/src/order.ts — no longer occurs
  ~ domain-entity-shape apps/api/src/Domain/user → packages/domain/user
  + domain-forbids-outer apps/api/src/billing.ts — imports `@Infrastructure/Auth`

.archwarden/baseline.json would change: 1 added, 1 no longer occur, 1 moved.
```

Only `+` is a decision. A finding that merely changed path is reported as
moved, so a refactor that shifted a directory does not read as a hundred
acceptances — but two paths are only paired when the same directory move
explains at least two of them, so a fix and a new finding that happen to share
a folder name can never be laundered into one.

`--no-baseline` shows everything, including accepted findings. Use it to answer
"how much debt is there", never to decide whether your change is clean.

#### Filtering what the report shows

Five flags narrow the output. **None of them narrows what is checked.** Every
rule runs, every finding is computed, and the exit code is identical with them
and without — so a filter is safe to leave in a command that gates a build.

| flag | shows |
|---|---|
| `--summary` | per-rule counts instead of every finding |
| `--rules <id>[,<id>]` | only these rules |
| `--paths <path>[,<path>]` | only findings under these paths |
| `--level error\|warning` | only this level |
| `--changed [<ref>]` | only files that differ from `<ref>` (default `HEAD`) |
| `--by rule\|path` | what `--summary` counts by; implies `--summary` |

All four compose with AND, and both list flags are repeatable as well as
comma-separated.

`--paths` takes either form. **A plain path selects that path and everything
under it** — paste the one from a finding and it works. A pattern containing `*`,
`?`, `[` or `{` is used exactly as written, so `'src/*'` stays one level.

```bash
npx archwarden check --summary                       # what rule is dominating?
npx archwarden check --level error                   # warnings are known debt
npx archwarden check --paths 'packages/domain/**'    # I just touched domain
npx archwarden check --summary --rules usecase-name  # one rule, counted
npx archwarden check --changed                       # what I have not committed
npx archwarden check --changed main                  # what this branch does
```

`--changed` asks git. Untracked files count — a file you just created is the
one most worth checking — and gitignored ones do not. The directories a file
lives in count too, because a `structure` finding names the directory, not the
file that made it exist.

**It does not narrow the gate.** A run filtered to your own changes still fails
on somebody else's regression elsewhere; you just do not see it listed. That is
deliberate, and it means `--changed` cannot be used to make a build pass. If
the repository has accepted debt you want the build to ignore, that is a
different feature and archwarden does not have it yet.

`--summary` in text:

```
domain-entity-shape  3 errors
types-need-spec      3 errors
app-shape            1 error
calcs-need-spec      3 warnings

7 errors, 3 warnings · 8 files, 20 directories · 1ms
```

`--by path` counts the same findings by area instead:

```
packages/domain/src/invoice  2 errors, 1 warning
packages/domain/src/order    2 errors, 1 warning
packages/domain/src/client   1 error, 1 warning
```

The areas are the directories the rules' own scopes select, so a config with
`roots: packages/domain/src/*` gets one row per module. `--summary` says what
is dominating the output; `--by path` says which part of the repository is
furthest from the rules, which is the one that tells you where to start. Only
areas with findings get a row.

Worst first in both: errors descending, then warnings, then by name. **A rule with
no findings keeps its row** — that it was evaluated is an answer, and a missing
row would read as a rule someone disabled. `--rules` narrows the rows, because
it is the one filter that names rules; `--paths` and `--level` leave every row
in place with a zero.

In JSON, `--summary` adds `summary.by_rule` and **omits the `findings` array**.

```json
{ "summary": { "errors": 7, "by_rule": { "domain-entity-shape": { "errors": 3, "warnings": 0 } } } }
```

**Two things to read carefully when you filter.** The counts describe what you
asked to see, not what was checked — so `0 errors` with exit 1 is possible and
correct. `summary.hidden`, and a `note:` line in text, say how many findings
the filter removed. And an unknown rule id is **exit 2**, never an empty
report: a filter matching nothing would otherwise look exactly like a clean
repository.

### The HTML pages are for a human, not for you

```bash
npx archwarden agent-guide --format html > architecture.html   # the rules, as declared
npx archwarden check --html report.html                        # and where they stand
```

Read-only, self-contained, no script. **They are not a contract** — `--format
json` is. Never parse one, and never regenerate one to make a check pass; the
page shows what `check` decided and cannot change it.

Mention them when a human asks to *see* the architecture. For anything you have
to act on, use `describe`, `scaffold` or `--format json`.

### `agent-guide` — every rule, as context

```bash
npx archwarden agent-guide                          # markdown
npx archwarden agent-guide --scope packages/domain  # only what can fire there
npx archwarden agent-guide --kind import-boundary   # only that kind
```

`--kind` is repeatable and comma-separated, and composes with `--scope`:
`--scope packages/domain --kind import-boundary` answers "the import boundaries
that affect this directory" in one question. A kind no rule type has is exit 2,
not an empty digest.

Deterministic: same config, same bytes. Safe to commit or to regenerate.
Reach for it when you need the whole rule set at once — for one path,
`describe` is cheaper and more precise.

### `impact <path> --to <path>` — what a move would change

**A directory source takes a path or a step.** `--to apps/api/src/http` is a
path and replaces the source; `--to ../http` begins with a dot and is a step
from it. The leading dot is the whole distinction, and it is the same one for a
glob source — where a step is what the batch form is for.


Before moving a file, ask what it costs. Your editor rewrites the import
specifiers; this says whether the destination is somewhere the architecture
allows the file to be, and whether the move puts somebody else's import across
a boundary.

```bash
npx archwarden impact packages/domain/src/order/calcs/total.ts \
                --to  packages/app/src/billing/total.ts
```

```
Moving `packages/domain/src/order/calcs/total.ts` to `packages/app/src/billing/total.ts`:

  Rules that would stop applying:
    domain-forbids-app

  1 file imports it, 1 of which would newly cross a boundary:
    packages/domain/src/invoice/calcs/sum.ts — domain-forbids-app

  1 relative import in the file itself would need rewriting.

  1 file has a dynamic import this cannot read. Check it by hand:
    packages/app/src/loader.ts
```

**Read the last section every time.** `import(name)` names no module, so a file
containing one may or may not import the target and archwarden cannot tell. The
rest of the report is complete except for those files.

`newly_forbidden_by` means the boundary is crossed *because of this move* — a
boundary already being crossed is existing debt `check` reports today, not a
consequence of what you are about to do.

Relative imports are counted, not checked. Whether they still resolve after the
move is `tsc`'s question, and it answers it better.

It resolves the whole repository, so it costs about what `check` costs.

#### `--apply` — carry the move out

Dry run is the default. `--apply` is the second, explicit word:

```bash
npx archwarden impact packages/domain/src/id/shared/is-id-invalid-shared.ts \
               --to  packages/domain/src/id/calcs/is-id-invalid.ts --apply
```

`git mv`, then every import specifier that named the file — the ones written by
package name included, which is the half your editor cannot do. The spec
sibling comes along and follows a rename. A source directory left empty is
removed.

A directory or glob as the source makes `--to` relative to each match:

```bash
npx archwarden impact 'packages/domain/src/*/shared' --to '../calcs' --apply
```

**Three things to hold on to.**

**A refusal means nothing happened.** Everything is validated before a byte is
written, so there is no half-done state to clean up. Exit 2 with a reason: a
dirty working tree, a specifier archwarden cannot recompute, a destination that
exists. Fix the reason and run again; do not work around it.

**Install the workspace before moving anything in a monorepo.** A file that
imports a moving package by name — `@org/domain/thing` rather than `../thing` —
is invisible when that package does not resolve, which is the normal state of a
fresh clone before `install`. `--apply` refuses rather than rewriting the
importers it happens to see, and the refusal names the file and the specifier.
Run your package manager's install and try again.

**`--force` covers exactly one refusal** — a dynamic import naming no module,
where whether that file imports the target is unknowable. The report names the
file. Look at it. Do not reach for `--force` on anything else; it does not
apply to anything else.

**The exported symbol is not renamed.** A file renamed mid-move keeps its
export, and the output says so. Run `check` afterwards: a `naming` rule will
tell you whether the project wants them to match, and renaming an export is a
change to every caller — your decision, not the tool's.

**If the repository has a `no-passthrough` rule, run `check` for it before you
move anything.** `--apply` moves files; it does not delete indirection. A file
that only forwards another module survives the move and forwards the new
location instead. Deleting those first means fewer importers for `--apply` to
rewrite; doing it after means rewriting the same lines twice. Deleting one is
an edit to its importers, so it is a change to propose, not one to make on your
own initiative.

### `orphans` — does this folder have a reason to exist?

```bash
npx archwarden orphans                                   # every folder
npx archwarden orphans 'packages/domain/src/**/shared/**'
npx archwarden orphans packages/domain/src/order --by-file
```

Per folder, where its files' importers sit: inside the module the file lives
in, outside it, only specs, or nobody.

```
packages/domain/src/flow-node/shared/calcs  2 files  inside-only 0  outside-only 2  both 0  specs-only 0  nobody 0
                                            → only used from outside its module — the boundary is drawn elsewhere
```

- **only from outside** — nothing in its own module needs it; the boundary is
  drawn in the wrong place.
- **only from inside** — a private detail wearing a public name.
- **only specs** — test scaffolding. A mock is this, and it is working.
- **nobody** — dead, or reached through a dynamic import. Those files are
  listed at the end.

A folder that is a mix gets no verdict, and so does one whose claim the data
contradicts: "used only from outside" is withheld when a spec *inside* the
module imports it. A file's own spec never counts either way — it exists
because the file does.

**"Module" is the area the config already declares**, the same one
`check --by path` counts by. Not unused-export detection: Knip answers that,
and only the "nobody" column overlaps.

### `config explain <rule-id>` — what a rule reaches

```
calcs-need-spec (spec-pair) — error [domain]
  applies to: packages/domain/src/*

  Covers 1 path:
    packages/domain/src/order/calcs/total.ts

  Flags 1 path:
    packages/domain/src/order/calcs/total.ts — `…/total.spec.ts` does not exist
```

Use it when a rule's scope surprises you, before deciding it is wrong.

### `config validate` and `config doctor`

`validate` answers "does this config mean anything?" — schema only, no files
walked. `doctor` answers "does it mean what you think?": unreachable rules,
regexes matching nothing, scopes pointing at paths that do not exist.

`doctor` exits **2 when it reports anything at `error` level**, and `0` when it
reports only warnings. Two rather than one, and that distinction is why both
codes exist: everything `doctor` says is about the configuration, so it means
*fix your setup* rather than *fix your code*.

`--strict` fails on warnings too, which is what a gate that wants every concern
to block should use. Both directions are deliberate: a command that never
failed guarded nothing, and a warning that fails a build is a warning in name
only.

### `config verify-rules` — does a rule actually bite?

`explain` answers about **coverage**; this answers about **efficacy**. A rule
can be schema-valid, cover the right paths, appear in `explain` and still
enforce nothing, and that state is invisible from the outside: a rule enforcing
nothing looks exactly like a repository that satisfies it.

Each rule is handed a synthesised violation of its own terms and asked whether
it fires. Nothing is written to the repository.

```
✓ domain-is-self-contained — fires on `packages/domain/order.ts` importing `apps/api/env.ts`
✓ usecase-name — fires on `src/order/create.use-case.ts` exporting nothing
✗ cancelled-by-its-own-except — silent on `packages/domain/order.ts` importing `apps/api/env.ts`
? every-invoke-names-a-command — not verified: a violation is a `invoke` in one scope
  naming something no declaration in another answers to — two files that have to
  disagree, which this cannot build from one

3 enforce something, 1 enforce nothing, 1 not verified
```

It exits **non-zero on `✗`**, so it belongs in CI beside `check`.

**What it does not prove.** That a rule fires on a violation of *its own terms*.
It cannot know what you meant: a `forbid_import_from_packages` list missing an
entry is a question about intent, and a rule with that hole in it ticks here.
Rules whose violation cannot be synthesised are reported as `?` with the reason
rather than left out — an unchecked rule has to be visible as unchecked.

## The nine rule kinds

| kind | asks | you satisfy it by |
|---|---|---|
| `structure` | may this folder or filename exist here? | putting the file where `allowed_subfolders` / `filename_patterns` allow |
| `naming` | does the filename — and sometimes its directory — match the exported symbol? | exporting the exact name `scaffold` gives you |
| `presence` | do the files this folder owes exist? | creating each name `scaffold <directory>` lists |
| `frontmatter` | does this document's YAML block carry the keys something reads? | writing the keys `scaffold` names, with values from the vocabularies it lists |
| `pair` | does the file that goes with this one exist? | creating the companion `scaffold` names |
| `spec-pair` | is there a test for it? | creating the sibling `.spec.ts` — **write it, do not leave it empty** if `non_empty_spec` is true. On a `.rs` file the test goes **inside** the file |
| `import-boundary` | may this layer import that one — or that *dependency*? | importing through whatever `except` allows, or not at all |
| `call-obligation` | does this file call the required symbol? | calling it **anywhere in the file**, including from a local helper |
| `no-passthrough` | does this file add anything of its own? | writing something here, or deleting the file and importing what it forwards |

Two details that decide most cases:

- A scope like `packages/domain/src/*` selects the **directories one level
  below** `src`, and rules then apply to files inside them. `packages/domain/**`
  selects everything underneath.
- `spec-pair` takes the extension from the source file: `Component.tsx` pairs
  with `Component.spec.tsx`, not `.spec.ts`. On a `.rs` file it asks about the
  file itself — a `#[cfg(test)] mod tests` with at least one `#[test]` in it.
  There is no `create_client.spec.rs`; writing one satisfies nothing and the
  finding names the source file as the path to fix.
- A `naming` rule may spell the export from the **directory as well as the
  file** — `Order/fetch-by-id.ts` wanting `OrderFetchByIdRepository`. Never
  guess that name from the pattern. Ask `scaffold`, which renders it, and note
  that moving such a file to another directory changes the name it must export.
- An `import-boundary` rule may forbid a **dependency**, not only a layer:
  "only `src/scripts/three/**` may import `three`". Reaching for a package
  because it is installed is not enough — `describe` tells you whether this
  file may. The rule covers subpaths, so `three/examples/...` is the same
  package, and it fires even when dependencies are not installed.

## Reading a finding

Every finding has an `observed` and an `expected`, both tagged objects. The
pair is the whole diagnosis: `observed` is what the file does, `expected` is
what the rule wanted. You do not need to re-read the config to know what to
change.

`span` is a byte range into the file when the rule found something at a
position, and `null` when the finding is about the file's existence or
location. In the text output a finding with a span is printed as
`path:line:column`, which an editor and most terminals turn into a link. The
JSON keeps the byte range, which is what a tool wants.

`why` is the reason the rule exists, when the project wrote one down. It is
**not** part of the diagnosis — `observed` and `expected` are — and it is not
an argument to negotiate with. It is there so that a constraint which looks
arbitrary is not: read it before deciding the rule is wrong. In text output it
appears once per rule, under that rule's first finding.

`module_id` is `null` for a rule declared in the top-level `rules` array rather
than inside a module — import boundaries usually are. In text output that rule
prints as `[*]`.

`path` on a finding is the path the **rule** is about, which is not always the
file you asked about: a structure rule reports the offending *directory*.

`level` is `"error"` or `"warning"`. A warning does not fail the build; it
still means the project would rather you did it the other way.

## The pre-write hook

A repository may wire archwarden into a harness so invalid writes are rejected
before they land. `npx archwarden install-hooks --claude-code` writes it and
prints the command it installed. For Claude Code that is
`.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Write|Edit|MultiEdit",
        "hooks": [
          { "type": "command", "command": "./node_modules/.bin/archwarden hook claude-code" }
        ]
      }
    ]
  }
}
```

If a write is denied, the message names the rules and points at
`scaffold <path>`. Ask for the shape rather than trying variations.

If instead you get a `systemMessage` beginning **"archwarden did not check this
write"**, the write was allowed and *nothing examined it*: no config was found,
the config did not compile, the path fell outside the repository, or the event
was unreadable. That is not approval. Say so rather than treating the write as
cleared — the sentence names which of those happened, and every one of them is a
minute's fix.

## Rules for you

1. **Ask before writing.** `describe` or `scaffold` costs a millisecond; a
   rejected write costs a turn.
2. **Do not edit `arch.config.json` to make a check pass.** The config is the
   project's decision. If a rule seems wrong, say so and let a human decide.
3. **Do not suppress.** There is no ignore comment, on purpose. `ignore_files`
   in the config exists for real exceptions and is a human's call.
4. **A missing spec file means write the test**, not create an empty one. If
   `non_empty_spec` is true, an empty spec fails anyway — and if it is false,
   an empty spec passes the linter and defeats the rule.
5. **Exit 2 is not your problem to route around.** Report it.
