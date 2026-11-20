# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Observe the bare resume picker's second page using real local thread forks."""

import re
import time

import capacity_exec as fixture
from picker_screen import visible_lines
from quota_resume import Rpc


PAGE_SIZE = 25
OLDER_QUERY = "cedar"


def check_pages(b_threads, original, original_cwd, destination_cwd, current, first, later):
    """Require B's exact newer threads, then A's original across the returned cursor."""
    own, recent, older = (page["data"] for page in (current, first, later))
    def ids(rows):
        return [row["id"] for row in rows]
    fixture.require(len(b_threads) == PAGE_SIZE and original not in b_threads
                    and len(own) == PAGE_SIZE and set(ids(own)) == b_threads and not current["nextCursor"],
                    "current-login enumeration must contain precisely the 25 destination threads")
    fixture.require(len(recent) == PAGE_SIZE and set(ids(recent)) == b_threads and first["nextCursor"]
                    and ids(older) == [original] and not later["nextCursor"],
                    "the source thread must appear only after the first 25-result cursor")
    timestamps = [row["updatedAt"] for row in recent]
    fixture.require(older[0]["source"] == "exec" and older[0]["cwd"] == str(original_cwd)
                    and all(row["cwd"] == str(destination_cwd) for row in recent)
                    and timestamps == sorted(timestamps, reverse=True) and timestamps[-1] > older[0]["updatedAt"],
                    "page one must contain strictly newer destination threads in recency order")
    return {"current_login_ids": ids(own), "first_page_ids": ids(recent), "next_cursor": first["nextCursor"],
            "second_page_ids": ids(older), "source_updated_at": older[0]["updatedAt"],
            "oldest_destination_updated_at": timestamps[-1]}


def fork_page(rpc, parent, older, *, size=PAGE_SIZE):
    newer = {parent}
    # Fork the saved conversation through the public API, without a provider request per row.
    for _ in range(size - 1):
        thread = rpc.request("thread/fork", {"threadId": parent})["thread"]["id"]
        fixture.require(thread not in newer and thread != older, "thread/fork did not create a new conversation")
        newer.add(thread)
        fixture.require(rpc.request("thread/unsubscribe", {"threadId": thread})["status"] == "unsubscribed",
                        "could not release the fork's subscription")
        rpc.wait(lambda msg: msg.get("method") == "thread/closed" and msg["params"]["threadId"] == thread)
    return newer


def list_pages(rpc):
    params = {"sortKey": "updated_at", "archived": False, "modelProviders": [], "useStateDbOnly": True,
              "sourceKinds": ["cli", "vscode", "exec", "appServer"], "limit": PAGE_SIZE}
    current = rpc.request("thread/list", {**params, "currentAccountOnly": True, "limit": PAGE_SIZE + 1})
    first = rpc.request("thread/list", {**params, "currentAccountOnly": False})
    fixture.require(isinstance(first.get("nextCursor"), str) and first["nextCursor"],
                    "thread/list did not supply a continuation cursor")
    later = rpc.request("thread/list", {**params, "currentAccountOnly": False, "cursor": first["nextCursor"]})
    return current, first, later


def prepare(binary, root, env, original, sibling, destination_cwd):
    report = {}

    def exercise(process, output):
        rpc = Rpc(process, output)
        rpc.initialize()
        b_threads = fork_page(rpc, sibling, original)
        current, first, later = list_pages(rpc)
        report.update(check_pages(b_threads, original, root / "work", destination_cwd, current, first, later))

    status = fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, "pagination-rpc",
                         interact=exercise, cwd=destination_cwd)
    fixture.require(status == 0, "pagination app-server did not exit cleanly")
    return report


def picker_position(raw, *, after=0, query=None):
    return position_from_lines(visible_lines(raw, after), query=query)


def position_from_lines(lines, *, query=None):
    if query is not None and not any(line.strip() == f"Search: {query}" for line in lines):
        return None
    if not any("Resume a previous session" in line for line in lines) or not any(
            "Filter: All Logins: All local Active" in " ".join(line.split()) for line in lines):
        return None
    counters = [match for line in lines if (match := re.search(r"\b(\d+) / (\d+)…? · \d+%", line))]
    if len(counters) != 1:
        return None
    current, total = map(int, counters[0].groups())
    return current, total, any("Remember the birch" in line for line in lines), any("Remember the cedar" in line for line in lines)


def search_for_older(raw, send, process, deadline):
    offset = None
    while time.monotonic() < deadline and process.poll() is None:
        data = raw.read_bytes() if raw.is_file() else b""
        if offset is None:
            if picker_position(data) == (1, PAGE_SIZE, True, False):
                offset = len(data)
                send(OLDER_QUERY.encode())
        elif picker_position(data, after=offset, query=OLDER_QUERY) == (1, 1, False, True):
            return [[1, PAGE_SIZE], [1, 1]]
        time.sleep(.05)
    raise RuntimeError("searching bare resume did not find A's original conversation beyond the first page")


def select_older(raw, send, process, deadline):
    offset, positions = 0, []
    while time.monotonic() < deadline and process.poll() is None:
        data = raw.read_bytes() if raw.is_file() else b""
        if position := picker_position(data, after=offset):
            current, total, birch, cedar = position
            if not positions:
                ready = current == 1 and total == PAGE_SIZE and birch and not cedar
            else:
                last_row, last_total = positions[-1]
                ready = current >= last_row and total >= last_total and [current, total] != positions[-1]
            if ready:
                positions.append([current, total])
                if current == total == PAGE_SIZE + 1:
                    fixture.require(cedar, "last picker row did not display the source conversation")
                    return positions
                offset = len(data)
                send(b"\x1b[6~")  # Page Down keeps the default scopes and sort order.
        time.sleep(.05)
    raise RuntimeError(f"bare resume did not scroll from newer B conversations to A on row {PAGE_SIZE + 1}")
