# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Negative controls for the production-bound pre-Signal Stop oracle."""
import copy
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

    def test_the_case_is_declared_but_not_yet_in_the_matrix(self):
        import renewal_follow
        import renewal_matrix
        from renewal_follow import Model
        self.assertEqual(renewal_follow.CASE_BUDGETS["five-hour-wall-soon"], (270, 300))
        self.assertNotIn("five-hour-wall-soon", renewal_matrix.CASES)
        with self.assertRaisesRegex(RuntimeError, "unknown fixture wall"):
            Model(self.root, weekly_quota=True, wall="five-hour-far")
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


if __name__ == "__main__":
    unittest.main()
