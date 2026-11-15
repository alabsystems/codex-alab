# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Case teardown and Python 3.11+ exception notes; no listener or binary runs."""
from contextlib import contextmanager
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


SOURCE = Path(__file__).resolve().parents[1] / "smoke/request_recovery.py"
sys.path.insert(0, str(SOURCE.parent))
SPEC = importlib.util.spec_from_file_location("request_recovery_cleanup", SOURCE)
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)
FIXTURE_PARENT = None
FIXTURES = []


class RequestRecoveryCleanupTests(unittest.TestCase):
    @contextmanager
    def fixture(self, *, body_error=None, failures=None, verifier_error=None, main=False):
        root = Path(tempfile.mkdtemp(prefix="request-recovery-cleanup-", dir=FIXTURE_PARENT))
        FIXTURES.append(str(root))
        output = root / "results"
        if not main:
            output.mkdir()
        binary = root / "codex"
        binary.write_bytes(b"unexecuted fixture")
        calls = []
        failures = failures or {}
        model = SimpleNamespace(requests=[{"fixture": "request evidence"}])

        def cleanup(phase):
            def perform():
                calls.append(phase)
                if phase in failures:
                    raise failures[phase]
            return perform

        server = SimpleNamespace(server_port=1, shutdown=cleanup("shutdown"),
                                 server_close=cleanup("close"))
        thread = SimpleNamespace(join=cleanup("join"))
        save = SMOKE.probe.write_json

        def write_json(path, value):
            if path.name == "requests.json":
                cleanup("write")()
            save(path, value)

        def run(_codex, _args, case_root, _env, _label):
            if body_error is not None:
                raise body_error
            (case_root / "exec-stdout.jsonl").write_text('{"type":"fixture"}\n')
            return 0

        with mock.patch.object(SMOKE.probe, "fresh_environment", return_value={}), \
                mock.patch.object(SMOKE, "Rejection", return_value=model), \
                mock.patch.object(SMOKE.probe, "serve", return_value=(server, thread)) as serve, \
                mock.patch.object(SMOKE.probe, "configure"), \
                mock.patch.object(SMOKE.probe, "run", side_effect=run) as execute, \
                mock.patch.object(SMOKE, "check_result", return_value=2.0,
                                  side_effect=verifier_error) as check, \
                mock.patch.object(SMOKE.probe, "write_json", side_effect=write_json):
            yield SimpleNamespace(root=root, output=output, binary=binary, calls=calls,
                                  model=model, serve=serve, execute=execute, check=check)

    def call_case(self, fixture):
        return SMOKE.case(fixture.binary, fixture.output, "opaque", 400, {}, True)

    def capture(self, action):
        try:
            return None, action()
        except BaseException as error:
            return error, None

    def assert_teardown(self, fixture, *, shutdown_failed=False, write_failed=False, name="opaque"):
        expected = ["shutdown", "close", "write"] if shutdown_failed else ["shutdown", "join", "close", "write"]
        self.assertEqual(fixture.calls, expected)
        evidence = fixture.output / name / "requests.json"
        if write_failed:
            self.assertFalse(evidence.exists())
        else:
            self.assertEqual(json.loads(evidence.read_text()), fixture.model.requests)

    def assert_notes(self, error, *tokens):
        notes = getattr(error, "__notes__", [])
        self.assertIsInstance(notes, list)
        text = "\n".join(notes).lower()
        for token in tokens:
            self.assertIn(token.lower(), text)
        return notes

    def test_success_keeps_normal_fields_and_teardown_order(self):
        with self.fixture() as fixture:
            result = self.call_case(fixture)
        self.assertEqual(result, {"case": "opaque", "passed": True, "requestCount": 1,
                                  "returncode": 0, "retryAfterSeconds": 2.0})
        fixture.execute.assert_called_once()
        fixture.check.assert_called_once_with(0, [{"type": "fixture"}], fixture.model.requests, True)
        self.assert_teardown(fixture)

    def test_success_does_not_inherit_a_callers_handled_exception(self):
        with self.fixture() as fixture:
            try:
                raise ValueError("caller is handling this")
            except ValueError:
                result = self.call_case(fixture)
        self.assertTrue(result["passed"])
        self.assertNotIn("cleanupErrors", result)
        self.assert_teardown(fixture)

    def test_older_cli_interpreter_is_refused_before_output_or_case_work(self):
        with self.fixture(main=True) as fixture:
            with mock.patch.object(sys, "argv", [str(SOURCE), "--codex", str(fixture.binary),
                                                "--output", str(fixture.output)]), \
                    mock.patch.object(sys, "version_info", (3, 10, 0)), \
                    mock.patch("sys.stderr"):
                with self.assertRaises(SystemExit) as caught:
                    SMOKE.main()
            self.assertEqual(caught.exception.code, 2)
            self.assertFalse(fixture.output.exists())
            fixture.serve.assert_not_called()
            fixture.execute.assert_not_called()

    def test_body_error_survives_each_cleanup_failure_with_evidence(self):
        for make_error in (lambda: RuntimeError("body broke"), lambda: KeyboardInterrupt("body stopped"),
                           lambda: SystemExit(130), lambda: SystemExit(0)):
            for phase in ("shutdown", "join", "close", "write"):
                primary = make_error()
                with self.subTest(body=repr(primary), phase=phase), \
                        self.fixture(body_error=primary, failures={phase: OSError("cleanup " + phase)}) as fixture:
                    caught, _ = self.capture(lambda: self.call_case(fixture))
                    self.assertIs(caught, primary)
                    self.assert_notes(primary, phase, "OSError", "cleanup " + phase)
                    if phase == "shutdown":
                        self.assert_notes(primary, "join", "skip")
                    self.assert_teardown(fixture, shutdown_failed=phase == "shutdown", write_failed=phase == "write")

    def test_all_safe_cleanup_attempts_report_every_failure(self):
        for shutdown_failed in (False, True):
            phases = ("shutdown", "close", "write") if shutdown_failed else ("join", "close", "write")
            errors = {phase: OSError("cleanup " + phase) for phase in phases}
            primary = RuntimeError("body broke")
            with self.subTest(shutdown_failed=shutdown_failed), \
                    self.fixture(body_error=primary, failures=errors) as fixture:
                caught, _ = self.capture(lambda: self.call_case(fixture))
                self.assertIs(caught, primary)
                for phase in phases:
                    self.assert_notes(primary, phase, "cleanup " + phase)
                if shutdown_failed:
                    self.assert_notes(primary, "join", "skip")
                self.assert_teardown(fixture, shutdown_failed=shutdown_failed, write_failed=True)

    def test_cleanup_cancellation_wins_except_over_pending_body_cancellation(self):
        for make_body in (lambda: None, lambda: RuntimeError("body broke"),
                          lambda: KeyboardInterrupt("body stopped"), lambda: SystemExit(130), lambda: SystemExit(0)):
            for make_stop in (lambda: KeyboardInterrupt("cleanup stopped"), lambda: SystemExit(130), lambda: SystemExit(0)):
                primary, stop = make_body(), make_stop()
                winner = primary if isinstance(primary, (KeyboardInterrupt, SystemExit)) else stop
                with self.subTest(body=repr(primary), cleanup=repr(stop)), \
                        self.fixture(body_error=primary, failures={"close": stop}) as fixture:
                    caught, _ = self.capture(lambda: self.call_case(fixture))
                    self.assertIs(caught, winner)
                    self.assert_notes(winner, "close", type(stop).__name__)
                    if isinstance(primary, RuntimeError):
                        self.assert_notes(winner, "body", "RuntimeError", "body broke")
                    self.assert_teardown(fixture)

    def test_cleanup_failures_fail_results_without_erasing_verifier_error(self):
        for verifier_error in (None, RuntimeError("oracle refused")):
            for phase in ("shutdown", "join", "close", "write"):
                with self.subTest(verifier=repr(verifier_error), phase=phase), \
                        self.fixture(verifier_error=verifier_error,
                                     failures={phase: OSError("cleanup " + phase)}) as fixture:
                    caught, result = self.capture(lambda: self.call_case(fixture))
                    self.assertIsNone(caught)
                    self.assertFalse(result["passed"])
                    self.assertEqual(result["requestCount"], 1)
                    self.assertEqual(result["returncode"], 0)
                    if verifier_error is not None:
                        self.assertEqual(result["error"], "oracle refused")
                    errors = result["cleanupErrors"]
                    self.assertIsInstance(errors, list)
                    text = json.dumps(errors).lower()
                    self.assertIn(phase, text)
                    self.assertIn("cleanup " + phase, text)
                    if phase == "shutdown":
                        self.assertIn("join", text)
                        self.assertIn("skip", text)
                    self.assert_teardown(fixture, shutdown_failed=phase == "shutdown", write_failed=phase == "write")

    def test_other_control_exceptions_are_not_swallowed_or_replaced(self):
        for make_body in (lambda: None, lambda: RuntimeError("body broke"), lambda: GeneratorExit("body closed")):
            for make_stop in (lambda: GeneratorExit("cleanup closed"), lambda: KeyboardInterrupt("cleanup stopped")):
                primary, stop = make_body(), make_stop()
                winner = primary if isinstance(primary, GeneratorExit) else stop
                with self.subTest(body=repr(primary), cleanup=repr(stop)), \
                        self.fixture(body_error=primary, failures={"close": stop}) as fixture:
                    caught, _ = self.capture(lambda: self.call_case(fixture))
                    self.assertIs(caught, winner)
                    self.assert_notes(winner, "close", type(stop).__name__)
                    self.assert_teardown(fixture)

    def test_main_preserves_case_cancellation_notes_and_stops_later_cases(self):
        for make_stop in (lambda: KeyboardInterrupt("body stopped"), lambda: SystemExit(130), lambda: SystemExit(0)):
            for phase in ("shutdown", "close", "write"):
                stop = make_stop()
                with self.subTest(stop=repr(stop), phase=phase), \
                        self.fixture(body_error=stop, failures={phase: OSError("cleanup " + phase)}, main=True) as fixture:
                    with mock.patch.object(sys, "argv", [str(SOURCE), "--codex", str(fixture.binary),
                                                        "--output", str(fixture.output)]), mock.patch("builtins.print"):
                        caught, _ = self.capture(SMOKE.main)
                    self.assertIs(caught, stop)
                    notes = self.assert_notes(stop, phase, "OSError")
                    report = json.loads((fixture.output / "result.json").read_text())
                    self.assertFalse(report["passed"])
                    self.assertTrue(report["interrupted"])
                    self.assertEqual(report["activeCase"], SMOKE.CASES[0][0])
                    self.assertEqual(report["cases"], [])
                    self.assertEqual(report["errorNotes"], notes)
                    fixture.serve.assert_called_once()
                    fixture.execute.assert_called_once()
                    self.assert_teardown(fixture, shutdown_failed=phase == "shutdown", write_failed=phase == "write",
                                         name=SMOKE.CASES[0][0])

    def test_main_serializes_ordinary_case_cleanup_notes(self):
        primary = RuntimeError("body broke")
        with self.fixture(body_error=primary, failures={"close": OSError("cleanup close")}, main=True) as fixture:
            with mock.patch.object(sys, "argv", [str(SOURCE), "--codex", str(fixture.binary),
                                                "--output", str(fixture.output)]), \
                    mock.patch.object(SMOKE, "CASES", SMOKE.CASES[:1]), mock.patch("builtins.print"):
                self.assertEqual(SMOKE.main(), 1)
            notes = self.assert_notes(primary, "close", "OSError")
            report = json.loads((fixture.output / "result.json").read_text())
            self.assertFalse(report["passed"])
            self.assertEqual(report["cases"][0]["error"], "RuntimeError: body broke")
            self.assertEqual(report["cases"][0]["errorNotes"], notes)
            case_report = json.loads((fixture.output / SMOKE.CASES[0][0] / "result.json").read_text())
            self.assertEqual(case_report, report["cases"][0])
            self.assert_teardown(fixture, name=SMOKE.CASES[0][0])

    def test_cleanup_cancellation_retains_the_failed_verifier_diagnostic(self):
        for main in (False, True):
            for make_stop in (lambda: KeyboardInterrupt("cleanup stopped"), lambda: SystemExit(130), lambda: SystemExit(0)):
                stop = make_stop()
                with self.subTest(main=main, stop=repr(stop)), \
                        self.fixture(verifier_error=RuntimeError("oracle refused"),
                                     failures={"close": stop}, main=main) as fixture:
                    if main:
                        with mock.patch.object(sys, "argv", [str(SOURCE), "--codex", str(fixture.binary),
                                                            "--output", str(fixture.output)]), mock.patch("builtins.print"):
                            caught, _ = self.capture(SMOKE.main)
                    else:
                        caught, _ = self.capture(lambda: self.call_case(fixture))
                    self.assertIs(caught, stop)
                    notes = self.assert_notes(stop, "oracle refused", "close", type(stop).__name__)
                    if main:
                        report = json.loads((fixture.output / "result.json").read_text())
                        self.assertTrue(report["interrupted"])
                        self.assertFalse(report["passed"])
                        self.assertEqual(report["errorNotes"], notes)
                        fixture.execute.assert_called_once()


if __name__ == "__main__":
    unittest.main()
