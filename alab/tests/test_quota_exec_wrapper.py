# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Quota's actual exec wrapper preserves its PID and gives the child stdin EOF."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[1] / 'smoke/quota_wait.py'


class QuotaExecWrapperTests(unittest.TestCase):
    def test_wrapper_execs_exact_arguments_with_same_pid_and_stdin_eof(self):
        with tempfile.TemporaryDirectory(prefix='alab-quota-wrapper-') as directory:
            root = Path(directory)
            pidfile = root / 'native.pid'
            program = ('import json,os,sys; print(json.dumps(dict(pid=os.getpid(), '
                       'arguments=sys.argv[1:], stdin=sys.stdin.buffer.read().decode())))')
            arguments = ['a value with spaces', '--literal', "single'quote", '$(literal)']
            command = [sys.executable, '-B', str(SOURCE), '--exec-wrapper', str(pidfile),
                       sys.executable, '-I', '-B', '-c', program, *arguments]
            child = subprocess.Popen(command, cwd=root, stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                stdout, stderr = child.communicate(b'unread harness input\n', timeout=10)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.communicate(timeout=10)
            self.assertEqual(child.returncode, 0, stderr.decode())
            self.assertEqual(stderr, b'')
            observation = json.loads(stdout)
            self.assertEqual(observation, {'pid': child.pid, 'arguments': arguments, 'stdin': ''})
            self.assertEqual(int(pidfile.read_text()), child.pid)
            self.assertFalse((root / 'result.json').exists())

    def test_missing_exec_target_remains_a_failed_wrapper_without_a_smoke_report(self):
        with tempfile.TemporaryDirectory(prefix='alab-quota-wrapper-') as directory:
            root = Path(directory)
            pidfile = root / 'native.pid'
            done = subprocess.run([sys.executable, '-B', str(SOURCE), '--exec-wrapper',
                                   str(pidfile), str(root / 'nonexistent-target')],
                                  cwd=root, capture_output=True, timeout=10)
            self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
            self.assertIn(b'FileNotFoundError', done.stderr)
            self.assertTrue(pidfile.read_text().strip().isdigit())
            self.assertFalse((root / 'result.json').exists())


if __name__ == '__main__':
    unittest.main()
