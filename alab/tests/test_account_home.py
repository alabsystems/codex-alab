#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Account isolation uses the selected interpreter without trusting a scratch HOME."""

import os
from pathlib import Path
import pwd
import shlex
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class AccountHomeTests(unittest.TestCase):
    def lookup(self, root, interpreter):
        environment = dict(os.environ, HOME=str(root), ALAB_PYTHON=str(interpreter))
        return subprocess.run(
            ["/bin/bash", "-c", '. "$1/alab/lib.sh"; alab_account_home',
             "account-home", str(ROOT)],
            env=environment, capture_output=True, text=True, timeout=60,
        )

    def test_selected_interpreter_reads_the_account_home_despite_scratch_home(self):
        with tempfile.TemporaryDirectory(prefix="account-home-") as directory:
            root = Path(directory)
            called = root / "called"
            interpreter = root / "selected python"
            interpreter.write_text(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > " + shlex.quote(str(called))
                + "\nexec " + shlex.quote(sys.executable) + ' "$@"\n'
            )
            interpreter.chmod(0o755)
            result = self.lookup(root, interpreter)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), str(Path(pwd.getpwuid(os.getuid()).pw_dir).resolve()))
            self.assertEqual(called.read_text().splitlines()[:2], ["-I", "-c"])

    def test_a_refused_selected_interpreter_never_falls_back_to_the_system_one(self):
        with tempfile.TemporaryDirectory(prefix="account-home-") as directory:
            root = Path(directory)
            interpreter = root / "refused python"
            interpreter.write_text("#!/bin/sh\nexit 17\n")
            interpreter.chmod(0o755)
            result = self.lookup(root, interpreter)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")
            self.assertIn("cannot look up the current account home", result.stderr)


if __name__ == "__main__":
    unittest.main()
