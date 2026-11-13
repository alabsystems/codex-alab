# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('intake', Path(__file__).parents[1] / 'upstream_intake.py')
INTAKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INTAKE)

SCHEMA_CASES = ((1, True), (True, False), (1.0, False), (2, False),
                ('1', False), (None, False))


class IntakeTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.repo = Path(self.scratch.name)
        self.git('init', '-q')
        self.git('config', 'user.name', 'Fixture')
        self.git('config', 'user.email', 'fixture@invalid')
        self.commit('base')
        self.base = self.git('rev-parse', 'HEAD')
        (self.repo / ' model\nfile ').write_text('model')
        self.git('add', '.')
        self.commit('new model')
        self.target = self.git('rev-parse', 'HEAD')
        self.expected = INTAKE.snapshot(self.repo, self.base, self.target)

    def git(self, *args):
        env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
        env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1')
        return subprocess.check_output(['git', '-C', str(self.repo), *args], env=env).decode().strip()

    def commit(self, subject):
        self.git('commit', '-q', '--allow-empty', '-m', subject)

    def reviewed(self):
        record = copy.deepcopy(self.expected)
        record['commits'][0].update(classes=['model'], decision='adopt',
                                    reason='Expose supported catalog entry',
                                    evidence=['commit diff and request fixture'])
        return record

    def test_pending_review_fails_and_explicit_review_passes(self):
        self.assertTrue(INTAKE.validate(self.expected, self.expected))
        self.assertEqual(INTAKE.validate(self.reviewed(), self.expected), [])
        self.assertEqual(self.expected['commits'][0]['paths'], [' model\nfile '])

    def test_record_schema_requires_integer_one(self):
        for schema, accepted in SCHEMA_CASES:
            with self.subTest(schema=schema, kind=type(schema).__name__):
                record = self.reviewed()
                record['schema'] = schema
                errors = INTAKE.validate(record, self.expected)
                if accepted:
                    self.assertEqual(errors, [])
                else:
                    self.assertIn('stale or invalid schema', errors)

    def test_validation_cli_requires_integer_catalog_schema(self):
        (self.repo / 'alab').mkdir()
        catalog = self.repo / 'alab/features.toml'
        record = self.repo / 'review.json'
        record.write_text(json.dumps(self.reviewed()))
        for schema, accepted in SCHEMA_CASES:
            with self.subTest(schema=schema, kind=type(schema).__name__):
                header = '' if schema is None else f'schema = {json.dumps(schema)}\n'
                catalog.write_text(header + '[[features]]\nid = "model-discovery"\n')
                result = subprocess.run([
                    sys.executable, '-I', str(Path(INTAKE.__file__)), 'validate',
                    '--repo', str(self.repo), '--base', self.base, '--target', self.target,
                    '--record', str(record)], capture_output=True, text=True, timeout=15)
                if accepted:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn('review coverage complete:', result.stdout)
                else:
                    self.assertNotEqual(result.returncode, 0, result.stdout)
                    self.assertIn('unsupported feature inventory schema', result.stderr)
                    self.assertNotIn('review coverage complete:', result.stdout)

    def test_snapshot_cli_does_not_read_feature_inventory(self):
        self.assertFalse((self.repo / 'alab/features.toml').exists())
        record = self.repo / 'snapshot.json'
        result = subprocess.run([
            sys.executable, '-I', str(Path(INTAKE.__file__)), 'snapshot',
            '--repo', str(self.repo), '--base', self.base, '--target', self.target,
            '--record', str(record)], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(record.read_text()), self.expected)

    def test_coverage_and_stale_base_are_rejected(self):
        for mutation in ('missing', 'duplicate', 'base', 'paths'):
            with self.subTest(mutation=mutation):
                record = self.reviewed()
                if mutation == 'missing': record['commits'] = []
                if mutation == 'duplicate': record['commits'] *= 2
                if mutation == 'base': record['base'] = self.target
                if mutation == 'paths': record['commits'][0]['paths'] = []
                self.assertTrue(INTAKE.validate(record, self.expected))

    def test_new_upstream_commit_requires_new_review(self):
        self.commit('follow-up fix')
        expected = INTAKE.snapshot(self.repo, self.base, 'HEAD')
        self.assertTrue(INTAKE.validate(self.reviewed(), expected))

    def test_review_feature_ids_must_name_inventory_contracts(self):
        record = self.reviewed()
        record['commits'][0]['features'] = ['model-polling']
        errors = INTAKE.validate(record, self.expected, {'model-discovery'})
        self.assertTrue(any('unknown feature ID: model-polling' in e for e in errors))
        record['commits'][0]['features'] = ['model-discovery']
        self.assertEqual(INTAKE.validate(record, self.expected, {'model-discovery'}), [])
        record['commits'][0]['features'] = []
        self.assertEqual(INTAKE.validate(record, self.expected, {'model-discovery'}), [])

    def test_deferred_activation_needs_revisit_and_evidence(self):
        record = self.reviewed()
        record['commits'][0]['decision'] = 'defer-activation'
        self.assertTrue(INTAKE.validate(record, self.expected))
        record['commits'][0]['revisit'] = 'provider support lands'
        self.assertEqual(INTAKE.validate(record, self.expected), [])
        record['commits'][0]['evidence'] = []
        self.assertTrue(INTAKE.validate(record, self.expected))

    def test_merge_history_is_explicitly_rejected(self):
        self.git('checkout', '-q', '-b', 'side', self.base)
        self.commit('side')
        self.git('merge', '-q', '--no-ff', self.target, '-m', 'merge')
        with self.assertRaisesRegex(ValueError, 'not linear'):
            INTAKE.snapshot(self.repo, self.base, 'HEAD')


if __name__ == '__main__':
    unittest.main()
