# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Resolve merge metadata before changing a sync worktree or its records."""

import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / 'alab/sync.sh').read_text()
BEGIN = SOURCE.index('\nrun_sync() {') + 1
RUN = SOURCE[BEGIN:SOURCE.index('\n}\n', BEGIN) + 3]


class MergeMetadataTests(unittest.TestCase):
    def shell(self, failure='', partial=False):
        with tempfile.TemporaryDirectory(prefix='sync-merge-metadata-') as temporary:
            root = Path(temporary)
            names = ('msg', 'auto', 'hand', 'target', 'stop')
            before = {name: 'previous-' + name for name in names}
            for name, value in before.items():
                (root / name).write_text(value)
            script = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / 'alab/lib.sh')) + '\n'
                      + RUN + '\n' + r'''
wt="$SCRATCH"
merge_cfg=(-c fixture=yes)
record() { echo "$1" >>"$SCRATCH/actions"; }
state_path() { echo "$SCRATCH/$1"; }
git() {
  case "$1" in
    merge-base) [[ "$2" != --is-ancestor ]] || return 1; key=base; value=base ;;
    rev-list) key=pending; value=2 ;;
    rev-parse) key=subject; value=short-target ;;
    log)
      case "$3" in
        --format=%H*) key=upstream; value=upstream-summary ;;
        *) key=progress; value=progress-summary ;;
      esac ;;
    *) exit 97 ;;
  esac
  record "read:$key"
  if [[ "$key" == "$FAILED_READ" ]]; then
    [[ "$PARTIAL" == 0 ]] || echo partial-metadata
    echo 'fixture metadata read failed' >&2
    return 128
  fi
  echo "$value"
}
in_wt() { while [[ "$1" == -c ]]; do shift 2; done; record "$1"; }
merging() { return 0; }
unmerged() { :; }
apply_removed_paths() { :; }
keep_owned_paths() { :; }
note_watched_paths() { record watched; }
finish() { record finish; }
run_sync target
''')
            env = dict(os.environ, SCRATCH=str(root), FAILED_READ=failure, PARTIAL=str(int(partial)))
            result = subprocess.run(['/bin/bash', '-c', script], env=env, text=True,
                                    capture_output=True, timeout=10)
            actions = (root / 'actions').read_text().splitlines()
            after = {name: (root / name).read_text() for name in names if (root / name).exists()}
            return result, actions, before, after

    def test_failed_metadata_reads_preserve_worktree_and_previous_state(self):
        for failure in ('base', 'pending', 'subject', 'upstream', 'progress'):
            for partial in (False, True):
                with self.subTest(failure=failure, partial=partial):
                    result, actions, before, after = self.shell(failure, partial)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIn('cannot read sync merge metadata', result.stderr)
                    self.assertIn('fixture metadata read failed', result.stderr)
                    self.assertTrue(all(action.startswith('read:') for action in actions), actions)
                    self.assertEqual(after, before)

    def test_complete_metadata_still_prepares_the_expected_merge_message(self):
        result, actions, _, after = self.shell()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(after['msg'], 'alab sync: merge upstream openai/codex@short-target\n\n'
                         'Upstream: upstream-summary\nNew upstream commits: 2\n')
        self.assertEqual(after['target'], 'target\n')
        self.assertNotIn('stop', after)
        self.assertEqual(actions[-2:], ['merge', 'finish'])
        self.assertIn('progress-summary', result.stderr)


if __name__ == '__main__':
    unittest.main()
