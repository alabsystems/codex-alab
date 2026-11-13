#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise concurrent native exec sessions against one shared aterm account pool."""
import argparse
from contextlib import ExitStack
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import threading
import time

import capacity_exec as probe
import exec_accounts
import native_host_record
import renewal_follow as native


class Cohort(exec_accounts.Model):
    def __init__(self, root, count, canary_delay=0, chain=False):
        super().__init__(root, chain=chain)
        self.prompts = [f"{probe.PROMPT} [cohort:{i}]" for i in range(count)]
        self.by_worker = [[] for _ in range(count)]
        self.waiting = set()
        self.all_waiting = threading.Event()
        self.canary_delay = canary_delay
        self.canary_started = False

    def reply(self, body, generation=None):
        if native.request_kind(body) != "foreground":
            return super().reply(body, generation)
        texts = [part.get("text") for item in body.get("input", [])
                 if isinstance(item, dict) and item.get("role") == "user"
                 for part in item.get("content", []) if isinstance(part, dict)]
        matched = [i for i, prompt in enumerate(self.prompts) if texts.count(prompt) == 1]
        probe.require(len(matched) == 1, "request lost or mixed the original cohort input")
        index = matched[0]
        with self.lock:
            record = {"kind": "foreground", "worker": index, "body": body}
            self.requests.append(record)
            self.by_worker[index].append(record)
            number = len(self.by_worker[index])
            delay = number == 3 and not self.canary_started
            if delay:
                self.canary_started = True
            if number == 2:
                self.waiting.add(index)
                if len(self.waiting) == len(self.prompts):
                    self.all_waiting.set()
        expected = ("new", "next") if self.chain and number == 3 else (
            ("next",) if self.chain and number == 4 else ("new" if number == 3 else "old",))
        probe.require(generation in expected,
                      "cohort request used an unexpected account")
        if number == 2:
            probe.require(self.all_waiting.wait(60), "cohort never reached concurrent quota requests")
            return 429, json.dumps({"error": {"type": "usage_limit_reached",
                "resets_at": int(time.time()) + 86400,
                "message": "Shared fixture weekly quota exhausted"}}).encode(), "application/json", None, record
        if self.chain and generation == "new":
            with self.lock:
                self.middle_exhausted = True
            return 429, json.dumps({"error": {"type": "usage_limit_reached",
                "resets_at": int(time.time()) + 86400,
                "message": "Shared target exhausted before followers moved"}}).encode(), "application/json", None, record
        probe.require(number in ((1, 3, 4) if self.chain else (1, 3)), "cohort repeated model work")
        if number == 1:
            item = {"type": "function_call", "call_id": probe.CALL_ID, "name": "update_plan",
                    "arguments": json.dumps({"plan": [{"step": probe.PLAN, "status": "completed"}]})}
        else:
            if delay:
                time.sleep(self.canary_delay)
            item = {"type": "message", "role": "assistant", "id": f"cohort-answer-{index}",
                    "content": [{"type": "output_text", "text": f"Shared pool worker {index} recovered."}]}
        return 200, probe.response(f"cohort-{index}-{number}", item), "text/event-stream", None, record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True, type=Path)
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--sessions", type=int, default=4)
    parser.add_argument("--chain", action="store_true",
                        help="Exhaust the first target while other sessions await its canary")
    parser.add_argument("--late-prepare", action="store_true",
                        help="Hold one selected account until the chain has moved to the final target")
    parser.add_argument("--completion-timeout", type=int, default=180,
                        help="Seconds to wait for all sessions, plus the optional canary delay")
    parser.add_argument("--canary-delay", type=float, default=0,
                        help="Delay the first target response; values over 120 exercise reservation expiry")
    args = parser.parse_args()
    probe.require(args.sessions >= 2, "concurrency requires at least two sessions")
    probe.require(0 <= args.canary_delay <= 300, "canary delay must be between 0 and 300 seconds")
    probe.require(1 <= args.completion_timeout <= 600, "completion timeout must be between 1 and 600 seconds")
    probe.require(not (args.chain and args.canary_delay), "chain and delayed canary are distinct cases")
    probe.require(not args.late_prepare or args.chain, "late preparation requires a target-changing chain")
    build, _, codex = probe.package_records(args.package.resolve(strict=True))
    host = args.host.resolve(strict=True)
    host_record = native_host_record.load(host)
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    common = probe.fresh_environment(root)
    common.update(ATERM_HARNESS_STATE=str(root / "host-state"), XDG_CONFIG_HOME=str(root / "config"))
    model = Cohort(root, args.sessions, args.canary_delay, args.chain)
    server, server_thread = native.serve(model)
    report = {"passed": False, "source": build["source"],
              # The record's attestation travels with its source: a trial host's commit is
              # not a qualified one.
              "nativeHost": native_host_record.summary(host_record),
              "sessions": args.sessions, "canaryDelay": args.canary_delay, "chain": args.chain,
              "completionBudgetSeconds": args.completion_timeout, "latePrepare": args.late_prepare,
              "sharedPool": str(root / "host-state")}
    processes = []
    workers = []
    try:
        issuer = f"https://127.0.0.1:{server.server_port}"
        old, new = root / "auth-old/auth.json", root / "auth-new/auth.json"
        native.fake_auth(old, "old", issuer, "source-account")
        native.fake_auth(new, "new", issuer, "target-account", email="target@example.invalid", user="target-user")
        final_auth = root / "auth-next/auth.json" if args.chain else new
        if args.chain:
            native.fake_auth(final_auth, "next", issuer, "final-account", email="final@example.invalid", user="final-user")
        settings = root / "config/aterm"
        settings.mkdir(parents=True)
        (settings / "aterm.toml").write_text("[harness]\naccounts='early'\nrelaunch=true\nresume_limits=true\nhuman_grace_s=0\n")
        roster = (
            "[[account]]\nlabel='main'\nclass='dedicated'\ndir=" + json.dumps(str(old.parent))
            + "\nemail='fixture@example.invalid'\n\n[[account]]\nlabel='target'\nclass='dedicated'\ndir="
            + json.dumps(str(new.parent)) + "\nemail='target@example.invalid'\n")
        if args.chain:
            roster += ("\n[[account]]\nlabel='final'\nclass='dedicated'\ndir=" + json.dumps(str(final_auth.parent))
                       + "\nemail='final@example.invalid'\n")
        (settings / "accounts-codex.toml").write_text(roster)
        with ExitStack() as files:
            for index in range(args.sessions):
                worker = root / f"worker-{index}"
                worker.mkdir()
                workers.append(worker)
                if args.late_prepare:
                    (worker / "pause-prepare-ack").touch()
                env = probe.fresh_environment(worker)
                env.update({key: common[key] for key in ("HOME", "ATERM_HARNESS_STATE", "XDG_CONFIG_HOME")})
                probe.configure(worker, server.server_port)
                config_file = worker / "codex-home/config.toml"
                config = config_file.read_text().replace("requires_openai_auth = false", "requires_openai_auth = true")
                config = config.replace('name = "local capacity smoke"', 'name = "OpenAI"')
                config = config.replace("http://127.0.0.1:", "https://127.0.0.1:")
                config = config.replace("[tools.update_plan]", "[features]\nenable_request_compression = false\n[tools.update_plan]")
                config_file.write_text(f'cli_auth_credentials_store = "file"\nchatgpt_base_url = "{issuer}"\n' + config)
                shutil.copyfile(root / "fixture-ca.der", worker / "fixture-ca.der")
                fixture = {"root": str(worker), "shared_pool_root": str(root), "codex": str(codex),
                    "cwd": str(worker / "work"), "auth_path": str(old), "successor_auth_path": str(new),
                    "weekly_quota": True, "human_grace_s": 0,
                    "args": ["exec", "--json", "--color", "never", "--skip-git-repo-check",
                             "--dangerously-bypass-approvals-and-sandbox", model.prompts[index]],
                    "env": {"CODEX_HOME": env["CODEX_HOME"], "PATH": env["PATH"],
                            "XDG_CONFIG_HOME": env["XDG_CONFIG_HOME"], "NO_COLOR": "1",
                            "CODEX_CA_CERTIFICATE": str(root / "fixture-ca.pem")}}
                if args.chain:
                    fixture["additional_auth_paths"] = [str(final_auth)]
                probe.write_json(worker / "fixture.json", fixture)
                command = [str(host), "--renewal-config", str(worker / "fixture.json")]
                guarded = [sys.executable, "-I", str(probe.GUARD_PATH), "--", sys.executable, "-I",
                           str(probe.GUARD_PATH), "--result-file", str(worker / "guardian.json"),
                           "--inherited-group-timeout", str(args.completion_timeout + 20 + int(args.canary_delay)), "--", *command]
                processes.append(subprocess.Popen(guarded, cwd=worker / "work", env=env,
                    stdin=subprocess.DEVNULL, stdout=files.enter_context((worker / "stdout.jsonl").open("xb")),
                    stderr=files.enter_context((worker / "stderr.log").open("xb"))))
            deadline = time.monotonic() + args.completion_timeout + args.canary_delay
            exit_codes = [None] * args.sessions
            while time.monotonic() < deadline:
                # A reply the fixture could not build would keep every walled turn polling.
                native.require_no_fixture_fault(model)
                if args.late_prepare and all((worker / "prepare-ack-paused").is_file() for worker in workers):
                    for worker in workers[1:]:
                        (worker / "continue-prepare-ack").touch(exist_ok=True)
                    state_path = root / "host-state/accounts-codex/state.json"
                    if state_path.is_file():
                        state = json.loads(state_path.read_text())
                        used = (state.get("accounts", {}).get("target", {}).get("sample") or {}).get("week_pct")
                        if used == 100 and not (workers[0] / "continue-prepare-ack").exists():
                            probe.write_json(root / "late-release.json", {"targetUsedPercent": used})
                            (workers[0] / "continue-prepare-ack").touch()
                complete = []
                for index, (worker, process) in enumerate(zip(workers, processes)):
                    log = worker / "pty.log"
                    lines = log.read_text(errors="replace").splitlines() if log.exists() else []
                    codes = [int(match.group(1)) for line in lines
                             if (match := re.fullmatch(r"EXEC_FIXTURE_EXIT=(\d+)", line.strip()))]
                    probe.require(len(codes) <= 1, "cohort emitted multiple exit statuses")
                    if codes:
                        exit_codes[index] = codes[0]
                    complete.append(bool(codes))
                    probe.require(codes or process.poll() is None, "native cohort host exited before completion")
                if all(complete):
                    break
                time.sleep(.1)
            else:
                raise RuntimeError("shared-pool cohort did not finish")
            report["exitCodes"] = exit_codes
            probe.require(all(code == 0 for code in exit_codes), "cohort exec failed")
            for worker in workers:
                (worker / "stop").touch()
            for process in processes:
                probe.require(process.wait(timeout=60) == 0, "native cohort host failed")
        # Before any verdict: a fault after the loop's last pass explains what follows it.
        native.require_no_fixture_fault(model)
        probe.require(model.all_waiting.is_set(), "cohort did not overlap at the quota boundary")
        if args.late_prepare:
            probe.require(all((worker / "prepare-ack-paused").is_file() for worker in workers)
                          and (workers[0] / "continue-prepare-ack").is_file(),
                          "late selected-account reply was not released after the target changed")
        probe.require({"source", "target"}.issubset(model.usage_reads),
                      "shared pool did not verify source and destination quota")
        receipts = [json.loads(path.read_text()) for path in
                    (root / "host-state/accounts-codex/renewal-follow").glob("*.json")]
        consumed = [receipt for receipt in receipts if receipt.get("phase") == "consumed"]
        probe.require(len(consumed) == args.sessions + int(args.chain), "shared store lost or duplicated a session receipt")
        expired = [receipt for receipt in receipts if receipt.get("phase") == "retired"
                   and not receipt.get("launch_attempted") and receipt.get("revoked")]
        if args.canary_delay > 120:
            probe.require(len(expired) >= args.sessions - 1,
                          "slow canary did not exercise waiting reservation expiry")
        pool_state = json.loads((root / "host-state/accounts-codex/state.json").read_text())
        probe.require(pool_state.get("active") == ("final" if args.chain else "target") and pool_state.get("proven") is True
                      and pool_state.get("canary") is None, "completed turns did not prove the shared target")
        probe.require(len(pool_state.get("rotations", [])) == 1 + int(args.chain),
                      "shared-pool followers charged another rotation")
        pids = set()
        turns = set()
        for index, worker in enumerate(workers):
            guardian = json.loads((worker / "guardian.json").read_text())
            probe.require(guardian["returncode"] == 0 and not guardian["timed_out"], "guardian did not prove clean exit")
            metadata = json.loads((worker / "stdout.jsonl").read_text().splitlines()[0])
            actions = [json.loads(line) for line in (worker / "native-actions.jsonl").read_text().splitlines()]
            if args.late_prepare and index == 0:
                probe.require(any(item.get("action") == "exec-arm" and item.get("replyOk") is False
                                  for item in actions), "late preparation never reached stale-arm rejection")
            probe.require(any(item.get("action") == "exec-reserve" and item.get("replyOk") is True
                              for item in actions), "session did not reserve through the native host")
            probe.require(any(item.get("action") == "exec-complete" and item.get("replyOk") is True
                              for item in actions), "session did not report successful native completion")
            pid = metadata["child_pid"]
            matching = [item for item in consumed if item["old_pid"] == pid and item["new_auth"] == str(final_auth)]
            probe.require(len(matching) == 1 and matching[0]["successor_pid"] == pid, "receipt changed session process identity")
            probe.require(matching[0]["old_birth"] == matching[0]["successor_birth"], "receipt changed process birth")
            pids.add(pid)
            turns.add((matching[0]["intent"]["thread"], matching[0]["intent"]["wall"]))
            events = []
            for line in (worker / "pty.log").read_text(errors="replace").splitlines():
                try:
                    event = json.loads(line)
                except ValueError:
                    continue
                if isinstance(event, dict) and "type" in event:
                    events.append(event)
            kinds = [event["type"] for event in events]
            thread_ids = [event.get("thread_id") for event in events if event["type"] == "thread.started"]
            probe.require(thread_ids == [matching[0]["intent"]["thread"]], "receipt named another session's thread")
            probe.require(kinds.count("turn.completed") == 1 and kinds.count("turn.started") == 1,
                          "cohort session lost its original turn boundary")
            probe.require(not exec_accounts.stream_problems(events), "cohort corrupted JSON output")
            answers = [event.get("item", {}).get("text") for event in events
                       if event["type"] == "item.completed" and event.get("item", {}).get("type") == "agent_message"]
            probe.require(answers == [f"Shared pool worker {index} recovered."], "cohort mixed or repeated final answers")
            requests = model.by_worker[index]
            probe.require(len(requests) in ((3, 4) if args.chain else (3,)) and requests[-1].get("newTokenMatched"),
                          "cohort failed to resume on the shared target")
            outputs = [item for item in requests[-1]["body"]["input"] if item.get("type") == "function_call_output"]
            probe.require(len(outputs) == 1 and outputs[0].get("call_id") == probe.CALL_ID, "cohort lost or replayed completed work")
        probe.require(len(pids) == len(turns) == args.sessions, "cohort conflated independent sessions")
        report.update(passed=True, concurrentQuotaBoundary=True, consumedReceipts=len(consumed),
                      retiredUnexecutedReservations=len(expired),
                      sharedRotations=len(pool_state["rotations"]),
                      distinctProcesses=len(pids), distinctTurns=len(turns), sharedTargetProven=True)
    except BaseException as error:
        report["failure"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        for worker in workers:
            (worker / "stop").touch(exist_ok=True)
        for process in processes:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=60)
        server.shutdown()
        server_thread.join()
        server.server_close()
        probe.write_json(root / "result.json", report)
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
