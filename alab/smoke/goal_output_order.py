#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Observe goal set/clear output from a loaded thread and an unloaded thread over real stdio."""

import argparse
from contextlib import closing
import json
from pathlib import Path
import sqlite3
import sys
import time

import capacity_exec as fixture
import model_resume
from quota_resume import ResumeMock, Rpc


def persisted(root):
    databases = list((root / "codex-home").glob("goals_*.sqlite"))
    fixture.require(len(databases) == 1, f"expected one goals database, got {databases}")
    with closing(sqlite3.connect(databases[0].as_uri() + "?mode=ro", uri=True)) as database:
        return {thread: (objective, status) for thread, objective, status in
                database.execute("SELECT thread_id, objective, status FROM thread_goals")}


def send(rpc, requests, operation, thread, objective=None):
    rpc.request_id += 1
    request_id = rpc.request_id
    params = {"threadId": thread, "origin": "user"}
    if objective is not None:
        params.update(objective=objective, status="paused")
    requests[request_id] = (thread, operation, objective)
    rpc.send({"jsonrpc": "2.0", "id": request_id, "method": f"thread/goal/{operation}", "params": params})


def read_outputs(messages, requests, threads, *, refused=None):
    """Match exact issued IDs and goal payloads, preserving arrival order and multiplicity."""
    by_thread = {thread: [] for thread in threads}
    saw_refusal = False
    for message in messages:
        if refused is not None and message.get("id") == refused["id"]:
            fixture.require(message == refused and not saw_refusal, "changed or duplicated backlog refusal")
            saw_refusal = True
            continue
        request = requests.get(message.get("id"))
        if request:
            thread, operation, objective = request
            fixture.require("result" in message, f"goal {operation} failed: {message}")
            result = message["result"]
            if operation == "set":
                goal = result["goal"]
                fixture.require(goal["threadId"] == thread and goal["objective"] == objective
                                and goal["status"] == "paused", f"unexpected goal response: {goal}")
            else:
                fixture.require(result.get("cleared") is True, f"clear did not commit: {result}")
            by_thread[thread].append(("response", operation, objective))
        elif message.get("method") in ("thread/goal/updated", "thread/goal/cleared"):
            params = message["params"]
            thread = params["threadId"]
            fixture.require(thread in by_thread, f"goal notification for an unexpected thread: {thread}")
            if "goal" in params:
                goal = params["goal"]
                fixture.require(goal["threadId"] == thread and goal["status"] == "paused"
                                and params.get("turnId") is None, f"unexpected goal update: {params}")
                by_thread[thread].append(("notification", "set", goal["objective"]))
            else:
                by_thread[thread].append(("notification", "clear", None))
    fixture.require(saw_refusal == (refused is not None), "missing backlog refusal")
    for thread, sequence in by_thread.items():
        issued = [(operation, objective) for owner, operation, objective in requests.values() if owner == thread]
        responses = [event[1:] for event in sequence if event[0] == "response"]
        fixture.require(len(responses) == len(issued) and set(responses) == set(issued),
                        f"lost or duplicate responses for {thread}: {sequence}")
        expected = [(kind, *operation) for operation in responses for kind in ("response", "notification")]
        fixture.require(sequence == expected, f"goal output order for {thread}: {sequence}")
    return by_thread


def wait_for_batch(rpc, batch, threads):
    """Consume raw frames in arrival order until every requested reply and notification arrives."""
    messages = []
    deadline = time.monotonic() + 40
    while len(messages) < 2 * len(batch):
        message = rpc.wait(lambda value: value.get("id") in batch or (
            value.get("method") in ("thread/goal/updated", "thread/goal/cleared")
            and value["params"]["threadId"] in threads), timeout=max(0, deadline - time.monotonic()))
        messages.append(message)
        fixture.require("error" not in message, f"goal RPC failed: {message}")
    return read_outputs(messages, batch, threads)


def exercise_global_pressure(rpc, root, report, requests):
    # One loaded root and 128 unloaded forks exercise the actual 128-slot
    # queue without holding a live runtime or a provider turn per fork.
    threads = report["threads"]
    parent = rpc.request("thread/start", {"cwd": str(root / "work")})["thread"]["id"]
    threads["parent"] = parent
    fixture.require(rpc.turn(parent, "Materialize the goal pressure fixture.")["status"] == "completed",
                    "seed turn did not complete")
    for index in range(128):
        thread = rpc.request("thread/fork", {"threadId": parent})["thread"]["id"]
        threads[str(index)] = thread
        fixture.require(rpc.request("thread/unsubscribe", {"threadId": thread})["status"] == "unsubscribed",
                        "did not release fork subscription")
        rpc.wait(lambda m: m.get("method") == "thread/closed" and m["params"]["threadId"] == thread)
    fixture.require(len(set(threads.values())) == 129 and
                    rpc.request("thread/loaded/list", {})["data"] == [parent], "incorrect saved thread set")
    fixture.require(not persisted(root), "fixture already has goals")

    database, = (root / "codex-home").glob("goals_*.sqlite")
    with closing(sqlite3.connect(database, isolation_level=None)) as writer:
        writer.execute("BEGIN IMMEDIATE")
        for name, thread in threads.items():
            send(rpc, requests, "set", thread, "Paused goal " + name)
        refusal = rpc.wait(lambda m: m.get("id") in requests)
        refused_thread = requests[refusal["id"]][0]
        fixture.require(refusal.get("error") == {
            "code": -32603,
            "message": f"goal output backlog is full for {refused_thread}; retry after the client drains",
        }, f"expected a global queue refusal before releasing SQLite: {refusal}")
        report["refused"] = refusal
        del requests[refusal["id"]]
        writer.execute("ROLLBACK")

    wait_for_batch(rpc, requests, threads.values())
    expected = {thread: (objective, "paused") for thread, _, objective in requests.values()}
    fixture.require(persisted(root) == expected, "database disagrees with the admitted set requests")
    for operation, objective in (("set", "Recovered after pressure"), ("clear", None)):
        retry = {}
        send(rpc, retry, operation, refused_thread, objective)
        wait_for_batch(rpc, retry, [refused_thread])
        requests.update(retry)
    fixture.require(persisted(root) == expected, "recovered set and clear changed another goal")


def run(binary, root, *, overlap_threads=False, global_pressure=False):
    root.mkdir(mode=0o700)
    env = fixture.fresh_environment(root)
    env.update(HTTP_PROXY="http://127.0.0.1:1", HTTPS_PROXY="http://127.0.0.1:1",
               NO_PROXY="localhost,127.0.0.1,::1")
    report = {"passed": False, "overlap_threads": overlap_threads,
              "binary_sha256": model_resume.CHECK["sha256"](binary), "threads": {}}
    requests = {}
    server = None
    mock = ResumeMock(False)
    try:
        server = fixture.serve(mock)
        fixture.configure(root, server[0].server_port)
        config = root / "codex-home/config.toml"
        config.write_text("features.goals = true\nfeatures.daemon_auto_start = false\n"
                          "analytics.enabled = false\nthread_unload_delay_secs = 0\n" + config.read_text())

        def exercise(process, record):
            rpc = Rpc(process, record)
            rpc.initialize()
            if global_pressure:
                return exercise_global_pressure(rpc, root, report, requests)
            threads = report["threads"]
            for label in ("loaded", "unloaded"):
                thread = rpc.request("thread/start", {"cwd": str(root / "work")})["thread"]["id"]
                threads[label] = thread
                fixture.require(rpc.turn(thread, f"Materialize the {label} thread.")["status"] == "completed",
                                f"{label} thread did not finish its seed turn")
            fixture.require(len(set(threads.values())) == 2, "seed threads must be distinct")
            stopped = threads["unloaded"]
            fixture.require(rpc.request("thread/unsubscribe", {"threadId": stopped})["status"] == "unsubscribed",
                            "fixture did not unsubscribe its loaded thread")
            rpc.wait(lambda value: value.get("method") == "thread/closed" and value["params"]["threadId"] == stopped)
            fixture.require(rpc.request("thread/loaded/list", {})["data"] == [threads["loaded"]],
                            "unloaded thread is still listed as loaded")
            fixture.require(persisted(root) == {}, "fresh goals database already contains goals")

            groups = [list(threads.items())] if overlap_threads else [[entry] for entry in threads.items()]
            expected = {}
            for group in groups:
                sets = {}
                for index in range(2):
                    for label, thread in group:
                        send(rpc, sets, "set", thread, f"Pause {label} objective {index}")
                requests.update(sets)
                ordered = wait_for_batch(rpc, sets, [thread for _, thread in group])
                expected.update((thread, (sequence[-1][2], "paused")) for thread, sequence in ordered.items())
            after_set = persisted(root)
            fixture.require(after_set == expected, f"last observed update differs from SQLite: {after_set}")
            report["after_sets"] = after_set

            for group in groups:
                clears = {}
                for _, thread in group:
                    send(rpc, clears, "clear", thread)
                requests.update(clears)
                wait_for_batch(rpc, clears, [thread for _, thread in group])
            report["after_clears"] = persisted(root)
            fixture.require(not report["after_clears"], "goal clear left a saved goal")
            fixture.require(rpc.request("thread/loaded/list", {})["data"] == [threads["loaded"]],
                            "goal mutations loaded the cold thread")

        args = ["app-server", "--listen", "stdio://"]
        executable = binary
        if global_pressure:
            # Only the measured CLI inherits this limit, including from a release
            # runner that raises its own file limit for compilation.
            executable = Path(sys.executable)
            cap_files = ("import os,resource,sys; "
                         "limit=resource.RLIMIT_NOFILE; "
                         "resource.setrlimit(limit,(256,resource.getrlimit(limit)[1])); "
                         "os.execv(sys.argv[1],sys.argv[1:])")
            args = ["-I", "-c", cap_files, str(binary), *args]
            report["cli_open_file_limit"] = 256
        status = fixture.run(executable, args, root, env, "goals", interact=exercise)
        fixture.require(status == 0, f"app-server exited {status}")
        # Include queued frames and everything emitted through EOF to detect delayed duplicate delivery.
        with (root / "goals-stdout.jsonl").open() as wire:
            report["outputs"] = read_outputs((json.loads(line) for line in wire if line.strip()),
                                              requests, report["threads"].values(), refused=report.get("refused"))
        fixture.require(len(mock.seed_requests) == (1 if global_pressure else 2), "a paused or cleared goal submitted another model turn")
        fixture.require(model_resume.CHECK["sha256"](binary) == report["binary_sha256"], "CLI changed during run")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        report["mutations"] = [{"id": request_id, "thread": owner, "operation": op, "objective": objective}
                               for request_id, (owner, op, objective) in requests.items()]
        fixture.cleanup_resources((("model", *server),) if server else (), (), report, sys.exc_info()[1])
        fixture.write_json(root / "result.json", report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="Fresh directory for the wire trace and SQLite.")
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--global-pressure", action="store_true",
                       help="Exercise global goal backpressure and recovery at a 256-file limit on Unix.")
    modes.add_argument("--overlap-threads", action="store_true",
                        help="Issue mutations on both threads before reading either thread’s output.")
    args = parser.parse_args()
    result = run(args.codex.resolve(strict=True), args.output.resolve(),
                 overlap_threads=args.overlap_threads, global_pressure=args.global_pressure)
    print(result.get("error", "passed" if result["passed"] else "failed"))
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
