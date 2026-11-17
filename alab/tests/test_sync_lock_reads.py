# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Lock drift decisions require complete upstream/local package sets."""

import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / 'alab/sync.sh').read_text()
LOCK = '''[[package]]
name = "foo"
version = "1.0"
source = "registry+fixture"
checksum = "sum"
'''


def function(name):
    begin = SOURCE.index('\n' + name + '() {') + 1
    return SOURCE[begin:SOURCE.index('\n}\n', begin) + 3]


class LockReadTests(unittest.TestCase):
    def shell(self, tree_rc=0, show_rc=0, upstream=LOCK, parser='', vendor_rc=0,
              comm_rc=0, local=LOCK, present=True, own=''):
        with tempfile.TemporaryDirectory(prefix='sync-lock-read-') as temporary:
            root = Path(temporary)
            (root / 'codex-rs').mkdir()
            (root / 'codex-rs/Cargo.toml').write_text('[workspace]\n')
            if local is not None:
                (root / 'codex-rs/Cargo.lock').write_text('# local\n' + local)
            definitions = function('lock_packages').replace('lock_packages()', 'parse_lock_packages()', 1)
            definitions += '\n' + function('drift_check')
            script = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / 'alab/lib.sh')) + '\n'
                      + definitions + '\n' + r'''
wt="$SCRATCH"
cargo=targo
sync_target() { echo target; }
exit_with() { printf '%s\n' "$2" >&2; exit "$1"; }
vendored_packages() { printf '%s' "$OWN"; return "$VENDOR_RC"; }
in_wt() {
  case "$1" in
    cat-file) [[ "$PRESENT" == 1 ]] || return 128; return "$TREE_RC" ;;
    ls-tree) [[ "$PRESENT" == 0 ]] || echo codex-rs/Cargo.lock; return "$TREE_RC" ;;
    show) printf '%s' "$UPSTREAM"; return "$SHOW_RC" ;;
    *) exit 97 ;;
  esac
}
lock_packages() {
  local raw side=upstream
  raw="$(cat)"
  [[ "$raw" != '# local'* ]] || side=local
  printf '%s\n' "$raw" | parse_lock_packages
  [[ "$PARSER" != "$side" ]] || return 128
}
comm() { [[ "$COMM_RC" == 0 ]] || return "$COMM_RC"; command comm "$@"; }
drift_check
echo checked
''')
            env = dict(os.environ, SCRATCH=str(root), TREE_RC=str(tree_rc), SHOW_RC=str(show_rc),
                       UPSTREAM=upstream, PARSER=parser, VENDOR_RC=str(vendor_rc),
                       COMM_RC=str(comm_rc), PRESENT=str(int(present)), OWN=own)
            return subprocess.run(['/bin/bash', '-c', script], env=env, text=True,
                                  capture_output=True, timeout=10)

    def test_unknown_package_sets_cannot_become_a_drift_verdict(self):
        cases = ({'tree_rc': 128}, {'show_rc': 128, 'upstream': ''},
                 {'show_rc': 128}, {'parser': 'upstream'}, {'parser': 'local'},
                 {'vendor_rc': 128}, {'comm_rc': 128}, {'local': None})
        for injected in cases:
            with self.subTest(injected=injected):
                result = self.shell(**injected)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn('cannot read', result.stderr)
                self.assertNotIn('checked', result.stdout)
                self.assertNotIn('Put each back', result.stderr)

    def test_known_unchanged_lock_and_absent_upstream_lock_still_pass(self):
        for injected in ({}, {'present': False}, {'upstream': '', 'local': ''}, {'upstream': ''}):
            result = self.shell(**injected)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, 'checked\n')

    def test_known_drift_retains_existing_repair_advice(self):
        result = self.shell(local=LOCK.replace('foo', 'bar'))
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn('foo', result.stderr)
        self.assertIn('Put each back', result.stderr)

    def test_matching_vendor_identity_still_exempts_the_registry_row(self):
        result = self.shell(local=LOCK.replace('foo', 'bar'), own='"foo" "1.0" "sum"')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'checked\n')


if __name__ == '__main__':
    unittest.main()
