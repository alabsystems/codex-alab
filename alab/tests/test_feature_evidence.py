# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ALAB = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ALAB))
SPEC = importlib.util.spec_from_file_location('feature_evidence', ALAB / 'feature_evidence.py')
EVIDENCE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EVIDENCE)
from python_test_results import execute as execute_python


class FeatureEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.repo = Path(self.scratch.name)
        self.git('init', '-q')
        self.git('config', 'user.name', 'Fixture')
        self.git('config', 'user.email', 'fixture@invalid')
        (self.repo / 'alab').mkdir()
        self.inventory = self.repo / 'alab/features.toml'
        self.inventory.write_text('schema = 1\n[[features]]\nid = "ownership"\n'
                                  'lifecycle = "active"\nscenarios = ["Cancel during handoff."]\n')
        self.git('add', '.')
        self.git('commit', '-qm', 'base')
        self.source = self.git('rev-parse', 'HEAD')
        self.expected = EVIDENCE.snapshot(self.repo, self.source, self.source)
        self.log = self.repo / 'native.log'
        self.log.write_text('running 1 test\ntest ownership::cancel ... ok\n'
                            'test result: ok. 1 passed; 0 failed; 0 ignored; '
                            '0 measured; 12 filtered out; finished in 0.01s\n')

    def git(self, *args):
        env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
        env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1')
        return subprocess.check_output(['git', '-C', str(self.repo), *args], env=env,
                                       stderr=subprocess.PIPE, text=True).strip()

    def record(self):
        record = copy.deepcopy(self.expected)
        record['features'][0]['scenarios'][0]['checks'] = [{
            'source': self.source, 'command': ['targo', '--unverified', 'test'],
            'status': 0, 'log': str(self.log),
            'log_sha256': hashlib.sha256(self.log.read_bytes()).hexdigest(),
            'tests': ['ownership::cancel'], 'reason': 'The fixture cancels during transfer.'}]
        return record

    def observation(self, record):
        return record['features'][0]['scenarios'][0]['checks'][0]

    def test_pending_is_not_coverage_and_completed_named_test_is_accepted(self):
        self.assertTrue(EVIDENCE.validate(self.expected, self.expected))
        self.assertEqual(EVIDENCE.validate(self.record(), self.expected), [])

    def test_snapshot_schema_requires_integer_version_one(self):
        self.assertEqual(EVIDENCE.snapshot(self.repo, self.source, self.source), self.expected)
        original = self.inventory.read_text()
        for schema in ('true', '1.0', '2', '"1"'):
            with self.subTest(schema=schema):
                self.inventory.write_text(original.replace('schema = 1\n', f'schema = {schema}\n', 1))
                self.git('add', 'alab/features.toml')
                self.git('commit', '-qm', 'invalid catalog schema')
                source = self.git('rev-parse', 'HEAD')
                with self.assertRaisesRegex(ValueError, 'unsupported feature inventory schema'):
                    EVIDENCE.snapshot(self.repo, source, source)

    def test_completed_record_schema_requires_integer_version_one(self):
        record = json.loads(json.dumps(self.record()))
        self.assertEqual(EVIDENCE.validate(record, self.expected), [])
        for schema in (True, 1.0, 2, '1', None):
            with self.subTest(schema=schema):
                changed = copy.deepcopy(record)
                changed['schema'] = schema
                errors = EVIDENCE.validate(changed, self.expected)
                self.assertIn('stale or invalid schema', errors)

    def test_cursor_terminated_tui_frame_preserves_the_named_completion_boundary(self):
        self.log.write_text(self.log.read_text().replace(
            'test ownership::cancel',
            'visible TUI frame\x1b[?2026l\x1b[0 q\x1b[?25htest ownership::cancel'))
        self.assertEqual(EVIDENCE.validate(self.record(), self.expected), [])

    def test_colored_test_name_and_status_remain_one_named_completion(self):
        self.log.write_text(self.log.read_text().replace(
            'test ownership::cancel ... ok',
            '\x1b[32mtest ownership::cancel\x1b[0m ... \x1b[32mok\x1b[0m'))
        self.assertEqual(EVIDENCE.validate(self.record(), self.expected), [])

    def test_plain_text_prefix_does_not_invent_a_test_record_boundary(self):
        self.log.write_text(self.log.read_text().replace(
            'test ownership::cancel', 'unrelated output test ownership::cancel'))
        self.assertTrue(EVIDENCE.validate(self.record(), self.expected))

    def test_requires_complete_unique_feature_and_scenario_coverage(self):
        for field in ('features', 'scenarios'):
            for replacement in ([], [None], 'not a list'):
                with self.subTest(field=field, replacement=replacement):
                    record = self.record()
                    owner = record if field == 'features' else record['features'][0]
                    owner[field] = replacement
                    self.assertTrue(EVIDENCE.validate(record, self.expected))
        record = self.record()
        record['features'] *= 2
        self.assertTrue(EVIDENCE.validate(record, self.expected))
        record = self.record()
        record['features'][0]['scenarios'][0]['description'] = 'Different behavior.'
        self.assertTrue(EVIDENCE.validate(record, self.expected))

    def test_rejects_stale_identity_and_check_source(self):
        for field in ('source', 'upstream', 'inventory_sha256'):
            record = self.record()
            record[field] = 'stale'
            self.assertTrue(EVIDENCE.validate(record, self.expected))
        record = self.record()
        self.observation(record)['source'] = 'stale'
        self.assertTrue(EVIDENCE.validate(record, self.expected))

    def test_snapshot_reads_committed_inventory_and_requires_upstream_ancestry(self):
        self.inventory.write_text('invalid working copy')
        self.assertEqual(EVIDENCE.snapshot(self.repo, self.source, self.source), self.expected)
        self.git('checkout', '--orphan', 'unrelated')
        self.git('rm', '-rf', '.')
        self.git('commit', '--allow-empty', '-qm', 'unrelated')
        with self.assertRaisesRegex(ValueError, 'does not contain'):
            EVIDENCE.snapshot(self.repo, self.source, 'HEAD')

    def test_log_tampering_and_missing_log_are_rejected(self):
        record = self.record()
        self.log.write_text(self.log.read_text() + 'tampered\n')
        self.assertTrue(EVIDENCE.validate(record, self.expected))
        self.log.unlink()
        self.assertTrue(EVIDENCE.validate(record, self.expected))

    def test_requires_actual_success_and_mapping_reason(self):
        for field, value in [('status', 1), ('status', False), ('status', '0'),
                             ('reason', ''), ('command', []), ('command', 'test'),
                             ('tests', []), ('tests', ['ownership::cancel'] * 2),
                             ('tests', ['another::test']), ('log', 'relative.log')]:
            with self.subTest(field=field, value=value):
                record = self.record()
                self.observation(record)[field] = value
                self.assertTrue(EVIDENCE.validate(record, self.expected))

    def test_skipped_truncated_or_duplicate_named_results_cannot_qualify(self):
        original = self.log.read_text()
        for output in ['Skipping test because network is disabled.\n' + original,
                       'running 1 test\ntest ownership::cancel ... ok\n',
                       original.replace('ownership::cancel', 'unrelated::test')
                       + 'test ownership::cancel ... ok\n',
                       original.replace('test ownership::cancel ... ok\n', '')]:
            self.log.write_text(output)
            record = self.record()
            self.assertTrue(EVIDENCE.validate(record, self.expected))

    def test_duplicate_names_in_completed_suites_are_rejected(self):
        self.log.write_text(self.log.read_text() * 2)
        self.assertTrue(EVIDENCE.validate(self.record(), self.expected))

    def test_cli_snapshot_is_exclusive_and_validation_fails_pending(self):
        record = self.repo / 'record.json'
        command = [sys.executable, str(ALAB / 'feature_evidence.py'), 'snapshot',
                   '--repo', str(self.repo), '--source', self.source,
                   '--upstream', self.source, '--record', str(record)]
        self.assertEqual(subprocess.run(command, capture_output=True).returncode, 0)
        before = record.read_bytes()
        self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
        self.assertEqual(record.read_bytes(), before)
        command[2] = 'validate'
        self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
        record.write_text(json.dumps(self.record()))
        self.assertEqual(subprocess.run(command, capture_output=True).returncode, 0)

    def test_bundle_survives_relocation_and_loss_of_original_log(self):
        destination = self.repo / 'bundle'
        original = self.record()
        EVIDENCE.bundle(original, self.expected, destination)
        self.assertEqual(self.observation(original)['log'], str(self.log))
        moved = self.repo / 'moved'
        destination.rename(moved)
        self.log.unlink()
        record = json.loads((moved / 'record.json').read_text())
        self.assertEqual(EVIDENCE.validate(record, self.expected, moved), [])
        self.assertTrue(EVIDENCE.validate(record, self.expected))
        command = [sys.executable, '-I', str(ALAB / 'feature_evidence.py'), 'validate',
                   '--repo', str(self.repo), '--source', self.source,
                   '--upstream', self.source, '--record', str(moved / 'record.json'),
                   '--log-root', str(moved)]
        self.assertEqual(subprocess.run(command, capture_output=True).returncode, 0)

    def test_bundle_refuses_existing_destination_and_invalid_evidence(self):
        destination = self.repo / 'bundle'
        EVIDENCE.bundle(self.record(), self.expected, destination)
        before = (destination / 'record.json').read_bytes()
        with self.assertRaises(FileExistsError):
            EVIDENCE.bundle(self.record(), self.expected, destination)
        self.assertEqual((destination / 'record.json').read_bytes(), before)
        pending = self.repo / 'pending'
        with self.assertRaises(ValueError):
            EVIDENCE.bundle(self.expected, self.expected, pending)
        self.assertFalse(pending.exists())

    def test_bundle_refuses_tampered_missing_escaping_and_symlinked_logs(self):
        destination = self.repo / 'bundle'
        EVIDENCE.bundle(self.record(), self.expected, destination)
        record = json.loads((destination / 'record.json').read_text())
        observation = self.observation(record)
        path = destination / observation['log']
        raw = path.read_bytes()
        for bad_path in [str(self.log), '../native.log', 'logs/../native.log']:
            malformed = copy.deepcopy(record)
            self.observation(malformed)['log'] = bad_path
            self.assertTrue(EVIDENCE.validate(malformed, self.expected, destination))
        path.write_bytes(raw + b'tampered')
        self.assertTrue(EVIDENCE.validate(record, self.expected, destination))
        path.unlink()
        self.assertTrue(EVIDENCE.validate(record, self.expected, destination))
        path.symlink_to(self.log)
        self.assertTrue(EVIDENCE.validate(record, self.expected, destination))
        path.unlink()
        (destination / 'logs').rmdir()
        external = self.repo / 'external'
        external.mkdir()
        (external / path.name).write_bytes(raw)
        (destination / 'logs').symlink_to(external, target_is_directory=True)
        self.assertTrue(EVIDENCE.validate(record, self.expected, destination))

    def test_bundle_deduplicates_identical_logs_without_dropping_checks(self):
        record = self.record()
        checks = record['features'][0]['scenarios'][0]['checks']
        checks.append(copy.deepcopy(checks[0]))
        destination = self.repo / 'bundle'
        EVIDENCE.bundle(record, self.expected, destination)
        exported = json.loads((destination / 'record.json').read_text())
        self.assertEqual(len(list((destination / 'logs').iterdir())), 1)
        self.assertEqual(len(exported['features'][0]['scenarios'][0]['checks']), 2)
        self.assertEqual(EVIDENCE.validate(exported, self.expected, destination), [])

    def test_export_failure_removes_only_its_new_incomplete_destination(self):
        destination = self.repo / 'bundle'
        record = self.record()
        raw = self.log.read_bytes()
        with patch.object(EVIDENCE, 'log_bytes', side_effect=[raw, OSError('read failed')]):
            with self.assertRaisesRegex(OSError, 'read failed'):
                EVIDENCE.bundle(record, self.expected, destination)
        self.assertFalse(destination.exists())
        self.assertEqual(self.log.read_bytes(), raw)

    def test_cli_bundle_requires_destination_and_validates_export(self):
        record = self.repo / 'record.json'
        record.write_text(json.dumps(self.record()))
        command = [sys.executable, '-I', str(ALAB / 'feature_evidence.py'), 'bundle',
                   '--repo', str(self.repo), '--source', self.source,
                   '--upstream', self.source, '--record', str(record)]
        self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
        destination = self.repo / 'bundle'
        command += ['--destination', str(destination)]
        self.assertEqual(subprocess.run(command, capture_output=True).returncode, 0)
        exported = json.loads((destination / 'record.json').read_text())
        self.assertEqual(EVIDENCE.validate(exported, self.expected, destination), [])

    def python_record(self):
        case = type('WorkflowFixture', (unittest.TestCase,), {'test_cancel': lambda test: None})
        result = execute_python(unittest.defaultTestLoader.loadTestsFromTestCase(case))
        self.log.write_text(json.dumps(result))
        record = self.record()
        self.observation(record).update(format='python-unittest',
                                        tests=[result['tests'][0]['id']])
        return record

    def test_named_python_results_validate_and_survive_portable_export(self):
        record = self.python_record()
        self.assertEqual(EVIDENCE.validate(record, self.expected), [])
        destination = self.repo / 'bundle'
        EVIDENCE.bundle(record, self.expected, destination)
        self.log.unlink()
        exported = json.loads((destination / 'record.json').read_text())
        self.assertEqual(EVIDENCE.validate(exported, self.expected, destination), [])

    def test_python_format_and_selected_names_cannot_be_inferred_or_ignored(self):
        for field, value in [('format', 'unknown'), ('format', 'libtest'),
                             ('tests', ['not.executed'])]:
            record = self.python_record()
            self.observation(record)[field] = value
            self.assertTrue(EVIDENCE.validate(record, self.expected))
        record = self.python_record()
        result = json.loads(self.log.read_text())
        result['tests'][0]['status'] = 'skipped'
        self.log.write_text(json.dumps(result))
        self.observation(record)['log_sha256'] = hashlib.sha256(self.log.read_bytes()).hexdigest()
        self.assertTrue(EVIDENCE.validate(record, self.expected))


if __name__ == '__main__':
    unittest.main()
