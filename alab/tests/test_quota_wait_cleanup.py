# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Quota-wait case cleanup and interruption evidence; no native binary runs."""
from contextlib import contextmanager
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


SOURCE = Path(__file__).resolve().parents[1] / "smoke/quota_wait.py"
sys.path.insert(0, str(SOURCE.parent))
SPEC = importlib.util.spec_from_file_location("quota_wait_cleanup", SOURCE)
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)
NAME = "repeated-stale-reset"


class QuotaWaitCleanupTests(unittest.TestCase):
    @contextmanager
    def fixture(self, *, body_error=None, failures=None, main=False, summary_failure=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            output = root / "results"
            if not main:
                output.mkdir()
            binary = root / "codex"
            binary.write_bytes(b"unexecuted fixture")
            calls = []
            failures = failures or {}
            saved = []
            state = SimpleNamespace(root=root, output=output, binary=binary, calls=calls,
                                    model=None, saved=saved)

            def cleanup(phase):
                def action():
                    calls.append(phase)
                    if phase in failures:
                        raise failures[phase]
                return action

            server = SimpleNamespace(server_port=1234, shutdown=cleanup("shutdown"),
                                     server_close=cleanup("close"))
            thread = SimpleNamespace(join=cleanup("join"))

            def serve(model):
                state.model = model
                return server, thread

            save = SMOKE.probe.write_json

            def write_json(path, value):
                if path.name == "requests.json":
                    cleanup("write")()
                elif path == output / "result.json" and summary_failure == "write":
                    raise OSError("fixture summary write denied")
                elif path.name == "result.json":
                    saved.append((path, value.copy(), list(calls)))
                save(path, value)

            def run(_binary, _args, destination, _env, _label, *, interact=None):
                self.assertIsNone(interact)
                body = {"input": [{"type": "message", "role": "user", "content": SMOKE.probe.PROMPT}]}
                for number in range(1 if body_error else 5):
                    *_, record = state.model.reply(body)
                    record.update(headerStartedAt=10 * number, headerAt=10 * number + .001,
                                  requestStartedAt=10 * number)
                if body_error is not None:
                    raise body_error
                events = [{"type": "turn.started"},
                          {"type": "item.completed", "item": {
                              "type": "agent_message", "text": SMOKE.probe.ANSWER}},
                          {"type": "turn.completed"}]
                with (destination / "exec-stdout.jsonl").open("x") as stream:
                    for event in events:
                        stream.write(json.dumps(event) + "\n")
                save(destination / "exec-guardian-result.json", {"returncode": 0, "timed_out": False})
                return 0

            with mock.patch.object(SMOKE.probe, "serve", side_effect=serve) as start, \
                    mock.patch.object(SMOKE.probe, "run", side_effect=run) as execute, \
                    mock.patch.object(SMOKE.probe, "write_json", side_effect=write_json), \
                    mock.patch("builtins.print", side_effect=(
                        BrokenPipeError("fixture summary pipe closed") if summary_failure == "print" else None)):
                state.start, state.execute = start, execute
                yield state

    def call_case(self, fixture):
        return SMOKE.case(fixture.binary, fixture.output, False)

    def call_main(self, fixture):
        with mock.patch.object(sys, "argv", [str(SOURCE), "--codex", str(fixture.binary),
                                            "--output", str(fixture.output)]):
            return SMOKE.main()

    def capture(self, action):
        try:
            return None, action()
        except BaseException as error:
            return error, None

    def assert_cleanup(self, fixture, *, shutdown_failed=False, write_failed=False):
        expected = ["shutdown", "close", "write"] if shutdown_failed else ["shutdown", "join", "close", "write"]
        self.assertEqual(fixture.calls, expected)
        evidence = fixture.output / NAME / "requests.json"
        if write_failed:
            self.assertFalse(evidence.exists())
        else:
            self.assertEqual(json.loads(evidence.read_text()), fixture.model.requests)

    def assert_diagnostics(self, error, *tokens):
        fields = SMOKE.probe.case_error_fields(error)
        details = fields.get("errorNotes", fields.get("cleanupErrors", []))
        self.assertIsInstance(details, list)
        text = "\n".join(details)
        for token in tokens:
            self.assertIn(token, text)
        return fields

    def test_case_saves_success_only_after_cleanup_and_trace_retention(self):
        with self.fixture() as fixture:
            result = self.call_case(fixture)
            self.assertTrue(result["passed"])
            self.assertEqual(result["requestCount"], 5)
            self.assert_cleanup(fixture)
            self.assertEqual(fixture.saved, [(fixture.output / NAME / "result.json", result, fixture.calls)])
            self.assertEqual(json.loads((fixture.output / NAME / "result.json").read_text()), result)

    def test_each_cleanup_failure_retains_a_failed_case_after_all_safe_actions(self):
        for phase in ("shutdown", "join", "close", "write"):
            with self.subTest(phase=phase), self.fixture(failures={phase: OSError("cleanup " + phase)}) as fixture:
                caught, result = self.capture(lambda: self.call_case(fixture))
                self.assertIsNone(caught)
                self.assertFalse(result["passed"])
                self.assertIn("cleanup " + phase, json.dumps(result["cleanupErrors"]))
                self.assertEqual(json.loads((fixture.output / NAME / "result.json").read_text()), result)
                self.assert_cleanup(fixture, shutdown_failed=phase == "shutdown", write_failed=phase == "write")
                self.assertFalse(any(value["passed"] for _path, value, _calls in fixture.saved))

    def test_body_errors_survive_cleanup_failures_with_secondary_evidence(self):
        for make_error in (lambda: RuntimeError("body broke"), lambda: KeyboardInterrupt("body stopped"),
                           lambda: SystemExit(130), lambda: SystemExit(0)):
            for phase in ("shutdown", "join", "close", "write"):
                primary = make_error()
                with self.subTest(body=repr(primary), phase=phase), \
                        self.fixture(body_error=primary, failures={phase: OSError("cleanup " + phase)}) as fixture:
                    caught, _ = self.capture(lambda: self.call_case(fixture))
                    self.assertIs(caught, primary)
                    self.assert_diagnostics(caught, phase, "OSError", "cleanup " + phase)
                    self.assert_cleanup(fixture, shutdown_failed=phase == "shutdown", write_failed=phase == "write")
                    self.assertFalse((fixture.output / NAME / "result.json").exists())

    def test_all_cleanup_failures_are_retained_and_primary_cancellation_wins(self):
        primary = KeyboardInterrupt("body stopped")
        failures = {phase: OSError("cleanup " + phase) for phase in ("shutdown", "close", "write")}
        with self.fixture(body_error=primary, failures=failures) as fixture:
            caught, _ = self.capture(lambda: self.call_case(fixture))
            self.assertIs(caught, primary)
            self.assert_diagnostics(caught, "shutdown", "join", "skipped", "close", "write")
            self.assert_cleanup(fixture, shutdown_failed=True, write_failed=True)

    def test_cleanup_cancellation_wins_except_over_pending_body_cancellation(self):
        for make_body in (lambda: None, lambda: RuntimeError("body broke"),
                          lambda: KeyboardInterrupt("body stopped"), lambda: SystemExit(0)):
            for make_stop in (lambda: KeyboardInterrupt("cleanup stopped"), lambda: SystemExit(130), lambda: SystemExit(0)):
                primary, stop = make_body(), make_stop()
                expected = primary if isinstance(primary, (KeyboardInterrupt, SystemExit)) else stop
                with self.subTest(body=repr(primary), stop=repr(stop)), \
                        self.fixture(body_error=primary, failures={"close": stop}) as fixture:
                    caught, _ = self.capture(lambda: self.call_case(fixture))
                    self.assertIs(caught, expected)
                    self.assert_diagnostics(caught, "close", type(stop).__name__)
                    if isinstance(primary, RuntimeError):
                        self.assert_diagnostics(caught, "case body", "body broke")
                    self.assert_cleanup(fixture)
                    self.assertFalse((fixture.output / NAME / "result.json").exists())

    def test_main_stops_after_failed_cleanup_and_retains_the_failed_case(self):
        with self.fixture(failures={"close": OSError("cleanup close")}, main=True) as fixture:
            self.assertEqual(self.call_main(fixture), 1)
            report = json.loads((fixture.output / "result.json").read_text())
            self.assertFalse(report["passed"])
            self.assertEqual(report["activeCase"], NAME)
            self.assertEqual(len(report["cases"]), 1)
            self.assertFalse(report["cases"][0]["passed"])
            self.assertIn("cleanup close", json.dumps(report["cases"][0]["cleanupErrors"]))
            fixture.execute.assert_called_once()
            self.assert_cleanup(fixture)

    def test_main_retains_original_cancellation_and_cleanup_diagnostics(self):
        for make_stop in (lambda: KeyboardInterrupt("body stopped"), lambda: SystemExit(130), lambda: SystemExit(0)):
            for phase in (None, "shutdown", "close", "write"):
                stop = make_stop()
                failures = {} if phase is None else {phase: OSError("cleanup " + phase)}
                with self.subTest(stop=repr(stop), phase=phase), \
                        self.fixture(body_error=stop, failures=failures, main=True) as fixture:
                    caught, _ = self.capture(lambda: self.call_main(fixture))
                    self.assertIs(caught, stop)
                    report = json.loads((fixture.output / "result.json").read_text())
                    self.assertFalse(report["passed"])
                    self.assertTrue(report["interrupted"])
                    self.assertEqual(report["activeCase"], NAME)
                    self.assertEqual(report["cases"], [])
                    if phase is not None:
                        fields = self.assert_diagnostics(stop, phase, "OSError")
                        for key, value in fields.items():
                            self.assertEqual(report[key], value)
                    fixture.execute.assert_called_once()
                    self.assert_cleanup(fixture, shutdown_failed=phase == "shutdown", write_failed=phase == "write")

    def test_summary_failure_never_replaces_the_original_cancellation(self):
        for make_stop in (lambda: KeyboardInterrupt("body stopped"), lambda: SystemExit(130), lambda: SystemExit(0)):
            for failure in ("write", "print"):
                stop = make_stop()
                with self.subTest(stop=repr(stop), failure=failure), \
                        self.fixture(body_error=stop, failures={"close": OSError("cleanup close")},
                                     main=True, summary_failure=failure) as fixture:
                    caught, _ = self.capture(lambda: self.call_main(fixture))
                    self.assertIs(caught, stop)
                    self.assertIsInstance(stop.__context__, OSError)
                    self.assert_diagnostics(stop, "cleanup close", "OSError")
                    self.assert_cleanup(fixture)
                    if failure == "print":
                        self.assertFalse(json.loads((fixture.output / "result.json").read_text())["passed"])
                    else:
                        self.assertFalse((fixture.output / "result.json").exists())


if __name__ == "__main__":
    unittest.main()
