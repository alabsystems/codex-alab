# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""CLI cancellation accounting with inert cases; no Codex or provider is run."""
import importlib.util
import io
import json
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[1] / 'smoke/request_recovery.py'
sys.path.insert(0, str(SOURCE.parent))
SPEC = importlib.util.spec_from_file_location('request_recovery_cli_status', SOURCE)
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)


CHILD = r'''
import ast
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

path, scenario, root = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
sys.path.insert(0, str(path.parent))
spec = importlib.util.spec_from_file_location('controlled_request_recovery_cli', path)
value = importlib.util.module_from_spec(spec)
spec.loader.exec_module(value)
faults = {'zero': SystemExit(0), 'none': SystemExit(None), 'false': SystemExit(False),
          'wrapped': SystemExit(256), 'negative_wrapped': SystemExit(-256),
          'twice_wrapped': SystemExit(512), 'nonzero': SystemExit(47),
          'floating_zero': SystemExit(0.0), 'huge': SystemExit(sys.maxsize + 1),
          'keyboard': KeyboardInterrupt('controlled case interruption')}
primary = faults.get(scenario)
if scenario.startswith(('report_', 'output_')):
    primary = SystemExit(0)
if scenario == 'ordinary':
    primary = RuntimeError('controlled case refusal')
report_fault = None
binary = root / 'unexecuted-codex-fixture'
binary.write_bytes(b'owned data, never executed as a program\n')
binary_before = hashlib.sha256(binary.read_bytes()).hexdigest()
output = root / 'output'
witness = {'called': [], 'returned': [], 'controlled_cases_only': True}

def controlled_case(_codex, destination, name, _status, _envelope, _recover):
    witness['called'].append(name)
    (destination / name).mkdir()
    if len(witness['called']) == 2 and primary is not None:
        raise primary
    witness['returned'].append(name)
    return {'case': name, 'passed': not (scenario == 'returned_refusal'
                                        and len(witness['called']) == 2)}

value.case = controlled_case
if scenario.startswith(('report_', 'output_')):
    report_fault = faults[scenario.split('_', 1)[1]]

    class FailingSink:
        def write(self, _text):
            raise report_fault

        def flush(self):
            pass

    if scenario.startswith('report_'):
        sys.stderr = FailingSink()
    else:
        sys.stdout = FailingSink()
sys.argv = ([str(path), '--help'] if scenario == 'help' else
            [str(path)] if scenario == 'arguments' else
            [str(path), '--codex', str(binary), '--output', str(output)])
tree = ast.parse(path.read_bytes(), filename=str(path))
entries = [node for node in tree.body if isinstance(node, ast.If)
           and isinstance(node.test, ast.Compare) and isinstance(node.test.left, ast.Name)
           and node.test.left.id == '__name__']
assert len(entries) == 1
entry = ast.Module(body=entries, type_ignores=[])
witness['entry_ast_sha256'] = hashlib.sha256(ast.dump(entry).encode()).hexdigest()
value.__name__ = '__main__'
try:
    exec(compile(entry, str(path), 'exec'), value.__dict__)
except BaseException as error:
    witness['escaped_type'] = type(error).__name__
    witness['escaped_code'] = error.code if isinstance(error, SystemExit) else None
    witness['escaped_primary_identity'] = error is primary
    witness['escaped_report_fault_identity'] = error is report_fault
    raise
finally:
    witness['output_exists'] = output.exists()
    witness['report'] = (json.loads((output / 'result.json').read_text())
                         if (output / 'result.json').is_file() else None)
    witness['binary_unchanged'] = hashlib.sha256(binary.read_bytes()).hexdigest() == binary_before
    (root / 'witness.json').write_text(json.dumps(witness))
'''


class FailingSink(io.StringIO):
    def __init__(self, fault):
        super().__init__()
        self.fault = fault

    def write(self, _text):
        raise self.fault


class RequestRecoveryCliStatusTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='alab-request-recovery-cli-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name).resolve()
        self.child_observations = []

    def child(self, scenario):
        root = self.root / scenario
        root.mkdir()
        script = root / 'original-entry-child.py'
        script.write_text(CHILD)
        done = subprocess.run([sys.executable, '-I', '-B', str(script), str(SOURCE), scenario, str(root)],
                              cwd=root, capture_output=True, text=True, timeout=10)
        witness = json.loads((root / 'witness.json').read_text())
        self.child_observations.append({'case': scenario, 'actual_exit': done.returncode,
                                        'stdout': done.stdout, 'stderr': done.stderr,
                                        'witness': witness})
        self.assertTrue(witness['binary_unchanged'])
        self.assertTrue(witness['controlled_cases_only'])
        self.assertEqual(len(witness['entry_ast_sha256']), 64)
        return done, witness

    def assert_partial_report(self, witness):
        names = [item[0] for item in SMOKE.CASES]
        self.assertEqual(witness['called'], names[:2])
        self.assertEqual(witness['returned'], names[:1])
        report = witness['report']
        self.assertFalse(report['passed'])
        self.assertTrue(report['interrupted'])
        self.assertEqual(report['activeCase'], names[1])
        self.assertEqual(report['cases'], [{'case': names[0], 'passed': True}])

    def api_interruption(self, label, primary, sink=None):
        root = self.root / label
        root.mkdir()
        binary = root / 'unexecuted-codex-fixture'
        binary.write_bytes(b'owned API fixture; never executed\n')
        output = root / 'output'
        called, returned = [], []

        def controlled_case(_codex, destination, name, _status, _envelope, _recover):
            called.append(name)
            (destination / name).mkdir()
            if len(called) == 2:
                raise primary
            returned.append(name)
            return {'case': name, 'passed': True}

        with patch.object(sys, 'argv', [str(SOURCE), '--codex', str(binary), '--output', str(output)]), \
                patch.object(SMOKE, 'case', controlled_case), \
                redirect_stdout(io.StringIO() if sink is None else sink), redirect_stderr(io.StringIO()):
            with self.assertRaises(BaseException) as caught:
                SMOKE.main()
        self.assertIs(caught.exception, primary)
        self.assertEqual(binary.read_bytes(), b'owned API fixture; never executed\n')
        self.assert_partial_report({'called': called, 'returned': returned,
                                    'report': json.loads((output / 'result.json').read_text())})

    def test_api_main_keeps_exact_success_shaped_cancellation_identity(self):
        for index, code in enumerate((0, None, False, 256, -256, 512)):
            with self.subTest(code=code):
                self.api_interruption('api-success-shaped-' + str(index), SystemExit(code))

    def test_api_main_keeps_nonzero_float_huge_and_keyboard_identity(self):
        for index, primary in enumerate((SystemExit(47), SystemExit(0.0),
                                         SystemExit(sys.maxsize + 1), KeyboardInterrupt('case'))):
            with self.subTest(primary=repr(primary)):
                self.api_interruption('api-preserved-' + str(index), primary)

    def test_api_partial_report_output_fault_does_not_replace_pending_cancellation(self):
        for index, fault in enumerate((SystemExit(0), SystemExit(256), BrokenPipeError('controlled output'))):
            with self.subTest(fault=repr(fault)):
                self.api_interruption('api-output-' + str(index), SystemExit(0), FailingSink(fault))

    def test_original_entry_refuses_success_shaped_case_interruptions(self):
        for scenario in ('zero', 'none', 'false', 'wrapped', 'negative_wrapped', 'twice_wrapped'):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assert_partial_report(witness)
                self.assertFalse(witness['escaped_primary_identity'])
                self.assertIn('incomplete', done.stderr.lower())

    def test_original_entry_preserves_nonzero_and_keyboard_interruptions(self):
        for scenario, status in (('nonzero', 47), ('keyboard', -2), ('floating_zero', 1), ('huge', 255)):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assert_partial_report(witness)
                self.assertTrue(witness['escaped_primary_identity'])

    def test_help_and_argument_errors_do_not_start_cases_or_create_output(self):
        for scenario, status in (('help', 0), ('arguments', 2)):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assertEqual(witness['called'], [])
                self.assertEqual(witness['returned'], [])
                self.assertFalse(witness['output_exists'])
                self.assertIsNone(witness['report'])

    def test_all_four_controlled_cases_remain_successful(self):
        done, witness = self.child('healthy')
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        names = [item[0] for item in SMOKE.CASES]
        self.assertEqual(witness['called'], names)
        self.assertEqual(witness['returned'], names)
        self.assertTrue(witness['report']['passed'])
        self.assertNotIn('interrupted', witness['report'])
        self.assertEqual([item['case'] for item in witness['report']['cases']], names)
        self.assertTrue(all(item['passed'] for item in witness['report']['cases']))

    def test_returned_and_ordinary_case_refusals_remain_nonzero(self):
        for scenario in ('returned_refusal', 'ordinary'):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assertEqual(witness['called'], [item[0] for item in SMOKE.CASES])
                self.assertFalse(witness['report']['passed'])
                self.assertNotIn('interrupted', witness['report'])
                self.assertFalse(witness['report']['cases'][1]['passed'])

    def test_success_shaped_diagnostic_and_output_faults_cannot_restore_success(self):
        for scenario in ('report_zero', 'report_none', 'report_wrapped', 'report_negative_wrapped',
                         'output_zero', 'output_wrapped'):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assert_partial_report(witness)
                self.assertFalse(witness['escaped_primary_identity'])

    def test_nonzero_reporting_interruptions_keep_exact_new_identity(self):
        for scenario, status in (('report_nonzero', 47), ('report_keyboard', -2)):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assert_partial_report(witness)
                self.assertTrue(witness['escaped_report_fault_identity'])


if __name__ == '__main__':
    unittest.main()
