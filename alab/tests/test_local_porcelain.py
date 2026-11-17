# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Real Git output treats the exact known local receiver header as path data."""
import os
import unittest
from unittest.mock import patch

import test_local_publication as local_tests
import test_github_push as publication_tests


class LocalPorcelainTests(unittest.TestCase):
    setUp = local_tests.LocalPublicationTests.setUp
    publish = local_tests.LocalPublicationTests.publish
    source_snapshot = local_tests.LocalPublicationTests.source_snapshot
    receiver_snapshot = local_tests.LocalPublicationTests.receiver_snapshot
    push_seam = local_tests.LocalPublicationTests.push_seam

    def receiver_named(self, name):
        self.remote = self.remote.rename(self.root / name)

    def assert_real_fast_forward(self, name):
        self.receiver_named(name)
        identity, network = publication_tests.G.location(self.repo, str(self.remote))
        self.assertFalse(network)
        before = self.source_snapshot()
        actual_git = publication_tests.G.git
        raw = []

        def capture(repo, *arguments, **keywords):
            value = actual_git(repo, *arguments, **keywords)
            if arguments[0] == 'push':
                raw.append(value)
            return value

        with patch.object(publication_tests.G, 'git', side_effect=capture):
            output = self.publish()
        self.assertEqual(raw, [output])
        self.assertTrue(output.startswith(b'To ' + os.fsencode(identity) + b'\n'))
        self.assertTrue(output.endswith(b'\nDone\n'))
        self.assertEqual(publication_tests.git(self.remote, 'rev-parse', 'main'), self.after)
        self.assertEqual(self.source_snapshot(), before)
        self.assertEqual(self.native.calls, [])

    def test_tabs_and_nonascii_remain_literal_header_path_bytes(self):
        for name in ('receiver\twith\ttabs.git', 'r\u00e9cepteur-\u2603.git'):
            with self.subTest(name=name):
                publication_tests.git(self.remote, 'update-ref', 'refs/heads/main', self.before)
                self.assert_real_fast_forward(name)

    def test_embedded_newline_status_prefix_is_header_data(self):
        self.assert_real_fast_forward('receiver\n \tstatus continuation.git')

    def test_carriage_return_remains_literal_header_path_data(self):
        self.assert_real_fast_forward('receiver\rcarriage.git')

    def test_noop_with_record_shaped_header_keeps_valid_equal_status(self):
        self.receiver_named('receiver\n \tstatus continuation.git')
        publication_tests.git(self.remote, 'fetch', '--no-tags', '--no-write-fetch-head',
                              str(self.repo), self.after)
        publication_tests.git(self.remote, 'update-ref', 'refs/heads/main', self.after, self.before)
        before, source = self.receiver_snapshot(), self.source_snapshot()
        self.assertIsInstance(self.publish(expected=self.after), bytes)
        self.assertEqual(self.receiver_snapshot(), before)
        self.assertEqual(self.source_snapshot(), source)

    def test_stale_equal_status_after_preflight_remains_advanced_with_literal_header(self):
        self.receiver_named('receiver\n \tstatus continuation.git')
        publication_tests.git(self.remote, 'fetch', '--no-tags', '--no-write-fetch-head',
                              str(self.repo), self.after)
        written = []

        def writer(actual_git):
            actual_git(self.remote, 'update-ref', 'refs/heads/main', self.after, self.before)
            written.append(self.receiver_snapshot())

        source = self.source_snapshot()
        seam, pushes = self.push_seam(writer)
        with seam, self.assertRaises(publication_tests.PUSH.Advanced):
            self.publish()
        self.assertEqual(len(pushes), 1)
        self.assertEqual(self.receiver_snapshot(), written[0])
        self.assertEqual(self.source_snapshot(), source)

    def test_malformed_or_unknown_response_frame_stays_refused(self):
        identity, _network = publication_tests.G.location(self.repo, str(self.remote))
        header = b'To ' + os.fsencode(identity) + b'\n'
        faults = {
            'unknown header': lambda raw: b'Unexpected header\n' + raw[len(header):],
            'missing Done': lambda raw: raw[:-len(b'Done\n')],
            'trailing data': lambda raw: raw + b'extra data\n',
            'unknown status': lambda raw: header + b'?\t' + raw[len(header) + 2:],
        }
        source = self.source_snapshot()
        actual_git = publication_tests.G.git
        for label, corrupt in faults.items():
            with self.subTest(response_fault=label):
                publication_tests.git(self.remote, 'update-ref', 'refs/heads/main', self.before)
                pushes = []

                def response_fault(repo, *arguments, **keywords):
                    raw = actual_git(repo, *arguments, **keywords)
                    if arguments[0] == 'push':
                        pushes.append(raw)
                        self.assertTrue(raw.startswith(header + b' \t'))
                        return corrupt(raw)
                    return raw

                with patch.object(publication_tests.G, 'git', side_effect=response_fault), \
                        self.assertRaises(publication_tests.G.Refused) as caught:
                    self.publish()
                self.assertNotIsInstance(caught.exception, publication_tests.PUSH.Advanced)
                self.assertEqual(len(pushes), 1)
                # A response fault after a real write must not be mistaken for confirmed success.
                self.assertEqual(publication_tests.git(self.remote, 'rev-parse', 'main'), self.after)
                self.assertEqual(self.source_snapshot(), source)


if __name__ == '__main__':
    unittest.main()
