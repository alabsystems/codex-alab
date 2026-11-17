# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Public runner output ownership with real files and inert process seams."""
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


class RunnerOutputOwnershipTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-runner-output-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.sequence = 0

    @contextmanager
    def fixture(self, *, primary=None, phase="popen", close_errors=None,
                open_errors=None, cancel_after=None, existing=None, interactive=True):
        root = self.root / str(self.sequence)
        self.sequence += 1
        root.mkdir()
        env = probe.fresh_environment(root)
        paths = {"stdout": root / "exec-stdout.jsonl", "stderr": root / "exec-stderr.txt"}
        if existing is not None:
            paths[existing].write_bytes(b"keep existing log\n")
        close_errors, open_errors = close_errors or {}, open_errors or {}
        handles, actions, attempts, callback_names = {}, [], [], []
        real_open, previous_trace = Path.open, sys.gettrace()
        saved = {number: object() for number in SIGNALS}
        handlers = dict(saved)
        child_pipe = (root / "owned-child-pipe").open("w+b") if phase == "child" else None

        class Output:
            def __init__(self, name, handle):
                self.name_key, self.handle = name, handle
            def __getattr__(self, name):
                return getattr(self.handle, name)
            def __enter__(self):
                return self
            def __exit__(self, *_args):
                self.close()
            def close(self):
                actions.append(("close", self.name_key))
                self.handle.close()
                if self.name_key in close_errors:
                    raise close_errors[self.name_key]

        def open_log(path, mode="r", *args, **kwargs):
            name = next((name for name, target in paths.items() if path == target), None)
            if name is None or mode != "xb":
                return real_open(path, mode, *args, **kwargs)
            actions.append(("open", name, mode))
            if name in open_errors:
                raise open_errors[name]
            handle = Output(name, real_open(path, mode, *args, **kwargs))
            handles[name] = handle
            if name == cancel_after:
                caller = sys._getframe(1)
                def boundary(frame, event, _argument):
                    if frame is caller and event == "line":
                        sys.settrace(previous_trace)
                        frame.f_trace = None
                        raise primary
                    return boundary
                caller.f_trace = boundary
                sys.settrace(boundary)
            return handle

        def child_close():
            actions.append(("child-close",))
            child_pipe.close()
            raise primary

        child = SimpleNamespace(returncode=0, stdin=None,
            stdout=SimpleNamespace(close=child_close) if child_pipe is not None else None,
            wait=lambda: 0, communicate=lambda: (b"retained trailing output\n", None))
        executable = root / "never-executed"

        def spawn(*args, **kwargs):
            attempts.append((args, kwargs))
            if primary is not None and phase == "popen":
                raise primary
            (root / "exec-guardian-result.json").write_text(json.dumps({"schema": 1,
                "argv": [str(executable)], "timeout_seconds": probe.COMMAND_TIMEOUT_SECONDS,
                "returncode": 0, "timed_out": False}))
            if not interactive:
                kwargs["stdout"].write(b"retained direct output\n")
            return child

        def set_handler(number, handler):
            previous = handlers[number]
            handlers[number] = handler
            if handler is saved[number]:
                actions.append(("restore", number))
                if number == signal.SIGINT and phase == "signal" and primary is not None:
                    raise primary
            return previous

        def interact(_child, stdout):
            callback_names.append(stdout.name)
            if primary is not None and phase == "interact":
                raise primary

        def run():
            try:
                return None, probe.run(executable, [], root, env, "exec",
                                       interact=interact if interactive else None)
            except BaseException as error:
                return error, None

        with mock.patch.object(Path, "open", new=open_log), \
                mock.patch.object(probe.subprocess, "Popen", new=spawn), \
                mock.patch.object(probe.subprocess, "run", side_effect=AssertionError("extra child forbidden")) as extra_child, \
                mock.patch.object(signal, "getsignal", new=lambda number: handlers[number]), \
                mock.patch.object(signal, "signal", new=set_handler), \
                mock.patch.object(os, "kill", side_effect=AssertionError("raw PID signal forbidden")) as raw_signal, \
                mock.patch.object(os, "killpg", side_effect=AssertionError("group signal forbidden")) as group_signal:
            try:
                yield SimpleNamespace(root=root, paths=paths, handles=handles, actions=actions,
                    attempts=attempts, callback_names=callback_names, run=run)
                self.assertEqual(handlers, saved)
            finally:
                sys.settrace(previous_trace)
                for output in handles.values():
                    output.handle.close()
                if child_pipe is not None:
                    child_pipe.close()
                closes = [entry[1] for entry in actions if entry[0] == "close"]
                self.assertCountEqual(closes, list(handles), "each owned output needs exactly one close attempt")
                extra_child.assert_not_called()
                raw_signal.assert_not_called()
                group_signal.assert_not_called()

    def assert_closed(self, fixture):
        self.assertTrue(fixture.handles)
        self.assertTrue(all(output.handle.closed for output in fixture.handles.values()))
        self.assertEqual({entry[1] for entry in fixture.actions if entry[0] == "close"},
                         set(fixture.handles))

    def test_body_child_and_signal_primary_survive_both_output_close_faults(self):
        for phase in ("popen", "interact", "child", "signal"):
            for kind in (RuntimeError, KeyboardInterrupt, SystemExit, StopIteration):
                for name in ("stdout", "stderr"):
                    primary = kind(0 if kind is SystemExit else "selected " + phase)
                    with self.subTest(phase=phase, primary=kind.__name__, name=name), \
                            self.fixture(primary=primary, phase=phase, close_errors={name: SystemExit(0)}) as fixture:
                        previous, selected = sys.gettrace(), []

                        def observe(frame, event, argument):
                            if (event == "exception" and frame.f_code is probe.run.__code__
                                    and isinstance(argument[1], RuntimeError) and argument[1].__cause__ is primary):
                                selected.append(argument[1])
                            return observe

                        try:
                            sys.settrace(observe)
                            caught, result = fixture.run()
                        finally:
                            sys.settrace(previous)
                        if kind is SystemExit and phase in ("child", "signal"):
                            self.assertTrue(selected, "the existing phase did not select its normalized failure")
                            self.assertIs(caught, selected[0])
                            self.assertIs(caught.__cause__, primary)
                        else:
                            self.assertIs(caught, primary)
                        self.assertIsNone(result)
                        self.assert_closed(fixture)
                        self.assertIn("output", json.dumps(probe.case_error_fields(caught)))

    def test_second_open_failure_keeps_primary_despite_first_log_close_failure(self):
        for kind in (OSError, KeyboardInterrupt, SystemExit, StopIteration):
            for secondary in (OSError("stdout close failed"), SystemExit(0)):
                primary = kind(0 if kind is SystemExit else "stderr acquisition")
                with self.subTest(primary=kind.__name__, secondary=type(secondary).__name__), self.fixture(
                        open_errors={"stderr": primary}, close_errors={"stdout": secondary}) as fixture:
                    caught, result = fixture.run()
                    self.assertIs(caught, primary)
                    self.assertIsNone(result)
                    self.assertEqual(fixture.attempts, [])
                    self.assertEqual(set(fixture.handles), {"stdout"})
                    self.assert_closed(fixture)

    def test_close_only_failure_refuses_success_and_preserves_native_error_or_exact_zero_cause(self):
        for name in ("stdout", "stderr"):
            for kind, code in ((OSError, "close"), (KeyboardInterrupt, "close"),
                               (SystemExit, 47), (SystemExit, 0), (StopIteration, "close")):
                primary = kind(code)
                with self.subTest(name=name, primary=kind.__name__, code=code), \
                        self.fixture(close_errors={name: primary}) as fixture:
                    caught, result = fixture.run()
                    self.assertIsNone(result)
                    if kind is SystemExit and code == 0:
                        self.assertIsInstance(caught, RuntimeError)
                        self.assertIs(caught.__cause__, primary)
                    else:
                        self.assertIs(caught, primary)
                    self.assert_closed(fixture)

    def test_multiple_close_faults_apply_existing_control_selection_after_both_attempts(self):
        for first, last in ((OSError("stderr"), KeyboardInterrupt("stdout")),
                            (SystemExit(0), SystemExit(47)),
                            (OSError("stderr"), OSError("stdout")),
                            (StopIteration("stderr"), OSError("stdout"))):
            selected = last if isinstance(last, (KeyboardInterrupt, SystemExit)) else first
            with self.subTest(first=type(first).__name__, last=type(last).__name__), \
                    self.fixture(close_errors={"stderr": first, "stdout": last}) as fixture:
                caught, result = fixture.run()
                self.assertIs(caught, selected)
                self.assertIsNone(result)
                self.assert_closed(fixture)
                self.assertEqual([entry for entry in fixture.actions if entry[0] == "close"],
                                 [("close", "stderr"), ("close", "stdout")])

    def test_visible_post_open_cancellation_closes_each_returned_log(self):
        for name in ("stdout", "stderr"):
            primary = KeyboardInterrupt("post-open")
            with self.subTest(name=name), self.fixture(primary=primary,
                    phase="acquisition", cancel_after=name) as fixture:
                caught, result = fixture.run()
                self.assertIs(caught, primary)
                self.assertIsNone(result)
                self.assertEqual(fixture.attempts, [])
                self.assert_closed(fixture)

    def test_existing_output_is_never_overwritten_and_no_process_boundary_runs(self):
        for name in ("stdout", "stderr"):
            with self.subTest(name=name), self.fixture(existing=name) as fixture:
                caught, result = fixture.run()
                self.assertIsInstance(caught, FileExistsError)
                self.assertIsNone(result)
                self.assertEqual(fixture.paths[name].read_bytes(), b"keep existing log\n")
                self.assertEqual(fixture.attempts, [])
                self.assertTrue(all(output.handle.closed for output in fixture.handles.values()))

    def test_original_handles_names_output_and_guardian_status_are_preserved(self):
        for interactive in (False, True):
            with self.subTest(interactive=interactive), self.fixture(interactive=interactive) as fixture:
                caught, result = fixture.run()
                self.assertIsNone(caught)
                self.assertEqual(result, 0)
                self.assert_closed(fixture)
                self.assertEqual(len(fixture.attempts), 1)
                kwargs = fixture.attempts[0][1]
                self.assertIs(kwargs["stderr"], fixture.handles["stderr"])
                self.assertEqual(kwargs["stderr"].name, str(fixture.paths["stderr"]))
                if interactive:
                    self.assertIs(kwargs["stdout"], subprocess.PIPE)
                    self.assertEqual(fixture.callback_names, [str(fixture.paths["stdout"])])
                else:
                    self.assertIs(kwargs["stdout"], fixture.handles["stdout"])
                self.assertEqual(fixture.paths["stdout"].read_bytes(),
                    b"retained trailing output\n" if interactive else b"retained direct output\n")
                self.assertEqual([entry for entry in fixture.actions if entry[0] == "open"],
                                 [("open", "stdout", "xb"), ("open", "stderr", "xb")])

    def test_hostile_exception_hooks_cannot_replace_primary_through_output_exit(self):
        class Primary(RuntimeError):
            def add_note(self, _note):
                raise SystemExit(0)
            def __setattr__(self, _name, _value):
                raise SystemExit(0)
        class Secondary(OSError):
            def __str__(self):
                raise SystemExit(0)
        primary = Primary("body failure")
        with self.fixture(primary=primary, close_errors={"stdout": Secondary(), "stderr": Secondary()}) as fixture:
            caught, result = fixture.run()
            self.assertIs(caught, primary)
            self.assertIsNone(result)
            self.assert_closed(fixture)

    def test_first_open_failure_owns_nothing_and_keeps_its_identity(self):
        for primary in (OSError("stdout"), KeyboardInterrupt("stdout"), StopIteration("stdout")):
            with self.subTest(primary=type(primary).__name__), self.fixture(open_errors={"stdout": primary}) as fixture:
                caught, result = fixture.run()
                self.assertIs(caught, primary)
                self.assertIsNone(result)
                self.assertEqual(fixture.handles, {})
                self.assertEqual(fixture.attempts, [])


if __name__ == "__main__":
    unittest.main()
