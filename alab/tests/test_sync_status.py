#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Sync status observes the real kernel lock without changing sync state."""

import os
from pathlib import Path
import select
import shutil
import signal
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
        self.git("init", "-q", "-b", "main")
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
        return subprocess.run(["git", "-C", str(self.repo), *args], env=self.environment,
                              capture_output=True, text=True, check=True).stdout.strip()

    def status(self):
        return subprocess.run(["/bin/bash", str(self.repo / "alab/sync.sh"), "--status"],
                              cwd=self.repo, env=self.environment, capture_output=True,
                              text=True, timeout=60)

    def activity(self, result):
        return [line.removeprefix("sync activity=") for line in result.stdout.splitlines()
                if line.startswith("sync activity=")]

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
            result = self.status()
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
