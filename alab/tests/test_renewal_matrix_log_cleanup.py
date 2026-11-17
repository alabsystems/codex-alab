# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Public matrix log ownership and original entry, with inert controller seams."""
import ast
from contextlib import contextmanager, ExitStack, redirect_stderr, redirect_stdout
import errno
import hashlib
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
from types import SimpleNamespace
import unittest
from unittest import mock

import test_renewal_matrix_status as status_fixture


MATRIX = status_fixture.MATRIX


def original_entry(main):
    tree = ast.parse(status_fixture.SOURCE.read_bytes(), filename=str(status_fixture.SOURCE))
    entries = [node for node in tree.body if isinstance(node, ast.If)
               and isinstance(node.test, ast.Compare) and isinstance(node.test.left, ast.Name)
               and node.test.left.id == "__name__"]
    if len(entries) != 1:
        raise AssertionError("expected the one original matrix entry guard")
    namespace = dict(vars(MATRIX), __name__="__main__", main=main)
    exec(compile(ast.Module(body=entries, type_ignores=[]), str(status_fixture.SOURCE), "exec"), namespace)


class RenewalMatrixLogCleanupTests(unittest.TestCase):
    def setUp(self):
        self.world = status_fixture.RenewalMatrixStatusTests()
        self.addCleanup(self.world.doCleanups)
        self.world.setUp()
        self.sequence = 0

    @contextmanager
    def fixture(self, *, primary=None, close_error=None, open_error=None,
                after_open=None, existing=False, controller_code=0, requested=("weekly-quota",)):
        label = "log-" + str(self.sequence)
        self.sequence += 1
        output = self.world.root / label / "matrix"
        path = output / (requested[0] + ".log")
        handles, actions, controllers, admission = [], [], [], []
        real_open, previous_trace, main = Path.open, sys.gettrace(), MATRIX.main
        owner = self

        class Output:
            def __init__(self, raw):
                self.raw, self.descriptor = raw, raw.fileno()

            def __getattr__(self, name):
                return getattr(self.raw, name)

            def __enter__(self):
                return self

            def __exit__(self, *_args):
                self.close()

            def close(self):
                actions.append("close")
                self.raw.close()
                if close_error is not None:
                    raise close_error

        def opening(candidate, mode="r", *args, **kwargs):
            if candidate != path or mode != "xb":
                return real_open(candidate, mode, *args, **kwargs)
            actions.append("open")
            if open_error is not None:
                raise open_error
            if existing:
                with real_open(candidate, "xb") as retained:
                    retained.write(b"keep original controller log\n")
            handle = Output(real_open(candidate, mode, *args, **kwargs))
            handles.append(handle)
            if after_open is not None:
                caller = sys._getframe(1)

                def boundary(frame, event, _argument):
                    if frame is caller and event == "line":
                        sys.settrace(previous_trace)
                        frame.f_trace = None
                        raise after_open
                    return boundary

                caller.f_trace = boundary
                sys.settrace(boundary)
            return handle

        def namespaces(**keywords):
            if "run" in keywords and "STDOUT" in keywords:
                controller = keywords["run"]

                def run(command, **options):
                    controllers.append((command, options))
                    owner.assertEqual(len(handles), 1)
                    owner.assertIs(options["stdout"], handles[0])
                    owner.assertIs(options["stderr"], subprocess.STDOUT)
                    completed = controller(command, **options)
                    if primary is not None:
                        raise primary
                    return completed

                keywords["run"] = run
            if "load" in keywords and "require_aterm_name" in keywords:
                load = keywords["load"]

                def observe(path):
                    admission.append("native")
                    return load(path)

                keywords["load"] = observe
            return SimpleNamespace(**keywords)

        def capture(entry=False):
            caught = result = None
            with mock.patch.object(MATRIX, "main", new=(lambda: original_entry(main)) if entry else main):
                try:
                    result = self.world.run_matrix(label, {requested[0]: (controller_code, status_fixture.result())},
                                                   requested=requested)
                except BaseException as error:
                    caught = error
            manifest = output / "qualification.json"
            return SimpleNamespace(error=caught, result=result,
                                   manifest=json.loads(manifest.read_text()) if manifest.exists() else None)

        guards = []
        with ExitStack() as owned:
            targets = [(subprocess, name) for name in ("Popen", "run", "call", "check_call", "check_output")]
            targets += [(os, name) for name in ("kill", "killpg", "fork", "system") if hasattr(os, name)]
            targets += [(socket, "socket"), (threading, "Thread")]
            for module, name in targets:
                guards.append(owned.enter_context(mock.patch.object(module, name,
                    side_effect=AssertionError("forbidden " + name))))
            owned.enter_context(mock.patch.object(Path, "open", new=opening))
            owned.enter_context(mock.patch.object(status_fixture, "SimpleNamespace", new=namespaces))
            try:
                yield SimpleNamespace(capture=capture, path=path, output=output, handles=handles,
                                      actions=actions, controllers=controllers, admission=admission)
            finally:
                sys.settrace(previous_trace)
                # Observe before compensating. A test failure still retires only
                # the regular files this fixture owns, then checks call counters.
                before_fallback = [(handle.raw.closed, self.is_closed(handle.descriptor)) for handle in handles]
                for handle in handles:
                    handle.raw.close()
                self.assertEqual(before_fallback, [(True, True)] * len(handles))
                self.assertEqual(actions.count("close"), len(handles), "one close attempt per returned handle")
                self.assertLessEqual(actions.count("open"), 1)
                self.assertLessEqual(len(controllers), 1)
                for guard in guards:
                    guard.assert_not_called()

    @staticmethod
    def is_closed(descriptor):
        try:
            os.fstat(descriptor)
        except OSError as error:
            return error.errno == errno.EBADF
        return False

    def assert_failed_report(self, run):
        self.assertIs(run.manifest["passed"], False)
        self.assertEqual(run.manifest["cases"], [])

    def test_controller_cancellation_identity_survives_secondary_log_close_failures(self):
        for primary in (KeyboardInterrupt("controller"), SystemExit(0), SystemExit(47)):
            for secondary in (OSError("close"), StopIteration("close"), SystemExit(0)):
                with self.subTest(primary=type(primary).__name__, close=type(secondary).__name__), \
                        self.fixture(primary=primary, close_error=secondary) as fixture:
                    run = fixture.capture()
                    self.assertIs(run.error, primary)
                    self.assert_failed_report(run)
                    self.assertEqual(len(fixture.controllers), 1)

    def test_ordinary_controller_failure_remains_the_reported_primary(self):
        for primary in (OSError("controller error"), StopIteration("controller error")):
            for secondary in (OSError("close error"), SystemExit(0)):
                with self.subTest(primary=type(primary).__name__, close=type(secondary).__name__), \
                        self.fixture(primary=primary, close_error=secondary) as fixture:
                    run = fixture.capture()
                    self.assertIsNone(run.error)
                    self.assertEqual(run.result.status, 1)
                    self.assert_failed_report(run)
                    self.assertEqual(run.manifest["error"], type(primary).__name__ + ": " + str(primary))

    def test_close_only_failures_refuse_success_and_keep_native_control_identity(self):
        for failure in (OSError("close"), StopIteration("close"), KeyboardInterrupt("close"),
                        SystemExit(47), SystemExit(0)):
            with self.subTest(failure=type(failure).__name__, code=getattr(failure, "code", None)), \
                    self.fixture(close_error=failure) as fixture:
                run = fixture.capture()
                if isinstance(failure, (KeyboardInterrupt, SystemExit)) and not (
                        isinstance(failure, SystemExit) and failure.code == 0):
                    self.assertIs(run.error, failure)
                else:
                    self.assertIsNone(run.error)
                    self.assertEqual(run.result.status, 1)
                    expected = "RuntimeError: output close incomplete: requested a successful exit" if (
                        isinstance(failure, SystemExit)) else type(failure).__name__ + ": " + str(failure)
                    self.assertEqual(run.manifest["error"], expected)
                self.assert_failed_report(run)

    def test_original_entry_refuses_body_zero_exits_but_preserves_other_cancellations(self):
        for failure in (SystemExit(), SystemExit(0), SystemExit(False), SystemExit(256),
                        SystemExit(-256), SystemExit(47), SystemExit("0"), KeyboardInterrupt("controller")):
            for secondary in (None, OSError("close")):
                with self.subTest(code=getattr(failure, "code", None), kind=type(failure).__name__,
                                  close=type(secondary).__name__), \
                        self.fixture(primary=failure, close_error=secondary) as fixture:
                    run = fixture.capture(entry=True)
                    if isinstance(failure, SystemExit) and (failure.code is None or failure.code in (0, 256, -256)):
                        self.assertIsInstance(run.error, SystemExit)
                        self.assertEqual(run.error.code, 1)
                    else:
                        self.assertIs(run.error, failure)
                    self.assert_failed_report(run)

    def test_original_entry_refuses_close_zero_even_when_controller_returned_nonzero(self):
        for code in (0, 7):
            with self.subTest(controller_code=code), self.fixture(close_error=SystemExit(0), controller_code=code) as fixture:
                run = fixture.capture(entry=True)
                self.assertIsInstance(run.error, SystemExit)
                self.assertEqual(run.error.code, 1)
                self.assert_failed_report(run)

    def test_success_keeps_the_original_log_command_status_and_result_byte_digest(self):
        for entry in (False, True):
            with self.subTest(entry=entry), self.fixture() as fixture:
                run = fixture.capture(entry=entry)
                if entry:
                    self.assertIsInstance(run.error, SystemExit)
                    self.assertEqual(run.error.code, 0)
                else:
                    self.assertIsNone(run.error)
                    self.assertEqual(run.result.status, 0)
                self.assertIs(run.manifest["passed"], True)
                self.assertEqual(run.manifest["cases"][0]["controllerExit"], 0)
                result_bytes = (fixture.output / "weekly-quota/result.json").read_bytes()
                self.assertEqual(run.manifest["cases"][0]["resultSha256"], hashlib.sha256(result_bytes).hexdigest())
                self.assertEqual(fixture.path.read_bytes(), b"intercepted inert controller; no process was started\n")
                self.assertEqual([path.name for path in fixture.output.glob("*.log")], ["weekly-quota.log"])
                command, options = fixture.controllers[0]
                self.assertEqual(command[0], sys.executable)
                self.assertNotIn("--under-guardian", command)
                self.assertEqual(set(options), {"stdout", "stderr"})

    def test_exclusive_collision_preserves_old_bytes_and_never_calls_controller(self):
        with self.fixture(existing=True) as fixture:
            run = fixture.capture()
            self.assertIsNone(run.error)
            self.assertEqual(run.result.status, 1)
            self.assert_failed_report(run)
            self.assertTrue(run.manifest["error"].startswith("FileExistsError: "))
            self.assertEqual(fixture.path.read_bytes(), b"keep original controller log\n")
            self.assertEqual(fixture.handles, [])
            self.assertEqual(fixture.controllers, [])

    def test_open_refusal_owns_nothing_and_does_not_start_a_controller(self):
        for failure in (OSError("open"), StopIteration("open"), KeyboardInterrupt("open"), SystemExit(0)):
            with self.subTest(failure=type(failure).__name__), self.fixture(open_error=failure) as fixture:
                run = fixture.capture()
                if isinstance(failure, Exception):
                    self.assertIsNone(run.error)
                    self.assertEqual(run.result.status, 1)
                    self.assertEqual(run.manifest["error"], type(failure).__name__ + ": " + str(failure))
                else:
                    self.assertIs(run.error, failure)
                self.assert_failed_report(run)
                self.assertEqual(fixture.handles, [])
                self.assertEqual(fixture.controllers, [])
                self.assertFalse(fixture.path.exists())

    def test_visible_returned_open_cancellation_closes_the_owned_log(self):
        primary = KeyboardInterrupt("visible post-open boundary")
        with self.fixture(after_open=primary, close_error=OSError("secondary close")) as fixture:
            run = fixture.capture()
            self.assertIs(run.error, primary)
            self.assert_failed_report(run)
            self.assertEqual(len(fixture.handles), 1)
            self.assertEqual(fixture.controllers, [])

    def test_hostile_primary_and_diagnostic_hooks_cannot_replace_cancellation(self):
        class Primary(KeyboardInterrupt):
            def add_note(self, _note):
                raise SystemExit(0)
            def __setattr__(self, _name, _value):
                raise SystemExit(0)
        class Secondary(OSError):
            def __str__(self):
                raise SystemExit(0)
        primary = Primary("controller")
        with self.fixture(primary=primary, close_error=Secondary()) as fixture:
            run = fixture.capture()
            self.assertIs(run.error, primary)
            self.assert_failed_report(run)

    def test_caught_controller_errors_keep_safe_labels_and_available_cleanup_notes(self):
        class BadString(RuntimeError):
            def __str__(self):
                raise SystemExit(0)
        class NoteFallback(RuntimeError):
            def add_note(self, _note):
                raise SystemExit(0)
        class Unavailable(BadString):
            def add_note(self, _note):
                raise SystemExit(0)
            def __setattr__(self, _name, _value):
                raise SystemExit(0)

        for primary in (RuntimeError("controller"), BadString(), NoteFallback("controller"), Unavailable()):
            for entry in (False, True):
                with self.subTest(primary=type(primary).__name__, entry=entry), \
                        self.fixture(primary=primary, close_error=OSError("close")) as fixture:
                    run = fixture.capture(entry=entry)
                    if entry:
                        self.assertIsInstance(run.error, SystemExit)
                        self.assertEqual(run.error.code, 1)
                    else:
                        self.assertIsNone(run.error)
                        self.assertEqual(run.result.status, 1)
                    self.assert_failed_report(run)
                    message = "<message unavailable>" if isinstance(primary, BadString) else "controller"
                    self.assertEqual(run.manifest["error"], type(primary).__name__ + ": " + message)
                    notes = [*run.manifest.get("errorNotes", ()), *run.manifest.get("cleanupErrors", ())]
                    if not isinstance(primary, Unavailable):
                        self.assertIn("output close controller: OSError: close", notes)

    def test_help_parser_errors_and_duplicate_case_order_keep_their_original_status(self):
        for arguments, code in ((["--help"], 0), ([], 2), (["--case", "not-a-case"], 2)):
            with self.subTest(arguments=arguments), mock.patch.object(sys, "argv", [str(status_fixture.SOURCE), *arguments]), \
                    mock.patch.object(MATRIX.native_host_record, "load", side_effect=AssertionError("admission forbidden")) as load, \
                    redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as caught:
                    original_entry(MATRIX.main)
                self.assertEqual(caught.exception.code, code)
                load.assert_not_called()
        for entry in (False, True):
            with self.subTest(entry=entry), self.fixture(requested=("weekly-quota", "weekly-quota")) as fixture:
                run = fixture.capture(entry=entry)
                self.assertIsInstance(run.error, SystemExit)
                self.assertEqual(run.error.code, 2)
                self.assertEqual(fixture.admission, ["native"])
                self.assertEqual(fixture.controllers, [])
                self.assertEqual(fixture.handles, [])
                self.assertIsNone(run.manifest)

    def test_shared_named_file_owner_closes_all_paths_in_reverse_and_keeps_exact_zero_cause(self):
        root = self.world.root / "named-output-files"
        root.mkdir()
        paths = tuple((name, root / (name + ".log")) for name in ("first", "second", "third"))
        handles, actions = [], []
        real_open = Path.open
        zero = SystemExit(0)

        class Output:
            def __init__(self, path, raw):
                self.path, self.raw, self.descriptor = path, raw, raw.fileno()
            def close(self):
                actions.append(self.path.name)
                self.raw.close()
                if self.path.name == "third.log":
                    raise zero
                raise OSError("another close")

        def opening(path, *args, **kwargs):
            output = Output(path, real_open(path, *args, **kwargs))
            handles.append(output)
            return output

        try:
            with mock.patch.object(Path, "open", new=opening):
                with self.assertRaises(RuntimeError) as caught:
                    with MATRIX.probe.OutputFiles(paths) as outputs:
                        self.assertEqual(outputs, tuple(handles))
                self.assertIs(caught.exception.__cause__, zero)
            self.assertEqual(actions, ["third.log", "second.log", "first.log"])
            self.assertTrue(all(output.raw.closed and self.is_closed(output.descriptor) for output in handles))
        finally:
            for output in handles:
                output.raw.close()
            self.assertEqual(len(handles), 3)
            self.assertEqual(actions, ["third.log", "second.log", "first.log"])


if __name__ == "__main__":
    unittest.main()
