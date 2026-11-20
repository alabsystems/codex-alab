# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Search a bare picker's two pages for identical titles under different local logins."""

import argparse
from functools import partial
import hashlib
import json
from pathlib import Path
import secrets
import sys
import time

import account_resume as account
from account_resume_moved import histories
import account_resume_page as page
import account_resume_titles as titles
import account_resume_writer as writer
import capacity_exec as fixture
import model_resume as terminal
from quota_resume import Rpc
from terminal_controls import drive_pty


def check_boundary(current, first, later, a_ids, a, b, a_cwd, b_cwd):
    pages = page.check_pages(a_ids, b, b_cwd, a_cwd, current, first, later)
    rows = first["data"] + later["data"]
    named = [(pos, row["id"]) for pos, row in enumerate(rows, 1) if row["name"] == titles.TITLE]
    fixture.require(named == [(page.PAGE_SIZE, a), (page.PAGE_SIZE + 1, b)],
                    "the two same-named originals must be on opposite sides of the first cursor")
    return {"a_current_ids": pages["current_login_ids"], "first": pages["first_page_ids"],
            "cursor": pages["next_cursor"], "second": pages["second_page_ids"], "named": named}


def exercise(binary, seed_binary, root, port, gateway, report):
    env, slots = account.configure_accounts(root, port, gateway)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm")
    home, work, other = root / "codex-home", root / "work", root / "other-project"
    other.mkdir()
    config = home / "config.toml"
    config.write_text("thread_unload_delay_secs = 0\n" + config.read_text()
                      + f'\n[projects.{json.dumps(str(other))}]\ntrust_level = "trusted"\n')
    marker = "cedar-" + secrets.token_hex(12)
    (work / "cedar-marker.txt").write_text(marker + "\n")
    (other / "cedar-marker.txt").write_text("other-project\n")

    if report["shared_seed_auth"]:
        slots["destination"].replace(home / "auth.json")

    def select_login(login):
        if login == "destination" and report["shared_seed_auth"]:
            env.pop("CODEX_AUTH_PATH", None)
        else:
            env["CODEX_AUTH_PATH"] = str(slots[login])

    def seed(cli, login, cwd, prompt, answer):
        select_login(login)
        return account.exec_turn(cli, ["exec", "--json", "--skip-git-repo-check", prompt],
                                 root, env, login, answer, cwd=cwd)

    b, b_seed = seed(seed_binary, "destination", work, terminal.SEED, terminal.REPLY)
    titles.name_thread(binary, root, env, "destination", b)
    time.sleep(1.1)  # thread/list exposes whole-second timestamps.
    a, a_seed = seed(binary, "source", other, terminal.BIRCH_SEED, terminal.BIRCH_REPLY)
    titles.name_thread(binary, root, env, "source", a, cwd=other)
    time.sleep(1.1)

    def setup(process, output):
        rpc = Rpc(process, output)
        rpc.initialize()
        a_ids = page.fork_page(rpc, a, b)
        # Forks inherit their parent's title. Give only the two independent originals the search title.
        for index, thread in enumerate(sorted(a_ids - {a}), 1):
            rpc.request("thread/name/set", {"threadId": thread, "name": f"Remember the birch {index}"})
        report["boundary"] = check_boundary(*page.list_pages(rpc), a_ids, a, b, other, work)

    fixture.require(fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, "pages",
                                interact=setup, cwd=other) == 0, "could not prepare two authenticated pages")
    select_login("destination")

    def confirm_b(process, output):
        rpc = Rpc(process, output)
        rpc.initialize()
        current, first, later = page.list_pages(rpc)
        fixture.require([row["id"] for row in current["data"]] == [b] and not current["nextCursor"]
                        and [row["id"] for row in first["data"]] == report["boundary"]["first"]
                        and [row["id"] for row in later["data"]] == [b] and not later["nextCursor"],
                        "B's current scope or either all-local page disagrees with the seeded owners")
        report["boundary"]["b_current_ids"] = [b]

    fixture.require(fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, "b-pages",
                                interact=confirm_b) == 0, "B's catalog check did not finish")
    select_login(report["resume_login"])
    with gateway.lock:
        gateway.resumed = True
        gateway.verify_resume_directory = True
        reads_before = len(gateway.reads)
        fixture.require(len(gateway.requests) == 3, "expected only the two seed turns and B's tool result")
    previous = histories(home)
    rollout, = [path for path in previous if path.name.endswith(f"-{b}.jsonl")]
    fixture.require(len(previous) == page.PAGE_SIZE + 1, "expected a unique rollout for every listed thread")
    frames = report["frames"] = {}

    def interact(pty):
        deadline = time.monotonic() + 55
        frame = partial(titles.record_frame, pty, frames, deadline, "paged duplicate titles")
        frame("first-page", lambda lines: page.position_from_lines(lines) == (1, page.PAGE_SIZE, True, False))

        def matched(lines, index, *, expanded=False):
            return (any(line.strip() == "Search: " + titles.TITLE for line in lines)
                    and titles.selection(lines, "All local", index, 2, expanded=expanded))

        frame("search", lambda lines: matched(lines, 1), titles.TITLE.encode())
        frame("row-25", lambda lines: matched(lines, 1, expanded=True)
              and titles.details(lines, "All local", 1, 2, a, [terminal.BIRCH_REPLY], [terminal.REPLY]), b"\x05")
        frame("collapsed", lambda lines: matched(lines, 1), b"\x05")
        frame("second-match", lambda lines: matched(lines, 2), b"\x1b[B")
        frame("row-26", lambda lines: matched(lines, 2, expanded=True)
              and titles.details(lines, "All local", 2, 2, b, [terminal.REPLY], [terminal.BIRCH_REPLY]), b"\x05")
        frame("directory", lambda lines: any("› 1. Use session directory" in line for line in lines), b"\r")
        frame("resumed", lambda lines: any(terminal.REPLY in line for line in lines)
              and not any("Resume a previous session" in line for line in lines), b"\r")
        with gateway.lock:
            fixture.require(len(gateway.requests) == 3, "picker navigation or selection sent an inference request")
        pty.paste(terminal.CONTINUE, terminal.CONTINUE)
        pty.send(b"\r")
        pty.displayed(terminal.ANSWER)
        report["continued_turn"] = terminal.wait_for_turn_completion(
            root, gateway, pty.process, terminal.ANSWER, deadline, rollout_path=rollout)
        pty.quit()

    drive_pty([str(binary), "resume"], root, env, "bare-picker", interact, cwd=other)
    after = histories(home)
    fixture.require(after.keys() == previous.keys() and after[rollout].startswith(previous[rollout])
                    and len(after[rollout]) > len(previous[rollout])
                    and [path for path in after if after[path] != previous[path]] == [rollout],
                    "continuation must append exclusively to B's original rollout")
    report.update(a=a, b=b, a_seed_turn=a_seed, b_seed_turn=b_seed, unchanged_siblings=len(previous) - 1,
                  preserved_bytes=len(previous[rollout]), reads_before_picker=reads_before)
    for label, snapshot in (("before", previous), ("after", after)):
        report["rollouts_" + label] = {str(path.relative_to(home)): hashlib.sha256(data).hexdigest()
                                       for path, data in snapshot.items()}
    report["pty_sha256"] = terminal.CHECK["sha256"](root / "bare-picker.raw")
    report["custody_sha256"] = terminal.CHECK["sha256"](root / "bare-picker-custody.json")
    return writer.saved_conversation(previous[rollout], work), marker


def check_requests(report, root, saved, marker):
    first, result, sibling, resumed, followup = report["requests"]
    b, a, kept, separate, continued = (report[key] for key in ("b", "a", "b_seed_turn", "a_seed_turn", "continued_turn"))
    login = report["resume_login"]
    expected = [(b, kept, "destination"), (b, kept, "destination"), (a, separate, "source"),
                (b, continued, login), (b, continued, login)]
    fixture.require(len({kept, separate, continued}) == 3 and a != b and
                    [(r["thread"], r["turn"], r["login"], r["account"]) for r in report["requests"]] ==
                    [(thread, turn, login, "workspace-" + login) for thread, turn, login in expected],
                    "model requests changed a thread, turn, bearer or workspace")
    reads = report["account_reads"][report["reads_before_picker"]:]
    fixture.require({r["path"] for r in reads} == {"/v1/models", "/backend-api/wham/accounts/check"}
                    and all((r["login"], r["account"]) == (login, "workspace-" + login) for r in reads),
                    "the picker used the wrong login for its catalog or account reads")
    writer.check_conversation_input(saved, first, result, resumed, followup)
    call, output = result["tool_records"]
    fixture.require(sibling["history"] == [terminal.BIRCH_SEED] and not sibling["tool_records"]
                    and result["tool_records"] == resumed["tool_records"]
                    and all(call.get(key) == value for key, value in account.TOOL_CALL.items())
                    and output.get("type") == "function_call_output" and output.get("call_id") == call["call_id"]
                    and marker in output["output"]
                    and (root / "work/cedar-calls").read_text().splitlines() == [marker],
                    "the original seed command or the other conversation changed")
    report["resumed_cwd"] = account.check_resumed_directory(root / "work", root / "other-project", marker, resumed, followup)


def run(binary, seed_binary, root, resume_as, shared_seed_auth=False):
    root.mkdir(mode=0o700)
    digest = terminal.CHECK["sha256"]
    report = {"passed": False, "events": [], "requests": [], "binary_sha256": digest(binary),
              "seed_binary_sha256": digest(seed_binary), "shared_seed_auth": shared_seed_auth,
              "resume_login": {"a": "source", "b": "destination"}[resume_as]}
    gateway = account.Gateway(report)
    gateway.capture_conversation_input = True
    try:
        server, worker = account.start_gateway(root, gateway)
        try:
            saved, marker = exercise(binary, seed_binary, root, server.server_port, gateway, report)
        finally:
            fixture.cleanup_resources((("gateway", server, worker),), (), report, sys.exc_info()[1])
        fixture.require(not report.get("cleanupErrors"), "gateway did not close cleanly")
        check_requests(report, root, saved, marker)
        fixture.require((digest(binary), digest(seed_binary)) == (report["binary_sha256"], report["seed_binary_sha256"]),
                        "a fixture executable changed")
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
    parser.add_argument("--seed-codex", type=Path, help="Older CLI used to create B's original conversation.")
    parser.add_argument("--shared-seed-auth", action="store_true",
                        help="Use the standard auth.json for B, including when seeding with OpenAI Codex.")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--resume-as", choices=("a", "b"), default="a",
                        help="Select B’s older thread as A (default) or under its original login, B.")
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), (args.seed_codex or args.codex).resolve(strict=True),
                 args.output.resolve(), args.resume_as, args.shared_seed_auth))
