# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the production concurrent-main function at its ancestry boundary."""

import os
import shlex
import shutil
import sys
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_github import disk_fixture_cli


ROOT = Path(__file__).resolve().parents[2]


class ConcurrentMainTests(unittest.TestCase):
    def run_concurrent_main(self, ancestry=0, upstream="main-upstream", history=0,
                            merge=0, merge_head=None):
        source = (ROOT / "alab/sync.sh").read_text()
        begin = source.index("\ntake_concurrent_main() {") + 1
        end = source.index("\n}\n", begin) + 3
        with tempfile.TemporaryDirectory(prefix="sync-ancestry-") as temporary:
            root = Path(temporary)
            head_file = root / "MERGE_HEAD"
            if merge_head is not None:
                head_file.write_text(merge_head)
            harness = root / "run.sh"
            harness.write_text("set -euo pipefail\n. " + shlex.quote(str(ROOT / "alab/lib.sh")) + "\n" + source[begin:end] + r'''
wt=candidate
repo_root=source
merge_cfg=(-c fixture=yes)
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
write_in_wt() { in_wt "$@"; }
in_wt() {
  case "$1" in
    log) record own-log; printf '  local-fix preserve this fix-forward\n' ;;
    -c) record merge; return "${MERGE_RC}" ;;
    rev-parse) printf '%s\n' "${MERGE_HEAD_FILE}" ;;
    commit) record commit ;;
    *) exit 98 ;;
  esac
}
reresolve_cargo_lock() { record resolve; }
alab_say() { printf '%s\n' "$*"; }
alab_die() { printf '%s\n' "$*" >&2; exit 1; }
exit_with() { printf '%s\n' "$2" >&2; exit "$1"; }
take_concurrent_main fixed-main
''')
            actions = root / "actions"
            env = dict(os.environ, ACTIONS=str(actions), ANCESTRY_RC=str(ancestry),
                       MAIN_UPSTREAM=upstream, HISTORY_RC=str(history), MERGE_RC=str(merge),
                       MERGE_HEAD_FILE=str(head_file))
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

    def test_merge_errors_and_interruptions_preserve_status(self):
        for status, head in [(1, None), (1, "another-main\n"), (1, "fixed-main\nextra\n"),
                             (75, None), (128, None), (143, "fixed-main\n")]:
            with self.subTest(status=status, head=head):
                result, actions = self.run_concurrent_main(merge=status, merge_head=head)
                self.assertEqual(result.returncode, status, result.stderr)
                self.assertEqual(actions, ["ancestry", "merge"])
                self.assertNotIn("resolve, git add", result.stderr)

    def test_conflict_with_the_requested_pending_merge_needs_a_resolver(self):
        result, actions = self.run_concurrent_main(merge=1, merge_head="fixed-main\n")
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertEqual(actions, ["ancestry", "merge"])
        self.assertIn("resolve, git add", result.stderr)

    def test_scheduled_retry_of_an_index_lock_does_not_wait_for_a_resolver(self):
        with tempfile.TemporaryDirectory(prefix="sync-concurrent-lock-") as tmp:
            root = Path(tmp).resolve()
            repo, upstream = root / "main", root / "upstream"
            repo.mkdir()
            env = {k: v for k, v in os.environ.items() if not k.startswith(("GIT_", "ALAB_"))}
            env.update(HOME=str(root), GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                       GIT_OPTIONAL_LOCKS="0", GIT_AUTHOR_NAME="Fixture", GIT_COMMITTER_NAME="Fixture",
                       GIT_AUTHOR_EMAIL="fixture@invalid", GIT_COMMITTER_EMAIL="fixture@invalid",
                       ALAB_PYTHON=sys.executable)

            def git(cwd, *args):
                return subprocess.run(["git", *args], cwd=cwd, env=env, check=True,
                                      text=True, capture_output=True, timeout=30).stdout.strip()

            git(repo, "init", "-qb", "main")
            git(repo, "config", "rerere.enabled", "true")
            (repo / "tracked").write_text("base\n")
            git(repo, "add", "tracked")
            git(repo, "commit", "-qm", "base")
            git(repo, "clone", "-q", "--local", str(repo), str(upstream))
            (upstream / "tracked").write_text("upstream\n")
            git(upstream, "commit", "-qam", "advance upstream")
            (repo / "concurrent").write_text("main\n")
            git(repo, "add", "concurrent")
            git(repo, "commit", "-qm", "advance private main")
            for remote, path in (("upstream", upstream), ("origin", repo)):
                git(repo, "remote", "add", remote, str(path))
                git(repo, "fetch", "-q", remote)
            sync = repo / ".git/alab-sync"
            git(repo, "worktree", "add", "-q", "-b", "alab-sync", str(sync), "upstream/main")
            head = git(sync, "rev-parse", "HEAD")
            admin = Path(git(sync, "rev-parse", "--absolute-git-dir"))
            (admin / "alab-sync-target").write_text(head + "\n")
            lock = admin / "index.lock"
            lock.write_bytes(b"occupied")
            scripts = repo / "alab"
            scripts.mkdir()
            for name in ("sync.sh", "sync_worktree.py", "lib.sh", "github.py", "command_guard.py",
                         "cli_status.py", "file_hash.py"):
                shutil.copy2(ROOT / "alab" / name, scripts / name)
            (scripts / "command_guard.py").write_text(disk_fixture_cli(ROOT / "alab/command_guard.py", 30))
            for _ in range(2):
                result = subprocess.run(["/bin/bash", str(scripts / "sync.sh"), "--stepwise"],
                                        cwd=repo, env=env, text=True, capture_output=True, timeout=30)
                self.assertEqual(result.returncode, 128, result.stderr)
                self.assertIn("index.lock", result.stderr)
                self.assertFalse((admin / "alab-sync-stop").exists())
                self.assertFalse((admin / "MERGE_HEAD").exists())
                self.assertEqual(git(sync, "rev-parse", "HEAD"), head)
                self.assertEqual(lock.read_bytes(), b"occupied")


if __name__ == "__main__":
    unittest.main()
