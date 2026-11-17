# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exact byte snapshots and descriptor ownership using finite owned files."""
from contextlib import contextmanager, ExitStack
import errno
import os
from pathlib import Path
import runpy
import socket
import stat
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest import mock


FILE = runpy.run_path(str(Path(__file__).resolve().parents[1] / "file_hash.py"))


class FileBytesSnapshotTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-file-bytes-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.path = self.root / "owned-file"
        self.path.write_bytes(b"a" * 100)

    @contextmanager
    def observed(self, *, open_error=None, primary=None, fail_at=None,
                 close_error=None, after_read=None, after_open=None):
        original_open, original_read = os.open, os.read
        original_stat, original_close = os.fstat, os.close
        previous_trace = sys.gettrace()
        opened, closed, flags, reads, stats = [], [], [], [], []

        def opening(path, options, *args, **keywords):
            self.assertEqual(Path(path), self.path)
            flags.append(options)
            # Check before the real call: a missing nonblocking flag must fail
            # this finite control instead of hanging while opening its FIFO.
            self.assertTrue(options & os.O_NONBLOCK)
            self.assertTrue(options & os.O_NOFOLLOW)
            self.assertEqual(options & os.O_ACCMODE, os.O_RDONLY)
            if open_error is not None:
                raise open_error
            descriptor = original_open(path, options, *args, **keywords)
            opened.append(descriptor)
            if after_open is not None:
                caller = sys._getframe(1)
                self.assertIs(caller.f_code, FILE["_file_snapshot"].__code__)

                def cancel(frame, event, _argument):
                    if frame is caller and event == "line":
                        sys.settrace(previous_trace)
                        frame.f_trace = None
                        raise after_open
                    return cancel

                caller.f_trace = cancel
                sys.settrace(cancel)
            return descriptor

        def reading(descriptor, amount):
            self.assertIn(descriptor, opened)
            self.assertLess(len(reads), 4, "snapshot followed continuing file growth")
            if fail_at == "read":
                raise primary
            data = original_read(descriptor, amount)
            reads.append((amount, data))
            if after_read is not None:
                after_read(data)
            return data

        def metadata(descriptor):
            self.assertIn(descriptor, opened)
            stats.append(descriptor)
            if fail_at == "initial stat" and len(stats) == 1 or fail_at == "final stat" and len(stats) == 2:
                raise primary
            return original_stat(descriptor)

        def closing(descriptor):
            self.assertIn(descriptor, opened, "never close an unowned descriptor")
            closed.append(descriptor)
            original_close(descriptor)
            if close_error is not None:
                raise close_error

        guards = []
        with ExitStack() as owned:
            targets = [(subprocess, name) for name in ("Popen", "run", "call", "check_call", "check_output")]
            targets += [(os, name) for name in ("fork", "kill", "killpg", "system") if hasattr(os, name)]
            targets += [(socket, "socket"), (threading, "Thread")]
            for module, name in targets:
                guards.append(owned.enter_context(mock.patch.object(module, name,
                    side_effect=AssertionError("forbidden " + name))))
            for name, replacement in (("open", opening), ("read", reading), ("fstat", metadata), ("close", closing)):
                owned.enter_context(mock.patch.object(os, name, new=replacement))
            try:
                yield SimpleNamespace(opened=opened, closed=closed, flags=flags, reads=reads, stats=stats)
            finally:
                sys.settrace(previous_trace)
                observed = []
                for descriptor in opened:
                    try:
                        original_stat(descriptor)
                    except OSError as error:
                        observed.append(error.errno == errno.EBADF)
                    else:
                        observed.append(False)
                # Retire only our own remaining descriptor after recording the
                # evidence; fallback cannot make an ownership regression pass.
                for descriptor, retired in zip(opened, observed):
                    if not retired:
                        original_close(descriptor)
                self.assertEqual(observed, [True] * len(opened))
                self.assertEqual(closed, opened, "one owned close attempt per returned descriptor")
                self.assertEqual(len(flags), 1)
                for guard in guards:
                    guard.assert_not_called()

    def test_exact_empty_binary_and_large_bytes_keep_opened_metadata(self):
        for content in (b"", b"\x00\xff\r\nexact bytes\x80", b"z" * (2 * 1024 * 1024 + 17)):
            with self.subTest(size=len(content)):
                self.path.write_bytes(content)
                expected = self.path.stat()
                with self.observed() as observed:
                    metadata, actual = FILE["file_bytes_snapshot"](self.path)
                    self.assertEqual(actual, content)
                    self.assertEqual((metadata.st_dev, metadata.st_ino, metadata.st_size),
                                     (expected.st_dev, expected.st_ino, len(content)))
                    self.assertTrue(stat.S_ISREG(metadata.st_mode))
                    self.assertLessEqual(sum(len(data) for _amount, data in observed.reads), len(content) + 1)

    def test_directory_and_fifo_are_refused_without_any_read(self):
        for kind in ("directory", "FIFO"):
            with self.subTest(kind=kind):
                self.path.unlink()
                if kind == "directory":
                    self.path.mkdir()
                else:
                    os.mkfifo(self.path)
                try:
                    with self.observed() as observed, self.assertRaisesRegex(ValueError, "not a regular file"):
                        FILE["file_bytes_snapshot"](self.path)
                    self.assertEqual(observed.reads, [])
                finally:
                    if kind == "directory":
                        self.path.rmdir()
                    else:
                        self.path.unlink()
                    self.path.write_bytes(b"a" * 100)

    def test_final_symlink_is_refused_without_reading_its_target(self):
        target = self.root / "owned-target"
        self.path.rename(target)
        self.path.symlink_to(target)
        with self.observed() as observed, self.assertRaises(OSError):
            FILE["file_bytes_snapshot"](self.path)
        self.assertEqual(observed.opened, [])
        self.assertEqual(observed.reads, [])
        self.assertEqual(target.read_bytes(), b"a" * 100)

    def test_continuing_append_is_refused_after_at_most_initial_size_plus_one_bytes(self):
        def append(_data):
            with self.path.open("ab") as stream:
                stream.write(b"x")

        with self.observed(after_read=append) as observed, self.assertRaisesRegex(ValueError, "changed"):
            FILE["file_bytes_snapshot"](self.path)
        self.assertEqual(sum(len(data) for _amount, data in observed.reads), 101)
        self.assertLessEqual(len(observed.reads), 3)

    def test_real_truncation_after_read_is_refused(self):
        changed = []

        def truncate(data):
            if data and not changed:
                self.path.write_bytes(b"a")
                changed.append(True)

        with self.observed(after_read=truncate), self.assertRaisesRegex(ValueError, "changed"):
            FILE["file_bytes_snapshot"](self.path)
        self.assertEqual(changed, [True])
        self.assertEqual(self.path.read_bytes(), b"a")

    def test_real_same_size_rewrite_after_read_is_refused(self):
        changed, before = [], self.path.stat()

        def rewrite(data):
            if data and not changed:
                self.path.write_bytes(b"b" * 100)
                os.utime(self.path, ns=(before.st_atime_ns, before.st_mtime_ns + 1_000_000))
                changed.append(True)

        with self.observed(after_read=rewrite), self.assertRaisesRegex(ValueError, "changed"):
            FILE["file_bytes_snapshot"](self.path)
        self.assertEqual(changed, [True])
        self.assertEqual(self.path.read_bytes(), b"b" * 100)

    def test_requested_error_type_controls_validation_refusal(self):
        class Refusal(RuntimeError):
            pass

        self.path.unlink()
        os.mkfifo(self.path)
        with self.observed(), self.assertRaisesRegex(Refusal, "not a regular file"):
            FILE["file_bytes_snapshot"](self.path, error_type=Refusal)

    def test_active_read_stat_and_control_failures_survive_actual_close_faults(self):
        for phase in ("read", "initial stat", "final stat"):
            for primary in (OSError("snapshot operation"), KeyboardInterrupt("snapshot operation"), SystemExit(0)):
                for secondary in (OSError("descriptor close"), SystemExit(0)):
                    with self.subTest(phase=phase, primary=type(primary).__name__, secondary=type(secondary).__name__), \
                            self.observed(primary=primary, fail_at=phase, close_error=secondary), \
                            self.assertRaises(type(primary)) as caught:
                        FILE["file_bytes_snapshot"](self.path)
                    self.assertIs(caught.exception, primary)

    def test_hostile_close_formatting_and_note_hooks_cannot_replace_primary(self):
        class Primary(KeyboardInterrupt):
            def add_note(self, _note):
                raise SystemExit(0)

        class CloseFailure(OSError):
            def __str__(self):
                raise SystemExit(0)

        for phase in ("read", "initial stat"):
            primary = Primary("snapshot operation")
            with self.subTest(phase=phase), \
                    self.observed(primary=primary, fail_at=phase, close_error=CloseFailure()), \
                    self.assertRaises(Primary) as caught:
                FILE["file_bytes_snapshot"](self.path)
            self.assertIs(caught.exception, primary)

    def test_close_only_ordinary_and_genuine_controls_keep_exact_identity(self):
        for failure in (OSError("descriptor close"), KeyboardInterrupt("descriptor close"), SystemExit(47),
                        SystemExit(0.0), SystemExit("0"), SystemExit(sys.maxsize + 1)):
            with self.subTest(failure=type(failure).__name__, code=getattr(failure, "code", None)), \
                    self.observed(close_error=failure), self.assertRaises(type(failure)) as caught:
                FILE["file_bytes_snapshot"](self.path)
            self.assertIs(caught.exception, failure)

    def test_success_shaped_close_controls_refuse_a_snapshot_with_the_exact_cause(self):
        for code in (None, False, 0, 256, -256):
            primary = SystemExit(code)
            with self.subTest(code=code), self.observed(close_error=primary), \
                    self.assertRaisesRegex(RuntimeError, "close incomplete") as caught:
                FILE["file_bytes_snapshot"](self.path)
            self.assertIs(caught.exception.__cause__, primary)

    def test_failed_open_preserves_native_failure_without_closing_unowned_descriptors(self):
        for failure in (OSError("descriptor open"), KeyboardInterrupt("descriptor open"), SystemExit(0)):
            with self.subTest(failure=type(failure).__name__), self.observed(open_error=failure) as observed, \
                    self.assertRaises(type(failure)) as caught:
                FILE["file_bytes_snapshot"](self.path)
            self.assertIs(caught.exception, failure)
            self.assertEqual(observed.opened, [])
            self.assertEqual(observed.closed, [])

    def test_visible_post_open_cancellation_closes_the_actual_owner_descriptor(self):
        for primary in (KeyboardInterrupt("visible post-open cancellation"), SystemExit(0)):
            with self.subTest(primary=type(primary).__name__), \
                    self.observed(after_open=primary, close_error=OSError("secondary close")) as observed, \
                    self.assertRaises(type(primary)) as caught:
                FILE["file_bytes_snapshot"](self.path)
            self.assertIs(caught.exception, primary)
            self.assertEqual(len(observed.opened), 1)
            self.assertEqual(observed.reads, [])

    def test_missing_required_capabilities_refuse_before_io_or_cache_publication(self):
        class Refusal(RuntimeError):
            pass

        class Cache(dict):
            def __init__(self):
                super().__init__({("retained",): "existing digest"})
                self.queries, self.writes = [], []

            def get(self, key):
                self.queries.append(key)
                return super().get(key)

            def __setitem__(self, key, value):
                self.writes.append((key, value))
                super().__setitem__(key, value)

        core = FILE["_file_snapshot"].__globals__
        for missing in ("O_NOFOLLOW", "O_NONBLOCK"):
            for operation in ("bytes default", "bytes custom", "hash cached"):
                with self.subTest(missing=missing, operation=operation):
                    original = AttributeError("controlled missing " + missing)

                    class Capabilities(SimpleNamespace):
                        def __getattribute__(self, name):
                            if name == missing:
                                raise original
                            return super().__getattribute__(name)

                    namespace = Capabilities(**vars(os))
                    delattr(namespace, missing)
                    calls = []
                    for name in ("open", "read", "fstat", "close", "fork", "kill", "killpg", "system"):
                        if hasattr(os, name):
                            observer = mock.Mock(side_effect=AssertionError("forbidden " + name))
                            setattr(namespace, name, observer)
                            calls.append(observer)
                    cache = Cache()
                    error_type = Refusal if operation == "bytes custom" else ValueError
                    with mock.patch.dict(core, {"os": namespace}), \
                            self.assertRaisesRegex(error_type, "require O_NOFOLLOW and O_NONBLOCK") as caught:
                        if operation == "hash cached":
                            FILE["file_sha256_snapshot"](self.path, cache=cache)
                        else:
                            FILE["file_bytes_snapshot"](self.path, error_type=error_type)
                    self.assertIs(caught.exception.__cause__, original)
                    self.assertEqual(cache, {("retained",): "existing digest"})
                    self.assertEqual((cache.queries, cache.writes), ([], []))
                    for observer in calls:
                        observer.assert_not_called()


if __name__ == "__main__":
    unittest.main()
