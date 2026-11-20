# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""CLI operation interruptions cannot become successful exits; public APIs stay exact."""
from contextlib import redirect_stderr, redirect_stdout
import io
import json
from pathlib import Path
import subprocess
import signal
import sys
import tempfile
import unittest
from unittest.mock import patch

import test_github_push as publication_tests
import test_github_cleanup as cleanup_tests


CHILD = r'''
import ast
import hashlib
import importlib.util
import json
from pathlib import Path
import signal
import sys

path, case, witness_path = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
spec = importlib.util.spec_from_file_location('controlled_cli', path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
tool = path.stem
faults = {'zero': SystemExit(0), 'none': SystemExit(None), 'nonzero': SystemExit(47),
          'false': SystemExit(False), 'wrapped': SystemExit(256),
          'floating_zero': SystemExit(0.0), 'huge': SystemExit(sys.maxsize + 1),
          'keyboard': KeyboardInterrupt('controlled operation cancellation')}
if case == 'ordinary' or case.startswith('report_'):
    faults[case] = (module.GITHUB.Refused if tool == 'github_push' else module.Refused)('controlled refusal')
primary = faults.get(case)
witness = {'operation_called': False, 'operation_returned': False}
if case == 'inherited_sigchld':
    witness['ignored_after_import'] = signal.getsignal(signal.SIGCHLD) == signal.SIG_IGN

def operation(*arguments, **keywords):
    if case == 'inherited_sigchld':
        witness['default_at_operation'] = signal.getsignal(signal.SIGCHLD) == signal.SIG_DFL
    witness['operation_called'] = True
    if primary is not None:
        raise primary
    witness['operation_returned'] = True
    if tool == 'versions':
        return [dict(version='0.1.0', baseline='1.2.3')]
    return {} if tool == 'github' else b'controlled child output\n'

setattr(module, {'github': 'fetch', 'github_push': 'publish', 'versions': 'checked'}[tool], operation)
if case == 'help':
    sys.argv = [str(path), '--help']
elif tool == 'github':
    sys.argv = [str(path), '--repo', str(witness_path.parent), 'fetch', 'fixture/codex',
                'refs/heads/main:refs/remotes/origin/main']
elif tool == 'github_push':
    sys.argv = [str(path), '--repo', str(witness_path.parent), '--expected', 'a' * 40, 'fixture/codex']
else:
    sys.argv = [str(path), 'check', '--repo', str(witness_path.parent), '--commit', 'a' * 40,
                '--version', '0.1.0', '--baseline', '1.2.3', '--baseline-commit', 'b' * 40,
                '--main', 'main', '--released', str(witness_path.parent / 'unused-release')]
if case.startswith('report_'):
    class FailingStderr:
        def write(self, _text):
            raise SystemExit(256 if case == 'report_wrapped' else 0)

        def flush(self):
            pass

    sys.stderr = FailingStderr()
tree = ast.parse(path.read_bytes(), filename=str(path))
entry = next(node for node in tree.body if isinstance(node, ast.If)
             and isinstance(node.test, ast.Compare) and isinstance(node.test.left, ast.Name)
             and node.test.left.id == '__name__')
witness['entry_ast_sha256'] = hashlib.sha256(ast.dump(entry).encode()).hexdigest()
module.__name__ = '__main__'
try:
    exec(compile(ast.Module(body=[entry], type_ignores=[]), str(path), 'exec'), module.__dict__)
except BaseException as error:
    witness['escaped_type'] = type(error).__name__
    witness['escaped_code'] = error.code if isinstance(error, SystemExit) else None
    witness['escaped_primary_identity'] = error is primary
    raise
finally:
    witness_path.write_text(json.dumps(witness))
'''


class FailingSink(io.StringIO):
    def __init__(self, fault):
        super().__init__()
        self.fault = fault

    def write(self, _text):
        raise self.fault


class GithubCliTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='alab-github-cli-test-')
        self.addCleanup(self.scratch.cleanup)
        self.directory = Path(self.scratch.name)
        self.child_observations = []

    def tools(self):
        return (('github', publication_tests.G), ('github_push', publication_tests.PUSH),
                ('versions', cleanup_tests.VERSIONS))

    def operation_name(self, tool):
        return {'github': 'fetch', 'github_push': 'publish', 'versions': 'checked'}[tool]

    def refusal(self, tool, module):
        return (module.GITHUB.Refused if tool == 'github_push' else module.Refused)('controlled refusal')

    def arguments(self, tool, help_only=False):
        if help_only:
            return [tool, '--help']
        if tool == 'github':
            return [tool, '--repo', str(self.directory), 'fetch', 'fixture/codex',
                    'refs/heads/main:refs/remotes/origin/main']
        if tool == 'github_push':
            return [tool, '--repo', str(self.directory), '--expected', 'a' * 40, 'fixture/codex']
        return [tool, 'check', '--repo', str(self.directory), '--commit', 'a' * 40,
                '--version', '0.1.0', '--baseline', '1.2.3', '--baseline-commit', 'b' * 40,
                '--main', 'main', '--released', str(self.directory / 'unused-release')]

    def main(self, tool, module, operation, help_only=False, stdout_sink=None, stderr_sink=None):
        output = io.TextIOWrapper(io.BytesIO(), encoding='utf-8') if stdout_sink is None else stdout_sink
        error = io.StringIO() if stderr_sink is None else stderr_sink
        name = self.operation_name(tool)
        with patch.object(sys, 'argv', self.arguments(tool, help_only)), \
                patch.object(module, name, side_effect=operation), \
                redirect_stdout(output), redirect_stderr(error):
            returned = module.main()
        output.flush()
        text = output.buffer.getvalue().decode('utf-8') if hasattr(output, 'buffer') else output.getvalue()
        return returned, text, error.getvalue()

    def child(self, module, case):
        script = self.directory / 'child.py'
        script.write_text(CHILD)
        witness = self.directory / (Path(module.__file__).stem + '-' + case + '.json')
        command = [sys.executable, '-I', '-B', str(script), str(Path(module.__file__)), case, str(witness)]
        if case == 'inherited_sigchld':
            launcher = ('import os, signal, sys; signal.signal(signal.SIGCHLD, signal.SIG_IGN); '
                        'os.execv(sys.argv[1], sys.argv[1:])')
            command = [sys.executable, '-IB', '-c', launcher, *command]
        done = subprocess.run(command, cwd=self.directory, capture_output=True, text=True, timeout=10)
        observed = json.loads(witness.read_text())
        self.child_observations.append(dict(tool=Path(module.__file__).stem, case=case,
                                            actual_exit=done.returncode, witness=observed,
                                            stdout=done.stdout, stderr=done.stderr))
        return done, observed

    @unittest.skipUnless(hasattr(signal, 'SIGCHLD'), 'POSIX child exit signals')
    def test_entry_restores_inherited_child_exit_observation_after_import(self):
        for tool, module in self.tools():
            with self.subTest(tool=tool):
                done, witness = self.child(module, 'inherited_sigchld')
                self.assertEqual(done.returncode, 0, done.stderr)
                self.assertTrue(witness['ignored_after_import'])
                self.assertTrue(witness['default_at_operation'])

    def test_main_refuses_success_shaped_system_exit_from_incomplete_operation(self):
        for tool, module in self.tools():
            for code in (0, None, False, 256):
                with self.subTest(tool=tool, code=code):
                    primary = SystemExit(code)
                    returned, _output, error = self.main(tool, module, primary)
                    self.assertEqual(returned, 1)
                    self.assertIn('incomplete', error.lower())

    def test_main_preserves_nonzero_and_keyboard_interrupt_identity(self):
        for tool, module in self.tools():
            for primary in (SystemExit(47), SystemExit(0.0), KeyboardInterrupt('controlled operation cancellation')):
                with self.subTest(tool=tool, cancellation=type(primary).__name__):
                    with self.assertRaises(BaseException) as caught:
                        self.main(tool, module, primary)
                    self.assertTrue(caught.exception is primary)
        primary = SystemExit(sys.maxsize + 1)
        with self.assertRaises(BaseException) as caught:
            self.main('github', publication_tests.G, primary)
        self.assertTrue(caught.exception is primary)

    def test_main_retains_healthy_returned_success(self):
        for tool, module in self.tools():
            with self.subTest(tool=tool):
                value = {'github': {}, 'github_push': b'healthy output\n',
                         'versions': [dict(version='0.1.0', baseline='1.2.3')]}[tool]
                returned, output, error = self.main(tool, module, lambda *_args, **_kwargs: value)
                self.assertIn(returned, (None, 0))
                self.assertEqual(error, '')
                self.assertEqual(output, {'github': '', 'github_push': 'healthy output\n',
                                          'versions': '0.1.0+codex.1.2.3\n'}[tool])

    def test_help_stays_zero_without_calling_an_operation(self):
        for tool, module in self.tools():
            with self.subTest(tool=tool):
                with self.assertRaises(SystemExit) as caught:
                    self.main(tool, module, AssertionError('help must not run operation'), help_only=True)
                self.assertEqual(caught.exception.code, 0)

    def test_ordinary_refusal_returns_one_and_reports_primary(self):
        for tool, module in self.tools():
            for fault in (None, RuntimeError('diagnostic sink'), SystemExit(0), SystemExit(None), SystemExit(256)):
                with self.subTest(tool=tool, reporting_fault=type(fault).__name__):
                    primary = self.refusal(tool, module)
                    returned, _output, error = self.main(
                        tool, module, primary, stderr_sink=None if fault is None else FailingSink(fault))
                    self.assertEqual(returned, 1)
                    if fault is None:
                        self.assertIn('controlled refusal', error)

    def test_publication_advanced_retains_fetch_first_interface(self):
        for fault in (None, SystemExit(0), SystemExit(256)):
            with self.subTest(stdout_fault=type(fault).__name__):
                primary = publication_tests.PUSH.Advanced('controlled authoritative advance')
                returned, output, error = self.main(
                    'github_push', publication_tests.PUSH, primary,
                    stdout_sink=None if fault is None else FailingSink(fault))
                self.assertEqual(returned, 1)
                if fault is None:
                    self.assertIn('!\tHEAD:refs/heads/main\t[rejected] (fetch first)', output)
                    self.assertIn('controlled authoritative advance', error)

    def test_original_entries_refuse_success_shaped_operation_and_report_exits(self):
        for tool, module in self.tools():
            for case in ('zero', 'none', 'false', 'wrapped', 'report_zero', 'report_wrapped'):
                with self.subTest(tool=tool, case=case):
                    done, witness = self.child(module, case)
                    self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                    self.assertTrue(witness['operation_called'])
                    self.assertFalse(witness['operation_returned'])
                    if not case.startswith('report_'):
                        self.assertIn('incomplete', done.stderr.lower())

    def test_original_entries_keep_success_help_refusal_and_nonzero_controls(self):
        for tool, module in self.tools():
            for case, status in (('success', 0), ('help', 0), ('ordinary', 1),
                                 ('nonzero', 47), ('floating_zero', 1), ('keyboard', -2)):
                with self.subTest(tool=tool, case=case):
                    done, witness = self.child(module, case)
                    self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                    if case == 'help':
                        self.assertFalse(witness['operation_called'])
                    elif case == 'success':
                        self.assertTrue(witness['operation_returned'])
                    elif case in ('nonzero', 'floating_zero', 'keyboard'):
                        self.assertTrue(witness['escaped_primary_identity'])
        done, witness = self.child(publication_tests.G, 'huge')
        self.assertEqual(done.returncode, 255, done.stdout + done.stderr)
        self.assertTrue(witness['escaped_primary_identity'])

    def test_main_reports_natural_owned_cleanup_notes(self):
        for tool, module in self.tools():
            for kind in (('ordinary', 'zero', 'none', 'advanced') if tool == 'github_push'
                         else ('ordinary', 'zero', 'none')):
                with self.subTest(tool=tool, primary=kind):
                    fixture = cleanup_tests.GithubCleanupTests()
                    fixture.setUp()
                    try:
                        primary = (self.refusal(tool, module) if kind == 'ordinary'
                                   else publication_tests.PUSH.Advanced('controlled authoritative advance')
                                   if kind == 'advanced' else SystemExit(0 if kind == 'zero' else None))
                        cleanup = OSError('owned scratch cleanup failure')
                        name = self.operation_name(tool)
                        original = getattr(module, name)

                        def operation(*_arguments, **_keywords):
                            with patch.object(module, name, original):
                                return fixture.invoke({'github': 'fetch_batch', 'github_push': 'publish',
                                                       'versions': 'released'}[tool],
                                                      primary=primary, cleanup_fault=cleanup)

                        returned, _output, error = self.main(tool, module, operation)
                        self.assertEqual(returned, 1)
                        self.assertEqual(len(primary.__notes__), 1)
                        self.assertIn(primary.__notes__[0], error)
                    finally:
                        fixture.doCleanups()

    def test_nonzero_reporting_cancellation_retains_exact_identity(self):
        for tool, module in self.tools():
            for primary in (SystemExit(47), KeyboardInterrupt('diagnostic cancellation')):
                with self.subTest(tool=tool, reporting_cancellation=type(primary).__name__):
                    with self.assertRaises(BaseException) as caught:
                        self.main(tool, module, self.refusal(tool, module), stderr_sink=FailingSink(primary))
                    self.assertTrue(caught.exception is primary)


if __name__ == '__main__':
    unittest.main()
