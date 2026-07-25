#!/usr/bin/env python3
"""Tests for stamp-formula.py.

The script edits a file nobody reads until it is too late: a wrong checksum
surfaces as an integrity failure on a stranger's machine, days later. So the
refusals matter more than the happy path.
"""

import importlib.util
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

HERE = Path(__file__).parent
SPEC = importlib.util.spec_from_file_location("stamp", HERE / "stamp-formula.py")
stamp_module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stamp_module)

PLACEHOLDER = "0" * 64

FORMULA = """class Archwarden < Formula
  version "0.0.0"

  on_macos do
    on_arm do
      url "https://example/v#{version}/archwarden-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "%(zero)s"
    end
    on_intel do
      url "https://example/v#{version}/archwarden-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "%(zero)s"
    end
  end

  on_linux do
    on_arm do
      url "https://example/v#{version}/archwarden-#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "%(zero)s"
    end
    on_intel do
      url "https://example/v#{version}/archwarden-#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "%(zero)s"
    end
  end
end
""" % {"zero": PLACEHOLDER}


def digest_for(target: str) -> str:
    """A distinct, well-formed sha256 per target."""
    return (f"{abs(hash(target)):x}" * 16)[:64].replace("-", "a")


class StampTests(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = TemporaryDirectory()
        self.dist = Path(self.scratch.name)
        for target in stamp_module.TARGETS:
            (self.dist / f"archwarden-1.2.3-{target}.sha256").write_text(
                f"{digest_for(target)}  archwarden-1.2.3-{target}.tar.gz\n",
                encoding="utf-8",
            )

    def tearDown(self) -> None:
        self.scratch.cleanup()

    def test_every_target_gets_its_own_checksum(self) -> None:
        """The commonest way to get this wrong is to write one hash into all
        four slots, which nothing downstream would notice until three of the
        four platforms failed."""
        stamped = stamp_module.stamp(FORMULA, "1.2.3", self.dist)

        for target in stamp_module.TARGETS:
            expected = digest_for(target)
            url_line = next(
                line for line in stamped.splitlines() if target in line
            )
            index = stamped.index(url_line)
            following = stamped[index : index + 400]
            self.assertIn(expected, following, f"{target} kept its own hash")

        self.assertIn('version "1.2.3"', stamped)
        self.assertNotIn(PLACEHOLDER, stamped)

    def test_a_surviving_placeholder_is_refused(self) -> None:
        """A formula published with a zeroed checksum fails for every user,
        with a message about integrity rather than about a release nobody
        stamped."""
        extra = FORMULA.replace(
            "end\n",
            f'    url "https://example/archwarden-1.2.3-riscv64.tar.gz"\n'
            f'    sha256 "{PLACEHOLDER}"\n  end\n',
            1,
        )
        with self.assertRaises(SystemExit):
            stamp_module.stamp(extra, "1.2.3", self.dist)

    def test_a_missing_target_is_refused(self) -> None:
        """A formula that lost a platform's url should stop the release, not
        quietly ship without it."""
        without_linux_arm = FORMULA.replace("aarch64-unknown-linux-gnu", "nope")
        with self.assertRaises(SystemExit):
            stamp_module.stamp(without_linux_arm, "1.2.3", self.dist)

    def test_a_checksum_file_that_is_not_one_is_refused(self) -> None:
        """A truncated or reordered checksum file must not become a hash."""
        (self.dist / "archwarden-1.2.3-aarch64-apple-darwin.sha256").write_text(
            "not a checksum at all\n", encoding="utf-8"
        )
        with self.assertRaises(SystemExit):
            stamp_module.stamp(FORMULA, "1.2.3", self.dist)

    def test_the_real_formula_stamps(self) -> None:
        """Against the formula this repository actually ships, not a fixture:
        the pattern has to match its real indentation and quoting, and a
        fixture that drifted from it would pass while the release broke."""
        real = (HERE.parent / "Formula" / "archwarden.rb").read_text(encoding="utf-8")
        stamped = stamp_module.stamp(real, "1.2.3", self.dist)

        self.assertNotIn(PLACEHOLDER, stamped)
        self.assertIn('version "1.2.3"', stamped)
        for target in stamp_module.TARGETS:
            self.assertIn(digest_for(target), stamped, target)

    def test_a_missing_checksum_file_is_refused(self) -> None:
        (self.dist / "archwarden-1.2.3-x86_64-apple-darwin.sha256").unlink()
        with self.assertRaises(FileNotFoundError):
            stamp_module.stamp(FORMULA, "1.2.3", self.dist)


if __name__ == "__main__":
    unittest.main()
