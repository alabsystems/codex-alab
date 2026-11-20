# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Resume an exec thread by exact title and reject a collision with an editor thread."""

import argparse
from contextlib import closing
import hashlib
import json
from pathlib import Path
import re
import shutil
import sqlite3
import sys
import time

import account_resume as account
import account_resume_named_page as page
from account_resume_moved import histories
from account_resume_restored import index_state
from account_resume_titles import TITLE, name_thread
import capacity_exec as fixture
import model_resume as terminal
from quota_resume import Rpc
from terminal_controls import read_runs, run_pty


def snapshot(files, home):
    return {str(path.relative_to(home)): hashlib.sha256(data).hexdigest() for path, data in files.items()}


def editor_seed(binary, root, env, cwd):
    saved = {}

    def seed(process, stdout):
        rpc = Rpc(process, stdout)
        rpc.initialize()
        thread = rpc.request("thread/start", {"cwd": str(cwd), "ephemeral": False})["thread"]
        fixture.require(thread["source"] == "vscode" and thread["cwd"] == str(cwd), "unexpected editor thread")
        turn = rpc.turn(thread["id"], terminal.BIRCH_SEED)
        fixture.require(turn["status"] == "completed", "editor thread did not complete")
        saved.update(thread=thread["id"], turn=turn["id"])

    status = fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, "editor", cwd=cwd, interact=seed)
    fixture.require(status == 0, "editor seed did not exit cleanly")
    return saved["thread"], saved["turn"]


def check_collision(case, title, ids):
    custody = case["custody"]
    fixture.require(case["guardian_status"] == 1 and type(custody.get("exit")) is int and custody["exit"] == 1
                    and custody.get("command") == case["argv"] and custody.get("forced") is False
                    and custody.get("owning_reap") is True and custody.get("group_absent_after_reap") is True
                    and not case["observer_quit"], "ambiguous command did not fail and exit on its own")
    fixture.require(len(set(ids)) == 2 and case["argv"][1:] == ["resume", title, terminal.BIRCH_CONTINUE],
                    "expected two distinct sessions and a real positional prompt")
    # The terminal or its error reporter may wrap the diagnostic across several physical lines.
    text = "".join(case["terminal_text"].split())
    messages = [f"Multiple sessions match '{title}' (including {first} and {second});"
                " use a session UUID to disambiguate." for first, second in (ids, ids[::-1])]
    fixture.require(any("".join(message.split()) in text for message in messages),
                    "missing ambiguity diagnostic with both original UUIDs")
    fixture.require(not case["requests"], "ambiguous title dispatched a model request")
    fixture.require(case["before"] == case["after"], "ambiguous title changed the saved rollouts")


def named_rows(database):
    with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as db:
        rows = db.execute("SELECT id, source, cwd, rollout_path, creator_user_id, name FROM threads")
        return {thread: list(values) for thread, *values in rows}


def remove_source_row(home, env, paths, a, b):
    database = home / "state_5.sqlite"
    fixture.require(Path(env["CODEX_HOME"]).resolve(strict=True) == home and database.resolve(strict=True) == database,
                    "restore preparation requires the fixture's own SQLite database")
    original = named_rows(database)
    fixture.require(set(original) == {a, b} and all(original[thread][-1] == TITLE and
                    Path(original[thread][2]).resolve(strict=True) == paths[thread] for thread in (a, b)),
                    "expected both canonical named rollout paths before restore")
    saved = {r["id"]: r["thread_name"] for r in
             (json.loads(line) for line in (home / "session_index.jsonl").read_text().splitlines())}
    fixture.require(saved.get(a) == TITLE, "A's real title must survive outside SQLite")
    files = snapshot(histories(home), home)
    index_state(database)
    with closing(sqlite3.connect(database.as_uri() + "?mode=rw", uri=True)) as db:
        db.execute("PRAGMA foreign_keys = ON")
        with db:
            fixture.require(db.execute("DELETE FROM threads WHERE id = ?", (a,)).rowcount == 1,
                            "expected to remove only A's disposable thread row")
    remaining = named_rows(database)
    fixture.require(remaining == {b: original[b]} and snapshot(histories(home), home) == files,
                    "removing A's index row modified B or the saved rollouts")
    return {"original": original, "omitted": remaining, "before_lookup": index_state(database),
            "original_rollouts": files}


def exercise(binary, root, port, gateway, report, *, page_boundary=False):
    env, slots = account.configure_accounts(root, port, gateway)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm")
    home, work, other = root / "codex-home", root / "work", root / "other-project"
    other.mkdir()
    with (home / "config.toml").open("a") as config:
        config.write(f'\n[projects.{json.dumps(str(other))}]\ntrust_level = "trusted"\n')
        config.write('\n[tui]\nresume_cwd = "session"\n')
    marker = "original named exec workspace"
    (work / "cedar-marker.txt").write_text(marker + "\n")
    (other / "cedar-marker.txt").write_text("different named editor workspace\n")
    shutil.copy2(slots["source"], home / "auth.json")
    a, seed_turn = account.exec_turn(binary, ["exec", "--json", "--skip-git-repo-check", terminal.SEED], root,
                                     env, "exec", terminal.REPLY, cwd=work)
    name_thread(binary, root, env, "source", a, cwd=work)
    shutil.copy2(slots["destination"], home / "auth.json")
    b, editor_turn = editor_seed(binary, root, env, other)
    report.update(source=a, destination=b, seed_turn=seed_turn, editor_turn=editor_turn)
    originals = histories(home)
    paths = {}
    for thread, owner, origin, cwd in ((a, "source", "exec", work), (b, "destination", "vscode", other)):
        path, = (p for p in originals if p.name.endswith(f"-{thread}.jsonl"))
        header = json.loads(originals[path].splitlines()[0])["payload"]
        expected = {"id": thread, "cwd": str(cwd), "source": origin, "creator_user_id": "user-" + owner}
        fixture.require(all(header.get(k) == v for k, v in expected.items()), "unexpected seed rollout identity")
        paths[thread] = path
    fixture.require(a != b and len(originals) == 2, "need exactly two different original threads")
    report["seed_request_count"] = len(gateway.requests)
    seed, seed_tool, birch = [r for r in gateway.requests if r["history"]]
    fixture.require([(r["login"], r["thread"], r["turn"]) for r in (seed, seed_tool, birch)] ==
                    [("source", a, seed_turn), ("source", a, seed_turn), ("destination", b, editor_turn)],
                    "seed requests used unexpected login or thread")
    page_ids = page.prepare(binary, root, env, a, b, other) if page_boundary else None
    gateway.verify_resume_directory = True
    gateway.resumed = True

    def run_case(label):
        collision = label != "unique"
        case = report[label] = {"passed": False, "observer_quit": False}
        if label == "collision":
            name_thread(binary, root, env, "destination", b, cwd=other)
        if label == "restored-collision":
            case["index"] = remove_source_row(home, env, paths, a, b)
        if page_ids is not None:
            case["pages"] = page.observe_pages(binary, root, env, label, page_ids, a, b, other)
        before = histories(home)
        if page_ids is not None:
            saved_ids = {json.loads(data.split(b"\n", 1)[0])["payload"]["id"] for data in before.values()}
            fixture.require(saved_ids == page_ids and len(before) == len(page_ids),
                            "expected one original saved rollout per listed ID")
        first = len(gateway.requests)
        cli = [str(binary), "resume", TITLE]
        if collision:
            cli.append(terminal.BIRCH_CONTINUE)
        case.update(argv=cli, cwd=str(other), before=snapshot(before, home))

        def observe(pty):
            deadline = time.monotonic() + 32
            while pty.process.poll() is None and time.monotonic() < deadline:
                if not collision and b"".join(terminal.REPLY.encode().split()) in read_runs(pty.raw, 0):
                    if page_boundary:
                        case["original_answer_displayed"] = True
                        break  # Open and quit the page-two original without submitting a turn.
                    pty.paste(terminal.CONTINUE, terminal.CONTINUE)
                    pty.send(b"\r")
                    pty.displayed(terminal.ANSWER)
                    case["completed_turn"] = terminal.wait_for_turn_completion(
                        root, gateway, pty.process, terminal.ANSWER, deadline, rollout_path=paths[a])
                    break
                if collision:
                    # An incorrect selection can be an interactive successful process. Capture its real turn
                    # before quitting it. No key is sent to a correctly rejected ambiguity.
                    with gateway.lock:
                        calls = [r for r in gateway.requests[first:] if r["history"]]
                    if calls:
                        break
                time.sleep(.04)
            if pty.process.poll() is None:
                case["observer_quit"] = True
                pty.quit()

        try:
            status, custody = run_pty(cli, root, env, label, observe, cwd=other)
        finally:
            case["requests"] = gateway.requests[first:].copy()
            after = histories(home)
            case["after"] = snapshot(after, home)
            raw = (root / (label + ".raw")).read_bytes()
            case["raw_sha256"] = hashlib.sha256(raw).hexdigest()
            case["terminal_text"] = re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", raw).decode(errors="replace")
        case.update(guardian_status=status, custody=custody)
        if label == "restored-collision":
            database = home / "state_5.sqlite"
            case["index"]["after_lookup"] = index_state(database)
            case["index"]["recovered"] = named_rows(database)
        if collision:
            check_collision(case, TITLE, [a, b])
            if label == "restored-collision":
                fixture.require(case["index"]["original"] == case["index"]["recovered"],
                                "the title lookup did not restore A's original indexed identity and preserve B's")
        else:
            fixture.require(status == 0, "unique title did not exit successfully")
            terminal.CHECK["check_model_terminal_exit"](custody, cli)
            if page_boundary:
                fixture.require(case.get("original_answer_displayed")
                                and re.search(rf"(?m)^\s*codex resume {re.escape(a)}\s*$", case["terminal_text"])
                                and not case["requests"] and case["after"] == case["before"],
                                "read-only page-two lookup did not preserve the original UUID and every history")
                case["passed"] = True
                return
            fixture.require(len(case["requests"]) == 2, "expected B to send the resume and directory-check POSTs")
            resumed, followup = case["requests"]
            turn = case.get("completed_turn")
            fixture.require(turn and turn not in (seed_turn, editor_turn) and
                            all((r["login"], r["account"], r["thread"], r["turn"]) ==
                                ("destination", "workspace-destination", a, turn) for r in (resumed, followup)),
                            "unique title did not continue A's original thread using B")
            fixture.require(resumed["history"] == [*seed_tool["history"], terminal.REPLY, terminal.CONTINUE]
                            and resumed["tool_records"] == seed_tool["tool_records"], "lost original history or tools")
            case["shell_cwd"] = account.check_resumed_directory(work, other, marker, resumed, followup)
            fixture.require(set(before) == set(after) and after[paths[b]] == before[paths[b]]
                            and after[paths[a]].startswith(before[paths[a]]) and after[paths[a]] != before[paths[a]],
                            "unique selection did not append solely to the original exec rollout")
        case["passed"] = True

    labels = ("unique", "collision") if page_boundary else ("unique", "collision", "restored-collision")
    for label in labels:
        try:
            run_case(label)
        except (RuntimeError, ValueError, AssertionError) as error:
            report[label]["error"] = str(error)
    fixture.require(all(report[label]["passed"] for label in labels), "exact-title controls failed")
    return labels


def run(binary, root, *, page_boundary=False):
    root = root.resolve()
    root.mkdir(mode=0o700)
    binary = binary.resolve(strict=True)
    digest = terminal.CHECK["sha256"]
    report = {"passed": False, "binary": str(binary), "binary_sha256": digest(binary), "events": [], "requests": []}
    gateway = account.Gateway(report)
    try:
        server, worker = account.start_gateway(root, gateway)
        try:
            labels = exercise(binary, root, server.server_port, gateway, report, page_boundary=page_boundary)
        finally:
            fixture.cleanup_resources((("gateway", server, worker),), (), report, sys.exc_info()[1])
        observed = report["requests"][report["seed_request_count"]:]
        expected = [row for label in labels for row in report[label]["requests"]]
        fixture.require(observed == expected, "unattributed model request after a terminal exited")
        fixture.require(not report.get("cleanupErrors") and digest(binary) == report["binary_sha256"],
                        "gateway cleanup or binary identity check failed")
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
    parser.add_argument("--page-boundary", action="store_true",
                        help="Resolve exact titles across the 100-row startup search boundary.")
    args = parser.parse_args()
    raise SystemExit(run(args.codex, args.output, page_boundary=args.page_boundary))
