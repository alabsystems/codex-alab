# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Public matrix JSON reads stay bounded and stable; no native actor is launched."""
from contextlib import contextmanager
import hashlib
import json
import os
import unittest
from unittest.mock import patch

import test_renewal_matrix_digest as digest_tests
import test_renewal_matrix_results as result_tests


MATRIX = result_tests.MATRIX
PARTIAL = digest_tests.PARTIAL


def split_records():
    """Both complete records fail; their first/remaining chunks combine to pass."""
    padding = b'p' * (PARTIAL * 3)
    first = (b'{"passed":true ,"padding":"' + padding
             + b'","rawHostReturncode":0,"guardian":{"returncode":23,"timed_out":false}}\n')
    second = (b'{"passed":false,"padding":"' + padding
              + b'","rawHostReturncode":0,"guardian":{"returncode":0 ,"timed_out":false}}\n')
    return first, second


class RenewalMatrixResultSnapshotTests(unittest.TestCase):
    # Compose only fixture helpers; no imported/inherited TestCase adds test IDs.
    setUp = result_tests.RenewalMatrixResultTests.setUp
    run_matrix = result_tests.RenewalMatrixResultTests.run_matrix
    assert_case = result_tests.RenewalMatrixResultTests.assert_case

    @contextmanager
    def reading_result(self, label, after_read):
        path = self.root / label / 'matrix/weekly-quota/result.json'
        # The existing real partial-reader helper must instrument this matrix's
        # module-local FILE_HASH namespace, rather than its separately loaded one.
        with patch.object(digest_tests, 'MATRIX', MATRIX), \
                digest_tests.RenewalMatrixDigestTests.partial_reads(self, path, after_read) as probe:
            yield path, probe

    @staticmethod
    def diagnostic(run, received):
        cases = run.manifest['cases']
        digest = cases[0]['resultSha256'] if cases else None
        return (f'actual_status={run.status}; matrix_passed={run.manifest["passed"]}; '
                f'case_count={len(cases)}; result_digest={digest}; received={received}')

    def test_stable_partial_read_qualifies_exact_observed_bytes(self):
        first, second = split_records()
        raw = first[:PARTIAL] + second[PARTIAL:]
        self.assertTrue(MATRIX.case_passed('weekly-quota', 0, json.loads(raw)))
        chunks = []
        label = 'stable-partial'
        with self.reading_result(label, lambda _index, data: chunks.append(data)) as (path, reader):
            run = self.run_matrix(label, raw, result_open=reader.path.open)
        self.assertGreater(len(chunks), 1, 'the real partial reader was not exercised')
        self.assertEqual(b''.join(chunks), raw)
        self.assertEqual(path.read_bytes(), raw)
        self.assert_case(run, True, raw)

    def test_same_size_in_read_mix_cannot_qualify_two_failing_records(self):
        first, second = split_records()
        self.assertEqual(len(first), len(second))
        self.assertFalse(MATRIX.case_passed('weekly-quota', 0, json.loads(first)))
        self.assertFalse(MATRIX.case_passed('weekly-quota', 0, json.loads(second)))
        mixed = first[:PARTIAL] + second[PARTIAL:]
        self.assertTrue(MATRIX.case_passed('weekly-quota', 0, json.loads(mixed)))
        self.assertNotIn(hashlib.sha256(mixed).digest(),
                         (hashlib.sha256(first).digest(), hashlib.sha256(second).digest()))
        chunks, rewrites = [], []
        label = 'mixed-failing-records'

        def rewrite(index, data):
            chunks.append(data)
            if index == 1:
                self.assertEqual(data, first[:PARTIAL])
                metadata = path.stat()
                path.write_bytes(second)
                # Change real kernel metadata deterministically, never fstat output.
                os.utime(path, ns=(metadata.st_atime_ns, metadata.st_mtime_ns + 1000000000))
                rewrites.append(True)

        with self.reading_result(label, rewrite) as (path, reader):
            run = self.run_matrix(label, first, result_open=reader.path.open)
        observed = b''.join(chunks)
        self.assertEqual(rewrites, [True])
        self.assertGreater(len(chunks), 1)
        self.assertEqual(observed, mixed)
        self.assertEqual(path.read_bytes(), second)
        diagnostic = self.diagnostic(run, len(observed))
        self.assertEqual(run.status, 1, diagnostic)
        self.assertIs(run.manifest['passed'], False, diagnostic)
        self.assertEqual(run.manifest['cases'], [], diagnostic)
        self.assertIn('file changed while reading', run.manifest['error'])

    def test_finite_growth_stops_after_initial_size_plus_one(self):
        first, second = split_records()
        original = first[:PARTIAL] + second[PARTIAL:]
        self.assertTrue(MATRIX.case_passed('weekly-quota', 0, json.loads(original)))
        appends = []
        label = 'finite-valid-json-growth'

        def grow(index, _data):
            # Trailing spaces retain valid JSON. The old whole-file reader also
            # terminates: this fixture makes at most 32 finite real append writes.
            if index <= 32:
                with path.open('ab') as stream:
                    stream.write(b' ' * PARTIAL)
                appends.append(index)

        with self.reading_result(label, grow) as (path, reader):
            run = self.run_matrix(label, original, result_open=reader.path.open)
        received = sum(read['bytes'] for read in reader.reads)
        diagnostic = self.diagnostic(run, received)
        self.assertGreater(received, 0)
        self.assertGreater(len(appends), 0)
        self.assertLessEqual(len(appends), 32)
        self.assertEqual(path.stat().st_size, len(original) + len(appends) * PARTIAL)
        self.assertLessEqual(received, len(original) + 1, diagnostic)
        self.assertEqual(run.status, 1, diagnostic)
        self.assertIs(run.manifest['passed'], False, diagnostic)
        self.assertEqual(run.manifest['cases'], [], diagnostic)
        self.assertIn('file changed while reading', run.manifest['error'])


if __name__ == '__main__':
    unittest.main()
