#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the real smoke runner with finite, inert Python process trees."""

from contextlib import contextmanager
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock


SOURCE = Path(__file__).resolve().parents[1] / "smoke/capacity_exec.py"
sys.path.insert(0, str(SOURCE.parent))
FIXTURE_PARENT = None
FIXTURES = []


class CapacityProcessTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location("capacity_process", SOURCE)
        cls.probe = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.probe)

    def setUp(self):
        self.children = []
        self.fixtures = []

    def tearDown(self):
        # These files belong only to our inert fixtures. A failed old-runner
        # assertion must not leave its orphan running until the watchdog fires.
        for root in self.fixtures:
            (root / "stop").touch(exist_ok=True)
        for child in self.children:
            if child.returncode is None:
                # Signal only a still-unreaped direct child, never a PID read
                # from a fixture. A guardian must finish its own group cleanup.
                child.terminate()
                child.wait(timeout=25)
        deadline = time.monotonic() + 20
        for root in self.fixtures:
            ready = root / "worker.json"
            if ready.exists():
                pid = json.loads(ready.read_text())["pid"]
                while self.is_executing(pid) and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertFalse(self.is_executing(pid), "finite fixture worker did not stop")

    @staticmethod
    def is_executing(pid):
        result = subprocess.run(["/bin/ps", "-p", str(pid), "-o", "stat="],
                                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True, timeout=5)
        if result.returncode == 1 and not result.stdout.strip() and not result.stderr.strip():
            return False
        if result.returncode != 0 or not result.stdout.strip():
            raise RuntimeError(f"cannot inspect fixture process {pid}: {result!r}")
        return not result.stdout.strip().startswith("Z")

    def fixture(self):
        root = Path(tempfile.mkdtemp(prefix="capacity-process-", dir=FIXTURE_PARENT))
        self.fixtures.append(root)
        FIXTURES.append(str(root))
        print(f"preserved capacity process fixture: {root}", flush=True)
        env = self.probe.fresh_environment(root)
        worker = '''import json, os, signal, sys, time
from pathlib import Path
root = Path(sys.argv[1])
signal.alarm(15)
with (root / "worker.json").open("x") as out:
    json.dump({"pid": os.getpid(), "pgid": os.getpgrp()}, out)
with (root / "worker-ready").open("x") as out:
    out.write("ready\\n")
while not (root / "stop").exists():
    time.sleep(0.01)
with (root / "worker-done").open("x") as out:
    out.write("stopped by fixture owner\\n")
'''
        leader = '''import json, os, signal, subprocess, sys, time
from pathlib import Path
root = Path(sys.argv[1])
signal.alarm(12)
worker = subprocess.Popen([sys.executable, "-I", "-B", str(root / "worker.py"), str(root)],
                          stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                          stderr=subprocess.DEVNULL)
deadline = time.monotonic() + 5
while not (root / "worker-ready").exists():
    if time.monotonic() >= deadline:
        raise RuntimeError("fixture worker never became ready")
    time.sleep(0.005)
with (root / "leader.json").open("x") as out:
    json.dump({"pid": os.getpid(), "pgid": os.getpgrp()}, out)
with (root / "leader-ready").open("x") as out:
    out.write("ready\\n")
if sys.argv[2] != "wait":
    raise SystemExit(int(sys.argv[2]))
while not (root / "stop").exists():
    time.sleep(0.01)
worker.wait(timeout=5)
'''
        for name, source in (("worker.py", worker), ("leader.py", leader)):
            with (root / name).open("x") as out:
                out.write(source)
        return root, env

    @contextmanager
    def tracked_run(self, root, *, timeout=5):
        popen, run = subprocess.Popen, subprocess.run

        def spawn(*args, **kwargs):
            child = popen(*args, **kwargs)
            self.children.append(child)
            return child

        def legacy_timeout(*args, **kwargs):
            # The old runner hardcodes 120 seconds. Bound only that original
            # command so the same real timeout regression can precede the fix.
            command = args[0] if args else kwargs.get("args", [])
            if (kwargs.get("timeout") == 120 and len(command) > 3
                    and command[0] == sys.executable and str(root / "leader.py") in command):
                kwargs["timeout"] = timeout
            return run(*args, **kwargs)

        with (mock.patch.object(self.probe, "COMMAND_TIMEOUT_SECONDS", timeout, create=True),
              mock.patch.object(subprocess, "Popen", side_effect=spawn),
              mock.patch.object(subprocess, "run", side_effect=legacy_timeout)):
            yield

    def assert_reaped(self):
        self.assertTrue(self.children, "the actual runner must start a process")
        for child in self.children:
            self.assertIsNotNone(child.returncode, "runner returned without reaping its child")
            with self.assertRaises(ChildProcessError):
                os.waitpid(child.pid, os.WNOHANG)

    def assert_worker_stopped(self, root):
        self.assertTrue((root / "leader.json").is_file(), "leader reached fixture readiness")
        worker = json.loads((root / "worker.json").read_text())
        self.assertFalse(self.is_executing(worker["pid"]),
                         "smoke returned while its redirected descendant was still executing")
        self.assert_reaped()

    def test_leader_exit_stops_its_descendant_before_return(self):
        for code in (0, 23):
            with self.subTest(code=code):
                root, env = self.fixture()
                with self.tracked_run(root):
                    result = self.probe.run(sys.executable,
                                            ["-I", "-B", str(root / "leader.py"), str(root), str(code)],
                                            root, env, "leader")
                self.assertEqual(result, code)
                self.assert_worker_stopped(root)

    def test_a_failed_interaction_keeps_its_own_error_and_closes_the_childs_pipes(self):
        root, env = self.fixture()

        class Stopped(Exception):
            pass

        def interact(child, _stdout):
            deadline = time.monotonic() + 10
            while not (root / "leader-ready").exists():
                self.assertLess(time.monotonic(), deadline, "fixture leader never became ready")
                time.sleep(0.005)
            # Buffered, and never read: closing this pipe has to flush it to a child
            # the runner has by then stopped.
            child.stdin.write(b"never read\n")
            raise Stopped("the interaction's own error")

        with self.tracked_run(root):
            with self.assertRaises(Stopped):
                self.probe.run(sys.executable,
                               ["-I", "-B", str(root / "leader.py"), str(root), "wait"],
                               root, env, "interact", interact=interact)
        self.assert_worker_stopped(root)
        guardian = self.children[0]
        self.assertTrue(guardian.stdin.closed and guardian.stdout.closed)

    def test_signal_exit_preserves_the_negative_raw_status(self):
        root, env = self.fixture()
        with self.tracked_run(root):
            result = self.probe.run(sys.executable,
                                    ["-I", "-B", "-c", "import os,signal; os.kill(os.getpid(),signal.SIGUSR1)"],
                                    root, env, "signal")
        self.assertEqual(result, -signal.SIGUSR1)
        self.assert_reaped()

    def test_sigterm_smoke_parent_reaps_its_supervisor_and_stops_descendant(self):
        root, env = self.fixture()
        script = f'''import importlib.util, json, signal, subprocess, sys
from pathlib import Path
root = Path({str(root)!r})
signal.alarm(20)
sys.path.insert(0, {str(SOURCE.parent)!r})
spec = importlib.util.spec_from_file_location("capacity_sigterm", {str(SOURCE)!r})
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)
spawn = subprocess.Popen
children = []
def tracked(*args, **kwargs):
    child = spawn(*args, **kwargs)
    children.append(child)
    with (root / "supervisor.json").open("x") as out:
        json.dump({{"pid": child.pid, "argv": child.args}}, out)
    return child
subprocess.Popen = tracked
try:
    probe.run(sys.executable, ["-I", "-B", str(root / "leader.py"), str(root), "wait"],
              root, dict({env!r}), "cancel")
finally:
    with (root / "parent-finally.json").open("x") as out:
        json.dump({{"statuses": [child.returncode for child in children]}}, out)
'''
        with (root / "smoke-parent.py").open("x") as out:
            out.write(script)
        with (root / "parent-stdout.txt").open("xb") as out, (root / "parent-stderr.txt").open("xb") as err:
            child = subprocess.Popen([sys.executable, "-I", "-B", str(root / "smoke-parent.py")],
                                     stdin=subprocess.DEVNULL, stdout=out, stderr=err,
                                     env=env, start_new_session=True)
            self.children.append(child)
            deadline = time.monotonic() + 10
            while not (root / "leader-ready").exists():
                if child.poll() is not None or time.monotonic() >= deadline:
                    self.fail("smoke parent never reached actual descendant readiness")
                time.sleep(0.005)
            child.send_signal(signal.SIGTERM)
            code = child.wait(timeout=25)
        self.assertNotEqual(code, 0)
        self.assert_worker_stopped(root)
        final = json.loads((root / "parent-finally.json").read_text())
        self.assertEqual(len(final["statuses"]), 1)
        self.assertIsNotNone(final["statuses"][0], "smoke parent did not reap its supervisor")
        supervisor = json.loads((root / "supervisor.json").read_text())
        self.assertFalse(self.is_executing(supervisor["pid"]))


if __name__ == "__main__":
    unittest.main()
