#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Probe repeated stale quota resets and native SIGINT during a quota wait.

Uses isolated local HTTP fixtures and the existing command guardian; builds nothing.
"""

import argparse
import json
import math
import os
from pathlib import Path
import select
import signal
import sys
import time

import capacity_exec as probe


class Quota(probe.Mock):
    def __init__(self, cancel):
        super().__init__(False)
        self.cancel = cancel

    def reply(self, body):
        with self.lock:
            number = len(self.requests) + 1
            record = {"at": time.monotonic(), "unixAt": time.time(), "body": body}
            self.requests.append(record)
        if number <= (1 if self.cancel else 4):
            reset = math.ceil(time.time()) + (3600 if self.cancel else -60)
            record["quotaResetUnix"] = reset
            error = {"error": {"type": "usage_limit_reached", "resets_at": reset,
                               "message": "Local quota wait fixture"}}
            return 429, json.dumps(error).encode(), "application/json", None, record
        item = {"type": "message", "role": "assistant", "id": "quota-done",
                "content": [{"type": "output_text", "text": probe.ANSWER}]}
        return 200, probe.response("quota-done", item), "text/event-stream", None, record


def case(codex, output, cancel):
    root = output / ("cancel-distant-reset" if cancel else "repeated-stale-reset")
    root.mkdir()
    env = probe.fresh_environment(root)
    mock = Quota(cancel)
    server, thread = probe.serve(mock)
    cancellation = {}
    try:
        probe.configure(root, server.server_port)
        args = ["exec", "--json", "--color", "never", "--skip-git-repo-check",
                "--cd", str(root / "work"), "--sandbox", "read-only", probe.PROMPT]

        def interrupt(process, record):
            # Wait for the native notification emitted after the quota deadline
            # is armed. The wrapper has exec'd by then, retaining its leased PID.
            deadline = time.monotonic() + 30
            buffer = b""
            while b"Waiting for account quota" not in buffer:
                remaining = deadline - time.monotonic()
                probe.require(remaining > 0, "native quota wait notification missing")
                ready, _, _ = select.select([process.stdout], [], [], min(remaining, 1))
                if ready:
                    data = os.read(process.stdout.fileno(), 65536)
                    probe.require(bool(data), "process exited before quota wait")
                    record.write(data)
                    record.flush()
                    buffer += data
                    probe.require(len(buffer) < 1024 * 1024, "oversized wait notification")
            pid = int((root / "native.pid").read_text())
            with mock.lock:
                probe.require(len(mock.requests) == 1, "unexpected request before interrupt")
            cancellation["sentAt"] = time.monotonic()
            cancellation["signal"] = "SIGINT"
            os.kill(pid, signal.SIGINT)
            # Bound native shutdown independently of the guardian's longer limit.
            end = time.monotonic() + 10
            while True:
                remaining = end - time.monotonic()
                probe.require(remaining > 0, "native interrupt did not exit within 10 seconds")
                ready, _, _ = select.select([process.stdout], [], [], min(remaining, 1))
                if ready:
                    data = os.read(process.stdout.fileno(), 65536)
                    if not data:
                        break
                    record.write(data)
            cancellation["exitObservedAt"] = time.monotonic()

        # The guardian owns/reaps this process throughout. The private wrapper
        # writes its PID then execs codex, so SIGINT targets codex, not guardians.
        executable = codex
        interact = None
        if cancel:
            executable = Path(sys.executable)
            args = [str(Path(__file__).resolve()), "--exec-wrapper",
                    str(root / "native.pid"), str(codex), *args]
            interact = interrupt
        code = probe.run(executable, args, root, env, "exec", interact=interact)
        if cancel:
            cancellation["nativeExitConfirmedAt"] = time.monotonic()
        events = [json.loads(line) for line in (root / "exec-stdout.jsonl").read_text().splitlines()]
        types = [event.get("type") for event in events]
        guardian = json.loads((root / "exec-guardian-result.json").read_text())
        probe.require(not guardian["timed_out"], "guardian timed out")
        if cancel:
            probe.require(code == 1, "native SIGINT did not produce its handled failure exit")
            probe.require(len(mock.requests) == 1, "native cancellation retried HTTP")
            probe.require("turn.completed" not in types, "cancelled quota turn reported success")
            probe.require(cancellation["nativeExitConfirmedAt"] - cancellation["sentAt"] < 10,
                          "native cancellation was not bounded")
            cancellation["observedDelaySeconds"] = (
                cancellation["nativeExitConfirmedAt"] - cancellation["sentAt"])
        else:
            probe.require(code == 0 and len(mock.requests) == 5, "stale resets did not recover")
            probe.require(types.count("turn.completed") == 1 and "turn.failed" not in types,
                          "stale reset recovery did not retain one successful turn")
            original = mock.requests[0]["body"]["input"]
            for index, (prior, retry) in enumerate(zip(mock.requests, mock.requests[1:])):
                probe.require(retry["body"]["input"] == original, "quota retry changed input")
                lower = 0.5 * 2 ** index
                # Header start is the earliest possible client receipt time.
                probe.require(retry["requestStartedAt"] >= prior["headerStartedAt"] + lower,
                              "repeated stale reset bypassed growing backoff")
        result = {"case": root.name, "passed": True, "requestCount": len(mock.requests),
                  "rawReturncode": guardian["returncode"], "guardianTimedOut": guardian["timed_out"],
                  "cancellation": cancellation}
        probe.write_json(root / "result.json", result)
        return result
    finally:
        server.shutdown()
        thread.join()
        server.server_close()
        probe.write_json(root / "requests.json", mock.requests)


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "--exec-wrapper":
        Path(sys.argv[2]).write_text(str(os.getpid()))
        # The harness needs a pipe for observing stdout; exec must still see EOF
        # on stdin, otherwise it waits for supplemental prompt input at startup.
        with open(os.devnull, "rb") as empty:
            os.dup2(empty.fileno(), 0)
        os.execv(sys.argv[3], sys.argv[3:])
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    codex = args.codex.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    report = {"passed": False, "cases": []}
    try:
        for cancel in (False, True):
            report["cases"].append(case(codex, output, cancel))
        report["passed"] = True
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
    probe.write_json(output / "result.json", report)
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
