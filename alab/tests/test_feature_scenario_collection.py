# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise scenario-part publication and full admission through the campaign CLI."""
import json
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest

import test_feature_campaign as fixture


class ScenarioPartCliTests(unittest.TestCase):
    def setUp(self):
        self.gate = fixture.NativeRequestTests()
        self.addCleanup(self.gate.doCleanups)
        self.gate.setUp()
        self.gate.repo = self.gate.repo.resolve(strict=True)
        repo = self.gate.repo
        self.labels = [f'Fixture scenario {number}.' for number in range(1, 8)]
        (repo / 'alab/features.toml').write_text(
            'schema = 1\n[[features]]\nid = "model-discovery"\nlifecycle = "active"\n'
            + 'scenarios = ' + json.dumps(self.labels) + '\n')
        tests = repo / 'alab/tests'
        tests.mkdir(exist_ok=True)
        (tests / 'test_rows.py').write_text(
            'import subprocess, tomllib, unittest\n'
            'class FrozenRows(unittest.TestCase):\n'
            '    def check_row(self, number):\n'
            '        raw = subprocess.check_output(["git", "show", "HEAD:alab/features.toml"], text=True)\n'
            '        self.assertEqual(tomllib.loads(raw)["features"][0]["scenarios"][number - 1],\n'
            '                         f"Fixture scenario {number}.")\n'
            + ''.join(f'    def test_row_{n}(self):\n        self.check_row({n})\n' for n in range(1, 8)))
        self.gate.git('add', 'alab')
        self.gate.commit('seven ordered fixture scenarios')
        self.gate.record_native_request()

        # Run the actual CLI from a private copy with an independent fixture recipe.
        # The local controller runs the candidate tests and writes the real guardian result.
        self.collector = Path(self.enterContext(tempfile.TemporaryDirectory())) / 'alab'
        self.collector.mkdir()
        original = fixture.campaign.HERE
        for path in original.glob('*.py'):
            shutil.copyfile(path, self.collector / path.name)
        for relative in ('package.sh', 'lib.sh', 'tests/test_proof_build.py',
                         'feature_recipes/request.py', 'feature_recipes/manifest.py'):
            target = self.collector / relative
            target.parent.mkdir(exist_ok=True)
            shutil.copyfile(original / relative, target)
        recipes = self.collector / 'feature_recipes'
        runner = recipes / 'run.sh'
        runner.write_text(
            '#!/bin/bash\nset -eu\n'
            + f'python={shlex.quote(sys.executable)}\ncollector={shlex.quote(str(self.collector))}\n'
            + 'case "$1" in\n'
            '  outer) shift; exec "$python" -I -B "$collector/command_guard.py" '
            '--result-file "$3/../guard.json" -- /bin/bash "$0" python-profile "$@" ;;\n'
            '  python-profile) exec "$python" -I -B "$collector/python_test_results.py" '
            '--start-directory "$2/alab/tests" --pattern test_rows.py --record "$4/tests.json" ;;\n'
            '  *) exit 64 ;;\nesac\n')
        runner.chmod(0o700)
        self.manifest = recipes / 'scenarios.json'
        self.rows = [{'scenario': label, 'groups': [str(n)] if n in (1, 2, 4) else [],
                      'coverage': 'Read the declared fixture row from the candidate commit.'}
                     for n, label in enumerate(self.labels, 1)]
        self.groups = {str(n): {'stage': 'qualification', 'tests': [f'test_rows.FrozenRows.test_row_{n}']}
                       for n in range(1, 8)}
        self.write_recipes()

    def write_recipes(self):
        self.manifest.write_text(json.dumps({'features': {'model-discovery': self.rows},
                                             'groups': self.groups}))

    def cli(self, action, *args, error=None):
        cmd = [sys.executable, '-I', '-B', str(self.collector / 'feature_campaign.py'),
               action, '--repo', str(self.gate.repo), '--record', str(self.gate.path), *args]
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=45)
        if error:
            self.assertNotEqual(result.returncode, 0, cmd)
            self.assertIn(error, result.stderr)
            return
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(result.stdout.startswith('{'), result.stdout[:500])
        return json.loads(result.stdout)

    def collect(self, *numbers, error=None):
        options = [value for n in numbers for value in ('--scenario', str(n))]
        return self.cli('collect-part', '--feature', 'model-discovery', *options, error=error)

    def test_recipe_identity_changes_when_a_listed_collector_helper_changes(self):
        helper = self.collector / 'smoke' / 'sample.py'
        helper.parent.mkdir()
        helper.write_text('value = 1\n')
        (self.collector / 'feature_recipes/sample.inputs').write_text('smoke/sample.py\n')
        before = self.cli('request')['recipe_sha256']
        helper.write_text('value = 2\n')
        self.assertNotEqual(before, self.cli('request')['recipe_sha256'])

    def test_independent_parts_keep_all_seven_rows_and_only_full_execution_admits(self):
        self.assertEqual(self.cli('request')['unsupported_features'], ['model-discovery'])
        first = self.collect(4, 1)
        second = self.collect(2)
        self.assertNotEqual(first['run'], second['run'])
        self.assertEqual(first['recipe_sha256'], second['recipe_sha256'])
        collected = {}
        for info, numbers in ((first, [1, 4]), (second, [2])):
            self.assertFalse(info['admitted'])
            self.assertEqual(info['selected_scenarios'], numbers)
            record = json.loads(Path(info['part']).read_bytes())
            rows = record['features'][0]['scenarios']
            self.assertEqual([row['description'] for row in rows], self.labels)
            for n, row in enumerate(rows, 1):
                named = [name for check in row['checks'] for name in check['tests']]
                self.assertEqual(named, self.groups[str(n)]['tests'] if n in numbers else [])
                if n in numbers:
                    collected[n] = row['checks'][0]
        self.assertEqual(self.collect(1, 4)['part'], first['part'])
        imports = ['--part', first['part'], '--part', second['part']]
        self.cli('run', *imports, error='unsupported feature scope')
        self.assertFalse(Path(self.gate.report['bundle']).exists())

        log = Path(first['run']) / 'qualification/1/artifacts/tests.json'
        original = log.read_bytes()
        log.write_bytes(original + b'\n')
        self.collect(1, 4, error='named test log changed')
        log.write_bytes(original)
        for n, row in enumerate(self.rows, 1):
            row['groups'] = [str(n)]
        self.write_recipes()
        self.assertTrue(self.cli('run', *imports)['admitted'])
        accepted = json.loads((Path(self.gate.report['bundle']) / 'record.json').read_bytes())
        for n, row in enumerate(accepted['features'][0]['scenarios'], 1):
            self.assertEqual([check['tests'] for check in row['checks']], [self.groups[str(n)]['tests']])
            if n in collected:
                self.assertEqual(row['checks'][0]['command'], collected[n]['command'])
                self.assertEqual(row['checks'][0]['log_sha256'], collected[n]['log_sha256'])
        self.assertEqual(fixture.fixtures.GATE.admission(self.gate.repo, self.gate.base)['errors'], [])

    def test_pending_out_of_range_and_changed_omitted_rows_refuse_before_execution(self):
        self.collect(error='scenario has no recipe')
        self.collect(1, 3, error='scenario has no recipe')
        self.collect(0, error='scenario number outside')
        self.collect(8, error='scenario number outside')
        self.collect(2, 2, error='repeated scenario number')
        self.cli('run', '--scenario', '1', error='--scenario requires collect-part')
        self.rows[-1]['groups'] = None
        self.write_recipes()
        self.collect(1, error='invalid recipe groups')
        self.rows[-1]['groups'] = []
        self.rows[-1]['scenario'] = 'Changed omitted row.'
        self.write_recipes()
        self.collect(1, error='scenario contract changed for model-discovery')
        self.assertFalse((fixture.requests.common_dir(self.gate.repo) / 'alab-feature-campaign').exists())
