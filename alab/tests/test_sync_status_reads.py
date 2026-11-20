# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Unreadable worktree status must not grant a clean/staged decision."""

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


class StatusReadTests(unittest.TestCase):
    def shell(self, body, status=0, output=""):
        with tempfile.TemporaryDirectory(prefix="sync-status-read-") as temporary:
            root = Path(temporary)
            actions = root / "actions"
            definitions = '\n'.join(function(name) for name in (
                "worktree_clean", "worktree_fully_staged", "write_stop", "check_current_main", "finish"))
            script = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / "alab/lib.sh")) + '\n'
                      + definitions + r'''
wt=candidate
mode=start
push=0
check=1
state_path() { printf '%s/state-%s\n' "${SCRATCH}" "$1"; }
record() { printf '%s\n' "$1" >>"${ACTIONS}"; }
merging() { return 1; }
unlanded() { return 0; }
sync_target() { echo target; }
ensure_worktree() { :; }
in_wt() {
  case "$1" in
    status) record status; printf '%s' "${STATUS_OUT}"; return "${STATUS_RC}" ;;
    rev-parse) echo fixture-revision ;;
    merge-base) record ancestry; return 0 ;;
    *) exit 97 ;;
  esac
}
exit_with() { printf '%s\n' "$2" >&2; exit "$1"; }
drift_check() { :; }
run_check() { record gate; }
take_concurrent_main() { record merge; }
push_main() { record publish; }
''' + body)
            env = dict(os.environ, SCRATCH=str(root), ACTIONS=str(actions),
                       STATUS_RC=str(status), STATUS_OUT=output)
            result = subprocess.run(["/bin/bash", "-c", script], env=env,
                                    text=True, capture_output=True, timeout=15)
            return result, actions.read_text().splitlines() if actions.exists() else []

    def test_finish_read_errors_stop_before_gate_or_dirty_work_advice(self):
        for output in ("", " M partial.rs\n"):
            with self.subTest(output=output):
                result, actions = self.shell("finish", status=128, output=output)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(actions, ["status"])
                self.assertIn("cannot read sync worktree status", result.stderr)
                self.assertNotIn("discard", result.stderr)

    def test_start_read_errors_do_not_grant_new_sync_work(self):
        begin = SOURCE.index('\nensure_worktree\n', SOURCE.index('# mode=start'))
        end = SOURCE.index('\nif [[ "${stepwise}" -eq 0 ]]', begin)
        for output in ("", " M partial.rs\n"):
            with self.subTest(output=output):
                body = 'unlanded() { return 1; }\n' + SOURCE[begin:end] + '\nrecord accepted\n'
                result, actions = self.shell(body, status=128, output=output)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(actions, ["status"])
                self.assertIn("cannot read sync worktree status", result.stderr)
                self.assertNotIn("--abort", result.stderr)


    def test_known_status_classifications_preserve_clean_and_fully_staged_answers(self):
        for output, clean, staged in (("", True, True), ("M  staged.rs\n", False, True),
                                      ("A  new.rs\nD  old.rs\n", False, True),
                                      (" M loose.rs\n", False, False),
                                      ("MM mixed.rs\n", False, False),
                                      ("?? new.rs\n", False, False)):
            for predicate, expected in (("worktree_clean", clean), ("worktree_fully_staged", staged)):
                with self.subTest(output=output, predicate=predicate):
                    result, actions = self.shell('if ' + predicate + '; then echo ready; else echo dirty; fi',
                                                 output=output)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout, "ready\n" if expected else "dirty\n")
                    self.assertEqual(actions, ["status"])

    def test_predicate_read_failures_are_fatal_in_boolean_contexts(self):
        for predicate in ("worktree_clean", "worktree_fully_staged"):
            for body in ('if ! ' + predicate + '; then record fallback; fi',
                         predicate + ' || record fallback'):
                with self.subTest(body=body):
                    result, actions = self.shell(body, status=128, output=" M partial.rs\n")
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertEqual(actions, ["status"])
                    self.assertIn("cannot read sync worktree status", result.stderr)

    def test_known_clean_finish_still_reaches_the_gate(self):
        result, actions = self.shell("finish")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(actions, ["status", "ancestry", "gate"])

    def test_known_dirty_finish_still_requires_saved_changes(self):
        result, actions = self.shell("finish", output=" M loose.rs\n")
        self.assertEqual(result.returncode, 4, result.stderr)
        self.assertEqual(actions, ["status"])
        self.assertIn("uncommitted changes", result.stderr)


if __name__ == "__main__":
    unittest.main()
