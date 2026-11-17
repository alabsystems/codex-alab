# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Diagnostic faults cannot take ownership of fixture cleanup or cancellation."""
import json
import unittest

import test_quota_wait_cleanup as quota


class FailingText(OSError):
    def __init__(self, fault):
        super().__init__("original cleanup failure")
        self.fault = fault

    def __str__(self):
        raise self.fault


class FailingCancellationText(KeyboardInterrupt):
    def __init__(self, fault):
        super().__init__("original cancellation")
        self.fault = fault

    def __str__(self):
        raise self.fault


class FailingNotes(KeyboardInterrupt):
    def __init__(self, fault):
        super().__init__("original cancellation")
        self.fault = fault

    def add_note(self, _note):
        raise self.fault


class FailingTransport(KeyboardInterrupt):
    def __getattribute__(self, name):
        if name == "add_note":
            raise AttributeError(name)
        return super().__getattribute__(name)

    def __setattr__(self, name, value):
        if name == "_alab_case_cleanup_errors":
            raise OSError("optional diagnostic transport refused")
        return super().__setattr__(name, value)


class UnavailableNotes(KeyboardInterrupt):
    def __getattribute__(self, name):
        if name == "__notes__":
            raise SystemExit(0)
        return super().__getattribute__(name)


def faults():
    return (OSError("diagnostic operation failed"), KeyboardInterrupt("diagnostic stopped"), SystemExit(0))


class CleanupDiagnosticTests(unittest.TestCase):
    def fixture_case(self):
        return quota.QuotaWaitCleanupTests("test_case_saves_success_only_after_cleanup_and_trace_retention")

    def test_secondary_text_failure_never_replaces_primary_or_skips_safe_cleanup(self):
        case = self.fixture_case()
        for make_primary in (lambda: RuntimeError("original body error"),
                             lambda: KeyboardInterrupt("original cancellation")):
            for phase in ("shutdown", "close"):
                for fault in faults():
                    primary = make_primary()
                    with self.subTest(primary=type(primary).__name__, phase=phase, fault=type(fault).__name__), \
                            case.fixture(body_error=primary, failures={phase: FailingText(fault)}) as fixture:
                        caught, _ = case.capture(lambda: case.call_case(fixture))
                        self.assertIs(caught, primary)
                        case.assert_cleanup(fixture, shutdown_failed=phase == "shutdown")
                        fields = quota.SMOKE.probe.case_error_fields(primary)
                        self.assertIn("FailingText", json.dumps(fields))
                        self.assertIn("message unavailable", json.dumps(fields))

    def test_secondary_text_failure_without_primary_still_fails_the_case(self):
        case = self.fixture_case()
        for fault in faults():
            with self.subTest(fault=type(fault).__name__), \
                    case.fixture(failures={"shutdown": FailingText(fault)}) as fixture:
                caught, result = case.capture(lambda: case.call_case(fixture))
                self.assertIsNone(caught)
                self.assertFalse(result["passed"])
                self.assertIn("FailingText", json.dumps(result["cleanupErrors"]))
                case.assert_cleanup(fixture, shutdown_failed=True)

    def test_main_keeps_primary_cancellation_when_its_text_is_unavailable(self):
        case = self.fixture_case()
        for fault in faults():
            primary = FailingCancellationText(fault)
            with self.subTest(fault=type(fault).__name__), \
                    case.fixture(body_error=primary, failures={"close": OSError("cleanup close")}, main=True) as fixture:
                caught, _ = case.capture(lambda: case.call_main(fixture))
                self.assertIs(caught, primary)
                report = json.loads((fixture.output / "result.json").read_text())
                self.assertTrue(report["interrupted"])
                self.assertFalse(report["passed"])
                self.assertEqual(report["error"], "FailingCancellationText: <message unavailable>")
                case.assert_cleanup(fixture)

    def test_note_hook_failure_cannot_replace_primary_cancellation(self):
        case = self.fixture_case()
        for fault in faults():
            primary = FailingNotes(fault)
            with self.subTest(fault=type(fault).__name__), \
                    case.fixture(body_error=primary, failures={"close": OSError("cleanup close")}) as fixture:
                caught, _ = case.capture(lambda: case.call_case(fixture))
                self.assertIs(caught, primary)
                case.assert_cleanup(fixture)
                fields = quota.SMOKE.probe.case_error_fields(primary)
                self.assertIn("cleanup close", json.dumps(fields))

    def test_unavailable_fallback_or_native_notes_do_not_change_the_main_outcome(self):
        case = self.fixture_case()
        for kind in (FailingTransport, UnavailableNotes):
            primary = kind("original cancellation")
            with self.subTest(kind=kind.__name__), \
                    case.fixture(body_error=primary, failures={"close": OSError("cleanup close")}, main=True) as fixture:
                caught, _ = case.capture(lambda: case.call_main(fixture))
                self.assertIs(caught, primary)
                report = json.loads((fixture.output / "result.json").read_text())
                self.assertTrue(report["interrupted"])
                self.assertFalse(report["passed"])
                self.assertIn(kind.__name__, report["error"])
                case.assert_cleanup(fixture)

    def test_actual_cleanup_cancellation_is_selected_before_its_failed_diagnostics(self):
        case = self.fixture_case()
        for fault in faults():
            stop = FailingCancellationText(fault)
            with self.subTest(fault=type(fault).__name__), \
                    case.fixture(body_error=RuntimeError("original body error"), failures={"close": stop}) as fixture:
                caught, _ = case.capture(lambda: case.call_case(fixture))
                self.assertIs(caught, stop)
                self.assertIn("original body error", json.dumps(quota.SMOKE.probe.case_error_fields(stop)))
                case.assert_cleanup(fixture)

    def test_ordinary_main_failure_with_unavailable_text_still_returns_failure(self):
        case = self.fixture_case()
        primary = FailingText(SystemExit(0))
        with case.fixture(body_error=primary, main=True) as fixture:
            self.assertEqual(case.call_main(fixture), 1)
            report = json.loads((fixture.output / "result.json").read_text())
            self.assertFalse(report["passed"])
            self.assertEqual(report["error"], "FailingText: <message unavailable>")
            self.assertNotIn("interrupted", report)
            case.assert_cleanup(fixture)

    def test_working_native_notes_or_older_transport_keep_their_existing_fields(self):
        case = self.fixture_case()
        primary = KeyboardInterrupt("original cancellation")
        with case.fixture(body_error=primary, failures={"close": OSError("cleanup close")}) as fixture:
            caught, _ = case.capture(lambda: case.call_case(fixture))
            self.assertIs(caught, primary)
            fields = quota.SMOKE.probe.case_error_fields(primary)
            self.assertEqual(fields["error"], "KeyboardInterrupt: original cancellation")
            field = "errorNotes" if hasattr(BaseException, "add_note") else "cleanupErrors"
            other = "cleanupErrors" if field == "errorNotes" else "errorNotes"
            self.assertIn("cleanup close", json.dumps(fields[field]))
            self.assertNotIn(other, fields)
            case.assert_cleanup(fixture)


if __name__ == "__main__":
    unittest.main()
