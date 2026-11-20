# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the exact-title lookup's 100-result cursor with real thread forks and two logins."""

import capacity_exec as fixture
from account_resume_page import fork_page
from account_resume_titles import TITLE
from quota_resume import Rpc

LIMIT = 100
NEAR_TITLE = TITLE + " draft"


def app_server(binary, root, env, label, exercise, cwd):
    def interact(proc, stream):
        rpc = Rpc(proc, stream)
        rpc.initialize()
        exercise(rpc)

    status = fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, label,
                         interact=interact, cwd=cwd)
    fixture.require(status == 0, f"{label} app-server failed")


def check_pages(first, last, expected, a, b, b_title):
    recent, older = first["data"], last["data"]
    ids = [row["id"] for row in [*recent, *older]]
    fixture.require(len(expected) == LIMIT + 1 and len(recent) == LIMIT and len(older) == 1
                    and isinstance(first.get("nextCursor"), str) and "|" in first["nextCursor"]
                    and not last.get("nextCursor") and len(set(ids)) == len(ids) and set(ids) == expected,
                    "pagination did not enumerate each of the 101 original threads exactly once")
    fixture.require((recent[-1]["id"], recent[-1]["source"], recent[-1]["name"]) == (b, "vscode", b_title)
                    and (older[0]["id"], older[0]["source"], older[0]["name"]) == (a, "exec", TITLE),
                    "destination editor and source exec were not on opposite sides of the boundary")
    rows = recent + older
    fixture.require(all(TITLE in row["name"] for row in rows), "a label lacked the common search term")
    positions = [(i, row["id"]) for i, row in enumerate(rows, 1) if row["name"] == TITLE]
    fixture.require(positions == ([(LIMIT, b)] if b_title == TITLE else []) + [(LIMIT + 1, a)],
                    "expected only the declared exact matches among all substring matches")
    return {"ids": ids, "cursor": first["nextCursor"], "exact_positions": positions}


def prepare(binary, root, env, a, b, cwd):
    home = root / "codex-home"
    config = home / "config.toml"
    config.write_text("thread_unload_delay_secs = 0\n" + config.read_text())
    expected = {a, b}

    def populate(rpc):
        rpc.request("thread/name/set", {"threadId": b, "name": NEAR_TITLE})
        forks = fork_page(rpc, b, a, size=LIMIT) - {b}
        expected.update(forks)
        for i, thread in enumerate(sorted(forks)):
            rpc.request("thread/name/set", {"threadId": thread, "name": f"{NEAR_TITLE} {i:03d}"})

    app_server(binary, root, env, "fork-titles", populate, cwd)
    return expected


def observe_pages(binary, root, env, label, expected, a, b, cwd):
    pages = {}
    b_title = TITLE if label == "collision" else NEAR_TITLE

    def measure(rpc):
        # The real resolver lists all sources without a search term and compares labels locally.
        # Independently prove that even a server-side term search crosses the same boundary.
        params = {"limit": LIMIT, "sortKey": "created_at", "modelProviders": [], "archived": False,
                  "currentAccountOnly": False, "includeAttachedThreads": False, "useStateDbOnly": True,
                  "sourceKinds": ["cli", "vscode", "exec", "appServer"]}
        for mode, query in (("resolver", params), ("substring", {**params, "searchTerm": TITLE})):
            first = rpc.request("thread/list", query)
            fixture.require(first.get("nextCursor"), "server returned no continuation")
            last = rpc.request("thread/list", {**query, "cursor": first["nextCursor"]})
            pages[mode] = check_pages(first, last, expected, a, b, b_title)
        fixture.require(pages["resolver"]["ids"] == pages["substring"]["ids"],
                        "the public substring search disagrees with actual resolver order")

    app_server(binary, root, env, label + "-pages", measure, cwd)
    return pages
