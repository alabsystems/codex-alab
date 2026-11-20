# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Discovery interruption cannot qualify an empty claimed Python result record."""

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / 'python_test_results.py'
SPEC = importlib.util.spec_from_file_location('python_results_cli', SCRIPT)
RESULTS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RESULTS)


CHILD = r'''
import ast
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

path, case, root = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
spec = importlib.util.spec_from_file_location('controlled_python_results', path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
record_path = root / 'record.json'
report_fault = None
if case.startswith('report_'):
    report_fault = {'report_zero': SystemExit(0), 'report_none': SystemExit(None),
        'report_false': SystemExit(False), 'report_wrapped': SystemExit(256),
        'report_negative_wrapped': SystemExit(-256), 'report_nonzero': SystemExit(47),
        'report_keyboard': KeyboardInterrupt('controlled diagnostic cancellation'),
        'report_floating_zero': SystemExit(0.0), 'report_huge': SystemExit(sys.maxsize + 1),
        'report_oserror': OSError(5, 'controlled diagnostic failure'),
        'report_runtime': RuntimeError('controlled diagnostic failure')}[case]

    class FailingStderr:
        def write(self, _text):
            raise report_fault

        def flush(self):
            pass

    sys.stderr = FailingStderr()
sys.argv = [str(path), '--start-directory', str(root / 'suite'),
            '--pattern', 'test_owned.py', '--record', str(record_path)]
if case == 'help':
    sys.argv = [str(path), '--help']
elif case == 'arguments':
    sys.argv = [str(path), '--start-directory', str(root / 'suite')]
tree = ast.parse(path.read_bytes(), filename=str(path))
entries = [node for node in tree.body if isinstance(node, ast.If)
           and isinstance(node.test, ast.Compare) and isinstance(node.test.left, ast.Name)
           and node.test.left.id == '__name__']
assert len(entries) == 1
program = ast.Module(body=entries, type_ignores=[])
witness = dict(case=case, entry_ast_sha256=hashlib.sha256(ast.dump(program).encode()).hexdigest(),
               python_version=sys.version)
module.__name__ = '__main__'
try:
    # Both the old try/except guard and the new thin guard execute unmodified.
    exec(compile(program, str(path), 'exec'), module.__dict__)
except BaseException as error:
    owned = sys.modules.get('test_owned')
    witness.update(escaped_type=type(error).__name__, escaped_code=getattr(error, 'code', None),
        escaped_hook_identity=error is getattr(owned, 'INTERRUPTION', None),
        escaped_report_identity=error is report_fault)
    raise
finally:
    witness['record_exists'] = record_path.exists()
    raw = record_path.read_bytes() if record_path.exists() else b''
    witness['record_bytes'] = len(raw)
    witness['record_sha256'] = hashlib.sha256(raw).hexdigest()
    for name in ('imported', 'hook', 'body'):
        witness[name] = (root / name).read_text() if (root / name).exists() else None
    (root / 'witness.json').write_text(json.dumps(witness))
'''


class PythonResultsCliTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix='alab-python-results-cli-')
        self.addCleanup(scratch.cleanup)
        self.directory = Path(scratch.name)
        self.child_observations = []

    def child(self, case):
        root = self.directory / case
        root.mkdir()
        suite = root / 'suite'
        suite.mkdir()
        expressions = {'zero': 'SystemExit(0)', 'none': 'SystemExit(None)',
            'false': 'SystemExit(False)', 'wrapped': 'SystemExit(256)',
            'negative_wrapped': 'SystemExit(-256)', 'nonzero': 'SystemExit(47)',
            'keyboard': 'KeyboardInterrupt("owned load_tests cancellation")'}
        lines = ['import unittest', 'from pathlib import Path',
                 f'Path({str(root / "imported")!r}).write_text("1")']
        if case == 'import_zero':
            lines += ['INTERRUPTION = SystemExit(0)', 'raise INTERRUPTION']
        else:
            if case in expressions:
                lines.append('INTERRUPTION = ' + expressions[case])
            lines += ['class Owned(unittest.TestCase):', ' def test_one(self):',
                      f'  Path({str(root / "body")!r}).write_text("1")',
                      'def load_tests(loader, tests, pattern):',
                      f' Path({str(root / "hook")!r}).write_text("1")']
            if case in expressions:
                lines.append(' raise INTERRUPTION')
            elif case == 'ordinary':
                lines.append(' raise ValueError("owned load_tests failure")')
            else:
                lines.append(' return tests')
        (suite / 'test_owned.py').write_text('\n'.join(lines) + '\n')
        record = root / 'record.json'
        existing = case == 'existing' or case.startswith('report_')
        before = None
        if existing:
            record.write_text('{"exclusive": "original custody"}\n')
            value = record.stat()
            before = (value.st_dev, value.st_ino, value.st_mode, record.read_bytes())
        script = root / 'child.py'
        script.write_text(CHILD)
        done = subprocess.run([sys.executable, '-I', '-B', str(script), str(SCRIPT), case, str(root)],
                              cwd=root, capture_output=True, text=True, timeout=10)
        witness = json.loads((root / 'witness.json').read_text())
        parsed = json.loads(record.read_text()) if record.exists() and record.stat().st_size else None
        self.child_observations.append(dict(case=case, actual_exit=done.returncode,
            witness=witness, record=parsed, stdout=done.stdout, stderr=done.stderr))
        if existing:
            value = record.stat()
            self.assertEqual((value.st_dev, value.st_ino, value.st_mode, record.read_bytes()), before)
            self.assertIsNone(witness['imported'])
            self.assertIsNone(witness['hook'])
            self.assertIsNone(witness['body'])
        return done, witness, parsed

    def incomplete(self, witness, parsed):
        self.assertTrue(witness['record_exists'])
        self.assertEqual(witness['record_bytes'], 0)
        self.assertIsNone(parsed)
        self.assertEqual(witness['imported'], '1')
        self.assertEqual(witness['hook'], '1')
        self.assertIsNone(witness['body'])

    def test_exact_discovery_rejects_partial_and_ambiguous_selections_before_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            body = root / 'ran'
            fixture = ('from pathlib import Path\nimport unittest\n'
                       'class Case(unittest.TestCase):\n'
                       f' def test_selected(self): Path({str(body)!r}).write_text("yes")\n'
                       ' def test_unrelated(self): self.fail("unrelated method ran")\n')
            module = root / 'case_sample.py'
            module.write_text(fixture)
            selected = 'case_sample.Case.test_selected'
            for number, names in enumerate(([selected, 'case_sample.Case.missing'], [selected, selected],
                                             ['case_sample.Case.test_*'], [selected])):
                record = root / f'{number}.json'
                result = subprocess.run([sys.executable, '-I', '-B', str(SCRIPT), '--start-directory', str(root),
                                         '--pattern', module.name, '--record', str(record), '--tests', *names],
                                        capture_output=True, text=True)
                if names == [selected]:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(body.read_text(), 'yes')
                    rows = json.loads(record.read_bytes())['tests']
                    self.assertEqual(rows, [dict(id=selected, status='passed', finished=True)])
                else:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(record.read_bytes(), b'')
                    self.assertFalse(body.exists())

    def test_natural_success_shaped_hook_exits_fail_with_incomplete_records(self):
        for case in ('zero', 'none', 'false', 'wrapped', 'negative_wrapped'):
            with self.subTest(case=case):
                done, witness, parsed = self.child(case)
                self.incomplete(witness, parsed)
                self.assertEqual(done.returncode, 1)
                self.assertFalse(witness['escaped_hook_identity'])
                self.assertIn('operation incomplete', done.stderr)

    def test_natural_healthy_case_records_completed_real_execution(self):
        done, witness, parsed = self.child('healthy')
        self.assertEqual(done.returncode, 0)
        self.assertEqual(witness['imported'], '1')
        self.assertEqual(witness['hook'], '1')
        self.assertEqual(witness['body'], '1')
        self.assertTrue(parsed['completed'])
        self.assertEqual(parsed['tests_run'], 1)
        self.assertEqual(parsed['exit_status'], 0)
        self.assertEqual(RESULTS.passed_tests(parsed), ['test_owned.Owned.test_one'])
        self.assertIn('test_one', done.stdout)
        self.assertEqual(done.stderr, '')

    def test_natural_ordinary_hook_error_has_completed_failed_record(self):
        done, witness, parsed = self.child('ordinary')
        self.assertEqual(done.returncode, 1)
        self.assertEqual(witness['imported'], '1')
        self.assertEqual(witness['hook'], '1')
        self.assertIsNone(witness['body'])
        self.assertTrue(parsed['completed'])
        self.assertEqual(parsed['exit_status'], 1)
        self.assertEqual(parsed['tests_run'], 1)
        self.assertEqual(parsed['tests'][0]['status'], 'error')
        self.assertIn('owned load_tests failure', parsed['output'])
        with self.assertRaises(ValueError):
            RESULTS.passed_tests(parsed)

    def test_import_system_exit_is_already_a_completed_loader_error(self):
        done, witness, parsed = self.child('import_zero')
        self.assertEqual(done.returncode, 1)
        self.assertEqual(witness['imported'], '1')
        self.assertIsNone(witness['hook'])
        self.assertIsNone(witness['body'])
        self.assertTrue(parsed['completed'])
        self.assertEqual(parsed['exit_status'], 1)
        self.assertEqual(parsed['tests_run'], 1)
        self.assertEqual(parsed['tests'][0]['status'], 'error')
        self.assertIn('SystemExit: 0', parsed['output'])

    def test_natural_nonzero_hook_and_keyboard_interrupt_keep_identity(self):
        for case, expected in (('nonzero', 47), ('keyboard', -2)):
            with self.subTest(case=case):
                done, witness, parsed = self.child(case)
                self.incomplete(witness, parsed)
                self.assertEqual(done.returncode, expected)
                self.assertTrue(witness['escaped_hook_identity'])

    def test_help_and_argument_errors_do_not_claim_records_or_discover(self):
        for case, expected in (('help', 0), ('arguments', 2)):
            with self.subTest(case=case):
                done, witness, parsed = self.child(case)
                self.assertEqual(done.returncode, expected)
                self.assertFalse(witness['record_exists'])
                self.assertIsNone(parsed)
                self.assertIsNone(witness['imported'])
                self.assertIsNone(witness['hook'])
                self.assertIsNone(witness['body'])
                self.assertIn('usage:', done.stdout if case == 'help' else done.stderr)

    def test_existing_record_is_refused_before_discovery_without_replacement(self):
        done, _witness, parsed = self.child('existing')
        self.assertEqual(done.returncode, 1)
        self.assertEqual(parsed, {'exclusive': 'original custody'})
        self.assertIn('File exists', done.stderr)

    def test_real_exclusive_refusal_stays_failed_when_reporting_cannot_finish(self):
        for case in ('report_zero', 'report_none', 'report_false', 'report_wrapped',
                     'report_negative_wrapped', 'report_oserror', 'report_runtime'):
            with self.subTest(case=case):
                done, witness, _parsed = self.child(case)
                self.assertEqual(done.returncode, 1)
                self.assertFalse(witness['escaped_report_identity'])

    def test_real_refusal_reporting_nonzero_and_keyboard_identity_is_preserved(self):
        for case, expected in (('report_nonzero', 47), ('report_keyboard', -2),
                               ('report_floating_zero', 1), ('report_huge', 255)):
            with self.subTest(case=case):
                done, witness, _parsed = self.child(case)
                self.assertEqual(done.returncode, expected)
                self.assertTrue(witness['escaped_report_identity'])

    def test_execute_api_retains_original_cancellation_objects(self):
        for primary in (SystemExit(0), SystemExit(None), SystemExit(False), SystemExit(256),
                        SystemExit(-256), SystemExit(47), SystemExit(0.0),
                        SystemExit(sys.maxsize + 1), KeyboardInterrupt('controlled suite cancellation')):
            with self.subTest(code=getattr(primary, 'code', None), kind=type(primary).__name__):
                class InterruptedSuite(unittest.TestSuite):
                    def run(self, result, debug=False):
                        raise primary

                with self.assertRaises(BaseException) as caught:
                    RESULTS.execute(InterruptedSuite())
                self.assertIs(caught.exception, primary)


if __name__ == '__main__':
    unittest.main()
