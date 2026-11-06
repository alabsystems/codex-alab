#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Clone-local preservation applies to packages, scratch trees and all worktrees."""

import json
import os
from pathlib import Path
import runpy
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
POLICY = runpy.run_path(str(ROOT / "alab/retention.py"))


@unittest.skipUnless(Path("/usr/bin/sandbox-exec").is_file(), "requires macOS sandbox-exec")
class ContainedScratchRetentionTests(unittest.TestCase):
    def run_contained(self, *, preserve, mode="command", unknown=False, marked_cwd=False):
        # Keep the fixture and contained TMPDIR for inspection. Even the normal-cleanup
        # control intercepts rm: this regression must never delete its evidence.
        directory = Path(tempfile.mkdtemp(prefix="alab-contained-retention-"))
        repo = directory / "driver"
        tooling = repo / "alab"
        tooling.mkdir(parents=True)
        for name in ("lib.sh",) if unknown else ("lib.sh", "retention.py"):
            shutil.copyfile(ROOT / "alab" / name, tooling / name)
        subprocess.run(["git", "init", "-q", str(repo)], check=True, capture_output=True)
        if preserve:
            (repo / ".git/alab-preserve-builds").write_text("Keep all build scratch\n")
        neutral = directory / "neutral"
        neutral.mkdir()
        sentinel = neutral / "keep-me"
        sentinel.write_bytes(b"the working directory is not scratch\n")
        if marked_cwd:
            subprocess.run(["git", "init", "-q", str(neutral)], check=True,
                           capture_output=True)
            (neutral / ".git/alab-preserve-builds").write_text("Another clone's policy\n")
        cargo_home = directory / "cargo-home"
        cargo_home.mkdir()
        (cargo_home / ".alab-seeded").touch()
        output = directory / "output"
        output.mkdir()
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith("ALAB_")}
        environment.update(ALAB_CARGO_HOME=str(cargo_home), ALAB_PYTHON=sys.executable)
        command = r'''
set -euo pipefail
cd "$1"
. alab/lib.sh
cd "$2"
allocated="$3/allocated"
removed="$3/removed"
mode="$4"
rm() { printf '%s\n' "$@" >>"$removed"; }
case "$mode" in
  mktemp-failure) mktemp() { printf '%s\n' "$PWD"; return 73; } ;;
  mktemp-empty) mktemp() { return 0; } ;;
  mktemp-cwd) mktemp() { printf '%s\n' "$PWD"; } ;;
esac
if [[ "$mode" == mktemp-* ]]; then
  if alab_real '' >/dev/null; then exit 91; fi
  if (cd() { return 73; }; alab_real "$PWD") >/dev/null; then exit 92; fi
  dirname_failed="$3/output/dirname-failed"
  if (
    dirname() {
      if [[ ! -e "$dirname_failed" ]]; then : >"$dirname_failed"; return 73; fi
      /usr/bin/dirname "$@"
    }
    alab_real "$PWD/missing-directory"
  ) >/dev/null; then exit 93; fi
fi
eval "$(declare -f alab_contain_profile | sed '1s/alab_contain_profile/alab_original_profile/')"
alab_contain_profile() {
  printf '%s\n' "$1" >"$allocated"
  [[ "$mode" != profile-failure ]] || return 71
  alab_original_profile "$@"
}
rc=0
alab_contained "$3/output" -- /bin/sh -c \
  'printf ran >"$2/command-ran"; printf witness >"$TMPDIR/witness"; exit "$1"' \
  contained-retention "$5" "$3/output" || rc=$?
exit "$rc"
'''
        exit_code = 42 if mode == "failed-command" else 0
        done = subprocess.run(["/bin/bash", "-c", command, "contained-retention", str(repo),
                               str(neutral), str(directory), mode, str(exit_code)],
                              env=environment, capture_output=True, text=True, timeout=60)
        allocation_failure = mode.startswith("mktemp-")
        self.assertEqual(done.returncode,
                         1 if mode == "profile-failure" or allocation_failure else exit_code,
                         done.stderr)
        removed = directory / "removed"
        self.assertEqual(sentinel.read_bytes(), b"the working directory is not scratch\n")
        if allocation_failure:
            self.assertFalse((directory / "allocated").exists(), "no profile after failed admission")
            self.assertFalse((output / "command-ran").exists(), "no command after failed admission")
            self.assertFalse(removed.exists(), "no cleanup after failed admission")
            return None, []
        scratch = Path((directory / "allocated").read_text().removesuffix("\n"))
        self.assertTrue(scratch.is_dir(), f"retained evidence: {directory}")
        if mode != "profile-failure":
            self.assertEqual((scratch / "witness").read_bytes(), b"witness")
            self.assertEqual((output / "command-ran").read_bytes(), b"ran")
        return scratch, removed.read_text().splitlines() if removed.exists() else []

    def test_driver_policy_keeps_inner_scratch_after_success_failure_and_profile_error(self):
        for mode in ("command", "failed-command", "profile-failure"):
            with self.subTest(mode=mode):
                _, removed = self.run_contained(preserve=True, mode=mode)
                self.assertEqual(removed, [], "the driver's policy also owns a neutral cwd")
        for mode in ("mktemp-failure", "mktemp-empty", "mktemp-cwd"):
            with self.subTest(mode=mode):
                _, removed = self.run_contained(preserve=False, mode=mode)
                self.assertEqual(removed, [], "allocation failure gives cleanup no path")

    def test_unknown_policy_preserves_and_another_clones_marker_does_not_change_ownership(self):
        _, removed = self.run_contained(preserve=False, unknown=True)
        self.assertEqual(removed, [], "an unavailable retention helper cannot authorize removal")
        scratch, removed = self.run_contained(preserve=False, marked_cwd=True)
        self.assertEqual(removed, ["-rf", str(scratch)])


class GateScratchRetentionTests(unittest.TestCase):
    def run_gate(self, policy):
        # Retain every fixture. Even the unmarked control only records rm requests.
        directory = Path(tempfile.mkdtemp(
            prefix="alab-gate-retention ",
            dir=os.environ.get("ALAB_GATE_RETENTION_FIXTURES")))
        print(f"retained gate fixture: {directory}", flush=True)
        repo = directory / "driver"
        tooling = repo / "alab"
        tooling.mkdir(parents=True)
        for name in ("check.sh", "lib.sh", "retention.py", "unit-tests.sh"):
            if name == "retention.py" and policy == "missing":
                continue
            with (tooling / name).open("xb") as output:
                output.write((ROOT / "alab" / name).read_bytes())
        (repo / "codex-rs").mkdir()
        (repo / ".git").mkdir()
        if policy == "marked":
            with (repo / ".git/alab-preserve-builds").open("x") as marker:
                marker.write("Keep the gate's scratch\n")
        for name in ("bin", "home", "tmp"):
            (directory / name).mkdir()
        removed = directory / "rm.jsonl"
        removed.touch(exist_ok=False)
        stub = directory / "stub.py"
        with stub.open("x") as output:
            output.write('''import json
import os
from pathlib import Path
import sys

kind, *args = sys.argv[1:]
if kind == "cargo":
    with open(os.environ["GATE_CARGO_LOG"], "x") as output:
        json.dump({"args": args, "codex_home": os.environ["CODEX_HOME"]}, output)
    raise SystemExit(9)  # The public gate fails before any compiler can run.
if kind == "rm":
    with open(os.environ["GATE_RM_LOG"], "a") as output:
        output.write(json.dumps(args) + "\\n")
    raise SystemExit(0)
if kind == "git":
    repo = os.environ["GATE_REPO"]
    if args == ["-C", repo, "rev-parse", "--path-format=absolute", "--git-common-dir"]:
        if os.environ["GATE_POLICY"] == "git-error":
            raise SystemExit(71)
        sys.stdout.buffer.write(os.fsencode(Path(repo) / ".git") + b"\\n")
    elif args in (["rev-parse", "-q", "--verify", "HEAD^{commit}"],
                   ["rev-parse", "HEAD"], ["rev-parse", "HEAD^{commit}"]):
        print("1" * 40)
    else:
        raise SystemExit(72)
''')
        for name in ("cargo", "rm", "git"):
            program = directory / "bin" / name
            with program.open("x") as output:
                output.write(f"#!/bin/sh\nexec {shlex.quote(sys.executable)} -B -I "
                             f"{shlex.quote(str(stub))} {name} \"$@\"\n")
            program.chmod(0o700)
        environment = {
            "HOME": str(directory / "home"),
            "PATH": f"{directory / 'bin'}:/usr/bin:/bin:/usr/sbin:/sbin",
            "TMPDIR": str(directory / "tmp"),
            "LANG": "en_US.UTF-8",
            "ALAB_CARGO": str(directory / "bin/cargo"),
            "ALAB_PYTHON": sys.executable,
            "ALAB_CHECK_BASE": "HEAD",
            "GATE_REPO": str(repo),
            "GATE_POLICY": policy,
            "GATE_CARGO_LOG": str(directory / "cargo.json"),
            "GATE_RM_LOG": str(removed),
        }
        command = ["/bin/bash", "-c", '. "$1"; alab_build_command() { "$@"; }; check_main', "gate-body", str(tooling / "check.sh")]
        done = subprocess.run(command, env=environment, capture_output=True, text=True,
                              timeout=30)
        for name, contents in (("stdout.txt", done.stdout), ("stderr.txt", done.stderr)):
            with (directory / name).open("x") as output:
                output.write(contents)
        with (directory / "result.json").open("x") as output:
            json.dump({"command": command, "policy": policy,
                       "gate_exit": done.returncode}, output, indent=2)
        self.assertEqual(done.returncode, 1, done.stderr)
        cargo = json.loads((directory / "cargo.json").read_text())
        self.assertEqual(cargo["args"], ["metadata", "--locked", "--no-deps", "--format-version", "1"])
        codex_home = Path(cargo["codex_home"])
        scratch = codex_home.parent
        self.assertEqual(scratch.parent.resolve(), Path("/tmp/.ubd").resolve())
        self.assertTrue(scratch.name.startswith("alab."))
        self.assertEqual(codex_home.name, "codex-home")
        self.assertTrue(codex_home.is_dir(), f"retained evidence: {directory}")
        return scratch, [json.loads(line) for line in removed.read_text().splitlines()], done

    def test_gate_keeps_scratch_when_preservation_is_marked_or_unknown(self):
        for policy in ("marked", "missing", "git-error"):
            with self.subTest(policy=policy):
                _, removed, done = self.run_gate(policy)
                self.assertEqual(removed, [], "unknown policy cannot authorize gate cleanup")
                self.assertIn("kept gate scratch", done.stderr)

    def test_unmarked_gate_cleanup_requests_only_its_scratch(self):
        scratch, removed, _ = self.run_gate("unmarked")
        self.assertEqual(removed, [["-rf", str(scratch)]])
        self.assertTrue(scratch.is_dir(), "the cleanup recorder never removes evidence")


class PristineRetentionTests(unittest.TestCase):
    def setUp(self):
        # Plain source and text target witnesses only. Keep every Git fixture.
        self.directory = Path(tempfile.mkdtemp(prefix="alab-pristine-revisions-",
                                             suffix=".noindex")).resolve()
        print(f"retained pristine fixture: {self.directory}", flush=True)
        self.repo = self.directory / "checkout"
        self.repo.mkdir()
        self.environment = {
            "HOME": str(self.directory / "home"), "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_OPTIONAL_LOCKS": "0", "ALAB_PYTHON": sys.executable,
        }
        Path(self.environment["HOME"]).mkdir()
        self.git("init", "-q")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@invalid")
        (self.repo / ".gitignore").write_text("/codex-rs/target/\n")
        (self.repo / "source").write_text("first upstream source\n")
        self.git("add", ".")
        self.git("commit", "-q", "-m", "first")
        self.first = self.git("rev-parse", "HEAD")
        (self.repo / "source").write_text("second upstream source\n")
        self.git("commit", "-q", "-am", "second")
        self.second = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/upstream/main", self.second)
        self.base = Path(self.git("rev-parse", "--path-format=absolute", "--git-path",
                                  "alab-pristine"))
        self.marker = self.repo / ".git/alab-preserve-builds"
        self.marker.write_text("Keep every source and build world\n")

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.repo), *args], env=self.environment,
                              check=True, capture_output=True, text=True,
                              timeout=60).stdout.removesuffix("\n")

    def shell(self, body, *args):
        return subprocess.run(
            ["/bin/bash", "-c", 'set -euo pipefail; . "$1/alab/lib.sh"; shift; ' + body,
             "pristine-revisions", str(ROOT), str(self.repo), *map(str, args)],
            cwd=self.repo, env=self.environment, capture_output=True, text=True, timeout=60)

    def path(self, revision):
        return Path(str(self.base) + "-" + revision)

    def prepare(self, revision):
        done = self.shell('alab_pristine_at "$1" "$2"; alab_pristine "$1" "$2"', revision)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.removesuffix("\n"), str(self.path(revision)))
        return self.path(revision)

    def witness(self, tree):
        target = tree / "codex-rs/target"
        target.mkdir(parents=True)
        (target / "keep-me").write_bytes(b"retained text build witness\n")
        return target / "keep-me"

    def snapshot(self, paths):
        return [(path.read_bytes(), path.stat().st_dev, path.stat().st_ino,
                 path.stat().st_mtime_ns) for path in paths]

    def test_two_upstream_bases_advance_without_changing_any_retained_world(self):
        self.git("worktree", "add", "-q", "--detach", str(self.base), self.first)
        legacy_witness = self.witness(self.base)
        first = self.prepare(self.first)
        first_witness = self.witness(first)
        watched = [self.base / "source", legacy_witness, first / "source", first_witness,
                   self.marker]
        before = self.snapshot(watched)
        second = self.prepare(self.second)
        self.assertEqual((second / "source").read_text(), "second upstream source\n")
        self.assertEqual(self.prepare(self.first), first)
        self.assertEqual(self.snapshot(watched), before)
        self.assertNotEqual(first, second)
        self.assertEqual(self.git("-C", str(first), "rev-parse", "HEAD"), self.first)
        self.assertEqual(self.git("-C", str(self.base), "rev-parse", "HEAD"), self.first)

    def test_dirty_or_wrong_revision_paths_refuse_without_repair(self):
        for kind in ("tracked", "staged", "untracked", "wrong-head"):
            with self.subTest(kind=kind):
                # Distinct linked checkout gives each hostile case an independent key.
                checkout = self.directory / kind
                self.git("worktree", "add", "-q", "--detach", str(checkout), self.first)
                done = self.shell('alab_pristine_at "$2" "$3"; alab_pristine "$2" "$3"',
                                  checkout, self.first)
                self.assertEqual(done.returncode, 0, done.stderr)
                tree = Path(done.stdout.removesuffix("\n"))
                if kind == "wrong-head":
                    self.git("-C", str(tree), "checkout", "-q", "--detach", self.second)
                elif kind == "untracked":
                    (tree / "untracked-source").write_text("do not overwrite\n")
                else:
                    (tree / "source").write_text("do not overwrite\n")
                    if kind == "staged":
                        self.git("-C", str(tree), "add", "source")
                watched = [tree / "source", tree / ".git"]
                before = self.snapshot(watched)
                refused = self.shell('alab_pristine_at "$2" "$3"', checkout, self.first)
                self.assertEqual(refused.returncode, 1, refused.stderr)
                self.assertIn("kept pristine tree", refused.stderr)
                self.assertEqual(self.snapshot(watched), before)

    def test_occupied_nonrepo_and_symlink_destinations_are_never_replaced(self):
        for kind in ("directory", "broken-link", "live-link"):
            with self.subTest(kind=kind):
                checkout = self.directory / kind
                self.git("worktree", "add", "-q", "--detach", str(checkout), self.first)
                named = self.shell('alab_pristine "$2" "$3"', checkout, self.first)
                self.assertEqual(named.returncode, 0, named.stderr)
                path = Path(named.stdout.removesuffix("\n"))
                if kind == "directory":
                    path.mkdir()
                    (path / "sentinel").write_bytes(b"existing plain data")
                else:
                    path.symlink_to(self.directory / "absent" if kind == "broken-link" else self.repo)
                before = path.lstat()
                log = self.directory / (kind + "-git-probes")
                refused = self.shell('probe_log="$4"; git() { printf "%s\\n" "$*" >>"$probe_log"; '
                                     'command git "$@"; }; alab_pristine_at "$2" "$3"',
                                     checkout, self.first, log)
                self.assertEqual(refused.returncode, 1, refused.stderr)
                self.assertEqual(path.lstat().st_ino, before.st_ino)
                self.assertNotIn(f"-C {path} ", log.read_text(), "do not probe through refused paths")
                if kind == "directory":
                    self.assertEqual((path / "sentinel").read_bytes(), b"existing plain data")
                else:
                    self.assertTrue(path.is_symlink())

    @unittest.skipUnless(Path("/usr/bin/sandbox-exec").is_file(), "requires macOS sandbox-exec")
    def test_contained_exact_revision_reuse_needs_no_git_write(self):
        tree = self.prepare(self.first)
        target = self.witness(tree).parent
        scratch = self.directory / "scratch"
        cargo_home = self.directory / "cargo-home"
        scratch.mkdir()
        cargo_home.mkdir()
        profile = self.shell('alab_contain_profile "$2" "$3" --pristine "$4" '
                             '--read "$5/alab" "$6"', scratch, cargo_home, tree, ROOT, target)
        self.assertEqual(profile.returncode, 0, profile.stderr)
        # No alab_contained process-copy preparation: run the system shell directly.
        command = 'set -euo pipefail; . "$1/alab/lib.sh"; alab_pristine_at "$2" "$3"; '
        command += 'alab_pristine "$2" "$3"; '
        command += 'if printf bad >"$2/.git/not-admitted"; then exit 91; fi'
        done = subprocess.run(
            ["/usr/bin/sandbox-exec", "-p", profile.stdout, "/bin/bash", "-c", command,
             "contained-pristine-reuse", str(ROOT), str(self.repo), self.first],
            cwd=self.repo, env=self.environment, capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.removesuffix("\n"), str(tree))
        self.assertFalse((self.repo / ".git/not-admitted").exists())

    def test_same_revision_paths_are_distinct_between_checkouts(self):
        other = self.directory / "other-checkout"
        self.git("worktree", "add", "-q", "--detach", str(other), self.first)
        first = self.prepare(self.first)
        done = self.shell('alab_pristine_at "$2" "$3"; alab_pristine "$2" "$3"',
                          other, self.first)
        self.assertEqual(done.returncode, 0, done.stderr)
        second = Path(done.stdout.removesuffix("\n"))
        self.assertNotEqual(first, second)
        self.assertEqual((second / "source").read_bytes(), (first / "source").read_bytes())

    def test_failed_add_keeps_stale_registration_and_existing_world(self):
        tree = self.prepare(self.first)
        witness = self.witness(tree)
        retained = self.directory / "retained-stale-world"
        tree.rename(retained)
        before = self.snapshot([retained / "source", retained / witness.relative_to(tree)])
        done = self.shell('alab_pristine_at "$1" "$2"', self.first)
        self.assertNotEqual(done.returncode, 0, done.stderr)
        self.assertFalse(tree.exists(), "ordinary add cannot force through stale registration")
        self.assertEqual(self.snapshot([retained / "source", retained / witness.relative_to(tree)]), before)
        self.assertIn(str(tree), self.git("worktree", "list", "--porcelain"))

    def test_actual_check_selects_the_resolved_upstream_base(self):
        first = self.prepare(self.first)
        witness = self.witness(first)
        before = self.snapshot([first / "source", witness])
        source = (ROOT / "alab/check.sh").read_text()
        function = 'pristine_upstream() {' + source.split(
            'pristine_upstream() {', 1)[1].split('\n}\n', 1)[0] + '\n}\n'
        done = self.shell(function + 'repo_root="$1"; pristine_upstream; printf "%s\\n" "$pristine"')
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.removesuffix("\n"), str(self.path(self.second)))
        self.assertEqual(self.snapshot([first / "source", witness]), before)


class RetentionTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix="alab-retention-")
        self.addCleanup(scratch.cleanup)
        self.repo = Path(scratch.name).resolve()
        self.git("init", "-q", str(self.repo))
        (self.repo / "alab").mkdir()
        shutil.copyfile(ROOT / "alab/retention.py", self.repo / "alab/retention.py")
        self.marker = self.repo / ".git/alab-preserve-builds"

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.repo), *args],
                              check=True, capture_output=True, text=True).stdout.strip()

    def test_presence_is_shared_by_linked_worktrees_without_reading_marker_contents(self):
        self.assertTrue(POLICY["cleanup_allowed"](self.repo))
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@invalid",
                 "commit", "-q", "--allow-empty", "-m", "fixture")
        linked = self.repo / "linked"
        self.git("worktree", "add", "--detach", str(linked), "HEAD")
        for kind in ["file", "directory", "symlink", "fifo"]:
            with self.subTest(kind=kind):
                if kind == "directory":
                    self.marker.mkdir()
                elif kind == "symlink":
                    self.marker.symlink_to(self.repo / "absent target")
                elif kind == "fifo":
                    os.mkfifo(self.marker)
                else:
                    self.marker.write_text("Explicit preservation requested by the user\n")
                try:
                    self.assertFalse(POLICY["cleanup_allowed"](self.repo))
                    self.assertFalse(POLICY["cleanup_allowed"](linked))
                finally:
                    if kind == "directory":
                        self.marker.rmdir()
                    else:
                        self.marker.unlink()
        self.assertTrue(POLICY["cleanup_allowed"](linked))

        # An external common directory can end in a space. Its exact name,
        # rather than an existing trimmed sibling, owns the policy.
        external = self.repo / "external checkout"
        common = self.repo / "external metadata "
        common.with_name("external metadata").mkdir()
        self.git("init", "-q", "--separate-git-dir", str(common), str(external))
        self.assertTrue(POLICY["cleanup_allowed"](external))
        (common / POLICY["MARKER"]).write_text("Preserve this external clone's builds\n")
        self.assertFalse(POLICY["cleanup_allowed"](external))
        (common / POLICY["MARKER"]).unlink()
        self.assertTrue(POLICY["cleanup_allowed"](external))

    def test_unknown_clone_policy_fails_closed(self):
        self.assertFalse(POLICY["cleanup_allowed"](self.repo / "does not exist"))
        regular = self.repo / "metadata file"
        regular.write_text("not a common directory\n")
        for path in [self.repo / "missing metadata", regular]:
            with self.subTest(path=path):
                reply = subprocess.CompletedProcess([], 0, os.fsencode(path) + b"\n", b"")
                with mock.patch.object(POLICY["subprocess"], "run", return_value=reply):
                    self.assertFalse(POLICY["cleanup_allowed"](self.repo))
        reply = subprocess.CompletedProcess([], 0, os.fsencode(self.repo / ".git"), b"")
        with mock.patch.object(POLICY["subprocess"], "run", return_value=reply):
            self.assertFalse(POLICY["cleanup_allowed"](self.repo))

    def test_actual_package_cleanup_and_copy_keep_scratch_under_preservation(self):
        shutil.copyfile(ROOT / "alab/copy_package.py", self.repo / "alab/copy_package.py")
        self.marker.write_text("Preserve builds and staging evidence\n")
        scratch = self.repo / "scratch"
        scratch.mkdir()
        (scratch / "diagnostic").write_text("useful evidence\n")
        worktree = self.repo / "run worktree"
        worktree.mkdir()
        (worktree / "compiled").write_text("generated output\n")
        staged = self.repo / "copied staging"
        staged.mkdir()
        command = '''
. "$1/alab/package.sh"
repo_root="$2"
python="$3"
wt="$4"
wt_made=1
scratch_paths=("$5")
package_copy_out "$5" "$6"
package_cleanup
'''
        done = subprocess.run(["/bin/bash", "-c", command, "retention-fixture", str(ROOT),
                               str(self.repo), sys.executable, str(worktree), str(scratch),
                               str(staged)], capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
        self.assertEqual((scratch / "diagnostic").read_text(), "useful evidence\n")
        self.assertEqual((staged / "diagnostic").read_text(), "useful evidence\n")
        self.assertEqual((worktree / "compiled").read_text(), "generated output\n")
        self.assertIn("keeps this run's scratch paths and worktree", done.stderr)

    def test_a_failed_worktree_does_not_block_the_next_build_attempt(self):
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@invalid",
                 "commit", "-q", "--allow-empty", "-m", "fixture")
        command = '''
. "$1/alab/package.sh"
repo_root="$2"
python="$3"
package_worktree_path "$4"
'''
        cleanup = '''
. "$1/alab/package.sh"
repo_root="$2"
python="$3"
wt="$4"
wt_made=1
scratch_paths=()
package_cleanup
'''
        cases = [(preserve, previous) for preserve in (False, True)
                 for previous in ("none", "present", "missing")]
        for preserve, previous in cases:
            with self.subTest(preserve=preserve, previous=previous):
                if preserve:
                    self.marker.write_text("Preserve the failed build too\n")
                existing = self.repo / f"package attempt {preserve} {previous}" / "worktree"
                if previous != "none":
                    self.git("worktree", "add", "--detach", str(existing), "HEAD")
                    (existing / "failure evidence").write_text("keep this\n")
                if previous == "missing":
                    shutil.rmtree(existing)
                paths = []
                for _ in range(2):
                    done = subprocess.run(
                        ["/bin/bash", "-c", command, "worktree-fixture", str(ROOT),
                         str(self.repo), sys.executable, str(existing)],
                        capture_output=True, text=True, timeout=60,
                    )
                    self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
                    path = Path(done.stdout.strip())
                    self.assertTrue(path.is_dir())
                    self.assertEqual(path.parent, existing.parent)
                    self.assertNotEqual(path, existing)
                    self.git("worktree", "add", "--detach", str(path), "HEAD")
                    self.assertTrue((path / ".git").is_file())
                    paths.append(path)
                    cleaned = subprocess.run(
                        ["/bin/bash", "-c", cleanup, "worktree-cleanup-fixture", str(ROOT),
                         str(self.repo), sys.executable, str(path)],
                        capture_output=True, text=True, timeout=60,
                    )
                    self.assertEqual(cleaned.returncode, 0, cleaned.stderr + cleaned.stdout)
                    # This synthetic cleanup has no creation-time source binding
                    # or enclosing guardian. It cannot authorize source removal.
                    self.assertTrue(path.exists())
                    if not preserve:
                        self.assertIn("no exact cleanup binding", cleaned.stderr)
                    self.assertEqual(existing.exists(), previous == "present")
                    if previous == "present":
                        self.assertEqual((existing / "failure evidence").read_text(), "keep this\n")
                self.assertNotEqual(*paths)
                registrations = self.git("worktree", "list", "--porcelain").splitlines()
                self.assertEqual(f"worktree {existing}" in registrations, previous != "none")

    def test_failure_before_registration_preserves_unbound_reservations(self):
        command = '''
. "$1/alab/package.sh"
repo_root="$2"
python="$3"
wt="$(package_worktree_path "$4")"
wt_made=""
scratch_paths=()
if [[ "$5" == evidence ]]; then
  printf 'keep this\n' >"${wt}/diagnostic"
fi
printf '%s\n' "$wt"
package_cleanup
'''
        for preserve in (False, True):
            if preserve:
                self.marker.write_text("Preserve early failures too\n")
            for contents in ("empty", "evidence"):
                with self.subTest(preserve=preserve, contents=contents):
                    base = self.repo / "private attempts" / "worktree"
                    done = subprocess.run(
                        ["/bin/bash", "-c", command, "early-failure-fixture", str(ROOT),
                         str(self.repo), sys.executable, str(base), contents],
                        capture_output=True, text=True, timeout=60,
                    )
                    self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
                    path = Path(done.stdout.strip())
                    self.assertEqual(path.parent, base.parent)
                    self.assertNotEqual(path, base)
                    self.assertTrue(path.exists(), "an unbound reservation cannot authorize deletion")
                    if contents == "evidence":
                        self.assertEqual((path / "diagnostic").read_text(), "keep this\n")


if __name__ == "__main__":
    unittest.main()
