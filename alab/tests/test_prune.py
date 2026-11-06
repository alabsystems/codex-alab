#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Prune real scratch directories while recoverable session records hold custody."""

import fcntl
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import tempfile
import unittest


PRUNE = Path(__file__).resolve().parents[1] / "prune.py"


class PruneCustodyTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix="alab-prune-custody-")
        self.addCleanup(scratch.cleanup)
        self.home = Path(scratch.name).resolve()
        subprocess.run(["git", "init", "-q", str(self.home)], check=True)
        self.root = self.home / "artifacts with spaces/codex"
        self.prefix = self.home / "pkg"
        self.codex_home = self.home / "codex home"
        self.sessions = self.codex_home / "alab/sessions"
        self.sessions.mkdir(parents=True)
        (self.prefix / "links").mkdir(parents=True)
        (self.prefix / "store.lock").touch(mode=0o600)
        self.old = self.package("old pinned", "2026-10-20T00:00:00Z")
        self.unused = self.package("old unused", "2026-10-20T01:00:00Z")
        self.new = self.package("new selected", "2026-10-21T00:00:00Z")
        (self.prefix / "links/codex").write_text(
            'schema = 1\nprogram = "codex"\n'
            f"checkout = {json.dumps(str(self.new))}\nbins = [\"bin/codex\"]\n"
        )

    def package(self, name, built_at):
        package = self.root / name
        (package / "bin").mkdir(parents=True)
        # No Mach-O is copied or executed; only the record and package paths matter.
        (package / "bin/codex").write_bytes(b"")
        (package / "alab-build.json").write_text(json.dumps({"built_at": built_at}))
        return package

    def record(self, field):
        record = {
            "v": 1, "pid": 123, "procStart": None, "identity": None,
            "sessionId": "019a0000-0000-7000-8000-000000000001",
            "displayedThreadId": None, "cwd": str(self.home), "version": "test",
            "exe": None, "launchArgv0": None, "execGeneration": 0,
            "resumeArgv": None, "env": {}, "status": "busy", "exit": None,
            "state": "working", "blockedKind": None, "draftEmpty": True,
            "lastHumanInputAt": None, "autopilot": "active", "claim": None,
            "statusUpdatedAt": 1000, "updatedAt": 1000,
            "kind": "interactive", "entrypoint": "cli", "carriedOn": 0,
        }
        path = str(self.old / "bin/codex")
        record[field] = [path, "resume", record["sessionId"]] if field == "resumeArgv" else path
        return json.dumps(record).encode()

    def prune(self, *, rename_after_listing=False, extra=()):
        command = [sys.executable, "-I", str(PRUNE)]
        if rename_after_listing:
            # Interpose the writer's actual custody rename at the census/read boundary,
            # rather than relying on scheduling or a wall-clock race.
            command = [sys.executable, "-I", "-c", """
import os, runpy, sys
script, sessions = sys.argv[1:3]
sys.argv = [script, *sys.argv[3:]]
listed = os.listdir
def listdir(path):
    names = listed(path)
    if os.fspath(path) == sessions:
        os.rename(os.path.join(sessions, "123.json"),
                  os.path.join(sessions, "123.json.interrupted.tmp"))
    return names
os.listdir = listdir
runpy.run_path(script, run_name="__main__")
""", str(PRUNE), str(self.sessions)]
        done = subprocess.run(
            [*command, "--root", str(self.root),
             "--keep", "1", "--prefix", str(self.prefix),
             "--codex-home", str(self.codex_home), "--repo", str(self.home), *extra],
            env={"PATH": "/usr/bin:/bin", "HOME": str(self.home)},
            capture_output=True, text=True, timeout=60,
        )
        self.assertEqual(done.returncode, 0, done.stderr)
        return done.stderr

    def restore_packages(self):
        if not self.old.exists():
            self.old = self.package("old pinned", "2026-10-20T00:00:00Z")
        if not self.unused.exists():
            self.unused = self.package("old unused", "2026-10-20T01:00:00Z")

    def test_a_sole_canonical_atomic_or_quarantined_record_keeps_its_package(self):
        for name in ["123.json", "123.json.1a2b.tmp", "123.json.interrupted.tmp",
                     "123.json..tmp", "123.json.nonce\npart.tmp"]:
            for field in ["resumeArgv", "exe", "launchArgv0"]:
                with self.subTest(name=name, field=field):
                    self.restore_packages()
                    path = self.sessions / name
                    path.write_bytes(self.record(field))
                    try:
                        said = self.prune()
                        self.assertTrue(self.old.exists(), said)
                        self.assertFalse(self.unused.exists(), said)
                        self.assertTrue(self.new.exists(), said)
                    finally:
                        path.unlink()

    def test_an_existing_empty_canonical_or_temporary_record_fails_closed(self):
        for name in ["123.json", "123.json.interrupted.tmp"]:
            with self.subTest(name=name):
                self.restore_packages()
                path = self.sessions / name
                path.write_bytes(b"")
                try:
                    said = self.prune()
                    self.assertTrue(self.old.exists() and self.unused.exists(), said)
                finally:
                    path.unlink()

    def test_malformed_and_unreadable_records_fail_closed(self):
        for name in ["123.json", "123.json.interrupted.tmp"]:
            for kind in ["malformed", "nonobject", "invalid-utf8", "oversized",
                         "unreadable", "symlink", "fifo", "directory"]:
                with self.subTest(name=name, kind=kind):
                    self.restore_packages()
                    path = self.sessions / name
                    if kind == "directory":
                        path.mkdir()
                    elif kind == "fifo":
                        os.mkfifo(path)
                    elif kind == "symlink":
                        path.symlink_to(self.home / "missing record")
                    else:
                        payloads = {"malformed": b"{broken", "nonobject": b"[]",
                                    "invalid-utf8": b"\xff", "oversized": b" " * (1024 * 1024 + 1),
                                    "unreadable": self.record("resumeArgv")}
                        path.write_bytes(payloads[kind])
                        if kind == "unreadable":
                            path.chmod(0)
                    try:
                        said = self.prune()
                        self.assertTrue(self.old.exists() and self.unused.exists(), said)
                    finally:
                        if kind == "directory":
                            path.rmdir()
                        else:
                            path.unlink()

    def test_nonrecord_junk_does_not_freeze_collection(self):
        for name in ["notes", "123.json.tmp", "123.json.pending", "123.json.x.tmp.extra",
                     "4294967296.json", "4294967296.json.x.tmp", "١٢٣.json"]:
            (self.sessions / name).write_bytes(b"{not a record")
        said = self.prune()
        self.assertFalse(self.old.exists(), said)
        self.assertFalse(self.unused.exists(), said)
        self.assertTrue(self.new.exists(), said)

    def test_a_record_renamed_after_listing_defers_collection_until_the_next_census(self):
        (self.sessions / "123.json").write_bytes(self.record("resumeArgv"))
        said = self.prune(rename_after_listing=True)
        self.assertTrue((self.sessions / "123.json.interrupted.tmp").exists())
        self.assertTrue(self.old.exists() and self.unused.exists(), said)
        # A normal later census recovers the sole renamed witness and still frees junk.
        said = self.prune()
        self.assertTrue(self.old.exists(), said)
        self.assertFalse(self.unused.exists(), said)

    def test_an_inaccessible_sessions_ancestor_fails_closed(self):
        ancestor = self.sessions.parent
        ancestor.chmod(0)
        try:
            with self.assertRaises(PermissionError):
                self.sessions.stat()
            said = self.prune()
            self.assertTrue(self.old.exists() and self.unused.exists(), said)
        finally:
            ancestor.chmod(0o700)

    def test_a_truly_absent_sessions_directory_does_not_freeze_collection(self):
        self.sessions.rmdir()
        self.sessions.parent.rmdir()
        self.codex_home.rmdir()
        said = self.prune()
        self.assertFalse(self.old.exists(), said)
        self.assertFalse(self.unused.exists(), said)
        self.assertTrue(self.new.exists(), said)

    def test_a_missing_special_or_contended_store_lock_defers_all_deletion(self):
        lock = self.prefix / "store.lock"
        lock.unlink()
        for kind in ["missing", "directory", "symlink", "fifo", "held"]:
            with self.subTest(kind=kind):
                if kind == "directory":
                    lock.mkdir()
                elif kind == "symlink":
                    lock.symlink_to(self.home / "not the store lock")
                elif kind == "fifo":
                    os.mkfifo(lock)
                elif kind == "held":
                    lock.touch(mode=0o600)
                try:
                    if kind == "held":
                        with lock.open("rb") as stream:
                            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
                            said = self.prune()
                    else:
                        said = self.prune()
                    self.assertIn("pruning stopped", said)
                    self.assertTrue(self.old.exists() and self.unused.exists(), said)
                finally:
                    if kind == "directory":
                        lock.rmdir()
                    elif kind != "missing":
                        lock.unlink()

    def test_a_manual_selection_holds_the_same_lock_and_is_seen_by_the_next_census(self):
        marker = self.prefix / "links/codex"
        with (self.prefix / "store.lock").open("rb") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            marker.write_text(f"checkout = {json.dumps(str(self.old))}\n")
            said = self.prune()
            self.assertTrue(self.old.exists() and self.unused.exists(), said)
        said = self.prune()
        self.assertTrue(self.old.exists(), said)
        self.assertFalse(self.unused.exists(), said)

    def test_a_selector_cannot_change_the_link_between_its_snapshot_and_deletion(self):
        # Pause after the real link snapshot, exactly where a recheck-only fix races.
        # An independent writer using atpkg's BSD flock protocol must be excluded.
        child = subprocess.Popen([
            sys.executable, "-I", "-c", """
import runpy, sys
script = sys.argv[1]
ns = runpy.run_path(script)
g = ns["main"].__globals__
linked = g["linked"]
def snapshot(prefix):
    paths = linked(prefix)
    print("snapshot", flush=True)
    assert sys.stdin.readline() == "continue\\n"
    return paths
g["linked"] = snapshot
sys.argv = [script, *sys.argv[2:]]
sys.exit(ns["main"]())
""", str(PRUNE), "--root", str(self.root), "--keep", "1", "--prefix", str(self.prefix),
            "--codex-home", str(self.codex_home), "--repo", str(self.home)],
            env={"PATH": "/usr/bin:/bin", "HOME": str(self.home)},
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        try:
            ready, _, _ = select.select([child.stdout], [], [], 60)
            self.assertTrue(ready, "pruner snapshot hang detector")
            self.assertEqual(child.stdout.readline(), "snapshot\n")
            before = (self.prefix / "links/codex").read_bytes()
            writer = subprocess.run([sys.executable, "-I", "-c", """
import fcntl, sys
with open(sys.argv[1], "rb") as stream:
    try:
        fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        sys.exit(75)
    raise AssertionError("selector acquired the store lock inside the prune census")
""", str(self.prefix / "store.lock")], capture_output=True, text=True, timeout=60)
            self.assertEqual(writer.returncode, 75, writer.stderr)
            self.assertEqual((self.prefix / "links/codex").read_bytes(), before)
            _, stderr = child.communicate("continue\n", timeout=60)
            self.assertEqual(child.returncode, 0, stderr)
            self.assertFalse(self.old.exists() or self.unused.exists(), stderr)
            self.assertTrue(self.new.exists(), stderr)
            # The guard is released at completion; it was not a permanent claim.
            with (self.prefix / "store.lock").open("rb") as stream:
                fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        finally:
            if child.poll() is None:
                child.kill()
                child.communicate(timeout=60)

    def test_clone_policy_preserves_all_packages_and_dry_run_never_deletes(self):
        said = self.prune(extra=("--dry-run",))
        self.assertIn("would prune", said)
        self.assertTrue(self.old.exists() and self.unused.exists(), said)
        (self.home / ".git/alab-preserve-builds").write_text("keep the user's builds\n")
        said = self.prune()
        self.assertIn("cleanup policy preserves all packages", said)
        self.assertTrue(self.old.exists() and self.unused.exists(), said)


if __name__ == "__main__":
    unittest.main()
