# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
from idle_account_renewal import check_writes


class IdleCredentialWrites(unittest.TestCase):
    def test_rejects_early_write_destination_write_and_in_place_source_write(self):
        before = {"source": {"inode": 1, "sha256": "old"}, "destination": {"inode": 2, "sha256": "other"}}
        after = {"source": {"inode": 3, "sha256": "renewed"}, "destination": before["destination"].copy()}
        check_writes(before, before, after)
        for when, owner, field, value in (("held", "source", "sha256", "renewed"),
                                          ("held", "destination", "inode", 4),
                                          ("after", "source", "inode", 1),
                                          ("after", "source", "sha256", "old"),
                                          ("after", "destination", "sha256", "renewed")):
            with self.subTest(when=when, owner=owner, field=field):
                states = {"held": copy.deepcopy(before), "after": copy.deepcopy(after)}
                states[when][owner][field] = value
                with self.assertRaises(RuntimeError):
                    check_writes(before, states["held"], states["after"])
