# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import test_feature_evidence as fixtures


NON_LIST_SCENARIOS = (
    ('string', '"ab"'),
    ('table', '{ first = "one", second = "two" }'),
    ('integer', '7'),
    ('float', '1.5'),
    ('boolean', 'true'),
    ('missing', None),
)
INVALID_ENTRIES = (
    ('numeric entry', '[1]'),
    ('mixed entries', '["valid", 2]'),
    ('boolean entry', '[true]'),
    ('nested list', '[["nested"]]'),
    ('table entry', '[{ description = "nested" }]'),
    ('empty description', '[""]'),
    ('blank description', '[" \\t\\n "]'),
    ('blank with valid description', '["valid", "  "]'),
)
EMPTY_OR_DUPLICATE = (
    ('empty list', '[]'),
    ('duplicate descriptions', '["same", "same"]'),
)
INVALID_SCENARIOS = NON_LIST_SCENARIOS + INVALID_ENTRIES + EMPTY_OR_DUPLICATE


class FeatureScenarioContractTests(unittest.TestCase):
    # Reuse only the existing owned-repository Git helper, never its test methods.
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
        self.upstream = self.commit_catalog('["Initial contract."]')

    @staticmethod
    def catalog(scenarios, *, extra=''):
        text = ('schema = 1\n[[features]]\nid = "ownership"\n'
                'lifecycle = "active"\n')
        if scenarios is not None:
            text += 'scenarios = ' + scenarios + '\n'
        return text + extra

    def commit_catalog(self, scenarios, *, extra=''):
        self.inventory.write_text(self.catalog(scenarios, extra=extra))
        self.git('add', 'alab/features.toml')
        self.git('commit', '-qm', 'owned feature contract')
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

    def assert_contract(self, record, source, descriptions):
        self.assertEqual(record['source'], source)
        self.assertEqual(record['upstream'], self.upstream)
        self.assertEqual(record['features'], [{
            'id': 'ownership', 'scenarios': [
                {'description': value, 'checks': []} for value in descriptions]}])

    def test_snapshot_api_rejects_non_list_scenarios(self):
        for label, literal in NON_LIST_SCENARIOS:
            with self.subTest(contract=label):
                source = self.commit_catalog(literal)
                with self.assertRaises(ValueError):
                    self.snapshot(source)

    def test_snapshot_api_rejects_non_string_and_blank_entries(self):
        for label, literal in INVALID_ENTRIES:
            with self.subTest(contract=label):
                source = self.commit_catalog(literal)
                with self.assertRaises(ValueError):
                    self.snapshot(source)

    def test_snapshot_api_rejects_empty_and_duplicate_scenarios(self):
        for label, literal in EMPTY_OR_DUPLICATE:
            with self.subTest(contract=label):
                source = self.commit_catalog(literal)
                with self.assertRaises(ValueError):
                    self.snapshot(source)

    def test_snapshot_cli_refuses_invalid_contract_before_claiming_output(self):
        for index, (label, literal) in enumerate(INVALID_SCENARIOS):
            with self.subTest(contract=label):
                source = self.commit_catalog(literal)
                record = self.repo / f'new-record-{index}.json'
                result = self.snapshot_cli(source, record)
                self.assertEqual(result.returncode, 1, (result.stdout, result.stderr))
                self.assertIn('feature evidence:', result.stderr)
                self.assertFalse(record.exists())

    def test_snapshot_cli_never_overwrites_existing_output_on_refusal(self):
        record = self.repo / 'existing-record.json'
        record.write_bytes(b'preexisting evidence must remain unchanged\n')
        before = self.output_identity(record)
        for label, literal in INVALID_SCENARIOS:
            with self.subTest(contract=label):
                source = self.commit_catalog(literal)
                result = self.snapshot_cli(source, record)
                self.assertEqual(result.returncode, 1, (result.stdout, result.stderr))
                self.assertEqual(self.output_identity(record), before)

    def test_unique_descriptions_preserve_order_and_literal_whitespace(self):
        descriptions = [' Same ', 'Same', '\tSame\t', 'Second\nline']
        source = self.commit_catalog(json.dumps(descriptions))
        expected = self.snapshot(source)
        self.assert_contract(expected, source, descriptions)
        record = self.repo / 'literal-record.json'
        result = self.snapshot_cli(source, record)
        self.assertEqual(result.returncode, 0, (result.stdout, result.stderr))
        self.assertEqual(json.loads(record.read_text()), expected)

    def test_successful_snapshot_cli_remains_exclusive(self):
        source = self.git('rev-parse', 'HEAD')
        record = self.repo / 'exclusive-record.json'
        first = self.snapshot_cli(source, record)
        self.assertEqual(first.returncode, 0, (first.stdout, first.stderr))
        self.assert_contract(json.loads(record.read_text()), source, ['Initial contract.'])
        before = self.output_identity(record)
        repeated = self.snapshot_cli(source, record)
        self.assertEqual(repeated.returncode, 1, (repeated.stdout, repeated.stderr))
        self.assertEqual(self.output_identity(record), before)

    def test_retired_invalid_scenario_contracts_are_still_omitted(self):
        extra = ''
        for index, (_, literal) in enumerate(INVALID_SCENARIOS):
            extra += (f'\n[[features]]\nid = "retired-{index}"\n'
                      'lifecycle = "retired"\n')
            if literal is not None:
                extra += 'scenarios = ' + literal + '\n'
        source = self.commit_catalog('["Active contract."]', extra=extra)
        expected = self.snapshot(source)
        self.assert_contract(expected, source, ['Active contract.'])
        record = self.repo / 'active-only-record.json'
        result = self.snapshot_cli(source, record)
        self.assertEqual(result.returncode, 0, (result.stdout, result.stderr))
        self.assertEqual(json.loads(record.read_text()), expected)

    def test_committed_catalog_selection_ignores_invalid_and_valid_workcopies(self):
        source = self.commit_catalog('["Committed contract."]')
        self.inventory.write_text(self.catalog('"ab"'))
        expected = self.snapshot(source)
        self.assert_contract(expected, source, ['Committed contract.'])
        record = self.repo / 'committed-record.json'
        result = self.snapshot_cli(source, record)
        self.assertEqual(result.returncode, 0, (result.stdout, result.stderr))
        self.assertEqual(json.loads(record.read_text()), expected)
        self.assertEqual(self.inventory.read_text(), self.catalog('"ab"'))

        invalid_source = self.commit_catalog('"ab"')
        self.inventory.write_text(self.catalog('["Uncommitted repair."]'))
        with self.assertRaises(ValueError):
            self.snapshot(invalid_source)
        missing = self.repo / 'uncommitted-repair-record.json'
        refused = self.snapshot_cli(invalid_source, missing)
        self.assertEqual(refused.returncode, 1, (refused.stdout, refused.stderr))
        self.assertFalse(missing.exists())
        self.assertEqual(self.inventory.read_text(), self.catalog('["Uncommitted repair."]'))


if __name__ == '__main__':
    unittest.main()
