# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Qualification observer failures retire owned fake processes without masking cancellation."""

from contextlib import redirect_stderr, redirect_stdout
import errno
import importlib.util
import io
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "qualification_cleanup", Path(__file__).resolve().parents[1] / "qualification.py")
QUAL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(QUAL)


class FakeProcess:
    """Only public Popen operations are simulated; no PID or OS descriptor exists."""

    def __init__(self, *, primary=None, primary_at="read", close_error=None,
                 terminate_error=None, kill_error=None, waits=None):
        self.primary = primary
        self.primary_at = primary_at
        self.close_error = close_error
        self.terminate_error = terminate_error
        self.kill_error = kill_error
        self.waits = list(waits if waits is not None else [0])
        self.events = []
        self.returncode = None
        self.owned = False
        self.closed = False
        self.chunks = [b"fixture gate output\n", b""]
        self.stdout = SimpleNamespace(read1=self.read1, close=self.close)

    def read1(self, size):
        self.events.append(("read", size))
        if self.primary is not None and self.primary_at == "read":
            raise self.primary
        return self.chunks.pop(0)

    def close(self):
        self.events.append(("close", None))
        if not self.owned or self.closed:
            raise AssertionError("observer closed an unowned or consumed fake pipe")
        self.closed = True
        if self.close_error is not None:
            raise self.close_error

    def terminate(self):
        self.events.append(("terminate", None))
        if not self.owned:
            raise AssertionError("observer signalled an unowned fake process")
        if self.terminate_error is not None:
            raise self.terminate_error

    def kill(self):
        self.events.append(("kill", None))
        if not self.owned:
            raise AssertionError("observer signalled an unowned fake process")
        if self.kill_error is not None:
            raise self.kill_error

    def wait(self, timeout=None):
        self.events.append(("wait", timeout))
        if not self.owned:
            raise AssertionError("observer waited for an unowned fake process")
        if not self.waits:
            raise AssertionError("observer retried an exhausted fake wait plan")
        outcome = self.waits.pop(0)
        if isinstance(outcome, tuple):
            # CPython wait can reap and set public returncode before an
            # interruption is re-raised to the caller.
            self.returncode, error = outcome
            raise error
        if isinstance(outcome, BaseException):
            raise outcome
        self.returncode = outcome
        return outcome


class FakeStream:
    def __init__(self, primary, primary_at):
        self.primary = primary
        self.primary_at = primary_at
        self.writes = 0
        self.flushes = 0
        self.buffer = io.BytesIO()

    def write(self, value):
        self.writes += 1
        if self.primary is not None and (
                self.primary_at == "notice" and self.writes == 1
                or self.primary_at == "stream_write" and self.writes == 2):
            raise self.primary
        return self.buffer.write(value)

    def flush(self):
        self.flushes += 1
        if self.primary is not None and self.primary_at == "stream_flush" and self.flushes == 2:
            raise self.primary


class QualificationCleanupTests(unittest.TestCase):
    def exercise(self, *, primary=None, primary_at="read", close_error=None,
                 terminate_error=None, kill_error=None, waits=None, launch_error=None,
                 file_close_errors=None, file_write_error=None, file_write_at=None):
        process = FakeProcess(primary=primary, primary_at=primary_at, close_error=close_error,
                              terminate_error=terminate_error, kill_error=kill_error, waits=waits)
        stream = FakeStream(primary, primary_at)

        def launch(*args, **kwargs):
            if launch_error is not None:
                raise launch_error
            process.owned = True
            return process

        api = SimpleNamespace(Popen=mock.Mock(side_effect=launch), PIPE=subprocess.PIPE,
                              STDOUT=subprocess.STDOUT, TimeoutExpired=subprocess.TimeoutExpired)
        snapshot = {"source": "a" * 40, "tree": "b" * 40, "base": "c" * 40,
                    "upstream": "d" * 40, "clean": True}
        caught = None
        returned = None
        file_handles, file_events = [], []
        file_close_errors = file_close_errors or {}
        with tempfile.TemporaryDirectory(prefix="alab-qualification-cleanup-") as temporary:
            root = Path(temporary).resolve()
            repo, common, output = (root / name for name in ("candidate", "common", "observation"))
            repo.mkdir()
            common.mkdir()

            class OwnedFile:
                def __init__(self, handle, name):
                    self.handle, self.name = handle, name
                    self.closed = False
                    self.write_failed = False

                def write(self, value):
                    if self.name == file_write_at and file_write_error is not None and not self.write_failed:
                        self.write_failed = True
                        raise file_write_error
                    return self.handle.write(value)

                def close(self):
                    file_events.append((self.name, "close"))
                    if self.closed:
                        raise AssertionError("observer retried an already consumed owned file")
                    self.closed = True
                    self.handle.close()
                    if self.name in file_close_errors:
                        raise file_close_errors[self.name]

                def __getattr__(self, name):
                    return getattr(self.handle, name)

                def __enter__(self):
                    return self

                def __exit__(self, *args):
                    self.close()
                    return False

            class ControlledPath(type(Path())):
                def open(self, *args, **kwargs):
                    handle = super().open(*args, **kwargs)
                    if self.parent == output and self.name in ("start.json", "gate.log", "record.json"):
                        owned = OwnedFile(handle, self.name)
                        file_handles.append(owned)
                        return owned
                    return handle

            with mock.patch.object(QUAL, "snapshot", return_value=snapshot), \
                    mock.patch.object(QUAL, "git", return_value=str(common)), \
                    mock.patch.object(QUAL, "subprocess", api), \
                    mock.patch.object(QUAL, "Path", ControlledPath):
                try:
                    returned = QUAL.run(repo, output, stream)
                except BaseException as error:
                    caught = error
            record_exists = (output / "record.json").exists()
            file_states = [(handle.name, handle.closed, handle.handle.closed) for handle in file_handles]
            # Compensate only for a missing source cleanup, after capturing the
            # facts asserted below. These are test-owned real file handles.
            for handle in file_handles:
                if not handle.handle.closed:
                    handle.handle.close()
        return SimpleNamespace(process=process, caught=caught, returned=returned,
                               record_exists=record_exists, launch=api.Popen,
                               file_states=file_states, file_events=file_events)

    def assert_owned_files_closed(self, observed):
        self.assertTrue(observed.file_states, "owned-file fault seam did not observe the qualification files")
        self.assertTrue(all(closed and real_closed for _, closed, real_closed in observed.file_states))
        self.assertEqual(observed.file_events, [(name, "close") for name, _, _ in observed.file_states])

    def assert_failure(self, observed, expected, *, cleanup=(), retired=True):
        self.assertIsNone(observed.returned)
        self.assertFalse(observed.record_exists)
        self.assertIs(observed.caught, expected)
        self.assert_owned_files_closed(observed)
        observed.launch.assert_called_once()
        self.assertTrue(observed.process.closed)
        self.assertEqual([name for name, _ in observed.process.events].count("close"), 1)
        if retired:
            self.assertIsNotNone(observed.process.returncode)
        for error in cleanup:
            self.assertIn(repr(error), "\n".join(getattr(expected, "__notes__", [])))

    def retirement(self, observed):
        return [(name, value) for name, value in observed.process.events
                if name in ("terminate", "wait", "kill", "close")]

    def test_read_keyboard_interrupt_survives_stdout_close_eio(self):
        primary = KeyboardInterrupt("controlled gate read")
        cleanup = OSError(errno.EIO, "controlled pipe close")
        observed = self.exercise(primary=primary, close_error=cleanup)
        self.assert_failure(observed, primary, cleanup=(cleanup,))
        self.assertEqual(self.retirement(observed),
                         [("terminate", None), ("wait", 30), ("close", None)])

    def test_read_system_exit_survives_stdout_close_eio(self):
        primary = SystemExit(47)
        cleanup = OSError(errno.EIO, "controlled pipe close")
        self.assert_failure(self.exercise(primary=primary, close_error=cleanup), primary,
                            cleanup=(cleanup,))

    def test_stream_write_cancellation_survives_terminate_and_close_errors(self):
        primary = KeyboardInterrupt("controlled stream write")
        terminate = OSError(errno.EIO, "controlled terminate")
        close = OSError(errno.EIO, "controlled pipe close")
        observed = self.exercise(primary=primary, primary_at="stream_write",
                                 terminate_error=terminate, close_error=close)
        self.assert_failure(observed, primary, cleanup=(terminate, close))
        self.assertIn(("wait", 30), self.retirement(observed))

    def test_stream_flush_system_exit_survives_stdout_close_error(self):
        primary = SystemExit(47)
        cleanup = OSError(errno.EIO, "controlled pipe close")
        self.assert_failure(self.exercise(primary=primary, primary_at="stream_flush", close_error=cleanup),
                            primary, cleanup=(cleanup,))

    def test_ordinary_stream_error_survives_stdout_close_error(self):
        primary = OSError(errno.EPIPE, "controlled stream write")
        cleanup = OSError(errno.EIO, "controlled pipe close")
        self.assert_failure(self.exercise(primary=primary, primary_at="stream_write", close_error=cleanup),
                            primary, cleanup=(cleanup,))

    def test_terminate_failure_still_attempts_wait_and_close(self):
        primary = KeyboardInterrupt("controlled gate read")
        cleanup = OSError(errno.EIO, "controlled terminate")
        observed = self.exercise(primary=primary, terminate_error=cleanup)
        self.assert_failure(observed, primary, cleanup=(cleanup,))
        self.assertEqual(self.retirement(observed),
                         [("terminate", None), ("wait", 30), ("close", None)])

    def test_retirement_wait_error_still_attempts_kill_final_wait_and_close(self):
        primary = KeyboardInterrupt("controlled gate read")
        cleanup = OSError(errno.EIO, "controlled wait")
        observed = self.exercise(primary=primary, waits=[cleanup, 0])
        self.assert_failure(observed, primary, cleanup=(cleanup,))
        self.assertEqual(self.retirement(observed),
                         [("terminate", None), ("wait", 30), ("kill", None),
                          ("wait", None), ("close", None)])

    def test_timeout_escalates_to_kill_and_final_wait_before_close(self):
        primary = KeyboardInterrupt("controlled gate read")
        timeout = subprocess.TimeoutExpired("fixture gate", 30)
        observed = self.exercise(primary=primary, waits=[timeout, 0])
        self.assert_failure(observed, primary)
        self.assertEqual(self.retirement(observed),
                         [("terminate", None), ("wait", 30), ("kill", None),
                          ("wait", None), ("close", None)])

    def test_kill_failure_still_attempts_final_wait_and_close(self):
        primary = KeyboardInterrupt("controlled gate read")
        cleanup = OSError(errno.EIO, "controlled kill")
        timeout = subprocess.TimeoutExpired("fixture gate", 30)
        observed = self.exercise(primary=primary, waits=[timeout, 0], kill_error=cleanup)
        self.assert_failure(observed, primary, cleanup=(cleanup,))
        self.assertEqual(self.retirement(observed),
                         [("terminate", None), ("wait", 30), ("kill", None),
                          ("wait", 30), ("close", None)])

    def test_failed_kill_uses_a_bounded_reap_and_retains_its_timeout(self):
        primary = KeyboardInterrupt("controlled gate read")
        kill = OSError(errno.EPERM, "controlled kill permission")
        timeout = subprocess.TimeoutExpired("initial termination", 30)
        final = subprocess.TimeoutExpired("bounded reap after failed kill", 30)
        observed = self.exercise(primary=primary, waits=[timeout, final], kill_error=kill)
        self.assert_failure(observed, primary, cleanup=(kill, final), retired=False)
        self.assertEqual(self.retirement(observed),
                         [("terminate", None), ("wait", 30), ("kill", None),
                          ("wait", 30), ("close", None)])

    def test_final_wait_error_still_closes_and_retains_pending_cancellation(self):
        primary = KeyboardInterrupt("controlled gate read")
        cleanup = OSError(errno.EIO, "controlled final wait")
        timeout = subprocess.TimeoutExpired("fixture gate", 30)
        observed = self.exercise(primary=primary, waits=[timeout, cleanup])
        self.assert_failure(observed, primary, cleanup=(cleanup,), retired=False)
        self.assertEqual(self.retirement(observed),
                         [("terminate", None), ("wait", 30), ("kill", None),
                          ("wait", None), ("close", None)])

    def test_successful_kill_retries_only_cancellation_until_reaped(self):
        primary = OSError(errno.EPIPE, "controlled stream write")
        first = KeyboardInterrupt("controlled first reap")
        later = SystemExit(48)
        timeout = subprocess.TimeoutExpired("initial termination", 30)
        observed = self.exercise(primary=primary, primary_at="stream_write",
                                 waits=[timeout, first, later, 0])
        self.assert_failure(observed, first, cleanup=(later,))
        self.assertIs(first.__cause__, primary)
        self.assertIn(repr(primary), "\n".join(getattr(first, "__notes__", [])))
        self.assertEqual(self.retirement(observed),
                         [("terminate", None), ("wait", 30), ("kill", None),
                          ("wait", None), ("wait", None), ("wait", None), ("close", None)])

    def test_successful_kill_stops_reaping_when_cancellation_has_already_reaped(self):
        primary = KeyboardInterrupt("controlled gate read")
        cleanup = SystemExit(48)
        timeout = subprocess.TimeoutExpired("initial termination", 30)
        observed = self.exercise(primary=primary, waits=[timeout, (0, cleanup)])
        self.assert_failure(observed, primary, cleanup=(cleanup,))
        self.assertEqual(self.retirement(observed),
                         [("terminate", None), ("wait", 30), ("kill", None),
                          ("wait", None), ("close", None)])

    def test_failed_kill_bounded_reap_does_not_reset_after_cancellation(self):
        primary = KeyboardInterrupt("controlled gate read")
        kill = OSError(errno.EPERM, "controlled kill permission")
        cleanup = SystemExit(48)
        timeout = subprocess.TimeoutExpired("initial termination", 30)
        observed = self.exercise(primary=primary, waits=[timeout, cleanup], kill_error=kill)
        self.assert_failure(observed, primary, cleanup=(kill, cleanup), retired=False)
        self.assertEqual(self.retirement(observed),
                         [("terminate", None), ("wait", 30), ("kill", None),
                          ("wait", 30), ("close", None)])

    def test_cleanup_cancellation_supersedes_ordinary_error_with_cause(self):
        primary = OSError(errno.EPIPE, "controlled stream write")
        cleanup = KeyboardInterrupt("controlled pipe close")
        observed = self.exercise(primary=primary, primary_at="stream_write", close_error=cleanup)
        self.assert_failure(observed, cleanup)
        self.assertIs(cleanup.__cause__, primary)
        self.assertIn(repr(primary), "\n".join(getattr(cleanup, "__notes__", [])))

    def test_terminate_cancellation_is_preserved_while_wait_and_close_continue(self):
        primary = OSError(errno.EPIPE, "controlled stream write")
        cancellation = SystemExit(48)
        cleanup = OSError(errno.EIO, "controlled pipe close")
        observed = self.exercise(primary=primary, primary_at="stream_write",
                                 terminate_error=cancellation, close_error=cleanup)
        self.assert_failure(observed, cancellation, cleanup=(cleanup,))
        self.assertIs(cancellation.__cause__, primary)
        self.assertIn(repr(primary), "\n".join(getattr(cancellation, "__notes__", [])))

    def test_pending_cancellation_wins_over_later_cleanup_cancellations(self):
        primary = KeyboardInterrupt("controlled gate read")
        terminate = SystemExit(48)
        close = SystemExit(49)
        self.assert_failure(self.exercise(primary=primary, terminate_error=terminate, close_error=close),
                            primary, cleanup=(terminate, close))

    def test_first_cleanup_cancellation_wins_over_later_cancellation(self):
        primary = OSError(errno.EPIPE, "controlled stream write")
        first = KeyboardInterrupt("controlled terminate")
        later = SystemExit(49)
        observed = self.exercise(primary=primary, primary_at="stream_write",
                                 terminate_error=first, close_error=later)
        self.assert_failure(observed, first, cleanup=(later,))
        self.assertIs(first.__cause__, primary)

    def test_kill_cancellation_is_promoted_after_a_bounded_reap(self):
        primary = OSError(errno.EPIPE, "controlled stream write")
        cancellation = SystemExit(48)
        close = OSError(errno.EIO, "controlled pipe close")
        timeout = subprocess.TimeoutExpired("initial termination", 30)
        observed = self.exercise(primary=primary, primary_at="stream_write", waits=[timeout, 0],
                                 kill_error=cancellation, close_error=close)
        self.assert_failure(observed, cancellation, cleanup=(close,))
        self.assertIs(cancellation.__cause__, primary)
        self.assertIn(repr(primary), "\n".join(getattr(cancellation, "__notes__", [])))
        self.assertEqual(self.retirement(observed),
                         [("terminate", None), ("wait", 30), ("kill", None),
                          ("wait", 30), ("close", None)])

    def test_normal_stdout_close_error_prevents_a_completed_record(self):
        primary = OSError(errno.EIO, "controlled pipe close")
        observed = self.exercise(close_error=primary)
        self.assert_failure(observed, primary)
        self.assertEqual(self.retirement(observed), [("wait", None), ("close", None)])

    def test_normal_stdout_close_cancellation_prevents_a_completed_record(self):
        primary = KeyboardInterrupt("controlled pipe close")
        observed = self.exercise(close_error=primary)
        self.assert_failure(observed, primary)
        self.assertEqual(self.retirement(observed), [("wait", None), ("close", None)])

    def test_launch_error_does_not_clean_up_a_process_it_never_owned(self):
        primary = OSError(errno.EPERM, "controlled Popen")
        observed = self.exercise(launch_error=primary)
        self.assertIs(observed.caught, primary)
        self.assertFalse(observed.record_exists)
        self.assertEqual(observed.process.events, [])
        self.assertFalse(observed.process.owned or observed.process.closed)
        observed.launch.assert_called_once()

    def test_launch_cancellation_does_not_clean_up_a_process_it_never_owned(self):
        primary = SystemExit(47)
        observed = self.exercise(launch_error=primary)
        self.assertIs(observed.caught, primary)
        self.assertFalse(observed.record_exists)
        self.assertEqual(observed.process.events, [])
        self.assertFalse(observed.process.owned or observed.process.closed)
        observed.launch.assert_called_once()

    def test_initial_notice_cancellation_does_not_attempt_launch_or_cleanup(self):
        primary = KeyboardInterrupt("controlled initial notice")
        observed = self.exercise(primary=primary, primary_at="notice")
        self.assertIs(observed.caught, primary)
        self.assertFalse(observed.record_exists)
        self.assertEqual(observed.process.events, [])
        observed.launch.assert_not_called()

    def test_initial_process_wait_cancellation_survives_retirement_and_close_error(self):
        primary = KeyboardInterrupt("controlled normal wait")
        cleanup = OSError(errno.EIO, "controlled pipe close")
        observed = self.exercise(waits=[primary, 0], close_error=cleanup)
        self.assert_failure(observed, primary, cleanup=(cleanup,))
        self.assertEqual(self.retirement(observed),
                         [("wait", None), ("terminate", None), ("wait", 30), ("close", None)])

    def test_already_reaped_wait_cancellation_does_not_signal_or_wait_again(self):
        primary = KeyboardInterrupt("controlled wait after reap")
        cleanup = OSError(errno.EIO, "controlled pipe close")
        observed = self.exercise(waits=[(0, primary)], close_error=cleanup)
        self.assert_failure(observed, primary, cleanup=(cleanup,))
        self.assertEqual(self.retirement(observed), [("wait", None), ("close", None)])

    def test_retirement_wait_reaped_cancellation_skips_kill_and_another_wait(self):
        primary = KeyboardInterrupt("controlled gate read")
        cleanup = SystemExit(48)
        observed = self.exercise(primary=primary, waits=[(0, cleanup)])
        self.assert_failure(observed, primary, cleanup=(cleanup,))
        self.assertEqual(self.retirement(observed),
                         [("terminate", None), ("wait", 30), ("close", None)])

    def test_caller_handled_exception_does_not_hide_a_normal_close_failure(self):
        caller = RuntimeError("unrelated caller exception")
        cleanup = OSError(errno.EIO, "controlled pipe close")
        try:
            raise caller
        except RuntimeError:
            observed = self.exercise(close_error=cleanup)
            self.assertIs(sys.exc_info()[1], caller)
        self.assert_failure(observed, cleanup)

    def test_gate_log_close_error_does_not_replace_pending_read_cancellation(self):
        primary = KeyboardInterrupt("controlled gate read")
        cleanup = OSError(errno.EIO, "controlled gate log close")
        observed = self.exercise(primary=primary, file_close_errors={"gate.log": cleanup})
        self.assert_failure(observed, primary, cleanup=(cleanup,))

    def test_gate_log_close_cancellation_supersedes_ordinary_read_error_with_cause(self):
        primary = OSError(errno.EPIPE, "controlled gate read")
        cleanup = SystemExit(48)
        observed = self.exercise(primary=primary, file_close_errors={"gate.log": cleanup})
        self.assert_failure(observed, cleanup)
        self.assertIs(cleanup.__cause__, primary)
        self.assertIn(repr(primary), "\n".join(getattr(cleanup, "__notes__", [])))

    def test_launch_failure_survives_gate_log_close_error_without_child_cleanup(self):
        primary = OSError(errno.EPERM, "controlled Popen")
        cleanup = OSError(errno.EIO, "controlled gate log close")
        observed = self.exercise(launch_error=primary, file_close_errors={"gate.log": cleanup})
        self.assertIs(observed.caught, primary)
        self.assertFalse(observed.record_exists)
        self.assertEqual(observed.process.events, [])
        self.assertFalse(observed.process.owned or observed.process.closed)
        self.assert_owned_files_closed(observed)
        self.assertIn(repr(cleanup), "\n".join(getattr(primary, "__notes__", [])))
        observed.launch.assert_called_once()

    def test_start_record_write_cancellation_survives_close_error_without_launch(self):
        primary = KeyboardInterrupt("controlled start record write")
        cleanup = OSError(errno.EIO, "controlled start record close")
        observed = self.exercise(file_write_at="start.json", file_write_error=primary,
                                 file_close_errors={"start.json": cleanup})
        self.assertIs(observed.caught, primary)
        self.assertFalse(observed.record_exists)
        self.assertEqual(observed.process.events, [])
        self.assert_owned_files_closed(observed)
        self.assertIn(repr(cleanup), "\n".join(getattr(primary, "__notes__", [])))
        observed.launch.assert_not_called()

    def test_main_refuses_zero_system_exit_from_an_incomplete_run(self):
        outcomes = []
        with tempfile.TemporaryDirectory(prefix="alab-qualification-main-") as temporary:
            for code in (0, None):
                primary = SystemExit(code)
                caught = None
                returned = None
                with mock.patch.object(QUAL, "run", side_effect=primary) as run, \
                        mock.patch.object(sys, "argv", ["qualification.py", "--repo", temporary]), \
                        redirect_stderr(io.StringIO()):
                    try:
                        returned = QUAL.main()
                    except BaseException as error:
                        caught = error
                run.assert_called_once_with(Path(temporary), None)
                outcomes.append((code, returned, type(caught).__name__ if caught is not None else None))
        self.assertEqual(outcomes, [(0, 1, None), (None, 1, None)])

    def test_main_preserves_success_nonzero_cancellation_and_help(self):
        with tempfile.TemporaryDirectory(prefix="alab-qualification-main-") as temporary:
            argv = ["qualification.py", "--repo", temporary]
            with mock.patch.object(QUAL, "run", return_value=0) as run, \
                    mock.patch.object(sys, "argv", argv):
                self.assertEqual(QUAL.main(), 0)
            run.assert_called_once_with(Path(temporary), None)
            for primary in (SystemExit(47), KeyboardInterrupt("controlled run callback")):
                with mock.patch.object(QUAL, "run", side_effect=primary) as run, \
                        mock.patch.object(sys, "argv", argv), \
                        self.assertRaises(type(primary)) as caught:
                    QUAL.main()
                self.assertIs(caught.exception, primary)
                run.assert_called_once_with(Path(temporary), None)
        with mock.patch.object(QUAL, "run", side_effect=AssertionError("help must not run the gate")) as run, \
                mock.patch.object(sys, "argv", ["qualification.py", "--help"]), \
                redirect_stdout(io.StringIO()), self.assertRaises(SystemExit) as caught:
            QUAL.main()
        self.assertEqual(caught.exception.code, 0)
        run.assert_not_called()


class DiagnosticSink:
    def __init__(self, error=None, *, at_note=False):
        self.error = error
        self.at_note = at_note
        self.buffer = io.BytesIO()
        self.write_calls = 0
        self.fault_write = None

    def write(self, value):
        self.write_calls += 1
        if self.error is not None and self.fault_write is None and (
                not self.at_note or "qualification observation note:" in value):
            self.fault_write = self.write_calls
            raise self.error
        self.buffer.write(value.encode())
        return len(value)

    def flush(self):
        return None

    def getvalue(self):
        return self.buffer.getvalue().decode()


class QualificationDiagnosticTests(unittest.TestCase):
    def through_main(self, primary, sink=None):
        terminate = OSError(errno.EPERM, "controlled terminate permission")
        kill = OSError(errno.EPERM, "controlled kill permission")
        close = OSError(errno.EIO, "controlled stdout cleanup")
        initial = subprocess.TimeoutExpired("initial termination", 30)
        final = subprocess.TimeoutExpired("bounded reap remains unconfirmed", 30)
        process = FakeProcess(primary=primary, primary_at="stream_write", terminate_error=terminate,
                              kill_error=kill, close_error=close, waits=[initial, final])
        stream = FakeStream(primary, "stream_write")

        def launch(*args, **kwargs):
            process.owned = True
            return process

        api = SimpleNamespace(Popen=mock.Mock(side_effect=launch), PIPE=subprocess.PIPE,
            STDOUT=subprocess.STDOUT, TimeoutExpired=subprocess.TimeoutExpired,
            CalledProcessError=subprocess.CalledProcessError)
        snapshot = {"source": "a" * 40, "tree": "b" * 40, "base": "c" * 40,
                    "upstream": "d" * 40, "clean": True}
        real_run = QUAL.run

        def supplied_stream_run(repo, directory):
            return real_run(repo, directory, stream=stream)

        sink = DiagnosticSink() if sink is None else sink
        caught = None
        status = None
        with tempfile.TemporaryDirectory(prefix="alab-qualification-diagnostic-") as temporary:
            root = Path(temporary).resolve()
            repo, common, output = (root / name for name in ("candidate", "common", "output"))
            repo.mkdir()
            common.mkdir()
            with mock.patch.object(QUAL, "run", side_effect=supplied_stream_run) as run, \
                    mock.patch.object(QUAL, "snapshot", return_value=snapshot), \
                    mock.patch.object(QUAL, "git", return_value=str(common)), \
                    mock.patch.object(QUAL, "subprocess", api), \
                    mock.patch.object(sys, "argv", ["qualification.py", "--repo", str(repo),
                                                   "--directory", str(output)]), redirect_stderr(sink):
                try:
                    status = QUAL.main()
                except BaseException as error:
                    caught = error
            record_exists = (output / "record.json").exists()
            self.assertEqual(stream.buffer.getvalue(),
                             ("qualification log: " + str(output / "gate.log") + "\n").encode())
        run.assert_called_once()
        api.Popen.assert_called_once()
        self.assertFalse(record_exists)
        self.assertIsNone(process.returncode, "the fixture must retain its unconfirmed retirement")
        self.assertTrue(process.closed)
        self.assertFalse(stream.buffer.closed)
        self.assertFalse(sink.buffer.closed)
        notes = list(getattr(primary, "__notes__", ()))
        self.assertEqual(len(notes), 4)
        for marker in ("qualification terminate", "qualification kill", "qualification reap",
                       "qualification stdout cleanup"):
            self.assertIn(marker, "\n".join(notes), "public run did not naturally retain its cleanup fault")
        return SimpleNamespace(status=status, caught=caught, sink=sink, notes=notes)

    def assert_reported(self, primary, header):
        observed = self.through_main(primary)
        self.assertIsNone(observed.caught)
        self.assertEqual(observed.status, 1)
        text = observed.sink.getvalue()
        self.assertTrue(text.startswith(header + "\n"))
        for note in observed.notes:
            self.assertEqual(text.count(note), 1)

    def test_main_reports_naturally_retained_cleanup_notes_for_ordinary_error(self):
        primary = OSError(errno.EPIPE, "controlled observer stream")
        self.assert_reported(primary, "qualification observation: " + str(primary))

    def test_main_reports_naturally_retained_cleanup_notes_for_zero_system_exit(self):
        self.assert_reported(SystemExit(0),
            "qualification observation incomplete: observer requested a successful exit")

    def test_main_reports_naturally_retained_cleanup_notes_for_none_system_exit(self):
        self.assert_reported(SystemExit(None),
            "qualification observation incomplete: observer requested a successful exit")

    def test_diagnostic_sink_failures_do_not_make_an_incomplete_cli_succeed(self):
        cases = (
            (OSError(errno.EPIPE, "controlled observer stream"), OSError(errno.EPIPE, "header sink"), False),
            (OSError(errno.EPIPE, "controlled observer stream"), RuntimeError("header sink"), False),
            (SystemExit(0), SystemExit(0), False),
            (OSError(errno.EPIPE, "controlled observer stream"), OSError(errno.EIO, "first note sink"), True),
            (SystemExit(0), SystemExit(0), True),
            (SystemExit(None), SystemExit(None), True),
        )
        outcomes = []
        for primary, failure, at_note in cases:
            sink = DiagnosticSink(failure, at_note=at_note)
            observed = self.through_main(primary, sink)
            outcomes.append((observed.status, observed.caught is None, sink.fault_write is not None))
            if at_note and sink.fault_write is not None:
                self.assertGreater(sink.fault_write, 1)
                self.assertTrue(sink.getvalue().startswith("qualification observation"))
        self.assertEqual(outcomes, [(1, True, True)] * len(cases))

    def test_nonzero_and_keyboard_interrupt_from_diagnostic_sink_remain_exact(self):
        for failure in (SystemExit(47), KeyboardInterrupt("controlled diagnostic sink")):
            sink = DiagnosticSink(failure)
            observed = self.through_main(OSError(errno.EPIPE, "controlled observer stream"), sink)
            self.assertIs(observed.caught, failure)
            self.assertIsNone(observed.status)
            self.assertIsNotNone(sink.fault_write)

    def test_diagnostic_formatting_failure_cannot_escape_as_a_successful_exit(self):
        outcomes = []
        for failure in (RuntimeError("controlled error formatting"), SystemExit(0), SystemExit(None)):
            class Unformattable(OSError):
                def __str__(self):
                    raise failure

            sink = DiagnosticSink()
            caught = None
            status = None
            with mock.patch.object(QUAL, "run", side_effect=Unformattable(errno.EPIPE, "observer")) as run, \
                    mock.patch.object(sys, "argv", ["qualification.py"]), redirect_stderr(sink):
                try:
                    status = QUAL.main()
                except BaseException as error:
                    caught = error
            run.assert_called_once()
            outcomes.append((status, caught is None))
        self.assertEqual(outcomes, [(1, True)] * len(outcomes))

    def test_no_note_headers_and_help_output_remain_compatible(self):
        cases = (
            (OSError(errno.EPIPE, "controlled observer"), "qualification observation: [Errno 32] controlled observer\n"),
            (SystemExit(0), "qualification observation incomplete: observer requested a successful exit\n"),
            (SystemExit(None), "qualification observation incomplete: observer requested a successful exit\n"),
        )
        for primary, header in cases:
            sink = DiagnosticSink()
            with mock.patch.object(QUAL, "run", side_effect=primary) as run, \
                    mock.patch.object(sys, "argv", ["qualification.py"]), redirect_stderr(sink):
                self.assertEqual(QUAL.main(), 1)
            run.assert_called_once()
            self.assertEqual(sink.getvalue(), header)
        sink = DiagnosticSink()
        with mock.patch.object(QUAL, "run", side_effect=AssertionError("help must not run a gate")) as run, \
                mock.patch.object(sys, "argv", ["qualification.py", "--help"]), \
                redirect_stdout(io.StringIO()), redirect_stderr(sink), self.assertRaises(SystemExit) as caught:
            QUAL.main()
        self.assertEqual(caught.exception.code, 0)
        run.assert_not_called()
        self.assertEqual(sink.getvalue(), "")


if __name__ == "__main__":
    unittest.main()
