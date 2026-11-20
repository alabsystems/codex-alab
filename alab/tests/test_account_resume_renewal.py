# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the boundary between the source and destination terminal sessions."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock


SMOKE = Path(__file__).resolve().parents[1] / "smoke"
sys.path.insert(0, str(SMOKE))
import account_resume as resume
from ordinary_account_renewal import fingerprint


class AccountResumeRenewalTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)

    def prepare(self):
        gateway = resume.RenewalGateway({"events": [], "requests": []})
        issuer = "https://127.0.0.1"
        slots = {"source": resume.write_login(self.root, gateway, "source", issuer),
                 "destination": gateway.prepare_destination(self.root, issuer)}
        tokens = json.loads(slots["destination"].read_text())["tokens"]
        grant = {"grant_type": "refresh_token", "refresh_token": tokens["refresh_token"],
                 "client_id": tokens["oauth_provenance"]["client_id"], "refresh_reason": "proactive_expiry"}
        return gateway, slots, grant

    def test_destination_renewal_requires_separate_slots_before_opening_the_binary(self):
        result = subprocess.run(
            [sys.executable, "-B", str(SMOKE / "account_resume.py"), "--codex",
             str(self.root / "missing-codex"), "--output", str(self.root / "evidence"),
             "--renew-destination", "--surface", "id"], capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("--separate-auth-files", result.stderr.splitlines()[-1])
        self.assertFalse((self.root / "evidence").exists())

    def test_interrupted_exchange_body_still_counts_as_an_attempt(self):
        gateway, _, grant = self.prepare()
        gateway.publish_successor(0, "test-thread")
        handler_type = gateway.handler()
        handler = handler_type.__new__(handler_type)
        handler.path = "/oauth/token"
        handler.headers = {"Content-Length": "100"}
        handler.rfile = Mock()
        handler.rfile.read.side_effect = TimeoutError()
        handler.send_error = Mock()
        handler.do_POST()
        handler.send_error.assert_called_once_with(400)
        self.assertEqual(gateway.slot.exchanges, 0)
        with self.assertRaisesRegex(RuntimeError, "unexpected destination renewal attempt"):
            gateway.exchange(grant)

    def test_a_valid_destination_grant_during_the_source_turn_is_rejected(self):
        gateway, slots, grant = self.prepare()
        before = fingerprint(slots["destination"])
        with self.assertRaises(RuntimeError):
            gateway.exchange(grant)
        self.assertEqual(fingerprint(slots["destination"]), before)
        gateway.publish_successor(0, "test-thread")
        with self.assertRaises(RuntimeError):
            gateway.exchange(grant)

    def test_resumed_requests_distinguish_the_issued_bearer_from_the_expired_bearer(self):
        gateway, slots, grant = self.prepare()
        destination = json.loads(slots["destination"].read_text())
        expired = destination["tokens"]["access_token"]
        gateway.publish_successor(0, "test-thread")
        issued = gateway.exchange(grant)
        for bearer in (expired, issued["access_token"]):
            gateway.response({"input": []}, {"authorization": "Bearer " + bearer,
                                             "chatgpt-account-id": "workspace-destination"})
        self.assertEqual([request["login"] for request in gateway.requests], [None, "destination"])

    def test_atomic_replacement_requires_the_issued_tokens_and_intact_source(self):
        gateway, slots, grant = self.prepare()
        before = {name: fingerprint(path) for name, path in slots.items()}
        source, destination = slots["source"], slots["destination"]
        gateway.publish_successor(0, "test-thread")
        issued = gateway.exchange(grant)
        staged = self.root / "staged"
        renewed = self.root / "renewed"
        auth = json.loads(destination.read_text())
        auth["tokens"].update(issued)
        renewed.write_text(json.dumps(auth))
        staged.write_bytes(destination.read_bytes())
        staged.replace(destination)
        with self.assertRaisesRegex(RuntimeError, "saved credential"):
            gateway.check_renewal(slots, before)

        renewed.replace(destination)
        gateway.check_renewal(slots, before)
        staged.write_bytes(source.read_bytes())
        staged.replace(source)
        with self.assertRaisesRegex(RuntimeError, "source credential"):
            gateway.check_renewal(slots, before)


if __name__ == "__main__":
    unittest.main()
