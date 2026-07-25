#!/usr/bin/env python3
"""Write a release's version and checksums into the Homebrew formula.

A formula whose checksums are edited by hand is a formula that eventually
ships the wrong ones. The workflow that built the archives is the only thing
that knows their hashes, so it is what writes them.

Refuses to leave a placeholder behind: a formula published with a zeroed
checksum would fail for every user, at install time, with a message about
integrity rather than about a release that was never stamped.

    stamp-formula.py <formula> <version> <checksum-dir>
"""

import re
import sys
from pathlib import Path

PLACEHOLDER = "0" * 64

# The targets Homebrew serves. The release builds more than this -- musl and
# Windows -- which Homebrew has no use for.
TARGETS = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
]


def checksum_for(directory: Path, version: str, target: str) -> str:
    """The archive's sha256, from the file the release published beside it."""
    path = directory / f"archwarden-{version}-{target}.sha256"
    first_line = path.read_text(encoding="utf-8").strip().splitlines()[0]
    digest = first_line.split()[0]
    if not re.fullmatch(r"[0-9a-f]{64}", digest):
        raise SystemExit(f"{path} does not start with a sha256: {first_line!r}")
    return digest


def stamp(formula: str, version: str, directory: Path) -> str:
    """The formula with its version and every checksum written in."""
    stamped = re.sub(
        r'^(  version ")[^"]*(")$',
        rf"\g<1>{version}\g<2>",
        formula,
        count=1,
        flags=re.MULTILINE,
    )
    if f'version "{version}"' not in stamped:
        raise SystemExit("no `version` line to stamp")

    for target in TARGETS:
        digest = checksum_for(directory, version, target)
        pattern = re.compile(
            r'(url "[^"]*' + re.escape(target) + r'\.tar\.gz"\s*\n\s*sha256 ")'
            + PLACEHOLDER
            + r'(")'
        )
        stamped, count = pattern.subn(rf"\g<1>{digest}\g<2>", stamped)
        if count != 1:
            raise SystemExit(
                f"expected one placeholder for {target}, replaced {count}"
            )

    if PLACEHOLDER in stamped:
        raise SystemExit("a checksum placeholder survived stamping")

    return stamped


def main() -> None:
    if len(sys.argv) != 4:
        raise SystemExit(__doc__)

    formula = Path(sys.argv[1])
    version = sys.argv[2].removeprefix("v")
    directory = Path(sys.argv[3])

    formula.write_text(
        stamp(formula.read_text(encoding="utf-8"), version, directory),
        encoding="utf-8",
    )
    print(f"stamped {formula} for {version}")


if __name__ == "__main__":
    main()
