# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Negative controls for the production-bound pre-Signal Stop oracle."""
import copy
import re
import unittest

from renewal_follow import validate_presignal_stop


class ParallelLoginOracle(unittest.TestCase):
    def test_a_wrong_account_or_task_cannot_count_toward_concurrency(self):
        from parallel_logins import Accounts, prompt
        from renewal_follow import access_token
        accounts = Accounts(2)
        headers = {"Authorization": "Bearer " + access_token("0", "parallel-account-0"),
                   "ChatGPT-Account-Id": "parallel-account-0"}
        body = {"input": [{"role": "user", "content": [
            {"type": "input_text", "text": prompt(1)}]}]}
        with self.assertRaises(ValueError):
            accounts.respond(headers, body)
        with self.assertRaises(ValueError):
            accounts.identity(dict(headers, **{"ChatGPT-Account-Id": "parallel-account-1"}))
        self.assertEqual(accounts.arrived, set())

    def test_first_request_is_held_until_every_distinct_account_arrives(self):
        import threading
        from parallel_logins import Accounts, prompt
        from renewal_follow import access_token
        accounts = Accounts(2)
        finished = [threading.Event(), threading.Event()]
        errors = []

        def request(index):
            try:
                accounts.respond({"Authorization": "Bearer " + access_token(str(index)),
                    "ChatGPT-Account-Id": f"parallel-account-{index}"},
                    {"input": [{"role": "user", "content": [
                        {"type": "input_text", "text": prompt(index)}]}]})
                finished[index].set()
            except ValueError as error:
                errors.append(str(error))

        first = threading.Thread(target=request, args=(0,))
        second = threading.Thread(target=request, args=(1,))
        first.start()
        try:
            with accounts.condition:
                self.assertTrue(accounts.condition.wait_for(lambda: accounts.arrived == {0}, timeout=2))
                self.assertFalse(finished[0].is_set())
            second.start()
            self.assertTrue(finished[0].wait(2))
            self.assertTrue(finished[1].wait(2))
        finally:
            with accounts.condition:
                accounts.aborted = True
                accounts.condition.notify_all()
            first.join(timeout=2)
            if second.ident is not None:
                second.join(timeout=2)
        self.assertEqual(errors, [])


class QuotaFixture(unittest.TestCase):
    def test_chain_exhausts_middle_only_after_its_failed_request(self):
        import tempfile
        from pathlib import Path
        from renewal_follow import Model
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            model = Model(root, weekly_quota=True, chain=True)
            body = {"input": [{"role": "user", "content": [
                {"type": "input_text", "text": "original"}]}],
                "tools": [{"type": "function", "name": "update_plan"}]}
            model.reply(body, "old")
            self.assertEqual(model.reply(body, "old")[0], 429)
            root.joinpath("renewed").touch()
            self.assertEqual(model.usage(False, True)["rate_limit"]["secondary_window"]["used_percent"], 0)
            self.assertEqual(model.reply(body, "new")[0], 429)
            self.assertEqual(model.usage(False, True)["rate_limit"]["secondary_window"]["used_percent"], 100)
            self.assertEqual(model.usage(False, False, True)["rate_limit"]["secondary_window"]["used_percent"], 0)
            self.assertEqual(model.reply(body, "next")[0], 200)

    def test_usage_server_requires_its_explicit_tls_root(self):
        import json
        import ssl
        import tempfile
        import urllib.error
        import urllib.request
        from pathlib import Path
        from renewal_follow import Model, access_token, serve
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            model = Model(root, weekly_quota=True)
            # The source's usage says spent only once the provider has refused it.
            turn = {"input": [], "tools": [{"type": "function", "name": "update_plan"}]}
            self.assertEqual([model.reply(turn, "old")[0] for _ in range(2)], [200, 429])
            server, worker = serve(model)
            try:
                request = urllib.request.Request(
                    f"https://127.0.0.1:{server.server_port}/api/codex/usage",
                    headers={"Authorization": "Bearer " + access_token("old", "source-account")})
                with self.assertRaises(urllib.error.URLError):
                    urllib.request.urlopen(request, timeout=2)
                context = ssl.create_default_context(cafile=str(root / "fixture-ca.pem"))
                with urllib.request.urlopen(request, context=context, timeout=2) as reply:
                    usage = json.load(reply)
                self.assertEqual(usage["rate_limit"]["secondary_window"]["used_percent"], 100)
                self.assertEqual(ssl.PEM_cert_to_DER_cert((root / "fixture-ca.pem").read_text()),
                                 (root / "fixture-ca.der").read_bytes())
            finally:
                server.shutdown()
                server.server_close()
                worker.join(timeout=2)

    def test_recovery_marker_cannot_mix_login_and_quota_custody(self):
        from renewal_follow import recovery_marker
        marker = {"receipt": "ab" * 16}
        self.assertEqual(recovery_marker({"rotation": marker}), marker)
        self.assertEqual(recovery_marker({"renewal": marker}), marker)
        for invalid in ({"renewal": marker, "rotation": marker}, {"rotation": True}):
            with self.assertRaises(RuntimeError):
                recovery_marker(invalid)

    def test_weekly_provider_exhausts_source_and_keeps_target_fresh(self):
        import json
        import tempfile
        from pathlib import Path
        from renewal_follow import Model
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            model = Model(root, weekly_quota=True)
            request = {"input": [{"role": "user", "content": [
                {"type": "input_text", "text": "original request"}]}],
                "tools": [{"type": "function", "name": "update_plan"}]}
            self.assertEqual(model.reply(request)[0], 200)
            status, body, *_ = model.reply(request)
            self.assertEqual(status, 429)
            self.assertEqual(json.loads(body)["error"]["type"], "usage_limit_reached")
            root.joinpath("renewed").touch()
            source = model.usage(True, False)
            target = model.usage(False, True)
            self.assertEqual(source["rate_limit"]["secondary_window"]["used_percent"], 100)
            self.assertFalse(source["rate_limit"]["allowed"])
            self.assertEqual(target["rate_limit"]["secondary_window"]["used_percent"], 0)
            self.assertTrue(target["rate_limit"]["allowed"])
            # Both replies are ones the client decodes: each window says when it
            # resets both ways, and the credit summary is present.
            import provider_fixture
            for usage in (source, target):
                provider_fixture.check("usage", usage)
                week = usage["rate_limit"]["secondary_window"]
                self.assertEqual(week["limit_window_seconds"], 604800)
                self.assertEqual(week["reset_after_seconds"], 86400)
                self.assertEqual(usage["rate_limit_reset_credits"], {"available_count": 0})
            exhausted = Model(root, weekly_quota=True, all_exhausted=True)
            self.assertEqual(exhausted.usage(False, True)["rate_limit"]["secondary_window"]["used_percent"], 100)
            self.assertFalse(exhausted.usage(False, True)["rate_limit"]["allowed"])
            with self.assertRaises(RuntimeError):
                model.usage(False, False)

    def test_quota_fixture_credentials_name_distinct_native_accounts(self):
        import base64
        import json
        import tempfile
        from pathlib import Path
        from renewal_follow import fake_auth
        with tempfile.TemporaryDirectory() as directory:
            claims = []
            for label in ("source", "target"):
                path = Path(directory) / label / "auth.json"
                fake_auth(path, label, "https://127.0.0.1:1234", account=label,
                          email=f"{label}@example.invalid", user=f"{label}-user")
                auth = json.loads(path.read_text())
                encoded = auth["tokens"]["id_token"].split(".")[1]
                claim = json.loads(base64.urlsafe_b64decode(encoded + "=" * (-len(encoded) % 4)))
                self.assertEqual(claim["email"], f"{label}@example.invalid")
                self.assertEqual(claim["sub"], f"{label}-user")
                claims.append(claim["https://api.openai.com/auth"]["chatgpt_account_id"])
            self.assertEqual(claims, ["source", "target"])


class FiveHourWallSoon(unittest.TestCase):
    """The fixture of `five-hour-wall-soon` and the oracle of its run."""

    def setUp(self):
        import tempfile
        from pathlib import Path
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))

    def model(self):
        from renewal_follow import Model
        model = Model(self.root, weekly_quota=True, wall="five-hour-soon")
        self.clock = [1000.0]
        model.monotonic = lambda: self.clock[0]
        return model

    @staticmethod
    def turn():
        return {"input": [], "tools": [{"type": "function", "name": "update_plan"}]}

    def test_the_wall_is_on_the_five_hour_window_and_lifts_twenty_seconds_on(self):
        import json
        import time
        import provider_fixture
        from renewal_follow import WALL_LIFT_SECONDS, WALL_RESET_SECONDS
        model = self.model()
        before = provider_fixture.check("usage", model.usage(True, False))
        self.assertTrue(before["rate_limit"]["allowed"])
        self.assertEqual(model.reply(self.turn(), "old")[0], 200)
        started = int(time.time())
        status, data, _, retry_after, record = model.reply(self.turn(), "old")
        self.assertEqual((status, retry_after), (429, None))
        error = json.loads(data)["error"]
        self.assertEqual((error["type"], error["plan_type"]), ("usage_limit_reached", "pro"))
        self.assertIn(error["resets_at"] - started, (WALL_RESET_SECONDS, WALL_RESET_SECONDS + 1))
        headers = record["responseHeaders"]
        self.assertEqual((headers["x-codex-primary-used-percent"], headers["x-codex-primary-window-minutes"],
                          headers["x-codex-secondary-used-percent"], headers["x-codex-active-limit"]),
                         ("100", "300", "10", "codex"))
        self.assertEqual(int(headers["x-codex-primary-reset-at"]), error["resets_at"])

        def walled():
            body = provider_fixture.check("usage", model.usage(True, False))
            limit = body["rate_limit"]
            return (limit["allowed"], limit["primary_window"]["used_percent"],
                    limit["primary_window"]["limit_window_seconds"],
                    limit["secondary_window"]["used_percent"]), limit["primary_window"]

        for at in (1000.0, 1000.0 + WALL_LIFT_SECONDS - 0.01):
            self.clock[0] = at
            verdict, window = walled()
            self.assertEqual(verdict, (False, 100, 18000, 10))
            # The reset stays where the refusal put it, within fifteen minutes.
            self.assertEqual(window["reset_at"], error["resets_at"])
            self.assertLessEqual(window["reset_after_seconds"], WALL_RESET_SECONDS)
            self.assertLess(WALL_RESET_SECONDS, 15 * 60)
        # A request sent again before the lift is refused again, on the same reset.
        status, data, _, _, record = model.reply(self.turn(), "old")
        self.assertEqual((status, json.loads(data)["error"]["resets_at"]), (429, error["resets_at"]))
        self.assertEqual(record["responseHeaders"]["x-codex-primary-used-percent"], "100")
        self.clock[0] = 1000.0 + WALL_LIFT_SECONDS
        verdict, _ = walled()
        self.assertEqual(verdict, (True, 0, 18000, 10))
        status, _, _, _, record = model.reply(self.turn(), "old")
        self.assertEqual(status, 200)
        self.assertEqual(record["responseHeaders"]["x-codex-primary-used-percent"], "0")
        self.assertNotIn("x-codex-active-limit", record["responseHeaders"])
        # The week was never the wall, and the weekly cases' marker played no part.
        self.assertFalse(model.source_walled)
        self.assertFalse((self.root / "renewed").exists())

    def test_the_walls_reset_does_not_move_when_a_second_passes_inside_one_reading(self):
        import itertools
        import json
        import time
        from unittest import mock
        from renewal_follow import WALL_RESET_SECONDS
        model = self.model()
        model.reply(self.turn(), "old")
        with mock.patch.object(time, "time", return_value=5000.25):
            resets_at = json.loads(model.reply(self.turn(), "old")[1])["error"]["resets_at"]
        self.assertEqual(resets_at, 5000 + WALL_RESET_SECONDS)
        # Every look at the clock is a second later than the one before.
        ticks = itertools.count(5007)
        with mock.patch.object(time, "time", side_effect=lambda: float(next(ticks))):
            window = model.usage_body("old")["rate_limit"]["primary_window"]
            headers = model.limit_headers("old", wall=True)
        self.assertEqual(window["reset_at"], resets_at)
        self.assertEqual(window["reset_at"] - window["reset_after_seconds"], 5007)
        self.assertEqual(int(headers["x-codex-primary-reset-at"]), resets_at)

    def test_the_other_account_is_usable_throughout_and_answers_at_once(self):
        import time
        import provider_fixture
        from renewal_follow import QUOTA_ACCOUNTS
        model = self.model()

        def destination():
            started = time.monotonic()
            body = provider_fixture.check("usage", model.usage(False, True))
            # The weekly cases hold this reading for up to 60 s; this case opens nothing.
            self.assertLess(time.monotonic() - started, 5)
            self.assertEqual((body["account_id"], body["user_id"]), QUOTA_ACCOUNTS["new"][1:])
            return (body["rate_limit"]["allowed"], body["rate_limit"]["primary_window"]["used_percent"],
                    body["rate_limit"]["secondary_window"]["used_percent"])

        self.assertEqual(destination(), (True, 0, 0))
        self.assertEqual([model.reply(self.turn(), "old")[0] for _ in range(2)], [200, 429])
        # While the source is walled, and nothing ever having created `renewed`.
        self.assertFalse(model.usage(True, False)["rate_limit"]["allowed"])
        self.assertEqual(destination(), (True, 0, 0))
        self.assertFalse((self.root / "renewed").exists())
        self.assertEqual(model.usage_reads, ["target", "source", "target"])

    def test_the_case_is_declared_and_in_the_matrix(self):
        import renewal_follow
        import renewal_matrix
        from renewal_follow import Model
        self.assertEqual(renewal_follow.CASE_BUDGETS["five-hour-wall-soon"], (270, 300))
        # Passed six times on hosts built from a clean aterm commit before it was put in
        # (PARALLEL-LOGINS.md, "Qualifying runs of 2026-11-13" and "Repeat runs").
        self.assertIn("five-hour-wall-soon", renewal_matrix.CASES)
        for wall in ("five-hour", "five-hour-near", "weekly"):
            with self.assertRaisesRegex(RuntimeError, "unknown fixture wall"):
                Model(self.root, weekly_quota=True, wall=wall)
        with self.assertRaisesRegex(RuntimeError, "unknown fixture wall"):
            Model(self.root, wall="five-hour-soon")
        # A pool with nowhere to go would make "no handoff" say nothing.
        for options in ({"all_exhausted": True}, {"chain": True}):
            with self.assertRaisesRegex(RuntimeError, "one usable destination"):
                Model(self.root, weekly_quota=True, wall="five-hour-soon", **options)

    def test_served_usage_reads_record_the_verdict_the_reader_was_given(self):
        import json
        import urllib.request
        from renewal_follow import access_token, serve
        model = self.model()
        server, worker = serve(model, secure=False)
        try:
            request = urllib.request.Request(f"http://127.0.0.1:{server.server_port}/api/codex/usage",
                headers={"authorization": "Bearer " + access_token("old", "source-account")})
            for _ in range(2):
                model.reply(self.turn(), "old")
                with urllib.request.urlopen(request, timeout=10) as reply:
                    json.load(reply)
        finally:
            server.shutdown()
            server.server_close()
            worker.join(timeout=2)
        self.assertEqual([event["allowed"] for event in model.http_events], [True, False])
        # And each window's figure, with the five-hour window's distance to its reset.
        from renewal_follow import WALL_RESET_SECONDS
        healthy, walled = model.http_events
        self.assertEqual((healthy["sessionUsed"], healthy["weekUsed"]), (0, 10))
        self.assertEqual((walled["sessionUsed"], walled["weekUsed"]), (100, 10))
        self.assertIn(walled["sessionResetsIn"], (WALL_RESET_SECONDS - 1, WALL_RESET_SECONDS))

    @staticmethod
    def events(retry_at=31, extra=()):
        def event(method, path, at, account="source", **more):
            return {"method": method, "path": path, "account": account, "client": "codex",
                    "monotonicUs": at * 1_000_000, **more}
        usage, responses = "/api/codex/usage", "/v1/responses"
        return sorted([
            event("GET", usage, 1, allowed=True),
            event("POST", responses, 2, kind="foreground", status=200),
            event("POST", responses, 3, kind="thread-title", status=200),
            event("POST", responses, 10, kind="foreground", status=429),
            event("GET", usage, 11, allowed=False),
            event("GET", usage, 30, allowed=True),
            event("POST", responses, retry_at, kind="foreground", status=200),
            *[event(*row[:3], **row[3]) for row in extra]], key=lambda entry: entry["monotonicUs"])

    def test_the_oracle_accepts_one_wall_waited_out_on_its_own_account(self):
        from renewal_follow import validate_wall_waited_out
        self.assertEqual(validate_wall_waited_out(self.events()), {
            "wallUs": 10_000_000, "retryUs": 31_000_000, "waitedSeconds": 21.0,
            "walledUsageReads": 1, "usageReads": 2})

    def test_the_oracle_rejects_a_handoff_an_early_retry_or_an_unread_wall(self):
        from renewal_follow import validate_wall_waited_out
        usage, responses = "/api/codex/usage", "/v1/responses"
        cases = {
            "retried before its wall lifted": self.events(retry_at=29),
            "refused 2 times": self.events(extra=[("POST", responses, 15, {"kind": "foreground", "status": 429})]),
            "refused 0 times": [dict(event, status=200) if event.get("status") == 429 else event
                                for event in self.events()],
            "other than the walled one": self.events(extra=[
                ("POST", responses, 40, {"kind": "foreground", "status": 200, "account": "target"})]),
            "not the second request, retried once": self.events(extra=[
                ("POST", responses, 50, {"kind": "foreground", "status": 200})]),
            "never read its own wall": [event for event in self.events() if event.get("allowed") is not False],
        }
        # A title request from another login is a model request too.
        cases["other than the walled one "] = self.events(extra=[
            ("POST", responses, 40, {"kind": "thread-title", "status": 200, "account": "target"})])
        # The host's reading of the wall is not the walled turn's.
        hosted = self.events()
        next(event for event in hosted if event.get("allowed") is False)["client"] = "host"
        cases["never read its own wall "] = hosted
        for message, events in cases.items():
            with self.subTest(message=message):
                with self.assertRaisesRegex(RuntimeError, message.strip()):
                    validate_wall_waited_out(events)


class FiveHourWallFar(unittest.TestCase):
    """The fixture of `five-hour-wall-far`: a five-hour wall two hours from its reset, on a
    usable week, beside a destination the weekly cases' `renewed` opens."""

    def setUp(self):
        import tempfile
        from pathlib import Path
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))

    def model(self):
        from renewal_follow import Model
        model = Model(self.root, weekly_quota=True, wall="five-hour-far")
        self.clock = [1000.0]
        model.monotonic = lambda: self.clock[0]
        return model

    turn = staticmethod(FiveHourWallSoon.turn)

    def test_the_wall_is_on_the_five_hour_window_far_from_its_reset_and_never_lifts(self):
        import json
        import time
        import provider_fixture
        from renewal_follow import FAR_RESET_SECONDS, HANDOFF_SKEW_SECONDS, WALL_LIFT_SECONDS
        model = self.model()
        before = provider_fixture.check("usage", model.usage(True, False))
        self.assertEqual((before["rate_limit"]["allowed"],
                          before["rate_limit"]["secondary_window"]["used_percent"]), (True, 10))
        self.assertEqual(model.reply(self.turn(), "old")[0], 200)
        started = int(time.time())
        status, data, _, retry_after, record = model.reply(self.turn(), "old")
        self.assertEqual((status, retry_after), (429, None))
        error = json.loads(data)["error"]
        self.assertEqual((error["type"], error["message"]),
                         ("usage_limit_reached", "Fixture five-hour limit reached"))
        self.assertIn(error["resets_at"] - started, (FAR_RESET_SECONDS, FAR_RESET_SECONDS + 1))
        headers = record["responseHeaders"]
        self.assertEqual((headers["x-codex-primary-used-percent"], headers["x-codex-primary-window-minutes"],
                          headers["x-codex-secondary-used-percent"], headers["x-codex-active-limit"]),
                         ("100", "300", "10", "codex"))
        self.assertEqual(int(headers["x-codex-primary-reset-at"]), error["resets_at"])
        # Far: beyond the fifteen minutes a host waits out, and the minute codex-alab adds.
        self.assertGreater(FAR_RESET_SECONDS, 4 * (15 * 60 + HANDOFF_SKEW_SECONDS))
        self.assertLess(FAR_RESET_SECONDS, provider_fixture.SESSION_WINDOW_SECONDS)
        # It stands for the whole run: long after the near wall would have lifted.
        for at in (1000.0, 1000.0 + WALL_LIFT_SECONDS, 1000.0 + 600):
            self.clock[0] = at
            limit = provider_fixture.check("usage", model.usage(True, False))["rate_limit"]
            self.assertEqual((limit["allowed"], limit["primary_window"]["used_percent"],
                              limit["primary_window"]["limit_window_seconds"],
                              limit["secondary_window"]["used_percent"]), (False, 100, 18000, 10))
            self.assertEqual(limit["primary_window"]["reset_at"], error["resets_at"])
            self.assertGreater(limit["primary_window"]["reset_after_seconds"], FAR_RESET_SECONDS - 10)
            status, data, _, _, record = model.reply(self.turn(), "old")
            self.assertEqual((status, json.loads(data)["error"]["resets_at"]), (429, error["resets_at"]))
            self.assertEqual(record["responseHeaders"]["x-codex-secondary-used-percent"], "10")
        self.assertTrue(model.source_walled)
        # Opening the destination does not lift the source's wall, as it does in the weekly cases.
        (self.root / "renewed").touch()
        self.assertFalse(model.usage(True, False)["rate_limit"]["allowed"])
        self.assertEqual(model.reply(self.turn(), "old")[0], 429)
        # A login the fixture does not know is the walled one, as in the weekly cases.
        self.assertEqual(model.reply(self.turn(), "")[0], 429)

    @staticmethod
    def title():
        from renewal_follow import TITLE_FORMAT, TITLE_INSTRUCTIONS
        return {"input": [{"role": "user", "content": [{"type": "input_text",
                    "text": TITLE_INSTRUCTIONS + " for this conversation."}]}],
                "tools": [], "text": {"format": TITLE_FORMAT}}

    def test_the_walled_login_is_refused_every_model_request_its_title_too(self):
        import json
        from renewal_follow import Model, request_kind
        self.assertEqual(request_kind(self.title()), "thread-title")
        model = self.model()
        # Before the wall a title is answered, as any request is.
        self.assertEqual(model.reply(self.turn(), "old")[0], 200)
        status, _, _, _, record = model.reply(self.title(), "old")
        self.assertEqual((status, record["responseHeaders"]["x-codex-primary-used-percent"]), (200, "0"))
        refused = model.reply(self.turn(), "old")
        self.assertEqual(refused[0], 429)
        # At it, the title is refused as the turn was: the same wall, the same reset.
        for generation in ("old", ""):
            status, data, _, _, record = model.reply(self.title(), generation)
            self.assertEqual(status, 429)
            self.assertEqual(json.loads(data)["error"], json.loads(refused[1])["error"])
            self.assertEqual((record["responseHeaders"]["x-codex-primary-used-percent"],
                              record["responseHeaders"]["x-codex-secondary-used-percent"]), ("100", "10"))
            self.assertNotIn("limitHeadersWithheld", record)
        # A title is no turn: it is counted apart and moves no wall.
        self.assertEqual((len(model.requests), len(model.auxiliary_requests)), (2, 3))
        # The other account's title is answered once it is open.
        (self.root / "renewed").touch()
        self.assertEqual(model.reply(self.title(), "new")[0], 200)
        self.assertEqual(model.reply(self.title(), "old")[0], 429)
        # The weekly wall and the near five-hour wall answer theirs, as before.
        for name, options in (("weekly", {}), ("soon", {"wall": "five-hour-soon"})):
            (self.root / name).mkdir()
            other = Model(self.root / name, weekly_quota=True, **options)
            self.assertEqual([other.reply(self.turn(), "old")[0] for _ in range(2)], [200, 429])
            status, _, _, _, record = other.reply(self.title(), "old")
            self.assertEqual((status, record["limitHeadersWithheld"]), (200, True))

    def test_the_week_is_never_the_wall_whoever_reads_and_whenever(self):
        model = self.model()
        self.assertFalse(model.week_spent(True, False))
        self.assertEqual([model.reply(self.turn(), "old")[0] for _ in range(2)], [200, 429])
        (self.root / "renewed").touch()
        for _ in range(3):
            limit = model.usage(True, False)["rate_limit"]
            self.assertEqual((limit["secondary_window"]["used_percent"], limit["allowed"]), (10, False))
            self.assertFalse(model.week_spent(True, False))
        self.assertEqual(model.usage(False, True)["rate_limit"]["secondary_window"]["used_percent"], 0)

    def test_the_destination_is_held_until_opened_then_usable_and_answers(self):
        import threading
        import time
        import provider_fixture
        from renewal_follow import QUOTA_ACCOUNTS
        model = self.model()
        self.assertEqual([model.reply(self.turn(), "old")[0] for _ in range(2)], [200, 429])
        # Closed until the controller has recorded the source and published `renew`:
        # what a reading would say, and a request sent there before then.
        closed = provider_fixture.check("usage", model.usage_body("new"))
        self.assertFalse(closed["rate_limit"]["allowed"])
        self.assertEqual(model.reply(self.turn(), "new")[0], 429)
        # Its first reading waits for `renewed`, as in the weekly cases.
        opener = threading.Timer(0.3, (self.root / "renewed").touch)
        started = time.monotonic()
        opener.start()
        try:
            body = provider_fixture.check("usage", model.usage(False, True))
        finally:
            opener.join()
        self.assertGreaterEqual(time.monotonic() - started, 0.25)
        self.assertEqual((body["account_id"], body["user_id"]), QUOTA_ACCOUNTS["new"][1:])
        limit = body["rate_limit"]
        self.assertEqual((limit["allowed"], limit["primary_window"]["used_percent"],
                          limit["secondary_window"]["used_percent"]), (True, 0, 0))
        status, _, _, _, record = model.reply(self.turn(), "new")
        self.assertEqual(status, 200)
        self.assertEqual(record["responseHeaders"]["x-codex-primary-used-percent"], "0")

    def test_the_unnamed_wall_is_refused_with_no_reset_in_its_body(self):
        """`five-hour-wall-far-unnamed`: the same wall, its 429's body naming no reset while
        its headers and every usage read still state it."""
        import json
        import time
        import provider_fixture
        import renewal_follow
        from renewal_follow import FAR_RESET_SECONDS, Model
        model = Model(self.root, weekly_quota=True, wall="five-hour-far", wall_names_reset=False)
        model.monotonic = lambda: 1000.0
        self.assertEqual(model.reply(self.turn(), "old")[0], 200)
        started = int(time.time())
        status, data, _, _, record = model.reply(self.turn(), "old")
        self.assertEqual(status, 429)
        error = json.loads(data)["error"]
        self.assertEqual((error["type"], "resets_at" in error), ("usage_limit_reached", False))
        self.assertIs(record["refusalNamedReset"], False)
        reset = int(record["responseHeaders"]["x-codex-primary-reset-at"])
        self.assertIn(reset - started, (FAR_RESET_SECONDS, FAR_RESET_SECONDS + 1))
        limit = provider_fixture.check("usage", model.usage(True, False))["rate_limit"]
        self.assertEqual((limit["allowed"], limit["primary_window"]["reset_at"]), (False, reset))
        # The named case's refusal says it named its reset.
        named = Model(self.root, weekly_quota=True, wall="five-hour-far")
        named.reply(self.turn(), "old")
        self.assertIs(named.reply(self.turn(), "old")[4]["refusalNamedReset"], True)
        # Declared, proven by the far wall's own oracle, and only the far wall goes unnamed.
        self.assertEqual(renewal_follow.CASE_BUDGETS["five-hour-wall-far-unnamed"], (270, 300))
        self.assertEqual(renewal_follow.CASE_PROOFS["five-hour-wall-far-unnamed"],
                         renewal_follow.CASE_PROOFS["five-hour-wall-far"])
        self.assertEqual(renewal_follow.FAR_WALL_CASES,
                         {"five-hour-wall-far": True, "five-hour-wall-far-unnamed": False})
        for wall in (None, "five-hour-soon"):
            with self.assertRaisesRegex(RuntimeError, "only the far five-hour wall"):
                Model(self.root, weekly_quota=True, wall=wall, wall_names_reset=False)
        # The fixture's check takes a body that names no reset, and still refuses one that
        # names another than the headers state.
        usage = model.usage_body("old")
        provider_fixture.check("refusal", provider_fixture.refusal(usage, "x", names_reset=False))
        wrong = provider_fixture.refusal(usage, "x")
        wrong["body"]["error"]["resets_at"] += 1
        with self.assertRaisesRegex(provider_fixture.FixtureFault, "not the spent window's reset"):
            provider_fixture.check("refusal", wrong)

    def test_the_case_is_declared_and_its_pool_has_one_destination(self):
        import renewal_follow
        from renewal_follow import Model
        self.assertEqual(renewal_follow.CASE_BUDGETS["five-hour-wall-far"], (270, 300))
        # In the matrix since its three runs of 2026-11-13 passed (PARALLEL-LOGINS.md).
        import renewal_matrix
        self.assertEqual(renewal_matrix.CASES[-2:], ("five-hour-wall-soon", "five-hour-wall-far"))
        self.assertLessEqual(set(renewal_matrix.CASES), set(renewal_follow.CASE_BUDGETS))
        # Failing by name comes well before the hang detector, and before the thirty
        # seconds after which a turn that kept its request reads its credits again.
        self.assertLess(renewal_follow.FAR_YIELD_SECONDS * 2, 270)
        self.assertLess(renewal_follow.FAR_YIELD_SECONDS, 30)
        with self.assertRaisesRegex(RuntimeError, "unknown fixture wall"):
            Model(self.root, wall="five-hour-far")
        for options in ({"all_exhausted": True}, {"chain": True}):
            with self.assertRaisesRegex(RuntimeError, "one usable destination"):
                Model(self.root, weekly_quota=True, wall="five-hour-far", **options)

    def test_served_usage_reads_record_what_each_window_stated(self):
        import json
        import urllib.request
        from renewal_follow import FAR_RESET_SECONDS, access_token, serve
        model = self.model()
        (self.root / "renewed").touch()
        server, worker = serve(model, secure=False)
        try:
            def read(generation, account):
                request = urllib.request.Request(f"http://127.0.0.1:{server.server_port}/api/codex/usage",
                    headers={"authorization": "Bearer " + access_token(generation, account)})
                with urllib.request.urlopen(request, timeout=10) as reply:
                    json.load(reply)
            for _ in range(2):
                model.reply(self.turn(), "old")
                read("old", "source-account")
            read("new", "target-account")
        finally:
            server.shutdown()
            server.server_close()
            worker.join(timeout=2)
        seen = [(event["account"], event["allowed"], event["sessionUsed"], event["weekUsed"])
                for event in model.http_events]
        self.assertEqual(seen, [("source", True, 0, 10), ("source", False, 100, 10), ("target", True, 0, 0)])
        self.assertIn(model.http_events[1]["sessionResetsIn"], (FAR_RESET_SECONDS - 1, FAR_RESET_SECONDS))


class FiveHourWallMoved(unittest.TestCase):
    """What fails a `five-hour-wall-far` run by name while it runs, and its verdict."""

    RECEIPT = "ab" * 16
    WALL_UNIX = 5000

    def setUp(self):
        import tempfile
        from pathlib import Path
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.pool = self.root / "host-state" / "accounts-codex"
        self.trace = self.root / "renewal-control-trace.jsonl"
        self.OLD = str(self.root / "auth-old" / "auth.json")
        self.NEW = str(self.root / "auth-new" / "auth.json")
        # The session processes the run's loop has seen on the walled login.
        self.processes = set()

    @staticmethod
    def events(extra=(), drop=()):
        """A run that moved: refused at 10 s, its title refused too, read by its turn and
        the host, answered on the other account at 25 s. `drop` removes the rows at those
        seconds."""
        def event(method, path, at, account="source", client="codex", **more):
            return {"method": method, "path": path, "account": account, "client": client,
                    "monotonicUs": at * 1_000_000, **more}
        usage, responses, credits = "/api/codex/usage", "/v1/responses", "/api/codex/rate-limit-reset-credits"
        healthy = {"allowed": True, "sessionUsed": 0, "weekUsed": 10, "sessionResetsIn": 3600}
        walled = {"allowed": False, "sessionUsed": 100, "weekUsed": 10, "sessionResetsIn": 7200}
        open_ = {"allowed": True, "sessionUsed": 0, "weekUsed": 0, "sessionResetsIn": 3600}
        rows = [
            event("GET", usage, 1, **healthy),
            event("POST", responses, 2, kind="foreground", status=200),
            event("POST", responses, 10, kind="foreground", status=429),
            event("POST", responses, 11, kind="thread-title", status=429),
            event("GET", usage, 12, **walled),
            event("GET", credits, 13),
            event("GET", usage, 14, client="host", **walled),
            event("GET", usage, 15, account="target", client="host", **open_),
            event("GET", usage, 22, account="target", **open_),
            event("POST", responses, 25, account="target", kind="foreground", status=200),
            *[event(*row[:3], **row[3]) for row in extra]]
        return sorted((row for row in rows if row["monotonicUs"] // 1_000_000 not in drop),
                      key=lambda entry: entry["monotonicUs"])

    def requests(self, named=True, **headers):
        """The turn's three requests; the second, the refusal, names its reset unless not
        `named`."""
        stated = {"x-codex-primary-used-percent": "100", "x-codex-primary-window-minutes": "300",
                  "x-codex-primary-reset-at": str(self.WALL_UNIX + 7200),
                  "x-codex-secondary-used-percent": "10", "x-codex-secondary-window-minutes": "10080",
                  "x-codex-secondary-reset-at": str(self.WALL_UNIX + 86400),
                  "x-codex-active-limit": "codex", **headers}
        return [{"unixAt": self.WALL_UNIX - 8.0},
                {"unixAt": self.WALL_UNIX + 0.4, "responseHeaders": stated, "refusalNamedReset": named},
                {"unixAt": self.WALL_UNIX + 15.0}]

    def calls(self, *rows):
        import json
        self.trace.write_text("".join(json.dumps(
            {"kind": "rotation", "verb": verb, "reply": reply, "monotonicUs": at * 1_000_000}) + "\n"
            for verb, reply, at in rows))

    def moved(self):
        """The control trace of the run `events()` describes."""
        self.calls(("prepare", "OK codex-rotation state=waiting", 13),
                   ("prepare", f"OK codex-rotation state=ready id={self.RECEIPT} thread=t turn=w", 18),
                   ("commit", f"OK codex-rotation state=waiting id={self.RECEIPT} thread=t", 18))

    def receipt(self, **changes):
        import json
        directory = self.pool / "renewal-follow"
        directory.mkdir(parents=True, exist_ok=True)
        value = dict({"id": self.RECEIPT, "phase": "consumed", "old_auth": self.OLD, "new_auth": self.NEW,
            "intent": {"purpose": "weekly_quota", "identity": {"account": "source-hash", "user": "u"}},
            "destination": {"account": "target-hash", "user": "v"}}, **changes)
        (directory / (self.RECEIPT + ".json")).write_text(json.dumps(value))
        return value

    def session(self, pid, auth):
        import json
        sessions = self.root / "codex-home" / "alab" / "sessions"
        sessions.mkdir(parents=True, exist_ok=True)
        (sessions / f"{pid}.json").write_text(json.dumps({"sessionId": "t", "env": {"CODEX_AUTH_PATH": auth}}))

    def ledger(self, **changes):
        import json
        self.pool.mkdir(parents=True, exist_ok=True)
        main = {"walls": {"five_hour": {"at": self.WALL_UNIX, "cleared": False, "until": self.WALL_UNIX + 7200}},
                "sample": {"session_pct": 100, "week_pct": 10}}
        main.update({key: changes.pop(key) for key in ("walls", "sample") if key in changes})
        state = dict({"accounts": {"main": main, "target": {"walls": {}}}, "active": "target",
                      "previous": "main", "rotations": [self.WALL_UNIX + 5]}, **changes)
        (self.pool / "state.json").write_text(json.dumps(state))

    def failure(self, events=None, now=None):
        from renewal_follow import far_wall_failure
        return far_wall_failure(self.root, self.events() if events is None else events, self.OLD,
                                self.processes, None if now is None else now * 1_000_000)

    def test_a_run_that_is_moving_or_has_moved_fails_nothing(self):
        # No refusal yet: nothing to judge, however late.
        self.assertIsNone(self.failure(self.events()[:2], now=500))
        # Refused, the host not asked yet, inside the time a turn has to end.
        from renewal_follow import FAR_YIELD_SECONDS
        self.assertIsNone(self.failure(self.events()[:5], now=10 + FAR_YIELD_SECONDS))
        self.moved()
        self.receipt()
        self.session(11, self.OLD)
        self.session(12, self.NEW)
        for now in (20, 500, None):
            self.assertIsNone(self.failure(now=now))

    def test_a_session_that_waits_in_its_turn_fails_by_name(self):
        from renewal_follow import FAR_YIELD_SECONDS
        waiting = self.events()[:6]
        # Past the time a turn has to end, with the host never asked.
        late = self.failure(waiting, now=10 + FAR_YIELD_SECONDS + 1)
        self.assertTrue(late.startswith("its session waited in its turn: the host was not asked to "
                                        f"move it within {FAR_YIELD_SECONDS} s of the refusal; "), late)
        self.assertIn("1 codex usage reads, 1 codex credit-list reads, 0 prepares traced", late)
        # A prepare traced before the refusal is not an answer to it; one after it is.
        self.calls(("prepare", "OK codex-rotation state=waiting", 9))
        self.assertIsNotNone(self.failure(waiting, now=200))
        self.calls(("prepare", "OK codex-rotation state=waiting", 12))
        self.assertIsNone(self.failure(waiting, now=200))
        # Once the run has ended there is no clock to judge by: the verdict
        # (far_wall_verdict) is what requires the prepare then.
        self.trace.unlink()
        self.assertIsNone(self.failure(waiting))
        # Asked, but refused: still asked. The host refuses a session not yet at rest.
        self.calls(("prepare", "ERR codex-renewal wall changed", 11),
                   ("prepare", "OK codex-rotation state=waiting", 41))
        self.assertIsNone(self.failure(waiting, now=200))
        self.assertIsNone(self.failure(waiting))
        # A turn that kept its request and sent it again fails at once, asked or not.
        self.moved()
        for status in (429, 200):
            again = self.events(extra=[("POST", "/v1/responses", 40, {"kind": "foreground", "status": status})])
            self.assertEqual(self.failure(again, now=41), "its session waited in its turn: the walled "
                             "login sent the turn's request again 30.0 s after the refusal")

    def test_a_session_moved_only_after_waiting_in_its_turn_fails_by_name(self):
        from renewal_follow import FAR_YIELD_SECONDS
        usage = "/api/codex/usage"
        walled = {"allowed": False, "sessionUsed": 100, "weekUsed": 10, "sessionResetsIn": 7100}
        # The turn kept its request through two polls, thirty seconds apart, and only
        # then asked the host, which moved it: first asked 89 s after the refusal.
        polled = self.events(drop=(22, 25), extra=[("GET", usage, 40, walled), ("GET", usage, 70, walled)])
        # While the run goes on: before the late asking, just after it, and at its verdict.
        self.assertIn("the host was not asked to move it within", self.failure(polled, now=98))
        self.calls(("prepare", "OK codex-rotation state=waiting", 99),
                   ("prepare", f"OK codex-rotation state=ready id={self.RECEIPT} thread=t turn=w", 110))
        named = ("its session waited in its turn: the host was first asked to move it 89.0 s after "
                 f"the refusal, not within {FAR_YIELD_SECONDS} s; after the first 429: ")
        for now in (99.9, 120, None):
            with self.subTest(now=now):
                self.assertTrue(self.failure(polled, now=now).startswith(named), self.failure(polled, now=now))
        self.assertIn("3 codex usage reads, 1 codex credit-list reads, 2 prepares traced", self.failure(polled))
        # The bound itself: asked exactly at it passes, a microsecond past it fails.
        import json
        for micros, fails in ((0, False), (1, True)):
            self.trace.write_text(json.dumps({"kind": "rotation", "verb": "prepare",
                "reply": "OK codex-rotation state=waiting",
                "monotonicUs": (10 + FAR_YIELD_SECONDS) * 1_000_000 + micros}) + "\n")
            self.assertEqual(self.failure(polled) is not None, fails)

    def test_a_move_to_the_same_account_fails_by_name(self):
        self.moved()
        named = f"receipt {self.RECEIPT} names the walled login as its destination"
        prefix = "it was moved to the account it is walled on: "
        for changes in ({"destination": {"account": "source-hash", "user": "u"}},
                        {"new_auth": self.OLD}, {"old_auth": self.NEW},
                        # The walled login by the run's own naming, whatever the receipt
                        # calls its source.
                        {"new_auth": self.OLD, "old_auth": "/elsewhere/auth.json"}):
            with self.subTest(changes=changes):
                self.receipt(**changes)
                self.assertEqual(self.failure(now=20), prefix + named)
        self.receipt()
        self.assertIsNone(self.failure(now=20))
        # A second process on the walled login is the same wrong end, receipt or none.
        self.session(11, self.OLD)
        self.session(12, self.OLD)
        self.assertEqual(self.failure(now=20), prefix + "2 session processes ran on the walled login")
        self.receipt(new_auth=self.OLD)
        self.assertEqual(self.failure(), prefix + named + "; 2 session processes ran on the walled login")

    def test_a_second_process_on_the_walled_login_is_counted_after_its_record_is_gone(self):
        # A session record does not outlive the handoff (q5f/r1: the moved session's was
        # gone 14.6 s after the refusal), so the count is the run's, not the directory's.
        self.moved()
        self.receipt()
        sessions = self.root / "codex-home" / "alab" / "sessions"
        self.session(11, self.OLD)
        # Seen before any refusal too: the set is filled on every call.
        self.assertIsNone(self.failure(self.events()[:2], now=5))
        self.assertEqual(self.processes, {"11"})
        (sessions / "11.json").unlink()
        self.session(12, self.NEW)
        self.assertIsNone(self.failure(now=20))
        # A restart on the walled login with no receipt, its predecessor's record gone.
        self.session(13, self.OLD)
        self.assertEqual(self.failure(now=21), "it was moved to the account it is walled on: "
                         "2 session processes ran on the walled login")
        (sessions / "13.json").unlink()
        self.assertEqual(self.failure(), "it was moved to the account it is walled on: "
                         "2 session processes ran on the walled login")
        self.assertEqual(self.processes, {"11", "13"})
        # A record that is not valid JSON, or names no login, is no process on it.
        self.processes.clear()
        (sessions / "14.json").write_text("{")
        (sessions / "15.json").write_text("[]")
        (sessions / "16.json").write_text('{"env": null}')
        self.assertIsNone(self.failure(now=22))
        self.assertEqual(self.processes, set())

    def test_a_host_that_declines_fails_by_name(self):
        self.calls(("prepare", "OK codex-rotation state=waiting", 13),
                   ("prepare", "OK codex-rotation state=not-needed", 18))
        self.assertEqual(self.failure(self.events()[:8], now=19),
                         "the host answered codex-rotation not-needed: it leaves no such wall")
        # A line still being written is passed over, not read as an answer.
        with self.trace.open("a") as trace:
            trace.write('{"kind": "rotation", "verb": "prep')
        from renewal_follow import rotation_calls
        self.assertEqual([verb for verb, _, _ in rotation_calls(self.trace)], ["prepare", "prepare"])
        self.assertEqual(rotation_calls(self.root / "absent.jsonl"), [])

    def test_the_hosts_policy_must_print_a_five_hour_bound_short_of_the_wall(self):
        from renewal_follow import FAR_RESET_SECONDS, five_hour_policy
        reply = ("OK gen=3 effective=on why=enabled contract=2,3 codex_rotation=1 "
                 "codex_rotation_five_hour=900 limit_wait=true accounts=early questions=ask")
        self.assertEqual(five_hour_policy(reply), {"codexRotation": True, "fiveHourBoundSeconds": 900,
                                                   "effective": "on", "limitWait": "true"})
        refused = {
            "prints no five-hour bound": reply.replace(" codex_rotation_five_hour=900", ""),
            "prints no five-hour bound ": reply.replace("codex_rotation=1 ", ""),
            "prints no five-hour bound  ": reply.replace("five_hour=900", "five_hour=0900"),
            "prints no five-hour bound   ": reply.replace("five_hour=900", "five_hour=15m"),
            "not short of the fixture wall's reset": reply.replace("=900", f"={FAR_RESET_SECONDS - 60}"),
            # An edge token's reply, and a refusal.
            "not answered to its owner": "OK effective=on",
            "not answered to its owner ": "ERR no such session",
        }
        for message, text in refused.items():
            with self.subTest(message=message):
                with self.assertRaisesRegex(RuntimeError, message.strip()):
                    five_hour_policy(text)
        # A refusal says what the host printed of rotation: an aterm without the word,
        # rotation switched off and a malformed bound read differently.
        said = {
            reply.replace(" codex_rotation_five_hour=900", ""):
                "; of rotation its reply says codex_rotation=1, and begins: OK gen=3 effective=on",
            reply.replace("codex_rotation=1 codex_rotation_five_hour=900 ", ""):
                "; of rotation its reply says nothing, and begins: OK gen=3 effective=on",
            reply.replace("codex_rotation=1", "codex_rotation=0"):
                "; of rotation its reply says codex_rotation=0 codex_rotation_five_hour=900, and begins: ",
            reply.replace("five_hour=900", "five_hour=15m"):
                "; of rotation its reply says codex_rotation=1 codex_rotation_five_hour=15m, and begins: ",
            "ERR no such session": "; of rotation its reply says nothing, and begins: ERR no such session",
        }
        for text, expected in said.items():
            with self.subTest(text=text):
                with self.assertRaises(RuntimeError) as caught:
                    five_hour_policy(text)
                self.assertIn(expected, str(caught.exception))
        # However long the reply, the refusal stays one bounded line.
        with self.assertRaises(RuntimeError) as caught:
            five_hour_policy("OK gen=3 " + "codex_rotation_x=" + "y" * 5000)
        self.assertLess(len(str(caught.exception)), 600)

    def test_the_trigger_is_the_five_hour_wall_by_the_providers_record(self):
        from renewal_follow import validate_far_wall_trigger
        self.assertEqual(validate_far_wall_trigger(self.events(), self.requests(), 900), {
            "wallUs": 10_000_000, "resetSecondsAtRefusal": 7200, "refusalNamedReset": True,
            "hostBoundSeconds": 900,
            "refusalStated": {"fiveHourUsedPercent": 100.0, "weekUsedPercent": 10.0},
            "walledLoginRequestsRefusedAfterWall": 1,
            "sourceWeekUsedPercentMax": 10, "walledUsageReadsByTurn": 1, "walledUsageReadsByHost": 1,
            "firstHostReadOfWallUs": 14_000_000})
        # A run whose session sent no title after its wall has nothing more to refuse.
        self.assertEqual(validate_far_wall_trigger(self.events(drop=(11,)), self.requests(), 900)[
            "walledLoginRequestsRefusedAfterWall"], 0)

    def test_the_refusal_names_its_reset_as_the_case_serves_it(self):
        """`five-hour-wall-far-unnamed` passes only on a refusal that named no reset, and
        `five-hour-wall-far` only on one that named it: each case proves its own 429."""
        from renewal_follow import validate_far_wall_trigger
        self.assertIs(validate_far_wall_trigger(self.events(), self.requests(named=False), 900,
            names_reset=False)["refusalNamedReset"], False)
        for named, served in ((True, False), (False, True)):
            with self.subTest(named=named):
                with self.assertRaisesRegex(RuntimeError, "which this case does not serve"):
                    validate_far_wall_trigger(self.events(), self.requests(named=served), 900,
                        names_reset=named)
        # A record with no word of it proves neither.
        bare = self.requests()
        del bare[1]["refusalNamedReset"]
        for named in (True, False):
            with self.assertRaisesRegex(RuntimeError, "which this case does not serve"):
                validate_far_wall_trigger(self.events(), bare, 900, names_reset=named)

    def test_each_other_explanation_of_the_move_is_rejected(self):
        from renewal_follow import validate_far_wall_trigger
        usage, responses = "/api/codex/usage", "/v1/responses"
        spent_week = {"allowed": False, "sessionUsed": 100, "weekUsed": 100, "sessionResetsIn": 7200}
        lifted = {"allowed": True, "sessionUsed": 0, "weekUsed": 10, "sessionResetsIn": 7200}
        near = {"allowed": False, "sessionUsed": 100, "weekUsed": 10, "sessionResetsIn": 600}
        closed = {"allowed": False, "sessionUsed": 0, "weekUsed": 100, "sessionResetsIn": 3600}
        cases = {
            # The turn's three requests.
            "not answered once on the source, refused once there": (self.events(extra=[
                ("POST", responses, 30, {"kind": "foreground", "status": 200})]), self.requests()),
            "not answered once on the source, refused once there ": ([
                dict(event, account="source") if event.get("status") == 200 else event
                for event in self.events()], self.requests()),
            "not answered once on the source, refused once there  ": (
                self.events(), self.requests() + [{"unixAt": 1}]),
            # A wall that refuses the turn and answers the same login's title is not one.
            "walled login was answered a model request after its refusal: thread-title 200": (
                self.events(drop=(11,), extra=[("POST", responses, 16, {"kind": "thread-title", "status": 200})]),
                self.requests()),
            "walled login was answered a model request after its refusal: thread-title 429, thread-title 200": (
                self.events(extra=[("POST", responses, 16, {"kind": "thread-title", "status": 200})]),
                self.requests()),
            # What the refusal stated.
            "did not state a spent five-hour window on a usable week": (
                self.events(), self.requests(**{"x-codex-secondary-used-percent": "100"})),
            "did not state a spent five-hour window on a usable week ": (
                self.events(), self.requests(**{"x-codex-primary-used-percent": "99"})),
            "did not state a spent five-hour window on a usable week  ": (
                self.events(), self.requests(**{"x-codex-primary-window-minutes": "10080"})),
            "did not state a spent five-hour window on a usable week   ": (
                self.events(), self.requests(**{"x-codex-active-limit": "other"})),
            r"reset \(600 s off\) is not beyond the host's bound \(900 s\)": (
                self.events(), self.requests(**{"x-codex-primary-reset-at": str(self.WALL_UNIX + 600)})),
            # 960 s is the bound and the skew exactly: not beyond them.
            r"reset \(960 s off\) is not beyond": (
                self.events(), self.requests(**{"x-codex-primary-reset-at": str(self.WALL_UNIX + 960)})),
            # What the readings stated.
            "stated its week spent": (self.events(extra=[("GET", usage, 16, spent_week)]), self.requests()),
            "stated its week spent ": (self.events(extra=[
                ("GET", usage, 3, dict(spent_week, allowed=True, sessionUsed=0))]), self.requests()),
            "after its refusal did not state the five-hour wall": (
                self.events(extra=[("GET", usage, 16, lifted)]), self.requests()),
            "after its refusal did not state the five-hour wall ": (
                self.events(extra=[("GET", usage, 16, near)]), self.requests()),
            "after its refusal did not state the five-hour wall  ": (
                self.events(drop=(12, 14)), self.requests()),
            # Figures of a wall under a verdict that permits: not a statement of it.
            "after its refusal did not state the five-hour wall   ": (self.events(extra=[
                ("GET", usage, 16, dict(near, allowed=True, sessionResetsIn=7200))]), self.requests()),
            # Refused, far from its reset, but with the five-hour window not spent.
            "after its refusal did not state the five-hour wall    ": (self.events(extra=[
                ("GET", usage, 16, dict(near, sessionUsed=99, sessionResetsIn=7200))]), self.requests()),
            "walled turn never read its own wall": (self.events(drop=(12,)), self.requests()),
            "host never read the walled login's usage": (self.events(drop=(14,)), self.requests()),
            "host never read the other account as usable": (self.events(drop=(15,)), self.requests()),
            # A destination the host read as closed, or one only the successor read.
            "host never read the other account as usable ": (self.events(drop=(15,), extra=[
                ("GET", usage, 15, dict(closed, account="target", client="host"))]), self.requests()),
        }
        for message, (events, requests) in cases.items():
            with self.subTest(message=message):
                with self.assertRaisesRegex(RuntimeError, message.strip()):
                    validate_far_wall_trigger(events, requests, 900)
        # The figures are judged where they were recorded: a reading with none is not one.
        bare = [{key: value for key, value in event.items() if key != "weekUsed"} for event in self.events()]
        with self.assertRaisesRegex(RuntimeError, "stated its week spent"):
            validate_far_wall_trigger(bare, self.requests(), 900)

    def test_the_hosts_own_ledger_must_hold_the_five_hour_wall_alone(self):
        from renewal_follow import host_wall_record
        self.ledger()
        self.assertEqual(host_wall_record(self.root, self.WALL_UNIX + 0.4, 900), {
            "hostWall": "five_hour", "hostWallStandsSeconds": 7200,
            "hostSample": {"fiveHourUsedPercent": 100, "weekUsedPercent": 10}})
        five_hour = {"at": self.WALL_UNIX, "until": self.WALL_UNIX + 7200}
        refused = {
            r"five-hour wall alone: \['five_hour', 'seven_day'\]": {"walls": {
                "five_hour": five_hour, "seven_day": {"until": self.WALL_UNIX + 86400}}},
            r"five-hour wall alone: \['seven_day'\]": {"walls": {"seven_day": {"until": self.WALL_UNIX + 86400}}},
            r"five-hour wall alone: \[\]": {"walls": {}},
            "does not hold the five-hour wall beyond its bound": {"walls": {
                "five_hour": {"until": self.WALL_UNIX + 900}}},
            "does not hold the five-hour wall beyond its bound ": {"walls": {"five_hour": {"until": None}}},
            "not a spent five-hour window on a usable week": {"sample": {"session_pct": 100, "week_pct": 100}},
            "not a spent five-hour window on a usable week ": {"sample": {"session_pct": 40, "week_pct": 10}},
            "not a spent five-hour window on a usable week  ": {"sample": {"session_pct": 100}},
            "did not move from the walled login to the other account": {"active": "main"},
            "did not move from the walled login to the other account ": {"previous": None},
        }
        for message, changes in refused.items():
            with self.subTest(message=message):
                self.ledger(**changes)
                with self.assertRaisesRegex(RuntimeError, message.strip()):
                    host_wall_record(self.root, self.WALL_UNIX, 900)

    def verdict(self, events=None, policy=True, logins=None, processes=None, receipt=None):
        from renewal_follow import far_wall_verdict
        policy = {"codexRotation": True, "fiveHourBoundSeconds": 900} if policy is True else policy
        receipt = {"id": self.RECEIPT, "old_pid": 11, "new_auth": self.NEW} if receipt is None else receipt
        return far_wall_verdict(self.root, self.events() if events is None else events, self.requests(),
                                receipt, policy, self.OLD, self.logins if logins is None else logins,
                                {"11"} if processes is None else processes)

    def whole(self):
        """What a run that moved leaves at its verdict: the moved session's record is gone
        and the loop saw one process on the walled login."""
        import hashlib
        from pathlib import Path
        self.moved()
        self.receipt()
        self.ledger()
        self.session(12, self.NEW)
        self.logins = {}
        for path in (Path(self.OLD), Path(self.NEW)):
            path.parent.mkdir()
            path.write_text(path.parent.name)
            self.logins[str(path)] = hashlib.sha256(path.parent.name.encode()).hexdigest()

    def test_the_verdict_records_both_sides_of_the_proof(self):
        self.whole()
        proof = self.verdict()
        self.assertEqual((proof["resetSecondsAtRefusal"], proof["hostBoundSeconds"], proof["hostWall"],
                          proof["hostSample"], proof["refusalStated"]),
                         (7200, 900, "five_hour", {"fiveHourUsedPercent": 100, "weekUsedPercent": 10},
                          {"fiveHourUsedPercent": 100.0, "weekUsedPercent": 10.0}))
        self.assertEqual((proof["firstHostReadOfWallUs"], proof["readyUs"]), (14_000_000, 18_000_000))
        self.assertEqual((proof["noSignInOrTokenRequest"], proof["loginFilesUnchanged"]), (True, True))
        # When the host was first asked, when it was ready, and what it refused between.
        self.assertEqual((proof["hostFirstAskedSecondsAfterRefusal"], proof["hostReadySecondsAfterRefusal"],
                          proof["preparesRefusedBeforeReady"], proof["walledLoginProcesses"]),
                         (3.0, 8.0, {"count": 0, "replies": []}, ["11"]))
        # A host that refused the session until its record was at rest is recorded so.
        self.calls(("prepare", "ERR codex-renewal wall changed", 11),
                   ("prepare", "ERR codex-renewal wall changed", 16),
                   ("prepare", "OK codex-rotation state=waiting", 41),
                   ("prepare", f"OK codex-rotation state=ready id={self.RECEIPT} thread=t turn=w", 46),
                   ("prepare", "ERR codex-renewal late", 50))
        proof = self.verdict()
        self.assertEqual((proof["hostFirstAskedSecondsAfterRefusal"], proof["hostReadySecondsAfterRefusal"],
                          proof["preparesRefusedBeforeReady"]),
                         (1.0, 36.0, {"count": 2, "replies": ["ERR codex-renewal wall changed"]}))

    def test_the_verdict_fails_each_wrong_outcome(self):
        from renewal_follow import FAR_NOT_MOVED, FAR_YIELD_SECONDS
        self.whole()
        posted = self.events(extra=[("POST", "/oauth/token", 20, {"kind": "non-responses", "account": None})])
        with self.assertRaisesRegex(RuntimeError, "a sign-in, token or other non-model request was "
                                    "posted: /oauth/token"):
            self.verdict(posted)
        with self.assertRaisesRegex(RuntimeError, "five-hour bound was never read: KeyError: 'socket'"):
            self.verdict(policy={"unread": "KeyError: 'socket'"})
        with self.assertRaisesRegex(RuntimeError, "five-hour bound was never read: None"):
            self.verdict(policy=None)
        # A reply that was kept but never judged is not a bound.
        with self.assertRaisesRegex(RuntimeError, "five-hour bound was never read: None"):
            self.verdict(policy={"reply": "OK gen=3 codex_rotation=1 codex_rotation_five_hour=900"})
        # The provider's and the host's records are both required.
        with self.assertRaisesRegex(RuntimeError, "host never read the walled login's usage"):
            self.verdict(self.events(drop=(14,)))
        self.ledger(walls={"seven_day": {"until": self.WALL_UNIX + 86400}})
        with self.assertRaisesRegex(RuntimeError, "five-hour wall alone"):
            self.verdict()
        self.ledger()
        # A login file that changed: a token written over it, or a sign-in.
        from pathlib import Path
        Path(self.OLD).write_text("auth-new")
        with self.assertRaisesRegex(RuntimeError, "a fixture login file changed during the run: auth-old"):
            self.verdict()
        Path(self.OLD).write_text("auth-old")
        # Both login files are held, by the receipt's naming of them: no map, half of one
        # and a map of other files each prove nothing of the two.
        elsewhere = self.root / "elsewhere" / "auth.json"
        elsewhere.parent.mkdir()
        elsewhere.write_text("auth-old")
        partial = {
            "not both held to the bytes written: $": {},
            "not both held to the bytes written: auth-old$": {self.OLD: self.logins[self.OLD]},
            "not both held to the bytes written: auth-new$": {self.NEW: self.logins[self.NEW]},
            "not both held to the bytes written: auth-old, elsewhere$": {
                self.OLD: self.logins[self.OLD], str(elsewhere): self.logins[self.OLD]},
        }
        for message, logins in partial.items():
            with self.subTest(message=message):
                with self.assertRaisesRegex(RuntimeError, "the two login files of this handoff were " + message):
                    self.verdict(logins=logins)
        with self.assertRaisesRegex(RuntimeError, "the two login files of this handoff were not both"):
            self.verdict(receipt={"id": self.RECEIPT, "old_pid": 11, "new_auth": self.OLD})
        # `ready` for this receipt must follow the host's reading of the wall.
        for rows in ([("prepare", f"OK codex-rotation state=ready id={self.RECEIPT} thread=t", 13)],
                     [("prepare", "OK codex-rotation state=ready id=" + "cd" * 16 + " thread=t", 18)],
                     [("commit", f"OK codex-rotation state=ready id={self.RECEIPT} thread=t", 18)]):
            self.calls(("prepare", "OK codex-rotation state=waiting", 12), *rows)
            with self.assertRaisesRegex(RuntimeError, "did not read the five-hour wall before it answered ready"):
                self.verdict()
        # A host that was never asked, by its own trace.
        self.calls(("commit", f"OK codex-rotation state=waiting id={self.RECEIPT} thread=t", 18))
        with self.assertRaisesRegex(RuntimeError, "the host was never asked to move the walled session"):
            self.verdict()
        self.moved()
        self.verdict()
        # The wrong ends the loop fails by name fail the verdict too, with the case's name.
        # A session that was moved in the end, but only after it waited in its turn.
        self.calls(("prepare", "OK codex-rotation state=waiting", 10 + FAR_YIELD_SECONDS + 1),
                   ("prepare", f"OK codex-rotation state=ready id={self.RECEIPT} thread=t turn=w", 40))
        with self.assertRaisesRegex(RuntimeError, "^" + FAR_NOT_MOVED + "its session waited in its turn: "
                                    f"the host was first asked to move it {FAR_YIELD_SECONDS + 1}.0 s after"):
            self.verdict()
        self.moved()
        # A second process the loop saw on the walled login, its record long gone.
        with self.assertRaisesRegex(RuntimeError, "^" + FAR_NOT_MOVED + "it was moved to the account it "
                                    "is walled on: 2 session processes"):
            self.verdict(processes={"11", "13"})
        # What the loop saw there must be the process the receipt moved, and something.
        for processes in (set(), {"13"}):
            with self.subTest(processes=processes):
                with self.assertRaisesRegex(RuntimeError, "seen on the walled login are not the one the "
                                            r"receipt moved \(11\): \[" + ("'13'" if processes else "") + r"\]"):
                    self.verdict(processes=processes)
        with self.assertRaisesRegex(RuntimeError, r"not the one the receipt moved \(None\): \['11'\]"):
            self.verdict(receipt={"id": self.RECEIPT, "new_auth": self.NEW})
        self.verdict()

    def test_no_run_of_a_case_with_its_own_oracle_passes_without_that_oracles_record(self):
        import renewal_follow
        from renewal_follow import CASE_PROOFS, require_case_proof
        self.assertEqual({case: record for case, record in CASE_PROOFS.items()
                          if not case.startswith("lost-login-")},
                         {"five-hour-wall-soon": "fiveHourWallWaitedOut",
                          "five-hour-wall-far": "fiveHourWallHandedOver",
                          "five-hour-wall-far-unnamed": "fiveHourWallHandedOver"})
        # Every lost-login case has an oracle of its own (LostLoginCases below).
        self.assertEqual({case for case in CASE_PROOFS if case.startswith("lost-login-")},
                         set(renewal_follow.LOST_LOGIN_CASES))
        self.assertLessEqual(set(CASE_PROOFS), set(renewal_follow.CASE_BUDGETS))
        # The weekly oracle's records alone do not pass the far case: its own must be there.
        weekly = {"nativeReceipt": {"id": self.RECEIPT}, "creditlessWallsYieldedToRotation": [{}]}
        for record in ({}, {"fiveHourWallHandedOver": None}, {"fiveHourWallHandedOver": {}},
                       {"fiveHourWallHandedOver": True}, {"fiveHourWallWaitedOut": {"wallUs": 1}}):
            with self.subTest(record=record):
                with self.assertRaisesRegex(RuntimeError, "the five-hour-wall-far oracle left no "
                                            "fiveHourWallHandedOver record"):
                    require_case_proof("five-hour-wall-far", dict(weekly, **record))
        require_case_proof("five-hour-wall-far", dict(weekly, fiveHourWallHandedOver={"wallUs": 1}))
        with self.assertRaisesRegex(RuntimeError, "the five-hour-wall-soon oracle left no fiveHourWallWaitedOut"):
            require_case_proof("five-hour-wall-soon", {"fiveHourWallHandedOver": {"wallUs": 1}})
        require_case_proof("five-hour-wall-soon", {"fiveHourWallWaitedOut": {"wallUs": 1}})
        # A case with no oracle of its own is not asked for one.
        for case in ("weekly-quota", "happy", "owner-stop"):
            require_case_proof(case, {})


class UsageWallCompletion(unittest.TestCase):
    """The usage-wall successor's report after its consume (completion_verdict), and when the
    loop may stop the host (completion_seen)."""

    RECEIPT = "ab" * 16
    THREAD = "01a1e1d7-ead3-7681-babc-69c2bb854b46"
    WALL = "01a1e1d7-eb09-7c40-8fa0-435cd49284a7"
    IDENTITY = " source_account=s source_user=u target_account=t target_user=v"

    def answer(self, state, receipt=None, thread=None, turn=None):
        return (f"OK codex-rotation state={state} id={receipt or self.RECEIPT} "
                f"thread={thread or self.THREAD} turn={turn or self.WALL}" + self.IDENTITY)

    def receipt(self):
        return {"id": self.RECEIPT, "intent": {"thread": self.THREAD, "wall": self.WALL}}

    def calls(self, *tail):
        """The traced calls of r/wq (2026-11-15) up to its consume, then `tail`."""
        return [("prepare", "OK codex-rotation state=waiting", 100),
                ("prepare", self.answer("ready"), 5_000_000),
                ("commit", self.answer("committed"), 8_000_000),
                ("consume", self.answer("consumed"), 14_000_000), *tail]

    @staticmethod
    def pool(**changes):
        state = dict({"active": "target", "proven": True, "canary": None, "rotations": [1]}, **changes)
        ledger = [{"account": "main", "kind": "wall", "sid": None, "why": "seven_day until 1"},
                  {"account": "main", "kind": "rotate", "sid": "host", "to": "target", "why": ""},
                  {"account": "target", "kind": "proven", "sid": None, "why": ""}]
        return state, ledger

    def test_a_report_answered_completed_after_the_consume_passes_and_ends_the_wait(self):
        from renewal_follow import completion_seen, completion_verdict
        calls = self.calls(("complete", self.answer("completed"), 14_070_000))
        self.assertEqual(completion_seen(calls, self.RECEIPT), 14_070_000)
        self.assertIsNone(completion_seen(calls, "cd" * 16))
        self.assertIsNone(completion_seen(self.calls(), self.RECEIPT))
        proof = completion_verdict(calls, self.receipt(), *self.pool())
        self.assertEqual(proof, {"completedUs": 14_070_000, "secondsAfterLastConsume": 0.07,
                                 "waitingBefore": 0, "consumes": 1, "provenAccount": "target"})
        # Without the pool's records the report alone is judged.
        self.assertNotIn("provenAccount", completion_verdict(calls, self.receipt()))

    def test_a_busy_host_answers_waiting_first_and_a_dropped_consume_is_asked_again(self):
        from renewal_follow import completion_verdict
        calls = self.calls(("consume", self.answer("consumed"), 15_000_000),
                           ("complete", "OK codex-rotation state=waiting", 15_100_000),
                           ("complete", "OK codex-rotation state=waiting", 15_350_000),
                           ("complete", self.answer("completed"), 15_600_000))
        proof = completion_verdict(calls, self.receipt(), *self.pool())
        self.assertEqual((proof["waitingBefore"], proof["consumes"], proof["secondsAfterLastConsume"]),
                         (2, 2, 0.6))

    def test_each_wrong_report_fails_by_name(self):
        from renewal_follow import completion_verdict
        done = ("complete", self.answer("completed"), 14_070_000)
        cases = {
            "no `codex-rotation complete` followed the successor's consume": self.calls(),
            "the successor never consumed its receipt": [call for call in self.calls(done)
                                                         if call[0] != "consume"],
            "answered the successor's report completed 0 times, not once: ERR codex-rotation "
            "selection changed": self.calls(("complete", "ERR codex-rotation selection changed", 14_070_000)),
            "answered the successor's report completed 2 times": self.calls(
                done, ("complete", self.answer("completed"), 14_200_000)),
            "names another receipt, thread or turn": self.calls(
                ("complete", self.answer("completed", receipt="cd" * 16), 14_070_000)),
            "names another receipt, thread or turn ": self.calls(
                ("complete", self.answer("completed", turn="other-turn"), 14_070_000)),
            "answered completed before the receipt's last consume": self.calls(
                done, ("consume", self.answer("consumed"), 14_100_000)),
            "answered otherwise than waiting": self.calls(
                ("complete", "ERR codex-rotation wrong process", 14_030_000), done),
            "sent before the consume": [("complete", "OK codex-rotation state=waiting", 13_000_000),
                                        *self.calls(done)],
        }
        for message, calls in cases.items():
            with self.subTest(message=message), self.assertRaisesRegex(RuntimeError, re.escape(message.strip())):
                completion_verdict(calls, self.receipt(), *self.pool())

    def test_a_pool_left_unproven_or_with_a_canary_fails_by_name(self):
        from renewal_follow import completion_verdict
        calls = self.calls(("complete", self.answer("completed"), 14_070_000))
        state, ledger = self.pool()
        taken = ledger + [{"account": "target", "kind": "rotate", "sid": "other", "to": "target",
                           "why": "canary: in place of host, which did not answer"}]
        cases = {
            "did not end proven on its active account": self.pool(proven=False),
            "did not end proven on its active account ": self.pool(canary={"sid": "host", "at": 1}),
            "a session took the canary: canary: in place of host": (state, taken),
            "the pool's active account was proven 0 times, not once": (state, ledger[:-1]),
            "the pool's active account was proven 2 times, not once": (state, ledger + ledger[-1:]),
        }
        for message, (state_, ledger_) in cases.items():
            with self.subTest(message=message), self.assertRaisesRegex(RuntimeError, re.escape(message.strip())):
                completion_verdict(calls, self.receipt(), state_, ledger_)
        with self.assertRaisesRegex(RuntimeError, "by its state and its ledger together"):
            completion_verdict(calls, self.receipt(), state, None)


class FiveHourWallHeld(unittest.TestCase):
    """What ends a `five-hour-wall-soon` run, what fails it at once, and its verdict."""

    def setUp(self):
        import tempfile
        from pathlib import Path
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.pool = self.root / "host-state" / "accounts-codex"

    @staticmethod
    def rest(**changes):
        return dict({"sessionId": "thread", "pid": 7, "status": "idle", "draftEmpty": True}, **changes)

    def test_the_case_ends_only_with_the_retry_answered_and_a_session_at_rest(self):
        from renewal_follow import wall_case_finished
        events = FiveHourWallSoon.events()
        self.assertTrue(wall_case_finished(events, [self.rest()]))
        unfinished = {
            "no session record": (events, []),
            "a session still working": (events, [self.rest(status="working")]),
            "a session with an unsent draft": (events, [self.rest(draftEmpty=False)]),
            "a session that owes a recovery": (events, [self.rest(
                owed={"thread": "thread", "rotation": {"receipt": None}})]),
            "the retry not sent yet": (events[:-1], [self.rest()]),
            "the retry refused": ([dict(event, status=429) if event is events[-1] else event
                                   for event in events], [self.rest()]),
            "only another login answered": ([dict(event, account="target")
                if event is events[-1] else event for event in events], [self.rest()]),
        }
        for name, (seen, observed) in unfinished.items():
            with self.subTest(name=name):
                self.assertFalse(wall_case_finished(seen, observed))

    def test_everything_a_handoff_leaves_is_named_and_fails_the_run(self):
        import json
        from renewal_follow import require_wall_held, wall_handoff_evidence
        events = FiveHourWallSoon.events()
        self.assertEqual(wall_handoff_evidence(self.root, events), [])
        require_wall_held(self.root, events)
        # A pool that has only been read is not a handoff.
        self.pool.mkdir(parents=True)
        (self.pool / "state.json").write_text(json.dumps({"accounts": {}, "rotations": []}))
        (self.pool / "renewal-follow").mkdir()
        (self.root / "native-actions.jsonl").write_text("")
        self.assertEqual(wall_handoff_evidence(self.root, events), [])

        def evidence(name):
            if name == "charged":
                (self.pool / "state.json").write_text(json.dumps({"rotations": [{"at": 1}]}))
            if name == "receipt":
                (self.pool / "renewal-follow" / ("ab" * 16 + ".json")).write_text("{}")
            if name == "action":
                (self.root / "native-actions.jsonl").write_text(
                    json.dumps({"action": "signal", "replyOk": True}) + "\n"
                    + json.dumps({"action": "resume", "replyOk": False}) + "\n")
            return wall_handoff_evidence(self.root, events)

        self.assertEqual(evidence("charged"), ["1 rotation(s) charged"])
        self.assertEqual(evidence("receipt"), ["1 rotation(s) charged", "1 handoff receipt file(s)"])
        self.assertEqual(evidence("action"), ["1 rotation(s) charged", "1 handoff receipt file(s)",
                                              "native action(s): resume, signal"])
        moved = FiveHourWallSoon.events(extra=[
            ("POST", "/v1/responses", 40, {"kind": "thread-title", "status": 200, "account": "target"})])
        self.assertEqual(wall_handoff_evidence(self.root, moved)[-1], "model request(s) from target")
        with self.assertRaisesRegex(RuntimeError, "resets soon was handed over, charged or replayed: "
                                    "1 rotation\\(s\\) charged; 1 handoff receipt"):
            require_wall_held(self.root, events)

    def test_the_verdict_needs_the_wait_no_handoff_and_exactly_three_requests(self):
        import json
        from renewal_follow import wall_verdict
        events = FiveHourWallSoon.events()
        trace = self.root / "renewal-control-trace.jsonl"
        waited = {"wallUs": 10_000_000, "retryUs": 31_000_000, "waitedSeconds": 21.0,
                  "walledUsageReads": 1, "usageReads": 2}
        # No control trace: how often the host was asked is unknown, and said so.
        self.assertEqual(wall_verdict(self.root, events, [1, 2, 3], trace), dict(waited, preparesTraced=None))
        trace.write_text(json.dumps({"kind": "rotation", "verb": "prepare", "monotonicUs": 12}) + "\n"
                         + json.dumps({"kind": "session-status"}) + "\n")
        self.assertEqual(wall_verdict(self.root, events, [1, 2, 3], trace), dict(waited, preparesTraced=1))
        for count in (2, 4):
            with self.subTest(count=count):
                with self.assertRaisesRegex(RuntimeError, f"{count} foreground model requests, not 3"):
                    wall_verdict(self.root, events, list(range(count)), trace)
        with self.assertRaisesRegex(RuntimeError, "retried before its wall lifted"):
            wall_verdict(self.root, FiveHourWallSoon.events(retry_at=29), [1, 2, 3], trace)
        (self.root / "native-actions.jsonl").write_text(json.dumps({"action": "signal"}) + "\n")
        with self.assertRaisesRegex(RuntimeError, "handed over, charged or replayed: native action"):
            wall_verdict(self.root, events, [1, 2, 3], trace)


class PreSignalStop(unittest.TestCase):
    def setUp(self):
        self.operation = "ab" * 16
        self.initial = {"intent": {"thread": "thread", "wall": "wall"}, "human_seq": 3,
            "native_prepare": "deadline-us=120000001", "expires_ms": 120001}
        self.state = {"id": self.operation, "intent": self.initial["intent"],
            "phase": "retired", "revoked": True, "launch_attempted": True,
            "native_prepare": self.initial["native_prepare"], "expires_ms": 120001}
        self.cancel = {"root": self.operation, "intent": self.initial["intent"],
            "reason": "owner_stop", "human_seq": 4}
        self.worker = {"operation": self.operation, "deadlineUs": 120000001, "monotonicUs": 100}
        self.observation = {"operation": self.operation, "observationTriggeredReconciliation": True,
            "state": "completed", "decision": "rollback-safe", "deadlineUs": 120000001,
            "deadlineExpired": False, "monotonicUs": 101, "signalSent": False, "signalAccepted": None,
            "resumeSent": False, "inputCustodySettled": True, "inputAttemptSettled": True,
            "goGrantedChildren": 0, "retainedChildren": 0}

    def validate(self):
        validate_presignal_stop(self.operation, self.initial, self.state,
            self.cancel, self.worker, self.observation)

    def test_durable_effect_free_closure_does_not_require_a_signal_rpc(self):
        self.validate()

    def test_cancellation_of_another_intent_or_old_person_sequence_is_rejected(self):
        for key, value in (("root", "cd" * 16), ("intent", {"wall": "other"}),
                           ("reason", "new_task"), ("human_seq", 3)):
            with self.subTest(key=key):
                old = copy.deepcopy(self.cancel)
                self.cancel[key] = value
                with self.assertRaises(RuntimeError):
                    self.validate()
                self.cancel = old

    def test_missing_terminal_proof_mutation_or_unsettled_input_is_rejected(self):
        for key, value in (("state", "active"), ("decision", "none"), ("operation", "cd" * 16),
                           ("monotonicUs", 99), ("monotonicUs", 120000001),
                           ("signalSent", True), ("signalAccepted", True), ("signalAccepted", 0), ("resumeSent", True),
                           ("inputCustodySettled", False), ("inputAttemptSettled", False),
                           ("goGrantedChildren", 1), ("retainedChildren", 1)):
            with self.subTest(key=key):
                old = self.observation.copy()
                self.observation[key] = value
                with self.assertRaises(RuntimeError):
                    self.validate()
                self.observation = old

    def test_expired_or_changed_budget_and_revived_attempt_are_rejected(self):
        self.worker["monotonicUs"] = self.worker["deadlineUs"]
        with self.assertRaises(RuntimeError):
            self.validate()
        self.worker["monotonicUs"] = 100
        self.worker["deadlineUs"] += 1
        with self.assertRaises(RuntimeError):
            self.validate()
        self.worker["deadlineUs"] -= 1
        self.state["native_prepare"] += " changed=1"
        with self.assertRaises(RuntimeError):
            self.validate()
        self.state["native_prepare"] = self.initial["native_prepare"]
        self.state["expires_ms"] += 1
        with self.assertRaises(RuntimeError):
            self.validate()
        self.state["expires_ms"] -= 1
        self.state["current_attempt"] = {"id": "child"}
        with self.assertRaises(RuntimeError):
            self.validate()


class RollbackSignals(unittest.TestCase):
    def test_two_signals_require_distinct_exact_live_predecessors_in_order(self):
        import json
        from pathlib import Path
        import tempfile
        from renewal_follow import rollback_signal_effects
        report = {"originalPid": 11, "originalProcess": {"kernelBirth": 21}}
        rollback = {"pid": 12, "kernelBirth": 22}
        effects = [{"schema": 1, "kind": "native-signal-effect", "pid": pid,
            "birth": birth, "group": pid, "signal": 15, "monotonicUs": mono}
            for pid, birth, mono in [(11, 21, 1), (12, 22, 2)]]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "native-signal-effects.jsonl"
            def write(value):
                path.write_text("".join(json.dumps(event) + "\n" for event in value))
            write(effects)
            self.assertEqual(rollback_signal_effects(root, report, rollback), effects)
            for changed in (effects[:1], effects + effects[:1], list(reversed(effects)),
                            [effects[0], dict(effects[1], pid=13)],
                            [effects[0], dict(effects[1], birth=23)],
                            [effects[0], dict(effects[1], signal=9)],
                            [effects[0], dict(effects[1], monotonicUs=1)]):
                write(changed)
                with self.assertRaises(RuntimeError):
                    rollback_signal_effects(root, report, rollback)


class RollbackCompletion(unittest.TestCase):
    """A Retired receipt can precede the ledger's native completion: the vendor's own
    status retires it from live evidence (r3 of weekly-quota-rollback, 2026-11-13)."""

    def setUp(self):
        import hashlib
        from pathlib import Path
        self.prepare = "prepare id=" + "ab" * 16 + " deadline-us=120000001"
        self.old, self.new = Path("/owned/auth-old/auth.json"), Path("/owned/auth-new/auth.json")
        self.codex = Path("/owned/package/bin/codex")
        self.boundary = {"operation": "ab" * 16, "originalExpiresMs": 120001,
            "originalPrepareSha256": hashlib.sha256(self.prepare.encode()).hexdigest()}
        self.process = {"pid": 42, "kernelBirth": 52, "exe": str(self.codex)}
        self.original = {"id": "ab" * 16, "phase": "retired", "revoked": False,
            "launch_attempted": True, "successor_pid": 42, "successor_birth": 52,
            "accepted_turn": None, "candidate": {"pid": 42, "forward": False},
            "old_auth": str(self.old), "new_auth": str(self.new), "current_attempt": None,
            "expires_ms": 120001, "native_prepare": self.prepare}
        self.claimed = {"kind": "session-status", "holder": "codex-alab@42", "proven": True}
        # The old operation while its rollback runs, and once the Completed reply is saved.
        self.running = {"phase": "exiting", "why": "", "native_prepare": self.prepare,
            "native_id": "cd" * 16, "native_rollback": True, "native_coordination": True}
        self.completed = {"phase": "relaunched", "why": "", "native_prepare": "",
            "native_rollback": True, "native_coordination": True,
            "native_completed_prepare": self.prepare, "native_observation": "observation"}

    def proven(self, ledger, original=None, claimed=True):
        from renewal_follow import rollback_proven
        return rollback_proven(self.boundary, self.original if original is None else original,
            self.claimed if claimed else None, ledger, self.process, self.old, self.new, self.codex)

    def test_a_retired_receipt_without_ledger_completion_keeps_waiting(self):
        self.assertFalse(self.proven(self.running))
        # Not even a retained stop, or the flag a staged account candidate shares, ends it.
        self.assertFalse(self.proven(dict(self.running, phase="failed", why="transient")))
        self.assertFalse(self.proven(dict(self.running, native_retired_unissued=True,
            native_account_candidate={"version": 1})))
        # The same holds for a completed ledger while the receipt or the claim is still due.
        self.assertFalse(self.proven(self.completed, dict(self.original, phase="committing")))
        self.assertFalse(self.proven(self.completed, claimed=False))

    def test_durable_completion_with_the_original_prepare_proves_the_rollback(self):
        self.assertTrue(self.proven(self.completed))
        for key, value in (("native_completed_prepare", self.prepare + " other=1"),
                           ("native_rollback", None)):
            with self.subTest(key=key):
                with self.assertRaisesRegex(RuntimeError, "rollback lacks exact old operation"):
                    self.proven(dict(self.completed, **{key: value}))
        # The completed prepare is the proof: a still-retained one no longer counts.
        with self.assertRaisesRegex(RuntimeError, "rollback lacks exact old operation"):
            self.proven(dict(self.completed, native_completed_prepare="prepare id=" + "ef" * 16,
                             native_prepare=self.prepare))
        with self.assertRaisesRegex(RuntimeError, "completion carries no observation"):
            self.proven(dict(self.completed, native_observation=None))

    def test_a_terminal_non_completion_fails_fast_with_what_ended_it(self):
        released = dict(self.running, native_prepare="")
        del released["native_id"]
        ended = {
            "retired with no exec granted": dict(self.running, native_retired_unissued=True),
            "proven never executed": dict(released, native_noexec_prepare=self.prepare),
            "stopped: native-cohort-retired": dict(released, phase="failed", why="native-cohort-retired"),
            "released its prepare without completing it": released,
        }
        for message, ledger in ended.items():
            # Whatever the receipt says: the operation can never complete.
            for original in (self.original, dict(self.original, phase="committing")):
                with self.subTest(message=message, phase=original["phase"]):
                    with self.assertRaisesRegex(RuntimeError,
                            "^rollback native operation ended without completion: .*" + message):
                        self.proven(ledger, original)


class ImageDigest(unittest.TestCase):
    def test_an_unchanged_image_is_read_once_and_a_rewritten_one_again(self):
        import hashlib
        import os
        from pathlib import Path
        import tempfile
        from unittest import mock
        import renewal_follow
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / "codex"
            image.write_bytes(b"first image")
            with mock.patch.dict(renewal_follow.IMAGE_DIGESTS, clear=True), \
                    mock.patch.object(os, "read", wraps=os.read) as read:
                for _ in range(3):
                    stat, digest = renewal_follow.image_digest(image)
                    self.assertEqual(digest, hashlib.sha256(b"first image").hexdigest())
                    self.assertEqual((stat.st_dev, stat.st_ino), (image.stat().st_dev, image.stat().st_ino))
                self.assertEqual(read.call_count, 2)
                # Same size, mtime put back: the ctime still names the new contents.
                before = image.stat()
                image.write_bytes(b"other image")
                os.utime(image, ns=(before.st_atime_ns, before.st_mtime_ns))
                self.assertEqual(renewal_follow.image_digest(image)[1],
                                 hashlib.sha256(b"other image").hexdigest())
                self.assertEqual(read.call_count, 4)


class HostExitDiagnostics(unittest.TestCase):
    """What a host that outlived its exit wait was doing, written before terminate()."""

    def setUp(self):
        import json
        from pathlib import Path
        import subprocess
        import sys
        import tempfile
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.host = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        self.addCleanup(self.host.wait)
        self.addCleanup(self.host.kill)
        gone = subprocess.Popen(["/bin/sh", "-c", "exit 0"])
        gone.wait()
        self.gone = gone.pid
        sessions = self.root / "codex-home" / "alab" / "sessions"
        sessions.mkdir(parents=True)
        # A live codex keeps its PID field; one that exited keeps only its record's name.
        (sessions / f"{self.host.pid}.json").write_text(json.dumps({"pid": self.host.pid}))
        (sessions / f"{self.gone}.json").write_text(json.dumps({"exit": "signal:1"}))
        (sessions / "stand-in.json").write_text("{}")
        self.sampler = self.root / "sampler"
        self.sampler.write_text('#!/bin/sh\nprintf "threads of %s\\n" "$1" > "$5"\necho sampled\n')
        self.sampler.chmod(0o755)

    def diagnose(self, budget=30, sampler=None):
        from unittest import mock
        import renewal_follow
        # A run writes its diagnostics once; each call here is another run's.
        (self.root / "host-exit-diagnostics.json").unlink(missing_ok=True)
        with mock.patch.object(renewal_follow, "SAMPLER", sampler or self.sampler):
            return renewal_follow.host_exit_diagnostics(self.root, self.host, budget)

    def test_it_names_who_still_lives_and_samples_the_host_into_the_run(self):
        import json
        diagnostics = self.diagnose()
        self.assertEqual(json.loads((self.root / "host-exit-diagnostics.json").read_text()), diagnostics)
        self.assertEqual((diagnostics["hostPid"], diagnostics["hostAlive"]), (self.host.pid, True))
        self.assertEqual({entry["pid"]: (entry["alive"], entry["recordPid"], entry["exit"])
                          for entry in diagnostics["codex"]},
                         {self.host.pid: (True, self.host.pid, None), self.gone: (False, None, "signal:1")})
        sample = diagnostics["sample"]
        self.assertEqual((sample["returncode"], sample["output"], sample["file"]),
                         (0, "sampled\n", "host-sample.txt"))
        self.assertEqual((self.root / "host-sample.txt").read_text(), f"threads of {self.host.pid}\n")

    def test_it_samples_nothing_it_cannot_or_has_no_time_for(self):
        absent = self.diagnose(sampler=self.root / "no-sampler")["sample"]
        self.assertEqual(absent["skipped"], "sampler absent")
        hurried = self.diagnose(budget=2.5)["sample"]
        self.assertEqual(hurried["skipped"], "2.5 s of guardian budget left")
        self.host.kill()
        self.host.wait()
        exited = self.diagnose()
        self.assertEqual((exited["hostAlive"], exited["sample"]["skipped"]), (False, "host exited"))
        self.assertFalse((self.root / "host-sample.txt").exists())


class PrefixCrashClosure(unittest.TestCase):
    def setUp(self):
        import hashlib
        self.boundary = {"operation": "ab" * 16, "prefix": {"pid": 41, "birth": 51,
            "parent": 31, "group": 41, "session": 31, "forward": True},
            "originalDeadlineUs": 120000001, "originalExpiresMs": 120001,
            "originalPrepareSha256": hashlib.sha256(b"immutable-plan").hexdigest()}
        self.state = {"id": self.boundary["operation"], "phase": "committing", "revoked": False,
            "candidate": self.boundary["prefix"].copy(), "native_prepare": "immutable-plan", "expires_ms": 120001}
        self.closed = {"operation": self.boundary["operation"], "observationTriggeredReconciliation": True,
            "state": "completed", "decision": "retired-cohort-exited", "deadlineExpired": True,
            "deadlineUs": 120000001, "monotonicUs": 121000001, "signalAccepted": True, "resumeSent": True,
            "sourceGroupGone": True, "retainedSourceWatch": True, "sourceWatchExited": True,
            "closedRestartWitness": True, "closedRestartQuiescent": True, "closedRestartAvailable": True,
            "inputAttemptSettled": True, "inputCustodySettled": True, "sourceBirthAlive": False,
            "goGrantedChildren": 0, "retainedChildren": 0}

    def validate(self):
        from renewal_follow import validate_prefix_crash_closure
        validate_prefix_crash_closure(self.boundary, self.state, self.closed)

    def test_go_granted_prefix_requires_positive_cohort_death_and_drain(self):
        self.validate()
        for key, value in (("decision", "retired-no-exec-grant"), ("sourceBirthAlive", None),
                           ("sourceWatchExited", False), ("closedRestartQuiescent", False),
                           ("inputCustodySettled", False), ("inputAttemptSettled", False),
                           ("goGrantedChildren", 1), ("monotonicUs", 120000000)):
            with self.subTest(key=key):
                old = self.closed.copy()
                self.closed[key] = value
                with self.assertRaises(RuntimeError):
                    self.validate()
                self.closed = old

    def test_prefix_identity_plan_and_logical_intent_cannot_change(self):
        for key, value in (("candidate", dict(self.boundary["prefix"], birth=52)),
                           ("candidate", dict(self.boundary["prefix"], pid=42)),
                           ("current_attempt", {"id": "child"}), ("native_prepare", "different"),
                           ("expires_ms", 120002), ("revoked", True), ("successor_pid", 42)):
            with self.subTest(key=key):
                old = self.state.copy()
                self.state[key] = value
                with self.assertRaises(RuntimeError):
                    self.validate()
                self.state = old


class LauncherFaultBoundary(unittest.TestCase):
    def test_fault_and_checkpoint_follow_the_inherited_launcher_home(self):
        from pathlib import Path
        import tempfile
        from renewal_follow import launcher_fixture_root
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            host_faults = root / "host-faults"
            home = root / "inherited-home"
            launcher = launcher_fixture_root({"HOME": str(home)})
            launcher.mkdir(parents=True)
            host_faults.mkdir()
            pause = launcher / "forward-after-go.pause"
            pause.touch()
            # Match the actual native_host callback: HOME/.native-renewal-fixture.
            callback_root = home / ".native-renewal-fixture"
            self.assertTrue((callback_root / pause.name).exists())
            self.assertFalse((host_faults / pause.name).exists())
            (callback_root / "forward-after-go.reached").write_text("41")
            self.assertEqual(int((launcher / "forward-after-go.reached").read_text()), 41)


class RollbackCallbackScope(unittest.TestCase):
    def test_actual_record_callback_keeps_outer_auth_path_across_other_case_assignments(self):
        import ast
        import copy
        from pathlib import Path
        source = ast.parse(Path(__file__).with_name("renewal_follow.py").read_text())
        drive = next(node for node in ast.walk(source)
            if isinstance(node, ast.FunctionDef) and node.name == "drive")
        assignment = next(node for node in ast.walk(drive) if isinstance(node, ast.Assign)
            and any(isinstance(target, ast.Name) and target.id == "rollback_record" for target in node.targets))
        def compile_callback(reintroduce_shadow):
            # Keep every real drive binding in a nonexecuted branch. Python still
            # applies their lexical scope to the actual generator callback below.
            bindings = copy.deepcopy(drive.body)
            if reintroduce_shadow:
                bindings.append(ast.Assign(targets=[ast.Name(id="old", ctx=ast.Store())], value=ast.Constant(None)))
            callback = ast.FunctionDef(name="callback",
                args=ast.arguments(posonlyargs=[], args=[ast.arg(arg=name) for name in
                    ("observed", "previous", "report")], kwonlyargs=[], kw_defaults=[], defaults=[]),
                body=[ast.If(test=ast.Constant(False), body=bindings, orelse=[]),
                    ast.Return(value=copy.deepcopy(assignment.value))], decorator_list=[])
            module = ast.fix_missing_locations(ast.Module(body=[callback], type_ignores=[]))
            namespace = {"old": Path("/owned/auth-old/auth.json")}
            exec(compile(module, "<actual-drive-record-callback>", "exec"), namespace)
            return namespace["callback"]
        record = {"pid": 2, "sessionId": "thread", "env": {"CODEX_AUTH_PATH": "/owned/auth-old/auth.json"}}
        arguments = ([record], {"pid": 1}, {"thread": "thread"})
        self.assertIs(compile_callback(False)(*arguments), record)
        with self.assertRaises(NameError):
            compile_callback(True)(*arguments)


class CaseObservationBudgets(unittest.TestCase):
    def test_real_claim_grace_rounding_fits_before_cleanup(self):
        import math
        from renewal_follow import CASE_BUDGETS
        # Source: Supervisor.ready_for waits human_grace after EVERY new claim;
        # renewal_follow schedules a 60s RECHECK for readiness Wait. g5 reached
        # initial Ready~181s, fresh Ready~421s and a successor claim~424s.
        policy_poll = 60
        rounded_grace = math.ceil(121 / policy_poll) * policy_poll
        required = rounded_grace + rounded_grace + policy_poll + rounded_grace + 120
        drive, guardian = CASE_BUDGETS["grace-beyond-budget"]
        self.assertGreaterEqual(drive, required)
        self.assertGreaterEqual(guardian - drive, 30)
        # Ordinary source admission can require two policy checks, then cold
        # successor observation another two; preserve bounded cleanup headroom.
        for case in ("happy", "lost-signal-ack", "panic-before-signal", "lost-consume-ack",
                     "late-hold", "owner-stop", "owner-stop-after-signal"):
            drive, guardian = CASE_BUDGETS[case]
            self.assertGreaterEqual(drive, 2 * policy_poll + 2 * policy_poll + 30)
            self.assertGreaterEqual(guardian - drive, 30)

    def test_fault_and_postsignal_cases_keep_their_independent_bounds(self):
        from renewal_follow import CASE_BUDGETS
        for case in ("failed-forward-rollback", "successor-prefix-crash"):
            self.assertEqual(CASE_BUDGETS[case], (450, 480))
        for case in ("post-signal-resume", "post-signal-expiry-diagnostic"):
            self.assertEqual(CASE_BUDGETS[case], (390, 420))


class LostLoginCases(unittest.TestCase):
    """How the lost-login cases are declared, and what their fixture provider answers."""

    def setUp(self):
        import tempfile
        from pathlib import Path
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))

    @staticmethod
    def turn():
        return {"input": [], "tools": [{"type": "function", "name": "update_plan"}]}

    @staticmethod
    def title():
        from renewal_follow import TITLE_FORMAT, TITLE_INSTRUCTIONS
        return {"tools": [], "text": {"format": TITLE_FORMAT}, "input": [{"role": "user", "content": [
            {"type": "input_text", "text": TITLE_INSTRUCTIONS + " for this task."}]}]}

    def test_the_cases_are_declared_with_budgets_and_oracles_and_are_not_in_the_matrix(self):
        import renewal_follow
        import renewal_matrix
        from renewal_follow import (CASE_BUDGETS, CASE_PROOFS, LOST_LOGIN_CASES, LOST_LOGIN_MOVED,
                                    PLAIN_ISSUER, REFRESH_REFUSALS, TLS_ISSUER, VETO_WORDS)
        # The issuer is the fixture's TLS listener, as in every other case of the file,
        # in every case but the one that is named for serving it elsewhere.
        self.assertEqual(LOST_LOGIN_CASES, {
            "lost-login-expired": ("refresh_token_expired", True, TLS_ISSUER),
            "lost-login-expired-plain-issuer": ("refresh_token_expired", True, PLAIN_ISSUER),
            "lost-login-invalidated": ("refresh_token_invalidated", True, TLS_ISSUER),
            "lost-login-reused": ("refresh_token_reused", True, TLS_ISSUER),
            "lost-login-invalid-grant": ("invalid_grant", True, TLS_ISSUER),
            "lost-login-unrecognised": ("account_deactivated", True, TLS_ISSUER),
            "lost-login-no-word": ("refresh_token_expired", False, TLS_ISSUER)})
        self.assertEqual([case for case, (_, _, issuer) in LOST_LOGIN_CASES.items() if issuer == PLAIN_ISSUER],
                         ["lost-login-expired-plain-issuer"])
        # A move over the other issuer leaves its own record: nothing that looks for the
        # specified case's is satisfied by it.
        self.assertEqual({case: CASE_PROOFS[case] for case in LOST_LOGIN_MOVED},
                         {"lost-login-expired": "lostLoginMoved",
                          "lost-login-expired-plain-issuer": "lostLoginMovedOverPlainIssuer"})
        for case, (word, host_moves, _) in LOST_LOGIN_CASES.items():
            with self.subTest(case=case):
                moved = case in LOST_LOGIN_MOVED
                # Only the refusal of a token that simply ended is moved, and only by a
                # host that prints the word for it.
                self.assertEqual(moved, word == "refresh_token_expired" and host_moves)
                if not moved:
                    self.assertEqual(CASE_PROOFS[case], "lostLoginStayed")
                self.assertEqual(CASE_BUDGETS[case], (270, 300) if moved else (180, 210))
                self.assertIn(word, REFRESH_REFUSALS)
                # codex-alab keeps its own refusal for every word but the one it moves for.
                self.assertEqual(VETO_WORDS[word] is None, word == "refresh_token_expired")
                # None has passed with a qualified package: the runs of 2026-11-13 used a
                # trial package, which the matrix refuses (PARALLEL-LOGINS.md), so the
                # matrix does not hold them.
                self.assertNotIn(case, renewal_matrix.CASES)
        self.assertEqual(set(VETO_WORDS), set(REFRESH_REFUSALS))
        self.assertEqual({word: VETO_WORDS[word] for word in ("refresh_token_invalidated",
            "refresh_token_reused", "invalid_grant", "account_deactivated")},
            {"refresh_token_invalidated": "revoked", "refresh_token_reused": "exhausted",
             "invalid_grant": "other", "account_deactivated": "other"})
        # The host settles its own refusal for a minute before it moves anything, and a
        # wall that stays is watched past codex-alab's next look at it a minute on: both
        # fit their case's budget with room for the successor and for cleanup.
        # A moved session is then watched as long as a wall that stays.
        self.assertEqual(renewal_follow.LOGIN_SETTLE_SECONDS, 60)
        self.assertEqual(renewal_follow.LOGIN_MOVED_WATCH_SECONDS, renewal_follow.LOGIN_WALL_HOLD_SECONDS)
        self.assertLess(renewal_follow.LOGIN_SETTLE_SECONDS + renewal_follow.LOGIN_ASK_SECONDS
                        + renewal_follow.LOGIN_MOVED_WATCH_SECONDS + 60, 270)
        self.assertGreater(renewal_follow.LOGIN_WALL_HOLD_SECONDS, 60)
        self.assertLess(renewal_follow.LOGIN_WALL_HOLD_SECONDS + 60, 180)
        # One ask a pass, a pass every five seconds, with half a second for the trace's clock.
        self.assertEqual((renewal_follow.LOGIN_ASK_PERIOD_SECONDS, renewal_follow.LOGIN_ASK_EARLY_SECONDS),
                         (5, 0.5))

    def test_a_lost_login_is_no_usage_wall_and_names_a_known_refusal(self):
        from renewal_follow import Model
        with self.assertRaisesRegex(RuntimeError, "unknown refresh refusal"):
            Model(self.root, lost_login="refresh_token_revoked")
        for options in ({"weekly_quota": True}, {"all_exhausted": True}, {"chain": True}):
            with self.assertRaisesRegex(RuntimeError, "a lost login is no usage wall"):
                Model(self.root, lost_login="refresh_token_expired", **options)

    def test_the_source_is_answered_once_then_refused_every_request_its_title_too(self):
        import json
        from renewal_follow import Model
        model = Model(self.root, lost_login="refresh_token_expired")
        self.assertFalse(model.signed_out("old"))
        status, _, _, _, record = model.reply(self.turn(), "old")
        self.assertEqual((status, record["responseHeaders"]["x-codex-primary-used-percent"]), (200, "0"))
        # A title before the loss is answered, and loses nothing.
        self.assertEqual(model.reply(self.title(), "old")[0], 200)
        self.assertIsNone(model.login_lost)
        for body in (self.turn(), self.title(), self.turn()):
            status, data, content_type, _, record = model.reply(body, "old")
            self.assertEqual((status, content_type), (401, "application/json"))
            # A login wall, not a usage wall: no window is stated and none is spent.
            self.assertNotIn("responseHeaders", record)
            self.assertEqual(json.loads(data)["error"]["code"], "invalid_api_key")
        self.assertTrue(model.signed_out("old"))
        # A bearer the fixture does not know is the source's, as in every other case.
        self.assertEqual(model.reply(self.title(), "")[0], 401)
        # The loss is dated once, by its first refusal.
        first = model.login_lost
        self.assertEqual(model.reply(self.turn(), "old")[0], 401)
        self.assertIs(model.login_lost, first)
        # The other login is answered throughout, with nothing opened for it.
        self.assertFalse(model.signed_out("new"))
        for body in (self.turn(), self.title()):
            status, _, _, _, record = model.reply(body, "new")
            self.assertEqual((status, record["responseHeaders"]["x-codex-secondary-used-percent"]), (200, "0"))
        self.assertFalse((self.root / "renewed").exists())

    def test_no_window_of_either_login_is_ever_spent_and_no_reading_is_held(self):
        import time
        from renewal_follow import Model
        model = Model(self.root, lost_login="refresh_token_reused")
        for _ in range(2):
            model.reply(self.turn(), "old")
        started = time.monotonic()
        source, target = model.usage(True, False), model.usage(False, True)
        # The destination's reading is not held for a `renewed` file, as a quota case's is.
        self.assertLess(time.monotonic() - started, 5)
        for usage, account in ((source, "source-account"), (target, "target-account")):
            self.assertTrue(usage["rate_limit"]["allowed"])
            self.assertEqual((usage["rate_limit"]["primary_window"]["used_percent"],
                              usage["rate_limit"]["secondary_window"]["used_percent"]), (0, 0))
            self.assertEqual(usage["account_id"], account)
        self.assertEqual(model.usage_reads, ["source", "target"])
        with self.assertRaisesRegex(RuntimeError, "unexpected usage identity"):
            model.usage(False, False)

    def test_each_refusal_is_the_providers_own_status_and_body_for_the_source_alone(self):
        import json
        from renewal_follow import Model, REFRESH_REFUSALS
        expected = {
            "refresh_token_expired": (401, {"error": {"code": "refresh_token_expired"}}),
            "refresh_token_invalidated": (401, {"error": {"code": "refresh_token_invalidated"}}),
            "refresh_token_reused": (401, {"error": {"code": "refresh_token_reused"}}),
            "invalid_grant": (400, {"error": "invalid_grant"}),
            "account_deactivated": (401, {"error": {"code": "account_deactivated"}})}
        self.assertEqual(REFRESH_REFUSALS, expected)
        for word, (status, body) in expected.items():
            with self.subTest(word=word):
                model = Model(self.root, lost_login=word)
                answered, data = model.refresh_reply("old")
                self.assertEqual((answered, json.loads(data)), (status, body))
                # Any other token is the spent one of every other case of this file.
                for generation in ("new", "next", None):
                    answered, data = model.refresh_reply(generation)
                    self.assertEqual((answered, json.loads(data)), (400, {"error": "invalid_grant"}))
        answered, data = Model(self.root).refresh_reply("old")
        self.assertEqual((answered, json.loads(data)), (400, {"error": "invalid_grant"}))

    def test_a_token_request_is_known_by_the_role_of_the_token_it_presents(self):
        import json
        from renewal_follow import refresh_generation
        for generation in ("old", "new", "next"):
            self.assertEqual(refresh_generation(json.dumps({"grant_type": "refresh_token",
                "refresh_token": "fixture-refresh-" + generation}).encode()), generation)
        for payload in (b"", b"grant_type=refresh_token&refresh_token=fixture-refresh-old", b"[]", b'"x"',
                        json.dumps({"refresh_token": "fixture-refresh-other"}).encode(),
                        json.dumps({"refresh_token": "some-other-token"}).encode(),
                        json.dumps({"refresh_token": 7}).encode(), json.dumps({}).encode()):
            with self.subTest(payload=payload):
                self.assertIsNone(refresh_generation(payload))

    # codex's own User-Agent form, and aterm's.
    CODEX = "codex_cli_rs/0.0.0 (Mac OS 26.0.0; arm64) fixture"
    HOST = "codex-cli"

    def test_a_request_is_attributed_only_to_a_program_whose_own_user_agent_it_carries(self):
        from renewal_follow import agent_seen, request_agent, request_client
        said = lambda agent: request_agent({} if agent is None else {"user-agent": agent})
        self.assertEqual(said("codex-cli"), "host")
        for agent in (self.CODEX, "codex-tui/0.8.0-trial+g81ec56a7b (Mac OS 27.2.0; arm64) aterm/0.200.0",
                      "codex_cli_rs/0.7.0 (Mac OS 27.2.0; arm64) unknown (codex-tui; 0.7.0)"):
            self.assertEqual(said(agent), "codex", agent)
        # Neither program's: a host under another name, a bare library, nothing at all.
        for agent in (None, "", "codex-cli/1.0", "codex-cli ", "Codex-CLI", "aterm/0.200.0", "curl/8.7.1",
                      "Python-urllib/3.14", "codex_cli_rs/0.0.0", "codex_cli_rs (Mac OS 26.0.0; arm64)",
                      "x codex_cli_rs/0.0.0 (Mac OS 26.0.0; arm64) fixture"):
            self.assertEqual(said(agent), "other", agent)
            # The older reading calls each of them codex-alab's.
            self.assertEqual(request_client({} if agent is None else {"user-agent": agent}), "codex")
        self.assertEqual(agent_seen({"user-agent": "curl/8.7.1 \"x\"\n" + "y" * 200}),
                         "curl/8.7.1 _x__" + "y" * 65)
        self.assertEqual(agent_seen({}), "")

    def get(self, port, path, generation, account, agent=CODEX):
        import json
        import urllib.error
        import urllib.request
        from renewal_follow import access_token
        request = urllib.request.Request(f"http://127.0.0.1:{port}{path}", headers={
            "authorization": "Bearer " + access_token(generation, account), "user-agent": agent})
        try:
            with urllib.request.urlopen(request, timeout=10) as reply:
                return reply.status, json.load(reply)
        except urllib.error.HTTPError as error:
            with error:
                return error.code, error.read()

    def post(self, port, path, body, generation=None, agent=CODEX):
        import json
        import urllib.error
        import urllib.request
        from renewal_follow import access_token
        headers = {"content-type": "application/json", "user-agent": agent}
        if generation is not None:
            headers["authorization"] = "Bearer " + access_token(generation, "source-account")
        request = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=json.dumps(body).encode(),
                                         headers=headers, method="POST")
        try:
            with urllib.request.urlopen(request, timeout=10) as reply:
                return reply.status, reply.read()
        except urllib.error.HTTPError as error:
            with error:
                return error.code, error.read()

    def served(self, word="refresh_token_expired", issuer_only=False):
        from renewal_follow import Model, serve
        model = Model(self.root, lost_login=word)
        server, worker = serve(model, secure=False, issuer_only=issuer_only)
        def stop():
            server.shutdown()
            server.server_close()
            worker.join(timeout=2)
        self.addCleanup(stop)
        return model, server.server_port

    def test_served_requests_record_what_each_login_was_answered_and_by_whom(self):
        import json
        model, port = self.served("refresh_token_invalidated")
        usage, responses, token = "/api/codex/usage", "/v1/responses", "/oauth/token"
        refresh = lambda generation: {"grant_type": "refresh_token",
                                      "refresh_token": "fixture-refresh-" + generation}
        self.assertEqual(self.get(port, usage, "old", "source-account")[0], 200)
        self.assertEqual(self.post(port, responses, self.turn(), "old")[0], 200)
        self.assertEqual(self.post(port, responses, self.turn(), "old")[0], 401)
        self.assertEqual(self.post(port, responses, self.title(), "old")[0], 401)
        status, data = self.post(port, token, refresh("old"))
        self.assertEqual((status, json.loads(data)), (401, {"error": {"code": "refresh_token_invalidated"}}))
        # The source's usage is refused from the loss on, whoever reads it.
        self.assertEqual(self.get(port, usage, "old", "source-account")[0], 401)
        self.assertEqual(self.get(port, usage, "old", "source-account", agent="codex-cli")[0], 401)
        self.assertEqual(self.get(port, "/backend-api/accounts/check", "old", "source-account")[0], 401)
        status, data = self.post(port, token, refresh("old"), agent="codex-cli")
        self.assertEqual(status, 401)
        # The other login is read and answered, and its refresh token is the spent one.
        status, body = self.get(port, usage, "new", "target-account", agent="codex-cli")
        self.assertEqual((status, body["account_id"], body["rate_limit"]["allowed"]), (200, "target-account", True))
        status, body = self.get(port, "/backend-api/accounts/check", "new", "target-account")
        self.assertEqual((status, body["default_account_id"]), (200, "target-account"))
        self.assertEqual(self.post(port, token, refresh("new"), agent="codex-cli")[0], 400)
        self.assertEqual(self.post(port, responses, self.turn(), "new")[0], 200)
        # Each request names its sender by that program's own User-Agent as well.
        self.assertEqual([event["agent"] for event in model.http_events],
                         [event["client"] for event in model.http_events])
        self.assertFalse(any("agentSeen" in event or "unrouted" in event for event in model.http_events))
        kept = [(event["method"], event["path"].rsplit("/", 1)[-1], event.get("account"), event["client"],
                 event.get("status"), event.get("refreshOf")) for event in model.http_events]
        self.assertEqual(kept, [
            ("GET", "usage", "source", "codex", 200, None),
            ("POST", "responses", "source", "codex", 200, None),
            ("POST", "responses", "source", "codex", 401, None),
            ("POST", "responses", "source", "codex", 401, None),
            ("POST", "token", None, "codex", 401, "source"),
            ("GET", "usage", "source", "codex", 401, None),
            ("GET", "usage", "source", "host", 401, None),
            ("GET", "check", "source", "codex", 401, None),
            ("POST", "token", None, "host", 401, "source"),
            ("GET", "usage", "target", "host", 200, None),
            ("GET", "check", "target", "codex", 200, None),
            ("POST", "token", None, "host", 400, "target"),
            ("POST", "responses", "target", "codex", 200, None)])
        # Each refusal says how many requests the provider had when the loss began, so a
        # request that raced the first refusal is not judged as answered after it.
        self.assertEqual([event.get("requestsAtLoss") for event in model.http_events],
                         [None, None, 3, 3, None, None, None, None, None, None, None, None, None])
        self.assertEqual([event.get("allowed") for event in model.http_events if event.get("status") == 200
                          and event["path"].endswith("/usage")], [True, True])
        self.assertEqual(model.usage_reads, ["source", "source", "source", "target"])
        # Nothing of a token is kept: only whose it was.
        self.assertNotIn("fixture-refresh", json.dumps(model.http_events))
        self.assertEqual((model.unrouted_posts, model.fixture_faults), ([], []))
        from renewal_follow import PLAIN_ISSUER, TLS_ISSUER, validate_login_loss
        with self.assertRaisesRegex(RuntimeError, "a refresh token other than the source's was presented: "
                                    "host of target"):
            validate_login_loss(model.http_events, "refresh_token_invalidated", TLS_ISSUER)
        own = [event for event in model.http_events if event.get("refreshOf") != "target"]
        proof = validate_login_loss(own, "refresh_token_invalidated", TLS_ISSUER)
        self.assertEqual((proof["sourceModelRequestsRefused"], proof["sourceRefreshPresentedByCodex"],
                          proof["sourceRefreshPresentedByHost"], proof["sourceUsageReadsRefused"],
                          proof["refusalStatus"], proof["issuer"], proof["tokenRequests"],
                          proof["unroutedRequests"], proof["tlsHandshakesRefused"]),
                         (1, 1, 1, 2, 401, "fixture-tls", 2, 0, 0))
        # These token requests arrived at the provider's own listener, not at a second one.
        with self.assertRaisesRegex(RuntimeError, r"a token request did not arrive at this case's issuer "
                                    r"\(loopback-http\): 2 elsewhere"):
            validate_login_loss(own, "refresh_token_invalidated", PLAIN_ISSUER)
        with self.assertRaisesRegex(RuntimeError, "did not arrive at this case's issuer"):
            validate_login_loss(own, "refresh_token_invalidated", "somewhere")

    def test_a_request_from_neither_program_or_to_no_route_fails_the_providers_side(self):
        import json
        from renewal_follow import TLS_ISSUER, validate_login_loss
        refresh = {"grant_type": "refresh_token", "refresh_token": "fixture-refresh-old"}
        def lost(word="refresh_token_expired"):
            model, port = self.served(word)
            self.assertEqual(self.post(port, "/v1/responses", self.turn(), "old")[0], 200)
            self.assertEqual(self.post(port, "/v1/responses", self.turn(), "old")[0], 401)
            return model, port
        judged = lambda model: validate_login_loss(model.http_events, "refresh_token_expired", TLS_ISSUER)
        model, port = lost()
        self.assertEqual(self.post(port, "/oauth/token", refresh)[0], 401)
        self.assertEqual(self.post(port, "/oauth/token", refresh, agent=self.HOST)[0], 401)
        self.assertEqual((judged(model)["sourceRefreshPresentedByCodex"],
                          judged(model)["sourceRefreshPresentedByHost"]), (1, 1))
        # A second presentation under another name is neither program's: it is not
        # counted as codex-alab's first, and the run does not pass with it.
        self.assertEqual(self.post(port, "/oauth/token", refresh, agent="curl/8.7.1")[0], 401)
        self.assertEqual((model.http_events[-1]["client"], model.http_events[-1]["agent"],
                          model.http_events[-1]["agentSeen"]), ("codex", "other", "curl/8.7.1"))
        with self.assertRaisesRegex(RuntimeError, "a token request or a usage read came from a program that "
                                    "is neither codex-alab nor the host by its User-Agent: "
                                    "POST token from 'curl/8.7.1'"):
            judged(model)
        model, port = lost()
        self.assertEqual(self.get(port, "/api/codex/usage", "new", "target-account", agent="aterm/0.200.0")[0], 200)
        with self.assertRaisesRegex(RuntimeError, "GET usage from 'aterm/0.200.0'"):
            judged(model)
        # A POST the fixture has no route for is answered as before, and fails the case.
        model, port = lost()
        self.assertEqual(self.post(port, "/oauth/revoke", refresh, agent=self.HOST)[0], 400)
        self.assertEqual((model.http_events[-1]["unrouted"], model.unrouted_posts),
                         (True, [{"path": "/oauth/revoke", "client": "host"}]))
        with self.assertRaisesRegex(RuntimeError, "a request the fixture has no route for was sent: "
                                    "host POST /oauth/revoke$"):
            judged(model)
        self.assertNotIn("fixture-refresh", json.dumps(model.http_events))

    def test_a_handshake_a_client_would_not_complete_is_in_a_lost_login_cases_record(self):
        import socket
        import ssl
        from renewal_follow import Model, TLS_ISSUER, refused_handshakes, serve, validate_login_loss
        def listening(model):
            server, worker = serve(model)
            def stop():
                server.shutdown()
                server.server_close()
                worker.join(timeout=2)
            self.addCleanup(stop)
            return server.server_port
        def connect(port, context):
            with socket.create_connection(("127.0.0.1", port), timeout=10) as plain:
                with context.wrap_socket(plain, server_hostname="127.0.0.1") as secure:
                    secure.sendall(b"GET /v1/models HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
                    return secure.recv(64)
        model = Model(self.root, lost_login="refresh_token_expired")
        port = listening(model)
        trusting = ssl.create_default_context(cafile=str(self.root / "fixture-ca.pem"))
        self.assertTrue(connect(port, trusting).startswith(b"HTTP/1.0 200"))
        self.assertEqual(refused_handshakes(model.http_events), [])
        # A client that trusts the platform's roots, as the host's own refresh does.
        with self.assertRaises(ssl.SSLCertVerificationError):
            connect(port, ssl.create_default_context())
        deadline = __import__("time").monotonic() + 10
        while not refused_handshakes(model.http_events) and __import__("time").monotonic() < deadline:
            __import__("time").sleep(0.01)
        refused = refused_handshakes(model.http_events)
        self.assertEqual([(event["method"], event["path"], event["kind"], event["account"], event["client"])
                          for event in refused], [("TLS", "", "handshake-refused", None, None)])
        self.assertTrue(refused[0]["reason"] and type(refused[0]["monotonicUs"]) is int, refused)
        # The server goes on serving, and a case that is no lost login's records none.
        self.assertTrue(connect(port, trusting).startswith(b"HTTP/1.0 200"))
        other = Model(self.root / "other-case")
        (self.root / "other-case").mkdir()
        port = listening(other)
        with self.assertRaises(ssl.SSLCertVerificationError):
            connect(port, ssl.create_default_context())
        trusting = ssl.create_default_context(cafile=str(self.root / "other-case" / "fixture-ca.pem"))
        self.assertTrue(connect(port, trusting).startswith(b"HTTP/1.0 200"))
        self.assertEqual([event["method"] for event in other.http_events], ["GET"])
        # What a refused handshake would have carried is not in the record: it fails the
        # provider's side of a lost-login case, whoever sent it.
        events = [{"method": "POST", "path": "/v1/responses", "kind": "foreground", "account": "source",
                   "client": "codex", "agent": "codex", "status": status, "monotonicUs": at, **more}
                  for status, at, more in ((200, 1, {}), (401, 2, {"requestsAtLoss": 2}))]
        validate_login_loss(events, "refresh_token_expired", TLS_ISSUER)
        with self.assertRaisesRegex(RuntimeError, r"a client would not complete the fixture's TLS handshake, "
                                    r"1 time\(s\), the first with " + refused[0]["reason"] + ": a program that "
                                    "does not trust the run's CA tried to reach the provider"):
            validate_login_loss(events + refused, "refresh_token_expired", TLS_ISSUER)

    def test_the_issuers_listener_answers_token_requests_and_nothing_else(self):
        import json
        model, port = self.served(issuer_only=True)
        status, data = self.post(port, "/oauth/token", {"refresh_token": "fixture-refresh-old"}, agent="codex-cli")
        self.assertEqual((status, json.loads(data)), (401, {"error": {"code": "refresh_token_expired"}}))
        self.assertEqual(self.get(port, "/api/codex/usage", "new", "target-account")[0], 404)
        self.assertEqual(self.post(port, "/v1/responses", self.turn(), "old")[0], 404)
        self.assertEqual(self.post(port, "/oauth/revoke", {})[0], 404)
        self.assertEqual([(event["method"], event["listener"], event["status"], event.get("unrouted"))
                          for event in model.http_events],
                         [("POST", "issuer", 401, None), ("GET", "issuer", 404, True),
                          ("POST", "issuer", 404, True), ("POST", "issuer", 404, True)])
        # Anything it refused fails the case whose issuer it is.
        from renewal_follow import PLAIN_ISSUER, validate_login_loss
        turns = [{"method": "POST", "path": "/v1/responses", "kind": "foreground", "account": "source",
                  "client": "codex", "agent": "codex", "status": status, "monotonicUs": at, **more}
                 for status, at, more in ((200, 1, {}), (401, 2, {"requestsAtLoss": 2}))]
        validate_login_loss(turns + model.http_events[:1], "refresh_token_expired", PLAIN_ISSUER)
        with self.assertRaisesRegex(RuntimeError, "a request the fixture has no route for was sent: "
                                    "codex GET /api/codex/usage on the issuer's listener, "
                                    "codex POST /v1/responses on the issuer's listener, "
                                    "codex POST /oauth/revoke on the issuer's listener"):
            validate_login_loss(turns + model.http_events, "refresh_token_expired", PLAIN_ISSUER)
        # What it refused is listed among the unrouted; no model request reached the model.
        self.assertEqual([entry["path"] for entry in model.unrouted_posts],
                         ["/api/codex/usage", "/v1/responses", "/oauth/revoke"])
        self.assertEqual((model.requests, model.usage_reads, model.login_lost), ([], [], None))

    def test_the_hosts_policy_reply_says_whether_it_moves_a_lost_login(self):
        from renewal_follow import login_policy
        moves = ("OK gen=4 effective=land codex_rotation=1 codex_rotation_five_hour=900 "
                 "codex_rotation_login=1 logins=renew accounts=early")
        self.assertEqual(login_policy(moves), {"codexRotationLogin": True, "codexRotation": True,
            "logins": "renew", "accounts": "early", "effective": "land"})
        quota_only = moves.replace(" codex_rotation_login=1", "")
        self.assertEqual(login_policy(quota_only)["codexRotationLogin"], False)
        self.assertEqual(login_policy(quota_only)["codexRotation"], True)
        # Only the exact word counts.
        for reply in ("OK gen=1 codex_rotation_login=0", "OK gen=1 codex_rotation_login=yes",
                      "OK gen=1 codex_rotation_login", "OK gen=1 xcodex_rotation_login=1"):
            self.assertFalse(login_policy(reply)["codexRotationLogin"], reply)
        for reply in ("ERR not the owner", "OK codex_rotation_login=1", ""):
            with self.assertRaisesRegex(RuntimeError, "harness policy was not answered to its owner"):
                login_policy(reply)

    def test_the_veto_file_is_named_by_a_digest_of_the_token_and_holds_one_word(self):
        import hashlib
        from renewal_follow import kept_vetoes, veto_name
        name = veto_name("fixture-refresh-old")
        self.assertEqual(name, hashlib.sha256(b"alab/login-refusal/v1\x00fixture-refresh-old").hexdigest())
        self.assertNotEqual(name, veto_name("fixture-refresh-new"))
        self.assertEqual(kept_vetoes(self.root), {})
        directory = self.root / "codex-home" / "alab" / "login-refusals"
        directory.mkdir(parents=True)
        (directory / name).write_text("revoked\n")
        # Only a file under a token's digest is a veto.
        (directory / "notes.txt").write_text("other")
        (directory / ("ab" * 32)).mkdir()
        (directory / ("cd" * 32)).symlink_to(directory / name)
        self.assertEqual(kept_vetoes(self.root), {name: "revoked"})

    def test_the_threads_own_record_says_where_its_turn_ended_and_what_was_typed(self):
        import capacity_exec as probe
        from renewal_follow import thread_proof, user_texts
        end = lambda error=None: {"type": "event_msg", "payload": dict({"type": "task_complete"},
            **({"error": {"codex_error_info": error}} if error else {}))}
        said = lambda text, role="user": {"type": "response_item", "payload": {"type": "message", "role": role,
            "content": [{"type": "input_text", "text": text}]}}
        rollout = [said("<environment_context>\n  <cwd>/x</cwd>\n</environment_context>"), said(probe.PROMPT),
                   said("A plan.", role="assistant"), end("unauthorized"), end()]
        self.assertEqual(thread_proof(rollout), {"turnEndedAtLoginWall": True, "userMessagesInThread": 1})
        self.assertEqual(thread_proof(rollout[:-1], answered=False),
                         {"turnEndedAtLoginWall": True, "userMessagesInThread": 1})
        with self.assertRaisesRegex(RuntimeError, r"did not end once at the login wall and stay there: "
                                    r"\['unauthorized', None\]"):
            thread_proof(rollout, answered=False)
        with self.assertRaisesRegex(RuntimeError, r"did not end once at the login wall and once answered: "
                                    r"\['unauthorized'\]"):
            thread_proof(rollout[:-1])
        with self.assertRaisesRegex(RuntimeError, r"once answered: \['usage_limit_exceeded', None\]"):
            thread_proof(rollout[:3] + [end("usage_limit_exceeded"), end()])
        # Anything typed for the move is a second user message.
        for typed in ("Continue the original task.", probe.PROMPT):
            with self.assertRaisesRegex(RuntimeError, "the thread holds 2 typed user messages, not the one prompt"):
                thread_proof(rollout[:4] + [said(typed), end()])
        with self.assertRaisesRegex(RuntimeError, "the thread holds 0 typed user messages"):
            thread_proof([rollout[0], *rollout[2:]])
        request = {"body": {"input": [said("<context>")["payload"], said(probe.PROMPT)["payload"],
                                      said("answer", role="assistant")["payload"], {"type": "function_call"}]}}
        self.assertEqual(user_texts(request), ["<context>", probe.PROMPT])


class LostLoginRun(unittest.TestCase):
    """The run a lost-login oracle judges: the provider's record, the host's trace and
    what the host and codex-alab left under the run's directory."""

    RECEIPT = "ab" * 16
    SOURCE, TARGET = "5e" * 32, "7a" * 32
    INTENT = {"rule": "login-rotate@v1", "field": "rotation", "text": "Continue the original task."}

    def setUp(self):
        import hashlib
        import json
        import tempfile
        from pathlib import Path
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.pool = self.root / "host-state" / "accounts-codex"
        self.pool.mkdir(parents=True)
        self.trace = self.root / "renewal-control-trace.jsonl"
        self.logins = {}
        for name in ("old", "new"):
            path = self.root / ("auth-" + name) / "auth.json"
            path.parent.mkdir()
            path.write_text(json.dumps({"tokens": {"refresh_token": "fixture-refresh-" + name}}))
            self.logins[str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()
            setattr(self, name.upper(), str(path))
        self.state()

    def state(self, **changes):
        import json
        value = dict({"active": "main", "rotations": [], "accounts": {
            "main": {"auth": "signed-out"}, "target": {"auth": "ok"}}}, **changes)
        (self.pool / "state.json").write_text(json.dumps(value))

    # Where this class's token requests arrive: the issuer's own listener, as the case
    # that serves it apart records them, or (None) the provider's, as every other case.
    LISTENER = "issuer"

    def events(self, word="refresh_token_expired", moved=True, extra=(), drop=()):
        """The source answered at 2 s and refused at 10 s, its title too; codex-alab's
        refresh refused at 11 s. With `moved`, the host's own reading and refusal at 12 s,
        its confirming reads at 73 s, its one reading of the destination at 74 s, and the
        successor answered on the destination at 85 s. `drop` removes the rows at those
        seconds. Each row's sender carries that program's own User-Agent (`agent`) unless
        the row says otherwise."""
        import renewal_follow
        status = renewal_follow.REFRESH_REFUSALS[word][0]
        heard = {} if self.LISTENER is None else {"listener": self.LISTENER}
        def event(method, path, at, account="source", client="codex", **more):
            return {"method": method, "path": path, "account": account, "client": client,
                    "agent": client, "monotonicUs": int(at * 1_000_000), **more}
        usage, responses, token = "/api/codex/usage", "/v1/responses", "/oauth/token"
        rows = [
            event("GET", usage, 1, status=200, allowed=True),
            event("POST", responses, 2, kind="foreground", status=200),
            event("POST", responses, 3, kind="thread-title", status=200),
            event("POST", responses, 10, kind="foreground", status=401, requestsAtLoss=4),
            event("POST", token, 11, account=None, kind="non-responses", refreshOf="source",
                  status=status, **heard)]
        if moved:
            rows += [
                event("GET", usage, 12, client="host", status=401),
                event("POST", token, 12.5, account=None, client="host", kind="non-responses",
                      refreshOf="source", status=status, **heard),
                event("GET", usage, 73, client="host", status=401),
                event("GET", usage, 73.5, client="host", status=401),
                event("GET", usage, 74, account="target", client="host", status=200, allowed=True),
                event("GET", usage, 84, account="target", status=200, allowed=True),
                event("POST", responses, 85, account="target", kind="foreground", status=200)]
        else:
            rows += [event("GET", usage, 71, status=401)]
        rows += [event(*row[:3], **{**(heard if row[1] == token else {}), **row[3]}) for row in extra]
        return sorted((row for row in rows if row["monotonicUs"] / 1_000_000 not in drop),
                      key=lambda entry: entry["monotonicUs"])

    def calls(self, *rows):
        """Write the host's control trace: (kind, verb, reply or outcome, second)."""
        import json
        self.trace.write_text("".join(json.dumps({"kind": kind, "verb": verb,
            ("outcome" if kind == "renewal" else "reply"): said, "monotonicUs": int(at * 1_000_000)}) + "\n"
            for kind, verb, said, at in rows))

    def ready(self, receipt=None, source=None):
        return (f"OK codex-login-rotation state=ready id={receipt or self.RECEIPT} thread=t turn=w "
                f"source_account={source or self.SOURCE}")

    WAITING = "OK codex-login-rotation state=waiting retry_ms=5000"

    # When the host answers `ready` in a moved run: the fourteenth ask, five seconds after
    # the thirteenth, 63.7 s after the host's own refusal at 12.5 s.
    READY = 76.2
    # The host's clock when the controller stopped watching a moved run.
    WATCHED = 150

    def asked(self, ready_at=None, first=11.2, every=5.0, also=()):
        """The host's trace of a move: `waiting` to an ask at `first` and each `every`
        seconds after until `ready_at`, `ready` there, then the commit; `also` adds rows."""
        ready_at = self.READY if ready_at is None else ready_at
        count = 0
        rows = []
        while first + count * every < ready_at - 1e-6:
            rows.append(("login-rotation", "prepare", self.WAITING, round(first + count * every, 6)))
            count += 1
        rows += [("login-rotation", "prepare", self.ready(), ready_at),
                 ("login-rotation", "commit", f"OK codex-login-rotation state=waiting id={self.RECEIPT}",
                  ready_at + 0.1), *also]
        return sorted(rows, key=lambda row: row[3])

    def moved(self, ready_at=None, **options):
        self.calls(*self.asked(ready_at, **options))

    def receipt(self, **changes):
        import json
        directory = self.pool / "renewal-follow"
        directory.mkdir(parents=True, exist_ok=True)
        value = dict({"id": self.RECEIPT, "phase": "consumed", "old_auth": self.OLD, "new_auth": self.NEW,
            "intent": {"purpose": "login_rotation", "identity": {"account": self.SOURCE, "user": "u"}},
            "destination": {"account": self.TARGET, "user": "v"}}, **changes)
        (directory / (value["id"] + ".json")).write_text(json.dumps(value))
        # The host's index of the intent, which is no receipt.
        (directory / "login-rotation-intent-0f").write_text("{}")
        return value

    @staticmethod
    def rollout(answered=True, typed=()):
        import capacity_exec as probe
        end = lambda error=None: {"type": "event_msg", "payload": dict({"type": "task_complete"},
            **({"error": {"codex_error_info": error}} if error else {}))}
        said = lambda text: {"type": "response_item", "payload": {"type": "message", "role": "user",
            "content": [{"type": "input_text", "text": text}]}}
        return [said("<environment_context/>"), said(probe.PROMPT), end("unauthorized"),
                *[said(text) for text in typed], *([end()] if answered else [])]

    @staticmethod
    def requests(*typed):
        import capacity_exec as probe
        said = lambda text: {"type": "message", "role": "user", "content": [{"type": "input_text", "text": text}]}
        turn = [said("<environment_context/>"), said(probe.PROMPT)]
        return [{"body": {"input": turn}}, {"body": {"input": turn}},
                {"body": {"input": turn + [said(text) for text in typed]}}]

    def veto(self, word, token="fixture-refresh-old"):
        from renewal_follow import veto_name
        directory = self.root / "codex-home" / "alab" / "login-refusals"
        directory.mkdir(parents=True, exist_ok=True)
        (directory / veto_name(token)).write_text(word)

    POLICY = {"codexRotationLogin": True, "codexRotation": True, "logins": "renew", "accounts": "early",
              "effective": "land"}


class LostLoginMoved(LostLoginRun):
    """What fails a `lost-login-expired` run by name while it runs, and its verdict."""

    def failure(self, events=None, policy=None, now=None):
        from renewal_follow import lost_login_failure
        return lost_login_failure(self.root, self.events() if events is None else events,
                                  self.POLICY if policy is None else policy,
                                  None if now is None else now * 1_000_000)

    def verdict(self, events=None, **changes):
        from renewal_follow import PLAIN_ISSUER, lost_login_verdict
        arguments = dict(requests=self.requests(), receipt=self.receipt(), policy=self.POLICY,
            old_auth=self.OLD, new_auth=self.NEW, logins=self.logins, rollout=self.rollout(),
            intent=self.INTENT, issuer=PLAIN_ISSUER, watched_us=self.WATCHED * 1_000_000)
        arguments.update(changes)
        return lost_login_verdict(self.root, self.events() if events is None else events, **arguments)

    def test_a_run_that_is_moving_or_has_moved_fails_nothing(self):
        from renewal_follow import LOGIN_ASK_SECONDS
        # Nothing lost yet: nothing to judge, however late.
        self.assertIsNone(self.failure(self.events()[:3], now=500))
        # Lost, the host not asked yet, inside the time codex-alab has to ask.
        self.assertIsNone(self.failure(self.events()[:5], now=11 + LOGIN_ASK_SECONDS))
        self.moved()
        for now in (12, 70, 500, None):
            self.assertIsNone(self.failure(now=now))

    def test_a_host_without_the_word_a_host_that_declines_and_a_silent_client_fail_by_name(self):
        from renewal_follow import LOGIN_ASK_SECONDS
        silent = self.events()[:5]
        self.assertEqual(self.failure(silent, policy=dict(self.POLICY, codexRotationLogin=False), now=12),
                         "the host's policy prints no codex_rotation_login=1, so codex-alab asks it nothing")
        # A policy that was never read fails nothing by itself: the verdict does.
        self.assertIsNone(self.failure(silent, policy={"unread": "KeyError: 'socket'"}, now=12))
        late = self.failure(silent, now=11 + LOGIN_ASK_SECONDS + 1)
        self.assertEqual(late, f"codex-alab did not ask the host to move it within {LOGIN_ASK_SECONDS} s of "
            "the refusal; after the first 401: token requests 1 x codex of source (401); usage reads none; "
            "codex-login-rotation calls none; codex-renewal calls none")
        # Before codex-alab's own refresh is refused the clock runs from the first 401.
        self.assertIsNone(self.failure(self.events()[:4], now=10 + LOGIN_ASK_SECONDS))
        self.assertIn("did not ask", self.failure(self.events()[:4], now=10 + LOGIN_ASK_SECONDS + 1))
        # The run has ended: no clock, and no ask is the verdict's to name.
        self.assertIsNone(self.failure(silent))
        # A host that answers that no move is coming, with what it did first.
        self.calls(("login-rotation", "prepare", self.WAITING, 11.2),
                   ("login-rotation", "prepare", self.WAITING, 16.2),
                   ("login-rotation", "prepare", "OK codex-login-rotation state=not-needed", 21.2),
                   ("renewal", "prepare", "waiting", 21.3))
        declined = self.events(drop=(12.5, 73, 73.5, 74, 84, 85))
        self.assertEqual(self.failure(declined, now=22), "the host answered codex-login-rotation not-needed: "
            "it moves no such login; after the first 401: token requests 1 x codex of source (401); "
            "usage reads 1 x host of source (401); codex-login-rotation calls "
            "1 x prepare: OK codex-login-rotation state=not-needed, "
            "2 x prepare: OK codex-login-rotation state=waiting retry_ms=5000; "
            "codex-renewal calls 1 x prepare: waiting")
        # An ask traced before the loss is not this wall's.
        self.calls(("login-rotation", "prepare", "OK codex-login-rotation state=not-needed", 5))
        self.assertIsNone(self.failure(silent, now=12))

    def test_the_verdict_records_the_whole_move(self):
        self.moved()
        held = {}
        proof = self.verdict(held=held)
        self.assertEqual({key: proof[key] for key in (
            "refusal", "refusalStatus", "issuer", "tokenRequests", "sourceModelRequestsRefused",
            "sourceRefreshPresentedByCodex", "sourceRefreshPresentedByHost", "sourceUsageReadsByHost",
            "preparesAsked", "preparesAnsweredWaiting", "preparesAfterReady", "firstAskedSecondsAfterLoss",
            "readySecondsAfterHostRefusal", "readySecondsAfterFirstAsk", "watchedSecondsAfterReady",
            "refreshPresentedAfterReady", "destinationUsageReadsByHost", "destinationUsageReadsBySuccessor",
            "destinationRefreshPresented", "recoveredTurnRequests", "receiptPurpose", "receiptsOpened",
            "hostActive", "hostRotations", "hostCanary", "hostSourceAuth", "recordedIntentRule",
            "usageRotationCalls", "renewalCalls", "unroutedRequests", "tlsHandshakesRefused",
            "askPeriodSeconds", "onePrepareAPass", "smallestAskGapSeconds", "largestAskGapSeconds")},
            {"refusal": "refresh_token_expired", "refusalStatus": 401, "issuer": "loopback-http",
             "tokenRequests": 2, "sourceModelRequestsRefused": 1,
             "sourceRefreshPresentedByCodex": 1, "sourceRefreshPresentedByHost": 1,
             "sourceUsageReadsByHost": 3, "preparesAsked": 14, "preparesAnsweredWaiting": 13,
             "preparesAfterReady": 0, "firstAskedSecondsAfterLoss": 1.2,
             "readySecondsAfterHostRefusal": 63.7, "readySecondsAfterFirstAsk": 65.0,
             "watchedSecondsAfterReady": 73.8, "refreshPresentedAfterReady": 0,
             "destinationUsageReadsByHost": 1, "destinationUsageReadsBySuccessor": 1,
             "destinationRefreshPresented": 0, "recoveredTurnRequests": 1,
             "receiptPurpose": "login_rotation", "receiptsOpened": 1,
             "hostActive": "main", "hostRotations": 0, "hostCanary": None, "hostSourceAuth": "signed-out",
             "recordedIntentRule": "login-rotate@v1", "usageRotationCalls": 0, "renewalCalls": 0,
             "unroutedRequests": 0, "tlsHandshakesRefused": 0,
             "askPeriodSeconds": 5, "onePrepareAPass": True, "smallestAskGapSeconds": 5.0,
             "largestAskGapSeconds": 5.0})
        self.assertEqual(proof["askGapsSeconds"], [5.0] * 13)
        for key in ("loginFilesUnchanged", "sourceStillHoldsRefusedToken", "turnEndedAtLoginWall",
                    "nothingTypedForTheMove", "policyPrintedCodexRotationLogin",
                    "receiptDestinationDiffersFromSource"):
            self.assertIs(proof[key], True, key)
        self.assertEqual((proof["successorAuthPath"], proof["userMessagesInThread"]), (self.NEW, 1))
        # What held before the last proof is the record less that proof's own entries.
        cadence = ("askPeriodSeconds", "onePrepareAPass", "smallestAskGapSeconds", "largestAskGapSeconds")
        self.assertEqual(held, {key: value for key, value in proof.items() if key not in cadence})
        # codex-alab may be moved without having presented the token itself: once at most.
        self.assertEqual(self.verdict(self.events(drop=(11,)))["sourceRefreshPresentedByCodex"], 0)
        # The whole minute, on the provider's clock: 60.0 s after the host's refusal passes.
        self.moved(ready_at=72.5, first=12.5)
        self.assertEqual(self.verdict(self.events(drop=(73, 73.5, 74), extra=[
            ("GET", "/api/codex/usage", 72.2, {"client": "host", "status": 401}),
            ("GET", "/api/codex/usage", 72.3, {"account": "target", "client": "host", "status": 200,
                                               "allowed": True})]))["readySecondsAfterHostRefusal"], 60.0)

    def test_the_same_move_with_the_issuer_under_the_fixtures_tls_is_the_specified_case(self):
        from renewal_follow import PLAIN_ISSUER, TLS_ISSUER
        self.moved()
        # Token requests that arrived at the issuer's own listener are not that layout's.
        with self.assertRaisesRegex(RuntimeError, r"did not arrive at this case's issuer \(fixture-tls\): "
                                    "2 elsewhere"):
            self.verdict(issuer=TLS_ISSUER)
        self.LISTENER = None
        self.assertEqual(self.verdict(issuer=TLS_ISSUER)["issuer"], "fixture-tls")
        with self.assertRaisesRegex(RuntimeError, r"did not arrive at this case's issuer \(loopback-http\)"):
            self.verdict(issuer=PLAIN_ISSUER)
        # A client that would not complete the fixture's handshake, as the recorded host's
        # refresh does not, fails it: what it would have sent is not in the record.
        refused = {"method": "TLS", "path": "", "kind": "handshake-refused", "account": None, "client": None,
                   "monotonicUs": 12_400_000, "reason": "UNEXPECTED_EOF_WHILE_READING"}
        with self.assertRaisesRegex(RuntimeError, "a client would not complete the fixture's TLS handshake, "
                                    r"1 time\(s\), the first with UNEXPECTED_EOF_WHILE_READING"):
            self.verdict(sorted(self.events() + [refused], key=lambda event: event["monotonicUs"]),
                         issuer=TLS_ISSUER)

    def test_how_often_it_asked_is_the_last_proof_and_fails_by_name(self):
        from renewal_follow import login_ask_cadence
        second = lambda value: int(value * 1_000_000)
        steady = [second(11.2 + 5 * step) for step in range(14)]
        self.assertEqual(login_ask_cadence(steady, second(10)), {"askPeriodSeconds": 5, "onePrepareAPass": True,
            "smallestAskGapSeconds": 5.0, "largestAskGapSeconds": 5.0})
        # A pass takes its own time, and the trace's clock is the host's: 4.5 to 10 s.
        self.assertEqual(login_ask_cadence([second(1), second(5.5), second(15.5)], 0)["smallestAskGapSeconds"], 4.5)
        self.assertEqual(login_ask_cadence([second(1)], 0)["smallestAskGapSeconds"], None)
        doubled = "codex-alab asked the host again 0.016 s after an ask, 1.216 s after the loss "
        with self.assertRaisesRegex(RuntimeError, doubled + r"\(1 such pair\(s\) among 15 asks\): that is more "
                                    "than one ask a pass, a pass every 5 s"):
            login_ask_cadence(sorted(steady + [second(11.216)]), second(10))
        with self.assertRaisesRegex(RuntimeError, r"again 4.499 s after an ask"):
            login_ask_cadence([second(1), second(5.499)], 0)
        with self.assertRaisesRegex(RuntimeError, "codex-alab left 10.1 s between two asks, 1.0 s after the "
                                    "loss: that is not a pass every 5 s"):
            login_ask_cadence([second(1), second(11.1)], 0)
        # In the verdict it is judged after everything else, and what held is kept.
        self.moved(also=[("login-rotation", "prepare", self.WAITING, 11.216)])
        held = {}
        with self.assertRaisesRegex(RuntimeError, "^" + doubled):
            self.verdict(held=held)
        self.assertEqual((held["preparesAsked"], held["preparesAnsweredWaiting"], held["askGapsSeconds"][:2],
                          held["readySecondsAfterHostRefusal"], held["receiptPurpose"], held["hostActive"],
                          held["recoveredTurnRequests"]),
                         (15, 14, [0.016, 4.984], 63.7, "login_rotation", "main", 1))
        self.assertNotIn("onePrepareAPass", held)
        # A flood: two thousand asks a millisecond apart.
        flood = [("login-rotation", "prepare", self.WAITING, round(20 + step / 1000, 3)) for step in range(2000)]
        self.moved(also=flood)
        with self.assertRaisesRegex(RuntimeError, r"asked the host again 3.800 s after an ask, 10.000 s after "
                                    r"the loss \(2002 such pair\(s\) among 2014 asks\)"):
            self.verdict()
        # A pass that was skipped, and an ask after `ready`, which is counted and not timed.
        self.calls(*sorted([row for row in self.asked() if row[3] not in (41.2, 46.2)]
                           + [("login-rotation", "prepare", self.WAITING, 46.3)], key=lambda row: row[3]))
        with self.assertRaisesRegex(RuntimeError, "codex-alab left 10.1 s between two asks, 26.2 s after "
                                    "the loss: that is not a pass every 5 s"):
            self.verdict()
        self.moved(also=[("login-rotation", "prepare", self.ready(), 76.25)])
        self.assertEqual(self.verdict()["preparesAfterReady"], 1)
        # A failure of any other proof is that proof's, and nothing is kept as held.
        self.moved(also=[("login-rotation", "prepare", self.WAITING, 11.216)])
        held = {}
        with self.assertRaisesRegex(RuntimeError, "the host presented the source's refresh token 2 times"):
            self.verdict(self.events(extra=[("POST", "/oauth/token", 40, {
                "account": None, "client": "host", "refreshOf": "source", "status": 401})]), held=held)
        self.assertEqual(held, {})

    def test_a_moved_session_is_watched_before_the_run_says_nothing_followed(self):
        from renewal_follow import LOGIN_MOVED_WATCH_SECONDS, login_move_watched
        second = 1_000_000
        self.assertFalse(login_move_watched(80 * second, 80 * second))
        self.assertFalse(login_move_watched(80 * second, (80 + LOGIN_MOVED_WATCH_SECONDS) * second - 1))
        self.assertTrue(login_move_watched(80 * second, (80 + LOGIN_MOVED_WATCH_SECONDS) * second))
        self.moved()
        self.assertEqual(self.verdict(watched_us=int((self.READY + LOGIN_MOVED_WATCH_SECONDS) * second))[
            "watchedSecondsAfterReady"], 70.0)
        # A run that stopped when the successor answered watched nine seconds.
        with self.assertRaisesRegex(RuntimeError, "the moved session was watched 8.8 s after ready, not 70 s"):
            self.verdict(watched_us=85 * second)
        with self.assertRaisesRegex(RuntimeError, r"was watched -76.2 s after ready"):
            self.verdict(watched_us=0)
        # What the watch is for: a token presented, the destination read again by the
        # host, or the turn sent again, long after the successor answered.
        token = lambda client: ("POST", "/oauth/token", 140, {"account": None, "client": client,
                                                               "refreshOf": "source", "status": 401})
        with self.assertRaisesRegex(RuntimeError, "the host presented the source's refresh token 2 times"):
            self.verdict(self.events(extra=[token("host")]))
        with self.assertRaisesRegex(RuntimeError, "a refresh token was presented after the host answered ready"):
            self.verdict(self.events(drop=(11,), extra=[token("codex")]))
        with self.assertRaisesRegex(RuntimeError, r"exactly once, as usable, before ready: 2 read\(s\)"):
            self.verdict(self.events(extra=[("GET", "/api/codex/usage", 140, {
                "account": "target", "client": "host", "status": 200, "allowed": True})]))
        with self.assertRaisesRegex(RuntimeError, "the successor sent the recovered turn 2 times, not once"):
            self.verdict(self.events(extra=[("POST", "/v1/responses", 140, {
                "account": "target", "kind": "foreground", "status": 200})]))
        # The successor's own readings of the login it runs on are counted, and fail nothing.
        self.assertEqual(self.verdict(self.events(extra=[("GET", "/api/codex/usage", at, {
            "account": "target", "status": 200, "allowed": True}) for at in (90, 140)]))[
            "destinationUsageReadsBySuccessor"], 3)

    def test_the_verdict_fails_each_wrong_outcome(self):
        from pathlib import Path
        from renewal_follow import LOGIN_NOT_MOVED
        token, usage, responses = "/oauth/token", "/api/codex/usage", "/v1/responses"
        self.moved()
        self.verdict()
        refresh = lambda at, of="source", client="host", status=401: ("POST", token, at, {
            "account": None, "client": client, "refreshOf": of, "status": status})
        read = lambda at, account, client, status=200, **more: ("GET", usage, at, {
            "account": account, "client": client, "status": status, **more})
        wrong = {
            # The provider's side.
            "the source was not answered once and refused with 401 from then on: 200, 401, 200":
                self.events(extra=[("POST", responses, 20, {"kind": "foreground", "status": 200})]),
            "the provider answered the source's access token after it had refused it":
                self.events(extra=[read(20, "source", "codex")]),
            r"not answered the case's refusal \(401 refresh_token_expired\)":
                self.events(extra=[refresh(13, status=400)]),
            "a refresh token other than the source's was presented: host of target":
                self.events(extra=[refresh(60, of="target")]),
            "a refresh token other than the source's was presented: codex of None":
                self.events(extra=[refresh(60, of=None, client="codex")]),
            "codex-alab presented the source's refused refresh token 2 times, not once at most":
                self.events(extra=[refresh(30, client="codex")]),
            "a login other than the source was refused":
                self.events(extra=[read(90, "target", "codex", status=401)]),
            # The host's side.
            "the host presented the source's refresh token 0 times, not exactly once":
                self.events(drop=(12.5,)),
            "the host presented the source's refresh token 2 times, not exactly once":
                self.events(extra=[refresh(40)]),
            "a refresh token was presented after the host answered ready":
                self.events(drop=(11,), extra=[refresh(80, client="codex")]),
            r"did not read the destination's usage exactly once, as usable, before ready: 0 read\(s\)":
                self.events(drop=(74,)),
            r"did not read the destination's usage exactly once, as usable, before ready: 2 read\(s\)":
                self.events(extra=[read(74.5, "target", "host", allowed=True)]),
            r"exactly once, as usable, before ready: 1 read\(s\)":
                self.events(drop=(74,), extra=[read(77, "target", "host", allowed=True)]),
            "as usable, before ready":
                self.events(drop=(74,), extra=[read(74, "target", "host", allowed=False)]),
            "the destination's usage was read by someone other than the host before the move":
                self.events(extra=[read(60, "target", "codex", allowed=True)]),
            "the host read the destination before its own refusal of the source":
                self.events(drop=(74,), extra=[read(12.2, "target", "host", allowed=True)]),
            "the host read the source's usage 0 times, not within its two bounded reads":
                self.events(drop=(12, 73, 73.5)),
            "the host read the source's usage 4 times, not within its two bounded reads":
                self.events(extra=[read(40, "source", "host", status=401)]),
        }
        for message, events in wrong.items():
            with self.subTest(message=message):
                with self.assertRaisesRegex(RuntimeError, message):
                    self.verdict(events)
        with self.assertRaisesRegex(RuntimeError, "no model request of the source was refused with 401"):
            self.verdict(self.events()[:3])
        # The policy's word must have been read, and printed.
        for policy, said in (({"unread": "KeyError: 'socket'"}, "KeyError: 'socket'"), (None, "None"),
                             ({"reply": "OK gen=1 codex_rotation_login=1"}, "None")):
            with self.assertRaisesRegex(RuntimeError, "host's word for the move was never read: " + said):
                self.verdict(policy=policy)
        with self.assertRaisesRegex(RuntimeError, "^" + LOGIN_NOT_MOVED + "the host's policy prints no "
                                    "codex_rotation_login=1"):
            self.verdict(policy=dict(self.POLICY, codexRotationLogin=False))
        # The host's answers.
        traces = {
            "the host was never asked to move the lost login": [],
            "the host was asked to move the session before its login was lost": [
                ("login-rotation", "prepare", self.WAITING, 5),
                ("login-rotation", "prepare", self.WAITING, 11.2),
                ("login-rotation", "prepare", self.ready(), 75)],
            "the host never answered ready for this receipt": [
                ("login-rotation", "prepare", self.WAITING, 11.2),
                ("login-rotation", "prepare", self.ready(receipt="cd" * 16), 75)],
            "the host never answered ready for this receipt$": [
                ("login-rotation", "prepare", self.WAITING, 11.2),
                ("login-rotation", "commit", self.ready(), 75)],
            "did not answer waiting, and only waiting, until it answered ready: $": [
                ("login-rotation", "prepare", self.ready(), 75)],
            "did not answer waiting, and only waiting, until it answered ready: ERR codex-login-rotation busy": [
                ("login-rotation", "prepare", self.WAITING, 11.2),
                ("login-rotation", "prepare", "ERR codex-login-rotation busy", 16.2),
                ("login-rotation", "prepare", self.ready(), 75)],
            "the host answered ready 47.5 s after its own refusal, before it had settled": [
                ("login-rotation", "prepare", self.WAITING, 11.2),
                ("login-rotation", "prepare", self.ready(), 60)],
            "the receipt's source is not the account the host answered ready for": [
                ("login-rotation", "prepare", self.WAITING, 11.2),
                ("login-rotation", "prepare", self.ready(source=self.TARGET), 75)],
            "^" + LOGIN_NOT_MOVED + "the host answered codex-login-rotation not-needed": [
                ("login-rotation", "prepare", self.WAITING, 11.2),
                ("login-rotation", "prepare", "OK codex-login-rotation state=not-needed", 16.2),
                ("login-rotation", "prepare", self.ready(), 75)],
            r"a usage rotation or a same-account renewal was asked for: \{'rotation': 1, 'renewal': 0\}": [
                ("login-rotation", "prepare", self.WAITING, 11.2),
                ("rotation", "prepare", "OK codex-rotation state=waiting", 11.3),
                ("login-rotation", "prepare", self.ready(), 75)],
            r"a usage rotation or a same-account renewal was asked for: \{'rotation': 0, 'renewal': 1\}": [
                ("login-rotation", "prepare", self.WAITING, 11.2),
                ("renewal", "prepare", "waiting", 11.3),
                ("login-rotation", "prepare", self.ready(), 75)],
        }
        for message, rows in traces.items():
            with self.subTest(message=message):
                self.calls(*rows)
                with self.assertRaisesRegex(RuntimeError, message):
                    self.verdict()
        # A tenth of a second short of the minute is short, though the host's own
        # arithmetic, in whole seconds, could allow it.
        self.moved(ready_at=72.4)
        with self.assertRaisesRegex(RuntimeError, "the host answered ready 59.9 s after its own refusal, "
                                    "before it had settled"):
            self.verdict()
        self.moved()
        # The receipt: a login rotation's, between two accounts named by digest, from the
        # source's own login file to the other account's own.
        receipts = {
            "purpose weekly_quota": {"intent": {"purpose": "weekly_quota", "identity": {"account": self.SOURCE}}},
            "purpose None": {"intent": None},
            "5e5e5e5e5e5e to 5e5e5e5e5e5e": {"destination": {"account": self.SOURCE}},
            "5e5e5e5e5e5e to target-accou": {"destination": {"account": "target-account"}},
            "source-accou to 7a7a7a7a7a7a": {"intent": {"purpose": "login_rotation",
                                                         "identity": {"account": "source-account"}}},
            "auth-old to auth-old": {"new_auth": self.OLD},
            "auth-new to auth-new": {"old_auth": self.NEW},
        }
        for message, changes in receipts.items():
            with self.subTest(message=message):
                with self.assertRaisesRegex(RuntimeError, "the receipt is not a login rotation from the "
                                            "source's login file to another account's own: .*" + message):
                    self.verdict(receipt=self.receipt(**changes))
        self.receipt()
        (self.pool / "renewal-follow" / ("cd" * 16 + ".json")).write_text("{}")
        with self.assertRaisesRegex(RuntimeError, "the host opened 2 receipts, not one"):
            self.verdict()
        (self.pool / "renewal-follow" / ("cd" * 16 + ".json")).unlink()
        # The session's own record of what it owed for this wall.
        for intent in (None, {}, dict(self.INTENT, rule="rotate@v1"), dict(self.INTENT, field="renewal"),
                       dict(self.INTENT, text="Continue.")):
            with self.subTest(intent=intent):
                with self.assertRaisesRegex(RuntimeError, "did not hold the lost login's own intent"):
                    self.verdict(intent=intent)
        # A token that simply ended is never vetoed.
        self.veto("other")
        with self.assertRaisesRegex(RuntimeError, "codex-alab kept a veto of a token that simply ended"):
            self.verdict()
        for path in (self.root / "codex-home" / "alab" / "login-refusals").iterdir():
            path.unlink()
        # The host's pool: the move is the session's alone.
        pools = {
            "the host's active account changed: target": {"active": "target"},
            "the host charged a usage rotation": {"rotations": [5000]},
            "the host set a rotation canary": {"canary": {"account": "target"}},
        }
        for message, changes in pools.items():
            with self.subTest(message=message):
                self.state(**changes)
                with self.assertRaisesRegex(RuntimeError, message):
                    self.verdict()
        self.state()
        (self.pool / "logins.json").write_text('{"native_rotations": {"op": "target"}}')
        with self.assertRaisesRegex(RuntimeError, "the host bound a rotation to a native operation"):
            self.verdict()
        (self.pool / "logins.json").write_text('{"native_rotations": {}}')
        self.verdict()
        # The login files: nothing signs anyone in and no token is copied.
        original = Path(self.NEW).read_text()
        Path(self.NEW).write_text(Path(self.OLD).read_text())
        with self.assertRaisesRegex(RuntimeError, "a fixture login file changed during the run: auth-new"):
            self.verdict()
        import hashlib
        copied = dict(self.logins, **{self.NEW: hashlib.sha256(Path(self.NEW).read_bytes()).hexdigest()})
        with self.assertRaisesRegex(RuntimeError, "no longer holds the refresh token it was written with"):
            self.verdict(logins=copied)
        Path(self.NEW).write_text(original)
        with self.assertRaisesRegex(RuntimeError, "were not both held to the bytes written"):
            self.verdict(logins={self.OLD: self.logins[self.OLD]})
        # Nothing was typed for the move, by the requests and by the thread's own record.
        with self.assertRaisesRegex(RuntimeError, "something was typed beside the original prompt"):
            self.verdict(requests=self.requests("Continue the original task."))
        with self.assertRaisesRegex(RuntimeError, "the thread holds 2 typed user messages"):
            self.verdict(rollout=self.rollout(typed=["Continue the original task."]))
        with self.assertRaisesRegex(RuntimeError, r"once answered: \['unauthorized'\]"):
            self.verdict(rollout=self.rollout(answered=False))
        self.verdict()


class LostLoginStayed(LostLoginRun):
    """What ends a lost-login case that moves nothing, what fails it at once, and its verdict."""

    HELD = 82
    # These cases keep the issuer where every other case has it: the provider's listener.
    LISTENER = None

    def own_path(self, *extra):
        self.calls(("renewal", "prepare", "waiting", 11.04), ("renewal", "prepare", "waiting", 71.06), *extra)

    def verdict(self, case="lost-login-invalidated", events=None, **changes):
        from renewal_follow import LOST_LOGIN_CASES, login_stayed_verdict
        word = LOST_LOGIN_CASES[case][0]
        arguments = dict(policy=self.POLICY, case=LOST_LOGIN_CASES[case], old_auth=self.OLD,
            new_auth=self.NEW, logins=self.logins, rollout=self.rollout(answered=False),
            held_us=self.HELD * 1_000_000)
        arguments.update(changes)
        return login_stayed_verdict(self.root, self.events(word, moved=False) if events is None else events,
                                    **arguments)

    def setUp(self):
        super().setUp()
        self.state(accounts={"main": {"auth": "ok"}, "target": {"auth": "ok"}})

    def test_the_case_ends_only_with_the_wall_on_its_own_path_for_two_looks(self):
        from renewal_follow import LOGIN_WALL_HOLD_SECONDS, login_wall_stood, own_path
        events = self.events("refresh_token_invalidated", moved=False)
        stood = lambda seen, now: login_wall_stood(self.root, seen, int(now * 1_000_000))
        # No trace, no refusal of codex-alab's refresh, one look only: not yet.
        self.assertFalse(stood(events, 500))
        self.own_path()
        self.assertFalse(stood(events[:4], 500))
        self.assertEqual(own_path(events[:4], self.trace), (None, None))
        self.assertFalse(stood(events, 11 + LOGIN_WALL_HOLD_SECONDS - 0.1))
        self.assertTrue(stood(events, 11 + LOGIN_WALL_HOLD_SECONDS))
        self.assertEqual(own_path(events, self.trace),
                         ([("waiting", 11_040_000), ("waiting", 71_060_000)], 11_000_000))
        self.calls(("renewal", "prepare", "waiting", 11.04))
        self.assertFalse(stood(events, 500))
        # A renewal call traced before the refusal, or one that is no ask, is not a look.
        self.calls(("renewal", "prepare", "waiting", 9), ("renewal", "prepare", "waiting", 11.04),
                   ("renewal", "commit", "waiting", 71.06))
        self.assertFalse(stood(events, 500))

    def test_everything_a_move_or_an_ask_leaves_is_named(self):
        import json
        from renewal_follow import login_move_evidence
        events = self.events("refresh_token_invalidated", moved=False)
        found = lambda seen=events: login_move_evidence(self.root, seen, self.NEW)
        self.own_path()
        # The wall's own path, the source's own reads and a session on the source's login
        # file are no move.
        sessions = self.root / "codex-home" / "alab" / "sessions"
        sessions.mkdir(parents=True)
        (sessions / "11.json").write_text(json.dumps({"env": {"CODEX_AUTH_PATH": self.OLD}}))
        (sessions / "13.json").write_text("{")
        self.assertEqual(found(), [])
        self.own_path(("login-rotation", "prepare", self.WAITING, 11.2),
                      ("login-rotation", "prepare", self.WAITING, 16.2))
        self.assertEqual(found(), ["2 codex-login-rotation line(s) sent, the first answered "
                                   "OK codex-login-rotation state=waiting retry_ms=5000"])
        self.own_path(("rotation", "prepare", "OK codex-rotation state=not-needed", 11.2))
        self.assertEqual(found(), ["1 codex-rotation line(s) sent, the first answered "
                                   "OK codex-rotation state=not-needed"])
        self.own_path()
        moved = self.events("refresh_token_invalidated")
        self.assertEqual(found(moved), ["2 usage read(s) of the other account by codex, host",
                                        "model request(s) from target"])
        presented = events + [{"method": "POST", "path": "/oauth/token", "account": None, "client": "host",
                               "refreshOf": "target", "status": 400, "monotonicUs": 90_000_000}]
        self.assertEqual(found(presented), ["1 presentation(s) of another login's refresh token"])
        self.receipt()
        self.assertEqual(found(), ["2 handoff receipt file(s)"])
        for path in (self.pool / "renewal-follow").iterdir():
            path.unlink()
        (self.root / "native-actions.jsonl").write_text(json.dumps({"action": "open"}) + "\n")
        self.assertEqual(found(), ["native action(s): open"])
        (self.root / "native-actions.jsonl").unlink()
        (sessions / "12.json").write_text(json.dumps({"env": {"CODEX_AUTH_PATH": self.NEW}}))
        self.assertEqual(found(), ["session process 12 runs on the other login file"])

    def test_the_verdict_records_what_stopped_the_move_for_each_refusal(self):
        from renewal_follow import LOST_LOGIN_CASES, LOST_LOGIN_MOVED, VETO_WORDS
        self.own_path()
        for case, (word, host_moves, _) in LOST_LOGIN_CASES.items():
            if case in LOST_LOGIN_MOVED:
                continue
            with self.subTest(case=case):
                directory = self.root / "codex-home" / "alab" / "login-refusals"
                for path in directory.glob("*"):
                    path.unlink()
                if VETO_WORDS[word] is not None:
                    self.veto(VETO_WORDS[word])
                policy = dict(self.POLICY, codexRotationLogin=host_moves)
                proof = self.verdict(case, policy=policy)
                self.assertEqual({key: proof[key] for key in (
                    "refusal", "refusalStatus", "vetoKept", "stoppedBy", "policyPrintedCodexRotationLogin",
                    "policyLogins", "codexLoginRotationLines", "codexRotationLines", "otherAccountUsageReads",
                    "receipts", "nativeActions", "renewalPreparesAsked",
                    "firstRenewalAskSecondsAfterRefusal", "lastRenewalAskSecondsAfterRefusal", "heldSeconds",
                    "sourceRefreshPresentedByCodex", "sourceRefreshPresentedByHost", "hostActive",
                    "hostSourceAuth", "turnEndedAtLoginWall", "loginFilesUnchanged", "issuer",
                    "tokenRequests", "sourceUsageReadsByHost", "renewalAskGapsSeconds",
                    "unroutedRequests", "tlsHandshakesRefused")},
                    {"issuer": "fixture-tls", "tokenRequests": 1, "sourceUsageReadsByHost": 0,
                     "renewalAskGapsSeconds": [60.02], "unroutedRequests": 0, "tlsHandshakesRefused": 0,
                     "refusal": word, "refusalStatus": 400 if word == "invalid_grant" else 401,
                     "vetoKept": VETO_WORDS[word],
                     "stoppedBy": "the host's policy prints no word" if case == "lost-login-no-word"
                        else "codex-alab's veto",
                     "policyPrintedCodexRotationLogin": host_moves, "policyLogins": "renew",
                     "codexLoginRotationLines": 0, "codexRotationLines": 0, "otherAccountUsageReads": 0,
                     "receipts": 0, "nativeActions": 0, "renewalPreparesAsked": 2,
                     "firstRenewalAskSecondsAfterRefusal": 0.04, "lastRenewalAskSecondsAfterRefusal": 60.06,
                     "heldSeconds": 71.0, "sourceRefreshPresentedByCodex": 1,
                     "sourceRefreshPresentedByHost": 0, "hostActive": "main", "hostSourceAuth": "ok",
                     "turnEndedAtLoginWall": True, "loginFilesUnchanged": True})

    def test_the_verdict_fails_each_wrong_outcome(self):
        from renewal_follow import LOGIN_NOT_HELD, LOGIN_WALL_HOLD_SECONDS
        self.own_path()
        self.veto("revoked")
        self.verdict()
        # The veto on record is the one this case's refusal leaves, and no other.
        vetoes = {
            "lost-login-reused": r"\(the source's token, as exhausted\): [0-9a-f]{12}: revoked",
            "lost-login-invalid-grant": r"\(the source's token, as other\): [0-9a-f]{12}: revoked",
            "lost-login-no-word": r"\(none\): [0-9a-f]{12}: revoked",
        }
        for case, message in vetoes.items():
            with self.subTest(case=case):
                with self.assertRaisesRegex(RuntimeError, "kept refusals are not the one this case's refusal "
                                            "leaves " + message):
                    self.verdict(case, policy=dict(self.POLICY, codexRotationLogin=case != "lost-login-no-word"))
        self.veto("revoked", token="fixture-refresh-new")
        with self.assertRaisesRegex(RuntimeError, "kept refusals are not the one this case's refusal leaves"):
            self.verdict()
        for path in (self.root / "codex-home" / "alab" / "login-refusals").iterdir():
            path.unlink()
        with self.assertRaisesRegex(RuntimeError, r"\(the source's token, as revoked\): none kept"):
            self.verdict()
        self.veto("revoked")
        # The provider's side is the case's own refusal.
        with self.assertRaisesRegex(RuntimeError, r"not answered the case's refusal \(401 refresh_token_reused\)"):
            self.verdict("lost-login-reused", events=self.events("invalid_grant", moved=False))
        # The policy the case is run under.
        for policy, said in (({"unread": "TimeoutError"}, "TimeoutError"), (None, "None")):
            with self.assertRaisesRegex(RuntimeError, "the host's policy was never read: " + said):
                self.verdict(policy=policy)
        with self.assertRaisesRegex(RuntimeError, "the host's policy prints no codex_rotation_login=1, which "
                                    "is not what this case is run under"):
            self.verdict(policy=dict(self.POLICY, codexRotationLogin=False))
        with self.assertRaisesRegex(RuntimeError, "the host's policy prints codex_rotation_login=1, which "
                                    "is not what this case is run under"):
            self.verdict("lost-login-no-word")
        with self.assertRaisesRegex(RuntimeError, "the host's policy says logins=never"):
            self.verdict(policy=dict(self.POLICY, logins="never"))
        # Anything of a move or an ask, by name.
        self.own_path(("login-rotation", "prepare", "OK codex-login-rotation state=not-needed", 11.2))
        with self.assertRaisesRegex(RuntimeError, "^" + LOGIN_NOT_HELD + "1 codex-login-rotation "
                                    r"line\(s\) sent, the first answered OK codex-login-rotation state=not-needed"):
            self.verdict()
        self.own_path()
        with self.assertRaisesRegex(RuntimeError, "^" + LOGIN_NOT_HELD + r"1 usage read\(s\) of the other "
                                    "account by host"):
            self.verdict(events=self.events("refresh_token_invalidated", moved=False, extra=[
                ("GET", "/api/codex/usage", 30, {"account": "target", "client": "host", "status": 200})]))
        # The host was asked nothing about a move, and does nothing a move's job does with
        # the refused login: it presents the token never, by any road.
        presented = lambda count: self.events("refresh_token_invalidated", moved=False, extra=[
            ("POST", "/oauth/token", 20 + step, {"account": None, "client": "host", "refreshOf": "source",
                                                 "status": 401}) for step in range(count)])
        for count in (1, 5):
            with self.assertRaisesRegex(RuntimeError, f"the host presented the source's refused refresh token "
                                        rf"{count} time\(s\), though it was asked nothing about a move"):
                self.verdict(events=presented(count))
        refused = {"method": "TLS", "path": "", "kind": "handshake-refused", "account": None, "client": None,
                   "monotonicUs": 20_000_000, "reason": "UNEXPECTED_EOF_WHILE_READING"}
        with self.assertRaisesRegex(RuntimeError, "a client would not complete the fixture's TLS handshake, "
                                    r"1 time\(s\), the first with UNEXPECTED_EOF_WHILE_READING"):
            self.verdict(events=sorted(self.events("refresh_token_invalidated", moved=False) + [refused],
                                       key=lambda event: event["monotonicUs"]))
        # A token request at a second listener is not this layout's.
        self.LISTENER = "issuer"
        with self.assertRaisesRegex(RuntimeError, r"did not arrive at this case's issuer \(fixture-tls\)"):
            self.verdict()
        self.LISTENER = None
        # It may look at the login once for each time it is asked to renew it, and no more.
        reads = lambda count: self.events("refresh_token_invalidated", moved=False, extra=[
            ("GET", "/api/codex/usage", 20 + step, {"client": "host", "status": 401}) for step in range(count)])
        self.assertEqual(self.verdict(events=reads(2))["sourceUsageReadsByHost"], 2)
        for count in (3, 49):
            with self.assertRaisesRegex(RuntimeError, f"the host read the source's usage {count} times, more "
                                        "than once for each of the 2 times it was asked about that login"):
                self.verdict(events=reads(count))
        # The wall's own path, on two looks, each answered waiting, over the whole hold.
        paths = {
            "on two looks: no codex-renewal prepare": [],
            "on two looks: waiting$": [("renewal", "prepare", "waiting", 11.04)],
            "on two looks: waiting, ready": [("renewal", "prepare", "waiting", 11.04),
                                             ("renewal", "prepare", "ready", 71.06)],
        }
        for message, rows in paths.items():
            with self.subTest(message=message):
                self.calls(*rows)
                with self.assertRaisesRegex(RuntimeError, "the login wall did not take its own path, the "
                                            "same account's renewal, " + message):
                    self.verdict()
        self.own_path()
        with self.assertRaisesRegex(RuntimeError, "the login wall was watched 69.0 s after the refusal, "
                                    f"not {LOGIN_WALL_HOLD_SECONDS} s"):
            self.verdict(held_us=80_000_000)
        # The turn stays ended at the wall; the pool and the login files are as they were.
        with self.assertRaisesRegex(RuntimeError, r"did not end once at the login wall and stay there: "
                                    r"\['unauthorized', None\]"):
            self.verdict(rollout=self.rollout())
        self.state(active="target")
        with self.assertRaisesRegex(RuntimeError, "the host's active account changed: target"):
            self.verdict()
        self.state()
        from pathlib import Path
        original = Path(self.OLD).read_text()
        Path(self.OLD).write_text("{}")
        with self.assertRaisesRegex(RuntimeError, "a fixture login file changed during the run: auth-old"):
            self.verdict()
        Path(self.OLD).write_text(original)
        self.verdict()


if __name__ == "__main__":
    unittest.main()
