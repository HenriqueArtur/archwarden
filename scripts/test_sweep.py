#!/usr/bin/env python3
"""Tests for sweep.py.

This is the one script in the repository that causes a deletion, and it runs
unattended at the start of a session. Two things have to hold: it must do
nothing at all on a repository with nothing to sweep, and it must never fail in
a way that lands in front of somebody who did not ask for it.
"""

import importlib.util
import io
import os
import stat
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from tempfile import TemporaryDirectory

HERE = Path(__file__).parent
SPEC = importlib.util.spec_from_file_location("sweep", HERE / "sweep.py")
sweep = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sweep)


class LeftoverTests(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = TemporaryDirectory()
        self.root = Path(self.scratch.name) / "repo"
        self.temp = Path(self.scratch.name) / "temp"
        self.root.mkdir()
        self.temp.mkdir()

    def tearDown(self) -> None:
        self.scratch.cleanup()

    def test_a_clean_repository_has_nothing_to_sweep(self) -> None:
        self.assertEqual(sweep.leftovers_in(self.root, self.temp), [])

    def test_a_build_directory_is_a_leftover(self) -> None:
        (self.root / "target").mkdir()

        self.assertEqual(sweep.leftovers_in(self.root, self.temp), ["target"])

    def test_mutants_output_survives_a_deleted_target_and_is_still_found(self) -> None:
        # `cargo-mutants` writes this at the repository root, so `rm -rf target`
        # leaves it behind. It was 13 MB twice over on the machine that ran out
        # of disk.
        (self.root / "mutants.out").mkdir()
        (self.root / "mutants.out.old").mkdir()

        self.assertEqual(
            sweep.leftovers_in(self.root, self.temp),
            ["mutants.out", "mutants.out.old"],
        )

    def test_a_killed_mutants_run_leaves_a_whole_build_tree_nothing_else_collects(
        self,
    ) -> None:
        (self.temp / "cargo-mutants-archwarden-abc123").mkdir()

        self.assertEqual(
            sweep.leftovers_in(self.root, self.temp),
            ["cargo-mutants-archwarden-abc123"],
        )

    def test_a_directory_that_is_not_ours_is_not_our_business(self) -> None:
        # However large it is. The temporary directory belongs to the machine.
        (self.temp / "somebody-elses-build").mkdir()
        (self.temp / "cargo-mutants").mkdir()

        self.assertEqual(sweep.leftovers_in(self.root, self.temp), [])

    def test_a_temporary_directory_it_cannot_read_does_not_stop_the_sweep(self) -> None:
        (self.root / "target").mkdir()

        self.assertEqual(
            sweep.leftovers_in(self.root, Path("/nowhere/at/all")), ["target"]
        )


class SweepTests(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = TemporaryDirectory()
        self.root = Path(self.scratch.name) / "repo"
        self.temp = Path(self.scratch.name) / "temp"
        self.root.mkdir()
        self.temp.mkdir()
        self.path = os.environ["PATH"]

    def tearDown(self) -> None:
        # Restored, and not only for tidiness: a fake `cargo` left on the PATH
        # is a fake `cargo` for every test after this one, and which ones those
        # are depends on the order they happen to run in.
        os.environ["PATH"] = self.path
        self.scratch.cleanup()

    def fake_cargo(self, says: str) -> None:
        """A `cargo` on the PATH that prints `says` and exits clean.

        The sweep runs `cargo xtask clean` and decides what to report from what
        it said. Driving that with the real task would mean building it, and
        would tie a test about *reporting* to whatever this machine's `target/`
        happens to hold.
        """
        binary = self.root / "cargo"
        binary.write_text(f"#!/bin/sh\ncat <<'SAID'\n{says}\nSAID\n")
        binary.chmod(binary.stat().st_mode | stat.S_IEXEC)
        os.environ["PATH"] = f"{self.root}{os.pathsep}{self.path}"

    def test_nothing_to_sweep_says_nothing_at_all(self) -> None:
        # A hook that speaks every session is a hook somebody removes.
        written = io.StringIO()
        with redirect_stdout(written):
            code = sweep.sweep(self.root, self.temp)

        self.assertEqual(code, 0)
        self.assertEqual(written.getvalue(), "")

    def test_a_dry_run_names_what_it_found_and_removes_nothing(self) -> None:
        (self.root / "target").mkdir()

        written = io.StringIO()
        with redirect_stdout(written):
            code = sweep.sweep(self.root, self.temp, dry_run=True)

        self.assertEqual(code, 0)
        self.assertIn("target", written.getvalue())
        self.assertIn("would sweep", written.getvalue())
        self.assertTrue((self.root / "target").exists())

    def test_a_target_that_holds_nothing_dead_is_swept_in_silence(self) -> None:
        """The common case, and the one that has to say nothing.

        A `target/` that exists is not a `target/` worth taking: a tree holding
        only `debug/deps` is a warm cache the task correctly leaves alone. This
        put three lines into the context of every session before it was fixed,
        which is how a hook gets removed.
        """
        (self.root / "target").mkdir()
        self.fake_cargo(says=sweep.NOTHING_TAKEN)

        written = io.StringIO()
        with redirect_stdout(written):
            code = sweep.sweep(self.root, self.temp)

        self.assertEqual(code, 0)
        self.assertEqual(written.getvalue(), "")

    def test_what_it_actually_freed_is_reported(self) -> None:
        # "freed some space" is the kind of report this repository refuses
        # everywhere else, so the task's own numbers are passed straight
        # through rather than summarised.
        (self.root / "target").mkdir()
        self.fake_cargo(says="  target/debug/incremental  27.4 GB\n\n27.4 GB freed.")

        written = io.StringIO()
        with redirect_stdout(written):
            code = sweep.sweep(self.root, self.temp)

        self.assertEqual(code, 0)
        self.assertIn("27.4 GB freed.", written.getvalue())

    def test_a_cargo_that_will_not_run_is_reported_and_never_fails_the_session(
        self,
    ) -> None:
        # A session hook that exited non-zero over housekeeping would put an
        # error in front of somebody who did not ask for one. It still has to
        # *say* something: a sweep that silently did nothing is the third
        # answer this repository keeps refusing.
        (self.root / "mutants.out").mkdir()
        self.fake_cargo(says="error: no such command: `xtask`")

        written = io.StringIO()
        with redirect_stdout(written):
            code = sweep.sweep(self.root, self.temp)

        self.assertEqual(code, 0)
        self.assertIn("no such command", written.getvalue())

    def test_a_cargo_that_is_not_installed_at_all_is_not_a_failed_session(self) -> None:
        (self.root / "mutants.out").mkdir()
        os.environ["PATH"] = str(self.root)

        written = io.StringIO()
        with redirect_stdout(written):
            code = sweep.sweep(self.root, self.temp)

        self.assertEqual(code, 0)
        self.assertIn("could not run", written.getvalue())


class DriftTests(unittest.TestCase):
    """The names here have to be ones `cargo xtask clean` actually removes.

    This script decides *whether* to run that task and the task decides what to
    take, which is the split that keeps one list. A name here that the task
    does not know would trigger a sweep that removes nothing and reports
    success — the shape of failure this repository keeps finding.
    """

    def test_every_name_it_watches_for_is_one_the_task_removes(self) -> None:
        source = (HERE.parent / "xtask" / "src" / "clean.rs").read_text()

        for name in sweep.LEFTOVERS:
            self.assertIn(
                f'"{name}"',
                source,
                f"{name} is swept for here and not named in clean.rs",
            )

    def test_the_orphan_prefix_is_the_one_the_task_matches(self) -> None:
        source = (HERE.parent / "xtask" / "src" / "clean.rs").read_text()

        self.assertIn(f'"{sweep.ORPHAN_PREFIX}"', source)

    def test_the_sentence_it_reads_silence_from_is_one_the_task_prints(self) -> None:
        """The sentinel that decides whether a session hears anything.

        If `clean.rs` reworded this, the sweep would announce itself every
        session and nobody would know why — which is the shape of failure this
        repository keeps finding, in the place it is least expected.
        """
        source = (HERE.parent / "xtask" / "src" / "clean.rs").read_text()

        self.assertIn(sweep.NOTHING_TAKEN, source)


if __name__ == "__main__":
    unittest.main()
