#!/usr/bin/env python3
"""Tests for checksum.py.

A checksum file is read by strangers, on machines nobody here has, to decide
whether to trust a binary. Getting the format wrong fails their verification;
getting the digest wrong is worse.
"""

import hashlib
import importlib.util
import subprocess
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

HERE = Path(__file__).parent
SPEC = importlib.util.spec_from_file_location("checksum", HERE / "checksum.py")
checksum = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(checksum)


class ChecksumTests(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = TemporaryDirectory()
        self.dist = Path(self.scratch.name)
        self.name = "archwarden-1.2.3-x86_64-unknown-linux-gnu"
        (self.dist / f"{self.name}.tar.gz").write_bytes(b"pretend archive")

    def tearDown(self) -> None:
        self.scratch.cleanup()

    def test_the_digest_is_the_file_s_sha256(self) -> None:
        expected = hashlib.sha256(b"pretend archive").hexdigest()
        line = checksum.checksums_for(self.dist, self.name).strip()

        self.assertTrue(line.startswith(expected), line)

    def test_the_format_is_the_one_shasum_reads(self) -> None:
        """Two spaces, and the bare filename: `shasum -c` resolves the name
        against the working directory, so a path only verifies from where it
        was written."""
        line = checksum.checksums_for(self.dist, self.name).strip()
        digest, _, filename = line.partition("  ")

        self.assertEqual(len(digest), 64)
        self.assertEqual(filename, f"{self.name}.tar.gz")
        self.assertNotIn("/", filename)

    def test_shasum_itself_accepts_it(self) -> None:
        """The real check: hand the file to `shasum -c` and see it pass. A
        format this only agrees with itself about is a format that fails on a
        user's machine."""
        checksum.main.__globals__["sys"].argv = [
            "checksum.py",
            str(self.dist),
            self.name,
        ]
        checksum.main()

        verified = subprocess.run(
            ["shasum", "-a", "256", "-c", f"{self.name}.sha256"],
            cwd=self.dist,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(verified.returncode, 0, verified.stdout + verified.stderr)
        self.assertIn("OK", verified.stdout)

    def test_a_zip_is_covered_too(self) -> None:
        """Windows ships a zip, and a checksum file that quietly skipped it
        would leave one platform unverifiable."""
        windows = "archwarden-1.2.3-x86_64-pc-windows-msvc"
        (self.dist / f"{windows}.zip").write_bytes(b"pretend zip")

        self.assertIn(f"{windows}.zip", checksum.checksums_for(self.dist, windows))

    def test_nothing_to_checksum_is_refused(self) -> None:
        """An empty checksum file would publish as if everything were fine."""
        with self.assertRaises(SystemExit):
            checksum.checksums_for(self.dist, "archwarden-9.9.9-nope")

    def test_only_this_release_s_archives_are_included(self) -> None:
        """The directory holds every target's archive by the time this runs."""
        (self.dist / "archwarden-1.2.3-aarch64-apple-darwin.tar.gz").write_bytes(b"x")

        lines = checksum.checksums_for(self.dist, self.name).splitlines()
        self.assertEqual(len(lines), 1, lines)


if __name__ == "__main__":
    unittest.main()
