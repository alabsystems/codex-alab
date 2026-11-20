#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the fixed owner used by a publisher-selected native compile worker."""

import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import unittest


ALAB = Path(__file__).resolve().parents[1]
OWNER = (ALAB / "native_worker.py").read_text()
GUARD = (ALAB / "command_guard.py").read_text()


class NativeWorker(unittest.TestCase):
    def test_closed_lifeline_never_starts_the_payload(self):
        reader, writer = socket.socketpair()
        with reader, writer, tempfile.TemporaryDirectory() as folder:
            writer.shutdown(socket.SHUT_WR)
            marker = Path(folder) / "started"
            result = subprocess.run([sys.executable, "-I", "-B", "-S", "-c", OWNER,
                                     GUARD, "", str(reader.fileno()), "disposable", "/usr/bin/touch", str(marker)],
                                    pass_fds=(reader.fileno(),), timeout=60)
            self.assertEqual(result.returncode, 128 + signal.SIGTERM)
            self.assertFalse(marker.exists())

    def test_owned_front_retires_its_child_before_return(self):
        reader, writer = socket.socketpair()
        job, caller = socket.socketpair()
        fixture = """
import json, os, signal, socket, subprocess, sys
fd, expected = int(sys.argv[1]), int(sys.argv[2])
try:
    same_control = os.fstat(fd).st_ino == expected
except OSError:
    same_control = False
assert not same_control, "private cancellation socket reached the payload"
job = socket.socket(fileno=int(sys.argv[3]))
# An admitted front deliberately owns its compiler's independent group.
child = subprocess.Popen(['/bin/cat'], stdin=subprocess.PIPE, start_new_session=True)
def stop(signum, frame):
    child.stdin.close()
    child.wait()
    job.sendall(b'drained')
    raise SystemExit(75)
signal.signal(signal.SIGTERM, stop)
job.sendall((json.dumps({'front':os.getpid(), 'compiler':child.pid})+'\\n').encode())
while True:
    signal.pause()
"""
        with reader, writer, job, caller:
            command = [sys.executable, "-I", "-B", "-S", "-c", OWNER,
                       GUARD, "", str(reader.fileno()), "compiler", sys.executable, "-I", "-c", fixture,
                       str(reader.fileno()), str(os.fstat(reader.fileno()).st_ino), str(job.fileno())]
            def background_signals():
                signal.signal(signal.SIGTERM, signal.SIG_IGN)
                signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM})
            with subprocess.Popen(command, pass_fds=(reader.fileno(), job.fileno()),
                                  preexec_fn=background_signals) as owner:
                try:
                    caller.settimeout(60)
                    with caller.makefile("rb", buffering=0) as ready:
                        row = json.loads(ready.readline())
                    self.assertNotEqual(os.getpgid(row["front"]), os.getpgrp())
                    self.assertNotEqual(os.getpgid(row["front"]), os.getpgid(row["compiler"]))
                finally:
                    writer.shutdown(socket.SHUT_WR)
                    status = owner.wait(timeout=60)
                self.assertEqual(status, 75)
                self.assertEqual(caller.recv(64), b"drained")
                for pid in row.values():
                    with self.assertRaises(ProcessLookupError):
                        os.kill(pid, 0)

    def test_disposable_term_ignoring_tree_retires_and_independent_session_survives(self):
        reader, writer = socket.socketpair()
        reports, child_reports = socket.socketpair()
        with reader, writer, reports, child_reports:
            # This peer stands for a shared service with its own session. Its
            # explicit control lease belongs to the test, independent of the job.
            with subprocess.Popen(["/bin/cat"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  start_new_session=True) as daemon:
                try:
                    body = """
import json,os,signal,socket,subprocess,sys
from time import sleep
signal.signal(signal.SIGTERM, signal.SIG_IGN)
child = subprocess.Popen(["/bin/sleep", "60"])
socket.socket(fileno=int(sys.argv[1])).sendall((json.dumps([os.getpid(),child.pid])+"\\n").encode())
sleep(60)
"""
                    args = [sys.executable, "-I", "-B", "-S", "-c", OWNER,
                            GUARD, "", str(reader.fileno()), "disposable", sys.executable, "-I", "-c",
                            body, str(child_reports.fileno())]
                    with subprocess.Popen(args, pass_fds=(reader.fileno(), child_reports.fileno())) as owner:
                        try:
                            reports.settimeout(60)
                            with reports.makefile("rb", buffering=0) as ready:
                                pids = json.loads(ready.readline())
                            self.assertNotEqual(os.getpgid(pids[0]), os.getpgid(daemon.pid))
                        finally:
                            writer.shutdown(socket.SHUT_WR)
                            status = owner.wait(timeout=60)
                        self.assertEqual(status, 128 + signal.SIGKILL)
                    for pid in pids:
                        with self.assertRaises(ProcessLookupError):
                            os.kill(pid, 0)
                    self.assertIsNone(daemon.poll())
                    daemon.stdin.write(b"still held\n")
                    daemon.stdin.flush()
                    self.assertEqual(daemon.stdout.readline(), b"still held\n")
                finally:
                    daemon.stdin.close()
                    daemon.wait(timeout=60)


if __name__ == "__main__":
    unittest.main()
