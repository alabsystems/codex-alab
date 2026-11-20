# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Run the candidate's dispatch and observation fixtures inside allocated scratch."""
import os
from pathlib import Path
import sys
import unittest

workspace = Path(os.environ['ALAB_FEATURE_WORKSPACE'])
sys.path.insert(0, str(workspace / 'alab/tests'))
import test_proof_build as candidate


class CertifiedDispatchTests(candidate.CertifiedDispatchTests):
    def setUp(self):
        super().setUp()
        # Older fixtures build a fresh env without TMPDIR for pristine Git status.
        self.env.setdefault('TMPDIR', os.environ['ALAB_TEST_SCRATCH'])


def load_tests(loader, tests, pattern):
    # This kernel control must apply its own narrow profile from the trusted host.
    kernel = 'test_package_controller_inputs_are_readable_but_readonly_outside_target'
    dispatch = (CertifiedDispatchTests(name) for name in loader.getTestCaseNames(CertifiedDispatchTests)
                if name != kernel)
    return unittest.TestSuite([unittest.TestSuite(dispatch),
                               loader.loadTestsFromTestCase(candidate.ProofObservationTests)])
