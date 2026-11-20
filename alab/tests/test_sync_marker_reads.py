# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Unknown marker sets must not authorize a sync commit or a marker diagnosis."""

import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / "alab/sync.sh").read_text()
PATCH = "+++ b/file.rs\n+<<<<<<< ours\n"


def function(name):
    begin = SOURCE.index("\n" + name + "() {") + 1
    end = SOURCE.index("\n}\n", begin) + 3
    return SOURCE[begin:end]


class MarkerReadTests(unittest.TestCase):
    def shell(self, merging=True, left=0, right=0, left_patch="", right_patch="", processor="", conflict_rc=0, conflicts="", ref_rc=0, ref_output="target\n"):
        with tempfile.TemporaryDirectory(prefix="sync-marker-read-") as temporary:
            root = Path(temporary)
            (root / "state-msg").write_text("fixture sync\n")
            (root / "merge-head").write_text(ref_output)
            actions = root / "actions"
            definitions = '\n'.join(function(name) for name in (
                "new_markers", "shared_new_markers", "worktree_clean", "worktree_fully_staged", "write_stop", "check_current_main", "finish"))
            script = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / "alab/lib.sh")) + '\n'
                      + definitions + r'''
wt=candidate
mode=start
push=0
check=1
record() { printf '%s\n' "$1" >>"${ACTIONS}"; }
state_path() { printf '%s/state-%s\n' "${SCRATCH}" "$1"; }
merging() { [[ "${MERGING}" == 1 ]]; }
unlanded() { return 0; }
sync_target() { echo target; }
unmerged() { printf '%s' "${CONFLICTS}"; return "${CONFLICT_RC}"; }
apply_removed_paths() { :; }
keep_owned_paths() { :; }
commit_sync_merge() { record commit; }
exit_with() { printf '%s\n' "$2" >&2; exit "$1"; }
stop() { exit "$1"; }
in_wt() {
  while [[ "$1" == -c ]]; do shift 2; done
  case "$1" in
    rev-parse)
      if [[ "$*" == 'rev-parse --path-format=absolute --git-path MERGE_HEAD' ]]; then
        printf '%s/merge-head\n' "${SCRATCH}"
      else
        printf '%s' "${REF_OUTPUT}"; return "${REF_RC}"
      fi ;;
    status) record status ;;
    merge-base) record ancestry; return 0 ;;
    diff)
      case "$*" in
        *MERGE_HEAD*|*HEAD^2*) record right; printf '%s' "${RIGHT_PATCH}"; return "${RIGHT_RC}" ;;
        *) record left; printf '%s' "${LEFT_PATCH}"; return "${LEFT_RC}" ;;
      esac ;;
    *) exit 97 ;;
  esac
}
drift_check() { :; }
run_check() { record gate; }
take_concurrent_main() { record merge; }
comm() {
  [[ "${FAILED_PROCESSOR}" != comm ]] || return 128
  command comm "$@"
}
sort() {
  [[ "${FAILED_PROCESSOR}" != sort ]] || return 128
  command sort "$@"
}
finish --observed-merge
''')
            env = dict(os.environ, SCRATCH=str(root), ACTIONS=str(actions), MERGING=str(int(merging)),
                       LEFT_RC=str(left), RIGHT_RC=str(right), LEFT_PATCH=left_patch, RIGHT_PATCH=right_patch,
                       FAILED_PROCESSOR=processor, CONFLICT_RC=str(conflict_rc), CONFLICTS=conflicts, REF_RC=str(ref_rc), REF_OUTPUT=ref_output)
            result = subprocess.run(["/bin/bash", "-c", script], env=env,
                                    text=True, capture_output=True, timeout=15)
            return result, actions.read_text().splitlines() if actions.exists() else []

    def test_failed_merge_reference_reads_reach_no_recovery_advice_or_mutation(self):
        for merging in (True, False):
            for status in (2, 128, 137):
                for output in ("", "partial-id\n", "target\n"):
                    with self.subTest(merging=merging, status=status, output=output):
                        result, actions = self.shell(merging=merging, ref_rc=status, ref_output=output)
                        self.assertEqual(result.returncode, 1, result.stderr)
                        self.assertIn("cannot read sync merge", result.stderr)
                        self.assertEqual(actions, [])
                        self.assertNotIn("--abort", result.stderr)
                        self.assertNotIn("git reset", result.stderr)

    def test_known_missing_second_parent_retains_non_head_merge_advice(self):
        result, actions = self.shell(merging=False, ref_rc=1, ref_output="")
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertEqual(actions, ["ancestry"])
        self.assertIn("git reset --soft", result.stderr)

    def test_failed_conflict_reads_stop_before_commit_or_marker_checks(self):
        for conflicts in ("", "partial.rs\n"):
            with self.subTest(conflicts=conflicts):
                result, actions = self.shell(conflict_rc=128, conflicts=conflicts)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("cannot read unresolved paths", result.stderr)
                self.assertNotIn("commit", actions)
                self.assertNotIn("gate", actions)
                self.assertNotIn("left", actions)
                self.assertNotIn("right", actions)

    def test_failed_empty_or_partial_marker_reads_stop_before_commit(self):
        for merging in (True, False):
            for side in ("left", "right"):
                for partial in ("", PATCH):
                    with self.subTest(merging=merging, side=side, partial=partial):
                        injected = {side: 128, "left_patch": PATCH, "right_patch": PATCH}
                        injected[side + "_patch"] = partial
                        result, actions = self.shell(merging=merging, **injected)
                        self.assertEqual(result.returncode, 1, result.stderr)
                        self.assertIn("cannot read", result.stderr)
                        self.assertNotIn("commit", actions)
                        self.assertNotIn("gate", actions)
                        self.assertNotIn("conflict markers are", result.stderr)

    def test_failed_marker_processing_reaches_no_commit_or_gate(self):
        for merging in (True, False):
            for processor in ("sort", "comm"):
                with self.subTest(merging=merging, processor=processor):
                    result, actions = self.shell(merging=merging, left_patch=PATCH,
                                                 right_patch=PATCH, processor=processor)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIn("cannot read", result.stderr)
                    self.assertNotIn("commit", actions)
                    self.assertNotIn("gate", actions)
                    self.assertNotIn("conflict markers are", result.stderr)

    def test_known_common_markers_retain_the_resolution_stop(self):
        for merging in (True, False):
            with self.subTest(merging=merging):
                result, actions = self.shell(merging=merging, left_patch=PATCH, right_patch=PATCH)
                self.assertEqual(result.returncode, 3, result.stderr)
                self.assertIn("conflict markers are", result.stderr)
                self.assertNotIn("commit", actions)

    def test_known_empty_or_one_sided_markers_still_allow_the_commit_and_gate(self):
        for merging in (True, False):
            for patches in ({}, {"left_patch": PATCH}, {"right_patch": PATCH}):
                with self.subTest(merging=merging, patches=patches):
                    result, actions = self.shell(merging=merging, **patches)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn("commit", actions)
                    self.assertIn("gate", actions)


if __name__ == "__main__":
    unittest.main()
