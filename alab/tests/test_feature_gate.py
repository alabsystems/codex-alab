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
from unittest.mock import patch

import test_upstream_gate as review_tests

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import feature_evidence as EVIDENCE
import feature_gate as GATE
import upstream_gate
from python_test_results import execute as execute_python


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

    def review(self, affected=True, feature='model-discovery'):
        record = self.reviewed()
        record['commits'][0]['features'] = [feature] if affected else []
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

    def test_bundle_admission_scans_committed_intake_once(self):
        self.ready()
        with patch.object(upstream_gate, 'snapshot', wraps=upstream_gate.snapshot) as scan:
            self.assertEqual(self.admit()['errors'], [])
        self.assertEqual(scan.call_count, 1)

    def test_review_must_match_inventory_source_and_integrated_frontier(self):
        self.review()
        inventory = EVIDENCE.snapshot(self.repo, self.source, self.target)
        review = upstream_gate.admission(self.repo, self.base)
        self.assertEqual(EVIDENCE.scoped_snapshot(inventory, review), self.expected)
        for field in ('source', 'integrated_upstream'):
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, 'source or frontier'):
                EVIDENCE.scoped_snapshot(inventory, {**review, field: self.base})

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

    def test_provider_refusal_bundle_requires_committed_live_and_replay_results(self):
        feature = 'provider-recovery'
        (self.repo / 'alab/features.toml').write_text(
            'schema = 1\n[[features]]\nid = "provider-recovery"\nlifecycle = "active"\n'
            'scenarios = ["Refusal.", "Independent."]\n'
            'required_libtest_groups = ["misalignment-turn", "misalignment-replay"]\n')
        manifest = self.repo / 'alab/feature_recipes/scenarios.json'
        manifest.parent.mkdir()
        manifest.write_text(json.dumps({'features': {feature: [
            {'scenario': 'Refusal.', 'groups': ['misalignment-turn', 'misalignment-replay'], 'coverage': 'Refusal.'},
            {'scenario': 'Independent.', 'groups': ['other'], 'coverage': 'Independent.'}]}, 'groups': {
                'misalignment-turn': {'stage': 'autopilot-turn', 'tests': ['live']},
                'misalignment-replay': {'stage': 'recovery', 'prefix': 'resume::', 'tests': ['first', 'duplicate']},
                'other': {'stage': 'core', 'tests': ['other']}}}))
        self.git('add', 'alab')
        self.commit('declare provider refusal controls')
        self.review(feature=feature)
        manifest.write_text('{"features": {}, "groups": {}}')

        # The old portable shape has no recipe obligations of its own. Admission
        # must derive those from the candidate, even for an independent worker.
        old = {k: self.expected[k] for k in ('schema', 'source', 'base', 'upstream', 'inventory_sha256')}
        old['features'] = [{'id': feature, 'scenarios': [
            {'description': description, 'checks': []} for description in ('Refusal.', 'Independent.')]}]
        parts = []
        for index, name in enumerate(('live', 'resume::first', 'resume::duplicate', 'independent')):
            log = self.repo / f'worker-{index}.log'
            log.write_text(f'running 1 test\ntest {name} ... ok\n'
                           'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n')
            observation = dict(source=self.source, command=[f'worker-{index}'], status=0,
                               log=str(log), log_sha256=hashlib.sha256(log.read_bytes()).hexdigest(),
                               tests=[name], reason='Independent fixture execution.')
            part = copy.deepcopy(old)
            part['features'][0]['scenarios'][int(name == 'independent')]['checks'].append(observation)
            parts.append(part)

        complete = EVIDENCE.combine(parts, self.expected)
        root = self.repo / 'complete-bundle'
        EVIDENCE.bundle(complete, self.expected, root)
        self.assertEqual(self.admit(bundle_root=root)['errors'], [])
        for index in (0, 2):
            with self.subTest(missing=parts[index]['features'][0]['scenarios'][0]['checks'][0]['tests']):
                wrong = copy.deepcopy(parts[index])
                refusal, other = wrong['features'][0]['scenarios']
                other['checks'].extend(refusal['checks'])
                refusal['checks'].clear()
                partial = EVIDENCE.combine(parts[:index] + [wrong] + parts[index+1:], old)
                cached = self.repo / f'old-bundle-{index}'
                EVIDENCE.bundle(partial, old, cached)
                self.assertTrue(self.admit(bundle_root=cached)['errors'])
                self.assertTrue(EVIDENCE.validate(partial, self.expected))
                with self.assertRaises(ValueError):
                    EVIDENCE.bundle(partial, self.expected, self.repo / f'incomplete-{index}')

    def test_wire_obligations_require_the_named_python_results_on_their_own_scenarios(self):
        # Execute a small genuine suite so each positive and misattributed Python name
        # is backed by a completed unittest result in the shared capture.
        class Wire(unittest.TestCase):
            def test_catalog(self):
                self.assertEqual(2 + 2, 4)

            def test_unavailable(self):
                self.assertFalse([])

        suite = unittest.defaultTestLoader.loadTestsFromTestCase(Wire)
        catalog, unavailable = [test.id() for test in suite]
        pylog = self.repo / 'wire.json'
        pylog.write_text(json.dumps(execute_python(suite)))
        rustlog = self.repo / 'native.log'
        rustlog.write_text(f'running 2 tests\ntest native ... ok\ntest {catalog} ... ok\n'
                           'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n')

        old_source = self.git('rev-parse', 'HEAD')
        old_inventory = EVIDENCE.snapshot(self.repo, old_source, self.target)
        with (self.repo / 'alab/features.toml').open('a') as file:
            file.write('required_python_groups = ["catalog-wire", "unavailable-wire"]\n')
        manifest = self.repo / 'alab/feature_recipes/scenarios.json'
        manifest.parent.mkdir()
        descriptions = ['Fresh catalog.', 'Unavailable account.']
        names = [catalog, unavailable]
        groups = ['catalog-wire', 'unavailable-wire']
        manifest.write_text(json.dumps({'features': {'model-discovery': [
            {'scenario': description, 'groups': [group]} for description, group in zip(descriptions, groups, strict=True)]},
            'groups': {group: {'stage': 'execution-wire', 'tests': [name]}
                       for group, name in zip(groups, names, strict=True)}}))
        self.git('add', 'alab')
        self.commit('declare required Python results')
        self.review()
        manifest.write_text('{}')
        self.assertEqual(EVIDENCE.snapshot(self.repo, old_source, self.target), old_inventory)
        rows = self.expected['features'][0]['scenarios']
        self.assertEqual([row['required_python_tests'] for row in rows], [[catalog], [unavailable]])
        self.assertFalse(any('required_python_tests' in row for row in old_inventory['features'][0]['scenarios']))

        def observation(log, name, result_format):
            return dict(source=self.source, status=0, command=['validator-fixture'],
                        reason='Test the admission boundary.', format=result_format, tests=[name],
                        log=str(log), log_sha256=hashlib.sha256(log.read_bytes()).hexdigest())

        # Older worker records carry only observations. The candidate supplies the obligations.
        worker = copy.deepcopy(self.expected)
        for row in worker['features'][0]['scenarios']:
            del row['required_python_tests']
        parts = []
        for index, name in enumerate(names):
            part = copy.deepcopy(worker)
            part['features'][0]['scenarios'][index]['checks'].append(observation(pylog, name, 'python-unittest'))
            parts.append(part)
        complete = EVIDENCE.combine(parts, self.expected)
        root = self.repo / 'wire-bundle'
        EVIDENCE.bundle(complete, self.expected, root)
        self.assertEqual(self.admit(bundle_root=root)['errors'], [])

        for control in ('missing', 'wrong scenario', 'same name in libtest'):
            with self.subTest(control=control):
                wrong = copy.deepcopy(complete)
                first, second = wrong['features'][0]['scenarios']
                misplaced = first['checks'].pop()
                if control == 'wrong scenario':
                    second['checks'].append(misplaced)
                name = catalog if control == 'same name in libtest' else 'native'
                first['checks'].append(observation(rustlog, name, 'libtest'))
                cache = self.repo / control
                EVIDENCE.bundle(wrong, worker, cache)
                self.assertEqual(self.admit(bundle_root=cache)['errors'],
                                 ['model-discovery: Fresh catalog.: missing required python tests: ' + catalog])

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
        # Record at the verified boundary: its production wrapper starts a new
        # shell, so an override of only the inner containment function is lost.
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
                'alab_verified_contained() { "$ALAB_PYTHON" -c '\
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
