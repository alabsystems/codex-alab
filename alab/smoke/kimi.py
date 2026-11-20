#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise every native Kimi model through codex exec and a loopback Responses server."""

import argparse
import base64
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading

# Both packaged Kimi profiles explicitly request High. These are wire expectations,
# including the model whose catalog has no reasoning-effort parameter.
MODEL_EFFORTS = {
    "k3-256k": "high",
    "k3.1-pro": "high",
    "k3": "high",
    "kimi-for-coding": "high",
    "k3.1-pro-highspeed": "high",
    "kimi-for-coding-highspeed": None,
}
ANSWER = "Kimi Responses tool and image support passed."
CALL_ID = "kimi-plan-1"
CALL_ID_2 = "kimi-context-2"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def sse(events):
    return "".join(
        f"event: {event['type']}\ndata: {json.dumps(event)}\n\n" for event in events
    ).encode()


def created(response_id):
    return {"type": "response.created", "response": {"id": response_id}}


def completed(response_id):
    return {
        "type": "response.completed",
        "response": {
            "id": response_id,
            "usage": {
                "input_tokens": 1,
                "input_tokens_details": None,
                "output_tokens": 1,
                "output_tokens_details": None,
                "total_tokens": 2,
            },
        },
    }


def reply_events(continued):
    response_id = "kimi-continued" if continued else "kimi-initial"
    if continued:
        item = {
            "type": "message",
            "role": "assistant",
            "id": "kimi-final-message",
            "content": [{"type": "output_text", "text": ANSWER}],
        }
        return sse([created(response_id), output_item(item), completed(response_id)])
    reasoning = {
        "type": "reasoning",
        "id": "kimi-reasoning-1",
        "summary": [{"type": "summary_text", "text": "Inspect and plan."}],
    }
    call = {
        "type": "function_call",
        "call_id": CALL_ID,
        "name": "update_plan",
        "arguments": json.dumps({
            "plan": [{"step": "Verify the Kimi Responses round trip", "status": "completed"}]
        }),
    }
    context_call = {
        "type": "function_call",
        "call_id": CALL_ID_2,
        "name": "exec_command",
        "arguments": json.dumps({"cmd": "printf kimi-exec-ok", "yield_time_ms": 1000}),
    }
    return sse([
        created(response_id),
        output_item(reasoning),
        output_item(call),
        output_item(context_call),
        completed(response_id),
    ])


def output_item(item):
    return {"type": "response.output_item.done", "item": item}


class Mock:
    def __init__(self):
        self.requests = []
        self.authorization = []
        self.failures = []
        self.lock = threading.Lock()

    def record(self, body, authorization):
        with self.lock:
            continued = any(
                item.get("type") == "function_call_output" and item.get("call_id") == CALL_ID
                for item in body.get("input", [])
                if isinstance(item, dict)
            )
            self.requests.append(body)
            self.authorization.append(authorization)
            if self.failures:
                message = self.failures.pop(0)
                data = json.dumps({"error": {"message": message}}).encode()
                return 429, data, "application/json"
            return 200, reply_events(continued), "text/event-stream"


def serve(mock):
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            if self.path != "/coding/v1/responses":
                self.send_error(404)
                return
            length = int(self.headers.get("content-length", "0"))
            body = json.loads(self.rfile.read(length))
            status, data, content_type = mock.record(body, self.headers.get("authorization"))
            self.send_response(status)
            self.send_header("content-type", content_type)
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def contains_item(value, predicate):
    if isinstance(value, dict):
        return predicate(value) or any(contains_item(item, predicate) for item in value.values())
    if isinstance(value, list):
        return any(contains_item(item, predicate) for item in value)
    return False


def run_model(executable, root, port, profile, model, expect_success=True, label=None, *, overrides=()):
    scratch = root / (label or f"{profile}-{model}")
    for name in ("home", "codex-home", "work", "tmp"):
        (scratch / name).mkdir(parents=True)
    (scratch / "codex-home" / "config.toml").write_text(
        'model = "wrong-base-user-model"\nmodel_provider = "openai"\nweb_search = "disabled"\n'
    )
    image = scratch / "pixel.png"
    image.write_bytes(base64.b64decode(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="
    ))
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": str(scratch / "home"),
        "CODEX_HOME": str(scratch / "codex-home"),
        "TMPDIR": str(scratch / "tmp"),
        "TERM": "dumb",
        "NO_COLOR": "1",
        "LANG": "en_US.UTF-8",
        "KIMI_API_KEY": "loopback-kimi-subscription-key",
    }
    command = [
        str(executable),
        "exec",
        "--profile", profile,
        "--model", model,
        "--skip-git-repo-check",
        "--sandbox", "read-only",
        "--json",
        "-c", f'model_providers.{profile}.base_url="http://127.0.0.1:{port}/coding/v1"',
    ]
    for key, value in overrides:
        command.extend(["-c", f"{key}={json.dumps(value)}"])
    command.extend(["Inspect the image, update the plan, and report completion.", "--image", str(image)])
    result = subprocess.run(
        command,
        cwd=scratch / "work",
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=120,
    )
    if expect_success:
        require(
            result.returncode == 0,
            f"{profile}/{model} exited {result.returncode}\n{result.stdout}\n{result.stderr}",
        )
        require(ANSWER in result.stdout, f"{profile}/{model} missing final JSON message")
    else:
        require(result.returncode != 0, f"{profile}/{model} unexpectedly succeeded")
        require(
            "Quota exceeded" in result.stdout + result.stderr,
            f"quota failure was not surfaced\n{result.stdout}\n{result.stderr}",
        )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--codex", required=True, type=Path)
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()
    executable = args.codex.resolve(strict=True)
    mock = Mock()
    server = serve(mock)
    scratch_context = tempfile.TemporaryDirectory(prefix="codex-alab-kimi-")
    scratch = Path(scratch_context.name)
    try:
        cases = [("kimi", model) for model in MODEL_EFFORTS] + [("kimi-cn", "k3-256k")]
        for profile, model in cases:
            run_model(
                executable,
                scratch,
                server.server_port,
                profile,
                model,
            )
        require(len(mock.requests) == 2 * len(cases), "each turn needs one tool continuation")
        for index, (profile, model) in enumerate(cases):
            initial, continued = mock.requests[2 * index:2 * index + 2]
            expected_effort = MODEL_EFFORTS[model]
            require(initial.get("model") == model, f"wrong model for {model}")
            require(initial.get("parallel_tool_calls") is True, f"parallel tools off for {model}")
            require(
                all((body.get("reasoning") or {}).get("effort") == expected_effort
                    and ("effort" in (body.get("reasoning") or {})) == (expected_effort is not None)
                    for body in (initial, continued)),
                f"wrong effort for {model}: {initial.get('reasoning')}",
            )
            require(
                contains_item(
                    initial.get("input", []),
                    lambda item: item.get("role") == "developer"
                    and "You are a coding agent running in the Codex CLI" in json.dumps(item),
                ),
                f"Codex instructions missing for {model}: {initial.get('input')!r}",
            )
            tools = json.dumps(initial.get("tools", []))
            require("web_search" in tools, f"hosted search missing for {model}")
            require("spawn_agent" in tools and "wait_agent" in tools, f"subagents missing for {model}")
            require(
                "update_plan" in tools and "exec_command" in tools,
                f"function tools missing for {model}: {tools}",
            )
            require(
                contains_item(
                    initial.get("input", []),
                    lambda item: item.get("type") == "input_image"
                    and str(item.get("image_url", "")).startswith("data:image/png;base64,"),
                ),
                f"image input missing for {model}",
            )
            require(
                any(item.get("type") == "reasoning" for item in continued.get("input", [])),
                f"reasoning history missing for {model}",
            )
            call_outputs = {
                item.get("call_id")
                for item in continued.get("input", [])
                if item.get("type") == "function_call_output"
            }
            require(
                {CALL_ID, CALL_ID_2}.issubset(call_outputs),
                f"parallel tool results missing for {model}: {call_outputs}",
            )
            require(
                any(
                    item.get("call_id") == CALL_ID_2 and "kimi-exec-ok" in json.dumps(item)
                    for item in continued.get("input", [])
                    if item.get("type") == "function_call_output"
                ),
                f"shell tool did not execute for {model}",
            )
        successful_requests = 2 * len(cases)
        mock.failures.append("The engine is currently overloaded, please try again later")
        run_model(executable, scratch, server.server_port, "kimi", "k3-256k", label="transient")
        require(
            len(mock.requests) == successful_requests + 3,
            "transient overload did not recover with exactly one retry",
        )
        mock.failures.append("You've reached your 5-hour usage limit")
        run_model(
            executable,
            scratch,
            server.server_port,
            "kimi",
            "k3-256k",
            expect_success=False,
            label="quota",
        )
        require(
            len(mock.requests) == successful_requests + 4,
            "subscription quota wall was retried",
        )
        require(
            all(value == "Bearer loopback-kimi-subscription-key" for value in mock.authorization),
            "subscription API key was not sent as the provider bearer credential",
        )
        print(f"Kimi smoke passed for {len(cases)} profile/model launches and two 429 controls")
        if args.keep:
            print(scratch)
            scratch_context = None
    finally:
        server.shutdown()
        server.server_close()
        if scratch_context is not None:
            scratch_context.cleanup()


if __name__ == "__main__":
    main()
