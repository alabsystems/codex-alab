#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The provider replies the rotation smokes serve, in the shape the real client decodes.

One place builds the usage, earned-reset list and redemption replies and the refusal of a
model request at a wall, and `check` refuses
a reply whose required fields are missing or mistyped. The client decodes these replies
strictly: a usage window without `reset_after_seconds` fails the whole read, the client
treats that as "usage unknown", and a walled turn then polls until the smoke's hang
detector fires. `check` names, for each reply, the Rust struct that decides its shape.

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


def credit_list():
    """The reply to GET .../rate-limit-reset-credits for an account with no earned reset."""
    return {"available_count": 0, "credits": []}


def consume_reply():
    """The reply to POST .../rate-limit-reset-credits/consume when nothing can be redeemed."""
    return {"code": "no_credit", "windows_reset": 0}


def limit_headers(usage_body, wall=False):
    """The rate-limit headers a model response carries for the account `usage_body` is for.

    The provider reports an account's windows on every model response as well as in its
    usage reply, and the two agree. These are the names codex-api's rate_limits.rs reads
    (`x-codex-<window>-used-percent`, `-window-minutes`, `-reset-at`); a refusal also names
    the limit that was reached and why, as api_bridge.rs reads them.
    """
    headers = {}
    for name in ("primary", "secondary"):
        snapshot = usage_body["rate_limit"].get(name + "_window")
        if snapshot is None:
            continue
        headers[f"x-codex-{name}-used-percent"] = str(snapshot["used_percent"])
        headers[f"x-codex-{name}-window-minutes"] = str(snapshot["limit_window_seconds"] // 60)
        headers[f"x-codex-{name}-reset-at"] = str(snapshot["reset_at"])
    if wall:
        headers["x-codex-active-limit"] = "codex"
        headers["x-codex-rate-limit-reached-type"] = "rate_limit_reached"
    return headers


def refusal(usage_body, message, headers=None):
    """The 429 a model request gets at the wall `usage_body` states.

    `{"status", "headers", "body"}`. The body is what codex-api's api_bridge.rs decodes
    as a usage limit (`UsageErrorResponse`: the type, the plan and the second the limit
    resets); the headers are `limit_headers` of the same reading unless `headers` gives
    others. `resets_at` is the reset of the spent window, the later one when both are
    spent, taken from the reading and not from the clock again: the body and the headers
    of one refusal name one instant. A reading with no spent window has no wall to refuse
    a request at, which is a `FixtureFault`.
    """
    spent = [snapshot["reset_at"] for name in ("primary_window", "secondary_window")
             if (snapshot := usage_body["rate_limit"].get(name)) is not None
             and snapshot["used_percent"] >= 100]
    _require(bool(spent), "refusal: the account's usage states no spent window")
    return {"status": 429,
            "headers": limit_headers(usage_body, wall=True) if headers is None else headers,
            "body": {"error": {"type": "usage_limit_reached", "plan_type": usage_body["plan_type"],
                               "resets_at": max(spent), "message": message}}}


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


def _usage(body):
    # codex-backend-client `RateLimitStatusWithResetCredits`, which flattens
    # codex-backend-openapi-models `RateLimitStatusPayload`.
    _require(body.get("plan_type") in PLAN_TYPES, "usage: plan_type is absent or not a PlanType")
    # Optional to the decoder. alab-quota-recovery `matches` then refuses a reading that
    # does not name the login's own account and user, so the fixture always names both.
    for key in ("account_id", "user_id"):
        _require(_text(body.get(key)), f"usage: {key} is absent or empty")
    # `RateLimitStatusDetails`. Optional to the decoder; a walled turn needs its verdict.
    limit = body.get("rate_limit")
    _require(isinstance(limit, dict), "usage: rate_limit is absent")
    for key in ("allowed", "limit_reached"):
        _require(type(limit.get(key)) is bool, f"usage: rate_limit.{key} is not a boolean")
    seen = 0
    for name in ("primary_window", "secondary_window"):
        snapshot = limit.get(name)
        if snapshot is None:
            continue
        # `RateLimitWindowSnapshot`: every field is a plain i32 with no default.
        _require(isinstance(snapshot, dict), f"usage: {name} is not an object")
        for key in ("used_percent", "limit_window_seconds", "reset_after_seconds", "reset_at"):
            _require(_integer(snapshot.get(key), I32), f"usage: {name}.{key} is absent or not an i32")
        seen += 1
    _require(seen > 0, "usage: no usage window is present")
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


def _refusal(reply):
    # What codex-api `map_api_error` makes `CodexErr::UsageLimitReached` of: a 429 whose
    # body is a `UsageErrorResponse` of type usage_limit_reached.
    _require(type(reply.get("status")) is int and reply["status"] == 429, "refusal: the status is not 429")
    error = reply["body"].get("error") if isinstance(reply.get("body"), dict) else None
    _require(isinstance(error, dict), "refusal: the body has no error object")
    _require(error.get("type") == "usage_limit_reached", "refusal: error.type is not usage_limit_reached")
    # Each optional to the decoder; one of the wrong type makes the whole body undecodable,
    # and the refusal is then an ordinary retried 429.
    _require("plan_type" not in error or error["plan_type"] in PLAN_TYPES,
             "refusal: error.plan_type is not a PlanType")
    _require("resets_at" not in error or _integer(error["resets_at"], I64),
             "refusal: error.resets_at is not an integer")
    headers = reply.get("headers")
    _require(isinstance(headers, dict) and all(
        isinstance(key, str) and key == key.lower() and isinstance(value, str)
        for key, value in headers.items()), "refusal: the headers are not lower-case names with text values")
    # `RateLimitReachedType`: a word it does not know is dropped, and the wall then reads
    # as an ordinary one whatever was meant.
    reached = headers.get("x-codex-rate-limit-reached-type")
    _require(reached is None or reached in REACHED_TYPES,
             "refusal: x-codex-rate-limit-reached-type is not a RateLimitReachedType")
    spent = []
    for name in ("primary", "secondary"):
        used = headers.get(f"x-codex-{name}-used-percent")
        if used is None:
            continue
        # codex-api rate_limits.rs: the percent is an f64, the other two are i64.
        percent = _number(used, float)
        _require(percent is not None and math.isfinite(percent),
                 f"refusal: x-codex-{name}-used-percent is not a finite number")
        for part in ("window-minutes", "reset-at"):
            _require(_integer(_number(headers.get(f"x-codex-{name}-{part}"), int), I64),
                     f"refusal: x-codex-{name}-{part} is absent or not an integer")
        if percent >= 100:
            spent.append(int(headers[f"x-codex-{name}-reset-at"]))
    if any(key.endswith("-used-percent") for key in headers):
        # The windows are read from the header family of the limit named as reached.
        _require(headers.get("x-codex-active-limit", "codex") == "codex",
                 "refusal: x-codex-active-limit names a limit whose windows are not sent")
        _require(bool(spent), "refusal: the windows it states are not spent")
        _require(error.get("resets_at") == max(spent),
                 "refusal: error.resets_at is not the spent window's reset")


CHECKS = {"usage": _usage, "credits": _credits, "consume": _consume, "refusal": _refusal}


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


def golden():
    """Each golden file's name and reply."""
    return {
        # The source login after its weekly wall: what ends a walled turn for rotation.
        "usage-weekly-wall.json": check("usage", usage(
            *GOLDEN_SOURCE, week_used=100, allowed=False, now=GOLDEN_NOW)),
        # The destination login: what a turn-start preflight must find permitting.
        "usage-healthy.json": check("usage", usage(
            *GOLDEN_TARGET, week_used=0, allowed=True, now=GOLDEN_NOW)),
        # A five-hour wall that resets in ten minutes, on a usable week.
        "usage-five-hour-wall.json": check("usage", usage(
            *GOLDEN_SOURCE, week_used=10, session_used=100, allowed=False,
            session_resets_in=600, now=GOLDEN_NOW)),
        "credits-empty.json": check("credits", credit_list()),
        "consume-no-credit.json": check("consume", consume_reply()),
        # The model request refused at each wall: status, headers and body.
        "refusal-weekly-wall.json": check("refusal", refusal(usage(
            *GOLDEN_SOURCE, week_used=100, allowed=False, now=GOLDEN_NOW),
            "Fixture weekly quota exhausted")),
        "refusal-five-hour-wall.json": check("refusal", refusal(usage(
            *GOLDEN_SOURCE, week_used=10, session_used=100, allowed=False,
            session_resets_in=600, now=GOLDEN_NOW), "Fixture five-hour limit reached")),
    }


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
