# Decisions

Short ADR-style notes on the load-bearing choices behind archwarden.
Every entry has: context, decision, and the alternatives that were
weighed against it. New entries go at the top.

Format for each entry:

```
### N — Title
Status: accepted | superseded | proposed
Context: what forced this decision.
Decision: what we chose.
Alternatives: what we considered and why they lost.
Consequences: what this locks us into or unlocks.
```

---

### 36 — `skip_type_only` asks one question, and each language answers it
Status: accepted.
Context: issue #157, found turning `spec-pair` on over this repository once
#137 taught it Rust's inline tests. Two files were reported for carrying no
test and both are right to carry none: `crates/archwarden-cli/src/command.rs`,
the clap surface, and `crates/archwarden-core/src/docs.rs`, ninety-seven lines
and zero functions.

`skip_type_only` exists for exactly this and did not fire. It exempted a file
whose exports are all `type` or `interface`, and a Rust declaration-only file
exports `struct` and `enum`.

**Widening the exempt list was the wrong fix, and it is worth saying why.** A
`struct` is not a JavaScript `interface`. It can carry an `impl` block full of
behaviour, and a file exporting one is usually the most testable thing in the
crate. Adding `struct` and `enum` to the exempt set would have quietly excused
most of a Rust codebase from the TDD gate — a rule enforcing nothing while
looking like it enforces something, which `docs/CONFIG.md` calls the worst
failure a linter has.

The question the flag asks is language-independent: *does this file export
anything a test could call?* The **answer** is not. For JavaScript the export
tags are complete, because `type` and `interface` are the only two forms with
no runtime behaviour. For Rust the answer is not on the export list at all: a
`struct` is an export and its `impl` block's methods are not, and those are the
behaviour a test would reach.

Decision: one field, one sentence for the user, and a per-language answer
behind it — the shape `FileClass::tests_live` already uses for *where do this
language's tests live*. `FileFacts` gains `callables`, the functions a file
declares outside its own tests, populated by the Rust front-end and zero
everywhere else.

**The predicate lives in `archwarden-core`, on `FileFacts`.** `Language` is
`#[non_exhaustive]`, so a match in `archwarden-rules` needs a wildcard arm and
a language added later would fall into somebody else's answer instead of
failing to build. Answering it in the crate that owns the enum keeps the
compiler as the guard. The same trap caught `Language::as_str` in decision 35,
one field earlier.

A `#[test]`, and anything inside a `#[cfg(test)]` module, does not count as a
callable. A file whose only functions are its own tests has no behaviour for a
*caller* to reach, and counting them would exempt exactly the file that already
satisfies the rule.

Alternatives:
- **Widen `skip_type_only` to accept `struct` and `enum`.** One line, and it
  excuses most of a Rust codebase. See above.
- **A second flag, named for *callable* rather than *type*.** Honest, and it
  asks the config author to know which of two questions their language answers
  — which is a thing archwarden knows and they should not have to.
- **Leave it, and use `ignore_files`.** The workaround #157 rejected in its own
  words: a hand-maintained list naming two files is precisely what
  `skip_type_only` documents itself as existing to replace.

Consequences: cache format 14 to 15. `spec-pair` is now in this repository's
own `arch.config.json`, over 88 files — which it could not be before, and which
is the sharpest test the rule has. Turning it on found `command.rs` genuinely
untested: three small translation tables between the CLI's vocabulary and the
domain's, each one transposition away from a report that reads perfectly and
answers a question nobody asked. They have tests now.

---

### 35 — A preset may turn a language on, and `disable` is the way back off
Status: accepted.
Context: issue #158, found writing the Rust preset. A preset declaring
`"languages": ["rust"]` did not take: `languages` was read from the entry
config alone, so the local default `["ts"]` won and every rule in the preset
was reported as a **counted skip**.

The failure is the one decision 31 exists to name, arriving through the front
door. A preset exists so somebody can adopt a set of conventions without
knowing them; a Rust preset that requires the adopter to *also* write
`"languages": ["rust"]` has a step nobody can infer, and getting it wrong
produces a clean green run with a note. `docs/CONFIG.md` calls a rule that
enforces nothing while looking like it enforces something the worst failure a
linter has, and this manufactured one on adoption day. Measured on the
published 0.32.0: `0 errors, 0 warnings, 2 skipped`, against a file that
violates two of the preset's rules.

Decision: `languages` is unioned across the whole `extends` chain, like the
lists and unlike the other scalars.

The reasoning it replaces was not wrong, it was one-sided. A preset cannot know
whether the project including it has any `.astro` — that is why `governance`
and `root` stay with the entry config. But a preset whose every rule is about
`.rs` knows exactly what it needs, and the two cases are not the same.

**Nothing new spells "off".** The open question was whether an adopter needs a
way to decline a language a preset turns on, since `disable` drops rules by id
and had no equivalent. It turns out `disable` is already the answer: a language
costs nothing on its own, because a file is only parsed when some rule's scope
reaches it. Disabling the preset's rules leaves the union in place and reads no
files — measured at four milliseconds and zero files parsed. A second field
would have been a second way to say what one already says.

**The union is announced.** A preset that turns a language on turns on
*reading files*, and `docs/RULES.md` is careful about which rules open one at
all. `config validate` now prints `reads: rust, ts` whenever a preset is
involved, so the cost is visible rather than inferred from a run getting
slower. Not printed for a config with no presets: that would be telling
somebody what they just wrote.

Alternatives:
- **Leave it, and document the extra line.** A documented step that a green run
  does not enforce is a step people skip, and the skipping is invisible.
- **Let a preset *propose* a language, confirmed locally.** Honest, and it is
  the same extra step wearing a ceremony.
- **Infer the language from the rules in the preset.** Removes the field from
  the preset entirely, and makes `languages` mean two different things
  depending on where it is written -- the kind of implicitness this format has
  refused elsewhere.

Consequences: a repository extending a preset that names a language starts
reading those files on upgrade, and rules that were skipped begin to fire. That
is the bug being fixed rather than a regression, and `baseline` is the answer
for anyone not paying that debt today.

---

### 34 — A read is a fact of its own, deduplicated by name
Status: accepted.
Context: issue #118 asks for *"only `src/config` reads the environment"*, and
`process.env` is the example it opens with. The issue says the fact is already
there — `CallFact` is extracted for every file — and that is true for half of
what it lists. `Date.now()`, `fetch()` and `console.log()` are calls;
`process.env`, `localStorage` and `crypto.subtle` are **property reads**, and
there is no call site to record. A `chokepoint` that could only see calls would
answer the question it was raised for with half an answer.

Decision: a second list, `FileFacts::reads`, holding the dotted names a file
reads without calling.

**Not folded into `calls`.** A read is not a call, and putting one there would
quietly satisfy a `call-obligation` rule requiring `Event.save` with a file that
merely mentions it — a rule going green for the wrong reason, which is worse
than a rule that never fires.

**The longest chain only.** `a.b.c` records `a.b.c` and not also `a.b` and `a`.
The prefix match a `chokepoint` applies reaches the shorter forms from the
longer one, so recording all three would be three ways to say one thing and
three times the cache.

**Deduplicated by name, first span kept.** This is cached per file, and a
repository's property reads outnumber its calls several times over.
Deduplicating bounds the list by a file's distinct vocabulary rather than by its
length. The cost is that a file reading the same name twice is reported once, at
the first occurrence — which is where a reader would start anyway.

Measured, on 120 generated files and 7,560 lines of deliberately
member-expression-heavy TypeScript: **52 ms cold with the fact and 52 ms
without**, five runs each, and the cache file did not change size at all. The
walk is the same walk the calls already need, and the dedup is what keeps the
list from growing with the file.

Alternatives:
- **Record every occurrence.** Keeps a span per site, and grows with file length
  rather than with vocabulary. The spans of the second and third occurrence buy
  a reader nothing they do not get from the first.
- **Record only reads whose root is not locally bound.** Precise, and it needs
  scope analysis this front-end does not do — the parser is syntax-only by
  design, and adding a binding pass for one rule is a large thing to owe.
- **A committed catalogue of ambient globals** (`process`, `window`,
  `localStorage`). Cheapest, and it is exactly the fixed catalogue issue #118
  says archwarden must not be: it cannot ask about `Ledger.post`, which is the
  case that makes this architecture rather than lint.

Consequences: cache format 13 to 14 (decision 3), so every repository re-parses
once. `chokepoint` reads both lists and reports one finding per site, ordered by
position rather than by which list it came from. The Rust front-end records
none: `std::env::var` is a path to a function, called when it is called, and a
`use` of it is an import `import-boundary` already cuts — the gap this fills
does not exist there.

---

### 33 — Object keys are top-level, and Rust has no options bag
Status: accepted.
Context: issue #164, raised by a repository that found the problem the
expensive way. `FactoryMockDependencies(ENV, { PAY_IN_MEMORY: "all" })` and
`FactoryMockDependencies()` are the same callee at the same arity and opposite
meanings — one runs against in-memory twins, the other starts a Postgres
container. Of 215 files in a suite that was supposed to be entirely in-memory,
five were not, and the only evidence was a run that took longer than it should.
`CallFact::arguments` records string literals *by position*, so it cannot reach
a difference that lives in an object key.

Two questions had to be settled while adding the fact, and both are about where
to stop.

**How deep.** `{ db: { inMemory: true } }` could record `db.inMemory`, and that
spelling is not in the source. Decision: top-level keys only, with a literal
value where the value is a literal and nothing where it is not — the same line
`arguments` already draws about a template with an interpolation in it. A
nested bag records its key and stops. Going deeper is a second grammar and can
be a second decision if a repository ever needs it.

**Whether Rust answers this at all.** Decision 31 said a language gets the
facts its own grammar has, not a translation of another's. Rust has no options
bag: the nearest thing is a struct literal, which is a *typed construction with
a name* rather than an argument shape — `Config { retries: 3 }` names a type
the compiler already checks, and passing it is not the same act as spreading an
untyped bag into a call. Decision: the Rust front-end records no options.

Alternatives:
- **Flatten nested keys into dotted paths.** Reaches more cases and invents a
  spelling nobody wrote, which is how a rule reports an edge that does not
  exist.
- **Record struct-literal fields as `options` in Rust.** A rule written for
  TypeScript would then half-work on Rust without saying so — the failure mode
  decision 31 exists to prevent. A Rust rule about a struct literal should be
  its own thing, named for what it is.
- **Positional arguments only, and tell people to close the door by import.**
  The reporter's own workaround, and they named the cost themselves: forbidding
  the container-capable module from those specs is *a coarser statement than
  the one they mean*.

Consequences: `CallFact` gains `options`, which is a cache format bump
(decision 3) — every repository re-parses once on upgrade. A rule that asks for
options costs nothing extra: the keys are collected in the same walk that
already collects the arguments. And presence is separable from value on both
sides, so `with_options: ["PAY_IN_MEMORY"]` never has to name a value it does
not care about.

---

### 32 — The Rust front-end reads a CST, because the markers are in the comments
Status: accepted.
Context: issue #134. Decision 6 put the parser behind a trait so the front-end
could be replaced; decision 19 measured what a third one costs and said the
parser is one function. Neither says which crate goes behind it for Rust, and
this workspace has been burned by that choice before — the oxc crates are
pinned with `=` because 0.x versions break often.

Four things the choice has to satisfy: byte spans, because a finding carries a
caret; no panic, because archwarden runs as a pre-commit hook and an agent
pre-write hook where a stack trace is a worse failure than a diagnostic;
comments, because decision 30 put `archwarden-allow:` and `archwarden-<key>:`
in them; and a licence `deny.toml` accepts, which excludes every copyleft.

Measured rather than argued. Both candidates were built and run against the
same file.

| | `syn` 2 | `ra_ap_syntax` |
|---|---|---|
| byte spans | yes, `byte_range()` on stable | yes |
| malformed input | `Err` with a message | a tree *and* an error list |
| `//` comments | **discarded** | `COMMENT 33..82`, with ranges |
| transitive crates | 6 | 34 |
| licences | permissive | permissive |

Decision: `ra_ap_syntax`, pinned exactly.

**The comments decide it.** `syn` keeps `///` — those become `#[doc]`
attributes — and throws away every `//`, which is where the markers this
project reads actually live. A front-end on `syn` would need a second, hand
written pass to find them.

That pass is the argument. Finding `//` in a line is trivial and finding it
*correctly* is not: `let url = "https://example.com";` contains one, and so
does a raw string, a char literal and a nested block comment. A scanner that
gets those wrong reads a suppression that is not there, or misses one that is —
in silence, which is the single failure mode this project exists to refuse.
`ra_ap_syntax` was handed exactly that line in the spike and reported two
comments, not three.

**The error model is the second reason, and it fits better than `syn`'s.** A
malformed `.rs` should be a *named skip*, not an exception: `ra_ap_syntax`
returns a tree and a list of what was wrong with it, so a file that half-parses
still yields the facts it did carry. `syn` returns `Err` and nothing else,
which throws away the readable half of a file somebody is mid-edit on — and
mid-edit is exactly when the pre-write hook runs.

**The weight is affordable and was checked rather than assumed.** 34 crates
against a workspace that already resolves 211, of which oxc alone is 83. Every
licence in the added tree is MIT, Apache-2.0 or Unlicense.

Alternatives:
- **`syn` plus a hand-written comment scanner.** Rejected above: the scanner is
  a component whose failures are silent, and it would be ours to maintain.
  Cheaper in crates and dearer in the currency this project spends.
- **`tree-sitter-rust`.** Error-tolerant and keeps everything, and produces an
  untyped tree every extractor has to interpret by node kind. That is the same
  work as `ra_ap_syntax` without the typed AST layer over it, and it adds a C
  dependency to a workspace that has one only because `blake3` needs it for
  SIMD — which decision 14 already pays for once, in the release matrix.
- **Wait for `rustc`'s own parser to be published.** Not a plan with a date.

Consequences: `ra_ap_syntax` is pinned with `=` on the same terms as oxc, and
the version bump is a deliberate change rather than a `cargo update`. It
versions fast — 0.0.x — and that is the cost this pinning is for.

The dependency itself arrives with the front-end that uses it (#135), not with
this decision. Adding it here was tried and `cargo machete` refused it, which
is the gate working: a dependency nothing imports is one nobody can justify by
reading the tree, and "a decision says we will use it" is not something a build
can check.

---

### 31 — A declaration form and a visibility are two axes, and `ExportKind` is one of them
Status: accepted.
Context: decision 19 named this as the thing to settle before a second *code*
front-end, and gave the reason: an abstraction designed while looking at one
language is designed from the thing it is meant to abstract over, and getting
it wrong makes every later language fight it again. Issue #131 is that debt
coming due, because Rust is the next release.

*Written when that release was numbered 0.31 and named as such here. It shipped
as 0.29.0 — the milestones ahead of it were renumbered once 0.28 turned out to
carry nothing worth a minor. The sentence is fixed rather than left, because a
decision that names the wrong version is a decision a reader stops trusting on
the details that matter.*

`ExportKind` today is a declaration form — `function`, `arrow`, `const`,
`let`, `var`, `class`, `type`, `interface`, `enum`, `reexport` — carried as a
bitset (`ExportTags`, a `u16`) so one export can hold several, and filtered by
`must_export.kind` through `KindFilter::{Any, OneOf}`.

It is not an internal detail. `ExportKind::ALL` is the closed vocabulary a
config is validated against, and `compile/fields.rs` renders it into the error
a user reads when `must_export.kind` names something unknown.

**Rust has two axes where JavaScript has one.** Visibility — `pub`,
`pub(crate)`, `pub(super)`, `pub(in path)` — is orthogonal to form —
`fn`, `struct`, `enum`, `trait`, `type`, `const`, `static`, `mod`, `macro`.
`pub fn` and `pub(crate) fn` are the same form and different exports;
`pub fn` and `pub struct` are the same export and different forms.

Decision, in three parts.

**Visibility becomes a field of its own on `ExportFact`, not a variant of
`ExportKind`.** A set that mixed the two would make `OneOf([function, pub])`
sayable and meaningless, and `KindFilter::accepts` is an intersection test — it
cannot express "this form *and* that visibility" over one set.

JavaScript has one visibility and it is public: a symbol is exported or it is
not, and a symbol that is not produces no `ExportFact` at all. Rust keeps that
rule — an item with no `pub` is not an export and is not carried — so the field
distinguishes only *degrees* of exported. `must_export.visibility` narrows;
saying nothing accepts any of them, which is what every config written before
this keeps.

**`ExportKind` grows with Rust's forms under Rust's names.** `fn`, `struct`,
`trait`, `static`, `mod`, `macro` join it; `enum`, `type` and `const` are
already there and mean the same thing in both languages. The tag set widens
from `u16` to `u32`.

It deliberately does **not** reuse `function` for `fn` or `class` for `struct`,
though both readings are defensible and the second is nearly idiomatic. A
config saying `kind: ["class"]` over a Rust tree under the reused spelling
would match every struct; under this one it matches nothing — and `doctor`
says so, which is the whole difference. `docs/CONFIG.md` calls a rule that
enforces nothing while looking like it enforces something the worst failure a
linter has, and the reused spelling manufactures one every time somebody
copies a rule between the two halves of a Tauri repository.

**"Not applicable" is reported, never silent.** A `must_export.kind` naming a
form no language in the config's `languages` can produce is a `doctor` concern
naming both. That is what makes the previous paragraph safe: the answer to
`kind: ["interface"]` over `.rs` is a sentence, not an empty result set.

Alternatives:
- **Grow `ExportKind` with visibility variants.** Rejected above: it makes an
  unsatisfiable filter sayable, and the bitset cannot express the conjunction
  the two axes need.
- **Reuse the JavaScript spellings for Rust's forms** — `function` for `fn`,
  `class` for `struct`, `interface` for `trait`. Rejected: it reads better and
  it is exactly the shape of a silent false green. The overlap it buys is
  small; `enum`, `type` and `const` are already shared, and they are shared
  because they *are* the same thing rather than because the names collide.
- **A per-language `ExportKind`, generic over the front-end.** Rejected as
  decision 19 rejected generalising early: the rule engines consume one fact
  type, and a kind parameterised by language would put a type parameter through
  every engine, the cache and the JSON report to express something a `doctor`
  check answers.
- **Leave `must_export` alone and let Rust configs use `naming.file_pattern`
  only.** Rejected: it gives up the coupling that `naming` exists for on the
  language that arrived second, and the asymmetry would be permanent.

Consequences: `ExportFact` gains one field, so the cache format bumps
(decision 3). `must_export.visibility` is a new config key and `schema/v0.json`
regenerates. Nothing changes for an existing configuration: every JavaScript
export is public, and a rule that names no visibility accepts every one.

The forms themselves land with the front-end that can produce them (#135),
not here — a vocabulary nobody can satisfy would make the `doctor` check above
fire on every config in the world.

---

### 30 — A claim in a comment is a fact of its own, and the header is where it lives
Status: accepted.
Context: issue #104. `frontmatter` asks a **document** to declare things about
itself, and code had no equivalent. Ownership, stability and lifecycle are
ordinary ADR content — *"every file under `payments/` declares an `@owner`"* —
and they were properties of a file no rule could ask about.

Decision: a `metadata` kind reading `// archwarden-<key>: value` out of the
file header, carrying `frontmatter`'s three claims unchanged.

**Its own prefix, not JSDoc.** `@internal` and `@deprecated` already mean
something to `tsc`, to editors and to TypeDoc, and a marker with two readers
eventually has two interpretations: the day somebody writes `@internal` for the
editor's benefit and archwarden reports a boundary violation is the day the
feature gets removed. It also puts this in the same family as
`archwarden-allow`, so a `grep` for `archwarden-` finds everything this tool
reads out of a comment. The cost is that it is uglier than `@owner`.

**Called `metadata`, not `annotation`** — the one thing the issue's sketch got
changed. `naming.must_export.annotation` already means *"a TypeScript type is
written down"*, and `ExportFact::annotations` already carries those. A kind
called `annotation` would put the same word in a config with two meanings one
line apart, and two fields called `annotations` in one `facts.rs` with two
meanings. That is the issue's own argument against JSDoc — a marker with two
readers ends with two interpretations — applied to a word instead of a marker.
The marker a user writes is unaffected: it is still `archwarden-owner`.

**A fact of its own, not a widening of `allowances`.** They are both markers in
comments and that is where the resemblance stops: a suppression changes what is
reported, and a claim is something the file says. Merging them would put a
grammar that can silence findings and a grammar that carries ownership in one
parser, and the first has to stay small and boring for a reason. Same pass over
the comments; two facts out of it.

**And where they touch, the suppression wins.** The two grammars share a
prefix, so `archwarden-allow: reason` would otherwise read as a key called
`allow` holding the word `reason`. Rather than keep a list of reserved words in
step with the suppression parser, the claim parser asks *that* parser first:
whatever it accepts is not a claim. The same question answers the config side —
a rule asking for a key beginning with `allow` is unenforceable however a file
is written, and is refused where a rule naming an undeclared decision is.

**The header only, and everything below it is reported rather than ignored.**
The header is everything above the first statement, which reserves the space
for the per-export version the issue defers: a marker above an `export` means
nothing today, so it can mean something later without the same line acquiring a
second meaning. What building settled is the other half — a marker written
lower down is a finding of its own, `MetadataOutsideHeader`, not an absence.
Telling an author who wrote `archwarden-owner` that the file declares no owner
is the one answer nobody can act on, and it is the closed-vocabulary argument
arriving from the other direction: confidently wrong costs more than absent.
That is what makes the fact carry markers it will not read, and the flag that
says so is worked out in the front-end, where the source text is, for the
reason `AllowanceFact::governs` is.

**The same key twice is reported, not resolved.** Two claims about one thing.
A winner would be something an author has to know by heart, and would hide the
correction behind the line it was meant to replace. The questions about the
value wait, because which value a vocabulary would judge is exactly what has
not been settled.

**No `file_pattern`**, on decision 28's argument about `frozen`: a field
decided before anybody asked for one, addable later without breaking a config.

Alternatives:
- **JSDoc tags.** Shorter, familiar, and already understood by every editor —
  which is the problem, above.
- **Pairs on one line.** Fewer lines and a second grammar to explain when
  somebody writes it wrong.
- **Above any export, in this version.** Far more useful and far more work: it
  needs the marker bound to the declaration that follows it, a position a
  suppression never has to solve. `presence` of an owner is a file-level claim
  and covers the reported cases.
- **Widening `allowances`.** One parser, one pass, and a suppression grammar
  that starts carrying ownership.
- **Ignoring a marker below the header.** Less code and a smaller fact, and it
  reports the one thing an author cannot act on.
Consequences: `FileFacts` gained a field, so `FORMAT_VERSION` goes to 8 and a
warm cache is discarded once — the dangerous shape, as 3 → 4 and 6 → 7 were:
an entry written by the previous build deserialises cleanly and claims the file
declares nothing, which is a finding against a file whose `archwarden-owner` is
right there. Nothing an existing config reports changes otherwise: no rule of
this kind exists until one is written.

### 29 — A mirror is `presence` fed by `naming`'s renderer
Status: accepted.
Context: issue #103. `pair` and `spec-pair` both look in the *same directory*,
and plenty of conventions pair across parallel trees — *"every entity has a
migration"*, *"every route has a page in the docs"*, *"tests live in `test/`,
mirroring `src/`"*. `pair` takes a sibling **name**, so *"the same path,
elsewhere, transformed"* was inexpressible.

Decision: a `mirror` kind that is two existing pieces put together. `presence`
proves a file is on disk without parsing anything; `naming` renders a path from
capture groups with transforms. A mirror is the second producing a path for the
first to check — no new fact, no parse, path arithmetic and an existence check.

**One direction per rule.** *"Every entity has a migration"* and *"every
migration belongs to an entity"* are two claims, and each deserves its own
`why`: the first is about completeness, the second about orphans. A flag would
put two reasons on one rule and make a reader work out which half fired.

**A `subpath` group, decided here rather than deferred.** The issue left this
open — whether a mirror wants the whole relative directory path and not just
the immediate parent — and building settled it: *"tests live in `test/`,
mirroring `src/`"* is one of the four lines the issue names, and `dirname`
carries `b` where `a/b` is needed. Shipping a rule that cannot express its own
headline example is the rule that looks like it works.

It is computed against the **scope** that selected the file rather than a
configured prefix. The scope already decides which files the rule is about, and
a second way of saying where the tree starts is a second thing to get wrong.
The separator an empty `subpath` would leave is collapsed here rather than made
the config author's problem: one template has to work for a file at the root of
the mirrored tree and one three directories down, which is the whole reason the
group exists.

**The counterpart's contents do not matter.** Only that it exists. *"And it
must contain a test case"* is `spec-pair`'s question and has an answer there.

Alternatives:
- **One rule with a direction flag.** Fewer rules to write, and two `why`s on
  one line.
- **Collapsing `pair` and `spec-pair` into this.** They are one rule wearing
  three names, on one reading. Rejected, and the test is worth stating because
  the opposite is tempting: the question is whether the specialised forms are
  *shorter to write*, not whether they are expressible. A bare sibling name and
  a sibling with a marker are shorter, and making the common case wordier to
  buy a generality most configs never use is how a format gets heavy.
- **Waiting for two real rules before adding `subpath`.** What the issue said.
  The two real rules are written in the issue.
Consequences: the four lines the issue names are writable. The sharp edge is
that `roots` selects **directories**, so a rule about the files directly inside
`src/entities` takes `"roots": ["src/entities"]` — and the issue's own sketch
writes `"src/entities/*"`, which selects the directories inside it and reaches
nothing. `RULES.md` says so in a callout, and `config doctor` reports the empty
population as `scope-matches-nothing`.

### 28 — `frozen` is `baseline` pointed forward
Status: accepted.
Context: issue #102. `import-boundary` can forbid **importing** something and
nothing could forbid **adding** to it — which is half of every migration ADR:
*"the legacy module is closed for extension; new code goes in
`packages/core`"*. It is the shape of decision archwarden expressed least well.

Decision: a `frozen` kind whose engine is the smallest in the workspace —
**every file under the scope is a finding** — and whose whole substance is what
that composes with. `baseline` already records what a repository has accepted,
by rule and path. The rule points it forward instead of back:

> every file under these roots is a finding; today's are accepted; tomorrow's
> are not.

Nothing remembers a date and nothing reads `git`. archwarden answers from a
working tree and a committed baseline, which keeps it deterministic and keeps a
shallow clone working — a freeze that consulted history would answer
differently in CI than on a laptop. It also turns `baseline` from a record of
debt into a statement of intent, which is a better thing for it to be.

**A move within is reported; a move out is silent.** The case against — that
reorganising inside a frozen module is not growth — is real and loses on what a
freeze means: a module closed for extension has stopped, and reshuffling it is
not stopping. When the move is deliberate, `archwarden baseline` accepts it and
leaves the change in a diff somebody reviews. That diff already reads well: the
move pairing in `baseline` turns a removal and an addition into one sentence,
and it was written for a different reason before this rule existed.

**Every file, not only code.** A directory that has stopped growing has
stopped growing. `ignore` says "deliberately outside the architecture" and
`archwarden-allow` is the door for the one urgent exception — one line, one
reason, never hidden. A `file_pattern` would be a field decided before anybody
asked for it, and it can be added later without breaking anything.

**Files, not exports.** *"No new exports in this file"* needs the frozen set to
be per-symbol, and `baseline` accepts paths.

Alternatives:
- **Reading `git` to tell a move from an addition.** What would make the move
  question answerable. Rejected on determinism, above.
- **Silent until a baseline exists.** No wall of errors on the first run — and
  a rule that enforces nothing while looking switched on, which this repository
  calls the worst failure a linter has.
- **A `file_pattern` to narrow what counts.** A field before the case.
Consequences: turning a freeze on is **two steps**, and the second is not
optional — skip `archwarden baseline` and the first `check` reports every file
that was already there, each one a finding about the past. `check` still
reports them, which is honest; `config doctor` is where the missing step is
named, as `frozen-with-nothing-accepted` at `warning`, with the command in the
fix. A freeze whose scope reaches nothing is left to `scope-matches-nothing`,
because saying it twice in two voices is worse than saying it once.

### 27 — archwarden requires the annotation; `tsc` checks the body
Status: accepted.
Context: issue #101. `naming` couples what a file exports to what the file is
**called**, and plenty of decisions are about the export alone — *"we do not
use default exports"*, *"one export per file"*, *"every use case returns the
pattern"*. None mentions a filename, and the only way to say any of them was
inside a `naming` rule, which demands a name template. You had to invent a
naming claim you did not mean in order to make an export claim you did.

The motivating case is the third: a team standardises on a returned result
shape and cannot verify it is being followed. `tsc` checks what is annotated
and **cannot require that you annotate** — a function returning `{ ok: true }`
with no return type compiles perfectly.

Decision: an `export-shape` kind carrying three claims —`forbid_default`,
`max_exports`, `must_return` — and the division of labour that makes the third
one worth having: **archwarden guarantees the pattern is declared, `tsc`
guarantees the body conforms.** It works precisely because `tsc` cannot do the
first half at all.

**Three claims in one kind**, because they are the same question asked three
ways. Splitting them would be three kinds sharing one scope, one `roots` and
one `why`.

**`must_return` takes a list**, which settles the alias problem without
imposing a convention: `type Result<T> = ResponsePattern<T, Error>` is the same
type and a different string, and matching is text against text. A team with
aliases lists them; a team that writes one pattern has chosen *"annotate with
the canonical name"*, which is itself an architectural decision and now one the
config states rather than implies.

**`max_exports` counts what exists at runtime.** `type` and `interface` do not
count. A file exporting a function and the interface of its dependencies is
idiomatic TypeScript, and a limit that fired on it would be a rule nobody
leaves on — `spec-pair.skip_type_only` already makes that argument one rule
over.

**The return type is a field of its own, not another `annotation`.** The issue
left this open and leaned this way; building it confirmed the lean. An
annotation says *what this value is*; a return type says *what this call gives
you*. A single list could not tell them apart, and
`export const X: ResponsePattern<…> = () => {}` writes the pattern down about
the wrong thing — it would satisfy a rule asking what the *call* returns while
declaring nothing about the call. The field choice also made the parser change
a *sibling* of `record_annotations` rather than the third arm inside it the
issue predicted: merging them there would mean unpicking them again at the call
site.

Alternatives:
- **Inspecting the returned object literal in the AST.** What would make this a
  real guarantee rather than half of one. Rejected, and this is the line: early
  returns, ternaries, delegation to a helper, spreads — a rule right about most
  files and silently wrong about the rest is worse than no rule, because it is
  read as a guarantee. `RULES.md` already draws this for `call-obligation`.
- **Only `must_return`.** The reported case, and a third of the work. Rejected:
  the other two are the same question and would land later as two more kinds.
- **The return type joining `annotations`.** One field, less code, and the
  confusion above.
Consequences: the three sentences a team writes in an ADR are writable, and the
one archwarden is uniquely placed to enforce — *it is annotated at all* — is
enforceable.

**The hole is stated rather than hidden.** A text match is defeated by an alias
the config did not list, and by a local lookalike declared under the canonical
name. The second is closed by pairing this with
`import-boundary.must_import_from`, which already exists; the first is what the
list is for. `RULES.md` says both beside the field.

**And a defect surfaced on the way**, reported here because it is the part that
changes what an existing config reports: `export * from './x'` produced no fact
at all, so `no-passthrough` — the rule against a file that adds nothing of its
own — was silent about the loudest form of exactly that, while catching
`export { A } from './x'`. Measured, then fixed. The blast radius was measured
too and it is smaller than it looks: `allow_package_entrypoints` is on by
default and the star barrel is overwhelmingly written in a file called
`index.ts`, which was exempt before and stays exempt. What lands is a star
barrel under some other name.

### 26 — A rule names the decision it implements, and the key points one way
Status: accepted.
Context: issue #100. Every rule could say *why* it exists (#46) and nothing said
*what decision it implements*. That is the gap between a config that enforces an
architecture and one that describes it: archwarden's premise is that conventions
should be checkable rather than described, and it got halfway — the reasoning
was there, the decision as a **thing** was not. A name, its rationale, where it
is written down, whether it still holds.

The argument is #46's, one level up: *an agent that knows the rule and not the
reason can comply and nothing else, which is how a config gets edited to make a
check pass.* A rule id in a denial is a thing to satisfy; a decision with a link
is a thing to understand or to argue with.

Decision: a top-level `decisions` block carrying prose — `id`, `title`, `why`,
`link`, `status` — and a `decision` field on every rule kind naming one.

**The rule points at the decision.** A plain foreign key, written where the
author already is. There is no second list to keep in step, a deleted rule
leaves nothing dangling, and a new rule that forgets its decision is visible in
the one place it exists rather than absent from a list nobody re-reads. The
reverse — the rules serving a decision — is computed, which is what
`config explain <decision-id>` and the doctor's superseded check both read.

**The prose lives on the config and the rules carry the reference**, unlike
`why`, which is copied onto each `CompiledRule`. Many rules serve one decision
by construction, so copying would give one paragraph eight places to disagree
with itself. That difference propagates: the report's JSON normalises it —
findings name a decision by id and the envelope carries the prose once — where
`why` is repeated per finding.

**Every kind has the field, and that is why this shipped first of its
milestone.** `export-shape`, `mirror`, `frozen` and `annotation` each land
carrying `decision` from birth. Shipping this last would have been four
retrofits of a field that should have been there.

**It changes what every surface says and nothing about what fires.** The hook's
denial, `describe`, `agent-guide`, `config explain`, the page and MCP all change
shape in one release, reviewable as one diff — which is what makes a version of
its own worth spending.

Alternatives:
- **The decision lists its rules.** The shape the issue was reported in.
  Rejected: it is a second list, and the failure mode is a rule added without
  being added to it, which nothing can see. The foreign key makes the omission
  visible where the rule is.
- **A decision declared inside a module too.** Rejected: a decision that spans
  modules is the common case — the reported one, `ADR-014`, is about a boundary
  between two of them — and allowing both would create two places to look for
  one thing.
- **`check` reporting a rule with no decision.** Rejected on the issue's own
  argument: a repository's build must not fail because its config is
  under-documented, and a gate that failed for that is one people turn off. It
  is `doctor`'s, at `warning`, and only once some rule names one — every
  configuration in the world has zero decisions on the day this ships.
- **A dangling reference reported by `doctor`.** Rejected for the opposite
  reason: a rule naming a decision nobody declared is a typo, and it is refused
  at compile where `from_module` naming an undeclared module already is.
Consequences: **`config explain` takes either namespace**, which is what makes
the command answer the question people actually ask — not *what does this rule
do* but *why is this like this*. That costs one rule enforced at merge time: an
id may not be a rule and a decision at once, refused where both files can be
named, because a command that had to pick would be wrong half the time. It also
answers the half a document cannot: whether the decision is still being kept.

**`config doctor` grew a level, and only because this needed one.** It had
sixteen checks in a flat list with no notion of severity, and #100 needs two of
its three to be advice and one to be a contradiction. The sixteen stay
`warning` — some arguably deserve `error`, and promoting them is a review that
belongs to whichever release is about them. The level does not reach the exit
code: `doctor` is advice and `check` is the gate, which is the same line this
decision draws when it keeps `check` silent.

**`status` is the part to watch.** A `superseded` decision whose rules still
fire is the most valuable check here and the reason the field is not
decoration. `proposed` is deliberately silent — a decision under trial with
rules already running is how one is trialled — and that is the choice to revisit
first if a repository turns up where enforcing an unaccepted decision at `error`
is a real problem.

### 25 — A rule can choose its files by what they import, and pays only if it asks
Status: accepted.
Context: issue #98. A rule's population was where a file sits and what it is
called. Some obligations are about neither. The reported case is a service that
reaches one legacy system two ways — reads over a replica, writes over an HTTP
API — where reads and writes are deliberate siblings and the filenames say what
the action *does*, not how it travels, because erasing the transport from the
contract was the point of the refactor:

```
Entities/ConsumerUnit/update.ts        → an HTTP write
Entities/SystemUser/find-by-email.ts   → a replica read
```

"Every write goes through the request helper" was inexpressible. `roots` caught
the reads too, and no `file_pattern` separates them. The reporter considered
renaming the files and rejected it, correctly: **a linter should not be the
reason a design says something it does not mean.**

Decision: a second axis, `when_importing` and `when_importing_packages`, on
every rule kind whose population it can mean anything for. Path globs matched
against where an import *lands* and package names matched the way
`import-boundary` already matches them — one dialect, because two eventually
disagree.

**Opt-in, and that is the cost model.** A rule that names no imports resolves
nothing and behaves exactly as it did; a rule that names them turns resolution
on for the files its scope reaches, and no further. That is decision 21's shape
— `import-boundary` answers `needs_graph` from its `forbid_reaching` field
rather than from its kind — applied one axis over.

**For a directory rule it means "some file inside imports it".** `presence` and
`structure` report about a directory, so "this file imports X" has no reading
there. Of the three candidates, only this one is ever both true and false: "all
files" is defeated by one `index.ts`, and excluding directory rules would leave
the axis meaning one thing in seven kinds and nothing in two. It costs those two
their walk-only status — a `presence` rule that narrows parses and resolves
everything under its roots, where before it read a directory listing — and that
is the largest single cost here, paid only by a rule that asks for it.

**It lives in the runner, not in any engine.** No rule kind knows the axis
exists: the runner pairs each engine with its rule by position, which is what
`engines_for` already promises and what `describe` already relies on, and drops
the ones the filter excludes. Nine engines that each had to remember to ask
would be nine chances to forget.

`import-boundary` does not get it and will not. It already chooses its importers
with `from`, `from_module` and `from_kind`, and a second way to say one thing is
a second thing to get wrong.

Alternatives:
- **`--command-prefix`-style scoping, or a new rule kind.** Rejected: the
  obligation is the same obligation. A `call-obligation-when-importing` kind
  would double every future change to the one that exists.
- **Only on `call-obligation`.** What the reported case needs, and half the
  cost. Rejected on the ask: `spec-pair` narrowed by import — *"every file that
  talks to the database needs a spec"* — is the same question, and a field
  landing three releases apart under three names is worse than one decided
  once.
- **A skip count for a file whose imports did not resolve.** Attempted, and it
  cannot be done honestly: an unplaceable alias and an external package both
  arrive with nothing resolved, and only the resolver knows which was which.
  So the run reports them where it already does — `unresolved_imports`, naming
  the file and the specifier.
Consequences: the reported rules are writable, and the invariant the reporter
called the most valuable in their codebase — *"every write twin records the
call"* — is enforceable.

**A narrowing decided on an unresolved import is the sharp edge**, and it is
sharper here than for a boundary rule: that one checked the imports it could
place, while a rule narrowed by one may not have applied at all. Nothing new
reports it, deliberately — `summary.imports.unresolved_imports` already names
every specifier nobody placed, and a second signal for the same fact is a second
thing to keep true. `RULES.md` says so beside the field.

The pre-write hook applies the same filter, and
`the_hook_and_check_agree_about_a_rule_narrowed_by_imports` pins it. A write
judged by the hook against a rule `check` would not have applied is the two
surfaces disagreeing about one file, which is what decision 22 exists to make
impossible.

### 24 — The caller says where the repository is, and a translation has to earn it
Status: accepted.
Context: issue #93, then #95. A harness on the host sends
`/home/dev/proj/src/x.ts`; an archwarden inside a container has `/app` as its
root; `repo_relative` answers *outside the repository* — correctly, and
uselessly. Every hook is dead in that setup and the only symptom is a message
saying the write was not checked, which is not approval and reads like one.

0.18.1 made that failure audible. This is the other half.

The report asked for a `--command-prefix`, and measurement said no: with
`docker exec` and no path rewriting the payload still carries host paths, so a
prefix alone is a flag that looks like a fix. What the reporter's workaround
actually does is map one root onto another, written in `sed` because the tool
had nowhere to put it.

Decision: **derive the mapping; never configure it.** Both surfaces are already
told where the caller thinks the repository is, and neither was reading it:

| surface | where it comes from |
| --- | --- |
| every hook | `cwd`, in the shared base of every event's payload |
| MCP | `roots/list`, which the client answers and advertises with `listChanged` |

Measured against Claude Code 2.1.231 by running it, not by reading a schema.
That makes this the same rule `install-hooks` already follows — *how archwarden
is invoked is detected, not configured; a flag is a thing to get wrong and the
filesystem already knows* — and it means a team shares no configuration for
this and gets it anyway.

It lives in `repo_relative`, in `archwarden-api`, because both surfaces go
through that one function. Applying it in the hook alone would leave MCP wrong
in the same repository, which is decision 22's lesson arriving a third time.

**And a translation has to earn itself.** This is the part that took the
thinking. Deriving means a wrapper pointed at a container holding a *different*
project would have its paths rewritten into ours and judged against our rules —
turning a loud, useless failure into a quiet, wrong success, which is the
failure this project refuses everywhere else, arrived at from a new direction.

So a path is only re-rooted when **some ancestor of the result exists on our
side**. That is the same evidence `disambiguate` already uses one function
along, and it is available precisely when the two roots really are one
repository through two mounts: the code is there, so the directories are there.
Where nothing exists to go by, this refuses rather than guesses — unlike
`disambiguate`, which guesses because both of its candidates are already inside
one repository and this one is deciding whether that is even true.

When it refuses, it names **both** roots. *"Outside the repository"* about a
path the caller believes is inside it is a sentence that sends a reader nowhere.

Alternatives:
- **A field in `arch.config.json`.** Committable and it travels — and it cannot
  work: the host root is `/home/dev/proj` for one developer and
  `/Users/ana/proj` for the next, so the one thing a shared file cannot carry
  is this. It would have to be per-machine configuration of something the
  machine is already reporting.
- **`--command-prefix` on `install-hooks`.** What the report asked for.
  Rejected on measurement: it does not fix the reported case, because the paths
  are the problem and a prefix does not touch them.
- **Translating unconditionally.** Simpler, and it is the quiet-wrong-answer
  failure above. The guard is the whole reason this is a decision rather than a
  patch.
- **Requiring the whole translated path to exist.** Too strict by exactly the
  case that matters: `describe`, `scaffold` and the pre-write hook are all
  asked about files that do not exist yet. An ancestor is the strongest
  evidence available that does not refuse the question the tool exists to
  answer.
Consequences: **the wrapper stays and the `sed` goes.** #95 set its bar at "no
wrappers" and that bar was wrong: a harness runs a process on the host, so
something has to reach into the container whatever archwarden does. That half is
inherent and 0.18.1 is what makes it audible. This is the half that was ours.

Nothing changes for a caller whose root is ours — the translation is not
reached, and the common path does not grow a branch it pays for.

**And the guard costs something, measured rather than assumed.** A path in a
directory that does not exist on this side yet has no ancestor to stand on, so
it is refused: the first file of a brand-new module in a container setup comes
back *"did not check this write"* rather than judged. Measured, not reasoned —
`src/order/nota.md` under an existing directory is judged and denied, and
`packages/novo/x.ts` under one that is not there is refused.

That fails to the safe side: the message is never approval, and this project
made those two distinguishable in 0.11.0 precisely so it could be leaned on
here. It is still a legitimate question going unanswered, and it is the first
thing to revisit if the guard is ever loosened.

The guard is the part to break first if this is ever wrong. It trades a
detectable misconfiguration for a slightly larger surface of paths that are
refused with a clear sentence, and if a real repository is found where an
ancestor never exists, the answer is to say so louder rather than to translate
blind.

### 23 — The session hook has no matcher, and what that is worth was measured
Status: accepted.
Context: issue #66. A `SessionStart` hook can put the module map into an
agent's context without the user referencing a file from their `CLAUDE.md` by
hand. The issue was explicit that the matcher decides whether the feature works
at all, and that the names had to be **read from the current Claude Code
documentation rather than assumed** — a missed matcher means half the sessions
silently have no rules in context and nothing reports it.

That instruction was followed and it was not enough. An agent asked for the
documented answer returned two mutually incompatible ones and asserted the
payload field is `session_start_reason`, citing the official reference. It is
not. The string does not appear anywhere in the shipped binary.

What is true was read from the schema Claude Code validates its own payloads
with, and then measured by running it:

```
hook_event_name: "SessionStart"
source: enum(["startup","resume","clear","compact","fork"])
```

| action | events, in the order they fired |
| --- | --- |
| new session | `SessionStart source=startup` |
| `--continue` | `SessionStart source=resume` |
| `/compact` | `PreCompact` → **`SessionStart source=compact`** → `PostCompact` |

And end to end: a hook returning
`hookSpecificOutput.additionalContext` was given a marker no other channel could
have carried, and the model quoted it back. Re-checked against 2.1.231 after
Claude Code updated mid-milestone; unchanged.

Decision: **install the entry with no `matcher` at all.** The matcher is
compared against `source`, so an entry naming three of the five covers three of
them — and covers none of the ones added after it was written. A matcher that
is not there cannot miss one. Nothing in the hook reads the source either:
whichever way a session arrived, it arrived without the rules in it.

What goes in is **a pointer, not the guide** — the module names, the sentence
each author wrote about theirs, and the two commands that answer the rest.
`the_map_stays_short_enough_to_be_carried` pins the length, because the whole
argument for a pointer over the digest is that a long block is the first thing
compaction drops, and that is a property to assert rather than hope for.

Alternatives:
- **Enumerating `startup|resume|compact`.** Rejected: it is the failure the
  issue describes, written out. The cost of omitting the matcher is firing on
  `clear` too — a session that has just had its context wiped, which wants the
  map more than any other.
- **Hooking `PostCompact` as well.** Rejected on the measurement above: both
  fire on a manual compaction, so the map would be injected twice.
- **Injecting the full `agent-guide` digest.** Rejected on the issue's own
  argument: it costs context in every session including the ones touching no
  governed file, and it is the first thing compaction drops.
- **Reporting a broken config through `additionalContext`.** Rejected: the
  user is who can fix a config, and the model would carry an unactionable
  sentence in every session until they did. It goes to `systemMessage`, which
  the schema describes as *shown to the user*.
Consequences: one entry covers five sources and any sixth. **Auto-compaction
was not measured** — it could not be provoked in `-p`, across five `--continue`
turns at `CLAUDE_CODE_MAX_CONTEXT_TOKENS=25000`, or through a long
`--input-format stream-json` session — and the decision rests on three
structural facts instead: the three compaction events are interleaved and so
share a routine, that routine is parameterised by `trigger: manual|auto`, and
`source` carries one `compact` with no auto/manual split for a conditional to
key on. Written down so the next person knows which half is measured. The
matcher-free entry is also what makes the gap harmless: whatever
auto-compaction emits, if it is `SessionStart` at all, this catches it.

### 22 — The operations are the ones every surface asks, not the ones `check` needs
Status: accepted. Supersedes the boundary drawn in decision 20, which it
narrows rather than reverses.
Context: issue #65 said MCP would be the proof: *"if it needs anything not in
`archwarden-api`, it is not."* Building it produced the proof and it came back
negative. Every tool MCP exposes was outside the crate — `describe`,
`scaffold` and the `agent-guide` digest were in `archwarden-cli`, and what a
pre-write check *means* (the baseline applied, and the split between what a
write breaks and what it is fixing) was written out inside the hook.

Decision 20 was not wrong about the principle; it drew the boundary around the
pipeline it was extracted from. `Resolve → Load → Walk → Evaluate → Present` is
what `check` does, and `check` was the only surface that existed when it was
written.

Decision: the agent-facing operations moved into `archwarden-api` —
`describe`, `scaffold`, `guide`, `map` (#66's module map), and `single::check`,
which is the whole judgement of a write rather than the engine call inside it.
The test for what belongs is the one decision 20 already used and did not apply
widely enough: **a shape a program consumes is a contract, and a contract lives
where every surface can reach it.** `scaffold`'s JSON carries a version of its
own; so does `describe`'s; so does the report's.

What stayed behind is what a surface genuinely owns. Rendering splits at the
seam `crate::render` already draws: machine-readable shapes here, terminal
prose and the HTML page there. `GuideFormat` stayed because it carries
`clap::ValueEnum`, on the same argument decision 20 made about `LevelFilter` —
a command-line vocabulary is not an operation. Replaying an `Edit` into the
text it would leave stayed in the hook, because that is a harness's protocol.

And **MCP is a crate of its own** that depends on `archwarden-api` and cannot
see `archwarden-cli`. That is the enforcement. Decision 20 already observed
that the workspace denied `print_stderr` for years and never caught
`prepare()`; a rule that holds because nobody has broken it yet is not holding.
The binary stays one — `archwarden-cli` depends on `archwarden-mcp` and
dispatches `archwarden mcp` into it — because issue #65 requires no new
installation.

Alternatives:
- **Leave the operations in `archwarden-cli` and have MCP call them.** MCP
  lives in the same binary, so nothing would have failed to compile. Rejected:
  it is exactly the arrangement that produced this, and it would have left
  decision 20 asserting something the code contradicts.
- **Move only what MCP needs.** Cheaper by about half. Rejected: it answers
  "what does this surface want" rather than "what is an operation", which is
  the question the first answer got wrong.
- **A separate `archwarden-mcp` binary.** Rejected: issue #65 is explicit that
  the `.mcp.json` names the binary the hook already resolves.
Consequences: the cost was measured before it was accepted and it was
**coverage, not code**. `archwarden-api` is held at 99 lines / 99 functions
against the workspace's 95, so 2 900 lines changing crates raised the bar on
them: `guide.rs` was at 95.0 and `scaffold.rs` at 95.2, both of which had been
passing for a year. Twelve tests were written for the arms nobody had covered —
`no-passthrough`, `import-cycle`, allowlists, `frontmatter`, folder names — and
the crate now reads 99.67/99.58. That debt was real and invisible while the bar
was 95; moving the code is what exposed it.

The drift this milestone was exposed to is now asserted rather than argued. A
config from a version this build cannot read is fed to **every** surface from
outside the process, and each is tested in a pair: the version-0 half proves
the surface does the thing, the version-99 half proves it stops. Issue #55's
defect was caught by unit tests on a *message* that a surface with its own
loading path would never have called — every one of them would have stayed
green while the gate evaporated. `check_write` through MCP and through the hook
are asserted to agree, and `describe` through MCP is asserted byte-identical to
`describe --format json`.

### 21 — A graph rule reads the whole repository, and says what that costs
Status: accepted.
Context: issues #70 and #71. `run::check` walked the tree parsing and
resolving a file only when a rule whose scope covered *that file* asked
for it — the per-file gating issue #79 had just tightened, worth 0.2 s on a
real repository. A cycle rule cannot work that way. A loop that leaves the
scope and comes back is still a loop, so a graph built only from what the
scope reaches reports a clean repository over a real cycle, which
`CONFIG.md` calls the worst failure a linter has.
Decision: `RuleEngine::needs_graph()` — a third question beside
`needs_facts` and `needs_resolution`. When any rule answers `true`, the run
parses and resolves **every source file in the repository**, whatever any
scope says, collects each file's edges, and runs the graph rules after the
walk against one graph built from all of them. Rules that read the graph are
held back from the main loop rather than handed `graph: None`, because a
cycle rule with no graph reports nothing and nothing is what a clean
repository reports. `import-boundary` answers `needs_graph` from its
`forbid_reaching` field rather than from its kind, so every boundary rule
already written stays as cheap as it was.
Alternatives:
- **A graph limited to what the scope reaches.** Rejected: cheap, and
  silently wrong in exactly the case the rule exists for.
- **Resolve only the frontier within `MAX_DEPTH` hops of the scope.**
  Correct — a file more than twelve hops away cannot appear in a reportable
  chain — and genuinely cheaper on a monorepo with narrow scopes. Deferred
  rather than rejected: it replaces the single-pass walk with a worklist,
  its correctness argument needs its own tests, and getting it wrong fails
  as a silent clean report. It is a purely internal change that no rule or
  config field depends on, so it can be added later against a measured
  complaint. Recorded here so the next person does not re-derive it.
- **Holding every file's `FileFacts` to build the graph from.** Rejected on
  measurement: only paths and a type-only flag are read, so the run keeps
  `FileEdges` for every file and `FileFacts` only for the files a graph rule
  actually covers.
- **Interning paths to `u32` indices inside the graph.** Rejected on
  measurement: 30 000 edges cost 5 MB of a 28 MB run, and interning would
  return about 4 MB of it.
Consequences: the cost is real and stated rather than discovered. On a
10 000-file repository with 30 000 in-repo edges, a boundary rule governing
one module of forty runs in 0.01 s and 8 MB; the same scope with
`import-cycle` runs in 0.22 s and 28 MB. The run stops being proportional to
the scope and becomes proportional to the repository. It is opt-in — a
configuration with no graph rule pays none of it — and `RULES.md` publishes
the table. `check --file` and the pre-write hook cannot build a graph from
one file and so refuse such a rule under a `needs-repository` skip reason,
rather than evaluating it into silence.

### 20 — The operations are a crate, and nothing in it writes
Status: accepted.
Context: issue #63. `prepare()` — discovery, load, version guard,
`extends::merge`, `compile` — was called from thirteen places in
`archwarden-cli`, and two of them refused to use it and re-implemented the
orchestration instead. That was not carelessness. `prepare()` reported
failure by writing a miette report to stderr and returning exit code 2, and
neither the pre-write hook nor the end-of-turn hook may answer that way: one
must reply in JSON and exit clean, the other says nothing at all. The
difference in how a failure is *said* forced the path to be copied, and the
copy was missing the version guard. That shipped as issue #55 — a config
from a future version parsed into one with no rules, compiled, matched
nothing, and permitted every write. The gate did not fail; it evaporated.

Decision: a crate, `archwarden-api`, holding the operations every surface
goes through, with one rule:

> **Nothing in it writes, and no function in it takes a writer.** Every
> failure is a value the caller renders.

The stages are named — `Resolve → Load → Walk → Evaluate → Present` — even
where each has one implementation, because that is what lets a later surface
say *"the LSP reuses through Evaluate and brings its own Present"* instead of
negotiating the boundary from scratch. `Renderer` is a trait because
`report::render` was already a two-arm match with the HTML page on a separate
path, and SARIF (#64) would have been a fourth branch in three functions.

Two things the boundary is drawn *around*, not through. A committed file
format is an operation: the baseline decides the exit code and MCP must
respect it exactly as `check` does, which is why `describe_observed` moved
too — its prose is written into `.archwarden/baseline.json`. And a
command-line vocabulary is not: `LevelFilter` and `By` carry clap's
`ValueEnum` and stay in the surface that has flags, on the same argument that
kept them out of `archwarden-core`.

Alternatives:
- **A general lifecycle bus, where anything registers into any stage.**
  Rejected, and this is the one worth writing down: that is the v2 plugin
  API, `ROADMAP.md` never decided between WASM and dylib, and there is no
  external consumer to validate it against. Building it now designs against
  imagination. The seams here are the ones the project has *earned* by the
  same test it already applies — `Parser` is a trait because there are three
  front-ends, `RuleEngine` because there are nine. `Cache`, `Resolver` and
  `Walker` have one implementation each and get no trait.
- **Leave the orchestration in `archwarden-cli` and have MCP depend on it.**
  Rejected: a server depending on a binary crate to get the report format is
  the dependency pointing backwards, and it is the same mistake
  `archwarden-engine` exists to avoid one layer down.
- **Return rendered strings instead of values.** Rejected: it moves the
  entanglement rather than removing it. The CLI needs `LoadError::Invalid`'s
  source text and byte offsets to draw a caret, and a boundary that flattened
  those into a sentence would trade one duplication for a worse diagnostic.

Consequences: the CLI no longer mentions `extends::merge`, `compile::compile`
or `version_is_supported` anywhere, and `archwarden-cache` stopped being a
dependency of it. Enforcement is structural rather than by review — the
workspace already denied `print_stderr` and never caught `prepare()`, which
wrote through a `&mut dyn Write` it was handed; what catches it now is that
no signature in the crate mentions a sink, with `Renderer` as the single
documented exception. The crate is held at the `archwarden-core` coverage
bar rather than the CLI's 95, because a branch nothing tests there is one
every surface inherits at once. MCP (#65) is the proof the boundary is in
the right place: if it needs anything not in `archwarden-api`, it is not.

### 19 — A second front-end, and what a third would cost
Status: accepted.
Context: issue #44 asked for a rule over markdown frontmatter, which is the
first time archwarden reads a file that is not JavaScript. Decision 6 put
the parser behind a trait for exactly this and the trait had never been
exercised, so the question "what does another language cost" had no
written answer and was re-derived every time it came up.
Decision: markdown gets a front-end of its own producing `DocFacts`, a
second fact type rather than fields on `FileFacts`. Facts are per *kind of
file*, not one struct everything grows into: a markdown file has no
imports, exports or calls, and sharing the struct would hand every rule
engine a field it never reads. The cache gains a table on the same terms.
`FileClass` gains `Document` and `UnreadableSource`, and `needs_facts`
returns which facts a rule wants rather than whether it wants any, so a
missing fact is counted as a lost answer only when the file could have
carried it.

What a third front-end costs, measured against this repository:

- **The parser is one function.** `Parser::parse(path, source, hash) ->
  FileFacts`. The whole JS/TS front-end is ~1 200 lines including tests;
  a language with fewer ways to spell an export is smaller, not bigger.
  No rule, command, cache or report changes.
- **`FileFacts` is JS-shaped**, and that is the real cost. `ExportKind` is
  `function`/`arrow`/`const`/`class`/`interface`/…, plus `is_default` and
  `reexport_from`. Python has no `export`; Go's is capitalisation. Before
  a second *code* front-end, decide whether `ExportKind` grows or whether
  "not applicable" becomes a documented per-language state that `doctor`
  reports — getting that wrong makes every later language fight it again.
- **The resolver is the expensive half, and only `import-boundary` needs
  it.** ~2 650 lines across five files, Node-shaped throughout:
  `tsconfig.paths` per importer, `package.json` `exports`, workspace
  manifests, extension and directory-index resolution. Its size is not the
  cost; issues #36, #37 and #38 are all alias bugs found in a resolver
  that was already shipping, and a new one starts that clock at zero.
  Python's `sys.path`, Go's `go.mod` and Rust's module tree are different
  algorithms, not variants.
- **Everything except `import-boundary` needs only the parser.** That is
  the division worth holding on to.

Consequences: three rules (`structure`, `presence`, `pair`) already work on
any repository and `RULES.md` now says so. A language may ship without a
resolver; when it does, an `import-boundary` rule over its files must be a
loud refusal and never a silent pass — which is the same principle
`UnreadableSource` enforces today for a language with no front-end at all.
Astro (issue #13) is the cheapest possible third front-end and reuses both
`oxc` and this decision's fence extractor.
Alternatives:
- Generalise `FileFacts` now, before a second language exists. Rejected:
  an abstraction designed while looking at one language is designed from
  the thing it is supposed to abstract over. The seam is a one-function
  trait and needs exercising, not widening.
- Read frontmatter inside the rule, bypassing the facts layer. Rejected:
  it would need its own skip accounting, its own place in
  `unreadable_files`, its own answer for `check --file` and `--no-cache`.
  Going through the seam makes a malformed block behave exactly like a
  `.ts` that will not parse.

### 18 — `tsconfig.paths` is read per importer, and the maps are never merged
Status: accepted.
Context: issue #22 reported that the aliased half of a boundary rule
silently enforces nothing, and proposed reading `compilerOptions.paths`
into one repository-wide map. The premise was taken from this project's
own documentation, which said in five places that archwarden does not
read the alias map.

That documentation was wrong, and had been for as long as
`TsconfigDiscovery::Auto` has been set on the resolver. `paths` is read,
per importing file, and a boundary rule fires on an aliased import that
crosses it — `a_tsconfig_path_alias_resolves_to_the_same_file_as_the_relative_form`
has asserted so since v0. The cost of the wrong sentence was real: the
reporter duplicated their boundary by hand into
`forbid_import_from_packages`, their hand-written list was missing two
entries, and imports crossed the boundary with the build green. A false
claim of a blind spot produced an actual one.

What they hit is narrower than the docs led them to believe. Aliases are
resolved by TypeScript's own rule — the nearest `tsconfig.json` to the
file wins, whole — so an alias declared in an app's `tsconfig` does not
apply to a file in a package, and a bare `tsconfig.json` in a directory
takes the repository's aliases away from everything under it unless it
`extends` the one that declares them. Their files were mid-extraction:
physically in `packages/domain`, still compiled by the app's program.
Decision: keep per-importer discovery, and decline to merge every
`paths` map in a repository into one.
Alternatives:
- Merge all `paths` maps repository-wide, as the issue proposed.
  Rejected: `@/*` is the most common alias there is and means a
  different directory in every package — `each_package_gets_its_own_tsconfig`
  is that test. A merged map resolves one package's import into
  another package's source, and a boundary rule fed a *wrong* edge is
  worse than one fed no edge: `check` names the import it could not
  place (issue #18) and says nothing about the one it placed wrongly.
- Let `arch.config.json` declare the aliases. Rejected for the reason
  the issue itself gives: a second source of truth for the same fact,
  where an alias changed in the `tsconfig` and not mirrored here is a
  silent false green.
Consequences: an import that resolves under the build but not under the
file's own `tsconfig` is reported as unresolved and named, which is the
honest answer — under that file's compilation context it does not
resolve either. The fix is in the `tsconfig`: declare the path where the
file lives, or `extends` the config that does. Two tests pin the
behaviour so the documentation cannot drift away from it again.

---

### 17 — A boundary may name a package, in a field of its own
Status: accepted.
Context: `RULES.md` declared a prohibition on a dependency out of scope for
v0, and gave a real reason: globs are matched against repo-relative
paths, and an installed package does not have one. Issue #14 is the
motivating case — `three` is 150 KB gzipped against a page budget that is
otherwise a few KB, and the project's actual rule is "only
`src/scripts/three/**` may import it".

The argument that changed the answer is not the lint. Biome's
`noRestrictedImports` covers it and covers it well. It is that the rule
then lives in a second config file, and `describe` and `agent-guide` read
only the first — so an agent that consults archwarden before writing, which
is the whole premise of `AGENTS.md`, gets an incomplete answer and writes
the violation. A config that structurally cannot hold a project's rule
makes the agent-facing commands wrong, not merely incomplete.
Decision: `import-boundary` gains `forbid_import_from_packages`, matching
**package identity** rather than any path, plus `except_from` for the
importing side.

Four things fall out of "identity, not path", and each is load-bearing:

- The package **and everything under it**. `three/examples/jsm/loaders/
  GLTFLoader.js` is the import that costs the bytes; a rule that caught
  only the bare name would miss the case it exists for. `three-mesh-bvh`
  is a different package.
- `node:fs` and `fs` are one module and one identity.
- An import that resolves **into this repository** is a path, and
  `forbid_import_from` is its field. The two never both fire on one
  import, so a `tsconfig` alias spelling a local shim `three` is not
  caught by a rule about the dependency.
- Reading the specifier rather than the resolution means the rule holds on
  a repository whose dependencies are not installed. That is the opposite
  of the path half, which is blind there, and it is worth having: a CI job
  that lints before installing still enforces it.
Alternatives:
- **A glob against `node_modules/three/**`.** Rejected as a lie: under
  pnpm's store layout that path is a symlink into a content-addressed
  store, and under yarn PnP there is no such path at all. A rule that
  depends on a package manager's on-disk layout enforces nothing on half
  the ecosystem, silently.
- **A scheme prefix inside `forbid_import_from` (`"pkg:three"`).** One
  field and one mental model, which is genuinely attractive. Rejected on
  the same ground the issue raises: the day someone writes `"three"`
  without the prefix, it is silently a path glob that matches nothing —
  and a rule enforcing nothing is indistinguishable from a rule that
  passes. A separate field cannot be got wrong that way.
- **Reusing `except` for the importer side.** Rejected: `except` already
  means "and these imported paths are fine". Overloading it to sometimes
  mean the importer would make an existing rule's meaning depend on which
  other fields are present.
Consequences: transitivity is still declined, exactly as for the path
half — `src/lib` importing `src/scripts/three`, which imports `three`, is
not flagged. `RULES.md` declined reachability and this declines it the
same way, so the two halves of the rule stay one idea.

The rule reads the specifier rather than the resolution, so a dependency
reached through a `tsconfig` alias is spelled by the alias and not by the
package name this field matches. `check` names every import it could not
place (issue #18), which is where such a case shows up.

---

### 16 — `impact --apply` moves what you named; it never picks what to move
Status: accepted.
Context: decision 2 puts archwarden in the report-only space and decision
13 explains why the most obviously fixable rule must never have a fix.
`impact --apply` writes to the user's source tree, so it has to be
squared with both or it is those decisions being quietly reversed.

The pressure that produced it is real. Eliminating a `shared/` folder
across seven entities in one repository is 15 files, 29 import
specifiers, and 24 files edited. archwarden already knew every one of
those — it resolves the graph to answer `impact` — and reported them
correctly while helping with none of it. An editor does the relative
half of the rewrite and leaves the package-name half, which in that
repository is the majority.
Decision: `--apply` ships, under one rule: **archwarden carries out the
move the caller described, and never decides what to move.**

Concretely, and each of these is load-bearing:

- No finding suggests it. `check` never mentions it, and there is no
  `--fix`, no "apply all", no mode that moves more than the argument
  named.
- Dry run stays the default. `--apply` is a second, explicit word.
- The exported symbol is not renamed, even when the filename changes.
  Renaming an export breaks callers in a way this cannot see — decision
  13's argument about `naming` — so the output says the symbol was left
  alone and `check` reports the mismatch afterwards, which is where a
  `naming` rule belongs.

That is the same seam decision 13 left open for `scaffold --write`:
"write the file I am about to write" rather than "fix the violation".
The distinction is not the blast radius, it is who chose.
Alternatives:
- Leave it out and let editors do it. Rejected on measurement: an editor
  cannot rewrite `@org/domain/email/x`, because to it that is a package
  name like `react`. In the repository this was built against, 5481
  imports are written that way against 5690 relative ones, so "the
  editor handles it" is half a refactor.
- `--fix` on `structure` findings, moving a file into an allowed
  subfolder. Rejected, and this is the line: the rule knows the folder
  is wrong and not which of eight allowed folders is right. Picking one
  is a design decision, and a linter that makes it is guessing with
  write access.
- Warn on a dirty working tree instead of refusing. Rejected: `git` is
  the entire undo story here, and an undo that would take the user's
  uncommitted work with it is not one.
Consequences: everything is computed and validated before a byte is
written, so every refusal is total — there is no state where half the
imports are rewritten. A dynamic import naming no module blocks the
apply, because whether such a file imports the target is unknowable;
`--force` is the only refusal a flag may override, and the report prints
the file to look at first. Everything else — a specifier resolving
through a `tsconfig` alias, which is read forwards and cannot be written
backwards, a destination already occupied, two files landing on one path
— refuses outright, because forcing past one produces a repository that
does not build.

That promise was unconditional and, until issue #28, untrue in one case.
A file being moved that git does not track is refused by `git mv` — and
refused *during* the move, after the specifier rewrites are on disk, so
the repository was left with importers naming a module that had never
been created. The recovery the message offered, `git checkout .`, is
precisely what cannot restore an untracked file: the trigger and the
reason the advice fails are the same fact. Untrackedness now joins the
preconditions, asked in one `git ls-files` before anything is written.
The general lesson is the one the promise already implied: a question
answered by the tool performing the write is not a precondition, however
early in the write it happens to be asked.

The emptied source directory is removed. Not cosmetic: `structure` rules
are about directories, so an emptied `shared/` keeps reporting the exact
finding the refactor was run to remove. Measured before the fix: nine
warnings, unchanged, after every file in them had moved.

---

### 15 — Workspace packages resolve from their manifests, not from `node_modules`
Status: accepted.
Context: a monorepo imports itself by package name —
`@flowmaatik/domain/email/x`, not `../../domain/src/email/x`. Node answers
that specifier by looking in `node_modules`, where the package manager has
left a symlink. `oxc_resolver` does the same, correctly, and decision 7 says
not to hand-roll a second worse copy of Node resolution.

On a checkout that has not run an install, that whole half of the graph
resolves to nothing. Measured on a real pnpm monorepo with no
`node_modules`: **5481 imports by package name against 5690 relative ones**.
`impact` on one file found 2 importers where the true answer was 3; the
missing one was in another package and imported by alias.

What makes it a bug rather than a limitation is that nothing said so.
Import-boundary rules over those edges report nothing, which reads exactly
like a boundary that is satisfied. The tool was answering about the
relative half of a repository and presenting it as the whole.
Decision: build the alias map from what the repository itself declares —
every `package.json` with a `name`, and the `exports` field that says which
subpaths it offers and where they land. Feed it to `oxc_resolver` as
**`fallback`**, not `alias`.

`fallback` is consulted only after normal resolution has failed. A
repository that *has* installed its dependencies resolves exactly as it did
before, and an installed package always wins over our reconstruction of it.
The map fills a hole; it never overrules anything.
Alternatives:
- Read `pnpm-workspace.yaml` (and `workspaces`, and bun's equivalent) to
  learn which directories are members. Rejected: it puts a YAML parser in a
  binary that has no other use for one, and the answer is the same in every
  layout anyone writes. Taking every `package.json` in the walk at its word
  costs a package that exists on disk but is excluded from the workspace —
  and that one is only reachable by a specifier no file can be importing,
  since under Node it does not resolve and the repository would not build.
- Reimplement the `exports` subpath algorithm and resolve to a path
  directly. Rejected: that is decision 7 again. The `exports` patterns map
  onto `oxc_resolver`'s own wildcard alias form (`@org/domain/email/*` →
  `<dir>/src/email/*.ts`), so the matching stays in the resolver.
- Require an install. Rejected: it makes archwarden useless in the case it
  is most useful — CI that lints before it builds, and any checkout where
  the lockfile is the only thing that changed.
**Amendment, 0.5.1.** `fallback` is right and incomplete. It covers the
case where normal resolution *fails*; it says nothing about the case
where it succeeds and lands on a **copy** of a workspace package under
`node_modules`. pnpm with `node-linker=hoisted`, npm on a filesystem
without symlinks, a container volume, a partial install — all leave a
copy rather than a link, and a copy has `node_modules` in its path, so
`classify` called it somebody else's code and the file importing it
vanished from the graph.

The consequence was not a missing warning. `impact` reported two
importers where there were three, and `--apply` rewrote two, left the
third pointing at a file that had moved, and exited 0. Measured on the
same repository at the same commit with the same published binary: 29
specifiers rewritten with a symlink, 26 and three broken imports with a
copy.

So a resolved path under `node_modules` whose package name is one the
repository declares is mapped back to the source it was copied from —
when that source exists, and never otherwise. A dependency that merely
shares a name is still a dependency.

Consequences: resolution now depends on every local `package.json`, which
the `resolution_epoch` already hashes (decision 3's cache design listed
`package.json` for `exports` and workspaces), so no cache change was needed.

A repository that adds an `import-boundary` rule over an aliased edge will
now see findings it did not see before. That is the bug being fixed, not a
regression — but it is a behaviour change, and a project upgrading into it
should expect its first run to report more than the last one did.

---

### 14 — Linux binaries are static musl, and the floor is a decision
Status: accepted.
Context: archwarden 0.3.0 would not start on Debian 12, Ubuntu 22.04,
or any `node:` image. The binary required `GLIBC_2.39`, which only
Ubuntu 24.04 and newer have.

The cause was a feature: `--changed` was the first thing to put
`std::process::Command` into the shipped binary, and Rust's standard
library links its pidfd path along with it — `pidfd_spawnp` and
`pidfd_getpid`, both glibc 2.39. Nothing about the toolchain or the
runner changed; the code reached a corner of std that had a newer
floor.

Measuring the published binaries showed the deeper problem. 0.1.1 and
0.2.0 required 2.34, and 0.3.0 required 2.39 — nobody had ever chosen
either. The floor was whatever the build runner happened to have, and
it moved when a feature touched a new symbol. Running the published
0.2.0 in `debian:11` fails too, so "restore the old floor" was never
a fix, only a smaller version of the same bug.
Decision: the Linux packages carry **statically linked musl**
binaries, under the plain `linux-x64` and `linux-arm64` names, with no
`libc` field and no separate `-musl` packages. A static binary has no
floor to move.

The cost was measured rather than estimated, running the published
glibc and musl 0.3.0 binaries on the same machine over 8000 files that
are parsed and resolved: 55ms against 60ms cold, 51ms against 59ms
warm, and the musl binary is slightly *smaller*. About 10%, or 8ms —
against a failure mode that removes the tool from every common Linux
container.
Alternatives:
- Build on an old base image, or with `cargo-zigbuild` targeting a
  named glibc. Both work and both keep the performance. Rejected
  because they replace one floor with another: the next time std
  reaches a newer symbol the problem returns, and CI will not notice,
  because CI runs on the new runner. It is the same trap, deferred.
- Keep both, glibc as default and musl as fallback. Rejected: that is
  what 0.3.0 shipped, and the glibc package is the one every
  glibc-based machine installs — so the failure would remain the
  default experience.
- Avoid `std::process::Command`. Not viable: reading git's index by
  hand to answer `--changed` is not a trade anyone should make.
Consequences: Alpine and Debian 11 now run the same binary as Ubuntu
24.04, and there is no libc detection left in the wrapper. The release
archives for Linux are named `...-unknown-linux-musl`, which is what
someone downloading directly will see.

The floor being a decision is enforced, not documented: the release
workflow runs each Linux binary inside `debian:11` (glibc 2.31, older
than every current distribution) and `alpine` (no glibc at all) before
anything is published. Those are the two ways a Linux binary fails to
start.

That check is the real lesson. Every check archwarden had ran on a
machine shaped like the runner, so a binary that only worked on the
runner passed all of them — including a full local battery, on a
machine that happened to be Ubuntu 24.04 on aarch64. A test that
cannot fail on the developer's machine has to run somewhere that can.

---

### 13 — `--fix` stays out, and `spec-pair` is why
Status: accepted.
Context: a real repository put 37 warnings of "action without a spec"
on one screen, nearly all identical, and asked for a `--fix` that
would write the missing `.spec.ts` files. Decision 2 already deferred
`--fix` to a later version; this records why the most obviously
fixable rule is the one that must never have it.
Decision: no rule gets `--fix`. Not in v0, and `spec-pair` not ever
in the form proposed.

Rule by rule, the mechanical fix and what it costs:

- `structure` — move the file. Breaks every import of it.
- `naming` — rename the export, or the file. Either breaks callers.
- `import-boundary` — no mechanical fix exists; the answer is a
  design change.
- `call-obligation` — insert a call. That is editing behaviour, and
  a linter that writes statements into a function body has stopped
  being a linter.
- `spec-pair` — create an empty file. Mechanically trivial, and the
  reason for this entry.

`spec-pair` is the trap because the fix is easy and wrong. With
`require_non_empty_spec: false`, writing an empty spec turns a real
warning into a pass while changing nothing true about the repository:
the tests that were missing are still missing, and the linter now says
they are not. That is a tool manufacturing green. With
`require_non_empty_spec: true` the stub does not even help — it still
fails, so `--fix` would have done nothing but create files.

So the flag either lies or is useless, depending on a setting the user
did not think they were choosing between.
Alternatives:
- Emit a stub containing a failing test, so the suite goes red and the
  author is forced to look. Rejected twice over: archwarden would be
  writing test code in a framework's syntax it has no business
  knowing, and a linter that deliberately breaks your test suite is a
  linter people uninstall. It also inverts the contract — `check`
  reports, and the build failing afterwards would come from a file
  archwarden wrote.
- `--fix` restricted to `spec-pair` rules without
  `require_non_empty_spec`. Rejected: that is precisely the
  configuration where the stub is a lie, so the restriction selects
  for the dangerous case.
- `scaffold --write`, creating the shape for one named path on
  purpose. Not rejected, but not this: it is a create-time
  affordance, framed as "write the file I am about to write" rather
  than "fix the violation", and it earns its keep only if someone
  asks for it on its own terms.
Consequences: the pain that prompted the request is real and stays
unaddressed by this entry. It is not a fix problem — it is a debt
problem: a repository adopting archwarden inherits violations it has
not decided to fix yet, and every run reports them again.

The honest answer to that is a **baseline**: a committed record of
accepted findings, against which `check` reports only what is new.
It says out loud what a stub would have said silently, it is reviewable
in a pull request, and it does not require archwarden to write a single
byte of anyone's source. That is the feature to build when this pain
comes back, and `--changed` deliberately did not become a disguised
version of it.

Until then: `--summary` collapses the wall to one line per rule,
`--rules` isolates one, and `--changed` shows what a change touched.

---

### 12 — Dependency licences are a separate list from fixture licences
Status: accepted.
Context: decision 11 fixed an allowlist of MIT, Apache-2.0, BSD-2, BSD-3 and
0BSD. That list governs **fixture data imported into our test suite** — a
directory of fake `package.json` and `.ts` files representing a tricky
resolution scenario, copied with a `LICENSE-3RD-PARTY` marker. It has never
governed test *code*, which is always clean-room reimplemented and never
copied.
`cargo-deny` enforces an allowlist over something unrelated: the ~230 crates
downloaded from crates.io and compiled into the archwarden binary. Both are
"a list of acceptable licences", which makes them easy to confuse, and the
confusion is expensive because the two lists cannot be the same. Restricting
dependencies to decision 11's five licences makes archwarden unbuildable.
Decision: `deny.toml` carries its own allowlist for the dependency graph:
the five from decision 11 plus three that no Rust dependency tree can avoid.

- `Unicode-3.0` — `unicode-ident`, which sits under `proc-macro2` and `syn`
  and therefore under every `#[derive(...)]` in the language. Without it there
  is no `#[derive(Deserialize)]`, and no way to read `arch.config.json`.
- `ISC` — functionally identical to MIT, shorter text. Used by several small
  utility crates.
- `Apache-2.0 WITH LLVM-exception` — the terms of the Rust standard library
  itself. The exception removes an attribution requirement when distributing
  compiled binaries, so it is *more* permissive for us than plain Apache-2.0.

Copyleft is excluded from **both** lists, MPL-2.0 included.
Alternatives:
- One shared list for fixtures and dependencies. Rejected: archwarden does not
  compile under decision 11's five, so a shared list means either an
  unbuildable project or a fixture policy loosened for reasons that have
  nothing to do with fixtures.
- No dependency allowlist at all. Rejected: a copyleft crate could then be
  linked into the distributed binary without anyone noticing, which is a
  licensing problem users would inherit from us.
- Allow MPL-2.0 among dependencies. Rejected for the same reason decision 10
  rejected MPL for archwarden's own licence: file-level copyleft inside a
  statically linked binary is a question no user should have to answer.
Consequences: two allowlists exist and must not be conflated. The fixture list
stays strict and lives in decision 11 and `TESTING.md`. The dependency list
lives in `deny.toml` and is enforced on every CI run. Adding a licence to
either one is a deliberate change, not a fix for a red build.

### 11 — Testing strategy: clean-room reimplementation of prior tests
Status: accepted.
Context: prior tools in this space (dependency-cruiser, ESLint
plugin-import, enhanced-resolve, oxc_resolver) encode decades of
edge-case knowledge in their test suites. Discarding that would leave
archwarden blind to edge cases the ecosystem has already learned. Copying
tests verbatim carries the original licence and creates a maintenance
liability tied to another project's evolution.
Decision: three-tier testing (unit, integration, differential). Cases
inspired by other projects are reimplemented clean-room: read the
original test, describe the behaviour in prose, write our own test
against our own fixtures, and cite the origin in a comment. Verbatim
copies (even translated to Rust) are forbidden. Third-party fixtures may
be imported only under MIT/Apache-2.0/BSD/0BSD with a `LICENSE-3RD-PARTY`
attribution file. Differential tests run archwarden against
dependency-cruiser on real repos to catch divergences without importing
their code. See [`TESTING.md`](TESTING.md).
Alternatives:
- Vendor other projects' test suites and run them under our runner.
  Rejected: pulls in their dependencies, couples our CI speed to theirs,
  and breaks the "one binary" story.
- Write all tests from scratch with no reference. Rejected: reinvents
  edge cases the ecosystem already documented. Would take years to reach
  parity on tsconfig-paths and workspace resolution alone.
- Line-by-line ports of other suites into Rust. Rejected: derivative
  works carry the origin licence and create a permanent audit burden.
Consequences: every test inspired by a reference source has a citation
comment. Fixture directories imported from other projects have a
`LICENSE-3RD-PARTY` marker. Differential tests need dep-cruiser
installed on nightly CI. We accept slightly slower initial test
coverage in exchange for tests we fully own and can evolve.

### 10 — Dual licence: MIT OR Apache-2.0
Status: accepted.
Context: archwarden ships as both a CLI (used by JS/TS shops) and a
Rust crate (published to crates.io). Each ecosystem has different
conventions: JS/TS defaults to MIT; Rust defaults to `MIT OR Apache-2.0`
dual. archwarden must fit both without friction.
Decision: dual-licensed under MIT and Apache-2.0. Users pick either at
their option. Contributions are dual-licensed by default.
Alternatives:
- MIT only. Rejected: no patent grant. Corporate adopters who care
  about patent risk (Apache preferred) would have to negotiate.
- Apache-2.0 only. Rejected: some JS-side users see Apache as heavier
  and default to MIT-licensed alternatives without checking.
- MPL 2.0. Rejected: file-level copyleft is fine for dep-cruiser
  (a monolithic Node tool) but interacts awkwardly with a Cargo
  workspace where our crates might be embedded.
- AGPL. Rejected: makes no sense for a CLI linter — nobody hosts a
  linter as a service.
Consequences: `Cargo.toml` will declare `license = "MIT OR Apache-2.0"`.
Both `LICENSE-MIT` and `LICENSE-APACHE` ship in the repo root. Contributors
are informed via README that submissions are dual-licensed.

### 9 — archwarden is an informant, not only a gate
Status: accepted.
Context: coding agents that only meet archwarden after writing a file
waste an iteration every time a rule fires. The rules exist; the agent
just did not know them in advance. A pure-gate tool captures failures
but does not prevent them.
Decision: archwarden ships four integration layers from v0
([`AGENT-INTEGRATION.md`](AGENT-INTEGRATION.md)):
`describe` and `scaffold` for pre-write queries, `agent-guide` for a
`CLAUDE.md`-referenced rule digest, and `install-hooks` for harness-side
pre-write enforcement. `check` remains the gate; the other commands
prevent the write from being wrong in the first place.
Alternatives:
- Ship only `check` and `explain` in v0, defer agent commands. Rejected:
  agents are the primary source of the violations these rules exist to
  catch. Shipping the gate without the informant means the tool is
  measured on its worst loop (write, fail, retry) rather than its best
  loop (ask, write correctly).
- Rewrite the agent's output when it fails a rule. Rejected: crosses
  from linting into refactoring. Different tool, different scope.
Consequences: every rule kind must implement a `describe_expectation()`
method so `scaffold` and `agent-guide` stay in lockstep with the
checker. archwarden owns `.archwarden/` in the repo for generated
artefacts (`AGENT_RULES.md`, cache). It never edits the user's
`CLAUDE.md` or `AGENTS.md` — the user references the generated file
themselves.

### 8 — Rust from the start
Status: accepted.
Context: target repos include one with ~30k files growing at ~1k files
per month. Cold-run performance and warm-cache watch latency need to
stay flat as the file count grows.
Decision: implement archwarden in Rust from v0. Distribute as a native
binary.
Alternatives:
- TypeScript first, port hot paths later. Rejected: two implementations,
  double the maintenance, and the Node startup cost alone would exceed
  the whole target budget on warm caches.
- Go. Rejected: adequate performance, but the JS/TS parser and resolver
  ecosystem in Rust (`oxc_*`) is significantly more mature than in Go.
Consequences: contributors need Rust knowledge. End users need none —
the binary is downloaded, not built. CI must run a matrix of build
targets. This is a bigger up-front cost that pays back on every run.

### 7 — Own the import graph; do not depend on dependency-cruiser
Status: accepted.
Context: import boundaries are one of the five core rule categories.
The user wants archwarden to be the sole tool (paired only with Biome),
which rules out delegating the graph to dependency-cruiser.
Decision: implement the import graph inside archwarden, using
`oxc_parser` to parse and `oxc_resolver` to resolve.
Alternatives:
- Delegate to dependency-cruiser and just consume its output.
  Rejected: adds a Node/npm dependency to the pipeline and defeats the
  "one binary" story.
- Write our own resolver. Rejected: TypeScript resolution semantics
  (paths, exports, conditional exports, workspace resolution) took the
  Node ecosystem years to get right. `oxc_resolver` already handles it.
Consequences: archwarden is coupled to `oxc_resolver` correctness for
edge cases in monorepos with exotic configs. Mitigated by the resolver
trait (see decision 6).

### 6 — Resolver and parser behind traits
Status: accepted.
Context: `oxc_*` crates are young. Betting the project on one specific
implementation with no seam to replace it is risky. Additionally,
extending archwarden to another language later requires a swappable
parser.
Decision: rule engines depend on extracted `FileFacts` only. A
`Resolver` trait and a `Parser` trait sit between the parsing stage
and the rule engines. Default impls use `oxc_parser` and
`oxc_resolver`.
Alternatives:
- Call `oxc_*` directly from rule code. Rejected: couples every rule
  to the parser version and blocks language expansion.
- Full abstract-syntax-agnostic core (visit any AST via a generic
  visitor). Rejected: over-engineered for v0 when only JS/TS is
  targeted.
Consequences: adding a language means implementing the two traits;
rule code needs no changes. Swapping the resolver later requires no
rule changes either.

### 5 — Config format is JSON, not YAML or TOML or JS
Status: accepted.
Context: config is data that both humans and coding agents edit.
Decision: JSON with a published JSON Schema referenced via `$schema`.
Alternatives:
- YAML. Rejected: significant whitespace + type coercion (`no` → false,
  version strings interpreted as floats) is a bug source in a config
  meant to reduce ambiguity.
- TOML. Rejected: fine format, but nested arrays of objects (which
  archwarden configs are dominated by) are awkward in TOML.
- JS/TS config. Rejected: executable configs are a supply-chain and
  reproducibility problem; the whole point of archwarden is that
  configs are declarative artefacts, not code.
Consequences: schema autocomplete works in every mainstream editor
without a plugin. Comments are impossible in strict JSON — accepted;
users can use description fields inside rules if they need to
document intent.

### 4 — Config discovery walks up from CWD
Status: accepted.
Context: users will run archwarden from arbitrary subdirectories in
large monorepos. Requiring a `--config` flag every time is friction.
Decision: search for `arch.config.json` upward from the CWD until
found or the filesystem root is reached. The first match wins.
`--config` overrides.
Alternatives:
- Require `--config` always. Rejected: friction.
- Recursively find *all* configs under the CWD and analyse each
  scope. Rejected: makes reproducibility unclear (which config
  wins on overlaps?) and does not match how tools like git and
  biome behave.
Consequences: one config per repo is the intended model. Sub-config
files may be supported later via `extends`, but the root file is
always the entry point.

### 3 — Cache is a v0 requirement, not a v1 feature
Status: accepted.
Context: the target repo grows ~1k files per month. Non-incremental
tools cross a usability line somewhere between 10k and 100k files.
Decision: content-addressed on-disk cache from v0. Key includes both
file content hash and the hash of the rules that apply to the file.
Alternatives:
- Skip caching until users complain. Rejected: retrofitting cache
  invalidation into a code base that never assumed it is painful and
  bug-prone. Building cache correctness in from the start is cheaper
  than adding it later.
Consequences: `.archwarden/cache/` is a build artefact and must be
gitignored. Cache format is versioned; incompatible changes bump the
version and invalidate all entries.

### 2 — No auto-fix in v0
Status: accepted.
Context: architectural violations are usually not mechanically fixable.
Moving a file to a new folder can break imports; renaming an export
requires updating callers; splitting a use-case that has grown too big
is a design decision.
Decision: archwarden reports only. Biome handles what is safely
fixable in the code-style space; archwarden stays in the report-only
space.
Alternatives:
- Ship trivial fixes (naming coupling could suggest a rename).
  Deferred to v2 with `--fix`.
Consequences: `explain` becomes the primary interactive affordance:
"why is this wrong, and what should it look like instead?".

### 1 — Two levels only: error and warning
Status: accepted.
Context: linters that ship three or four severity levels tend to see
"info" and "hint" ignored entirely, and users lose confidence that
warnings matter.
Decision: `error` and `warning` are the only levels. Errors fail CI;
warnings do not.
Alternatives:
- Add `info`. Rejected: encourages dumping-ground rules.
- Errors only. Rejected: legitimate need to track technical debt
  without blocking CI (e.g., new spec-pair rule with existing
  offenders that will be resolved incrementally).
Consequences: rule authors must decide up front whether a rule is a
gate or a signpost. That decision is often a healthy conversation.
