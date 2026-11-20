# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Resolver staging requires a proven diff, not an unreadable index."""

import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / 'alab/sync.sh').read_text()
BEGIN = SOURCE.index('\nreresolve_cargo_lock() {') + 1
RESOLVE = SOURCE[BEGIN:SOURCE.index('\n}\n', BEGIN) + 3]


class LockChangeReadTests(unittest.TestCase):
    def shell(self, status, boolean=False):
        with tempfile.TemporaryDirectory(prefix='sync-lock-change-') as temporary:
            root = Path(temporary)
            (root / 'codex-rs').mkdir()
            for name in ('Cargo.toml', 'Cargo.lock'):
                (root / 'codex-rs' / name).write_text('fixture\n')
            record = root / 'auto'
            record.write_text('previous\n')
            script = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / 'alab/lib.sh')) + '\n'
                      + RESOLVE + '\n' + r'''
wt="$SCRATCH"
record() { echo "$1" >>"$SCRATCH/actions"; }
state_path() { echo "$SCRATCH/auto"; }
alab_project_run() { record resolver; }
exit_with() { exit "$1"; }
in_wt() {
  case "$1" in
    diff)
      record diff
      [[ "$DIFF_RC" -le 1 ]] || echo 'fixture diff read failed' >&2
      return "$DIFF_RC" ;;
    add) record stage ;;
    *) exit 97 ;;
  esac
}
drift_check() { record drift; }
''' + ('if reresolve_cargo_lock; then record accepted; fi' if boolean else 'reresolve_cargo_lock'))
            env = dict(os.environ, SCRATCH=str(root), DIFF_RC=str(status))
            result = subprocess.run(['/bin/bash', '-c', script], env=env, text=True,
                                    capture_output=True, timeout=10)
            return result, (root / 'actions').read_text().splitlines(), record.read_text()

    def test_unknown_diff_stops_before_staging_or_resolution_record(self):
        for status in (2, 128, 137):
            for boolean in (False, True):
                with self.subTest(status=status, boolean=boolean):
                    result, actions, record = self.shell(status, boolean)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIn('cannot read resolved lock changes', result.stderr)
                    self.assertIn('fixture diff read failed', result.stderr)
                    self.assertEqual(actions, ['resolver', 'diff'])
                    self.assertEqual(record, 'previous\n')

    def test_known_clean_and_changed_results_keep_existing_staging(self):
        for status in (0, 1):
            result, actions, record = self.shell(status)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(actions, ['resolver', 'diff'] + (['stage'] if status else []) + ['drift'])
            self.assertEqual(record, 'previous\n' + ('  codex-rs/Cargo.lock re-resolved\n' if status else ''))


if __name__ == '__main__':
    unittest.main()
