#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Real guardian death, writer retirement and inherited cache-lock custody."""

import errno
import fcntl
from contextlib import ExitStack
import json
import os
from pathlib import Path
import select
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


def await_guardian_marker(guardian, marker, log, description):
    def ready():
        status = guardian.poll()
        if status is not None:
            lines = Path(log.name).read_text(errors="replace").splitlines()
            detail = lines[-1] if lines else "empty guardian log"
            raise AssertionError(
                f"{description}: guardian exited {status} while waiting for {marker.name} "
                f"({log.name}): {detail}"
            )
        return marker.is_file()

    await_condition(ready, description)


def attempt_cleanup(errors, description, action):
    try:
        action()
    except BaseException as error:
        errors.append((description, error))


def cleanup_error(primary, errors, *, phase):
    if not errors:
        return primary
    selected = primary
    if selected is None or isinstance(selected, Exception):
        selected = next((error for _, error in errors if not isinstance(error, Exception)),
                        selected)
    if selected is None:
        selected = errors[0][1]
    if primary is not None and selected is not primary:
        selected.add_note(f"{phase} failed: {primary!r}")
    for operation, error in errors:
        selected.add_note(f"cleanup {operation} failed: {error!r}")
    return selected


def cleanup_fixture(stop, guardian, watches, owner, errors, *, primary, kill, description):
    """Attempt all owned cleanup; cancellation wins over ordinary failures."""
    attempt_cleanup(errors, "publish private stop marker",
                    lambda: stop.write_text("finish private fixture cleanup"))

    def signal_guardian():
        if guardian.poll() is None:
            guardian.kill() if kill else guardian.terminate()

    attempt_cleanup(errors, "signal owned guardian", signal_guardian)
    attempt_cleanup(errors, "wait for owned guardian", lambda: guardian.wait(timeout=60))
    for watch in watches:
        attempt_cleanup(errors, "wait for original process exit",
                        lambda watch=watch: await_condition(watch.exited, description))
    # Registered callbacks capture their own failures, so all handles close even
    # when a preceding close raises a cancellation or an ordinary OS error.
    attempt_cleanup(errors, "release original process exit handles", owner.close)
    selected = cleanup_error(primary, errors, phase="fixture body")
    if selected is not None and selected is not primary:
        raise selected


class OriginalProcessExit:
    """Test-local exit handle armed before the held original acknowledges it."""

    def __init__(self, pid):
        self.pid = pid
        self.queue = None
        self.descriptor = None
        self.poller = None
        self.observed = False
        try:
            if callable(getattr(select, "kqueue", None)):
                self.backend = "kqueue"
                self.queue = select.kqueue()
                event = select.kevent(pid, filter=select.KQ_FILTER_PROC,
                                      flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT,
                                      fflags=select.KQ_NOTE_EXIT)
                exited = self.kqueue_events(self.queue.control([event], 1, 0))
            elif callable(getattr(os, "pidfd_open", None)):
                self.backend = "pidfd"
                self.descriptor = os.pidfd_open(pid, 0)
                self.poller = select.poll()
                self.poller.register(self.descriptor, select.POLLIN | select.POLLHUP)
                exited = self.exited()
            else:
                raise RuntimeError("identity-bound process exit watching is unavailable")
            if exited:
                raise RuntimeError("original process exited before watcher acknowledgement")
        except BaseException as error:
            errors = []
            attempt_cleanup(errors, "close process exit watcher", self.close)
            selected = cleanup_error(error, errors, phase="process exit watcher registration")
            if selected is not error:
                raise selected
            raise

    def kqueue_events(self, events):
        if not events:
            return False
        if len(events) != 1:
            raise RuntimeError("unexpected process-exit event count")
        event = events[0]
        if event.flags & select.KQ_EV_ERROR:
            raise OSError(event.data, os.strerror(event.data))
        if (event.ident != self.pid or event.filter != select.KQ_FILTER_PROC
                or not event.fflags & select.KQ_NOTE_EXIT):
            raise RuntimeError("unexpected process-exit event identity or kind")
        return True

    def exited(self):
        if self.observed:
            return True
        if self.queue is not None:
            self.observed = self.kqueue_events(self.queue.control(None, 1, 0))
        elif self.descriptor is not None:
            events = self.poller.poll(0)
            if events:
                if (len(events) != 1 or events[0][0] != self.descriptor
                        or events[0][1] & ~(select.POLLIN | select.POLLHUP)
                        or not events[0][1] & (select.POLLIN | select.POLLHUP)):
                    raise OSError(errno.EIO, "unexpected pidfd exit event")
                self.observed = True
        else:
            raise RuntimeError("process exit watcher is closed")
        return self.observed

    def close(self):
        if self.queue is not None:
            queue, self.queue = self.queue, None
            queue.close()
        if self.descriptor is not None:
            descriptor, self.descriptor = self.descriptor, None
            os.close(descriptor)


class GuardianLifelineTests(unittest.TestCase):
    def test_readiness_reports_guardian_exit_even_if_marker_exists(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "ready"
            marker.touch()
            with (marker.parent / "command.log").open("w") as log:
                with subprocess.Popen(
                    [sys.executable, "-IB", "-c", "print('fixture refused'); raise SystemExit(75)"],
                    stdout=log,
                ) as guardian:
                    guardian.wait(timeout=60)
                    with self.assertRaisesRegex(AssertionError, "exited 75 .*ready .*fixture refused"):
                        await_guardian_marker(guardian, marker, log, "fixture readiness")

    def test_sigkill_guardian_keeps_flock_until_child_and_grandchild_are_gone(self):
        self.assert_guardian_signal(signal.SIGKILL, -signal.SIGKILL)

    def test_sigterm_guardian_keeps_flock_until_child_and_grandchild_are_gone(self):
        self.assert_guardian_signal(signal.SIGTERM, 128 + signal.SIGTERM)

    def assert_guardian_signal(self, signum, returncode):
        directory = Path(tempfile.mkdtemp(prefix="alab-lifeline-kill-", suffix=".noindex"))
        print(f"preserved lifeline fixture: {directory}", flush=True)
        ready, descendant, stop = (directory / name for name in ("ready", "descendant", "stop"))
        armed, watched, writer_watched = (directory / name for name in
                                          ("watch-armed", "command-watched", "writer-watched"))
        lock = directory / "cache.lock"
        lock.write_bytes(b"existing lock identity and contents\n")
        lock_identity = (lock.stat().st_dev, lock.stat().st_ino, lock.stat().st_mode)
        writer = f'''
import json, os, signal, time
from pathlib import Path
signal.alarm(60)
pending=Path({str(descendant)!r} + ".pending")
pending.write_text(json.dumps([os.getpid(), os.getpgrp()]))
os.replace(pending, {str(descendant)!r})
while not Path({str(armed)!r}).exists() and not Path({str(stop)!r}).exists(): time.sleep(.01)
Path({str(writer_watched)!r}).touch()
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
pending=Path({str(ready)!r} + ".pending")
pending.write_text(json.dumps([os.getpid(), os.getpgrp()]))
os.replace(pending, {str(ready)!r})
while not Path({str(armed)!r}).exists() and not Path({str(stop)!r}).exists(): time.sleep(.01)
while not Path({str(writer_watched)!r}).exists(): time.sleep(.01)
Path({str(watched)!r}).touch()
while not Path({str(stop)!r}).exists(): time.sleep(0.01)
child.wait(timeout=60)
'''
        identity = None
        watches = []
        cleanup_errors = []
        watch_owner = ExitStack()
        with (directory / "command.log").open("w") as log:
            guardian = subprocess.Popen(
                [sys.executable, "-I", "-B", str(GUARD), "--cache-lock", str(lock),
                 "--min-free-gib", "20", "--disk-path", str(directory), "--",
                 sys.executable, "-I", "-B", "-c", command],
                start_new_session=True, stdin=subprocess.DEVNULL,
                stdout=log, stderr=subprocess.STDOUT,
            )
            primary = None
            try:
                await_guardian_marker(guardian, ready, log, "actual command readiness")
                identity = json.loads(ready.read_text())
                writer_identity = json.loads(descendant.read_text())
                self.assertEqual(writer_identity[1], identity[1])
                for original in (identity, writer_identity):
                    watch = OriginalProcessExit(original[0])
                    watch_owner.callback(attempt_cleanup, cleanup_errors,
                                         "close original process exit handle", watch.close)
                    watches.append(watch)
                armed.write_text("both original process exit handles are armed")
                await_guardian_marker(guardian, watched, log, "original cache writers acknowledge watches")
                self.assertTrue(writer_watched.is_file())
                self.assertTrue(all(not watch.exited() for watch in watches))
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
                    self.assertTrue(all(watch.exited() for watch in watches),
                                    "cache reopened before both original writers exited")
                self.assertEqual((lock.stat().st_dev, lock.stat().st_ino, lock.stat().st_mode),
                                 lock_identity)
                self.assertEqual(lock.read_bytes(), b"existing lock identity and contents\n")
            except BaseException as error:
                primary = error
                raise
            finally:
                # The baseline has no custodian: ask only these harmless owned writers to
                # finish through their private marker; never signal an unleased PGID.
                cleanup_fixture(stop, guardian, watches, watch_owner, cleanup_errors,
                                primary=primary, kill=False,
                                description="original cache fixture process exit")

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


class GuardianReceiptLossTests(unittest.TestCase):
    def test_sigkill_before_command_completion_leaves_no_completed_receipt(self):
        self.exercise(after_completion=False)

    def test_sigkill_after_recorded_command_completion_preserves_its_outcome(self):
        self.exercise(after_completion=True)

    def exercise(self, *, after_completion):
        directory = Path(tempfile.mkdtemp(prefix="alab-receipt-loss-", suffix=".noindex")).resolve()
        print(f"preserved receipt-loss fixture: {directory}", flush=True)
        receipt = directory / "receipt.json"
        born = directory / "command-born.json"
        ready = directory / "command-ready.json"
        writer_born = directory / "writer-born.json"
        writer_ready = directory / "writer-ready.json"
        returned = directory / "wrapper-returned.json"
        armed = directory / "watch-armed"
        release, stop = directory / "release-command", directory / "stop"
        writer, command, wrapper = (directory / name for name in
                                    ("writer.py", "command.py", "wrapper.py"))
        writer.write_text('''
import json, os, signal, sys, time
from pathlib import Path
root = Path(sys.argv[1])
signal.alarm(60)
signal.signal(signal.SIGTERM, signal.SIG_IGN)
pending = root / "writer-pending.json"
pending.write_text(json.dumps({"pid": os.getpid(), "group": os.getpgrp()}))
os.replace(pending, root / "writer-born.json")
while not (root / "watch-armed").exists() and not (root / "stop").exists():
    time.sleep(.01)
pending.write_text(json.dumps({"pid": os.getpid(), "group": os.getpgrp()}))
os.replace(pending, root / "writer-ready.json")
while not (root / "stop").exists(): time.sleep(.01)
''')
        command.write_text('''
import json, os, signal, subprocess, sys, time
from pathlib import Path
root = Path(sys.argv[1])
signal.alarm(60)
assert (root / "receipt.json").read_bytes() == b""
subprocess.Popen([sys.executable, "-I", "-B", str(root / "writer.py"), str(root)],
                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
while not (root / "writer-born.json").is_file(): time.sleep(.01)
pending = root / "command-pending.json"
pending.write_text(json.dumps({"pid": os.getpid(), "group": os.getpgrp(),
                               "custodian": os.getppid()}))
os.replace(pending, root / "command-born.json")
while not (root / "watch-armed").exists() and not (root / "stop").exists():
    time.sleep(.01)
while not (root / "writer-ready.json").is_file(): time.sleep(.01)
pending.write_text(json.dumps({"pid": os.getpid(), "group": os.getpgrp(),
                               "custodian": os.getppid()}))
os.replace(pending, root / "command-ready.json")
while not (root / "release-command").exists() and not (root / "stop").exists():
    time.sleep(.01)
(root / "command-returning").touch()
''')
        supervised = [sys.executable, "-I", "-B", str(command), str(directory)]
        if after_completion:
            # Hold only this fixture wrapper after the actual API returned. Do
            # not race the CLI's natural exit or insert a production observer.
            wrapper.write_text('''
import json, os, runpy, signal, sys, time
from pathlib import Path
signal.alarm(60)
root = Path(sys.argv[1])
run_recorded = runpy.run_path(sys.argv[2])["run_recorded"]
code = run_recorded([sys.executable, "-I", "-B", str(root / "command.py"), str(root)],
                    result_file=root / "receipt.json")
pending = root / "wrapper-pending.json"
pending.write_text(json.dumps({"supervised_command_normalized_exit_status": code}))
os.replace(pending, root / "wrapper-returned.json")
while not (root / "stop").exists(): time.sleep(.01)
raise SystemExit(code)
''')
            launch = [sys.executable, "-I", "-B", str(wrapper), str(directory), str(GUARD)]
        else:
            launch = [sys.executable, "-I", "-B", str(GUARD), "--result-file", str(receipt),
                      "--", *supervised]
        identity = None
        watches = []
        cleanup_errors = []
        watch_owner = ExitStack()
        with (directory / "guardian.log").open("w") as log:
            guardian = subprocess.Popen(launch, start_new_session=True, stdin=subprocess.DEVNULL,
                                        stdout=log, stderr=subprocess.STDOUT)

            primary = None
            try:
                await_guardian_marker(guardian, born, log, "held actual command birth")
                identity = json.loads(born.read_text())
                writer_identity = json.loads(writer_born.read_text())
                # Both originals acknowledge only after both native handles are
                # armed. A recycled numeric PID cannot produce that handshake.
                for original in (identity, writer_identity):
                    watch = OriginalProcessExit(original["pid"])
                    watch_owner.callback(attempt_cleanup, cleanup_errors,
                                         "close original process exit handle", watch.close)
                    watches.append(watch)
                armed.write_text("both original process exit handles are armed")
                await_guardian_marker(guardian, ready, log, "watcher acknowledgement")
                self.assertEqual(json.loads(ready.read_text()), identity)
                self.assertEqual(json.loads(writer_ready.read_text()), writer_identity)
                self.assertTrue(all(not watch.exited() for watch in watches))
                self.assertEqual(identity["group"], identity["pid"])
                self.assertEqual(writer_identity["group"], identity["group"])
                self.assertNotEqual(identity["custodian"], guardian.pid)
                self.assertEqual(receipt.read_bytes(), b"")
                self.assertFalse(release.exists())
                self.assertFalse(stop.exists())
                self.assertFalse((directory / "command-returning").exists())

                if after_completion:
                    release.write_text("allow the supervised command to return")
                    await_guardian_marker(guardian, returned, log, "actual run_recorded return")
                    observed = json.loads(returned.read_text())
                    self.assertEqual(observed["supervised_command_normalized_exit_status"], 0)
                    data = receipt.read_bytes()
                    completed = json.loads(data)
                    self.assertEqual(completed["kind"], "guardian-command-completion")
                    self.assertIs(completed["completed"], True)
                    self.assertEqual(completed["argv"], supervised)
                    self.assertEqual(completed["normalized_exit_status"], 0)
                    self.assertTrue(all(watch.exited() for watch in watches),
                                    "run_recorded returned before its original command/writer exited")
                else:
                    data = receipt.read_bytes()
                    with self.assertRaises(json.JSONDecodeError):
                        json.loads(data)

                # Popen retains the direct child until wait. Its readiness gate
                # keeps it alive; never signal the reaped command PID or group.
                guardian.kill()
                guardian_status = guardian.wait(timeout=60)
                self.assertEqual(guardian_status, -signal.SIGKILL)
                await_condition(lambda: all(watch.exited() for watch in watches),
                                "original command/writer exit after guardian loss")
                self.assertEqual(receipt.read_bytes(), data)
                if not after_completion:
                    self.assertFalse(release.exists())
                    self.assertFalse((directory / "command-returning").exists())
                    with self.assertRaises(json.JSONDecodeError):
                        json.loads(receipt.read_bytes())
                (directory / "observation.json").write_text(json.dumps({
                    "phase": "after-completion" if after_completion else "before-completion",
                    "guardian_raw_returncode": guardian_status,
                    "supervised_command_normalized_exit_status": 0 if after_completion else None,
                    "completed_receipt": after_completion,
                    "original_command_exited": watches[0].exited(),
                    "original_writer_exited": watches[1].exited(),
                    "exit_watch_backends": [watch.backend for watch in watches],
                    "command_status_scope": "Receipt/run_recorded observation; no guessed raw child wait status",
                }) + "\n")
            except BaseException as error:
                primary = error
                raise
            finally:
                # A failed assertion must not leak these inert writers. Release
                # them cooperatively; only the still-owned Popen may be killed.
                cleanup_fixture(stop, guardian, watches, watch_owner, cleanup_errors,
                                primary=primary, kill=True,
                                description="original fixture process exit")


if __name__ == "__main__":
    unittest.main()
