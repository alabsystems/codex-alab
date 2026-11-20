# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise real sync Git attributes against ordered recipes and conflicting edits."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = Path('alab/feature_recipes/scenarios.json')


class RecipeMergeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab recipe '")
        self.addCleanup(temporary.cleanup)
        self.repo = Path(temporary.name)
        self.path = self.repo / MANIFEST
        self.path.parent.mkdir(parents=True)
        self.git('init', '-q', '-b', 'main')
        self.git('config', 'user.name', 'recipe-test')
        self.git('config', 'user.email', 'recipe-test@example.invalid')
        self.git('config', 'commit.gpgsign', 'false')

    def git(self, *args, check=True):
        return subprocess.run(['git', *args], cwd=self.repo, text=True, capture_output=True, check=check)

    def commit(self, document):
        raw = document if isinstance(document, str) else json.dumps(document, indent=2) + '\n'
        self.path.write_text(raw)
        self.git('add', str(MANIFEST))
        self.git('commit', '-qm', 'recipe')

    def branches(self, base, ours, theirs):
        self.commit(base)
        self.git('switch', '-qc', 'incoming')
        self.commit(theirs)
        self.git('switch', '-q', 'main')
        self.commit(ours)

    def sync_git(self, *args):
        script = (ROOT / 'alab/sync.sh').read_text()
        start = script.index('export ALAB_SYNC_RECIPE_DRIVER=')
        config = script[start:script.index('\n\nin_wt()', start)]
        return subprocess.run(['/bin/bash', '-c',
                               'set -eu\nrepo_root=$1\nshift\n' + config + '\ngit "${merge_cfg[@]}" "$@"',
                               'sync-git', str(ROOT), *args], cwd=self.repo, text=True, capture_output=True,
                              env=dict(os.environ, ALAB_PYTHON=sys.executable))

    def test_disjoint_edits_merge_in_probe_and_worktree(self):
        def rows(*groups):
            return [{'scenario': 'request', 'groups': list(groups), 'coverage': 'wire test'}]

        base = {'features': {'shell': rows('old'), 'models': rows('warm')}, 'groups': {'old': {}, 'warm': {}}}
        ours = {'features': {'models': rows('warm'), 'shell': rows('new')}, 'groups': {'warm': {}, 'new': {}}}
        theirs = {'features': {'shell': rows('old'), 'models': rows('warm', 'fork')},
                  'groups': {'old': {}, 'warm': {}, 'fork': {'tests': ['one', 'two']}}}
        self.branches(base, ours, theirs)
        expected = {'features': {'models': rows('warm', 'fork'), 'shell': rows('new')},
                    'groups': {'warm': {}, 'new': {}, 'fork': {'tests': ['one', 'two']}}}
        probe = self.sync_git('merge-tree', '--write-tree', 'main', 'incoming')
        self.assertEqual(probe.returncode, 0, probe.stderr)
        self.assertEqual(json.loads(self.git('show', f'{probe.stdout.strip()}:{MANIFEST}').stdout), expected)
        result = self.sync_git('merge', '--no-commit', 'incoming')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(self.path.read_bytes()), expected)

    def test_same_entry_requires_a_decision_even_if_text_edits_are_disjoint(self):
        def recipe(first, last):
            return {'features': {}, 'groups': {'model': {'tests': [first, 'b', 'c', 'd', 'e', last]}}}
        self.branches(recipe('a', 'f'), recipe('left', 'f'), recipe('a', 'right'))
        self.require_conflict('groups.model')

    def feature(self, *rows):
        return {'features': {'handoff': [
            {'scenario': name, 'groups': [], 'coverage': evidence} for name, evidence in rows
        ]}, 'groups': {}}

    def test_shared_new_scenario_does_not_conflict_with_an_independent_update(self):
        base = self.feature(('context', 'original'), ('helper', 'original'))
        ours = self.feature(('context', 'context test'), ('helper', 'original'), ('editor', 'editor test'))
        theirs = self.feature(('context', 'original'), ('helper', 'helper test'), ('editor', 'editor test'))
        self.branches(base, ours, theirs)
        expected = self.feature(('context', 'context test'), ('helper', 'helper test'), ('editor', 'editor test'))
        probe = self.sync_git('merge-tree', '--write-tree', 'main', 'incoming')
        self.assertEqual(probe.returncode, 0, probe.stderr)
        self.assertEqual(json.loads(self.git('show', f'{probe.stdout.strip()}:{MANIFEST}').stdout), expected)
        merged = self.sync_git('merge', '--no-commit', 'incoming')
        self.assertEqual(merged.returncode, 0, merged.stderr)
        self.assertEqual(json.loads(self.path.read_bytes()), expected)

    def test_removing_a_scenario_conflicts_with_new_evidence_for_it(self):
        self.branches(self.feature(('context', 'a'), ('editor', 'b')),
                      self.feature(('editor', 'b')),
                      self.feature(('context', 'changed'), ('editor', 'b')))
        self.require_conflict('features.handoff')

    def test_insertion_preserves_declared_order_and_existing_scenario_edits(self):
        base = self.feature(('old', 'a'), ('last', 'b'))
        ours = self.feature(('old', 'a'), ('new', 'c'), ('last', 'b'))
        theirs = self.feature(('old', 'changed'), ('last', 'b'))
        self.branches(base, ours, theirs)
        merged = self.sync_git('merge', '--no-commit', 'incoming')
        self.assertEqual(merged.returncode, 0, merged.stderr)
        self.assertEqual(json.loads(self.path.read_bytes()), self.feature(('old', 'changed'), ('new', 'c'), ('last', 'b')))

    def test_ambiguous_scenario_names_cannot_supply_independent_evidence(self):
        self.branches(self.feature(('old', 'a')), self.feature(('old', 'b'), ('old', 'c')),
                      self.feature(('old', 'changed')))
        self.require_conflict('duplicate scenario identity')

    def test_same_scenario_cannot_acquire_two_different_evidence_records(self):
        self.branches(self.feature(('editor', 'a')), self.feature(('editor', 'b')),
                      self.feature(('editor', 'c')))
        self.require_conflict('features.handoff')

    def test_independently_changed_scenario_order_requires_a_decision(self):
        self.branches(self.feature(('original', 'a')),
                      self.feature(('original', 'a'), ('left', 'b')),
                      self.feature(('original', 'a'), ('right', 'c')))
        self.require_conflict('features.handoff')

    def test_delete_against_change_requires_a_decision(self):
        self.branches({'features': {}, 'groups': {'a': ['x'], 'b': []}},
                      {'features': {}, 'groups': {'b': []}},
                      {'features': {}, 'groups': {'a': ['y'], 'b': []}})
        self.require_conflict('groups.a')

    def test_feature_cannot_adopt_a_group_removed_by_the_other_side(self):
        self.branches({'features': {}, 'groups': {'watch': {}}},
                      {'features': {}, 'groups': {}},
                      {'features': {'model': [{'groups': ['watch']}]}, 'groups': {'watch': {}}})
        self.require_conflict('model references missing group watch')

    def test_duplicate_keys_never_choose_a_winner(self):
        self.branches({'features': {}, 'groups': {'a': []}},
                      '{"features": {}, "groups": {"a": ["x"], "a": ["y"]}}\n',
                      {'features': {'new': []}, 'groups': {'a': []}})
        self.require_conflict('duplicate feature recipe key')

    def require_conflict(self, reason):
        result = self.sync_git('merge', '--no-commit', 'incoming')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn(reason, result.stderr)
        self.assertEqual(self.git('diff', '--name-only', '--diff-filter=U').stdout.strip(), str(MANIFEST))
        self.assertIn('<<<<<<<', self.path.read_text())


if __name__ == '__main__':
    unittest.main()
