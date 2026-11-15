# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Coverage accounting must not turn subset or failed runs into full qualification."""
import contextlib
import http.client
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


class PrePostFixtureTests(unittest.TestCase):
    def test_selected_credit_does_not_freeze_usage_eligibility(self):
        for name in weekly_reset.PREPOST_CASES:
            with self.subTest(case=name), patch.object(weekly_reset.time, "time", return_value=1000):
                mock = weekly_reset.Mock(name)
                mock.models = [{}, {}] if name != "prepost-preflight-99" else []
                before = mock.usage()
                self.assertEqual(before["account_id"], weekly_reset.ACCOUNT)
                self.assertEqual(mock.post_selection_reads, 0)
                mock.reply("GET", "/backend-api/wham/rate-limit-reset-credits", None)
                fresh = mock.usage()
                self.assertEqual(mock.post_selection_reads, 1)
                if name == "prepost-recovery-restored":
                    self.assertTrue(fresh["rate_limit"]["allowed"])
                    self.assertEqual(fresh["rate_limit"]["secondary_window"]["used_percent"], 99)
                    self.assertEqual(mock.usage()["rate_limit"]["secondary_window"]["used_percent"], 0)
                elif name == "prepost-recovery-shifted":
                    self.assertNotEqual(fresh["rate_limit"]["secondary_window"]["reset_at"],
                                        before["rate_limit"]["secondary_window"]["reset_at"])
                elif name == "prepost-recovery-foreign":
                    self.assertNotEqual(fresh["account_id"], before["account_id"])
                elif name == "prepost-recovery-below-99":
                    self.assertEqual(fresh["rate_limit"]["secondary_window"]["used_percent"], 98)
                else:
                    self.assertEqual(fresh, before)

    def test_a_post_before_the_fresh_observation_is_an_actual_fixture_error(self):
        for name in weekly_reset.PREPOST_CASES:
            with self.subTest(case=name), patch.object(weekly_reset.time, "time", return_value=1000):
                mock = weekly_reset.Mock(name)
                mock.models = [{}, {}]
                mock.reply("GET", "/backend-api/wham/usage", None)
                mock.reply("GET", "/backend-api/wham/rate-limit-reset-credits", None)
                with self.assertRaisesRegex(RuntimeError, "post-selection usage"):
                    mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                               {"redeem_request_id": "a" * 64, "credit_id": "earned-weekly-credit"})


class PropagationFixtureTests(unittest.TestCase):
    def test_immediate_policy_reads_cannot_advance_backend_propagation_time(self):
        for name in weekly_reset.PROPAGATION_CASES:
            with self.subTest(case=name), patch.object(weekly_reset.time, "time", return_value=1000), \
                    patch.object(weekly_reset.time, "monotonic", return_value=10) as clock:
                mock = weekly_reset.Mock(name)
                mock.models = [{}, {}]
                endpoint = "/backend-api/wham/usage"
                mock.reply("GET", endpoint, None)
                mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                           {"redeem_request_id": "a" * 64, "credit_id": "earned-weekly-credit"})
                for _ in range(8):
                    self.assertFalse(json.loads(mock.reply("GET", endpoint, None)[1])["rate_limit"]["allowed"])
                clock.return_value = 69.999
                self.assertFalse(json.loads(mock.reply("GET", endpoint, None)[1])["rate_limit"]["allowed"])
                clock.return_value = 70
                self.assertTrue(json.loads(mock.reply("GET", endpoint, None)[1])["rate_limit"]["allowed"])


class RedemptionPolicyFixtureTests(unittest.TestCase):
    def test_five_hour_cycle_survives_reads_across_a_second_boundary(self):
        with patch.object(weekly_reset.time, "time", return_value=1000) as clock:
            mock = weekly_reset.Mock("preflight-five-hour")
            first = json.loads(mock.reply("GET", "/backend-api/wham/usage", None)[1])
            clock.return_value = 1001
            second = json.loads(mock.reply("GET", "/backend-api/wham/usage", None)[1])
        first_window = first["rate_limit"]["primary_window"]
        second_window = second["rate_limit"]["primary_window"]
        self.assertEqual(first_window["used_percent"], 99)
        self.assertEqual(second_window["used_percent"], 99)
        self.assertEqual(first_window["reset_at"], 4600)
        self.assertEqual(second_window["reset_at"], first_window["reset_at"])
        self.assertEqual(first_window["reset_after_seconds"], 3600)
        self.assertEqual(second_window["reset_after_seconds"], 3599)

    def test_positive_preflights_offer_99_percent_with_a_future_reset(self):
        for name in weekly_reset.PREFLIGHT_CASES:
            with self.subTest(case=name), patch.object(weekly_reset.time, "time", return_value=1000):
                mock = weekly_reset.Mock(name)
                self.assertEqual(mock.reply("GET", "/backend-api/wham/usage", None)[0], 200)
                if name in weekly_reset.PREPOST_CASES:
                    mock.reply("GET", "/backend-api/wham/rate-limit-reset-credits", None)
                    mock.reply("GET", "/backend-api/wham/usage", None)
                self.assertEqual(mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                                           {"redeem_request_id": "a" * 64,
                                            "credit_id": "earned-weekly-credit"})[0], 200)
                self.assertTrue(mock.consumed)
                windows = mock.requests[0]["response"]["rate_limit"]
                self.assertTrue(any(window["used_percent"] == 99 for window in
                                    (windows["primary_window"], windows["secondary_window"])))
                if name == "preflight-weekly-24h":
                    self.assertEqual(windows["secondary_window"]["reset_at"] - 1000, 86400)

    def test_reactive_weekly_25h_consumes_its_exhausted_window(self):
        self.assertIn("recovery-weekly-25h", weekly_reset.SUCCESS_CASES)
        self.assertNotIn("recovery-weekly-25h", weekly_reset.POLICY_POLL_CASES)
        for original in ("preflight-mixed-windows", "recovery-elapsed-reset"):
            self.assertIn(original, weekly_reset.POLICY_POLL_CASES)
        with patch.object(weekly_reset.time, "time", return_value=1000):
            mock = weekly_reset.Mock("recovery-weekly-25h")
            self.assertTrue(mock.usage()["rate_limit"]["allowed"])
            mock.models = [{}, {}]
            self.assertEqual(mock.reply("GET", "/backend-api/wham/usage", None)[0], 200)
            windows = mock.requests[0]["response"]["rate_limit"]
            self.assertFalse(windows["allowed"])
            self.assertEqual(windows["primary_window"]["used_percent"], 0)
            self.assertEqual(windows["secondary_window"]["used_percent"], 99)
            self.assertEqual(windows["secondary_window"]["reset_at"] - 1000, 90000)
            mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                       {"redeem_request_id": "a" * 64,
                        "credit_id": "earned-weekly-credit"})
            self.assertTrue(mock.consumed)
            self.assertTrue(mock.spent)

    def test_ineligible_weekly_refresh_cannot_borrow_a_healthy_five_hour_refresh(self):
        for reset in (None, 999, 1000):
            with self.subTest(reset=reset), patch.object(weekly_reset.time, "time", return_value=1000):
                mock = weekly_reset.Mock("preflight-weekly")
                mock.reply("GET", "/backend-api/wham/usage", None)
                windows = mock.requests[0]["response"]["rate_limit"]
                windows["secondary_window"]["reset_at"] = reset
                self.assertEqual(windows["secondary_window"]["used_percent"], 99)
                self.assertEqual(windows["primary_window"]["used_percent"], 0)
                self.assertGreater(windows["primary_window"]["reset_at"], 1000)
                with self.assertRaisesRegex(RuntimeError, "redemption policy"):
                    mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                               {"redeem_request_id": "a" * 64,
                                "credit_id": "earned-weekly-credit"})
                self.assertFalse(mock.consumed)

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
                    if name in ("preflight-elapsed-reset", "recovery-elapsed-reset"):
                        self.assertLess(mock.requests[0]["response"]["rate_limit"]["secondary_window"]["reset_at"], 1000)
                    if name == "preflight-mixed-windows":
                        windows = mock.requests[0]["response"]["rate_limit"]
                        self.assertEqual(windows["primary_window"]["used_percent"], 98)
                        self.assertGreater(windows["primary_window"]["reset_at"], 1000)
                        self.assertGreaterEqual(windows["secondary_window"]["used_percent"], 99)
                        self.assertLess(windows["secondary_window"]["reset_at"], 1000)
                    with self.assertRaisesRegex(RuntimeError, "redemption policy"):
                        mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                                   {"redeem_request_id": "a" * 64,
                                    "credit_id": "earned-weekly-credit"})
                    self.assertFalse(mock.consumed)
                    self.assertFalse(mock.spent)


class BackendAuthenticationFixtureTests(unittest.TestCase):
    def test_missing_or_wrong_credentials_are_rejected_before_backend_state_changes(self):
        mock = weekly_reset.Mock("preflight-five-hour")
        mock.expected_access_token = "fixture-negative-control-access-token"
        server, worker = weekly_reset.serve(mock)
        valid = {"Authorization": "Bearer " + mock.expected_access_token,
                 "ChatGPT-Account-Id": weekly_reset.ACCOUNT}

        def request(method, path, headers, body=None):
            with contextlib.closing(http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=2)) as connection:
                connection.request(method, "/backend-api/wham/" + path, body=body, headers=headers)
                with connection.getresponse() as reply:
                    reply.read()
                    return reply.status

        try:
            for field in valid:
                for replacement in (None, "wrong-fixture-credential"):
                    headers = dict(valid)
                    if replacement is None:
                        del headers[field]
                    else:
                        headers[field] = replacement
                    for method, path, body in (
                            ("GET", "usage", None),
                            ("GET", "rate-limit-reset-credits", None),
                            ("POST", "rate-limit-reset-credits/consume",
                             b'{"redeem_request_id":"fixture-key","credit_id":"earned-weekly-credit"}')):
                        with self.subTest(field=field, replacement=replacement, path=path):
                            self.assertEqual(request(method, path, headers, body), 401)
            self.assertFalse(mock.consumed)
            self.assertFalse(mock.spent)
            self.assertEqual(mock.credit_listings, 0)
            self.assertTrue(all(request["status"] == 401
                                and not all(request["authentication"].values())
                                for request in mock.requests))
            self.assertEqual(request("GET", "usage", valid), 200)
            self.assertTrue(all(mock.requests[-1]["authentication"].values()))
            self.assertNotIn(mock.expected_access_token, json.dumps(mock.requests))
            self.assertTrue(all(type(value) is bool for request in mock.requests
                                for value in request["authentication"].values()))
        finally:
            server.shutdown()
            worker.join()
            server.server_close()


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
