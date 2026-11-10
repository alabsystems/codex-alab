# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).parents[1]))
import test_upstream_intake
import upstream_status


class StatusTests(unittest.TestCase):
    setUp = test_upstream_intake.IntakeTests.setUp
    git = test_upstream_intake.IntakeTests.git
    commit = test_upstream_intake.IntakeTests.commit

    def test_partial_and_complete_integration_are_distinct(self):
        result = upstream_status.report(self.repo, self.base, self.target)
        self.assertEqual(result['pending_count'], 1)
        self.assertEqual(result['integrated_frontier_sha'], self.base)
        self.assertIsNotNone(result['oldest_pending_committed_at'])
        self.assertIsNone(result['qualified_sha'])
        self.assertIsNone(result['last_successful_fetch_at'])
        result = upstream_status.report(self.repo, self.target, self.target)
        self.assertEqual(result['pending_count'], 0)
        self.assertIsNone(result['oldest_pending_age_seconds'])
        self.assertIsNone(result['qualified_sha'])

    def test_alab_only_changes_do_not_count_as_upstream_lag(self):
        self.git('checkout', '-q', '-b', 'alab', self.base)
        self.commit('alab change')
        result = upstream_status.report(self.repo, 'HEAD', self.target)
        self.assertEqual(result['pending_count'], 1)
        self.git('merge', '-q', '--no-ff', self.target, '-m', 'integrate')
        self.assertEqual(upstream_status.report(self.repo, 'HEAD', self.target)['pending_count'], 0)

    def test_missing_ref_is_unknown_not_zero(self):
        result = upstream_status.report(self.repo, 'missing', self.target)
        self.assertIsNone(result['pending_count'])
        self.assertTrue(result['errors'])


if __name__ == '__main__':
    unittest.main()
