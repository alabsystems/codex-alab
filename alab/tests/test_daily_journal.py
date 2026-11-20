#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the byte and line limits through the scheduled run's ordinary entry."""

import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
MIB = 1024 * 1024
LIMIT = 4 * MIB
WRITER = r"""import json, os, signal, sys
from pathlib import Path
size = int(os.environ["OUTPUT_BYTES"])
stream = sys.stdout.buffer
if size:
    stream.write(b"first output\n")
    for _ in range(size // 65536):
        stream.write(b"x" * 65536)
else:
    for n in range(int(os.environ["OUTPUT_LINES"])):
        stream.write(f'{os.environ["LABEL"]} {n}\n'.encode())
stream.write(b"\nfinal diagnostic\n")
stream.flush()
sizes = [p.stat().st_size for p in Path(os.environ["TMPDIR"]).glob("alab-daily.*") if p.is_file()]
Path(os.environ["PROBE"]).write_text(json.dumps(sizes))
if os.environ["EXIT"] == "TERM":
    os.kill(os.getpid(), signal.SIGTERM)
sys.exit(int(os.environ["EXIT"]))
"""


class DailyJournalTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-journal-")
        self.addCleanup(temporary.cleanup)
        self.repo = Path(temporary.name).resolve()
        self.env = {k: v for k, v in os.environ.items() if not k.startswith(("ALAB_", "GIT_"))}
        self.env.update(HOME=str(self.repo), GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
                        LC_ALL="C", ALAB_PYTHON=sys.executable, TMPDIR=str(self.repo / "temp"))
        self.git("init", "-q")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@invalid",
                 "commit", "-qm", "base", "--allow-empty")
        for ref in ("origin/main", "upstream/main"):
            self.git("update-ref", "refs/remotes/" + ref, "HEAD")
        Path(self.env["TMPDIR"]).mkdir()
        scripts = self.repo / "alab"
        scripts.mkdir()
        for name in ("daily.sh", "lib.sh", "daily_journal.py", "sync_worktree.py", "retention.py"):
            shutil.copyfile(ROOT / "alab" / name, scripts / name)
        (scripts / "writer.py").write_text(WRITER)
        self.sync = scripts / "sync.sh"
        self.sync.write_text('#!/bin/sh\nexec "$ALAB_PYTHON" -IB "$PWD/alab/writer.py"\n')
        self.sync.chmod(0o700)
        self.journal = self.repo / ".git/alab-daily.log"
        self.status = self.repo / ".git/alab-daily.status"

    def git(self, *args):
        subprocess.run(["git", "-C", str(self.repo), *args], env=self.env, check=True,
                       capture_output=True, timeout=30)

    def scheduled(self, label, code, *, size=0, lines=0):
        probe = self.repo / f"{label}.json"
        env = dict(self.env, LABEL=label, EXIT=str(code), OUTPUT_BYTES=str(size),
                   OUTPUT_LINES=str(lines), PROBE=str(probe))
        command = ["/bin/bash", str(self.repo / "alab/daily.sh")]
        result = subprocess.run(command, env=env, capture_output=True, timeout=60)
        expected = 128 + signal.SIGTERM if code == "TERM" else code
        self.assertEqual(result.returncode, expected, result.stderr)
        self.assertIn(f" exit={expected} ", self.status.read_text())
        return json.loads(probe.read_bytes())

    def test_large_unbroken_output_bounds_live_staging_and_journal_on_retry(self):
        for label in ("first", "retry"):
            self.assertLessEqual(sum(self.scheduled(label, 75, size=3 * LIMIT)), LIMIT)
            record = self.journal.read_bytes()
            self.assertLessEqual(len(record), LIMIT)
            self.assertIn(b"alab/daily.sh: exit 75\n[earlier output from this run omitted]", record)
            self.assertTrue(record.endswith(b"final diagnostic\n"))

    def test_line_limit_keeps_latest_diagnostics_and_child_signal(self):
        self.scheduled("old", 0, lines=12_000)
        self.scheduled("latest", "TERM", lines=12_000)
        record = self.journal.read_bytes()
        self.assertLessEqual(record.count(b"\n"), 20_000)
        self.assertNotIn(b"old 0\n", record)
        self.assertIn(b"old 11999\n", record)
        self.assertIn(b"alab/daily.sh: exit 143\nlatest 0\n", record)
        self.assertIn(b"latest 11999\n\nfinal diagnostic\n", record)

    def test_failed_journal_publication_preserves_resource_status_and_destination(self):
        target = self.repo / "sentinel"
        target.write_text("private\n")
        self.journal.symlink_to(target)
        self.scheduled("stopped", 75)
        self.assertTrue(self.journal.is_symlink())
        self.assertEqual(target.read_text(), "private\n")


if __name__ == "__main__":
    unittest.main()
