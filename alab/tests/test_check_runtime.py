#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Execute runtime-smoke dispatch with inert child programs; policy behavior stays in the real smoke."""

import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import os
import runpy
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
CASES = ["no-credit", "existing-usage-credits", "paid-retry-refused", "spend-control-reopens", "spend-control-blocked", "plan-renewed-before-credit-retry",
         "preflight-five-hour", "preflight-below-99", "preflight-weekly",
         "recovery-weekly-25h", "preflight-weekly-24h", "preflight-weekly-25h",
         "preflight-weekly-48h", "recovery-weekly-48h", "recovery-weekly-49h", "preflight-elapsed-reset",
         "prepost-recovery-restored", "prepost-recovery-shifted", "prepost-recovery-foreign",
         "prepost-recovery-below-99", "prepost-recovery-denied", "prepost-preflight-99"]
FIXTURE_PROBE = '''import argparse,hashlib,json,os,subprocess
from pathlib import Path
import provider_fixture
assert provider_fixture.SOURCE == "owned source fixture"
parser=argparse.ArgumentParser()
parser.add_argument("--codex",type=Path,required=True)
parser.add_argument("--output",type=Path,required=True)
parser.add_argument("--case",action="append",required=True)
args=parser.parse_args()
args.output.mkdir()
mode=os.environ.get("FIXTURE_MODE", "success")
cases=[]
for name in args.case:
    directory=args.output/name
    directory.mkdir()
    subprocess.run([str(args.codex),name,str(directory/"executed.txt")],check=True)
    positive=name in ("existing-usage-credits","spend-control-reopens","plan-renewed-before-credit-retry","preflight-weekly","preflight-weekly-25h","preflight-weekly-48h",
                      "recovery-weekly-25h","recovery-weekly-48h","recovery-weekly-49h",
                      "prepost-recovery-restored",
                      "prepost-recovery-denied","prepost-preflight-99")
    consumes=int(positive and name not in ("existing-usage-credits","spend-control-reopens","plan-renewed-before-credit-retry","prepost-recovery-restored"))
    if (mode=="five-hour-redemption" and name=="preflight-five-hour"
            or mode=="24h-redemption" and name=="preflight-weekly-24h"):
        consumes=1
    models=3 if name in ("existing-usage-credits","spend-control-reopens","paid-retry-refused","plan-renewed-before-credit-retry","prepost-recovery-restored","prepost-recovery-denied",
                        "recovery-weekly-25h","recovery-weekly-48h","recovery-weekly-49h") else 2
    (directory/"requests.json").write_text(json.dumps({"source":provider_fixture.SOURCE,"case":name}))
    cases.append({"case":name,"passed":True,"guardianTimedOut":False,"modelRequests":models,"postSelectionUsageReads":int(name.startswith("prepost-")),
                  "consumeRequests":consumes if mode!="bad-redemption" else 1,
                  "exit":0 if positive else 130})
    if name in ("no-credit","spend-control-blocked","existing-usage-credits","spend-control-reopens","paid-retry-refused","plan-renewed-before-credit-retry"):
        cases[-1]["resetNotices"]=int(name=="plan-renewed-before-credit-retry")
        cases[-1]["creditNotices"]=int(name not in ("no-credit","plan-renewed-before-credit-retry"))
    if mode=="noisy-empty-check" and name=="no-credit":
        cases[-1]["resetNotices"]=2
    if mode=="silent-paid-retry" and name=="existing-usage-credits":
        del cases[-1]["creditNotices"]
    if mode=="mislabeled-paid-retry" and name=="existing-usage-credits":
        cases[-1].update(resetNotices=1,creditNotices=0)
    if mode=="second-paid-attempt" and name=="paid-retry-refused":
        cases[-1]["modelRequests"]=4
    if mode=="repeat-paid-notice" and name=="paid-retry-refused":
        cases[-1]["creditNotices"]=2
    if mode=="duplicate-renewal" and name=="plan-renewed-before-credit-retry":
        cases[-1]["resetNotices"]=2
    if mode=="bad-25h" and name in ("preflight-weekly-25h","recovery-weekly-25h"):
        cases[-1].update(consumeRequests=0,exit=130,modelRequests=2)
    if mode=="bad-48h" and name in ("preflight-weekly-48h","recovery-weekly-48h"):
        cases[-1].update(consumeRequests=0,exit=130,modelRequests=2)
    if mode=="bad-49h" and name=="recovery-weekly-49h":
        cases[-1].update(consumeRequests=0,exit=130,modelRequests=2)
report={"passed":mode!="failure","fullSuite":False,"requestedCases":args.case,
        "binarySha256":hashlib.sha256(args.codex.read_bytes()).hexdigest(),
        "probeSha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "cases":cases[:-1] if mode=="missing-case" else cases}
if mode!="missing-report":
    (args.output/"result.json").write_text(json.dumps(report))
if mode=="changed-binary":
    args.codex.write_text(args.codex.read_text()+"\\n# changed after execution\\n")
if mode in ("changed-cli-status", "changed-file-hash"):
    helper=Path(__file__).resolve().parents[1]/("file_hash.py" if mode=="changed-file-hash" else "cli_status.py")
    helper.write_text(helper.read_text()+"\\n# changed during observation\\n")
raise SystemExit(23 if mode=="failure" else 0)
'''


FIXTURE_CAPACITY = '''import argparse,hashlib,json,os,subprocess
from pathlib import Path
parser=argparse.ArgumentParser()
parser.add_argument("--capacity-only", action="store_true", required=True)
parser.add_argument("--codex", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
args=parser.parse_args()
args.output.mkdir()
cases=[]
mode=os.environ.get("FIXTURE_MODE")
for name,count in (("overload",3),("streamed-overload",3),("late-stream-overload",3),("tool-stream-overload",2),("prewarm-overload",4),("repeated-overload",4),("unadvised-overload",5)):
    directory=args.output/name
    directory.mkdir()
    subprocess.run([str(args.codex),name,str(directory/"executed.txt")],check=True)
    command=[str(args.codex.resolve()),"exec","--json","--color","never","--skip-git-repo-check",
             "--cd",str(directory.resolve()/"work"),"--sandbox","read-only",
             "Keep this headless capacity smoke task and its completed plan."]
    receipt={"schema":1,"argv":command,"returncode":0,"timed_out":False,"timeout_seconds":120.0}
    if name=="overload":
        if mode=="capacity-guardian-timeout":
            receipt.update(returncode=None,timed_out=True)
        if mode=="capacity-guardian-failure":
            receipt["returncode"]=1
        if mode=="capacity-guardian-other-case":
            command[7]=str(directory.parent/"streamed-overload"/"work")
        if mode=="capacity-guardian-other-binary":
            command[0]+="-other"
    if mode!="capacity-guardian-missing" or name!="overload":
        (directory/"exec-guardian-result.json").write_text(json.dumps(receipt))
    events=[{"type":"turn.started"}]
    trace=[{"status":200} for _ in range(count)]
    if name=="overload":
        trace[1]["status"]=403 if mode=="capacity-denied-request" else 503
    if name=="tool-stream-overload" and mode=="capacity-refused-stream":
        trace[0]["status"]=503
    if name=="prewarm-overload" and mode=="capacity-missing-final-status":
        del trace[-1]["status"]
    if name=="streamed-overload" and mode=="capacity-noninteger-status":
        trace[0]["status"]=200.0
    trace[-2].update(headerStartedAt=100.0,headerAt=100.03125,retryAfter=2)
    if name=="overload" and mode=="capacity-missing-header":
        del trace[-2]["retryAfter"]
    if name=="streamed-overload" and mode=="capacity-wrong-header":
        trace[-2]["retryAfter"]=1
    trace[-1]["requestStartedAt"]=100.5 if mode=="capacity-early" else 102.0
    if name=="streamed-overload":
        trace[-2].update(status=200,failureFrameFlushedAt=100.2)
        trace[-1]["arrivedDuringFailedStream"]=mode!="capacity-waited-for-eof"
    if name=="late-stream-overload":
        start=101.9 if mode=="stream-advice-still-live" else 102.125
        trace[-2].update(status=200,retryAfter=2,failureFrameStartedAt=start,failureFrameFlushedAt=start+0.01)
        trace[-1].update(arrivedDuringFailedStream=True,requestStartedAt=102.375 if mode=="stream-skipped-backoff" else 103.5)
        seconds=2 if mode=="stream-rearmed-advice" else 1
        message=f"Model is at capacity; retrying automatically (interrupt to stop)... (Retry scheduled in {seconds}s.)"
        events.append({"type":"item.completed","item":{"type":"error","message":message}})
    if name=="prewarm-overload":
        trace[0].update(requestKind="prewarm",threadId="original",turnId="",status=503,retryAfter=2,
                        headerStartedAt=90.0,headerAt=90.03125)
        trace[1].update(requestKind="turn",threadId="original",turnId="active",status=426,
                        requestStartedAt=91.0 if mode=="capacity-startup-early" else 92.0,
                        headerStartedAt=92.25,headerAt=92.28125)
        trace[2].update(requestStartedAt=98.0,body={},headerStartedAt=98.25,headerAt=98.28125)
        if mode=="capacity-racing-fallback":
            trace[1].update(headerStartedAt=99.0,headerAt=99.03125)
        if mode=="capacity-racing-plan":
            trace[2].update(headerStartedAt=103.0,headerAt=103.03125)
        trace[3]["body"]={}
        if mode=="capacity-conflicting-startup-header":
            trace[0]["responseHeaders"]={"rEtRy-AfTeR":"10"}
        message="Model is at capacity; retrying automatically (interrupt to stop)... (Retry scheduled in 2s.)"
        events.append({"type":"item.completed","item":{"type":"error","message":message}})
        received=[90.0,92.5 if mode=="capacity-buffered-startup-notice" else 90.5,105.0,106.0]
        if mode=="capacity-answer-before-notice":
            received=[90.0,90.25,90.5,106.0]
        (directory/"exec-observed.json").write_text(json.dumps([87.0,*received]))
    if name=="repeated-overload":
        trace[-3].update(headerStartedAt=90.0,headerAt=90.03125)
        trace[-2]["requestStartedAt"]={"capacity-first-early":91.0, "capacity-backwards":100.25}.get(mode,92.0)
        trace[-1]["requestStartedAt"]=102.75 if mode=="capacity-second-early" else 103.0
        trace[-3].update(status=503,retryAfter=2,capacityCode="server_is_overloaded")
        trace[-2].update(status=403 if mode=="capacity-forbidden-trace" else 503,retryAfter=3,
                         capacityCode="server_is_overloaded" if mode=="capacity-unchanged-reason" else "policy_capacity_unavailable")
        server="Model is at capacity; retrying automatically (interrupt to stop)..."
        policy="Policy-restricted capacity is unavailable; retrying automatically (interrupt to stop)..."
        second=server if mode in ("capacity-stale-notice","capacity-unchanged-reason") else policy
        for prefix,seconds in ((server,2),(second,1 if mode=="capacity-understated-notice" else 3)):
            events.append({"type":"item.completed","item":{"type":"error","message":f"{prefix} (Retry scheduled in {seconds}s.)"}})
        received=[88.0,90.125,100.25,105.0,106.0]
        if mode=="capacity-late-first-notice":
            received[1]=92.5
            trace[-2]["requestStartedAt"]=93.0
        if mode=="capacity-buffered-notices":
            received[1:3]=[104.25,104.25]
        (directory/"exec-observed.json").write_text(json.dumps([87.0,*received]))
    if name=="unadvised-overload":
        trace[1].update(headerStartedAt=97.0,headerAt=97.03125,status=503,retryAfter=None)
        trace[2].update(requestStartedAt=97.25 if mode=="local-first-early" else 97.5,
                        headerStartedAt=98.0,headerAt=98.03125,status=503,
                        retryAfter=2 if mode=="local-unexpected-advice" else None)
        if mode=="local-extra-header":
            trace[2]["responseHeaders"]={"retry-after":"3"}
        trace[3].update(requestStartedAt=98.75 if mode=="local-reset" else 99.0,status=503,
                        retryAfter=True if mode=="local-invalid-short-header" else 1)
        trace[4]["requestStartedAt"]=101.5 if mode=="local-shortened-by-header" else 102.0
        received=[87.0,88.0,97.125,98.25,100.25,105.0,106.0]
        if mode=="local-buffered-notices":
            received[2:5]=[103.0]*3
        (directory/"exec-observed.json").write_text(json.dumps(received))
    elapsed="localBackoffSeconds" if name=="unadvised-overload" else "retryAfterSeconds"
    if name in ("overload","streamed-overload","tool-stream-overload","unadvised-overload"):
        delays=(1,1,2) if name=="unadvised-overload" else (2,)
        if name=="unadvised-overload" and mode=="local-overstated-notice":
            delays=(1,1,4)
        if name=="unadvised-overload" and mode=="capacity-silent-second-wait":
            delays=(1,4)
        if name=="unadvised-overload" and mode=="local-understated-notice":
            delays=(1,2,1)
        if name=="overload" and mode=="capacity-silent-wait":
            delays=()
        for seconds in delays:
            prefix="Model is at capacity; retrying automatically (interrupt to stop)..."
            if mode=="capacity-wrong-stream-reason" and name=="tool-stream-overload":
                prefix="Policy-restricted capacity is unavailable; retrying automatically (interrupt to stop)..."
            event={"type":"item.completed","item":{"type":"error","message":f"{prefix} (Retry scheduled in {seconds}s.)"}}
            if mode=="capacity-notice-before-turn" and name=="overload":
                events.insert(0,event)
            else:
                events.append(event)
    if name=="overload" and mode=="capacity-fatal-wait":
        events.append({"type":"error","message":"Model is at capacity"})
    if name=="tool-stream-overload" and mode=="capacity-restarted-turn":
        events.extend([{"type":"turn.failed"},{"type":"turn.started"}])
    answer={"type":"item.completed","item":{"type":"agent_message","text":"Headless capacity recovery succeeded."}}
    if name!="overload" or mode!="capacity-missing-answer":
        events.append(answer)
    if name=="overload" and mode=="capacity-duplicate-answer":
        events.append(answer)
    if name=="overload" and mode=="capacity-wrong-answer":
        answer["item"]["text"]="An unrelated answer."
    if name=="overload" and mode=="capacity-early-answer":
        events.insert(0,events.pop())
    if name=="prewarm-overload" and mode=="capacity-answer-before-notice":
        events.insert(1,events.pop())
    if name!="prewarm-overload" or mode!="capacity-unfinished-turn":
        events.append({"type":"turn.completed"})
    if name=="overload" and mode=="capacity-late-answer":
        events.append(events.pop(-2))
    events.insert(0,{"type":"thread.started","thread_id":"original"})
    if name=="overload" and mode=="capacity-late-exec-thread":
        events.append(events.pop(0))
    posts=trace[2:] if name=="prewarm-overload" else trace
    sessions=directory/"codex-home/sessions"
    sessions.mkdir(parents=True)
    saved_turn="other-turn" if name=="repeated-overload" and mode=="capacity-different-saved-turn" else "active"
    records=[{"type":"session_meta","payload":{"id":"original"}},
             *({"type":"event_msg","payload":{"type":kind,"turn_id":saved_turn}} for kind in ("task_started","task_complete"))]
    records[-1]["payload"]["last_agent_message"]=answer["item"]["text"]
    if name=="repeated-overload" and mode=="capacity-saved-failure":
        records[-1]["payload"]["error"]={"message":"provider did not recover"}
    prompt={"type":"message","role":"user","content":[{"type":"input_text","text":command[-1]}]}
    records.insert(2,{"type":"response_item","payload":prompt})
    if name=="repeated-overload" and mode=="capacity-saved-steering":
        records.insert(-1,{"type":"event_msg","payload":{"type":"user_message","message":"continue"}})
    if name=="repeated-overload" and mode=="capacity-saved-user-input":
        records.insert(-1,{"type":"response_item","payload":dict(prompt,content=[{"type":"input_text","text":"continue"}])})
    call={"type":"function_call","name":"update_plan","call_id":"capacity-smoke-plan",
          "arguments":json.dumps({"plan":[{"step":"Completed work survives a capacity retry","status":"completed"}]})}
    result={"type":"function_call_output","call_id":"capacity-smoke-plan","output":"Plan updated"}
    records.insert(-1,{"type":"response_item","payload":call})
    if name!="overload" or mode!="capacity-lost-saved-output":
        records.insert(-1,{"type":"response_item","payload":result})
    (sessions/"rollout-fixture-original.jsonl").write_text(chr(10).join(map(json.dumps,records)))
    for index,row in enumerate(posts):
        meta={"thread_id":"original","turn_id":"active","request_kind":"turn"}
        row["body"]={"model":"gpt-5","reasoning":{"effort":"medium"},"input":[prompt,call,result] if index else [prompt],
                     "client_metadata":{"x-codex-turn-metadata":json.dumps(meta)}}
    if name=="overload" and mode=="capacity-dropped-tool-result":
        posts[-1]["body"]["input"].pop()
    if name=="prewarm-overload" and mode=="capacity-preexisting-work":
        posts[0]["body"]["input"].extend([call,result])
    if name=="tool-stream-overload" and mode=="capacity-extra-stream-input":
        posts[1]["body"]["input"].append({"type":"reasoning","summary":[]})
    if name=="repeated-overload" and mode=="capacity-steered-retries":
        for row in posts[1:]:
            row["body"]["input"].append({"role":"user","content":"continue"})
    if name=="repeated-overload" and mode=="capacity-unattributed-input":
        for row in posts:
            row["body"]["input"].insert(0,{"role":"user","content":"<continue/>"})
    if name=="overload" and mode=="capacity-changed-model":
        posts[-1]["body"]["model"]="another-model"
    if name=="repeated-overload" and mode=="capacity-changed-reasoning":
        posts[-2]["body"]["reasoning"]["effort"]="low"
    if name=="tool-stream-overload" and mode=="capacity-changed-turn":
        meta["turn_id"]="another-turn"
        posts[-1]["body"]["client_metadata"]["x-codex-turn-metadata"]=json.dumps(meta)
    if name=="prewarm-overload" and mode=="capacity-other-handshake-turn":
        trace[1]["turnId"]="another-turn"
    if name=="prewarm-overload" and mode=="capacity-other-exec-thread":
        events[0]["thread_id"]="another-thread"
    if name=="streamed-overload" and mode=="capacity-missing-turn-metadata":
        del posts[-1]["body"]["client_metadata"]
    separator=chr(13) if mode=="capacity-cr-only" and name=="overload" else chr(10)
    (directory/"exec-stdout.jsonl").write_text(separator.join(map(json.dumps,events)))
    (directory/"requests.json").write_text(json.dumps(trace))
    cases.append({"case":name,"passed":True,"requestCount":count,"returncode":0,
                  "turnEvents":[event["type"] for event in events if event["type"].startswith("turn.")],
                  elapsed:0.5 if mode=="capacity-wrong-delay" else trace[-1]["requestStartedAt"]-100.03125})
if mode=="capacity-turn-summary-disagrees":
    cases[0]["turnEvents"]=["turn.started","turn.failed"]
if mode=="capacity-missing":
    cases.pop()
if mode=="capacity-extra-request":
    cases[-1]["requestCount"]+=1
if mode=="capacity-no-delay":
    cases[0].pop("retryAfterSeconds")
report={"passed":True,"binary":str(args.codex),"sha256":hashlib.sha256(args.codex.read_bytes()).hexdigest(),
        "cases":cases}
(args.output/"result.json").write_text(json.dumps(report))
raise SystemExit(23 if mode=="failed-capacity" else 0)
'''


FIXTURE_HISTORY = '''import argparse,hashlib,json,os,subprocess
from pathlib import Path
parser=argparse.ArgumentParser()
parser.add_argument("--codex",type=Path,required=True)
parser.add_argument("--output",type=Path,required=True)
args=parser.parse_args()
args.output.mkdir()
subprocess.run([str(args.codex),"history",str(args.output/"executed.txt")],check=True)
report={"passed":True,"sha256":hashlib.sha256(args.codex.read_bytes()).hexdigest(),
        "turns":260,"model_requests_after_seed":260,"older_history_page_requests":3,
        "custody":{"exit":0,"owning_reap":True,"group_absent_after_reap":True,"forced":False}}
mode=os.environ.get("FIXTURE_MODE")
if mode=="bad-history":
    report["custody"]["owning_reap"]=False
if mode=="failed-history":
    logs=args.output/"logs"
    logs.mkdir()
    (logs/"codex-tui.log").write_text("retained history failure\\n")
    for name in ("home","codex-home"):
        private=args.output/name
        private.mkdir()
        (private/"auth.json").write_text("private fake-auth fixture; do not retain")
        (private/"state.sqlite").write_text("private database fixture; do not retain")
    (logs/"linked-auth.json").symlink_to(args.output/"codex-home/auth.json")
    report["passed"]=False
(args.output/"result.json").write_text(json.dumps(report))
if mode in ("changed-history-cli-status", "changed-history-file-hash"):
    helper=Path(__file__).resolve().parents[1]/("file_hash.py" if mode=="changed-history-file-hash" else "cli_status.py")
    helper.write_text(helper.read_text()+"\\n# changed during history observation\\n")
raise SystemExit(29 if mode=="failed-history" else 0)
'''


def model_receipt():
    thread = "01234567-89ab-cdef-0123-456789abcdef"
    old, new, other = "gateway-amber", "gateway-indigo", "gateway-ochre"
    seed_turn = "01234567-89ab-cdef-0123-000000000001"
    resumed_turn = "01234567-89ab-cdef-0123-000000000002"
    return {"passed": True, "thread": thread, "original_effort": "ultra", "terminal_completed_turns": [resumed_turn], "events": [
        {"event": "catalog", "models": [old, other], "successor": None},
        {"event": "request", "thread": thread, "turn": seed_turn, "model": old, "effort": "xhigh",
         "seed_prompt": True, "prior_reply": False, "resume_prompt": False},
        {"event": "original_exit", "thread": thread, "exit": 0},
        {"event": "catalog", "models": [old, new, other], "successor": new},
        {"event": "request", "thread": thread, "turn": resumed_turn, "model": new, "effort": "high",
         "seed_prompt": True, "prior_reply": True, "resume_prompt": True}],
        "custody": {"exit": 0, "owning_reap": True, "group_absent_after_reap": True, "forced": False,
                    "command": ["codex", "--no-daemon", "--no-alt-screen", "resume", thread,
                                "Recall our cedar marker and continue."]}}


def workspace_model_receipt(binary, seed_binary):
    report = model_receipt()
    request = {"event": "workspace_request", "thread": "11111111-89ab-cdef-0123-456789abcdef",
               "turn": "11111111-89ab-cdef-0123-000000000001", "model": "gateway-ochre", "effort": "high",
               "seed_prompt": True, "mixed_history": False, "prior_reply": False, "resume_prompt": False}
    report["events"].insert(2, request)
    report["events"].insert(-1, dict(request, turn="11111111-89ab-cdef-0123-000000000002",
                                     prior_reply=True, resume_prompt=True))
    for key, exe, args in [("workspace_seed", seed_binary, ["-C", "../birch", "Remember the birch in the second workspace."]),
                           ("workspace_all", binary, ["resume", "--last", "--all", "Recall the second workspace."])]:
        report[key] = {"argv": [exe, "exec", "--skip-git-repo-check", *args], "returncode": 0, "timed_out": False}
    return report


def live_model_receipt():
    report = model_receipt()
    del report["custody"]
    first_catalog, seed, exited, published, final = report["events"]
    seed.update(after_release=False, resumed_reply=False)
    report["terminal_completed_turns"] = []
    active = dict(seed, turn="01234567-89ab-cdef-0123-000000000003", prior_reply=True, resume_prompt=True)
    final.update(after_release=True, resumed_reply=True)
    report["events"] = [first_catalog, seed, exited, first_catalog.copy(), active,
                        {"event": "catalog_signal", "etag": '"indigo"'}, published, final]
    report["idle_promotion"] = {"thread": report["thread"], "model": "gateway-indigo", "effort": "ultra",
                                "preceding_turns": [seed["turn"], active["turn"]]}
    report["process"] = {"argv": ["codex", "app-server", "--listen", "stdio://"],
                         "returncode": 0, "timed_out": False}
    return report


FIXTURE_MODEL = """import argparse,hashlib,json,os,subprocess
from pathlib import Path
parser = argparse.ArgumentParser()
parser.add_argument("--codex", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--surface", choices=("terminal","exec","live","live-terminal","loaded","unloaded","picker","terminal-last","exec-last"), required=True)
args = parser.parse_args()
args.output.mkdir()
subprocess.run([str(args.codex), "model-resume", str(args.output / "executed.txt")], check=True)
report = """ + repr(model_receipt()) + """
if args.surface == "exec-last":
    report = """ + repr(workspace_model_receipt("codex", "old-codex")) + """
    report["workspace_seed"]["argv"][0] = str(args.codex)
    report["workspace_all"]["argv"][0] = str(args.codex)
if args.surface in ("exec-last", "terminal-last", "picker"):
    report["seed_custody"] = dict(report["custody"], command=[str(args.codex), "--no-daemon", "--no-alt-screen",
                                                           "Remember the cedar marker from this conversation."])
if args.surface in ("exec", "exec-last"):
    target = "--last" if args.surface == "exec-last" else report["thread"]
    report["terminal_completed_turns"] = [report["events"][1]["turn"]] if args.surface == "exec-last" else []
    report.pop("custody")
    report["process"] = {"argv": [str(args.codex), "exec", "--skip-git-repo-check", "resume", target, "continue"],
                         "returncode": 0, "timed_out": False}
    report["answered"] = True
    if os.environ.get("FIXTURE_MODE") == "wrong-exec-thread":
        report["process"]["argv"][-2] = "foreign"
if args.surface in ("live", "live-terminal", "loaded", "unloaded"):
    report = """ + repr(live_model_receipt()) + """
    report["process"]["argv"][0] = str(args.codex)
    if os.environ.get("FIXTURE_MODE") == "live-stays-on-old-model":
        report["events"][-1]["model"] = "gateway-amber"
if args.surface == "unloaded":
    report["loaded_before_resume"] = []
if args.surface in ("loaded", "unloaded"):
    report["events"][5] = {"event": "reattach"}
    report["reattached"] = {"thread": report["thread"], "model": "gateway-indigo", "effort": "ultra"}
if args.surface in ("terminal", "live-terminal", "picker", "terminal-last"):
    report.pop("process", None)
    report["custody"] = """ + repr(model_receipt()["custody"]) + """
    turns = [event["turn"] for event in report["events"] if event["event"] == "request"]
    report["terminal_completed_turns"] = turns if args.surface in ("terminal-last", "picker") else turns[1:]
    command = report["custody"]["command"]
    command[0] = str(args.codex)
    if args.surface == "picker":
        del command[4:]
    if args.surface == "terminal-last":
        command[4] = "--last"
    answer = "Final cedar answer." if args.surface == "live-terminal" else "Cedar marker recalled."
    model = ("gateway-amber" if args.surface == "live-terminal" and os.environ.get("FIXTURE_MODE") == "stale-visible-model"
             else "gateway-indigo")
    raw = (answer + " Read Only · " + model + " ultra").encode()
    report["terminal_view_bytes"] = len(raw)
    (args.output / "terminal.raw").write_bytes(raw)
report["binary_sha256"] = hashlib.sha256(args.codex.read_bytes()).hexdigest()
report["seed_binary_sha256"] = report["binary_sha256"]
if os.environ.get("FIXTURE_MODE") == "changed-live-seed" and args.surface == "live":
    report["seed_binary_sha256"] = "other"
if os.environ.get("FIXTURE_MODE") == "bad-model-receipt":
    report["events"][-1]["model"] = "gateway-amber"
(args.output / "result.json").write_text(json.dumps(report))
raise SystemExit(37 if os.environ.get("FIXTURE_MODE") == "failed-model-resume" else 0)
"""


class RuntimePolicyContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location('runtime_policy_controller', ROOT / 'alab/check_runtime.py')
        cls.runtime = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.runtime)
        sys.path.insert(0, str(ROOT / 'alab/smoke'))
        spec = importlib.util.spec_from_file_location('runtime_policy_weekly', ROOT / 'alab/smoke/weekly_reset.py')
        cls.weekly = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.weekly)

    def test_live_model_requires_provider_signal_new_catalog_and_both_turns(self):
        original = live_model_receipt()
        self.runtime.check_model_resume(original, "live", "codex")
        cached = copy.deepcopy(original)
        del cached["events"][3]  # The resumed process can reuse the seed's still-current catalog.
        self.runtime.check_model_resume(cached, "live", "codex")
        for index in (0, 1, 2, 4, 5, 6, 7):
            with self.subTest(missing=index):
                report = copy.deepcopy(original)
                del report["events"][index]
                with self.assertRaises(ValueError):
                    self.runtime.check_model_resume(report, "live", "codex")
        for index, key, value in [(4, "model", "gateway-indigo"), (5, "etag", '"amber"'),
                                  (7, "model", "gateway-amber"), (7, "model", "gateway-ochre"), (7, "effort", "low"),
                                  (7, "resumed_reply", False), (7, "after_release", False)]:
            with self.subTest(index=index, key=key):
                report = copy.deepcopy(original)
                report["events"][index][key] = value
                with self.assertRaises(ValueError):
                    self.runtime.check_model_resume(report, "live", "codex")
        with self.assertRaises(ValueError):
            self.runtime.check_model_resume(original, "live", "different-binary")

    def test_idle_model_notification_precedes_the_next_request_on_the_original_thread(self):
        report = live_model_receipt()
        expected = report.pop("idle_promotion")
        with self.assertRaisesRegex(ValueError, "before the next turn"):
            self.runtime.check_model_resume(report, "live", "codex")
        for key, value in (("thread", "another-thread"), ("model", "gateway-amber"), ("effort", "low"),
                           ("preceding_turns", [e["turn"] for e in report["events"] if e["event"] == "request"])):
            with self.subTest(field=key), self.assertRaisesRegex(ValueError, "before the next turn"):
                report["idle_promotion"] = {**expected, key: value}
                self.runtime.check_model_resume(report, "live", "codex")

    def test_loaded_resume_requires_new_catalog_and_checks_the_returned_settings(self):
        report = live_model_receipt()
        report["reattached"] = {"thread": report["thread"], "model": "gateway-indigo", "effort": "ultra"}
        with self.assertRaisesRegex(ValueError, "without a provider signal"):
            self.runtime.check_model_resume(report, "loaded", "codex")
        report["events"][5] = {"event": "reattach"}
        self.runtime.check_model_resume(report, "loaded", "codex")
        with self.assertRaisesRegex(ValueError, "must have left the app-server"):
            self.runtime.check_model_resume(report, "unloaded", "codex")
        report["loaded_before_resume"] = [report["thread"]]
        with self.assertRaisesRegex(ValueError, "must have left the app-server"):
            self.runtime.check_model_resume(report, "unloaded", "codex")
        report["loaded_before_resume"] = []
        self.runtime.check_model_resume(report, "unloaded", "codex")
        for field, value in (("thread", "other"), ("model", "gateway-amber"), ("effort", "low")):
            changed = copy.deepcopy(report)
            changed["reattached"][field] = value
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "reattachment did not return"):
                self.runtime.check_model_resume(changed, "loaded", "codex")
        del report["events"][6]
        with self.assertRaises(ValueError):
            self.runtime.check_model_resume(report, "loaded", "codex")

    def test_live_terminal_requires_the_final_rendered_model_and_logical_effort(self):
        report = live_model_receipt()
        del report["process"]
        report["custody"] = model_receipt()["custody"]
        answer = b"Final cedar answer."
        label = "Read Only · ".encode()
        right = label + b"gateway-indigo \x1b[1multra\x1b[0m"
        self.assertEqual(self.runtime.terminal_model_selection(right + b"\r\n" + answer, True, after_reply=False),
                         (b"gateway-indigo", b"ultra"))
        self.assertIsNone(self.runtime.terminal_model_selection(right + answer, True))
        self.assertEqual(self.runtime.terminal_model_selection(
            right + label + b"gateway-amber ultra" + b"\r\n" + answer, True, after_reply=False),
            (b"gateway-amber", b"ultra"))
        report["terminal_completed_turns"] = [report["events"][i]["turn"] for i in (4, 7)]
        report["terminal_view_bytes"] = len(answer + right)
        self.runtime.check_model_resume(report, "live-terminal", "codex", answer + right)
        for raw in (right + answer, answer + right + label + b"gateway-amber ultra",
                    answer + label + b"gateway-indigo low", answer + label + b"gateway-indigo ultrahigh",
                    answer + b"\x1b]0;" + label + b"gateway-indigo ultra\x07"):
            report["terminal_view_bytes"] = len(raw)
            with self.subTest(raw=raw), self.assertRaisesRegex(ValueError, "final terminal status"):
                self.runtime.check_model_resume(report, "live-terminal", "codex", raw)
        report["terminal_view_bytes"] = len(answer)
        with self.assertRaisesRegex(ValueError, "final terminal status"):
            self.runtime.check_model_resume(report, "live-terminal", "codex", answer + right)
        with self.assertRaisesRegex(ValueError, "terminal exit"):
            self.runtime.check_model_resume(report, "live-terminal", "another-codex", answer + right)

    def test_resume_rejects_wrong_first_footer_even_if_the_final_footer_is_correct(self):
        report = model_receipt()
        label = "Read Only · ".encode()
        corrected = b"Cedar marker recalled." + label + b"gateway-indigo ultra"
        for stale in (b"gateway-ochre", b"gateway-ochre low", b"gateway-indigo low"):
            raw = label + stale + b"\r\n" + corrected
            report["terminal_view_bytes"] = len(raw)
            with self.subTest(stale=stale), self.assertRaisesRegex(ValueError, "initial terminal status"):
                self.runtime.check_model_resume(report, "terminal", "codex", raw)

    def test_model_receipt_requires_exit_then_discovery_then_actual_promoted_request(self):
        report = model_receipt()
        terminal = "Cedar marker recalled. Read Only · gateway-indigo ultra".encode()
        report["terminal_view_bytes"] = len(terminal)
        self.runtime.check_model_resume(report, "terminal", "codex", terminal)
        for index, key, value in [(0, "models", ["gateway-amber", "gateway-indigo"]),
                                  (2, "exit", 75), (3, "successor", None),
                                  (4, "model", "gateway-amber"), (4, "model", "gateway-ochre"), (4, "effort", "low"), (4, "effort", "xhigh"),
                                  (4, "thread", "other"), (4, "seed_prompt", False),
                                  (4, "prior_reply", False), (4, "resume_prompt", False)]:
            with self.subTest(index=index, key=key):
                changed = copy.deepcopy(report)
                changed["events"][index][key] = value
                with self.assertRaises(ValueError):
                    self.runtime.check_model_resume(changed, "terminal", "codex", terminal)
        for index in range(len(report["events"])):
            with self.subTest(missing=index):
                changed = copy.deepcopy(report)
                del changed["events"][index]
                with self.assertRaises(ValueError):
                    self.runtime.check_model_resume(changed, "terminal", "codex", terminal)

    def test_live_terminal_requires_completed_distinct_turns(self):
        report = live_model_receipt()
        report["custody"] = model_receipt()["custody"]
        raw = "Final cedar answer. Read Only · gateway-indigo ultra".encode()
        report["terminal_view_bytes"] = len(raw)
        first_reply, next_reply = (report["events"][i]["turn"] for i in (4, 7))
        for incomplete in ([], [first_reply], [next_reply, first_reply], [first_reply, first_reply]):
            with self.subTest(completed=incomplete):
                report["terminal_completed_turns"] = incomplete
                with self.assertRaisesRegex(ValueError, "terminal completion"):
                    self.runtime.check_model_resume(report, "live-terminal", "codex", raw)
        report["terminal_completed_turns"] = [first_reply, next_reply]
        report["events"][-1]["turn"] = first_reply
        with self.assertRaisesRegex(ValueError, "distinct provider turn"):
            self.runtime.check_model_resume(report, "live-terminal", "codex", raw)

    def test_selector_receipts_require_the_original_thread_and_unqualified_command(self):
        raw = b"Cedar marker recalled. Read Only \xc2\xb7 gateway-indigo ultra"
        for surface in ("terminal-last", "exec-last", "picker"):
            with self.subTest(surface=surface):
                report = workspace_model_receipt("codex", "old-codex") if surface == "exec-last" else model_receipt()
                report["seed_custody"] = dict(report["custody"], command=[
                    "old-codex", "--no-daemon", "--no-alt-screen", "Remember the cedar marker from this conversation."])
                turns = [event["turn"] for event in report["events"] if event["event"] == "request"]
                report["terminal_completed_turns"] = turns[:1] if surface == "exec-last" else turns
                if surface == "exec-last":
                    report["process"] = {"argv": ["codex", "exec", "--skip-git-repo-check", "resume", "--last", "continue"],
                                         "returncode": 0, "timed_out": False}
                    report["answered"] = True
                    command = report["process"]["argv"]
                else:
                    command = report["custody"]["command"]
                    if surface == "picker":
                        del command[4:]
                    else:
                        command[4] = "--last"
                    report["terminal_view_bytes"] = len(raw)
                self.runtime.check_model_resume(report, surface, "codex", raw, seed_binary="old-codex")
                with self.assertRaises(ValueError):
                    self.runtime.check_model_resume(report, surface, "codex", raw, seed_binary="different")
                report["events"][-1]["thread"] = "a different conversation"
                with self.assertRaises(ValueError):
                    self.runtime.check_model_resume(report, surface, "codex", raw, seed_binary="old-codex")
                report["events"][-1]["thread"] = report["thread"]
                if surface == "picker":
                    command.append(report["thread"])
                else:
                    command[4] = report["thread"]
                with self.assertRaises(ValueError):
                    self.runtime.check_model_resume(report, surface, "codex", raw, seed_binary="old-codex")

    def test_exec_resume_all_requires_the_second_workspace_and_its_own_history(self):
        for field, value in [("thread", model_receipt()["thread"]), ("turn", model_receipt()["events"][-1]["turn"]),
                             ("model", "gateway-indigo"), ("effort", "low"), ("mixed_history", True),
                             ("prior_reply", False)]:
            with self.subTest(field=field):
                report = workspace_model_receipt("codex", "old-codex")
                report["events"][-2][field] = value
                with self.assertRaisesRegex(ValueError, "workspace resume"):
                    self.runtime.check_exec_resume_workspaces(report, report["events"], "codex", "old-codex")
        report = workspace_model_receipt("codex", "old-codex")
        report["workspace_all"]["argv"].remove("--all")
        with self.assertRaisesRegex(ValueError, "command"):
            self.runtime.check_exec_resume_workspaces(report, report["events"], "codex", "old-codex")

    def backend(self, fixture, method, endpoint, body=None):
        authentication = {'authorizationPresent': True, 'authorizationMatches': True,
                          'accountPresent': True, 'accountMatches': True}
        return fixture.reply(method, '/backend-api/wham/' + endpoint, body, authentication=authentication)

    def usage(self, fixture):
        code, data, _content = self.backend(fixture, 'GET', 'usage')
        self.assertEqual(code, 200)
        return json.loads(data)

    def model_inputs(self):
        first = {'model': self.weekly.MODEL, 'input': [{'type': 'message', 'role': 'user',
                 'content': [{'type': 'input_text', 'text': self.weekly.PROMPT}]}]}
        second = {'model': self.weekly.MODEL, 'input': first['input'] + [
            {'type': 'function_call_output', 'call_id': self.weekly.CALL_ID, 'output': 'Plan updated'}]}
        return first, second

    def seed_usage_wall(self, fixture):
        first, second = self.model_inputs()
        self.assertEqual(fixture.reply('POST', '/v1/responses', first)[0], 200)
        self.assertEqual(fixture.reply('POST', '/v1/responses', second)[0], 429)
        return second

    def consume(self, fixture):
        return self.backend(fixture, 'POST', 'rate-limit-reset-credits/consume',
                            {'redeem_request_id': 'a' * 64, 'credit_id': 'earned-weekly-credit'})

    def test_every_gate_case_is_dispatched_by_the_real_weekly_parser(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            executable = root / 'unexecuted-binary'
            executable.write_bytes(b'parser control; never executed')
            output = root / 'result'
            arguments = ['weekly_reset.py', '--codex', str(executable), '--output', str(output)]
            for name in self.runtime.CASES:
                arguments.extend(('--case', name))
            with mock.patch.object(sys, 'argv', arguments), \
                    mock.patch.object(self.weekly, 'case', side_effect=lambda _binary, _output, name:
                                      {'case': name, 'passed': True}) as execute, \
                    contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(self.weekly.cli(), 0)
            self.assertEqual([call.args[2] for call in execute.call_args_list], list(self.runtime.CASES))
            self.assertEqual(json.loads((output / 'result.json').read_text())['requestedCases'], list(self.runtime.CASES))

    def test_five_hour_and_exact_24h_are_real_preservation_fixtures(self):
        # These requirements are independent of the probe's category lists and
        # the inert dispatch fixture. The actual model must reject a POST.
        scenarios = [('preflight-five-hour', 99, 0, 0, 72),
                     ('preflight-weekly-24h', 0, 99, 100, 24)]
        with mock.patch.object(self.weekly.time, 'time', return_value=1_800_000_000):
            for name, five_hour_used, week_before, week_after, hours in scenarios:
                with self.subTest(name=name):
                    self.assertIn(name, self.runtime.CASES)
                    self.assertEqual(self.runtime.EXPECTATIONS[name], (2, 0, False))
                    fixture = self.weekly.Mock(name)
                    before = self.usage(fixture)
                    self.assertTrue(before['rate_limit']['allowed'])
                    self.assertEqual(before['rate_limit']['primary_window']['used_percent'], five_hour_used)
                    self.assertEqual(before['rate_limit']['secondary_window']['used_percent'], week_before)
                    self.seed_usage_wall(fixture)
                    denied = self.usage(fixture)
                    self.assertFalse(denied['rate_limit']['allowed'])
                    if name == 'preflight-five-hour':
                        window = denied['rate_limit']['primary_window']
                        self.assertEqual(window['used_percent'], 100)
                        wall = next(r for r in fixture.requests if 'quotaResetUnix' in r)
                        self.assertEqual(wall['quotaResetUnix'], window['reset_at'])
                    self.assertEqual(denied['rate_limit']['secondary_window']['used_percent'], week_after)
                    self.assertEqual(denied['rate_limit']['secondary_window']['reset_after_seconds'], hours * 3600)
                    code, inventory, _content = self.backend(fixture, 'GET', 'rate-limit-reset-credits')
                    self.assertEqual(code, 200)
                    self.assertEqual(json.loads(inventory)['available_count'], 1)
                    self.usage(fixture)
                    with self.assertRaisesRegex(RuntimeError, 'more than 24 hours'):
                        self.consume(fixture)
                    self.assertEqual(len(fixture.models), 2)
                    self.assertTrue(fixture.credit)
                    self.assertFalse(fixture.consumed)
                    self.assertEqual(fixture.spent, [])

    def test_beyond_24h_positive_controls_use_the_real_preflight_and_recovery_models(self):
        with mock.patch.object(self.weekly.time, 'time', return_value=1_800_000_000):
            for name, reactive, models, hours in [('preflight-weekly-25h', False, 2, 25),
                                                  ('preflight-weekly-48h', False, 2, 48),
                                                  ('recovery-weekly-25h', True, 3, 25),
                                                  ('recovery-weekly-48h', True, 3, 48),
                                                  ('recovery-weekly-49h', True, 3, 49)]:
                with self.subTest(name=name):
                    self.assertIn(name, self.runtime.CASES)
                    self.assertEqual(self.runtime.EXPECTATIONS[name], (models, 1, True))
                    fixture = self.weekly.Mock(name)
                    if reactive:
                        retained = self.seed_usage_wall(fixture)
                    reading = self.usage(fixture)
                    week = reading['rate_limit']['secondary_window']
                    self.assertEqual((week['used_percent'], week['limit_window_seconds'], week['reset_after_seconds']),
                                     (99, 604800, hours * 3600))
                    self.assertEqual(reading['rate_limit']['allowed'], not reactive)
                    self.backend(fixture, 'GET', 'rate-limit-reset-credits')
                    self.usage(fixture)
                    code, body, _content = self.consume(fixture)
                    self.assertEqual((code, json.loads(body)['code']), (200, 'reset'))
                    self.assertEqual(fixture.spent, ['earned-weekly-credit'])
                    self.assertTrue(self.usage(fixture)['rate_limit']['allowed'])
                    if not reactive:
                        first, retained = self.model_inputs()
                        self.assertEqual(fixture.reply('POST', '/v1/responses', first)[0], 200)
                    code, body, _content = fixture.reply('POST', '/v1/responses', retained)
                    self.assertEqual(code, 200)
                    self.assertIn(self.weekly.ANSWER.encode(), body)
                    self.assertEqual(len(fixture.models), models)


class RuntimeDispatchTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="alab-runtime-dispatch-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.repo = self.root / "source with spaces"
        smoke = self.repo / "alab/smoke"
        smoke.mkdir(parents=True)
        shutil.copy2(ROOT / "alab/check_runtime.py", self.repo / "alab/check_runtime.py")
        shutil.copy2(ROOT / "alab/cli_status.py", self.repo / "alab/cli_status.py")
        shutil.copy2(ROOT / "alab/file_hash.py", self.repo / "alab/file_hash.py")
        (self.repo / "alab/command_guard.py").write_text("# inert dispatch fixture\n")
        shutil.copy2(ROOT / "alab/smoke/provider_fixture.py", smoke / "provider_fixture.py")
        with (smoke / "provider_fixture.py").open("a") as provider:
            provider.write('\nSOURCE="owned source fixture"\n')
        (smoke / "weekly_reset.py").write_text(FIXTURE_PROBE)
        (smoke / "request_recovery.py").write_text(FIXTURE_CAPACITY)
        (smoke / "history_pagination.py").write_text(FIXTURE_HISTORY)
        (smoke / "model_resume.py").write_text(FIXTURE_MODEL)
        for name in ("capacity_exec.py", "quota_resume.py"):
            (smoke / name).write_text("# inert dispatch fixture; no native ownership\n")
        self.executable = self.root / "new codex"
        self.executable.write_text('#!/bin/sh\nprintf "%s\\n" "$1" >> "$2"\n')
        self.executable.chmod(0o755)
        self.env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": str(self.root),
                    "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
                    "GIT_AUTHOR_NAME": "Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
                    "GIT_COMMITTER_NAME": "Fixture", "GIT_COMMITTER_EMAIL": "fixture@example.invalid"}
        self.git("init", "-q", "--template=", "-b", "main")
        self.git("add", ".")
        self.git("commit", "-qm", "runtime dispatch fixture")
        self.source = self.git("rev-parse", "HEAD")
        self.scratch = self.root / "short scratch"
        self.output = self.root / "retained evidence"

    def git(self, *args):
        return subprocess.check_output(["/usr/bin/git", "-C", str(self.repo), *args],
                                       env=self.env, text=True).strip()

    def run_smoke(self, mode="success", source=None, **env):
        return subprocess.run([sys.executable, "-I", "-B", "-S", str(self.repo / "alab/check_runtime.py"),
                               "--source", source or self.source, "--codex", str(self.executable),
                               "--scratch", str(self.scratch), "--output", str(self.output)],
                              cwd=self.repo, env=dict(self.env, FIXTURE_MODE=mode, **env),
                              capture_output=True, text=True, timeout=20)

    def test_dispatch_executes_each_case_and_retains_source_and_binary_bound_evidence(self):
        poison = self.root / "inherited python modules"
        poison.mkdir()
        (poison / "provider_fixture.py").write_text('raise RuntimeError("inherited fixture imported")\n')
        (poison / "file_hash.py").write_text('raise RuntimeError("inherited hash helper imported")\n')
        (poison / "sitecustomize.py").write_text('raise RuntimeError("inherited site imported")\n')
        done = self.run_smoke(PYTHONPATH=str(poison), PYTHONUSERBASE=str(poison))
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        result = json.loads((self.output / "result.json").read_text())
        self.assertIs(result["passed"], True)
        self.assertEqual(result["source"], self.source)
        self.assertEqual(result["requested_cases"], CASES)
        self.assertEqual(result["binary_sha256"], hashlib.sha256(self.executable.read_bytes()).hexdigest())
        self.assertIn("alab/cli_status.py", result["probe_inputs"])
        self.assertEqual(result["probe_inputs"]["alab/cli_status.py"],
                         hashlib.sha256((self.repo / "alab/cli_status.py").read_bytes()).hexdigest())
        self.assertEqual(result["probe_inputs"]["alab/file_hash.py"],
                         hashlib.sha256((self.repo / "alab/file_hash.py").read_bytes()).hexdigest())
        self.assertEqual((self.output / "smoke/history/executed.txt").read_text(), "history\n")
        self.assertIn("history_report_sha256", result)
        capacity = self.output / "smoke/capacity"
        self.assertEqual(result["capacity_exit"], 0)
        self.assertIn("alab/smoke/request_recovery.py", result["probe_inputs"])
        self.assertEqual(result["capacity_report_sha256"], hashlib.sha256((capacity / "result.json").read_bytes()).hexdigest())
        for name in ("overload", "streamed-overload", "late-stream-overload", "tool-stream-overload", "prewarm-overload", "repeated-overload", "unadvised-overload"):
            self.assertEqual((capacity / name / "executed.txt").read_text(), name + "\n")
        for selector in ("picker_model_resume", "terminal_last_model_resume", "exec_last_model_resume"):
            self.assertEqual(result[selector + "_exit"], 0)
        self.assertEqual(result["loaded_model_resume_exit"], 0)
        self.assertEqual(result["live_model_resume_exit"], 0)
        self.assertEqual(result["live_terminal_model_resume_exit"], 0)
        live_report = self.output / "smoke/live-model-resume/result.json"
        self.assertEqual(result["live_model_resume_report_sha256"], hashlib.sha256(live_report.read_bytes()).hexdigest())
        model = self.output / "smoke/model-resume"
        self.assertEqual((model / "executed.txt").read_text(), "model-resume\n")
        self.assertEqual(result["model_resume_report_sha256"], hashlib.sha256((model / "result.json").read_bytes()).hexdigest())
        self.assertEqual(result["probe_inputs"]["alab/smoke/model_resume.py"],
                         hashlib.sha256((self.repo / "alab/smoke/model_resume.py").read_bytes()).hexdigest())
        for name in CASES:
            self.assertEqual((self.output / "smoke" / name / "executed.txt").read_text(), name + "\n")
            self.assertEqual(json.loads((self.output / "smoke" / name / "requests.json").read_text())["source"],
                             "owned source fixture")

    def test_capacity_failure_or_incomplete_coverage_cannot_pass(self):
        for mode, expected in (("failed-capacity", 23), ("capacity-missing", 1),
                               ("capacity-extra-request", 1), ("capacity-no-delay", 1)):
            with self.subTest(mode=mode):
                self.output = self.root / (mode + " evidence")
                self.scratch = self.root / (mode + " scratch")
                done = self.run_smoke(mode)
                self.assertEqual(done.returncode, expected, done.stdout + done.stderr)
                record = json.loads((self.output / "result.json").read_text())
                self.assertIs(record["passed"], False)
                self.assertIn("capacity", record["error"])
                self.assertTrue((self.output / "smoke/capacity/tool-stream-overload/executed.txt").is_file())

    def test_passing_capacity_report_must_agree_with_turn_output_and_provider_timing(self):
        for mode, error in (("capacity-early", "Retry-After bypassed"),
                            ("capacity-missing-answer", "one recovered answer"),
                            ("capacity-dropped-tool-result", "changed the input or completed tool output"),
                            ("capacity-preexisting-work", "initial request already contained"),
                            ("capacity-extra-stream-input", "append only the tool call"),
                            ("capacity-steered-retries", "new user message"),
                            ("capacity-wrong-answer", "one recovered answer"),
                            ("capacity-duplicate-answer", "one recovered answer"),
                            ("capacity-early-answer", "one recovered answer"),
                            ("capacity-late-answer", "one recovered answer"),
                            ("capacity-answer-before-notice", "notice outside the wait"),
                            ("capacity-notice-before-turn", "notice outside the wait"),
                            ("capacity-silent-wait", "one advisory capacity notice per refusal"),
                            ("capacity-silent-second-wait", "one advisory capacity notice per refusal"),
                            ("capacity-wrong-stream-reason", "selected wait and capacity reason"),
                            ("capacity-changed-model", "configured model"),
                            ("capacity-changed-reasoning", "original reasoning settings"),
                            ("capacity-changed-turn", "changed turns"),
                            ("capacity-different-saved-turn", "rollout did not start and finish"),
                            ("capacity-saved-failure", "saved a failure for a recovered turn"),
                            ("capacity-lost-saved-output", "completed plan output is missing"),
                            ("capacity-saved-steering", "single original UI user message"),
                            ("capacity-saved-user-input", "original prompt alone"),
                            ("capacity-unattributed-input", "original prompt and attributed context"),
                            ("capacity-other-handshake-turn", "changed turns"),
                            ("capacity-other-exec-thread", "another exec thread"),
                            ("capacity-late-exec-thread", "thread started after"),
                            ("capacity-missing-turn-metadata", "original exec thread and turn identity"),
                            ("capacity-restarted-turn", "capacity turn evidence for tool-stream-overload"),
                            ("capacity-fatal-wait", "capacity exec output for overload emitted a top-level JSON error"),
                            ("capacity-unfinished-turn", "capacity turn evidence for prewarm-overload"),
                            ("capacity-turn-summary-disagrees", "capacity turn evidence for overload"),
                            ("capacity-stale-notice", "capacity reason"),
                            ("capacity-unchanged-reason", "expected capacity reasons"),
                            ("capacity-forbidden-trace", "request 3 for repeated-overload expected HTTP 503"),
                            ("capacity-denied-request", "request 2 for overload expected HTTP 503"),
                            ("capacity-refused-stream", "request 1 for tool-stream-overload expected HTTP 200"),
                            ("capacity-missing-final-status", "request 4 for prewarm-overload expected HTTP 200"),
                            ("capacity-noninteger-status", "request 1 for streamed-overload expected HTTP 200"),
                            ("capacity-first-early", "Retry-After bypassed"),
                            ("capacity-waited-for-eof", "while the failed SSE body was open"),
                            ("stream-advice-still-live", "after the opening deadline"),
                            ("stream-skipped-backoff", "backoff bypassed after the stream failure"),
                            ("stream-rearmed-advice", "announce the local capacity wait"),
                            ("capacity-late-first-notice", "not visible during"),
                            ("capacity-buffered-notices", "not visible during"),
                            ("capacity-buffered-startup-notice", "not visible during"),
                            ("capacity-cr-only", "Extra data"),
                            ("capacity-understated-notice", "did not report the selected wait"),
                            ("local-first-early", "local capacity backoff bypassed"),
                            ("local-reset", "local capacity backoff bypassed"),
                            ("local-shortened-by-header", "local capacity backoff bypassed"),
                            ("local-buffered-notices", "not visible during"),
                            ("local-understated-notice", "capacity notice did not report the selected wait"),
                            ("local-overstated-notice", "too early for its announced wait"),
                            ("local-invalid-short-header", "with Retry-After 1"),
                            ("capacity-missing-header", "with Retry-After 2"),
                            ("capacity-wrong-header", "with Retry-After 2"),
                            ("capacity-conflicting-startup-header", "Retry-After override"),
                            ("local-unexpected-advice", "without Retry-After"),
                            ("local-extra-header", "Retry-After override"),
                            ("capacity-startup-early", "Retry-After bypassed"),
                            ("capacity-racing-fallback", "preceding response headers"),
                            ("capacity-racing-plan", "preceding response headers"),
                            ("capacity-second-early", "Retry-After bypassed"),
                            ("capacity-backwards", "headers precede their request"),
                            ("capacity-wrong-delay", "disagrees")):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory(dir=self.root) as case:
                self.output = Path(case) / "retained evidence"
                self.scratch = Path(case) / "short scratch"
                done = self.run_smoke(mode)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                record = json.loads((self.output / "result.json").read_text())
                self.assertEqual(record["capacity_exit"], 0)
                self.assertIs(record["passed"], False)
                self.assertIn(error, record["error"])
                self.assertTrue((self.output / "smoke/capacity/overload/requests.json").is_file())

    def test_capacity_success_requires_the_guardians_matching_process_exit(self):
        for suffix in ("timeout", "failure", "other-case", "other-binary", "missing"):
            with self.subTest(suffix=suffix):
                mode = "capacity-guardian-" + suffix
                self.output = self.root / (mode + " evidence")
                self.scratch = self.root / (mode + " scratch")
                done = self.run_smoke(mode)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                record = json.loads((self.output / "result.json").read_text())
                self.assertEqual(record["capacity_exit"], 0)
                self.assertIs(record["passed"], False)
                self.assertIn("overload", record["error"])
                expected = "exec-guardian-result.json" if suffix == "missing" else "guardian did not confirm"
                self.assertIn(expected, record["error"])

    def test_changed_shared_cli_policy_cannot_leave_a_passing_runtime_record(self):
        helper = self.repo / "alab/cli_status.py"
        before = helper.read_bytes()
        done = self.run_smoke("changed-cli-status")
        self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
        result = json.loads((self.output / "result.json").read_text())
        self.assertIs(result["passed"], False)
        self.assertIn("source inputs changed", result["error"])
        self.assertEqual(result["probe_inputs"]["alab/cli_status.py"], hashlib.sha256(before).hexdigest())
        self.assertNotEqual(helper.read_bytes(), before)
        self.assertEqual(result["history_exit"], 0)

    def test_changed_hash_helper_in_either_probe_cannot_leave_a_passing_runtime_record(self):
        helper = self.repo / "alab/file_hash.py"
        before = helper.read_bytes()
        for mode in ("changed-file-hash", "changed-history-file-hash"):
            with self.subTest(mode=mode):
                helper.write_bytes(before)
                self.output = self.root / (mode + " evidence")
                self.scratch = self.root / (mode + " scratch")
                done = self.run_smoke(mode)
                self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
                result = json.loads((self.output / "result.json").read_text())
                self.assertIs(result["passed"], False)
                self.assertIn("source inputs changed", result["error"])
                self.assertEqual(result["probe_inputs"]["alab/file_hash.py"], hashlib.sha256(before).hexdigest())
                self.assertNotEqual(helper.read_bytes(), before)

    def test_failed_runtime_child_propagates_its_failure_and_keeps_diagnostics(self):
        done = self.run_smoke("failure")
        self.assertEqual(done.returncode, 23, done.stdout + done.stderr)
        result = json.loads((self.output / "result.json").read_text())
        self.assertIs(result["passed"], False)
        self.assertEqual(result["exit"], 23)
        self.assertTrue((self.output / "smoke/result.json").is_file())

    def retention_fixture(self, stop, obstruction, label, *, late_write=False):
        self.output = self.root / (label + " evidence")
        self.scratch = self.root / (label + " scratch")
        probe = self.repo / "alab/smoke/weekly_reset.py"
        body = FIXTURE_PROBE
        tail = 'raise SystemExit(23 if mode=="failure" else 0)'
        replacement = (
            'destination = Path(os.environ["RETENTION_OUTPUT"])\n'
            'obstruction = os.environ["RETENTION_OBSTRUCTION"]\n'
            'if obstruction == "diagnostics":\n'
            '    (destination / "smoke/result.json").mkdir(parents=True)\n'
            'elif obstruction == "record":\n'
            '    (destination / "result.json").mkdir()\n'
            'stop = os.environ["RETENTION_STOP"]\n'
            'if stop == "signal":\n'
            '    import signal\n'
            '    os.kill(os.getpid(), signal.SIGTERM)\n'
            'raise SystemExit(int(stop))')
        self.assertEqual(body.count(tail), 1)
        probe.write_text(body.replace(tail, replacement))
        self.git('add', 'alab/smoke/weekly_reset.py')
        if self.git('diff', '--cached', '--name-only'):
            self.git('commit', '-qm', 'owned runtime retention fixture')
        self.source = self.git('rev-parse', 'HEAD')
        mode = 'success' if stop == 0 else 'failure'
        environment = dict(RETENTION_OUTPUT=str(self.output),
                           RETENTION_OBSTRUCTION=obstruction, RETENTION_STOP=str(stop))
        if late_write:
            # The public main still runs in its own process. Fail only after
            # the real record bytes have reached its owned temporary/output file.
            bootstrap = """import errno,json,runpy,sys
from pathlib import Path
api=runpy.run_path(sys.argv.pop(1))
marker=Path(sys.argv.pop(1))
original=api['json'].dump
def fail_after_real_write(record, stream, **kwargs):
    original(record, stream, **kwargs)
    stream.flush()
    written=Path(stream.name).read_text()
    marker.write_text(json.dumps({'bytes':len(written.encode()),
        'passed':json.loads(written)['passed']}))
    raise OSError(errno.EIO, 'owned injected error after real record bytes were written')
api['json'].dump=fail_after_real_write
sys.exit(api['main']())
"""
            command = [sys.executable, '-I', '-B', '-S', '-c', bootstrap,
                       str(self.repo / 'alab/check_runtime.py'), str(self.root / (label + ' write marker.json')),
                       '--source', self.source, '--codex', str(self.executable),
                       '--scratch', str(self.scratch), '--output', str(self.output)]
            done = subprocess.run(command, cwd=self.repo,
                                  env=dict(self.env, FIXTURE_MODE=mode, **environment),
                                  capture_output=True, text=True, timeout=20)
        else:
            done = self.run_smoke(mode, **environment)
        if stop != 0:
            self.assertFalse((self.scratch / 'history').exists())
        self.assertTrue((self.scratch / 'result.json').is_file())
        return done

    def test_failed_probe_status_survives_diagnostic_copy_failure(self):
        for stop, public, raw in [(23, 23, 23), (75, 75, 75), ('signal', 143, -15)]:
            if stop == 'signal' and os.name != 'posix':
                continue
            with self.subTest(stop=stop):
                control = self.retention_fixture(stop, 'none', f'copy control {stop}')
                self.assertEqual(control.returncode, public, control.stdout + control.stderr)
                self.assertEqual(json.loads((self.output / 'result.json').read_text())['exit'], raw)
                done = self.retention_fixture(stop, 'diagnostics', f'copy obstructed {stop}')
                self.assertEqual(done.returncode, public, done.stdout + done.stderr)
                record = json.loads((self.output / 'result.json').read_text())
                self.assertIs(record['passed'], False)
                self.assertEqual(record['exit'], raw)
                self.assertIn('app runtime smoke failed', record['error'])
                self.assertIn('Is a directory', record['diagnostics_error'])
                self.assertIn('diagnostic retention failed', done.stderr)
                self.assertTrue((self.output / 'smoke/result.json').is_dir())

    def test_failed_probe_status_survives_record_write_failure(self):
        for stop, public in [(23, 23), (75, 75), ('signal', 143)]:
            if stop == 'signal' and os.name != 'posix':
                continue
            with self.subTest(stop=stop):
                done = self.retention_fixture(stop, 'record', f'record obstructed {stop}')
                self.assertEqual(done.returncode, public, done.stdout + done.stderr)
                self.assertIn('record retention failed', done.stderr)
                self.assertTrue((self.output / 'result.json').is_dir())
                self.assertTrue((self.output / 'smoke/result.json').is_file())

    def test_successful_probe_cannot_pass_when_diagnostic_copy_fails(self):
        control = self.retention_fixture(0, 'none', 'successful retention control')
        self.assertEqual(control.returncode, 0, control.stdout + control.stderr)
        self.assertIs(json.loads((self.output / 'result.json').read_text())['passed'], True)
        done = self.retention_fixture(0, 'diagnostics', 'successful copy obstructed')
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        self.assertTrue((self.output / 'result.json').is_file(), done.stdout + done.stderr)
        record = json.loads((self.output / 'result.json').read_text())
        self.assertIs(record['passed'], False)
        self.assertEqual(record['exit'], 0)
        self.assertEqual(record['history_exit'], 0)
        self.assertIn('Is a directory', record['diagnostics_error'])
        self.assertIn('diagnostic retention failed', done.stderr)

    def test_successful_probe_cannot_pass_when_record_write_fails(self):
        done = self.retention_fixture(0, 'record', 'successful record obstructed')
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        self.assertIn('record retention failed', done.stderr)
        self.assertTrue((self.output / 'result.json').is_dir())
        self.assertTrue((self.output / 'smoke/history/result.json').is_file())

    def test_late_record_write_failure_never_publishes_success_or_masks_a_stop(self):
        for stop in (75, 0):
            with self.subTest(stop=stop):
                label = f'late record write {stop}'
                done = self.retention_fixture(stop, 'none', label, late_write=True)
                self.assertEqual(done.returncode, stop or 1, done.stdout + done.stderr)
                marker = json.loads((self.root / (label + ' write marker.json')).read_text())
                self.assertGreater(marker['bytes'], 0)
                self.assertEqual(marker['passed'], stop == 0)
                self.assertFalse((self.output / 'result.json').exists())
                self.assertEqual(list(self.output.glob('.runtime-record-*')), [])
                self.assertIn('record retention failed', done.stderr)

    def test_exec_resume_receipt_binds_the_invocation_to_the_original_thread(self):
        done = self.run_smoke("wrong-exec-thread")
        self.assertNotEqual(done.returncode, 0)
        record = json.loads((self.output / "result.json").read_text())
        self.assertIs(record["passed"], False)
        self.assertEqual(record["model_resume_exit"], 0)
        self.assertEqual(record["exec_model_resume_exit"], 0)
        self.assertIn("exec resume did not complete the original thread", record["error"])

    def test_failed_model_resume_preserves_child_status_and_report(self):
        done = self.run_smoke("failed-model-resume")
        self.assertEqual(done.returncode, 37, done.stdout + done.stderr)
        record = json.loads((self.output / "result.json").read_text())
        self.assertIs(record["passed"], False)
        self.assertEqual(record["model_resume_exit"], 37)
        self.assertTrue((self.output / "smoke/model-resume/result.json").is_file())

    def test_failed_history_keeps_its_explicit_log_after_scratch_disposal_only(self):
        done = self.run_smoke("failed-history")
        self.assertEqual(done.returncode, 29, done.stdout + done.stderr)
        result = json.loads((self.output / "result.json").read_text())
        self.assertIs(result["passed"], False)
        self.assertEqual(result["history_exit"], 29)
        self.assertEqual((self.output / "smoke/history/executed.txt").read_text(), "history\n")
        retained = self.output / "smoke/history/logs/codex-tui.log"
        self.assertFalse(retained.is_symlink())
        shutil.rmtree(self.scratch)
        self.assertEqual(retained.read_text(), "retained history failure\n")
        for private in ("home", "codex-home", "logs/linked-auth.json"):
            self.assertFalse((self.output / "smoke/history" / private).exists())
            self.assertFalse((self.output / "smoke/history" / private).is_symlink())

    def test_zero_exit_cannot_hide_missing_cases_or_a_redemption_in_a_negative_case(self):
        for mode in ("noisy-empty-check", "silent-paid-retry", "mislabeled-paid-retry", "duplicate-renewal", "second-paid-attempt", "repeat-paid-notice",
                     "missing-report", "missing-case", "bad-redemption", "five-hour-redemption",
                     "24h-redemption", "bad-25h", "bad-48h", "bad-49h", "changed-binary",
                     "changed-cli-status", "changed-history-cli-status", "bad-history", "bad-model-receipt",
                     "live-stays-on-old-model", "changed-live-seed", "stale-visible-model"):
            with self.subTest(mode=mode):
                self.output = self.root / (mode + " evidence")
                self.scratch = self.root / (mode + " scratch")
                done = self.run_smoke(mode)
                self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
                self.assertIs(json.loads((self.output / "result.json").read_text())["passed"], False)

    def test_runtime_case_arguments_reach_the_real_probe_before_executable_validation(self):
        dispatch = runpy.run_path(str(ROOT / "alab/check_runtime.py"))
        output = self.root / "real probe admission"
        missing = self.root / "absent codex"
        command = [sys.executable, "-I", "-B", "-S", "-c", dispatch["BOOTSTRAP"],
                   str(ROOT / "alab/smoke/weekly_reset.py"), "--codex", str(missing),
                   "--output", str(output)]
        for name in dispatch["CASES"]:
            command.extend(("--case", name))
        done = subprocess.run(command, cwd=self.repo, env=self.env,
                              capture_output=True, text=True, timeout=20)
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        report = json.loads((output / "result.json").read_text())
        self.assertEqual(report["requestedCases"], list(dispatch["CASES"]))
        self.assertIs(report["passed"], False)
        self.assertEqual(report["cases"], [])
        self.assertIn("FileNotFoundError", report["error"])
        self.assertFalse(missing.exists())

    def test_stale_evidence_or_wrong_source_never_executes_the_probe(self):
        done = self.run_smoke(source="f" * 40)
        self.assertNotEqual(done.returncode, 0)
        self.assertFalse(self.scratch.exists())
        self.output.mkdir()
        (self.output / "result.json").write_text('{"passed":true}')
        done = self.run_smoke()
        self.assertNotEqual(done.returncode, 0)
        self.assertFalse(self.scratch.exists())
        self.assertEqual((self.output / "result.json").read_text(), '{"passed":true}')


if __name__ == "__main__":
    unittest.main()
