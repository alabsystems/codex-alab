# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Renewal's actual main/drive/host cleanup with inert process and file owners."""
from contextlib import contextmanager
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
import unittest
from unittest import mock

import test_renewal_tail_cleanup as tail


RENEWAL = tail.RENEWAL
CONFIGURE = RENEWAL.probe.configure


class RenewalHostCleanupTests(unittest.TestCase):
    capture = tail.RenewalTailTests.capture

    @contextmanager
    def fixture(self, primary, *, faults=None, timeout=False, retired=False,
                retire_on=(), diagnostic_before_poll=False, after_popen=False):
        faults = faults or {}
        with tail.RenewalTailTests.fixture(self, RuntimeError("unused configure sentinel")) as fixture:
            actions, opened_logs, log_closes, selected_errors, export_errors = [], {}, [], [], []
            pipe = (fixture.world.root / "owned-inert-host-stdout").open("w+b")
            touch, open_file = Path.touch, Path.open
            write, fields = RENEWAL.probe.write_json, RENEWAL.probe.case_error_fields
            previous, stops, waits = sys.gettrace(), 0, 0

            def fail(phase):
                if phase in faults:
                    raise faults[phase]

            class Stream:
                def fileno(self):
                    return pipe.fileno()
                def close(self):
                    actions.append(("stdout.close",))
                    pipe.close()
                    fail("pipe")

            class Child:
                pid = 123456
                returncode = 0 if retired else None
                stdout = Stream()
                def wait(self, timeout=None):
                    nonlocal waits
                    waits += 1
                    phase = "wait" if waits == 1 else "wait10"
                    actions.append((phase, timeout))
                    if phase in retire_on:
                        self.returncode = 0
                    if phase == "wait" and timeout_first:
                        raise subprocess.TimeoutExpired("inert owned host", timeout)
                    fail(phase)
                    self.returncode = 0
                    return 0
                def terminate(self):
                    actions.append(("terminate",))
                    if "terminate" in retire_on:
                        self.returncode = 0
                    fail("terminate")
                def poll(self):
                    actions.append(("poll",))
                    if "poll" in retire_on:
                        self.returncode = 0
                    fail("poll")
                    return self.returncode
                def kill(self):
                    actions.append(("forbidden-kill",))
                    raise AssertionError("host kill forbidden")

            timeout_first, child = timeout, Child()

            def spawn(command, **kwargs):
                actions.append(("popen", tuple(command), kwargs))
                fail("popen")
                if faults.get("host-wait-collision"):
                    (fixture.output / "host-wait.json").mkdir()
                if after_popen:
                    caller = sys._getframe(1)
                    def boundary(frame, event, _argument):
                        if frame is caller and event == "line" and frame.f_locals.get("process") is child:
                            sys.settrace(previous)
                            frame.f_trace = None
                            raise primary
                        return boundary
                    caller.f_trace = boundary
                    sys.settrace(boundary)
                return child

            def sample(*_args, **_kwargs):
                actions.append(("drive.select",))
                if primary is not None:
                    raise primary
                return [], [], []

            def publish_stop(path, *args, **kwargs):
                nonlocal stops
                if path == fixture.output / "stop":
                    stops += 1
                    actions.append(("stop", stops))
                    if stops == 1:
                        fail("drive-stop")
                    elif stops == 2:
                        fail("host-stop")
                return touch(path, *args, **kwargs)

            def diagnostics(root, process, budget):
                actions.append(("diagnostics", budget))
                if diagnostic_before_poll:
                    fail("diagnostics")
                process.poll()  # Match the existing diagnostic helper's owned poll.
                fail("diagnostics")

            def export(path, value):
                if path == fixture.output / "host-wait.json":
                    actions.append(("host-wait.write", value))
                try:
                    return write(path, value)
                except BaseException as error:
                    if path == fixture.output / "host-wait.json":
                        export_errors.append(error)
                    raise

            def error_fields(error):
                selected_errors.append(error)
                return fields(error)

            class Log:
                def __init__(self, name, handle):
                    self.name_key, self.handle = name, handle
                def __getattr__(self, name):
                    return getattr(self.handle, name)
                def __enter__(self):
                    return self
                def __exit__(self, *_args):
                    self.close()
                def close(self):
                    log_closes.append(self.name_key)
                    self.handle.close()
                    fail("log." + self.name_key)

            def open_log(path, *args, **kwargs):
                handle = open_file(path, *args, **kwargs)
                names = {fixture.output / "host-stdout.jsonl": "stdout",
                         fixture.output / "host-stderr.txt": "stderr"}
                if path in names and args and args[0] == "xb":
                    result = Log(names[path], handle)
                    opened_logs[result.name_key] = result
                    return result
                return handle

            arguments = list(sys.argv)
            if primary is None:
                arguments[arguments.index("--case") + 1] = "five-hour-wall-soon"
            with mock.patch.object(sys, "argv", arguments), \
                    mock.patch.object(RENEWAL.probe, "configure", new=CONFIGURE), \
                    mock.patch.object(RENEWAL.subprocess, "Popen", new=spawn), \
                    mock.patch.object(RENEWAL.select, "select", new=sample), \
                    mock.patch.object(RENEWAL, "host_exit_diagnostics", new=diagnostics), \
                    mock.patch.object(RENEWAL, "wall_case_finished", return_value=True), \
                    mock.patch.object(RENEWAL.time, "monotonic", return_value=100.0), \
                    mock.patch.object(Path, "touch", new=publish_stop), \
                    mock.patch.object(Path, "open", new=open_log), \
                    mock.patch.object(RENEWAL.probe, "write_json", new=export), \
                    mock.patch.object(RENEWAL.probe, "case_error_fields", new=error_fields), \
                    mock.patch.object(os, "kill", side_effect=AssertionError("raw signal forbidden")) as raw_signal, \
                    mock.patch.object(os, "killpg", side_effect=AssertionError("group signal forbidden")) as group_signal, \
                    mock.patch.object(socket, "socket", side_effect=AssertionError("socket forbidden")) as sockets, \
                    mock.patch.object(threading, "Thread", side_effect=AssertionError("thread forbidden")) as threads:
                try:
                    fixture.actions_host = actions
                    fixture.pipe = pipe
                    fixture.child = child
                    fixture.selected_errors = selected_errors
                    fixture.export_errors = export_errors
                    fixture.opened_logs = opened_logs
                    fixture.log_closes = log_closes
                    yield fixture
                finally:
                    sys.settrace(previous)
                    # Fallback owns only inert regular handles; it is not retirement evidence.
                    pipe.close()
                    for log in opened_logs.values():
                        log.handle.close()
                    self.assertCountEqual(log_closes, list(opened_logs))
                    self.assertEqual(sum(action[0] == "popen" for action in actions), 1)
                    self.assertLessEqual(sum(action[0] == "terminate" for action in actions), 1)
                    self.assertLessEqual(sum(action[0] == "stdout.close" for action in actions), 1)
                    if child.returncode is None:
                        self.assertFalse(any(action[0] == "stdout.close" for action in actions))
                    self.assertLessEqual(sum(action[0] == "poll" for action in actions), 1)
                    observed_waits = [action for action in actions if action[0] in ("wait", "wait10")]
                    self.assertLessEqual(len(observed_waits), 2)
                    for phase, bound in observed_waits:
                        self.assertEqual(bound, RENEWAL.HOST_EXIT_SECONDS if phase == "wait" else 10)
                    refusal = faults.get("terminate")
                    if refusal is not None and not isinstance(refusal, ProcessLookupError):
                        self.assertFalse(any(action[0] == "wait10" for action in actions))
                    self.assertFalse(any(action[0] == "forbidden-kill" for action in actions))
                    for operation in (raw_signal, group_signal, sockets, threads):
                        operation.assert_not_called()

    def report(self, fixture):
        report = json.loads((fixture.output / "result.json").read_text())
        self.assertIs(report["passed"], False)
        self.assertIs(report["nativeHost"]["qualifies"], False)
        self.assertIs(report["package"]["qualifies"], False)
        return report

    def assert_primary(self, fixture, primary):
        caught, status = self.capture()
        if isinstance(primary, Exception):
            self.assertIsNone(caught)
            self.assertEqual(status, 1)
            self.assertTrue(any(error is primary for error in fixture.selected_errors))
        else:
            self.assertIs(caught, primary)
        self.assertEqual(sum(action[0] == "popen" for action in fixture.actions_host), 1)
        self.assertEqual(sum(action[0] == "drive.select" for action in fixture.actions_host), 1)
        return self.report(fixture)

    def test_selected_drive_error_survives_every_host_cleanup_phase(self):
        for phase in ("drive-stop", "host-stop", "wait", "diagnostics", "terminate", "wait10",
                      "pipe", "host-wait-collision", "log.stderr", "log.stdout"):
            for kind in (RuntimeError, KeyboardInterrupt, SystemExit, StopIteration):
                primary = kind(0 if kind is SystemExit else "drive failed")
                faults = {phase: True if phase == "host-wait-collision" else OSError("cleanup " + phase)}
                with self.subTest(phase=phase, primary=kind.__name__), self.fixture(primary, faults=faults,
                        timeout=phase in ("diagnostics", "terminate", "wait10")) as fixture:
                    report = self.assert_primary(fixture, primary)
                    self.assertEqual(report.get("interrupted", False), not isinstance(primary, Exception))
                    self.assertIn("cleanup", json.dumps(report))

    def test_wait_error_after_reap_still_closes_stdout_and_exports_actual_status(self):
        primary = KeyboardInterrupt("drive")
        with self.fixture(primary, faults={"wait": OSError("after reap")}, retire_on=("wait",)) as fixture:
            self.assert_primary(fixture, primary)
            self.assertTrue(fixture.pipe.closed)
            record = json.loads((fixture.output / "host-wait.json").read_text())
            self.assertEqual((record["pid"], record["returncode"], record["stalled"]), (123456, 0, False))

    def test_unconfirmed_retirement_skips_buffered_close_and_exports_explicit_failure(self):
        for faults, timed_out, before_poll in (
                ({"wait": OSError("wait")}, False, False),
                ({"wait": OSError("wait"), "poll": OSError("poll")}, False, False),
                ({"terminate": OSError("refused")}, True, False),
                ({"wait10": subprocess.TimeoutExpired("owned host", 10)}, True, False),
                ({"diagnostics": OSError("before poll"), "wait10": OSError("wait10")}, True, True)):
            primary = KeyboardInterrupt("drive")
            with self.subTest(faults=tuple(faults)), self.fixture(primary, faults=faults,
                    timeout=timed_out, diagnostic_before_poll=before_poll) as fixture:
                report = self.assert_primary(fixture, primary)
                self.assertFalse(fixture.pipe.closed)
                record = json.loads((fixture.output / "host-wait.json").read_text())
                self.assertIsNone(record["returncode"])
                self.assertIs(record["retirementConfirmed"], False)
                self.assertIs(record["stdoutCloseSkipped"], True)
                self.assertIn("retirement unconfirmed", json.dumps(report))

    def test_timeout_diagnostics_keep_budget_and_cannot_skip_safe_termination_wait(self):
        primary = KeyboardInterrupt("drive")
        with self.fixture(primary, faults={"diagnostics": OSError("sample failed")}, timeout=True) as fixture:
            report = self.assert_primary(fixture, primary)
            self.assertEqual([action for action in fixture.actions_host if action[0] in ("wait", "wait10")],
                             [("wait", RENEWAL.HOST_EXIT_SECONDS), ("wait10", 10)])
            budget = next(action[1] for action in fixture.actions_host if action[0] == "diagnostics")
            self.assertEqual(budget, report["observationBudgetSeconds"]["guardian"] - 20)
            self.assertTrue(fixture.pipe.closed)
            self.assertTrue(json.loads((fixture.output / "host-wait.json").read_text())["stalled"])

    def test_terminate_refusals_and_process_lookup_keep_bounded_owned_authority(self):
        for failure in (OSError("stop"), KeyboardInterrupt("stop"), SystemExit(0), ProcessLookupError("gone")):
            primary = KeyboardInterrupt("drive")
            with self.subTest(failure=type(failure).__name__), self.fixture(primary,
                    faults={"terminate": failure}, timeout=True) as fixture:
                self.assert_primary(fixture, primary)
                self.assertEqual(fixture.pipe.closed, isinstance(failure, ProcessLookupError))
                self.assertEqual(sum(action[0] == "wait10" for action in fixture.actions_host),
                                 int(isinstance(failure, ProcessLookupError)))

    def test_constructor_failure_and_published_return_boundary_own_only_returned_child(self):
        for after in (False, True):
            primary = KeyboardInterrupt("published Popen boundary")
            with self.subTest(after=after), self.fixture(primary, faults={} if after else {"popen": primary},
                    after_popen=after) as fixture:
                caught, _ = self.capture()
                self.assertIs(caught, primary)
                self.report(fixture)
                self.assertEqual(fixture.pipe.closed, after)
                self.assertEqual((fixture.output / "host-wait.json").is_file(), after)

    def test_already_retired_host_retains_normal_wait_record_without_extra_native_actions(self):
        primary = KeyboardInterrupt("drive")
        with self.fixture(primary, retired=True) as fixture:
            self.assert_primary(fixture, primary)
            self.assertTrue(fixture.pipe.closed)
            self.assertFalse(any(action[0] in ("wait", "wait10", "terminate", "diagnostics", "poll")
                                 for action in fixture.actions_host))
            self.assertEqual(json.loads((fixture.output / "host-wait.json").read_text()),
                             {"pid": 123456, "returncode": 0, "stalled": False})

    def test_clean_host_cleanup_still_reaches_the_unchanged_case_oracle(self):
        with self.fixture(None) as fixture:
            caught, status = self.capture()
            self.assertIsNone(caught)
            self.assertEqual(status, 1)
            report = self.report(fixture)
            self.assertIn("the source was refused 0 times, not once", report["error"])
            self.assertTrue(fixture.pipe.closed)
            self.assertEqual(json.loads((fixture.output / "host-wait.json").read_text()),
                             {"pid": 123456, "returncode": 0, "stalled": False})

    def test_close_only_zero_and_log_fault_keep_the_prior_selected_failure(self):
        zero = SystemExit(0)
        with self.fixture(None, faults={"pipe": zero, "log.stdout": SystemExit(0)}) as fixture:
            caught, status = self.capture()
            self.assertIsNone(caught)
            self.assertEqual(status, 1)
            self.report(fixture)
            selected = [error for error in fixture.selected_errors
                        if isinstance(error, RuntimeError) and error.__cause__ is zero]
            self.assertTrue(selected)
            self.assertTrue(fixture.pipe.closed)
            self.assertTrue((fixture.output / "host-wait.json").is_file())

    def test_host_wait_collision_keeps_drive_primary_and_other_evidence(self):
        primary = KeyboardInterrupt("drive")
        with self.fixture(primary, faults={"host-wait-collision": True}) as fixture:
            self.assert_primary(fixture, primary)
            self.assertTrue(fixture.pipe.closed)
            self.assertEqual(len(fixture.export_errors), 1)
            self.assertIsInstance(fixture.export_errors[0], FileExistsError)
            self.assertTrue(all((fixture.output / name).is_file() for name in tail.TRACES))

    def test_host_logs_keep_original_handles_and_command_arguments(self):
        primary = KeyboardInterrupt("drive")
        with self.fixture(primary) as fixture:
            self.assert_primary(fixture, primary)
            popen = next(action for action in fixture.actions_host if action[0] == "popen")
            self.assertEqual(popen[1][1:], ("--renewal-config", str(fixture.output / "fixture.json")))
            self.assertIs(popen[2]["stdin"], subprocess.DEVNULL)
            self.assertIs(popen[2]["stdout"], subprocess.PIPE)
            self.assertIs(popen[2]["stderr"], fixture.opened_logs["stderr"])
            self.assertEqual(popen[2]["stderr"].name, str(fixture.output / "host-stderr.txt"))

    def test_host_and_log_diagnostic_hooks_cannot_replace_selected_cancellation(self):
        class Primary(KeyboardInterrupt):
            def add_note(self, _note):
                raise SystemExit(0)
            def __setattr__(self, _name, _value):
                raise SystemExit(0)
        class Secondary(OSError):
            def __str__(self):
                raise SystemExit(0)
        for phase in ("host-stop", "diagnostics"):
            primary = Primary("drive cancelled")
            with self.subTest(phase=phase), self.fixture(primary,
                    faults={phase: Secondary(), "log.stdout": Secondary()},
                    timeout=phase == "diagnostics") as fixture:
                self.assert_primary(fixture, primary)
                self.assertTrue(fixture.pipe.closed)
                self.assertTrue((fixture.output / "host-wait.json").is_file())


if __name__ == "__main__":
    unittest.main()
