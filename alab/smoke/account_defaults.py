# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Verify a fresh installed Codex thread against its own account's model catalog."""

import re
import time


# sol_family.rs ranks at most three u64 components, padded with zeroes.
_COMPONENT = r"(?:0|[1-9][0-9]{0,19})"
_RELEASE = re.compile(rf"gpt-({_COMPONENT}(?:\.{_COMPONENT}){{0,2}})(?:-(sol|astra|terra))?")
_EFFORTS = ("none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra", "persistent")


def _catalog(rpc):
    """Keep the first eligible model at the highest release and only reportable model names."""
    params = {}
    cursors = set()
    public_models = set()
    best_version = ()
    best_model = None
    now = time.time()
    deadline = time.monotonic() + 60
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise ValueError("catalog deadline")
        page = rpc.request("model/list", params, timeout=remaining)
        if not isinstance(page["data"], list):
            raise ValueError("invalid catalog")
        for row in page["data"]:
            model = row["model"]
            match = _RELEASE.fullmatch(model)
            if match is None:
                continue
            numbers = tuple(int(part) for part in match[1].split("."))
            if max(numbers) >= 2**64:
                continue
            public_models.add(model)
            if match[2] != "sol" or row["hidden"] is True:
                continue
            if row["hidden"] is not False:
                raise ValueError("invalid model visibility")
            upgrade = row.get("upgradeInfo")
            if upgrade is None and row.get("upgrade") is not None:
                raise ValueError("missing upgrade retirement metadata")
            retirement = upgrade["retirementAt"] if upgrade is not None else None
            if retirement is not None:
                if type(retirement) is not int:
                    raise ValueError("invalid retirement time")
                if retirement <= now:
                    continue
            efforts = row["supportedReasoningEfforts"]
            if not isinstance(efforts, list):
                raise ValueError("invalid effort list")
            levels = [option["reasoningEffort"] for option in efforts]
            if any(not isinstance(level, str) or not level for level in levels):
                raise ValueError("invalid effort")
            version = numbers + (0,) * (3 - len(numbers))
            if version > best_version and "ultra" in levels:
                best_version, best_model = version, model
        cursor = page["nextCursor"]
        if cursor is None:
            return best_model, public_models
        if not isinstance(cursor, str) or not cursor or cursor in cursors or not page["data"]:
            raise ValueError("invalid catalog cursor")
        cursors.add(cursor)
        params = {"cursor": cursor}


def check_defaults(rpc):
    """Use the slot's initialized app server and clean cwd. Never send a model turn."""
    result = {"verified": False}
    error = "model catalog check failed"
    try:
        expected, listed = _catalog(rpc)
        if expected is None:
            return dict(result, error="no eligible Sol model supports Ultra")
        error = "fresh thread check failed"
        started = rpc.request("thread/start", {"ephemeral": True})
        if started["thread"]["ephemeral"] is not True or started["modelProvider"] != "openai":
            return dict(result, error="ephemeral OpenAI thread unconfirmed")
        model, effort = started["model"], started["reasoningEffort"]
        error = "model catalog check failed"
        confirmed, after = _catalog(rpc)
        if confirmed != expected:
            return dict(result, error="default model changed during verification")
        result["expectedModel"] = expected
        if isinstance(model, str) and model in listed and model in after:
            result["model"] = model
        if effort in _EFFORTS:
            result["reasoningEffort"] = effort
        if model != expected or effort != "ultra":
            return dict(result, error="fresh thread defaults mismatch")
        result["verified"] = True
    except (AttributeError, KeyError, OSError, RuntimeError, TypeError, ValueError):
        # The RPC or provider can include private data in its reply or exception.
        result["error"] = error
    return result
