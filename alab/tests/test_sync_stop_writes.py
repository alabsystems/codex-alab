# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Stop updates must publish a complete decision or preserve the previous one."""

from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / 'alab/sync.sh').read_text()


def function(name):
    begin = SOURCE.index('\n' + name + '() {') + 1
    return SOURCE[begin:SOURCE.index('\n}\n', begin) + 3]


class StopWriteTests(unittest.TestCase):
    def shell(self, failure='', directory=False):
        with tempfile.TemporaryDirectory(prefix='sync-stop-write-') as temporary:
            root = Path(temporary)
            record = root / 'stop'
            if directory:
                record.mkdir()
            else:
                record.write_text('continue\nprevious decision\n')
            definitions = function('stop')
            if '\nwrite_stop() {' in SOURCE:
                definitions += '\n' + function('write_stop')
            script = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / 'alab/lib.sh')) + '\n'
                      + definitions + '\nstop_record_path=' + shlex.quote(str(record)) + '\n'
                      + 'failure=' + shlex.quote(failure) + '\n' + r'''
state_path() { [[ "$failure" != path ]] || return 128; echo "$stop_record_path"; }
printf() {
  if [[ "$failure" == write && "$1" != 'alab: '* ]]; then command printf 'partial'; return 128; fi
  command printf "$@"
}
mv() { [[ "$failure" != replace ]] || return 128; command mv "$@"; }
stop 3 'resolver decision'
''')
            result = subprocess.run(['/bin/bash', '-c', script], text=True,
                                    capture_output=True, timeout=10)
            self.assertEqual(sorted(path.name for path in root.iterdir()), ['stop'])
            if directory:
                self.assertEqual(list(record.iterdir()), [])
            return result, record.read_text() if not directory else None

    def test_failed_updates_preserve_previous_stop_and_report_failure(self):
        for failure in ('path', 'write', 'replace'):
            with self.subTest(failure=failure):
                result, content = self.shell(failure)
                self.assertEqual(content, 'continue\nprevious decision\n')
                self.assertEqual(result.returncode, 1, result.stderr)
                reason = {'path': 'path read failed', 'write': 'incomplete write',
                          'replace': 'replacement failed'}[failure]
                self.assertIn('cannot write sync stop record: ' + reason, result.stderr)

    def test_directory_destination_cannot_absorb_a_staged_record(self):
        result, _ = self.shell(directory=True)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('destination is a directory', result.stderr)

    def test_successful_update_records_complete_decision_and_keeps_stop_code(self):
        result, content = self.shell()
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertEqual(content, '3\nresolver decision\n')


if __name__ == '__main__':
    unittest.main()
