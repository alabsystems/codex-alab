#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Retained unit output is admitted before check.sh's first compiler boundary."""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
STOP_STATUS = 75
CHECK_BODY = r"""
. "$1/alab/check.sh"
alab_build_command() {
  printf '%s\n' "$*" >"$ALAB_FAKE_COMPILER_MARKER"
  return 75
}
check_main
"""


@unittest.skipUnless(shutil.which("jq") and shutil.which("git"), "requires jq and git")
class CheckResultsPreflightTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="alab-check-results-")
        self.addCleanup(self.directory.cleanup)
        self.scratch = Path(os.path.realpath(self.directory.name))
        self.repo = self.scratch / "repo"
        (self.repo / "alab").mkdir(parents=True)
        (self.repo / "codex-rs").mkdir()
        for name in ("check.sh", "lib.sh", "retention.py"):
            shutil.copyfile(ROOT / "alab" / name, self.repo / "alab" / name)
        self.marker = self.scratch / "compiler-called"
        self.environment = {
            key: value for key, value in os.environ.items()
            if not key.startswith(("ALAB_", "CARGO_", "GIT_", "CODEX_"))
        }
        self.environment.update(
            ALAB_CARGO="fixture-compiler-must-not-run",
            ALAB_PYTHON=sys.executable,
            ALAB_FAKE_COMPILER_MARKER=str(self.marker),
            GIT_CONFIG_GLOBAL="/dev/null",
            GIT_CONFIG_NOSYSTEM="1",
            GIT_AUTHOR_NAME="t",
            GIT_AUTHOR_EMAIL="t@example.invalid",
            GIT_COMMITTER_NAME="t",
            GIT_COMMITTER_EMAIL="t@example.invalid",
        )
        self.git("init", "-q", "-b", "main")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "fixture")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")

    def git(self, *args):
        subprocess.run(
            ["git", "-C", str(self.repo), *args],
            env=self.environment, check=True, capture_output=True, timeout=60,
        )

    def check(self, output):
        return subprocess.run(
            ["/bin/bash", "-c", CHECK_BODY, "check-results", str(self.repo)],
            cwd=self.repo,
            env=dict(self.environment, ALAB_UNIT_RESULTS_DIR=str(output)),
            capture_output=True, text=True, timeout=60,
        )

    def assert_refused_before_compiler(self, output, reason):
        result = self.check(output)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn(reason, result.stderr)
        self.assertFalse(self.marker.exists(), "refusal precedes the first compiler boundary")

    def test_existing_output_is_preserved_before_compiler(self):
        existing_file = self.scratch / "existing-file"
        existing_file.write_bytes(b"preserve this file")
        existing_directory = self.scratch / "existing-directory"
        existing_directory.mkdir()
        sentinel = existing_directory / "sentinel"
        sentinel.write_bytes(b"preserve this evidence")
        existing_link = self.scratch / "existing-link"
        existing_link.symlink_to(existing_directory, target_is_directory=True)
        for output in (existing_file, existing_directory, existing_link):
            with self.subTest(output=output.name):
                self.assert_refused_before_compiler(
                    output, "cannot create fresh unit-results directory",
                )
                self.assertEqual(existing_file.read_bytes(), b"preserve this file")
                self.assertEqual(sentinel.read_bytes(), b"preserve this evidence")
                self.assertEqual(list(existing_directory.iterdir()), [sentinel])
                self.assertTrue(existing_link.is_symlink())
                self.assertEqual(existing_link.readlink(), existing_directory)

    def test_missing_parent_refuses_before_compiler(self):
        output = self.scratch / "missing-parent" / "results"
        self.assert_refused_before_compiler(
            output, "cannot create fresh unit-results directory",
        )
        self.assertFalse(output.parent.exists(), "preflight does not create output ancestors")

    def test_relative_output_refuses_before_compiler(self):
        self.assert_refused_before_compiler(
            "relative-results", "ALAB_UNIT_RESULTS_DIR must be absolute",
        )
        self.assertFalse((self.repo / "relative-results").exists())

    def test_fresh_output_is_created_before_compiler(self):
        output = self.scratch / "fresh-results"
        result = self.check(output)
        self.assertEqual(result.returncode, STOP_STATUS, result.stderr)
        self.assertTrue(output.is_dir())
        self.assertEqual(output.stat().st_mode & 0o777, 0o700)
        self.assertEqual(list(output.iterdir()), [], "the stub stops before unit execution")
        self.assertIn("metadata", self.marker.read_text())


if __name__ == "__main__":
    unittest.main()
