#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Actual child lifetimes, including Python 3.9's non-reaping kqueue observer."""

import errno
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("cache_command_guard", ROOT / "alab/command_guard.py")
GUARD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GUARD)


class CommandGuardTests(unittest.TestCase):
    def test_refused_lock_symlink_cannot_redirect_post_package_maintenance(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-guard-maintenance-path-"))
        cache, decoy = directory / "cache", directory / "decoy"
        cache.mkdir()
        decoy.mkdir()
        (decoy / "lock").touch()
        lock = cache / "lock"
        lock.symlink_to(decoy / "lock")
        observed = []
        with (mock.patch.object(sys, "argv", ["guard", "--cache-lock", str(lock),
                                             "--package-source-repo", str(directory), "--", "unused"]),
              mock.patch.object(GUARD.runpy, "run_path", return_value={
                  "compact": lambda selected: observed.append(selected)}),
              mock.patch.object(GUARD, "run", side_effect=AssertionError("refused lock started command"))):
            self.assertEqual(GUARD.main(), 1)
        self.assertEqual(observed, [cache.resolve()], "maintenance belongs to the named cache parent")
        self.assertTrue(lock.is_symlink())
        self.assertEqual((decoy / "lock").read_bytes(), b"")

    def setUp(self):
        self.children = []
        self.mask = signal.pthread_sigmask(signal.SIG_BLOCK, set())
        self.handlers = {sig: signal.getsignal(sig)
                         for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}

    def tearDown(self):
        for child in self.children:
            if child.returncode is None:
                child.close_lifeline()
                child.wait(timeout=60)
            child.events.close()
        for sig, handler in self.handlers.items():
            signal.signal(sig, handler)
        signal.pthread_sigmask(signal.SIG_SETMASK, self.mask)

    def tracked_spawn(self, during_spawn=None):
        start = GUARD.spawn_custodian

        def spawn(*args, **kwargs):
            child = start(*args, **kwargs)
            self.children.append(child)
            if during_spawn:
                during_spawn()
            return child

        return mock.patch.object(GUARD, "spawn_custodian", side_effect=spawn)

    def assert_reaped(self):
        for child in self.children:
            self.assertIsNotNone(child.returncode)
            self.assertFalse(GUARD.group_exists(child.pid))
        self.assertEqual(signal.pthread_sigmask(signal.SIG_BLOCK, set()), self.mask)
        self.assertEqual({sig: signal.getsignal(sig) for sig in self.handlers}, self.handlers)

    def test_instant_exit_keeps_exact_status_and_reaps_after_group_stop(self):
        with self.tracked_spawn():
            for code in (0, 23):
                self.assertEqual(GUARD.run([sys.executable, "-I", "-B", "-c",
                                           f"raise SystemExit({code})"]), code)
        self.assert_reaped()

    def test_exec_failure_restores_cancellation_mask_without_a_live_child(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-guard-exec-failure-"))
        print(f"preserved exec-failure fixture: {directory}", flush=True)
        with self.tracked_spawn(), self.assertRaises(FileNotFoundError):
            GUARD.run([str(directory / "missing-command")])
        self.assertTrue(self.children, "the startup error still retires its custodian")
        self.assert_reaped()

    def test_partial_channel_setup_cannot_start_a_command_or_leak_descriptors(self):
        def open_descriptors():
            directory = "/dev/fd" if os.path.isdir("/dev/fd") else "/proc/self/fd"
            result = set()
            for name in os.listdir(directory):
                if not name.isdigit():
                    continue
                try:
                    os.fstat(int(name))
                except OSError:
                    continue
                result.add(int(name))
            return result

        for operation, ordinal in [("pipe", 1), ("pipe", 2), ("fdopen", 1), ("fdopen", 2),
                                   ("fork", 1)]:
            with self.subTest(operation=operation, ordinal=ordinal):
                original = getattr(GUARD.os, operation)
                calls = []

                def refuse(*args, **kwargs):
                    calls.append(None)
                    if len(calls) == ordinal:
                        raise OSError(errno.EMFILE, "controlled descriptor refusal")
                    return original(*args, **kwargs)

                before = open_descriptors()
                fork = os.fork
                with mock.patch.object(GUARD.os, operation, side_effect=refuse):
                    if operation == "fork":
                        with self.assertRaises(OSError):
                            GUARD.run([sys.executable, "-I", "-B", "-c", "raise SystemExit(0)"])
                    else:
                        with (mock.patch.object(GUARD.os, "fork", wraps=fork) as spawn,
                              self.assertRaises(OSError)):
                            GUARD.run([sys.executable, "-I", "-B", "-c", "raise SystemExit(0)"])
                        spawn.assert_not_called()
                self.assertEqual(open_descriptors(), before)
                self.assert_reaped()

    def test_owned_cache_lock_preserves_file_and_busy_alias_without_spawning(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-guard-cache-lock-"))
        print(f"preserved cache-lock fixture: {directory}", flush=True)
        lock = directory / "cache.lock"
        contents = b"preserve this existing lock file\n"
        lock.write_bytes(contents)
        before = lock.stat()
        alias = directory.with_name(directory.name + "-alias")
        alias.symlink_to(directory, target_is_directory=True)
        with lock.open("a") as held:
            fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with mock.patch.object(GUARD, "run", side_effect=AssertionError("busy cache spawned")):
                self.assertEqual(GUARD.run_with_cache_lock(alias / lock.name, ["unused"]), 75)
        with self.tracked_spawn():
            self.assertEqual(GUARD.run_with_cache_lock(
                lock, [sys.executable, "-I", "-B", "-c", "raise SystemExit(0)"]), 0)
            with self.assertRaises(FileNotFoundError):
                GUARD.run_with_cache_lock(lock, [str(directory / "missing-command")])
        with lock.open("a") as released:
            fcntl.flock(released, fcntl.LOCK_EX | fcntl.LOCK_NB)
        after = lock.stat()
        self.assertEqual((after.st_dev, after.st_ino, after.st_mode),
                         (before.st_dev, before.st_ino, before.st_mode))
        self.assertEqual(lock.read_bytes(), contents)
        self.assert_reaped()

    def test_normal_controller_exit_stops_a_started_writer_with_redirected_stdio(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-guard-normal-exit-"))
        print(f"preserved normal-exit fixture: {directory}", flush=True)
        ready = directory / "writer-ready"
        writer = ("import os, signal; from pathlib import Path; "
                  f"Path({str(ready)!r}).write_text(str(os.getpid())); signal.pause()")
        controller = f'''
import subprocess, sys, time
from pathlib import Path
subprocess.Popen([sys.executable, "-I", "-B", "-c", {writer!r}],
                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
deadline = time.monotonic() + 60
while not Path({str(ready)!r}).exists():
    if time.monotonic() >= deadline:
        raise RuntimeError("writer startup exceeded the one-minute hang detector")
    time.sleep(0.01)
'''
        with self.tracked_spawn():
            self.assertEqual(GUARD.run([sys.executable, "-I", "-B", "-c", controller]), 0)
        self.assertTrue(ready.is_file(), "the writer actually started before controller exit")
        self.assert_reaped()

    def test_controller_sigkill_before_observer_setup_is_not_missed(self):
        observe = GUARD.observe_exit
        trigger, command = self.self_killing_command()

        def killed_before_setup(pid):
            trigger.write_text("kill only the command, preserving its custodian")
            observe(pid)

        with self.tracked_spawn(), mock.patch.object(GUARD, "observe_exit", killed_before_setup):
            self.assertEqual(GUARD.run([sys.executable, "-I", "-B", "-c", command]), 137)
        self.assert_reaped()

    def self_killing_command(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-guard-command-kill-"))
        trigger = directory / "kill-command"
        command = f'''
import os, signal, time
from pathlib import Path
signal.alarm(60)
while not Path({str(trigger)!r}).exists(): time.sleep(0.01)
os.kill(os.getpid(), signal.SIGKILL)
'''
        return trigger, command

    @unittest.skipUnless(hasattr(select, "kqueue"), "requires the macOS kqueue observer")
    def test_controller_sigkill_after_real_kqueue_registration_is_not_missed(self):
        kqueue = select.kqueue
        trigger, command = self.self_killing_command()

        class RegisteredQueue:
            def __init__(self):
                self.queue = kqueue()

            def close(self):
                self.queue.close()

            def control(self, changes, count, timeout):
                self.queue.control(changes, 0, 0)
                trigger.write_text("kill only the command after observer registration")
                return self.queue.control(None, count, timeout)

        with (self.tracked_spawn(), mock.patch.object(GUARD, "observe_exit", GUARD.observe_exit_kqueue),
              mock.patch.object(GUARD.select, "kqueue", RegisteredQueue)):
            self.assertEqual(GUARD.run([sys.executable, "-I", "-B", "-c", command]), 137)
        self.assert_reaped()

    def test_sigterm_during_spawn_is_delivered_only_inside_the_cleanup_guard(self):
        def interrupted_spawn():
            os.kill(os.getpid(), signal.SIGTERM)

        with self.tracked_spawn(interrupted_spawn), self.assertRaises(SystemExit) as stopped:
            GUARD.run([sys.executable, "-I", "-B", "-c", "import signal; signal.pause()"])
        self.assertEqual(stopped.exception.code, 143)
        self.assert_reaped()

    def test_inherited_ignored_sigchld_preserves_child_lease_and_caller_disposition(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-guard-ignored-sigchld-"))
        print(f"preserved ignored-SIGCHLD fixture: {directory}", flush=True)
        observed = directory / "child-disposition"
        controller = ("import signal; from pathlib import Path; "
                      f"Path({str(observed)!r}).write_text(str(int(signal.getsignal(signal.SIGCHLD))))")
        prior = signal.signal(signal.SIGCHLD, signal.SIG_IGN)
        try:
            # Both owners must retain their direct-child lease despite the caller's
            # ignored SIGCHLD; the real command still receives that disposition.
            with self.tracked_spawn():
                self.assertEqual(GUARD.run([sys.executable, "-I", "-B", "-c", controller]), 0)
            self.assertEqual(signal.getsignal(signal.SIGCHLD), signal.SIG_IGN)
            self.assertEqual(observed.read_text(), str(int(signal.SIG_IGN)))
        finally:
            for child in self.children:
                child.wait(timeout=60)
            signal.signal(signal.SIGCHLD, prior)
        self.assertTrue(all(child.returncode is not None for child in self.children))
        self.assertEqual(signal.pthread_sigmask(signal.SIG_BLOCK, set()), self.mask)

    @unittest.skipUnless(hasattr(select, "kqueue"), "requires the macOS kqueue observer")
    def test_returned_kqueue_error_uses_errno_before_exit_bits_and_still_stops_group(self):
        for code in (errno.ESRCH, errno.EPERM):
            with self.subTest(errno=code):
                class ErrorQueue:
                    def close(self):
                        pass

                    def control(self, changes, _count, _timeout):
                        return [select.kevent(changes[0].ident, filter=select.KQ_FILTER_PROC,
                                              flags=select.KQ_EV_ERROR,
                                              fflags=select.KQ_NOTE_EXIT, data=code)]

                with (self.tracked_spawn(),
                      mock.patch.object(GUARD, "observe_exit", GUARD.observe_exit_kqueue),
                      mock.patch.object(GUARD.select, "kqueue", ErrorQueue)):
                    argv = [sys.executable, "-I", "-B", "-c", "import signal; signal.pause()"]
                    if code == errno.ESRCH:
                        self.assertEqual(GUARD.run(argv), 137)
                    else:
                        with self.assertRaises(OSError) as failed:
                            GUARD.run(argv)
                        self.assertEqual(failed.exception.errno, code)
                self.assert_reaped()

    def test_inherited_fetch_timeout_leaves_no_escaped_group_or_open_writer_pipe(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-guard-inherited-deadline-"))
        print(f"preserved inherited-deadline fixture: {directory}", flush=True)
        ready = directory / "writer-ready.json"
        observed = directory / "helper-result.json"
        writer = ("import json, os, signal; from pathlib import Path; "
                  f"Path({str(ready)!r}).write_text(json.dumps([os.getpid(), os.getpgrp()])); "
                  "signal.alarm(60); signal.pause()")
        git = ("import subprocess, sys, signal; "
               f"subprocess.Popen([sys.executable, '-I', '-B', '-c', {writer!r}]); "
               "signal.alarm(60); signal.pause()")
        helper = f'''
import importlib.util, json, os, subprocess, sys, time
from pathlib import Path
spec = importlib.util.spec_from_file_location("inherited_guard", {str(ROOT / 'alab/command_guard.py')!r})
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)
wait = subprocess.Popen.wait
def deadline(child, timeout=None):
    if timeout is not None:
        assert 0 < timeout <= 120
        until = time.monotonic() + 60
        while not Path({str(ready)!r}).exists():
            if time.monotonic() >= until:
                raise RuntimeError("writer startup exceeded the one-minute hang detector")
            time.sleep(0.01)
        raise subprocess.TimeoutExpired(child.args, timeout)
    return wait(child, timeout=timeout)
subprocess.Popen.wait = deadline
code = guard.run_inherited([sys.executable, "-I", "-B", "-c", {git!r}], timeout=120)
Path({str(observed)!r}).write_text(json.dumps([code, os.getpgrp()]))
raise SystemExit(code)
'''
        with self.tracked_spawn():
            code = GUARD.run_with_cache_lock(
                directory / "cache.lock", [sys.executable, "-I", "-B", "-c", helper])
        self.assertEqual(code, 124)
        group = json.loads(ready.read_text())[1]
        self.assertEqual(json.loads(observed.read_text()), [124, group])
        self.assertNotEqual(group, self.children[0].pid,
                            "the custodian survives outside its command group")
        self.assertEqual(json.loads(ready.read_text())[1], group,
                         "Git's helper stays in the cache guardian's leased group")
        self.assertFalse(GUARD.group_exists(group))
        self.assert_reaped()
        with (directory / "cache.lock").open("a") as released:
            fcntl.flock(released, fcntl.LOCK_EX | fcntl.LOCK_NB)


    def test_disk_floor_checks_each_output_volume_before_spawn(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-guard-disk-volumes-")).resolve()
        output = directory / "output"
        output.mkdir()
        observed = []

        def space(path):
            observed.append(Path(path))
            if not Path(path).exists():
                raise FileNotFoundError(path)
            return type("Space", (), {"f_bavail": (19 if Path(path) == output else 35) * 2**30,
                                      "f_frsize": 1})()

        floor = GUARD.DiskFloor([directory, output / "not-yet-created/target"], 20)
        with (mock.patch.object(GUARD.os, "statvfs", side_effect=space),
              mock.patch.object(GUARD.subprocess, "Popen", side_effect=AssertionError("low disk spawned"))):
            with self.assertRaisesRegex(RuntimeError, "19.00 GiB"):
                GUARD.run(["unused"], disk_floor=floor)
        self.assertEqual(observed, [directory, output / "not-yet-created/target",
                                    output / "not-yet-created", output])
        self.assert_reaped()

    def test_invalid_or_inaccessible_disk_measurement_cannot_launch(self):
        with self.assertRaises(ValueError):
            GUARD.DiskFloor(["relative"], 20)
        for value in (float("nan"), float("inf"), 0):
            with self.assertRaises(ValueError):
                GUARD.DiskFloor(["/"], value)
        with (mock.patch.object(GUARD.os, "statvfs", side_effect=PermissionError("no metadata")),
              mock.patch.object(GUARD.subprocess, "Popen", side_effect=AssertionError("unknown disk spawned"))):
            with self.assertRaisesRegex(GUARD.DiskSpaceError, "measurement unavailable"):
                GUARD.run(["unused"], disk_floor=GUARD.DiskFloor(["/"], 20))
        self.assert_reaped()

    def test_symlink_loop_is_a_typed_resource_stop_before_command_launch(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-guard-loop-")).resolve()
        (directory / "a").symlink_to("b")
        (directory / "b").symlink_to("a")
        with (mock.patch.object(sys, "argv", ["guard", "--resource-stop", "--min-free-gib", "20", "--disk-path",
                                              str(directory / "a"), "--", "unused"]),
              mock.patch.object(GUARD.subprocess, "Popen", side_effect=AssertionError("loop spawned"))):
            self.assertEqual(GUARD.main(), GUARD.DISK_STOP)

    def test_low_disk_stops_leader_and_descendant_before_releasing_cache(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-guard-live-disk-")).resolve()
        print(f"preserved live-disk fixture: {directory}", flush=True)
        lock = directory / "cache.lock"
        identity, ready = directory / "writer.json", directory / "ready"
        writer = f'''
import json, os, signal
from pathlib import Path
signal.signal(signal.SIGINT, signal.SIG_IGN)
signal.signal(signal.SIGTERM, signal.SIG_IGN)
signal.alarm(60)
Path({str(identity)!r}).write_text(json.dumps([os.getpid(), os.getpgrp()]))
Path({str(ready)!r}).write_text("handlers and identity ready")
signal.pause()
'''
        controller = ("import subprocess,sys,signal; signal.alarm(60); "
                      f"subprocess.Popen([sys.executable,'-I','-B','-c',{writer!r}]); signal.pause()")
        samples = []

        def space(_path):
            if samples:
                import time
                deadline = time.monotonic() + 60
                while not ready.exists():
                    if time.monotonic() >= deadline:
                        raise RuntimeError("fake writer did not reach readiness")
                    time.sleep(0.01)
                with lock.open("rb") as probe, self.assertRaises(BlockingIOError):
                    fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)
            samples.append(19 if samples else 35)
            return type("Space", (), {"f_bavail": samples[-1] * 2**30, "f_frsize": 1})()

        with self.tracked_spawn(), mock.patch.object(GUARD.os, "statvfs", side_effect=space):
            with self.assertRaisesRegex(RuntimeError, "running floor"):
                GUARD.run_with_cache_lock(lock, [sys.executable, "-I", "-B", "-c", controller],
                                          disk_floor=GUARD.DiskFloor([directory], 20))
        self.assertEqual(samples, [35, 19])
        self.assert_reaped()
        self.assertFalse(GUARD.group_exists(json.loads(identity.read_text())[1]))
        with self.assertRaises(ProcessLookupError):
            os.kill(json.loads(identity.read_text())[0], 0)
        with lock.open("rb") as probe:
            fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def test_disk_error_after_spawn_still_reaps_before_returning(self):
        space = type("Space", (), {"f_bavail": 35 * 2**30, "f_frsize": 1})()
        with self.tracked_spawn(), mock.patch.object(
                GUARD.os, "statvfs", side_effect=[space, PermissionError("metadata lost")]):
            with self.assertRaisesRegex(GUARD.DiskSpaceError, "metadata lost"):
                GUARD.run([sys.executable, "-I", "-B", "-c",
                           "import signal; signal.alarm(20); signal.pause()"],
                          disk_floor=GUARD.DiskFloor(["/"], 20))
        self.assert_reaped()

    def test_sufficient_disk_uses_the_real_exit_observer_and_exact_child_status(self):
        space = type("Space", (), {"f_bavail": 35 * 2**30, "f_frsize": 1})()
        with self.tracked_spawn(), mock.patch.object(GUARD.os, "statvfs", return_value=space) as measured:
            self.assertEqual(GUARD.run(
                [sys.executable, "-I", "-B", "-c", "import time; time.sleep(0.02); raise SystemExit(23)"],
                disk_floor=GUARD.DiskFloor(["/"], 20)), 23)
        self.assertGreaterEqual(measured.call_count, 2, "prelaunch and real observer both check")
        self.assert_reaped()

    def test_disk_cli_refusal_is_nonzero_and_plain_clients_keep_their_defaults(self):
        space = type("Space", (), {"f_bavail": 19 * 2**30, "f_frsize": 1})()
        with (mock.patch.object(sys, "argv", ["guard", "--min-free-gib", "20",
                                              "--disk-path", "/", "--", "unused"]),
              mock.patch.object(GUARD.os, "statvfs", return_value=space),
              mock.patch.object(GUARD.subprocess, "Popen", side_effect=AssertionError("low disk spawned"))):
            self.assertEqual(GUARD.main(), 1)
        with (mock.patch.object(sys, "argv", ["guard", "--", "untouched"]),
              mock.patch.object(GUARD, "run", return_value=23) as ordinary):
            self.assertEqual(GUARD.main(), 23)
        ordinary.assert_called_once_with(["untouched"])

    def test_existing_cache_busy_status_stays_distinct_from_disk_failure(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-guard-status-")).resolve()
        lock = directory / 'cache.lock'
        space = type('Space', (), {'f_bavail': 19 * 2**30, 'f_frsize': 1})()
        args = ['guard', '--cache-lock', str(lock), '--min-free-gib', '20',
                '--disk-path', str(directory), '--', 'unused']
        with (mock.patch.object(GUARD.os, 'statvfs', return_value=space),
              mock.patch.object(GUARD.subprocess, 'Popen', side_effect=AssertionError('low disk spawned'))):
            with lock.open('a') as held:
                fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
                with mock.patch.object(sys, 'argv', args):
                    self.assertEqual(GUARD.main(), 75, 'only cache contention keeps the old busy status')
            with mock.patch.object(sys, 'argv', args):
                self.assertEqual(GUARD.main(), 1, 'package disk failure is not cache contention')
            source = ['guard', '--resource-stop', '--min-free-gib', '20',
                      '--disk-path', str(directory), '--', 'unused']
            with mock.patch.object(sys, 'argv', source):
                self.assertEqual(GUARD.main(), 75, 'source builds explicitly select typed resource stops')

    def test_compiler_opt_in_preserves_inheritable_fds_without_changing_cache_default(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-guard-inherited-fd-")).resolve()
        print(f"preserved inherited-fd fixture: {directory}", flush=True)
        descriptor = os.open(directory / "input", os.O_CREAT | os.O_RDWR, 0o600)
        try:
            os.write(descriptor, b"caller descriptor")
            os.set_inheritable(descriptor, True)
            for inherited in (False, True):
                output = directory / str(inherited)
                code = ("import os; from pathlib import Path\n"
                        f"try: value = os.pread({descriptor}, 32, 0).decode()\n"
                        "except OSError: value = 'closed'\n"
                        f"Path({str(output)!r}).write_text(value)\n")
                with self.tracked_spawn():
                    self.assertEqual(GUARD.run([sys.executable, "-I", "-B", "-c", code],
                                               inherit_fds=inherited), 0)
                self.assertEqual(output.read_text(), "caller descriptor" if inherited else "closed")
        finally:
            os.close(descriptor)
        self.assert_reaped()

    @unittest.skipUnless(hasattr(select, "kqueue"), "requires macOS kqueue")
    def test_disk_observer_prefers_kqueue_and_keeps_the_original_exit_registration(self):
        changes, checks, timeouts = [], [], []

        class PollQueue:
            def close(self):
                pass

            def control(self, submitted, count, timeout):
                changes.append(submitted)
                timeouts.append(timeout)
                if len(changes) == 1:
                    return []
                return [select.kevent(123, filter=select.KQ_FILTER_PROC,
                                      fflags=select.KQ_NOTE_EXIT)]

        floor = mock.Mock()
        floor.check.side_effect = lambda: checks.append(True)
        with (mock.patch.object(GUARD.select, "kqueue", PollQueue),
              mock.patch.object(GUARD.os, "waitid", create=True,
                                side_effect=AssertionError("kqueue must observe the disk-guarded exit")),
              mock.patch.object(GUARD.time, "sleep",
                                side_effect=AssertionError("kqueue must wait on the exit notification"))):
            GUARD.observe_exit_with_disk(123, floor, 0.25)
        self.assertEqual(len(checks), 2)
        self.assertEqual(len(changes[0]), 1)
        self.assertEqual(changes[1], [])
        self.assertEqual(timeouts, [0.25, 0.25])

    @unittest.skipUnless(hasattr(select, "kqueue"), "requires macOS kqueue")
    def test_disk_kqueue_error_is_not_retried_with_waitid(self):
        space = type("Space", (), {"f_bavail": 35 * 2**30, "f_frsize": 1})()
        queue = mock.Mock()
        queue.control.side_effect = OSError(errno.EPERM, "controlled observer refusal")
        with (self.tracked_spawn(),
              mock.patch.object(GUARD.os, "statvfs", return_value=space) as measured,
              mock.patch.object(GUARD.select, "kqueue", return_value=queue),
              mock.patch.object(GUARD.os, "waitid", create=True) as waitid,
              self.assertRaises(OSError) as failed):
            GUARD.run([sys.executable, "-I", "-B", "-c",
                       "import signal; signal.alarm(20); signal.pause()"],
                      disk_floor=GUARD.DiskFloor(["/"], 20))
        self.assertEqual(failed.exception.errno, errno.EPERM)
        self.assertEqual(measured.call_count, 2, "prelaunch and observer both check disk")
        waitid.assert_not_called()
        queue.close.assert_called_once_with()
        self.assert_reaped()

    @unittest.skipUnless(hasattr(os, "waitid"), "requires waitid")
    def test_disk_waitid_fallback_checks_between_nonreaping_polls(self):
        calls = mock.Mock()
        floor = mock.Mock()
        calls.attach_mock(floor.check, "check")
        with (mock.patch.object(GUARD, "select", object()),
              mock.patch.object(GUARD.os, "waitid", create=True,
                                side_effect=[None, object()]) as waitid,
              mock.patch.object(GUARD.os, "waitpid") as waitpid,
              mock.patch.object(GUARD.time, "sleep") as sleep):
            calls.attach_mock(waitid, "waitid")
            calls.attach_mock(sleep, "sleep")
            GUARD.observe_exit_with_disk(123, floor, 0.25)
            waitpid.assert_not_called()
        poll = mock.call.waitid(os.P_PID, 123, os.WEXITED | os.WNOWAIT | os.WNOHANG)
        self.assertEqual(calls.mock_calls, [mock.call.check(), poll, mock.call.sleep(0.25),
                                           mock.call.check(), poll])

    @unittest.skipUnless(hasattr(os, "waitid"), "requires waitid")
    def test_disk_waitid_fallback_propagates_resource_and_observer_errors(self):
        for resource_failure in (True, False):
            with self.subTest(resource_failure=resource_failure):
                floor = mock.Mock()
                error = (GUARD.DiskSpaceError("controlled disk refusal") if resource_failure
                         else OSError(errno.EPERM, "controlled observer refusal"))
                if resource_failure:
                    floor.check.side_effect = error
                with (mock.patch.object(GUARD, "select", object()),
                      mock.patch.object(GUARD.os, "waitid", create=True, side_effect=error) as waitid,
                      mock.patch.object(GUARD.os, "waitpid") as waitpid,
                      mock.patch.object(GUARD.time, "sleep") as sleep,
                      self.assertRaises(type(error)) as failed):
                    GUARD.observe_exit_with_disk(123, floor, 0.25)
                self.assertIs(failed.exception, error)
                floor.check.assert_called_once_with()
                self.assertEqual(waitid.call_count, 0 if resource_failure else 1)
                waitpid.assert_not_called()
                sleep.assert_not_called()

    @unittest.skipUnless(hasattr(os, "waitid"), "requires waitid")
    def test_unrestricted_observer_keeps_the_blocking_nonreaping_waitid(self):
        with (mock.patch.object(GUARD.select, "kqueue", create=True) as kqueue,
              mock.patch.object(GUARD.os, "waitid", create=True) as waitid,
              mock.patch.object(GUARD.os, "waitpid") as waitpid):
            GUARD.observe_exit(123)
        waitid.assert_called_once_with(os.P_PID, 123, os.WEXITED | os.WNOWAIT)
        waitpid.assert_not_called()
        kqueue.assert_not_called()


if __name__ == "__main__":
    unittest.main()
