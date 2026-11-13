#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise separate account files in concurrent real Codex exec processes."""

import argparse
import hashlib
import http.server
import json
import subprocess
import sys
import threading
import time
from pathlib import Path

import capacity_exec as probe
import provider_fixture
from renewal_follow import bearer_generation, fake_auth, fixture_tls, request_kind


def prompt(index):
    return f"Isolated parallel login {index}"


def answer(index):
    return f"parallel login {index} succeeded"


class Accounts:
    def __init__(self, count):
        self.count = count
        self.condition = threading.Condition()
        self.arrived = set()
        self.requests = []
        self.errors = []
        self.aborted = False

    def identity(self, headers):
        generation = bearer_generation(headers)
        if generation is None or not generation.isdigit():
            raise ValueError("unrecognized fixture credential")
        index = int(generation)
        if not 0 <= index < self.count:
            raise ValueError("fixture credential outside account set")
        if headers.get("ChatGPT-Account-Id") != f"parallel-account-{index}":
            raise ValueError("credential and account header disagree")
        return index

    def respond(self, headers, body):
        index = self.identity(headers)
        kind = request_kind(body)
        if kind == "thread-title":
            text = json.dumps({"title": f"Parallel {index}"})
        else:
            texts = [part.get("text") for item in body.get("input", [])
                     if item.get("role") == "user" for part in item.get("content", [])
                     if isinstance(part, dict)]
            if texts.count(prompt(index)) != 1:
                raise ValueError("credential does not belong to the requested task")
            with self.condition:
                self.requests.append({"account": index, "at": time.time()})
                self.arrived.add(index)
                self.condition.notify_all()
                ready = self.condition.wait_for(
                    lambda: len(self.arrived) == self.count or self.aborted, timeout=120)
                if not ready or self.aborted:
                    raise ValueError("not all account requests were concurrently in flight")
            text = answer(index)
        item = {"type": "message", "role": "assistant", "id": f"parallel-{index}",
                "content": [{"type": "output_text", "text": text}]}
        return probe.response(f"parallel-{index}", item)


def server_for(accounts, root):
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            body = {"models": []}
            if self.path.split("?", 1)[0].endswith("/accounts/check"):
                index = accounts.identity(self.headers)
                account = f"parallel-account-{index}"
                body = {"accounts": [{"id": account, "can_access_with_session": True,
                        "is_deactivated": False, "plan_type": "pro", "name": "Parallel fixture",
                        "workspace_backend_origin": "NO_CONSTRAINT",
                        "account_routing_override": "NO_CONSTRAINT"}],
                        "account_ordering": [account], "default_account_id": account}
            self.send_response(200)
            self.send_header("content-type", "application/json")
            data = json.dumps(body).encode()
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_POST(self):
            try:
                if self.path != "/v1/responses":
                    raise ValueError("unexpected endpoint")
                length = int(self.headers.get("content-length", "0"))
                if not 0 < length <= 8 * 1024 * 1024:
                    raise ValueError("unbounded request")
                data = accounts.respond(self.headers, json.loads(self.rfile.read(length)))
                self.send_response(200)
                self.send_header("content-type", "text/event-stream")
                self.send_header("content-length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
            except (ValueError, OSError) as error:
                with accounts.condition:
                    accounts.errors.append(str(error))
                self.send_error(400)

    class Server(provider_fixture.LoopbackHTTPServer):
        request_queue_size = max(128, 2 * accounts.count)

    server = Server(("127.0.0.1", 0), Handler)
    server.socket = fixture_tls(root).wrap_socket(server.socket, server_side=True)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, thread


def run_worker(root, index, executable):
    directory = root / "clients" / str(index)
    env = json.loads((directory / "environment.json").read_text())
    return probe.run(executable, ["exec", "--json", "--color", "never",
        "--skip-git-repo-check", "--cd", str(root / "work"), "--sandbox", "read-only",
        prompt(index)], directory, env, "exec")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--count", type=int, default=40)
    parser.add_argument("--worker", type=int, help=argparse.SUPPRESS)
    parser.add_argument("--executable", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    root = args.output.resolve()
    if args.worker is not None:
        if args.executable is None:
            parser.error("--worker requires --executable")
        return run_worker(root, args.worker, args.executable.resolve(strict=True))
    if args.package is None:
        parser.error("--package is required")
    probe.require(1 <= args.count <= 128, "qualification count must be between 1 and 128")
    package = args.package.resolve(strict=True)
    build, manifest, executable = probe.package_records(package)
    root.mkdir(parents=True, exist_ok=False)
    env = probe.fresh_environment(root)
    accounts = Accounts(args.count)
    server, thread = server_for(accounts, root)
    processes, handles, auth_hashes, thread_ids = [], [], {}, []
    executable_hash = hashlib.sha256(executable.read_bytes()).hexdigest()
    report = {"passed": False, "source": build["source"], "version": manifest["version"],
              "accountCount": args.count, "sharedConversationStorage": env["CODEX_HOME"],
              "executableSha256": executable_hash}
    try:
        probe.configure(root, server.server_port)
        config = root / "codex-home/config.toml"
        text = config.read_text().replace("requires_openai_auth = false", "requires_openai_auth = true")
        text = text.replace('name = "local capacity smoke"', 'name = "OpenAI"')
        text = text.replace("http://127.0.0.1:", "https://127.0.0.1:")
        text = text.replace("[tools.update_plan]", "[features]\nenable_request_compression = false\n[tools.update_plan]")
        config.write_text(f'cli_auth_credentials_store = "file"\nchatgpt_base_url = "https://127.0.0.1:{server.server_port}"\n' + text)
        for index in range(args.count):
            directory = root / "clients" / str(index)
            (directory / "work").mkdir(parents=True)
            auth = directory / "auth/auth.json"
            fake_auth(auth, str(index), f"https://127.0.0.1:{server.server_port}",
                      f"parallel-account-{index}", email=f"parallel-{index}@example.invalid",
                      user=f"parallel-user-{index}")
            auth_hashes[auth] = hashlib.sha256(auth.read_bytes()).hexdigest()
            worker_env = dict(env, CODEX_AUTH_PATH=str(auth),
                              CODEX_CA_CERTIFICATE=str(root / "fixture-ca.pem"))
            probe.write_json(directory / "environment.json", worker_env)
            output = (directory / "worker-output.txt").open("xb")
            handles.append(output)
            processes.append(subprocess.Popen([sys.executable, str(Path(__file__).resolve()),
                "--output", str(root), "--worker", str(index), "--executable", str(executable)],
                env=env, stdout=output, stderr=subprocess.STDOUT))
        deadline = time.monotonic() + 240
        for index, process in enumerate(processes):
            probe.require(process.wait(timeout=max(1, deadline - time.monotonic())) == 0,
                          f"account {index} failed; see its worker/exec logs")
            output = (root / "clients" / str(index) / "exec-stdout.jsonl").read_text()
            probe.require(answer(index) in output, f"account {index} received no matching answer")
            events = [json.loads(line) for line in output.splitlines()]
            threads = [event.get("thread_id") for event in events if event.get("type") == "thread.started"]
            probe.require(len(threads) == 1 and isinstance(threads[0], str),
                          f"account {index} has no unique conversation start")
            thread_ids.append(threads[0])
        probe.require(accounts.arrived == set(range(args.count)), "not all accounts reached the barrier")
        probe.require(len(accounts.requests) == args.count and not accounts.errors,
                      "credential mismatch, duplicate request or incomplete concurrency barrier")
        probe.require(not (root / "codex-home/auth.json").exists(), "a login leaked into shared storage")
        probe.require(len(set(thread_ids)) == args.count, "separate processes reused a conversation")
        probe.require(hashlib.sha256(executable.read_bytes()).hexdigest() == executable_hash,
                      "the tested executable changed during qualification")
        for path, expected in auth_hashes.items():
            probe.require(hashlib.sha256(path.read_bytes()).hexdigest() == expected,
                          "a separate login was overwritten")
        report.update(passed=True, concurrentRequests=len(accounts.arrived),
                      credentialFilesUnchanged=True, threadIds=thread_ids, requests=accounts.requests)
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
    finally:
        with accounts.condition:
            accounts.aborted = True
            accounts.condition.notify_all()
        for process in processes:
            if process.poll() is None:
                process.terminate()
        for process in processes:
            try:
                process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        for handle in handles:
            handle.close()
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        report["serverErrors"] = accounts.errors
        probe.write_json(root / "result.json", report)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
