# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Observe cancellation and continuation after a local project is renamed."""

from contextlib import closing
import sqlite3

import capacity_exec as fixture
import model_resume as terminal


def histories(home):
    return {path: path.read_bytes() for folder in (home / "sessions", home / "archived_sessions")
            for path in folder.rglob("rollout-*.jsonl")}


def thread_rows(home):
    with closing(sqlite3.connect((home / "state_5.sqlite").as_uri() + "?mode=ro", uri=True)) as db:
        return db.execute("SELECT * FROM threads ORDER BY id").fetchall()


def cancel_at_directory_choice(binary, root, env, gateway, cwd, thread, sibling):
    home = root / "codex-home"
    before = histories(home)
    fixture.require(len(before) == 2 and all(any(path.name.endswith(f"-{owner}.jsonl") for path in before)
                                            for owner in (thread, sibling)),
                    "expected just the two original conversations before cancelling")
    indexed = thread_rows(home)
    fixture.require(len(indexed) == 2, "expected two indexed conversations before cancelling")
    with gateway.lock:
        requests_before = len(gateway.requests)
    custody, _, _ = terminal.terminal_session(
        binary, root, env, "bare-all-default", ["resume"], gateway, [],
        cwd=cwd, directory_choice="cancel", label="cancel-directory")
    terminal.CHECK["check_model_terminal_exit"](custody, [str(binary), "resume"])
    with gateway.lock:
        fixture.require(len(gateway.requests) == requests_before, "cancelling the directory choice started a model request")
    fixture.require(histories(home) == before and thread_rows(home) == indexed,
                    "cancelling the directory choice changed a saved conversation or its index entry")
    return before


def verify_continuation(home, original, selected, before):
    after = histories(home)
    fixture.require(not original.exists(), "resuming recreated the old project directory")
    fixture.require(after.keys() == before.keys() and selected in before,
                    "resuming changed the set of saved conversations")
    prefix = before[selected]
    fixture.require(len(after[selected]) > len(prefix) and after[selected].startswith(prefix),
                    "continuation did not append to the original conversation")
    fixture.require(all(data == before[path] for path, data in after.items() if path != selected),
                    "continuation changed the other account's conversation")
    return len(prefix)
