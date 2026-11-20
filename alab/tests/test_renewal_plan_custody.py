# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check real durable tool evidence, independently of nonpersisted UI events."""
import copy
import json
from pathlib import Path
import runpy
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import renewal_follow as renewal

FIXTURES = Path(__file__).parent / "fixtures" / "renewal-plan-custody"


class DurablePlanTests(unittest.TestCase):
    def setUp(self):
        self.events = json.loads((FIXTURES / "rollout.json").read_text())
        self.wall = self.events[0]["payload"]["turn_id"]

    def test_actual_completed_tool_before_original_turn_recovery(self):
        self.assertEqual(renewal.validate_durable_plan(self.events, self.wall), [self.wall, self.wall])
        self.assertFalse(any(event["payload"]["type"] == "plan_update" for event in self.events))
        project = runpy.run_path(str(FIXTURES / "project.py"))["project"]
        self.assertEqual(project(self.events), self.events)
        provenance = json.loads((FIXTURES / "provenance.json").read_text())
        self.assertRegex(provenance["sha256"], r"^[0-9a-f]{64}$")

    def test_rollout_reads_physical_utf8_lines_including_unicode_in_message_text(self):
        message = {"type": "response_item", "payload": {"type": "message", "role": "user",
                   "content": [{"type": "input_text", "text": "one\u0085two\u2028three\u2029four"}]}}
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            sessions = root / "codex-home/sessions"
            sessions.mkdir(parents=True)
            path = sessions / "rollout-thread.jsonl"
            events = [message, *self.events]
            path.write_bytes(b"\r\n".join(json.dumps(row, ensure_ascii=False).encode("utf-8") for row in events))
            self.assertEqual(renewal.thread_rollout(root, "thread"), events)
            with path.open("ab") as stream:
                stream.write(b'\n{"type":')
            with self.assertRaises(json.JSONDecodeError):
                renewal.thread_rollout(root, "thread")

    def test_changed_lost_duplicated_or_late_work_is_rejected(self):
        for mutation in ("duplicate_call", "duplicate_output", "missing_call", "missing_output",
                         "wrong_name", "wrong_args", "invalid_args", "wrong_call_id", "wrong_output_id",
                         "wrong_output", "wrong_wall", "extra_start", "late_output", "late_call",
                         "output_before_call", "before_first_start", "ui_plan", "other_tool"):
            events = copy.deepcopy(self.events)
            call, output = events[1]["payload"], events[2]["payload"]
            if mutation == "duplicate_call":
                events.insert(2, copy.deepcopy(events[1]))
            elif mutation == "duplicate_output":
                events.insert(3, copy.deepcopy(events[2]))
            elif mutation == "missing_call":
                del events[1]
            elif mutation == "missing_output":
                del events[2]
            elif mutation == "wrong_name":
                call["name"] = "another_tool"
            elif mutation == "wrong_args":
                call["arguments"] = json.dumps({"plan": [{"step": renewal.probe.PLAN, "status": "in_progress"}]})
            elif mutation == "invalid_args":
                call["arguments"] = "not JSON"
            elif mutation == "wrong_call_id":
                call["call_id"] = "another-call"
            elif mutation == "wrong_output_id":
                output["call_id"] = "another-call"
            elif mutation == "wrong_output":
                output["output"] = "Plan did not update"
            elif mutation == "wrong_wall":
                events[3]["payload"]["turn_id"] = "another-native-turn"
            elif mutation == "extra_start":
                events.append(copy.deepcopy(events[3]))
            elif mutation == "late_output":
                events.append(events.pop(2))
            elif mutation == "late_call":
                events.append(events.pop(1))
            elif mutation == "output_before_call":
                events[1], events[2] = events[2], events[1]
            elif mutation == "before_first_start":
                events.insert(2, events.pop(0))
            elif mutation == "ui_plan":
                events.append({"type": "event_msg", "payload": {"type": "plan_update", "plan": []}})
            else:
                events.insert(3, {"type": "response_item", "payload": {"type": "custom_tool_call"}})
            with self.subTest(mutation=mutation):
                with self.assertRaises(RuntimeError):
                    renewal.validate_durable_plan(events, self.wall)

    def test_native_calls_after_recovery_survive_projection_and_fail_the_check(self):
        project = runpy.run_path(str(FIXTURES / "project.py"))["project"]
        for kind in ("local_shell_call", "web_search_call", "image_generation_call"):
            with self.subTest(kind=kind):
                extra = {"type": "response_item", "payload": {"type": kind}}
                rows = [*self.events, extra]
                projected = project(rows)
                self.assertEqual(projected[-1], extra)
                for events in (rows, projected):
                    with self.assertRaisesRegex(RuntimeError, "lost or duplicated"):
                        renewal.validate_durable_plan(events, self.wall)

    def test_supplied_tool_turn_attribution_must_agree_with_the_recovered_turn(self):
        project = runpy.run_path(str(FIXTURES / "project.py"))["project"]
        for index in (1, 2):
            for turn in (self.wall, "another-turn"):
                rows = copy.deepcopy(self.events)
                rows[index]["payload"]["internal_chat_message_metadata_passthrough"] = {
                    "turn_id": turn, "unrelated": "private"}
                projected = project(rows)
                self.assertNotIn("private", json.dumps(projected))
                for evidence in (rows, projected):
                    with self.subTest(index=index, turn=turn):
                        if turn == self.wall:
                            self.assertEqual(renewal.validate_durable_plan(evidence, self.wall), [turn, turn])
                        else:
                            with self.assertRaisesRegex(RuntimeError, "turn attribution"):
                                renewal.validate_durable_plan(evidence, self.wall)


if __name__ == "__main__":
    unittest.main()
