# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check child lifecycle evidence; parent success alone proves no child continuity."""
import json
import time

import capacity_exec as probe


def verify_ephemeral(report, requests, parent, completed):
    """Use provider identity and a live runtime status, without a rollout file."""
    identities = [request.get("requestIdentity", {}) for request in requests]
    probe.require(len(identities) == 2 and identities[0] == identities[1]
                  and all(identities[0].get(key) for key in ("thread_id", "turn_id", "parent_thread_id")),
                  "ephemeral child did not retain its original request identity")
    identity = identities[0]
    probe.require(identity["parent_thread_id"] == parent and identity["thread_id"] != parent
                  and completed, "original ephemeral child did not finish under its parent")
    report["ephemeralChildContinuity"] = {"identity": identity, "completed": True}


def wait_for_original_turn_end(root, timeout=30):
    """Keep the parent alive until the original child ends, successfully or not.

    Failure remains the final oracle's responsibility. Waiting only for a new
    request or an HTTP response would let parent shutdown abort a resumed child.
    """
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        for path in (root / "codex-home").rglob("rollout-*.jsonl"):
            # A concurrent append may expose an incomplete trailing record.
            rows = [json.loads(line) for line in path.read_bytes().splitlines(keepends=True)
                    if line.endswith(b"\n")]
            metadata = [row["payload"] for row in rows if row.get("type") == "session_meta"]
            if not metadata or metadata[0].get("agent_path") != "/root/pending_mail":
                continue
            events = [row["payload"] for row in rows if row.get("type") == "event_msg"]
            started = next((event.get("turn_id") for event in events
                            if event.get("type") == "task_started"), None)
            if started is not None and any(
                    event.get("type") in ("task_complete", "turn_aborted")
                    and event.get("turn_id") in (None, started) for event in events):
                return
        time.sleep(.01)
    raise RuntimeError("original child did not reach a terminal state while the parent stayed alive")


def verify(root, report, *, active):
    children = []
    for path in (root / "codex-home").rglob("rollout-*.jsonl"):
        rows = [json.loads(line) for line in path.read_text().splitlines()]
        metadata = [row["payload"] for row in rows if row.get("type") == "session_meta"]
        if not metadata or metadata[0].get("agent_path") != "/root/pending_mail":
            continue
        events = [row["payload"] for row in rows if row.get("type") == "event_msg"
                  and row.get("payload", {}).get("type") in ("task_started", "task_complete", "turn_aborted", "error")]
        children.append({"child": metadata[0]["id"], "events": events})
    evidence = {"active": active, "children": children,
                "heldInFlight": (root / "child-held-in-flight").is_file()}
    report["childContinuity"] = evidence
    probe.write_json(root / "child-continuity.json", evidence)
    probe.require(len(children) == 1, "expected exactly one persisted child identity")
    events = children[0]["events"]
    starts = [event["turn_id"] for event in events if event["type"] == "task_started"]
    completions = [event["turn_id"] for event in events if event["type"] == "task_complete"]
    probe.require(starts, "child never started")
    probe.require(not any(event["type"] == "turn_aborted" for event in events),
                  "account rotation aborted the child's original turn")
    probe.require(all(event["type"] != "error" and event.get("error") is None for event in events),
                  "child completed with an error")
    if active:
        probe.require(evidence["heldInFlight"], "child was not held in flight across rotation")
        probe.require(set(starts) == {starts[0]} and completions == [starts[0]],
                      "active child did not complete its original turn exactly once")
    else:
        probe.require(len(starts) == 2 and starts[0] != starts[1] and completions == starts,
                      "same child did not complete both original and followup turns")
        probe.require((root / "restored-child-request.json").is_file(),
                      "child never requested with the selected account")
    evidence["passed"] = True
