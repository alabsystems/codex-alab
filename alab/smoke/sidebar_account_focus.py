#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Keep the visible side chat, parent draft, and weekly quota through TUI focus changes."""

import argparse
import json
import os
from pathlib import Path
import sys
import time
from urllib.parse import urlsplit

import account_resume
import capacity_exec as fixture
import model_resume
import provider_fixture
from renewal_follow import fixture_tls
from terminal_controls import drive_pty, painted_rows, read_runs


PARENT = "Open the parent sample."
SIDE = "Open the sidebar sample."
PARENT_REPLY = "PARENT-EMBER"
SIDE_REPLY = "SIDELIGHT-COBALT"
PARENT_DRAFT = "Parent draft cedar"
SIDE_DRAFT = "Side draft spruce"
ACCOUNT = "focus@example.invalid"
QUOTA = "weekly usage 23%"


class Gateway(account_resume.Gateway):
    def handler(self):
        gateway = self

        class Handler(super().handler()):
            def do_GET(self):
                if urlsplit(self.path).path != "/backend-api/wham/usage":
                    return super().do_GET()
                if gateway.tokens.get(self.headers.get("authorization")) != "focus":
                    self.send_error(401)
                    return
                data = provider_fixture.usage("workspace-focus", "user-focus", week_used=23, session_used=11,
                                              allowed=True)
                self.send_body("application/json", json.dumps(data).encode())

            def send_body(self, mime, data, headers=()):
                if mime == "text/event-stream":
                    headers = [*headers, ("x-codex-secondary-used-percent", "23"),
                               ("x-codex-secondary-window-minutes", "10080"),
                               ("x-codex-secondary-reset-at", str(int(time.time()) + 86400)),
                               ("x-codex-primary-used-percent", "11"),
                               ("x-codex-primary-window-minutes", "300"),
                               ("x-codex-primary-reset-at", str(int(time.time()) + 3600))]
                return super().send_body(mime, data, headers)

        return Handler

    def response(self, body, headers):
        users = model_resume.messages(body, "user")
        answer = "Sidebar focus sample"
        if users and users[-1] in (PARENT, SIDE, PARENT_DRAFT + " returns"):
            prompt = users[-1]
            answer = {PARENT: PARENT_REPLY, SIDE: SIDE_REPLY, PARENT_DRAFT + " returns": "PARENT-DRAFT-ACCEPTED"}[prompt]
            with self.lock:
                self.requests.append({"prompt": prompt, "thread": headers.get("thread-id"),
                                      "login": self.tokens.get(headers.get("authorization")),
                                      "account": headers.get("chatgpt-account-id")})
        return fixture.response(answer, {"type": "message", "id": answer, "role": "assistant",
                                "content": [{"type": "output_text", "text": answer}]})


def frame_evidence(rows, width, name, markers):
    """Use the focused pane's own footer and the correct pane for each draft."""
    sides = {"left": [line[:width // 2 - 1] for line in rows],
             "right": [line[width // 2:] for line in rows]}
    focus = "right" if name == "side" else "left"
    evidence = {}
    for marker in markers:
        if marker == QUOTA:
            area = rows if name == "initial" else sides[focus]
            matching = [line.strip() for line in area if ACCOUNT in line and
                        ("weekly [██░░░░░░] 23% used" in line or "weekly 77% left" in line)]
        else:
            side = "right" if marker.startswith((SIDE_REPLY, SIDE_DRAFT)) else "left"
            if marker == ACCOUNT:
                side = focus
            area = rows if name == "initial" else sides[side]
            matching = [line.strip() for line in area if marker in line]
        if not matching:
            return None
        evidence[marker] = matching[0]
    return evidence


def drive_terminal(binary, root, env, report, *, cold_start_only=False, seed=False, parent=None):
    def exercise(terminal):
        def focus(key):
            terminal.send(b"\x18")  # Default chord leader, Ctrl-X.
            terminal.send(key.encode())

        def frame(name, width, markers):
            capture = terminal.frame(width, lambda rows: frame_evidence(rows, width, name, markers),
                                     f"{name} pane content")
            report["frames"][f"seed-{name}" if seed else name] = capture

        terminal.command(resize=[32, 232])
        terminal.displayed(PARENT_REPLY)
        initial = (PARENT_REPLY,) if cold_start_only else (PARENT_REPLY, ACCOUNT, QUOTA)
        frame("initial", 228, initial)
        if parent:
            focus("s")
            terminal.displayed("› ○ " + SIDE)
        else:
            terminal.paste("/side " + SIDE, "sidebar sample.")
        terminal.send(b"\r")
        terminal.displayed(SIDE_REPLY)
        if cold_start_only:
            frame("cold-side", 232, (PARENT_REPLY, SIDE_REPLY))
            focus("c")
            terminal.paste("/quit", "/quit")
            terminal.send(b"\r")
            terminal.process.wait(timeout=12)
            return
        # /side leaves its child focused. Park a distinctive child draft,
        # then switch to main and compose a different unsent draft there.
        terminal.paste(SIDE_DRAFT, SIDE_DRAFT)
        focus("c")
        terminal.paste(PARENT_DRAFT, PARENT_DRAFT)
        markers = (PARENT_REPLY, SIDE_REPLY, PARENT_DRAFT, SIDE_DRAFT, ACCOUNT, QUOTA)
        frame("main", 232, markers)
        focus("s")
        terminal.paste(" focused", "focused")
        frame("side", 228, (PARENT_REPLY, SIDE_REPLY, PARENT_DRAFT, SIDE_DRAFT + " focused", ACCOUNT, QUOTA))
        focus("c")
        terminal.paste(" returns", "returns")
        frame("returned", 232, (PARENT_REPLY, SIDE_REPLY, PARENT_DRAFT + " returns",
                                 SIDE_DRAFT + " focused", ACCOUNT, QUOTA))
        terminal.send(b"\r")
        terminal.displayed("PARENT-DRAFT-ACCEPTED")
        terminal.quit()

    cli = [str(binary), "resume", parent] if parent else [str(binary), PARENT]
    drive_pty(cli, root, env, "seed-terminal" if seed else "terminal", exercise)


def run(binary, root, *, cold_start_only=False, min_stack=None, seed_binary=None):
    root.mkdir(mode=0o700)
    fixture.require(hasattr(os, "waitid") and hasattr(os, "WNOWAIT"), "PTY smoke requires os.waitid and WNOWAIT")
    env = fixture.fresh_environment(root)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm", HTTP_PROXY="http://127.0.0.1:1",
               HTTPS_PROXY="http://127.0.0.1:1", NO_PROXY="localhost,127.0.0.1,::1",
               CODEX_CA_CERTIFICATE=str(root / "fixture-ca.pem"))
    if min_stack is not None:
        env["RUST_MIN_STACK"] = str(min_stack)
    report = {"passed": False, "cold_start_only": cold_start_only, "min_stack": min_stack,
              "events": [], "requests": [], "frames": {},
              "binary_sha256": model_resume.CHECK["sha256"](binary)}
    gateway = Gateway(report)
    listener = None
    try:
        def prepare(server):
            server.socket = fixture_tls(root).wrap_socket(server.socket, server_side=True)

        listener = fixture.start_listener(gateway.handler(), prepare=prepare)
        port = listener[0].server_port
        account_resume.select_login(root, gateway, "focus", f"https://127.0.0.1:{port}")
        model_resume.configure(root, port, "high", tls=True)
        config = root / "codex-home/config.toml"
        config.write_text('cli_auth_credentials_store = "file"\ntui.sidebar_width_percent = 50\n'
                          'tui.animations = false\ntui.fullscreen_transcript = true\ntui.alternate_screen = "always"\n'
                          'features.tui_workspace = true\n' + config.read_text())
        if seed_binary:
            report["seed_sha256"] = model_resume.CHECK["sha256"](seed_binary)
            drive_terminal(seed_binary, root, env, report, cold_start_only=True, seed=True)
            fixture.require([row["prompt"] for row in report["requests"]] == [PARENT, SIDE],
                            "seed did not open exactly one parent and child")
            drive_terminal(binary, root, env, report, parent=report["requests"][0]["thread"])
            fixture.require(model_resume.CHECK["sha256"](seed_binary) == report["seed_sha256"], "seed binary changed")
        else:
            drive_terminal(binary, root, env, report, cold_start_only=cold_start_only)
        rows = report["requests"]
        expected = [PARENT, SIDE] if cold_start_only else [PARENT, SIDE, PARENT_DRAFT + " returns"]
        fixture.require([row["prompt"] for row in rows] == expected,
                        "unexpected submitted prompts")
        fixture.require(rows[0]["thread"] != rows[1]["thread"], "side prompt ran on the parent")
        if not cold_start_only:
            fixture.require(rows[0]["thread"] == rows[2]["thread"], "submitted the parent draft on another thread")
        fixture.require(all(row["thread"] and row["login"] == "focus" and row["account"] == "workspace-focus"
                            for row in rows), "a request used the wrong account")
        fixture.require(model_resume.CHECK["sha256"](binary) == report["binary_sha256"], "binary changed")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        fixture.cleanup_resources((("gateway", *listener),) if listener else (), (), report, sys.exc_info()[1])
        for label in ("terminal", "seed-terminal"):
            custody = root / (label + "-custody.json")
            if custody.is_file():
                record = json.loads(custody.read_text())
                report[label] = {key: record.get(key) for key in
                                 ("exit", "forced", "owning_reap", "group_absent_after_reap")}
        fixture.write_json(root / "result.json", report)
    print(report.get("error", "passed" if report["passed"] else "failed"))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--seed-binary", type=Path, help="Open the side chat with this CLI, then resume it with --codex")
    mode.add_argument("--cold-start-only", action="store_true",
                        help="Check cold /side creation and both visible replies on either edition")
    parser.add_argument("--min-stack", type=int, help="Set RUST_MIN_STACK for a diagnostic comparison")
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), args.output.resolve(),
                 cold_start_only=args.cold_start_only, min_stack=args.min_stack,
                 seed_binary=args.seed_binary.resolve(strict=True) if args.seed_binary else None))
