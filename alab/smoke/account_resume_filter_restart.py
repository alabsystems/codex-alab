# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Quit with narrowed local resume filters and inspect the next process's first listing."""

import argparse
import hashlib
from pathlib import Path
import shutil
import sys
import time

import account_resume as account
from account_resume_moved import histories
import capacity_exec as fixture
import model_resume as terminal
from picker_screen import SYNC_END
from terminal_controls import drive_pty


def picker(binary, root, env, label, directory):
    frames = []

    def interact(pty):
        deadline = time.monotonic() + 35

        def observe(folder, login, names, keys=None):
            offset = pty.raw.stat().st_size if keys else 0
            if keys:
                pty.send(keys)
            while time.monotonic() < deadline and pty.process.poll() is None:
                data = pty.raw.read_bytes() if pty.raw.exists() else b""
                rows = terminal.bare_picker_rows(data, after=offset, directory=folder, login=login, expected=names)
                if rows is not None:
                    frames.append({"directory": folder, "login": login, "rows": rows, "after": offset,
                                   "end": data.rfind(SYNC_END) + len(SYNC_END)})
                    return
                time.sleep(.05)
            raise RuntimeError(f"{label} did not display {folder}, {login}, {names}")

        both = ["birch", "cedar"]
        observe("All", "All local", both)
        if directory:
            observe("Cwd", "All local", ["birch"], b"\x1b[C")
            if directory == "Repo":
                observe("Repo", "All local", both, b"\x1b[C")
            observe(directory, "Current", ["birch"], b"\t\x1b[C")
        pty.quit()

    drive_pty([str(binary), "resume"], root, env, label, interact)
    return {"frames": frames, "raw_sha256": terminal.CHECK["sha256"](root / f"{label}.raw")}


def run(binary, root):
    root = root.parent.resolve(strict=True) / root.name
    root.mkdir(mode=0o700)
    digest = terminal.CHECK["sha256"]
    report = {"passed": False, "binary_sha256": digest(binary), "events": [], "requests": [], "pickers": {}}
    gateway = account.Gateway(report)
    try:
        server, worker = account.start_gateway(root, gateway)
        try:
            env, slots = account.configure_accounts(root, server.server_port, gateway)
            env.update(TERM="xterm-256color", TERM_PROGRAM="xterm")
            home, work, linked = root / "codex-home", root / "work", root / "linked"
            for label, args in (
                ("init", ["init", "--quiet"]),
                ("commit", ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit",
                            "--allow-empty", "-qm", "Fixture worktree"]),
                ("link", ["worktree", "add", "--quiet", "--detach", str(linked), "HEAD"]),
            ):
                fixture.require(fixture.run(Path("/usr/bin/git"), args, root, env, label) == 0,
                                f"could not prepare fixture repository: {label}")
            (linked / "cedar-marker.txt").write_text("linked worktree cedar\n")
            report["seeds"] = {}
            for login, cwd, prompt, reply in (
                ("source", linked, terminal.SEED, terminal.REPLY),
                ("destination", work, terminal.BIRCH_SEED, terminal.BIRCH_REPLY),
            ):
                shutil.copy2(slots[login], home / "auth.json")
                thread, turn = account.exec_turn(binary, ["exec", "--json", prompt], root, env, login + "-seed",
                                                reply, cwd=cwd)
                report["seeds"][login] = {"thread": thread, "turn": turn, "cwd": str(cwd)}
            before = histories(home)
            owners = {seed["thread"]: login for login, seed in report["seeds"].items()}
            fixture.require(len(owners) == len(before) == 2, "expected two distinct saved conversations")
            with gateway.lock:
                report["seed_request_count"] = len(gateway.requests)
                gateway.resumed = True

            # Each new PTY first observes both original rows before sending any keys.
            for label, directory in (("narrow-cwd", "Cwd"), ("narrow-repo", "Repo"), ("reopened", None)):
                report["pickers"][label] = picker(binary, root, env, label, directory)
            fixture.require(histories(home) == before, "browsing changed a saved conversation")
            report["unchanged_rollouts"] = {str(path.relative_to(home)): hashlib.sha256(data).hexdigest()
                                            for path, data in before.items()}
        finally:
            fixture.cleanup_resources((("gateway", server, worker),), (), report, sys.exc_info()[1])
        report["browse_model_requests"] = len(gateway.requests) - report["seed_request_count"]
        fixture.require(report["browse_model_requests"] == 0, "browsing started a model request")
        seed_turns = [row for row in gateway.requests if row["history"]]
        fixture.require({row["thread"] for row in seed_turns} == set(owners) and all(
            row["login"] == owners[row["thread"]] and row["account"] == "workspace-" + row["login"]
            and row["turn"] == report["seeds"][row["login"]]["turn"] for row in seed_turns),
            "seed requests did not match their saved threads and authenticated owners")
        reads = [row for row in gateway.reads if row["phase"] == "resume"]
        fixture.require(reads and all(row["login"] == "destination" and row["account"] == "workspace-destination"
                                      for row in reads), "picker metadata used the wrong account")
        fixture.require(not report.get("cleanupErrors") and digest(binary) == report["binary_sha256"],
                        "gateway cleanup or binary identity check failed")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
        print(error, file=sys.stderr)
    finally:
        fixture.write_json(root / "result.json", report)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), args.output.absolute()))
