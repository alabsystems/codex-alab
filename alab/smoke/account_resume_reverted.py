# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Resume a real paginated thread after revert and loss of its local index row."""

import argparse
from contextlib import closing
import json
from pathlib import Path
import sqlite3
import sys

import account_resume as account
from account_resume_restored import index_state, observe_picker
import capacity_exec as fixture
import model_resume as terminal
from quota_resume import Rpc


DISCARD = "Discard the saffron sentinel turn."
DISCARD_REPLY = "This saffron answer must also be reverted."


class Gateway(account.Gateway):
    def response(self, body, headers):
        users = terminal.messages(body, "user")
        assistants = terminal.messages(body, "assistant")
        model_input = json.dumps(body["input"])
        with self.lock:
            login, workspace = self.account_route(headers)
            self.requests.append({
                "thread": headers.get("thread-id"),
                "turn": json.loads(headers["x-codex-turn-metadata"])["turn_id"],
                "login": login, "account": workspace,
                "users": [text for text in users if text in (terminal.SEED, DISCARD, terminal.BIRCH_SEED, terminal.CONTINUE)],
                "answers": [text for text in assistants if text in (terminal.REPLY, DISCARD_REPLY, terminal.BIRCH_REPLY)],
                "discarded_context": any(marker in model_input for marker in (DISCARD, DISCARD_REPLY)),
            })
        if users[-1:] == [DISCARD]:
            return fixture.response("discard", {"type": "message", "id": "discard", "role": "assistant",
                                                "content": [{"type": "output_text", "text": DISCARD_REPLY}]})
        return terminal.Gateway.response(self, body, headers)


def check_requests(report):
    a, b = report["source"], report["sibling"]
    continued, = report["completed"]
    expected = [
        (a, report["kept"], "source", [terminal.SEED], [], False),
        (a, report["reverted"], "source", [terminal.SEED, DISCARD], [terminal.REPLY], True),
        (b, report["sibling_turn"], "destination", [terminal.BIRCH_SEED], [], False),
        (a, continued, "destination", [terminal.SEED, terminal.CONTINUE], [terminal.REPLY], False),
    ]
    fixture.require(a != b and len({r[1] for r in expected}) == 4, "thread or turn identities were reused")
    actual = [(r["thread"], r["turn"], r["login"], r["users"], r["answers"], r["discarded_context"])
              for r in report["requests"]]
    fixture.require(actual == expected and all(r["account"] == "workspace-" + r["login"] for r in report["requests"]),
                    "continued request lost surviving history, replayed discarded content or used another login")


def exercise(binary, root, env, gateway, report, port, keep_index):
    home, work, other = root / "codex-home", root / "work", root / "second-project"
    database = home / "state_5.sqlite"
    other.mkdir()
    terminal.configure(root, port, "ultra", tls=True)
    config = home / "config.toml"
    config.write_text('cli_auth_credentials_store = "file"\n' + config.read_text().replace(
        "features.daemon_auto_start = false\n", "") + f'\n[projects.{json.dumps(str(other))}]\ntrust_level = "trusted"\n')

    def start(login, cwd, prompt):
        account.select_login(root, gateway, login, f"https://127.0.0.1:{port}")

        def seed(process, stdout):
            rpc = Rpc(process, stdout)
            rpc.initialize()
            thread = rpc.request("thread/start", {"cwd": str(cwd), "historyMode": "paginated"})["thread"]["id"]
            kept = rpc.turn(thread, prompt)
            fixture.require(kept["status"] == "completed", "seed turn did not finish")
            if login == "destination":
                report.update(sibling=thread, sibling_turn=kept["id"])
                return
            discarded = rpc.turn(thread, DISCARD)
            fixture.require(discarded["status"] == "completed", "sentinel turn did not finish")
            revert = rpc.request("thread/revert", {"threadId": thread, "beforeTurnId": discarded["id"]})
            fixture.require(revert["thread"]["id"] == thread and revert["itemsBackwardsCursor"],
                            "revert did not retain a paginated history")
            report.update(source=thread, kept=kept["id"], reverted=discarded["id"])

        status = fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, "seed-" + login,
                             cwd=cwd, interact=seed)
        fixture.require(status == 0, "seed app-server did not exit cleanly")

    start("source", work, terminal.SEED)
    start("destination", other, terminal.BIRCH_SEED)
    a, b = report["source"], report["sibling"]
    fixture.require(database.resolve(strict=True) == home / "state_5.sqlite", "unexpected fixture database")

    def index():
        index_state(database)
        with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as db:
            return {row[0]: row[1:] for row in db.execute(
                "SELECT id, rollout_path, preview, first_user_message, creator_account_id, cwd, history_mode FROM threads")}

    original = index()
    fixture.require(set(original) == {a, b} and original[a][1:] == (
        terminal.SEED, terminal.SEED, "workspace-source", str(work), "paginated")
        and original[b][1:] == (terminal.BIRCH_SEED, terminal.BIRCH_SEED, "workspace-destination", str(other), "paginated"),
        "seed index does not describe the two expected local projects")
    active = Path(original[a][0]).resolve(strict=True)
    fixture.require(active.is_relative_to(home / "sessions"), "reverted branch escaped the fixture")
    rows = [json.loads(line) for line in active.read_text().splitlines()]
    meta = rows[0]["payload"]
    fixture.require(rows[0]["type"] == "session_meta" and meta["id"] == a and meta["cwd"] == str(work)
                    and meta["history_base"]["thread_id"] == a and meta["history_base"]["end_ordinal_exclusive"] > 0
                    and not any(row["type"] == "response_item" for row in rows),
                    "active branch must inherit its surviving conversation from the earlier rollout")
    report.update(active_rollout=str(active.relative_to(home)), history_base=meta["history_base"])
    ancestor, = (home / "sessions").rglob(f"rollout-*-{a}.jsonl")
    previous = ancestor.read_bytes()
    boundary = meta["history_base"]["end_byte_offset"]
    surviving, removed = previous[:boundary], previous[boundary:]
    fixture.require(len(surviving.splitlines()) == meta["history_base"]["end_ordinal_exclusive"]
                    and terminal.SEED.encode() in surviving and terminal.REPLY.encode() in surviving
                    and all(text.encode() in removed and text.encode() not in surviving for text in (DISCARD, DISCARD_REPLY)),
                    "revert boundary did not separate retained and discarded turns")

    def files():
        return {str(p.relative_to(home)): terminal.CHECK["sha256"](p) for p in (home / "sessions").rglob("rollout-*.jsonl")}

    before = report["rollouts_before"] = files()
    fixture.require(len(before) == 3, "expected two A rollouts and B's independent rollout")

    def prepare():
        fixture.require(index() == original, "discovery changed an owner, working directory, summary or active branch")
        if not keep_index:
            with closing(sqlite3.connect(database.as_uri() + "?mode=rw", uri=True)) as db, db:
                db.execute("PRAGMA foreign_keys = ON")
                fixture.require(db.execute("DELETE FROM threads WHERE id = ?", (a,)).rowcount == 1,
                                "could not remove the source index row")
            fixture.require(index() == {b: original[b]}, "source removal changed the destination index")
        fixture.require(files() == before, "the index preparation changed the rollout files")

    prepare()
    report["first_picker"] = observe_picker(binary, root, env, gateway, "reverted-picker")
    report["discovered"] = index()
    fixture.require(sorted(report["first_picker"]["rows"]) == ["birch", "cedar"],
                    "bare resume omitted A's surviving conversation after thread/revert")
    prepare()  # The continuing picker must independently recover the same missing row.
    with gateway.lock:
        fixture.require(len(gateway.requests) == 3, "opening the picker sent a model request")
        gateway.resumed = True
    completed = report["completed"] = []
    custody, _, _ = terminal.terminal_session(binary, root, env, "bare-all-default", ["resume"], gateway, completed,
                                              cwd=other, completed_rollout=active)
    terminal.CHECK["check_model_terminal_exit"](custody, [str(binary), "resume"])
    after = report["rollouts_after"] = files()
    fixture.require(after.keys() == before.keys() and {p for p in before if before[p] != after[p]} == {report["active_rollout"]},
                    "continuation did not update only the active reverted rollout")
    fixture.require(index()[a][0] == str(active), "the continuing thread changed its active rollout")


def run(binary, root, *, keep_index=False):
    root.mkdir(mode=0o700)
    env = fixture.fresh_environment(root)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm", HTTP_PROXY="http://127.0.0.1:1",
               HTTPS_PROXY="http://127.0.0.1:1", NO_PROXY="localhost,127.0.0.1,::1",
               CODEX_CA_CERTIFICATE=str(root / "fixture-ca.pem"))
    report = {"passed": False, "binary_sha256": terminal.CHECK["sha256"](binary), "intact_index": keep_index,
              "events": [], "requests": []}
    gateway = Gateway(report)

    def secure(server):
        server.socket = account.logins.fixture_tls(root).wrap_socket(server.socket, server_side=True,
                                                                     do_handshake_on_connect=False)

    try:
        server, worker = fixture.start_listener(gateway.handler(), bind=account.AccountResumeServer, prepare=secure)
        try:
            exercise(binary, root, env, gateway, report, server.server_port, keep_index)
        finally:
            fixture.cleanup_resources((("gateway", server, worker),), (), report, sys.exc_info()[1])
        fixture.require(not report.get("cleanupErrors"), "gateway did not close cleanly")
        check_requests(report)
        report["passed"] = True
    except Exception as error:
        report["error"] = str(error)
        print(error, file=sys.stderr)
    finally:
        fixture.write_json(root / "result.json", report)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--intact-index", action="store_true", help="Control with A's correct index row still present.")
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), args.output.resolve(), keep_index=args.intact_index))
