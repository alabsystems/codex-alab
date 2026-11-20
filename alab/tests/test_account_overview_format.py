# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check the human-readable subscription overview independently of native I/O."""

from datetime import datetime
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
from account_overview import format_overview


class AccountOverviewFormatTests(unittest.TestCase):
    def test_full_addresses_labels_and_permission_are_independent_of_usage_windows(self):
        rows = [
            {"email": "møon.2@example.test", "slotLabel": "backup",
             "loginVerified": True, "ordinaryUsageAllowed": True,
             "windows": [{"usedPercent": 100, "windowDurationMins": 300}]},
            {"email": "møon@example.test", "slotLabel": "main",
             "loginVerified": True, "ordinaryUsageAllowed": False,
             "windows": [{"usedPercent": 0, "windowDurationMins": 10080}]},
        ]
        self.assertEqual(format_overview(rows), """møon.2@example.test
  Request switch  /accounts use backup
  Sign-in         Verified
  Provider usage  Allowed
  5h              0% left · reset unavailable

møon@example.test
  Switch          main · unavailable (provider usage blocked)
  Sign-in         Verified
  Provider usage  Blocked
  Weekly          100% left · reset unavailable""")

    def test_reset_time_round_trips_through_the_local_offset(self):
        timestamp = 1795195398
        shown = format_overview([{"email": "moon@example.test", "loginVerified": True, "ordinaryUsageAllowed": None,
                                  "windows": [{"windowDurationMins": 90, "usedPercent": 12,
                                               "resetsAt": timestamp}]}])
        self.assertIn("Sign-in         Verified", shown)
        self.assertIn("Provider usage  Unknown", shown)
        self.assertIn("90 min          88% left · resets ", shown)
        local = datetime.fromisoformat(shown.split(" · resets ")[1])
        self.assertIsNotNone(local.tzinfo)
        self.assertEqual(local.timestamp(), timestamp)
        self.assertEqual(local.utcoffset(), datetime.fromtimestamp(timestamp).astimezone().utcoffset())

    def test_unknown_durations_and_invalid_numbers_do_not_acquire_named_allowances(self):
        for minutes, used, reset in [(None, True, 10**30), (300.0, 10.5, False), (0, -1, None),
                                     (True, 101, "tomorrow")]:
            with self.subTest(minutes=minutes, used=used, reset=reset):
                shown = format_overview([{"email": "moon@example.test", "loginVerified": True, "ordinaryUsageAllowed": 1,
                                          "windows": [{"windowDurationMins": minutes, "usedPercent": used,
                                                       "resetsAt": reset}]}])
                self.assertIn("Provider usage  Unknown", shown)
                self.assertIn("Usage window    allowance unavailable · reset unavailable", shown)
                self.assertNotIn("5h", shown)
                self.assertNotIn("Weekly", shown)
        empty = format_overview([{"email": "moon@example.test", "ordinaryUsageAllowed": True,
                                  "windows": [{"usedPercent": 0, "windowDurationMins": 10080}]}])
        self.assertIn("Slot label      unavailable", empty)
        self.assertIn("Sign-in         Unverified", empty)
        self.assertIn("Provider usage  Unknown", empty)
        self.assertIn("Usage windows   unavailable", empty)
        self.assertNotIn("Weekly", empty)

    def test_error_rows_escape_terminal_text_and_withhold_stale_permission(self):
        row = {"email": "møon\x1b[2J\n@example.test", "slotLabel": "main\u202eother",
               "loginVerified": True, "ordinaryUsageAllowed": True, "windows": [{"usedPercent": 0}],
               "error": "bad\rresult\u2068hidden\u2069"}
        shown = format_overview([row])
        self.assertEqual(shown.splitlines(), [
            r"møon\x1b[2J\n@example.test",
            r"  Slot label      main\u202eother",
            "  Sign-in         Unverified",
            "  Provider usage  Unknown",
            r"  Verification    bad\rresult\u2068hidden\u2069",
        ])

    def test_switch_requests_require_a_verified_identity_and_one_valid_label(self):
        for label in (None, "", "help", "a b", "-x", "a" * 65, "a\n/status", "MOON", 17):
            with self.subTest(label=label):
                shown = format_overview([{"email": "sample@example.test", "loginVerified": True, "slotLabel": label}])
                self.assertNotIn("/accounts use", shown)

    def test_signin_address_escapes_terminal_controls_and_requires_a_verified_row(self):
        row = {"email": "provider@example.test", "slotLabel": "main", "loginVerified": True,
               "loginEmail": "alias\x1b[2J\n\u202e@example.test"}
        self.assertIn(r"alias\x1b[2J\n\u202e@example.test", format_overview([row]))
        for change in ({"loginVerified": False}, {"slotLabel": None}, {"error": "identity check failed"}):
            self.assertNotIn("alias", format_overview([dict(row, **change)]))

    def test_registry_describes_retained_evidence_and_withholds_unverified_claims(self):
        association = {"machine": "example", "status": "associated",
                       "sources": ["machine-preference", "preference", "retained-observation"]}
        row = {"email": "sample@example.test", "loginVerified": True, "registryAssociation": association}
        self.assertIn("Associated (preferred, previously observed)", format_overview([row]))
        self.assertIn("Unverified (login unconfirmed)", format_overview([dict(row, loginVerified=False)]))
        for sources in (None, [], ["private-label"], [26]):
            record = dict(row, registryAssociation=dict(association, sources=sources))
            self.assertIn("Unverified", format_overview([record]))
            self.assertNotIn("private-label", format_overview([record]))
        for row in ({"loginVerified": False}, {"loginVerified": True, "error": "slot changed"}):
            shown = format_overview([dict(row, email="sample@example.test", slotLabel="main")])
            self.assertNotIn("/accounts use", shown)
        self.assertIn("/accounts use a._-2", format_overview([{"loginVerified": True, "slotLabel": "a._-2"}]))
