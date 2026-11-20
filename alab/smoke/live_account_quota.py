#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Show live Codex subscriptions, verify their quota, or look up local switch labels."""

import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from dataclasses import dataclass, field, replace
from datetime import datetime, timezone
from functools import partial
import json
import os
from pathlib import Path
import re
import shlex
import stat
import subprocess
import sys
import tempfile
import time
from typing import NamedTuple, Optional

from account_defaults import check_defaults
from account_overview import format_overview, valid_slot_label
from quota_resume import Rpc


ASCII_LOWER = str.maketrans("ABCDEFGHIJKLMNOPQRSTUVWXYZ", "abcdefghijklmnopqrstuvwxyz")


def email_matches(expected, answered):
    # aterm compares a configured address to a trimmed answer with ASCII case folding.
    return (isinstance(answered, str)
            and expected.translate(ASCII_LOWER) == answered.strip().translate(ASCII_LOWER))


class LaunchChoice(NamedTuple):
    key: str
    email: str


@dataclass(frozen=True)
class LaunchWithoutAccount:
    roster_path: Path
    accounts: frozenset
    until: Optional[datetime]
    # Native reasons can name a private account or path. Compare them across samples only.
    reason: Optional[str] = field(repr=False)


@dataclass(frozen=True)
class SwitchDestination:
    label: str
    configured_dir: Optional[str]
    key: Optional[str]
    auth_path: Path
    # A host refusal changes switch eligibility independently of this slot's identity.
    switch_refusal: Optional[str] = field(compare=False)
    leased_elsewhere_until: Optional[datetime] = field(default=None, compare=False)

    # The worker launcher can differ from the program in an existing session.
    launcher_default_only: bool = field(default=False, compare=False)
    not_enrolled: bool = field(default=False, compare=False)
    # Losing confirmation of the sign-in alias does not invalidate a named switch.
    login_alias: Optional[str] = field(default=None, compare=False, repr=False)

    def current_refusal(self):
        if self.switch_refusal:
            return self.switch_refusal
        if self.leased_elsewhere_until and self.leased_elsewhere_until > datetime.now(timezone.utc):
            return f"host reported a lease on another Mac until {self.leased_elsewhere_until:%Y-%m-%d %H:%M:%S UTC}"
        return None


class RosterSlot(NamedTuple):
    email: Optional[str]
    destination: Optional[SwitchDestination]
    login: Optional[str]
    admitted_login_key: Optional[str]


def listed_lease_until(key, accounts, next_row):
    # The signed host serializes LeasedElsewhere in next.not. Other pool
    # exclusions can still serve a named session move.
    if (key is None or not isinstance(next_row, dict) or "to" not in next_row or next_row["to"] == key
            or (next_row["to"] is not None and not isinstance(next_row["to"], str))
            or not isinstance(next_row.get("not"), list)):
        return None
    # Native references fall back to the label for an account without a key.
    if any(isinstance(row, dict) and row.get("ac") is None and row.get("label") == key for row in accounts):
        return None
    reasons = [item.get("why") for item in next_row["not"] if isinstance(item, dict) and item.get("account") == key]
    if len(reasons) != 1 or not isinstance(reasons[0], str):
        return None
    match = re.fullmatch(r"leased elsewhere until (\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)Z", reasons[0], re.ASCII)
    if not match:
        return None
    try:
        return datetime.fromisoformat(match[1]).replace(tzinfo=timezone.utc)
    except ValueError:
        return None


def listed_destination(label, configured_dir, identity, accounts, *, held, rotation_off, next_row, launcher_default_only):
    if not isinstance(identity, str) or not isinstance(accounts, list):
        return None
    matches = [row for row in accounts if isinstance(row, dict) and email_matches(
        identity, row.get("email") if row.get("identity_email") is None else row["identity_email"])]
    if len(matches) != 1:
        return None
    row = matches[0]
    if row.get("label", label) != label:
        return None
    key = row.get("ac")
    if key is not None and (
        not isinstance(key, str) or re.fullmatch(r"ac-[0-9a-f]{16}", key) is None
        or sum(isinstance(other, dict) and other.get("ac") == key for other in accounts) != 1
    ):
        return None
    # The list applies the login store's renewal directory. The default row
    # omits dir and the native observer checks the user's default Codex home.
    directory = row.get("dir", "~/.codex")
    if not isinstance(directory, str) or not directory:
        return None
    try:
        path = Path(directory).expanduser() / "auth.json"
    except (ValueError, OSError, RuntimeError):
        return None
    if not path.is_absolute():
        return None
    condition = row.get("condition")
    # Native asked_row_refusal accepts a low row. Only its named refusals apply here.
    if rotation_off:
        refusal = "host account switching is off"
    elif condition == "not-enrolled":
        refusal = "host reports no enrollment"
    elif condition == "signed-out":
        refusal = "host reports signed out"
    elif held or row.get("held_by") is not None:
        refusal = "held by host"
    elif condition == "limited":
        refusal = "host reports a usage limit"
    else:
        refusal = None
    return SwitchDestination(label, configured_dir, key, path, refusal,
                             listed_lease_until(key, accounts, next_row), launcher_default_only,
                             not_enrolled=condition == "not-enrolled")


def launch_without_account(next_row, roster_path, identities):
    if next_row.get("to_note") is not None:
        return None
    timestamp = next_row.get("wait_until")
    rendering = next_row.get("wait_until_rfc3339")
    reason = next_row.get("none")
    until = None
    if timestamp is not None or rendering is not None:
        # Signed aterm supplies both a UTC epoch and its RFC3339 rendering for a wait.
        if type(timestamp) is not int or reason is not None:
            return None
        try:
            utc = datetime.fromtimestamp(timestamp, timezone.utc)
            until = utc.astimezone()
        except (ValueError, OverflowError, OSError):
            return None
        if utc.isoformat().replace("+00:00", "Z") != rendering:
            return None
    elif not isinstance(reason, str) or not reason.strip():
        return None
    accounts = []
    for key, email in identities:
        if (not isinstance(key, str) or re.fullmatch(r"ac-[0-9a-f]{16}", key) is None
                or not isinstance(email, str) or not email.strip()):
            return None
        accounts.append(LaunchChoice(key, email.strip().translate(ASCII_LOWER)))
    if len({a.key for a in accounts}) != len(accounts) or len({a.email for a in accounts}) != len(accounts):
        return None
    return LaunchWithoutAccount(roster_path, frozenset(accounts), until, reason)


def next_launch_choice(report, roster_path):
    next_row = report.get("next")
    rows = report.get("accounts")
    if not (isinstance(next_row, dict) and "to" in next_row
            and isinstance(rows, list) and all(isinstance(row, dict) for row in rows)):
        return None
    identities = [
        (row.get("ac"), row.get("email") if row.get("identity_email") is None else row["identity_email"])
        for row in rows
    ]
    key = next_row["to"]
    if key is None:
        return launch_without_account(next_row, roster_path, identities)
    if any(next_row.get(field) is not None for field in ("wait_until", "wait_until_rfc3339", "none")):
        return None
    if not isinstance(key, str) or re.fullmatch(r"ac-[0-9a-f]{16}", key) is None:
        return None
    matches = [email for ac, email in identities if ac == key]
    if len(matches) != 1 or not isinstance(matches[0], str) or not matches[0].strip():
        return None
    email = matches[0].strip().translate(ASCII_LOWER)
    if sum(email_matches(email, candidate) for _, candidate in identities) != 1:
        return None
    return LaunchChoice(key, email)


def configured_accounts(*, allow_missing=False):
    path, configured, listing = reported_roster(allow_missing=allow_missing)
    return path, configured, next_launch_choice(listing, path)


def reported_roster(*, allow_missing=False):
    try:
        report = subprocess.run(["aterm", "harness", "accounts", "list", "--cli", "codex", "--json"],
                                check=True, capture_output=True, timeout=30)
        listing = json.loads(report.stdout)
        if listing["kind"] != "accounts" or listing["cli"] != "codex" or listing.get("refused") is not None:
            raise ValueError("invalid roster report")
        path = Path(listing["roster"])
        if not path.is_absolute():
            raise ValueError("unavailable roster")
    except (OSError, subprocess.SubprocessError, ValueError, TypeError, KeyError):
        raise ValueError("Could not read aterm's local Codex roster.") from None
    configured = read_roster(path, listing, allow_missing=allow_missing and listing.get("accounts") == [])
    return path, configured, listing


def read_roster(path, listing, *, allow_missing=False):
    try:
        import tomllib
    except ModuleNotFoundError:
        sys.exit("Reading local switch labels requires Python 3.11 or newer.")

    try:
        try:
            if not stat.S_ISREG(path.stat().st_mode):
                raise ValueError("unavailable roster")
            with path.open("rb") as source:
                rows = tomllib.load(source).get("account", [])
        except FileNotFoundError:
            # On a new Mac, aterm lists zero accounts before its roster file exists.
            if not allow_missing:
                raise
            rows = []
        if not isinstance(rows, list):
            raise ValueError("invalid roster rows")
        accounts = listing.get("accounts")
        # The host can pin a row for new launches while refusing every explicit session move.
        next_row = listing.get("next")
        off = next_row.get("off") if isinstance(next_row, dict) else None
        rotation_off = isinstance(off, str) and bool(off.strip())
        # The field contains a native diagnostic that can name a private executable.
        why = next_row.get("only_default") if isinstance(next_row, dict) else None
        launcher_default_only = isinstance(why, str) and bool(why.strip())
        configured = {}
        for row in rows:
            label = row["label"]
            if not valid_slot_label(label) or label in configured:
                raise ValueError("invalid or duplicate switch label")
            # A provider may return a different address from the one used to sign in.
            identity = row.get("identity_email", row.get("email"))
            if identity is not None and not isinstance(identity, str):
                raise ValueError("invalid provider address")
            destination = listed_destination(label, row.get("dir"), identity, accounts,
                                             held=row.get("held_by") is not None, rotation_off=rotation_off,
                                             next_row=next_row, launcher_default_only=launcher_default_only)
            login = row.get("email")
            admitted = None
            if destination and isinstance(login, str):
                native = [entry for entry in accounts if isinstance(entry, dict)
                          and email_matches(login, entry.get("email"))]
                if len(native) == 1 and native[0].get("ac") == destination.key:
                    admitted = destination.key
                    if (login and not email_matches(login, identity)
                            and email_matches(identity, native[0].get("identity_email"))
                            and sum(isinstance(raw, dict) and email_matches(login, raw.get("email"))
                                    for raw in rows) == 1):
                        destination = replace(destination, login_alias=login)
            configured[label] = RosterSlot(identity, destination, login, admitted)
    except (OSError, ValueError, TypeError, KeyError):
        raise ValueError("Could not read aterm's local Codex roster.") from None
    return configured


def slot_label(configured, email):
    labels = [label for label, slot in configured.items()
              if slot.email is not None and email_matches(slot.email, email)]
    return labels[0] if len(labels) == 1 else None


def switch_destination(roster, email):
    label = slot_label(roster, email)
    return roster[label].destination if label is not None else None


class RegistrySelection(NamedTuple):
    machine_key: str
    accounts: tuple
    roster_path: Path

    @classmethod
    def from_report(cls, report, roster_path):
        if report["proven"] is not True or len(report["machines"]) != 1:
            sys.exit("The Registry could not confirm this machine's assigned sign-ins.")
        machine = report["machines"][0]
        if len(machine["accounts"]) < 2:
            sys.exit("Registry selection requires at least two currently assigned subscriptions.")
        accounts = tuple((row["account_key"], row["registered_login"], tuple(row["sources"]))
                         for row in machine["accounts"])
        if (len({key for key, _, _ in accounts}) != len(accounts)
                or not distinct_addresses([login for _, login, _ in accounts])):
            sys.exit("The Registry could not confirm distinct assigned subscriptions.")
        return cls(machine["machine_key"], accounts, roster_path)

    def destinations(self, roster):
        routes = {}
        for key, login, _sources in self.accounts:
            matches = [slot for slot in roster.values() if email_matches(login, slot.login)]
            if len(matches) != 1 or matches[0].admitted_login_key != key:
                sys.exit("The local roster could not confirm all registered sign-in addresses and account keys.")
            slot = matches[0]
            if slot_label(roster, slot.email) != slot.destination.label:
                sys.exit("The local roster could not confirm distinct provider identities for the selected subscriptions.")
            routes[slot.email] = slot.destination
        if len({destination.auth_path for destination in routes.values()}) != len(routes):
            sys.exit("Multiple selected accounts resolve to the same credential slot.")
        return routes

    def missing_roster_rows(self, roster, listing):
        # A missing match is evidence of absence only when every native row
        # matches a distinct configured login, provider identity and Registry key.
        from account_registry_probe import login_address

        accounts = listing.get("accounts")
        if (type(listing.get("schema")) is not int or listing["schema"] != 3 or "refused" not in listing
                or not isinstance(accounts, list) or len(accounts) != len(roster)):
            return None
        keys = {slot.admitted_login_key for slot in roster.values()}
        if None in keys or len(keys) != len(roster):
            return None
        if len({slot.destination.auth_path for slot in roster.values()}) != len(roster):
            return None
        try:
            logins = {login_address(slot.login) for slot in roster.values()}
            identities = {login_address(slot.email) for slot in roster.values()}
        except ValueError:
            return None
        if len(logins) != len(roster) or len(identities) != len(roster):
            return None
        missing = []
        for key, login, sources in self.accounts:
            if (not re.fullmatch(r"ac-[0-9a-f]{16}", key) or not sources
                    or not set(sources) <= {"preference", "machine-preference", "placement"}):
                return None
            matches = [slot for slot in roster.values() if email_matches(login, slot.login)]
            if matches:
                if len(matches) != 1 or matches[0].admitted_login_key != key:
                    return None
            elif key in keys or key in roster or any(email_matches(login, slot.email) for slot in roster.values()):
                return None
            else:
                missing.append((key, sources))
        return missing

    def confirm(self, path, roster, routes, bindings):
        if (path != self.roster_path or self.destinations(roster) != routes
                or any(email not in bindings or not bindings[email].matches(destination)
                       for email, destination in routes.items())):
            sys.exit("The selected Registry accounts no longer match all local credential slots.")

    def enrollment_guidance(self, routes, bindings, places, read_registry):
        generic = ("aterm could not confirm all selected sign-ins on this Mac. "
                   "Open /accounts add to check the slots.")
        missing = [email for email in routes if email not in bindings]
        if not missing or not bindings:
            return generic
        if any(not binding.matches(routes[email]) for email, binding in bindings.items()):
            return generic
        if any(not routes[email].not_enrolled or not observed_empty_slot(places, routes[email]) for email in missing):
            return generic
        # Name slots only while the Registry and roster agree about their identities.
        if RegistrySelection.from_report(read_registry(2, show_logins=True), self.roster_path) != self:
            return generic
        path, roster, listing = reported_roster()
        current = self.destinations(roster)
        if path != self.roster_path or routes != current or any(not current[email].not_enrolled for email in missing):
            return generic
        # The native report can give the person a login step or ask them to wait.
        # Show where to read that decision without copying its private details.
        steps = {row["ac"] for row in listing["accounts"]
                 if isinstance(row, dict) and isinstance(row.get("ac"), str)
                 and isinstance(row.get("sign_in_needed"), str) and row["sign_in_needed"].strip()}
        labels = [current[email].label for email in missing]
        noun = "slot" if len(labels) == 1 else "slots"
        message = f"aterm reports no enrollment for {noun} {', '.join(labels)}."
        native = [current[email] for email in missing if current[email].key in steps]
        if not native:
            return f"{message} Open /accounts add to check {'that slot' if len(labels) == 1 else 'those slots'}."
        # Native text identifies an account by its name or slug, which can
        # differ from the local slot label. The JSON report exposes its exact key.
        matches = ", ".join(f"{slot.label} (ac={slot.key})" for slot in native)
        message += f" In 'aterm harness accounts list --cli codex --json', read 'sign_in_needed' for {matches}."
        other = [current[email].label for email in missing if current[email].key not in steps]
        if other:
            message += f" Open /accounts add to check {', '.join(other)}."
        return message


def registry_setup_roster(report, read_registry):
    path, roster, listing = reported_roster(allow_missing=True)
    selected = RegistrySelection.from_report(report, path)
    if any(not any(email_matches(login, slot.login) for slot in roster.values())
           for _key, login, _sources in selected.accounts):
        generic = "The local roster could not confirm all registered sign-in addresses and account keys."
        missing = selected.missing_roster_rows(roster, listing)
        if not missing:
            sys.exit(generic)
        confirmed = read_registry(2, show_logins=True, setup_accounts=[key for key, _ in missing])
        if confirmed.get("setup_confirmed") is not True:
            sys.exit("The Registry could not confirm that the missing subscriptions can be added on this Mac.")
        if RegistrySelection.from_report(confirmed, path) != selected:
            sys.exit(generic)
        new_path, current, native = reported_roster(allow_missing=True)
        if new_path != path or current != roster or selected.missing_roster_rows(current, native) != missing:
            sys.exit(generic)
        steps = []
        sync_preferred = any("machine-preference" in sources or "preference" in sources for _, sources in missing)
        if sync_preferred:
            steps.append("Request this Mac's preferred subscriptions with 'aterm harness accounts sync alab --cli codex'.")
        for key, sources in missing:
            if set(sources) == {"placement"}:
                intro = ("If the placed subscription is still missing after sync checks the default login, request its row"
                         if sync_preferred else "Request the missing placed subscription")
                steps.append(f"{intro} with 'aterm harness accounts add {key} --cli codex'.")
        sys.exit(" ".join([*steps, "Once aterm lists the slot, open /accounts add to check it."]))
    return path, roster, next_launch_choice(listing, path)


def local_overview_hint(args):
    if any(not email.isprintable() for email in args.email):
        return "\nTo check the named provider logins independently of Registry assignments, rerun without --registry-here."
    command = [sys.executable, "alab/smoke/live_account_quota.py", "--overview"]
    if args.defaults:
        command.append("--defaults")
    if args.show_logins:
        command.append("--show-logins")
    if args.email:
        command.extend(["--", *args.email])
        scope = "Check the two named provider logins"
    else:
        scope = "If the local roster has two logins, check them"
    return f"\n{scope} independently of Registry assignments from the codex-alab checkout\n  {shlex.join(command)}"


def local_registry_machine(report, *, initial_overview=None):
    if report["status"] not in ("matched", "insufficient-accounts") or len(report["machines"]) != 1:
        if report.get("errors") == ["current-machine-binding-changed"]:
            sys.exit("This Mac's Registry binding changed during verification.")
        # A here report retains the machine key only if its opening and closing published bindings agree.
        if (report.get("requested_here") is True and report.get("requested_machine") is not None
                and report.get("errors") == ["unconfirmed-registered-login"]):
            sys.exit("The Registry could not confirm the sign-in address for every subscription assigned to this Mac.")
        message = "The Registry could not confirm the signed machine identity for this Mac."
        if (report.get("requested_here") is True and report.get("errors") == ["current-machine-binding-unconfirmed"]
                and report.get("commands")):
            published = report["commands"][-1].get("published_file", {})
            if published.get("state") == "never" and published.get("complete") is False:
                message += " The Registry machine table has never been published on this Mac."
                if initial_overview is not None and len(report["commands"]) == 1:
                    message += local_overview_hint(initial_overview)
        sys.exit(message)
    return report["machines"][0]["machine_key"]


def show_configured_labels(configured, emails, *, show_logins=False):
    def labeled(email, label):
        result = {"email": email, "slotLabel": label}
        slot = configured[label]
        login = slot.login.strip() if isinstance(slot.login, str) else None
        if show_logins and login and not email_matches(login, slot.email):
            result["loginEmail"] = login
        return result

    if not emails:
        print(json.dumps([labeled(slot.email, label)
                          for label, slot in configured.items()], indent=2))
        return 0

    results = []
    for email in emails:
        label = slot_label(configured, email)
        if label is not None:
            results.append(labeled(email, label))
        else:
            results.append({"email": email, "error": "expected one configured slot for this address"})
    print(json.dumps(results, indent=2))
    return int(any("error" in result for result in results))


def show_overview(rows, *, registry=None, launch=None):
    checked = datetime.now().astimezone()
    print(f"Codex subscriptions checked at {checked.isoformat(sep=' ', timespec='seconds')}\n")
    if registry and registry.get("status") in ("matched", "insufficient-accounts"):
        machines = registry.get("machines")
        if isinstance(machines, list) and len(machines) == 1 and isinstance(machines[0], dict):
            count = machines[0].get("machine_facts_seat_count")
            if type(count) is int and count >= 0:
                if count == 0:
                    print("The Registry has no signed machine identity recorded for the selected machine.\n")
                else:
                    noun = "identity" if count == 1 else "identities"
                    print(f"The Registry records {count} signed machine {noun} for the selected machine.\n")
    print(format_overview(rows))
    if not any(row.get("nextLaunch") for row in rows):
        outlook = "The host's next-launch choice is unconfirmed for these accounts."
        if (isinstance(launch, LaunchWithoutAccount) and len(rows) >= 2 and all(
                row.get("loginVerified") and "error" not in row and row.get("slotLabel") for row in rows)):
            if launch.until is None:
                outlook = "aterm has selected no roster account for the next launch."
            elif launch.until > checked:
                outlook = f"For the next launch, aterm expects to wait until {launch.until.isoformat(sep=' ', timespec='seconds')}."
        print(f"\n{outlook}")
    return int(any("error" in row or row["slotLabel"] is None for row in rows))


class FileStamp(NamedTuple):
    identity: tuple
    mtime_ns: int
    ctime_ns: int
    size: int


class SlotRead(NamedTuple):
    row: dict
    account_id: Optional[str] = None
    revision: Optional[FileStamp] = None
    retry_workspace: Optional[str] = None
    provider_switch_refusal: Optional[str] = None

    def with_error(self, error):
        return self._replace(row=dict(self.row, error=error))


def file_stamp(path):
    info = path.stat()
    if not stat.S_ISREG(info.st_mode):
        raise ValueError("credential slot unavailable")
    return FileStamp((info.st_dev, info.st_ino), info.st_mtime_ns, info.st_ctime_ns, info.st_size)


def observed_accounts():
    try:
        report = subprocess.run(["aterm", "harness", "accounts", "observed", "--json"],
                                check=True, capture_output=True, timeout=30)
        places = json.loads(report.stdout)["places"]
        if not isinstance(places, list) or not all(isinstance(row, dict) for row in places):
            raise ValueError("invalid account observations")
    except (OSError, subprocess.SubprocessError, ValueError, KeyError, TypeError):
        raise ValueError("Could not read aterm's local account observations.") from None
    return places


class ObservedBinding(NamedTuple):
    path: Path
    label: Optional[str]
    key: Optional[str]

    def matches(self, destination):
        return (self.key == destination.key and self.path == destination.auth_path
                and self.label in (None, destination.label))


def observed_path(row):
    try:
        path = Path(row["place"]).expanduser()
    except (KeyError, TypeError, ValueError, OSError, RuntimeError):
        return None
    return path if path.is_absolute() else None


def slot_claims(places, key, auth_path):
    # Native PlaceSeen reports the file when found and the registered directory
    # when unread. Either spelling belongs to the same slot.
    paths = (auth_path, auth_path.parent)
    for row in places:
        if row.get("cli") != "codex" or row.get("source") != "auth-file":
            continue
        belief = row.get("believed")
        if (isinstance(belief, dict) and belief.get("ac") == key) or observed_path(row) in paths:
            yield row


def observed_empty_slot(places, destination):
    claims = list(slot_claims(places, destination.key, destination.auth_path))
    if len(claims) != 1:
        return False
    row = claims[0]
    belief = row.get("believed")
    return (isinstance(belief, dict) and belief.get("ac") == destination.key
            and belief.get("label") in (None, destination.label)
            and observed_path(row) in (destination.auth_path, destination.auth_path.parent)
            and "observed" in row and row["observed"] is None and "agrees" in row and row["agrees"] is None)


def observed_binding(places, email):
    expected = email.strip()
    rows = [row for row in places if row.get("cli") == "codex"
            and row.get("source") == "auth-file" and row.get("agrees") is True
            and email_matches(expected, row.get("observed"))]
    if len(rows) != 1:
        raise ValueError("expected one matching enrolled account")
    path = observed_path(rows[0])
    if path is None:
        raise ValueError("credential slot unavailable")
    # Registry-keyed beliefs identify the account without revealing its local switch label.
    belief = rows[0].get("believed")
    label = belief.get("label") if isinstance(belief, dict) else belief
    key = belief.get("ac") if isinstance(belief, dict) else None
    if key and sum(1 for _ in slot_claims(places, key, path)) != 1:
        key = None
    return ObservedBinding(path, label, key)


def confirmed_launch(choice, bindings):
    if isinstance(choice, LaunchWithoutAccount):
        if len(bindings) >= 2 and all(LaunchChoice(binding.key, email.strip().translate(ASCII_LOWER))
                                      in choice.accounts for email, binding in bindings.items()):
            return choice
        return None
    if choice and any(binding.key == choice.key and email_matches(choice.email, email)
                      for email, binding in bindings.items()):
        return choice
    return None


def retain_registry_keys(keys, bindings):
    if keys is not None:
        for email, original in keys.items():
            if email not in bindings or bindings[email].key != original:
                keys[email] = None


def current_bindings(emails):
    try:
        places = observed_accounts() if emails else []
    except ValueError as error:
        print(error, file=sys.stderr)
        places = []
    return observed_bindings(places, emails)


def observed_bindings(places, emails):
    bindings = {}
    for email in emails:
        try:
            bindings[email] = observed_binding(places, email)
        except ValueError:
            pass
    return bindings


def confirmed_labels(candidates, bindings, *rosters):
    confirmed = {}
    for email, destination in candidates.items():
        binding = bindings.get(email)
        samples = [switch_destination(roster, email) for roster in rosters]
        if (destination is not None and binding is not None and binding.path == destination.auth_path
                and binding.label in (None, destination.label)
                and (not binding.key or not destination.key or binding.key == destination.key)
                and all(sample == destination for sample in samples)):
            refusal = next((sample.switch_refusal for sample in reversed(samples) if sample.switch_refusal),
                           destination.switch_refusal)
            leased_until = next((row.leased_elsewhere_until for row in reversed(samples)
                                 if row.leased_elsewhere_until is not None), destination.leased_elsewhere_until)
            launcher_default_only = destination.launcher_default_only or any(row.launcher_default_only for row in samples)
            alias = destination.login_alias
            if (not binding.matches(destination) or not (binding.key or binding.label)
                    or (alias and not all(email_matches(alias, sample.login_alias) for sample in samples))):
                alias = None
            confirmed[email] = replace(destination, switch_refusal=refusal, leased_elsewhere_until=leased_until,
                                       launcher_default_only=launcher_default_only, login_alias=alias)
    return confirmed


def account_slot(places, email):
    binding = observed_binding(places, email)
    try:
        return binding, file_stamp(binding.path).identity
    except (ValueError, OSError):
        raise ValueError("credential slot unavailable") from None


@contextmanager
def app_server(slot):
    with tempfile.TemporaryDirectory() as home, open(os.devnull, "wb") as sink:
        # Use the installed binary, a clean configuration and the native account's
        # credential slot. Normal Codex token refresh remains under its own lock.
        env = {key: os.environ[key] for key in ("HOME", "PATH", "TMPDIR") if key in os.environ}
        env.update(CODEX_HOME=home, CODEX_AUTH_PATH=str(slot))
        with subprocess.Popen(["codex", "app-server", "--stdio"], cwd=home, env=env,
                              stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=sink) as proc:
            try:
                rpc = Rpc(proc, sink)
                rpc.initialize()
                yield rpc
            finally:
                # Allow a graceful shutdown after EOF, including any pending credential write.
                try:
                    proc.communicate(timeout=60)
                except subprocess.TimeoutExpired:
                    proc.terminate()
                    try:
                        proc.communicate(timeout=5)
                    except subprocess.TimeoutExpired:
                        proc.kill()
                        proc.communicate()


def read_identity(rpc, email, *, refresh=False):
    # A forced exchange can wait 30s for the credential lock and 45s for OAuth.
    identity = rpc.request("account/read", {"refreshToken": refresh}, timeout=120 if refresh else 60)
    account = identity["account"]
    if (not isinstance(account, dict) or account.get("type") != "chatgpt"
            or not email_matches(email.strip(), account.get("email"))):
        raise ValueError("account mismatch")
    return identity


def check_routing(identity, account_id):
    routing = identity.get("workspaceRouting")
    if routing is not None and (
        not isinstance(routing, dict) or routing.get("chatgptAccountId") != account_id
    ):
        raise ValueError("quota identity mismatch")


def valid_account_id(value):
    return isinstance(value, str) and bool(value) and value.strip() == value


def provider_switch_refusal(limits):
    # Native codex_app_login::refusal_of checks the ordinary limits snapshot.
    reached = limits.get("rateLimitReachedType")
    if reached in ("workspace_owner_credits_depleted", "workspace_member_credits_depleted"):
        return "workspace credits depleted"
    if reached in ("workspace_owner_usage_limit_reached", "workspace_member_usage_limit_reached"):
        return "workspace usage limit reached"
    if limits.get("spendControlReached") is True:
        return "provider spend control reached"
    if reached is not None:
        return "provider rate limit reached"
    return None


def read_quota(rpc, identity, revision=None):
    quota = rpc.request("account/rateLimits/read", {"excludeResetCreditDetails": True})
    account_id = quota.get("accountId")
    if not valid_account_id(account_id):
        raise ValueError("invalid quota identity")
    check_routing(identity, account_id)
    allowed = quota.get("ordinaryUsageAllowed")
    if allowed is not None and not isinstance(allowed, bool):
        raise ValueError("invalid usage permission")
    limits = quota["rateLimits"]
    windows = []
    for key in ("primary", "secondary"):
        window = limits.get(key)
        if window is not None:
            windows.append({field: window[field] for field in
                            ("usedPercent", "windowDurationMins", "resetsAt")})
    account = identity["account"]
    result = {"email": account["email"], "plan": account["planType"],
              "ordinaryUsageAllowed": allowed, "windows": windows}
    individual = limits.get("individualLimit")
    remaining = individual.get("remainingPercent") if isinstance(individual, dict) else None
    # Match the native account inspector's ordinary_permission check while retaining the raw flag.
    if allowed is True and (quota.get("rateLimitUpsell") is not None or limits.get("spendControlReached") is True
                            or limits.get("rateLimitReachedType") is not None
                            or (type(remaining) is int and remaining <= 0)):
        result["ordinaryUsageConflict"] = True
    return SlotRead(result, account_id, revision, provider_switch_refusal=provider_switch_refusal(limits))


BINDING_ERROR = "enrollment no longer confirms this credential slot"


class RefreshBindingChanged(Exception):
    def __init__(self, withdrawn, baseline):
        super().__init__(BINDING_ERROR)
        self.withdrawn = withdrawn
        self.baseline = baseline


def verify(email, slot, *, defaults=False, refresh=None, sibling=None, sibling_revision=None, registry_keys=None,
           refresh_bindings=None):
    stage = "credential slot"
    baseline = None
    try:
        revision = file_stamp(slot)
        if refresh is not None:
            sibling_email, sibling = sibling
            stage = "pre-refresh identity"
            if revision != refresh.revision:
                raise ValueError("selected slot changed")
            if current_revision(sibling) != sibling_revision:
                return refresh.with_error("pre-refresh sibling check failed")
        stage = "startup"
        with app_server(slot) as rpc:
            stage = "identity"
            identity = read_identity(rpc, email)
            stage = "quota"
            read = read_quota(rpc, identity, revision)
            if refresh is None:
                if defaults:
                    read.row["freshThreadDefaults"] = check_defaults(rpc)
                return read

            stage = "pre-refresh identity"
            if read.account_id != refresh.account_id:
                raise ValueError("selected slot changed")
            baseline = read
            pair = {email: slot, sibling_email: sibling}
            # A new file on HFS+ can receive the same whole-second mtime. Cross
            # that boundary before the final binding and file checks so a fast
            # atomic renewal can still prove it published a newer credential.
            second, fraction = divmod(time.time_ns(), 1_000_000_000)
            if revision.mtime_ns == second * 1_000_000_000:
                time.sleep((1_000_000_000 - fraction) / 1_000_000_000)
            bindings = (refresh_bindings or current_bindings)(pair)
            retain_registry_keys(registry_keys, bindings)
            withdrawn = [address for address, path in pair.items()
                         if address not in bindings or bindings[address].path != path]
            if withdrawn:
                raise RefreshBindingChanged(withdrawn, baseline)
            if file_stamp(slot) != revision:
                raise ValueError("selected slot changed")
            if current_revision(sibling) != sibling_revision or revision.identity == sibling_revision.identity:
                return baseline.with_error("pre-refresh sibling check failed")
            stage = "refresh response"
            check_routing(read_identity(rpc, email, refresh=True), refresh.account_id)
            # The signed CLI can answer from its cache after an OAuth failure.
            stage = "refresh persistence"
            persisted = file_stamp(slot)
            if (persisted.identity == revision.identity or persisted.mtime_ns <= revision.mtime_ns
                    or file_stamp(sibling) != sibling_revision):
                raise ValueError("independent file replacement unconfirmed")

        stage = "fresh server"
        with app_server(slot) as rpc:
            stage = "fresh identity"
            identity = read_identity(rpc, email)
            stage = "fresh quota"
            fresh = read_quota(rpc, identity, persisted)
            if fresh.account_id != refresh.account_id:
                raise ValueError("subscription changed")
        stage = "refresh persistence"
        if file_stamp(slot) != persisted or file_stamp(sibling) != sibling_revision:
            raise ValueError("slot changed during verification")
        return fresh
    except (OSError, RuntimeError, ValueError, TypeError, KeyError, AttributeError) as exc:
        # RPC errors may include provider data. Report only the failing operation.
        error = f"{stage} check failed"
        if baseline is not None and (stage == "refresh persistence"
                                     or (stage == "refresh response" and isinstance(exc, (OSError, RuntimeError)))):
            # A refused or unwritten exchange does not erase a successful ordinary read
            # of the same file. The final pair check withdraws it if that revision changed.
            return baseline.with_error(error)
        failed = SlotRead({"email": email, "error": error})
        if stage == "quota" and refresh is None:
            # The ordinary identity read succeeded before quota failed. Only its routed
            # workspace can anchor another read after a concurrent atomic credential renewal.
            routing = identity.get("workspaceRouting")
            workspace = routing.get("chatgptAccountId") if isinstance(routing, dict) else None
            if valid_account_id(workspace):
                return failed._replace(revision=revision, retry_workspace=workspace)
        return failed


def current_revision(slot):
    try:
        return file_stamp(slot)
    except (ValueError, OSError):
        return None


def settle_reads(slots, reads, *, retry, defaults=False, labels=None, launch=None, registry_keys=None, selected=None,
                 before_retry=None):
    """Tie each provider answer to a current, independent file after all slow reads."""
    reopened = set()
    while True:
        path = None
        if labels is not None:
            try:
                path, before, first_launch = configured_accounts()
                if first_launch != launch:
                    launch = None
            except ValueError as error:
                print(error, file=sys.stderr)
                labels = {}

        # Native observations include the effective credential path, which can differ
        # from the raw roster directory after renewal. Bracket this slow call with the
        # label map and sample both files after it.
        bindings = current_bindings(slots)
        retain_registry_keys(registry_keys, bindings)
        launch = confirmed_launch(launch, bindings)
        if selected is not None:
            if path is None:
                sys.exit("The local roster could not confirm the selected Registry accounts.")
            selected.confirm(path, before, labels, bindings)
        if path is not None:
            # Native can refuse the whole roster even if its raw label/email pairs still
            # parse. Reconfirm the switch commands independently of a next-launch choice.
            try:
                final_path, final_roster, last_launch = configured_accounts()
                if selected is not None:
                    selected.confirm(final_path, final_roster, labels, bindings)
                labels = confirmed_labels(labels, bindings, before, final_roster) if path == final_path else {}
                if path != final_path or last_launch != launch:
                    launch = None
            except ValueError as error:
                if selected is not None:
                    sys.exit(str(error))
                print(error, file=sys.stderr)
                labels = {}
                launch = None
        revisions = {email: current_revision(slot) for email, slot in slots.items()}
        shared = set()
        by_file = {}
        for email, revision in revisions.items():
            if revision is not None:
                by_file.setdefault(revision.identity, []).append(email)
        for identity, aliases in by_file.items():
            if len(aliases) > 1:
                # Keep the unique original owner when other paths were redirected at it.
                owners = [email for email in aliases if reads[email].revision is not None
                          and reads[email].revision.identity == identity]
                shared.update(set(aliases) - set(owners) if len(owners) == 1 else aliases)

        anchors = {email: read.account_id or read.retry_workspace for email, read in reads.items()}
        reread = False
        for email, revision in revisions.items():
            previous = reads[email]
            expected = anchors[email]
            if expected is None:
                continue
            if revision is None:
                reads[email] = SlotRead({"email": email, "error": "credential slot unavailable"})
            elif email in shared:
                reads[email] = SlotRead({"email": email, "error": "credential slot shared with the other account"})
            elif email not in bindings or bindings[email].path != slots[email]:
                reads[email] = SlotRead({"email": email, "error": BINDING_ERROR})
            elif revision != previous.revision:
                if previous.account_id is None:
                    if revision.identity == previous.revision.identity:
                        continue
                    if any(peer != email and anchor == expected for peer, anchor in anchors.items()):
                        reads[email] = SlotRead({"email": email, "error": "workspace also reported by the other slot"})
                        continue
                if not retry or email in reopened:
                    reads[email] = SlotRead({"email": email, "error": "credential slot changed during verification"})
                    continue
                # Ordinary Codex renewal may atomically replace its own file. Verify the
                # replacement once with a fresh server and the same subscription identity.
                # This sample also established which original owner, if any, can
                # still use a file now shared by multiple paths.
                if before_retry is not None and before_retry() != revisions:
                    reads[email] = SlotRead({"email": email, "error": "credential slot changed during verification"})
                    continue
                reopened.add(email)
                fresh = verify(email, slots[email], defaults=defaults)
                if fresh.account_id not in (None, expected):
                    fresh = SlotRead({"email": email, "error": "subscription identity changed"})
                # A failed second read supplies neither a verified file nor another retry.
                reads[email] = fresh if fresh.account_id is not None else SlotRead(fresh.row)
                reread = True
        if not reread:
            return labels, launch


def distinct_addresses(emails):
    return (len(emails) >= 2 and all(isinstance(email, str) and email.strip() for email in emails)
            and len({email.strip().translate(ASCII_LOWER) for email in emails}) == len(emails))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--refresh", action="store_true",
                       help="request one OAuth refresh per slot and verify each persisted login")
    modes.add_argument("--labels-only", action="store_true",
                       help="list configured provider addresses with their /accounts use labels, "
                            "or look up two addresses in the order supplied")
    modes.add_argument("--overview", action="store_true",
                       help="show live logins, switch labels, quota and the host's next-launch choice "
                            "(two optional addresses select a pair from a larger roster)")
    parser.add_argument("--show-logins", action="store_true",
                        help="with --labels-only or --overview, include each selected slot's configured sign-in "
                             "address when it differs from its provider address")
    parser.add_argument("--defaults", action="store_true",
                        help="check each login's newest eligible Sol and Ultra in a fresh temporary thread; "
                             "can be combined with --overview")
    machines = parser.add_mutually_exclusive_group()
    machines.add_argument("--registry-machine", metavar="MACHINE",
                          help="check local accounts against the Registry; for a larger roster, a read-only check "
                               "without addresses selects the machine's two assigned accounts")
    machines.add_argument("--registry-here", action="store_true",
                          help="verify this Mac's Registry binding and read all its assigned subscriptions "
                               "(at least two), or check two explicitly named subscriptions")
    parser.add_argument("email", nargs="*", help="two distinct subscription provider addresses")
    args = parser.parse_args()
    if args.show_logins and not (args.labels_only or args.overview):
        parser.error("--show-logins requires --labels-only or --overview")
    emails = args.email
    all_assigned = args.registry_here and not emails and not args.refresh
    launch = None
    machine = args.registry_machine
    selected = None
    if machine is not None or args.registry_here:
        if machine is not None and (not machine.strip() or not machine.isprintable() or len(machine) > 128):
            parser.error("--registry-machine requires a machine label or stable key")
        if args.labels_only:
            parser.error("Registry verification cannot be used with --labels-only")
        sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
        import account_registry_probe
        read_registry = (account_registry_probe.CurrentMachineProbe().read if args.registry_here
                         else partial(account_registry_probe.probe, machine))
    if args.defaults and (args.refresh or args.labels_only):
        parser.error("--defaults checks fresh threads with the ordinary quota read or --overview")
    use_roster = args.labels_only or args.overview or ((args.defaults or machine or args.registry_here) and not emails)
    if (emails or not use_roster) and (len(emails) != 2 or not distinct_addresses(emails)):
        parser.error("supply two distinct subscription addresses")
    if args.registry_here:
        registry = read_registry(1) if emails else read_registry(2, show_logins=True)
        machine = local_registry_machine(registry, initial_overview=args if args.overview else None)
    if use_roster:
        try:
            if all_assigned:
                roster_path, configured, launch = registry_setup_roster(registry, read_registry)
            else:
                roster_path, configured, launch = configured_accounts(allow_missing=not emails)
        except ValueError as error:
            if args.labels_only or not emails:
                sys.exit(str(error))
            print(error, file=sys.stderr)
            configured = {}
        if args.labels_only:
            return show_configured_labels(configured, emails, show_logins=args.show_logins)
        if not emails:
            if not configured:
                parser.error("the local Codex roster is empty. On a Registry-managed Mac, "
                             "load its preferred accounts with 'aterm harness accounts sync alab --cli codex'.")
            emails = [slot.email for slot in configured.values()]
            if args.registry_here and args.refresh and len(configured) != 2:
                parser.error("supply the two provider addresses to refresh when the roster does not have two slots")
            if args.registry_here or (len(configured) > 2 and machine and not args.refresh):
                if not args.registry_here:
                    registry = read_registry(2, show_logins=True)
                selected = RegistrySelection.from_report(registry, roster_path)
                if not all_assigned and len(selected.accounts) != 2:
                    sys.exit("Registry selection requires exactly two currently assigned subscriptions.")
                routes = selected.destinations(configured)
                emails = list(routes)
            if not distinct_addresses(emails) or (not all_assigned and len(emails) != 2):
                parser.error("supply the two provider addresses to check when the roster "
                             "does not have exactly two distinct configured addresses")
    try:
        places = observed_accounts()
    except ValueError as error:
        sys.exit(str(error))
    if selected is not None:
        try:
            bindings = observed_bindings(places, emails)
            if len(bindings) != len(routes):
                sys.exit(selected.enrollment_guidance(routes, bindings, places, read_registry))
            path, current, current_launch = configured_accounts()
            selected.confirm(path, current, routes, bindings)
            if launch != current_launch:
                launch = None
        except ValueError as error:
            sys.exit(str(error))
    slots = {}
    initial_bindings = {}
    files = set()
    reads = {}
    for email in emails:
        try:
            binding, file_id = account_slot(places, email)
        except ValueError as error:
            reads[email] = SlotRead({"email": email, "error": str(error)})
            continue
        if file_id in files:
            sys.exit("Multiple accounts resolve to the same credential slot.")
        slots[email] = binding.path
        initial_bindings[email] = binding
        files.add(file_id)
    labels = None
    if args.overview or selected is not None:
        if selected is not None:
            labels = confirmed_labels(routes, initial_bindings, current)
        else:
            candidates = {email: switch_destination(configured, email) for email in emails}
            labels = confirmed_labels(candidates, initial_bindings)
        launch = confirmed_launch(launch, initial_bindings)
        if selected is not None and labels != routes:
            sys.exit("All Registry-selected credentials must be available for verification.")
    registry_keys = {email: binding.key for email, binding in initial_bindings.items()} if machine else None

    def confirmed_registry():
        report = read_registry(2, show_logins=True) if selected is not None else read_registry(1)
        if args.registry_here:
            local_registry_machine(report)
        if selected is not None and RegistrySelection.from_report(report, roster_path) != selected:
            sys.exit("The Registry machine or its assigned subscriptions changed during verification.")
        return report

    def confirm_selected_reads():
        confirmed_registry()
        try:
            path, roster, _ = configured_accounts()
            bindings = current_bindings(slots)
            selected.confirm(path, roster, routes, bindings)
            path, roster, _ = configured_accounts()
            selected.confirm(path, roster, routes, bindings)
            return {email: current_revision(slot) for email, slot in slots.items()}
        except (ValueError, OSError):
            sys.exit("The local roster could not confirm the selected Registry credentials.")

    addresses = list(slots)
    provider_ids = set()
    batch_size = 2
    with ThreadPoolExecutor(max_workers=batch_size) as pool:
        for start in range(0, len(addresses), batch_size):
            if start:
                # Only this Mac's implicit read selects more than two accounts.
                # Finish the previous requests before admitting the next ones.
                revisions = confirm_selected_reads()
                if None in revisions.values():
                    sys.exit("All Registry-selected credentials must be available for verification.")
                if len({revision.identity for revision in revisions.values()}) != len(slots):
                    sys.exit("Multiple selected accounts resolve to the same credential slot.")
            batch = addresses[start:start + batch_size]
            answers = pool.map(partial(verify, defaults=args.defaults), batch, [slots[email] for email in batch])
            for email, answer in zip(batch, answers):
                if answer.account_id is not None:
                    if answer.account_id in provider_ids:
                        sys.exit("The provider returned the same subscription account for both slots." if len(emails) == 2
                                 else "The provider returned the same subscription account for multiple slots.")
                    provider_ids.add(answer.account_id)
                reads[email] = answer

    def local_refresh_bindings(pair):
        report = confirmed_registry()
        bindings = current_bindings(pair)
        if selected is not None:
            path, roster, _ = configured_accounts()
            if len(roster) != 2:
                sys.exit("Refreshing without explicit provider addresses requires exactly two configured slots.")
            selected.confirm(path, roster, routes, bindings)
        if any(email not in bindings or bindings[email].key != registry_keys[email]
               or account_registry_probe.association_for(report, registry_keys[email])["status"] != "associated"
               for email in pair):
            sys.exit("The Registry could not confirm both local accounts for renewal.")
        return bindings

    renewed = set()
    if args.refresh:
        first, second = emails
        for email, other in ((first, second), (second, first)):
            settle_reads(slots, reads, retry=not renewed, registry_keys=registry_keys)
            if any(read.account_id is None for read in reads.values()):
                break
            try:
                reads[email] = verify(email, slots[email], refresh=reads[email],
                                      sibling=(other, slots[other]), sibling_revision=reads[other].revision,
                                      registry_keys=registry_keys,
                                      refresh_bindings=local_refresh_bindings if args.registry_here else None)
            except RefreshBindingChanged as error:
                reads[email] = error.baseline
                for address in error.withdrawn:
                    reads[address] = SlotRead({"email": address, "error": BINDING_ERROR})
                break
            if "error" in reads[email].row:
                break
            renewed.add(email)
    if selected is not None or args.registry_here:
        # Complete any provider re-read before the final Registry and native reads.
        labels, launch = settle_reads(slots, reads, retry=not args.refresh, defaults=args.defaults, labels=labels,
                                      launch=launch, registry_keys=registry_keys, selected=selected,
                                      before_retry=confirm_selected_reads if all_assigned else None)
    if machine:
        registry = confirmed_registry()
    labels, launch = settle_reads(slots, reads, retry=not (args.refresh or args.registry_here or selected is not None),
                                  defaults=args.defaults,
                                  labels=labels, launch=launch, registry_keys=registry_keys, selected=selected)
    check_failed = args.defaults and any(
        read.row.get("freshThreadDefaults", {}).get("verified") is not True for read in reads.values()
    )
    if machine:
        for email, read in reads.items():
            if read.account_id is None:
                association = {"status": "unverified", "reason": "provider-login-unconfirmed"}
            else:
                association = account_registry_probe.association_for(registry, registry_keys.get(email))
            read.row["registryAssociation"] = dict(association, machine=machine)
            check_failed = check_failed or association["status"] != "associated"
    if args.refresh:
        for email, read in reads.items():
            read.row["refreshVerified"] = email in renewed and read.account_id is not None
    if args.overview:
        rows = [dict(reads[email].row, loginVerified=reads[email].account_id is not None,
                     slotLabel=labels[email].label if email in labels else None,
                     launcherDefaultOnly=email in labels and labels[email].launcher_default_only,
                     switchRefusal=reads[email].provider_switch_refusal or
                     (labels[email].current_refusal() if email in labels else None))
                for email in emails]
        for email, row in zip(emails, rows):
            if args.show_logins and row["loginVerified"] and "error" not in row and email in labels:
                row["loginEmail"] = labels[email].login_alias
            if (isinstance(launch, LaunchChoice) and row["loginVerified"] and "error" not in row
                    and row["slotLabel"] is not None and email_matches(launch.email, row["email"])):
                row["nextLaunch"] = True
        return int(show_overview(rows, registry=registry if machine else None, launch=launch) or check_failed)
    rows = [reads[email].row for email in emails]
    print(json.dumps(rows, indent=2))
    return int(check_failed or any("error" in row for row in rows))


if __name__ == "__main__":
    sys.exit(main())
