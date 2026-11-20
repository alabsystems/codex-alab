# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The native smoke fixture must own every directory it removes."""

from contextlib import redirect_stderr, redirect_stdout
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import native


class NativeSmokeScratchTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name).resolve()
        self.scratch = self.root / ("long-" * 13)
        self.missing = self.root / "missing-executable"

    def run_fixture(self, *options):
        argv = ["native.py", "--codex", str(self.missing), "--next-codex", str(self.missing),
                "--fake", str(self.missing), *options]
        with mock.patch.object(sys, "argv", argv), mock.patch.object(tempfile, "tempdir", str(self.root)), \
                redirect_stderr(io.StringIO()), redirect_stdout(io.StringIO()):
            native.main()

    def test_existing_directory_and_symlink_are_refused_without_touching_contents(self):
        self.scratch.mkdir()
        marker = self.scratch / "caller-evidence"
        marker.write_bytes(b"keep")
        link = self.root / "link"
        link.symlink_to(self.scratch, target_is_directory=True)
        for path in (self.scratch, link):
            with self.subTest(path=path), self.assertRaises(SystemExit) as failure:
                self.run_fixture("--scratch", str(path))
            self.assertEqual(2, failure.exception.code)
            self.assertEqual([marker], list(self.scratch.iterdir()))
            self.assertEqual(b"keep", marker.read_bytes())
        self.assertTrue(link.is_symlink())

    def test_early_setup_failure_releases_socket_and_owned_scratch(self):
        for keep in (False, True):
            with self.subTest(keep=keep), self.assertRaises(FileNotFoundError):
                self.run_fixture("--scratch", str(self.scratch), *(["--keep"] if keep else []))
            self.assertEqual(keep, self.scratch.exists())
            self.assertEqual([], list(self.root.glob("alab-x-*")))

    def test_replaced_scratch_directory_survives_cleanup(self):
        def replace(_args, scratch, _xdg, _cleanup):
            original = Path(scratch)
            original.rename(self.root / "moved")
            original.mkdir()
            (original / "caller-marker").touch()

        with mock.patch.object(native, "run_probe", replace):
            self.run_fixture("--scratch", str(self.scratch))
        self.assertTrue((self.scratch / "caller-marker").is_file())
        self.assertEqual([], list(self.root.glob("alab-x-*")))

    def test_completed_probe_cleans_socket_and_keeps_receipt_only_when_requested(self):
        def finish(_args, scratch, xdg, _cleanup):
            self.assertEqual(self.scratch, Path(scratch))
            self.assertEqual(self.root, Path(xdg).parent)
            self.assertTrue(Path(xdg).is_dir())
            (Path(scratch) / "live-update-receipt.json").write_text("receipt")

        with mock.patch.object(native, "run_probe", finish):
            for keep in (False, True):
                with self.subTest(keep=keep):
                    self.run_fixture("--scratch", str(self.scratch), *(["--keep"] if keep else []))
                    self.assertEqual(keep, self.scratch.exists())
                    self.assertEqual([], list(self.root.glob("alab-x-*")))
            self.assertEqual("receipt", (self.scratch / "live-update-receipt.json").read_text())


if __name__ == "__main__":
    unittest.main()
