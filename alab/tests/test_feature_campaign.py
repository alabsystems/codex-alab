# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import hashlib
import json
import os
import re
import runpy
import shlex
import shutil
import subprocess
from pathlib import Path
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch

import test_feature_gate as fixtures

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import feature_campaign as campaign
from feature_recipes import request as requests
from feature_recipes.manifest import required_libtests, required_tests


class NativeRequestTests(unittest.TestCase):
    git = fixtures.FeatureAdmissionTests.git
    commit = fixtures.FeatureAdmissionTests.commit
    reviewed = fixtures.FeatureAdmissionTests.reviewed
    persist = fixtures.FeatureAdmissionTests.persist
    review = fixtures.FeatureAdmissionTests.review

    def setUp(self):
        fixtures.FeatureAdmissionTests.setUp(self)
        self.review()
        self.record_native_request()

    def record_native_request(self):
        self.source = self.git('rev-parse', 'HEAD')
        self.context = requests.context(self.repo, self.base)
        self.root = requests.common_dir(self.repo) / 'alab-qualification' / self.source / 'run-test'
        self.root.mkdir(parents=True)
        self.path = self.root / 'record.json'
        self.report = fixtures.GATE.admission(self.repo, self.base)
        self.set_record()

    def set_record(self, log=None, **changes):
        raw = ((log or json.dumps(self.report, indent=2)) + '\nalab: feature evidence admission failed; complete the exact-source affected-feature bundle\n').encode()
        (self.root / 'gate.log').write_bytes(raw)
        record = dict(schema=requests.SCHEMA, kind='native-check-execution-observation',
                      before=self.context, after=self.context, qualified_source=None, status=1,
                      log='gate.log', log_sha256=hashlib.sha256(raw).hexdigest(),
                      command=[str(self.repo / 'alab/check.sh')])
        self.path.write_text(json.dumps({**record, **changes}))

    def test_actual_frozen_review_can_be_described_after_another_origin_commit(self):
        self.git('update-ref', 'refs/remotes/origin/main', self.source)
        frozen, result, observed = requests.request(self.repo)
        self.assertEqual(frozen, self.context)
        self.assertEqual(result, self.report)
        self.assertEqual(observed, self.path)
        with tempfile.TemporaryDirectory() as folder, patch.object(campaign, 'RECIPES', Path(folder)):
            (Path(folder) / 'scenarios.json').write_text('{"features": {}, "groups": {}}')
            described = campaign.campaign(self.repo, self.path, None, False)
            self.assertEqual(described['unsupported_features'], ['model-discovery'])
            self.assertEqual(described['stages'], [])
            self.assertFalse(described['admitted'])
            with self.assertRaisesRegex(ValueError, 'unsupported feature scope'):
                campaign.campaign(self.repo, self.path, None, True)
        with self.assertRaisesRegex(ValueError, 'scenario contract changed for model-discovery'):
            campaign.campaign(self.repo, self.path, None, False)

    def stopped_recovery_fixture(self, *, alternate_producer=False, published=True, separate_clone=True):
        tooling = self.repo.resolve() / 'alab'
        recipes = tooling / 'feature_recipes'
        recipes.mkdir(exist_ok=True)
        for name in ('feature_campaign.py', 'tests/test_proof_build.py', 'lib.sh', 'package.sh',
                     'python_test_results.py', 'cli_status.py', 'feature_recipes/run.sh'):
            path = tooling / name
            path.parent.mkdir(parents=True, exist_ok=True)
            if not path.exists():
                path.write_text('fixture recipe input\n')
        manifest = {'features': {'model-discovery': [
            {'scenario': 'Fresh catalog.', 'groups': ['catalog'], 'coverage': 'Read fresh catalog.'},
            {'scenario': 'Unavailable account.', 'groups': ['catalog', 'worker'], 'coverage': 'Read and stop.'}]},
            'groups': {'catalog': {'stage': 'boot', 'tests': ['fixture::catalog']},
                       'worker': {'stage': 'app-server', 'tests': ['fixture::worker']}}}
        if alternate_producer:
            inventory = tooling / 'features.toml'
            inventory.write_text(inventory.read_text().replace('"Unavailable account."]',
                                 '"Unavailable account.", "Candidate has no recipe.", "Pending worker."]'))
            producer_rows = manifest['features']['model-discovery']
            producer_rows[1] = {'scenario': 'Unavailable account.', 'groups': ['catalog'], 'coverage': 'Read catalog.'}
            producer_rows.extend([
                {'scenario': 'Candidate has no recipe.', 'groups': ['catalog'], 'coverage': 'Read catalog.'},
                {'scenario': 'Pending worker.', 'groups': ['catalog', 'worker'], 'coverage': 'Read and stop.'}])
        candidate = copy.deepcopy(manifest)
        if alternate_producer:
            candidate['groups']['extra'] = {'stage': 'boot', 'tests': ['fixture::also_required']}
            candidate['features']['model-discovery'][1]['groups'].append('extra')
            candidate['features']['model-discovery'][2]['groups'] = []
        (recipes / 'scenarios.json').write_text(json.dumps(candidate))
        self.git('add', 'alab')
        self.commit('checked fixture collector and candidate recipe')
        self.record_native_request()
        self.output = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve() / 'recovered.json'
        self.original_producer = self.repo.resolve()
        if alternate_producer:
            self.original_producer = self.output.parent / 'original-collector'
            if separate_clone:
                self.git('clone', '--quiet', '--no-hardlinks', str(self.repo), str(self.original_producer))
            else:
                self.git('worktree', 'add', '--quiet', '--detach', str(self.original_producer), self.source)
            tooling = self.original_producer / 'alab'
            recipes = tooling / 'feature_recipes'
            (recipes / 'scenarios.json').write_text(json.dumps(manifest))
            self.git('-C', str(self.original_producer), 'add', 'alab')
            self.git('-C', str(self.original_producer), '-c', 'user.name=Fixture', '-c', 'user.email=test@example.org',
                     'commit', '--quiet', '-m', 'supplemental producer')
            self.git('fetch', '--quiet', str(self.original_producer), 'HEAD')
            if published:
                self.git('update-ref', 'refs/remotes/origin/main', self.git('rev-parse', 'FETCH_HEAD'))

        def drive(argv, cwd, log):
            self.original_run = log.parents[2]
            if argv[5] == 'app-server':
                log.write_text('resource stop fixture\n')
                return 75
            self.assertEqual(argv[5:], ['boot', 'fixture::catalog'])
            log.write_text('running 1 test\ntest fixture::catalog ... ok\n'
                           'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; '
                           '0 filtered out; finished in 0.01s\n')
            return 0

        with patch.object(campaign, 'HERE', tooling), patch.object(campaign, 'RECIPES', recipes), patch.object(
                campaign, 'run_stage', side_effect=drive), self.assertRaises(campaign.ExecutionFailure) as failed:
            numbers = list(range(1, len(manifest['features']['model-discovery']) + 1))
            campaign.campaign(self.repo, self.path, None, True, feature_id='model-discovery', scenario_numbers=numbers)
        self.assertEqual(failed.exception.status, 75)

    def recover(self, *, producer=None):
        with patch.object(campaign, 'run_stage', side_effect=AssertionError('recovery launched a stage')):
            return campaign.recover_part(self.repo, self.path, self.original_run, producer or self.original_producer, self.output)

    def test_recover_only_complete_rows_preserves_failed_status_and_is_idempotent(self):
        self.stopped_recovery_fixture()
        failed_receipt = self.original_run / 'app-server/1/result.json'
        original = failed_receipt.read_bytes()
        report = self.recover()
        self.assertEqual((report['recovered_scenarios'], report['unresolved_scenarios']), ([1], [2]))
        self.assertEqual((report['stopped_stage'], report['stopped_status']), ('app-server', 75))
        data = self.output.read_bytes()
        first, second = json.loads(data)['features'][0]['scenarios']
        self.assertEqual(first['checks'][0]['tests'], ['fixture::catalog'])
        self.assertEqual(second['checks'], [])
        self.assertEqual(self.recover(), report)
        self.assertEqual(self.output.read_bytes(), data)
        self.assertEqual(failed_receipt.read_bytes(), original)
        self.assertFalse((self.original_run / 'part.json').exists())
        self.assertFalse(Path(self.report['bundle']).exists())

    def test_recover_authenticates_recorded_scope_producer_and_stage_requests(self):
        self.stopped_recovery_fixture()
        inputs = self.original_run / 'inputs.json'
        original = campaign.read_record(inputs)
        for changes in [{'snapshot': {**original['snapshot'], 'source': '0' * 40}},
                        {'snapshot': {**original['snapshot'], 'base': 'f' * 40}},
                        {'recipe_sha256': 'f' * 64}, {'selected_scenarios': [1]}]:
            with self.subTest(changes=list(changes)), self.assertRaisesRegex(ValueError, 'scope'):
                inputs.write_text(json.dumps({**original, **changes}))
                self.recover()
        inputs.write_text(json.dumps(original))
        with self.assertRaisesRegex(ValueError, 'stage request changed'):
            self.recover(producer=Path(original['workspace']))
        folder = self.original_run / 'boot/1'
        request_path = folder / 'request.json'
        stage = campaign.read_record(request_path)
        request_path.write_text(json.dumps({**stage, 'source': 'a' * 40}))
        with self.assertRaisesRegex(ValueError, 'stage request changed'):
            self.recover()
        self.assertFalse(self.output.exists())

    def test_recover_does_not_credit_finished_log_text_from_a_stopped_stage(self):
        self.stopped_recovery_fixture()
        (self.original_run / 'app-server').rename(self.output.parent / 'later-fixture')
        receipt = self.original_run / 'boot/1/result.json'
        data = campaign.read_record(receipt)
        data.pop('test_log_sha256')
        receipt.write_text(json.dumps({**data, 'status': 75}))
        report = self.recover()
        self.assertEqual((report['recovered_scenarios'], report['unresolved_scenarios']), ([], [1, 2]))
        self.assertEqual((report['stopped_status'], report['part']), (75, None))
        self.assertFalse(self.output.exists())

    def test_recover_demands_committed_recipe_helpers_even_if_git_ignores_them(self):
        self.stopped_recovery_fixture()
        exclude = self.repo / '.git/info/exclude'
        with exclude.open('a') as stream:
            stream.write('\n/alab/feature_recipes/local-fixture.py\n')
        (self.repo / 'alab/feature_recipes/local-fixture.py').write_text('# untracked, ignored fixture\n')
        with self.assertRaisesRegex(ValueError, 'absent from the original producer commit'):
            self.recover()
        self.assertFalse(self.output.exists())

    def test_recover_matches_the_candidate_selection_independently_on_each_completed_row(self):
        self.stopped_recovery_fixture(alternate_producer=True, separate_clone=False)
        self.assertEqual(requests.common_dir(self.original_producer), requests.common_dir(self.repo))
        report = self.recover()
        self.assertEqual(report['recovered_scenarios'], [1])
        self.assertEqual(report['unresolved_scenarios'], [2, 3, 4])
        rows = campaign.read_record(self.output)['features'][0]['scenarios']
        self.assertEqual([bool(row['checks']) for row in rows], [True, False, False, False])

    def test_recover_one_matching_row_from_a_published_separate_producer(self):
        self.stopped_recovery_fixture(alternate_producer=True)
        self.assertNotEqual(requests.common_dir(self.original_producer), requests.common_dir(self.repo))
        report = self.recover()
        self.assertEqual(report['checked_producer'], self.git('rev-parse', 'refs/remotes/origin/main'))
        self.assertEqual((report['recovered_scenarios'], report['unresolved_scenarios']), ([1], [2, 3, 4]))
        rows = campaign.read_record(self.output)['features'][0]['scenarios']
        self.assertEqual(rows[0]['checks'][0]['tests'], ['fixture::catalog'])
        # The original collector ran catalog for the first three rows. The
        # candidate required an extra name on row 2 and had no recipe on row 3.
        self.assertTrue(all(row['checks'] == [] for row in rows[1:]))

    def test_recover_refuses_a_separate_producer_outside_trusted_private_history(self):
        self.stopped_recovery_fixture(alternate_producer=True, published=False)
        with self.assertRaisesRegex(ValueError, 'published private history'):
            self.recover()
        self.assertFalse(self.output.exists())

    def test_recover_refuses_missing_signalled_and_mutated_stage_observations(self):
        self.stopped_recovery_fixture()
        failure = self.original_run / 'app-server/1/result.json'
        original = failure.read_bytes()
        failure.unlink()
        with self.assertRaisesRegex(ValueError, 'no completion status'):
            self.recover()
        failure.write_text(json.dumps({**json.loads(original), 'status': 143}))
        with self.assertRaisesRegex(ValueError, 'signal'):
            self.recover()
        failure.write_bytes(original)
        folder = self.original_run / 'boot/1'
        result = folder / 'result.json'
        good = campaign.read_record(result)
        log = folder / 'combined.log'
        log.write_bytes(log.read_bytes().replace(b'fixture::catalog', b'fixture::incorrect'))
        with self.assertRaisesRegex(ValueError, 'output changed'):
            self.recover()
        result.write_text(json.dumps({**good, 'output_sha256': campaign.digest(log),
                                      'test_log_sha256': campaign.digest(log)}))
        with self.assertRaisesRegex(ValueError, 'selected test'):
            self.recover()
        self.assertFalse(self.output.exists())

    def test_one_feature_part_retains_the_full_gate_scope_and_can_be_imported(self):
        with (self.repo / 'alab/features.toml').open('a') as stream:
            stream.write('[[features]]\nid = "history-pagination"\nlifecycle = "active"\n'
                         'scenarios = ["Complete history."]\n')
        self.git('add', 'alab/features.toml')
        self.commit('also require history coverage')
        review = json.loads((self.repo / 'alab/upstream/one-intake.json').read_bytes())
        review['commits'][0]['features'] = ['model-discovery', 'history-pagination']
        self.persist(review)
        self.record_native_request()
        recipes = Path(self.enterContext(tempfile.TemporaryDirectory()))
        fixture_recipe = {'features': {'model-discovery': [
            {'scenario': label, 'groups': ['catalog'], 'coverage': 'Catalog fixture.'}
            for label in ('Fresh catalog.', 'Unavailable account.')]},
            'groups': {'catalog': {'stage': 'boot', 'tests': ['fixture::catalog']}}}
        manifest = recipes / 'scenarios.json'
        manifest.write_text(json.dumps(fixture_recipe))

        def run_fixture(argv, workspace, log):
            (name,) = argv[6:]
            log.write_text(f'running 1 test\ntest {name} ... ok\n'
                           'test result: ok. 1 passed; 0 failed; 0 ignored; '
                           '0 measured; 0 filtered out; finished in 0.01s\n')
            return 0

        with patch.object(campaign, 'RECIPES', recipes), patch.object(
                campaign, 'run_stage', side_effect=run_fixture) as execute:
            for feature, error in [('account-isolation', 'outside the recorded gate scope'),
                                   ('history-pagination', 'unsupported feature scope')]:
                with self.subTest(feature=feature), self.assertRaisesRegex(ValueError, error):
                    campaign.campaign(self.repo, self.path, None, True, feature_id=feature)
            execute.assert_not_called()
            summary = campaign.campaign(self.repo, self.path, None, True, feature_id='model-discovery')
            self.assertFalse(summary['admitted'])
            self.assertEqual(summary['unsupported_features'], ['history-pagination'])
            part = Path(summary['part'])
            record = campaign.read_record(part)
            catalog, history = record['features']
            self.assertEqual(catalog['id'], 'model-discovery')
            self.assertTrue(all(r['checks'][0]['tests'] == ['fixture::catalog'] for r in catalog['scenarios']))
            self.assertEqual(history['scenarios'], [{'description': 'Complete history.', 'checks': []}])
            self.assertFalse(Path(self.report['bundle']).exists())
            self.assertEqual(campaign.campaign(self.repo, self.path, None, True,
                                               feature_id='model-discovery')['part'], str(part))
            original = part.read_bytes()
            part.write_bytes(b'{}\n')
            with self.assertRaisesRegex(ValueError, 'existing feature artifact differs'):
                campaign.campaign(self.repo, self.path, None, True, feature_id='model-discovery')
            self.assertEqual(part.read_bytes(), b'{}\n')
            self.assertFalse(Path(self.report['bundle']).exists())
            part.write_bytes(original)
            self.assertEqual(execute.call_count, 1)
            with self.assertRaisesRegex(ValueError, 'unsupported feature scope'):
                campaign.campaign(self.repo, self.path, None, True, parts=[part])
            fixture_recipe['features']['history-pagination'] = [
                {'scenario': 'Complete history.', 'groups': ['history'], 'coverage': 'History fixture.'}]
            fixture_recipe['groups']['history'] = {'stage': 'tui', 'tests': ['fixture::history']}
            manifest.write_text(json.dumps(fixture_recipe))
            result = campaign.campaign(self.repo, self.path, None, True, parts=[part])
            self.assertTrue(result['admitted'])
            self.assertEqual(execute.call_count, 2)
            self.assertEqual(execute.call_args.args[0][5:], ['tui', 'fixture::history'])
            self.assertEqual(fixtures.GATE.admission(self.repo, self.base)['errors'], [])

    def test_campaign_identifies_the_manifest_bytes_it_planned(self):
        with tempfile.TemporaryDirectory() as folder, patch.object(campaign, 'RECIPES', Path(folder)):
            manifest = Path(folder) / 'scenarios.json'
            manifest.write_text('{"features": {}, "groups": {}}')
            scenarios = [{'scenario': label, 'groups': ['wire'], 'coverage': 'Fixture only.'}
                         for label in ('Fresh catalog.', 'Unavailable account.')]
            updated = {'features': {'model-discovery': scenarios},
                       'groups': {'wire': {'stage': 'model-wire', 'tests': ['fixture']}}}

            def describe():
                return campaign.campaign(self.repo, self.path, None, False)

            before = describe()
            snapshot = campaign.FILE_HASH['file_bytes_snapshot']

            def replace_after_read(path):
                observed = snapshot(path)
                if path == manifest:
                    replacement = manifest.with_suffix('.new')
                    replacement.write_text(json.dumps(updated))
                    replacement.replace(manifest)
                return observed

            with patch.dict(campaign.FILE_HASH, file_bytes_snapshot=replace_after_read):
                racing = describe()
            self.assertEqual(racing['recipe_sha256'], before['recipe_sha256'])
            after = describe()
            self.assertEqual(racing['unsupported_features'], ['model-discovery'])
            self.assertEqual(after['stages'], ['model-wire'])
            self.assertNotEqual(after['recipe_sha256'], before['recipe_sha256'])

    def test_duplicate_recipe_fields_refuse_campaign_planning(self):
        recipes = (
            ('model-discovery', '{"features":{"model-discovery":[],"model-discovery":[]},"groups":{}}'),
            ('tests', '{"features":{},"groups":{"unselected":{"tests":["first"],"tests":["last"]}}}'),
        )
        with tempfile.TemporaryDirectory() as folder, patch.object(campaign, 'RECIPES', Path(folder)):
            for key, raw in recipes:
                (Path(folder) / 'scenarios.json').write_text(raw)
                for run in (False, True):
                    with self.subTest(key=key, run=run):
                        with self.assertRaisesRegex(ValueError, f'duplicate feature recipe key {key!r}'):
                            campaign.campaign(self.repo, self.path, None, run)

    def install_sync_gate(self):
        repo = self.repo.resolve()
        for name in ('qualification.py', 'cli_status.py', 'file_hash.py', 'upstream_intake.py',
                     'source_content.py'):
            shutil.copyfile(campaign.HERE / name, repo / 'alab' / name)
        gate = repo / 'alab/check.sh'
        gate.write_text(
            f'#!/bin/sh\nexec {shlex.quote(sys.executable)} - "$@" <<\'GATE_PYTHON\'\n'
            'import json, sys\nfrom pathlib import Path\n'
            f'sys.path.insert(0, {str(campaign.HERE)!r})\nsys.dont_write_bytecode = True\n'
            'from feature_gate import admission\n'
            'report = admission(Path.cwd(), "refs/remotes/origin/main")\n'
            'print(json.dumps(report, indent=2))\n'
            'if report["errors"]:\n'
            '    print("alab: feature evidence admission failed; complete the exact-source affected-feature bundle")\n'
            '    sys.exit(1)\n'
            'sys.exit(6)\nGATE_PYTHON\n')
        gate.chmod(0o700)
        self.git('add', 'alab')
        self.commit('native gate fixture')
        self.git('update-ref', 'refs/remotes/origin/main', self.base)
        self.git('update-ref', 'refs/remotes/upstream/main', self.target)
        return repo

    def sync_recipes(self):
        recipes = Path(self.enterContext(tempfile.TemporaryDirectory()))
        recipe = {'scenario': 'Fresh catalog.', 'groups': ['control'], 'coverage': 'Fixture only.'}
        (recipes / 'scenarios.json').write_text(json.dumps({
            'features': {'model-discovery': [recipe, {**recipe, 'scenario': 'Unavailable account.'}]},
            'groups': {'control': {'stage': 'boot', 'tests': ['fixture::catalog']}}}))
        return recipes

    def run_catalog_fixture(self, argv, workspace, output):
        self.assertEqual(argv[5:], ['boot', 'fixture::catalog'])
        output.write_text('running 1 test\ntest fixture::catalog ... ok\n'
                          'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; '
                          '0 filtered out; finished in 0.01s\n')
        return 0

    def test_sync_leaves_a_divergent_private_base_for_reconciliation(self):
        repo = self.install_sync_gate()
        peer = self.git('commit-tree', self.base + '^{tree}', '-p', self.base, '-m', 'private change')
        self.git('update-ref', 'refs/remotes/origin/main', peer, self.base)
        with patch.object(campaign, 'RECIPES', self.sync_recipes()), patch.object(
                campaign, 'execution_workspace', side_effect=AssertionError('allocated a workspace')):
            self.assertEqual(campaign.sync_check(repo), 1)
        record = requests.request(repo)
        self.assertEqual(record[0]['base'], peer)
        self.assertEqual(record[0]['source'], self.git('rev-parse', 'HEAD'))
        self.assertTrue(record[1]['errors'])
        self.assertFalse(Path(record[1]['bundle']).exists())
        with patch.object(campaign, 'RECIPES', self.sync_recipes()), patch.object(
                campaign, 'run_stage', side_effect=self.run_catalog_fixture):
            standalone = campaign.campaign(repo, record[2], None, True)
        self.assertTrue(standalone['admitted'])
        self.assertEqual(fixtures.GATE.admission(repo, peer)['errors'], [])

    def test_sync_records_the_completed_stage_when_private_main_advances(self):
        repo = self.install_sync_gate()
        peer = self.git('commit-tree', self.base + '^{tree}', '-p', self.base, '-m', 'private change')

        def advance_main(argv, workspace, output):
            output.write_text('fixture stage completed\n')
            self.git('update-ref', 'refs/remotes/origin/main', peer, self.base)
            return 0

        with patch.object(campaign, 'RECIPES', self.sync_recipes()), patch.object(
                campaign, 'run_stage', side_effect=advance_main) as executed:
            self.assertEqual(campaign.sync_check(repo), 1)
            assembled = campaign.campaign(repo, None, None, False)
        self.assertEqual(executed.call_count, 1)
        self.assertEqual(assembled['context']['base'], self.base)
        self.assertEqual(assembled['stages'], ['boot'])
        common = requests.common_dir(repo)
        [result] = common.glob('alab-feature-campaign/*/*/*/boot/1/result.json')
        stage = json.loads(result.read_text())
        self.assertEqual(stage['status'], 0)
        self.assertIn('private main advanced', stage['invalidated'])
        self.assertFalse((common / 'alab-feature-evidence' / self.git('rev-parse', 'HEAD')).exists())

    def test_sync_collects_the_exact_request_then_returns_the_second_native_gate_status(self):
        repo = self.install_sync_gate()
        source = self.git('rev-parse', 'HEAD')
        with patch.object(campaign, 'RECIPES', self.sync_recipes()), patch.object(
                campaign, 'run_stage', side_effect=self.run_catalog_fixture) as executed:
            self.assertEqual(campaign.sync_check(repo), 6)
        self.assertEqual(executed.call_count, 1)
        records = [json.loads(p.read_bytes()) for p in (
            requests.common_dir(repo) / 'alab-qualification' / source).glob('run-*/record.json')]
        self.assertCountEqual([r['status'] for r in records], [1, 6])
        self.assertTrue(all(r['qualified_source'] is None for r in records))
        self.assertEqual(fixtures.GATE.admission(repo, self.base)['errors'], [])

    def test_sync_collects_all_runnable_rows_once_and_leaves_missing_recipes_pending(self):
        with (self.repo / 'alab/features.toml').open('a') as stream:
            stream.write('[[features]]\nid = "history-pagination"\nlifecycle = "active"\n'
                         'scenarios = ["Complete history."]\n')
        self.git('add', 'alab/features.toml')
        self.commit('history contract')
        review = json.loads((self.repo / 'alab/upstream/one-intake.json').read_bytes())
        review['commits'][0]['features'].append('history-pagination')
        self.persist(review)
        repo = self.install_sync_gate()
        recipes = self.sync_recipes()
        manifest = recipes / 'scenarios.json'
        recipe = json.loads(manifest.read_bytes())
        fresh, unavailable = recipe['features']['model-discovery']
        unavailable['groups'] = []
        recipe['features']['history-pagination'] = [{**fresh, 'scenario': 'Complete history.'}]
        manifest.write_text(json.dumps(recipe))
        with patch.object(campaign, 'RECIPES', recipes), patch.object(
                campaign, 'run_stage', side_effect=self.run_catalog_fixture) as executed:
            self.assertEqual(campaign.sync_check(repo), 1)
            source, _, record = requests.request(repo)
            result = campaign.campaign(repo, record, None, True, collect_ready=True)
            self.assertEqual(result['scenarios_without_recipes'], {'model-discovery': [2]})
            self.assertFalse(result['admitted'])
            self.assertEqual(executed.call_count, 1)
            part = Path(result['part'])
            observed = json.loads(part.read_bytes())
            passed = {f['id']: [bool(s['checks']) for s in f['scenarios']] for f in observed['features']}
            self.assertEqual(passed, {'model-discovery': [True, False], 'history-pagination': [True]})
            self.assertFalse(fixtures.GATE.bundle_path(repo, source['source']).exists())
            recipe['features']['model-discovery'][1] = {**fresh, 'scenario': 'Unavailable account.'}
            manifest.write_text(json.dumps(recipe))
            complete = campaign.campaign(repo, record, None, True, parts=[part])
            self.assertTrue(complete['admitted'])
            self.assertEqual(executed.call_count, 2)
            self.assertEqual(fixtures.GATE.admission(repo, self.base)['errors'], [])

    def test_empty_recipes_do_not_hide_a_changed_contract_or_allocate_useless_work(self):
        recipes = self.sync_recipes()
        manifest = recipes / 'scenarios.json'
        row = {'scenario': 'Fresh catalog.', 'groups': []}
        with patch.object(campaign, 'RECIPES', recipes), patch.object(
                campaign, 'execution_workspace', side_effect=AssertionError('allocated workspace')):
            for features in ({}, {'model-discovery': [row, {**row, 'scenario': 'Unavailable account.'}]}):
                manifest.write_text(json.dumps({'features': features, 'groups': {}}))
                result = campaign.campaign(self.repo, self.path, None, True, collect_ready=True)
                self.assertEqual(result['scenarios_without_recipes'], {'model-discovery': [1, 2]})
                self.assertFalse(result['admitted'])
                self.assertNotIn('part', result)
            features['model-discovery'][1]['scenario'] = 'Renamed contract.'
            manifest.write_text(json.dumps({'features': features, 'groups': {}}))
            with self.assertRaisesRegex(ValueError, 'scenario contract changed'):
                campaign.campaign(self.repo, self.path, None, True, collect_ready=True)

    def test_feature_workspace_contention_retains_a_temporary_status(self):
        lockfile = self.root / 'workspace.lock'
        with campaign.lock(lockfile), self.assertRaises(campaign.ExecutionFailure) as caught:
            with campaign.lock(lockfile):
                self.fail('a concurrent collector entered the workspace')
        self.assertEqual(caught.exception.status, 75)

    def test_mismatched_state_wrong_gate_stale_digest_and_corrupt_bundle_are_refused(self):
        invalid = [
            dict(after={**self.context, 'base': self.source}),
            dict(command=['a different command']),
            dict(log_sha256='0' * 64),
            dict(log=json.dumps({**self.report, 'errors': ['different failure']})),
        ]
        for changes in invalid:
            with self.subTest(changes=changes):
                self.set_record(**changes)
                with self.assertRaises(ValueError):
                    requests.request(self.repo)
        self.set_record()
        bundle = Path(self.report['bundle'])
        bundle.mkdir(parents=True)
        (bundle / 'record.json').write_text('{}')
        with self.assertRaisesRegex(ValueError, 'differs'):
            requests.request(self.repo)

    def interrupted_publication(self):
        observed = copy.deepcopy(self.expected)
        destination = Path(self.report['bundle'])
        (destination / 'logs').mkdir(parents=True)
        logs = []
        for row, name in zip(observed['features'][0]['scenarios'], ('catalog', 'account'), strict=True):
            capture = self.root / (name + '.log')
            raw = (f'running 1 test\ntest fixture::{name} ... ok\n'
                   'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; '
                   '0 filtered out; finished in 0.00s\n').encode()
            capture.write_bytes(raw)
            digest = hashlib.sha256(raw).hexdigest()
            row['checks'] = [dict(source=self.source, status=0, command=['validator-fixture'],
                                  tests=['fixture::' + name], reason='Synthetic validator test only.',
                                  log=str(capture), log_sha256=digest)]
            logs.append((destination / 'logs' / (digest + '.log'), raw))
        (prior, prior_raw), fresh = logs
        prior.write_bytes(prior_raw)
        return observed, destination / 'record.json', (prior, prior_raw), fresh

    def intercept_log_writes(self, directory, intercept):
        open_file = Path.open

        def open_log(path, mode='r', *args, **kwargs):
            stream = open_file(path, mode, *args, **kwargs)
            if (path.suffix == '.log' and path.is_relative_to(directory)
                    and 'b' in mode and any(c in mode for c in 'xwa')):
                write = stream.write
                stream.write = lambda data: intercept(path, write, data)
            return stream

        return patch.object(Path, 'open', open_log)

    def test_record_is_published_last_and_only_new_log_bytes_are_staged(self):
        observed, record, (prior, prior_raw), (fresh, fresh_raw) = self.interrupted_publication()
        original = prior.stat()
        before_record = []
        written = bytearray()

        def count_writes(_path, write, data):
            size = write(data)
            written.extend(data[:size])
            return size

        def observe_publication():
            if not record.exists():
                before_record.append({p.name: p.read_bytes() for p in prior.parent.iterdir()})

        self.assertTrue(fixtures.GATE.admission(self.repo, self.base)['errors'])
        with self.intercept_log_writes(record.parent.parent, count_writes):
            result = campaign.publish(self.repo, self.context, observed, self.expected, observe_publication)
        self.assertEqual(result['errors'], [])
        self.assertIn({prior.name: prior_raw, fresh.name: fresh_raw}, before_record)
        self.assertEqual(len(written), len(fresh_raw))
        self.assertEqual(written, fresh_raw)
        prior_stat = prior.stat()
        self.assertEqual((prior_stat.st_ino, prior_stat.st_mtime_ns),
                         (original.st_ino, original.st_mtime_ns))
        contents = record.read_bytes()
        campaign.publish(self.repo, self.context, observed, self.expected)
        self.assertEqual(record.read_bytes(), contents)

    def test_damaged_retained_log_refuses_publication_and_preserves_the_original(self):
        observed, record, (prior, _), _ = self.interrupted_publication()
        prior.write_bytes(b'damaged retained evidence')
        inode = prior.stat().st_ino
        with self.assertRaises(ValueError):
            campaign.publish(self.repo, self.context, observed, self.expected)
        self.assertFalse(record.exists())
        self.assertEqual(prior.stat().st_ino, inode)
        self.assertEqual(prior.read_bytes(), b'damaged retained evidence')

    def test_damaged_staged_log_cannot_take_the_immutable_name(self):
        observed, record, (prior, prior_raw), (fresh, _) = self.interrupted_publication()
        damaged = []

        def corrupt_fresh(path, write, data):
            if path.name == fresh.name:
                damaged.append(path)
                return write(b'x' * len(data))
            return write(data)

        with self.intercept_log_writes(record.parent.parent, corrupt_fresh), self.assertRaises(ValueError):
            campaign.publish(self.repo, self.context, observed, self.expected)
        self.assertTrue(damaged)
        self.assertFalse(fresh.exists())
        self.assertFalse(record.exists())
        self.assertEqual(prior.read_bytes(), prior_raw)

    def test_dedicated_detached_cache_can_fast_forward_but_never_discard_a_commit(self):
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp).resolve() / 'execution'
            self.git('worktree', 'add', '--quiet', '--detach', str(workspace), self.base)
            common = requests.common_dir(self.repo)
            self.assertEqual(campaign.execution_workspace(self.repo, self.source, workspace, common), workspace)
            self.assertEqual(campaign.git(workspace, 'rev-parse', 'HEAD'), self.source)
            with self.assertRaisesRegex(ValueError, 'only fast-forward'):
                campaign.execution_workspace(self.repo, self.base, workspace, common)
            (workspace / 'unowned.txt').write_text('keep')
            with self.assertRaisesRegex(ValueError, 'must be clean'):
                campaign.execution_workspace(self.repo, self.source, workspace, common)
            self.assertEqual((workspace / 'unowned.txt').read_text(), 'keep')

    def test_staged_changes_are_not_the_frozen_execution_source(self):
        (self.repo / 'alab/features.toml').write_text('changed')
        with self.assertRaisesRegex(ValueError, 'cleanliness'):
            requests.request(self.repo)


class ExecutionTests(unittest.TestCase):
    def setUp(self):
        self.workspace = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.state = dict(source='a' * 40)
        self.tests = ['exact::case']
        self.output = ('running 1 test\ntest exact::case ... ok\n'
                       'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; '
                       '0 filtered out; finished in 0.01s\n').encode()

    def execute(self, driver, stable=lambda: None):
        with patch.object(campaign, 'run_stage', side_effect=driver):
            return campaign.execute('boot', self.tests, self.workspace, self.workspace,
                                    self.state, stable)

    def test_only_a_precompiler_refusal_is_retried_and_success_is_revalidated(self):
        calls = []
        def driver(argv, cwd, log):
            folder = Path(argv[4]).parent
            calls.append(folder.name)
            if len(calls) == 1:
                log.write_bytes(b'precompiler cache refused\n')
                (folder / 'cache-refused.status').write_text('75\n')
                return 75
            log.write_bytes(self.output)
            return 0
        observed = self.execute(driver)
        self.assertEqual(calls, ['1', '2'])
        self.assertEqual(observed['tests'], self.tests)
        self.assertEqual(json.loads((self.workspace / 'boot/1/result.json').read_text())['status'], 75)
        self.assertEqual(self.execute(self.fail), observed)
        Path(observed['log']).write_bytes(self.output + b'changed')
        with self.assertRaisesRegex(ValueError, 'changed'):
            self.execute(self.fail)

    def test_resource_words_in_a_failed_test_never_make_it_a_precompiler_retry(self):
        def driver(argv, cwd, log):
            log.write_bytes(b'cache owner publication failed for workspace; no build started\n')
            return 75
        with self.assertRaises(campaign.ExecutionFailure) as failed:
            self.execute(driver)
        self.assertEqual(failed.exception.status, 75)
        self.assertFalse((self.workspace / 'boot/2').exists())
        with self.assertRaises(campaign.ExecutionFailure):
            self.execute(self.fail)

    def test_source_mutation_stops_before_a_passing_receipt_is_used(self):
        visits = []
        def stable():
            visits.append(1)
            if len(visits) == 2:
                raise ValueError('changed source')
        def driver(argv, cwd, log):
            log.write_bytes(self.output)
            return 0
        with self.assertRaisesRegex(ValueError, 'changed source'):
            self.execute(driver, stable)
        result = json.loads((self.workspace / 'boot/1/result.json').read_bytes())
        self.assertEqual(result['invalidated'], 'changed source')
        self.assertEqual(result['status'], 0)
        with self.assertRaisesRegex(ValueError, 'source changed'):
            self.execute(self.fail)

    def test_empty_or_wrong_test_cannot_complete_a_required_stage(self):
        def driver(argv, cwd, log):
            log.write_bytes(self.output.replace(b'exact::case', b'different::case'))
            return 0
        with self.assertRaisesRegex(ValueError, 'missing'):
            self.execute(driver)

    def test_pool_and_inbox_scenarios_require_each_exact_libtest(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            contracts = {feature['id']: feature for feature in tomllib.load(stream)['features']}
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        cases = [
            ('native-session', 2, 'aterm', 'tests::', [
                'claim_order_tests::release_cannot_be_overwritten_by_an_already_answered_claim_proof',
                'claim_order_tests::a_local_loss_invalidates_an_already_answered_claim_proof',
                'claim_order_tests::release_revokes_an_in_flight_reconfirmation_before_it_can_authorize',
                'claim_order_tests::a_reconfirmation_that_retakes_a_locally_lost_claim_is_given_back',
                'claim_order_tests::close_wakes_queued_claim_transitions_before_a_buffered_proof_resumes',
                'policy_order_tests::concurrent_policy_reads_retain_the_latest_five_hour_recovery_bound',
                'policy_order_tests::forgetting_policy_invalidates_a_buffered_read_before_a_waiting_fresh_read',
                'policy_order_tests::closing_keeps_policy_empty_when_a_buffered_read_and_a_waiter_finish',
            ]),
            ('goal-storage-recovery', 1, 'state', 'sqlite::connection::alab_transaction_recovery::tests::', [
                'full_write_is_rolled_back_and_next_borrower_can_commit',
                'ordinary_dropped_transactions_keep_a_reusable_connection',
            ]),
            ('native-session', 3, 'aterm', 'tests::', [
                'fleet_rows_keep_older_unhandled_tasks_visible',
                'repeated_broker_offset_keeps_ring_work_visible_without_reading_the_wrong_body',
                'duplicate_local_row_identity_refuses_the_snapshot_and_recovers_on_refresh',
                'malformed_inbox_rows_never_supply_a_partial_receipt_snapshot',
                'the_badge_counts_rows_above_the_handled_watermark',
            ]),
            ('native-session', 4, 'aterm', 'conn::tests::', [
                'aggregate_limit_counts_wire_bytes_and_preserves_following_reply',
                'aggregate_limit_stops_before_allocating_an_over_budget_line',
                'an_empty_reply_needs_no_body_budget_and_errors_have_no_body',
                'aggregate_budget_does_not_change_status_or_bytes_framing',
                'status_obeys_its_own_wire_limit_and_has_no_body',
                'status_and_lines_stop_at_the_default_line_limit',
                'bytes_enforce_their_size_limit_and_consume_exactly_the_declared_body',
                'lines_enforce_the_announced_count_before_reading_any_body',
            ]),
            ('native-session', 5, 'alab-tui', 'session_tests::', [
                'record_sweep_tests::start_sweep_preserves_a_record_replaced_after_the_verdict_snapshot',
                'record_sweep_tests::start_sweep_rechecks_liveness_before_retiring_a_judged_record',
                'record_sweep_tests::start_sweep_preserves_atomic_republication_of_the_same_known_fields',
                'record_sweep_tests::start_sweep_preserves_a_valid_replacement_of_an_unreadable_candidate',
                'record_sweep_tests::start_sweep_hold_keeps_both_publications_when_restore_finds_a_new_canonical_record',
                'the_start_sweep_removes_only_dead_identities_from_before',
                'the_start_sweep_hands_on_each_dead_record_it_removed',
                'startup_sweeps_the_configured_home_before_any_frame',
            ]),
            ('native-session', 10, 'autopilot-library', 'extension::model_blocked_goal_tests::source::', [
                'a_host_round_trip_defeats_a_prepared_model_block_resume',
                'a_mode_round_trip_defeats_a_prepared_model_block_resume',
                'a_mode_round_trip_during_goal_instruction_rejects_only_the_stale_model_block',
                'a_fresh_model_block_after_host_activity_establishes_new_epoch_authority',
            ]),
        ]
        for feature, number, stage, prefix, cases_in_stage in cases:
            with self.subTest(feature=feature):
                contract = contracts[feature]
                descriptions = contract['scenarios']
                selected = descriptions[number - 1]
                names = sorted(prefix + name for name in cases_in_stage)
                required = required_libtests(contract, recipes)
                self.assertEqual(required[selected], names)
                for text in descriptions:
                    if text != selected:
                        self.assertTrue(set(names).isdisjoint(required[text]))
                inventory = {'features': [{'id': feature, 'scenarios': [
                    {'description': text, 'required_libtests': required[text]} for text in descriptions]}]}
                rows, stages = campaign.plan(inventory, recipes, [number])
                self.assertEqual(set(rows), {(feature, selected)})
                self.assertEqual(stages, {stage: names})
                # A successful suite containing a similarly named control cannot replace any required result.
                log = self.workspace / 'combined.log'
                output = (f'running {len(names)} tests\n' + ''.join(f'test {name} ... ok\n' for name in names)
                          + f'test result: ok. {len(names)} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n')
                argv = [str(campaign.RECIPES / 'run.sh'), 'outer', str(self.workspace), self.state['source'],
                        str(self.workspace / 'artifacts'), stage, *names]
                log.write_text(output)
                passed = campaign.stage_observation(stage, names, self.workspace, argv, self.state['source'], 0)
                self.assertEqual(passed['tests'], names)
                for required_name in names:
                    with self.subTest(missing=required_name):
                        log.write_text(output.replace(f'{required_name} ... ok', f'{required_name}_other ... ok'))
                        with self.assertRaisesRegex(ValueError, 'missing'):
                            campaign.stage_observation(stage, names, self.workspace, argv, self.state['source'], 0)

    def test_native_goal_call_id_collisions_require_both_orders_and_failure_outcomes(self):
        prefix = 'extension::upgrade_handoff_tests::model_blocked_identity_tests::'
        names = sorted(prefix + name for name in (
            'model_goal_observed_native_call_id_collision_preserves_the_original_source',
            'model_goal_observed_native_call_id_collision_preserves_an_origin_callback_arriving_first',
            'model_goal_observed_native_call_id_collision_failure_before_sql_cannot_consume_the_source',
            'model_goal_observed_native_call_id_collision_preserves_a_committed_aborted_source',
            'model_goal_observed_native_call_id_collision_failed_duplicate_cannot_strand_aborted_usage',
        ))
        self.assert_native_goal_recipe(7, names)

    def test_goal_teardown_requires_the_native_commit_and_all_three_controls(self):
        prefix = 'extension::model_blocked_goal_tests::teardown::'
        names = sorted(prefix + name for name in (
            'a_forced_sampling_teardown_preserves_the_committed_model_block_and_its_recovery',
            'releasing_the_sampling_observer_drains_the_native_goal_tool_and_keeps_its_source',
            'an_api_pause_after_commit_defeats_model_recovery_across_forced_sampling_teardown',
            'shutdown_drains_the_committed_model_stop_before_retiring_its_native_owner',
        ))
        self.assert_native_goal_recipe(8, names)

    def assert_native_goal_recipe(self, number, names):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            feature = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'native-session')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        scenario = feature['scenarios'][number - 1]
        inventory = {'features': [{'id': feature['id'], 'scenarios': [
            {'description': text} for text in feature['scenarios']]}]}
        rows, stages = campaign.plan(inventory, recipes, [number])
        self.assertEqual(set(rows), {(feature['id'], scenario)})
        self.assertEqual(stages, {'autopilot-library': names})
        self.assertCountEqual(required_libtests(feature, recipes)[scenario], names)
        argv = ['run.sh', 'outer', str(self.workspace), self.state['source'], 'out', 'autopilot-library', *names]
        for omitted in names:
            with self.subTest(omitted=omitted):
                ran = [name for name in names if name != omitted]
                count = len(ran)
                log = (f'running {count} tests\n' + ''.join(f'test {name} ... ok\n' for name in ran)
                       + f'test result: ok. {count} passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;\n')
                (self.workspace / 'combined.log').write_text(log)
                with self.assertRaisesRegex(ValueError, 'selected test is missing'):
                    campaign.stage_observation('autopilot-library', names, self.workspace, argv, self.state['source'], 0)

    def test_qualification_startup_selects_only_its_owning_tui_suite(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            feature = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'qualification-fixtures')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        inventory = {'features': [{'id': feature['id'], 'scenarios': [
            {'description': text} for text in feature['scenarios']]}]}
        _, startup = campaign.plan(inventory, recipes, [1])
        self.assertEqual(startup.get('tui'), [
            'app::alab_startup_fixture::tests::attached_startup_does_not_hide_a_backend_drain_failure',
            'app::alab_startup_fixture::tests::only_the_successfully_attached_expected_thread_completes_startup',
        ])
        self.assertNotIn('tui-fixtures', startup)

        _, combined = campaign.plan(inventory, recipes, [4, 5])
        self.assertEqual(set(combined), {'tui-fixtures'})
        self.assertIn('app::tests::sidebar_reopens_saved_side_chat_after_restart', combined['tui-fixtures'])
        self.assertIn('test_diagnostics::tests::a_busy_recorder_does_not_block_the_timeout_diagnostic',
                      combined['tui-fixtures'])

    def test_prepared_plan_requires_the_ui_preparation_and_accepted_target_input(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            feature = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'native-session')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        tui = 'app::tests::session_lifecycle_requests::new_session_tests::'
        opening = 'extension::opening_tests::'
        expected = {
            'tui': sorted(tui + name for name in (
                'plan_implementation_reads_approved_plan_file_in_fresh_session',
                'plan_implementation_handles_unavailable_attachment')),
            'autopilot-library': sorted(opening + name for name in (
                'a_prepared_plan_continues_only_its_targets_accepted_prompt',
                'editing_a_prepared_targets_first_input_cancels_its_continuation',
                'concurrent_prepared_roots_keep_independent_accepted_prompts',
                'stopping_a_prepared_root_discards_only_its_own_prompt',
                'a_real_prepared_root_continues_only_after_its_approved_input_is_recorded')),
        }
        scenario = feature['scenarios'][5]
        required = required_libtests(feature, recipes)
        self.assertCountEqual(required[scenario], expected['tui'] + expected['autopilot-library'])
        inventory = {'features': [{'id': feature['id'], 'scenarios': [
            {'description': text} for text in feature['scenarios']]}]}
        rows, stages = campaign.plan(inventory, recipes, [6])
        self.assertEqual(set(rows), {(feature['id'], scenario)})
        self.assertEqual(stages, expected)
        for stage, names in expected.items():
            argv = ['run.sh', 'outer', str(self.workspace), self.state['source'], 'out', stage, *names]
            for omitted in names:
                with self.subTest(stage=stage, omitted=omitted):
                    ran = [name for name in names if name != omitted]
                    count = len(ran)
                    log = (f'running {count} tests\n' + ''.join(f'test {name} ... ok\n' for name in ran)
                           + f'test result: ok. {count} passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;\n')
                    (self.workspace / 'combined.log').write_text(log)
                    with self.assertRaisesRegex(ValueError, 'selected test is missing'):
                        campaign.stage_observation(stage, names, self.workspace, argv, self.state['source'], 0)

    def test_goal_accounting_needs_both_native_paths_for_retained_usage(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            feature = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'goal-storage-recovery')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        required = required_tests(feature, recipes, 'required_libtest_groups', 'libtest')
        inventory = {'features': [{'id': feature['id'], 'scenarios': [
            {'description': text, 'required_libtests': required[text]} for text in feature['scenarios']]}]}
        _, stages = campaign.plan(inventory, recipes, [3])
        runtime = 'extension::goal_retained_usage_tests::'
        self.assertEqual(stages, {
            'autopilot-library': sorted(runtime + name for name in [
                'terminal_paths_flush_retained_tokens_before_stopping_a_later_manual_turn',
                'same_original_stopped_goal_recovers_but_replacement_keeps_retained_usage_held',
                'a_new_manual_turn_cannot_drop_shared_usage_from_a_zero_token_failed_turn',
                'rejected_current_and_idle_usage_survive_native_unchanged_outcomes',
            ]),
            'goal-extension': ['accounting::alab_storage::tests::a_raw_replacement_quarantines_the_original_shared_boundary'],
        })
        self.assertCountEqual(required[feature['scenarios'][2]], stages['autopilot-library'] + stages['goal-extension'])
        _, recovery = campaign.plan(inventory, recipes, [4])
        self.assertEqual(recovery, {'autopilot-library': [
            'extension::goal_persistence_tests::a_queued_old_stop_cannot_overwrite_a_successful_same_goal_reactivation',
            'extension::goal_persistence_tests::goal_runtime_status_failed_external_stop_holds_until_a_fresh_persisted_action',
        ]})
        self.assertCountEqual(required[feature['scenarios'][3]], recovery['autopilot-library'])

    def test_goal_transport_loss_requires_the_real_disconnect_and_queue_controls(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            feature = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'goal-storage-recovery')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        required = required_libtests(feature, recipes)
        scenario = feature['scenarios'][5]
        server = 'extensions::alab_goal_status::tests::goal_runtime_status_'
        widget = 'chatwidget::alab_hand::goal_runtime_status_widget_tests::'
        expected = {
            'app-server': sorted(server + name for name in [
                'cancelled_blocked_selection_and_inline_preserve_each_final_phase',
                'cancelled_old_selection_cannot_restore_over_acknowledged_inline',
                'repeated_churn_reset_bypasses_stalled_listener_and_closes_terminally',
                'cancelled_blocked_reset_keeps_one_control_and_rearms',
            ]),
            'tui': [widget + 'goal_runtime_status_widget_backend_and_reset_controls',
                    widget + 'physical_disconnect_clears_held_goal_and_preserves_saved_active_status'],
        }
        all_names = set(expected['app-server'] + expected['tui'])
        self.assertCountEqual(required[scenario], all_names)
        inventory = {'features': [{'id': feature['id'], 'scenarios': [
            {'description': text, 'required_libtests': required[text]} for text in feature['scenarios']]}]}
        rows, stages = campaign.plan(inventory, recipes, [6])
        self.assertEqual(stages, expected)
        key = feature['id'], scenario
        for stage, names in expected.items():
            for name in names:
                with self.subTest(missing=name):
                    observed = {'features': [{'id': feature['id'], 'scenarios': [
                        {'description': scenario, 'checks': [{'tests': sorted(all_names - {name})}]}]}]}
                    missing, _ = campaign.remaining(observed, rows, stages)
                    self.assertEqual(missing[key], {stage: {name}})

    def test_candidate_python_requires_an_external_guard_for_this_profile_source_and_stage(self):
        stage = 'proof_build'
        artifacts = self.workspace / 'artifacts'
        artifacts.mkdir()
        argv = [str(campaign.RECIPES / 'run.sh'), 'outer', str(self.workspace),
                self.state['source'], str(artifacts), stage]
        record = {'schema': 1, 'kind': 'python-unittest-results', 'completed': True,
                  'exit_status': 0, 'output': 'test fixture passed', 'tests_run': 1,
                  'tests': [{'id': 'fixture.test', 'status': 'passed', 'finished': True}]}
        (artifacts / 'tests.json').write_text(json.dumps(record))
        guard = {'kind': 'guardian-command-completion', 'completed': True,
                 'normalized_exit_status': 0, 'argv': ['/bin/bash', argv[0], 'python-profile', *argv[2:]]}
        (artifacts / 'guard.json').write_text(json.dumps(guard))

        def observe():
            return campaign.stage_observation(stage, ['fixture.test'], self.workspace,
                                              argv, self.state['source'], 0)

        with self.assertRaises(FileNotFoundError):
            observe()
        path = self.workspace / 'guard.json'
        path.write_text(json.dumps({**guard, 'argv': [*guard['argv'][:-1], 'another-stage']}))
        with self.assertRaisesRegex(ValueError, 'this source and stage'):
            observe()
        path.write_text(json.dumps(guard))
        self.assertEqual(observe()['command'], guard['argv'])

    def test_native_python_checks_its_inner_guard_and_exact_fixture(self):
        artifacts = self.workspace / 'artifacts'
        artifacts.mkdir()
        name = 'fixture.test'
        (artifacts / 'tests.json').write_text(json.dumps({
            'schema': 1, 'kind': 'python-unittest-results', 'completed': True, 'exit_status': 0,
            'output': 'fixture passed', 'tests_run': 1,
            'tests': [{'id': name, 'status': 'passed', 'finished': True}]}))
        guard = {'kind': 'guardian-command-completion', 'completed': True, 'normalized_exit_status': 0}
        path = artifacts / 'guard.json'
        self.assertEqual(set(campaign.INNER_PYTHON_FIXTURES), set(campaign.NATIVE_PYTHON))
        for stage, pattern in campaign.INNER_PYTHON_FIXTURES.items():
            with self.subTest(stage=stage):
                argv = [str(campaign.RECIPES / 'run.sh'), 'outer', str(self.workspace),
                        self.state['source'], str(artifacts), stage]
                command = [sys.executable, '-I', '-B', str(self.workspace / 'alab/python_test_results.py'),
                           '--start-directory', str(campaign.RECIPES), '--pattern', pattern,
                           '--record', str(artifacts / 'tests.json')]
                path.write_text(json.dumps({**guard, 'argv': [*command[:-3], 'other.py', *command[-2:]]}))
                with self.assertRaisesRegex(ValueError, 'selected fixture'):
                    campaign.stage_observation(stage, [name], self.workspace, argv, self.state['source'], 0)
                path.write_text(json.dumps({**guard, 'argv': command}))
                observation = campaign.stage_observation(stage, [name], self.workspace, argv, self.state['source'], 0)
                self.assertEqual(observation['command'], command)

                if stage in campaign.WIRE:
                    command[3] = str(campaign.HERE / 'python_test_results.py')
                    command += ['--tests', name]
                    path.write_text(json.dumps({**guard, 'argv': command}))
                    selected = campaign.stage_observation(stage, [name], self.workspace,
                                                          [*argv, name], self.state['source'], 0)
                    self.assertEqual(selected['command'], command)
                    with self.assertRaisesRegex(ValueError, 'scenario selection'):
                        campaign.stage_observation(stage, [name], self.workspace, [*argv, 'other'],
                                                   self.state['source'], 0)
                    path.write_text(json.dumps({**guard, 'argv': [*command[:-1], 'other']}))
                    with self.assertRaisesRegex(ValueError, 'selected fixture'):
                        campaign.stage_observation(stage, [name], self.workspace, [*argv, name],
                                                   self.state['source'], 0)

    def test_kernel_fixture_refuses_a_changed_candidate_controller_before_initializing_it(self):
        fixture = campaign.RECIPES / 'package_input_controls.py'
        (self.workspace / 'alab').mkdir()
        (self.workspace / 'alab/package.sh').write_text('fixture controller mismatch')
        with patch.dict(os.environ, ALAB_FEATURE_WORKSPACE=str(self.workspace)):
            native = runpy.run_path(str(fixture))['NativePackageInputTests']
        case = native('test_package_controller_inputs_are_readable_but_readonly_outside_target')
        with self.assertRaisesRegex(AssertionError, 'independent review: package.sh'):
            case.setUp()
        self.assertFalse(hasattr(case, 'temp'))

    def test_imported_names_match_scope_scenario_and_format_and_keep_ambiguous_stages(self):
        rows = {('f', 'a'): ('coverage', {'core': {'overlap', 'only-core'}, 'tui': {'overlap'}}),
                ('f', 'b'): ('coverage', {'tui': {'only-tui'}}),
                ('f', 'c'): ('coverage', {'wire': {'runtime-case'}})}
        stages = {'core': ['only-core', 'overlap'], 'tui': ['only-tui', 'overlap'],
                  'wire': ['runtime-case']}
        feature = {'id': 'f', 'scenarios': [
            {'description': 'a', 'checks': [{'format': 'libtest', 'tests': ['only-core', 'overlap']}]},
            {'description': 'b', 'checks': [{'format': 'libtest', 'tests': ['only-tui']}]},
            {'description': 'c', 'checks': [{'format': 'libtest', 'tests': ['runtime-case']}]},
        ]}
        missing, remaining = campaign.remaining({'features': [feature]}, rows, stages)
        self.assertEqual(missing['f', 'a'], {'core': {'overlap'}, 'tui': {'overlap'}})
        self.assertEqual(missing['f', 'b'], {})
        self.assertEqual(missing['f', 'c'], {'wire': {'runtime-case'}})
        self.assertEqual(remaining, stages)
        feature['scenarios'][2]['checks'][0]['format'] = 'python-unittest'
        _, remaining = campaign.remaining({'features': [feature]}, rows, stages)
        self.assertNotIn('wire', remaining)

    def test_account_budget_and_socket_switch_requires_each_native_transport_result(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            contract = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'account-isolation')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        inventory = {'features': [{'id': contract['id'], 'scenarios': [
            {'description': text} for text in contract['scenarios']]}]}
        socket = 'suite::client_websockets::'
        runtime = socket + 'runtime_auth_tests::'
        http = runtime + 'http_reprepares_request_when_account_switches_during_budget_check::'
        websocket = runtime + 'websocket_reconnects_for_replacement_and_account_switch_within_a_turn::'
        expected = {http + mode for mode in ('dual_header', 'assertion_only')}
        expected.update(websocket + mode for mode in (
            'dual_header', 'assertion_only', 'dual_header_with_gateway', 'assertion_only_with_gateway'))
        expected.add(socket + 'responses_websocket_reconnects_after_account_switch')

        scenario = 'Switch accounts during async budget checks and WebSocket reuse.'
        rows, stages = campaign.plan(inventory, recipes, [1])
        self.assertEqual(set(rows), {(contract['id'], scenario)})
        self.assertEqual(stages, {'core-integration': sorted(expected)})
        required = required_libtests(contract, recipes)
        self.assertEqual(set(required[scenario]), expected)
        for description, names in required.items():
            if description != scenario:
                self.assertTrue(expected.isdisjoint(names))

    def test_account_reader_scenarios_require_the_matching_parent_test(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            contract = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'account-isolation')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        rows = [{'description': text} for text in contract['scenarios']]
        inventory = {'features': [{'id': contract['id'], 'scenarios': rows}]}
        required = required_libtests(contract, recipes)
        cases = (
            (3, 'login-reader', 'auth::manager::alab_native_member::tests::'
             'native_managed_direct_reader_owns_real_reader_lifetime', 'constructor_child'),
            (4, 'cloud-reader', 'retained_cloud_transport_owns_its_actual_reader_and_destination_policy',
             'actual_cloud_native_reader_child'),
        )
        for number, stage, name, child in cases:
            scenario = rows[number - 1]['description']
            with self.subTest(stage=stage):
                self.assertEqual(required[scenario], [name])
                # A frozen source may predate the stricter inventory declaration.
                selected, stages = campaign.plan(inventory, recipes, [number])
                self.assertEqual(set(selected), {(contract['id'], scenario)})
                self.assertEqual(stages, {stage: [name]})
                passing = self.output.replace(b'exact::case', name.encode())
                argv = ['run.sh', 'outer', str(self.workspace), self.state['source'], 'out', stage, name]
                def observe(output):
                    (self.workspace / 'combined.log').write_bytes(output)
                    return campaign.stage_observation(stage, [name], self.workspace, argv,
                                                      self.state['source'], 0)
                self.assertEqual(observe(passing)['tests'], [name])
                for invalid in (
                    passing.replace(name.encode(), child.encode()),
                    passing.replace(b'... ok', b'... ignored').replace(b'1 passed', b'0 passed')
                           .replace(b'0 ignored', b'1 ignored'),
                    passing + passing,
                    b'running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; '
                    b'0 measured; 1 filtered out; finished in 0.00s\n',
                ):
                    with self.subTest(output=invalid), self.assertRaises(ValueError):
                        observe(invalid)

    def test_exec_model_context_requires_installed_state_and_allocation_controls(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            feature = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'exec-handoff')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        context = 'context_manager::history::alab_exec_context::tests::'
        ephemeral = 'session::alab_exec_recovery_options::ephemeral::'
        names = sorted([
            context + 'memory_context_retains_accounting_and_renews_process_evidence',
            context + 'memory_context_retains_item_allocations_across_actual_install',
            context + 'memory_context_keeps_file_audio_in_both_tool_output_forms',
            context + 'invalid_accounting_boundaries_are_rejected',
            context + 'sampling_and_audio_metadata_cannot_invent_a_trusted_boundary',
            ephemeral + 'records::tests::' + 'compaction_keeps_nested_tool_outcomes_and_review_provenance',
            ephemeral + 'codec::tests::' + 'generated_item_allocations_survive_private_handoff_and_stay_off_provider_wire',
            ephemeral + 'codec::tests::' + 'orphaned_allocation_is_refused_instead_of_discarded',
        ])
        scenario = feature['scenarios'][1]
        self.assertCountEqual(required_libtests(feature, recipes)[scenario], names)
        inventory = {'features': [{'id': feature['id'], 'scenarios': [
            {'description': text} for text in feature['scenarios']]}]}
        rows, stages = campaign.plan(inventory, recipes, [2])
        self.assertEqual(set(rows), {(feature['id'], scenario)})
        self.assertEqual(stages, {'core': names})
        argv = ['run.sh', 'outer', str(self.workspace), self.state['source'], 'out', 'core', *names]
        for omitted in names:
            with self.subTest(omitted=omitted):
                ran = [name for name in names if name != omitted]
                log = ('running 7 tests\n' + ''.join(f'test {name} ... ok\n' for name in ran)
                       + 'test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;\n')
                (self.workspace / 'combined.log').write_text(log)
                with self.assertRaisesRegex(ValueError, 'selected test is missing'):
                    campaign.stage_observation('core', names, self.workspace, argv, self.state['source'], 0)

    def test_exec_helper_handoff_requires_wire_requests_and_fresh_model_authority(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            feature = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'exec-handoff')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        wires = 'message_processor::alab_exec_suspend::recovery::tree::tests::helper_choices::'
        intent = 'agent::helper_daybreak::turn::alab_exec_helper::tests::'
        expected = {
            'app-server': sorted(wires + name for name in (
                'native_helper_tree_retains_an_implicit_choice_under_a_fresh_owner',
                'native_helper_tree_refuses_another_accounts_implicit_choice',
                'native_helper_tree_preserves_an_explicit_override_across_accounts')),
            'core': sorted(intent + name for name in (
                'restored_helper_data_binds_a_fresh_owner_and_keeps_selection_implicit',
                'another_account_refuses_the_inherited_automatic_choice',
                'an_explicit_override_does_not_borrow_an_inherited_choice',
                'stored_data_cannot_activate_disabled_propagation',
                'active_choice_is_frozen_separately_from_later_turn_defaults')),
        }
        scenario = feature['scenarios'][3]
        required = required_libtests(feature, recipes)
        self.assertEqual(required[scenario], sorted(expected['app-server'] + expected['core']))
        # The older candidate also has these tests, before its source required their names.
        inventory = {'features': [{'id': feature['id'], 'scenarios': [
            {'description': text} for text in feature['scenarios']]}]}
        rows, stages = campaign.plan(inventory, recipes, [4])
        self.assertEqual(set(rows), {(feature['id'], scenario)})
        self.assertEqual(stages, expected)
        with self.assertRaisesRegex(ValueError, 'scenario has no recipe'):
            campaign.plan(inventory, recipes)

        for stage, names in expected.items():
            argv = ['run.sh', 'outer', str(self.workspace), self.state['source'], 'out', stage, *names]
            for omitted in names:
                with self.subTest(stage=stage, omitted=omitted):
                    ran = [name for name in names if name != omitted]
                    count = len(ran)
                    log = (f'running {count} tests\n' + ''.join(f'test {name} ... ok\n' for name in ran)
                           + f'test result: ok. {count} passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;\n')
                    (self.workspace / 'combined.log').write_text(log)
                    with self.assertRaisesRegex(ValueError, 'selected test is missing'):
                        campaign.stage_observation(stage, names, self.workspace, argv, self.state['source'], 0)

    def test_account_credential_output_requires_every_ownership_and_retirement_observation(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            contract = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'account-isolation')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        rows = [{'description': text} for text in contract['scenarios']]
        inventory = {'features': [{'id': contract['id'], 'scenarios': rows}]}
        required = required_libtests(contract, recipes)
        expected = sorted('auth::manager::' + name for name in (
            'alab_native_member::tests::native_credential_output_refuses_staged_and_pending_bytes_after_source_loss_or_policy_withdrawal',
            'alab_context_host::tests::credential_export_rechecks_before_delivering_rendered_response',
            'alab_context_host::tests::credential_export_refuses_host_replacement_with_unchanged_watch_counters',
            'alab_epoch_snapshot::tests::credential_export_holds_retirement_until_output_is_dropped',
        ))
        self.assertEqual(required[rows[4]['description']], expected)
        # A frozen source without the new output test must still be asked for it.
        _, stages = campaign.plan(inventory, recipes, [5])
        self.assertEqual(stages, {'login-reader': expected})
        _, together = campaign.plan(inventory, recipes, [3, 5])
        self.assertEqual(together, {'login-reader': sorted(required[rows[2]['description']] + expected)})

        for missing in expected:
            with self.subTest(missing=missing):
                ran = [name for name in expected if name != missing]
                log = 'running 3 tests\n' + ''.join(f'test {name} ... ok\n' for name in ran)
                log += ('test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; '
                        '1 filtered out; finished in 0.01s\n')
                (self.workspace / 'combined.log').write_text(log)
                argv = ['run.sh', 'outer', str(self.workspace), self.state['source'], 'out',
                        'login-reader', *expected]
                with self.assertRaisesRegex(ValueError, 'selected test is missing'):
                    campaign.stage_observation('login-reader', expected, self.workspace, argv,
                                               self.state['source'], 0)

    def test_continuity_requires_each_wire_result_on_its_own_scenario(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            contract = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'execution-continuity')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        names = required_tests(contract, recipes, 'required_python_groups', 'python-unittest')
        expected = ['test_project_hook_execution_respects_mode_and_explicit_thread_override',
                    'test_optional_model_stays_and_retired_model_moves_with_a_notice',
                    'test_capable_user_thread_accepts_confirmation_and_declines_requested_fields',
                    'test_automatic_title_is_suppressed_and_manual_suggestion_is_editable']
        scenarios = contract['scenarios']
        wire_cases = [names[scenario] for scenario in scenarios[:len(expected)]]
        self.assertEqual(wire_cases, [['execution_wire_controls.ExecutionWireControls.' + name] for name in expected])
        finding = 'Recover local TUI input awaiting delivery when interactive review is off.'
        held_input = 'native_ui_controls.NativeUI.test_passive_refusal_holds_queued_input_until_new_user_input'
        reviewed = 'native_ui_controls.NativeUI.test_interactive_refusal_review_sends_neither_queue_nor_continuation'
        self.assertCountEqual(names[finding], [held_input, reviewed])
        for scenario in scenarios[len(expected):]:
            if scenario != finding:
                self.assertEqual(names[scenario], [], scenario)
        group = recipes['groups'][contract['required_python_groups'][0]]
        for invalid_stage in ('core', 'unrecognized'):
            with self.subTest(stage=invalid_stage), self.assertRaisesRegex(ValueError, 'non-python-unittest'):
                group['stage'] = invalid_stage
                required_tests(contract, recipes, 'required_python_groups', 'python-unittest')

    def test_provider_recovery_libraries_select_their_own_named_cases_alone_and_together(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            provider = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'provider-recovery')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        inventory = {'features': [{'id': provider['id'], 'scenarios': [
            {'description': text} for text in provider['scenarios']]}]}
        owners = {
            1: ('autopilot-library', 4, 'resume_history::tests::'),
            2: ('autopilot-library', 13, 'recovery::tests::'),
            20: ('recovery', 12, 'unix::'),
            22: ('recovery', 12, 'unix::'),
            23: ('kimi-library', 4, 'errors_tests::'),
        }
        libraries = {owner for owner, _, _ in owners.values()}
        combined = {}
        for number, (owner, count, prefix) in owners.items():
            with self.subTest(number=number):
                _, stages = campaign.plan(inventory, recipes, [number])
                self.assertEqual(stages.keys() & libraries, {owner})
                names = stages[owner]
                self.assertEqual(len(names), count)
                self.assertTrue(all(name.startswith(prefix) for name in names))
                combined.setdefault(owner, set()).update(names)
                if number == 22:
                    self.assertIn('wire_controls.WeeklyQuotaControls.test_preflight_and_reactive_24_and_25_hour_boundaries',
                                  stages['wire'])
        # A prefix and a count alone cannot detect a renamed test in its owning crate.
        quota = campaign.HERE.parent / 'codex-rs/alab-quota-recovery/src'
        for name in combined['recovery']:
            with self.subTest(selector=name):
                _, module, function = name.split('::')
                source = quota / ('unix.rs' if module == 'tests' else module + '.rs')
                self.assertRegex(source.read_text(), r'#\[(?:tokio::)?test\]\s+(?:async )?fn '
                                 + re.escape(function) + r'\(')
        _, stages = campaign.plan(inventory, recipes, list(owners))
        self.assertEqual({key: set(names) for key, names in stages.items() if key in libraries}, combined)

    def test_natural_reset_requires_the_native_goal_turn_from_candidates_that_ship_it(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            provider = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'provider-recovery')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        required = required_libtests(provider, recipes)
        scenario = 'Natural reset, earned reset, duplicate receipt, account switch and long-wait cancellation.'
        case = 'wall_acts::a_goal_started_without_a_user_turn_resumes_itself_after_a_dated_usage_limit'
        deferred = ('extension::upgrade_handoff_tests::upgrade_inventory_tests::'
                    'a_pending_provider_reset_keeps_the_original_goal_timer_on_upgrade_refusal')
        stages = {'autopilot-turn': case, 'autopilot-library': deferred}
        rows = [{'description': text, 'required_libtests': required[text]} for text in provider['scenarios']]
        inventory = {'features': [{'id': provider['id'], 'scenarios': rows}]}
        number = provider['scenarios'].index(scenario) + 1
        _, current = campaign.plan(inventory, recipes, [number])
        (self.workspace / 'combined.log').write_bytes(self.output)
        for stage, name in stages.items():
            with self.subTest(stage=stage):
                self.assertEqual([text for text, names in required.items() if name in names], [scenario])
                self.assertEqual(current[stage], [name])
                argv = ['run.sh', 'outer', str(self.workspace), self.state['source'], 'out', stage, name]
                with self.assertRaisesRegex(ValueError, 'selected test is missing'):
                    campaign.stage_observation(stage, [name], self.workspace, argv, self.state['source'], 0)

        # The first candidate carried just the native turn. Earlier candidates carried neither test.
        required[scenario].remove(deferred)
        _, prior = campaign.plan(inventory, recipes, [number])
        self.assertEqual(prior, {stage: names for stage, names in current.items() if stage != 'autopilot-library'})
        required[scenario].remove(case)
        _, historical = campaign.plan(inventory, recipes, [number])
        self.assertEqual(historical, {stage: names for stage, names in current.items() if stage not in stages})

    def test_only_libtest_progress_can_witness_the_native_goal_reset(self):
        case = 'wall_acts::a_goal_started_without_a_user_turn_resumes_itself_after_a_dated_usage_limit'
        other = 'fixture::other'
        stage = 'autopilot-turn'
        argv = ['run.sh', 'outer', str(self.workspace), self.state['source'], 'out', stage, case]
        log = self.workspace / 'combined.log'

        def observe(ran, prefix, printed):
            log.write_text(f'running 1 test\n{prefix}test {ran} ... ok\n\nsuccesses:\n\n'
                           f'---- {ran} stdout ----\ntest {printed} ... ok\n\nsuccesses:\n    {ran}\n\n'
                           'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; '
                           '1 filtered out; finished in 0.01s\n')
            return campaign.stage_observation(stage, [case], self.workspace, argv, self.state['source'], 0)

        # A TUI frame interrupted the genuine progress line. A transcript in its
        # captured stdout must never substitute for the missing verdict.
        with self.assertRaisesRegex(ValueError, 'executed test names disagree'):
            observe(other, 'terminal frame ', case)
        self.assertEqual(observe(case, '', other)['tests'], [case])

    def test_selected_quota_libtests_dispatch_only_to_the_quota_package(self):
        helpers = self.workspace / 'alab'
        helpers.mkdir()
        (self.workspace / 'codex-rs').mkdir()
        (helpers / 'lib.sh').write_text('alab_project_selected() { :; }\n')
        (helpers / 'unit-tests.sh').touch()
        (helpers / 'focused-tests.sh').write_text(
            'alab_run_focused() { printf "%s\\n" "${packages[@]}" "$@"; }\n')
        selector = 'unix::tests::credit_granted_after_exhaustion_resumes_the_original_turn'
        command = ['/bin/bash', str(campaign.RECIPES / 'run.sh'), 'contained', str(self.workspace),
                   self.state['source'], str(self.workspace), 'recovery', selector]
        target = str(self.workspace / 'target')
        env = dict(os.environ, TMPDIR=str(self.workspace), CARGO_TARGET_DIR=target, ALAB_CARGO='unused')
        args = subprocess.check_output(command, env=env, text=True).splitlines()
        self.assertEqual(args, ['-p', 'codex-alab-quota-recovery', '1', '--require-test', selector,
                                '--', 'unit_tests', str(self.workspace), target, selector, '--exact', '--show-output'])

    def test_provider_refusal_scenario_selects_the_live_turn_and_its_replay_controls(self):
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            provider = next(f for f in tomllib.load(stream)['features'] if f['id'] == 'provider-recovery')
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        required = required_libtests(provider, recipes)
        inventory = {'features': [{'id': provider['id'], 'scenarios': [
            {'description': text, 'required_libtests': required[text]} for text in provider['scenarios']]}]}
        rows, _ = campaign.plan(inventory, recipes)
        scenario = 'Advised and unadvised overload, invalid compaction and retry exhaustion, while preserving specific request refusals.'
        _, controls = rows[provider['id'], scenario]
        live = 'a_misalignment_finding_waits_for_the_person_before_another_request'
        self.assertIn(live, controls.get('autopilot-turn', set()))
        queued = 'typed_provider_refusal_holds_queued_input_until_explicit_start'
        mailbox = 'trigger_turn_mail_waits_for_review_and_reaches_the_explicit_queue_turn'
        relay = 'relay_cannot_replace_the_finishing_turn_before_its_finding_is_published'
        delayed = 'observed_steer_fallback_cannot_answer_a_finding_after_a_missing_reply'
        self.assertEqual(controls.get('queue-review'), {queued, mailbox, relay, delayed})
        required_replay = {'resume_history::tests::' + name for name in (
            'a_duplicate_start_cannot_reopen_a_terminal_misalignment_refusal',
            'a_retired_duplicate_start_cannot_displace_a_running_turn_before_its_finding',
            'misalignment_replay_requires_a_new_turn_even_when_the_start_is_missing',
            'sparse_duplicate_completion_keeps_cruise_stopped_at_the_misalignment_finding',
        )}
        self.assertLessEqual(required_replay, controls.get('autopilot-library', set()))
        self.assertLessEqual({live, queued, mailbox, relay, delayed} | required_replay, set(required[scenario]))

        old = copy.deepcopy(inventory)
        old_row = old['features'][0]['scenarios'][0]
        old_row['required_libtests'].remove(delayed)
        prior_controls, _ = campaign.plan(old, recipes, [1])
        self.assertEqual(prior_controls[provider['id'], scenario][1],
                         {**controls, 'queue-review': {queued, mailbox, relay}})
        for test_name in (queued, mailbox, relay):
            old_row['required_libtests'].remove(test_name)
        older_controls, older_stages = campaign.plan(old, recipes, [1])
        self.assertNotIn('queue-review', older_stages)
        self.assertEqual(older_controls[provider['id'], scenario][1],
                         {stage: names for stage, names in controls.items() if stage != 'queue-review'})
        for malformed in (queued, [queued, queued], [False]):
            old_row['required_libtests'] = malformed
            with self.subTest(malformed=malformed), self.assertRaisesRegex(ValueError, 'invalid required_libtests'):
                campaign.plan(old, recipes, [1])

        def driver(argv, cwd, log):
            self.assertEqual(argv[5:], ['autopilot-turn', live])
            log.write_bytes(self.output.replace(b'exact::case', live.encode()))
            return 0

        with patch.object(campaign, 'run_stage', side_effect=driver):
            observed = campaign.execute('autopilot-turn', [live], self.workspace, self.workspace,
                                        self.state, lambda: None)
        self.assertEqual(observed['tests'], [live])

    def test_planning_identifies_the_scenario_with_missing_or_invalid_coverage(self):
        inventory = {'features': [{'id': 'account-isolation', 'scenarios': [{'description': 'Switch logins.'}]}]}
        row = {'scenario': 'Switch logins.', 'groups': ['switch']}
        recipes = {'features': {'account-isolation': [row]},
                   'groups': {'switch': {'stage': 'login-reader', 'tests': ['switch_logins']}}}
        for value in [None, '', '  ', ['A useful check']]:
            if value is not None:
                row['coverage'] = value
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, 'account-isolation scenario 1.*Switch logins'):
                campaign.plan(inventory, recipes)
        row['coverage'] = 'The continued turn uses the destination account.'
        results, _ = campaign.plan(inventory, recipes)
        self.assertEqual(results['account-isolation', 'Switch logins.'][0], row['coverage'])

    def test_recipe_change_or_unknown_feature_never_silently_reduces_coverage(self):
        recipes, _ = campaign.read_recipe_snapshot(campaign.RECIPES / 'scenarios.json')
        with (campaign.HERE / 'features.toml').open('rb') as stream:
            contracts = tomllib.load(stream)['features']
        inventory = {'features': []}
        for feature in contracts:
            if feature['id'] in recipes['features']:
                required = required_libtests(feature, recipes) if 'required_libtest_groups' in feature else {}
                inventory['features'].append({'id': feature['id'], 'scenarios': [
                    {'description': scenario, 'checks': [], 'required_libtests': required.get(scenario, [])}
                    for scenario in feature['scenarios']
                ]})
        complete = {'features': []}
        pending_features, partial_stages = set(), set()
        for feature in inventory['features']:
            controls = recipes['features'][feature['id']]
            if all(row['groups'] for row in controls):
                complete['features'].append(feature)
                continue
            pending_features.add(feature['id'])
            with self.assertRaisesRegex(ValueError, 'scenario has no recipe'):
                campaign.plan({'features': [feature]}, recipes)
            for number, row in enumerate(controls, 1):
                if row['groups']:
                    _, stages = campaign.plan({'features': [feature]}, recipes, [number])
                    partial_stages.update(stages)
        rows, stages = campaign.plan(complete, recipes)
        self.assertTrue({'provider-recovery', 'build-and-adoption', 'model-discovery',
                         'product-policy', 'history-pagination', 'security-notices', 'skill-discovery',
                         'qualification-fixtures', 'native-ui'}.issubset(
                             recipes['features']))
        self.assertEqual({feature for feature, _ in rows} | pending_features, set(recipes['features']))
        self.assertEqual(set(stages) | partial_stages, set(campaign.STAGES))
        changed = copy.deepcopy(complete)
        changed['features'][0]['scenarios'][0]['description'] += ' changed'
        with self.assertRaisesRegex(ValueError, 'contract changed'):
            campaign.plan(changed, recipes)
        complete['features'].append({'id': 'fixture-without-recipe', 'scenarios': []})
        with self.assertRaisesRegex(ValueError, 'unsupported feature scope'):
            campaign.plan(complete, recipes)
