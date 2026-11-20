#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Select one of two identically masked enrollment slots in a real Codex terminal."""

import argparse
import base64
import json
import os
from pathlib import Path
import re
import select
import socketserver
import subprocess
import sys
import tempfile
import threading
import time

import capacity_exec as fixture
import model_resume as terminal
from account_reconnect import stop_server
from sidebar_account_focus import painted_rows, read_runs


SID = "t0000000001"
TOKEN = "enrollment-fixture-only"
REVISION = "c" * 64
SLOTS = [("1" * 64, "Primary fixture"), ("2" * 64, "Secondary fixture")]
MASK = "f***@example.test"
LOOKUP = "bash alab/account-labels.sh"
ACCOUNT_LIST = "aterm harness accounts list --cli codex"
SUBMITTING = "Request pending · closing this view does not cancel enrollment"
PHASES = (
    ("pending", ("Request pending",)),
    ("challenge", ("Complete sign-in in aterm", "Account authentication required")),
    ("applied", ("Enrollment complete",)),
)


def progress_title(rows, phase_text, *, refreshable=True):
    """Read the title from the same freshly painted card as the phase."""
    visible = [row for row in rows if row]
    positions = [i for i, row in enumerate(visible) if row.startswith(phase_text)
                 and (not refreshable or i + 1 < len(visible) and "Refresh this operation" in visible[i + 1])]
    if len(positions) == 1:
        index = positions[0]
        return visible[index - 1] if index else ""
    return None


def enrollment_lookup(rows, first_target, picker_masks):
    if not picker_masks:
        return "duplicate masks unavailable"
    header = [index for index, row in enumerate(rows) if row == "Account enrollment"]
    first = rows.index(first_target)
    if len(header) != 1 or header[0] >= first:
        return "picker subtitle unavailable"
    subtitle = " ".join(rows[header[0] + 1:first])
    return "shown" if LOOKUP in " ".join(subtitle.split()) else "missing"


class Control(socketserver.StreamRequestHandler):
    def handle(self):
        self.connection.settimeout(3)
        try:
            if self.rfile.readline() != f"AUTH {TOKEN}\n".encode():
                return
            for line in self.rfile:
                with self.server.lock:
                    reply = self.server.reply(line.decode().strip())
                if reply.startswith("OK codex-accounts-change waiting "):
                    self.server.start_received.set()
                    if not self.server.release_start.wait(15):
                        return
                self.wfile.write((reply + "\n").encode())
        except (TimeoutError, ConnectionResetError, BrokenPipeError):
            return


class EnrollmentHost(socketserver.ThreadingUnixStreamServer):
    def __init__(self, root, handler, *, refuse=False):
        directory = root / "xdg/aterm"
        (directory / "graph").mkdir(parents=True)
        owner = os.getpid()
        endpoint = directory / f"aterm-{owner}.sock"
        (directory / "graph" / SID).write_text(f"sock {endpoint}\npid {owner}\n")
        (directory / f"aterm-{owner}.token").write_text(TOKEN + "\n")
        self.lock = threading.Lock()
        self.start_received = threading.Event()
        self.release_start = threading.Event()
        self.session = None
        self.changes = []
        self.starts = []
        self.operation = None
        self.refuse = refuse
        super().__init__(str(endpoint), handler)

    def message(self, prefix, schema=1, **fields):
        return "OK " + prefix + " " + json.dumps(dict(schema=schema, session=self.session, **fields))

    def reply(self, line):
        if line == "version":
            return "OK version=0.208.0"
        route, _, command = line.partition(" ")
        mutation = "codex-accounts-change" in line or "restart" in line.split()
        if mutation:
            self.changes.append(line)
        if route != "@" + SID or self.session is None:
            return "ERR unsupported"
        if match := re.fullmatch(r"codex-accounts enrollment-status schema=1 request=([a-f0-9]{32})", command):
            return self.message("codex-accounts enrollment-status", request=match[1], revision=REVISION,
                                capabilities={"enroll": True, "create": False}, operations=[],
                                targets=[dict(account_id=key, label=label) for key, label in SLOTS])
        if match := re.fullmatch(r"codex-accounts snapshot schema=([23]) request=([a-f0-9]{32})", command):
            schema = int(match[1])
            quota = dict(stale=True, observed_at=None, five_hour_used_percent=None, five_hour_reset_at=None,
                         weekly_used_percent=None, weekly_reset_at=None)
            # Display metadata arrives in the opposite order from the authoritative targets.
            accounts = [dict(id=key, label=label, masked_email=MASK, condition="unseen", enrolled=True,
                             eligible=False, held=False, quota=quota) for key, label in reversed(SLOTS)]
            if schema == 3:
                for account in accounts:
                    account["signed_out_reason"] = None
            return self.message("codex-accounts", schema=schema, request=match[2], revision=REVISION,
                                observed_at=1792700000,
                                assignment={"account_id": SLOTS[0][0]}, accounts=accounts,
                                pool=dict(choice_id=SLOTS[0][0], pinned=False, rotation="early"),
                                capabilities=dict(add=False, select=True, auto=False))
        if match := re.fullmatch(r"codex-accounts-change enroll schema=1 operation=([a-f0-9]{32}) "
                                 r"revision=([a-f0-9]{64}) account=([a-f0-9]{64})", command):
            self.starts.append(dict(operation=match[1], revision=match[2], account=match[3]))
            if self.operation or (match[2], match[3]) != (REVISION, SLOTS[1][0]):
                return "ERR wrong enrollment target, revision or duplicate start"
            self.operation = match[1]
            return self.message("codex-accounts-change waiting", operation=self.operation)
        if self.operation and line == self.poll_command() and len(self.changes) in (2, 3):
            phase = "running" if self.refuse else "authentication_required"
            if len(self.changes) == 3:
                phase = "refused" if self.refuse else "applied"
            return self.message("codex-accounts-change enrollment", operation=self.operation, account_id=SLOTS[1][0],
                                phase=phase, generation=7 if phase == "applied" else None)
        return "ERR unsupported"

    def poll_command(self):
        return f"@{SID} codex-accounts-change enrollment-poll schema=1 operation={self.operation}"

    def check_wire(self):
        start = (f"@{SID} codex-accounts-change enroll schema=1 operation={self.operation} "
                 f"revision={REVISION} account={SLOTS[1][0]}")
        return self.operation is not None and self.changes == [start, self.poll_command(), self.poll_command()]


def exercise(binary, root, env, host, report, *, check_picker_guidance):
    sock = root / "backend.sock"
    endpoint = f"unix://{sock}"
    argv = [str(binary), "--no-alt-screen", "--remote", endpoint]
    spec, raw, custody = root / "tmp/pty.json", root / "terminal.raw", root / "custody.json"
    fixture.write_json(spec, dict(argv=argv, cwd=str(root / "work"), env=env, raw=str(raw), result=str(custody)))

    @terminal.with_pty_lifeline
    def interact(process, _log):
        sequence = 0
        width = 228

        def command(**fields):
            nonlocal sequence
            sequence += 1
            process.stdin.write((json.dumps(dict(id=sequence, **fields)) + "\n").encode())
            process.stdin.flush()

        def send(data):
            command(write=base64.b64encode(data).decode())

        def dismiss():
            for timeout in (1, 12):
                send(b"\x1b")
                time.sleep(.15)  # Let the terminal dispatch Escape before sending Ctrl-C.
                send(b"\x03")
                try:
                    process.wait(timeout=timeout)
                    return
                except subprocess.TimeoutExpired:
                    if timeout == 12:
                        raise

        def wait_for(label, observe):
            deadline = time.monotonic() + 12
            while process.poll() is None and time.monotonic() < deadline:
                if result := observe():
                    return result
                time.sleep(.05)
            raise RuntimeError(f"terminal did not show {label}")

        def frame(label, matches):
            nonlocal width

            def repaint():
                nonlocal width
                offset = raw.stat().st_size
                width = 232 if width == 228 else 228
                command(resize=[32, width])
                end = time.monotonic() + .5
                with raw.open("rb") as trace:
                    while time.monotonic() < end:
                        trace.seek(offset)
                        rows = [row.strip() for row in painted_rows(trace.read(), width)]
                        if matches(rows):
                            return rows
                        time.sleep(.05)
                return None

            return wait_for(label, repaint)

        fixture.require(select.select([process.stdout], [], [], 10)[0], "terminal guardian did not start")
        pid = json.loads(process.stdout.readline())["started"]
        birth = subprocess.check_output(["/bin/ps", "-p", str(pid), "-o", "lstart="], env={"LC_ALL": "C", "TZ": "UTC"},
                                        text=True, timeout=5)
        with host.lock:
            host.session = dict(sid=SID, process_pid=pid, process_birth=" ".join(birth.split()), epoch="fixture")
        wait_for("startup", lambda: b"Codex" in read_runs(raw, 0))
        send(b"\x1b[200~/accounts add\x1b[201~")
        wait_for("/accounts add", lambda: b"/accountsadd" in read_runs(raw, 0))
        send(b"\r")
        first, selected = [label for _, label in SLOTS]
        rows = frame("both slot labels", lambda lines: all(any(label in row for row in lines)
                     for label in (first, selected, "Refresh enrollment status")))
        picker = [row for row in rows if first in row or selected in row]
        report["picker_masks"] = (len(picker) == 2 and first in picker[0] and selected in picker[1]
                                  and all(MASK in row for row in picker))
        if check_picker_guidance:
            report["picker_guidance"] = enrollment_lookup(rows, picker[0], report["picker_masks"])
            dismiss()
            return
        send(b"\x1b[B")
        frame("second slot focused", lambda lines: any(row.startswith("›") and selected in row for row in lines))
        with host.lock:
            fixture.require(not host.changes, "selection moved the host before Enter")
        send(b"\r")
        expected_title = "Account enrollment · " + selected
        try:
            fixture.require(host.start_received.wait(12), "host did not receive the selected enrollment")
            rows = frame("unacknowledged enrollment", lambda lines: progress_title(
                lines, SUBMITTING, refreshable=False) is not None)
            report["selected_title"]["submitting"] = progress_title(rows, SUBMITTING, refreshable=False) == expected_title
            with host.lock:
                fixture.require(len(host.changes) == 1, "unexpected host mutation before start acknowledgment")
        finally:
            host.release_start.set()
        phases = (PHASES[0], ("running", ("Enrollment running",))) if host.refuse else PHASES
        for index, (phase, texts) in enumerate(phases):
            rows = frame(phase, lambda lines: progress_title(lines, texts) is not None)
            report["selected_title"][phase] = progress_title(rows, texts) == expected_title
            with host.lock:
                fixture.require(len(host.changes) == index + 1, f"unexpected host mutation count at {phase}")
            if phase != "applied":
                send(b"\r")
        if host.refuse:
            # Wait for the whole card, then read its subtitle up to the focused action.
            rows = frame("confirmed refusal", lambda lines:
                         progress_title(lines, "Enrollment refused", refreshable=False) is not None
                         and any("Refresh enrollment status" in row for row in lines))
            title = progress_title(rows, "Enrollment refused", refreshable=False)
            report["selected_title"]["refused"] = title == expected_title
            start = next(i for i, row in enumerate(rows) if row.startswith("Enrollment refused"))
            action = next((i for i in range(start + 1, len(rows)) if rows[i].startswith("›")), len(rows))
            subtitle = " ".join(" ".join(rows[start:action]).split())
            report["refusal_guidance"] = ACCOUNT_LIST in subtitle
            report["open_accounts"] = action < len(rows) and "Open /accounts" in rows[action]
            report["refresh_offered"] = any("Refresh this operation" in row for row in rows[start:])
            report["accounts_opened"] = False
            if report["open_accounts"]:
                send(b"\r")
                frame("current subscription accounts", lambda lines: any(
                    f"Session account (by its launch login): {first}" in row for row in lines))
                report["accounts_opened"] = True
        dismiss()

    server = subprocess.Popen([binary, "app-server", "--listen", endpoint], cwd=root / "work", env=env,
                              stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        deadline = time.monotonic() + 12
        while not sock.is_socket():
            fixture.require(server.poll() is None and time.monotonic() < deadline, "fixture app-server did not start")
            time.sleep(.05)
        guardian = ["-I", "-B", "-S", str(Path(__file__).with_name("history_pagination.py")), "--guardian", str(spec)]
        fixture.require(fixture.run(sys.executable, guardian, root, env, "terminal", interact=interact,
                                    timeout_seconds=110) == 0, "terminal guardian failed")
        terminal.CHECK["check_model_terminal_exit"](json.loads(custody.read_text()), argv)
    finally:
        def record_custody():
            if custody.exists():
                evidence = json.loads(custody.read_text())
                report["terminal_custody"] = {key: evidence.get(key) for key in
                                              ("exit", "forced", "owning_reap", "group_absent_after_reap")}
                fixture.require(evidence.get("owning_reap") and evidence.get("group_absent_after_reap"),
                                "terminal group retirement unconfirmed")

        fixture.cleanup_resources((), (("terminal custody", record_custody),), report, sys.exc_info()[1],
                                  stop=lambda: stop_server(server))


def run(binary, output, *, check_picker_guidance=False, check_refusal_guidance=False):
    output.mkdir(mode=0o700)
    report = dict(passed=False, binary_sha256=terminal.CHECK["sha256"](binary))
    if not check_picker_guidance:
        report["selected_title"] = {}
    try:
        with tempfile.TemporaryDirectory(prefix="codex-enroll-", dir="/tmp") as directory:
            root = Path(directory).resolve()
            env = fixture.fresh_environment(root)
            env.update(TERM="xterm-256color", TERM_PROGRAM="xterm", ATERM_PARENT_SESSION_ID=SID,
                       HTTP_PROXY="http://127.0.0.1:1", HTTPS_PROXY="http://127.0.0.1:1",
                       NO_PROXY="127.0.0.1,localhost")
            fixture.configure(root, 1)
            config = root / "codex-home/config.toml"
            project = json.dumps(str(root / "work"))
            config.write_text('features.daemon_auto_start = false\nanalytics.enabled = false\n'
                              'check_for_update_on_startup = false\nopenai_base_url = "http://127.0.0.1:1/v1"\n'
                              'chatgpt_base_url = "http://127.0.0.1:1/backend-api"\n'
                              + config.read_text() + f'\n[projects.{project}]\ntrust_level = "trusted"\n')
            host, worker = fixture.start_listener(Control, bind=lambda _, handler: EnrollmentHost(
                root, handler, refuse=check_refusal_guidance))

            def record_wire():
                if not check_picker_guidance:
                    report["selected_wire"] = host.check_wire()
                report["starts"] = host.starts
                report["matching_polls"] = host.changes.count(host.poll_command())
                report["mutation_count"] = len(host.changes)

            try:
                exercise(binary, root, env, host, report, check_picker_guidance=check_picker_guidance)
            finally:
                host.release_start.set()
                fixture.cleanup_resources((("control", host, worker),), (("wire", record_wire),),
                                          report, sys.exc_info()[1])
        fixture.require("cleanupErrors" not in report, "fixture cleanup failed")
        fixture.require(terminal.CHECK["sha256"](binary) == report["binary_sha256"], "executable changed")
        if check_picker_guidance:
            fixture.require(report["mutation_count"] == 0, "viewing the picker changed an enrollment")
            fixture.require(report["picker_masks"], "enrollment picker did not show both identically masked targets")
            fixture.require(report["picker_guidance"] != "picker subtitle unavailable", "enrollment picker subtitle unavailable")
            fixture.require(report["picker_guidance"] == "shown", "duplicate-mask enrollment lookup missing")
        else:
            fixture.require(report["selected_wire"], "expected one start for the second slot and two polls of its operation")
            if check_refusal_guidance:
                fixture.require(report["refusal_guidance"], "confirmed refusal omitted the native account list command")
                fixture.require(report["selected_title"]["refused"], "confirmed refusal lost the selected slot label")
                fixture.require(not report["refresh_offered"], "confirmed refusal still offered to poll the operation")
                fixture.require(report["accounts_opened"], "confirmed refusal did not open the subscription accounts view")
            else:
                phases = ["submitting", *(phase for phase, _ in PHASES)]
                missing = [phase for phase in phases if not report["selected_title"].get(phase)]
                if not report["picker_masks"]:
                    missing.append("picker masks")
                fixture.require(not missing, "missing enrollment slot identity in " + ", ".join(missing))
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    fixture.write_json(output / "result.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path, help="new directory for the fixture result")
    checks = parser.add_mutually_exclusive_group()
    checks.add_argument("--check-picker-guidance", action="store_true",
                        help="check the local enrollment picker's duplicate-mask lookup before choosing a slot")
    checks.add_argument("--check-refusal-guidance", action="store_true",
                        help="refuse the accepted operation and check its account-list command and navigation")
    args = parser.parse_args()
    os.umask(0o077)
    result = run(args.codex.resolve(strict=True), args.output.resolve(), check_picker_guidance=args.check_picker_guidance,
                 check_refusal_guidance=args.check_refusal_guidance)
    print(result.get("error", "passed"))
    sys.exit(0 if result["passed"] else 1)
