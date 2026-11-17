# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The public release hook binds its shared CLI status source before version work."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class CliStatusBindingTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='alab-cli-status-binding-')
        self.addCleanup(self.scratch.cleanup)
        self.home = Path(self.scratch.name)
        self.repo = self.home / 'owned source'
        self.repo.mkdir()
        self.environment = {key: value for key, value in os.environ.items()
                            if not key.startswith(('GIT_', 'ALAB_', 'PUB_'))}
        self.environment.update(
            HOME=str(self.home), GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1',
            GIT_AUTHOR_NAME='Fixture', GIT_AUTHOR_EMAIL='fixture@example.invalid',
            GIT_COMMITTER_NAME='Fixture', GIT_COMMITTER_EMAIL='fixture@example.invalid')
        self.git('init', '-q', '-b', 'main')
        self.git('config', 'commit.gpgsign', 'false')
        directory = self.repo / 'alab'
        directory.mkdir()
        self.helper = directory / 'cli_status.py'
        shutil.copyfile(ROOT / 'alab/cli_status.py', self.helper)
        package = directory / 'package.sh'
        package.write_text('''#!/bin/sh
set -eu
[ "$#" -eq 3 ]
[ "$1" = --release-version ]
[ "$2" = --commit ]
[ "$3" = "$PUB_SOURCE_COMMIT" ]
printf 'owned dummy version path\n' > .git/package-called
printf '0.1.0+codex.0.164.0\n'
''')
        package.chmod(0o755)
        self.git('add', 'alab')
        self.git('commit', '-qm', 'owned release-hook inputs')
        self.source = self.git('rev-parse', 'HEAD')
        self.marker = self.repo / '.git/package-called'
        self.hook = self.home / 'engine/pre-promote'
        self.hook.parent.mkdir()
        shutil.copyfile(ROOT / 'publish/pre-promote', self.hook)

    def git(self, *arguments):
        result = subprocess.run(['git', '-C', str(self.repo), *arguments],
                                capture_output=True, text=True, timeout=30,
                                env=self.environment)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def hook_result(self):
        return subprocess.run(['/bin/bash', str(self.hook)], cwd=self.repo,
                              env=dict(self.environment, PUB_SOURCE_COMMIT=self.source,
                                       PUB_VERSION='0.1.0'),
                              capture_output=True, text=True, timeout=30)

    def snapshot(self):
        return {'helper': self.helper.read_bytes(),
                'head': self.git('rev-parse', 'HEAD'),
                'status': self.git('status', '--porcelain'),
                'refs': self.git('for-each-ref', '--format=%(refname) %(objectname)'),
                'config': (self.repo / '.git/config').read_bytes()}

    def test_unchanged_helper_allows_only_the_owned_dummy_version_path(self):
        before = self.snapshot()
        result = self.hook_result()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(result.stderr, '')
        self.assertEqual(self.marker.read_text(), 'owned dummy version path\n')
        self.assertIn('released as 0.1.0+codex.0.164.0', result.stdout)
        self.assertEqual(self.snapshot(), before)

    def test_substituted_committed_helper_is_refused_before_version_work(self):
        self.helper.write_bytes(self.helper.read_bytes() + b'\n# owned changed classifier input\n')
        self.git('add', 'alab/cli_status.py')
        self.git('commit', '-qm', 'substitute owned CLI classifier')
        self.assertNotEqual(self.git('rev-parse', 'HEAD'), self.source)
        before = self.snapshot()
        result = self.hook_result()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(result.stdout, '')
        self.assertIn("this checkout's release gate", result.stderr)
        self.assertIn('is not ' + self.source[:9], result.stderr)
        self.assertFalse(self.marker.exists(), 'the dummy version path must not start')
        self.assertEqual(self.snapshot(), before)


if __name__ == '__main__':
    unittest.main()
