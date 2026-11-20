#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Disconnect the goal RPC caller after observing its SQLite commit during a held checkpoint."""

import argparse
from contextlib import closing
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time

import capacity_exec as fixture
from goal_output_order import persisted, send
import model_resume
from quota_resume import ResumeMock, Rpc


class SocketRpc(Rpc):
    def __init__(self, socket, wire, label):
        super().__init__(None, wire)
        self.socket = socket
        self.label = label

    def record(self, direction, message):
        self.output.write(json.dumps([self.label, direction, message]) + "\n")
        self.output.flush()

    def send(self, value):
        self.record("send", value)
        self.socket.send(json.dumps(value))

    def poll(self, matches, timeout):
        deadline = time.monotonic() + timeout
        while True:
            for index, value in enumerate(self.pending):
                if matches(value):
                    return self.pending.pop(index)
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return None
            try:
                value = json.loads(self.socket.recv(timeout=remaining))
            except TimeoutError:
                return None
            self.record("receive", value)
            self.pending.append(value)


def connect(path):
    from websockets.sync.client import unix_connect

    deadline = time.monotonic() + 15
    while True:
        try:
            return unix_connect(str(path), "ws://localhost/rpc", compression=None, open_timeout=2, close_timeout=1)
        except (FileNotFoundError, ConnectionRefusedError):
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.02)


def verify_delivery(messages, thread, set_id, clear_id):
    observed = []
    for peer, direction, value in messages:
        if direction != "receive":
            continue
        fixture.require(value.get("id") != set_id, f"abandoned goal response reached {peer}")
        if peer == "surviving":
            if value.get("id") == clear_id:
                observed.append("clear response")
            if value.get("method") in ("thread/goal/updated", "thread/goal/cleared"):
                fixture.require(value["params"]["threadId"] == thread, "goal delivered to the wrong thread")
                observed.append(value["method"])
    fixture.require(observed == ["thread/goal/updated", "clear response", "thread/goal/cleared"],
                    f"incorrect survivor delivery: {observed}")
    return observed


def exercise(root, socket_path, report):
    with (root / "clients.jsonl").open("x") as wire, connect(socket_path) as subscriber:
        surviving = SocketRpc(subscriber, wire, "surviving")
        surviving.initialize()
        thread = surviving.request("thread/start", {"cwd": str(root / "work")})["thread"]["id"]
        report["thread"] = thread
        fixture.require(surviving.turn(thread, "Materialize the goal thread.")["status"] == "completed",
                        "seed turn did not complete")
        fixture.require(persisted(root) == {}, "fresh goals database already contained a row")
        database, = (root / "codex-home").glob("goals_*.sqlite")
        objective = "Pause the goal whose caller disconnects"
        request = {}
        with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as held:
            fixture.require(held.execute("PRAGMA journal_mode").fetchone() == ("wal",), "expected WAL database")
            held.execute("BEGIN")
            fixture.require(held.execute("SELECT count(*) FROM thread_goals").fetchone() == (0,),
                            "reader did not pin the original empty snapshot")
            with connect(socket_path) as departed:
                caller = SocketRpc(departed, wire, "departing")
                caller.request_id = 1000  # Distinct IDs expose accidental cross-connection replies.
                caller.initialize()
                send(caller, request, "set", thread, objective)
                request_id, = request
                deadline = time.monotonic() + 3
                while persisted(root).get(thread) != (objective, "paused"):
                    fixture.require(time.monotonic() < deadline, "did not observe commit before checkpoint timeout")
                    time.sleep(0.01)
                fixture.require(held.execute("SELECT count(*) FROM thread_goals").fetchone() == (0,),
                                "pinned snapshot advanced")
                fixture.require(caller.poll(lambda msg: msg.get("id") == request_id, 0.03) is None,
                                "request completed before observer disconnect")
                report["visible_before_disconnect"] = {thread: [objective, "paused"]}
            fixture.require(departed.socket.fileno() == -1, "requesting socket remained connected")
            report["disconnect_close_code"] = departed.close_code
            fixture.require(surviving.poll(lambda msg: msg.get("method") == "thread/goal/updated", 0.05) is None,
                            "survivor received the update before the old reader released")
            held.rollback()
        update = surviving.wait(lambda msg: msg.get("method") == "thread/goal/updated"
                                and msg["params"]["threadId"] == thread)
        goal = update["params"]["goal"]
        fixture.require((goal["threadId"], goal["objective"], goal["status"]) == (thread, objective, "paused")
                        and update["params"].get("turnId") is None, f"wrong survivor notification: {update}")
        fixture.require(surviving.request("thread/goal/get", {"threadId": thread})["goal"] == goal,
                        "surviving goal/get disagrees with notification")
        clear = surviving.request("thread/goal/clear", {"threadId": thread, "origin": "user"})
        clear_id = surviving.request_id
        fixture.require(clear.get("cleared") is True, "surviving client could not clear the goal")
        surviving.wait(lambda msg: msg.get("method") == "thread/goal/cleared" and msg["params"]["threadId"] == thread)
        fixture.require(persisted(root) == {}, "surviving clear left a saved goal")
        fixture.require(surviving.request("thread/goal/get", {"threadId": thread})["goal"] is None,
                        "public goal/get disagrees with SQLite after clear")
        report.update(goal=goal, cleared_by_survivor=True, departing_request_id=request_id)
    with (root / "clients.jsonl").open() as capture:
        report["survivor_delivery"] = verify_delivery(map(json.loads, capture), thread, request_id, clear_id)


def serve_until_input_closes(argv):
    """Keep the exact CLI under the fixture guardian; its parent owns orderly socket-server shutdown."""
    with subprocess.Popen(argv, stdin=subprocess.DEVNULL) as server:
        try:
            sys.stdin.buffer.read(1)
            if server.poll() is not None:
                raise RuntimeError(f"server exited before requested shutdown: {server.returncode}")
        finally:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
                raise
    return server.returncode


def run(binary, root):
    root.mkdir(mode=0o700)
    env = fixture.fresh_environment(root)
    env.update(HTTP_PROXY="http://127.0.0.1:1", HTTPS_PROXY="http://127.0.0.1:1",
               NO_PROXY="localhost,127.0.0.1,::1")
    report = {"passed": False, "binary_sha256": model_resume.CHECK["sha256"](binary)}
    mock = ResumeMock(False)
    server = None
    try:
        server = fixture.serve(mock)
        fixture.configure(root, server[0].server_port)
        config = root / "codex-home/config.toml"
        config.write_text("features.goals = true\nfeatures.daemon_auto_start = false\nanalytics.enabled = false\n"
                          + config.read_text())
        with tempfile.TemporaryDirectory(prefix="goal-rpc-", dir="/tmp") as socket_dir:
            socket_path = Path(socket_dir) / "server.sock"
            argv = [str(Path(__file__).resolve()), "--serve", str(binary), "app-server", "--listen",
                    f"unix://{socket_path}"]
            status = fixture.run(sys.executable, argv, root, env, "server",
                                 interact=lambda _child, _stdout: exercise(root, socket_path, report))
            fixture.require(status == 0, f"socket server exited {status}")
        fixture.require(len(mock.seed_requests) == 1, "goal mutation dispatched an additional model turn")
        fixture.require(model_resume.CHECK["sha256"](binary) == report["binary_sha256"], "CLI changed during run")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        fixture.cleanup_resources((("model", *server),) if server else (), (), report, sys.exc_info()[1])
        fixture.write_json(root / "result.json", report)
    return report


if __name__ == "__main__":
    if sys.argv[1:2] == ["--serve"]:
        sys.exit(serve_until_input_closes(sys.argv[2:]))
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = run(args.codex.resolve(strict=True), args.output.resolve())
    print(result.get("error", "passed" if result["passed"] else "failed"))
    sys.exit(0 if result["passed"] else 1)
