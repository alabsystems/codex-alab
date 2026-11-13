# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The rotation fixture names its accounts and proves earned reset before rotation."""
import base64
import json
from pathlib import Path
import sys
import tempfile
import time
import unittest
import urllib.error
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import provider_fixture
import renewal_follow as renewal

# The paths the client uses against a `chatgpt_base_url` without `/backend-api`.
USAGE = "/api/codex/usage"
CREDITS = "/api/codex/rate-limit-reset-credits"
RESPONSES = "/v1/responses"


def event(method, path, account, at, **extra):
    return {"method": method, "path": path, "account": account, "monotonicUs": at, **extra}


def wall(account, at):
    return event("POST", RESPONSES, account, at, kind="foreground", status=429)


class FixtureIdentityTests(unittest.TestCase):
    def test_each_login_reads_its_own_named_and_creditless_account(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            model = renewal.Model(root, weekly_quota=True, chain=True)
            # The provider refuses the source before its usage says spent.
            turn = {"input": [], "tools": [{"type": "function", "name": "update_plan"}]}
            self.assertEqual([model.reply(turn, "old")[0] for _ in range(2)], [200, 429])
            (root / "renewed").touch()
            server, worker = renewal.serve(model, secure=False)
            try:
                base = f"http://127.0.0.1:{server.server_port}"

                tokens = {}
                for generation, (_, account, user) in renewal.QUOTA_ACCOUNTS.items():
                    path = root / f"auth-{generation}" / "auth.json"
                    renewal.fake_auth(path, generation, "https://issuer.invalid", account, user=user)
                    tokens[generation] = json.loads(path.read_text())["tokens"]["access_token"]

                def get(path, generation):
                    request = urllib.request.Request(base + path,
                        headers={"authorization": "Bearer " + tokens[generation]})
                    with urllib.request.urlopen(request, timeout=10) as reply:
                        return json.loads(reply.read())

                for generation, (role, account, user) in renewal.QUOTA_ACCOUNTS.items():
                    with self.subTest(generation=generation):
                        usage = get(USAGE, generation)
                        self.assertEqual((usage["account_id"], usage["user_id"]), (account, user))
                        provider_fixture.check("usage", usage)
                        credits = get(CREDITS, generation)
                        self.assertEqual(credits, {"available_count": 0, "credits": []})
                        provider_fixture.check("credits", credits)
                        listed = model.http_events[-1]
                        self.assertEqual((listed["method"], listed["account"], listed["client"]),
                                         ("GET", role, "codex"))
                        self.assertIs(type(listed["monotonicUs"]), int)
                # The source stays spent; a renewed destination is usable.
                self.assertFalse(get(USAGE, "old")["rate_limit"]["allowed"])
                self.assertEqual(get(USAGE, "old")["rate_limit"]["secondary_window"]["used_percent"], 100)
                self.assertTrue(get(USAGE, "new")["rate_limit"]["allowed"])
                stamps = [entry["monotonicUs"] for entry in model.http_events]
                self.assertEqual(stamps, sorted(stamps))
                recorded = json.dumps(model.http_events) + (root / "http-events-live.json").read_text()
                for token in tokens.values():
                    self.assertNotIn(token, recorded)
                    self.assertNotIn(token.split(".")[1], recorded)
                self.assertNotIn("fixture_generation", recorded)
                # A bearer that is no fixture login's is refused, and so is none at all.
                for headers in ({}, {"authorization": "Bearer fixture-access-old"},
                                {"authorization": "Bearer " + renewal.access_token("stranger")},
                                {"authorization": "Bearer " + tokens["old"].rsplit(".", 2)[0] + ".e30.sig"},
                                {"authorization": "Basic " + tokens["old"]}):
                    with self.assertRaises(urllib.error.HTTPError) as refused:
                        urllib.request.urlopen(urllib.request.Request(base + USAGE, headers=headers), timeout=10)
                    with refused.exception:
                        self.assertEqual(refused.exception.code, 401)
            finally:
                server.shutdown()
                worker.join()
                server.server_close()

    def test_fixture_claims_and_usage_identity_agree(self):
        with tempfile.TemporaryDirectory() as directory:
            for generation, (_, account, user) in renewal.QUOTA_ACCOUNTS.items():
                path = Path(directory) / generation / "auth.json"
                renewal.fake_auth(path, generation, "https://issuer.invalid", account, user=user)
                tokens = json.loads(path.read_text())["tokens"]
                self.assertEqual(tokens["account_id"], account)
                header = {"authorization": "Bearer " + tokens["access_token"]}
                self.assertEqual(renewal.bearer_generation(header), generation)
                self.assertEqual(renewal.bearer_generation({"Authorization": header["authorization"]}), generation)

                def payload(token):
                    encoded = token.split(".")[1]
                    return json.loads(base64.urlsafe_b64decode(encoded + "=" * (-len(encoded) % 4)))

                self.assertEqual(len(tokens["access_token"].split(".")), 3)
                access, identity = payload(tokens["access_token"]), payload(tokens["id_token"])
                self.assertGreater(access["exp"], time.time() + 29 * 86400)
                self.assertEqual(access.pop("fixture_generation"), generation)
                del access["exp"]
                # The same sub, email and auth claims as the id token, which has no expiry.
                self.assertEqual(access, identity)
                self.assertEqual(identity["https://api.openai.com/auth"],
                    {"chatgpt_user_id": user, "chatgpt_account_id": account, "chatgpt_plan_type": "pro"})
                self.assertNotIn("chatgpt_account_user_id", json.dumps(access))
        for headers in ({}, {"authorization": "Bearer a.b"}, {"authorization": "Bearer a.!!.c"},
                        {"authorization": "Bearer a.e30.c"}, {"authorization": "Bearer a.WzFd.c"},
                        {"authorization": "Bearer a.eyJmaXh0dXJlX2dlbmVyYXRpb24iOjF9.c"}):
            self.assertIsNone(renewal.bearer_generation(headers))

    def test_a_fixture_login_names_no_login_owner(self):
        # A real sign-in writes tokens.login_owner_id. Giving each fixture login its own
        # changes what the client's same_owner() answers for the two logins of one account
        # and user that every same-account case presents, and those cases last passed
        # with none. One is added only with a run of `happy` before and after it.
        with tempfile.TemporaryDirectory() as directory:
            for generation in ("old", "new", "next"):
                path = Path(directory) / generation / "auth.json"
                renewal.fake_auth(path, generation, "https://issuer.invalid")
                tokens = json.loads(path.read_text())["tokens"]
                self.assertEqual(sorted(tokens), ["access_token", "account_id", "id_token",
                    "oauth_provenance", "refresh_token"])


class CreditlessYieldTests(unittest.TestCase):
    def good(self):
        return [event("GET", USAGE, "source", 10), event("GET", CREDITS, "source", 11),
            event("POST", RESPONSES, "source", 20, kind="foreground", status=200),
            wall("source", 30), event("GET", USAGE, "source", 31),
            event("GET", CREDITS, "source", 32), event("GET", USAGE, "target", 50),
            event("POST", RESPONSES, "target", 60, kind="foreground", status=200)]

    def test_a_wall_that_tried_its_earned_reset_before_rotation_passes(self):
        self.assertEqual(renewal.validate_creditless_yield(self.good(), [40, 45]),
            [{"account": "source", "wallUs": 30, "creditReadUs": 32, "firstPrepareUs": 40}])
        chain = self.good()[:-1] + [wall("target", 60), event("GET", CREDITS, "target", 62),
            event("POST", RESPONSES, "final", 90, kind="foreground", status=200)]
        self.assertEqual([proof["account"] for proof in
            renewal.validate_creditless_yield(chain, [40, 70])], ["source", "target"])

    def test_each_departure_from_the_recovery_order_is_refused(self):
        cases = {
            # The turn ended without asking for an earned reset: only the
            # pre-wall preflight read the credits.
            "no credit read": (self.good()[:5] + self.good()[6:], [40]),
            "rotation first": (self.good(), [31]),
            # The turn is still polling: nothing was ever prepared.
            "never prepared": (self.good(), []),
            "stale prepare only": (self.good(), [25]),
            "another account's read": (self.good()[:5] + [event("GET", CREDITS, "target", 32)]
                + self.good()[6:], [40]),
            "redeemed": (self.good() + [event("POST", CREDITS + "/consume", "source", 33,
                kind="non-responses")], [40]),
            "retried the request": (self.good() + [event("POST", RESPONSES, "source", 61,
                kind="foreground", status=429)], [40, 70]),
            "no wall": ([entry for entry in self.good() if entry.get("status") != 429], [40]),
            "unattributed wall": (self.good()[:3] + [wall(None, 30)] + self.good()[4:], [40]),
        }
        for name, (events, prepares) in cases.items():
            with self.subTest(name=name):
                with self.assertRaises(RuntimeError):
                    renewal.validate_creditless_yield(events, prepares)

    def test_only_completed_prepare_stamps_are_read_from_the_host_trace(self):
        with tempfile.TemporaryDirectory() as directory:
            trace = Path(directory) / "renewal-control-trace.jsonl"
            with self.assertRaises(RuntimeError):
                renewal.rotation_prepares(trace)
            rows = [{"kind": "rotation", "verb": "prepare", "monotonicUs": 40},
                {"kind": "rotation", "verb": "status", "monotonicUs": 41},
                {"kind": "renewal", "verb": "prepare", "monotonicUs": 42},
                {"kind": "rotation", "verb": "prepare", "monotonicUs": 45}]
            trace.write_text("".join(json.dumps(row) + "\n" for row in rows))
            self.assertEqual(renewal.rotation_prepares(trace), [40, 45])
            trace.write_text(json.dumps({"kind": "rotation", "verb": "prepare"}) + "\n")
            with self.assertRaises(RuntimeError):
                renewal.rotation_prepares(trace)


if __name__ == "__main__":
    unittest.main()
