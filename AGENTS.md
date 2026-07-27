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
make — one per rule that wanted a file whose facts were unavailable, usually
because the file would not parse. It appears in the text line only when it is
not zero:

```
1 error, 0 warnings, 2 skipped · 3 files, 3 directories · 1 parsed · 2ms
```

A run with skips is a run that decided less than it looks like. Do not report
it as clean.

`files_parsed` and `facts_reused` are the cache working. A warm run parses
nothing and is not much faster in wall clock — it still reads and hashes every
file, because that is the only honest way to know one did not change. The cache
saves the parse, not the read.

`--no-cache` re-parses everything. Use it only if you suspect a stale result;
a run that disagrees with `--no-cache` is a bug worth reporting.

#### Filtering what the report shows

Four flags narrow the output. **None of them narrows what is checked.** Every
rule runs, every finding is computed, and the exit code is identical with them
and without — so a filter is safe to leave in a command that gates a build.

| flag | shows |
|---|---|
| `--summary` | per-rule counts instead of every finding |
| `--rules <id>[,<id>]` | only these rules |
| `--paths <path>[,<path>]` | only findings under these paths |
| `--level error\|warning` | only this level |

All four compose with AND, and both list flags are repeatable as well as
comma-separated.

`--paths` takes either. **A plain path selects that path and everything under
it** — paste the one from a finding and it works. A pattern containing `*`,
`?`, `[` or `{` is used exactly as written, so `'src/*'` stays one level.

```bash
npx archwarden check --summary                       # what rule is dominating?
npx archwarden check --level error                   # warnings are known debt
npx archwarden check --paths 'packages/domain/**'    # I just touched domain
npx archwarden check --summary --rules usecase-name  # one rule, counted
```

`--summary` in text:

```
domain-entity-shape  3 errors
types-need-spec      3 errors
app-shape            1 error
calcs-need-spec      3 warnings

7 errors, 3 warnings · 8 files, 20 directories · 1ms
```

Worst first: errors descending, then warnings, then by rule id. **A rule with
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

`doctor` exits **0 even with findings** — they are advice about a
configuration, not findings about code.

## The five rule kinds

| kind | asks | you satisfy it by |
|---|---|---|
| `structure` | may this folder or filename exist here? | putting the file where `allowed_subfolders` / `filename_patterns` allow |
| `naming` | does the filename match the exported symbol? | exporting the exact name `scaffold` gives you |
| `spec-pair` | is there a test beside it? | creating the sibling `.spec.ts` — **write it, do not leave it empty** if `non_empty_spec` is true |
| `import-boundary` | may this layer import that one? | importing through whatever `except` allows, or not at all |
| `call-obligation` | does this file call the required symbol? | calling it **anywhere in the file**, including from a local helper |

Two details that decide most cases:

- A scope like `packages/domain/src/*` selects the **directories one level
  below** `src`, and rules then apply to files inside them. `packages/domain/**`
  selects everything underneath.
- `spec-pair` takes the extension from the source file: `Component.tsx` pairs
  with `Component.spec.tsx`, not `.spec.ts`.

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
          { "type": "command", "command": "npx archwarden hook claude-code" }
        ]
      }
    ]
  }
}
```

If a write is denied, the message names the rules and points at
`scaffold <path>`. Ask for the shape rather than trying variations.

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
