#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Actual command retirement after failures in parent-side post-fork cleanup."""
import errno
import fcntl
import gc
import importlib.util
import json
import os
from pathlib import Path
import signal
import sys
import tempfile
import time
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("postfork_guard", ROOT / "alab/command_guard.py")
guard = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(guard)


def postfork_controls(root):
    real_pipe, real_fork, real_close, real_fdopen = os.pipe, os.fork, os.close, os.fdopen
    real_observe, real_custodian = guard.observe_lifeline, guard.Custodian
    real_outer_observe = guard.observe_exit
    outer = os.getpid()
    signal.alarm(60)
    results = []
    for kind, startup in [("lifeline-close", "running"), ("sender-close", "running"),
                          ("lifeline-close", "exec-error"), ("sender-close", "exec-error"),
                          ("writer-close-before", "running"), ("writer-close-after", "running")]:
        directory = Path(tempfile.mkdtemp(prefix="postfork-fixed-" + kind + "-" + startup + "-", suffix=".noindex", dir=root))
        observing, ready = directory / "observing", directory / "ready"
        lock = directory / "cache.lock"
        lock.write_text("preserved private lock\n")
        pipes, children, wrappers, streams = [], [], [], []
        refused = False
        def wait_ready():
            if startup != "running": return
            deadline = time.monotonic() + 60
            while not observing.exists() or not ready.exists():
                if time.monotonic() >= deadline: raise RuntimeError("actual startup hang detector")
                time.sleep(0.01)
        def refuse():
            nonlocal refused
            if not refused:
                wait_ready()
                refused = True
                raise OSError(errno.EINTR, "private post-fork cleanup refusal")
        def pipe():
            result = real_pipe()
            if os.getpid() == outer: pipes.append(result)
            return result
        def fork():
            pid = real_fork()
            if pid: children.append(pid)
            return pid
        def close(fd):
            if os.getpid() == outer and kind == "lifeline-close" and pipes and fd == pipes[0][0]: refuse()
            if (os.getpid() == outer and kind.startswith("writer-close-") and pipes
                    and fd == pipes[0][1] and not refused):
                wait_ready()
                if kind == "writer-close-after":
                    real_close(fd)
                refuse()
            return real_close(fd)
        def custodian(*args, **kwargs):
            result = real_custodian(*args, **kwargs)
            if os.getpid() == outer: wrappers.append(result)
            return result
        def observe(pid, lifeline):
            observing.write_text(str(pid))
            return real_observe(pid, lifeline)
        def outer_observe(pid):
            if kind.startswith("writer-close-"):
                wait_ready()
                raise OSError(errno.EIO, "controlled outer observation failure")
            return real_outer_observe(pid)
        class Sender:
            def __init__(self, stream): self.stream = stream
            def close(self):
                if os.getpid() == outer and kind == "sender-close": refuse()
                return self.stream.close()
            def __enter__(self): return self
            def __exit__(self, *_args): self.close()
            def __getattr__(self, name): return getattr(self.stream, name)
        def fdopen(fd, mode, *args, **kwargs):
            stream = real_fdopen(fd, mode, *args, **kwargs)
            if mode == "wb": stream = Sender(stream)
            if os.getpid() == outer: streams.append(stream)
            return stream
        command = ([str(directory / ("x" * 12000))] if startup == "exec-error" else
                   [sys.executable, "-I", "-B", "-c",
                    f"import os, signal; from pathlib import Path; Path({str(ready)!r}).write_text(str(os.getpid())); signal.pause()"])
        before_mask = signal.pthread_sigmask(signal.SIG_BLOCK, set())
        try:
            with mock.patch.object(guard.os, "pipe", side_effect=pipe), \
                 mock.patch.object(guard.os, "fork", side_effect=fork), \
                 mock.patch.object(guard.os, "close", side_effect=close), \
                 mock.patch.object(guard.os, "fdopen", side_effect=fdopen), \
                 mock.patch.object(guard, "Custodian", side_effect=custodian), \
                 mock.patch.object(guard, "observe_lifeline", side_effect=observe), \
                 mock.patch.object(guard, "observe_exit", side_effect=outer_observe):
                try: guard.run_with_cache_lock(lock, command)
                except OSError as error: error_number = error.errno
                else: raise AssertionError("fault was not reported")
            expected = errno.EIO if kind.startswith("writer-close-") else errno.EINTR
            assert refused and error_number == expected
            reaped = False
            try:
                observed = os.waitpid(children[0], os.WNOHANG)
            except ChildProcessError:
                reaped = True
            assert reaped, "run raised before reaping its exact custodian"
            with lock.open("rb") as released: fcntl.flock(released, fcntl.LOCK_EX | fcntl.LOCK_NB)
            assert signal.pthread_sigmask(signal.SIG_BLOCK, set()) == before_mask
            leaked = []
            for pair in pipes:
                for fd in pair:
                    try: os.fstat(fd)
                    except OSError: continue
                    leaked.append(fd)
            assert not leaked, leaked
            results.append({"fault": kind, "startup": startup, "errno": error_number,
                            "custodian_reaped": reaped, "cache_free": True, "leaked": leaked})
        finally:
            for item in wrappers: item.close_lifeline()
            for pid in children:
                try:
                    guard.observe_exit(pid)
                    os.waitpid(pid, 0)
                except ChildProcessError: pass
            for stream in streams: stream.close()
            gc.collect()
            for pair in pipes:
                for fd in pair:
                    try: os.fstat(fd)
                    except OSError: continue
                    real_close(fd)
    signal.alarm(0)
    return results


class PostForkCleanupTests(unittest.TestCase):
    def test_parent_close_failures_retire_running_and_failed_startup_commands(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-postfork-", suffix=".noindex"))
        print(f"preserved post-fork fixture: {directory}", flush=True)
        try:
            self.assertEqual(len(postfork_controls(directory)), 6)
        finally:
            signal.alarm(0)


if __name__ == "__main__":
    unittest.main()
