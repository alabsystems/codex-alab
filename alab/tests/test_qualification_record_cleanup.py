# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""A failed owned record write retains its failure during temporary cleanup."""

import errno
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "qualification_record_cleanup", Path(__file__).resolve().parents[1] / "qualification.py")
QUAL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(QUAL)


class QualificationRecordCleanupTests(unittest.TestCase):
    def failed_publication(self, primary, cleanup):
        record = {"kind": "owned-record-cleanup-fixture", "payload": "complete real JSON"}
        real_dump = json.dump
        real_unlink = Path.unlink
        events = []
        observed = {}
        caught = None

        with tempfile.TemporaryDirectory(prefix="alab-qualification-record-cleanup-") as temporary:
            root = Path(temporary).resolve()
            final = root / "record.json"

            def dump(value, writer, *arguments, **keywords):
                self.assertIs(value, record)
                real_dump(value, writer, *arguments, **keywords)
                writer.flush()
                path = Path(writer.name)
                identity = os.fstat(writer.fileno())
                observed.update(writer=writer, path=path,
                                inode=(identity.st_dev, identity.st_ino),
                                bytes=path.read_bytes())
                self.assertEqual(json.loads(observed["bytes"]), record)
                self.assertFalse(writer.closed)
                self.assertFalse(final.exists())
                events.append("complete-dump")
                raise primary

            class ControlledPath(type(Path())):
                def unlink(path, *arguments, **keywords):
                    if path.parent == root and path.name.startswith(".qualification-record-"):
                        self.assertEqual(path, observed["path"])
                        identity = path.stat()
                        self.assertEqual((identity.st_dev, identity.st_ino), observed["inode"])
                        self.assertEqual(path.read_bytes(), observed["bytes"])
                        observed["closed_before_cleanup"] = observed["writer"].closed
                        observed["public_at_cleanup"] = final.exists()
                        events.append("temporary-unlink")
                        raise cleanup
                    return real_unlink(path, *arguments, **keywords)

            with mock.patch.object(QUAL, "Path", ControlledPath), \
                    mock.patch.object(QUAL.json, "dump", dump):
                try:
                    QUAL.write_record(root, record)
                except BaseException as error:
                    caught = error

            self.assertEqual(events, ["complete-dump", "temporary-unlink"])
            self.assertTrue(observed["closed_before_cleanup"])
            self.assertFalse(observed["public_at_cleanup"])
            self.assertTrue(observed["writer"].closed)
            self.assertFalse(final.exists())
            # Witness the failed unlink before TemporaryDirectory compensates
            # for this test-owned orphan, after both source patches have ended.
            self.assertTrue(observed["path"].is_file())
            self.assertEqual(observed["path"].read_bytes(), observed["bytes"])
        return caught

    def test_serialization_error_survives_temporary_cleanup_io_error(self):
        primary = OSError(errno.EIO, "owned completed serialization failure")
        cleanup = OSError(errno.EIO, "owned temporary unlink failure")
        caught = self.failed_publication(primary, cleanup)
        self.assertIs(caught, primary)
        self.assertIn("qualification record temporary cleanup failed: " + repr(cleanup),
                      getattr(caught, "__notes__", ()))

    def test_cleanup_cancellation_takes_priority_over_serialization_error(self):
        for cleanup in (KeyboardInterrupt("owned unlink cancellation"), SystemExit(47)):
            with self.subTest(cleanup=type(cleanup).__name__):
                primary = OSError(errno.EIO, "owned completed serialization failure")
                caught = self.failed_publication(primary, cleanup)
                self.assertIs(caught, cleanup)
                self.assertIs(caught.__cause__, primary)
                self.assertIn("qualification operation failed: " + repr(primary),
                              getattr(caught, "__notes__", ()))

    def test_primary_cancellation_survives_later_cleanup_failures(self):
        cases = (
            (KeyboardInterrupt("owned serialization cancellation"),
             OSError(errno.EIO, "owned temporary unlink failure")),
            (SystemExit(47), OSError(errno.EIO, "owned temporary unlink failure")),
            (KeyboardInterrupt("owned serialization cancellation"), SystemExit(59)),
            (SystemExit(47), KeyboardInterrupt("later owned unlink cancellation")),
        )
        for primary, cleanup in cases:
            with self.subTest(primary=type(primary).__name__, cleanup=type(cleanup).__name__):
                caught = self.failed_publication(primary, cleanup)
                self.assertIs(caught, primary)
                self.assertIn("qualification record temporary cleanup failed: " + repr(cleanup),
                              getattr(caught, "__notes__", ()))
