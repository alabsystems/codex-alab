#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Continue one signed Codex terminal conversation across a remote Unix socket account change."""

import argparse
import base64
from contextlib import ExitStack
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

import account_resume
import capacity_exec as fixture
import model_resume as terminal
from ordinary_account_renewal import fingerprint
from renewal_follow import fixture_tls
from terminal_controls import read_runs


LEGS = (("source", "source"), ("destination", "destination"), ("returned", "source"))
EMAILS = {"source": "round.one@example.invalid", "destination": "round.two@example.invalid"}
DIALOGUE = (
    (terminal.BIRCH_SEED, terminal.BIRCH_REPLY),
    (terminal.BIRCH_CONTINUE, terminal.BIRCH_ANSWER),
    ("Back on the first login, recall both replies.", "Both birch replies survived the return."),
)


def expected_messages(index):
    expected = []
    for prompt, answer in DIALOGUE[:index + 1]:
        expected.extend([("user", [("input_text", prompt)]), ("assistant", [("output_text", answer)])])
    return expected[:-1]


class ReconnectGateway(account_resume.Gateway):
    def response(self, body, headers):
        messages = [(item["role"], [(part.get("type"), part.get("text")) for part in item.get("content", [])])
                    for item in body.get("input", []) if item.get("role") in ("user", "assistant")]
        # Codex prepends its local environment to the original user prompt.
        if (messages and messages[0][0] == "user" and len(messages[0][1]) == 1
                and messages[0][1][0][0] == "input_text"
                and (messages[0][1][0][1] or "").startswith("<environment_context>\n")):
            messages = messages[1:]
        with self.lock:
            login, account = self.account_route(headers)
            thread = headers.get("thread-id")
            turn = json.loads(headers.get("x-codex-turn-metadata", "{}")).get("turn_id")
            self.requests.append({"thread": thread, "turn": turn, "login": login, "account": account,
                                  "messages": messages})
            self.events.append({"event": "workspace_request", "thread": thread, "turn": turn})
        last = messages[-1] if messages else None
        answer = next((reply for prompt, reply in DIALOGUE if last == ("user", [("input_text", prompt)])),
                      "Unexpected reconnect fixture input.")
        return fixture.response("reconnect", {"type": "message", "id": "reconnect", "role": "assistant",
                                             "content": [{"type": "output_text", "text": answer}]})

    def confirmed_turn(self, index, turn):
        leg, label = LEGS[index]
        with self.lock:
            rows = list(self.requests)
        fixture.require(len(rows) == 1 and rows[0]["turn"] == turn,
                        f"expected exactly one model request on the {leg} listener")
        row = rows[0]
        fixture.require((row["login"], row["account"]) == (label, "workspace-" + label),
                        f"{leg} inference used the wrong credential or workspace")
        fixture.require(row["messages"] == expected_messages(index), f"{leg} inference changed the conversation history")
        return {"thread": row["thread"], "turn": turn, "login": label, "account": row["account"],
                "history": [parts[0][1] for _, parts in row["messages"]]}


def current_status(runs, email, thread):
    # The latest card must itself contain both fields. Repainting an earlier
    # matching card next to a new incomplete card cannot satisfy this read.
    _, heading, card = runs.rpartition(b"/statuscodex-alab")
    account = card.partition(b"Account:")[2]
    session = account.partition(b"Session:")[2]
    return bool(heading and account.startswith(email.encode() + b"(") and session.startswith(thread.encode()))


def stop_server(server):
    if server.poll() is None:
        server.terminate()
        try:
            server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait(timeout=5)


def exercise(binary, root, env, backends, slots, report):
    sock = root / "backend.sock"
    endpoint = f"unix://{sock}"
    raw = root / "terminal.raw"
    custody = root / "terminal-custody.json"
    argv = [str(binary), "--no-alt-screen", "--remote", endpoint, terminal.BIRCH_SEED]
    spec = root / "tmp/terminal.json"
    fixture.write_json(spec, {"argv": argv, "cwd": str(root / "work"), "env": env,
                              "raw": str(raw), "result": str(custody)})

    with ExitStack() as stack:
        def start(leg, label):
            port, _gateway = backends[leg]
            terminal.configure(root, port, "ultra", tls=True)
            config = root / "codex-home/config.toml"
            config.write_text('cli_auth_credentials_store = "file"\n' + config.read_text())
            server = subprocess.Popen([binary, "app-server", "--listen", endpoint], cwd=root / "work",
                                      env=dict(env, CODEX_AUTH_PATH=str(slots[label])),
                                      stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            stack.callback(stop_server, server)
            return server

        source = start("source", "source")
        deadline = time.monotonic() + 15
        while not sock.is_socket():
            fixture.require(source.poll() is None and time.monotonic() < deadline, "source socket did not open")
            time.sleep(.05)

        @terminal.with_pty_lifeline
        def interact(process, _stdout):
            deadline = time.monotonic() + 75
            sequence = 0

            def command(**fields):
                nonlocal sequence
                sequence += 1
                process.stdin.write((json.dumps({"id": sequence, **fields}) + "\n").encode())
                process.stdin.flush()

            def send(data):
                command(write=base64.b64encode(data).decode())

            def wait_paint(offset, matches, expected):
                while not matches(read_runs(raw, offset)):
                    fixture.require(process.poll() is None and time.monotonic() < deadline,
                                    f"terminal did not display {expected}")
                    time.sleep(.05)

            def displayed(offset, text):
                needle = b"".join(text.encode().split())
                wait_paint(offset, lambda runs: needle in runs, text)

            def submit(text):
                offset = raw.stat().st_size
                send(b"\x1b[200~" + text.encode() + b"\x1b[201~")
                displayed(offset, text)
                offset = raw.stat().st_size
                send(b"\r")
                return offset

            def check_turn(index, leg, offset):
                gateway = backends[leg][1]
                answer = DIALOGUE[index][1]
                displayed(offset, answer)
                turn = terminal.wait_for_turn_completion(root, gateway, process, answer, deadline,
                                                         event_kind="workspace_request")
                return gateway.confirmed_turn(index, turn)

            def status(label, thread):
                email = EMAILS[label]
                # Purge the old cards before submitting a fresh status command, including on the return to A.
                clear_at = raw.stat().st_size
                send(b"\x0c")
                while True:
                    with raw.open("rb") as trace:
                        trace.seek(clear_at)
                        if b"\x1b[2J\x1b[3J" in trace.read():
                            break
                    fixture.require(process.poll() is None and time.monotonic() < deadline,
                                    "terminal did not clear the previous status display")
                    if b"Ctrl+Lisdisabledwhileataskisinprogress" in read_runs(raw, clear_at):
                        # The rollout can finish before the terminal returns to idle.
                        time.sleep(.15)
                        clear_at = raw.stat().st_size
                        send(b"\x0c")
                    time.sleep(.05)

                wait_paint(submit("/status"), lambda runs: current_status(runs, email, thread),
                           f"status card for {email} on {thread}")
                report.setdefault("status_emails", []).append(email)

            started = json.loads(process.stdout.readline()).get("started")
            fixture.require(type(started) is int and started > 0, "missing original terminal PID")
            report["terminal_pid"] = started
            command(resize=[32, 228])
            server = source
            report["server_pids"] = [source.pid]
            for index, (leg, label) in enumerate(LEGS):
                if index:
                    offset = raw.stat().st_size
                    stop_server(server)
                    displayed(offset, "Reconnecting to server")
                    server = start(leg, label)
                    fixture.require(server.pid not in report["server_pids"], "expected a new server process")
                    report["server_pids"].append(server.pid)
                    displayed(offset, EMAILS[label])
                    offset = submit(DIALOGUE[index][0])
                else:
                    offset = 0
                result = check_turn(index, leg, offset)
                if index:
                    fixture.require(result["thread"] == report["source"]["thread"]
                                    and all(result["turn"] != report[prior]["turn"] for prior, _ in LEGS[:index]),
                                    "replacement did not continue the original thread in a new turn")
                report[leg] = result
                status(label, result["thread"])
            for timeout in (1, 12):
                send(b"\x03")
                try:
                    process.wait(timeout=timeout)
                    break
                except subprocess.TimeoutExpired:
                    if timeout == 12:
                        raise

        guardian = Path(__file__).with_name("history_pagination.py")
        args = ["-I", "-B", "-S", str(guardian), "--guardian", str(spec)]
        def record_custody():
            evidence = json.loads(custody.read_text())
            fields = ("exit", "forced", "owning_reap", "group_absent_after_reap")
            report["terminal_custody"] = {field: evidence.get(field) for field in fields}
            fixture.require(evidence.get("owning_reap") is True and evidence.get("group_absent_after_reap") is True,
                            "terminal process group retirement unconfirmed")

        try:
            fixture.require(fixture.run(sys.executable, args, root, env, "terminal", interact=interact,
                                        timeout_seconds=100) == 0, "terminal guardian failed")
            terminal.CHECK["check_model_terminal_exit"](json.loads(custody.read_text()), argv)
        finally:
            fixture.cleanup_resources((), (("terminal custody", record_custody),), report, sys.exc_info()[1])


def run(binary, output):
    output.mkdir(mode=0o700)
    report = {"passed": False, "binary_sha256": terminal.CHECK["sha256"](binary)}
    try:
        with tempfile.TemporaryDirectory(prefix="codex-account-") as temporary:
            root = Path(temporary).resolve()
            env = fixture.fresh_environment(root)
            env.update(TERM="xterm-256color", TERM_PROGRAM="xterm", HTTP_PROXY="http://127.0.0.1:1",
                       HTTPS_PROXY="http://127.0.0.1:1", NO_PROXY="localhost,127.0.0.1,::1",
                       CODEX_CA_CERTIFICATE=str(root / "fixture-ca.pem"))
            tls = fixture_tls(root)

            def secure(server):
                server.socket = tls.wrap_socket(server.socket, server_side=True, do_handshake_on_connect=False)

            listeners = []
            backends = {}
            try:
                # A separate provider listener per process keeps late requests attributable through shutdown.
                for leg, _label in LEGS:
                    gateway = ReconnectGateway({"events": [], "requests": []})
                    listener = fixture.start_listener(gateway.handler(), bind=account_resume.AccountResumeServer,
                                                      prepare=secure)
                    listeners.append((leg, *listener))
                    backends[leg] = (listener[0].server_port, gateway)
                port, source = backends["source"]
                slots = {label: account_resume.write_login(root, source, label, f"https://127.0.0.1:{port}", email=email)
                         for label, email in EMAILS.items()}
                for _, gateway in backends.values():
                    gateway.tokens.update(source.tokens)
                fingerprints = {label: fingerprint(path) for label, path in slots.items()}
                exercise(binary, root, env, backends, slots, report)
            finally:
                fixture.cleanup_resources(listeners, (), report, sys.exc_info()[1])
            for index, (leg, label) in enumerate(LEGS):
                gateway = backends[leg][1]
                fixture.require(gateway.confirmed_turn(index, report[leg]["turn"]) == report[leg],
                                f"model evidence changed on the {leg} listener")
                paths = [r["path"] for r in gateway.reads]
                fixture.require(set(paths) == {"/v1/models", "/backend-api/wham/accounts/check"}
                                and all((r["login"], r["account"]) == (label, "workspace-" + label)
                                        for r in gateway.reads), f"{leg} account read used an unexpected login")
                report[leg]["account_read_paths"] = paths
            fixture.require(all(fingerprint(path) == fingerprints[label]
                                for label, path in slots.items()) and not (root / "codex-home/auth.json").exists(),
                            "reconnecting replaced a named credential or created a shared default")
            report["named_credential_files_unchanged"] = True
        fixture.require("cleanupErrors" not in report and terminal.CHECK["sha256"](binary) == report["binary_sha256"],
                        "fixture cleanup or executable identity check failed")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    fixture.write_json(output / "result.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="new directory for the redacted result")
    args = parser.parse_args()
    os.umask(0o077)
    result = run(args.codex.resolve(strict=True), args.output.resolve())
    print(result.get("error", "passed"))
    sys.exit(0 if result["passed"] else 1)
