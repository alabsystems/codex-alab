#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Opt-in real native controller cancellation with inert cache-locked writers.

ALAB_TEST_PROJECT_CONTROLLER names a frozen qualified atpkg-publish executable.
No compiler runs. Retain every fixture, native transcript and incomplete attempt.
"""

import fcntl
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[2]


def available(lock):
    with lock.open("r+b") as stream:
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return True
        except BlockingIOError:
            return False


def until(predicate, description):
    deadline = time.monotonic() + 60
    while not predicate():
        if time.monotonic() >= deadline:
            raise RuntimeError(description)
        time.sleep(.02)


@unittest.skipUnless("ALAB_TEST_PROJECT_CONTROLLER" in os.environ,
                     "set ALAB_TEST_PROJECT_CONTROLLER to a qualified native producer")
class NativePackageControllerTests(unittest.TestCase):
    def exercise(self, signum, *, exec_root):
        parent = Path(tempfile.gettempdir()) / ".ubd"
        parent.mkdir(exist_ok=True)
        root = Path(tempfile.mkdtemp(prefix="native-package-controller-", suffix=".noindex", dir=parent)).resolve()
        print(f"retained native controller fixture: {root}", flush=True)
        cache = root / "cache"
        cache.mkdir(mode=0o700)
        ready, release = root / "ready", root / "release"
        writer = root / "writer.py"
        writer.write_text(
            "from pathlib import Path\nimport sys,time\nroot=Path(sys.argv[1])\n"
            "(root/'ready').touch()\n"
            "while not (root/'release').exists(): time.sleep(.02)\n"
            "(root/'released-writer').touch()\n")
        lock = cache / ".alab-package.lockf"
        helper = "alab_package_cache_exec" if exec_root else "alab_package_cache_locked"
        body = '. "$1/alab/lib.sh"; shift; ' + helper + ' "$@"'
        command = [os.environ["ALAB_TEST_PROJECT_CONTROLLER"], "toolchain", "run", "--repo", str(ROOT),
                   "--", "/bin/bash", "-c", body, "native-controller-fixture", str(ROOT), str(cache),
                   sys.executable, "-I", "-B", str(writer), str(root)]
        environment = dict(os.environ, ALAB_PYTHON=sys.executable)
        stderr = root / "stderr"
        with (root / "stdout").open("wb") as out, stderr.open("wb") as err:
            child = subprocess.Popen(command, env=environment, cwd=ROOT, start_new_session=True,
                                     stdout=out, stderr=err)
            try:
                def started():
                    if child.poll() is not None:
                        raise RuntimeError(f"native controller refused before writer startup: {stderr.read_text()}")
                    return ready.exists()
                until(started, "native writer startup did not complete")
                child.send_signal(signum)  # The owning unreaped native front, never a numeric descendant.
                if exec_root:
                    code = child.wait(timeout=60)
                    self.assertTrue(available(lock), "native front exit preceded writer retirement")
                    self.assertFalse((root / "released-writer").exists())
                else:
                    # Negative control: a returning shell root cannot cancel its guardian.
                    until(lambda: "root ended; retaining resources" in stderr.read_text(),
                          "native controller did not observe the negative-control root exit")
                    self.assertIsNone(child.poll(), "native custody must retain the live guardian")
                    self.assertFalse(available(lock))
                    release.touch()
                    code = child.wait(timeout=60)
                    self.assertTrue(available(lock))
                self.assertEqual(code, 128 + signum, stderr.read_text())
            finally:
                # Release only our inert writer if a negative control/assertion failed.
                # The native controller and guardian retain their own process authority.
                release.touch()
                child.wait(timeout=60)
                if lock.exists():
                    until(lambda: available(lock), "native fixture custody did not retire")

    def test_native_root_signals_reach_guardian_and_retire_its_writers(self):
        for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            with self.subTest(signal=signum):
                self.exercise(signum, exec_root=True)

    def test_returning_shell_control_retains_custody_until_explicit_writer_release(self):
        self.exercise(signal.SIGTERM, exec_root=False)


if __name__ == "__main__":
    unittest.main()
