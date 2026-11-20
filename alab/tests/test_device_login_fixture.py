# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0

import json
from pathlib import Path
import sys
import tempfile
import unittest
from urllib.parse import urlencode

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
from device_login_fixture import DeviceIssuer, Slot, device_login_issuer
from renewal_follow import access_token, fixture_tls


class LoopbackDeviceLogin(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.slots = [Slot(f"g{i}", f"account-{i}", f"user-{i}", f"{i}@example.invalid",
                           self.root / str(i) / "auth.json") for i in range(2)]

    def grant(self, issuer, index):
        device = json.loads(issuer.reply(index, "api/accounts/deviceauth/usercode",
                                         b'{"client_id":"fixture-client"}'))
        poll = {key: device[key] for key in ("device_auth_id", "user_code")}
        challenge = json.loads(issuer.reply(index, "api/accounts/deviceauth/token", json.dumps(poll).encode()))
        form = {"grant_type": "authorization_code", "code": challenge["authorization_code"],
                "code_verifier": challenge["code_verifier"], "client_id": "fixture-client"}
        return "Bearer " + json.loads(issuer.reply(index, "oauth/token", urlencode(form).encode()))["access_token"]

    def test_bearer_requires_the_exact_completed_exchange_for_its_slot(self):
        issuer = DeviceIssuer(self.slots)
        other_run = DeviceIssuer(self.slots)
        first, second = self.slots
        seeded = "Bearer " + access_token(first.generation, first.account, email=first.email, user=first.user)
        self.assertFalse(issuer.bearer_matches(first.generation, None))
        with self.assertRaisesRegex(ValueError, "unexpected device issuer request"):
            issuer.reply(0, "oauth/token", b"")
        old, new = self.grant(issuer, 0), self.grant(issuer, 1)
        self.assertTrue(issuer.bearer_matches(first.generation, old))
        self.assertTrue(issuer.bearer_matches(second.generation, new))
        for wrong in (new, seeded, self.grant(other_run, 0), None):
            self.assertFalse(issuer.bearer_matches(first.generation, wrong))
        with self.assertRaisesRegex(ValueError, "unexpected device issuer request"):
            self.grant(issuer, 0)

    def test_context_awaits_existing_ca_and_refuses_an_initialized_slot(self):
        ca = self.root / "fixture-ca.pem"
        issuer = device_login_issuer(ca, self.slots)
        self.assertFalse(ca.exists())
        self.assertFalse(issuer.bearer_matches("g0", "Bearer unissued"))
        fixture_tls(self.root)
        before = ca.read_bytes()
        slot = self.slots[0]
        slot.auth_path.parent.mkdir()
        slot.auth_path.write_bytes(b"existing slot")
        with issuer, self.assertRaisesRegex(ValueError, "already initialized"):
            issuer.login(Path("/unused"), self.root, slot, {})
        self.assertEqual(slot.auth_path.read_bytes(), b"existing slot")
        self.assertEqual(ca.read_bytes(), before)
        self.assertIsNone(issuer.port)

    def test_distinct_generations_cannot_alias_the_same_slot(self):
        one, two = self.slots
        alias = Slot(two.generation, two.account, two.user, two.email, one.auth_path)
        with self.assertRaisesRegex(ValueError, "distinct absolute"):
            DeviceIssuer([one, alias])


if __name__ == "__main__":
    unittest.main()
