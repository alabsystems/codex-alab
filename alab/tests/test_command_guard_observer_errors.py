#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Fault controls for test-local exit observations and real fixture cleanup.

Synthetic native events do not establish Linux execution. Real cleanup controls
observe their original processes independently before the injected phase failure.
"""

import errno
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
from types import SimpleNamespace
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
MODULE_NAME = "test_command_guard_lifeline"
if MODULE_NAME in sys.modules:
    # The private counterfactual runner supplies the committed baseline module.
    MODULE = sys.modules[MODULE_NAME]
else:
    SPEC = importlib.util.spec_from_file_location(
        MODULE_NAME, ROOT / "alab/tests/test_command_guard_lifeline.py")
    MODULE = importlib.util.module_from_spec(SPEC)
    sys.modules[MODULE_NAME] = MODULE
    SPEC.loader.exec_module(MODULE)


class FakeQueue:
    def __init__(self, frames, close_error=None):
        self.frames = list(frames)
        self.close_error = close_error
        self.closes = 0

    def control(self, changes, count, timeout):
        frame = self.frames.pop(0)
        if isinstance(frame, BaseException):
            raise frame
        return frame

    def close(self):
        self.closes += 1
        if self.close_error is not None:
            raise self.close_error


def fake_kqueue(queue):
    return SimpleNamespace(kqueue=lambda: queue, kevent=lambda ident, **values:
                           SimpleNamespace(ident=ident, **values), KQ_FILTER_PROC=-5,
                           KQ_EV_ADD=1, KQ_EV_ONESHOT=16, KQ_EV_ERROR=0x4000,
                           KQ_NOTE_EXIT=0x80000000)


def event(ident=123, filter=-5, flags=0, fflags=0x80000000, data=0):
    return SimpleNamespace(ident=ident, filter=filter, flags=flags, fflags=fflags, data=data)


class FakePoll:
    def __init__(self, frames, registration_error=None):
        self.frames = list(frames)
        self.registration_error = registration_error
        self.registrations = []

    def register(self, descriptor, mask):
        self.registrations.append((descriptor, mask))
        if self.registration_error is not None:
            raise self.registration_error

    def poll(self, timeout):
        return self.frames.pop(0)


def fake_pidfd(poller, closes, *, opening_error=None, close_error=None):
    descriptor = 712345  # Only the fake close below may ever receive this value.

    def opening(pid, flags):
        if opening_error is not None:
            raise opening_error
        return descriptor

    def close(value):
        closes.append(value)
        if close_error is not None:
            raise close_error

    return (SimpleNamespace(kqueue=None, poll=lambda: poller, POLLIN=1, POLLHUP=16),
            SimpleNamespace(pidfd_open=opening, close=close), descriptor)


class ObserverErrorTests(unittest.TestCase):
    def test_kqueue_registration_errors_close_the_queue_without_fallback(self):
        for number in (errno.EPERM, errno.ESRCH):
            with self.subTest(errno=number):
                primary = OSError(number, "registration refused")
                queue = FakeQueue([primary])
                fallback = mock.Mock(side_effect=AssertionError("permission failure is not fallback"))
                with (mock.patch.object(MODULE, "select", fake_kqueue(queue)),
                      mock.patch.object(MODULE, "os", SimpleNamespace(pidfd_open=fallback)),
                      self.assertRaises(OSError) as caught):
                    MODULE.OriginalProcessExit(123)
                self.assertIs(caught.exception, primary)
                self.assertEqual(queue.closes, 1)
                fallback.assert_not_called()

    def test_constructor_preserves_registration_or_cancellation_when_close_fails(self):
        for primary in (OSError(errno.EPERM, "registration refused"),
                        SystemExit(143), KeyboardInterrupt()):
            with self.subTest(primary=type(primary).__name__):
                queue = FakeQueue([primary], OSError(errno.EIO, "queue close refused"))
                with (mock.patch.object(MODULE, "select", fake_kqueue(queue)),
                      self.assertRaises(BaseException) as caught):
                    MODULE.OriginalProcessExit(123)
                self.assertEqual(queue.closes, 1)
                self.assertIs(caught.exception, primary)
                self.assertIn("queue close refused", "\n".join(getattr(caught.exception, "__notes__", [])))

    def test_constructor_cleanup_cancellation_wins_only_over_ordinary_primary(self):
        for primary in (OSError(errno.EPERM, "registration refused"), SystemExit(143)):
            with self.subTest(primary=type(primary).__name__):
                cancellation = KeyboardInterrupt()
                queue = FakeQueue([primary], cancellation)
                with (mock.patch.object(MODULE, "select", fake_kqueue(queue)),
                      self.assertRaises(BaseException) as caught):
                    MODULE.OriginalProcessExit(123)
                self.assertEqual(queue.closes, 1)
                self.assertIs(caught.exception, cancellation if isinstance(primary, Exception) else primary)
                notes = "\n".join(getattr(caught.exception, "__notes__", []))
                self.assertIn("KeyboardInterrupt", notes)
                if isinstance(primary, Exception):
                    self.assertIn("registration refused", notes)

    def test_kqueue_error_wins_over_apparent_exit_bits(self):
        queue = FakeQueue([[], [event(flags=0x4000, data=errno.EPERM)]])
        with mock.patch.object(MODULE, "select", fake_kqueue(queue)):
            watch = MODULE.OriginalProcessExit(123)
            try:
                with self.assertRaises(OSError) as caught:
                    watch.exited()
                self.assertEqual(caught.exception.errno, errno.EPERM)
                self.assertIs(watch.observed, False)
            finally:
                watch.close()
        self.assertEqual(queue.closes, 1)

    def test_kqueue_wrong_identity_or_kind_cannot_be_an_exit(self):
        for events in ([event(ident=456)], [event(filter=-1)], [event(fflags=0)],
                       [event(), event()]):
            with self.subTest(events=events):
                queue = FakeQueue([[], events])
                with mock.patch.object(MODULE, "select", fake_kqueue(queue)):
                    watch = MODULE.OriginalProcessExit(123)
                    try:
                        with self.assertRaises(RuntimeError):
                            watch.exited()
                        self.assertIs(watch.observed, False)
                    finally:
                        watch.close()
                self.assertEqual(queue.closes, 1)

    def test_exit_during_registration_refuses_the_readiness_contract(self):
        queue = FakeQueue([[event()]])
        with (mock.patch.object(MODULE, "select", fake_kqueue(queue)),
              self.assertRaisesRegex(RuntimeError, "before watcher acknowledgement")):
            MODULE.OriginalProcessExit(123)
        self.assertEqual(queue.closes, 1)

    def test_simulated_pidfd_errors_never_close_a_real_descriptor(self):
        for value, mask in ((712345, 8), (712345, 32), (712345, 1 | 32),
                            (712346, 1), (712345, 0)):
            with self.subTest(value=value, mask=mask):
                closes = []
                select_api, os_api, descriptor = fake_pidfd(FakePoll([[], [(value, mask)]]), closes)
                with (mock.patch.object(MODULE, "select", select_api),
                      mock.patch.object(MODULE, "os", os_api)):
                    watch = MODULE.OriginalProcessExit(123)
                    try:
                        with self.assertRaises(OSError):
                            watch.exited()
                        self.assertIs(watch.observed, False)
                    finally:
                        watch.close()
                self.assertEqual(closes, [descriptor])

    def test_simulated_pidfd_valid_exit_is_cached_and_owned_close_is_idempotent(self):
        for mask in (1, 16, 1 | 16):
            with self.subTest(mask=mask):
                closes = []
                poller = FakePoll([[], [], [(712345, mask)]])
                select_api, os_api, descriptor = fake_pidfd(poller, closes)
                with (mock.patch.object(MODULE, "select", select_api),
                      mock.patch.object(MODULE, "os", os_api)):
                    watch = MODULE.OriginalProcessExit(123)
                    try:
                        self.assertEqual(poller.registrations, [(descriptor, 1 | 16)])
                        self.assertIs(watch.exited(), False)
                        self.assertIs(watch.observed, False)
                        self.assertIs(watch.exited(), True)
                        self.assertIs(watch.observed, True)
                        self.assertEqual(poller.frames, [])
                        # The fake has no more frames: polling again would fail.
                        self.assertIs(watch.exited(), True)
                        watch.close()
                        self.assertIsNone(watch.descriptor)
                        self.assertIs(watch.exited(), True)
                    finally:
                        watch.close()
                self.assertEqual(closes, [descriptor])

    def test_simulated_pidfd_exit_during_registration_refuses_and_closes_once(self):
        for mask in (1, 16, 1 | 16):
            with self.subTest(mask=mask):
                closes = []
                poller = FakePoll([[(712345, mask)]])
                select_api, os_api, descriptor = fake_pidfd(poller, closes)
                with (mock.patch.object(MODULE, "select", select_api),
                      mock.patch.object(MODULE, "os", os_api),
                      self.assertRaisesRegex(RuntimeError, "before watcher acknowledgement")):
                    MODULE.OriginalProcessExit(123)
                self.assertEqual(poller.registrations, [(descriptor, 1 | 16)])
                self.assertEqual(poller.frames, [])
                self.assertEqual(closes, [descriptor])

    def test_simulated_pidfd_open_errors_have_no_allocated_handle(self):
        for number in (errno.EPERM, errno.ESRCH):
            with self.subTest(errno=number):
                closes = []
                primary = OSError(number, "pidfd open refused")
                select_api, os_api, _ = fake_pidfd(FakePoll([]), closes, opening_error=primary)
                with (mock.patch.object(MODULE, "select", select_api),
                      mock.patch.object(MODULE, "os", os_api), self.assertRaises(OSError) as caught):
                    MODULE.OriginalProcessExit(123)
                self.assertIs(caught.exception, primary)
                self.assertEqual(closes, [])

    def test_simulated_pidfd_partial_setup_preserves_primary_despite_close_error(self):
        primary = OSError(errno.EPERM, "poll registration refused")
        closes = []
        select_api, os_api, descriptor = fake_pidfd(
            FakePoll([], registration_error=primary), closes,
            close_error=OSError(errno.EIO, "pidfd close refused"))
        with (mock.patch.object(MODULE, "select", select_api),
              mock.patch.object(MODULE, "os", os_api), self.assertRaises(OSError) as caught):
            MODULE.OriginalProcessExit(123)
        self.assertEqual(closes, [descriptor])
        self.assertIs(caught.exception, primary)

    def test_unavailable_native_watch_mechanisms_fail_explicitly(self):
        with (mock.patch.object(MODULE, "select", SimpleNamespace(kqueue=None)),
              mock.patch.object(MODULE, "os", SimpleNamespace()),
              self.assertRaisesRegex(RuntimeError, "watching is unavailable")):
            MODULE.OriginalProcessExit(123)

    def test_real_fixture_failure_preserves_primary_and_retires_originals_despite_cleanup_faults(self):
        for kind, cancellation in (("receipt", False), ("receipt", True),
                                   ("cache", False), ("cache", True)):
            with self.subTest(kind=kind, cancellation=cancellation):
                self.real_cleanup_control(kind, cancellation)

    def test_real_cleanup_cancellation_precedence_preserves_exact_exception_identity(self):
        for cancellation in (False, True):
            with self.subTest(body_cancellation=cancellation):
                self.real_cleanup_control("receipt", cancellation, close_cancellation=True)

    def test_successful_fixture_cannot_inherit_an_unrelated_callers_exception(self):
        for kind in ("receipt", "cache"):
            with self.subTest(kind=kind):
                self.real_cleanup_control(kind, False, active_caller=True)

    def real_cleanup_control(self, kind, cancellation, *, close_cancellation=False, active_caller=False):
        primary = (None if active_caller else SystemExit(143) if cancellation
                   else RuntimeError("controlled ready-phase refusal"))
        marker_error = OSError(errno.ENOSPC, "controlled stop-marker refusal")
        close_error = (None if active_caller else KeyboardInterrupt() if close_cancellation
                       else OSError(errno.EIO, "controlled watch close refusal after close"))
        wait_error = OSError(errno.EIO, "controlled first original watch refusal")
        real_popen, real_watch, real_write = subprocess.Popen, MODULE.OriginalProcessExit, Path.write_text
        record = {"proxies": [], "watches": [], "independent": [], "marker_attempts": 0,
                  "phase_attempts": 0, "root": None}

        class Watch:
            def __init__(self, pid):
                self.inner = real_watch(pid)
                self.closes = 0
                self.cleanup_waits = 0
                record["watches"].append(self)

            def __getattr__(self, name):
                return getattr(self.inner, name)

            def exited(self):
                if record["phase_attempts"]:
                    self.cleanup_waits += 1
                    if self is record["watches"][0] and self.cleanup_waits == 1 and not active_caller:
                        raise wait_error
                return self.inner.exited()

            def close(self):
                self.closes += 1
                self.inner.close()
                if self is record["watches"][0] and close_error is not None:
                    raise close_error

        class Process:
            def __init__(self, command, **keywords):
                self.inner = real_popen(command, **keywords)
                self.waits = 0
                record["proxies"].append(self)
                option = "--result-file" if kind == "receipt" else "--cache-lock"
                record["root"] = Path(command[command.index(option) + 1]).parent

            def __getattr__(self, name):
                return getattr(self.inner, name)

            def phase(self):
                record["phase_attempts"] += 1
                if record["phase_attempts"] != 1:
                    return
                root = record["root"]
                if kind == "receipt":
                    command = json.loads((root / "command-ready.json").read_text())
                    writer = json.loads((root / "writer-ready.json").read_text())
                    pids = (command["pid"], writer["pid"])
                else:
                    pids = (json.loads((root / "ready").read_text())[0],
                            json.loads((root / "descendant").read_text())[0])
                for pid in pids:
                    record["independent"].append(real_watch(pid))
                if any(watch.exited() for watch in record["independent"]):
                    raise AssertionError("originals were not held at the injected phase")
                if primary is not None:
                    raise primary

            def kill(self):
                self.phase()
                self.inner.kill()

            def send_signal(self, signum):
                self.phase()
                self.inner.send_signal(signum)

            def wait(self, *arguments, **keywords):
                self.waits += 1
                return self.inner.wait(*arguments, **keywords)

        def write(path, *arguments, **keywords):
            if path.name == "stop" and path.parent == record["root"]:
                record["marker_attempts"] += 1
                raise marker_error
            return real_write(path, *arguments, **keywords)

        caught, before_outer_cleanup = None, None
        unrelated = KeyError("unrelated active caller")
        caller_preserved = None
        outer_primary = None
        try:
            with (mock.patch.object(MODULE, "subprocess", SimpleNamespace(
                      Popen=Process, DEVNULL=subprocess.DEVNULL, STDOUT=subprocess.STDOUT)),
                  mock.patch.object(MODULE, "OriginalProcessExit", Watch),
                  mock.patch.object(Path, "write_text", write)):
                def invoke():
                    nonlocal caught
                    try:
                        if kind == "receipt":
                            MODULE.GuardianReceiptLossTests().exercise(after_completion=False)
                        else:
                            MODULE.GuardianLifelineTests().assert_guardian_signal(signal.SIGKILL, -signal.SIGKILL)
                    except BaseException as error:
                        caught = error

                if active_caller:
                    try:
                        raise unrelated
                    except KeyError:
                        invoke()
                        caller_preserved = sys.exc_info()[1] is unrelated
                else:
                    invoke()
            # Capture every behavioral observation BEFORE our independent
            # baseline-failure cleanup can retire anything on the fixture's behalf.
            before_outer_cleanup = {
                "waited": bool(record["proxies"]) and all(p.waits for p in record["proxies"]),
                "retired": len(record["independent"]) == 2
                           and all(w.exited() for w in record["independent"]),
                "closes": [w.closes for w in record["watches"]],
                "watch_waits": [w.cleanup_waits for w in record["watches"]],
            }
            print("OBSERVER_CLEANUP_PRE_COMPENSATION " + json.dumps({
                "fixture": kind, "body_cancellation": cancellation,
                "cleanup_cancellation": close_cancellation, "active_caller": active_caller,
                "caught_type": type(caught).__name__, "caught_is_body_primary": caught is primary,
                "phase_attempts": record["phase_attempts"], **before_outer_cleanup,
            }, sort_keys=True), flush=True)
        except BaseException as error:
            outer_primary = error
            raise
        finally:
            cleanup_errors = []
            actions = []
            root = record["root"]
            if root is not None:
                actions.append(lambda: real_write(root / "stop", "independent failed-fixture cleanup"))
            for proxy in record["proxies"]:
                actions.append(lambda proxy=proxy: proxy.inner.kill() if proxy.inner.poll() is None else None)
                actions.append(lambda proxy=proxy: proxy.inner.wait(timeout=60))
            for watch in record["independent"]:
                actions.append(lambda watch=watch: MODULE.await_condition(
                    watch.exited, "independent original process cleanup"))
            actions.extend(watch.close for watch in record["independent"])
            # Independent compensation must work on the baseline as well. It
            # neither calls the cleanup helper under test nor abandons later
            # owned actions because an earlier marker/wait/close failed.
            for action in actions:
                try:
                    action()
                except BaseException as error:
                    cleanup_errors.append(error)
            if cleanup_errors:
                selected = outer_primary
                if selected is None or isinstance(selected, Exception):
                    selected = next((e for e in cleanup_errors if not isinstance(e, Exception)), selected)
                if selected is None:
                    selected = cleanup_errors[0]
                for error in cleanup_errors:
                    selected.add_note(f"independent fixture compensation failed: {error!r}")
                if selected is not outer_primary:
                    raise selected
        if caught is not None and record["phase_attempts"] == 0:
            raise caught
        self.assertGreaterEqual(record["phase_attempts"], 1)
        self.assertEqual(record["marker_attempts"], 1)
        self.assertEqual(before_outer_cleanup["closes"], [1, 1])
        expected = (marker_error if active_caller else close_error
                    if close_cancellation and isinstance(primary, Exception) else primary)
        self.assertIs(caught, expected)
        if not active_caller:
            notes = "\n".join(getattr(caught, "__notes__", []))
            self.assertIn("stop-marker refusal", notes)
            self.assertIn("first original watch refusal", notes)
            if close_cancellation:
                self.assertIn("KeyboardInterrupt", notes)
            else:
                self.assertIn("watch close refusal", notes)
        else:
            self.assertTrue(caller_preserved)
        self.assertEqual(len(before_outer_cleanup["watch_waits"]), 2)
        self.assertTrue(all(before_outer_cleanup["watch_waits"]), "fixture skipped an original watcher")
        self.assertTrue(before_outer_cleanup["waited"], "fixture skipped its owned guardian wait")
        self.assertTrue(before_outer_cleanup["retired"], "fixture skipped original process retirement")


if __name__ == "__main__":
    unittest.main()
