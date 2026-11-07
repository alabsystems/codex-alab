#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Daily cleanup respects live Cargo locks and the clone's preservation policy."""

import fcntl
import json
import os
from pathlib import Path
import plistlib
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'


class DailyTests(unittest.TestCase):
    def setUp(self):
        self.make_fixture()

    def make_fixture(self, *, repo=None, sync_name=".git/alab-sync"):
        # Keep every fixture and use a recording rm: even the old bug deletes nothing.
        self.repo = (repo or Path(tempfile.mkdtemp(prefix="alab-daily-test-"))).resolve()
        self.repo.mkdir(exist_ok=True)
        self.git("init", "-q", str(self.repo))
        (self.repo / ".gitignore").write_text("/codex-rs/target/\n")
        self.git("add", ".gitignore")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@invalid",
                 "commit", "-q", "--allow-empty", "-m", "fixture")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        scripts = self.repo / "alab"
        scripts.mkdir()
        for name in ["daily.sh", "lib.sh", "retention.py"]:
            shutil.copyfile(ROOT / "alab" / name, scripts / name)
        self.sync = self.repo / sync_name
        self.git("worktree", "add", "-q", "--detach", str(self.sync), "HEAD")
        self.pristine = Path(self.git("-C", str(self.sync), "rev-parse",
                                     "--path-format=absolute", "--git-path", "alab-pristine"))
        first = self.git("rev-parse", "HEAD")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@invalid",
                 "commit", "-q", "--allow-empty", "-m", "second upstream")
        second = self.git("rev-parse", "HEAD")
        self.revisions = [Path(str(self.pristine) + "-" + rev) for rev in (first, second)]
        for revision, path in zip((first, second), self.revisions):
            self.git("worktree", "add", "-q", "--detach", str(path), revision)
        self.targets = [tree / "codex-rs/target"
                        for tree in [self.sync, self.pristine, *self.revisions]]
        for target in self.targets:
            (target / "debug").mkdir(parents=True)
            (target / "debug/.cargo-lock").touch()
            (target / "keep-me").write_text("retained tiny build witness\n")
        self.bin = self.repo / "fake-bin"
        self.bin.mkdir()
        self.script("du", "#!/bin/sh\nprintf '42991616\\t%s\\n' \"$2\"\n")
        self.script("df", "#!/bin/sh\nprintf 'Filesystem 1024-blocks Used Available Capacity Mounted on\\nfixture 1048576 1048575 1 99%% /\\n'\n")
        for name in ["lsof", "ps"]:
            self.script(name, "#!/bin/sh\nexit 0\n")
        self.script("rm", PYTHON_ENTRY +
                    "import json, os, sys\n"
                    "with open(os.environ['DAILY_RM_LOG'], 'a') as out:\n"
                    "    out.write(json.dumps(sys.argv[1:]) + '\\n')\n")
        self.runs = 0

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.repo), *args], check=True,
                              capture_output=True, text=True).stdout.removesuffix("\n")

    def script(self, name, body):
        path = self.bin / name
        path.write_text(body)
        path.chmod(0o700)

    def cleanup(self, phase="reap", *, runner=None, sync_dir=None):
        self.runs += 1
        log = self.repo / f"rm-{self.runs}.jsonl"
        env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                   DAILY_RM_LOG=str(log), ALAB_TARGET_CAP_GB="40",
                   ALAB_KEEP_TARGET_GB="100", ALAB_PYTHON=sys.executable)
        env.pop("ALAB_SYNC_DIR", None)
        if sync_dir is not None:
            env["ALAB_SYNC_DIR"] = str(sync_dir)
        done = subprocess.run(["/bin/bash", str((runner or self.repo) / "alab/daily.sh"),
                               f"--{phase}-locked"], env=env, capture_output=True,
                              text=True, timeout=30)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
        return {str(target) for target in self.targets
                if any(str(target) in call for call in calls)}

    def reap(self):
        return self.cleanup()

    def test_reap_keeps_each_cargo_locked_target(self):
        for target in self.targets:
            with self.subTest(target=target):
                with (target / "debug/.cargo-lock").open("rb") as held:
                    fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    requested = self.reap()
                    self.assertNotIn(str(target), requested,
                                     f"a real Cargo flock must preserve {target}")
                    other = next(path for path in self.targets if path != target)
                    self.assertIn(str(other), requested, "idle over-cap control is reaped")
                self.assertEqual((target / "keep-me").read_text(),
                                 "retained tiny build witness\n")

    def test_reap_requests_legacy_and_all_registered_revision_targets(self):
        self.assertEqual(self.reap(), {str(target) for target in self.targets})
        for target in self.targets:
            self.assertTrue((target / "keep-me").exists(), "recording rm never deletes")

    def test_tidy_requests_legacy_and_all_registered_revision_targets_without_preservation(self):
        self.assertEqual(self.cleanup("tidy"), {str(target) for target in self.targets})
        for target in self.targets:
            self.assertTrue((target / "keep-me").exists(), "recording rm never deletes")

    def test_reap_and_tidy_keep_every_common_marker_kind_from_a_linked_worktree(self):
        scripts = self.sync / "alab"
        scripts.mkdir()
        for name in ["daily.sh", "lib.sh", "retention.py"]:
            shutil.copyfile(ROOT / "alab" / name, scripts / name)
        marker = self.repo / ".git/alab-preserve-builds"
        for kind in ["file", "directory", "dangling-symlink"]:
            with self.subTest(kind=kind):
                if kind == "file":
                    marker.write_text("preserve these fixture builds\n")
                elif kind == "directory":
                    marker.mkdir()
                else:
                    marker.symlink_to(self.repo / "missing-marker-target")
                try:
                    for phase in ["reap", "tidy"]:
                        with self.subTest(phase=phase):
                            self.assertEqual(self.cleanup(phase, runner=self.sync), set(),
                                             f"{phase} must honor {kind} in the common directory")
                    for target in self.targets:
                        self.assertEqual((target / "keep-me").read_text(),
                                         "retained tiny build witness\n")
                finally:
                    marker.rmdir() if kind == "directory" else marker.unlink()

    def test_reap_and_tidy_keep_targets_when_the_policy_helper_is_missing(self):
        helper = self.repo / "alab/retention.py"
        helper.rename(helper.with_suffix(".py.retained"))
        for phase in ["reap", "tidy"]:
            with self.subTest(phase=phase):
                self.assertEqual(self.cleanup(phase), set())

    def test_unregistered_revision_lookalikes_are_kept_even_without_preservation(self):
        lookalike = Path(str(self.pristine) + "-" + "f" * 40)
        target = lookalike / "codex-rs/target"
        target.mkdir(parents=True)
        (target / "keep-me").write_bytes(b"unknown plain target witness")
        for phase in ("reap", "tidy"):
            self.assertEqual(self.cleanup(phase), {str(item) for item in self.targets})
            log = self.repo / f"rm-{self.runs}.jsonl"
            self.assertNotIn(str(target), log.read_text())

    def test_newline_paths_keep_exact_tree_and_cleanup_boundaries(self):
        parent = Path(tempfile.mkdtemp(prefix="alab-newline-candidates-", suffix=".noindex")).resolve()
        self.make_fixture(repo=parent / "repo\nsplit", sync_name="sync\ncandidate")
        trees = [self.sync, self.pristine, *self.revisions]
        # A line-split reader would mistake these own plain targets for the candidates.
        poisons = []
        for fragment in {part for tree in trees for part in str(tree).split("\n")}:
            path = Path(fragment)
            if not path.is_absolute():
                path = self.repo / path
            path.relative_to(parent)
            target = path / "codex-rs/target"
            target.mkdir(parents=True, exist_ok=True)
            sentinel = target / "not-a-candidate"
            sentinel.write_bytes(b"plain split-path sentinel\n")
            poisons.append(sentinel)
        watched = [target / "keep-me" for target in self.targets] + poisons
        before = [(path.read_bytes(), path.stat().st_ino, path.stat().st_mtime_ns)
                  for path in watched]
        stream = subprocess.run(
            ["/bin/bash", "-c", 'set -euo pipefail; . "$1/alab/lib.sh"; alab_pristine_trees "$2"',
             "newline-tree-stream", str(self.repo), str(self.sync)],
            capture_output=True, timeout=60)
        self.assertEqual(stream.returncode, 0, stream.stderr)
        self.assertTrue(stream.stdout.endswith(b"\0"))
        decoded = [os.fsdecode(path) for path in stream.stdout.split(b"\0")[:-1]]
        self.assertEqual(set(decoded), {str(tree) for tree in trees[1:]})
        self.assertEqual(len(decoded), len(trees) - 1)

        def exact_calls():
            log = self.repo / f"rm-{self.runs}.jsonl"
            return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []

        for phase in ("reap", "tidy"):
            self.cleanup(phase, sync_dir=self.sync)
            calls = exact_calls()
            self.assertEqual({tuple(call) for call in calls},
                             {("-rf", str(target)) for target in self.targets})
            self.assertEqual(len(calls), len(self.targets))
        # The lock path must keep its newline too, before any deletion is requested.
        locked = self.targets[-1]
        with (locked / "debug/.cargo-lock").open("rb") as held:
            fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.cleanup(sync_dir=self.sync)
            self.assertEqual({tuple(call) for call in exact_calls()},
                             {("-rf", str(target)) for target in self.targets if target != locked})
        (self.repo / ".git/alab-preserve-builds").write_text("Preserve every newline world\n")
        for phase in ("reap", "tidy"):
            self.cleanup(phase, sync_dir=self.sync)
            self.assertEqual(exact_calls(), [])
        self.assertEqual([(path.read_bytes(), path.stat().st_ino, path.stat().st_mtime_ns)
                          for path in watched], before)

    def test_reap_and_tidy_keep_targets_when_the_policy_helper_is_unreadable(self):
        helper = self.repo / "alab/retention.py"
        helper.chmod(0)
        if os.access(helper, os.R_OK):
            self.skipTest("effective identity can read the mode-000 fixture helper")
        for phase in ["reap", "tidy"]:
            with self.subTest(phase=phase):
                self.assertEqual(self.cleanup(phase), set())

    def launchd_environment(self, limits):
        env = {key: value for key, value in os.environ.items()
               if not key.startswith("ALAB_")
               and key not in ("CARGO_BUILD_JOBS", "RUST_TEST_THREADS", "ATPKG_BIN")}
        env.update(limits)
        done = subprocess.run(["/bin/bash", str(self.repo / "alab/daily.sh"),
                               "--launchd-plist"], env=env, capture_output=True, timeout=60)
        self.assertEqual(done.returncode, 0, done.stderr.decode())
        return plistlib.loads(done.stdout)["EnvironmentVariables"]

    def test_launchd_keeps_supplied_compile_test_and_python_settings(self):
        limits = {"CARGO_BUILD_JOBS": "2", "RUST_TEST_THREADS": "3",
                  "ALAB_PYTHON": sys.executable}
        environment = self.launchd_environment(limits)
        for key, value in limits.items():
            self.assertEqual(environment.get(key), value)

    def test_launchd_keeps_explicit_cache_helpers_without_running_them(self):
        helpers = self.repo / 'helper & <trial> paths'
        package_bin = helpers / 'pkg\'s "bin"'
        package_bin.mkdir(parents=True)
        host = helpers / "aterm helper"
        called = self.repo / "helper-was-executed"
        body = f"#!/bin/sh\nprintf 'unexpected helper call\\n' >>{shlex.quote(str(called))}\nexit 97\n"
        for helper in (host, package_bin / "atpkg"):
            helper.write_text(body)
            helper.chmod(0o700)
        requested = {"ALAB_ATERM": str(host), "ATPKG_BIN": str(package_bin)}
        environment = self.launchd_environment(requested)
        for name, value in requested.items():
            self.assertTrue(Path(value).is_absolute())
            self.assertEqual(environment.get(name), value)
        self.assertFalse(called.exists(), "printing a job must not execute its helpers")
        self.assertTrue(all((target / "keep-me").is_file() for target in self.targets))

    def test_launchd_leaves_unallocated_limits_to_the_check_runner(self):
        environment = self.launchd_environment({})
        for key in ("CARGO_BUILD_JOBS", "RUST_TEST_THREADS", "ALAB_PYTHON",
                    "ALAB_ATERM", "ATPKG_BIN"):
            self.assertNotIn(key, environment)

    def test_sync_outcomes_are_journaled_without_notifications_or_callbacks(self):
        calls = self.repo / 'attention-calls'
        sync = self.repo / 'alab/sync.sh'
        sync.write_text('#!/bin/sh\nprintf "alab: fixture sync outcome\\n"\nexit "$DAILY_SYNC_EXIT"\n')
        sync.chmod(0o700)
        self.script('aterm', f'#!/bin/sh\nprintf "%s\\n" "$*" >> {shlex.quote(str(calls))}\nexit 0\n')
        status = self.repo / '.git/alab-daily.status'
        status.write_text('when=old exit=3 notified=old notice-pending=1 notice-post=1\n')
        callback = self.repo / 'legacy-callback'
        env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ['PATH'],
                   ALAB_NOTIFY=f'touch {shlex.quote(str(callback))}',
                   ALAB_PYTHON=sys.executable, ALAB_KEEP_TARGET_GB='0',
                   DAILY_RM_LOG=str(self.repo / 'journal-rm.jsonl'))
        for code in (3, 5, 0):
            env['DAILY_SYNC_EXIT'] = str(code)
            done = subprocess.run(['/bin/bash', str(self.repo / 'alab/daily.sh')],
                                  env=env, capture_output=True, text=True, timeout=60)
            self.assertEqual(done.returncode, code, done.stdout + done.stderr)
            fields = dict(field.split('=', 1) for field in status.read_text().split())
            self.assertEqual(set(fields), {'when', 'exit', 'origin', 'upstream', 'pending'})
            self.assertEqual(fields['exit'], str(code))
        self.assertIn('fixture sync outcome', (self.repo / '.git/alab-daily.log').read_text())
        self.assertFalse(callback.exists())
        if calls.exists():
            self.assertNotIn('notice', calls.read_text())

    def test_launchd_does_not_preserve_retired_attention_settings(self):
        environment = self.launchd_environment({'ALAB_NOTIFY': 'old callback',
                                               'ALAB_ESCALATE_HOURS': '1'})
        self.assertNotIn('ALAB_NOTIFY', environment)
        self.assertNotIn('ALAB_ESCALATE_HOURS', environment)


if __name__ == '__main__':
    unittest.main()
