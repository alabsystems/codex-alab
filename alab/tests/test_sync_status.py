#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Sync status observes the real kernel lock without changing sync state."""

import os
from pathlib import Path
import select
import shlex
import shutil
import signal
import stat
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class SyncStatusTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="alab-sync-status-")
        self.addCleanup(self.scratch.cleanup)
        self.repo = Path(self.scratch.name).resolve()
        self.environment = {key: value for key, value in os.environ.items()
                            if not key.startswith(("ALAB_", "GIT_"))}
        self.environment.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
                                GIT_OPTIONAL_LOCKS="0")
        self.git_executable = shutil.which("git")
        self.git("init", "-q", "-b", "main")
        (self.repo / "tracked.txt").write_text("base\n")
        self.git("add", "tracked.txt")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@invalid",
                 "commit", "-q", "--allow-empty", "-m", "base")
        self.git("remote", "add", "upstream", str(self.repo))
        for remote in ("origin", "upstream"):
            self.git("update-ref", f"refs/remotes/{remote}/main", "HEAD")
        scripts = self.repo / "alab"
        scripts.mkdir()
        for name in ("lib.sh", "sync.sh"):
            shutil.copyfile(ROOT / "alab" / name, scripts / name)
        self.wt = self.repo / ".git/alab-sync"
        self.git("worktree", "add", "-q", "--detach", str(self.wt), "HEAD")
        self.git("-C", str(self.wt), "-c", "user.name=Fixture",
                 "-c", "user.email=fixture@invalid", "commit", "-q", "--allow-empty",
                 "-m", "unlanded prefix")
        self.stop = Path(self.git("-C", str(self.wt), "rev-parse", "--path-format=absolute",
                                  "--git-path", "alab-sync-stop"))
        self.lock = self.repo / ".git/alab-sync.lockf"

    def git(self, *args):
        return subprocess.run([self.git_executable, "-C", str(self.repo), *args], env=self.environment,
                              capture_output=True, text=True, check=True).stdout.strip()

    def status(self):
        return subprocess.run(["/bin/bash", str(self.repo / "alab/sync.sh"), "--status"],
                              cwd=self.repo, env=self.environment, capture_output=True,
                              text=True, timeout=60)

    def activity(self, result):
        return [line.removeprefix("sync activity=") for line in result.stdout.splitlines()
                if line.startswith("sync activity=")]

    def observation(self):
        registrations = subprocess.run(
            [self.git_executable, "-C", str(self.repo), "worktree", "list", "--porcelain", "-z"],
            env=self.environment, capture_output=True, check=True).stdout
        roots = {self.repo, *(Path(os.fsdecode(field[9:])) for field in registrations.split(b"\0")
                              if field.startswith(b"worktree "))}
        files = {}

        def capture(path):
            info = path.lstat()
            payload = (os.readlink(path) if stat.S_ISLNK(info.st_mode) else
                       path.read_bytes() if stat.S_ISREG(info.st_mode) else None)
            files[str(path)] = (info.st_mode, payload)

        for root in roots:
            if not root.is_dir():
                continue
            for directory, children, names in os.walk(root, followlinks=False):
                children[:] = [name for name in children if name != ".git"]
                for name in list(children):
                    path = Path(directory) / name
                    if path.is_symlink():
                        capture(path)
                        children.remove(name)
                for name in names:
                    capture(Path(directory) / name)
        common = self.repo / ".git"
        controls = [common / "index", *common.glob("worktrees/*/index"),
                    *common.glob("alab-sync-*"), *common.glob("worktrees/*/alab-sync-*")]
        for path in controls:
            if path.is_file() or path.is_symlink():
                capture(path)
        return {"refs": self.git("show-ref", "--dereference"),
                "registrations": registrations, "files": files}

    def readonly_status(self):
        before = self.observation()
        result = self.status()
        self.assertEqual(self.observation(), before, "status changed repository or checkout state")
        return result

    def holder(self, unique=True, name="other live checkout"):
        path = self.repo / name
        self.git("worktree", "add", "-q", "-b", "alab-sync", str(path), "origin/main")
        if unique:
            (path / "tracked.txt").write_text("unique holder commit\n")
            self.git("-C", str(path), "add", "tracked.txt")
            self.git("-C", str(path), "-c", "user.name=Fixture", "-c",
                     "user.email=fixture@invalid", "commit", "-q", "-m", "unique holder")
        (path / "tracked.txt").write_text("staged holder content\n")
        self.git("-C", str(path), "add", "tracked.txt")
        (path / "tracked.txt").write_text("unstaged holder content\n")
        (path / "untracked.txt").write_text("keep this untracked file\n")
        return path

    def no_recovery_advice(self, result):
        for text in ("--abort", "--continue", "git worktree remove", "git worktree prune", "reset --hard"):
            self.assertNotIn(text, result.stdout + result.stderr)

    def assert_holder_status(self, result, holder, message):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.activity(result), ["inactive"])
        self.assertIn(message, result.stdout)
        for path in (holder, self.wt):
            quoted = subprocess.run(["/bin/bash", "-c", 'printf "%q" "$1"', "quote-path", str(path)],
                                    capture_output=True, text=True, check=True, timeout=10).stdout
            self.assertIn(quoted, result.stdout)
        self.assertNotIn("none yet", result.stdout)
        self.no_recovery_advice(result)

    def test_live_branch_holder_is_reported_when_configured_checkout_is_missing(self):
        self.git("worktree", "remove", "--force", str(self.wt))
        holder = self.holder()
        self.assert_holder_status(self.readonly_status(), holder, "branch held elsewhere")
        self.assertFalse(self.wt.exists())

    def test_live_branch_holder_takes_precedence_over_an_existing_detached_checkout(self):
        holder = self.holder()
        self.assert_holder_status(self.readonly_status(), holder, "branch held elsewhere")

    def test_branch_holder_path_preserves_embedded_and_trailing_newlines(self):
        holder = self.holder(name="other\ncheckout\n")
        self.assert_holder_status(self.readonly_status(), holder, "branch held elsewhere")

    def unavailable_holder(self, unique):
        holder = self.holder(unique=unique)
        moved = self.repo / "unavailable checkout files"
        holder.rename(moved)
        self.assert_holder_status(self.readonly_status(), holder, "registered branch checkout unavailable")
        self.assertIn(str(holder), self.git("worktree", "list", "--porcelain"))
        self.assertEqual((moved / "untracked.txt").read_text(), "keep this untracked file\n")
        self.assertEqual(bool(self.git("rev-list", "origin/main..alab-sync")), unique)

    def test_unavailable_registered_holder_is_observed_without_pruning(self):
        self.unavailable_holder(unique=False)

    def test_unavailable_registered_holder_preserves_unique_commits(self):
        self.unavailable_holder(unique=True)

    def test_matching_holder_and_its_configured_alias_keep_normal_status(self):
        self.git("-C", str(self.wt), "switch", "-q", "-c", "alab-sync")
        (self.wt / "untracked.txt").write_text("keep configured checkout data\n")
        alias = self.repo / "configured alias"
        alias.symlink_to(self.wt, target_is_directory=True)
        for configured in (self.wt, alias):
            with self.subTest(configured=configured):
                self.environment["ALAB_SYNC_DIR"] = str(configured)
                result = self.readonly_status()
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.activity(result), ["inactive"])
                self.assertIn("committed, not landed", result.stdout)
                self.assertNotIn("branch held elsewhere", result.stdout)
                self.assertNotIn("checkout unavailable", result.stdout)
                self.assertNotIn("none yet", result.stdout)

    def test_active_lock_precedes_a_different_registered_branch_holder(self):
        self.holder()
        result = self.locked_status()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.activity(result), ["active"])
        self.assertNotIn("branch held elsewhere", result.stdout)
        self.no_recovery_advice(result)

    def test_failed_branch_holder_enumeration_reports_unknown_without_recovery(self):
        self.holder()
        wrappers = self.repo / "status wrappers"
        wrappers.mkdir()
        wrapper = wrappers / "git"
        wrapper.write_text("#!/bin/sh\nif [ \"$1\" = worktree ] && [ \"$2\" = list ]; then\n"
                           "  echo 'fixture worktree read failed' >&2\n  exit 23\nfi\n"
                           "exec " + shlex.quote(self.git_executable) + ' "$@"\n')
        wrapper.chmod(0o755)
        self.environment["PATH"] = str(wrappers) + os.pathsep + self.environment["PATH"]
        result = self.readonly_status()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown", result.stdout + result.stderr)
        self.assertNotIn("none yet", result.stdout)
        self.no_recovery_advice(result)

    def test_duplicate_forced_branch_holders_report_unknown_without_recovery(self):
        self.holder()
        self.git("worktree", "add", "-q", "--force", str(self.repo / "second holder"), "alab-sync")
        result = self.readonly_status()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown", result.stdout + result.stderr)
        self.assertNotIn("none yet", result.stdout)
        self.no_recovery_advice(result)

    def test_malformed_branch_record_cannot_borrow_a_previous_worktree_path(self):
        self.holder()
        wrappers = self.repo / "malformed status wrapper"
        wrappers.mkdir()
        wrapper = wrappers / "git"
        wrapper.write_text(
            '#!/bin/sh\nif [ "$1" = worktree ] && [ "$2" = list ]; then\n'
            "  printf '%s\\0\\0%s\\0\\0' " + shlex.quote("worktree " + str(self.repo / "stale record"))
            + " 'branch refs/heads/alab-sync'\n  exit 0\nfi\n"
            + "exec " + shlex.quote(self.git_executable) + ' "$@"\n')
        wrapper.chmod(0o755)
        self.environment["PATH"] = str(wrappers) + os.pathsep + self.environment["PATH"]
        result = self.readonly_status()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown", result.stdout + result.stderr)
        self.assertNotIn("none yet", result.stdout)
        self.no_recovery_advice(result)

    def ensure_without_changes(self):
        source = (self.repo / "alab/sync.sh").read_text()
        functions = []
        for name in ("sync_branch_holder", "ensure_worktree"):
            begin = source.index("\n" + name + "() {") + 1
            end = source.index("\n}\n", begin) + 3
            functions.append(source[begin:end])
        command = "set -euo pipefail\n" + "\n".join(functions) + '\nwt="$1"\n'
        command += 'alab_die() { printf "%s\\n" "$*" >&2; exit 1; }\nensure_worktree\n'
        before = self.observation()
        result = subprocess.run(["/bin/bash", "-c", command, "ensure-fixture", str(self.wt)],
                                cwd=self.repo, env=self.environment, capture_output=True,
                                text=True, timeout=60)
        self.assertEqual(self.observation(), before, "refused startup changed checkout custody")
        return result

    def test_startup_preserves_unavailable_holder_with_unique_commits(self):
        holder = self.holder()
        holder.rename(self.repo / "unavailable startup checkout")
        result = self.ensure_without_changes()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("alab-sync holds commits main lacks", result.stderr)
        self.assertIn(str(holder), result.stderr)
        self.assertTrue(self.git("rev-list", "origin/main..alab-sync"))

    def test_startup_refuses_live_newline_holder_elsewhere_without_changes(self):
        holder = self.holder(name="startup\nholder\n")
        result = self.ensure_without_changes()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("the sync worktree of this clone is", result.stderr)
        self.assertIn(str(holder), result.stderr)
        self.assertIn(str(self.wt), result.stderr)

    def test_unchecked_start_and_resume_cannot_publish_or_modify_sync_state(self):
        before = self.git("-C", str(self.wt), "rev-parse", "HEAD")
        for options in (["--no-check"], ["--continue", "--no-check"],
                        ["--stepwise", "--no-check"]):
            with self.subTest(options=options):
                result = subprocess.run(
                    ["/bin/bash", str(self.repo / "alab/sync.sh"), *options],
                    cwd=self.repo, env=self.environment, capture_output=True,
                    text=True, timeout=60)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("--no-check requires --no-push", result.stderr)
                self.assertFalse(self.stop.exists())
                self.assertFalse(self.lock.exists())
                self.assertEqual(self.git("-C", str(self.wt), "rev-parse", "HEAD"), before)

    def locked_status(self):
        release = self.repo / "release"
        os.mkfifo(release)
        release_fd = os.open(release, os.O_RDWR | os.O_NONBLOCK)
        command = 'set -euo pipefail; . "$1"; shift; alab_locked "$@"'
        holder_command = 'printf "ready\\n"; IFS= read -r answer < "$1"'
        holder = subprocess.Popen(
            ["/bin/bash", "-c", command, "sync-lock-fixture", str(self.repo / "alab/lib.sh"),
             str(self.lock), "/bin/bash", "-c", holder_command, "lock-held", str(release)],
            env=self.environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            start_new_session=True)
        try:
            self.assertTrue(select.select([holder.stdout], [], [], 60)[0],
                            "fixture never acquired the kernel lock")
            self.assertEqual(holder.stdout.readline(), "ready\n")
            result = self.readonly_status()
            os.write(release_fd, b"release\n")
            stdout, stderr = holder.communicate(timeout=60)
            self.assertEqual(holder.returncode, 0, stdout + stderr)
            return result
        finally:
            os.close(release_fd)
            if holder.poll() is None:
                os.killpg(holder.pid, signal.SIGKILL)
                holder.communicate(timeout=60)

    def test_committed_prefix_reports_active_only_while_the_kernel_lock_is_held(self):
        before = self.git("-C", str(self.wt), "rev-parse", "HEAD")
        active = self.locked_status()
        self.assertEqual(active.returncode, 0, active.stderr)
        self.assertEqual(self.activity(active), ["active"])
        self.assertNotIn("--continue", active.stdout)
        inactive = self.status()
        self.assertEqual(inactive.returncode, 0, inactive.stderr)
        self.assertEqual(self.activity(inactive), ["inactive"])
        self.assertEqual(self.git("-C", str(self.wt), "rev-parse", "HEAD"), before)
        self.assertFalse(self.stop.exists())

    def test_a_held_lock_takes_precedence_over_a_previous_gate_stop(self):
        record = "4\nfixture-gate-reason\n"
        self.stop.write_text(record)
        active = self.locked_status()
        self.assertEqual(active.returncode, 0, active.stderr)
        self.assertEqual(self.activity(active), ["active"])
        self.assertNotIn("fixture-gate-reason", active.stdout)
        inactive = self.status()
        self.assertEqual(inactive.returncode, 0, inactive.stderr)
        self.assertEqual(self.activity(inactive), ["inactive"])
        self.assertIn("fixture-gate-reason", inactive.stdout)
        self.assertEqual(self.stop.read_text(), record)

    def test_an_inaccessible_lock_is_unknown_instead_of_an_inactive_verdict(self):
        self.lock.symlink_to(self.lock.name)
        result = self.status()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.activity(result), ["unknown"])
        self.assertFalse(self.stop.exists())


if __name__ == "__main__":
    unittest.main()
