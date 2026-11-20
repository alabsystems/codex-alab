#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise automatic weekly reset credits with fake ChatGPT auth and loopback only.

Builds nothing. Supply --codex /absolute/binary or --package /absolute/package.
All cases retain commands, guardian outcomes, native events and HTTP traces.
Backend requests require the fixture bearer and account headers; traces retain
only presence and match booleans for those headers. Automatic spending requires
a weekly window at >=99% with more than 24 hours until its natural refresh.
"""
import argparse
import base64
import datetime
import hashlib
import http.server
import json
import math
import os
from pathlib import Path
import re
import select
import signal
import sys
import threading
import time

# The protected runtime probe starts this mode with Python isolation enabled.
# It must exec its child before importing adjacent fixture modules, which are
# deliberately unavailable on the isolated module search path.
if __name__ == "__main__" and len(sys.argv) > 1 and sys.argv[1] == "--exec-wrapper":
    with Path(sys.argv[2]).open("x") as pidfile:
        pidfile.write(str(os.getpid()))
    os.execv(sys.argv[3], sys.argv[3:])

import runpy

import provider_fixture
from capacity_exec import case_error_fields, check_package_executable, cleanup_case, package_records, run as guarded_run, start_listener, write_json

CLI = runpy.run_path(str(Path(__file__).resolve().parents[1] / "cli_status.py"))
FILE_HASH = runpy.run_path(str(Path(__file__).resolve().parents[1] / "file_hash.py"))

MODEL = "gpt-5"
PROMPT = "Keep this weekly reset smoke task and its completed plan."
CALL_ID = "weekly-reset-plan"
PLAN = "Completed work survives an earned weekly reset"
PLAN2 = "Additional completed work survives another reset in the same turn"
CALL_ID2 = "weekly-reset-second-plan"
ANSWER = "Weekly reset recovery succeeded."
ACCOUNT = "weekly-smoke-account"
USER = "weekly-smoke-user"
COMMAND_TIMEOUT_SECONDS = 120
OUTAGE_CASES = ("usage-outage", "credit-list-outage")
DEADLINE_OUTAGE_CASES = ("usage-outage-deadline", "credit-list-outage-deadline")
PROPAGATION_CASES = ("propagating-reset", "lost-ack-propagation")
TWO_CREDITS = ("stale-allowance-credit", *PROPAGATION_CASES, "repeated-same-cycle", "stale-credit-ledger", "selected-no-credit")
TWO_TOOLS = ("shared-credit", "repeated-same-cycle", "stale-credit-ledger", "unused-pending")
PREPOST_NEGATIVE_CASES = ("prepost-recovery-shifted", "prepost-recovery-foreign", "prepost-recovery-below-99")
PREPOST_CASES = ("prepost-recovery-restored", *PREPOST_NEGATIVE_CASES,
                 "prepost-recovery-denied", "prepost-preflight-99")
PREFLIGHT_CASES = ("preflight-weekly", "preflight-beyond-24h", "preflight-weekly-25h", "preflight-weekly-48h", "preflight-weekly-49h", "prepost-preflight-99")
POLICY_PREFLIGHT_CASES = ("preflight-five-hour", "preflight-weekly-24h", "preflight-below-99", "preflight-elapsed-reset", "preflight-mixed-windows")
POLICY_BLOCKED_CASES = (*POLICY_PREFLIGHT_CASES, "recovery-weekly-24h", "near-reset", "recovery-below-99", "recovery-elapsed-reset")
POLICY_POLL_CASES = ("preflight-five-hour", "preflight-weekly-24h", "recovery-weekly-24h", "near-reset", "preflight-elapsed-reset", "preflight-mixed-windows", "recovery-elapsed-reset")
PAID_RETRY_CASES = ("existing-usage-credits", "paid-retry-refused", "spend-control-reopens")
FUNDED_CASES = (*PAID_RETRY_CASES, "plan-renewed-before-credit-retry", "spend-control-blocked")
SUCCESS_CASES = ("existing-usage-credits", "spend-control-reopens", "plan-renewed-before-credit-retry", "recovery-beyond-24h", "recovery-weekly-25h", "recovery-weekly-48h", "recovery-weekly-49h", "prepost-recovery-restored", "prepost-recovery-denied", "spent-retry-deadline", *DEADLINE_OUTAGE_CASES, "stale-allowance-credit", *OUTAGE_CASES, "shared-credit", "natural-reset", "committed-ack", "stale-reset-credit", *PROPAGATION_CASES, "late-credit", "recovery", "lost-ack", "pending-old-cycle", "repeated-same-cycle", "unused-pending", "selected-no-credit", *PREFLIGHT_CASES)
CASE_NAMES = (*SUCCESS_CASES, "paid-retry-refused", "spend-control-blocked", *POLICY_BLOCKED_CASES, *PREPOST_NEGATIVE_CASES, "stale-credit-ledger", "stale-restored", "identity-mismatch", "no-credit", "unrelated-provider")
NOTICE_SUCCESS_CASES = ("recovery", "preflight-weekly", "preflight-weekly-25h", "preflight-weekly-48h", "preflight-weekly-49h", "recovery-weekly-25h", "recovery-weekly-48h", "recovery-weekly-49h")
NOTICE_PRESERVE_CASES = ("preflight-weekly-24h", "recovery-weekly-24h")
RESET_NOTICE_PREFIX = "[codex-alab reset] "
CREDIT_NOTICE_PREFIX = "[codex-alab credits] "

def decision_prefix(name):
    return CREDIT_NOTICE_PREFIX if name in (*PAID_RETRY_CASES, "spend-control-blocked") else RESET_NOTICE_PREFIX

def require(condition, message):
    if not condition:
        raise RuntimeError(message)

def has_paced_poll(reads):
    return any(second["at"] - first["at"] >= 29 for first, second in zip(reads, reads[1:]))

def notice_messages(events, prefix):
    """Exec JSON represents runtime warnings as nonfatal completed error items."""
    messages = []
    for event in events:
        item = event.get("item")
        if (event.get("type") == "item.completed" and isinstance(item, dict)
                and item.get("type") == "error" and isinstance(item.get("message"), str)
                and item["message"].startswith(prefix)):
            messages.append(item["message"][len(prefix):])
    return messages

def reset_expiry_label(value):
    if value is None:
        return "expiry not provided"
    invalid = "unknown expiry (invalid server timestamp)"
    match = re.fullmatch(r"(\d{4}-\d{2}-\d{2})[Tt ](\d{2}:\d{2}):(\d{2})(?:\.(\d+))?"
                         r"([Zz]|[+-](?:[01]\d|2[0-3]):[0-5]\d)", value, re.ASCII)
    if not match:
        return invalid
    date, minute, second, fraction, offset = match.groups()
    offset = "+00:00" if offset.upper() == "Z" else offset
    # datetime handles calendar and offset arithmetic at whole-second precision.
    # Preserve RFC 3339 leap seconds and the native nanosecond precision separately.
    try:
        timestamp = datetime.datetime.fromisoformat(
            f"{date}T{minute}:{'59' if second == '60' else second}{offset}")
        utc = timestamp.astimezone(datetime.timezone.utc).replace(tzinfo=None)
    except (ValueError, OverflowError):
        return invalid
    digits = (fraction or "")[:9].ljust(9, "0")
    while digits.endswith("000"):
        digits = digits[:-3]
    fraction = "." + digits if digits else ""
    return f"{utc.isoformat(timespec='minutes')}:{second}{fraction}+00:00"

def reset_listings_with_weekly_deadlines(requests):
    """Pair each inventory with the ordinary usage read that preceded it."""
    deadline = None
    for request in requests:
        path = request.get("path", "")
        response = request.get("response") or {}
        if path.endswith("/usage"):
            limit = response.get("rate_limit") or {}
            windows = (limit.get("primary_window"), limit.get("secondary_window"))
            deadline = max((w["reset_at"] for w in windows if w and w.get("limit_window_seconds") == 604800
                            and type(w.get("used_percent")) in (float, int) and w["used_percent"] >= 99
                            and type(w.get("reset_at")) is int), default=None)
        elif path.endswith("/rate-limit-reset-credits") and "available_count" in response:
            yield response, deadline


def check_creditless_wait(messages, requests):
    """Silence is expected after the fixture confirms both funding sources are empty."""
    wall = next((i for i, r in enumerate(requests) if r["path"] == "/v1/responses" and "quotaResetUnix" in r), None)
    require(wall is not None, "quiet wait requires the original quota wall")
    usage_seen = False
    empty_lists = 0
    for request in requests[wall + 1:]:
        path = request["path"]
        response = request.get("response") or {}
        if path.endswith("/usage"):
            require((response.get("rate_limit") or {}).get("allowed") is False,
                    "quiet wait requires exhausted plan allowance")
            credits = response.get("credits") or {}
            require(credits.get("has_credits") is False and credits.get("unlimited") is False
                    and credits.get("balance") == "0",
                    "quiet wait fixture must report its zero paid credit balance and no spending access")
            usage_seen = True
        elif path.endswith("/rate-limit-reset-credits"):
            require(usage_seen, "quiet wait requires a post-wall usage reading before listing earned resets")
            require(response.get("available_count") == 0 and response.get("credits") == [],
                    "quiet wait requires repeated empty earned-reset inventories")
            empty_lists += 1
    require(empty_lists >= 2, "quiet wait requires repeated empty earned-reset inventories")
    require(not messages, f"empty earned-reset checks printed a reset notice: {messages[:1]}")


def spending_open(read):
    """Evidence that the funded fixture permits a paid retry on this reading."""
    control = read.get("spend_control")
    if not isinstance(control, dict) or control.get("reached") is not False or read.get("rate_limit_upsell") is not None:
        return False
    reason = read.get("rate_limit_reached_type")
    if reason is not None and (not isinstance(reason, dict) or reason.get("type") != "rate_limit_reached"):
        return False
    limit = control.get("individual_limit")
    return limit is None or (isinstance(limit, dict) and type(limit.get("remaining_percent")) is int
                             and limit["remaining_percent"] > 0)


def check_reset_notice_events(name, events, requests):
    """Qualify representative printed decisions through the production JSON renderer."""
    if name not in ("no-credit", *FUNDED_CASES, *NOTICE_SUCCESS_CASES, *NOTICE_PRESERVE_CASES):
        return
    require(not any(event.get("type") == "error"
                    and str(event.get("message", "")).startswith((RESET_NOTICE_PREFIX, CREDIT_NOTICE_PREFIX))
                    for event in events), "recovery notice became a fatal JSON error")
    prefix = decision_prefix(name)
    other = RESET_NOTICE_PREFIX if prefix == CREDIT_NOTICE_PREFIX else CREDIT_NOTICE_PREFIX
    require(not notice_messages(events, other), "unexpected recovery notice label")
    messages = notice_messages(events, prefix)
    if name in (*FUNDED_CASES, "no-credit"):
        for request in requests:
            if request["path"].endswith("/usage"):
                read = request.get("response") or {}
                require(read.get("account_id") == ACCOUNT and read.get("user_id") == USER,
                        "credit decision requires usage returned for the fixture account and user")
    if name == "spend-control-blocked":
        models = [r for r in requests if r["path"] == "/v1/responses"]
        require(len(models) == 2 and "quotaResetUnix" in models[-1], "spending block must retain the original wall")
        usage = [r for r in requests if r["path"].endswith("/usage")]
        require(all(r["response"]["credits"]["balance"] == "30966.04065"
                    and r["response"]["credits"]["has_credits"] is True
                    and r["response"]["spend_control"]["reached"] is True for r in usage)
                and any(r["at"] < models[0]["at"] and r["response"]["rate_limit"]["allowed"] is True for r in usage)
                and any(r["at"] > models[1]["at"] and r["response"]["rate_limit"]["allowed"] is False for r in usage),
                "spending block must be confirmed before sampling and after the wall with credits available")
        require(not any("/rate-limit-reset-credits" in r["path"] for r in requests),
                "a spending restriction triggered an earned-reset request")
        require(messages == ["Credit retry skipped. The account spending limit was reached."],
                "expected exactly one notice naming the spending restriction")
        plan = next((i for i, e in enumerate(events)
                     if e.get("type") == "item.started" and e.get("item", {}).get("type") == "todo_list"), None)
        wait = next((i for i, e in enumerate(events)
                     if e.get("type") == "error" and e.get("message", "").startswith("Waiting for account quota")), None)
        require(plan is not None and wait is not None and notice_messages(events[plan + 1:wait], prefix) == messages,
                "expected the restriction notice after tool work and before the quota wait")
        return
    if name in PAID_RETRY_CASES:
        models = [request for request in requests if request["path"] == "/v1/responses"]
        require(len(models) == 3 and "quotaResetUnix" in models[1], "paid retry must follow the original quota wall")
        checks = [request for request in requests if models[1]["at"] < request["at"] < models[2]["at"]]
        resets = [request for request in checks if request["path"].endswith("/rate-limit-reset-credits")]
        reopening = name == "spend-control-reopens"
        require(len(resets) == (2 if reopening else 1)
                and all(r.get("response") == {"available_count": 0, "credits": []} for r in resets),
                "paid retry must recheck the account after listing its empty earned resets")
        usage = [request for request in checks if request["path"].endswith("/usage")]
        before = [r["response"] for r in usage if r["at"] < resets[-1]["at"]]
        after = [r["response"] for r in usage if r["at"] > resets[-1]["at"]]
        require(before and after and all(read.get("credits", {}).get("has_credits") is True
                and read.get("rate_limit", {}).get("allowed") is False for read in before + after),
                "credit retry requires a fresh balance while the plan allowance remains exhausted")
        if reopening:
            require(all(read["credits"].get("balance") == "30966.04065" for read in before),
                    "the positive balance must remain unchanged while spending is blocked and reopened")
            initial = [r for r in usage if r["at"] < resets[0]["at"]]
            between = [r for r in usage if resets[0]["at"] < r["at"] < resets[1]["at"]]
            require(initial and all(spending_open(r["response"]) for r in initial)
                    and between and between[0]["response"]["spend_control"]["reached"] is True
                    and between[0]["at"] - resets[0]["at"] < 29,
                    "the immediate post-list reading must observe the new spending block")
            require(len(between) >= 2 and between[1]["at"] - between[0]["at"] >= 29
                    and all(spending_open(r["response"]) for r in between[1:]),
                    "a paced account poll must confirm spending reopened before selecting again")
        require(all(spending_open(read) for read in after),
                "a blocked reading cannot authorize a paid retry")
        balance = after[0]["credits"].get("balance")
        require(isinstance(balance, str) and balance != before[-1]["credits"].get("balance"),
                "fixture must change the reported balance during the reset listing")
        require(messages == [f"Retrying this request once with usage credits (reported balance {balance})."],
                "paid retry must report the authorizing balance in one notice")
        if name == "paid-retry-refused":
            retry = models[2]
            require("quotaResetUnix" in retry, "provider must refuse the paid retry")
            later = [r for r in requests if r["path"].endswith("/usage") and r["at"] > retry["at"]]
            require(has_paced_poll(later)
                    and all(r["response"]["credits"]["has_credits"] is True
                            and r["response"]["rate_limit"]["allowed"] is False for r in later),
                    "refused retry must retain the funded quota wall through a second paced poll")
        return
    if name == "plan-renewed-before-credit-retry":
        require(messages == ["Allowance restored; continuing the original request without spending another reset."],
                "renewed allowance must report one ordinary retry without a paid-retry notice")
        return
    if name == "no-credit":
        check_creditless_wait(messages, requests)
        return
    listings = list(reset_listings_with_weekly_deadlines(requests))
    require(listings, "reset notice fixture never listed earned credits")
    expected = []
    for listing, _ in listings:
        expiries = sorted(reset_expiry_label(credit.get("expires_at")) for credit in listing.get("credits", [])
                          if credit.get("status") == "available" and credit.get("reset_type") == "codex_rate_limits")
        if not expected:
            require(listing["available_count"] > 0 and any(expiry.endswith("+00:00") for expiry in expiries),
                    "reset notice fixture must offer credits with explicit expiry")
        expected.append(f'Considered earned usage reset: {max(listing["available_count"], 0)} available; '
                        f'expirations: {", ".join(expiries) if expiries else "none provided"}.')
    inventories, decisions, applied, restorations = [], [], [], []
    listing_index = 0
    last_inventory = None
    spending = name in NOTICE_SUCCESS_CASES
    informational_inventory = (
        "Considered earned usage reset: available count and expiry unknown because authentication changed or is unavailable. No reset used.",
        "Considered earned usage reset: allowance already restored; no new reset needed, so available count and expiry were not queried.",
        "Considered earned usage reset: available count and expiry are not re-listed while reconciling an earlier reset. Retaining its operation key to avoid spending twice.",
        "Considered earned usage reset: available count and expiry unknown because the reset list could not be read. No new reset will be used until availability is confirmed.",
    )
    for index, message in enumerate(messages):
        inventory_prefix = "Considered earned usage reset:"
        if message in informational_inventory:
            last_inventory = None
        elif message.startswith(inventory_prefix):
            require(message != last_inventory, "printed reset notice repeated an unchanged inventory")
            match = next((position for position in range(listing_index, len(expected))
                          if message == expected[position]), None)
            require(match is not None,
                    "printed reset notice lost the backend's available count or expiration timestamps")
            require(inventories or match == 0,
                    "printed reset notice omitted the initial backend inventory")
            last_inventory = message
            listing_index = match + 1
            inventories.append(index)
        elif message.startswith(("Using an earned usage reset", "Reset not used")):
            require(inventories, "printed reset notice decision preceded its inventory")
            deadline = listings[listing_index - 1][1]
            require(deadline is not None, "printed reset notice has no matching weekly allowance reading")
            try:
                stamp = datetime.datetime.fromtimestamp(deadline, datetime.timezone.utc).isoformat()
            except (ValueError, OSError, OverflowError) as error:
                raise RuntimeError("printed reset notice has an invalid provider weekly refresh time") from error
            if spending:
                decision = ("Using an earned usage reset automatically: the weekly allowance refreshes "
                            f"at {stamp}, more than 24 hours away.")
                require(message in (decision, decision + " Choosing the earliest-expiring usable reset."),
                        "printed reset notice contradicts the automatic decision or weekly refresh time")
            else:
                require(message == f"Reset not used: the weekly allowance refreshes at {stamp} "
                        "(within 24 hours); preserving earned resets.",
                        "printed reset notice contradicts the preservation decision or weekly refresh time")
            decisions.append(index)
        elif message.startswith("Usage allowance restored"):
            require(spending and message == "Usage allowance restored after the earned reset; continuing the original request.",
                    "printed reset notice contradicts confirmed restoration")
            restorations.append(index)
        elif message.startswith("Earned usage reset applied"):
            require(spending and message == "Earned usage reset applied; checking that allowance is restored.",
                    "printed reset notice contradicts reset application")
            applied.append(index)
    require(inventories,
            "printed reset notice lost the backend's available count or expiration timestamps")
    require(len(decisions) == 1, "printed reset notice must report one decision for this reset")
    decision = decisions[0]
    require(max(inventories) < decision, "printed reset notice decision preceded its inventory")
    if spending:
        require(len(restorations) == 1, "printed reset notice must report one confirmed restoration")
        require(decision < restorations[0], "printed reset notice restoration preceded its decision")
        require(len(applied) <= 1, "printed reset notice repeated the reset application")
        require(not applied or decision < applied[0] < restorations[0],
                "printed reset notice application preceded its decision or followed restoration")


def file_sha256(path):
    return FILE_HASH["file_sha256"](path, error_type=RuntimeError)

def fresh_environment(root):
    for name in ("home", "codex-home", "work", "xdg", "tmp"):
        (root / name).mkdir()
    # No inherited login, provider, proxy, aterm or agent variables.
    return {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": str(root / "home"),
            "CODEX_HOME": str(root / "codex-home"), "XDG_RUNTIME_DIR": str(root / "xdg"),
            "TMPDIR": str(root / "tmp"), "TERM": "dumb", "NO_COLOR": "1", "LANG": "en_US.UTF-8"}

def run(executable, args, root, env, label, *, interact=None):
    interaction_error = None

    def observe(child, stdout):
        nonlocal interaction_error
        try:
            interact(child, stdout)
        except BaseException as error:
            # This callback interrupts its leased native PID even on failure.
            # Preserve its raw exit before propagating the interaction error.
            interaction_error = error

    raw = guarded_run(executable, args, root, env, label,
                      interact=observe if interact else None, stdin_pipe=False,
                      timeout_seconds=COMMAND_TIMEOUT_SECONDS)
    if interaction_error is not None:
        try:
            write_json(root / (label + "-interaction-failure.json"), {
                "error": f"{type(interaction_error).__name__}: {interaction_error}",
                "guardianExit": raw if raw >= 0 else 128 - raw,
                "rawReturncode": raw, "guardianTimedOut": False})
        finally:
            raise interaction_error
    return raw

def sse(events):
    return "".join(f"event: {event['type']}\ndata: {json.dumps(event)}\n\n" for event in events).encode()

def response(response_id, item):
    return sse([{"type": "response.created", "response": {"id": response_id}},
                {"type": "response.output_item.done", "item": item},
                {"type": "response.completed", "response": {"id": response_id, "usage": {
                    "input_tokens": 0, "input_tokens_details": None, "output_tokens": 0,
                    "output_tokens_details": None, "total_tokens": 0}}}])

def jwt():
    def segment(value):
        return base64.urlsafe_b64encode(json.dumps(value).encode()).rstrip(b"=").decode()
    return segment({"alg": "none"}) + "." + segment({
        "email": "fixture@example.invalid", "sub": USER, "exp": int(time.time()) + 86400,
        "https://api.openai.com/auth": {"chatgpt_account_id": ACCOUNT,
                                        "chatgpt_user_id": USER, "chatgpt_plan_type": "pro"}
    }) + ".fixture"


class Mock:
    def __init__(self, name):
        self.name = name
        reset_delay = (90 if name == "natural-reset" else 4 * 60 if name == "near-reset"
                       else -3600 if name == "preflight-mixed-windows"
                       else 49 * 3600 if name in ("preflight-weekly-49h", "recovery-weekly-49h")
                       else 25 * 3600 if name in ("preflight-beyond-24h", "recovery-beyond-24h", "preflight-weekly-25h", "recovery-weekly-25h")
                       else 48 * 3600 if name in ("preflight-weekly-48h", "recovery-weekly-48h")
                       else 24 * 3600 if name in ("preflight-weekly-24h", "recovery-weekly-24h") else 72 * 3600)
        now = int(time.time())
        self.reset = now + reset_delay
        self.five_hour_reset = now + 3600
        self.credit = name not in (*FUNDED_CASES, "no-credit", "late-credit", "natural-reset")
        self.consumed = name == "unused-pending"
        self.requests = []
        self.models = []
        self.lock = threading.Lock()
        self.checked = threading.Event()
        self.old_cycle = self.reset - 6 * 3600
        self.old_key = None
        self.account_key = None
        self.spent = []
        self.declined = []
        self.renewed_after_listing = False
        self.wall_usage_reads = 0
        self.propagation_started = None
        self.partial_receipt_path = None
        self.receipt_dir = None
        self.stale_allowance_reads = 0
        self.credit_listings = 0
        self.post_selection_reads = 0
        self.expected_access_token = None

    def usage(self):
        now = int(time.time())
        preflight = self.name in PREFLIGHT_CASES or (self.name in POLICY_PREFLIGHT_CASES and len(self.models) < 2)
        healthy = self.consumed or (not preflight and len(self.models) < 2)
        if len(self.models) >= 2:
            self.wall_usage_reads += 1
        reset = self.reset
        if self.name == "stale-allowance-credit" and len(self.models) >= 3 and len(self.spent) == 1:
            self.stale_allowance_reads += 1
            healthy = self.stale_allowance_reads == 1
            if not healthy:
                self.consumed = False
        if (self.name == "natural-reset" and self.wall_usage_reads >= 2) or self.renewed_after_listing:
            healthy = True
            reset += 604800
        if self.name == "stale-reset-credit" and self.wall_usage_reads == 1:
            reset = now - 60
        if self.name in ("preflight-elapsed-reset", "recovery-elapsed-reset"):
            # The provider's 429 deadline remains ahead while its usage read
            # reports an elapsed refresh. Polling must not spend on that stale read.
            reset = now - 60
        if self.name in PROPAGATION_CASES and self.consumed:
            healthy = (self.propagation_started is not None
                       and time.monotonic() - self.propagation_started >= 60)
            reset += 3600
        available = int(self.credit and not self.consumed)
        if self.name in TWO_CREDITS:
            available = 2 - len(self.spent) - len(self.declined)
            if self.name == "stale-credit-ledger" and len(self.spent) == 2:
                available = 2
        elif self.name == "unused-pending":
            available = int(not self.spent)
        primary_used = 0
        if self.name == "preflight-five-hour" and not healthy:
            primary_used = 99 if preflight else 100
        elif self.name == "preflight-below-99":
            primary_used = 95
        elif self.name == "preflight-mixed-windows":
            primary_used = 98
        value = {"account_id": "other-account" if self.name == "identity-mismatch" else ACCOUNT,
                "user_id": USER, "plan_type": "pro",
                "rate_limit": {"allowed": healthy or preflight, "limit_reached": not (healthy or preflight),
                    "primary_window": {"used_percent": primary_used, "limit_window_seconds": 18000,
                        "reset_after_seconds": self.five_hour_reset - now, "reset_at": self.five_hour_reset},
                    "secondary_window": {"used_percent": (0 if healthy or self.name in ("preflight-five-hour", "preflight-below-99")
                                                         else 98 if self.name == "recovery-below-99"
                                                         else 99 if preflight or self.name in ("recovery-weekly-25h", "recovery-weekly-48h", "recovery-weekly-49h") else 100),
                        "limit_window_seconds": 604800,
                        "reset_after_seconds": reset - now, "reset_at": reset}},
                "rate_limit_reset_credits": {"available_count": available}}
        if self.name in FUNDED_CASES:
            authorizing_list = 2 if self.name == "spend-control-reopens" else 1
            balance = "30965.02000" if self.name in PAID_RETRY_CASES and self.credit_listings >= authorizing_list else "30966.04065"
            value["credits"] = {"has_credits": True, "unlimited": False, "balance": balance}
            if self.name == "spend-control-reopens" and self.credit_listings:
                self.post_selection_reads += 1
            value["spend_control"] = {"reached": self.name == "spend-control-blocked"
                                      or (self.name == "spend-control-reopens" and self.post_selection_reads == 1)}
            if not healthy:
                value["rate_limit_reached_type"] = {"type": "rate_limit_reached"}
        elif self.name == "no-credit":
            value["credits"] = {"has_credits": False, "unlimited": False, "balance": "0"}
        if self.name in PREPOST_CASES and self.credit_listings and not self.consumed:
            self.post_selection_reads += 1
            window = value["rate_limit"]["secondary_window"]
            if self.name == "prepost-recovery-restored":
                # The first fresh read is restored but still at99%; recovery
                # must require DENIED. The subsequent sampler sees ordinary
                # headroom, so its independent preflight has no reason to POST.
                value["rate_limit"].update(allowed=True, limit_reached=False)
                window["used_percent"] = 99 if self.post_selection_reads == 1 else 0
            elif self.name == "prepost-recovery-shifted":
                # Every eligibility/fresh-read pair differs, even when the
                # retained credit is reused without another list request.
                require(self.post_selection_reads <= 512, "excessive adversarial usage reads")
                window["reset_at"] += self.post_selection_reads * 60
                window["reset_after_seconds"] += self.post_selection_reads * 60
            elif self.name == "prepost-recovery-foreign":
                value["account_id"] = "other-account"
            elif self.name == "prepost-recovery-below-99":
                window["used_percent"] = 98
        return value

    def reply(self, method, path, body, *, authentication=None):
        with self.lock:
            record = {"method": method, "path": path, "body": body, "at": time.monotonic()}
            self.requests.append(record)
            if authentication is not None:
                record["authentication"] = authentication
                if not all(authentication.values()):
                    record.update(status=401, fixtureError="backend fixture authentication mismatch")
                    return 401, b'{"error":"fixture authentication mismatch"}', "application/json"
            if method == "POST" and path == "/v1/responses":
                self.models.append(body)
                number = len(self.models)
                if self.name in PREFLIGHT_CASES:
                    require(self.consumed and len(self.spent) == 1, "preflight did not redeem before model work")
                    if number == 2:
                        item = {"type": "message", "role": "assistant", "id": "preflight-answer",
                                "content": [{"type": "output_text", "text": ANSWER}]}
                        return 200, response("preflight-completed", item), "text/event-stream"
                if number == 1:
                    if self.name == "committed-ack":
                        receipt = json.loads(self.partial_receipt_path.read_text())
                        require(receipt["consumed"] is True and receipt["retry_released"] is True,
                                "first model request preceded settlement of the committed receipt")
                    item = {"type": "function_call", "call_id": CALL_ID, "name": "update_plan",
                            "arguments": json.dumps({"plan": [{"step": PLAN, "status": "completed"}]})}
                    return 200, response("weekly-plan", item), "text/event-stream"
                if number == 3 and self.name in TWO_TOOLS:
                    expected_spent = [] if self.name == "unused-pending" else ["earned-weekly-credit"]
                    require(self.consumed and self.spent == expected_spent,
                            "first weekly wall retried without its earned reset")
                    item = {"type": "function_call", "call_id": CALL_ID2, "name": "update_plan",
                            "arguments": json.dumps({"plan": [
                                {"step": PLAN, "status": "completed"},
                                {"step": PLAN2, "status": "completed"}]})}
                    return 200, response("weekly-second-plan", item), "text/event-stream"
                if (number == 2 or (number == 3 and self.name in ("paid-retry-refused", "stale-restored", "stale-allowance-credit", "spent-retry-deadline"))
                        or (number == 4 and self.name in TWO_TOOLS)
                        or (number == 5 and self.name == "stale-credit-ledger")):
                    if number in (4, 5):
                        self.consumed = False
                    if number == 4 and self.name == "shared-credit":
                        # A peer settles a fresh credit while this turn is sampling.
                        # Its account receipt must not consume this turn's retry.
                        state = json.loads((self.receipt_dir / "state.json").read_text())
                        generation = state["next_generation"]
                        peer_credit = "peer-earned-credit"
                        key = hashlib.sha256(
                            f"alab/weekly-reset/v3/{self.account_key}/{self.reset}/{peer_credit}/{generation}".encode()).hexdigest()
                        peer_path = self.receipt_dir / ("credit-" + hashlib.sha256(
                            peer_credit.encode()).hexdigest() + ".json")
                        write_json(peer_path, {"id": peer_credit, "consumed": True,
                                               "retry_released": True})
                        peer_path.chmod(0o600)
                        state["next_generation"] = generation + 1
                        state["current"] = {"credit": peer_credit, "cycle": self.reset,
                            "key": key, "generation": generation, "outcome": "consumed"}
                        peer_state = self.receipt_dir / "peer-state.json"
                        write_json(peer_state, state)
                        peer_state.chmod(0o600)
                        peer_state.replace(self.receipt_dir / "state.json")
                        self.consumed = True
                        record["peerResetCredit"] = peer_credit
                    if number == 3:
                        if self.name == "spent-retry-deadline":
                            self.reset = int(time.time()) + 3
                        self.checked.set()
                    if number == 2 and self.name in DEADLINE_OUTAGE_CASES:
                        self.reset = int(time.time()) + 3
                    elif number == 2 and self.name == "natural-reset":
                        # Backend restoration is observed on the paced second
                        # poll, before the original wall deadline takes over.
                        self.reset = int(time.time()) + 90
                    wall_reset, window = (self.five_hour_reset, "five-hour") if self.name == "preflight-five-hour" else (self.reset, "weekly")
                    record["quotaResetUnix"] = wall_reset
                    return 429, json.dumps({"error": {"type": "usage_limit_reached",
                        "message": f"Fixture {window} quota exhausted", "resets_at": wall_reset}}).encode(), "application/json"
                naturally_restored = ((self.name == "natural-reset" and self.wall_usage_reads >= 2)
                                      or (self.name == "prepost-recovery-restored" and self.post_selection_reads > 0))
                if self.name in DEADLINE_OUTAGE_CASES:
                    record["serverUnix"] = int(time.time())
                    naturally_restored = record["serverUnix"] >= self.reset
                if ((number == 3 or (number == 5 and self.name in TWO_TOOLS)
                     or (number == 4 and self.name in ("stale-allowance-credit", "spent-retry-deadline")))
                        and (self.consumed or naturally_restored or self.name in FUNDED_CASES)):
                    if self.name == "spent-retry-deadline":
                        record["serverUnix"] = int(time.time())
                        require(record["serverUnix"] >= self.reset,
                                "native retry ran before its original reset deadline")
                    item = {"type": "message", "role": "assistant", "id": "weekly-answer",
                            "content": [{"type": "output_text", "text": ANSWER}]}
                    return 200, response("weekly-recovered", item), "text/event-stream"
                return 403, b'{"error":{"message":"Unexpected model retry"}}', "application/json"
            if method == "GET" and path == "/backend-api/wham/usage":
                value = self.usage()
                if self.credit_listings:
                    record["afterCreditSelection"] = self.credit_listings
                if ((self.name == "usage-outage" and self.wall_usage_reads == 1)
                        or (self.name == "usage-outage-deadline" and len(self.models) >= 2)):
                    record["status"] = 503
                    record["response"] = {"error": "temporary usage service outage"}
                    return 503, json.dumps(record["response"]).encode(), "application/json"
                if len(self.models) >= 2 and self.name != "stale-restored":
                    self.checked.set()
            elif method == "GET" and path == "/backend-api/wham/rate-limit-reset-credits":
                if ((self.name == "credit-list-outage" and sum(
                        request["path"] == path for request in self.requests) == 1)
                        or (self.name == "credit-list-outage-deadline" and len(self.models) >= 2)):
                    record["status"] = 503
                    record["response"] = {"error": "temporary credit service outage"}
                    return 503, json.dumps(record["response"]).encode(), "application/json"
                if self.name == "late-credit" and sum(
                        request["path"].endswith("/rate-limit-reset-credits")
                        for request in self.requests) >= 2:
                    self.credit = True
                value = {"available_count": int(self.credit), "credits": []}
                if self.credit:
                    value["credits"] = [{"id": "earned-weekly-credit", "reset_type": "codex_rate_limits",
                        "status": "available", "granted_at": "2026-01-01T00:00:00Z",
                        "expires_at": "2098-01-01T00:00:00Z"}]
                    if self.name in TWO_CREDITS:
                        value["credits"].append({"id": "earned-weekly-credit-2",
                            "reset_type": "codex_rate_limits", "status": "available",
                            "granted_at": "2026-01-01T00:00:00Z", "expires_at": "2099-01-01T00:00:00Z"})
                        if not (self.name == "stale-credit-ledger" and len(self.spent) == 2):
                            value["credits"] = [credit for credit in value["credits"]
                                                if credit["id"] not in self.spent + self.declined]
                        value["available_count"] = len(value["credits"])
                if self.name == "plan-renewed-before-credit-retry" and len(self.models) >= 2:
                    self.renewed_after_listing = True
                self.credit_listings += 1
                record["selectionNumber"] = self.credit_listings
            elif method == "POST" and path == "/backend-api/wham/rate-limit-reset-credits/consume":
                latest = next((request["response"] for request in reversed(self.requests[:-1])
                               if request["path"].endswith("/usage") and "rate_limit" in request.get("response", {})), None)
                now = int(time.time())
                record["serverUnix"] = now
                if self.name in PREPOST_CASES:
                    fresh = [request for request in self.requests[:-1]
                             if request["path"].endswith("/usage") and request.get("afterCreditSelection")]
                    require(fresh, "redemption POST preceded its post-selection usage read")
                require(latest is not None and latest["account_id"] == ACCOUNT and latest["user_id"] == USER
                        and any(isinstance(window.get("used_percent"), (int, float))
                                and math.isfinite(window["used_percent"]) and window["used_percent"] >= 99
                                and window.get("limit_window_seconds") == 604800
                                and isinstance(window.get("reset_at"), int)
                                and window["reset_at"] - now > 24 * 3600
                                for window in (latest["rate_limit"]["primary_window"], latest["rate_limit"]["secondary_window"])),
                        "redemption policy requires a weekly window at >=99% with more than 24 hours until its reset")
                if self.name == "pending-old-cycle" and body.get("credit_id") == "previous-earned-credit":
                    require(body.get("redeem_request_id") == self.old_key,
                            "pending redemption did not retain the original key")
                    record["response"] = {"code": "already_redeemed", "windows_reset": 2}
                    return 200, json.dumps(record["response"]).encode(), "application/json"
                if self.name == "lost-ack-propagation" and self.consumed:
                    previous = next(request for request in self.requests[:-1]
                                    if request["path"].endswith("/consume"))
                    require(body == previous["body"], "uncertain reset changed its original key or credit")
                    record["response"] = {"code": "already_redeemed", "windows_reset": 2}
                    return 200, json.dumps(record["response"]).encode(), "application/json"
                require(self.name in (*SUCCESS_CASES, "stale-restored", "stale-credit-ledger"),
                        "ineligible case attempted to consume a credit")
                require(not self.consumed and self.credit, "credit consumed twice")
                require(set(body) == {"redeem_request_id", "credit_id"}, "unexpected consume payload")
                expected_credit = ("earned-weekly-credit-2" if self.spent or self.declined else "earned-weekly-credit")
                require(body["credit_id"] == expected_credit, "wrong or already spent credit selected")
                require(body["credit_id"] not in self.spent, "known consumed credit was posted again")
                require(isinstance(body["redeem_request_id"], str)
                        and re.fullmatch(r"[0-9a-f]{64}", body["redeem_request_id"]),
                        "missing stable SHA256 redeem ID")
                if self.name in (*TWO_TOOLS, "selected-no-credit", "committed-ack"):
                    generation = (1 if self.name == "unused-pending" else len(self.spent) + len(self.declined) + 1)
                    if self.name == "committed-ack":
                        generation += 1
                    expected_key = hashlib.sha256(
                        f"alab/weekly-reset/v3/{self.account_key}/{self.reset}/{expected_credit}/{generation}".encode()).hexdigest()
                    require(body["redeem_request_id"] == expected_key, "credit-specific stable key missing")
                if self.name == "selected-no-credit" and not self.declined:
                    self.declined.append(body["credit_id"])
                    record["response"] = {"code": "no_credit", "windows_reset": 0}
                    return 200, json.dumps(record["response"]).encode(), "application/json"
                self.consumed = True
                if self.name in PROPAGATION_CASES:
                    self.propagation_started = record["at"]
                self.spent.append(body["credit_id"])
                value = {"code": "reset", "windows_reset": 2}
                if self.name in ("lost-ack", "lost-ack-propagation"):
                    record["droppedResponse"] = True
                    return None, b"", "application/json"
            else:
                return 404, b'{"error":"unexpected fixture endpoint"}', "application/json"
            record["response"] = value
            return 200, json.dumps(value).encode(), "application/json"


def serve(mock):
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def handle_request(self):
            length = int(self.headers.get("content-length", "0"))
            if not 0 <= length <= 8 * 1024 * 1024:
                self.send_error(413)
                return
            body = json.loads(self.rfile.read(length)) if length else None
            authentication = None
            if self.path.startswith("/backend-api/"):
                bearer = self.headers.get_all("authorization", [])
                account = self.headers.get_all("chatgpt-account-id", [])
                authentication = {
                    "authorizationPresent": bool(bearer),
                    "authorizationMatches": (mock.expected_access_token is not None
                                              and bearer == ["Bearer " + mock.expected_access_token]),
                    "accountPresent": bool(account),
                    "accountMatches": account == [ACCOUNT],
                }
            try:
                if authentication is None:
                    status, data, content_type = mock.reply(self.command, self.path, body)
                else:
                    status, data, content_type = mock.reply(self.command, self.path, body,
                                                          authentication=authentication)
            except Exception as error:
                with mock.lock:
                    mock.requests[-1]["fixtureError"] = str(error)
                status, data, content_type = 500, b'{"error":"fixture contract failed"}', "application/json"
            if status is None:
                # The backend committed the one redemption, then lost its HTTP
                # acknowledgement. Only a fresh usage read can release the retry.
                self.close_connection = True
                self.connection.shutdown(2)
                return
            self.send_response(status)
            self.send_header("content-type", content_type)
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            try:
                self.wfile.write(data)
            except (BrokenPipeError, ConnectionResetError):
                pass
        do_GET = handle_request
        do_POST = handle_request

    return start_listener(Handler)


def configure(root, port, openai_auth=True):
    (root / "codex-home/config.toml").write_text(f'''model = "{MODEL}"
model_provider = "weekly-smoke"
chatgpt_base_url = "http://127.0.0.1:{port}/backend-api"
approval_policy = "never"
sandbox_mode = "read-only"
[features]
enable_request_compression = false
[tools.update_plan]
enabled = true
[model_providers.weekly-smoke]
name = "{'OpenAI' if openai_auth else 'unrelated loopback provider'}"
base_url = "http://127.0.0.1:{port}/v1"
wire_api = "responses"
requires_openai_auth = {str(openai_auth).lower()}
request_max_retries = 0
stream_max_retries = 0
''')
    token = jwt()
    write_json(root / "codex-home/auth.json", {"auth_mode": "chatgpt", "tokens": {
        "id_token": token, "access_token": token, "refresh_token": "fixture-refresh",
        "account_id": ACCOUNT}, "last_refresh": datetime.datetime.now(datetime.timezone.utc).isoformat()})
    (root / "codex-home/auth.json").chmod(0o600)
    return token


def case(executable, output, name):
    root = output / name
    root.mkdir()
    env = fresh_environment(root)
    mock = Mock(name)
    server, worker = serve(mock)
    cancelled = False
    cleanup_result = {"case": name, "passed": True}
    primary = None

    def cancel_after_check(child, stdout):
        nonlocal cancelled
        try:
            # A request appearing in the server log does not prove its response
            # reached recovery. A second paced poll witnesses a completed wait loop.
            expected_models, expected_consumes = {
                "stale-restored": (3, 1), "stale-credit-ledger": (5, 2), "paid-retry-refused": (3, 0)
            }.get(name, (2, 0))
            retained = name in ("paid-retry-refused", "no-credit", "stale-credit-ledger", "stale-restored", *POLICY_POLL_CASES, *PREPOST_NEGATIVE_CASES)

            def retained_wait():
                if not retained:
                    return False
                with mock.lock:
                    consumes = sum(request["path"].endswith("/consume") for request in mock.requests)
                    require(len(mock.models) <= expected_models and consumes <= expected_consumes,
                            f"{name}: unexpected retry or reset redemption during the wait")
                    if not mock.checked.is_set() or len(mock.models) != expected_models or consumes != expected_consumes:
                        return False
                    last_model = max(request["at"] for request in mock.requests
                                     if request["path"] == "/v1/responses")
                    usage_polls = ("stale-restored", "paid-retry-refused", *POLICY_POLL_CASES, *PREPOST_NEGATIVE_CASES)
                    endpoint = "/usage" if name in usage_polls else "/rate-limit-reset-credits"
                    polls = [request for request in mock.requests
                             if request["path"].endswith(endpoint)
                             and request["at"] > last_model]
                    return has_paced_poll(polls)

            deadline = time.monotonic() + 60
            buffer = b""
            while not (retained_wait() if retained else b"Waiting for account quota" in buffer):
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    with mock.lock:
                        require(len(mock.models) == expected_models,
                                f"{name}: expected {expected_models} model requests before the wait, observed {len(mock.models)}")
                    raise RuntimeError("retained recovery did not make a paced second poll" if retained
                                       else "native quota wait notification missing")
                ready, _, _ = select.select([child.stdout], [], [], min(remaining, 1))
                if ready:
                    data = os.read(child.stdout.fileno(), 65536)
                    require(bool(data), "process exited before native quota wait")
                    stdout.write(data)
                    stdout.flush()
                    buffer += data
                    require(len(buffer) < 1024 * 1024, "oversized quota notification")
            with mock.lock:
                require(mock.checked.is_set() or name == "unrelated-provider",
                        "native wait skipped fresh weekly usage check")
                require(len(mock.models) == expected_models,
                        "unexpected sampling before native quota wait")
                require(sum(request["path"].endswith("/consume") for request in mock.requests)
                        == expected_consumes, "unexpected consume before native wait")
        finally:
            cancelled = child.poll() is None
            if cancelled:
                # The guardian retains/reaps this exact native PID; signal Codex so
                # both guardian layers can retain its handled failure status.
                try:
                    os.kill(int((root / "native.pid").read_text()), signal.SIGINT)
                except (FileNotFoundError, ProcessLookupError):
                    pass

    try:
        mock.expected_access_token = configure(root, server.server_port,
                                               openai_auth=name != "unrelated-provider")
        base = f"http://127.0.0.1:{server.server_port}/backend-api"
        account_key = hashlib.sha256(f"{base}\0{ACCOUNT}\0{USER}".encode()).hexdigest()
        mock.account_key = account_key
        mock.receipt_dir = root / "codex-home" / ("weekly-reset-" + account_key + ".d")
        if name in ("pending-old-cycle", "unused-pending", "committed-ack"):
            cycle = mock.old_cycle if name in ("pending-old-cycle", "committed-ack") else mock.reset
            credit = "previous-earned-credit" if name in ("pending-old-cycle", "committed-ack") else "earned-weekly-credit"
            mock.old_key = hashlib.sha256(
                f"alab/weekly-reset/v3/{account_key}/{cycle}/{credit}/1".encode()).hexdigest()
            receipt = root / "codex-home" / ("weekly-reset-" + account_key + ".d")
            receipt.mkdir(mode=0o700)
            state = {"version": 2 if name == "committed-ack" else 1,
                "next_generation": 2, "current": {"credit": credit,
                "cycle": cycle, "key": mock.old_key, "generation": 1,
                "outcome": "awaiting_ack" if name == "committed-ack" else "pending"}}
            write_json(receipt / "state.json", state)
            credit_file = receipt / ("credit-" + hashlib.sha256(credit.encode()).hexdigest() + ".json")
            mock.partial_receipt_path = credit_file
            write_json(credit_file, {"id": credit, "consumed": name == "committed-ack", "retry_released": False})
            (receipt / "state.json").chmod(0o600)
            credit_file.chmod(0o600)
        arguments = ["exec", "--json", "--color", "never", "--skip-git-repo-check",
                     "--cd", str(root / "work"), "--sandbox", "read-only", PROMPT]
        observed = name not in SUCCESS_CASES
        command_executable = executable
        if observed:
            command_executable = Path(sys.executable)
            arguments = ["-I", "-B", str(Path(__file__).resolve()), "--exec-wrapper", str(root / "native.pid"),
                         str(executable), *arguments]
        code = run(command_executable, arguments, root, env, "exec",
                   interact=cancel_after_check if observed else None)
    except BaseException as error:
        primary = error
        raise
    finally:
        cleanup_case(server, worker, lambda: write_json(root / "requests.json", mock.requests),
                     cleanup_result, primary)
    if not cleanup_result["passed"]:
        write_json(root / "result.json", cleanup_result)
        return cleanup_result
    events = [json.loads(line) for line in (root / "exec-stdout.jsonl").read_text().splitlines()]
    types = [event.get("type") for event in events]
    guardian = json.loads((root / "exec-guardian-result.json").read_text())
    require(guardian["timed_out"] is False, "guardian timed out")
    require(types.count("turn.started") == 1, "expected exactly one native turn")
    require(not any("fixtureError" in request for request in mock.requests), "HTTP fixture contract failed")
    require(len(mock.models) >= 2, "model never reached retained-tool quota wall")
    for body in mock.models:
        require(body.get("model") == MODEL, "model changed")
        user_texts = [part.get("text") for item in body.get("input", [])
                     if item.get("type") == "message" and item.get("role") == "user"
                     for part in item.get("content", []) if isinstance(part, dict)]
        require(user_texts.count(PROMPT) == 1, "original input lost or duplicated")
    account_requests = [request for request in mock.requests if request["path"].startswith("/backend-api/")]
    require(all(request.get("authentication") is not None
                and all(request["authentication"].values()) for request in account_requests),
            "backend request lacks verified fixture authentication")
    if name == "unrelated-provider":
        require(not account_requests, "unrelated provider consulted the stored ChatGPT account")
    else:
        require(any(request["path"] == "/backend-api/wham/usage" for request in mock.requests),
                "automatic weekly quota check missing")
    consumes = [request for request in mock.requests if request["path"].endswith("/consume")]
    previous_post = float("-inf")
    for post in consumes:
        preceding = [request for request in account_requests if request["at"] < post["at"]]
        latest = preceding[-1] if preceding else {}
        require(latest.get("method") == "GET" and latest.get("path", "").endswith("/usage")
                and "rate_limit" in latest.get("response", {})
                and all(latest.get("authentication", {}).values())
                and previous_post < latest["at"] < post["at"],
                "redemption POST lacks a fresh authenticated usage GET")
        previous_post = post["at"]
    if name in SUCCESS_CASES or name in ("paid-retry-refused", "spend-control-blocked"):
        outputs = [item for item in mock.models[-1]["input"] if item.get("type") == "function_call_output"]
        expected_calls = {CALL_ID, CALL_ID2} if name in TWO_TOOLS else {CALL_ID}
        require(len(outputs) == len(expected_calls) and {item.get("call_id") for item in outputs} == expected_calls
                and all("Plan updated" in str(item.get("output")) for item in outputs),
                "completed plan outputs missing, duplicated or changed")
        plans = [event for event in events if event.get("item", {}).get("type") == "todo_list"]
        expected_plan = [{"text": PLAN, "completed": True}]
        if name in TWO_TOOLS:
            expected_plan.append({"text": PLAN2, "completed": True})
        else:
            require([event["type"] for event in plans] == ["item.started", "item.completed"],
                    "completed plan was executed more than once")
        require(plans[-1]["item"]["items"] == expected_plan, "completed plan was lost")
    if name in SUCCESS_CASES:
        expected_models = 2 if name in PREFLIGHT_CASES else (4 if name in ("stale-allowance-credit", "spent-retry-deadline") else 5 if name in TWO_TOOLS else 3)
        require(code == 0 and len(mock.models) == expected_models, "weekly reset did not recover")
        expected_consumes = 2 if name in ("stale-allowance-credit", "lost-ack-propagation", "pending-old-cycle", "repeated-same-cycle", "selected-no-credit") else 1
        if name in (*FUNDED_CASES, "natural-reset", "prepost-recovery-restored") or name in DEADLINE_OUTAGE_CASES:
            expected_consumes = 0
        require(len(consumes) == expected_consumes
                and (mock.consumed or name in (*FUNDED_CASES, "natural-reset", "prepost-recovery-restored") or name in DEADLINE_OUTAGE_CASES),
                "earned credit not consumed exactly once after pending reconciliation")
        if name in DEADLINE_OUTAGE_CASES:
            models = [request for request in mock.requests if request["path"] == "/v1/responses"]
            failures = [request for request in mock.requests if request.get("status") == 503]
            require(models[-1]["serverUnix"] >= models[-2]["quotaResetUnix"],
                    "outage retried the model before the native reset deadline")
            require(models[-1]["at"] - models[-2]["at"] < 20,
                    "backend outage held the native deadline until another polling interval")
            require(failures and all(models[-2]["at"] < request["at"] < models[-1]["at"]
                                     for request in failures),
                    "fixture did not retain the backend outage through reset recovery")
            require(not mock.spent and not mock.consumed,
                    "native reset deadline spent an earned credit")
        if name == "spent-retry-deadline":
            models = [request for request in mock.requests if request["path"] == "/v1/responses"]
            require(models[-1]["serverUnix"] >= models[-2]["quotaResetUnix"],
                    "spent-grant wait stranded the native reset deadline")
            require(mock.models[2]["input"] == mock.models[3]["input"],
                    "native deadline handoff lost the retained request")
        if name == "stale-allowance-credit":
            denied_at = [request["at"] for request in mock.requests if "quotaResetUnix" in request][-1]
            reads = [request for request in mock.requests if request["path"].endswith("/usage")
                     and denied_at < request["at"] < consumes[1]["at"]]
            require(len(reads) == 3 and reads[1]["at"] - reads[0]["at"] >= 29,
                    "spent retry did not retain a paced usage poll")
            require(reads[0]["response"]["rate_limit"]["allowed"] is True
                    and all(read["response"]["rate_limit"]["allowed"] is False
                            for read in reads[1:]),
                    "fixture did not reconcile stale healthy usage with current exhaustion")
            lists = [request for request in mock.requests
                     if request["path"].endswith("/rate-limit-reset-credits")
                     and reads[1]["at"] < request["at"] < consumes[1]["at"]]
            require(len(lists) == 1 and lists[0]["at"] < reads[2]["at"] < consumes[1]["at"],
                    "redemption did not recheck usage after listing the next earned credit")
            require([request["body"]["credit_id"] for request in consumes]
                    == ["earned-weekly-credit", "earned-weekly-credit-2"],
                    "current exhaustion did not redeem the next earned credit")
            require(mock.models[2]["input"] == mock.models[3]["input"],
                    "retained turn changed while stale usage converged")
        if name in OUTAGE_CASES:
            failures = [request for request in mock.requests if request.get("status") == 503]
            require(len(failures) == 1, "fixture did not inject one transient outage")
            failed = failures[0]
            retry = next(request for request in mock.requests
                         if request["path"] == failed["path"] and request["at"] > failed["at"])
            require(retry["at"] - failed["at"] >= 29,
                    "transient endpoint failure did not retain a paced poll")
            if name == "usage-outage":
                require(retry["response"]["rate_limit"]["allowed"] is False,
                        "usage outage retry did not verify current exhaustion")
                require(all(request["at"] > retry["at"] for request in mock.requests
                            if request["path"].endswith("/rate-limit-reset-credits")),
                        "failed usage authorized a credit listing")
            else:
                require(retry["response"]["available_count"] == 1,
                        "credit outage retry did not return a usable credit")
            require(consumes[0]["at"] > retry["at"],
                    "failed endpoint response authorized a redemption")
        if name == "shared-credit":
            peer = [request for request in mock.requests if "peerResetCredit" in request]
            require(len(peer) == 1 and mock.spent == ["earned-weekly-credit"],
                    "shared reset must resume without redeeming the peer's credit")
            require(consumes[0]["at"] < peer[0]["at"],
                    "fixture did not settle the shared reset after the turn's first retry")
            peer_usage = [request for request in mock.requests
                          if request["path"].endswith("/usage") and request["at"] > peer[0]["at"]]
            retried = [request for request in mock.requests if request["path"] == "/v1/responses"][-1]
            require(any(request["response"]["rate_limit"]["allowed"] is True
                        and request["at"] < retried["at"] for request in peer_usage),
                    "shared retry did not verify fresh restored allowance")
        if name == "plan-renewed-before-credit-retry":
            wall, retry = [request for request in mock.requests if request["path"] == "/v1/responses"][1:]
            checks = [request for request in account_requests if wall["at"] < request["at"] < retry["at"]]
            listings = [request for request in checks if request["path"].endswith("/rate-limit-reset-credits")]
            require(len(listings) == 1 and listings[0].get("response") == {"available_count": 0, "credits": []},
                    "renewal requires one empty reset listing between the wall and retry")
            selection = checks.index(listings[0])
            for phase, allowed in ((checks[:selection], False), (checks[selection + 1:], True)):
                readings = [request["response"] for request in phase if request["path"].endswith("/usage")]
                require(readings and all(read.get("credits", {}).get("has_credits") is True
                        and read["rate_limit"]["allowed"] is allowed
                        and read["rate_limit"]["limit_reached"] is (not allowed)
                        and (read["rate_limit"]["secondary_window"]["used_percent"] < 100) is allowed
                        for read in readings),
                        "renewal requires funded exhausted usage before listing and restored usage before retry")
            fresh = next(request for request in checks[selection + 1:] if request["path"].endswith("/usage"))
            require(fresh["at"] - listings[0]["at"] < 29,
                    "renewed plan was only discovered on a later quota poll")
        if name == "natural-reset":
            wall = next(request for request in mock.requests if "quotaResetUnix" in request)
            retry = [request for request in mock.requests if request["path"] == "/v1/responses"][-1]
            usage = [request for request in mock.requests
                     if request["path"].endswith("/usage")
                     and wall["at"] < request["at"] < retry["at"]]
            lists = [request for request in mock.requests
                     if request["path"].endswith("/rate-limit-reset-credits")]
            # Recovery observes restoration on its paced poll. The sampling
            # boundary then performs its own fresh healthy preflight.
            require(len(usage) >= 2 and usage[1]["at"] - usage[0]["at"] >= 29,
                    "natural reset did not retain a paced usage poll")
            require(usage[0]["response"]["rate_limit"]["allowed"] is False
                    and all(request["response"]["rate_limit"]["allowed"] is True
                            and request["response"]["rate_limit"]["limit_reached"] is False
                            and request["response"]["rate_limit"]["secondary_window"]["reset_at"]
                            > wall["quotaResetUnix"] for request in usage[1:]),
                    "fixture did not restore the exhausted allowance")
            require(len(lists) == 1 and lists[0]["response"]["available_count"] == 0
                    and usage[0]["at"] <= lists[0]["at"] < usage[1]["at"],
                    "natural reset fixture offered or queried another credit")
            require(not mock.spent and not mock.consumed,
                    "natural reset was mistaken for a credit redemption")
        if name == "committed-ack":
            require(consumes[0]["body"]["credit_id"] == "earned-weekly-credit"
                    and consumes[0]["body"]["redeem_request_id"] != mock.old_key,
                    "committed receipt was replayed instead of selecting the fresh credit")
        if name == "late-credit":
            lists = [request for request in mock.requests
                     if request["path"].endswith("/rate-limit-reset-credits")]
            require(len(lists) == 2 and lists[1]["at"] - lists[0]["at"] >= 29,
                    "late credit did not require a paced second poll")
            require(lists[0]["response"]["available_count"] == 0
                    and lists[1]["response"]["available_count"] == 1,
                    "fixture did not grant a new credit after the first check")
        if name == "stale-reset-credit":
            denied = [request for request in mock.requests
                      if request["path"].endswith("/usage")
                      and request.get("response", {}).get("rate_limit", {}).get("allowed") is False]
            lists = [request for request in mock.requests
                     if request["path"].endswith("/rate-limit-reset-credits")]
            require(len(denied) == 3 and denied[1]["at"] - denied[0]["at"] >= 29,
                    "stale reset did not retain a paced usage poll")
            require(denied[0]["response"]["rate_limit"]["secondary_window"]["reset_after_seconds"] < 0
                    and all(request["response"]["rate_limit"]["secondary_window"]["reset_after_seconds"] > 0
                            for request in denied[1:]),
                    "fixture did not advance stale usage to a fresh cycle")
            require(len(lists) == 1 and lists[0]["at"] > denied[1]["at"]
                    and lists[0]["at"] < denied[2]["at"] < consumes[0]["at"],
                    "stale usage authorized a credit list or redemption")
        if name in PROPAGATION_CASES:
            denied = [request for request in mock.requests
                      if request["path"].endswith("/usage") and request["at"] > consumes[0]["at"]
                      and request.get("response", {}).get("rate_limit", {}).get("allowed") is False]
            require(len(denied) == (4 if name == "lost-ack-propagation" else 2)
                    and mock.spent == ["earned-weekly-credit"],
                    "delayed usage propagation spent an additional reset")
            require(all(request["response"]["rate_limit"]["secondary_window"]["reset_at"] > mock.reset
                        for request in denied), "fixture did not advance the reset while usage lagged")
            retried = [request for request in mock.requests if request["path"] == "/v1/responses"][-1]
            require(retried["at"] - consumes[0]["at"] >= 59,
                    "sampler resumed before the two paced propagation polls")
        if name == "lost-ack-propagation":
            require(consumes[0].get("droppedResponse") is True
                    and consumes[1]["response"]["code"] == "already_redeemed"
                    and consumes[0]["body"] == consumes[1]["body"],
                    "lost acknowledgement did not reconcile the exact same reset")
            require(sum(request["path"].endswith("/rate-limit-reset-credits")
                        for request in mock.requests) == 1,
                    "uncertain recent reset queried another credit while usage lagged")
        if name == "pending-old-cycle":
            require(consumes[0]["body"] == {"redeem_request_id": mock.old_key,
                    "credit_id": "previous-earned-credit"}, "older pending key was abandoned")
            require(consumes[0].get("response", {}).get("code") == "already_redeemed",
                    "older credit was not reconciled as previously redeemed")
            require(consumes[1]["body"]["credit_id"] == "earned-weekly-credit",
                    "current cycle did not redeem the new earned credit")
        if name == "lost-ack":
            require(consumes[0].get("droppedResponse") is True,
                    "consume acknowledgement was not actually dropped")
            ledger = json.loads((root / "codex-home" / ("weekly-reset-" + account_key + ".d")
                                 / "state.json").read_text())
            require(ledger["version"] == 2 and ledger["current"]["outcome"] == "restored_pending"
                    and ledger["current"]["key"] == consumes[0]["body"]["redeem_request_id"],
                    "healthy allowance did not preserve the uncertain receipt and its restoration witness")
        if name == "selected-no-credit":
            require(consumes[0].get("response", {}).get("code") == "no_credit"
                    and consumes[1].get("response", {}).get("code") == "reset"
                    and mock.spent == ["earned-weekly-credit-2"],
                    "selected NoCredit did not release the next available credit")
            require(any(request["path"].endswith("/usage")
                        and request.get("response", {}).get("rate_limit", {}).get("allowed") is False
                        and consumes[0]["at"] < request["at"] < consumes[1]["at"] for request in mock.requests),
                    "NoCredit was not reconciled against fresh denied usage")
        if name == "unused-pending":
            require(mock.spent == ["earned-weekly-credit"], "external restore excluded an unspent earned credit")
            require(consumes[0]["body"]["redeem_request_id"] == mock.old_key,
                    "new depletion abandoned the original ambiguous redemption key")
        if name in ("recovery-weekly-25h", "recovery-weekly-48h", "recovery-weekly-49h"):
            denied = [request["response"] for request in mock.requests
                      if request["path"].endswith("/usage")
                      and request.get("response", {}).get("rate_limit", {}).get("allowed") is False]
            require(denied and all(reading["rate_limit"]["primary_window"]["used_percent"] == 0
                                  and reading["rate_limit"]["secondary_window"]["used_percent"] == 99
                                  and reading["rate_limit"]["secondary_window"]["limit_window_seconds"] == 604800
                                  and reading["rate_limit"]["secondary_window"]["reset_after_seconds"] > 24 * 3600
                                  for reading in denied),
                    "recovery did not observe its denied 99% weekly window beyond 24 hours")
        if name in PREFLIGHT_CASES:
            first_model = next(request for request in mock.requests if request["path"] == "/v1/responses")
            require(consumes[0]["at"] < first_model["at"], "reset happened after model work started")
            require(not any("quotaResetUnix" in request for request in mock.requests), "preflight hit a quota wall")
        else:
            require(mock.models[1]["input"] == mock.models[2]["input"], "retry changed retained tool/input")
        if name in TWO_TOOLS:
            require(mock.models[3]["input"] == mock.models[4]["input"], "second retry changed retained input")
        if name == "repeated-same-cycle":
            require([request["body"]["credit_id"] for request in consumes]
                    == ["earned-weekly-credit", "earned-weekly-credit-2"], "two fresh credits not spent once each")
            require(consumes[0]["body"]["redeem_request_id"] != consumes[1]["body"]["redeem_request_id"],
                    "same cycle reused one redemption key for different credits")
            model_records = [request for request in mock.requests if request["path"] == "/v1/responses"]
            require(model_records[1]["quotaResetUnix"] == model_records[3]["quotaResetUnix"],
                    "fixture did not exercise the same natural reset cycle")
            for consume, retry in zip(consumes, (model_records[2], model_records[4])):
                require(any(request["path"].endswith("/usage")
                            and request.get("response", {}).get("rate_limit", {}).get("allowed") is True
                            and consume["at"] < request["at"] < retry["at"] for request in mock.requests),
                        "quota retry did not verify restored usage after its own reset")
        require(types.count("turn.completed") == 1 and "turn.failed" not in types, "native turn failed")
        answers = [(index, event["item"].get("text")) for index, event in enumerate(events)
                   if event.get("type") == "item.completed"
                   and event.get("item", {}).get("type") == "agent_message"]
        require(len(answers) == 1 and answers[0][1] == ANSWER,
                "weekly reset recovery lost, changed or repeated the fixture answer")
        turn_start = types.index("turn.started")
        answer_index = answers[0][0]
        require(turn_start < answer_index < types.index("turn.completed"),
                "weekly reset recovery answer is outside its original turn")
        if name in (*FUNDED_CASES, *NOTICE_SUCCESS_CASES):
            prefix = decision_prefix(name)
            in_turn = notice_messages(events[turn_start + 1:answer_index], prefix)
            require(in_turn and in_turn == notice_messages(events, prefix),
                    "all recovery notices must appear in the original turn before the recovered answer")
        restored_after = consumes[-1]["at"] if consumes else next(
            request["at"] for request in mock.requests if "quotaResetUnix" in request)
        require(name in (*PAID_RETRY_CASES, *DEADLINE_OUTAGE_CASES) or any(request["path"].endswith("/usage")
                    and request.get("response", {}).get("rate_limit", {}).get("allowed") is True
                    and request["at"] > restored_after for request in mock.requests),
                "restored usage was not verified")
    else:
        if name == "paid-retry-refused":
            require(cancelled and len(mock.models) == 3 and not consumes,
                    "refused paid retry must stay pending without another retry or redemption")
            require(mock.models[1]["input"] == mock.models[2]["input"], "paid retry changed retained input")
            require(not any(event.get("item", {}).get("type") == "agent_message" for event in events),
                    "refused paid retry reported an answer")
            end = types.index("turn.failed") if "turn.failed" in types else len(events)
            # Exec can finalize its plan display on interruption. The retry's model
            # input above is the witness that the tool already completed.
            start = types.index("turn.started")
            require(notice_messages(events[start + 1:end], CREDIT_NOTICE_PREFIX),
                    "paid retry notice must appear in the pending turn")
        elif name == "stale-credit-ledger":
            require(len(mock.models) == 5 and len(consumes) == 2
                    and mock.spent == ["earned-weekly-credit", "earned-weekly-credit-2"],
                    "stale credit details caused reuse of a consumed credit")
            require(mock.models[1]["input"] == mock.models[2]["input"]
                    and mock.models[3]["input"] == mock.models[4]["input"], "retained tool inputs changed")
        elif name == "stale-restored":
            require(len(consumes) == 1 and mock.consumed, "stale usage spent multiple credits")
            require(len(mock.models) == 3, "stale restored usage caused a sampling retry loop")
            require(mock.models[1]["input"] == mock.models[2]["input"],
                    "stale usage recovery changed retained tool/input")
        else:
            require(not consumes and not mock.consumed, "ineligible credit consumed")
            require(len(mock.models) == 2, "ineligible wall retried before reset")
            if name in POLICY_BLOCKED_CASES:
                require(mock.credit and not mock.spent, "policy case did not retain its available earned credit")
                outputs = [item for item in mock.models[-1]["input"]
                           if item.get("type") == "function_call_output"]
                require(len(outputs) == 1 and outputs[0].get("call_id") == CALL_ID
                        and "Plan updated" in str(outputs[0].get("output")),
                        "policy refusal lost the completed plan output from model input")
                plans = [event for event in events if event.get("item", {}).get("type") == "todo_list"]
                require([event["type"] for event in plans] == ["item.started", "item.completed"]
                        and plans[-1]["item"]["items"] == [{"text": PLAN, "completed": True}],
                        "policy refusal lost or repeated completed plan work")
                denied = [request["response"] for request in mock.requests
                          if request["path"].endswith("/usage")
                          and request.get("response", {}).get("rate_limit", {}).get("allowed") is False]
                require(denied, "policy case did not exercise reactive recovery")
                if name in POLICY_PREFLIGHT_CASES:
                    first_model = next(request["at"] for request in mock.requests if request["path"] == "/v1/responses")
                    require(any(request["path"].endswith("/usage") and request["at"] < first_model
                                and request["response"]["rate_limit"]["allowed"] is True
                                for request in mock.requests), "policy case skipped the preflight boundary")
        require(code != 0 and "turn.completed" not in types, "ineligible case reported success")
    if name in PREPOST_CASES:
        lists = [request for request in mock.requests if "selectionNumber" in request]
        fresh = [request for request in mock.requests if request.get("afterCreditSelection")]
        require(lists and fresh and lists[0]["at"] < fresh[0]["at"],
                "fixture never reached usage after actual credit selection")
        if name == "prepost-recovery-restored":
            first = fresh[0]["response"]["rate_limit"]
            require(first["allowed"] is True and first["secondary_window"]["used_percent"] == 99
                    and not consumes, "restored recovery spent a credit at99%")
        if name in PREPOST_NEGATIVE_CASES:
            require(not consumes and not mock.spent, "changed post-selection usage authorized a POST")
        for post in consumes:
            require(any(lists[0]["at"] < read["at"] < post["at"] for read in fresh),
                    "POST lacks an actual post-selection usage observation")
    check_reset_notice_events(name, events, mock.requests)
    result = {"case": name, "passed": True, "exit": code, "cancelled": cancelled,
              "guardianTimedOut": guardian["timed_out"], "rawReturncode": guardian["returncode"],
              "authenticatedBackendRequests": len(account_requests),
              "modelRequests": len(mock.models), "consumeRequests": len(consumes),
              "resetNotices": len(notice_messages(events, RESET_NOTICE_PREFIX)),
              "creditNotices": len(notice_messages(events, CREDIT_NOTICE_PREFIX)),
              "postSelectionUsageReads": mock.post_selection_reads, "events": types}
    write_json(root / "result.json", result)
    return result


def _arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--codex", type=Path)
    source.add_argument("--package", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--case", action="append", choices=CASE_NAMES,
                        help="Run selected cases only; repeat to retain a subset after a fixture failure")
    args = parser.parse_args()
    selected = list(args.case or CASE_NAMES)
    if len(selected) != len(set(selected)):
        parser.error("--case must not repeat a case")
    return args


def main(args=None):
    if args is None:
        args = _arguments()
    selected = list(args.case or CASE_NAMES)
    output = args.output.absolute()
    output.mkdir(parents=True, exist_ok=False)
    report = {"passed": False, "fullSuite": False, "requestedCases": selected,
              "cases": [], "scope": "isolated fake ChatGPT auth and loopback only; no compilation"}
    interrupted = None
    try:
        if args.package:
            package = args.package.resolve(strict=True)
            build, manifest, executable = package_records(package)
            report.update(package=str(package), source=build["source"], packageRecords={"build": build, "manifest": manifest})
        else:
            executable = args.codex.resolve(strict=True)
        require(executable.is_file(), "Codex executable missing")
        report.update(executable=str(executable), binarySha256=file_sha256(executable))
        if args.package:
            check_package_executable(build, report["binarySha256"])
        for name in selected:
            report["failedCase"] = name
            result = case(executable, output, name)
            report["cases"].append(result)
            require(result["passed"], f"{name} did not pass")
        del report["failedCase"]
        require(file_sha256(executable) == report["binarySha256"],
                "Codex executable changed during the smoke")
        report["passed"] = True
        report["fullSuite"] = set(selected) == set(CASE_NAMES)
    except Exception as error:
        report.update(case_error_fields(error))
    except (KeyboardInterrupt, SystemExit) as error:
        # case/run have already unwound their child and server cleanup. Keep
        # incomplete evidence without converting cancellation into a normal exit.
        interrupted = error
        report["interrupted"] = True
        report.update(case_error_fields(error))
    try:
        report["probeSha256"] = file_sha256(Path(__file__))
        write_json(output / "result.json", report)
        print(json.dumps(report, indent=2))
    finally:
        # Reporting incomplete evidence must preserve the original cancellation,
        # even when hashing, saving or printing the report also fails.
        if interrupted is not None:
            raise interrupted
    return 0 if report["passed"] else 1


def cli():
    # Help, parser errors and duplicate selections finish before an observation.
    args = _arguments()
    return CLI["run_operation"](lambda: main(args), "weekly reset: ")


if __name__ == "__main__":
    sys.exit(cli())
