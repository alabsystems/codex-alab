# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


HELPER = Path(__file__).resolve().parents[1] / 'focused-tests.sh'
SUMMARY = ('running 1 test\ntest fixture::case ... ok\n'
           'test result: ok. 1 passed; 0 failed; 0 ignored; '
           '0 measured; 0 filtered out; finished in 0.01s\n')


class FocusedCaptureTests(unittest.TestCase):
    def run_capture(self, code, prelude='', target_exists=True, requirements=(), separator=True):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        root = Path(self.scratch.name)
        target = root if target_exists else root / 'missing'
        env = dict(os.environ, CARGO_TARGET_DIR=str(target), ALAB_PYTHON=sys.executable)
        if requirements:
            command = 'alab_run_focused 1 "${@:4}" ' + ('-- ' if separator else '') + '"$2" -c "$3"'
        else:
            command = 'alab_run_focused 1 "$2" -c "$3"'
        script = 'set -u\n. "$1"\n' + prelude + '\n' + command
        result = subprocess.run(['/bin/bash', '-c', script, 'focused-fixture',
                                 str(HELPER), sys.executable, code, *requirements], env=env,
                                capture_output=True, text=True)
        logs = list(root.glob('alab-focused-results.*'))
        return result, logs

    def test_captures_stderr_and_checks_completed_stdout(self):
        result, logs = self.run_capture(
            f'import sys; print({SUMMARY!r}, end=""); print("diagnostic", file=sys.stderr)')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(logs), 1)
        self.assertIn('diagnostic', logs[0].read_text())
        self.assertIn(SUMMARY, logs[0].read_text())

    def test_stderr_skip_cannot_turn_into_a_passing_selection(self):
        result, logs = self.run_capture(
            f'import sys; print({SUMMARY!r}, end=""); '
            'print("skipping cloud-managed subprocess: host auth", file=sys.stderr)')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('skipped scenario', result.stderr)
        self.assertIn('skipping cloud-managed subprocess', logs[0].read_text())

    def test_command_failure_wins_over_passing_text(self):
        result, logs = self.run_capture(f'import sys; print({SUMMARY!r}); sys.exit(7)',
                                        requirements=['--require-filter', 'missing'])
        self.assertEqual(result.returncode, 7)
        self.assertEqual(len(logs), 1)

    def test_capture_failure_does_not_qualify_the_command(self):
        result, _ = self.run_capture(f'print({SUMMARY!r})',
                                     prelude='tee() { cat; return 9; }',
                                     requirements=['--require-test', 'fixture::case'])
        self.assertEqual(result.returncode, 9)

    def test_empty_output_is_not_coverage(self):
        result, _ = self.run_capture('pass')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('missing selected suites', result.stderr)

    def test_missing_target_refuses_before_execution(self):
        result, logs = self.run_capture('raise RuntimeError("must not run")',
                                       target_exists=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('must not run', result.stderr)
        self.assertEqual(logs, [])

    def test_requirements_are_forwarded_to_checker_without_reaching_command(self):
        code = f'import sys; assert sys.argv == ["-c"]; print({SUMMARY!r})'
        result, logs = self.run_capture(code, requirements=[
            '--require-filter', 'fixture::', '--require-test', 'fixture::case'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(logs), 1)
        result, logs = self.run_capture(code, requirements=[
            '--require-filter', 'fixture::', '--require-filter', 'missing'])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('required filter matched no passed test: missing', result.stderr)
        self.assertEqual(len(logs), 1)

    def test_malformed_requirements_refuse_before_command_or_log_creation(self):
        for requirements, separator in ((['--require-test', ''], True),
                                        (['--require-test'], True),
                                        (['--require-test', '--require-filter'], True),
                                        (['--require-filter', '-missing'], True),
                                        (['--unknown', 'value'], True),
                                        (['--require-filter', 'fixture'], False)):
            with self.subTest(requirements=requirements, separator=separator):
                result, logs = self.run_capture('raise RuntimeError("must not run")',
                                                requirements=requirements,
                                                separator=separator)
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn('must not run', result.stderr)
                self.assertEqual(logs, [])


if __name__ == '__main__':
    unittest.main()
