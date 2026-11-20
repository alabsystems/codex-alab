#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Observe model upgrades on cold resume and in an active conversation."""

import argparse
import base64
import http.server
import json
from pathlib import Path
import runpy
import subprocess
import sys
import threading
import time
from urllib.parse import urlsplit

import capacity_exec as fixture
from picker_screen import visible_lines
from quota_resume import Rpc


OLD = "gateway-amber"
NEW = "gateway-indigo"
OTHER = "gateway-ochre"
SEED = "Remember the cedar marker from this conversation."
REPLY = "Cedar marker saved."
CONTINUE = "Recall our cedar marker and continue."
ANSWER = "Cedar marker recalled."
AFTER_RELEASE = "Continue after the catalog release."
FINAL_ANSWER = "Final cedar answer."
BIRCH_SEED = "Remember the birch in the second workspace."
BIRCH_REPLY = "Birch workspace remembered."
BIRCH_CONTINUE = "Recall the second workspace."
BIRCH_ANSWER = "Birch workspace recalled."
OLD_ETAG = '"amber"'
NEW_ETAG = '"indigo"'
HERE = Path(__file__).resolve().parent
CHECK = runpy.run_path(str(HERE.parent / "check_runtime.py"))


def model(slug, successor=None):
    return {"slug": slug, "display_name": slug, "shell_type": "shell_command", "visibility": "list",
            "base_instructions": "Answer the user.", "supported_in_api": True, "priority": 1, "default_reasoning_level": "low",
            "supported_reasoning_levels": [{"effort": value, "description": value}
                                           for value in ("low", "high", "xhigh", "ultra")],
            # Different concrete Ultra settings distinguish restored Ultra from a copied wire effort.
            "multi_agent_version": "v2", "multi_agent_reasoning_effort": "xhigh" if slug == OLD else "high",
            "support_verbosity": False,
            "supports_reasoning_summaries": True, "supports_parallel_tool_calls": True,
            "truncation_policy": {"mode": "tokens", "limit": 10000},
            "experimental_supported_tools": [],
            "upgrade": {"model": successor, "migration_markdown": "New catalog release."} if successor else None}


def messages(body, role):
    content_type = "input_text" if role == "user" else "output_text"
    return [part.get("text") for item in body.get("input", []) if item.get("role") == role
            for part in item.get("content", []) if isinstance(part, dict) and part.get("type") == content_type]


class Gateway:
    def __init__(self, events, live=False):
        self.events = events
        self.lock = threading.Lock()
        self.resumed = False
        self.live = live
        self.released = False

    def catalog_rows(self):
        rows = [model(OLD, NEW), model(NEW)] if self.released else [model(OLD)]
        return [*rows, model(OTHER)]

    def catalog(self):
        with self.lock:
            models = self.catalog_rows()
            self.events.append({"event": "catalog", "models": [item["slug"] for item in models],
                                "successor": (models[0]["upgrade"] or {}).get("model")})
            return json.dumps({"models": models}).encode(), NEW_ETAG if self.released else OLD_ETAG

    def catalog_for_originator(self, originator):
        return self.catalog()

    def response(self, body, headers):
        with self.lock:
            users = messages(body, "user")
            turn = json.loads(headers.get("x-codex-turn-metadata", "{}")).get("turn_id")
            assistants = messages(body, "assistant")
            row = {"thread": headers.get("thread-id"), "turn": turn,
                   "model": body["model"], "effort": (body.get("reasoning") or {}).get("effort")}
            if BIRCH_SEED in users or BIRCH_CONTINUE in users or BIRCH_REPLY in assistants:
                self.events.append(dict(row, event="workspace_request", seed_prompt=BIRCH_SEED in users,
                                        prior_reply=BIRCH_REPLY in assistants, resume_prompt=BIRCH_CONTINUE in users,
                                        mixed_history=SEED in users or REPLY in assistants or CONTINUE in users))
                name, text = ("birch-continued", BIRCH_ANSWER) if BIRCH_CONTINUE in users else ("birch", BIRCH_REPLY)
            else:
                self.events.append(dict(row, event="request", seed_prompt=SEED in users,
                                        prior_reply=REPLY in assistants, resume_prompt=CONTINUE in users,
                                        after_release=AFTER_RELEASE in users, resumed_reply=ANSWER in assistants))
                name, text = ("continued", ANSWER) if self.resumed else ("original", REPLY)
                if AFTER_RELEASE in users:
                    name, text = "after-release", FINAL_ANSWER
            return fixture.response(name, {"type": "message", "id": name, "role": "assistant",
                                           "content": [{"type": "output_text", "text": text}]})

    def publish_successor(self, exit_code, thread, *, on_reattach=False):
        fixture.require(exit_code == 0, "original app-server did not exit successfully")
        with self.lock:
            self.events.append({"event": "original_exit", "exit": exit_code, "thread": thread})
            self.resumed = True
            self.released = not self.live and not on_reattach

    def release_before_reattach(self):
        with self.lock:
            self.released = True
            self.events.append({"event": "reattach"})

    def response_etag(self):
        with self.lock:
            if self.resumed and not self.released:
                self.released = True
                self.events.append({"event": "catalog_signal", "etag": NEW_ETAG})
            return NEW_ETAG if self.released else OLD_ETAG

    def handler(self):
        gateway = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def send_body(self, mime, data, headers=()):
                self.send_response(200)
                self.send_header("Content-Type", mime)
                self.send_header("Content-Length", str(len(data)))
                for name, value in headers:
                    self.send_header(name, value)
                self.end_headers()
                self.wfile.write(data)

            def do_GET(self):
                if urlsplit(self.path).path == "/v1/models":
                    data, etag = gateway.catalog_for_originator(self.headers.get("originator"))
                    self.send_body("application/json", data, [("ETag", etag)] if gateway.live else ())
                else:
                    self.send_error(404)

            def do_POST(self):
                length = int(self.headers.get("Content-Length", "0"))
                if self.path != "/v1/responses" or not 0 < length <= 8 * 1024 * 1024:
                    self.send_error(400)
                    return
                body = json.loads(self.rfile.read(length))
                data = gateway.response(body, self.headers)
                self.send_body("text/event-stream", data,
                               [("X-Models-Etag", gateway.response_etag())] if gateway.live else ())

        return Handler


def configure(root, port, effort, *, model_preference=OLD, tls=False):
    url = f"{'https' if tls else 'http'}://127.0.0.1:{port}"
    (root / "codex-home/config.toml").write_text(f'''model_provider = "gateway"
model = "{model_preference}"
model_reasoning_effort = "{effort}"
features.api_key_model_discovery = true
features.enable_request_compression = false
features.daemon_auto_start = false
analytics.enabled = false
check_for_update_on_startup = false
approval_policy = "never"
sandbox_mode = "read-only"
chatgpt_base_url = "{url}/backend-api"
openai_base_url = "{url}/v1"
[projects.{json.dumps(str(root / "work"))}]
trust_level = "trusted"
[model_providers.gateway]
name = "Local catalog gateway"
base_url = "{url}/v1"
model_catalog_url = "{url}/v1/models"
requires_openai_auth = true
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0
''')


def workspace_exec(binary, args, root, env, label, answer):
    status = fixture.run(binary, ["exec", "--skip-git-repo-check", *args], root, env, label)
    fixture.require(status == 0 and answer in (root / (label + "-stdout.jsonl")).read_text(),
                    "second workspace did not complete its turn")
    return json.loads((root / (label + "-guardian-result.json")).read_text())


def with_pty_lifeline(interact):
    def supervised(process, log):
        try:
            interact(process, log)
        except BaseException as error:
            # EOF lets the PTY custodian retire its original process group and
            # publish its result before the outer command guardian shuts down.
            try:
                process.stdin.close()
                process.wait(timeout=12)
            except (OSError, subprocess.TimeoutExpired) as cleanup:
                error.add_note(f"PTY lifeline cleanup: {cleanup}")
            raise
    return supervised


def wait_for_turn_completion(root, gateway, process, answer, deadline, *, event_kind="request",
                             expected_error=None, rollout_path=None):
    # A rendered reply can still accept steering. Match the provider's turn to
    # the completed event before submitting again or closing the terminal.
    with gateway.lock:
        request = next(row for row in reversed(gateway.events) if row["event"] == event_kind)
    thread, turn = request["thread"], request["turn"]
    fixture.require(isinstance(thread, str) and CHECK["MODEL_UUID"].fullmatch(thread)
                    and isinstance(turn, str) and CHECK["MODEL_UUID"].fullmatch(turn),
                    "missing terminal request identity")
    sessions = root / "codex-home/sessions"
    while process.poll() is None and time.monotonic() < deadline:
        paths = (rollout_path,) if rollout_path is not None else sessions.glob(f"*/*/*/rollout-*-{thread}.jsonl")
        for path in paths:
            with path.open("rb") as stream:
                for line in stream:
                    if not line.endswith(b"\n"):
                        break
                    row = json.loads(line)
                    event = row.get("payload", {})
                    if row.get("type") != "event_msg" or event.get("turn_id") != turn:
                        continue
                    fixture.require(event.get("type") != "turn_aborted", "terminal turn was aborted")
                    if event.get("type") == "task_complete":
                        error = event.get("error") or None
                        fixture.require(event.get("last_agent_message") == answer and error == expected_error,
                                        "terminal turn completed without the expected answer or error")
                        return turn
        time.sleep(.05)
    raise RuntimeError("terminal did not complete the provider turn")


def bare_picker_rows(raw, *, after=0, directory="All", login="All local", status="Active", expected=None):
    """Read the settled picker scope, count and distinct visible fixture rows."""
    lines = visible_lines(raw, after)
    if not any("Resume a previous session" in line for line in lines):
        return None
    toolbar = f"Filter: {directory} Logins: {login} {status} "
    if not any(toolbar in " ".join(line.split()) + " " for line in lines):
        return None
    if expected is None:
        expected = ["birch", "cedar"] if (directory, login) == ("All", "All local") else ["birch"]
    position = 1 if expected else 0
    if not any(f" {position} / {len(expected)} · 100%" in line for line in lines):
        return None
    empty_message = any(line.strip() == "No sessions yet" for line in lines)
    if empty_message != (not expected):
        return None
    visible = []
    for line in lines:
        names = [name for name in ("birch", "cedar") if f"Remember the {name}" in line]
        if len(names) > 1:
            return None
        visible.extend(names)
    return visible if sorted(visible) == expected else None


def terminal_session(binary, root, env, surface, args, gateway, completed, *, no_daemon=True, cwd=None,
                     observe_completed=None, empty_destination_login=False, observe_bare_picker=None,
                     search_older=False, label=None, completed_rollout=None, directory_choice="session", cwd_rows=None,
                     retry_idle_owner=None):
    if label is None:
        label = {"seed": "seed", "excluded-exec-origin": "origin-filter",
                 "narrow-bare-all-default": "narrow-picker"}.get(surface, "terminal")
    raw, custody = root / (label + ".raw"), root / (label + "-custody.json")
    # The spec contains the isolated environment and stays in scratch.
    spec = root / "tmp" / (label + ".json")
    bare = (observe_bare_picker is not None
            or surface in ("bare-all-default", "narrow-bare-all-default", "paged-bare-all-default", "archived-picker"))
    if bare:
        fixture.require(args == ["resume"], "bare acceptance requires a literal codex resume")
    flags = [] if bare else [*(["--no-daemon"] if no_daemon else []), "--no-alt-screen"]
    argv = [str(binary), *flags, *args]
    spec.write_text(json.dumps({"argv": argv, "cwd": str(cwd or root / "work"), "env": env,
                                "raw": str(raw), "result": str(custody)}))
    view_bytes = 0
    resized = False

    def finish(process, _log):
        nonlocal view_bytes, resized
        deadline = time.monotonic() + (105 if retry_idle_owner else 60)
        command_id = 0

        def command(**fields):
            nonlocal command_id
            command_id += 1
            process.stdin.write((json.dumps({"id": command_id, **fields}) + "\n").encode())
            process.stdin.flush()

        def send(keys):
            command(write=base64.b64encode(keys).decode())

        def quit_session():
            for attempt in (1, 2):
                try:
                    send(b"\x03")
                except BrokenPipeError:
                    break
                try:
                    process.wait(timeout=1 if attempt == 1 else 15)
                    break
                except subprocess.TimeoutExpired:
                    if attempt == 2:
                        raise

        def displayed(text, offset=0):
            needle = text.encode()
            tail = b""
            while True:
                if raw.is_file():
                    if bare:
                        if any(text in line for line in visible_lines(raw.read_bytes(), after=offset)):
                            return
                    else:
                        with raw.open("rb") as stream:
                            stream.seek(offset)
                            data = tail + stream.read()
                            offset = stream.tell()
                        if needle in data:
                            return
                        tail = data[-(len(needle) - 1):]
                fixture.require(process.poll() is None, f"terminal exited before displaying {text!r}")
                fixture.require(time.monotonic() < deadline, f"terminal did not display {text!r}")
                time.sleep(.05)

        def submit(prompt):
            start = raw.stat().st_size
            send(b"\x1b[200~" + prompt.encode() + b"\x1b[201~")
            # The editor can draw different words in separate changed cells.
            displayed(prompt if bare else prompt.split()[-1], start)
            send(b"\r")

        if observe_bare_picker is not None:
            view_bytes = observe_bare_picker(raw, process, deadline)
            quit_session()
            return

        if bare or surface in ("picker", "shared-picker", "cross-project-picker", "excluded-exec-origin"):
            if not bare:
                displayed("Resume a previous session")
            if surface == "paged-bare-all-default":
                from account_resume_page import search_for_older, select_older
                select = search_for_older if search_older else select_older
                fixture.write_json(root / "picker-pagination.json", select(raw, send, process, deadline))
            elif bare:
                def wait_for_rows(*, after=0, directory="All", login="All local", status="Active", expected=None):
                    while True:
                        if raw.is_file():
                            if empty_destination_login:
                                expected = ["cedar"] if login == "All local" else []
                            rows = bare_picker_rows(raw.read_bytes(), after=after, directory=directory,
                                                    login=login, status=status, expected=expected)
                            if rows is not None:
                                return rows
                        fixture.require(process.poll() is None and time.monotonic() < deadline,
                                        f"bare resume did not display the expected threads for {directory}, {login}, {status}")
                        time.sleep(.05)

                # Read the initial screen before any filter keys or selection.
                if surface == "archived-picker":
                    wait_for_rows(expected=["birch"])
                    start = raw.stat().st_size
                    send(b"\t\t\x1b[C")  # Focus Status and change Active to Archived.
                    initial_rows = wait_for_rows(after=start, status="Archived", expected=["cedar"])
                else:
                    initial_rows = wait_for_rows()
                if surface == "narrow-bare-all-default":
                    if not empty_destination_login:
                        start = raw.stat().st_size
                        send(b"\x1b[C")  # All directories -> Cwd.
                        wait_for_rows(after=start, directory="Cwd", expected=cwd_rows)
                        start = raw.stat().st_size
                        send(b"\x1b[D")
                        wait_for_rows(after=start)
                    send(b"\t")  # Focus Logins.
                    start = raw.stat().st_size
                    send(b"\x1b[C")  # All local -> Current.
                    wait_for_rows(after=start, login="Current")
                    quit_session()
                    return
                # The picker starts at the first row. Navigate only if B is first.
                if initial_rows[0] == "birch":
                    send(b"\x1b[B")
            elif surface in ("shared-picker", "cross-project-picker", "excluded-exec-origin"):
                def check_empty_scope(offset, width):
                    # The poll can paint just the changed cells of "Loading sessions…".
                    # Resize after this lookup settles to inspect its complete empty state.
                    displayed("yet", offset)
                    start = raw.stat().st_size
                    command(resize=[32, width])
                    displayed("No sessions yet", start)
                    view = raw.read_bytes()
                    fixture.require(b"Logins:" in view, "resume picker has no login scope selector")
                    # On initial load no prior frame may disclose the row. When narrowing,
                    # inspect the settled frame after the earlier, deliberately shared list.
                    scoped_view = view if offset == 0 else view[start:]
                    fixture.require(b"Remember the cedar" not in scoped_view,
                                    "the picker exposed a session outside the selected scope")
                    return view[start:]

                check_empty_scope(0, 90)
                send(b"\t")  # Move from the workspace filter to Logins.
                start = raw.stat().st_size
                send(b"\x1b[C")  # Right selects All local.
                if surface == "excluded-exec-origin":
                    frame = check_empty_scope(start, 76)
                    fixture.require(b"All local" in frame, "default origin filter was not checked across all logins")
                    quit_session()
                    return
                displayed("Remember the cedar", start)
                start = raw.stat().st_size
                send(b"\x1b[D")  # Left returns to Current.
                check_empty_scope(start, 76)
                start = raw.stat().st_size
                send(b"\x1b[C")
                displayed("Remember the cedar", start)
                if surface == "cross-project-picker":
                    send(b"\x1b[Z")  # Shift-Tab returns to the directory filter.
                    start = raw.stat().st_size
                    send(b"\x1b[C")  # Narrow All to this directory, keeping All local logins.
                    check_empty_scope(start, 90)
                    start = raw.stat().st_size
                    send(b"\x1b[D")  # In a non-repository workspace, Cwd returns to All.
                    displayed("Remember the cedar", start)
            else:
                displayed("Remember the cedar")
            send(b"\r")
            if surface in ("cross-project-picker", "bare-all-default", "paged-bare-all-default", "archived-picker"):
                displayed("Use session directory")
                if directory_choice == "session":
                    send(b"\r")
                else:
                    displayed("Use current directory")
                    current_highlight = "› 2. Use current directory"
                    current_is_default = any(current_highlight in line for line in visible_lines(raw.read_bytes()))
                    if directory_choice == "default-current":
                        fixture.require(current_is_default, "the missing session directory was still the default")
                        send(b"\r")
                    else:
                        start = raw.stat().st_size
                        # Observe one event-driven paint before cancelling or choosing by number.
                        send(b"\x1b[A" if current_is_default else b"\x1b[B")
                        displayed("› 1. Use session directory" if current_is_default else current_highlight, start)
                        if directory_choice == "cancel":
                            send(b"\x03")
                            process.wait(timeout=15)
                            return
                        fixture.require(directory_choice == "current", "unknown working-directory selection")
                        send(b"2")
            displayed(REPLY)
            if retry_idle_owner:
                retry_idle_owner(raw, process, send, displayed)
            submit(CONTINUE)
        answer = REPLY if surface == "seed" else ANSWER
        displayed(answer)
        completed.append(wait_for_turn_completion(root, gateway, process, answer, deadline, rollout_path=completed_rollout))
        if observe_completed is not None:
            observe_completed()
        live = surface == "live-terminal"
        if live:
            submit(AFTER_RELEASE)
            displayed(FINAL_ANSWER)
            completed.append(wait_for_turn_completion(root, gateway, process, FINAL_ANSWER, deadline))
        repaint_deadline = min(deadline, time.monotonic() + 5)
        expected = ((OLD if surface == "seed" else NEW).encode(), b"ultra")
        while True:
            view = raw.read_bytes()
            current = CHECK["terminal_model_selection"](view, live, after_reply=surface != "seed")
            if current == expected or time.monotonic() >= repaint_deadline:
                view_bytes = len(view)
                break
            # An unchanged footer may already show the successor when the reply
            # arrives. Resize once to obtain a fresh visible footer on that PTY.
            if not resized and CHECK["terminal_model_selection"](view, live, after_reply=False) == expected:
                command(resize=[32, 90])
                resized = True
            fixture.require(process.poll() is None, "terminal exited before the final status check")
            time.sleep(.05)
        quit_session()

    status = fixture.run(sys.executable, ["-I", "-B", "-S", str(HERE / "history_pagination.py"),
                                         "--guardian", str(spec)], root, env, label, interact=finish)
    fixture.require(status == 0, "resume PTY guardian failed")
    return json.loads(custody.read_text()), view_bytes, resized


def run(binary, root, surface, seed_binary=None):
    root.mkdir(mode=0o700)
    env = fixture.fresh_environment(root)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm")
    report = {"passed": False, "binary_sha256": CHECK["sha256"](binary), "events": []}
    seed_binary = seed_binary or binary
    report["seed_binary_sha256"] = (report["binary_sha256"] if seed_binary == binary
                                     else CHECK["sha256"](seed_binary))
    completed = report["terminal_completed_turns"] = []
    gateway = Gateway(report["events"], live=surface in ("live", "live-terminal"))
    listener = None
    try:
        listener = fixture.start_listener(gateway.handler())
        port = listener[0].server_port
        configure(root, port, "ultra")
        (root / "codex-home/auth.json").write_text(json.dumps({"OPENAI_API_KEY": "sk-loopback-fixture"}))

        def seed(process, log):
            rpc = Rpc(process, log)
            rpc.initialize()
            start = rpc.request("thread/start", {"cwd": str(root / "work"), "model": OLD, "ephemeral": False})
            report["thread"] = start["thread"]["id"]
            fixture.require(start["model"] == OLD, "original thread selected another model")
            report["original_effort"] = start["reasoningEffort"]
            fixture.require(report["original_effort"] == "ultra", "original thread did not select Ultra")
            turn = rpc.turn(report["thread"], SEED)
            fixture.require(turn["status"] == "completed", "original turn did not complete")

        if surface in CHECK["SELECTOR_SURFACES"]:
            receipt, end, _ = terminal_session(seed_binary, root, env, "seed", [SEED], gateway, completed)
            report["seed_custody"] = receipt
            selection = CHECK["terminal_model_selection"]((root / "seed.raw").read_bytes()[:end], False,
                                                         after_reply=False)
            fixture.require(selection == (OLD.encode(), b"ultra"), "original terminal did not select the old model and Ultra")
            original, = [r for r in report["events"] if r["event"] == "request"]
            report.update(thread=original["thread"], original_effort="ultra")
            status = receipt["exit"]
        else:
            status = fixture.run(seed_binary, ["app-server", "--listen", "stdio://"], root, env, "seed", interact=seed)
        if surface == "exec-last":
            (root / "birch").mkdir()
            # The thread list sorts by whole-second update timestamps.
            time.sleep(1.1)
            configure(root, port, "high", model_preference=OTHER)
            report["workspace_seed"] = workspace_exec(seed_binary, ["-C", "../birch", BIRCH_SEED],
                                                       root, env, "workspace-seed", BIRCH_REPLY)
        gateway.publish_successor(status, report["thread"], on_reattach=surface in ("loaded", "unloaded"))
        # Reopening must follow this thread even when another available model is the global default.
        configure(root, port, "low", model_preference=OTHER)
        if surface in ("live", "loaded", "unloaded"):
            def follow(process, log):
                rpc = Rpc(process, log)
                rpc.initialize()
                thread = report["thread"]
                params = {"threadId": thread, "cwd": str(root / "work")}
                saved = rpc.request("thread/resume", params)
                fixture.require(saved["thread"]["id"] == thread and saved["model"] == OLD
                                and saved["reasoningEffort"] == "ultra", "live resume changed the saved settings")
                fixture.require(rpc.turn(thread, CONTINUE)["status"] == "completed", "resumed turn did not complete")
                if surface == "live":
                    # The provider announced a release on the previous response. Observe the
                    # ordinary settings notification while the user leaves the thread idle.
                    changed = rpc.wait(lambda event: event.get("method") == "thread/settings/updated"
                                       and event["params"]["threadId"] == thread
                                       and event["params"]["threadSettings"]["model"] == NEW)["params"]
                    with gateway.lock:
                        report["idle_promotion"] = {
                            "thread": changed["threadId"], "model": changed["threadSettings"]["model"],
                            "effort": changed["threadSettings"]["effort"],
                            "preceding_turns": [e["turn"] for e in gateway.events if e["event"] == "request"],
                        }
                if surface == "unloaded":
                    rpc.request("thread/archive", {"threadId": thread})
                    restored = rpc.request("thread/unarchive", {"threadId": thread})
                    fixture.require(restored["thread"]["id"] == thread, "archive round trip changed the thread")
                    report["loaded_before_resume"] = rpc.request("thread/loaded/list", {})["data"]
                    fixture.require(report["loaded_before_resume"] == [], "thread remained loaded after archive")
                if surface in ("loaded", "unloaded"):
                    gateway.release_before_reattach()
                    saved = rpc.request("thread/resume", params)
                    report["reattached"] = {"thread": saved["thread"]["id"], "model": saved["model"],
                                            "effort": saved["reasoningEffort"]}
                fixture.require(rpc.turn(thread, AFTER_RELEASE)["status"] == "completed", "live turn did not complete")

            status = fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, "continued", interact=follow)
            report["process"] = json.loads((root / "continued-guardian-result.json").read_text())
            fixture.require(status == 0, "live app-server did not exit successfully")
        elif surface in ("exec", "exec-last"):
            if surface == "exec-last":
                # The newest thread overall is the birch thread. Resuming it here keeps
                # it newest when the following workspace-scoped lookup selects cedar.
                report["workspace_all"] = workspace_exec(binary, ["resume", "--last", "--all", BIRCH_CONTINUE],
                                                          root, env, "workspace-all", BIRCH_ANSWER)
            target = "--last" if surface == "exec-last" else report["thread"]
            args = ["exec", "--skip-git-repo-check", "resume", target, CONTINUE]
            status = fixture.run(binary, args, root, env, "continued")
            report["process"] = json.loads((root / "continued-guardian-result.json").read_text())
            report["answered"] = ANSWER in (root / "continued-stdout.jsonl").read_text()
            fixture.require(status == 0, "exec resume failed")
            banner = (root / "continued-stderr.txt").read_text().splitlines()
            fixture.require(f"reasoning effort: {report['original_effort']}" in banner,
                            "exec banner did not report the resumed reasoning effort")
        else:
            args = ["resume"]
            if surface != "picker":
                args.extend(["--last" if surface == "terminal-last" else report["thread"], CONTINUE])
            report["custody"], report["terminal_view_bytes"], report["terminal_resized"] = terminal_session(
                binary, root, env, surface, args, gateway, completed)
        terminal = (root / "terminal.raw").read_bytes() if surface in CHECK["TERMINAL_SURFACES"] else None
        CHECK["check_model_resume"](report, surface, binary, terminal, seed_binary=seed_binary)
        fixture.require(CHECK["sha256"](binary) == report["binary_sha256"], "runtime executable changed")
        if seed_binary != binary:
            fixture.require(CHECK["sha256"](seed_binary) == report["seed_binary_sha256"],
                            "original executable changed")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        fixture.cleanup_resources((("catalog", *listener),) if listener else (), (), report, sys.exc_info()[1])
        fixture.write_json(root / "result.json", report)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--seed-codex", type=Path,
                        help="Create the saved thread with this previous release (default: --codex).")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--surface", choices=tuple(CHECK["MODEL_SURFACES"]), default="terminal")
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), args.output.absolute(), args.surface,
                 args.seed_codex.resolve(strict=True) if args.seed_codex else None))
