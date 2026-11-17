# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Failed bundle exports retain material without deleting replacement paths."""

import copy
import errno
import json
import unittest
from unittest.mock import patch

import test_feature_evidence as fixtures


class FeatureBundleCustodyTests(unittest.TestCase):
    git = fixtures.FeatureEvidenceTests.git
    record = fixtures.FeatureEvidenceTests.record
    observation = fixtures.FeatureEvidenceTests.observation

    def setUp(self):
        fixtures.FeatureEvidenceTests.setUp(self)
        self.sequence = 0

    def identity(self, path):
        value = path.stat()
        return value.st_dev, value.st_ino

    def failed_export(self, primary, *, replacement=None):
        self.sequence += 1
        destination = self.repo / ('bundle-' + str(self.sequence))
        held = self.repo / ('held-' + str(self.sequence))
        target = self.repo / ('target-' + str(self.sequence))
        record = self.record()
        original = copy.deepcopy(record)
        raw = self.log.read_bytes()
        log_identity = self.identity(self.log)
        read = fixtures.EVIDENCE.log_bytes
        observed = {}
        calls = []

        def log_bytes(observation, log_root=None):
            calls.append(log_root)
            if len(calls) == 2:
                observed['created'] = self.identity(destination)
                self.assertTrue((destination / 'logs').is_dir())
                if replacement is not None:
                    destination.rename(held)
                    if replacement == 'directory':
                        destination.mkdir()
                        sentinel = destination / 'sentinel'
                    else:
                        target.mkdir()
                        sentinel = target / 'sentinel'
                        destination.symlink_to(target, target_is_directory=True)
                    sentinel.write_bytes(b'owned replacement must survive\n')
                    observed['replacement'] = self.identity(destination)
                    entry = destination.lstat()
                    observed['replacement_entry'] = entry.st_dev, entry.st_ino
                    observed['sentinel'] = self.identity(sentinel)
                raise primary
            return read(observation, log_root)

        caught = None
        with patch.object(fixtures.EVIDENCE, 'log_bytes', side_effect=log_bytes):
            try:
                fixtures.EVIDENCE.bundle(record, self.expected, destination)
            except BaseException as error:
                caught = error
        self.assertIs(caught, primary)
        self.assertEqual(calls, [None, None])
        self.assertEqual(record, original)
        self.assertEqual(self.log.read_bytes(), raw)
        self.assertEqual(self.identity(self.log), log_identity)
        partial = held if replacement is not None else destination
        self.assertTrue(partial.is_dir(), 'failed export material must remain')
        self.assertEqual(self.identity(partial), observed['created'])
        self.assertTrue((partial / 'logs').is_dir())
        self.assertFalse((partial / 'record.json').exists())
        if replacement is not None:
            self.assertTrue(destination.is_dir(), 'replacement custody must survive rollback')
            self.assertEqual(self.identity(destination), observed['replacement'])
            entry = destination.lstat()
            self.assertEqual((entry.st_dev, entry.st_ino), observed['replacement_entry'])
            sentinel = destination / 'sentinel'
            self.assertTrue(sentinel.is_file())
            self.assertEqual(self.identity(sentinel), observed['sentinel'])
            self.assertEqual(sentinel.read_bytes(), b'owned replacement must survive\n')
            if replacement == 'symlink':
                self.assertTrue(destination.is_symlink())
                self.assertEqual(destination.resolve(), target.resolve())
            else:
                self.assertFalse(destination.is_symlink())
        return destination, record

    def test_read_error_retains_the_owned_partial_and_exact_exception(self):
        self.failed_export(OSError(errno.EIO, 'owned captured-log read failure'))

    def test_read_cancellations_retain_partial_and_exact_objects(self):
        for primary in (KeyboardInterrupt('owned read cancellation'), SystemExit(0)):
            with self.subTest(kind=type(primary).__name__):
                self.failed_export(primary)

    def test_replacement_directory_survives_read_error_with_original_partial(self):
        self.failed_export(OSError(errno.EIO, 'owned captured-log read failure'),
                           replacement='directory')

    def test_replacement_directory_survives_read_cancellation(self):
        self.failed_export(KeyboardInterrupt('owned read cancellation'), replacement='directory')

    def test_replacement_symlink_and_target_survive_the_original_read_error(self):
        self.failed_export(OSError(errno.EIO, 'owned captured-log read failure'),
                           replacement='symlink')

    def test_replacement_symlink_survives_read_cancellation(self):
        self.failed_export(SystemExit(0), replacement='symlink')

    def test_retained_partial_cannot_validate_as_complete_portable_evidence(self):
        destination, record = self.failed_export(OSError(errno.EIO, 'owned read failure'))
        portable = copy.deepcopy(record)
        observation = self.observation(portable)
        observation['log'] = 'logs/' + observation['log_sha256'] + '.log'
        self.assertTrue(fixtures.EVIDENCE.validate(portable, self.expected, destination))
        self.assertFalse((destination / 'record.json').exists())

    def test_success_deduplication_and_existing_destination_remain_intact(self):
        destination = self.repo / 'complete-bundle'
        record = self.record()
        checks = record['features'][0]['scenarios'][0]['checks']
        checks.append(copy.deepcopy(checks[0]))
        fixtures.EVIDENCE.bundle(record, self.expected, destination)
        stored = (destination / 'record.json').read_bytes()
        exported = json.loads(stored)
        self.assertEqual(len(list((destination / 'logs').iterdir())), 1)
        self.assertEqual(len(exported['features'][0]['scenarios'][0]['checks']), 2)
        self.assertEqual(fixtures.EVIDENCE.validate(exported, self.expected, destination), [])
        with self.assertRaises(FileExistsError):
            fixtures.EVIDENCE.bundle(record, self.expected, destination)
        self.assertEqual((destination / 'record.json').read_bytes(), stored)

    def test_invalid_evidence_is_refused_before_destination_claim(self):
        destination = self.repo / 'unclaimed-bundle'
        with self.assertRaises(ValueError):
            fixtures.EVIDENCE.bundle(self.expected, self.expected, destination)
        self.assertFalse(destination.exists())


if __name__ == '__main__':
    unittest.main()
