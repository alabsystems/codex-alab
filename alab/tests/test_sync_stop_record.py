# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Only absent or successfully parsed stop records can authorize resumption."""

from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / 'alab/sync.sh').read_text()
BEGIN = SOURCE.index('\nstop_code() {') + 1
STOP_CODE = SOURCE[BEGIN:SOURCE.index('\n}\n', BEGIN) + 3]


class StopRecordTests(unittest.TestCase):
    def shell(self, payload, startup=False, dangling=False):
        with tempfile.TemporaryDirectory(prefix='sync-stop-record-') as temporary:
            path = Path(temporary) / 'stop'
            if dangling:
                path.symlink_to('missing-target')
            elif payload is not None:
                path.write_bytes(payload)
            definitions = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / 'alab/lib.sh')) + '\n'
                           + STOP_CODE + '\nstate_path() { printf %s ' + shlex.quote(str(path)) + '; }\n')
            if startup:
                begin = SOURCE.index('\nensure_worktree\n', SOURCE.index('# mode=start'))
                end = SOURCE.index('\nfi\n', begin) + 3
                definitions += '''
wt=candidate
waiting_hours() { echo 0; }
ensure_worktree() { :; }
merging() { return 0; }
exit_with() { exit "$1"; }
sync_target() { echo target; }
alab_say() { :; }
finish() { echo resumed; }
push=1
'''
                body = SOURCE[begin:end]
            else:
                body = 'code="$(stop_code)" || exit 1\nprintf "%s" "$code"'
            return subprocess.run(['/bin/bash', '-c', definitions + body], text=True,
                                  capture_output=True, timeout=10)

    def test_written_stop_codes_and_absence_are_preserved(self):
        for code in ('3', '4', '6', 'no-push', 'no-push-unchecked', 'continue'):
            with self.subTest(code=code):
                result = self.shell((code + '\nreason\n').encode())
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, code)
        absent = self.shell(None)
        self.assertEqual(absent.returncode, 0, absent.stderr)
        self.assertEqual(absent.stdout, '')

    def test_invalid_and_dangling_records_are_not_absent(self):
        for payload in (b'', b'\n', b'unknown\n', b'3\0\n'):
            with self.subTest(payload=payload):
                result = self.shell(payload)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(result.stdout, '')
        self.assertEqual(self.shell(None, dangling=True).returncode, 1)

    def test_startup_respects_known_stops_and_resumes_only_when_absent(self):
        for code in ('3', '4', '6', 'no-push', 'no-push-unchecked', 'continue'):
            with self.subTest(code=code):
                result = self.shell((code + '\nreason\n').encode(), startup=True)
                self.assertEqual(result.returncode, 5, result.stderr)
                self.assertNotIn('resumed', result.stdout)
        absent = self.shell(None, startup=True)
        self.assertEqual(absent.returncode, 0, absent.stderr)
        self.assertEqual(absent.stdout, 'resumed\n')

    def test_startup_does_not_resume_with_an_unreadable_or_invalid_stop(self):
        for payload, dangling in ((b'unknown\n', False), (b'', False), (None, True)):
            with self.subTest(payload=payload, dangling=dangling):
                result = self.shell(payload, startup=True, dangling=dangling)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn('cannot read sync stop record', result.stderr)
                self.assertNotIn('resumed', result.stdout)


if __name__ == '__main__':
    unittest.main()
