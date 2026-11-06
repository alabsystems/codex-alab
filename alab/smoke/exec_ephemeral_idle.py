# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Observe an idle child before quota and exercise it again after native recovery."""
import json
import time

import capacity_exec as probe


class IdleChild:
    BEFORE = "Original idle child completed."
    AFTER = "Restored idle child followup completed."

    def __init__(self, owner, *, task, followup, model, spawn_id, followup_id, find_tool):
        self.owner = owner
        self.task = task
        self.followup = followup
        self.model = model
        self.spawn_id = spawn_id
        self.followup_id = followup_id
        self.find_tool = find_tool
        self.phase = "before"
        self.calls = []
        self.last_status = None
        self.completed_before_quota = False
        self.idle_after_restore = False
        self.completed_after_restore = False

    def call(self, body, name, call_id, arguments):
        tool = self.find_tool(body, name)
        probe.require(tool is not None, f"idle-child fixture requires {name}")
        self.calls.append(call_id)
        return {"type": "function_call", "call_id": call_id, **tool,
                "arguments": json.dumps(arguments)}

    def status(self, body):
        probe.require(len(self.calls) < 60, "idle child did not reach its expected status")
        time.sleep(.05)
        self.last_status = f"ephemeral-idle-status-{len(self.calls)}"
        return self.call(body, "list_agents", self.last_status, {"path_prefix": "/root/pending_mail"})

    @staticmethod
    def output(body, call_id):
        outputs = [item for item in body.get("input", [])
                   if item.get("type") == "function_call_output" and item.get("call_id") == call_id]
        probe.require(len(outputs) == 1, "idle-child tool output was lost or repeated")
        return outputs[0]["output"]

    def completed(self, body, expected):
        if self.last_status is None:
            return False
        agents = json.loads(self.output(body, self.last_status))["agents"]
        children = [agent for agent in agents if agent["agent_name"] == "/root/pending_mail"]
        probe.require(len(children) == 1, "idle child is missing or duplicated")
        state = children[0]["agent_status"]
        if state in ("pending_init", "running"):
            return False
        probe.require(isinstance(state, dict) and "completed" in state,
                      "idle child was interrupted or failed")
        if expected == self.AFTER and state["completed"] == self.BEFORE:
            return False
        probe.require(state["completed"] == expected, "idle child completed different work")
        return True

    def root_item(self, body, generation, number):
        if number == 1:
            probe.require(generation == "old", "idle-child root started with the wrong account")
            return self.call(body, "spawn_agent", self.spawn_id,
                             {"task_name": "pending_mail", "message": self.task,
                              "fork_turns": "none", "model": self.model})
        if generation == "old":
            probe.require(self.phase == "before", "idle-child source repeated its quota request")
            if not self.completed(body, self.BEFORE):
                return self.status(body)
            self.completed_before_quota = True
            self.phase = "quota"
            return None
        probe.require(generation == "new" and self.completed_before_quota,
                      "idle child rotated before its original completion")
        if self.phase == "quota":
            self.phase = "restored"
            self.last_status = None
        if self.phase == "restored":
            if not self.completed(body, self.BEFORE):
                return self.status(body)
            self.idle_after_restore = True
            self.phase = "followup"
            self.last_status = None
            return self.call(body, "followup_task", self.followup_id,
                             {"target": "/root/pending_mail", "message": self.followup})
        if self.phase == "followup":
            probe.require(self.output(body, self.followup_id) == "", "idle-child followup was not accepted")
            self.phase = "after"
        probe.require(self.phase == "after", "unexpected idle-child phase")
        if not self.completed(body, self.AFTER):
            return self.status(body)
        self.completed_after_restore = True
        return {"type": "message", "role": "assistant", "id": "ephemeral-idle-root-finished",
                "content": [{"type": "output_text", "text": "Native renewal fixture recovered."}]}

    def child_item(self, body, generation, number):
        rendered = json.dumps(body.get("input", []))
        probe.require(self.task in rendered, "idle child lost original context")
        if number == 1:
            probe.require(generation == "old" and self.followup not in rendered,
                          "idle child started with replacement input or account")
            text = self.BEFORE
        else:
            probe.require(number == 2 and generation == "new" and self.followup in rendered,
                          "idle child lost or repeated its restored followup")
            text = self.AFTER
        return {"type": "message", "role": "assistant", "id": f"ephemeral-idle-child-{number}",
                "content": [{"type": "output_text", "text": text}]}

    def verify(self, report, parent):
        requests = self.owner.child_requests
        identities = [request.get("requestIdentity", {}) for request in requests]
        probe.require(len(identities) == 2 and all(
            identity.get("thread_id") and identity.get("turn_id")
            and identity.get("parent_thread_id") == parent for identity in identities),
            "idle-child request identities are missing or have the wrong parent")
        first, second = identities
        probe.require(first["thread_id"] == second["thread_id"] != parent
                      and first["turn_id"] != second["turn_id"],
                      "restored idle child changed identity or reused its completed turn")
        probe.require(self.completed_before_quota and self.idle_after_restore
                      and self.completed_after_restore, "idle-child lifecycle was not observed")
        report["ephemeralIdleChildContinuity"] = {"original": first, "followup": second,
                                                "idleBeforeQuota": True, "completed": True}
