# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Public smoke runners retain interaction errors and close owned real-child pipes."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

SMOKE = Path(__file__).resolve().parents[1] / "smoke"
sys.path.insert(0, str(SMOKE))
import capacity_exec
import weekly_reset


class SmokeRunnerInteractionTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.children = []
        self.sequence = 0

    def tearDown(self):
        for child in self.children:
            if child.returncode is None:
                child.terminate()
                child.wait(timeout=10)
            for stream in (child.stdin, child.stdout, child.stderr):
                if stream is not None and not stream.closed:
                    try:
                        stream.close()
                    except OSError:
                        pass

    def call(self, runner, *, interact=None, before=None, program="pass"):
        root = self.root / str(self.sequence)
        self.sequence += 1
        root.mkdir()
        env = runner.fresh_environment(root)
        if before:
            before(root)
        original = subprocess.Popen

        def spawn(*args, **kwargs):
            child = original(*args, **kwargs)
            self.children.append(child)
            return child

        handlers = {number: signal.getsignal(number) for number in
                    (signal.SIGINT, signal.SIGTERM, signal.SIGHUP, signal.SIGCHLD)}
        try:
            with mock.patch.object(subprocess, "Popen", side_effect=spawn):
                return runner.run(sys.executable, ["-I", "-B", "-c", program],
                                  root, env, "exec", interact=interact)
        finally:
            self.assertEqual({number: signal.getsignal(number) for number in handlers}, handlers)

    def assert_child_reaped_and_pipes_closed(self):
        child = self.children[-1]
        self.assertIsNotNone(child.returncode)
        with self.assertRaises(ChildProcessError):
            os.waitpid(child.pid, os.WNOHANG)
        for stream in (child.stdin, child.stdout):
            if stream is not None:
                self.assertTrue(stream.closed)

    def test_callback_that_reaps_then_fails_keeps_error_and_closes_pipes(self):
        for runner in (capacity_exec, weekly_reset):
            for error in (RuntimeError("fixture interaction failed"), KeyboardInterrupt(),
                          SystemExit(0), SystemExit(143)):
                with self.subTest(runner=runner.__name__, error=repr(error)):
                    def interact(child, _stdout):
                        child.wait(timeout=5)
                        raise error

                    with self.assertRaises(type(error)) as caught:
                        self.call(runner, interact=interact)
                    self.assertIs(caught.exception, error)
                    self.assert_child_reaped_and_pipes_closed()
                    if runner is weekly_reset:
                        evidence = json.loads((self.root / str(self.sequence - 1)
                                               / "exec-interaction-failure.json").read_text())
                        self.assertEqual(evidence["error"], f"{type(error).__name__}: {error}")
                        self.assertEqual(evidence["rawReturncode"], 0)
                        self.assertFalse(evidence["guardianTimedOut"])

    def test_failed_interaction_evidence_write_keeps_the_original_error(self):
        for error in (RuntimeError("fixture interaction failed"), KeyboardInterrupt(),
                      SystemExit(0), SystemExit(143)):
            with self.subTest(error=repr(error)):
                def collision(root):
                    (root / "exec-interaction-failure.json").mkdir()

                def interact(child, _stdout):
                    child.wait(timeout=5)
                    raise error

                with self.assertRaises(type(error)) as caught:
                    self.call(weekly_reset, interact=interact, before=collision)
                self.assertIs(caught.exception, error)
                self.assertIsInstance(caught.exception.__context__, FileExistsError)
                self.assert_child_reaped_and_pipes_closed()
                evidence = self.root / str(self.sequence - 1) / "exec-guardian-result.json"
                self.assertEqual(json.loads(evidence.read_text())["returncode"], 0)

    def test_weekly_interaction_keeps_eof_stdin_and_trailing_output(self):
        def interact(child, _stdout):
            self.assertIsNone(child.stdin)
            child.wait(timeout=5)

        code = self.call(weekly_reset, interact=interact,
                         program="import sys; print('stdin-bytes=' + str(len(sys.stdin.buffer.read())))")
        self.assertEqual(code, 0)
        self.assert_child_reaped_and_pipes_closed()
        output = self.root / str(self.sequence - 1) / "exec-stdout.jsonl"
        self.assertEqual(output.read_text().strip(), "stdin-bytes=0")

    def test_capacity_interaction_keeps_its_writable_stdin(self):
        def interact(child, _stdout):
            child.stdin.write(b"retained fixture input\n")

        code = self.call(capacity_exec, interact=interact,
                         program="import sys; print(sys.stdin.buffer.read().decode().strip())")
        self.assertEqual(code, 0)
        self.assert_child_reaped_and_pipes_closed()
        output = self.root / str(self.sequence - 1) / "exec-stdout.jsonl"
        self.assertEqual(output.read_text().strip(), "retained fixture input")

    def test_normal_and_signal_exits_keep_the_validated_raw_status(self):
        for runner in (capacity_exec, weekly_reset):
            for program, expected in (("pass", 0), ("raise SystemExit(23)", 23),
                                      ("import os,signal; os.kill(os.getpid(),signal.SIGUSR1)",
                                       -signal.SIGUSR1)):
                with self.subTest(runner=runner.__name__, expected=expected):
                    self.assertEqual(self.call(runner, program=program), expected)
                    self.assert_child_reaped_and_pipes_closed()

    def test_changed_guardian_evidence_still_refuses_after_interaction(self):
        for runner in (capacity_exec, weekly_reset):
            with self.subTest(runner=runner.__name__):
                def interact(child, stdout):
                    child.wait(timeout=5)
                    evidence = Path(stdout.name).with_name("exec-guardian-result.json")
                    result = json.loads(evidence.read_text())
                    result["returncode"] = 23
                    evidence.write_text(json.dumps(result))

                with self.assertRaisesRegex(RuntimeError, "guardian exit disagrees"):
                    self.call(runner, interact=interact)
                self.assert_child_reaped_and_pipes_closed()

    def test_weekly_interaction_evidence_retains_the_negative_raw_status(self):
        error = RuntimeError("fixture observation failed")

        def interact(child, _stdout):
            child.wait(timeout=5)
            raise error

        with self.assertRaises(RuntimeError) as caught:
            self.call(weekly_reset, interact=interact,
                      program="import os,signal; os.kill(os.getpid(),signal.SIGUSR1)")
        self.assertIs(caught.exception, error)
        self.assert_child_reaped_and_pipes_closed()
        evidence = json.loads((self.root / str(self.sequence - 1)
                               / "exec-interaction-failure.json").read_text())
        self.assertEqual(evidence["rawReturncode"], -signal.SIGUSR1)
        self.assertEqual(evidence["guardianExit"], 128 + signal.SIGUSR1)

    def test_weekly_deadline_does_not_inherit_a_shared_runner_global_override(self):
        with mock.patch.object(capacity_exec, "COMMAND_TIMEOUT_SECONDS", 230):
            for runner, expected in ((capacity_exec, 230), (weekly_reset, 120)):
                with self.subTest(runner=runner.__name__):
                    self.assertEqual(self.call(runner), 0)
                    self.assert_child_reaped_and_pipes_closed()
                    root = self.root / str(self.sequence - 1)
                    evidence = json.loads((root / "exec-guardian-result.json").read_text())
                    command = json.loads((root / "exec-guardian-command.json").read_text())
                    self.assertEqual(evidence["timeout_seconds"], expected)
                    self.assertEqual(command[command.index("--inherited-group-timeout") + 1], str(expected))

    def test_active_run_validates_its_deadline_snapshot_after_a_global_change(self):
        for runner, expected in ((capacity_exec, 230), (weekly_reset, 120)):
            with self.subTest(runner=runner.__name__):
                def interact(child, _stdout):
                    child.wait(timeout=5)
                    capacity_exec.COMMAND_TIMEOUT_SECONDS = 777

                with mock.patch.object(capacity_exec, "COMMAND_TIMEOUT_SECONDS", 230):
                    self.assertEqual(self.call(runner, interact=interact), 0)
                self.assert_child_reaped_and_pipes_closed()
                evidence = json.loads((self.root / str(self.sequence - 1)
                                       / "exec-guardian-result.json").read_text())
                self.assertEqual(evidence["timeout_seconds"], expected)


if __name__ == "__main__":
    unittest.main()
