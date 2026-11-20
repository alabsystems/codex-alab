#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise typed and picked switches against an isolated aterm that only understands labels.

Every restart is refused. Once the picker is confirmed, the fake changes its slot
binding immediately after serving the next snapshot. A client may withhold the
picker on this host. If it opens one, Enter must never dispatch a label-only
restart. The separate typed control still resolves the label and gets a refusal.
This checks compatibility at the request boundary. It does not exercise a native
host that implements identity-bound relaunch or authenticate a destination.
"""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import select
import subprocess
import sys
import tempfile
import time

import capacity_exec as fixture
import model_resume as terminal
from account_enrollment import Control, EnrollmentHost, SID
from provider_fixture import LoopbackHTTPServer
from sidebar_account_focus import painted_rows, read_runs


MASK = "s***@example.invalid"


def shows_quota(rows, label, condition, used):
    starts = [i for i, row in enumerate(rows) if MASK in row and re.search(rf"\b{label}\b", row)]
    if len(starts) != 1:
        return False
    # These short rows fit the fixture's 228-column pane. Bind the weekly
    # reading to this masked slot, even if its sibling displays the same value.
    details = rows[starts[0] + 1:starts[0] + 4]
    return (len(details) == 3 and re.search(rf"\b{condition}\b", details[0]) is not None
            and "5h:" in details[1] and f"Weekly: {used}% used" in details[2])


def routed_command(line):
    """An optional @selector and command. The native request head splits on space and tab."""
    parts = re.split(r"[ \t]+", line.strip(" \t"), maxsplit=1)
    if parts[0].startswith("@"):
        return parts[0], parts[1] if len(parts) > 1 else ""
    return None, line.strip(" \t")


def legacy_restart(command):
    words = command.split()
    if words[:1] != ["restart"]:
        return False
    # Each of these arguments is understood by the old host. None binds an account.
    return all(re.fullmatch(r"(?:pid|quiet)=[0-9]+|if-stalled|account=[a-z0-9][a-z0-9._-]*|account-auto", word)
               for word in words[1:])


def check_dispatch(mode, restarts, pid):
    if mode == "typed":
        fixture.require(restarts == [("@" + SID, f"restart pid={pid} account=standby")],
                        "explicit named switch did not issue its single legacy request")
    else:
        fixture.require(not any(legacy_restart(command) for _, command in restarts),
                        "confirmed picker issued a label-only restart")


class JoinedProvider(LoopbackHTTPServer):
    # server_close joins every accepted HTTP request before the case counts model turns.
    daemon_threads = False


class LegacyHost(EnrollmentHost):
    def __init__(self, root, handler):
        super().__init__(root, handler)
        self.rebound = False
        self.standby_used = None
        self.after_confirmation = False
        self.snapshots = []
        self.restarts = []
        self.unsupported_after_confirmation = []
        self.unsupported_changes = []

    def reply(self, line):
        if line == "version":
            return "OK version=0.208.0"
        route, command = routed_command(line)
        words = command.split()
        restart = words[:1] == ["restart"]
        change = words[:1] == ["codex-accounts-change"]
        # Observe what the client sent before deciding whether this host would answer it.
        if restart:
            self.restarts.append((route, command))
        if change:
            self.unsupported_changes.append((route, words[:2]))
        if route != "@" + SID or self.session is None:
            return "ERR unsupported"
        if restart:
            if legacy_restart(command):
                return "ERR restart unavailable: account-limited"
            return "ERR bad restart argument: unsupported"
        match = re.fullmatch(r"codex-accounts snapshot schema=([23]) request=([a-f0-9]{32})", command)
        if not match:
            if self.after_confirmation:
                self.unsupported_after_confirmation.append((route, words[:2]))
            return "ERR unsupported"
        schema = int(match[1])
        current = "replacement" if self.rebound else "original"
        revision = hashlib.sha256(f"{current}/{self.standby_used}".encode()).hexdigest()
        quota = dict(stale=True, observed_at=None, five_hour_used_percent=None, five_hour_reset_at=None,
                     weekly_used_percent=None, weekly_reset_at=None)
        rows = []
        for label in ("source", "standby"):
            # The real account ID is derived from the label, even after its binding changes.
            key = hashlib.sha256(b"aterm-codex-pool-account-v1\0" + label.encode()).hexdigest()
            row = dict(id=key, label=label, masked_email=MASK, condition="usable",
                       enrolled=True, eligible=label == "standby", held=False, quota=quota)
            if self.standby_used is not None:
                used = self.standby_used if label == "standby" else 23
                row["quota"] = dict(quota, stale=False, observed_at=1792700000, weekly_used_percent=used)
                if used == 100:
                    row.update(condition="limited", eligible=False)
            if schema == 3:
                row["signed_out_reason"] = None
            rows.append(row)
        self.snapshots.append(current)
        result = self.message("codex-accounts", schema=schema, request=match[2], revision=revision,
                              observed_at=1792700000, assignment={"account_id": rows[0]["id"]},
                              accounts=rows, pool=dict(choice_id=rows[0]["id"], pinned=False, rotation="early"),
                              capabilities=dict(add=False, select=True, auto=False))
        if self.after_confirmation:
            self.rebound = True
        return result


def exercise(binary, directory, mode, report):
    env = fixture.fresh_environment(directory)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm", ATERM_PARENT_SESSION_ID=SID,
               HTTP_PROXY="http://127.0.0.1:1", HTTPS_PROXY="http://127.0.0.1:1", NO_PROXY="127.0.0.1,localhost")
    listeners, exports = [], []
    provider = terminal.Gateway([])
    try:
        def record_provider():
            with provider.lock:
                requests = [row for row in provider.events if row["event"] in ("workspace_request", "request")]
                report["provider_calls"] = len(requests)
                report["original_turn_only"] = len(requests) == 1 and requests[0]["seed_prompt"]

        handler = provider.handler()
        handler.timeout = 3
        gateway = fixture.start_listener(handler, bind=JoinedProvider)
        listeners.append(("provider", *gateway))
        exports.append(("provider calls", record_provider))
        terminal.configure(directory, gateway[0].server_port, "ultra")
        (directory / "codex-home/auth.json").write_text(json.dumps({"OPENAI_API_KEY": "sk-loopback-fixture"}))
        host, worker = fixture.start_listener(Control, bind=lambda _, handler: LegacyHost(directory, handler))
        listeners.append(("control", host, worker))

        def record_wire():
            with host.lock:
                report.update(snapshots=list(host.snapshots), rebound=host.rebound,
                              restarts=[dict(route=route, command=cmd) for route, cmd in host.restarts
                                        if legacy_restart(cmd)],
                              unsupported_restarts=[dict(route=route, command=cmd) for route, cmd in host.restarts
                                                    if not legacy_restart(cmd)],
                              unsupported_changes=list(host.unsupported_changes),
                              unsupported_after_confirmation=list(host.unsupported_after_confirmation))

        exports.append(("switch requests", record_wire))
        argv = [str(binary), "--no-daemon", "--no-alt-screen", terminal.SEED]
        raw, custody, spec = (directory / name for name in ("terminal.raw", "custody.json", "terminal.json"))

        def record_custody():
            if custody.exists():
                result = json.loads(custody.read_text())
                report["terminal_custody"] = {key: result.get(key) for key in
                                              ("exit", "forced", "owning_reap", "group_absent_after_reap")}

        exports.append(("terminal custody", record_custody))
        fixture.write_json(spec, dict(argv=argv, cwd=str(directory / "work"), env=env, raw=str(raw), result=str(custody)))

        @terminal.with_pty_lifeline
        def interact(process, _log):
            sequence, width = 0, 228

            def command(**fields):
                nonlocal sequence
                sequence += 1
                process.stdin.write((json.dumps(dict(id=sequence, **fields)) + "\n").encode())
                process.stdin.flush()

            def send(keys):
                command(write=base64.b64encode(keys).decode())

            def wait(label, observe):
                deadline = time.monotonic() + 12
                while process.poll() is None and time.monotonic() < deadline:
                    if result := observe():
                        return result
                    time.sleep(.05)
                raise RuntimeError(f"terminal did not reach {label}")

            def frame(label, matches):
                def repaint():
                    nonlocal width
                    start = raw.stat().st_size
                    width = 232 if width == 228 else 228
                    command(resize=[32, width])
                    until = time.monotonic() + .6
                    while time.monotonic() < until:
                        with raw.open("rb") as trace:
                            trace.seek(start)
                            rows = [row.strip() for row in painted_rows(trace.read(), width)]
                        if matches(rows):
                            return rows
                        time.sleep(.05)
                    return None
                return wait(label, repaint)

            def submit(text):
                start = raw.stat().st_size
                send(b"\x1b[200~" + text.encode() + b"\x1b[201~")
                wait("typed command", lambda: b"".join(text.encode().split()) in read_runs(raw, start))
                send(b"\r")

            fixture.require(select.select([process.stdout], [], [], 8)[0], "guardian did not start")
            pid = json.loads(process.stdout.readline())["started"]
            birth = subprocess.check_output(["/bin/ps", "-p", str(pid), "-o", "lstart="], env={"LC_ALL": "C", "TZ": "UTC"},
                                            text=True, timeout=5)
            with host.lock:
                host.session = dict(sid=SID, process_pid=pid, process_birth=" ".join(birth.split()), epoch="fixture")
            wait("initial turn", lambda: b"".join(terminal.REPLY.encode().split()) in read_runs(raw, 0))
            terminal.wait_for_turn_completion(directory, provider, process, terminal.REPLY, time.monotonic() + 12)
            if mode == "typed":
                with host.lock:
                    host.standby_used = 100
            submit("/accounts")

            def roster(rows):
                return all(any(mark in row for row in rows) for mark in ("Subscription accounts", "source", "standby"))

            rows = frame("two account rows", roster)
            if mode == "typed":
                def pair_shown(rows, condition, used):
                    return shows_quota(rows, "source", "usable", 23) and shows_quota(rows, "standby", condition, used)

                frame("limited standby account", lambda rows: pair_shown(rows, "limited", 100))
                with host.lock:
                    before = len(host.snapshots)
                    host.standby_used = 12
                send(b"\r")
                wait("explicit roster refresh", lambda: len(host.snapshots) > before)
                frame("available standby account", lambda rows: pair_shown(rows, "usable", 12))
                with host.lock:
                    fixture.require(not host.restarts and not host.unsupported_changes,
                                    "refresh dispatched an account change")
                report["standby_limit_cleared_on_refresh"] = True
                send(b"\x1b")
                time.sleep(.2)
                submit("/accounts use standby")
                frame("host quota refusal", lambda rows: any("That account is at a usage limit." in row for row in rows))
            else:
                offered = any("switch account" in row for row in rows if "refresh" in row)
                report["picker_offered"] = offered
                send(b"\x1b[C")
                if offered:
                    frame("focused standby row", lambda rows: any(row == "Switch account" for row in rows)
                          and any(row.startswith("›") and "standby" in row for row in rows))
                    with host.lock:
                        fixture.require(not host.restarts and not host.unsupported_changes,
                                        "account change sent before confirmation")
                        host.after_confirmation = True
                    send(b"\r")
                    frame("completed selection on legacy host", lambda rows: any(
                        word in row.casefold() for row in rows for word in ("unavailable", "cannot", "refused",
                                                                          "did not", "could not", "usage limit",
                                                                          "does not support", "update aterm")))
                else:
                    # Verify Right left focus on the roster: Enter refreshes it and the
                    # two existing slots are painted again, without dispatching a restart.
                    frame("legacy roster after right navigation", roster)
                    with host.lock:
                        before = len(host.snapshots)
                    send(b"\r")
                    wait("roster refresh", lambda: len(host.snapshots) > before)
                    frame("refreshed legacy roster", roster)
            send(b"\x1b")
            time.sleep(.15)
            for timeout in (1, 10):
                send(b"\x03")
                try:
                    process.wait(timeout=timeout)
                    break
                except subprocess.TimeoutExpired:
                    if timeout == 10:
                        raise

        guardian = ["-I", "-B", "-S", str(Path(__file__).with_name("history_pagination.py")), "--guardian", str(spec)]
        fixture.require(fixture.run(sys.executable, guardian, directory, env, "terminal", interact=interact,
                                    timeout_seconds=90) == 0, "terminal guardian failed")
        terminal.CHECK["check_model_terminal_exit"](json.loads(custody.read_text()), argv)
    finally:
        # Requests accepted before terminal exit may still be waiting on the handler lock.
        # Drain all handlers on both listeners before freezing any observations for a verdict.
        fixture.cleanup_resources(reversed(listeners), exports, report, sys.exc_info()[1])
    fixture.require("cleanupErrors" not in report, "switch fixture cleanup failed")
    fixture.require(report["original_turn_only"] and report["snapshots"],
                    "expected one loopback turn and a verified roster")
    restarts = [(row["route"], row["command"]) for row in report["restarts"] + report["unsupported_restarts"]]
    check_dispatch(mode, restarts, host.session["process_pid"])


def run(binary, output):
    output.mkdir(mode=0o700)
    report = dict(passed=False, binary_sha256=terminal.CHECK["sha256"](binary))
    errors = []
    for mode in ("typed", "picked"):
        report[mode] = {}
        try:
            with tempfile.TemporaryDirectory(prefix="account-pick-", dir="/tmp") as root:
                exercise(binary, Path(root).resolve(), mode, report[mode])
        except Exception as error:
            report[mode].update(fixture.case_error_fields(error))
            errors.append(mode)
    if errors:
        report["error"] = "failed " + ", ".join(errors)
    elif terminal.CHECK["sha256"](binary) != report["binary_sha256"]:
        report["error"] = "executable changed"
    else:
        report["passed"] = True
    fixture.write_json(output / "result.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    result = run(args.codex.resolve(strict=True), args.output.resolve())
    print(result.get("error", "passed"))
    sys.exit(0 if result["passed"] else 1)
