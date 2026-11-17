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


def notice(message):
    return {"type": "item.completed", "item": {"type": "error",
            "message": weekly.RESET_NOTICE_PREFIX + message}}


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
    def public_case(self, transform=lambda messages: messages):
        fixture = weekly_tests.SuccessfulTurnEvidenceTests("test_one_exact_completed_answer_within_its_turn_passes")
        try:
            fixture.setUp()
            output = fixture.root / "notice-case"
            output.mkdir()

            def run(*args, **kwargs):
                messages = [inventory(), automatic(fixture.model.reset), application(), restoration()]
                fixture.events[1:1] = transform(messages)
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

    def test_selected_preservation_traces_require_only_the_true_policy_branch(self):
        for name in weekly.NOTICE_PRESERVE_CASES:
            with self.subTest(case=name), self.trace(name) as (model, _backend):
                events = [inventory(), preservation(model.reset)]
                weekly.check_reset_notice_events(name, events, model.requests)
                denied = [request["response"] for request in model.requests if request["path"].endswith("/usage")
                          and request["response"]["rate_limit"]["allowed"] is False]
                self.assertTrue(denied)
                self.assertEqual(denied[-1]["rate_limit"]["secondary_window"]["reset_after_seconds"], 24 * 3600)
                self.assertEqual(sum(request["path"].endswith("/consume") for request in model.requests), 0)
                negated = copy.deepcopy(events)
                negated[1]["item"]["message"] = negated[1]["item"]["message"].replace(
                    "preserving earned resets", "not preserving earned resets")
                for bad in (negated, [*events, automatic(model.reset)], [*events, restoration()],
                            [*events, application()], list(reversed(events)), [*events, events[0]]):
                    with self.subTest(notices=bad), self.assertRaisesRegex(RuntimeError, "printed reset notice"):
                        weekly.check_reset_notice_events(name, bad, model.requests)

    def test_selected_spending_traces_accept_real_canonical_actions_and_backed_duplicates(self):
        for name in weekly.NOTICE_SUCCESS_CASES:
            with self.subTest(case=name), self.trace(name) as (model, backend):
                backend("GET", "rate-limit-reset-credits")
                events = [inventory(), inventory(), automatic(model.reset), automatic(model.reset),
                          application(), restoration(), restoration()]
                weekly.check_reset_notice_events(name, events, model.requests)
                self.assertEqual(sum(request["path"].endswith("/consume") for request in model.requests), 1)
                if name.endswith("25h"):
                    before = next(request["at"] for request in model.requests if request["path"].endswith("/consume"))
                    readings = [request["response"] for request in model.requests
                                if request["path"].endswith("/usage") and request["at"] < before]
                    self.assertEqual(readings[-1]["rate_limit"]["secondary_window"]["reset_after_seconds"], 25 * 3600)

    def test_duplicate_inventory_can_share_one_observed_listing_and_ignored_warnings_remain_valid(self):
        with self.trace("preflight-weekly") as (model, _backend):
            events = [notice("Considered earned usage reset; checking available resets and expiry before stopping for the usage limit."),
                notice("Considered earned usage reset: available count and expiry unknown because authentication changed or is unavailable. No reset used."),
                notice("Considered earned usage reset: allowance already restored; no new reset needed, so available count and expiry were not queried."),
                notice("Considered earned usage reset: available count and expiry are not re-listed while reconciling an earlier reset. Retaining its operation key to avoid spending twice."),
                notice("Considered earned usage reset: available count and expiry unknown because the reset list could not be read. No new reset will be used until availability is confirmed."),
                inventory(), inventory(), automatic(model.reset), restoration()]
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
        tail = [automatic(1_900_000_000), restoration()]
        weekly.check_reset_notice_events("preflight-weekly", [inventory(), changed, *tail], [first, second])
        for events in ([changed, *tail], [inventory(), changed, inventory(), *tail]):
            with self.subTest(notices=events), self.assertRaisesRegex(RuntimeError, "printed reset notice"):
                weekly.check_reset_notice_events("preflight-weekly", events, [first, second])
        weekly.check_reset_notice_events("preflight-weekly", [inventory(), changed, inventory(), *tail], [first, second, first])

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

