# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Retry a still-open conversation in the same bare resume picker after its original writer exits."""

import argparse
import hashlib
import json
from pathlib import Path
import secrets
import sys
import time
import xml.etree.ElementTree as ET

import account_resume as account
from account_resume_moved import histories
import capacity_exec as fixture
import model_resume as terminal
from picker_screen import SYNC_START, visible_lines
from quota_resume import Rpc
from terminal_controls import drive_pty


CONFLICT = "This conversation is open in another app"
RETRY = "Close it there and press R to continue here."


def message(role, text):
    return {"role": role, "content": [{"type": "input_text" if role == "user" else "output_text", "text": text}]}


def saved_conversation(prefix, work):
    records = (json.loads(line) for line in prefix.splitlines())
    saved = account.conversation_input(row["payload"] for row in records if row["type"] == "response_item")
    # The CLI records its initial environment as a user message. Bind its complete content
    # to A's original rollout before opening B, including the fixture's physical workspace.
    fixture.require(len(saved) == 3 and saved[0]["role"] == "user"
                    and len(saved[0]["content"]) == 1 and saved[0]["content"][0]["type"] == "input_text"
                    and saved[1:] == [message("user", terminal.SEED), message("assistant", terminal.REPLY)],
                    "unexpected user or assistant content in the original rollout")
    context = ET.fromstring(saved[0]["content"][0]["text"])
    fixture.require(context.tag == "environment_context" and context.findtext("cwd") == str(work),
                    "the original context did not name the source workspace")
    return saved


def check_conversation_input(saved, first, original, resumed, followup):
    continued = [*saved, message("user", terminal.CONTINUE)]
    fixture.require(first["conversation_input"] == original["conversation_input"] == saved[:-1],
                    "unexpected user or assistant input on the source turn")
    fixture.require(resumed["conversation_input"] == followup["conversation_input"] == continued,
                    "unexpected user or assistant input on the resumed turn")


def retry_in_picker(binary, root, env, gateway, report, release_owner, rollout, marker):
    def interact(pty):
        deadline = time.monotonic() + 55

        def frame(label, matches, keys=None, *, fresh=False):
            offset = pty.raw.stat().st_size if keys else 0
            if keys:
                pty.send(keys)
            while pty.process.poll() is None and time.monotonic() < deadline:
                raw = pty.raw.read_bytes() if pty.raw.exists() else b""
                if fresh:
                    start = raw.find(SYNC_START, offset)
                    lines = visible_lines(raw[start:]) if start >= 0 else []
                else:
                    lines = visible_lines(raw, after=offset)
                if lines and matches(raw, "\n".join(lines)):
                    report["frames"][label] = [line.rstrip() for line in lines if line.strip()]
                    return
                time.sleep(.05)
            raise RuntimeError(f"bare resume did not display {label}")

        frame("initial picker", lambda raw, _: terminal.bare_picker_rows(raw, expected=["cedar"]) == ["cedar"])
        frame("directory choice", lambda _, view: "Use session directory" in view, b"\r")

        def read_only(_, view):
            return all(text in view for text in (CONFLICT, RETRY, terminal.SEED, terminal.REPLY, marker))

        frame("active writer", read_only, b"\r")
        # Require a fresh replay after r. An unchanged lock screen or an ignored key cannot pass.
        frame("retry while still owned", read_only, b"r", fresh=True)
        # Try real text and Enter. Ctrl-T then shows that the locked view still owns the input loop.
        pty.send(b"z\r")
        command = json.loads(account.TOOL_CALL["arguments"])["cmd"]
        frame("locked transcript", lambda raw, view: read_only(raw, view) and f"$ {command}" in view, b"\x14")
        frame("back from transcript", lambda raw, view: read_only(raw, view) and f"Ran {command}" in view
              and f"$ {command}" not in view, b"\x14")
        release_owner()
        frame("conversation after retry", lambda _, view: CONFLICT not in view and RETRY not in view
              and terminal.REPLY in view, b"r")
        pty.paste(terminal.CONTINUE, terminal.CONTINUE)
        frame("continued reply", lambda _, view: terminal.ANSWER in view, b"\r")
        report["destination_turn"] = terminal.wait_for_turn_completion(
            root, gateway, pty.process, terminal.ANSWER, deadline, rollout_path=rollout)
        pty.quit()
        report["pty_commands"] = pty.index

    drive_pty([str(binary), "resume"], root, env, "retry", interact, cwd=root / "second-project")
    # One forkpty child receives every key, including the retry. Its owning guardian records the PID and reap.
    events = [json.loads(line) for line in (root / "retry-stdout.jsonl").read_bytes().splitlines()]
    started, = [e["started"] for e in events if "started" in e]
    custody = json.loads((root / "retry-custody.json").read_bytes())
    fixture.require(type(started) is int and started > 0 and events[0] == {"started": started}
                    and events[-1] == {"completed": custody}, "unexpected PTY lifecycle")
    fixture.require([e["ack"] for e in events if "ack" in e] == list(range(1, report["pty_commands"] + 1)),
                    "the same PTY did not acknowledge every command")
    report.update(pty_pid=started, custody=custody, raw_sha256=terminal.CHECK["sha256"](root / "retry.raw"))


def exercise(binary, seed, root, port, gateway, report):
    env, slots = account.configure_accounts(root, port, gateway)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm")
    # Vanilla reads the default auth file. Keep A there while B explicitly opens its own file.
    shared = root / "codex-home/auth.json"
    slots["source"].rename(shared)
    slots["source"] = shared
    fingerprints = {login: terminal.CHECK["sha256"](path) for login, path in slots.items()}
    work, other = root / "work", root / "second-project"
    other.mkdir()
    with (root / "codex-home/config.toml").open("a") as config:
        config.write(f'\n[projects.{json.dumps(str(other))}]\ntrust_level = "trusted"\n')
    marker = "cedar-" + secrets.token_hex(12)
    (work / "cedar-marker.txt").write_text(marker + "\n")

    def seed_original(owner, output):
        rpc = Rpc(owner, output)
        rpc.initialize()
        profile = rpc.request("account/read", {"refreshToken": False})
        fixture.require(profile["account"]["email"] == "source@example.invalid", "wrong original account")
        thread = rpc.request("thread/start", {"cwd": str(work), "ephemeral": False})["thread"]["id"]
        turn = rpc.turn(thread, terminal.SEED)
        fixture.require(turn["status"] == "completed", "original turn never became idle")
        report.update(thread=thread, source_turn=turn["id"])
        rollout, = (root / "codex-home/sessions").rglob(f"rollout-*-{thread}.jsonl")
        prefix = rollout.read_bytes()
        report["saved_conversation"] = saved_conversation(prefix, work)

        def original_turns():
            saved = rpc.request("thread/read", {"threadId": thread, "includeTurns": True})["thread"]
            fixture.require(saved["status"]["type"] == "idle" and saved["id"] == thread
                            and [t["id"] for t in saved["turns"]] == [turn["id"]]
                            and saved["turns"][0]["status"] == "completed", "the idle original changed")
            return saved["turns"]

        saved_turns = original_turns()
        with gateway.lock:
            report["seed_requests"] = len(gateway.requests)
            gateway.resumed = True

        def release_owner():
            # This is the connection that started the thread. It must still own its live Core at B's conflict.
            fixture.require(rpc.request("thread/loaded/list", {})["data"] == [thread], "A no longer has its thread")
            with gateway.lock:
                fixture.require(len(gateway.requests) == report["seed_requests"], "the locked picker submitted inference")
            fixture.require(original_turns() == saved_turns, "the early retry changed A's original turn")
            fixture.require(histories(root / "codex-home") == {rollout: prefix},
                            "the still-locked retry forked, archived or changed A's rollout")
            report["locked_thread"] = {"id": thread, "turn": turn["id"], "status": "idle", "rollouts": 1,
                                       "bytes": len(prefix), "sha256": hashlib.sha256(prefix).hexdigest()}
            owner.stdin.close()
            owner.stdin = None
            fixture.require(owner.wait(timeout=15) == 0, "A did not exit normally on EOF")
            receipt = json.loads((root / "original-guardian-result.json").read_bytes())
            fixture.require(receipt["returncode"] == 0 and receipt["timed_out"] is False, "A's guardian timed out")
            report.update(retry_after_owner_exit=True, open_writer_inference_requests=0)

        retry_in_picker(binary, root, {**env, "CODEX_AUTH_PATH": str(slots["destination"])}, gateway,
                        report, release_owner, rollout, marker)
        final = rollout.read_bytes()
        fixture.require(final.startswith(prefix) and len(final) > len(prefix), "original rollout prefix changed")
        report.update(preserved_bytes=len(prefix), prefix_sha256=hashlib.sha256(prefix).hexdigest())

    fixture.require(fixture.run(seed, ["app-server", "--listen", "stdio://"], root, env, "original",
                                cwd=work, interact=seed_original) == 0, "original app-server failed")
    fixture.require(fingerprints == {login: terminal.CHECK["sha256"](path) for login, path in slots.items()},
                    "a login file changed during retry")
    return marker


def check_requests(root, report, marker):
    rows, count = report["requests"], report["seed_requests"]
    # Vanilla can request an automatic title on a different thread.
    source = [row for row in rows[:count] if row["thread"] == report["thread"] or row["history"]]
    fixture.require(len(source) == 2, "unexpected request count on the original thread")
    first, original = source
    resumed, followup = rows[count:]
    for phase, login, requests in (("seed", "source", rows[:count]), ("resume", "destination", rows[count:])):
        reads = [row for row in report["account_reads"] if row["phase"] == phase]
        fixture.require({r["path"] for r in reads} == {"/v1/models", "/backend-api/wham/accounts/check"}
                        and all((r["login"], r["account"]) == (login, "workspace-" + login) for r in [*reads, *requests]),
                        f"{phase} model or account requests used the wrong credentials")
    fixture.require(report["source_turn"] != report["destination_turn"] and
                    all((row["thread"], row["turn"]) == (report["thread"], report[expected])
                        for row, expected in ((first, "source_turn"), (original, "source_turn"),
                                              (resumed, "destination_turn"), (followup, "destination_turn"))),
                    "the original thread or its two turn IDs changed")
    check_conversation_input(report["saved_conversation"], first, original, resumed, followup)
    fixture.require(first["history"] == [terminal.SEED] and original["history"] == [
                        terminal.SEED, "function_call", "function_call_output"] and
                    resumed["history"] == [*original["history"], terminal.REPLY, terminal.CONTINUE]
                    and resumed["tool_records"] == original["tool_records"], "destination lost original work")
    call, result = original["tool_records"]
    fixture.require(all(call.get(k) == v for k, v in account.TOOL_CALL.items()) and result["call_id"] == call["call_id"]
                    and marker in result["output"] and (root / "work/cedar-calls").read_text().splitlines() == [marker],
                    "original command or its result changed")
    report["resumed_directory"] = account.check_resumed_directory(root / "work", root / "second-project", marker,
                                                                 resumed, followup)


def run(binary, seed, root):
    root.mkdir(mode=0o700)
    report = {"passed": False, "frames": {}, "events": [], "requests": [],
              "binary_sha256": terminal.CHECK["sha256"](binary), "seed_binary_sha256": terminal.CHECK["sha256"](seed)}
    gateway = account.Gateway(report)
    gateway.verify_resume_directory = True
    gateway.capture_conversation_input = True
    try:
        server, worker = account.start_gateway(root, gateway)
        try:
            marker = exercise(binary, seed, root, server.server_port, gateway, report)
        finally:
            fixture.cleanup_resources((("gateway", server, worker),), (), report, sys.exc_info()[1])
        fixture.require(not report.get("cleanupErrors"), "loopback gateway failed to retire")
        check_requests(root, report, marker)
        fixture.require(terminal.CHECK["sha256"](binary) == report["binary_sha256"]
                        and terminal.CHECK["sha256"](seed) == report["seed_binary_sha256"], "executable changed")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        fixture.write_json(root / "result.json", report)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", required=True, type=Path, help="Candidate CLI for B's bare picker")
    parser.add_argument("--seed-codex", type=Path, help="CLI that creates and keeps A's original thread open (default: --codex)")
    parser.add_argument("--output", required=True, type=Path, help="New directory for private fixture data and result.json")
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), (args.seed_codex or args.codex).resolve(strict=True),
                 args.output.parent.resolve(strict=True) / args.output.name))
