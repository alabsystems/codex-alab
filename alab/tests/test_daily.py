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
from unittest.mock import patch


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
        for name in ["daily.sh", "daily_journal.py", "lib.sh", "retention.py", "pristine_state.py", "sync_worktree.py",
                     "qualification.py", "cli_status.py", "file_hash.py", "upstream_intake.py", "source_content.py"]:
            shutil.copyfile(ROOT / "alab" / name, scripts / name)
        self.sync = self.repo / sync_name
        self.git("worktree", "add", "-q", "--detach", str(self.sync), "HEAD")
        self.pristine = Path(self.git("-C", str(self.sync), "rev-parse",
                                     "--path-format=absolute", "--git-path", "alab-pristine"))
        first = self.git("rev-parse", "HEAD")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@invalid",
                 "commit", "-q", "--allow-empty", "-m", "second upstream")
        second = self.git("rev-parse", "HEAD")
        # Keep the legacy pristine cache only in a real linked checkout too.
        self.git("worktree", "add", "-q", "--detach", str(self.pristine), first)
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
                    "    args = sys.argv[1:]\n"
                    "    if args[:2] == ['-rf', '--']:\n"
                    "        args[2:] = [os.path.abspath(path) for path in args[2:]]\n"
                    "    out.write(json.dumps(args) + '\\n')\n")
        self.runs = 0

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.repo), *args], check=True,
                              capture_output=True, text=True).stdout.removesuffix("\n")

    def script(self, name, body):
        path = self.bin / name
        path.write_text(body)
        path.chmod(0o700)

    def removal_paths(self, log):
        operands = []
        for line in log.read_text().splitlines() if log.exists() else []:
            call = json.loads(line)
            if call[:2] == ["-f", "--"]:  # the daily journal capture's exit trap
                continue
            self.assertEqual(call[:2], ["-rf", "--"])
            self.assertGreater(len(call), 2)
            operands.extend(call[2:])
        return operands

    def assert_children_selected(self, targets):
        self.assertCountEqual(self.removals, [str(child) for target in targets for child in target.iterdir()])

    def cleanup(self, phase="reap", *, runner=None, sync_dir=None, expected_status=0):
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
        self.assertEqual(done.returncode, expected_status, done.stdout + done.stderr)
        self.removals = self.removal_paths(log)
        return {str(Path(operand).parent) for operand in self.removals}

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
        self.assert_children_selected(self.targets)
        for target in self.targets:
            self.assertTrue((target / "keep-me").exists(), "recording rm never deletes")

    def test_tidy_requests_legacy_and_all_registered_revision_targets_without_preservation(self):
        self.assertEqual(self.cleanup("tidy"), {str(target) for target in self.targets})
        self.assert_children_selected(self.targets)
        for target in self.targets:
            self.assertTrue((target / "keep-me").exists(), "recording rm never deletes")

    def test_reap_and_tidy_keep_every_common_marker_kind_from_a_linked_worktree(self):
        scripts = self.sync / "alab"
        scripts.mkdir()
        for name in ["daily.sh", "daily_journal.py", "lib.sh", "retention.py", "pristine_state.py", "sync_worktree.py",
                     "qualification.py", "cli_status.py", "file_hash.py", "upstream_intake.py", "source_content.py"]:
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

        for phase in ("reap", "tidy"):
            self.cleanup(phase, sync_dir=self.sync)
            self.assert_children_selected(self.targets)
        # The lock path must keep its newline too, before any deletion is requested.
        locked = self.targets[-1]
        with (locked / "debug/.cargo-lock").open("rb") as held:
            fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.cleanup(sync_dir=self.sync)
            self.assert_children_selected(self.targets[:-1])
        (self.repo / ".git/alab-preserve-builds").write_text("Preserve every newline world\n")
        for phase in ("reap", "tidy"):
            self.cleanup(phase, sync_dir=self.sync)
            self.assertEqual(self.removals, [])
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

    def test_cleanup_requires_a_registered_idle_checkout_of_the_expected_clone(self):
        foreign = self.repo / "foreign"
        self.git("init", "-q", str(foreign))
        (foreign / ".git/alab-preserve-builds").write_text("keep this clone's build\n")
        linked = []
        for kind in ("other-branch", "locked", "dirty", "pending", "merge"):
            path = self.repo / kind
            args = ["worktree", "add", "-q"]
            args.extend(["-b", kind] if kind == "other-branch" else ["--detach"])
            self.git(*args, str(path), "HEAD" if kind == "pending" else "origin/main")
            linked.append(path)
        self.git("worktree", "lock", str(linked[1]))
        (linked[2] / "unfinished").write_text("untracked work\n")
        marker = Path(self.git("-C", str(linked[4]), "rev-parse", "--path-format=absolute",
                               "--git-path", "MERGE_HEAD"))
        marker.symlink_to(self.repo / "missing-merge-state")
        for tree in (self.repo, foreign, *linked):
            target = tree / "codex-rs/target"
            target.mkdir(parents=True)
            witness = target / "keep-me"
            witness.write_bytes(b"preserved")
            for phase in ("reap", "tidy"):
                with self.subTest(tree=tree.name, phase=phase):
                    self.cleanup(phase, sync_dir=tree)
                    self.assertEqual(self.removals, [])
                    self.assertEqual(witness.read_bytes(), b"preserved")
        with patch.dict(os.environ, {"GIT_DIR": str(foreign / ".git")}):
            for phase in ("reap", "tidy"):
                self.cleanup(phase, sync_dir=foreign)
                self.assertEqual(self.removals, [])

    def test_unreadable_cleanup_evidence_preserves_targets(self):
        real_git = shlex.quote(shutil.which("git"))
        for operation in ("worktree list", "--absolute-git-dir", "merge-base --is-ancestor", "status --porcelain"):
            self.script("git", "#!/bin/sh\ncase \" $* \" in *" + shlex.quote(" " + operation + " ") +
                        "*) exit 128 ;; esac\nexec " + real_git + ' "$@"\n')
            for phase in ("reap", "tidy"):
                with self.subTest(operation=operation, phase=phase):
                    self.cleanup(phase)
                    self.assertEqual(self.removals, [])

    def test_relative_sync_dir_survives_daily_reentry_and_resource_stop(self):
        caller = self.repo / "invocation"
        caller.mkdir()
        actual = caller / "cache"
        decoy = self.repo / "cache"
        for tree in (actual, decoy):
            self.git("worktree", "add", "-q", "--detach", str(tree), "origin/main")
            (tree / "codex-rs/target").mkdir(parents=True)
            (tree / "codex-rs/target/keep-me").write_bytes(b"retained")
        sync = self.repo / "alab/sync.sh"
        sync.write_text('#!/bin/sh\nprintf "%s\\n%s\\n" "$ALAB_SYNC_DIR" "$*" >>"$SYNC_OBSERVED"\nexit 75\n')
        sync.chmod(0o700)
        observed = caller / "sync-arguments"
        calls = caller / "cleanup-arguments"
        env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                   ALAB_PYTHON=sys.executable, ALAB_SYNC_DIR="cache", DAILY_RM_LOG=str(calls),
                   SYNC_OBSERVED=str(observed), ALAB_TARGET_CAP_GB="40", ALAB_KEEP_TARGET_GB="100")
        result = subprocess.run(["/bin/bash", str(self.repo / "alab/daily.sh")], cwd=caller,
                                env=env, capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 75, result.stderr)
        self.assertEqual(observed.read_text().splitlines(), [str(actual), "--stepwise"])
        status = (self.repo / ".git/alab-daily.status").read_text()
        self.assertIn("exit=75", status)
        self.assertEqual(self.removal_paths(calls), [str(actual / "codex-rs/target/keep-me")] * 2)
        for tree in (actual, decoy):
            self.assertEqual((tree / "codex-rs/target/keep-me").read_bytes(), b"retained")
        plist = subprocess.run(["/bin/bash", str(self.repo / "alab/daily.sh"), "--launchd-plist"],
                               cwd=caller, env=env, capture_output=True, check=True, timeout=30)
        self.assertEqual(plistlib.loads(plist.stdout)["EnvironmentVariables"]["ALAB_SYNC_DIR"], str(actual))

    def test_foreign_git_environment_cannot_redirect_scheduled_writes(self):
        foreign = self.repo / "second clone"
        self.git("init", "-q", str(foreign))
        metadata = foreign / ".git"
        def contents():
            return {path.relative_to(metadata): path.read_bytes() for path in metadata.rglob("*")
                    if path.is_file()}
        before = contents()
        (self.repo / ".git/alab-preserve-builds").write_text("retain targets\n")
        observed = self.repo / "sync-environment"
        sync = self.repo / "alab/sync.sh"
        sync.write_text('#!/bin/sh\nset -eu\n{ git rev-parse --path-format=absolute --git-common-dir\n'
                        'git config --get http.version\nprintf "%s\\n" "$GIT_SSH_COMMAND" "$ALAB_ATERM"\n'
                        '} >"$SYNC_OBSERVED"\nexit 75\n')
        sync.chmod(0o700)
        env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                   GIT_DIR=str(metadata), GIT_COMMON_DIR=str(metadata), GIT_WORK_TREE=str(foreign),
                   GIT_INDEX_FILE=str(metadata / "index"), GIT_OBJECT_DIRECTORY=str(metadata / "objects"),
                   GIT_NAMESPACE="other", GIT_CONFIG_COUNT="1", GIT_CONFIG_KEY_0="http.version",
                   GIT_CONFIG_VALUE_0="HTTP/1.1", GIT_SSH_COMMAND="fixture ssh",
                   ALAB_ATERM="fixture helper", ALAB_PYTHON=sys.executable,
                   DAILY_RM_LOG=str(self.repo / "removal-log"), SYNC_OBSERVED=str(observed))
        done = subprocess.run(["/bin/bash", str(self.repo / "alab/daily.sh")], cwd=foreign,
                              env=env, capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 75, done.stderr)
        self.assertEqual(contents(), before)
        self.assertIn("exit=75", (self.repo / ".git/alab-daily.status").read_text())
        self.assertEqual(observed.read_text().splitlines(), [str(self.repo / ".git"), "HTTP/1.1",
                                                              "fixture ssh", "fixture helper"])

    def test_preservation_skips_scans_and_is_rechecked_before_removal(self):
        marker = self.repo / ".git/alab-preserve-builds"
        marker.write_text("keep all\n")
        calls = self.repo / "size-calls"
        for name in ("du", "lsof", "ps"):
            self.script(name, "#!/bin/sh\nprintf '%s\\n' " + name + " >>" + shlex.quote(str(calls))
                        + "\nexit 1\n")
        for phase in ("reap", "tidy"):
            self.cleanup(phase)
            self.assertEqual(self.removals, [])
        self.assertFalse(calls.exists())
        marker.rename(self.repo / "prior-preservation-marker")
        self.script("du", "#!/bin/sh\nprintf '%s\\n' keep >" + shlex.quote(str(marker))
                    + '\nprintf \'42991616\\t%s\\n\' "$2"\n')
        for name in ("lsof", "ps"):
            self.script(name, "#!/bin/sh\nexit 0\n")
        self.cleanup("reap")
        self.assertTrue(marker.exists())
        self.assertEqual(self.removals, [])

    def test_cleanup_preserves_an_unreadable_or_linked_cargo_lock(self):
        lock = self.targets[0] / "debug" / ".cargo-lock"
        retained = lock.with_name("retained.lock")
        lock.rename(retained)
        lock.symlink_to(retained)
        for phase in ("reap", "tidy"):
            with self.subTest(phase=phase):
                self.assertNotIn(str(self.targets[0]), self.cleanup(phase))
                self.assertTrue(lock.is_symlink())

    def test_pristine_cleanup_keeps_locked_attached_or_replaced_checkouts(self):
        # Leave every original file in place, including the first registration.
        self.git("worktree", "lock", str(self.revisions[0]))
        self.git("-C", str(self.revisions[1]), "switch", "-qc", "local-work")
        self.pristine.rename(str(self.pristine) + ".retained")
        self.pristine.mkdir()
        outside = self.repo / "unowned-pristine-target"
        outside.mkdir()
        (outside / "target").mkdir()
        (outside / "target/keep-me").write_text("separate checkout data")
        (self.pristine / "codex-rs").symlink_to(outside, target_is_directory=True)
        for phase in ("reap", "tidy"):
            with self.subTest(phase=phase):
                self.assertEqual(self.cleanup(phase), {str(self.targets[0])})
                self.assertEqual((outside / "target/keep-me").read_text(), "separate checkout data")

    def test_cleanup_keeps_target_below_a_linked_workspace(self):
        workspace = self.sync / "codex-rs"
        moved = self.repo / "separate workspace"
        workspace.rename(moved)
        workspace.symlink_to(moved, target_is_directory=True)
        witness = moved / "target/keep-me"
        before = witness.read_bytes()
        for phase in ("reap", "tidy"):
            with self.subTest(phase=phase):
                self.assertNotIn(str(self.targets[0]), self.cleanup(phase))
                self.assertEqual(witness.read_bytes(), before)

    def test_cleanup_keeps_committed_or_index_only_target_files(self):
        leaf = self.targets[0] / "keep-me"
        self.git("-C", str(self.sync), "add", "-f", str(leaf))
        # Also cover a clean tracked target and an entry staged for removal.
        for state in ("index", "committed", "staged removal"):
            if state == "committed":
                self.git("-C", str(self.sync), "-c", "user.name=Fixture", "-c",
                         "user.email=fixture@invalid", "commit", "-qm", "keep fixture in source")
                self.git("update-ref", "refs/remotes/origin/main",
                         self.git("-C", str(self.sync), "rev-parse", "HEAD"))
            if state == "staged removal":
                self.git("-C", str(self.sync), "rm", "--cached", str(leaf))
            for phase in ("reap", "tidy"):
                with self.subTest(state=state, phase=phase):
                    self.assertNotIn(str(self.targets[0]), self.cleanup(phase))
                    self.assertTrue(leaf.is_file())

    def test_cleanup_keeps_independent_repository_at_target(self):
        target = self.targets[0]
        self.make_nested_repo(target)
        for phase in ("reap", "tidy"):
            self.assertNotIn(str(target), self.cleanup(phase))

    def test_cleanup_keeps_a_checkout_nested_inside_an_ignored_target(self):
        target = self.targets[0]
        nested = target / "debug" / "independent"
        self.make_nested_repo(nested)
        for phase in ("reap", "tidy"):
            self.assertNotIn(str(target), self.cleanup(phase))

    def test_cleanup_keeps_nested_bare_and_case_aliased_git_metadata(self):
        upper = self.targets[0] / "debug" / "working"
        self.make_nested_repo(upper)
        bare = self.targets[-1] / "debug" / "mirror"
        self.git("clone", "-q", "--bare", str(upper), str(bare))
        (upper / ".git").rename(upper / ".GIT")
        for phase in ("reap", "tidy"):
            with self.subTest(phase=phase):
                self.assertEqual(self.cleanup(phase), {str(path) for path in self.targets[1:-1]})

    def test_reap_keeps_replacement_repository_after_process_use_scan(self):
        original = self.sync
        retained = self.repo / "retained-original"
        replacement = self.repo / "other-clone"
        self.git("clone", "-q", "--local", str(self.repo), str(replacement))
        witness = replacement / "codex-rs/target/keep-me"
        witness.parent.mkdir(parents=True)
        witness.write_bytes(b"private other clone")
        self.script("lsof", PYTHON_ENTRY +
                    "from pathlib import Path\n" +
                    f"original, retained, other = map(Path, {list(map(str, (original, retained, replacement)))!r})\n" +
                    "if other.exists():\n    original.rename(retained)\n    other.rename(original)\n")
        self.assertNotIn(str(self.targets[0]), self.cleanup("reap"))
        self.assertEqual((original / "codex-rs/target/keep-me").read_bytes(), b"private other clone")

    def test_replacement_after_final_policy_probe_keeps_both_directories(self):
        helper = self.repo / "alab/retention.py"
        saved = helper.with_name("original-policy.py")
        helper.rename(saved)
        helper.write_text(
            "import os, subprocess, sys\nfrom pathlib import Path\n"
            f"result = subprocess.run([sys.executable, '-IB', {str(saved)!r}, *sys.argv[1:]])\n"
            "target, retained, other = map(Path, (os.environ[k] for k in ('TARGET', 'RETAINED', 'OTHER')))\n"
            "if result.returncode == 0 and Path.cwd() == target.parent and other.exists():\n"
            "    target.rename(retained)\n    other.rename(target)\nsys.exit(result.returncode)\n")
        for index, phase in enumerate(("reap", "tidy")):
            target = self.targets[index]
            retained = self.repo / f"original-{phase}"
            other = self.repo / f"foreign-{phase}"
            self.make_nested_repo(other)
            with patch.dict(os.environ, {"TARGET": str(target), "RETAINED": str(retained), "OTHER": str(other)}):
                self.cleanup(phase)
            self.assert_children_selected(self.targets[index + 1:])
            self.assertEqual((target / "keep-me").read_text(), "independent work")
            self.assertTrue((retained / "debug/.cargo-lock").is_file())

    def test_rename_at_removal_keeps_every_operand_in_the_opened_directory(self):
        target = self.targets[0]
        retained = self.repo / "retained-target"
        replacement = self.repo / "replacement"
        self.make_nested_repo(replacement)
        record = self.bin / "record-removal"
        (self.bin / "rm").rename(record)
        self.script("rm", PYTHON_ENTRY +
                    "from pathlib import Path\nimport runpy\n"
                    f"target, retained, other = map(Path, {list(map(str, (target, retained, replacement)))!r})\n"
                    "if other.exists():\n    target.rename(retained)\n    other.rename(target)\n"
                    f"runpy.run_path({str(record)!r}, run_name='__main__')\n")
        self.cleanup("reap")
        self.assert_children_selected([retained, *self.targets[1:]])
        self.assertEqual((target / "keep-me").read_text(), "independent work")

    def test_child_operands_preserve_names_across_batches_and_report_rm_failure(self):
        target = self.targets[0]
        for name in [".hidden", "--", "*?[x]", "line\nbreak", *(f"output-{i}" for i in range(260))]:
            (target / name).touch()
        # Some filesystems only accept valid Unicode. Exercise a non-UTF-8
        # operand too when the underlying fixture can actually represent it.
        try:
            (target / os.fsdecode(b"byte-\xff")).touch()
        except OSError as error:
            import errno
            if error.errno not in (errno.EINVAL, errno.EILSEQ):
                raise
        self.cleanup("reap")
        self.assert_children_selected(self.targets)
        with (self.bin / "rm").open("a") as script:
            script.write("sys.exit(37)\n")
        self.cleanup("reap", expected_status=37)
        self.assertTrue(self.removals)
        self.assertTrue(all(Path(child).parent == target for child in self.removals))

    def make_nested_repo(self, nested):
        self.git("init", "-q", str(nested))
        if not (nested / "keep-me").exists():
            (nested / "keep-me").write_text("independent work")
        self.git("-C", str(nested), "add", "keep-me")
        self.git("-C", str(nested), "-c", "user.name=Fixture", "-c",
                 "user.email=fixture@invalid", "commit", "-qm", "independent work")

    def test_case_alias_to_tracked_content_is_preserved(self):
        target = self.targets[0]
        alias = target.with_name("Target")
        if not alias.is_dir() or not target.samefile(alias):
            self.skipTest("the fixture volume distinguishes filename case")
        entry = "codex-rs/Target/keep-me"
        blob = self.git("-C", str(self.sync), "hash-object", "-w", str(target / "keep-me"))
        self.git("-C", str(self.sync), "update-index", "--add", "--cacheinfo", "100644", blob, entry)
        self.assertIn(entry, self.git("-C", str(self.sync), "ls-files").splitlines())
        self.git("-C", str(self.sync), "-c", "user.name=Fixture", "-c",
                 "user.email=fixture@invalid", "commit", "-qm", "retain case alias")
        self.git("update-ref", "refs/remotes/origin/main", self.git("-C", str(self.sync), "rev-parse", "HEAD"))
        for phase in ("reap", "tidy"):
            self.assertNotIn(str(target), self.cleanup(phase))

    def launchd_environment(self, limits):
        env = {key: value for key, value in os.environ.items()
               if not key.startswith("ALAB_")
               and key not in ("CARGO_BUILD_JOBS", "CARGO_TARGET_DIR", "RUST_TEST_THREADS", "ATPKG_BIN")}
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

    def test_launchd_keeps_explicit_helpers_without_running_them(self):
        helpers = self.repo / 'helper & <trial> paths'
        package_bin = helpers / 'pkg\'s "bin"'
        package_bin.mkdir(parents=True)
        host = helpers / "aterm helper"
        publisher = helpers / "qualified publisher"
        called = self.repo / "helper-was-executed"
        body = f"#!/bin/sh\nprintf 'unexpected helper call\\n' >>{shlex.quote(str(called))}\nexit 97\n"
        for helper in (host, publisher, package_bin / "atpkg"):
            helper.write_text(body)
            helper.chmod(0o700)
        requested = {"ALAB_ATERM": str(host), "ALAB_ATPKG_PUBLISH": str(publisher),
                     "ATPKG_BIN": str(package_bin)}
        environment = self.launchd_environment(requested)
        for name, value in requested.items():
            self.assertTrue(Path(value).is_absolute())
            self.assertEqual(environment.get(name), value)
        self.assertFalse(called.exists(), "printing a job must not execute its helpers")
        self.assertTrue(all((target / "keep-me").is_file() for target in self.targets))

    def test_launchd_carries_external_build_target_without_giving_cleanup_ownership(self):
        target = self.repo / "external target & cache"
        target.mkdir()
        witness = target / "keep-me"
        witness.write_bytes(b"external build target witness\n")
        environment = self.launchd_environment({"CARGO_TARGET_DIR": str(target)})
        self.assertEqual(environment.get("CARGO_TARGET_DIR"), str(target))
        with patch.dict(os.environ, {"CARGO_TARGET_DIR": environment["CARGO_TARGET_DIR"]}):
            for phase in ("reap", "tidy"):
                with self.subTest(phase=phase):
                    self.cleanup(phase)
                    self.assert_children_selected(self.targets)
        self.assertEqual(witness.read_bytes(), b"external build target witness\n")

    def test_launchd_leaves_unallocated_limits_to_the_check_runner(self):
        environment = self.launchd_environment({})
        for key in ("CARGO_BUILD_JOBS", "CARGO_TARGET_DIR", "RUST_TEST_THREADS", "ALAB_PYTHON",
                    "ALAB_ATERM", "ALAB_ATPKG_PUBLISH", "ATPKG_BIN"):
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
