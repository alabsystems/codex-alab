#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Observe reset, history, model resume and capacity recovery in the newly built Codex."""

import argparse
import json
import math
import os
from pathlib import Path
import re
import runpy
import shutil
import stat
import subprocess
import sys
import tempfile


# All entries run against the actual requested executable on every maintained
# gate. Counts are model requests, consumption POSTs, and successful app exit.
# Keep policy outcomes explicit instead of inheriting the probe's categories.
EXPECTATIONS = {
    "no-credit": (2, 0, False),
    "existing-usage-credits": (3, 0, True),
    "paid-retry-refused": (3, 0, False),
    "spend-control-reopens": (3, 0, True),
    "spend-control-blocked": (2, 0, False),
    "plan-renewed-before-credit-retry": (3, 0, True),
    "preflight-five-hour": (2, 0, False),
    "preflight-below-99": (2, 0, False),
    "preflight-weekly": (2, 1, True),
    "recovery-weekly-25h": (3, 1, True),
    "preflight-weekly-24h": (2, 0, False),
    "preflight-weekly-25h": (2, 1, True),
    "preflight-weekly-48h": (2, 1, True),
    "recovery-weekly-48h": (3, 1, True),
    "recovery-weekly-49h": (3, 1, True),
    "preflight-elapsed-reset": (2, 0, False),
    "prepost-recovery-restored": (3, 0, True),
    "prepost-recovery-shifted": (2, 0, False),
    "prepost-recovery-foreign": (2, 0, False),
    "prepost-recovery-below-99": (2, 0, False),
    "prepost-recovery-denied": (3, 1, True),
    "prepost-preflight-99": (2, 1, True),
}
# Expected counts of earned-reset and paid-credit notices, respectively.
NOTICE_COUNTS = {"no-credit": (0, 0), "existing-usage-credits": (0, 1), "paid-retry-refused": (0, 1),
                 "spend-control-blocked": (0, 1), "spend-control-reopens": (0, 1),
                 "plan-renewed-before-credit-retry": (1, 0)}
CASES = tuple(EXPECTATIONS)
# HTTP statuses in request order and Retry-After headers. None exercises local backoff.
CAPACITY_REQUESTS = {
    "overload": ((200, 503, 200), (2,)),
    "streamed-overload": ((200, 200, 200), (2,)),
    "late-stream-overload": ((200, 200, 200), (2,)),
    "tool-stream-overload": ((200, 200), (2,)),
    "prewarm-overload": ((503, 426, 200, 200), (2,)),
    "repeated-overload": ((200, 503, 503, 200), (2, 3)),
    "unadvised-overload": ((200, 503, 503, 503, 200), (None, None, 1)),
}
MODEL_SURFACES = {"terminal": "model-resume", "exec": "exec-model-resume",
                  "live": "live-model-resume", "live-terminal": "live-terminal-model-resume",
                  "loaded": "loaded-model-resume", "unloaded": "unloaded-model-resume",
                  "picker": "picker-model-resume", "terminal-last": "terminal-last-model-resume",
                  "exec-last": "exec-last-model-resume"}
SELECTOR_SURFACES = ("picker", "terminal-last", "exec-last")
TERMINAL_SURFACES = ("terminal", "live-terminal", "picker", "terminal-last")
MODEL_UUID = re.compile(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}")
BOOTSTRAP = ("import runpy,sys; from pathlib import Path; "
             "probe=Path(sys.argv.pop(1)); sys.path.insert(0,str(probe.parent)); "
             "runpy.run_path(str(probe),run_name='__main__')")
CLI = runpy.run_path(str(Path(__file__).resolve().with_name("cli_status.py")))
FILE_HASH = runpy.run_path(str(Path(__file__).resolve().with_name("file_hash.py")))


def sha256(path):
    return FILE_HASH["file_sha256"](path)


def check_capacity_exit(case_dir, executable):
    receipt = json.loads((case_dir / "exec-guardian-result.json").read_text())
    expected = [str(executable.resolve()), "exec", "--json", "--color", "never", "--skip-git-repo-check",
                "--cd", str(case_dir.resolve() / "work"), "--sandbox", "read-only",
                "Keep this headless capacity smoke task and its completed plan."]
    if (not isinstance(receipt, dict) or type(receipt.get("schema")) is not int or receipt["schema"] != 1
            or receipt.get("argv") != expected or receipt.get("timed_out") is not False
            or type(receipt.get("returncode")) is not int or receipt["returncode"] != 0):
        raise ValueError(f"guardian did not confirm a successful capacity exec for {case_dir.name}")


def check_capacity_answer(events, name):
    # The caller already checked that stdout contains one start and one completion.
    kinds = [event.get("type") for event in events]
    start = kinds.index("turn.started")
    end = kinds.index("turn.completed")
    answers = [(index, item.get("text")) for index, event in enumerate(events)
               if event.get("type") == "item.completed" and isinstance(item := event.get("item"), dict)
               and item.get("type") == "agent_message"]
    if (len(answers) != 1 or answers[0][1] != "Headless capacity recovery succeeded."
            or not start < answers[0][0] < end):
        raise ValueError(f"capacity exec output for {name} must contain one recovered answer in its original turn")
    return answers[0][0]


def terminal_model_selection(raw, live, *, after_reply=True, first=False):
    # Ignore window titles and
    # color, but keep cursor controls and line breaks from joining unrelated text.
    rendered = re.sub(rb"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)", b"", raw)
    rendered = re.sub(rb"\x1b\[[0-9;:]*m", b"", rendered)
    answer = b"Final cedar answer." if live else b"Cedar marker recalled."
    if after_reply:
        rendered = rendered.partition(answer)[2]
    partition = rendered.partition if first else rendered.rpartition
    _prefix, found, selection = partition("Read Only · ".encode())
    match = re.match(rb"([\w.-]+) ([\w-]+)(?:$|[\s\x1b])", selection) if found else None
    return match.groups() if match else None


def check_model_terminal_exit(custody, expected_command):
    if (not isinstance(custody, dict) or type(custody.get("exit")) is not int
            or custody.get("command") != expected_command or custody["exit"] != 0
            or custody.get("forced") is not False or custody.get("owning_reap") is not True
            or custody.get("group_absent_after_reap") is not True):
        raise ValueError("missing resumed provider request or normal terminal exit")


def check_exec_resume_workspaces(report, events, binary, seed_binary):
    # The seed in the other directory is newer. --all reaches it before the
    # workspace-scoped resume returns to the original conversation.
    ordered = [row for row in events if row.get("event") != "catalog"]
    if [row.get("event") for row in ordered] != [
            "request", "workspace_request", "original_exit", "workspace_request", "request"]:
        raise ValueError("missing ordered workspace resume requests")
    first, second = ordered[1], ordered[3]
    thread = first.get("thread")
    if (not isinstance(thread, str) or not MODEL_UUID.fullmatch(thread) or thread == report.get("thread")
            or second.get("thread") != thread):
        raise ValueError("workspace resume selected the wrong conversation")
    turns = [row.get("turn") for row in (ordered[0], first, second, ordered[-1])]
    if not all(isinstance(turn, str) and MODEL_UUID.fullmatch(turn) for turn in turns) or len(set(turns)) != 4:
        raise ValueError("workspace resume requires four distinct turns")
    for row, resumed in ((first, False), (second, True)):
        if (row.get("model") != "gateway-ochre" or row.get("effort") != "high"
                or row.get("seed_prompt") is not True or row.get("prior_reply") is not resumed
                or row.get("resume_prompt") is not resumed or row.get("mixed_history") is not False):
            raise ValueError("workspace resume changed the model, effort or history")
    commands = [
        ("workspace_seed", seed_binary, ["-C", "../birch", "Remember the birch in the second workspace."]),
        ("workspace_all", binary, ["resume", "--last", "--all", "Recall the second workspace."]),
    ]
    for key, executable, args in commands:
        process = report.get(key)
        if (not isinstance(process, dict) or process.get("argv") != [str(executable), "exec", "--skip-git-repo-check", *args]
                or type(process.get("returncode")) is not int or process["returncode"] != 0
                or process.get("timed_out") is not False):
            raise ValueError("workspace resume command did not complete")


def check_model_resume(report, surface, binary, terminal_output=None, *, seed_binary=None):
    """Verify the gateway's ordered observations across two independent processes."""
    if not isinstance(report, dict) or report.get("original_effort") != "ultra":
        raise ValueError("original model-resume selection did not report Ultra")
    thread = report.get("thread")
    if not isinstance(thread, str) or not MODEL_UUID.fullmatch(thread):
        raise ValueError("missing original thread identity")
    old, new, other = "gateway-amber", "gateway-indigo", "gateway-ochre"
    if surface not in MODEL_SURFACES:
        raise ValueError("unknown model-resume surface")
    if surface in SELECTOR_SURFACES:
        check_model_terminal_exit(report.get("seed_custody"), [
            str(seed_binary or binary), "--no-daemon", "--no-alt-screen", "Remember the cedar marker from this conversation."])
    reattach = surface in ("loaded", "unloaded")
    live = reattach or surface in ("live", "live-terminal")
    resumed = released = False
    catalogs = requests = 0
    turns = []
    events = report.get("events")
    if not isinstance(events, list) or not all(isinstance(row, dict) for row in events):
        raise ValueError("missing ordered model-resume observations")
    if surface == "exec-last":
        check_exec_resume_workspaces(report, events, binary, seed_binary or binary)
        events = [row for row in events if row.get("event") != "workspace_request"]
    for event in events:
        kind = event.get("event")
        if kind == "catalog":
            if (event.get("models") != ([old, new, other] if released else [old, other])
                    or event.get("successor") != (new if released else None)):
                raise ValueError("unexpected model catalog for this phase of the conversation")
            catalogs += 1
        elif kind == "original_exit":
            if (resumed or requests != 1 or event.get("thread") != thread
                    or type(event.get("exit")) is not int or event["exit"] != 0):
                raise ValueError("original exit must follow exactly one original request")
            resumed = True
            released = not live
            # A live resume can reuse the old catalog the seed already fetched.
            if released:
                catalogs = 0
            requests = 0
        elif kind in ("catalog_signal", "reattach"):
            if not live or not resumed or released or requests != 1:
                raise ValueError("provider release must follow the first active turn's request")
            if reattach:
                if kind != "reattach":
                    raise ValueError("reattachment must discover the release without a provider signal")
            elif kind != "catalog_signal" or event.get("etag") != '"indigo"':
                raise ValueError("missing provider catalog signal")
            released = True
            catalogs = requests = 0
        elif kind == "request":
            turn = event.get("turn")
            if (not isinstance(turn, str) or not MODEL_UUID.fullmatch(turn)
                    or turn in turns):
                raise ValueError("model resume requires a distinct provider turn for every prompt")
            turns.append(turn)
            if (not catalogs or requests or event.get("thread") != thread
                    or event.get("model") != (new if released else old) or event.get("effort") != ("high" if released else "xhigh")
                    or event.get("seed_prompt") is not True or event.get("prior_reply") is not resumed
                    or event.get("resume_prompt") is not resumed
                    or (live and (event.get("after_release") is not released or event.get("resumed_reply") is not released))):
                raise ValueError("provider request did not preserve the expected model, effort, thread and history")
            requests += 1
        else:
            raise ValueError("unknown model-resume observation")
    if not resumed or not released or requests != 1:
        raise ValueError("missing resumed provider request")
    terminal_turns = turns[:1] if surface in SELECTOR_SURFACES else []
    if surface in TERMINAL_SURFACES:
        terminal_turns.extend(turns[1:])
    if report.get("terminal_completed_turns") != terminal_turns:
        raise ValueError("terminal completion does not match the provider turns")
    if surface == "unloaded" and report.get("loaded_before_resume") != []:
        raise ValueError("saved thread must have left the app-server before resume")
    if reattach and report.get("reattached") != {"thread": thread, "model": new, "effort": "ultra"}:
        raise ValueError("reattachment did not return the successor on the original thread with Ultra")
    if surface == "live" and report.get("idle_promotion") != {
        "thread": thread, "model": new, "effort": "ultra", "preceding_turns": turns[:-1],
    }:
        raise ValueError("live session did not announce the successor and Ultra before the next turn")
    if reattach or surface == "live":
        process = report.get("process")
        if (not isinstance(process, dict) or process.get("argv") != [str(binary), "app-server", "--listen", "stdio://"]
                or type(process.get("returncode")) is not int or process["returncode"] != 0
                or process.get("timed_out") is not False):
            raise ValueError("live app-server did not complete both turns on the resumed thread")
        return
    target = "--last" if surface in ("exec-last", "terminal-last") else thread
    if surface in ("exec", "exec-last"):
        process = report.get("process")
        argv = process.get("argv") if isinstance(process, dict) else None
        if (not isinstance(argv, list) or argv[:-1] != [str(binary), "exec", "--skip-git-repo-check", "resume", target]
                or not isinstance(argv[-1], str) or not argv[-1] or type(process.get("returncode")) is not int
                or process["returncode"] != 0 or process.get("timed_out") is not False or report.get("answered") is not True):
            raise ValueError("exec resume did not complete the original thread")
        return
    command = [str(binary), "--no-daemon", "--no-alt-screen", "resume"]
    if surface != "picker":
        command += [target, "Recall our cedar marker and continue."]
    check_model_terminal_exit(report.get("custody"), command)
    end = report.get("terminal_view_bytes")
    if (type(end) is not int or not terminal_output or not 0 < end <= len(terminal_output)
            or terminal_model_selection(terminal_output[:end], live) != (new.encode(), b"ultra")):
        raise ValueError("final terminal status did not show the successor and saved Ultra effort")
    if not live and terminal_model_selection(terminal_output[:end], live, after_reply=False, first=True) != (new.encode(), b"ultra"):
        raise ValueError("initial terminal status did not show the opened thread's model and effort")


def git_head(repo):
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1", GIT_OPTIONAL_LOCKS="0",
               GIT_NO_REPLACE_OBJECTS="1", GIT_NO_LAZY_FETCH="1")
    return subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"],
                                   env=env, text=True).strip()


def retain_diagnostics(scratch, output):
    # Keep commands, native events, HTTP traces and guardian results. The fake
    # HOME/CODEX_HOME, databases and sockets stay in the short protected scratch.
    if not scratch.is_dir():
        return
    directories = [scratch, *(scratch / name for name in (*CASES, "history", "history/logs", *MODEL_SURFACES.values()))]
    capacity = scratch / "capacity"
    if capacity.is_dir() and not capacity.is_symlink():
        directories.extend([capacity, *(capacity / name for name in CAPACITY_REQUESTS)])
    for directory in directories:
        if not directory.is_dir() or directory.is_symlink():
            continue
        destination = output / "smoke" / directory.relative_to(scratch)
        destination.mkdir(parents=True, exist_ok=True)
        for path in directory.iterdir():
            if stat.S_ISREG(path.lstat().st_mode):
                shutil.copyfile(path, destination / path.name, follow_symlinks=False)


def write_record(output, record):
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=output,
                                         prefix=".runtime-record-", suffix=".json",
                                         delete=False) as stream:
            temporary = Path(stream.name)
            json.dump(record, stream, indent=2)
            stream.write("\n")
        # Closing the temporary completes critical record I/O. Publish only to
        # a fresh name; an existing file, symlink or directory is never replaced.
        os.link(temporary, output / "result.json")
    finally:
        if temporary is not None:
            try:
                temporary.unlink()
            except OSError as error:
                # After link this is cleanup of an already committed record;
                # before link it must not replace the selected write failure.
                CLI["report_failure"]("alab: runtime record temporary cleanup failed: ", error)


def run(repo, executable, source, scratch, output):
    if not re.fullmatch(r"[0-9a-f]{40}", source) or git_head(repo) != source:
        raise ValueError("runtime smoke requires the exact checked-out source commit")
    if not all(path.is_absolute() for path in (executable, scratch, output)):
        raise ValueError("runtime executable, scratch and evidence paths must be absolute")
    if scratch.exists() or scratch.is_symlink():
        raise ValueError("runtime smoke requires fresh scratch")
    output.mkdir(mode=0o700)
    probe = repo / "alab/smoke/weekly_reset.py"
    inputs = [repo / "alab/check_runtime.py", repo / "alab/cli_status.py", repo / "alab/file_hash.py",
              probe, probe.with_name("provider_fixture.py"),
              repo / "alab/command_guard.py", probe.with_name("history_pagination.py"),
              probe.with_name("capacity_exec.py"), probe.with_name("quota_resume.py"), probe.with_name("model_resume.py"),
              probe.with_name("request_recovery.py")]
    before = {str(path.relative_to(repo)): sha256(path) for path in inputs}
    binary = sha256(executable)
    command = [sys.executable, "-I", "-B", "-S", "-c", BOOTSTRAP, str(probe),
               "--codex", str(executable), "--output", str(scratch)]
    for name in CASES:
        command.extend(("--case", name))
    record = {"schema": 1, "kind": "built-codex-runtime-observation", "source": source,
              "binary_sha256": binary, "probe_inputs": before, "requested_cases": list(CASES),
              "command": command, "passed": False,
              "scope": "selected reset, persisted TUI pagination, cold and live model resume, and capacity recovery with fake auth and loopback; no proof or full reset-suite claim"}
    status = 1
    try:
        with (output / "stdout.txt").open("xb") as stdout, (output / "stderr.txt").open("xb") as stderr:
            status = subprocess.call(command, cwd=repo, stdout=stdout, stderr=stderr)
        record["exit"] = status
        report_path = scratch / "result.json"
        report = json.loads(report_path.read_text())
        record["smoke_report_sha256"] = sha256(report_path)
        if status != 0 or report.get("passed") is not True:
            raise ValueError("app runtime smoke failed; see retained stdout/stderr and report")
        if (report.get("requestedCases") != list(CASES) or report.get("fullSuite") is not False
                or report.get("binarySha256") != binary or report.get("probeSha256") != before["alab/smoke/weekly_reset.py"]):
            raise ValueError("runtime smoke report differs from its requested source/binary/coverage")
        cases = report.get("cases", [])
        if [case.get("case") for case in cases] != list(CASES):
            raise ValueError("runtime smoke omitted a required policy boundary")
        for case in cases:
            models, consumes, positive = EXPECTATIONS[case["case"]]
            if (case.get("passed") is not True or case.get("guardianTimedOut") is not False
                    or case.get("consumeRequests") != consumes
                    or case.get("modelRequests") != models or (case.get("exit") == 0) != positive):
                raise ValueError("runtime smoke did not observe the required reset/zero-POST behavior")
            if case["case"] in NOTICE_COUNTS:
                counts = (case.get("resetNotices"), case.get("creditNotices"))
                if any(type(count) is not int for count in counts) or counts != NOTICE_COUNTS[case["case"]]:
                    raise ValueError(f"unexpected recovery notice counts for {case['case']}")
            if case["case"].startswith("prepost-") and case.get("postSelectionUsageReads", 0) < 1:
                raise ValueError("runtime smoke never reached post-selection usage")
        history_command = [sys.executable, "-I", "-B", "-S", "-c", BOOTSTRAP,
                           str(probe.with_name("history_pagination.py")), "--codex", str(executable),
                           "--output", str(scratch / "history")]
        record["history_command"] = history_command
        with (output / "history-stdout.txt").open("xb") as stdout, (output / "history-stderr.txt").open("xb") as stderr:
            status = subprocess.call(history_command, cwd=repo, stdout=stdout, stderr=stderr)
        record["history_exit"] = status
        history_path = scratch / "history/result.json"
        history = json.loads(history_path.read_text())
        custody = history.get("custody", {})
        if (status != 0 or history.get("passed") is not True or history.get("sha256") != binary
                or history.get("turns") != 260 or history.get("model_requests_after_seed") != 260
                or history.get("older_history_page_requests", 0) < 3
                or custody.get("exit") != 0 or custody.get("owning_reap") is not True
                or custody.get("group_absent_after_reap") is not True or custody.get("forced") is not False):
            raise ValueError("actual persisted history/resize runtime smoke failed")
        record["history_report_sha256"] = sha256(history_path)
        for surface, directory in MODEL_SURFACES.items():
            model_path = scratch / directory / "result.json"
            field = directory.replace("-", "_")
            model_command = [sys.executable, "-I", "-B", "-S", "-c", BOOTSTRAP, str(probe.with_name("model_resume.py")),
                             "--codex", str(executable), "--output", str(model_path.parent), "--surface", surface]
            record[field + "_command"] = model_command
            with (output / (directory + "-stdout.txt")).open("xb") as stdout, (output / (directory + "-stderr.txt")).open("xb") as stderr:
                status = subprocess.call(model_command, cwd=repo, stdout=stdout, stderr=stderr)
            record[field + "_exit"] = status
            model_report = json.loads(model_path.read_text())
            record[field + "_report_sha256"] = sha256(model_path)
            if (status != 0 or not isinstance(model_report, dict)
                    or model_report.get("passed") is not True or model_report.get("binary_sha256") != binary
                    or model_report.get("seed_binary_sha256") != binary):
                raise ValueError(f"{surface} model-resume runtime smoke failed")
            terminal = surface in TERMINAL_SURFACES
            check_model_resume(model_report, surface, executable,
                               (model_path.parent / "terminal.raw").read_bytes() if terminal else None)
        capacity_path = scratch / "capacity/result.json"
        command = [sys.executable, "-I", "-B", "-S", "-c", BOOTSTRAP, str(probe.with_name("request_recovery.py")),
                   "--capacity-only", "--codex", str(executable), "--output", str(capacity_path.parent)]
        record["capacity_command"] = command
        with (output / "capacity-stdout.txt").open("xb") as stdout, (output / "capacity-stderr.txt").open("xb") as stderr:
            status = subprocess.call(command, cwd=repo, stdout=stdout, stderr=stderr)
        record["capacity_exit"] = status
        capacity = json.loads(capacity_path.read_text())
        record["capacity_report_sha256"] = sha256(capacity_path)
        if (status != 0 or not isinstance(capacity, dict) or capacity.get("passed") is not True
                or capacity.get("sha256") != binary or capacity.get("binary") != str(executable.resolve())):
            raise ValueError("capacity runtime smoke failed for the requested executable")
        cases = capacity.get("cases")
        if not isinstance(cases, list) or len(cases) != len(CAPACITY_REQUESTS):
            raise ValueError("capacity runtime smoke has incomplete coverage")
        timing = runpy.run_path(str(probe.with_name("provider_fixture.py")))
        for case, (name, (statuses, waits)) in zip(cases, CAPACITY_REQUESTS.items()):
            count = len(statuses)
            if (not isinstance(case, dict) or case.get("case") != name or case.get("passed") is not True
                    or type(case.get("requestCount")) is not int or case["requestCount"] != count
                    or type(case.get("returncode")) is not int or case["returncode"] != 0):
                raise ValueError(f"capacity runtime smoke did not recover the {name} case")
            elapsed = case.get("localBackoffSeconds" if name == "unadvised-overload" else "retryAfterSeconds")
            if type(elapsed) not in (float, int) or not 0 < elapsed < math.inf:
                raise ValueError(f"capacity runtime smoke omitted a measured retry delay for {name}")
            case_dir = capacity_path.parent / name
            check_capacity_exit(case_dir, executable)
            events = timing["read_stdout_events"](case_dir / "exec-stdout.jsonl")
            turns = timing["turn_events"](events)
            if any(event.get("type") == "error" for event in events):
                raise ValueError(f"capacity exec output for {name} emitted a top-level JSON error")
            if turns != ["turn.started", "turn.completed"] or case.get("turnEvents") != turns:
                raise ValueError(f"capacity turn evidence for {name} must show one started and completed turn "
                                 "in both exec output and report")
            answer_index = check_capacity_answer(events, name)
            requests = json.loads((case_dir / "requests.json").read_text())
            if not isinstance(requests, list) or len(requests) != count:
                raise ValueError(f"capacity request trace does not have {count} requests for {name}")
            for index, (row, expected) in enumerate(zip(requests, statuses, strict=True), 1):
                status_code = row.get("status") if isinstance(row, dict) else None
                if type(status_code) is not int or status_code != expected:
                    raise ValueError(f"capacity request {index} for {name} expected HTTP {expected}, got {status_code!r}")
            try:
                if name == "prewarm-overload":
                    attempts = requests[:2]
                    observed = timing["prewarm_retry_delay"](requests, waits[0])
                else:
                    attempts = requests[-(len(waits) + 1):]
                    pairs = zip(attempts[:-1], attempts[1:], waits, strict=True)
                    for attempt, (failed, retry, seconds) in enumerate(pairs, 1):
                        observed = timing["retry_after_delay"](failed, retry, seconds, attempt=attempt)
                    if name in ("streamed-overload", "late-stream-overload"):
                        timing["check_open_stream_retry"](*attempts)
                    if name == "late-stream-overload":
                        timing["check_expired_stream_advice"](*attempts, waits[0], events)
                timing["check_capacity_notices"](events, answer_index, waits, attempts[:-1])
                if name in ("prewarm-overload", "repeated-overload", "unadvised-overload"):
                    received = json.loads((case_dir / "exec-observed.json").read_text())
                    codes = timing["REPEATED_CAPACITY_CODES"] if name == "repeated-overload" else None
                    timing["check_live_notices"](events, received, attempts, waits, expected_codes=codes,
                                                 startup=name == "prewarm-overload")
                posts = requests[2:] if name == "prewarm-overload" else requests
                identity = timing["check_capacity_request_identity"](
                    events, posts, "gpt-5", handshake=requests[1] if name == "prewarm-overload" else None)
                timing["check_capacity_history"](posts, fail_with_tool=name == "tool-stream-overload")
                timing["check_capacity_rollout"](case_dir, *identity, requests=posts, answer=events[answer_index]["item"]["text"])
            except ValueError as error:
                raise ValueError(f"capacity request trace for {name}: {error}") from error
            if elapsed != observed:
                raise ValueError(f"capacity retry delay disagrees with the request trace for {name}")
        if (sha256(executable) != binary or git_head(repo) != source
                or {str(path.relative_to(repo)): sha256(path) for path in inputs} != before):
            raise ValueError("runtime executable or source inputs changed during the smoke")
        record["passed"] = True
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        record["error"] = f"{type(error).__name__}: {error}"
        status = status or 1
    finally:
        try:
            retain_diagnostics(scratch, output)
        except OSError as error:
            record["passed"] = False
            record["diagnostics_error"] = f"{type(error).__name__}: {error}"
            status = status or 1
            CLI["report_failure"]("alab: runtime diagnostic retention failed: ", error)
        try:
            write_record(output, record)
        except OSError as error:
            record["passed"] = False
            status = status or 1
            CLI["report_failure"]("alab: runtime record retention failed: ", error)
    return status


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--source", required=True)
    parser.add_argument("--scratch", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        status = run(Path(__file__).resolve().parents[1], args.codex, args.source, args.scratch, args.output)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f"alab: runtime smoke refused: {error}\n")
    print(f"alab: app runtime smoke {'passed' if status == 0 else 'failed'}; evidence: {args.output}")
    return 128 - status if status < 0 else status


if __name__ == "__main__":
    sys.exit(main())
