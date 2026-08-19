# Changelog

Notable changes to archwarden, in the format of
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versions before `0.6.0` are not reconstructed here. Their history is in the
[GitHub releases](https://github.com/HenriqueArtur/archwarden/releases) and, in
more detail than the releases carry, in the body of each `chore(release)`
commit — `git log --grep '^chore(release)'`.

archwarden is pre-1.0: minor versions carry new behaviour, patch versions carry
fixes, and neither is a stability promise. The distinction that matters more
than semver is called out explicitly below whenever it applies — **a change
that makes an existing, unchanged config report differently** is the one that
breaks someone's build, and it is listed first in its release with a note
saying so.

## [Unreleased]

## [0.27.0] — 2026-08-19

The decision as a document, and a workflow that fails instead of hanging.
**No existing configuration reports anything new, and nothing moves out of the
config.**

### Added

- **`archwarden decisions`, the document archwarden writes and you edit**
  (#116). A decision's reasoning is three paragraphs, and JSON has no comments
  (decision 5). `why` is one string and `link` points somewhere archwarden
  cannot read, so a team either wrote a sentence where a page was needed or
  kept the real ADR where nothing joined it to the rules.

  ```
  $ archwarden decisions
    + .archwarden/decisions/ADR-031.md

  wrote 1 document, updated 0. The region between the `archwarden:yours`
  markers was kept.
  ```

  **Not two owners — one owner and a rendering with a hand-written region.**
  Everything the config knows is generated; one marked region belongs to
  whoever opens the file and survives every regeneration. It is the shape
  `schema/v0.json` and `check-schema` already use.

  **Nothing left the config.** The question the issue held open — whether the
  long `why` moves out — dissolved while designing the generator: there is no
  duplication if the generated half *contains* the `why`. The region is for
  what the config cannot carry, which is new space rather than moved space, so
  an existing config is untouched.

  `--dry-run` says what would change and writes nothing, the shape
  `baseline --dry-run` already has. Both exit clean.

- **`decision-document-out-of-date`** in `config doctor`, at `warning`. A
  document that has fallen behind is a file telling a reader something the
  config no longer says — advice, not a gate: a team adopting this
  incrementally must not get a red build because a file needs regenerating.

- **Every CI and release job has a `timeout-minutes`** (#121). Cutting v0.26.0
  produced a release where four binaries built in about two minutes each and
  the fifth sat **21 minutes** in `apt-get install musl-tools` — that runner's
  mirror, not this repository. With no timeout anywhere, GitHub's default let
  it sit for six hours, silently, with the release blocked.

  Fifteen minutes on every job, ten on the two publish jobs, chosen from six
  runs of measurements: the slowest job that exists is coverage at **57
  seconds**, and there is no mutants job in CI at all — those run in the
  pre-push hook. A timeout does not make apt faster; it turns twenty-one
  minutes of silence into a red job with a re-run button.

  No automatic retry, on `RELEASING.md`'s own argument about 0.9.1: two runs
  publishing one version is the failure that document spends a page on.


## [0.26.0] — 2026-08-18

The two fields an ADR has and a decision did not: the options that lost, and
what replaced this one. **No existing configuration reports anything new.**

### Added

- **`alternatives`, what was considered and rejected** (#114). Every entry in
  this repository's own `docs/DECISIONS.md` has one, and the config could not
  hold it — which is how the losing option gets proposed again, by the next
  person or by an agent that reads the rules, complies, and helpfully suggests
  the thing that was already tried.

  ```json
  "alternatives": [
    { "option": "an HTTP client in the domain",
      "why_not": "a consumer would inherit our transport",
      "refused_by": "domain-forbids-http" },
    { "option": "a shared kernel",
      "why_not": "it becomes the place everything goes" }
  ]
  ```

  It reaches the place it is worth most — a denial, a finding, and `describe`:

  ```
  decision: ADR-031 — the domain does not know about transport
    `an HTTP client in the domain` was considered and rejected:
      a consumer would inherit our transport, and the retry policy with it
  ```

  **`refused_by` points at a rule you already wrote and never generates one.**
  `baseline` keys on rule ids, and an id derived from this prose would orphan
  accepted debt the day somebody reworded the sentence. What the reference buys
  is the distinction every surface now draws: an option with a rule is
  mechanically refused, one without it is written down while nothing stops
  anybody taking it, and `config explain` and the page say which is which.

  `why_not` is required. An option with no argument against it is a name nobody
  can disagree with — the shape `archwarden-allow` already takes.

- **`supersedes`, and the chain drawn both ways** (#115). `status:
  "superseded"` said a decision was replaced and could not say *by what*, which
  is the one thing a reader who finds it needs.

  ```json
  { "id": "ADR-031", "title": "the new way", "supersedes": "ADR-009" }
  ```

  Written on the new decision, where the author already is; the reverse is
  computed. `config explain ADR-009` now answers *and what now?* —
  `ADR-009 (superseded by ADR-031)`.

- **The status comes with the edge.** A decision another one supersedes is
  superseded, and does not repeat it. Somebody who writes `supersedes` and
  forgets to go and edit the old decision used to leave a config that said two
  things — and silently disarmed `superseded-decision-still-enforced`, which is
  the check with the most value here. Writing the contradiction is refused
  where the config compiles; writing it out in agreement is fine.

- **`config doctor` names the replacement.** The check that already existed can
  now say what to do instead of posing a dilemma:

  ```
  error  superseded-decision-still-enforced
         decision `ADR-009` is superseded by `ADR-031`, and 1 rule still
         enforces it: `domain-forbids-http`
    fix: point those rules at `ADR-031`, or the config renamed a decision
         rather than replacing it
  ```

### Reasoning

Issues #114 and #115 carry both decisions, taken before the code was written.
A second doctor check was built for #115 and **deleted**: *"the new decision
has no rules while the old one still does"* fires under exactly the condition
the existing check does, so it was one mistake reported in two voices. One
check, saying more, is the version that shipped.


## [0.25.0] — 2026-08-18

The first half of making a decision the unit rather than a footnote on a rule.
**No existing configuration reports anything new**, and nothing new fires: what
changes is what three surfaces are able to say.

### Added

- **The baseline, attributed to the decision it belongs to** (#112). A decision
  could be `accepted`, be named by three rules, report **zero findings today**,
  and still be one this repository had never kept — because all of it was in
  `.archwarden/baseline.json`. The page said *Enforced by two rules* and
  stopped there, which reads as *kept*.

  Both halves already existed and had never been joined: the baseline records
  what was accepted by rule and path, and every rule has carried `decision`
  since 0.21. **No new config field.**

  `config explain <decision-id>` now says it:

  ```
  Implemented by 2 rules:
    [error] domain-forbids-infrastructure (import-boundary) — flags 68 paths, 68 excused
    [error] no-orm-in-domain (import-boundary) — flags 19 paths, 19 excused

  87 paths break it, and the baseline excuses all of them.
  It has never refused anything.
  12 accepted entries no longer occur — run `archwarden baseline` to update.
  ```

  It is the only honest measure of whether an ADR is real, and the one number
  here with a direction.

- **`summary.baseline.by_decision`** in `--format json`, carrying `accepted`
  and `gone` per decision. Absent when no rule names one. Debt from a rule with
  no decision stays in the totals and belongs to nobody.

- **The decision card carries its debt**, on the HTML page and in the markdown
  digest an agent is handed. Counted off the committed file rather than a run —
  the digest does not walk anything — so it says *"The baseline carries 2
  entries against it"* rather than claiming they are all still excusing
  something. `config explain` runs a check and can tell those apart; this
  cannot, and does not claim to.

- **`baseline --dry-run` names the decision new debt is added against** (#113):

  ```
  + domain-forbids-infrastructure packages/domain/src/order/repo.ts — imports `axios`
      against ADR-014 — the domain does not know infrastructure
  ```

  Under the addition and only the addition: the removals are the cheerful half
  and already read well. A rule naming no decision adds nothing to the line,
  which is every configuration written before 0.21.

### Reasoning

Issue #112 records why the verdict lives in `config explain` rather than in
`config doctor`, and it was decided while building: the doctor walks and parses
but **never resolves imports and never builds a graph**, and putting this there
would have handed it the import graph — the one cost `RULES.md` singles out and
that decisions 21 and 25 made opt-in.


## [0.24.1] — 2026-08-17

One fix, and it is about the format the documentation tells a tool to use.

### Fixed

- **`check --format json` wrote text after the document** (#110). With a
  `.archwarden/baseline.json` present — which is every repository that adopted
  archwarden after its code existed — the line `N accepted` was written to
  **stdout, past the closing brace**, so stdout was not JSON:

  ```
  $ archwarden check --format json | python3 -c "import json,sys; json.load(sys.stdin)"
  json.decoder.JSONDecodeError: Extra data: line 263 column 1
  ```

  `AGENTS.md` tells an agent to use this format *instead of* parsing the prose,
  so the path the documentation calls the tool path was the broken one.

  **The numbers moved into the document** rather than to stderr, which is the
  half of the fix worth stating. A baseline nobody is reminded of is a
  suppression file, and `gone` — accepted entries that no longer occur — is the
  only cheerful number archwarden has as well as the thing that stops a stale
  entry hiding a violation that came back. Sending that to a stream CI throws
  away would have fixed the parse and lost the point:

  ```json
  "summary": { "baseline": { "accepted": 78, "gone": 12 } }
  ```

  Absent when the repository has no baseline, so a consumer can tell "accepts
  nothing" from "nothing accepted" — the distinction `summary.imports` already
  draws. `REPORT_VERSION` stays 0 on `duration_ms`'s precedent: a consumer that
  ignores the field reads the report exactly as before.

- **And the second writer, one line above it in the same function.**
  `check --format json --html page.html` also wrote `page written to page.html`
  to stdout. The issue did not name it and it is the same defect, so it went in
  the same fix: under `--format json` it goes to stderr, where the failure half
  of that same write already went. The rule is now one sentence — in
  `--format json`, stdout is the document and nothing else — and `AGENTS.md`
  says it.

  **The text format is untouched**, deliberately: same lines, same order, same
  stream. The report *is* prose there, and a note beside it is part of what
  somebody at a terminal reads.


## [0.24.0] — 2026-08-17

What a file says about itself: ownership, stability and lifecycle, declared in
the file they are about. **No existing configuration reports anything new.**

### Changed — this changes nothing a config reports, and costs one cold run

- **The cache format is 8.** `FileFacts` gained a field, so entries written by
  0.23 are discarded rather than misread. You lose one warm cache and pay one
  cold run. Misreading them would have been the expensive failure: an old entry
  deserialises cleanly and claims the file declares nothing about itself, which
  is a finding against a file whose `archwarden-owner` is right there in the
  header.

### Added

- **`metadata`, the fourteenth rule kind** (#104). `frontmatter` asks a
  *document* to declare things about itself, and code had no equivalent —
  ownership, stability and lifecycle are ordinary ADR content and were
  properties of a file no rule could ask about.

  ```json
  { "type": "metadata", "id": "payments-declares-an-owner", "level": "error",
    "roots": ["src/payments/**"],
    "require": ["owner"],
    "one_of": { "stability": ["stable", "experimental", "deprecated"] },
    "why": "ADR-031: a module without an owner is a module nobody reviews" }
  ```

  and the file that satisfies it:

  ```ts
  // archwarden-owner: payments-team
  // archwarden-stability: experimental

  import { db } from './db';
  ```

  **Its own prefix, not a JSDoc tag.** `@internal` and `@deprecated` already
  mean something to `tsc`, to editors and to TypeDoc, and a marker with two
  readers eventually has two interpretations. It also puts these in the same
  family as `archwarden-allow`, so a `grep` for `archwarden-` finds everything
  archwarden reads out of a comment.

  **The shape is `frontmatter`'s, deliberately.** `require`, `one_of`,
  `equals`, values compared as text, `{{raw(dirname)}}` in an agreement. Two
  kinds asking the same question of two file formats should look the same.

- **A marker below the header is reported, never treated as absent.** Claims
  are read from above the file's first statement; one written lower down is a
  finding of its own, pointing at the line it is on. Telling an author who
  wrote `archwarden-owner` that the file declares no owner is the one answer
  nobody can act on.

- **The same key twice is reported, with both values.** Picking a winner in
  silence would make which one wins something an author has to know by heart.

- **A rule asking for an unreachable key is refused where the config loads.**
  The suppression grammar reaches every key beginning with `allow` first —
  `// archwarden-allow: reason` is a suppression and never a claim — so a rule
  asking for one could never be satisfied by any file.

### Reasoning

Decision 30 in [`DECISIONS.md`](docs/DECISIONS.md), including why this is a
fact of its own rather than a widening of `allowances`, why the kind is called
`metadata` when the issue called it `annotation`, and why the header-only
reading is what keeps the per-export version possible.


## [0.23.0] — 2026-08-14

Two rules the config could not write: a directory that has stopped growing, and
a counterpart in a parallel tree. **No existing configuration reports anything
new.**

### Added

- **`frozen`, a directory closed for extension** (#102). `import-boundary`
  could forbid *importing* something; nothing could forbid *adding* to it —
  which is half of every migration ADR.

  ```json
  { "type": "frozen", "id": "legacy-is-closed-for-extension", "level": "error",
    "roots": ["packages/legacy/**"],
    "why": "ADR-021: closed for extension; new work goes in packages/core" }
  ```

  The engine is the smallest in the workspace, and that is the design rather
  than a shortcut: every file under the scope is a finding, and `baseline`
  already decides which are accepted, by rule and path. The rule points that
  machinery forward instead of back — and turns `baseline` from a record of
  debt into a statement of intent.

  **Turning one on is two steps.** `archwarden baseline` accepts what is there
  today; skip it and the first `check` reports every file that was already
  there. `config doctor` names that as `frozen-with-nothing-accepted` and gives
  you the command.

  A move *within* the freeze is reported and a move *out* is silent, which is
  the point of the freeze. Nothing reads `git`: a freeze that consulted history
  would answer differently in CI than on a laptop.

- **`mirror`, a counterpart in a parallel tree** (#103). `pair` and `spec-pair`
  both look in the same directory, and `pair` takes a sibling *name* — so
  *"tests live in `test/`, mirroring `src/`"* was inexpressible.

  ```json
  { "type": "mirror", "id": "entities-have-migrations", "level": "error",
    "roots": ["src/entities"], "file_pattern": "^(?<name>[a-z-]+)\\.ts$",
    "must_exist": "migrations/{{raw(name)}}.sql" }
  ```

  One direction per rule, so *"every entity has a migration"* and *"every
  migration belongs to an entity"* each carry their own `why`. Only that the
  counterpart **exists** is checked — whether it has anything in it is
  `spec-pair`'s question.

- **`{{raw(subpath)}}`**, the directory path from a rule's root down to the
  file. `test/{{raw(subpath)}}/{{raw(name)}}.test.ts` turns `src/a/b/x.ts` into
  `test/a/b/x.test.ts`, which `dirname` could not carry. Empty for a file
  directly in a root, with the separator it would leave collapsed.

- **`frozen-with-nothing-accepted`** in `config doctor`, at `warning`.

### Reasoning

Decisions 28 and 29 in [`DECISIONS.md`](docs/DECISIONS.md), including why a
move within a freeze is reported, why nothing reads `git`, and why `pair` and
`spec-pair` stay rather than collapsing into `mirror`.

## [0.22.0] — 2026-08-14

What a file exports, without tying it to what the file is called.

### Changed — this changes what an existing config reports

- **`export * from './x'` is now seen by `no-passthrough`** (#101). It produced
  no fact at all, so the rule against a file that adds nothing of its own was
  silent about the loudest form of exactly that — while catching
  `export { A } from './x'` all along. That was a defect, not a missing
  feature.

  **A repository with a `no-passthrough` rule and star barrels gets findings on
  its first 0.22 run that 0.21 never produced.** `baseline` is the answer for
  anyone not paying that debt today.

  The blast radius was measured rather than assumed, and it is narrower than it
  sounds: `allow_package_entrypoints` is on by default and a star barrel is
  overwhelmingly written in a file called `index.ts`, which was exempt before
  and stays exempt. What lands is a star barrel under some other name.

- **The cache format is 7.** `ExportFact` gained a field, so entries written by
  0.21 are discarded rather than misread. You lose one warm cache and pay one
  cold run.

### Added

- **`export-shape`, the eleventh rule kind** (#101). `naming` couples what a
  file exports to what the file is *called*, and plenty of decisions are about
  the export alone — *"we do not use default exports"*, *"one export per
  file"*, *"every use case returns the pattern"*. None mentions a filename, and
  saying any of them meant inventing a naming claim you did not mean.

  ```json
  { "type": "export-shape",
    "id": "use-cases-return-the-pattern",
    "level": "error",
    "roots": ["src/use-cases/*"],
    "forbid_default": true,
    "max_exports": 1,
    "must_return": ["^ResponsePattern<.+,.+>$"],
    "why": "a use case returns the pattern, it never throws" }
  ```

  `max_exports` counts what exists at **runtime**: `type` and `interface` do
  not count, and the default counts as one. A file exporting a function and the
  interface of its dependencies is idiomatic TypeScript.

- **The guarantee `must_return` is, said exactly.** archwarden requires that a
  function **declares** its return type; whether the body conforms stays
  `tsc`'s question. It is worth having because `tsc` checks what is annotated
  and *cannot require that you annotate at all* — a function returning
  `{ ok: true }` with no return type compiles perfectly.

  It matches text against text, so an alias under a different name is a
  different string. The field takes a list for that reason, and pairing it with
  `import-boundary.must_import_from` closes the remaining hole. `RULES.md` says
  both beside the field.

- **`ExportFact.returns`**, a field of its own beside `annotations`. An
  annotation says *what this value is*; a return type says *what this call
  gives you*, and a rule asking for one must not be satisfied by the other.

### Reasoning

Decision 27 in [`DECISIONS.md`](docs/DECISIONS.md), including why the returned
object literal is deliberately never inspected, and why the star-export fix
belongs to `no-passthrough` rather than to a fourth claim here.

## [0.21.0] — 2026-08-14

Decisions. The config can name the choices its rules keep, and every surface
says so. **No existing configuration reports anything new** — this release
changes what archwarden *says*, not what it checks.

### Added

- **`decisions`, and a `decision` on every rule** (#100). A rule could say why
  it exists (#46); nothing said *what decision it implements*, which is the
  difference between a config that enforces an architecture and one that
  describes it.

  ```json
  {
    "decisions": [
      { "id": "ADR-014", "title": "The domain does not know about transport",
        "why": "it is published, and a consumer must not inherit our HTTP client",
        "link": "docs/adr/014-domain-transport.md", "status": "accepted" }
    ],
    "rules": [
      { "type": "import-boundary", "id": "domain-forbids-http",
        "decision": "ADR-014", "level": "error", "from": ["packages/domain/**"],
        "forbid_import_from_packages": ["axios"] }
    ]
  }
  ```

  The rule points at the decision, not the other way round: there is no second
  list to keep in step, a deleted rule leaves nothing dangling, and a rule that
  forgets its decision is visible where it is written. Declared at the top level
  only. Every rule kind takes the field, so the kinds landing after this one
  carry it from birth.

- **What each surface now says.** The pre-write hook's denial stops being
  *"breaks `domain-forbids-http`"* and becomes *"breaks ADR-014, and here is why,
  and here is where it is written"*. `describe` answers with the decision each
  rule serves. `agent-guide` opens with the decisions and the rules that keep
  each one. The HTML page leads with the architecture as decisions rather than a
  rule table. MCP's `check_write` names the decision a refusal breaks — and
  gained the rule's `why` alongside it, which it had never carried.

- **`config explain` takes a decision id**, not only a rule id, and answers the
  question people actually ask: not *what does this rule do* but *why is this
  like this* — plus the half a document cannot answer, which is whether the
  decision is still being kept. An id may not be both a rule and a decision;
  that collision is refused when the config loads.

- **Three checks in `config doctor`.** `rule-without-a-decision` at `warning`,
  counted in one line and only once some rule names one. `decision-nobody-
  enforces` at `warning`. And `superseded-decision-still-enforced` at `error`,
  which is the check most worth having: a decision recorded as replaced with
  rules still enforcing it is a config saying two things at once.

- **Presets ship decisions**, folded the way rules are folded. A preset stops
  being a bag of rules and becomes a set of opinions with names and reasons.

### Changed

- **`config doctor` prints a level on every concern.** The sixteen checks that
  came before are all `warning`, which is what they have always been in
  practice. It does not reach the exit code — `doctor` is advice and `check` is
  the gate.

- **`config explain`'s not-found message** says *"nothing is called `x`"* rather
  than *"no rule is called `x`"*, and lists the declared decisions as well as
  the configured rules. A user who mistyped does not know which of the two
  namespaces they got wrong.

### Reasoning

Decision 26 in [`DECISIONS.md`](docs/DECISIONS.md), including why the foreign
key points from the rule, why a dangling reference is refused at compile while a
*missing* one is only ever a `doctor` warning, and why `proposed` is reported by
nothing.

## [0.20.0] — 2026-08-14

Authoring. Asking before you write a rule, and being able to write the rule you
mean. **No existing configuration reports anything new.**

### Added

- **`config options`, on the CLI and over MCP** (#97). *"What does a rule of
  kind X take?"* had no answer: `describe` and `scaffold` are about paths,
  `config explain` is about a rule you already declared, and `agent-guide` is a
  digest of the ones you have. The reported workaround was reaching into
  `node_modules/archwarden/schema/v0.json` and chasing `$defs` by hand — and
  over MCP there was no workaround at all, because a client has no
  `node_modules` to read.

  It answers about the config's **own keys** as well as the ten rule kinds:
  required fields, what each means, defaults, and a rule to paste. Everything
  but the examples is generated from archwarden's own types, so it cannot
  describe a shape the binary would refuse. It answers with **no configuration
  present**, which is the moment it is asked.

- **`when_importing`, on every rule kind whose population it can mean anything
  for** (#98). A rule's files were where they sit and what they are called.
  Some obligations are about neither: *"every write goes through the request
  helper"*, in a repository where reads and writes are deliberate siblings
  because erasing the transport from the filename was the point of the design.

  ```json
  { "type": "call-obligation", "roots": ["services/api/Entities/*"],
    "when_importing": "services/api/Http/connection.ts",
    "must_call": { "symbol": "HttpRequest", "imported_from": "../../Http/request" } }
  ```

  **Opt-in, including in cost.** A rule that names no imports resolves nothing
  and behaves exactly as it did. A rule that names them turns resolution on for
  the files its scope reaches, and no further.

  For a rule about a **directory** — `presence`, `structure` — it means *some
  file inside imports it*, which is the only reading of the axis that is ever
  both true and false there. Those two lose their walk-only status when they
  narrow, and that is the largest cost here. Decision 25 has the argument.

  `import-boundary` does not get it: it already chooses its importers with
  `from`, `from_module` and `from_kind`.

### Fixed

- **`agent-guide --kind <kind>` no longer says a repository has no rules when
  it has nine** (#97). Three states shared one sentence, and one of them was
  false — it read as *this kind does not exist* rather than *you have none of
  it*. Each says its own thing now, and the kind case points at
  `config options`.

## [0.19.0] — 2026-08-13

One repository, two roots. **No existing configuration reports anything new**,
and nothing is configured for this at all.

### Fixed

- **A harness and archwarden can now disagree about where the repository is**
  (#93, #95). A harness on the host sends `/home/dev/proj/src/x.ts`; an
  archwarden inside a container has `/app` as its root; the answer used to be
  *outside the repository* — correct, and useless. Every hook was dead in that
  setup, and the only symptom was a message saying the write was not checked.

  Both surfaces were already being told where the caller stands and neither was
  reading it: every hook payload carries `cwd`, and an MCP client answers
  `roots/list` (it advertises `roots: { listChanged: true }`, and archwarden now
  asks). So the mapping is **derived, never configured** — which is also the
  only thing that could work, since the host root differs per developer and a
  committed config file cannot carry it.

  **The wrapper stays; the `sed` inside it goes.** A harness runs a process on
  the host, so something still has to reach into the container — that half is
  inherent, and 0.18.1 is what tells you so. What archwarden owed was the other
  half:

  ```diff
  - sed -e "s#$CLAUDE_PROJECT_DIR/#/app/#g" \
  -   | docker exec -i -w /app "$CONTAINER" ./node_modules/.bin/archwarden hook claude-code
  + docker exec -i -w /app "$CONTAINER" ./node_modules/.bin/archwarden hook claude-code
  ```

  **A translation has to earn itself.** A path is re-rooted only when something
  on this side stands under the result, so a wrapper pointed at a container
  holding a different project is refused rather than judged against the wrong
  rules — a quiet wrong answer in place of a loud useless one is the trade this
  refuses. When it refuses, it names both roots. Decision 24.

  The cost of that guard is stated rather than discovered: a path in a
  directory that does not exist on this side yet is refused too, so the first
  file of a brand-new module in a container setup comes back *"did not check
  this write"* rather than judged. It fails to the safe side — that message is
  never approval — and it is the trade the guard makes.

## [0.18.1] — 2026-08-13

### Fixed

- **`install-hooks` no longer installs a command in silence that the harness
  may not be able to run** (#93). The command written is the one that works
  *where the installer ran*, and a harness runs it as its own process
  somewhere else — the same machine, until it is not. A project whose
  dependencies live only inside a container installs
  `./node_modules/.bin/archwarden`, hands it to a harness on the host, and the
  hook is dead: every write comes back *"archwarden did not check this
  write"*, which is not approval and reads like one.

  The installer now says the harness has to be able to run that command, and
  says it sharply in the one case it can recognise — running inside a container
  while installing a command that names a path inside it.

  It does not make the container case work on its own. A wrapper still has to
  reconcile the two roots, because a harness on the host sends absolute host
  paths and archwarden inside the container has a different one. That half is
  its own piece of work and is tracked separately; this release stops the
  failure being silent.

## [0.18.0] — 2026-08-13

Three surfaces onto the same operations, and the boundary corrected so they
really are the same ones. **No existing configuration reports anything new.**

### Added

- **An MCP server, over stdio** (#65). `archwarden mcp` speaks JSON-RPC on its
  pipes — no port, no daemon, nothing listening — and `install-hooks
  --claude-code` now writes a committable `.mcp.json` beside the hooks, naming
  the same binary they do. The tool that earns it is `check_write(path,
  content)`: it existed already and was reachable only through the pre-write
  hook, which means only *reactively* — the agent writes, and is denied.
  Through MCP it can ask **would this pass?** before writing. `describe` and
  `scaffold` are there too. The server re-reads the configuration on every
  call, because a long-lived process that prepared its rules at startup would
  answer from a config the user has since edited.

- **A `SessionStart` hook that injects the module map** (#66). Installed by
  `install-hooks --claude-code`, with **no matcher** — it fires on a new
  session, a resume, a clear, a fork, and after compaction, which is the case
  it exists for and the one where the rules leave the context with nobody
  noticing. What goes in is a pointer rather than the guide: the module names,
  the sentence each author wrote about theirs, and the commands that answer the
  rest. A long block is the first thing compaction drops.

- **A programmatic binding, for architecture claims that live in the test
  suite** (#73). `import { check } from "archwarden"` runs the binary and hands
  back findings for the test framework to assert on — no fluent DSL, because
  that would be a second way to express a rule and a second thing that can
  drift from the first. It reads the repository's own `arch.config.json`,
  filtered by `rules`, `paths` or `level`, so the rules stay declarative and in
  one file and the test asserts a subset of them. Types ship with it.

  A rule id no rule has is an error rather than an empty result: a typo that
  came back clean would be a test that passes for the wrong reason, and goes on
  passing after the rule is deleted.

### Changed

- **The agent-facing operations moved into `archwarden-api`** — `describe`,
  `scaffold`, the `agent-guide` digest, the module map, and the whole judgement
  of a write rather than the engine call inside it. Decision 20 said the crate
  held the operations every surface goes through; it held the ones `check`
  needs, because `check` was the only surface when it was written. Decision 22
  records the correction and what it cost. Nothing a user runs behaves
  differently.

- **MCP is its own crate**, depending on `archwarden-api` and unable to see
  `archwarden-cli`. The binary is still one. A rule that holds because nobody
  has broken it yet is not holding — the workspace denied `print_stderr` for
  years and never caught `prepare()`.

- **`cargo xtask clean` is the last step of a release**, and a `SessionStart`
  hook in this repository's own `.claude/settings.json` sweeps what a previous
  session left. 0.18 filled the disk of the machine it was built on and froze
  it; the recovery was deleting `target/` by hand to free enough space to save
  the session's transcript. `docs/RELEASING.md` says so and says why `--deps`
  is right there and nowhere else.

### Fixed

- **Every surface is now tested against a config this build cannot read.**
  Issue #55 — a future config parsing into one with no rules, compiling,
  matching nothing, and permitting every write — was covered only by unit tests
  on the *sentence* a broken config produces. A surface that grew its own
  loading path, which is exactly what #55 was, would never have called them:
  all of them would have stayed green while the gate evaporated. Each surface
  is now driven from outside the process against a version-99 repository, in a
  pair with a version-0 half that proves the surface works at all.

- Twelve rule kinds and shapes that no test reached — `no-passthrough`,
  `import-cycle`, import allowlists, `frontmatter` keys, `presence` patterns,
  folder-name constraints — are covered. The gap was invisible under the
  workspace's 95% floor and surfaced when the code moved to a crate held at 99.

## [0.17.0] — 2026-08-12

Nothing unwatched. Three answers to one question — *what is nobody
looking at?* — and the exception mechanism that makes the answer
liveable. **No existing configuration reports anything new.**

### Added

- **Inline suppression, with a mandatory reason and no way to hide it**
  ([#72](https://github.com/HenriqueArtur/archwarden/issues/72)).

  ```ts
  // archwarden-allow: the vendor SDK ships no types, tracked in ARCH-412
  import { Widget } from '@vendor/sdk';
  ```

  The marker governs **the line after it**, and only that one. Naming a rule
  (`// archwarden-allow ui-forbids-domain: …`) narrows it further.

  Three constraints, and they are the feature rather than details of it.
  **No reason, no suppression** — a marker with nothing after the colon is a
  comment. **Never silently dropped** — a suppressed finding is its own line in
  the report, with its reason, in every format, and the summary line reads
  `0 errors, 0 warnings, 1 allowed`, so a run with forty does not look like a
  clean one. **Countable** — that number only ever goes up, visibly.

  **It reaches only findings that point at a line**, which today means
  `import-boundary` and nothing else: a marker governs the line below it, and
  `structure` reporting a folder that should not exist has no line to sit
  above. Stated here rather than left to be discovered, and it is the case the
  feature was asked for — the request is *"a way to skip the next import
  line"*. It also only works where archwarden parses comments, so a `.md`
  under a `presence` rule has nowhere to put one.

  This is not `baseline` and the difference is the promise: `baseline` says
  *this repository has this debt today* and shrinks; a marker says *this line
  is a deliberate exception*, with the reason where the next reader finds it.

- **`governance: "closed"` — every file must be governed by some rule**
  ([#60](https://github.com/HenriqueArtur/archwarden/issues/60)). The gate half
  of `config coverage`: every file no rule governs becomes a finding, and
  `ignore` becomes the escape hatch with a meaning it did not have —
  **deliberately outside the architecture** rather than merely unchecked.

  ```json
  { "version": 0, "governance": "closed", "rules": [ ... ] }
  { "version": 0, "governance": { "mode": "closed", "level": "warning" } }
  ```

  Absent means `open`, which is what every config written before this field
  means and still means. **No existing configuration reports anything new.**

  The long form carries a level, for the migration the report exists for: two
  thousand ungoverned files can be closed at `warning` today, watched in CI
  without blocking anyone, and raised to `error` at zero. `baseline` is the
  other way and produces a two-thousand-entry committed file; both are honest.

  **One finding per file**, not per directory: `baseline` accepts by rule and
  path, so a grouped finding would keep matching as new ungoverned files
  arrived under it — an escape hatch that swallows tomorrow's debt. Findings
  report under the rule id `governance`, and a rule of your own may not take
  that id, because a baseline could not tell the two apart.

  A preset cannot set it, for the reason a preset cannot set `root`, one step
  stronger: it would fail a build over files its author never saw.

- **`archwarden config coverage`** — which files no rule governs, grouped by
  directory ([#59](https://github.com/HenriqueArtur/archwarden/issues/59)).

  ```
  1843 of 2800 files are governed by no rule

    packages/legacy/**            412 files
    apps/admin/src/screens/**     280 files
    scripts/*                      94 files
  ```

  `CONFIG.md` calls a rule enforcing nothing the worst failure a linter has.
  This is that sentence one level up: **a file no rule governs is
  indistinguishable from a file that satisfies every rule**, and `check`
  printing `0 errors` over it reads as *the architecture holds* when it may
  mean *half the tree was never looked at*.

  Every other config command asks per rule — is it broken, does it bite, what
  does it cover. None of them can be asked what nobody is watching, because a
  file nothing mentions appears in no rule's answer.

  Governed is decided by the same code `check` uses to pick a file's rules, so
  the report cannot disagree with the checker. Grouped because per file it
  would be a thousand paths and nothing to do: a `**` line is a directory where
  everything below is ungoverned and one rule covers the lot, a `*` line is a
  directory holding both kinds. Exits 0 always — the gate is `governance:
  closed`, and nobody should have to enable it to find out what it would cost.

### Internal

- **The mutant count is visible between a commit and a push.** `cargo xtask ci`
  now ends with a line naming how many mutants the current diff would produce
  and the command that runs them — listing costs **1.0 s for 224 mutants**,
  where running them costs seven minutes.

  0.16 was written over five commits with no push, so the pre-push hook never
  ran, and twenty-eight survivors accumulated in silence across four issues
  until a release turned them up. Nothing between a commit and a push had said
  a word.

  It is a line rather than a gate on purpose. `cargo xtask ci` is 73.7 s;
  running the mutants of an ordinary commit would add about 65% to it and of an
  accumulated branch about 570%, and a gate people run less often because it
  got slower is a gate that catches less. The block stays at push.

- **`cargo xtask mutants`** is now the whole implementation, and
  `.githooks/pre-push` is four lines that call it — down from 170. One place
  decides what an exit code means, what an empty survivor list means, and what
  a missing tool means, and the number `ci` reports cannot drift from the
  number that blocks a push. `--since <ref>` scopes the diff, which the hook
  passes as the sha the remote already has.

  This also fixes a push that broke itself: `git` opens its connection to the
  remote *before* running `pre-push`, so a fifteen-minute hook has the server
  time the connection out after every gate has passed. Releasing 0.16.0 hit it
  twice.

## [0.16.0] — 2026-08-12

The two questions one file cannot answer. Nothing an existing, unchanged
config reports has moved: both new rules fire only where somebody writes one.

### Added

- **`import-cycle`: no file in scope may sit on an import loop**
  ([#70](https://github.com/HenriqueArtur/archwarden/issues/70)).

  ```json
  { "type": "import-cycle", "id": "no-cycles", "level": "error",
    "roots": "packages/**" }
  ```

  The finding carries the whole chain — `a.ts → b.ts → a.ts`, with both ends,
  so a reader can see it closed. Breadth-first, so the *shortest* loop is the
  one reported; the shortest is not always the one to fix and it is always the
  one somebody can read. A chain longer than twelve files is not walked, because
  a forty-file loop is technically correct and useless.

  **Every file of the loop that the scope covers is reported, once each.** A
  loop has no owner. dependency-cruiser reports the closing edge, which depends
  on where its walk started, so the same cycle moves between machines. N files
  have to change or N people have to agree not to, and `baseline` accepts
  findings per rule and per path, so an accepted cycle is accepted at that same
  N. There is deliberately **no `ignored_circular_dependencies`**: a cycle is a
  finding, `baseline` already accepts findings, and a second mechanism for the
  same thing disagrees with the first the day somebody uses both.

  A file importing itself is a typo, not an architecture fault, and is not a
  loop. `include_type_only` defaults to `true` and spells what
  `import-boundary` spells: a loop of `import type` is erased at runtime and is
  still a loop the compiler walks.

- **`forbid_reaching` on `import-boundary`: the dependency nobody wrote down**
  ([#71](https://github.com/HenriqueArtur/archwarden/issues/71)).

  ```json
  { "type": "import-boundary", "id": "ui-must-not-reach-db", "level": "error",
    "from": "packages/ui/**", "forbid_reaching": ["packages/db/**"] }
  ```

  `packages/ui` does not import `packages/db`, and it depends on it through
  `packages/orders` anyway. `forbid_reaching_modules` names a declared module
  instead of repeating its globs, the way `forbid_module` already does for the
  direct form, and `except` applies to both.

  The finding carries the chain, because *"ui reaches db"* is not actionable
  and *"ui → orders → db"* names the edge to cut. A **direct** import is not
  reported here: that is `forbid_import_from`'s finding, and reporting it twice
  would make one fault look like two.

- **What these two cost, stated rather than discovered.** A rule that reads the
  import graph makes the run parse and resolve **every source file in the
  repository**, whatever any scope says — because a chain that leaves the scope
  and comes back is still a chain, and a graph built only from what the scope
  reaches would report a clean repository over a real cycle.

  Measured on a 10 000-file repository with 30 000 in-repo edges: a boundary
  rule governing one module of forty runs in **0.01 s / 8 MB**; the same scope
  with `import-cycle` runs in **0.22 s / 28 MB**. The run stops being
  proportional to the scope and becomes proportional to the repository. Holding
  the edges is the small part of that — **+5 MB for 30 000 edges** — and the
  resolution pass is the rest.

  A configuration with no graph rule pays none of it, and an `import-boundary`
  rule that leaves `forbid_reaching` empty is exactly as cheap as it was.

- **A module can declare the paths it is, and a boundary can name it**
  ([#74](https://github.com/HenriqueArtur/archwarden/issues/74)). `scope` on a
  module, `from_module` and `forbid_module` on an `import-boundary`:

  ```json
  "modules": [
    { "id": "domain",         "scope": "packages/domain/**", "rules": [ ... ] },
    { "id": "infrastructure", "scope": "packages/infrastructure/**" }
  ],
  "rules": [
    { "type": "import-boundary", "id": "domain-is-sealed", "level": "error",
      "from_module": "domain", "forbid_module": ["infrastructure"] }
  ]
  ```

  This repository's own fixture said `packages/domain/**` in a boundary and
  `packages/domain/src/*` in the rules of a module called `domain`, and
  forbade `infrastructure` — a declared module — by glob. Moving the package
  meant editing four places, and missing one made a rule stop reaching with
  nothing reporting it.

  `scope` is optional and a module without one behaves exactly as before, so
  no existing config changes. A rule inside a scoped module reaches where both
  reach; naming a module that does not exist, one with no `scope`, or saying a
  scope both ways on one rule is refused when the config compiles.

- **A `kind` on each module, so one rule governs every module wearing it**
  ([#76](https://github.com/HenriqueArtur/archwarden/issues/76)).

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

  "An assembly may not import another assembly" was one rule *per* assembly,
  each listing every other: six assemblies is six rules of five entries, and
  the seventh means editing the six. Here the seventh is governed because it
  exists with `kind: "app"`.

  One label per module rather than a list: one axis is what the case needs,
  and a list would buy composition across dimensions at the cost of a second
  vocabulary for scope. An allowlist rather than `forbid_kind`, so a kind
  invented later is refused rather than permitted by omission. **A module never
  fails this against itself** — an app may import its own files and not a
  sibling app, decided by identity rather than by the label. A kind no module
  wears is refused when the config compiles, and `config doctor` reports
  `module-wears-no-kind`.

- **`import-boundary` gained an allowlist**
  ([#75](https://github.com/HenriqueArtur/archwarden/issues/75)).
  `only_import_from`, `only_import_from_modules` and
  `only_import_from_packages`: these, and nothing else.

  ```json
  { "type": "import-boundary", "id": "api-depends-only-on-libs", "level": "error",
    "from_module": "api-orders", "only_import_from_modules": ["orders-core"] }
  ```

  A denylist decays. Every new package, app and directory is permitted by
  omission, and omission is invisible — the failure `CONFIG.md` names as the
  worst a linter has, arriving one import at a time. An allowlist refuses
  things that do not exist yet, which is the point.

  Three things sit outside it and stay allowed: the rule's own scope, because
  a file importing its neighbour is not what "only these" refuses; anything
  that did not resolve into this repository, because a builtin or a dependency
  has no path a glob could match; and packages, which have their own field for
  the same reason forbidding one does. Setting it alongside
  `forbid_import_from` or `except` is refused — "only these, except those" is
  two rules.

- **Three `config doctor` checks** that a module with paths makes possible:
  `module-scope-matches-nothing`, `module-nobody-references`, and
  `rule-reaches-outside-its-module` — the last being the one that stops
  narrowing from being silent.

### Fixed

- **A warm `check` on a shared mount spent most of its time on `stat` calls
  that found nothing**
  ([#82](https://github.com/HenriqueArtur/archwarden/issues/82)). Node
  resolution is a ladder — `./order` means try `.ts`, then `.tsx`, `.js`,
  `/index.ts` — and over half of those probes miss. On a filesystem that is
  really a network, a failed `stat` is a full round trip that returns nothing.
  One directory listing now answers every rung at once.

  Measured over 3 030 files and 15 000 import specifiers, warm, resolution
  only: **186 ms → 58 ms** on a shared mount. A local disk pays **0.8 ms
  more**, because a failed `stat` there is a page-cache lookup and the listing
  buys nothing — a small regression on the fast path, stated rather than
  rounded away.

  This is not a niche case. Docker Desktop on macOS and Windows, WSL2 with the
  repository on the Windows side, and any devcontainer with a bind mount all
  produce it.

- **`agent-guide --kinds no-passthrough` refused a kind archwarden has.** The
  list of kinds that command validates against was hand-written and had been
  missing `no-passthrough` since that rule shipped; the test guarding it listed
  five of the then-eight kinds, so it agreed. Both are fixed, and the test now
  builds one rule of every kind and checks the list in both directions.

### Changed

- **The cache format moved to 6.** `FileFacts` now carries the suppression
  markers a file holds, so a cache written by 0.16 is discarded rather than
  misread: one cold run, once.

### Fixed

- **A scalar added to the config was silently ignored after a merge.** The
  `extends` merge copied the entry config's scalars by a hand-written list, so
  a field not named there kept its *default* for every configuration in the
  world. `governance` shipped that way during development and reported nothing
  at all — the exact silence it exists to break. The list is a destructuring
  now, so the next field fails to build until somebody decides which side it
  comes from.

### Internal

- `check --file` and the pre-write hook **refuse** a rule that reads the import
  graph, under a new `needs-repository` skip reason, rather than evaluating it
  against one file. A cycle rule with no graph reports nothing, and nothing is
  what a repository with no cycles reports — a hook that let a write through on
  that basis would be approving a file it never examined.

- The benchmark gained a case that resolves imports. Every case in it ran a
  `naming` rule, which reads no imports at all, so the half of a warm run this
  release is about was never measured — which is why it went unnoticed until
  somebody timed the same repository on two filesystems.

## [0.15.0] — 2026-08-11

One implementation of every operation, and a boundary the surfaces sit on.
Nothing a rule reports moved.

### Fixed

- **The pre-write hook said a broken config was a missing one.** Every load
  failure was reported as *"no archwarden config was found from here"*, so
  someone who had just introduced a syntax error was sent looking for a file
  that was sitting there, found and unreadable. It now distinguishes the two,
  and a config that will not parse points at `archwarden config validate`.
- **`archwarden baseline` discarded a cache it could not write**, where
  `check` reported it. The two were separate copies of one sequence, quietly
  disagreeing about whether a user hears that their next run will be slower.
  Both say it now.
- **A violation fixed in the same regeneration as a directory move could
  vanish from `baseline --dry-run`.** Debt paid is the only encouraging number
  archwarden prints, and one absorbed into a rename was one nobody was told
  about.
- **A summary could reorder between runs on identical input.** Two areas with
  the same counts had no tie-break, so `check --summary --by path` produced a
  table whose row order was not stable — which makes every diff of it
  unreadable, and only shows up on somebody else's machine.

### Changed

- **A config declaring an unsupported version now renders like every other
  config problem**, with a help line saying the fix is to upgrade archwarden.
  It was a bare sentence on stderr with no help, and a user reading a
  complaint about their file's version number would reasonably edit the
  number — which makes this build parse a config written for a schema it does
  not have. Exit code and the numbers in the message are unchanged.
- **`--root` given a path outside the repository is refused with a caret and a
  help line** rather than a hand-drawn block of text. Same refusal, same exit
  code.

### Internal

No change to what any command reports beyond the entries above.

- **`archwarden-api`** ([#63](https://github.com/HenriqueArtur/archwarden/issues/63)).
  The operations every surface goes through — `Resolve → Load → Walk →
  Evaluate → Present` — move out of `archwarden-cli` into a crate of their
  own, and return their failures as values instead of writing them. Nothing in
  it writes and no function in it takes a writer, which is what lets the CLI
  turn a failure into stderr and exit 2, the agent hook into a `systemMessage`
  and exit 0, and MCP ([#65](https://github.com/HenriqueArtur/archwarden/issues/65))
  into a JSON-RPC error, without any of them re-walking the path. The pre-write
  and end-of-turn hooks carried their own copies of that sequence precisely
  because they could not answer the way it insisted on; the missing version
  guard that shipped as
  [#55](https://github.com/HenriqueArtur/archwarden/issues/55) was in one of
  them. See decision 20 in `docs/DECISIONS.md`.
- The JSON report moves with it, behind a `Renderer` trait, so the object an
  MCP server emits is the one `check --format json` emits rather than a second
  implementation of the same contract. Human text and the HTML page stay in
  `archwarden-cli`.

## [0.14.0] — 2026-08-11

The pre-write hook stops refusing writes that are fixing the thing it is
complaining about, and gains a second half for the rules one write cannot
answer.

### Fixed

- **An agent could not create a module at all**
  ([#57](https://github.com/HenriqueArtur/archwarden/issues/57)). A `presence`
  rule requiring three files made all three illegal until all three existed:
  the first write refused for the absence of the second, the second for the
  third. No write order passed.

  The write was never what was wrong. Writing `projeto.md` violates nothing —
  the *directory* is incomplete, it was incomplete before the write, and it is
  less so after. Refusing it attributed a directory's fault to a file, which is
  0.13.0's fix one layer up.

  A write **supplying one of the required files** passes with a note saying
  what is still missing. A write **supplying none of them** leaves the
  directory exactly as broken as it found it, and is refused as before — which
  is what keeps this from being a way to switch `presence` off.

  Every other rule keeps denying. `spec-pair` has an order that works, and a
  `structure` violation is caused by the write rather than pre-existing it.

### Added

- **A `Stop` hook**
  ([#61](https://github.com/HenriqueArtur/archwarden/issues/61)).
  `install-hooks` now writes two entries, both running `archwarden hook
  claude-code`, which dispatches on the event it is sent. Two commands could be
  wired to the wrong event, and an answer to the wrong question is a hook that
  reports nothing while looking installed.

  The pre-write hook sees one write at a time and cannot judge a rule about a
  group. At the end of a turn the group is there to judge, and what is missing
  is a fact rather than a prediction. It reports and never blocks — the writes
  have landed — and it is silent when nothing broke. Scoped to what changed
  against `HEAD` plus untracked files.

  An existing installation gains the stop entry on the next `install-hooks`
  run; the pre-write entry is not duplicated.

- **`spec-pair.spec_dirs`**
  ([#67](https://github.com/HenriqueArtur/archwarden/issues/67)). A spec may
  live in a directory the project names — `__tests__`, `tests`, `__specs__`.
  Empty by default, which is sibling-only, so every existing config behaves
  exactly as it did.

  **One level, and the limit is the feature.** A spec in `__tests__/unit/` does
  not count unless `unit` is named too, and an entry with a path separator is
  refused when the config compiles. A reading that accepted a spec anywhere
  below would let one file satisfy the rule for a whole subtree, and the rule
  would report nothing and look exactly like a repository that is fully tested.

### Changed

- **A file's imports are resolved only when a rule of its own asks.**
  Resolution used to be decided for the run: if any rule anywhere needed
  imports placed, every file that had facts for any reason was resolved.
  Measured on a repository of 4 154 files, a boundary rule governing *one file*
  cost about 0.2 s. It is strictly less work and never more.

### Internal

- **A failing gate in `cargo xtask ci` repeats its last 40 lines** under the
  summary ([#80](https://github.com/HenriqueArtur/archwarden/issues/80)).
  "Re-run the command on its own" is advice that works for a gate failing every
  time and is useless for one that does not — and the intermittent one is the
  one worth diagnosing.

- **The plan moved to issues and milestones.** `ROADMAP.md` and `PLAN-V0.md`
  are gone, 2 511 lines of them. Both described work rather than doing it, and
  both had drifted: the roadmap still called v1 "watch mode, LSP, SARIF", and
  the plan described itself as living four releases after it stopped being
  true. Fourteen code comments citing them were rewritten, and one convention
  that lived only inside the deleted file moved to `CONTRIBUTING.md`.

## [0.13.0] — 2026-08-09

**The pre-write hook judges the write. It used to judge the file on disk**
([#55](https://github.com/HenriqueArtur/archwarden/issues/55)) — which, for a
`PreToolUse` hook, is the previous version of the file. A minor rather than a
patch by the question `RELEASING.md` asks: writes that sailed through are now
refused, which is what the hook was installed to do.

The path fix in 0.11.0 was masking this. Both were live at once, and a
repository weighted toward `structure` rules looked like a working hook.

### Fixed

- **A new file was never checked.** Nothing on disk meant no facts, so every
  `naming`, `must_export`, `no-passthrough` and `import-boundary` violation
  passed on creation — the case a pre-write gate most exists for.

- **An edit that introduced a violation was permitted**, because the disk was
  still clean at the moment of the question.

- **An edit that *fixed* a violation was refused**, and this one had no way out
  from inside an agent loop: the agent is told to fix the file and denied
  permission to fix it, against a rule the pending write already satisfies. An
  agent that trusts the hook tries variations of a write that was right the
  first time.

  Path-based rules were unaffected — `structure`, and `spec-pair`'s missing
  sibling — because they read the path and the directory rather than the file.

### Changed

- **`Write`, `Edit` and `MultiEdit` are all replayed.** `Write` carries the
  document; the other two carry replacements, so the result is reconstructed
  from disk before it is judged. `replace_all` is honoured. An edit whose
  `old_string` is not in the file is not replayed at all — the harness will
  refuse it, and judging a write that will not happen is the same fault by
  another route.

  Only the target's own facts come from the event. Siblings, importers and
  directory listings still come from disk: those are what the write is not
  about, and the harness does not send them.

- **`AGENT-INTEGRATION.md` states the invariant** the report asked for: the
  hook answers about the file *as it would be after this write*.

### Added

- **`archwarden_engine::single::check_write`**, beside `check_file`. One judges
  what would be there, the other what is.

### Internal

- **The `cargo-mutants` config moved to `.cargo/mutants.toml`.** It had been at
  the repository root since 0.10.0, where the tool does not look for it, so
  every exclusion in it was written, documented and inert — `cargo mutants
  --list` still offered 102 mutants from a file it says to skip. It never
  blocked a push, because the hook runs `--in-diff` and those files stopped
  appearing in diffs. A configuration doing nothing looks exactly like one
  whose rules are satisfied.

## [0.12.0] — 2026-08-09

### Fixed

- **`subfolder_patterns` is reported to the folder it governs, not only to its
  parent** ([#53](https://github.com/HenriqueArtur/archwarden/issues/53)).
  `filename_patterns` and `subfolder_patterns` are siblings over the two kinds
  of directory entry, and they were attributed on opposite sides. So
  `describe projetos/sensor-sem-numero` answered *"No rule applies"* about a
  name `check` refuses — and `scaffold`, whose entire answer is a shape to go
  and build, returned one for a path that could never pass. Following it
  produced a directory that failed on the next run.

  `describe --help` says *"what the rules require of a path, which need not
  exist yet"*, and the path that does not exist yet is precisely where the name
  is still a choice. Answering after the folder is created is answering too
  late.

  A path with no extension is taken to be a folder — the only evidence there is
  about a path that does not exist. An extensionless *file* therefore hears one
  sentence that does not govern it, which is a sentence too many in a command
  that lists what applies; the alternative was staying silent about a name that
  is refused. `check` is unaffected: it walked the tree and knows.

- **`scaffold` leads with a path that cannot pass**, before listing the shape.
  Correction C11 made this argument for filenames — *"an agent scaffolding a
  path whose name is already wrong would be told everything except the thing it
  has to fix first"* — and it had never been carried to folders.

- **`describe` distinguishes a file name from a folder name.** Both rendered as
  *"a name matching …"*, so a reader could not tell which kind of entry was
  constrained. `check` had always distinguished them.

### Added

- **`Expectation::FolderName`**, and `folder_name` on the `scaffold` JSON
  shape. Additive: `Expectation` is `#[non_exhaustive]` and consumers already
  ignore kinds they do not know, so `DESCRIBE_VERSION` and `SCAFFOLD_VERSION`
  are unchanged.

### Changed

- **`config explain` lists the directories whose names a rule constrains**
  among what it covers. It reads the same `describe_expectation` the fix above
  changed, so a `structure` rule now covers both the directory whose contents
  it governs and the ones whose names it governs. Being governed is not the
  same as being in breach: a folder with a permitted name is covered and
  clean.

## [0.11.0] — 2026-08-09

Four faults in the pre-write hook, reported together against 0.10.0
([#48](https://github.com/HenriqueArtur/archwarden/issues/48)). Three of them
are the same fault: **the hook permitted writes it had never examined, in
silence.**

A minor rather than a patch, by the question `RELEASING.md` asks — *does this
change what an existing, unchanged config reports?* On a machine with a
symlinked checkout it changes it completely: writes that sailed through are now
refused, which is what the hook was installed to do and never did.

### Fixed

- **A path that reaches the repository by another route is inside the
  repository.** A symlinked checkout, a bind-mounted worktree, `/tmp` →
  `/private/tmp` on macOS, a container whose mount path differs from the
  host's: each gives one directory two absolute spellings, and a harness hands
  over whichever its own `cwd` resolved to. The two were compared as text, so
  the other spelling read as "outside the repository" — and the hook permitted
  every write on such a machine while reporting success. The only symptom was
  CI failing later, on files a pre-write gate was installed to refuse.

  The parent directory is resolved rather than the whole path: a pre-write hook
  is asked *before* the write, so the file it names usually does not exist yet.
  `check --file` gets the same fix, because two spellings of one directory
  should mean one thing everywhere.

- **A gate that could not judge a write now says so.** An unreadable event, a
  missing config, a config that does not compile, a path genuinely elsewhere —
  each returned `{}`, which is *"no objection"*. They carry a `systemMessage`
  beginning *"archwarden did not check this write"* now, and still permit: a
  hook that blocked because it could not do its job would be worse than no
  hook.

  A tool that writes no file stays silent, and it is the only thing that does.
  With a matcher broader than `Write|Edit|MultiEdit` that is every `Bash` and
  every `Read`, and a remark on each is a hook somebody removes.

- **A config declaring an unsupported version no longer disables the hook
  silently.** `check` refuses one; the hook parsed it into a config with no
  rules, which compiled, matched nothing and permitted everything. Found by a
  test written for the fault above, in the one place it had not been looked
  for.

- **`config verify-rules` probes the axis a rule constrains**
  ([#49](https://github.com/HenriqueArtur/archwarden/issues/49)). Every
  `structure` rule was offered an unlisted folder. A rule that constrains
  `filename_patterns` and says nothing about subfolders is correctly silent on
  that, and was reported as enforcing nothing — 5 of 14 rules in one
  repository, all 5 of which fire.

  Worse than a wrong tick, because *"5 enforce nothing"* is the line a reader
  acts on, and acting on it means deleting five working rules. A verifier that
  reports a false negative is worse than no verifier, for the reason the docs
  give about silent rules: it is indistinguishable from the real thing. Each
  constrained axis is probed now, and a rule that constrains neither is still
  reported silent — that one really does enforce nothing.

- **`config doctor` no longer calls every `presence` rule idle**
  ([#51](https://github.com/HenriqueArtur/archwarden/issues/51)).
  `rule-evaluates-nothing` asks whether any file is subject to a rule, which is
  a good question about a rule concerning files and a meaningless one about a
  rule concerning directories. `structure` was exempt by name; `presence`
  arrived answering the same way and was not, so `doctor` reported it as idle
  while `check` was firing it on the same repository.

  The suggested fix made it worse: widening `roots` from `projetos/*` to
  `projetos/**` would have asked every subdirectory to hold the required files,
  turning a working rule into a wall of false errors. The engine is asked now,
  through `RuleEngine::answers_for_directories`, so the third directory rule
  has to answer rather than be remembered.

- **`pair.must_exist` refuses a template instead of hunting for braces**
  ([#50](https://github.com/HenriqueArtur/archwarden/issues/50)). The field is
  documented as literal, and a config reaching for `{{raw(dirname)}}` anyway
  validated, ran, and reported every governed file as missing a companion with
  braces in its name — sixteen confident findings about a file nothing could
  create. The template form is what `naming` and `frontmatter.equals` accept,
  so writing one here is the obvious mistake; it is refused when the config
  compiles, the way `presence.require` refuses a path separator.

### Changed

- **`install-hooks` edits the `hooks` key and nothing else.** It used to
  round-trip `.claude/settings.json` through a serialiser, which produces valid
  JSON and *a different file*: blank lines grouping a long `permissions.allow`
  list into sections were dropped, and everything re-indented. Installing and
  then removing now returns the file byte-identical.

- **The installed command is detected, not configured.**
  `./node_modules/.bin/archwarden` when it is installed, `npx archwarden` for a
  `package.json` with nothing installed yet, the bare command otherwise. The
  local binary is preferred because `npx` *fetches* what it cannot find, so a
  project that dropped the dependency would keep a working hook at a version
  nobody chose — and because some repositories ban `npx` outright.

## [0.10.0] — 2026-08-08

### Added

- **`must_export.annotation`: a `naming` rule can require the export to write
  its type down.** ([#39](https://github.com/HenriqueArtur/archwarden/issues/39))

  ```json
  "must_export": { "kind": ["const"], "name": "AGENT_TOOL", "annotation": "AgentToolModule" }
  ```

  `export const AGENT_TOOL: AgentToolModule = {...}` passes;
  `export const AGENT_TOOL = {...}` does not. A registry that moves from a
  typed static array to `readdir` plus `import()` loses its compile-time gate:
  the name and the declaration form are still expressible, the shape is not,
  and a module missing `build` is green in `check` and `tsc` and dies at boot.

  Not type checking. Nothing is resolved and nothing is inferred — the
  annotation is a token in the declaration whose `kind` the rule already reads.
  Whether the annotated value really is of that type stays `tsc`'s question;
  what this gates is whether the declaration is submitted to `tsc` at all.

  A binding writes the type after the colon and a class writes it in
  `implements`, so both are read; a class implementing several contracts
  satisfies a rule asking for any of them. Whitespace is not significant on
  either side. A list means "any of". `annotation` beside `kind: ["function"]`
  is a rule no file could satisfy — a function declares a *return* type, which
  is a different claim — so the config is refused rather than left to flag
  every file forever.

  `scaffold` renders the annotated declaration
  (`export const AGENT_TOOL: AgentToolModule = /* ... */;`), which is a line
  that passes — a promise `signature_hint` never made. `signature_hint` is
  unchanged and still never verified.

  Existing configs are unaffected: a rule that names no annotation ignores them
  exactly as before.

- **The pages speak English or Brazilian Portuguese.** `"language": "pt-br"` in
  the config, or `--lang pt-br` for one run.

  The page and nothing else. The terminal, the JSON and the digest stay in
  English whatever it says: a CI log is pasted into an issue, searched for and
  read by an agent, and one whose language depends on who ran it is worse than
  one somebody has to translate.

  A trait with one implementation per language, so a page cannot grow a heading
  that exists in one of them only — the compiler refuses an implementation with
  a method missing, which is the same property the exhaustive rule-kind match
  gives. Adding a language is one file and the compiler lists what it needs.

  Never detected from the environment: a report whose language depends on the
  machine that produced it cannot be diffed. The sentences a *rule* produces are
  still English on both pages, and the module docs say so.

- **Two HTML pages, for the human the JSON was never for.**

  ```bash
  archwarden agent-guide --format html > architecture.html   # as declared
  archwarden check --html .archwarden/report.html            # as it stands
  ```

  The JSON is a contract with agents and the text output is a gate. Neither is
  what somebody about to *change* an architecture reads: that person is asking
  where reality is pushing against the design, and gets there today by running
  four commands and holding the results in their head.

  So the pages are ordered for them. The centrepiece is a **module grid** —
  rows import, columns are imported — and the one decision that carries it is
  that **a forbidden edge is drawn, not alarmed**: a wall is the design working,
  so it is hatched and colourless, and colour is spent only on a wall being
  crossed. Hatching also means the two states differ by texture and not only by
  hue.

  Rows are numbered and columns carry only the number, which is what keeps the
  grid readable past ten modules. Pressure is grouped **by wall rather than by
  file**, because a wall crossed eleven times is a question about the wall; and
  accepted debt is given the same weight as a current error, since it is where
  somebody already decided the design was losing.

  A cell is decided by asking the **same matchers the engines use** against the
  directories the walk found. Nothing on a page is computed for the page.

  Read-only, self-contained, no script and no network — a page that fetched
  something would stop rendering from a CI artefact in two years. The section
  naming what the run could not decide is bordered rather than tucked away: a
  page that hid it would be worse than the JSON, because it would look more
  trustworthy while knowing less.

  `--html` on `check` is a side artefact rather than a `--format`: a browser
  cannot read a pipe, so the terminal keeps its summary and its exit code and
  the file is written beside them. A page that cannot be written is reported and
  never changes the exit code.

  `cargo xtask preview` writes both against a fixture repository, by running the
  real binary — contributors judge the pages by looking at them.

- **Astro support: the module inside the `---` fence is read.**
  ([#13](https://github.com/HenriqueArtur/archwarden/issues/13))

  ```json
  { "version": 0, "languages": ["ts", "astro"] }
  ```

  `.astro` files were invisible to every fact-based rule, and silently so. An
  Astro repository with `from: "src/**"` and `forbid_import_from:
  ["src/domain/**"]` got exit 0 while every page imported the domain directly.

  Stage 1 of the issue's own design: the `---` fence is a plain TypeScript
  module and is where essentially every import in an Astro page lives, so the
  front-end finds the fence and hands the slice to `oxc`. It owns no parser.
  Spans are shifted back into the file, because a wrong `path:line:column` is
  worse than none — it is one a reader opens.

  The template and inline `<script>` are **not** read. That is stated rather
  than discovered, and it is stage 2.

  Opt-in through `languages`, and not because of cost: widening what archwarden
  governs should be a decision written in the config rather than one that
  arrives with a dependency upgrade. **The un-opted state is loud** — an
  `.astro` file under a rule that needs facts is a counted, named skip.

  `.astro` is its own file class rather than plain source, which is what keeps
  `spec-pair` from demanding `Card.spec.astro` — the spec for an Astro
  component is `Card.spec.ts`, and that override is issue #45's shape, not this
  one's. `.vue` and `.svelte` land in the same class when they arrive.

  The resolver learned `.astro`, so `import Layout from './Base.astro'` lands on
  a path rather than resolving to nothing.

- **`config explain` and `check` now name a skipped check nobody attempted.**
  The text output said `1 skipped` and nothing else, which is
  indistinguishable from a skip on a file that could not be read — and the two
  are opposite decisions. Only the JSON carried the path.

- **A `frontmatter` rule: a document's YAML block must carry these keys.**
  ([#44](https://github.com/HenriqueArtur/archwarden/issues/44))

  ```json
  { "type": "frontmatter", "id": "projeto-frontmatter", "level": "error",
    "roots": ["projetos/*"], "file_pattern": "^projeto\\.md$",
    "require": ["id", "nivel", "componentes"],
    "one_of": { "nivel": ["1", "2", "3"] },
    "equals": { "id": "{{raw(dirname)}}" } }
  ```

  The first rule that reads a file which is not code. A `.md`'s frontmatter is
  often not documentation at all — it is the machine-readable half of the
  document — and nothing type-checks a markdown file. A `projeto.md` with no
  `componentes` does not fail to load; it reports as a lesson that needs none.

  `one_of` is the clause that earns the rule. A missing key is an absence; a
  value *outside* the vocabulary is confidently wrong — `status: concluido`
  where the vocabulary is `feito` drops the document out of the generated table
  with no row and no error.

  `equals` is the `naming` question asked of a file with no exported symbol:
  a name agreeing with a path. Values compare as text, so `"1"` matches
  `nivel: 1`.

  Deliberately absent: `type`, `min_items`, nested paths, and anything about a
  value's shape. That is a document schema and JSON Schema is one. The line
  every rule here keeps is names and vocabularies, never shapes.

  A document with no block is a finding, not a skip — otherwise deleting the
  block would be the way out of the rule. A block that is not YAML is a
  different finding, because the next steps differ.

  `---`-fenced YAML only. Reading it takes `yaml-rust2`, the first parsing
  dependency that is not `oxc`: `status: "feito"  # done`, a flow mapping and
  an anchor are all things a line scanner reads wrong in silence.

- **A `pair` rule: a file of one kind must have a companion of another.**
  ([#45](https://github.com/HenriqueArtur/archwarden/issues/45))

  ```json
  { "type": "pair", "id": "licao-tem-notas", "level": "error",
    "roots": ["projetos/*"],
    "file_pattern": "^projeto\\.md$", "must_exist": "notas.md" }
  ```

  `spec-pair` is this rule for one specific pair and cannot be bent to any
  other: its default ignores exclude anything that is not a JS/TS source file,
  and its companion is *derived* — `<stem>.<marker>.<ext>` — which generalises
  to nothing. Two fixed names in one directory is what the rest of the world
  has.

  **The difference from `presence` is the anchor.** `presence` asks about a
  directory: these files must be here. `pair` asks about a file: because this
  one exists, that one must too. An empty directory is a `presence` finding and
  not a `pair` one.

  The companion may leave the directory — `../projeto.md`, for a sketch that
  needs the lesson one level up and may be called anything, which no
  directory-scoped rule can reach. One direction, always: an orphan companion is
  a note taken before the lesson was written, and is not a finding.

  `FileContext` gained an existence predicate for it, supplied by the caller the
  way `siblings` already is — `check` answers from the walk it has, `check
  --file` answers from disk because it has none, and a rule still never touches
  the filesystem itself.

- **A `presence` rule: these files must exist in each governed directory.**
  ([#42](https://github.com/HenriqueArtur/archwarden/issues/42))

  ```json
  { "type": "presence", "id": "licao-completa", "level": "error",
    "roots": ["projetos/*"],
    "require": ["projeto.md", "exercicios.md", "notas.md"],
    "require_any": ["\\.ino$"] }
  ```

  The rule kind `RULES.md` has been deferring by name. `filename_patterns` is a
  whitelist of what *may* exist and is satisfied by an empty directory, which is
  exactly the state this is about: a unit of work is incomplete until its
  companion files are there, nothing errors when one is missing, and the gap is
  found by whoever needed the file.

  The first rule that reasons about a path that is *not* there. It needs no
  parse and no resolution — a name against the walk.

  `require` takes **filenames, not paths**; an entry with a `/` is refused when
  the config compiles, and the same requirement is sayable by a second rule
  scoped one level down. One rule answering for one directory is what lets
  `describe` and `scaffold` answer for a directory that does not exist yet,
  which is where this rule is worth most: `scaffold projetos/17-nova` prints the
  filenames, and a unit of work gets started rather than corrected.

  One finding per missing entry, not one per directory — each is a separate file
  to create, which is how `spec-pair` reports a missing sibling too.

  It is also the cleanest rule `config verify-rules` has: a violation is a
  directory with no files in it, so nothing has to be invented.

- **`why`: a rule, or a module, can say why it exists.**
  ([#46](https://github.com/HenriqueArtur/archwarden/issues/46))

  ```json
  { "type": "import-boundary", "id": "domain-forbids-app", "level": "error",
    "why": "domain is published as its own package and the app is not",
    "from": ["packages/domain/**"], "forbid_import_from": ["packages/app/**"] }
  ```

  A finding said what the rule wanted and what the file did, and never why the
  rule exists. An agent reading one could comply, and that is all it could do —
  which is the failure mode `AGENTS.md` already had three bare prohibitions
  against ("do not edit `arch.config.json` to make a check pass", "a missing
  spec file means write the test", "exit 2 is not your problem to route
  around"). Each of those is a rule broken because the constraint looked
  arbitrary, and a reason is what makes a constraint non-arbitrary.

  There was nowhere to write one. The config is JSON, so it has no comments,
  and the reason lived in a commit message or a wiki — neither in front of
  anybody at the moment a rule fires.

  It surfaces in the pre-write hook's denial, in `describe` and `scaffold`, in
  `agent-guide`, in `config explain`, and beside a finding. In text a rule's
  reason prints **once per run, at its first finding**; in JSON every finding
  carries it. It is not part of a finding's identity, so rewording one never
  touches `.archwarden/baseline.json`.

  A module takes one too, as a separate answer rather than a fallback.

  `config doctor` reports `rules-without-a-reason` as a count, and only once at
  least one rule has a `why` — a project that never used the field has not
  adopted the practice.

- **`structure.subfolder_patterns`: a regex over directory names.**
  ([#43](https://github.com/HenriqueArtur/archwarden/issues/43))

  ```json
  "subfolder_patterns": ["^\\d{2}-[a-z0-9-]+$"]
  ```

  `filename_patterns` one field over, for the other kind of directory entry.
  `allowed_subfolders` constrains names by enumeration, which works for a fixed
  vocabulary and cannot work for an open set where the *shape* is the rule —
  sixteen lesson folders named `NN-slug` with more arriving, and nobody listing
  them forever.

  The same matcher already existed as `naming.dir_pattern` and was reachable
  only through `must_export`, which needs a TypeScript parse of a file inside;
  a directory with no `.ts` near it could not use it at all.

  A union with the two lists: a name passes if a list names it *or* a pattern
  matches it. The lists are read first, so a `warn_subfolders` entry whose name
  happens to have the right shape still warns — the most specific declaration
  wins, and a name written out is more specific than a regex.

  `describe`, `scaffold` and `agent-guide` all carry it, so the shape is
  answerable before the folder exists, which is where a folder-naming
  convention is cheap to follow.

### Fixed

- **Source in a language archwarden cannot read is counted, instead of passing
  in silence.**

  A `.py` under an `import-boundary` rule was classified `Other` — the class
  that exists so a PNG does not inflate `checks_skipped` — so the rule saw no
  imports, reported nothing and counted nothing. A rule enforcing nothing looks
  exactly like a repository that satisfies it.

  `FileClass` now has four answers, and a missing fact counts as a lost answer
  only when the file could have carried it: a boundary rule pointed at a `.md`
  lost nothing, and pointed at a `.py` lost everything. `check --file` gained a
  `no-front-end` reason distinct from `not-source` — one means the rule is
  pointed at the wrong thing, the other means the rule is right and archwarden
  cannot read the file.

- **`allowed_subfolders: []` now forbids every subfolder, instead of enforcing
  nothing.** ([#40](https://github.com/HenriqueArtur/archwarden/issues/40))

  **This can make an existing, unchanged config report differently** — but only
  one that wrote the empty list explicitly, which today enforces nothing and is
  therefore a config whose author meant something by it. A rule that *omits*
  the field is untouched, and that is every rule that constrains filenames
  only.

  The distinction is the fix: absent and `[]` used to arrive identical, so the
  literal reading — a list of what may exist, holding nothing — could not be
  given to one without giving it to the other. `allowed_subfolders` is now an
  option. Omitted, the rule says nothing about subfolders; `[]` permits none of
  them, which is how a directory says it is a leaf and was previously
  unsayable.

  `config doctor` gained `rule-constrains-nothing` for a `structure` rule that
  names no allowed subfolder, no warned subfolder and no filename pattern —
  the state that used to be valid at `validate`, silent at `doctor` and skipped
  at `check`, all three agreeing a rule was fine while it enforced nothing.

- **`config explain` no longer refers to `config doctor` for an answer `doctor`
  does not have.** ([#41](https://github.com/HenriqueArtur/archwarden/issues/41))

  "It covers nothing in this repository. Try `archwarden config doctor` for
  why." merged two different faults and sent users to a command that had
  nothing to say about one of them. They are now separate sentences:

  - the scope matched no path — *"Its scope matches no path in this
    repository"*, and the referral stays, because `doctor` does have that one;
  - the scope matched and the rule asks nothing of what it matched — *"It
    constrains nothing: its scope reaches 3 paths, and the rule has no
    requirement about any of them"*, said by `explain` itself. It is the
    command that decided, so it is the command that says why.

### Changed

- **A `naming` finding about an export that exists now carries a span**, so it
  prints as `path:line:column` and opens in an editor. Findings about an export
  that is *missing* still have none — there is no position to name.
- **The cache format is at version 5**, from 3. `ExportFact` gained its
  annotations, `FileClass` gained two answers, and documents got a table of
  their own — an entry written by 0.9.2 would deserialise cleanly and be wrong
  about all three. A cache from an older format is discarded rather than
  misread, so the first run after upgrading is a cold one and nothing else
  changes.
- **`cargo xtask ci` runs every gate the workflow runs**, and the `pre-push`
  hook runs it. A gate whose tool is not installed *fails* rather than
  skipping: this release lost three rounds of CI to checks that had never run
  locally, each one reported as `skipped` in a message that read like a pass.
  A test reads `.github/workflows/ci.yml` and fails if the two lists disagree,
  so a job added to CI cannot be one that only ever fails on GitHub. The prose
  list in `CONTRIBUTING.md` that this replaces had silently lost both coverage
  floors.

  The environment is part of the gate, not decoration around it. CI's
  `RUSTFLAGS: -D warnings` does not reach rustdoc — the toolchain action
  exports `RUSTDOCFLAGS` as well — and a run carrying only the command
  reproduced CI's compiler while missing its documentation build. A broken
  intra-doc link passed all 13 gates locally and failed on GitHub before this
  was fixed, and both halves of the environment are checked against the
  workflow now.
- **`cargo xtask clean` removes build caches in tiers.** The default takes
  incremental compilation state and leaves the compiled dependencies, so the
  next build is still warm; `--deps` and `--all` take more. Targets are named
  rather than globbed, benchmark history is never removed, and `cargo-mutants`
  build trees orphaned by a killed run are swept. Measured here once: 27 GB of
  the 59 in `target` was incremental state.
- **A `cargo-mutants` run that is interrupted after finding survivors now
  blocks the push.** It used to read any exit code other than `2` as "the tool
  could not form an opinion" and let the push through — which is right for a
  build failure and wrong for a run that had already printed what it found.
  190 survivors left through that gap.

## [0.9.2] — 2026-08-07

### Fixed

- **An aliased import that reaches its file through a directory `index.ts` is
  rewritten.** ([#36](https://github.com/HenriqueArtur/archwarden/issues/36),
  reopened)

  0.9.1 compared the specifier against the path with its extension and without
  it, and `@Infra/Ent/Card/types` reaching `Ent/Card/types/index.ts` is neither:
  the `*` captured from the specifier was one component shorter than the one
  captured from the path, so it never matched and every directory-index import
  refused. A repository that writes `Entities/Card/types/index.ts` has those
  everywhere.

  Three spellings reach one file — `types`, `types/index` and
  `types/index.ts` — and all three are now recognised, with the destination
  written in whichever form the author used.

  The reopened report blamed the tsconfig's *location*, since the working
  fixture had it at the repository root and the failing one did not. That was
  not it: a tsconfig in a subdirectory works, and 0.9.1 shipped a test for it.
  What the two runs actually differed by was the directory `index.ts` — which
  the report's own evidence pointed at, by quoting two different refusal
  messages.

## [0.9.1] — 2026-08-07

### Fixed

- **`impact --apply` rewrites an aliased specifier when the importer's own
  alias still covers the destination.**
  ([#36](https://github.com/HenriqueArtur/archwarden/issues/36))

  It refused any move whose importer reached the file through a `tsconfig`
  path alias. `paths` is not invertible in general — several patterns may
  reach one file — so the refusal was right in general and wrong for the case
  that dominates a rename: `@Lib/* → ./src/lib/*` covers both
  `src/lib/thing.ts` and `src/lib/renamed.ts`, so `@Lib/thing` → `@Lib/renamed`
  is determined.

  The entry that reaches the file being moved is the entry that produced the
  specifier, so re-running *that* pattern against the destination computes
  rather than chooses. Everything else still refuses: a destination outside
  what the alias covers, an entry that names one file rather than a subtree,
  or aliases that could not be read.

  An entry with no `*` needs no special case and gets none — `"@Env":
  ["./src/Env.ts"]` names one file, the destination is a different file, so it
  does not match. That case is the reason this reads the real map rather than
  transposing strings: a string-level guess would have rewritten `@Env` to
  `@Environment` and produced a repository that does not build.

  `extends` is followed only as far as the first config that declares `paths`.
  `oxc_resolver` merges `extends` properly for resolution and keeps that
  private; a config whose aliases arrive another way finds none here and the
  move refuses exactly as before.

- **A legitimate refusal no longer also reports itself as a bug in
  archwarden.**

  The guard for "an importer the dry run named got no edit" fired alongside
  the refusal that had just explained why, so a single unrewritable specifier
  produced two messages about one import — the second asking the reader to
  report a bug. The guard is for the *unexplained* case and now skips
  importers a refusal already accounts for.

## [0.9.0] — 2026-08-06

A minor for a bug fix, because it changes what an existing, unchanged config
reports — and in the direction of reporting files that were never checked.

### Changed

- **`spec-pair` covers files below a named subfolder, and a nested path in
  `subfolders` means what it looks like.**
  ([#34](https://github.com/HenriqueArtur/archwarden/issues/34))

  **This changes what an existing config reports.** A repository that groups
  related files into a folder under a named subfolder will gain findings on the
  next run. They are files the gate was never applied to.

  An entry was compared against a directory's *name*, so only a direct child
  was covered:

  - `subfolders: ["calcs"]` reached `Entity/calcs/direct.ts` and never
    `Entity/calcs/group/nested.ts`;
  - `subfolders: ["calcs/group"]` could not equal a single component, so it was
    accepted by the schema, reported valid by `config validate`, shown as
    unchanged coverage by `config explain`, and matched nothing.

  An entry now names a directory relative to the selected one and covers it and
  everything below it. `["."]` is unchanged and stays non-recursive — naming
  `calcs` is how a project says which subtree is under the gate, and a
  recursive `.` would swallow `types` and everything else it did not name.

  The cost of the old behaviour, measured in the reporting repository: two
  entities grouped their validation steps the same way, one had thirteen files
  and thirteen specs, the other eleven files and no test at all. Neither the
  report nor the baseline had ever mentioned it. Sixteen files sat in that
  blind spot.

  Run `archwarden baseline --dry-run` after upgrading to see what appears
  before deciding what to do about it.

### Fixed

- **The nightly job is gone; the differential tier runs on CI.**

  It had failed every night for at least six days with
  ``ARCHWARDEN_DIFF_REPO=`` is not a readable path``: an unset repository
  variable substitutes as the empty string, `env::var` succeeds with `""`, and
  the empty path cannot be canonicalised — while the test file promised a
  missing target "prints why it did nothing and passes". An empty value is now
  the same state as an unset one, with a test for it.

  The job moved into CI rather than being repaired in place, for the reason
  `docs/TESTING.md` already gives about mutation testing: a red job nobody
  opens is not a test, it is a habit of ignoring red. Nothing in this project
  runs on a schedule any more.

## [0.8.1] — 2026-08-06

### Fixed

- **`impact <dir> --to … --apply` renames the directory instead of flattening
  it.** ([#32](https://github.com/HenriqueArtur/archwarden/issues/32))

  **This changes what an existing command does, and the old behaviour lost
  directory structure.** Every file landed directly in the destination and the
  levels between were gone:

  ```console
  $ archwarden impact src/Group --to '../Renamed' --apply
    src/Group/A/alpha.ts → src/Renamed/alpha.ts     # `A/` gone
    src/Group/B/beta.ts  → src/Renamed/beta.ts      # `B/` gone
  ```

  Silent, with exit 0, whenever no two basenames collided. Where they did the
  collision guard refused — which is how it stayed hidden, and is itself the
  bug stated plainly: two files in different directories have no business
  landing on one path. On a real 19-entity namespace, 93 source files mapped to
  57 destinations.

  The path below the match now comes along, so `src/Group/A/alpha.ts` lands at
  `src/Renamed/A/alpha.ts`.

  **The glob form changed with it**, and this is worth reading if you use it. A
  file nested inside a match keeps its nesting: with
  `'src/*/shared' --to '../calcs'`, a file at `order/shared/calcs/total.ts`
  now lands at `order/calcs/calcs/total.ts` where it used to land at
  `order/calcs/total.ts`. The doubled name looks odd and is the honest answer —
  the file was in `shared/calcs/`, and `shared` is becoming `calcs`. Collapsing
  that level is a guess about intent, and it is the same guess that flattened
  the namespace above. The dry run prints every destination, so an unwanted one
  is visible before `--apply`.

## [0.8.0] — 2026-08-06

Nine reported issues. Read the first two entries before upgrading: both change
what an existing, unchanged config reports — the first towards reporting more,
the second towards reporting less.

### Changed

- **A warm run no longer resolves one file's imports from another file's
  directory.** ([#20](https://github.com/HenriqueArtur/archwarden/issues/20))

  **This changes what an existing config reports.** Repositories that contain
  two files with identical bytes — barrel files, re-export stubs, `export {};`
  — may see boundary findings on the next run that 0.7.0 did not report, and
  may stop seeing findings that were never real. Both directions are the same
  fix.

  The fact cache is keyed by content alone, so identical files share one entry,
  and the entry carried the path of whichever file was stamped first.
  `resolve_imports` reads that path to know which directory a relative
  specifier points from. Measured on a five-file repository:

  ```console
  $ archwarden check     # cold cache
  1 error, 0 warnings · 4 parsed, 0 reused

  $ archwarden check     # warm cache, nothing touched on disk
  0 errors, 0 warnings · 0 parsed, 4 reused
  ```

  A real boundary violation disappeared, and two runs over an unchanged tree
  disagreed. Reverse which twin is stored and the mirror happens: a finding
  against a file that imports nothing forbidden.

  `Cache::facts` now takes the path it is being asked about and stamps the
  answer with it. No cache format bump — existing caches heal on read rather
  than being thrown away.

- **The `_` escape hatch exempts a directory that is itself a rule root.**
  ([#30](https://github.com/HenriqueArtur/archwarden/issues/30))

  **This changes what an existing config reports.** A `structure` rule whose
  scope selects a `_`-prefixed directory stops reporting its subfolders. If
  your repository has such a directory and you were relying on those findings,
  they will disappear — check with `config explain <rule-id>`, which lists
  every directory a rule governs.

  `RULES.md` documents the hatch without qualifying it by position, and it was
  consulted for subfolders only: `_` worked on a child of a root and not on a
  root, which is the case the sentence describes best. It was easy to believe
  it worked — the reporter's `_database` held only allowed names, so the rule
  had nothing to say about it either way, and their own documentation recorded
  the exemption as fact.

  Only the directory's own name is asked about, never an ancestor's, so a rule
  rooted *below* an exempt directory still fires. That is what makes a
  namespace expressible: `_Legacy` is exempt, and a rule rooted at
  `_Legacy/*` governs each of its entities normally. Silencing a whole subtree
  is still `skip_dirs.globs` with a `/**`.

- **`impact --apply` refuses a move of a file git does not track**, instead of
  discovering it mid-move.
  ([#28](https://github.com/HenriqueArtur/archwarden/issues/28))

  `git mv` refuses an untracked file *during* the move, after the specifier
  rewrites are on disk. That left importers naming a module that had never
  been created, against decision 16's unconditional promise that a refusal
  means nothing happened — and the recovery the message offered,
  `git checkout .`, is the one thing that cannot restore an untracked file.

  The question is now asked in one `git ls-files` before a byte is written.

- **An installed package whose `exports` offers only platform conditions
  resolves.** ([#21](https://github.com/HenriqueArtur/archwarden/issues/21))

  `bwip-js` maps `.` to `browser`, `electron`, `react-native` and `node`, with
  no `default`, so nothing matched and `exports` blocked the fall back to
  `main`. An installed dependency was reported as an import no boundary rule
  could see. `node` joins the conditions; such imports move from `unresolved`
  to `external` in `summary.imports`.

- **`impact --apply` computes a destination specifier for a package with no
  `exports`.** ([#27](https://github.com/HenriqueArtur/archwarden/issues/27))

  A package without `exports` exports everything, so the new specifier is the
  destination's path under the package root. It used to refuse and suggest
  adding an `exports` map — which no map can do for a package resolving
  subpaths through `index.ts`, and which changes what every consumer may
  import. Whichever of `thing`, `thing/index` and `thing/index.ts` the author
  wrote is what comes back.

### Added

- **`archwarden config verify-rules`** — proves a rule bites, where `explain`
  only shows what it reaches.
  ([#24](https://github.com/HenriqueArtur/archwarden/issues/24))

  ```
  ✓ domain-is-self-contained — fires on `packages/domain/order.ts` importing `apps/api/env.ts`
  ✗ cancelled-by-its-own-except — silent on the same import
  ? usecase-name — not verified: a violation means inventing a filename that
    matches this rule's `file_pattern`, which is a regex run backwards
  ```

  Each rule is handed a synthesised violation of its own terms; nothing is
  written to the repository. Exits non-zero on `✗`, so it belongs in CI beside
  `check`.

  What it cannot do is printed on every run: it proves a rule fires on a
  violation of *its own terms* and cannot know what you meant — a
  `forbid_import_from_packages` list missing an entry is a question about
  intent, and a rule with that hole ticks here. Rules whose violation cannot
  be synthesised are reported as `?` with the reason rather than left out.

- **`archwarden baseline --dry-run`** — says what regenerating would change,
  and writes nothing.
  ([#23](https://github.com/HenriqueArtur/archwarden/issues/23))

  ```
    - domain-needs-spec   apps/api/src/order.ts — no longer occurs
    ~ domain-entity-shape apps/api/src/Domain/user → packages/domain/user
    + domain-forbids-outer apps/api/src/billing.ts — imports `@Infrastructure/Auth`

  .archwarden/baseline.json would change: 1 added, 1 no longer occur, 1 moved.
  ```

  Only `+` is a decision. A finding that only changed path is reported as
  moved — but a prefix mapping must explain **two** pairs before it counts as
  one, so a fix and a new finding that happen to share a folder name are never
  laundered into a move.

### Fixed

- **`tsconfig.paths` is read, and the documentation said in five places that
  it was not.** ([#22](https://github.com/HenriqueArtur/archwarden/issues/22))

  No behaviour change: aliases have resolved since `TsconfigDiscovery::Auto`
  was set, and a boundary rule fires on an aliased import that crosses it. The
  false claim cost a real repository real work — believing the blind spot, its
  author duplicated a boundary by hand into `forbid_import_from_packages`, the
  hand-written list was missing two entries, and imports crossed the boundary
  with the build green.

  Aliases apply by TypeScript's own rule: the nearest `tsconfig.json` to the
  file wins, whole. So an app's alias does not apply to a file in a package,
  and a bare `tsconfig.json` takes the repository's aliases away from
  everything under it unless it `extends` the config that declares them. Two
  tests pin both cases; decision 18 records why the maps are not merged
  repository-wide.

- **`recurse_into` names a container, not a contract.**
  ([#29](https://github.com/HenriqueArtur/archwarden/issues/29))

  No behaviour change: the recursion runs, and a directory inside a container
  is held to the folder list. The description — "subdirectories that carry the
  same structural contract, recursively" — reads as *the contract applies
  inside this folder*, and means the opposite one level down: the container is
  not governed, its children are, and a child's name is not this rule's
  business.

  So naming a container **removes** findings. One repository added a namespace
  holding nineteen entities, cleared nineteen findings in a single run, and
  read it as having modelled the namespace. `config explain <rule-id>` lists
  every directory a rule governs, which is where that decision is visible.

## [0.7.0] — 2026-08-05

### Added

- **`check` names every import that did not resolve**, where it used to report
  only how many.
  ([#18](https://github.com/HenriqueArtur/archwarden/issues/18))

  ```
  note: 2 imports could not resolve, so boundary rules did not see them
        `packages/domain/row.ts`: `@Domain/Order/id`, `@Domain/Order/types`
  0 errors, 0 warnings · 4153 files, 1268 directories · 820ms
  ```

  A boundary rule matches globs against where an import *lands*, so one that
  landed nowhere was never checked — and the note that said so gave the reader
  nothing to open. The person who reported it found theirs by deleting imports
  until the count moved, in a repository of four thousand files.

  It bites hardest where the rule matters most: extracting a package out of an
  app, where imports still written with the app's `tsconfig` aliases resolve to
  nothing and are precisely the ones that cross the boundary being introduced.

  `summary.imports.unresolved_imports` carries every `{path, specifier}` pair,
  for a CI job gating on "no import escapes the rules". The text names the
  first ten files and says how many it left out — a repository whose
  dependencies are not installed cannot place a single bare specifier, and a
  line each would bury the findings the run was for.

  `check --file` reports the same thing for one file, under
  `unresolved_imports`, and no longer answers `is fine.` about a file whose
  imports nothing could see. That is the shape a pre-write hook asks in: the
  import an agent has just written is exactly the one nothing has seen yet.

  Unchanged: `tsconfig` path aliases are still not read
  ([`CONFIG.md`](docs/CONFIG.md)), and an unresolved import is still a note
  rather than a finding. It is the statement that no rule could tell, not a
  rule saying no.

### Changed

- **`check --file` no longer prints `is fine.` for a file whose imports did not
  resolve.** No finding changes and no exit code changes — a run that passed on
  0.6.0 passes on 0.7.0. What changes is one line of text, for the one case
  where it was not true: a boundary rule ran against that file, and ran blind.
  Anything matching on that string, rather than on the exit code or the JSON,
  will see the difference.

## [0.6.0] — 2026-08-03

### Added

- **`naming` rules can spell the export name from the directory as well as the
  filename**, through a new optional `dir_pattern` whose capture groups join
  `file_pattern`'s in the same template.
  ([#16](https://github.com/HenriqueArtur/archwarden/issues/16))

  ```json
  {
    "type": "naming",
    "roots": ["src/Infrastructure/Repositories/Entities/*"],
    "dir_pattern": "^(?<entity>[A-Za-z0-9]+)$",
    "file_pattern": "^(?<action>[a-z0-9-]+)\\.ts$",
    "must_export": {
      "kind": "function",
      "name": "{{pascal(entity)}}{{pascal(action)}}Repository"
    }
  }
  ```

  This is the per-entity repository shape, where `fetch-by-id.ts` exists once
  per entity and the entity prefix is what a stack trace names. Previously the
  closest expressible rule asked for `FetchByIdRepository` and was wrong on
  every file it touched.

  `dir_pattern` matches the *name* of the directory the file sits in, not the
  path to it. When set, it must match, exactly as `file_pattern` must. A group
  defined by both patterns is refused at compile time rather than resolved by
  precedence. The rule stays purely lexical, so `describe` and `scaffold` keep
  answering for files that do not exist yet.

- **`import-boundary` can forbid a dependency**, through
  `forbid_import_from_packages` and an importer-side `except_from`.
  ([#14](https://github.com/HenriqueArtur/archwarden/issues/14), decision 17)

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

  `RULES.md` declared this out of scope for v0 because a dependency has no
  repo-relative path for a glob to match. It still has none — the field names
  the *package*, which is why it is correct under pnpm's store layout and under
  yarn PnP, where `node_modules/three/**` is a lie or does not exist.

  Matched as "the package and anything under it", so
  `three/examples/jsm/loaders/GLTFLoader.js` is caught and `three-mesh-bvh` is
  not. `node:fs` and `fs` are one identity. An import that resolves into the
  repository is a path and is matched by `forbid_import_from` instead, never
  both. Transitivity is still declined.

  It reads the specifier rather than the resolution, so unlike the path half it
  still fires on a repository whose dependencies are not installed.

- `config doctor` reports **`dir-pattern-matches-nothing`** when a `dir_pattern`
  matches no directory in its scope — the mistake being writing it against the
  whole path — because such a rule applies to no file and is indistinguishable
  from one that passes.

### Changed

- **`checks_skipped` no longer counts files that are not JavaScript or
  TypeScript.** A `DOC.md` inside an `import-boundary` scope used to produce one
  skipped check per rule, forever, so a repository that documents its layers
  could never report a run the way `AGENTS.md` asks — the number was pinned
  above zero with nothing to fix. It now counts only rules that wanted a
  *source* file whose facts were unavailable, which is the case a reader can act
  on. `check --file` is unchanged and still reports these under `not-source`.
  ([#15](https://github.com/HenriqueArtur/archwarden/issues/15))

  This changes what an existing, unchanged config reports: `checks_skipped` will
  drop, in some repositories to zero. No finding changes, and the exit code
  cannot change.

  One consequence worth naming: an `.astro`, `.vue` or `.svelte` file in a rule's
  scope was counted under the same reason as a `DOC.md`, and now is not counted
  either. That signal was already misleading — it said "a check was skipped"
  where the honest statement is "no parser exists for this extension" — and
  giving it its own reason is
  [#13](https://github.com/HenriqueArtur/archwarden/issues/13).

- **The text output names which rules were skipped, and where.** Previously it
  printed the count alone, so the only reader who could answer "which ones?" was
  one already piping through `jq`.
  ([#12](https://github.com/HenriqueArtur/archwarden/issues/12))

  ```
  note: `src/user/broken.ts` was not checked — unexpected token
        2 checks skipped there: calcs-need-spec, domain-forbids-infrastructure
  1 error, 0 warnings, 2 skipped · 3 files, 3 directories · 1 parsed · 2ms
  ```

### Fixed

- **A refusal from `impact --apply` now names the file to go and fix.** It had
  one sentence for four different causes, and the sentence it used said
  `tsconfig` path alias. A repository whose `exports` map does not reach the
  destination — the realistic case, and the one issue #11 was filed from — was
  told to look in the wrong file.

  The four causes are now distinct: a `tsconfig` alias this does not read, an
  `exports` map that reaches no subpath at the destination, a file leaving the
  package its specifier names, and an importer at the repository root. Each one
  names a different thing to open.

- **`impact --apply` no longer reports success after leaving imports pointing at
  a file it just deleted.**
  ([#11](https://github.com/HenriqueArtur/archwarden/issues/11))

  A file that imports a moving package by name — `@org/domain/thing` rather than
  `../thing` — is invisible to archwarden when that package does not resolve.
  Nothing listed it as an importer, so nothing rewrote it and nothing refused:
  the move went through, printed the files it *had* rewritten, and exited `0`
  over a repository that no longer type-checks. The existing guard could not
  catch it, because it asks whether every *known* importer was rewritten and
  this was a file that was never known.

  `--apply` now refuses when an import names a package the move is taking files
  out of and archwarden cannot place it. The refusal is total, as every refusal
  here is, and `--force` does not override it — `--force` was in the command
  that produced the broken repository.

  The usual cause is a workspace that is not installed, so the message says so.
  An unresolved import to a real dependency does *not* refuse: `react` names no
  package in the repository, and no move could change what it means.

  Reproduced from two `exports` shapes archwarden reads differently from the
  bundler (`"./*/*": "./src/*/*.ts"`, and a package with no `exports` at all); a
  clone before `install` reaches the same state.

---

[Unreleased]: https://github.com/HenriqueArtur/archwarden/compare/v0.27.0...HEAD
[0.27.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.26.0...v0.27.0
[0.26.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.25.0...v0.26.0
[0.25.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.24.1...v0.25.0
[0.24.1]: https://github.com/HenriqueArtur/archwarden/compare/v0.24.0...v0.24.1
[0.24.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.23.0...v0.24.0
[0.23.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.22.0...v0.23.0
[0.22.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.21.0...v0.22.0
[0.21.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.20.0...v0.21.0
[0.20.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.19.0...v0.20.0
[0.19.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.18.1...v0.19.0
[0.18.1]: https://github.com/HenriqueArtur/archwarden/compare/v0.18.0...v0.18.1
[0.18.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.17.0...v0.18.0
[0.17.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.16.0...v0.17.0
[0.16.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.15.0...v0.16.0
[0.15.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.9.2...v0.10.0
[0.9.2]: https://github.com/HenriqueArtur/archwarden/compare/v0.9.1...v0.9.2
[0.9.1]: https://github.com/HenriqueArtur/archwarden/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.8.1...v0.9.0
[0.8.1]: https://github.com/HenriqueArtur/archwarden/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/HenriqueArtur/archwarden/compare/v0.5.1...v0.6.0
