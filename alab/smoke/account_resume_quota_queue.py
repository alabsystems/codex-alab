#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check whether a saved quota error blocks input under a different login in the real terminal."""

import argparse
import base64
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import tempfile
import threading
import time
import traceback
from urllib.parse import urlsplit

import account_resume
import account_resume_after_quota as quota
import capacity_exec as fixture
import model_resume as terminal
from ordinary_account_renewal import fingerprint


class Gateway(quota.QuotaGateway):
    def __init__(self, failed):
        super().__init__()
        self.failed = failed
        self.read_started = threading.Event()
        self.release = threading.Event()
        self.followup = threading.Event()
        self.read_ns = None
        self.release_ns = None
        self.followup_ns = None
        self.unreleased_read = False
        self.unexpected_login = False

    def response(self, body, headers):
        response = super().response(body, headers) if self.failed else account_resume.Gateway.response(self, body, headers)
        if (self.account_route(headers) == ("destination", "workspace-destination")
                and terminal.CONTINUE in terminal.messages(body, "user")):
            with self.lock:
                self.followup_ns = time.monotonic_ns()
                self.followup.set()
        return response

    def release_usage(self):
        with self.lock:
            if self.release_ns is None:
                self.release_ns = time.monotonic_ns()
            self.release.set()

    def handler(self):
        gateway = self

        class Handler(super().handler()):
            def do_GET(self):
                if urlsplit(self.path).path != "/backend-api/wham/usage":
                    return super().do_GET()
                if gateway.account_route(self.headers) != ("destination", "workspace-destination"):
                    with gateway.lock:
                        gateway.unexpected_login = True
                    self.send_error(401)
                    return
                with gateway.lock:
                    if gateway.read_ns is None:
                        gateway.read_ns = time.monotonic_ns()
                    gateway.read_started.set()
                if not gateway.release.wait(40):
                    with gateway.lock:
                        gateway.unreleased_read = True
                    self.send_error(503)
                    return
                usage = {"plan_type": "pro", "rate_limit": {"allowed": True, "limit_reached": False,
                         "primary_window": {"used_percent": 0, "limit_window_seconds": 18000,
                                            "reset_at": int(time.time()) + 18000}}}
                self.send_body("application/json", json.dumps(usage).encode())

        return Handler


def drive_terminal(binary, root, env, thread, history, gateway):
    raw, custody = root / "resume.raw", root / "pty.json"
    argv = [str(binary), "--no-daemon", "--no-alt-screen", "resume", thread]
    spec = root / "tmp/controller.json"
    spec.write_text(json.dumps(dict(argv=argv, cwd=str(root / "work"), env=env, raw=str(raw), result=str(custody))))
    queued = False

    def interact(process, _output):
        nonlocal queued
        command_id = 0

        def send(data):
            nonlocal command_id
            command_id += 1
            process.stdin.write((json.dumps(dict(id=command_id, write=base64.b64encode(data).decode())) + "\n").encode())
            process.stdin.flush()

        def visible(text, offset=0):
            return raw.is_file() and text.encode() in raw.read_bytes()[offset:]

        def wait_until(condition, description):
            deadline = time.monotonic() + 10
            while not condition():
                fixture.require(process.poll() is None and time.monotonic() < deadline, description)
                time.sleep(.05)

        try:
            wait_until(lambda: visible(history), "terminal did not render the original turn")
            fixture.require(gateway.read_started.wait(5), "no authenticated destination quota read")
            offset = raw.stat().st_size
            send(b"\x1b[200~" + terminal.CONTINUE.encode() + b"\x1b[201~")
            wait_until(lambda: visible(terminal.CONTINUE, offset), "continuation did not reach the editor")
            offset = raw.stat().st_size
            send(b"\r")
            wait_until(lambda: gateway.followup.is_set() or visible("Queued follow-up inputs", offset),
                       "neither a submitted nor a queued continuation was observed")
            queued = visible("Queued follow-up inputs", offset)
            if queued:
                # A transient queue that drains independently of quota can still pass.
                gateway.followup.wait(3)
        finally:
            gateway.release_usage()
        wait_until(lambda: visible(terminal.ANSWER), "destination did not display the continuation reply")
        terminal.wait_for_turn_completion(root, gateway, process, terminal.ANSWER, time.monotonic() + 10)
        for wait in (1, 12):
            if process.poll() is not None:
                break
            send(b"\x03")
            try:
                process.wait(timeout=wait)
            except subprocess.TimeoutExpired:
                if wait == 12:
                    raise

    status = fixture.run(sys.executable, ["-I", "-B", "-S", str(terminal.HERE / "history_pagination.py"),
                                          "--guardian", str(spec)], root, env, "terminal",
                         interact=terminal.with_pty_lifeline(interact))
    fixture.require(status == 0, "terminal guardian failed")
    terminal.CHECK["check_model_terminal_exit"](json.loads(custody.read_text()), argv)
    return queued


def exercise(binary, root, gateway, port):
    env, slots = quota.configure_accounts(root, port, gateway)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm")
    fingerprints = {login: fingerprint(path) for login, path in slots.items()}
    marker = "cedar-" + secrets.token_hex(16)
    work = root / "work"
    (work / "cedar-marker.txt").write_text(marker + "\n")

    source_env = dict(env, CODEX_AUTH_PATH=str(slots["source"]))
    thread, turn = quota.run_app_server(binary, root, source_env, "source", "seed",
                                        lambda rpc: quota.seed_source_turn(rpc, work))
    status = "failed" if gateway.failed else "completed"
    fixture.require(turn["status"] == status and gateway.refusals == int(gateway.failed), "unexpected source result")
    if gateway.failed:
        fixture.require(turn["error"]["codexErrorInfo"] == "usageLimitExceeded", "source failed for a different reason")
    with gateway.lock:
        gateway.resumed = True
    history = turn["error"]["message"] if gateway.failed else terminal.REPLY
    queued = drive_terminal(binary, root, dict(env, CODEX_AUTH_PATH=str(slots["destination"])), thread, history, gateway)
    fixture.require((work / "cedar-calls").read_text().splitlines() == [marker], "original command was replayed")
    fixture.require(all(fingerprint(path) == fingerprints[login] for login, path in slots.items())
                    and not os.path.lexists(root / "codex-home/auth.json"), "a credential slot changed")
    return thread, turn["id"], status, queued


def case(binary, root, failed):
    gateway = Gateway(failed)
    listener = quota.start_gateway(root, gateway)
    cleanup = {}
    try:
        thread, source_turn, status, queued = exercise(binary, root, gateway, listener[0].server_port)
    finally:
        gateway.release_usage()
        fixture.cleanup_resources((("gateway", *listener),), (), cleanup, sys.exc_info()[1])
    fixture.require("cleanupErrors" not in cleanup and not gateway.unreleased_read and not gateway.unexpected_login,
                    "usage gateway did not finish with the selected destination")
    first, tool, resumed = [r for r in gateway.requests if r["history"]]
    fixture.require([(r["login"], r["account"]) for r in [first, tool, resumed]] ==
                    [("source", "workspace-source")] * 2 + [("destination", "workspace-destination")],
                    "inference used a different bearer or workspace")
    fixture.require(all(r["thread"] == thread for r in [first, tool, resumed])
                    and first["turn"] == tool["turn"] == source_turn != resumed["turn"]
                    and terminal.CHECK["MODEL_UUID"].fullmatch(resumed["turn"]), "turn or thread identity changed")
    expected = [terminal.SEED, "function_call", "function_call_output"]
    fixture.require(first["history"] == [terminal.SEED] and tool["history"] == expected
                    and resumed["history"] == [*expected, *([] if failed else [terminal.REPLY]), terminal.CONTINUE]
                    and resumed["tool_records"] == tool["tool_records"], "destination received unexpected history")
    read, released, continued = gateway.read_ns, gateway.release_ns, gateway.followup_ns
    fixture.require(read is not None and released is not None and continued is not None and read < min(released, continued),
                    "destination continuation did not overlap the controlled quota check")
    before = continued < released
    fixture.require(before or queued, "missing visible queue evidence for a delayed continuation")
    return dict(sourceStatus=status, thread=thread, sourceTurn=source_turn, destinationTurn=resumed["turn"],
                queuedBannerShown=queued, destinationResponded=True, originalCommandExecutions=1,
                followupBeforeUsageRelease=before, releaseSinceReadMs=(released - read) // 1_000_000,
                followupSinceReadMs=(continued - read) // 1_000_000)


def run(binary, output):
    output = output.parent.resolve(strict=True) / output.name
    output.mkdir(mode=0o700)
    report = {"passed": False, "binarySha256": terminal.CHECK["sha256"](binary), "cases": []}
    for failed in (True, False):
        try:
            with tempfile.TemporaryDirectory(dir=output) as tmp:
                report["cases"].append(case(binary, Path(tmp), failed))
        except Exception as error:
            frames = traceback.extract_tb(error.__traceback__)[-3:]
            report["cases"].append(dict(sourceStatus="failed" if failed else "completed", error=type(error).__name__,
                                        errorAt=[f"{Path(frame.filename).name}:{frame.lineno}" for frame in frames]))
    report["passed"] = (all(c.get("followupBeforeUsageRelease") is True for c in report["cases"])
                        and terminal.CHECK["sha256"](binary) == report["binarySha256"])
    fixture.write_json(output / "result.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="New private directory for result.json")
    args = parser.parse_args()
    sys.exit(0 if run(args.codex.resolve(strict=True), args.output)["passed"] else 1)
