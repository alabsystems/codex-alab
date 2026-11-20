# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Plain-text display for the live subscription report."""

import re
from datetime import datetime, timezone


def valid_slot_label(label):
    return (isinstance(label, str) and label != "help"
            and re.fullmatch(r"[a-z0-9][a-z0-9._-]{0,63}", label) is not None)


def _terminal_text(value):
    if not isinstance(value, str) or not value:
        return "unavailable"
    return "".join(char if char.isprintable() else ascii(char)[1:-1] for char in value)


def _window(window):
    minutes = window.get("windowDurationMins")
    name = "Usage window"
    if type(minutes) is int and minutes > 0:
        name = {300: "5h", 10080: "Weekly"}.get(minutes, f"{minutes} min")

    used = window.get("usedPercent")
    allowance = f"{100 - used}% left" if type(used) is int and 0 <= used <= 100 else "allowance unavailable"
    reset = "reset unavailable"
    timestamp = window.get("resetsAt")
    if type(timestamp) is int:
        try:
            local = datetime.fromtimestamp(timestamp, timezone.utc).astimezone()
            reset = f"resets {local.isoformat(sep=' ', timespec='seconds')}"
        except (ValueError, OverflowError, OSError):
            pass
    return name, f"{allowance} · {reset}"


def _registry(association, verified):
    machine = _terminal_text(association.get("machine"))
    sources = association.get("sources")
    status = association.get("status")
    labels = {"preference": "preferred", "machine-preference": "preferred",
              "placement": "placement", "retained-observation": "previously observed"}
    if (verified and status == "associated" and isinstance(sources, list) and sources
            and all(isinstance(s, str) and s in labels for s in sources)):
        observed = ", ".join(dict.fromkeys(labels[s] for s in sources))
        return f"{machine} · Associated ({observed})"
    if verified and status == "missing":
        return f"{machine} · No association found"
    reason = association.get("reason") if verified else "provider-login-unconfirmed"
    explanations = {
        "commons-membership-not-ready": "Commons membership is not ready",
        "machine-not-found": "machine not found",
        "ambiguous-machine": "ambiguous machine",
        "unconfirmed-account-key": "local account key unconfirmed",
        "provider-login-unconfirmed": "login unconfirmed",
        "registry-view-unconfirmed": "published Registry view unconfirmed",
    }
    return f"{machine} · Unverified ({explanations.get(_terminal_text(reason), 'Registry read unavailable')})"


def format_overview(rows):
    blocks = []
    for row in rows:
        verified = row.get("loginVerified") is True and "error" not in row
        allowed = row.get("ordinaryUsageAllowed") if verified else None
        if allowed is True and not row.get("ordinaryUsageConflict"):
            permission = "Allowed"
        elif allowed is False:
            permission = "Blocked"
        else:
            permission = "Unknown"
        label = row.get("slotLabel")
        switch = ("Slot label", _terminal_text(label))
        if verified and valid_slot_label(label):
            if row.get("switchRefusal"):
                switch = ("Switch", f"{label} · unavailable ({_terminal_text(row['switchRefusal'])})")
            elif allowed is False:
                switch = ("Switch", f"{label} · unavailable (provider usage blocked)")
            elif row.get("launcherDefaultOnly") is True:
                switch = ("Switch", f"{label} · check /accounts in the session (configured launcher uses only the default login)")
            else:
                switch = ("Request switch", f"/accounts use {label}")
        details = [
            switch,
            ("Sign-in", "Verified" if verified else "Unverified"),
            ("Provider usage", permission),
        ]
        if verified and valid_slot_label(label) and row.get("loginEmail"):
            details.append(("Sign-in address", _terminal_text(row["loginEmail"])))
        if verified and row.get("nextLaunch") is True:
            details.append(("Next launch", "Selected by aterm"))
        association = row.get("registryAssociation")
        if isinstance(association, dict):
            details.append(("Registry", _registry(association, verified)))
        defaults = row.get("freshThreadDefaults")
        if verified and isinstance(defaults, dict):
            if defaults.get("model") and defaults.get("reasoningEffort"):
                observed = f"{_terminal_text(defaults['model'])} · {_terminal_text(defaults['reasoningEffort'])}"
                details.append(("Fresh thread", observed))
            if defaults.get("verified") is not True:
                if defaults.get("expectedModel"):
                    details.append(("Expected", f"{_terminal_text(defaults['expectedModel'])} · ultra"))
                details.append(("Default check", _terminal_text(defaults.get("error"))))
        if "error" in row:
            details.append(("Verification", _terminal_text(row["error"])))
        elif verified and row.get("windows"):
            details.extend(_window(window) for window in row["windows"])
        else:
            details.append(("Usage windows", "unavailable"))
        lines = [_terminal_text(row.get("email"))]
        lines.extend(f"  {label:<14}  {value}" for label, value in details)
        blocks.append("\n".join(lines))
    return "\n\n".join(blocks)
