#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Read a failed turn under a second login, then explicitly continue it with codex exec."""

import argparse
import json
import os
from pathlib import Path
import secrets
import sys
import tempfile
import traceback

import account_resume
import capacity_exec as fixture
import model_resume as terminal
from account_resume import configure_accounts, start_gateway
from ordinary_account_renewal import fingerprint
from quota_resume import Rpc


class WeeklyQuotaReached(Exception):
    """The source submitted the result of its completed command."""


class QuotaGateway(account_resume.Gateway):
    def __init__(self):
        super().__init__({"events": [], "requests": []})
        self.refusals = 0

    def response(self, body, headers):
        response = super().response(body, headers)  # Retain normal per-request routing and history evidence.
        if self.account_route(headers) == ("source", "workspace-source") and any(
            item.get("type") == "function_call_output" and item.get("call_id") == account_resume.TOOL_CALL["call_id"]
            for item in body["input"]
        ):
            raise WeeklyQuotaReached
        return response

    def handler(self):
        gateway = self

        class Handler(super().handler()):
            def do_POST(self):
                try:
                    super().do_POST()
                except WeeklyQuotaReached:
                    body = json.dumps({"error": {"type": "usage_limit_reached", "resets_in_seconds": 86400,
                                                 "message": "Synthetic weekly quota exhausted"}}).encode()
                    self.send_response(429)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                    with gateway.lock:
                        gateway.refusals += 1

        return Handler


def run_app_server(binary, root, env, login, phase, action):
    result = None

    def interact(process, output):
        nonlocal result
        rpc = Rpc(process, output)
        rpc.initialize()
        identity = rpc.request("account/read", {"refreshToken": False})
        fixture.require(identity["account"]["email"] == f"{login}@example.invalid"
                        and identity["workspaceRouting"]["chatgptAccountId"] == f"workspace-{login}",
                        f"{phase} opened the wrong login or workspace")
        result = action(rpc)

    status = fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, phase, interact=interact)
    fixture.require(status == 0, f"{phase} app-server failed to exit cleanly")
    return result


def seed_source_turn(rpc, work):
    started = rpc.request("thread/start", {"cwd": str(work), "model": terminal.OLD, "modelProvider": "gateway"})
    thread = started["thread"]["id"]
    turn = rpc.turn(thread, terminal.SEED)
    fixture.require(all(terminal.CHECK["MODEL_UUID"].fullmatch(value) for value in (thread, turn["id"])),
                    "source thread or turn ID is invalid")
    return thread, turn


def fail_source_turn(rpc, work):
    thread, turn = seed_source_turn(rpc, work)
    fixture.require(turn["status"] == "failed" and turn["error"]["codexErrorInfo"] == "usageLimitExceeded",
                    "source turn did not fail with a usage limit")
    return thread, turn


def read_failed_turn(rpc, thread, source, marker):
    saved = rpc.request("thread/read", {"threadId": thread, "includeTurns": True})["thread"]
    turn, = saved["turns"]
    fixture.require(saved["id"] == thread and turn["id"] == source["id"] and turn["status"] == "failed"
                    and turn["error"] == source["error"], "reopening changed the failed turn or its error")
    fixture.require([item["type"] for item in turn["items"]] == ["userMessage", "commandExecution"],
                    "reopening lost or duplicated the original input or command")
    prompt, call = turn["items"]
    fixture.require([(p["type"], p.get("text")) for p in prompt["content"]] == [("text", terminal.SEED)],
                    "saved input differs from the original prompt")
    fixture.require(call["id"] == account_resume.TOOL_CALL["call_id"] and call["status"] == "completed"
                    and call["exitCode"] == 0 and call["aggregatedOutput"] == marker + "\n",
                    "saved command did not retain its successful output")
    return turn


def check_requests(gateway, thread, source, destination, marker):
    fixture.require(len(gateway.requests) == 3, "expected two source requests and one destination continuation")
    first, refused, continued = gateway.requests
    fixture.require(gateway.refusals == 1 and all(row["thread"] == thread for row in gateway.requests)
                    and first["turn"] == refused["turn"] == source != destination == continued["turn"],
                    "model requests crossed a thread boundary or reused the failed turn")
    routes = [(r["login"], r["account"]) for r in gateway.requests]
    fixture.require(routes == [("source", "workspace-source")] * 2 + [("destination", "workspace-destination")],
                    "model requests used the wrong bearer or workspace")
    for phase, login in (("seed", "source"), ("resume", "destination")):
        reads = [r for r in gateway.reads if r["phase"] == phase]
        fixture.require({r["path"] for r in reads} == {"/v1/models", "/backend-api/wham/accounts/check"}
                        and all((r["login"], r["account"]) == (login, "workspace-" + login) for r in reads),
                        f"{phase} catalog or workspace read used the wrong login")
    fixture.require(first["history"] == [terminal.SEED]
                    and refused["history"] == [terminal.SEED, "function_call", "function_call_output"]
                    and continued["history"] == [*refused["history"], terminal.CONTINUE],
                    "continuation lost or replayed the original prompt or command")
    call, output = refused["tool_records"]
    fixture.require(all(call.get(k) == v for k, v in account_resume.TOOL_CALL.items())
                    and output["call_id"] == call["call_id"] and output["output"].endswith(marker + "\n")
                    and continued["tool_records"] == refused["tool_records"],
                    "destination did not receive the original tool call and identical result")


def exercise(binary, root, port, gateway, report):
    env, slots = configure_accounts(root, port, gateway)
    before = {name: fingerprint(path) for name, path in slots.items()}

    def check_slots():
        fixture.require(all(fingerprint(path) == before[name] for name, path in slots.items())
                        and not os.path.lexists(root / "codex-home/auth.json"), "a credential slot changed")

    marker = "cedar-" + secrets.token_hex(16)
    work = root / "work"
    (work / "cedar-marker.txt").write_text(marker + "\n")
    check_slots()
    thread, source = run_app_server(binary, root, dict(env, CODEX_AUTH_PATH=str(slots["source"])), "source", "seed",
                                    lambda rpc: fail_source_turn(rpc, work))
    check_slots()
    fixture.require((work / "cedar-calls").read_text().splitlines() == [marker], "source did not run exactly once")
    report.update(thread=thread, source_turn=source["id"], source_status=source["status"],
                  source_error_code=source["error"]["codexErrorInfo"])
    destination_env = dict(env, CODEX_AUTH_PATH=str(slots["destination"]))
    with gateway.lock:
        previous = len(gateway.requests)
        gateway.resumed = True
    saved = run_app_server(binary, root, destination_env, "destination", "read",
                           lambda rpc: read_failed_turn(rpc, thread, source, marker))
    check_slots()
    with gateway.lock:
        fixture.require(len(gateway.requests) == previous == 2, "opening the failed turn submitted inference")
    report.update(reopened_status=saved["status"], reopened_error_code=saved["error"]["codexErrorInfo"],
                  read_inference_requests=0)
    destination = account_resume.exec_resume(binary, root, destination_env, thread)
    check_slots()
    fixture.require((work / "cedar-calls").read_text().splitlines() == [marker], "continuation replayed the command")
    return thread, source["id"], destination, marker


def run(binary, output):
    output = output.parent.resolve(strict=True) / output.name
    output.mkdir(mode=0o700)
    report = {"passed": False, "binary_sha256": terminal.CHECK["sha256"](binary)}
    try:
        with tempfile.TemporaryDirectory(dir=output) as temporary:
            root = Path(temporary)
            gateway = QuotaGateway()
            listener = start_gateway(root, gateway)
            cleanup = {}
            try:
                thread, source, destination, marker = exercise(binary, root, listener[0].server_port, gateway, report)
            finally:
                fixture.cleanup_resources((("gateway", *listener),), (), cleanup, sys.exc_info()[1])
            fixture.require("cleanupErrors" not in cleanup, "fixture listener cleanup failed")
            check_requests(gateway, thread, source, destination, marker)
            report.update(destination_turn=destination, destination_status="completed",
                          source_quota_responses=gateway.refusals,
                          original_command_executions=1, original_tool_result_preserved=True, slots_preserved=True)
        fixture.require(terminal.CHECK["sha256"](binary) == report["binary_sha256"], "executable identity changed")
        report["passed"] = True
    except Exception as error:
        # RPC error text can echo a response. Retain the failed check's location without saving that payload.
        report["error_type"] = type(error).__name__
        report["error_at"] = [f"{Path(frame.filename).name}:{frame.lineno}"
                              for frame in traceback.extract_tb(error.__traceback__)[-3:]]
    fixture.write_json(output / "result.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="New private directory for result.json")
    args = parser.parse_args()
    sys.exit(0 if run(args.codex.resolve(strict=True), args.output)["passed"] else 1)
