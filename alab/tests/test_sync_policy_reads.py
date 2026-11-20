# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""A failed or partial main-policy read grants no sync operation."""

import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / "alab/sync.sh").read_text()


def function(name):
    begin = SOURCE.index("\n" + name + "() {") + 1
    end = SOURCE.index("\n}\n", begin) + 3
    return SOURCE[begin:end]


class PolicyReadTests(unittest.TestCase):
    def shell(self, body, policy, status=0, output=""):
        with tempfile.TemporaryDirectory(prefix="sync-policy-") as temporary:
            root = Path(temporary)
            actions = root / "actions"
            definitions = '\n'.join(function(name) for name in (
                "specs", "apply_removed_paths", "keep_owned_paths", "note_watched_paths",
                "sync_merge_probe", "merges_unattended"))
            script = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / "alab/lib.sh"))) + '\n' + definitions + r'''
common_dir="${SCRATCH}"
wt="${SCRATCH}"
merge_cfg=(-c fixture=yes)
sync_disk_write() { "$@"; }
record() { printf '%s\n' "$1" >>"${ACTIONS}"; }
state_path() { printf '%s/state-%s\n' "${SCRATCH}" "$1"; }
unmerged() { record inspect-owned; }
in_wt() {
  case "$1" in
    ls-files) record enumerate; printf 'covered.rs\n' ;;
    rm) record remove ;;
    *) exit 98 ;;
  esac
}
git() {
  while [[ "$1" == -c ]]; do shift 2; done
  case "$1" in
    show)
      name="${2##*/}"
      record "read:${name}"
      if [[ "${name}" == "${POLICY_NAME}" ]]; then
        printf '%s' "${POLICY_OUT}"
        [[ "${POLICY_RC}" == 0 ]] || printf 'synthetic policy read failure\n' >&2
        return "${POLICY_RC}"
      fi ;;
    merge-tree) record probe; printf 'tree\ncovered.rs\n'; return 1 ;;
    read-tree) record index; : >"${GIT_INDEX_FILE}" ;;
    ls-files) record paths; printf 'covered.rs\n' ;;
    diff) record inspect-watched ;;
    *) exit 97 ;;
  esac
}
''' + 'export -f git record\n' + body
            env = dict(os.environ, SCRATCH=str(root), ACTIONS=str(actions), POLICY_NAME=policy,
                       POLICY_RC=str(status), POLICY_OUT=output)
            result = subprocess.run(["/bin/bash", "-c", script], env=env,
                                    text=True, capture_output=True, timeout=15)
            observed = actions.read_text().splitlines() if actions.exists() else []
            self.assertEqual(list(root.glob("alab-probe.*")), [])
            return result, observed

    def test_failed_or_partial_policy_reads_reach_no_sync_actions(self):
        cases = (("apply_removed_paths", "removed-paths"), ("keep_owned_paths", "owned-paths"),
                 ("note_watched_paths base target", "watched-paths"),
                 ("merges_unattended target", "removed-paths"),
                 ("merges_unattended target", "owned-paths"))
        for body, policy in cases:
            for output in ("", "covered.rs\n"):
                with self.subTest(body=body, policy=policy, output=output):
                    result, actions = self.shell(body, policy, status=128, output=output)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertEqual(result.stdout, "")
                    self.assertIn("cannot read", result.stderr)
                    self.assertTrue(all(action == "probe" or action.startswith("read:")
                                        for action in actions), actions)

    def test_empty_successful_policies_keep_existing_empty_behavior(self):
        for body, policy in (("apply_removed_paths", "removed-paths"),
                             ("keep_owned_paths", "owned-paths"),
                             ("note_watched_paths base target", "watched-paths")):
            with self.subTest(body=body):
                result, actions = self.shell(body, policy)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(actions, ["read:" + policy])
        result, actions = self.shell('if merges_unattended target; then echo clean; else echo resolver; fi',
                                     "owned-paths")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "resolver\n")
        self.assertEqual(actions, ["probe", "read:removed-paths", "read:owned-paths"])

    def test_successful_policy_still_allows_removed_path_handling(self):
        result, actions = self.shell("apply_removed_paths", "removed-paths", output="covered.rs\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(actions, ["read:removed-paths", "enumerate", "remove"])

    def test_raw_policy_reader_emits_no_partial_policy_on_git_failure(self):
        result, _ = self.shell("specs removed-paths", "removed-paths", status=128, output="covered.rs\n")
        self.assertEqual(result.returncode, 128, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertIn("synthetic policy read failure", result.stderr)


if __name__ == "__main__":
    unittest.main()
