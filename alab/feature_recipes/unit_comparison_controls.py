# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Compare independent failing harnesses with the candidate's native unit runner."""
import json
import os
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(os.environ['ALAB_FEATURE_WORKSPACE']) / 'alab/tests'))
import test_unit_results as fixtures


class UnitComparison(fixtures.UnitResultsTests):
    def test_same_pristine_failure_remains_a_failed_copy(self):
        plan = self.original()
        pristine = fixtures.Tree(self.base, 'pristine')
        state, prepared = self.prepare(pristine, 'pristine-comparison', plan)
        self.assertEqual(prepared.returncode, 0, prepared.stderr)
        log, status = self.execute(pristine, state)
        self.assertEqual(status, 101)
        collected, result = self.collect(state, log, status)
        self.assertEqual(collected.returncode, 0, collected.stderr)
        for path in (plan, result):
            report = json.loads(path.read_bytes())
            self.assertEqual([(row['package'], row['test']) for row in report['failed']],
                             [('package-a', fixtures.TEST)])
            self.assertFalse(any(row['package'] == 'package-a' for row in report['passed']))
        self.assertEqual(pristine.calls('a'), 1)
        self.assertEqual(pristine.calls('b'), 0)


def load_tests(_loader, _discovered, _pattern):
    return unittest.TestSuite([UnitComparison('test_same_pristine_failure_remains_a_failed_copy')])
