# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The runtime controller's public digest reader keeps the shared snapshot bound."""
import hashlib
import os
from pathlib import Path
import runpy
import sys
import tempfile
import unittest
from unittest import mock


RUNTIME = runpy.run_path(str(Path(__file__).resolve().parents[1] / "check_runtime.py"))


class RuntimeFileHashTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-runtime-hash-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.path = self.root / "owned-file"
        self.path.write_bytes(b"a" * 100)

    def test_stable_regular_file_keeps_its_digest(self):
        self.assertEqual(RUNTIME["sha256"](self.path), hashlib.sha256(b"a" * 100).hexdigest())

    def test_appending_after_every_read_stops_at_initial_size_plus_one(self):
        original_read = os.read
        calls, received, opened = 0, 0, []

        def append_at_read(fd, amount):
            nonlocal calls, received
            calls += 1
            self.assertLess(calls, 4, "runtime hash followed continuous growth")
            opened.append(fd)
            data = original_read(fd, amount)
            received += len(data)
            with self.path.open("ab") as stream:
                stream.write(b"x")
            return data

        with mock.patch.object(os, "read", side_effect=append_at_read), \
                self.assertRaisesRegex(ValueError, "changed while hashing"):
            RUNTIME["sha256"](self.path)
        self.assertEqual((calls, received), (2, 101))
        with self.assertRaises(OSError):
            os.fstat(opened[0])

    def test_same_size_mutation_after_read_is_refused(self):
        original_read = os.read
        changed = False

        def change_at_read(fd, amount):
            nonlocal changed
            data = original_read(fd, amount)
            if data and not changed:
                self.path.write_bytes(b"b" * 100)
                changed = True
            return data

        with mock.patch.object(os, "read", side_effect=change_at_read), \
                self.assertRaisesRegex(ValueError, "changed while hashing"):
            RUNTIME["sha256"](self.path)
        self.assertTrue(changed)

    def test_fifo_is_still_refused_without_a_writer(self):
        self.path.unlink()
        os.mkfifo(self.path)
        with self.assertRaisesRegex(ValueError, "not a regular file"):
            RUNTIME["sha256"](self.path)

    def test_cancellation_immediately_after_open_closes_the_published_descriptor(self):
        original_open = os.open
        previous_trace = sys.gettrace()
        opened = []
        primary = KeyboardInterrupt("controlled immediate post-open cancellation")

        def open_then_cancel(*args, **kwargs):
            descriptor = original_open(*args, **kwargs)
            opened.append(descriptor)
            caller = sys._getframe(1)
            self.assertIs(caller.f_code, RUNTIME["FILE_HASH"]["_file_snapshot"].__code__)

            def cancel_at_next_line(frame, event, _argument):
                if frame is caller and event == "line":
                    sys.settrace(previous_trace)
                    frame.f_trace = None
                    raise primary
                return cancel_at_next_line

            caller.f_trace = cancel_at_next_line
            sys.settrace(cancel_at_next_line)
            return descriptor

        try:
            with mock.patch.object(os, "open", new=open_then_cancel), \
                    self.assertRaises(KeyboardInterrupt) as caught:
                RUNTIME["sha256"](self.path)
            self.assertIs(caught.exception, primary)
            self.assertEqual(len(opened), 1)
            with self.assertRaises(OSError):
                os.fstat(opened[0])
        finally:
            sys.settrace(previous_trace)
            for descriptor in opened:
                try:
                    os.fstat(descriptor)
                except OSError:
                    pass
                else:
                    os.close(descriptor)

    def test_open_failure_keeps_the_exact_error_without_closing_an_unowned_descriptor(self):
        primary = OSError("controlled open failure")
        with mock.patch.object(os, "open", side_effect=primary), \
                mock.patch.object(os, "close") as close, \
                self.assertRaises(OSError) as caught:
            RUNTIME["sha256"](self.path)
        self.assertIs(caught.exception, primary)
        close.assert_not_called()

    def test_secondary_close_failure_cannot_replace_read_or_stat_failure(self):
        for operation in ("read", "fstat"):
            for primary in (OSError("controlled primary IO failure"), KeyboardInterrupt("controlled primary cancellation")):
                for secondary in (OSError("controlled secondary close failure"), SystemExit(0)):
                    with self.subTest(operation=operation, primary=type(primary).__name__, secondary=type(secondary).__name__):
                        original_close = os.close
                        closed = []

                        def close_then_fail(fd):
                            original_close(fd)
                            closed.append(fd)
                            raise secondary

                        with mock.patch.object(os, operation, side_effect=primary), \
                                mock.patch.object(os, "close", new=close_then_fail), \
                                self.assertRaises(type(primary)) as caught:
                            RUNTIME["sha256"](self.path)
                        self.assertIs(caught.exception, primary)
                        self.assertEqual(len(closed), 1)
                        with self.assertRaises(OSError):
                            os.fstat(closed[0])

    def test_close_failure_after_a_successful_read_cannot_return_a_digest(self):
        for error in (OSError("controlled final close failure"), KeyboardInterrupt(), SystemExit(47),
                      SystemExit(0.0), SystemExit(sys.maxsize + 1)):
            with self.subTest(error=type(error).__name__):
                original_close = os.close
                closed = []

                def close_then_fail(fd):
                    original_close(fd)
                    closed.append(fd)
                    raise error

                with mock.patch.object(os, "close", new=close_then_fail), \
                        self.assertRaises(type(error)) as caught:
                    RUNTIME["sha256"](self.path)
                self.assertIs(caught.exception, error)
                self.assertEqual(len(closed), 1)
                with self.assertRaises(OSError):
                    os.fstat(closed[0])

    def test_success_shaped_close_exit_is_a_definite_failure_with_its_exact_cause(self):
        for code in (None, False, 0, 256, -256):
            with self.subTest(code=code):
                original_close = os.close
                closed = []
                primary = SystemExit(code)

                def close_then_cancel(fd):
                    original_close(fd)
                    closed.append(fd)
                    raise primary

                with mock.patch.object(os, "close", new=close_then_cancel), \
                        self.assertRaisesRegex(RuntimeError, "close incomplete") as caught:
                    RUNTIME["sha256"](self.path)
                self.assertIs(caught.exception.__cause__, primary)
                self.assertEqual(len(closed), 1)
                with self.assertRaises(OSError):
                    os.fstat(closed[0])

    def test_foreign_close_exit_and_payload_cannot_invoke_classification_callbacks(self):
        callbacks = []

        class ForeignExit(SystemExit):
            def __getattribute__(self, name):
                callbacks.append(name)
                raise SystemExit(0)

        class Payload:
            def __getattribute__(self, name):
                callbacks.append(name)
                raise SystemExit(0)

        for primary in (ForeignExit(47), SystemExit(Payload())):
            with self.subTest(kind=type(primary).__name__):
                original_close = os.close
                closed = []

                def close_then_fail(fd):
                    original_close(fd)
                    closed.append(fd)
                    raise primary

                with mock.patch.object(os, "close", new=close_then_fail):
                    try:
                        RUNTIME["sha256"](self.path)
                    except BaseException as observed:
                        if type(primary) is SystemExit:
                            self.assertIs(observed, primary)
                        else:
                            self.assertIs(type(observed), RuntimeError)
                            self.assertEqual(str(observed), "unsupported SystemExit type")
                    else:
                        self.fail("failed close returned a digest")
                self.assertEqual(callbacks, [])
                self.assertEqual(len(closed), 1)
                with self.assertRaises(OSError):
                    os.fstat(closed[0])

    def test_close_diagnostics_cannot_replace_primary_when_formatting_or_notes_raise(self):
        class Primary(OSError):
            def add_note(self, _note):
                raise SystemExit(0)

        class CloseFailure(OSError):
            def __str__(self):
                raise SystemExit(0)

        original_close = os.close
        primary = Primary("controlled primary failure")
        closed = []

        def close_then_fail(fd):
            original_close(fd)
            closed.append(fd)
            raise CloseFailure()

        with mock.patch.object(os, "read", side_effect=primary), \
                mock.patch.object(os, "close", new=close_then_fail), \
                self.assertRaises(Primary) as caught:
            RUNTIME["sha256"](self.path)
        self.assertIs(caught.exception, primary)
        self.assertEqual(len(closed), 1)
        with self.assertRaises(OSError):
            os.fstat(closed[0])
