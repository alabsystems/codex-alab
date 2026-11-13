#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""A stand-in for aterm's native host and the codex it launches, for the smokes' own tests.

It is not a host and proves nothing about one. It lets a test run a smoke's whole
controller (its server, its loop and its verdict) in a few seconds with no package and no
aterm build: it reads the fixture file the controller wrote, signs in as the fixture's
source login, sends the requests a session would send to the controller's loopback
provider, and leaves the files a host would leave.

  stand_in_host.py --mode <mode> --renewal-config <fixture.json>

Modes:
  wall          the five-hour-wall-soon session: one answered turn, the refused request,
                usage polled until it is allowed again, the request sent again, at rest
  handoff       as `wall`, but once refused it records a native Signal, as a host that
                moved the session would
  late-handoff  as `wall`, and records the native Signal only after the controller has
                stopped looking (when `stop` appears)
  exec          an exec session that reads its usage once and never finishes
  exit          a host that says nothing and exits 3
"""

import argparse
import json
import os
from pathlib import Path
import re
import ssl
import sys
import time
import urllib.error
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "smoke"))
import capacity_exec as probe  # noqa: E402  (the prompt and plan the controllers look for)

POLL_SECONDS = 0.1
# Longer than any test lets a controller run; the stand-in never outlives its test.
LIFE_SECONDS = 60


class Session:
    def __init__(self, config):
        self.root = Path(config["root"])
        self.home = Path(config["env"]["CODEX_HOME"])
        settings = (self.home / "config.toml").read_text()
        self.base = re.search(r'^chatgpt_base_url = "([^"]+)"$', settings, re.MULTILINE).group(1)
        # The fixture login the controller generated for this run.
        self.token = json.loads(Path(config["auth_path"]).read_text())["tokens"]["access_token"]
        # The controller's certificate is its own; what is under test is not TLS.
        self.tls = ssl.create_default_context()
        self.tls.check_hostname = False
        self.tls.verify_mode = ssl.CERT_NONE
        self.deadline = time.monotonic() + LIFE_SECONDS

    def call(self, method, path, body=None):
        """(status, JSON reply or None); a refusal or a fault is a status, not an error."""
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(self.base + path, data=data, method=method, headers={
            "authorization": "Bearer " + self.token, "content-type": "application/json"})
        try:
            with urllib.request.urlopen(request, context=self.tls, timeout=10) as reply:
                text = reply.read()
                status = reply.status
        except urllib.error.HTTPError as error:
            with error:
                text = error.read()
                status = error.code
        try:
            return status, json.loads(text)
        except ValueError:
            return status, None

    def usage_allowed(self):
        status, body = self.call("GET", "/api/codex/usage")
        return body["rate_limit"]["allowed"] if status == 200 else None

    def stopped(self):
        return (self.root / "stop").exists() or time.monotonic() >= self.deadline

    def wait_for_stop(self):
        while not self.stopped():
            time.sleep(POLL_SECONDS)

    def native_signal(self):
        with (self.root / "native-actions.jsonl").open("a") as actions:
            actions.write(json.dumps({"schema": 1, "action": "signal", "operation": "ab" * 16,
                                      "replyOk": True, "nativeHold": False}) + "\n")

    def at_rest(self):
        sessions = self.home / "alab" / "sessions"
        sessions.mkdir(parents=True)
        (sessions / "stand-in.json").write_text(json.dumps({
            "sessionId": "stand-in-thread", "pid": os.getpid(), "status": "idle",
            "draftEmpty": True}))


def turn(completed):
    """The session's one model request: the prompt, then the prompt with its finished plan."""
    plan = json.dumps({"plan": [{"step": probe.PLAN, "status": "completed"}]})
    done = [{"type": "function_call", "call_id": probe.CALL_ID, "name": "update_plan",
             "arguments": plan},
            {"type": "function_call_output", "call_id": probe.CALL_ID, "output": "Plan updated"}]
    return {"input": [{"type": "message", "role": "user",
                       "content": [{"type": "input_text", "text": probe.PROMPT}]}]
                     + (done if completed else []),
            "tools": [{"type": "function", "name": "update_plan"}]}


def wall(session, mode):
    session.usage_allowed()
    if session.call("POST", "/v1/responses", turn(False))[0] != 200:
        return 4
    if session.call("POST", "/v1/responses", turn(True))[0] != 429:
        return 5
    if mode == "handoff":
        session.native_signal()
        session.wait_for_stop()
        return 0
    # Wait the wall out on this account: its own usage says when.
    while session.usage_allowed() is not True:
        if session.stopped():
            return 0
        time.sleep(POLL_SECONDS)
    if session.call("POST", "/v1/responses", turn(True))[0] != 200:
        return 6
    session.at_rest()
    session.wait_for_stop()
    if mode == "late-handoff":
        session.native_signal()
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--mode", required=True,
                        choices=("wall", "handoff", "late-handoff", "exec", "exit"))
    parser.add_argument("--renewal-config", required=True, type=Path)
    args = parser.parse_args()
    if args.mode == "exit":
        return 3
    session = Session(json.loads(args.renewal_config.read_text()))
    # What each controller reads from the host's first line.
    print(json.dumps({"host_pid": os.getpid(), "host_birth": 1, "sid": "stand-in",
                      "shell_pid": os.getpid(), "child_pid": os.getpid()}), flush=True)
    if args.mode == "exec":
        session.usage_allowed()
        session.wait_for_stop()
        return 0
    return wall(session, args.mode)


if __name__ == "__main__":
    sys.exit(main())
