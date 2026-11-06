#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Project an actual rollout onto the durable plan oracle's fields only."""
import hashlib
import json
from pathlib import Path
import sys


def project(events):
    result = []
    for event in events:
        payload = event.get("payload", {})
        kind = payload.get("type")
        if event.get("type") == "event_msg" and kind in ("task_started", "turn_started", "plan_update"):
            keys = ("type", "turn_id", "plan")
        elif event.get("type") == "response_item" and kind in (
                "function_call", "function_call_output", "custom_tool_call", "custom_tool_call_output",
                "tool_search_call", "tool_search_output"):
            keys = ("type", "name", "arguments", "call_id", "output")
        else:
            continue
        result.append({"type": event["type"], "payload": {
            key: payload[key] for key in keys if key in payload}})
    return result


if __name__ == "__main__":
    source = Path(sys.argv[1])
    events = [json.loads(line) for line in source.read_text().splitlines()]
    root = Path(__file__).resolve().parent
    (root / "rollout.json").write_text(json.dumps(project(events), indent=2) + "\n")
    (root / "provenance.json").write_text(json.dumps({
        "kind": "actual-rollout-durable-plan-projection",
        "case": "native-renewal-143d-f519-happy-h1",
        "source": source.name,
        "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "codexSourceCommit": "143dca0fa4d625ebf97a38204e839bb1a23c65fd",
        "controllerSourceCommit": "a4e1d0712e1292ff3b0d845d55cdc38891a841fc",
        "projection": "Only event/response type, original turn_id, tool name/arguments/call_id/output and unexpected plan UI payload. All prose, credentials, environment and private paths omitted."
    }, indent=2) + "\n")
