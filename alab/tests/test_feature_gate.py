# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import hashlib
import json
import os
from pathlib import Path
import sys
import subprocess
import unittest

import test_upstream_gate as review_tests

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import feature_evidence as EVIDENCE
import feature_gate as GATE


class FeatureAdmissionTests(unittest.TestCase):
    git = review_tests.ReviewAdmissionTests.git
    commit = review_tests.ReviewAdmissionTests.commit
    reviewed = review_tests.ReviewAdmissionTests.reviewed
    persist = review_tests.ReviewAdmissionTests.persist
    check_body = review_tests.ReviewAdmissionTests.check_body

    def setUp(self):
        review_tests.ReviewAdmissionTests.setUp(self)
        (self.repo / 'alab/features.toml').write_text(
            'schema = 1\n[[features]]\nid = "model-discovery"\nlifecycle = "active"\n'
            'scenarios = ["Fresh catalog.", "Unavailable account."]\n')
        self.git('add', 'alab/features.toml')
        self.commit('contracts')

    def review(self, affected=True):
        record = self.reviewed()
        record['commits'][0]['features'] = ['model-discovery'] if affected else []
        self.persist(record)
        self.source = self.git('rev-parse', 'HEAD')
        self.expected = EVIDENCE.snapshot(self.repo, self.source, self.target, self.base)

    def ready(self):
        self.review()
        record = copy.deepcopy(self.expected)
        log = self.repo / 'fixture.log'
        log.write_text('running 1 test\ntest fixture::catalog ... ok\n'
                       'test result: ok. 1 passed; 0 failed; 0 ignored; '
                       '0 measured; 0 filtered out; finished in 0.01s\n')
        for scenario in record['features'][0]['scenarios']:
            scenario['checks'] = [{
                'source': self.source, 'command': ['fixture-command'], 'status': 0,
                'log': str(log), 'log_sha256': hashlib.sha256(log.read_bytes()).hexdigest(),
                'tests': ['fixture::catalog'], 'reason': 'Synthetic validator fixture only.'}]
        root = GATE.bundle_path(self.repo, self.source)
        root.parent.mkdir(parents=True)
        EVIDENCE.bundle(record, self.expected, root)
        return root

    def admit(self, **kwargs):
        return GATE.admission(self.repo, self.base, **kwargs)

    def test_unreviewed_source_cannot_choose_its_own_feature_scope(self):
        with self.assertRaisesRegex(ValueError, 'complete committed review'):
            self.admit()

    def test_affected_review_requires_all_scenarios_before_compilation(self):
        self.review()
        result = self.admit()
        self.assertEqual(result['affected_features'], ['model-discovery'])
        self.assertTrue(result['errors'])
        done, entered = self.check_body(hook_base='HEAD')
        self.assertFalse(entered)
        self.assertIn('feature evidence admission failed', done.stderr)

    def test_complete_bundle_admits_and_reaches_existing_gate_checks(self):
        root = self.ready()
        self.assertEqual(self.admit()['errors'], [])
        self.assertEqual(self.admit()['bundle'], str(root))
        done, entered = self.check_body()
        self.assertTrue(entered, done.stderr)
        self.assertIn('metadata failed', done.stderr)

    def test_completed_bundle_requires_integer_schema_at_both_gate_boundaries(self):
        root = self.ready()
        path = root / 'record.json'
        complete = json.loads(path.read_text())
        for name, schema, admitted in [('integer', 1, True),
                                        ('boolean', True, False),
                                        ('float', 1.0, False)]:
            record = copy.deepcopy(complete)
            record['schema'] = schema
            path.write_text(json.dumps(record))
            with self.subTest(schema=name, boundary='admission'):
                errors = self.admit()['errors']
                if admitted:
                    self.assertEqual(errors, [])
                else:
                    self.assertTrue(any('schema' in error for error in errors),
                                    f'{name} schema admitted: {errors}')
            with self.subTest(schema=name, boundary='check.sh'):
                # A prior admitted case must not leave a false compiler-entry witness.
                (self.repo / 'build-command-entered').unlink(missing_ok=True)
                done, entered = self.check_body()
                self.assertEqual(entered, admitted, done.stderr)
                self.assertIn('metadata failed' if admitted else
                              'feature evidence admission failed', done.stderr)

    def test_missing_or_partial_scenario_still_refuses_with_existing_bundle(self):
        root = self.ready()
        path = root / 'record.json'
        record = json.loads(path.read_text())
        record['features'][0]['scenarios'][1]['checks'] = []
        path.write_text(json.dumps(record))
        self.assertTrue(any('Unavailable account' in error for error in self.admit()['errors']))
        done, entered = self.check_body()
        self.assertFalse(entered)
        self.assertIn('feature evidence admission failed', done.stderr)

    def test_candidate_change_cannot_reuse_prior_bundle(self):
        root = self.ready()
        self.commit('candidate changed')
        self.assertTrue(self.admit()['errors'])
        errors = self.admit(bundle_root=root)['errors']
        self.assertIn('stale or invalid source', errors)

    def test_main_base_change_invalidates_even_an_explicit_prior_bundle(self):
        root = self.ready()
        self.git('checkout', '-q', '--detach', self.base)
        self.commit('main advanced independently')
        new_base = self.git('rev-parse', 'HEAD')
        self.git('checkout', '-q', '--detach', self.source)
        errors = GATE.admission(self.repo, new_base, bundle_root=root)['errors']
        self.assertIn('stale or invalid base', errors)

    def test_log_tampering_and_stale_inventory_are_refused(self):
        root = self.ready()
        path = root / 'record.json'
        record = json.loads(path.read_text())
        record['inventory_sha256'] = 'stale'
        path.write_text(json.dumps(record))
        self.assertIn('stale or invalid inventory_sha256', self.admit()['errors'])
        record['inventory_sha256'] = self.expected['inventory_sha256']
        path.write_text(json.dumps(record))
        log = root / record['features'][0]['scenarios'][0]['checks'][0]['log']
        log.write_text('changed')
        self.assertTrue(any('hash changed' in error for error in self.admit()['errors']))

    def test_symlinked_bundle_or_record_and_relative_override_are_refused(self):
        root = self.ready()
        alias = self.repo / 'bundle-alias'
        alias.symlink_to(root, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'must not be symlinks'):
            self.admit(bundle_root=alias)
        path = root / 'record.json'
        moved = self.repo / 'record.json'
        path.rename(moved)
        path.symlink_to(moved)
        with self.assertRaisesRegex(ValueError, 'must not be symlinks'):
            self.admit()
        with self.assertRaisesRegex(ValueError, 'must be absolute'):
            self.admit(bundle_root=Path('relative'))

    def test_explicit_moved_bundle_and_shared_worktree_default(self):
        root = self.ready()
        moved = self.repo / 'retained-evidence'
        root.rename(moved)
        self.assertEqual(self.admit(bundle_root=moved)['errors'], [])
        root.parent.mkdir(parents=True, exist_ok=True)
        moved.rename(root)
        worktree = self.repo.parent / (self.repo.name + '-candidate')
        self.git('worktree', 'add', '-q', '--detach', str(worktree), self.source)
        try:
            result = GATE.admission(worktree, self.base)
            self.assertEqual(result['errors'], [])
            self.assertEqual(result['bundle'], str(root))
        finally:
            self.git('worktree', 'remove', str(worktree))

    def test_no_new_affected_feature_requires_no_new_bundle(self):
        self.review(affected=False)
        result = self.admit()
        self.assertEqual((result['affected_features'], result['errors'], result['bundle']),
                         ([], [], None))
        self.assertEqual(GATE.admission(self.repo, self.target)['errors'], [])

    def test_selected_profile_reads_only_existing_bundle_and_keeps_empty_args_portable(self):
        # Exercise the actual check_selected shell entry under macOS /bin/bash,
        # recording its containment arguments without claiming native authority.
        self.review(affected=False)
        self.check_body()
        pristine = self.repo / 'pristine'
        (pristine / 'codex-rs/target').mkdir(parents=True)
        target = self.repo / 'target'
        target.mkdir()
        capture = self.repo / 'profile.json'
        base_env = dict(os.environ, ALAB_PYTHON=sys.executable,
                        ATPKG_PROJECT_FRONT=str(self.repo / 'front'),
                        ATPKG_PROJECT_EXECUTION=str(self.repo / 'execution'),
                        FIXTURE_PRISTINE=str(pristine), FIXTURE_TARGET=str(target),
                        FIXTURE_CAPTURE=str(capture))
        base_env.pop('ALAB_FEATURE_BUNDLE', None)
        body = ('. "$1/alab/check.sh"\n'
                'alab_project_selected() { :; }\n'
                'alab_pristine() { printf "%s\\n" "$FIXTURE_PRISTINE"; }\n'
                'alab_pristine_at() { :; }\n'
                'alab_check_targets_ready() { printf "%s\\n" "$FIXTURE_TARGET"; }\n'
                'alab_contained() { "$ALAB_PYTHON" -c '\
                "'import json,os,sys; open(os.environ[\"FIXTURE_CAPTURE\"],\"w\").write(json.dumps(sys.argv[1:]))' "
                '"$@"; }\ncheck_selected')
        missing = GATE.bundle_path(self.repo, self.source)
        moved = self.repo / 'evidence with spaces'
        moved.mkdir()
        for override, expected in [(None, None), (str(moved), str(moved)),
                                   (str(missing), None)]:
            with self.subTest(override=override):
                env = dict(base_env)
                if override is not None:
                    env['ALAB_FEATURE_BUNDLE'] = override
                done = subprocess.run(['/bin/bash', '-c', body, 'profile-fixture', str(self.repo)],
                                      env=env, cwd=self.repo, capture_output=True, text=True)
                self.assertEqual(done.returncode, 0, done.stderr)
                args = json.loads(capture.read_text())
                self.assertIn('--pristine', args)
                self.assertIn(str(target), args)
                if expected is not None:
                    self.assertEqual(args.count(expected), 1)
                    self.assertEqual(args[args.index(expected)-1], '--read')
                else:
                    self.assertNotIn(str(missing), args)


if __name__ == '__main__':
    unittest.main()
