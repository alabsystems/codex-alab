#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Run real Codex exec through a native account handoff with isolated logins.

Builds nothing. All provider requests, credentials and PTY output stay in --output.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import select
import subprocess
import time

import capacity_exec as probe
import renewal_follow as native

ORIGINAL_INSTRUCTIONS = "Retain the original handoff configuration marker."
CHANGED_INSTRUCTIONS = "Replacement configuration must not enter the original turn."
PENDING_CHILD_TASK = "Deliver the pending-mail fixture result to the root agent."
PENDING_MAIL = "Pending mail accepted during the original quota wait."
PENDING_SEND_ID = "pending-mail-send"
PENDING_MODEL = "gpt-5.6-sol"
FOLLOWUP_TEXT = "Confirm the restored child can issue a request after the parent account switch."
FOLLOWUP_ID = "restored-child-followup"
CHILD_QUOTA_WAIT_ID = "child-quota-parent-wait"


def request_tools(body):
    tools = list(body.get("tools") or [])
    for item in body.get("input", []):
        if item.get("type") == "additional_tools":
            tools.extend(item.get("tools", []))
    return tools


def find_tool(body, name):
    for tool in request_tools(body):
        if not isinstance(tool, dict):
            continue
        if tool.get("type") == "function" and tool.get("name") == name:
            return {"name": name}
        if tool.get("type") == "namespace" and any(
                isinstance(nested, dict) and nested.get("name") == name
                for nested in tool.get("tools", [])):
            return {"name": name, "namespace": tool["name"]}
    return None


def parent_is_waiting(root):
    path = root / "pty.log"
    if not path.is_file():
        return False
    pending = set()
    for line in path.read_bytes().splitlines(keepends=True):
        if not line.endswith(b"\n"):
            continue
        try:
            event = json.loads(line)
        except (ValueError, UnicodeDecodeError):
            continue
        item = event.get("item", {})
        if (event.get("type") == "item.started" and item.get("type") == "collab_tool_call"
                and item.get("tool") == "wait" and item.get("status") == "in_progress"):
            pending.add(item.get("id"))
        if event.get("type") == "item.completed":
            pending.discard(item.get("id"))
    return bool(pending)


def managed_checkout(root):
    result = subprocess.run(["/usr/bin/git", "-C", str(root / "work"), "worktree", "list", "--porcelain"],
                            check=True, capture_output=True, text=True)
    paths = [Path(line.removeprefix("worktree ")).resolve() for line in result.stdout.splitlines()
             if line.startswith("worktree ")]
    managed = [path for path in paths if path != (root / "work").resolve()]
    probe.require(len(managed) == 1, "handoff lost or allocated another managed checkout")
    probe.require(managed[0].is_relative_to((root / "codex-home/worktrees").resolve()),
                  "managed checkout escaped the fixture pool")
    return managed[0]


class Model(native.Model):
    def __init__(self, root, chain=False, exhausted=False, change_model=False, worktree=False, pending_mail=False, token_usage=False, child_followup=False, active_child=False, child_quota=False, ephemeral=False, review=None):
        super().__init__(root, weekly_quota=True, chain=chain, all_exhausted=exhausted)
        self.review = None
        if review is not None:
            from exec_review import Review
            self.review = Review(find_tool, control=review == "control")
        self.change_model = change_model
        self.model_changed = False
        self.worktree = worktree
        self.edited_checkout = None
        self.pending_mail = pending_mail or child_quota or (ephemeral and child_followup)
        self.child_quota = child_quota
        self.ephemeral = ephemeral
        self.token_usage = token_usage
        self.child_followup = child_followup
        self.active_child = active_child
        self.child_requests = []
        self.root_wait_calls = []
        self.child_status_calls = []
        self.child_completed = False
        self.idle_child = None
        if ephemeral and child_followup:
            from exec_ephemeral_idle import IdleChild
            self.idle_child = IdleChild(self, task=PENDING_CHILD_TASK, followup=FOLLOWUP_TEXT,
                                       model=PENDING_MODEL, spawn_id=probe.CALL_ID,
                                       followup_id=FOLLOWUP_ID, find_tool=find_tool)

    def usage(self, old_token, new_token, next_token=False):
        probe.require(old_token or new_token or (self.chain and next_token), "unexpected usage account")
        with self.lock:
            if self.worktree and self.edited_checkout is None:
                checkout = managed_checkout(self.root)
                (checkout / "tracked.txt").write_text("original task edits\n")
                (checkout / "unfinished.txt").write_text("untracked task work\n")
                self.edited_checkout = checkout
            if self.change_model and not self.model_changed:
                config = self.root / "codex-home/config.toml"
                source = config.read_text()
                original = f'model = "{PENDING_MODEL if self.pending_mail else probe.MODEL}"'
                probe.require(source.count(original) == 1, "original model config missing")
                instruction = f'developer_instructions = {json.dumps(ORIGINAL_INSTRUCTIONS)}'
                probe.require(source.count(instruction) == 1, "original instructions missing")
                source = source.replace(original, 'model = "gpt-5.4"', 1)
                config.write_text(source.replace(instruction,
                                  f'developer_instructions = {json.dumps(CHANGED_INSTRUCTIONS)}', 1))
                self.model_changed = True
            self.usage_reads.append("source" if old_token else "target")
            full = self.all_exhausted or old_token or (self.chain and (
                (new_token and self.middle_exhausted) or (next_token and not self.middle_exhausted)))
            if self.review and self.review.control:
                full = False
        return {"plan_type": "pro", "rate_limit": {
            "allowed": not full, "limit_reached": full,
            "primary_window": {"used_percent": 0, "limit_window_seconds": 18000,
                               "reset_at": int(time.time()) + 3600},
            "secondary_window": {"used_percent": 100 if full else 0,
                                 "limit_window_seconds": 604800,
                                 "reset_at": int(time.time()) + 86400}}}

    def reply(self, body, generation=None):
        if self.review:
            return self.review.reply(self, body, generation)
        if self.pending_mail and native.request_kind(body) != "thread-title":
            rendered = json.dumps(body.get("input", []))
            if PENDING_CHILD_TASK in rendered and probe.PROMPT not in rendered:
                return self.pending_child_reply(body, generation)
            probe.require(probe.PROMPT in rendered, "unknown pending-mail fixture request")
            return self.pending_root_reply(body, generation)
        if generation == "new":
            (self.root / "renewed").touch()
        return super().reply(body, generation)

    def pending_root_reply(self, body, generation):
        if self.idle_child:
            record = {"kind": "foreground", "role": "ephemeral-idle-root", "body": body}
            with self.lock:
                self.requests.append(record)
                number = len(self.requests)
            item = self.idle_child.root_item(body, generation, number)
            if item is None:
                return 429, json.dumps({"error": {"type": "usage_limit_reached",
                    "resets_at": int(time.time()) + 86400, "message": "Idle-child quota fixture"}}).encode(), "application/json", None, record
            return 200, probe.response(f"ephemeral-idle-root-{number}", item), "text/event-stream", None, record
        if self.child_quota:
            return self.child_quota_root_reply(body, generation)
        record = {"kind": "foreground", "role": "pending-mail-root", "body": body}
        with self.lock:
            self.requests.append(record)
            number = len(self.requests)
        probe.require(generation == ("new" if number >= 3 else "old"), "root used an unexpected account")
        if number == 1:
            tool = find_tool(body, "spawn_agent")
            if tool is None:
                probe.write_json(self.root / "pending-mail-tool-shapes.json", [
                    {"type": item.get("type"), "name": item.get("name"),
                     "keys": sorted(item),
                     "nested": [{"type": nested.get("type"), "name": nested.get("name")}
                                for nested in item.get("tools", [])]}
                    for item in request_tools(body) if isinstance(item, dict)])
            probe.require(tool is not None, "pending-mail fixture requires the real spawn_agent tool")
            item = {"type": "function_call", "call_id": probe.CALL_ID, **tool,
                    "arguments": json.dumps({"task_name": "pending_mail", "message": PENDING_CHILD_TASK,
                                             "fork_turns": "none", "model": PENDING_MODEL})}
        elif number == 2:
            outputs = [item for item in body.get("input", [])
                       if item.get("type") == "function_call_output" and item.get("call_id") == probe.CALL_ID]
            probe.require(len(outputs) == 1, "root did not finish the child launch")
            probe.write_json(self.root / "pending-spawn-output.json", {"output": outputs[0].get("output")})
            return 429, json.dumps({"error": {"type": "usage_limit_reached",
                "resets_at": int(time.time()) + 86400, "message": "Pending-mail quota fixture"}}).encode(), "application/json", None, record
        else:
            probe.require(number in ((3, 4) if self.child_followup else (3,)), "root repeated model work")
            (self.root / "renewed").touch()
            count = json.dumps(body.get("input", [])).count(PENDING_MAIL)
            if count != 1 and not (self.root / "pending-mail-lost.json").exists():
                probe.write_json(self.root / "pending-mail-lost.json", {"count": count,
                    "accepted": (self.root / "pending-mail-accepted.json").is_file()})
            probe.require(count == 1, "replacement lost or duplicated accepted mail")
            if self.active_child:
                from exec_child_continuity import wait_for_original_turn_end
                wait_for_original_turn_end(self.root)
            if self.child_followup and number == 3:
                tool = find_tool(body, "followup_task")
                probe.require(tool is not None, "followup_task unavailable after recovery")
                item = {"type": "function_call", "call_id": FOLLOWUP_ID, **tool,
                        "arguments": json.dumps({"target": "/root/pending_mail", "message": FOLLOWUP_TEXT})}
            else:
                if self.child_followup:
                    outputs = [entry for entry in body.get("input", [])
                               if entry.get("type") == "function_call_output" and entry.get("call_id") == FOLLOWUP_ID]
                    probe.require(len(outputs) == 1 and outputs[0].get("output") == "",
                                  "restored child followup was not accepted")
                    deadline = time.monotonic() + 30
                    while not (self.root / "restored-child-request.json").is_file() and time.monotonic() < deadline:
                        time.sleep(.01)
                    probe.require((self.root / "restored-child-request.json").is_file(),
                                  "restored child made no model request")
                item = {"type": "message", "role": "assistant", "id": "pending-mail-root-finished",
                        "content": [{"type": "output_text", "text": "Native renewal fixture recovered."}]}
        usage = None
        if self.token_usage:
            source = number == 1
            usage = {"input_tokens": 13 if source else 17,
                     "input_tokens_details": {"cached_tokens": 4 if source else 6,
                                               "cache_write_tokens": 2 if source else 3},
                     "output_tokens": 5 if source else 7,
                     "output_tokens_details": {"reasoning_tokens": 2 if source else 3},
                     "total_tokens": 18 if source else 24}
        return 200, probe.response(f"pending-mail-root-{number}", item, usage=usage), "text/event-stream", None, record

    def pending_child_reply(self, body, generation):
        if self.idle_child:
            record = {"kind": "foreground", "role": "ephemeral-idle-child", "body": body}
            with self.lock:
                self.child_requests.append(record)
                number = len(self.child_requests)
            item = self.idle_child.child_item(body, generation, number)
            return 200, probe.response(f"ephemeral-idle-child-{number}", item), "text/event-stream", None, record
        if self.child_quota:
            return self.child_quota_reply(body, generation)
        record = {"kind": "foreground", "role": "pending-mail-child", "body": body}
        with self.lock:
            self.child_requests.append(record)
            number = len(self.child_requests)
        probe.require(generation == ("new" if number == 3 else "old"), "child used the wrong account generation")
        if self.active_child and number == 3:
            outputs = [item for item in body.get("input", [])
                       if item.get("type") == "function_call_output" and item.get("call_id") == PENDING_SEND_ID]
            probe.require(len(outputs) == 1 and outputs[0].get("output") == "",
                          "active child recovery lost its completed message tool output")
            probe.require(FOLLOWUP_TEXT not in json.dumps(body.get("input", [])),
                          "active child was restarted with a new followup prompt")
            probe.write_json(self.root / "active-child-resumed-request.json", {"newAccountMatched": True})
            item = {"type": "message", "role": "assistant", "id": "active-child-resumed-finished",
                    "content": [{"type": "output_text", "text": "Original child continuation succeeded."}]}
        elif self.child_followup and number == 3:
            probe.require(FOLLOWUP_TEXT in json.dumps(body.get("input", [])), "restored child lost followup input")
            probe.write_json(self.root / "restored-child-request.json",
                             {"newAccountMatched": True, "followupInputPresent": True})
            item = {"type": "message", "role": "assistant", "id": "restored-child-finished",
                    "content": [{"type": "output_text", "text": "Restored child request succeeded."}]}
        elif number == 1:
            deadline = time.monotonic() + 45
            while not (self.root / "prepare-ack-paused").is_file() and time.monotonic() < deadline:
                time.sleep(.01)
            probe.require((self.root / "prepare-ack-paused").is_file(), "root never reached native quota preparation")
            tool = find_tool(body, "send_message")
            probe.require(tool is not None, "pending-mail child requires the real send_message tool")
            item = {"type": "function_call", "call_id": PENDING_SEND_ID, **tool,
                    "arguments": json.dumps({"target": "/root", "message": PENDING_MAIL})}
        else:
            probe.require(number == 2, "child repeated its message work")
            outputs = [item for item in body.get("input", [])
                       if item.get("type") == "function_call_output" and item.get("call_id") == PENDING_SEND_ID]
            probe.require(len(outputs) == 1 and outputs[0].get("output") == "", "child message was not acknowledged as accepted")
            probe.write_json(self.root / "pending-mail-accepted.json", {"accepted": True, "childRequests": number})
            (self.root / "continue-prepare-ack").touch()
            if self.active_child:
                (self.root / "child-held-in-flight").touch()
                deadline = time.monotonic() + 30
                while not (self.root / "renewed").is_file() and time.monotonic() < deadline:
                    time.sleep(.01)
                probe.require((self.root / "renewed").is_file(), "parent never recovered while child was active")
            item = {"type": "message", "role": "assistant", "id": "pending-mail-child-finished",
                    "content": [{"type": "output_text", "text": "Child delivery finished."}]}
        return 200, probe.response(f"pending-mail-child-{number}", item), "text/event-stream", None, record


    def child_quota_root_reply(self, body, generation):
        record = {"kind": "foreground", "role": "child-quota-root", "body": body}
        with self.lock:
            self.requests.append(record)
            number = len(self.requests)
        probe.require(generation in ("old", "new") and (number > 1 or generation == "old"),
                      "parent used an unexpected account")
        if number == 1:
            tool = find_tool(body, "spawn_agent")
            probe.require(tool is not None, "child quota requires the real spawn_agent tool")
            item = {"type": "function_call", "call_id": probe.CALL_ID, **tool,
                    "arguments": json.dumps({"task_name": "pending_mail", "message": PENDING_CHILD_TASK,
                                             "fork_turns": "none", "model": PENDING_MODEL})}
        elif generation == "old":
            probe.require(number <= 4, "parent repeatedly woke without child quota recovery")
            tool = find_tool(body, "wait_agent")
            probe.require(tool is not None, "child quota requires the real wait_agent tool")
            outputs = [entry for entry in body.get("input", [])
                       if entry.get("type") == "function_call_output" and entry.get("call_id") == probe.CALL_ID]
            probe.require(len(outputs) == 1, "parent did not finish the child launch")
            # A child quota event may wake an already-running wait before the
            # account controller handles it. Wait again without inventing work.
            call_id = f"{CHILD_QUOTA_WAIT_ID}-{number}"
            self.root_wait_calls.append(call_id)
            item = {"type": "function_call", "call_id": call_id, **tool,
                    "arguments": json.dumps({"timeout_ms": 180000})}
            (self.root / "parent-wait-requested").touch()
        else:
            probe.require(number >= 3, "parent switched before entering its child wait")
            if self.ephemeral:
                status = self.ephemeral_child_status(body)
                if status is not None:
                    return 200, probe.response(f"child-quota-root-{number}", status), "text/event-stream", None, record
            else:
                from exec_child_continuity import wait_for_original_turn_end
                wait_for_original_turn_end(self.root)
            item = {"type": "message", "role": "assistant", "id": "child-quota-root-finished",
                    "content": [{"type": "output_text", "text": "Native renewal fixture recovered."}]}
        return 200, probe.response(f"child-quota-root-{number}", item), "text/event-stream", None, record

    def ephemeral_child_status(self, body):
        if self.child_status_calls:
            outputs = [item for item in body.get("input", [])
                       if item.get("type") == "function_call_output"
                       and item.get("call_id") == self.child_status_calls[-1]]
            probe.require(len(outputs) == 1, "ephemeral child status call was lost or replayed")
            result = json.loads(outputs[0]["output"])
            children = [agent for agent in result["agents"]
                        if agent["agent_name"] == "/root/pending_mail"]
            probe.require(len(children) == 1, "ephemeral child identity was not restored")
            state = children[0]["agent_status"]
            if isinstance(state, dict) and "completed" in state:
                probe.require(state["completed"] == "Original child quota continuation succeeded.",
                              "ephemeral child completed different work")
                self.child_completed = True
                return None
            probe.require(state in ("pending_init", "running"),
                          "ephemeral child was interrupted or failed")
        probe.require(len(self.child_status_calls) < 50, "ephemeral child did not finish its original work")
        time.sleep(.05)
        tool = find_tool(body, "list_agents")
        probe.require(tool is not None, "ephemeral child qualification requires list_agents")
        call_id = f"ephemeral-child-status-{len(self.child_status_calls)}"
        self.child_status_calls.append(call_id)
        return {"type": "function_call", "call_id": call_id, **tool,
                "arguments": json.dumps({"path_prefix": "/root/pending_mail"})}

    def child_quota_reply(self, body, generation):
        record = {"kind": "foreground", "role": "child-quota-child", "body": body}
        with self.lock:
            self.child_requests.append(record)
            number = len(self.child_requests)
        probe.require(generation == ("old" if number == 1 else "new"),
                      "child did not resume on the selected account")
        if number == 1:
            deadline = time.monotonic() + 30
            while not parent_is_waiting(self.root) and time.monotonic() < deadline:
                time.sleep(.01)
            probe.require(parent_is_waiting(self.root), "parent never entered its child wait")
            (self.root / "child-held-in-flight").touch()
            (self.root / "child-quota-reached").touch()
            return 429, json.dumps({"error": {"type": "usage_limit_reached",
                "resets_at": int(time.time()) + 86400, "message": "Child-only quota fixture"}}).encode(), "application/json", None, record
        probe.require(number == 2, "child repeated work after its quota wall")
        probe.require(FOLLOWUP_TEXT not in json.dumps(body.get("input", [])),
                      "child quota recovery started a replacement task")
        probe.write_json(self.root / "active-child-resumed-request.json", {"newAccountMatched": True})
        item = {"type": "message", "role": "assistant", "id": "child-quota-finished",
                "content": [{"type": "output_text", "text": "Original child quota continuation succeeded."}]}
        return 200, probe.response("child-quota-child-finished", item), "text/event-stream", None, record


def stream_problems(events):
    items = {}
    problems = []
    for event in events:
        kind = event.get("type")
        if kind not in ("item.started", "item.updated", "item.completed"):
            continue
        item = event.get("item", {})
        identity = item.get("id")
        item_type = item.get("type")
        if not isinstance(identity, str) or not isinstance(item_type, str):
            problems.append("invalid item identity")
            continue
        previous = items.setdefault(identity, {"type": item_type, "started": 0, "completed": 0})
        if previous["type"] != item_type:
            problems.append(f"{identity} changed item type")
        if kind in ("item.started", "item.completed"):
            state = kind.split(".")[1]
            previous[state] += 1
            if previous[state] > 1:
                problems.append(f"{identity} repeated {state}")
    for identity, state in items.items():
        if state["started"] and not state["completed"]:
            problems.append(f"{identity} never completed")
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True, type=Path)
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--drop-consume-ack", action="store_true")
    parser.add_argument("--chain", action="store_true", help="exhaust a second account and recover the same turn again")
    parser.add_argument("--human", action="store_true", help="check human output across the same native handoff")
    parser.add_argument("--change-model", action="store_true", help="change model and developer instructions during the quota wait and require original settings")
    parser.add_argument("--worktree", action="store_true", help="retain the original managed checkout, owner and local edits")
    parser.add_argument("--exhausted", action="store_true", help="keep the original wait when every account is full, then cancel it")
    parser.add_argument("--stop-after-consume", action="store_true", help="cancel after native consumption but before its acknowledgement reaches Core")
    parser.add_argument("--pending-mail", action="store_true", help="queue child mail while native quota preparation is paused")
    parser.add_argument("--token-usage", action="store_true", help="require nonzero source and replacement usage in the final JSON report")
    parser.add_argument("--child-followup", action="store_true", help="verify a completed child can finish a followup after rotation")
    parser.add_argument("--active-child", action="store_true", help="require the child's original in-flight turn to survive rotation")
    parser.add_argument("--child-quota", action="store_true", help="require a child-only quota wall to rotate the waiting parent and child")
    parser.add_argument("--ephemeral", action="store_true", help="require the original root turn to recover from memory without creating a rollout")
    parser.add_argument("--review", nargs="?", const="quota", choices=("quota", "control"),
                        help="exercise the genuine review task, with a quota wall or a no-wall oracle control")
    args = parser.parse_args()
    probe.require(not args.review or not any((args.human, args.chain, args.exhausted,
                  args.worktree, args.change_model, args.stop_after_consume, args.pending_mail,
                  args.token_usage, args.child_followup, args.active_child, args.child_quota, args.drop_consume_ack)),
                  "review qualification is a distinct structured-result scenario")
    probe.require(not args.ephemeral or not any((args.worktree, args.pending_mail,
                  args.active_child, args.token_usage)),
                  "ephemeral root qualification cannot use the persisted-child oracle or a managed worktree")
    probe.require(not (args.ephemeral and args.child_followup) or not any((args.chain, args.exhausted,
                  args.stop_after_consume, args.change_model, args.human)),
                  "ephemeral idle-child qualification uses one successful JSON account move")
    probe.require(not args.child_quota or not any((args.pending_mail, args.active_child, args.child_followup,
                  args.token_usage, args.chain, args.exhausted, args.stop_after_consume, args.change_model, args.worktree, args.human)),
                  "child quota is a distinct waiting-parent scenario")
    probe.require(not (args.child_followup and args.active_child), "choose one child continuity scenario")
    probe.require(not (args.child_followup or args.active_child)
                  or ((args.pending_mail or (args.ephemeral and args.child_followup)) and not args.token_usage),
                  "child continuity uses pending mail without token-usage accounting")
    probe.require(not (args.exhausted and (args.chain or args.drop_consume_ack)),
                  "exhaustion does not perform a chain or consume acknowledgement")
    probe.require(not (args.stop_after_consume and (args.exhausted or args.chain)),
                  "post-consume cancellation is a distinct case")
    probe.require(not (args.worktree and (args.exhausted or args.stop_after_consume)),
                  "managed-worktree qualification checks successful original-turn recovery")
    probe.require(not (args.pending_mail and (args.chain or args.exhausted or args.stop_after_consume)),
                  "pending mail uses one successful quota move")
    probe.require(not args.token_usage or (args.pending_mail and not args.human),
                  "token-usage qualification uses the pending-mail JSON scenario")
    build, manifest, codex = probe.package_records(args.package.resolve(strict=True))
    host = args.host.resolve(strict=True)
    host_record_file = host.parent / "build.json"
    probe.require(host_record_file.is_file() and not host_record_file.is_symlink()
                  and host_record_file.stat().st_size <= 1024 * 1024, "native host build record missing")
    host_record = json.loads(host_record_file.read_text())
    host_sha = hashlib.sha256(host.read_bytes()).hexdigest()
    probe.require(isinstance(host_record.get("source"), str)
                  and re.fullmatch(r"[0-9a-f]{40}", host_record["source"])
                  and host_record.get("host_sha256") == host_sha,
                  "native host image does not match its recorded source build")
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = probe.fresh_environment(root)
    env["ATERM_HARNESS_STATE"] = str(root / "host-state")
    env["XDG_CONFIG_HOME"] = str(root / "config")
    model = Model(root, chain=args.chain, exhausted=args.exhausted,
                  change_model=args.change_model, worktree=args.worktree, pending_mail=args.pending_mail, token_usage=args.token_usage,
                  child_followup=args.child_followup, active_child=args.active_child, child_quota=args.child_quota,
                  ephemeral=args.ephemeral, review=args.review)
    server, worker = native.serve(model)
    report = {"passed": False, "source": build["source"], "version": manifest["version"],
              "nativeHost": {"source": host_record["source"], "sha256": host_sha},
              "dropConsumeAck": args.drop_consume_ack or args.stop_after_consume,
              "chain": args.chain, "exhausted": args.exhausted,
              "human": args.human, "ephemeral": args.ephemeral, "review": args.review,
              "changeModel": args.change_model,
              "worktree": args.worktree,
              "stopAfterConsume": args.stop_after_consume, "pendingMail": args.pending_mail,
              "tokenUsage": args.token_usage, "childFollowup": args.child_followup, "activeChild": args.active_child, "childQuota": args.child_quota}
    try:
        probe.configure(root, server.server_port)
        if args.worktree:
            for command in (["init", "--quiet"], ["add", "."],
                            ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                             "commit", "--quiet", "--no-gpg-sign", "-m", "fixture"]):
                if command[0] == "add":
                    (root / "work/tracked.txt").write_text("initial file\n")
                subprocess.run(["/usr/bin/git", "-C", str(root / "work"), *command], env=env,
                               check=True, capture_output=True)
        config_file = root / "codex-home/config.toml"
        config = config_file.read_text().replace("requires_openai_auth = false", "requires_openai_auth = true")
        config = config.replace('name = "local capacity smoke"', 'name = "OpenAI"')
        config = config.replace("http://127.0.0.1:", "https://127.0.0.1:")
        config = config.replace("[tools.update_plan]", "[features]\nenable_request_compression = false\n[tools.update_plan]")
        if args.pending_mail or args.child_quota or model.idle_child:
            config = config.replace(f'model = "{probe.MODEL}"', f'model = "{PENDING_MODEL}"', 1)
            config = config.replace("enable_request_compression = false", "enable_request_compression = false\nmulti_agent = true\nmulti_agent_v2 = true")
            if args.pending_mail:
                (root / "pause-prepare-ack").touch()
        if args.worktree:
            config += f'\n[projects.{json.dumps(str((root / "work").resolve()))}]\ntrust_level = "trusted"\n'
        if args.change_model:
            config = f'developer_instructions = {json.dumps(ORIGINAL_INSTRUCTIONS)}\n' + config
        config_file.write_text(f'cli_auth_credentials_store = "file"\nchatgpt_base_url = "https://127.0.0.1:{server.server_port}"\n' + config)
        old, new = root / "auth-old/auth.json", root / "auth-new/auth.json"
        issuer = f"https://127.0.0.1:{server.server_port}"
        native.fake_auth(old, "old", issuer, "source-account")
        native.fake_auth(new, "new", issuer, "target-account", email="target@example.invalid", user="target-user")
        if args.chain:
            final_auth = root / "auth-next/auth.json"
            native.fake_auth(final_auth, "next", issuer, "final-account", email="final@example.invalid", user="final-user")
        fixture = {"root": str(root), "codex": str(codex), "cwd": str(root / "work"),
                   "auth_path": str(old), "successor_auth_path": str(new), "weekly_quota": True,
                   "human_grace_s": 0,
                   "args": ["exec", "--json", "--color", "never", "--skip-git-repo-check",
                            "--dangerously-bypass-approvals-and-sandbox", probe.PROMPT],
                   "env": {"CODEX_HOME": env["CODEX_HOME"], "PATH": env["PATH"],
                           "XDG_CONFIG_HOME": env["XDG_CONFIG_HOME"], "NO_COLOR": "1",
                           "LANG": "en_US.UTF-8", "RUST_LOG": "codex_alab_exec_handoff=debug",
                           "CODEX_CA_CERTIFICATE": str(root / "fixture-ca.pem")}}
        if args.review:
            fixture["args"].insert(-1, "review")
        if args.human:
            fixture["args"].remove("--json")
        if args.ephemeral:
            fixture["args"].insert(1, "--ephemeral")
        if args.worktree:
            fixture["args"].insert(1, "--worktree")
        if args.chain:
            fixture["additional_auth_paths"] = [str(final_auth)]
        probe.write_json(root / "fixture.json", fixture)
        if args.drop_consume_ack or args.stop_after_consume:
            (root / "drop-consume-ack").touch()
        if args.stop_after_consume:
            (root / "pause-consume-ack").touch()
        metadata = None

        def drive(process, output):
            nonlocal metadata
            deadline = time.monotonic() + 200
            buffer = b""
            exhausted_since = None
            try:
                while time.monotonic() < deadline:
                    ready, _, _ = select.select([process.stdout], [], [], .1)
                    if ready:
                        data = os.read(process.stdout.fileno(), 65536)
                        probe.require(bool(data), "native host exited before exec completion")
                        output.write(data)
                        output.flush()
                        buffer += data
                        if metadata is None and b"\n" in buffer:
                            metadata = json.loads(buffer.split(b"\n", 1)[0])
                            probe.require(metadata["child_pid"] > 1, "missing original exec PID")
                    if args.exhausted:
                        with model.lock:
                            both_full = {"source", "target"}.issubset(model.usage_reads)
                        if both_full and exhausted_since is None:
                            exhausted_since = time.monotonic()
                        if exhausted_since is not None and time.monotonic() - exhausted_since >= 6:
                            (root / "owner-stop").touch(exist_ok=True)
                    if args.stop_after_consume and (root / "consume-ack-dropped").exists():
                        (root / "owner-stop").touch(exist_ok=True)
                        if (root / "owner-stop-completed.json").exists():
                            (root / "continue-consume-ack").touch(exist_ok=True)
                    log = root / "pty.log"
                    if log.exists() and any(re.fullmatch(r"EXEC_FIXTURE_EXIT=\d+", line.strip())
                                            for line in log.read_text(errors="replace").splitlines()):
                        if not args.stop_after_consume or (root / "continue-consume-ack").exists():
                            break
                else:
                    raise AssertionError("exec did not complete inside the fixture deadline")
            finally:
                (root / "stop").touch()

        probe.COMMAND_TIMEOUT_SECONDS = 230
        code = probe.run(host, ["--renewal-config", str(root / "fixture.json")], root, env, "native-host", interact=drive)
        probe.require(code == 0, "native host failed")
        text = (root / "pty.log").read_text(errors="replace").replace("\r", "")
        if args.exhausted or args.stop_after_consume:
            stop = json.loads((root / "owner-stop-completed.json").read_text())
            probe.require(stop.get("ok") is True, "owner cancellation did not reach the active exec")
            exit_codes = [int(match.group(1)) for line in text.splitlines()
                          if (match := re.fullmatch(r"EXEC_FIXTURE_EXIT=(\d+)", line.strip()))]
            probe.require(len(exit_codes) == 1 and exit_codes[0] != 0,
                          "cancelled exec did not preserve its parent exit")
            with model.lock:
                requests = list(model.requests)
                reads = list(model.usage_reads)
            probe.require(len(requests) == 2 and all(item.get("oldTokenMatched") for item in requests),
                          "cancelled exec dispatched model work after the original quota wait")
            probe.require({"source", "target"}.issubset(reads), "cancellation skipped pooled account verification")
            receipts = list((root / "host-state/accounts-codex/renewal-follow").glob("*.json"))
            if args.exhausted:
                probe.require(not receipts, "exhausted pool granted an account handoff")
            else:
                consumed = [json.loads(path.read_text()) for path in receipts]
                probe.require(len(consumed) == 1 and consumed[0].get("phase") == "consumed",
                              "owner stop did not occur after native consumption")
                probe.require(consumed[0]["old_pid"] == consumed[0]["successor_pid"] == metadata["child_pid"],
                              "cancelled handoff replaced the original process identity")
            report.update(passed=True, originalPid=metadata["child_pid"], requestCount=len(requests),
                          ownerCancelled=True, exitCode=exit_codes[0])
            print(json.dumps(report, indent=2))
            return 0
        probe.require(any(line.strip() == "EXEC_FIXTURE_EXIT=0" for line in text.splitlines()),
                      "exec did not retain a successful parent wait")
        events = []
        for line in text.splitlines():
            try:
                item = json.loads(line)
            except (ValueError, TypeError):
                continue
            if isinstance(item, dict) and "type" in item:
                events.append(item)
        types = [event["type"] for event in events]
        if not args.human:
            probe.require(types.count("thread.started") == 1 and types.count("turn.started") == 1,
                          "native handoff replayed a JSON startup boundary")
            probe.require(types.count("turn.completed") == 1 and "turn.failed" not in types,
                          "original exec turn did not complete")
        if model.review and model.review.control:
            with model.lock:
                requests = list(model.requests)
            model.review.verify(root, requests, report, ephemeral=args.ephemeral, events=events)
            probe.require(not stream_problems(events), "review control has inconsistent JSON items")
            report.update(passed=True, originalPid=metadata["child_pid"], requestCount=len(requests))
            print(json.dumps(report, indent=2))
            return 0
        receipts = [json.loads(p.read_text()) for p in (root / "host-state/accounts-codex/renewal-follow").glob("*.json")]
        completed = [item for item in receipts if item.get("phase") == "consumed"]
        probe.require(len(completed) == (2 if args.chain else 1), "handoff did not consume exactly one receipt per account move")
        receipt = completed[0]
        if args.worktree:
            checkout = managed_checkout(root)
            probe.require(checkout == model.edited_checkout, "handoff changed the working checkout")
            probe.require((checkout / "tracked.txt").read_text() == "original task edits\n"
                          and (checkout / "unfinished.txt").read_text() == "untracked task work\n",
                          "handoff lost local task edits")
            owner_file = subprocess.run(["/usr/bin/git", "-C", str(checkout), "rev-parse", "--git-path", "codex-thread.json"],
                                        env=env, check=True, capture_output=True, text=True).stdout.strip()
            owner = json.loads((checkout / owner_file).read_text())
            probe.require(owner.get("ownerThreadId") == receipt["intent"]["thread"],
                          "managed checkout changed thread ownership")
            report["originalManagedCheckout"] = str(checkout)
        probe.require(all(item["old_pid"] == item["successor_pid"] == metadata["child_pid"] for item in completed),
                      "handoff replaced the caller's PID")
        probe.require(all(item["old_birth"] == item["successor_birth"] == receipt["old_birth"] for item in completed),
                      "handoff changed process birth")
        probe.require(all(item["intent"]["thread"] == receipt["intent"]["thread"]
                          and item["intent"]["wall"] == receipt["intent"]["wall"] for item in completed),
                      "repeated handoff changed the original turn")
        with model.lock:
            requests = list(model.requests)
            reads = list(model.usage_reads)
        expected_count = len(model.root_wait_calls) + len(model.child_status_calls) + 2 if args.child_quota else (4 if args.chain or args.child_followup else 3)
        if model.idle_child:
            expected_count = len(model.idle_child.calls) + 2
        probe.require(len(requests) == expected_count and requests[-1].get("newTokenMatched") is True,
                      "account handoff did not resume with the selected login")
        if args.change_model:
            probe.require(model.model_changed, "model config mutation was not reached")
            probe.require(all(item["body"]["model"] == (PENDING_MODEL if args.pending_mail else probe.MODEL) for item in requests),
                          "account handoff changed the original model after config mutation")
            for request in requests:
                rendered = json.dumps(request["body"])
                probe.require(ORIGINAL_INSTRUCTIONS in rendered,
                              "account handoff lost original developer instructions")
                probe.require(CHANGED_INSTRUCTIONS not in rendered,
                              "account handoff adopted changed developer instructions")
            report["originalModelRetained"] = True
            report["originalDeveloperInstructionsRetained"] = True
        if args.pending_mail:
            probe.require((root / "pending-mail-accepted.json").is_file(), "no proven child message acceptance")
            expected_child_requests = (2, 3) if args.active_child else ((3,) if args.child_followup else (2,))
            probe.require(len(model.child_requests) in expected_child_requests, "child request count changed")
            probe.require(PENDING_MAIL not in json.dumps(requests[1]["body"]["input"]),
                          "mail entered history before the quota wait")
            probe.require(json.dumps(requests[-1]["body"]["input"]).count(PENDING_MAIL) == 1,
                          "recovered request did not preserve queued mail exactly once")
            report["pendingMailRetained"] = True
            report["childRequestCount"] = len(model.child_requests)
        outputs = [item for item in requests[-1]["body"]["input"] if item.get("type") == "function_call_output"]
        if args.child_quota:
            wait_outputs = [item for item in outputs if item.get("call_id") in model.root_wait_calls]
            probe.require(all("aborted by user" not in str(item.get("output", "")) for item in wait_outputs),
                          "automatic account handoff was misreported as user cancellation")
        expected_calls = ({probe.CALL_ID, *model.root_wait_calls, *model.child_status_calls} if args.child_quota else
                          {probe.CALL_ID, FOLLOWUP_ID} if args.child_followup else {probe.CALL_ID})
        if model.idle_child:
            expected_calls = set(model.idle_child.calls)
        probe.require(len(outputs) == len(expected_calls) and {item.get("call_id") for item in outputs} == expected_calls,
                      "completed tool output was replayed or lost")
        probe.require({"source", "target"}.issubset(reads), "shared pool did not verify both accounts")
        if args.drop_consume_ack:
            probe.require((root / "consume-ack-dropped").exists(), "lost-ACK injection was not reached")
        report.update(originalTurnRecovered=True, originalPid=receipt["old_pid"], thread=receipt["intent"]["thread"],
                      turn=receipt["intent"]["wall"], requestCount=len(requests), eventTypes=types)
        if args.human:
            answer = "Native renewal fixture recovered."
            probe.require(sum(line.strip() == answer for line in text.splitlines()) == 1,
                          "human output lost or repeated the final answer")
            probe.require(not events, "human renderer unexpectedly emitted JSON events")
            report["humanOutputConsistent"] = True
        else:
            problems = stream_problems(events)
            report.update(jsonStreamConsistent=not problems, jsonStreamProblems=problems)
            probe.require(not problems, "handoff did not preserve JSON item identities and completion")
        if args.token_usage:
            completion = [event for event in events if event.get("type") == "turn.completed"]
            probe.require(len(completion) == 1, "token usage requires one completed turn")
            usage = completion[0].get("usage", {})
            report["reportedTokenUsage"] = usage
            expected = {"input_tokens": 30, "cached_input_tokens": 10,
                        "cache_write_input_tokens": 5, "output_tokens": 12,
                        "reasoning_output_tokens": 5}
            probe.require(all(usage.get(key) == value for key, value in expected.items()),
                          "replacement lost or duplicated original token usage")
            report["originalTokenUsageRetained"] = True
        if args.child_quota:
            probe.require((root / "child-quota-reached").is_file(), "child never reached quota")
            probe.require(len(model.child_requests) == 2, "child quota recovery repeated or lost model work")
            report["childQuotaRotatedTree"] = True
        if (args.child_followup or args.active_child or args.child_quota) and not args.ephemeral:
            from exec_child_continuity import verify
            verify(root, report, active=args.active_child or args.child_quota)
        if args.ephemeral:
            rollouts = list((root / "codex-home").rglob("rollout-*.jsonl"))
            probe.require(not rollouts, "ephemeral account handoff persisted a session rollout")
            report["ephemeralOriginalTurnRecovered"] = True
            if args.child_quota:
                from exec_child_continuity import verify_ephemeral
                verify_ephemeral(report, model.child_requests, receipt["intent"]["thread"], model.child_completed)
            if model.idle_child:
                model.idle_child.verify(report, receipt["intent"]["thread"])
        if model.review:
            model.review.verify(root, requests, report, receipt["intent"]["thread"], receipt["intent"]["wall"],
                                ephemeral=args.ephemeral, events=events)
        report["passed"] = True
    except BaseException as error:
        report["passed"] = False
        report["failure"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        if args.pending_mail:
            report["pendingMailAccepted"] = (root / "pending-mail-accepted.json").is_file()
            lost = root / "pending-mail-lost.json"
            if lost.is_file():
                report["pendingMailCountAtReplacement"] = json.loads(lost.read_text())["count"]
        server.shutdown()
        worker.join()
        probe.write_json(root / "result.json", report)
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
