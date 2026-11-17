# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Matrix reader status types; inert admission and subprocess seams run no host."""
from contextlib import redirect_stderr, redirect_stdout
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[1] / 'smoke/renewal_matrix.py'
sys.path.insert(0, str(SOURCE.parent))
SPEC = importlib.util.spec_from_file_location('renewal_matrix_status', SOURCE)
MATRIX = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MATRIX)


def result(**changes):
    return dict({'passed': True, 'rawHostReturncode': 0,
                 'guardian': {'returncode': 0, 'timed_out': False}}, **changes)


class RenewalMatrixStatusTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='alab-renewal-matrix-status-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name).resolve()

    def assert_predicate(self, expected, record, code=0, case='weekly-quota'):
        before = copy.deepcopy(record)
        self.assertIs(MATRIX.case_passed(case, code, record), expected)
        self.assertEqual(record, before)

    def run_matrix(self, label, outcomes=None, requested=('weekly-quota', 'happy')):
        """Exercise real admission/qualification flow with explicit inert readers.

        The reader seams deliberately assert no native/package standing. Only
        matrix processing of the intercepted controller's result is under test.
        """
        root = self.root / label
        root.mkdir()
        host_directory = root / 'owned host inputs'
        host_directory.mkdir()
        host = host_directory / 'aterm-owned-inert'
        host.write_bytes(b'owned host fixture data; never executed\n')
        (host_directory / 'build.json').write_text('{}\n')
        package = root / 'owned package inputs'
        (package / 'bin').mkdir(parents=True)
        (package / 'bin/codex').write_bytes(b'owned package fixture data; never executed\n')
        (package / 'alab-build.json').write_text('{}\n')
        (package / 'codex-package.json').write_text('{}\n')
        output = root / 'matrix'
        outcomes = outcomes or {}
        calls, admission = [], []
        records = {}

        def native_record(path):
            self.assertEqual(path, host)
            admission.append('native record reader')
            return {'controlled_fixture_only': True}

        def package_records(path):
            self.assertEqual(path, package)
            admission.append('package record reader')
            return {'controlled_fixture_only': True}, {}, package / 'bin/codex'

        def native_standing(record):
            self.assertEqual(record, {'controlled_fixture_only': True})
            admission.append('native standing seam')
            return None

        def package_standing(record):
            self.assertEqual(record, {'controlled_fixture_only': True})
            admission.append('package standing seam')
            return None

        def native_name(path):
            self.assertEqual(path, host)

        def controller(command, **keywords):
            case = command[command.index('--case') + 1]
            self.assertEqual(case, requested[len(calls)])
            self.assertEqual(Path(command[1]), SOURCE.with_name('renewal_follow.py').resolve())
            self.assertEqual(command[command.index('--host') + 1], str(host))
            self.assertEqual(command[command.index('--package') + 1], str(package))
            calls.append(case)
            code, record = outcomes.get(case, (0, result()))
            destination = Path(command[command.index('--output') + 1])
            destination.mkdir()
            (destination / 'result.json').write_text(json.dumps(record) + '\n')
            records[case] = copy.deepcopy(record)
            keywords['stdout'].write(b'intercepted inert controller; no process was started\n')
            return SimpleNamespace(returncode=code)

        native = SimpleNamespace(load=native_record, not_qualifying=native_standing,
                                 require_aterm_name=native_name, require=MATRIX.native_host_record.require)
        probe = SimpleNamespace(**{**vars(MATRIX.probe), 'package_records': package_records})
        standing = SimpleNamespace(not_qualifying=package_standing)
        process = SimpleNamespace(run=controller, STDOUT=subprocess.STDOUT)
        arguments = [str(SOURCE), '--host', str(host), '--package', str(package), '--output', str(output)]
        for case in requested:
            arguments.extend(('--case', case))
        pinned = {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in MATRIX.pinned_inputs(host, package)}
        with patch.object(MATRIX, 'native_host_record', native), patch.object(MATRIX, 'probe', probe), \
                patch.object(MATRIX, 'package_standing', standing), patch.object(MATRIX, 'subprocess', process), \
                patch.object(sys, 'argv', arguments), redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            status = MATRIX.main()
        manifest = json.loads((output / 'qualification.json').read_text())
        self.assertEqual(admission, ['native record reader', 'native standing seam',
                                     'package record reader', 'package standing seam'])
        self.assertEqual(manifest['inputs'], pinned)
        self.assertEqual(pinned, {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
                                  for path in MATRIX.pinned_inputs(host, package)})
        for case in calls:
            retained = json.loads((output / case / 'result.json').read_text())
            self.assertEqual(json.dumps(retained, sort_keys=True), json.dumps(records[case], sort_keys=True))
        return SimpleNamespace(status=status, manifest=manifest, calls=calls, output=output)

    def assert_failed_case(self, run, case, index=0):
        self.assertEqual(run.status, 1)
        self.assertFalse(run.manifest['passed'])
        self.assertEqual(run.manifest['error'], 'RuntimeError: paired case failed: ' + case)
        entry = run.manifest['cases'][index]
        self.assertEqual(entry['case'], case)
        self.assertIs(entry['passed'], False)
        self.assertEqual(entry['result'], str(run.output / case / 'result.json'))
        self.assertEqual(entry['resultSha256'],
                         hashlib.sha256((run.output / case / 'result.json').read_bytes()).hexdigest())
        self.assertFalse(run.manifest['allSixCasesPassed'])
        self.assertFalse(run.manifest['allAvailableCasesPassed'])

    def test_exact_integer_zero_statuses_remain_successful(self):
        self.assert_predicate(True, result())
        for case in ('five-hour-wall-soon', 'five-hour-wall-far'):
            proof = MATRIX.renewal_follow.CASE_PROOFS[case]
            self.assert_predicate(True, result(**{proof: {'owned_fixture_oracle': True}}), case=case)

    def test_boolean_and_float_zero_aliases_never_confirm_any_status(self):
        for alias in (False, 0.0):
            for field in ('controller', 'guardian', 'host'):
                with self.subTest(field=field, value=alias, scalar_type=type(alias).__name__):
                    code, record = 0, result()
                    if field == 'controller':
                        code = alias
                    elif field == 'guardian':
                        record['guardian']['returncode'] = alias
                    else:
                        record['rawHostReturncode'] = alias
                    self.assert_predicate(False, record, code)

    def test_missing_or_malformed_guardians_are_false_without_raising(self):
        for guardian in (None, {}, False, True, 0, 1, '', 'not-an-object', [], [0]):
            with self.subTest(guardian=guardian):
                self.assert_predicate(False, result(guardian=guardian))
        self.assert_predicate(False, {'passed': True, 'rawHostReturncode': 0})

    def test_pass_timeout_and_nonzero_status_controls_remain_strict(self):
        for value in (False, None, 1, 'true'):
            with self.subTest(passed=value):
                self.assert_predicate(False, result(passed=value))
        for timeout in (True, None, 0, 1, 'false'):
            with self.subTest(timeout=timeout):
                self.assert_predicate(False, result(guardian={'returncode': 0, 'timed_out': timeout}))
        for value in (1, -1, None, '0'):
            with self.subTest(status=value):
                self.assert_predicate(False, result(), value)
                self.assert_predicate(False, result(guardian={'returncode': value, 'timed_out': False}))
                self.assert_predicate(False, result(rawHostReturncode=value))
        for invalid in (None, [], True):
            with self.subTest(result=invalid):
                self.assert_predicate(False, invalid)

    def test_case_specific_oracle_requirements_remain_independent(self):
        for case in ('five-hour-wall-soon', 'five-hour-wall-far'):
            proof = MATRIX.renewal_follow.CASE_PROOFS[case]
            for value in (None, {}, True, [], 'record'):
                with self.subTest(case=case, proof=value):
                    self.assert_predicate(False, result(**{proof: value}), case=case)
            self.assert_predicate(False, result(), case=case)

    def test_public_matrix_keeps_canonical_success_and_original_inputs(self):
        run = self.run_matrix('canonical')
        self.assertEqual(run.status, 0)
        self.assertIs(run.manifest['passed'], True)
        self.assertEqual(run.calls, ['weekly-quota', 'happy'])
        self.assertEqual(run.manifest['requestedCases'], run.calls)
        self.assertEqual([entry['case'] for entry in run.manifest['cases']], run.calls)
        self.assertTrue(all(entry['passed'] is True for entry in run.manifest['cases']))
        self.assertFalse(run.manifest['allSixCasesPassed'])
        self.assertFalse(run.manifest['allAvailableCasesPassed'])

    def test_public_matrix_refuses_zero_aliases_and_stops_before_the_next_case(self):
        for index, alias in enumerate((False, 0.0)):
            for field in ('controller', 'guardian', 'host'):
                with self.subTest(field=field, value=alias, scalar_type=type(alias).__name__):
                    code, record = 0, result()
                    if field == 'controller':
                        code = alias
                    elif field == 'guardian':
                        record['guardian']['returncode'] = alias
                    else:
                        record['rawHostReturncode'] = alias
                    run = self.run_matrix(field + str(index), {'weekly-quota': (code, record)})
                    self.assertEqual(run.calls, ['weekly-quota'])
                    self.assertEqual(len(run.manifest['cases']), 1)
                    self.assert_failed_case(run, 'weekly-quota')
                    self.assertIs(type(run.manifest['cases'][0]['controllerExit']), type(code))

    def test_public_matrix_retains_malformed_guardian_as_an_ordinary_failed_case(self):
        # main() already returned 1 through its broad catch before this fix.
        # The reader must return False so the known case/result stays recorded.
        for index, guardian in enumerate(([0], 'not-an-object', 1)):
            with self.subTest(guardian=guardian):
                run = self.run_matrix('malformed' + str(index),
                                      {'weekly-quota': (0, result(guardian=guardian))})
                self.assertEqual(run.status, 1)
                self.assertEqual(run.calls, ['weekly-quota'])
                self.assertEqual(len(run.manifest['cases']), 1)
                self.assert_failed_case(run, 'weekly-quota')

    def test_public_matrix_retains_its_passed_prefix_and_first_failure(self):
        run = self.run_matrix('passed-prefix', {'weekly-quota': (0, result(rawHostReturncode=False))},
                              requested=('happy', 'weekly-quota', 'lost-signal-ack'))
        self.assertEqual(run.calls, ['happy', 'weekly-quota'])
        self.assertEqual([entry['passed'] for entry in run.manifest['cases']], [True, False])
        self.assert_failed_case(run, 'weekly-quota', index=1)
        self.assertFalse((run.output / 'lost-signal-ack').exists())


if __name__ == '__main__':
    unittest.main()
