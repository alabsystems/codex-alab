# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Real local Git finishes negotiation before a peer updates the receiver."""
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import test_github as fixture


# Git invokes the hook after the requesting fetch has advertised its local old
# values and before Git packs its reply. The peer runs synchronously at that point.
PACK_HOOK = """
import os, runpy, sys
from pathlib import Path
module, source, receiver, name, old, *pack = sys.argv[1:]
g = runpy.run_path(module)
root = Path(receiver).parent
(root / 'packed').touch()
refs = ['refs/heads/main', 'refs/heads/mirror']
specs = [ref + ':refs/remotes/test/' + ref.rsplit('/', 1)[1] for ref in refs]
if name == 'divergent':
    g['git'](source, 'update-ref', refs[0], old)
g['fetch'](receiver, source, specs[:1] if name in ('partial', 'second-identical', 'divergent') else specs)
if name == 'hidden':
    g['git'](source, 'config', 'uploadpack.hideRefs', refs[0])
if name == 'bad-pack':
    sys.exit(1)
os.execvp(pack[0], pack)
"""


@unittest.skipUnless(os.name == "posix", "requires local Git upload-pack")
class LocalBatchConflictTests(unittest.TestCase):
    def exercise(self, mode):
        g = fixture.GITHUB
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            source, receiver = root / "source", root / "receiver.git"
            fixture.git(root, "init", "--template=", "-q", "-b", "main", str(source))
            fixture.git(root, "init", "--template=", "--bare", "-q", str(receiver))
            fixture.git(source, "config", "user.name", "Fixture")
            fixture.git(source, "config", "user.email", "fixture@example.invalid")
            def commit(label):
                fixture.git(source, "commit", "--allow-empty", "-q", "-m", label)
                return fixture.git(source, "rev-parse", "HEAD")

            before = commit("previous")
            tree = fixture.git(source, "rev-parse", "HEAD^{tree}")
            displaced = g.git(source, "commit-tree", tree, "-p", before, data=b"other\n").decode().strip()
            fixture.git(source, "update-ref", "refs/heads/mirror", before)
            refs = ["refs/heads/main", "refs/heads/mirror"]
            targets = ["refs/remotes/test/main", "refs/remotes/test/mirror"]
            specs = [ref + ":" + target for ref, target in zip(refs, targets)]
            g.fetch(receiver, str(source), specs)
            wanted = commit("desired")
            fixture.git(source, "update-ref", refs[1], wanted)
            snap = dict.fromkeys(refs, wanted)
            real = g.run_transport
            batches = []
            checks = []
            hook = shlex.join([sys.executable, "-IB", "-c", PACK_HOOK, g.__file__, str(source),
                               str(receiver), mode, displaced])
            upload = shlex.join([shutil.which("git"), "-c", "uploadpack.packObjectsHook=" + hook, "upload-pack"])

            def transport(argv, **options):
                if "fetch" in argv:
                    batches.append([row[1:].split(":", 1)[0] for row in options["input"].decode().splitlines()])
                    if mode == "second-identical" and len(batches) == 2:
                        # A new peer completes the outstanding slot while this last
                        # writer is committing. Return the files backend's same
                        # exact-old-value failure witnessed in the first real race.
                        with mock.patch.object(g, "run_transport", real):
                            g.fetch(receiver, str(source), specs[1:])
                        slot = options["input"].decode().strip().split(":", 1)[1]
                        err = f"error: cannot lock ref '{slot}': is at {wanted} but expected {before}\n"
                        return subprocess.CompletedProcess(argv, 251, b"", err.encode())
                    if len(batches) == 1:
                        i = argv.index("fetch") + 1
                        argv = [*argv[:i], "--upload-pack=" + upload, *argv[i:]]
                return real(argv, **options)

            def admit(_common, _scratch):
                def check():
                    checks.append(len(batches))
                    if mode == "pressure" and (root / "packed").exists():
                        raise g.Refused("fixture disk floor")
                return check

            with mock.patch.object(g, "run_transport", transport):
                if mode in ("identical", "partial", "second-identical"):
                    self.assertEqual(g.fetch(receiver, str(source), specs, snapshot=snap, admit_transfer=admit), snap)
                else:
                    why = "fixture disk floor" if mode == "pressure" else "local Git fetch failed"
                    with self.assertRaisesRegex(g.Refused, why):
                        g.fetch(receiver, str(source), specs, snapshot=snap, admit_transfer=admit)
            self.assertTrue((root / "packed").is_file())
            self.assertEqual(batches, [refs, refs[1:]] if mode in ("partial", "second-identical") else [refs])
            if mode == "identical":
                self.assertEqual(checks, [1, 1], "sample the losing writer and the final reuse")
            if mode == "second-identical":
                self.assertEqual(checks, [1, 2, 2])
            if mode in ("identical", "partial", "second-identical"):
                self.assertEqual([g.current_ref(receiver, ref) for ref in targets], [wanted, wanted])
            elif mode == "divergent":
                self.assertEqual([g.current_ref(receiver, ref) for ref in targets], [displaced, before])

    def test_identical_peer_finishes_supplied_snapshot_without_another_writer(self):
        self.exercise("identical")

    def test_partial_peer_retries_only_the_unfinished_name(self):
        self.exercise("partial")

    def test_second_identical_conflict_reconciles_without_a_third_writer(self):
        self.exercise("second-identical")

    def test_divergent_peer_disappeared_source_space_and_transport_failure_are_refused(self):
        for mode in ("divergent", "hidden", "pressure", "bad-pack"):
            with self.subTest(mode=mode):
                self.exercise(mode)

    def test_only_one_bounded_local_cas_diagnosis_is_recognized(self):
        g = fixture.GITHUB
        slot = b"refs/agh/codex-release-" + b"a" * 24
        line = b"error: cannot lock ref '" + slot + b"': is at " + b"a" * 40 + b" but expected " + b"b" * 40
        for status, stderr, valid in ((251, line, True), (253, line, False), (251, b"remote: " + line, False),
                                      (251, line + b"\nerror: another failure", False),
                                      (251, b"hint:" + b"a" * 4096 + b"\n" + line, False)):
            with self.subTest(status=status, valid=valid):
                done = subprocess.CompletedProcess([], status, b"", stderr)
                self.assertEqual(g.local_fetch_conflict(done) is not None, valid)
