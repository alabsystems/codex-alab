# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Unknown ancestry must stop before source cleanup or recovery actions."""

import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class AncestryCheckTests(unittest.TestCase):
    def run_shell(self, body, status):
        with tempfile.TemporaryDirectory(prefix="sync-query-") as temporary:
            actions = Path(temporary) / "actions"
            script = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / "alab/lib.sh")) + '\n'
                      + r'''
record() { printf '%s\n' "$1" >>"${ACTIONS}"; }
git() {
  if [[ "$1" == merge-base && "$2" == --is-ancestor ]]; then
    record ancestry
    return "${ANCESTRY_RC}"
  fi
  case "$1" in
    merge-base) printf 'base\n' ;;
    rev-list) printf '1\n' ;;
    rev-parse) printf 'target\n' ;;
    log) printf 'existing target\n' ;;
    *) exit 97 ;;
  esac
}
in_wt() { record "$1"; exit 91; }
alab_say() { printf '%s\n' "$*"; }
''' + body)
            env = dict(os.environ, ACTIONS=str(actions), ANCESTRY_RC=str(status))
            result = subprocess.run(["/bin/bash", "-c", script], env=env,
                                    text=True, capture_output=True, timeout=15)
            return result, actions.read_text().splitlines() if actions.exists() else []

    def test_unknown_ancestry_never_reaches_sync_worktree_cleanup(self):
        source = (ROOT / "alab/sync.sh").read_text()
        begin = source.index("\nrun_sync() {") + 1
        end = source.index("\n}\n", begin) + 3
        for status in (2, 128, 137):
            with self.subTest(status=status):
                result, actions = self.run_shell(source[begin:end] + '\nrun_sync target\n', status)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(actions, ["ancestry"])
                self.assertIn("cannot read ancestry", result.stderr)

    def test_known_ancestry_still_selects_the_existing_sync_paths(self):
        source = (ROOT / "alab/sync.sh").read_text()
        begin = source.index("\nrun_sync() {") + 1
        end = source.index("\n}\n", begin) + 3
        for status, code, actions in ((0, 0, ["ancestry"]), (1, 91, ["ancestry", "clean"])):
            with self.subTest(status=status):
                result, observed = self.run_shell(source[begin:end] + '\nrun_sync target\n', status)
                self.assertEqual(result.returncode, code, result.stderr)
                self.assertEqual(observed, actions)

    def test_read_failures_are_fatal_in_shell_boolean_contexts(self):
        for body in (
            'if alab_ancestor_check git merge-base --is-ancestor a b; then record yes; else record no; fi',
            'if ! alab_ancestor_check git merge-base --is-ancestor a b; then record no; fi',
            'alab_ancestor_check git merge-base --is-ancestor a b || record fallback',
            'while ! alab_ancestor_check git merge-base --is-ancestor a b; do record retry; break; done',
        ):
            with self.subTest(body=body):
                result, actions = self.run_shell(body, 128)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(actions, ["ancestry"])
                self.assertIn("cannot read ancestry", result.stderr)


if __name__ == "__main__":
    unittest.main()
