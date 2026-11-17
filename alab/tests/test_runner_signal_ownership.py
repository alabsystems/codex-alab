# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Own real handler mutations while forbidding any application or guardian launch."""
from contextlib import contextmanager
import json
from pathlib import Path
import signal
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import capacity_exec as probe

SIGNALS = (signal.SIGINT, signal.SIGTERM, signal.SIGHUP, signal.SIGCHLD)


def original_handler(_number, _frame):
    raise AssertionError("these controls never deliver a real signal")


class RunnerSignalOwnershipTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-runner-signals-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.sequence = 0

    @contextmanager
    def actor(self):
        real_signal = signal.signal
        originals = {number: signal.getsignal(number) for number in SIGNALS}
        baseline = {number: original_handler for number in SIGNALS}
        baseline[signal.SIGCHLD] = signal.SIG_IGN
        try:
            for number, handler in baseline.items():
                real_signal(number, handler)
            yield real_signal, baseline
        finally:
            failures = []
            for number, handler in originals.items():
                try:
                    real_signal(number, handler)
                except BaseException as error:
                    failures.append(error)
            self.assertEqual(failures, [], "test actor could not restore its own real handlers")

    def assert_restored(self, baseline):
        self.assertEqual({number: signal.getsignal(number) for number in SIGNALS}, baseline)

    def call(self, *, body=None, healthy=False, signal_call=None):
        root = self.root / str(self.sequence)
        self.sequence += 1
        root.mkdir()
        env = probe.fresh_environment(root)
        attempts = []
        executable = root / "never-executed"
        child = SimpleNamespace(returncode=0, stdin=None, stdout=None, wait=lambda: 0)

        def forbidden(*_args, **_kwargs):
            attempts.append("inert Popen boundary")
            if body is not None:
                raise body
            if not healthy:
                raise AssertionError("application/guardian launch forbidden")
            (root / "exec-guardian-result.json").write_text(json.dumps({"schema": 1,
                "argv": [str(executable)], "timeout_seconds": probe.COMMAND_TIMEOUT_SECONDS,
                "returncode": 0, "timed_out": False}))
            return child

        with mock.patch.object(probe.subprocess, "Popen", new=forbidden), \
                mock.patch.object(signal, "signal", new=signal_call or signal.signal):
            try:
                result = probe.run(executable, [], root, env, "exec")
            except BaseException as error:
                return error, None, attempts
        return None, result, attempts

    def test_real_installation_then_cancellation_restores_all_handlers_including_sigchld(self):
        for target in SIGNALS:
            for primary in (KeyboardInterrupt("controlled install cancellation"), SystemExit(0)):
                with self.subTest(target=target, primary=type(primary).__name__), self.actor() as (real_signal, baseline):
                    installed = False

                    def mutate_then_cancel(number, handler):
                        nonlocal installed
                        previous = real_signal(number, handler)
                        if number == target and handler != baseline[number] and not installed:
                            installed = True
                            raise primary
                        return previous

                    caught, result, attempts = self.call(signal_call=mutate_then_cancel)
                    self.assertTrue(installed)
                    self.assertIs(caught, primary)
                    self.assertIsNone(result)
                    self.assertEqual(attempts, [])
                    self.assert_restored(baseline)

    def test_failed_getsignal_does_not_mutate_that_handler_and_restores_prior_installations(self):
        for target in SIGNALS:
            with self.subTest(target=target), self.actor() as (real_signal, baseline):
                real_getsignal = signal.getsignal
                primary = KeyboardInterrupt("controlled getsignal cancellation")
                mutations = []

                def get(number):
                    if number == target:
                        raise primary
                    return real_getsignal(number)

                def mutate(number, handler):
                    mutations.append((number, handler))
                    return real_signal(number, handler)

                with mock.patch.object(signal, "getsignal", new=get):
                    caught, _, attempts = self.call(signal_call=mutate)
                self.assertIs(caught, primary)
                self.assertEqual(attempts, [])
                self.assertFalse(any(number == target for number, _handler in mutations))
                self.assert_restored(baseline)

    def test_restoration_failures_preserve_active_primary_and_attempt_every_later_restore(self):
        for target in SIGNALS:
            for primary in (RuntimeError("controlled body failure"), KeyboardInterrupt("controlled body cancellation"), SystemExit(0)):
                for secondary in (OSError("controlled restoration failure"), SystemExit(0)):
                    with self.subTest(target=target, primary=type(primary).__name__, secondary=type(secondary).__name__), self.actor() as (real_signal, baseline):
                        restored = []

                        def restore_then_fail(number, handler):
                            previous = real_signal(number, handler)
                            if handler == baseline[number]:
                                restored.append(number)
                                if number == target:
                                    raise secondary
                            return previous

                        caught, result, attempts = self.call(body=primary, signal_call=restore_then_fail)
                        self.assertIs(caught, primary)
                        self.assertIsNone(result)
                        self.assertEqual(len(attempts), 1)
                        self.assertEqual(restored, list(SIGNALS))
                        self.assert_restored(baseline)

    def test_no_active_primary_restoration_failures_keep_native_error_or_control(self):
        for primary in (OSError("controlled restoration failure"), KeyboardInterrupt(), SystemExit(47),
                        SystemExit(0.0), SystemExit(sys.maxsize + 1)):
            with self.subTest(primary=type(primary).__name__), self.actor() as (real_signal, baseline):
                restored = []

                def restore_then_fail(number, handler):
                    previous = real_signal(number, handler)
                    if handler == baseline[number]:
                        restored.append(number)
                        if number == signal.SIGINT:
                            raise primary
                    return previous

                caught, result, attempts = self.call(healthy=True, signal_call=restore_then_fail)
                self.assertIs(caught, primary)
                self.assertIsNone(result)
                self.assertEqual(len(attempts), 1)
                self.assertEqual(restored, list(SIGNALS))
                self.assert_restored(baseline)

    def test_no_active_primary_success_shaped_restore_exit_refuses_with_exact_cause(self):
        for code in (None, False, 0, 256, -256):
            with self.subTest(code=code), self.actor() as (real_signal, baseline):
                primary = SystemExit(code)
                restored = []

                def restore_then_cancel(number, handler):
                    previous = real_signal(number, handler)
                    if handler == baseline[number]:
                        restored.append(number)
                        if number == signal.SIGINT:
                            raise primary
                    return previous

                caught, result, _ = self.call(healthy=True, signal_call=restore_then_cancel)
                self.assertIsInstance(caught, RuntimeError)
                self.assertIs(caught.__cause__, primary)
                self.assertIsNone(result)
                self.assertEqual(restored, list(SIGNALS))
                self.assert_restored(baseline)

    def test_genuine_cleanup_control_has_authority_over_ordinary_or_zero_restore_error(self):
        for first in (OSError("controlled first restoration failure"), SystemExit(0)):
            for primary in (KeyboardInterrupt(), SystemExit(47)):
                with self.subTest(first=type(first).__name__, primary=type(primary).__name__), self.actor() as (real_signal, baseline):
                    restored = []

                    def restore_then_fail(number, handler):
                        previous = real_signal(number, handler)
                        if handler == baseline[number]:
                            restored.append(number)
                            if number == signal.SIGINT:
                                raise first
                            if number == signal.SIGTERM:
                                raise primary
                        return previous

                    caught, _, _ = self.call(healthy=True, signal_call=restore_then_fail)
                    self.assertIs(caught, primary)
                    self.assertEqual(restored, list(SIGNALS))
                    self.assert_restored(baseline)

    def test_outer_handled_exception_is_not_the_runs_primary(self):
        for failed_restore in (False, True):
            with self.subTest(failed_restore=failed_restore), self.actor() as (real_signal, baseline):
                primary = OSError("controlled restoration failure")

                def restore_then_fail(number, handler):
                    previous = real_signal(number, handler)
                    if failed_restore and number == signal.SIGINT and handler == baseline[number]:
                        raise primary
                    return previous

                try:
                    raise LookupError("unrelated caller's handled exception")
                except LookupError:
                    caught, result, attempts = self.call(healthy=True, signal_call=restore_then_fail)
                if failed_restore:
                    self.assertIs(caught, primary)
                    self.assertIsNone(result)
                else:
                    self.assertIsNone(caught)
                    self.assertEqual(result, 0)
                self.assertEqual(len(attempts), 1)
                self.assert_restored(baseline)

    def test_hostile_restoration_diagnostics_cannot_replace_primary_or_skip_restores(self):
        class Primary(RuntimeError):
            def add_note(self, _note):
                raise SystemExit(0)
            def __setattr__(self, _key, _value):
                raise SystemExit(0)

        class Secondary(OSError):
            def __str__(self):
                raise SystemExit(0)

        with self.actor() as (real_signal, baseline):
            primary = Primary("controlled primary")
            restored = []

            def restore_then_fail(number, handler):
                previous = real_signal(number, handler)
                if handler == baseline[number]:
                    restored.append(number)
                    raise Secondary()
                return previous

            caught, _, _ = self.call(body=primary, signal_call=restore_then_fail)
            self.assertIs(caught, primary)
            self.assertEqual(restored, list(SIGNALS))
            self.assert_restored(baseline)
