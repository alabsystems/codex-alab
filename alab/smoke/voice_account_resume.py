#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise TUI-owned WebRTC signaling and observer credentials with synthetic local media."""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time
from urllib.parse import urlsplit

import account_resume
import capacity_exec as fixture
import model_resume
from renewal_follow import fixture_tls


HERE = Path(__file__).resolve().parent
WAIT = 18


def package(binary, root):
    # Mirror the private-package fixture used by realtime-webrtc's actor tests.
    # The marker represents the synthetic helper's availability; it is never loaded.
    destination = root / "package/bin/codex"
    destination.parent.mkdir(parents=True)
    try:
        os.link(binary, destination)
    except OSError:
        shutil.copyfile(binary, destination)
        destination.chmod(0o700)
    (root / "package/codex-package.json").write_text("{}")
    voice = root / "package/codex-resources/voice"
    (voice / "bin").mkdir(parents=True)
    (voice / "lib").mkdir()
    helper = voice / "bin/codex-voice-host"
    shutil.copyfile(HERE / "voice_media_fixture.py", helper)
    helper.chmod(0o700)
    lib = "libgstreamer-1.0.0.dylib" if sys.platform == "darwin" else "libgstreamer-1.0.so.0"
    (voice / "lib" / lib).touch()
    return destination


class Calls(account_resume.Gateway):
    def __init__(self, report):
        super().__init__(report)
        self.calls = report["calls"]
        self.attachments = report["attachments"]
        self.seen = threading.Condition()
        self.release = []
        self.stopping = threading.Event()

    def wait(self, description, predicate):
        with self.seen:
            fixture.require(self.seen.wait_for(predicate, WAIT), "timed out waiting for " + description)

    def shutdown(self):
        self.stopping.set()
        with self.seen:
            for event in self.release:
                event.set()

    def identity(self, headers):
        return {"login": self.tokens.get(headers.get("authorization")), "account": headers.get("chatgpt-account-id")}

    def handler(self):
        owner = self

        class Handler(super().handler()):
            protocol_version = "HTTP/1.1"

            def do_POST(self):
                path = urlsplit(self.path).path
                if path != "/backend-api/realtime/calls":
                    return super().do_POST()
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length < 128 * 1024 or b"fixture-offer" not in self.rfile.read(length):
                    self.send_error(400)
                    return
                with owner.seen:
                    call_id = f"rtc_fixture_{len(owner.calls) + 1}"
                    owner.calls.append(dict(call_id=call_id, thread=self.headers.get("thread-id"),
                                            **owner.identity(self.headers)))
                    ready = threading.Event()
                    owner.release.append(ready)
                    owner.seen.notify_all()
                if not ready.wait(WAIT) or owner.stopping.is_set():
                    self.close_connection = True
                    return
                answer = b"v=0\r\ns=fixture-answer\r\n"
                self.send_response(201)
                self.close_connection = True
                self.send_header("Connection", "close")
                self.send_header("Location", "/realtime/calls/" + call_id)
                self.send_header("Content-Type", "application/sdp")
                self.send_header("Content-Length", str(len(answer)))
                self.end_headers()
                try:
                    self.wfile.write(answer)
                except (BrokenPipeError, ConnectionResetError):
                    pass  # A TUI cancellation drops its in-flight HTTPS request.

            def do_GET(self):
                path = urlsplit(self.path).path
                if not path.startswith("/v1/live/"):
                    return super().do_GET()
                call_id = path.rsplit("/", 1)[-1]
                actual = owner.identity(self.headers)
                with owner.seen:
                    creation = next((c for c in owner.calls if c["call_id"] == call_id), None)
                    accepted = creation is not None and actual["login"] is not None and all(
                        actual[field] == creation[field] for field in ("login", "account"))
                    observation = dict(call_id=call_id, accepted=accepted, closed=False, frame_opcodes=[], **actual)
                    owner.attachments.append(observation)
                    owner.seen.notify_all()
                if not accepted:
                    self.send_error(403)
                    return
                key = self.headers["Sec-WebSocket-Key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
                self.send_response(101)
                self.send_header("Upgrade", "websocket")
                self.send_header("Connection", "Upgrade")
                self.send_header("Sec-WebSocket-Accept", base64.b64encode(hashlib.sha1(key.encode()).digest()).decode())
                self.end_headers()
                self.close_connection = True
                self.connection.settimeout(WAIT)
                try:
                    # An observer closes its socket. Sending application data here
                    # would reconfigure or end the client-owned session.
                    while header := self.rfile.read(2):
                        fixture.require(len(header) == 2 and header[1] & 128, "invalid client WebSocket frame")
                        opcode = header[0] & 15
                        with owner.seen:
                            observation["frame_opcodes"].append(opcode)
                        if opcode not in (9, 10):
                            break
                        size = header[1] & 127
                        fixture.require(size <= 125 and len(self.rfile.read(size + 4)) == size + 4,
                                        "invalid WebSocket control frame")
                    with owner.seen:
                        observation["closed"] = True
                        owner.seen.notify_all()
                except (TimeoutError, OSError, RuntimeError) as error:
                    with owner.seen:
                        observation["error"] = str(error)
                        owner.seen.notify_all()

        return Handler


def terminal(binary, root, env, label, exercise, *, args=()):
    raw = root / (label + ".raw")
    custody = root / (label + "-custody.json")
    spec = root / "tmp" / (label + ".json")
    args = [str(binary), "--no-alt-screen", *args]
    spec.write_text(json.dumps({"argv": args, "cwd": str(root / "work"), "env": env,
                                "raw": str(raw), "result": str(custody)}))

    def interact(process, _log):
        command_id = 0

        def send(keys):
            nonlocal command_id
            command_id += 1
            process.stdin.write((json.dumps({"id": command_id, "write": base64.b64encode(keys).decode()}) + "\n").encode())
            process.stdin.flush()

        def displayed(text, offset=0):
            deadline = time.monotonic() + WAIT
            while not raw.is_file() or text.encode() not in raw.read_bytes()[offset:]:
                fixture.require(process.poll() is None and time.monotonic() < deadline,
                                f"{label}: terminal did not display {text!r}")
                time.sleep(0.04)

        def voice():
            start = raw.stat().st_size
            send(b"/voice")
            displayed("voice", start)
            send(b"\r")

        displayed(model_resume.OLD)
        exercise(voice, displayed, raw)
        for timeout in (1, 12):
            send(b"\x03")
            try:
                process.wait(timeout=timeout)
                break
            except subprocess.TimeoutExpired:
                if timeout == 12:
                    raise

    status = fixture.run(sys.executable, ["-I", "-B", "-S", str(HERE / "history_pagination.py"),
                                         "--guardian", str(spec)], root, env, label, interact=interact)
    fixture.require(status == 0, label + " PTY fixture failed")
    model_resume.CHECK["check_model_terminal_exit"](json.loads(custody.read_text()), args)


def run(binary, root):
    root.mkdir(mode=0o700)
    fixture.require(sys.platform == "darwin" or sys.platform.startswith("linux"), "PTY smoke requires macOS or Linux")
    fixture.require(hasattr(os, "waitid") and hasattr(os, "WNOWAIT"), "PTY smoke requires Python os.waitid and WNOWAIT")
    env = fixture.fresh_environment(root)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm", HTTP_PROXY="http://127.0.0.1:1",
               HTTPS_PROXY="http://127.0.0.1:1", NO_PROXY="localhost,127.0.0.1,::1",
               CODEX_CA_CERTIFICATE=str(root / "fixture-ca.pem"))
    report = {"passed": False, "events": [], "requests": [], "calls": [], "attachments": [],
              "binary_sha256": model_resume.CHECK["sha256"](binary)}
    gateway = Calls(report)
    listener = None
    try:
        cli = package(binary, root)

        def prepare(server):
            server.socket = fixture_tls(root).wrap_socket(server.socket, server_side=True)

        listener = fixture.start_listener(gateway.handler(), prepare=prepare)
        port = listener[0].server_port
        issuer = f"https://127.0.0.1:{port}"
        config = root / "codex-home/config.toml"
        config.write_text(f'''model_provider = "openai"
model = "{model_resume.OLD}"
model_reasoning_effort = "high"
chatgpt_base_url = "{issuer}/backend-api"
openai_base_url = "{issuer}/v1"
experimental_realtime_ws_base_url = "{issuer}/v1"
experimental_realtime_webrtc_call_base_url = "{issuer}/backend-api"
cli_auth_credentials_store = "file"
features.daemon_auto_start = false
analytics.enabled = false
check_for_update_on_startup = false
[projects.{json.dumps(str(root / "work"))}]
trust_level = "trusted"
''')
        account_resume.select_login(root, gateway, "source", issuer)

        def original(voice, displayed, raw):
            voice()
            gateway.wait("first call creation", lambda: len(gateway.calls) == 1)
            account_resume.select_login(root, gateway, "destination", issuer)
            gateway.release[0].set()
            gateway.wait("first observer", lambda: len(gateway.attachments) == 1)
            displayed("Voice conversation started")
            voice()
            gateway.wait("first observer to close", lambda: gateway.attachments[0]["closed"])
            displayed("Voice conversation ended")
            voice()
            gateway.wait("replacement login's pending call", lambda: len(gateway.calls) == 2)
            offset = raw.stat().st_size
            voice()
            # A fresh call reaching the provider proves the TUI processed cancellation.
            voice()
            gateway.wait("fresh call after cancellation", lambda: len(gateway.calls) == 3)
            gateway.release[1].set()
            gateway.release[2].set()
            gateway.wait("fresh observer attempt", lambda: any(row["call_id"] == "rtc_fixture_3" for row in gateway.attachments))
            displayed("403", offset)

        terminal(cli, root, env, "original", original)

        def restarted(voice, displayed, raw):
            voice()
            gateway.wait("new process call", lambda: len(gateway.calls) == 4)
            gateway.release[3].set()
            gateway.wait("new process observer", lambda: any(row["call_id"] == "rtc_fixture_4" for row in gateway.attachments))
            displayed("Voice conversation started")
            voice()
            gateway.wait("new process observer to close", lambda: gateway.attachments[-1]["closed"])
            displayed("Voice conversation ended")

        thread = gateway.calls[0]["thread"]
        fixture.require(bool(thread), "first voice call had no parent thread")
        terminal(cli, root, env, "restarted", restarted, args=["resume", thread])
        fixture.require(all(call["thread"] == thread for call in gateway.calls), "voice changed the resumed thread")
        fixture.require([c["login"] for c in gateway.calls] == ["source", "destination", "destination", "destination"],
                        "call creation used the wrong local credentials")
        fixture.require(all(c["account"] == "workspace-" + c["login"] for c in gateway.calls), "wrong create workspace")
        observed = [(row["call_id"], row["login"], row["accepted"]) for row in gateway.attachments]
        # The provider owns the retry count. Each refused handshake must keep the
        # original runtime identity, and no handshake may target the cancelled call.
        denied = ("rtc_fixture_3", "source", False)
        expected = [("rtc_fixture_1", "source", True), *([denied] * observed.count(denied)),
                    ("rtc_fixture_4", "destination", True)]
        fixture.require(denied in observed and observed == expected, f"unexpected observer requests: {observed}")
        fixture.require(all(row["account"] == "workspace-" + row["login"] for row in gateway.attachments),
                        "wrong observer workspace")
        fixture.require(all(row["closed"] and all(op in (8, 9, 10) for op in row["frame_opcodes"])
                            for row in gateway.attachments if row["accepted"]),
                        "observer did not close cleanly or sent application data")
        fixture.require(model_resume.CHECK["sha256"](cli) == report["binary_sha256"], "executable changed")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        package_dir = root / "package"
        cleanup = (("remove fixture package", lambda: shutil.rmtree(package_dir)),) if package_dir.exists() else ()
        fixture.cleanup_resources((("gateway", *listener),) if listener else (), cleanup, report, sys.exc_info()[1],
                                  stop=gateway.shutdown)
        fixture.write_json(root / "result.json", report)
    print(report.get("error", "passed" if report["passed"] else "failed"))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), args.output.resolve()))
