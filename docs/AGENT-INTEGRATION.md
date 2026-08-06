# Agent integration

archwarden is designed to be **queried by coding agents before they write
code**, not only consulted after the fact. This document describes the
four integration layers, when to use each, and the CLI surface that
supports them.

The design principle: archwarden is an informant first and a gate second.
Gates catch what the informant failed to prevent. Both matter, but the
informant is the loop that saves iterations.

## The four layers

### Layer 1 — Passive

The agent runs `archwarden check` after writing. On failure it reads
`archwarden explain <file>` and retries.

- **When it applies**: baseline. Always available. Configured through CI
  and pre-commit hooks.
- **Cost**: one wasted iteration per violation.
- **Limits**: the agent had no way to know the rule before writing.

### Layer 2 — Discoverable

The agent asks archwarden what applies to a target path *before* writing.

- `archwarden describe <path>` — lists every rule whose `roots` match this
  path, in both human prose and raw JSON.
- `archwarden scaffold <path>` — returns the minimal valid file shape at
  this path: expected export name, required sibling files, required
  imports (implied by `call-obligation`), forbidden imports (implied by
  `import-boundary`).

The agent's prompt or system message instructs it to call these before any
`Write` operation in the tracked repo.

- **When it applies**: agents with disciplined tool use. Very high signal,
  zero enforcement.
- **Cost**: two extra CLI calls per file created. Warm-cache latency of
  each is expected to be under 20 ms.
- **Limits**: depends on the agent actually calling them.

### Layer 3 — Proactive

archwarden emits a markdown digest of active rules that agent harnesses
already know how to read: `CLAUDE.md`, `AGENTS.md`, and equivalents.

- `archwarden agent-guide --format markdown > .archwarden/AGENT_RULES.md`
- The user references the file from their `CLAUDE.md`:
  ```
  Before creating any file under packages/ or apps/, read
  .archwarden/AGENT_RULES.md and follow the rules in it.
  ```
- A file-watch hook (installed by `install-hooks`) regenerates
  `AGENT_RULES.md` whenever `arch.config.json` changes.

- **When it applies**: any harness that respects CLAUDE.md/AGENTS.md.
  Complements Layer 2 — the guide teaches the rules, `describe` answers
  specific questions.
- **Cost**: file-watch overhead on config change; zero at agent-time.
- **Limits**: depends on the agent reading its context files. Long guides
  compete for context budget — `agent-guide` output is optimised to be
  compact and grep-friendly.

### Layer 4 — Bloqueante

The harness runs `archwarden check --file <path>` as a pre-write hook and
rejects the write on failure.

- `archwarden install-hooks --claude-code` writes a `PreToolUse` hook in
  `.claude/settings.json` that intercepts `Write` and `Edit` calls.
- `archwarden install-hooks --cursor`, `--zed`, etc. as those harnesses
  gain equivalent hook APIs.
- LSP diagnostics (v1) cover the editor-side case: agents that respect the
  LSP see the diagnostic in real time.

- **When it applies**: when you want zero possibility of an invalid write
  landing. Combines with Layers 2 and 3.
- **Cost**: adds one archwarden invocation per write. On warm caches this
  is dominated by the harness's own IPC overhead.
- **Limits**: hook rejection is disruptive if the rule is wrong. Layers 2
  and 3 exist to make the agent aware *before* the write, so Layer 4 rarely
  fires in practice.

## Command surface

### `archwarden describe <path>`

Given a path (which may or may not exist yet), returns the rules that
apply to it.

Output — text mode:

```
$ archwarden describe packages/application/src/use-cases/foo/foo.use-case.ts

Rules that apply to this path:

  [error] usecase-factory-name (naming)
    File must be named "<name>.use-case.ts" and export a function named
    Pascal(<name>). For "foo.use-case.ts", the required export is:
      export function Foo(deps: FooDeps): UseCase<FooInput, FooOutput> { ... }

  [error] usecase-needs-spec (spec-pair)
    Must have a sibling file "foo.use-case.spec.ts" containing at least
    one it(...) or test(...) call.

  [error] application-cannot-import-domain-internals (import-boundary)
    Imports from "packages/domain/**" are forbidden except for
    "packages/domain/src/*/types/**".
```

Output — JSON mode: same content as a stable, versioned object. Agents
should prefer JSON.

### `archwarden scaffold <path>`

Given a path, returns the smallest valid skeleton the file could have
under the current rules. This is not a code generator — it emits the
structural requirements only.

Output — text mode:

```
$ archwarden scaffold packages/application/src/use-cases/foo/foo.use-case.ts

Expected file shape:

  Required export:
    export function Foo(deps: FooDeps): UseCase<FooInput, FooOutput> { ... }

  Required sibling files:
    - packages/application/src/use-cases/foo/foo.use-case.spec.ts
      (must contain at least one it(...) or test(...) call)

  Import constraints:
    - forbidden: packages/domain/** (except packages/domain/src/*/types/**)

  Additional obligations: none.
```

Output — JSON mode: structured. Fields:

```json
{
  "version": 0,
  "path": "...",
  "required_exports": [ { "name": "Foo", "kinds": ["function"], "signature_hint": "..." } ],
  "required_siblings": [ { "path": "...", "constraints": ["non-empty-spec"] } ],
  "forbidden_imports": [
    { "pattern": "packages/domain/**", "except": ["..."], "include_type_only": true }
  ],
  "required_imports": [],
  "call_obligations": [],
  "filename_patterns": [],
  "allowed_subfolders": null
}
```

`kinds` is a list because `kind: ["function", "arrow"]` is a normal way to say
"callable, either form"; it is empty for `kind: "any"`, which asks for no
particular declaration form. One entry per glob in `forbidden_imports`, not per
rule: an agent asks "may I import this?" about one path at a time.

`filename_patterns` and `allowed_subfolders` are not extras. Without the first,
an agent scaffolding a path whose *name* is already wrong is told everything
except the thing it has to fix first; the second is what `scaffold` answers
when asked about a directory, which `describe` already does.

`signature_hint` is reproduced verbatim after the declaration keyword, so a
hint written in one form (`(deps: Deps) => UseCase`) under a rule demanding
another (`kind: "function"`) produces a line that does not compile. archwarden
never verifies the hint — that is `config doctor`'s job.

### `archwarden agent-guide`

Generates a rules digest optimised for agent context.

- `--format markdown` (default): grep-friendly headings, one section per rule.
- `--format json`: machine-readable dump of all active rules.
- `--scope <glob>`: restrict the guide to rules affecting a subset of paths.

The output is deterministic: same config, same output. Safe to commit if
you prefer to version it, or gitignore and regenerate on demand.

### `archwarden install-hooks <harness-flag>`

One-shot installer that writes the appropriate harness hook file.

- `--claude-code` — writes a `PreToolUse` entry matching `Write|Edit|MultiEdit`
  in `.claude/settings.json`, running `archwarden hook claude-code`.
- Future flags for other harnesses as their hook APIs stabilise.

The command is `archwarden hook claude-code`, not
`archwarden check --file $CLAUDE_FILE_PATH`. Claude Code does not pass the path
in the environment: the hook is handed the event as JSON on stdin, with the
target under `tool_input.file_path`. archwarden reads that itself, so the
installed line needs no `jq` and no shell quoting.

**The hook never blocks by failing.** An unreadable payload, a tool that writes
no file, a broken configuration — each allows the write. Blocking is a decision
carried in the response (`hookSpecificOutput.permissionDecision: "deny"`), never
a side effect of something going wrong. A hook that took a user's write down
with it would be uninstalled the same day.

A warning-level finding is shown without blocking, per decision 1.

The installer is idempotent: a second run reports the hook is already there and
does not rewrite the file, so it does not appear in `git status` for nothing. It
recognises its own entry by the command, so a user who narrowed the matcher or
added a timeout keeps that edit. Other hooks, other settings, and the order the
user wrote their keys in all survive. Uninstall with `archwarden install-hooks
--claude-code --remove`, which is also safe to run when nothing is installed.

### `archwarden check --file <path>`

Same engine as `archwarden check`, restricted to a single file. Used by
Layer 4 hooks.

**Every rule runs**, boundary rules included. An earlier draft of this
document expected graph rules to need cross-file state and be skipped on a
cold cache; that is not so. A boundary rule is file-local once its imports are
resolved — it asks about *its own* imports — and resolving them costs a handful
of filesystem probes. Measured at 3 ms per invocation against a real monorepo's
`node_modules` and `tsconfig`, for a file with four imports.

Two of the five rule kinds report through a directory check: a forbidden folder
and a missing spec are both facts about a directory's contents. Those run too,
against one listing per ancestor directory, **with the write folded in** —
neither the file nor the folders leading to it necessarily exist when a hook
asks, and checking the tree as it stands would miss exactly what the hook is
for. Their findings are then filtered to this path's own ancestry: an agent
writing one file is not handed its neighbour's problems.

What genuinely cannot run is a rule that reads a file this command could not.
That is **reported, never silently dropped**:

```json
{
  "version": 0,
  "path": "...",
  "findings": [ ... ],
  "skipped": [
    { "rule_id": "usecase-factory-name", "reason": "unreadable" }
  ],
  "unresolved_imports": []
}
```

Reasons are stable slugs: `unreadable` (the file could not be read or parsed)
and `not-source` (the rule is pointed at a file that is not TypeScript or
JavaScript, so there are no facts to read — the file is fine and the rule is
not). `skipped` is always present, even when empty: a caller has to see the
list is empty rather than infer it from absence.

This matters because a silent skip would make the same write pass or fail
depending on what the run happened to have available, which contradicts the
determinism goal in [`ARCHITECTURE.md`](ARCHITECTURE.md).

**`unresolved_imports` is the same failure one level down.** A boundary rule
that ran against an import nothing could place ran blind, and this command used
to answer `is fine.` either way. It carries every specifier this file imports
that did not resolve — an alias the file was written against, a dependency that
is not installed — and is present even when empty, for the same reason
`skipped` is. Where a hook is concerned this is the sharpest case there is: the
import an agent has just written is exactly the one nothing has seen yet.

```
$ archwarden check --file packages/domain/row.ts
note: `@Domain/Order/types` did not resolve, so boundary rules did not see it
```

It does not block the write. archwarden does not read `tsconfig` path aliases
([`CONFIG.md`](CONFIG.md)), so an unresolved specifier is not by itself a
violation of anything — it is the statement that no rule could tell.

## Recommended setup

For a project with active agent usage:

1. Add archwarden and a starter config: `archwarden init`.
2. Generate the agent guide: `archwarden agent-guide > .archwarden/AGENT_RULES.md`.
3. Add to `CLAUDE.md`:
   ```
   Before creating or editing any file under packages/ or apps/, run
   `archwarden describe <path>` and follow the returned rules. The full
   rule list is in .archwarden/AGENT_RULES.md.
   ```
4. Install the pre-write hook: `archwarden install-hooks --claude-code`.
5. Wire `archwarden check` into CI and `pre-commit`.

Layers 2, 3, and 4 stack. Layer 3 educates the agent; Layer 2 answers
specific questions; Layer 4 catches the residual cases where the agent
ignored both.

## Non-goals

- **archwarden does not modify the user's `CLAUDE.md` or `AGENTS.md`.**
  The user controls those files. archwarden writes to `.archwarden/`
  only.
- **archwarden does not generate business code.** `scaffold` returns
  structural requirements, never a working file body. Actual code is
  the agent's job.
- **archwarden does not enforce rules by rewriting the agent's output.**
  It rejects, explains, or informs. Rewriting is out of scope; that
  crosses into "AI-assisted refactoring" territory.
