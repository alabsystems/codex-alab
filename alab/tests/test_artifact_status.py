# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import artifact_status as STATUS


class ArtifactStatusTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.first = self.package('first', 'a' * 40)
        self.second = self.package('second', 'b' * 40)

    def package(self, name, source):
        root = self.root / name
        (root / 'bin').mkdir(parents=True)
        marker = self.root / 'must-not-run'
        binary = root / 'bin/codex'
        binary.write_text('#!/bin/sh\ntouch "' + str(marker) + '"\n')
        binary.chmod(0o755)
        (root / 'alab-build.json').write_text(json.dumps({'source': source, 'version': '0.4.0'}))
        return root

    def mappings(self, *packages, wrong_inode=False, wrong_device=False):
        output = 'p123\n'
        for package in packages:
            binary = package / 'bin/codex'
            inode = binary.stat().st_ino + int(wrong_inode)
            device = binary.stat().st_dev + int(wrong_device)
            output += 'ftxt\nD' + hex(device) + '\ni' + str(inode) + '\nn' + str(binary.resolve()) + '\n'
        output += 'ftxt\ni123\nn/usr/lib/dyld\n'
        return output

    def test_explicit_package_and_pointer_are_inspected_without_execution(self):
        pointer = self.root / 'selected'
        pointer.symlink_to(self.first / 'bin/codex')
        result = STATUS.observe(self.second, pointer)
        self.assertEqual(result['packaged_sha'], 'b' * 40)
        self.assertEqual(result['selected_sha'], 'a' * 40)
        self.assertIsNone(result['running_sha'])
        self.assertFalse(result['errors'])
        self.assertFalse((self.root / 'must-not-run').exists())

    def test_running_source_is_independent_of_newer_package_and_selection(self):
        pointer = self.root / 'selected'
        pointer.symlink_to(self.second / 'bin/codex')
        with patch.object(STATUS.sys, 'platform', 'darwin'), patch.object(
                STATUS.subprocess, 'check_output', side_effect=[
                    'started', self.mappings(self.first), 'started']):
            result = STATUS.observe(self.second, pointer, 123)
        self.assertEqual(result['packaged_sha'], 'b' * 40)
        self.assertEqual(result['selected_sha'], 'b' * 40)
        self.assertEqual(result['running_sha'], 'a' * 40)
        self.assertEqual(result['observations']['running']['pid'], 123)
        self.assertFalse(result['errors'])

    def test_replaced_inode_and_changed_pid_identity_stay_unknown(self):
        for output, after in [(self.mappings(self.first, wrong_inode=True), 'started'),
                              (self.mappings(self.first), 'reused PID')]:
            with self.subTest(after=after), patch.object(STATUS.sys, 'platform', 'darwin'), patch.object(
                    STATUS.subprocess, 'check_output', side_effect=['started', output, after]):
                result = STATUS.observe(running_pid=123)
                self.assertIsNone(result['running_sha'])
                self.assertTrue(result['errors'])

    def test_same_inode_on_a_different_device_cannot_name_running_source(self):
        output = self.mappings(self.first, wrong_device=True)

        def answer(command, **_kwargs):
            if command[0] == '/bin/ps':
                return 'started'
            # Model the selected lsof fields, including the old command's missing D.
            return output if 'D' in command[-1] else ''.join(
                line for line in output.splitlines(keepends=True) if not line.startswith('D'))

        with patch.object(STATUS.sys, 'platform', 'darwin'), patch.object(
                STATUS.subprocess, 'check_output', side_effect=answer):
            result = STATUS.observe(running_pid=123)
        self.assertIsNone(result['running_sha'])
        self.assertTrue(result['errors'])

    def test_missing_malformed_or_duplicate_loaded_device_is_refused(self):
        output = self.mappings(self.first)
        device = 'D' + hex((self.first / 'bin/codex').stat().st_dev) + '\n'
        for replacement in ['', 'Dinvalid\n', device + device]:
            with self.subTest(replacement=replacement), patch.object(STATUS.sys, 'platform', 'darwin'), \
                    patch.object(STATUS.subprocess, 'check_output', side_effect=[
                        'started', output.replace(device, replacement, 1), 'started']):
                result = STATUS.observe(running_pid=123)
                self.assertIsNone(result['running_sha'])
                self.assertTrue(result['errors'])

    def test_missing_ambiguous_or_inode_less_process_image_cannot_qualify(self):
        outputs = ['p123\n', self.mappings(self.first, self.second),
                   'p123\nftxt\nn' + str((self.first / 'bin/codex').resolve()) + '\n']
        for output in outputs:
            with self.subTest(output=output), patch.object(STATUS.sys, 'platform', 'darwin'), patch.object(
                    STATUS.subprocess, 'check_output', side_effect=['started', output]):
                result = STATUS.observe(running_pid=123)
                self.assertIsNone(result['running_sha'])
                self.assertTrue(result['errors'])

    def test_wrong_process_or_malformed_file_records_cannot_name_running_source(self):
        output = self.mappings(self.first)
        image = str((self.first / 'bin/codex').resolve())
        inode = str((self.first / 'bin/codex').stat().st_ino)
        malformed = [
            output.replace('p123\n', 'p999\n'),
            output.removeprefix('p123\n'),
            output.replace('p123\n', 'p123\np123\n'),
            output + 'p999\n',
            output.replace('ftxt\n', '', 1),
            output.replace('ftxt\n', 'f7\n', 1),
            output.replace('i' + inode + '\n', 'i0\ni' + inode + '\n', 1),
            output.replace('n' + image + '\n', 'n/wrong/bin/codex\nn' + image + '\n', 1),
            output + 'unexpected\n',
            output.rstrip('\n'),
        ]
        for mapping in malformed:
            with self.subTest(mapping=mapping), patch.object(STATUS.sys, 'platform', 'darwin'), patch.object(
                    STATUS.subprocess, 'check_output', side_effect=['started', mapping, 'started']):
                result = STATUS.observe(running_pid=123)
                self.assertIsNone(result['running_sha'])
                self.assertNotIn('running', result['observations'])
                self.assertTrue(result['errors'])

    def test_multiple_text_records_preserve_one_unique_codex_mapping(self):
        # Other text mappings need not expose an inode; the selected codex must.
        output = self.mappings(self.first, self.first) + 'ftxt\nn/usr/lib/libfixture.dylib\n'
        with patch.object(STATUS.sys, 'platform', 'darwin'), patch.object(
                STATUS.subprocess, 'check_output', side_effect=['started', output, 'started']):
            result = STATUS.observe(running_pid=123)
        self.assertEqual(result['running_sha'], 'a' * 40)
        self.assertFalse(result['errors'])

    def test_invalid_pid_and_other_platform_refuse_process_inspection(self):
        with patch.object(STATUS.subprocess, 'check_output') as command:
            for pid in [0, -1, True]:
                self.assertTrue(STATUS.observe(running_pid=pid)['errors'])
            with patch.object(STATUS.sys, 'platform', 'linux'):
                self.assertTrue(STATUS.observe(running_pid=123)['errors'])
            command.assert_not_called()

    def test_missing_pointer_and_incomplete_manifest_do_not_invent_source(self):
        self.assertTrue(STATUS.observe(selected_executable=self.root / 'missing')['errors'])
        manifest = self.first / 'alab-build.json'
        for value in [{}, {'source': 'short', 'version': '0.4'},
                      {'source': 'a' * 40, 'version': None}, []]:
            manifest.write_text(json.dumps(value))
            result = STATUS.observe(package=self.first)
            self.assertIsNone(result['packaged_sha'])
            self.assertTrue(result['errors'])

    def test_escaping_or_nonexecutable_package_image_is_refused(self):
        image = self.first / 'bin/codex'
        image.chmod(0o644)
        self.assertTrue(STATUS.observe(package=self.first)['errors'])
        image.unlink()
        image.symlink_to(self.second / 'bin/codex')
        self.assertTrue(STATUS.observe(package=self.first)['errors'])

    def test_linked_manifest_cannot_supply_another_packages_source(self):
        manifest = self.first / 'alab-build.json'
        manifest.unlink()
        manifest.symlink_to(self.second / 'alab-build.json')
        with patch.object(STATUS.sys, 'platform', 'darwin'), patch.object(
                STATUS.subprocess, 'check_output', side_effect=[
                    'started', self.mappings(self.first), 'started']):
            result = STATUS.observe(self.first, self.first / 'bin/codex', 123)
        for stage in ('packaged', 'selected', 'running'):
            self.assertIsNone(result[stage + '_sha'])
            self.assertNotIn(stage, result['observations'])
        self.assertEqual(len(result['errors']), 3)
        self.assertFalse((self.root / 'must-not-run').exists())

    def test_package_changes_during_observation_cannot_mix_identities(self):
        for change in ['root', 'bin', 'manifest', 'image', 'image-content']:
            first = self.package('race-' + change, 'a' * 40)
            second = self.package('replacement-' + change, 'b' * 40)
            loads = STATUS.json.loads

            def changed_after_read(raw):
                value = loads(raw)
                if change == 'root':
                    first.rename(self.root / ('held-' + change))
                    second.rename(first)
                elif change == 'bin':
                    (first / 'bin').rename(first / 'old-bin')
                    (second / 'bin').rename(first / 'bin')
                elif change == 'manifest':
                    (second / 'alab-build.json').replace(first / 'alab-build.json')
                elif change == 'image':
                    (second / 'bin/codex').replace(first / 'bin/codex')
                else:
                    image = first / 'bin/codex'
                    image.write_bytes(image.read_bytes() + b'\nchanged\n')
                return value

            with self.subTest(change=change):
                with patch.object(STATUS.json, 'loads', side_effect=changed_after_read):
                    result = STATUS.observe(package=first)
                self.assertIsNone(result['packaged_sha'])
                self.assertNotIn('packaged', result['observations'])
                self.assertTrue(result['errors'])

    def test_manifest_change_during_bounded_read_is_refused(self):
        pread = os.pread
        changed = False
        manifest = self.first / 'alab-build.json'

        def changed_while_reading(fd, count, offset):
            nonlocal changed
            data = pread(fd, count, offset)
            if not changed:
                changed = True
                manifest.write_bytes(manifest.read_bytes() + b' ')
            return data

        with patch.object(STATUS.os, 'pread', side_effect=changed_while_reading):
            result = STATUS.observe(package=self.first)
        self.assertTrue(changed)
        self.assertIsNone(result['packaged_sha'])
        self.assertTrue(result['errors'])

    def test_oversized_manifest_is_refused(self):
        manifest = self.first / 'alab-build.json'
        manifest.write_bytes(b' ' * (128 * 1024) + manifest.read_bytes())
        result = STATUS.observe(package=self.first)
        self.assertIsNone(result['packaged_sha'])
        self.assertTrue(result['errors'])

    @unittest.skipUnless(hasattr(os, 'mkfifo'), 'requires POSIX special files')
    def test_fifo_manifest_is_refused_without_waiting_for_a_writer(self):
        manifest = self.first / 'alab-build.json'
        manifest.unlink()
        os.mkfifo(manifest)
        code = ('import json,sys; sys.path.insert(0,sys.argv[1]); import artifact_status; '
                'print(json.dumps(artifact_status.observe(package=sys.argv[2])))')
        child = subprocess.run([sys.executable, '-I', '-B', '-c', code,
                                str(Path(STATUS.__file__).parent), str(self.first)],
                               capture_output=True, text=True, check=True, timeout=5)
        result = json.loads(child.stdout)
        self.assertIsNone(result['packaged_sha'])
        self.assertTrue(result['errors'])

    def test_no_inputs_perform_no_process_or_package_inspection(self):
        with patch.object(STATUS.subprocess, 'check_output') as command:
            result = STATUS.observe()
        self.assertEqual(result['observations'], {})
        self.assertEqual(result['errors'], [])
        self.assertIsNone(result['running_sha'])
        command.assert_not_called()

    def test_opaque_command_shim_is_not_executed_or_treated_as_selection(self):
        directory = self.root / 'dispatch/bin'
        directory.mkdir(parents=True)
        shim = directory / 'codex'
        marker = self.root / 'shim-ran'
        shim.write_text('#!/bin/sh\ntouch "' + str(marker) + '"\n')
        shim.chmod(0o755)
        result = STATUS.observe(package=self.first, selected_executable=shim)
        self.assertEqual(result['packaged_sha'], 'a' * 40)
        self.assertIsNone(result['selected_sha'])
        self.assertTrue(result['errors'])
        self.assertFalse(marker.exists())


if __name__ == '__main__':
    unittest.main()
