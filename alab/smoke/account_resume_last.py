# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check which login and project the local terminal automatically resumes with --last."""

import argparse
import json
from pathlib import Path
import shutil
import sys
import time

import account_resume as account
from account_resume_moved import histories
import capacity_exec as fixture
import model_resume as terminal
from quota_resume import Rpc
from terminal_controls import drive_pty


def exercise(binary, root, port, gateway, report):
    env, slots = account.configure_accounts(root, port, gateway)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm")
    home, work, other = root / "codex-home", root / "work", root / "other-project"
    other.mkdir()
    config = home / "config.toml"
    with config.open("a") as stream:
        stream.write(f'\n[projects.{json.dumps(str(other))}]\ntrust_level = "trusted"\n')
        stream.write('\n[tui]\nresume_cwd = "session"\n')
    (work / "cedar-marker.txt").write_text("original cedar workspace\n")

    def turn(label, login, cwd, args, answer, event):
        shutil.copy2(slots[login], home / "auth.json")
        before = len(gateway.requests)

        def finish(pty):
            pty.displayed(answer)
            terminal.wait_for_turn_completion(root, gateway, pty.process, answer,
                                              time.monotonic() + 30, event_kind=event)
            pty.quit()

        drive_pty([str(binary), *args], root, env, label, finish, cwd=cwd)
        calls = gateway.requests[before:]
        fixture.require(calls and all(row["login"] == login and row["account"] == "workspace-" + login
                                      for row in calls), f"{label} sent a request with the wrong login")
        ids = {(row["thread"], row["turn"]) for row in calls if row["history"]}
        fixture.require(len(ids) == 1, f"{label} did not use a single thread and turn")
        thread, turn_id = ids.pop()
        fixture.require(thread and turn_id, f"{label} is missing its thread or turn ID")
        report["turns"][label] = {"thread": thread, "turn": turn_id}
        return thread

    b_local = turn("b-local", "destination", work, [terminal.SEED], terminal.REPLY, "request")
    # The public listing exposes timestamps in whole seconds.
    time.sleep(1.1)
    b_other = turn("b-other", "destination", other, [terminal.BIRCH_SEED], terminal.BIRCH_REPLY, "workspace_request")
    time.sleep(1.1)
    a_local = turn("a-local", "source", work, [terminal.BIRCH_SEED], terminal.BIRCH_REPLY, "workspace_request")
    fixture.require(len({a_local, b_other, b_local}) == 3, "expected three distinct original conversations")
    shutil.copy2(slots["destination"], home / "auth.json")

    def list_threads(process, output):
        rpc = Rpc(process, output)
        rpc.initialize()
        params = {"sortKey": "updated_at", "archived": False, "sourceKinds": ["cli"], "modelProviders": [],
                  "useStateDbOnly": True}
        for scope, current, expected in (("allLogins", False, [a_local, b_other, b_local]),
                                         ("activeLogin", True, [b_other, b_local])):
            rows = rpc.request("thread/list", {**params, "currentAccountOnly": current})["data"]
            report[scope] = [row["id"] for row in rows]
            if not current:
                report["initialThreads"] = [{key: row[key] for key in ("id", "cwd", "updatedAt")} for row in rows]
                fixture.require([row["cwd"] for row in rows] == [str(work), str(other), str(work)],
                                "the conversations were not indexed in their original projects")
            fixture.require(report[scope] == expected, f"{scope} has the wrong initial order")
            fixture.require(all(a["updatedAt"] > b["updatedAt"] for a, b in zip(rows, rows[1:])),
                            "the setup did not establish a strict recency order")

    fixture.require(fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, "ordering",
                                interact=list_threads, cwd=work) == 0, "could not verify the initial listing")
    with gateway.lock:
        report["firstResumeRequest"] = len(gateway.requests)
        gateway.resumed = True
    for label, options, prompt, answer, event, expected in (
        ("all-projects", ["--last", "--all"], terminal.BIRCH_CONTINUE, terminal.BIRCH_ANSWER, "workspace_request", b_other),
        ("current-project", ["--last"], terminal.CONTINUE, terminal.ANSWER, "request", b_local),
    ):
        before = histories(home)
        selected = turn(label, "destination", work, ["resume", *options, prompt], answer, event)
        fixture.require(selected == expected, f"{label} resumed {selected}, expected {expected}")
        after = histories(home)
        chosen, = (path for path in before if path.name.endswith(f"-{expected}.jsonl"))
        fixture.require(before.keys() == after.keys() and after[chosen].startswith(before[chosen])
                        and len(after[chosen]) > len(before[chosen])
                        and all(after[path] == data for path, data in before.items() if path != chosen),
                        f"{label} did not append solely to its selected conversation")
    fixture.require(len({row["turn"] for row in report["turns"].values()}) == 5, "expected five distinct turns")


def check_completed_requests(report):
    requests = report["requests"][report["firstResumeRequest"]:]
    reads = [row for row in report["account_reads"] if row["phase"] == "resume"]
    fixture.require(reads and all(row["login"] == "destination" and row["account"] == "workspace-destination"
                                  for row in [*requests, *reads]), "a resumed request used the wrong login or workspace")
    observed = [(row["thread"], row["turn"]) for row in requests if row["history"]]
    expected = [report["turns"][label] for label in ("all-projects", "current-project")]
    fixture.require(observed == [(row["thread"], row["turn"]) for row in expected],
                    "expected one resumed inference on each selected conversation")


def run(binary, root):
    root.mkdir(mode=0o700)
    digest = terminal.CHECK["sha256"]
    report = {"passed": False, "binary_sha256": digest(binary), "events": [], "requests": [], "turns": {}}
    gateway = account.Gateway(report)
    try:
        server, worker = account.start_gateway(root, gateway)
        try:
            exercise(binary, root, server.server_port, gateway, report)
        finally:
            fixture.cleanup_resources((("gateway", server, worker),), (), report, sys.exc_info()[1])
        check_completed_requests(report)
        fixture.require(not report.get("cleanupErrors") and digest(binary) == report["binary_sha256"],
                        "the gateway did not shut down cleanly or the executable changed")
        report["passed"] = True
    except Exception as error:
        report["error"] = str(error)
        print(error, file=sys.stderr)
    finally:
        fixture.write_json(root / "result.json", report)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), args.output.resolve()))
