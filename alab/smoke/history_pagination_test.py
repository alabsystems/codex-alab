# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Actual PTY custodian controls; these do not stand in for the Codex smoke."""
import base64
import errno
import http.client
import json
import os
from pathlib import Path
import runpy
import pty
import select
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch


PROBE = Path(__file__).with_name("history_pagination.py")


class ModelFixtureTests(unittest.TestCase):
    def test_actual_history_model_keeps_its_three_argument_http_callback(self):
        namespace = runpy.run_path(str(PROBE))
        model = namespace["Model"]()
        server, worker = namespace["weekly"].serve(model)
        connection = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=2)
        body = {"model": "gpt-5", "input": []}
        try:
            connection.request("POST", "/v1/responses", body=json.dumps(body))
            with connection.getresponse() as reply:
                self.assertEqual(reply.status, 200)
                self.assertEqual(reply.getheader("content-type"), "text/event-stream")
                self.assertIn(b"HISTORY-ANSWER-001", reply.read())
            self.assertEqual(model.models, [body])
            self.assertEqual(len(model.requests), 1)
        finally:
            connection.close()
            server.shutdown()
            worker.join()
            server.server_close()


def transition_actor(spec, boundary):
    """Inject real signals around actual syscall returns, without product hooks."""
    module = runpy.run_path(str(PROBE))
    guardian = module["guardian"]
    namespace = guardian.__globals__
    original_fork, original_wait = os.forkpty, os.waitpid
    original_killpg = os.killpg
    reaped = False
    witness = Path(spec).with_suffix(".witness")

    def fork_return():
        returned = original_fork()
        if returned[0] and boundary == "birth":
            witness.write_text("actual forkpty returned before publication")
            os.kill(os.getpid(), signal.SIGTERM)
        return returned

    def owning_wait(*args):
        nonlocal reaped
        returned = original_wait(*args)
        reaped = True
        if boundary == "reap":
            witness.write_text("actual waitpid returned before publication")
            os.kill(os.getpid(), signal.SIGTERM)
        return returned

    def signal_original(group, signum):
        if signum and reaped:
            raise AssertionError("attempted group signal after actual owning reap")
        return original_killpg(group, signum)

    with patch.object(os, "forkpty", fork_return), patch.object(os, "waitpid", owning_wait), \
            patch.object(os, "killpg", signal_original):
        return guardian(spec)


class CustodianTests(unittest.TestCase):
    def test_real_pty_child_exits_and_is_reaped_before_success(self):
        self.exercise(eof=False)

    def test_parent_lifeline_eof_cancels_and_reaps_the_actual_pty_child(self):
        self.exercise(eof=True)

    def test_actual_fork_return_signal_cannot_drop_the_unpublished_child(self):
        self.transition("birth")

    def test_actual_wait_return_signal_cannot_signal_an_already_reaped_identity(self):
        self.transition("reap")

    def test_real_queued_output_remains_bounded_after_owning_reap(self):
        self.drain(bound=True)

    def test_actual_elapsed_endpoint_remains_original_after_owning_reap(self):
        self.drain(bound=False)

    def test_actual_terminal_eof_cannot_skip_the_final_original_endpoint(self):
        self.eof_bookend(cancel=False)

    def test_actual_terminal_eof_cannot_skip_a_native_cancellation(self):
        self.eof_bookend(cancel=True)

    def eof_bookend(self, *, cancel):
        namespace = runpy.run_path(str(PROBE))['drain_pty'].__globals__
        master, slave = pty.openpty()
        os.close(slave)
        lifeline, writer = os.pipe()
        observed = {'eof': False, 'signal': None}
        original_read = os.read
        previous = signal.signal(signal.SIGTERM, lambda number, _frame: observed.update(signal=number))

        def read_then_boundary(descriptor, count):
            try:
                value = original_read(descriptor, count)
            except OSError as error:
                if error.errno != errno.EIO:  # Native PTY EIO is an actual terminal EOF.
                    raise
                value = None
                terminal_error = error
            else:
                terminal_error = None
            if descriptor == master and not value:
                observed['eof'] = True
                if cancel:
                    os.kill(os.getpid(), signal.SIGTERM)
                else:
                    time.sleep(.03)
            if terminal_error is not None:
                raise terminal_error
            return value

        try:
            with tempfile.TemporaryFile() as raw, patch.object(os, 'read', read_then_boundary):
                with self.assertRaisesRegex(RuntimeError, 'cancelled' if cancel else 'original guardian deadline'):
                    namespace['drain_pty'](master, raw, time.monotonic() + (2 if cancel else .02),
                                           lambda: observed['signal'], lifeline)
            self.assertIs(observed['eof'], True)
            if cancel:
                self.assertEqual(observed['signal'], signal.SIGTERM)
        finally:
            signal.signal(signal.SIGTERM, previous)
            for descriptor in (master, lifeline, writer):
                os.close(descriptor)

    def drain(self, *, bound):
        namespace = runpy.run_path(str(PROBE))['drain_pty'].__globals__
        drain = namespace['drain_pty']
        master, slave = pty.openpty()
        lifeline, writer = os.pipe()
        try:
            with tempfile.TemporaryFile() as raw:
                if bound:
                    self.assertEqual(os.write(slave, b'x' * 256), 256)
                    with patch.dict(namespace, OUTPUT_LIMIT_BYTES=64):
                        with self.assertRaisesRegex(RuntimeError, 'output byte bound'):
                            drain(master, raw, time.monotonic() + 2, lambda: None, lifeline)
                    self.assertEqual(raw.tell(), 0, 'refused bytes reached the retained output')
                else:
                    # A real retained slave keeps the master quiet but non-EOF.
                    # The original endpoint must expire, never report a drain.
                    deadline = time.monotonic() + .02
                    with self.assertRaisesRegex(RuntimeError, 'original guardian deadline'):
                        drain(master, raw, deadline, lambda: None, lifeline)
                    self.assertGreaterEqual(time.monotonic(), deadline)
        finally:
            for descriptor in (master, slave, lifeline, writer):
                os.close(descriptor)

    def transition(self, boundary):
        with tempfile.TemporaryDirectory(prefix="alab-pty-transition-") as directory:
            root = Path(directory)
            spec = root / "spec.json"
            code = "import signal; signal.pause()" if boundary == "birth" else "import os; os.write(1,b'x'*256)"
            spec.write_text(json.dumps({"argv": [sys.executable, "-I", "-B", "-S", "-c", code],
                                       "cwd": str(root), "env": dict(os.environ),
                                       "raw": str(root / "raw"), "result": str(root / "result.json")}))
            child = subprocess.Popen([sys.executable, "-I", "-B", "-S", __file__,
                                      "--transition-actor", str(spec), boundary],
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, bufsize=0)
            try:
                while True:
                    self.assertTrue(select.select([child.stdout], [], [], 10)[0], "actual completion absent")
                    line = child.stdout.readline()
                    if not line:
                        _, errors = child.communicate(timeout=10)
                        self.fail(f"transition actor closed without completion: {errors!r}")
                    if "completed" in json.loads(line):
                        break
                stdout, stderr = child.communicate(timeout=10)
                self.assertEqual(child.returncode, 1, (stdout, stderr))
                result = json.loads((root / "result.json").read_text())
                self.assertIs(result["owning_reap"], True)
                self.assertIs(result["group_absent_after_reap"], True)
                self.assertIs(result["forced"], True)
                self.assertEqual(result["cancellation_signal"], signal.SIGTERM)
                self.assertIn("actual", spec.with_suffix(".witness").read_text())
            finally:
                if child.poll() is None:
                    child.stdin.close()
                    child.stdin = None
                child.communicate(timeout=10)

    def exercise(self, *, eof):
        with tempfile.TemporaryDirectory(prefix="alab-pty-owner-") as directory:
            root = Path(directory)
            code = ("import signal,sys; from pathlib import Path; Path(sys.argv[1]).write_text('ENTERED'); signal.pause()" if eof else
                    "import os; print('ACTUAL-PTY-CHILD',flush=True); os.read(0,1)")
            spec = root / "spec.json"
            result = root / "result.json"
            spec.write_text(json.dumps({"argv": [sys.executable, "-I", "-B", "-S", "-c", code, str(root / "entered")],
                                       "cwd": str(root), "env": dict(os.environ),
                                       "raw": str(root / "raw"), "result": str(result)}))
            child = subprocess.Popen([sys.executable, "-I", "-B", "-S", str(PROBE),
                                      "--guardian", str(spec)], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0)
            try:
                self.assertTrue(select.select([child.stdout], [], [], 10)[0], "birth not observed")
                started = json.loads(child.stdout.readline())
                self.assertIn("started", started)
                if eof:
                    deadline = time.monotonic() + 10
                    while not (root / "entered").exists():
                        self.assertLess(time.monotonic(), deadline, "actual exec body never entered")
                        time.sleep(.01)
                    self.assertEqual((root / "entered").read_text(), "ENTERED")
                    child.stdin.close()
                    child.stdin = None
                else:
                    child.stdin.write(json.dumps({"id": 1, "write": base64.b64encode(b"x\n").decode()}).encode() + b"\n")
                    child.stdin.flush()
                    while True:
                        self.assertTrue(select.select([child.stdout], [], [], 10)[0], "owning completion absent")
                        line = child.stdout.readline()
                        if not line:
                            _, errors = child.communicate(timeout=10)
                            self.fail(f"custodian closed before completion: {errors!r}; {result.read_text() if result.exists() else 'no result'}")
                        message = json.loads(line)
                        if "completed" in message:
                            break
                stdout, stderr = child.communicate(timeout=10)
                self.assertEqual(child.returncode, 1 if eof else 0, (stdout, stderr))
                observed = json.loads(result.read_text())
                self.assertIs(observed["owning_reap"], True)
                self.assertIs(observed["forced"], eof)
                self.assertIs(observed["group_absent_after_reap"], True)
                if not eof:
                    self.assertEqual(observed["exit"], 0)
                    self.assertIs(observed["group_absent_after_reap"], True)
                    self.assertIn(b"ACTUAL-PTY-CHILD", (root / "raw").read_bytes())
                # This is an observation after the guardian's real owning wait,
                # never signaling authority derived from its reported PID.
                with self.assertRaises(ProcessLookupError):
                    os.kill(started["started"], 0)
            finally:
                if child.poll() is None:
                    if child.stdin is not None:
                        child.stdin.close()
                        child.stdin = None
                    child.communicate(timeout=10)
                else:
                    child.communicate(timeout=10)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--transition-actor":
        raise SystemExit(transition_actor(sys.argv[2], sys.argv[3]))
    unittest.main()
