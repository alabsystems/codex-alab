# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parents[1]))
import feature_inventory


class InventoryTests(unittest.TestCase):
    def test_ownership_retirement_and_path_failures(self):
        with tempfile.TemporaryDirectory() as scratch:
            repo = Path(scratch)
            (repo / 'owned').touch()
            feature = dict(id='recovery', lifecycle='active', purpose='Retain input',
                           topics=['exec-input'], implementation=['owned'],
                           invariants=['No lost input'], scenarios=['Cancel handoff'],
                           equivalence='Same cancellation and ownership behavior')
            catalog = dict(schema=1, features=[feature])
            topics = {'exec-input'}
            self.assertEqual(feature_inventory.validate(catalog, topics, repo), [])
            self.assertIn('unowned hook topic: new-state', feature_inventory.validate(catalog, topics | {'new-state'}, repo))
            for field, value in [('lifecycle', 'retired'), ('implementation', ['missing']),
                                 ('implementation', ['../owned']), ('scenarios', [])]:
                with self.subTest(field=field, value=value):
                    changed = copy.deepcopy(catalog)
                    changed['features'][0][field] = value
                    self.assertTrue(feature_inventory.validate(changed, topics, repo))
            catalog['features'].append(copy.deepcopy(feature))
            errors = feature_inventory.validate(catalog, topics, repo)
            self.assertIn('duplicate feature ID: recovery', errors)
            self.assertIn('duplicate topic owner: exec-input', errors)


if __name__ == '__main__':
    unittest.main()
