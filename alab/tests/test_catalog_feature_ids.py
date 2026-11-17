# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Catalog identities through public intake, status and committed admission readers."""
import json
from pathlib import Path
import subprocess
import sys
import unittest

import test_upstream_intake as intake_tests

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import upstream_gate as GATE


class CatalogFeatureIdentityTests(unittest.TestCase):
    git = intake_tests.IntakeTests.git
    commit = intake_tests.IntakeTests.commit
    reviewed = intake_tests.IntakeTests.reviewed

    def setUp(self):
        intake_tests.IntakeTests.setUp(self)
        (self.repo / 'alab/upstream').mkdir(parents=True)
        self.record = self.repo / 'alab/upstream/one-intake.json'

    def install(self, catalog, features=()):
        (self.repo / 'alab/features.toml').write_text(catalog)
        record = self.reviewed()
        record['commits'][0]['features'] = list(features)
        self.record.write_text(json.dumps(record))
        self.git('add', 'alab')
        self.commit('owned catalog and review fixture')

    def intake_cli(self):
        return subprocess.run([
            sys.executable, '-I', '-B', str(Path(intake_tests.INTAKE.__file__)), 'validate',
            '--repo', str(self.repo), '--base', self.base, '--target', self.target,
            '--record', str(self.record),
        ], capture_output=True, text=True, timeout=15)

    def status_cli(self):
        return subprocess.run([
            sys.executable, '-I', '-B', str(Path(GATE.__file__).with_name('upstream_status.py')),
            '--repo', str(self.repo), '--base', self.base, '--upstream', self.target,
            '--main', self.base, '--candidate', 'HEAD', '--intake', str(self.record),
        ], capture_output=True, text=True, timeout=15)

    def admit(self, base=None):
        return GATE.admission(self.repo, base or self.base, upstream=self.target)

    def assert_known_source(self, result):
        self.assertEqual(result['main_sha'], self.base)
        self.assertEqual(result['upstream_sha'], self.target)
        self.assertEqual(result['integrated_frontier_sha'], self.base)
        self.assertEqual(result['pending_count'], 1)

    def assert_intake_refuses(self):
        done = self.intake_cli()
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        self.assertNotIn('review coverage complete:', done.stdout)
        self.assertNotIn('Traceback', done.stderr)

    def assert_status_refuses(self):
        done = self.status_cli()
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        result = json.loads(done.stdout)
        self.assert_known_source(result)
        self.assertTrue(result['errors'])
        self.assertNotIn('assessment', result)
        self.assertNotIn('Traceback', done.stderr)

    def assert_all_accept(self, features):
        intake = self.intake_cli()
        self.assertEqual(intake.returncode, 0, intake.stdout + intake.stderr)
        self.assertIn('review coverage complete: 1 commits', intake.stdout)
        status = self.status_cli()
        self.assertEqual(status.returncode, 0, status.stdout + status.stderr)
        result = json.loads(status.stdout)
        self.assert_known_source(result)
        self.assertEqual(result['errors'], [])
        self.assertTrue(result['assessment']['intake_complete'])
        self.assertEqual(result['assessment']['reviewed'], 1)
        admitted = self.admit()
        self.assertEqual((admitted['reviewed'], admitted['errors'], admitted['missing']), (1, [], []))
        self.assertEqual(admitted['affected_features'], sorted(features))

    def test_duplicate_ids_are_refused_by_status_with_source_json_preserved(self):
        self.install('schema = 1\n[[features]]\nid = "same"\n[[features]]\nid = "same"\n', ['same'])
        self.assert_status_refuses()

    def test_duplicate_ids_are_refused_by_intake_cli(self):
        self.install('schema = 1\n[[features]]\nid = "same"\n[[features]]\nid = "same"\n', ['same'])
        self.assert_intake_refuses()

    def test_duplicate_ids_are_refused_by_committed_admission(self):
        self.install('schema = 1\n[[features]]\nid = "same"\n[[features]]\nid = "same"\n', ['same'])
        with self.assertRaisesRegex(ValueError, 'duplicate feature ID'):
            self.admit()

    def test_malformed_catalog_structures_are_refused_by_each_reader(self):
        cases = (
            ('missing features', 'schema = 1\n'),
            ('scalar features', 'schema = 1\nfeatures = "same"\n'),
            ('table features', 'schema = 1\n[features]\nid = "same"\n'),
            ('scalar entry', 'schema = 1\nfeatures = ["same"]\n'),
            ('missing ID', 'schema = 1\n[[features]]\nother = "value"\n'),
            ('numeric ID', 'schema = 1\n[[features]]\nid = 47\n'),
            ('boolean schema', 'schema = true\nfeatures = []\n'),
            ('floating schema', 'schema = 1.0\nfeatures = []\n'),
        )
        for label, catalog in cases:
            self.install(catalog)
            with self.subTest(catalog=label, reader='status'):
                self.assert_status_refuses()
            with self.subTest(catalog=label, reader='intake'):
                self.assert_intake_refuses()
            with self.subTest(catalog=label, reader='admission'):
                with self.assertRaises(ValueError):
                    self.admit()

    def test_blank_ids_are_refused_by_each_reader(self):
        for identity in ('', ' ', '\t\n'):
            self.install('schema = 1\n[[features]]\nid = ' + json.dumps(identity) + '\n')
            with self.subTest(identity=repr(identity), reader='status'):
                self.assert_status_refuses()
            with self.subTest(identity=repr(identity), reader='intake'):
                self.assert_intake_refuses()
            with self.subTest(identity=repr(identity), reader='admission'):
                with self.assertRaises(ValueError):
                    self.admit()

    def test_minimal_feature_entry_needs_no_lifecycle_path_or_topic_fields(self):
        self.install('schema = 1\n[[features]]\nid = "minimal"\n', ['minimal'])
        self.assert_all_accept(['minimal'])

    def test_empty_feature_list_is_accepted(self):
        self.install('schema = 1\nfeatures = []\n')
        self.assert_all_accept([])

    def test_valid_ids_are_retained_verbatim_without_normalization(self):
        identities = [' spaced ', 'spaced', 'Case', 'case', 'snowman-\u2603', '/path-like', 'line\nbreak']
        catalog = 'schema = 1\n' + ''.join(
            '[[features]]\nid = ' + json.dumps(identity) + '\n' for identity in identities)
        self.install(catalog, identities)
        self.assert_all_accept(identities)

    def test_no_new_upstream_bypasses_catalog_validation_as_before(self):
        for catalog in ('schema = 1\nfeatures = "malformed"\n',
                        'schema = 1\n[[features]]\nid = "same"\n[[features]]\nid = "same"\n'):
            with self.subTest(catalog=catalog):
                self.install(catalog)
                result = self.admit(base=self.target)
                self.assertEqual((result['commits'], result['reviewed'], result['errors'], result['missing']),
                                 (0, 0, [], []))
                self.assertEqual(result['affected_features'], [])


if __name__ == '__main__':
    unittest.main()
