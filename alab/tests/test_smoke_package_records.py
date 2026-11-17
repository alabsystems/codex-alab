# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Package statement readers must read their own bounded regular metadata."""
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

SMOKE = Path(__file__).resolve().parents[1] / "smoke"
sys.path.insert(0, str(SMOKE))
import capacity_exec
import weekly_reset

READERS = (capacity_exec.package_records, weekly_reset.package_records)
LIMIT = 1024 * 1024


class PackageRecordTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()

    def package(self, name):
        package = self.root / name
        (package / "bin").mkdir(parents=True)
        (package / "bin/codex").write_bytes(b"inert fixture; never executed")
        build = {"version": "0.0.0-stand-in", "target": "stand-in", "source": "a" * 40}
        manifest = {"version": build["version"], "target": build["target"], "entrypoint": "bin/codex"}
        (package / "alab-build.json").write_text(json.dumps(build))
        (package / "codex-package.json").write_text(json.dumps(manifest))
        return package, build, manifest

    def at_open(self, path, change):
        """Mutate the real file at either the old text-open or descriptor-open boundary."""
        original_io, original_os = io.open, os.open
        changed = False

        def mutate(opened):
            nonlocal changed
            if not changed and not isinstance(opened, int) and Path(opened) == path:
                changed = True
                change()

        def text_open(opened, *args, **kwargs):
            mutate(opened)
            return original_io(opened, *args, **kwargs)

        def descriptor_open(opened, *args, **kwargs):
            mutate(opened)
            return original_os(opened, *args, **kwargs)

        return mock.patch.multiple(io, open=text_open), mock.patch.multiple(os, open=descriptor_open)

    def test_regular_records_keep_their_current_schema_and_entrypoint(self):
        for index, reader in enumerate(READERS):
            with self.subTest(reader=reader.__module__):
                package, build, manifest = self.package("valid-" + str(index))
                self.assertEqual(reader(package), (build, manifest, package / "bin/codex"))

    def test_both_probes_use_the_same_package_reader(self):
        self.assertIs(weekly_reset.package_records, capacity_exec.package_records)

    def test_record_swapped_to_foreign_symlink_at_open_is_refused(self):
        for index, reader in enumerate(READERS):
            with self.subTest(reader=reader.__module__):
                package, build, _ = self.package("swapped-" + str(index))
                record = package / "alab-build.json"
                foreign = self.root / ("foreign-" + str(index) + ".json")
                foreign.write_text(json.dumps({**build, "source": "b" * 40}))

                def swap():
                    record.unlink()
                    record.symlink_to(foreign)

                text_open, descriptor_open = self.at_open(record, swap)
                with text_open, descriptor_open, self.assertRaises((OSError, RuntimeError, ValueError)):
                    reader(package)
                self.assertTrue(record.is_symlink())

    def test_record_grown_past_limit_at_open_is_refused(self):
        for index, reader in enumerate(READERS):
            with self.subTest(reader=reader.__module__):
                package, _, _ = self.package("grown-" + str(index))
                record = package / "alab-build.json"
                initial = record.read_bytes()
                text_open, descriptor_open = self.at_open(
                    record, lambda: record.write_bytes(initial + b" " * LIMIT))
                with text_open, descriptor_open, self.assertRaises((OSError, RuntimeError, ValueError)):
                    reader(package)
                self.assertGreater(record.stat().st_size, LIMIT)

    def test_regular_record_at_limit_is_still_accepted(self):
        for index, reader in enumerate(READERS):
            with self.subTest(reader=reader.__module__):
                package, build, manifest = self.package("boundary-" + str(index))
                record = package / "alab-build.json"
                data = record.read_bytes()
                record.write_bytes(data + b" " * (LIMIT - len(data)))
                self.assertEqual(reader(package), (build, manifest, package / "bin/codex"))

    def test_growth_after_open_never_reads_more_than_the_limit_plus_one(self):
        package, _, _ = self.package("grows-during-read")
        record = package / "alab-build.json"
        initial, identity = record.read_bytes(), record.stat()
        original_stat, original_read = os.fstat, os.read
        grown, observed = False, 0

        def inspect(descriptor):
            nonlocal grown
            metadata = original_stat(descriptor)
            if not grown and (metadata.st_dev, metadata.st_ino) == (identity.st_dev, identity.st_ino):
                grown = True
                record.write_bytes(initial + b" " * (2 * LIMIT))
            return metadata

        def read(descriptor, size):
            nonlocal observed
            chunk = original_read(descriptor, size)
            observed += len(chunk)
            return chunk

        with mock.patch.object(os, "fstat", side_effect=inspect), \
                mock.patch.object(os, "read", side_effect=read), \
                self.assertRaisesRegex(RuntimeError, "oversized"):
            capacity_exec.package_records(package)
        self.assertTrue(grown)
        self.assertEqual(observed, LIMIT + 1)

    def test_changed_metadata_and_read_errors_close_the_opened_descriptor(self):
        for failure in ("changed", "read-error"):
            with self.subTest(failure=failure):
                package, _, _ = self.package(failure)
                record = package / "alab-build.json"
                initial = record.read_bytes()
                original_open, original_read = os.open, os.read
                opened = []

                def open_record(path, *args, **kwargs):
                    descriptor = original_open(path, *args, **kwargs)
                    opened.append(descriptor)
                    return descriptor

                def read_record(descriptor, size):
                    if failure == "read-error":
                        raise OSError("fixture read failed")
                    chunk = original_read(descriptor, size)
                    if chunk:
                        record.write_bytes(initial.replace(b'"a' + b"a" * 39, b'"b' + b"b" * 39))
                    return chunk

                with mock.patch.object(os, "open", side_effect=open_record), \
                        mock.patch.object(os, "read", side_effect=read_record), \
                        self.assertRaises((OSError, RuntimeError)):
                    capacity_exec.package_records(package)
                self.assertEqual(len(opened), 1)
                with self.assertRaises(OSError):
                    os.fstat(opened[0])

    def test_static_symlink_and_special_metadata_are_refused(self):
        for index, reader in enumerate(READERS):
            for kind in ("symlink", "fifo"):
                with self.subTest(reader=reader.__module__, kind=kind):
                    package, _, _ = self.package(kind + "-" + str(index))
                    record = package / "alab-build.json"
                    original = record.read_bytes()
                    record.unlink()
                    if kind == "symlink":
                        foreign = self.root / ("target-" + str(index) + ".json")
                        foreign.write_bytes(original)
                        record.symlink_to(foreign)
                    else:
                        os.mkfifo(record)
                    with self.assertRaises((OSError, RuntimeError, ValueError)):
                        reader(package)


class PackageRecordCleanupTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-package-record-cleanup-")
        self.addCleanup(temporary.cleanup)
        self.path = Path(temporary.name).resolve() / "alab-build.json"
        self.path.write_bytes(b'{"fixture": true}')

    def capture(self):
        try:
            return None, capacity_exec.package_record(self.path)
        except BaseException as error:
            return error, None

    def close_then_fail(self, primary, closed):
        original = os.close

        def close(descriptor):
            original(descriptor)
            closed.append(descriptor)
            raise primary

        return close

    def assert_closed(self, closed):
        self.assertEqual(len(closed), 1)
        with self.assertRaises(OSError):
            os.fstat(closed[0])

    def test_immediate_post_open_cancellation_closes_the_published_descriptor(self):
        for primary in (KeyboardInterrupt("post-open"), SystemExit(0)):
            with self.subTest(primary=type(primary).__name__):
                previous, original_open, opened = sys.gettrace(), os.open, []

                def open_then_cancel(*args, **kwargs):
                    descriptor = original_open(*args, **kwargs)
                    opened.append(descriptor)
                    caller = sys._getframe(1)
                    self.assertIs(caller.f_code, capacity_exec.package_record.__code__)

                    def boundary(frame, event, _argument):
                        if frame is caller and event == "line":
                            sys.settrace(previous)
                            frame.f_trace = None
                            raise primary
                        return boundary

                    caller.f_trace = boundary
                    sys.settrace(boundary)
                    return descriptor

                try:
                    with mock.patch.object(os, "open", new=open_then_cancel):
                        caught, result = self.capture()
                    self.assertIs(caught, primary)
                    self.assertIsNone(result)
                    self.assert_closed(opened)
                finally:
                    sys.settrace(previous)
                    # Own the real descriptors even on the old implementation.
                    for descriptor in opened:
                        try:
                            os.fstat(descriptor)
                        except OSError:
                            pass
                        else:
                            os.close(descriptor)

    def test_secondary_close_failures_preserve_exact_read_and_stat_outcomes(self):
        for operation in ("read", "fstat"):
            for primary_type in (OSError, KeyboardInterrupt, SystemExit):
                for secondary in (OSError("secondary close"), KeyboardInterrupt("secondary close"), SystemExit(0)):
                    primary = primary_type(0 if primary_type is SystemExit else "primary read/stat failure")
                    closed = []
                    with self.subTest(operation=operation, primary=type(primary).__name__, secondary=type(secondary).__name__):
                        with mock.patch.object(os, operation, side_effect=primary), \
                                mock.patch.object(os, "close", new=self.close_then_fail(secondary, closed)):
                            caught, result = self.capture()
                        self.assertIs(caught, primary)
                        self.assertIsNone(result)
                        self.assert_closed(closed)
                        fields = json.dumps(capacity_exec.case_error_fields(primary))
                        self.assertIn("descriptor close", fields)
                        self.assertIn(type(secondary).__name__, fields)

    def test_real_decode_and_json_errors_survive_a_secondary_close_failure(self):
        for data, expected in ((b"\xff", UnicodeDecodeError), (b"{broken-json}", json.JSONDecodeError)):
            for secondary in (OSError("secondary close"), SystemExit(0)):
                with self.subTest(primary=expected.__name__, secondary=type(secondary).__name__):
                    self.path.write_bytes(data)
                    previous, observed, closed = sys.gettrace(), [], []

                    def observe(frame, event, argument):
                        if (frame.f_code is capacity_exec.package_record.__code__ and event == "exception"
                                and isinstance(argument[1], expected)):
                            observed.append(argument[1])
                        return observe

                    try:
                        sys.settrace(observe)
                        with mock.patch.object(os, "close", new=self.close_then_fail(secondary, closed)):
                            caught, result = self.capture()
                    finally:
                        sys.settrace(previous)
                    self.assertTrue(observed, "the actual decoder did not raise")
                    self.assertIs(caught, observed[0])
                    self.assertIsNone(result)
                    self.assert_closed(closed)

    def test_close_only_errors_and_genuine_nonzero_exits_do_not_return_metadata(self):
        for primary in (OSError("close"), KeyboardInterrupt("close"), SystemExit(47),
                        SystemExit(0.0), SystemExit(sys.maxsize + 1)):
            with self.subTest(primary=type(primary).__name__):
                closed = []
                with mock.patch.object(os, "close", new=self.close_then_fail(primary, closed)):
                    caught, result = self.capture()
                self.assertIs(caught, primary)
                self.assertIsNone(result)
                self.assert_closed(closed)

    def test_success_shaped_close_exits_are_definite_failures_with_the_exact_cause(self):
        for code in (None, False, 0, 256, -256):
            with self.subTest(code=code):
                primary, closed = SystemExit(code), []
                with mock.patch.object(os, "close", new=self.close_then_fail(primary, closed)):
                    caught, result = self.capture()
                self.assertIsInstance(caught, RuntimeError)
                self.assertIn("close incomplete", str(caught))
                self.assertIs(caught.__cause__, primary)
                self.assertIsNone(result)
                self.assert_closed(closed)

    def test_unavailable_formatting_and_notes_do_not_replace_primary(self):
        class Primary(OSError):
            def add_note(self, _note):
                raise KeyboardInterrupt("native note refused")

            def __setattr__(self, name, value):
                if name == "_alab_case_cleanup_errors":
                    raise SystemExit(0)
                super().__setattr__(name, value)

        class CloseFailure(OSError):
            def __str__(self):
                raise SystemExit(0)

        primary, closed = Primary("original read"), []
        with mock.patch.object(os, "read", side_effect=primary), \
                mock.patch.object(os, "close", new=self.close_then_fail(CloseFailure(), closed)):
            caught, result = self.capture()
        self.assertIs(caught, primary)
        self.assertIsNone(result)
        self.assert_closed(closed)

    def test_open_refusal_wrapper_and_no_ownership_are_preserved(self):
        for primary in (OSError("open refused"), KeyboardInterrupt("open cancelled"), SystemExit(0)):
            with self.subTest(primary=type(primary).__name__), \
                    mock.patch.object(os, "open", side_effect=primary), mock.patch.object(os, "close") as close:
                caught, result = self.capture()
                self.assertIsNone(result)
                if isinstance(primary, OSError):
                    self.assertIsInstance(caught, RuntimeError)
                    self.assertEqual(str(caught), "missing regular alab-build.json")
                    self.assertIs(caught.__cause__, primary)
                else:
                    self.assertIs(caught, primary)
                close.assert_not_called()

    def test_single_record_reader_keeps_general_json_decode_semantics(self):
        for value in (None, [], 123, "fixture", {"fixture": True}):
            with self.subTest(value=value):
                self.path.write_text(json.dumps(value))
                self.assertEqual(capacity_exec.package_record(self.path), value)


if __name__ == "__main__":
    unittest.main()
