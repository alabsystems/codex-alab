#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The provider replies the rotation smokes serve, in the shape the real client decodes.

One place builds the usage, earned-reset list and redemption replies and the refusal of a
model request at a wall, of every wall kind the provider names (a spent window of the
ordinary `codex` bucket or of another limit's, a workspace's reached type, spend control,
a credit-only plan out of credits, and a plan that does not include the usage), and
`check` refuses a reply whose required fields are missing or mistyped. The client decodes
these replies strictly: a usage window without `reset_after_seconds` fails the whole read,
the client treats that as "usage unknown", and a walled turn then polls until the smoke's
hang detector fires. `check` names, for each reply, the Rust struct that decides its shape.

Imported by the smokes beside it. Stands alone (no sibling imports).

  provider_fixture.py --write-golden <dir>

writes the replies as files, with a fixed clock. The committed copies under
alab/smoke/fixtures/provider are what codex-rs/alab-quota-recovery's `fixture_tests`
decodes through the real client, in the gate.
"""

import argparse
import http.server
import ipaddress
import json
import math
from pathlib import Path
import re
import socketserver
import sys
import time

SESSION_WINDOW_SECONDS = 5 * 3600
WEEK_WINDOW_SECONDS = 7 * 24 * 3600
I32 = range(-2 ** 31, 2 ** 31)
I64 = range(-2 ** 63, 2 ** 63)

# codex-backend-openapi-models `PlanType`. An unlisted word decodes as `unknown`, which
# no account really has, so the fixture may only name a listed plan.
PLAN_TYPES = frozenset((
    "guest", "free", "go", "plus", "pro", "prolite", "promax", "free_workspace", "team",
    "self_serve_business_prolite", "self_serve_business_usage_based", "business", "ent26",
    "enterprise_cbp_automation", "enterprise_cbp_usage_based", "education", "quorum", "k12",
    "enterprise", "edu", "edu_plus", "edu_pro", "sci", "sci_plus"))
# codex-backend-client `ConsumeRateLimitResetCreditCode` (snake_case, no fallback).
CONSUME_CODES = frozenset(("reset", "nothing_to_reset", "no_credit", "already_redeemed"))
# codex-protocol `RateLimitReachedType` (`FromStr`; another word is dropped unread).
REACHED_TYPES = frozenset((
    "rate_limit_reached", "workspace_owner_credits_depleted",
    "workspace_member_credits_depleted", "workspace_owner_usage_limit_reached",
    "workspace_member_usage_limit_reached"))
# The reached type of an ordinary wall; every other one is a workspace's.
ORDINARY_REACHED = "rate_limit_reached"
# A limit id as codex-api's `normalize_limit_id` leaves it (trimmed, lower case, `_` for
# `-`). The fixture names every limit that way, so the id the client reads back from a
# refusal's `x-codex-active-limit` and from a usage read's `metered_feature` is the one sent.
LIMIT_ID = re.compile(r"[a-z0-9_]+")
# A window header of one limit's family: `x-<family>-primary-used-percent` and so on.
WINDOW_HEADER = re.compile(r"(x-[a-z0-9-]+)-(primary|secondary)-used-percent")


class LoopbackHTTPServer(http.server.ThreadingHTTPServer):
    """Bind a numeric loopback fixture without a reverse-DNS lookup for its label."""

    def server_bind(self):
        if not ipaddress.ip_address(self.server_address[0]).is_loopback:
            raise ValueError("fixture server requires a numeric loopback address")
        socketserver.TCPServer.server_bind(self)
        self.server_name, self.server_port = self.server_address[:2]


def window(used, seconds, resets_in, now):
    """One usage window: `RateLimitWindowSnapshot`, four required integers."""
    return {"used_percent": int(used), "limit_window_seconds": int(seconds),
            "reset_after_seconds": int(resets_in), "reset_at": int(now) + int(resets_in)}


def usage(account, user, *, week_used, session_used=0, allowed, plan="pro", credits=0,
          week_resets_in=86400, session_resets_in=3600, now=None):
    """The reply to GET .../usage for `account` and `user`."""
    now = int(time.time()) if now is None else int(now)
    return {"account_id": account, "user_id": user, "plan_type": plan,
            "rate_limit": {"allowed": bool(allowed), "limit_reached": not allowed,
                "primary_window": window(session_used, SESSION_WINDOW_SECONDS, session_resets_in, now),
                "secondary_window": window(week_used, WEEK_WINDOW_SECONDS, week_resets_in, now)},
            "rate_limit_reset_credits": {"available_count": int(credits)}}


def reached_type(kind):
    """codex-backend-openapi-models `RateLimitReachedType`: `{"type": <RateLimitReachedKind>}`."""
    return {"type": kind}


def spend_control(*, limit, used, resets_in, now):
    """`SpendControlStatusDetails` with its `individual_limit` (`SpendControlLimitDetails`).

    The amounts are decimal text and the percents whole numbers, as the model types them;
    the source is the one upstream's own app-server fixture names. It is reached once
    `used` meets `limit`.
    """
    remaining = limit - used
    return {"reached": used >= limit, "individual_limit": {
        "source": "workspace_spend_controls", "limit": str(limit), "used": str(used),
        "remaining": str(remaining), "used_percent": used * 100 // limit,
        "remaining_percent": remaining * 100 // limit,
        "reset_after_seconds": int(resets_in), "reset_at": int(now) + int(resets_in)}}


def credit_balance(*, has_credits, balance):
    """`CreditStatusDetails`: whether the account holds credits, and their balance as text."""
    return {"has_credits": bool(has_credits), "unlimited": False, "balance": str(balance)}


def additional_limit(name, feature, *, allowed, primary=None, secondary=None):
    """One entry of a usage reply's `additional_rate_limits`.

    backend-client `AdditionalRateLimitWithNormalModel`: codex-backend-openapi-models
    `AdditionalRateLimitDetails` (the limit's name, the metered feature the client keeps
    as its limit id, and the limit's own `RateLimitStatusDetails`) and the normal model
    whose picker metadata it borrows, which the fixture leaves unnamed.
    """
    limit = {"allowed": bool(allowed), "limit_reached": not allowed}
    for key, snapshot in (("primary_window", primary), ("secondary_window", secondary)):
        if snapshot is not None:
            limit[key] = snapshot
    return {"limit_name": name, "metered_feature": feature, "rate_limit": limit,
            "normal_model_slug": None}


def credit_only_usage(account, user, *, plan, balance=0):
    """The usage reply of a credit-only plan whose credits are spent.

    Its `codex` bucket refuses ordinary use and has no window, so no reset ends the wall,
    only a purchase; its credits say none are left.
    """
    return {"account_id": account, "user_id": user, "plan_type": plan,
            "rate_limit": {"allowed": False, "limit_reached": True},
            "credits": credit_balance(has_credits=False, balance=balance),
            "rate_limit_reset_credits": {"available_count": 0}}


def reserve_usage(account, user, *, now):
    """A reading on Reserve, the fallback upstream serves once ordinary usage is spent.

    The `codex` bucket's week is spent, and so is Reserve's own limit, named as upstream's
    TUI fixture names it (`base_model_inference`, `gpt-reserve`, a weekly window).
    """
    body = usage(account, user, week_used=100, allowed=False, now=now)
    body["additional_rate_limits"] = [additional_limit(
        "gpt-reserve", "base_model_inference", allowed=False,
        primary=window(100, WEEK_WINDOW_SECONDS, 2 * 86400, now))]
    return body


def credit_list():
    """The reply to GET .../rate-limit-reset-credits for an account with no earned reset."""
    return {"available_count": 0, "credits": []}


def consume_reply():
    """The reply to POST .../rate-limit-reset-credits/consume when nothing can be redeemed."""
    return {"code": "no_credit", "windows_reset": 0}


def header_family(limit):
    """The prefix codex-api's `parse_rate_limit_for_limit` reads a limit's headers under."""
    return "x-" + limit.replace("_", "-")


def _window_headers(prefix, bucket):
    headers = {}
    for name in ("primary", "secondary"):
        snapshot = (bucket or {}).get(name + "_window")
        if snapshot is None:
            continue
        headers[f"{prefix}-{name}-used-percent"] = str(snapshot["used_percent"])
        headers[f"{prefix}-{name}-window-minutes"] = str(snapshot["limit_window_seconds"] // 60)
        headers[f"{prefix}-{name}-reset-at"] = str(snapshot["reset_at"])
    return headers


def limit_headers(usage_body, wall=False, *, limit="codex", reached=ORDINARY_REACHED):
    """The rate-limit headers a model response carries for the account `usage_body` is for.

    The provider reports an account's windows on every model response as well as in its
    usage reply, and the two agree. These are the names codex-api's rate_limits.rs reads:
    the `codex` bucket's `x-codex-<window>-used-percent`, `-window-minutes` and `-reset-at`;
    the same three for each other limit the reading names, under its own family
    (`header_family`), with its name in `<family>-limit-name`; and a credit balance in
    `x-codex-credits-*` (`parse_credits_snapshot`). A refusal also names the limit that
    was reached (`limit`) and why (`reached`), as api_bridge.rs reads them.
    """
    headers = _window_headers("x-codex", usage_body["rate_limit"])
    for entry in usage_body.get("additional_rate_limits") or ():
        family = header_family(entry["metered_feature"])
        headers.update(_window_headers(family, entry.get("rate_limit")))
        headers[f"{family}-limit-name"] = entry["limit_name"]
    credits = usage_body.get("credits")
    if credits is not None:
        for key, flag in (("has-credits", "has_credits"), ("unlimited", "unlimited")):
            headers[f"x-codex-credits-{key}"] = "true" if credits[flag] else "false"
        if credits.get("balance") is not None:
            headers["x-codex-credits-balance"] = credits["balance"]
    if wall:
        headers["x-codex-active-limit"] = limit
        headers["x-codex-rate-limit-reached-type"] = reached
    return headers


def _bucket(usage_body, limit):
    """`limit`'s `RateLimitStatusDetails` in a usage reply: `rate_limit` for the `codex`
    bucket, else the additional limit whose metered feature it is."""
    if limit == "codex":
        return usage_body["rate_limit"]
    named = [entry.get("rate_limit") or {} for entry in usage_body.get("additional_rate_limits") or ()
             if entry["metered_feature"] == limit]
    _require(len(named) == 1, f"refusal: the account's usage names no limit {limit!r}")
    return named[0]


def refusal(usage_body, message, headers=None, *, limit="codex", reached=ORDINARY_REACHED,
            resets_at=None, names_reset=True):
    """The 429 a model request gets at the wall `usage_body` states.

    `{"status", "headers", "body"}`. The body is what codex-api's api_bridge.rs decodes
    as a usage limit (`UsageErrorResponse`: the type, the plan and the second the limit
    resets); the headers are `limit_headers` of the same reading, naming `limit` as the
    one reached and `reached` as why, unless `headers` gives others.

    At an ordinary wall (`reached` is rate_limit_reached) the wall is `limit`'s bucket,
    and `resets_at` is the reset of its spent window, the later one when both are spent,
    taken from the reading and not from the clock again: the body and the headers of one
    refusal name one instant. A bucket with no window that refuses ordinary use (a
    credit-only plan out of credits) names no reset. A bucket with windows and none spent
    has no wall to refuse a request at, which is a `FixtureFault`.

    At a workspace's reached type no window explains the wall, and the reset is
    `resets_at` as given, none when it is not.

    Without `names_reset` the body carries no `resets_at` at either kind of wall, which the
    decoder takes as a usage limit that names no reset; the headers still state the
    windows and their resets. A reset given beside it (`resets_at`) is a `FixtureFault`:
    the refusal would both name one and not.
    """
    _require(names_reset or resets_at is None, "refusal: a refusal that names no reset is given one")
    bucket = _bucket(usage_body, limit)
    if reached == ORDINARY_REACHED:
        _require(resets_at is None, "refusal: an ordinary wall's reset is its spent window's")
        windows = [snapshot for name in ("primary_window", "secondary_window")
                   if (snapshot := bucket.get(name)) is not None]
        spent = [snapshot["reset_at"] for snapshot in windows if snapshot["used_percent"] >= 100]
        _require(bool(spent) or (not windows and bucket.get("allowed") is False),
                 "refusal: the account's usage states no spent window")
        resets_at = max(spent) if spent else None
    error = {"type": "usage_limit_reached", "plan_type": usage_body["plan_type"], "message": message}
    if resets_at is not None and names_reset:
        error["resets_at"] = int(resets_at)
    return {"status": 429,
            "headers": limit_headers(usage_body, wall=True, limit=limit, reached=reached)
            if headers is None else headers,
            "body": {"error": error}}


def not_included(plan, message):
    """The 429 of a model request the account's plan does not include.

    codex-api's api_bridge.rs makes `CodexErr::UsageNotIncluded` of a `UsageErrorResponse`
    of type usage_not_included, and reads no header or reset for it.
    """
    return {"status": 429, "headers": {},
            "body": {"error": {"type": "usage_not_included", "plan_type": plan, "message": message}}}


class FixtureFault(ValueError):
    """A reply the fixture built that the client would not decode."""


def _require(condition, message):
    if not condition:
        raise FixtureFault(message)


def _integer(value, domain):
    # JSON true is not a number to serde, and Python would let it pass as 1.
    return type(value) is int and value in domain


def _text(value):
    return isinstance(value, str) and bool(value)


def _optional_text(value):
    return value is None or isinstance(value, str)


def _word(value, words):
    return isinstance(value, str) and value in words


def _status(limit, where, windows):
    """Check a `RateLimitStatusDetails` at `where`, its windows named after the prefix
    `windows`, and count its windows."""
    _require(isinstance(limit, dict), f"usage: {where} is not an object")
    for key in ("allowed", "limit_reached"):
        _require(type(limit.get(key)) is bool, f"usage: {where}.{key} is not a boolean")
    seen = 0
    for name in ("primary_window", "secondary_window"):
        snapshot = limit.get(name)
        if snapshot is None:
            continue
        # `RateLimitWindowSnapshot`: every field is a plain i32 with no default.
        _require(isinstance(snapshot, dict), f"usage: {windows}{name} is not an object")
        for key in ("used_percent", "limit_window_seconds", "reset_after_seconds", "reset_at"):
            _require(_integer(snapshot.get(key), I32),
                     f"usage: {windows}{name}.{key} is absent or not an i32")
        seen += 1
    return seen


def _credit_status(credits):
    # codex-backend-openapi-models `CreditStatusDetails`.
    _require(isinstance(credits, dict), "usage: credits is not an object")
    for key in ("has_credits", "unlimited"):
        _require(type(credits.get(key)) is bool, f"usage: credits.{key} is not a boolean")
    _require(_optional_text(credits.get("balance")), "usage: credits.balance is not text")
    for key in ("approx_local_messages", "approx_cloud_messages"):
        _require(credits.get(key) is None or isinstance(credits[key], list),
                 f"usage: credits.{key} is not a list")


def _spend_control(control):
    # `SpendControlStatusDetails` and its `SpendControlLimitDetails`.
    _require(isinstance(control, dict) and type(control.get("reached")) is bool,
             "usage: spend_control.reached is absent or not a boolean")
    limit = control.get("individual_limit")
    if limit is None:
        return
    where = "spend_control.individual_limit"
    _require(isinstance(limit, dict), f"usage: {where} is not an object")
    _require(_optional_text(limit.get("source")), f"usage: {where}.source is not text")
    for key in ("limit", "used", "remaining"):
        _require(isinstance(limit.get(key), str), f"usage: {where}.{key} is absent or not text")
    for key in ("used_percent", "remaining_percent", "reset_after_seconds", "reset_at"):
        _require(_integer(limit.get(key), I32), f"usage: {where}.{key} is absent or not an i32")


def _additional_limits(limits):
    # backend-client `RateLimitStatusWithResetCredits` owns `additional_rate_limits` on the
    # wire, as a list of `AdditionalRateLimitWithNormalModel`.
    _require(isinstance(limits, list), "usage: additional_rate_limits is not a list")
    for index, entry in enumerate(limits):
        where = f"additional_rate_limits[{index}]"
        _require(isinstance(entry, dict), f"usage: {where} is not an object")
        _require(_text(entry.get("limit_name")), f"usage: {where}.limit_name is absent or empty")
        # The client keeps metered_feature as the limit id unnormalized, and every ALab
        # reader of the ordinary bucket takes `codex` for it, so the fixture names another
        # limit only as a 429's active limit normalizes it.
        feature = entry.get("metered_feature")
        _require(isinstance(feature, str) and LIMIT_ID.fullmatch(feature) is not None
                 and feature != "codex",
                 f"usage: {where}.metered_feature is not a normalized limit id other than codex")
        _require(_optional_text(entry.get("normal_model_slug")),
                 f"usage: {where}.normal_model_slug is not text")
        if entry.get("rate_limit") is not None:
            _status(entry["rate_limit"], f"{where}.rate_limit", f"{where}.rate_limit.")


def _usage(body):
    # codex-backend-client `RateLimitStatusWithResetCredits`, which flattens
    # codex-backend-openapi-models `RateLimitStatusPayload`.
    _require(_word(body.get("plan_type"), PLAN_TYPES), "usage: plan_type is absent or not a PlanType")
    # Optional to the decoder. alab-quota-recovery `matches` then refuses a reading that
    # does not name the login's own account and user, so the fixture always names both.
    for key in ("account_id", "user_id"):
        _require(_text(body.get(key)), f"usage: {key} is absent or empty")
    # `RateLimitStatusDetails`. Optional to the decoder; a walled turn needs its verdict.
    limit = body.get("rate_limit")
    _require(isinstance(limit, dict), "usage: rate_limit is absent")
    seen = _status(limit, "rate_limit", "")
    # Each of these is optional, and null, to the decoder.
    credits = body.get("credits")
    if credits is not None:
        _credit_status(credits)
    # A reading states a window, or the credit balance a credit-only plan has instead.
    _require(seen > 0 or credits is not None, "usage: neither a usage window nor credits are present")
    if body.get("spend_control") is not None:
        _spend_control(body["spend_control"])
    if body.get("additional_rate_limits") is not None:
        _additional_limits(body["additional_rate_limits"])
    # `RateLimitReachedKind` decodes a word it does not know as `unknown`, which the
    # client then drops: the wall would read as an ordinary one whatever was meant.
    reached = body.get("rate_limit_reached_type")
    _require(reached is None or isinstance(reached, dict) and _word(reached.get("type"), REACHED_TYPES),
             "usage: rate_limit_reached_type.type is not a RateLimitReachedKind the client keeps")
    # `RateLimitResetCreditsSummary`, optional as a whole.
    summary = body.get("rate_limit_reset_credits")
    if summary is not None:
        _require(isinstance(summary, dict) and _integer(summary.get("available_count"), I64),
                 "usage: rate_limit_reset_credits.available_count is absent or not an integer")


def _credits(body):
    # codex-backend-client `RateLimitResetCreditsDetails`.
    _require(_integer(body.get("available_count"), I64), "credits: available_count is absent or not an integer")
    _require(isinstance(body.get("credits"), list), "credits: credits is absent or not a list")
    for credit in body["credits"]:
        # `RateLimitResetCreditDetails`.
        _require(isinstance(credit, dict), "credits: an entry is not an object")
        for key in ("id", "reset_type", "status", "granted_at"):
            _require(isinstance(credit.get(key), str), f"credits: an entry's {key} is absent or not text")
        for key in ("expires_at", "title", "description"):
            _require(credit.get(key) is None or isinstance(credit[key], str),
                     f"credits: an entry's {key} is not text")


def _consume(body):
    # codex-backend-client `ConsumeRateLimitResetCreditResponse`.
    _require(body.get("code") in CONSUME_CODES, "consume: code is absent or not a ConsumeRateLimitResetCreditCode")
    _require("windows_reset" not in body or _integer(body["windows_reset"], I64),
             "consume: windows_reset is not an integer")


def _number(text, kind):
    try:
        return kind(text)
    except (TypeError, ValueError):
        return None


def _error(reply, kind):
    """The error object and headers of a 429 whose body codex-api decodes as a
    `UsageErrorResponse` of type `kind`."""
    _require(type(reply.get("status")) is int and reply["status"] == 429, "refusal: the status is not 429")
    error = reply["body"].get("error") if isinstance(reply.get("body"), dict) else None
    _require(isinstance(error, dict), "refusal: the body has no error object")
    _require(error.get("type") == kind, f"refusal: error.type is not {kind}")
    # Each optional to the decoder; one of the wrong type makes the whole body undecodable,
    # and the refusal is then an ordinary retried 429.
    _require("plan_type" not in error or _word(error["plan_type"], PLAN_TYPES),
             "refusal: error.plan_type is not a PlanType")
    _require("resets_at" not in error or _integer(error["resets_at"], I64),
             "refusal: error.resets_at is not an integer")
    _require("code" not in error or isinstance(error["code"], str), "refusal: error.code is not text")
    headers = reply.get("headers")
    _require(isinstance(headers, dict) and all(
        isinstance(key, str) and key == key.lower() and isinstance(value, str)
        for key, value in headers.items()), "refusal: the headers are not lower-case names with text values")
    return error, headers


def _refusal(reply):
    # What codex-api `map_api_error` makes `CodexErr::UsageLimitReached` of: a 429 whose
    # body is a `UsageErrorResponse` of type usage_limit_reached.
    error, headers = _error(reply, "usage_limit_reached")
    # `RateLimitReachedType`: a word it does not know is dropped, and the wall then reads
    # as an ordinary one whatever was meant.
    reached = headers.get("x-codex-rate-limit-reached-type")
    _require(reached is None or reached in REACHED_TYPES,
             "refusal: x-codex-rate-limit-reached-type is not a RateLimitReachedType")
    # The limit named as reached: the client reads that limit's header family alone.
    active = headers.get("x-codex-active-limit", "codex")
    _require(LIMIT_ID.fullmatch(active) is not None,
             "refusal: x-codex-active-limit is not a normalized limit id")
    windows = []
    for key, used in headers.items():
        match = WINDOW_HEADER.fullmatch(key)
        if match is None:
            continue
        prefix, name = match.groups()
        # codex-api rate_limits.rs: the percent is an f64, the other two are i64.
        percent = _number(used, float)
        _require(percent is not None and math.isfinite(percent), f"refusal: {key} is not a finite number")
        for part in ("window-minutes", "reset-at"):
            _require(_integer(_number(headers.get(f"{prefix}-{name}-{part}"), int), I64),
                     f"refusal: {prefix}-{name}-{part} is absent or not an integer")
        windows.append((prefix, percent, int(headers[f"{prefix}-{name}-reset-at"])))
    # A blank limit name is dropped, and the snapshot then names none.
    for key, value in headers.items():
        _require(not key.endswith("-limit-name") or bool(value.strip()), f"refusal: {key} is blank")
    # `parse_credits_snapshot` reads both flags or no credits at all.
    flags = [headers.get(f"x-codex-credits-{name}") for name in ("has-credits", "unlimited")]
    if any(flag is not None for flag in flags) or "x-codex-credits-balance" in headers:
        _require(all(flag in ("true", "false") for flag in flags),
                 "refusal: x-codex-credits-has-credits and -unlimited are not both true or false")
    if windows:
        named = [(percent, reset) for prefix, percent, reset in windows
                 if prefix == header_family(active)]
        _require(bool(named), "refusal: x-codex-active-limit names a limit whose windows are not sent")
        # At an ordinary wall the named limit's spent window is the wall. A body may name
        # no reset at all; one it names is that window's, as the headers state it. A
        # workspace's reached type is no window's.
        if reached in (None, ORDINARY_REACHED):
            spent = [reset for percent, reset in named if percent >= 100]
            _require(bool(spent), "refusal: the windows it states are not spent")
            _require("resets_at" not in error or error["resets_at"] == max(spent),
                     "refusal: error.resets_at is not the spent window's reset")


def _entitlement(reply):
    # What codex-api `map_api_error` makes `CodexErr::UsageNotIncluded` of: a 429 whose
    # body is a `UsageErrorResponse` of type usage_not_included. It reads no header for it.
    _error(reply, "usage_not_included")


CHECKS = {"usage": _usage, "credits": _credits, "consume": _consume, "refusal": _refusal,
          "entitlement": _entitlement}


def check(kind, body):
    """Raise `FixtureFault` unless the client would decode `body` as a `kind` reply."""
    _require(kind in CHECKS, f"unknown reply kind {kind!r}")
    _require(isinstance(body, dict), f"{kind}: the reply is not a JSON object")
    CHECKS[kind](body)
    return body


# The clock the golden files are written with. The Rust test moves each window's
# `reset_at` to its own present, keeping the distance `reset_after_seconds` states.
GOLDEN_NOW = 1_800_000_000
GOLDEN_DIR = Path(__file__).resolve().parent / "fixtures" / "provider"
# The rotation fixture's source and target logins (renewal_follow.QUOTA_ACCOUNTS).
GOLDEN_SOURCE = ("source-account", "fixture-user")
GOLDEN_TARGET = ("target-account", "target-user")
# The plans of the walls only a workspace or a credit-only plan has (`PlanType` words).
GOLDEN_WORKSPACE_PLAN = "team"
GOLDEN_CREDIT_PLAN = "self_serve_business_usage_based"
# A member's spend cap resets with its billing period, far beyond any usage window.
SPEND_RESET_SECONDS = 20 * 24 * 3600
# The limit that is not the ordinary bucket: upstream's own example, a model's limit.
GOLDEN_MODEL_LIMIT = "codex_other"


def golden_usage():
    """Each golden usage reading's file name and reply, unchecked."""
    now = GOLDEN_NOW
    # A walled account whose windows do not explain its wall: neither is spent.
    unspent = dict(week_used=10, session_used=20, allowed=False, now=now)
    readings = {
        # The source login after its weekly wall: what ends a walled turn for rotation.
        "usage-weekly-wall.json": usage(*GOLDEN_SOURCE, week_used=100, allowed=False, now=now),
        # The destination login: what a turn-start preflight must find permitting.
        "usage-healthy.json": usage(*GOLDEN_TARGET, week_used=0, allowed=True, now=now),
        # A five-hour wall that resets in ten minutes, on a usable week.
        "usage-five-hour-wall.json": usage(
            *GOLDEN_SOURCE, week_used=10, session_used=100, allowed=False,
            session_resets_in=600, now=now),
        # An ordinary five-hour wall five minutes from its reset whose reading also names
        # the ordinary reached type: still the ordinary wall, not a workspace's.
        "usage-five-hour-reached.json": usage(
            *GOLDEN_SOURCE, week_used=10, session_used=100, allowed=False,
            session_resets_in=300, now=now),
        # A member's spend cap is reached; no window is spent.
        "usage-spend-control.json": usage(*GOLDEN_SOURCE, plan=GOLDEN_WORKSPACE_PLAN, **unspent),
        # The workspace is out of credits; no window is spent.
        "usage-workspace-credits.json": usage(*GOLDEN_SOURCE, plan=GOLDEN_WORKSPACE_PLAN, **unspent),
        # A credit-only plan out of credits: no window at all.
        "usage-credit-only.json": credit_only_usage(*GOLDEN_SOURCE, plan=GOLDEN_CREDIT_PLAN),
        # A model's own limit is spent while the ordinary bucket has room.
        "usage-model-limit.json": usage(
            *GOLDEN_SOURCE, week_used=10, session_used=20, allowed=True, now=now),
    }
    readings["usage-five-hour-reached.json"]["rate_limit_reached_type"] = reached_type(ORDINARY_REACHED)
    readings["usage-spend-control.json"]["spend_control"] = spend_control(
        limit=25000, used=25000, resets_in=SPEND_RESET_SECONDS, now=now)
    readings["usage-workspace-credits.json"]["rate_limit_reached_type"] = reached_type(
        "workspace_member_credits_depleted")
    readings["usage-model-limit.json"]["additional_rate_limits"] = [additional_limit(
        GOLDEN_MODEL_LIMIT, GOLDEN_MODEL_LIMIT, allowed=False,
        primary=window(100, SESSION_WINDOW_SECONDS, 7200, now),
        secondary=window(40, WEEK_WINDOW_SECONDS, 3 * 86400, now))]
    return readings


def golden():
    """Each golden file's name and reply."""
    readings = golden_usage()
    replies = {name: check("usage", body) for name, body in readings.items()}
    spend_reset = readings["usage-spend-control.json"]["spend_control"]["individual_limit"]["reset_at"]
    replies.update({
        "credits-empty.json": check("credits", credit_list()),
        "consume-no-credit.json": check("consume", consume_reply()),
        # The model request refused at each wall: status, headers and body.
        "refusal-weekly-wall.json": check("refusal", refusal(
            readings["usage-weekly-wall.json"], "Fixture weekly quota exhausted")),
        "refusal-five-hour-wall.json": check("refusal", refusal(
            readings["usage-five-hour-wall.json"], "Fixture five-hour limit reached")),
        "refusal-workspace-credits.json": check("refusal", refusal(
            readings["usage-workspace-credits.json"], "Fixture workspace credits depleted",
            reached="workspace_member_credits_depleted")),
        # The cap names its own reset: nothing but the billing period ends it.
        "refusal-spend-cap.json": check("refusal", refusal(
            readings["usage-spend-control.json"], "Fixture spend cap reached",
            reached="workspace_member_usage_limit_reached", resets_at=spend_reset)),
        "refusal-credit-only.json": check("refusal", refusal(
            readings["usage-credit-only.json"], "Fixture credits spent")),
        "refusal-model-limit.json": check("refusal", refusal(
            readings["usage-model-limit.json"], "Fixture model limit reached",
            limit=GOLDEN_MODEL_LIMIT)),
        "refusal-reserve.json": check("refusal", refusal(
            reserve_usage(*GOLDEN_SOURCE, now=GOLDEN_NOW), "Fixture reserve limit reached",
            limit="base_model_inference")),
        "refusal-usage-not-included.json": check("entitlement", not_included(
            GOLDEN_WORKSPACE_PLAN, "Fixture usage not included")),
    })
    return replies


def render(body):
    return json.dumps(body, indent=2, sort_keys=True) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--write-golden", required=True, type=Path, metavar="DIR")
    args = parser.parse_args()
    args.write_golden.mkdir(parents=True, exist_ok=True)
    for name, body in golden().items():
        (args.write_golden / name).write_text(render(body))
        print(args.write_golden / name)
    return 0


if __name__ == "__main__":
    sys.exit(main())
