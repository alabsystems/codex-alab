# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Compare a restored rollout on disk with its healthy but incomplete SQLite index."""

from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import tempfile
import time

import capacity_exec as fixture
import model_resume as terminal
from picker_screen import SYNC_END, visible_lines
from quota_resume import Rpc


def index_state(database):
    with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as db:
        integrity = db.execute("PRAGMA integrity_check").fetchall()
        violations = db.execute("PRAGMA foreign_key_check").fetchall()
        fixture.require(integrity == [("ok",)] and not violations, "fixture index failed SQLite integrity checks")
        status, = db.execute("SELECT status FROM backfill_state WHERE id = 1").fetchone()
        fixture.require(status == "complete", "index backfill has not finished")
        return {"thread_ids": [row[0] for row in db.execute("SELECT id FROM threads ORDER BY id")],
                "backfill": status, "integrity": "ok", "foreign_key_violations": len(violations)}


def drop_source(root, original, sibling, database):
    home = root / "codex-home"
    fixture.require(database.resolve(strict=True) == home.resolve(strict=True) / "state_5.sqlite",
                    "expected the disposable fixture's own state database")
    with closing(sqlite3.connect(database.as_uri() + "?mode=rw", uri=True)) as db:
        db.execute("PRAGMA foreign_keys = ON")
        with db:
            rows = db.execute("SELECT id, rollout_path, source, cwd FROM threads ORDER BY id").fetchall()
            fixture.require(len(rows) == 2 and {row[0] for row in rows} == {original, sibling},
                            "fixture must begin with exactly two indexed conversations")
            for thread, path, source, cwd in rows:
                rollout = Path(path).resolve(strict=True)
                fixture.require(source == "exec" and rollout.is_relative_to(home / "sessions")
                                and rollout.name.endswith(f"-{thread}.jsonl")
                                and cwd == str(root / ("work" if thread == original else "second-project")),
                                "unexpected indexed source, rollout or directory")
            sibling_before = db.execute("SELECT * FROM threads WHERE id = ?", (sibling,)).fetchone()
            fixture.require(db.execute("DELETE FROM threads WHERE id = ?", (original,)).rowcount == 1,
                            "could not remove exactly the original index row")
            fixture.require(db.execute("SELECT * FROM threads WHERE id = ?", (sibling,)).fetchone() == sibling_before,
                            "restoring the fixture changed the destination index row")


def observe_picker(binary, root, env, gateway, stage):
    result = {}

    def capture(raw, process, deadline):
        while process.poll() is None and time.monotonic() < deadline:
            data = raw.read_bytes() if raw.exists() else b""
            for expected in (["birch", "cedar"], ["birch"]):
                rows = terminal.bare_picker_rows(data, expected=expected)
                if rows is None:
                    continue
                lines = [line.rstrip() for line in visible_lines(data) if line.strip()]
                if any("Loading" in line or "Searching" in line for line in lines):
                    continue
                end = data.rindex(SYNC_END) + len(SYNC_END)
                result.update(rows=rows, screen=lines, before_input_end_byte=end)
                return end
            time.sleep(.05)
        raise RuntimeError(f"the {stage} default picker did not settle")

    custody, _, _ = terminal.terminal_session(binary, root, env, stage, ["resume"], gateway, [],
                                              cwd=root / "second-project", observe_bare_picker=capture, label=stage)
    terminal.CHECK["check_model_terminal_exit"](custody, [str(binary), "resume"])
    result["custody"] = custody
    return result


def exercise_progressive_restore(binary, root, env, original, sibling, gateway, database, rollout):
    """Simulate a rollout arriving in two writes and a blank row left by an older reader."""
    def indexed(thread):
        with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as db:
            db.row_factory = sqlite3.Row
            row = db.execute("SELECT * FROM threads WHERE id = ?", (thread,)).fetchone()
            return dict(row) if row else None

    def picker(stage, expected):
        observed = observe_picker(binary, root, env, gateway, stage)
        fixture.require(sorted(observed["rows"]) == expected, f"{stage} showed {observed['rows']}")
        return {"rows": observed["rows"], "raw_sha256": terminal.CHECK["sha256"](root / f"{stage}.raw")}

    report = {}
    original_row = indexed(original)
    sibling_row = indexed(sibling)
    saved = rollout.read_bytes()
    header, rest = saved.split(b"\n", 1)
    metadata = json.loads(header)
    fields = ("first_user_message", "preview")
    fixture.require(metadata["type"] == "session_meta" and metadata["payload"]["id"] == original
                    and rest and original_row and sibling_row and all(original_row[key] for key in fields),
                    "expected a complete, visible signed source thread")
    drop_source(root, original, sibling, database)
    try:
        rollout.write_bytes(header + b"\n")
        report["header_only"] = picker("restored-header-only", ["birch"])
    finally:
        with rollout.open("ab") as stream:
            stream.write(rest)
    fixture.require(rollout.read_bytes() == saved, "incremental restore did not reproduce the signed history")
    report["complete"] = picker("restored-complete", ["birch", "cedar"])
    recovered = indexed(original)
    fixture.require(recovered and all(recovered[k] == original_row[k] for k in fields),
                    "completed history did not recover the original preview")

    # Older readers could persist a valid index row before the first user message arrived.
    with closing(sqlite3.connect(database.as_uri() + "?mode=rw", uri=True)) as db, db:
        fixture.require(db.execute("UPDATE threads SET first_user_message = '', preview = '' WHERE id = ?",
                                   (original,)).rowcount == 1, "expected one source index row")
    report["previously_blank"] = picker("restored-previously-blank", ["birch", "cedar"])
    fixture.require(indexed(original) == recovered, "repair changed fields beyond the missing source preview")
    fixture.require(indexed(sibling) == sibling_row, "browsing the restored history changed B's index row")
    return report


def exercise(binary, root, env, original, sibling, gateway, report):
    home = root / "codex-home"
    fixture.require(Path(env["CODEX_HOME"]).resolve(strict=True) == home.resolve(strict=True),
                    "mixed-index preparation requires the synthetic Codex home")
    database = home / "state_5.sqlite"
    with gateway.lock:
        seed_requests = list(gateway.requests)
    fixture.require([(row["login"], row["account"], row["thread"]) for row in seed_requests if row["history"]] ==
                    [("source", "workspace-source", original)] * 2 + [("destination", "workspace-destination", sibling)],
                    "expected signed-in A and B to create their own conversations")
    rollouts = sorted((home / "sessions").rglob("rollout-*.jsonl"))
    fixture.require(len(rollouts) == 2 and all(any(path.name.endswith(f"-{thread}.jsonl") for path in rollouts)
                                                  for thread in (original, sibling)),
                    "expected exactly A's signed exec rollout and B's current rollout")

    def files():
        return {str(path.relative_to(home)): {"sha256": terminal.CHECK["sha256"](path), "bytes": path.stat().st_size}
                for path in rollouts}

    phases = report["states"] = {}
    lists = report["list_controls"] = {}
    report.update(source_thread=original, destination_thread=sibling, rollouts=files())

    def query(stage, requests):
        def controls(process, output):
            rpc = Rpc(process, output)
            rpc.initialize()
            identity = rpc.request("account/read", {"refreshToken": False})
            fixture.require(identity["account"]["email"] == "destination@example.invalid"
                            and identity["workspaceRouting"]["chatgptAccountId"] == "workspace-destination",
                            "thread lists must run as B")
            for name, current, mode in requests:
                params = {"archived": False, "limit": 25, "sortKey": "updated_at", "modelProviders": [],
                          "sourceKinds": ["cli", "vscode", "exec", "appServer"], "currentAccountOnly": current}
                if mode is not None:
                    params["useStateDbOnly"] = mode
                page = rpc.request("thread/list", params)
                rows = [{key: row[key] for key in ("id", "source", "cwd")} for row in page["data"]]
                lists[name] = {"params": params, "rows": rows, "next_cursor": page["nextCursor"]}
                fixture.require(page["nextCursor"] is None and all(row["source"] == "exec"
                                and row["id"] in (original, sibling) and row["cwd"] ==
                                str(root / ("work" if row["id"] == original else "second-project")) for row in rows),
                                "unexpected list origin, directory or continuation")
        status = fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, stage, interact=controls,
                             cwd=root / "second-project")
        fixture.require(status == 0, f"{stage} app-server did not exit cleanly")
        phases[stage] = index_state(database)

    def ids(name):
        return sorted(row["id"] for row in lists[name]["rows"])

    query("seeded", [("current_seeded", True, True), ("all_seeded", False, True)])
    fixture.require(ids("current_seeded") == [sibling] and ids("all_seeded") == sorted((original, sibling)),
                    "the original index did not distinguish the source and destination accounts")
    drop_source(root, original, sibling, database)
    phases["restored"] = index_state(database)
    fixture.require(phases["restored"]["thread_ids"] == [sibling] and files() == report["rollouts"],
                    "restored home must contain both unchanged rollouts and just B's index row")

    query("index-only", [("current_db", True, True), ("all_db", False, True)])
    fixture.require(ids("current_db") == ids("all_db") == [sibling]
                    and phases["index-only"]["thread_ids"] == [sibling], "DB-only controls changed the precondition")
    # One inaccessible sibling previously discarded every readable discovery.
    with tempfile.TemporaryDirectory(dir=home / "sessions") as temporary:
        unrelated = Path(temporary)
        try:
            if os.name == "posix":
                unrelated.chmod(0)
            try:
                list(unrelated.iterdir())
            except PermissionError as error:
                report["unrelated_directory_errno"] = error.errno
            else:
                report["unrelated_directory_errno"] = None  # Privileged or non-POSIX runner.
            report["first_picker"] = observe_picker(binary, root, env, gateway, "unindexed-picker")
        finally:
            if os.name == "posix":
                unrelated.chmod(0o700)
    phases["first_picker"] = index_state(database)
    # A normal store enumeration may populate SQLite. Run it strictly after observing bare startup.
    query("store-default", [("all_store_default", False, None), ("current_db_after_store", True, True),
                            ("all_db_after_store", False, True)])
    fixture.require(ids("all_store_default") == sorted((original, sibling)),
                    "rollout-aware listing did not enumerate both local conversations")
    fixture.require(ids("current_db_after_store") == [sibling], "recovery attributed A's conversation to B")
    report["after_store_picker"] = observe_picker(binary, root, env, gateway, "after-store-picker")
    phases["after_store_picker"] = index_state(database)
    report["evidence_sha256"] = {name: terminal.CHECK["sha256"](root / name) for name in (
        "seed-stdout.jsonl", "sibling-stdout.jsonl", "seeded-stdout.jsonl", "index-only-stdout.jsonl",
        "store-default-stdout.jsonl", "unindexed-picker.raw", "unindexed-picker-custody.json",
        "after-store-picker.raw", "after-store-picker-custody.json")}
    fixture.require(sorted(report["first_picker"]["rows"]) == ["birch", "cedar"],
                    "literal bare resume omitted A's intact rollout before the store listing")
    fixture.require(sorted(report["after_store_picker"]["rows"]) == ["birch", "cedar"],
                    "reopening the bare picker lost a previously visible conversation")
    rollout, = (path for path in rollouts if path.name.endswith(f"-{original}.jsonl"))
    report["progressive"] = exercise_progressive_restore(binary, root, env, original, sibling, gateway,
                                                          database, rollout)
    report["rollouts_after_observations"] = files()
    fixture.require(report["rollouts_after_observations"] == report["rollouts"],
                    "an observation changed the saved rollout bytes")
    with gateway.lock:
        fixture.require(gateway.requests == seed_requests, "browsing history unexpectedly submitted a model request")
    # The continuing PTY must also start from the mixed state, independently of the observations.
    drop_source(root, original, sibling, database)
    phases["before_continuation"] = index_state(database)
    fixture.require(phases["before_continuation"]["thread_ids"] == [sibling] and files() == report["rollouts"],
                    "the continuation must start from B's row and both unchanged rollouts")
