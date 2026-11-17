# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Resolve the entire recovery-state deletion set before deleting any files."""

from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / 'alab/sync.sh').read_text()
BEGIN = SOURCE.index('\nclear_state() {') + 1
CLEAR_STATE = SOURCE[BEGIN:SOURCE.index('\n}\n', BEGIN) + 3]
NAMES = ('msg', 'auto', 'hand', 'target', 'stop')


class StateCleanupTests(unittest.TestCase):
    def shell(self, failed='', partial=False, boolean=False):
        with tempfile.TemporaryDirectory(prefix='sync state cleanup ') as temporary:
            root = Path(temporary)
            names = ['.final', 'partial', *(f'state-{name}' for name in NAMES), 'state-msg.final']
            for name in names:
                (root / name).write_text('preserve ' + name)
            before = {name: (root / name).read_bytes() for name in names}
            script = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / 'alab/lib.sh')) + '\n'
                      + CLEAR_STATE + '\n'
                      + 'root=' + shlex.quote(str(root)) + '\n'
                      + 'failed=' + shlex.quote(failed) + '\n'
                      + 'partial=' + str(int(partial)) + '\n' + r'''
state_path() {
  if [[ "$1" == "$failed" ]]; then
    [[ "$partial" == 0 ]] || printf '%s/partial' "$root"
    echo 'fixture state path read failed' >&2
    return 128
  fi
  printf '%s/state-%s\n' "$root" "$1"
}
''' + ('if clear_state; then echo cleaned; else echo failed; fi' if boolean else 'clear_state'))
            result = subprocess.run(['/bin/bash', '-c', script], cwd=root, text=True,
                                    capture_output=True, timeout=10)
            after = {name: (root / name).read_bytes() for name in names if (root / name).exists()}
            return result, before, after

    def test_failed_path_reads_preserve_all_recovery_and_unrelated_files(self):
        for name in NAMES:
            for partial in (False, True):
                for boolean in (False, True):
                    with self.subTest(name=name, partial=partial, boolean=boolean):
                        result, before, after = self.shell(name, partial, boolean)
                        self.assertEqual(after, before, result.stderr)
                        self.assertEqual(result.returncode, 1, result.stderr)
                        self.assertIn('cannot locate sync recovery state', result.stderr)
                        self.assertNotIn('cleaned', result.stdout)

    def test_successful_path_reads_remove_only_complete_recovery_set(self):
        result, before, after = self.shell()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(after, {name: before[name] for name in ('.final', 'partial')})


if __name__ == '__main__':
    unittest.main()
