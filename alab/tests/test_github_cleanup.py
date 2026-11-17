# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Public scratch owners preserve failures while retiring only real owned directories."""
from contextlib import ExitStack
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import test_github_push as publication_tests

SPEC = importlib.util.spec_from_file_location('cleanup_versions',
                                             Path(__file__).parents[1] / 'versions.py')
VERSIONS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERSIONS)


class ReportingFailure(RuntimeError):
    def __init__(self, fault):
        super().__init__('primary body failure')
        self.fault = fault

    def add_note(self, _note):
        raise self.fault


class UnrepresentableCleanup(OSError):
    def __init__(self, fault):
        super().__init__('owned cleanup failure')
        self.fault = fault

    def __repr__(self):
        raise self.fault


class GithubCleanupTests(unittest.TestCase):
    def setUp(self):
        publication_tests.PublicationTests.setUp(self)
        self.empty_releases = self.root / 'empty releases.git'
        publication_tests.git(self.root, 'init', '--quiet', '--bare', str(self.empty_releases))

    def invoke(self, consumer, primary=None, cleanup_fault=None, creation_fault=None):
        created, cleaned, bodies = [], [], []

        class OwnedDirectory(tempfile.TemporaryDirectory):
            def __init__(self, **keywords):
                if creation_fault is not None:
                    raise creation_fault
                super().__init__(**keywords)
                created.append(self)

            def cleanup(self):
                super().cleanup()
                cleaned.append(self.name)
                if cleanup_fault is not None:
                    raise cleanup_fault

        proxy = SimpleNamespace(**{**vars(tempfile), 'TemporaryDirectory': OwnedDirectory})
        github = VERSIONS.GITHUB if consumer == 'released' else publication_tests.G
        caller = VERSIONS if consumer == 'released' else publication_tests.PUSH
        with ExitStack() as patches:
            patches.enter_context(patch.object(github, 'tempfile', proxy))
            if caller is not github:
                patches.enter_context(patch.object(caller, 'tempfile', proxy, create=True))
            if consumer == 'fetch_batch':
                client = object.__new__(github.Agh)
                rows = [dict(source='refs/heads/main', into='refs/agh/owned', expected_oid=self.after)]

                def call(*arguments, **_keywords):
                    bodies.append('fetch_batch')
                    self.assertEqual(json.loads(Path(arguments[5]).read_text()), rows)
                    if primary is not None:
                        raise primary
                    return b'{}'

                patches.enter_context(patch.object(client, 'call', side_effect=call))
                action = lambda: client.fetch_batch('fixture/codex', self.repo, rows)
            elif consumer == 'publish':
                actual_git = github.git

                def git(repo, *arguments, **keywords):
                    if arguments[0] == 'push':
                        bodies.append('publish')
                        if primary is not None:
                            raise primary
                    return actual_git(repo, *arguments, **keywords)

                patches.enter_context(patch.object(github, 'git', side_effect=git))
                action = lambda: publication_tests.PUSH.publish(
                    self.repo, str(self.remote), self.before, client=self.native)
            else:
                actual_list = github.list_refs

                def list_refs(*arguments, **keywords):
                    value = actual_list(*arguments, **keywords)
                    bodies.append('released')
                    self.assertEqual(value, {})
                    if primary is not None:
                        raise primary
                    return value

                patches.enter_context(patch.object(github, 'list_refs', side_effect=list_refs))
                action = lambda: VERSIONS.released(self.repo, str(self.empty_releases), self.before)
            try:
                return action()
            finally:
                leaked = []
                for owner in created:
                    if Path(owner.name).exists():
                        leaked.append(owner.name)
                        tempfile.TemporaryDirectory.cleanup(owner)
                self.assertEqual(leaked, [])
                self.assertEqual(cleaned, [owner.name for owner in created])
                self.assertEqual(len(created), 0 if creation_fault is not None else 1)
                self.assertEqual(len(bodies), 0 if creation_fault is not None else 1)

    def assert_escapes(self, expected, consumer, **faults):
        with self.assertRaises(BaseException) as caught:
            self.invoke(consumer, **faults)
        self.assertTrue(caught.exception is expected, 'the exact selected exception must escape')
        return caught.exception

    def test_successful_public_contexts_remove_their_directory_once(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            with self.subTest(consumer=consumer):
                self.assertIsNotNone(self.invoke(consumer))

    def test_clean_cleanup_preserves_primary_identity(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            for primary in (KeyboardInterrupt('body cancellation'), RuntimeError('body failure')):
                with self.subTest(consumer=consumer, primary=type(primary).__name__):
                    self.assert_escapes(primary, consumer, primary=primary)

    def test_pending_cancellation_survives_ordinary_cleanup_failure(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            for primary in (KeyboardInterrupt('body cancellation'), SystemExit(47)):
                with self.subTest(consumer=consumer, primary=type(primary).__name__):
                    cleanup = OSError('owned cleanup failure')
                    self.assert_escapes(primary, consumer, primary=primary,
                                        cleanup_fault=cleanup)
                    self.assertIn('temporary directory cleanup failed: ' + repr(cleanup), primary.__notes__)

    def test_pending_cancellation_beats_later_cleanup_cancellation(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            for primary, cleanup in ((KeyboardInterrupt('body cancellation'), SystemExit(47)),
                                     (SystemExit(47), KeyboardInterrupt('cleanup cancellation'))):
                with self.subTest(consumer=consumer, primary=type(primary).__name__):
                    self.assert_escapes(primary, consumer, primary=primary, cleanup_fault=cleanup)
                    self.assertIn('temporary directory cleanup failed: ' + repr(cleanup), primary.__notes__)

    def test_zero_and_none_system_exit_survive_cleanup_failure(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            for code in (0, None):
                with self.subTest(consumer=consumer, code=code):
                    primary = SystemExit(code)
                    self.assert_escapes(primary, consumer, primary=primary,
                                        cleanup_fault=OSError('owned cleanup failure'))

    def test_ordinary_primary_survives_with_natural_secondary_note(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            with self.subTest(consumer=consumer):
                primary, cleanup = RuntimeError('body failure'), OSError('owned cleanup failure')
                self.assert_escapes(primary, consumer, primary=primary, cleanup_fault=cleanup)
                self.assertIn('temporary directory cleanup failed: ' + repr(cleanup), primary.__notes__)

    def test_cleanup_cancellation_promotes_over_ordinary_primary(self):
        cases = (('fetch_batch', KeyboardInterrupt('cleanup cancellation')),
                 ('publish', SystemExit(0)), ('released', SystemExit(None)))
        for consumer, cancellation in cases:
            with self.subTest(consumer=consumer):
                primary = RuntimeError('body failure')
                caught = self.assert_escapes(cancellation, consumer, primary=primary,
                                             cleanup_fault=cancellation)
                self.assertIs(caught.__cause__, primary)

    def test_successful_body_cleanup_failure_remains_visible(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            with self.subTest(consumer=consumer):
                cleanup = OSError('owned cleanup failure')
                self.assert_escapes(cleanup, consumer, cleanup_fault=cleanup)

    def test_creation_failure_starts_no_body_and_owns_no_directory(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            with self.subTest(consumer=consumer):
                failure = OSError('owned scratch creation failed')
                self.assert_escapes(failure, consumer, creation_fault=failure)

    def test_cleanup_repr_cancellation_cannot_replace_pending_cancellation(self):
        primary = KeyboardInterrupt('body cancellation')
        self.assert_escapes(primary, 'fetch_batch', primary=primary,
                            cleanup_fault=UnrepresentableCleanup(SystemExit(47)))

    def test_reporting_cancellation_promotes_over_ordinary_primary(self):
        cancellation = SystemExit(0)
        primary = ReportingFailure(cancellation)
        caught = self.assert_escapes(cancellation, 'fetch_batch', primary=primary,
                                     cleanup_fault=OSError('owned cleanup failure'))
        self.assertIs(caught.__cause__, primary)

    def test_ordinary_reporting_failure_preserves_primary(self):
        primary = ReportingFailure(RuntimeError('note failure'))
        self.assert_escapes(primary, 'fetch_batch', primary=primary,
                            cleanup_fault=OSError('owned cleanup failure'))


if __name__ == '__main__':
    unittest.main()
