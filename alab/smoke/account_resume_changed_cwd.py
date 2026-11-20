# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check the selected thread's current directory after its picker row was painted."""

import argparse
from contextlib import contextmanager, nullcontext
import json
from pathlib import Path
import sqlite3
import sys
import time

import account_resume as account
import capacity_exec as fixture
import model_resume as terminal
from quota_resume import Rpc
from terminal_controls import drive_pty


@contextmanager
def unavailable_metadata(binary, root, env, thread, rollout, report):
    # Withdraw only the fixture's selected rollout and its row after the source
    # writer exits. Restore both before B starts its actual continuation.
    held = root / "held-rollout"
    db_path = root / "codex-home/state_5.sqlite"
    with sqlite3.connect(db_path) as db:
        row = db.execute("SELECT * FROM threads WHERE id = ?", (thread,)).fetchone()
    fixture.require(row is not None, "source metadata was already absent")
    rollout.rename(held)
    try:
        with sqlite3.connect(db_path) as db:
            fixture.require(db.execute("DELETE FROM threads WHERE id = ?", (thread,)).rowcount == 1,
                            "could not withdraw the selected metadata")
        try:
            def confirm(process, output):
                rpc = Rpc(process, output)
                rpc.initialize()
                rpc.send({"jsonrpc": "2.0", "id": "absent", "method": "thread/read",
                          "params": {"threadId": thread, "includeTurns": False}})
                reply = rpc.wait(lambda msg: msg.get("id") == "absent")
                error = reply.get("error", {})
                fixture.require(error.get("code") == -32600 and thread in error.get("message", ""),
                                "metadata-only probe did not reject the missing selected UUID")
                report["missing_read"] = error
                process.stdin.close()
                process.stdin = None
                fixture.require(process.wait(timeout=15) == 0, "missing-metadata observer did not exit")
            fixture.require(fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env,
                                        "missing", interact=confirm) == 0, "missing metadata observer failed")
            yield
        finally:
            with sqlite3.connect(db_path) as db:
                db.execute("INSERT INTO threads VALUES (" + ", ".join("?" for _ in row) + ")", row)
    finally:
        held.rename(rollout)


def exercise(binary, root, port, gateway, report, explicit, missing):
    env, slots = account.configure_accounts(root, port, gateway)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm")
    old, latest, launch = root / "work", root / "updated-project", root / "picker-project"
    for project in (latest, launch):
        project.mkdir()
    with (root / "codex-home/config.toml").open("a") as config:
        for project in (latest, launch):
            config.write(f'\n[projects.{json.dumps(str(project))}]\ntrust_level = "trusted"\n')
    for project in (old, latest, launch):
        (project / "cedar-marker.txt").write_text(project.name + "\n")
    source = {**env, "CODEX_AUTH_PATH": str(slots["source"])}
    destination = {**env, "CODEX_AUTH_PATH": str(slots["destination"])}

    def seed(owner, output):
        rpc = Rpc(owner, output)
        rpc.initialize()
        thread = rpc.request("thread/start", {"cwd": str(old), "ephemeral": False})["thread"]["id"]
        started = rpc.turn(thread, terminal.SEED)
        fixture.require(started["status"] == "completed", "original turn did not complete")
        report["thread"] = thread
        rollout, = (root / "codex-home/sessions").rglob(f"rollout-*-{thread}.jsonl")

        def select(pty):
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline and pty.process.poll() is None:
                if pty.raw.exists() and terminal.bare_picker_rows(pty.raw.read_bytes(), expected=["cedar"]) == ["cedar"]:
                    break
                time.sleep(.05)
            else:
                raise RuntimeError("B did not see A's row before its directory changed")

            # A publishes the change through its actual live thread after B has drawn it.
            rpc.request("thread/settings/update", {"threadId": thread, "cwd": str(latest)})
            with sqlite3.connect(f"file:{root / 'codex-home/state_5.sqlite'}?mode=ro", uri=True) as db:
                while time.monotonic() < deadline:
                    saved = db.execute("SELECT cwd FROM threads WHERE id = ?", (thread,)).fetchone()
                    if saved == (str(latest),):
                        break
                    time.sleep(.05)
                else:
                    raise RuntimeError("A's updated directory was not durable")
            observed = rpc.request("thread/read", {"threadId": thread, "includeTurns": False})["thread"]
            report["owner_read_after_persistence"] = observed["cwd"]
            fixture.require(observed["id"] == thread, "owner read returned another thread")
            owner.stdin.close()
            owner.stdin = None
            fixture.require(owner.wait(timeout=15) == 0, "original writer did not shut down")
            with sqlite3.connect(f"file:{root / 'codex-home/state_5.sqlite'}?mode=ro", uri=True) as db:
                saved = db.execute("SELECT cwd FROM threads WHERE id = ?", (thread,)).fetchone()
            fixture.require(saved == (str(latest),), "changed directory was not durable before Enter")
            report["persisted_before_enter"] = saved[0]
            baseline = rollout.read_bytes()
            with gateway.lock:
                gateway.resumed = True
            hide = unavailable_metadata(binary, root, destination, thread, rollout, report) if missing else nullcontext()
            with hide:
                pty.send(b"\r")
                if not explicit:
                    pty.displayed("Use session directory")
                    # Record the actual session directory before continuing into it.
                    lines = pty.frame(232, lambda rows: next((row.strip() for row in rows
                                      if "Use session directory (" in row), None), "saved directory choice")
                    report["offered"] = lines["rendered"]
            if not explicit:
                pty.send(b"\r")
            pty.displayed(terminal.REPLY)
            pty.paste(terminal.CONTINUE, terminal.CONTINUE)
            pty.send(b"\r")
            pty.displayed(terminal.ANSWER)
            terminal.wait_for_turn_completion(root, gateway, pty.process, terminal.ANSWER, time.monotonic() + 25,
                                              rollout_path=rollout)
            pty.quit()
            fixture.require(rollout.read_bytes().startswith(baseline), "B overwrote earlier history")

        cli = [str(binary), *(["-C", str(launch)] if explicit else []), "resume"]
        drive_pty(cli, root, destination, "picker", select, cwd=launch)

    fixture.require(fixture.run(binary, ["app-server", "--listen", "stdio://"], root, source,
                                "original", cwd=old, interact=seed) == 0, "source app-server failed")
    rows = [row for row in report["requests"] if row["history"]]
    fixture.require(len(rows) == 4 and {r["thread"] for r in rows} == {report["thread"]}
                    and [r["login"] for r in rows] == ["source", "source", "destination", "destination"]
                    and all(r["account"] == "workspace-" + r["login"] for r in rows), "unexpected thread or account")
    expected = launch if explicit else old if missing else latest
    actual = rows[-1]["tool_records"][-1]["output"].rstrip("\n").splitlines()[-2:]
    report.update(expected_cwd=str(expected), actual_shell=actual)
    account.check_resumed_directory(expected, latest if explicit else launch, expected.name, rows[-2], rows[-1])
    if not missing:
        fixture.require(not (old / "cedar-resume-calls").exists(), "the resumed tool ran in the stale directory")
    if not explicit:
        fixture.require(str(expected) in report["offered"], "picker offered the wrong session directory")


def run(binary, root, explicit, missing):
    root.mkdir(mode=0o700)
    report = {"passed": False, "events": [], "requests": [], "explicit_cwd": explicit, "missing_read_control": missing,
              "binary_sha256": terminal.CHECK["sha256"](binary)}
    gateway = account.Gateway(report)
    gateway.verify_resume_directory = True
    try:
        server, worker = account.start_gateway(root, gateway)
        try:
            exercise(binary, root, server.server_port, gateway, report, explicit, missing)
        finally:
            fixture.cleanup_resources((("gateway", server, worker),), (), report, sys.exc_info()[1])
        fixture.require(not report.get("cleanupErrors"), "fixture cleanup failed")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        fixture.write_json(root / "result.json", report)
    print(report.get("error", "passed"))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    controls = parser.add_mutually_exclusive_group()
    controls.add_argument("--missing-metadata", action="store_true", help="Make only the selected lookup unavailable")
    controls.add_argument("--explicit-cwd", action="store_true", help="Control with an explicit -C")
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), args.output.resolve(), args.explicit_cwd, args.missing_metadata))
