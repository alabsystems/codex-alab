# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Matrix.digest uses real bounded, stable reads of owned regular files."""
from contextlib import contextmanager
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[1] / 'smoke/renewal_matrix.py'
sys.path.insert(0, str(SOURCE.parent))
SPEC = importlib.util.spec_from_file_location('renewal_matrix_digest', SOURCE)
MATRIX = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MATRIX)
PARTIAL = 8192


class RenewalMatrixDigestTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='alab-renewal-matrix-digest-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name).resolve()

    @contextmanager
    def partial_reads(self, path, after_read):
        """Cap real reads, then apply finite physical changes after returned bytes.

        The old Path.read_bytes route opens an actual RawIO/BufferedReader. The
        shared descriptor route gets a private module-local os namespace proxy.
        Neither route replaces bytes, file metadata, hashes or the global os
        module. Every original descriptor/file object remains owned and closed.
        """
        owner = self
        reads, active_files, active_descriptors = [], [], set()
        opened, closed = [], []

        def observed(data):
            if data:
                owner.assertLessEqual(len(data), PARTIAL)
                reads.append({'bytes': len(data), 'prefix': data[:1]})
                after_read(len(reads), data)
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
                    observed(bytes(memoryview(destination)[:count]))
                return count

            def close(self):
                if not self.closed:
                    try:
                        self.raw.close()
                    finally:
                        super().close()

        class ReadingPath(type(Path())):
            def open(candidate, mode='r', buffering=-1, encoding=None, errors=None, newline=None):
                if candidate != path or mode != 'rb':
                    return super().open(mode, buffering, encoding, errors, newline)
                raw = super().open(mode, buffering=0)
                active_files.append(raw)
                limited = LimitedRaw(raw)
                try:
                    return io.BufferedReader(limited, buffer_size=PARTIAL)
                except BaseException:
                    limited.close()
                    raise

        def descriptor_open(candidate, *arguments, **keywords):
            descriptor = os.open(candidate, *arguments, **keywords)
            if Path(candidate) == path:
                active_descriptors.add(descriptor)
                opened.append(descriptor)
            return descriptor

        def descriptor_read(descriptor, length):
            if descriptor not in active_descriptors:
                return os.read(descriptor, length)
            return observed(os.read(descriptor, min(length, PARTIAL)))

        def descriptor_close(descriptor):
            os.close(descriptor)
            if descriptor in active_descriptors:
                active_descriptors.remove(descriptor)
                closed.append(descriptor)

        shared = getattr(MATRIX, 'FILE_HASH', None)
        if shared is None:
            reader = None
        else:
            reader = shared['file_sha256'].__globals__
            namespace = SimpleNamespace(**vars(reader['os']))
            namespace.open = descriptor_open
            namespace.read = descriptor_read
            namespace.close = descriptor_close
        probe = SimpleNamespace(path=ReadingPath(path), reads=reads, opened=opened, closed=closed)
        try:
            if reader is None:
                yield probe
            else:
                with patch.dict(reader, {'os': namespace}):
                    yield probe
        finally:
            # Compensation is independent of Matrix's result: the assertions
            # below still reject any descriptor or file it failed to retire.
            leaked_descriptors = tuple(active_descriptors)
            leaked_files = [raw for raw in active_files if not raw.closed]
            try:
                for descriptor in leaked_descriptors:
                    os.close(descriptor)
                for raw in leaked_files:
                    raw.close()
            finally:
                owner.assertEqual(leaked_descriptors, (), 'digest leaked an owned descriptor')
                owner.assertEqual(leaked_files, [], 'digest leaked an owned file object')

    def test_large_and_empty_regular_files_have_the_exact_digest(self):
        for label, raw in [('empty', b''), ('large', bytes(range(256)) * 8193 + b'end')]:
            with self.subTest(file=label):
                path = self.root / label
                path.write_bytes(raw)
                self.assertEqual(MATRIX.digest(path), hashlib.sha256(raw).hexdigest())
                self.assertEqual(path.read_bytes(), raw)

    def test_second_observation_hashes_fresh_bytes_after_same_size_rewrite(self):
        path = self.root / 'fresh'
        first, second = b'A' * 131072, b'B' * 131072
        path.write_bytes(first)
        with self.partial_reads(path, lambda _index, _data: None) as probe:
            for _ in range(2):
                previous_reads = len(probe.reads)
                self.assertEqual(MATRIX.digest(probe.path), hashlib.sha256(first).hexdigest())
                self.assertGreater(len(probe.reads), previous_reads,
                                   'each matrix observation must read fresh content')
        path.write_bytes(second)
        self.assertEqual(MATRIX.digest(path), hashlib.sha256(second).hexdigest())
        self.assertNotEqual(hashlib.sha256(first).hexdigest(), hashlib.sha256(second).hexdigest())

    def test_same_size_rewrite_between_real_partial_reads_is_rejected(self):
        path = self.root / 'rewritten'
        first, second = b'A' * 131072, b'B' * 131072
        path.write_bytes(first)
        metadata = path.stat()
        rewrites = []

        def rewrite(index, data):
            if index == 1:
                self.assertEqual(data, first[:len(data)])
                path.write_bytes(second)
                # Change actual kernel metadata deterministically as well as
                # content; no fstat response is synthesized by the fixture.
                os.utime(path, ns=(metadata.st_atime_ns, metadata.st_mtime_ns + 1000000000))
                rewrites.append(True)

        failure = None
        with self.partial_reads(path, rewrite) as probe:
            try:
                MATRIX.digest(probe.path)
            except RuntimeError as error:
                failure = error
        self.assertEqual(rewrites, [True])
        self.assertGreaterEqual(len(probe.reads), 2)
        self.assertEqual(probe.reads[0]['prefix'], b'A')
        self.assertEqual(probe.reads[1]['prefix'], b'B')
        self.assertEqual(path.read_bytes(), second)
        self.assertIsInstance(failure, RuntimeError)
        self.assertIn('file changed while hashing', str(failure))

    def test_finite_continuous_growth_never_reads_beyond_initial_size_plus_one(self):
        path = self.root / 'growing'
        original = b'A' * 131072
        path.write_bytes(original)
        appends = []

        def grow(index, data):
            # The legacy whole-file route also terminates: after at most 32
            # real partial reads, this fixture stops extending its owned file.
            if index <= 32:
                with path.open('ab') as stream:
                    stream.write(b'G' * PARTIAL)
                appends.append(index)

        failure = None
        with self.partial_reads(path, grow) as probe:
            try:
                MATRIX.digest(probe.path)
            except RuntimeError as error:
                failure = error
        total_read = sum(entry['bytes'] for entry in probe.reads)
        self.assertGreater(total_read, 0)
        self.assertGreater(len(appends), 0)
        self.assertLessEqual(len(appends), 32)
        self.assertLessEqual(total_read, len(original) + 1)
        self.assertIsInstance(failure, RuntimeError)
        self.assertIn('file changed while hashing', str(failure))
        self.assertEqual(path.stat().st_size, len(original) + len(appends) * PARTIAL)

    def test_leaf_symlink_is_refused_without_changing_its_owned_target(self):
        target = self.root / 'target'
        raw = b'owned symlink target\n'
        target.write_bytes(raw)
        link = self.root / 'link'
        link.symlink_to(target.name)
        with self.assertRaises((OSError, RuntimeError)):
            MATRIX.digest(link)
        self.assertTrue(link.is_symlink())
        self.assertEqual(os.readlink(link), target.name)
        self.assertEqual(target.read_bytes(), raw)

    def test_directory_is_refused_without_blocking_or_changing_it(self):
        directory = self.root / 'directory'
        directory.mkdir()
        sentinel = directory / 'sentinel'
        sentinel.write_bytes(b'keep\n')
        with self.assertRaises((OSError, RuntimeError)):
            MATRIX.digest(directory)
        self.assertEqual(sentinel.read_bytes(), b'keep\n')


if __name__ == '__main__':
    unittest.main()
