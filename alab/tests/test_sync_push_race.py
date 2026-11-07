#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The real push loop retries only proven fast-forward races, with every merge gated."""

import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
REF = "HEAD:refs/heads/main"
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'


def rejected(reason="fetch first", ref=REF):
    return [1, f"To fixture\n!\t{ref}\t[rejected] ({reason})\nDone\n"]


class PushRaceTests(unittest.TestCase):
    def run_loop(self, *, pushes=None, tips=None, fetch_rc=0, forward_rc=0,
                 behind_rc=1, merge_rc=0, check_rc=0):
        # Keep the recording fixtures; no real push, checkout, build or cleanup is invoked.
        scratch = Path(tempfile.mkdtemp(prefix="alab-push-race-", suffix=".noindex"))
        fake = scratch / "git"
        fake.write_text(PYTHON_ENTRY + '''import json, os, sys
from pathlib import Path
root = Path(os.environ["RACE_FIXTURE"])
plan = json.loads((root / "plan.json").read_text())
state_file = root / "state.json"
state = json.loads(state_file.read_text()) if state_file.exists() else {"push": 0, "fetch": 0}
args = sys.argv[1:]
with (root / "git.jsonl").open("a") as log:
    log.write(json.dumps(args) + "\\n")
if args[:2] == ["-C", "candidate"]:
    args = args[2:]
rc, out = 0, ""
if args == ["rev-parse", "origin/main"]:
    out = plan["tips"][state["fetch"]] + "\\n"
elif args == ["push", "--porcelain", "origin", "HEAD:refs/heads/main"]:
    rc, out = plan["pushes"][state["push"]]
    state["push"] += 1
elif args == ["fetch", "--quiet", "origin"]:
    rc = plan["fetch_rc"]
    if rc == 0:
        state["fetch"] += 1
elif args[:2] == ["merge-base", "--is-ancestor"]:
    if args[2:] == ["origin/main", "HEAD"]:
        rc = plan["behind_rc"]
    else:
        assert args[2:] == [plan["tips"][state["fetch"] - 1], "origin/main"], args
        rc = plan["forward_rc"]
else:
    raise AssertionError(args)
state_file.write_text(json.dumps(state))
sys.stdout.write(out)
sys.exit(rc)
''')
        fake.chmod(0o700)
        source = (ROOT / "alab/sync.sh").read_text()
        begin = source.index("\npush_main() {") + 1
        end = source.index("\n}\n", begin) + 3
        # Execute the production function, replacing only external Git, sleep and unchanged
        # merge/gate actions with recording implementations and explicit failure outcomes.
        harness = scratch / "run.sh"
        harness.write_text("set -euo pipefail\n" + source[begin:end] + '''
wt=candidate
in_wt() { git -C "${wt}" "$@"; }
alab_die() { printf '%s\\n' "$*" >&2; exit 1; }
alab_say() { printf '%s\\n' "$*"; }
sleep() { printf 'sleep %s\\n' "$1" >>"${RACE_FIXTURE}/actions"; }
take_concurrent_main() {
  echo merge >>"${RACE_FIXTURE}/actions"
  [[ "${MERGE_RC}" == 0 ]] || exit "${MERGE_RC}"
}
run_check() {
  echo check >>"${RACE_FIXTURE}/actions"
  [[ "${CHECK_RC}" == 0 ]] || exit "${CHECK_RC}"
}
push_main
''')
        plan = dict(pushes=pushes or [rejected(), rejected(), [0, "landed\n"]],
                    tips=tips or ["old", "new1", "new2"], fetch_rc=fetch_rc,
                    forward_rc=forward_rc, behind_rc=behind_rc)
        (scratch / "plan.json").write_text(json.dumps(plan))
        env = dict(os.environ, PATH=str(scratch) + os.pathsep + os.environ["PATH"],
                   RACE_FIXTURE=str(scratch), MERGE_RC=str(merge_rc), CHECK_RC=str(check_rc))
        result = subprocess.run(["/bin/bash", str(harness)], env=env,
                                capture_output=True, text=True, timeout=60)
        commands = [json.loads(line) for line in (scratch / "git.jsonl").read_text().splitlines()]
        actions = (scratch / "actions").read_text().splitlines() if (scratch / "actions").exists() else []
        return result, commands, actions

    def test_repeated_races_merge_and_check_every_new_tip_before_landing(self):
        result, commands, actions = self.run_loop()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(actions, ["sleep 1", "merge", "check", "sleep 2", "merge", "check"])
        self.assertEqual(sum("push" in command for command in commands), 3)
        self.assertEqual(sum("fetch" in command for command in commands), 2)

    def test_non_fast_forward_rejection_also_retries_and_backoff_stays_capped(self):
        result, _, actions = self.run_loop(
            pushes=[rejected("non-fast-forward")] * 9 + [[0, "landed\n"]],
            tips=[f"tip{n}" for n in range(10)])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual([row for row in actions if row.startswith("sleep ")],
                         [f"sleep {n}" for n in [1, 2, 4, 8, 16, 32, 60, 60, 60]])
        self.assertEqual(actions.count("merge"), 9)
        self.assertEqual(actions.count("check"), 9)

    def test_auth_transport_hook_or_other_ref_rejection_never_retries(self):
        for report in ["", "fatal: Authentication failed\n",
                       f"!\t{REF}\t[remote rejected] (pre-receive hook declined)\n",
                       rejected(ref="HEAD:refs/heads/other")[1],
                       rejected(reason="stale info")[1]]:
            with self.subTest(report=report):
                result, commands, actions = self.run_loop(pushes=[[1, report]])
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertEqual(actions, [])
                self.assertFalse(any("fetch" in command for command in commands))

    def test_failed_fetch_no_advance_rewrite_or_covered_tip_never_retries(self):
        for options in [dict(fetch_rc=128), dict(tips=["old", "old"]),
                        dict(forward_rc=1), dict(behind_rc=0), dict(behind_rc=128)]:
            with self.subTest(options=options):
                result, commands, actions = self.run_loop(pushes=[rejected()], **options)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertEqual(actions, [])
                self.assertEqual(sum("push" in command for command in commands), 1)

    def test_conflict_or_failed_check_stops_before_another_push(self):
        for options, code, actions in [(dict(merge_rc=3), 3, ["sleep 1", "merge"]),
                                       (dict(check_rc=4), 4, ["sleep 1", "merge", "check"]),
                                       (dict(check_rc=6), 6, ["sleep 1", "merge", "check"])]:
            with self.subTest(options=options):
                result, commands, observed = self.run_loop(pushes=[rejected()], **options)
                self.assertEqual(result.returncode, code, result.stdout + result.stderr)
                self.assertEqual(observed, actions)
                self.assertEqual(sum("push" in command for command in commands), 1)


if __name__ == "__main__":
    unittest.main()
