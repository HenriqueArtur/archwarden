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
  "path": "...",
  "required_exports": [ { "kind": "function", "name": "Foo", "signature_hint": "..." } ],
  "required_siblings": [ { "path": "...", "constraints": ["non-empty-spec"] } ],
  "forbidden_imports": [ { "pattern": "packages/domain/**", "except": ["..."] } ],
  "required_imports": [],
  "call_obligations": []
}
```

### `archwarden agent-guide`

Generates a rules digest optimised for agent context.

- `--format markdown` (default): grep-friendly headings, one section per rule.
- `--format json`: machine-readable dump of all active rules.
- `--scope <glob>`: restrict the guide to rules affecting a subset of paths.

The output is deterministic: same config, same output. Safe to commit if
you prefer to version it, or gitignore and regenerate on demand.

### `archwarden install-hooks <harness-flag>`

One-shot installer that writes the appropriate harness hook file.

- `--claude-code` — writes a `PreToolUse` matcher for `Write`/`Edit` in
  `.claude/settings.json` that runs `archwarden check --file $CLAUDE_FILE_PATH`
  and blocks on non-zero exit.
- Future flags for other harnesses as their hook APIs stabilise.

The installer is idempotent: running it again either no-ops or updates
the block, never duplicates it. Uninstall with `archwarden install-hooks
--claude-code --remove`.

### `archwarden check --file <path>`

Same engine as `archwarden check`, restricted to a single file. Used by
Layer 4 hooks. Bypasses graph rules that require cross-file state unless
the file's cached facts already exist; the graph rules run in full on
`archwarden check` at commit time.

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
