#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Promotion reads canonical ancestry without changing the selected artifact lane."""

import os
from pathlib import Path
import stat
import subprocess
import unittest

import test_promote as promotion_tests


class PromotionGraftsTests(unittest.TestCase):
    package = promotion_tests.PromotionTests.package
    select = promotion_tests.PromotionTests.select
    link = promotion_tests.PromotionTests.link
    promote = promotion_tests.PromotionTests.promote

    def setUp(self):
        promotion_tests.PromotionTests.setUp(self)
        self.common = Path(self.git('rev-parse', '--path-format=absolute', '--git-common-dir'))
        self.graft = self.common / 'info/grafts'

    def command(self, *arguments, canonical=True, replacements=False):
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith('GIT_')}
        environment.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1')
        if canonical:
            environment['GIT_GRAFT_FILE'] = os.devnull
        if not replacements:
            environment['GIT_NO_REPLACE_OBJECTS'] = '1'
        return subprocess.run(['git', '-C', str(self.repo), *arguments],
                              capture_output=True, timeout=60, env=environment)

    def git(self, *arguments):
        result = self.command(*arguments)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.decode().strip()

    def parents(self, commit):
        result = self.command('cat-file', 'commit', commit)
        self.assertEqual(result.returncode, 0, result.stderr)
        return [line for line in result.stdout.split(b'\n\n', 1)[0].split(b'\n')
                if line.startswith(b'parent ')]

    def ancestor(self, older, newer, *, canonical=True, replacements=False):
        result = self.command('merge-base', '--is-ancestor', older, newer,
                              canonical=canonical, replacements=replacements)
        self.assertIn(result.returncode, (0, 1), result.stderr)
        return result.returncode == 0

    def root_commit(self, message):
        commit = self.git('commit-tree', self.old_source + '^{tree}', '-m', message)
        self.assertEqual(self.parents(commit), [])
        return commit

    def snapshot(self):
        return {
            'config': (self.common / 'config').read_bytes(),
            'graft': self.graft.read_bytes() if self.graft.exists() else None,
            'marker': self.marker.read_bytes(),
            'packages': {str(path.relative_to(self.root)):
                         (stat.S_IMODE(path.stat().st_mode), path.read_bytes())
                         for path in self.root.rglob('*') if path.is_file()},
            'head_file': (self.common / 'HEAD').read_bytes(),
            'head': self.git('rev-parse', 'HEAD'),
            'refs': self.git('for-each-ref', '--format=%(refname) %(objectname) %(symref)'),
            'reflogs': {str(path.relative_to(self.common / 'logs')): path.read_bytes()
                        for path in (self.common / 'logs').rglob('*') if path.is_file()},
        }

    def write_graft(self, child, parent=None):
        self.graft.write_text(child + (' ' + parent if parent else '') + '\n')

    def assert_refused(self, candidate, commit):
        before = self.snapshot()
        selected, reason = promotion_tests.PROMOTE.promote(
            self.repo, self.root, self.prefix, candidate, commit,
            aterm='/owned host/aterm', run=self.link)
        self.assertFalse(selected, reason)
        self.assertEqual(self.calls, [], 'ineligible ancestry must never reach the selector')
        self.assertEqual(self.snapshot(), before)

    def assert_forward(self):
        before = self.snapshot()
        selected, reason = self.promote()
        self.assertTrue(selected, reason)
        self.assertEqual(self.calls, [
            ['/owned host/aterm', 'pkg', 'link', '--help'],
            ['/owned host/aterm', 'pkg', 'link', 'codex', str(self.new), 'bin/codex',
             '--if-checkout', str(self.old), '--if-prefix', str(self.prefix),
             '--wait-lock', '1800'],
        ])
        self.assertEqual(self.snapshot(), before, 'the fake selector changes no installed lane')

    def test_grafted_unrelated_candidate_cannot_advance_selected_source(self):
        unrelated = self.root_commit('unrelated candidate')
        candidate = self.package(unrelated, 3)
        self.git('update-ref', 'refs/remotes/origin/main', unrelated)
        self.assertFalse(self.ancestor(self.old_source, unrelated))
        self.assertTrue(self.ancestor(unrelated, 'refs/remotes/origin/main'))
        self.write_graft(unrelated, self.old_source)
        self.assertTrue(self.ancestor(self.old_source, unrelated, canonical=False))
        self.assertEqual(self.parents(unrelated), [])
        self.assert_refused(candidate, unrelated)

    def test_grafted_origin_cannot_admit_candidate_outside_main(self):
        unrelated = self.root_commit('unrelated origin main')
        self.git('update-ref', 'refs/remotes/origin/main', unrelated)
        self.assertEqual(self.parents(self.source), [b'parent ' + self.old_source.encode()])
        self.assertTrue(self.ancestor(self.old_source, self.source))
        self.assertFalse(self.ancestor(self.source, 'refs/remotes/origin/main'))
        self.write_graft(unrelated, self.source)
        self.assertTrue(self.ancestor(self.source, 'refs/remotes/origin/main', canonical=False))
        self.assertEqual(self.parents(unrelated), [])
        self.assert_refused(self.new, self.source)

    def test_graft_hiding_selected_parent_does_not_hide_forward_update(self):
        self.assertEqual(self.parents(self.source), [b'parent ' + self.old_source.encode()])
        self.write_graft(self.source)
        self.assertFalse(self.ancestor(self.old_source, self.source, canonical=False))
        self.assertTrue(self.ancestor(self.old_source, self.source))
        self.assertTrue(self.ancestor(self.source, 'refs/remotes/origin/main'))
        self.assert_forward()

    def test_graft_hiding_origin_parent_does_not_hide_forward_update(self):
        self.git('commit', '-q', '--allow-empty', '-m', 'later origin main')
        main = self.git('rev-parse', 'HEAD')
        self.git('update-ref', 'refs/remotes/origin/main', main)
        self.assertEqual(self.parents(main), [b'parent ' + self.source.encode()])
        self.write_graft(main)
        self.assertTrue(self.ancestor(self.old_source, self.source))
        self.assertFalse(self.ancestor(self.source, main, canonical=False))
        self.assertTrue(self.ancestor(self.source, main))
        self.assert_forward()

    def test_replacement_object_still_cannot_forge_candidate_ancestry(self):
        unrelated = self.root_commit('unrelated replacement candidate')
        candidate = self.package(unrelated, 3)
        self.git('update-ref', 'refs/remotes/origin/main', unrelated)
        replacement = self.git('commit-tree', self.old_source + '^{tree}',
                               '-p', self.old_source, '-m', 'replacement with forged parent')
        self.git('update-ref', 'refs/replace/' + unrelated, replacement)
        self.assertTrue(self.ancestor(self.old_source, unrelated, replacements=True))
        self.assertFalse(self.ancestor(self.old_source, unrelated))
        self.assertEqual(self.parents(unrelated), [])
        self.assert_refused(candidate, unrelated)


if __name__ == '__main__':
    unittest.main()
