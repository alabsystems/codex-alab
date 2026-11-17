# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Public fetch decisions use raw ancestry without changing caller graph files."""
import os
from pathlib import Path
import subprocess
import unittest

import test_github as read_tests


class GithubGraftTests(unittest.TestCase):
    def setUp(self):
        read_tests.GithubTests.setUp(self)
        read_tests.GITHUB.fetch(self.repo, str(self.remote), [self.specs[0]])
        self.tracking = 'refs/remotes/upstream/main'
        self.common = Path(read_tests.git(
            self.repo, 'rev-parse', '--path-format=absolute', '--git-common-dir'))
        self.graft = self.common / 'info/grafts'

    def graph_case(self, *, forward):
        arguments = ['commit-tree', self.head + '^{tree}']
        if forward:
            arguments.extend(['-p', self.head])
        new = read_tests.git(self.repo, *arguments, '-m', 'owned graph fixture')
        raw = read_tests.git(self.repo, 'cat-file', '-p', new)
        parents = [line for line in raw.splitlines() if line.startswith('parent ')]
        self.assertEqual(parents, ['parent ' + self.head] if forward else [])
        remote = self.home / 'graph remote with spaces.git'
        remote.mkdir()
        read_tests.git(remote, 'init', '-q', '--bare', '-b', 'main')
        read_tests.git(remote, 'fetch', '--no-tags', '--no-write-fetch-head',
                       str(self.repo), new + ':refs/heads/main')
        return new, remote

    def ordinary_ancestor(self, new, *, repo=None, replacements=False):
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith('GIT_')}
        environment.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1')
        if not replacements:
            environment['GIT_NO_REPLACE_OBJECTS'] = '1'
        done = subprocess.run([
            'git', '-c', 'core.hooksPath=' + os.devnull,
            '-c', 'core.fsmonitor=false', '-c', 'advice.graftFileDeprecated=false',
            '-C', str(repo or self.repo), 'merge-base', '--is-ancestor', self.head, new,
        ], env=environment, capture_output=True, timeout=15)
        self.assertIn(done.returncode, (0, 1), done.stderr)
        return done.returncode == 0

    def source_snapshot(self, repo=None):
        repo = repo or self.repo
        # Cache transfers are allowed before refusal. Every other ref and its
        # reflog, including the caller's HEAD, remains part of the source guard.
        refs = {name: oid for name, oid in read_tests.GITHUB.local_refs(repo).items()
                if not name.startswith('refs/agh/')}
        return {
            'config': (self.common / 'config').read_bytes(),
            'grafts': self.graft.read_bytes() if self.graft.exists() else None,
            'head': read_tests.git(repo, 'rev-parse', 'HEAD'),
            'refs': refs,
            'reflogs': {name: read_tests.git(repo, 'reflog', 'show', '--format=%H %gs', name)
                        for name in [*refs, 'HEAD']},
        }

    def fetch_case(self, remote, *, native, repo=None):
        client = read_tests.NativeFixture(remote)
        observed = read_tests.GITHUB.fetch(
            repo or self.repo, 'fixture/codex' if native else str(remote),
            [self.specs[0]], client=client)
        if not native:
            self.assertEqual(client.requests, [])
        return observed

    def assert_refused_without_source_changes(self, remote, *, native=False, repo=None):
        before = self.source_snapshot(repo)
        refusal = None
        try:
            self.fetch_case(remote, native=native, repo=repo)
        except read_tests.GITHUB.Refused as error:
            refusal = error
        after = self.source_snapshot(repo)
        self.assertEqual(after, before)
        self.assertIsInstance(refusal, read_tests.GITHUB.Refused)

    def assert_real_forward(self, new, remote, *, native):
        before = self.source_snapshot()
        refusal = None
        try:
            observed = self.fetch_case(remote, native=native)
        except read_tests.GITHUB.Refused as error:
            refusal = error
        after = self.source_snapshot()
        self.assertIsNone(refusal, str(refusal))
        self.assertEqual(observed, {'refs/heads/main': new})
        for field in ('config', 'grafts', 'head'):
            self.assertEqual(after[field], before[field])
        self.assertEqual(after['refs'], {**before['refs'], self.tracking: new})
        for name, log in before['reflogs'].items():
            if name == self.tracking:
                self.assertEqual(after['reflogs'][name].splitlines()[1:], log.splitlines())
                self.assertEqual(after['reflogs'][name].splitlines()[0],
                                 new + ' ALab Agh release fetch')
            else:
                self.assertEqual(after['reflogs'][name], log)

    def test_caller_graft_cannot_authorize_unrelated_local_fetch(self):
        new, remote = self.graph_case(forward=False)
        self.assertFalse(self.ordinary_ancestor(new))
        self.graft.write_text(new + ' ' + self.head + '\n')
        self.assertTrue(self.ordinary_ancestor(new))
        self.assert_refused_without_source_changes(remote)

    def test_caller_graft_cannot_authorize_unrelated_native_fixture_fetch(self):
        new, remote = self.graph_case(forward=False)
        self.graft.write_text(new + ' ' + self.head + '\n')
        self.assertTrue(self.ordinary_ancestor(new))
        self.assert_refused_without_source_changes(remote, native=True)

    def test_hidden_real_parent_still_allows_local_fast_forward(self):
        new, remote = self.graph_case(forward=True)
        self.assertTrue(self.ordinary_ancestor(new))
        self.graft.write_text(new + '\n')
        self.assertFalse(self.ordinary_ancestor(new))
        self.assert_real_forward(new, remote, native=False)

    def test_hidden_real_parent_still_allows_native_fixture_fast_forward(self):
        new, remote = self.graph_case(forward=True)
        self.graft.write_text(new + '\n')
        self.assertFalse(self.ordinary_ancestor(new))
        self.assert_real_forward(new, remote, native=True)

    def test_linked_worktree_common_graft_cannot_authorize_unrelated_fetch(self):
        new, remote = self.graph_case(forward=False)
        linked = self.home / 'linked caller with spaces'
        read_tests.git(self.repo, 'worktree', 'add', '-q', '--detach', str(linked), self.head)
        common = read_tests.git(linked, 'rev-parse', '--path-format=absolute', '--git-common-dir')
        self.assertEqual(Path(common), self.common)
        self.graft.write_text(new + ' ' + self.head + '\n')
        self.assertTrue(self.ordinary_ancestor(new, repo=linked))
        self.assert_refused_without_source_changes(remote, repo=linked)

    def test_replacement_refs_still_cannot_authorize_unrelated_fetch(self):
        new, remote = self.graph_case(forward=False)
        read_tests.git(self.repo, 'replace', '--graft', new, self.head)
        self.assertTrue(self.ordinary_ancestor(new, replacements=True))
        for native in (False, True):
            with self.subTest(native=native):
                self.assert_refused_without_source_changes(remote, native=native)


if __name__ == '__main__':
    unittest.main()
