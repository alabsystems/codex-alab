# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the live quota verifier's CLI against local fixture accounts."""
from argparse import Namespace
from contextlib import redirect_stdout
from copy import deepcopy
from datetime import datetime, timezone
from io import StringIO
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import unittest
import venv
from unittest.mock import patch


PROBE = Path(__file__).resolve().parents[1] / "smoke/live_account_quota.py"
EMAILS = ["first@example.test", "second@example.test"]
requires_tomllib = unittest.skipUnless(sys.version_info >= (3, 11), "local roster lookup uses tomllib")


def configured_slot(label, email, identity=None, *, default=False):
    row = {"label": label, "class": "dedicated"}
    if not default:
        row["dir"] = f"/fixture/accounts/{label}"
    if email is not None:
        row["email"] = email
    if identity is not None:
        row["identity_email"] = identity
    return "[[account]]\n" + "".join(f"{key} = {json.dumps(value)}\n" for key, value in row.items())


class LiveAccountQuotaTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        (self.root / "python3").symlink_to(sys.executable)
        self.executable("aterm", """
import json
import sys
from pathlib import Path
if sys.argv[1] == "harness":
    with Path(__file__).with_name("native-reads").open("a") as log:
        print(" ".join(sys.argv[1:]), file=log)
if sys.argv[1] in ("db", "file"):
    source = Path(__file__).with_name("registry.json")
    after = source.with_name("registry-after-provider.json")
    calls = source.with_name("first.calls")
    if after.is_file() and calls.is_file():
        source = after
    last = source.with_name("registry-after-third-provider.json")
    if last.is_file() and source.with_name("third.calls").is_file():
        source = last
    renewed = source.with_name("registry-after-refresh.json")
    if renewed.is_file() and calls.is_file() and any(json.loads(line)[2] for line in calls.read_text().splitlines()):
        source = renewed
    replies = json.loads(source.read_text())
    value = replies[" ".join(sys.argv[1:])]
    if value == "not-ready":
        print(f"aterm db {sys.argv[2]}: Commons membership is not ready for table access", file=sys.stderr)
        sys.exit(1)
    print(json.dumps(value))
elif sys.argv[1:] == ["harness", "accounts", "list", "--cli", "codex", "--json"]:
    # A final local roster check may overlap an external slot replacement.
    after = Path(__file__).with_name("first.after-roster")
    if after.exists() and after.with_name("first.calls").is_file():
        after.replace(after.with_name("first.json"))
    report = Path(__file__).with_name("report.json").read_text()
    print(report)
    for name in ("report.json", "registry.json"):
        staged = Path(__file__).with_name("after-first-list-" + name)
        if staged.exists():
            staged.replace(staged.with_name(name))
    if json.loads(report).get("refused") is not None:
        sys.exit(1)
else:
    assert sys.argv[1:] == ["harness", "accounts", "observed", "--json"]
    root = Path(__file__).parent
    for name in ("report.json", "registry.json"):
        staged = root / ("at-first-observation-" + name)
        if staged.exists():
            staged.replace(root / name)
    if (root / "first.calls").exists():
        linked = root / "during-observation-credential"
        if linked.exists() and (root / "third.calls").exists():
            linked.replace(root / "third.json")
        for staged, destination in (("during-observation.toml", "private-roster.toml"),
                                    ("during-observation.json", "places.json"),
                                    ("during-observation-report.json", "report.json")):
            if (root / staged).exists():
                (root / staged).replace(root / destination)
        for suffix in (".json", "-report.json"):
            restored = root / ("restore-after-observation" + suffix)
            if restored.exists():
                restored.replace(root / ("during-observation" + suffix))
    print((root / "places.json").read_text())
""")
        self.executable("codex", """
import json
import os
from pathlib import Path
import sys
assert sys.argv[1:] == ["app-server", "--stdio"]
# Each native directory exposes auth.json. Keep the fixture's call logs and
# replacement controls beside the backing file, whichever native path selected it.
slot = Path(os.environ["CODEX_AUTH_PATH"]).resolve()
replies = json.loads(slot.read_text())
call_log = slot.with_suffix(".calls")
if call_log.exists() and "renewSiblingOnReopen" in replies:
    sibling = slot.with_name(replies["renewSiblingOnReopen"])
    replacement = sibling.with_suffix(".ordinary")
    replacement.write_bytes(sibling.read_bytes())
    replacement.replace(sibling)
if call_log.exists() and "reopenedProviderId" in replies:
    provider_id = replies["reopenedProviderId"]
    replies["account/read"]["workspaceRouting"]["chatgptAccountId"] = provider_id
    replies["account/rateLimits/read"]["accountId"] = provider_id
for line in sys.stdin:
    msg = json.loads(line)
    method = msg["method"]
    if method != "initialized":
        forced = method == "account/read" and msg.get("params", {}).get("refreshToken") is True
        with call_log.open("a") as calls:
            print(json.dumps([os.getpid(), method, forced]), file=calls)
        if method == "account/rateLimits/read":
            params = msg["params"]
            assert set(params) == {"excludeResetCreditDetails"}
            assert params["excludeResetCreditDetails"] is True
            if replies.get("deleteDuringQuota"):
                slot.unlink()
            if "rewriteDuringQuota" in replies:
                slot.write_text(json.dumps(replies["rewriteDuringQuota"]))
            if "replaceDuringQuota" in replies:
                staging = slot.with_suffix(".ordinary")
                staging.write_text(json.dumps(replies["replaceDuringQuota"]))
                staging.replace(slot)
            preflight = slot.with_suffix(".preflight-places")
            restore = slot.with_suffix(".reread-roster")
            if preflight.exists() or restore.exists():
                quota_calls = sum(json.loads(call)[1] == method for call in call_log.read_text().splitlines())
                if quota_calls == 2:
                    if preflight.exists():
                        preflight.replace(slot.with_name("places.json"))
                    if restore.exists():
                        restore.replace(slot.with_name("private-roster.toml"))
            binding = slot.with_suffix(".next-places")
            if binding.exists():
                binding.replace(slot.with_name("places.json"))
            if replies.get("refuseQuota"):
                print(json.dumps({"id": msg["id"], "error": {"code": -32000,
                                 "message": "fixture quota refusal"}}), flush=True)
                continue
            changed = slot.with_suffix(".next-roster")
            if changed.exists():
                changed.replace(slot.with_name("private-roster.toml"))
        if method == "thread/start":
            assert msg["params"] == {"ephemeral": True}
        if forced:
            binding = slot.with_suffix(".refresh-places")
            if binding.exists():
                binding.replace(slot.with_name("places.json"))
            if "refreshError" in replies:
                print(json.dumps({"id": msg["id"], "error": {"code": -32000,
                                 "message": replies["refreshError"]}}), flush=True)
                continue
            if replies.get("touchOnRefresh"):
                later = slot.stat().st_mtime_ns + 1_000_000_000
                os.utime(slot, ns=(later, later))
            else:
                # Model the signed CLI's atomic write, including writes that
                # leave a plausible in-memory reply in the issuing process.
                for name, replacement in replies.get("refreshWrites", {slot.name: None}).items():
                    target = slot.with_name(name)
                    staging = target.with_suffix(".new")
                    staging.write_text(target.read_text() if replacement is None
                                       else json.dumps(replacement))
                    if replies.get("keepRefreshTime"):
                        old = target.stat()
                        os.utime(staging, ns=(old.st_atime_ns, old.st_mtime_ns))
                    staging.replace(target)
        print(json.dumps({"id": msg["id"], "result": replies[method]}), flush=True)
""")
        self.slots = [self.root / "first.json", self.root / "second.json"]
        self.native_dirs = [self.root / "renewed-first", self.root / ".codex"]
        for directory, slot in zip(self.native_dirs, self.slots):
            directory.mkdir()
            (directory / "auth.json").symlink_to(slot)
        self.places = [{"cli": "codex", "source": "auth-file", "agrees": True,
                        "observed": email, "place": str(directory / "auth.json")}
                       for email, directory in zip(EMAILS, self.native_dirs)]
        self.replies = []
        for i, email in enumerate(EMAILS):
            provider_id = f"private-provider-id-{i}"
            self.replies.append({
                "initialize": {},
                "account/read": {"account": {"type": "chatgpt", "email": email, "planType": "promax"},
                                 "workspaceRouting": {"chatgptAccountId": provider_id}},
                "account/rateLimits/read": {
                    "accountId": provider_id, "ordinaryUsageAllowed": i == 0,
                    "rateLimits": {"secondary": {"usedPercent": [2, 100][i],
                                                 "windowDurationMins": 10080, "resetsAt": 1800000000}},
                },
            })

    def executable(self, name, body):
        path = self.root / name
        path.write_text("#!/usr/bin/env python3\n" + body)
        path.chmod(0o700)

    def probe(self, *options, emails=EMAILS):
        (self.root / "places.json").write_text(json.dumps({"places": self.places}))
        for slot, replies in zip(self.slots, self.replies):
            slot.write_text(json.dumps(replies))
            slot.with_suffix(".calls").unlink(missing_ok=True)
        return self.invoke(*options, emails=emails)

    def invoke(self, *options, emails=EMAILS):
        return self.run_command([sys.executable, "-B", str(PROBE), *options, *emails], cwd=self.root)

    def run_command(self, command, *, cwd):
        result = subprocess.run(command, cwd=cwd,
                                env={"PATH": str(self.root), "HOME": str(self.root), "TMPDIR": str(self.root)},
                                text=True, capture_output=True, timeout=15)
        self.assertNotIn("private-provider-id", result.stdout + result.stderr)
        return result

    def calls(self, slot):
        path = slot.with_suffix(".calls")
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def forced_count(self, slot):
        return sum(forced for _, _, forced in self.calls(slot))

    # Reverse the rows and use the second subscription's address as the first
    # row's login address. The provider identity decides which label to return.
    FIRST_SLOT = configured_slot("first-slot", EMAILS[1], EMAILS[0])
    SECOND_SLOT = configured_slot("second-slot", "second-login@example.test", EMAILS[1], default=True)
    OTHER_SLOT = configured_slot("private-other-slot", "private-other@example.test")

    def write_roster(self, roster, report=None):
        path = self.root / "private-roster.toml"
        if roster is None:
            path.unlink(missing_ok=True)
        else:
            path.write_text(roster)
        header = {"schema": 3, "kind": "accounts", "cli": "codex", "refused": None, "roster": str(path)}
        if report is None and roster is not None and sys.version_info >= (3, 11):
            import tomllib

            try:
                rows = tomllib.loads(roster).get("account", [])
            except tomllib.TOMLDecodeError:
                rows = []
            if isinstance(rows, list):
                header["accounts"] = []
                for raw in rows:
                    if not isinstance(raw, dict):
                        continue
                    row = dict(raw)
                    email = row.get("identity_email", row.get("email"))
                    if email == EMAILS[0]:
                        row["dir"] = str(self.native_dirs[0])
                    elif email == EMAILS[1]:
                        row.pop("dir", None)
                    header["accounts"].append(row)
        elif report is not None:
            header.update(report)
        (self.root / "report.json").write_text(json.dumps(header))

    def label_lookup(self, roster, *, report=None, options=("--labels-only",), emails=EMAILS, expect_list=True):
        self.write_roster(roster, report)
        log = self.root / "label-commands"
        log.unlink(missing_ok=True)
        for name in ("aterm", "codex"):
            self.executable(name, """
from pathlib import Path
import sys
exe = Path(__file__)
with exe.with_name("label-commands").open("a") as log:
    print(" ".join([exe.name, *sys.argv[1:]]), file=log)
print(exe.with_name("report.json").read_text())
""")
        result = self.invoke(*options, emails=emails)
        commands = log.read_text().splitlines() if log.exists() else []
        self.assertEqual(commands, ["aterm harness accounts list --cli codex --json"] if expect_list else [])
        for private in (str(self.root), "private-roster", "private-other"):
            self.assertNotIn(private, result.stdout + result.stderr)
        return result

    @requires_tomllib
    def test_labels_only_pairs_expected_provider_emails_with_native_labels_in_input_order(self):
        result = self.label_lookup(self.OTHER_SLOT + self.SECOND_SLOT + self.FIRST_SLOT)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), [
            {"email": EMAILS[0], "slotLabel": "first-slot"},
            {"email": EMAILS[1], "slotLabel": "second-slot"},
        ])

    @requires_tomllib
    @unittest.skipUnless(Path("/bin/bash").is_file(), "the checkout launcher uses Bash")
    def test_account_labels_shortcut_uses_the_selected_python_for_local_labels_only(self):
        checkout = self.root / "checkout with spaces"
        checkout.symlink_to(PROBE.parents[2], target_is_directory=True)
        python = self.root / "selected Python"
        python.symlink_to(sys.executable)
        env = {"ALAB_PYTHON": str(python), "PATH": f"{self.root}:/usr/bin:/bin",
               "HOME": str(self.root), "PYTHONDONTWRITEBYTECODE": "1"}
        self.write_roster(configured_slot("personal", "alias@example.test", EMAILS[0])
                          + configured_slot("work", EMAILS[1]))

        def lookup(*args):
            return subprocess.run(["bash", "alab/account-labels.sh", *args], cwd=checkout,
                                  env=env, capture_output=True, text=True, timeout=15)

        rows = [{"email": EMAILS[0], "slotLabel": "personal", "loginEmail": "alias@example.test"},
                {"email": EMAILS[1], "slotLabel": "work"}]
        for args, expected in [((), rows), (tuple(reversed(EMAILS)), rows[::-1])]:
            result = lookup(*args)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout), expected)
        invalid = lookup("--refresh")
        self.assertEqual(invalid.returncode, 2, invalid.stderr)
        self.assertIn("two distinct subscription addresses", invalid.stderr)
        literal = lookup("--help", EMAILS[0])
        self.assertEqual(literal.returncode, 1, literal.stderr)
        self.assertEqual(json.loads(literal.stdout), [
            {"email": "--help", "error": "expected one configured slot for this address"}, rows[0]])
        env["ALAB_PYTHON"] = str(self.root / "unavailable Python")
        missing = lookup()
        self.assertEqual(missing.returncode, 1, missing.stderr)
        self.assertIn("no Python >= 3.11", missing.stderr)
        self.assertEqual((self.root / "native-reads").read_text().splitlines(),
                         ["harness accounts list --cli codex --json"] * 3)
        self.assertFalse(any(self.calls(slot) for slot in self.slots))

    @unittest.skipUnless(Path("/bin/bash").is_file(), "the checkout launcher uses Bash")
    def test_account_labels_help_works_without_python_or_roster_access(self):
        script = PROBE.parents[1] / "account-labels.sh"
        env = {"PATH": str(self.root), "ALAB_PYTHON": str(self.root / "unavailable Python")}
        for flag in ("-h", "--help"):
            with self.subTest(flag=flag):
                result = subprocess.run(["/bin/bash", str(script), flag], cwd=self.root, env=env,
                                        capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")
                for text in ("[FIRST_PROVIDER_EMAIL SECOND_PROVIDER_EMAIL]", "all configured Codex slots",
                             "input order", "slotLabel", "loginEmail"):
                    self.assertIn(text, result.stdout)
        self.assertFalse((self.root / "native-reads").exists())
        self.assertFalse(any(self.calls(slot) for slot in self.slots))

    @requires_tomllib
    def test_labels_only_without_addresses_lists_each_slot_in_roster_order(self):
        roster = (configured_slot("second", "private-login@example.test", EMAILS[1])
                  + configured_slot("unassigned", None)
                  + configured_slot("shared", EMAILS[1])
                  + configured_slot("first", EMAILS[0]))
        result = self.label_lookup(roster, emails=[])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), [
            {"email": EMAILS[1], "slotLabel": "second"},
            {"email": None, "slotLabel": "unassigned"},
            {"email": EMAILS[1], "slotLabel": "shared"},
            {"email": EMAILS[0], "slotLabel": "first"},
        ])
        self.assertNotIn("private-login@example.test", result.stdout + result.stderr)
        self.assertNotIn("/fixture/accounts/", result.stdout + result.stderr)

    @requires_tomllib
    def test_requested_login_addresses_stay_with_the_selected_provider_slots(self):
        inbox = "shared@example.test"
        roster = (self.OTHER_SLOT + configured_slot("second", inbox, EMAILS[1])
                  + configured_slot("first", inbox, EMAILS[0]))
        options = ("--labels-only", "--show-logins")
        result = self.label_lookup(roster, options=options)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), [
            {"email": EMAILS[0], "slotLabel": "first", "loginEmail": inbox},
            {"email": EMAILS[1], "slotLabel": "second", "loginEmail": inbox},
        ])
        # A shared sign-in address does not identify either provider subscription.
        result = self.label_lookup(roster, options=options, emails=[inbox, EMAILS[1]])
        self.assertEqual(result.returncode, 1, result.stderr)
        missing, found = json.loads(result.stdout)
        self.assertEqual(set(missing), {"email", "error"})
        self.assertEqual(found, {"email": EMAILS[1], "slotLabel": "second", "loginEmail": inbox})

    @requires_tomllib
    def test_requested_login_lookup_hides_every_ambiguous_provider_match(self):
        roster = (self.FIRST_SLOT + self.SECOND_SLOT
                  + configured_slot("duplicate", "undisclosed@example.test", EMAILS[0]))
        result = self.label_lookup(roster, options=("--labels-only", "--show-logins"))
        self.assertEqual(result.returncode, 1, result.stderr)
        ambiguous, found = json.loads(result.stdout)
        self.assertEqual(set(ambiguous), {"email", "error"})
        self.assertEqual(found, {"email": EMAILS[1], "slotLabel": "second-slot",
                                 "loginEmail": "second-login@example.test"})
        self.assertNotIn("undisclosed@example.test", result.stdout + result.stderr)

    @requires_tomllib
    def test_all_slot_login_addresses_omit_missing_blank_and_equivalent_spellings(self):
        roster = (configured_slot("different", "Kelvin@example.test", "kelvin@example.test")
                  + configured_slot("same", "KELVIN@example.test", "kelvin@example.test")
                  + configured_slot("provider-only", None, EMAILS[0])
                  + configured_slot("blank", "  ", EMAILS[1])
                  + configured_slot("fallback", "fallback@example.test"))
        result = self.label_lookup(roster, options=("--labels-only", "--show-logins"), emails=[])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), [
            {"email": "kelvin@example.test", "slotLabel": "different", "loginEmail": "Kelvin@example.test"},
            {"email": "kelvin@example.test", "slotLabel": "same"},
            {"email": EMAILS[0], "slotLabel": "provider-only"},
            {"email": EMAILS[1], "slotLabel": "blank"},
            {"email": "fallback@example.test", "slotLabel": "fallback"},
        ])

    def test_show_logins_requires_a_display_mode_before_opening_the_roster(self):
        for extra in ((), ("--refresh",), ("--defaults",), ("--registry-here",)):
            with self.subTest(extra=extra):
                result = self.label_lookup(self.FIRST_SLOT, options=("--show-logins", *extra), expect_list=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("--show-logins requires --labels-only or --overview", result.stderr)
                self.assertEqual(result.stdout, "")

    @requires_tomllib
    def test_labels_only_without_addresses_accepts_an_empty_configured_roster(self):
        result = self.label_lookup("account = []\n", emails=[])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), [])

    @requires_tomllib
    def test_missing_roster_lists_zero_slots_when_the_native_list_is_empty(self):
        result = self.label_lookup(None, emails=[], report={"accounts": []})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), [])
        self.assertFalse((self.root / "private-roster.toml").exists())

    @requires_tomllib
    def test_missing_roster_fails_for_unknown_or_nonempty_native_lists_and_address_lookups(self):
        for emails, report in (([], {}), ([], {"accounts": [{"email": EMAILS[0]}]}),
                               (EMAILS, {"accounts": []})):
            with self.subTest(emails=emails, report=report):
                result = self.label_lookup(None, emails=emails, report=report)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")

    @requires_tomllib
    def test_labels_only_retains_a_unique_match_if_the_other_alias_is_missing_or_ambiguous(self):
        duplicate = configured_slot("other-first", "another-login@example.test", EMAILS[0])
        for first in ("", self.FIRST_SLOT + duplicate):
            with self.subTest(missing=not first):
                result = self.label_lookup(self.OTHER_SLOT + first + self.SECOND_SLOT)
                self.assertEqual(result.returncode, 1, result.stderr)
                failed, good = json.loads(result.stdout)
                self.assertEqual(set(failed), {"email", "error"})
                self.assertEqual(failed["email"], EMAILS[0])
                self.assertTrue(failed["error"])
                self.assertEqual(good, {"email": EMAILS[1], "slotLabel": "second-slot"})

    @requires_tomllib
    def test_labels_only_malformed_reports_and_rosters_do_not_expose_private_metadata(self):
        valid_roster = self.FIRST_SLOT + self.SECOND_SLOT
        for roster, report in (("[private-other@example.test", None),
                               (self.OTHER_SLOT, {"roster": ["private-other@example.test"]}),
                               (valid_roster, {"refused": "private-other@example.test"}),
                               (valid_roster, {"refused": True})):
            with self.subTest(report=report):
                result = self.label_lookup(roster, report=report)
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("slotLabel", result.stdout)

    @requires_tomllib
    def test_labels_only_matches_ascii_case_without_merging_other_unicode_letters(self):
        # Unicode case-folding also turns the Kelvin sign into ASCII k.
        roster = (configured_slot("ascii", "Kelvin@EXAMPLE.test")
                  + configured_slot("kelvin-sign", "login@example.test", "KELVIN@Example.test"))
        emails = ["Kelvin@EXAMPLE.TEST", "kELVin@example.TEST"]
        result = self.label_lookup(roster, emails=emails)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), [
            {"email": emails[0], "slotLabel": "kelvin-sign"},
            {"email": emails[1], "slotLabel": "ascii"},
        ])

    def test_equivalent_requests_are_rejected_before_host_or_provider_access_in_all_modes(self):
        emails = [EMAILS[0], f"  {EMAILS[0].upper()}\t"]
        for options in ((), ("--refresh",), ("--labels-only",), ("--defaults",)):
            with self.subTest(options=options):
                result = self.label_lookup(self.FIRST_SLOT, options=options, emails=emails, expect_list=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")

    def test_modes_are_mutually_exclusive_before_host_or_provider_access(self):
        for pair in (("--labels-only", "--refresh"), ("--overview", "--refresh"), ("--overview", "--labels-only"),
                     ("--defaults", "--refresh"), ("--defaults", "--labels-only")):
            with self.subTest(pair=pair):
                result = self.label_lookup(self.FIRST_SLOT + self.SECOND_SLOT, options=pair, expect_list=False)
                self.assertEqual(result.returncode, 2)

    def test_invalid_address_counts_are_rejected_before_host_or_provider_access(self):
        addresses = EMAILS + ["third@example.test"]
        for options in ((), ("--refresh",), ("--labels-only",)):
            for count in ((1, 3) if options == ("--labels-only",) else (0, 1, 3)):
                with self.subTest(options=options, count=count):
                    result = self.label_lookup("", options=options, emails=addresses[:count], expect_list=False)
                    self.assertEqual(result.returncode, 2, result.stderr)

    def test_absent_first_row_preserves_second_accounts_verified_quota(self):
        del self.places[0]
        result = self.probe()
        self.assertEqual(result.returncode, 1)
        missing, verified = json.loads(result.stdout)
        self.assertEqual(missing["email"], EMAILS[0])
        self.assertTrue(missing["error"])
        self.assertNotIn("error", verified)
        self.assertEqual(verified["email"], EMAILS[1])
        self.assertEqual(verified["plan"], "promax")
        self.assertIs(verified["ordinaryUsageAllowed"], False)
        self.assertEqual(verified["windows"][0]["usedPercent"], 100)

    def defaults_fixture(self):
        for data, version in zip(self.replies, ("6.2", "6.10")):
            model = f"gpt-{version}-sol"
            data["model/list"] = {"data": [{"model": model, "hidden": False, "upgradeInfo": None,
                                            "supportedReasoningEfforts": [{"reasoningEffort": "ultra"}]}],
                                  "nextCursor": None}
            data["thread/start"] = {"thread": {"id": "private-thread", "ephemeral": True},
                                    "model": model, "modelProvider": "openai", "reasoningEffort": "ultra"}

    def test_defaults_use_each_serving_catalog_even_when_quota_is_exhausted(self):
        self.defaults_fixture()
        result = self.probe("--defaults")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("private-thread", result.stdout + result.stderr)
        for row, slot, version in zip(json.loads(result.stdout), self.slots, ("6.2", "6.10")):
            check = row["freshThreadDefaults"]
            self.assertIs(check["verified"], True)
            self.assertEqual(check["model"], check["expectedModel"])
            self.assertEqual((check["model"], check["reasoningEffort"]), (f"gpt-{version}-sol", "ultra"))
            methods = [method for _, method, forced in self.calls(slot)]
            self.assertIn("model/list", methods)
            self.assertEqual(methods.count("thread/start"), 1)
            self.assertNotIn("turn/start", methods)
            self.assertEqual(self.forced_count(slot), 0)

    @requires_tomllib
    def test_overview_reports_default_mismatch_separately_from_sign_in(self):
        self.defaults_fixture()
        self.replies[1]["thread/start"]["reasoningEffort"] = "low"
        self.write_roster(self.FIRST_SLOT + self.SECOND_SLOT)
        result = self.probe("--overview", "--defaults", emails=())
        self.assertEqual(result.returncode, 1, result.stderr)
        first, second = self.overview_rows(result.stdout, EMAILS)
        self.assertRegex(first, r"Fresh thread\s+gpt-6\.2-sol · ultra")
        self.assertRegex(second, r"Fresh thread\s+gpt-6\.10-sol · low")
        self.assertRegex(second, r"Expected\s+gpt-6\.10-sol · ultra")
        self.assertRegex(second, r"Sign-in\s+Verified")
        self.assertRegex(second, r"Provider usage\s+Blocked")

    def test_defaults_recheck_the_replacement_credential(self):
        self.defaults_fixture()
        fresh = deepcopy(self.replies[0])
        self.replies[0]["replaceDuringQuota"] = fresh
        self.replies[0]["thread/start"]["reasoningEffort"] = "low"
        result = self.probe("--defaults")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)[0]["freshThreadDefaults"]["reasoningEffort"], "ultra")
        self.assertEqual(sum(method == "thread/start" for _, method, _ in self.calls(self.slots[0])), 2)

    @requires_tomllib
    def test_defaults_reads_the_roster_without_addresses_and_preserves_a_verified_sibling(self):
        self.defaults_fixture()
        self.write_roster(self.SECOND_SLOT + self.FIRST_SLOT)
        self.places.pop(0)
        result = self.probe("--defaults", emails=())
        self.assertEqual(result.returncode, 1, result.stderr)
        second, first = json.loads(result.stdout)
        self.assertEqual([second["email"], first["email"]], list(reversed(EMAILS)))
        self.assertIs(second["freshThreadDefaults"]["verified"], True)
        self.assertIn("error", first)
        self.assertNotIn("freshThreadDefaults", first)
        self.assertEqual(self.calls(self.slots[0]), [])

    def overview(self, roster, *, emails=(), after=None, report=None):
        self.write_roster(roster, report)
        if after is not None:
            self.slots[0].with_suffix(".next-roster").write_text(after)
        result = self.probe("--overview", emails=emails)
        self.assertFalse(any(self.forced_count(slot) for slot in self.slots))
        for private in (str(self.root), "private-other", "second-login@example.test"):
            self.assertNotIn(private, result.stdout + result.stderr)
        return result

    def overview_rows(self, text, emails):
        lines = text.splitlines()
        positions = [next(i for i, line in enumerate(lines) if email in line) for email in emails]
        self.assertLess(positions[0], positions[1], text)
        return "\n".join(lines[positions[0]:positions[1]]), "\n".join(lines[positions[1]:])

    @requires_tomllib
    def test_overview_shows_only_a_distinct_confirmed_signin_address_on_request(self):
        alias = "first-login@example.test"
        self.places[0]["believed"] = {"label": "first-slot"}
        self.places[1]["believed"] = {"label": "second-slot"}
        self.replies[0]["account/read"]["account"]["email"] = "  " + EMAILS[0].upper() + " "
        self.write_roster(configured_slot("first-slot", alias, EMAILS[0])
                          + configured_slot("second-slot", EMAILS[1].upper(), EMAILS[1], default=True))
        result = self.probe("--overview", emails=())
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn(alias, result.stdout + result.stderr)
        self.assertNotIn("Sign-in address", result.stdout)
        result = self.probe("--overview", "--show-logins", emails=())
        self.assertEqual(result.returncode, 0, result.stderr)
        first, second = self.overview_rows(result.stdout, (EMAILS[0].upper(), EMAILS[1]))
        self.assertIn("Sign-in address  " + alias, first)
        self.assertNotIn("Sign-in address", second)

    @requires_tomllib
    def test_overview_withdraws_only_the_alias_when_the_signin_address_changes(self):
        old, new = "old-signin@example.test", "new-signin@example.test"
        for place, key in zip(self.places, ("ac-0000000000000001", "ac-0000000000000002")):
            place["believed"] = {"ac": key}
        roster = configured_slot("first-slot", old, EMAILS[0]) + self.SECOND_SLOT
        self.write_roster(roster)
        listed = json.loads((self.root / "report.json").read_text())
        for row, place in zip(listed["accounts"], self.places):
            row.update(place["believed"], mailbox="private-receiver@example.test")
        (self.root / "report.json").write_text(json.dumps(listed))
        (self.root / "during-observation.toml").write_text(roster.replace(old, new))
        listed["accounts"][0]["email"] = new
        (self.root / "during-observation-report.json").write_text(json.dumps(listed))
        result = self.probe("--overview", "--show-logins", emails=())
        self.assertEqual(result.returncode, 0, result.stderr)
        first, second = self.overview_rows(result.stdout, EMAILS)
        self.assertIn("/accounts use first-slot", first)
        self.assertNotIn("Sign-in address", first)
        self.assertIn("second-login@example.test", second)
        for private in (old, new, "private-receiver@example.test"):
            self.assertNotIn(private, result.stdout + result.stderr)

    @requires_tomllib
    def test_overview_omits_unconfirmed_aliases_without_changing_switch_or_error_status(self):
        alias = "first-login@example.test"
        roster = configured_slot("first-slot", alias, EMAILS[0]) + self.SECOND_SLOT
        for reason in ("duplicate raw alias", "native disagreement", "unconfirmed key", "provider error"):
            with self.subTest(reason=reason):
                self.write_roster(roster)
                path = self.root / "report.json"
                listed = json.loads(path.read_text())
                listed["accounts"][0]["ac"] = "ac-0000000000000001"
                self.places[0]["believed"] = {"ac": "ac-0000000000000001"}
                if reason == "duplicate raw alias":
                    (self.root / "private-roster.toml").write_text(roster + configured_slot("other", alias, "other@test.example"))
                elif reason == "native disagreement":
                    listed["accounts"][0]["email"] = "unconfirmed@example.test"
                elif reason == "unconfirmed key":
                    self.places[0].pop("believed")
                else:
                    self.replies[0]["account/read"]["account"]["email"] = EMAILS[1]
                path.write_text(json.dumps(listed))
                result = self.probe("--overview", "--show-logins")
                self.assertEqual(result.returncode, int(reason == "provider error"), result.stderr)
                first, _ = self.overview_rows(result.stdout, EMAILS)
                self.assertNotIn("Sign-in address", first)
                self.assertNotIn(alias, result.stdout + result.stderr)
                self.assertNotIn("unconfirmed@example.test", result.stdout + result.stderr)
                self.assertIn("identity check failed" if reason == "provider error" else "/accounts use first-slot", first)

    @requires_tomllib
    def test_overview_uses_provider_identity_and_roster_order_for_two_subscriptions(self):
        quota = self.replies[0]["account/rateLimits/read"]
        quota["ordinaryUsageAllowed"] = None
        quota["rateLimits"]["primary"] = {"usedPercent": 80, "windowDurationMins": 300, "resetsAt": None}
        result = self.overview(self.SECOND_SLOT + self.FIRST_SLOT)
        self.assertEqual(result.returncode, 0, result.stderr)
        blocked, unknown = self.overview_rows(result.stdout, list(reversed(EMAILS)))
        for row, label, usage, left in ((blocked, "second-slot", "Blocked", "0%"),
                                        (unknown, "first-slot", "Unknown", "98%")):
            if usage == "Blocked":
                self.assertIn(label + " · unavailable (provider usage blocked)", row)
                self.assertNotIn("/accounts use", row)
            else:
                self.assertIn("/accounts use " + label, row)
            self.assertIn(usage, row)
            self.assertRegex(row, rf"Weekly\s+{left} left")
        self.assertTrue(all(self.calls(slot) for slot in self.slots))

    def launch_report(self):
        keys = ["ac-0000000000000001", "ac-0000000000000002"]
        for place, key in zip(self.places, keys):
            place["believed"] = {"ac": key}
        return {"next": {"to": keys[0], "wall_moves": [{"to": keys[1]}]}, "accounts": [
            {"ac": keys[0], "email": EMAILS[1], "identity_email": EMAILS[0], "dir": str(self.native_dirs[0])},
            {"ac": keys[1], "email": EMAILS[1]}]}

    def write_registry(self, *, place_second=True, ready=True):
        roster = self.launch_report()
        first, second = [r["ac"] for r in roster["accounts"]]
        machine = "mc-0000000000000001"
        host = {"mc": machine, "label": "m3", "row": "live"}
        replies = {
            "db computers --registry": {"complete": True, "computers": [dict(host, seats=[])]},
            "db alab machines": {"complete": True, "machines": [dict(host, uuids=[])]},
            "db table scan alab/registry account --json": {"collection": "account", "malformed": 0, "rows": [
                {"pk": ac, "provider": "openai", "label": f"private-registry-label-{i}"}
                for i, ac in enumerate((first, second))]},
            "db alab preferences": {"complete": True, "preferences": [{"mc": machine, "ac": first, "row": "live"}]},
            "db table scan alab/registry account-obs --json": {"collection": "account-obs", "malformed": 0, "rows": []},
            f"db alab machines {machine}": dict(host, complete=True, facts=[], prefers=[{"ac": first}],
                                                places=[{"ac": second}] if place_second else []),
        }
        if not ready:
            replies = {command: "not-ready" for command in replies}
            replies["file status ai/accounts --json"] = {"file": "ai/accounts", "state": "never", "why": "custody",
                                                        "complete": False}
        (self.root / "registry.json").write_text(json.dumps(replies))
        self.write_roster(self.SECOND_SLOT + self.FIRST_SLOT, roster)

    def discovery_roster(self):
        self.write_registry()
        source = self.root / "registry.json"
        replies = json.loads(source.read_text())
        replies["db computers --registry"].update(state="live", why=None)
        native = json.loads((self.root / "report.json").read_text())
        second = native["accounts"][1]
        second.update(email="second-login@example.test", identity_email=EMAILS[1])
        native["accounts"].append({"ac": "ac-0000000000000003", "email": "private-other@example.test",
                                   "dir": str(self.root / "other")})
        raw, addresses = [], []
        for i, account in enumerate(native["accounts"], start=1):
            address = f"ad-{i:016x}"
            raw.append({"pk": account["ac"], "provider": "openai", "login": address, "row": "live", "status": "active"})
            addresses.append({"pk": address, "addr": account["email"], "row": "live"})
        for row in raw:
            replies[f"db table get alab/registry account {row['pk']} --json"] = {
                "collection": "account", "row": row, "lifecycle": "live", "raw": [], "conflicts": []}
        for name, prefix, collection, options, entries in (
            ("ai-accounts", "ac", "account", " --registry", raw), ("emails", "ad", "address", "", addresses)
        ):
            replies[f"db table scan alab/registry {collection} --json"] = {
                "collection": collection, "malformed": 0, "rows": entries}
            replies["db " + name + options] = {
                "table": name, "complete": True, "state": "live", "why": None,
                name: [{prefix: row["pk"], **{k: v for k, v in row.items() if k != "pk"}} for row in entries]}
        source.write_text(json.dumps(replies))
        return self.OTHER_SLOT + self.SECOND_SLOT + self.FIRST_SLOT, native, replies

    def registry_here(self, replies, *, machine="mc-0000000000000001", marked=True, seats=("1" * 32,), rev=1):
        replies = deepcopy(replies)
        computer, = replies["db computers --registry"]["computers"]
        computer.update(mc=machine, seats=list(seats))
        host, = replies["db alab machines"]["machines"]
        host.update(mc=machine, uuids=list(seats))
        preference, = replies["db alab preferences"]["preferences"]
        preference["mc"] = machine
        detail = replies.pop("db alab machines mc-0000000000000001")
        replies["db alab machines " + machine] = dict(detail, mc=machine, facts=[{"uuid": seat} for seat in seats])
        cells = {"seats": ", ".join(seats)}
        if marked:
            cells["here"] = "yes"
        replies["file cat lab/machines --published --json"] = {
            "file": "lab/machines", "state": "live", "why": None, "complete": True, "rev": rev,
            "columns": ["here", "seats"],
            "rows": [{"pk": machine, "live": True, "cells": cells, "values": cells}]}
        return replies

    @requires_tomllib
    def test_registry_here_selects_the_current_pair_for_two_or_more_slots(self):
        other = self.root / "other/auth.json"
        other.parent.mkdir()
        other.write_text(json.dumps(self.replies[0]))
        self.places.append(dict(self.places[0], observed="private-other@example.test", place=str(other),
                                believed={"ac": "ac-0000000000000003"}))
        for larger in (False, True):
            with self.subTest(larger=larger):
                roster, native, registry = self.discovery_roster()
                if not larger:
                    roster = self.SECOND_SLOT + self.FIRST_SLOT
                    native["accounts"].pop()
                self.write_roster(roster, native)
                (self.root / "registry.json").write_text(json.dumps(self.registry_here(registry)))
                result = self.probe("--registry-here", emails=[])
                self.assertEqual(result.returncode, 0, result.stderr)
                rows = json.loads(result.stdout)
                self.assertEqual([row["email"] for row in rows], EMAILS)
                for row in rows:
                    self.assertEqual(row["registryAssociation"]["machine"], "mc-0000000000000001")
                    self.assertEqual(row["registryAssociation"]["status"], "associated")
                self.assertFalse(self.calls(other))
                for private in ("private-other", "second-login@", "ac-000", "ad-000", str(self.root)):
                    self.assertNotIn(private, result.stdout + result.stderr)

    @requires_tomllib
    def test_registry_here_never_guesses_a_local_pair_or_an_unbound_machine(self):
        _, native, registry = self.discovery_roster()
        native["accounts"].pop()
        self.write_roster(self.FIRST_SLOT + self.SECOND_SLOT, native)
        for missing_local, emails in ((True, []), (True, EMAILS), (False, [])):
            with self.subTest(missing_local=missing_local, emails=emails):
                bound = self.registry_here(registry, marked=not missing_local)
                if not missing_local:
                    bound["db alab machines mc-0000000000000001"]["places"] = []
                (self.root / "registry.json").write_text(json.dumps(bound))
                result = self.probe("--registry-here", emails=emails)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertNotIn("@", result.stderr)
                self.assertFalse(any(self.calls(slot) for slot in self.slots))
                self.assertNotIn("accounts observed", (self.root / "native-reads").read_text()
                                 if (self.root / "native-reads").exists() else "")

    def three_assigned_slots(self):
        roster, native, registry = self.discovery_roster()
        third = native["accounts"][-1]
        third_file = self.root / "third.json"
        third_dir = Path(third["dir"])
        third_dir.mkdir()
        (third_dir / "auth.json").symlink_to(third_file)
        self.slots.append(third_file)
        reply = deepcopy(self.replies[0])
        reply["account/read"]["account"]["email"] = third["email"]
        reply["account/read"]["workspaceRouting"]["chatgptAccountId"] = "private-provider-id-third"
        reply["account/rateLimits/read"]["accountId"] = "private-provider-id-third"
        self.replies.append(reply)
        self.places.append(dict(self.places[0], observed=third["email"], place=str(third_dir / "auth.json"),
                                believed={"ac": third["ac"]}))
        registry = self.registry_here(registry)
        registry["db alab machines mc-0000000000000001"]["places"].append({"ac": third["ac"]})
        (self.root / "registry.json").write_text(json.dumps(registry))
        self.write_roster(roster, native)
        return roster, native, registry

    @requires_tomllib
    def test_registry_here_verifies_all_three_assignments_in_a_larger_roster(self):
        roster, native, _ = self.three_assigned_slots()
        fourth = "unassigned@example.test"
        native["accounts"].append({"ac": "ac-0000000000000004", "email": fourth, "dir": str(self.root / "fourth")})
        self.write_roster(roster + configured_slot("unassigned", fourth), native)
        result = self.probe("--registry-here", emails=[])
        self.assertEqual(result.returncode, 0, result.stderr)
        rows = json.loads(result.stdout)
        self.assertEqual([row["email"] for row in rows], EMAILS + [native["accounts"][2]["email"]])
        self.assertEqual([row["registryAssociation"]["status"] for row in rows], ["associated"] * 3)
        self.assertTrue(all(self.calls(path) and not self.forced_count(path) for path in self.slots))
        self.assertNotIn(fourth, result.stdout + result.stderr)
        overview = self.probe("--registry-here", "--overview", emails=[])
        self.assertEqual(overview.returncode, 0, overview.stderr)
        self.assertIn("/accounts use private-other-slot", overview.stdout)
        self.assertNotIn(fourth, overview.stdout)
        for options in (("--registry-here", "--refresh"), ("--registry-machine", "m3", "--overview")):
            with self.subTest(options=options):
                result = self.probe(*options, emails=[])
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(any(self.calls(path) for path in self.slots))
        explicit = self.probe("--registry-here", "--overview", emails=EMAILS)
        self.assertEqual(explicit.returncode, 0, explicit.stderr)
        self.assertFalse(self.calls(self.slots[2]))

    @requires_tomllib
    def test_registry_here_guides_setup_of_two_missing_placements(self):
        _, native, _ = self.three_assigned_slots()
        first, second, third = native["accounts"]
        self.write_roster(self.FIRST_SLOT, dict(native, accounts=[first]))
        result = self.probe("--registry-here", "--overview", emails=[])
        self.assertNotEqual(result.returncode, 0)
        for row in (second, third):
            self.assertIn(f"aterm harness accounts add {row['ac']} --cli codex", result.stderr)
        self.assertNotIn(first["ac"], result.stderr)
        self.assertNotIn("@", result.stderr)
        self.assertFalse(any(self.calls(path) for path in self.slots))

    @requires_tomllib
    def test_registry_here_names_all_confirmed_empty_slots(self):
        roster, native, _ = self.three_assigned_slots()
        for account, place in zip(native["accounts"][1:], self.places[1:]):
            account["condition"] = "not-enrolled"
            place.update(agrees=None, observed=None, place=str(Path(place["place"]).parent))
        self.write_roster(roster, native)
        for ambiguous in (False, True):
            with self.subTest(ambiguous=ambiguous):
                if ambiguous:
                    self.places.append(dict(self.places[2]))
                result = self.probe("--registry-here", "--overview", emails=[])
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertEqual("second-slot" in result.stderr, not ambiguous, result.stderr)
                self.assertEqual("private-other-slot" in result.stderr, not ambiguous, result.stderr)
                self.assertNotIn("first-slot", result.stderr)
                self.assertFalse(any(self.calls(path) for path in self.slots))

    @requires_tomllib
    def test_empty_slots_use_the_latest_native_sign_in_instructions(self):
        roster, native, _ = self.three_assigned_slots()
        for account, place in zip(native["accounts"][1:], self.places[1:]):
            account["condition"] = "not-enrolled"
            place.update(agrees=None, observed=None, place=str(Path(place["place"]).parent))
        native["accounts"][2]["sign_in_needed"] = "earlier private step"
        self.write_roster(roster, native)
        latest = deepcopy(native)
        latest["accounts"][1]["sign_in_needed"] = "private instruction for owner@example.test"
        latest["accounts"][2]["sign_in_needed"] = None
        (self.root / "at-first-observation-report.json").write_text(json.dumps(latest))
        result = self.probe("--registry-here", "--overview", emails=[])
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertIn("aterm harness accounts list --cli codex --json", result.stderr)
        self.assertIn(f"'sign_in_needed' for second-slot (ac={latest['accounts'][1]['ac']})", result.stderr)
        self.assertNotIn(latest["accounts"][2]["ac"], result.stderr)
        self.assertNotIn(latest["accounts"][0]["ac"], result.stderr)
        self.assertIn("Open /accounts add to check private-other-slot", result.stderr)
        self.assertNotIn("private instruction", result.stderr)
        self.assertNotIn("earlier private step", result.stderr)
        self.assertNotIn("@", result.stderr)
        self.assertFalse(any(self.calls(path) for path in self.slots))

    @requires_tomllib
    def test_registry_here_rechecks_assignments_before_each_batch_and_before_display(self):
        _, _, original = self.three_assigned_slots()
        later = deepcopy(original)
        later["db alab machines mc-0000000000000001"]["places"].pop()
        for stage, replace_login in (("after-provider", False), ("after-third-provider", False),
                                     ("after-third-provider", True)):
            with self.subTest(stage=stage, replace_login=replace_login):
                if replace_login:
                    self.replies[2]["replaceDuringQuota"] = deepcopy(self.replies[2])
                replacement = self.root / f"registry-{stage}.json"
                replacement.write_text(json.dumps(later))
                result = self.probe("--registry-here", "--overview", emails=[])
                replacement.unlink()
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertTrue(all(self.calls(path) for path in self.slots[:2]))
                third_reads = [method for _, method, _ in self.calls(self.slots[2]) if method == "account/read"]
                self.assertEqual(len(third_reads), 1 if stage == "after-third-provider" else 0)

    @requires_tomllib
    def test_registry_here_skips_queued_login_if_native_binding_changes(self):
        self.three_assigned_slots()
        changed = deepcopy(self.places)
        changed[2]["believed"] = {"ac": "ac-0000000000000004"}
        (self.root / "during-observation.json").write_text(json.dumps({"places": changed}))
        result = self.probe("--registry-here", emails=[])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertTrue(all(self.calls(path) for path in self.slots[:2]))
        self.assertFalse(self.calls(self.slots[2]))

    @requires_tomllib
    def test_enrollment_guidance_withholds_labels_on_conflicting_native_file_ownership(self):
        roster, native, _ = self.three_assigned_slots()
        native["accounts"][2]["condition"] = "not-enrolled"
        self.places[2].update(agrees=None, observed=None)
        self.places.append(dict(self.places[0], believed={"ac": "ac-0000000000000004"},
                                place=str(Path(self.places[0]["place"]).parent), observed="fourth@example.test"))
        self.write_roster(roster, native)
        result = self.probe("--registry-here", emails=[])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertNotIn("private-other-slot", result.stderr)
        self.assertFalse(any(self.calls(path) for path in self.slots))

    @requires_tomllib
    def test_late_collision_of_third_credential_keeps_its_original_owner(self):
        self.three_assigned_slots()
        # Redirect the third login to the first inode during the final native observation.
        self.slots[0].touch()
        (self.root / "during-observation-credential").hardlink_to(self.slots[0])
        result = self.probe("--registry-here", emails=[])
        self.assertNotEqual(result.returncode, 0)
        first, second, third = json.loads(result.stdout)
        self.assertNotIn("error", first)
        self.assertNotIn("error", second)
        self.assertIn("shared", third["error"])
        self.assertEqual(third["registryAssociation"]["status"], "unverified")

    @requires_tomllib
    def test_registry_here_reports_an_unpublished_machine_table_before_opening_any_account(self):
        snapshot = {"file": "lab/machines", "state": "never", "complete": False, "why": "custody"}
        for change, specific in [
            ({}, True), ({"why": "private-detail"}, True),
            ({"file": "other/table"}, False), ({"state": "other"}, False),
            ({"complete": True}, False), ({"complete": 0}, False),
        ]:
            with self.subTest(change=change):
                registry = {"file cat lab/machines --published --json": dict(snapshot, **change)}
                (self.root / "registry.json").write_text(json.dumps(registry))
                result = self.probe("--overview", "--registry-here", emails=[])
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertEqual("has never been published on this Mac" in result.stderr, specific)
                command = shlex.join([sys.executable, "alab/smoke/live_account_quota.py", "--overview"])
                self.assertEqual(result.stderr.rstrip().endswith(command), specific)
                self.assertNotIn("private-detail", result.stderr)
                self.assertFalse((self.root / "native-reads").exists())
                self.assertFalse(any(self.calls(slot) for slot in self.slots))

    @requires_tomllib
    def test_unpublished_registry_guidance_preserves_the_requested_local_check(self):
        registry = {"file cat lab/machines --published --json": {"file": "lab/machines", "state": "never",
                                                                  "complete": False}}
        (self.root / "registry.json").write_text(json.dumps(registry))
        pair = ["--first+$(private)@example.test", "o'brien@example.test"]
        for flags, emails in ((["--defaults"], []), (["--show-logins"], EMAILS),
                              (["--defaults", "--show-logins"], pair)):
            with self.subTest(flags=flags, emails=emails):
                result = self.probe("--registry-here", "--overview", *flags, "--", emails=emails)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn("independently of Registry assignments", result.stderr)
                command = shlex.split(result.stderr.splitlines()[-1])
                self.assertEqual(command, [sys.executable, "alab/smoke/live_account_quota.py", "--overview", *flags,
                                           *(["--", *emails] if emails else [])])
        result = self.probe("--registry-here", "--overview", emails=[EMAILS[0], "unsafe\x1b@example.test"])
        self.assertIn("rerun without --registry-here", result.stderr)
        self.assertNotIn("unsafe", result.stderr)
        for flags in ([], ["--defaults"], ["--refresh"]):
            result = self.probe("--registry-here", *flags, emails=EMAILS)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("has never been published", result.stderr)
            self.assertNotIn("--overview", result.stderr)
        self.assertFalse((self.root / "native-reads").exists())
        self.assertFalse(any(self.calls(slot) for slot in self.slots))
        self.write_roster(self.FIRST_SLOT + self.SECOND_SLOT)
        self.places[1]["believed"] = {"label": "second-slot"}
        # Framework Python on macOS can resolve a bare executable symlink back
        # to its install path. A venv keeps the spaced path in the child too.
        environment = self.root.resolve() / "Python with spaces"
        venv.EnvBuilder(with_pip=False, symlinks=True).create(environment)
        interpreter = environment / "bin" / "python"
        with patch.object(sys, "executable", str(interpreter)):
            stopped = self.probe("--registry-here", "--overview", "--show-logins")
        command = shlex.split(stopped.stderr.splitlines()[-1])
        self.assertEqual(command[0], str(interpreter))
        checked = self.run_command(command, cwd=PROBE.parents[2])
        self.assertEqual(checked.returncode, 0, checked.stderr)
        first, second = self.overview_rows(checked.stdout, EMAILS)
        self.assertIn("/accounts use first-slot", first)
        self.assertIn("second-login@example.test", second)

    def test_registry_here_explains_the_final_published_read(self):
        with patch.object(sys, "path", [str(PROBE.parent), *sys.path]):
            import live_account_quota as live

        live_read = {"published_file": {"state": "live", "complete": True}}
        never_read = {"published_file": {"state": "never", "complete": False}}
        failed_read = {"error": "command-failed"}
        for selector, commands, never in (
            ({"requested_here": True}, [live_read, never_read], True),
            ({"requested_here": True}, [never_read, failed_read], False),
            ({"requested_machine": "m3"}, [live_read, never_read], False),
        ):
            with self.subTest(selector=selector, commands=commands):
                report = {"status": "unavailable", "machines": [], "errors": ["current-machine-binding-unconfirmed"],
                          "commands": commands, **selector}
                overview = Namespace(email=[], defaults=False, show_logins=False)
                with self.assertRaises(SystemExit) as stopped:
                    live.local_registry_machine(report, initial_overview=overview)
                self.assertEqual("has never been published on this Mac" in str(stopped.exception), never)
                self.assertNotIn("--overview", str(stopped.exception))

    @requires_tomllib
    def test_registry_here_identifies_a_missing_registered_login_only_for_the_bound_machine(self):
        for binding in ("confirmed", "unmarked", "different seats"):
            with self.subTest(binding=binding):
                _, _, source = self.discovery_roster()
                registry = self.registry_here(source, marked=binding != "unmarked")
                # The assigned subscription exists, but neither Registry view names its sign-in address.
                del registry["db table scan alab/registry account --json"]["rows"][0]["login"]
                registry["db ai-accounts --registry"]["ai-accounts"][0]["login"] = None
                if binding == "different seats":
                    row, = registry["file cat lab/machines --published --json"]["rows"]
                    for fields in ("cells", "values"):
                        row[fields]["seats"] = "2" * 32
                (self.root / "registry.json").write_text(json.dumps(registry))

                result = self.probe("--overview", "--registry-here", emails=[])
                self.assertEqual(result.returncode, 1)
                self.assertEqual(result.stdout, "")
                if binding == "confirmed":
                    reason = "The Registry could not confirm the sign-in address for every subscription assigned to this Mac."
                else:
                    reason = "The Registry could not confirm the signed machine identity for this Mac."
                self.assertEqual(result.stderr.strip(), reason)
                self.assertFalse((self.root / "native-reads").exists())
                self.assertFalse(any(self.calls(slot) for slot in self.slots))

    @requires_tomllib
    def test_registry_here_withholds_local_guidance_when_publication_changes_after_login_reads(self):
        self.three_assigned_slots()
        after = {"file cat lab/machines --published --json": {"file": "lab/machines", "state": "never",
                                                              "complete": False}}
        (self.root / "registry-after-provider.json").write_text(json.dumps(after))
        result = self.probe("--overview", "--registry-here", emails=[])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("has never been published", result.stderr)
        self.assertNotIn("--overview", result.stderr)
        self.assertTrue(all(self.calls(slot) for slot in self.slots[:2]))
        self.assertFalse(self.calls(self.slots[2]))

    @requires_tomllib
    def test_registry_here_offers_setup_only_for_corroborated_missing_rows(self):
        for case in ("empty", "missing file", "missing placement", "missing preferred", "native-only row",
                     "file-only row", "missing native array", "malformed native row", "unknown schema",
                     "missing refusal", "missing key", "wrong key", "invalid native login", "identity alias collision",
                     "key used as label", "placement suspended", "placement newly suspended", "raw placement status",
                     "existing row paused", "registry changed", "sources changed", "native changed", "named machine", "refresh"):
            with self.subTest(case=case):
                _, native, original = self.discovery_roster()
                registry = self.registry_here(original)
                roster = self.FIRST_SLOT
                first, second, _ = native["accounts"]
                preferred, placement = first["ac"], second["ac"]
                native["accounts"] = [first]
                opts = ["--registry-here", "--overview"]
                if case in ("empty", "missing file", "native-only row", "missing native array",
                            "malformed native row", "unknown schema", "missing refusal", "named machine", "refresh"):
                    roster = None if case == "missing file" else ""
                    native["accounts"] = []
                if case == "missing preferred":
                    # Both valid logins are independent of the present row's provider identity.
                    second["identity_email"] = "another-identity@example.test"
                    roster = configured_slot("second-slot", second["email"], second["identity_email"], default=True)
                    native["accounts"] = [second]
                elif case == "native-only row":
                    native["accounts"] = [second]
                elif case == "file-only row":
                    native["accounts"] = []
                elif case == "missing native array":
                    del native["accounts"]
                elif case == "malformed native row":
                    native["accounts"] = [None]
                elif case == "unknown schema":
                    native["schema"] = 4
                elif case == "missing refusal":
                    del native["refused"]
                elif case == "missing key":
                    del first["ac"]
                elif case == "wrong key":
                    first["ac"] = placement
                elif case == "invalid native login":
                    first["email"] = "private-invalid"
                    roster = configured_slot("first-slot", first["email"], first["identity_email"])
                elif case == "identity alias collision":
                    # The missing registered alias is in use as an existing row's provider identity.
                    roster = self.SECOND_SLOT
                    native["accounts"] = [second]
                elif case == "key used as label":
                    roster = configured_slot(placement, first["email"], first["identity_email"])
                elif case == "raw placement status":
                    registry[f"db table get alab/registry account {placement} --json"]["raw"] = ["status"]
                elif case in ("placement suspended", "placement newly suspended", "existing row paused"):
                    updated = deepcopy(registry) if case == "placement newly suspended" else registry
                    held, status = (preferred, "paused") if case == "existing row paused" else (placement, "suspended")
                    for rows, field in ((updated["db table scan alab/registry account --json"]["rows"], "pk"),
                                        (updated["db ai-accounts --registry"]["ai-accounts"], "ac")):
                        next(row for row in rows if row[field] == held)["status"] = status
                    if case == "placement newly suspended":
                        (self.root / "after-first-list-registry.json").write_text(json.dumps(updated))
                elif case in ("registry changed", "sources changed"):
                    after = deepcopy(registry)
                    detail = after["db alab machines mc-0000000000000001"]
                    if case == "registry changed":
                        detail["places"] = []
                    else:
                        detail["prefers"].append({"ac": placement})
                    (self.root / "after-first-list-registry.json").write_text(json.dumps(after))
                elif case == "native changed":
                    replacement = dict(native, accounts=[])
                    (self.root / "after-first-list-report.json").write_text(json.dumps(replacement))
                elif case == "named machine":
                    opts = ["--registry-machine", "m3", "--overview"]
                elif case == "refresh":
                    opts = ["--registry-here", "--refresh"]
                self.write_roster(roster, native)
                if case == "missing refusal":
                    listing = self.root / "report.json"
                    malformed = json.loads(listing.read_text())
                    del malformed["refused"]
                    listing.write_text(json.dumps(malformed))
                (self.root / "registry.json").write_text(json.dumps(registry))
                (self.root / "native-reads").unlink(missing_ok=True)
                result = self.probe(*opts, emails=[])
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertNotIn("Traceback", result.stderr)
                expected_placement = case in ("empty", "missing file", "missing placement", "existing row paused")
                self.assertEqual(placement in result.stderr, expected_placement, result.stderr)
                if expected_placement:
                    self.assertIn(f"aterm harness accounts add {placement} --cli codex", result.stderr)
                    self.assertEqual("default login" in result.stderr, case in ("empty", "missing file"))
                    self.assertIn("Once aterm lists the slot", result.stderr)
                if case in ("empty", "missing file", "missing preferred"):
                    self.assertIn("Request this Mac's preferred subscriptions", result.stderr)
                    self.assertIn("aterm harness accounts sync alab --cli codex", result.stderr)
                elif case not in ("named machine", "refresh"):
                    self.assertNotIn("sync alab", result.stderr)
                for private in (preferred, "@", "private-other", str(self.root)):
                    self.assertNotIn(private, result.stderr)
                self.assertFalse(any(self.calls(slot) for slot in self.slots))
                self.assertNotIn("accounts observed", (self.root / "native-reads").read_text())

    @requires_tomllib
    def test_registry_here_guides_enrollment_when_only_one_local_login_is_observed(self):
        _, native, registry = self.discovery_roster()
        native["accounts"].pop()
        self.write_roster(self.FIRST_SLOT + self.SECOND_SLOT, native)
        (self.root / "registry.json").write_text(json.dumps(self.registry_here(registry)))
        self.places.pop()
        result = self.probe("--overview", "--registry-here", emails=[])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Open /accounts add", result.stderr)
        self.assertNotIn("@", result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertFalse(any(self.calls(slot) for slot in self.slots))

    @requires_tomllib
    def test_registry_enrollment_names_only_a_stable_empty_slot(self):
        original = deepcopy(self.places)
        for case in ("empty directory", "empty file", "other credential", "duplicate", "other key at file",
                     "other key at directory", "same key elsewhere", "wrong path", "wrong label",
                     "missing agrees", "missing observed", "sibling mismatch", "enrolled", "registry changed"):
            with self.subTest(case=case):
                self.places = deepcopy(original)
                _, native, registry = self.discovery_roster()
                native["accounts"][1]["condition"] = "not-enrolled"
                registry = self.registry_here(registry)
                self.write_roster(self.FIRST_SLOT + self.SECOND_SLOT, native)
                (self.root / "registry.json").write_text(json.dumps(registry))
                empty = self.places[1]
                auth_file = Path(empty["place"])
                empty.update(place=str(auth_file.parent), agrees=None, observed=None)
                if case in ("empty file", "other key at directory"):
                    empty["place"] = str(auth_file)
                if case == "other credential":
                    empty.update(agrees=False, observed="private-other@example.test")
                elif case == "duplicate":
                    self.places.append(deepcopy(empty))
                elif case in ("other key at file", "other key at directory"):
                    self.places.append(dict(empty, believed={"ac": "ac-0000000000000003"},
                                            place=str(auth_file if case == "other key at file" else auth_file.parent),
                                            agrees=True, observed="private-other@example.test"))
                elif case == "same key elsewhere":
                    self.places.append(dict(empty, place=str(self.root / "unrelated")))
                elif case == "wrong path":
                    empty["place"] = str(auth_file.parent.parent)
                elif case == "wrong label":
                    empty["believed"]["label"] = "private-other-slot"
                elif case == "missing agrees":
                    del empty["agrees"]
                elif case == "missing observed":
                    del empty["observed"]
                elif case == "sibling mismatch":
                    self.places[0]["place"] = str(self.root / "other")
                elif case == "enrolled":
                    updated = json.loads((self.root / "report.json").read_text())
                    updated["accounts"][1]["condition"] = "serving"
                    (self.root / "at-first-observation-report.json").write_text(json.dumps(updated))
                elif case == "registry changed":
                    registry["db alab machines mc-0000000000000001"]["places"] = []
                    (self.root / "at-first-observation-registry.json").write_text(json.dumps(registry))
                result = self.probe("--overview", "--registry-here", emails=[])
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual("no enrollment for slot second-slot" in result.stderr,
                                 case in ("empty directory", "empty file"), result.stderr)
                self.assertNotIn("first-slot", result.stderr)
                for private in ("@", "private-other", "ac-000", str(self.root)):
                    self.assertNotIn(private, result.stderr)
                self.assertEqual(result.stdout, "")
                self.assertFalse(any(self.calls(slot) for slot in self.slots))

    def test_registry_here_explicit_pair_can_report_a_missing_association_but_cannot_renew_it(self):
        self.write_registry(place_second=False)
        source = self.root / "registry.json"
        registry = json.loads(source.read_text())
        registry["db computers --registry"].update(state="live", why=None)
        source.write_text(json.dumps(self.registry_here(registry)))
        result = self.probe("--registry-here")
        self.assertEqual(result.returncode, 1, result.stderr)
        first, second = json.loads(result.stdout)
        self.assertEqual([first["email"], second["email"]], EMAILS)
        self.assertEqual(first["registryAssociation"]["status"], "associated")
        self.assertEqual(second["registryAssociation"], {"status": "missing", "machine": "mc-0000000000000001"})
        result = self.probe("--registry-here", "--refresh")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [0, 0])

    @requires_tomllib
    def test_registry_here_withholds_results_if_another_machine_now_has_the_same_accounts(self):
        roster, native, registry = self.discovery_roster()
        self.write_roster(roster, native)
        (self.root / "registry.json").write_text(json.dumps(self.registry_here(registry)))
        moved = self.registry_here(registry, machine="mc-0000000000000002")
        (self.root / "registry-after-provider.json").write_text(json.dumps(moved))
        for emails in ([], EMAILS):
            with self.subTest(emails=emails):
                result = self.probe("--registry-here", emails=emails)
                self.assertTrue(all(self.calls(slot) for slot in self.slots))
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn("binding changed", result.stderr)
                for private in ("@", "ac-000", "mc-000", str(self.root)):
                    self.assertNotIn(private, result.stderr)

    @requires_tomllib
    def test_registry_here_detects_replaced_seats_across_provider_reads_and_renewals(self):
        _, native, registry = self.discovery_roster()
        native["accounts"].pop()
        self.write_roster(self.SECOND_SLOT + self.FIRST_SLOT, native)
        original, replacement = "1" * 32, "2" * 32
        (self.root / "registry.json").write_text(json.dumps(self.registry_here(registry, seats=[original])))
        new_view = self.registry_here(registry, seats=[replacement])
        for emails in ([], EMAILS):
            for mode, when, expected_forced in (("--overview", "provider", [0, 0]),
                                                ("--refresh", "provider", [0, 0]),
                                                ("--refresh", "refresh", [1, 0])):
                with self.subTest(emails=emails, mode=mode, when=when):
                    changed = self.root / f"registry-after-{when}.json"
                    changed.write_text(json.dumps(new_view))
                    result = self.probe("--registry-here", mode, emails=emails)
                    changed.unlink()
                    self.assertTrue(all(self.calls(slot) for slot in self.slots))
                    self.assertEqual([self.forced_count(slot) for slot in self.slots], expected_forced)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(result.stdout, "")
                    self.assertIn("binding changed", result.stderr)
                    for private in (original, replacement, "@", "ac-000", "mc-000", str(self.root)):
                        self.assertNotIn(private, result.stderr)

    @requires_tomllib
    def test_registry_here_accepts_the_same_seats_after_reordering_and_republication(self):
        _, native, registry = self.discovery_roster()
        native["accounts"].pop()
        self.write_roster(self.SECOND_SLOT + self.FIRST_SLOT, native)
        seats = ("1" * 32, "2" * 32)
        (self.root / "registry.json").write_text(json.dumps(self.registry_here(registry, seats=seats)))
        later = self.registry_here(registry, seats=seats[::-1], rev=2)
        (self.root / "registry-after-provider.json").write_text(json.dumps(later))
        result = self.probe("--registry-here", "--refresh", emails=[])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [1, 1])
        self.assertTrue(all(row["refreshVerified"] for row in json.loads(result.stdout)))
        for seat in seats:
            self.assertNotIn(seat, result.stdout + result.stderr)

    @requires_tomllib
    def test_registry_here_renewal_only_accepts_two_implicit_slots_and_rechecks_each_renewal(self):
        roster, native, registry = self.discovery_roster()
        (self.root / "registry.json").write_text(json.dumps(self.registry_here(registry)))
        self.write_roster(roster, native)
        result = self.probe("--registry-here", "--refresh", emails=[])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertFalse(any(self.calls(slot) for slot in self.slots))
        result = self.probe("--registry-here", "--refresh")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [1, 1])
        native["accounts"].pop()
        self.write_roster(self.SECOND_SLOT + self.FIRST_SLOT, native)
        (self.root / "during-observation.toml").write_text(roster)
        result = self.probe("--registry-here", "--refresh", emails=[])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [0, 0])
        self.write_roster(self.SECOND_SLOT + self.FIRST_SLOT, native)
        result = self.probe("--registry-here", "--refresh", emails=[])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(all(row["refreshVerified"] for row in json.loads(result.stdout)))
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [1, 1])
        for when, forced in (("provider", [0, 0]), ("refresh", [1, 0])):
            with self.subTest(when=when):
                changed = self.root / f"registry-after-{when}.json"
                changed.write_text(json.dumps(self.registry_here(registry, machine="mc-0000000000000002")))
                result = self.probe("--registry-here", "--refresh")
                changed.unlink()
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn("binding changed", result.stderr)
                self.assertEqual([self.forced_count(slot) for slot in self.slots], forced)

    @requires_tomllib
    def test_registry_selects_two_crossing_login_aliases_from_larger_roster(self):
        self.replies[1]["account/rateLimits/read"]["ordinaryUsageAllowed"] = True
        roster, native, _ = self.discovery_roster()
        self.write_roster(roster, native)
        other = self.root / "other/auth.json"
        other.parent.mkdir()
        other.write_text(json.dumps(self.replies[0]))
        self.places.append(dict(self.places[0], observed="private-other@example.test", place=str(other),
                                believed={"ac": "ac-0000000000000003"}))
        result = self.probe("--overview", "--registry-machine", "m3", emails=[])
        self.assertEqual(result.returncode, 0, result.stderr)
        for row, label in zip(self.overview_rows(result.stdout, EMAILS), ("first-slot", "second-slot")):
            self.assertIn("/accounts use " + label, row)
            self.assertIn("Sign-in         Verified", row)
            self.assertIn("Associated", row)
        self.assertFalse(self.calls(other))
        for private in ("private-other", "second-login@", "ac-000", "ad-000", str(self.root)):
            self.assertNotIn(private, result.stdout + result.stderr)
        result = self.probe("--registry-machine", "m3", emails=[])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([row["email"] for row in json.loads(result.stdout)], EMAILS)
        self.assertFalse(self.calls(other))

    @requires_tomllib
    def test_registry_selection_never_guesses_a_pair_or_authorizes_forced_refresh(self):
        roster, native, registry = self.discovery_roster()
        self.write_roster(roster, native)
        for options in (("--refresh",), ("--overview",)):
            with self.subTest(options=options):
                if "--overview" in options:
                    registry["db computers --registry"] = "not-ready"
                    (self.root / "registry.json").write_text(json.dumps(registry))
                result = self.probe(*options, "--registry-machine", "m3", emails=[])
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertFalse(any(self.calls(slot) for slot in self.slots))

    @requires_tomllib
    def test_overview_honors_only_a_matching_foreign_lease_for_a_named_move(self):
        self.replies[1]["account/rateLimits/read"]["ordinaryUsageAllowed"] = True
        for scenario in ("stable", "late", "expired", "shortened", "wrong key", "key is also a label",
                         "also selected", "malformed selection", "ambiguous", "bad timestamp", "private text",
                         "other pool decision"):
            with self.subTest(scenario=scenario):
                report = self.launch_report()
                first, second = [row["ac"] for row in report["accounts"]]
                until = "leased elsewhere until 2099-11-19T20:00:00Z"
                past = "leased elsewhere until 2000-11-19T20:00:00Z"
                reason = {"expired": past, "bad timestamp": "leased elsewhere until 2099-02-31T20:00:00Z",
                          "private text": until + " private-host", "other pool decision": "the account in use"
                          }.get(scenario, until)
                claim = {"account": "ac-0000000000000099" if scenario == "wrong key" else first, "why": reason}
                report["next"] = {"to": first if scenario == "also selected" else second,
                                  "not": [] if scenario == "late" else [claim]}
                if scenario == "malformed selection":
                    report["next"]["to"] = []
                if scenario == "ambiguous":
                    report["next"]["not"].append({"account": first, "why": "another decision"})
                if scenario == "key is also a label":
                    report["accounts"].append({"label": first, "ac": None})
                self.write_roster(self.FIRST_SLOT + self.SECOND_SLOT, report)
                if scenario in ("late", "shortened"):
                    later = json.loads((self.root / "report.json").read_text())
                    later["next"]["not"] = [dict(claim, why=past if scenario == "shortened" else until)]
                    (self.root / "during-observation-report.json").write_text(json.dumps(later))

                result = self.probe("--overview", emails=())
                self.assertEqual(result.returncode, 0, result.stderr)
                first_row, second_row = self.overview_rows(result.stdout, EMAILS)
                leased = scenario in ("stable", "late")
                self.assertEqual("host reported a lease on another Mac until 2099-11-19 20:00:00 UTC" in first_row,
                                 leased)
                self.assertEqual("/accounts use first-slot" in first_row, not leased)
                self.assertIn("Sign-in         Verified", first_row)
                self.assertIn("Provider usage  Allowed", first_row)
                self.assertIn("/accounts use second-slot", second_row)
                if leased:
                    self.assertIn("Selected by aterm", second_row)
                self.assertNotIn("private-host", result.stdout + result.stderr)

    @requires_tomllib
    def test_overview_scopes_default_only_to_the_configured_launcher(self):
        for case in ("stable", "late", "cleared", "blocked", "held", "missing", "blank", "malformed"):
            with self.subTest(case=case):
                self.replies[1]["account/rateLimits/read"]["ordinaryUsageAllowed"] = case != "blocked"
                report = self.launch_report()
                own, default = report["accounts"]
                report["next"] = {"to": default["ac"], "not": [{"account": own["ac"], "why": "private-detail"}],
                                  "none": None}
                # Native serializes this typed decision's reason as text that may contain a local path.
                diagnostic = "/private/launcher is not a file here"
                if case not in ("late", "missing"):
                    report["next"]["only_default"] = {"blank": " ", "malformed": [diagnostic]}.get(case, diagnostic)
                if case == "held":
                    own["held_by"] = "private-machine"
                self.write_roster(self.FIRST_SLOT + self.SECOND_SLOT, report)
                if case in ("late", "cleared"):
                    later = json.loads((self.root / "report.json").read_text())
                    if case == "late":
                        later["next"]["only_default"] = diagnostic
                    else:
                        del later["next"]["only_default"]
                    (self.root / "during-observation-report.json").write_text(json.dumps(later))

                result = self.probe("--overview", emails=())
                self.assertEqual(result.returncode, 0, result.stderr)
                first, second = self.overview_rows(result.stdout, EMAILS)
                for label, row in (("first-slot", first), ("second-slot", second)):
                    self.assertIn("Sign-in         Verified", row)
                    if case == "held" and row == first:
                        self.assertIn("unavailable (held by host)", row)
                    elif case == "blocked" and row == second:
                        self.assertIn("unavailable (provider usage blocked)", row)
                    elif case in ("missing", "blank", "malformed"):
                        self.assertIn("/accounts use " + label, row)
                    else:
                        self.assertIn(label + " · check /accounts in the session", row)
                        self.assertIn("configured launcher uses only the default login", row)
                        self.assertNotIn("unavailable", row)
                    if case not in ("missing", "blank", "malformed"):
                        self.assertNotIn("/accounts use", row)
                self.assertIn("Selected by aterm", second)
                self.assertNotIn("private-", result.stdout + result.stderr)
                self.assertNotIn("/private/", result.stdout + result.stderr)

    @requires_tomllib
    def test_overview_withholds_manual_switches_when_rotation_is_off_even_with_a_pinned_launch(self):
        self.replies[1]["account/rateLimits/read"]["ordinaryUsageAllowed"] = True
        for timing in ("stable", "turned off", "turned on", "unpinned", "no host refusal"):
            with self.subTest(timing=timing):
                report = self.launch_report()
                off = "private-native-rotation-policy-detail"
                if timing in ("stable", "turned on", "unpinned"):
                    report["next"]["off"] = off
                if timing in ("unpinned", "no host refusal"):
                    report["next"].update(to=None, none="private-native-no-selection-detail")
                self.write_roster(self.FIRST_SLOT + self.SECOND_SLOT, report)
                if timing in ("turned off", "turned on"):
                    updated = json.loads((self.root / "report.json").read_text())
                    if timing == "turned off":
                        updated["next"]["off"] = off
                    else:
                        del updated["next"]["off"]
                    (self.root / "during-observation-report.json").write_text(json.dumps(updated))
                result = self.probe("--overview", emails=[])
                self.assertEqual(result.returncode, 0, result.stderr)
                first, second = self.overview_rows(result.stdout, EMAILS)
                for label, row in zip(("first-slot", "second-slot"), (first, second)):
                    self.assertIn("Sign-in         Verified", row)
                    self.assertIn("Provider usage  Allowed", row)
                    if timing == "no host refusal":
                        self.assertIn("/accounts use " + label, row)
                    else:
                        self.assertIn(label + " · unavailable (host account switching is off)", row)
                        self.assertNotIn("/accounts use", row)
                self.assertEqual("Selected by aterm" in first, report["next"]["to"] is not None)
                self.assertNotIn("private-native", result.stdout + result.stderr)

    @requires_tomllib
    def test_registry_overview_keeps_verified_labels_when_the_host_refuses_a_named_switch(self):
        self.replies[1]["account/rateLimits/read"]["ordinaryUsageAllowed"] = True
        cases = (
            ("raw hold", {}, "held by host"),
            ("stable", {"held_by": "m3"}, "held by host"),
            ("transient", {"held_by": "private-other-host"}, "held by host"),
            ("late", {"condition": "limited"}, "host reports a usage limit"),
            ("stable", {"condition": "signed-out"}, "host reports signed out"),
            ("stable", {"condition": "not-enrolled"}, "host reports no enrollment"),
            ("stable", {"condition": "low"}, None),
        )
        for timing, fields, refusal in cases:
            with self.subTest(timing=timing, fields=fields):
                roster, native, _ = self.discovery_roster()
                if timing == "raw hold":
                    roster = roster.replace('label = "first-slot"', 'label = "first-slot"\nheld_by = "pool"')
                if timing == "stable":
                    native["accounts"][0].update(fields)
                self.write_roster(roster, native)
                if timing in ("late", "transient"):
                    original = (self.root / "report.json").read_text()
                    changed = json.loads(original)
                    changed["accounts"][0].update(fields)
                    (self.root / "during-observation-report.json").write_text(json.dumps(changed))
                    if timing == "transient":
                        (self.root / "restore-after-observation-report.json").write_text(original)
                result = self.probe("--overview", "--registry-machine", "m3", emails=[])
                self.assertEqual(result.returncode, 0, result.stderr)
                first, second = self.overview_rows(result.stdout, EMAILS)
                self.assertIn("Sign-in         Verified", first)
                self.assertIn("Provider usage  Allowed", first)
                self.assertIn("Associated", first)
                if refusal:
                    self.assertIn(f"first-slot · unavailable ({refusal})", first)
                    self.assertNotIn("/accounts use", first)
                else:
                    self.assertIn("/accounts use first-slot", first)
                self.assertIn("/accounts use second-slot", second)
                self.assertNotIn("private-other-host", result.stdout + result.stderr)
                if timing == "transient":
                    self.assertEqual((self.root / "report.json").read_text(), original)

    @requires_tomllib
    def test_discovery_requires_unique_native_keys_aliases_and_observed_paths_before_provider_reads(self):
        for issue in ("missing key", "wrong observed key", "wrong path", "crossed native login",
                      "duplicate native login", "duplicate raw login", "three assignments"):
            with self.subTest(issue=issue):
                roster, native, registry = self.discovery_roster()
                if issue == "missing key":
                    native["accounts"][0].pop("ac")
                elif issue == "wrong observed key":
                    self.places[0]["believed"]["ac"] = native["accounts"][2]["ac"]
                elif issue == "wrong path":
                    native["accounts"][0]["dir"] = str(self.native_dirs[1])
                elif issue == "crossed native login":
                    native["accounts"][0]["email"], native["accounts"][1]["email"] = (
                        native["accounts"][1]["email"], native["accounts"][0]["email"])
                elif issue == "duplicate native login":
                    native["accounts"][2]["email"] = native["accounts"][0]["email"]
                elif issue == "duplicate raw login":
                    roster += configured_slot("another", EMAILS[1], "private-extra@example.test")
                else:
                    registry["db alab machines mc-0000000000000001"]["places"].append({"ac": native["accounts"][2]["ac"]})
                    (self.root / "registry.json").write_text(json.dumps(registry))
                self.write_roster(roster, native)
                result = self.probe("--registry-machine", "m3", emails=[])
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertFalse(any(self.calls(slot) for slot in self.slots))
                self.assertNotIn("private-", result.stderr)

    @requires_tomllib
    def test_final_discovery_withholds_the_pair_if_registry_or_local_ownership_moves(self):
        for issue in ("registry", "roster", "observation"):
            with self.subTest(issue=issue):
                roster, native, registry = self.discovery_roster()
                self.write_roster(roster, native)
                staged = self.root / "registry-after-provider.json"
                staged.unlink(missing_ok=True)
                if issue == "registry":
                    registry["db alab machines mc-0000000000000001"]["places"] = [{"ac": native["accounts"][2]["ac"]}]
                    staged.write_text(json.dumps(registry))
                elif issue == "roster":
                    (self.root / "during-observation.toml").write_text(roster.replace("second-login@", "another-login@"))
                else:
                    changed = deepcopy(self.places)
                    changed[0]["believed"]["ac"] = native["accounts"][2]["ac"]
                    (self.root / "during-observation.json").write_text(json.dumps({"places": changed}))
                result = self.probe("--overview", "--registry-machine", "m3", emails=[])
                self.assertTrue(all(self.calls(slot) for slot in self.slots))
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertNotIn("@", result.stderr)
                self.assertNotIn("ac-000", result.stderr)

    @requires_tomllib
    def test_registry_machine_reports_each_stable_key_in_implicit_roster_order(self):
        self.write_registry()
        self.defaults_fixture()
        result = self.probe("--registry-machine", "m3", "--defaults", emails=())
        self.assertEqual(result.returncode, 0, result.stderr)
        second, first = json.loads(result.stdout)
        self.assertEqual([second["email"], first["email"]], list(reversed(EMAILS)))
        for row, sources in ((first, ["machine-preference", "preference"]), (second, ["placement"])):
            self.assertEqual(row["registryAssociation"], {"status": "associated", "sources": sources, "machine": "m3"})
            self.assertTrue(row["freshThreadDefaults"]["verified"])
        self.assertNotIn("private-registry", result.stdout)
        self.assertNotIn("ac-000", result.stdout)
        self.assertFalse(second["ordinaryUsageAllowed"])

    def test_registry_machine_preserves_provider_checks_for_absent_and_unavailable_bindings(self):
        for ready in (True, False):
            with self.subTest(ready=ready):
                self.write_registry(place_second=False, ready=ready)
                result = self.probe("--registry-machine", "m3")
                self.assertEqual(result.returncode, 1, result.stderr)
                first, second = json.loads(result.stdout)
                for i, row in enumerate((first, second)):
                    self.assertEqual(row["email"], EMAILS[i])
                    self.assertNotIn("error", row)
                    self.assertIs(row["ordinaryUsageAllowed"], i == 0)
                if ready:
                    self.assertEqual(first["registryAssociation"]["status"], "associated")
                    self.assertEqual(second["registryAssociation"], {"machine": "m3", "status": "missing"})
                else:
                    for row in (first, second):
                        self.assertEqual(row["registryAssociation"], {
                            "machine": "m3", "status": "unverified", "reason": "commons-membership-not-ready"})

    @requires_tomllib
    def test_registry_machine_reports_a_separate_line_for_each_overview_row(self):
        for ready in (True, False):
            with self.subTest(ready=ready):
                self.write_registry(ready=ready)
                result = self.probe("--overview", "--registry-machine", "m3")
                self.assertEqual(result.returncode, 0 if ready else 1, result.stderr)
                binding = "The Registry has no signed machine identity recorded for the selected machine."
                self.assertEqual(result.stdout.count(binding), int(ready))
                for row in self.overview_rows(result.stdout, EMAILS):
                    line = next(line for line in row.splitlines() if line.lstrip().startswith("Registry "))
                    self.assertIn("m3", line)
                    self.assertIn("Associated" if ready else "Commons membership is not ready", line)
                    self.assertIn("Sign-in         Verified", row)
                self.assertNotIn("private-registry", result.stdout)

    def test_overview_omits_unreported_seat_counts_in_legacy_registry_results(self):
        with patch.object(sys, "path", [str(PROBE.parent), *sys.path]):
            import live_account_quota as live
        cases = (
            ({}, None),
            ({"machine_facts_seat_count": 1}, "1 signed machine identity for the selected machine."),
            ({"machine_facts_seat_count": 2}, "2 signed machine identities for the selected machine."),
        )
        for machine, expected in cases:
            with self.subTest(machine=machine):
                output = StringIO()
                with redirect_stdout(output):
                    live.show_overview([], registry={"status": "matched", "machines": [machine]})
                if expected is None:
                    self.assertNotIn("signed machine", output.getvalue())
                else:
                    self.assertIn(expected, output.getvalue())

    def test_registry_machine_remembers_a_temporarily_changed_observed_key_during_renewal(self):
        self.write_registry()
        changed = deepcopy(self.places)
        changed[0]["believed"]["ac"] = "ac-0000000000000003"
        (self.root / "during-observation.json").write_text(json.dumps({"places": changed}))
        (self.root / "restore-after-observation.json").write_text(json.dumps({"places": self.places}))
        self.replies[0]["replaceDuringQuota"] = deepcopy(self.replies[0])
        result = self.probe("--registry-machine", "m3")
        self.assertEqual(result.returncode, 1, result.stderr)
        first, second = json.loads(result.stdout)
        self.assertEqual(first["registryAssociation"], {
            "machine": "m3", "status": "unverified", "reason": "unconfirmed-account-key"})
        self.assertNotIn("error", first)
        self.assertEqual(second["registryAssociation"]["status"], "associated")
        self.assertEqual(json.loads((self.root / "places.json").read_text())["places"], self.places)

    def test_registry_machine_cannot_be_combined_with_a_local_labels_only_lookup(self):
        for options in (("--registry-machine", "m3", "--labels-only"), ("--registry-here", "--labels-only"),
                        ("--registry-here", "--registry-machine", "m3")):
            with self.subTest(options=options):
                result = self.probe(*options)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(result.stdout, "")
                self.assertFalse(any(self.calls(slot) for slot in self.slots))

    @requires_tomllib
    def test_overview_shows_next_launch_from_host_policy_and_verified_identity(self):
        report = self.launch_report()
        self.replies[0]["account/rateLimits/read"]["ordinaryUsageAllowed"] = False
        result = self.overview(self.SECOND_SLOT + self.FIRST_SLOT, report=report)
        self.assertEqual(result.returncode, 0, result.stderr)
        other, selected = self.overview_rows(result.stdout, list(reversed(EMAILS)))
        self.assertNotIn("Next launch", other)
        self.assertIn("Blocked", selected)
        self.assertRegex(selected, r"Next launch\s+Selected by aterm")
        self.assertNotIn("unconfirmed", result.stdout)

    @requires_tomllib
    def test_overview_shows_the_hosts_reconfirmed_wait_separately_from_account_resets(self):
        until = int(datetime.now().timestamp()) + 86400
        utc = datetime.fromtimestamp(until, timezone.utc)
        report = self.launch_report()
        report["next"] = {"to": None, "wait_until": until, "wait_until_rfc3339": utc.isoformat().replace("+00:00", "Z")}
        result = self.overview(self.FIRST_SLOT + self.SECOND_SLOT, report=report)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("aterm expects to wait until " + utc.astimezone().isoformat(sep=" "), result.stdout)
        self.assertIn("Provider usage  Allowed", result.stdout)
        self.assertIn("Provider usage  Blocked", result.stdout)
        self.assertNotIn("Selected by aterm", result.stdout)
        self.assertNotIn("unconfirmed", result.stdout)

    @requires_tomllib
    def test_overview_confirms_an_explicit_no_account_decision_and_keeps_its_reason_private(self):
        roster = self.FIRST_SLOT + self.SECOND_SLOT
        report = self.launch_report()
        decision = {"to": None, "wait_until": None, "none": "private pin or launcher path"}
        report["next"] = decision
        later = {"to": None, "none": "private changed reason"}
        for name, update in (("stable", None), ("changed reason", later), ("withdrawn", {"to": None}),
                             ("became a choice", {"to": report["accounts"][1]["ac"]})):
            with self.subTest(name=name):
                self.write_roster(roster, report)
                if update is not None:
                    changed = json.loads((self.root / "report.json").read_text())
                    changed["next"] = update
                    (self.root / "during-observation-report.json").write_text(json.dumps(changed))
                result = self.probe("--overview", emails=())
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual("aterm has selected no roster account for the next launch." in result.stdout,
                                 name == "stable")
                self.assertEqual("next-launch choice is unconfirmed" in result.stdout, name != "stable")
                self.assertIn("Provider usage  Allowed", result.stdout)
                self.assertIn("Provider usage  Blocked", result.stdout)
                self.assertNotIn("private", result.stdout + result.stderr)

        self.replies[0]["refuseQuota"] = True
        result = self.overview(roster, report=report)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("next-launch choice is unconfirmed", result.stdout)
        self.assertNotIn("private", result.stdout + result.stderr)

    def test_no_account_decision_requires_consistent_fields_and_distinct_identities(self):
        with patch.object(sys, "path", [str(PROBE.parent), *sys.path]):
            from live_account_quota import next_launch_choice
        report = self.launch_report()
        decision = {"to": None, "none": "private reason"}
        for fields in ({"to": report["accounts"][0]["ac"]}, {"to_note": "private note"},
                       {"wait_until_rfc3339": "2040-01-01T00:00:00Z"}, {"wait_until": 1},
                       {"none": "  "}, {"none": 1}):
            with self.subTest(fields=fields):
                self.assertIsNone(next_launch_choice(dict(report, next=dict(decision, **fields)), self.root))
        duplicate = dict(report, accounts=[*report["accounts"], report["accounts"][0]], next=decision)
        self.assertIsNone(next_launch_choice(duplicate, self.root))

    @requires_tomllib
    def test_overview_withholds_an_expired_conflicting_or_changed_host_wait(self):
        until = int(datetime.now().timestamp()) + 86400

        def wait(timestamp):
            utc = datetime.fromtimestamp(timestamp, timezone.utc)
            return {"to": None, "wait_until": timestamp, "wait_until_rfc3339": utc.isoformat().replace("+00:00", "Z")}

        roster = self.FIRST_SLOT + self.SECOND_SLOT
        for case in ("expired", "no decision", "disagreement", "contradiction", "no target field",
                     "malformed epoch", "missing key", "changed time", "changed pool", "different host choice"):
            with self.subTest(case=case):
                report = self.launch_report()
                decision = report["next"] = wait(until)
                if case == "expired":
                    decision.update(wait(1))
                elif case == "no decision":
                    decision.update(wait_until=None, wait_until_rfc3339=None, none=None)
                elif case == "disagreement":
                    decision["wait_until"] += 1000
                elif case == "contradiction":
                    decision["to"] = report["accounts"][0]["ac"]
                elif case == "no target field":
                    del decision["to"]
                elif case == "malformed epoch":
                    decision["wait_until"] = 10**40
                elif case == "missing key":
                    report["accounts"][0].pop("ac")
                else:
                    self.write_roster(roster, report)
                    changed = json.loads((self.root / "report.json").read_text())
                    if case == "changed time":
                        changed["next"] = wait(until + 10)
                    elif case == "changed pool":
                        changed["accounts"].append({"ac": "ac-0000000000000003", "email": "private-other@example.test"})
                    else:
                        changed["next"] = {"to": report["accounts"][1]["ac"]}
                    (self.root / "during-observation-report.json").write_text(json.dumps(changed))
                result = self.overview(roster, report=report)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("next-launch choice is unconfirmed", result.stdout)
                self.assertNotIn("expects to wait", result.stdout)
                self.assertNotIn("Selected by aterm", result.stdout)

    @requires_tomllib
    def test_overview_withholds_changed_or_ambiguous_next_launch(self):
        from copy import deepcopy

        self.replies[1]["account/rateLimits/read"]["ordinaryUsageAllowed"] = True
        for scenario in ("rotated", "no destination", "reassigned", "duplicate key", "malformed key",
                         "duplicate identity", "observation disagrees", "duplicate observed key"):
            with self.subTest(scenario=scenario):
                roster = self.FIRST_SLOT + self.SECOND_SLOT
                report = self.launch_report()
                self.write_roster(roster, report)
                updated = json.loads((self.root / "report.json").read_text())
                accounts = updated["accounts"]
                if scenario == "rotated":
                    updated["next"]["to"] = accounts[1]["ac"]
                elif scenario == "no destination":
                    updated["next"]["to"] = None
                elif scenario == "reassigned":
                    accounts[0]["identity_email"] = "new@example.test"
                elif scenario == "duplicate key":
                    accounts.append(dict(accounts[0], identity_email="third@example.test"))
                elif scenario == "malformed key":
                    accounts[0]["ac"] = "bad-key"
                elif scenario == "duplicate identity":
                    accounts.append(dict(accounts[0], ac="ac-0000000000000003"))
                else:
                    places = deepcopy(self.places)
                    places[0 if scenario == "observation disagrees" else 1]["believed"]["ac"] = (
                        "ac-0000000000000003" if scenario == "observation disagrees" else accounts[0]["ac"])
                    (self.root / "during-observation.json").write_text(json.dumps({"places": places}))
                (self.root / "during-observation-report.json").write_text(json.dumps(updated))
                result = self.overview(roster, report=report)
                withdraw_first = scenario in ("reassigned", "duplicate identity", "observation disagrees",
                                              "duplicate key", "malformed key")
                self.assertEqual(result.returncode, int(withdraw_first), result.stderr)
                if withdraw_first:
                    first, second = self.overview_rows(result.stdout, EMAILS)
                    self.assertNotIn("/accounts use", first)
                    self.assertIn("/accounts use second-slot", second)
                self.assertNotIn("Selected by aterm", result.stdout)
                self.assertIn("next-launch choice is unconfirmed", result.stdout)
                self.assertNotIn("new@example.test", result.stdout)

    @requires_tomllib
    def test_overview_reconciles_native_switch_paths_with_provider_reads(self):
        self.replies[1]["account/rateLimits/read"]["ordinaryUsageAllowed"] = True
        roster = self.FIRST_SLOT + self.SECOND_SLOT
        for scenario in ("stable", "effective dir changed", "raw dir changed", "listed dir never matched"):
            with self.subTest(scenario=scenario):
                report = self.launch_report()
                changed_dir = str(self.root / "reassigned")
                if scenario == "listed dir never matched":
                    report["accounts"][0]["dir"] = changed_dir
                self.write_roster(roster, report)
                if scenario == "effective dir changed":
                    updated = json.loads((self.root / "report.json").read_text())
                    updated["accounts"][0]["dir"] = changed_dir
                    (self.root / "during-observation-report.json").write_text(json.dumps(updated))
                elif scenario == "raw dir changed":
                    updated = roster.replace("/fixture/accounts/first-slot", changed_dir)
                    (self.root / "during-observation.toml").write_text(updated)

                result = self.probe("--overview", emails=())
                stable = scenario == "stable"
                self.assertEqual(result.returncode, 0 if stable else 1, result.stdout + result.stderr)
                first, second = self.overview_rows(result.stdout, EMAILS)
                self.assertEqual("/accounts use first-slot" in first, stable)
                self.assertEqual("Selected by aterm" in first, stable)
                if not stable:
                    self.assertNotIn("/accounts use", first)
                # The first account uses a renewal overlay and the second uses the
                # default login. The independent provider results survive route changes.
                self.assertIn("Sign-in         Verified", first)
                self.assertIn("98% left", first)
                self.assertIn("/accounts use second-slot", second)
                self.assertIn("Sign-in         Verified", second)
                self.assertIn("0% left", second)
                self.assertNotIn(str(self.root), result.stdout + result.stderr)

    @requires_tomllib
    def test_configured_label_lookup_survives_missing_native_rows_but_switches_are_withheld(self):
        roster = self.FIRST_SLOT + self.SECOND_SLOT
        result = self.overview(roster, report={})
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertNotIn("/accounts use", result.stdout)
        for row, left in zip(self.overview_rows(result.stdout, EMAILS), ("98%", "0%")):
            self.assertIn("Sign-in         Verified", row)
            self.assertIn(left + " left", row)
        lookup = self.label_lookup(roster, emails=EMAILS, report={})
        self.assertEqual(lookup.returncode, 0, lookup.stderr)
        self.assertEqual([row["slotLabel"] for row in json.loads(lookup.stdout)], ["first-slot", "second-slot"])

    @requires_tomllib
    def test_overview_withholds_switch_commands_when_the_final_native_roster_is_refused(self):
        roster = self.FIRST_SLOT + self.SECOND_SLOT
        for next_launch in (True, False):
            with self.subTest(next_launch=next_launch):
                report = self.launch_report()
                if not next_launch:
                    report["next"]["to"] = None
                self.write_roster(roster, report)
                refused = dict(json.loads((self.root / "report.json").read_text()),
                               accounts=[], refused="invalid account row")
                (self.root / "during-observation-report.json").write_text(json.dumps(refused))
                # Native rejects unknown row fields. The display can still parse the labels.
                (self.root / "during-observation.toml").write_text(roster + 'unknown_field = true\n')
                result = self.overview(roster, report=report)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertNotIn("/accounts use", result.stdout)
                self.assertNotIn("Selected by aterm", result.stdout)
                for row, left in zip(self.overview_rows(result.stdout, EMAILS), ("98%", "0%")):
                    self.assertRegex(row, r"Sign-in\s+Verified")
                    self.assertIn(left + " left", row)

    @requires_tomllib
    def test_overview_keeps_a_transient_roster_change_withdrawn(self):
        self.replies[1]["account/rateLimits/read"]["ordinaryUsageAllowed"] = True
        with patch.object(sys, "path", [str(PROBE.parent), *sys.path]):
            import live_account_quota as quota

        roster = self.FIRST_SLOT + self.SECOND_SLOT
        self.write_roster(roster, self.launch_report())
        (self.root / "places.json").write_text(json.dumps({"places": self.places}))
        for slot, replies in zip(self.slots, self.replies):
            slot.write_text(json.dumps(replies))
        native_sample = quota.configured_accounts
        roster_path = self.root / "private-roster.toml"
        alternate_path = self.root / "alternate-roster.toml"
        alternate_path.write_text(roster)
        report_path = self.root / "report.json"
        alternate_report = dict(json.loads(report_path.read_text()), roster=str(alternate_path))
        labels = ["first-slot", "second-slot"]
        for changed in (0, 1, "path"):
            with self.subTest(changed=changed):
                samples = 0

                def transient_sample(**kwargs):
                    nonlocal samples
                    samples += 1
                    # Change one genuine native sample and restore it before the next read.
                    if changed == "path" and samples == 2:
                        path, replacement = report_path, json.dumps(alternate_report)
                    elif changed != "path" and samples == 3:
                        path, replacement = roster_path, roster.replace(labels[changed], "renamed-slot")
                    else:
                        return native_sample(**kwargs)
                    original = path.read_bytes()
                    try:
                        path.write_text(replacement)
                        return native_sample(**kwargs)
                    finally:
                        path.write_bytes(original)

                output = StringIO()
                with (patch.object(sys, "path", [str(PROBE.parent), *sys.path]),
                      patch.object(sys, "argv", [str(PROBE), "--overview"]),
                      patch.dict("os.environ", {"PATH": str(self.root), "HOME": str(self.root)}),
                      patch.object(quota, "configured_accounts", side_effect=transient_sample),
                      redirect_stdout(output)):
                    status = quota.main()
                self.assertEqual(samples, 3)
                self.assertEqual(status, 1, output.getvalue())
                rows = self.overview_rows(output.getvalue(), EMAILS)
                for i, row in enumerate(rows):
                    self.assertIn("Sign-in         Verified", row)
                    if changed in (i, "path"):
                        self.assertIn("Slot label      unavailable", row)
                        self.assertNotIn("/accounts use", row)
                    else:
                        self.assertIn(f"Request switch  /accounts use {labels[i]}", row)
                    self.assertEqual("Selected by aterm" in row, i == 0 and changed == 1)
                self.assertNotIn("renamed-slot", output.getvalue())

    @requires_tomllib
    def test_overview_checks_permission_against_provider_denials(self):
        quota = self.replies[0]["account/rateLimits/read"]
        for key, value in (("spendControlReached", True), ("rateLimitReachedType", "rate_limit_reached"),
                           ("individualLimit", {"remainingPercent": 0}), ("rateLimitUpsell", {})):
            with self.subTest(key=key):
                container = quota if key == "rateLimitUpsell" else quota["rateLimits"]
                container[key] = value
                result = self.overview(self.FIRST_SLOT + self.SECOND_SLOT)
                self.assertEqual(result.returncode, 0, result.stderr)
                first, second = self.overview_rows(result.stdout, EMAILS)
                self.assertIn("Provider usage  Unknown", first)
                self.assertIn("Provider usage  Blocked", second)
                self.assertIn("Weekly", first)
                self.assertEqual("/accounts use first-slot" in first, key in ("rateLimitUpsell", "individualLimit"))
                del container[key]
        for reply in self.replies:
            reply["account/rateLimits/read"]["rateLimits"]["spendControlReached"] = True
        result = self.probe()
        self.assertEqual(result.returncode, 0, result.stderr)
        permitted, blocked = json.loads(result.stdout)
        self.assertIs(permitted["ordinaryUsageAllowed"], True)
        self.assertIs(permitted["ordinaryUsageConflict"], True)
        self.assertIs(blocked["ordinaryUsageAllowed"], False)
        self.assertNotIn("ordinaryUsageConflict", blocked)
        for row in (permitted, blocked):
            self.assertEqual(set(row) - {"ordinaryUsageConflict"},
                             {"email", "plan", "ordinaryUsageAllowed", "windows"})

    @requires_tomllib
    def test_overview_withholds_a_provider_refusal_until_a_fresh_read_clears_it(self):
        quota = self.replies[0]["account/rateLimits/read"]
        for allowed, fields, reason in (
            (True, {"spendControlReached": True}, "provider spend control reached"),
            (None, {"spendControlReached": True}, "provider spend control reached"),
            (None, {"rateLimitReachedType": "other"}, "provider rate limit reached"),
            (True, {"rateLimitReachedType": "workspace_member_credits_depleted", "spendControlReached": True},
             "workspace credits depleted"),
            (True, {"rateLimitReachedType": "workspace_owner_usage_limit_reached"}, "workspace usage limit reached"),
            (None, {"rateLimitReachedType": {}}, "provider rate limit reached"),
        ):
            with self.subTest(allowed=allowed, fields=fields):
                quota["ordinaryUsageAllowed"] = allowed
                quota["rateLimits"].update(fields)
                result = self.overview(self.FIRST_SLOT + self.SECOND_SLOT)
                self.assertEqual(result.returncode, 0, result.stderr)
                first, _ = self.overview_rows(result.stdout, EMAILS)
                self.assertIn(f"first-slot · unavailable ({reason})", first)
                self.assertNotIn("/accounts use", first)
                self.assertIn("Provider usage  Unknown", first)
                self.assertIn("Sign-in         Verified", first)
                for field in fields:
                    del quota["rateLimits"][field]
        quota["ordinaryUsageAllowed"] = True
        quota["rateLimits"].update(spendControlReached=False, rateLimitReachedType=None)
        result = self.overview(self.FIRST_SLOT + self.SECOND_SLOT)
        self.assertEqual(result.returncode, 0, result.stderr)
        first, _ = self.overview_rows(result.stdout, EMAILS)
        self.assertIn("/accounts use first-slot", first)
        self.assertIn("Provider usage  Allowed", first)

    @requires_tomllib
    def test_empty_overview_points_to_registry_sync_without_starting_a_provider_read(self):
        for roster in ("", None):
            with self.subTest(roster=roster):
                result = self.label_lookup(roster, report={"accounts": []}, options=("--overview",), emails=[])
                self.assertEqual(result.returncode, 2)
                self.assertIn("local Codex roster is empty", result.stderr)
                self.assertIn("aterm harness accounts sync alab --cli codex", result.stderr)
                self.assertNotIn("supply the two provider addresses", result.stderr)

    @requires_tomllib
    def test_overview_requires_an_unambiguous_pair_before_automatic_provider_reads(self):
        for roster in (self.FIRST_SLOT + self.SECOND_SLOT + self.OTHER_SLOT,
                       self.FIRST_SLOT + configured_slot("collides", EMAILS[0].upper())):
            with self.subTest(roster=roster):
                result = self.label_lookup(roster, options=("--overview",), emails=[])
                self.assertNotEqual(result.returncode, 0)

    @requires_tomllib
    def test_explicit_overview_on_a_larger_roster_retains_healthy_sibling_and_skips_unrequested_login(self):
        other = self.root / "other.json"
        self.places.append(dict(self.places[0], observed="private-other@example.test", place=str(other)))
        self.slots.append(other)
        self.replies.append(deepcopy(self.replies[0]))
        self.replies[0]["account/read"]["account"]["email"] = EMAILS[1]
        result = self.overview(self.SECOND_SLOT + self.OTHER_SLOT + self.FIRST_SLOT, emails=EMAILS)
        self.assertEqual(result.returncode, 1, result.stderr)
        failed, healthy = self.overview_rows(result.stdout, EMAILS)
        self.assertIn("identity check failed", failed)
        self.assertIn("second-slot", healthy)
        self.assertIn("Blocked", healthy)
        self.assertEqual(self.calls(other), [])

    @requires_tomllib
    def test_overview_withdraws_a_changed_or_ambiguous_switch_label_but_keeps_live_quota(self):
        for after in (configured_slot("new-slot", EMAILS[0]),
                      self.FIRST_SLOT + configured_slot("ambiguous", EMAILS[0].upper())):
            with self.subTest(after=after):
                result = self.overview(self.FIRST_SLOT + self.SECOND_SLOT, after=after + self.SECOND_SLOT)
                self.assertEqual(result.returncode, 1, result.stderr)
                changed, healthy = self.overview_rows(result.stdout, EMAILS)
                self.assertNotIn("first-slot", changed)
                self.assertNotIn("new-slot", changed)
                self.assertIn("unavailable", changed.lower())
                self.assertIn("Allowed", changed)
                self.assertIn("98%", changed)
                self.assertIn("second-slot", healthy)
                self.assertIn("Blocked", healthy)

    @requires_tomllib
    def test_live_report_withdraws_only_the_rebound_effective_slot(self):
        # A native renewal can redirect the effective directory with the raw roster unchanged.
        replacement = self.root / "replacement.json"
        replacement.write_text(json.dumps(self.replies[0]))
        for options in (("--overview",), (), ("--refresh",)):
            for observed in (dict(self.places[0], place=str(replacement)),
                             dict(self.places[0], agrees=False, observed=None)):
                with self.subTest(options=options, place=observed["observed"]):
                    self.write_roster(self.FIRST_SLOT + self.SECOND_SLOT)
                    if observed["agrees"] is False:
                        moved_roster = self.FIRST_SLOT.replace("/fixture/accounts/first-slot", "/fixture/moved")
                        self.slots[0].with_suffix(".next-roster").write_text(moved_roster + self.SECOND_SLOT)
                    self.slots[0].with_suffix(".next-places").write_text(
                        json.dumps({"places": [observed, self.places[1]]}))
                    result = self.probe(*options)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertTrue(all(slot.is_file() for slot in self.slots))
                    self.assertEqual(self.calls(replacement), [])
                    self.assertFalse(any(self.forced_count(slot) for slot in self.slots))
                    if options == ("--overview",):
                        first, second = self.overview_rows(result.stdout, EMAILS)
                        self.assertIn("Unverified", first)
                        self.assertNotIn("98% left", first)
                        self.assertIn("Verified", second)
                        self.assertIn("Blocked", second)
                    else:
                        first, second = json.loads(result.stdout)
                        self.assertIn("credential slot", first["error"])
                        self.assertNotIn("windows", first)
                        self.assertNotIn("error", second)
                        self.assertIs(second["ordinaryUsageAllowed"], False)

    def test_withdrawn_binding_during_slow_preflight_prevents_both_forced_exchanges(self):
        for removed in (0, 1):
            with self.subTest(removed=removed):
                kept = 1 - removed
                self.slots[0].with_suffix(".preflight-places").write_text(json.dumps({"places": [self.places[kept]]}))
                result = self.probe("--refresh")
                self.assertEqual(result.returncode, 1, result.stderr)
                rows = json.loads(result.stdout)
                self.assertIn("credential slot", rows[removed]["error"])
                self.assertNotIn("error", rows[kept])
                self.assertIn("windows", rows[kept])
                self.assertFalse(any(row["refreshVerified"] for row in rows))
                self.assertEqual([self.forced_count(slot) for slot in self.slots], [0, 0])

    @requires_tomllib
    def test_overview_confirms_slot_labels_across_the_final_native_observation(self):
        cases = [
            ("first-slot", True, {"label": "renamed-first"}, False),
            ("first-slot", True, {"ac": "key"}, False),
            ("first-slot", False, {"label": "contradictory"}, False),
            ("contradictory", False, {"label": "first-slot"}, False),
            ("first-slot", False, {"ac": "key"}, True),
        ]
        for initial_label, rename, belief, stable in cases:
            with self.subTest(initial=initial_label, rename=rename, belief=belief):
                self.places[0]["believed"] = {"label": initial_label}
                self.places[1]["believed"] = {"ac": "other-key"}
                after = [dict(self.places[0], believed=belief), self.places[1]]
                (self.root / "during-observation.json").write_text(json.dumps({"places": after}))
                if rename:
                    (self.root / "during-observation.toml").write_text(
                        self.FIRST_SLOT.replace("first-slot", "renamed-first") + self.SECOND_SLOT)
                result = self.overview(self.FIRST_SLOT + self.SECOND_SLOT)
                self.assertEqual(result.returncode, 0 if stable else 1, result.stderr)
                first, second = self.overview_rows(result.stdout, EMAILS)
                self.assertIn("first-slot" if stable else "unavailable", first)
                if not stable:
                    self.assertNotIn("first-slot", first)
                    self.assertNotIn("renamed-first", first)
                self.assertIn("98% left", first)
                self.assertIn("second-slot", second)
                self.assertIn("Verified", second)

    @requires_tomllib
    def test_observed_label_change_stays_withdrawn_after_an_ordinary_renewal(self):
        roster = self.FIRST_SLOT + self.SECOND_SLOT
        renamed = roster.replace("first-slot", "renamed-first")
        (self.root / "during-observation.toml").write_text(renamed)
        self.slots[0].with_suffix(".reread-roster").write_text(roster)
        self.replies[0]["replaceDuringQuota"] = dict(self.replies[0])
        result = self.overview(roster)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual((self.root / "private-roster.toml").read_text(), roster)
        first, second = self.overview_rows(result.stdout, EMAILS)
        self.assertIn("unavailable", first)
        self.assertIn("98% left", first)
        self.assertNotIn("first-slot", first)
        self.assertIn("second-slot", second)
        self.assertIn("Verified", second)

    def test_refresh_rechecks_native_bindings_between_exchanges(self):
        initial, other = self.slots
        # The first exchange leaves the second file untouched but its slot moves elsewhere.
        initial.with_suffix(".refresh-places").write_text(json.dumps({"places": [self.places[0]]}))
        result = self.probe("--refresh")
        self.assertEqual(result.returncode, 1, result.stderr)
        first, second = json.loads(result.stdout)
        self.assertTrue(first["refreshVerified"])
        self.assertIn("credential slot", second["error"])
        self.assertFalse(second["refreshVerified"])
        self.assertEqual([self.forced_count(initial), self.forced_count(other)], [1, 0])

    @requires_tomllib
    def test_overview_accepts_a_renewed_file_at_the_same_effective_path(self):
        fresh = deepcopy(self.replies[0])
        fresh["account/rateLimits/read"]["rateLimits"]["secondary"]["usedPercent"] = 23
        self.replies[0]["replaceDuringQuota"] = fresh
        result = self.overview(self.FIRST_SLOT + self.SECOND_SLOT)
        self.assertEqual(result.returncode, 0, result.stderr)
        first, second = self.overview_rows(result.stdout, EMAILS)
        self.assertIn("Verified", first)
        self.assertIn("77% left", first)
        self.assertIn("Verified", second)
        self.assertEqual(sum(method == "initialize" for _, method, _ in self.calls(self.slots[0])), 2)

    def test_live_checks_preserve_distinct_unicode_identities_and_provider_spelling(self):
        observed = ["Kelvin@exAmple.test", "KELVIN@example.TEST"]
        provider = ["KELvin@example.test", "KelVIN@example.test"]
        renewed = ["keLVin@example.TEST", "KELvin@example.TEST"]
        requests = [" \tKelvin@Example.test ", "kelvin@EXAMPLE.TEST"]
        for i, (place, reply, slot) in enumerate(zip(self.places, self.replies, self.slots)):
            place["observed"] = observed[i]
            reply["account/read"]["account"]["email"] = provider[i]
            after = deepcopy(reply)
            after["account/read"]["account"]["email"] = renewed[i]
            reply["refreshWrites"] = {slot.name: after}
        self.places.append(dict(self.places[0], observed=None))

        for options, expected in (((), provider), (("--refresh",), renewed)):
            with self.subTest(options=options):
                result = self.probe(*options, emails=requests)
                self.assertEqual(result.returncode, 0, result.stderr)
                rows = json.loads(result.stdout)
                self.assertEqual([row["email"] for row in rows], list(reversed(expected)))
                self.assertEqual([row["ordinaryUsageAllowed"] for row in rows], [False, True])
                if options:
                    self.assertTrue(all(row["refreshVerified"] for row in rows))
                self.assertEqual([self.forced_count(slot) for slot in self.slots], [int(bool(options))] * 2)

    def test_ambiguous_observations_after_case_folding_leave_the_other_account_verifiable(self):
        self.places.append(dict(self.places[0], observed=EMAILS[0].upper(),
                                place=str(self.root / "another-slot.json")))
        result = self.probe("--refresh")
        self.assertEqual(result.returncode, 1, result.stderr)
        failed, good = json.loads(result.stdout)
        self.assertEqual(failed["error"], "expected one matching enrolled account")
        self.assertEqual(good["email"], EMAILS[1])
        self.assertNotIn("error", good)
        self.assertEqual(self.calls(self.slots[0]), [])
        self.assertEqual(self.forced_count(self.slots[1]), 0)

    def test_similar_provider_alias_fails_without_losing_another_case_matched_result(self):
        emails = ["Moon@Example.test", "Moon.2@Example.test"]
        for place, email in zip(self.places, emails):
            place["observed"] = email.upper()
        for reply in self.replies:
            reply["account/read"]["account"]["email"] = emails[1].lower()

        result = self.probe("--refresh", emails=emails)
        self.assertEqual(result.returncode, 1, result.stderr)
        failed, good = json.loads(result.stdout)
        self.assertEqual(failed["error"], "identity check failed")
        self.assertEqual(failed["email"], emails[0])
        self.assertEqual(good["email"], emails[1].lower())
        self.assertIs(good["ordinaryUsageAllowed"], False)
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [0, 0])

    def test_two_observations_of_one_file_fail_before_checking_accounts(self):
        alias = self.root / "alias.json"
        alias.symlink_to(self.slots[0])
        self.places[1]["place"] = str(alias)
        for options in ((), ("--refresh",)):
            with self.subTest(options=options):
                result = self.probe(*options)
                self.assertEqual(result.returncode, 1)
                self.assertIn("same credential slot", result.stderr)
                self.assertEqual(result.stdout, "")
                self.assertEqual(self.calls(self.slots[0]), [])

    def test_different_slots_cannot_prove_one_subscription_twice(self):
        shared_id = self.replies[0]["account/rateLimits/read"]["accountId"]
        self.replies[1]["account/read"]["workspaceRouting"]["chatgptAccountId"] = shared_id
        self.replies[1]["account/rateLimits/read"]["accountId"] = shared_id
        for options in ((), ("--refresh",)):
            with self.subTest(options=options):
                result = self.probe(*options)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("same subscription account for both slots", result.stderr)
                self.assertEqual([self.forced_count(slot) for slot in self.slots], [0, 0])

    def test_blank_or_padded_provider_id_cannot_verify_a_second_subscription(self):
        other_id = self.replies[0]["account/rateLimits/read"]["accountId"]
        quota = self.replies[1]["account/rateLimits/read"]
        routing = self.replies[1]["account/read"]["workspaceRouting"]
        distinct_id = quota["accountId"]
        for account_id in ("", " \t ", f" {other_id}", f"{other_id}\t"):
            with self.subTest(account_id=account_id):
                quota["accountId"] = routing["chatgptAccountId"] = account_id
                result = self.probe("--refresh")
                self.assertEqual(result.returncode, 1, result.stderr)
                first, second = json.loads(result.stdout)
                self.assertNotIn("error", first)
                self.assertEqual(second, {"email": EMAILS[1], "error": "quota check failed",
                                          "refreshVerified": False})
                self.assertEqual([self.forced_count(slot) for slot in self.slots], [0, 0])
        quota["accountId"] = routing["chatgptAccountId"] = distinct_id
        result = self.probe("--refresh")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([row["refreshVerified"] for row in json.loads(result.stdout)], [True, True])

    def test_inconsistent_or_malformed_workspace_cannot_prove_quota(self):
        for workspace in ({"chatgptAccountId": "different"}, {}, {"chatgptAccountId": ""}):
            with self.subTest(workspace=workspace):
                self.replies[1]["account/read"]["workspaceRouting"] = workspace
                result = self.probe()
                self.assertEqual(result.returncode, 1)
                good, failed = json.loads(result.stdout)
                self.assertEqual(good["email"], EMAILS[0])
                self.assertNotIn("error", good)
                self.assertEqual(failed, {"email": EMAILS[1], "error": "quota check failed"})

    def test_exhausted_subscription_with_distinct_identity_is_verified(self):
        del self.replies[1]["account/read"]["workspaceRouting"]
        result = self.probe()
        self.assertEqual(result.returncode, 0, result.stderr)
        ready, exhausted = json.loads(result.stdout)
        self.assertEqual([ready["email"], exhausted["email"]], EMAILS)
        self.assertIs(ready["ordinaryUsageAllowed"], True)
        self.assertIs(exhausted["ordinaryUsageAllowed"], False)
        self.assertEqual(exhausted["windows"][0]["usedPercent"], 100)
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [0, 0])
        self.assertNotIn("refreshVerified", ready)
        for slot in self.slots:
            methods = [method for _, method, _ in self.calls(slot)]
            self.assertEqual(methods.count("account/rateLimits/read"), 1)

    def test_missing_usage_permission_stays_unknown_with_available_or_full_windows(self):
        self.replies[0]["account/rateLimits/read"].pop("ordinaryUsageAllowed")
        self.replies[1]["account/rateLimits/read"]["ordinaryUsageAllowed"] = None
        result = self.probe()
        self.assertEqual(result.returncode, 0, result.stderr)
        rows = json.loads(result.stdout)
        self.assertEqual([row["ordinaryUsageAllowed"] for row in rows], [None, None])
        self.assertEqual([row["windows"][0]["usedPercent"] for row in rows], [2, 100])

    def test_malformed_usage_permission_fails_the_quota_check_before_refresh(self):
        for value in ("false", 1):
            with self.subTest(value=value):
                self.replies[0]["account/rateLimits/read"]["ordinaryUsageAllowed"] = value
                result = self.probe("--refresh")
                self.assertEqual(result.returncode, 1, result.stderr)
                failed, good = json.loads(result.stdout)
                self.assertEqual(failed["error"], "quota check failed")
                self.assertIs(good["ordinaryUsageAllowed"], False)
                self.assertEqual([self.forced_count(slot) for slot in self.slots], [0, 0])

    def test_removed_slot_loses_its_provider_answer_without_affecting_the_sibling(self):
        self.replies[0]["deleteDuringQuota"] = True
        result = self.probe()
        self.assertEqual(result.returncode, 1)
        failed, healthy = json.loads(result.stdout)
        self.assertEqual(failed, {"email": EMAILS[0], "error": "credential slot unavailable"})
        self.assertIs(healthy["ordinaryUsageAllowed"], False)
        self.assertNotIn("error", healthy)
        self.assertNotIn("loginVerified", healthy)

    @requires_tomllib
    def test_failed_quota_can_recover_the_pinned_workspace_from_an_atomic_renewal(self):
        fresh = deepcopy(self.replies[0])
        fresh["account/rateLimits/read"]["rateLimits"]["secondary"]["usedPercent"] = 37
        self.replies[0].update(refuseQuota=True, replaceDuringQuota=fresh)
        self.write_roster(self.FIRST_SLOT + self.SECOND_SLOT)
        result = self.probe("--overview")
        self.assertEqual(result.returncode, 0, result.stderr)
        first, second = self.overview_rows(result.stdout, EMAILS)
        self.assertIn("Verified", first)
        self.assertIn("63% left", first)
        self.assertIn("Verified", second)
        self.assertEqual([sum(method == "initialize" for _, method, _ in self.calls(slot))
                          for slot in self.slots], [2, 1])
        self.assertFalse(any(self.forced_count(slot) for slot in self.slots))

    def test_failed_quota_recovery_requires_an_independent_pinned_workspace_and_new_inode(self):
        original = self.replies[0]
        for reason in ("no routing", "empty workspace", "identical inode", "other subscription", "sibling workspace",
                       "replacement changed twice"):
            with self.subTest(reason=reason):
                first = deepcopy(original)
                fresh = deepcopy(original)
                expected_reads = 1
                if reason == "no routing":
                    first["account/read"].pop("workspaceRouting")
                elif reason == "empty workspace":
                    first["account/read"]["workspaceRouting"]["chatgptAccountId"] = " "
                elif reason == "other subscription":
                    fresh["account/read"]["workspaceRouting"]["chatgptAccountId"] = "different-subscription"
                    fresh["account/rateLimits/read"]["accountId"] = "different-subscription"
                    expected_reads = 2
                elif reason == "sibling workspace":
                    shared = self.replies[1]["account/rateLimits/read"]["accountId"]
                    first["account/read"]["workspaceRouting"]["chatgptAccountId"] = shared
                    fresh["account/read"]["workspaceRouting"]["chatgptAccountId"] = shared
                    fresh["account/rateLimits/read"]["accountId"] = shared
                elif reason == "replacement changed twice":
                    fresh["replaceDuringQuota"] = deepcopy(original)
                    expected_reads = 2
                first["rewriteDuringQuota" if reason == "identical inode" else "replaceDuringQuota"] = fresh
                first["refuseQuota"] = True
                self.replies[0] = first
                result = self.probe("--refresh")
                self.assertEqual(result.returncode, 1, result.stderr)
                failed, healthy = json.loads(result.stdout)
                self.assertIn("error", failed)
                self.assertNotIn("windows", failed)
                self.assertNotIn("error", healthy)
                self.assertIs(healthy["ordinaryUsageAllowed"], False)
                self.assertFalse(any(self.forced_count(slot) for slot in self.slots))
                self.assertEqual(sum(method == "initialize" for _, method, _ in self.calls(self.slots[0])),
                                 expected_reads)

    def test_ordinary_replacement_reopens_once_without_forcing_and_uses_the_fresh_quota(self):
        fresh = deepcopy(self.replies[0])
        fresh["account/rateLimits/read"]["rateLimits"]["secondary"]["usedPercent"] = 31
        self.replies[0]["replaceDuringQuota"] = fresh
        result = self.probe()
        self.assertEqual(result.returncode, 0, result.stderr)
        first, second = json.loads(result.stdout)
        self.assertEqual(first["windows"][0]["usedPercent"], 31)
        self.assertEqual(second["windows"][0]["usedPercent"], 100)
        for slot, expected in zip(self.slots, [2, 1]):
            calls = self.calls(slot)
            self.assertEqual(len({pid for pid, method, _ in calls if method == "account/read"}), expected)
            self.assertEqual(len({pid for pid, method, _ in calls if method == "account/rateLimits/read"}), expected)
            self.assertEqual(self.forced_count(slot), 0)

    def test_a_second_replacement_or_another_subscription_cannot_inherit_the_first_read(self):
        original = deepcopy(self.replies[0])
        for changed_owner in (False, True):
            with self.subTest(changed_owner=changed_owner):
                replacement = deepcopy(original)
                if changed_owner:
                    for fields, key in [(replacement["account/read"]["workspaceRouting"], "chatgptAccountId"),
                                        (replacement["account/rateLimits/read"], "accountId")]:
                        fields[key] = "private-provider-id-reassigned"
                else:
                    replacement["replaceDuringQuota"] = original
                self.replies[0]["replaceDuringQuota"] = replacement
                result = self.probe()
                self.assertEqual(result.returncode, 1)
                failed, healthy = json.loads(result.stdout)
                self.assertIn("error", failed)
                self.assertNotIn("windows", failed)
                self.assertIs(healthy["ordinaryUsageAllowed"], False)
                self.assertEqual(len({pid for pid, _, _ in self.calls(self.slots[0])}), 2)
                self.assertEqual([self.forced_count(slot) for slot in self.slots], [0, 0])

    @requires_tomllib
    def test_overview_rechecks_slots_after_the_last_roster_lookup(self):
        # The two independent servers have already replied when the first label's slot
        # starts pointing at its sibling. The original sibling evidence remains valid.
        self.slots[0].with_suffix(".after-roster").symlink_to(self.slots[1])
        result = self.overview(self.FIRST_SLOT + self.SECOND_SLOT)
        self.assertEqual(result.returncode, 1)
        first, second = self.overview_rows(result.stdout, EMAILS)
        self.assertRegex(first, r"Sign-in\s+Unverified")
        self.assertRegex(first, r"Provider usage\s+Unknown")
        self.assertNotIn("98%", first)
        self.assertRegex(second, r"Sign-in\s+Verified")
        self.assertRegex(second, r"Provider usage\s+Blocked")
        self.assertIn("0% left", second)
        for slot in self.slots:
            self.assertEqual(len({pid for pid, _, _ in self.calls(slot)}), 1)

    def test_refresh_rechecks_the_sibling_after_waiting_for_a_whole_second_timestamp(self):
        with patch.object(sys, "path", [str(PROBE.parent), *sys.path]):
            import live_account_quota as quota

        (self.root / "places.json").write_text(json.dumps({"places": self.places}))
        for slot, replies in zip(self.slots, self.replies):
            slot.write_text(json.dumps(replies))
        selected, sibling = (directory / "auth.json" for directory in self.native_dirs)
        timestamp = 1_800_000_000_000_000_000
        os.utime(selected, ns=(timestamp, timestamp))
        previous = quota.SlotRead({"email": EMAILS[0]}, self.replies[0]["account/rateLimits/read"]["accountId"],
                                  quota.file_stamp(selected))
        with (patch.dict(os.environ, {"PATH": str(self.root), "HOME": str(self.root), "TMPDIR": str(self.root)}),
              patch.object(quota, "time") as clock):
            clock.time_ns.return_value = timestamp + 250_000_000
            clock.sleep.side_effect = lambda _delay: self.slots[1].unlink()
            read = quota.verify(EMAILS[0], selected, refresh=previous, sibling=(EMAILS[1], sibling),
                                sibling_revision=quota.file_stamp(sibling))
        clock.sleep.assert_called_once_with(.75)
        self.assertEqual(read.row["error"], "pre-refresh sibling check failed")
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [0, 0])

    def test_a_sibling_disappearing_before_refresh_leaves_the_selected_read_intact(self):
        with patch.object(sys, "path", [str(PROBE.parent), *sys.path]):
            import live_account_quota as quota
        selected, sibling = self.slots
        selected.touch()
        sibling.touch()
        previous = quota.SlotRead({"email": EMAILS[0], "ordinaryUsageAllowed": True},
                                  "fixture-account", quota.file_stamp(selected))
        sibling_revision = quota.file_stamp(sibling)
        sibling.unlink()
        with patch.object(quota, "app_server", side_effect=AssertionError("refresh server opened")):
            read = quota.verify(EMAILS[0], selected, refresh=previous, sibling=(EMAILS[1], sibling),
                                sibling_revision=sibling_revision)
        self.assertEqual(read.revision, previous.revision)
        self.assertEqual(read.account_id, previous.account_id)
        self.assertIs(read.row["ordinaryUsageAllowed"], True)
        self.assertEqual(read.row["error"], "pre-refresh sibling check failed")

    def test_a_sibling_renewing_during_preflight_leaves_the_selected_quota_intact(self):
        self.replies[0]["renewSiblingOnReopen"] = self.slots[1].name
        result = self.probe("--refresh")
        self.assertEqual(result.returncode, 1)
        selected, changed = json.loads(result.stdout)
        self.assertIs(selected["refreshVerified"], False)
        self.assertIs(selected["ordinaryUsageAllowed"], True)
        self.assertEqual(selected["windows"][0]["usedPercent"], 2)
        self.assertEqual(selected["error"], "pre-refresh sibling check failed")
        self.assertIs(changed["refreshVerified"], False)
        self.assertNotIn("ordinaryUsageAllowed", changed)
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [0, 0])

    def test_a_refused_exchange_preserves_the_unchanged_files_ordinary_quota(self):
        self.replies[1]["refreshError"] = "private-provider-id-refusal"
        result = self.probe("--refresh")
        self.assertEqual(result.returncode, 1)
        good, refused = json.loads(result.stdout)
        self.assertIs(good["refreshVerified"], True)
        self.assertIs(refused["refreshVerified"], False)
        self.assertEqual(refused["error"], "refresh response check failed")
        self.assertIs(refused["ordinaryUsageAllowed"], False)
        self.assertEqual(refused["windows"][0]["usedPercent"], 100)
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [1, 1])

    def test_a_second_exchange_that_replaces_its_sibling_withdraws_both_renewal_claims(self):
        self.replies[1]["refreshWrites"] = {slot.name: None for slot in self.slots}
        result = self.probe("--refresh")
        self.assertEqual(result.returncode, 1)
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [1, 1])
        for row in json.loads(result.stdout):
            self.assertIn("error", row)
            self.assertIs(row["refreshVerified"], False)
            self.assertNotIn("ordinaryUsageAllowed", row)

    def test_refresh_reopens_each_persisted_login_and_reports_its_fresh_quota(self):
        for index, slot in enumerate(self.slots):
            after = deepcopy(self.replies[index])
            after["account/rateLimits/read"]["rateLimits"]["secondary"]["usedPercent"] = [7, 100][index]
            self.replies[index]["refreshWrites"] = {slot.name: after}
        result = self.probe("--refresh")
        self.assertEqual(result.returncode, 0, result.stderr)
        rows = json.loads(result.stdout)
        self.assertEqual([row["email"] for row in rows], EMAILS)
        for slot, row in zip(self.slots, rows):
            self.assertIs(row["refreshVerified"], True)
            calls = self.calls(slot)
            self.assertEqual([method for _, method, _ in calls].count("account/rateLimits/read"), 3)
            forced = [(i, pid) for i, (pid, _, refresh) in enumerate(calls) if refresh]
            self.assertEqual(len(forced), 1)
            i, pid = forced[0]
            self.assertIn([pid, "account/read", False], calls[:i])
            self.assertTrue(any(p != pid and method == "account/rateLimits/read"
                                for p, method, _ in calls[i + 1:]))
        self.assertEqual([row["windows"][0]["usedPercent"] for row in rows], [7, 100])
        self.assertIs(rows[1]["ordinaryUsageAllowed"], False)

    def test_refresh_stops_before_forcing_when_one_slot_has_the_wrong_identity(self):
        self.replies[1]["account/read"]["account"]["email"] = EMAILS[0]
        result = self.probe("--refresh")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [0, 0])

    def test_refresh_rechecks_provider_identity_in_the_issuing_process(self):
        self.replies[0]["reopenedProviderId"] = "private-provider-id-changed"
        result = self.probe("--refresh")
        self.assertNotEqual(result.returncode, 0)
        reader_counts = [len({pid for pid, method, _ in self.calls(slot) if method == "account/read"})
                         for slot in self.slots]
        self.assertEqual(reader_counts, [2, 1])
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [0, 0])

    def test_refresh_requires_atomic_publication_and_an_unchanged_sibling(self):
        cases = {
            "acknowledged without persistence": {"refreshWrites": {}},
            "provider refused": {"refreshError": "private-provider-id-in-error"},
            "original inode only touched": {"touchOnRefresh": True},
            "new inode with old modification time": {"keepRefreshTime": True},
            "both slots replaced": {"refreshWrites": {slot.name: None for slot in self.slots}},
        }
        first = self.replies[0]
        for description, action in cases.items():
            with self.subTest(action=description):
                self.replies[0] = dict(first, **action)
                result = self.probe("--refresh")
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual([self.forced_count(slot) for slot in self.slots], [1, 0])
                self.assertFalse(any(row.get("refreshVerified") for row in json.loads(result.stdout)))

    def test_refresh_refuses_a_new_provider_account_with_the_expected_email(self):
        first = self.slots[0]
        after = deepcopy(self.replies[0])
        after["account/read"]["workspaceRouting"]["chatgptAccountId"] = "private-provider-id-changed"
        after["account/rateLimits/read"]["accountId"] = "private-provider-id-changed"
        self.replies[0]["refreshWrites"] = {first.name: after}
        result = self.probe("--refresh")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [1, 0])
        self.assertFalse(any(row.get("refreshVerified") for row in json.loads(result.stdout)))

    def test_refresh_preserves_the_first_result_if_the_second_exchange_is_inconclusive(self):
        self.replies[1]["refreshWrites"] = {}
        result = self.probe("--refresh")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual([self.forced_count(slot) for slot in self.slots], [1, 1])
        first, second = json.loads(result.stdout)
        self.assertIs(first["refreshVerified"], True)
        self.assertTrue(second["error"])
        self.assertFalse(second.get("refreshVerified"))
