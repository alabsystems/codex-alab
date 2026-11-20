# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Evidence for an exec handoff on a named limit, including its nonpackaged test driver."""
import json
import re
import subprocess

import capacity_exec as probe
import renewal_follow as native


def test_driver(args):
    probe.require(re.fullmatch(r"[0-9a-f]{40}", args.test_driver_source or "")
                  and re.fullmatch(r"[0-9a-f]{64}", args.test_driver_sha256 or ""),
                  "the named exec test driver needs its exact source and SHA-256")
    codex = args.test_driver.resolve(strict=True)
    digest = native.image_digest(codex)[1]
    probe.require(digest == args.test_driver_sha256, "the integration test image differs from its build")
    return codex, {"evidenceClass": "integration-test", "testDriver": {
        "attestedSource": args.test_driver_source, "sha256": digest, "path": str(codex)}}


def require_declaration(codex, record, env, root):
    declaration = subprocess.run([codex, "--describe-named-exec-probe"], env=env, cwd=root,
                                 check=True, stdin=subprocess.DEVNULL, capture_output=True, timeout=10)
    description = json.loads(declaration.stdout)
    probe.require(description == {"schema": 1, "feature": "named-exec-probe", "limit": native.provider_fixture.GOLDEN_MODEL_LIMIT,
                                  "profile": "debug", "source": record["attestedSource"]},
                  "the test driver does not declare its expected debug feature, quota and source")
    record["declaration"] = description


def summary(model, root):
    with model.lock:
        events = list(model.http_events)
        requests = list(model.requests)
    wall = next((r for r in requests if r.get("responseHeaders", {}).get("x-codex-active-limit") == model.limit
                 and r.get("refusalNamedReset") is not None), None)
    _, _, reads = native.native_usage_between_requests(events)
    return {
        "providerCalls": [(e.get("account"), e["status"], e.get("routingAccountMatched")) for e in events
                          if e.get("kind") == "foreground" and "status" in e],
        "refusalHeaders": wall["responseHeaders"] if wall else {},
        "nativeNamedUsage": native.named_usage_readings(reads, model.limit),
        "latestNativeUsageMatched": native.latest_handoff_usage_matches(reads, model.limit),
        "requestIdentities": [r.get("requestIdentity") for r in requests],
        "foregroundModels": [r["body"].get("model") for r in requests],
        "nativeActions": [e["action"] for e in native.native_actions(root) if e["replyOk"]],
        "receiptCount": len(list((root / "host-state/accounts-codex/renewal-follow").glob("*.json"))),
    }


def verify_provider(root, thread, turn, requests, evidence):
    probe.require(evidence["providerCalls"] == [("source", 200, True), ("source", 429, True), ("target", 200, True)]
                  and evidence["foregroundModels"] == [probe.MODEL] * 3,
                  "model or login did not match the expected handoff")
    bound = {"thread_id": thread, "turn_id": turn}
    probe.require(evidence["requestIdentities"] == [bound] * 3 and requests[-1]["body"]["input"] == requests[1]["body"]["input"],
                  "the resumed model request changed the original thread, turn or refused input")
    native.provider_fixture.completed_plan_records(requests[-1]["body"]["input"])
    rollout = native.thread_rollout(root, thread)
    native.provider_fixture.check_unsteered_input(rollout, requests)
    terminal = [(e["payload"]["type"], e["payload"].get("turn_id")) for e in rollout if e.get("type") == "event_msg"
                and e["payload"].get("type") in ("task_complete", "turn_complete", "turn_aborted")]
    probe.require(terminal in ([("turn_complete", turn)], [("task_complete", turn)]),
                  "the durable original turn did not complete exactly once")


def verify(model, root, receipt, requests, evidence):
    verify_provider(root, receipt["intent"]["thread"], receipt["intent"]["wall"], requests, evidence)
    probe.require(evidence["refusalHeaders"].get("x-codex-active-limit") == model.limit and
                  evidence["latestNativeUsageMatched"], "latest native reads do not support the named quota decision")
    intent = receipt["intent"]
    probe.require(evidence["receiptCount"] == 1 and intent.get("limit") == model.limit
                  and intent["purpose"] == "weekly_quota" and receipt["accepted_turn"] == intent["wall"],
                  "native receipt did not consume the original named wall")
    expected = ["exec-arm", "exec-reserve", "exec-execute", "exec-load", "exec-consume", "exec-complete"]
    proved = [e["action"] for e in native.native_actions(root) if e["replyOk"] and
              (e["action"] == "exec-arm" or e["operation"] == receipt["id"])]
    probe.require(list(dict.fromkeys(proved)) == expected, "native handoff did not settle every stage on its receipt")
    for generation, identity, auth in (("old", intent["identity"], receipt["old_auth"]),
                                       ("new", receipt["destination"], receipt["new_auth"])):
        probe.require(identity == native.fixture_renewal_identity(generation) and
                      auth == str(root / f"auth-{generation}" / "auth.json"),
                      "receipt account or user differs from its fixture login")
    evidence.update(receiptId=receipt["id"], receiptLimit=intent["limit"],
                    originalInputAndCompletedPlanPreserved=True)
