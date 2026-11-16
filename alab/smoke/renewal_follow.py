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
import package_standing
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
    # account; one whose reset is far off is handed to another account, as a spent week is.
    "five-hour-wall-soon": (270, 300),
    "five-hour-wall-far": (270, 300),
    # The far wall again, its 429 naming no reset: the session is handed over all the same,
    # and the wall it stands at is the usage read's (alab/PARALLEL-LOGINS.md, gap 6).
    "five-hour-wall-far-unnamed": (270, 300),
    # A lost login is moved only once the host's own refusal of its refresh token has
    # settled, a minute after the host presented it: the move takes about 75 s, and the
    # run watches for LOGIN_MOVED_WATCH_SECONDS after it.
    "lost-login-expired": (270, 300),
    "lost-login-expired-plain-issuer": (270, 300),
    # The cases in which nothing is moved end LOGIN_WALL_HOLD_SECONDS after the login wall
    # took its own path.
    "lost-login-invalidated": (180, 210),
    "lost-login-reused": (180, 210),
    "lost-login-invalid-grant": (180, 210),
    "lost-login-unrecognised": (180, 210),
    "lost-login-no-word": (180, 210),
}
# How long after the provider refused it the five-hour-wall-soon source is usable again,
# and how far off that refusal says its window resets.
WALL_LIFT_SECONDS = 20
WALL_RESET_SECONDS = 600
# How far off the five-hour-wall-far refusal says its window resets. A host leaves a
# five-hour wall only where its reset is further off than the bound its policy prints
# (codex_rotation_five_hour: 900 on aterm main), and codex-alab ends the walled turn only
# beyond that bound plus HANDOFF_SKEW_SECONDS. Two hours clears both and is inside the
# window's five; the wall does not lift during a run.
FAR_RESET_SECONDS = 2 * 3600
HANDOFF_SKEW_SECONDS = 60
# How long after that refusal the host has to have been asked to move the session. A turn
# that ends at its refusal asks at once (0.32 to 0.45 s in the runs of 2026-11-13). One
# that keeps its request reads its credits again thirty seconds later and only then may
# ask (alab-quota-recovery's poll), so a first asking beyond this bound, which is short of
# that poll, is a session that waited in its turn, whether or not it was moved later.
FAR_YIELD_SECONDS = 20
# The cases of the far five-hour wall, each with whether its 429 names the window's reset.
# codex-alab offers the wall on its usage read alone and keeps that read's reset for the
# ended turn, so one whose 429 names none is moved as the other is.
FAR_WALL_CASES = {"five-hour-wall-far": True, "five-hour-wall-far-unnamed": False}
# The record each of these cases' own oracle leaves in its report. No run of such a case
# is marked passed without it (require_case_proof), and renewal_matrix.py counts none.
CASE_PROOFS = {"five-hour-wall-soon": "fiveHourWallWaitedOut",
               "five-hour-wall-far": "fiveHourWallHandedOver",
               # The far wall's oracle, which for this case also requires the refusal
               # to have named no reset (validate_far_wall_trigger's `names_reset`).
               "five-hour-wall-far-unnamed": "fiveHourWallHandedOver",
               "lost-login-expired": "lostLoginMoved",
               # Its own record, so that nothing which looks for a move in the layout every
               # other case uses is satisfied by a move over the other one.
               "lost-login-expired-plain-issuer": "lostLoginMovedOverPlainIssuer",
               "lost-login-invalidated": "lostLoginStayed",
               "lost-login-reused": "lostLoginStayed",
               "lost-login-invalid-grant": "lostLoginStayed",
               "lost-login-unrecognised": "lostLoginStayed",
               "lost-login-no-word": "lostLoginStayed"}
# What the provider answers a refresh of the source login's token, by the word a
# lost-login case names it with: (status, body). `refresh_token_expired` is the one
# refusal a session is moved for (alab/design/accounts-lost-login.md); the 400 is what
# every other case of this file answers a refresh.
REFRESH_REFUSALS = {
    "refresh_token_expired": (401, {"error": {"code": "refresh_token_expired"}}),
    "refresh_token_invalidated": (401, {"error": {"code": "refresh_token_invalidated"}}),
    "refresh_token_reused": (401, {"error": {"code": "refresh_token_reused"}}),
    "invalid_grant": (400, {"error": "invalid_grant"}),
    "account_deactivated": (401, {"error": {"code": "account_deactivated"}}),
}
# Each lost-login case: the refusal its source login's refresh is answered, whether the
# host's fixture policy moves a lost login (`lost_login` in its config, which makes its
# `harness policy` print `codex_rotation_login=1`), and where the logins' issuer, whose
# token endpoint a refresh is posted to, is served. Only a `refresh_token_expired` on a
# host that prints the word is moved; codex-alab vetoes every other refusal and asks a
# host without the word nothing.
#
# The issuer is TLS_ISSUER, the fixture's one TLS listener, as in every other case of this
# file, in every case but one. `lost-login-expired` is the specified case in that layout,
# and the host recorded from aterm 7f1450f9dda cannot pass it: a host moves a lost login
# only after it has presented the refused token itself, and aterm's fixture reader trusts
# the run's test CA for the usage read alone. Its refresh request verifies against the
# platform's trust store (aterm codex_app_login.rs, `read_identified_login_with_fixture_roots`
# and `http_client`), so it never reaches a token endpoint under the fixture's certificate.
# That is a gap in aterm's fixture, and the case stands as written until it is closed.
#
# `lost-login-expired-plain-issuer` is the one departure, and is named for it. It serves
# the issuer on PLAIN_ISSUER, a second loopback listener without TLS, which both aterm
# (`refresh_endpoint`) and codex-alab accept as a login's recorded issuer, so that the
# host's refresh arrives and the move can be driven at all. A pass of it is evidence of
# the move with the host's refresh over plain HTTP, and of nothing in the layout the other
# cases share: it leaves its own record (CASE_PROOFS) and no pass of it is a pass of
# `lost-login-expired`. The cases that move nothing need no such listener, because in
# them the host is never asked and presents nothing.
PLAIN_ISSUER, TLS_ISSUER = "loopback-http", "fixture-tls"
LOST_LOGIN_CASES = {
    "lost-login-expired": ("refresh_token_expired", True, TLS_ISSUER),
    "lost-login-expired-plain-issuer": ("refresh_token_expired", True, PLAIN_ISSUER),
    "lost-login-invalidated": ("refresh_token_invalidated", True, TLS_ISSUER),
    "lost-login-reused": ("refresh_token_reused", True, TLS_ISSUER),
    "lost-login-invalid-grant": ("invalid_grant", True, TLS_ISSUER),
    "lost-login-unrecognised": ("account_deactivated", True, TLS_ISSUER),
    "lost-login-no-word": ("refresh_token_expired", False, TLS_ISSUER),
}
# The lost-login cases whose session must be moved.
LOST_LOGIN_MOVED = ("lost-login-expired", "lost-login-expired-plain-issuer")
# The word codex-alab keeps its own refusal of a refresh token under, by the provider's
# word for it (alab-autopilot login_refusal.rs); None for the one refusal it never keeps.
VETO_WORDS = {"refresh_token_expired": None, "refresh_token_invalidated": "revoked",
              "refresh_token_reused": "exhausted", "invalid_grant": "other",
              "account_deactivated": "other"}
# How long the host's own refusal of the source's refresh token has to have stood before
# it answers a lost login's ask `ready`: it presents the refused token itself and moves
# nothing for a minute after (aterm's codex_app_login ROTATION_HANG). Measured on the
# provider's clock, from the host's token request to the host's `ready`, and held to the
# whole minute the specification names. aterm dates its refusal in whole Unix seconds
# (codex_app_login_refusal.rs `settled`), so a host could by its own arithmetic answer up
# to a second sooner; such a run fails here by name with what it measured, and whether
# the minute or the arithmetic gives way is then decided in the open.
LOGIN_SETTLE_SECONDS = 60
# How often codex-alab asks the host about a move to another account: one
# `codex-login-rotation prepare` a pass and one pass every five seconds (alab/CONTRACT.md,
# "A sealed claim"; alab-autopilot renewal_follow.rs `point_recheck`). Two asks nearer
# than the period less LOGIN_ASK_EARLY_SECONDS are more than one ask a pass, and two
# further apart than twice the period are a pass that was missed.
LOGIN_ASK_PERIOD_SECONDS = 5
LOGIN_ASK_EARLY_SECONDS = 0.5
# How long a moved session is watched after the controller saw its move complete, before
# the run says nothing more was presented, read or sent: as long as a wall that stays is
# watched, so the host's minute and codex-alab's next look a minute on are both inside it.
LOGIN_MOVED_WATCH_SECONDS = 70
# How long a login wall that is not moved is watched after it took its own path, before
# the run says nothing was asked: longer than the 60 s after which codex-alab looks at an
# unbound login point again (alab-autopilot renewal_follow.rs RECHECK), so a second pass
# is inside it.
LOGIN_WALL_HOLD_SECONDS = 70


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


def refresh_generation(payload):
    """Which fixture login's refresh token a token request presents: its generation, or
    None. A fixture login's refresh token is `fixture-refresh-<generation>` (fake_auth);
    the request's body is JSON from codex and from aterm alike. Nothing of it is kept."""
    try:
        body = json.loads(payload)
    except ValueError:
        return None
    token = body.get("refresh_token") if isinstance(body, dict) else None
    if not isinstance(token, str) or not token.startswith("fixture-refresh-"):
        return None
    generation = token[len("fixture-refresh-"):]
    return generation if generation in QUOTA_ACCOUNTS else None


def request_client(headers):
    """Which program sent a request: aterm's usage reader names itself exactly
    `codex-cli`; codex-alab's user agent begins with its originator and version."""
    return "host" if headers.get("user-agent") == "codex-cli" else "codex"


# codex's own User-Agent: `<originator>/<version> (<os> <version>; <arch>) <terminal>`
# (codex-rs login default_client.rs `get_codex_user_agent`).
CODEX_AGENT = re.compile(r"[A-Za-z0-9_.-]+/\S+ \([^()]+\)(?: .*)?")


def request_agent(headers):
    """Which program a request's User-Agent names, held to each program's own form.

    "host" for aterm's exact `codex-cli`, "codex" for codex's own form, "other" for
    anything else. `request_client` calls every request that is not the host's codex's;
    a lost-login case counts who presented a refresh token, and there a request from
    neither program must not be counted as either.
    """
    agent = headers.get("user-agent") or ""
    if agent == "codex-cli":
        return "host"
    return "codex" if CODEX_AGENT.fullmatch(agent) else "other"


def agent_seen(headers):
    """The start of a User-Agent neither program sends, for the run's record."""
    return re.sub(r"[^A-Za-z0-9_./ ();-]", "_", headers.get("user-agent") or "")[:80]


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


FAR_NOT_MOVED = "a five-hour wall whose reset is far off was not handed to another account: "
# How much of the host's `harness policy` reply a report keeps. The reply is one line of
# policy words and prompts and holds no credential; a refusal quotes its rotation words.
POLICY_REPLY_BYTES = 4096


def rotation_calls(trace, kind="rotation"):
    """The host's traced `codex-rotation` calls in order, as (verb, reply, stamp), or
    with `kind` "login-rotation" its `codex-login-rotation` calls.

    Empty while there is no trace; a line still being written is passed over.
    """
    calls = []
    try:
        lines = trace.read_text().splitlines()
    except OSError:
        return calls
    for line in lines:
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if isinstance(event, dict) and event.get("kind") == kind and type(event.get("monotonicUs")) is int:
            calls.append((event.get("verb"), str(event.get("reply")), event["monotonicUs"]))
    return calls


def five_hour_policy(reply):
    """What the host's `harness policy` reply says of leaving a five-hour wall.

    A host hands a five-hour wall to another account only while its policy prints
    `codex_rotation=1 codex_rotation_five_hour=<seconds>`; codex-alab ends a walled turn
    for it only then, and only beyond that bound plus HANDOFF_SKEW_SECONDS. A host that
    prints no bound cannot pass `five-hour-wall-far`, and the run says so at once, with
    the reply: rotation switched off, a malformed bound and an aterm that predates the
    word are different causes, and only the reply tells them apart.
    """
    words = dict(word.split("=", 1) for word in reply.split() if "=" in word)
    rotation = [word for word in reply.split() if word.startswith("codex_rotation")]
    said = ("; of rotation its reply says " + (" ".join(rotation)[:240] if rotation else "nothing")
        + ", and begins: " + reply[:120])
    probe.require(reply.startswith("OK ") and "gen" in words,
        "the host's harness policy was not answered to its owner" + said)
    bound = words.get("codex_rotation_five_hour")
    probe.require(words.get("codex_rotation") == "1" and isinstance(bound, str)
        and re.fullmatch(r"0|[1-9][0-9]{0,8}", bound) is not None,
        "the host's policy prints no five-hour bound (codex_rotation_five_hour), so it "
        "hands no five-hour wall to another account" + said)
    probe.require(int(bound) + HANDOFF_SKEW_SECONDS < FAR_RESET_SECONDS,
        f"the host's five-hour bound ({bound} s) is not short of the fixture wall's reset" + said)
    return {"codexRotation": True, "fiveHourBoundSeconds": int(bound),
            "effective": words.get("effective"), "limitWait": words.get("limit_wait")}


LOGIN_NOT_MOVED = "a lost login whose refresh token simply ended was not moved to another account: "
LOGIN_NOT_HELD = "a lost login that must stay a stop for a person was moved or asked about: "
# How long after the provider refused the source's refresh codex-alab has to have asked the
# host to move the session. It asks on the pass that follows that refusal (0.02 to 0.04 s
# after it in the runs of 2026-11-13), so a run without an ask by then is failed by name,
# with what was seen, long before the hang detector.
LOGIN_ASK_SECONDS = 30


def reply_state(reply):
    """The `state=` word of a traced control reply, or None for a refusal."""
    if not reply.startswith("OK "):
        return None
    return reply_fields(reply).get("state")


def reply_fields(reply):
    """The `key=value` words of a traced control reply."""
    return dict(word.split("=", 1) for word in reply.split() if "=" in word)


# How long the loop waits, once a usage-wall successor is at rest on its consumed receipt,
# for the host to answer that successor's report (`codex-rotation complete`) completed
# before it stops the host. codex-alab sends the report once its resumed turn was served
# and asks again while the host answers `waiting`, for 5 s at most (alab-autopilot
# renewal_completion.rs, COMPLETION_BUDGET). The successor is at rest before its report is
# traced as often as not, so a loop that stopped the host there cut the report off.
COMPLETION_WAIT_SECONDS = 10
# The host's answer to a report it cannot record yet (a busy receipt writer, sign-in turn
# or auth lock: aterm codex_rotation_control.rs `complete`). It names no receipt.
WAITING_REPLY = "OK codex-rotation state=waiting"


def completion_seen(calls, receipt_id):
    """When the host answered the report of receipt `receipt_id`'s successor completed, or
    None. `calls` are one host's traced `codex-rotation` calls (rotation_calls)."""
    return next((stamp for verb, reply, stamp in calls if verb == "complete"
                 and reply_state(reply) == "completed" and reply_fields(reply).get("id") == receipt_id), None)


def completion_verdict(calls, receipt, state=None, ledger=None):
    """The successor's report that its resumed turn was served, and the pool it proved.

    `calls` are the traced `codex-rotation` calls of the host of the session `receipt`
    moved, in order. The successor consumes its receipt, resumes the walled turn on the
    destination and, once that turn was served, reports it (`complete`). The host answers
    `completed`, naming the receipt, its thread and the walled turn, and proves the
    destination for the pool, which releases every session walled behind this one at its
    next ask. Before that it may answer `waiting`, and nothing else; the report follows
    the receipt's last consume, a dropped consume's retry included, and is answered
    completed once. With `state` and `ledger` (the pool's state.json and its ledger.jsonl
    rows), the pool ends proven on its active account with no canary held, proven once,
    and no session took the canary.
    """
    consumes = [stamp for verb, reply, stamp in calls
                if verb == "consume" and reply_fields(reply).get("id") == receipt["id"]]
    probe.require(bool(consumes), "the successor never consumed its receipt")
    reports = [(reply, stamp) for verb, reply, stamp in calls if verb == "complete"]
    probe.require(bool(reports), "no `codex-rotation complete` followed the successor's consume: "
                  "the report that its resumed turn was served never reached the host")
    completed = [(reply, stamp) for reply, stamp in reports if reply_state(reply) == "completed"]
    probe.require(len(completed) == 1, f"the host answered the successor's report completed "
        f"{len(completed)} times, not once: " + ", ".join(sorted({reply[:60] for reply, _ in reports})))
    reply, stamp = completed[0]
    fields = reply_fields(reply)
    probe.require(fields.get("id") == receipt["id"] and fields.get("thread") == receipt["intent"]["thread"]
        and fields.get("turn") == receipt["intent"]["wall"],
        "the completed report names another receipt, thread or turn than the consumed receipt's")
    probe.require(stamp > consumes[-1], "the report was answered completed before the receipt's last consume")
    before = [(answer, at) for answer, at in reports if at < stamp]
    probe.require(all(answer == WAITING_REPLY and at > consumes[0] for answer, at in before),
        "before it was answered completed the report was answered otherwise than waiting, or sent "
        "before the consume: " + ", ".join(sorted({answer[:60] for answer, _ in before})))
    probe.require(not any(at > stamp for _, at in reports),
        "the successor reported again after its report was answered completed")
    proof = {"completedUs": stamp, "secondsAfterLastConsume": round((stamp - consumes[-1]) / 1_000_000, 3),
             "waitingBefore": len(before), "consumes": len(consumes)}
    probe.require((state is None) == (ledger is None), "the pool is judged by its state and its ledger together")
    if state is not None:
        probe.require(state.get("proven") is True and state.get("canary") is None
            and isinstance(state.get("active"), str),
            "the pool did not end proven on its active account with no canary held")
        canaries = [event for event in ledger if event.get("kind") == "rotate"
                    and str(event.get("why") or "").startswith("canary:")]
        probe.require(not canaries, "a session took the canary: " + str(canaries[0].get("why") if canaries else ""))
        proven = [event for event in ledger if event.get("kind") == "proven"
                  and event.get("account") == state["active"]]
        probe.require(len(proven) == 1, f"the pool's active account was proven {len(proven)} times, not once")
        proof["provenAccount"] = state["active"]
    return proof


def pool_ledger(pool):
    """The rows of a pool's ledger.jsonl, in order."""
    return [json.loads(line) for line in (pool / "ledger.jsonl").read_text().splitlines() if line.strip()]


def login_policy(reply):
    """What the host's `harness policy` reply says of moving a lost login.

    codex-alab asks `codex-login-rotation` only of a host whose policy prints
    `codex_rotation_login=1` (alab/CONTRACT.md); the other words kept are the ones a
    login wall's own path is decided by. Only the reply tells a host that does not move
    lost logins from one that was never asked.
    """
    words = dict(word.split("=", 1) for word in reply.split() if "=" in word)
    probe.require(reply.startswith("OK ") and "gen" in words,
        "the host's harness policy was not answered to its owner; its reply begins: " + reply[:120])
    return {"codexRotationLogin": words.get("codex_rotation_login") == "1",
            "codexRotation": words.get("codex_rotation") == "1",
            "logins": words.get("logins"), "accounts": words.get("accounts"),
            "effective": words.get("effective")}


def login_events(events):
    """A lost-login run's provider record, sorted for its oracles.

    `events` are the fixture server's requests in arrival order. `lost` is the first
    model request of the source the provider refused with 401 (its index and event), or
    None; `refreshes` every token request, as (index, event); `usage` every usage read.
    """
    def path(event):
        return event["path"].split("?", 1)[0]
    lost = next(((index, event) for index, event in enumerate(events) if event["method"] == "POST"
        and path(event).endswith("/responses") and event.get("account") == "source"
        and event.get("status") == 401), None)
    refreshes = [(index, event) for index, event in enumerate(events)
        if event["method"] == "POST" and path(event).endswith("/oauth/token")]
    usage = [(index, event) for index, event in enumerate(events)
        if event["method"] == "GET" and path(event).endswith("/usage")]
    return lost, refreshes, usage


def refused_handshakes(events):
    """The TLS handshakes a client would not complete with the fixture, in order.

    A program that does not trust the run's CA sends no request the provider could count.
    The host's own refresh is such a program's on aterm 7f1450f9dda, whose fixture reader
    trusts that CA for its usage read alone.
    """
    return [event for event in events if event["method"] == "TLS"]


def presenter(event):
    """Which program presented a token request's refresh token: "host", "codex", or
    "other" for a User-Agent that is neither program's own (request_agent)."""
    return event.get("agent", "other")


def login_evidence(events, trace):
    """What followed the loss of the source's login, for a run that reached no outcome."""
    lost, refreshes, usage = login_events(events)
    if lost is None:
        return "no model request of the source was refused with 401"
    hits = {}
    for _, event in refreshes:
        key = f"{presenter(event)} of {event.get('refreshOf')} ({event.get('status')})"
        hits[key] = hits.get(key, 0) + 1
    reads = {}
    for index, event in usage:
        if index > lost[0]:
            key = f"{event.get('client')} of {event.get('account')} ({event.get('status')})"
            reads[key] = reads.get(key, 0) + 1
    calls = [call for call in rotation_calls(trace, "login-rotation") if call[2] > lost[1]["monotonicUs"]]
    replies = {}
    for verb, reply, _ in calls:
        key = f"{verb}: {reply[:60]}"
        replies[key] = replies.get(key, 0) + 1
    renewals = {}
    for verb, outcome, stamp in renewal_calls(trace):
        if stamp > lost[1]["monotonicUs"]:
            renewals[f"{verb}: {outcome[:40]}"] = renewals.get(f"{verb}: {outcome[:40]}", 0) + 1
    listed = lambda counts: ", ".join(f"{count} x {key}" for key, count in sorted(counts.items())) or "none"
    said = ("after the first 401: token requests " + listed(hits) + "; usage reads " + listed(reads)
        + "; codex-login-rotation calls " + listed(replies) + "; codex-renewal calls " + listed(renewals))
    refused = refused_handshakes(events)
    if refused:
        # Said only where one exists: a client that does not trust the fixture's CA.
        after = (refused[0]["monotonicUs"] - lost[1]["monotonicUs"]) / 1_000_000
        said += (f"; TLS handshakes a client would not complete with the fixture: {len(refused)}, the "
            f"first {after:.3f} s after the first 401 ({refused[0].get('reason')})")
    return said


def lost_login_failure(root, events, policy, now_us=None):
    """Why a `lost-login-expired` run can no longer pass, in words; None while it can.

    The host's policy prints no word for the move, the host answered that no move is
    coming, or codex-alab did not ask within LOGIN_ASK_SECONDS of the refusal of its
    refresh (`now_us` is the host's clock now, or None once the run has ended).
    """
    trace = root / "renewal-control-trace.jsonl"
    lost, refreshes, _ = login_events(events)
    if lost is None:
        return None
    if isinstance(policy, dict) and policy.get("codexRotationLogin") is False:
        return "the host's policy prints no codex_rotation_login=1, so codex-alab asks it nothing"
    calls = [call for call in rotation_calls(trace, "login-rotation") if call[2] > lost[1]["monotonicUs"]]
    if any(verb == "prepare" and reply_state(reply) == "not-needed" for verb, reply, _ in calls):
        return ("the host answered codex-login-rotation not-needed: it moves no such login; "
            + login_evidence(events, trace))
    refused = next((event for _, event in refreshes if event.get("refreshOf") == "source"
        and presenter(event) == "codex"), None)
    asked = next((stamp for verb, _, stamp in calls if verb == "prepare"), None)
    since = (refused or lost[1])["monotonicUs"]
    if asked is None and now_us is not None and now_us > since + LOGIN_ASK_SECONDS * 1_000_000:
        return (f"codex-alab did not ask the host to move it within {LOGIN_ASK_SECONDS} s of the "
            "refusal; " + login_evidence(events, trace))
    return None


def login_record_fault(events):
    """Why a lost-login run's provider record cannot be judged, in words; None while it can.

    A request the fixture has no route for, on either listener, or a token request or a
    usage read from a program that is neither codex-alab nor the host by its User-Agent
    (request_agent). Either is something the cases' counts would miss or would give to
    the wrong program, so the loop fails the run by name the moment one exists.
    """
    def path(event):
        return event["path"].split("?", 1)[0]
    unrouted = [event for event in events if event.get("unrouted")]
    if unrouted:
        return "a request the fixture has no route for was sent: " + ", ".join(
            f"{event.get('agent')} {event['method']} {path(event)[:60]}"
            + (" on the issuer's listener" if event.get("listener") == "issuer" else "") for event in unrouted)
    _, refreshes, usage = login_events(events)
    strangers = [event for _, event in refreshes + usage if event.get("agent") not in ("host", "codex")]
    if strangers:
        return ("a token request or a usage read came from a program that is neither codex-alab nor the "
            "host by its User-Agent: " + ", ".join(sorted({
                f"{event['method']} {path(event).rsplit('/', 1)[-1]} from {event.get('agentSeen')!r}"
                for event in strangers})))
    return None


def validate_login_loss(events, word, issuer):
    """The provider's side of every lost-login case: how the source's login was lost.

    The source was answered its first model request and refused every later one, its
    title's too, with 401; codex presented the source's refresh token once at most, and
    it was refused with the case's `word`; no login but the source was ever refused, and
    the other login's refresh token was never presented. Every token request arrived at
    the case's `issuer` and came from codex-alab or from the host by its own User-Agent
    (request_agent), as did every usage read: a request from neither is counted as
    neither. No request was sent that the fixture has no route for, and no client refused
    the fixture's TLS handshake: either is a request this record could not count.
    """
    def path(event):
        return event["path"].split("?", 1)[0]
    lost, refreshes, usage = login_events(events)
    probe.require(lost is not None, "no model request of the source was refused with 401")
    fault = login_record_fault(events)
    probe.require(fault is None, str(fault))
    refused_tls = refused_handshakes(events)
    probe.require(not refused_tls, f"a client would not complete the fixture's TLS handshake, "
        f"{len(refused_tls)} time(s), the first with {refused_tls[0].get('reason') if refused_tls else None}: "
        "a program that does not trust the run's CA tried to reach the provider, and what it would "
        "have sent is not in this record")
    turns = [event for event in events if event["method"] == "POST" and event.get("kind") == "foreground"
             and event.get("account") == "source"]
    probe.require(bool(turns) and turns[0].get("status") == 200
        and all(event.get("status") == 401 for event in turns[1:]) and len(turns) >= 2,
        "the source was not answered once and refused with 401 from then on: "
        + ", ".join(str(event.get("status")) for event in turns))
    # A request that arrived while that first refusal was being served may have been
    # answered: the refusal's event says how many requests the provider had by then.
    since = lost[1].get("requestsAtLoss")
    probe.require(type(since) is int and since > lost[0], "the first refusal does not say when it was served")
    later = [event for event in events[since:] if event.get("account") == "source"
             and (event["method"] == "GET" or path(event).endswith("/responses"))]
    probe.require(all(event.get("status") == 401 for event in later),
        "the provider answered the source's access token after it had refused it")
    elsewhere = [event for _, event in refreshes if (event.get("listener") == "issuer") != (issuer == PLAIN_ISSUER)]
    probe.require(issuer in (PLAIN_ISSUER, TLS_ISSUER) and not elsewhere,
        f"a token request did not arrive at this case's issuer ({issuer}): {len(elsewhere)} elsewhere")
    expected = REFRESH_REFUSALS[word][0]
    source = [event for _, event in refreshes if event.get("refreshOf") == "source"]
    probe.require(all(event.get("status") == expected for event in source),
        f"a refresh of the source's token was not answered the case's refusal ({expected} {word})")
    others = [event for _, event in refreshes if event.get("refreshOf") != "source"]
    probe.require(not others, "a refresh token other than the source's was presented: "
        + ", ".join(f"{presenter(event)} of {event.get('refreshOf')}" for event in others))
    by_codex = [event for event in source if presenter(event) == "codex"]
    probe.require(len(by_codex) <= 1,
        f"codex-alab presented the source's refused refresh token {len(by_codex)} times, not once at most")
    refused = [event for event in events if event.get("status") == 401 and event.get("account") != "source"
               and not path(event).endswith("/oauth/token")]
    probe.require(not refused, "a login other than the source was refused")
    return {"lostUs": lost[1]["monotonicUs"], "refusal": word, "refusalStatus": expected,
        "issuer": issuer, "tokenRequests": len(refreshes),
        "sourceModelRequestsRefused": len(turns) - 1,
        "sourceRefreshPresentedByCodex": len(by_codex),
        "codexRefreshUs": by_codex[0]["monotonicUs"] if by_codex else None,
        "sourceRefreshPresentedByHost": sum(presenter(event) == "host" for event in source),
        "sourceUsageReadsRefused": sum(event.get("account") == "source" and event.get("status") == 401
            for index, event in usage if index > lost[0]),
        "unroutedRequests": 0, "tlsHandshakesRefused": 0}


def login_ask_cadence(asks, lost_us):
    """How often codex-alab asked the host to move it, held to one ask a pass and a pass
    every LOGIN_ASK_PERIOD_SECONDS; its record.

    `asks` are the host's stamps of the `codex-login-rotation prepare` lines it traced
    after the loss, up to the one it answered `ready`, in order. The session "sends
    exactly one `codex-login-rotation prepare` line" and "may send that line again on
    rechecks" (the plan's smoke, case 1), and its follow loop makes "one pass every 5 s
    for a move to another account" (alab/CONTRACT.md). So no two asks are nearer than
    that period less LOGIN_ASK_EARLY_SECONDS, which a second ask on one pass, an ask on
    every wake and a flood all are, and none are further apart than twice the period,
    which a pass that was skipped is. The host answering every ask alike, and opening
    one job for them all, is not this proof's: it is the verdict's count of the host's
    one presentation, one reading and one receipt.
    """
    gaps = [(later - earlier) / 1_000_000 for earlier, later in zip(asks, asks[1:])]
    after = lambda index: (asks[index] - lost_us) / 1_000_000
    early = [index for index, gap in enumerate(gaps) if gap < LOGIN_ASK_PERIOD_SECONDS - LOGIN_ASK_EARLY_SECONDS]
    probe.require(not early, (f"codex-alab asked the host again {gaps[early[0]]:.3f} s after an ask, "
        f"{after(early[0] + 1):.3f} s after the loss ({len(early)} such pair(s) among {len(asks)} asks): "
        f"that is more than one ask a pass, a pass every {LOGIN_ASK_PERIOD_SECONDS} s") if early else "")
    late = [index for index, gap in enumerate(gaps) if gap > 2 * LOGIN_ASK_PERIOD_SECONDS]
    probe.require(not late, (f"codex-alab left {gaps[late[0]]:.1f} s between two asks, "
        f"{after(late[0]):.1f} s after the loss: that is not a pass every {LOGIN_ASK_PERIOD_SECONDS} s")
        if late else "")
    return {"askPeriodSeconds": LOGIN_ASK_PERIOD_SECONDS, "onePrepareAPass": True,
            "smallestAskGapSeconds": round(min(gaps), 3) if gaps else None,
            "largestAskGapSeconds": round(max(gaps), 3) if gaps else None}


def host_pool_unmoved(root):
    """The host's pool after a lost-login case: nothing of a usage rotation is in it.

    The fixture's pool starts with `main`, the source's row, active. A lost login's move
    is the session's alone (alab/design/accounts-lost-login.md, step 4): `active` stays
    `main`, no rotation is charged, no canary is set and no rotation is bound to a native
    operation. A case that moves nothing leaves the same.
    """
    pool = root / "host-state" / "accounts-codex"
    state = json.loads((pool / "state.json").read_text())
    probe.require(state.get("active") == "main", "the host's active account changed: "
        + str(state.get("active")))
    probe.require(not state.get("rotations"), "the host charged a usage rotation")
    probe.require(not state.get("canary"), "the host set a rotation canary")
    try:
        bindings = json.loads((pool / "logins.json").read_text())
    except FileNotFoundError:
        bindings = {}
    probe.require(not bindings.get("native_rotations"), "the host bound a rotation to a native operation")
    return {"hostActive": state["active"], "hostRotations": 0, "hostCanary": None,
            "hostPinned": bool(state.get("pinned")),
            "hostSourceAuth": ((state.get("accounts") or {}).get("main") or {}).get("auth"),
            "hostTargetAuth": ((state.get("accounts") or {}).get("target") or {}).get("auth")}


def login_files_proof(old_auth, new_auth, logins):
    """Neither login file changed, and the source's still holds the token that was refused.

    `logins` is each login file's sha256 as the controller wrote it. Nothing signs anyone
    in and no token is copied: the move only names the destination's own file.
    """
    probe.require(set(logins) == {str(old_auth), str(new_auth)},
        "the two login files of this case were not both held to the bytes written")
    changed = sorted(Path(path).parent.name for path, digest in logins.items()
        if hashlib.sha256(Path(path).read_bytes()).hexdigest() != digest)
    probe.require(not changed, "a fixture login file changed during the run: " + ", ".join(changed))
    held = {name: json.loads(Path(path).read_text())["tokens"]["refresh_token"]
            for name, path in (("old", old_auth), ("new", new_auth))}
    probe.require(held == {"old": "fixture-refresh-old", "new": "fixture-refresh-new"},
        "a fixture login file no longer holds the refresh token it was written with")
    return {"loginFilesUnchanged": True, "sourceStillHoldsRefusedToken": True}


def lost_login_verdict(root, events, requests, receipt, policy, old_auth, new_auth, logins, rollout,
                       intent, issuer, watched_us, held=None):
    """The oracle of a lost-login run that must move, beyond the handoff the loop held, and its record.

    The run's loop has already held the handoff to what every native handoff must show
    (the receipt's phases and process bindings, the exact native operation, one signal,
    the successor on the other login file answering the original turn). This adds what
    is the lost login's own. The source's login was lost as the case says
    (validate_login_loss, with the logins' `issuer` where the case serves it) and the
    host's policy printed its word. After the refusal codex-alab asked
    `codex-login-rotation prepare`; the host answered `waiting`, and `ready` for this
    receipt no sooner than its own refusal of the token had stood LOGIN_SETTLE_SECONDS.
    The source's refresh token was presented once by codex-alab at most and exactly once
    by the host, and never after `ready`, for the LOGIN_MOVED_WATCH_SECONDS the run went
    on watching to `watched_us` (the host's clock when the loop stopped looking). The
    host read the destination's usage exactly once in the whole run, as usable, before
    `ready`; nobody else read it before `ready`, and its refresh token was never
    presented. The receipt is a login rotation's to another account, one receipt in all,
    and the successor sent the recovered turn once. The host's pool was not rotated
    (host_pool_unmoved), no `codex-rotation` or `codex-renewal` was asked, and neither
    login file changed (login_files_proof). The prompt was typed once and nothing was
    typed for the move: the recovered turn's request is the refused one's, and `rollout`
    is the thread's own record read after the watch. `intent` is what the session's
    record held for the wall when the loop first saw it: the lost login's own rule,
    `login-rotate@v1`, under the record's `rotation` field (alab/CONTRACT.md), with the
    text a successor that typed would type. Last, how often codex-alab asked
    (login_ask_cadence).

    Two readings of the plan's smoke, case 1, are narrower than its words. "Target
    /usage: exactly 1 hit" is held for the host, over the whole run: the successor reads
    the usage of the login it now runs on, as any session does, and those readings are
    counted in the record (`destinationUsageReadsBySuccessor`) and fail nothing after
    `ready`. "Waiting for at least 60 s" is measured from the host's own refusal, which
    follows the first ask, so the answer was `waiting` for longer than that.

    `held`, where given, receives the record as it stood when every proof but the last
    had held, so that a run failed for how often it asked still says what else it showed.
    """
    trace = root / "renewal-control-trace.jsonl"
    failure = lost_login_failure(root, events, policy)
    probe.require(failure is None, LOGIN_NOT_MOVED + str(failure))
    probe.require(isinstance(policy, dict) and policy.get("codexRotationLogin") is True,
        "the host's word for the move was never read: " + str((policy or {}).get("unread")))
    proof = validate_login_loss(events, "refresh_token_expired", issuer)
    lost, refreshes, usage = login_events(events)
    calls = [call for call in rotation_calls(trace, "login-rotation") if call[2] > proof["lostUs"]]
    prepares = [(reply, stamp) for verb, reply, stamp in calls if verb == "prepare"]
    probe.require(bool(prepares), "the host was never asked to move the lost login")
    probe.require(not any(call[2] <= proof["lostUs"] for call in rotation_calls(trace, "login-rotation")),
        "the host was asked to move the session before its login was lost")
    ready = next((stamp for reply, stamp in prepares
        if reply_state(reply) == "ready" and f" id={receipt['id']} " in reply + " "), None)
    probe.require(ready is not None, "the host never answered ready for this receipt")
    before = [reply for reply, stamp in prepares if stamp < ready]
    probe.require(bool(before) and all(reply_state(reply) == "waiting" for reply in before),
        "the host did not answer waiting, and only waiting, until it answered ready: "
        + ", ".join(sorted({reply[:80] for reply in before})))
    by_host = [event for _, event in refreshes if event.get("refreshOf") == "source"
               and presenter(event) == "host"]
    probe.require(len(by_host) == 1,
        f"the host presented the source's refresh token {len(by_host)} times, not exactly once")
    settled = (ready - by_host[0]["monotonicUs"]) / 1_000_000
    probe.require(settled >= LOGIN_SETTLE_SECONDS,
        f"the host answered ready {settled:.1f} s after its own refusal, before it had settled")
    watched = (watched_us - ready) / 1_000_000
    probe.require(watched >= LOGIN_MOVED_WATCH_SECONDS,
        f"the moved session was watched {watched:.1f} s after ready, not {LOGIN_MOVED_WATCH_SECONDS} s")
    probe.require(not any(event["monotonicUs"] > ready for _, event in refreshes),
        "a refresh token was presented after the host answered ready")
    target = [(index, event) for index, event in usage if event.get("account") == "target"]
    by_reader = [event for _, event in target if event.get("client") == "host"]
    probe.require(len(by_reader) == 1 and by_reader[0].get("status") == 200
        and by_reader[0].get("allowed") is True and by_reader[0]["monotonicUs"] < ready,
        f"the host did not read the destination's usage exactly once, as usable, before ready: "
        f"{len(by_reader)} read(s)")
    probe.require(all(event["monotonicUs"] > ready for _, event in target if event.get("client") != "host"),
        "the destination's usage was read by someone other than the host before the move")
    probe.require(by_reader[0]["monotonicUs"] > by_host[0]["monotonicUs"],
        "the host read the destination before its own refusal of the source")
    recovered = [event for event in events if event["method"] == "POST" and event.get("kind") == "foreground"
                 and event.get("account") == "target"]
    probe.require(len(recovered) == 1 and recovered[0].get("status") == 200
        and recovered[0]["monotonicUs"] > ready,
        f"the successor sent the recovered turn {len(recovered)} times, not once, answered, after ready")
    # A receipt names each account by a digest, never by its id.
    purpose = (receipt.get("intent") or {}).get("purpose")
    source = ((receipt.get("intent") or {}).get("identity") or {}).get("account")
    destination = (receipt.get("destination") or {}).get("account")
    digest = lambda value: isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None
    probe.require(purpose == "login_rotation" and digest(source) and digest(destination)
        and source != destination and receipt.get("old_auth") == str(old_auth)
        and receipt.get("new_auth") == str(new_auth),
        f"the receipt is not a login rotation from the source's login file to another "
        f"account's own: purpose {purpose}, {str(source)[:12]} to {str(destination)[:12]}, "
        f"{Path(str(receipt.get('old_auth'))).parent.name} to {Path(str(receipt.get('new_auth'))).parent.name}")
    probe.require(f" source_account={source} " in next(reply for reply, stamp in prepares if stamp == ready) + " ",
        "the receipt's source is not the account the host answered ready for")
    receipts = sorted(path.stem for path in (root / "host-state" / "accounts-codex" / "renewal-follow").glob("*.json")
        if re.fullmatch(r"[0-9a-f]{32}", path.stem))
    probe.require(receipts == [receipt["id"]], f"the host opened {len(receipts)} receipts, not one")
    others = {kind: len(rotation_calls(trace, kind)) for kind in ("rotation", "renewal")}
    probe.require(not any(others.values()),
        f"a usage rotation or a same-account renewal was asked for: {others}")
    probe.require(intent == {"rule": "login-rotate@v1", "field": "rotation",
                             "text": "Continue the original task."},
        f"the session's record did not hold the lost login's own intent for this wall: {intent}")
    proof["recordedIntentRule"] = intent["rule"]
    probe.require(not kept_vetoes(root), "codex-alab kept a veto of a token that simply ended")
    # One reading with its presentation, and one confirming the sign-out once the refusal
    # had settled, which asks twice at most.
    own_reads = [event for index, event in usage if event.get("account") == "source"
                 and event.get("client") == "host"]
    probe.require(0 < len(own_reads) <= 3 and all(event.get("status") == 401 for event in own_reads),
        f"the host read the source's usage {len(own_reads)} times, not within its two bounded reads")
    proof["sourceUsageReadsByHost"] = len(own_reads)
    proof.update(host_pool_unmoved(root))
    proof.update(login_files_proof(old_auth, new_auth, logins))
    typed = [text for request in requests for text in user_texts(request) if text != probe.PROMPT
             and not text.lstrip().startswith("<")]
    probe.require(not typed, "something was typed beside the original prompt")
    proof.update(thread_proof(rollout))
    asks = [stamp for _, stamp in prepares if stamp <= ready]
    proof.update(
        policyPrintedCodexRotationLogin=True,
        preparesAsked=len(prepares), preparesAnsweredWaiting=len(before),
        preparesAfterReady=len(prepares) - len(asks),
        firstAskedSecondsAfterLoss=round((prepares[0][1] - proof["lostUs"]) / 1_000_000, 3),
        askGapsSeconds=[round((later - earlier) / 1_000_000, 3) for earlier, later in zip(asks, asks[1:])],
        hostRefusalUs=by_host[0]["monotonicUs"], readyUs=ready,
        readySecondsAfterHostRefusal=round(settled, 3),
        readySecondsAfterFirstAsk=round((ready - prepares[0][1]) / 1_000_000, 3),
        watchedSecondsAfterReady=round(watched, 3), refreshPresentedAfterReady=0,
        destinationUsageReadsByHost=1,
        destinationUsageReadsBeforeMove=1, destinationRefreshPresented=0,
        destinationUsageReadsBySuccessor=len(target) - 1,
        recoveredTurnRequests=1,
        receiptPurpose=purpose, receiptDestinationDiffersFromSource=True,
        receiptsOpened=1, successorAuthPath=str(new_auth),
        usageRotationCalls=0, renewalCalls=0, nothingTypedForTheMove=True)
    if held is not None:
        held.update(proof)
    proof.update(login_ask_cadence(asks, proof["lostUs"]))
    return proof


def login_move_watched(seen_us, now_us):
    """Whether a moved session has been watched long enough after its move was seen.

    `seen_us` is the host's clock when the loop saw the handoff complete, which is after
    the host's `ready`, so a run that watched LOGIN_MOVED_WATCH_SECONDS from then watched
    at least as long from `ready`, which is what the verdict measures.
    """
    return now_us >= seen_us + LOGIN_MOVED_WATCH_SECONDS * 1_000_000


def thread_proof(rollout, answered=True):
    """What the thread's own rollout says: where its turn ended, and what was typed.

    `rollout` is the thread's persisted events. Its turn ended once with codex's
    `unauthorized` error (the login wall) and then, where `answered`, once answered, and
    no other time; and the one thing a person or a harness typed into it is the original
    prompt: the model's context (`<environment_context>` and the like) aside, no other
    user message exists.
    """
    ends = [event["payload"] for event in rollout if event.get("type") == "event_msg"
            and event.get("payload", {}).get("type") in ("task_complete", "turn_complete")]
    errors = [(end.get("error") or {}).get("codex_error_info") for end in ends]
    probe.require(errors == (["unauthorized", None] if answered else ["unauthorized"]),
        "the turn did not end once at the login wall" + (" and once answered" if answered else
        " and stay there") + f": {errors}")
    typed = [part.get("text") for event in rollout if event.get("type") == "response_item"
             and event.get("payload", {}).get("type") == "message"
             and event["payload"].get("role") == "user"
             for part in event["payload"].get("content") or [] if isinstance(part, dict)]
    typed = [text for text in typed if not (isinstance(text, str) and text.lstrip().startswith("<"))]
    probe.require(typed == [probe.PROMPT],
        f"the thread holds {len(typed)} typed user messages, not the one prompt")
    return {"turnEndedAtLoginWall": True, "userMessagesInThread": 1}


def thread_rollout(root, thread=None):
    """The persisted events of the run's thread: of `thread`, or of every rollout under
    the run's CODEX_HOME, which holds one thread."""
    events = []
    for rollout in sorted((root / "codex-home" / "sessions").rglob("*.jsonl")):
        if thread is None or thread in rollout.name:
            events.extend(json.loads(line) for line in rollout.read_text().splitlines())
    return events


def user_texts(request):
    """The text of each user message part of a model request's input."""
    return [part.get("text") for item in request["body"].get("input", [])
            if item.get("type") == "message" and item.get("role") == "user"
            for part in item.get("content", []) if isinstance(part, dict)]


def login_move_evidence(root, events, new_auth):
    """What a move of, or an ask about, a lost login leaves under `root`, in words; empty if none.

    A `codex-login-rotation` or `codex-rotation` line the host traced, a reading of the
    other account's usage or a presentation of its refresh token, a receipt, a native
    action, a session process on the other login file, or a model request from it.
    """
    trace = root / "renewal-control-trace.jsonl"
    found = []
    for kind, verb in (("login-rotation", "codex-login-rotation"), ("rotation", "codex-rotation")):
        calls = rotation_calls(trace, kind)
        if calls:
            found.append(f"{len(calls)} {verb} line(s) sent, the first answered {calls[0][1][:60]}")
    _, refreshes, usage = login_events(events)
    reads = [event for _, event in usage if event.get("account") != "source"]
    if reads:
        found.append(f"{len(reads)} usage read(s) of the other account by "
            + ", ".join(sorted({str(event.get("client")) for event in reads})))
    presented = [event for _, event in refreshes if event.get("refreshOf") != "source"]
    if presented:
        found.append(f"{len(presented)} presentation(s) of another login's refresh token")
    pool = root / "host-state" / "accounts-codex"
    receipts = [path.name for path in (pool / "renewal-follow").glob("*") if path.is_file()]
    if receipts:
        found.append(f"{len(receipts)} handoff receipt file(s)")
    actions = [event.get("action") for event in native_actions(root)]
    if actions:
        found.append("native action(s): " + ", ".join(sorted({str(action) for action in actions})))
    for path in (root / "codex-home" / "alab" / "sessions").glob("*.json"):
        try:
            record = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        if isinstance(record, dict) and (record.get("env") or {}).get("CODEX_AUTH_PATH") == str(new_auth):
            found.append(f"session process {path.stem} runs on the other login file")
    others = sorted({str(event.get("account")) for event in events if event["method"] == "POST"
        and event["path"].split("?", 1)[0].endswith("/responses") and event.get("account") != "source"})
    if others:
        found.append("model request(s) from " + ", ".join(others))
    return found


def renewal_calls(trace):
    """The host's traced `codex-renewal` calls in order, as (verb, outcome, stamp): the
    same account's own renewal, which is where a login wall goes on a host whose policy
    says `logins=renew`. Empty while there is no trace."""
    calls = []
    try:
        lines = trace.read_text().splitlines()
    except OSError:
        return calls
    for line in lines:
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if isinstance(event, dict) and event.get("kind") == "renewal" and type(event.get("monotonicUs")) is int:
            calls.append((event.get("verb"), str(event.get("outcome")), event["monotonicUs"]))
    return calls


def kept_vetoes(root):
    """codex-alab's own refusals kept under the run's CODEX_HOME: {file name: word}.

    One file per refused refresh token, named by a digest of it and holding one reason
    word and no token byte (alab-autopilot login_refusal.rs). A refusal other than
    `refresh_token_expired` is kept, and vetoes a move of that login.
    """
    kept = {}
    for path in (root / "codex-home" / "alab" / "login-refusals").glob("*"):
        if re.fullmatch(r"[0-9a-f]{64}", path.name) and path.is_file() and not path.is_symlink():
            kept[path.name] = path.read_text()[:64].strip()
    return kept


def veto_name(refresh_token):
    """The name codex-alab keeps a refresh token's veto under (login_refusal.rs `name`)."""
    return hashlib.sha256(b"alab/login-refusal/v1\0" + refresh_token.encode()).hexdigest()


def own_path(events, trace):
    """The login wall's own path after the source's refresh was refused: the
    `codex-renewal prepare` asks the host traced since, as (outcome, stamp), and the
    stamp of that refusal; (None, None) before the refusal."""
    lost, refreshes, _ = login_events(events)
    refused = next((event for _, event in refreshes if event.get("refreshOf") == "source"
        and presenter(event) == "codex"), None)
    if lost is None or refused is None:
        return None, None
    return [(outcome, stamp) for verb, outcome, stamp in renewal_calls(trace)
            if verb == "prepare" and stamp > refused["monotonicUs"]], refused["monotonicUs"]


def login_wall_stood(root, events, now_us):
    """Whether a lost-login case that moves nothing has reached its end.

    Its end is the login wall on its own path for a whole look and the next: codex-alab
    asked the host for the same account's renewal (`codex-renewal prepare`) on the pass
    after its refresh was refused and again on its next look a minute later, and
    LOGIN_WALL_HOLD_SECONDS have passed since that refusal. Nothing of a move is judged
    here: the loop fails the run by name the moment any exists (login_move_evidence).
    """
    asks, refused = own_path(events, root / "renewal-control-trace.jsonl")
    return (asks is not None and len(asks) >= 2
            and now_us >= refused + LOGIN_WALL_HOLD_SECONDS * 1_000_000)


def login_stayed_verdict(root, events, policy, case, old_auth, new_auth, logins, rollout, held_us):
    """The oracle of a lost-login case in which nothing is moved, and its record.

    `case` is the case's LOST_LOGIN_CASES entry, `held_us` the host's clock when the loop
    stopped looking. The source's login was lost as the case says (validate_login_loss).
    The host's policy printed, or did not print, the word for the move as the case is
    run under, and says `logins=renew`. What stopped the move is on record: codex-alab's
    own veto of the refused token, kept with the word of its reason, for every refusal
    but `refresh_token_expired`; for that one, which it never keeps, the host's missing
    word. Nothing of a move or an ask exists (login_move_evidence): no
    `codex-login-rotation` and no `codex-rotation` line, no reading of the other
    account, no receipt, no native action. The host, asked nothing about a move, did
    nothing with the refused login that a move's job does: it presented the source's
    refresh token never, and read the source's usage no more than once for each time it
    was asked about that login's renewal. (Under the fixture's TLS a presentation by a
    host that does not trust the run's CA would show only as a refused handshake, which
    validate_login_loss fails.) The login wall took its own path instead and stood on
    it: `codex-renewal prepare` on the pass after the refusal and on the next look, each
    answered `waiting`, over LOGIN_WALL_HOLD_SECONDS. The turn ended once, at the login
    wall, and was never answered; the host's pool and both login files are as they were.

    How often `codex-renewal prepare` was asked is recorded and not judged: the plan's
    smoke bounds nothing of the login wall's own path but that it is taken.
    """
    word, host_moves, issuer = case
    trace = root / "renewal-control-trace.jsonl"
    proof = validate_login_loss(events, word, issuer)
    found = login_move_evidence(root, events, new_auth)
    probe.require(not found, LOGIN_NOT_HELD + "; ".join(found))
    probe.require(isinstance(policy, dict) and "codexRotationLogin" in policy,
        "the host's policy was never read: " + str((policy or {}).get("unread")))
    probe.require(policy["codexRotationLogin"] is host_moves,
        "the host's policy " + ("prints no" if host_moves else "prints")
        + " codex_rotation_login=1, which is not what this case is run under")
    probe.require(policy.get("logins") == "renew",
        f"the host's policy says logins={policy.get('logins')}: this oracle knows the login "
        "wall's own path only where the host renews logins")
    expected = VETO_WORDS[word]
    kept = kept_vetoes(root)
    probe.require(kept == ({} if expected is None else {veto_name("fixture-refresh-old"): expected}),
        "codex-alab's kept refusals are not the one this case's refusal leaves ("
        + (f"the source's token, as {expected}" if expected else "none")
        + "): " + (", ".join(f"{name[:12]}: {said}" for name, said in sorted(kept.items())) or "none kept"))
    asks, refused = own_path(events, trace)
    probe.require(asks is not None and len(asks) >= 2 and all(outcome == "waiting" for outcome, _ in asks),
        "the login wall did not take its own path, the same account's renewal, on two looks: "
        + (", ".join(outcome for outcome, _ in asks) if asks else "no codex-renewal prepare"))
    held = (held_us - refused) / 1_000_000
    probe.require(held >= LOGIN_WALL_HOLD_SECONDS,
        f"the login wall was watched {held:.1f} s after the refusal, not {LOGIN_WALL_HOLD_SECONDS} s")
    probe.require(proof["sourceRefreshPresentedByHost"] == 0,
        f"the host presented the source's refused refresh token {proof['sourceRefreshPresentedByHost']} "
        "time(s), though it was asked nothing about a move")
    _, _, usage = login_events(events)
    own_reads = [event for _, event in usage if event.get("account") == "source"
                 and event.get("client") == "host"]
    probe.require(len(own_reads) <= len(asks),
        f"the host read the source's usage {len(own_reads)} times, more than once for each of the "
        f"{len(asks)} times it was asked about that login")
    probe.require(not rotation_calls(trace, "login-rotation") and not rotation_calls(trace, "rotation"),
        LOGIN_NOT_HELD + "the host traced a rotation line")
    proof.update(host_pool_unmoved(root))
    proof.update(login_files_proof(old_auth, new_auth, logins))
    proof.update(thread_proof(rollout, answered=False))
    proof.update(
        policyPrintedCodexRotationLogin=policy["codexRotationLogin"], policyLogins=policy["logins"],
        vetoKept=expected, stoppedBy="codex-alab's veto" if expected else "the host's policy prints no word",
        codexLoginRotationLines=0, codexRotationLines=0, otherAccountUsageReads=0,
        receipts=0, nativeActions=0,
        renewalPreparesAsked=len(asks), sourceUsageReadsByHost=len(own_reads),
        renewalAskGapsSeconds=[round((later[1] - earlier[1]) / 1_000_000, 3)
                               for earlier, later in zip(asks, asks[1:])],
        firstRenewalAskSecondsAfterRefusal=round((asks[0][1] - refused) / 1_000_000, 3),
        lastRenewalAskSecondsAfterRefusal=round((asks[-1][1] - refused) / 1_000_000, 3),
        heldSeconds=round(held, 3))
    return proof


def same_account_evidence(root, old_auth, processes):
    """What a move of the walled session onto its own login leaves under `root`, in words.

    A handoff receipt that names the walled login or its account as the destination, or
    a second session process started on that login. `processes` is the caller's set of
    the session processes seen on that login so far, which this call adds to.
    """
    found = []
    for path in sorted((root / "host-state" / "accounts-codex" / "renewal-follow").glob("*.json")):
        if not re.fullmatch(r"[0-9a-f]{32}", path.stem):
            continue
        try:
            receipt = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        if not isinstance(receipt, dict):
            continue
        source = ((receipt.get("intent") or {}).get("identity") or {}).get("account")
        destination = (receipt.get("destination") or {}).get("account")
        new_auth = receipt.get("new_auth")
        if ((source is not None and source == destination)
                or (new_auth is not None and new_auth in (str(old_auth), receipt.get("old_auth")))):
            found.append(f"receipt {path.stem} names the walled login as its destination")
    # A session record is named by its process and does not outlive the handoff: the
    # moved session's was gone 14.6 s after the refusal in q5f/r1, once its successor's
    # turn was accepted. So the count is kept over the run's loop, not read at its end.
    for path in (root / "codex-home" / "alab" / "sessions").glob("*.json"):
        try:
            record = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        if isinstance(record, dict) and (record.get("env") or {}).get("CODEX_AUTH_PATH") == str(old_auth):
            processes.add(path.stem)
    if len(processes) > 1:
        found.append(f"{len(processes)} session processes ran on the walled login")
    return found


def far_wall_failure(root, events, old_auth, processes, now_us=None):
    """Why a `five-hour-wall-far` run can no longer pass, in words; None while it can.

    Each is a wrong end of a five-hour wall whose reset is far off: the session was put
    back on the login it is walled on, its turn kept its request (it sent it again, or
    the host was first asked to move it more than FAR_YIELD_SECONDS after the refusal,
    or `now_us` is past that with the host never asked), or the host answered that no
    handoff is needed. `events` are the fixture server's requests in arrival order,
    `processes` the run's set of session processes seen on the walled login
    (same_account_evidence), and `now_us` the host's clock now, or None once the run
    has ended.
    """
    trace = root / "renewal-control-trace.jsonl"
    # Read on every call, refused yet or not: the set is the run's, not this call's.
    same = same_account_evidence(root, old_auth, processes)
    wall = next(((index, event) for index, event in enumerate(events) if event["method"] == "POST"
        and event.get("kind") == "foreground" and event.get("status") == 429
        and event.get("account") == "source"), None)
    if wall is None:
        return None
    index, refused = wall
    if same:
        return "it was moved to the account it is walled on: " + "; ".join(same)
    again = next((event for event in events[index + 1:] if event["method"] == "POST"
        and event.get("kind") == "foreground" and event.get("account") == "source"), None)
    if again is not None:
        waited = (again["monotonicUs"] - refused["monotonicUs"]) / 1_000_000
        return (f"its session waited in its turn: the walled login sent the turn's request "
            f"again {waited:.1f} s after the refusal")
    calls = [call for call in rotation_calls(trace) if call[2] > refused["monotonicUs"]]
    if any(reply.startswith("OK codex-rotation state=not-needed") for _, reply, _ in calls):
        return "the host answered codex-rotation not-needed: it leaves no such wall"
    # A session that is moved in the end still waited if its turn did not end at the
    # refusal: the first asking is judged, while the run goes on and at its verdict.
    asked = next((stamp for verb, _, stamp in calls if verb == "prepare"), None)
    limit = refused["monotonicUs"] + FAR_YIELD_SECONDS * 1_000_000
    if asked is not None and asked > limit:
        waited = (asked - refused["monotonicUs"]) / 1_000_000
        return (f"its session waited in its turn: the host was first asked to move it "
            f"{waited:.1f} s after the refusal, not within {FAR_YIELD_SECONDS} s; "
            + wall_evidence(events, trace))
    if asked is None and now_us is not None and now_us > limit:
        return (f"its session waited in its turn: the host was not asked to move it within "
            f"{FAR_YIELD_SECONDS} s of the refusal; " + wall_evidence(events, trace))
    return None


def validate_far_wall_trigger(events, requests, bound, names_reset=True):
    """The provider's side of the proof that a five-hour wall, and no weekly one, was left.

    `events` are the fixture server's requests in arrival order, `requests` its foreground
    model requests and `bound` the host's five-hour bound in seconds. The turn was
    answered once on the source, refused once there and answered once on the other
    account. The refusal stated the five-hour window spent, on a usable week, with a
    reset beyond the bound plus the skew, its body naming that reset or, without
    `names_reset`, no reset at all, and after it the walled login was answered no
    model request of any kind, its title request included. No reading of the source ever
    stated its week spent; after the refusal every reading of it stated that five-hour
    wall, and both the walled turn and the host read it. The host read the destination
    as usable.
    """
    def path(event):
        return event["path"].split("?", 1)[0]
    turns = [(index, event) for index, event in enumerate(events) if event["method"] == "POST"
             and event.get("kind") == "foreground"]
    order = [(event.get("account"), event.get("status")) for _, event in turns]
    probe.require(order == [("source", 200), ("source", 429), ("target", 200)] and len(requests) == 3,
        "the turn was not answered once on the source, refused once there and answered "
        f"once on the other account: {order}")
    index, wall = turns[1]
    later = [event for event in events[index + 1:] if event["method"] == "POST"
             and path(event).endswith("/responses") and event.get("account") == "source"]
    probe.require(all(event.get("status") == 429 for event in later),
        "the walled login was answered a model request after its refusal: "
        + ", ".join(f"{event.get('kind')} {event.get('status')}" for event in later))
    headers = requests[1].get("responseHeaders") or {}

    def stated(name, kind=float):
        try:
            return kind(headers.get(name))
        except (TypeError, ValueError):
            return None
    session, week = stated("x-codex-primary-used-percent"), stated("x-codex-secondary-used-percent")
    probe.require(session is not None and session >= 100 and week is not None and week < 100
        and stated("x-codex-primary-window-minutes", int) == provider_fixture.SESSION_WINDOW_SECONDS // 60
        and headers.get("x-codex-active-limit") == "codex",
        "the refusal did not state a spent five-hour window on a usable week")
    reset = stated("x-codex-primary-reset-at", int)
    distance = None if reset is None else reset - int(requests[1]["unixAt"])
    probe.require(distance is not None and distance > bound + HANDOFF_SKEW_SECONDS,
        f"the refused window's reset ({distance} s off) is not beyond the host's bound "
        f"({bound} s) and the skew")
    probe.require(requests[1].get("refusalNamedReset") is names_reset,
        "the refusal's body " + ("named no reset" if names_reset else "named a reset")
        + ", which this case does not serve")
    usage = [(position, event) for position, event in enumerate(events) if event["method"] == "GET"
             and path(event).endswith("/usage")]
    source = [(position, event) for position, event in usage if event.get("account") == "source"]
    probe.require(all(type(event.get("weekUsed")) is int and event["weekUsed"] < 100 for _, event in source),
        "a reading of the walled login stated its week spent")
    walled = [event for position, event in source if position > index]
    probe.require(bool(walled) and all(event.get("allowed") is False
        and type(event.get("sessionUsed")) is int and event["sessionUsed"] >= 100
        and type(event.get("sessionResetsIn")) is int and event["sessionResetsIn"] > bound
        for event in walled),
        "a reading of the walled login after its refusal did not state the five-hour wall")
    by_turn = sum(event.get("client") == "codex" for event in walled)
    by_host = [event for event in walled if event.get("client") == "host"]
    probe.require(by_turn > 0, "the walled turn never read its own wall")
    probe.require(bool(by_host), "the host never read the walled login's usage")
    probe.require(any(event.get("account") == "target" and event.get("client") == "host"
        and position > index and event.get("allowed") is True
        and type(event.get("sessionUsed")) is int and event["sessionUsed"] < 100
        and type(event.get("weekUsed")) is int and event["weekUsed"] < 100
        for position, event in usage), "the host never read the other account as usable")
    return {"wallUs": wall["monotonicUs"], "resetSecondsAtRefusal": distance,
        "refusalNamedReset": names_reset, "hostBoundSeconds": bound, "refusalStated": {"fiveHourUsedPercent": session, "weekUsedPercent": week},
        "walledLoginRequestsRefusedAfterWall": len(later),
        "sourceWeekUsedPercentMax": max(event["weekUsed"] for _, event in source),
        "walledUsageReadsByTurn": by_turn, "walledUsageReadsByHost": len(by_host),
        "firstHostReadOfWallUs": by_host[0]["monotonicUs"]}


def host_wall_record(root, wall_unix, bound):
    """The host's side of that proof: what its own pool ledger says the session left.

    The fixture's roster names the source login `main` and the other `target`. After the
    handoff the ledger holds, for `main`, a wall on the five-hour window alone, standing
    beyond the bound from the refusal (`wall_unix`), beside a sample whose five-hour
    window is spent and whose week is not; the pool's active row is `target`.
    """
    state = json.loads((root / "host-state" / "accounts-codex" / "state.json").read_text())
    row = (state.get("accounts") or {}).get("main") or {}
    walls, sample = row.get("walls") or {}, row.get("sample") or {}
    probe.require(sorted(walls) == ["five_hour"],
        f"the host's ledger does not hold the walled login at a five-hour wall alone: {sorted(walls)}")
    until = walls["five_hour"].get("until")
    probe.require(type(until) is int and until - int(wall_unix) > bound,
        "the host's ledger does not hold the five-hour wall beyond its bound")
    probe.require(type(sample.get("session_pct")) is int and sample["session_pct"] >= 100
        and type(sample.get("week_pct")) is int and sample["week_pct"] < 100,
        "the host's sample of the walled login is not a spent five-hour window on a usable week")
    probe.require(state.get("active") == "target" and state.get("previous") == "main",
        "the host's pool did not move from the walled login to the other account")
    return {"hostWall": "five_hour", "hostWallStandsSeconds": until - int(wall_unix),
        "hostSample": {"fiveHourUsedPercent": sample["session_pct"], "weekUsedPercent": sample["week_pct"]}}


def far_wall_verdict(root, events, requests, receipt, policy, old_auth, logins, processes,
                     names_reset=True):
    """The oracle of a `five-hour-wall-far` run beyond the weekly cases', and its record.

    `names_reset` is the case's (FAR_WALL_CASES): without it the refusal must have named no
    reset, which `five-hour-wall-far-unnamed` serves.

    The run's loop has already held the handoff to everything `weekly-quota` demands
    (the receipt's phases and two identities, the exact native operation, the successor
    on the other login answering the original turn). This adds that none of the wrong
    ends happened (far_wall_failure), that the one session process the loop saw on the
    walled login is the one the receipt moved, that the wall left was the five-hour one,
    by the provider's record and the host's own (validate_far_wall_trigger,
    host_wall_record), that the host read that wall before it answered `ready` for this
    receipt, that nothing signed in, and that neither login file changed. `policy` is
    five_hour_policy's reading, `logins` each login file's sha256 as the controller wrote
    it, and `processes` the set the loop's far_wall_failure calls filled.
    """
    failure = far_wall_failure(root, events, old_auth, processes)
    probe.require(failure is None, FAR_NOT_MOVED + str(failure))
    # Nothing seen is no evidence that nothing else ran there.
    probe.require(processes == {str(receipt.get("old_pid"))},
        "the session processes seen on the walled login are not the one the receipt moved "
        f"({receipt.get('old_pid')}): {sorted(processes)}")
    probe.require(isinstance(policy, dict) and type(policy.get("fiveHourBoundSeconds")) is int,
        "the host's five-hour bound was never read: " + str((policy or {}).get("unread")))
    proof = validate_far_wall_trigger(events, requests, policy["fiveHourBoundSeconds"],
        names_reset=names_reset)
    proof.update(host_wall_record(root, requests[1]["unixAt"], policy["fiveHourBoundSeconds"]))
    calls = rotation_calls(root / "renewal-control-trace.jsonl")
    asked = next((stamp for verb, _, stamp in calls if verb == "prepare" and stamp > proof["wallUs"]), None)
    probe.require(asked is not None, "the host was never asked to move the walled session")
    proof["hostFirstAskedSecondsAfterRefusal"] = round((asked - proof["wallUs"]) / 1_000_000, 3)
    ready = next((stamp for verb, reply, stamp in calls
        if verb == "prepare" and reply.startswith(f"OK codex-rotation state=ready id={receipt['id']} ")), None)
    probe.require(ready is not None and proof["firstHostReadOfWallUs"] < ready,
        "the host did not read the five-hour wall before it answered ready for this handoff")
    proof["readyUs"] = ready
    # Recorded, not required: what the host answered between the refusal and `ready`
    # other than OK (it refuses a session whose record is not yet at rest).
    refusals = [reply for verb, reply, stamp in calls
        if verb == "prepare" and proof["wallUs"] < stamp < ready and not reply.startswith("OK ")]
    proof["hostReadySecondsAfterRefusal"] = round((ready - proof["wallUs"]) / 1_000_000, 3)
    proof["preparesRefusedBeforeReady"] = {"count": len(refusals),
        "replies": sorted({reply[:120] for reply in refusals})}
    posted = sorted({event["path"].split("?", 1)[0] for event in events if event["method"] == "POST"
        and not event["path"].split("?", 1)[0].endswith("/responses")})
    probe.require(not posted, "a sign-in, token or other non-model request was posted: " + ", ".join(posted))
    # Both logins, by the receipt's own naming of them: an empty map would prove nothing.
    probe.require(len(logins) == 2 and set(logins) == {str(old_auth), receipt.get("new_auth")},
        "the two login files of this handoff were not both held to the bytes written: "
        + ", ".join(sorted(Path(path).parent.name for path in logins)))
    changed = sorted(Path(path).parent.name for path, digest in logins.items()
        if hashlib.sha256(Path(path).read_bytes()).hexdigest() != digest)
    probe.require(not changed, "a fixture login file changed during the run: " + ", ".join(changed))
    proof.update(noSignInOrTokenRequest=True, loginFilesUnchanged=True,
        walledLoginProcesses=sorted(processes))
    return proof


def require_case_proof(case, report):
    """Refuse to mark a run passed whose case has an oracle of its own that left no record.

    The weekly oracle alone would pass a `five-hour-wall-far` run that proved nothing of
    its trigger; this is asked where `passed` is set, so losing the call to that oracle
    fails every run of the case instead of passing it.
    """
    key = CASE_PROOFS.get(case)
    probe.require(key is None or (isinstance(report.get(key), dict) and bool(report[key])),
        f"the {case} oracle left no {key} record, so the run proves nothing of its own case")


class Model(probe.Mock):
    def __init__(self, root, weekly_quota=False, all_exhausted=False, chain=False, wall=None,
                 lost_login=None, wall_names_reset=True):
        super().__init__(False)
        probe.require(wall in (None, "five-hour-soon", "five-hour-far")
                      and (wall is None or weekly_quota), "unknown fixture wall")
        probe.require(wall_names_reset or wall == "five-hour-far",
                      "only the far five-hour wall is refused with no reset named")
        probe.require(wall is None or not (all_exhausted or chain),
                      "the five-hour wall has one usable destination and no chain")
        probe.require(lost_login is None or lost_login in REFRESH_REFUSALS,
                      "unknown refresh refusal")
        probe.require(lost_login is None or not (weekly_quota or all_exhausted or chain),
                      "a lost login is no usage wall: it has one usable destination and no quota story")
        self.root = root
        # None, or the word (REFRESH_REFUSALS) the source login's refresh is refused with.
        # The source's first model request is answered; from its second on the provider
        # knows its access token no more: every model request and usage read of it is a
        # 401. The other login is usable throughout and is answered whatever it asks.
        self.lost_login = lost_login
        # When the provider first refused the source's access token, and how many requests
        # it had been sent by then: (monotonic, unix, count).
        self.login_lost = None
        # None: the weekly wall of the quota cases. "five-hour-soon": the source's second
        # request is refused on its five-hour window, which lifts WALL_LIFT_SECONDS later.
        # "five-hour-far": it is refused on that window with the reset FAR_RESET_SECONDS
        # off and stays refused, its title request too; the destination is the weekly
        # cases', opened by `renewed`.
        self.wall = wall
        # Whether each 429 body names the spent window's reset (`resets_at`). Its headers
        # state the windows either way.
        self.wall_names_reset = wall_names_reset
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
        if self.wall == "five-hour-soon":
            # The five-hour wall is not on the week, and the other account is usable from
            # the start: the case asserts that a wall which resets soon is not handed
            # over, and that is only an assertion if the pool has somewhere to go.
            return False
        if old_token:
            # Healthy until the provider has refused it: an account's usage never says
            # "spent" before a request of its own was turned away, and a turn's first
            # preflight must find the source permitting. The far five-hour wall is never
            # on the source's week: only the five-hour rule can explain its move.
            return self.source_walled and self.wall is None
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
        if self.lost_login is not None:
            # No window of either login is ever spent: nothing but the lost login can
            # explain a move. Whether the source is still answered is `signed_out`'s.
            _, account, user = QUOTA_ACCOUNTS[generation if generation in QUOTA_ACCOUNTS else "old"]
            return provider_fixture.usage(account, user, week_used=0, allowed=True)
        if not self.weekly_quota:
            return provider_fixture.usage("fixture-account", "fixture-user", week_used=0, allowed=True)
        generation = generation if generation in QUOTA_ACCOUNTS else "old"
        _, account, user = QUOTA_ACCOUNTS[generation]
        if self.wall is not None and generation == "old":
            with self.lock:
                served = self.wall_served
            soon = self.wall == "five-hour-soon"
            if served is None or (soon and self.wall_lifted()):
                return provider_fixture.usage(account, user, week_used=10, allowed=True)
            # The window's reset stays where the refusal put it: one reading of the clock
            # gives both the distance and the instant it is measured from.
            now = int(time.time())
            remaining = max(1, int(served[1]) + (
                WALL_RESET_SECONDS if soon else FAR_RESET_SECONDS) - now)
            return provider_fixture.usage(account, user, week_used=10, session_used=100,
                                          allowed=False, session_resets_in=remaining, now=now)
        full = self.week_spent(generation == "old", generation == "new", generation == "next")
        return provider_fixture.usage(account, user, week_used=100 if full else 0, allowed=not full)

    def signed_out(self, generation):
        """Whether the provider refuses `generation`'s access token now: the source's,
        once a lost-login case has refused its second model request. No side effect."""
        if self.lost_login is None or generation in ("new", "next"):
            return False
        with self.lock:
            return self.login_lost is not None

    def unauthorized(self, record=None):
        """The 401 of a request whose access token the provider no longer knows."""
        return 401, json.dumps({"error": {"type": "invalid_request_error",
            "code": "invalid_api_key", "message": "Fixture login expired"}}).encode(), "application/json", None, record

    def refresh_reply(self, generation):
        """What a refresh of `generation`'s refresh token is answered: (status, body).

        A fixture login cannot be refreshed. The source of a lost-login case is refused
        with that case's word; every other token, as before these cases, with the 400
        `invalid_grant` of a spent one.
        """
        word = self.lost_login if self.lost_login is not None and generation == "old" else "invalid_grant"
        status, body = REFRESH_REFUSALS[word]
        return status, json.dumps(body).encode()

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
            usage, message, headers=self.limit_headers(generation, wall=True, usage=usage),
            names_reset=self.wall_names_reset))
        record["responseHeaders"] = reply["headers"]
        record["refusalNamedReset"] = "resets_at" in reply["body"]["error"]
        return reply["status"], json.dumps(reply["body"]).encode(), "application/json", None, record

    def answered(self, record, generation):
        """Put the rate-limit headers of a 200 to `generation`'s login in its record.

        A response the provider answers never states a spent window. This fixture does
        answer a login whose usage reads as not allowed: a title request that arrives
        after the wall (the far five-hour wall alone refuses it), and a walled or
        exhausted login that sends a model request again, which the cases' own oracles
        fail. Such a 200 states no windows, as every response
        did before these headers existed, and its record says they were withheld.
        """
        if self.usage_body(generation)["rate_limit"]["allowed"]:
            record["responseHeaders"] = self.limit_headers(generation)
        else:
            record["responseHeaders"] = {}
            record["limitHeadersWithheld"] = True

    def usage(self, old_token, new_token, next_token=False):
        probe.require((self.weekly_quota or self.lost_login is not None)
                      and (old_token or new_token or next_token), "unexpected usage identity")
        if self.lost_login is not None:
            # Nothing holds a lost login's destination: the host reads it once, a minute
            # after the loss, and every reading is counted by the case's oracle.
            with self.lock:
                self.usage_reads.append("source" if old_token else "target")
            return self.usage_body("old" if old_token else "new")
        if (new_token or next_token) and self.wall != "five-hour-soon":
            # The cases that hand a session over hold a destination's first reading until
            # the host has opened it. five-hour-wall-soon opens nothing: its destination
            # answers at once.
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
        if self.lost_login is not None:
            if generation not in ("new", "next") and (
                    self.signed_out(generation) or (kind == "foreground" and number > 1)):
                # The source's access token ended after its first answered request: this
                # request, its title's too, and every later one of that login are refused.
                with self.lock:
                    if self.login_lost is None:
                        self.login_lost = (self.monotonic(), time.time(), len(self.http_events))
                return self.unauthorized(record)
            if kind == "thread-title":
                item = {"type": "message", "role": "assistant", "id": "fixture-title",
                    "content": [{"type": "output_text", "text": json.dumps({"title": "Recover original work"})}]}
                self.answered(record, generation)
                return 200, probe.response("fixture-title", item), "text/event-stream", None, record
            if number == 1:
                item = {"type": "function_call", "call_id": probe.CALL_ID, "name": "update_plan",
                        "arguments": json.dumps({"plan": [{"step": probe.PLAN, "status": "completed"}]})}
            else:
                item = {"type": "message", "role": "assistant", "id": "renewed-reply",
                        "content": [{"type": "output_text", "text": "Native renewal fixture recovered."}]}
            self.answered(record, generation)
            return 200, probe.response(f"renewal-{number}", item), "text/event-stream", None, record
        if kind == "thread-title":
            if self.wall == "five-hour-far" and generation not in ("new", "next"):
                with self.lock:
                    walled = self.wall_served is not None
                if walled:
                    # A provider at this wall refuses every model request of the account,
                    # the title's too: the weekly cases answer theirs, this one does not.
                    return self.refused(record, generation, "Fixture five-hour limit reached")
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
        if self.wall == "five-hour-far" and number > 1 and generation not in ("new", "next"):
            # The source's five-hour window is spent from its second request to the end
            # of the run: the first refusal, and any request it sends again.
            with self.lock:
                self.source_walled = True
                if self.wall_served is None:
                    self.wall_served = (self.monotonic(), time.time())
            return self.refused(record, generation, "Fixture five-hour limit reached")
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
            return self.unauthorized(record)
        if number == 1:
            item = {"type": "function_call", "call_id": probe.CALL_ID, "name": "update_plan",
                    "arguments": json.dumps({"plan": [{"step": probe.PLAN, "status": "completed"}]})}
        else:
            item = {"type": "message", "role": "assistant", "id": "renewed-reply",
                    "content": [{"type": "output_text", "text": "Native renewal fixture recovered."}]}
        self.answered(record, generation)
        return 200, probe.response(f"renewal-{number}", item), "text/event-stream", None, record


def usage_figures(body):
    """What a usage reading stated of its two windows: how much of each is used, and how
    far off the five-hour window's reset is."""
    limit = body["rate_limit"]
    return {"sessionUsed": limit["primary_window"]["used_percent"],
            "weekUsed": limit["secondary_window"]["used_percent"],
            "sessionResetsIn": limit["primary_window"]["reset_after_seconds"]}


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



def serve(model, secure=True, issuer_only=False):
    """Serve `model` on a loopback port: under the fixture's TLS, or without (`secure`).

    `issuer_only` is the logins' issuer of a lost-login case and nothing else: it answers
    a token request, and any other request with 404, listed among the unrouted.

    A lost-login case's record also says, for each request, which program its User-Agent
    names by that program's own form (`agent`), marks a request the fixture has no route
    for (`unrouted`), and holds each TLS handshake a client would not complete
    (`method` "TLS"): a program that does not trust the fixture's CA sends no request
    this server could count, and its refused handshake is all there is to see of it.
    """
    def sender(headers):
        """Who sent a request, for its record."""
        said = {"account": request_account(headers), "client": request_client(headers)}
        if model.lost_login is not None:
            said["agent"] = request_agent(headers)
            if said["agent"] == "other":
                said["agentSeen"] = agent_seen(headers)
        return said

    class Listener(provider_fixture.LoopbackHTTPServer):
        def get_request(self):
            try:
                return super().get_request()
            except OSError as error:
                # The listening socket is the fixture's TLS: accepting a connection is
                # its handshake, and this is a client that did not complete it.
                if model.lost_login is not None and secure:
                    with model.lock:
                        model.http_events.append({"method": "TLS", "path": "", "kind": "handshake-refused",
                            "account": None, "client": None, "monotonicUs": monotonic_us(),
                            "reason": str(getattr(error, "reason", None) or type(error).__name__)[:80]})
                raise

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def not_the_issuers(self, method):
            """Refuse, on the issuer's listener, a request that is no token request."""
            path = self.path.split("?", 1)[0]
            if not issuer_only or (method == "POST" and path.endswith("/oauth/token")):
                return False
            with model.lock:
                model.http_events.append({"method": method, "path": self.path, "kind": "non-responses",
                    **sender(self.headers), "monotonicUs": monotonic_us(), "listener": "issuer",
                    "status": 404, "unrouted": True})
                model.unrouted_posts.append({"path": path, "client": request_client(self.headers)})
            self.send_error(404)
            return True

        def do_GET(self):
            if self.not_the_issuers("GET"):
                return
            path = self.path.split("?", 1)[0]
            event = {"method": "GET", "path": self.path, **sender(self.headers),
                "monotonicUs": monotonic_us()}
            with model.lock:
                model.http_events.append(event)
                (model.root / "http-events-live.json").write_text(json.dumps(model.http_events))
            accounts = model.weekly_quota or model.lost_login is not None
            if model.lost_login is not None:
                # A lost-login case's oracles count what each login was answered.
                lost = bearer_generation(self.headers) == "old" and model.signed_out("old")
                with model.lock:
                    event["status"] = 401 if lost else 200
                    if lost and path.endswith("/usage"):
                        model.usage_reads.append("source")
                if lost:
                    # The provider no longer knows this login's access token, whatever
                    # it asks with it.
                    _, data, content_type, _, _ = model.unauthorized()
                    self.send_response(401)
                    self.send_header("content-type", content_type)
                    self.send_header("content-length", str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)
                    return
            body = {"models": []}
            try:
                if path.endswith(CREDITS_PATH):
                    # No earned reset exists in this fixture: a valid empty list.
                    body = provider_fixture.check("credits", provider_fixture.credit_list())
                if path.endswith("/usage") and not accounts:
                    # The same-account cases have one login and no quota story: its
                    # account is healthy. Model.usage is the quota cases' alone.
                    body = provider_fixture.check("usage", provider_fixture.usage(
                        "fixture-account", "fixture-user", week_used=0, allowed=True))
                if accounts and path.endswith("/usage"):
                    generation = bearer_generation(self.headers)
                    old_token, new_token, next_token = (
                        generation == "old", generation == "new", generation == "next")
                    if not (old_token or new_token or (model.chain and next_token)):
                        self.send_error(401)
                        return
                    # Every model's reply is held to what the client decodes: none opts out.
                    body = provider_fixture.check("usage", model.usage(old_token, new_token, next_token))
                    # What the reader was told, for the oracles: the verdict and each
                    # window's figure, never the reply.
                    with model.lock:
                        event["allowed"] = body["rate_limit"]["allowed"]
                        event.update(usage_figures(body))
            except Exception as error:
                # Seen by the controller's loop (require_no_fixture_fault), not by the client.
                with model.lock:
                    model.fixture_faults.append({"path": path, "error": f"{type(error).__name__}: {error}"})
                self.send_error(500)
                return
            if path.endswith("/accounts/check"):
                account = "fixture-account"
                if accounts:
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
            if self.not_the_issuers("POST"):
                return
            with model.lock:
                event = {"method": "POST", "path": self.path, "kind": "non-responses",
                    **sender(self.headers), "monotonicUs": monotonic_us()}
                if issuer_only:
                    event["listener"] = "issuer"
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
                # A fixture login cannot be refreshed: its refresh token is spent, and the
                # source's of a lost-login case is refused with that case's word. Whose
                # token was presented is kept, by its role and never its text.
                generation = refresh_generation(payload)
                status, data = model.refresh_reply(generation)
                content_type = "application/json"
                with model.lock:
                    event["refreshOf"] = QUOTA_ACCOUNTS.get(generation, (None,))[0]
                    event["status"] = status
            elif not path.endswith("/responses"):
                # No route: answered as before, and listed apart from model requests of
                # unknown shape (unknown-requests.json), which fail a run.
                status, data, content_type = 400, b'{"error":"invalid_grant"}', "application/json"
                with model.lock:
                    event["unrouted"] = True
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
                    if model.login_lost is not None and status == 401:
                        # From this many requests on, the source's token was refused.
                        event["requestsAtLoss"] = model.login_lost[2]
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
    server = Listener(("127.0.0.1", 0), Handler)
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


# Each image digest process_evidence has read, by file identity. The package codex is
# about 322 MB, and a waiting loop otherwise reads it again on every pass.
IMAGE_DIGESTS = {}


def image_digest(executable):
    """The image's stat and sha256, read again only when its identity or contents change.

    ctime is in the key because, unlike mtime, nothing can set it back after a write. A
    digest is kept only when the file did not change while it was read.
    """
    def identity(stat):
        return (stat.st_dev, stat.st_ino, stat.st_size, stat.st_mtime_ns, stat.st_ctime_ns)
    with executable.open("rb") as image:
        stat = os.fstat(image.fileno())
        digest = IMAGE_DIGESTS.get(identity(stat))
        if digest is None:
            digest = hashlib.file_digest(image, "sha256").hexdigest()
            if identity(os.fstat(image.fileno())) == identity(stat):
                IMAGE_DIGESTS[identity(stat)] = digest
    return stat, digest


def process_evidence(record):
    executable = Path(record["exe"]).resolve(strict=True)
    identity = record.get("identity")
    probe.require(isinstance(identity, dict)
        and type(identity.get("startSeconds")) is int
        and type(identity.get("startMicroseconds")) is int, "native Darwin process birth is absent")
    stat, digest = image_digest(executable)
    return {"pid": record["pid"], "birth": record["identity"], "exe": str(executable),
            "kernelBirth": identity["startSeconds"] * 1000000 + identity["startMicroseconds"],
            "image": {"device": stat.st_dev, "inode": stat.st_ino, "sha256": digest}}


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


def rollback_native_completed(ledger):
    """Whether the old operation's native ledger durably holds its completion yet.

    A Retired receipt is not that proof. The rollback vendor's own `status` retires the
    receipt from live evidence alone, before the host's renewal worker has read the
    native Completed reply, and aterm refuses pool retirement until it has: a retired
    receipt alone is insufficient (aterm codex_renewal_attempts.rs). That reply writes
    native_completed_prepare and native_observation in one save. A ledger that has
    ended the operation any other way can never complete it, and fails here, naming
    what ended it, instead of at the hang detector.
    """
    if ledger.get("native_completed_prepare"):
        probe.require(ledger.get("native_observation"),
            "rollback native completion carries no observation")
        return True
    ended = []
    # The same flag marks a staged account candidate, which does not end the operation.
    if ledger.get("native_retired_unissued") is True and "native_account_candidate" not in ledger:
        ended.append("retired with no exec granted (native_retired_unissued)")
    # A stop while the prepare is still retained can yet complete; a released one cannot.
    if not ledger.get("native_prepare") and "native_id" not in ledger:
        if ledger.get("native_noexec_prepare"):
            ended.append("proven never executed (native_noexec_prepare)")
        if ledger.get("phase") == "failed":
            ended.append("stopped: " + (ledger.get("why") or "no reason recorded"))
        ended = ended or ["released its prepare without completing it"]
    probe.require(not ended, "rollback native operation ended without completion: " + "; ".join(ended))
    return False


def rollback_proven(boundary, original, claimed, ledger, process, old, new, codex):
    """Whether the old operation's live rollback is proven; False while it is still due.

    The Retired receipt, the rollback vendor's proven claim and the ledger's native
    completion arrive in any order, so each is waited for. Once all three exist, every
    part of the proof must hold exactly.
    """
    completed = rollback_native_completed(ledger)
    if original is None or original["phase"] != "retired" or claimed is None or not completed:
        return False
    probe.require(original.get("revoked") is False
        and original.get("launch_attempted") is True
        and original.get("successor_pid") == process["pid"]
        and original.get("successor_birth") == process["kernelBirth"]
        and original.get("accepted_turn") is None
        and (original.get("candidate") or {}).get("forward") is False
        and (original.get("candidate") or {}).get("pid") == process["pid"]
        and original.get("old_auth") == str(old) and original.get("new_auth") == str(new)
        and original.get("current_attempt") is None
        and original["expires_ms"] == boundary["originalExpiresMs"]
        and hashlib.sha256(original["native_prepare"].encode()).hexdigest()
            == boundary["originalPrepareSha256"]
        and ledger.get("native_rollback") is True
        # Completion clears native_prepare; the completed prepare is the original one.
        and original["native_prepare"] == ledger["native_completed_prepare"]
        and process["exe"] == str(codex),
        "rollback lacks exact old operation, auth, image and completed phase proof")
    return True


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


# How long the host has to exit once `stop` exists.
HOST_EXIT_SECONDS = 15
# What samples the threads of a host that outlives that wait, where macOS ships it.
SAMPLER = Path("/usr/bin/sample")


def host_exit_diagnostics(root, process, budget_seconds):
    """Record what a host that outlived its exit wait is doing, before it is stopped.

    Two failures end in terminate() with the same code. One is a host stalled in its own
    teardown: in weekly-quota-exhausted r3 (2026-11-13), aterm's fixture was joining a
    control worker whose main-thread hop nothing pumped any more, with codex already
    gone. The other is a product that hangs. Whether codex still lives, and where the
    host's threads wait, tell them apart. This extends no wait, and the run still fails.
    `budget_seconds` is what the guardian leaves for sampling.
    """
    def alive(pid):
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return False
        except PermissionError:
            return True
        except OSError:
            return None
        return True

    sampler = SAMPLER
    diagnostics = {"hostPid": process.pid, "hostAlive": process.poll() is None, "codex": []}
    # A session record is named by its process ID and keeps that name after codex exits.
    for path in sorted((root / "codex-home" / "alab" / "sessions").glob("*.json")):
        if not re.fullmatch(r"[1-9][0-9]{0,6}", path.stem):
            continue
        try:
            record = json.loads(path.read_text())
        except (OSError, ValueError):
            record = None
        record = record if isinstance(record, dict) else {}
        diagnostics["codex"].append({"pid": int(path.stem), "alive": alive(int(path.stem)),
            "recordPid": record.get("pid"), "exit": record.get("exit")})
    sample = {"tool": str(sampler)}
    if not sampler.exists():
        sample["skipped"] = "sampler absent"
    elif not diagnostics["hostAlive"]:
        sample["skipped"] = "host exited"
    elif budget_seconds < 3:
        sample["skipped"] = f"{max(budget_seconds, 0):.1f} s of guardian budget left"
    else:
        output = root / "host-sample.txt"
        try:
            completed = subprocess.run([str(sampler), str(process.pid), "1", "-mayDie", "-file", str(output)],
                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                timeout=min(budget_seconds, 10))
            sample.update(returncode=completed.returncode,
                output=completed.stdout.decode(errors="replace")[-4096:])
        except subprocess.TimeoutExpired:
            sample["timedOut"] = True
        except OSError as error:
            sample["error"] = str(error)
        sample["file"] = output.name if output.is_file() else None
    diagnostics["sample"] = sample
    probe.write_json(root / "host-exit-diagnostics.json", diagnostics)
    return diagnostics


def main():
    # Under --under-guardian, the guardian stops this process about guardian_seconds from here.
    started = time.monotonic()
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
    far_wall = args.case in FAR_WALL_CASES
    # Whether the far wall's 429 names the window's reset; True for every other case.
    wall_names_reset = FAR_WALL_CASES.get(args.case, True)
    weekly_quota = args.quota or args.case in ("weekly-quota", "weekly-quota-rollback", "weekly-quota-exhausted", "weekly-quota-chain", "five-hour-wall-soon", *FAR_WALL_CASES)
    rollback_case = args.case in ("failed-forward-rollback", "weekly-quota-rollback")
    # A lost-login case: the word its source's refresh is refused with and whether the
    # host moves lost logins, or None. Only `lost-login-expired` is moved.
    lost_login = LOST_LOGIN_CASES.get(args.case)
    lost_moved = args.case in LOST_LOGIN_MOVED
    lost_stays = lost_login is not None and not lost_moved
    if lost_login is not None and args.quota:
        parser.error("a lost-login case builds its own two accounts; --quota does not apply")
    # Two logins of distinct accounts: a usage-limit handoff's, and a lost login's.
    two_accounts = weekly_quota or lost_login is not None
    model = Model(root, weekly_quota=weekly_quota,
                  all_exhausted=args.case == "weekly-quota-exhausted", chain=chain,
                  wall="five-hour-soon" if soon_wall else "five-hour-far" if far_wall else None,
                  lost_login=lost_login[0] if lost_login else None,
                  wall_names_reset=wall_names_reset)
    server, server_thread = serve(model)
    # The logins' issuer of a lost-login case, where it is not the fixture's TLS server.
    issuer_server = issuer_thread = None
    if lost_login is not None and lost_login[2] == PLAIN_ISSUER:
        issuer_server, issuer_thread = serve(model, secure=False, issuer_only=True)
    report = {"passed": False, "scope": "isolated native host and real Codex PTY",
              "source": build["source"], "version": manifest["version"], "case": args.case,
              "quotaAccounts": weekly_quota,
              "observationBudgetSeconds": {"drive": drive_seconds, "guardian": guardian_seconds},
              "nativeHost": native_host_record.summary(host_record),
              # What the package's own record is worth: a trial package, or a record no
              # certified packager wrote, qualifies nothing. `passed` is this run's own
              # verdict and does not depend on it.
              "package": package_standing.summary(build),
              "packageRecords": {"build": build, "manifest": manifest},
              "controllerSha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              # The modules that decide what the fixture serves, which host it accepts
              # and what the package's record is worth.
              "fixtureSha256": {Path(module.__file__).name: hashlib.sha256(
                  Path(module.__file__).read_bytes()).hexdigest()
                  for module in (provider_fixture, native_host_record, package_standing)}}
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
        if issuer_server is not None:
            issuer = f"http://127.0.0.1:{issuer_server.server_port}"
        fake_auth(old, "old", issuer, "source-account" if two_accounts else "fixture-account")
        fake_auth(new, "new", issuer, "target-account" if two_accounts else "fixture-account",
                  email="target@example.invalid" if two_accounts else "fixture@example.invalid",
                  user="target-user" if two_accounts else "fixture-user")
        # Each login file as written, for the case that proves neither changed.
        login_digests = {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in (old, new)}
        # The session processes the loop saw on the source login (same_account_evidence).
        walled_login_processes = set()
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
        if lost_login is not None:
            # `lost_login` lays out the usage-limit handoff's pool and policy with the
            # lost login's move enabled instead of the handoff (aterm
            # native_renewal_fixture.rs). The host that prints no word for the move is the
            # usage-limit handoff's own: the same pool, with only that handoff enabled.
            config["lost_login" if lost_login[1] else "weekly_quota"] = True
            report["lostLogin"] = {"refusal": lost_login[0],
                "refusalStatus": REFRESH_REFUSALS[lost_login[0]][0], "hostMovesLostLogins": lost_login[1],
                "issuer": lost_login[2]}
        config["env"]["CODEX_CA_CERTIFICATE"] = str(root / "fixture-ca.pem")
        # What this run asked the host's fixture policy for, as written to fixture.json.
        report["fixtureConfig"] = {"weekly_quota": config.get("weekly_quota", False),
            "lost_login": config.get("lost_login", False),
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
            # When the loop first saw a usage-wall successor at rest on its receipt.
            completion_wait_from = None
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
                    if far_wall:
                        # The weekly handoff below is this case's too. What is its own
                        # is failed here by name while the run goes on: a session put
                        # back on its walled login, a turn that keeps its request, a
                        # host that declines, and a host whose policy leaves no such wall.
                        with model.lock:
                            seen = list(model.http_events)
                            walled = model.wall_served is not None
                        if walled and metadata is not None and "hostFiveHourPolicy" not in report:
                            try:
                                answer = control(root, metadata, f"@{metadata['sid']} harness policy")
                            except (OSError, KeyError) as error:
                                # No answer is not the host's "no": the verdict requires one.
                                report["hostFiveHourPolicy"] = {"unread": f"{type(error).__name__}: {error}"}
                            else:
                                # The reply is kept before it is judged: a run this
                                # refuses still shows what its host printed.
                                report["hostFiveHourPolicy"] = {"reply": answer[:POLICY_REPLY_BYTES]}
                                report["hostFiveHourPolicy"].update(five_hour_policy(answer))
                        failure = far_wall_failure(root, seen, old, walled_login_processes, monotonic_us())
                        probe.require(failure is None, FAR_NOT_MOVED + str(failure))
                    if lost_login is not None:
                        # What is a lost login's own is judged here while the run goes on:
                        # the host's word for the move, and each wrong end by name.
                        with model.lock:
                            seen = list(model.http_events)
                            signed_out = model.login_lost is not None
                        if signed_out and metadata is not None and "hostLoginPolicy" not in report:
                            try:
                                answer = control(root, metadata, f"@{metadata['sid']} harness policy")
                            except (OSError, KeyError) as error:
                                # No answer is not the host's "no": the verdict requires one.
                                report["hostLoginPolicy"] = {"unread": f"{type(error).__name__}: {error}"}
                            else:
                                report["hostLoginPolicy"] = {"reply": answer[:POLICY_REPLY_BYTES]}
                                report["hostLoginPolicy"].update(login_policy(answer))
                        fault = login_record_fault(seen)
                        probe.require(fault is None, str(fault))
                        if lost_stays:
                            # Nothing is moved, so this controller publishes no `renew`
                            # and follows no receipt: it fails the run the moment anything
                            # of a move exists, and ends it once the login wall has stood
                            # on its own path for LOGIN_WALL_HOLD_SECONDS.
                            found = login_move_evidence(root, seen, new)
                            probe.require(not found, LOGIN_NOT_HELD + "; ".join(found))
                            if login_wall_stood(root, seen, monotonic_us()):
                                with model.lock:
                                    report["requestCount"] = len(model.requests)
                                report["loginWallWatchedUntilUs"] = monotonic_us()
                                renewed = True
                                break
                            continue
                        failure = lost_login_failure(root, seen, report.get("hostLoginPolicy"), monotonic_us())
                        probe.require(failure is None, LOGIN_NOT_MOVED + str(failure))
                        if "loginMoveSeenUs" in report:
                            # The move is proven. The run watches on, so that what must
                            # not follow a move (a token presented, the destination read
                            # again by the host, the turn sent again) has time to show.
                            if login_move_watched(report["loginMoveSeenUs"], monotonic_us()):
                                report["loginMoveWatchedUntilUs"] = monotonic_us()
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
                            if lost_login is not None:
                                # Which intent the session put on its record for this
                                # wall: its rule, and the field its marker is kept in.
                                report["sourceIntent"] = {"rule": previous["owed"].get("rule"),
                                    "field": next((key for key in ("renewal", "rotation")
                                        if previous["owed"].get(key) is not None), None),
                                    "text": previous["owed"].get("text")}
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
                                    if rollback_proven(boundary, original, claimed, ledger,
                                            process_proof, old, new, codex):
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
                        settled = bool(candidate and receipt and count > report["requestsBeforeRenewal"]
                                       and successor_rest(candidate, report["thread"]) is not None)
                        if settled and weekly_quota:
                            # A usage-wall successor at rest has yet to have its report
                            # answered as often as not: the host stops once that is
                            # traced, or COMPLETION_WAIT_SECONDS on (completion_verdict).
                            if completion_wait_from is None:
                                completion_wait_from = time.monotonic()
                            if (completion_seen(rotation_calls(root / "renewal-control-trace.jsonl"),
                                                receipt["id"]) is None
                                    and time.monotonic() < completion_wait_from + COMPLETION_WAIT_SECONDS):
                                continue
                        if settled:
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
                            if lost_moved:
                                report["loginMoveSeenUs"] = monotonic_us()
                                continue
                            renewed = True
                            break
                hang = "native same-account renewal did not reach its required causal outcome before the hang detector"
                if not renewed:
                    hang += "; " + live_snapshot_outcomes(root / "renewal-control-trace.jsonl")
                if not renewed and weekly_quota:
                    with model.lock:
                        seen = list(model.http_events)
                    hang += "; " + wall_evidence(seen, root / "renewal-control-trace.jsonl")
                if not renewed and lost_login is not None:
                    with model.lock:
                        seen = list(model.http_events)
                    hang += "; " + login_evidence(seen, root / "renewal-control-trace.jsonl")
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
                stalled = False
                try:
                    code = process.wait(timeout=HOST_EXIT_SECONDS)
                except subprocess.TimeoutExpired:
                    stalled = True
                    try:
                        # Leave the terminate wait and the result their time inside the guardian.
                        host_exit_diagnostics(root, process,
                            started + guardian_seconds - time.monotonic() - 20)
                    finally:
                        process.terminate()
                    code = process.wait(timeout=10)
                process.stdout.close()
                probe.write_json(root / "host-wait.json",
                    {"pid": process.pid, "returncode": code, "stalled": stalled})
        # A host that exits by itself while it is sampled still outlived its wait.
        probe.require(not stalled, "native host did not exit cleanly: it outlived its "
            f"{HOST_EXIT_SECONDS} s wait after stop (host-exit-diagnostics.json)")
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
            if "nativeReceipt" in report:
                # The handoff's successor reported its resumed turn served, and that proved
                # the destination (aterm A1, codex-alab C2): a host or package without the
                # report fails here by name.
                pool = root / "host-state" / "accounts-codex"
                report["completionReported"] = completion_verdict(
                    rotation_calls(root / "renewal-control-trace.jsonl"), report["nativeReceipt"],
                    json.loads((pool / "state.json").read_text()), pool_ledger(pool))
            if far_wall:
                report["fiveHourWallHandedOver"] = far_wall_verdict(root, events, model.requests,
                    report["nativeReceipt"], report.get("hostFiveHourPolicy"), old, login_digests,
                    walled_login_processes, names_reset=wall_names_reset)
        elif lost_login is not None:
            with model.lock:
                events = list(model.http_events)
            if lost_moved:
                # What held before the last proof, how often it asked, is kept for a run
                # that fails there.
                held = {}
                try:
                    report[CASE_PROOFS[args.case]] = lost_login_verdict(root, events, model.requests,
                        report["nativeReceipt"], report.get("hostLoginPolicy"), old, new, login_digests,
                        thread_rollout(root, report["thread"]), report.get("sourceIntent"),
                        lost_login[2], report.get("loginMoveWatchedUntilUs", 0), held=held)
                except RuntimeError:
                    if held:
                        report["lostLoginHeldBeforeItsAsks"] = held
                    raise
            else:
                report["lostLoginStayed"] = login_stayed_verdict(root, events,
                    report.get("hostLoginPolicy"), lost_login, old, new, login_digests,
                    thread_rollout(root), report.get("loginWallWatchedUntilUs", 0))
        # The cases that end with the turn still refused sent it twice at least.
        unanswered = lost_stays or args.case in ("owner-stop", "owner-stop-after-signal", "post-signal-expiry-diagnostic", "weekly-quota-rollback", "weekly-quota-exhausted")
        probe.require(len(model.requests) >= (2 if unanswered else 3),
                      "renewal did not exercise completed-tool work")
        for request in model.requests:
            probe.require(user_texts(request).count(probe.PROMPT) == 1,
                          "original user input was lost or duplicated")
        failed_input = model.requests[1]["body"]["input"] if unanswered else model.requests[-2]["body"]["input"]
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
        require_case_proof(args.case, report)
        report["passed"] = args.case != "post-signal-expiry-diagnostic"
        if args.case == "post-signal-expiry-diagnostic":
            report["error"] = "Known unresolved goal gap: expired post-Signal receipt remains Committing after hold lifts"
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
    finally:
        (root / "stop").touch()
        for listener, worker in ((server, server_thread), (issuer_server, issuer_thread)):
            if listener is not None:
                listener.shutdown()
                worker.join()
                listener.server_close()
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
