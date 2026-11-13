# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The rotation smokes serve provider replies the real client decodes, and say so at once."""
import copy
import http.client
import json
from pathlib import Path
import socket
import ssl
import sys
import tempfile
import unittest
from unittest import mock
import urllib.error
import urllib.request

SMOKE = Path(__file__).resolve().parents[1] / "smoke"
sys.path.insert(0, str(SMOKE))
import provider_fixture as provider
import renewal_follow as renewal
import weekly_reset

USAGE = "/api/codex/usage"
CREDITS = "/api/codex/rate-limit-reset-credits"


def foreground():
    """A model request renewal_follow.request_kind takes for the turn's own."""
    return {"input": [], "tools": [{"type": "function", "name": "update_plan"}]}


def title():
    """The client's thread-title request, as renewal_follow.request_kind knows it."""
    return {"input": [{"role": "user", "content": [{"type": "input_text",
                "text": renewal.TITLE_INSTRUCTIONS + " for this conversation."}]}],
            "tools": [], "text": {"format": renewal.TITLE_FORMAT}}


def refuse_source(case, model):
    """Have the provider serve the source login its first turn and then its wall."""
    marker = model.root / "renewed"
    renewed = marker.exists()
    marker.unlink(missing_ok=True)
    replies = [model.reply(foreground(), "old") for _ in range(2)]
    case.assertEqual([reply[0] for reply in replies], [200, 429])
    if renewed:
        marker.touch()
    return [reply[4] for reply in replies]


def served(case, model):
    """The fixture server over plain loopback HTTP, stopped when the test ends."""
    server, worker = renewal.serve(model, secure=False)

    def stop():
        server.shutdown()
        worker.join()
        server.server_close()
    case.addCleanup(stop)
    return f"http://127.0.0.1:{server.server_port}"


def bearer(generation):
    """The Authorization value of that fixture login (any other word: no login's)."""
    role, account, user = renewal.QUOTA_ACCOUNTS.get(generation, (None, "stranger-account", "stranger"))
    return "Bearer " + renewal.access_token(generation, account, user=user)


def get(base, path, generation, agent=None):
    headers = {"authorization": bearer(generation)}
    if agent is not None:
        headers["user-agent"] = agent
    with urllib.request.urlopen(urllib.request.Request(base + path, headers=headers), timeout=10) as reply:
        return json.loads(reply.read())


def status(request):
    """The status of a request, with a refusal's reply closed."""
    try:
        with urllib.request.urlopen(request, timeout=10):
            return 200
    except urllib.error.HTTPError as error:
        with error:
            return error.code


def get_status(base, path, generation):
    return status(urllib.request.Request(base + path, headers={"authorization": bearer(generation)}))


class ReplyShapeTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))

    def test_loopback_provider_serves_without_hostname_resolution(self):
        model = renewal.Model(self.root, weekly_quota=True)
        with mock.patch("socket.getfqdn", side_effect=AssertionError("unexpected fixture DNS lookup")):
            base = served(self, model)
            reply = get(base, USAGE, "old")
        provider.check("usage", reply)
        self.assertEqual(reply["account_id"], "source-account")

    def test_tls_fixture_requires_its_own_ca_and_correct_hostname_under_strict_verification(self):
        model = renewal.Model(self.root, weekly_quota=True)
        server, worker = renewal.serve(model)

        def stop():
            server.shutdown()
            server.server_close()
            worker.join(timeout=5)

        self.addCleanup(stop)

        def context(cafile=None):
            result = ssl.create_default_context(cafile=cafile)
            result.verify_flags |= ssl.VERIFY_X509_STRICT
            result.verify_flags |= getattr(ssl, "VERIFY_X509_PARTIAL_CHAIN", 0)
            self.assertEqual(result.verify_mode, ssl.CERT_REQUIRED)
            self.assertTrue(result.check_hostname)
            return result

        trusted = context(str(self.root / "fixture-ca.pem"))
        connection = http.client.HTTPSConnection("127.0.0.1", server.server_port,
                                                 context=trusted, timeout=5)
        try:
            connection.request("GET", USAGE, headers={"authorization": bearer("old")})
            response = connection.getresponse()
            self.assertEqual(response.status, 200)
            reply = json.loads(response.read())
            provider.check("usage", reply)
            self.assertEqual(reply["account_id"], "source-account")
        finally:
            connection.close()

        other = self.root / "other-ca"
        other.mkdir()
        renewal.fixture_tls(other)
        for label, policy, hostname in (
                ("untrusted", context(), "127.0.0.1"),
                ("wrong CA", context(str(other / "fixture-ca.pem")), "127.0.0.1"),
                ("wrong hostname", trusted, "localhost")):
            with self.subTest(policy=label):
                with socket.create_connection(("127.0.0.1", server.server_port), timeout=5) as raw:
                    with self.assertRaises(ssl.SSLCertVerificationError):
                        policy.wrap_socket(raw, server_hostname=hostname)

    def test_fixture_server_labels_its_numeric_bind_and_refuses_other_addresses(self):
        with mock.patch("socket.getfqdn", side_effect=AssertionError("unexpected fixture DNS lookup")):
            with provider.LoopbackHTTPServer(("127.0.0.1", 0), renewal.http.server.BaseHTTPRequestHandler) as server:
                self.assertEqual(server.server_name, "127.0.0.1")
                self.assertEqual(server.server_port, server.server_address[1])
                self.assertGreater(server.server_port, 0)
            for host in ("0.0.0.0", "192.0.2.1", "localhost"):
                with self.subTest(host=host), self.assertRaises(ValueError):
                    provider.LoopbackHTTPServer((host, 0), renewal.http.server.BaseHTTPRequestHandler)

    def test_every_window_carries_the_four_integers_the_decoder_requires(self):
        self.assertEqual(provider.window(100, 604800, 86400, 1000), {"used_percent": 100,
            "limit_window_seconds": 604800, "reset_after_seconds": 86400, "reset_at": 87400})
        body = provider.usage("account", "user", week_used=100, allowed=False, now=1000)
        self.assertEqual(body, {"account_id": "account", "user_id": "user", "plan_type": "pro",
            "rate_limit": {"allowed": False, "limit_reached": True,
                "primary_window": {"used_percent": 0, "limit_window_seconds": 18000,
                    "reset_after_seconds": 3600, "reset_at": 4600},
                "secondary_window": {"used_percent": 100, "limit_window_seconds": 604800,
                    "reset_after_seconds": 86400, "reset_at": 87400}},
            "rate_limit_reset_credits": {"available_count": 0}})
        self.assertIs(provider.check("usage", body), body)
        five_hour = provider.usage("account", "user", week_used=10, session_used=100,
            allowed=False, session_resets_in=600, now=1000)["rate_limit"]
        self.assertEqual(five_hour["primary_window"], {"used_percent": 100,
            "limit_window_seconds": 18000, "reset_after_seconds": 600, "reset_at": 1600})
        self.assertEqual(five_hour["secondary_window"]["used_percent"], 10)

    def test_the_rotation_fixture_serves_a_decodable_reply_to_each_login(self):
        (self.root / "renewed").touch()
        model = renewal.Model(self.root, weekly_quota=True, chain=True)
        for generation, (_, account, user) in renewal.QUOTA_ACCOUNTS.items():
            with self.subTest(generation=generation):
                body = model.usage(generation == "old", generation == "new", generation == "next")
                provider.check("usage", body)
                self.assertEqual((body["account_id"], body["user_id"]), (account, user))
                for name in ("primary_window", "secondary_window"):
                    self.assertGreater(body["rate_limit"][name]["reset_after_seconds"], 0)

    def test_the_earned_reset_smokes_reply_is_decodable_too(self):
        for name in ("no-credit", "stale-reset-credit", "preflight-five-hour"):
            with self.subTest(name=name):
                provider.check("usage", weekly_reset.Mock(name).usage())

    def test_a_reply_the_client_would_refuse_is_refused_here(self):
        good = provider.usage("account", "user", week_used=100, allowed=False, now=1000)

        def changed(edit):
            body = copy.deepcopy(good)
            edit(body)
            return body

        def window(name):
            return lambda body: body["rate_limit"][name]
        # The reply the fixture served before: windows without reset_after_seconds.
        before = changed(lambda body: [body["rate_limit"][name].pop("reset_after_seconds")
            for name in ("primary_window", "secondary_window")])
        refused = {
            "the reply served before this fixture": before,
            "one window without reset_after_seconds": changed(
                lambda body: window("secondary_window")(body).pop("reset_after_seconds")),
            "no plan_type": changed(lambda body: body.pop("plan_type")),
            "a plan no account has": changed(lambda body: body.update(plan_type="platinum")),
            "a fractional used_percent": changed(
                lambda body: window("secondary_window")(body).update(used_percent=100.0)),
            "a textual used_percent": changed(
                lambda body: window("primary_window")(body).update(used_percent="0")),
            "a boolean used_percent": changed(
                lambda body: window("primary_window")(body).update(used_percent=True)),
            "a reset beyond i32": changed(
                lambda body: window("secondary_window")(body).update(reset_at=2 ** 31)),
            "no account": changed(lambda body: body.pop("account_id")),
            "no user": changed(lambda body: body.update(user_id="")),
            "no verdict": changed(lambda body: body["rate_limit"].pop("allowed")),
            "a numeric verdict": changed(lambda body: body["rate_limit"].update(limit_reached=1)),
            "no rate_limit": changed(lambda body: body.pop("rate_limit")),
            "no window at all": changed(lambda body: body["rate_limit"].update(
                primary_window=None, secondary_window=None)),
            "a textual credit count": changed(
                lambda body: body["rate_limit_reset_credits"].update(available_count="0")),
            "a list": [good],
        }
        for name, body in refused.items():
            with self.subTest(name=name):
                with self.assertRaises(provider.FixtureFault):
                    provider.check("usage", body)
        # What the decoder leaves optional stays optional.
        provider.check("usage", changed(lambda body: body.pop("rate_limit_reset_credits")))
        provider.check("usage", changed(lambda body: body["rate_limit"].pop("primary_window")))
        with self.assertRaises(provider.FixtureFault):
            provider.check("models", good)

    def test_the_credit_list_and_redemption_replies_are_decodable(self):
        self.assertEqual(provider.check("credits", provider.credit_list()),
                         {"available_count": 0, "credits": []})
        self.assertEqual(provider.check("consume", provider.consume_reply()),
                         {"code": "no_credit", "windows_reset": 0})
        credit = {"id": "A", "reset_type": "codex_rate_limits", "status": "available",
                  "granted_at": "2026-01-01T00:00:00Z", "expires_at": None}
        provider.check("credits", {"available_count": 1, "credits": [credit]})
        provider.check("consume", {"code": "reset"})
        for name, kind, body in (
                ("no count", "credits", {"credits": []}),
                ("no list", "credits", {"available_count": 0}),
                ("a credit without an id", "credits", {"available_count": 1,
                    "credits": [{key: value for key, value in credit.items() if key != "id"}]}),
                ("a numeric expiry", "credits", {"available_count": 1,
                    "credits": [dict(credit, expires_at=1)]}),
                ("an unknown code", "consume", {"code": "declined", "windows_reset": 0}),
                ("no code", "consume", {"windows_reset": 0}),
                ("a fractional count", "consume", {"code": "reset", "windows_reset": 2.0})):
            with self.subTest(name=name):
                with self.assertRaises(provider.FixtureFault):
                    provider.check(kind, body)


class GoldenReplyTests(unittest.TestCase):
    """The files codex-rs/alab-quota-recovery's `fixture_tests` decodes with the real client."""

    def test_the_committed_files_are_exactly_what_the_fixture_generates(self):
        expected = provider.golden()
        self.assertEqual(sorted(expected), ["consume-no-credit.json", "credits-empty.json",
            "refusal-five-hour-wall.json", "refusal-weekly-wall.json",
            "usage-five-hour-wall.json", "usage-healthy.json", "usage-weekly-wall.json"])
        self.assertEqual(sorted(path.name for path in provider.GOLDEN_DIR.iterdir()), sorted(expected))
        for name, body in expected.items():
            with self.subTest(name=name):
                committed = (provider.GOLDEN_DIR / name).read_text()
                self.assertEqual(committed, provider.render(body))
                self.assertEqual(json.loads(committed), body)

    def test_the_writer_reproduces_them_with_its_fixed_clock(self):
        directory = Path(self.enterContext(tempfile.TemporaryDirectory())) / "golden"
        with mock.patch.object(sys, "argv", ["provider_fixture.py", "--write-golden", str(directory)]), \
                mock.patch("builtins.print"):
            self.assertEqual(provider.main(), 0)
        for name in provider.golden():
            self.assertEqual((directory / name).read_bytes(), (provider.GOLDEN_DIR / name).read_bytes())

    def test_they_are_the_replies_the_rotation_fixture_serves(self):
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        golden = provider.golden()
        self.assertEqual(provider.GOLDEN_SOURCE, renewal.QUOTA_ACCOUNTS["old"][1:])
        self.assertEqual(provider.GOLDEN_TARGET, renewal.QUOTA_ACCOUNTS["new"][1:])
        model = renewal.Model(root, weekly_quota=True)
        refuse_source(self, model)
        (root / "renewed").touch()
        with mock.patch.object(provider.time, "time", return_value=provider.GOLDEN_NOW):
            self.assertEqual(model.usage(True, False), golden["usage-weekly-wall.json"])
            self.assertEqual(model.usage(False, True), golden["usage-healthy.json"])
        self.assertEqual(provider.credit_list(), golden["credits-empty.json"])
        self.assertEqual(provider.consume_reply(), golden["consume-no-credit.json"])
        wall = golden["usage-five-hour-wall.json"]["rate_limit"]
        self.assertFalse(wall["allowed"])
        self.assertEqual((wall["primary_window"]["used_percent"],
            wall["primary_window"]["limit_window_seconds"],
            wall["primary_window"]["reset_after_seconds"]), (100, 18000, 600))
        self.assertLess(wall["secondary_window"]["used_percent"], 100)

    def test_the_golden_refusals_are_the_429s_the_rotation_fixture_serves(self):
        golden = provider.golden()
        clock = mock.patch.object(provider.time, "time", return_value=provider.GOLDEN_NOW)

        def refused(model):
            """The second model request of the source login, as (status, headers, body)."""
            model.reply(foreground(), "old")
            status, data, _, _, record = model.reply(foreground(), "old")
            return {"status": status, "headers": record["responseHeaders"], "body": json.loads(data)}
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        with clock:
            weekly = refused(renewal.Model(root / "weekly", weekly_quota=True))
        self.assertEqual(weekly, golden["refusal-weekly-wall.json"])
        # The five-hour wall is served WALL_RESET_SECONDS before its reset: the golden's ten minutes.
        self.assertEqual(renewal.WALL_RESET_SECONDS, 600)
        with clock, mock.patch.object(renewal.time, "time", return_value=provider.GOLDEN_NOW):
            soon = refused(renewal.Model(root / "soon", weekly_quota=True, wall="five-hour-soon"))
        self.assertEqual(soon, golden["refusal-five-hour-wall.json"])
        # Each golden refusal is the refusal of the golden usage reading of the same wall.
        for wall in ("weekly-wall", "five-hour-wall"):
            with self.subTest(wall=wall):
                reply = golden[f"refusal-{wall}.json"]
                usage = golden[f"usage-{wall}.json"]
                self.assertEqual(reply["headers"], provider.limit_headers(usage, wall=True))
                self.assertEqual(reply, provider.refusal(usage, reply["body"]["error"]["message"]))
                spent = [window for window in (usage["rate_limit"]["primary_window"],
                         usage["rate_limit"]["secondary_window"]) if window["used_percent"] >= 100]
                self.assertEqual([window["reset_at"] for window in spent],
                                 [reply["body"]["error"]["resets_at"]])


class RefusalTests(unittest.TestCase):
    """The 429 of a model request at a wall, held to what codex-api's error mapping reads."""

    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.usage = provider.usage("account", "user", week_used=100, allowed=False, now=1000)

    def test_a_refusal_states_one_instant_in_its_body_and_its_headers(self):
        reply = provider.check("refusal", provider.refusal(self.usage, "spent"))
        self.assertEqual(reply["status"], 429)
        self.assertEqual(reply["body"], {"error": {"type": "usage_limit_reached", "plan_type": "pro",
            "resets_at": 1000 + 86400, "message": "spent"}})
        self.assertEqual(reply["headers"], provider.limit_headers(self.usage, wall=True))
        self.assertEqual(reply["headers"]["x-codex-secondary-reset-at"], str(reply["body"]["error"]["resets_at"]))
        # With both windows spent the later reset is the one a request must wait for.
        both = provider.usage("account", "user", week_used=100, session_used=100, allowed=False, now=1000)
        self.assertEqual(provider.check("refusal", provider.refusal(both, "spent"))
                         ["body"]["error"]["resets_at"], 1000 + 86400)
        # The rotation model reads its usage once for a refusal, however the clock moves.
        model = renewal.Model(self.root, weekly_quota=True)
        model.reply(foreground(), "old")
        ticks = iter(range(7000, 7100))
        with mock.patch.object(provider.time, "time", side_effect=lambda: float(next(ticks))):
            status, data, _, _, record = model.reply(foreground(), "old")
        self.assertEqual(status, 429)
        self.assertEqual(str(json.loads(data)["error"]["resets_at"]),
                         record["responseHeaders"]["x-codex-secondary-reset-at"])

    def test_a_refusal_the_client_would_not_read_as_a_usage_limit_is_refused_here(self):
        good = provider.refusal(self.usage, "spent")

        def changed(edit):
            reply = copy.deepcopy(good)
            edit(reply)
            return reply
        refused = {
            "another status": changed(lambda reply: reply.update(status=503)),
            "no error object": changed(lambda reply: reply.update(body={"detail": "spent"})),
            "another error type": changed(lambda reply: reply["body"]["error"].update(type="rate_limit_exceeded")),
            "a plan no account has": changed(lambda reply: reply["body"]["error"].update(plan_type="platinum")),
            "a textual reset": changed(lambda reply: reply["body"]["error"].update(resets_at="87400")),
            "a fractional reset": changed(lambda reply: reply["body"]["error"].update(resets_at=87400.5)),
            "a reset that is not the spent window's": changed(
                lambda reply: reply["body"]["error"].update(resets_at=87401)),
            "no reset beside spent windows": changed(lambda reply: reply["body"]["error"].pop("resets_at")),
            "an unknown reached type": changed(
                lambda reply: reply["headers"].update({"x-codex-rate-limit-reached-type": "usage_limit_reached"})),
            "a limit whose windows are not sent": changed(
                lambda reply: reply["headers"].update({"x-codex-active-limit": "codex_other"})),
            "a percent that is no number": changed(
                lambda reply: reply["headers"].update({"x-codex-secondary-used-percent": "full"})),
            "a percent that is not finite": changed(
                lambda reply: reply["headers"].update({"x-codex-secondary-used-percent": "nan"})),
            "a window without its length": changed(
                lambda reply: reply["headers"].pop("x-codex-secondary-window-minutes")),
            "a reset that is no integer": changed(
                lambda reply: reply["headers"].update({"x-codex-primary-reset-at": "soon"})),
            "windows that are not spent": changed(
                lambda reply: reply["headers"].update({"x-codex-secondary-used-percent": "99"})),
            "a header with a number for a value": changed(
                lambda reply: reply["headers"].update({"x-codex-primary-used-percent": 0})),
            "a header name in capitals": changed(
                lambda reply: reply["headers"].update({"X-Codex-Active-Limit": "codex"})),
            "no headers object": changed(lambda reply: reply.pop("headers")),
            "a list": [good],
        }
        for name, reply in refused.items():
            with self.subTest(name=name):
                with self.assertRaises(provider.FixtureFault):
                    provider.check("refusal", reply)
        # What the decoder leaves optional stays optional: the exec fixtures send no
        # windows and name no plan, and a refusal may leave its reset unknown.
        bare = {"status": 429, "headers": {}, "body": {"error": {"type": "usage_limit_reached",
                "resets_at": 87400, "message": "Idle-child quota fixture"}}}
        provider.check("refusal", bare)
        del bare["body"]["error"]["resets_at"]
        provider.check("refusal", bare)
        provider.check("refusal", provider.refusal(self.usage, "spent", headers={}))

    def test_an_account_with_no_spent_window_has_no_wall_to_refuse_at(self):
        healthy = provider.usage("account", "user", week_used=99, session_used=99, allowed=True, now=1000)
        with self.assertRaisesRegex(provider.FixtureFault, "states no spent window"):
            provider.refusal(healthy, "spent")

        class Unwalled(renewal.Model):
            def week_spent(self, old_token, new_token, next_token=False):
                return False
        model = Unwalled(self.root, weekly_quota=True)
        base = served(self, model)

        def post():
            return status(urllib.request.Request(base + "/v1/responses", method="POST",
                data=json.dumps(foreground()).encode(),
                headers={"authorization": bearer("old"), "content-type": "application/json"}))
        self.assertEqual([post(), post()], [200, 500])
        self.assertEqual(model.fixture_faults, [{"path": "/v1/responses", "error":
            "FixtureFault: refusal: the account's usage states no spent window"}])
        with self.assertRaisesRegex(RuntimeError, "cannot decode on /v1/responses: FixtureFault: refusal"):
            renewal.require_no_fixture_fault(model)

    def test_the_exec_fixture_refuses_with_a_decodable_body_and_no_windows(self):
        import exec_accounts
        model = exec_accounts.Model(self.root)
        model.reply(foreground(), "old")
        code, data, _, _, record = model.reply(foreground(), "old")
        self.assertEqual((code, record["responseHeaders"]), (429, {}))
        body = json.loads(data)
        self.assertEqual((body["error"]["type"], body["error"]["plan_type"]), ("usage_limit_reached", "pro"))
        provider.check("refusal", {"status": code, "headers": record["responseHeaders"], "body": body})


class ServedReplyTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        (self.root / "renewed").touch()

    def test_a_reply_the_fixture_cannot_build_is_a_recorded_fault_not_a_silent_hang(self):
        model = renewal.Model(self.root, weekly_quota=True)
        refuse_source(self, model)
        base = served(self, model)
        renewal.require_no_fixture_fault(model)
        good = model.usage(True, False)
        bad = copy.deepcopy(good)
        del bad["rate_limit"]["secondary_window"]["reset_after_seconds"]
        with mock.patch.object(model, "usage", return_value=bad):
            self.assertEqual(get_status(base, USAGE, "old"), 500)
        self.assertEqual(model.fixture_faults, [{"path": USAGE, "error":
            "FixtureFault: usage: secondary_window.reset_after_seconds is absent or not an i32"}])
        with self.assertRaisesRegex(RuntimeError, "cannot decode on /api/codex/usage: FixtureFault: "
                                    "usage: secondary_window.reset_after_seconds"):
            renewal.require_no_fixture_fault(model)
        # A fault in the model's own bookkeeping is recorded the same way.
        with mock.patch.object(model, "usage", side_effect=RuntimeError("unexpected usage identity")):
            self.assertEqual(get_status(base, USAGE + "?probe=1", "old"), 500)
        self.assertEqual(model.fixture_faults[1], {"path": USAGE,
            "error": "RuntimeError: unexpected usage identity"})
        # A good reply still serves, and an unknown login is refused without a fault.
        self.assertEqual(get(base, USAGE, "old")["rate_limit"]["secondary_window"]["used_percent"], 100)
        self.assertEqual(get_status(base, USAGE, "stranger"), 401)
        self.assertEqual(len(model.fixture_faults), 2)

    def test_no_model_can_opt_out_of_the_reply_check(self):
        bad = provider.usage("source-account", "fixture-user", week_used=100, allowed=False)
        del bad["rate_limit"]["primary_window"]["reset_after_seconds"]

        class OwnReply(renewal.Model):
            # The switch the exec fixture once used to serve its own, undecodable reply.
            checked_replies = False

            def usage(self, old_token, new_token, next_token=False):
                return bad

        model = OwnReply(self.root, weekly_quota=True)
        base = served(self, model)
        self.assertEqual(get_status(base, USAGE, "old"), 500)
        self.assertEqual(model.fixture_faults, [{"path": USAGE, "error":
            "FixtureFault: usage: primary_window.reset_after_seconds is absent or not an i32"}])
        self.assertFalse(hasattr(renewal.Model, "checked_replies"))

    def test_the_exec_fixture_serves_the_same_decodable_named_reply(self):
        import exec_accounts
        model = exec_accounts.Model(self.root, chain=True)
        base = served(self, model)
        for generation, (role, account, user) in renewal.QUOTA_ACCOUNTS.items():
            with self.subTest(generation=generation):
                direct = model.usage(generation == "old", generation == "new", generation == "next")
                provider.check("usage", direct)
                self.assertEqual((direct["account_id"], direct["user_id"]), (account, user))
                body = get(base, USAGE, generation)
                self.assertEqual((body["account_id"], body["user_id"]), (account, user))
                self.assertEqual(body["rate_limit"], direct["rate_limit"])
        # The source is spent, the first target usable, the final one held back until
        # the middle account is exhausted: the rule the exec smokes already relied on.
        spent = [model.usage(True, False), model.usage(False, False, True)]
        self.assertEqual([body["rate_limit"]["secondary_window"]["used_percent"] for body in spent], [100, 100])
        self.assertEqual([body["rate_limit"]["allowed"] for body in spent], [False, False])
        self.assertTrue(model.usage(False, True)["rate_limit"]["allowed"])
        model.middle_exhausted = True
        self.assertFalse(model.usage(False, True)["rate_limit"]["allowed"])
        self.assertTrue(model.usage(False, False, True)["rate_limit"]["allowed"])
        self.assertEqual(model.fixture_faults, [])

    def test_each_request_names_its_program_and_never_its_bearer(self):
        model = renewal.Model(self.root, weekly_quota=True)
        base = served(self, model)
        get(base, USAGE, "old", agent="codex-cli")
        get(base, USAGE, "old", agent="codex_cli_rs/0.5.0 (Mac OS 27.2.0; arm64) aterm")
        get(base, CREDITS, "new", agent="codex-cli/1.0")
        get(base, CREDITS, "new")
        request = urllib.request.Request(base + "/oauth/token", data=b"{}", method="POST",
            headers={"authorization": bearer("old"), "user-agent": "codex-cli"})
        self.assertEqual(status(request), 400)
        self.assertEqual([(event["method"], event["account"], event["client"])
            for event in model.http_events],
            [("GET", "source", "host"), ("GET", "source", "codex"), ("GET", "target", "codex"),
             ("GET", "target", "codex"), ("POST", "source", "host")])
        recorded = json.dumps(model.http_events) + (self.root / "http-events-live.json").read_text()
        for secret in ("Bearer", "codex_cli_rs", "fixture_generation",
                       *bearer("old").removeprefix("Bearer ").split(".")[:2]):
            self.assertNotIn(secret, recorded)
        self.assertEqual(model.fixture_faults, [])


CODEX_RS = SMOKE.parents[1] / "codex-rs"


class LimitHeaderTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))

    def test_every_header_is_one_the_client_reads(self):
        windows = (CODEX_RS / "codex-api/src/rate_limits.rs").read_text()
        bridge = (CODEX_RS / "codex-api/src/api_bridge.rs").read_text()
        protocol = (CODEX_RS / "protocol/src/protocol.rs").read_text()
        body = provider.usage("a", "u", week_used=100, allowed=False, now=provider.GOLDEN_NOW)
        headers = provider.limit_headers(body, wall=True)
        self.assertEqual(sorted(headers), [
            "x-codex-active-limit", "x-codex-primary-reset-at", "x-codex-primary-used-percent",
            "x-codex-primary-window-minutes", "x-codex-rate-limit-reached-type",
            "x-codex-secondary-reset-at", "x-codex-secondary-used-percent",
            "x-codex-secondary-window-minutes"])
        # The window names are built there as "{prefix}-…" for the prefix "x-codex".
        self.assertIn('format!("x-{normalized_limit}")', windows)
        self.assertIn('.unwrap_or("codex")', windows)
        for name in headers:
            with self.subTest(name=name):
                self.assertTrue(f'"{name}"' in windows + bridge
                    or 'format!("{prefix}' + name.removeprefix("x-codex") + '")' in windows)
        self.assertIn(f'"{headers["x-codex-rate-limit-reached-type"]}" => Ok(Self::RateLimitReached)',
                      protocol)
        # The limit named as reached is the family those window headers belong to.
        self.assertEqual(headers["x-codex-active-limit"], "codex")
        self.assertEqual({key: value for key, value in headers.items() if "secondary" in key}, {
            "x-codex-secondary-used-percent": "100", "x-codex-secondary-window-minutes": "10080",
            "x-codex-secondary-reset-at": str(provider.GOLDEN_NOW + 86400)})
        self.assertEqual({key: value for key, value in headers.items() if "primary" in key}, {
            "x-codex-primary-used-percent": "0", "x-codex-primary-window-minutes": "300",
            "x-codex-primary-reset-at": str(provider.GOLDEN_NOW + 3600)})
        ordinary = provider.limit_headers(body)
        self.assertEqual(sorted(set(headers) - set(ordinary)),
                         ["x-codex-active-limit", "x-codex-rate-limit-reached-type"])

    def test_the_source_is_healthy_until_the_provider_has_refused_it(self):
        for options in ({}, {"all_exhausted": True}, {"chain": True}):
            with self.subTest(options=options):
                root = self.root / "-".join(options or {"plain": 1})
                root.mkdir()
                model = renewal.Model(root, weekly_quota=True, **options)
                before = model.usage(True, False)
                provider.check("usage", before)
                self.assertTrue(before["rate_limit"]["allowed"])
                self.assertEqual(before["rate_limit"]["secondary_window"]["used_percent"], 0)
                first, refused = refuse_source(self, model)
                after = model.usage(True, False)
                provider.check("usage", after)
                self.assertFalse(after["rate_limit"]["allowed"])
                self.assertEqual(after["rate_limit"]["secondary_window"]["used_percent"], 100)
                # Each model response states the windows its account's usage read states.
                self.assertEqual(first["responseHeaders"], provider.limit_headers(before))
                self.assertNotIn("100", [value for key, value in first["responseHeaders"].items()
                                         if key.endswith("used-percent")])
                self.assertEqual(refused["responseHeaders"], provider.limit_headers(after, wall=True))
                self.assertEqual((refused["responseHeaders"]["x-codex-secondary-used-percent"],
                                  refused["responseHeaders"]["x-codex-secondary-window-minutes"]),
                                 ("100", "10080"))
                # A destination is unreadable as usable until the host has opened it.
                self.assertEqual(model.usage_body("new")["rate_limit"]["allowed"], False)
                (root / "renewed").touch()
                self.assertEqual(model.usage_body("new")["rate_limit"]["allowed"],
                                 not options.get("all_exhausted", False))
                self.assertFalse(model.usage(True, False)["rate_limit"]["allowed"])

    def test_no_answered_response_states_a_spent_window(self):
        self.assertEqual(renewal.request_kind(title()), "thread-title")

        def model(name, **options):
            root = self.root / name
            root.mkdir()
            built = renewal.Model(root, weekly_quota=True, **options)
            refuse_source(self, built)
            return built

        # Each 200 this fixture gives a login whose usage reads as not allowed.
        weekly = model("weekly")
        late_title = weekly.reply(title(), "old")
        (weekly.root / "renewed").touch()
        exhausted = model("exhausted", all_exhausted=True)
        (exhausted.root / "renewed").touch()
        walled = model("five-hour", wall="five-hour-soon")
        answered = {
            "a title request after the weekly wall": (weekly, "old", late_title),
            "the walled source, sent again once the destination is open":
                (weekly, "old", weekly.reply(foreground(), "old")),
            "an exhausted destination": (exhausted, "new", exhausted.reply(foreground(), "new")),
            "a title request during the five-hour wall": (walled, "old", walled.reply(title(), "old")),
        }
        for name, (built, generation, reply) in answered.items():
            with self.subTest(name=name):
                status, record = reply[0], reply[4]
                self.assertEqual(status, 200)
                self.assertFalse(built.usage_body(generation)["rate_limit"]["allowed"])
                self.assertEqual(record["responseHeaders"], {})
                self.assertIs(record["limitHeadersWithheld"], True)
        # A healthy login's 200 still states its windows, and says nothing was withheld.
        (walled.root / "elsewhere").mkdir()
        healthy = renewal.Model(walled.root / "elsewhere", weekly_quota=True)
        record = healthy.reply(foreground(), "old")[4]
        self.assertEqual(record["responseHeaders"], provider.limit_headers(healthy.usage_body("old")))
        self.assertNotIn("limitHeadersWithheld", record)
        # Across every model request these fixtures recorded, no 200 states a spent window
        # and every refusal states one.
        for built in (weekly, exhausted, walled, healthy):
            for record in built.requests + built.auxiliary_requests:
                percents = [value for key, value in record["responseHeaders"].items()
                            if key.endswith("-used-percent")]
                self.assertEqual("100" in percents, "x-codex-active-limit" in record["responseHeaders"])

    def test_a_200_that_states_a_spent_window_is_a_recorded_fault(self):
        spent = provider.limit_headers(provider.usage("a", "u", week_used=100, allowed=False))
        self.assertEqual(renewal.spent_window_on_success(200, spent), "x-codex-secondary-used-percent")
        self.assertIsNone(renewal.spent_window_on_success(429, spent))
        self.assertIsNone(renewal.spent_window_on_success(200, {}))
        self.assertIsNone(renewal.spent_window_on_success(200, provider.limit_headers(
            provider.usage("a", "u", week_used=99, allowed=True))))
        self.assertEqual(renewal.spent_window_on_success(200, provider.limit_headers(provider.usage(
            "a", "u", week_used=10, session_used=100, allowed=False))), "x-codex-primary-used-percent")
        # A model that states one anyway (as every 200 after a wall did) fails the run
        # through the same list the controller's loop reads.
        model = renewal.Model(self.root, weekly_quota=True)
        refuse_source(self, model)
        (self.root / "renewed").touch()
        base = served(self, model)

        def post():
            return status(urllib.request.Request(base + "/v1/responses", method="POST",
                data=json.dumps(foreground()).encode(),
                headers={"authorization": bearer("old"), "content-type": "application/json"}))

        self.assertEqual(post(), 200)
        self.assertEqual(model.fixture_faults, [])
        renewal.require_no_fixture_fault(model)

        def stating(record, generation):
            record["responseHeaders"] = model.limit_headers(generation)
        with mock.patch.object(model, "answered", side_effect=stating):
            self.assertEqual(post(), 200)
        self.assertEqual(model.fixture_faults, [{"path": "/v1/responses", "error":
            "FixtureFault: a 200 states a spent window in x-codex-secondary-used-percent"}])
        with self.assertRaisesRegex(RuntimeError, "a 200 states a spent window"):
            renewal.require_no_fixture_fault(model)

    def test_a_chains_middle_account_is_refused_with_its_own_spent_week(self):
        model = renewal.Model(self.root, weekly_quota=True, chain=True)
        refuse_source(self, model)
        (self.root / "renewed").touch()
        status, data, _, _, record = model.reply(foreground(), "new")
        self.assertEqual(status, 429)
        self.assertEqual(json.loads(data)["error"]["plan_type"], "pro")
        self.assertEqual(record["responseHeaders"], provider.limit_headers(model.usage_body("new"), wall=True))
        self.assertEqual(record["responseHeaders"]["x-codex-secondary-used-percent"], "100")
        status, _, _, _, record = model.reply(foreground(), "next")
        self.assertEqual(status, 200)
        self.assertEqual(record["responseHeaders"], provider.limit_headers(model.usage_body("next")))
        self.assertEqual(record["responseHeaders"]["x-codex-secondary-used-percent"], "0")

    def test_the_handler_sends_them_and_a_wall_names_its_plan(self):
        model = renewal.Model(self.root, weekly_quota=True)
        base = served(self, model)
        seen = []
        for _ in range(2):
            request = urllib.request.Request(base + "/v1/responses", method="POST",
                data=json.dumps(foreground()).encode(),
                headers={"authorization": bearer("old"), "content-type": "application/json"})
            try:
                with urllib.request.urlopen(request, timeout=10) as reply:
                    seen.append((reply.status, dict(reply.headers), reply.read()))
            except urllib.error.HTTPError as error:
                with error:
                    seen.append((error.code, dict(error.headers), error.read()))
        self.assertEqual([entry[0] for entry in seen], [200, 429])
        for (_, sent, _), record in zip(seen, model.requests):
            self.assertTrue(record["responseHeaders"])
            for key, value in record["responseHeaders"].items():
                self.assertEqual(sent.get(key), value)
        self.assertNotIn("x-codex-active-limit", seen[0][1])
        self.assertEqual(seen[1][1]["x-codex-rate-limit-reached-type"], "rate_limit_reached")
        error = json.loads(seen[1][2])["error"]
        self.assertEqual((error["type"], error["plan_type"]), ("usage_limit_reached", "pro"))
        self.assertIn(error["plan_type"], provider.PLAN_TYPES)

    def test_a_same_account_case_states_its_one_healthy_account(self):
        model = renewal.Model(self.root)
        record = model.reply(foreground())[4]
        self.assertEqual(record["responseHeaders"], provider.limit_headers(model.usage_body("old")))
        self.assertEqual(record["responseHeaders"]["x-codex-secondary-used-percent"], "0")
        # Its expired login is refused as a login, not as a quota: no limit headers.
        status, _, _, _, record = model.reply(foreground())
        self.assertEqual(status, 401)
        self.assertNotIn("responseHeaders", record)


class RouteTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))

    def post(self, base, path, generation="old"):
        request = urllib.request.Request(base + path, data=b"{}", method="POST",
            headers={"authorization": bearer(generation), "content-type": "application/json"})
        try:
            with urllib.request.urlopen(request, timeout=10) as reply:
                return reply.status, json.loads(reply.read())
        except urllib.error.HTTPError as error:
            with error:
                return error.code, json.loads(error.read())

    def test_reset_token_and_unknown_posts_are_answered_by_path(self):
        model = renewal.Model(self.root, weekly_quota=True)
        base = served(self, model)
        status, body = self.post(base, CREDITS + "/consume")
        self.assertEqual((status, body), (200, {"code": "no_credit", "windows_reset": 0}))
        provider.check("consume", body)
        self.assertEqual(self.post(base, "/oauth/token"), (400, {"error": "invalid_grant"}))
        self.assertEqual(model.unrouted_posts, [])
        self.assertEqual(self.post(base, "/api/codex/analytics-events/events?x=1"),
                         (400, {"error": "invalid_grant"}))
        self.assertEqual(model.unrouted_posts,
                         [{"path": "/api/codex/analytics-events/events", "client": "codex"}])
        # None of these is a model request: nothing that fails a run was recorded.
        self.assertEqual((model.unknown_requests, model.requests, model.fixture_faults), ([], [], []))
        self.assertEqual([(event["path"].split("?")[0].rsplit("/", 1)[1], event["kind"])
            for event in model.http_events],
            [("consume", "non-responses"), ("token", "non-responses"), ("events", "non-responses")])
        # A redemption stays visible to the oracle that forbids one.
        with self.assertRaisesRegex(RuntimeError, "attempted an earned-reset redemption"):
            renewal.validate_creditless_yield(model.http_events, [1])

    def test_a_same_account_case_reads_one_healthy_decodable_account(self):
        model = renewal.Model(self.root)
        base = served(self, model)
        with mock.patch.object(model, "usage", side_effect=AssertionError("the quota cases' alone")):
            body = get(base, USAGE, "old")
        provider.check("usage", body)
        self.assertEqual((body["account_id"], body["user_id"]), ("fixture-account", "fixture-user"))
        self.assertTrue(body["rate_limit"]["allowed"])
        self.assertEqual([body["rate_limit"][name]["used_percent"]
            for name in ("primary_window", "secondary_window")], [0, 0])
        self.assertEqual(provider.check("credits", get(base, CREDITS, "old")), provider.credit_list())
        self.assertEqual(get(base, "/v1/models", "old"), {"models": []})
        self.assertEqual(model.fixture_faults, [])
        # The identity is the one a same-account fixture login claims.
        auth = self.root / "auth" / "auth.json"
        renewal.fake_auth(auth, "old", "https://issuer.invalid")
        self.assertEqual(json.loads(auth.read_text())["tokens"]["account_id"], body["account_id"])


class WallEvidenceTests(unittest.TestCase):
    def events(self, *later):
        def event(method, path, client, at, **extra):
            return {"method": method, "path": path, "account": "source", "client": client,
                    "monotonicUs": at, **extra}
        return [event("GET", USAGE, "codex", 1), event("GET", CREDITS, "codex", 2),
                event("POST", "/v1/responses", "codex", 3, kind="foreground", status=200),
                event("POST", "/v1/responses", "codex", 10, kind="foreground", status=429),
                *(event("GET", path, client, at) for path, client, at in later)]

    def trace(self, *rows):
        path = Path(self.enterContext(tempfile.TemporaryDirectory())) / "renewal-control-trace.jsonl"
        if rows:
            path.write_text("".join(json.dumps(row) + "\n" for row in rows))
        return path

    def test_the_counts_after_the_wall_tell_the_causes_of_a_hang_apart(self):
        polling = self.events((USAGE, "codex", 11), (USAGE, "host", 12), (USAGE + "?x=1", "codex", 41))
        self.assertEqual(renewal.wall_evidence(polling, self.trace()),
            "after the first 429: 2 codex usage reads, 0 codex credit-list reads, 0 prepares traced; "
            "the usage reply is not decoding or does not name the login's account")
        unready = self.events((USAGE, "codex", 11), (CREDITS, "codex", 12), (CREDITS, "host", 13),
                              (USAGE, "codex", 41), (CREDITS, "codex", 42))
        self.assertIn("2 codex usage reads, 2 codex credit-list reads, 0 prepares traced; "
            "the earned reset was read but the handoff was not ready",
            renewal.wall_evidence(unready, self.trace({"kind": "rotation", "verb": "prepare",
                "monotonicUs": 9}, {"kind": "renewal", "verb": "prepare", "monotonicUs": 20})))
        asked = renewal.wall_evidence(unready, self.trace(
            {"kind": "rotation", "verb": "prepare", "monotonicUs": 20},
            {"kind": "rotation", "verb": "prepare", "monotonicUs": 50}))
        self.assertIn("2 prepares traced; the turn ended and the host was asked", asked)
        self.assertIn("0 codex usage reads, 0 codex credit-list reads, 0 prepares traced; "
            "the walled turn read nothing", renewal.wall_evidence(self.events(), self.trace()))
        self.assertEqual(renewal.wall_evidence(self.events()[:3], self.trace()),
            "no foreground request was refused with 429")
        # An unreadable trace counts no prepare; it never hides the request counts.
        broken = self.trace()
        broken.write_text("{not json\n")
        self.assertIn("2 codex credit-list reads, 0 prepares traced", renewal.wall_evidence(unready, broken))

    def test_a_hang_names_the_hosts_live_snapshot_verdicts(self):
        self.assertEqual(renewal.live_snapshot_outcomes(self.trace()), "live snapshot: none traced")
        refused = self.trace({"kind": "live-snapshot", "outcome": "no-live-primary", "pid": None},
            {"kind": "session-status", "outcome": "ok"},
            {"kind": "live-snapshot", "outcome": "no-live-resume", "pid": None},
            {"kind": "live-snapshot", "outcome": "terminal", "pid": None})
        self.assertEqual(renewal.live_snapshot_outcomes(refused),
            "live snapshot: no-live-primary, no-live-resume, terminal")
        published = self.trace({"kind": "live-snapshot", "outcome": "published", "pid": 7},
            {"kind": "live-snapshot", "outcome": 3})
        self.assertEqual(renewal.live_snapshot_outcomes(published), "live snapshot: published")
        broken = self.trace()
        broken.write_text("{not json\n")
        self.assertEqual(renewal.live_snapshot_outcomes(broken), "live snapshot: none traced")


if __name__ == "__main__":
    unittest.main()
