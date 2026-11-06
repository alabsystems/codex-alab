#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise quota recovery across real persisted thread origins; no account access.

Builds nothing. Requires the codex and standalone codex-app-server executables
from the same tree. All inputs, requests and command results remain in --output.
"""

import argparse
import json
import os
from pathlib import Path
import select
import sys
import time

import capacity_exec as probe

SEED = "Create a persisted thread for quota recovery."


class Rpc:
    def __init__(self, process, output):
        self.process = process
        self.output = output
        self.pending = []
        self.buffer = b""
        self.request_id = 0

    def send(self, value):
        self.process.stdin.write((json.dumps(value) + "\n").encode())
        self.process.stdin.flush()

    def wait(self, matches):
        end = time.monotonic() + 60
        while True:
            for index, value in enumerate(self.pending):
                if matches(value):
                    return self.pending.pop(index)
            remaining = end - time.monotonic()
            probe.require(remaining > 0, "app-server protocol deadline expired")
            ready, _, _ = select.select([self.process.stdout], [], [], min(remaining, 1))
            if not ready:
                continue
            data = os.read(self.process.stdout.fileno(), 65536)
            probe.require(bool(data), "app-server exited before its protocol reply")
            self.output.write(data)
            self.output.flush()
            self.buffer += data
            probe.require(len(self.buffer) <= 8 * 1024 * 1024, "oversized app-server frame")
            while b"\n" in self.buffer:
                line, self.buffer = self.buffer.split(b"\n", 1)
                if line.strip():
                    self.pending.append(json.loads(line))
            probe.require(len(self.pending) <= 10000, "unbounded app-server notifications")

    def request(self, method, params):
        self.request_id += 1
        request_id = self.request_id
        self.send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        reply = self.wait(lambda value: value.get("id") == request_id
                          and ("result" in value or "error" in value))
        probe.require("error" not in reply, f"{method} failed: {reply.get('error')}")
        return reply["result"]

    def initialize(self):
        self.request("initialize", {
            "clientInfo": {"name": "quota_resume_probe", "version": "1"},
            "capabilities": {"experimentalApi": True},
        })
        self.send({"jsonrpc": "2.0", "method": "initialized"})

    def turn(self, thread_id, prompt):
        self.request("turn/start", {
            "threadId": thread_id,
            "input": [{"type": "text", "text": prompt, "text_elements": []}],
        })
        return self.wait(lambda value: value.get("method") == "turn/completed")["params"]["turn"]


class ResumeMock(probe.Mock):
    def __init__(self, unknown_reset):
        super().__init__(False, quota=True, unknown_reset=unknown_reset)
        self.seed_requests = []
        self.seeding = True

    def reply(self, body):
        with self.lock:
            seeding = self.seeding
            if seeding:
                record = {"at": time.monotonic(), "unixAt": time.time(), "body": body}
                self.seed_requests.append(record)
        if not seeding:
            return super().reply(body)
        item = {"type": "message", "role": "assistant", "id": "seed-message",
                "content": [{"type": "output_text", "text": "Seed persisted."}]}
        return 200, probe.response("seed-response", item), "text/event-stream", None, record


def server_args(source):
    return ["--session-source", source, "--listen", "stdio://"]


def case(codex, app_server, output, origin, *, non_exec=False, unknown_reset=False):
    name = f"resume-{origin}-{'non-exec' if non_exec else 'exec'}"
    root = output / name
    root.mkdir()
    env = probe.fresh_environment(root)
    mock = ResumeMock(unknown_reset)
    server, thread = probe.serve(mock)
    saved = {}
    try:
        probe.configure(root, server.server_port)

        def seed(process, record):
            rpc = Rpc(process, record)
            rpc.initialize()
            response = rpc.request("thread/start", {
                "cwd": str(root / "work"), "model": probe.MODEL,
                "modelProvider": "capacity-smoke", "ephemeral": False,
            })
            saved.update(id=response["thread"]["id"], source=response["thread"]["source"])
            probe.require(saved["source"] == origin, "seed used the wrong native source")
            result = rpc.turn(saved["id"], SEED)
            probe.require(result["status"] == "completed", "seed turn did not complete")

        probe.require(probe.run(app_server, server_args(origin), root, env, "seed", interact=seed) == 0,
                      "seeding app-server did not shut down cleanly")
        probe.write_json(root / "seed-thread.json", saved)
        with mock.lock:
            mock.seeding = False

        if non_exec:
            def resume_interactive(process, record):
                rpc = Rpc(process, record)
                rpc.initialize()
                response = rpc.request("thread/resume", {"threadId": saved["id"]})
                probe.require(response["thread"]["source"] == origin, "saved origin was rewritten")
                turn = rpc.turn(saved["id"], probe.PROMPT)
                probe.require(turn["status"] == "failed", "non-exec quota control unexpectedly recovered")

            # The bundled command is ALab-active but has a non-exec host. This
            # control must not pass merely because a standalone server is inactive.
            code = probe.run(codex, ["app-server", "--listen", "stdio://"], root, env, "resume",
                             interact=resume_interactive)
            probe.require(code == 0 and len(mock.requests) == 2,
                          "non-exec quota control retried or exited abnormally")
        else:
            code = probe.run(codex, ["exec", "--json", "--color", "never",
                                    "--skip-git-repo-check", "--cd", str(root / "work"),
                                    "--sandbox", "read-only", "resume", saved["id"], probe.PROMPT],
                             root, env, "resume")
            events = [json.loads(line) for line in (root / "resume-stdout.jsonl").read_text().splitlines()]
            types = [event.get("type") for event in events]
            probe.require(code == 0 and len(mock.requests) == 3, "resumed exec did not recover quota")
            probe.require(types.count("turn.started") == types.count("turn.completed") == 1
                          and "turn.failed" not in types, "quota did not retain one resumed turn")
            probe.require(any(event.get("thread_id") == saved["id"] for event in events),
                          "exec did not resume the seeded thread")
            second, third = mock.requests[1:]
            probe.require(second["body"]["input"] == third["body"]["input"],
                          "quota retry changed completed work")
            probe.retry_after_delay(second, third)
            if unknown_reset:
                probe.require(third["requestStartedAt"] >= second["headerAt"] + 30,
                              "unknown-reset backoff was bypassed")
            else:
                probe.require(third["unixAt"] >= second["quotaResetUnix"], "quota reset was bypassed")

        def verify_source(process, record):
            rpc = Rpc(process, record)
            rpc.initialize()
            response = rpc.request("thread/read", {"threadId": saved["id"], "includeTurns": False})
            probe.require(response["thread"]["source"] == origin, "recovery rewrote saved origin")

        probe.require(probe.run(app_server, server_args("vscode"), root, env, "source",
                                interact=verify_source) == 0, "source check did not shut down cleanly")
        result = {"case": name, "passed": True, "origin": origin, "nonExec": non_exec,
                  "unknownReset": unknown_reset, "requests": len(mock.requests)}
        probe.write_json(root / "result.json", result)
        return result
    finally:
        server.shutdown()
        thread.join()
        server.server_close()
        probe.write_json(root / "seed-requests.json", mock.seed_requests)
        probe.write_json(root / "requests.json", mock.requests)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", required=True, type=Path)
    parser.add_argument("--app-server", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    output = args.output.absolute()
    output.mkdir(parents=True, exist_ok=False)
    report = {"passed": False, "scope": "isolated local quota-resume fixtures", "cases": []}
    try:
        codex, app_server = args.codex.resolve(strict=True), args.app_server.resolve(strict=True)
        for origin, non_exec, unknown in [("cli", False, False), ("vscode", False, True),
                                           ("exec", True, False)]:
            report["cases"].append(case(codex, app_server, output, origin,
                                         non_exec=non_exec, unknown_reset=unknown))
        report["passed"] = True
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
    probe.write_json(output / "result.json", report)
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
