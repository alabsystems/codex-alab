# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The actual hook-count preflight, with one retained and one removed hook."""
import os
from pathlib import Path
import shutil
import sys
import unittest

workspace = Path(os.environ['ALAB_FEATURE_WORKSPACE'])
sys.path.insert(0, str(workspace / 'alab'))
sys.path.insert(0, str(workspace / 'alab/tests'))
from test_check_results_preflight import CheckResultsPreflightTests, STOP_STATUS


class ActualHookLossTests(CheckResultsPreflightTests):
    def baseline(self):
        # Earlier candidate fixtures omit this dependency of their own check.sh.
        reader = self.repo / 'alab/upstream_intake.py'
        candidate_reader = workspace / 'alab/upstream_intake.py'
        if candidate_reader.is_file() and not reader.exists():
            shutil.copyfile(candidate_reader, reader)
        self.hook = self.repo / 'codex-rs/fixture.rs'
        self.hook.write_text('// ALAB(fixture): retained behavior\n')
        self.git('add', '-A')
        self.git('commit', '-q', '-m', 'baseline hook')
        for remote in ('origin', 'upstream'):
            self.git('update-ref', f'refs/remotes/{remote}/main', 'HEAD')

    def test_retained_hook_reaches_the_metadata_boundary(self):
        self.baseline()
        self.git('commit', '-q', '--allow-empty', '-m', 'unchanged hook')
        result = self.check(self.scratch / 'results')
        self.assertEqual(result.returncode, STOP_STATUS, result.stderr)
        self.assertIn('metadata', self.marker.read_text())

    def test_missing_hook_refuses_before_the_metadata_boundary(self):
        self.baseline()
        self.hook.write_text('// upstream content retained\n')
        self.git('add', '-A')
        self.git('commit', '-q', '-m', 'missing hook')
        result = self.check(self.scratch / 'results')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('hooks went missing', result.stderr)
        self.assertFalse(self.marker.exists())


def load_tests(loader, tests, pattern):
    names = ('test_retained_hook_reaches_the_metadata_boundary',
             'test_missing_hook_refuses_before_the_metadata_boundary')
    return unittest.TestSuite(ActualHookLossTests(name) for name in names)
