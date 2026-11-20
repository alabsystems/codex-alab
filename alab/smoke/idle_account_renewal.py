#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Hold the idle TUI's ordinary /status OAuth refresh at a synthetic issuer."""

import argparse
import json
from pathlib import Path
import sys
import threading
import time

import account_resume as account
import capacity_exec as fixture
import model_resume as terminal
import ordinary_account_renewal as renewal
from terminal_controls import drive_pty


class IdleIssuer(account.RenewalGateway):
    def __init__(self, report):
        super().__init__(report)
        self.pending = threading.Event()
        self.release = threading.Event()

    def exchange(self, payload):
        response = self.slot.exchange(payload)  # Verifies the source credential, client, grant and refresh reason.
        with self.lock:
            self.tokens["Bearer " + self.slot.new_access] = "source"
        self.pending.set()
        fixture.require(self.release.wait(20), "idle refresh response was never released")
        return response


def files(slots):
    def fingerprint(path):
        device, inode, modified, digest = renewal.fingerprint(path)
        return {"device": device, "inode": inode, "mtime_ns": modified, "sha256": digest.hex()}
    return {name: fingerprint(path) for name, path in slots.items()}


def check_writes(before, held, after):
    fixture.require(held == before, "a credential changed before its issuer replied")
    fixture.require(after["destination"] == before["destination"], "idle A refresh changed B's credential")
    fixture.require(all(after["source"][k] != before["source"][k] for k in ("inode", "sha256")),
                    "idle refresh did not atomically replace A's credential with new data")


def observe(pty, root, gateway, slots, before, report):
    pty.displayed(terminal.REPLY)
    turn = terminal.wait_for_turn_completion(root, gateway, pty.process, terminal.REPLY, time.monotonic() + 20)
    with gateway.lock:
        original = list(gateway.requests)
    source_turn = [r for r in original if r["turn"] == turn]
    fixture.require(len(source_turn) == 2 and source_turn[0]["thread"] == source_turn[1]["thread"]
                    and all((r["login"], r["account"]) == ("source", "workspace-source") for r in original),
                    "the original model and tool requests did not belong to A")
    seconds = gateway.slot.expires - time.time()
    fixture.require(seconds > 300 and not gateway.pending.is_set(), "seed failed to reach idle before the refresh window")
    report.update(thread=source_turn[0]["thread"], turn=turn, seconds_to_expiry_at_idle=seconds)
    time.sleep(max(0, seconds - 299.5))
    fixture.require(not gateway.pending.is_set(), "refresh preceded the idle /status command")
    # Distinct keypresses keep the terminal's paste detector from treating slash text as a pasted draft.
    for key in b"/status":
        pty.send(bytes([key]))
        time.sleep(.11)
    pty.displayed("/status")
    pty.send(b"\r")
    fixture.require(gateway.pending.wait(12), "/status did not reach A's private token endpoint")
    held = files(slots)
    with gateway.lock:
        fixture.require(gateway.requests == original, "idle /status ran inference while refreshing")
    gateway.release.set()
    deadline = time.monotonic() + 8
    while files(slots)["source"] == before["source"] and time.monotonic() < deadline:
        time.sleep(.05)
    gateway.slot.check_saved_credential()
    after = files(slots)
    check_writes(before, held, after)
    report.update(files_before=before, files_while_held=held, files_after=after)
    pty.quit()
    return original


def run(binary, root, shared):
    root.mkdir(mode=0o700)
    report = {"passed": False, "events": [], "requests": [], "binary_sha256": terminal.CHECK["sha256"](binary),
              "source_file": "shared" if shared else "named"}
    gateway = IdleIssuer(report)
    listener = None
    try:
        listener = account.start_gateway(root, gateway)
        env, issuer = account.configure_account_home(root, listener[0].server_port)
        slot = gateway.slot = renewal.Slot(root, "source", 315, issuer, account="workspace-source")
        if shared:
            slot.path = slot.path.replace(root / "codex-home/auth.json")
        else:
            env["CODEX_AUTH_PATH"] = str(slot.path)
        env.update(CODEX_REFRESH_TOKEN_URL_OVERRIDE=issuer + "/oauth/token", TERM="xterm-256color", TERM_PROGRAM="xterm")
        slots = {"source": slot.path, "destination": account.write_login(root, gateway, "destination", issuer)}
        before = files(slots)
        gateway.tokens["Bearer " + json.loads(slot.path.read_bytes())["tokens"]["access_token"]] = "source"
        (root / "work/cedar-marker.txt").write_text("idle-source\n")
        original = []

        def exercise(pty):
            original.extend(observe(pty, root, gateway, slots, before, report))

        drive_pty([str(binary), "--no-daemon", "--no-alt-screen", terminal.SEED], root, env, "idle", exercise)
        gateway.release.set()
        fixture.cleanup_resources((("issuer", *listener),), (), report, None)
        listener = None
        fixture.require(not report.get("cleanupErrors") and gateway.slot.exchanges == 1 and gateway.requests == original,
                        "unexpected issuer cleanup, exchange or model request after idle refresh")
        fixture.require(terminal.CHECK["sha256"](binary) == report["binary_sha256"], "tested executable changed")
        report.update(passed=True, issuer_exchanges=1, extra_model_requests=0)
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        gateway.release.set()
        if listener:
            fixture.cleanup_resources((("issuer", *listener),), (), report, sys.exc_info()[1])
        fixture.write_json(root / "result.json", report)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", required=True, type=Path)
    parser.add_argument("--shared-auth-file", action="store_true", help="Use the fixture's default auth.json for vanilla.")
    parser.add_argument("--output", required=True, type=Path, help="Fresh private fixture directory.")
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), args.output.parent.resolve(strict=True) / args.output.name,
                 args.shared_auth_file))
