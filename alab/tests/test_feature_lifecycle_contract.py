# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import test_feature_evidence as fixtures


INVALID_LIFECYCLES = (
    ('missing', None),
    ('unknown', '"paused"'),
    ('empty', '""'),
    ('blank', '" \\t\\n "'),
    ('padded active', '" active "'),
    ('uppercase', '"ACTIVE"'),
    ('mixed case', '"Retiring"'),
    ('boolean', 'true'),
    ('integer', '7'),
    ('float', '1.5'),
    ('list', '["active"]'),
    ('table', '{ state = "active" }'),
)


class FeatureLifecycleContractTests(unittest.TestCase):
    # Reuse only the owned-repository Git helper, not another class's test cases.
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
    def feature(identity='ownership', lifecycle='"active"', scenarios='["Initial contract."]'):
        text = '\n[[features]]\nid = ' + json.dumps(identity) + '\n'
        if lifecycle is not None:
            text += 'lifecycle = ' + lifecycle + '\n'
        if scenarios is not None:
            text += 'scenarios = ' + scenarios + '\n'
        return text

    def commit_catalog(self, *features):
        self.inventory.write_text('schema = 1\n' + ''.join(features))
        self.git('add', 'alab/features.toml')
        self.git('commit', '-qm', 'owned lifecycle contract')
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

    def test_snapshot_api_requires_a_known_string_lifecycle(self):
        for label, literal in INVALID_LIFECYCLES:
            with self.subTest(lifecycle=label):
                source = self.commit_catalog(self.feature(lifecycle=literal))
                with self.assertRaises(ValueError):
                    self.snapshot(source)

    def test_snapshot_cli_refuses_invalid_lifecycle_before_new_output(self):
        for index, (label, literal) in enumerate(INVALID_LIFECYCLES):
            with self.subTest(lifecycle=label):
                source = self.commit_catalog(self.feature(lifecycle=literal))
                record = self.repo / f'new-record-{index}.json'
                result = self.snapshot_cli(source, record)
                self.assertEqual(result.returncode, 1, (result.stdout, result.stderr))
                self.assertIn('feature evidence:', result.stderr)
                self.assertFalse(record.exists())

    def test_snapshot_cli_preserves_existing_output_on_lifecycle_refusal(self):
        record = self.repo / 'existing-record.json'
        record.write_bytes(b'preexisting evidence must remain unchanged\n')
        before = self.output_identity(record)
        for label, literal in INVALID_LIFECYCLES:
            with self.subTest(lifecycle=label):
                source = self.commit_catalog(self.feature(lifecycle=literal))
                result = self.snapshot_cli(source, record)
                self.assertEqual(result.returncode, 1, (result.stdout, result.stderr))
                self.assertEqual(self.output_identity(record), before)

    def test_active_and_retiring_contracts_preserve_source_order_and_literals(self):
        contracts = [(' retiring\tfirst ', [' first scenario ', 'Second\nline']),
                     ('Active second', ['Exact active scenario.']),
                     ('retiring-last', ['Final scenario.'])]
        lifecycles = ['retiring', 'active', 'retiring']
        source = self.commit_catalog(*[
            self.feature(identity, json.dumps(lifecycle), json.dumps(descriptions))
            for (identity, descriptions), lifecycle in zip(contracts, lifecycles)])
        expected = self.snapshot(source)
        self.assert_contracts(expected, source, contracts)
        record = self.repo / 'ordered-record.json'
        result = self.snapshot_cli(source, record)
        self.assertEqual(result.returncode, 0, (result.stdout, result.stderr))
        self.assertEqual(json.loads(record.read_text()), expected)

    def test_retired_contracts_still_omit_invalid_scenarios(self):
        retired = [self.feature('retired-' + str(index), '"retired"', scenarios)
                   for index, scenarios in enumerate((None, '"ab"', '7', '[]', '[1]'))]
        source = self.commit_catalog(*retired, self.feature('remaining', '"active"',
                                                           '["Required contract."]'))
        expected = self.snapshot(source)
        self.assert_contracts(expected, source, [('remaining', ['Required contract.'])])
        record = self.repo / 'remaining-record.json'
        result = self.snapshot_cli(source, record)
        self.assertEqual(result.returncode, 0, (result.stdout, result.stderr))
        self.assertEqual(json.loads(record.read_text()), expected)

    def test_all_retired_catalog_keeps_the_no_active_contract_refusal(self):
        source = self.commit_catalog(self.feature('first', '"retired"', None),
                                     self.feature('second', '"retired"', '7'))
        with self.assertRaisesRegex(ValueError, 'no active feature contracts'):
            self.snapshot(source)
        record = self.repo / 'all-retired-record.json'
        result = self.snapshot_cli(source, record)
        self.assertEqual(result.returncode, 1, (result.stdout, result.stderr))
        self.assertIn('no active feature contracts', result.stderr)
        self.assertFalse(record.exists())

    def test_retiring_only_snapshot_remains_included_and_exclusive(self):
        source = self.commit_catalog(self.feature('retiring-only', '"retiring"'))
        expected = self.snapshot(source)
        self.assert_contracts(expected, source, [('retiring-only', ['Initial contract.'])])
        record = self.repo / 'retiring-record.json'
        first = self.snapshot_cli(source, record)
        self.assertEqual(first.returncode, 0, (first.stdout, first.stderr))
        self.assertEqual(json.loads(record.read_text()), expected)
        before = self.output_identity(record)
        repeated = self.snapshot_cli(source, record)
        self.assertEqual(repeated.returncode, 1, (repeated.stdout, repeated.stderr))
        self.assertEqual(self.output_identity(record), before)

    def test_committed_lifecycle_wins_over_invalid_and_valid_workcopies(self):
        source = self.commit_catalog(self.feature(lifecycle='"retiring"'))
        invalid_workcopy = 'schema = 1\n' + self.feature(lifecycle='"paused"')
        self.inventory.write_text(invalid_workcopy)
        expected = self.snapshot(source)
        self.assert_contracts(expected, source, [('ownership', ['Initial contract.'])])
        record = self.repo / 'committed-record.json'
        accepted = self.snapshot_cli(source, record)
        self.assertEqual(accepted.returncode, 0, (accepted.stdout, accepted.stderr))
        self.assertEqual(json.loads(record.read_text()), expected)
        self.assertEqual(self.inventory.read_text(), invalid_workcopy)

        invalid_source = self.commit_catalog(self.feature(lifecycle='"paused"'))
        valid_workcopy = 'schema = 1\n' + self.feature(lifecycle='"active"')
        self.inventory.write_text(valid_workcopy)
        with self.subTest(selection='invalid committed API'):
            with self.assertRaises(ValueError):
                self.snapshot(invalid_source)
        with self.subTest(selection='invalid committed CLI'):
            missing = self.repo / 'uncommitted-repair-record.json'
            refused = self.snapshot_cli(invalid_source, missing)
            self.assertEqual(refused.returncode, 1, (refused.stdout, refused.stderr))
            self.assertFalse(missing.exists())
        self.assertEqual(self.inventory.read_text(), valid_workcopy)


if __name__ == '__main__':
    unittest.main()
