# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import renewal_follow as renewal


class ReceiptLedgerTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.directory = self.root / "host-state/accounts-codex/renewal-follow"
        self.directory.mkdir(parents=True)
        self.ledgers = self.root / "host-state/upgrade"
        self.ledgers.mkdir()
        self.original, self.child = "ab" * 16, "cd" * 16
        self.receipt = {"id": self.original, "phase": "consumed",
            "intent": {"thread": "thread", "wall": "wall"},
            "native_prepare": "prepare id=" + self.original, "current_attempt": None}

    def write(self, path, value):
        path.write_text(json.dumps(value))

    def ledger(self, prepare):
        return {"native_completed_prepare": prepare, "native_coordination": True,
            "native_rollback": False, "native_observation": "completed observation"}

    def consumed(self):
        self.write(self.directory / (self.original + ".json"), self.receipt)
        return renewal.consumed_receipt(self.root, "thread", "wall")

    def continuation(self):
        reference = {"id": self.child, "generation": 1}
        self.receipt["current_attempt"] = reference
        self.attempt = {"root": self.original, "reference": reference,
            "parent": {"id": self.original, "generation": 0},
            "native_predecessor": self.original, "native_prepare": "prepare id=" + self.child}
        directory = self.directory / (self.original + ".attempts")
        directory.mkdir()
        self.attempt_path = directory / (self.child + ".json")
        self.write(self.attempt_path, self.attempt)

    def test_original_operation_uses_its_own_completion_ledger(self):
        self.write(self.ledgers / ("renewal-" + self.original + ".json"),
            self.ledger(self.receipt["native_prepare"]))
        self.assertEqual(self.consumed()["nativeOperationId"], self.original)

    def test_child_requires_its_own_exact_completed_native_ledger(self):
        self.continuation()
        ledger = self.ledger(self.attempt["native_prepare"])
        # Even a matching plan in the old root's ledger cannot stand for its child.
        self.write(self.ledgers / ("renewal-" + self.original + ".json"), ledger)
        self.assertIsNone(self.consumed())
        child_path = self.ledgers / ("renewal-" + self.child + ".json")
        self.write(child_path, ledger)
        result = self.consumed()
        self.assertEqual(result["id"], self.original)
        self.assertEqual(result["nativeOperationId"], self.child)
        self.assertEqual(result["nativeAttempt"], self.attempt["reference"])
        for key, value in (("native_completed_prepare", self.receipt["native_prepare"]),
                           ("native_coordination", False), ("native_rollback", True),
                           ("native_observation", "")):
            with self.subTest(key=key):
                changed = dict(ledger, **{key: value})
                self.write(child_path, changed)
                self.assertIsNone(self.consumed())

    def test_an_unbound_child_journal_cannot_select_a_completion(self):
        self.continuation()
        for key, value in (("root", self.child),
                           ("parent", {"id": self.child, "generation": 0}),
                           ("native_predecessor", self.child),
                           ("native_prepare", "prepare id=" + self.original)):
            with self.subTest(key=key):
                changed = copy.deepcopy(self.attempt)
                changed[key] = value
                self.write(self.attempt_path, changed)
                with self.assertRaises(RuntimeError):
                    self.consumed()
