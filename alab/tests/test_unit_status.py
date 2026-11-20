# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Portable refusal/CLI statuses with owned minimal state; no native attribution."""
import importlib.util
import io
import json
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('status_unit_results',
                                             Path(__file__).parents[1] / 'unit_results.py')
UNIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(UNIT)


CHILD = r'''
import ast
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

path, case, root = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
spec = importlib.util.spec_from_file_location('controlled_unit_cli', path)
value = importlib.util.module_from_spec(spec)
spec.loader.exec_module(value)
primary = {'zero': SystemExit(0), 'none': SystemExit(None), 'false': SystemExit(False),
           'wrapped': SystemExit(256), 'prepare_zero': SystemExit(0), 'nonzero': SystemExit(47),
           'keyboard': KeyboardInterrupt('controlled operation interruption'),
           'ordinary': ValueError('controlled input refusal'),
           'interrupted143': value.Refusal('controlled process interruption', 143),
           'report_zero': value.Refusal('controlled process interruption', 143),
           'report_wrapped': ValueError('controlled input refusal')}.get(case)
witness = {'operation_called': False, 'operation_returned': False,
           'controlled_dispatch_only': True}

def operation(arguments):
    witness['operation_called'] = True
    if primary is not None:
        raise primary
    witness['operation_returned'] = True

value.collect = operation
value.prepare = operation
output = root / 'controlled-output.json'
if case == 'help':
    sys.argv = [str(path), '--help']
elif case == 'prepare_zero':
    sys.argv = [str(path), 'prepare', '--repo', str(root), '--target-dir', str(root),
                '--metadata', str(root / 'unused-metadata'), '--artifacts', str(root / 'unused-artifacts'),
                '--state', str(root / 'unused-state')]
else:
    sys.argv = [str(path), 'collect', '--state', str(root), '--log', str(root / 'unused-log'),
                '--out', str(output), '--status', '0']
if case.startswith('report_'):
    class FailingStderr:
        def write(self, text):
            raise SystemExit(256 if case == 'report_wrapped' else 0)
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
    witness['output_exists'] = output.exists()
    (root / (case + '-witness.json')).write_text(json.dumps(witness))
'''


class FailingSink(io.StringIO):
    def __init__(self, fault):
        super().__init__()
        self.fault = fault

    def write(self, _text):
        raise self.fault


class UnitStatusTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='alab-unit-status-test-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.state = self.root / 'state'
        self.state.mkdir()
        (self.state / 'state.json').write_text('{}\n')
        self.log = self.root / 'empty.log'
        self.log.write_text('')
        self.child_observations = []

    def output(self, status):
        return self.root / ('result-' + str(status) + '.json')

    def arguments(self, status=0, command='collect'):
        if command == 'help':
            return ['unit_results', '--help']
        if command == 'prepare':
            return ['unit_results', 'prepare', '--repo', str(self.root), '--target-dir', str(self.root),
                    '--metadata', str(self.state / 'state.json'), '--artifacts', str(self.log),
                    '--state', str(self.state)]
        return ['unit_results', 'collect', '--state', str(self.state), '--log', str(self.log),
                '--out', str(self.output(status)), '--status', str(status)]

    def api(self, status):
        return UNIT.collect(SimpleNamespace(state=str(self.state), log=str(self.log),
                                             out=str(self.output(status)), status=status, sequential_output=False))

    def natural_child(self, status):
        argv = [sys.executable, '-I', '-B', str(Path(UNIT.__file__)), *self.arguments(status)[1:]]
        done = subprocess.run(argv, cwd=self.root, capture_output=True, text=True, timeout=10)
        self.child_observations.append(dict(case='natural-' + str(status), actual_exit=done.returncode,
                                            output_exists=self.output(status).exists(),
                                            stdout=done.stdout, stderr=done.stderr, argv=argv))
        return done

    def main(self, operation, command='collect', sink=None):
        output, error = io.StringIO(), io.StringIO() if sink is None else sink
        with patch.object(sys, 'argv', self.arguments(command=command)), \
                patch.object(UNIT, 'prepare' if command == 'prepare' else 'collect', side_effect=operation), \
                redirect_stdout(output), redirect_stderr(error):
            result = UNIT.main()
        return result, output.getvalue(), error.getvalue()

    def child(self, case):
        script = self.root / 'controlled-entry.py'
        script.write_text(CHILD)
        done = subprocess.run([sys.executable, '-I', '-B', str(script), str(Path(UNIT.__file__)),
                               case, str(self.root)], cwd=self.root,
                              capture_output=True, text=True, timeout=10)
        witness = json.loads((self.root / (case + '-witness.json')).read_text())
        self.child_observations.append(dict(case=case, actual_exit=done.returncode, witness=witness,
                                            stdout=done.stdout, stderr=done.stderr))
        return done, witness

    def test_natural_nonportable_interruption_refusals_have_portable_api_status(self):
        for status in (256, 512):
            with self.subTest(status=status):
                with self.assertRaises(UNIT.Refusal) as caught:
                    self.api(status)
                self.assertEqual(caught.exception.status, 1)
                self.assertIs(type(caught.exception.status), int)
                self.assertFalse(self.output(status).exists())

    def test_natural_nonportable_cli_refusals_never_exit_successfully(self):
        for status in (256, 512):
            with self.subTest(status=status):
                done = self.natural_child(status)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assertIn('unit-test process was interrupted', done.stderr)
                self.assertFalse(self.output(status).exists())

    def test_natural_portable_interruption_statuses_are_preserved(self):
        for status in (143, 255):
            with self.subTest(status=status):
                with self.assertRaises(UNIT.Refusal) as caught:
                    self.api(status)
                self.assertEqual(caught.exception.status, status)
                done = self.natural_child(status)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assertFalse(self.output(status).exists())

    def test_negative_status_and_unprepared_state_never_qualify_results(self):
        for status in (-2, 0, 101):
            with self.subTest(status=status):
                done = self.natural_child(status)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assertIn('unit-test attribution refused:', done.stderr)
                self.assertFalse(self.output(status).exists())

    def test_nonportable_refusal_payloads_return_integer_failure(self):
        for status in (0, None, False, True, 143.0, '143', -2, 256, 512):
            with self.subTest(status=status, scalar_type=type(status).__name__):
                returned, _output, _error = self.main(UNIT.Refusal('controlled refusal', status))
                self.assertEqual(returned, 1)
                self.assertIs(type(returned), int)

    def test_main_refuses_success_shaped_interruptions_after_argument_parsing(self):
        for code in (0, None, False, 256):
            with self.subTest(code=code):
                returned, _output, error = self.main(SystemExit(code))
                self.assertEqual(returned, 1)
                self.assertIn('incomplete', error.lower())
        returned, _output, _error = self.main(SystemExit(0), command='prepare')
        self.assertEqual(returned, 1)

    def test_public_collect_preserves_exact_cancellation_before_metadata_read(self):
        for primary in (SystemExit(0), SystemExit(None), SystemExit(47), KeyboardInterrupt('metadata')):
            with self.subTest(cancellation=type(primary).__name__, code=getattr(primary, 'code', None)):
                with patch.object(UNIT, 'load', side_effect=primary), \
                        self.assertRaises(BaseException) as caught:
                    self.api(0)
                self.assertTrue(caught.exception is primary)
                self.assertFalse(self.output(0).exists())

    def test_main_retains_nonzero_cancellation_help_and_controlled_dispatch(self):
        for primary in (SystemExit(47), KeyboardInterrupt('dispatch')):
            with self.assertRaises(BaseException) as caught:
                self.main(primary)
            self.assertTrue(caught.exception is primary)
        with self.assertRaises(SystemExit) as caught:
            self.main(AssertionError('help must not dispatch'), command='help')
        self.assertEqual(caught.exception.code, 0)
        returned, _output, _error = self.main(lambda _arguments: None)
        self.assertEqual(returned, 0)  # Controlled dispatch return, not an attributed test result.
        self.assertFalse(self.output(0).exists())

    def test_reporting_faults_preserve_failure_status_and_new_cancellation(self):
        for primary, status in ((ValueError('controlled refusal'), 1),
                                (UNIT.Refusal('controlled interruption', 143), 143)):
            for report_fault in (RuntimeError('sink'), SystemExit(0), SystemExit(256)):
                with self.subTest(status=status, reporting_fault=type(report_fault).__name__):
                    returned, _output, _error = self.main(primary, sink=FailingSink(report_fault))
                    self.assertEqual(returned, status)
        for primary in (SystemExit(47), KeyboardInterrupt('reporting')):
            with self.assertRaises(BaseException) as caught:
                self.main(ValueError('controlled refusal'), sink=FailingSink(primary))
            self.assertTrue(caught.exception is primary)

    def test_original_entry_controlled_operations_preserve_failure_and_help_semantics(self):
        cases = (('zero', 1), ('none', 1), ('false', 1), ('wrapped', 1), ('prepare_zero', 1),
                 ('nonzero', 47), ('keyboard', -2), ('help', 0), ('ordinary', 1),
                 ('interrupted143', 143), ('report_zero', 143), ('report_wrapped', 1), ('stub_completed', 0))
        for case, status in cases:
            with self.subTest(case=case):
                done, witness = self.child(case)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assertFalse(witness['output_exists'])
                if case == 'help':
                    self.assertFalse(witness['operation_called'])
                elif case == 'stub_completed':
                    self.assertTrue(witness['operation_returned'])
                else:
                    self.assertTrue(witness['operation_called'])
                    self.assertFalse(witness['operation_returned'])
                    if case in ('nonzero', 'keyboard'):
                        self.assertTrue(witness['escaped_primary_identity'])


if __name__ == '__main__':
    unittest.main()
