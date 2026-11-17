# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Actual PTY custodian controls; these do not stand in for the Codex smoke."""
import base64
import contextlib
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
from types import SimpleNamespace
from unittest.mock import patch


PROBE = Path(__file__).with_name("history_pagination.py")


def finish_actor(child, *, timeout=8):
    """Close the lifeline, then bound/reap this fixture's own Popen actor."""
    try:
        if child.stdin is not None:
            child.stdin.close()
            child.stdin = None
        try:
            child.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            # Fixture PTY children self-arm an alarm before they can park.
            # This retained Popen handle owns only the still-unreaped actor.
            child.kill()
            child.communicate(timeout=3)
    finally:
        for stream in (child.stdin, child.stdout, child.stderr):
            if stream is not None:
                stream.close()


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
    original_killpg, original_kill = os.killpg, os.kill
    reaped = False
    actual_child = None
    birth_gate = None
    late_ready = None
    witness = Path(spec).with_suffix(".witness")

    def fork_return():
        nonlocal actual_child, birth_gate, late_ready
        if boundary in ("birth-before-group", "birth-late-group"):
            # Real fork/PTY, with the child held before setsid. This makes the
            # native forkpty parent-return/session-creation race deterministic.
            master, slave = pty.openpty()
            reader, writer = os.pipe()
            ready_reader, ready_writer = os.pipe() if boundary == "birth-late-group" else (None, None)
            child = os.fork()
            if child == 0:
                os.close(master)
                os.close(writer)
                if ready_reader is not None:
                    os.close(ready_reader)
                signal.signal(signal.SIGALRM, signal.SIG_DFL)
                signal.alarm(3)
                os.read(reader, 1)
                os.close(reader)
                os.setsid()
                if ready_writer is not None:
                    os.write(ready_writer, b"ready")
                    os.close(ready_writer)
                os.close(slave)
                return 0, master
            os.close(slave)
            os.close(reader)
            if ready_writer is not None:
                os.close(ready_writer)
                late_ready = ready_reader
            birth_gate = writer
            returned = child, master
        else:
            returned = original_fork()
            if returned[0] == 0:
                signal.signal(signal.SIGALRM, signal.SIG_DFL)
                signal.alarm(3)
        if returned[0]:
            actual_child = returned[0]
        if returned[0] and boundary in ("birth", "birth-before-group", "birth-late-group"):
            label = "actual forkpty return" if boundary == "birth" else "actual fork/PTY return"
            witness.write_text(label + " before publication")
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
        nonlocal birth_gate, late_ready
        if group != actual_child:
            raise AssertionError("attempted signal of an external group")
        if signum and reaped:
            raise AssertionError("attempted group signal after actual owning reap")
        try:
            return original_killpg(group, signum)
        finally:
            if signum and birth_gate is not None:
                os.close(birth_gate)
                birth_gate = None
                if late_ready is not None:
                    if not select.select([late_ready], [], [], 1)[0]:
                        raise AssertionError("actual child did not establish its late session")
                    if os.read(late_ready, 5) != b"ready":
                        raise AssertionError("actual child session witness missing")
                    os.close(late_ready)
                    late_ready = None
                    witness.write_text(witness.read_text() + "; actual late session ready")

    def signal_child(target, signum):
        if target != os.getpid():
            if target != actual_child:
                raise AssertionError("attempted signal of an external child")
            if signum and reaped:
                raise AssertionError("attempted child signal after actual owning reap")
            if boundary == "birth-late-group" and os.getpgid(target) != actual_child:
                raise AssertionError("direct cancellation did not follow actual late group creation")
        return original_kill(target, signum)

    with patch.object(os, "forkpty", fork_return), patch.object(os, "waitpid", owning_wait), \
            patch.object(os, "killpg", signal_original), patch.object(os, "kill", signal_child):
        return guardian(spec)


class GuardianPublicationTests(unittest.TestCase):
    @contextlib.contextmanager
    def fixture(self, *, boundary=None, raw_collision=False, open_error=None,
                install_error=None, install_return_error=None, fork_error=None,
                restore_error=None, emit_error=None, final_write_error=None):
        namespace = runpy.run_path(str(PROBE))['guardian'].__globals__
        numbers = (signal.SIGINT, signal.SIGTERM, signal.SIGHUP, signal.SIGCHLD)
        original_handlers = {number: signal.getsignal(number) for number in numbers}
        expected = {number: (lambda _number, _frame: None) for number in numbers}
        native_signal, native_kill, native_close = signal.signal, os.kill, os.close
        master, slave = pty.openpty()
        os.close(slave)
        closed = False
        reaped = False
        restored = []
        packets = []
        injected = False
        writes = 0
        fake_child = 424242
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw, result = root / 'raw', root / 'result.json'
            spec = root / 'spec.json'
            spec.write_text(json.dumps({'argv': ['unexecuted-fixture'], 'cwd': str(root),
                                         'env': {}, 'raw': str(raw), 'result': str(result)}))
            if raw_collision:
                raw.write_bytes(b'original evidence')
            fixture = SimpleNamespace(spec=spec, result=result, packets=packets, restored=restored,
                                      expected=expected, guardian=namespace['guardian'])
            original_open, original_write = Path.open, Path.write_text

            def inject():
                nonlocal injected
                if not injected:
                    injected = True
                    native_kill(os.getpid(), signal.SIGTERM)

            def install(number, handler):
                restoring = handler is expected[number]
                if number == signal.SIGTERM and not restoring and install_error is not None:
                    raise install_error
                if restoring and number == signal.SIGTERM and boundary == 'restore':
                    inject()
                previous = native_signal(number, handler)
                if restoring:
                    restored.append(number)
                    if number == signal.SIGTERM and restore_error is not None:
                        raise restore_error
                elif install_return_error is not None and number == install_return_error[0]:
                    raise install_return_error[1]
                return previous

            def open_raw(path, *args, **kwargs):
                if path == raw and open_error is not None:
                    raise open_error
                return original_open(path, *args, **kwargs)

            def write_result(path, text, *args, **kwargs):
                nonlocal writes
                if path == result:
                    writes += 1
                    if writes > 1 and final_write_error is not None:
                        raise final_write_error
                count = original_write(path, text, *args, **kwargs)
                if path == result and boundary == 'write':
                    inject()
                return count

            def close_master(descriptor):
                nonlocal closed
                if descriptor == master:
                    if boundary == 'close':
                        inject()
                    closed = True
                native_close(descriptor)

            def owning_wait(pid, options):
                nonlocal reaped
                self.assertEqual((pid, options), (fake_child, 0))
                reaped = True
                return pid, 0

            def group_signal(pid, number):
                self.assertEqual(pid, fake_child)
                self.assertFalse(number and reaped, 'post-reap group signal')
                if number == 0:
                    raise ProcessLookupError()

            def signal_self(pid, number):
                self.assertEqual(pid, os.getpid(), 'fake child must never reach native signaling')
                native_kill(pid, number)

            def emit(value):
                packets.append(json.loads(json.dumps(value)))
                if 'completed' in value:
                    if boundary == 'emit':
                        inject()
                    if emit_error is not None:
                        raise emit_error

            try:
                for number, handler in expected.items():
                    native_signal(number, handler)
                with patch.object(signal, 'signal', side_effect=install), \
                        patch.object(pty, 'fork', return_value=(fake_child, master), side_effect=fork_error) as fork, \
                        patch.object(os, 'waitid', return_value=object()), \
                        patch.object(os, 'waitpid', side_effect=owning_wait), \
                        patch.object(os, 'killpg', side_effect=group_signal), \
                        patch.object(os, 'kill', side_effect=signal_self), \
                        patch.object(os, 'close', side_effect=close_master), \
                        patch.object(Path, 'open', open_raw), patch.object(Path, 'write_text', write_result), \
                        patch.dict(namespace, drain_pty=lambda *args: None, emit=emit):
                    fixture.fork = fork
                    yield fixture
            finally:
                for number, handler in original_handlers.items():
                    native_signal(number, handler)
                if not closed:
                    native_close(master)

    def assert_handlers_restored(self, fixture):
        self.assertEqual({number: signal.getsignal(number) for number in fixture.expected}, fixture.expected)

    def assert_owning_facts(self, result):
        self.assertTrue(result['owning_reap'])
        self.assertTrue(result['group_absent_after_reap'])
        self.assertEqual(result['exit'], 0)

    def test_normal_publication_restores_all_handlers(self):
        with self.fixture() as fixture:
            self.assertEqual(fixture.guardian(str(fixture.spec)), 0)
            self.assert_handlers_restored(fixture)
            result = json.loads(fixture.result.read_text())
            self.assert_owning_facts(result)
            self.assertFalse(result['forced'])

    def test_raw_open_collision_restores_all_handlers_without_forking(self):
        with self.fixture(raw_collision=True) as fixture:
            with self.assertRaises(FileExistsError):
                fixture.guardian(str(fixture.spec))
            self.assert_handlers_restored(fixture)
            fixture.fork.assert_not_called()
            self.assertFalse(fixture.result.exists())

    def test_partial_handler_install_restores_every_installed_handler(self):
        primary = OSError('fixture handler install failed')
        with self.fixture(install_error=primary) as fixture:
            with self.assertRaises(OSError) as caught:
                fixture.guardian(str(fixture.spec))
            self.assertIs(caught.exception, primary)
            self.assert_handlers_restored(fixture)
            self.assertEqual(set(fixture.restored), {signal.SIGCHLD, signal.SIGINT, signal.SIGTERM})
            fixture.fork.assert_not_called()

    def test_interruption_after_actual_install_restores_the_unpublished_prior_handler(self):
        for number in (signal.SIGCHLD, signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            primary = KeyboardInterrupt('fixture interrupted after actual handler install')
            with self.subTest(number=number), self.fixture(install_return_error=(number, primary)) as fixture:
                with self.assertRaises(KeyboardInterrupt) as caught:
                    fixture.guardian(str(fixture.spec))
                self.assertIs(caught.exception, primary)
                self.assert_handlers_restored(fixture)
                self.assertIn(number, fixture.restored)
                fixture.fork.assert_not_called()

    def test_success_shaped_body_diagnostics_cannot_escape_as_success(self):
        class BadMessage(RuntimeError):
            def __str__(self):
                raise SystemExit(0)

        class BadKind(type):
            def __getattribute__(cls, name):
                if name == '__name__':
                    raise SystemExit(0)
                return super().__getattribute__(name)

        class BadBoth(BadMessage, metaclass=BadKind):
            pass

        for label, error in (('message', BadMessage()), ('kind-and-message', BadBoth())):
            with self.subTest(label=label), self.fixture(fork_error=error) as fixture:
                self.assertEqual(fixture.guardian(str(fixture.spec)), 1)
                result = json.loads(fixture.result.read_text())
                self.assertTrue(result['forced'])
                self.assertFalse(result['owning_reap'])
                self.assertEqual(type(result['error']), str)
                self.assertTrue(result['error'])
                self.assert_handlers_restored(fixture)
        primary = RuntimeError('fixture active open failure')
        with self.fixture(open_error=primary, restore_error=BadBoth()) as fixture:
            with self.assertRaises(RuntimeError) as caught:
                fixture.guardian(str(fixture.spec))
            self.assertIs(caught.exception, primary)
            self.assertIn('BaseException', '\n'.join(primary.__notes__))
            self.assert_handlers_restored(fixture)
            fixture.fork.assert_not_called()

    def test_restoration_diagnostics_cannot_replace_an_active_failure(self):
        class Unprintable(OSError):
            def __str__(self):
                raise RuntimeError('fixture diagnostic refused')

        class RefusesNotes(RuntimeError):
            def add_note(self, _note):
                raise RuntimeError('fixture notes refused')

        for primary in (KeyboardInterrupt('fixture stopped'), SystemExit(0), RefusesNotes('fixture open failed')):
            with self.subTest(primary=type(primary).__name__), \
                    self.fixture(open_error=primary, restore_error=Unprintable()) as fixture:
                with self.assertRaises(type(primary)) as caught:
                    fixture.guardian(str(fixture.spec))
                self.assertIs(caught.exception, primary)
                self.assert_handlers_restored(fixture)
                self.assertEqual(set(fixture.restored), set(fixture.expected))
                fixture.fork.assert_not_called()

    def test_witnessed_cancellation_through_publication_never_returns_success(self):
        for boundary in ('close', 'write', 'emit', 'restore'):
            with self.subTest(boundary=boundary), self.fixture(boundary=boundary) as fixture:
                self.assertEqual(fixture.guardian(str(fixture.spec)), 1)
                result = json.loads(fixture.result.read_text())
                self.assert_owning_facts(result)
                self.assertTrue(result['forced'])
                self.assertEqual(result['cancellation_signal'], signal.SIGTERM)
                self.assert_handlers_restored(fixture)
                completed = [packet['completed'] for packet in fixture.packets if 'completed' in packet]
                self.assertEqual(len(completed), 1)
                if boundary in ('emit', 'restore'):
                    self.assertFalse(completed[0]['forced'], 'the final file and exit must correct the earlier packet')

    def test_restoration_failure_fails_publication_without_losing_owning_facts(self):
        with self.fixture(restore_error=OSError('fixture restore failed')) as fixture:
            self.assertEqual(fixture.guardian(str(fixture.spec)), 1)
            result = json.loads(fixture.result.read_text())
            self.assert_owning_facts(result)
            self.assertTrue(result['forced'])
            self.assertIn('fixture restore failed', '\n'.join(result['signal_restore_errors']))
            self.assert_handlers_restored(fixture)

    def test_completion_failure_survives_restoration_and_final_write_failures(self):
        primary = BrokenPipeError('fixture completion failed')
        with self.fixture(boundary='emit', emit_error=primary,
                          restore_error=OSError('fixture restore failed'),
                          final_write_error=OSError('fixture final write failed')) as fixture:
            with self.assertRaises(BrokenPipeError) as caught:
                fixture.guardian(str(fixture.spec))
            self.assertIs(caught.exception, primary)
            self.assert_handlers_restored(fixture)
            notes = '\n'.join(primary.__notes__)
            self.assertIn('fixture restore failed', notes)
            self.assertIn('fixture final write failed', notes)

    def test_failed_correction_cannot_combine_a_healthy_file_with_successful_cli_exit(self):
        for write_code in (None, 0, 256, -256):
            for primary_code in ('no-primary', None, 0, 256, -256, 130, 257,
                                 sys.maxsize + 1, 'ordinary-error'):
                secondary = SystemExit(write_code)
                primary = (None if primary_code == 'no-primary' else BrokenPipeError('fixture completion failed')
                           if primary_code == 'ordinary-error' else SystemExit(primary_code))
                with self.subTest(write_code=write_code, primary_code=primary_code), \
                        self.fixture(boundary='emit', emit_error=primary, final_write_error=secondary) as fixture:
                    caught = None
                    try:
                        cli_status = fixture.guardian(str(fixture.spec))
                    except SystemExit as error:
                        caught = error
                        code = error.code
                        cli_status = (0 if code is None else code & 0xff
                                      if isinstance(code, int) and -sys.maxsize - 1 <= code <= sys.maxsize else 1)
                    except BaseException as error:
                        caught = error
                        cli_status = 1
                    result = json.loads(fixture.result.read_text())
                    healthy_file = (result['exit'] == 0 and result['owning_reap']
                                    and result['group_absent_after_reap'] and not result['forced'])
                    self.assertFalse(cli_status == 0 and healthy_file,
                                     'incomplete corrective publication qualified the consumer conjunction')
                    self.assertNotEqual(cli_status, 0)
                    if primary is not None and primary_code in (130, 257, sys.maxsize + 1, 'ordinary-error'):
                        self.assertIs(caught, primary)
                    else:
                        self.assertIsInstance(caught, RuntimeError)
                        self.assertIs(caught.__cause__, secondary if primary is None else primary)
                    self.assert_handlers_restored(fixture)


class CustodianTests(unittest.TestCase):
    def test_real_pty_child_exits_and_is_reaped_before_success(self):
        self.exercise(eof=False)

    def test_parent_lifeline_eof_cancels_and_reaps_the_actual_pty_child(self):
        self.exercise(eof=True)

    def test_actual_fork_return_signal_cannot_drop_the_unpublished_child(self):
        self.transition("birth")

    def test_birth_cancellation_stops_the_owned_child_before_its_group_exists(self):
        self.transition("birth-before-group")

    def test_birth_cancellation_sweeps_a_group_formed_after_the_first_lookup(self):
        self.transition("birth-late-group")

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

    def test_failed_detector_cleanup_reaps_its_owned_actor_and_closes_pipes(self):
        child = subprocess.Popen([sys.executable, "-I", "-B", "-S", "-c",
                                  "import signal,time; signal.alarm(2); time.sleep(30)"],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE)
        finish_actor(child, timeout=.02)
        self.assertEqual(child.returncode, -signal.SIGKILL)
        with self.assertRaises(ChildProcessError):
            os.waitpid(child.pid, os.WNOHANG)
        for stream in (child.stdout, child.stderr):
            self.assertTrue(stream.closed)

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
            birth = boundary in ("birth", "birth-before-group", "birth-late-group")
            code = "import signal; signal.pause()" if birth else "import os; os.write(1,b'x'*256)"
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
                if birth:
                    # Closing a real controlling PTY can deliver SIGHUP first.
                    expected = (-signal.SIGKILL, -signal.SIGHUP) if boundary == "birth" else (-signal.SIGKILL,)
                    self.assertIn(result["exit"], expected,
                                  "the guardian must cancel its child before the fixture's safety alarm")
                if boundary in ("birth-before-group", "birth-late-group"):
                    self.assertEqual(result["cleanup_signal"]["result"], "not-found")
                    self.assertEqual(result["owned_child_signal"]["result"], "sent")
                    self.assertIn("cleanup_group_after_child_exit", result)
                if boundary == "birth-late-group":
                    self.assertIn("actual late session ready", spec.with_suffix(".witness").read_text())
                self.assertIn("actual", spec.with_suffix(".witness").read_text())
            finally:
                finish_actor(child)

    def exercise(self, *, eof):
        with tempfile.TemporaryDirectory(prefix="alab-pty-owner-") as directory:
            root = Path(directory)
            code = ("import signal,sys; from pathlib import Path; Path(sys.argv[1]).write_text('ENTERED'); signal.pause()" if eof else
                    "import os; print('ACTUAL-PTY-CHILD',flush=True); os.read(0,1)")
            code = "import signal; signal.alarm(3); " + code
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
                finish_actor(child)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--transition-actor":
        raise SystemExit(transition_actor(sys.argv[2], sys.argv[3]))
    unittest.main()
