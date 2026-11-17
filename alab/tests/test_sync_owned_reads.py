# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Unknown source-tree state cannot authorize an owned-path deletion."""

import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / 'alab/sync.sh').read_text()


def function(name):
    begin = SOURCE.index('\n' + name + '() {') + 1
    return SOURCE[begin:SOURCE.index('\n}\n', begin) + 3]


class OwnedReadTests(unittest.TestCase):
    def shell(self, side='', output='', ours=True, theirs=True, archive_rc=0, failed_stage=''):
        with tempfile.TemporaryDirectory(prefix='sync-owned-read-') as temporary:
            root = Path(temporary)
            archive = root / 'alab/upstream/owned.rs'
            archive.parent.mkdir(parents=True)
            archive.write_text('previous complete archive\n')
            archive.chmod(0o755)
            definitions = function('keep_owned_paths')
            if '\narchive_owned_path() {' in SOURCE:
                definitions += '\n' + function('archive_owned_path')
            if '\ntree_has_path() {' in SOURCE:
                definitions += '\n' + function('tree_has_path')
            script = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / 'alab/lib.sh')) + '\n'
                      + definitions + '\n' + r'''
wt="$SCRATCH"
record() { printf '%s\n' "$1" >>"$SCRATCH/actions"; }
state_path() { printf '%s/state-%s\n' "$SCRATCH" "$1"; }
specs() { echo owned.rs; }
unmerged() { echo owned.rs; }
in_wt() {
  case "$1" in
    cat-file|ls-tree)
      if [[ "$*" == *MERGE_HEAD* ]]; then source=theirs; else source=ours; fi
      if [[ "$source" == "$FAILED_SIDE" ]]; then
        printf '%s' "$PARTIAL_OUTPUT"
        echo 'fixture tree read failed' >&2
        return 128
      fi
      if [[ "$source" == ours ]]; then present="$OURS"; else present="$THEIRS"; fi
      if [[ "$1" == cat-file ]]; then [[ "$present" == 1 ]] || return 128
      elif [[ "$present" == 1 ]]; then printf 'owned.rs\n'; fi ;;
    show) record archive-read; echo archived; return "$ARCHIVE_RC" ;;
    add) record "add:${3}" ;;
    checkout) record checkout ;;
    rm) record "remove:${*: -1}" ;;
    *) exit 97 ;;
  esac
}
cp() { [[ "$FAILED_STAGE" != copy ]] || return 128; command cp "$@"; }
mv() { [[ "$FAILED_STAGE" != replace ]] || return 128; command mv "$@"; }
keep_owned_paths
''')
            env = dict(os.environ, SCRATCH=str(root), FAILED_SIDE=side, PARTIAL_OUTPUT=output,
                       OURS=str(int(ours)), THEIRS=str(int(theirs)), ARCHIVE_RC=str(archive_rc), FAILED_STAGE=failed_stage)
            result = subprocess.run(['/bin/bash', '-c', script], env=env, text=True,
                                    capture_output=True, timeout=10)
            actions = (root / 'actions').read_text().splitlines() if (root / 'actions').exists() else []
            if archive_rc or failed_stage:
                self.assertEqual(archive.read_text(), 'previous complete archive\n')
            self.assertEqual(sorted(path.name for path in archive.parent.iterdir()), ['owned.rs'])
            self.assertEqual(archive.stat().st_mode & 0o777, 0o755)
            return result, actions

    def test_failed_presence_reads_never_resolve_or_delete_the_conflict(self):
        for side in ('ours', 'theirs'):
            for output in ('', 'partial-entry\n'):
                with self.subTest(side=side, output=output):
                    result, actions = self.shell(side=side, output=output)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIn('cannot read owned path', result.stderr)
                    self.assertEqual(actions, [])

    def test_failed_archive_read_preserves_previous_copy_and_does_not_stage(self):
        result, actions = self.shell(archive_rc=128)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('cannot archive owned path', result.stderr)
        self.assertEqual(actions, ['archive-read'])

    def test_failed_archive_copy_or_replacement_preserves_the_previous_file(self):
        for stage in ('copy', 'replace'):
            with self.subTest(stage=stage):
                result, actions = self.shell(failed_stage=stage)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn('cannot archive owned path', result.stderr)
                self.assertEqual(actions, [] if stage == 'copy' else ['archive-read'])

    def test_real_tree_lookup_uses_literal_paths(self):
        with tempfile.TemporaryDirectory(prefix='sync-owned-tree-') as temporary:
            root = Path(temporary)
            subprocess.run(['git', 'init', '-q', temporary], check=True, capture_output=True)
            for name in ('owned.rs', 'owned*[x].rs'):
                (root / name).write_text('fixture\n')
            subprocess.run(['git', '-C', temporary, 'add', '.'], check=True, capture_output=True)
            subprocess.run(['git', '-C', temporary, '-c', 'user.name=Fixture', '-c',
                            'user.email=fixture@invalid', 'commit', '-qm', 'fixture'],
                           check=True, capture_output=True)
            definitions = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / 'alab/lib.sh')) + '\n'
                           + function('tree_has_path') + '\n'
                           + 'in_wt() { git -C ' + shlex.quote(temporary) + ' "$@"; }\n')
            for name, expected in (('owned.rs', 'present'), ('owned*[x].rs', 'present'),
                                   ('*.rs', 'absent'), ('missing.rs', 'absent')):
                with self.subTest(name=name):
                    body = ('if tree_has_path HEAD ' + shlex.quote(name)
                            + '; then echo present; else echo absent; fi')
                    result = subprocess.run(['/bin/bash', '-c', definitions + body],
                                            text=True, capture_output=True, timeout=10)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout, expected + '\n')

    def test_known_presence_and_deletion_keep_existing_resolution(self):
        for ours in (True, False):
            for theirs in (True, False):
                with self.subTest(ours=ours, theirs=theirs):
                    result, actions = self.shell(ours=ours, theirs=theirs)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn('checkout' if ours else 'remove:owned.rs', actions)
                    self.assertIn('archive-read' if theirs else 'remove:alab/upstream/owned.rs', actions)


if __name__ == '__main__':
    unittest.main()
