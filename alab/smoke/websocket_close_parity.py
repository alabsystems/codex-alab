#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Compare Responses transport selection on a resumed turn. Requires Python websockets."""

import argparse
import json
from http.server import BaseHTTPRequestHandler
from pathlib import Path
from uuid import UUID
import sys
import threading

import capacity_exec as fixture
from request_recovery import digest


SEED = "Remember the blue notebook."
SEED_ANSWER = "The blue notebook is saved."
RESUME = "Which notebook did I ask you to remember?"
ANSWER = "You asked about the blue notebook."
TRANSCRIPT = [("user", SEED), ("assistant", SEED_ANSWER), ("user", RESUME)]
CASES = {
    "size-before-events": (1009, False, ["WS", "POST"]),
    "ordinary-before-events": (1001, False, ["WS", "WS"]),
    "size-after-created": (1009, True, ["WS", "WS"]),
}


def frames(response_id, answer=None):
    response = {"id": response_id}
    events = [{"type": "response.created", "response": response}]
    if answer:
        events.append({"type": "response.output_item.done", "item": {
            "type": "message", "id": "msg-" + response_id, "role": "assistant",
            "content": [{"type": "output_text", "text": answer}]}})
    events.append({"type": "response.completed", "response": response})
    return events


def dialogue(items):
    return [(item["role"], part["text"]) for item in items if item.get("role") in ("user", "assistant")
            for part in item.get("content", []) if isinstance(part, dict) and "text" in part
            and not (item["role"] == "user" and part["text"].lstrip().startswith("<environment_context>"))]


class Provider:
    def __init__(self, case):
        self.case = case
        self.events = []
        self.connections = 0
        self.lock = threading.Lock()

    def respond(self, transport, path, body, connection=None):
        items = body.get("input", [])
        warmup = body.get("generate") is False
        with self.lock:
            attempt = {"transport": transport, "path": path, "connection": connection, "model": body.get("model"),
                       "warmup": warmup, "input": items, "previous_response_id": body.get("previous_response_id")}
            self.events.append(attempt)
            if warmup:
                attempt["response_id"] = f"warmup-{len(self.events)}"
                return frames(attempt["response_id"]), None
            if dialogue(items) == [("user", SEED)]:
                return frames("seed", SEED_ANSWER), None
            previous = any("close" in row for row in self.events[:-1])
            if transport == "WS" and dialogue(items) == TRANSCRIPT and not previous:
                code, after_created, _ = CASES[self.case]
                attempt.update(close=code, sent_created=after_created)
                sent = [{"type": "response.created", "response": {"id": "interrupted"}}] if after_created else []
                return sent, code
            return frames("continued", ANSWER), None


def handler(provider):
    from websockets.datastructures import Headers
    from websockets.frames import Opcode
    from websockets.http11 import Request
    from websockets.server import ServerProtocol

    class Endpoint(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            if self.path != "/v1/responses":
                with provider.lock:
                    provider.events.append({"unexpected_post": self.path})
                self.send_error(404)
                return
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            events, _ = provider.respond("POST", self.path, body)
            data = fixture.sse(events)
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            if self.path != "/v1/responses" or self.headers.get("Upgrade", "").lower() != "websocket":
                self.send_error(404)
                return
            # Retain one loopback address for real HTTP and RFC 6455, as a Responses provider does.
            protocol = ServerProtocol()
            protocol.receive_data(Request(self.path, Headers(self.headers.items())).serialize())
            opening, = protocol.events_received()
            response = protocol.accept(opening)
            protocol.send_response(response)
            self.wfile.writelines(protocol.data_to_send())
            if response.status_code != 101:
                return
            with provider.lock:
                provider.connections += 1
                connection = provider.connections
            fragments = bytearray()
            while chunk := self.rfile.read1(65536):
                protocol.receive_data(chunk)
                for frame in protocol.events_received():
                    if frame.opcode not in (Opcode.TEXT, Opcode.CONT):
                        continue
                    fragments.extend(frame.data)
                    if not frame.fin:
                        continue
                    body = json.loads(fragments)
                    fragments.clear()
                    if body.get("type") != "response.create":
                        protocol.send_close(1003)
                        self.wfile.writelines(protocol.data_to_send())
                        return
                    events, close = provider.respond("WS", self.path, body, connection)
                    for event in events:
                        protocol.send_text(json.dumps(event).encode())
                    if close:
                        protocol.send_close(close)
                        self.wfile.writelines(protocol.data_to_send())
                        return
                self.wfile.writelines(protocol.data_to_send())

    return Endpoint


def exec_turn(binary, args, root, env, label, answer):
    status = fixture.run(binary, args, root, env, label, timeout_seconds=25)
    custody = json.loads((root / f"{label}-guardian-result.json").read_text())
    fixture.require(status == 0 and custody["argv"] == [str(binary), *args]
                    and custody["returncode"] == 0 and not custody["timed_out"], f"{label} CLI exit")
    events = [json.loads(line) for line in (root / f"{label}-stdout.jsonl").read_bytes().splitlines()]
    starts = [e["thread_id"] for e in events if e["type"] == "thread.started"]
    answers = [e["item"]["text"] for e in events if e["type"] == "item.completed"
               and e.get("item", {}).get("type") == "agent_message"]
    lifecycle = [e["type"] for e in events if e["type"] in ("turn.started", "turn.failed", "turn.completed")]
    fixture.require(len(starts) == 1 and answers == [answer] and lifecycle == ["turn.started", "turn.completed"],
                    f"{label} must complete one turn with its expected answer")
    fixture.require(str(UUID(starts[0])) == starts[0], "CLI did not report a thread UUID")
    return starts[0]


def check(case, events):
    code, after_created, transports = CASES[case]
    fixture.require(not any("unexpected_post" in row for row in events), f"{case} POST to an unexpected path")
    requests = [row for row in events if not row["warmup"]]
    fixture.require([row["transport"] for row in requests] == ["WS", *transports],
                    f"{case} transport sequence, including seed")
    fixture.require(all(row["path"] == "/v1/responses" and row["model"] == fixture.MODEL for row in events),
                    f"{case} Responses path and model")
    fixture.require([dialogue(row["input"]) for row in requests] == [[("user", SEED)], TRANSCRIPT, TRANSCRIPT],
                    f"{case} must preserve the exact resumed dialogue on retry")
    # A response.create can reference an acknowledged prewarm and send only the new items.
    # A retry on a fresh connection resends the full input, including the original developer item.
    warmups = {}
    inputs = []
    for row in events:
        previous = row["previous_response_id"]
        key = (row["connection"], previous)
        fixture.require(previous is None or key in warmups, f"{case} references an unknown response on this connection")
        effective = [*warmups.get(key, []), *row["input"]]
        if row["warmup"]:
            response_id = row["response_id"]
            fixture.require((row["connection"], response_id) not in warmups, f"{case} reused a warmup response ID")
            warmups[row["connection"], response_id] = effective
        else:
            inputs.append(effective)
    if transports[-1] == "WS":
        fixture.require(requests[1]["connection"] != requests[2]["connection"],
                        f"{case} did not reconnect after close")
    fixture.require(inputs[1] == inputs[2], f"{case} changed a resumed input item during retry")
    fixture.require([(row.get("close"), row.get("sent_created")) for row in requests]
                    == [(None, None), (code, after_created), (None, None)], f"{case} close boundary")


def run_case(binary, case, root):
    root.mkdir()
    env = fixture.fresh_environment(root)
    provider = Provider(case)
    listener = fixture.start_listener(handler(provider))
    result = {"case": case, "passed": False}
    try:
        fixture.configure(root, listener[0].server_port, stream_max_retries=1, supports_websockets=True)
        common = ["--json", "--skip-git-repo-check"]
        thread = exec_turn(binary, ["exec", *common, SEED], root, env, "seed", SEED_ANSWER)
        resumed = exec_turn(binary, ["exec", "resume", *common, thread, RESUME], root, env, "resume", ANSWER)
        fixture.require(thread == resumed, "exec resumed a different thread")
        check(case, provider.events)
        result.update(passed=True, thread=thread)
    except Exception as error:
        result.update(fixture.case_error_fields(error))
    finally:
        fixture.cleanup_resources((("provider", *listener),),
                                  (("events", lambda: fixture.write_json(root / "events.json", provider.events)),),
                                  result, sys.exc_info()[1])
        result["evidence_sha256"] = {path.name: digest(path) for path in (root / "events.json",
                                       *root.glob("*-stdout.jsonl"), *root.glob("*-guardian-result.json"))
                                     if path.is_file()}
        fixture.write_json(root / "result.json", result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vendor", type=Path, required=True)
    parser.add_argument("--alab", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.vendor.samefile(args.alab):
        parser.error("comparison requires separate vendor and ALab executable files")
    output = args.output.resolve()
    output.mkdir(mode=0o700, parents=True)
    report = {}
    for name, executable in (("vendor", args.vendor), ("alab", args.alab)):
        binary = executable.resolve(strict=True)
        before = digest(binary)
        root = output / name
        root.mkdir()
        cases = [run_case(binary, case, root / case) for case in CASES]
        fixture.require(digest(binary) == before, f"{name} executable changed")
        report[name] = {"binary": str(binary), "sha256": before, "cases": cases}
    fixture.write_json(output / "comparison.json", report)
    return 0 if all(row["passed"] for observation in report.values() for row in observation["cases"]) else 1


if __name__ == "__main__":
    sys.exit(main())
