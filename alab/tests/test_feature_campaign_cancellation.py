#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise feature-stage custody with a short controller and the real source-selected lease."""

import fcntl
import os
from pathlib import Path
import shlex
import signal
import subprocess
import sys
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path(__file__).resolve()


def until(predicate, label, timeout=60):
    deadline = time.monotonic() + timeout
    while not predicate():
        if time.monotonic() >= deadline:
            raise RuntimeError(label)
        time.sleep(.02)


def wait_for_controller(process, directory, timeout=60):
    ready = directory / "ready"
    try:
        until(lambda: ready.exists() or process.poll() is not None,
              "native controller did not start before the readiness deadline", timeout)
        if not ready.exists():
            raise RuntimeError(f"launcher exited {process.returncode} before controller readiness")
    except RuntimeError as error:
        # The collector and its native selector write separate logs. Both
        # disappear with the temporary directory after this subtest.
        for name in ("collector.log", "stage.log"):
            try:
                with (directory / name).open("rb") as stream:
                    stream.seek(0, os.SEEK_END)
                    stream.seek(max(0, stream.tell() - 4096))
                    tail = stream.read(4096).decode(errors="replace").strip()
            except FileNotFoundError:
                continue
            except OSError as exc:
                error.add_note(f"Could not read {name}: {exc}")
                continue
            if tail:
                error.add_note(f"{name} (last 4096 bytes):\n{tail}")
        raise


def locked(path):
    with path.open("r+b") as stream:
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return True
    return False


def controller(directory):
    def cancel(_signum, _frame):
        (directory / "cancel").touch()

    signal.signal(signal.SIGTERM, cancel)
    (directory / "ready").touch()
    until((directory / "finish").exists, "controller was never released")
    print("native controller settled", flush=True)
    return 143 if (directory / "cancel").exists() else 0


def collect(argv, directory):
    sys.path.insert(0, str(ROOT / "alab"))
    import feature_campaign as campaign

    with campaign.lock(directory / "workspace.lock"):
        return campaign.run_stage(argv, ROOT, directory / "stage.log")


@unittest.skipUnless(Path('/usr/bin/sandbox-exec').is_file(), "requires the protected macOS feature runner")
class FeatureStageCancellationTests(unittest.TestCase):
    def invocation(self, directory, native=None):
        # Retain the actual outer recipe and its executable/root PID. A test front
        # selects the short fixture as native controller so no compiler has to run.
        source = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
        front = directory / "front"
        handler = directory / "front.py"
        front.write_text("#!/bin/sh\nexec " + shlex.join([sys.executable, "-I", "-B", "-S", str(handler)]) + ' "$@"\n')
        handler.write_text(f"""import os, sys
from pathlib import Path
args = sys.argv[1:]
split = args.index("--")
assert args[:split] == ["toolchain", "run", "--repo", {str(ROOT)!r}, "--source", {source!r}]
assert args[split+1:split+4] == ["/bin/bash", {str(ROOT / "alab/feature_recipes/run.sh")!r}, "selected"]
Path({str(directory / "front.pid")!r}).write_text(str(os.getpid()))
native = {native!r}
if native:
    os.execv(native, [native, *args[:split+1], sys.executable, "-I", "-B", "-S", {str(SCRIPT)!r}, "controller", {str(directory)!r}])
""")
        front.chmod(0o700)
        env = dict(os.environ, ALAB_ATPKG_PUBLISH=str(front))
        argv = [str(ROOT / "alab/feature_recipes/run.sh"), "outer", str(ROOT), source,
                str(directory), "tui-fixtures"]
        return argv, env

    def test_outer_recipe_execs_native_publisher_at_original_pid(self):
        with tempfile.TemporaryDirectory() as scratch:
            directory = Path(scratch)
            argv, env = self.invocation(directory)
            with subprocess.Popen(argv, cwd=ROOT, env=env) as stage:
                self.assertEqual(stage.wait(timeout=60), 0)
                self.assertEqual(int((directory / "front.pid").read_text()), stage.pid)

    def test_readiness_failure_reports_the_native_selector_before_cancellation(self):
        with tempfile.TemporaryDirectory() as scratch:
            directory = Path(scratch)
            (directory / "collector.log").write_text("fixture collector entered\n")
            with (directory / "stage.log").open("wb") as stream:
                argv = [sys.executable, "-I", "-c",
                        "import sys; print('x' * 5000); print('fixture selector refused'); sys.exit(17)"]
                with subprocess.Popen(argv, stdout=stream) as process:
                    with self.assertRaisesRegex(RuntimeError, "exited 17 before controller readiness") as raised:
                        wait_for_controller(process, directory)
            notes = "\n".join(raised.exception.__notes__)
            self.assertIn("fixture collector entered", notes)
            self.assertIn("fixture selector refused", notes)
            self.assertNotIn("x" * 4096, notes)
            with (directory / "stage.log").open("wb") as stream:
                argv = [sys.executable, "-I", "-c",
                        "import time; print('fixture selector waiting', flush=True); time.sleep(30)"]
                with subprocess.Popen(argv, stdout=stream) as waiting:
                    try:
                        until(lambda: (directory / "stage.log").stat().st_size, "fixture did not write", timeout=5)
                        with self.assertRaisesRegex(RuntimeError, "readiness deadline") as timed_out:
                            wait_for_controller(waiting, directory, timeout=.03)
                        self.assertIn("fixture selector waiting", "\n".join(timed_out.exception.__notes__))
                    finally:
                        waiting.terminate()
                        waiting.wait(timeout=5)

    @unittest.skipUnless("ALAB_TEST_PROJECT_CONTROLLER" in os.environ, "requires a qualified native publisher")
    def test_selected_stage_and_campaign_loss_wait_for_native_settlement(self):
        for mode in ("stage-term", "collector-term", "collector-kill"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as scratch:
                directory = Path(scratch)
                argv, env = self.invocation(directory, os.environ["ALAB_TEST_PROJECT_CONTROLLER"])
                collector = mode != "stage-term"
                if collector:
                    argv = [sys.executable, "-I", "-B", "-S", str(SCRIPT), "collect", str(directory), *argv]
                capture = directory / ("collector.log" if collector else "stage.log")
                with capture.open("xb") as output, subprocess.Popen(argv, cwd=ROOT, env=env,
                                                                   stdout=output, stderr=output) as process:
                    try:
                        wait_for_controller(process, directory)
                        process.send_signal(signal.SIGKILL if mode == "collector-kill" else signal.SIGTERM)
                        until((directory / "cancel").exists, "native controller missed cancellation")
                        if mode == "collector-kill":
                            self.assertEqual(process.wait(timeout=60), -signal.SIGKILL)
                        else:
                            self.assertIsNone(process.poll())
                        if collector:
                            self.assertTrue(locked(directory / "workspace.lock"))
                    finally:
                        (directory / "finish").touch()
                        status = process.wait(timeout=60)
                    if collector:
                        until(lambda: not locked(directory / "workspace.lock"), "workspace remained owned")
                    self.assertEqual(status, -signal.SIGKILL if mode == "collector-kill" else 143)
                    self.assertIn(b"native controller settled\n", (directory / "stage.log").read_bytes())


if __name__ == "__main__":
    if sys.argv[1:2] == ["controller"]:
        sys.exit(controller(Path(sys.argv[2])))
    if sys.argv[1:2] == ["collect"]:
        sys.exit(collect(sys.argv[3:], Path(sys.argv[2])))
    unittest.main()
