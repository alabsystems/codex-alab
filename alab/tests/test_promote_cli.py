# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Promotion CLI status/diagnostics without invoking a selector or artifact build."""
from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


def module(name):
    spec = importlib.util.spec_from_file_location('promotion_cli_' + name,
                                                 Path(__file__).parents[1] / (name + '.py'))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


PROMOTE, GITHUB = module('promote'), module('github')

CHILD = r'''
import ast
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

path, case, witness_path = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
def load(name):
    spec = importlib.util.spec_from_file_location('controlled_' + name, path.with_name(name + '.py'))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value

value = load('promote')
primary = {'zero': SystemExit(0), 'none': SystemExit(None), 'false': SystemExit(False),
           'wrapped': SystemExit(256), 'nonzero': SystemExit(47),
           'floating_zero': SystemExit(0.0), 'huge': SystemExit(sys.maxsize + 1),
           'keyboard': KeyboardInterrupt('controlled promotion interruption')}.get(case)
if case in ('ordinary', 'notes', 'report_zero', 'report_wrapped'):
    primary = RuntimeError('controlled promotion refusal')
witness = {'operation_called': False, 'operation_returned': False, 'owned_removed': []}

def operation(*arguments, **keywords):
    witness['operation_called'] = True
    if case == 'notes':
        github = load('github')
        class OwnedDirectory(github._ScratchDirectory):
            def cleanup(self):
                super().cleanup()
                witness['owned_removed'].append(not Path(self.name).exists())
                raise OSError('owned cleanup fault')
        github._ScratchDirectory = OwnedDirectory
        with github.temporary_directory(prefix='owned-promote-cli-'):
            raise primary
    if primary is not None:
        raise primary
    witness['operation_returned'] = True
    return case != 'success_noop', 'controlled completed operation'

value.promote = operation
sys.argv = [str(path), '--help'] if case == 'help' else [
    str(path), '--repo', str(witness_path.parent), '--root', str(witness_path.parent),
    '--prefix', str(witness_path.parent), '--package', str(witness_path.parent),
    '--commit', 'a' * 40, '--aterm', '/never-invoked-selector']
if case.startswith(('report_', 'reason_')):
    class FailingStderr:
        def write(self, text):
            raise SystemExit(256 if case.endswith('wrapped') else 0)
        def flush(self):
            pass
    sys.stderr = FailingStderr()
tree = ast.parse(path.read_bytes(), filename=str(path))
entry = next(node for node in tree.body if isinstance(node, ast.If)
             and isinstance(node.test, ast.Compare) and isinstance(node.test.left, ast.Name)
             and node.test.left.id == '__name__')
witness['entry_ast_sha256'] = hashlib.sha256(ast.dump(entry).encode()).hexdigest()
value.__name__ = '__main__'
try:
    exec(compile(ast.Module(body=[entry], type_ignores=[]), str(path), 'exec'), value.__dict__)
except BaseException as error:
    witness['escaped_type'] = type(error).__name__
    witness['escaped_code'] = error.code if isinstance(error, SystemExit) else None
    witness['escaped_primary_identity'] = error is primary
    raise
finally:
    witness['natural_notes'] = list(getattr(primary, '__notes__', ()))
    witness_path.write_text(json.dumps(witness))
'''


class FailingSink(io.StringIO):
    def __init__(self, fault):
        super().__init__()
        self.fault = fault

    def write(self, _text):
        raise self.fault


class PromoteCliTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='alab-promote-cli-test-')
        self.addCleanup(self.scratch.cleanup)
        self.directory = Path(self.scratch.name)
        self.child_observations = []

    def arguments(self, help_only=False):
        if help_only:
            return ['promote', '--help']
        return ['promote', '--repo', str(self.directory), '--root', str(self.directory),
                '--prefix', str(self.directory), '--package', str(self.directory),
                '--commit', 'a' * 40, '--aterm', '/never-invoked-selector']

    def main(self, operation, stderr_sink=None, help_only=False):
        output = io.StringIO()
        error = io.StringIO() if stderr_sink is None else stderr_sink
        with patch.object(sys, 'argv', self.arguments(help_only)), \
                patch.object(PROMOTE, 'promote', side_effect=operation), \
                redirect_stdout(output), redirect_stderr(error):
            result = PROMOTE.main()
        return result, output.getvalue(), error.getvalue()

    def child(self, case):
        script = self.directory / 'child.py'
        script.write_text(CHILD)
        witness_path = self.directory / (case + '.json')
        done = subprocess.run([sys.executable, '-I', '-B', str(script), str(Path(PROMOTE.__file__)),
                               case, str(witness_path)], cwd=self.directory,
                              capture_output=True, text=True, timeout=10)
        witness = json.loads(witness_path.read_text())
        self.child_observations.append(dict(case=case, actual_exit=done.returncode, witness=witness,
                                            stdout=done.stdout, stderr=done.stderr))
        return done, witness

    def test_returned_selected_and_harmless_noop_remain_successful(self):
        for selected in (True, False):
            with self.subTest(selected=selected):
                returned, output, error = self.main(lambda *_args: (selected, 'completed operation'))
                self.assertEqual(returned, 0)
                self.assertEqual(output, '')
                self.assertIn('completed operation', error)

    def test_incomplete_success_shaped_operation_exit_is_nonzero(self):
        for code in (0, None, False, 256):
            with self.subTest(code=code):
                returned, _output, error = self.main(SystemExit(code))
                self.assertEqual(returned, 1)
                self.assertIn('incomplete', error.lower())

    def test_nonzero_and_keyboard_interrupt_identity_is_preserved(self):
        for primary in (SystemExit(47), SystemExit(0.0), SystemExit(sys.maxsize + 1),
                        KeyboardInterrupt('controlled interruption')):
            with self.subTest(cancellation=type(primary).__name__, code=getattr(primary, 'code', None)):
                with self.assertRaises(BaseException) as caught:
                    self.main(primary)
                self.assertTrue(caught.exception is primary)

    def test_help_remains_zero_without_operation(self):
        with self.assertRaises(SystemExit) as caught:
            self.main(AssertionError('help must not run operation'), help_only=True)
        self.assertEqual(caught.exception.code, 0)

    def test_refusal_and_advisory_reporting_faults_stay_nonzero(self):
        for fault in (None, RuntimeError('diagnostic sink'), SystemExit(0), SystemExit(None), SystemExit(256)):
            with self.subTest(reporting_fault=type(fault).__name__):
                returned, _output, error = self.main(
                    RuntimeError('controlled refusal'), None if fault is None else FailingSink(fault))
                self.assertEqual(returned, 1)
                if fault is None:
                    self.assertIn('controlled refusal', error)

    def test_success_reason_interruption_cannot_return_zero(self):
        for code in (0, None, 256):
            with self.subTest(code=code):
                returned, _output, _error = self.main(
                    lambda *_args: (True, 'completed operation'), FailingSink(SystemExit(code)))
                self.assertEqual(returned, 1)

    def test_nonzero_reporting_cancellations_preserve_exact_identity(self):
        for operation, cancellation in ((RuntimeError('controlled refusal'), SystemExit(47)),
                                        (lambda *_args: (False, 'completed no-op'), KeyboardInterrupt('sink'))):
            with self.subTest(reporting_cancellation=type(cancellation).__name__):
                with self.assertRaises(BaseException) as caught:
                    self.main(operation, FailingSink(cancellation))
                self.assertTrue(caught.exception is cancellation)

    def test_naturally_retained_owned_cleanup_notes_reach_stderr(self):
        for primary in (RuntimeError('controlled refusal'), SystemExit(0), SystemExit(None)):
            with self.subTest(primary=type(primary).__name__):
                removed = []

                class OwnedDirectory(GITHUB._ScratchDirectory):
                    def cleanup(self):
                        super().cleanup()
                        removed.append(not Path(self.name).exists())
                        raise OSError('owned cleanup fault')

                def operation(*_arguments):
                    with patch.object(GITHUB, '_ScratchDirectory', OwnedDirectory):
                        with GITHUB.temporary_directory(prefix='owned-promote-cli-'):
                            raise primary

                returned, _output, error = self.main(operation)
                self.assertEqual(returned, 1)
                self.assertEqual(removed, [True])
                self.assertEqual(len(primary.__notes__), 1)
                self.assertIn(primary.__notes__[0], error)

    def test_original_entry_refuses_success_shaped_operation_and_report_exits(self):
        for case in ('zero', 'none', 'false', 'wrapped', 'report_zero', 'report_wrapped',
                     'reason_zero', 'reason_wrapped'):
            with self.subTest(case=case):
                done, witness = self.child(case)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assertTrue(witness['operation_called'])
                self.assertEqual(witness['operation_returned'], case.startswith('reason_'))

    def test_original_entry_keeps_help_success_nonzero_and_natural_notes(self):
        controls = (('success_selected', 0), ('success_noop', 0), ('help', 0), ('ordinary', 1),
                    ('nonzero', 47), ('keyboard', -2), ('floating_zero', 1), ('huge', 255), ('notes', 1))
        for case, status in controls:
            with self.subTest(case=case):
                done, witness = self.child(case)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                if case == 'help':
                    self.assertFalse(witness['operation_called'])
                elif case.startswith('success_'):
                    self.assertTrue(witness['operation_returned'])
                elif case in ('nonzero', 'keyboard', 'floating_zero', 'huge'):
                    self.assertTrue(witness['escaped_primary_identity'])
                elif case == 'notes':
                    self.assertEqual(witness['owned_removed'], [True])
                    self.assertEqual(len(witness['natural_notes']), 1)
                    self.assertIn(witness['natural_notes'][0], done.stderr)


if __name__ == '__main__':
    unittest.main()
