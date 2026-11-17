# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Listener startup ownership with inert servers and finite owned thread controls."""
from contextlib import contextmanager
import json
from pathlib import Path
import sys
import threading
import unittest
from unittest import mock


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import capacity_exec as probe
import weekly_reset as weekly

REAL_THREAD, REAL_EVENT = threading.Thread, threading.Event


def models():
    return ((probe.serve, probe.Mock(False)), (weekly.serve, weekly.Mock("recovery")))


class InertServer:
    def __init__(self, *, stop=True, close_error=None, shutdown_error=None, stop_before_error=False):
        self.entered, self.release, self.exited, self.shutdown_entered = (REAL_EVENT() for _ in range(4))
        self.stop, self.close_error = stop, close_error
        self.shutdown_error, self.stop_before_error = shutdown_error, stop_before_error
        self.calls = []
        self.closed = False

    def serve_forever(self):
        self.calls.append("serve")
        self.entered.set()
        try:
            self.release.wait(2)
        finally:
            self.exited.set()

    def shutdown(self):
        self.calls.append("shutdown")
        self.shutdown_entered.set()
        if self.shutdown_error is not None and not self.stop_before_error:
            raise self.shutdown_error
        if self.stop:
            self.release.set()
        if self.shutdown_error is not None:
            raise self.shutdown_error
        self.exited.wait(2)

    def server_close(self):
        self.calls.append("close")
        self.closed = True
        if self.close_error is not None:
            raise self.close_error


class ListenerStartupTests(unittest.TestCase):
    def capture(self, serve, model):
        try:
            return None, serve(model)
        except BaseException as error:
            return error, None

    @contextmanager
    def owned(self, phase, primary, *, server=None, cleanup_seconds=5, clock_error=None,
              shutdown_start_error=None):
        server = InertServer() if server is None else server
        threads, gates = [], []

        def construct(**kwargs):
            self.assertIs(kwargs["daemon"], True)
            thread = REAL_THREAD(**kwargs)
            threads.append(thread)  # Own the actual object before any native start.
            index, start = len(threads), thread.start

            def start_then_boundary():
                if index == 2 and shutdown_start_error is not None:
                    raise shutdown_start_error
                start()
                if index == 1 and phase == "partial-start":
                    raise primary
                if index == 2 and phase == "shutdown-partial":
                    raise SystemExit(0)

            thread.start = start_then_boundary
            return thread

        def event():
            value = REAL_EVENT()
            if not gates:
                gates.append(value)
                original_set, fired = value.set, False

                def release_boundary():
                    nonlocal fired
                    if phase != "partial-start" and phase != "success" and not fired:
                        fired = True
                        if phase == "before-release":
                            raise primary
                        original_set()
                        self.assertTrue(server.entered.wait(2), "owned serving loop did not enter")
                        raise primary
                    original_set()

                value.set = release_boundary
            return value

        with mock.patch.object(probe.provider_fixture, "LoopbackHTTPServer", return_value=server), \
                mock.patch.object(probe.threading, "Thread", side_effect=construct), \
                mock.patch.object(probe.threading, "Event", side_effect=event), \
                mock.patch.object(probe, "LISTENER_STARTUP_CLEANUP_SECONDS", cleanup_seconds):
            try:
                if clock_error is None:
                    yield server, threads, gates
                else:
                    with mock.patch.object(probe.time, "monotonic", side_effect=clock_error):
                        yield server, threads, gates
            finally:
                # Every test actor is finite. This is fixture fallback, never
                # the observation used to claim factory-owned retirement.
                if gates:
                    REAL_EVENT.set(gates[0])
                server.release.set()
                for thread in threads:
                    if thread.ident is not None:
                        thread.join(timeout=3)
                    self.assertFalse(thread.is_alive(), "fixture failed to retire its owned worker")

    def test_never_started_construction_and_start_failures_close_only(self):
        for serve, model in models():
            for phase in ("construct", "start"):
                for primary in (RuntimeError("startup"), KeyboardInterrupt("startup"), SystemExit(0)):
                    with self.subTest(serve=serve.__module__, phase=phase, primary=type(primary).__name__):
                        server = mock.Mock(spec=("serve_forever", "server_close", "shutdown"))
                        thread = mock.Mock(spec=("start", "join", "is_alive"))
                        thread.start.side_effect = primary if phase == "start" else None
                        with mock.patch.object(probe.provider_fixture, "LoopbackHTTPServer", return_value=server), \
                                mock.patch.object(probe.threading, "Thread", return_value=thread) as construct, \
                                mock.patch.object(probe, "LISTENER_STARTUP_CLEANUP_SECONDS", 0):
                            if phase == "construct":
                                construct.side_effect = primary
                            caught, _ = self.capture(serve, model)
                        self.assertIs(caught, primary)
                        server.server_close.assert_called_once_with()
                        server.serve_forever.assert_not_called()
                        server.shutdown.assert_not_called()
                        thread.join.assert_not_called()

    def test_close_failure_never_replaces_the_startup_outcome(self):
        for serve, model in models():
            for close_error in (OSError("close"), KeyboardInterrupt("close"), SystemExit(0)):
                primary = RuntimeError("original startup")
                with self.subTest(serve=serve.__module__, close=type(close_error).__name__):
                    server = mock.Mock(spec=("serve_forever", "server_close", "shutdown"))
                    server.server_close.side_effect = close_error
                    with mock.patch.object(probe.provider_fixture, "LoopbackHTTPServer", return_value=server), \
                            mock.patch.object(probe.threading, "Thread", side_effect=primary):
                        caught, _ = self.capture(serve, model)
                    self.assertIs(caught, primary)
                    self.assertIn("close", json.dumps(probe.case_error_fields(primary)))
                    server.server_close.assert_called_once_with()
                    server.shutdown.assert_not_called()

    def test_bind_failure_transfers_no_listener_or_thread_ownership(self):
        for serve, model in models():
            for primary in (RuntimeError("bind"), KeyboardInterrupt("bind"), SystemExit(0)):
                with self.subTest(serve=serve.__module__, primary=type(primary).__name__), \
                        mock.patch.object(probe.provider_fixture, "LoopbackHTTPServer", side_effect=primary), \
                        mock.patch.object(probe.threading, "Thread") as construct:
                    caught, _ = self.capture(serve, model)
                    self.assertIs(caught, primary)
                    construct.assert_not_called()

    def test_cancellation_immediately_after_bind_retires_the_published_listener(self):
        primary = KeyboardInterrupt("actual bind returned")
        server = mock.Mock(spec=("serve_forever", "server_close", "shutdown"))
        previous, observed = sys.gettrace(), []

        def boundary(frame, event, _argument):
            if (event == "line" and frame.f_code is probe.start_listener.__code__
                    and frame.f_locals.get("server") is server):
                observed.append(True)
                sys.settrace(previous)
                raise primary
            return boundary

        def bind(*_args):
            sys._getframe(1).f_trace = boundary
            sys.settrace(boundary)
            return server

        try:
            with mock.patch.object(probe.provider_fixture, "LoopbackHTTPServer", new=bind), \
                    mock.patch.object(probe.threading, "Thread") as construct:
                caught, _ = self.capture(lambda _model: probe.start_listener(object), None)
            self.assertIs(caught, primary)
            self.assertEqual(observed, [True])
            server.server_close.assert_called_once_with()
            server.shutdown.assert_not_called()
            construct.assert_not_called()
        finally:
            sys.settrace(previous)

    def test_both_handlers_use_one_factory_and_success_hands_off_daemon_worker(self):
        self.assertIs(weekly.start_listener, probe.start_listener)
        for serve, model in models():
            with self.subTest(serve=serve.__module__), self.owned("success", None) as (server, threads, _gates):
                caught, result = self.capture(serve, model)
                self.assertIsNone(caught)
                self.assertEqual(result, (server, threads[0]))
                self.assertTrue(server.entered.wait(2))
                self.assertEqual(server.calls, ["serve"])
                self.assertFalse(server.closed)
                server.shutdown()
                threads[0].join(timeout=2)
                self.assertFalse(threads[0].is_alive())
                server.server_close()

    def test_partial_native_start_aborts_before_serving_and_retires_the_worker(self):
        for serve, model in models():
            primary = KeyboardInterrupt("partial start")
            with self.subTest(serve=serve.__module__), self.owned("partial-start", primary) as (server, threads, _gates):
                caught, _ = self.capture(serve, model)
                self.assertIs(caught, primary)
                self.assertEqual(server.calls, ["close"])
                self.assertFalse(threads[0].is_alive())

    def test_committed_cancellation_before_and_after_release_retires_both_workers(self):
        for serve, model in models():
            for phase in ("before-release", "after-release", "shutdown-partial", "close-failure"):
                primary = KeyboardInterrupt("committed startup")
                server = InertServer(close_error=OSError("close refused") if phase == "close-failure" else None)
                with self.subTest(serve=serve.__module__, phase=phase), self.owned(phase, primary, server=server) as (server, threads, _gates):
                    caught, _ = self.capture(serve, model)
                    self.assertIs(caught, primary)
                    self.assertEqual(len(threads), 2)
                    self.assertTrue(server.closed)
                    self.assertIn("shutdown", server.calls)
                    self.assertTrue(all(not thread.is_alive() for thread in threads))
                    if phase == "shutdown-partial":
                        self.assertIn("SystemExit", json.dumps(probe.case_error_fields(primary)))
                    if phase == "close-failure":
                        self.assertIn("close refused", json.dumps(probe.case_error_fields(primary)))

    def test_deadline_exhaustion_records_unconfirmed_retirement_without_replacing_primary(self):
        for serve, model in models():
            primary = KeyboardInterrupt("startup")
            with self.subTest(serve=serve.__module__), \
                    self.owned("after-release", primary, server=InertServer(stop=False), cleanup_seconds=.03) as (server, threads, _gates):
                caught, _ = self.capture(serve, model)
                self.assertIs(caught, primary)
                self.assertTrue(server.closed)
                self.assertTrue(any(thread.is_alive() for thread in threads))
                self.assertIn("retirement unconfirmed", json.dumps(probe.case_error_fields(primary)))

    def test_shutdown_failures_before_and_after_stop_are_attached_to_primary(self):
        for after_stop in (False, True):
            for failure in (OSError("shutdown refused"), KeyboardInterrupt("shutdown refused"), SystemExit(0)):
                primary = KeyboardInterrupt("startup")
                server = InertServer(shutdown_error=failure, stop_before_error=after_stop)
                with self.subTest(after_stop=after_stop, failure=type(failure).__name__), \
                        self.owned("after-release", primary, server=server, cleanup_seconds=.03 if not after_stop else 5) as (server, threads, _gates):
                    caught, _ = self.capture(probe.serve, probe.Mock(False))
                    self.assertIs(caught, primary)
                    self.assertTrue(server.closed)
                    self.assertIn(type(failure).__name__, json.dumps(probe.case_error_fields(primary)))
                    if after_stop:
                        self.assertTrue(all(not thread.is_alive() for thread in threads))
                    else:
                        self.assertIn("retirement unconfirmed", json.dumps(probe.case_error_fields(primary)))

    def test_failed_cleanup_clock_still_dispatches_shutdown_retires_and_closes(self):
        primary = KeyboardInterrupt("startup")
        with self.owned("after-release", primary, clock_error=SystemExit(0)) as (server, threads, gates):
            caught, _ = self.capture(probe.serve, probe.Mock(False))
            self.assertIs(caught, primary)
            self.assertTrue(gates[0].is_set())
            self.assertTrue(server.closed)
            self.assertEqual(len(threads), 2)
            self.assertTrue(server.shutdown_entered.wait(2), "safe shutdown was not dispatched")
            details = json.dumps(probe.case_error_fields(primary))
            self.assertIn("clock unavailable", details)
            if any(thread.is_alive() for thread in threads):
                self.assertIn("retirement unconfirmed", details)

    def test_shutdown_start_refusal_keeps_failure_and_reports_unconfirmed_retirement(self):
        for refusal in (RuntimeError("cannot start shutdown thread"),
                        KeyboardInterrupt("cancel before shutdown thread start")):
            primary = KeyboardInterrupt("cancel after committed release")
            with self.subTest(refusal=type(refusal).__name__):
                with self.owned("after-release", primary, cleanup_seconds=.03,
                                shutdown_start_error=refusal) as (server, threads, _gates):
                    caught, result = self.capture(probe.serve, probe.Mock(False))
                    self.assertIs(caught, primary)
                    self.assertIsNone(result)
                    self.assertEqual(len(threads), 2)
                    self.assertTrue(threads[0].is_alive())
                    self.assertIsNone(threads[1].ident)
                    self.assertFalse(threads[1].is_alive())
                    self.assertEqual(server.calls, ["serve", "close"])
                    self.assertFalse(server.shutdown_entered.is_set())
                    self.assertTrue(server.closed)
                    details = json.dumps(probe.case_error_fields(primary))
                    self.assertIn(type(refusal).__name__, details)
                    self.assertIn(str(refusal), details)
                    self.assertIn("retirement unconfirmed", details)
                # A closed listener is not retirement evidence. The owned fixture
                # fallback, separately, must release and join every real worker.
                self.assertTrue(all(not thread.is_alive() for thread in threads))


if __name__ == "__main__":
    unittest.main()
