#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""A sync may only create or fast-forward its reserved branch in its own worktree."""

import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / "alab/sync.sh").read_text()


def function(name):
    start = SOURCE.index("\n" + name + "() {") + 1
    return SOURCE[start:SOURCE.index("\n}\n", start) + 3]


class SyncBranchTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="sync-branch-")
        self.addCleanup(temporary.cleanup)
        self.repo = Path(temporary.name).resolve()
        self.env = {name: value for name, value in os.environ.items()
                    if not name.startswith(("ALAB_", "GIT_"))}
        self.env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
                        GIT_AUTHOR_NAME="Fixture", GIT_COMMITTER_NAME="Fixture",
                        GIT_AUTHOR_EMAIL="fixture@invalid", GIT_COMMITTER_EMAIL="fixture@invalid")
        self.git("init", "-q", "-b", "main")
        (self.repo / ".gitignore").write_text("ignored.txt\n")
        (self.repo / "tracked.txt").write_text("base\n")
        self.git("add", ".gitignore", "tracked.txt")
        self.git("commit", "-qm", "base")
        self.base = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", self.base)
        self.target = self.commit(self.base, "pending upstream")
        self.wt = self.repo / ".git/alab-sync"
        self.git("worktree", "add", "-q", "--detach", str(self.wt), self.base)

    def git(self, *args, cwd=None):
        return subprocess.run(["git", *args], cwd=cwd or self.repo, env=self.env, check=True,
                              capture_output=True, text=True, timeout=30).stdout.strip()

    def commit(self, parent, message):
        return self.git("commit-tree", parent + "^{tree}", "-p", parent, "-m", message)

    def advance_main(self, file="tracked.txt"):
        (self.repo / file).write_text("new main\n")
        self.git("add", "-f", "--", file)
        self.git("commit", "-qm", "advance main")
        tip = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", tip)
        return tip

    def prepare(self, *, run_sync=False, before_git=""):
        body = ("set -euo pipefail\nrepo_root=" + shlex.quote(str(ROOT))
                + '\n. "$repo_root/alab/lib.sh"\ncommon_dir="$PWD/.git"\ncheck=0\nwt="$common_dir/alab-sync"\n'
                + "\n".join(function(name) for name in
                            ("sync_worktree_state", "merging", "worktree_clean", "write_in_wt", "prepare_sync_branch", "run_sync"))
                + "\nin_wt() {\n" + before_git + '\ngit -C "$wt" "$@"\n}\n'
                + ('run_sync "$1"\n' if run_sync else 'prepare_sync_branch "$(git rev-parse origin/main)"\n'))
        return subprocess.run(["/bin/bash", "-c", body, "sync-branch-fixture", self.target], cwd=self.repo,
                              env=self.env, capture_output=True, text=True, timeout=30)

    def assert_prepared(self, expected):
        result = self.prepare()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.git("symbolic-ref", "HEAD", cwd=self.wt), "refs/heads/alab-sync")
        self.assertEqual(self.git("rev-parse", "alab-sync"), expected)
        self.assertEqual(self.git("write-tree", cwd=self.wt), self.git("rev-parse", expected + "^{tree}"))
        self.assertEqual(self.git("status", "--porcelain", cwd=self.wt), "")

    def test_detached_checkout_creates_missing_branch_and_reuses_it_at_same_tip(self):
        self.assert_prepared(self.base)
        before = self.git("reflog", "show", "alab-sync")
        self.assert_prepared(self.base)
        self.assertEqual(self.git("reflog", "show", "alab-sync"), before)

    def test_detached_checkout_switches_to_existing_branch_and_fast_forwards(self):
        self.git("branch", "alab-sync", self.base)
        advanced = self.advance_main()
        self.assert_prepared(advanced)
        self.assertEqual((self.wt / "tracked.txt").read_text(), "new main\n")

    def test_checked_out_sync_branch_fast_forwards_without_a_merge_commit(self):
        self.git("switch", "-qc", "alab-sync", cwd=self.wt)
        self.assert_prepared(self.advance_main())

    def test_unheld_divergent_branch_refuses_before_checkout_or_state_changes(self):
        unique = self.commit(self.base, "unlanded work")
        self.git("branch", "alab-sync", unique)
        self.advance_main()
        index = Path(self.git("rev-parse", "--path-format=absolute", "--git-path", "index", cwd=self.wt))
        receipt = index.with_name("alab-sync-stop")
        receipt.write_text("previous stop\n")
        before = index.read_bytes()
        result = self.prepare(run_sync=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("alab-sync holds commits main lacks", result.stderr)
        self.assertEqual(self.git("rev-parse", "alab-sync"), unique)
        self.assertEqual(self.git("rev-parse", "--abbrev-ref", "HEAD", cwd=self.wt), "HEAD")
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=self.wt), self.base)
        self.assertEqual(receipt.read_text(), "previous stop\n")
        self.assertEqual(index.read_bytes(), before)

    def test_next_prefix_preserves_untracked_file_even_when_status_hides_it(self):
        self.assert_prepared(self.base)
        self.git("config", "status.showUntrackedFiles", "no")
        sentinel = self.wt / "unsaved.txt"
        sentinel.write_bytes(b"keep this between prefixes")
        result = self.prepare(run_sync=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("untracked", result.stderr)
        self.assertEqual(sentinel.read_bytes(), b"keep this between prefixes")
        self.assertEqual(self.git("rev-parse", "alab-sync"), self.base)

    def test_open_merge_keeps_its_recovery_record_even_with_a_clean_index(self):
        self.assert_prepared(self.base)
        self.git("merge", "--no-ff", "--no-commit", self.target, cwd=self.wt)
        self.assertEqual(self.git("status", "--porcelain", cwd=self.wt), "")
        common = Path(self.git("rev-parse", "--absolute-git-dir", cwd=self.wt))
        stop = common / "alab-sync-stop"
        stop.write_text("keep resolver state\n")
        index = (common / "index").read_bytes()
        result = self.prepare(run_sync=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("merge is already in progress", result.stderr)
        self.assertEqual(self.git("rev-parse", "MERGE_HEAD", cwd=self.wt), self.target)
        self.assertEqual((common / "index").read_bytes(), index)
        self.assertEqual(stop.read_text(), "keep resolver state\n")

    def test_fast_forward_refuses_to_overwrite_an_ignored_path(self):
        self.git("switch", "-qc", "alab-sync", cwd=self.wt)
        self.advance_main(file="ignored.txt")
        ignored = self.wt / "ignored.txt"
        ignored.write_text("keep local bytes\n")
        result = self.prepare()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(ignored.read_text(), "keep local bytes\n")
        self.assertEqual(self.git("rev-parse", "alab-sync"), self.base)

    def test_concurrent_creation_keeps_the_new_branch_tip(self):
        unique = self.commit(self.base, "new owner")
        self.env["RACE_TIP"] = unique
        result = self.prepare(before_git='''
if [[ "$1" == switch ]]; then git branch alab-sync "$RACE_TIP"; fi''')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.git("rev-parse", "alab-sync"), unique)
        self.assertEqual(self.git("rev-parse", "--abbrev-ref", "HEAD", cwd=self.wt), "HEAD")

    def test_switch_refuses_to_replace_an_ignored_path_with_an_older_tracked_file(self):
        tracked = self.advance_main(file="ignored.txt")
        self.git("branch", "alab-sync", tracked)
        self.git("rm", "-q", "ignored.txt")
        self.git("commit", "-qm", "remove ignored file")
        main = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", main)
        self.git("switch", "--detach", "-q", main, cwd=self.wt)
        ignored = self.wt / "ignored.txt"
        ignored.write_text("preserve before switching\n")
        result = self.prepare()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(ignored.read_text(), "preserve before switching\n")
        self.assertEqual(self.git("rev-parse", "alab-sync"), tracked)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=self.wt), main)

    def test_concurrent_worktree_claim_refuses_to_switch(self):
        self.git("branch", "alab-sync", self.base)
        result = self.prepare(before_git='''
if [[ "$1" == switch ]]; then git worktree add -q "$PWD/other" alab-sync; fi''')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.git("symbolic-ref", "HEAD", cwd=self.repo / "other"), "refs/heads/alab-sync")
        self.assertEqual(self.git("rev-parse", "--abbrev-ref", "HEAD", cwd=self.wt), "HEAD")

    def test_tip_advanced_after_ancestry_check_is_kept_when_merge_reports_up_to_date(self):
        self.git("switch", "-qc", "alab-sync", cwd=self.wt)
        advanced = self.advance_main()
        unique = self.commit(advanced, "concurrent commit")
        self.env.update(RACE_TIP=unique, ORIGINAL=self.base)
        result = self.prepare(run_sync=True, before_git='''
if [[ "$1" == merge ]]; then git update-ref refs/heads/alab-sync "$RACE_TIP" "$ORIGINAL"; fi''')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("sync branch moved during preparation", result.stderr)
        self.assertEqual(self.git("rev-parse", "alab-sync"), unique)


if __name__ == "__main__":
    unittest.main()
