#!/usr/bin/env python3
"""Take out the build artefacts a finished session left behind.

    sweep.py [--root <path>] [--temp <path>] [--dry-run]

# Why this exists

A session filled the disk and froze the machine it was running on, and the
recovery was deleting `target/` by hand to free enough space to save the
session's own transcript. `cargo xtask clean` measured the shape of it once:
`target` at 59 GB, of which `debug/incremental` was 27 and `debug/deps` was 28.

Nothing swept it, because nothing ran between sessions.

# What it does not do

**It does not decide what is dead.** `cargo xtask clean` owns that list, and
this runs it. A second list here would be a second thing to keep in step, and a
sweep that disagreed with the task it is named after would take something
nobody meant — in the one place in this repository that deletes.

# What it costs

Nothing on a repository with nothing to sweep: the checks below are three
`exists()` calls, and on a tree with no `target/` and no `mutants.out` it
returns without building anything.

When there *is* something, it runs `cargo xtask clean`, which compiles `xtask`
first if that is not already built. That build is worth paying for exactly when
there is something to delete, which is the condition it is under.

The default tier keeps `deps`, so the next build is still warm.

# A limit worth stating

It deletes incremental state, and a second session building in the same
repository at that moment would lose it mid-build. Nothing here detects that.
For one person's machine that is the right trade; for a shared checkout it is
not, and `--dry-run` is how to see what it would take first.
"""

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

# What tells us a previous run left something. Cheap on purpose: three
# filesystem checks, not a walk. `cargo xtask clean` reads the authoritative
# list and reports what it actually took.
#
# `target/` covers the caches; `mutants.out` survives a `rm -rf target` because
# `cargo-mutants` writes it at the repository root; and a killed mutants run
# leaves a whole build tree under the system temporary directory, which is the
# one nothing else will ever collect.
LEFTOVERS = ("target", "mutants.out", "mutants.out.old")

# The prefix `cargo-mutants` gives its scratch trees. Matched here only to
# decide *whether* to run the task; the task itself is what removes them.
ORPHAN_PREFIX = "cargo-mutants-"

# What `cargo xtask clean` says when there was nothing worth taking.
#
# `target/` existing is not the same as `target/` holding anything dead: a tree
# with only `debug/deps` in it is a warm cache and the task correctly leaves it
# alone. Watching for the presence of `target/` is how this decides to *look*;
# this sentence is how it learns it found nothing, and stays quiet.
#
# Coupled to the task's own wording on purpose, and pinned by a test in
# `test_sweep.py` — the alternative is a second way to ask the same question.
NOTHING_TAKEN = "nothing to remove."


def leftovers_in(root: Path, temp: Path) -> list[str]:
    """What a previous run left, by name, for a caller to report.

    Names rather than sizes: measuring means walking tens of thousands of
    files, and the decision this feeds is only whether to run the task that
    measures properly.
    """
    found = [name for name in LEFTOVERS if (root / name).exists()]

    try:
        orphans = [
            entry.name
            for entry in temp.iterdir()
            if entry.is_dir() and entry.name.startswith(ORPHAN_PREFIX)
        ]
    except OSError:
        # An unreadable temporary directory is not this script's problem, and
        # refusing to sweep the repository over it would be the wrong half to
        # give up on.
        orphans = []

    return found + sorted(orphans)


def sweep(root: Path, temp: Path, dry_run: bool = False) -> int:
    """Runs the sweep, and returns what to exit with.

    Always zero. This is called from a session hook, and a non-zero exit there
    puts an error in front of somebody who did not ask for one — over a
    housekeeping task that is allowed to fail.
    """
    found = leftovers_in(root, temp)
    if not found:
        # Silence. A hook that speaks every session is a hook somebody removes,
        # which is the same argument the stop hook is built on.
        return 0

    if dry_run:
        print(f"archwarden: would sweep ({', '.join(found)})")
        return 0

    try:
        result = subprocess.run(
            ["cargo", "xtask", "clean"],
            cwd=root,
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError as error:
        print(f"archwarden: could not run `cargo xtask clean` — {error}")
        return 0

    said = result.stdout.rstrip() or result.stderr.rstrip()

    # The common case, and the one that has to be silent: a `target/` that
    # exists and holds nothing dead. Reported before this was written, and it
    # put three lines into the context of every session for nothing.
    if not said or NOTHING_TAKEN in said:
        return 0

    print("archwarden: swept build artefacts a previous session left behind")
    print(said)
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--temp", type=Path, default=Path(tempfile.gettempdir()))
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args(argv)

    return sweep(args.root, args.temp, args.dry_run)


if __name__ == "__main__":
    sys.exit(main())
