#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Qualify same-account renewal using a real headless native host and Codex PTY.

Requires the Rust-only native_host fixture executable and a canonical Codex package.
All accounts, credentials and HTTP traffic are generated inside --output.
"""

import argparse
import base64
from datetime import datetime, timezone
import json
import hashlib
import http.server
import threading
import ssl
from pathlib import Path
import select
import os
import re
import socket
import subprocess
import sys
import time

import capacity_exec as probe
import native_host_record
import provider_fixture


# (causal-outcome hang detector, guardian), in seconds. These bound only the
# qualification process; native operation deadlines and policy clocks are unchanged.
CASE_BUDGETS = {
    "happy": (270, 300),
    "weekly-quota": (270, 300),
    "weekly-quota-rollback": (450, 480),
    "weekly-quota-exhausted": (270, 300),
    "weekly-quota-chain": (450, 480),
    "lost-signal-ack": (270, 300),
    "panic-before-signal": (270, 300),
    "lost-consume-ack": (270, 300),
    "late-hold": (270, 300),
    "owner-stop": (270, 300),
    "owner-stop-after-signal": (270, 300),
    "grace-beyond-budget": (720, 750),
    "failed-forward-rollback": (450, 480),
    "successor-prefix-crash": (450, 480),
    "post-signal-resume": (390, 420),
    "post-signal-expiry-diagnostic": (390, 420),
    # A five-hour wall whose reset is within fifteen minutes is waited out on its own
    # account. The far-reset case (handed to another account) belongs to the commit
    # that makes alab-quota-recovery yield a five-hour wall; it is not written here.
    "five-hour-wall-soon": (270, 300),
}
# How long after the provider refused it the five-hour-wall-soon source is usable again,
# and how far off that refusal says its window resets.
WALL_LIFT_SECONDS = 20
WALL_RESET_SECONDS = 600


TITLE_FORMAT = {"type": "json_schema", "strict": True, "name": "codex_output_schema",
    "schema": {"type": "object", "properties": {"title": {"type": "string", "minLength": 1,
        "maxLength": 36}}, "required": ["title"], "additionalProperties": False}}
TITLE_INSTRUCTIONS = "Generate a concise, single-line task title"


def request_kind(body):
    if not isinstance(body, dict) or not isinstance(body.get("input"), list):
        return "unknown"
    tools = body.get("tools")
    title_instruction = any(item.get("role") == "user" and any(
        part.get("type") == "input_text" and isinstance(part.get("text"), str)
        and part["text"].startswith(TITLE_INSTRUCTIONS)
        for part in item.get("content", []) if isinstance(part, dict))
        for item in body["input"] if isinstance(item, dict) and isinstance(item.get("content"), list))
    text = body.get("text")
    # JSON type equality matters: Python would otherwise equate true with 1.
    title_format = isinstance(text, dict) and json.dumps(text.get("format"), sort_keys=True) == json.dumps(
        TITLE_FORMAT, sort_keys=True)
    if tools == [] and title_instruction and title_format:
        return "thread-title"
    if not title_instruction and not title_format and isinstance(tools, list) and any(
        tool.get("type") == "function" and tool.get("name") == "update_plan"
        for tool in tools if isinstance(tool, dict)):
        return "foreground"
    return "unknown"


def validate_durable_plan(events, wall, expected_starts=2):
    """Check persisted execution evidence; PlanUpdate UI events are not persisted."""
    starts = []
    calls = []
    outputs = []
    for index, event in enumerate(events):
        payload = event.get("payload", {})
        kind = payload.get("type")
        if event.get("type") == "event_msg":
            probe.require(kind != "plan_update", "unexpected persisted plan UI event")
            if kind in ("task_started", "turn_started"):
                starts.append((index, payload.get("turn_id")))
        if event.get("type") == "response_item":
            if kind in ("function_call", "custom_tool_call", "tool_search_call"):
                calls.append((index, payload))
            elif kind in ("function_call_output", "custom_tool_call_output", "tool_search_output"):
                outputs.append((index, payload))
    probe.require([turn for _, turn in starts] == [wall] * expected_starts,
                  "native recovery changed or duplicated original turn")
    probe.require(len(calls) == 1 and len(outputs) == 1,
                  "completed plan call or output was lost or duplicated")
    call_index, call = calls[0]
    output_index, output = outputs[0]
    probe.require(call.get("type") == "function_call" and call.get("name") == "update_plan"
        and call.get("call_id") == probe.CALL_ID,
        "completed plan call identity changed")
    try:
        arguments = json.loads(call.get("arguments", ""))
    except (TypeError, ValueError):
        arguments = None
    probe.require(arguments == {"plan": [{"step": probe.PLAN, "status": "completed"}]},
                  "completed plan arguments changed")
    probe.require(output.get("type") == "function_call_output"
        and output.get("call_id") == probe.CALL_ID and output.get("output") == "Plan updated",
        "completed plan output changed")
    probe.require(starts[0][0] < call_index < output_index < starts[1][0],
                  "completed plan did not finish exactly once before native recovery")
    return [turn for _, turn in starts]


ORDINARY_PACED_RULES = {"continue@v1", "policy-continue@v1", "answer@v1",
    "prose-long-term@v1", "prose-short-term@v1", "continue-done-check@v1",
    "land@v1", "land-nudge@v1", "land-answer@v1", "harness-turn@v1"}


def recovery_marker(owed):
    if owed is None:
        return None
    probe.require(isinstance(owed, dict), "invalid owed recovery record")
    markers = [owed[key] for key in ("renewal", "rotation") if owed.get(key) is not None]
    probe.require(len(markers) <= 1 and all(isinstance(marker, dict) for marker in markers),
                  "ambiguous or invalid account recovery marker")
    return markers[0] if markers else None


def successor_rest(record, thread):
    """Recognize a settled native handoff, including a known ordinary paced wait."""
    if record.get("sessionId") != thread or record.get("status") != "idle" or record.get("draftEmpty") is not True:
        return None
    if any(record.get(key) is not None and record.get(key) is not False for key in
           ("queuedInput", "pendingUserTurn", "popupOpen", "editorOpen", "recovery", "inFlightRetry")):
        return None
    owed = record.get("owed")
    if owed is None:
        return {"kind": "none", "draftEmpty": True}
    probe.require(isinstance(owed, dict), "unknown successor owed record")
    if owed.get("nativeOrigin") is not None or recovery_marker(owed) is not None:
        return None
    probe.require(set(owed).issubset({"thread", "nativeOrigin", "renewal", "rotation", "rule", "text", "mode",
        "notBefore", "repoOwed", "resumeGoal", "landing", "landed"})
        and owed.get("thread") == thread and owed.get("rule") in ORDINARY_PACED_RULES
        and owed.get("mode") in ("cruise", "land")
        and isinstance(owed.get("text"), str) and bool(owed["text"].strip())
        and type(owed.get("notBefore")) is int and owed["notBefore"] >= 0
        and owed.get("resumeGoal", False) is False and owed.get("landing", False) is False
        and owed.get("landed") is False
        and (owed.get("repoOwed") is None or (isinstance(owed["repoOwed"], list)
            and all(isinstance(item, str) for item in owed["repoOwed"]))),
        "successor has unknown ordinary work instead of cleared native custody")
    return {"kind": "ordinary-paced", "rule": owed["rule"], "mode": owed["mode"],
        "notBefore": owed["notBefore"], "landed": False, "textPresent": True,
        "repoOwedCount": len(owed.get("repoOwed") or []), "draftEmpty": True}


def validate_completed_original_turn(events, wall, expected_starts=2):
    starts = validate_durable_plan(events, wall, expected_starts)
    boundaries = [(index, event["payload"]) for index, event in enumerate(events)
        if event.get("type") == "event_msg" and event.get("payload", {}).get("type") in
        ("task_started", "turn_started", "task_complete", "turn_complete", "turn_aborted")]
    probe.require([payload.get("turn_id") for _, payload in boundaries] == [wall] * (2 * expected_starts)
        and [payload["type"] for _, payload in boundaries] in
        (["task_started", "task_complete"] * expected_starts,
         ["turn_started", "turn_complete"] * expected_starts),
        "original native turn did not complete exactly once after recovery")
    return starts


# Each fixture login's role, and the identity its token claims carry (fake_auth).
QUOTA_ACCOUNTS = {"old": ("source", "source-account", "fixture-user"),
                  "new": ("target", "target-account", "target-user"),
                  "next": ("final", "final-account", "final-user")}
CREDITS_PATH = "/rate-limit-reset-credits"


def monotonic_us():
    """The clock the native host stamps its control trace with."""
    return time.clock_gettime_ns(time.CLOCK_MONOTONIC) // 1000


def token_part(value):
    return base64.urlsafe_b64encode(json.dumps(value).encode()).rstrip(b"=").decode()


def access_token(generation, account="fixture-account", *, email="fixture@example.invalid",
                 user="fixture-user"):
    """A fixture login's access token: an unsigned JWT, as a real one is a JWT.

    Its payload carries the identity claims of the login's id token, an expiry thirty
    days off (so no client refreshes it) and the private claim `fixture_generation`,
    by which this fixture knows the login. It has no chatgpt_account_user_id.
    """
    return token_part({"alg": "none"}) + "." + token_part({
        **identity_claims(account, email, user), "exp": int(time.time()) + 30 * 86400,
        "fixture_generation": generation}) + ".sig"


def identity_claims(account, email, user):
    return {"sub": user, "email": email, "https://api.openai.com/auth": {
        "chatgpt_user_id": user, "chatgpt_account_id": account, "chatgpt_plan_type": "pro"}}


def bearer_generation(headers):
    """Which fixture login a request's bearer belongs to: its access token's
    `fixture_generation`, or None. Nothing of the token is kept."""
    value = headers.get("authorization") or headers.get("Authorization") or ""
    scheme, _, token = value.partition(" ")
    parts = token.split(".")
    if scheme != "Bearer" or len(parts) != 3:
        return None
    try:
        payload = json.loads(base64.urlsafe_b64decode(parts[1] + "=" * (-len(parts[1]) % 4)))
    except ValueError:
        return None
    generation = payload.get("fixture_generation") if isinstance(payload, dict) else None
    return generation if isinstance(generation, str) else None


def request_account(headers):
    """Which fixture login sent a request; never its bearer text."""
    return QUOTA_ACCOUNTS.get(bearer_generation(headers), (None,))[0]


def request_client(headers):
    """Which program sent a request: aterm's usage reader names itself exactly
    `codex-cli`; codex-alab's user agent begins with its originator and version."""
    return "host" if headers.get("user-agent") == "codex-cli" else "codex"


def require_no_fixture_fault(model):
    """A reply the fixture could not build correctly fails the run where it is seen.

    The reply is built on a server thread, whose exception only closes the connection.
    The client reads that as "usage unknown" and keeps a walled turn polling until the
    hang detector, so the controller's own loop has to look.
    """
    with model.lock:
        fault = model.fixture_faults[0] if model.fixture_faults else None
    probe.require(fault is None, "fixture served a reply the client cannot decode"
        + (f" on {fault['path']}: {fault['error']}" if fault else ""))


def live_snapshot_outcomes(trace):
    """The host's live-snapshot verdicts in order, for a run that reached no outcome."""
    outcomes = []
    try:
        for line in trace.read_text().splitlines():
            event = json.loads(line)
            if event.get("kind") == "live-snapshot" and isinstance(event.get("outcome"), str):
                outcomes.append(event["outcome"][:64])
    except (OSError, ValueError, AttributeError):
        pass
    return "live snapshot: " + (", ".join(outcomes[-8:]) if outcomes else "none traced")


def wall_evidence(events, trace):
    """What followed the first weekly wall, for a quota run that reached no outcome."""
    def path(event):
        return event["path"].split("?", 1)[0]
    wall = next((index for index, event in enumerate(events) if event["method"] == "POST"
        and event.get("kind") == "foreground" and event.get("status") == 429), None)
    if wall is None:
        return "no foreground request was refused with 429"
    later = [event for event in events[wall + 1:]
        if event["method"] == "GET" and event.get("client") == "codex"]
    usage = sum(path(event).endswith("/usage") for event in later)
    credits = sum(path(event).endswith(CREDITS_PATH) for event in later)
    prepares = 0
    try:
        for line in trace.read_text().splitlines():
            event = json.loads(line)
            stamp = event.get("monotonicUs")
            prepares += (event.get("kind") == "rotation" and event.get("verb") == "prepare"
                and type(stamp) is int and stamp > events[wall]["monotonicUs"])
    except (OSError, ValueError, AttributeError):
        prepares = 0
    meaning = ("the turn ended and the host was asked" if prepares else
        "the earned reset was read but the handoff was not ready (host policy, or a pool "
        "snapshot showing no other account)" if credits else
        "the usage reply is not decoding or does not name the login's account" if usage else
        "the walled turn read nothing")
    return (f"after the first 429: {usage} codex usage reads, {credits} codex credit-list "
        f"reads, {prepares} prepares traced; {meaning}")


def validate_creditless_yield(events, prepares):
    """Earned reset first, then another account, for every weekly wall.

    `events` are the fixture server's requests in arrival order and `prepares`
    the host's completed `codex-rotation prepare` stamps. After an account's
    usage 429 its turn must read that account's (empty) earned-reset credits,
    redeem nothing, send no further model request, and only then end for the
    host to prepare rotation. A turn that keeps polling never reaches prepare.
    """
    def path(event):
        return event["path"].split("?", 1)[0]
    probe.require(not any(event["method"] == "POST" and path(event).endswith(CREDITS_PATH + "/consume")
        for event in events), "a creditless wall attempted an earned-reset redemption")
    walls = [(index, event) for index, event in enumerate(events)
        if event["method"] == "POST" and event.get("kind") == "foreground" and event.get("status") == 429]
    probe.require(bool(walls) and all(event.get("account") for _, event in walls),
        "no attributable weekly wall was exercised")
    proofs = []
    for index, wall in walls:
        later = events[index + 1:]
        probe.require(not any(event["method"] == "POST" and event.get("kind") == "foreground"
            and event.get("account") == wall["account"] for event in later),
            "the walled account sent another model request")
        credit = next((event for event in later if event["method"] == "GET"
            and path(event).endswith(CREDITS_PATH) and event.get("account") == wall["account"]), None)
        probe.require(credit is not None, "the wall was not followed by its own earned-reset read")
        prepare = min((stamp for stamp in prepares if stamp > wall["monotonicUs"]), default=None)
        probe.require(prepare is not None, "the walled turn never ended for account rotation")
        probe.require(wall["monotonicUs"] <= credit["monotonicUs"] < prepare,
            "account rotation was prepared before the earned reset was tried")
        proofs.append({"account": wall["account"], "wallUs": wall["monotonicUs"],
            "creditReadUs": credit["monotonicUs"], "firstPrepareUs": prepare})
    return proofs


def validate_wall_waited_out(events, lift_seconds=None):
    """A five-hour wall that resets soon is waited out on its own account.

    `events` are the fixture server's requests in arrival order. The source login is
    refused once, reads its own walled usage, and sends its turn's request again only
    once the wall has lifted; no other login sends a model request.
    """
    lift_seconds = WALL_LIFT_SECONDS if lift_seconds is None else lift_seconds

    def path(event):
        return event["path"].split("?", 1)[0]
    posts = [(index, event) for index, event in enumerate(events)
        if event["method"] == "POST" and path(event).endswith("/responses")]
    probe.require(all(event.get("account") == "source" for _, event in posts),
        "a login other than the walled one sent a model request")
    turns = [(index, event) for index, event in posts if event.get("kind") == "foreground"]
    walls = [(index, event) for index, event in turns if event.get("status") == 429]
    probe.require(len(walls) == 1, f"the source was refused {len(walls)} times, not once")
    index, wall = walls[0]
    probe.require(len(turns) == 3 and turns[1][0] == index and turns[2][1].get("status") == 200,
        "the walled turn was not the second request, retried once and answered")
    retry_index, retry = turns[2]
    waited = retry["monotonicUs"] - wall["monotonicUs"]
    probe.require(waited >= lift_seconds * 1_000_000,
        "the walled turn was retried before its wall lifted")
    reads = [event for event in events[index + 1:retry_index] if event["method"] == "GET"
        and path(event).endswith("/usage") and event.get("account") == "source"
        and event.get("client") == "codex"]
    probe.require(any(event.get("allowed") is False for event in reads),
        "the walled turn never read its own wall")
    return {"wallUs": wall["monotonicUs"], "retryUs": retry["monotonicUs"],
        "waitedSeconds": round(waited / 1_000_000, 3),
        "walledUsageReads": sum(event.get("allowed") is False for event in reads),
        "usageReads": len(reads)}


def wall_case_finished(events, observed):
    """Whether `five-hour-wall-soon` has reached its end.

    Nothing is handed over in that case: it ends when the source's own turn was answered
    after its wall (its third foreground request, with 200) and a session record is at
    rest. `observed` are the session records under the fixture's CODEX_HOME.
    """
    turns = [event for event in events if event["method"] == "POST"
             and event.get("kind") == "foreground" and event.get("account") == "source"]
    return (len(turns) >= 3 and turns[2].get("status") == 200 and any(
        successor_rest(record, record.get("sessionId")) is not None for record in observed))


def wall_handoff_evidence(root, events):
    """What a handoff of the walled session leaves under `root`, in words; empty if none.

    The pool had somewhere to go (the other account reads as usable throughout), so
    each of these is the host or the client moving a session that should have waited: a
    charged rotation, a handoff receipt, a native action on the session, or a model
    request from a login other than the walled one.
    """
    found = []
    pool = root / "host-state" / "accounts-codex"
    try:
        rotations = json.loads((pool / "state.json").read_text()).get("rotations")
    except FileNotFoundError:
        rotations = None
    if rotations:
        found.append(f"{len(rotations)} rotation(s) charged")
    receipts = [path.name for path in (pool / "renewal-follow").glob("*") if path.is_file()]
    if receipts:
        found.append(f"{len(receipts)} handoff receipt file(s)")
    actions = [event.get("action") for event in native_actions(root)]
    if actions:
        found.append("native action(s): " + ", ".join(sorted({str(action) for action in actions})))
    others = sorted({str(event.get("account")) for event in events if event["method"] == "POST"
        and event["path"].split("?", 1)[0].endswith("/responses") and event.get("account") != "source"})
    if others:
        found.append("model request(s) from " + ", ".join(others))
    return found


def require_wall_held(root, events):
    """Fail `five-hour-wall-soon` by name as soon as its session is being handed over."""
    found = wall_handoff_evidence(root, events)
    probe.require(not found, "a five-hour wall that resets soon was handed over, charged or "
        "replayed: " + "; ".join(found))


def wall_verdict(root, events, requests, trace):
    """The oracle of a finished `five-hour-wall-soon` run, and what it records.

    The wall was waited out on its own account (validate_wall_waited_out), nothing of a
    handoff exists (require_wall_held), and the turn's work went to the model exactly
    three times: the plan, the refused request and its one retry.
    """
    waited = validate_wall_waited_out(events)
    require_wall_held(root, events)
    probe.require(len(requests) == 3, "a five-hour wall that resets soon was handed over, "
        f"charged or replayed: {len(requests)} foreground model requests, not 3")
    # Recorded, not required: whether the host was asked to move the walled turn.
    try:
        waited["preparesTraced"] = len(rotation_prepares(trace))
    except RuntimeError:
        waited["preparesTraced"] = None
    return waited


def rotation_prepares(trace):
    """Completion stamps of the host's `codex-rotation prepare` calls, in order."""
    probe.require(trace.is_file() and not trace.is_symlink(), "native control trace is absent")
    events = [json.loads(line) for line in trace.read_text().splitlines()]
    stamps = [event.get("monotonicUs") for event in events
        if event.get("kind") == "rotation" and event.get("verb") == "prepare"]
    probe.require(all(type(stamp) is int and stamp > 0 for stamp in stamps),
        "native rotation trace has no usable clock")
    return stamps


class Model(probe.Mock):
    def __init__(self, root, weekly_quota=False, all_exhausted=False, chain=False, wall=None):
        super().__init__(False)
        probe.require(wall in (None, "five-hour-soon") and (wall is None or weekly_quota),
                      "unknown fixture wall")
        probe.require(wall is None or not (all_exhausted or chain),
                      "the five-hour wall has one usable destination and no chain")
        self.root = root
        # None: the weekly wall of the quota cases. "five-hour-soon": the source's second
        # request is refused on its five-hour window, which lifts WALL_LIFT_SECONDS later.
        self.wall = wall
        # When the provider served that refusal: (monotonic, unix), or None.
        self.wall_served = None
        self.monotonic = time.monotonic
        self.http_events = []
        self.auxiliary_requests = []
        self.unknown_requests = []
        self.weekly_quota = weekly_quota
        self.all_exhausted = all_exhausted
        self.chain = chain
        self.middle_exhausted = False
        self.usage_reads = []
        # Replies this fixture built that the client would not decode: {path, error}.
        self.fixture_faults = []
        # POSTs to a path the fixture has no route for: {path, client}.
        self.unrouted_posts = []
        # Whether the provider has refused the source login's model request yet.
        self.source_walled = False

    def week_spent(self, old_token, new_token, next_token=False):
        """Whether that login's week is spent now. Asked by usage() and by each model
        response's headers, so it has no side effect."""
        if self.wall is not None:
            # The five-hour wall is not on the week, and the other account is usable from
            # the start: the case asserts that a wall which resets soon is not handed
            # over, and that is only an assertion if the pool has somewhere to go.
            return False
        if old_token:
            # Healthy until the provider has refused it: an account's usage never says
            # "spent" before a request of its own was turned away, and a turn's first
            # preflight must find the source permitting.
            return self.source_walled
        return bool(self.all_exhausted or not (self.root / "renewed").exists()
                    or (self.chain and ((new_token and self.middle_exhausted)
                                        or (next_token and not self.middle_exhausted))))

    def wall_lifted(self):
        """Whether the five-hour wall, once served, has lifted."""
        with self.lock:
            served = self.wall_served
        return served is not None and self.monotonic() >= served[0] + WALL_LIFT_SECONDS

    def usage_body(self, generation):
        """The usage reply `generation`'s login would read now, with no read recorded."""
        if not self.weekly_quota:
            return provider_fixture.usage("fixture-account", "fixture-user", week_used=0, allowed=True)
        generation = generation if generation in QUOTA_ACCOUNTS else "old"
        _, account, user = QUOTA_ACCOUNTS[generation]
        if self.wall == "five-hour-soon" and generation == "old":
            with self.lock:
                served = self.wall_served
            if served is None or self.wall_lifted():
                return provider_fixture.usage(account, user, week_used=10, allowed=True)
            # The window's reset stays where the refusal put it: one reading of the clock
            # gives both the distance and the instant it is measured from.
            now = int(time.time())
            remaining = max(1, int(served[1]) + WALL_RESET_SECONDS - now)
            return provider_fixture.usage(account, user, week_used=10, session_used=100,
                                          allowed=False, session_resets_in=remaining, now=now)
        full = self.week_spent(generation == "old", generation == "new", generation == "next")
        return provider_fixture.usage(account, user, week_used=100 if full else 0, allowed=not full)

    def limit_headers(self, generation, wall=False, usage=None):
        """The rate-limit headers of a model response to `generation`'s login.

        `usage` is the reading already taken for this response, when one was.
        """
        return provider_fixture.limit_headers(
            self.usage_body(generation) if usage is None else usage, wall=wall)

    def refused(self, record, generation, message):
        """The 429 of a model request `generation`'s login sent at its wall.

        Its headers and its body come from one reading of that login's usage, and it is
        held to what the client decodes (provider_fixture `refusal`). A login whose usage
        states no spent window has no wall: that is a FixtureFault, which the handler
        records for the controller's loop.
        """
        usage = self.usage_body(generation)
        reply = provider_fixture.check("refusal", provider_fixture.refusal(
            usage, message, headers=self.limit_headers(generation, wall=True, usage=usage)))
        record["responseHeaders"] = reply["headers"]
        return reply["status"], json.dumps(reply["body"]).encode(), "application/json", None, record

    def answered(self, record, generation):
        """Put the rate-limit headers of a 200 to `generation`'s login in its record.

        A response the provider answers never states a spent window. This fixture does
        answer a login whose usage reads as not allowed: a title request that arrives
        after the wall, and a walled or exhausted login that sends a model request again,
        which the cases' own oracles fail. Such a 200 states no windows, as every response
        did before these headers existed, and its record says they were withheld.
        """
        if self.usage_body(generation)["rate_limit"]["allowed"]:
            record["responseHeaders"] = self.limit_headers(generation)
        else:
            record["responseHeaders"] = {}
            record["limitHeadersWithheld"] = True

    def usage(self, old_token, new_token, next_token=False):
        probe.require(self.weekly_quota and (old_token or new_token or next_token), "unexpected usage identity")
        if (new_token or next_token) and self.wall is None:
            # The weekly cases hold a destination's first reading until the host has
            # opened it. The five-hour case opens nothing: its destination answers at once.
            deadline = time.monotonic() + 60
            while not (self.root / "renewed").exists() and time.monotonic() < deadline:
                time.sleep(0.01)
        with self.lock:
            self.usage_reads.append("source" if old_token else "target")
        # The reply names the account it is for. A reply with no identity makes the
        # client refuse any earned-reset attempt and end the walled turn at
        # once, which hid a turn that never ended once the account was named.
        return self.usage_body("old" if old_token else "next" if next_token else "new")

    def reply(self, body, generation=None):
        with self.lock:
            kind = request_kind(body)
            record = {"at": time.monotonic(), "unixAt": time.time(), "kind": kind, "body": body}
            destination = self.requests if kind == "foreground" else (
                self.auxiliary_requests if kind == "thread-title" else self.unknown_requests)
            destination.append(record)
            number = len(self.requests)
        if kind == "unknown":
            return 400, b'{"error":"unrecognized fixture request"}', "application/json", None, record
        if kind == "thread-title":
            item = {"type": "message", "role": "assistant", "id": "fixture-title",
                "content": [{"type": "output_text", "text": json.dumps({"title": "Recover original work"})}]}
            self.answered(record, generation)
            return 200, probe.response("fixture-title", item), "text/event-stream", None, record
        if self.wall == "five-hour-soon" and number > 1:
            if number == 2:
                with self.lock:
                    self.wall_served = (self.monotonic(), time.time())
            if not self.wall_lifted():
                # The first refusal, or a request sent again before the wall lifted. Its
                # reset is the five-hour window's own: WALL_RESET_SECONDS after the wall.
                return self.refused(record, generation, "Fixture five-hour limit reached")
            item = {"type": "message", "role": "assistant", "id": "renewed-reply",
                    "content": [{"type": "output_text", "text": "Native renewal fixture recovered."}]}
            self.answered(record, generation)
            return 200, probe.response(f"renewal-{number}", item), "text/event-stream", None, record
        if self.chain and number > 1:
            expected = {2: "old", 3: "new", 4: "next"}.get(number)
            probe.require(generation == expected, "quota chain used an unexpected account")
            if number == 3:
                self.middle_exhausted = True
                return self.refused(record, generation, "Fixture second account exhausted")
        if number > 1 and not (self.root / "renewed").exists():
            if self.weekly_quota:
                if generation in (None, "old"):
                    with self.lock:
                        self.source_walled = True
                return self.refused(record, generation, "Fixture weekly quota exhausted")
            return 401, json.dumps({"error": {"type": "invalid_request_error",
                "code": "invalid_api_key", "message": "Fixture login expired"}}).encode(), "application/json", None, record
        if number == 1:
            item = {"type": "function_call", "call_id": probe.CALL_ID, "name": "update_plan",
                    "arguments": json.dumps({"plan": [{"step": probe.PLAN, "status": "completed"}]})}
        else:
            item = {"type": "message", "role": "assistant", "id": "renewed-reply",
                    "content": [{"type": "output_text", "text": "Native renewal fixture recovered."}]}
        self.answered(record, generation)
        return 200, probe.response(f"renewal-{number}", item), "text/event-stream", None, record


def spent_window_on_success(status, headers):
    """The header of an answered model response that states a spent window, or None.

    No provider answers a request and reports the account's window at 100 in the same
    response. The client reads those headers as evidence against the turn's clear usage
    reading, so such a reply sets it re-reading a usage that says not allowed while the
    model goes on answering.
    """
    if status != 200:
        return None
    return next((name for name, value in headers.items()
                 if name.endswith("-used-percent") and float(value) >= 100), None)



def serve(model, secure=True):
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            path = self.path.split("?", 1)[0]
            event = {"method": "GET", "path": self.path,
                "account": request_account(self.headers), "client": request_client(self.headers),
                "monotonicUs": monotonic_us()}
            with model.lock:
                model.http_events.append(event)
                (model.root / "http-events-live.json").write_text(json.dumps(model.http_events))
            body = {"models": []}
            try:
                if path.endswith(CREDITS_PATH):
                    # No earned reset exists in this fixture: a valid empty list.
                    body = provider_fixture.check("credits", provider_fixture.credit_list())
                if path.endswith("/usage") and not model.weekly_quota:
                    # The same-account cases have one login and no quota story: its
                    # account is healthy. Model.usage is the quota cases' alone.
                    body = provider_fixture.check("usage", provider_fixture.usage(
                        "fixture-account", "fixture-user", week_used=0, allowed=True))
                if model.weekly_quota and path.endswith("/usage"):
                    generation = bearer_generation(self.headers)
                    old_token, new_token, next_token = (
                        generation == "old", generation == "new", generation == "next")
                    if not (old_token or new_token or (model.chain and next_token)):
                        self.send_error(401)
                        return
                    # Every model's reply is held to what the client decodes: none opts out.
                    body = provider_fixture.check("usage", model.usage(old_token, new_token, next_token))
                    # What the reader was told, for the oracles: the verdict alone.
                    with model.lock:
                        event["allowed"] = body["rate_limit"]["allowed"]
            except Exception as error:
                # Seen by the controller's loop (require_no_fixture_fault), not by the client.
                with model.lock:
                    model.fixture_faults.append({"path": path, "error": f"{type(error).__name__}: {error}"})
                self.send_error(500)
                return
            if path.endswith("/accounts/check"):
                account = "fixture-account"
                if model.weekly_quota:
                    account = self.headers.get("chatgpt-account-id") or (
                        {"next": "final-account", "new": "target-account"}.get(
                            bearer_generation(self.headers), "source-account"))
                body = {"accounts": [{"id": account, "can_access_with_session": True,
                    "is_deactivated": False, "plan_type": "pro", "name": "Fixture",
                    "workspace_backend_origin": "NO_CONSTRAINT", "account_routing_override": "NO_CONSTRAINT"}],
                    "account_ordering": [account], "default_account_id": account}
            data = json.dumps(body).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_POST(self):
            with model.lock:
                event = {"method": "POST", "path": self.path, "kind": "non-responses",
                    "account": request_account(self.headers), "client": request_client(self.headers),
                    "monotonicUs": monotonic_us()}
                model.http_events.append(event)
                (model.root / "http-events-live.json").write_text(json.dumps(model.http_events))
            length = int(self.headers.get("content-length", "0"))
            if not 0 < length <= 8 * 1024 * 1024:
                self.send_error(413)
                return
            payload = self.rfile.read(length)
            path = self.path.split("?", 1)[0]
            record = {}
            if path.endswith(CREDITS_PATH + "/consume"):
                # Nothing can be redeemed here. Any case that redeems fails its own
                # oracle (validate_creditless_yield); the reply is still a real one.
                status, data, content_type = 200, json.dumps(provider_fixture.check(
                    "consume", provider_fixture.consume_reply())).encode(), "application/json"
            elif path.endswith("/oauth/token"):
                # A fixture login cannot be refreshed: its refresh token is spent.
                status, data, content_type = 400, b'{"error":"invalid_grant"}', "application/json"
            elif not path.endswith("/responses"):
                # No route: answered as before, and listed apart from model requests of
                # unknown shape (unknown-requests.json), which fail a run.
                status, data, content_type = 400, b'{"error":"invalid_grant"}', "application/json"
                with model.lock:
                    model.unrouted_posts.append({"path": path, "client": event["client"]})
            else:
                # A bearer that is no fixture login's stays a login the model does not know.
                generation = bearer_generation(self.headers) or ""
                try:
                    status, data, content_type, _, record = model.reply(json.loads(payload), generation)
                except provider_fixture.FixtureFault as error:
                    # Seen by the controller's loop (require_no_fixture_fault), not by the client.
                    with model.lock:
                        model.fixture_faults.append({"path": path, "error": f"FixtureFault: {error}"})
                    self.send_error(500)
                    return
                stated = spent_window_on_success(status, record.get("responseHeaders", {}))
                if stated is not None:
                    # Seen by the controller's loop (require_no_fixture_fault). The reply
                    # goes out as built: the fault, not a changed reply, fails the run.
                    with model.lock:
                        model.fixture_faults.append({"path": path, "error":
                            f"FixtureFault: a 200 states a spent window in {stated}"})
                # Retain only proof booleans and allowlisted UUIDs, never bearer text.
                with model.lock:
                    metadata = json.loads(self.headers.get("x-codex-turn-metadata", "{}"))
                    record["requestIdentity"] = {
                        key: value for key in ("thread_id", "turn_id", "parent_thread_id")
                        if isinstance(value := metadata.get(key), str)
                        and re.fullmatch(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}", value)
                    }
                    event["kind"] = record["kind"]
                    event["status"] = status
                    (model.root / "http-events-live.json").write_text(json.dumps(model.http_events))
                    record["oldTokenMatched"] = generation == "old"
                    record["newTokenMatched"] = generation == ("next" if model.chain else "new")
            self.send_response(status)
            self.send_header("content-type", content_type)
            self.send_header("content-length", str(len(data)))
            # The account's rate-limit windows, as every model response states them.
            for key, value in record.get("responseHeaders", {}).items():
                self.send_header(key, value)
            self.end_headers()
            self.wfile.write(data)
    if not secure:
        server = provider_fixture.LoopbackHTTPServer(("127.0.0.1", 0), Handler)
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        return server, worker
    server = provider_fixture.LoopbackHTTPServer(("127.0.0.1", 0), Handler)
    server.socket = fixture_tls(model.root).wrap_socket(server.socket, server_side=True)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    return server, worker


def fixture_tls(root):
    """An isolated test CA shared by native recovery and parallel-login tests."""
    cert = root / "fixture-ca.pem"
    key = root / "fixture-tls.key"
    config = root / "fixture-tls.cnf"
    config.write_text("[req]\ndistinguished_name=dn\nx509_extensions=v3\nprompt=no\n[dn]\nCN=ALab loopback fixture CA\n[v3]\nsubjectAltName=IP:127.0.0.1\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,digitalSignature,keyEncipherment,keyCertSign\nsubjectKeyIdentifier=hash\n")
    subprocess.run(["/usr/bin/openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
        "-days", "1", "-config", str(config), "-keyout", str(key), "-out", str(cert)],
        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run(["/usr/bin/openssl", "x509", "-in", str(cert), "-outform", "DER",
        "-out", str(root / "fixture-ca.der")],
        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    key.chmod(0o600)
    server_key = root / "fixture-server.key"
    server_cert = root / "fixture-server.pem"
    request = root / "fixture-server.csr"
    extensions = root / "fixture-server.ext"
    extensions.write_text("subjectAltName=IP:127.0.0.1\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n")
    subprocess.run(["/usr/bin/openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes",
        "-subj", "/CN=127.0.0.1", "-keyout", str(server_key), "-out", str(request)],
        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run(["/usr/bin/openssl", "x509", "-req", "-days", "1", "-in", str(request),
        "-CA", str(cert), "-CAkey", str(key), "-CAcreateserial", "-extfile", str(extensions),
        "-out", str(server_cert)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    server_key.chmod(0o600)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(server_cert, server_key)
    return context


def process_evidence(record):
    executable = Path(record["exe"]).resolve(strict=True)
    stat = executable.stat()
    identity = record.get("identity")
    probe.require(isinstance(identity, dict)
        and type(identity.get("startSeconds")) is int
        and type(identity.get("startMicroseconds")) is int, "native Darwin process birth is absent")
    return {"pid": record["pid"], "birth": record["identity"], "exe": str(executable),
            "kernelBirth": identity["startSeconds"] * 1000000 + identity["startMicroseconds"],
            "image": {"device": stat.st_dev, "inode": stat.st_ino,
                      "sha256": hashlib.sha256(executable.read_bytes()).hexdigest()}}


def fake_auth(path, generation, issuer, account="fixture-account", *,
              email="fixture@example.invalid", user="fixture-user"):
    token = token_part({"alg": "none"}) + "." + token_part(identity_claims(account, email, user)) + ".sig"
    path.parent.mkdir()
    probe.write_json(path, {"auth_mode": "chatgpt", "OPENAI_API_KEY": None,
        "tokens": {"id_token": token,
                   "access_token": access_token(generation, account, email=email, user=user),
                   "refresh_token": f"fixture-refresh-{generation}", "account_id": account,
                   "oauth_provenance": {"issuer": issuer, "client_id": "fixture-client"}},
        "last_refresh": datetime.now(timezone.utc).isoformat()})
    path.chmod(0o600)


def records(home):
    result = []
    for path in (home / "alab" / "sessions").glob("*.json"):
        try:
            value = json.loads(path.read_text())
            if isinstance(value, dict) and value.get("pid"):
                result.append(value)
        except (OSError, ValueError):
            pass
    return result


def structural_records(home, thread):
    """Retain lifecycle state even after the dead source loses its PID field."""
    result = []
    for path in (home / "alab" / "sessions").glob("*.json"):
        try:
            if path.is_symlink() or path.stat().st_size > 2 * 1024 * 1024:
                continue
            record = json.loads(path.read_text())
            if record.get("sessionId") != thread:
                continue
            item = {key: record.get(key) for key in (
                "sessionId", "pid", "status", "autopilot", "draftEmpty", "queuedInput",
                "pendingUserTurn", "popupOpen", "editorOpen")}
            exit_kind = record.get("exit")
            item["exit"] = exit_kind if exit_kind is None or exit_kind in ("fatal", "orderly") or (
                isinstance(exit_kind, str) and re.fullmatch(r"signal:[0-9]{1,3}", exit_kind)) else "other"
            owed = record.get("owed") or {}
            native = owed.get("nativeOrigin") or {}
            renewal = recovery_marker(owed) or {}
            item["owed"] = {
                "ordinary": {"rule": owed.get("rule") if owed.get("rule") in ORDINARY_PACED_RULES else
                    ("unknown" if owed.get("rule") is not None else None),
                    "mode": owed.get("mode") if owed.get("mode") in ("cruise", "land") else None,
                    "notBefore": owed.get("notBefore") if type(owed.get("notBefore")) is int else None,
                    "landed": owed.get("landed") if type(owed.get("landed")) is bool else None,
                    "landing": owed.get("landing") if type(owed.get("landing")) is bool else None,
                    "resumeGoal": owed.get("resumeGoal") if type(owed.get("resumeGoal")) is bool else None,
                    "textPresent": isinstance(owed.get("text"), str) and bool(owed["text"].strip())},
                "nativeOrigin": {key: native.get(key) for key in ("turnId", "phase")},
                "renewal": {key: renewal.get(key) for key in (
                    "receipt", "turn", "accepted", "starts", "local", "openaiScope", "goalId")},
            }
            item["recordFile"] = path.name
            result.append(item)
        except (OSError, ValueError, AttributeError):
            continue
    return sorted(result, key=lambda item: item["recordFile"])



def active_prepare(path, receipt):
    """Read the one immutable child expected by the beyond-budget fixture."""
    reference = receipt.get("current_attempt")
    if reference is None:
        return receipt.get("native_prepare"), None
    probe.require(isinstance(reference, dict)
        and re.fullmatch(r"[0-9a-f]{32}", reference.get("id", ""))
        and type(reference.get("generation")) is int and reference["generation"] == 1,
        "unexpected continuation attempt reference")
    attempt_path = path.parent / (receipt["id"] + ".attempts") / (reference["id"] + ".json")
    probe.require(not attempt_path.parent.is_symlink() and not attempt_path.is_symlink() and attempt_path.is_file()
        and attempt_path.stat().st_size <= 2 * 1024 * 1024,
        "continuation journal is not a bounded regular record")
    attempt = json.loads(attempt_path.read_text())
    original = dict(word.split("=", 1) for word in receipt["native_prepare"].split() if "=" in word)
    probe.require(attempt.get("root") == receipt["id"] and attempt.get("reference") == reference
        and attempt.get("parent") == {"id": receipt["id"], "generation": 0}
        and attempt.get("native_predecessor") == original.get("id"),
        "continuation journal did not bind the original root and predecessor")
    prepare = attempt.get("native_prepare")
    probe.require(isinstance(prepare, str), "continuation plan is absent")
    fields = dict(word.split("=", 1) for word in prepare.split() if "=" in word)
    probe.require(fields.get("id") == reference["id"], "continuation plan ID changed")
    return prepare, attempt


def consumed_receipt(root, thread, wall, successor_pid=None):
    for directory in root.rglob("renewal-follow"):
        if not directory.is_dir():
            continue
        for path in directory.glob("*.json"):
            if len(path.stem) != 32 or any(c not in "0123456789abcdef" for c in path.stem):
                continue
            try:
                value = json.loads(path.read_text())
            except (OSError, ValueError):
                continue
            intent = value.get("intent", {})
            if (value.get("phase") == "consumed" and intent.get("thread") == thread and intent.get("wall") == wall
                and (successor_pid is None or value.get("successor_pid") == successor_pid)):
                original, attempt = active_prepare(path, value)
                if not isinstance(original, str):
                    continue
                fields = dict(word.split("=", 1) for word in original.split() if "=" in word)
                operation = attempt["reference"]["id"] if attempt is not None else value["id"]
                probe.require(fields.get("id") == operation, "consumed native operation ID changed")
                # The logical root survives continuation; each native operation
                # keeps its own completion ledger, including Resume-only children.
                ledger_path = root / "host-state" / "upgrade" / f"renewal-{operation}.json"
                try:
                    ledger = json.loads(ledger_path.read_text())
                except (OSError, ValueError):
                    continue
                matched = original == ledger.get("native_completed_prepare")
                if not matched or ledger.get("native_coordination") is not True or ledger.get("native_rollback", False):
                    continue
                observation = ledger.get("native_observation")
                if not isinstance(observation, str) or not observation:
                    continue
                result = {key: value.get(key) for key in ("id", "phase", "old_pid", "old_birth",
                    "successor_pid", "successor_birth", "accepted_turn", "intent", "destination",
                    "old_auth", "new_auth")}
                result.update(nativeOperationId=fields.get("id"), nativePlanMatched=True,
                    nativeCoordination=True, nativeRollback=False,
                    nativeObservationSha256=hashlib.sha256(observation.encode()).hexdigest())
                if attempt is not None:
                    result["nativeAttempt"] = attempt["reference"]
                return result
    return None


def control(root, metadata, command):
    endpoint = Path(metadata["socket"]).resolve(strict=True)
    token_path = Path(metadata["token_path"]).resolve(strict=True)
    probe.require(endpoint.is_relative_to(root) and token_path.is_relative_to(root),
                  "fixture control pointers escaped the owned root")
    token = token_path.read_text().strip()
    probe.require(token and len(token) <= 256 and not any(c.isspace() for c in token),
                  "fixture control token is malformed")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(60)
        connection.connect(str(endpoint))
        connection.sendall(("AUTH " + token + "\n" + command + "\n").encode())
        with connection.makefile("rb") as incoming:
            reply = incoming.readline(256 * 1024)
    probe.require(reply.endswith(b"\n"), "fixture control reply was incomplete")
    return reply.decode().strip()


def native_actions(root):
    path = root / "native-actions.jsonl"
    if not path.exists():
        return []
    probe.require(path.is_file() and path.stat().st_size <= 16 * 1024 * 1024,
                  "native action evidence is not bounded")
    return [json.loads(line) for line in path.read_text().splitlines(keepends=True) if line.endswith("\n")]


def validate_presignal_stop(operation, initial, state, cancellation, worker, observation):
    """Require durable owner revocation and an actual settled effect-free native cap."""
    deadline = int(dict(word.split("=", 1) for word in initial["native_prepare"].split()
        if "=" in word)["deadline-us"])
    probe.require(state.get("id") == operation and state.get("intent") == initial["intent"]
        and state.get("phase") == "retired" and state.get("revoked") is True
        and state.get("native_prepare") == initial["native_prepare"]
        and state.get("expires_ms") == initial["expires_ms"]
        and all(state.get(key) is None for key in
            ("candidate", "current_attempt", "successor_pid", "accepted_turn")),
        "OwnerStop did not retain an exact revoked terminal intent")
    probe.require(cancellation.get("root") == operation and cancellation.get("intent") == initial["intent"]
        and cancellation.get("reason") == "owner_stop"
        and type(cancellation.get("human_seq")) is int
        and cancellation["human_seq"] > initial["human_seq"],
        "OwnerStop lacks durable person-bound cancellation")
    probe.require(worker.get("operation") == operation and worker.get("deadlineUs") == deadline
        and type(worker.get("monotonicUs")) is int and 0 < worker["monotonicUs"] < deadline,
        "OwnerStop coordinator did not stop before its unchanged deadline")
    probe.require(observation.get("operation") == operation
        and observation.get("observationTriggeredReconciliation") is True
        and observation.get("state") == "completed" and observation.get("decision") == "rollback-safe"
        and observation.get("deadlineUs") == deadline and observation.get("deadlineExpired") is False
        and type(observation.get("monotonicUs")) is int
        and worker["monotonicUs"] <= observation["monotonicUs"] < deadline
        and observation.get("signalSent") is False
        and (observation.get("signalAccepted") is None or observation.get("signalAccepted") is False)
        and observation.get("resumeSent") is False
        and observation.get("inputCustodySettled") is True and observation.get("inputAttemptSettled") is True
        and observation.get("goGrantedChildren") == 0 and observation.get("retainedChildren") == 0,
        "OwnerStop lacks positive effect-free native closure and drained input")


def original_signal_effect(root, report):
    """Count actual successful kernel calls, independently of RPC ACK replay."""
    path = root / "native-signal-effects.jsonl"
    probe.require(path.is_file() and not path.is_symlink() and path.stat().st_size <= 1024 * 1024,
        "physical native signal evidence is absent or unbounded")
    effects = [json.loads(line) for line in path.read_text().splitlines()]
    probe.require(len(effects) == 1, "native recovery did not record exactly one physical signal")
    effect = effects[0]
    probe.require(isinstance(effect, dict) and set(effect) == {
        "schema", "kind", "pid", "birth", "group", "signal", "monotonicUs"},
        "physical native signal evidence contains unknown fields")
    probe.require(type(effect.get("schema")) is int and effect["schema"] == 1
        and effect.get("kind") == "native-signal-effect"
        and type(effect.get("signal")) is int and effect["signal"] == 15
        and type(effect.get("pid")) is int and effect["pid"] == report["originalPid"]
        and type(effect.get("birth")) is int and effect["birth"] == report["originalProcess"]["kernelBirth"]
        and type(effect.get("group")) is int and effect["group"] > 0
        and type(effect.get("monotonicUs")) is int and effect["monotonicUs"] > 0,
        "physical native signal did not bind the original process")
    return effect


def launcher_fixture_root(environment):
    """The real native launcher reads its checkpoints under its inherited HOME."""
    return Path(environment["HOME"]) / ".native-renewal-fixture"


def validate_prefix_crash_closure(boundary, state, closed):
    """A Go-granted prefix needs positive retained cohort death, never no-Go inference."""
    probe.require(state is not None and state.get("id") == boundary["operation"]
        and state.get("phase") == "committing" and state.get("current_attempt") is None
        and state.get("successor_pid") is None and state.get("accepted_turn") is None
        and state.get("revoked") is False
        and {key: (state.get("candidate") or {}).get(key) for key in boundary["prefix"]}
            == boundary["prefix"]
        and hashlib.sha256(state["native_prepare"].encode()).hexdigest() == boundary["originalPrepareSha256"]
        and state["expires_ms"] == boundary["originalExpiresMs"],
        "dead Go prefix changed its original retained logical intent or candidate")
    probe.require(closed.get("operation") == boundary["operation"]
        and closed.get("observationTriggeredReconciliation") is True
        and closed.get("state") == "completed" and closed.get("decision") == "retired-cohort-exited"
        and closed.get("deadlineExpired") is True
        and closed.get("deadlineUs") == boundary["originalDeadlineUs"]
        and type(closed.get("monotonicUs")) is int
        and closed["monotonicUs"] >= boundary["originalDeadlineUs"]
        and closed.get("signalAccepted") is True and closed.get("resumeSent") is True
        and all(closed.get(key) is True for key in ("sourceGroupGone", "retainedSourceWatch",
            "sourceWatchExited", "closedRestartWitness", "closedRestartQuiescent",
            "closedRestartAvailable", "inputAttemptSettled", "inputCustodySettled"))
        and closed.get("sourceBirthAlive") is False
        and closed.get("goGrantedChildren") == 0 and closed.get("retainedChildren") == 0,
        "Go-granted prefix lacks retained kernel cohort-death and drained-input proof")


def rollback_signal_effects(root, report, rollback):
    path = root / "native-signal-effects.jsonl"
    probe.require(path.is_file() and not path.is_symlink() and path.stat().st_size <= 1024 * 1024,
        "rollback physical signal evidence is absent or unbounded")
    effects = [json.loads(line) for line in path.read_text().splitlines()]
    expected = [(report["originalPid"], report["originalProcess"]["kernelBirth"]),
                (rollback["pid"], rollback["kernelBirth"])]
    probe.require(len(effects) == 2, "rollback recovery did not signal exactly its two live predecessors")
    for effect, (pid, birth) in zip(effects, expected):
        probe.require(set(effect) == {"schema", "kind", "pid", "birth", "group", "signal", "monotonicUs"}
            and type(effect.get("schema")) is int and effect["schema"] == 1
            and effect.get("kind") == "native-signal-effect"
            and type(effect.get("signal")) is int and effect["signal"] == 15
            and type(effect.get("pid")) is int and effect["pid"] == pid
            and type(effect.get("birth")) is int and effect["birth"] == birth
            and type(effect.get("group")) is int and effect["group"] > 0
            and type(effect.get("monotonicUs")) is int and effect["monotonicUs"] > 0,
            "rollback signal did not bind its exact live predecessor")
    probe.require(effects[0]["monotonicUs"] < effects[1]["monotonicUs"],
        "rollback signal order changed")
    return effects


def pending_receipt(root, thread, wall, operation=None):
    directory = root / "host-state" / "accounts-codex" / "renewal-follow"
    if operation is not None:
        probe.require(re.fullmatch(r"[0-9a-f]{32}", operation), "invalid observed operation ID")
    matching = {}
    for path in directory.glob("*.json"):
        if not re.fullmatch(r"[0-9a-f]{32}", path.stem):
            continue
        if operation is not None and path.stem != operation:
            continue
        probe.require(path.is_file() and not path.is_symlink() and path.stat().st_size <= 2 * 1024 * 1024,
            "native receipt is not a bounded regular record")
        value = json.loads(path.read_text())
        if value.get("intent", {}).get("thread") == thread and value["intent"].get("wall") == wall:
            probe.require(value.get("id") == path.stem, "native receipt filename/ID mismatch")
            matching[path.stem] = value
    if operation is not None:
        return matching.get(operation)
    markers = set()
    for record in records(root / "codex-home"):
        owed = record.get("owed") or {}
        if record.get("sessionId") != thread or (owed.get("nativeOrigin") or {}).get("turnId") != wall:
            continue
        marker = (recovery_marker(owed) or {}).get("receipt")
        if marker is not None:
            probe.require(isinstance(marker, str) and re.fullmatch(r"[0-9a-f]{32}", marker),
                "malformed emitted native receipt marker")
            markers.add(marker)
    probe.require(len(markers) <= 1, "ambiguous emitted native receipt marker")
    if markers:
        marker = next(iter(markers))
        probe.require(marker in matching, "emitted receipt lacks its bound native record")
        return matching[marker]
    active = [value for value in matching.values() if value.get("phase") in ("ready", "committing", "committed")]
    probe.require(len(active) <= 1, "ambiguous active native receipts before acknowledgement")
    return active[0] if active else None


def signalled_operation(actions):
    operations = {event["operation"] for event in actions
        if event.get("action") == "signal" and event.get("replyOk") is True}
    probe.require(len(operations) <= 1, "multiple operations completed Signal for one recovery")
    return next(iter(operations), None)


def publish_fixture_request(path, value):
    """The host polls existence, so publish only a complete request document."""
    probe.require(not path.exists() and not path.is_symlink(), "fixture request already exists")
    pending = path.with_name(path.name + ".pending")
    encoded = json.dumps(value, allow_nan=False) + "\n"
    with os.fdopen(os.open(pending, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w") as output:
        output.write(encoded)
        output.flush()
        os.fsync(output.fileno())
    os.replace(pending, path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True, type=Path)
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--case", choices=tuple(CASE_BUDGETS), default="happy")
    parser.add_argument("--quota", action="store_true",
                        help="Run an existing fault case with distinct quota accounts")
    parser.add_argument("--under-guardian", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    drive_seconds, guardian_seconds = CASE_BUDGETS[args.case]
    host = args.host.resolve(strict=True)
    # The record says which host image this is and what its own self-run reported. Which
    # constructors the session may use is the fixture policy's, set by --renewal-config.
    host_record = native_host_record.load(host)
    native_host_record.require_aterm_name(host)
    package = args.package.resolve(strict=True)
    build, manifest, codex = probe.package_records(package)
    root = args.output.resolve()
    if not args.under_guardian:
        # The controller owns and reaps each actual host PID. A surrounding
        # guardian owns the whole qualification, including adopted PTY holders.
        guard_result = root.with_name(root.name + "-guardian-result.json")
        command = [sys.executable, "-I", str(probe.GUARD_PATH), "--",
            sys.executable, "-I", str(probe.GUARD_PATH), "--result-file", str(guard_result),
            "--inherited-group-timeout", str(guardian_seconds), "--", sys.executable, str(Path(__file__).resolve()),
            "--host", str(host), "--package", str(package), "--output", str(root), "--case", args.case, "--under-guardian"]
        if args.quota:
            command.append("--quota")
        completed = subprocess.run(command)
        result_path = root / "result.json"
        if result_path.exists() and guard_result.exists():
            result = json.loads(result_path.read_text())
            result["guardian"] = json.loads(guard_result.read_text())
            result["passed"] = bool(result.get("passed") and completed.returncode == 0
                and result["guardian"].get("returncode") == 0 and not result["guardian"].get("timed_out"))
            result_path.write_text(json.dumps(result, indent=2) + "\n")
        return completed.returncode
    root.mkdir(parents=True, exist_ok=False)
    env = probe.fresh_environment(root)
    env["ATERM_HARNESS_STATE"] = str(root / "host-state")
    env["XDG_CONFIG_HOME"] = str(root / "config")
    launcher_root = launcher_fixture_root(env)
    chain = args.case == "weekly-quota-chain"
    soon_wall = args.case == "five-hour-wall-soon"
    weekly_quota = args.quota or args.case in ("weekly-quota", "weekly-quota-rollback", "weekly-quota-exhausted", "weekly-quota-chain", "five-hour-wall-soon")
    rollback_case = args.case in ("failed-forward-rollback", "weekly-quota-rollback")
    model = Model(root, weekly_quota=weekly_quota,
                  all_exhausted=args.case == "weekly-quota-exhausted", chain=chain,
                  wall="five-hour-soon" if soon_wall else None)
    server, server_thread = serve(model)
    report = {"passed": False, "scope": "isolated native host and real Codex PTY",
              "source": build["source"], "version": manifest["version"], "case": args.case,
              "quotaAccounts": weekly_quota,
              "observationBudgetSeconds": {"drive": drive_seconds, "guardian": guardian_seconds},
              "nativeHost": native_host_record.summary(host_record),
              "packageRecords": {"build": build, "manifest": manifest},
              "controllerSha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              # The modules that decide what the fixture serves and which host it accepts.
              "fixtureSha256": {Path(module.__file__).name: hashlib.sha256(
                  Path(module.__file__).read_bytes()).hexdigest()
                  for module in (provider_fixture, native_host_record)}}
    try:
        probe.configure(root, server.server_port)
        provider_config = root / "codex-home" / "config.toml"
        text = provider_config.read_text().replace("requires_openai_auth = false", "requires_openai_auth = true")
        text = text.replace('name = "local capacity smoke"', 'name = "OpenAI"')
        scheme = "https"
        text = text.replace("http://127.0.0.1:", f"{scheme}://127.0.0.1:")
        text = text.replace("[tools.update_plan]", "[features]\nenable_request_compression = false\n[tools.update_plan]")
        probe.require('name = "OpenAI"' in text and "requires_openai_auth = true" in text,
                      "fixture does not use the actual first-party provider contract")
        provider_config.write_text(f'cli_auth_credentials_store = "file"\nchatgpt_base_url = "{scheme}://127.0.0.1:{server.server_port}"\n' + text)
        old, new = root / "auth-old" / "auth.json", root / "auth-new" / "auth.json"
        issuer = f"{scheme}://127.0.0.1:{server.server_port}"
        fake_auth(old, "old", issuer, "source-account" if weekly_quota else "fixture-account")
        fake_auth(new, "new", issuer, "target-account" if weekly_quota else "fixture-account",
                  email="target@example.invalid" if weekly_quota else "fixture@example.invalid",
                  user="target-user" if weekly_quota else "fixture-user")
        final_auth = new
        if chain:
            final_auth = root / "auth-next" / "auth.json"
            fake_auth(final_auth, "next", issuer, "final-account", email="final@example.invalid", user="final-user")
        config = {"root": str(root), "codex": str(codex), "cwd": str(root / "work"),
                  "auth_path": str(old), "successor_auth_path": str(new),
                  "args": ["--no-alt-screen", "--dangerously-bypass-approvals-and-sandbox",
                           "--cd", str(root / "work"), probe.PROMPT],
                  "human_grace_s": 121 if args.case == "grace-beyond-budget" else 0,
                  "env": {"CODEX_HOME": env["CODEX_HOME"], "PATH": env["PATH"],
                          "XDG_CONFIG_HOME": env["XDG_CONFIG_HOME"],
                          "NO_COLOR": "1", "LANG": "en_US.UTF-8", "RUST_LOG": "codex_alab_autopilot=debug"}}
        if chain:
            config["additional_auth_paths"] = [str(final_auth)]
        if weekly_quota:
            config["weekly_quota"] = True
        config["env"]["CODEX_CA_CERTIFICATE"] = str(root / "fixture-ca.pem")
        # What this run asked the host's fixture policy for, as written to fixture.json.
        report["fixtureConfig"] = {"weekly_quota": config.get("weekly_quota", False),
            "additionalAuthCount": len(config.get("additional_auth_paths", []))}
        config_path = root / "fixture.json"
        probe.write_json(config_path, config)
        config_path.chmod(0o600)

        def drive(process, output):
            deadline = time.monotonic() + drive_seconds
            previous = None
            renewed = False
            metadata = None
            metadata_bytes = b""
            hold_set = False
            hold_released = False
            observe_expired_after = None
            last_source_records = None
            source_record_transitions = 0
            try:
                while time.monotonic() < deadline:
                    require_no_fixture_fault(model)
                    ready, _, _ = select.select([process.stdout], [], [], 0.1)
                    if ready:
                        data = os.read(process.stdout.fileno(), 65536)
                        probe.require(bool(data), "native host exited before qualification completed")
                        output.write(data)
                        output.flush()
                        if metadata is None:
                            metadata_bytes += data
                            probe.require(len(metadata_bytes) <= 1024 * 1024, "native host metadata oversized")
                            if b"\n" in metadata_bytes:
                                metadata = json.loads(metadata_bytes.split(b"\n", 1)[0])
                                probe.require(metadata.get("host_pid") == process.pid,
                                              "host metadata did not name the owned child")
                                probe.require(re.fullmatch(r"[a-zA-Z0-9_-]{1,128}", metadata["sid"]),
                                              "native session ID is malformed")
                                report["nativeHostProcess"] = {key: metadata[key] for key in ("host_pid", "host_birth", "sid")}
                    observed = records(root / "codex-home")
                    if report.get("thread"):
                        current_records = structural_records(root / "codex-home", report["thread"])
                        if current_records != last_source_records:
                            probe.require(source_record_transitions < 256,
                                "source lifecycle trace exceeded its bounded transition count")
                            with (root / "source-record-transitions.jsonl").open("a") as trace:
                                trace.write(json.dumps({"monotonicAt": time.monotonic(), "unixAt": time.time(),
                                    "records": current_records}) + "\n")
                            last_source_records = current_records
                            source_record_transitions += 1
                    if report.get("thread") and "sourceRecordsAfterSignal" not in report:
                        if any(event.get("action") == "signal" and event.get("replyOk") is True
                               for event in native_actions(root)):
                            report["sourceRecordsAfterSignal"] = structural_records(root / "codex-home", report["thread"])
                    if soon_wall:
                        # Nothing is handed over, so this controller publishes no `renew`
                        # and follows no receipt: it fails the run the moment anything of
                        # a handoff exists, and ends it when the source's own turn was
                        # answered after its wall and its session is at rest.
                        with model.lock:
                            seen = list(model.http_events)
                        require_wall_held(root, seen)
                        if wall_case_finished(seen, observed):
                            with model.lock:
                                report["requestCount"] = len(model.requests)
                            renewed = True
                            break
                        continue
                    if previous is None:
                        source_record = next((record for record in observed if recovery_marker(record.get("owed"))), None)
                        live_path = root / "live-snapshot-first-ready.json"
                        if source_record and metadata is not None and live_path.exists():
                            probe.require(live_path.is_file() and not live_path.is_symlink()
                                and live_path.stat().st_size <= 64 * 1024,
                                "live conversation checkpoint is not bounded")
                            live = json.loads(live_path.read_text())
                            process_identity = process_evidence(source_record)
                            probe.require(live.get("conversationProven") is True
                                and live.get("pid") == source_record["pid"]
                                and live.get("birth") == process_identity["kernelBirth"]
                                and live.get("shellPid") == metadata["shell_pid"]
                                and live.get("thread") == source_record["sessionId"]
                                and type(live.get("monotonicUs")) is int and live["monotonicUs"] > 0,
                                "native live conversation does not bind the original real process")
                            previous = source_record
                            probe.require(recovery_marker(previous["owed"]).get("openaiScope") is True,
                                          "renewal lacks original first-party request proof")
                            report["originalOpenaiScope"] = True
                            report["originalPid"] = previous["pid"]
                            report["originalProcess"] = process_identity
                            report["liveSnapshotBeforeRenewal"] = {key: live[key] for key in (
                                "pid", "birth", "shellPid", "thread", "conversationProven", "monotonicUs")}
                            report["thread"] = previous["sessionId"]
                            report["wall"] = previous["owed"]["nativeOrigin"]["turnId"]
                            report["sourceRecordBeforeRenewal"] = structural_records(root / "codex-home", report["thread"])
                            with model.lock:
                                report["requestsBeforeRenewal"] = len(model.requests)
                            (root / "renew").touch()
                    elif (root / "renewed").exists():
                        if args.case == "weekly-quota-exhausted":
                            trace = root / "renewal-control-trace.jsonl"
                            try:
                                events = [json.loads(line) for line in trace.read_text().splitlines()]
                            except (OSError, ValueError):
                                continue
                            attempts = [event for event in events if event.get("kind") == "rotation"
                                and event.get("verb") == "prepare"
                                and event.get("reply") == "OK codex-rotation state=waiting"]
                            with model.lock:
                                checked_both = {"source", "target"}.issubset(model.usage_reads)
                                requests = len(model.requests)
                            if len(attempts) < 2 or not checked_both:
                                continue
                            retained = next((record for record in observed
                                if record["pid"] == previous["pid"]), None)
                            if retained is None:
                                continue
                            marker = recovery_marker(retained.get("owed"))
                            probe.require(marker is not None and marker.get("receipt") is None
                                and marker.get("starts") == 1 and marker.get("accepted") is False
                                and retained.get("owed", {}).get("nativeOrigin", {}).get("turnId") == report["wall"],
                                "exhausted pool lost the original task or claimed a recovery")
                            pool = root / "host-state" / "accounts-codex"
                            state = json.loads((pool / "state.json").read_text())
                            probe.require(not state.get("rotations") and not native_actions(root)
                                and not list((pool / "renewal-follow").glob("*.json"))
                                and requests == report["requestsBeforeRenewal"],
                                "exhausted pool launched, charged or replayed work")
                            report.update(allAccountsExhaustedRetained=True,
                                retainedOriginalPid=previous["pid"], requestCount=requests)
                            renewed = True
                            break
                        if rollback_case:
                            boundary = report.get("rollback")
                            actions = native_actions(root)
                            forward_checkpoint = launcher_root / "forward-before-attach.reached"
                            if boundary is None and forward_checkpoint.exists():
                                operation = signalled_operation(actions)
                                original = pending_receipt(root, report["thread"], report["wall"], operation)
                                probe.require(operation is not None and original is not None
                                    and original["phase"] == "committing" and original.get("candidate") is None,
                                    "failed forward missed its genuine pre-Attach boundary")
                                report["rollback"] = boundary = {"operation": operation,
                                    "forwardPrefixPid": int(forward_checkpoint.read_text()),
                                    "originalPrepareSha256": hashlib.sha256(original["native_prepare"].encode()).hexdigest(),
                                    "originalExpiresMs": original["expires_ms"],
                                    "originalDeadlineUs": int(dict(word.split("=", 1)
                                        for word in original["native_prepare"].split() if "=" in word)["deadline-us"]),
                                    "credentialGeneration": original["generation"]}
                                rollback_row_stamp = original["row_stamp"]
                                if args.case == "weekly-quota-rollback":
                                    # The destination ceases to exist only after native
                                    # Signal and before the forward prefix can attach.
                                    new.rename(root / "unavailable-target-auth.json")
                            if boundary is not None and (launcher_root / "rollback-before-attach.reached").exists():
                                # The real forward prefix timed out and exited before Attach.
                                # Release only future Forward prefixes after actual Rollback began.
                                (launcher_root / "forward-before-attach.pause").unlink(missing_ok=True)
                                operation = boundary["operation"]
                                original = pending_receipt(root, report["thread"], report["wall"], operation)
                                rollback_record = next((record for record in observed
                                    if record["pid"] != previous["pid"]
                                    and record.get("sessionId") == report["thread"]
                                    and record.get("env", {}).get("CODEX_AUTH_PATH") == str(old)), None)
                                if rollback_record is not None and "process" not in boundary:
                                    process_proof = process_evidence(rollback_record)
                                    traces = [json.loads(line) for line in
                                        (root / "renewal-control-trace.jsonl").read_text().splitlines()]
                                    claimed = next((event for event in traces if event.get("kind") == "session-status"
                                        and event.get("holder") == f"codex-alab@{rollback_record['pid']}"
                                        and event.get("proven") is True and event.get("hold") is False
                                        and event.get("replySent") is True and event.get("contract") == 2
                                        and {"relaunch", "upgrade"}.issubset(set(event.get("scope", "").split(",")))
                                        and type(event.get("monotonicUs")) is int
                                        and event["monotonicUs"] > 0), None)
                                    ledger_path = root / "host-state" / "upgrade" / f"renewal-{operation}.json"
                                    ledger = json.loads(ledger_path.read_text())
                                    if original is not None and original["phase"] == "retired" and claimed is not None:
                                        probe.require(original.get("revoked") is False
                                            and original.get("launch_attempted") is True
                                            and original.get("successor_pid") == process_proof["pid"]
                                            and original.get("successor_birth") == process_proof["kernelBirth"]
                                            and original.get("accepted_turn") is None
                                            and (original.get("candidate") or {}).get("forward") is False
                                            and (original.get("candidate") or {}).get("pid") == process_proof["pid"]
                                            and original.get("old_auth") == str(old) and original.get("new_auth") == str(new)
                                            and original.get("current_attempt") is None
                                            and original["expires_ms"] == boundary["originalExpiresMs"]
                                            and hashlib.sha256(original["native_prepare"].encode()).hexdigest()
                                                == boundary["originalPrepareSha256"]
                                            and ledger.get("native_rollback") is True
                                            and ledger.get("native_observation")
                                            and original["native_prepare"] in
                                                (ledger.get("native_prepare"), ledger.get("native_completed_prepare"))
                                            and process_proof["exe"] == str(codex),
                                            "rollback lacks exact old operation, auth, image and completed phase proof")
                                        if args.case == "weekly-quota-rollback":
                                            pool = root / "host-state" / "accounts-codex"
                                            bindings = json.loads((pool / "logins.json").read_text())
                                            if operation in bindings.get("native_rotations", {}):
                                                continue
                                            charged = json.loads((pool / "state.json").read_text())
                                            probe.require(len(charged.get("rotations", [])) == 1,
                                                "rollback changed the original rotation charge")
                                        boundary.update(process=process_proof, currentClaim=claimed,
                                            oldAuthPath=True, originalRetiredWithoutConsume=True,
                                            originalPlanUnchanged=True)
                                        if args.case == "weekly-quota-rollback":
                                            with model.lock:
                                                probe.require(len(model.requests) == report["requestsBeforeRenewal"],
                                                    "quota rollback replayed the original model turn")
                                            probe.require(not new.exists(), "unavailable target was restored by the host")
                                            report["quotaRollbackRestored"] = True
                                            report["singleQuotaChargeAndTerminalCleanup"] = True
                                            report["requestCount"] = report["requestsBeforeRenewal"]
                                            renewed = True
                                            break
                            if boundary is None or "process" not in boundary:
                                continue
                        if args.case == "owner-stop-after-signal":
                            actions = native_actions(root)
                            operation = report.get("activeStop", {}).get("operation") or signalled_operation(actions)
                            state = pending_receipt(root, report["thread"], report["wall"], operation)
                            if "activeStop" not in report and (root / "signal-ack-dropped").exists():
                                probe.require(metadata is not None and state is not None and operation is not None
                                    and state["phase"] == "committing" and state.get("candidate") is None
                                    and state.get("current_attempt") is None,
                                    "active Stop missed the actual post-Signal pre-launch boundary")
                                reply = control(root, metadata, f"hold {metadata['sid']} on reason=active-stop-fixture")
                                probe.require(reply.startswith("OK") and "hold=1" in
                                    control(root, metadata, f"@{metadata['sid']} status").split(),
                                    "active Stop hold was not observed")
                                report["activeStop"] = {"operation": operation, "holdSetReply": reply}
                                (root / "continue-signal-ack").touch()
                            if "activeStop" in report:
                                stopped = report["activeStop"]
                                denied = next((event for event in actions if event["operation"] == operation
                                    and event["action"] == "resume" and event["replyOk"] is False
                                    and event["nativeHold"] is True), None)
                                if denied is not None and "deniedResume" not in stopped:
                                    stopped["deniedResume"] = denied
                                    (root / "owner-stop").touch()
                                completed = root / "owner-stop-completed.json"
                                if completed.exists() and "completed" not in stopped:
                                    outcome = json.loads(completed.read_text())
                                    before, after = outcome.get("before") or {}, outcome.get("after") or {}
                                    probe.require(outcome.get("operation") == operation and outcome.get("ok") is True
                                        and outcome.get("reply") == "recovery cancelled",
                                        "actual no-vendor Stop did not durably cancel recovery")
                                    probe.require(before.get("operation") == operation and after.get("operation") == operation
                                        and before.get("observationTriggeredReconciliation") is False
                                        and after.get("observationTriggeredReconciliation") is False
                                        and before.get("state") == "active" and before.get("deadlineExpired") is False
                                        and before.get("signalAccepted") is True and before.get("resumeSent") is False
                                        and before.get("sourceBirthAlive") is False and before.get("sourceGroupGone") is True
                                        and before.get("sourceWatchExited") is True
                                        and type(before.get("monotonicUs")) is int
                                        and type(before.get("deadlineUs")) is int
                                        and 0 < before["monotonicUs"] < before["deadlineUs"],
                                        "Stop did not begin with an active, unexpired operation and an exited vendor")
                                    probe.require(after.get("state") == "completed"
                                        and after.get("decision") == "retired-no-exec-grant"
                                        and after.get("deadlineExpired") is False
                                        and after.get("deadlineUs") == before.get("deadlineUs")
                                        and type(after.get("monotonicUs")) is int
                                        and before["monotonicUs"] <= after["monotonicUs"] < before["deadlineUs"]
                                        and after.get("sourceWatchExited") is True
                                        and after.get("closedRestartQuiescent") is True
                                        and after.get("inputCustodySettled") is True
                                        and after.get("inputAttemptSettled") is True
                                        and after.get("goGrantedChildren") == 0 and after.get("retainedChildren") == 0,
                                        "active cancellation did not promptly prove terminal no-launch and settled input")
                                    stopped["completed"] = outcome
                                worker = root / f"renewal-worker-{operation}-stopped.json"
                                if "completed" in stopped and worker.exists() and "workerExit" not in stopped:
                                    exited = json.loads(worker.read_text())
                                    probe.require(exited.get("operation") == operation
                                        and exited.get("deadlineUs") == stopped["completed"]["before"]["deadlineUs"]
                                        and type(exited.get("monotonicUs")) is int
                                        and stopped["completed"]["before"]["monotonicUs"]
                                            <= exited["monotonicUs"] < exited["deadlineUs"],
                                        "cancelled coordinator waited for its original deadline")
                                    ledger_dir = root / "host-state" / "accounts-codex" / "renewal-follow"
                                    cancellation = json.loads((ledger_dir / f"{operation}.revoked.json").read_text())
                                    probe.require(state is not None and cancellation.get("root") == operation
                                        and cancellation.get("intent") == state["intent"]
                                        and cancellation.get("reason") == "owner_stop"
                                        and type(cancellation.get("human_seq")) is int
                                        and type(stopped["completed"]["before"].get("expectedHumanSeq")) is int
                                        and cancellation["human_seq"] > stopped["completed"]["before"]["expectedHumanSeq"],
                                        "Stop lacks the exact durable original-intent cancellation")
                                    reply = control(root, metadata, f"hold {metadata['sid']} off")
                                    probe.require(reply.startswith("OK") and "hold=0" in
                                        control(root, metadata, f"@{metadata['sid']} status").split(),
                                        "cancelled recovery hold did not lift")
                                    stopped.update(workerExit=exited, durableCancellation={
                                        "root": cancellation["root"], "reason": cancellation["reason"],
                                        "humanSeq": cancellation["human_seq"]}, holdLiftReply=reply)
                                    publish_fixture_request(root / "observe-native-operation.json", {"operation": operation})
                                observation = root / "native-operation-observed.json"
                                if "workerExit" in stopped and observation.exists():
                                    after_lift = json.loads(observation.read_text())
                                    state = pending_receipt(root, report["thread"], report["wall"], operation)
                                    probe.require(after_lift.get("state") == "completed"
                                        and after_lift.get("operation") == operation
                                        and after_lift.get("deadlineUs") == stopped["completed"]["before"]["deadlineUs"]
                                        and after_lift.get("deadlineExpired") is False
                                        and type(after_lift.get("monotonicUs")) is int
                                        and stopped["completed"]["after"]["monotonicUs"]
                                            <= after_lift["monotonicUs"] < after_lift["deadlineUs"]
                                        and after_lift.get("decision") == "retired-no-exec-grant"
                                        and after_lift.get("inputCustodySettled") is True
                                        and state is not None and state["phase"] == "retired"
                                        and state.get("current_attempt") is None and state.get("candidate") is None
                                        and state.get("successor_pid") is None and state.get("accepted_turn") is None,
                                        "lifting the hold revived cancelled native work")
                                    probe.require(json.loads((root / "host-state" / "accounts-codex" / "renewal-follow"
                                        / f"{operation}.revoked.json").read_text()) == cancellation,
                                        "lifting the hold changed the durable cancellation")
                                    probe.require(not any(event["action"] in ("resume", "attach", "execute")
                                        and event["replyOk"] is True for event in native_actions(root)),
                                        "active Stop admitted launcher work")
                                    report["nativeSignalEffect"] = original_signal_effect(root, report)
                                    if weekly_quota:
                                        pool = root / "host-state" / "accounts-codex"
                                        bindings = json.loads((pool / "logins.json").read_text())
                                        if operation in bindings.get("native_rotations", {}):
                                            continue
                                        charged = json.loads((pool / "state.json").read_text())
                                        probe.require(len(charged.get("rotations", [])) == 1
                                            and state.get("launch_attempted") is True,
                                            "cancelled quota cleanup changed its charge or signal history")
                                        report["cancelledQuotaCleanup"] = True
                                    with model.lock:
                                        probe.require(len(model.requests) == report["requestsBeforeRenewal"],
                                            "active Stop replayed model work")
                                    stopped.update(afterHoldLift=after_lift, activeOperationClosed=True,
                                        inputCustodySettled=True, noSuccessorLaunched=True)
                                    report.update(ownerStopRespected=True, requestCount=report["requestsBeforeRenewal"])
                                    renewed = True
                                    break
                            continue
                        if args.case == "successor-prefix-crash":
                            actions = native_actions(root)
                            operation = report.get("prefixCrash", {}).get("operation") or signalled_operation(actions)
                            state = pending_receipt(root, report["thread"], report["wall"], operation)
                            checkpoint = launcher_root / "forward-after-go.reached"
                            if "prefixCrash" not in report and checkpoint.exists():
                                prefix = (state or {}).get("candidate") or {}
                                executed = [event for event in actions if event["action"] == "execute"
                                    and event["replyOk"] is True]
                                if state is not None and len(executed) == 1:
                                    probe.require(metadata is not None and state["phase"] == "committing"
                                        and state.get("current_attempt") is None and state.get("successor_pid") is None
                                        and prefix.get("forward") is True
                                        and prefix.get("pid") == int(checkpoint.read_text())
                                        and type(prefix.get("birth")) is int and prefix["birth"] > 0
                                        and executed[0]["operation"] == operation,
                                        "crash fixture missed the actual Go-granted prefix and kernel birth")
                                    fields = dict(word.split("=", 1) for word in state["native_prepare"].split() if "=" in word)
                                    reply = control(root, metadata, f"hold {metadata['sid']} on reason=prefix-crash-fixture")
                                    probe.require(reply.startswith("OK") and "hold=1" in
                                        control(root, metadata, f"@{metadata['sid']} status").split(),
                                        "Go-prefix crash hold was not observed")
                                    report["prefixCrash"] = {"operation": operation,
                                        "prefix": {key: prefix[key] for key in ("pid", "birth", "parent", "group", "session", "forward")},
                                        "originalDeadlineUs": int(fields["deadline-us"]),
                                        "originalExpiresMs": state["expires_ms"],
                                        "originalPrepareSha256": hashlib.sha256(state["native_prepare"].encode()).hexdigest(),
                                        "holdSetReply": reply, "goGrant": executed[0],
                                        "mechanism": "actual after-Go prefix callback timeout before target exec"}
                                    observe_expired_after = time.monotonic() + 121
                            if "prefixCrash" in report and not hold_released:
                                boundary = report["prefixCrash"]
                                with model.lock:
                                    probe.require(len(model.requests) == report["requestsBeforeRenewal"],
                                        "Go-granted dead prefix submitted model work")
                                if time.monotonic() >= observe_expired_after:
                                    request = root / "observe-native-operation.json"
                                    if not request.exists():
                                        publish_fixture_request(request, {"operation": boundary["operation"]})
                                observation_path = root / "native-operation-observed.json"
                                if observation_path.exists():
                                    closed = json.loads(observation_path.read_text())
                                    validate_prefix_crash_closure(boundary, state, closed)
                                    (launcher_root / "forward-after-go.pause").unlink()
                                    reply = control(root, metadata, f"hold {metadata['sid']} off")
                                    probe.require(reply.startswith("OK") and "hold=0" in
                                        control(root, metadata, f"@{metadata['sid']} status").split(),
                                        "dead-prefix recovery hold did not lift")
                                    boundary.update(nativeClosure=closed, holdLiftReply=reply,
                                        retainedGoCohortDeathProven=True, inputCustodySettled=True)
                                    hold_released = True
                            if not hold_released:
                                continue
                        if args.case == "post-signal-resume":
                            actions = native_actions(root)
                            operation = report.get("postSignal", {}).get("operation") or signalled_operation(actions)
                            state = pending_receipt(root, report["thread"], report["wall"], operation)
                            if "postSignal" not in report and (root / "signal-ack-dropped").exists():
                                probe.require(metadata is not None and state is not None
                                    and state.get("phase") == "committing" and state.get("candidate") is None
                                    and state.get("current_attempt") is None,
                                    "post-Signal fixture missed the original pre-launch boundary")
                                operation = state["id"]
                                probe.require(any(event["action"] == "signal" and event["operation"] == operation
                                    and event["replyOk"] is True for event in actions), "original Signal did not complete")
                                fields = dict(word.split("=", 1) for word in state["native_prepare"].split() if "=" in word)
                                reply = control(root, metadata, f"hold {metadata['sid']} on reason=renewal-expiry-fixture")
                                probe.require(reply.startswith("OK") and "hold=1" in
                                    control(root, metadata, f"@{metadata['sid']} status").split(),
                                    "post-Signal hold was not observed")
                                report["postSignal"] = {"operation": operation,
                                    "originalDeadlineUs": int(fields["deadline-us"]),
                                    "originalExpiresMs": state["expires_ms"],
                                    "originalPrepareSha256": hashlib.sha256(state["native_prepare"].encode()).hexdigest(),
                                    "holdSetReply": reply}
                                # Time schedules the observation; its positive native proof,
                                # rather than elapsed silence, establishes terminal closure.
                                observe_expired_after = time.monotonic() + 121
                                (root / "continue-signal-ack").touch()
                            if "postSignal" in report and not hold_released:
                                boundary = report["postSignal"]
                                probe.require(state is not None and state["id"] == boundary["operation"]
                                    and state["phase"] == "committing" and state.get("candidate") is None
                                    and state.get("current_attempt") is None,
                                    "held continuation launched or replaced the original intent")
                                with model.lock:
                                    probe.require(len(model.requests) == report["requestsBeforeRenewal"],
                                        "held continuation replayed model work")
                                if time.monotonic() >= observe_expired_after:
                                    request = root / "observe-native-operation.json"
                                    if not request.exists():
                                        publish_fixture_request(request, {"operation": boundary["operation"]})
                                observation_path = root / "native-operation-observed.json"
                                if observation_path.exists():
                                    closed = json.loads(observation_path.read_text())
                                    probe.require(closed.get("operation") == boundary["operation"]
                                        and closed.get("state") == "completed"
                                        and closed.get("decision") == "retired-no-exec-grant"
                                        and closed.get("deadlineExpired") is True
                                        and closed.get("sourceBirthAlive") is False
                                        and closed.get("sourceGroupGone") is True
                                        and closed.get("retainedSourceWatch") is True
                                        and closed.get("sourceWatchExited") is True
                                        and closed.get("closedRestartWitness") is True
                                        and closed.get("closedRestartQuiescent") is True
                                        and closed.get("closedRestartAvailable") is True
                                        and closed.get("deadlineUs") == boundary["originalDeadlineUs"]
                                        and type(closed.get("monotonicUs")) is int
                                        and closed["monotonicUs"] >= closed["deadlineUs"]
                                        and closed.get("inputAttemptSettled") is True
                                        and closed.get("goGrantedChildren") == 0
                                        and closed.get("retainedChildren") == 0,
                                        "original operation lacks positive terminal no-launch proof")
                                    denied = next((event for event in actions
                                        if event["operation"] == boundary["operation"] and event["action"] == "resume"
                                        and event["replyOk"] is False and event["nativeHold"] is True), None)
                                    probe.require(denied is not None, "hold did not refuse an actual Resume")
                                    reply = control(root, metadata, f"hold {metadata['sid']} off")
                                    probe.require(reply.startswith("OK") and "hold=0" in
                                        control(root, metadata, f"@{metadata['sid']} status").split(),
                                        "post-Signal hold did not lift")
                                    boundary.update(nativeClosure=closed, deniedResume=denied,
                                        holdLiftReply=reply,
                                        recordsAtHoldLift=structural_records(root / "codex-home", report["thread"]))
                                    hold_released = True
                            if not hold_released:
                                continue
                        if args.case == "post-signal-expiry-diagnostic":
                            actions = native_actions(root)
                            operation = report.get("expiryDiagnostic", {}).get("operation") or signalled_operation(actions)
                            state = pending_receipt(root, report["thread"], report["wall"], operation)
                            if "expiryDiagnostic" not in report and (root / "signal-ack-dropped").exists():
                                probe.require(metadata is not None and state is not None
                                    and state.get("phase") == "committing" and state.get("candidate") is None,
                                    "expiry diagnostic missed actual post-Signal pre-launch boundary")
                                operation = state["id"]
                                probe.require(any(event["action"] == "signal" and event["operation"] == operation
                                    and event["replyOk"] is True for event in actions), "Signal did not complete")
                                reply = control(root, metadata, f"hold {metadata['sid']} on reason=renewal-expiry-fixture")
                                probe.require(reply.startswith("OK"), "expiry hold was refused")
                                probe.require("hold=1" in control(root, metadata, f"@{metadata['sid']} status").split(),
                                    "expiry hold was not observed")
                                report["expiryDiagnostic"] = {"operation":operation,"holdSetReply":reply,
                                    "recordsAfterSignal":structural_records(root / "codex-home", report["thread"])}
                                (root / "continue-signal-ack").touch()
                            if "expiryDiagnostic" in report:
                                diagnostic = report["expiryDiagnostic"]
                                operation = diagnostic["operation"]
                                exited_path = root / f"renewal-worker-{operation}-stopped.json"
                                if exited_path.exists() and "workerExit" not in diagnostic:
                                    exit_event = json.loads(exited_path.read_text())
                                    probe.require(exit_event["operation"] == operation
                                        and exit_event["monotonicUs"] >= exit_event["deadlineUs"],
                                        "coordinator did not actually exit past its original budget")
                                    denied = next((event for event in actions if event["operation"] == operation
                                        and event["action"] == "resume" and event["replyOk"] is False
                                        and event["nativeHold"] is True), None)
                                    probe.require(denied is not None, "hold did not causally refuse an actual Resume")
                                    diagnostic.update(workerExit=exit_event, deniedResume=denied,
                                        recordsAtWorkerExit=structural_records(root / "codex-home", report["thread"]))
                                    reply = control(root, metadata, f"hold {metadata['sid']} off")
                                    probe.require(reply.startswith("OK"), "expiry hold did not lift")
                                    probe.require("hold=0" in control(root, metadata, f"@{metadata['sid']} status").split(),
                                        "expiry hold remained after lift")
                                    diagnostic["holdLiftReply"] = reply
                                    diagnostic["recordsAfterHoldLift"] = structural_records(root / "codex-home", report["thread"])
                                    publish_fixture_request(root / "observe-native-operation.json", {"operation":operation})
                                observation_path = root / "native-operation-observed.json"
                                if "workerExit" in diagnostic and observation_path.exists():
                                    observed_native = json.loads(observation_path.read_text())
                                    probe.require(observed_native.get("operation") == operation
                                        and observed_native.get("state") == "completed"
                                        and observed_native.get("decision") == "retired-no-exec-grant"
                                        and observed_native.get("deadlineExpired") is True
                                        and observed_native.get("sourceBirthAlive") is False
                                        and observed_native.get("sourceGroupGone") is True
                                        and observed_native.get("inputAttemptSettled") is True
                                        and observed_native.get("goGrantedChildren") == 0
                                        and observed_native.get("retainedChildren") == 0,
                                        "old operation was not positively closed without a launch")
                                    state = pending_receipt(root, report["thread"], report["wall"], operation)
                                    probe.require(state["id"] == operation and state["phase"] == "committing"
                                        and state["launch_attempted"] is True and state.get("successor_pid") is None
                                        and state.get("candidate") is None,
                                        "diagnostic did not expose the unresolved logical receipt")
                                    with model.lock:
                                        probe.require(len(model.requests) == report["requestsBeforeRenewal"],
                                            "expiry diagnostic replayed original model work")
                                    diagnostic.update(nativeObservation=observed_native,
                                        logicalReceiptPhase=state["phase"], launchAttempted=True, successorPid=None)
                                    report.update(diagnosticValidated=True, unresolvedGoalGap=True, safeHeld=True,
                                        requestCount=report["requestsBeforeRenewal"])
                                    renewed = True
                                    break
                            continue
                        if args.case == "grace-beyond-budget":
                            if "grace" not in report and (root / "prepare-ack-paused").exists():
                                old_ready = pending_receipt(root, report["thread"], report["wall"])
                                probe.require(old_ready is not None and old_ready.get("phase") == "ready"
                                    and old_ready.get("launch_attempted") is False,
                                    "grace did not begin at an actual uncommitted Ready receipt")
                                fields = dict(word.split("=", 1) for word in old_ready["native_prepare"].split() if "=" in word)
                                report["grace"] = {"oldOperation": old_ready["id"],
                                    "oldExpiresMs": old_ready["expires_ms"],
                                    "oldDeadlineUs": int(fields["deadline-us"]),
                                    "oldHumanSeq": old_ready["human_seq"], "humanGraceSeconds": 121}
                                (root / "person-gesture").touch()
                            gesture_path = root / "person-gesture-completed.json"
                            if "grace" in report and "personGesture" not in report["grace"] and gesture_path.exists():
                                gesture = json.loads(gesture_path.read_text())
                                probe.require(gesture["after"] > gesture["before"]
                                    and gesture["before"] == report["grace"]["oldHumanSeq"]
                                    and type(gesture.get("humanAgeMs")) is int,
                                    "actual person clock did not advance at the Ready boundary")
                                report["grace"]["personGesture"] = gesture
                                (root / "continue-prepare-ack").touch()
                        if args.case == "owner-stop":
                            operation = report.get("ownerStop", {}).get("operation")
                            state = pending_receipt(root, report["thread"], report["wall"], operation)
                            if "ownerStop" not in report and (root / "before-signal.reached").exists():
                                probe.require(state is not None and state.get("phase") == "committing"
                                    and state.get("successor_pid") is None,
                                    "OwnerStop missed the prepared pre-Signal boundary")
                                owner_stop_initial = state
                                report["ownerStop"] = {"operation": state["id"]}
                                (root / "owner-stop").touch()
                            stopped = report.get("ownerStop")
                            completed_stop = root / "owner-stop-completed.json"
                            if stopped is not None and completed_stop.exists() and "completed" not in stopped:
                                outcome = json.loads(completed_stop.read_text())
                                probe.require(outcome.get("ok") is True
                                    and outcome.get("reply", "").startswith("signalled "),
                                    "production OwnerStop did not signal the actual live vendor")
                                stopped["completed"] = outcome
                                (root / "before-signal.continue").touch()
                            if stopped is not None and "completed" in stopped:
                                operation = stopped["operation"]
                                worker = root / f"renewal-worker-{operation}-stopped.json"
                                if worker.exists() and "workerExit" not in stopped:
                                    stopped["workerExit"] = json.loads(worker.read_text())
                                    publish_fixture_request(root / "observe-native-operation.json", {"operation": operation})
                                observation = root / "native-operation-observed.json"
                                if "workerExit" in stopped and observation.exists():
                                    state = pending_receipt(root, report["thread"], report["wall"], operation)
                                    ledger_dir = root / "host-state" / "accounts-codex" / "renewal-follow"
                                    cancellation = json.loads((ledger_dir / f"{operation}.revoked.json").read_text())
                                    observed_stop = json.loads(observation.read_text())
                                    validate_presignal_stop(operation, owner_stop_initial, state,
                                        cancellation, stopped["workerExit"], observed_stop)
                                    actions = native_actions(root)
                                    probe.require(not any(event["action"] in ("resume", "attach", "execute")
                                        or (event["action"] == "signal" and event["replyOk"] is True)
                                        for event in actions), "OwnerStop admitted native mutation work")
                                    effects = root / "native-signal-effects.jsonl"
                                    probe.require(not effects.exists() or (effects.is_file() and not effects.is_symlink()
                                        and effects.stat().st_size == 0), "OwnerStop performed a native renewal signal")
                                    if weekly_quota:
                                        pool = root / "host-state" / "accounts-codex"
                                        bindings = json.loads((pool / "logins.json").read_text())
                                        if operation in bindings.get("native_rotations", {}):
                                            continue
                                        charged = json.loads((pool / "state.json").read_text())
                                        probe.require(len(charged.get("rotations", [])) == 1,
                                            "cancelled quota attempt changed its charge")
                                        report["cancelledQuotaCleanup"] = True
                                    stopped.update(retired=True, nativeOperationClosed=True,
                                        inputCustodySettled=True, durableCancellation=True,
                                        nativeObservation=observed_stop, successorPid=None)
                                    with model.lock:
                                        probe.require(len(model.requests) == report["requestsBeforeRenewal"],
                                            "OwnerStop replayed model work")
                                    report["requestCount"] = report["requestsBeforeRenewal"]
                                    renewed = True
                                    break
                            continue
                        if args.case == "late-hold" and not hold_released:
                            actions = native_actions(root)
                            operation = report.get("lateHold", {}).get("operation") or signalled_operation(actions)
                            state = pending_receipt(root, report["thread"], report["wall"], operation)
                            if not hold_set and (root / "signal-ack-dropped").exists():
                                probe.require(metadata is not None and state is not None,
                                              "signal boundary lacks native operation evidence")
                                probe.require(state.get("phase") == "committing" and state.get("candidate") is None,
                                              "late hold missed the pre-launch boundary")
                                operation = state["id"]
                                probe.require(any(event["action"] == "signal" and event["operation"] == operation
                                    and event["replyOk"] is True for event in actions), "native Signal was not completed")
                                reply = control(root, metadata, f"hold {metadata['sid']} on reason=renewal-fixture")
                                probe.require(reply.startswith("OK"), "native Owner hold was refused")
                                status = control(root, metadata, f"@{metadata['sid']} status")
                                probe.require("hold=1" in status.split(), "native hold was not observed")
                                report["lateHold"] = {"operation": operation, "holdSetReply": reply}
                                (root / "continue-signal-ack").touch()
                                hold_set = True
                            if hold_set:
                                operation = report["lateHold"]["operation"]
                                denied = next((event for event in actions if event["operation"] == operation
                                    and event["action"] == "resume" and event["replyOk"] is False
                                    and event["nativeHold"] is True), None)
                                if denied:
                                    probe.require(state.get("candidate") is None,
                                                  "native hold allowed a launcher candidate")
                                    reply = control(root, metadata, f"hold {metadata['sid']} off")
                                    probe.require(reply.startswith("OK"), "native Owner hold did not lift")
                                    status = control(root, metadata, f"@{metadata['sid']} status")
                                    probe.require("hold=0" in status.split(), "native hold remained after lift")
                                    report["lateHold"].update(deniedResume=denied, holdLiftReply=reply)
                                    hold_released = True
                            if not hold_released:
                                continue
                        candidate = next((record for record in observed if record["pid"] != previous["pid"]
                            and record.get("sessionId") == previous["sessionId"]
                            and record.get("env", {}).get("CODEX_AUTH_PATH") == str(final_auth)), None)
                        with model.lock:
                            count = len(model.requests)
                        receipt = consumed_receipt(root, report["thread"], report["wall"], candidate["pid"] if candidate else None)
                        if candidate and receipt and count > report["requestsBeforeRenewal"] and successor_rest(candidate, report["thread"]) is not None:
                            report["successorRest"] = successor_rest(candidate, report["thread"])
                            probe.require(count == report["requestsBeforeRenewal"] + (2 if chain else 1), "renewal replayed model work")
                            source_process = report["rollback"]["process"] if rollback_case else report["originalProcess"]
                            if chain:
                                first = next((json.loads(path.read_text()) for path in
                                    (root / "host-state/accounts-codex/renewal-follow").glob("*.json")
                                    if json.loads(path.read_text()).get("old_auth") == str(old)), None)
                                probe.require(first is not None and first.get("phase") == "consumed"
                                    and first.get("new_auth") == str(new)
                                    and first.get("old_pid") == report["originalPid"]
                                    and first.get("old_birth") == report["originalProcess"]["kernelBirth"]
                                    and first.get("accepted_turn") == report["wall"]
                                    and first.get("intent", {}).get("thread") == report["thread"]
                                    and first.get("id") != receipt["id"]
                                    and first["destination"] == receipt["intent"]["identity"],
                                    "quota chain did not preserve the first handoff")
                                first_proof = consumed_receipt(root, report["thread"], report["wall"], first["successor_pid"])
                                probe.require(first_proof is not None and first_proof["id"] == first["id"],
                                    "first quota handoff has no completed native proof")
                                source_process = {"pid": first["successor_pid"], "kernelBirth": first["successor_birth"]}
                                report["quotaChain"] = {"firstReceipt": first["id"], "secondReceipt": receipt["id"],
                                    "middleProcess": source_process}
                            probe.require(receipt["old_pid"] == source_process["pid"] and receipt["successor_pid"] == candidate["pid"], "native receipt process binding changed")
                            probe.require(receipt["accepted_turn"] == report["wall"], "native receipt accepted a different turn")
                            report["nativeReceipt"] = receipt
                            if weekly_quota:
                                probe.require(receipt["intent"].get("purpose") == "weekly_quota"
                                    and receipt["destination"]["account"] != receipt["intent"]["identity"]["account"]
                                    and receipt["old_auth"] == str(new if chain else old) and receipt["new_auth"] == str(final_auth),
                                    "quota handoff did not bind distinct source and destination accounts")
                                pool = root / "host-state" / "accounts-codex"
                                ledger = json.loads((pool / "state.json").read_text())
                                bindings = json.loads((pool / "logins.json").read_text())
                                probe.require(len(ledger.get("rotations", [])) == (2 if chain else 1)
                                    and not bindings.get("native_rotations", {}),
                                    "quota rotation was charged more than once or terminal binding was retained")
                                report["twoQuotaChargesAndTerminalCleanup" if chain else "singleQuotaChargeAndTerminalCleanup"] = True
                            if rollback_case:
                                boundary = report["rollback"]
                                fresh = pending_receipt(root, report["thread"], report["wall"], receipt["id"])
                                fresh_fields = dict(word.split("=", 1) for word in fresh["native_prepare"].split()
                                    if "=" in word)
                                probe.require(fresh["generation"] == boundary["credentialGeneration"]
                                    and fresh["row_stamp"] == rollback_row_stamp
                                    and fresh["old_auth"] == str(old) and fresh["new_auth"] == str(new)
                                    and fresh["expires_ms"] > boundary["originalExpiresMs"]
                                    and int(fresh_fields["deadline-us"]) > boundary["originalDeadlineUs"]
                                    and fresh.get("current_attempt") is None,
                                    "live rollback did not use a new budget against the same proven credential generation")
                                probe.require(receipt["id"] != boundary["operation"]
                                    and receipt["nativeOperationId"] == receipt["id"],
                                    "live rollback reused retired native authority")
                                report["nativeSignalEffects"] = rollback_signal_effects(root, report, source_process)
                                launches = [event for event in native_actions(root)
                                    if event["action"] == "execute" and event["replyOk"] is True]
                                probe.require([event["operation"] for event in launches] ==
                                    [boundary["operation"], receipt["id"]],
                                    "failed forward executed or rollback recovery duplicated a launch")
                                boundary.update(freshLiveSourceOperation=receipt["id"],
                                    freshSourceBirthBound=True, terminalAuthorityNeverReused=True,
                                    freshDeadlineUs=int(fresh_fields["deadline-us"]),
                                    sameCredentialGeneration=True)
                            elif chain:
                                report["nativeSignalEffects"] = rollback_signal_effects(root, report, source_process)
                            else:
                                report["nativeSignalEffect"] = original_signal_effect(root, report)
                            if args.case in ("post-signal-resume", "successor-prefix-crash"):
                                boundary = report["prefixCrash"] if args.case == "successor-prefix-crash" else report["postSignal"]
                                receipt_path = root / "host-state" / "accounts-codex" / "renewal-follow" / f"{receipt['id']}.json"
                                logical = json.loads(receipt_path.read_text())
                                prepare, attempt = active_prepare(receipt_path, logical)
                                probe.require(receipt["id"] == boundary["operation"] and attempt is not None
                                    and logical["expires_ms"] == boundary["originalExpiresMs"]
                                    and hashlib.sha256(logical["native_prepare"].encode()).hexdigest()
                                        == boundary["originalPrepareSha256"],
                                    "continuation replaced the logical root or extended its original plan")
                                fields = dict(word.split("=", 1) for word in prepare.split() if "=" in word)
                                child = attempt["reference"]["id"]
                                fresh_deadline = int(fields["deadline-us"])
                                probe.require(child != boundary["operation"]
                                    and fresh_deadline - 120_000_000 >= boundary["originalDeadlineUs"]
                                    and receipt["nativeOperationId"] == child,
                                    "continuation did not use a distinct freshly budgeted native attempt")
                                actions = native_actions(root)
                                signals = [event for event in actions if event["action"] == "signal"]
                                probe.require(signals and all(event["operation"] == boundary["operation"] for event in signals)
                                    and any(event["replyOk"] is True for event in signals),
                                    "continuation sent Signal to a different operation")
                                launches = [event for event in actions if event["action"] in ("resume", "attach", "execute")
                                    and event["replyOk"] is True]
                                if args.case == "successor-prefix-crash":
                                    probe.require(attempt["reference"]["generation"] == 1
                                        and attempt["parent"]["id"] == boundary["operation"]
                                        and attempt["native_predecessor"] == boundary["operation"],
                                        "dead Go prefix did not produce a distinct directly linked first continuation")
                                    executions = [event for event in launches if event["action"] == "execute"]
                                    probe.require([event["operation"] for event in executions] ==
                                        [boundary["operation"], child]
                                        and sum(event["operation"] == child for event in executions) == 1,
                                        "dead Go prefix was replayed or fresh continuation duplicated its launch")
                                else:
                                    probe.require(launches and all(event["operation"] == child for event in launches)
                                        and sum(event["action"] == "execute" for event in launches) == 1,
                                        "continuation reused old launch authority or executed more than once")
                                boundary.update(freshAttempt=attempt["reference"], freshDeadlineUs=fresh_deadline,
                                    originalDeadlineUnchanged=True,
                                    singleOriginalSignal=True, signalRpcReplyCount=len(signals))
                                if args.case == "successor-prefix-crash":
                                    boundary.update(oldOperationNeverRelaunched=True, freshAttemptOnlyNewLaunch=True)
                                else:
                                    boundary.update(oldOperationNeverLaunched=True, freshAttemptOnlyLaunch=True)
                            if args.case == "grace-beyond-budget":
                                grace = report["grace"]
                                receipt_dir = root / "host-state" / "accounts-codex" / "renewal-follow"
                                old_receipt = json.loads((receipt_dir / f"{grace['oldOperation']}.json").read_text())
                                fresh = json.loads((receipt_dir / f"{receipt['id']}.json").read_text())
                                fields = dict(word.split("=", 1) for word in fresh["native_prepare"].split() if "=" in word)
                                probe.require(old_receipt["phase"] == "retired" and old_receipt["launch_attempted"] is False
                                    and old_receipt.get("successor_pid") is None,
                                    "expired Ready receipt did not retire without launching")
                                old_fields = dict(word.split("=", 1) for word in old_receipt["native_prepare"].split() if "=" in word)
                                fresh_admission = int(fields["deadline-us"]) - 120_000_000
                                probe.require(int(old_fields["deadline-us"]) == grace["oldDeadlineUs"]
                                    and fresh_admission >= grace["oldDeadlineUs"]
                                    and fresh_admission >= grace["personGesture"]["beforeMonotonicUs"] + 121_000_000,
                                    "fresh admission did not follow the unchanged original budget and actual person grace")
                                probe.require(receipt["id"] != grace["oldOperation"]
                                    and fresh["human_seq"] == grace["personGesture"]["after"]
                                    and fresh["expires_ms"] > grace["oldExpiresMs"]
                                    and int(fields["deadline-us"]) > grace["oldDeadlineUs"],
                                    "recovery extended the old_receipt budget instead of fresh person-bound admission")
                                probe.require(not any(event["operation"] == grace["oldOperation"]
                                    for event in native_actions(root)), "expired Ready operation performed native work")
                                grace.update(oldRetiredWithoutLaunch=True, freshOperation=receipt["id"],
                                    freshDeadlineUs=int(fields["deadline-us"]),
                                    freshAdmissionUs=fresh_admission, originalDeadlineUnchanged=True,
                                    freshHumanSeq=fresh["human_seq"])
                            if args.case == "late-hold":
                                probe.require(receipt["nativeOperationId"] == report["lateHold"]["operation"],
                                              "lifting the hold minted a different operation")
                            report["renewedPid"] = candidate["pid"]
                            report["renewedProcess"] = process_evidence(candidate)
                            probe.require(receipt["old_birth"] == source_process["kernelBirth"]
                                and receipt["successor_birth"] == report["renewedProcess"]["kernelBirth"],
                                "native receipt process birth binding changed")
                            probe.require(report["originalProcess"]["exe"] == str(codex), "unexpected original image")
                            probe.require(report["renewedProcess"]["exe"] == str(codex), "unexpected renewed image")
                            probe.require(model.requests[-1]["newTokenMatched"], "renewed request did not use new token")
                            report["newTokenMatched"] = True
                            rollout_events = []
                            for rollout in (root / "codex-home" / "sessions").rglob("*.jsonl"):
                                if report["thread"] in rollout.name:
                                    rollout_events.extend(json.loads(line) for line in rollout.read_text().splitlines())
                            native_starts = validate_completed_original_turn(rollout_events, report["wall"], 3 if chain else 2)
                            report["completedPlanPreserved"] = True
                            report["nativeTurnIds"] = native_starts
                            report["originalTurnPreserved"] = True
                            report["requestCount"] = count
                            report["sameThread"] = True
                            report["newAuthPath"] = True
                            renewed = True
                            break
                hang = "native same-account renewal did not reach its required causal outcome before the hang detector"
                if not renewed:
                    hang += "; " + live_snapshot_outcomes(root / "renewal-control-trace.jsonl")
                if not renewed and weekly_quota:
                    with model.lock:
                        seen = list(model.http_events)
                    hang += "; " + wall_evidence(seen, root / "renewal-control-trace.jsonl")
                probe.require(renewed, hang)
            finally:
                (root / "stop").touch()

        faults = {"lost-signal-ack": ("drop-signal-ack", "signal-ack-dropped"),
                  "panic-before-signal": ("panic-before-signal", "panic-before-signal-fired"),
                  "lost-consume-ack": ("drop-consume-ack", "consume-ack-dropped"),
                  "late-hold": ("drop-signal-ack", "signal-ack-dropped"),
                  "owner-stop-after-signal": ("drop-signal-ack", "signal-ack-dropped"),
                  "post-signal-resume": ("drop-signal-ack", "signal-ack-dropped"),
                  "post-signal-expiry-diagnostic": ("drop-signal-ack", "signal-ack-dropped")}
        if args.case in faults:
            (root / faults[args.case][0]).touch()
        if args.case in ("late-hold", "owner-stop-after-signal", "post-signal-resume", "post-signal-expiry-diagnostic"):
            (root / "pause-signal-ack").touch()
        if args.case == "successor-prefix-crash" or rollback_case:
            launcher_root.mkdir(parents=True, mode=0o700)
        if args.case == "successor-prefix-crash":
            (launcher_root / "forward-after-go.pause").touch()
        if rollback_case:
            (launcher_root / "forward-before-attach.pause").touch()
        if args.case == "owner-stop":
            (root / "pause-before-signal").touch()
        if args.case == "grace-beyond-budget":
            (root / "pause-prepare-ack").touch()

        command = [str(host), "--renewal-config", str(config_path)]
        probe.write_json(root / "host-command.json", command)
        with (root / "host-stdout.jsonl").open("xb") as stdout, (root / "host-stderr.txt").open("xb") as stderr:
            process = subprocess.Popen(command, cwd=root / "work", env=env, stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE, stderr=stderr)
            try:
                drive(process, stdout)
            finally:
                (root / "stop").touch()
                try:
                    code = process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    process.terminate()
                    code = process.wait(timeout=10)
                process.stdout.close()
                probe.write_json(root / "host-wait.json", {"pid": process.pid, "returncode": code})
        probe.require(code == 0, "native host did not exit cleanly")
        probe.require(not model.unknown_requests, "unknown model request shape")
        report["auxiliaryTitleRequestCount"] = len(model.auxiliary_requests)
        if soon_wall:
            # Not a wall that yields: validate_creditless_yield does not apply.
            with model.lock:
                events = list(model.http_events)
            report["fiveHourWallWaitedOut"] = wall_verdict(
                root, events, model.requests, root / "renewal-control-trace.jsonl")
        elif weekly_quota:
            with model.lock:
                events = list(model.http_events)
            report["creditlessWallsYieldedToRotation"] = validate_creditless_yield(
                events, rotation_prepares(root / "renewal-control-trace.jsonl"))
        probe.require(len(model.requests) >= (2 if args.case in ("owner-stop", "owner-stop-after-signal", "post-signal-expiry-diagnostic", "weekly-quota-rollback", "weekly-quota-exhausted") else 3),
                      "renewal did not exercise completed-tool work")
        for request in model.requests:
            user_texts = [part.get("text") for item in request["body"].get("input", [])
                          if item.get("type") == "message" and item.get("role") == "user"
                          for part in item.get("content", []) if isinstance(part, dict)]
            probe.require(user_texts.count(probe.PROMPT) == 1,
                          "original user input was lost or duplicated")
        failed_input = model.requests[1]["body"]["input"] if args.case in ("owner-stop", "owner-stop-after-signal", "post-signal-expiry-diagnostic", "weekly-quota-rollback", "weekly-quota-exhausted") else model.requests[-2]["body"]["input"]
        resumed_input = model.requests[-1]["body"]["input"]
        probe.require(resumed_input == failed_input
            and all(request["body"]["input"] == model.requests[1]["body"]["input"]
                    for request in model.requests[2:]),
            "renewal changed original input or completed tool output")
        outputs = [item for item in resumed_input if item.get("type") == "function_call_output"]
        probe.require(len(outputs) == 1 and outputs[0].get("call_id") == probe.CALL_ID,
                      "completed tool output was lost or duplicated")
        probe.require("Plan updated" in str(outputs[0].get("output")), "plan tool did not complete")
        calls = [item for item in resumed_input if item.get("type") == "function_call"]
        probe.require(len(calls) == 1 and calls[0].get("call_id") == probe.CALL_ID
            and calls[0].get("name") == "update_plan"
            and json.loads(calls[0].get("arguments", "{}")) == {
                "plan": [{"step": probe.PLAN, "status": "completed"}]},
            "completed plan call was changed or duplicated")
        report["originalInputAndToolOutputPreserved"] = True
        if args.case in ("owner-stop", "owner-stop-after-signal"):
            probe.require((root / "owner-stop-completed.json").is_file(),
                          "OwnerStop did not exercise the actual production operation")
            report["ownerStopRespected"] = True
        report["rawHostReturncode"] = code
        if args.case in faults:
            probe.require((root / faults[args.case][1]).is_file(), "requested native failure boundary was not exercised")
            report["nativeFailureBoundaryExercised"] = True
        report["passed"] = args.case != "post-signal-expiry-diagnostic"
        if args.case == "post-signal-expiry-diagnostic":
            report["error"] = "Known unresolved goal gap: expired post-Signal receipt remains Committing after hold lifts"
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
    finally:
        (root / "stop").touch()
        server.shutdown()
        server_thread.join()
        server.server_close()
        report["requestAttribution"] = {"foreground": len(model.requests),
            "threadTitle": len(model.auxiliary_requests), "unknown": len(model.unknown_requests),
            "unroutedPosts": len(model.unrouted_posts)}
        probe.write_json(root / "requests.json", model.requests)
        probe.write_json(root / "auxiliary-requests.json", model.auxiliary_requests)
        probe.write_json(root / "unknown-requests.json", model.unknown_requests)
        probe.write_json(root / "unrouted-posts.json", model.unrouted_posts)
        probe.write_json(root / "http-events.json", model.http_events)
        probe.write_json(root / "result.json", report)
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
