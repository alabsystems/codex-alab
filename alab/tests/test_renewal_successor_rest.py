# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import json
import runpy
from pathlib import Path
import sys
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import renewal_follow as renewal

FIXTURE = Path(__file__).parent / "fixtures/renewal-plan-custody"


class SuccessorRestTests(unittest.TestCase):
    def test_known_paced_wait_is_distinct_from_native_or_unknown_custody(self):
        # Shape emitted by extension::owed_act_at, preserving only inert timer semantics.
        record = {"sessionId": "thread", "status": "idle", "draftEmpty": True,
            "owed": {"thread": "thread", "rule": "continue@v1", "text": "Continue working.",
                "mode": "cruise", "notBefore": 1234, "resumeGoal": False,
                "landing": False, "landed": False}}
        observed = renewal.successor_rest(record, "thread")
        self.assertEqual(observed["kind"], "ordinary-paced")
        self.assertNotIn("Continue working.", json.dumps(observed))
        for key, value in [("renewal", {"accepted": True}), ("nativeOrigin", {"turnId": "wall"})]:
            changed = copy.deepcopy(record)
            changed["owed"][key] = value
            self.assertIsNone(renewal.successor_rest(changed, "thread"))
        for key, value in [("rule", "unknown@v1"), ("rule", "login-back@v1"),
                           ("thread", "other"), ("notBefore", True), ("text", ""),
                           ("landed", True), ("mode", "off"), ("extra", "secret")]:
            changed = copy.deepcopy(record)
            changed["owed"][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(RuntimeError):
                renewal.successor_rest(changed, "thread")
        for key, value in [("draftEmpty", False), ("status", "busy"), ("queuedInput", True),
                           ("pendingUserTurn", True), ("popupOpen", True), ("editorOpen", True)]:
            changed = copy.deepcopy(record)
            changed[key] = value
            self.assertIsNone(renewal.successor_rest(changed, "thread"))
        record["owed"] = None
        self.assertEqual(renewal.successor_rest(record, "thread")["kind"], "none")

    def test_terminal_proof_is_required_in_addition_to_retained_plan(self):
        events = json.loads((FIXTURE / "rollout.json").read_text())
        wall = events[0]["payload"]["turn_id"]
        terminal = {"type": "event_msg", "payload": {"type": "task_complete", "turn_id": wall}}
        events.insert(3, copy.deepcopy(terminal))
        events.append(copy.deepcopy(terminal))
        self.assertEqual(renewal.validate_completed_original_turn(events, wall), [wall, wall])
        for mutation in ("missing", "wrong", "abort", "duplicate", "early"):
            changed = copy.deepcopy(events)
            if mutation == "missing":
                changed.pop()
            elif mutation == "wrong":
                changed[-1]["payload"]["turn_id"] = "another"
            elif mutation == "abort":
                changed[-1]["payload"]["type"] = "turn_aborted"
            elif mutation == "duplicate":
                changed.append(copy.deepcopy(terminal))
            else:
                changed.insert(4, changed.pop())
            with self.subTest(mutation=mutation), self.assertRaises(RuntimeError):
                renewal.validate_completed_original_turn(changed, wall)

    def test_completed_plan_must_be_recorded_before_the_first_terminal_event(self):
        start, call, output, resume = json.loads((FIXTURE / "rollout.json").read_text())
        wall = start["payload"]["turn_id"]
        project = runpy.run_path(str(FIXTURE / "project.py"))["project"]
        completed = {"type": "event_msg", "payload": {"type": "task_complete", "turn_id": wall}}
        for kind in ("task_complete", "turn_complete", "turn_aborted"):
            end = {"type": "event_msg", "payload": {"type": kind, "turn_id": wall}}
            for prefix in ([start, call, end, output], [start, end, call, output]):
                rows = [*prefix, resume, completed]
                for evidence in (rows, project(rows)):
                    with self.subTest(kind=kind, prefix=prefix), self.assertRaisesRegex(RuntimeError, "first turn ended"):
                        renewal.validate_completed_original_turn(evidence, wall)

    def test_boundary_aliases_can_change_during_a_chain_of_resumes(self):
        start, call, output, resume = json.loads((FIXTURE / "rollout.json").read_text())
        wall = start["payload"]["turn_id"]

        def boundary(kind):
            return {"type": "event_msg", "payload": {"type": kind, "turn_id": wall}}

        events = [start, call, output, boundary("turn_complete"), boundary("turn_started"),
                  boundary("task_complete"), resume, boundary("task_complete")]
        self.assertEqual(renewal.validate_completed_original_turn(events, wall, 3), [wall] * 3)


if __name__ == "__main__":
    unittest.main()
