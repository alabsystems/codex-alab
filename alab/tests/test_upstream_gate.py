# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Committed intake admission, including its real pre-build check.sh boundary."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import unittest

import test_upstream_intake as intake_tests

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'alab'))
SPEC = importlib.util.spec_from_file_location('upstream_gate', ROOT / 'alab/upstream_gate.py')
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)
import feature_evidence as EVIDENCE


class ReviewAdmissionTests(unittest.TestCase):
    git = intake_tests.IntakeTests.git
    commit = intake_tests.IntakeTests.commit
    reviewed = intake_tests.IntakeTests.reviewed

    def setUp(self):
        intake_tests.IntakeTests.setUp(self)
        (self.repo / 'alab/upstream').mkdir(parents=True)
        (self.repo / 'alab/features.toml').write_text(
            'schema = 1\n[[features]]\nid = "model-discovery"\n')
        self.git('add', 'alab/features.toml')
        self.commit('feature catalog')
        self.git('update-ref', 'refs/remotes/upstream/main', self.target)
        self.git('update-ref', 'refs/remotes/origin/main', self.base)

    def persist(self, record, name='one-intake.json'):
        (self.repo / 'alab/upstream' / name).write_text(json.dumps(record))
        self.git('add', 'alab/upstream')
        self.commit('intake review')

    def admit(self, base=None):
        return GATE.admission(self.repo, base or self.base)

    def test_missing_and_uncommitted_reviews_do_not_admit(self):
        self.assertEqual(self.admit()['missing'], [self.target])
        (self.repo / 'alab/upstream/one-intake.json').write_text(json.dumps(self.reviewed()))
        self.assertEqual(self.admit()['missing'], [self.target])
        self.persist(self.reviewed())
        result = self.admit()
        self.assertEqual((result['commits'], result['reviewed'], result['errors'], result['missing']),
                         (1, 1, [], []))

    def test_changed_source_and_unknown_feature_do_not_admit(self):
        record = self.reviewed()
        record['commits'][0]['paths'] = ['invented']
        self.persist(record)
        self.assertTrue(self.admit()['errors'])
        record = self.reviewed()
        record['commits'][0]['features'] = ['unknown']
        self.persist(record)
        self.assertTrue(any('unknown feature ID' in error for error in self.admit()['errors']))

    def test_record_schema_requires_integer_one_before_counting_valid_decisions(self):
        for schema, accepted in intake_tests.SCHEMA_CASES:
            with self.subTest(schema=schema, kind=type(schema).__name__):
                record = self.reviewed()
                record['schema'] = schema
                self.persist(record)
                result = self.admit()
                if accepted:
                    self.assertEqual((result['reviewed'], result['errors'], result['missing']),
                                     (1, [], []))
                else:
                    self.assertEqual(result['reviewed'], 0)
                    self.assertTrue(result['errors'])
                    self.assertEqual(result['missing'], [self.target])

    def test_committed_catalog_schema_requires_integer_one(self):
        self.persist(self.reviewed())
        for schema, accepted in intake_tests.SCHEMA_CASES:
            with self.subTest(schema=schema, kind=type(schema).__name__):
                header = '' if schema is None else f'schema = {json.dumps(schema)}\n'
                (self.repo / 'alab/features.toml').write_text(
                    header + '[[features]]\nid = "model-discovery"\n')
                self.git('add', 'alab/features.toml')
                self.commit('catalog schema fixture')
                if accepted:
                    result = self.admit()
                    self.assertEqual((result['reviewed'], result['errors'], result['missing']),
                                     (1, [], []))
                else:
                    with self.assertRaisesRegex(ValueError, 'unsupported feature inventory schema'):
                        self.admit()

    def test_duplicate_records_never_silently_select_one(self):
        self.persist(self.reviewed())
        self.persist(self.reviewed(), 'two-intake.json')
        result = self.admit()
        self.assertEqual(result['reviewed'], 0)
        self.assertTrue(any('duplicate coverage' in error for error in result['errors']))

    def test_explicit_supersession_uses_only_the_validated_committed_replacement(self):
        self.persist(self.reviewed())
        older = self.reviewed()
        older.update(superseded_by='alab/upstream/one-intake.json',
                     supersession_reason='Reconciled into the complete review batch')
        self.persist(older, 'older-intake.json')
        result = self.admit()
        self.assertEqual((result['reviewed'], result['missing'], result['errors']), (1, [], []))
        older['superseded_by'] = 'alab/upstream/absent-intake.json'
        self.persist(older, 'older-intake.json')
        self.assertTrue(self.admit()['errors'])

    def test_superseded_record_requires_integer_one_despite_valid_replacement(self):
        replacement = self.reviewed()
        replacement['commits'][0]['features'] = ['model-discovery']
        self.persist(replacement)
        for schema, accepted in ((1, True), (True, False), (1.0, False), (2, False)):
            with self.subTest(schema=schema, kind=type(schema).__name__):
                older = copy.deepcopy(replacement)
                older.update(schema=schema, superseded_by='alab/upstream/one-intake.json',
                             supersession_reason='Use the complete committed replacement')
                self.persist(older, 'older-intake.json')
                result = self.admit()
                if accepted:
                    self.assertEqual((result['reviewed'], result['missing'], result['errors']),
                                     (1, [], []))
                    self.assertEqual(result['affected_features'], ['model-discovery'])
                else:
                    self.assertTrue(result['errors'], result)
                    self.assertEqual(result['affected_features'], [])

    def test_supersession_cannot_replace_a_review_with_a_pending_declaration(self):
        self.persist(self.expected)
        older = self.reviewed()
        older.update(superseded_by='alab/upstream/one-intake.json',
                     supersession_reason='Use the new batch')
        self.persist(older, 'older-intake.json')
        result = self.admit()
        self.assertEqual(result['reviewed'], 0)
        self.assertEqual(result['missing'], [self.target])

    def test_stepwise_prefix_can_use_a_larger_batch_but_new_commits_stay_pending(self):
        candidate = self.git('rev-parse', 'HEAD')
        self.git('checkout', '-q', '--detach', self.target)
        self.commit('next upstream change')
        later = self.git('rev-parse', 'HEAD')
        self.git('update-ref', 'refs/remotes/upstream/main', later)
        self.git('checkout', '-q', '--detach', candidate)
        batch = GATE.snapshot(self.repo, self.base, later)
        batch['commits'][0] = copy.deepcopy(self.reviewed()['commits'][0])
        self.persist(batch)
        self.assertEqual(self.admit()['missing'], [])
        self.git('merge', '-q', '--no-ff', later, '-m', 'integrate next upstream change')
        result = self.admit()
        self.assertEqual((result['reviewed'], result['missing']), (1, [later]))
        # An already integrated prefix need not be reviewed again. Only the
        # remaining commit's declaration contributes to this admission.
        batch['commits'][0]['decision'] = None
        batch['commits'][1].update(classes=['fix'], decision='adopt',
                                   reason='Retain upstream fix', evidence=['diff reviewed'])
        self.persist(batch)
        result = self.admit(base=self.target)
        self.assertEqual((result['commits'], result['reviewed'], result['missing']), (1, 1, []))

    def test_owned_only_change_needs_no_new_upstream_assessment(self):
        self.git('rm', 'alab/features.toml')
        self.commit('no inventory needed without new upstream work')
        result = self.admit(base=self.target)
        self.assertEqual((result['commits'], result['reviewed'], result['errors'], result['missing']),
                         (0, 0, [], []))

    def test_feature_scope_requires_complete_committed_review(self):
        record = self.reviewed()
        record['commits'][0]['features'] = ['model-discovery']
        (self.repo / 'alab/upstream/one-intake.json').write_text(json.dumps(record))
        self.assertEqual(self.admit()['affected_features'], [])
        self.persist(record)
        self.assertEqual(self.admit()['affected_features'], ['model-discovery'])
        self.persist(record, 'duplicate-intake.json')
        self.assertEqual(self.admit()['affected_features'], [])

    def test_evidence_scope_binds_base_and_retains_complete_affected_contract(self):
        (self.repo / 'alab/features.toml').write_text(
            'schema = 1\n[[features]]\nid = "model-discovery"\n'
            'lifecycle = "active"\nscenarios = ["Fresh catalog.", "Unavailable account."]\n'
            '[[features]]\nid = "unaffected"\nlifecycle = "active"\n'
            'scenarios = ["Other behavior."]\n')
        self.git('add', 'alab/features.toml')
        self.commit('contracts')
        with self.assertRaisesRegex(ValueError, 'complete committed upstream review'):
            EVIDENCE.snapshot(self.repo, 'HEAD', self.target, self.base)
        record = self.reviewed()
        record['commits'][0]['features'] = ['model-discovery']
        self.persist(record)
        expected = EVIDENCE.snapshot(self.repo, 'HEAD', self.target, self.base)
        self.assertEqual(expected['base'], self.base)
        self.assertEqual([f['id'] for f in expected['features']], ['model-discovery'])
        self.assertEqual(len(expected['features'][0]['scenarios']), 2)
        self.assertEqual(len(EVIDENCE.validate(expected, expected)), 2)
        pending = self.repo / 'pending-evidence.json'
        done = subprocess.run([
            sys.executable, '-I', str(ROOT / 'alab/feature_evidence.py'), 'snapshot',
            '--repo', str(self.repo), '--source', 'HEAD', '--upstream', self.target,
            '--base', self.base, '--record', str(pending)], capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(json.loads(pending.read_text()), expected)
        stale = copy.deepcopy(expected)
        stale['base'] = self.target
        self.assertIn('stale or invalid base', EVIDENCE.validate(stale, expected))
        no_new_upstream = EVIDENCE.snapshot(self.repo, 'HEAD', self.target, self.target)
        self.assertEqual(no_new_upstream['features'], [])
        self.assertEqual(EVIDENCE.validate(no_new_upstream, no_new_upstream), [])

    def test_affected_retired_contract_cannot_be_silently_omitted(self):
        (self.repo / 'alab/features.toml').write_text(
            'schema = 1\n[[features]]\nid = "model-discovery"\nlifecycle = "retired"\n'
            '[[features]]\nid = "other"\nlifecycle = "active"\nscenarios = ["Other."]\n')
        self.git('add', 'alab/features.toml')
        self.commit('retired contract')
        record = self.reviewed()
        record['commits'][0]['features'] = ['model-discovery']
        self.persist(record)
        with self.assertRaisesRegex(ValueError, 'no active or retiring contract'):
            EVIDENCE.snapshot(self.repo, 'HEAD', self.target, self.base)

    def test_prefix_scope_excludes_features_only_in_unintegrated_commits(self):
        (self.repo / 'alab/features.toml').write_text(
            'schema = 1\n[[features]]\nid = "model-discovery"\n'
            'lifecycle = "active"\nscenarios = ["Catalog."]\n'
            '[[features]]\nid = "later-feature"\nlifecycle = "active"\n'
            'scenarios = ["Later behavior."]\n')
        self.git('add', 'alab/features.toml')
        self.commit('contracts')
        candidate = self.git('rev-parse', 'HEAD')
        self.git('checkout', '-q', '--detach', self.target)
        self.commit('next upstream change')
        later = self.git('rev-parse', 'HEAD')
        self.git('checkout', '-q', '--detach', candidate)
        batch = GATE.snapshot(self.repo, self.base, later)
        batch['commits'][0].update(self.reviewed()['commits'][0])
        batch['commits'][0]['features'] = ['model-discovery']
        batch['commits'][1].update(classes=['fix'], decision='adopt',
                                   features=['later-feature'], reason='Retain fix', evidence=['diff'])
        self.persist(batch)
        expected = EVIDENCE.snapshot(self.repo, 'HEAD', self.target, self.base)
        self.assertEqual([f['id'] for f in expected['features']], ['model-discovery'])

    def test_isolated_cli_returns_nonzero_for_unreviewed_source(self):
        done = subprocess.run([sys.executable, '-I', str(ROOT / 'alab/upstream_gate.py'),
                               '--repo', str(self.repo), '--base', self.base],
                              capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 1, done.stderr)
        self.assertEqual(json.loads(done.stdout)['missing'], [self.target])

    def check_body(self, hook_base=None):
        (self.repo / 'codex-rs').mkdir(exist_ok=True)
        for name in ('check.sh', 'lib.sh', 'retention.py', 'upstream_gate.py', 'upstream_intake.py',
                     'feature_gate.py', 'feature_evidence.py', 'focused_results.py', 'python_test_results.py',
                     'pristine_state.py', 'qualification.py', 'source_content.py'):
            shutil.copyfile(ROOT / 'alab' / name, self.repo / 'alab' / name)
        marker = self.repo / 'build-command-entered'
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(('GIT_', 'ALAB_'))}
        env.update(ALAB_PYTHON=sys.executable, ALAB_CHECK_BASE=hook_base or self.base,
                   ALAB_ALLOW_HOOK_LOSS='1',
                   ALAB_TEST_COMPILER_CALLED=str(marker),
                   GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1')
        done = subprocess.run(['/bin/bash', '-c',
                               '. "$1/alab/check.sh"\n'
                               'alab_build_command() { : > "$ALAB_TEST_COMPILER_CALLED"; return 71; }\n'
                               'check_main', 'gate-fixture', str(self.repo)],
                              cwd=self.repo, env=env, capture_output=True, text=True, timeout=60)
        return done, marker.exists()

    def test_real_gate_refuses_before_its_first_build_command(self):
        done, entered = self.check_body()
        self.assertNotEqual(done.returncode, 0)
        self.assertIn('upstream review admission failed', done.stderr)
        self.assertFalse(entered)

    def test_hook_baseline_override_cannot_bypass_upstream_review(self):
        done, entered = self.check_body(hook_base='HEAD')
        self.assertNotEqual(done.returncode, 0)
        self.assertIn('upstream review admission failed', done.stderr)
        self.assertFalse(entered)

    def test_real_gate_reaches_existing_checks_after_committed_review(self):
        self.persist(self.reviewed())
        done, entered = self.check_body()
        self.assertTrue(entered, done.stderr)
        self.assertIn('metadata failed', done.stderr)


if __name__ == '__main__':
    unittest.main()
