# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Case teardown and Python 3.11+ exception notes; no listener or binary runs."""
from contextlib import contextmanager
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
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
        if FIXTURE_PARENT is None:
            self.addCleanup(shutil.rmtree, root)
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
            elif path.name == "exec-observed.json":
                cleanup("write stdout times")()
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
                                  "returncode": 0, "turnEvents": [], "retryAfterSeconds": 2.0})
        fixture.execute.assert_called_once()
        fixture.check.assert_called_once_with(0, [{"type": "fixture"}], fixture.model.requests, True,
                                              expect_capacity_notice=False, fail_with_tool=False, waits=(2,), handshakes=None,
                                              case_dir=fixture.output / "opaque", refusal=None)
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

    def test_main_preserves_observations_from_a_timed_out_exec(self):
        def partial_output(_codex, _args, root, _env, _label):
            messages = SMOKE.probe.provider_fixture.CAPACITY_MESSAGES
            notice = {"type": "error", "message": messages["server_is_overloaded"] + " (Retry scheduled in 324s.)"}
            events = [
                {"type": "item.completed", "item": notice},
                {"type": "error", "message": messages["policy_capacity_unavailable"]},
                {"type": "error", "message": "PRIVATE unrelated diagnostic"},
                {"type": "error", "message": notice["message"] + " PRIVATE suffix"},
            ]
            (root / "exec-stdout.jsonl").write_bytes(
                b'{"type":"turn.started"}\n\nfalse\n{"type":"item.completed"}\n'
                + '{"type":"turn.completed"}'.encode("utf-16le") + b'\n{"type":"turn.\xff\n'
                + b"\n".join(json.dumps(event).encode() for event in events))
            raise subprocess.TimeoutExpired("fixture", 6)

        with self.fixture(main=True) as fixture:
            fixture.execute.side_effect = partial_output
            args = SimpleNamespace(output=fixture.output, codex=fixture.binary, package=None, capacity_only=False)
            with mock.patch.object(SMOKE, "CASES", SMOKE.CASES[:1]), mock.patch("builtins.print"):
                self.assertEqual(SMOKE.main(args), 1)
            report = json.loads((fixture.output / "result.json").read_text())
            case = report["cases"][0]
            self.assertFalse(case["passed"])
            self.assertIn("TimeoutExpired", case["error"])
            self.assertNotIn("returncode", case)
            self.assertEqual(case["requestCount"], 1)
            self.assertEqual(case["turnEvents"], ["turn.started"])
            self.assertEqual(case["invalidStdoutLines"], [3, 5, 6])
            self.assertEqual(case["capacityNotices"], [
                {"stdoutLine": 7, "eventType": "item.completed", "reason": "server_is_overloaded", "scheduledSeconds": 324},
                {"stdoutLine": 8, "eventType": "error", "reason": "policy_capacity_unavailable"},
                {"stdoutLine": 10, "eventType": "error", "reason": "server_is_overloaded"},
            ])
            self.assertNotIn("PRIVATE", json.dumps(case))
            self.assertEqual(case, json.loads((fixture.output / case["case"] / "result.json").read_text()))

    def test_malformed_stdout_after_exit_keeps_the_exit_and_partial_observations(self):
        for code, invalid, terminal in ((0, b'{"type":', "turn.completed"), (1, b"null", "turn.failed")):
            with self.subTest(code=code), self.fixture() as fixture:
                def run(_codex, _args, root, _env, _label):
                    notice = {"type": "error", "message": SMOKE.probe.provider_fixture.CAPACITY_NOTICE}
                    (root / "exec-stdout.jsonl").write_bytes(b"\n".join((
                        b'{"type":"turn.started"}', json.dumps(notice).encode(),
                        json.dumps({"type": terminal}).encode(), invalid)))
                    return code

                fixture.execute.side_effect = run
                case = self.call_case(fixture)
                self.assertFalse(case["passed"])
                self.assertIn("error", case)
                self.assertEqual(case["returncode"], code)
                self.assertEqual(case["requestCount"], 1)
                self.assertEqual(case["turnEvents"], ["turn.started", terminal])
                self.assertEqual(case["invalidStdoutLines"], [4])
                self.assertEqual(case["capacityNotices"], [
                    {"stdoutLine": 2, "eventType": "error", "reason": "server_is_overloaded"}])
                fixture.check.assert_not_called()
                self.assert_teardown(fixture)

    def test_timeout_retains_stdout_timing_even_if_request_trace_export_fails(self):
        lines = [b'{"type":"turn.started"}\n', b'\n', b'{"type":"item.completed"}\n']

        def partial_output(_codex, _args, root, _env, _label, *, interact, stdin_pipe):
            with (root / "exec-stdout.jsonl").open("wb") as log:
                interact(SimpleNamespace(stdout=iter(lines)), log)
            raise primary

        for name in ("prewarm-overload", "repeated-overload", "unadvised-overload"):
            with self.subTest(case=name), self.fixture(failures={"write": OSError("trace unavailable")}) as fixture:
                primary = subprocess.TimeoutExpired("fixture", 6)
                fixture.execute.side_effect = partial_output
                caught, _ = self.capture(lambda: SMOKE.case(fixture.binary, fixture.output, name, 503, {}, True))
                self.assertIs(caught, primary)
                self.assert_notes(caught, "trace unavailable")
                times = json.loads((fixture.output / name / "exec-observed.json").read_text())
                self.assertEqual(len(times), 2)
                self.assertLessEqual(times[0], times[1])

    def test_timing_export_failure_preserves_timeout_and_provider_trace(self):
        primary = subprocess.TimeoutExpired("fixture", 6)
        with self.fixture(failures={"write stdout times": OSError("timing unavailable")}) as fixture:
            fixture.execute.side_effect = primary
            caught, _ = self.capture(lambda: SMOKE.case(
                fixture.binary, fixture.output, "repeated-overload", 503, {}, True))
            self.assertIs(caught, primary)
            self.assert_notes(caught, "write stdout times", "timing unavailable")
            root = fixture.output / "repeated-overload"
            self.assertEqual(json.loads((root / "requests.json").read_bytes()), fixture.model.requests)
            self.assertFalse((root / "exec-observed.json").exists())

    def test_timeout_before_stdout_is_available_keeps_the_original_error(self):
        primary = subprocess.TimeoutExpired("fixture", 6)
        with self.fixture(body_error=primary, failures={"write": OSError("trace unavailable")}, main=True) as fixture:
            args = SimpleNamespace(output=fixture.output, codex=fixture.binary, package=None, capacity_only=False)
            with mock.patch.object(SMOKE, "CASES", SMOKE.CASES[:1]), mock.patch("builtins.print"):
                self.assertEqual(SMOKE.main(args), 1)
            case = json.loads((fixture.output / "result.json").read_text())["cases"][0]
            self.assertIn("TimeoutExpired", case["error"])
            self.assertIn("trace unavailable", str(case["errorNotes"]))
            self.assertNotIn("requestCount", case)
            self.assertNotIn("turnEvents", case)
            self.assertIn("exec-stdout.jsonl", case["stdoutReadError"])

    def test_damaged_compaction_stdout_retains_rpc_and_process_outcome(self):
        import compaction_recovery

        for mid_turn, status, invalid in ((True, "failed", b"null"), (False, "completed", b'{"method":')):
            with self.subTest(mid_turn=mid_turn), self.fixture() as fixture:
                final = {"id": "compact", "status": status}
                event = {"method": "turn/completed", "params": {"threadId": "thread", "turn": final}}
                rpc = mock.Mock()
                rpc.request.return_value = {"thread": {"id": "thread"}}
                rpc.turn.return_value = final if mid_turn else {"id": "seed", "status": "completed"}
                rpc.wait.return_value = event

                def run(_codex, _args, root, _env, _label, *, interact):
                    interact(None, None)
                    (root / "app-stdout.jsonl").write_bytes(json.dumps(event).encode() + b"\n" + invalid)
                    return 0

                fixture.execute.side_effect = run
                with mock.patch.object(compaction_recovery, "Rpc", return_value=rpc), \
                        mock.patch.object(compaction_recovery, "CompactionModel", return_value=fixture.model), \
                        mock.patch.object(compaction_recovery, "check_compaction") as verify:
                    SMOKE.probe.configure.side_effect = lambda root, *_args, **_kwargs: (root / "codex-home").mkdir()
                    case = compaction_recovery.compaction_case(
                        fixture.binary, fixture.output, "compaction", "v2", "frame-advice", mid_turn)
                self.assertFalse(case["passed"])
                self.assertIn("error", case)
                self.assertEqual(case["returncode"], 0)
                self.assertEqual(case["requestCount"], 1)
                self.assertEqual(case["threadId"], "thread")
                self.assertEqual(case["compactionTurnId"], "compact")
                self.assertEqual(case["compactionStatus"], status)
                self.assertEqual(case.get("seedTurnId"), None if mid_turn else "seed")
                self.assertEqual(case["invalidStdoutLines"], [2])
                self.assertEqual(case["appServerEvents"], [
                    {"method": "turn/completed", "threadId": "thread", "turnId": "compact", "status": status}])
                verify.assert_not_called()
                self.assert_teardown(fixture, name="compaction")

    def test_compaction_deadline_preserves_the_observed_wait_and_partial_final_line(self):
        import compaction_recovery

        def interrupted_case(_codex, output, name, *_scenario):
            root = output / name
            root.mkdir()
            SMOKE.probe.write_json(root / "requests.json", [{}, {}])
            events = [
                {"method": "turn/started", "params": {"threadId": "t", "turn": {"id": "u", "status": "inProgress"}}},
                {"method": "item/started", "params": {"threadId": "t", "turnId": "u",
                                                      "item": {"type": "contextCompaction", "id": "c"}}},
                {"method": "error", "params": {"threadId": "t", "turnId": "u", "willRetry": True,
                                               "error": {"codexErrorInfo": "serverOverloaded",
                                                         "additionalDetails": "Retry scheduled in 324s."}}},
                {"id": 3, "result": {}},
            ]
            rows = b"".join((json.dumps(e) + "\n").encode() for e in events)
            (root / "app-stdout.jsonl").write_bytes(rows + b'{"method":"turn/completed"')
            raise RuntimeError("app-server protocol deadline expired")

        with self.fixture(main=True) as fixture:
            args = SimpleNamespace(output=fixture.output, codex=fixture.binary, package=None, compaction_only=True)
            with mock.patch.object(compaction_recovery, "COMPACTION_CASES", compaction_recovery.COMPACTION_CASES[:1]), \
                    mock.patch.object(compaction_recovery, "compaction_case", side_effect=interrupted_case), \
                    mock.patch("builtins.print"):
                self.assertEqual(SMOKE.main(args), 1)
            case, = json.loads((fixture.output / "result.json").read_text())["cases"]
            self.assertEqual(case["error"], "RuntimeError: app-server protocol deadline expired")
            self.assertEqual(case["requestCount"], 2)
            self.assertEqual(case["invalidStdoutLines"], [5])
            self.assertNotIn("returncode", case)
            self.assertEqual(case["appServerEvents"], [
                {"method": "turn/started", "threadId": "t", "turnId": "u", "status": "inProgress"},
                {"method": "item/started", "threadId": "t", "turnId": "u",
                 "itemType": "contextCompaction", "itemId": "c"},
                {"method": "error", "threadId": "t", "turnId": "u", "willRetry": True,
                 "reason": "serverOverloaded", "scheduledSeconds": 324},
            ])

    def test_app_server_schedule_requires_a_capacity_retry_and_numeric_details(self):
        details = "Retry scheduled in 0s."
        for kind, retry, text, expected in (
                ("policyCapacityUnavailable", True, details, 0),
                ("policyCapacityUnavailable", False, details, None),
                ({"httpConnectionFailed": {"httpStatusCode": 403}}, True, details, None),
                ("serverOverloaded", True, "Please try a different model.", None),
                ("serverOverloaded", True, details + " private suffix", None),
                ("serverOverloaded", True, None, None)):
            params = {"threadId": "t", "turnId": "u", "willRetry": retry,
                      "error": {"codexErrorInfo": kind, "additionalDetails": text}}
            with self.subTest(kind=kind, retry=retry, text=text):
                row = SMOKE.probe.provider_fixture.app_server_observation({"method": "error", "params": params})
                if expected is None:
                    self.assertNotIn("scheduledSeconds", row)
                else:
                    self.assertEqual(row["scheduledSeconds"], expected)
                self.assertNotIn("additionalDetails", row)

    def test_partial_compaction_can_report_a_final_refusal_from_the_completion_alone(self):
        error = {"message": "private provider message", "additionalDetails": "private details",
                 "codexErrorInfo": {"httpConnectionFailed": {"httpStatusCode": 403}}}
        event = {"method": "turn/completed", "params": {"threadId": "t", "turn": {
            "id": "u", "status": "failed", "error": error}}}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "app-stdout.jsonl").write_text(json.dumps(event))
            result = SMOKE.partial_observations(root, compaction=True)
        self.assertEqual(result, {"appServerEvents": [{"method": "turn/completed", "threadId": "t",
                         "turnId": "u", "status": "failed", "reason": "httpConnectionFailed", "httpStatusCode": 403}]})

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
