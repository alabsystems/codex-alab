# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Weekly/quota CLI cancellation accounting; inert cases run no native binary."""
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


SMOKE_DIRECTORY = Path(__file__).resolve().parents[1] / 'smoke'
sys.path.insert(0, str(SMOKE_DIRECTORY))


def load(name):
    spec = importlib.util.spec_from_file_location('cli_status_' + name,
                                                 SMOKE_DIRECTORY / (name + '.py'))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


SMOKES = {name: load(name) for name in ('weekly_reset', 'quota_wait')}


CHILD = r'''
import ast
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

path, consumer, scenario, root = Path(sys.argv[1]), sys.argv[2], sys.argv[3], Path(sys.argv[4])
sys.path.insert(0, str(path.parent))
spec = importlib.util.spec_from_file_location('controlled_' + consumer, path)
value = importlib.util.module_from_spec(spec)
spec.loader.exec_module(value)
faults = {'zero': SystemExit(0), 'none': SystemExit(None), 'false': SystemExit(False),
          'wrapped': SystemExit(256), 'negative_wrapped': SystemExit(-256),
          'nonzero': SystemExit(47), 'keyboard': KeyboardInterrupt('controlled case interruption'),
          'floating_zero': SystemExit(0.0), 'huge': SystemExit(sys.maxsize + 1),
          'ordinary': OSError('controlled reporting failure')}
primary = faults.get(scenario)
if scenario.startswith('report_'):
    primary = SystemExit(0)
report_fault = None
binary = root / 'inert-codex'
binary.write_bytes(b'owned fixture bytes; never executed as a program\n')
original_binary = hashlib.sha256(binary.read_bytes()).hexdigest()
output = root / 'report'
witness = {'called': [], 'returned': [], 'main_called': False, 'main_returned': False,
           'controlled_cases_only': True}


def controlled_case(_binary, destination, identity):
    name = (identity if consumer == 'weekly_reset' else
            'cancel-distant-reset' if identity else 'repeated-stale-reset')
    witness['called'].append(name)
    (destination / name).mkdir()
    if len(witness['called']) == 2 and primary is not None:
        raise primary
    witness['returned'].append(name)
    return {'case': name, 'passed': not (scenario == 'refusal' and len(witness['called']) == 2),
            'controlled_fixture': True}


actual_main = value.main


def observed_main(*arguments, **keywords):
    witness['main_called'] = True
    try:
        result = actual_main(*arguments, **keywords)
    except BaseException as error:
        witness['main_exception_is_primary'] = error is primary
        raise
    witness['main_returned'] = True
    witness['main_result'] = result
    return result


value.case = controlled_case
value.main = observed_main
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
normal = [str(path), '--codex', str(binary), '--output', str(output)]
if consumer == 'weekly_reset':
    normal += ['--case', 'late-credit', '--case', 'lost-ack']
if scenario == 'help':
    sys.argv = [str(path), '--help']
elif scenario == 'arguments':
    sys.argv = [str(path)]
elif scenario == 'duplicate':
    assert consumer == 'weekly_reset'
    sys.argv = normal[:-1] + ['late-credit']
else:
    sys.argv = normal
tree = ast.parse(path.read_bytes(), filename=str(path))
entries = [node for node in tree.body if isinstance(node, ast.If)
           and any(isinstance(test, ast.Compare) and isinstance(test.left, ast.Name)
                   and test.left.id == '__name__' for test in ast.walk(node.test))]
# Weekly's isolated exec wrapper is a compound early guard. Keep that guard as
# well as the final entry, with --exec-wrapper deliberately inactive here.
assert len(entries) == (2 if consumer == 'weekly_reset' else 1)
entry = ast.Module(body=entries, type_ignores=[])
witness['original_entry_guards'] = len(entries)
witness['entry_ast_sha256'] = hashlib.sha256(ast.dump(entry).encode()).hexdigest()
value.__name__ = '__main__'
try:
    exec(compile(entry, str(path), 'exec'), value.__dict__)
except BaseException as error:
    witness['escaped_type'] = type(error).__name__
    witness['escaped_code_repr'] = repr(error.code) if isinstance(error, SystemExit) else None
    witness['escaped_primary_identity'] = error is primary
    witness['escaped_report_fault_identity'] = error is report_fault
    raise
finally:
    witness['output_exists'] = output.exists()
    witness['report'] = (json.loads((output / 'result.json').read_text())
                         if (output / 'result.json').is_file() else None)
    witness['binary_unchanged'] = hashlib.sha256(binary.read_bytes()).hexdigest() == original_binary
    (root / 'witness.json').write_text(json.dumps(witness))
'''


class FailingSink(io.StringIO):
    def __init__(self, fault):
        super().__init__()
        self.fault = fault

    def write(self, _text):
        raise self.fault


class SmokeCliStatusTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='alab-smoke-cli-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name).resolve()
        self.child_observations = []

    def expected_cases(self, consumer):
        return (['late-credit', 'lost-ack'] if consumer == 'weekly_reset' else
                ['repeated-stale-reset', 'cancel-distant-reset'])

    def child(self, consumer, scenario):
        root = self.root / (consumer + '-' + scenario)
        root.mkdir()
        script = root / 'original-entry-child.py'
        script.write_text(CHILD)
        done = subprocess.run([sys.executable, '-I', '-B', str(script),
                               str(SMOKE_DIRECTORY / (consumer + '.py')), consumer, scenario, str(root)],
                              cwd=root, capture_output=True, text=True, timeout=10)
        witness = json.loads((root / 'witness.json').read_text())
        self.child_observations.append({'consumer': consumer, 'case': scenario,
                                        'actual_exit': done.returncode, 'stdout': done.stdout,
                                        'stderr': done.stderr, 'witness': witness})
        self.assertTrue(witness['binary_unchanged'])
        self.assertTrue(witness['controlled_cases_only'])
        self.assertEqual(witness['original_entry_guards'], 2 if consumer == 'weekly_reset' else 1)
        return done, witness

    def assert_partial_report(self, consumer, witness):
        names = self.expected_cases(consumer)
        self.assertEqual(witness['called'], names)
        self.assertEqual(witness['returned'], names[:1])
        report = witness['report']
        self.assertFalse(report['passed'])
        self.assertTrue(report['interrupted'])
        self.assertEqual(report['failedCase' if consumer == 'weekly_reset' else 'activeCase'], names[1])
        self.assertEqual(report['cases'], [{'case': names[0], 'passed': True,
                                          'controlled_fixture': True}])

    def api_interruption(self, consumer, label, primary, sink=None):
        root = self.root / (consumer + '-' + label)
        root.mkdir()
        binary = root / 'inert-codex'
        binary.write_bytes(b'owned API fixture bytes; never executed\n')
        output = root / 'report'
        called, returned = [], []

        def controlled_case(_binary, destination, identity):
            name = (identity if consumer == 'weekly_reset' else
                    'cancel-distant-reset' if identity else 'repeated-stale-reset')
            called.append(name)
            (destination / name).mkdir()
            if len(called) == 2:
                raise primary
            returned.append(name)
            return {'case': name, 'passed': True, 'controlled_fixture': True}

        arguments = [str(SMOKE_DIRECTORY / (consumer + '.py')), '--codex', str(binary),
                     '--output', str(output)]
        if consumer == 'weekly_reset':
            arguments += ['--case', 'late-credit', '--case', 'lost-ack']
        with patch.object(sys, 'argv', arguments), patch.object(SMOKES[consumer], 'case', controlled_case), \
                redirect_stdout(io.StringIO() if sink is None else sink), redirect_stderr(io.StringIO()):
            with self.assertRaises(BaseException) as caught:
                SMOKES[consumer].main()
        self.assertIs(caught.exception, primary)
        self.assertEqual(binary.read_bytes(), b'owned API fixture bytes; never executed\n')
        self.assert_partial_report(consumer, {'called': called, 'returned': returned,
                                             'report': json.loads((output / 'result.json').read_text())})

    def test_api_main_retains_success_shaped_cancellation_identity(self):
        for consumer in SMOKES:
            for index, code in enumerate((0, None, False, 256, -256)):
                with self.subTest(consumer=consumer, code=code):
                    self.api_interruption(consumer, 'api-zero-' + str(index), SystemExit(code))

    def test_api_main_retains_nonzero_float_huge_and_keyboard_identity(self):
        for consumer in SMOKES:
            for index, primary in enumerate((SystemExit(47), SystemExit(0.0),
                                             SystemExit(sys.maxsize + 1), KeyboardInterrupt('case'))):
                with self.subTest(consumer=consumer, primary=repr(primary)):
                    self.api_interruption(consumer, 'api-preserved-' + str(index), primary)

    def test_api_summary_output_fault_cannot_replace_pending_cancellation(self):
        for consumer in SMOKES:
            for index, fault in enumerate((SystemExit(0), SystemExit(256), BrokenPipeError('output'))):
                with self.subTest(consumer=consumer, fault=repr(fault)):
                    self.api_interruption(consumer, 'api-output-' + str(index),
                                          SystemExit(0), FailingSink(fault))

    def test_original_entry_refuses_incomplete_success_shaped_case_exits(self):
        for consumer in SMOKES:
            for scenario in ('zero', 'none', 'false', 'wrapped', 'negative_wrapped'):
                with self.subTest(consumer=consumer, scenario=scenario):
                    done, witness = self.child(consumer, scenario)
                    self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                    self.assertTrue(witness['main_called'])
                    self.assertFalse(witness['main_returned'])
                    self.assertTrue(witness['main_exception_is_primary'])
                    self.assert_partial_report(consumer, witness)
                    self.assertFalse(witness['escaped_primary_identity'])
                    self.assertIn('incomplete', done.stderr.lower())

    def test_original_entry_preserves_genuine_failure_exit_identity(self):
        for consumer in SMOKES:
            for scenario, status in (('nonzero', 47), ('keyboard', -2),
                                     ('floating_zero', 1), ('huge', 255)):
                with self.subTest(consumer=consumer, scenario=scenario):
                    done, witness = self.child(consumer, scenario)
                    self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                    self.assertTrue(witness['main_exception_is_primary'])
                    self.assertTrue(witness['escaped_primary_identity'])
                    self.assert_partial_report(consumer, witness)

    def test_help_arguments_and_duplicate_weekly_selection_stay_before_work(self):
        controls = [(consumer, scenario, status) for consumer in SMOKES
                    for scenario, status in (('help', 0), ('arguments', 2))]
        controls.append(('weekly_reset', 'duplicate', 2))
        for consumer, scenario, status in controls:
            with self.subTest(consumer=consumer, scenario=scenario):
                done, witness = self.child(consumer, scenario)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assertEqual(witness['called'], [])
                self.assertEqual(witness['returned'], [])
                self.assertFalse(witness['output_exists'])
                self.assertIsNone(witness['report'])

    def test_controlled_healthy_and_returned_refusal_keep_their_statuses(self):
        for consumer in SMOKES:
            for scenario, status in (('healthy', 0), ('refusal', 1)):
                with self.subTest(consumer=consumer, scenario=scenario):
                    done, witness = self.child(consumer, scenario)
                    self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                    self.assertTrue(witness['main_returned'])
                    self.assertEqual(witness['main_result'], status)
                    self.assertEqual(witness['called'], self.expected_cases(consumer))
                    self.assertEqual(witness['returned'], self.expected_cases(consumer))
                    self.assertEqual(witness['report']['passed'], scenario == 'healthy')
                    self.assertNotIn('interrupted', witness['report'])

    def test_diagnostic_failure_cannot_make_incomplete_operation_successful(self):
        for consumer in SMOKES:
            for scenario in ('report_zero', 'report_wrapped', 'report_ordinary'):
                with self.subTest(consumer=consumer, scenario=scenario):
                    done, witness = self.child(consumer, scenario)
                    self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                    self.assert_partial_report(consumer, witness)

    def test_new_nonzero_reporting_cancellation_retains_exact_identity(self):
        for consumer in SMOKES:
            for scenario, status in (('report_nonzero', 47), ('report_keyboard', -2)):
                with self.subTest(consumer=consumer, scenario=scenario):
                    done, witness = self.child(consumer, scenario)
                    self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                    self.assertTrue(witness['escaped_report_fault_identity'])
                    self.assert_partial_report(consumer, witness)

    def test_success_shaped_summary_output_failure_cannot_exit_zero(self):
        for consumer in SMOKES:
            for scenario in ('output_zero', 'output_wrapped'):
                with self.subTest(consumer=consumer, scenario=scenario):
                    done, witness = self.child(consumer, scenario)
                    self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                    self.assertTrue(witness['main_called'])
                    self.assertFalse(witness['main_returned'])
                    self.assertFalse(witness['escaped_report_fault_identity'])
                    self.assertEqual(witness['called'], self.expected_cases(consumer))
                    self.assertEqual(witness['returned'], self.expected_cases(consumer))
                    self.assertTrue(witness['report']['passed'])


if __name__ == '__main__':
    unittest.main()
