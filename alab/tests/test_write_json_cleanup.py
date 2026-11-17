# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Shared JSON writer ownership with finite actual file and serializer faults."""
from contextlib import contextmanager
import errno
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import capacity_exec as probe
import weekly_reset as weekly


class WriteJsonCleanupTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-write-json-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.sequence = 0

    @contextmanager
    def fixture(self, *, close_error=None, write_error=None, write_phase="serialize",
                actual_write_error=False, open_error=None, cancel_after_open=None, existing=False, writer=None):
        path = self.root / (str(self.sequence) + ".json")
        self.sequence += 1
        if existing:
            path.write_bytes(b"keep original JSON bytes\n")
        real_open, real_dump = Path.open, json.dump
        previous, opened, closes, dump_calls, errors, open_calls = sys.gettrace(), [], [], [], [], []
        serialized = False

        class Output:
            def __init__(self, handle):
                self.handle, self.fd, self.failed = handle, handle.fileno(), False
            def __getattr__(self, name):
                return getattr(self.handle, name)
            def __enter__(self):
                return self
            def __exit__(self, *_args):
                self.close()
            def write(self, text):
                if actual_write_error and not self.failed:
                    self.failed = True
                    self.handle.close()
                    return self.handle.write(text)  # Real ValueError from the closed file.
                result = self.handle.write(text)
                if (write_error is not None and not self.failed
                        and serialized == (write_phase == "newline")):
                    self.failed = True
                    raise write_error
                return result
            def close(self):
                closes.append(self)
                self.handle.close()
                if close_error is not None:
                    raise close_error

        def open_file(opened_path, *args, **kwargs):
            mode = args[0] if args else kwargs.get("mode", "r")
            if opened_path != path or mode != "x":
                return real_open(opened_path, *args, **kwargs)
            open_calls.append((args, kwargs))
            if open_error is not None:
                raise open_error
            output = Output(real_open(opened_path, *args, **kwargs))
            opened.append(output)
            if cancel_after_open is not None:
                caller = sys._getframe(1)
                self.assertIs(caller.f_code, probe.write_json.__code__)
                def boundary(frame, event, _argument):
                    if frame is caller and event == "line":
                        sys.settrace(previous)
                        frame.f_trace = None
                        raise cancel_after_open
                    return boundary
                caller.f_trace = boundary
                sys.settrace(boundary)
            return output

        def dump(*args, **kwargs):
            nonlocal serialized
            dump_calls.append((args, kwargs))
            result = real_dump(*args, **kwargs)
            serialized = True
            return result

        def observe(frame, event, argument):
            if frame.f_code is probe.write_json.__code__ and event == "exception":
                errors.append(argument[1])
            return observe

        def run(value):
            try:
                return None, (probe.write_json if writer is None else writer)(path, value)
            except BaseException as error:
                return error, None

        with mock.patch.object(Path, "open", new=open_file), mock.patch.object(json, "dump", new=dump), \
                mock.patch.object(probe.subprocess, "Popen", side_effect=AssertionError("child forbidden")) as popen, \
                mock.patch.object(probe.subprocess, "run", side_effect=AssertionError("child forbidden")) as process, \
                mock.patch.object(os, "kill", side_effect=AssertionError("raw signal forbidden")) as raw_signal, \
                mock.patch.object(os, "killpg", side_effect=AssertionError("group signal forbidden")) as group_signal:
            try:
                sys.settrace(observe)
                yield SimpleNamespace(path=path, opened=opened, closes=closes, errors=errors,
                    dump_calls=dump_calls, open_calls=open_calls, run=run)
            finally:
                sys.settrace(previous)
                # Fallback never calls the fault wrapper; it owns real handles only.
                for output in opened:
                    output.handle.close()
                self.assertEqual(closes, opened, "each returned file needs exactly one close attempt")
                popen.assert_not_called()
                process.assert_not_called()
                raw_signal.assert_not_called()
                group_signal.assert_not_called()

    def assert_closed_before_fallback(self, fixture):
        self.assertEqual(len(fixture.opened), 1)
        self.assertTrue(fixture.opened[0].handle.closed)
        self.assertEqual(fixture.closes, fixture.opened)
        with self.assertRaises(OSError) as error:
            os.fstat(fixture.opened[0].fd)
        self.assertEqual(error.exception.errno, errno.EBADF)

    def test_actual_serialization_errors_survive_secondary_close_failures(self):
        circular = []
        circular.append(circular)
        for value, expected in (({"bad": object()}, TypeError), (circular, ValueError)):
            for secondary in (OSError("close"), KeyboardInterrupt("close"), SystemExit(0), StopIteration("close")):
                with self.subTest(primary=expected.__name__, secondary=type(secondary).__name__), \
                        self.fixture(close_error=secondary) as fixture:
                    caught, _ = fixture.run(value)
                    originals = [error for error in fixture.errors if isinstance(error, expected)]
                    self.assertTrue(originals, "the actual JSON encoder did not fail")
                    self.assertIs(caught, originals[0])
                    self.assert_closed_before_fallback(fixture)
                    self.assertIn("close", json.dumps(probe.case_error_fields(caught)))

    def test_actual_closed_file_write_error_keeps_its_identity_through_close(self):
        for secondary in (OSError("close"), SystemExit(0), StopIteration("close")):
            with self.subTest(secondary=type(secondary).__name__), \
                    self.fixture(actual_write_error=True, close_error=secondary) as fixture:
                caught, _ = fixture.run({"healthy": True})
                originals = [error for error in fixture.errors if isinstance(error, ValueError)]
                self.assertTrue(originals)
                self.assertIs(caught, originals[0])
                self.assert_closed_before_fallback(fixture)

    def test_write_failures_and_cancellations_keep_exact_identity_in_both_phases(self):
        for phase in ("serialize", "newline"):
            for kind, message in ((RuntimeError, "write"), (KeyboardInterrupt, "write"),
                                  (SystemExit, 0), (SystemExit, 47), (StopIteration, "write")):
                for secondary in (OSError("close"), SystemExit(0)):
                    primary = kind(message)
                    with self.subTest(phase=phase, primary=kind.__name__, secondary=type(secondary).__name__), \
                            self.fixture(write_error=primary, write_phase=phase, close_error=secondary) as fixture:
                        caught, _ = fixture.run({"healthy": True})
                        self.assertIs(caught, primary)
                        self.assert_closed_before_fallback(fixture)

    def test_close_only_native_errors_and_genuine_controls_do_not_become_success(self):
        for primary in (OSError("close"), KeyboardInterrupt("close"), StopIteration("close"),
                        SystemExit(47), SystemExit(0.0), SystemExit(sys.maxsize + 1)):
            with self.subTest(primary=type(primary).__name__), self.fixture(close_error=primary) as fixture:
                caught, _ = fixture.run({"healthy": True})
                self.assertIs(caught, primary)
                self.assert_closed_before_fallback(fixture)

    def test_close_only_success_shaped_exits_are_failures_with_the_exact_cause(self):
        for code in (None, False, 0, 256, -256):
            primary = SystemExit(code)
            with self.subTest(code=code), self.fixture(close_error=primary) as fixture:
                caught, _ = fixture.run({"healthy": True})
                self.assertIsInstance(caught, RuntimeError)
                self.assertIs(caught.__cause__, primary)
                self.assertIn("close incomplete", str(caught))
                self.assert_closed_before_fallback(fixture)

    def test_hostile_close_diagnostics_cannot_replace_the_active_writer_error(self):
        class Primary(RuntimeError):
            def add_note(self, _note):
                raise SystemExit(0)
            def __setattr__(self, _name, _value):
                raise SystemExit(0)
        class Secondary(OSError):
            def __str__(self):
                raise SystemExit(0)
        primary = Primary("write failed")
        with self.fixture(write_error=primary, close_error=Secondary()) as fixture:
            caught, _ = fixture.run({"healthy": True})
            self.assertIs(caught, primary)
            self.assert_closed_before_fallback(fixture)

    def test_open_refusals_and_exclusive_collision_keep_original_outcomes(self):
        for primary in (OSError("open"), KeyboardInterrupt("open"), SystemExit(0), StopIteration("open")):
            with self.subTest(primary=type(primary).__name__), self.fixture(open_error=primary) as fixture:
                caught, _ = fixture.run({"healthy": True})
                self.assertIs(caught, primary)
                self.assertEqual(fixture.opened, [])
                self.assertEqual(fixture.dump_calls, [])
        with self.fixture(existing=True) as fixture:
            caught, _ = fixture.run({"healthy": True})
            self.assertIsInstance(caught, FileExistsError)
            self.assertEqual(fixture.path.read_bytes(), b"keep original JSON bytes\n")
            self.assertEqual(fixture.opened, [])

    def test_visible_returned_open_cancellation_closes_the_handle_and_keeps_identity(self):
        for primary in (KeyboardInterrupt("post-open"), SystemExit(0), StopIteration("post-open")):
            with self.subTest(primary=type(primary).__name__), self.fixture(cancel_after_open=primary) as fixture:
                caught, _ = fixture.run({"healthy": True})
                self.assertIs(caught, primary)
                self.assertEqual(fixture.dump_calls, [])
                self.assert_closed_before_fallback(fixture)

    def test_healthy_general_json_keeps_text_open_call_shape_indent_and_newline(self):
        for value in (None, [], {"unicode": ["caf\u00e9", 2]}, float("nan")):
            with self.subTest(value=value), self.fixture() as fixture:
                caught, result = fixture.run(value)
                self.assertIsNone(caught)
                self.assertIsNone(result)
                self.assert_closed_before_fallback(fixture)
                self.assertEqual(fixture.path.read_bytes(), (json.dumps(value, indent=2) + "\n").encode())
                self.assertEqual(fixture.open_calls, [(("x",), {})])
                self.assertEqual(len(fixture.dump_calls), 1)
                args, kwargs = fixture.dump_calls[0]
                self.assertEqual(len(args), 2)
                self.assertIs(args[0], value)
                self.assertIs(args[1], fixture.opened[0])
                self.assertEqual(kwargs, {"indent": 2})

    def test_both_public_entries_share_the_writer_and_keep_its_outcomes(self):
        for writer in (probe.write_json, weekly.write_json):
            for scenario in ("active", "close-only", "healthy"):
                primary, secondary = KeyboardInterrupt("write"), SystemExit(0)
                with self.subTest(writer=writer.__module__, scenario=scenario), self.fixture(writer=writer,
                        write_error=primary if scenario == "active" else None,
                        close_error=secondary if scenario != "healthy" else None) as fixture:
                    caught, result = fixture.run({"healthy": True})
                    if scenario == "active":
                        self.assertIs(caught, primary)
                    elif scenario == "close-only":
                        self.assertIsInstance(caught, RuntimeError)
                        self.assertIs(caught.__cause__, secondary)
                    else:
                        self.assertIsNone(caught)
                        self.assertIsNone(result)
                        self.assertEqual(fixture.path.read_text(), json.dumps({"healthy": True}, indent=2) + "\n")
                    self.assert_closed_before_fallback(fixture)
        self.assertIs(weekly.write_json, probe.write_json)


if __name__ == "__main__":
    unittest.main()
