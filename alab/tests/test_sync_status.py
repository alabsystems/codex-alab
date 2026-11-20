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
import sys
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
        for name in ("lib.sh", "sync.sh", "sync_worktree.py"):
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

    def status(self, *, environment=None):
        return subprocess.run(["/bin/bash", str(self.repo / "alab/sync.sh"), "--status"],
                              cwd=self.repo, env=self.environment if environment is None else environment,
                              capture_output=True,
                              text=True, timeout=60)

    def activity(self, result):
        return [line.removeprefix("sync activity=") for line in result.stdout.splitlines()
                if line.startswith("sync activity=")]

    def observation(self):
        registrations = subprocess.run(
            [self.git_executable, "-C", str(self.repo), "worktree", "list", "--porcelain", "-z"],
            env=self.environment, capture_output=True, check=True).stdout
        roots = {self.repo, self.wt, *(Path(os.fsdecode(field[9:])) for field in registrations.split(b"\0")
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
        controls = [common / "index", *common.glob("alab-sync-*"), *common.glob("worktrees/*/alab-sync-*")]
        for name in ("index", "locked", "gitdir", "commondir", "HEAD"):
            controls.extend(common.glob("worktrees/*/" + name))
        for path in controls:
            if path.is_file() or path.is_symlink():
                capture(path)
        return {"refs": self.git("show-ref", "--dereference"),
                "registrations": registrations, "files": files}

    def readonly_status(self, *, environment=None):
        before = self.observation()
        result = self.status(environment=environment)
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

    def assert_idle_status_preserves_index(self, optional_locks):
        head = self.git("-C", str(self.wt), "rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", head)
        self.assertEqual(self.git("-C", str(self.wt), "rev-list", "origin/main..HEAD"), "")
        tracked = self.wt / "tracked.txt"
        content = tracked.read_bytes()
        before = tracked.stat()
        changed_mtime = before.st_mtime_ns - 2_000_000_000
        os.utime(tracked, ns=(before.st_atime_ns, changed_mtime))
        self.assertEqual(tracked.stat().st_mtime_ns, changed_mtime)
        self.assertEqual(tracked.read_bytes(), content)
        environment = self.environment.copy()
        if optional_locks is None:
            environment.pop("GIT_OPTIONAL_LOCKS", None)
        else:
            environment["GIT_OPTIONAL_LOCKS"] = optional_locks
        self.assertEqual(self.environment["GIT_OPTIONAL_LOCKS"], "0")
        result = self.readonly_status(environment=environment)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.activity(result), ["inactive"])
        self.assertIn("sync worktree  idle (", result.stdout)
        self.assertNotIn("uncommitted changes", result.stdout)

    def test_idle_status_preserves_index_when_optional_locks_is_unset(self):
        self.assert_idle_status_preserves_index(None)

    def test_idle_status_preserves_index_when_optional_locks_is_enabled(self):
        self.assert_idle_status_preserves_index("1")

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

    def test_unreadable_stop_record_does_not_advise_unattended_resume(self):
        self.stop.mkdir()
        result = self.readonly_status()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("cannot read sync stop record", result.stderr)
        self.assertNotIn("next alab/sync.sh resumes", result.stdout)
        self.no_recovery_advice(result)

    def test_invalid_stop_record_does_not_advise_unattended_resume(self):
        self.stop.write_text("unknown-hold\nkeep waiting\n")
        result = self.readonly_status()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("cannot read sync stop record", result.stderr)
        self.assertNotIn("next alab/sync.sh resumes", result.stdout)
        self.no_recovery_advice(result)

    def ensure(self, configured):
        source = (self.repo / "alab/sync.sh").read_text()
        functions = []
        for name in ("sync_worktree_state", "ensure_worktree"):
            begin = source.index("\n" + name + "() {") + 1
            end = source.index("\n}\n", begin) + 3
            functions.append(source[begin:end])
        command = ("set -euo pipefail\n" + "\n".join(functions)
                   + '\nrepo_root="$PWD"\ncommon_dir="$PWD/.git"\nwt="$1"\ncheck=0\n'
                   + '. "$repo_root/alab/lib.sh"\nensure_worktree\n')
        return subprocess.run(["/bin/bash", "-c", command, "ensure-fixture", str(configured)],
                              cwd=self.repo, env=self.environment, capture_output=True,
                              text=True, timeout=60)

    def ensure_without_changes(self, configured=None):
        before = self.observation()
        result = self.ensure(self.wt if configured is None else configured)
        self.assertEqual(self.observation(), before, "startup changed checkout custody")
        return result

    def recover(self, mode, configured):
        return subprocess.run(["/bin/bash", str(self.repo / "alab/sync.sh"), mode],
                              cwd=self.repo, env=dict(self.environment, ALAB_SYNC_DIR=str(configured)),
                              capture_output=True, text=True, timeout=60)

    def assert_custody_refused(self, configured):
        self.git("config", "rerere.enabled", "true")
        before = self.observation()
        for mode in ("start", "--continue", "--abort"):
            with self.subTest(mode=mode, configured=configured):
                result = self.ensure(configured) if mode == "start" else self.recover(mode, configured)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn("cannot use sync worktree", result.stderr)
                self.assertEqual(self.observation(), before, "sync changed refused checkout custody")

    def test_occupied_reserved_checkout_with_missing_gitfile_is_preserved(self):
        holder = self.holder(unique=False)
        (holder / ".git").rename(holder / "saved gitfile")
        self.assert_custody_refused(self.wt)
        self.assertEqual((holder / "untracked.txt").read_text(), "keep this untracked file\n")

    def test_locked_detached_registration_survives_even_when_checkout_disappears(self):
        self.git("worktree", "lock", "--reason", "preserve this checkout", str(self.wt))
        self.assert_custody_refused(self.wt)
        self.wt.rename(self.repo / "temporarily unavailable")
        self.assert_custody_refused(self.wt)

    def test_missing_detached_registration_is_preserved_without_a_lock(self):
        self.wt.rename(self.repo / "unavailable detached sync")
        self.assert_custody_refused(self.wt)

    def test_redirected_worktree_configuration_cannot_select_a_different_directory(self):
        protected = self.repo / "outside sync"
        protected.mkdir()
        (protected / "keep.txt").write_text("keep this file\n")
        self.git("config", "extensions.worktreeConfig", "true")
        self.git("-C", str(self.wt), "config", "--worktree", "core.worktree", str(protected))
        self.assert_custody_refused(self.wt)
        self.assertEqual((protected / "keep.txt").read_text(), "keep this file\n")

    def test_relative_worktree_links_identify_the_same_checkout(self):
        help_result = subprocess.run([self.git_executable, "worktree", "add", "-h"], cwd=self.repo,
                                     env=self.environment, capture_output=True, text=True, timeout=60)
        if "relative-paths" not in help_result.stdout + help_result.stderr:
            self.skipTest("this Git cannot create relative worktree links")
        fresh = self.repo / "relative links"
        self.git("worktree", "add", "--relative-paths", "-q", "--detach", str(fresh), "HEAD")
        self.assertFalse(Path((fresh / ".git").read_text().removeprefix("gitdir: ").rstrip("\n")).is_absolute())
        result = self.ensure_without_changes(fresh)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_primary_foreign_and_uncatalogued_paths_are_preserved(self):
        self.assert_custody_refused(self.repo)
        empty = self.repo / "existing empty folder"
        empty.mkdir()
        self.assert_custody_refused(empty)
        dangling = self.repo / "dangling checkout alias"
        dangling.symlink_to(self.repo / "missing target")
        self.assert_custody_refused(dangling)
        foreign = self.repo / "separate clone"
        self.git("clone", "-q", str(self.repo), str(foreign))
        (foreign / "untracked.txt").write_text("owner's work\n")
        self.assert_custody_refused(foreign)
        self.assertEqual((foreign / "untracked.txt").read_text(), "owner's work\n")

    def test_other_branch_checkout_and_borrowed_gitfile_are_preserved(self):
        self.git("-C", str(self.wt), "switch", "-qc", "another-task")
        self.assert_custody_refused(self.wt)
        self.git("-C", str(self.wt), "switch", "--detach", "-q")
        sibling = self.repo / "detached sibling"
        self.git("worktree", "add", "-q", "--detach", str(sibling), "HEAD")
        (self.wt / ".git").write_bytes((sibling / ".git").read_bytes())
        self.assert_custody_refused(self.wt)

    def test_foreign_admin_is_rejected_before_looking_up_its_backlink(self):
        external = self.repo / "unrelated admin folder"
        external.mkdir()
        (self.wt / ".git").write_text(f"gitdir: {external}\n")
        result = self.ensure_without_changes()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("is outside this clone", result.stderr)

    def test_unregistered_missing_path_can_be_created_and_reached_by_its_alias(self):
        fresh = self.repo / "new checkout"
        self.git("config", "rerere.enabled", "true")
        self.assertEqual(self.recover("--abort", fresh).returncode, 0)
        self.assertFalse(fresh.exists())
        result = self.ensure(fresh)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.git("-C", str(fresh), "rev-parse", "HEAD"), self.git("rev-parse", "origin/main"))
        alias = self.repo / "alias for new checkout"
        alias.symlink_to(fresh, target_is_directory=True)
        result = self.ensure_without_changes(alias)
        self.assertEqual(result.returncode, 0, result.stderr)
        (fresh / "tracked.txt").write_text("sync edit to abandon\n")
        result = self.recover("--abort", alias)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((fresh / "tracked.txt").read_text(), "base\n")

    def test_first_checked_checkout_is_admitted_and_existing_worktrees_are_reused(self):
        # Exercise real Git registration and checkout, controlling only capacity admission.
        source = (ROOT / "alab/sync.sh").read_text()
        functions = []
        for name in ("sync_worktree_state", "ensure_worktree"):
            begin = source.index("\n" + name + "() {") + 1
            functions.append(source[begin:source.index("\n}\n", begin) + 3])
        script = ("set -euo pipefail\n. " + shlex.quote(str(ROOT / "alab/lib.sh")) + "\n"
                  + "\n".join(functions) + '\nrepo_root="$1" common_dir="$1/.git" wt="$2" check="$3"\n'
                  + 'ensure_worktree\n')
        calls = self.repo / "seed calls"
        guardian = self.repo / "guardian"
        guardian.write_text('#!/bin/sh\nif [ "${3##*/}" != command_guard.py ]; then exec '
                            + shlex.quote(sys.executable) + ' "$@"; fi\n'
                            '{ printf "%s\\0" "$@"; printf "\\0"; } >>"$CALLS"\n'
                            'if [ "$STOP" != 0 ]; then exit "$STOP"; fi\n'
                            'while [ "$1" != -- ]; do shift; done\nshift\nexec "$@"\n')
        guardian.chmod(0o755)
        seed = self.repo / "configured seed"

        def run(path, check, status):
            return subprocess.run(["/bin/bash", "-c", script, "seed", str(self.repo), str(path), str(check)],
                                  cwd=self.repo, env=dict(self.environment, ALAB_PYTHON=str(guardian),
                                                         CALLS=str(calls), STOP=str(status)),
                                  capture_output=True, text=True, timeout=60)

        registrations = self.git("worktree", "list", "--porcelain")
        for status in (75, 143):
            result = run(seed, 1, status)
            self.assertEqual(result.returncode, status, result.stderr)
            self.assertFalse(seed.exists())
            self.assertEqual(self.git("worktree", "list", "--porcelain"), registrations)
        result = run(seed, 1, 0)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((seed / "tracked.txt").read_text(), "base\n")
        invocations = calls.read_bytes().removesuffix(b"\0\0").split(b"\0\0")
        self.assertEqual(len(invocations), 3)
        for invocation in invocations:
            args = os.fsdecode(invocation).split("\0")
            guard, command = args[:args.index("--")], args[args.index("--") + 1:]
            self.assertIn("--resource-stop", guard)
            self.assertEqual(guard[guard.index("--min-free-gib") + 1], "20")
            self.assertEqual([value for option, value in zip(guard, guard[1:]) if option == "--disk-path"],
                             [str(seed), str(self.repo / ".git")])
            self.assertEqual(command[:3], ["git", "worktree", "add"])
        admitted = calls.read_bytes()
        self.assertEqual(run(seed, 1, 75).returncode, 0)
        unchecked = self.repo / "source only"
        self.assertEqual(run(unchecked, 0, 75).returncode, 0)
        self.assertEqual((unchecked / "tracked.txt").read_text(), "base\n")
        self.assertEqual(calls.read_bytes(), admitted)

    def test_startup_preserves_unavailable_holder_with_unique_commits(self):
        holder = self.holder()
        holder.rename(self.repo / "unavailable startup checkout")
        result = self.ensure_without_changes()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("alab-sync holds commits main lacks", result.stderr)
        self.assertIn(str(holder), result.stderr)
        self.assertTrue(self.git("rev-list", "origin/main..alab-sync"))

    def failed_missing_holder_history(self, output):
        holder = self.holder()
        holder.rename(self.repo / "unavailable history checkout")
        wrappers = self.repo / "history wrappers"
        wrappers.mkdir()
        wrapper = wrappers / "git"
        wrapper.write_text(
            '#!/bin/sh\nif [ "$1" = rev-list ]; then\n'
            + "  printf %s " + shlex.quote(output) + "\n"
            + "  echo 'fixture history read failed' >&2\n  exit 128\nfi\n"
            + "exec " + shlex.quote(self.git_executable) + ' "$@"\n')
        wrapper.chmod(0o755)
        self.environment["PATH"] = str(wrappers) + os.pathsep + self.environment["PATH"]
        result = self.ensure_without_changes()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("cannot read missing sync worktree history", result.stderr)
        self.assertIn("fixture history read failed", result.stderr)
        self.no_recovery_advice(result)

    def test_missing_holder_empty_history_read_failure_preserves_registration(self):
        self.failed_missing_holder_history("")

    def test_missing_holder_partial_history_read_failure_is_not_unique_commit_advice(self):
        self.failed_missing_holder_history("partial-id\n")

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
