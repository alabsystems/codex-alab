#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Opt-in macOS admission and cancellation controls for codex-release compile.

Set ALAB_TEST_CODEX_RELEASE and ALAB_TEST_RELEASE_INTENT. Cancellation controls
also require ALAB_TEST_PROJECT_CONTROLLER. Source admission uses authenticated
Agh reads in a disposable Git repository. The disk control mounts a private
16 MiB image and records when source fetching and compiler selection begin. The
pre-admission probe substitutes only the worker's selected front; the external
subcommand control exercises the original front and Targo without compiling.
"""

from contextlib import closing, contextmanager, ExitStack
import json
import os
from pathlib import Path
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

if sys.platform == "darwin":
    import fcntl


ROOT = Path(__file__).resolve().parents[2]
REQUIRED = ("ALAB_TEST_CODEX_RELEASE", "ALAB_TEST_PROJECT_CONTROLLER", "ALAB_TEST_RELEASE_INTENT")
VERB = "alab-cancel-probe"

PROBE = """#!/usr/bin/python3
import fcntl, json, os, subprocess, sys, time
from pathlib import Path
state = Path(sys.argv[-1])
if sys.argv[1] == 'daemon':
    (state / 'daemon-ready').touch()
    deadline = time.monotonic() + 180
    while state.is_dir() and not (state / 'stop-daemon').exists() and time.monotonic() < deadline:
        if (state / 'ping').exists():
            (state / 'pong').touch()
        time.sleep(.02)
    sys.exit(0)
if sys.argv[1] == '__build_admit':
    assert sys.argv[2] == os.environ['ATPKG_PROJECT_TARGO']
    assert sys.argv[3:-1] == ['--unverified', 'check', '--manifest-path']
else:
    assert sys.argv[1:-1] == ['alab-cancel-probe']
with (state / 'writer.lock').open('r+b') as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    daemon = subprocess.Popen([sys.executable, '-IB', __file__, 'daemon', str(state)],
                              start_new_session=True, stdin=subprocess.DEVNULL,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    with (state / 'ready.tmp').open('x') as output:
        json.dump({'writer': os.getpid(), 'daemon': daemon.pid, 'argv': sys.argv}, output)
    (state / 'ready.tmp').rename(state / 'ready')
    deadline = time.monotonic() + 180
    while not (state / 'go').exists():
        if time.monotonic() >= deadline:
            sys.exit(1)
        time.sleep(.02)
    with (state / 'admitted').open('a') as output:
        output.write('admitted\\n')
"""


def script(path, body):
    path.write_text(body)
    path.chmod(0o700)
    return path


def until(predicate, message):
    deadline = time.monotonic() + 60
    while not predicate():
        if time.monotonic() >= deadline:
            raise AssertionError(message)
        time.sleep(.03)


def lease_available(path):
    with path.open("rb") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return False
        return True


def native_owners(writer, outer, binary, publisher):
    owners = {}
    pid = writer
    while pid != outer:
        status = subprocess.check_output(["/bin/ps", "-ww", "-p", str(pid), "-o", "ppid=,command="],
                                         text=True)
        parent_text, command = status.lstrip().split(None, 1)
        if command.startswith(f"{publisher} toolchain run --repo "):
            owners["publisher"] = pid
        if command.startswith(f"{binary} compile-worker --repo "):
            owners["worker"] = pid
        parent = int(parent_text)
        if parent in (0, 1, pid):
            raise AssertionError("gated writer does not descend from the public compile process")
        pid = parent
    if set(owners) != {"publisher", "worker"}:
        raise AssertionError(f"native owners missing from gated writer ancestry: {owners}")
    return owners


def exit_watch(pids):
    watch = select.kqueue()
    try:
        watch.control([select.kevent(pid, filter=select.KQ_FILTER_PROC,
                                     flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT,
                                     fflags=select.KQ_NOTE_EXIT) for pid in pids], 0, 0)
    except BaseException:
        watch.close()
        raise
    return closing(watch)


class FrozenReleaseCompile(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.binary = Path(os.environ["ALAB_TEST_CODEX_RELEASE"]).resolve()
        cls.intent = Path(os.environ["ALAB_TEST_RELEASE_INTENT"]).resolve()
        parent = Path("/private/tmp/.ubd")
        parent.mkdir(exist_ok=True)
        cls.temp = tempfile.TemporaryDirectory(dir=parent, prefix="rwc.")
        cls.addClassCleanup(cls.temp.cleanup)
        cls.root = Path(cls.temp.name)
        cls.source = cls.root / "repo"
        original = Path(os.environ.get("ALAB_TEST_RELEASE_REPO", ROOT))
        subprocess.run(["git", "clone", "-q", "--local", "--shared", "--no-checkout",
                        str(original), str(cls.source)], check=True)
        origin = subprocess.check_output(["git", "-C", str(original), "remote", "get-url", "origin"],
                                         text=True).strip()
        subprocess.run(["git", "-C", str(cls.source), "remote", "set-url", "origin", origin],
                       check=True)
        # Reuse the source repository's own fetched snapshot as the starting point.
        # The native release entry still refreshes it and checks authenticated main.
        snapshot = subprocess.run(["git", "-C", str(original), "rev-parse", "-q", "--verify",
                                   "refs/agh/fetched-main^{commit}"], capture_output=True, text=True)
        if snapshot.returncode == 0:
            subprocess.run(["git", "-C", str(cls.source), "update-ref", "refs/agh/fetched-main",
                            snapshot.stdout.strip()], check=True)


@unittest.skipUnless(sys.platform == "darwin" and os.environ.get("ALAB_TEST_CODEX_RELEASE")
                     and os.environ.get("ALAB_TEST_RELEASE_INTENT"),
                     "set release binary and frozen release intent on macOS")
class NativeCompileSpace(FrozenReleaseCompile):
    @contextmanager
    def space_case(self):
        with tempfile.TemporaryDirectory(dir=self.root, prefix="space.") as tmp:
            case = Path(tmp)
            image = case / "tiny.sparseimage"
            mount = case / "volume"
            mount.mkdir()
            subprocess.run(["/usr/bin/hdiutil", "create", "-quiet", "-size", "16m", "-layout",
                            "NONE", "-fs", "HFS+", "-type", "SPARSE", str(image)], check=True)
            subprocess.run(["/usr/bin/hdiutil", "attach", "-quiet", "-nobrowse", "-mountpoint",
                            str(mount), str(image)], check=True)
            try:
                self.assertNotEqual(mount.stat().st_dev, case.stat().st_dev)
                volume = os.statvfs(mount)
                self.assertLess(volume.f_bavail * volume.f_frsize, 20 << 30)
                fetched, selected = case / "fetched", case / "selected"
                agh = shutil.which("agh")
                self.assertIsNotNone(agh)
                script(case / "agh", "#!/usr/bin/python3\nimport os,sys\nfrom pathlib import Path\n"
                       f"if sys.argv[1:3] == ['repo', 'fetch-main']: Path({str(fetched)!r}).touch()\n"
                       f"os.execv({agh!r}, [{agh!r}, *sys.argv[1:]])\n")
                publisher = script(case / "publisher", "#!/usr/bin/python3\nimport json,sys\n"
                                   "from pathlib import Path\n"
                                   f"Path({str(selected)!r}).write_text(json.dumps(sys.argv[1:]))\n"
                                   "sys.exit(1)\n")
                env = dict(os.environ, TMPDIR=tmp, PATH=tmp + os.pathsep + os.environ["PATH"])

                def check(cargo, arguments, output=None, stopped_at=None):
                    cmd = [str(self.binary), "compile", "--repo", str(self.source), "--cargo-home",
                           str(cargo), "--intent", str(self.intent), "--publisher", str(publisher)]
                    if output is not None:
                        cmd.extend(["--write", str(output)])
                    result = subprocess.run([*cmd, "--", *arguments], env=env, capture_output=True,
                                            text=True, timeout=60)
                    if stopped_at is not None:
                        self.assertEqual(result.returncode, 75,
                                         f"fetched={fetched.exists()}, selected={selected.exists()}\n{result.stderr}")
                        self.assertIn(str(stopped_at), result.stderr)
                        self.assertFalse(fetched.exists(), result.stderr)
                        self.assertFalse(selected.exists(), result.stderr)
                    else:
                        self.assertEqual(result.returncode, 1, result.stderr)
                        self.assertTrue(fetched.exists(), result.stderr)
                        self.assertTrue(selected.exists(), result.stderr)
                        argv = json.loads(selected.read_text())
                        self.assertEqual(argv[:3], ["toolchain", "run", "--repo"])
                        self.assertEqual(argv[-len(arguments)-1:], ["--", *arguments])
                        fetched.unlink()
                        selected.unlink()

                yield case, mount, check
            finally:
                subprocess.run(["/usr/bin/hdiutil", "detach", "-quiet", str(mount)], check=True)

    def test_small_output_refuses_build_before_fetch_and_selection_but_allows_version(self):
        with self.space_case() as (case, mount, check):
            cargo = case / "cargo"
            cargo.mkdir()
            output = mount / "target"
            output.mkdir()
            check(cargo, ["--version"], output)
            check(cargo, ["--unverified", "check", "--target-dir", str(output)], output, stopped_at=output)

    def test_metadata_checks_dependency_cache_without_charging_unused_output(self):
        with self.space_case() as (case, mount, check):
            small_cargo = mount / "cargo"
            small_cargo.mkdir()
            cargo = case / "cargo"
            cargo.mkdir()
            unused = mount / "unused"
            unused.mkdir()
            metadata = ["metadata", "--locked", "--offline", "--manifest-path",
                        str(self.source / "codex-rs/Cargo.toml"), "--format-version", "1"]
            check(small_cargo, [*metadata, "--no-deps"])
            check(small_cargo, metadata, stopped_at=small_cargo)
            check(cargo, metadata, unused)


@unittest.skipUnless(sys.platform == "darwin" and all(os.environ.get(k) for k in REQUIRED),
                     "set release binary, qualified publisher and frozen release intent on macOS")
class NativeCompileCancellation(FrozenReleaseCompile):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.publisher = Path(os.environ["ALAB_TEST_PROJECT_CONTROLLER"]).resolve()

    def exercise(self, mode, queued=True):
        with tempfile.TemporaryDirectory(dir=self.root, prefix="case.") as tmp:
            case = Path(tmp)
            state = case / "state"
            tools = case / "tools"
            cargo = case / "cargo"
            for directory in (state, tools, cargo):
                directory.mkdir(mode=0o700)
            (state / "writer.lock").touch()
            front = script(tools / ("targo-" + VERB), PROBE)
            os.link(front, tools / ("cargo-" + VERB))
            publisher = self.publisher
            if queued:
                # The genuine publisher still selects, leases and supervises the native
                # compile-worker. Replace only the worker's admission call with a gate.
                bridge = script(tools / "selected", "#!/usr/bin/python3\nimport os,sys\n"
                                f"os.environ['ATPKG_PROJECT_FRONT'] = {str(front)!r}\n"
                                "os.execv(sys.argv[1], sys.argv[1:])\n")
                publisher = script(tools / "publisher", "#!/usr/bin/python3\nimport os,sys\n"
                                   f"pub = {str(self.publisher)!r}\n"
                                   "split = sys.argv.index('--') + 1\n"
                                   f"os.execv(pub, [pub, *sys.argv[1:split], {str(bridge)!r}, "
                                   "*sys.argv[split:]])\n")
            env = dict(os.environ, TMPDIR=tmp, PATH=str(tools) + os.pathsep + os.environ["PATH"])
            arguments = ["--unverified", "check", "--manifest-path"] if queued else [VERB]
            command = [str(self.binary), "compile", "--repo", str(self.source), "--cargo-home",
                       str(cargo), "--intent", str(self.intent), "--publisher", str(publisher),
                       "--read", str(tools), "--write", str(state), "--", *arguments, str(state)]
            with (case / "transcript").open("wb") as log, ExitStack() as subscriptions:
                child = subprocess.Popen(command, env=env, stdout=log, stderr=log,
                                         start_new_session=True)
                daemon_exit = None
                observe_drain = None
                failed = False
                try:
                    def ready():
                        if child.poll() is not None:
                            self.fail((case / "transcript").read_text())
                        return (state / "ready").exists() and (state / "daemon-ready").exists()
                    until(ready, f"probe startup timed out: {case / 'transcript'}")
                    info = json.loads((state / "ready").read_bytes())
                    daemon_exit = exit_watch([info["daemon"]])
                    owners = native_owners(info["writer"], child.pid, self.binary, self.publisher)
                    self.assertFalse(lease_available(state / "writer.lock"))
                    watched = set(owners.values()) | {info["writer"]}
                    watch = subscriptions.enter_context(exit_watch(watched))
                    def drained():
                        watched.difference_update(e.ident for e in watch.control(None, 4, 0))
                        return not watched
                    observe_drain = drained
                    if mode == "success":
                        (state / "go").touch()
                        expected = 0
                    elif mode == "kill-group":
                        self.assertEqual(os.getpgid(child.pid), child.pid)
                        os.killpg(child.pid, signal.SIGKILL)
                        expected = -signal.SIGKILL
                    else:
                        sig = signal.SIGTERM if mode == "term" else signal.SIGKILL
                        child.send_signal(sig)
                        expected = 128 + sig if mode == "term" else -sig
                    self.assertEqual(child.wait(timeout=60), expected,
                                     (case / "transcript").read_text())
                    if mode in ("success", "term"):
                        self.assertTrue(drained(), f"public exit preceded native settlement: {watched}")
                    else:
                        until(drained, f"native owners/writer still live after {mode}: {owners}")
                    self.assertTrue(lease_available(state / "writer.lock"))
                    (state / "go").touch()
                    (state / "ping").touch()
                    until((state / "pong").exists, "detached fixture daemon did not survive")
                    self.assertEqual((state / "admitted").exists(), mode == "success")
                    self.assertFalse(any(case.rglob("completion.json")))
                    print(json.dumps({"mode": mode, "queued_front": queued, "public": child.pid,
                                      "status": child.returncode, "drained": owners,
                                      "writer": info["writer"], "daemon_responded": info["daemon"],
                                      "lease_available": True, "appended": (state / "admitted").exists()}),
                          flush=True)
                except BaseException:
                    failed = True
                    print(json.dumps({"mode": mode, "public": child.pid, "status": child.poll(),
                                      "lease_available_at_failure": lease_available(state / "writer.lock"),
                                      "appended_at_failure": (state / "admitted").exists()}), flush=True)
                    print((case / "transcript").read_text(), file=sys.stderr)
                    raise
                finally:
                    # A failed assertion opens only this fixture's gate to let its owned
                    # workload/lease settle. Never signal a descendant by numeric PID.
                    (state / "go").touch()
                    (state / "stop-daemon").touch()
                    if daemon_exit is not None:
                        with daemon_exit as daemon_watch:
                            self.assertTrue(daemon_watch.control(None, 1, 60), "fixture daemon survived cleanup")
                    child.wait(timeout=60)
                    if observe_drain is not None:
                        until(observe_drain, "native fixture processes survived their cleanup gate")
                    until(lambda: lease_available(state / "writer.lock"), "fixture failed to drain")
                    if failed:
                        print(json.dumps({"mode": mode, "cleanup_gate_open": True,
                                          "appended_after_cleanup_gate": (state / "admitted").exists(),
                                          "native_processes_drained": observe_drain is not None}),
                              flush=True)

    def test_unmodified_front_completes_the_admitted_command(self):
        self.exercise("success", queued=False)

    def test_unmodified_front_cancels_the_admitted_command(self):
        self.exercise("term", queued=False)

    def test_queued_front_cancels_on_term(self):
        self.exercise("term")

    def test_queued_front_cancels_on_public_pid_death(self):
        self.exercise("kill")

    def test_queued_front_cancels_on_public_group_death(self):
        self.exercise("kill-group")
