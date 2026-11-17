# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Renewal's public image evidence uses stable cached descriptor snapshots only."""
import hashlib
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import renewal_follow as renewal


class ImageDigestTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-renewal-image-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.path = self.root / "inert-image"
        self.path.write_bytes(b"a" * 100)
        cache = mock.patch.dict(renewal.IMAGE_DIGESTS, clear=True)
        cache.start()
        self.addCleanup(cache.stop)

    def test_stable_image_is_read_once_but_descriptor_metadata_is_checked_on_every_call(self):
        original_read, original_stat = os.read, os.fstat
        with mock.patch.object(os, "read", wraps=original_read) as read, \
                mock.patch.object(os, "fstat", wraps=original_stat) as stat:
            for _ in range(3):
                metadata, digest = renewal.image_digest(self.path)
                self.assertEqual(digest, hashlib.sha256(b"a" * 100).hexdigest())
                self.assertEqual((metadata.st_dev, metadata.st_ino),
                                 (self.path.stat().st_dev, self.path.stat().st_ino))
            self.assertEqual(read.call_count, 2)
            self.assertEqual(stat.call_count, 6)
        self.assertEqual(len(renewal.IMAGE_DIGESTS), 1)

    def test_same_size_rewrite_with_restored_mtime_invalidates_cached_digest(self):
        initial = renewal.image_digest(self.path)
        self.path.write_bytes(b"b" * 100)
        os.utime(self.path, ns=(initial[0].st_atime_ns, initial[0].st_mtime_ns))
        changed = renewal.image_digest(self.path)
        self.assertNotEqual(initial[1], changed[1])
        self.assertEqual(changed[1], hashlib.sha256(b"b" * 100).hexdigest())
        self.assertNotEqual(initial[0].st_ctime_ns, changed[0].st_ctime_ns)

    def test_returned_metadata_and_digest_describe_the_same_opened_descriptor(self):
        replacement, retained = self.root / "replacement", self.root / "retained-image"
        replacement.write_bytes(b"b" * 100)
        original_open = os.open

        def replace_after_open(path, *args, **kwargs):
            descriptor = original_open(path, *args, **kwargs)
            try:
                self.path.rename(retained)
                replacement.rename(self.path)
            except BaseException:
                os.close(descriptor)
                raise
            return descriptor

        with mock.patch.object(os, "open", new=replace_after_open):
            metadata, digest = renewal.image_digest(self.path)
        self.assertEqual(metadata.st_ino, retained.stat().st_ino)
        self.assertNotEqual(metadata.st_ino, self.path.stat().st_ino)
        self.assertEqual(digest, hashlib.sha256(b"a" * 100).hexdigest())
        self.assertNotEqual(digest, hashlib.sha256(self.path.read_bytes()).hexdigest())

    def mutation_seams(self, action):
        """Use the actual old and new reader boundaries for the same real mutation."""
        original_open, original_read = Path.open, os.read
        handles, descriptors = [], []

        class Reader:
            def __init__(self, handle):
                self.handle = handle
                handles.append(handle)
            def __getattr__(self, key):
                return getattr(self.handle, key)
            def __enter__(self):
                return self
            def __exit__(self, *args):
                return self.handle.__exit__(*args)
            def readinto(self, buffer):
                action(None)
                count = self.handle.readinto(buffer)
                action(count)
                return count

        def open_reader(path, *args, **kwargs):
            handle = original_open(path, *args, **kwargs)
            return Reader(handle) if path == self.path and args and args[0] == "rb" else handle

        def read(fd, amount):
            descriptors.append(fd)
            action(None)
            data = original_read(fd, amount)
            action(len(data))
            return data

        return mock.patch.object(Path, "open", new=open_reader), mock.patch.object(os, "read", new=read), handles, descriptors

    def assert_closed(self, handles, descriptors):
        self.assertTrue(all(handle.closed for handle in handles))
        for descriptor in set(descriptors):
            with self.assertRaises(OSError):
                os.fstat(descriptor)

    def test_process_evidence_refuses_the_proven_cross_version_read_without_caching_it(self):
        self.path.write_bytes(b"A" * 600000)
        mutated = False

        def overwrite(count):
            nonlocal mutated
            if count and not mutated:
                self.path.write_bytes(b"B" * 600000)
                mutated = True

        opened, read, handles, descriptors = self.mutation_seams(overwrite)
        record = {"exe": str(self.path), "pid": 123456,
                  "identity": {"startSeconds": 7, "startMicroseconds": 8}}
        with opened, read, self.assertRaisesRegex(RuntimeError, "changed while hashing"):
            renewal.process_evidence(record)
        self.assertTrue(mutated)
        self.assertEqual(renewal.IMAGE_DIGESTS, {})
        self.assert_closed(handles, descriptors)

    def test_continuous_growth_stops_after_at_most_initial_size_plus_one(self):
        calls, received = 0, 0

        def append(count):
            nonlocal calls, received
            if count is None:
                calls += 1
                self.assertLess(calls, 4, "image digest followed continuous growth")
            else:
                received += count
                with self.path.open("ab") as stream:
                    stream.write(b"x")

        opened, read, handles, descriptors = self.mutation_seams(append)
        with opened, read, self.assertRaisesRegex(RuntimeError, "changed while hashing"):
            renewal.image_digest(self.path)
        self.assertEqual((calls, received), (2, 101))
        self.assertEqual(renewal.IMAGE_DIGESTS, {})
        self.assert_closed(handles, descriptors)

    def test_cache_hit_refuses_mutation_between_metadata_observations(self):
        renewal.image_digest(self.path)
        original_stat = os.fstat
        calls = 0

        def change_after_stat(fd):
            nonlocal calls
            calls += 1
            metadata = original_stat(fd)
            if calls == 1:
                self.path.write_bytes(b"b" * 100)
            return metadata

        with mock.patch.object(os, "fstat", new=change_after_stat), \
                mock.patch.object(os, "read", side_effect=AssertionError("cache hit reread content")), \
                self.assertRaisesRegex(RuntimeError, "changed while hashing"):
            renewal.image_digest(self.path)
        self.assertEqual(calls, 2)
        self.assertEqual(len(renewal.IMAGE_DIGESTS), 1)
        self.assertEqual(renewal.image_digest(self.path)[1], hashlib.sha256(b"b" * 100).hexdigest())

    def test_cache_publication_follows_successful_descriptor_close(self):
        original_open = os.open
        opened = []

        class Cache(dict):
            def __setitem__(cache, key, digest):
                self.assertEqual(len(opened), 1)
                with self.assertRaises(OSError):
                    os.fstat(opened[0])
                super().__setitem__(key, digest)

        def open_file(*args, **kwargs):
            fd = original_open(*args, **kwargs)
            opened.append(fd)
            return fd

        cache = Cache()
        with mock.patch.object(renewal, "IMAGE_DIGESTS", cache), mock.patch.object(os, "open", new=open_file):
            metadata, digest = renewal.image_digest(self.path)
        self.assertEqual(len(cache), 1)
        self.assertEqual(digest, hashlib.sha256(b"a" * 100).hexdigest())
        self.assertEqual(metadata.st_ino, self.path.stat().st_ino)

    def test_failed_close_or_cancelled_read_does_not_publish_a_cache_entry(self):
        for operation in ("read", "close"):
            for primary in (OSError("controlled image IO failure"), KeyboardInterrupt(), SystemExit(0)):
                with self.subTest(operation=operation, error=type(primary).__name__):
                    original_close = os.close
                    descriptors = []

                    def close(fd):
                        original_close(fd)
                        descriptors.append(fd)
                        if operation == "close":
                            raise primary

                    read = mock.patch.object(os, "read", side_effect=primary) if operation == "read" else mock.patch.object(os, "read", wraps=os.read)
                    expected = RuntimeError if operation == "close" and isinstance(primary, SystemExit) else type(primary)
                    with read, mock.patch.object(os, "close", new=close), self.assertRaises(expected) as caught:
                        renewal.image_digest(self.path)
                    if expected is RuntimeError:
                        self.assertIs(caught.exception.__cause__, primary)
                    else:
                        self.assertIs(caught.exception, primary)
                    self.assertEqual(renewal.IMAGE_DIGESTS, {})
                    self.assertEqual(len(descriptors), 1)
                    self.assert_closed([], descriptors)

    def test_symlink_and_directory_inputs_are_refused(self):
        linked = self.root / "linked"
        linked.symlink_to(self.path)
        for path in (linked, self.root):
            with self.subTest(path=path), self.assertRaises((OSError, RuntimeError)):
                renewal.image_digest(path)
        self.assertEqual(renewal.IMAGE_DIGESTS, {})

    def test_process_evidence_keeps_its_existing_schema(self):
        record = {"exe": str(self.path), "pid": 123456,
                  "identity": {"startSeconds": 7, "startMicroseconds": 8}}
        evidence = renewal.process_evidence(record)
        metadata = self.path.stat()
        self.assertEqual(evidence, {"pid": record["pid"], "birth": record["identity"], "exe": str(self.path),
            "kernelBirth": 7000008, "image": {"device": metadata.st_dev, "inode": metadata.st_ino,
            "sha256": hashlib.sha256(b"a" * 100).hexdigest()}})
