<!--
Delete any section that does not apply. An empty section is noise; a deleted
one is a decision.

CONTRIBUTING.md has the full checklist. This is the part a reviewer reads.
-->

## What this changes

<!-- The behaviour, not the diff. What can someone do now that they could not? -->

Closes #

## Why

<!--
The part that matters and the part that survives into the commit body. If there
is a measurement behind this, put the number here. If it was a trade, name what
lost.
-->

## What a reviewer should be suspicious of

<!--
The most useful paragraph in this template. Where is this most likely to be
wrong? What did you decide without being sure? What is the edge case you
thought about longest?

If you left something out of scope, say so here. An explicit gap is a decision;
a silent one is a bug someone else finds later.
-->

## Does this change what an existing, unchanged config reports?

<!--
Yes / No. If yes: say what starts or stops being reported, and add it to
CHANGELOG.md under Unreleased -> Changed with a note saying so. This is the
line that breaks somebody's build, and it does not track semver.
-->

---

- [ ] Tests written first, and they fail without the change
- [ ] `cargo mutants --in-diff` leaves no survivors — or a survivor is left alive with a comment saying why it is harmless
- [ ] Coverage floors still pass (`archwarden-core` 99% lines / 100% functions, workspace 95% lines) — never lowered to make a red build green
- [ ] `cargo xtask gen-schema` run and committed, if config types changed
- [ ] `AGENTS.md` updated, if any command's output changed — it ships inside the npm package and agents read it in the field
- [ ] `docs/RULES.md` / `docs/CONFIG.md` updated, if rule or config semantics changed
- [ ] `CHANGELOG.md` updated under `Unreleased`
- [ ] `docs/DECISIONS.md` entry added, if this locks the project into something or declines something a reasonable person would expect
