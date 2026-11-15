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
  linger        as `wall`, but once `stop` appears it keeps running until it is
                terminated, as a host stalled in its own teardown does
  same-account  as `wall`, but once refused it leaves a handoff receipt whose
                destination is the account the session is already on
  no-bound      as `wall`, with a control socket whose `harness policy` reply has
                rotation switched off and prints no five-hour bound
  login-stays   a lost-login session that is not moved: one answered turn, the request
                refused with 401, its refresh token presented once and refused, the
                refusal kept as codex-alab keeps it, the same account's renewal asked
                twice by the host's trace, and a control socket whose `harness policy`
                reply prints `codex_rotation_login=1` where the fixture says `lost_login`
  login-asks    as `login-stays`, but the host's trace also holds a
                `codex-login-rotation prepare`, as a host asked to move the session has
  login-no-word as `login-stays`, but its policy never prints the word, whatever the
                fixture says
  login-stranger as `login-stays`, but its refresh token is presented under a User-Agent
                that is neither codex's nor aterm's
  exec          an exec session that reads its usage once and never finishes
  exit          a host that says nothing and exits 3
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import ssl
import sys
import threading
import time
import urllib.error
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "smoke"))
import capacity_exec as probe  # noqa: E402  (the prompt and plan the controllers look for)

POLL_SECONDS = 0.1
# What the `no-bound` host answers `harness policy`: an owner's reply, rotation off.
NO_BOUND_POLICY = "OK gen=1 effective=on why=enabled contract=2,3 codex_rotation=0 limit_wait=true"
# What a lost-login stand-in answers `harness policy`, without and with the word for the move.
LOGIN_POLICY = "OK gen=1 effective=on why=enabled contract=2,3 codex_rotation=1 codex_rotation_five_hour=900"
LOGIN_POLICY_TAIL = " logins=renew accounts=early"
# The word codex-alab keeps a refused refresh token under, by the provider's code for it.
VETO_WORDS = {"refresh_token_expired": None, "refresh_token_invalidated": "revoked",
              "refresh_token_reused": "exhausted"}
# Longer than any test lets a controller run; the stand-in never outlives its test.
LIFE_SECONDS = 60
# The session's requests carry a User-Agent of codex's own form, which the lost-login
# cases' record attributes a request by (renewal_follow.py `request_agent`).
USER_AGENT = "codex_cli_rs/0.0.0-stand-in (stand-in 0; none) stand-in"


class Session:
    def __init__(self, config):
        self.root = Path(config["root"])
        self.home = Path(config["env"]["CODEX_HOME"])
        settings = (self.home / "config.toml").read_text()
        self.base = re.search(r'^chatgpt_base_url = "([^"]+)"$', settings, re.MULTILINE).group(1)
        # The fixture login the controller generated for this run.
        tokens = json.loads(Path(config["auth_path"]).read_text())["tokens"]
        self.token = tokens["access_token"]
        # Where this login's refresh token is presented, and the token: both the fixture's.
        self.issuer = tokens["oauth_provenance"]["issuer"]
        self.refresh_token = tokens["refresh_token"]
        # The controller's certificate is its own; what is under test is not TLS.
        self.tls = ssl.create_default_context()
        self.tls.check_hostname = False
        self.tls.verify_mode = ssl.CERT_NONE
        self.deadline = time.monotonic() + LIFE_SECONDS

    def call(self, method, path, body=None):
        """(status, JSON reply or None); a refusal or a fault is a status, not an error."""
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(self.base + path, data=data, method=method, headers={
            "authorization": "Bearer " + self.token, "content-type": "application/json",
            "user-agent": USER_AGENT})
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

    def refresh(self, agent=USER_AGENT):
        """Present this login's refresh token to its issuer: (status, the refusal's code)."""
        request = urllib.request.Request(self.issuer + "/oauth/token", method="POST",
            data=json.dumps({"client_id": "fixture-client", "grant_type": "refresh_token",
                             "refresh_token": self.refresh_token}).encode(),
            headers={"content-type": "application/json", "user-agent": agent})
        try:
            with urllib.request.urlopen(request, context=self.tls, timeout=10) as reply:
                return reply.status, None
        except urllib.error.HTTPError as error:
            with error:
                status, body = error.code, json.loads(error.read())
        code = body.get("error")
        return status, code.get("code") if isinstance(code, dict) else code

    def keep_refusal(self, code):
        """Keep the refusal as codex-alab does: one word under a digest of the token, and
        nothing for a token that simply ended."""
        word = VETO_WORDS.get(code, "other")
        if word is not None:
            directory = self.home / "alab" / "login-refusals"
            directory.mkdir(parents=True)
            (directory / hashlib.sha256(
                b"alab/login-refusal/v1\0" + self.refresh_token.encode()).hexdigest()).write_text(word)

    def traced(self, kind, verb, said):
        """One line of the host's control trace, stamped with the clock a host stamps with."""
        with (self.root / "renewal-control-trace.jsonl").open("a") as trace:
            trace.write(json.dumps({"kind": kind, "verb": verb,
                "outcome" if kind == "renewal" else "reply": said,
                "monotonicUs": time.clock_gettime_ns(time.CLOCK_MONOTONIC) // 1000}) + "\n")

    def untouched_pool(self):
        """The host's pool as a lost-login fixture lays it out and a host that moved
        nothing leaves it."""
        pool = self.root / "host-state" / "accounts-codex"
        pool.mkdir(parents=True, exist_ok=True)
        (pool / "state.json").write_text(json.dumps({"active": "main", "rotations": [],
            "accounts": {"main": {"auth": "ok"}, "target": {"auth": "ok"}}}))

    def thread_at_login_wall(self):
        """The thread's own record: the prompt typed once, and its turn ended unauthorized."""
        sessions = self.home / "sessions"
        sessions.mkdir(parents=True, exist_ok=True)
        (sessions / "rollout-stand-in-thread.jsonl").write_text("".join(json.dumps(event) + "\n" for event in (
            {"type": "response_item", "payload": {"type": "message", "role": "user",
                "content": [{"type": "input_text", "text": probe.PROMPT}]}},
            {"type": "event_msg", "payload": {"type": "task_complete",
                "error": {"codex_error_info": "unauthorized"}}})))

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

    def same_account_receipt(self):
        directory = self.root / "host-state" / "accounts-codex" / "renewal-follow"
        directory.mkdir(parents=True, exist_ok=True)
        identity = {"account": "stand-in-source", "user": "stand-in-user"}
        (directory / ("cd" * 16 + ".json")).write_text(json.dumps({
            "id": "cd" * 16, "phase": "ready", "intent": {"identity": identity},
            "destination": identity}))

    def at_rest(self):
        sessions = self.home / "alab" / "sessions"
        sessions.mkdir(parents=True)
        (sessions / "stand-in.json").write_text(json.dumps({
            "sessionId": "stand-in-thread", "pid": os.getpid(), "status": "idle",
            "draftEmpty": True}))


def serve_policy(root, reply=NO_BOUND_POLICY):
    """Answer each control connection one line, `reply`, as a host answers `harness policy`.

    Returns what the host's first line adds for it: the socket and its token's path.
    """
    endpoint, token = root / "control.sock", root / "control.sock.token"
    token.write_text("stand-in-token\n")
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.bind(str(endpoint))
    listener.listen()

    def answer():
        while True:
            connection, _ = listener.accept()
            with connection, connection.makefile("rb") as incoming:
                # The AUTH line, then the command.
                incoming.readline()
                incoming.readline()
                connection.sendall((reply + "\n").encode())
    threading.Thread(target=answer, daemon=True).start()
    return {"socket": str(endpoint), "token_path": str(token)}


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
    if mode in ("handoff", "same-account"):
        session.native_signal() if mode == "handoff" else session.same_account_receipt()
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
    if mode == "linger":
        while time.monotonic() < session.deadline:
            time.sleep(POLL_SECONDS)
    return 0


def lost_login(session, mode):
    session.untouched_pool()
    session.usage_allowed()
    if session.call("POST", "/v1/responses", turn(False))[0] != 200:
        return 4
    if session.call("POST", "/v1/responses", turn(True))[0] != 401:
        return 5
    status, code = session.refresh(*(["curl/8.7.1"] if mode == "login-stranger" else []))
    if status not in (400, 401):
        return 6
    session.keep_refusal(code)
    session.thread_at_login_wall()
    if mode == "login-asks":
        session.traced("login-rotation", "prepare", "OK codex-login-rotation state=waiting retry_ms=5000")
    # The login wall's own path: the pass after the refusal, and the next look.
    session.traced("renewal", "prepare", "waiting")
    time.sleep(POLL_SECONDS)
    session.traced("renewal", "prepare", "waiting")
    session.wait_for_stop()
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--mode", required=True,
                        choices=("wall", "handoff", "late-handoff", "linger", "same-account",
                                 "no-bound", "login-stays", "login-asks", "login-no-word",
                                 "login-stranger", "exec", "exit"))
    parser.add_argument("--renewal-config", required=True, type=Path)
    args = parser.parse_args()
    if args.mode == "exit":
        return 3
    config = json.loads(args.renewal_config.read_text())
    session = Session(config)
    control = serve_policy(session.root) if args.mode == "no-bound" else {}
    if args.mode.startswith("login-"):
        word = config.get("lost_login") and args.mode != "login-no-word"
        control = serve_policy(session.root, LOGIN_POLICY
            + (" codex_rotation_login=1" if word else "") + LOGIN_POLICY_TAIL)
    # What each controller reads from the host's first line.
    print(json.dumps({"host_pid": os.getpid(), "host_birth": 1, "sid": "stand-in",
                      "shell_pid": os.getpid(), "child_pid": os.getpid(), **control}), flush=True)
    if args.mode == "exec":
        session.usage_allowed()
        session.wait_for_stop()
        return 0
    if args.mode.startswith("login-"):
        return lost_login(session, args.mode)
    return wall(session, args.mode)


if __name__ == "__main__":
    sys.exit(main())
