# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    'focused_results', Path(__file__).resolve().parents[1] / 'focused_results.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def suite(count=1, passed=1, failed=0, ignored=0, measured=0, outcome='ok'):
    return (f'running {count} tests\n'
            f'test result: {outcome}. {passed} passed; {failed} failed; '
            f'{ignored} ignored; {measured} measured; 12 filtered out; finished in 0.01s\n')


def named_suite(*names):
    header, result = suite(len(names), len(names)).split('\n', 1)
    return header + '\n' + ''.join(f'test {name} ... ok\n' for name in names) + result


class FocusedResultsTests(unittest.TestCase):
    def test_accepts_completed_singular_and_multiple_suites(self):
        output = suite().replace('1 tests', '1 test') + suite(3, 3)
        self.assertEqual([1, 3], [r['passed'] for r in module.confirm(output, 0, 2)])

    def test_rejects_empty_even_with_another_passing_suite(self):
        with self.assertRaises(ValueError):
            module.confirm(suite(0, 0) + suite(), 0, 1)

    def test_rejects_failed_command_despite_passing_summary(self):
        with self.assertRaises(ValueError):
            module.confirm(suite(), 101, 1)

    def test_rejects_missing_truncated_or_unselected_output(self):
        for output in ['', 'running 1 test\n', suite().split('\n', 1)[1],
                       suite() + 'running 1 test\n']:
            with self.subTest(output=output), self.assertRaises(ValueError):
                module.confirm(output, 0, 1)

    def test_rejects_failed_ignored_measured_and_wrong_counts(self):
        for output in [suite(2, 1, failed=1, outcome='FAILED'),
                       suite(2, 1, ignored=1), suite(2, 1, measured=1), suite(3, 2)]:
            with self.subTest(output=output), self.assertRaises(ValueError):
                module.confirm(output, 0, 1)

    def test_rejects_missing_minimum_and_invalid_minimum(self):
        for minimum in [0, -1, 2]:
            with self.subTest(minimum=minimum), self.assertRaises(ValueError):
                module.confirm(suite(), 0, minimum)

    def test_rejects_reported_skip_that_libtest_calls_passed(self):
        def shown_output(message):
            return named_suite('fixture::case').replace(
                'test result:',
                'successes:\n\n---- fixture::case stdout ----\n' + message
                + '\n\nsuccesses:\n    fixture::case\n\ntest result:', 1)

        # Successful diagnostics must remain compatible with exact named checks.
        module.confirm(shown_output('fixture debug output\n{"status": "complete"}'),
                       0, 1, required_tests=['fixture::case'])
        for message in ['Skipping test because network is disabled.',
                        "CODEX_SANDBOX is set to 'seatbelt', skipping test.",
                        "skipping cloud-managed subprocess: host-managed authentication prevents isolated mock credentials"]:
            with self.subTest(message=message), self.assertRaises(ValueError):
                module.confirm(message + '\n' + suite(), 0, 1)
            with self.subTest(success_output=message), self.assertRaisesRegex(
                    ValueError, 'skipped scenario'):
                module.confirm(shown_output(message), 0, 1, required_tests=['fixture::case'])

    def test_rejects_nested_or_reordered_completion(self):
        for output in ['running 1 test\n' + suite(),
                       suite().split('\n', 1)[1] + 'running 1 test\n',
                       'running 1 test\nrunning 1 test\n' +
                       suite().split('\n', 1)[1] * 2]:
            with self.subTest(output=output), self.assertRaises(ValueError):
                module.confirm(output, 0, 1)

    def test_requires_every_filter_even_when_other_tests_pass(self):
        output = named_suite('cloud_backend::tests::first', 'cloud_backend::idle::second')
        module.confirm(output, 0, 1, required_filters=['cloud_backend::tests', 'idle::'])
        for filters in (['cloud_backend::connection_tests'],
                        ['cloud_backend::idle', 'cloud_backend::connection_tests'], ['']):
            with self.subTest(filters=filters), self.assertRaises(ValueError):
                module.confirm(output, 0, 1, required_filters=filters)

    def test_exact_names_do_not_match_siblings_or_nested_variants(self):
        output = named_suite('module::case', 'module::case_extra', 'module::case::variant')
        module.confirm(output, 0, 1, required_tests=['module::case', 'module::case::variant'])
        for required in ('case', 'module', 'module::missing', ''):
            with self.subTest(required=required), self.assertRaises(ValueError):
                module.confirm(output, 0, 1, required_tests=[required])
        with self.assertRaises(ValueError):
            module.confirm(named_suite('module::case_extra', 'module::case::variant'),
                           0, 1, required_tests=['module::case'])

    def test_guards_reject_outside_names_duplicates_and_incomplete_named_counts(self):
        valid = named_suite('module::other')
        for output in ('test module::wanted ... ok\n' + valid,
                       valid + 'test module::wanted ... ok\n',
                       named_suite('module::wanted') * 2,
                       named_suite('module::wanted').replace('running 1 tests', 'running 2 tests')
                       .replace('1 passed', '2 passed')):
            with self.subTest(output=output), self.assertRaises(ValueError):
                module.confirm(output, 0, 1, required_filters=['module::wanted'])

    def test_cli_guards_normalize_ansi_and_keep_real_record_boundaries(self):
        output = named_suite('module::wanted')
        colored = output.replace('test module::wanted ... ok',
                                 '\x1b[32mtest module::wanted\x1b[0m ... \x1b[32mok\x1b[0m')
        frame = output.replace('test module::wanted',
                               'first frame line\nlast frame\x1b[?25htest module::wanted')
        plain = output.replace('test module::wanted', 'visible text test module::wanted')
        with tempfile.TemporaryDirectory() as scratch:
            log = Path(scratch) / 'test.log'
            command = [sys.executable, '-I', str(Path(module.__file__)),
                       '--log', str(log), '--status', '0', '--require-test', 'module::wanted']
            for text, status in ((colored, 0), (frame, 0), (plain, 1)):
                log.write_text(text)
                with self.subTest(text=text):
                    result = subprocess.run(command, capture_output=True, text=True)
                    self.assertEqual(result.returncode, status, result.stderr)


if __name__ == '__main__':
    unittest.main()
