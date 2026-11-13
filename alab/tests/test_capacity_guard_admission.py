#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Adversarial external status producers; never run Codex or a server."""

import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SOURCE = Path(__file__).resolve().parents[1] / "smoke/capacity_exec.py"
sys.path.insert(0, str(SOURCE.parent))
FIXTURE_PARENT = None
FIXTURES = []


class CapacityGuardAdmissionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location("capacity_guard_admission", SOURCE)
        cls.probe = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.probe)

    def fixture(self):
        root = Path(tempfile.mkdtemp(prefix="capacity-guard-admission-", dir=FIXTURE_PARENT))
        FIXTURES.append(str(root))
        print(f"preserved capacity admission fixture: {root}", flush=True)
        return root, self.probe.fresh_environment(root)

    def producer(self, change=None, *, payload=None, raw_bytes=False, missing=False,
                 actual_exit=0, observe_child=None):
        """Call the real runner against a separate, deliberately untrusted producer."""
        root, env = self.fixture()
        executable = Path("/never-executed-codex-admission-fixture")
        arguments = ["--fixture-marker", "retained input"]
        command = [str(executable), *arguments]
        record = {"schema": 1, "argv": command,
                  "timeout_seconds": self.probe.COMMAND_TIMEOUT_SECONDS,
                  "returncode": 0, "timed_out": False}
        if change is not None:
            record.update(change)
        if not raw_bytes:
            payload = json.dumps(record).encode()
        if missing:
            payload = None
        guardian = root / "fake-guardian.py"
        guardian.write_text(f'''import json, os, sys
from pathlib import Path
root = Path({str(root)!r})
with (root / "producer-started.json").open("x") as stream:
    json.dump({{"argv": sys.argv, "cwd": os.getcwd(), "pid": os.getpid()}}, stream)
payload = {payload!r}
if payload is not None:
    with (root / "admission-guardian-result.json").open("xb") as stream:
        stream.write(payload)
raise SystemExit({actual_exit!r})
''')
        children = []
        popen = subprocess.Popen

        def observe_spawn(*args, **kwargs):
            child = popen(*args, **kwargs)
            children.append(child)
            if observe_child is not None:
                observe_child(child)
            return child

        try:
            with mock.patch.object(self.probe, "GUARD_PATH", guardian), \
                    mock.patch.object(self.probe.subprocess, "Popen", side_effect=observe_spawn):
                return self.probe.run(executable, arguments, root, env, "admission")
        finally:
            # A refusal is meaningful only if our adversarial producer actually
            # ran, exited as configured, and was reaped by the real runner.
            observed = [child.returncode for child in children]
            for child in children:
                if child.returncode is None:
                    child.wait(timeout=10)
            (root / "admission-observation.json").write_text(json.dumps({
                "expectedProducerExit": actual_exit, "runnerObservedExits": observed,
                "afterFixtureCleanupExits": [child.returncode for child in children],
                "producerPids": [child.pid for child in children]}, indent=2) + "\n")
            self.assertTrue((root / "producer-started.json").is_file())
            self.assertEqual(len(children), 1)
            self.assertNotIn(None, observed, "runner returned before reaping its guardian")
            self.assertEqual(children[0].returncode, actual_exit)
            self.assertEqual(json.loads((root / "admission-command.json").read_text()), command)

    def test_valid_external_raw_status_preserves_normal_and_signal_exits(self):
        for raw, actual in ((0, 0), (1, 1), (23, 23), (-11, 139)):
            with self.subTest(raw=raw, actual=actual):
                self.assertEqual(self.producer({"returncode": raw}, actual_exit=actual), raw)

    def test_missing_empty_or_partial_result_cannot_pass_a_zero_exit(self):
        for label, options in (("missing", {"missing": True}),
                               ("empty", {"raw_bytes": True, "payload": b""}),
                               ("partial", {"raw_bytes": True, "payload": b'{"schema":'})):
            with self.subTest(label=label):
                with self.assertRaises((RuntimeError, OSError, ValueError)):
                    self.producer(**options)

    def test_wrong_command_or_timeout_cannot_pass_a_zero_exit(self):
        for change in ({"argv": ["a different command"]},
                       {"timeout_seconds": self.probe.COMMAND_TIMEOUT_SECONDS + 1}):
            with self.subTest(change=change):
                with self.assertRaises(RuntimeError):
                    self.producer(change)

    def test_raw_status_must_be_an_integer_and_not_a_boolean(self):
        for value in (False, True, 0.0, "0", None):
            with self.subTest(value=value):
                with self.assertRaises(RuntimeError):
                    self.producer({"returncode": value})

    def test_wrong_schema_cannot_pass_a_zero_exit(self):
        for value in (2, True, "1"):
            with self.subTest(value=value):
                with self.assertRaises(RuntimeError):
                    self.producer({"schema": value})

    def test_timeout_flag_must_be_a_boolean(self):
        for value in (0, 1, "false", None):
            with self.subTest(value=value):
                with self.assertRaises(RuntimeError):
                    self.producer({"timed_out": value})

    def test_raw_status_must_match_the_actual_guardian_exit(self):
        for raw, actual in ((0, 23), (23, 0), (-11, 0), (-11, 137)):
            with self.subTest(raw=raw, actual=actual):
                with self.assertRaises(RuntimeError):
                    self.producer({"returncode": raw}, actual_exit=actual)

    def test_valid_timeout_record_raises_timeout_instead_of_returning_status(self):
        with self.assertRaises(subprocess.TimeoutExpired) as stopped:
            self.producer({"returncode": None, "timed_out": True}, actual_exit=124)
        self.assertEqual(stopped.exception.timeout, self.probe.COMMAND_TIMEOUT_SECONDS)
        self.assertEqual(stopped.exception.cmd[0], "/never-executed-codex-admission-fixture")

    def test_preexisting_result_refuses_before_starting_any_guardian(self):
        root, env = self.fixture()
        guardian = root / "unused-guardian.py"
        guardian.write_text("raise AssertionError('preexisting evidence started a producer')\n")
        result = root / "admission-guardian-result.json"
        retained = b"preserve this prior evidence\n"
        result.write_bytes(retained)
        with mock.patch.object(self.probe, "GUARD_PATH", guardian), \
                mock.patch.object(self.probe.subprocess, "Popen",
                                  side_effect=AssertionError("preexisting evidence spawned")), \
                self.assertRaisesRegex(RuntimeError, "already exists"):
            self.probe.run(Path("/never-executed"), [], root, env, "admission")
        self.assertEqual(result.read_bytes(), retained)
        self.assertFalse((root / "admission-command.json").exists())

    def test_interrupt_after_actual_reap_cannot_signal_a_stale_guardian_pid(self):
        kill = os.kill
        gaps = []
        stale_signals = []

        def inject_status_storage_gap(child):
            wait = child.wait
            first = True

            def interrupted_wait(timeout=None):
                nonlocal first
                code = wait(timeout=timeout)
                if first:
                    first = False
                    self.assertEqual(code, 0)
                    # The child really is reaped. Reopen only the Python status
                    # cache to reproduce interruption before its original store.
                    with self.assertRaises(ChildProcessError):
                        os.waitpid(child.pid, os.WNOHANG)
                    child.returncode = None
                    gaps.append(child.pid)
                    kill(os.getpid(), signal.SIGTERM)
                return code

            child.wait = interrupted_wait

        def checked_kill(pid, signum):
            if pid in gaps:
                # Never actually signal an already-reaped fixture PID, even in
                # the broken direct-os.kill control; report the attempted signal.
                stale_signals.append([pid, signum])
                raise ProcessLookupError
            return kill(pid, signum)

        with mock.patch.object(os, "kill", side_effect=checked_kill), \
                self.assertRaises(SystemExit) as stopped:
            self.producer(observe_child=inject_status_storage_gap)
        self.assertEqual(stopped.exception.code, 128 + signal.SIGTERM)
        self.assertEqual(len(gaps), 1, "the actual reaped-status gap was exercised")
        self.assertEqual(stale_signals, [], "cleanup attempted a signal after actual child reap")

    def test_cancellation_deferred_during_normal_cleanup_cannot_return_success(self):
        install = signal.signal
        original_int = signal.getsignal(signal.SIGINT)
        children = []
        delivered = []

        def cancel_at_restoration(signum, handler):
            if (signum == signal.SIGINT and handler is original_int and children
                    and children[0].returncode == 0 and not delivered):
                # The real producer already exited; the runner is restoring its
                # handlers after cleanup. Its SIGTERM handler must defer, then
                # preserve this actual cancellation rather than admit status0.
                delivered.append(children[0].pid)
                os.kill(os.getpid(), signal.SIGTERM)
            return install(signum, handler)

        with mock.patch.object(self.probe.signal, "signal", side_effect=cancel_at_restoration), \
                self.assertRaises(SystemExit) as stopped:
            self.producer(observe_child=children.append)
        self.assertEqual(stopped.exception.code, 128 + signal.SIGTERM)
        self.assertEqual(len(delivered), 1, "actual cleanup-time SIGTERM was delivered")


if __name__ == "__main__":
    unittest.main()
