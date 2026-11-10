# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('publication', Path(__file__).parents[1] / 'github_push.py')
PUSH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PUSH)
G = PUSH.GITHUB


def git(repo, *args):
    return G.git(repo, *args).decode().strip()


class Native:
    def __init__(self, remote):
        self.remote = remote
        self.calls = []
        self.failure = None
        self.race = None

    def call(self, *args):
        self.calls.append(args)
        if args[:2] == ('repo', 'clone'):
            git(self.remote, 'clone', '--mirror', str(self.remote), args[3])
            return b'{}'
        assert args[:2] == ('repo', 'push'), args
        row = json.loads(Path(args[5]).read_text())[0]
        assert git(args[3], 'rev-parse', 'refs/alab/candidate') == row['after']
        if self.race:
            git(self.remote, 'update-ref', 'refs/heads/main', self.race)
        if self.failure:
            error = G.Refused('fixture native failure')
            error.native_error = self.failure
            raise error
        git(self.remote, 'fetch', '--no-tags', args[3], 'refs/alab/candidate')
        git(self.remote, 'update-ref', row['ref'], row['after'], row['before'])
        return b'{"outcome":"created"}\n'

    def api(self, path, **kwargs):
        if '/git/matching-refs/' not in path:
            return {'full_name': 'fixture/codex'}
        return [{'ref': 'refs/heads/main', 'object': {'sha': git(self.remote, 'rev-parse', 'main')}}]

    def fetch_batch(self, identity, repo, rows, **kwargs):
        git(repo, 'fetch', '--no-tags', str(self.remote),
            *[row['source'] + ':' + row['into'] for row in rows])


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='alab-publish-test-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.repo = self.root / 'source with spaces'
        self.repo.mkdir()
        git(self.repo, 'init', '-q', '-b', 'main')
        git(self.repo, 'config', 'user.name', 'Fixture')
        git(self.repo, 'config', 'user.email', 'fixture@invalid')
        git(self.repo, 'commit', '-q', '--allow-empty', '-m', 'base')
        self.before = git(self.repo, 'rev-parse', 'HEAD')
        self.remote = self.root / 'remote.git'
        git(self.repo, 'clone', '--bare', str(self.repo), str(self.remote))
        git(self.repo, 'remote', 'add', 'origin', 'https://github.com/fixture/codex.git')
        git(self.repo, 'remote', 'add', 'upstream', 'https://github.com/openai/codex.git')
        git(self.repo, 'config', 'remote.upstream.pushurl', 'NEVER-PUSH-TO-UPSTREAM')
        git(self.repo, 'config', 'remote.origin.pushurl', 'NEVER-PUSH-USE-AGH')
        git(self.repo, 'commit', '-q', '--allow-empty', '-m', 'candidate')
        self.after = git(self.repo, 'rev-parse', 'HEAD')
        self.native = Native(self.remote)

    def publish(self):
        return PUSH.publish(self.repo, 'origin', self.before, client=self.native)

    def advance(self):
        git(self.repo, 'checkout', '-q', '--detach', self.before)
        git(self.repo, 'commit', '-q', '--allow-empty', '-m', 'concurrent')
        oid = git(self.repo, 'rev-parse', 'HEAD')
        git(self.remote, 'fetch', '--no-tags', str(self.repo), oid)
        git(self.repo, 'checkout', '-q', 'main')
        return oid

    def test_exact_graph_is_published_and_all_source_guards_remain(self):
        config = (self.repo / '.git/config').read_bytes()
        self.assertIn(b'created', self.publish())
        self.assertEqual(git(self.remote, 'rev-parse', 'main'), self.after)
        self.assertEqual((self.repo / '.git/config').read_bytes(), config)
        self.assertEqual([c[1] for c in self.native.calls], ['clone', 'push'])

    def test_advance_before_clone_is_a_proven_race_without_push(self):
        git(self.remote, 'update-ref', 'refs/heads/main', self.advance())
        with self.assertRaises(PUSH.Advanced): self.publish()
        self.assertEqual(len(self.native.calls), 1)

    def test_typed_lease_conflict_requires_a_confirmed_forward_advance(self):
        self.native.failure = dict(v=1, exit=9, kind='conflict', side_effect='none')
        self.native.race = self.advance()
        with self.assertRaises(PUSH.Advanced): self.publish()

    def test_auth_network_unknown_effect_and_untyped_failures_are_not_races(self):
        for failure in [dict(v=1, exit=4, kind='unauthenticated', side_effect='none'),
                        dict(v=1, exit=3, kind='could_not_run', side_effect='none'),
                        dict(v=1, exit=9, kind='conflict', side_effect='unknown'),
                        dict(v=0, exit=9, kind='conflict', side_effect='none')]:
            with self.subTest(failure=failure):
                git(self.remote, 'update-ref', 'refs/heads/main', self.before)
                self.native.failure = failure
                self.native.race = self.advance()
                with self.assertRaises(G.Refused) as caught: self.publish()
                self.assertNotIsInstance(caught.exception, PUSH.Advanced)

    def test_typed_conflict_without_main_movement_is_not_a_race(self):
        self.native.failure = dict(v=1, exit=9, kind='conflict', side_effect='none')
        with self.assertRaises(G.Refused) as caught: self.publish()
        self.assertNotIsInstance(caught.exception, PUSH.Advanced)

    def test_rewritten_remote_or_nonforward_candidate_refuses(self):
        git(self.repo, 'checkout', '-q', '--orphan', 'rewritten')
        git(self.repo, 'commit', '-q', '--allow-empty', '-m', 'different history')
        rewritten = git(self.repo, 'rev-parse', 'HEAD')
        with self.assertRaises(G.Refused): self.publish()
        git(self.remote, 'fetch', '--no-tags', str(self.repo), rewritten)
        git(self.remote, 'update-ref', 'refs/heads/main', rewritten)
        git(self.repo, 'checkout', '-q', 'main')
        with self.assertRaises((G.Refused, subprocess.CalledProcessError)): self.publish()


if __name__ == '__main__': unittest.main()
