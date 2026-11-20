# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise enhanced execution controls through a real, isolated Codex frontend."""
import base64
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import time
import tomllib
import unittest

workspace = Path(os.environ["ALAB_FEATURE_WORKSPACE"])
binary = Path(os.environ["ALAB_FEATURE_BINARY"])
sys.path.insert(0, str(workspace / "alab/smoke"))
import capacity_exec as fixture
import model_resume
from provider_fixture import LoopbackHTTPServer
from quota_resume import Rpc
from sidebar_account_focus import read_runs


# The server returns the actual elicitation reply as its tool result. A client that never
# answers, prompts an operator, fabricates a field, or simply skips the tool cannot pass.
MCP_SERVER = r'''
import json
import sys

def send(message):
    print(json.dumps({"jsonrpc": "2.0", **message}), flush=True)

pending = {}
for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    if method == "initialize":
        result = {"protocolVersion": msg["params"]["protocolVersion"], "capabilities": {"tools": {}},
                  "serverInfo": {"name": "execution-control", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [{"name": name, "description": "Exercise an MCP form", "inputSchema":
                             {"type": "object", "properties": {}}, "annotations": {"readOnlyHint": True}}
                            for name in ("confirmation", "requested_fields")]}
    elif method == "tools/call":
        name = msg["params"]["name"]
        pending[name] = msg["id"]
        schema = {"type": "object", "properties": {}}
        if name == "requested_fields":
            schema.update(properties={"answer": {"type": "string"}}, required=["answer"])
        send({"method": "elicitation/create", "id": name,
              "params": {"mode": "form", "message": name, "requestedSchema": schema}})
        continue
    elif method is None and msg.get("id") in pending:
        send({"id": pending.pop(msg["id"]), "result": {"content": [
            {"type": "text", "text": json.dumps({"tool": msg["id"], "reply": msg.get("result")})}]}})
        continue
    elif method == "resources/list":
        result = {"resources": []}
    elif method == "resources/templates/list":
        result = {"resourceTemplates": []}
    elif "id" not in msg:
        continue
    else:
        result = {}
    send({"id": msg["id"], "result": result})
'''


class JoinedLoopbackServer(LoopbackHTTPServer):
    daemon_threads = False

    def finish_request(self, request, client_address):
        request.settimeout(10)
        super().finish_request(request, client_address)


def isolated_environment(root):
    env = fixture.fresh_environment(root)
    env.update(HTTP_PROXY="http://127.0.0.1:1", HTTPS_PROXY="http://127.0.0.1:1",
               NO_PROXY="localhost,127.0.0.1,::1")
    return env


class TitleGateway(model_resume.Gateway):
    def __init__(self):
        super().__init__([])
        self.title_requests = []

    def response(self, body, headers):
        prompts = model_resume.messages(body, "user")
        if model_resume.SEED not in prompts:
            with self.lock:
                self.title_requests.append({"model": body["model"], "prompts": prompts,
                                            "thread": headers.get("thread-id")})
            title = json.dumps({"title": "Cedar suggestion"})
            return fixture.response("title-control", {"type": "message", "id": "title", "role": "assistant",
                                    "content": [{"type": "output_text", "text": title}]})
        return super().response(body, headers)


def terminal_turn(root, env, gateway, *, rename=False, wait_for_automatic_title=False):
    raw, custody, spec = root / "policy.raw", root / "custody.json", root / "tmp/pty.json"
    argv = [str(binary), "--no-daemon", "--no-alt-screen", model_resume.SEED]
    spec.write_text(json.dumps({"argv": argv, "cwd": str(root / "work"), "env": env,
                                "raw": str(raw), "result": str(custody)}))

    def finish(process, _):
        deadline = time.monotonic() + 25
        command_id = 0

        def send(keys):
            nonlocal command_id
            command_id += 1
            process.stdin.write((json.dumps({"id": command_id, "write": base64.b64encode(keys).decode()})
                                 + "\n").encode())
            process.stdin.flush()

        def displayed(text, offset=0):
            needle = b"".join(text.encode().split())
            while time.monotonic() < deadline and process.poll() is None:
                if needle in read_runs(raw, offset):
                    return
                time.sleep(.04)
            raise RuntimeError(f"terminal did not show {text!r}")

        displayed(model_resume.REPLY)
        model_resume.wait_for_turn_completion(root, gateway, process, model_resume.REPLY, deadline)
        if wait_for_automatic_title:
            while time.monotonic() < deadline and process.poll() is None:
                with gateway.lock:
                    if gateway.title_requests:
                        break
                time.sleep(.04)
            else:
                raise RuntimeError("vanilla startup did not issue the automatic title request")
        if rename:
            offset = raw.stat().st_size
            send(b"/rename")
            displayed("/rename", offset)
            send(b"\r")
            displayed("Cedar suggestion", offset)
            edit_offset = raw.stat().st_size
            send(b"\x1b[200~ edited\x1b[201~")
            displayed("edited", edit_offset)
            send(b"\r")
            index = root / "codex-home/session_index.jsonl"
            while time.monotonic() < deadline and process.poll() is None:
                if index.exists() and b'"Cedar suggestion edited"' in index.read_bytes():
                    break
                time.sleep(.04)
            else:
                raise RuntimeError("edited title was not saved before terminal shutdown")
        for timeout in (1, 10):
            try:
                send(b"\x03")
            except BrokenPipeError:
                break
            try:
                process.wait(timeout=timeout)
                break
            except subprocess.TimeoutExpired:
                if timeout == 10:
                    raise

    guardian = workspace / "alab/smoke/history_pagination.py"
    fixture.require(fixture.run(sys.executable, ["-I", "-B", "-S", str(guardian), "--guardian", str(spec)],
                                root, env, "terminal", interact=finish, timeout_seconds=45) == 0,
                    "terminal custodian did not exit successfully")
    model_resume.CHECK["check_model_terminal_exit"](json.loads(custody.read_text()), argv)
    return read_runs(raw, 0)


class ExecutionWireControls(unittest.TestCase):
    def test_automatic_title_is_suppressed_and_manual_suggestion_is_editable(self):
        self.check_titles(enhanced=True)
        self.check_titles(enhanced=False)

    def check_titles(self, *, enhanced):
        with tempfile.TemporaryDirectory(prefix="execution-title-") as directory:
            root = Path(directory).resolve()
            env = isolated_environment(root)
            env.update(TERM="xterm-256color", TERM_PROGRAM="xterm")
            gateway = TitleGateway()
            server, worker = fixture.start_listener(gateway.handler(), bind=JoinedLoopbackServer)
            try:
                model_resume.configure(root, server.server_port, "low")
                config = root / "codex-home/config.toml"
                config.write_text(f'tui.alab_interface={str(enhanced).lower()}\n' + config.read_text())
                (root / "codex-home/auth.json").write_text(json.dumps({"OPENAI_API_KEY": "sk-local-fixture"}))
                terminal_turn(root, env, gateway, rename=enhanced, wait_for_automatic_title=not enhanced)
            finally:
                server.shutdown()
                worker.join()
                server.server_close()
            original, = [row for row in gateway.events if row["event"] == "request"]
            title, = gateway.title_requests
            self.assertTrue(original["thread"] and title["thread"])
            self.assertNotEqual(original["thread"], title["thread"])
            prefix = "Recent conversation messages:" if enhanced else "User prompt:"
            prompt, = [text for text in title["prompts"] if prefix in text]
            self.assertIn(model_resume.SEED, prompt)
            if not enhanced:
                return
            self.assertIn(model_resume.REPLY, prompt)

            def persisted(process, output):
                rpc = Rpc(process, output)
                rpc.initialize()
                thread = rpc.request("thread/read", {"threadId": original["thread"]})["thread"]
                self.assertEqual(thread["name"], "Cedar suggestion edited")
            self.assertEqual(fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env,
                                         "read-name", interact=persisted, timeout_seconds=20), 0)

    def test_optional_model_stays_and_retired_model_moves_with_a_notice(self):
        for optional in (True, False):
            with self.subTest(optional=optional), tempfile.TemporaryDirectory(prefix="execution-model-") as directory:
                root = Path(directory).resolve()
                env = isolated_environment(root)
                env.update(TERM="xterm-256color", TERM_PROGRAM="xterm")
                gateway = TitleGateway()
                server, worker = fixture.start_listener(gateway.handler(), bind=JoinedLoopbackServer)
                try:
                    old, new = "gpt-5.4", "gpt-6-sol"
                    catalog = root / "catalog.json"
                    models = [model_resume.model(new)]
                    models[0]["default_reasoning_level"] = "high"
                    if optional:
                        models.insert(0, model_resume.model(old, new))
                    catalog.write_text(json.dumps({"models": models}))
                    model_resume.configure(root, server.server_port, "low", model_preference=old)
                    config = root / "codex-home/config.toml"
                    config.write_text("model_catalog_json=" + json.dumps(str(catalog)) + "\n" +
                                      config.read_text().replace('model_provider = "gateway"', 'model_provider = "openai"'))
                    (root / "codex-home/auth.json").write_text(json.dumps({"OPENAI_API_KEY": "sk-local-fixture"}))
                    screen = terminal_turn(root, env, gateway)
                finally:
                    server.shutdown()
                    worker.join()
                    server.server_close()
                turn, = [row for row in gateway.events if row["event"] == "request"]
                self.assertEqual(turn["model"], old if optional else new)
                self.assertEqual(turn["effort"], "low" if optional else "high")
                notice = b"".join(f"Model {old} is no longer available; switched to {new}.".encode().split())
                self.assertEqual(notice in screen, not optional)
                saved = tomllib.loads(config.read_text())
                self.assertEqual(saved["model"], old if optional else new)
                self.assertEqual(saved["model_reasoning_effort"], "low" if optional else "high")

    def test_project_hook_execution_respects_mode_and_explicit_thread_override(self):
        for term, enhanced, override, enabled, expected in (
                ("aterm", True, None, True, True), ("aterm", True, False, True, False),
                ("xterm", True, None, True, False), ("aterm", False, None, True, False),
                ("xterm", False, True, True, True), ("aterm", True, None, False, False)):
            with self.subTest(term=term, enhanced=enhanced, override=override, enabled=enabled):
                with tempfile.TemporaryDirectory(prefix="execution-hook-") as directory:
                    root = Path(directory).resolve()
                    env = isolated_environment(root)
                    env["TERM_PROGRAM"] = term
                    gateway = model_resume.Gateway([])
                    server, worker = fixture.start_listener(gateway.handler(), bind=JoinedLoopbackServer)
                    try:
                        model_resume.configure(root, server.server_port, "low")
                        config = root / "codex-home/config.toml"
                        config.write_text(f'tui.alab_interface={str(enhanced).lower()}\n'
                                          f'features.hooks={str(enabled).lower()}\n' +
                                          config.read_text().replace("requires_openai_auth = true",
                                                                     "requires_openai_auth = false"))
                        # A project layer must exist for the adjacent hook file to load.
                        project = root / "work/.codex"
                        project.mkdir()
                        (project / "config.toml").write_text("")
                        receipt = root / "work/hook-event.json"
                        command = "cat >> " + shlex.quote(str(receipt))
                        (project / "hooks.json").write_text(json.dumps({"hooks": {"UserPromptSubmit": [
                            {"hooks": [{"type": "command", "command": command}]}]}}))

                        def interact(process, output):
                            rpc = Rpc(process, output)
                            rpc.initialize()
                            params = {"cwd": str(root / "work"), "model": model_resume.OLD,
                                      "modelProvider": "gateway", "threadSource": "user"}
                            if override is not None:
                                params["config"] = {"bypass_hook_trust": override}
                            started = rpc.request("thread/start", params)
                            thread_id = started["thread"]["id"]
                            turn = rpc.turn(thread_id, model_resume.SEED)
                            self.assertEqual(turn["status"], "completed")
                            if expected:
                                event = json.loads(receipt.read_text())
                                self.assertEqual(event["hook_event_name"], "UserPromptSubmit")
                                self.assertEqual(event["prompt"], model_resume.SEED)
                            else:
                                self.assertFalse(receipt.exists())
                        self.assertEqual(fixture.run(binary, ["app-server", "--listen", "stdio://"],
                                                     root, env, "hook", interact=interact, timeout_seconds=50), 0)
                    finally:
                        server.shutdown()
                        worker.join()
                        server.server_close()

    def test_capable_user_thread_accepts_confirmation_and_declines_requested_fields(self):
        self.check_elicitation(enhanced=True)
        self.check_elicitation(enhanced=False)

    def check_elicitation(self, *, enhanced):
        with tempfile.TemporaryDirectory(prefix="execution-elicitation-") as directory:
            root = Path(directory).resolve()
            env = isolated_environment(root)
            script = root / "work/mcp_fixture.py"
            script.write_text(MCP_SERVER)
            (root / "codex-home/config.toml").write_text(
                f'tui.alab_interface={str(enhanced).lower()}\ncheck_for_update_on_startup=false\nanalytics.enabled=false\n'
                'features.daemon_auto_start=false\nfeatures.apps=false\nfeatures.plugins=false\n'
                'openai_base_url="http://127.0.0.1:1/v1"\n'
                '[mcp_servers.execution_control]\ncommand=' + json.dumps(sys.executable)
                + '\nargs=' + json.dumps(["-I", "-B", str(script)]) + '\n')

            def interact(process, output):
                rpc = Rpc(process, output)
                rpc.request("initialize", {"clientInfo": {"name": "execution_control", "version": "1"},
                                           "capabilities": {"experimentalApi": True,
                                                            "extensions": {"openai/standard-form-input": {}}}})
                rpc.send({"jsonrpc": "2.0", "method": "initialized"})
                started = rpc.request("thread/start", {"cwd": str(root / "work"), "threadSource": "user",
                                                      "model": "fixture-model", "approvalPolicy": "never",
                                                      "sandbox": "danger-full-access"})
                self.assertEqual(started["approvalPolicy"], "never")
                self.assertEqual(started["sandbox"]["type"], "dangerFullAccess")
                thread_id = started["thread"]["id"]
                self.assertEqual(started["thread"]["threadSource"], "user")
                for name, expected in (("confirmation", {"action": "accept", "content": {}}),
                                       ("requested_fields", {"action": "decline"})):
                    rpc.request_id += 1
                    rpc.send({"id": rpc.request_id, "method": "mcpServer/tool/call",
                              "params": {"threadId": thread_id, "server": "execution_control", "tool": name,
                                         "arguments": {}}})
                    reply = rpc.wait(lambda row: row.get("method") == "mcpServer/elicitation/request" or
                                     row.get("id") == rpc.request_id and ("result" in row or "error" in row),
                                     timeout=20)
                    if name == "requested_fields" and not enhanced:
                        self.assertEqual(reply.get("method"), "mcpServer/elicitation/request")
                        self.assertEqual(reply["params"]["threadId"], thread_id)
                        self.assertEqual(reply["params"]["serverName"], "execution_control")
                        self.assertEqual(reply["params"]["message"], name)
                        self.assertEqual(reply["params"]["requestedSchema"]["required"], ["answer"])
                        expected = {"action": "accept", "content": {"answer": "typed by the fixture"}}
                        rpc.send({"id": reply["id"], "result": expected})
                        reply = rpc.wait(lambda row: row.get("id") == rpc.request_id and
                                         ("result" in row or "error" in row), timeout=20)
                    self.assertNotIn("method", reply, "optional user input reached the frontend")
                    self.assertNotIn("error", reply)
                    content, = reply["result"]["content"]
                    self.assertEqual(content["type"], "text")
                    self.assertEqual(json.loads(content["text"]), {"tool": name, "reply": expected})

            self.assertEqual(fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env,
                                         "elicitation", interact=interact, timeout_seconds=65), 0)
