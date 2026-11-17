# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Pending sync predicates must reject unavailable Git state in boolean contexts."""

from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / "alab/sync.sh").read_text()


def function(name):
    begin = SOURCE.index('\n' + name + '() {') + 1
    line_end = SOURCE.index('\n', begin)
    if SOURCE[begin:line_end].endswith('}'):  # Also exercise the original one-line predicates.
        return SOURCE[begin:line_end]
    return SOURCE[begin:SOURCE.index('\n}\n', begin) + 3]


class PendingReadTests(unittest.TestCase):
    def shell(self, predicate, status, output, context):
        script = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / 'alab/lib.sh')) + '\n'
                  + function(predicate) + '\n'
                  + 'in_wt() { printf %s ' + shlex.quote(output) + '; return ' + str(status) + '; }\n'
                  + context.format(predicate=predicate))
        return subprocess.run(['/bin/bash', '-c', script], text=True, capture_output=True, timeout=10)

    def test_failed_pending_reads_do_not_allow_idle_or_pending_actions(self):
        for predicate in ('merging', 'unlanded'):
            for status in (2, 128, 137):
                for output in ('', 'partial-id\n'):
                    for context in ('if {predicate}; then echo pending; else echo idle; fi',
                                    '{predicate} || echo idle', 'if ! {predicate}; then echo idle; fi'):
                        with self.subTest(predicate=predicate, status=status, output=output, context=context):
                            result = self.shell(predicate, status, output, context)
                            self.assertEqual(result.returncode, 1, result.stderr)
                            self.assertEqual(result.stdout, '')
                            self.assertIn('cannot read', result.stderr)

    def test_real_repository_absent_merge_and_unreadable_history(self):
        with tempfile.TemporaryDirectory(prefix='sync-pending-read-') as temporary:
            subprocess.run(['git', 'init', '-q', temporary], check=True, capture_output=True)
            definitions = ('set -euo pipefail\n. ' + shlex.quote(str(ROOT / 'alab/lib.sh')) + '\n'
                           + function('merging') + '\n' + function('unlanded') + '\n'
                           + 'in_wt() { git -C ' + shlex.quote(temporary) + ' "$@"; }\n')
            absent = subprocess.run(['/bin/bash', '-c', definitions
                                     + 'if merging; then echo pending; else echo idle; fi'],
                                    text=True, capture_output=True, timeout=10)
            self.assertEqual(absent.returncode, 0, absent.stderr)
            self.assertEqual(absent.stdout, 'idle\n')
            unknown = subprocess.run(['/bin/bash', '-c', definitions
                                      + 'if unlanded; then echo pending; else echo idle; fi'],
                                     text=True, capture_output=True, timeout=10)
            self.assertEqual(unknown.returncode, 1, unknown.stderr)
            self.assertEqual(unknown.stdout, '')
            self.assertIn('cannot read unlanded sync commits', unknown.stderr)

    def test_known_merge_presence_and_absence(self):
        for status, expected in ((0, 'pending\n'), (1, 'idle\n')):
            result = self.shell('merging', status, '',
                                'if {predicate}; then echo pending; else echo idle; fi')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, expected)

    def test_known_commit_presence_and_absence(self):
        for output, expected in (('', 'idle\n'), ('commit-id\n', 'pending\n')):
            result = self.shell('unlanded', 0, output,
                                'if {predicate}; then echo pending; else echo idle; fi')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, expected)


if __name__ == '__main__':
    unittest.main()
