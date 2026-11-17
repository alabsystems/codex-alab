# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import test_feature_evidence as fixtures


INVALID_IDS = (
    ('missing', None),
    ('integer', '7'),
    ('zero', '0'),
    ('boolean true', 'true'),
    ('boolean false', 'false'),
    ('float', '1.5'),
    ('empty', '""'),
    ('blank', '" \\t\\n "'),
    ('list', '["ownership"]'),
    ('table', '{ name = "ownership" }'),
)


class FeatureSnapshotIdTests(unittest.TestCase):
    # Reuse only the owned Git helper; do not inherit another class's methods.
    git = fixtures.FeatureEvidenceTests.git

    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.repo = Path(self.scratch.name)
        self.git('init', '-q')
        self.git('config', 'user.name', 'Fixture')
        self.git('config', 'user.email', 'fixture@invalid')
        (self.repo / 'alab').mkdir()
        self.inventory = self.repo / 'alab/features.toml'
        self.upstream = self.commit_catalog(self.feature())

    @staticmethod
    def feature(identity='"ownership"', lifecycle='"active"',
                scenarios='["Initial contract."]'):
        text = '\n[[features]]\n'
        if identity is not None:
            text += 'id = ' + identity + '\n'
        text += 'lifecycle = ' + lifecycle + '\n'
        if scenarios is not None:
            text += 'scenarios = ' + scenarios + '\n'
        return text

    def commit_catalog(self, *features):
        self.inventory.write_text('schema = 1\n' + ''.join(features))
        self.git('add', 'alab/features.toml')
        self.git('commit', '-qm', 'owned feature identity contract')
        return self.git('rev-parse', 'HEAD')

    def snapshot(self, source):
        return fixtures.EVIDENCE.snapshot(self.repo, source, self.upstream)

    def snapshot_cli(self, source, record):
        return subprocess.run(
            [sys.executable, '-I', '-B', str(fixtures.ALAB / 'feature_evidence.py'),
             'snapshot', '--repo', str(self.repo), '--source', source,
             '--upstream', self.upstream, '--record', str(record)],
            cwd=self.repo, capture_output=True, text=True, timeout=30)

    @staticmethod
    def output_identity(path):
        stat = path.stat()
        return stat.st_dev, stat.st_ino, path.read_bytes()

    def assert_contracts(self, record, source, contracts):
        self.assertEqual(record['source'], source)
        self.assertEqual(record['upstream'], self.upstream)
        self.assertEqual(record['features'], [
            {'id': identity, 'scenarios': [
                {'description': description, 'checks': []} for description in descriptions]}
            for identity, descriptions in contracts])

    def test_snapshot_api_requires_nonblank_string_ids_for_included_contracts(self):
        for lifecycle in ('active', 'retiring'):
            for label, literal in INVALID_IDS:
                with self.subTest(lifecycle=lifecycle, identity=label):
                    source = self.commit_catalog(self.feature(literal, json.dumps(lifecycle)))
                    with self.assertRaises(ValueError):
                        self.snapshot(source)

    def test_snapshot_cli_refuses_invalid_ids_before_claiming_output(self):
        for lifecycle in ('active', 'retiring'):
            for index, (label, literal) in enumerate(INVALID_IDS):
                with self.subTest(lifecycle=lifecycle, identity=label):
                    source = self.commit_catalog(self.feature(literal, json.dumps(lifecycle)))
                    record = self.repo / f'new-{lifecycle}-{index}.json'
                    result = self.snapshot_cli(source, record)
                    self.assertEqual((result.returncode, record.exists()), (1, False),
                                     (result.stdout, result.stderr))
                    self.assertIn('feature evidence:', result.stderr)

    def test_snapshot_cli_preserves_existing_output_for_invalid_ids(self):
        record = self.repo / 'existing-record.json'
        record.write_bytes(b'preexisting evidence must remain unchanged\n')
        before = self.output_identity(record)
        for lifecycle in ('active', 'retiring'):
            for label, literal in INVALID_IDS:
                with self.subTest(lifecycle=lifecycle, identity=label):
                    source = self.commit_catalog(self.feature(literal, json.dumps(lifecycle)))
                    result = self.snapshot_cli(source, record)
                    self.assertEqual(result.returncode, 1, (result.stdout, result.stderr))
                    self.assertEqual(self.output_identity(record), before)

    def test_literal_id_whitespace_case_unicode_and_source_order_are_preserved(self):
        contracts = [(' feature ', [' Literal scenario. ']),
                     ('Feature', ['Case matters.']),
                     ('feature', ['Different exact ID.']),
                     ('naïve☃', ['Unicode is retained.'])]
        lifecycles = ('retiring', 'active', 'retiring', 'active')
        source = self.commit_catalog(*[
            self.feature(json.dumps(identity), json.dumps(lifecycle), json.dumps(descriptions))
            for (identity, descriptions), lifecycle in zip(contracts, lifecycles)])
        expected = self.snapshot(source)
        self.assert_contracts(expected, source, contracts)
        record = self.repo / 'literal-record.json'
        result = self.snapshot_cli(source, record)
        self.assertEqual(result.returncode, 0, (result.stdout, result.stderr))
        self.assertEqual(json.loads(record.read_text()), expected)

    def test_exact_duplicate_ids_still_refuse_across_included_lifecycles(self):
        for first, second in (('active', 'active'), ('retiring', 'retiring'),
                              ('active', 'retiring')):
            with self.subTest(first=first, second=second):
                source = self.commit_catalog(
                    self.feature('"same"', json.dumps(first), '["First scenario."]'),
                    self.feature('"same"', json.dumps(second), '["Second scenario."]'))
                with self.assertRaises(ValueError):
                    self.snapshot(source)
                record = self.repo / f'duplicate-{first}-{second}.json'
                result = self.snapshot_cli(source, record)
                self.assertEqual((result.returncode, record.exists()), (1, False),
                                 (result.stdout, result.stderr))

    def test_retired_invalid_ids_are_omitted_without_reserving_included_ids(self):
        retired = [self.feature(literal, '"retired"', None)
                   for _, literal in INVALID_IDS]
        retired.append(self.feature('"ownership"', '"retired"', '7'))
        source = self.commit_catalog(*retired, self.feature())
        expected = self.snapshot(source)
        self.assert_contracts(expected, source, [('ownership', ['Initial contract.'])])
        record = self.repo / 'included-record.json'
        result = self.snapshot_cli(source, record)
        self.assertEqual(result.returncode, 0, (result.stdout, result.stderr))
        self.assertEqual(json.loads(record.read_text()), expected)

    def test_committed_ids_win_over_invalid_and_valid_workcopies(self):
        source = self.commit_catalog(self.feature('" Committed ID "', '"retiring"'))
        invalid_workcopy = 'schema = 1\n' + self.feature('7')
        self.inventory.write_text(invalid_workcopy)
        expected = self.snapshot(source)
        self.assert_contracts(expected, source, [(' Committed ID ', ['Initial contract.'])])
        record = self.repo / 'committed-record.json'
        accepted = self.snapshot_cli(source, record)
        self.assertEqual(accepted.returncode, 0, (accepted.stdout, accepted.stderr))
        self.assertEqual(json.loads(record.read_text()), expected)
        self.assertEqual(self.inventory.read_text(), invalid_workcopy)

        invalid_source = self.commit_catalog(self.feature('7'))
        valid_workcopy = 'schema = 1\n' + self.feature()
        self.inventory.write_text(valid_workcopy)
        with self.subTest(selection='invalid committed API'):
            with self.assertRaises(ValueError):
                self.snapshot(invalid_source)
        with self.subTest(selection='invalid committed CLI'):
            missing = self.repo / 'uncommitted-repair-record.json'
            refused = self.snapshot_cli(invalid_source, missing)
            self.assertEqual((refused.returncode, missing.exists()), (1, False),
                             (refused.stdout, refused.stderr))
        self.assertEqual(self.inventory.read_text(), valid_workcopy)

    def test_valid_snapshot_output_remains_exclusive(self):
        source = self.git('rev-parse', 'HEAD')
        record = self.repo / 'exclusive-record.json'
        first = self.snapshot_cli(source, record)
        self.assertEqual(first.returncode, 0, (first.stdout, first.stderr))
        self.assert_contracts(json.loads(record.read_text()), source,
                              [('ownership', ['Initial contract.'])])
        before = self.output_identity(record)
        repeated = self.snapshot_cli(source, record)
        self.assertEqual(repeated.returncode, 1, (repeated.stdout, repeated.stderr))
        self.assertEqual(self.output_identity(record), before)


if __name__ == '__main__':
    unittest.main()
