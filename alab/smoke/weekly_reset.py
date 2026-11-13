#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise automatic weekly reset credits with fake ChatGPT auth and loopback only.

Builds nothing. Supply --codex /absolute/binary or --package /absolute/package.
All cases retain commands, guardian outcomes, native events and HTTP traces.
"""
import argparse
import base64
import datetime
import hashlib
import http.server
import json
import os
from pathlib import Path
import re
import select
import signal
import subprocess
import sys
import threading
import time

import provider_fixture

MODEL = "gpt-5"
PROMPT = "Keep this weekly reset smoke task and its completed plan."
CALL_ID = "weekly-reset-plan"
PLAN = "Completed work survives an earned weekly reset"
PLAN2 = "Additional completed work survives another reset in the same turn"
CALL_ID2 = "weekly-reset-second-plan"
ANSWER = "Weekly reset recovery succeeded."
ACCOUNT = "weekly-smoke-account"
USER = "weekly-smoke-user"
COMMAND_TIMEOUT_SECONDS = 120
GUARD_PATH = Path(__file__).resolve().parents[1] / "command_guard.py"
OUTAGE_CASES = ("usage-outage", "credit-list-outage")
DEADLINE_OUTAGE_CASES = ("usage-outage-deadline", "credit-list-outage-deadline")
PROPAGATION_CASES = ("propagating-reset", "lost-ack-propagation")
TWO_CREDITS = ("stale-allowance-credit", *PROPAGATION_CASES, "repeated-same-cycle", "stale-credit-ledger", "selected-no-credit")
TWO_TOOLS = ("shared-credit", "repeated-same-cycle", "stale-credit-ledger", "unused-pending")
PREFLIGHT_CASES = ("preflight-five-hour", "preflight-weekly")
SUCCESS_CASES = ("spent-retry-deadline", *DEADLINE_OUTAGE_CASES, "stale-allowance-credit", *OUTAGE_CASES, "shared-credit", "natural-reset", "committed-ack", "stale-reset-credit", *PROPAGATION_CASES, "near-reset", "late-credit", "recovery", "lost-ack", "pending-old-cycle", "repeated-same-cycle", "unused-pending", "selected-no-credit", *PREFLIGHT_CASES)
CASE_NAMES = (*SUCCESS_CASES, "stale-credit-ledger", "stale-restored", "identity-mismatch", "no-credit", "unrelated-provider")

def require(condition, message):
    if not condition:
        raise RuntimeError(message)

def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

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
                                     stdin=subprocess.DEVNULL,
                                     stdout=subprocess.PIPE if interact else stdout, stderr=stderr)
            if pending is not None:
                raise SystemExit(128 + pending)
            if interact:
                interact(child, stdout)
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

def response(response_id, item):
    return sse([{"type": "response.created", "response": {"id": response_id}},
                {"type": "response.output_item.done", "item": item},
                {"type": "response.completed", "response": {"id": response_id, "usage": {
                    "input_tokens": 0, "input_tokens_details": None, "output_tokens": 0,
                    "output_tokens_details": None, "total_tokens": 0}}}])

def jwt():
    def segment(value):
        return base64.urlsafe_b64encode(json.dumps(value).encode()).rstrip(b"=").decode()
    return segment({"alg": "none"}) + "." + segment({
        "email": "fixture@example.invalid", "sub": USER, "exp": int(time.time()) + 86400,
        "https://api.openai.com/auth": {"chatgpt_account_id": ACCOUNT,
                                        "chatgpt_user_id": USER, "chatgpt_plan_type": "pro"}
    }) + ".fixture"


class Mock:
    def __init__(self, name):
        self.name = name
        reset_delay = 90 if name == "natural-reset" else 4 * 60 if name == "near-reset" else 24 * 3600
        self.reset = int(time.time()) + reset_delay
        self.credit = name not in ("no-credit", "late-credit", "natural-reset")
        self.consumed = name == "unused-pending"
        self.requests = []
        self.models = []
        self.lock = threading.Lock()
        self.checked = threading.Event()
        self.old_cycle = self.reset - 6 * 3600
        self.old_key = None
        self.account_key = None
        self.spent = []
        self.declined = []
        self.wall_usage_reads = 0
        self.propagation_reads = 0
        self.partial_receipt_path = None
        self.receipt_dir = None
        self.stale_allowance_reads = 0

    def usage(self):
        preflight = self.name in PREFLIGHT_CASES
        healthy = self.consumed or (not preflight and len(self.models) < 2)
        if len(self.models) >= 2:
            self.wall_usage_reads += 1
        reset = self.reset
        if self.name == "stale-allowance-credit" and len(self.models) >= 3 and len(self.spent) == 1:
            self.stale_allowance_reads += 1
            healthy = self.stale_allowance_reads == 1
            if not healthy:
                self.consumed = False
        if self.name == "natural-reset" and self.wall_usage_reads >= 2:
            healthy = True
            reset += 604800
        if self.name == "stale-reset-credit" and self.wall_usage_reads == 1:
            reset = int(time.time()) - 60
        if self.name in PROPAGATION_CASES and self.consumed:
            self.propagation_reads += 1
            healthy = self.propagation_reads >= (4 if self.name == "lost-ack-propagation" else 3)
            reset += 3600
        available = int(self.credit and not self.consumed)
        if self.name in TWO_CREDITS:
            available = 2 - len(self.spent) - len(self.declined)
            if self.name == "stale-credit-ledger" and len(self.spent) == 2:
                available = 2
        elif self.name == "unused-pending":
            available = int(not self.spent)
        return {"account_id": "other-account" if self.name == "identity-mismatch" else ACCOUNT,
                "user_id": USER, "plan_type": "pro",
                "rate_limit": {"allowed": healthy or preflight, "limit_reached": not (healthy or preflight),
                    "primary_window": {"used_percent": 95 if self.name == "preflight-five-hour" and not healthy else 0, "limit_window_seconds": 18000,
                        "reset_after_seconds": 3600, "reset_at": int(time.time()) + 3600},
                    "secondary_window": {"used_percent": 0 if healthy or self.name == "preflight-five-hour" else (95 if preflight else 100),
                        "limit_window_seconds": 604800,
                        "reset_after_seconds": reset - int(time.time()), "reset_at": reset}},
                "rate_limit_reset_credits": {"available_count": available}}

    def reply(self, method, path, body):
        with self.lock:
            record = {"method": method, "path": path, "body": body, "at": time.monotonic()}
            self.requests.append(record)
            if method == "POST" and path == "/v1/responses":
                self.models.append(body)
                number = len(self.models)
                if self.name in PREFLIGHT_CASES:
                    require(self.consumed and len(self.spent) == 1, "preflight did not redeem before model work")
                    if number == 2:
                        item = {"type": "message", "role": "assistant", "id": "preflight-answer",
                                "content": [{"type": "output_text", "text": ANSWER}]}
                        return 200, response("preflight-completed", item), "text/event-stream"
                if number == 1:
                    if self.name == "committed-ack":
                        receipt = json.loads(self.partial_receipt_path.read_text())
                        require(receipt["consumed"] is True and receipt["retry_released"] is True,
                                "first model request preceded settlement of the committed receipt")
                    item = {"type": "function_call", "call_id": CALL_ID, "name": "update_plan",
                            "arguments": json.dumps({"plan": [{"step": PLAN, "status": "completed"}]})}
                    return 200, response("weekly-plan", item), "text/event-stream"
                if number == 3 and self.name in TWO_TOOLS:
                    expected_spent = [] if self.name == "unused-pending" else ["earned-weekly-credit"]
                    require(self.consumed and self.spent == expected_spent,
                            "first weekly wall retried without its earned reset")
                    item = {"type": "function_call", "call_id": CALL_ID2, "name": "update_plan",
                            "arguments": json.dumps({"plan": [
                                {"step": PLAN, "status": "completed"},
                                {"step": PLAN2, "status": "completed"}]})}
                    return 200, response("weekly-second-plan", item), "text/event-stream"
                if (number == 2 or (number == 3 and self.name in ("stale-restored", "stale-allowance-credit", "spent-retry-deadline"))
                        or (number == 4 and self.name in TWO_TOOLS)
                        or (number == 5 and self.name == "stale-credit-ledger")):
                    if number in (4, 5):
                        self.consumed = False
                    if number == 4 and self.name == "shared-credit":
                        # A peer settles a fresh credit while this turn is sampling.
                        # Its account receipt must not consume this turn's retry.
                        state = json.loads((self.receipt_dir / "state.json").read_text())
                        generation = state["next_generation"]
                        peer_credit = "peer-earned-credit"
                        key = hashlib.sha256(
                            f"alab/weekly-reset/v3/{self.account_key}/{self.reset}/{peer_credit}/{generation}".encode()).hexdigest()
                        peer_path = self.receipt_dir / ("credit-" + hashlib.sha256(
                            peer_credit.encode()).hexdigest() + ".json")
                        write_json(peer_path, {"id": peer_credit, "consumed": True,
                                               "retry_released": True})
                        peer_path.chmod(0o600)
                        state["next_generation"] = generation + 1
                        state["current"] = {"credit": peer_credit, "cycle": self.reset,
                            "key": key, "generation": generation, "outcome": "consumed"}
                        peer_state = self.receipt_dir / "peer-state.json"
                        write_json(peer_state, state)
                        peer_state.chmod(0o600)
                        peer_state.replace(self.receipt_dir / "state.json")
                        self.consumed = True
                        record["peerResetCredit"] = peer_credit
                    if number == 3:
                        if self.name == "spent-retry-deadline":
                            self.reset = int(time.time()) + 3
                        self.checked.set()
                    if number == 2 and self.name in DEADLINE_OUTAGE_CASES:
                        self.reset = int(time.time()) + 3
                    elif number == 2 and self.name == "natural-reset":
                        # Backend restoration is observed on the paced second
                        # poll, before the original wall deadline takes over.
                        self.reset = int(time.time()) + 90
                    record["quotaResetUnix"] = self.reset
                    return 429, json.dumps({"error": {"type": "usage_limit_reached",
                        "message": "Fixture weekly quota exhausted", "resets_at": self.reset}}).encode(), "application/json"
                naturally_restored = self.name == "natural-reset" and self.wall_usage_reads >= 2
                if self.name in DEADLINE_OUTAGE_CASES:
                    record["serverUnix"] = int(time.time())
                    naturally_restored = record["serverUnix"] >= self.reset
                if ((number == 3 or (number == 5 and self.name in TWO_TOOLS)
                     or (number == 4 and self.name in ("stale-allowance-credit", "spent-retry-deadline")))
                        and (self.consumed or naturally_restored)):
                    if self.name == "spent-retry-deadline":
                        record["serverUnix"] = int(time.time())
                        require(record["serverUnix"] >= self.reset,
                                "native retry ran before its original reset deadline")
                    item = {"type": "message", "role": "assistant", "id": "weekly-answer",
                            "content": [{"type": "output_text", "text": ANSWER}]}
                    return 200, response("weekly-recovered", item), "text/event-stream"
                return 403, b'{"error":{"message":"Unexpected model retry"}}', "application/json"
            if method == "GET" and path == "/backend-api/wham/usage":
                value = self.usage()
                if ((self.name == "usage-outage" and self.wall_usage_reads == 1)
                        or (self.name == "usage-outage-deadline" and len(self.models) >= 2)):
                    record["status"] = 503
                    record["response"] = {"error": "temporary usage service outage"}
                    return 503, json.dumps(record["response"]).encode(), "application/json"
                if len(self.models) >= 2 and self.name != "stale-restored":
                    self.checked.set()
            elif method == "GET" and path == "/backend-api/wham/rate-limit-reset-credits":
                if ((self.name == "credit-list-outage" and sum(
                        request["path"] == path for request in self.requests) == 1)
                        or (self.name == "credit-list-outage-deadline" and len(self.models) >= 2)):
                    record["status"] = 503
                    record["response"] = {"error": "temporary credit service outage"}
                    return 503, json.dumps(record["response"]).encode(), "application/json"
                if self.name == "late-credit" and sum(
                        request["path"].endswith("/rate-limit-reset-credits")
                        for request in self.requests) >= 2:
                    self.credit = True
                value = {"available_count": int(self.credit), "credits": []}
                if self.credit:
                    value["credits"] = [{"id": "earned-weekly-credit", "reset_type": "codex_rate_limits",
                        "status": "available", "granted_at": "2026-01-01T00:00:00Z",
                        "expires_at": "2098-01-01T00:00:00Z"}]
                    if self.name in TWO_CREDITS:
                        value["credits"].append({"id": "earned-weekly-credit-2",
                            "reset_type": "codex_rate_limits", "status": "available",
                            "granted_at": "2026-01-01T00:00:00Z", "expires_at": "2099-01-01T00:00:00Z"})
                        if not (self.name == "stale-credit-ledger" and len(self.spent) == 2):
                            value["credits"] = [credit for credit in value["credits"]
                                                if credit["id"] not in self.spent + self.declined]
                        value["available_count"] = len(value["credits"])
            elif method == "POST" and path == "/backend-api/wham/rate-limit-reset-credits/consume":
                if self.name == "pending-old-cycle" and body.get("credit_id") == "previous-earned-credit":
                    require(body.get("redeem_request_id") == self.old_key,
                            "pending redemption did not retain the original key")
                    record["response"] = {"code": "already_redeemed", "windows_reset": 2}
                    return 200, json.dumps(record["response"]).encode(), "application/json"
                if self.name == "lost-ack-propagation" and self.consumed:
                    previous = next(request for request in self.requests[:-1]
                                    if request["path"].endswith("/consume"))
                    require(body == previous["body"], "uncertain reset changed its original key or credit")
                    record["response"] = {"code": "already_redeemed", "windows_reset": 2}
                    return 200, json.dumps(record["response"]).encode(), "application/json"
                require(self.name in (*SUCCESS_CASES, "stale-restored", "stale-credit-ledger"),
                        "ineligible case attempted to consume a credit")
                require(not self.consumed and self.credit, "credit consumed twice")
                require(set(body) == {"redeem_request_id", "credit_id"}, "unexpected consume payload")
                expected_credit = ("earned-weekly-credit-2" if self.spent or self.declined else "earned-weekly-credit")
                require(body["credit_id"] == expected_credit, "wrong or already spent credit selected")
                require(body["credit_id"] not in self.spent, "known consumed credit was posted again")
                require(isinstance(body["redeem_request_id"], str)
                        and re.fullmatch(r"[0-9a-f]{64}", body["redeem_request_id"]),
                        "missing stable SHA256 redeem ID")
                if self.name in (*TWO_TOOLS, "selected-no-credit", "committed-ack"):
                    generation = (1 if self.name == "unused-pending" else len(self.spent) + len(self.declined) + 1)
                    if self.name == "committed-ack":
                        generation += 1
                    expected_key = hashlib.sha256(
                        f"alab/weekly-reset/v3/{self.account_key}/{self.reset}/{expected_credit}/{generation}".encode()).hexdigest()
                    require(body["redeem_request_id"] == expected_key, "credit-specific stable key missing")
                if self.name == "selected-no-credit" and not self.declined:
                    self.declined.append(body["credit_id"])
                    record["response"] = {"code": "no_credit", "windows_reset": 0}
                    return 200, json.dumps(record["response"]).encode(), "application/json"
                self.consumed = True
                self.spent.append(body["credit_id"])
                value = {"code": "reset", "windows_reset": 2}
                if self.name in ("lost-ack", "lost-ack-propagation"):
                    record["droppedResponse"] = True
                    return None, b"", "application/json"
            else:
                return 404, b'{"error":"unexpected fixture endpoint"}', "application/json"
            record["response"] = value
            return 200, json.dumps(value).encode(), "application/json"


def serve(mock):
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def handle_request(self):
            length = int(self.headers.get("content-length", "0"))
            if not 0 <= length <= 8 * 1024 * 1024:
                self.send_error(413)
                return
            body = json.loads(self.rfile.read(length)) if length else None
            try:
                status, data, content_type = mock.reply(self.command, self.path, body)
            except Exception as error:
                with mock.lock:
                    mock.requests[-1]["fixtureError"] = str(error)
                status, data, content_type = 500, b'{"error":"fixture contract failed"}', "application/json"
            if status is None:
                # The backend committed the one redemption, then lost its HTTP
                # acknowledgement. Only a fresh usage read can release the retry.
                self.close_connection = True
                self.connection.shutdown(2)
                return
            self.send_response(status)
            self.send_header("content-type", content_type)
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            try:
                self.wfile.write(data)
            except (BrokenPipeError, ConnectionResetError):
                pass
        do_GET = handle_request
        do_POST = handle_request

    server = provider_fixture.LoopbackHTTPServer(("127.0.0.1", 0), Handler)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    return server, worker


def configure(root, port, openai_auth=True):
    (root / "codex-home/config.toml").write_text(f'''model = "{MODEL}"
model_provider = "weekly-smoke"
chatgpt_base_url = "http://127.0.0.1:{port}/backend-api"
approval_policy = "never"
sandbox_mode = "read-only"
[features]
enable_request_compression = false
[tools.update_plan]
enabled = true
[model_providers.weekly-smoke]
name = "{'OpenAI' if openai_auth else 'unrelated loopback provider'}"
base_url = "http://127.0.0.1:{port}/v1"
wire_api = "responses"
requires_openai_auth = {str(openai_auth).lower()}
request_max_retries = 0
stream_max_retries = 0
''')
    token = jwt()
    write_json(root / "codex-home/auth.json", {"auth_mode": "chatgpt", "tokens": {
        "id_token": token, "access_token": token, "refresh_token": "fixture-refresh",
        "account_id": ACCOUNT}, "last_refresh": datetime.datetime.now(datetime.timezone.utc).isoformat()})
    (root / "codex-home/auth.json").chmod(0o600)


def case(executable, output, name):
    root = output / name
    root.mkdir()
    env = fresh_environment(root)
    mock = Mock(name)
    server, worker = serve(mock)
    cancelled = False

    def cancel_after_check(child, stdout):
        nonlocal cancelled
        # A request appearing in the server log does not prove its response
        # reached recovery. A second paced poll witnesses a completed wait loop.
        retained = name in ("no-credit", "stale-credit-ledger", "stale-restored")

        def retained_wait():
            if not retained:
                return False
            with mock.lock:
                expected_models, expected_consumes = {"stale-restored": (3, 1),
                    "stale-credit-ledger": (5, 2)}.get(name, (2, 0))
                if (not mock.checked.is_set() or len(mock.models) != expected_models
                        or sum(request["path"].endswith("/consume") for request in mock.requests)
                        != expected_consumes):
                    return False
                last_model = max(request["at"] for request in mock.requests
                                 if request["path"] == "/v1/responses")
                endpoint = "/usage" if name == "stale-restored" else "/rate-limit-reset-credits"
                polls = [request for request in mock.requests
                         if request["path"].endswith(endpoint)
                         and request["at"] > last_model]
                return len(polls) >= 2 and polls[-1]["at"] - polls[0]["at"] >= 29

        deadline = time.monotonic() + 60
        buffer = b""
        while not (retained_wait() if retained else b"Waiting for account quota" in buffer):
            remaining = deadline - time.monotonic()
            require(remaining > 0, "retained recovery did not make a paced second poll" if retained
                    else "native quota wait notification missing")
            ready, _, _ = select.select([child.stdout], [], [], min(remaining, 1))
            if ready:
                data = os.read(child.stdout.fileno(), 65536)
                require(bool(data), "process exited before native quota wait")
                stdout.write(data)
                stdout.flush()
                buffer += data
                require(len(buffer) < 1024 * 1024, "oversized quota notification")
        with mock.lock:
            require(mock.checked.is_set() or name == "unrelated-provider",
                    "native wait skipped fresh weekly usage check")
            expected_models, expected_consumes = {"stale-restored": (3, 1),
                "stale-credit-ledger": (5, 2)}.get(name, (2, 0))
            require(len(mock.models) == expected_models,
                    "unexpected sampling before native quota wait")
            require(sum(request["path"].endswith("/consume") for request in mock.requests)
                    == expected_consumes, "unexpected consume before native wait")
        cancelled = child.poll() is None
        if cancelled:
            # The guardian retains/reaps this exact native PID; signal Codex so
            # both guardian layers can retain its handled failure status.
            os.kill(int((root / "native.pid").read_text()), signal.SIGINT)

    try:
        configure(root, server.server_port, openai_auth=name != "unrelated-provider")
        base = f"http://127.0.0.1:{server.server_port}/backend-api"
        account_key = hashlib.sha256(f"{base}\0{ACCOUNT}\0{USER}".encode()).hexdigest()
        mock.account_key = account_key
        mock.receipt_dir = root / "codex-home" / ("weekly-reset-" + account_key + ".d")
        if name in ("pending-old-cycle", "unused-pending", "committed-ack"):
            cycle = mock.old_cycle if name in ("pending-old-cycle", "committed-ack") else mock.reset
            credit = "previous-earned-credit" if name in ("pending-old-cycle", "committed-ack") else "earned-weekly-credit"
            mock.old_key = hashlib.sha256(
                f"alab/weekly-reset/v3/{account_key}/{cycle}/{credit}/1".encode()).hexdigest()
            receipt = root / "codex-home" / ("weekly-reset-" + account_key + ".d")
            receipt.mkdir(mode=0o700)
            state = {"version": 2 if name == "committed-ack" else 1,
                "next_generation": 2, "current": {"credit": credit,
                "cycle": cycle, "key": mock.old_key, "generation": 1,
                "outcome": "awaiting_ack" if name == "committed-ack" else "pending"}}
            write_json(receipt / "state.json", state)
            credit_file = receipt / ("credit-" + hashlib.sha256(credit.encode()).hexdigest() + ".json")
            mock.partial_receipt_path = credit_file
            write_json(credit_file, {"id": credit, "consumed": name == "committed-ack", "retry_released": False})
            (receipt / "state.json").chmod(0o600)
            credit_file.chmod(0o600)
        arguments = ["exec", "--json", "--color", "never", "--skip-git-repo-check",
                     "--cd", str(root / "work"), "--sandbox", "read-only", PROMPT]
        observed = name not in SUCCESS_CASES
        command_executable = executable
        if observed:
            command_executable = Path(sys.executable)
            arguments = [str(Path(__file__).resolve()), "--exec-wrapper", str(root / "native.pid"),
                         str(executable), *arguments]
        code = run(command_executable, arguments, root, env, "exec",
                   interact=cancel_after_check if observed else None)
    finally:
        server.shutdown()
        worker.join()
        server.server_close()
        write_json(root / "requests.json", mock.requests)
    events = [json.loads(line) for line in (root / "exec-stdout.jsonl").read_text().splitlines()]
    types = [event.get("type") for event in events]
    guardian = json.loads((root / "exec-guardian-result.json").read_text())
    require(guardian["timed_out"] is False, "guardian timed out")
    require(types.count("turn.started") == 1, "expected exactly one native turn")
    require(not any("fixtureError" in request for request in mock.requests), "HTTP fixture contract failed")
    require(len(mock.models) >= 2, "model never reached retained-tool quota wall")
    for body in mock.models:
        require(body.get("model") == MODEL, "model changed")
        user_texts = [part.get("text") for item in body.get("input", [])
                     if item.get("type") == "message" and item.get("role") == "user"
                     for part in item.get("content", []) if isinstance(part, dict)]
        require(user_texts.count(PROMPT) == 1, "original input lost or duplicated")
    account_requests = [request for request in mock.requests if request["path"].startswith("/backend-api/")]
    if name == "unrelated-provider":
        require(not account_requests, "unrelated provider consulted the stored ChatGPT account")
    else:
        require(any(request["path"] == "/backend-api/wham/usage" for request in mock.requests),
                "automatic weekly quota check missing")
    consumes = [request for request in mock.requests if request["path"].endswith("/consume")]
    if name in SUCCESS_CASES:
        expected_models = 2 if name in PREFLIGHT_CASES else (4 if name in ("stale-allowance-credit", "spent-retry-deadline") else 5 if name in TWO_TOOLS else 3)
        require(code == 0 and len(mock.models) == expected_models, "weekly reset did not recover")
        expected_consumes = 2 if name in ("stale-allowance-credit", "lost-ack-propagation", "pending-old-cycle", "repeated-same-cycle", "selected-no-credit") else 1
        if name == "natural-reset" or name in DEADLINE_OUTAGE_CASES:
            expected_consumes = 0
        require(len(consumes) == expected_consumes
                and (mock.consumed or name == "natural-reset" or name in DEADLINE_OUTAGE_CASES),
                "earned credit not consumed exactly once after pending reconciliation")
        if name in DEADLINE_OUTAGE_CASES:
            models = [request for request in mock.requests if request["path"] == "/v1/responses"]
            failures = [request for request in mock.requests if request.get("status") == 503]
            require(models[-1]["serverUnix"] >= models[-2]["quotaResetUnix"],
                    "outage retried the model before the native reset deadline")
            require(models[-1]["at"] - models[-2]["at"] < 20,
                    "backend outage held the native deadline until another polling interval")
            require(failures and all(models[-2]["at"] < request["at"] < models[-1]["at"]
                                     for request in failures),
                    "fixture did not retain the backend outage through reset recovery")
            require(not mock.spent and not mock.consumed,
                    "native reset deadline spent an earned credit")
        if name == "spent-retry-deadline":
            models = [request for request in mock.requests if request["path"] == "/v1/responses"]
            require(models[-1]["serverUnix"] >= models[-2]["quotaResetUnix"],
                    "spent-grant wait stranded the native reset deadline")
            require(mock.models[2]["input"] == mock.models[3]["input"],
                    "native deadline handoff lost the retained request")
        if name == "stale-allowance-credit":
            denied_at = [request["at"] for request in mock.requests if "quotaResetUnix" in request][-1]
            reads = [request for request in mock.requests if request["path"].endswith("/usage")
                     and denied_at < request["at"] < consumes[1]["at"]]
            require(len(reads) == 2 and reads[1]["at"] - reads[0]["at"] >= 29,
                    "spent retry did not retain a paced usage poll")
            require(reads[0]["response"]["rate_limit"]["allowed"] is True
                    and reads[1]["response"]["rate_limit"]["allowed"] is False,
                    "fixture did not reconcile stale healthy usage with current exhaustion")
            require([request["body"]["credit_id"] for request in consumes]
                    == ["earned-weekly-credit", "earned-weekly-credit-2"],
                    "current exhaustion did not redeem the next earned credit")
            require(mock.models[2]["input"] == mock.models[3]["input"],
                    "retained turn changed while stale usage converged")
        if name in OUTAGE_CASES:
            failures = [request for request in mock.requests if request.get("status") == 503]
            require(len(failures) == 1, "fixture did not inject one transient outage")
            failed = failures[0]
            retry = next(request for request in mock.requests
                         if request["path"] == failed["path"] and request["at"] > failed["at"])
            require(retry["at"] - failed["at"] >= 29,
                    "transient endpoint failure did not retain a paced poll")
            if name == "usage-outage":
                require(retry["response"]["rate_limit"]["allowed"] is False,
                        "usage outage retry did not verify current exhaustion")
                require(all(request["at"] > retry["at"] for request in mock.requests
                            if request["path"].endswith("/rate-limit-reset-credits")),
                        "failed usage authorized a credit listing")
            else:
                require(retry["response"]["available_count"] == 1,
                        "credit outage retry did not return a usable credit")
            require(consumes[0]["at"] > retry["at"],
                    "failed endpoint response authorized a redemption")
        if name == "shared-credit":
            peer = [request for request in mock.requests if "peerResetCredit" in request]
            require(len(peer) == 1 and mock.spent == ["earned-weekly-credit"],
                    "shared reset must resume without redeeming the peer's credit")
            require(consumes[0]["at"] < peer[0]["at"],
                    "fixture did not settle the shared reset after the turn's first retry")
            peer_usage = [request for request in mock.requests
                          if request["path"].endswith("/usage") and request["at"] > peer[0]["at"]]
            retried = [request for request in mock.requests if request["path"] == "/v1/responses"][-1]
            require(any(request["response"]["rate_limit"]["allowed"] is True
                        and request["at"] < retried["at"] for request in peer_usage),
                    "shared retry did not verify fresh restored allowance")
        if name == "natural-reset":
            usage = [request for request in mock.requests
                     if request["path"].endswith("/usage")
                     and request["at"] > next(request["at"] for request in mock.requests
                                              if "quotaResetUnix" in request)]
            lists = [request for request in mock.requests
                     if request["path"].endswith("/rate-limit-reset-credits")]
            require(len(usage) == 2 and usage[1]["at"] - usage[0]["at"] >= 29,
                    "natural reset did not retain a paced usage poll")
            require(usage[0]["response"]["rate_limit"]["allowed"] is False
                    and usage[1]["response"]["rate_limit"]["allowed"] is True,
                    "fixture did not restore the exhausted allowance")
            require(len(lists) == 1 and lists[0]["response"]["available_count"] == 0,
                    "natural reset fixture offered or queried another credit")
            require(not mock.spent and not mock.consumed,
                    "natural reset was mistaken for a credit redemption")
        if name == "committed-ack":
            require(consumes[0]["body"]["credit_id"] == "earned-weekly-credit"
                    and consumes[0]["body"]["redeem_request_id"] != mock.old_key,
                    "committed receipt was replayed instead of selecting the fresh credit")
        if name == "late-credit":
            lists = [request for request in mock.requests
                     if request["path"].endswith("/rate-limit-reset-credits")]
            require(len(lists) == 2 and lists[1]["at"] - lists[0]["at"] >= 29,
                    "late credit did not require a paced second poll")
            require(lists[0]["response"]["available_count"] == 0
                    and lists[1]["response"]["available_count"] == 1,
                    "fixture did not grant a new credit after the first check")
        if name == "stale-reset-credit":
            denied = [request for request in mock.requests
                      if request["path"].endswith("/usage")
                      and request.get("response", {}).get("rate_limit", {}).get("allowed") is False]
            lists = [request for request in mock.requests
                     if request["path"].endswith("/rate-limit-reset-credits")]
            require(len(denied) == 2 and denied[1]["at"] - denied[0]["at"] >= 29,
                    "stale reset did not retain a paced usage poll")
            require(denied[0]["response"]["rate_limit"]["secondary_window"]["reset_after_seconds"] < 0
                    and denied[1]["response"]["rate_limit"]["secondary_window"]["reset_after_seconds"] > 0,
                    "fixture did not advance stale usage to a fresh cycle")
            require(len(lists) == 1 and lists[0]["at"] > denied[1]["at"]
                    and consumes[0]["at"] > lists[0]["at"],
                    "stale usage authorized a credit list or redemption")
        if name in PROPAGATION_CASES:
            denied = [request for request in mock.requests
                      if request["path"].endswith("/usage") and request["at"] > consumes[0]["at"]
                      and request.get("response", {}).get("rate_limit", {}).get("allowed") is False]
            require(len(denied) == (3 if name == "lost-ack-propagation" else 2)
                    and mock.spent == ["earned-weekly-credit"],
                    "delayed usage propagation spent an additional reset")
            require(all(request["response"]["rate_limit"]["secondary_window"]["reset_at"] > mock.reset
                        for request in denied), "fixture did not advance the reset while usage lagged")
            retried = [request for request in mock.requests if request["path"] == "/v1/responses"][-1]
            require(retried["at"] - consumes[0]["at"] >= 59,
                    "sampler resumed before the two paced propagation polls")
        if name == "lost-ack-propagation":
            require(consumes[0].get("droppedResponse") is True
                    and consumes[1]["response"]["code"] == "already_redeemed"
                    and consumes[0]["body"] == consumes[1]["body"],
                    "lost acknowledgement did not reconcile the exact same reset")
            require(sum(request["path"].endswith("/rate-limit-reset-credits")
                        for request in mock.requests) == 1,
                    "uncertain recent reset queried another credit while usage lagged")
        if name == "pending-old-cycle":
            require(consumes[0]["body"] == {"redeem_request_id": mock.old_key,
                    "credit_id": "previous-earned-credit"}, "older pending key was abandoned")
            require(consumes[0].get("response", {}).get("code") == "already_redeemed",
                    "older credit was not reconciled as previously redeemed")
            require(consumes[1]["body"]["credit_id"] == "earned-weekly-credit",
                    "current cycle did not redeem the new earned credit")
        if name == "lost-ack":
            require(consumes[0].get("droppedResponse") is True,
                    "consume acknowledgement was not actually dropped")
            ledger = json.loads((root / "codex-home" / ("weekly-reset-" + account_key + ".d")
                                 / "state.json").read_text())
            require(ledger["version"] == 2 and ledger["current"]["outcome"] == "restored_pending"
                    and ledger["current"]["key"] == consumes[0]["body"]["redeem_request_id"],
                    "healthy allowance did not preserve the uncertain receipt and its restoration witness")
        if name == "selected-no-credit":
            require(consumes[0].get("response", {}).get("code") == "no_credit"
                    and consumes[1].get("response", {}).get("code") == "reset"
                    and mock.spent == ["earned-weekly-credit-2"],
                    "selected NoCredit did not release the next available credit")
            require(any(request["path"].endswith("/usage")
                        and request.get("response", {}).get("rate_limit", {}).get("allowed") is False
                        and consumes[0]["at"] < request["at"] < consumes[1]["at"] for request in mock.requests),
                    "NoCredit was not reconciled against fresh denied usage")
        if name == "unused-pending":
            require(mock.spent == ["earned-weekly-credit"], "external restore excluded an unspent earned credit")
            require(consumes[0]["body"]["redeem_request_id"] == mock.old_key,
                    "new depletion abandoned the original ambiguous redemption key")
        if name in PREFLIGHT_CASES:
            first_model = next(request for request in mock.requests if request["path"] == "/v1/responses")
            require(consumes[0]["at"] < first_model["at"], "reset happened after model work started")
            require(not any("quotaResetUnix" in request for request in mock.requests), "preflight hit a quota wall")
        else:
            require(mock.models[1]["input"] == mock.models[2]["input"], "retry changed retained tool/input")
        if name in TWO_TOOLS:
            require(mock.models[3]["input"] == mock.models[4]["input"], "second retry changed retained input")
        if name == "repeated-same-cycle":
            require([request["body"]["credit_id"] for request in consumes]
                    == ["earned-weekly-credit", "earned-weekly-credit-2"], "two fresh credits not spent once each")
            require(consumes[0]["body"]["redeem_request_id"] != consumes[1]["body"]["redeem_request_id"],
                    "same cycle reused one redemption key for different credits")
            model_records = [request for request in mock.requests if request["path"] == "/v1/responses"]
            require(model_records[1]["quotaResetUnix"] == model_records[3]["quotaResetUnix"],
                    "fixture did not exercise the same natural reset cycle")
            for consume, retry in zip(consumes, (model_records[2], model_records[4])):
                require(any(request["path"].endswith("/usage")
                            and request.get("response", {}).get("rate_limit", {}).get("allowed") is True
                            and consume["at"] < request["at"] < retry["at"] for request in mock.requests),
                        "quota retry did not verify restored usage after its own reset")
        outputs = [item for item in mock.models[-1]["input"] if item.get("type") == "function_call_output"]
        expected_calls = {CALL_ID, CALL_ID2} if name in TWO_TOOLS else {CALL_ID}
        require({item.get("call_id") for item in outputs} == expected_calls
                and all("Plan updated" in str(item.get("output")) for item in outputs),
                "completed plan outputs missing")
        require(types.count("turn.completed") == 1 and "turn.failed" not in types, "native turn failed")
        require(any(event.get("item", {}).get("text") == ANSWER for event in events), "reply missing")
        restored_after = consumes[-1]["at"] if consumes else next(
            request["at"] for request in mock.requests if "quotaResetUnix" in request)
        require(name in DEADLINE_OUTAGE_CASES or any(request["path"].endswith("/usage")
                    and request.get("response", {}).get("rate_limit", {}).get("allowed") is True
                    and request["at"] > restored_after for request in mock.requests),
                "restored usage was not verified")
        plans = [event for event in events if event.get("item", {}).get("type") == "todo_list"]
        expected_plan = [{"text": PLAN, "completed": True}]
        if name in TWO_TOOLS:
            expected_plan.append({"text": PLAN2, "completed": True})
        else:
            require([event["type"] for event in plans] == ["item.started", "item.completed"],
                    "completed plan was executed more than once")
        require(plans[-1]["item"]["items"] == expected_plan, "completed plan was lost")
    else:
        if name == "stale-credit-ledger":
            require(len(mock.models) == 5 and len(consumes) == 2
                    and mock.spent == ["earned-weekly-credit", "earned-weekly-credit-2"],
                    "stale credit details caused reuse of a consumed credit")
            require(mock.models[1]["input"] == mock.models[2]["input"]
                    and mock.models[3]["input"] == mock.models[4]["input"], "retained tool inputs changed")
        elif name == "stale-restored":
            require(len(consumes) == 1 and mock.consumed, "stale usage spent multiple credits")
            require(len(mock.models) == 3, "stale restored usage caused a sampling retry loop")
            require(mock.models[1]["input"] == mock.models[2]["input"],
                    "stale usage recovery changed retained tool/input")
        else:
            require(not consumes and not mock.consumed, "ineligible credit consumed")
            require(len(mock.models) == 2, "ineligible wall retried before reset")
        require(code != 0 and "turn.completed" not in types, "ineligible case reported success")
    result = {"case": name, "passed": True, "exit": code, "cancelled": cancelled,
              "guardianTimedOut": guardian["timed_out"], "rawReturncode": guardian["returncode"],
              "modelRequests": len(mock.models), "consumeRequests": len(consumes), "events": types}
    write_json(root / "result.json", result)
    return result


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "--exec-wrapper":
        with Path(sys.argv[2]).open("x") as pidfile:
            pidfile.write(str(os.getpid()))
        os.execv(sys.argv[3], sys.argv[3:])
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--codex", type=Path)
    source.add_argument("--package", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--case", action="append", choices=CASE_NAMES,
                        help="Run selected cases only; repeat to retain a subset after a fixture failure")
    args = parser.parse_args()
    selected = list(args.case or CASE_NAMES)
    if len(selected) != len(set(selected)):
        parser.error("--case must not repeat a case")
    output = args.output.absolute()
    output.mkdir(parents=True, exist_ok=False)
    report = {"passed": False, "fullSuite": False, "requestedCases": selected,
              "cases": [], "scope": "isolated fake ChatGPT auth and loopback only; no compilation"}
    interrupted = None
    try:
        if args.package:
            package = args.package.resolve(strict=True)
            build, manifest, executable = package_records(package)
            report.update(package=str(package), source=build["source"], packageRecords={"build": build, "manifest": manifest})
        else:
            executable = args.codex.resolve(strict=True)
        require(executable.is_file(), "Codex executable missing")
        report.update(executable=str(executable), binarySha256=file_sha256(executable))
        for name in selected:
            report["failedCase"] = name
            report["cases"].append(case(executable, output, name))
        del report["failedCase"]
        report["passed"] = True
        report["fullSuite"] = set(selected) == set(CASE_NAMES)
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
    except (KeyboardInterrupt, SystemExit) as error:
        # case/run have already unwound their child and server cleanup. Keep
        # incomplete evidence without converting cancellation into a normal exit.
        interrupted = error
        report["interrupted"] = True
        report["error"] = f"{type(error).__name__}: {error}"
    report["probeSha256"] = file_sha256(Path(__file__))
    write_json(output / "result.json", report)
    print(json.dumps(report, indent=2))
    if interrupted is not None:
        raise interrupted
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
