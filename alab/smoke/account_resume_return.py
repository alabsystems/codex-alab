# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Return to A's original thread after B has already continued it from the bare picker."""

import json
import time

import account_resume_moved
import capacity_exec as fixture
import model_resume as terminal
from terminal_controls import drive_pty


def exercise(binary, root, env, gateway, thread, sibling):
    home = root / "codex-home"
    before = account_resume_moved.histories(home)
    rollout, = [p for p in before if p.name.endswith(f"-{thread}.jsonl")]
    fixture.require(len(before) == 2 and any(p.name.endswith(f"-{sibling}.jsonl") for p in before),
                    "returning to A requires both original conversations")
    with gateway.lock:
        requests_before = len(gateway.requests)
    launch_dir = root / "return-project"
    launch_dir.mkdir()
    evidence = {}

    def interact(pty):
        deadline = time.monotonic() + 55
        while pty.process.poll() is None and time.monotonic() < deadline:
            raw = pty.raw.read_bytes() if pty.raw.is_file() else b""
            rows = terminal.bare_picker_rows(raw)
            if rows is not None:
                break
            time.sleep(.05)
        else:
            raise RuntimeError("returning A did not see both conversations in the default picker")
        evidence["initial_rows"] = rows
        if rows[0] != "cedar":
            pty.send(b"\x1b[B")
        pty.send(b"\r")
        pty.displayed("Use session directory")
        pty.send(b"\r")
        pty.displayed(terminal.ANSWER)
        with gateway.lock:
            fixture.require(len(gateway.requests) == requests_before, "selecting the conversation ran inference")
        pty.paste(terminal.AFTER_RELEASE, terminal.AFTER_RELEASE)
        pty.send(b"\r")
        pty.displayed(terminal.FINAL_ANSWER)
        evidence["turn"] = terminal.wait_for_turn_completion(
            root, gateway, pty.process, terminal.FINAL_ANSWER, deadline, rollout_path=rollout)
        pty.quit()

    drive_pty([str(binary), "resume"], root, env, "return-to-source", interact, cwd=launch_dir)
    after = account_resume_moved.histories(home)
    fixture.require(after.keys() == before.keys() and after[rollout].startswith(before[rollout])
                    and len(after[rollout]) > len(before[rollout])
                    and all(after[p] == before[p] for p in before if p != rollout),
                    "returning A changed earlier history or the sibling conversation")
    contexts = [item["payload"].get("cwd") for item in map(json.loads, after[rollout].splitlines())
                if item.get("type") == "turn_context" and item["payload"].get("turn_id") == evidence["turn"]]
    fixture.require(len(set(contexts)) == 1, "returning turn lacks a single recorded project directory")
    evidence.update(thread=thread, directory=contexts[0], preserved_history_bytes=len(before[rollout]))
    return evidence


def check_request(requests, reads, evidence, previous_turns, last_request, directory):
    request, = requests
    fixture.require((request["login"], request["account"]) == ("source", "workspace-source")
                    and all((r["login"], r["account"]) == ("source", "workspace-source") for r in reads)
                    and {r["path"] for r in reads} == {"/v1/models", "/backend-api/wham/accounts/check"},
                    "the returning turn, catalog or account read used the wrong login")
    fixture.require(request["thread"] == evidence["thread"] and request["turn"] == evidence["turn"]
                    and request["turn"] not in previous_turns, "returning A did not continue the same thread in a new turn")
    fixture.require(request["history"] == [*last_request["history"], terminal.ANSWER, terminal.AFTER_RELEASE]
                    and request["tool_records"] == last_request["tool_records"],
                    "returning A's request lost or changed a prior message, tool call or tool output")
    expected_messages = [
        *last_request["conversation_input"],
        {"role": "assistant", "content": [{"type": "output_text", "text": terminal.ANSWER}]},
        {"role": "user", "content": [{"type": "input_text", "text": terminal.AFTER_RELEASE}]},
    ]
    fixture.require(request["conversation_input"] == expected_messages,
                    "the return request added, lost or reordered sender content")
    fixture.require(evidence["directory"] == str(directory), "returning A used a different project")
