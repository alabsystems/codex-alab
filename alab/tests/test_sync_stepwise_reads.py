# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Failed probe/frontier reads must not become clean probes or usable plans."""

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


class StepwiseReadTests(unittest.TestCase):
    def shell(self, body, **overrides):
        with tempfile.TemporaryDirectory(prefix="stepwise-reads-") as temporary:
            root = Path(temporary)
            actions = root / "actions"
            script = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / "alab/lib.sh")) + '\n'
                      + function("merges_unattended") + '\n' + function("stepwise_plan") + r'''
merge_cfg=(-c fixture=yes)
common_dir="${SCRATCH}"
record() { printf '%s\n' "$1" >>"${ACTIONS}"; }
specs() { [[ "$1" != removed-paths ]] || printf '%s' "${SPECS}"; }
git() {
  while [[ "$1" == -c ]]; do shift 2; done
  case "$1" in
    merge-tree) record probe; printf '%s' "${PROBE_OUT}"; return "${PROBE_RC}" ;;
    read-tree) record index; : >"${GIT_INDEX_FILE}"; return "${INDEX_RC}" ;;
    ls-files) record paths; printf '%s' "${PATHS_OUT}"; return "${PATHS_RC}" ;;
    merge-base) record frontier; printf 'base\n'; return "${BASE_RC}" ;;
    rev-list) record range; printf '%s' "${RANGE_OUT}"; return "${RANGE_RC}" ;;
    rev-parse) printf 'before\n' ;;
    *) exit 97 ;;
  esac
}
''' + body)
            values = dict(PROBE_RC="1", PROBE_OUT="tree\ncovered.rs\n", INDEX_RC="0",
                          PATHS_RC="0", PATHS_OUT="covered.rs\n", SPECS="covered.rs\n",
                          BASE_RC="0", RANGE_RC="0", RANGE_OUT="c1\nc2\nc3\ntarget\n")
            values.update({key: str(value) for key, value in overrides.items()})
            env = dict(os.environ, SCRATCH=str(root), ACTIONS=str(actions), **values)
            result = subprocess.run(["/bin/bash", "-c", script], env=env,
                                    text=True, capture_output=True, timeout=15)
            observed = actions.read_text().splitlines() if actions.exists() else []
            self.assertEqual(list(root.glob("alab-probe.*.index")), [], "owned probe index leaked")
            return result, observed

    def test_probe_read_failures_do_not_report_clean_or_resolver_results(self):
        for injected in (dict(PROBE_RC=128), dict(INDEX_RC=128), dict(PATHS_RC=128)):
            with self.subTest(injected=injected):
                result, _ = self.shell('if merges_unattended target; then echo clean; else echo resolver; fi',
                                       **injected)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn("cannot", result.stderr)

    def test_known_probe_results_keep_clean_and_uncovered_conflict_paths(self):
        for injected, expected in ((dict(PROBE_RC=0), "clean"), ({}, "clean"),
                                   (dict(PROBE_OUT="tree\nuncovered.rs\n"), "resolver"),
                                   (dict(SPECS=""), "resolver")):
            with self.subTest(injected=injected):
                result, _ = self.shell('if merges_unattended target; then echo clean; else echo resolver; fi',
                                       **injected)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected + "\n")

    def test_failed_path_processing_cleans_the_index_without_a_probe_result(self):
        for command in ("sort", "comm"):
            with self.subTest(command=command):
                body = command + '() { return 128; }\nif merges_unattended target; then echo clean; else echo resolver; fi'
                result, _ = self.shell(body)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn("cannot", result.stderr)

    def test_native_full_range_probe_stops_planning_on_read_failure(self):
        for status, expected_status, expected in ((0, 0, "target|\n"), (128, 1, "")):
            with self.subTest(status=status):
                result, actions = self.shell('stepwise_plan target', PROBE_RC=status)
                self.assertEqual(result.returncode, expected_status, result.stderr)
                self.assertEqual(result.stdout, expected)
                self.assertEqual(actions, ["probe"])

    def planner(self, **overrides):
        return self.shell('merges_unattended() { return 1; }\nstepwise_plan target\n', **overrides)

    def test_frontier_and_partial_range_read_failures_emit_no_plan(self):
        for injected in (dict(BASE_RC=128), dict(RANGE_RC=128, RANGE_OUT="c1\n")):
            with self.subTest(injected=injected):
                result, _ = self.planner(**injected)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn("cannot", result.stderr)

    def test_an_empty_frontier_is_an_explicit_error(self):
        result, _ = self.planner(RANGE_OUT="")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("no upstream commits", result.stderr)

    def test_known_plans_keep_all_clean_all_conflict_and_binary_prefix_results(self):
        cases = (("merges_unattended() { return 0; }", "target|\n"),
                 ("merges_unattended() { return 1; }", "|c1\n"),
                 ('merges_unattended() { [[ "$1" == c1 || "$1" == c2 ]]; }', "c2|c3\n"))
        for probe, expected in cases:
            with self.subTest(probe=probe):
                result, _ = self.shell(probe + '\nstepwise_plan target\n')
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected)

    def test_caller_does_not_accept_a_partial_plan_from_a_failed_planner(self):
        begin = SOURCE.index('\n  before="$(git rev-parse origin/main)"', SOURCE.index('while ! alab_ancestor_check'))
        end = SOURCE.index('\n  if [[ -n "${prefix}" ]]', begin)
        body = ('target=target\nstepwise_plan() { printf "c2|c3\\n"; return 128; }\n'
                + SOURCE[begin:end] + '\nrecord accepted\n')
        result, actions = self.shell(body)
        self.assertEqual(result.returncode, 128, result.stderr)
        self.assertNotIn("accepted", actions)


class RealMergeProbeTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="real-merge-probe-")
        self.addCleanup(self.scratch.cleanup)
        self.repo = Path(self.scratch.name)
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1", LC_ALL="C",
                        GIT_AUTHOR_NAME="Probe test", GIT_AUTHOR_EMAIL="probe@example.invalid",
                        GIT_COMMITTER_NAME="Probe test", GIT_COMMITTER_EMAIL="probe@example.invalid")
        self.git("init", "-q", "-b", "main")
        (self.repo / "conflict.txt").write_text("base\n")
        self.commit("base")
        self.git("checkout", "-q", "-b", "upstream-work")
        (self.repo / "conflict.txt").write_text("upstream\n")
        self.upstream = self.commit("upstream")
        self.git("checkout", "-q", "main")
        (self.repo / "conflict.txt").write_text("local\n")
        self.local = self.commit("local")
        self.git("update-ref", "refs/remotes/origin/main", self.local)

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo, env=self.env,
                                       text=True, stderr=subprocess.PIPE, timeout=15).strip()

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)
        return self.git("rev-parse", "HEAD")

    def probe(self, target, allowed=False):
        index = self.repo / ".git/index"
        before = index.read_bytes()
        script = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / "alab/lib.sh")) + '\n'
                  + function("merges_unattended") + r'''
merge_cfg=(-c merge.conflictstyle=merge)
common_dir="$(git rev-parse --absolute-git-dir)"
specs() {
  if [[ "$1" == removed-paths && "${ALLOW_PATH}" == 1 ]]; then printf 'conflict.txt\n'; fi
}
if merges_unattended "${PROBE_TARGET}"; then echo clean; else echo resolver; fi
''')
        env = dict(self.env, ALLOW_PATH=str(int(allowed)), PROBE_TARGET=target)
        result = subprocess.run(["/bin/bash", "-c", script], cwd=self.repo, env=env,
                                text=True, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(index.read_bytes(), before, "probe changed the source index")
        self.assertEqual(self.git("status", "--porcelain"), "")
        self.assertEqual(list((self.repo / ".git").glob("alab-probe.*.index")), [])
        return result.stdout

    def test_native_conflict_requires_complete_allowed_path_coverage(self):
        self.assertEqual(self.probe(self.upstream), "resolver\n")
        self.assertEqual(self.probe(self.upstream, allowed=True), "clean\n")

    def test_native_integrated_target_is_clean_without_allowed_paths(self):
        self.assertEqual(self.probe(self.local), "clean\n")


if __name__ == "__main__":
    unittest.main()
