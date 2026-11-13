#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The loopback Responses model, command-guardian runner and package reader the smokes share.

Imported by the smokes beside it, never run. Its own capacity and quota cases, which ran a
package's `codex exec` with the Internet open, became aterm's jailed capacity lane
(`aterm-lab codex-alab-proof --capacity-only`; alab/README.md, "Headless capacity and quota
recovery"), which runs every one of them with only its loopback model reachable.
"""

import http.server
import json
import math
from pathlib import Path
import re
import signal
import subprocess
import sys
import threading
import time

import provider_fixture

MODEL = "gpt-5"
PROMPT = "Keep this headless capacity smoke task and its completed plan."
CALL_ID = "capacity-smoke-plan"
PLAN = "Completed work survives a capacity retry"
ANSWER = "Headless capacity recovery succeeded."
RETRY_SECONDS = 2
HEADER_EMISSION_LIMIT_SECONDS = 0.050
COMMAND_TIMEOUT_SECONDS = 120
GUARD_PATH = Path(__file__).resolve().parents[1] / "command_guard.py"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def retry_after_delay(overload, retry):
    """Measure from response headers, excluding request handling before them.

    The client can receive headers between the two recorded header timestamps.
    Use only that measured overlap, and refuse an inconclusive clock sample.
    Handler entry is server-observed arrival, not a TCP wire-time measurement.
    """
    start, header, request = (overload.get("headerStartedAt"), overload.get("headerAt"),
                              retry.get("requestStartedAt"))
    require(all(type(value) in (int, float) and math.isfinite(value)
                for value in (start, header, request)), "missing or invalid Retry-After timing")
    emission = header - start
    require(0 <= emission <= HEADER_EMISSION_LIMIT_SECONDS,
            f"Retry-After timing inconclusive: header emission took {emission:.6f}s")
    delay = request - header
    deadline = start + RETRY_SECONDS
    require(request >= math.nextafter(deadline, -math.inf),
            f"Retry-After bypassed: {delay:.6f}s after headers")
    return delay


def write_json(path, value):
    with path.open("x") as handle:
        json.dump(value, handle, indent=2)
        handle.write("\n")


def package_records(package):
    records = []
    for name in ("alab-build.json", "codex-package.json"):
        path = package / name
        require(path.is_file() and not path.is_symlink(), f"missing regular {name}")
        require(path.stat().st_size <= 1024 * 1024, f"oversized {name}")
        records.append(json.loads(path.read_text()))
    build, manifest = records
    require(build["version"] == manifest["version"], "package versions disagree")
    require(build["target"] == manifest["target"], "package targets disagree")
    require(re.fullmatch(r"[0-9a-f]{40}", build["source"]), "missing exact source commit")
    require(manifest["entrypoint"] == "bin/codex", "unexpected package entrypoint")
    executable = (package / manifest["entrypoint"]).resolve(strict=True)
    require(executable.is_relative_to(package) and executable.is_file(), "entrypoint escapes package")
    return build, manifest, executable


def fresh_environment(root):
    for name in ("home", "codex-home", "work", "xdg", "tmp"):
        (root / name).mkdir()
    # No inherited login, provider, proxy, aterm or agent variables.
    return {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": str(root / "home"),
            "CODEX_HOME": str(root / "codex-home"), "XDG_RUNTIME_DIR": str(root / "xdg"),
            "TMPDIR": str(root / "tmp"), "TERM": "dumb", "NO_COLOR": "1", "LANG": "en_US.UTF-8"}


def run(executable, args, root, env, label, *, interact=None):
    command = [str(executable), *args]
    require(GUARD_PATH.is_file(), "missing alab/command_guard.py; keep the probe in its source tree")
    result_path = root / (label + "-guardian-result.json")
    require(not result_path.exists() and not result_path.is_symlink(), "guardian result already exists")
    guardian = [sys.executable, "-I", str(GUARD_PATH), "--", sys.executable, "-I", str(GUARD_PATH),
                "--result-file", str(result_path), "--inherited-group-timeout",
                str(COMMAND_TIMEOUT_SECONDS), "--", *command]
    write_json(root / (label + "-command.json"), command)
    write_json(root / (label + "-guardian-command.json"), guardian)
    with (root / (label + "-stdout.jsonl")).open("xb") as stdout, \
            (root / (label + "-stderr.txt")).open("xb") as stderr:
        child, pending, cleaning = None, None, False
        handlers = {}

        def cancel(signum, _frame):
            nonlocal pending
            pending = signum
            # Do not interrupt Popen before it returns the child we must reap.
            if child is not None and not cleaning:
                raise SystemExit(128 + signum)

        try:
            for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
                handlers[signum] = signal.signal(signum, cancel)
            # Keep our direct guardian PID leased until wait, even under SIG_IGN.
            handlers[signal.SIGCHLD] = signal.signal(signal.SIGCHLD, signal.SIG_DFL)
            if pending is not None:
                raise SystemExit(128 + pending)
            # The HTTP server has threads. All preexec/group handling belongs in
            # the dedicated single-threaded guardian, never in this process.
            child = subprocess.Popen(guardian, cwd=root / "work", env=env,
                                     stdin=subprocess.PIPE if interact else subprocess.DEVNULL,
                                     stdout=subprocess.PIPE if interact else stdout, stderr=stderr)
            if pending is not None:
                raise SystemExit(128 + pending)
            if interact:
                interact(child, stdout)
                child.stdin.close()
                child.stdin = None
                trailing, _ = child.communicate()
                stdout.write(trailing)
                code = child.returncode
            else:
                code = child.wait()
        finally:
            cleaning = True
            try:
                if child is not None and child.returncode is None:
                    # Popen checks child ownership before relaying TERM: wait may
                    # have reaped it just before an interrupt prevented status storage.
                    # The guardian stops its group before exiting; never kill it.
                    try:
                        child.terminate()
                    except ProcessLookupError:
                        pass
                    child.wait()
                    # An interaction that raised left the reaped child's pipes open.
                    for pipe in (child.stdin, child.stdout):
                        if pipe is not None:
                            try:
                                pipe.close()
                            except OSError:
                                # Input the stopped child never read cannot be flushed.
                                # The pipe is closed all the same, and the error that
                                # ended the interaction is the one to report.
                                pass
            finally:
                for signum, handler in handlers.items():
                    signal.signal(signum, handler)
    if pending is not None:
        raise SystemExit(128 + pending)
    result = json.loads(result_path.read_text())
    require(isinstance(result, dict) and type(result.get("schema")) is int and result["schema"] == 1
            and result.get("argv") == command
            and type(result.get("timeout_seconds")) in (int, float)
            and result["timeout_seconds"] == COMMAND_TIMEOUT_SECONDS
            and type(result.get("timed_out")) is bool, "invalid guardian command evidence")
    raw = result.get("returncode")
    if result["timed_out"]:
        require(raw is None and code == 124, "guardian timeout evidence disagrees with exit")
        raise subprocess.TimeoutExpired(command, COMMAND_TIMEOUT_SECONDS)
    require(type(raw) is int and -signal.NSIG < raw < 256,
            "missing or invalid raw command status")
    require(code == (raw if raw >= 0 else 128 - raw), "guardian exit disagrees with raw command status")
    return raw


def sse(events):
    return "".join(f"event: {event['type']}\ndata: {json.dumps(event)}\n\n" for event in events).encode()


def response(response_id, item, usage=None):
    return sse([{"type": "response.created", "response": {"id": response_id}},
                {"type": "response.output_item.done", "item": item},
                {"type": "response.completed", "response": {"id": response_id, "usage": usage if usage is not None else {
                    "input_tokens": 0, "input_tokens_details": None, "output_tokens": 0,
                    "output_tokens_details": None, "total_tokens": 0}}}])


class Mock:
    def __init__(self, negative, quota=False, unknown_reset=False, refusal_type=None):
        self.negative = negative
        self.quota = quota
        self.unknown_reset = unknown_reset
        self.refusal_type = refusal_type
        self.requests = []
        self.lock = threading.Lock()

    def reply(self, body):
        with self.lock:
            number = len(self.requests) + 1
            record = {"at": time.monotonic(), "unixAt": time.time(), "body": body}
            self.requests.append(record)
        if self.negative or number == 2:
            status = 403 if self.negative else 503
            error = {"error": {"code": "server_is_overloaded", "message": "Model is at capacity."}}
            if self.quota:
                status = 403 if self.negative else 429
                record["quotaResetUnix"] = math.ceil(time.time()) + RETRY_SECONDS
                error = {"error": {"type": "usage_limit_reached",
                                   "message": "Fixture account quota exhausted"}}
                if not self.unknown_reset:
                    error["error"]["resets_at"] = record["quotaResetUnix"]
                if self.refusal_type:
                    status = 429
                    record["responseHeaders"] = {
                        "x-codex-rate-limit-reached-type": self.refusal_type}
            return status, json.dumps(error).encode(), "application/json", RETRY_SECONDS, record
        if number == 1:
            item = {"type": "function_call", "call_id": CALL_ID, "name": "update_plan",
                    "arguments": json.dumps({"plan": [{"step": PLAN, "status": "completed"}]})}
        elif number == 3:
            item = {"type": "message", "role": "assistant", "id": "recovered-message",
                    "content": [{"type": "output_text", "text": ANSWER}]}
        else:
            return 400, b'{"error":{"message":"Unexpected extra request"}}', "application/json", None, record
        return 200, response(f"response-{number}", item), "text/event-stream", None, record


def serve(mock):
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            request_started = time.monotonic()
            if self.path != "/v1/responses":
                self.send_error(404)
                return
            length = int(self.headers.get("content-length", "0"))
            if not 0 < length <= 8 * 1024 * 1024:
                self.send_error(413)
                return
            body = json.loads(self.rfile.read(length))
            status, data, content_type, retry_after, record = mock.reply(body)
            with mock.lock:
                record["requestStartedAt"] = request_started
            self.send_response(status)
            self.send_header("content-type", content_type)
            self.send_header("content-length", str(len(data)))
            if retry_after is not None:
                self.send_header("Retry-After", str(retry_after))
            for key, value in record.get("responseHeaders", {}).items():
                self.send_header(key, value)
            header_started = time.monotonic()
            self.end_headers()
            header_at = time.monotonic()
            # Bind timing to this exact response even if another request arrives.
            # Retry-After starts at headers; body transmission must not move it.
            with mock.lock:
                record.update(headerStartedAt=header_started, headerAt=header_at)
            self.wfile.write(data)

    server = provider_fixture.LoopbackHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, thread


def configure(root, port):
    config = f'''model = "{MODEL}"
model_provider = "capacity-smoke"
approval_policy = "never"
sandbox_mode = "read-only"
[tools.update_plan]
enabled = true
[model_providers.capacity-smoke]
name = "local capacity smoke"
base_url = "http://127.0.0.1:{port}/v1"
wire_api = "responses"
requires_openai_auth = false
request_max_retries = 0
stream_max_retries = 0
'''
    with (root / "codex-home/config.toml").open("x") as handle:
        handle.write(config)
