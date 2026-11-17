# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Renewal acquisition controls using inert public-main fixtures only."""
from contextlib import contextmanager
import json
from pathlib import Path
import sys
import unittest
from unittest import mock

import test_renewal_tail_cleanup as tail


RENEWAL = tail.RENEWAL


class RenewalStartupTests(unittest.TestCase):
    capture = tail.RenewalTailTests.capture

    @contextmanager
    def fixture(self, primary, **kwargs):
        with tail.RenewalTailTests.fixture(self, primary, **kwargs) as fixture:
            fixture.pairs = tuple(RENEWAL.serve.side_effect)
            RENEWAL.serve.side_effect = fixture.pairs
            yield fixture

    def assert_observation(self, fixture, acquired):
        expected = ["stop"]
        for name in ("model", "issuer")[:acquired]:
            expected.append(name + ".shutdown")
            if name + ".shutdown" not in fixture.failures:
                expected.append(name + ".join")
            expected.append(name + ".close")
        expected.extend("export." + name for name in tail.TRACES)
        expected.append("report")
        self.assertEqual(fixture.actions, expected)
        for name in tail.TRACES:
            self.assertEqual((fixture.output / name).exists(), "export." + name not in fixture.failures)
        report = json.loads((fixture.output / "result.json").read_text())
        self.assertIs(report["passed"], False)
        self.assertEqual(report["observationBudgetSeconds"], {
            "drive": RENEWAL.CASE_BUDGETS[report["case"]][0],
            "guardian": RENEWAL.CASE_BUDGETS[report["case"]][1]})
        return report

    def test_acquisition_refusals_clean_only_returned_pairs_and_report(self):
        for acquired in (0, 1):
            for primary in (RuntimeError("acquisition"), KeyboardInterrupt("acquisition"), SystemExit(0), SystemExit(47)):
                with self.subTest(acquired=acquired, primary=type(primary).__name__), self.fixture(primary) as fixture:
                    RENEWAL.serve.side_effect = [*fixture.pairs[:acquired], primary]
                    caught, status = self.capture()
                    if isinstance(primary, Exception):
                        self.assertIsNone(caught)
                        self.assertEqual(status, 1)
                    else:
                        self.assertIs(caught, primary)
                    report = self.assert_observation(fixture, acquired)
                    self.assertIn(type(primary).__name__, report["error"])
                    self.assertEqual(report.get("interrupted", False), not isinstance(primary, Exception))
                    self.assertEqual(RENEWAL.serve.call_count, acquired + 1)
                    RENEWAL.probe.configure.assert_not_called()

    def test_returned_pair_is_owned_before_its_server_is_extracted(self):
        for acquired in (1, 2):
            primary = KeyboardInterrupt("returned listener pair")
            with self.subTest(acquired=acquired), self.fixture(RuntimeError("must not configure")) as fixture:
                previous, observed, calls = sys.gettrace(), [], []
                slot = "model_listener" if acquired == 1 else "issuer_listener"

                def boundary(frame, event, _argument):
                    if (event == "line" and frame.f_code is RENEWAL._run.__code__
                            and frame.f_locals.get(slot) is fixture.pairs[acquired - 1]):
                        observed.append(True)
                        sys.settrace(previous)
                        raise primary
                    return boundary

                def acquire(*_args, **_kwargs):
                    calls.append(True)
                    if len(calls) == acquired:
                        sys._getframe(1).f_trace = boundary
                        sys.settrace(boundary)
                    return fixture.pairs[len(calls) - 1]

                try:
                    with mock.patch.object(RENEWAL, "serve", new=acquire):
                        caught, _ = self.capture()
                    self.assertIs(caught, primary)
                    self.assertEqual(observed, [True])
                    self.assertEqual(len(calls), acquired)
                    self.assertTrue(self.assert_observation(fixture, acquired)["interrupted"])
                    RENEWAL.probe.configure.assert_not_called()
                finally:
                    sys.settrace(previous)

    def test_report_metadata_refusals_run_before_acquisition_and_keep_failure_evidence(self):
        for module in (RENEWAL.native_host_record, RENEWAL.package_standing):
            for primary in (OSError("metadata"), KeyboardInterrupt("metadata"), SystemExit(0)):
                with self.subTest(module=module.__name__, primary=type(primary).__name__), self.fixture(primary) as fixture:
                    with mock.patch.object(module, "summary", side_effect=primary):
                        caught, status = self.capture()
                    if isinstance(primary, Exception):
                        self.assertIsNone(caught)
                        self.assertEqual(status, 1)
                    else:
                        self.assertIs(caught, primary)
                    report = self.assert_observation(fixture, 0)
                    self.assertIn(type(primary).__name__, report["error"])
                    RENEWAL.serve.assert_not_called()

    def test_acquisition_cancellation_survives_cleanup_and_final_save_failures(self):
        for failure in (OSError("save failed"), SystemExit(0)):
            primary = KeyboardInterrupt("issuer acquisition")
            failures = {"model.shutdown": OSError("shutdown failed"),
                        "export.requests.json": SystemExit(0)}
            with self.subTest(save=type(failure).__name__), \
                    self.fixture(primary, failures=failures, summary_failure=failure) as fixture:
                RENEWAL.serve.side_effect = [fixture.pairs[0], primary]
                caught, _ = self.capture()
                self.assertIs(caught, primary)
                self.assertIs(caught.__context__, failure)
                self.assertFalse((fixture.output / "result.json").exists())
                self.assertEqual(fixture.actions, ["stop", "model.shutdown", "model.close",
                    *("export." + name for name in tail.TRACES), "report"])
                fields = json.dumps(RENEWAL.probe.case_error_fields(primary))
                self.assertIn("shutdown failed", fields)
                self.assertIn("SystemExit", fields)

    def test_acquisition_entry_refuses_success_shaped_cancellation_and_keeps_nonzero(self):
        for primary, expected in ((SystemExit(0), 1), (SystemExit(47), 47), (KeyboardInterrupt("issuer"), None)):
            with self.subTest(primary=type(primary).__name__), self.fixture(primary) as fixture:
                RENEWAL.serve.side_effect = [fixture.pairs[0], primary]
                caught, _ = self.capture(entry=True)
                if expected == 1:
                    self.assertIsInstance(caught, SystemExit)
                    self.assertEqual(caught.code, 1)
                    self.assertIsNot(caught, primary)
                else:
                    self.assertIs(caught, primary)
                self.assertTrue(self.assert_observation(fixture, 1)["interrupted"])

    def test_successful_acquisitions_handoff_to_the_existing_body_and_cleanup(self):
        primary = RuntimeError("body configure sentinel")
        with self.fixture(primary) as fixture:
            caught, status = self.capture()
            self.assertIsNone(caught)
            self.assertEqual(status, 1)
            report = self.assert_observation(fixture, 2)
            self.assertEqual(report["error"], "RuntimeError: body configure sentinel")
            self.assertIs(report["nativeHost"]["qualifies"], False)
            self.assertIs(report["package"]["qualifies"], False)
            self.assertEqual(RENEWAL.serve.call_args_list, [mock.call(mock.ANY),
                mock.call(mock.ANY, secure=False, issuer_only=True)])
            RENEWAL.probe.configure.assert_called_once_with(fixture.output, fixture.pairs[0][0].server_port)


if __name__ == "__main__":
    unittest.main()
