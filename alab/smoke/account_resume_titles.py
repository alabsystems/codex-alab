# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Select two same-named conversations from the same local directory."""

from functools import partial
import re
import time

import capacity_exec as fixture
import model_resume as terminal
from picker_screen import visible_lines
from quota_resume import Rpc
from terminal_controls import drive_pty


TITLE = "Release notes"


def name_thread(binary, root, env, login, thread, *, cwd=None):
    def rename(process, stdout):
        rpc = Rpc(process, stdout)
        rpc.initialize()
        rpc.request("thread/name/set", {"threadId": thread, "name": TITLE})
        row = rpc.request("thread/read", {"threadId": thread})["thread"]
        fixture.require((row["id"], row["cwd"], row["name"]) == (thread, str(cwd or root / "work"), TITLE),
                        "rename did not persist the intended title on the original thread")

    status = fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, "name-" + login, interact=rename)
    fixture.require(status == 0, f"could not name the {login} conversation")


def selection(lines, login_scope, index, count, *, expanded=False):
    toolbar = f"Filter: All Logins: {login_scope} Active"
    rows = [line.strip() for line in lines if line.rstrip().endswith(" " + TITLE)
            and "│" not in line and not line.lstrip().startswith("Search:")]
    counters = [(int(a), int(b)) for line in lines for a, b in re.findall(r"\b(\d+) / (\d+) · 100%", line)]
    marker = "⌄" if expanded else "›"
    return (any("Resume a previous session" in line for line in lines)
            and any(toolbar in " ".join(line.split()) for line in lines)
            and counters == [(index, count)] and len(rows) == count
            and [i for i, row in enumerate(rows, 1) if row.startswith(("›", "⌄"))] == [index]
            and rows[index - 1].startswith(marker))


def details(lines, scope, index, count, thread, replies, other_replies):
    contents = [line.strip(" │└") for line in lines]
    identities = [" ".join(line.split()) for line in contents if line.startswith("Session:")]
    return (selection(lines, scope, index, count, expanded=True)
            and identities == [f"Session: {TITLE} ({thread})"] and "Conversation:" in contents
            and any(reply in contents for reply in replies) and not any(reply in contents for reply in other_replies))


def record_frame(pty, frames, deadline, label, name, matches, keys=None):
    offset = pty.raw.stat().st_size if keys else 0
    if keys:
        pty.send(keys)
    while time.monotonic() < deadline and pty.process.poll() is None:
        lines = visible_lines(pty.raw.read_bytes(), after=offset) if pty.raw.exists() else []
        if matches(lines):
            frames[name] = [line.rstrip() for line in lines if line.strip()]
            return
        time.sleep(.05)
    raise RuntimeError(f"{label} did not display {name}")


def exercise(binary, root, env, gateway, original, sibling):
    """Observe B's Current control, select A on row two, then B on row two after A becomes newest."""
    fixture.require(original != sibling, "duplicate-title control needs distinct threads")
    owners = {"source": (original, (terminal.REPLY, terminal.ANSWER)),
              "destination": (sibling, (terminal.BIRCH_REPLY, terminal.BIRCH_ANSWER))}
    sessions = root / "codex-home/sessions"
    files = {}
    for owner, (thread, _) in owners.items():
        files[owner], = sessions.glob(f"*/*/*/rollout-*-{thread}.jsonl")
    fixture.require(set(sessions.rglob("rollout-*.jsonl")) == set(files.values()), "expected just two original rollouts")
    # The list API exposes whole-second timestamps. Give A's continuation a later timestamp than B's seed.
    earliest_continuation = time.monotonic() + 1.1
    results = {}
    for case in ("current", "source", "destination"):
        frames = {}
        results[case] = frames
        label = "same-title-" + case

        def interact(pty):
            deadline = time.monotonic() + 55

            frame = partial(record_frame, pty, frames, deadline, label)

            def expand(owner, scope, index, count):
                thread, replies = owners[owner]
                other = "destination" if owner == "source" else "source"
                frame("expanded-" + owner,
                      lambda lines: details(lines, scope, index, count, thread, replies, owners[other][1]), b"\x05")

            frame("initial", lambda lines: selection(lines, "All local", 1, 2))
            if case == "current":
                pty.send(b"\t")
                frame("narrowed", lambda lines: selection(lines, "Current", 1, 1), b"\x1b[C")
                expand("destination", "Current", 1, 1)
            else:
                other = "destination" if case == "source" else "source"
                expand(other, "All local", 1, 2)
                frame("collapsed", lambda lines: selection(lines, "All local", 1, 2), b"\x05")
                frame("second-row", lambda lines: selection(lines, "All local", 2, 2), b"\x1b[B")
                expand(case, "All local", 2, 2)
                previous = owners[case][1][0]
                frame("resumed", lambda lines: any(previous in line for line in lines)
                      and not any("Resume a previous session" in line for line in lines), b"\r")
                prompt, answer, event = ((terminal.CONTINUE, terminal.ANSWER, "request") if case == "source" else
                                         (terminal.BIRCH_CONTINUE, terminal.BIRCH_ANSWER, "workspace_request"))
                pty.paste(prompt, prompt)
                frame("answered", lambda lines: any(answer in line for line in lines), b"\r")
                frames["turn"] = terminal.wait_for_turn_completion(root, gateway, pty.process, answer, deadline,
                                                                   event_kind=event, rollout_path=files[case])
            pty.quit()

        if case == "source":
            time.sleep(max(0, earliest_continuation - time.monotonic()))
        before = {owner: terminal.CHECK["sha256"](path) for owner, path in files.items()}
        drive_pty([str(binary), "resume"], root, env, label, interact)
        fixture.require(set(sessions.rglob("rollout-*.jsonl")) == set(files.values()), "picker changed the rollout set")
        changed = [owner for owner, path in files.items() if terminal.CHECK["sha256"](path) != before[owner]]
        fixture.require(changed == ([] if case == "current" else [case]), "picker changed an unexpected rollout")
        frames["changed_rollouts"] = changed
        frames["raw_sha256"] = terminal.CHECK["sha256"](root / (label + ".raw"))
        frames["custody_sha256"] = terminal.CHECK["sha256"](root / (label + "-custody.json"))
    return results
