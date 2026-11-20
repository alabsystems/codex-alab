# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import json
from email.message import Message
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import account_reconnect as probe


class AccountReconnectTests(unittest.TestCase):
    def test_previous_status_cannot_supply_fields_for_a_later_card(self):
        first = b"/statuscodex-alabv1Account:round.one@example.invalid(Pro)Session:thread"
        self.assertTrue(probe.current_status(first, probe.EMAILS["source"], "thread"))
        for latest in (b"/statuscodex-alabv1", b"/statuscodex-alabv1Session:thread",
                       b"/statuscodex-alabv1Account:round.one@example.invalid(Pro)",
                       b"/statuscodex-alabv1Account:round.two@example.invalid(Pro)Session:thread"):
            with self.subTest(latest=latest):
                self.assertFalse(probe.current_status(first + latest, probe.EMAILS["source"], "thread"))

    def test_return_requires_both_replies_and_only_one_request_after_shutdown(self):
        headers = Message()
        for key, value in (("Authorization", "Bearer A"), ("ChatGPT-Account-Id", "workspace-source"),
                           ("thread-id", "thread"), ("x-codex-turn-metadata", json.dumps({"turn_id": "third"}))):
            headers[key] = value
        messages = [{"role": role, "content": [{"type": kind, "text": text} for kind, text in parts]}
                    for role, parts in probe.expected_messages(2)]
        for change in ("none", "missing B reply", "extra request", "multiple credentials"):
            with self.subTest(change=change):
                gateway = probe.ReconnectGateway({"events": [], "requests": []})
                gateway.tokens["Bearer A"] = "source"
                current_headers = headers
                if change == "multiple credentials":
                    current_headers = Message()
                    for key, value in headers.items():
                        current_headers[key] = value
                    current_headers["AUTHORIZATION"] = "Bearer B"
                body = {"input": messages if change != "missing B reply" else messages[:3] + messages[4:]}
                gateway.response(body, current_headers)
                if change == "extra request":
                    gateway.confirmed_turn(2, "third")
                    gateway.response(body, current_headers)
                if change == "none":
                    self.assertEqual(gateway.confirmed_turn(2, "third")["thread"], "thread")
                else:
                    with self.assertRaises(RuntimeError):
                        gateway.confirmed_turn(2, "third")
