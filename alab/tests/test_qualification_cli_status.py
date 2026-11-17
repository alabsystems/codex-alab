# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""An incomplete qualification cannot exit successfully through CLI cancellation."""

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    'qualification_cli_status', Path(__file__).resolve().parents[1] / 'qualification.py')
QUAL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(QUAL)


CHILD = r'''
import ast
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

path, case, witness_path = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
spec = importlib.util.spec_from_file_location('controlled_qualification', path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
faults = {'zero': SystemExit(0), 'none': SystemExit(None), 'false': SystemExit(False),
          'wrapped': SystemExit(256), 'negative_wrapped': SystemExit(-256),
          'nonzero': SystemExit(47), 'floating_zero': SystemExit(0.0),
          'huge': SystemExit(sys.maxsize + 1),
          'keyboard': KeyboardInterrupt('controlled qualification cancellation')}
primary = faults.get(case)
report_fault = None
if case == 'ordinary' or case.startswith('report_'):
    primary = OSError(5, 'controlled qualification failure')
if case.startswith('report_'):
    report_fault = faults[case.removeprefix('report_')]

    class FailingStderr:
        def write(self, _text):
            raise report_fault

        def flush(self):
            pass

    sys.stderr = FailingStderr()
if case == 'format_wrapped':
    report_fault = SystemExit(256)

    class FormatFailure(OSError):
        def __str__(self):
            raise report_fault

    primary = FormatFailure('controlled qualification formatting failure')
witness = {'run_called': False, 'run_returned': False}

def controlled_run(*arguments, **keywords):
    witness['run_called'] = True
    if primary is not None:
        raise primary
    witness['run_returned'] = True
    return 7 if case == 'returned_failure' else 0

module.run = controlled_run
output = witness_path.parent / 'unstarted-qualification'
sys.argv = [str(path), '--help'] if case == 'help' else [
    str(path), '--repo', str(witness_path.parent), '--directory', str(output)]
tree = ast.parse(path.read_bytes(), filename=str(path))
entries = [node for node in tree.body if isinstance(node, ast.If)
           and isinstance(node.test, ast.Compare) and isinstance(node.test.left, ast.Name)
           and node.test.left.id == '__name__']
# The first guard sets dont_write_bytecode; the final guard invokes main.
# Execute both original guards so selecting only the first cannot appear healthy.
assert len(entries) == 2
entry = ast.Module(body=entries, type_ignores=[])
witness['original_entry_guards'] = len(entries)
witness['entry_ast_sha256'] = hashlib.sha256(ast.dump(entry).encode()).hexdigest()
witness['python_version'] = sys.version
module.__name__ = '__main__'
try:
    exec(compile(entry, str(path), 'exec'), module.__dict__)
except BaseException as error:
    witness['escaped_type'] = type(error).__name__
    witness['escaped_code'] = error.code if isinstance(error, SystemExit) else None
    witness['escaped_primary_identity'] = error is primary
    witness['escaped_report_identity'] = error is report_fault
    raise
finally:
    witness['qualification_output_exists'] = output.exists()
    witness_path.write_text(json.dumps(witness))
'''


class QualificationCliStatusTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix='alab-qualification-cli-status-')
        self.addCleanup(scratch.cleanup)
        self.directory = Path(scratch.name)
        self.child_observations = []

    def child(self, case):
        script = self.directory / 'child.py'
        script.write_text(CHILD)
        witness_path = self.directory / (case + '.json')
        done = subprocess.run([
            sys.executable, '-I', '-B', str(script), str(Path(QUAL.__file__)),
            case, str(witness_path),
        ], cwd=self.directory, capture_output=True, text=True, timeout=10)
        witness = json.loads(witness_path.read_text())
        self.child_observations.append(dict(
            case=case, actual_exit=done.returncode, witness=witness,
            stdout=done.stdout, stderr=done.stderr))
        self.assertEqual(witness['original_entry_guards'], 2)
        self.assertEqual(len(witness['entry_ast_sha256']), 64)
        self.assertFalse(witness['qualification_output_exists'])
        self.assertEqual(witness['run_called'], case != 'help')
        self.assertEqual(witness['run_returned'], case in ('success', 'returned_failure'))
        return done, witness

    def test_wrapped_operation_exits_refuse_incomplete_qualification(self):
        for case in ('wrapped', 'negative_wrapped'):
            with self.subTest(case=case):
                done, witness = self.child(case)
                self.assertEqual(done.returncode, 1)
                self.assertFalse(witness['escaped_primary_identity'])
                self.assertIn('qualification observation incomplete:', done.stderr)

    def test_existing_success_shaped_cancellation_remains_failure(self):
        for case in ('zero', 'none', 'false'):
            with self.subTest(case=case):
                done, witness = self.child(case)
                self.assertEqual(done.returncode, 1)
                self.assertFalse(witness['escaped_primary_identity'])
                self.assertIn('qualification observation incomplete:', done.stderr)

    def test_wrapped_reporting_exits_cannot_replace_failed_status(self):
        for case in ('report_wrapped', 'report_negative_wrapped', 'format_wrapped'):
            with self.subTest(case=case):
                done, witness = self.child(case)
                self.assertEqual(done.returncode, 1)
                self.assertFalse(witness['escaped_report_identity'])

    def test_existing_success_shaped_reporting_exit_remains_failure(self):
        for case in ('report_zero', 'report_none', 'report_false'):
            with self.subTest(case=case):
                done, witness = self.child(case)
                self.assertEqual(done.returncode, 1)
                self.assertFalse(witness['escaped_report_identity'])

    def test_nonzero_operation_and_keyboard_cancellation_keep_identity(self):
        for case, status in (('nonzero', 47), ('keyboard', -2)):
            with self.subTest(case=case):
                done, witness = self.child(case)
                self.assertEqual(done.returncode, status)
                self.assertTrue(witness['escaped_primary_identity'])

    def test_float_and_overflow_operation_payloads_keep_identity(self):
        for case, status in (('floating_zero', 1), ('huge', 255)):
            with self.subTest(case=case):
                done, witness = self.child(case)
                self.assertEqual(done.returncode, status)
                self.assertTrue(witness['escaped_primary_identity'])

    def test_nonzero_reporting_and_keyboard_cancellation_keep_identity(self):
        for case, status in (('report_nonzero', 47), ('report_keyboard', -2),
                             ('report_floating_zero', 1), ('report_huge', 255)):
            with self.subTest(case=case):
                done, witness = self.child(case)
                self.assertEqual(done.returncode, status)
                self.assertTrue(witness['escaped_report_identity'])

    def test_healthy_help_and_returned_failure_statuses_are_unchanged(self):
        for case, status in (('success', 0), ('help', 0), ('returned_failure', 7),
                             ('ordinary', 1)):
            with self.subTest(case=case):
                done, _witness = self.child(case)
                self.assertEqual(done.returncode, status)
                if case == 'help':
                    self.assertIn('usage:', done.stdout)
                elif case == 'ordinary':
                    self.assertEqual(done.stderr,
                                     'qualification observation: [Errno 5] controlled qualification failure\n')
                else:
                    self.assertEqual(done.stdout + done.stderr, '')


if __name__ == '__main__':
    unittest.main()
