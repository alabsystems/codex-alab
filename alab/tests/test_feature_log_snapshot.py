# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Physical log snapshots; synthetic declarations do not claim test execution."""
from contextlib import contextmanager, ExitStack
import copy
import errno
import hashlib
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import feature_evidence as EVIDENCE

PART = 8192
SOURCE = 'a' * 40
GOOD = b'running 1 test\ntest ownership::cancel ... ok\n'
BAD = b'running 1 test\ntest unrelated::cancel ... ok\n'
OK = b'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n'
FAILED = OK.replace(b'ok. 1 passed; 0 failed', b'FAILED. 0 passed; 1 failed')

def block(prefix):
    return prefix + b'#' + b'P' * (PART - len(prefix) - 2) + b'\n'

WIDTH = max(len(OK), len(FAILED))
FIRST = block(GOOD) + FAILED.ljust(WIDTH)
LAST = block(BAD) + OK.ljust(WIDTH)
HEALTHY = FIRST[:PART] + LAST[PART:]


class FeatureLogSnapshotTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='alab-feature-log-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.expected = {'schema': 1, 'source': SOURCE, 'upstream': SOURCE,
                         'inventory_sha256': 'b' * 64, 'features': [{'id': 'ownership',
                         'scenarios': [{'description': 'Cancel during handoff.', 'checks': []}]}]}

    def observation(self, path, data, *, bundled=False):
        digest = hashlib.sha256(data).hexdigest()
        return {'source': SOURCE, 'command': ['fixture-command'], 'status': 0,
                'log': 'logs/' + digest + '.log' if bundled else str(path), 'log_sha256': digest,
                'tests': ['ownership::cancel'], 'reason': 'Synthetic validator fixture only.'}

    def record(self, observation):
        value = copy.deepcopy(self.expected)
        value['features'][0]['scenarios'][0]['checks'] = [observation]
        return value

    def location(self, name, data, bundled):
        root = self.root / name
        (root / 'logs').mkdir(parents=True)
        path = root / 'logs' / (hashlib.sha256(data).hexdigest() + '.log')
        return root if bundled else None, path, self.observation(path, data, bundled=bundled)

    def invoke(self, operation, observation, root=None):
        if operation == 'log_bytes':
            return EVIDENCE.log_bytes(observation, root)
        if operation == 'check':
            return EVIDENCE.check(observation, SOURCE, root)
        return EVIDENCE.validate(self.record(observation), self.expected, root)

    @contextmanager
    def physical_reads(self, target, action=None, on_open=None):
        real_open = Path.open
        handles, active, retired, chunks = [], set(), [], []
        owner = self

        def consume(data):
            if data:
                chunks.append(data)
                owner.assertLessEqual(len(chunks), 16, 'finite read budget exceeded')
                if action is not None:
                    action(len(chunks))
            return data

        class Raw(io.RawIOBase):
            def __init__(self, raw):
                self.raw = raw
            def readable(self):
                return True
            def readinto(self, destination):
                count = self.raw.readinto(memoryview(destination)[:PART])
                if count:
                    consume(bytes(destination[:count]))
                return count
            def close(self):
                try:
                    self.raw.close()
                finally:
                    super().close()

        class ReadingPath(type(Path())):
            def open(path, mode='r', *args, **kwargs):
                if path != target or mode != 'rb':
                    return real_open(path, mode, *args, **kwargs)
                raw = real_open(path, 'rb', buffering=0)
                handles.append((raw, raw.fileno()))
                if on_open is not None:
                    on_open()
                return io.BufferedReader(Raw(raw), buffer_size=PART)

        def opening(path, flags, *args, **kwargs):
            owner.assertTrue(flags & os.O_NONBLOCK, 'refuse before a FIFO can block')
            owner.assertTrue(flags & os.O_NOFOLLOW)
            descriptor = os.open(path, flags, *args, **kwargs)
            if Path(path) == target:
                active.add(descriptor)
                if on_open is not None:
                    on_open()
            return descriptor

        def read(descriptor, amount):
            data = os.read(descriptor, min(PART, amount))
            return consume(data) if descriptor in active else data

        def close(descriptor):
            os.close(descriptor)
            if descriptor in active:
                active.remove(descriptor)
                retired.append(descriptor)

        shared = getattr(EVIDENCE, 'FILE_HASH', None)
        globals_ = shared['file_bytes_snapshot'].__globals__ if shared is not None else None
        namespace = SimpleNamespace(**vars(os))
        namespace.open, namespace.read, namespace.close = opening, read, close
        guards = []
        with ExitStack() as owned:
            for module, name in ([(subprocess, name) for name in ('Popen', 'run', 'call', 'check_call', 'check_output')]
                                 + [(os, name) for name in ('fork', 'kill', 'killpg', 'system') if hasattr(os, name)]
                                 + [(socket, 'socket'), (threading, 'Thread')]):
                guards.append(owned.enter_context(mock.patch.object(module, name,
                    side_effect=AssertionError('forbidden ' + name))))
            owned.enter_context(mock.patch.object(EVIDENCE, 'Path', ReadingPath))
            if globals_ is not None:
                owned.enter_context(mock.patch.dict(globals_, {'os': namespace}))
            try:
                yield chunks
            finally:
                before = [raw.closed and self.closed(fd) for raw, fd in handles]
                closed_descriptors = [self.closed(fd) for fd in retired]
                leaked = tuple(active)
                for raw, _fd in handles:
                    raw.close()
                for descriptor in leaked:
                    os.close(descriptor)
                self.assertEqual(before, [True] * len(handles))
                self.assertTrue(all(closed_descriptors))
                self.assertEqual(leaked, ())
                self.assertEqual(len(handles) + len(retired), 1)
                for guard in guards:
                    guard.assert_not_called()

    @staticmethod
    def closed(descriptor):
        try:
            os.fstat(descriptor)
        except OSError as error:
            return error.errno == errno.EBADF
        return False

    def test_complete_bad_images_refuse_and_stable_healthy_bytes_pass(self):
        path = self.root / 'captured.log'
        for data in (FIRST, LAST):
            path.write_bytes(data)
            observation = self.observation(path, data)
            with self.subTest(image=data[:50]), self.assertRaises(ValueError):
                EVIDENCE.check(observation, SOURCE)
            self.assertTrue(EVIDENCE.validate(self.record(observation), self.expected))
        path.write_bytes(HEALTHY)
        observation = self.observation(path, HEALTHY)
        self.assertIsNone(EVIDENCE.check(observation, SOURCE))
        self.assertEqual(EVIDENCE.validate(self.record(observation), self.expected), [])

    def test_same_size_physical_rewrite_refuses_all_three_public_readers(self):
        for bundled in (False, True):
            for operation in ('log_bytes', 'check', 'validate'):
                with self.subTest(bundled=bundled, operation=operation):
                    root, path, observation = self.location(operation + str(bundled), HEALTHY, bundled)
                    path.write_bytes(FIRST)
                    before, rewrites = path.stat(), []
                    def rewrite(index):
                        if index == 1:
                            path.write_bytes(LAST)
                            os.utime(path, ns=(before.st_atime_ns, before.st_mtime_ns + 1_000_000))
                            rewrites.append(True)
                    with self.physical_reads(path, rewrite) as chunks:
                        if operation == 'validate':
                            self.assertTrue(any('changed while reading' in error for error in self.invoke(operation, observation, root)))
                        else:
                            with self.assertRaisesRegex(ValueError, 'changed while reading'):
                                self.invoke(operation, observation, root)
                    self.assertEqual(rewrites, [True])
                    self.assertEqual(b''.join(chunks), HEALTHY)
                    self.assertEqual(path.read_bytes(), LAST)
                    self.assertEqual(path.stat().st_size, before.st_size)
                    self.assertNotEqual(path.stat().st_mtime_ns, before.st_mtime_ns)

    def test_finite_growth_refuses_after_at_most_initial_size_plus_one(self):
        added = b'#' + b'G' * (PART - 2) + b'\n'
        for bundled in (False, True):
            for operation in ('log_bytes', 'check', 'validate'):
                with self.subTest(bundled=bundled, operation=operation):
                    final = HEALTHY + added * 6
                    root, path, observation = self.location('grow' + operation + str(bundled), final, bundled)
                    path.write_bytes(HEALTHY)
                    initial, appends = path.stat().st_size, []
                    def append(index):
                        if index <= 6:
                            with path.open('ab') as output:
                                output.write(added)
                            appends.append(True)
                    with self.physical_reads(path, append) as chunks:
                        if operation == 'validate':
                            self.assertTrue(any('changed while reading' in error for error in self.invoke(operation, observation, root)))
                        else:
                            with self.assertRaisesRegex(ValueError, 'changed while reading'):
                                self.invoke(operation, observation, root)
                    self.assertTrue(appends)
                    self.assertEqual(sum(map(len, chunks)), initial + 1)

    def test_external_leaf_and_parent_aliases_validate_and_export(self):
        outside = self.root / 'outside'
        outside.mkdir()
        target = outside / 'captured.log'
        target.write_bytes(HEALTHY)
        leaf, parent = self.root / 'leaf', self.root / 'parent'
        leaf.symlink_to(target)
        parent.symlink_to(outside, target_is_directory=True)
        for index, path in enumerate((target, leaf, parent / target.name)):
            observation = self.observation(path, HEALTHY)
            self.assertEqual(EVIDENCE.validate(self.record(observation), self.expected), [])
            destination = self.root / ('export' + str(index))
            EVIDENCE.bundle(self.record(observation), self.expected, destination)
            exported = json.loads((destination / 'record.json').read_text())
            alias = self.root / ('bundle-alias' + str(index))
            alias.symlink_to(destination, target_is_directory=True)
            self.assertEqual(EVIDENCE.validate(exported, self.expected, alias), [])

    def test_bundle_leaf_and_logs_directory_symlinks_remain_refused(self):
        root, path, observation = self.location('bundle', HEALTHY, True)
        path.write_bytes(HEALTHY)
        retained = self.root / 'retained.log'
        path.rename(retained)
        path.symlink_to(retained)
        self.assertTrue(EVIDENCE.validate(self.record(observation), self.expected, root))
        path.unlink()
        retained.rename(path)
        external = self.root / 'moved-logs'
        (root / 'logs').rename(external)
        (root / 'logs').symlink_to(external, target_is_directory=True)
        self.assertTrue(EVIDENCE.validate(self.record(observation), self.expected, root))

    def test_bundle_leaf_swap_after_precheck_is_refused_at_descriptor_open(self):
        root, path, observation = self.location('bundle-swap', HEALTHY, True)
        path.write_bytes(HEALTHY)
        donor, backup = self.root / 'donor.log', self.root / 'original.log'
        donor.write_bytes(HEALTHY)
        attempts = []

        def opening(candidate, flags, *args, **kwargs):
            self.assertEqual(Path(candidate), path)
            self.assertTrue(flags & os.O_NOFOLLOW)
            self.assertTrue(flags & os.O_NONBLOCK)
            attempts.append(flags)
            path.rename(backup)
            path.symlink_to(donor)
            return os.open(candidate, flags, *args, **kwargs)

        namespace = SimpleNamespace(**vars(os))
        namespace.open = opening
        globals_ = EVIDENCE.FILE_HASH['file_bytes_snapshot'].__globals__
        try:
            with mock.patch.object(namespace, 'read', side_effect=AssertionError('read forbidden')) as read, \
                    mock.patch.object(namespace, 'close', side_effect=AssertionError('unowned close forbidden')) as close, \
                    mock.patch.dict(globals_, {'os': namespace}), self.assertRaises(OSError):
                EVIDENCE.log_bytes(observation, root)
            self.assertEqual(len(attempts), 1)
            read.assert_not_called()
            close.assert_not_called()
            self.assertEqual(donor.read_bytes(), HEALTHY)
            self.assertEqual(backup.read_bytes(), HEALTHY)
        finally:
            if backup.exists():
                path.unlink()
                backup.rename(path)

    def test_raw_python_json_detection_and_strict_libtest_utf8_are_preserved(self):
        path = self.root / 'formats.log'
        result = {'schema': 1, 'kind': 'python-unittest-results', 'completed': True,
                  'tests_run': 1, 'tests': [{'id': 'fixture.test', 'finished': True, 'status': 'passed'}],
                  'exit_status': 0, 'output': 'naïve 雪\r\n'}
        text = json.dumps(result, ensure_ascii=False)
        for data in (text.encode(), b'\xef\xbb\xbf' + text.encode(), text.encode('utf-16'), text.encode('utf-32')):
            path.write_bytes(data)
            observation = self.observation(path, data)
            observation.update(format='python-unittest', tests=['fixture.test'])
            self.assertIsNone(EVIDENCE.check(observation, SOURCE))
            self.assertEqual(EVIDENCE.log_bytes(observation), data)
        for data in (b'\xff' + HEALTHY, b'\xef\xbb\xbf' + HEALTHY):
            path.write_bytes(data)
            with self.assertRaises((UnicodeError, ValueError)):
                EVIDENCE.check(self.observation(path, data), SOURCE)

    def test_large_regular_log_has_no_new_absolute_cap(self):
        data = GOOD + b'#' + b'P' * (2 * 1024 * 1024) + b'\n' + OK
        path = self.root / 'large.log'
        path.write_bytes(data)
        observation = self.observation(path, data)
        self.assertEqual(EVIDENCE.log_bytes(observation), data)
        self.assertEqual(EVIDENCE.validate(self.record(observation), self.expected), [])

    def test_preloaded_fifo_is_refused_without_a_thread_or_blocking_open(self):
        for bundled in (False, True):
            data = GOOD + OK
            root, path, observation = self.location('fifo' + str(bundled), data, bundled)
            os.mkfifo(path)
            keeper = os.open(path, os.O_RDWR | os.O_NONBLOCK)
            try:
                self.assertEqual(os.write(keeper, data), len(data))
                def release():
                    nonlocal keeper
                    os.close(keeper)
                    keeper = None
                with self.physical_reads(path, on_open=release), self.assertRaisesRegex(ValueError, 'not a regular file'):
                    EVIDENCE.log_bytes(observation, root)
            finally:
                if keeper is not None:
                    os.close(keeper)

    def test_unbundled_resolve_loop_refusal_preserves_cause_and_controls(self):
        path = self.root / 'loop'
        path.symlink_to(path)
        observation = self.observation(path, HEALTHY)
        self.assertTrue(EVIDENCE.validate(self.record(observation), self.expected))
        primary = RuntimeError('older pathlib loop')
        with mock.patch.object(Path, 'resolve', side_effect=primary), self.assertRaises(ValueError) as caught:
            EVIDENCE.log_bytes(observation)
        self.assertIs(caught.exception.__cause__, primary)
        with mock.patch.object(Path, 'resolve', side_effect=primary):
            self.assertTrue(EVIDENCE.validate(self.record(observation), self.expected))
        for primary in (KeyboardInterrupt('resolve'), SystemExit(0)):
            with mock.patch.object(Path, 'resolve', side_effect=primary), self.assertRaises(type(primary)) as caught:
                EVIDENCE.validate(self.record(observation), self.expected)
            self.assertIs(caught.exception, primary)

    def test_unavailable_snapshot_capability_is_an_ordinary_validation_refusal(self):
        path = self.root / 'capability.log'
        path.write_bytes(HEALTHY)
        observation = self.observation(path, HEALTHY)
        globals_ = EVIDENCE.FILE_HASH['file_bytes_snapshot'].__globals__
        for missing in ('O_NOFOLLOW', 'O_NONBLOCK'):
            namespace = SimpleNamespace(**vars(os))
            delattr(namespace, missing)
            with mock.patch.object(namespace, 'open', side_effect=AssertionError('open forbidden')) as opened, \
                    mock.patch.dict(globals_, {'os': namespace}):
                errors = EVIDENCE.validate(self.record(observation), self.expected)
            opened.assert_not_called()
            self.assertTrue(any('require O_NOFOLLOW and O_NONBLOCK' in error for error in errors))
