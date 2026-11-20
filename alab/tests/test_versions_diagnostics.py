# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Version CLI failures retain natural diagnostics without becoming successful exits."""
import contextlib
import errno
import io
import os
from pathlib import Path
import unittest
from unittest.mock import patch

import test_version_ledger as ledger_tests


class VersionsDiagnosticsTests(unittest.TestCase):
    def check_arguments(self):
        return ['check', '--repo', 'unused-owned-repo', '--commit', 'a' * 40,
                '--version', '0.1.0', '--baseline', '0.164.0',
                '--baseline-commit', 'b' * 40, '--main', 'unused-owned-main',
                '--released', 'unused-owned-public']

    def invoke_main(self, arguments):
        stdout, stderr = io.StringIO(), io.StringIO()
        returned, escaped = None, None
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            try:
                returned = ledger_tests.VERSIONS.main(arguments)
            except BaseException as error:
                escaped = error
        return returned, escaped, stdout.getvalue(), stderr.getvalue()

    def release_fixture(self, *, malformed=False):
        fixture = ledger_tests.Scratch()
        self.addCleanup(fixture.doCleanups)
        fixture.setUp()
        fixture.on(fixture.u3, 'main-on-u3')
        source, _ = fixture.layer(
            ledger=ledger_tests.HEADER + fixture.row('0.1.0', '0.164.0'), land=False)
        fixture.git(fixture.alab, 'push', '-q', 'origin', source + ':refs/heads/main')
        message = ('owned malformed public snapshot' if malformed else
                   'snapshot: 2026-11-16 from codex-alab@' + source)
        snapshot = fixture.git(fixture.alab, 'commit-tree', source + '^{tree}', '-m', message)
        fixture.git(fixture.alab, 'push', '-q', str(fixture.public), snapshot + ':refs/heads/main')
        arguments = ['check', '--repo', str(fixture.alab), '--commit', source,
                     '--version', '0.1.0', '--baseline', '0.164.0',
                     '--baseline-commit', fixture.tags['0.164.0'],
                     '--main', 'refs/remotes/origin/main', '--released', fixture.public.as_uri()]
        return fixture, arguments

    def source_snapshot(self, fixture):
        return {
            'config': (fixture.alab / '.git/config').read_bytes(),
            'head': fixture.git(fixture.alab, 'rev-parse', 'HEAD'),
            'refs': fixture.git(fixture.alab, 'for-each-ref', '--format=%(refname) %(objectname)'),
            'reflogs': fixture.git(fixture.alab, 'reflog', 'show', '--all', '--format=%H %gD %gs'),
        }

    def directory_fault(self, error=None, *, prefix=None):
        events = []
        github = ledger_tests.VERSIONS.GITHUB

        class Directory(github._ScratchDirectory):
            def __init__(self, **kwargs):
                super().__init__(**kwargs)
                self.event = {'path': self.name, 'prefix': kwargs.get('prefix'), 'calls': 0}
                events.append(self.event)

            def cleanup(self):
                self.event['calls'] += 1
                super().cleanup()
                self.event['absent_after_real_cleanup'] = not Path(self.name).exists()
                if error is not None and (prefix is None or self.event['prefix'] == prefix):
                    raise error

        return patch.object(github, '_ScratchDirectory', Directory), events

    def assert_cleaned(self, events):
        self.assertTrue(events)
        for event in events:
            self.assertEqual(event['calls'], 1)
            self.assertTrue(event['absent_after_real_cleanup'])

    def test_success_status_exit_after_check_is_an_incomplete_cli_failure(self):
        codes = (0, None, False) + ((256, -256) if os.name == 'posix' else ())
        for code in codes:
            with self.subTest(code=code):
                fault = SystemExit(code)
                with patch.object(ledger_tests.VERSIONS, 'checked', side_effect=fault) as checked:
                    returned, escaped, stdout, stderr = self.invoke_main(self.check_arguments())
                self.assertIsNone(escaped)
                self.assertEqual(returned, 1)
                self.assertEqual(stdout, '')
                self.assertIn('incomplete', stderr)
                checked.assert_called_once()

    def test_nonzero_exit_and_keyboard_interrupt_keep_exact_identity(self):
        for fault in (SystemExit(47), SystemExit(0.0), SystemExit('0'),
                      SystemExit(1 << 128), KeyboardInterrupt('owned check cancellation')):
            with self.subTest(kind=type(fault).__name__, code=getattr(fault, 'code', None)):
                with patch.object(ledger_tests.VERSIONS, 'checked', side_effect=fault):
                    returned, escaped, stdout, stderr = self.invoke_main(self.check_arguments())
                self.assertIs(escaped, fault)
                self.assertIsNone(returned)
                self.assertEqual((stdout, stderr), ('', ''))

    def test_normal_check_and_help_keep_success_contract(self):
        rows = [{'version': '0.1.0', 'baseline': '0.164.0'}]
        with patch.object(ledger_tests.VERSIONS, 'checked', return_value=rows):
            returned, escaped, stdout, stderr = self.invoke_main(self.check_arguments())
        self.assertEqual((returned, escaped, stdout, stderr),
                         (0, None, '0.1.0+codex.0.164.0\n', ''))
        with patch.object(ledger_tests.VERSIONS, 'checked') as checked:
            returned, escaped, stdout, stderr = self.invoke_main(['--help'])
        self.assertIsNone(returned)
        self.assertIsInstance(escaped, SystemExit)
        self.assertEqual(escaped.code, 0)
        self.assertIn('usage:', stdout)
        self.assertEqual(stderr, '')
        checked.assert_not_called()

    def test_handled_error_without_notes_keeps_exact_header(self):
        errors = (ledger_tests.VERSIONS.Refused('owned refusal'),
                  OSError(errno.EIO, 'owned read failure'),
                  UnicodeDecodeError('utf-8', b'\xff', 0, 1, 'owned invalid input'))
        for error in errors:
            with self.subTest(kind=type(error).__name__):
                with patch.object(ledger_tests.VERSIONS, 'checked', side_effect=error):
                    returned, escaped, stdout, stderr = self.invoke_main(self.check_arguments())
                self.assertEqual((returned, escaped, stdout), (1, None, ''))
                self.assertEqual(stderr, 'alab: versions: ' + str(error) + '\n')

    def test_main_reports_natural_release_cleanup_note(self):
        fixture, arguments = self.release_fixture(malformed=True)
        before = self.source_snapshot(fixture)
        versions = ledger_tests.VERSIONS
        actual_released = versions.released
        captured = []

        def capture_released(*args, **kwargs):
            try:
                return actual_released(*args, **kwargs)
            except BaseException as error:
                captured.append(error)
                raise

        cleanup = OSError(errno.EIO, 'owned release cleanup diagnostic')
        directory, events = self.directory_fault(cleanup)
        with directory, patch.object(versions, 'released', side_effect=capture_released):
            returned, escaped, stdout, stderr = self.invoke_main(arguments)
        self.assertEqual((returned, escaped, stdout), (1, None, ''))
        self.assertEqual(len(captured), 1)
        self.assertIsInstance(captured[0], versions.Refused)
        note = 'temporary directory cleanup failed: ' + repr(cleanup)
        self.assertIn(note, captured[0].__notes__)
        self.assertIn(str(captured[0]), stderr)
        self.assertIn(note, stderr)
        self.assert_cleaned(events)
        self.assertEqual(self.source_snapshot(fixture), before)

    def test_release_transport_wrappers_retain_natural_cleanup_notes(self):
        fixture, arguments = self.release_fixture()
        versions = ledger_tests.VERSIONS
        github = versions.GITHUB
        before = self.source_snapshot(fixture)
        source = fixture.git(fixture.alab, 'rev-parse', 'HEAD')
        for stage in ('list_refs', 'fetch'):
            with self.subTest(stage=stage):
                primary = github.Refused('owned ' + stage + ' transport refusal')
                cleanup = OSError(errno.EIO, 'owned nested ' + stage + ' cleanup diagnostic')
                client = object.__new__(github.Agh)
                captured = []

                def transport_failure(*_args, **_kwargs):
                    try:
                        return client.fetch_batch('owned/fixture', fixture.alab, [
                            {'source': 'refs/heads/main', 'into': 'refs/agh/owned',
                             'expected_oid': source},
                        ])
                    except BaseException as error:
                        captured.append(error)
                        raise

                directory, events = self.directory_fault(cleanup, prefix='codex-agh-fetch-')
                with directory, patch.object(client, 'call', side_effect=primary), \
                        patch.object(github, stage, side_effect=transport_failure):
                    returned, escaped, stdout, stderr = self.invoke_main(arguments)
                self.assertEqual((returned, escaped, stdout), (1, None, ''))
                self.assertEqual(captured, [primary])
                note = 'temporary directory cleanup failed: ' + repr(cleanup)
                self.assertIn(note, primary.__notes__)
                self.assertIn(str(primary), stderr)
                self.assertIn(note, stderr)
                self.assert_cleaned(events)
                self.assertEqual(self.source_snapshot(fixture), before)

    def test_released_api_keeps_zero_and_none_cancellation_after_cleanup(self):
        fixture, arguments = self.release_fixture()
        versions = ledger_tests.VERSIONS
        for code in (0, None):
            with self.subTest(code=code):
                fault = SystemExit(code)
                directory, events = self.directory_fault()
                with directory, patch.object(versions.GITHUB, 'list_refs', side_effect=fault):
                    with self.assertRaises(BaseException) as caught:
                        versions.released(fixture.alab, arguments[-1], arguments[4])
                self.assertIs(caught.exception, fault)
                self.assert_cleaned(events)

    def test_real_release_cancellation_is_an_incomplete_cli_failure(self):
        fixture, arguments = self.release_fixture()
        before = self.source_snapshot(fixture)
        for code in (0, None):
            with self.subTest(code=code):
                fault = SystemExit(code)
                directory, events = self.directory_fault()
                with directory, patch.object(ledger_tests.VERSIONS.GITHUB, 'list_refs', side_effect=fault):
                    returned, escaped, stdout, stderr = self.invoke_main(arguments)
                self.assertIsNone(escaped)
                self.assertEqual(returned, 1)
                self.assertEqual(stdout, '')
                self.assertIn('incomplete', stderr)
                self.assert_cleaned(events)
                self.assertEqual(self.source_snapshot(fixture), before)


if __name__ == '__main__':
    unittest.main()
