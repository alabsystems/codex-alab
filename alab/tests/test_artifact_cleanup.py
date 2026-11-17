# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Public artifact inspection controls using only this fixture's owned descriptors."""
import errno
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import artifact_status as STATUS


class RefusingNotes:
    def __init__(self, message, reporting_error):
        super().__init__(message)
        self.reporting_error = reporting_error

    def add_note(self, _note):
        raise self.reporting_error


class UnreportableValueError(RefusingNotes, ValueError):
    pass


class UnreportableOSError(RefusingNotes, OSError):
    pass


class UnreportableInterrupt(RefusingNotes, KeyboardInterrupt):
    pass


class OwnedDescriptors:
    def __init__(self, package, primary, fstat_index, open_index, close_faults):
        self.package = package
        self.primary = primary
        self.fstat_index = fstat_index
        self.open_index = open_index
        self.close_faults = close_faults
        self.acquired = []
        self.live = {}
        self.closed = []
        self.opens = []
        self.fstats = {}

    def open(self, path, flags, *, dir_fd=None):
        index = len(self.opens)
        expected = ((self.package, None), ('bin', self.acquired[0]),
                    ('alab-build.json', self.acquired[0]), ('codex', self.acquired[1])) \
            if len(self.acquired) >= 2 else None
        if index == 0:
            assert path == self.package and dir_fd is None
        elif index == 1:
            assert path == 'bin' and dir_fd == self.acquired[0]
        else:
            assert (path, dir_fd) == expected[index]
        if dir_fd is not None:
            assert dir_fd in self.live, 'open parent must be owned'
        self.opens.append((path, flags, dir_fd))
        if index == self.open_index:
            raise self.primary
        fd = os.open(path, flags, dir_fd=dir_fd)
        assert fd not in self.live
        self.acquired.append(fd)
        self.live[fd] = index
        return fd

    def fstat(self, fd):
        assert fd in self.live, 'fstat descriptor must be owned'
        index = self.live[fd]
        self.fstats[index] = self.fstats.get(index, 0) + 1
        if index == self.fstat_index and self.fstats[index] == 1:
            raise self.primary
        return os.fstat(fd)

    def close(self, fd):
        assert fd in self.live, 'close descriptor must be owned and not consumed'
        index = self.live.pop(fd)
        os.close(fd)
        self.closed.append(fd)
        if index in self.close_faults:
            raise self.close_faults[index]


class ArtifactCleanupTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.package = Path(self.scratch.name).resolve() / 'package'
        (self.package / 'bin').mkdir(parents=True)
        image = self.package / 'bin/codex'
        image.write_bytes(b'Owned inert fixture; never executed.\n')
        image.chmod(0o755)
        (self.package / 'alab-build.json').write_text(
            json.dumps({'source': 'a' * 40, 'version': 'fixture'}))
        self.ledgers = []

    def tearDown(self):
        leaked = []
        cleanup_failures = []
        for ledger in self.ledgers:
            for fd in list(ledger.live):
                leaked.append(fd)
                del ledger.live[fd]
                try:
                    os.close(fd)
                except BaseException as error:
                    cleanup_failures.append(error)
        for error in cleanup_failures:
            if not isinstance(error, Exception):
                raise error
        self.assertEqual(cleanup_failures, [])
        self.assertEqual(leaked, [], 'inspection leaked an owned descriptor')
        for ledger in self.ledgers:
            self.assertEqual(ledger.closed, list(reversed(ledger.acquired)))
            self.assertEqual(len(ledger.closed), len(set(ledger.closed)))

    def inspect(self, primary=None, *, fstat_index=None, open_index=None,
                close_faults=None, observation=False, close_return=None):
        ledger = OwnedDescriptors(self.package, primary, fstat_index, open_index,
                                  close_faults or {})
        self.ledgers.append(ledger)
        def close(fd):
            ledger.close(fd)
            return close_return

        proxy = SimpleNamespace(**{**vars(os), 'open': ledger.open,
                                   'fstat': ledger.fstat, 'close': close})
        read = STATUS.regular_fd_bytes

        def read_manifest(fd, label):
            self.assertIn(fd, ledger.live)
            if primary is not None and fstat_index is None and open_index is None:
                raise primary
            return read(fd, label)

        with patch.object(STATUS, 'os', proxy), \
                patch.object(STATUS, 'regular_fd_bytes', side_effect=read_manifest):
            if observation:
                return STATUS.observe(package=self.package)
            return STATUS.package_identity(self.package)

    def assert_primary(self, primary, **faults):
        with self.assertRaises(BaseException) as caught:
            self.inspect(primary, **faults)
        self.assertIs(caught.exception, primary)
        return caught.exception

    def eio(self, index):
        return OSError(errno.EIO, 'controlled close fault #' + str(index))

    def test_success_closes_each_descriptor_once_and_retains_open_guards(self):
        result = self.inspect(observation=True)
        self.assertEqual(result['packaged_sha'], 'a' * 40)
        self.assertEqual(result['errors'], [])
        required = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
        ledger = self.ledgers[-1]
        self.assertEqual(len(ledger.acquired), 4)
        for index, (_path, flags, _parent) in enumerate(ledger.opens):
            self.assertEqual(flags, required | (os.O_DIRECTORY if index < 2 else 0))

    def test_successful_body_close_error_refuses_public_identity(self):
        result = self.inspect(close_faults={3: self.eio(3)}, observation=True)
        self.assertIsNone(result['packaged_sha'])
        self.assertNotIn('packaged', result['observations'])
        self.assertTrue(any('controlled close fault #3' in error for error in result['errors']))

    def test_successful_body_multiple_close_errors_preserve_first_error(self):
        first = self.eio(3)
        with self.assertRaises(BaseException) as caught:
            self.inspect(close_faults={3: first, 2: self.eio(2), 0: self.eio(0)})
        self.assertIs(caught.exception, first)

    def test_read_keyboard_interrupt_survives_all_ordinary_close_errors(self):
        primary = KeyboardInterrupt('primary read cancellation')
        faults = {index: self.eio(index) for index in range(4)}
        self.assert_primary(primary, close_faults=faults, observation=True)
        for fault in faults.values():
            self.assertIn(f'artifact descriptor cleanup failed: {fault!r}', primary.__notes__)

    def test_read_system_exit_survives_ordinary_close_error(self):
        self.assert_primary(SystemExit(47), close_faults={3: self.eio(3)}, observation=True)

    def test_truthy_close_result_cannot_suppress_pending_cancellation(self):
        self.assert_primary(KeyboardInterrupt('primary read cancellation'), close_return=object())

    def test_pending_cancellation_beats_later_cleanup_cancellation(self):
        primary = KeyboardInterrupt('first pending cancellation')
        self.assert_primary(primary, close_faults={3: SystemExit(47), 2: self.eio(2)})

    def test_ordinary_read_error_preserves_identity_across_cleanup_errors(self):
        primary = ValueError('primary metadata refusal')
        faults = {3: self.eio(3), 1: self.eio(1)}
        self.assert_primary(primary, close_faults=faults)
        for fault in faults.values():
            self.assertIn(f'artifact descriptor cleanup failed: {fault!r}', primary.__notes__)

    def test_public_observation_reports_primary_ordinary_failure(self):
        cleanup = self.eio(3)
        result = self.inspect(ValueError('primary metadata refusal'),
                              close_faults={3: cleanup}, observation=True)
        self.assertIsNone(result['packaged_sha'])
        self.assertEqual(result['errors'], [
            'packaged: primary metadata refusal',
            'packaged note: artifact descriptor cleanup failed: ' + repr(cleanup),
        ])

    def test_unhandled_ordinary_read_error_is_not_converted_to_packaged_error(self):
        self.assert_primary(RuntimeError('primary unhandled failure'),
                            close_faults={3: self.eio(3)}, observation=True)

    def test_cleanup_cancellation_promotes_over_ordinary_read_error(self):
        primary, cancellation = ValueError('primary metadata refusal'), KeyboardInterrupt('cleanup stop')
        later = self.eio(2)
        with self.assertRaises(BaseException) as caught:
            self.inspect(primary, close_faults={3: cancellation, 2: later}, observation=True)
        self.assertIs(caught.exception, cancellation)
        self.assertIs(caught.exception.__cause__, primary)
        self.assertIn(f'artifact observation failed: {primary!r}', cancellation.__notes__)
        self.assertIn(f'artifact descriptor cleanup failed: {later!r}', cancellation.__notes__)

    def test_successful_body_cleanup_cancellation_survives_later_close_error(self):
        cancellation = SystemExit(47)
        with self.assertRaises(BaseException) as caught:
            self.inspect(close_faults={3: cancellation, 2: self.eio(2)}, observation=True)
        self.assertIs(caught.exception, cancellation)

    def test_first_cleanup_cancellation_beats_later_cleanup_cancellation(self):
        primary, first = ValueError('primary metadata refusal'), SystemExit(47)
        with self.assertRaises(BaseException) as caught:
            self.inspect(primary, close_faults={3: first, 2: KeyboardInterrupt('later stop')})
        self.assertIs(caught.exception, first)
        self.assertIs(caught.exception.__cause__, primary)

    def test_acquisition_fstat_failure_closes_the_new_descriptor(self):
        for index in range(4):
            with self.subTest(index=index):
                primary = OSError(errno.EIO, 'primary acquisition fstat')
                self.assert_primary(primary, fstat_index=index)
                self.assertEqual(len(self.ledgers[-1].acquired), index + 1)

    def test_acquisition_fstat_cancellation_survives_cleanup_error(self):
        for primary in (KeyboardInterrupt('primary fstat cancellation'), SystemExit(47)):
            with self.subTest(cancellation=type(primary).__name__):
                self.assert_primary(primary, fstat_index=2, close_faults={2: self.eio(2)},
                                    observation=True)
                self.assertEqual(len(self.ledgers[-1].acquired), 3)

    def test_open_failure_releases_only_previously_acquired_descriptors(self):
        for index in range(4):
            with self.subTest(index=index):
                self.assert_primary(OSError(errno.EIO, 'primary open failure'), open_index=index)
                self.assertEqual(len(self.ledgers[-1].acquired), index)

    def test_open_failure_survives_errors_closing_earlier_descriptors(self):
        self.assert_primary(OSError(errno.EIO, 'primary open failure'), open_index=3,
                            close_faults={2: self.eio(2), 0: self.eio(0)})

    def test_open_cancellation_survives_cleanup_error(self):
        self.assert_primary(KeyboardInterrupt('primary open cancellation'), open_index=2,
                            close_faults={1: self.eio(1)}, observation=True)

    def test_ordinary_note_failure_preserves_selected_ordinary_primary(self):
        primary = UnreportableValueError('primary metadata refusal', RuntimeError('note failure'))
        self.assert_primary(primary, close_faults={3: self.eio(3)})

    def test_ordinary_note_failure_preserves_first_cleanup_error(self):
        first = UnreportableOSError('first cleanup error', RuntimeError('note failure'))
        with self.assertRaises(BaseException) as caught:
            self.inspect(close_faults={3: first, 2: self.eio(2)})
        self.assertIs(caught.exception, first)

    def test_note_fault_cannot_replace_pending_cancellation(self):
        for note_error in (RuntimeError('note failure'), SystemExit(47)):
            with self.subTest(reporting_error=type(note_error).__name__):
                primary = UnreportableInterrupt('primary read cancellation', note_error)
                self.assert_primary(primary, close_faults={3: self.eio(3), 1: self.eio(1)})

    def test_note_cancellation_promotes_over_selected_ordinary_primary(self):
        cancellation = SystemExit(47)
        primary = UnreportableValueError('primary metadata refusal', cancellation)
        with self.assertRaises(BaseException) as caught:
            self.inspect(primary, close_faults={3: self.eio(3), 2: self.eio(2)}, observation=True)
        self.assertIs(caught.exception, cancellation)
        self.assertIs(caught.exception.__cause__, primary)

    def test_note_fault_cannot_replace_selected_cleanup_cancellation(self):
        primary = ValueError('primary metadata refusal')
        cancellation = UnreportableInterrupt('cleanup stop', SystemExit(47))
        with self.assertRaises(BaseException) as caught:
            self.inspect(primary, close_faults={3: cancellation, 2: self.eio(2)})
        self.assertIs(caught.exception, cancellation)
        self.assertIs(caught.exception.__cause__, primary)

    def test_note_cancellation_promotes_over_first_ordinary_cleanup_error(self):
        cancellation = KeyboardInterrupt('reporting stop')
        first = UnreportableOSError('first cleanup error', cancellation)
        with self.assertRaises(BaseException) as caught:
            self.inspect(close_faults={3: first, 2: self.eio(2), 0: self.eio(0)})
        self.assertIs(caught.exception, cancellation)
        self.assertIs(caught.exception.__cause__, first)


if __name__ == '__main__':
    unittest.main()
