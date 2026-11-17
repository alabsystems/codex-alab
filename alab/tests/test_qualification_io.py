# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Inspect real owned records/logs using local Git and the finite shell gate fixture."""
from contextlib import contextmanager
import copy
import errno
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
from types import SimpleNamespace
import unittest
from unittest import mock

import test_qualification as qualification_fixture


QUAL = qualification_fixture.QUAL
PARTIAL = 8192


class QualificationIoTests(unittest.TestCase):
    def setUp(self):
        self.world = qualification_fixture.QualificationTests()
        self.addCleanup(self.world.doCleanups)
        self.world.setUp()
        status, self.output, _stream = self.world.execute()
        self.assertEqual(status, 0)
        self.record_path, self.log = self.output / "record.json", self.output / "gate.log"
        self.record = json.loads(self.record_path.read_text())

    @contextmanager
    def partial_reads(self, target, after_read):
        """Both old Path IO and the real descriptor owner consume physical bytes.

        Metadata/hashes are never mocked. Shared reads are capped, and only the
        selected path's bytes trigger a finite write to that same owned file.
        The private primitive namespace never intercepts Git's own IO.
        """
        handles, descriptors, reads, closes = [], set(), [], []
        owner = self

        def observed(data):
            if data:
                reads.append(len(data))
                owner.assertLessEqual(len(reads), 64, "finite fixture read budget exceeded")
                after_read(len(reads))
            return data

        class LimitedRaw(io.RawIOBase):
            def __init__(self, raw):
                self.raw = raw
            def readable(self):
                return True
            def fileno(self):
                return self.raw.fileno()
            def readinto(self, destination):
                count = self.raw.readinto(memoryview(destination)[:PARTIAL])
                if count:
                    observed(bytes(destination[:count]))
                return count
            def close(self):
                try:
                    self.raw.close()
                finally:
                    super().close()

        class ReadingPath(type(Path())):
            def open(candidate, mode="r", buffering=-1, encoding=None, errors=None, newline=None):
                if candidate != target or mode not in ("r", "rb"):
                    return super().open(mode, buffering, encoding, errors, newline)
                raw = super().open("rb", buffering=0)
                handles.append(raw)
                limited = LimitedRaw(raw)
                try:
                    buffered = io.BufferedReader(limited, buffer_size=PARTIAL)
                    return buffered if mode == "rb" else io.TextIOWrapper(
                        buffered, encoding=encoding, errors=errors, newline=newline)
                except BaseException:
                    limited.close()
                    raise

        def opening(path, *args, **kwargs):
            descriptor = os.open(path, *args, **kwargs)
            if Path(path) == target:
                descriptors.add(descriptor)
            return descriptor

        def read(descriptor, amount):
            data = os.read(descriptor, min(PARTIAL, amount))
            return observed(data) if descriptor in descriptors else data

        def close(descriptor):
            os.close(descriptor)
            if descriptor in descriptors:
                descriptors.remove(descriptor)
                closes.append(descriptor)

        shared = getattr(QUAL, "FILE_HASH", None)
        reader = shared["file_sha256"].__globals__ if shared is not None else None
        namespace = None
        if reader is not None:
            namespace = SimpleNamespace(**vars(os))
            namespace.open, namespace.read, namespace.close = opening, read, close
        state = SimpleNamespace(reads=reads, closes=closes)
        try:
            with mock.patch.object(QUAL, "Path", ReadingPath):
                if reader is None:
                    yield state
                else:
                    with mock.patch.dict(reader, {"os": namespace}):
                        yield state
        finally:
            leaked_files = [raw for raw in handles if not raw.closed]
            leaked_descriptors = tuple(descriptors)
            retired = []
            for descriptor in closes:
                try:
                    os.fstat(descriptor)
                except OSError as error:
                    retired.append(error.errno == errno.EBADF)
                else:
                    retired.append(False)
            for raw in handles:
                raw.close()
            for descriptor in leaked_descriptors:
                os.close(descriptor)
            self.assertEqual(leaked_files, [])
            self.assertEqual(leaked_descriptors, ())
            self.assertTrue(all(retired))
            self.assertEqual(len(handles) + len(closes), 1, "exactly one returned file owns the selected read")

    def inspect(self):
        try:
            return None, QUAL.inspect(self.world.repo, self.record_path)
        except BaseException as error:
            return error, None

    def test_healthy_record_and_log_are_accepted(self):
        self.assertEqual(QUAL.inspect(self.world.repo, self.record_path)["source"], self.world.source)

    def test_same_size_log_mix_is_refused_even_when_its_digest_matches_the_record(self):
        first, final = b"A" * (8 * PARTIAL), b"B" * (8 * PARTIAL)
        hybrid = first[:PARTIAL] + final[PARTIAL:]
        record = copy.deepcopy(self.record)
        record["log_sha256"] = hashlib.sha256(hybrid).hexdigest()
        self.record_path.write_text(json.dumps(record) + "\n")
        for data in (first, final):
            self.log.write_bytes(data)
            with self.assertRaisesRegex(ValueError, "log hash changed"):
                QUAL.inspect(self.world.repo, self.record_path)
        self.log.write_bytes(first)
        before = self.log.stat()
        mutations = []

        def rewrite(index):
            if index == 1:
                self.log.write_bytes(final)
                mutations.append(True)

        with self.partial_reads(self.log, rewrite) as state:
            error, result = self.inspect()
        after = self.log.stat()
        self.assertEqual(mutations, [True])
        self.assertEqual(before.st_size, after.st_size)
        self.assertNotEqual((before.st_mtime_ns, before.st_ctime_ns), (after.st_mtime_ns, after.st_ctime_ns))
        self.assertNotEqual(hashlib.sha256(self.log.read_bytes()).hexdigest(), record["log_sha256"])
        self.assertGreater(len(state.reads), 1)
        self.assertIsInstance(error, ValueError)
        self.assertIsNone(result)

    def test_record_mix_is_refused_when_neither_complete_record_is_valid(self):
        first = {"schema": self.record["schema"], "kind": self.record["kind"],
                 "before": copy.deepcopy(self.record["before"]), "after": copy.deepcopy(self.record["after"]),
                 "qualified_source": self.record["qualified_source"], "padding": "P" * (4 * PARTIAL),
                 "status": 1, "log": "gate.log", "log_sha256": self.record["log_sha256"]}
        final = copy.deepcopy(first)
        final["before"]["clean"] = final["after"]["clean"] = False
        final["status"] = 0
        first_bytes, final_bytes = (json.dumps(value) + "\n" for value in (first, final))
        first_bytes += " " * (len(final_bytes) - len(first_bytes))
        self.assertEqual(len(first_bytes), len(final_bytes))
        for data in (first_bytes, final_bytes):
            self.record_path.write_text(data)
            with self.assertRaisesRegex(ValueError, "no successful unchanged-source"):
                QUAL.inspect(self.world.repo, self.record_path)
        self.record_path.write_text(first_bytes)
        before = self.record_path.stat()
        mutations = []

        def rewrite(index):
            if index == 1:
                self.record_path.write_text(final_bytes)
                mutations.append(True)

        with self.partial_reads(self.record_path, rewrite) as state:
            error, result = self.inspect()
        after = self.record_path.stat()
        self.assertEqual(mutations, [True])
        self.assertEqual(before.st_size, after.st_size)
        self.assertNotEqual((before.st_mtime_ns, before.st_ctime_ns), (after.st_mtime_ns, after.st_ctime_ns))
        self.assertGreater(len(state.reads), 1)
        self.assertIsInstance(error, ValueError)
        self.assertIsNone(result)

    def test_record_and_log_growth_stop_at_each_initial_size_plus_one(self):
        for role in ("record", "log"):
            with self.subTest(role=role):
                self.log.write_bytes(b"A" * (2 * PARTIAL))
                record = copy.deepcopy(self.record)
                appended = b"B" * PARTIAL if role == "log" else b" " * PARTIAL
                record["log_sha256"] = hashlib.sha256(self.log.read_bytes() + appended * 6).hexdigest() if (
                    role == "log") else hashlib.sha256(self.log.read_bytes()).hexdigest()
                self.record_path.write_text(json.dumps(record) + "\n")
                path = self.log if role == "log" else self.record_path
                initial = path.stat().st_size
                appends = []

                def grow(index):
                    if index <= 6:
                        with path.open("ab") as stream:
                            stream.write(appended)
                        appends.append(True)

                with self.partial_reads(path, grow) as state:
                    error, result = self.inspect()
                self.assertTrue(appends)
                self.assertLessEqual(sum(state.reads), initial + 1)
                self.assertIsInstance(error, ValueError)
                self.assertIsNone(result)

    def test_observer_writer_also_rejects_growth_while_hashing_its_closed_log(self):
        self.world.install("printf '%016384d' 0\n")
        output = (self.world.root / "writer-observation").resolve()
        log = output / "gate.log"
        appends = []

        def grow(index):
            if index <= 6:
                with log.open("ab") as stream:
                    stream.write(b"B" * PARTIAL)
                appends.append(True)

        error = None
        with self.partial_reads(log, grow) as state:
            try:
                self.world.execute("writer-observation")
            except BaseException as caught:
                error = caught
        self.assertTrue(appends)
        self.assertLessEqual(sum(state.reads), 16384 + 1)
        self.assertIsInstance(error, ValueError)
        self.assertFalse((output / "record.json").exists())

    def test_minimal_record_arbitrary_name_copies_and_parent_aliases_still_work(self):
        kept = ("schema", "kind", "before", "after", "status", "qualified_source", "log", "log_sha256")
        minimal = {name: self.record[name] for name in kept}
        self.record_path.write_text(json.dumps(minimal) + "\n")
        copied = self.world.root / "copied observation"
        shutil.copytree(self.output, copied)
        arbitrary = copied / "arbitrary-name.txt"
        (copied / "record.json").rename(arbitrary)
        alias = self.world.root / "parent alias"
        alias.symlink_to(copied, target_is_directory=True)
        for path in (self.record_path, arbitrary, alias / arbitrary.name):
            with self.subTest(path=path):
                result = QUAL.inspect(self.world.repo, path)
                self.assertEqual(result["source"], self.world.source)
                self.assertIsNone(result["completed_at"])
                self.assertEqual(result["record"], str(path.resolve()))

    def test_default_text_decoding_and_universal_newlines_are_preserved(self):
        record = {**self.record, "literal": "naïve 雪"}
        raw = (json.dumps(record, ensure_ascii=False, indent=2).replace("\n", "\r\n") + "\r\n").encode("utf-8")
        self.record_path.write_bytes(raw)
        self.assertEqual(json.loads(self.record_path.read_text()), record)
        self.assertEqual(QUAL.inspect(self.world.repo, self.record_path)["source"], self.world.source)
        text = json.dumps(record, ensure_ascii=False)
        for data in (b"\xff", b"{", b"\xef\xbb\xbf" + raw, text.encode("utf-16"), text.encode("utf-32")):
            with self.subTest(prefix=data[:4]):
                self.record_path.write_bytes(data)
                with self.assertRaises(ValueError):
                    json.loads(self.record_path.read_text())
                with self.assertRaises(ValueError):
                    QUAL.inspect(self.world.repo, self.record_path)

    def test_existing_leaf_symlink_refusal_is_unchanged_for_both_inputs(self):
        for path in (self.record_path, self.log):
            with self.subTest(path=path):
                retained = path.with_name(path.name + ".retained")
                path.rename(retained)
                path.symlink_to(retained)
                try:
                    with self.assertRaisesRegex(ValueError, "must not be symlinks"):
                        QUAL.inspect(self.world.repo, self.record_path)
                finally:
                    path.unlink()
                    retained.rename(path)

    def test_leaf_symlink_swap_before_descriptor_open_is_refused(self):
        reader = QUAL.FILE_HASH["file_sha256"].__globals__
        for path in (self.record_path, self.log):
            with self.subTest(path=path):
                donor = path.with_name(path.name + ".donor")
                backup = path.with_name(path.name + ".original")
                original = path.read_bytes()
                donor.write_bytes(original)
                attempts = []

                def open_after_swap(candidate, flags, *args, **kwargs):
                    if Path(candidate) == path:
                        attempts.append(flags)
                        path.rename(backup)
                        path.symlink_to(donor)
                    return os.open(candidate, flags, *args, **kwargs)

                namespace = SimpleNamespace(**vars(os))
                namespace.open = open_after_swap
                try:
                    with mock.patch.dict(reader, {"os": namespace}):
                        with self.assertRaises(OSError):
                            QUAL.inspect(self.world.repo, self.record_path)
                    self.assertEqual(len(attempts), 1)
                    self.assertTrue(attempts[0] & os.O_NOFOLLOW)
                    self.assertEqual(donor.read_bytes(), original)
                finally:
                    if backup.exists():
                        path.unlink()
                        backup.rename(path)

    def test_large_regular_metadata_has_no_new_absolute_byte_cap(self):
        record = {**self.record, "padding": "P" * (1024 * 1024 + 1)}
        self.record_path.write_text(json.dumps(record) + "\n")
        self.assertGreater(self.record_path.stat().st_size, 1024 * 1024)
        self.assertEqual(QUAL.inspect(self.world.repo, self.record_path)["source"], self.world.source)


if __name__ == "__main__":
    unittest.main()
