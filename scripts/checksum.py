#!/usr/bin/env python3
"""Write a `shasum`-compatible checksum file for the archives in a directory.

Not `shasum -a 256`: the release matrix includes a Windows runner, and what
that has under `shell: bash` depends on what Git for Windows happened to
install. `python3` is on every GitHub runner, and this produces the same format
`shasum -c` reads, so the file works wherever a user verifies it.

    checksum.py <directory> <name>
"""

import hashlib
import sys
from pathlib import Path

# 1 MiB. The archives are a few megabytes; reading them whole would work, and
# streaming costs nothing to write.
CHUNK = 1024 * 1024


def digest(path: Path) -> str:
    """The file's sha256, as lowercase hex."""
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(CHUNK):
            hasher.update(chunk)
    return hasher.hexdigest()


def checksums_for(directory: Path, name: str) -> str:
    """A checksum line per archive, in the format `shasum -c` reads.

    Two spaces between digest and filename, and the filename alone rather than
    a path: `shasum -c` resolves it against the working directory, and a path
    would only verify from the directory it was written in.
    """
    archives = sorted(
        path
        for path in directory.iterdir()
        if path.name.startswith(name) and path.suffix in {".gz", ".zip"}
    )
    if not archives:
        raise SystemExit(f"no archive named `{name}*` in {directory}")

    return "".join(f"{digest(path)}  {path.name}\n" for path in archives)


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)

    directory, name = Path(sys.argv[1]), sys.argv[2]
    (directory / f"{name}.sha256").write_text(
        checksums_for(directory, name), encoding="utf-8"
    )


if __name__ == "__main__":
    main()
