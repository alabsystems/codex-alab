# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Unreadable sync targets are errors, not proof that recovery state was lost."""

from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / 'alab/sync.sh').read_text()
BEGIN = SOURCE.index('\nsync_target() {') + 1
TARGET = SOURCE[BEGIN:SOURCE.index('\n}\n', BEGIN) + 3]


class TargetReadTests(unittest.TestCase):
    def shell(self, contents=b'target-commit\n', directory=False, path_error=False, partial=False, dangling=False):
        with tempfile.TemporaryDirectory(prefix='sync-target-read-') as temporary:
            root = Path(temporary)
            target = root / 'target'
            if dangling:
                target.symlink_to('missing-target')
            elif directory:
                target.mkdir()
            elif contents is not None:
                target.write_bytes(contents)
            definitions = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / 'alab/lib.sh')) + '\n'
                           + TARGET + '\nwt=candidate\nfixture_root=' + shlex.quote(str(root))
                           + '\npath_error=' + str(int(path_error)) + '\npartial=' + str(int(partial)) + '\n'
                           + r'''
state_path() {
  if [[ "$path_error" == 1 ]]; then
    [[ "$partial" == 0 ]] || echo "$fixture_root/target"
    return 128
  fi
  echo "$fixture_root/$1"
}
merging() { return 0; }
exit_with() { printf '%s\n' "$2" >&2; exit "$1"; }
value="$(sync_target)"
printf '%s' "$value"
''')
            return subprocess.run(['/bin/bash', '-c', definitions], text=True,
                                  capture_output=True, timeout=10)

    def test_failed_path_lookup_is_not_missing_state(self):
        for partial in (False, True):
            with self.subTest(partial=partial):
                result = self.shell(path_error=True, partial=partial)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn('cannot read sync target', result.stderr)
                self.assertEqual(result.stdout, '')
                self.assertNotIn('--abort', result.stderr)

    def test_unreadable_or_empty_target_is_not_missing_state(self):
        for contents, directory in ((None, True), (b'', False), (b'\n', False),
                                    (b'target\nother\n', False), (b'target\0\n', False)):
            with self.subTest(contents=contents, directory=directory):
                result = self.shell(contents=contents, directory=directory)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn('cannot read sync target', result.stderr)
                self.assertEqual(result.stdout, '')
                self.assertNotIn('--abort', result.stderr)

    def test_dangling_target_is_not_missing_state(self):
        result = self.shell(contents=None, dangling=True)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('cannot read sync target', result.stderr)
        self.assertNotIn('--abort', result.stderr)

    def test_known_missing_target_keeps_existing_lost_state_diagnosis(self):
        result = self.shell(contents=None)
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn('lost its state file', result.stderr)

    def test_complete_target_is_returned(self):
        result = self.shell()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'target-commit')


if __name__ == '__main__':
    unittest.main()
