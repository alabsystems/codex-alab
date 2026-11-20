# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""A broken output is retired even when its null-device replacement fails."""

import errno
import unittest

import test_qualification_publication as publication


class QualificationNullOutputTests(unittest.TestCase):
    def setUp(self):
        self.publication = publication.QualificationPublicationTests()
        self.publication.setUp()
        self.addCleanup(self.publication.doCleanups)

    def assert_null_allocation_failed(self, result, descriptor):
        self.assertEqual(result.witness["broken_output_descriptor"], descriptor)
        self.assertEqual(result.witness["null_open_errors"], [errno.EMFILE])

    def test_null_allocation_failure_preserves_actual_gate_status(self):
        for phase, descriptor in (("broken-stdout-null", 1), ("broken-stderr-null", 2)):
            for gate in (0, 23, 75, "signal"):
                with self.subTest(phase=phase, gate=gate):
                    result = self.publication.run_cli(phase, gate)
                    self.assert_null_allocation_failed(result, descriptor)
                    self.assertEqual(
                        (result.done.returncode, result.witness["failed_standard_stream_closed"]),
                        (result.expected_exit or 1, True), result.done.stderr)
                    self.assertNotIn("Exception ignored while flushing", result.done.stderr)
                    if descriptor == 2:
                        self.publication.assert_fault(result, "hash")

    def test_null_allocation_failure_preserves_pending_cancellation(self):
        for gate in (0, 23):
            for kind in ("exit47", "keyboard"):
                with self.subTest(gate=gate, kind=kind):
                    result = self.publication.run_cli("broken-stdout-cancel-null", gate, kind)
                    self.publication.assert_fault(result, "broken-stdout-cancel-null")
                    self.assert_null_allocation_failed(result, 1)
                    self.assertTrue(result.witness["escaped_failure_identity"])
                    self.assertEqual(
                        (result.done.returncode, result.witness["failed_standard_stream_closed"]),
                        (47 if kind == "exit47" else -2, True), result.done.stderr)
                    self.assertNotIn("Exception ignored while flushing", result.done.stderr)
