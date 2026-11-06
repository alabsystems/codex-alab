#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Real Git stand-ins exercise scoped deadlines and the next ordinary attempt."""

import fcntl
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


GUARD = module("network_guard", ROOT / "alab/command_guard.py")


class NetworkFetchTests(unittest.TestCase):
    def setUp(self):
        self.home = Path(tempfile.mkdtemp(prefix="alab-network-fetch-")).resolve()
        print(f"preserved network-fetch fixture: {self.home}", flush=True)
        self.python = sys.executable
        self.real_git = shutil.which("git")
        self.ready = self.home / "writer-ready.json"
        self.expired = self.home / "deadline-observation.json"
        self.release = self.home / "allow-next-attempt"
        self.environment = dict(os.environ, HOME=str(self.home), GIT_CONFIG_GLOBAL="/dev/null",
                                GIT_CONFIG_NOSYSTEM="1", GIT_AUTHOR_NAME="Fixture",
                                GIT_COMMITTER_NAME="Fixture", GIT_AUTHOR_EMAIL="fixture@example.invalid",
                                GIT_COMMITTER_EMAIL="fixture@example.invalid")
        self.repo = self.home / "repo"
        self.repo.mkdir()
        self.git("init", "-q", "-b", "main")
        (self.repo / "alab").mkdir()
        for name in ("package.sh", "lib.sh", "command_guard.py", "github.py"):
            shutil.copyfile(ROOT / "alab" / name, self.repo / "alab" / name)
        (self.repo / "VERSION").write_text("0.164.0\n")
        self.git("add", ".")
        self.git("commit", "-q", "-m", "owned network fixture")
        self.git("tag", "rust-v0.164.0")
        self.git("remote", "add", "upstream", str(self.repo))
        self.git("config", "--unset-all", "remote.upstream.fetch")
        for spec in ("refs/heads/main:refs/remotes/upstream/main",
                     "+refs/tags/rust-v*:refs/upstream-tags/rust-v*"):
            self.git("config", "--add", "remote.upstream.fetch", spec)
        tools = self.home / "tools"
        tools.mkdir()
        writer = ("import json, os, signal; from pathlib import Path; "
                  f"Path({str(self.ready)!r}).write_text(json.dumps([os.getpid(), os.getpgrp()])); "
                  "print('owned writer inherited stdout', flush=True); signal.alarm(60); signal.pause()")
        fake = tools / "git"
        fake.write_text(f'''#!{self.python}
import os, signal, subprocess, sys
from pathlib import Path
if "fetch" in sys.argv[1:] and not Path({str(self.release)!r}).exists():
    subprocess.Popen([sys.executable, "-I", "-B", "-c", {writer!r}])
    signal.alarm(60)
    signal.pause()
os.execv({self.real_git!r}, [{self.real_git!r}, *sys.argv[1:]])
''')
        fake.chmod(0o755)
        self.wrapper = tools / "fixture-python"
        self.wrapper.write_text(f'''#!{self.python}
import json, runpy, subprocess, sys, time
from pathlib import Path
args = sys.argv[1:]
while args and args[0] in ("-I", "-B"):
    args.pop(0)
path = args.pop(0)
def expired(timeout):
    until = time.monotonic() + 60
    while not Path({str(self.ready)!r}).exists():
        if time.monotonic() >= until:
            raise RuntimeError("writer startup exceeded the one-minute hang detector")
        time.sleep(0.01)
    Path({str(self.expired)!r}).write_text(json.dumps([timeout, args]))
if not Path({str(self.release)!r}).exists() and "--inherited-group-timeout" in args:
    wait = subprocess.Popen.wait
    def deadline(child, timeout=None):
        if timeout is not None:
            expired(timeout)
            raise subprocess.TimeoutExpired(child.args, timeout)
        return wait(child, timeout=timeout)
    subprocess.Popen.wait = deadline
sys.argv = [path, *args]
runpy.run_path(path, run_name="__main__")
''')
        self.wrapper.chmod(0o755)
        self.environment.update(PATH=str(tools) + os.pathsep + self.environment["PATH"],
                                ALAB_PYTHON=str(self.wrapper),
                                ALAB_PACKAGE_CACHE=str(self.home / "cache"))

    def git(self, *args):
        result = subprocess.run([self.real_git, "-C", str(self.repo), *args],
                                env=self.environment, capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)

    def assert_stopped(self):
        self.assertTrue(self.ready.is_file(), "the real Git helper reached its stalled transfer")
        _, group = json.loads(self.ready.read_text())
        self.assertFalse(GUARD.group_exists(group))

    def test_package_fetch_timeout_stops_its_inherited_group_then_next_attempt_succeeds(self):
        lock = self.home / "cache.lock"
        command = ["/bin/bash", "-c", '. "$1/alab/package.sh"; package_main --version-only',
                   "network-fixture", str(self.repo)]
        with mock.patch.dict(os.environ, self.environment, clear=True):
            self.assertEqual(GUARD.run_with_cache_lock(lock, command), 1)
        self.assert_stopped()
        budget, args = json.loads(self.expired.read_text())
        self.assertTrue(0 < budget <= 120)
        self.assertIn("--inherited-group-timeout", args)
        self.assertIn("fetch", args)
        before = lock.stat()
        with lock.open("a") as released:
            fcntl.flock(released, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.release.touch()
        with mock.patch.dict(os.environ, self.environment, clear=True):
            self.assertEqual(GUARD.run_with_cache_lock(lock, command), 0)
        self.assertEqual(lock.stat().st_ino, before.st_ino)
        # The public read-only entry also owns a group, without using the cache.
        result = subprocess.run(["/bin/bash", str(self.repo / "alab/package.sh"), "--upstream-base"],
                                env=self.environment, capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(lock.stat().st_ino, before.st_ino)
        self.assertFalse((self.home / "cache").exists())

if __name__ == "__main__":
    unittest.main()
