#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""POSIX release CLI admits only object transfers, in their existing lifetime."""
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

import test_github as fixture


@unittest.skipUnless(os.name == "posix", "release shell transfer admission is POSIX")
class ReleaseFetchAdmissionTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.home = Path(scratch.name).resolve()
        self.source = self.home / "source"
        self.source.mkdir()
        fixture.git(self.source, "init", "-q", "-b", "main")
        fixture.git(self.source, "-c", "user.name=Fixture", "-c", "user.email=test@example.invalid",
                    "commit", "-q", "--allow-empty", "-m", "source")
        self.oid = fixture.git(self.source, "rev-parse", "HEAD")
        self.repo = self.home / "receiver.git"
        fixture.git(self.home, "init", "-q", "--bare", str(self.repo))
        self.target = "refs/remotes/origin/main"

    def cli(self, remote, destination=None):
        command = ["github.py", "--repo", str(self.repo), "fetch", str(remote),
                   "refs/heads/main:" + (destination or self.target)]
        stderr = io.StringIO()
        with mock.patch.object(sys, "argv", command), redirect_stderr(stderr):
            status = fixture.GITHUB.main()
        return status, stderr.getvalue()

    def test_native_monitors_preserve_a_pinned_volume_and_independent_observer(self):
        common = self.home / "output"
        objects = common / "objects"
        objects.mkdir(parents=True)
        old_pack, new_pack = self.home / "first pack", self.home / "second pack"
        old_pack.mkdir()
        new_pack.mkdir()
        pack = objects / "pack"
        pack.symlink_to(old_pack)
        old_has_room = True
        def space(path):
            return SimpleNamespace(f_bavail=19 if path == old_pack and not old_has_room else 21,
                                   f_frsize=2**30)

        # The running writer may still hold the original directory open after retargeting.
        extra = mock.Mock()
        def consume(_identity, _repo, _rows, **options):
            nonlocal old_has_room
            options["fetch_check"]()
            pack.rename(objects / "initial link")
            pack.symlink_to(new_pack)
            old_has_room = False
            with self.assertRaisesRegex(fixture.GITHUB.Refused, "first pack"):
                options["fetch_check"]()
        client = mock.Mock(fetch_batch=consume)
        with mock.patch.object(os, "statvfs", space):
            fixture.GITHUB.fetch_authenticated_batch(client, "example/repo", self.repo, [], common,
                                                      True, extra, admit_transfer=fixture.GITHUB.admit_release_transfer)
        self.assertEqual(extra.call_count, 2)

    def test_inherited_local_fetch_rechecks_space_before_publishing(self):
        fixture.git(self.repo, "fetch", "--quiet", str(self.source), "refs/heads/main:" + self.target)
        fixture.git(self.source, "-c", "user.name=Fixture", "-c", "user.email=test@example.invalid",
                    "commit", "-q", "--allow-empty", "-m", "advance")
        new_oid = fixture.git(self.source, "rev-parse", "HEAD")
        pack = self.repo / "objects" / "pack"
        writers = []
        groups = []
        samples = []
        real_popen = subprocess.Popen

        def observe(argv, *args, **options):
            process = real_popen(argv, *args, **options)
            if "--no-write-fetch-head" in argv:
                writers.append(process)
                groups.append(os.getpgid(process.pid))
            return process

        def space(path):
            free = 21
            if path == pack:
                if writers and writers[-1].returncode == 0:
                    free = 19
                samples.append(free)
            return SimpleNamespace(f_bavail=free, f_frsize=2**30)

        with mock.patch.object(subprocess, "Popen", side_effect=observe):
            with mock.patch.object(os, "statvfs", space):
                status, detail = self.cli(self.source)
            self.assertEqual(groups, [os.getpgrp()])
            self.assertEqual(writers[0].returncode, 0)
            self.assertEqual(status, 75, detail)
            self.assertIn("20 GiB running floor", detail)
            self.assertIn(str(pack), detail)
            self.assertEqual((samples[0], samples[-1]), (21, 19))
            self.assertEqual(fixture.git(self.repo, "rev-parse", self.target), self.oid)
            self.assertEqual(fixture.git(self.repo, "cat-file", "-t", new_oid), "commit")

            with mock.patch.object(os, "statvfs", side_effect=AssertionError("retry should reuse the import")):
                self.assertEqual(self.cli(self.source), (0, ""))
        self.assertEqual(len(writers), 1)
        self.assertEqual(fixture.git(self.repo, "rev-parse", self.target), new_oid)

    def test_local_writer_admission_then_current_and_cached_reads(self):
        pack = self.repo / "objects" / "pack"
        measured = []
        def space(path):
            measured.append(path)
            return SimpleNamespace(f_bavail=19 if path == pack else 20, f_frsize=2**30)
        before = fixture.GITHUB.local_refs(self.repo)
        with mock.patch.object(os, "statvfs", space):
            status, detail = self.cli(self.source)
        self.assertEqual(status, 75, detail)
        self.assertIn("20 GiB running floor", detail)
        self.assertIn(str(pack), detail)
        self.assertEqual(measured, [self.repo, self.repo / "objects", pack])
        self.assertEqual(fixture.GITHUB.local_refs(self.repo), before)
        object_state = fixture.GITHUB.git(self.repo, "cat-file", "--batch-check", data=(self.oid + "\n").encode())
        self.assertEqual(object_state.decode().strip(), self.oid + " missing")
        with mock.patch.object(os, "statvfs", return_value=SimpleNamespace(f_bavail=20, f_frsize=2**30)):
            self.assertEqual(self.cli(self.source), (0, ""))
        alias = "refs/remotes/another/main"
        with mock.patch.object(os, "statvfs", side_effect=AssertionError("no object writer expected")):
            self.assertEqual(self.cli(self.source), (0, ""))
            self.assertEqual(self.cli(self.source, alias), (0, ""))
        self.assertEqual(fixture.git(self.repo, "rev-parse", alias), self.oid)

    def test_native_batch_binds_measured_scratch_and_keeps_existing_transport(self):
        native = fixture.NativeFixture(self.source)
        scratch = self.home / "native temp"
        scratch.mkdir()
        measured = []
        denied = True
        def space(path):
            measured.append(path)
            return SimpleNamespace(f_bavail=19 if denied and path == scratch else 20, f_frsize=2**30)
        calls = []
        def call(*args, **options):
            manifest = Path(args[args.index("--input") + 1])
            calls.append((manifest, options))
            return native.fetch_batch("fixture/codex", self.repo, json.loads(manifest.read_text()))
        client = object.__new__(fixture.GITHUB.Agh)
        client.api = native.api
        client.call = call
        with mock.patch.object(fixture.GITHUB, "Agh", return_value=client), mock.patch.object(
                tempfile, "tempdir", str(scratch)), mock.patch.object(os, "statvfs", space):
            before = fixture.GITHUB.local_refs(self.repo)
            status, detail = self.cli("fixture/codex")
            self.assertEqual(status, 75, detail)
            self.assertIn(str(scratch), detail)
            self.assertEqual(fixture.GITHUB.local_refs(self.repo), before)
            self.assertEqual(calls, [])
            self.assertEqual(list(scratch.iterdir()), [])
            denied = False
            self.assertEqual(self.cli("fixture/codex"), (0, ""))
            self.assertEqual(len(calls), 1)
            manifest, options = calls[0]
            self.assertEqual(manifest.parent.parent, scratch)
            self.assertEqual(options["temp_root"], str(manifest.parent))
            self.assertTrue(callable(options["fetch_check"]))
            self.assertEqual(list(scratch.iterdir()), [])
            self.assertEqual(measured.count(scratch), 2)
            denied = True
            with self.assertRaisesRegex(fixture.GITHUB.Refused, "20 GiB running floor"):
                options["fetch_check"]()
            measured.clear()
            denied = True
            self.assertEqual(self.cli("fixture/codex"), (0, ""))
            self.assertEqual(measured, [])
            self.assertEqual(len(calls), 1)


if __name__ == "__main__":
    unittest.main()
