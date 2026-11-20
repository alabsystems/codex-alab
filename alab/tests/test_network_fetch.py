#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Real Git stand-ins exercise scoped deadlines and the next ordinary attempt."""

import fcntl
import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'

# The native front-end and its writer have separate process groups, as in Agh.
NATIVE_CLIENT = r'''
import fcntl, json, os, signal, subprocess, sys, time
from pathlib import Path

root = Path(os.environ["NATIVE_PACKAGE_FIXTURE"])
fallback = root / "release-test-writer"
stop_requested = False

def stop(signum, frame):
    global stop_requested
    stop_requested = True

signal.signal(signal.SIGTERM, stop)
signal.alarm(60)
if "--writer" in sys.argv:
    (root / "writer-ready.json").write_text(json.dumps([os.getpid(), os.getpgrp()]))
    while not (stop_requested or fallback.is_file()):
        time.sleep(0.01)
    if stop_requested:
        with (root / "cache.lock").open("rb") as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                (root / "writer-observed-cache-lock").touch()
    sys.exit(0)
if "help" in sys.argv:
    print("--ref --into --input --anonymous")
elif "public-refs" in sys.argv:
    if os.environ.get("WAIT_FOR_SETUP_DEADLINE") and not (root / "allow-next-attempt").exists():
        with (root / "metadata.lock").open("ab") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            (root / "writer-ready.json").write_text(json.dumps([os.getpid(), os.getpgrp()]))
            while not (stop_requested or fallback.is_file()):
                time.sleep(0.01)
        sys.exit(1)
    prefix = sys.argv[sys.argv.index("--prefix") + 1]
    for ref in ("refs/heads/main", "refs/tags/rust-v0.164.0"):
        if ref.startswith(prefix):
            print(os.environ["NATIVE_FIXTURE_HEAD"] + "\t" + ref)
elif "fetch" in sys.argv:
    if (root / "allow-next-attempt").is_file():
        receiver = sys.argv[sys.argv.index("fetch") + 2]
        for row in json.loads(Path(sys.argv[sys.argv.index("--input") + 1]).read_bytes()):
            subprocess.run([os.environ["NATIVE_FIXTURE_GIT"], "-C", receiver, "update-ref",
                            row["into"], row["expected_oid"]], check=True)
        sys.exit(0)
    (root / "native-group").write_text(str(os.getpgrp()))
    child = subprocess.Popen([sys.executable, "-IB", __file__, "--writer"],
                             start_new_session=True, stdin=subprocess.DEVNULL,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    while not (stop_requested or fallback.is_file()):
        time.sleep(0.01)
    if stop_requested:
        child.terminate()
    child.wait(timeout=60)
    (root / "native-reaped-writer").touch()
    if stop_requested:
        print(json.dumps({"error": {"v": 1, "exit": 130, "kind": "interrupted",
                                     "side_effect": "none"}}), file=sys.stderr)
        sys.exit(130)
else:
    raise RuntimeError("unexpected native fixture request")
'''


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
        for name in ("package.sh", "lib.sh", "upstream_intake.py", "command_guard.py", "github.py", "cli_status.py", "file_hash.py"):
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
        fake.write_text(PYTHON_ENTRY + f'''import os, signal, subprocess, sys
from pathlib import Path
if "fetch" in sys.argv[1:] and not Path({str(self.release)!r}).exists():
    subprocess.Popen([sys.executable, "-I", "-B", "-c", {writer!r}])
    signal.alarm(60)
    signal.pause()
os.execv({self.real_git!r}, [{self.real_git!r}, *sys.argv[1:]])
''')
        fake.chmod(0o755)
        self.wrapper = tools / "fixture-python"
        self.wrapper.write_text(PYTHON_ENTRY + f'''import json, runpy, subprocess, sys, time
from pathlib import Path
args = sys.argv[1:]
while args and args[0] in ("-I", "-B"):
    args.pop(0)
path = args.pop(0)
if path == "-c":
    exec(args[0])
    raise SystemExit(0)
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
        if timeout is not None and not Path({str(self.expired)!r}).is_file():
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

    def test_standalone_setup_retires_ref_discovery_at_its_deadline(self):
        shutil.copyfile(ROOT / "alab/setup.sh", self.repo / "alab/setup.sh")
        native = self.home / "setup-native-client"
        native.write_text(PYTHON_ENTRY + NATIVE_CLIENT)
        native.chmod(0o755)
        head = subprocess.check_output([self.real_git, "-C", str(self.repo), "rev-parse", "HEAD"],
                                       text=True).strip()
        environment = dict(self.environment, ALAB_AGH=str(native), NATIVE_PACKAGE_FIXTURE=str(self.home),
                           WAIT_FOR_SETUP_DEADLINE="1", NATIVE_FIXTURE_HEAD=head,
                           NATIVE_FIXTURE_GIT=self.real_git, TMPDIR=str(self.home))
        command = ["/bin/bash", str(self.repo / "alab/setup.sh")]
        try:
            first = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=60)
            self.assertNotEqual(first.returncode, 0)
            self.assertIn("deadline", first.stderr)
            self.assertTrue(self.ready.is_file(), first.stderr)
            with (self.home / "metadata.lock").open("rb") as observer:
                fcntl.flock(observer, fcntl.LOCK_EX | fcntl.LOCK_NB)
        finally:
            (self.home / "release-test-writer").touch()
        self.release.touch()
        retry = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=60)
        self.assertEqual(retry.returncode, 0, retry.stderr)
        actual = subprocess.check_output([self.real_git, "-C", str(self.repo), "rev-parse",
                                          "refs/remotes/upstream/main"], text=True).strip()
        self.assertEqual(actual, head)

    def test_package_deadline_retires_native_writer_before_unlock_and_retry(self):
        native = self.home / "native-client"
        native.write_text(PYTHON_ENTRY + NATIVE_CLIENT)
        native.chmod(0o755)
        self.git("remote", "set-url", "upstream", "https://github.com/fixture/transfer.git")
        head = subprocess.check_output([self.real_git, "-C", str(self.repo), "rev-parse", "HEAD"],
                                       text=True).strip()
        environment = dict(self.environment, ALAB_AGH=str(native), NATIVE_PACKAGE_FIXTURE=str(self.home),
                           NATIVE_FIXTURE_HEAD=head, NATIVE_FIXTURE_GIT=self.real_git, TMPDIR=str(self.home))
        lock = self.home / "cache.lock"
        command = [sys.executable, "-IB", str(self.repo / "alab/command_guard.py"),
                   "--cache-lock", str(lock), "--", "/bin/bash", "-c",
                   '. "$1/alab/package.sh"; package_main --version-only', "native-fixture", str(self.repo)]
        retired = self.home / "native-reaped-writer"
        try:
            first = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=60)
            self.assertEqual(first.returncode, 1, first.stderr)
            self.assertTrue(self.ready.is_file(), first.stderr)
            budget, args = json.loads(self.expired.read_bytes())
            self.assertEqual(args[args.index("--inherited-group-timeout") + 1], "120")
            self.assertTrue(0 < budget <= 120)
            self.assertIn("deadline", first.stderr)
            self.assertTrue((self.home / "writer-observed-cache-lock").is_file(),
                            "the writer must see the package lock held while retiring on TERM")
            self.assertTrue(retired.is_file())
            native_group = int((self.home / "native-group").read_bytes())
            _, writer_group = json.loads(self.ready.read_bytes())
            self.assertNotEqual(writer_group, native_group)
            self.assertFalse(GUARD.group_exists(writer_group))
            self.assertFalse(GUARD.group_exists(native_group))
        finally:
            # Release the two synthetic processes even when an older guardian lost ownership.
            (self.home / "release-test-writer").touch()
            if self.ready.exists():
                deadline = time.monotonic() + 60
                while not retired.is_file():
                    if time.monotonic() >= deadline:
                        self.fail("native fixture did not finish after release")
                    time.sleep(0.01)

        with lock.open("rb") as unlocked:
            fcntl.flock(unlocked, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.release.touch()
        retry = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=60)
        self.assertEqual(retry.returncode, 0, retry.stderr)
        self.assertIn("0.165.0-dev", retry.stdout)


if __name__ == "__main__":
    unittest.main()
