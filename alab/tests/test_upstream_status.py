# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import io
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest
from unittest import mock

import test_upstream_intake as intake_tests

sys.path.insert(0, str(Path(__file__).parents[1]))
SPEC = importlib.util.spec_from_file_location('status', Path(__file__).parents[1] / 'upstream_status.py')
STATUS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(STATUS)


class StatusTests(unittest.TestCase):
    setUp = intake_tests.IntakeTests.setUp
    git = intake_tests.IntakeTests.git
    commit = intake_tests.IntakeTests.commit
    reviewed = intake_tests.IntakeTests.reviewed

    def observe(self, records, candidate=None, target=None):
        return STATUS.observe(self.repo, self.base, target or self.target,
                              candidate or self.target, self.base, records, {'model-discovery'})

    def test_integrated_source_does_not_prove_review_qualification_or_landing(self):
        result = self.observe([('pending', self.expected)])
        self.assertTrue(result['candidate_contains_target'])
        self.assertFalse(result['origin_tracking_contains_target'])
        self.assertFalse(result['intake_complete'])
        self.assertEqual((result['reviewed'], result['pending']), (0, 1))
        self.assertEqual(result['qualification'], 'unproven')
        self.assertEqual(result['installed_adoption'], 'unproven')

    def test_complete_review_does_not_prove_source_integration(self):
        result = self.observe([('reviewed', self.reviewed())], candidate=self.base)
        self.assertTrue(result['intake_complete'])
        self.assertFalse(result['candidate_contains_target'])

    def test_record_schema_requires_integer_one_before_counting_valid_decisions(self):
        for schema, accepted in intake_tests.SCHEMA_CASES:
            with self.subTest(schema=schema, kind=type(schema).__name__):
                record = self.reviewed()
                record['schema'] = schema
                result = self.observe([('schema-fixture', record)])
                if accepted:
                    self.assertEqual((result['reviewed'], result['errors'], result['missing']),
                                     (1, [], []))
                    self.assertTrue(result['intake_complete'])
                else:
                    self.assertEqual(result['reviewed'], 0)
                    self.assertTrue(result['errors'])
                    self.assertEqual(result['missing'], [self.target])
                    self.assertFalse(result['intake_complete'])

    def test_new_target_exposes_uncovered_commit(self):
        self.commit('new upstream change')
        new = self.git('rev-parse', 'HEAD')
        result = self.observe([('old', self.reviewed())], target=new)
        self.assertEqual(result['missing'], [new])
        self.assertEqual(result['pending'], 1)
        self.assertFalse(result['intake_complete'])

    def test_stale_immutable_fields_never_count_as_reviewed(self):
        for field, value in [('subject', 'invented'), ('paths', ['invented'])]:
            record = self.reviewed()
            record['commits'][0][field] = value
            result = self.observe([('stale', record)])
            self.assertEqual(result['reviewed'], 0)
            self.assertTrue(result['errors'])
            self.assertEqual(result['missing'], [self.target])

    def test_overlapping_records_are_not_silently_selected(self):
        result = self.observe([('one', self.reviewed()), ('two', self.reviewed())])
        self.assertEqual(result['reviewed'], 0)
        self.assertFalse(result['intake_complete'])
        self.assertIn('duplicate coverage', result['errors'][0])

    def test_decision_with_unknown_feature_stays_pending(self):
        record = self.reviewed()
        record['commits'][0]['features'] = ['unknown']
        result = self.observe([('bad', record)])
        self.assertEqual(result['pending'], 1)
        self.assertTrue(any('unknown feature' in error for error in result['errors']))

    def test_disjoint_batches_cover_complete_range(self):
        first = self.reviewed()
        self.commit('second upstream change')
        target = self.git('rev-parse', 'HEAD')
        second = STATUS.snapshot(self.repo, self.target, target)
        second['commits'][0].update(classes=['fix'], decision='adopt',
                                    reason='Retain fix', evidence=['reviewed diff'])
        result = self.observe([('first', first), ('second', second)], target=target)
        self.assertEqual((result['reviewed'], result['pending']), (2, 0))
        self.assertTrue(result['intake_complete'])

    def test_isolated_cli_emits_incomplete_observation_without_admitting_it(self):
        (self.repo / 'alab').mkdir()
        (self.repo / 'alab/features.toml').write_text('schema = 1\nfeatures = []\n')
        self.git('add', 'alab/features.toml')
        self.commit('owned feature inventory')
        result = subprocess.run([
            sys.executable, '-I', str(Path(STATUS.__file__)), '--repo', str(self.repo),
            '--base', self.base, '--upstream', self.target, '--main', self.base,
        ], capture_output=True, text=True, check=True)
        observation = json.loads(result.stdout)['assessment']
        self.assertEqual(observation['missing'], [self.target])
        self.assertFalse(observation['intake_complete'])
        self.assertEqual(observation['qualification'], 'unproven')

    def test_assessment_cli_requires_integer_catalog_schema(self):
        (self.repo / 'alab').mkdir()
        catalog = self.repo / 'alab/features.toml'
        record = self.repo / 'review.json'
        record.write_text(json.dumps(self.reviewed()))
        for schema, accepted in intake_tests.SCHEMA_CASES:
            with self.subTest(schema=schema, kind=type(schema).__name__):
                header = '' if schema is None else f'schema = {json.dumps(schema)}\n'
                catalog.write_text(header + '[[features]]\nid = "model-discovery"\n')
                self.git('add', 'alab/features.toml')
                self.commit('catalog schema fixture')
                result = subprocess.run([
                    sys.executable, '-I', str(Path(STATUS.__file__)), '--repo', str(self.repo),
                    '--base', self.base, '--upstream', self.target, '--main', self.base,
                    '--intake', str(record)], capture_output=True, text=True, timeout=15)
                if accepted:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertTrue(json.loads(result.stdout)['assessment']['intake_complete'])
                else:
                    self.assertNotEqual(result.returncode, 0, result.stdout)
                    self.assertIn('unsupported feature inventory schema', result.stderr)

    def test_lag_cli_does_not_read_feature_inventory(self):
        self.assertFalse((self.repo / 'alab/features.toml').exists())
        result = subprocess.run([
            sys.executable, '-I', str(Path(STATUS.__file__)), '--repo', str(self.repo),
            '--upstream', self.target, '--main', self.base],
            capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        observation = json.loads(result.stdout)
        self.assertNotIn('assessment', observation)
        self.assertEqual(observation['pending_count'], 1)

    def test_adoption_error_keeps_independent_source_and_review_observations(self):
        (self.repo / 'alab').mkdir()
        (self.repo / 'alab/features.toml').write_text('schema = 1\nfeatures = []\n')
        self.git('add', 'alab/features.toml')
        self.commit('inventory')
        done = subprocess.run([
            sys.executable, '-I', str(Path(STATUS.__file__)), '--repo', str(self.repo),
            '--base', self.base, '--upstream', self.target, '--main', self.base,
            '--package', str(self.repo / 'missing-artifact'),
        ], capture_output=True, text=True)
        self.assertEqual(done.returncode, 1)
        result = json.loads(done.stdout)
        self.assertEqual(result['pending_count'], 1)
        self.assertEqual(result['assessment']['missing'], [self.target])
        self.assertIsNone(result['packaged_sha'])
        self.assertTrue(result['adoption']['errors'])

    def test_failed_assessment_cli_keeps_independent_source_observations(self):
        (self.repo / 'alab').mkdir()
        (self.repo / 'alab/features.toml').write_text('schema = 1\nfeatures = []\n')
        self.git('add', 'alab/features.toml')
        self.commit('assessment inventory')
        malformed = self.repo / 'malformed.json'
        malformed.write_text('{')
        cases = (['--candidate', 'missing-candidate'], ['--candidate', self.target],
                 ['--intake', str(self.repo / 'missing.json')],
                 ['--intake', str(malformed)], ['--base', 'missing-base'])
        for arguments in cases:
            with self.subTest(arguments=arguments):
                done = subprocess.run([
                    sys.executable, '-I', '-B', str(Path(STATUS.__file__)), '--repo', str(self.repo),
                    '--main', self.base, '--upstream', self.target, '--base', self.base, *arguments,
                ], capture_output=True, text=True, timeout=15)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assertTrue(done.stdout.lstrip().startswith('{'), done.stderr)
                result = json.loads(done.stdout)
                self.assertEqual(result['main_sha'], self.base)
                self.assertEqual(result['upstream_sha'], self.target)
                self.assertEqual(result['integrated_frontier_sha'], self.base)
                self.assertEqual(result['pending_count'], 1)
                self.assertNotIn('assessment', result)
                self.assertTrue(any(error.startswith('assessment: ') for error in result['errors']))
                self.assertIn('upstream status:', done.stderr)

    def test_assessment_faults_preserve_source_but_cancellation_still_propagates(self):
        (self.repo / 'alab').mkdir()
        (self.repo / 'alab/features.toml').write_text('schema = 1\nfeatures = []\n')
        self.git('add', 'alab/features.toml')
        self.commit('assessment inventory')
        arguments = ['upstream_status.py', '--repo', str(self.repo), '--main', self.base,
                     '--upstream', self.target, '--base', self.base]
        fault = subprocess.TimeoutExpired(['git', 'assessment'], 1)
        output = io.StringIO()
        with mock.patch.object(sys, 'argv', arguments), mock.patch.object(sys, 'stdout', output), \
                mock.patch.object(sys, 'stderr', io.StringIO()), \
                mock.patch.object(STATUS, 'observe', side_effect=fault):
            self.assertEqual(STATUS.main(), 1)
        result = json.loads(output.getvalue())
        self.assertEqual(result['pending_count'], 1)
        self.assertNotIn('assessment', result)
        self.assertTrue(result['errors'])
        for cancellation in (KeyboardInterrupt('cancel assessment'), SystemExit(47)):
            with self.subTest(cancellation=type(cancellation).__name__):
                with mock.patch.object(sys, 'argv', arguments), \
                        mock.patch.object(STATUS, 'observe', side_effect=cancellation), \
                        self.assertRaises(type(cancellation)) as caught:
                    STATUS.main()
                self.assertIs(caught.exception, cancellation)

    def test_intake_without_base_remains_an_invalid_cli_invocation(self):
        done = subprocess.run([
            sys.executable, '-I', '-B', str(Path(STATUS.__file__)), '--repo', str(self.repo),
            '--intake', str(self.repo / 'missing.json')], capture_output=True, text=True, timeout=15)
        self.assertEqual(done.returncode, 1)
        self.assertEqual(done.stdout, '')
        self.assertIn('--intake requires an explicit --base', done.stderr)


class LagStatusTests(unittest.TestCase):
    setUp = intake_tests.IntakeTests.setUp
    git = intake_tests.IntakeTests.git
    commit = intake_tests.IntakeTests.commit

    def test_partial_and_complete_integration_are_distinct(self):
        result = STATUS.report(self.repo, self.base, self.target)
        self.assertEqual(result['pending_count'], 1)
        self.assertEqual(result['integrated_frontier_sha'], self.base)
        self.assertIsNotNone(result['oldest_pending_committed_at'])
        self.assertIsNone(result['qualified_sha'])
        self.assertIsNone(result['last_successful_fetch_at'])
        result = STATUS.report(self.repo, self.target, self.target)
        self.assertEqual(result['pending_count'], 0)
        self.assertIsNone(result['oldest_pending_age_seconds'])
        self.assertIsNone(result['qualified_sha'])

    def test_alab_only_changes_do_not_count_as_upstream_lag(self):
        self.git('checkout', '-q', '-b', 'alab', self.base)
        self.commit('alab change')
        result = STATUS.report(self.repo, 'HEAD', self.target)
        self.assertEqual(result['pending_count'], 1)
        self.git('merge', '-q', '--no-ff', self.target, '-m', 'integrate')
        self.assertEqual(STATUS.report(self.repo, 'HEAD', self.target)['pending_count'], 0)

    def test_missing_ref_is_unknown_not_zero(self):
        result = STATUS.report(self.repo, 'missing', self.target)
        self.assertIsNone(result['pending_count'])
        self.assertTrue(result['errors'])

    def unrelated_root(self):
        tree = self.git('rev-parse', self.target + '^{tree}')
        return self.git('commit-tree', tree, '-m', 'unrelated root')

    def future_child(self):
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith('GIT_')}
        environment.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1',
                           GIT_COMMITTER_DATE='@253402300800 +0000')
        tree = self.git('rev-parse', self.target + '^{tree}')
        child = subprocess.check_output([
            'git', '-C', str(self.repo), 'commit-tree', tree, '-p', self.base,
            '-m', 'valid Git date outside Python datetime range',
        ], env=environment, text=True).strip()
        self.assertEqual(self.git('show', '-s', '--format=%ct', child), '253402300800')
        return child

    def cli_observation(self, main, upstream):
        done = subprocess.run([
            sys.executable, '-I', '-B', str(Path(STATUS.__file__)), '--repo', str(self.repo),
            '--main', main, '--upstream', upstream,
        ], capture_output=True, text=True, timeout=15)
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        self.assertTrue(done.stdout.lstrip().startswith('{'), done.stderr)
        return json.loads(done.stdout)

    def assert_source_fields(self, result, main, upstream, frontier=None, pending=None):
        self.assertEqual(result['main_sha'], main)
        self.assertEqual(result['upstream_sha'], upstream)
        self.assertEqual(result['integrated_frontier_sha'], frontier)
        self.assertEqual(result['pending_count'], pending)
        self.assertIsNone(result['oldest_pending_committed_at'])
        self.assertIsNone(result['oldest_pending_age_seconds'])
        self.assertIsNone(result['last_successful_fetch_at'])
        self.assertIsNone(result['qualified_sha'])
        self.assertIsNone(result['packaged_sha'])
        self.assertIsNone(result['selected_sha'])
        self.assertIsNone(result['running_sha'])
        self.assertTrue(result['errors'])

    def test_disconnected_histories_keep_known_source_identities(self):
        unrelated = self.unrelated_root()
        result = STATUS.report(self.repo, self.base, unrelated)
        self.assert_source_fields(result, self.base, unrelated)
        self.assertTrue(any('integration frontier' in error for error in result['errors']))

    def test_disconnected_histories_cli_emits_partial_json(self):
        unrelated = self.unrelated_root()
        self.assert_source_fields(self.cli_observation(self.base, unrelated), self.base, unrelated)

    def test_out_of_range_commit_date_keeps_known_frontier_and_pending_count(self):
        future = self.future_child()
        result = STATUS.report(self.repo, self.base, future)
        self.assert_source_fields(result, self.base, future, self.base, 1)
        self.assertTrue(any('pending timestamps' in error for error in result['errors']))

    def test_out_of_range_commit_date_cli_emits_partial_json(self):
        future = self.future_child()
        self.assert_source_fields(self.cli_observation(self.base, future),
                                  self.base, future, self.base, 1)

    def test_ancestry_command_faults_preserve_completed_observations(self):
        real_git = STATUS.git
        cases = (
            ('merge-base', subprocess.CalledProcessError(128, ['git', 'merge-base']), None, None),
            ('rev-list', subprocess.TimeoutExpired(['git', 'rev-list'], 1), self.base, None),
            ('log', OSError('controlled timestamp command failure'), self.base, 1),
        )
        for operation, fault, frontier, pending in cases:
            with self.subTest(operation=operation):
                def query(repo, *arguments):
                    if arguments[0] == operation:
                        raise fault
                    return real_git(repo, *arguments)

                with mock.patch.object(STATUS, 'git', side_effect=query):
                    result = STATUS.report(self.repo, self.base, self.target)
                self.assert_source_fields(result, self.base, self.target, frontier, pending)
                if operation == 'merge-base':
                    self.assertTrue(any('128' in error for error in result['errors']))

    def test_reference_fault_still_observes_the_other_reference(self):
        real_git = STATUS.git
        cases = (
            (self.base, OSError('controlled first reference failure'), None, self.target),
            (self.target, subprocess.TimeoutExpired(['git', 'rev-parse'], 1), self.base, None),
        )
        for failed_ref, fault, main, upstream in cases:
            with self.subTest(reference=failed_ref):
                def query(repo, *arguments):
                    if arguments[0] == 'rev-parse' and arguments[-1] == failed_ref + '^{commit}':
                        raise fault
                    return real_git(repo, *arguments)

                with mock.patch.object(STATUS, 'git', side_effect=query):
                    result = STATUS.report(self.repo, self.base, self.target)
                self.assert_source_fields(result, main, upstream)

    def test_source_observation_cancellation_preserves_exception_identity(self):
        real_git = STATUS.git
        for operation in ('merge-base', 'rev-list', 'log'):
            for control in (KeyboardInterrupt('stop source observation'), SystemExit(47)):
                with self.subTest(operation=operation, cancellation=type(control).__name__):
                    def query(repo, *arguments):
                        if arguments[0] == operation:
                            raise control
                        return real_git(repo, *arguments)

                    with mock.patch.object(STATUS, 'git', side_effect=query), \
                            self.assertRaises(type(control)) as caught:
                        STATUS.report(self.repo, self.base, self.target)
                    self.assertIs(caught.exception, control)

    def test_multiple_real_frontiers_remain_explicitly_ambiguous(self):
        tree = self.git('rev-parse', self.target + '^{tree}')
        right = self.git('commit-tree', tree, '-p', self.base, '-m', 'right branch')
        first = self.git('commit-tree', tree, '-p', self.target, '-p', right, '-m', 'first merge')
        second = self.git('commit-tree', tree, '-p', right, '-p', self.target, '-m', 'second merge')
        self.assertEqual(set(self.git('merge-base', '--all', first, second).splitlines()),
                         {self.target, right})
        result = STATUS.report(self.repo, first, second)
        self.assert_source_fields(result, first, second)
        self.assertIn('Integration frontier is ambiguous', result['errors'])

    def test_malformed_qualification_after_keeps_independent_source_status(self):
        # A local record-shape control, not evidence that a native gate ran.
        directory = self.repo / 'observation'
        directory.mkdir()
        log = directory / 'gate.log'
        log.write_bytes(b'inert retained observation fixture\n')
        snapshot = {'source': self.base, 'tree': self.git('rev-parse', self.base + '^{tree}'),
                    'base': self.base, 'upstream': self.base, 'clean': True}
        original = {'schema': 5, 'kind': 'native-check-execution-observation',
                    'status': 0, 'before': snapshot, 'after': dict(snapshot),
                    'qualified_source': self.base, 'log': 'gate.log',
                    'log_sha256': hashlib.sha256(log.read_bytes()).hexdigest()}
        path = directory / 'record.json'
        path.write_text(json.dumps(original))
        accepted = STATUS.report(self.repo, self.base, self.target, qualification_record=path)
        self.assertEqual(accepted['qualified_sha'], self.base)
        self.assertEqual(accepted['errors'], [])
        cases = (
            ('integer clean', {**snapshot, 'clean': 1}),
            ('float clean', {**snapshot, 'clean': 1.0}),
            ('false clean', {**snapshot, 'clean': False}),
            ('missing clean', {key: value for key, value in snapshot.items() if key != 'clean'}),
            ('null snapshot', None),
            ('list snapshot', []),
            ('missing snapshot', None),
        )
        for label, after in cases:
            with self.subTest(shape=label):
                record = dict(original)
                if label == 'missing snapshot':
                    del record['after']
                else:
                    record['after'] = after
                path.write_text(json.dumps(record))
                result = STATUS.report(self.repo, self.base, self.target, qualification_record=path)
                self.assertIsNone(result['qualified_sha'])
                self.assertTrue(any(error.startswith('qualification: ') for error in result['errors']))
                self.assertEqual(result['main_sha'], self.base)
                self.assertEqual(result['upstream_sha'], self.target)
                self.assertEqual(result['integrated_frontier_sha'], self.base)
                self.assertEqual(result['pending_count'], 1)
                self.assertIsNone(result['packaged_sha'])
                self.assertIsNone(result['selected_sha'])
                self.assertIsNone(result['running_sha'])
                self.assertIsNone(result['last_successful_fetch_at'])


if __name__ == '__main__':
    unittest.main()
