# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import exec_accounts
import exec_named_simulated as simulated


class SimulatedExecContract(unittest.TestCase):
    def setUp(self):
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        model = exec_accounts.Model(root, limit=simulated.LIMIT)
        self.move = simulated.SimulatedMove(root, model)
        self.thread = "00000000-0000-0000-0000-000000000001"
        self.turn = "00000000-0000-0000-0000-000000000002"
        self.sid = "t0000000001"
        foreground = {"input": [], "tools": [{"type": "function", "name": "update_plan"}]}
        model.reply(foreground, "old")
        model.reply(foreground, "old")[-1]["requestIdentity"] = {"thread_id": self.thread, "turn_id": self.turn}
        self.origin = f"thread={self.thread} turn={self.turn} source_account={self.move.source['account']} source_user={self.move.source['user']}"
        self.target = f"target_account={self.move.target['account']} target_user={self.move.target['user']}"
        self.limit = "limit=" + simulated.LIMIT

    def ask(self, request):
        return self.move.answer(f"@{self.sid} codex-exec-handoff {request}", self.sid)

    def test_a_new_scope_or_ordinary_arm_cannot_consume_the_scripted_move(self):
        with self.assertRaises(RuntimeError):
            self.ask("prepare " + self.origin + " limit=another")
        self.assertIn(self.target, self.ask(f"prepare {self.origin} {self.limit}"))
        checkpoint = {"thread": self.thread, "turn": self.turn}
        def arm():
            payload = json.dumps(checkpoint).encode().hex()
            return f"arm {self.origin} {self.target} checkpoint={payload} {self.limit}"
        with self.assertRaises(RuntimeError):
            self.ask(arm())
        checkpoint["limit"] = simulated.LIMIT
        self.assertIn("state=armed ", self.ask(arm()))
        with self.assertRaises(RuntimeError):
            self.ask("reserve id=wrong cap=wrong")
        self.assertEqual(self.move.steps, ["prepare", "arm"])

    def test_ordinary_week_has_numeric_windows_and_accepts_only_the_unextended_wire(self):
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        model = simulated.OrdinaryWindowsModel(root)
        self.move = simulated.SimulatedMove(root, model)
        body = {"input": [], "tools": [{"type": "function", "name": "update_plan"}]}
        for generation, status, week in (("old", 200, "0"), ("old", 429, "100"), ("new", 200, "0")):
            result = model.reply(body, generation)
            self.assertEqual(result[0], status)
            headers = result[-1]["responseHeaders"]
            self.assertEqual((headers["x-codex-primary-used-percent"], headers["x-codex-secondary-used-percent"]),
                             ("0", week))
            if status == 429:
                result[-1]["requestIdentity"] = {"thread_id": self.thread, "turn_id": self.turn}
                self.assertEqual(headers["x-codex-active-limit"], "codex")
        with self.assertRaises(RuntimeError):
            self.ask(f"prepare {self.origin} limit=codex")
        self.ask("prepare " + self.origin)
        checkpoint = {"thread": self.thread, "turn": self.turn}
        def arm(record):
            return self.ask(f"arm {self.origin} {self.target} checkpoint={json.dumps(record).encode().hex()}")
        with self.assertRaises(RuntimeError):
            arm({**checkpoint, "limit": None})
        self.assertIn("state=armed ", arm(checkpoint))
