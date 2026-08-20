# Configuration

archwarden reads exactly one config file: `arch.config.json` at the repo root.

## Discovery

Running `archwarden <command>` walks up from the current working directory
looking for `arch.config.json`. The first one found wins. This mirrors how
`git` finds `.git` and how `biome` finds `biome.json`.

Consequence: in a monorepo, you can run archwarden from any subpackage and
it will still analyse the whole repo, using the root config as the source
of truth.

If you must override discovery, pass `--config path/to/config.json`.

### `--config` and `--root` are two questions

`--config` answers *where the rules are*. It also answers *what they are about*,
because globs resolve from the config file's own directory — which is right for
the config a repository carries, and wrong for one kept anywhere else.

`--root` separates them:

```bash
archwarden check --config ../experiments/stricter.json --root .
```

Without it, a config outside the repository would take its own directory to be
the repository, walk it, find no TypeScript and report a clean run — exit 0, no
findings, and the question answered with the one wrong answer a reader takes as
good news. So that case is **exit 2** with a message naming this flag.

The refusal is narrow: an empty root you are *standing in* is checked normally,
because a repository that has just run `archwarden init` is empty and the very
next `check` must not claim the setup is broken. What is never legitimate is an
empty root reached only through a config file's location.

This is what makes "how many findings would this stricter rule produce?"
answerable without editing the file the project committed — see
[Measuring a rule change](#measuring-a-rule-change) below.

## Format

JSON. Not YAML, not TOML, not JS. Reasoning is in [`DECISIONS.md`](DECISIONS.md).

Every file starts with a `$schema` reference so editors give autocomplete
and inline validation without a plugin:

```json
{
  "$schema": "https://raw.githubusercontent.com/HenriqueArtur/archwarden/main/schema/v0.json",
  "version": 0,
  "modules": [ ... ]
}
```

**Where archwarden is installed from npm, point at the copy on disk instead:**

```json
{
  "$schema": "./node_modules/archwarden/schema/v0.json"
}
```

`archwarden init` writes this form automatically when it finds an install. It
is the schema for the version in your lockfile, it works offline, and it cannot
describe a different build than the one you are running — a URL can only ever
serve one version, and it will not always be yours.

The schema itself is generated from the Rust types that parse the config, and
CI fails if the committed copy drifts from them (`cargo xtask check-schema`).
A field that exists in the parser but not in the schema is a field your editor
would refuse to complete.

## Top-level shape

```json
{
  "$schema": "https://raw.githubusercontent.com/HenriqueArtur/archwarden/main/schema/v0.json",
  "version": 0,

  "root": ".",

  "ignore": [
    "**/node_modules/**",
    "**/dist/**",
    "**/.next/**"
  ],

  "skip_dirs": {
    "prefixes": ["_"],
    "globs": [],
    "scope": "structure"
  },

  "modules": [
    { "id": "domain",      "scope": "packages/domain/**",      "rules": [ ... ] },
    { "id": "application", "scope": "packages/application/**", "rules": [ ... ] },
    { "id": "api-routes",  "scope": "apps/api/src/routes/**",  "rules": [ ... ] }
  ],

  "rules": [ ... ]
}
```

- `root` — where to resolve globs from. Defaults to the config file's directory.
- `ignore` — extra ignore globs on top of `.gitignore` (which is always
  honoured). Ignore always wins over a rule's scope, however specific that
  scope is.
- `language` — the language the HTML pages are written in: `en` (default) or
  `pt-br`. Only the pages.
- `languages` — which languages archwarden reads. Defaults to `["ts"]`, which
  is JavaScript and TypeScript together. `["ts", "astro"]` adds the TypeScript
  module inside an `.astro` file's `---` fence.
- `skip_dirs` — the `_`-prefix escape hatch, see [`RULES.md`](RULES.md).
- `modules` — logical groupings of rules, optionally with paths of their own.
  Naming things helps error reporting: findings show
  `[domain] packages/domain/src/user/wrong-folder/`. See
  [modules with a scope](#modules-with-a-scope) for what `scope` adds.
- `rules` — rules that belong to no particular module, typically import
  boundaries (which are cross-module by nature). They report as `[*]`.
- `decisions` — the choices the rules keep, as prose. Top level only; a rule
  names one with `decision`. See [`decisions`](#decisions--what-the-rules-are-for).

### Allowing instead of forbidding

`import-boundary` has three directions. `forbid_import_from` denies,
`must_import_from` requires, and `only_import_from` permits — everything not
named is refused, including what nobody has thought of yet.

```json
{ "type": "import-boundary", "id": "api-depends-only-on-libs", "level": "error",
  "from_module": "api-orders",
  "only_import_from_modules": ["orders-core", "shared"] }
```

A denylist decays. Every new package, app or directory is allowed by omission,
and omission is the thing nobody notices. See [RULES.md](RULES.md#import-boundary)
for what sits outside an allowlist and why.

### One rule for every module of a sort

A `kind` on each module, and a rule quantifies over it — see
[RULES.md](RULES.md#import-boundary). One label per module, not a list: one
axis (assembly versus piece) is what the case needs, and a second vocabulary
for scope is what carrying a list would cost. If a second real axis appears,
that is where the conversation resumes.

### Modules with a scope

A module is a name for a group of rules. Give it a `scope` and it also becomes
a name for a part of the repository:

```json
{
  "modules": [
    { "id": "domain",         "scope": "packages/domain/**", "rules": [ ... ] },
    { "id": "infrastructure", "scope": "packages/infrastructure/**" }
  ],
  "rules": [
    { "type": "import-boundary", "id": "domain-is-sealed", "level": "error",
      "from_module": "domain", "forbid_module": ["infrastructure"] }
  ]
}
```

`scope` is optional, and a module without one is exactly what modules have
always been.

**A rule inside a scoped module reaches where both reach.** It keeps its own
`roots`, and the module narrows it. In practice the rule's `roots` is already
inside the module and nothing changes; when it is not, the rule reaches
*nothing* — so `config doctor` reports `rule-reaches-outside-its-module` rather
than leaving it silent. Narrowing rather than refusing at compile time is not a
preference: whether one glob contains another is not a question a glob engine
answers, and the only honest test is against a tree that has been walked.

**A boundary can name a module instead of re-describing it.** `from_module`
and `forbid_module` take module ids and become that module's paths when the
config compiles. Saying it both ways on one rule — `from` *and* `from_module` —
is refused, as is naming a module that does not exist or one that declared no
`scope`. Each of those would otherwise be a rule quietly governing nothing.

What it buys beyond less repetition: move a package and one line changes
instead of four, and `config doctor` gains two questions it could not ask
while a module was only a label — `module-scope-matches-nothing`, and
`module-nobody-references` for a module declared and never used.

### The HTML pages, and their language

```bash
archwarden agent-guide --format html --lang pt-br > arquitetura.html
archwarden check --html relatorio.html --lang pt-br
```

A repository decides this once, in the config, so nobody has to remember a
flag to read their own report:

```json
{ "version": 0, "language": "pt-br" }
```

`--lang` overrides it for one run. Neither reaches **anything but the page**. The terminal, the JSON and the
markdown digest stay in English whatever it says — a CI log is pasted into an
issue, searched for and read by an agent, and one whose language depends on who
ran it is worse than one somebody has to translate. The JSON was never in
question: its `type` slugs are stable identifiers.

`en` and `pt-br` today. The language is never detected from the environment: a
report whose language depends on the machine that produced it cannot be diffed,
and the guide page is meant to be committable.

The sentences a *rule* produces are still English on both pages. Those are
written once and shown in three places, so translating them is a change to the
terminal too — which is the line above.

### `languages` — what archwarden reads

```json
{ "version": 0, "languages": ["ts", "astro"] }
```

Opt-in, and **not because of cost** — a repository with no `.astro` file pays
nothing either way. What the field buys is that widening what archwarden
governs is a decision written in the config, rather than one that arrives with
a dependency upgrade.

**The un-opted state is loud, not silent.** A file in a language this config did
not ask for still produces a *counted, named* skip, so a user who never read
about the feature finds out:

```
note: `src/pages/blog.astro` was not read, so 1 check was skipped there: pages-forbid-domain
0 errors, 0 warnings, 1 skipped · 3 files, 4 directories · 9ms
```

That distinction matters: a skip on a file archwarden *could not read* is a bug
to investigate, and a skip on one it was never asked to read is a decision the
project has not made. `1 skipped` could not tell them apart.

A preset may not set `languages`, for the same reason it may not set `root`: it
cannot know whether the repository including it has any `.astro` at all.

Markdown is deliberately absent from the list. A `frontmatter` rule names the
documents it is about, and asking for the same thing in two places would let
them disagree.

## Rule categories

Every rule has:

- `type` — discriminator (`structure`, `naming`, `presence`, `pair`, `frontmatter`, `spec-pair`, `import-boundary`, `import-cycle`, `call-obligation`, `no-passthrough`, `export-shape`, `frozen`, `mirror`, `metadata`).
- `decision` — optional, on every kind: the id of the decision this rule
  implements. See [`decisions`](#decisions--what-the-rules-are-for).
- `id` — stable identifier used in output and in `explain`. Required, unique per config.
- `level` — `error` or `warning`.
- a **scope**: `roots` on every rule, except `import-boundary` where it is
  called `from`. Scope globs select **directories** — see
  [`RULES.md`](RULES.md) for what each rule then inspects inside them.

Every glob field accepts a single string or an array of strings:
`"roots": "src/**"` and `"roots": ["src/**"]` are the same.

### `why` — the reason, which nothing else records

Optional on every rule, and on every module. Free text.

```json
{
  "type": "import-boundary",
  "id": "domain-forbids-app",
  "level": "error",
  "why": "domain is published as its own package and the app is not; an import here makes the published artefact unbuildable outside this repo",
  "from": ["packages/domain/**"],
  "forbid_import_from": ["packages/app/**"]
}
```

The config already says *what* a rule does, in a form that cannot drift from
what it enforces; a prose restatement of that is a second source of truth that
goes stale. The reason cannot drift, because nothing else records it — the
format is JSON, so there are no comments, and a commit message is not in front
of anybody at the moment a rule fires.

It shows up where the rule is met: in the pre-write hook's denial, in
`describe` and `scaffold`, in `agent-guide`, in `config explain`, and beside a
finding. In text output a rule's reason is printed **once per run, at its first
finding** — a repository with two hundred findings over six rules would
otherwise print two hundred paragraphs. In JSON every finding carries it.

Two things it is not:

- **Not a message override.** `observed` and `expected` remain the whole
  diagnosis. A `why` that restates them has duplicated the finding and will
  contradict it the day the rule changes.
- **Not part of a finding's identity.** Rewording one never touches
  `.archwarden/baseline.json`.

A module takes one too, and it is a separate answer rather than a fallback:
"why is `domain` sealed" explains eight rules at once and is not an answer to
"why this one".

`config doctor` reports `rules-without-a-reason` as a count, and only once at
least one rule in the config has a `why` — a project that never used the field
has not adopted the practice, and being nagged about a convention you did not
choose is how a command that gives advice becomes one nobody runs.

### `decisions` — what the rules are for

`why` says why a rule exists. This says **what decision it implements**, which
is the difference between a config that enforces an architecture and one that
describes it.

```json
{
  "decisions": [
    {
      "id": "ADR-014",
      "title": "The domain does not know about transport",
      "why": "it is published, and a consumer must not inherit our HTTP client",
      "link": "docs/adr/014-domain-transport.md",
      "status": "accepted"
    }
  ],
  "rules": [
    { "type": "import-boundary", "id": "domain-forbids-http", "level": "error",
      "decision": "ADR-014",
      "from": ["packages/domain/**"],
      "forbid_import_from_packages": ["axios"] }
  ]
}
```

**The rule points at the decision, not the other way round.** A plain foreign
key, written where the author already is. There is no second list to keep in
step, a deleted rule leaves nothing dangling, and a new rule that forgets its
decision is visible in the one place it exists rather than absent from a list
nobody re-reads.

`title` is required and everything else is optional. `why` and `link` are two
answers to the same question and either is enough: a decision whose reasoning
runs to paragraphs belongs in the document, and copying its first sentence here
is the drift this field exists to avoid. The link is carried verbatim and never
resolved — archwarden does not check that a wiki page exists.

Declared at the **top level only**, never inside a module. A decision that spans
modules is the common case, and allowing both would create two places to look
for one thing.

Naming a decision the config does not declare is refused when the config loads,
like naming a module that does not exist. Naming *no* decision is fine, and is
what every rule written before 0.21 does.

**What it changes is what every surface says**, not what fires:

- the pre-write hook's denial stops being *"breaks `domain-forbids-http`"* and
  becomes *"breaks ADR-014, and here is why, and here is where it is written"*;
- `describe` answers "what applies here" with the decision each rule serves;
- `agent-guide` opens with the decisions, the rules that keep each one under it;
- `config explain` takes a decision id as well as a rule id, and answers the
  question people actually ask — not *what does this rule do* but *why is this
  like this*, plus the half a document cannot answer: whether it is still being
  kept, and **how much of it the baseline still excuses**;
- the HTML page leads with the architecture as decisions rather than as a rule
  table, each one carrying the debt the baseline holds against it;
- `baseline --dry-run` names the decision a new entry would be debt against;
- a denial, a finding and `describe` all say when the write is an option the
  decision already **weighed and rejected**, and why it lost;
- MCP's `check_write` names the decision a refusal breaks.

Not a place to restate what the rule enforces. A prose restatement of a check is
a second source of truth going stale — the decision explains the *choice*, and
the rules remain the only statement of what is enforced.

### What was rejected, and what replaced it

```json
{ "id": "ADR-031",
  "title": "the domain does not know about transport",
  "supersedes": "ADR-009",
  "alternatives": [
    { "option": "an HTTP client in the domain",
      "why_not": "a consumer would inherit our transport",
      "refused_by": "domain-forbids-http" },
    { "option": "a shared kernel",
      "why_not": "it becomes the place everything goes" }
  ] }
```

**`alternatives` is the half that stops the losing option being proposed
again** — by the next person, or by an agent that reads the rules, complies,
and helpfully suggests the thing that was already tried. `why_not` is required:
an option with no argument against it is a name nobody can disagree with.

**`refused_by` points at a rule you already wrote.** It never generates one.
`baseline` keys on rule ids, and an id derived from this prose would orphan
accepted debt the day somebody reworded the sentence. What the reference buys
is the honest distinction every surface then draws: an option with a rule is
mechanically refused, and one without it is written down while nothing stops
anybody taking it.

### The document archwarden writes and you edit

```
$ archwarden decisions
  + .archwarden/decisions/ADR-031.md

wrote 1 document, updated 0. The region between the `archwarden:yours`
markers was kept.
```

A decision's reasoning is three paragraphs, and JSON has no comments. This is
where they go — **not a second owner, but a rendering with a hand-written
region**. Everything the config knows is generated: the title, the `why`, the
status and the chain, what was rejected, and the rules that keep it. One marked
region belongs to whoever opens the file, and regenerating never rewrites it.

```markdown
<!-- archwarden:yours -->

## Context

Three services shared the order model and each one stored it its own way.

<!-- archwarden:end -->
```

Nothing moves out of the config. `title` is still the sentence a denial says
out loud and `why` is still what travels beside a finding — the document
renders both. What the region adds is the space the config never had.

They live in `.archwarden/decisions/`, beside `baseline.json` and outside the
gitignored `cache/`, because writing into a `docs/adr/` a team maintains by
hand is how a `--write` overwrites the wrong ADR.

**A document that falls behind is `config doctor`'s to report**, at `warning`,
as `decision-document-out-of-date`. Not a gate: a team adopting this
incrementally should not get a red build because a file needs regenerating.

**`supersedes` is written on the new decision**, and the reverse is computed.
The decision it names *is* superseded — its own `status` is not repeated, and
writing `accepted` there is refused where the config compiles. A cycle, or a
decision superseding itself, is refused for the same reason: a chain with no
end is one every surface would walk forever.

#### `status`, which is not decoration

`accepted` (the default), `proposed`, or `superseded` — the same three
`DECISIONS.md` uses, so this config can describe this project's own ADRs.

A **`superseded` decision whose rules still fire** is a config saying two things
at once, and `config doctor` reports it as an error. It is the most valuable
check here and the reason the field exists.

`proposed` is reported by nothing. A decision under trial with rules already
running is how one is trialled.

#### `enforcement: "none"` — the decisions no rule can keep

Most of what a team decides is not checkable. "We review schema changes with
the data team", "errors are values at the boundary and exceptions inside" — a
parser sees none of it, and a config that can only hold the checkable half
holds the smaller half.

```json
{
  "id": "ADR-023",
  "title": "Pub/Sub is the message broker",
  "enforcement": "none",
  "why_not_enforceable": "the broker is chosen in infrastructure, not in code",
  "scope": ["packages/queue/**"]
}
```

The claim does two things. `config doctor` stops reporting the decision under
`decision-nobody-enforces` — it is not an orphan, it is a decision that
declares its own limits. And `describe` prints it under **"decisions that
govern it, with no rule to keep them"**, which is how it reaches the agent
about to write there.

`why_not_enforceable` is **required** with the claim, and refused without it.
Half of the pair is worse than neither: `enforcement: "none"` alone is a way to
switch the orphan report off one decision at a time, and the reason is the part
a reader actually needs. A reason with no claim is refused too — the decision
would explain why it cannot be enforced while never saying it is not.

A rule pointing at a decision that claims unenforceability is an **error** in
`config doctor`: the config is saying two things at once, and only the author
knows which is stale.

#### `scope` — where a decision applies

Without it a decision governs the repository, which for most is right. With it
the decision is about a place:

```json
{ "id": "ADR-023", "title": "Pub/Sub is the message broker",
  "scope": ["packages/queue/**", "services/worker/**"] }
```

The same globs a rule's scope takes. It changes one thing and it is the point
of the field: `describe packages/queue/worker.ts` brings this decision
unprompted, and `describe packages/ui/button.tsx` does not. A decision that
reaches everybody about everything reaches nobody.

A scope matching no directory is a **warning** — `decision-scope-matches-nothing`
— not an error, on different terms from a rule's empty scope. A rule with no
files enforces nothing and that is a hole; a decision with no files is still
written down and still true. What it has lost is the way it arrives unprompted.

#### Finding one before you propose against it

`alternatives` records what lost and why, and `config explain` ends with **"Do
not propose it again."** — which it can only say to somebody who already knows
the id. The person about to propose the losing option is, by definition, not
that person, and will name it differently from whoever rejected it: *single
layer*, *monolith*, *one package* and *just put it together* are the same
option under four names.

```
$ archwarden decisions find camada unica
2 places mention `camada unica`:

  ADR-001 — Quatro camadas, mais o System
    title  "Quatro camadas, mais o System"
      `camada` prefix of `camadas`
    alternatives[0].option  "uma única camada"
      `camada` exact
      `unica` exact
```

It says **why it matched**, never a score: exact, a prefix, or how many
characters off. The same contract a finding keeps — a reader adjusts the query
by reading the answer, rather than trusting a number they cannot inspect.
Accents and case are ignored on both sides, which is not optional for a
bilingual repository, since nobody types the accent into a query.

Every match, in declaration order, with no ranking and no top-N. The corpus is
a hundred short strings, so returning eight instead of three is free — and the
errors are asymmetric: a false negative means the rejected option gets proposed
again, which is the exact failure `alternatives` exists to prevent, while a
false positive costs two seconds of reading. `--format json` for the same
answer as data, and the MCP tool `decisions_find` for an agent.

`config doctor` asks the same question in the other direction:
`decision-may-duplicate` reports two decisions that appear to say the same
thing, catching the duplicate at the moment it is written rather than waiting
to be asked. That check is **stricter** than the command, and deliberately: the
command answers a person who will read what comes back, while the concern lives
in a gate, and a gate that cries wolf is one somebody turns off. It compares
only the fields that *name* something — titles and rejected options, never the
prose — and requires every word of the shorter phrase to be reached. A decision
that `supersedes` another is exempt: saying the same thing twice is what
superseding is.

#### Presets ship decisions

`extends` folds them the way it folds rules: concatenated, presets first. Two
decisions with one id is refused, for the same reason two rules with one id
already are. So is one id that is a rule in one place and a decision in
another — `config explain` takes either, and an id naming two things names
neither.

This is the interesting consequence: a preset stops being a bag of rules and
becomes **a set of opinions with names and reasons**, which is what makes one
worth adopting rather than copying.

#### What `doctor` says, and what `check` does not

`check` stays silent about all of this. A repository's build must not fail
because its config is under-documented, and a gate that failed for that is one
people turn off.

| code | level | when |
| --- | --- | --- |
| `rule-without-a-decision` | warning | some rules name one and others do not, counted in one line |
| `superseded-decision-still-enforced` | error | a replaced decision whose rules still fire |
| `decision-nobody-enforces` | warning | a decision declared and implemented by no rule |

The first appears only once at least one rule names a decision — a project that
never used the field has not adopted the practice, and being nagged about a
convention you did not choose is how a command that gives advice becomes one
nobody runs.

`config doctor` gained a **level** on every concern in 0.21. The sixteen checks
that came before are all `warning`, which is what they have always been in
practice; it does not reach the exit code, because `doctor` is advice and
`check` is the gate.

### Frontmatter rule

For a document whose YAML block is read by something.

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

Values compare as text, so `"1"` matches `nivel: 1` and a quoted value matches
an unquoted one. `{{raw(dirname)}}` is the directory the document sits in, and
it is the only group a document template may name — the form is `naming`'s, so
`{{kebab(dirname)}}` works too.

A document with no `---` block is a finding, not a skip; a block that is not
YAML is a different finding. What this rule deliberately cannot say is anything
about the *shape* of a value — no `type`, no `min_items`, no nested paths. That
is a document schema and JSON Schema is one. See [`RULES.md`](RULES.md).

### A note on regexes

Regex fields (`file_pattern`, `filename_patterns`) are matched with Rust's
`regex` engine, which guarantees linear-time matching. It does **not** support
lookahead, lookbehind, or backreferences — a deliberate trade, because
archwarden runs inside pre-commit hooks and agent pre-write hooks, where a
catastrophically backtracking pattern would be a denial of service on your own
workflow. Named capture groups (`(?<name>...)`) work normally.

`archwarden config validate` reports unsupported constructs with a message
saying so, rather than a raw engine error.

### Unknown fields are refused

A key archwarden does not recognise is an error, not something ignored:

```
× arch.config.json is not a valid archwarden config: at `rules[0]`:
  unknown field `allow`, expected one of `id`, `level`, `roots`,
  `allowed_subfolders`, `warn_subfolders`, `recurse_into`, `filename_patterns`
```

A misspelled key would otherwise compile to a rule that constrains nothing,
which `validate` would call valid and `check` would report as a clean
repository. A rule that silently enforces nothing is the worst failure a linter
has, because it is indistinguishable from a rule that passes.

The published JSON Schema says the same (`additionalProperties: false`), so an
editor with `$schema` wired up flags the typo before archwarden runs.

The cost is that a config written for a newer archwarden is **refused** by an
older one rather than degrading. That is the intended trade: a config file is
small, versioned by its `version` field, and a wrong guess about what a key
means is worse than an error.

The six rule types are specified in [`RULES.md`](RULES.md). This section
shows realistic examples for each.

### Structure rule

Ported from Flowmaatik's `check-structure.config.ts`:

```json
{
  "type": "structure",
  "id": "domain-entity-shape",
  "level": "error",
  "roots": ["packages/domain/src/*"],
  "allowed_subfolders": [
    "types", "calcs", "actions", "services",
    "mocks", "repositories", "const", "variants"
  ],
  "warn_subfolders": ["shared", "adapters"],
  "recurse_into": ["variants"]
}
```

**`allowed_subfolders: []` is not the same as leaving it out.** Omitted, the
rule says nothing about subfolders. Written as an empty list, it is a list of
what may exist holding nothing, so no subfolder may exist — the way to say
"this directory is a leaf". A rule that names none of `allowed_subfolders`,
`warn_subfolders` or `filename_patterns` enforces nothing, and `config doctor`
says so as `rule-constrains-nothing`.

`recurse_into` names a **container whose children** are entities of the same
shape: `user/variants/nfe` is governed, `user/variants` is not, and `nfe` may
be called anything. It is one level deeper than it reads, and it *removes*
findings — the directories inside the container stop being unexpected
subfolders and become entities. That is a decision worth making on purpose;
`config explain domain-entity-shape` lists every directory the rule governs,
which is how to see that you made it.

### Filename rule

```json
{
  "type": "structure",
  "id": "api-route-filenames",
  "level": "error",
  "roots": ["apps/app/src/app/api/**"],
  "filename_patterns": [
    "^route\\.ts$",
    "^route\\.(get|post|put|patch|delete|options)\\.ts$",
    "^route\\.(get|post|put|patch|delete|options)\\.spec\\.ts$",
    "^route\\.(get|post|put|patch|delete|options)\\.factory\\.ts$",
    "^DOC\\.md$"
  ]
}
```

### Naming coupling

```json
{
  "type": "naming",
  "id": "usecase-factory-name",
  "level": "error",
  "roots": ["packages/application/src/use-cases/*"],
  "file_pattern": "^(?<name>[a-z0-9-]+)\\.use-case\\.ts$",
  "must_export": {
    "kind": "function",
    "name": "{{pascal(name)}}",
    "signature_hint": "(deps: {{pascal(name)}}Deps): UseCase<{{pascal(name)}}Input, {{pascal(name)}}Output>"
  }
}
```

Note the scope: `use-cases/*` selects each use-case *directory*, and
`file_pattern` then matches files directly inside it. `signature_hint` is
never verified — it only makes `scaffold` output realistic.

`must_export.annotation` is the one field here that **is** verified. It names
the type the export must be annotated with, as a template over the same capture
groups:

```json
"must_export": {
  "kind": ["const"],
  "name": "AGENT_TOOL",
  "annotation": "AgentToolModule"
}
```

`export const AGENT_TOOL: AgentToolModule = {...}` passes;
`export const AGENT_TOOL = {...}` does not. It is still not type checking —
nothing is resolved and nothing is inferred, and whether the annotated value
really is of that type stays `tsc`'s question. What it gates is whether the
declaration is submitted to `tsc` at all, which is what a registry loses when
it moves from a typed array to `readdir` and `import()`. See `docs/RULES.md`.

`{{pascal(name)}}` is a small templating helper: the named capture group
`name` from `file_pattern` gets fed to a case transformer. Supported:
`pascal`, `camel`, `kebab`, `snake`, `upper`, `lower`, `raw`.

When the convention spells the export from the directory as well as the
filename — `Order/fetch-by-id.ts` exporting `OrderFetchByIdRepository` — add
`dir_pattern`, whose capture groups join the same template:

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

`dir_pattern` is matched against the *name* of the directory the file sits in —
`Order` — not against the path leading to it. A group defined by both patterns
is refused when the config compiles, because it would have two values and no
rule for choosing between them. See `docs/RULES.md` for the full semantics.

### Spec pairing (TDD gate)

```json
{
  "type": "spec-pair",
  "id": "domain-calcs-need-spec",
  "level": "error",
  "roots": ["packages/domain/src/*"],
  "subfolders": ["calcs", "services", "adapters"],
  "spec_markers": ["spec", "test"],
  "ignore_files": [
    "packages/domain/src/nota-fiscal/variants/nfe/services/nfe-service.ts",
    "packages/domain/src/**/*.types.ts"
  ]
}
```

`ignore_files` takes globs, so both an exact path and a pattern work. `naming`
takes the same field with the same meaning — and both are one rule's exemption
rather than the walk's. The top-level `ignore` hides a file from **every** rule,
which is the wrong tool for *this rule should not ask about that file*: a
repository wanting a `metadata` rule to still see it had to choose between the
two. Issue #153.

`spec_markers` defaults to `["spec", "test"]` and can usually be omitted: it
is what vitest and jest both accept. The extension is never configured — it
comes from the source file, so `Component.tsx` pairs with
`Component.spec.tsx`. See [`RULES.md`](RULES.md) for how a compound name like
`user.db.repository.ts` is handled.

`spec_dirs` names directories beside the file where a spec also counts —
`["__tests__"]`, `["tests"]`, whatever the project uses. Empty by default,
which is sibling-only. It reaches exactly one level: a spec in
`__tests__/unit/` does not count unless `unit` is named too, and an entry with
a path separator is refused when the config compiles. `RULES.md` has the
reason that limit is not a shortcut.

Optional `require_non_empty_spec: true` fails on `.spec.ts` files that contain
no `it(...)` or `test(...)` calls — this is what enforces "spec written
first", not just "spec file exists". A `describe(...)` alone does not satisfy
it.

### Import boundary

An ordinary rule, with `from` as its scope field. Boundaries are cross-module,
so they normally live in the top-level `rules` array:

```json
{
  "rules": [
    {
      "type": "import-boundary",
      "id": "domain-forbids-application",
      "level": "error",
      "from": "packages/domain/**",
      "forbid_import_from": ["packages/application/**"]
    },
    {
      "type": "import-boundary",
      "id": "ui-forbids-domain-direct",
      "level": "error",
      "from": "apps/**/src/**",
      "forbid_import_from": ["packages/domain/**"],
      "except": ["packages/domain/src/*/types/**"]
    }
  ]
}
```

There is no `graph` key. Boundaries are rules like any other, so they go
through the same matcher, the same `describe_expectation()`, and show up in
`describe` and `agent-guide` with no special-casing — which is what keeps
those commands in lockstep with the checker (decision 9).

A boundary can also name a **dependency**, which has no repo-relative path for
a glob to match:

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

`forbid_import_from_packages` takes package names, matched as "this package and
anything under it" — so `three/examples/jsm/loaders/GLTFLoader.js` is caught and
`three-mesh-bvh` is not. `node:fs` and `fs` are one identity.

`except_from` exempts the *importing* file from the whole rule, which is the
side an exception to a dependency rule sits on: `except` is about what is
imported. See [`RULES.md`](RULES.md) for the full semantics, including why this
is a separate field rather than a prefix inside `forbid_import_from`.

A boundary can also forbid what a file **ends up** depending on, however many
imports away:

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

`forbid_reaching_modules` names a declared module instead of repeating its
globs, as `forbid_module` does for the direct form; saying it both ways on one
rule is refused. A **direct** import is not reported by this field —
`forbid_import_from` is what reports that one.

**This field makes the run read the whole repository.** See
[import cycles](#import-cycles) just below and
[`RULES.md`](RULES.md#what-a-graph-rule-costs) for the measured cost.

### Import cycles

A rule about the files it governs, so its scope field is `roots`:

```json
{
  "type": "import-cycle",
  "id": "no-cycles",
  "level": "error",
  "roots": "packages/**"
}
```

Every file of a loop that `roots` covers is reported, once each, carrying the
whole chain. There is no `ignored_circular_dependencies`: a cycle is a finding
and [`baseline`](#adopting-archwarden-in-an-existing-repository) already accepts
findings, per rule and per path.

`include_type_only` defaults to `true` and means what it means on
`import-boundary`.

`roots` decides where a finding is *reported*, not what the graph is built
from. The graph is always the whole repository, because a loop that leaves the
scope and comes back is still a loop — which is why a configuration carrying
this rule, or `forbid_reaching` above, parses and resolves every source file.
On a 10 000-file repository that is 0.22 s against 0.01 s for the same scope
without it. A configuration with neither pays nothing.

`check --file` and the pre-write hook refuse these rules, under the
`needs-repository` skip reason, rather than answering from one file.

### Call obligation

The semantic rule that no lint plugin does well:

```json
{
  "type": "call-obligation",
  "id": "non-get-routes-must-audit",
  "level": "error",
  "roots": ["apps/app/src/app/api/**"],
  "file_pattern": "^route\\.(post|put|patch|delete)\\.ts$",
  "must_call": {
    "symbol": "Event.save",
    "imported_from": "@flowmaatik/domain/event"
  }
}
```

The obligation is satisfied when the call appears **anywhere in the file**.
That includes a local helper the export delegates to, which is the case this
rule has to get right — demanding the call at the top level would fire on
well-factored code.

It deliberately stops there. A file that calls `Event.save` only from a
function nothing reaches still passes, in the same way `RULES.md` declines to
filter calls inside `if (false)`: archwarden is a structural linter, not a
reachability analyser, and a rule that were sometimes right about dead code
would be harder to trust than one that is never asked.

Cross-file analysis is out of scope for v0 — the obligation must be satisfied
within the file itself.

### No passthrough

```json
{
  "type": "no-passthrough",
  "id": "domain-no-indirection",
  "level": "warning",
  "roots": ["packages/domain/src/**"],
  "forms": ["reexport", "alias", "wrapper"],
  "except": [],
  "allow_package_entrypoints": true,
  "allow_partial": true
}
```

A file whose whole content is forwarding another module. Run against a real
repository, that config found four:

```
warning packages/domain/src/plan/calcs/to-json.ts
        [domain] domain-no-indirection — adds nothing of its own: `PlanJson`, `planToJson` only forward another module
```

`forms` defaults to all three and narrows to one question at a time. **"No
barrel files"** is this rule with `forms: ["reexport"]` and
`allow_package_entrypoints: false` — a barrel is a re-export and nothing else.

`allow_package_entrypoints` defaults to `true` because a package's public API
is a file whose job is forwarding. Leave it on unless you are hunting barrels.

`allow_partial` defaults to `true`, so only a file where *every* export is a
forward is reported. Setting it to `false` also reports a file that forwards
some and declares others, naming which — on the same repository, 4 findings
became 26. Both are true; they answer different questions.

Every rule type is specified in [`RULES.md`](RULES.md).

## Presets

Presets let you share rule sets between projects.

```json
{
  "$schema": "https://raw.githubusercontent.com/HenriqueArtur/archwarden/main/schema/v0.json",
  "version": 0,
  "extends": ["@myorg/arch-preset-clean-arch"],
  "modules": [
    { "id": "project-specific", "rules": [ ... ] }
  ]
}
```

A preset is any published package whose entry point is a JSON file matching
the config schema. Local presets work too: `"extends": ["./presets/base.json"]`.

**Resolution.** A `./`-prefixed entry is a path. Anything else is an npm
package name, resolved with the same resolver archwarden uses for imports
(`oxc_resolver`), so npm, yarn classic, pnpm, and yarn PnP layouts all work
without special handling.

### The ones that ship

`presets/rust.json` travels inside the npm package, so a repository that
installs archwarden has it at `node_modules/archwarden/presets/rust.json`:

```json
{
  "version": 0,
  "languages": ["rust"],
  "extends": ["./node_modules/archwarden/presets/rust.json"]
}
```

Three rules: every unit carries its tests inside it, a file names what it
exports, and a file that declares a stability carries a deadline where it needs
one. Barrels — `mod.rs`, `lib.rs`, `main.rs` — are exempt from both of the
first two by construction, so nothing has to list them.

`presets/tauri.json` is the same package, and is the rule set this whole
milestone exists for: every `invoke("...")` in `src/` names a
`#[tauri::command]` in `src-tauri/src/`, and commands live in one folder.

```json
{
  "version": 0,
  "languages": ["rust"],
  "extends": ["./node_modules/archwarden/presets/tauri.json"]
}
```

**The preset turns the language on for you.** Extending it is the whole
instruction; there is no second line to remember. Until 0.33 there was, and
forgetting it produced a clean green run with a skip note — the failure
`docs/CONFIG.md` calls the worst a linter has, arriving on adoption day. Issue
#158, decision 35.

**Merging.**

- Arrays (`modules`, `rules`, `decisions`, `ignore`, `extends`) are concatenated.
- `languages` is **unioned** across the whole chain. It is a set, and extending
  a Rust preset and an Astro one means both — there is no way to spell a
  conflict between two members. A preset cannot know whether your repository
  has any `.astro`, but a preset whose every rule is about `.rs` knows exactly
  what it needs.
- Scalars (`root`, `version`, `governance`) — the local config wins over any
  preset, and a preset may not set the last two at all.
- A preset declaring `root` is an error. A preset cannot know your repo
  layout, and silently relocating every glob in the config is not something
  a shared package should be able to do.
- Rule `id` collisions are an error caught by the doctor. So are decision `id`
  collisions, and an id that is a rule in one file and a decision in another.

**Declining a language a preset turned on.** There is no field for it, and
there does not need to be: a language costs nothing on its own, because a file
is only parsed when some rule's scope reaches it. `disable` the preset's rules
and the union stays while nothing is read. `config validate` prints
`reads: rust, ts` whenever a preset is involved, so what a preset turned on is
visible rather than inferred from a run getting slower.

**Removing an inherited rule.** A top-level `disable` list drops rules that
came from a preset:

```json
{
  "extends": ["@myorg/arch-preset-clean-arch"],
  "disable": ["clean-arch-no-barrel-files"]
}
```

Without this, one unwanted rule makes a whole preset unusable. Disabling an
id that does not exist is a doctor error, so a typo fails loudly instead of
silently disabling nothing.

## Adopting archwarden in an existing repository

The first run on a repository that did not grow up with these rules reports
everything at once — on one real project, 32 errors and 46 warnings. That
leaves two bad choices: keep archwarden out of CI, where the rules rot, or put
it in and teach everyone to ignore red.

```bash
archwarden baseline     # writes .archwarden/baseline.json
git add .archwarden/baseline.json
```

`check` now reports only findings that are not in it. The build is green today
and fails at the next *new* violation.

**Commit the file.** Each line is debt the project has decided to carry, and a
line added in a pull request is a visible decision — reviewed like any other.
That is the whole difference between a baseline and a suppression file.

Every run says where it stands:

```
0 errors, 0 warnings · 3778 files, 1034 directories · 593ms
78 accepted, 12 no longer occur — run `archwarden baseline` to update
```

The second number is the ratchet. Fixing accepted debt is reported, and the
entries become removable — without which, fixing a violation and reintroducing
it later would be hidden by the stale entry.

`archwarden check --no-baseline` reports everything again, for when the
question is "how bad is it really".

**Unlike the filters below, a baseline changes the exit code.** That is what it
is for, and why it is a committed file rather than a flag.

### What counts as the same accepted finding

The rule and the path, and nothing else.

Not the level: promoting a rule from `warning` to `error` is the project
raising its own bar on debt it already acknowledged.

Not the detail: renaming a disallowed folder from `handlers` to `controllers`
is not a new violation, and treating it as one would make the file churn on
every rename. The cost is a case this deliberately does not catch — fixing a
violation and breaking differently *at the same path under the same rule* stays
accepted.

And no timestamp, so regenerating an unchanged repository produces no diff.
`git blame` on the file already says when each line arrived and who wrote it.

The pre-write hook respects the baseline too: an agent editing a legacy file is
not blocked by debt that is not its own.

## Filtering the report

Four flags on `check` decide what is **printed**. None of them decides what is
**checked**: every rule runs, every finding is computed, and the exit code is
identical with them and without. That is what makes one safe to leave in a
command that gates a build.

```bash
archwarden check --summary                        # per-rule counts, no listing
archwarden check --rules domain-entity-shape      # one rule's findings
archwarden check --paths 'packages/domain/**'     # one area of the repo
archwarden check --level error                    # warnings are known debt
archwarden check --changed                        # uncommitted work
archwarden check --changed main                   # everything this branch does
```

`--changed` asks git which files differ from a ref, defaulting to `HEAD`.
Untracked files count; gitignored ones do not. So do the directories those
files live in, because a `structure` finding names the directory rather than
the file that brought it into existence.

It is a filter like the rest, which is the point: decision 12 says `check`
covers the repository, and a `--changed` that narrowed what is *evaluated*
would let a pull request touching only `apps/web` pass with a regression
sitting in `packages/domain`. Here the build still fails; the report just shows
the part you asked about, and `hidden` says how much it left out.

For the same reason it is not "fail only on new violations". That is a
baseline — a committed record of accepted debt — and it is a different feature.

`--rules` and `--paths` are repeatable and comma-separated; `--rules a,b` and
`--rules a --rules b` are the same thing. All four compose with AND. `--paths`
matches against the finding's path through the same glob engine `ignore`,
`roots` and `forbid_import_from` use — there is only one matcher.

**An entry with no glob character in it is a path, not a pattern**, and selects
that path and everything under it. The path a reader has to hand is the one
they just copied out of a finding, and having to remember `/**` would turn
"look closer at this" into an empty report.

```bash
archwarden check --paths packages/domain/src/order     # that directory and below
archwarden check --paths 'packages/domain/src/*'       # exactly one level
```

An entry that *does* contain a glob is used exactly as written. Someone who
wrote `src/*` means one level, and widening it to `src/*/**` would be
archwarden overruling them.

`--summary` prints one row per rule, worst first: errors descending, then
warnings, then by id.

```
domain-entity-shape  3 errors
types-need-spec      3 errors
app-shape            1 error
calcs-need-spec      3 warnings

7 errors, 3 warnings · 8 files, 20 directories · 1ms
```

`--by path` counts the same findings by area of the repository instead, and
implies `--summary`:

```
packages/domain/src/invoice  2 errors, 1 warning
packages/domain/src/order    2 errors, 1 warning
packages/domain/src/client   1 error, 1 warning
```

The areas are the directories the rules' own scopes already select — a config
saying `roots: packages/domain/src/*` has declared that
`packages/domain/src/order` is a unit, so nothing here has to choose a depth. A
finding no scope reaches keeps its own path rather than being dropped or filed
under a heading that means nothing.

The two answer different questions. `--summary` says which rule is dominating
the output; `--by path` says which part of the repository is furthest from the
rules, which is the one that says where to start. Unlike the rule breakdown,
only areas with findings get a row: printing every clean directory in a
monorepo would bury the ones that are not.

A rule that found nothing keeps its row with a `0`. That it was evaluated is
an answer; a missing row would read as a rule someone disabled. `--rules`
narrows the rows — it is the one filter that names rules — while `--paths` and
`--level` leave every row in place.

In `--format json`, `--summary` adds a `by_rule` map beside the counts and
**omits the `findings` array**. A summary that still emitted every finding
would give a piping consumer no size benefit, which is most of the reason to
ask for one.

Two behaviours worth knowing:

- **The counts describe what you asked to see.** `0 errors` beside exit code 1
  is possible and correct: the gate counts what was evaluated. `summary.hidden`
  and a `note:` line in the text output say how many findings the filter
  removed, so the two are always reconcilable.
- **An unknown rule id is an error (exit 2)**, not an empty report — the same
  way `disable` and `config explain` refuse one. A filter that silently matched
  nothing would look exactly like a clean repository, which is the one wrong
  answer a user reads as good news.

## Before you move a file

```bash
archwarden impact packages/domain/src/order/calcs/total.ts \
           --to  packages/app/src/billing/total.ts
```

An editor moves a file and rewrites its imports. It says nothing about whether
the destination is somewhere the architecture allows the file to be, or whether
the move puts an existing import across a boundary. That half is this one, and
it is the half nothing else answers.

It reports which rules would start and stop applying, which files import the
target and which of those imports would *newly* be forbidden, how many of the
file's own relative imports would need rewriting — and which files contain a
dynamic import it cannot read.

That last one matters. `import(name)` names no single module, so archwarden
records nothing for it: right for a rule, which must not report a path nobody
wrote, and wrong for a question about who imports a file. Those files are
listed separately, because a confident answer with a hole in it is worse than
an incomplete one that says so.

"Newly" is doing work too. A boundary already being crossed is debt `check`
reports today, not a consequence of the move, and listing it here would blame
the move for something it did not do.

Relative imports are counted, not re-resolved: whether they still point
somewhere afterwards is a question `tsc` answers better.

Reading the import graph backwards means resolving the whole repository, so
this costs about what a `check` costs.

### Carrying it out

`--apply` does the move. Dry run stays the default, and this is a second,
explicit word.

```bash
archwarden impact packages/domain/src/id/shared/is-id-invalid-shared.ts \
           --to  packages/domain/src/id/calcs/is-id-invalid.ts --apply
```

Files move with `git mv`, so history follows them. Every import specifier that
named the file is rewritten — **including the ones written by package name**,
which is the half an editor cannot do: to an editor, `@org/domain/email/x` is a
package like `react`. In the repository this was built against, that is the
majority of imports.

The spec sibling travels with its unit file, and follows a rename:
`is-id-invalid-shared.spec.ts` becomes `is-id-invalid.spec.ts`. Leaving it
behind would break archwarden's own `spec-pair` rule.

A source directory the move empties is removed, because `structure` rules are
about directories and an emptied `shared/` would keep reporting the finding the
refactor was run to remove.

**The exported symbol is not renamed.** A file renamed mid-move keeps
`isIdInvalidShared`, and the output says so. Renaming an export breaks every
caller in a way this cannot see; `check` reports the mismatch afterwards, which
is where a `naming` rule belongs.

### A whole layer at once

A directory or a glob as the source makes `--to` relative to **each matched
directory**:

```bash
archwarden impact 'packages/domain/src/*/shared' --to '../calcs' --apply
```

Every `shared` becomes the `calcs` beside it. Files nested inside a match land
in the destination directly — `feature/shared/consts/list-shared.ts` goes to
`feature/calcs/`, not to `feature/shared/calcs/`. Two files landing on one path
is refused before anything is written.

One file keeps the other reading: `--to` is the whole destination path, which
is what makes renaming during a move expressible at all.

### Sequencing a layer refactor: passthrough first, then the move

`impact --apply` moves files. It does not delete indirection — a file whose
whole content forwards another module is still there afterwards, forwarding the
new location. So when a refactor is both ("collapse this folder away" *and*
"the files in it are reached through wrappers"), the order changes the size of
the diff and it is not obvious which way.

**Run `no-passthrough` first.** Every passthrough file you delete is a file
`--apply` no longer has to rewrite importers for — because its importers now
name the real module directly.

Taking it the other way round works and costs more: the move rewrites every
specifier that goes *through* the wrapper, and then deleting the wrapper
rewrites them again. Two commits touching the same lines, and the first one's
diff is noise.

Neither order is wrong, and neither is automatic: deleting a passthrough file
means editing its importers to name what it forwarded, which is a change to
call sites and archwarden does not make it. `no-passthrough` reports; you
decide; then `--apply` moves what is left.

One thing the report will not tell you: a passthrough can cross a module
boundary. On the repository this was built against, an entity's `calcs/` file
wrapped *another entity's* `shared/` module — so collapsing `shared/` inside
each entity did not remove that indirection, it only moved its target. Read the
`no-passthrough` findings before deciding what the move is meant to achieve.

### What it refuses, and why a refusal is safe

Everything is computed and validated before a byte is written, so **a refusal
means nothing happened** — there is no state where half the imports are
rewritten.

| refusal | why |
|---|---|
| the working tree is dirty | `git` is the undo, and one that takes your own work with it is not one |
| not a git repository | same reason: no undo |
| a file being moved is untracked | `git mv` cannot move it, and `git checkout .` cannot bring it back — the two halves of the same fact. Asked before anything is written, because git asks it in the middle of the move |
| a specifier this cannot recompute | a `tsconfig` path alias whose entry does not reach the destination. The alias the importer already writes *is* re-run against the new location, and rewritten when it still covers it; what refuses is a move out of what the alias covers, or an entry naming one file rather than a subtree |
| the destination exists, or two files land on one path | carrying it out would delete something |
| a dynamic import naming no module | whether that file imports the target is unknowable |

Only the last is overridable. `--force` is a human saying they looked, and the
report prints the file and the line to look at. The others produce a repository
that does not build, which is not a judgement a flag should be able to make.

There is one more, and it should never fire. After the plan is built, every
file the dry run named as an importer must have come out of it with an edit.
Nothing above is supposed to be able to break that — a specifier that cannot be
recomputed refuses on its own — but the failure it would hide is the worst one
here: a repository that compiles nowhere, reported as success with exit 0, and
found by whoever runs `tsc` next. If you ever see it, it is a bug worth
reporting with the command you ran.

## Measuring a rule change

Rule 2 of [`AGENTS.md`](AGENT-INTEGRATION.md) says not to edit `arch.config.json`
to make a check pass. Planning to *tighten* a rule needs the opposite of that
and looks identical from outside: you have to change the file to find out what
changes. A config kept somewhere else answers without persisting anything.

```bash
cp arch.config.json /tmp/stricter.json
# edit /tmp/stricter.json — drop `shared` from warn_subfolders
archwarden check --config /tmp/stricter.json --root . --summary
```

```
domain-entity-shape                        7 errors, 2 warnings
domain-actions-should-have-spec           37 warnings
domain-calcs-services-adapters-need-spec   0
domain-variants-calcs-services-need-spec   0
```

Seven errors, and `config explain` on the id says which paths. Nothing was
written, so there is no config change to remember to revert — which is the
difference between measuring a decision and making one.

`--root .` is not optional here; without it the run refuses. See
[`--config` and `--root` are two questions](#--config-and---root-are-two-questions).

## Does this folder have a reason to exist?

```bash
archwarden orphans                                  # every folder
archwarden orphans 'packages/domain/src/**/shared/**'
archwarden orphans packages/domain/src/order --by-file
```

For every file: who imports it, and whether from inside the module it lives in,
from outside it, or nobody. Aggregated by folder.

```
packages/domain/src/flow-node/shared/calcs     2 files   inside-only 0   outside-only 2   both 0   nobody 0
                                               → only used from outside its module — the boundary is drawn elsewhere
packages/domain/src/feature/shared/consts      1 file    inside-only 0   outside-only 0   both 1   nobody 0
```

Three shapes, three meanings:

- **Only from outside** — nothing in the module it sits in needs it. It belongs
  to its callers, not to its parent, and the boundary is drawn in the wrong
  place.
- **Only from inside** — part of how the module works rather than of what it
  offers. It should be private.
- **Nobody** — dead, or reached only through a dynamic import. Those files are
  listed at the end, because a folder above may be reached from one without
  showing it.

A folder that is a mix gets no verdict. That is a folder nobody has decided
about, and a sentence claiming otherwise would be the tool guessing.

**"Module" is the area the config already declares** — the same directories
`check --by path` counts by. A config with `roots: packages/domain/src/*` gets
one module per entity, so a `shared/` and a `calcs/` under the same entity are
*inside* each other. Nothing here picks a depth the config did not.

**Specs get a column of their own.** A spec is never a row: nothing imports one
by design, so a row for it is a phantom dead file in every folder.

As an *importer* it is neither counted with the rest nor dropped, and both
halves were learned from being wrong.

Counting a spec as an ordinary importer destroys the signal — a file's own spec
sits in the same module, so every tested file reads as used from inside *and*
outside at once. On one repository that turned six `shared/` folders, all used
only from other modules, into six marked "both".

Dropping it is worse, and only a mocks convention shows why: a mock is imported
by specs and nothing else, so 43 of 44 `mocks/` folders reported **"nothing
imports any of it"** — the opposite of the truth, with a verdict attached.

So specs are counted apart, and two rules follow from it:

- **A file's own spec never counts.** It exists because the file does and
  always sits in the same module; letting it count would mean no tested file
  could ever be reported as used only from outside.
- **Another file's spec does.** A mock reached from `plan/calcs/to-json.spec.ts`
  is genuinely used by the module it sits in, so the "boundary is drawn
  elsewhere" verdict is **withheld** — the counts still print, the conclusion
  does not.

**This is not Knip.** Knip finds exports nobody uses. The question here is where
the importers come from for the exports that *are* used. The "nobody" column
does overlap; the other two are what this exists for.

It resolves the whole repository, so it costs about what a `check` costs.

## Suppressing one finding, with a reason

```ts
// archwarden-allow: the vendor SDK ships no types, tracked in ARCH-412
import { Widget } from '@vendor/sdk';
```

The marker governs **the line after it**, and only that one. Naming a rule
narrows it further:

```ts
// archwarden-allow ui-forbids-domain: one screen, being deleted in Q3
```

**No reason, no suppression.** `// archwarden-allow:` with nothing after the
colon is not a marker — it is a comment, and it suppresses nothing.
`// eslint-disable-next-line` with no explanation is how debt becomes
invisible, and a suppression that hides itself is worse than the violation it
hides.

**A suppressed finding is never absent from the report.** It appears as its own
line, with its reason, in every format, and the count is on the summary line:

```
1 finding allowed on purpose:

  packages/ui/button.tsx · ui-forbids-orders — the vendor SDK ships no types

0 errors, 0 warnings, 1 allowed · 3 files, 5 directories · 1 parsed
```

A run with forty suppressions must not look like a clean run at a glance. A
number that only ever goes up, visibly, is one somebody eventually acts on.

### What it can and cannot reach

**Only findings that point at a line.** A marker governs the line below it, so
a finding with no line has nothing to sit above. Today that means
`import-boundary` findings — a forbidden import, a forbidden package, an
import outside an allowlist — and nothing else. `structure` reporting a folder
that should not exist, `presence` reporting a missing file, `import-cycle`
reporting a loop: none of these has a line, and some never could.

This is a limit worth knowing before you reach for the feature rather than
after. It is also the case the feature was asked for: the request it answers is
*"a way to tell it to skip the next import line"*.

**Only files archwarden parses as code.** The marker lives in a comment, so a
`.md` or a `.json` under a `presence` rule has nowhere to put one.

### This is not `baseline`, and the difference is the promise

| | means |
| --- | --- |
| `baseline` | *this repository has this debt today* — a committed file, reviewable as a diff, that shrinks |
| `archwarden-allow` | *this line is a deliberate exception* — with the reason, where the next reader finds it |

Reach for `baseline` to adopt archwarden on an existing repository, and for a
marker when one line is genuinely an exception and will stay one. Using a
marker for a legacy module means editing hundreds of files; that is what
`baseline` is for.

## `governance` — is every file somebody's responsibility?

```json
{ "version": 0, "governance": "closed", "rules": [ ... ] }
```

Every file no rule governs becomes a finding. Absent means `open`, which is
what every config written before this field means and still means.

`config coverage` reports the gap; this is the gate. Read that section first —
nobody should turn this on before seeing what it would cost.

**`ignore` is the escape hatch, and gains a meaning it did not have.** An
entry there stops meaning *merely unchecked* and starts meaning **deliberately
outside the architecture** — a decision somebody wrote down, in a file
reviewable as a diff.

**One finding per file**, not per directory. `baseline` accepts a finding by
rule *and path*, so a grouped finding would keep matching as new ungoverned
files appeared under it — an escape hatch that silently swallows tomorrow's
debt, which is the shape archwarden refuses everywhere else. The grouped view
is `config coverage`, which is a report rather than a record.

Findings report under the rule id `governance`. A rule of your own may not take
that id: `arch.baseline.json` keys on rule and path, so the two would be
indistinguishable there, and the config is refused where the author is looking.

### Turning it on

Two ways, and they suit different repositories:

```json
{ "governance": { "mode": "closed", "level": "warning" } }
```

The long form carries a level. A repository with two thousand ungoverned files
can close the architecture today at `warning`, watch the number in CI without
blocking anyone, and bring it to `error` when it reaches zero. Writing
`"closed"` on its own is `error`, because a gate that does not fail a build is
a report.

The other way is `archwarden baseline`, which accepts today's gap in one
commit and fails on anything new. That produces a two-thousand-entry committed
file, which is honest and is a large diff. Neither is more correct.

**A preset cannot set it.** The same reasoning that stops a preset setting
`root`, one step stronger: closing the architecture says every file *here* is
somebody's responsibility, and a shared package cannot know what is in a tree
it has never seen. A preset that could turn it on would fail a build over files
its author never heard of.

## Config validation commands

Four commands cover the config itself:

- `archwarden config validate` — schema-only. Fast. Fails on structural JSON errors.
- `archwarden config doctor` — semantic. Answers "does this config mean what
  you think?", where `validate` only answers "does it mean anything?".

  Three of the checks originally listed here — duplicate rule `id`s, `disable`
  naming a rule that does not exist, and a preset declaring `root` — are **hard
  errors** when the config loads, not doctor findings. That is strictly better:
  a typo fails where the user is looking, rather than in a command they may
  never run.

  Answerable from the config alone:
  - unreachable rules (scope fully covered by an `ignore` entry),
  - `skip_dirs.scope: "walk"` coexisting with `import-boundary` rules,
  - `spec-pair` targeting a subfolder the corresponding structure rule forbids,
  - a `signature_hint` written in a style the rule's `kind` does not accept.

  Answerable only against the repository:
  - regexes that never match any file,
  - scopes pointing to non-existent paths,
  - `call-obligation` naming a symbol that no file in scope imports,
  - files targeted by a `naming` rule that export only a default.

  Every finding carries a code, a sentence, and a fix. The command exits 0 even
  with findings: they are advice about a configuration, not findings about
  code, and a non-zero exit would put a deliberate choice into a CI gate.
- `archwarden config explain <rule-id>` — lists every path the rule currently
  covers and every one it currently flags, one line each. This is the compact
  answer to "which paths did that rule flag?" after a `--summary`.
- `archwarden config coverage` — which files **no rule governs**, grouped by
  directory.

  ```
  $ archwarden config coverage
  1843 of 2800 files are governed by no rule

    packages/legacy/**            412 files
    apps/admin/src/screens/**     280 files
    scripts/*                      94 files

  A `**` line is one rule away from covered. A `*` line already has a rule
  beside it, so look at what the two would each catch.
  ```

  The other three ask **per rule**: is this rule broken, does it bite, what
  does it cover. This one asks **per file**, and it is the only one that can
  answer *"what is nobody watching?"* — a file no rule mentions appears in no
  rule's answer, and `check` reporting `0 errors` over it reads exactly like a
  file that satisfies everything. `CONFIG.md` calls a rule enforcing nothing
  the worst failure a linter has; this is that sentence one level up.

  **Governed means a rule would evaluate the file**, decided by the same code
  `check` uses to pick a file's rules — so this report cannot disagree with the
  checker about what is covered. One consequence is worth stating: a `presence`
  rule governs a *directory* and claims no file, so a file dropped into a
  directory only a `presence` rule governs is reported here. That is right. A
  `presence` rule does not object to a file you add.

  **The grouping is the report.** Per file it would be a thousand paths and
  nothing to do; per directory it is one rule to write. A `**` line is a
  directory where *everything* below is ungoverned, so one rule covers the lot.
  A `*` line is a directory holding both kinds, which is a different decision:
  there is already a rule there, and the question is what it does not catch.

  It exits 0 always. The number is worth having before anyone is asked to act
  on it, and nobody should have to enable a gate to find out what it would
  cost.

`archwarden describe` asks the same question from the other end. Given a glob
rather than a path, it answers for every path that matches:

```
$ archwarden describe 'packages/domain/src/*'
Rules that apply under `packages/domain/src/*`:

  packages/domain/src/invoice  domain-entity-shape
  packages/domain/src/order    domain-entity-shape, calcs-need-spec

2 paths, 2 rules.
```

Only paths that exist, necessarily — a glob can match nothing else. Asking
about a single path still answers for one that does not exist yet, which is
most of what `describe` is for.

## Minimal config

The smallest useful config:

```json
{
  "$schema": "https://raw.githubusercontent.com/HenriqueArtur/archwarden/main/schema/v0.json",
  "version": 0,
  "modules": [
    {
      "id": "src",
      "rules": [
        {
          "type": "spec-pair",
          "id": "src-needs-spec",
          "level": "error",
          "roots": ["src/**"],
          "subfolders": ["."]
        }
      ]
    }
  ]
}
```
