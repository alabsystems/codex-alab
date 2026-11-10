# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Coverage accounting must not turn subset or failed runs into full qualification."""
import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import weekly_reset


class CoverageReportTests(unittest.TestCase):
    def run_probe(self, root, selected, fail=None, interruption=None):
        executable = root / "fixture-codex"
        executable.write_bytes(b"fixture binary; never executed")
        output = root / "report"
        args = ["weekly_reset.py", "--codex", str(executable), "--output", str(output)]
        for name in selected:
            args.extend(["--case", name])

        def run_case(_executable, _output, name):
            if name == fail:
                if interruption is not None:
                    raise interruption
                raise RuntimeError("fixture recovery assertion failed")
            return {"case": name, "passed": True}

        with patch.object(sys, "argv", args), patch.object(weekly_reset, "case", run_case), \
                contextlib.redirect_stdout(io.StringIO()):
            code = weekly_reset.main()
        return code, json.loads((output / "result.json").read_text())

    def test_subset_success_is_distinct_from_complete_qualification(self):
        with tempfile.TemporaryDirectory() as directory:
            code, report = self.run_probe(Path(directory), ["late-credit"])
        self.assertEqual(code, 0)
        self.assertTrue(report["passed"])
        self.assertFalse(report["fullSuite"])
        self.assertEqual(report["requestedCases"], ["late-credit"])
        self.assertNotIn("failedCase", report)

    def test_failure_retains_requested_coverage_and_completed_cases(self):
        with tempfile.TemporaryDirectory() as directory:
            code, report = self.run_probe(Path(directory), ["late-credit", "lost-ack"], "lost-ack")
        self.assertEqual(code, 1)
        self.assertFalse(report["passed"])
        self.assertFalse(report["fullSuite"])
        self.assertEqual(report["requestedCases"], ["late-credit", "lost-ack"])
        self.assertEqual([case["case"] for case in report["cases"]], ["late-credit"])
        self.assertEqual(report["failedCase"], "lost-ack")
        self.assertIn("fixture recovery assertion failed", report["error"])

    def test_default_run_accounts_for_every_declared_case(self):
        with tempfile.TemporaryDirectory() as directory:
            code, report = self.run_probe(Path(directory), [])
        self.assertEqual(code, 0)
        self.assertTrue(report["fullSuite"])
        self.assertEqual(report["requestedCases"], list(weekly_reset.CASE_NAMES))
        self.assertEqual([case["case"] for case in report["cases"]], report["requestedCases"])

    def test_cancellation_keeps_incomplete_evidence_and_the_original_exit(self):
        for interruption in [KeyboardInterrupt(), SystemExit(143)]:
            with self.subTest(interruption=type(interruption).__name__), \
                    tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                with self.assertRaises(type(interruption)) as caught:
                    self.run_probe(root, ["late-credit", "lost-ack"], "lost-ack", interruption)
                self.assertIs(caught.exception, interruption)
                report = json.loads((root / "report/result.json").read_text())
                self.assertTrue(report["interrupted"])
                self.assertFalse(report["passed"])
                self.assertFalse(report["fullSuite"])
                self.assertEqual(report["failedCase"], "lost-ack")
                self.assertEqual([case["case"] for case in report["cases"]], ["late-credit"])

    def test_duplicate_selection_is_rejected_before_creating_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as caught:
                self.run_probe(root, ["late-credit", "late-credit"])
            self.assertEqual(caught.exception.code, 2)
            self.assertFalse((root / "report").exists())


class DeadlineOutageFixtureTests(unittest.TestCase):
    def test_clock_restoration_refuses_early_retry_and_keeps_the_backend_outage(self):
        for name in weekly_reset.DEADLINE_OUTAGE_CASES:
            with self.subTest(case=name), patch.object(weekly_reset.time, "time", return_value=1000):
                early, late = weekly_reset.Mock(name), weekly_reset.Mock(name)
                for mock in (early, late):
                    self.assertEqual(mock.reply("POST", "/v1/responses", {})[0], 200)
                    self.assertEqual(mock.reply("POST", "/v1/responses", {})[0], 429)
                    self.assertEqual(mock.reset, 1003)
                self.assertEqual(early.reply("POST", "/v1/responses", {})[0], 403)
                with patch.object(weekly_reset.time, "time", return_value=1003):
                    self.assertEqual(late.reply("POST", "/v1/responses", {})[0], 200)
                    endpoint = "usage" if name == "usage-outage-deadline" else "rate-limit-reset-credits"
                    self.assertEqual(late.reply("GET", "/backend-api/wham/" + endpoint, {})[0], 503)
                self.assertFalse(late.consumed)
                self.assertFalse(late.spent)


if __name__ == "__main__":
    unittest.main()
