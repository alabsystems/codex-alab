# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise candidate dispatcher inputs in the package fixture's native profile."""
import os
from pathlib import Path
import sys
import unittest

# All code imported on the host comes from the reviewed collector. The fixture
# invokes the frozen candidate's certified dispatcher only in its narrow profile.
controller = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(controller / 'alab/tests'))
from test_proof_build import CertifiedDispatchTests


class NativePackageInputTests(CertifiedDispatchTests):
    package_inputs = Path(os.environ['ALAB_FEATURE_WORKSPACE'])

    def setUp(self):
        for name in ('package.sh', 'lib.sh'):
            local = controller / 'alab' / name
            subject = self.package_inputs / 'alab' / name
            if subject.resolve(strict=True) != subject or local.read_bytes() != subject.read_bytes():
                self.fail('candidate package controller requires independent review: ' + name)
        super().setUp()


def load_tests(loader, tests, pattern):
    return unittest.TestSuite([NativePackageInputTests(
        'test_package_controller_inputs_are_readable_but_readonly_outside_target')])
