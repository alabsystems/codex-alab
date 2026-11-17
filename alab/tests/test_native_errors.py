# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""ALab confirms native absence/races only from canonical integer metadata."""
import json
import subprocess
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import test_github as read_tests
import test_github_push as push_tests


class NativeErrorReadTests(unittest.TestCase):
    def request(self, error, code=6):
        github = read_tests.GITHUB
        client = object.__new__(github.Agh)
        client.executable = '/owned-response-fixture/never-invoked'
        client.identity = 'response fixture identity'
        calls = []
        stderr = ('agh: native failure\n' + json.dumps({'error': error}) + '\n').encode()

        def response(arguments, **_keywords):
            calls.append(arguments)
            return subprocess.CompletedProcess(arguments, code, b'', stderr)

        proxy = SimpleNamespace(**{**vars(subprocess), 'run': response})
        with patch.object(github, 'subprocess', proxy), \
                patch.object(client, 'file_identity', return_value=client.identity):
            try:
                return client.api('/repos/fixture/codex', absent=True)
            finally:
                self.assertEqual(len(calls), 1)

    def missing(self, **changes):
        return dict(v=1, exit=6, side_effect='none', kind='not_found', status=404) | changes

    def test_canonical_integer_absence_and_empty_repository_remain_absent(self):
        for code, error in (
            (6, self.missing()),
            (9, dict(v=1, exit=9, side_effect='none', kind='conflict', status=409,
                     message='Git Repository is empty.')),
        ):
            with self.subTest(code=code):
                self.assertIsNone(self.request(error, code))

    def test_noncanonical_version_aliases_do_not_confirm_absence(self):
        for version in (True, 1.0, '1', None, [1]):
            with self.subTest(version=version, scalar_type=type(version).__name__):
                with self.assertRaises(read_tests.GITHUB.Refused) as caught:
                    self.request(self.missing(v=version))
                self.assertIsNone(caught.exception.native_error)

    def test_noncanonical_exit_aliases_do_not_confirm_absence(self):
        for exit_status in (6.0, '6', True, None):
            with self.subTest(exit_status=exit_status, scalar_type=type(exit_status).__name__):
                with self.assertRaises(read_tests.GITHUB.Refused) as caught:
                    self.request(self.missing(exit=exit_status))
                self.assertIsNone(caught.exception.native_error)

    def test_noncanonical_http_status_cannot_confirm_absence_or_typed_error(self):
        for code, error in (
            (6, self.missing(status=404.0)),
            (6, self.missing(status='404')),
            (9, dict(v=1, exit=9, side_effect='none', kind='conflict', status=409.0,
                     message='Git Repository is empty.')),
            (9, dict(v=1, exit=9, side_effect='none', kind='conflict', status='409',
                     message='Git Repository is empty.')),
        ):
            with self.subTest(status=error['status'], scalar_type=type(error['status']).__name__):
                with self.assertRaises(read_tests.GITHUB.Refused) as caught:
                    self.request(error, code)
                self.assertIsNone(caught.exception.native_error)

    def test_missing_and_null_optional_http_status_preserve_generic_error_annotation(self):
        for optional in ({}, {'status': None}):
            error = dict(v=1, exit=3, side_effect='none', kind='could_not_run') | optional
            with self.subTest(optional=optional):
                with self.assertRaises(read_tests.GITHUB.Refused) as caught:
                    self.request(error, 3)
                self.assertEqual(caught.exception.native_error, error)

    def test_permission_unknown_effect_and_mismatched_exit_stay_refused(self):
        cases = (
            (5, self.missing(exit=5, kind='unreadable'), True),
            (6, self.missing(side_effect='unknown'), False),
            (6, self.missing(exit=5), False),
        )
        for code, error, annotated in cases:
            with self.subTest(error=error):
                with self.assertRaises(read_tests.GITHUB.Refused) as caught:
                    self.request(error, code)
                self.assertEqual(caught.exception.native_error, error if annotated else None)


class RecordedNative(push_tests.Native):
    def __init__(self, remote):
        super().__init__(remote)
        self.api_requests = []
        self.raised_failure = None

    def api(self, path, **keywords):
        self.api_requests.append(path)
        return super().api(path, **keywords)

    def call(self, *arguments):
        try:
            return super().call(*arguments)
        except push_tests.G.Refused as failure:
            self.raised_failure = failure
            raise


class NativePublicationErrorTests(unittest.TestCase):
    publish = push_tests.PublicationTests.publish
    advance = push_tests.PublicationTests.advance

    def setUp(self):
        push_tests.PublicationTests.setUp(self)
        self.native = RecordedNative(self.remote)

    def test_canonical_conflict_requires_fresh_authoritative_forward_main(self):
        config = (self.repo / '.git/config').read_bytes()
        self.native.failure = dict(v=1, exit=9, kind='conflict', side_effect='none')
        self.native.race = self.advance()
        with self.assertRaises(push_tests.PUSH.Advanced) as caught:
            self.publish()
        self.assertIs(caught.exception.__cause__, self.native.raised_failure)
        self.assertEqual(len(self.native.api_requests), 2)
        self.assertEqual([call[1] for call in self.native.calls], ['fetch', 'push', 'fetch'])
        self.assertEqual(push_tests.git(self.remote, 'rev-parse', 'main'), self.native.race)
        self.assertEqual((self.repo / '.git/config').read_bytes(), config)
        self.assertEqual(push_tests.git(self.repo, 'rev-parse', 'HEAD'), self.after)

    def test_canonical_conflict_without_forward_main_rethrows_original_failure(self):
        self.native.failure = dict(v=1, exit=9, kind='conflict', side_effect='none', status=None)
        with self.assertRaises(push_tests.G.Refused) as caught:
            self.publish()
        self.assertIs(caught.exception, self.native.raised_failure)
        self.assertNotIsInstance(caught.exception, push_tests.PUSH.Advanced)
        self.assertEqual(len(self.native.api_requests), 2)
        self.assertEqual(push_tests.git(self.remote, 'rev-parse', 'main'), self.before)

    def test_malformed_conflict_metadata_rethrows_original_without_race_classification(self):
        canonical = dict(v=1, exit=9, kind='conflict', side_effect='none')
        failures = (
            canonical | {'v': True}, canonical | {'v': 1.0},
            canonical | {'exit': 9.0}, canonical | {'v': '1'},
            canonical | {'status': 409.0}, canonical | {'status': True},
            canonical | {'status': '409'},
            canonical | {'side_effect': 'unknown'},
            [canonical], 'conflict', True, 47,
        )
        config = (self.repo / '.git/config').read_bytes()
        for metadata in failures:
            with self.subTest(metadata=metadata):
                push_tests.git(self.remote, 'update-ref', 'refs/heads/main', self.before)
                self.native = RecordedNative(self.remote)
                self.native.failure = metadata
                self.native.race = self.advance()
                with self.assertRaises(push_tests.G.Refused) as caught:
                    self.publish()
                self.assertIs(caught.exception, self.native.raised_failure)
                self.assertNotIsInstance(caught.exception, push_tests.PUSH.Advanced)
                self.assertEqual(len(self.native.api_requests), 1)
                self.assertEqual(push_tests.git(self.remote, 'rev-parse', 'main'), self.native.race)
                self.assertEqual((self.repo / '.git/config').read_bytes(), config)
                self.assertEqual(push_tests.git(self.repo, 'rev-parse', 'HEAD'), self.after)


if __name__ == '__main__':
    unittest.main()
