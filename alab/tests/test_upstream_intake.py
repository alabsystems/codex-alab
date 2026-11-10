# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('intake', Path(__file__).parents[1] / 'upstream_intake.py')
INTAKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INTAKE)


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
