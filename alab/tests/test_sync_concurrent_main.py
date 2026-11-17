# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the production concurrent-main function at its ancestry boundary."""

import os
import shlex
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class ConcurrentMainTests(unittest.TestCase):
    def run_concurrent_main(self, ancestry=0, upstream="main-upstream", history=0):
        source = (ROOT / "alab/sync.sh").read_text()
        begin = source.index("\ntake_concurrent_main() {") + 1
        end = source.index("\n}\n", begin) + 3
        with tempfile.TemporaryDirectory(prefix="sync-ancestry-") as temporary:
            root = Path(temporary)
            harness = root / "run.sh"
            harness.write_text("set -euo pipefail\n. " + shlex.quote(str(ROOT / "alab/lib.sh")) + "\n" + source[begin:end] + r'''
wt=candidate
repo_root=source
record() { printf '%s\n' "$1" >>"${ACTIONS}"; }
sync_target() { printf 'sync-target\n'; }
alab_last_sync_upstream() {
  [[ "${HISTORY_RC}" == 0 ]] || return "${HISTORY_RC}"
  printf '%s\n' "${MAIN_UPSTREAM}"
}
git() {
  [[ "$*" == 'merge-base --is-ancestor main-upstream sync-target' ]] || exit 97
  record ancestry
  return "${ANCESTRY_RC}"
}
in_wt() {
  case "$1" in
    log) record own-log; printf '  local-fix preserve this fix-forward\n' ;;
    -c) record merge ;;
    commit) record commit ;;
    *) exit 98 ;;
  esac
}
reresolve_cargo_lock() { record resolve; }
alab_say() { printf '%s\n' "$*"; }
alab_die() { printf '%s\n' "$*" >&2; exit 1; }
exit_with() { printf '%s\n' "$2" >&2; exit "$1"; }
take_concurrent_main
''')
            actions = root / "actions"
            env = dict(os.environ, ACTIONS=str(actions), ANCESTRY_RC=str(ancestry),
                       MAIN_UPSTREAM=upstream, HISTORY_RC=str(history))
            result = subprocess.run(["/bin/bash", str(harness)], env=env,
                                    text=True, capture_output=True, timeout=15)
            return result, actions.read_text().splitlines() if actions.exists() else []

    def test_ancestry_read_errors_keep_the_sync_without_abort_advice(self):
        for status in (2, 128, 137):
            with self.subTest(status=status):
                result, actions = self.run_concurrent_main(ancestry=status)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(actions, ["ancestry"])
                self.assertIn("cannot read ancestry", result.stderr)
                self.assertNotIn("--abort", result.stderr)
                self.assertNotIn("main merged upstream", result.stderr)

    def test_proven_newer_upstream_retains_local_commit_advice(self):
        result, actions = self.run_concurrent_main(ancestry=1)
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertEqual(actions, ["ancestry", "own-log"])
        self.assertIn("--abort", result.stderr)
        self.assertIn("preserve this fix-forward", result.stderr)

    def test_existing_upstream_allows_normal_merge_and_resolution(self):
        result, actions = self.run_concurrent_main()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(actions, ["ancestry", "merge", "resolve", "commit"])

    def test_no_prior_sync_does_not_require_an_ancestry_query(self):
        result, actions = self.run_concurrent_main(upstream="")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(actions, ["merge", "resolve", "commit"])

    def test_failed_history_read_reaches_no_merge_or_ancestry_query(self):
        result, actions = self.run_concurrent_main(history=2)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(actions, [])
        self.assertIn("cannot read main's last upstream sync", result.stderr)


if __name__ == "__main__":
    unittest.main()
