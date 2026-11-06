#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Provider and continuity oracle for a genuine `codex exec review` turn."""
import json
import time

import capacity_exec as probe
import renewal_follow as native

MARKER = "review-work-before-quota"
ANSWER = "Native review fixture recovered."
OUTPUT = {"findings": [], "overall_correctness": "patch is correct",
          "overall_explanation": ANSWER, "overall_confidence_score": 1.0}


class Review:
    def __init__(self, find_tool, *, control=False):
        self.find_tool = find_tool
        self.control = control

    def reply(self, model, body, generation):
        if native.request_kind(body) == "thread-title":
            return native.Model.reply(model, body, generation)
        probe.require(probe.PROMPT in json.dumps(body.get("input", [])),
                      "review request lost the original custom instructions")
        record = {"at": time.monotonic(), "unixAt": time.time(), "kind": "foreground",
                  "role": "review", "body": body}
        with model.lock:
            model.requests.append(record)
            number = len(model.requests)
        if number == 1:
            probe.require(generation == "old", "review did not start on the source account")
            tool = self.find_tool(body, "exec_command")
            probe.require(tool is not None, "review has no command tool for retained-work proof")
            item = {"type": "function_call", "call_id": probe.CALL_ID, **tool,
                    "arguments": json.dumps({"cmd": f"printf '{MARKER}\\n'", "max_output_tokens": 100})}
        else:
            outputs = [item for item in body.get("input", [])
                       if item.get("type") == "function_call_output"
                       and item.get("call_id") == probe.CALL_ID]
            probe.require(len(outputs) == 1 and MARKER in str(outputs[0].get("output")),
                          "review lost or duplicated actual pre-quota command output")
            if generation == "old" and not self.control:
                return 429, json.dumps({"error": {"type": "usage_limit_reached",
                    "resets_at": int(time.time()) + 86400,
                    "message": "Review weekly quota fixture"}}).encode(), "application/json", None, record
            probe.require(generation == ("old" if self.control else "new"),
                          "review completed on the wrong account")
            if not self.control:
                (model.root / "renewed").touch()
            item = {"type": "message", "role": "assistant", "id": "review-result",
                    "content": [{"type": "output_text", "text": json.dumps(OUTPUT)}]}
        return 200, probe.response(f"review-{number}", item), "text/event-stream", None, record

    def verify(self, root, requests, report, parent=None, turn=None, *, ephemeral=False, events=None):
        expected = 2 if self.control else 3
        probe.require(len(requests) == expected, "review request count changed")
        identity = requests[0].get("requestIdentity", {})
        probe.require(all(identity.get(key) for key in ("thread_id", "turn_id", "parent_thread_id")),
                      "review delegate identity is missing")
        probe.require(all(row.get("requestIdentity") == identity for row in requests),
                      "review restarted as a different delegate or turn")
        if parent is not None:
            probe.require(identity["parent_thread_id"] == parent,
                          "review delegate changed its owning root")
        probe.require(all(row.get("oldTokenMatched") is True for row in requests[:-1]),
                      "review changed accounts before its quota boundary")
        probe.require(requests[-1].get("oldTokenMatched" if self.control else "newTokenMatched") is True,
                      "review completion did not use the admitted account")
        if ephemeral:
            self.verify_ephemeral(root, events, report, identity, turn)
            return
        histories = []
        for path in (root / "codex-home").rglob("rollout-*.jsonl"):
            rows = [json.loads(line) for line in path.read_text().splitlines() if line]
            metadata = [row.get("payload", {}) for row in rows if row.get("type") == "session_meta"]
            if metadata and metadata[0].get("id") == identity["parent_thread_id"]:
                histories.extend(rows)
        events = [row.get("payload", {}) for row in histories if row.get("type") == "event_msg"]
        # The parent forwards the delegate's TaskStarted; its own review entry
        # item and TaskComplete carry the parent turn. Do not mistake that
        # forwarded event for a new parent turn.
        entered = [event for event in events if event.get("type") == "item_completed"
                   and event.get("item", {}).get("type") == "EnteredReviewMode"]
        ends = [event for event in events if event.get("type") == "task_complete"]
        probe.require(len(entered) == 1 and len(ends) == 1, "original review lifecycle is incomplete")
        turn = turn or entered[0].get("turn_id")
        probe.require(turn and all(event.get("turn_id") == turn for event in entered + ends),
                      "review restarted as a different parent turn")
        completed = [event.get("item", {}) for event in events
                     if event.get("type") == "item_completed"
                     and event.get("item", {}).get("type") == "ExitedReviewMode"]
        probe.require(len(completed) == 1 and completed[0].get("review_output") == OUTPUT,
                      "original review did not publish its structured result exactly once")
        exits = [event for event in events if event.get("type") == "item_completed"
                 and event.get("item", {}).get("type") == "ExitedReviewMode"]
        probe.require(exits[0].get("turn_id") == turn, "structured review result belongs to another turn")
        probe.require(not any(row.get("type") == "event_msg"
                              and row.get("payload", {}).get("type") == "turn_aborted"
                              for row in histories), "original review was aborted")
        report["reviewContinuity"] = {"identity": identity, "parentTurn": turn, "structuredResult": True,
                                      "control": self.control}

    def verify_ephemeral(self, root, events, report, identity, turn):
        # Exec JSON exposes the rendered result, not typed review lifecycle items.
        # Keep that evidence distinct from the durable structured-result oracle.
        probe.require(not list((root / "codex-home").rglob("rollout-*.jsonl")),
                      "ephemeral review persisted a session rollout")
        probe.require(isinstance(events, list), "ephemeral review requires exec JSON evidence")
        starts = [event for event in events if event.get("type") == "thread.started"]
        types = [event.get("type") for event in events]
        probe.require(len(starts) == 1 and starts[0].get("thread_id") == identity["parent_thread_id"],
                      "ephemeral review changed its owning root")
        probe.require(types.count("turn.started") == types.count("turn.completed") == 1
                      and "turn.failed" not in types,
                      "ephemeral review lifecycle is incomplete or replayed")
        probe.require(self.control or isinstance(turn, str) and bool(turn),
                      "ephemeral review recovery needs the native parent turn binding")
        completed = [event.get("item", {}) for event in events if event.get("type") == "item.completed"]
        answers = [item for item in completed if item.get("type") == "agent_message"]
        probe.require(len(answers) == 1 and answers[0].get("text") == ANSWER,
                      "ephemeral review did not render its result exactly once")
        commands = [item for item in completed if item.get("type") == "command_execution"]
        probe.require(len(commands) == 1 and commands[0].get("exit_code") == 0
                      and commands[0].get("status") == "completed"
                      and commands[0].get("aggregated_output", "").strip() == MARKER,
                      "ephemeral review lost or repeated its actual command output")
        report["reviewContinuity"] = {
            "identity": identity, "parentTurn": turn, "control": self.control,
            "evidence": "exec-json-and-native-receipt" if turn else "exec-json-control",
            "renderedResult": True, "noRollout": True,
        }
