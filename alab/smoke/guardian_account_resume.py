#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise direct Guardian reviews before and after restarting a local session."""

import argparse
import json
from pathlib import Path
import secrets
import sys

import account_resume
import capacity_exec as fixture
import model_resume
from quota_resume import Rpc
from renewal_follow import fixture_tls

# A fresh process or a different parent starts with no private reviewer history.
# On a live parent the previous assessment must appear as an assistant message.
EXPECTED_HISTORY = {
    "original-first": [],
    "original-second": ["original-first"],
    "resumed-first": [],
    "resumed-second": ["resumed-first"],
    "other-parent": [],
}
COMMAND = "echo reviewed > guardian-direct.txt"


class GuardianGateway(account_resume.Gateway):
    def __init__(self, report):
        super().__init__(report)
        self.review = None
        self.verdicts = report["verdicts"]
        nonce = secrets.token_hex(16)
        self.rationales = {call: f"review-{nonce}-{call}" for call in EXPECTED_HISTORY}

    def handler(self):
        class Handler(super().handler()):
            protocol_version = "HTTP/1.1"

            def do_GET(self):
                # The built-in OpenAI provider falls back to HTTP on a 426.
                if self.headers.get("Upgrade", "").lower() == "websocket":
                    self.send_error(426)
                else:
                    super().do_GET()

        return Handler

    def response(self, body, headers):
        metadata = body.get("client_metadata", {})
        reviewer = metadata.get("x-openai-subagent", headers.get("x-openai-subagent"))
        text = "Parent thread created."
        call = "parent"
        if reviewer == "guardian":
            items = body.get("input", [])
            assistant = [item for item in items if item.get("role") == "assistant"]
            with self.lock:
                call, outcome = self.review
                observations = {}
                for key, inputs in (("assistant", assistant), ("any_input", items)):
                    contents = json.dumps(inputs)
                    observations[key] = [name for name, marker in self.rationales.items()
                                         if marker in contents]
                self.requests.append({
                    "call_id": call, "login": self.tokens.get(headers.get("authorization")),
                    "workspace": headers.get("chatgpt-account-id"), **observations,
                })
                text = json.dumps({"outcome": outcome, "risk_level": "medium",
                                   "user_authorization": "high", "rationale": self.rationales[call]})
        return fixture.response("response-" + call, {
            "type": "message", "id": "message-" + call, "role": "assistant",
            "content": [{"type": "output_text", "text": text}],
        })


def review(rpc, gateway, parent, call, outcome, workspace, previous=None):
    # Supply a recorded pending call. Direct review should assess it without execution.
    evidence = ({"type": "function_call_output", "call_id": previous, "output": "Recorded result."}
                if previous else {"type": "message", "role": "user",
                                  "content": [{"type": "input_text", "text": "Create the requested file."}]})
    tool = {"type": "function_call", "name": "exec_command", "call_id": call,
            "arguments": json.dumps({"cmd": COMMAND, "sandbox_permissions": "require_escalated",
                                     "justification": "Create the requested file."})}
    rpc.request("thread/inject_items", {"threadId": parent, "items": [evidence, tool]})
    with gateway.lock:
        gateway.review = (call, outcome)
    result = rpc.request("thread/guardianReview", {"threadId": parent})
    assessment = result["assessment"]
    fixture.require(result["callId"] == call and assessment["outcome"] == outcome
                    and assessment["rationale"] == gateway.rationales[call],
                    f"{call}: direct review did not return a fresh {outcome} assessment")
    gateway.verdicts[call] = assessment["outcome"]
    fixture.require(not (workspace / "guardian-direct.txt").exists(), f"{call}: direct review executed the call")


def run_case(binary, root, destination):
    root.mkdir(mode=0o700)
    env = fixture.fresh_environment(root)
    env.update(HTTP_PROXY="http://127.0.0.1:1", HTTPS_PROXY="http://127.0.0.1:1",
               NO_PROXY="localhost,127.0.0.1,::1", CODEX_CA_CERTIFICATE=str(root / "fixture-ca.pem"))
    report = {"passed": False, "destination_login": destination, "events": [], "requests": [], "verdicts": {}}
    workspace = root / "work"
    gateway = GuardianGateway(report)
    listener = None
    try:
        def prepare(server):
            server.socket = fixture_tls(root).wrap_socket(server.socket, server_side=True)

        listener = fixture.start_listener(gateway.handler(), prepare=prepare)
        issuer = f"https://127.0.0.1:{listener[0].server_port}"
        (root / "codex-home/config.toml").write_text(f'''model_provider = "openai"
model = "{model_resume.OLD}"
model_reasoning_effort = "high"
openai_base_url = "{issuer}/v1"
chatgpt_base_url = "{issuer}/backend-api"
cli_auth_credentials_store = "file"
features.guardian_approval = true
features.guardianv2.enabled = false
features.enable_request_compression = false
features.daemon_auto_start = false
analytics.enabled = false
check_for_update_on_startup = false
approval_policy = "on-request"
approvals_reviewer = "auto_review"
sandbox_mode = "workspace-write"
[projects.{json.dumps(str(workspace))}]
trust_level = "trusted"
''')
        account_resume.select_login(root, gateway, "source", issuer)

        def original(process, record):
            rpc = Rpc(process, record)
            rpc.initialize()
            parent = rpc.request("thread/start", {"cwd": str(workspace)})["thread"]["id"]
            report["original_parent"] = parent
            turn = rpc.turn(parent, "Create a persisted parent for direct review.")
            fixture.require(turn["status"] == "completed", "original parent turn did not complete")
            review(rpc, gateway, parent, "original-first", "allow", workspace)
            review(rpc, gateway, parent, "original-second", "allow", workspace, previous="original-first")

        server_args = ["app-server", "--listen", "stdio://"]
        fixture.require(fixture.run(binary, server_args, root, env, "original", interact=original) == 0,
                        "original app-server did not shut down cleanly")
        if destination != "source":
            account_resume.select_login(root, gateway, destination, issuer)

        def resumed(process, record):
            rpc = Rpc(process, record)
            rpc.initialize()
            parent = rpc.request("thread/resume", {"threadId": report["original_parent"]})["thread"]["id"]
            report["resumed_parent"] = parent
            fixture.require(parent == report["original_parent"], "resume changed the parent thread")
            review(rpc, gateway, parent, "resumed-first", "deny", workspace, previous="original-second")
            review(rpc, gateway, parent, "resumed-second", "deny", workspace, previous="resumed-first")
            other = rpc.request("thread/start", {"cwd": str(workspace)})["thread"]["id"]
            report["other_parent"] = other
            fixture.require(other != parent, "second-parent control reused the resumed parent")
            review(rpc, gateway, other, "other-parent", "deny", workspace)

        fixture.require(fixture.run(binary, server_args, root, env, "resumed", interact=resumed) == 0,
                        "resumed app-server did not shut down cleanly")
        fixture.require(not (workspace / "guardian-direct.txt").exists(), "a recorded tool call executed")
        fixture.require([row["call_id"] for row in gateway.requests] == list(EXPECTED_HISTORY),
                        "unexpected sequence of Guardian model requests")
        for row in gateway.requests:
            call = row["call_id"]
            login = "source" if call.startswith("original-") else destination
            fixture.require(row["login"] == login and row["workspace"] == "workspace-" + login,
                            f"{call}: wrong model request bearer or workspace")
            fixture.require(row["any_input"] == row["assistant"] == EXPECTED_HISTORY[call],
                            f"{call}: unexpected private reviewer history {row}")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        fixture.cleanup_resources((("gateway", *listener),) if listener else (), (), report, sys.exc_info()[1])
        fixture.write_json(root / "result.json", report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True, help="Codex CLI executable to test.")
    parser.add_argument("--output", type=Path, required=True, help="Fresh directory for both cases.")
    args = parser.parse_args()
    binary = args.codex.resolve(strict=True)
    root = args.output.resolve()
    root.mkdir(mode=0o700)
    digest = model_resume.CHECK["sha256"](binary)
    results = {}
    for label, destination in (("cross-login", "destination"), ("same-login", "source")):
        result = run_case(binary, root / label, destination)
        results[label] = result["passed"]
        print(f"{label}: {result.get('error', 'passed' if result['passed'] else 'failed')}", flush=True)
    passed = all(results.values()) and model_resume.CHECK["sha256"](binary) == digest
    fixture.write_json(root / "result.json", {"passed": passed, "binary_sha256": digest, "cases": results})
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
