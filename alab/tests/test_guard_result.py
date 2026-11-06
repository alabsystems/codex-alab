#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Raw command evidence distinguishes signals and timeouts from ordinary failure."""

import importlib.util
import json
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("result_guard", ROOT / "alab/command_guard.py")
GUARD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GUARD)


class GuardResultTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="alab-guard-result-")).resolve()
        print(f"retained raw-result fixture: {self.root}", flush=True)

    def run_recorded(self, source, timeout=10):
        command = [sys.executable, "-I", "-B", "-c", source]
        record = self.root / "result.json"
        code = GUARD.run_inherited(command, timeout=timeout, result_file=record)
        result = json.loads(record.read_text())
        self.assertEqual(result["schema"], 1)
        self.assertEqual(result["argv"], command)
        self.assertEqual(result["timeout_seconds"], timeout)
        return code, result

    def test_normal_nonzero_and_timeout_shaped_exit_are_not_signals_or_timeouts(self):
        for code in (0, 23, 124, 143):
            with self.subTest(code=code):
                record = self.root / f"result-{code}.json"
                command = [sys.executable, "-I", "-B", "-c", f"raise SystemExit({code})"]
                self.assertEqual(GUARD.run_inherited(command, timeout=10, result_file=record), code)
                result = json.loads(record.read_text())
                self.assertEqual(result["returncode"], code)
                self.assertIs(result["timed_out"], False)

    def test_actual_signal_keeps_raw_negative_status_beside_mapped_guardian_exit(self):
        code, result = self.run_recorded("import os, signal; os.kill(os.getpid(), signal.SIGTERM)")
        self.assertEqual(code, 128 + signal.SIGTERM)
        self.assertEqual(result["returncode"], -signal.SIGTERM)
        self.assertIs(result["timed_out"], False)

    def test_actual_deadline_has_no_ordinary_raw_exit(self):
        code, result = self.run_recorded("import time; time.sleep(10)", timeout=0.05)
        self.assertEqual(code, 124)
        self.assertIsNone(result["returncode"])
        self.assertIs(result["timed_out"], True)

    def test_existing_result_is_preserved_and_refused_before_command_starts(self):
        record = self.root / "result.json"
        record.write_bytes(b"existing evidence\n")
        ran = self.root / "command-ran"
        with self.assertRaises(FileExistsError):
            GUARD.run_inherited([sys.executable, "-I", "-B", "-c",
                                 f"from pathlib import Path; Path({str(ran)!r}).touch()"],
                                timeout=10, result_file=record)
        self.assertFalse(ran.exists())
        self.assertEqual(record.read_bytes(), b"existing evidence\n")

    def test_result_option_cannot_launch_outside_inherited_timeout_mode(self):
        record = self.root / "result.json"
        ran = self.root / "command-ran"
        command = [sys.executable, "-I", str(ROOT / "alab/command_guard.py"),
                   "--result-file", str(record), "--", sys.executable, "-I", "-c",
                   f"from pathlib import Path; Path({str(ran)!r}).touch()"]
        done = subprocess.run(command, capture_output=True, text=True, timeout=10)
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("requires --inherited-group-timeout", done.stderr)
        self.assertFalse(ran.exists())
        self.assertFalse(record.exists())


if __name__ == "__main__":
    unittest.main()
