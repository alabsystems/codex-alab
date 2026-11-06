#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Real guardian death, writer retirement and inherited cache-lock custody."""

import errno
import fcntl
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[2]
GUARD = ROOT / "alab/command_guard.py"


def await_condition(test, description):
    deadline = time.monotonic() + 60
    while not test():
        if time.monotonic() >= deadline:
            raise AssertionError(description + " exceeded the one-minute hang detector")
        time.sleep(0.01)


def group_exists(pgid):
    try:
        os.killpg(pgid, 0)
    except ProcessLookupError:
        return False
    return True


class GuardianLifelineTests(unittest.TestCase):
    def test_sigkill_guardian_keeps_flock_until_child_and_grandchild_are_gone(self):
        self.assert_guardian_signal(signal.SIGKILL, -signal.SIGKILL)

    def test_sigterm_guardian_keeps_flock_until_child_and_grandchild_are_gone(self):
        self.assert_guardian_signal(signal.SIGTERM, 128 + signal.SIGTERM)

    def assert_guardian_signal(self, signum, returncode):
        directory = Path(tempfile.mkdtemp(prefix="alab-lifeline-kill-", suffix=".noindex"))
        print(f"preserved lifeline fixture: {directory}", flush=True)
        ready, descendant, stop = (directory / name for name in ("ready", "descendant", "stop"))
        lock = directory / "cache.lock"
        lock.write_bytes(b"existing lock identity and contents\n")
        lock_identity = (lock.stat().st_dev, lock.stat().st_ino, lock.stat().st_mode)
        writer = f'''
import json, os, signal, time
from pathlib import Path
signal.alarm(60)
Path({str(descendant)!r}).write_text(json.dumps([os.getpid(), os.getpgrp()]))
while not Path({str(stop)!r}).exists(): time.sleep(0.01)
'''
        command = f'''
import json, os, signal, subprocess, sys, time
from pathlib import Path
signal.alarm(60)
child = subprocess.Popen([sys.executable, "-I", "-B", "-c", {writer!r}],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
deadline = time.monotonic() + 60
while not Path({str(descendant)!r}).exists():
    if time.monotonic() >= deadline: raise RuntimeError("grandchild startup hang")
    time.sleep(0.01)
Path({str(ready)!r}).write_text(json.dumps([os.getpid(), os.getpgrp()]))
while not Path({str(stop)!r}).exists(): time.sleep(0.01)
child.wait(timeout=60)
'''
        identity = None
        with (directory / "command.log").open("w") as log:
            guardian = subprocess.Popen(
                [sys.executable, "-I", "-B", str(GUARD), "--cache-lock", str(lock),
                 "--min-free-gib", "20", "--disk-path", str(directory), "--",
                 sys.executable, "-I", "-B", "-c", command],
                start_new_session=True, stdin=subprocess.DEVNULL,
                stdout=log, stderr=subprocess.STDOUT,
            )
            try:
                await_condition(ready.is_file, "actual command readiness")
                identity = json.loads(ready.read_text())
                self.assertEqual(json.loads(descendant.read_text())[1], identity[1])
                with lock.open("rb") as competing:
                    with self.assertRaises(BlockingIOError):
                        fcntl.flock(competing, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    guardian.send_signal(signum)
                    self.assertEqual(guardian.wait(timeout=60), returncode)

                    def acquire():
                        try:
                            fcntl.flock(competing, fcntl.LOCK_EX | fcntl.LOCK_NB)
                        except BlockingIOError:
                            return False
                        return True

                    await_condition(acquire, "custodian cache-lock retirement")
                    self.assertFalse(group_exists(identity[1]),
                                     "cache reopened while the original writers still exist")
                self.assertEqual((lock.stat().st_dev, lock.stat().st_ino, lock.stat().st_mode),
                                 lock_identity)
                self.assertEqual(lock.read_bytes(), b"existing lock identity and contents\n")
            finally:
                # The baseline has no custodian: ask only these harmless owned writers to
                # finish through their private marker; never signal an unleased PGID.
                stop.write_text("finish private fixture")
                if guardian.poll() is None:
                    guardian.terminate()
                    guardian.wait(timeout=60)
                if identity is not None:
                    await_condition(lambda: not group_exists(identity[1]), "fixture cleanup")

    def test_cache_command_receives_no_lock_or_lifeline_descriptors(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-lifeline-fds-", suffix=".noindex"))
        observed = directory / "fds.json"
        command = f'''
import json, os
from pathlib import Path
descriptors = []
for name in os.listdir('/dev/fd' if os.path.isdir('/dev/fd') else '/proc/self/fd'):
    if name.isdigit():
        try: os.fstat(int(name))
        except OSError: continue
        descriptors.append(int(name))
Path({str(observed)!r}).write_text(json.dumps(sorted(descriptors)))
'''
        with (directory / "command.log").open("w") as log:
            result = subprocess.run(
                [sys.executable, "-I", "-B", str(GUARD), "--cache-lock",
                 str(directory / "cache.lock"), "--", sys.executable, "-I", "-B", "-c", command],
                stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT, timeout=60,
            )
        self.assertEqual(result.returncode, 0)
        self.assertEqual(json.loads(observed.read_text()), [0, 1, 2])

    def test_large_exec_failure_is_drained_before_waiting_for_retirement(self):
        # A legitimate OS error can include a filename larger than a pipe's
        # ordinary capacity. The one-minute timeout detects protocol deadlock.
        result = subprocess.run(
            [sys.executable, "-I", "-B", str(GUARD), "--", "/" + "x" * 12000],
            stdin=subprocess.DEVNULL, capture_output=True, timeout=60,
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn(os.strerror(errno.ENAMETOOLONG).encode(), result.stderr)


if __name__ == "__main__":
    unittest.main()
