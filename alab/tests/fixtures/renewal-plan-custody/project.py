#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Project an actual rollout onto the durable plan oracle's fields only."""
import hashlib
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "smoke"))
from provider_fixture import TOOL_CALL_TYPES, TOOL_OUTPUT_TYPES


def project(events):
    result = []
    for event in events:
        payload = event.get("payload", {})
        kind = payload.get("type")
        if event.get("type") == "event_msg" and kind in (
                "task_started", "turn_started", "task_complete", "turn_complete", "turn_aborted", "plan_update"):
            keys = ("type", "turn_id", "plan")
        elif event.get("type") == "response_item" and isinstance(kind, str) and (
                kind in TOOL_CALL_TYPES or kind in TOOL_OUTPUT_TYPES):
            keys = ("type", "name", "arguments", "call_id", "output")
        else:
            continue
        projected = {key: payload[key] for key in keys if key in payload}
        if event["type"] == "response_item":
            metadata = payload.get("internal_chat_message_metadata_passthrough")
            if metadata is not None:
                if not isinstance(metadata, dict):
                    raise ValueError("invalid tool metadata")
                if (turn := metadata.get("turn_id")) is not None:
                    if not isinstance(turn, str):
                        raise ValueError("invalid tool turn ID")
                    projected["internal_chat_message_metadata_passthrough"] = {"turn_id": turn}
        result.append({"type": event["type"], "payload": projected})
    return result


if __name__ == "__main__":
    source = Path(sys.argv[1])
    with source.open("rb") as stream:
        events = project(json.loads(line.decode("utf-8")) for line in stream)
    root = Path(__file__).resolve().parent
    (root / "rollout.json").write_text(json.dumps(events, indent=2) + "\n")
    (root / "provenance.json").write_text(json.dumps({
        "kind": "actual-rollout-durable-plan-projection",
        "case": "native-renewal-143d-f519-happy-h1",
        "source": source.name,
        "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "codexSourceCommit": "143dca0fa4d625ebf97a38204e839bb1a23c65fd",
        "controllerSourceCommit": "a4e1d0712e1292ff3b0d845d55cdc38891a841fc",
        "projection": "Only event/response type, original turn_id, tool name/arguments/call_id/output and unexpected plan UI payload. All prose, credentials, environment and private paths omitted."
    }, indent=2) + "\n")
