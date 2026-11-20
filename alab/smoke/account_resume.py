#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise interactive and exec resume across two logins sharing one local Codex home."""

import argparse
import hashlib
import json
from pathlib import Path
import secrets
import sys
import time
from urllib.parse import urlsplit

import capacity_exec as fixture
import account_resume_moved
import account_resume_page
import account_resume_restored
import account_resume_return
import account_resume_titles
import model_resume as terminal
import ordinary_account_renewal as renewal
from account_resume_daemon import ImplicitSeedDaemon, ManagedDaemon, SHARED_EMAIL
from provider_fixture import LoopbackHTTPServer
from quota_resume import Rpc
import renewal_follow as logins


TOOL_CALL = {"type": "function_call", "call_id": "read-cedar",
             "name": "exec_command", "arguments": json.dumps({"cmd": "tee -a cedar-calls < cedar-marker.txt"})}


RESUME_DIRECTORY_CALL = {
    "type": "function_call", "call_id": "verify-resumed-directory", "name": "exec_command",
    "arguments": json.dumps({"cmd": "pwd -P && tee -a cedar-resume-calls < cedar-marker.txt"}),
}


class AccountResumeServer(LoopbackHTTPServer):
    # server_close joins every accepted handler before the account evidence is checked.
    daemon_threads = False

    def finish_request(self, request, client_address):
        request.settimeout(10)
        super().finish_request(request, client_address)


def conversation_input(items):
    return [{"role": item["role"], "content": item.get("content")}
            for item in items if item.get("role") in ("user", "assistant")]


class Gateway(terminal.Gateway):
    verify_resume_directory = False
    capture_conversation_input = False

    def __init__(self, report):
        super().__init__(report["events"])
        self.tokens = {}
        self.requests = report["requests"]
        self.reads = report.setdefault("account_reads", [])
        self.issued_tool = False

    def account_route(self, headers):
        # HTTPMessage.items() preserves repeated fields, including different casing.
        bearers = [value for name, value in headers.items() if name.lower() == "authorization"]
        accounts = [value for name, value in headers.items() if name.lower() == "chatgpt-account-id"]
        login = self.tokens.get(bearers[0]) if len(bearers) == 1 else None
        return login, accounts[0] if len(accounts) == 1 else None

    def handler(self):
        gateway = self

        class Handler(super().handler()):
            def do_GET(self):
                path = urlsplit(self.path).path
                if path in ("/backend-api/wham/accounts/check", "/v1/models"):
                    with gateway.lock:
                        label, account = gateway.account_route(self.headers)
                        gateway.reads.append({"path": path, "login": label, "account": account,
                                              "phase": "resume" if gateway.resumed else "seed"})
                if path == "/backend-api/wham/accounts/check":
                    if not label or account != "workspace-" + label:
                        self.send_error(401)
                        return
                    body = {"accounts": [{"id": account, "name": label, "can_access_with_session": True,
                                          "is_deactivated": False, "plan_type": "pro",
                                          "workspace_backend_origin": "NO_CONSTRAINT",
                                          "account_routing_override": "NO_CONSTRAINT"}],
                            "account_ordering": [account], "default_account_id": account}
                    self.send_body("application/json", json.dumps(body).encode())
                elif path == "/backend-api/wham/config/bundle":
                    self.send_body("application/json", b"{}")
                else:
                    super().do_GET()

        return Handler

    def response(self, body, headers):
        markers = []
        tool_records = []
        for item in body.get("input", []):
            if item.get("call_id") in (TOOL_CALL["call_id"], RESUME_DIRECTORY_CALL["call_id"]):
                tool_records.append(item)
                markers.append(item["type"])
            for part in item.get("content", []):
                if isinstance(part, dict) and (item.get("role"), part.get("text")) in (
                        ("user", terminal.SEED), ("assistant", terminal.REPLY), ("user", terminal.CONTINUE),
                        ("user", terminal.BIRCH_SEED), ("assistant", terminal.BIRCH_REPLY),
                        ("user", terminal.BIRCH_CONTINUE), ("assistant", terminal.ANSWER),
                        ("user", terminal.AFTER_RELEASE)):
                    markers.append(part["text"])
        with self.lock:
            login, account = self.account_route(headers)
            request = {
                "login": login, "account": account,
                "thread": headers.get("thread-id"),
                "turn": json.loads(headers.get("x-codex-turn-metadata", "{}")).get("turn_id"),
                "history": markers,
                "tool_records": tool_records,
            }
            if self.capture_conversation_input:
                request["conversation_input"] = conversation_input(body.get("input", []))
            self.requests.append(request)
            if markers == [terminal.SEED] and not self.issued_tool:
                self.issued_tool = True
                return fixture.response("read-workspace", TOOL_CALL)
            if self.verify_resume_directory and self.resumed and markers[-1:] == [terminal.CONTINUE]:
                return fixture.response("verify-directory", RESUME_DIRECTORY_CALL)
        if not markers:
            # Vanilla also requests an automatic title on a different thread.
            return fixture.response("title", {"type": "message", "id": "title", "role": "assistant",
                                    "content": [{"type": "output_text", "text": "Remember the cedar marker"}]})
        return super().response(body, headers)


class RenewalGateway(Gateway):
    def __init__(self, report):
        super().__init__(report)
        self.attempts = 0

    def prepare_destination(self, root, issuer):
        self.slot = renewal.Slot(root, "destination", -600, issuer, account="workspace-destination")
        return self.slot.path

    def exchange(self, payload):
        with self.lock:
            self.attempts += 1
            fixture.require(self.resumed and self.attempts == 1, "unexpected destination renewal attempt")
            result = self.slot.exchange(payload)
            self.tokens["Bearer " + self.slot.new_access] = "destination"
            return result

    def handler(self):
        gateway = self

        class Handler(super().handler()):
            def do_POST(self):
                if urlsplit(self.path).path != "/oauth/token":
                    return super().do_POST()
                try:
                    length = int(self.headers.get("Content-Length", "0"))
                    payload = json.loads(self.rfile.read(length)) if 0 < length <= 16384 else None
                except (OSError, ValueError):
                    payload = None
                try:
                    result = gateway.exchange(payload)
                except RuntimeError:
                    self.send_error(400)
                    return
                self.send_body("application/json", json.dumps(result).encode())

        return Handler

    def check_renewal(self, slots, before):
        fixture.require(self.attempts == self.slot.exchanges == 1,
                        "resume did not exchange the expired destination credential exactly once")
        fixture.require(renewal.fingerprint(slots["source"]) == before["source"],
                        "destination renewal changed the source credential")
        fixture.require(renewal.fingerprint(self.slot.path)[:2] != before["destination"][:2],
                        "resume did not atomically replace the destination credential")
        self.slot.check_saved_credential()


def write_login(root, gateway, label, issuer, *, email=None):
    path = root / label / "auth.json"
    logins.fake_auth(path, label, issuer, "workspace-" + label, email=email or label + "@example.invalid",
                     user="user-" + label, account_user_id="membership-" + label)
    token = json.loads(path.read_text())["tokens"]["access_token"]
    gateway.tokens["Bearer " + token] = label
    return path


def select_login(root, gateway, label, issuer, *, email=None):
    write_login(root, gateway, label, issuer, email=email).replace(root / "codex-home/auth.json")


def configure_account_home(root, port):
    env = fixture.fresh_environment(root)
    env.update(HTTP_PROXY="http://127.0.0.1:1", HTTPS_PROXY="http://127.0.0.1:1", NO_PROXY="localhost,127.0.0.1,::1",
               CODEX_CA_CERTIFICATE=str(root / "fixture-ca.pem"))
    terminal.configure(root, port, "ultra", tls=True)
    config = root / "codex-home/config.toml"
    settings = config.read_text().replace('sandbox_mode = "read-only"', 'sandbox_mode = "workspace-write"')
    config.write_text('cli_auth_credentials_store = "file"\n' + settings)
    return env, f"https://127.0.0.1:{port}"


def configure_accounts(root, port, gateway):
    env, issuer = configure_account_home(root, port)
    slots = {label: write_login(root, gateway, label, issuer) for label in ("source", "destination")}
    return env, slots


def start_gateway(root, gateway):
    tls = logins.fixture_tls(root)

    def secure(server):
        server.socket = tls.wrap_socket(server.socket, server_side=True, do_handshake_on_connect=False)

    return fixture.start_listener(gateway.handler(), bind=AccountResumeServer, prepare=secure)


def exec_resume(binary, root, env, thread, *, launch_cwd=None):
    args = ["exec", "--json"]
    if launch_cwd is not None:
        fixture.require(launch_cwd.resolve(strict=True) != (root / "work").resolve(strict=True),
                        "cross-project exec must start outside the original workspace")
        args.extend(["-C", str(root / "work")])
    else:
        args.append("--skip-git-repo-check")
    args.extend(["resume", thread, terminal.CONTINUE])
    _, turn = exec_turn(binary, args, root, env, "continued", terminal.ANSWER, thread=thread, cwd=launch_cwd)
    return turn


def exec_turn(binary, args, root, env, label, answer, *, thread=None, cwd=None):
    status = fixture.run(binary, args, root, env, label, cwd=cwd)
    process = json.loads((root / f"{label}-guardian-result.json").read_text())
    fixture.require(status == 0 and process.get("argv") == [str(binary), *args]
                    and process.get("returncode") == 0 and process.get("timed_out") is False,
                    "exec resume did not exit successfully on the requested thread")
    events = [json.loads(line) for line in (root / f"{label}-stdout.jsonl").read_bytes().splitlines()]
    relevant = [row for row in events if row.get("type") in
                ("thread.started", "turn.started", "turn.completed", "turn.failed", "error")
                or row.get("type") == "item.completed" and row.get("item", {}).get("type") == "agent_message"]
    fixture.require([row["type"] for row in relevant] == ["thread.started", "turn.started",
                                                          "item.completed", "turn.completed"]
                    and isinstance(relevant[0].get("thread_id"), str)
                    and terminal.CHECK["MODEL_UUID"].fullmatch(relevant[0]["thread_id"])
                    and (thread is None or relevant[0]["thread_id"] == thread)
                    and relevant[2]["item"].get("text") == answer,
                    "exec resume emitted the wrong thread, reply or turn boundaries")
    exec_seed = thread is None
    thread = relevant[0]["thread_id"]
    rollout, = (root / "codex-home/sessions").glob(f"*/*/*/rollout-*-{thread}.jsonl")
    with rollout.open("rb") as stream:
        if exec_seed:
            metadata = json.loads(next(stream))
            fixture.require(metadata["type"] == "session_meta" and metadata["payload"].get("source") == "exec"
                            and metadata["payload"].get("id") == thread
                            and metadata["payload"].get("cwd") == str(cwd or root / "work"),
                            "original rollout is not the exec thread in the source workspace")
        completion, = (row["payload"] for row in map(json.loads, stream) if row.get("type") == "event_msg"
                       and row["payload"].get("type") == "task_complete"
                       and row["payload"].get("last_agent_message") == answer)
    fixture.require(not completion.get("error") and isinstance(completion.get("turn_id"), str)
                    and terminal.CHECK["MODEL_UUID"].fullmatch(completion["turn_id"]),
                    "exec did not persist the successfully completed turn")
    return thread, completion["turn_id"]


def check_resumed_directory(work, other, marker, resumed, followup):
    fixture.require((followup["thread"], followup["turn"]) == (resumed["thread"], resumed["turn"]),
                    "the directory check changed the resumed thread or turn")
    fixture.require(followup["history"] == [*resumed["history"], "function_call", "function_call_output"],
                    "the resumed turn did not complete its separate directory check")
    fixture.require(followup["tool_records"][:-2] == resumed["tool_records"],
                    "the directory check changed the original completed tool records")
    call, result = followup["tool_records"][-2:]
    fixture.require(all(call.get(k) == v for k, v in RESUME_DIRECTORY_CALL.items())
                    and result.get("type") == "function_call_output"
                    and result.get("call_id") == RESUME_DIRECTORY_CALL["call_id"],
                    "the resumed command did not return its own result")
    output = result.get("output")
    expected_directory = str(work.resolve(strict=True))
    fixture.require(isinstance(output, str) and output.rstrip("\n").splitlines()[-2:] == [expected_directory, marker],
                    "the resumed command did not report the selected physical directory and workspace marker")
    fixture.require((work / "cedar-resume-calls").read_text().splitlines() == [marker],
                    "the resumed workspace command did not run exactly once")
    fixture.require(not (other / "cedar-calls").exists(), "the original command ran in the unselected project")
    fixture.require(not (other / "cedar-resume-calls").exists(), "the resumed command ran in the unselected project")
    return expected_directory


def only_seed_rollout(root, thread, *, source="exec"):
    home = root / "codex-home"
    rollouts = [path for folder in ("sessions", "archived_sessions")
                for path in (home / folder).rglob("rollout-*.jsonl")]
    fixture.require(len(rollouts) == 1, "expected only the original rollout in the fixture home")
    path, = rollouts
    with path.open("rb") as stream:
        row = json.loads(next(stream))
    metadata = row.get("payload", {})
    fixture.require(row.get("type") == "session_meta" and metadata.get("id") == thread
                    and metadata.get("source") == source and metadata.get("cwd") == str(root / "work")
                    and path.is_relative_to(home / "sessions"), f"the sole rollout is not the active {source} thread")
    return str(path.relative_to(home))


def app_server_seed(binary, root, env, cwd):
    saved = {}

    def seed(process, output):
        rpc = Rpc(process, output)
        rpc.initialize()
        thread = rpc.request("thread/start", {"cwd": str(cwd), "ephemeral": False})["thread"]
        fixture.require(thread["source"] == "vscode" and thread["cwd"] == str(cwd),
                        "the local app server did not create an editor-origin conversation in the source project")
        turn = rpc.turn(thread["id"], terminal.SEED)
        fixture.require(turn["status"] == "completed", "the editor-origin turn did not finish")
        saved.update(thread=thread["id"], turn=turn["id"])

    status = fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, "seed", cwd=cwd, interact=seed)
    fixture.require(status == 0, "the seeding app server did not exit cleanly")
    home = root / "codex-home"
    rollout, = (home / "sessions").rglob(f"rollout-*-{saved['thread']}.jsonl")
    prefix = rollout.read_bytes()
    header = json.loads(prefix.split(b"\n", 1)[0])
    expected = {"id": saved["thread"], "cwd": str(cwd), "source": "vscode", "creator_user_id": "user-source",
                "creator_account_id": "workspace-source", "account_scoped": True}
    fixture.require(header["type"] == "session_meta" and all(header["payload"].get(k) == v for k, v in expected.items()),
                    "the saved editor conversation lost its original owner, source or project")
    return {**saved, "rollout": str(rollout.relative_to(home)), "original_bytes": len(prefix),
            "original_sha256": hashlib.sha256(prefix).hexdigest()}


def archive_seed(binary, root, env, thread):
    home = root / "codex-home"
    original = home / only_seed_rollout(root, thread)
    size = original.stat().st_size
    before = terminal.CHECK["sha256"](original)

    def archive(process, output):
        rpc = Rpc(process, output)
        rpc.initialize()
        rpc.request("thread/archive", {"threadId": thread})

    status = fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, "archive", interact=archive)
    fixture.require(status == 0 and not original.exists(), "archive did not remove A's active rollout")
    archived, = (home / "archived_sessions").rglob(original.name)
    fixture.require(terminal.CHECK["sha256"](archived) == before, "archive changed the saved conversation")
    return {"active_path": str(original.relative_to(home)), "archived_path": str(archived.relative_to(home)),
            "seed_bytes": size, "seed_sha256": before}


def run(binary, root, surface, *, seed_binary=None, separate_auth_files=False, renew_destination=False,
        seed_surface="terminal", managed_daemon=None, page_boundary=False, empty_destination_login=False,
        restored_source=False, archived_source=False, moved_project=None, duplicate_titles=False,
        switch_back=False, idle_seed_daemon=False):
    fixture.require(not idle_seed_daemon or (switch_back and seed_surface == "terminal" and not separate_auth_files),
                    "the idle writer control requires a shared terminal seed and a return to A")
    fixture.require(not switch_back or (surface == "bare-all-default"
                    and not any((empty_destination_login, page_boundary, duplicate_titles, managed_daemon))),
                    "switch-back needs the local bare picker with two distinct conversation titles")
    fixture.require(not renew_destination or separate_auth_files, "destination renewal requires separate auth files")
    fixture.require(managed_daemon is None or (managed_daemon in ("source", "destination", "source-to-named")
                    and surface == "bare-all-default" and not separate_auth_files),
                    "managed daemon requires bare-all-default with shared auth.json")
    fixture.require(not page_boundary or surface == "bare-all-default" and seed_surface == "exec" and not managed_daemon,
                    "page boundary requires bare-all-default with an exec seed and the embedded backend")
    fixture.require(not empty_destination_login or (surface == "bare-all-default" and seed_surface in ("exec", "app-server")
                    and not managed_daemon and not page_boundary),
                    "empty destination login requires bare-all-default with an exec or app-server seed and no sibling or daemon")
    fixture.require(not restored_source or (surface == "bare-all-default" and seed_surface == "exec"
                    and not any((managed_daemon, page_boundary, empty_destination_login, renew_destination))),
                    "restored source requires an exec seed, a destination sibling and plain bare resume")
    fixture.require(not archived_source or (surface == "bare-all-default" and seed_surface == "exec"
                    and not any((managed_daemon, page_boundary, empty_destination_login, restored_source))),
                    "archived source requires an exec seed, an active destination sibling and plain bare resume")
    fixture.require(not moved_project or (surface == "bare-all-default" and seed_surface == "exec"
                    and not any((managed_daemon, page_boundary, empty_destination_login, restored_source, archived_source))),
                    "moving the project requires an exec seed and the default active two-account picker")
    fixture.require(not duplicate_titles or (surface == "bare-all-default" and seed_surface == "exec"
                    and not any((managed_daemon, page_boundary, empty_destination_login, restored_source,
                                 archived_source, renew_destination, moved_project))),
                    "--duplicate-titles requires bare-all-default with two active exec threads")
    # Rollouts record the physical cwd. Keep the final component exclusive, even for a dangling link.
    root = root.parent.resolve(strict=True) / root.name
    root.mkdir(mode=0o700)
    env = fixture.fresh_environment(root)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm",
               HTTP_PROXY="http://127.0.0.1:1", HTTPS_PROXY="http://127.0.0.1:1",
               NO_PROXY="localhost,127.0.0.1,::1")
    report = {"passed": False, "surface": surface, "seed_surface": seed_surface, "events": [], "requests": [],
              "binary_sha256": terminal.CHECK["sha256"](binary)}
    seed_binary = seed_binary or binary
    report["seed_binary_sha256"] = (report["binary_sha256"] if binary == seed_binary
                                    else terminal.CHECK["sha256"](seed_binary))
    gateway = RenewalGateway(report) if renew_destination else Gateway(report)
    gateway.capture_conversation_input = switch_back
    completed = report["completed_turns"] = []
    listener = None
    daemon = None
    seed_owner = None
    try:
        listener = start_gateway(root, gateway)
        port = listener[0].server_port
        issuer = f"https://127.0.0.1:{port}"
        env["CODEX_CA_CERTIFICATE"] = str(root / "fixture-ca.pem")
        terminal.configure(root, port, "ultra", tls=True)
        config = root / "codex-home/config.toml"
        settings = config.read_text().replace('sandbox_mode = "read-only"', 'sandbox_mode = "workspace-write"')
        if page_boundary:
            settings = 'thread_unload_delay_secs = 0\n' + settings
        if surface in ("shared-picker", "cross-project-picker", "bare-all-default") and not managed_daemon:
            settings = settings.replace("features.daemon_auto_start = false\n", "")
        original = root / "work"
        if moved_project:
            # Keep the observer's work directory separate from the real project being moved.
            original = original / "before-move"
            original.mkdir()
        resume_cwd = original
        cross_project = not duplicate_titles and surface in ("cross-project-picker", "exec-project-id", "bare-all-default")
        other = root / "second-project"
        verify_directory = cross_project or seed_surface == "exec"
        if verify_directory:
            other.mkdir()
            (other / "cedar-marker.txt").write_text("cedar-other-project-" + secrets.token_hex(12) + "\n")
            gateway.verify_resume_directory = True
        if cross_project:
            resume_cwd = root / "relocated-project" if moved_project else other
            settings += f'\n[projects.{json.dumps(str(resume_cwd))}]\ntrust_level = "trusted"\n'
        if surface == "exec-project-id" or seed_surface == "exec":
            fixture.require(fixture.run(Path("/usr/bin/git"), ["init", "--quiet"], root, env, "init-worktree", cwd=original) == 0,
                            "could not initialize the original project for the normal exec repository check")
        config.write_text('cli_auth_credentials_store = "file"\n' + settings)
        marker = "cedar-tool-output-" + secrets.token_hex(12)
        (original / "cedar-marker.txt").write_text(marker + "\n")

        if separate_auth_files:
            slots = {"source": write_login(root, gateway, "source", issuer)}
            if renew_destination:
                slots["destination"] = gateway.prepare_destination(root, issuer)
                env["CODEX_REFRESH_TOKEN_URL_OVERRIDE"] = issuer + "/oauth/token"
            else:
                slots["destination"] = write_login(root, gateway, "destination", issuer)
            snapshot = renewal.fingerprint if renew_destination else terminal.CHECK["sha256"]
            before = {name: snapshot(path) for name, path in slots.items()}
            env["CODEX_AUTH_PATH"] = str(slots["source"])
        else:
            select_login(root, gateway, "source", issuer, email=SHARED_EMAIL if managed_daemon else None)
        no_daemon = surface in ("id", "exec-id", "exec-project-id")  # Use an isolated embedded server for the seed by ID.
        flags = [*(["--no-daemon"] if no_daemon else []), "--no-alt-screen"]
        if (surface == "bare-all-default" and seed_surface == "terminal"
                and not managed_daemon and not separate_auth_files):
            seed_owner = ImplicitSeedDaemon(seed_binary, root, env, report["seed_binary_sha256"],
                                            report.setdefault("implicit_seed_daemon", {}))
            daemon = seed_owner  # Cleanup also owns a partially started seed.
        if seed_surface == "app-server":
            editor = report["editor_seed"] = app_server_seed(seed_binary, root, env, original)
            seed_thread = editor["thread"]
            completed.append(editor["turn"])
        elif seed_surface == "exec":
            seed_thread, turn = exec_turn(seed_binary, ["exec", "--json", terminal.SEED],
                                         root, env, "seed", terminal.REPLY, cwd=original)
            completed.append(turn)
        else:
            custody, _, _ = terminal.terminal_session(seed_binary, root, env, "seed", [terminal.SEED], gateway,
                                                       completed, no_daemon=no_daemon,
                                                       observe_completed=seed_owner.note_live_seed if seed_owner else None)
            terminal.CHECK["check_model_terminal_exit"](custody, [str(seed_binary), *flags, terminal.SEED])
        with gateway.lock:
            if renew_destination:
                fixture.require(all(renewal.fingerprint(path) == before[name] for name, path in slots.items())
                                and not (root / "codex-home/auth.json").exists() and not gateway.attempts,
                                "seed changed the configured logins before resume")
            seed_requests = list(gateway.requests)
        seed, with_tool = [row for row in seed_requests if row["history"]]
        fixture.require(all(row["login"] == "source" and row["account"] == "workspace-source"
                            for row in seed_requests), "seed used the wrong bearer or workspace")
        fixture.require(seed["history"] == [terminal.SEED] and with_tool["history"] == [
                        terminal.SEED, "function_call", "function_call_output"], "seed lost its tool call or result")
        call, output = with_tool["tool_records"]
        fixture.require(all(call.get(k) == v for k, v in TOOL_CALL.items())
                        and marker in json.dumps(output.get("output")), "seed tool did not return the workspace marker")
        thread = seed["thread"]
        if seed_surface in ("exec", "app-server"):
            fixture.require(thread == seed_thread, "seed process and model request named different threads")
        fixture.require(with_tool["thread"] == thread and with_tool["turn"] == seed["turn"], "seed changed thread or turn")
        retained_writer = seed_owner and seed_owner.observe(thread, required=idle_seed_daemon)
        if duplicate_titles:
            account_resume_titles.name_thread(binary, root, env, "source", thread)
        if archived_source:
            report["archive"] = archive_seed(binary, root, env, thread)
        def start_daemon(login):
            nonlocal daemon
            setting = "features.daemon_auto_start = false"
            configured = config.read_text()
            fixture.require(configured.count(setting) == 1, "expected embedded settings while seeding")
            config.write_text(configured.replace(setting, "features.daemon_auto_start = true"))
            report["managed_daemon"] = {"initial_login": login}
            daemon = ManagedDaemon(binary, root, env, report["binary_sha256"], report["managed_daemon"])
            daemon.start(login, thread)

        if managed_daemon in ("source", "source-to-named"):
            start_daemon("source")
        if separate_auth_files:
            env["CODEX_AUTH_PATH"] = str(slots["destination"])
        elif managed_daemon == "source-to-named":
            shared = root / "codex-home/auth.json"
            named = write_login(root, gateway, "destination", issuer, email=SHARED_EMAIL)
            authority_hashes = {path: terminal.CHECK["sha256"](path) for path in (shared, named)}
            env["CODEX_AUTH_PATH"] = str(named)
        else:
            if switch_back:
                # Both launches use upstream's shared auth.json. Retain A's synthetic login for the return.
                original_auth = (root / "codex-home/auth.json").read_bytes()
            select_login(root, gateway, "destination", issuer, email=SHARED_EMAIL if managed_daemon else None)
        with gateway.lock:
            gateway.resumed = True
        if empty_destination_login:
            origin = "vscode" if seed_surface == "app-server" else "exec"
            original_rollout = only_seed_rollout(root, thread, source=origin)
            narrow, _, _ = terminal.terminal_session(
                binary, root, env, "narrow-bare-all-default", ["resume"], gateway, completed,
                cwd=resume_cwd, empty_destination_login=True)
            terminal.CHECK["check_model_terminal_exit"](narrow, [str(binary), "resume"])
        elif surface == "bare-all-default":
            # Give B a newer, independent conversation in the launch directory.
            if page_boundary or duplicate_titles:
                # thread/list exposes whole-second timestamps. Make B strictly newer.
                time.sleep(1.1)
            if moved_project:
                original.rename(resume_cwd)
            sibling_thread, sibling_turn = exec_turn(binary, ["exec", "--json", "--skip-git-repo-check", terminal.BIRCH_SEED],
                                                       root, env, "sibling", terminal.BIRCH_REPLY, cwd=resume_cwd)
            fixture.require(sibling_thread != thread, "B did not start an independent conversation")
            if moved_project:
                previous_histories = account_resume_moved.cancel_at_directory_choice(
                    binary, root, env, gateway, resume_cwd, thread, sibling_thread)
            report["sibling_thread"] = sibling_thread
            if duplicate_titles:
                account_resume_titles.name_thread(binary, root, env, "destination", sibling_thread)
            if restored_source:
                report["restored_source"] = {}
                account_resume_restored.exercise(binary, root, env, thread, sibling_thread, gateway,
                                                 report["restored_source"])
            if page_boundary:
                report["pagination"] = account_resume_page.prepare(binary, root, env, thread, sibling_thread, other)
            if managed_daemon == "destination":
                start_daemon("destination")
        if surface in ("exec-id", "exec-project-id"):
            completed.append(exec_resume(binary, root, env, thread, launch_cwd=resume_cwd if cross_project else None))
        elif duplicate_titles:
            pickers = account_resume_titles.exercise(binary, root, env, gateway, thread, sibling_thread)
            report["duplicate_titles"] = pickers
            completed.append(pickers["source"]["turn"])
        else:
            args = ["resume", thread, terminal.CONTINUE] if surface == "id" else ["resume"]
            if surface == "cross-project-picker":
                args.append("--all")
            if seed_surface == "exec" and surface in ("shared-picker", "cross-project-picker"):
                custody, _, _ = terminal.terminal_session(binary, root, env, "excluded-exec-origin", args,
                                                           gateway, completed, no_daemon=no_daemon, cwd=resume_cwd)
                terminal.CHECK["check_model_terminal_exit"](custody, [str(binary), *flags, *args])
                report["exec_excluded_without_origin_flag"] = True
                args.append("--include-non-interactive")
            witness = (lambda: daemon.witness_continuation(thread, gateway, terminal.CONTINUE)) if managed_daemon else None
            picker = "paged-bare-all-default" if page_boundary else surface
            if archived_source:
                picker = "archived-picker"
            retry_idle_owner = None
            if retained_writer:
                with gateway.lock:
                    request_count = len(gateway.requests)

                def retry_idle_owner(raw, process, send, displayed):
                    seed_owner.retry_after_unload(thread, gateway, request_count, raw, process, send, displayed, marker)
            custody, _, _ = terminal.terminal_session(
                binary, root, env, picker, args, gateway, completed, no_daemon=no_daemon, cwd=resume_cwd,
                observe_completed=witness, empty_destination_login=empty_destination_login,
                search_older=page_boundary == "search", directory_choice=moved_project or "session",
                retry_idle_owner=retry_idle_owner)
            if page_boundary:
                terminal.CHECK["check_model_terminal_exit"](custody, [str(binary), "resume"])
                report["pagination"]["picker_positions"] = json.loads((root / "picker-pagination.json").read_bytes())
                if page_boundary == "search":
                    report["pagination"]["query"] = account_resume_page.OLDER_QUERY
            elif empty_destination_login:
                terminal.CHECK["check_model_terminal_exit"](custody, [str(binary), "resume"])
                fixture.require(only_seed_rollout(root, thread, source=origin) == original_rollout,
                                "resuming created a different rollout")
                report["first_destination_resume"] = {
                    "current_login_rows": 0, "initial_all_local_rows": 1,
                    "selected_thread": thread, "only_rollout": original_rollout}
            elif surface == "bare-all-default":
                terminal.CHECK["check_model_terminal_exit"](custody, [str(binary), "resume"])
                narrow, _, _ = terminal.terminal_session(
                    binary, root, env, "narrow-bare-all-default", args, gateway, completed,
                    no_daemon=False, cwd=resume_cwd, cwd_rows=["birch", "cedar"] if moved_project else None)
                terminal.CHECK["check_model_terminal_exit"](narrow, [str(binary), "resume"])
                report["bare_picker_showed_both_threads"] = True
                report["narrowed_by_directory_and_login"] = True
            else:
                terminal.CHECK["check_model_terminal_exit"](custody, [str(binary), *flags, *args])
        request_end = read_end = None
        if switch_back:
            with gateway.lock:
                request_end, read_end = len(gateway.requests), len(gateway.reads)
            if separate_auth_files:
                env["CODEX_AUTH_PATH"] = str(slots["source"])
            else:
                (root / "codex-home/auth.json").write_bytes(original_auth)
            report["return_to_source"] = account_resume_return.exercise(
                binary, root, env, gateway, thread, sibling_thread)
        fixture.cleanup_resources((("gateway", *listener),), (), report, None, stop=daemon.stop if daemon else None)
        daemon = None
        listener = None
        fixture.require(not report.get("cleanupErrors"), report.get("error", "gateway cleanup failed"))
        resumed_requests = gateway.requests[len(seed_requests):request_end]
        turns = [row for row in resumed_requests if row["history"]]
        if empty_destination_login:
            fixture.require(len(resumed_requests) == 2, "expected only the resumed inference and its tool follow-up")
        elif surface == "bare-all-default":
            sibling, *turns = turns
            if duplicate_titles:
                *turns, last = turns
                expected = [terminal.BIRCH_SEED, terminal.BIRCH_REPLY, terminal.BIRCH_CONTINUE]
                distinct = {seed["turn"], sibling_turn, pickers["source"]["turn"], last["turn"]}
                fixture.require(len(resumed_requests) == len(distinct) == 4 and last["thread"] == sibling_thread
                                and last["turn"] == pickers["destination"]["turn"] and last["history"] == expected
                                and last["tool_records"] == [],
                                "resuming B's same-named row changed its identity or conversation")
            if archived_source or switch_back:
                fixture.require(len(resumed_requests) == 3,
                                "expected only B's sibling request and the two requests to continue A")
            fixture.require((sibling["thread"], sibling["turn"], sibling["history"], sibling["tool_records"])
                            == (sibling_thread, sibling_turn, [terminal.BIRCH_SEED], []),
                            "the independent B conversation was missing or borrowed A's history")
        if verify_directory:
            resumed, directory_result = turns
        else:
            resumed, = turns
        fixture.require(all(row["login"] == "destination" and row["account"] == "workspace-destination"
                            for row in resumed_requests), "resume used the wrong bearer or workspace")
        for phase, login in (("seed", "source"), ("resume", "destination")):
            reads = [row for row in gateway.reads[:read_end] if row["phase"] == phase]
            fixture.require({row["path"] for row in reads} == {"/v1/models", "/backend-api/wham/accounts/check"},
                            f"{phase} did not exercise both catalog and account reads")
            fixture.require(all(row["login"] == login and row["account"] == "workspace-" + login for row in reads),
                            f"{phase} catalog or account read used the wrong bearer or workspace")
        fixture.require(resumed["history"] == [*with_tool["history"], terminal.REPLY, terminal.CONTINUE],
                        "resumed request lost or reordered the original conversation")
        fixture.require(resumed["tool_records"] == with_tool["tool_records"], "resume altered the completed tool call")
        selected_directory = resume_cwd if moved_project else original
        fixture.require((selected_directory / "cedar-calls").read_text().splitlines() == [marker],
                        "workspace command did not execute exactly once")
        fixture.require(resumed["thread"] == thread and completed == [seed["turn"], resumed["turn"]]
                        and seed["turn"] != resumed["turn"], "resume changed the thread or reused its first turn")
        if verify_directory:
            report["resumed_shell_directory"] = check_resumed_directory(
                selected_directory, other, marker, resumed, directory_result)
            rollout, = (root / "codex-home/sessions").glob(f"*/*/*/rollout-*-{thread}.jsonl")
            if archived_source:
                home = root / "codex-home"
                archive = report["archive"]
                fixture.require(rollout == home / archive["active_path"]
                                and not (home / archive["archived_path"]).exists(),
                                "selecting the archived row did not restore A's original active rollout")
                with rollout.open("rb") as stream:
                    prefix_digest = hashlib.sha256(stream.read(archive["seed_bytes"])).hexdigest()
                    fixture.require(prefix_digest == archive["seed_sha256"] and stream.read(1),
                                    "restored rollout did not append to the archived history")
            with rollout.open("rb") as stream:
                directories = [(row["payload"]["turn_id"], row["payload"].get("cwd"))
                               for row in map(json.loads, stream)
                               if row.get("type") == "turn_context" and row["payload"].get("turn_id") in completed]
            fixture.require(set(directories) == {(seed["turn"], str(original)), (resumed["turn"], str(selected_directory))},
                            "the recorded turn directories did not match the original and selected projects")
            if moved_project:
                preserved = account_resume_moved.verify_continuation(root / "codex-home", original, rollout, previous_histories)
                report["moved_project"] = {"original": str(original), "selected": str(selected_directory),
                                           "preserved_bytes": preserved, "directory_choice": moved_project,
                                           "cancelled_before_turn": True}
            if cross_project:
                start_field = "exec_start_directory" if surface == "exec-project-id" else "picker_start_directory"
                report[start_field] = str(resume_cwd)
            report["turn_directories"] = dict(directories)
        if switch_back:
            returned = report["return_to_source"]
            account_resume_return.check_request(gateway.requests[request_end:], gateway.reads[read_end:],
                                               returned, completed, directory_result, selected_directory)
            completed.append(returned["turn"])
            report["turn_directories"][returned["turn"]] = returned["directory"]
        fixture.require(terminal.CHECK["sha256"](binary) == report["binary_sha256"], "executable changed")
        if seed_binary != binary:
            fixture.require(terminal.CHECK["sha256"](seed_binary) == report["seed_binary_sha256"],
                            "original executable changed")
        if seed_surface == "app-server":
            with (root / "codex-home" / editor["rollout"]).open("rb") as stream:
                prefix_digest = hashlib.sha256(stream.read(editor["original_bytes"])).hexdigest()
                fixture.require(prefix_digest == editor["original_sha256"] and stream.read(1),
                                "continuation did not append to the original editor conversation")
        if separate_auth_files:
            fixture.require(not (root / "codex-home/auth.json").exists(),
                            "resume created a shared default credential")
            if renew_destination:
                gateway.check_renewal(slots, before)
                report["destination_renewal"] = {"exchanges": gateway.slot.exchanges,
                                                 "source_unchanged": True, "issued_credential_persisted": True}
            else:
                after = {name: snapshot(path) for name, path in slots.items()}
                fixture.require(after == before, "resume changed a named credential file")
                report["named_credential_files_unchanged"] = True
        if managed_daemon == "source-to-named":
            fixture.require(all(terminal.CHECK["sha256"](path) == digest
                                for path, digest in authority_hashes.items()),
                            "resume changed shared A's credential or explicitly selected B's file")
            report["shared_and_named_credentials_unchanged"] = True
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        fixture.cleanup_resources((("gateway", *listener),) if listener else (), (), report, sys.exc_info()[1],
                                  stop=daemon.stop if daemon else None)
        fixture.write_json(root / "result.json", report)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--seed-codex", type=Path, help="Create the original thread with a previous release.")
    parser.add_argument("--seed-surface", choices=("terminal", "exec", "app-server"), default="terminal",
                        help="Use app-server to create an editor-origin conversation through its stdio protocol.")
    parser.add_argument("--archived-source", action="store_true",
                        help="Archive A’s exec session, then restore and continue it from B’s bare picker.")
    parser.add_argument("--moved-project", nargs="?", choices=("current", "default-current"), const="current",
                        help="Move A’s project, cancel once, then choose its new directory as B by number or by pressing Enter.")
    parser.add_argument("--duplicate-titles", action="store_true",
                        help="Give A and B identical session names in one directory and select each from plain resume.")
    parser.add_argument("--restored-source", action="store_true",
                        help="Exercise missing, partially restored and previously blank history in the bare picker.")
    parser.add_argument("--page-boundary", nargs="?", choices=("scroll", "search"), const="scroll",
                        help="Fork 24 local B conversations and scroll to or search for A beyond the first 25.")
    parser.add_argument("--empty-destination-login", action="store_true",
                        help="Resume A's sole exec or app-server thread when B has no conversation in this home.")
    parser.add_argument("--separate-auth-files", action="store_true",
                        help="Keep both ALab logins active through separate CODEX_AUTH_PATH files.")
    parser.add_argument("--switch-back", action="store_true",
                        help="After B continues A's thread, return to A and continue it again from the bare picker.")
    parser.add_argument("--idle-seed-daemon", action="store_true",
                        help="Require the original TUI to start a private daemon and retry after its idle unload.")
    parser.add_argument("--managed-daemon", nargs="?", choices=("source", "destination", "source-to-named"),
                        const="source", help="Start a private daemon under A or B. source-to-named keeps "
                        "shared A and selects a separate B credential. Requires Python websockets.")
    parser.add_argument("--renew-destination", action="store_true",
                        help="Resume on an expired destination and verify its automatic renewal.")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--surface", choices=("id", "exec-id", "exec-project-id", "shared-picker", "cross-project-picker",
                                              "bare-all-default"), required=True)
    args = parser.parse_args()
    if args.renew_destination and not args.separate_auth_files:
        parser.error("--renew-destination requires --separate-auth-files")
    if args.managed_daemon and (args.surface != "bare-all-default" or args.separate_auth_files):
        parser.error("--managed-daemon requires --surface bare-all-default with shared auth.json")
    if args.page_boundary and (args.surface != "bare-all-default" or args.seed_surface != "exec" or args.managed_daemon):
        parser.error("--page-boundary requires --surface bare-all-default --seed-surface exec without a managed daemon")
    if args.empty_destination_login and (args.surface != "bare-all-default" or args.seed_surface not in ("exec", "app-server")
                                         or args.managed_daemon or args.page_boundary):
        parser.error("--empty-destination-login requires --surface bare-all-default "
                     "--seed-surface exec or app-server without a daemon or pagination")
    sys.exit(run(args.codex.resolve(strict=True), args.output.absolute(), args.surface,
                 seed_binary=args.seed_codex.resolve(strict=True) if args.seed_codex else None,
                 separate_auth_files=args.separate_auth_files, renew_destination=args.renew_destination,
                 seed_surface=args.seed_surface, managed_daemon=args.managed_daemon, page_boundary=args.page_boundary,
                 empty_destination_login=args.empty_destination_login, restored_source=args.restored_source,
                 archived_source=args.archived_source, moved_project=args.moved_project, duplicate_titles=args.duplicate_titles,
                 switch_back=args.switch_back, idle_seed_daemon=args.idle_seed_daemon))
