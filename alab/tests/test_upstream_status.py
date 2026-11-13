# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import unittest

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


if __name__ == '__main__':
    unittest.main()
