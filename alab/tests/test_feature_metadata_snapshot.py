# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Feature metadata public entries with real local Git and declared fixture logs."""
from contextlib import contextmanager, redirect_stderr, redirect_stdout
import copy
import errno
import io
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest import mock

import test_feature_gate as feature_fixture


EVIDENCE, GATE = feature_fixture.EVIDENCE, feature_fixture.GATE
PARTIAL = 8192


class FeatureMetadataSnapshotTests(unittest.TestCase):
    def setUp(self):
        self.world = feature_fixture.FeatureAdmissionTests()
        self.addCleanup(self.world.doCleanups)
        self.world.setUp()
        self.bundle = self.world.ready()
        self.path = self.bundle / 'record.json'
        self.complete = json.loads(self.path.read_text())
        commands = []
        original_popen = subprocess.Popen

        def local_git_only(command, *args, **kwargs):
            commands.append(command)
            self.assertIsInstance(command, (list, tuple))
            self.assertEqual(Path(command[0]).name, 'git')
            self.assertFalse(any(word in ('fetch', 'push', 'clone') for word in command))
            return original_popen(command, *args, **kwargs)

        process = mock.patch.object(subprocess, 'Popen', new=local_git_only)
        self.addCleanup(process.stop)
        process.start()

        def assert_local_git():
            self.assertTrue(all(isinstance(command, (list, tuple)) and Path(command[0]).name == 'git'
                                and not any(word in ('fetch', 'push', 'clone') for word in command)
                                for command in commands))

        self.addCleanup(assert_local_git)
        for module, name in ([(os, name) for name in ('kill', 'killpg', 'system') if hasattr(os, name)]
                             + [(socket, 'socket'), (threading, 'Thread')]):
            patcher = mock.patch.object(module, name, side_effect=AssertionError('forbidden ' + name))
            guard = patcher.start()
            self.addCleanup(patcher.stop)
            self.addCleanup(guard.assert_not_called)

    def invoke(self, entry, *, record=None, root=None, operation='validate'):
        with redirect_stdout(io.StringIO()) as stdout, redirect_stderr(io.StringIO()) as stderr:
            try:
                if entry == 'main':
                    arguments = [str(EVIDENCE.__file__), operation, '--repo', str(self.world.repo),
                                 '--source', self.world.source, '--upstream', self.world.target,
                                 '--base', self.world.base, '--record', str(self.path if record is None else record)]
                    if operation == 'validate':
                        arguments.extend(('--log-root', str(self.bundle if root is None else root)))
                    with mock.patch.object(sys, 'argv', arguments):
                        status = EVIDENCE.main()
                    return SimpleNamespace(error=None, accepted=status == 0, status=status,
                                           stdout=stdout.getvalue(), stderr=stderr.getvalue())
                result = GATE.admission(self.world.repo, self.world.base, bundle_root=root)
                return SimpleNamespace(error=None, accepted=not result['errors'], result=result)
            except BaseException as error:
                return SimpleNamespace(error=error, accepted=False)

    @contextmanager
    def physical_reads(self, entry, target, action):
        handles, active, retired, chunks = [], set(), [], []

        def observed(data):
            if data:
                chunks.append(data)
                self.assertLessEqual(len(chunks), 16, 'finite read budget exceeded')
                action(len(chunks))
            return data

        class Raw(io.RawIOBase):
            def __init__(self, raw):
                self.raw = raw
            def readable(self):
                return True
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
            def open(candidate, mode='r', buffering=-1, encoding=None, errors=None, newline=None):
                if candidate != target or mode != 'r':
                    return super().open(mode, buffering, encoding, errors, newline)
                raw = super().open('rb', buffering=0)
                handles.append((raw, raw.fileno()))
                limited = Raw(raw)
                try:
                    return io.TextIOWrapper(io.BufferedReader(limited, buffer_size=PARTIAL),
                                            encoding=encoding, errors=errors, newline=newline)
                except BaseException:
                    limited.close()
                    raise

        def opening(path, flags, *args, **kwargs):
            descriptor = os.open(path, flags, *args, **kwargs)
            if Path(path) == target:
                active.add(descriptor)
            return descriptor

        def read(descriptor, amount):
            if descriptor not in active:
                return os.read(descriptor, amount)
            return observed(os.read(descriptor, min(PARTIAL, amount)))

        def close(descriptor):
            os.close(descriptor)
            if descriptor in active:
                active.remove(descriptor)
                retired.append(descriptor)

        module = EVIDENCE if entry == 'main' else GATE
        reader = getattr(module, 'read_record', None)
        # Gate's imported function owns its feature module globals; another
        # fixture's separately loaded SPEC module is not an observation seam.
        shared = reader.__globals__['FILE_HASH'] if reader is not None else EVIDENCE.FILE_HASH
        globals_ = shared['file_bytes_snapshot'].__globals__
        namespace = SimpleNamespace(**vars(os))
        namespace.open, namespace.read, namespace.close = opening, read, close
        try:
            with mock.patch.object(module, 'Path', ReadingPath), mock.patch.dict(globals_, {'os': namespace}):
                yield chunks
        finally:
            file_states = [raw.closed and self.closed(descriptor) for raw, descriptor in handles]
            descriptor_states = [self.closed(descriptor) for descriptor in retired]
            leaked = tuple(active)
            for raw, _descriptor in handles:
                raw.close()
            for descriptor in leaked:
                os.close(descriptor)
            self.assertEqual(file_states, [True] * len(handles))
            self.assertTrue(all(descriptor_states))
            self.assertEqual(leaked, ())
            self.assertEqual(len(handles) + len(retired), 1, 'one metadata descriptor, without a hash/reopen')

    @staticmethod
    def closed(descriptor):
        try:
            os.fstat(descriptor)
        except OSError as error:
            return error.errno == errno.EBADF
        return False

    def test_both_complete_invalid_images_and_physical_mix_are_refused_at_both_entries(self):
        for entry in ('main', 'gate'):
            with self.subTest(entry=entry):
                self.path.write_text(json.dumps(self.complete) + '\n')
                self.assertTrue(self.invoke(entry).accepted)
                first = {name: self.complete[name] for name in
                         ('schema', 'source', 'upstream', 'inventory_sha256', 'base')}
                first['padding'] = 'P' * (4 * PARTIAL)
                first['features'] = copy.deepcopy(self.complete['features'])
                first['features'][0]['scenarios'][0]['checks'][0]['status'] = 1
                final = copy.deepcopy(first)
                final['schema'] = True
                final['features'][0]['scenarios'][0]['checks'][0]['status'] = 0
                first_text, final_text = (json.dumps(value) + '\n' for value in (first, final))
                first_text += ' ' * (len(final_text) - len(first_text))
                self.assertEqual(len(first_text), len(final_text))
                for data in (first_text, final_text):
                    self.path.write_text(data)
                    self.assertFalse(self.invoke(entry).accepted)
                self.path.write_text(first_text)
                before, rewrites = self.path.stat(), []

                def rewrite(index):
                    if index == 1:
                        self.path.write_text(final_text)
                        os.utime(self.path, ns=(before.st_atime_ns, before.st_mtime_ns + 1_000_000))
                        rewrites.append(True)

                with self.physical_reads(entry, self.path, rewrite) as chunks:
                    result = self.invoke(entry)
                mixed = json.loads(b''.join(chunks).decode('ascii'))
                self.assertIs(type(mixed['schema']), int)
                self.assertEqual(mixed['features'][0]['scenarios'][0]['checks'][0]['status'], 0)
                self.assertEqual(rewrites, [True])
                self.assertEqual(self.path.read_text(), final_text)
                self.assertEqual(self.path.stat().st_size, before.st_size)
                self.assertNotEqual(self.path.stat().st_mtime_ns, before.st_mtime_ns)
                if entry == 'main':
                    self.assertIsNone(result.error)
                    self.assertEqual(result.status, 1)
                    self.assertIn('changed while reading', result.stderr)
                else:
                    self.assertIsInstance(result.error, ValueError)
                    self.assertIn('changed while reading', str(result.error))
                self.assertFalse(result.accepted)

    def test_finite_metadata_growth_stops_at_initial_size_plus_one(self):
        for entry in ('main', 'gate'):
            with self.subTest(entry=entry):
                self.path.write_text(json.dumps(self.complete) + '\n')
                initial, appends = self.path.stat().st_size, []

                def grow(index):
                    if index <= 6:
                        with self.path.open('ab') as stream:
                            stream.write(b' ' * PARTIAL)
                        appends.append(True)

                with self.physical_reads(entry, self.path, grow) as chunks:
                    result = self.invoke(entry)
                self.assertTrue(appends)
                self.assertEqual(sum(map(len, chunks)), initial + 1)
                self.assertFalse(result.accepted)
                if entry == 'main':
                    self.assertEqual(result.status, 1)
                else:
                    self.assertIsInstance(result.error, ValueError)

    def test_cli_preserves_relative_arbitrary_external_leaf_and_parent_alias_inputs(self):
        temporary = tempfile.TemporaryDirectory(prefix='alab-external-feature-metadata-')
        self.addCleanup(temporary.cleanup)
        outside = Path(temporary.name).resolve()
        arbitrary = outside / 'different name.txt'
        arbitrary.write_text(json.dumps(self.complete) + '\n')
        leaf, parent = outside / 'leaf', outside / 'parent'
        leaf.symlink_to(arbitrary)
        parent.symlink_to(outside, target_is_directory=True)
        paths = (arbitrary, Path(os.path.relpath(arbitrary, Path.cwd())), leaf, parent / arbitrary.name)
        for path in paths:
            with self.subTest(path=path):
                self.assertTrue(self.invoke('main', record=path).accepted)
        with self.assertRaises(OSError):
            EVIDENCE.read_record(leaf)

    def test_gate_preserves_copied_bundles_parent_aliases_and_existing_exclusions(self):
        temporary = tempfile.TemporaryDirectory(prefix='alab-external-feature-bundle-')
        self.addCleanup(temporary.cleanup)
        outside = Path(temporary.name).resolve()
        copied = outside / 'copied bundle'
        shutil.copytree(self.bundle, copied)
        parent = outside / 'bundle parent alias'
        parent.symlink_to(copied.parent, target_is_directory=True)
        self.assertTrue(self.invoke('gate', root=copied).accepted)
        self.assertTrue(self.invoke('gate', root=parent / copied.name).accepted)
        alias = copied.with_name('bundle root alias')
        alias.symlink_to(copied, target_is_directory=True)
        self.assertIsInstance(self.invoke('gate', root=alias).error, ValueError)
        self.assertIsInstance(self.invoke('gate', root=Path('relative bundle')).error, ValueError)
        record = copied / 'record.json'
        retained = copied / 'record.retained'
        record.rename(retained)
        self.assertFalse(self.invoke('gate', root=copied).accepted)
        record.symlink_to(retained)
        self.assertIsInstance(self.invoke('gate', root=copied).error, ValueError)

    def test_default_text_json_semantics_remain_distinct_from_raw_log_json(self):
        value = {**self.complete, 'literal': 'naïve 雪'}
        text = json.dumps(value, ensure_ascii=False, indent=2)
        self.path.write_bytes((text.replace('\n', '\r\n') + '\r\n').encode('utf-8'))
        self.assertEqual(json.loads(self.path.read_text()), value)
        self.assertEqual(EVIDENCE.read_record(self.path), value)
        for entry in ('main', 'gate'):
            self.assertTrue(self.invoke(entry).accepted)
        for data in (b'\xff', b'{', b'\xef\xbb\xbf' + text.encode('utf-8'),
                     text.encode('utf-16'), text.encode('utf-32')):
            with self.subTest(prefix=data[:4]):
                self.path.write_bytes(data)
                with self.assertRaises(ValueError):
                    json.loads(self.path.read_text())
                with self.assertRaises(ValueError):
                    EVIDENCE.read_record(self.path)
                self.assertFalse(self.invoke('main').accepted)
                self.assertIsInstance(self.invoke('gate').error, ValueError)

    def test_reader_keeps_general_json_values_without_an_added_object_restriction(self):
        for value in (None, False, 0, [], 'literal'):
            with self.subTest(value=value):
                self.path.write_text(json.dumps(value) + '\n')
                self.assertEqual(EVIDENCE.read_record(self.path), value)
                result = self.invoke('main')
                self.assertIsNone(result.error)
                self.assertEqual(result.status, 1)
                self.assertEqual(result.stderr.strip(), 'record must be an object')
                result = self.invoke('gate')
                self.assertIsNone(result.error)
                self.assertEqual(result.result['errors'], ['record must be an object'])

    def test_snapshot_creation_still_writes_a_new_exclusive_record_without_reading_it(self):
        path = self.bundle / 'new pending name.json'
        with mock.patch.object(EVIDENCE, 'read_record', side_effect=AssertionError('snapshot input read forbidden')) as read:
            result = self.invoke('main', operation='snapshot', record=path)
        read.assert_not_called()
        self.assertTrue(result.accepted)
        pending = json.loads(path.read_text())
        self.assertEqual(pending, self.world.expected)
        self.assertTrue(all(not scenario['checks'] for feature in pending['features'] for scenario in feature['scenarios']))
        retained = path.read_bytes()
        self.assertFalse(self.invoke('main', operation='snapshot', record=path).accepted)
        self.assertEqual(path.read_bytes(), retained)

    def test_post_precheck_gate_leaf_swap_is_refused_by_the_direct_reader(self):
        donor, backup = self.bundle / 'donor.json', self.bundle / 'original.json'
        donor.write_bytes(self.path.read_bytes())
        reader = GATE.read_record.__globals__['FILE_HASH']['file_bytes_snapshot'].__globals__
        attempts = []

        def opening(candidate, flags, *args, **kwargs):
            self.assertEqual(Path(candidate), self.path)
            self.assertTrue(flags & os.O_NOFOLLOW)
            self.assertTrue(flags & os.O_NONBLOCK)
            attempts.append(flags)
            self.path.rename(backup)
            self.path.symlink_to(donor)
            return os.open(candidate, flags, *args, **kwargs)

        namespace = SimpleNamespace(**vars(os))
        namespace.open = opening
        try:
            with mock.patch.object(namespace, 'read', side_effect=AssertionError('read forbidden')) as read, \
                    mock.patch.object(namespace, 'close', side_effect=AssertionError('unowned close forbidden')) as close, \
                    mock.patch.dict(reader, {'os': namespace}):
                result = self.invoke('gate')
            self.assertIsInstance(result.error, OSError)
            self.assertEqual(len(attempts), 1)
            read.assert_not_called()
            close.assert_not_called()
            self.assertEqual(donor.read_bytes(), backup.read_bytes())
        finally:
            if backup.exists():
                self.path.unlink()
                backup.rename(self.path)

    def test_missing_snapshot_capability_refuses_before_open_at_both_entries(self):
        for entry in ('main', 'gate'):
            reader = EVIDENCE.read_record if entry == 'main' else GATE.read_record
            globals_ = reader.__globals__['FILE_HASH']['file_bytes_snapshot'].__globals__
            for missing in ('O_NOFOLLOW', 'O_NONBLOCK'):
                with self.subTest(entry=entry, missing=missing):
                    namespace = SimpleNamespace(**vars(os))
                    delattr(namespace, missing)
                    with mock.patch.object(namespace, 'open', side_effect=AssertionError('snapshot open forbidden')) as opened, \
                            mock.patch.dict(globals_, {'os': namespace}):
                        result = self.invoke(entry)
                    opened.assert_not_called()
                    self.assertFalse(result.accepted)
                    if entry == 'main':
                        self.assertIn('require O_NOFOLLOW and O_NONBLOCK', result.stderr)
                    else:
                        self.assertIsInstance(result.error, ValueError)

    def test_cli_regular_file_requirement_and_existing_gate_file_guard_are_distinct(self):
        backup = self.bundle / 'record.original'
        self.path.rename(backup)
        self.path.mkdir()
        try:
            result = self.invoke('main')
            self.assertIsNone(result.error)
            self.assertEqual(result.status, 1)
            self.assertIn('not a regular file', result.stderr)
            with mock.patch.object(GATE, 'read_record', side_effect=AssertionError('gate input read forbidden')) as read:
                result = self.invoke('gate')
            read.assert_not_called()
            self.assertFalse(result.accepted)
            self.assertIsNone(result.error)
            self.assertIn('complete exact-source feature bundle required', result.result['errors'][0])
        finally:
            self.path.rmdir()
            backup.rename(self.path)

    def test_cli_resolve_refuses_old_loop_errors_without_wrapping_reader_errors_or_controls(self):
        original_resolve = Path.resolve
        loop = RuntimeError('older pathlib loop')

        def resolve(path, *args, **kwargs):
            if path == self.path:
                raise loop
            return original_resolve(path, *args, **kwargs)

        previous_trace, mapped = sys.gettrace(), []

        def observe(frame, event, argument):
            if event == 'exception' and frame.f_code is EVIDENCE._run.__code__:
                if isinstance(argument[1], ValueError) and argument[1].__cause__ is loop:
                    mapped.append(argument[1])
            return observe

        try:
            sys.settrace(observe)
            with mock.patch.object(Path, 'resolve', new=resolve):
                result = self.invoke('main')
        finally:
            sys.settrace(previous_trace)
        self.assertEqual(result.status, 1)
        self.assertIn('feature record path cannot be resolved', result.stderr)
        self.assertTrue(mapped)
        self.assertIs(mapped[0].__cause__, loop)
        failure = RuntimeError('reader failure')
        with mock.patch.object(EVIDENCE, 'read_record', side_effect=failure):
            self.assertIs(self.invoke('main').error, failure)
        for primary in (KeyboardInterrupt('resolve'), SystemExit(0)):
            with self.subTest(primary=type(primary).__name__):
                def cancelled(path, *args, **kwargs):
                    if path == self.path:
                        raise primary
                    return original_resolve(path, *args, **kwargs)
                with mock.patch.object(Path, 'resolve', new=cancelled):
                    self.assertIs(self.invoke('main').error, primary)

    def test_large_regular_metadata_has_no_absolute_cap(self):
        value = {**self.complete, 'padding': 'P' * (2 * 1024 * 1024)}
        self.path.write_text(json.dumps(value) + '\n')
        self.assertGreater(self.path.stat().st_size, 2 * 1024 * 1024)
        for entry in ('main', 'gate'):
            self.assertTrue(self.invoke(entry).accepted)


if __name__ == '__main__':
    unittest.main()
