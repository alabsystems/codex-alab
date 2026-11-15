# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Coverage accounting must not turn subset or failed runs into full qualification."""
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
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


class RedemptionPolicyFixtureTests(unittest.TestCase):
    def test_positive_preflights_offer_99_percent_within_24_hours(self):
        for name in weekly_reset.PREFLIGHT_CASES:
            with self.subTest(case=name), patch.object(weekly_reset.time, "time", return_value=1000):
                mock = weekly_reset.Mock(name)
                self.assertEqual(mock.reply("GET", "/backend-api/wham/usage", None)[0], 200)
                self.assertEqual(mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                                           {"redeem_request_id": "a" * 64,
                                            "credit_id": "earned-weekly-credit"})[0], 200)
                self.assertTrue(mock.consumed)
                windows = mock.requests[0]["response"]["rate_limit"]
                self.assertTrue(any(window["used_percent"] == 99 for window in
                                    (windows["primary_window"], windows["secondary_window"])))
                if name == "preflight-weekly-24h":
                    self.assertEqual(windows["secondary_window"]["reset_at"] - 1000, 86400)

    def test_negative_cases_keep_a_credit_and_refuse_consumption(self):
        for name in weekly_reset.POLICY_BLOCKED_CASES:
            for models in (0, 2):
                with self.subTest(case=name, models=models), \
                        patch.object(weekly_reset.time, "time", return_value=1000):
                    mock = weekly_reset.Mock(name)
                    mock.models = [{} for _ in range(models)]
                    self.assertTrue(mock.credit)
                    self.assertEqual(mock.reply("GET", "/backend-api/wham/usage", None)[0], 200)
                    if name == "preflight-below-99":
                        self.assertEqual(mock.requests[0]["response"]["rate_limit"]["primary_window"]["used_percent"], 95)
                    with self.assertRaisesRegex(RuntimeError, "redemption policy"):
                        mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                                   {"redeem_request_id": "a" * 64,
                                    "credit_id": "earned-weekly-credit"})
                    self.assertFalse(mock.consumed)
                    self.assertFalse(mock.spent)


class IsolatedExecWrapperTests(unittest.TestCase):
    def test_isolated_wrapper_execs_exact_arguments_and_records_its_native_pid(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pidfile = root / "native.pid"
            program = ("import json, os, sys; "
                       "print(json.dumps({'pid':os.getpid(), 'argv':sys.argv, "
                       "'isolated':sys.flags.isolated, 'noBytecode':sys.dont_write_bytecode})); "
                       "sys.exit(7)")
            result = subprocess.run(
                [sys.executable, "-I", "-B", str(Path(weekly_reset.__file__).resolve()),
                 "--exec-wrapper", str(pidfile), sys.executable, "-I", "-B", "-c", program,
                 "one argument", "literal-$()"],
                cwd=root, env={"PATH": os.defpath}, capture_output=True, text=True, check=False,
            )
            self.assertEqual(result.returncode, 7, result.stderr)
            executed = json.loads(result.stdout)
            self.assertEqual(executed["pid"], int(pidfile.read_text()))
            self.assertEqual(executed["argv"], ["-c", "one argument", "literal-$()"])
            self.assertEqual(executed["isolated"], 1)
            self.assertTrue(executed["noBytecode"])


if __name__ == "__main__":
    unittest.main()
