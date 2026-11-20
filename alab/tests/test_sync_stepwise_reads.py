# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Failed probe/frontier reads must not become clean probes or usable plans."""

import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest

from test_github import disk_fixture_cli


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
                      + function("sync_merge_probe") + '\n' + function("merges_unattended") + '\n'
                      + function("stepwise_plan") + r'''
merge_cfg=(-c fixture=yes)
common_dir="${SCRATCH}"
wt="${SCRATCH}"
sync_disk_write() { "$@"; }
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
export -f git record
if [[ "${PRIOR_INDEX:-}" == 1 ]]; then
  printf 'prior index' >"${SCRATCH}/alab-probe.$$.index"
  printf 'prior lock' >"${SCRATCH}/alab-probe.$$.index.lock"
fi
''' + body)
            values = dict(PROBE_RC="1", PROBE_OUT="tree\ncovered.rs\n", INDEX_RC="0",
                          PATHS_RC="0", PATHS_OUT="covered.rs\n", SPECS="covered.rs\n",
                          BASE_RC="0", RANGE_RC="0", RANGE_OUT="c1\nc2\nc3\ntarget\n")
            values.update({key: str(value) for key, value in overrides.items()})
            env = dict(os.environ, SCRATCH=str(root), ACTIONS=str(actions), **values)
            result = subprocess.run(["/bin/bash", "-c", script], env=env,
                                    text=True, capture_output=True, timeout=15)
            observed = actions.read_text().splitlines() if actions.exists() else []
            remnants = sorted(root.glob("alab-probe.*"))
            if values.get("PRIOR_INDEX") == "1":
                self.assertEqual([p.read_text() for p in remnants], ["prior index", "prior lock"])
            elif values.get("UNEXPECTED") == "1":
                self.assertEqual(len(remnants), 1)
                self.assertEqual([p.name for p in remnants[0].iterdir()], ["keep"])
                self.assertEqual((remnants[0] / "keep").read_text(), "keep")
            else:
                self.assertEqual(remnants, [], "owned probe index leaked")
            return result, observed

    def test_probe_read_failures_do_not_report_clean_or_resolver_results(self):
        for injected in (dict(PROBE_RC=128), dict(INDEX_RC=128), dict(PATHS_RC=128)):
            with self.subTest(injected=injected):
                result, _ = self.shell('if merges_unattended target; then echo clean; else echo resolver; fi',
                                       **injected)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn("cannot", result.stderr)

    def test_plan_refusals_and_signals_do_not_become_conflicts(self):
        for status in (1, 75, 143):
            for stopped in ("merge-tree", "read-tree"):
                body = r'''
sync_disk_write() {
  case "$1" in /bin/bash) action=merge-tree ;; *) action=read-tree ;; esac
  [[ "$action" != "$STOPPED" ]] || return "$STOP_STATUS"
  "$@"
}
stepwise_plan target
'''
                with self.subTest(status=status, stopped=stopped):
                    result, actions = self.shell(body, STOPPED=stopped, STOP_STATUS=status)
                    self.assertEqual(result.returncode, status, result.stderr)
                    self.assertEqual(result.stdout, "")
                    self.assertNotIn("frontier", actions)

    def test_capacity_stop_during_bisection_propagates(self):
        result, actions = self.shell(r'''
sync_disk_write() {
  case "$*" in *"origin/main c2") return 75 ;; esac
  "$@"
}
stepwise_plan target
''', SPECS="")
        self.assertEqual(result.returncode, 75, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(actions, ["probe", "frontier", "range"])

    def test_private_index_leaves_prior_pid_index_and_lock_alone(self):
        result, _ = self.shell("merges_unattended target", PRIOR_INDEX=1)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_cleanup_keeps_an_unexpected_occupant_and_the_resource_status(self):
        result, _ = self.shell(r'''
sync_disk_write() {
  if [[ "$1" == git ]]; then
    printf 'keep' >"${GIT_INDEX_FILE%/*}/keep"
    : >"${GIT_INDEX_FILE}.lock"
    return 75
  fi
  "$@"
}
stepwise_plan target
''', UNEXPECTED=1)
        self.assertEqual(result.returncode, 75, result.stderr)
        self.assertEqual(result.stdout, "")

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
                  + function("sync_merge_probe") + '\n' + function("merges_unattended") + r'''
merge_cfg=(-c merge.conflictstyle=merge)
wt="$PWD"
sync_disk_write() { "$@"; }
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
        self.assertEqual(list((self.repo / ".git").glob("alab-probe.*")), [])
        return result.stdout

    def test_native_conflict_requires_complete_allowed_path_coverage(self):
        self.assertEqual(self.probe(self.upstream), "resolver\n")
        self.assertEqual(self.probe(self.upstream, allowed=True), "clean\n")

    def test_native_integrated_target_is_clean_without_allowed_paths(self):
        self.assertEqual(self.probe(self.local), "clean\n")

    def test_directory_split_requires_a_resolver_and_accepts_explicit_continuation(self):
        # On one side, split a directory equally. On the other, add a new file to it.
        scripts = self.repo / "alab"
        scripts.mkdir()
        for policy in ("owned-paths", "removed-paths", "watched-paths"):
            (scripts / policy).write_text("unrelated-path\n")
        old = self.repo / "old"
        old.mkdir()
        for name in ("a", "b"):
            (old / name).write_text(name + "\n")
        self.commit("two files under old")
        self.git("switch", "-qc", "addition")
        (old / "new").write_text("new file\n")
        target = self.commit("add to old")
        self.git("switch", "-q", "main")
        for name in ("a", "b"):
            (self.repo / name).mkdir()
            self.git("mv", f"old/{name}", f"{name}/{name}")
        base = self.commit("split old equally")
        self.git("update-ref", "refs/remotes/origin/main", base)
        raw = subprocess.run(["git", "merge-tree", "--write-tree", "--name-only", "--no-messages",
                              "origin/main", target], cwd=self.repo, env=self.env, capture_output=True,
                             text=True, timeout=15)
        self.assertEqual(raw.returncode, 1, raw.stderr)
        self.assertRegex(raw.stdout, r"^(?:[0-9a-f]{40}|[0-9a-f]{64})\n\Z")
        self.assertEqual(self.probe(target, allowed=True), "resolver\n")

        worktree, sync = self.public_sync(target)
        # The primary checkout suppresses directory renames; the actual linked checkout
        # retains normal conflict detection. Replaying the primary checkout would pass.
        self.git("config", "extensions.worktreeConfig", "true")
        self.git("config", "--worktree", "merge.directoryRenames", "false")
        self.git("worktree", "add", "-q", "--detach", str(worktree), base)
        self.git("-C", str(worktree), "config", "--worktree", "merge.directoryRenames", "conflict")
        local = ("--no-check", "--no-push")
        first = sync(*local)
        self.assertEqual(first.returncode, 3, first.stderr)
        self.assertIn("without conflicting file paths", first.stderr)
        self.assertEqual(self.git("-C", str(worktree), "ls-files", "-u"), "")
        self.assertEqual(self.git("-C", str(worktree), "rev-parse", "HEAD"), base)
        self.assertEqual(self.git("-C", str(worktree), "rev-parse", "MERGE_HEAD"), target)
        status = sync("--status")
        self.assertEqual(status.returncode, 0, status.stderr)
        self.assertIn("without conflicting file paths", status.stdout)
        self.assertEqual(sync(*local).returncode, 5)
        # Simulate interruption between Git returning and the shell recording its decision.
        decision = Path(self.git("-C", str(worktree), "rev-parse", "--path-format=absolute",
                                 "--git-path", "alab-sync-stop"))
        decision.rename(decision.with_name("first-stop"))
        resumed = sync(*local)
        self.assertEqual(resumed.returncode, 3, resumed.stderr)
        self.assertIn("without conflicting file paths", decision.read_text())
        self.assertEqual(sync(*local).returncode, 5)
        # Git's MERGE_HEAD shorthand reads only its first line. Preserve every parent
        # until a person restores the observed two-head merge.
        head_file = Path(self.git("-C", str(worktree), "rev-parse", "--path-format=absolute",
                                  "--git-path", "MERGE_HEAD"))
        original_head = head_file.read_bytes()
        with head_file.open("ab") as f:
            f.write((base + "\n").encode())
        unsupported = sync("--continue", *local)
        self.assertEqual(unsupported.returncode, 3, unsupported.stderr)
        self.assertIn("exactly one incoming commit", unsupported.stderr)
        self.assertEqual(head_file.read_bytes(), original_head + (base + "\n").encode())
        self.assertEqual(self.git("-C", str(worktree), "rev-parse", "HEAD"), base)
        head_file.write_bytes(original_head)
        self.git("-C", str(worktree), "mv", "old/new", "a/new")
        continued = sync("--continue", *local)
        self.assertEqual(continued.returncode, 0, continued.stderr)
        self.assertEqual(self.git("-C", str(worktree), "rev-parse", "HEAD^2"), target)
        self.assertEqual(self.git("-C", str(worktree), "show", "HEAD:a/new"), "new file")

    def public_sync(self, target):
        # Pre-fetch real local remotes so source-only tests can run below the transfer floor.
        scripts = self.repo / "alab"
        upstream = self.repo / "input.git"
        self.git("clone", "--bare", "-q", str(self.repo), str(upstream))
        self.git("-C", str(upstream), "update-ref", "refs/heads/main", target)
        self.git("remote", "add", "upstream", str(upstream))
        self.git("remote", "add", "origin", str(self.repo))
        for remote in ("upstream", "origin"):
            self.git("fetch", "-q", remote)
        self.git("config", "rerere.enabled", "true")
        for name in ("sync.sh", "lib.sh", "sync_worktree.py", "github.py", "command_guard.py",
                     "cli_status.py", "file_hash.py"):
            shutil.copyfile(ROOT / "alab" / name, scripts / name)
        worktree = self.repo / ".git/alab-sync"
        env = {k: v for k, v in self.env.items() if not k.startswith("ALAB_")}
        env.update(ALAB_PYTHON=sys.executable, ALAB_SYNC_DIR=str(worktree))

        def sync(*args, **overrides):
            return subprocess.run(["/bin/bash", str(scripts / "sync.sh"), *args], cwd=self.repo,
                                  env=dict(env, **overrides), capture_output=True, text=True, timeout=60)
        return worktree, sync

    def prepared_detached_sync(self):
        scripts = self.repo / "alab"
        scripts.mkdir()
        for policy in ("owned-paths", "removed-paths", "watched-paths"):
            (scripts / policy).touch()
        base = self.commit("local sync policies")
        worktree, sync = self.public_sync(self.upstream)
        self.git("worktree", "add", "-q", "--detach", str(worktree), base)
        admin = Path(self.git("-C", str(worktree), "rev-parse", "--absolute-git-dir"))
        return worktree, sync, admin

    def test_recorded_incomplete_merge_preserves_a_clean_checkout_with_an_index_lock(self):
        worktree, sync, admin = self.prepared_detached_sync()
        self.git("-C", str(worktree), "switch", "-q", "-c", "alab-sync")
        lock = admin / "index.lock"
        lock.write_bytes(b"interrupt evidence")
        target = admin / "alab-sync-target"
        target.write_text(self.upstream + "\n")
        reported = sync("--status")
        self.assertEqual(reported.returncode, 0, reported.stderr)
        self.assertIn("interrupted", reported.stdout)
        for _ in range(2):
            retry = sync()
            self.assertEqual(retry.returncode, 1, retry.stderr)
            self.assertIn("index lock", retry.stderr)
            self.assertEqual(lock.read_bytes(), b"interrupt evidence")
            self.assertEqual(target.read_text(), self.upstream + "\n")
            self.assertFalse((admin / "MERGE_HEAD").exists())

    def test_checked_merge_denied_before_branch_and_recovery_records(self):
        worktree, sync, admin = self.prepared_detached_sync()
        (self.repo / "alab/command_guard.py").write_text(disk_fixture_cli(ROOT / "alab/command_guard.py", 1))
        before = {p.name: p.read_bytes() for p in admin.iterdir() if p.is_file()}
        for _ in range(2):
            denied = sync()
            self.assertEqual(denied.returncode, 75, denied.stderr)
            self.assertIn("disk resource stop", denied.stderr)
            self.assertEqual({p.name: p.read_bytes() for p in admin.iterdir() if p.is_file()}, before)
            self.assertFalse(self.git("branch", "--list", "alab-sync"))
        ancestor = self.git("merge-base", self.local, self.upstream)
        self.assertEqual(sync("--ref", ancestor).returncode, 0)
        self.assertEqual(sync("--status").returncode, 0)
        unchecked = sync("--no-check", "--no-push")
        self.assertEqual(unchecked.returncode, 3, unchecked.stderr)
        self.assertEqual(self.git("-C", str(worktree), "rev-parse", "MERGE_HEAD"), self.upstream)

    def test_autostaged_rerere_merge_resumes_after_a_resource_stop(self):
        scripts = self.repo / "alab"
        scripts.mkdir()
        for policy in ("owned-paths", "removed-paths", "watched-paths"):
            (scripts / policy).touch()
        base = self.commit("local sync policies")
        worktree, sync = self.public_sync(self.upstream)
        self.git("config", "rerere.autoupdate", "true")
        training = self.repo / ".git/rerere-training"
        self.git("worktree", "add", "-q", "--detach", str(training), base)
        merge = subprocess.run(["git", "-C", str(training), "merge", "--no-ff", "--no-commit", self.upstream],
                               env=self.env, capture_output=True, text=True, timeout=15)
        self.assertEqual(merge.returncode, 1, merge.stderr)
        (training / "conflict.txt").write_text("remembered resolution\n")
        self.git("-C", str(training), "rerere")
        # Return a resource stop after the real raw merge has autostaged its resolution.
        # The first call must preserve the stop status. A later unattended start can recover.
        wrappers = self.repo / "wrappers"
        wrappers.mkdir()
        wrapper = wrappers / "git"
        wrapper.write_text('#!/bin/sh\nreal=' + shlex.quote(shutil.which("git")) + '\n'
                           'case " $* " in *" merge --no-ff --no-commit "*) "$real" "$@"; '
                           'rc=$?; [ "$rc" != 1 ] || exit 75; exit "$rc";; esac\nexec "$real" "$@"\n')
        wrapper.chmod(0o755)
        options = ("--no-check", "--no-push")
        first = sync(*options, PATH=str(wrappers) + os.pathsep + self.env["PATH"])
        self.assertEqual(first.returncode, 75, first.stderr)
        self.assertEqual(self.git("-C", str(worktree), "ls-files", "-u"), "")
        self.assertEqual(self.git("-C", str(worktree), "rev-parse", "HEAD"), base)
        self.assertEqual(self.git("-C", str(worktree), "rev-parse", "MERGE_HEAD"), self.upstream)
        resumed = sync(*options)
        self.assertEqual(resumed.returncode, 0, resumed.stderr)
        self.assertEqual(self.git("-C", str(worktree), "show", "HEAD:conflict.txt"), "remembered resolution")

    def test_guard_measures_the_git_selected_object_store_and_work_directories(self):
        objects = self.repo / "separate objects"
        objects.mkdir()
        index = self.repo / "external index"
        calls = self.repo / "guard args"
        python = self.repo / "guard recorder"
        python.write_text('#!/bin/sh\nprintf "%s\\0" "$@" >"$CALLS"\nexit 75\n')
        python.chmod(0o755)
        body = ('set -euo pipefail\n' + function("sync_disk_write") + r'''
repo_root="$(pwd -P)"
common_dir="$(git rev-parse --absolute-git-dir)"
sync_disk_write git read-tree HEAD
''')
        env = dict(self.env, ALAB_PYTHON=str(python), GIT_OBJECT_DIRECTORY=str(objects.resolve()),
                   GIT_INDEX_FILE=str(index.resolve()), TMPDIR="relative tmp", CALLS=str(calls))
        result = subprocess.run(["/bin/bash", "-c", body], cwd=self.repo, env=env,
                                capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 75, result.stderr)
        args = os.fsdecode(calls.read_bytes()).split("\0")[:-1]
        self.assertIn("--resource-stop", args)
        self.assertEqual(args[args.index("--min-free-gib") + 1], "20")
        self.assertEqual([value for option, value in zip(args, args[1:]) if option == "--disk-path"],
                         list(map(str, (objects.resolve(), index.resolve(), self.repo.resolve() / ".git",
                                        self.repo.resolve(), self.repo.resolve() / "relative tmp"))))
        self.assertEqual(args[args.index("--") + 1:], ["git", "read-tree", "HEAD"])


if __name__ == "__main__":
    unittest.main()
