# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Notice-only qualification through real backend fixtures and an inert public case."""
from contextlib import contextmanager
import copy
import datetime
import json
from pathlib import Path
import socket
import sys
import unittest
from unittest import mock


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import capacity_exec as capacity
import weekly_reset as weekly
import weekly_reset_test as weekly_tests


def notice(message, prefix=weekly.RESET_NOTICE_PREFIX):
    return {"type": "item.completed", "item": {"type": "error",
            "message": prefix + message}}


def inventory():
    return notice("Considered earned usage reset: 1 available; expirations: 2098-01-01T00:00:00+00:00.")


def automatic(reset):
    timestamp = datetime.datetime.fromtimestamp(reset, datetime.timezone.utc).isoformat()
    return notice("Using an earned usage reset automatically: the weekly allowance refreshes at "
                  + timestamp + ", more than 24 hours away. Choosing the earliest-expiring usable reset.")


def preservation(reset):
    timestamp = datetime.datetime.fromtimestamp(reset, datetime.timezone.utc).isoformat()
    return notice("Reset not used: the weekly allowance refreshes at " + timestamp
                  + " (within 24 hours); preserving earned resets.")


def application():
    return notice("Earned usage reset applied; checking that allowance is restored.")


def restoration():
    return notice("Usage allowance restored after the earned reset; continuing the original request.")


class ResetNoticeOracleTests(unittest.TestCase):
    @contextmanager
    def public_case(self, transform=lambda messages: messages, *, arrange_events=None):
        fixture = weekly_tests.SuccessfulTurnEvidenceTests("test_one_exact_completed_answer_within_its_turn_passes")
        try:
            fixture.setUp()
            output = fixture.root / "notice-case"
            output.mkdir()

            def run(*args, **kwargs):
                messages = [inventory(), automatic(fixture.model.reset), application(), restoration()]
                fixture.events[1:1] = transform(messages)
                if arrange_events is not None:
                    arrange_events(fixture.events)
                return fixture.run_binary(*args, **kwargs)

            with mock.patch.object(weekly, "serve", fixture.serve), mock.patch.object(weekly, "run", new=run), \
                    mock.patch.object(capacity.subprocess, "Popen", side_effect=AssertionError("child forbidden")) as popen, \
                    mock.patch.object(capacity.subprocess, "run", side_effect=AssertionError("child forbidden")) as process, \
                    mock.patch.object(socket, "socket", side_effect=AssertionError("socket forbidden")) as network:
                try:
                    yield fixture, output
                finally:
                    popen.assert_not_called()
                    process.assert_not_called()
                    network.assert_not_called()
        finally:
            fixture.doCleanups()

    def execute(self, output):
        return weekly.case(Path("/unexecuted-fixture/codex"), output, "preflight-weekly")

    def assert_notice_refused(self, transform):
        with self.public_case(transform) as (fixture, output):
            with self.assertRaisesRegex(RuntimeError, "printed reset notice"):
                self.execute(output)
            case = output / "preflight-weekly"
            self.assertFalse((case / "result.json").exists())
            self.assertTrue((case / "requests.json").is_file())
            consumes = [request for request in fixture.model.requests if request["path"].endswith("/consume")]
            self.assertEqual(len(consumes), 1)
            self.assertEqual(fixture.model.spent, ["earned-weekly-credit"])
            self.assertEqual(len(fixture.model.models), 2)

    @contextmanager
    def trace(self, name):
        authentication = {"authorizationPresent": True, "authorizationMatches": True,
                          "accountPresent": True, "accountMatches": True}

        def backend(method, endpoint, body=None):
            return model.reply(method, "/backend-api/wham/" + endpoint, body, authentication=authentication)

        with mock.patch.object(weekly.time, "time", return_value=1_800_000_000), \
                mock.patch.object(capacity.subprocess, "Popen", side_effect=AssertionError("child forbidden")) as popen, \
                mock.patch.object(capacity.subprocess, "run", side_effect=AssertionError("child forbidden")) as process, \
                mock.patch.object(socket, "socket", side_effect=AssertionError("socket forbidden")) as network:
            model = weekly.Mock(name)
            try:
                backend("GET", "usage")
                if name not in weekly.PREFLIGHT_CASES:
                    inputs = [{"type": "message", "role": "user", "content": [
                        {"type": "input_text", "text": weekly.PROMPT}]}]
                    model.reply("POST", "/v1/responses", {"model": weekly.MODEL, "input": inputs})
                    model.reply("POST", "/v1/responses", {"model": weekly.MODEL, "input": inputs + [
                        {"type": "function_call_output", "call_id": weekly.CALL_ID, "output": "Plan updated"}]})
                    backend("GET", "usage")
                if name != "spend-control-blocked":
                    backend("GET", "rate-limit-reset-credits")
                if name in weekly.NOTICE_SUCCESS_CASES:
                    backend("GET", "usage")
                    backend("POST", "rate-limit-reset-credits/consume", {
                        "redeem_request_id": "a" * 64, "credit_id": "earned-weekly-credit"})
                    backend("GET", "usage")
                yield model, backend
            finally:
                popen.assert_not_called()
                process.assert_not_called()
                network.assert_not_called()

    def test_actual_canonical_public_case_keeps_its_valid_redemption_and_report(self):
        with self.public_case() as (fixture, output):
            result = self.execute(output)
            self.assertTrue(result["passed"])
            self.assertEqual(result["consumeRequests"], 1)
            self.assertEqual(result["resetNotices"], 4)
            saved = json.loads((output / "preflight-weekly/result.json").read_text())
            self.assertTrue(saved["passed"])
            self.assertEqual(fixture.model.spent, ["earned-weekly-credit"])

    def test_public_case_rejects_negated_or_opposite_decisions(self):
        for text in (
                "Reset not used automatically: the weekly allowance refreshes within 24 hours; preserving earned resets.",
                "Using an earned usage reset automatically: no reset was used; the weekly allowance refreshes more than 24 hours away.",
                "Using an earned usage reset automatically: the weekly allowance refreshes more than 24 hours away. No reset was used."):
            with self.subTest(message=text):
                self.assert_notice_refused(lambda messages: [messages[0], notice(text), *messages[2:]])
        self.assert_notice_refused(lambda messages: [*messages[:2], preservation(1_900_000_000), *messages[2:]])

    def test_public_case_rejects_extra_expiries_and_stale_or_malformed_inventory(self):
        for text in (
                "Considered earned usage reset: 1 available; expirations: 2098-01-01T00:00:00+00:00, 2099-01-01T00:00:00+00:00.",
                "Considered earned usage reset: 900 available; expirations: 2099-01-01T00:00:00+00:00.",
                "Considered earned usage reset: 900 available, expires 2099-01-01T00:00:00+00:00.",
                "Considered earned usage reset: 900.1 available; expirations: 2098-01-01T00:00:00+00:00.",
                "Considered earned usage reset: -1 available; expirations: 2098-01-01T00:00:00+00:00.",
                "Considered earned usage reset: count unavailable; expirations: 2098-01-01T00:00:00+00:00.",
                "Considered earned usage reset:1 available; expirations: 2098-01-01T00:00:00+00:00.",
                "Considered earned usage reset: available count and expiry 900 available; expirations: 2099-01-01T00:00:00+00:00.",
                "Considered earned usage reset: allowance already restored; 900 available; expirations: 2099-01-01T00:00:00+00:00."):
            with self.subTest(message=text):
                self.assert_notice_refused(lambda messages: [messages[0], notice(text), *messages[1:]])

    def test_each_earned_reset_notice_belongs_to_the_original_turn_before_its_answer(self):
        for boundary in ("before turn", "after answer", "after turn"):
            def move_notice(events):
                if boundary == "before turn":
                    events.insert(0, events.pop(events.index(inventory())))
                else:
                    restored = events.pop(events.index(restoration()))
                    answer = next(i for i, event in enumerate(events)
                                  if event.get("item", {}).get("type") == "agent_message")
                    events.insert(answer + 1 if boundary == "after answer" else len(events), restored)

            with self.subTest(boundary=boundary), self.public_case(arrange_events=move_notice) as (_, output):
                with self.assertRaisesRegex(RuntimeError, "before the recovered answer"):
                    self.execute(output)

    def test_public_case_rejects_every_out_of_order_stage_not_just_one_good_chain(self):
        for transform in (
                lambda messages: list(reversed(messages)),
                lambda messages: [messages[-1], *messages],
                lambda messages: [*messages, messages[0]],
                lambda messages: [*messages, messages[1]],
                lambda messages: [*messages, messages[2]]):
            with self.subTest(transform=transform):
                self.assert_notice_refused(transform)

    def test_public_case_still_requires_each_existing_notice_stage(self):
        for omitted in (0, 1, 3):
            with self.subTest(omitted=omitted):
                self.assert_notice_refused(lambda messages: messages[:omitted] + messages[omitted + 1:])

    def test_confirmed_spending_block_is_reported_once_during_recovery(self):
        name = "spend-control-blocked"
        plan = {"type": "item.started", "item": {"type": "todo_list"}}
        blocked = notice("Credit retry skipped. The account spending limit was reached.", weekly.CREDIT_NOTICE_PREFIX)
        wait = {"type": "error", "message": "Waiting for account quota (original request)"}
        with self.trace(name) as (model, backend):
            def check(events):
                weekly.check_reset_notice_events(name, events, model.requests)

            check([plan, blocked, wait])
            with mock.patch.dict(model.requests[-1]["response"], user_id="another-user"), self.assertRaisesRegex(
                    RuntimeError, "fixture account and user"):
                check([plan, blocked, wait])
            for events in ([plan, wait], [blocked, plan, wait], [plan, wait, blocked], [plan, blocked, blocked, wait],
                           [plan, notice("Reset recovery stopped."), wait]):
                with self.subTest(events=events), self.assertRaises(RuntimeError):
                    check(events)
            with mock.patch.dict(model.requests[-1]["response"]["spend_control"], reached=False), self.assertRaisesRegex(
                    RuntimeError, "block must be confirmed"):
                check([plan, blocked, wait])
            backend("GET", "rate-limit-reset-credits")
            with self.assertRaisesRegex(RuntimeError, "earned-reset request"):
                check([plan, blocked, wait])

    def test_decision_names_the_provider_weekly_refresh_for_that_inventory(self):
        for name, decision in [("recovery", automatic), ("recovery-weekly-24h", preservation)]:
            with self.subTest(case=name), self.trace(name) as (model, _):
                tail = [restoration()] if decision is automatic else []
                good = decision(model.reset)["item"]["message"].removeprefix(weekly.RESET_NOTICE_PREFIX)
                stamp = datetime.datetime.fromtimestamp(model.reset, datetime.timezone.utc).isoformat()
                missing = good.replace(f"at {stamp}, ", "").replace(f"at {stamp} (within 24 hours)", "within 24 hours")
                for bad in (decision(model.five_hour_reset), notice(good.replace(stamp, "tomorrow")), notice(missing)):
                    with self.subTest(message=bad), self.assertRaisesRegex(RuntimeError, "printed reset notice"):
                        weekly.check_reset_notice_events(name, [inventory(), bad, *tail], model.requests)

    def test_later_usage_cannot_supply_the_time_for_an_earlier_inventory(self):
        first, later = 1_900_000_000, 1_900_003_600
        current = weekly_tests.weekly_usage_read(first)
        windows = current["response"]["rate_limit"]
        windows["primary_window"] = windows.pop("secondary_window")
        windows["secondary_window"] = {"limit_window_seconds": 18000, "used_percent": 100, "reset_at": later}
        listing = {"path": "/backend-api/wham/rate-limit-reset-credits", "response": {
            "available_count": 1, "credits": [{"status": "available", "reset_type": "codex_rate_limits",
                                              "expires_at": "2098-01-01T00:00:00Z"}]}}
        newer = weekly_tests.weekly_usage_read(later)
        requests = [current, listing, newer]
        def check(deadline):
            weekly.check_reset_notice_events("preflight-weekly", [inventory(), automatic(deadline), restoration()],
                                            requests)
        check(first)
        with self.assertRaisesRegex(RuntimeError, "printed reset notice"):
            check(later)
        windows["primary_window"]["reset_at"] = 10**60
        with self.assertRaisesRegex(RuntimeError, "invalid provider weekly refresh"):
            check(first)
        requests.pop(0)
        with self.assertRaisesRegex(RuntimeError, "no matching weekly"):
            check(later)

    def test_selected_preservation_traces_require_only_the_true_policy_branch(self):
        for name in weekly.NOTICE_PRESERVE_CASES:
            with self.subTest(case=name), self.trace(name) as (model, _backend):
                events = [inventory(), preservation(model.reset)]
                weekly.check_reset_notice_events(name, events, model.requests)
                denied = [request["response"] for request in model.requests if request["path"].endswith("/usage")
                          and request["response"]["rate_limit"]["allowed"] is False]
                self.assertTrue(denied)
                hours = int(name.rsplit("-", 1)[1][:-1])
                self.assertEqual(hours, 24)
                self.assertEqual(denied[-1]["rate_limit"]["secondary_window"]["reset_after_seconds"], hours * 3600)
                self.assertEqual(sum(request["path"].endswith("/consume") for request in model.requests), 0)
                negated = copy.deepcopy(events)
                negated[1]["item"]["message"] = negated[1]["item"]["message"].replace(
                    "preserving earned resets", "not preserving earned resets")
                for bad in (negated, [*events, automatic(model.reset)], [*events, restoration()],
                            [*events, application()], [*events, events[1]], list(reversed(events)), [*events, events[0]]):
                    with self.subTest(notices=bad), self.assertRaisesRegex(RuntimeError, "printed reset notice"):
                        weekly.check_reset_notice_events(name, bad, model.requests)

    def test_repeated_reads_do_not_justify_repeated_spending_notices(self):
        for name in weekly.NOTICE_SUCCESS_CASES:
            with self.subTest(case=name), self.trace(name) as (model, backend):
                backend("GET", "rate-limit-reset-credits")
                events = [inventory(), automatic(model.reset), application(), restoration()]
                weekly.check_reset_notice_events(name, events, model.requests)
                for index, event in enumerate(events):
                    repeated = events[:index + 1] + [event] + events[index + 1:]
                    with self.subTest(stage=index), self.assertRaisesRegex(RuntimeError, "printed reset notice"):
                        weekly.check_reset_notice_events(name, repeated, model.requests)
                self.assertEqual(sum(request["path"].endswith("/consume") for request in model.requests), 1)
                if name.endswith(("25h", "48h", "49h")):
                    hours = int(name.rsplit("-", 1)[1][:-1])
                    before = next(request["at"] for request in model.requests if request["path"].endswith("/consume"))
                    readings = [request["response"] for request in model.requests
                                if request["path"].endswith("/usage") and request["at"] < before]
                    self.assertEqual(readings[-1]["rate_limit"]["secondary_window"]["reset_after_seconds"], hours * 3600)

    def test_informational_notices_and_unrelated_warnings_remain_valid(self):
        with self.trace("preflight-weekly") as (model, _backend):
            events = [notice("Considered earned usage reset; checking available resets and expiry before stopping for the usage limit."),
                notice("Considered earned usage reset: available count and expiry unknown because authentication changed or is unavailable. No reset used."),
                notice("Considered earned usage reset: allowance already restored; no new reset needed, so available count and expiry were not queried."),
                notice("Considered earned usage reset: available count and expiry are not re-listed while reconciling an earlier reset. Retaining its operation key to avoid spending twice."),
                notice("Considered earned usage reset: available count and expiry unknown because the reset list could not be read. No new reset will be used until availability is confirmed."),
                inventory(), automatic(model.reset), restoration()]
            events.insert(0, {"type": "item.completed", "item": {"type": "error", "message": "unrelated warning"}})
            events.insert(1, {"type": "item.completed", "item": {"type": "command_execution", "output": "retained tool output"}})
            weekly.check_reset_notice_events("preflight-weekly", events, model.requests)
            self.assertEqual(sum(request["path"].endswith("/rate-limit-reset-credits") for request in model.requests), 1)

    def test_inventory_sequence_cannot_skip_initial_or_replay_an_older_snapshot(self):
        first = {"path": "/backend-api/wham/rate-limit-reset-credits", "response": {
            "available_count": 1, "credits": [{"status": "available", "reset_type": "codex_rate_limits",
            "expires_at": "2098-01-01T00:00:00Z"}]}}
        second = {"path": first["path"], "response": {"available_count": 1, "credits": [
            {"status": "available", "reset_type": "codex_rate_limits", "expires_at": "2099-01-01T00:00:00Z"}]}}
        changed = notice("Considered earned usage reset: 1 available; expirations: 2099-01-01T00:00:00+00:00.")
        usage = weekly_tests.weekly_usage_read(1_900_000_000)
        tail = [automatic(1_900_000_000), restoration()]
        weekly.check_reset_notice_events("preflight-weekly", [inventory(), changed, *tail], [usage, first, second])
        for events in ([changed, *tail], [inventory(), changed, inventory(), *tail]):
            with self.subTest(notices=events), self.assertRaisesRegex(RuntimeError, "printed reset notice"):
                weekly.check_reset_notice_events("preflight-weekly", events, [usage, first, second])
        weekly.check_reset_notice_events("preflight-weekly", [inventory(), changed, inventory(), *tail], [usage, first, second, first])

    def test_equivalent_reset_inventories_use_sorted_utc_expiries_and_preserve_precision(self):
        dates = ["2098-01-01T00:10:00.123456789Z", None, "bad date", "2098-01-01T02:00:00.12+02:00",
                 "2097-12-31T23:00:00.120000000-01:00", "2098-01-01T00:00:00.000001Z"]
        normalized = ["2098-01-01T00:00:00.000001+00:00", "2098-01-01T00:00:00.120+00:00",
                      "2098-01-01T00:00:00.120+00:00", "2098-01-01T00:10:00.123456789+00:00",
                      "expiry not provided", "unknown expiry (invalid server timestamp)"]
        def listing(expiries):
            return {"path": "/backend-api/wham/rate-limit-reset-credits", "response": {
                "available_count": len(expiries), "credits": [{"status": "available", "reset_type": "codex_rate_limits",
                "expires_at": expiry} for expiry in expiries]}}
        def events(expiries):
            message = "Considered earned usage reset: 6 available; expirations: " + ", ".join(expiries) + "."
            return [notice(message), automatic(1_900_000_000), restoration()]

        usage = weekly_tests.weekly_usage_read(1_900_000_000)
        for rows in (dates, dates[::-1]):
            weekly.check_reset_notice_events("preflight-weekly", events(normalized), [usage, listing(rows)])
        changed = dates.copy()
        changed[0] = "2098-01-01T00:10:00.123456788Z"
        with self.assertRaisesRegex(RuntimeError, "printed reset notice"):
            weekly.check_reset_notice_events("preflight-weekly", events(normalized), [usage, listing(changed)])

    def test_repeated_empty_checks_are_quiet_in_the_exec_transcript(self):
        with self.trace("no-credit") as (model, backend):
            with self.assertRaisesRegex(RuntimeError, "repeated empty"):
                weekly.check_reset_notice_events("no-credit", [], model.requests)
            backend("GET", "rate-limit-reset-credits")
            native_wait = {"type": "item.completed", "item": {
                "type": "error", "message": "Waiting for account quota"}}
            weekly.check_reset_notice_events("no-credit", [native_wait], model.requests)
            for text in (
                "Considered earned usage reset: 0 available; expirations: none provided.",
                "Reset not used: no usable, unexpired earned reset is available; waiting for allowance or another account.",
            ):
                with self.subTest(message=text), self.assertRaisesRegex(RuntimeError, "printed a reset notice"):
                    weekly.check_reset_notice_events("no-credit", [native_wait, notice(text)], model.requests)
            model.credit = True
            backend("GET", "rate-limit-reset-credits")
            with self.assertRaisesRegex(RuntimeError, "repeated empty"):
                weekly.check_reset_notice_events("no-credit", [], model.requests)

    def test_empty_resets_cannot_qualify_a_silent_wait_with_paid_credit_access(self):
        with self.trace("no-credit") as (model, backend):
            backend("GET", "rate-limit-reset-credits")
            wall = next(i for i, r in enumerate(model.requests) if "quotaResetUnix" in r)
            usage = next(r for r in model.requests[wall + 1:] if r["path"].endswith("/usage"))
            with mock.patch.dict(usage["response"], account_id="another-account"), self.assertRaisesRegex(
                    RuntimeError, "fixture account and user"):
                weekly.check_reset_notice_events("no-credit", [], model.requests)
            for credits in ({"has_credits": True, "unlimited": False, "balance": "25"},
                            {"has_credits": False, "unlimited": True, "balance": None},
                            {"has_credits": False, "unlimited": False, "balance": None}, None):
                with self.subTest(credits=credits), mock.patch.dict(usage["response"], credits=credits):
                    with self.assertRaisesRegex(RuntimeError, "paid credit"):
                        weekly.check_reset_notice_events("no-credit", [], model.requests)
            with mock.patch.dict(usage["response"]["rate_limit"], allowed=True), self.assertRaisesRegex(
                    RuntimeError, "plan allowance"):
                weekly.check_reset_notice_events("no-credit", [], model.requests)
            without_usage = [r for r in model.requests if r is not usage]
            for requests in (without_usage, without_usage + [usage]):
                with self.assertRaisesRegex(RuntimeError, "post-wall usage"):
                    weekly.check_reset_notice_events("no-credit", [], requests)

    def test_a_fatal_reset_error_fails_both_empty_and_paid_credit_checks(self):
        for prefix in (weekly.RESET_NOTICE_PREFIX, weekly.CREDIT_NOTICE_PREFIX):
            fatal = {"type": "error", "message": prefix + "failed"}
            for name in ("no-credit", *weekly.FUNDED_CASES):
                with self.subTest(case=name, prefix=prefix), self.assertRaisesRegex(RuntimeError, "fatal JSON error"):
                    weekly.check_reset_notice_events(name, [fatal], [])

    def test_existing_extraction_fatal_and_unselected_contracts_remain(self):
        with self.trace("preflight-weekly") as (model, _backend):
            events = [inventory(), automatic(model.reset), restoration()]
            wrong_shape = copy.deepcopy(events)
            wrong_shape[0]["type"] = "item.started"
            with self.assertRaisesRegex(RuntimeError, "printed reset notice"):
                weekly.check_reset_notice_events("preflight-weekly", wrong_shape, model.requests)
            fatal = [*events, {"type": "error", "message": weekly.RESET_NOTICE_PREFIX + "fatal"}]
            with self.assertRaisesRegex(RuntimeError, "fatal JSON error"):
                weekly.check_reset_notice_events("preflight-weekly", fatal, model.requests)
        weekly.check_reset_notice_events("unrelated-provider", [], [])

