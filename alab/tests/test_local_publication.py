# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exact local publication uses owned repositories and preserves source guards."""
import os
import shlex
import subprocess
import unittest
from unittest.mock import patch

import test_github_push as publication_tests


class LocalPublicationTests(unittest.TestCase):
    def setUp(self):
        publication_tests.PublicationTests.setUp(self)
        receiver = self.root / 'local receiver with spaces.git'
        self.remote.rename(receiver)
        self.remote = receiver
        self.native = publication_tests.Native(self.remote)

    def publish(self, expected=None, receiver=None):
        return publication_tests.PUSH.publish(
            self.repo, str(receiver or self.remote), expected or self.before, client=self.native)

    def source_snapshot(self):
        return {
            'config': (self.repo / '.git/config').read_bytes(),
            'head': publication_tests.git(self.repo, 'rev-parse', 'HEAD'),
            'refs': publication_tests.git(self.repo, 'for-each-ref', '--format=%(refname) %(objectname)'),
            'reflogs': publication_tests.git(self.repo, 'reflog', 'show', '--all', '--format=%H %gD %gs'),
        }

    def receiver_snapshot(self, receiver=None):
        receiver = receiver or self.remote
        return {
            'refs': publication_tests.git(receiver, 'for-each-ref', '--format=%(refname) %(objectname)'),
            'reflogs': publication_tests.git(receiver, 'reflog', 'show', '--all', '--format=%H %gD %gs'),
            'head': publication_tests.git(receiver, 'rev-parse', 'HEAD'),
        }

    def candidate_successor(self):
        middle = self.after
        publication_tests.git(self.repo, 'commit', '-q', '--allow-empty', '-m', 'candidate successor')
        candidate = publication_tests.git(self.repo, 'rev-parse', 'HEAD')
        publication_tests.git(self.remote, 'fetch', '--no-tags', '--no-write-fetch-head',
                              str(self.repo), candidate)
        return middle, candidate

    def push_seam(self, action=None):
        actual_git = publication_tests.G.git
        pushes = []

        def observed_git(repo, *arguments, **keywords):
            if arguments[0] == 'push':
                pushes.append((str(repo), arguments))
                if action is not None:
                    action(actual_git)
            return actual_git(repo, *arguments, **keywords)

        return patch.object(publication_tests.G, 'git', side_effect=observed_git), pushes

    def test_valid_local_fast_forward_preserves_source_guards_and_paths_with_spaces(self):
        before = self.source_snapshot()
        with patch.object(publication_tests.G, 'Agh', side_effect=AssertionError('no native client')):
            output = self.publish()
        self.assertIsInstance(output, bytes)
        self.assertEqual(publication_tests.git(self.remote, 'rev-parse', 'main'), self.after)
        self.assertEqual(self.source_snapshot(), before)
        self.assertEqual(self.native.calls, [])

    def test_stale_expected_main_refuses_before_push_even_when_candidate_fast_forwards_it(self):
        middle, _candidate = self.candidate_successor()
        publication_tests.git(self.remote, 'update-ref', 'refs/heads/main', middle, self.before)
        before, source = self.receiver_snapshot(), self.source_snapshot()
        seam, pushes = self.push_seam()
        with seam, self.assertRaises(publication_tests.PUSH.Advanced):
            self.publish()
        self.assertEqual(pushes, [])
        self.assertEqual(self.receiver_snapshot(), before)
        self.assertEqual(self.source_snapshot(), source)
        self.assertEqual(self.native.calls, [])

    def test_already_published_candidate_does_not_accept_a_stale_expected_main(self):
        publication_tests.git(self.remote, 'fetch', '--no-tags', '--no-write-fetch-head',
                              str(self.repo), self.after)
        publication_tests.git(self.remote, 'update-ref', 'refs/heads/main', self.after, self.before)
        before = self.receiver_snapshot()
        seam, pushes = self.push_seam()
        with seam, self.assertRaises(publication_tests.PUSH.Advanced):
            self.publish()
        self.assertEqual(pushes, [])
        self.assertEqual(self.receiver_snapshot(), before)

    def test_writer_after_preflight_preserves_middle_and_already_candidate_values(self):
        middle, candidate = self.candidate_successor()
        source = self.source_snapshot()
        for winner in (middle, candidate):
            with self.subTest(winner=winner):
                publication_tests.git(self.remote, 'update-ref', 'refs/heads/main', self.before)
                post_writer = []

                def writer(actual_git):
                    actual_git(self.remote, 'update-ref', '-m', 'owned competing writer',
                               'refs/heads/main', winner, self.before)
                    post_writer.append(self.receiver_snapshot())

                seam, pushes = self.push_seam(writer)
                refusal = (publication_tests.PUSH.Advanced if winner == candidate
                           else publication_tests.G.Refused)
                with seam, self.assertRaises(refusal):
                    self.publish()
                self.assertEqual(len(pushes), 1)
                self.assertEqual(publication_tests.git(self.remote, 'rev-parse', 'main'), winner)
                self.assertEqual(self.receiver_snapshot(), post_writer[0])
                self.assertEqual(self.source_snapshot(), source)
                self.assertEqual(self.native.calls, [])

    def test_expected_equal_to_candidate_is_a_valid_noop(self):
        publication_tests.git(self.remote, 'fetch', '--no-tags', '--no-write-fetch-head',
                              str(self.repo), self.after)
        publication_tests.git(self.remote, 'update-ref', 'refs/heads/main', self.after, self.before)
        before, source = self.receiver_snapshot(), self.source_snapshot()
        self.assertIsInstance(self.publish(expected=self.after), bytes)
        self.assertEqual(self.receiver_snapshot(), before)
        self.assertEqual(self.source_snapshot(), source)
        self.assertEqual(self.native.calls, [])

    def test_source_graft_cannot_fake_canonical_fast_forward_before_push(self):
        unrelated = publication_tests.git(self.repo, 'commit-tree', self.before + '^{tree}',
                                           '-m', 'unrelated owned candidate')
        publication_tests.git(self.repo, 'update-ref', 'refs/heads/main', unrelated, self.after)
        graft = self.repo / '.git/info/grafts'
        graft.write_text(unrelated + ' ' + self.before + '\n')
        environment = {key: value for key, value in os.environ.items() if not key.startswith('GIT_')}
        environment.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1',
                           GIT_NO_REPLACE_OBJECTS='1', GIT_GRAFT_FILE=str(graft))
        # The ordinary local graft view claims ancestry; publication must use raw objects.
        fake = subprocess.check_output([
            'git', '-c', 'advice.graftFileDeprecated=false', '-C', str(self.repo),
            'merge-base', self.before, unrelated,
        ], env=environment, stderr=subprocess.PIPE, text=True).strip()
        self.assertEqual(fake, self.before)
        before = self.receiver_snapshot()
        for receiver in (str(self.remote), 'origin'):
            with self.subTest(receiver=receiver):
                seam, pushes = self.push_seam()
                with seam, patch.object(self.native, 'api', side_effect=AssertionError('no native API')), \
                        self.assertRaises(publication_tests.G.Refused) as caught:
                    self.publish(receiver=receiver)
                self.assertNotIsInstance(caught.exception, publication_tests.PUSH.Advanced)
                self.assertEqual(pushes, [])
                self.assertEqual(self.receiver_snapshot(), before)
                self.assertEqual(self.native.calls, [])

    def test_receive_hook_refusal_still_runs_receiver_policy(self):
        hooks = self.root / 'receiver hooks with spaces'
        hooks.mkdir()
        marker = self.root / 'receive hook ran'
        hook = hooks / 'pre-receive'
        hook.write_text('#!/bin/sh\nprintf "owned hook ran\\n" > ' + shlex.quote(str(marker)) + '\nexit 1\n')
        hook.chmod(0o755)
        publication_tests.git(self.remote, 'config', 'core.hooksPath', str(hooks))
        before, source = self.receiver_snapshot(), self.source_snapshot()
        with self.assertRaises(publication_tests.G.Refused) as caught:
            self.publish()
        self.assertNotIsInstance(caught.exception, publication_tests.PUSH.Advanced)
        self.assertEqual(marker.read_text(), 'owned hook ran\n')
        self.assertEqual(self.receiver_snapshot(), before)
        self.assertEqual(self.source_snapshot(), source)

    def test_nonbare_checked_out_branch_refusal_is_preserved(self):
        receiver = self.root / 'checked out receiver with spaces'
        publication_tests.git(self.root, 'clone', '--quiet', str(self.remote), str(receiver))
        before, source = self.receiver_snapshot(receiver), self.source_snapshot()
        with self.assertRaises(publication_tests.G.Refused) as caught:
            self.publish(receiver=receiver)
        self.assertNotIsInstance(caught.exception, publication_tests.PUSH.Advanced)
        self.assertEqual(self.receiver_snapshot(receiver), before)
        self.assertEqual(self.source_snapshot(), source)
        self.assertEqual(self.native.calls, [])


if __name__ == '__main__':
    unittest.main()
