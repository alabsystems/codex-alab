# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Shared public runner cleanup with inert children and owned regular handles."""
from contextlib import contextmanager
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import capacity_exec as probe

SIGNALS = (signal.SIGINT, signal.SIGTERM, signal.SIGHUP, signal.SIGCHLD)


class RunnerChildCleanupTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-runner-cleanup-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.sequence = 0

    @contextmanager
    def fixture(self, *, primary=None, retired=False, stop_error=None, wait_error=None,
                poll_error=None, retire_before_error=False, poll_retires=False,
                pipe_errors=None, restoration_error=None):
        root = self.root / str(self.sequence)
        self.sequence += 1
        root.mkdir()
        env = probe.fresh_environment(root)
        actions, handles, saved = [], {}, {number: object() for number in SIGNALS}
        handlers = dict(saved)
        pipe_errors = pipe_errors or {}

        class Pipe:
            def __init__(self, name):
                self.name = name
                self.handle = (root / ("owned-" + name)).open("w+b")
                handles[name] = self.handle

            def close(self):
                actions.append(("close", self.name))
                self.handle.close()
                if self.name in pipe_errors:
                    raise pipe_errors[self.name]

        class Child:
            def __init__(self):
                self.returncode = 0 if retired else None
                self.stdin, self.stdout = Pipe("stdin"), Pipe("stdout")

            def terminate(self):
                actions.append(("terminate",))
                if stop_error is not None:
                    if retire_before_error:
                        self.returncode = 0
                    raise stop_error

            def wait(self, timeout=None):
                actions.append(("wait", timeout))
                if wait_error is not None:
                    if retire_before_error:
                        self.returncode = 0
                    raise wait_error
                self.returncode = 0
                return 0

            def poll(self):
                actions.append(("poll",))
                if poll_retires:
                    self.returncode = 0
                if poll_error is not None:
                    if retire_before_error:
                        self.returncode = 0
                    raise poll_error
                return self.returncode

            def communicate(self):
                actions.append(("communicate",))
                self.returncode = 0
                return b"retained trailing output\n", None

            def kill(self):
                actions.append(("forbidden-kill",))
                raise AssertionError("guardian kill forbidden")

        child = Child()
        executable = root / "never-executed"

        def spawn(*_args, **_kwargs):
            actions.append(("spawn",))
            (root / "exec-guardian-result.json").write_text(json.dumps({"schema": 1,
                "argv": [str(executable)], "timeout_seconds": probe.COMMAND_TIMEOUT_SECONDS,
                "returncode": 0, "timed_out": False}))
            return child

        def set_handler(number, handler):
            previous = handlers[number]
            handlers[number] = handler
            if handler is saved[number]:
                actions.append(("restore", number))
                if number == signal.SIGINT and restoration_error is not None:
                    raise restoration_error
            return previous

        def interact(_child, _stdout):
            if primary is not None:
                raise primary

        def run():
            try:
                return None, probe.run(executable, [], root, env, "exec", interact=interact)
            except BaseException as error:
                return error, None

        with mock.patch.object(probe.subprocess, "Popen", new=spawn), \
                mock.patch.object(probe.subprocess, "run", side_effect=AssertionError("extra child forbidden")) as extra_child, \
                mock.patch.object(signal, "getsignal", new=lambda number: handlers[number]), \
                mock.patch.object(signal, "signal", new=set_handler), \
                mock.patch.object(os, "kill", side_effect=AssertionError("raw PID signaling forbidden")) as raw_signal, \
                mock.patch.object(os, "killpg", side_effect=AssertionError("group signaling forbidden")) as group_signal:
            try:
                yield SimpleNamespace(root=root, child=child, actions=actions, handles=handles, run=run)
                self.assertEqual(handlers, saved)
                self.assertEqual([entry for entry in actions if entry[0] == "restore"],
                                 [("restore", number) for number in SIGNALS])
            finally:
                # These regular-file handles have no live child or blocking pipe.
                # Fixture fallback is distinct from the runner's retirement proof.
                for handle in handles.values():
                    handle.close()
                extra_child.assert_not_called()
                raw_signal.assert_not_called()
                group_signal.assert_not_called()
                self.assertFalse(any(entry[0] == "forbidden-kill" for entry in actions))
                waits = [entry for entry in actions if entry[0] == "wait"]
                self.assertLessEqual(len(waits), 1, "cleanup retried its owned wait")
                if stop_error is not None and not isinstance(stop_error, ProcessLookupError):
                    self.assertEqual(waits, [], "cleanup waited after stop refusal")
                for _phase, timeout in waits:
                    self.assertIsNotNone(timeout, "cleanup requested an unbounded wait")
                    self.assertGreater(timeout, 0)
                    self.assertLessEqual(timeout, 5)
                self.assertLessEqual(actions.count(("terminate",)), 1)
                self.assertLessEqual(actions.count(("poll",)), 1)

    def fields(self, error):
        return json.dumps(probe.case_error_fields(error))

    def test_retired_child_closes_both_pipes_independently_and_keeps_primary(self):
        for name in ("stdin", "stdout"):
            for primary_type in (RuntimeError, KeyboardInterrupt, SystemExit):
                for failure in (OSError("pipe close failed"), KeyboardInterrupt("pipe close failed"), SystemExit(0)):
                    primary = primary_type(0 if primary_type is SystemExit else "body failure")
                    with self.subTest(name=name, primary=primary_type.__name__, failure=type(failure).__name__), \
                            self.fixture(primary=primary, retired=True, pipe_errors={name: failure}) as fixture:
                        caught, result = fixture.run()
                        self.assertIs(caught, primary)
                        self.assertIsNone(result)
                        self.assertEqual([entry for entry in fixture.actions if entry[0] == "close"],
                                         [("close", "stdin"), ("close", "stdout")])
                        self.assertTrue(all(handle.closed for handle in fixture.handles.values()))
                        self.assertIn("pipe", self.fields(primary))
                        self.assertIn(type(failure).__name__, self.fields(primary))

    def test_stop_refusal_keeps_primary_skips_wait_and_both_buffered_closes(self):
        for failure in (OSError("stop refused"), KeyboardInterrupt("stop refused"), SystemExit(0)):
            primary = KeyboardInterrupt("body cancellation")
            with self.subTest(failure=type(failure).__name__), self.fixture(primary=primary, stop_error=failure) as fixture:
                caught, result = fixture.run()
                self.assertIs(caught, primary)
                self.assertIsNone(result)
                self.assertEqual([entry for entry in fixture.actions if entry[0] != "restore"],
                                 [("spawn",), ("terminate",), ("poll",)])
                self.assertTrue(all(not handle.closed for handle in fixture.handles.values()))
                self.assertIn("retirement unconfirmed", self.fields(primary))
                self.assertIn("stdin", self.fields(primary))
                self.assertIn("stdout", self.fields(primary))
            self.assertTrue(all(handle.closed for handle in fixture.handles.values()))

    def test_accepted_stop_and_process_lookup_use_a_bounded_wait_then_close(self):
        for failure in (None, ProcessLookupError("already exited")):
            primary = RuntimeError("body failed")
            with self.subTest(failure=type(failure).__name__), self.fixture(primary=primary, stop_error=failure) as fixture:
                caught, _ = fixture.run()
                self.assertIs(caught, primary)
                waits = [entry for entry in fixture.actions if entry[0] == "wait"]
                self.assertEqual(len(waits), 1)
                self.assertIsNotNone(waits[0][1])
                self.assertGreater(waits[0][1], 0)
                self.assertLessEqual(waits[0][1], 5)
                self.assertTrue(all(handle.closed for handle in fixture.handles.values()))

    def test_wait_fault_rechecks_retirement_before_deciding_safe_pipe_closes(self):
        for retired in (False, True):
            for failure in (OSError("wait failed"), KeyboardInterrupt("wait cancelled"), SystemExit(0),
                            subprocess.TimeoutExpired("owned guardian", 5)):
                primary = KeyboardInterrupt("original body")
                with self.subTest(retired=retired, failure=type(failure).__name__), \
                        self.fixture(primary=primary, wait_error=failure, retire_before_error=retired) as fixture:
                    caught, _ = fixture.run()
                    self.assertIs(caught, primary)
                    self.assertEqual(all(handle.closed for handle in fixture.handles.values()), retired)
                    self.assertIn(type(failure).__name__, self.fields(primary))
                    if not retired:
                        self.assertIn(("poll",), fixture.actions)
                        self.assertIn("retirement unconfirmed", self.fields(primary))
                    else:
                        self.assertEqual([entry for entry in fixture.actions if entry[0] == "close"],
                                         [("close", "stdin"), ("close", "stdout")])

    def test_stop_fault_after_retirement_still_closes_pipes_and_records_failure(self):
        primary = RuntimeError("body failed")
        with self.fixture(primary=primary, stop_error=OSError("stop after retirement"), retire_before_error=True) as fixture:
            caught, _ = fixture.run()
            self.assertIs(caught, primary)
            self.assertTrue(all(handle.closed for handle in fixture.handles.values()))
            self.assertFalse(any(entry[0] == "wait" for entry in fixture.actions))
            self.assertIn("stop after retirement", self.fields(primary))

    def test_poll_fault_keeps_primary_and_reports_unconfirmed_ownership(self):
        primary = SystemExit(0)
        with self.fixture(primary=primary, stop_error=OSError("stop refused"), poll_error=OSError("poll refused")) as fixture:
            caught, _ = fixture.run()
            self.assertIs(caught, primary)
            self.assertTrue(all(not handle.closed for handle in fixture.handles.values()))
            self.assertIn("poll refused", self.fields(primary))
            self.assertIn("retirement unconfirmed", self.fields(primary))

    def test_poll_retirement_published_before_return_or_error_allows_safe_closes(self):
        for failure in (None, OSError("poll after retirement")):
            primary = RuntimeError("body failed")
            with self.subTest(poll_error=type(failure).__name__), self.fixture(primary=primary,
                    stop_error=OSError("stop refused"), poll_error=failure, poll_retires=True) as fixture:
                caught, _ = fixture.run()
                self.assertIs(caught, primary)
                self.assertTrue(all(handle.closed for handle in fixture.handles.values()))
                self.assertIn(("poll",), fixture.actions)
                self.assertFalse(any(entry[0] == "wait" for entry in fixture.actions))
                self.assertNotIn("retirement unconfirmed", self.fields(primary))
                if failure is not None:
                    self.assertIn("poll after retirement", self.fields(primary))

    def test_healthy_body_cannot_pass_after_pipe_cleanup_failure(self):
        for failure in (OSError("stdout close failed"), KeyboardInterrupt("stdout close cancelled"),
                        SystemExit(0), SystemExit(47)):
            with self.subTest(failure=type(failure).__name__), self.fixture(pipe_errors={"stdout": failure}) as fixture:
                caught, result = fixture.run()
                self.assertIsNone(result)
                if isinstance(failure, SystemExit) and failure.code == 0:
                    self.assertIsInstance(caught, RuntimeError)
                    self.assertIs(caught.__cause__, failure)
                else:
                    self.assertIs(caught, failure)
                self.assertTrue(all(handle.closed for handle in fixture.handles.values()))

    def test_healthy_interaction_retains_output_and_validated_guardian_status(self):
        with self.fixture() as fixture:
            caught, result = fixture.run()
            self.assertIsNone(caught)
            self.assertEqual(result, 0)
            self.assertTrue(all(handle.closed for handle in fixture.handles.values()))
            self.assertEqual((fixture.root / "exec-stdout.jsonl").read_bytes(), b"retained trailing output\n")
            self.assertFalse(any(entry[0] in ("terminate", "poll", "wait") for entry in fixture.actions))

    def test_signal_restoration_preserves_selected_body_or_child_cleanup_failure(self):
        for has_body in (False, True):
            primary = KeyboardInterrupt("body cancellation") if has_body else None
            pipe_error = OSError("pipe cleanup failed")
            with self.subTest(has_body=has_body), self.fixture(primary=primary, retired=True,
                    pipe_errors={"stdout": pipe_error}, restoration_error=SystemExit(0)) as fixture:
                caught, result = fixture.run()
                self.assertIs(caught, primary if has_body else pipe_error)
                self.assertIsNone(result)
                self.assertTrue(all(handle.closed for handle in fixture.handles.values()))
                self.assertIn("pipe cleanup failed", self.fields(caught))
                self.assertIn("signal restoration", self.fields(caught))


if __name__ == "__main__":
    unittest.main()
