# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'python_test_results.py'
SPEC = importlib.util.spec_from_file_location('python_test_results', SCRIPT)
RESULTS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RESULTS)


class PythonTestResultsTests(unittest.TestCase):
    def execute(self, method, fixture=None):
        members = {'test_one': method}
        if fixture is not None:
            members['setUpClass'] = classmethod(fixture)
        case = type('Sample', (unittest.TestCase,), members)
        return RESULTS.execute(unittest.defaultTestLoader.loadTestsFromTestCase(case))

    def successful(self):
        return self.execute(lambda test: print('captured test output'))

    def stopped(self, include_later):
        executed = []

        class Stopping(unittest.TestCase):
            def test_one(self):
                executed.append('first')

            def run(self, result=None):
                result = super().run(result)
                result.stop()
                return result

        class Later(unittest.TestCase):
            def test_two(self):
                executed.append('second')
                self.fail('the stopped selection must not hide this failure')

        cases = [Stopping('test_one')]
        if include_later:
            cases.append(Later('test_two'))
        return RESULTS.execute(unittest.TestSuite(cases)), executed

    def test_pass_captures_name_output_and_completion(self):
        record = self.successful()
        self.assertEqual(record['exit_status'], 0)
        self.assertEqual(record['tests_run'], 1)
        self.assertIn('captured test output', record['output'])
        self.assertEqual(RESULTS.passed_tests(record), [record['tests'][0]['id']])

    def test_failure_error_skip_and_expected_outcomes_cannot_qualify(self):
        def error(test):
            raise RuntimeError('fixture error')

        methods = [lambda test: test.fail('assertion'), error,
                   lambda test: test.skipTest('unavailable'),
                   unittest.expectedFailure(lambda test: test.fail('expected')),
                   unittest.expectedFailure(lambda test: None)]
        for method in methods:
            with self.subTest(method=method):
                record = self.execute(method)
                self.assertEqual(record['exit_status'], 1)
                with self.assertRaises(ValueError):
                    RESULTS.passed_tests(record)

    def test_subtests_preserve_parent_failure_or_skip(self):
        for outcome in ('passed', 'failed', 'skipped'):
            def method(test):
                with test.subTest(part=1):
                    if outcome == 'failed':
                        test.fail('subtest failure')
                    if outcome == 'skipped':
                        test.skipTest('subtest unavailable')
                with test.subTest(part=2):
                    test.assertTrue(True)

            record = self.execute(method)
            with self.subTest(outcome=outcome):
                self.assertEqual(record['exit_status'], 0 if outcome == 'passed' else 1)
                self.assertEqual(record['tests_run'], 1)
                if outcome != 'passed':
                    self.assertNotEqual(record['tests'][0]['status'], 'passed')

    def test_class_fixture_failure_or_skip_is_recorded_without_invented_tests(self):
        for kind in ('error', 'skip'):
            def fixture(cls):
                if kind == 'skip':
                    raise unittest.SkipTest('fixture unavailable')
                raise RuntimeError('fixture failed')

            record = self.execute(lambda test: None, fixture)
            self.assertTrue(record['completed'])
            self.assertEqual(record['exit_status'], 1)
            self.assertEqual(record['tests_run'], 0)
            self.assertEqual(len(record['tests']), 1)

    def test_stop_before_remaining_selection_keeps_an_incomplete_failed_record(self):
        record, executed = self.stopped(include_later=True)
        self.assertEqual(executed, ['first'])
        self.assertEqual(record['tests_run'], 1)
        self.assertEqual(record['tests'][0]['status'], 'passed')
        self.assertTrue(record['tests'][0]['finished'])
        self.assertFalse(record['completed'])
        self.assertEqual(record['exit_status'], 1)
        with self.assertRaises(ValueError):
            RESULTS.passed_tests(record)

    def test_stop_after_last_case_still_refuses_complete_success(self):
        record, executed = self.stopped(include_later=False)
        self.assertEqual(executed, ['first'])
        self.assertEqual(record['tests_run'], 1)
        self.assertEqual(record['tests'][0]['status'], 'passed')
        self.assertFalse(record['completed'])
        self.assertEqual(record['exit_status'], 1)
        with self.assertRaises(ValueError):
            RESULTS.passed_tests(record)

    def test_empty_selection_and_repeated_ids_cannot_qualify(self):
        self.assertEqual(RESULTS.execute(unittest.TestSuite())['exit_status'], 1)
        case = type('Sample', (unittest.TestCase,), {'test_one': lambda test: None})
        suite = unittest.TestSuite([case('test_one'), case('test_one')])
        record = RESULTS.execute(suite)
        self.assertEqual(record['tests_run'], 2)
        self.assertEqual(record['exit_status'], 1)

    def test_validator_refuses_incomplete_or_inconsistent_records(self):
        good = self.successful()
        for key, value in [('schema', True), ('kind', 'invented'), ('completed', False),
                           ('exit_status', False), ('exit_status', 1), ('output', None),
                           ('tests_run', True), ('tests_run', 0), ('tests_run', 2),
                           ('tests', []), ('tests', [None])]:
            with self.subTest(key=key, value=value):
                record = copy.deepcopy(good)
                record[key] = value
                with self.assertRaises(ValueError):
                    RESULTS.passed_tests(record)
        for key, value in [('id', ''), ('id', None), ('finished', False), ('status', 'running')]:
            record = copy.deepcopy(good)
            record['tests'][0][key] = value
            with self.assertRaises(ValueError):
                RESULTS.passed_tests(record)

    def test_cli_records_actual_execution_and_refuses_existing_output_before_rerun(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            marker = root / 'count'
            (root / 'test_sample.py').write_text(
                'import unittest\nfrom pathlib import Path\nclass Sample(unittest.TestCase):\n'
                ' def test_one(self):\n'
                f'  p=Path({str(marker)!r})\n'
                '  p.write_text(str(int(p.read_text())+1) if p.exists() else "1")\n')
            record = root / 'results.json'
            command = [sys.executable, '-I', str(SCRIPT), '--start-directory', str(root),
                       '--pattern', 'test_sample.py', '--record', str(record)]
            done = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(done.returncode, 0, done.stderr)
            parsed = json.loads(record.read_text())
            self.assertEqual(RESULTS.passed_tests(parsed), ['test_sample.Sample.test_one'])
            before = record.read_bytes()
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            self.assertEqual(marker.read_text(), '1')
            self.assertEqual(record.read_bytes(), before)
            command[command.index('test_sample.py')] = 'missing_*.py'
            command[-1] = str(root / 'empty.json')
            self.assertEqual(subprocess.run(command, capture_output=True).returncode, 1)

    @unittest.skipUnless(sys.platform != 'win32', 'requires POSIX SIGINT delivery')
    def test_cli_handled_sigint_retains_incomplete_results_and_healthy_selection_still_passes(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            for interrupted in (True, False):
                with self.subTest(interrupted=interrupted):
                    suite = root / ('stopped' if interrupted else 'healthy')
                    suite.mkdir()
                    first, second = suite / 'first', suite / 'second'
                    (suite / 'test_signal.py').write_text(
                        'import os, signal, unittest\nfrom pathlib import Path\n'
                        'class SignalSelection(unittest.TestCase):\n'
                        ' def test_01(self):\n'
                        f'  Path({str(first)!r}).write_text("first body ran")\n'
                        '  unittest.installHandler()\n'
                        + ('  os.kill(os.getpid(), signal.SIGINT)\n' if interrupted else '')
                        + ' def test_02(self):\n'
                        f'  Path({str(second)!r}).write_text("second body ran")\n'
                        + ('  self.fail("selected failure must not disappear")\n' if interrupted else
                           '  self.assertTrue(True)\n'))
                    output = suite / 'result.json'
                    command = [sys.executable, '-I', '-B', str(SCRIPT), '--start-directory',
                               str(suite), '--pattern', 'test_signal.py', '--record', str(output)]
                    done = subprocess.run(command, capture_output=True, text=True, timeout=10)
                    record = json.loads(output.read_text())
                    self.assertEqual(first.read_text(), 'first body ran')
                    self.assertEqual(second.exists(), not interrupted)
                    self.assertEqual(record['tests_run'], 1 if interrupted else 2)
                    self.assertTrue(all(row['finished'] and row['status'] == 'passed'
                                        for row in record['tests']))
                    self.assertEqual(record['completed'], not interrupted)
                    self.assertEqual(record['exit_status'], int(interrupted))
                    self.assertEqual(done.returncode, int(interrupted), done.stdout + done.stderr)
                    self.assertEqual(done.stderr, '')
                    if interrupted:
                        with self.assertRaises(ValueError):
                            RESULTS.passed_tests(record)
                    else:
                        self.assertEqual(len(RESULTS.passed_tests(record)), 2)


if __name__ == '__main__':
    unittest.main()
