# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Renewal's public listener factory with inert sockets, TLS and owned threads."""
from contextlib import contextmanager
import json
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest import mock

import test_listener_startup as common
import test_rotation_smoke_runs as rotation


RENEWAL, PROBE = rotation.renewal, rotation.probe


class InertSocket:
    def __init__(self):
        self.closed = False
        self.detached = False

    def close(self):
        self.closed = True


class RenewalListenerStartupTests(unittest.TestCase):
    @contextmanager
    def fixture(self, *, phase="success", primary=None, bind_error=None, tls_error=None,
                wrap_error=None, thread_error=None, cleanup_seconds=5):
        servers, workers, gates, constructions, order = [], [], [], [], []
        model = SimpleNamespace(root=Path("/never-opened-renewal-fixture"), lost_login=None,
                                lock=RENEWAL.probe.threading.Lock(), http_events=[])
        raw, wrapped = InertSocket(), InertSocket()
        accept_error = OSError("controlled TLS handshake refusal")

        class BoundServer(common.InertServer):
            def __init__(self, address, handler):
                order.append("bind")
                if bind_error is not None:
                    raise bind_error
                super().__init__()
                self.address, self.handler, self.socket = address, handler, raw
                servers.append(self)

            def get_request(self):
                raise accept_error

            def server_close(self):
                super().server_close()
                self.socket.close()

        def wrap(socket, *, server_side):
            order.append("wrap")
            self.assertIs(socket, raw)
            self.assertIs(server_side, True)
            if wrap_error is not None:
                raise wrap_error
            raw.detached = True  # A successful TLS wrap transfers the raw socket.
            return wrapped

        context = SimpleNamespace(wrap_socket=mock.Mock(side_effect=wrap))

        def tls(root):
            order.append("tls")
            self.assertEqual(root, model.root)
            if tls_error is not None:
                raise tls_error
            return context

        def construct(**kwargs):
            order.append("thread")
            constructions.append(kwargs)
            self.assertIs(kwargs["daemon"], True)
            if thread_error is not None:
                when, failure = thread_error
                if when == "construct":
                    raise failure
                thread = mock.Mock(spec=("start", "join", "is_alive"))
                thread.start.side_effect = failure
                return thread
            thread = common.REAL_THREAD(**kwargs)
            workers.append(thread)
            index, start = len(workers), thread.start

            def start_then_boundary():
                if index == 2 and phase == "shutdown-refusal":
                    raise RuntimeError("controlled shutdown thread refusal")
                start()
                if index == 1 and phase == "partial-start":
                    raise primary

            thread.start = start_then_boundary
            return thread

        def event():
            value = common.REAL_EVENT()
            if sys._getframe(1).f_code is PROBE.start_listener.__code__ and not gates:
                gates.append(value)
                original_set, fired = value.set, False

                def release_boundary():
                    nonlocal fired
                    if phase not in ("success", "partial-start") and not fired:
                        fired = True
                        if phase == "before-release":
                            raise primary
                        original_set()
                        self.assertTrue(servers[0].entered.wait(2), "owned serving loop did not enter")
                        raise primary
                    original_set()

                value.set = release_boundary
            return value

        with mock.patch.object(RENEWAL.provider_fixture, "LoopbackHTTPServer", new=BoundServer), \
                mock.patch.object(RENEWAL, "fixture_tls", side_effect=tls) as tls_factory, \
                mock.patch.object(PROBE.threading, "Thread", side_effect=construct), \
                mock.patch.object(PROBE.threading, "Event", new=event), \
                mock.patch.object(PROBE, "LISTENER_STARTUP_CLEANUP_SECONDS", cleanup_seconds), \
                mock.patch.object(RENEWAL.subprocess, "run", side_effect=AssertionError("openssl/process forbidden")) as run, \
                mock.patch.object(RENEWAL.subprocess, "Popen", side_effect=AssertionError("application forbidden")) as popen:
            try:
                yield SimpleNamespace(model=model, servers=servers, workers=workers, order=order,
                    constructions=constructions, raw=raw, wrapped=wrapped, tls=tls_factory,
                    context=context, accept_error=accept_error, base=BoundServer)
            finally:
                # Fallback is independent of observations of factory retirement.
                if gates:
                    common.REAL_EVENT.set(gates[0])
                for server in servers:
                    server.release.set()
                for thread in workers:
                    if thread.ident is not None:
                        thread.join(timeout=3)
                    self.assertFalse(thread.is_alive(), "fixture failed to retire its owned worker")
                run.assert_not_called()
                popen.assert_not_called()

    def capture(self, fixture, secure):
        try:
            return None, RENEWAL.serve(fixture.model, secure=secure, issuer_only=not secure)
        except BaseException as error:
            return error, None

    def test_bind_refusals_transfer_no_listener_or_worker(self):
        for secure in (False, True):
            for primary in (OSError("bind"), KeyboardInterrupt("bind"), SystemExit(0)):
                with self.subTest(secure=secure, primary=type(primary).__name__), self.fixture(bind_error=primary) as fixture:
                    caught, result = self.capture(fixture, secure)
                    self.assertIs(caught, primary)
                    self.assertIsNone(result)
                    self.assertEqual(fixture.servers, [])
                    self.assertEqual(fixture.constructions, [])
                    fixture.tls.assert_not_called()

    def test_thread_refusals_close_the_current_socket_without_shutdown(self):
        for secure in (False, True):
            for when in ("construct", "start"):
                for primary in (RuntimeError("thread"), KeyboardInterrupt("thread"), SystemExit(0)):
                    with self.subTest(secure=secure, when=when, primary=type(primary).__name__), \
                            self.fixture(thread_error=(when, primary), cleanup_seconds=0) as fixture:
                        caught, result = self.capture(fixture, secure)
                        self.assertIs(caught, primary)
                        self.assertIsNone(result)
                        self.assertEqual(fixture.servers[0].calls, ["close"])
                        self.assertTrue((fixture.wrapped if secure else fixture.raw).closed)
                        if secure:
                            self.assertTrue(fixture.raw.detached)
                            self.assertFalse(fixture.raw.closed)

    def test_tls_preparation_refusals_close_the_bound_raw_socket_before_any_worker(self):
        for phase in ("tls_error", "wrap_error"):
            for primary in (OSError("preparation"), KeyboardInterrupt("preparation"), SystemExit(0)):
                with self.subTest(phase=phase, primary=type(primary).__name__), self.fixture(**{phase: primary}) as fixture:
                    caught, result = self.capture(fixture, True)
                    self.assertIs(caught, primary)
                    self.assertIsNone(result)
                    self.assertEqual(fixture.servers[0].calls, ["close"])
                    self.assertTrue(fixture.raw.closed)
                    self.assertFalse(fixture.wrapped.closed)
                    self.assertEqual(fixture.constructions, [])

    def test_preparation_that_transfers_then_raises_closes_the_current_socket(self):
        for primary in (OSError("after replacement"), KeyboardInterrupt("after replacement"), SystemExit(0)):
            with self.subTest(primary=type(primary).__name__), self.fixture() as fixture:
                def prepare(server):
                    server.socket = fixture.context.wrap_socket(server.socket, server_side=True)
                    raise primary

                with self.assertRaises(type(primary)) as caught:
                    PROBE.start_listener(object, bind=fixture.base, prepare=prepare)
                self.assertIs(caught.exception, primary)
                self.assertEqual(fixture.servers[0].calls, ["close"])
                self.assertTrue(fixture.raw.detached)
                self.assertFalse(fixture.raw.closed)
                self.assertTrue(fixture.wrapped.closed)
                self.assertEqual(fixture.constructions, [])

    def test_positive_handoff_preserves_binding_tls_order_and_handshake_attribution(self):
        for secure in (False, True):
            with self.subTest(secure=secure), self.fixture() as fixture:
                caught, result = self.capture(fixture, secure)
                self.assertIsNone(caught)
                server, thread = result
                self.assertIs(server, fixture.servers[0])
                self.assertIs(thread, fixture.workers[0])
                self.assertTrue(thread.daemon)
                self.assertTrue(server.entered.wait(2))
                self.assertEqual(server.address, ("127.0.0.1", 0))
                self.assertEqual(fixture.order, ["bind", "tls", "wrap", "thread"] if secure else ["bind", "thread"])
                self.assertEqual(server.calls, ["serve"])
                self.assertIs(server.socket, fixture.wrapped if secure else fixture.raw)
                fixture.model.lost_login = "expired"
                with self.assertRaises(OSError) as error:
                    server.get_request()
                self.assertIs(error.exception, fixture.accept_error)
                self.assertEqual(len(fixture.model.http_events), int(secure))
                if secure:
                    self.assertIsNot(type(server), fixture.base)
                    self.assertEqual(fixture.model.http_events[0]["kind"], "handshake-refused")
                else:
                    fixture.tls.assert_not_called()
                server.shutdown()
                thread.join(timeout=2)
                self.assertFalse(thread.is_alive())
                server.server_close()
                self.assertTrue(server.socket.closed)

    def test_real_start_and_committed_cancellation_retire_owned_workers(self):
        for secure in (False, True):
            for phase in ("partial-start", "before-release", "after-release"):
                primary = KeyboardInterrupt("controlled startup boundary")
                with self.subTest(secure=secure, phase=phase), self.fixture(phase=phase, primary=primary) as fixture:
                    caught, result = self.capture(fixture, secure)
                    self.assertIs(caught, primary)
                    self.assertIsNone(result)
                    server = fixture.servers[0]
                    self.assertTrue(server.socket.closed)
                    self.assertTrue(all(not worker.is_alive() for worker in fixture.workers))
                    if phase == "partial-start":
                        self.assertEqual(server.calls, ["close"])
                        self.assertEqual(len(fixture.workers), 1)
                    else:
                        self.assertIn("shutdown", server.calls)
                        self.assertEqual(len(fixture.workers), 2)

    def test_shutdown_start_refusal_retains_unknown_until_fixture_fallback(self):
        for secure in (False, True):
            primary = KeyboardInterrupt("committed startup")
            with self.subTest(secure=secure):
                with self.fixture(phase="shutdown-refusal", primary=primary, cleanup_seconds=.03) as fixture:
                    caught, result = self.capture(fixture, secure)
                    self.assertIs(caught, primary)
                    self.assertIsNone(result)
                    self.assertEqual(len(fixture.workers), 2)
                    self.assertTrue(fixture.workers[0].is_alive())
                    self.assertIsNone(fixture.workers[1].ident)
                    self.assertEqual(fixture.servers[0].calls, ["serve", "close"])
                    self.assertTrue(fixture.servers[0].socket.closed)
                    self.assertIn("retirement unconfirmed", json.dumps(PROBE.case_error_fields(primary)))
                self.assertTrue(all(not worker.is_alive() for worker in fixture.workers))


if __name__ == "__main__":
    unittest.main()
