# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Browse and continue a shared API-key or pre-enforcement conversation under another login."""

import argparse
from contextlib import closing
from functools import partial
import hashlib
import json
from pathlib import Path
import secrets
import shutil
import sqlite3
import sys
import time

import account_resume as account
from account_resume_moved import histories
from account_resume_titles import record_frame
import capacity_exec as fixture
import model_resume as terminal
from quota_resume import Rpc
from terminal_controls import drive_pty


def seed_prompts(shared_owner):
    return {shared_owner: (terminal.SEED, terminal.REPLY), "source": (terminal.AFTER_RELEASE, terminal.FINAL_ANSWER),
            "destination": (terminal.BIRCH_SEED, terminal.BIRCH_REPLY)}


def matches(lines, login, owners, selected=None, *, shared_owner="api-key"):
    if not any("Resume a previous session" in line for line in lines) or not any(
            f"Filter: All Logins: {login} Active " in " ".join(line.split()) + " " for line in lines):
        return False
    prompts = {prompt: owner for owner, (prompt, _) in seed_prompts(shared_owner).items()}
    rows = []
    for line in lines:
        if "● " in line:
            owner = prompts.get(line.rstrip().split("● ", 1)[1])
            if owner is None:
                return False
            rows.append((owner, line.lstrip().startswith("›")))
    highlighted = [(i, owner) for i, (owner, marked) in enumerate(rows, 1) if marked]
    if sorted(owner for owner, _ in rows) != sorted(owners) or len(highlighted) != 1:
        return False
    index, owner = highlighted[0]
    return (selected is None or owner == selected) and any(f" {index} / {len(owners)} · 100%" in s for s in lines)


def stored_creators(home):
    with closing(sqlite3.connect((home / "state_5.sqlite").as_uri() + "?mode=ro", uri=True)) as db:
        return {thread: (user, account_id) for thread, user, account_id in db.execute(
            "SELECT id, creator_user_id, creator_account_id FROM threads")}


def exercise(binary, root, port, gateway, report, legacy_binary):
    env, slots = account.configure_accounts(root, port, gateway)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm")
    home, work = root / "codex-home", root / "work"
    shared_owner = "legacy-source" if legacy_binary else "api-key"
    credentials = dict(slots)
    if legacy_binary:
        fixture.require(fixture.run(legacy_binary, ["--version"], root, env, "legacy-version") == 0,
                        "could not identify the supplied legacy CLI")
        report["legacy_version"] = (root / "legacy-version-stdout.jsonl").read_text().strip()
    else:
        key = "sk-loopback-" + secrets.token_hex(16)
        gateway.tokens["Bearer " + key] = "api-key"

        def provide_key(process, _output):
            process.stdin.write((key + "\n").encode())

        fixture.require(fixture.run(binary, ["login", "--with-api-key"], root, env, "key-login", interact=provide_key) == 0,
                        "the CLI did not accept the fixture API key")
        credentials["api-key"] = home / "auth.json"
    digests = {owner: terminal.CHECK["sha256"](path) for owner, path in credentials.items()}
    marker = "shared-cedar-" + secrets.token_hex(8)
    (work / "cedar-marker.txt").write_text(marker + "\n")

    def environment(owner):
        if legacy_binary:
            # Both generations use the ordinary shared file; select A or B before launch.
            shutil.copy2(slots["source" if owner == shared_owner else owner], home / "auth.json")
            return env
        return env if owner == "api-key" else {**env, "CODEX_AUTH_PATH": str(slots[owner])}

    seeds = report["seeds"] = {}
    for owner, (prompt, answer) in seed_prompts(shared_owner).items():
        cli = legacy_binary if legacy_binary and owner == shared_owner else binary
        thread, turn = account.exec_turn(cli, ["exec", "--json", "--skip-git-repo-check", prompt],
                                        root, environment(owner), owner + "-seed", answer, cwd=work)
        seeds[owner] = {"thread": thread, "turn": turn}
        if legacy_binary and owner == shared_owner:
            path, = (home / "sessions").rglob(f"rollout-*-{thread}.jsonl")
            original = path.read_bytes()
            meta = json.loads(original.splitlines()[0])["payload"]
            fixture.require("account_scoped" not in meta and meta.get("creator_user_id") == "user-source"
                            and meta.get("creator_account_id") == "workspace-source"
                            and report["legacy_version"] == "codex-cli " + meta["cli_version"],
                            "expected an authentic pre-enforcement thread that recorded A as creator")
            fixture.require(terminal.CHECK["sha256"](home / "auth.json") == digests["source"],
                            "the original seed changed A's login")
            report["legacy_original"] = {"bytes": len(original), "sha256": hashlib.sha256(original).hexdigest(),
                                         "cli_version": meta["cli_version"]}
    before = histories(home)
    fixture.require(len(before) == len({seed["thread"] for seed in seeds.values()}) == 3, "expected three threads")
    for owner, seed in seeds.items():
        path, = (path for path in before if path.name.endswith(f"-{seed['thread']}.jsonl"))
        first = json.loads(before[path].splitlines()[0])
        meta = first["payload"]
        fixture.require(first["type"] == "session_meta" and meta["id"] == seed["thread"] and meta["source"] == "exec"
                        and meta["cwd"] == str(work),
                        "seed has the wrong thread metadata")
        if legacy_binary and owner == shared_owner:
            fixture.require(before[path] == original, "modern neighbor creation changed the historical rollout")
        else:
            fixture.require(meta.get("account_scoped") is True, "the modern seed omitted its enforcement marker")
        if owner == shared_owner:
            shared = path
        if owner == "api-key":
            fixture.require("creator_account_id" not in meta and "creator_user_id" not in meta,
                            "the API-key thread already has ChatGPT creator IDs")
        else:
            login = "source" if owner == "legacy-source" else owner
            fixture.require((meta.get("creator_account_id"), meta.get("creator_user_id")) ==
                            ("workspace-" + login, "user-" + login), "ChatGPT creator mismatched its login")
        seed["header"] = {k: meta[k] for k in ("account_scoped", "creator_account_id", "creator_user_id") if k in meta}

    creators = {seed["thread"]: (seed["header"].get("creator_user_id"), seed["header"].get("creator_account_id"))
                for seed in seeds.values()}
    fixture.require(stored_creators(home) == creators, "the index disagrees with the three original creators")
    # Compare the shared thread’s original viewer and B through the public Current filter.
    report["current"] = {}
    original_view = ("source", [shared_owner, "source"]) if legacy_binary else ("api-key", list(seeds))
    for viewer, expected in (original_view, ("destination", ["destination", shared_owner])):
        def current(process, output):
            rpc = Rpc(process, output)
            rpc.initialize()
            page = rpc.request("thread/list", {"currentAccountOnly": True, "useStateDbOnly": True,
                                               "sourceKinds": ["exec"], "archived": False})
            ids = sorted(row["id"] for row in page["data"])
            fixture.require(ids == sorted(seeds[owner]["thread"] for owner in expected) and not page["nextCursor"],
                            f"Current returned unexpected threads for {viewer}")
            report["current"][viewer] = ids

        fixture.require(fixture.run(binary, ["app-server", "--listen", "stdio://"], root, environment(viewer),
                                    viewer + "-list", interact=current) == 0, "public thread/list failed")

    with gateway.lock:
        gateway.resumed = True
        report["seed_posts"] = len(gateway.requests)
    visible = [shared_owner, "destination"]
    match_view = partial(matches, shared_owner=shared_owner)
    frames = report["frames"] = {}

    def browse(pty):
        deadline = time.monotonic() + 50
        frame = partial(record_frame, pty, frames, deadline, "picker")
        frame("all", lambda lines: match_view(lines, "All local", list(seeds)))
        frame("current", lambda lines: match_view(lines, "Current", visible), b"\t\x1b[C")
        if not match_view(frames["current"], "Current", visible, shared_owner):
            # Each filter reload starts on the first row. With two rows the other is one step away.
            frame("selected", lambda lines: match_view(lines, "Current", visible, shared_owner), b"\x1b[B")
        pty.send(b"\r")
        pty.displayed(terminal.REPLY)
        with gateway.lock:
            fixture.require(len(gateway.requests) == report["seed_posts"], "picking the shared conversation ran inference")
        pty.paste(terminal.CONTINUE, terminal.CONTINUE)
        pty.send(b"\r")
        pty.displayed(terminal.ANSWER)
        report["continued_turn"] = terminal.wait_for_turn_completion(root, gateway, pty.process, terminal.ANSWER,
                                                                     deadline, rollout_path=shared)
        pty.quit()

    drive_pty([str(binary), "resume"], root, environment("destination"), "picker", browse)
    after = histories(home)
    fixture.require(before.keys() == after.keys() and after[shared].startswith(before[shared])
                    and len(after[shared]) > len(before[shared]) and all(after[p] == d for p, d in before.items()
                                                                       if p != shared), "history preservation failed")
    fixture.require(stored_creators(home) == creators and all(row["type"] != "session_meta" for row in
                    map(json.loads, after[shared][len(before[shared]):].splitlines())),
                    "continuation changed the recorded creator of a conversation")
    fixture.require(all(terminal.CHECK["sha256"](path) == digests[owner] for owner, path in credentials.items()),
                    "a credential changed while browsing or continuing the shared thread")
    if legacy_binary:
        fixture.require(terminal.CHECK["sha256"](home / "auth.json") == digests["destination"],
                        "the active shared login changed during B's continuation")
    report["preserved_bytes"] = len(before[shared])
    report["original_rollout_sha256"] = {p.name: hashlib.sha256(data).hexdigest() for p, data in before.items()}
    report["pty_sha256"] = terminal.CHECK["sha256"](root / "picker.raw")
    return marker


def check_requests(report, marker):
    shared, tool, a, b, continued = report["requests"]
    shared_owner = "legacy-source" if "legacy_version" in report else "api-key"
    for owner, requests in ((shared_owner, [shared, tool]), ("source", [a]), ("destination", [b])):
        seed = report["seeds"][owner]
        login = "source" if owner == "legacy-source" else owner
        account_id = None if login == "api-key" else "workspace-" + login
        fixture.require(all((r["login"], r["account"], r["thread"], r["turn"]) ==
                            (login, account_id, seed["thread"], seed["turn"]) for r in requests),
                        "seed request identity or credential changed")
    earlier_turns = {r["turn"] for r in (shared, a, b)}
    fixture.require(report["seed_posts"] == 4 and continued["login"] == "destination"
                    and continued["account"] == "workspace-destination" and continued["thread"] == shared["thread"]
                    and continued["turn"] == report["continued_turn"] and continued["turn"] not in earlier_turns
                    and continued["history"] == [*tool["history"], terminal.REPLY, terminal.CONTINUE]
                    and continued["tool_records"] == tool["tool_records"] and marker in tool["tool_records"][-1]["output"],
                    "B did not continue the original shared conversation with its completed tool history")
    reads = [r for r in report["account_reads"] if r["phase"] == "resume"]
    fixture.require(reads and all((r["login"], r["account"]) == ("destination", "workspace-destination") for r in reads),
                    "picker metadata used a different credential")


def run(binary, root, legacy_binary=None):
    root = root.parent.resolve(strict=True) / root.name
    root.mkdir(mode=0o700)
    report = {"passed": False, "events": [], "requests": [], "binary_sha256": terminal.CHECK["sha256"](binary)}
    if legacy_binary:
        report["legacy_binary_sha256"] = terminal.CHECK["sha256"](legacy_binary)
    gateway = account.Gateway(report)
    try:
        server, worker = account.start_gateway(root, gateway)
        try:
            marker = exercise(binary, root, server.server_port, gateway, report, legacy_binary)
        finally:
            fixture.cleanup_resources((("gateway", server, worker),), (), report, sys.exc_info()[1])
        check_requests(report, marker)
        fixture.require(not report.get("cleanupErrors") and terminal.CHECK["sha256"](binary) == report["binary_sha256"],
                        "gateway cleanup or binary identity check failed")
        if legacy_binary:
            fixture.require(terminal.CHECK["sha256"](legacy_binary) == report["legacy_binary_sha256"], "seed CLI changed")
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
    parser.add_argument("--legacy-codex", type=Path,
                        help="Create A's shared history with an older CLI that predates account filtering.")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), args.output.absolute(),
                 args.legacy_codex.resolve(strict=True) if args.legacy_codex else None))
