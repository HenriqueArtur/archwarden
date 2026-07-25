# Known divergences from dependency-cruiser

Every entry here is a place where archwarden and `dependency-cruiser`
disagree about a repository's import graph, and archwarden is the one we
have decided is right — or at least deliberately different.

The differential harness (`cargo test -p archwarden-engine --features
differential`) reads this file. A heading of the shape below silences that
one divergence; anything not listed fails the test.

```
### `<importer path>` -> `<specifier as written>`
```

Add an entry only after deciding which of the three cases applies
(`docs/TESTING.md`, Tier 3):

1. archwarden is wrong — fix archwarden, do not add an entry;
2. dependency-cruiser is wrong, or asks a different question — add an entry
   with the reasoning below it;
3. the configuration on either side is ambiguous — fix the configuration.

An entry with no rationale is not an entry. The whole value of this file is
that a future reader can tell a decision from an oversight.

---

## Whole-class divergences

These are not per-edge and cannot be listed as headings. The harness applies
them as filters, and they are written out here because a filter in code with
no rationale is indistinguishable from a bug.

### Re-export statements *are* import edges — both sides agree

`export * from './user.entity'` creates an edge in dependency-cruiser, typed
`export`. archwarden's parser records the same edge as an `ImportFact`,
deliberately: a file that re-exports from a layer depends on that layer, and
arguably depends on it harder, because it republishes it under its own name.

This entry exists because the first version of this file claimed the opposite,
and the harness caught it on its first run. Nothing is filtered here. The note
stays as a record of a wrong assumption that documentation held and code did
not.

### Only in-repository edges are compared

An edge that lands in `node_modules`, above the repository root, on a runtime
builtin, or on nothing at all has no repo-relative path, and archwarden
deliberately keeps only in-repository paths on a fact (see M5b in
`docs/PLAN-V0.md`). Both sides are filtered to edges that resolve inside the
repository before comparing.

This means the harness does not differentiate resolution *into* dependencies.
That is `oxc_resolver`'s territory and it has its own suite.

### A specifier dependency-cruiser could not place is not a contradiction

When dependency-cruiser reports `couldNotResolve` for a specifier, it is
admitting ignorance, not asserting that no edge exists. Diffing against an
admission of ignorance produces noise, not signal, so an edge archwarden
resolved and dependency-cruiser gave up on is printed as a note rather than
failed on.

The reverse still fails: an edge dependency-cruiser *placed* and archwarden
did not see is a real gap, and so is the same pair landing in two different
files.

Observed instance: an `exports` subpath in a workspace package —
`import { X } from '@org/domain/types'` where `@org/domain` is a symlink into
`packages/` and its `package.json` maps `"./types"` to a file. archwarden
follows the map and the symlink and lands on the source; dependency-cruiser
resolves the bare `@org/domain` but not the subpath, in both 17.4.3 and 18.1.0.

On a real monorepo this is not a corner: a package publishing
`"./address/*": "./src/address/*.ts"` produced 93 such edges in one package
alone, every one of them invisible to the reference tool.

---

## Per-edge divergences

_(none recorded yet)_
