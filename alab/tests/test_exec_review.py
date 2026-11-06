#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Review qualification must retain both delegate identity and structured output."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import exec_accounts as accounts
import exec_review as review


class ReviewContinuityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "codex-home").mkdir()
        self.identity = {"thread_id": "child", "turn_id": "original",
                         "parent_thread_id": "root"}
        self.requests = [{"requestIdentity": dict(self.identity), "oldTokenMatched": True},
                         {"requestIdentity": dict(self.identity), "oldTokenMatched": True},
                         {"requestIdentity": dict(self.identity), "newTokenMatched": True}]
        self.events = [{"type": "item_completed", "turn_id": "parent-turn", "item": {"type": "ExitedReviewMode", "review_output": review.OUTPUT}}]

    def history(self):
        rows = [{"type": "session_meta", "payload": {"id": "root"}}]
        rows += [{"type": "event_msg", "payload": event} for event in [
            {"type": "item_completed", "turn_id": "parent-turn", "item": {"type": "EnteredReviewMode"}},
            {"type": "task_started", "turn_id": "delegate-turn"}, *self.events,
            {"type": "task_complete", "turn_id": "parent-turn"}]]
        (self.root / "codex-home/rollout-root.jsonl").write_text(
            "".join(json.dumps(row) + "\n" for row in rows))

    def verify(self):
        self.history()
        report = {}
        review.Review(accounts.find_tool).verify(self.root, self.requests, report, "root")
        return report

    def test_original_review_and_delegate_complete(self):
        self.assertTrue(self.verify()["reviewContinuity"]["structuredResult"])

    def test_successful_replacement_delegate_cannot_hide_identity_loss(self):
        for key in self.identity:
            with self.subTest(key=key):
                changed = copy.deepcopy(self.requests)
                changed[-1]["requestIdentity"][key] = "replacement"
                self.history()
                with self.assertRaisesRegex(RuntimeError, "different delegate or turn"):
                    review.Review(accounts.find_tool).verify(self.root, changed, {}, "root")

    def test_missing_or_duplicate_structured_result_fails(self):
        for events in ([], self.events * 2):
            with self.subTest(events=events):
                self.events = events
                with self.assertRaisesRegex(RuntimeError, "structured result exactly once"):
                    self.verify()

    def test_interrupted_review_cannot_hide_behind_successful_output(self):
        self.events.append({"type": "turn_aborted", "turn_id": "original"})
        with self.assertRaisesRegex(RuntimeError, "review was aborted"):
            self.verify()

    def test_source_account_cannot_claim_successful_handoff(self):
        self.requests[-1] = {"requestIdentity": self.identity, "oldTokenMatched": True}
        with self.assertRaisesRegex(RuntimeError, "admitted account"):
            self.verify()

    def test_review_result_from_another_parent_turn_fails(self):
        self.history()
        path = self.root / "codex-home/rollout-root.jsonl"
        rows = [json.loads(line) for line in path.read_text().splitlines()]
        rows[-1]["payload"]["turn_id"] = "replacement-parent-turn"
        path.write_text("".join(json.dumps(row) + "\n" for row in rows))
        with self.assertRaisesRegex(RuntimeError, "different parent turn"):
            review.Review(accounts.find_tool).verify(self.root, self.requests, {}, "root")

    def test_forged_root_binding_fails_even_when_all_delegate_requests_agree(self):
        for row in self.requests:
            row["requestIdentity"]["parent_thread_id"] = "another-root"
        with self.assertRaisesRegex(RuntimeError, "owning root"):
            self.verify()

    def ephemeral_events(self):
        return [
            {"type": "thread.started", "thread_id": "root"},
            {"type": "turn.started"},
            {"type": "item.completed", "item": {"type": "command_execution",
             "status": "completed", "exit_code": 0, "aggregated_output": review.MARKER + "\n"}},
            {"type": "item.completed", "item": {"type": "agent_message", "text": review.ANSWER}},
            {"type": "turn.completed"},
        ]

    def verify_ephemeral(self, events=None, turn="parent-turn"):
        report = {}
        review.Review(accounts.find_tool).verify(
            self.root, self.requests, report, "root", turn, ephemeral=True,
            events=self.ephemeral_events() if events is None else events)
        return report["reviewContinuity"]

    def test_ephemeral_review_uses_rendered_evidence_without_claiming_typed_history(self):
        result = self.verify_ephemeral()
        self.assertTrue(result["renderedResult"])
        self.assertTrue(result["noRollout"])
        self.assertNotIn("structuredResult", result)
        self.assertEqual(result["parentTurn"], "parent-turn")

    def test_ephemeral_review_rejects_missing_native_turn_binding(self):
        with self.assertRaisesRegex(RuntimeError, "native parent turn binding"):
            self.verify_ephemeral(turn=None)

    def test_ephemeral_review_rejects_hidden_persistence(self):
        self.history()
        with self.assertRaisesRegex(RuntimeError, "persisted a session rollout"):
            self.verify_ephemeral()

    def test_ephemeral_review_rejects_missing_duplicate_or_unrendered_answer(self):
        for change in ("missing", "duplicate", "raw-json"):
            with self.subTest(change=change):
                events = self.ephemeral_events()
                if change == "missing":
                    del events[3]
                elif change == "duplicate":
                    events.insert(3, copy.deepcopy(events[3]))
                else:
                    events[3]["item"]["text"] = json.dumps(review.OUTPUT)
                with self.assertRaisesRegex(RuntimeError, "render its result exactly once"):
                    self.verify_ephemeral(events)

    def test_ephemeral_review_rejects_replayed_or_failed_lifecycle(self):
        for event in ({"type": "turn.started"}, {"type": "turn.completed"}, {"type": "turn.failed"}):
            with self.subTest(event=event):
                with self.assertRaisesRegex(RuntimeError, "incomplete or replayed"):
                    self.verify_ephemeral(self.ephemeral_events() + [event])

    def test_ephemeral_review_rejects_wrong_root_or_lost_command(self):
        events = self.ephemeral_events()
        events[0]["thread_id"] = "another-root"
        with self.assertRaisesRegex(RuntimeError, "owning root"):
            self.verify_ephemeral(events)
        for change in ("missing", "duplicate", "failed", "output"):
            with self.subTest(change=change):
                events = self.ephemeral_events()
                if change == "missing":
                    del events[2]
                elif change == "duplicate":
                    events.insert(2, copy.deepcopy(events[2]))
                elif change == "failed":
                    events[2]["item"]["exit_code"] = 1
                else:
                    events[2]["item"]["aggregated_output"] = ""
                with self.assertRaisesRegex(RuntimeError, "actual command output"):
                    self.verify_ephemeral(events)

    def test_quota_occurs_only_after_actual_command_output(self):
        model = accounts.Model(self.root, review="quota")
        body = {"tools": [{"type": "function", "name": "exec_command"}],
                "input": [{"role": "user", "content": [{"type": "input_text",
                           "text": accounts.probe.PROMPT}]}]}
        status, data, _, _, _ = model.reply(body, "old")
        self.assertEqual(status, 200)
        self.assertIn(review.MARKER.encode(), data)
        with self.assertRaisesRegex(RuntimeError, "actual pre-quota command output"):
            model.reply(body, "old")
        body["input"].append({"type": "function_call_output", "call_id": accounts.probe.CALL_ID,
                              "output": review.MARKER})
        self.assertEqual(model.reply(body, "old")[0], 429)
        status, data, _, _, _ = model.reply(body, "new")
        self.assertEqual(status, 200)
        self.assertIn(review.ANSWER.encode(), data)


if __name__ == "__main__":
    unittest.main()
