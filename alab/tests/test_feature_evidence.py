# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
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
    def test_standalone_help_leaves_its_python_sources_without_bytecode(self):
        reader = self.repo / 'reader'
        reader.mkdir()
        for module in ALAB.glob('*.py'):
            shutil.copyfile(module, reader / module.name)
        # -I ignores environment settings. Omit -B to exercise the CLI's own policy.
        done = subprocess.run([sys.executable, '-I', str(reader / 'feature_evidence.py'), '--help'],
                              capture_output=True, text=True, timeout=10)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn('export-progress', done.stdout)
        self.assertEqual(list(reader.rglob('*.pyc')), [])

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

    def test_progress_combines_typed_results_and_counts_each_row_once(self):
        python_observation = self.observation(self.python_record())
        python_log = self.repo / 'python-result.json'
        self.log.rename(python_log)
        python_observation['log'] = str(python_log)
        name, = python_observation['tests']
        # Both formats may legitimately report the same spelling. Each is a separate requirement.
        self.log.write_text(f'running 1 test\ntest {name} ... ok\n'
                            'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n')
        rust_observation = self.observation(self.record())
        rust_observation['tests'] = [name]
        expected = copy.deepcopy(self.expected)
        first, = expected['features'][0]['scenarios']
        first.update(required_libtests=[name], required_python_tests=[name])
        second = {'description': 'Retain the owner.', 'checks': []}
        expected['features'][0]['scenarios'].append(second)
        rust_part, python_part = copy.deepcopy(expected), copy.deepcopy(expected)
        rust_part['features'][0]['scenarios'][0]['checks'].append(rust_observation)
        python_part['features'][0]['scenarios'][0]['checks'].append(python_observation)
        # The committed contract owns the required names, even if a worker omits them.
        del rust_part['features'][0]['scenarios'][0]['required_python_tests']
        partial = EVIDENCE.progress([rust_part, rust_part], expected)
        self.assertEqual((partial['covered'], partial['total']), (0, 2))
        self.assertIn(f"{expected['features'][0]['id']}   0 of 2 verified, pending 1, 2",
                      EVIDENCE.progress_summary(partial))
        self.assertEqual(partial['features'][0]['pending'], [
            {'number': 1, 'description': first['description'], 'missing': {'required_python_tests': [name]}},
            {'number': 2, 'description': second['description']}])
        full = EVIDENCE.progress([rust_part, python_part, python_part], expected)
        self.assertEqual((full['covered'], full['total']), (1, 2))
        self.assertEqual(full['features'], [{'id': 'ownership', 'covered': 1, 'total': 2,
                                             'pending': [{'number': 2, 'description': second['description']}]}])
        scoped = EVIDENCE.progress([dict(rust_part, base=self.source)], dict(expected, base=self.source))
        self.assertEqual((scoped['scope'], scoped['base']), ('affected-features', self.source))
        with self.assertRaisesRegex(ValueError, 'base'):
            EVIDENCE.progress([rust_part, dict(python_part, base=self.source)], expected)

    def test_export_progress_replays_after_relocation_and_adds_local_evidence(self):
        self.inventory.write_text(self.inventory.read_text().replace(
            '["Cancel during handoff."]', '["Cancel during handoff.", "Reattach.", "Recover."]'))
        self.git('commit', '-qam', 'extend the observed contract')
        self.source = self.git('rev-parse', 'HEAD')
        self.expected = EVIDENCE.snapshot(self.repo, self.source, self.source)
        local = self.repo / 'part.json'
        local.write_text(json.dumps(self.record()))
        archive = self.repo / 'archive'
        moved = self.repo / 'moved'
        common = ['--repo', str(self.repo), '--source', self.source, '--upstream', self.source]

        def run(operation, *options, success=True):
            proc = subprocess.run([sys.executable, '-I', '-B', str(ALAB / 'feature_evidence.py'),
                                   operation, *common, *map(str, options)], capture_output=True, text=True, timeout=10)
            self.assertEqual(proc.returncode == 0, success, proc.stderr)
            return proc

        first = run('export-progress', '--record', local, '--part', local, '--destination', archive)
        self.assertEqual((json.loads(first.stdout)['covered'], json.loads(first.stdout)['total']), (1, 3))
        self.assertFalse((archive / 'record.json').exists())
        self.assertEqual(len(list((archive / 'logs').iterdir())), 1)
        archive.rename(moved)
        overlap = self.repo / 'overlap'
        run('export-progress', '--record', moved / 'progress-record.json', '--log-root', moved,
            '--part', local, '--destination', overlap)
        self.assertEqual((overlap / 'progress-record.json').read_bytes(),
                         (moved / 'progress-record.json').read_bytes())
        self.log.unlink()
        local.unlink()
        portable = ['--record', moved / 'progress-record.json', '--log-root', moved]
        self.assertEqual(json.loads(run('progress', *portable).stdout), json.loads(first.stdout))
        self.assertIn('executed evidence required', run('validate', *portable, success=False).stderr)

        self.log.write_text('running 1 test\ntest ownership::reattach ... ok\n'
                            'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n')
        second = self.record()
        scenarios = second['features'][0]['scenarios']
        scenarios[1]['checks'], scenarios[0]['checks'] = scenarios[0]['checks'], []
        scenarios[1]['checks'][0].update(tests=['ownership::reattach'], reason='Reattach after transfer.')
        local.write_text(json.dumps(second))
        both = run('export-progress', *portable, '--part', local, '--destination', archive)
        self.assertEqual(json.loads(both.stdout)['covered'], 2)
        self.assertEqual(len(list((archive / 'logs').iterdir())), 2)
        self.log.unlink()
        self.assertEqual(json.loads(run('progress', '--record', archive / 'progress-record.json',
                                        '--log-root', archive).stdout), json.loads(both.stdout))
        run('export-progress', *portable, '--destination', archive, success=False)
        capture, = (moved / 'logs').iterdir()
        capture.write_text('changed')
        refused = self.repo / 'refused'
        self.assertIn('hash changed', run('export-progress', *portable, '--destination', refused,
                                          success=False).stderr)
        self.assertFalse(refused.exists())

    def test_portable_and_local_rust_checks_share_the_default_format(self):
        implicit = self.record()
        explicit = copy.deepcopy(implicit)
        self.observation(explicit)['format'] = 'libtest'
        for name, first, second in [('implicit', implicit, explicit), ('explicit', explicit, implicit)]:
            with self.subTest(first=name):
                archive = self.repo / name
                EVIDENCE.export_progress([first], self.expected, archive)
                portable = EVIDENCE.read_record(archive / 'progress-record.json')
                local = copy.deepcopy(second)
                checks = local['features'][0]['scenarios'][0]['checks']
                original, = checks
                checks.extend([dict(original, command=[*original['command'], '--exact']),
                               dict(original, reason='Independently reviewed mapping.')])
                merged = self.repo / (name + '-merged')
                report = EVIDENCE.export_progress([portable, local], self.expected, merged, log_root=archive)
                replay = EVIDENCE.read_record(merged / 'progress-record.json')
                recorded = replay['features'][0]['scenarios'][0]['checks']
                first_check = self.observation(portable)
                expected = [first_check] + [dict(check, log=first_check['log']) for check in checks[1:]]
                self.assertEqual(recorded, expected)
                self.assertEqual(len(list((merged / 'logs').iterdir())), 1)
                self.assertEqual(EVIDENCE.progress([replay], self.expected, merged), report)

    def test_failed_progress_publication_leaves_no_visible_record(self):
        directory = self.repo / 'export'
        with patch.object(EVIDENCE.os, 'link', side_effect=OSError('publication interrupted')):
            with self.assertRaisesRegex(OSError, 'publication interrupted'):
                EVIDENCE.export_progress([self.record()], self.expected, directory)
        self.assertFalse((directory / 'progress-record.json').exists())
        self.assertFalse((directory / 'record.json').exists())
        self.assertEqual(len(list((directory / 'logs').iterdir())), 1)

    def test_progress_cli_rechecks_portable_primary_and_independent_local_parts(self):
        local = self.repo / 'part.json'
        part = self.record()
        local.write_text(json.dumps(part))
        destination = self.repo / 'portable'
        EVIDENCE.bundle(part, self.expected, destination)
        record = destination / 'record.json'
        command = [sys.executable, '-I', '-B', str(ALAB / 'feature_evidence.py'), 'progress',
                   '--repo', str(self.repo), '--source', self.source, '--upstream', self.source,
                   '--record', str(record), '--log-root', str(destination), '--part', str(local), '--part', str(local)]
        def run(*options):
            return subprocess.run([*command, *options], capture_output=True, text=True, timeout=10)
        done = run()
        self.assertEqual(done.returncode, 0, done.stderr)
        result = json.loads(done.stdout)
        self.assertEqual((result['source'], result['upstream'], result['scope']),
                         (self.source, self.source, 'inventory'))
        self.assertEqual((result['covered'], result['total']), (1, 1))
        summary = run('--summary')
        self.assertEqual(summary.returncode, 0, summary.stderr)
        self.assertEqual(summary.stdout.splitlines(), [f'ALab release scenario coverage for {self.source}',
                                                       '1 of 1 verified across 1 inventory feature contract',
                                                       'Pending scenarios   0'])
        # A valid portable record cannot conceal an invalid part, even on an already covered row.
        local.write_text(json.dumps(dict(part, source='other')))
        stale = run()
        self.assertEqual(stale.returncode, 1)
        self.assertIn('stale or invalid source', stale.stderr)
        self.assertEqual(stale.stdout, '')
        stale_summary = run('--summary')
        self.assertEqual((stale_summary.returncode, stale_summary.stdout), (1, ''))
        local.write_text(json.dumps(part))
        for log in (self.log, next((destination / 'logs').iterdir())):
            with self.subTest(log=log):
                original = log.read_bytes()
                log.write_bytes(original + b'tampered')
                corrupt = run()
                self.assertEqual(corrupt.returncode, 1)
                self.assertIn('captured log hash changed', corrupt.stderr)
                self.assertEqual(corrupt.stdout, '')
                log.write_bytes(original)

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

    def test_sgr_styles_preserve_real_named_boundaries_in_the_api_and_cli(self):
        original = self.log.read_text()
        colored = '\x1b[32mtest ownership::cancel\x1b[0m ... \x1b[32mok\x1b[0m'
        variants = [
            ('plain-prefix', 'ordinary visible text test ownership::cancel ... ok', 1),
            ('styled-prefix', 'ordinary visible text ' + colored, 1),
            ('styled-line', colored, 0),
            ('cursor-and-style', 'visible TUI frame\x1b[?25h' + colored, 0),
        ]
        path = self.repo / 'record.json'
        command = [sys.executable, '-I', '-B', str(ALAB / 'feature_evidence.py'), 'validate',
                   '--repo', str(self.repo), '--source', self.source,
                   '--upstream', self.source, '--record', str(path)]
        for name, line, status in variants:
            with self.subTest(variant=name):
                self.log.write_text(original.replace('test ownership::cancel ... ok', line))
                record = self.record()
                self.assertEqual(bool(EVIDENCE.validate(record, self.expected)), bool(status))
                path.write_text(json.dumps(record))
                done = subprocess.run(command, capture_output=True, text=True, timeout=10)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)

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

    def test_nul_command_arguments_are_refused_at_api_and_cli_entries(self):
        healthy = self.record()
        self.observation(healthy)['command'] = ['fixture-command', r'literal\0argument', 'café']
        self.assertEqual(EVIDENCE.validate(healthy, self.expected), [])
        self.assertEqual(EVIDENCE.combine([healthy], self.expected), healthy)
        path = self.repo / 'command-record.json'
        command = [sys.executable, '-I', '-B', str(ALAB / 'feature_evidence.py')]
        arguments = ['--repo', str(self.repo), '--source', self.source,
                     '--upstream', self.source, '--record', str(path)]
        path.write_text(json.dumps(healthy), encoding='utf-8')
        done = subprocess.run(command + ['validate'] + arguments,
                              capture_output=True, text=True, timeout=10)
        self.assertEqual(done.returncode, 0, done.stderr)
        for index, declared in enumerate((
                ['fixture\0command'], ['fixture-command', '\0'],
                ['fixture-command', 'prefix\0suffix'])):
            record = self.record()
            self.observation(record)['command'] = declared
            destination = self.repo / ('command-bundle-' + str(index))
            with self.subTest(command=declared):
                with patch.object(EVIDENCE, 'log_bytes',
                                  side_effect=AssertionError('invalid argv must precede log reads')):
                    self.assertTrue(EVIDENCE.validate(record, self.expected))
                    with self.assertRaisesRegex(ValueError, 'command must be'):
                        EVIDENCE.combine([record], self.expected)
                    with self.assertRaisesRegex(ValueError, 'command must be'):
                        EVIDENCE.bundle(record, self.expected, destination)
                self.assertFalse(destination.exists())
                path.write_text(json.dumps(record), encoding='utf-8')
                for operation in ('validate', 'bundle'):
                    with self.subTest(operation=operation):
                        extra = ['--destination', str(destination)] if operation == 'bundle' else []
                        done = subprocess.run(command + [operation] + arguments + extra,
                                              capture_output=True, text=True, timeout=10)
                        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                        self.assertIn('command must be a nonempty argument list', done.stderr)
                        self.assertFalse(destination.exists())

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
        command[2] = 'progress'
        pending = json.loads(subprocess.check_output(command))
        self.assertEqual((pending['covered'], pending['total']), (0, 1))
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

    def test_export_failure_retains_its_incomplete_destination(self):
        destination = self.repo / 'bundle'
        record = self.record()
        raw = self.log.read_bytes()
        with patch.object(EVIDENCE, 'log_bytes', side_effect=[raw, OSError('read failed')]):
            with self.assertRaisesRegex(OSError, 'read failed'):
                EVIDENCE.bundle(record, self.expected, destination)
        self.assertTrue((destination / 'logs').is_dir())
        self.assertFalse((destination / 'record.json').exists())
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

    def test_workers_can_fill_different_scenarios_of_one_snapshot(self):
        self.inventory.write_text(self.inventory.read_text().replace(
            '"Cancel during handoff."', '"Cancel during handoff.", "Preserve ownership."'))
        self.git('commit', '-qam', 'extend contract')
        self.source = self.git('rev-parse', 'HEAD')
        self.expected = EVIDENCE.snapshot(self.repo, self.source, self.source)
        first = self.record()
        second = copy.deepcopy(self.expected)
        second['features'][0]['scenarios'][1]['checks'].append(self.observation(first))
        files = [self.repo / name for name in ('first.json', 'second.json')]
        for path, part in zip(files, [first, second], strict=True):
            self.assertTrue(EVIDENCE.validate(part, self.expected))
            path.write_text(json.dumps(part))
        destination = self.repo / 'combined'
        command = [sys.executable, '-I', str(ALAB / 'feature_evidence.py'), 'bundle',
                   '--repo', str(self.repo), '--source', self.source, '--upstream', self.source,
                   '--record', str(files[0]), '--destination', str(destination)]
        incomplete = subprocess.run(command + ['--part', str(files[0])], capture_output=True)
        self.assertNotEqual(incomplete.returncode, 0)
        self.assertFalse(destination.exists())
        subprocess.run(command + ['--part', str(files[1]), '--part', str(files[0])],
                       capture_output=True, check=True)
        exported = EVIDENCE.read_record(destination / 'record.json')
        self.assertEqual(EVIDENCE.validate(exported, self.expected, destination), [])
        self.assertEqual([len(s['checks']) for s in exported['features'][0]['scenarios']], [1, 1])

    def test_combining_refuses_a_different_scope_and_checks_every_observation(self):
        scoped = dict(self.expected, base=self.source)
        valid = dict(self.record(), base=self.source)
        for expected, part in [(self.expected, valid), (scoped, self.record()),
                               (scoped, dict(valid, base='other')),
                               (scoped, dict(valid, source='other')),
                               (scoped, dict(valid, upstream='other'))]:
            with self.subTest(expected=expected.get('base'), part=part.get('source')):
                with self.assertRaises(ValueError):
                    EVIDENCE.combine([part], expected)
        changed = copy.deepcopy(valid)
        self.observation(changed)['tests'] = ['ownership::never_ran']
        with self.assertRaises(ValueError):
            EVIDENCE.combine([valid, changed], scoped)
        additional = copy.deepcopy(valid)
        log = self.repo / 'retained.log'
        log.write_bytes(self.log.read_bytes().replace(b'ownership::cancel', b'ownership::retain'))
        self.observation(additional).update(
            tests=['ownership::retain'], log=str(log), log_sha256=hashlib.sha256(log.read_bytes()).hexdigest(),
            reason='The second fixture observes the retained owner.')
        union = EVIDENCE.combine([valid, additional, valid], scoped)
        self.assertEqual(len(union['features'][0]['scenarios'][0]['checks']), 2)
        self.assertEqual(len(valid['features'][0]['scenarios'][0]['checks']), 1)

    def test_direct_validation_and_bundle_keep_the_original_scope(self):
        unscoped = self.record()
        scoped = dict(unscoped, base=self.source)
        expected_scoped = dict(self.expected, base=self.source)
        for name, expected, record in [
            ('unscoped', self.expected, unscoped),
            ('scoped', expected_scoped, scoped),
        ]:
            with self.subTest(healthy=name):
                self.assertEqual(EVIDENCE.validate(record, expected), [])
                self.assertEqual(EVIDENCE.combine([record], expected), record)
                destination = self.repo / (name + '-bundle')
                EVIDENCE.bundle(record, expected, destination)
                exported = EVIDENCE.read_record(destination / 'record.json')
                self.assertEqual(EVIDENCE.validate(exported, expected, destination), [])
        invalid = [
            ('missing', expected_scoped, unscoped),
            ('different', expected_scoped, dict(scoped, base='other')),
        ] + [
            ('unexpected-' + str(index), self.expected, dict(unscoped, base=base))
            for index, base in enumerate((self.source, 'other', None, True, ''))
        ]
        for name, expected, record in invalid:
            with self.subTest(refused=name):
                errors = EVIDENCE.validate(record, expected)
                self.assertTrue(errors)
                if name.startswith('unexpected-'):
                    self.assertIn('scoped evidence requires its original base', errors)
                destination = self.repo / (name + '-bundle')
                with self.assertRaises(ValueError):
                    EVIDENCE.bundle(record, expected, destination)
                self.assertFalse(destination.exists())
                with self.assertRaises(ValueError):
                    EVIDENCE.combine([record], expected)

    def test_cli_refuses_scoped_evidence_without_its_original_base(self):
        path = self.repo / 'scope-record.json'
        command = [sys.executable, '-I', '-B', str(ALAB / 'feature_evidence.py')]
        arguments = ['--repo', str(self.repo), '--source', self.source,
                     '--upstream', self.source, '--record', str(path)]
        path.write_text(json.dumps(self.record()))
        healthy = subprocess.run(command + ['validate'] + arguments,
                                 capture_output=True, text=True, timeout=10)
        self.assertEqual(healthy.returncode, 0, healthy.stderr)
        for index, base in enumerate((self.source, None)):
            path.write_text(json.dumps(dict(self.record(), base=base)))
            for operation in ('validate', 'bundle'):
                with self.subTest(operation=operation, base=base):
                    destination = self.repo / ('scope-cli-' + str(index))
                    extra = ['--destination', str(destination)] if operation == 'bundle' else []
                    done = subprocess.run(command + [operation] + arguments + extra,
                                          capture_output=True, text=True, timeout=10)
                    self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                    self.assertIn('scoped evidence requires its original base', done.stderr)
                    self.assertFalse(destination.exists())

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

    def test_python_evidence_refuses_a_gracefully_stopped_selection(self):
        executed = []

        class Stopping(unittest.TestCase):
            def test_one(self):
                executed.append('first')

            def run(self, result=None):
                result = super().run(result)
                result.stop()
                return result

        class Later(unittest.TestCase):
            def test_two(self):
                executed.append('second')
                self.fail('the selected failure must not disappear')

        result = execute_python(unittest.TestSuite([Stopping('test_one'), Later('test_two')]))
        self.assertEqual(executed, ['first'])
        self.assertEqual(result['tests_run'], 1)
        self.assertEqual(result['tests'][0]['status'], 'passed')
        self.log.write_text(json.dumps(result))
        record = self.record()
        self.observation(record).update(format='python-unittest', status=result['exit_status'],
                                        tests=[result['tests'][0]['id']])
        self.assertTrue(EVIDENCE.validate(record, self.expected))
        # Even an independently misreported command success cannot qualify the
        # producer's retained incomplete record through the existing consumer.
        self.observation(record)['status'] = 0
        self.assertTrue(EVIDENCE.validate(record, self.expected))

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
