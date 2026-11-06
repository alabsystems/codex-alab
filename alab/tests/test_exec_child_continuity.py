#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The child oracle distinguishes continuation from a successful replacement turn."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import exec_accounts as accounts
import exec_child_continuity as continuity


class ChildContinuityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "codex-home").mkdir()
        (self.root / "child-held-in-flight").touch()

    def history(self, events):
        rows = [{"type": "session_meta", "payload": {"id": "same-child", "agent_path": "/root/pending_mail"}}]
        rows += [{"type": "event_msg", "payload": event} for event in events]
        (self.root / "codex-home/rollout-child.jsonl").write_text(
            "".join(json.dumps(row) + "\n" for row in rows))

    def test_same_original_turn_can_resume_and_complete(self):
        self.history([{"type": "task_started", "turn_id": "original"},
                      {"type": "task_started", "turn_id": "original"},
                      {"type": "task_complete", "turn_id": "original"}])
        continuity.wait_for_original_turn_end(self.root)
        result = {}
        continuity.verify(self.root, result, active=True)
        self.assertTrue(result["childContinuity"]["passed"])

    def test_successful_replacement_turn_does_not_hide_original_abort(self):
        self.history([{"type": "task_started", "turn_id": "original"},
                      {"type": "turn_aborted", "turn_id": "original"},
                      {"type": "task_started", "turn_id": "new"},
                      {"type": "task_complete", "turn_id": "new"}])
        continuity.wait_for_original_turn_end(self.root)
        with self.assertRaisesRegex(RuntimeError, "aborted"):
            continuity.verify(self.root, {}, active=True)

    def test_unrelated_completion_does_not_release_the_parent(self):
        self.history([{"type": "task_started", "turn_id": "original"},
                      {"type": "task_complete", "turn_id": "other"}])
        with mock.patch.object(continuity.time, "monotonic", side_effect=[0, 0, 2]), \
                mock.patch.object(continuity.time, "sleep"):
            with self.assertRaisesRegex(RuntimeError, "original child did not reach"):
                continuity.wait_for_original_turn_end(self.root, timeout=1)

    def test_wait_ignores_a_partial_utf8_append(self):
        self.history([{"type": "task_started", "turn_id": "original"},
                      {"type": "task_complete", "turn_id": "original"}])
        path = self.root / "codex-home/rollout-child.jsonl"
        with path.open("ab") as stream:
            stream.write(b'{"type":"event_msg","payload":{"text":"\xe2')
        continuity.wait_for_original_turn_end(self.root)

    def test_resumed_request_keeps_completed_tool_output_without_new_prompt(self):
        model = accounts.Model(self.root, pending_mail=True, active_child=True)
        model.child_requests = [{}, {}]
        body = {"input": [{"type": "function_call_output", "call_id": accounts.PENDING_SEND_ID, "output": ""}]}
        status, _, _, _, _ = model.pending_child_reply(body, "new")
        self.assertEqual(status, 200)
        self.assertTrue((self.root / "active-child-resumed-request.json").is_file())


    def test_child_quota_waits_for_real_parent_tool_and_only_child_gets_429(self):
        model = accounts.Model(self.root, child_quota=True)
        model.requests = [{}]
        body = {"tools": [{"type": "function", "name": "wait_agent"}],
                "input": [{"type": "function_call_output", "call_id": accounts.probe.CALL_ID, "output": "child"}]}
        status, _, _, _, _ = model.pending_root_reply(body, "old")
        self.assertEqual(status, 200)
        self.assertFalse(accounts.parent_is_waiting(self.root))
        started = {"type": "item.started", "item": {"id": "wait", "type": "collab_tool_call",
                   "tool": "wait", "status": "in_progress"}}
        (self.root / "pty.log").write_text(json.dumps(started) + "\n")
        self.assertTrue(accounts.parent_is_waiting(self.root))
        status, data, _, _, _ = model.pending_child_reply({}, "old")
        self.assertEqual(status, 429)
        self.assertEqual(json.loads(data)["error"]["type"], "usage_limit_reached")
        self.assertTrue((self.root / "child-quota-reached").is_file())
        status, _, _, _, _ = model.pending_child_reply({}, "new")
        self.assertEqual(status, 200)
        self.assertTrue((self.root / "active-child-resumed-request.json").is_file())

    def test_child_quota_notification_may_wake_parent_before_switch(self):
        model = accounts.Model(self.root, child_quota=True)
        model.requests = [{}]
        body = {"tools": [{"type": "function", "name": "wait_agent"}],
                "input": [{"type": "function_call_output", "call_id": accounts.probe.CALL_ID, "output": "child"}]}
        for _ in range(2):
            status, _, _, _, _ = model.pending_root_reply(body, "old")
            self.assertEqual(status, 200)
        self.assertEqual(len(set(model.root_wait_calls)), 2)
        with mock.patch.object(continuity, "wait_for_original_turn_end"):
            status, _, _, _, _ = model.pending_root_reply(body, "new")
        self.assertEqual(status, 200)

    def test_finished_or_partial_wait_does_not_authorize_child_quota(self):
        started = {"type": "item.started", "item": {"id": "wait", "type": "collab_tool_call",
                   "tool": "wait", "status": "in_progress"}}
        path = self.root / "pty.log"
        path.write_text(json.dumps(started))
        self.assertFalse(accounts.parent_is_waiting(self.root))
        path.write_text(json.dumps(started) + "\n" + json.dumps(
            {"type": "item.completed", "item": {"id": "wait"}}) + "\n")
        self.assertFalse(accounts.parent_is_waiting(self.root))

    def test_child_quota_rejects_resumption_with_old_account(self):
        model = accounts.Model(self.root, child_quota=True)
        model.child_requests = [{}]
        with self.assertRaisesRegex(RuntimeError, "selected account"):
            model.pending_child_reply({}, "old")

    def test_ephemeral_completion_requires_the_original_child_turn(self):
        identity = {"thread_id": "child", "turn_id": "original", "parent_thread_id": "root"}
        original = {"requestIdentity": identity}
        report = {}
        continuity.verify_ephemeral(report, [original, original], "root", True)
        self.assertTrue(report["ephemeralChildContinuity"]["completed"])
        changed = {"requestIdentity": dict(identity, turn_id="replacement")}
        for requests, parent, completed in (
                ([original, changed], "root", True),
                ([original, original], "other-root", True),
                ([original, original], "root", False),
                ([{}, {}], "root", True)):
            with self.subTest(requests=requests, parent=parent, completed=completed):
                with self.assertRaises(RuntimeError):
                    continuity.verify_ephemeral({}, requests, parent, completed)

    def test_ephemeral_status_waits_for_runtime_completion(self):
        model = accounts.Model(self.root, child_quota=True, ephemeral=True)
        body = {"tools": [{"type": "function", "name": "list_agents"}]}
        with mock.patch.object(accounts.time, "sleep"):
            call = model.ephemeral_child_status(body)
        self.assertEqual(call["name"], "list_agents")
        body["input"] = [{"type": "function_call_output", "call_id": call["call_id"],
                          "output": json.dumps({"agents": [{"agent_name": "/root/pending_mail",
                          "agent_status": {"completed": "Original child quota continuation succeeded."}}]})}]
        self.assertIsNone(model.ephemeral_child_status(body))
        self.assertTrue(model.child_completed)

    def test_ephemeral_status_rejects_interrupted_or_different_work(self):
        for state in ("interrupted", "shutdown", "not_found", {"errored": "failed"},
                      {"completed": "different task"}):
            with self.subTest(state=state):
                model = accounts.Model(self.root, child_quota=True, ephemeral=True)
                model.child_status_calls = ["status"]
                body = {"input": [{"type": "function_call_output", "call_id": "status",
                        "output": json.dumps({"agents": [{"agent_name": "/root/pending_mail",
                        "agent_status": state}]})}]}
                with self.assertRaises(RuntimeError):
                    model.ephemeral_child_status(body)
                self.assertFalse(model.child_completed)

    def test_idle_child_is_observed_before_quota_and_again_before_followup(self):
        model = accounts.Model(self.root, child_followup=True, ephemeral=True)
        case = model.idle_child
        body = {"tools": [{"type": "function", "name": name}
                          for name in ("spawn_agent", "list_agents", "followup_task")], "input": []}

        def output(call, value):
            body["input"].append({"type": "function_call_output", "call_id": call["call_id"],
                                  "output": value})

        def status(call, state):
            output(call, json.dumps({"agents": [{"agent_name": "/root/pending_mail",
                                                 "agent_status": state}]}))

        with mock.patch("exec_ephemeral_idle.time.sleep"):
            output(case.root_item(body, "old", 1), "child")
            call = case.root_item(body, "old", 2)
            status(call, "running")
            call = case.root_item(body, "old", 3)
            self.assertFalse(case.completed_before_quota)
            status(call, {"completed": case.BEFORE})
            self.assertIsNone(case.root_item(body, "old", 4))
            self.assertTrue(case.completed_before_quota)
            call = case.root_item(body, "new", 5)
            self.assertEqual(call["name"], "list_agents")
            self.assertFalse(case.idle_after_restore)
            status(call, {"completed": case.BEFORE})
            call = case.root_item(body, "new", 6)
            self.assertEqual(call["name"], "followup_task")
            self.assertTrue(case.idle_after_restore)
            output(call, "")
            call = case.root_item(body, "new", 7)
            status(call, {"completed": case.BEFORE})
            call = case.root_item(body, "new", 8)
            self.assertFalse(case.completed_after_restore)
            status(call, {"completed": case.AFTER})
            self.assertEqual(case.root_item(body, "new", 9)["type"], "message")
            self.assertTrue(case.completed_after_restore)

    def test_idle_child_cannot_rotate_after_interruption(self):
        model = accounts.Model(self.root, child_followup=True, ephemeral=True)
        case = model.idle_child
        case.last_status = "status"
        body = {"input": [{"type": "function_call_output", "call_id": "status",
                "output": json.dumps({"agents": [{"agent_name": "/root/pending_mail",
                                                    "agent_status": "interrupted"}]})}]}
        with self.assertRaisesRegex(RuntimeError, "interrupted"):
            case.root_item(body, "old", 2)
        self.assertFalse(case.completed_before_quota)

    def test_idle_child_followup_requires_same_child_and_new_turn(self):
        model = accounts.Model(self.root, child_followup=True, ephemeral=True)
        case = model.idle_child
        case.completed_before_quota = case.idle_after_restore = case.completed_after_restore = True
        original = {"thread_id": "child", "turn_id": "original", "parent_thread_id": "root"}
        followup = dict(original, turn_id="followup")
        model.child_requests = [{"requestIdentity": original}, {"requestIdentity": followup}]
        case.verify({}, "root")
        for replacement in (original, dict(followup, thread_id="replacement"),
                            dict(followup, parent_thread_id="other")):
            model.child_requests[1] = {"requestIdentity": replacement}
            with self.subTest(replacement=replacement), self.assertRaises(RuntimeError):
                case.verify({}, "root")


if __name__ == "__main__":
    unittest.main()
