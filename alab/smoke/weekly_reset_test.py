# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Coverage accounting must not turn subset or failed runs into full qualification."""
import contextlib
import hashlib
import http.client
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import Mock, patch

import weekly_reset
import capacity_exec


class SuccessfulTurnEvidenceTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        plan = {"id": "plan", "type": "todo_list",
                "items": [{"text": weekly_reset.PLAN, "completed": True}]}
        self.events = [{"type": "turn.started"},
                       {"type": "item.started", "item": plan},
                       {"type": "item.completed", "item": plan},
                       {"type": "item.completed", "item": {
                           "id": "answer", "type": "agent_message", "text": weekly_reset.ANSWER}},
                       {"type": "turn.completed"}]
        self.server = SimpleNamespace(server_port=1234, shutdown=Mock(), server_close=Mock())
        self.worker = SimpleNamespace(join=Mock())

    def serve(self, model):
        self.model = model
        return self.server, self.worker

    def run_binary(self, _executable, _arguments, root, _env, _label, *, interact=None):
        refused = self.model.name == "paid-retry-refused"
        self.assertEqual(interact is not None, refused)
        auth = {"authorizationPresent": True, "authorizationMatches": True,
                "accountPresent": True, "accountMatches": True}

        def backend(method, path, body=None):
            return self.model.reply(method, "/backend-api/wham/" + path, body, authentication=auth)

        inputs = [{"type": "message", "role": "user", "content": [
            {"type": "input_text", "text": weekly_reset.PROMPT}]}]
        first = {"model": weekly_reset.MODEL, "input": inputs}
        retained = {"model": weekly_reset.MODEL, "input": inputs + [
            {"type": "function_call_output", "call_id": weekly_reset.CALL_ID, "output": "Plan updated"}]}

        backend("GET", "usage")
        if self.model.name in weekly_reset.FUNDED_CASES:
            self.assertEqual(self.model.reply("POST", "/v1/responses", first)[0], 200)
            self.assertEqual(self.model.reply("POST", "/v1/responses", retained)[0], 429)
            backend("GET", "usage")
        backend("GET", "rate-limit-reset-credits")
        backend("GET", "usage")
        if self.model.name == "spend-control-reopens":
            # Replay the interval spent waiting after the blocked post-list reading.
            for request in self.model.requests:
                request["at"] -= 30
            backend("GET", "usage")
            backend("GET", "rate-limit-reset-credits")
            backend("GET", "usage")
        if self.model.name not in weekly_reset.FUNDED_CASES:
            backend("POST", "rate-limit-reset-credits/consume", {
                "redeem_request_id": "a" * 64, "credit_id": "earned-weekly-credit"})
            backend("GET", "usage")
            self.model.reply("POST", "/v1/responses", first)
        retry = self.model.reply("POST", "/v1/responses", retained)
        self.assertEqual(retry[0], 429 if refused else 200)
        if refused:
            backend("GET", "usage")
            backend("GET", "usage")
            self.model.requests[-1]["at"] += 30
            interact(SimpleNamespace(poll=lambda: None), None)
        code = 130 if refused else 0
        with (root / "exec-stdout.jsonl").open("x") as stream:
            for event in self.events:
                stream.write(json.dumps(event) + "\n")
        weekly_reset.write_json(root / "exec-guardian-result.json", {"returncode": code, "timed_out": False})
        return code

    def case(self, label, name="preflight-beyond-24h"):
        output = self.root / label
        output.mkdir()
        with patch.object(weekly_reset, "serve", self.serve), \
                patch.object(weekly_reset, "run", self.run_binary):
            return weekly_reset.case(Path("/unexecuted-fixture/codex"), output, name)

    def assert_refused(self, label):
        with self.assertRaises(RuntimeError):
            self.case(label)
        case = self.root / label / "preflight-beyond-24h"
        self.assertFalse((case / "result.json").exists())
        self.assertTrue((case / "requests.json").is_file())

    def test_one_exact_completed_answer_within_its_turn_passes(self):
        self.assertTrue(self.case("valid")["passed"])
        self.server.shutdown.assert_called_once_with()
        self.worker.join.assert_called_once_with()
        self.server.server_close.assert_called_once_with()

    def test_paid_retry_completes_the_turn_while_ordinary_allowance_stays_exhausted(self):
        self.events.insert(3, {"type": "item.completed", "item": {"type": "error",
            "message": "[codex-alab credits] Retrying this request once with usage credits (reported balance 30965.02000)."}})
        result = self.case("paid", "existing-usage-credits")
        self.assertEqual((result["modelRequests"], result["consumeRequests"], result["creditNotices"]), (3, 0, 1))

        message = self.events[3]["item"]["message"]
        with patch.dict(self.events[3]["item"], message=message.replace("[codex-alab credits]", "[codex-alab reset]")):
            with self.assertRaisesRegex(RuntimeError, "unexpected recovery notice label"):
                weekly_reset.check_reset_notice_events("existing-usage-credits", self.events, self.model.requests)
        for label, text in (("stale", message.replace("30965.02000", "30966.04065")),
                            ("generic", "[codex-alab credits] Retrying this request once with available usage credits.")):
            self.events[3]["item"]["message"] = text
            with self.subTest(label=label), self.assertRaisesRegex(RuntimeError, "authorizing balance"):
                self.case(label, "existing-usage-credits")
        self.events[3]["item"]["message"] = message

        read = weekly_reset.Mock.usage

        def changed_balance(model):
            reading = read(model)
            if model.credit_listings:
                reading["credits"]["has_credits"] = False
            return reading

        with patch.object(weekly_reset.Mock, "usage", changed_balance), self.assertRaisesRegex(
                RuntimeError, "credit retry requires a fresh balance"):
            self.case("revoked", "existing-usage-credits")

    def test_paid_retry_requires_the_returned_balance_to_belong_to_the_active_login(self):
        notice = {"type": "item.completed", "item": {"type": "error", "message":
            weekly_reset.CREDIT_NOTICE_PREFIX + "Retrying this request once with usage credits (reported balance 30965.02000)."}}
        self.events.insert(3, notice)
        native_usage = weekly_reset.Mock.usage
        for field in ("account_id", "user_id"):
            for owner in (None, "different"):
                def returned_usage(model):
                    response = native_usage(model)
                    if model.credit_listings:
                        response[field] = owner
                    return response
                label = f"mismatched-{field}-{owner}"
                with self.subTest(field=field, owner=owner), patch.object(weekly_reset.Mock, "usage", returned_usage):
                    with self.assertRaisesRegex(RuntimeError, "fixture account and user"):
                        self.case(label, "existing-usage-credits")
                    self.assertFalse((self.root / label / "existing-usage-credits" / "result.json").exists())

    def test_blocked_final_read_preserves_the_paid_retry_for_a_later_open_read(self):
        name = "spend-control-reopens"
        notice = {"type": "item.completed", "item": {"type": "error", "message":
            weekly_reset.CREDIT_NOTICE_PREFIX + "Retrying this request once with usage credits (reported balance 30965.02000)."}}
        self.events.insert(3, notice)
        result = self.case("reopened", name)
        self.assertEqual((result["modelRequests"], result["consumeRequests"], result["creditNotices"]), (3, 0, 1))

        requests = self.model.requests
        readings = [r for r in requests if r["path"].endswith("/usage") and r.get("afterCreditSelection")]
        blocked, reopened, authorized = readings
        with patch.dict(blocked["response"]["credits"], balance="0"), self.assertRaisesRegex(
                RuntimeError, "positive balance must remain unchanged"):
            weekly_reset.check_reset_notice_events(name, self.events, requests)
        with patch.dict(blocked["response"]["spend_control"], reached=False), self.assertRaisesRegex(
                RuntimeError, "observe the new spending block"):
            weekly_reset.check_reset_notice_events(name, self.events, requests)
        with patch.dict(reopened, at=blocked["at"] + 1), self.assertRaisesRegex(RuntimeError, "paced account poll"):
            weekly_reset.check_reset_notice_events(name, self.events, requests)
        with patch.dict(authorized["response"]["spend_control"], reached=True), self.assertRaisesRegex(
                RuntimeError, "blocked reading cannot authorize"):
            weekly_reset.check_reset_notice_events(name, self.events, requests)
        restrictions = [
            {"rate_limit_reached_type": {"type": "workspace_member_credits_depleted"}},
            {"rate_limit_reached_type": {"type": "workspace_owner_usage_limit_reached"}},
            {"spend_control": {"reached": False, "individual_limit": {"remaining_percent": 0}}},
            {"rate_limit_upsell": {}},
        ]
        for reading in (reopened, authorized):
            for restriction in restrictions:
                with self.subTest(stage=reading["at"], restriction=restriction), patch.dict(reading["response"], restriction):
                    with self.assertRaisesRegex(RuntimeError, "spending reopened|blocked reading"):
                        weekly_reset.check_reset_notice_events(name, self.events, requests)
        with patch.dict(authorized["response"]["spend_control"], individual_limit={"remaining_percent": 1}):
            weekly_reset.check_reset_notice_events(name, self.events, requests)
        with self.assertRaisesRegex(RuntimeError, "authorizing balance in one notice"):
            weekly_reset.check_reset_notice_events(name, self.events + [notice], requests)

    def test_refused_paid_retry_retains_one_notice_and_the_original_tool_work(self):
        name = "paid-retry-refused"
        notice = {"type": "item.completed", "item": {"type": "error", "message":
            weekly_reset.CREDIT_NOTICE_PREFIX + "Retrying this request once with usage credits (reported balance 30965.02000)."}}
        self.events[2:] = [notice, self.events[2]]
        result = self.case("refused", name)
        self.assertTrue(result["cancelled"])
        self.assertEqual((result["exit"], result["modelRequests"], result["consumeRequests"], result["creditNotices"]),
                         (130, 3, 0, 1))

        requests = self.model.requests
        retry = [r for r in requests if r["path"] == "/v1/responses"][-1]
        polls = [r for r in requests if r["at"] > retry["at"] and r["path"].endswith("/usage")]
        with self.assertRaisesRegex(RuntimeError, "second paced poll"):
            weekly_reset.check_reset_notice_events(name, self.events, requests[:-1])
        with patch.dict(polls[-1], at=polls[0]["at"] + 2), self.assertRaisesRegex(RuntimeError, "second paced poll"):
            weekly_reset.check_reset_notice_events(name, self.events, requests)
        middle = dict(polls[0], at=polls[0]["at"] + 15)
        with self.assertRaisesRegex(RuntimeError, "second paced poll"):
            weekly_reset.check_reset_notice_events(name, self.events, requests[:-1] + [middle, polls[-1]])
        with patch.dict(polls[-1]["response"]["credits"], has_credits=False), self.assertRaisesRegex(
                RuntimeError, "funded quota wall"):
            weekly_reset.check_reset_notice_events(name, self.events, requests)
        with self.assertRaisesRegex(RuntimeError, "paid retry must follow"):
            weekly_reset.check_reset_notice_events(name, self.events, requests + [retry])

        self.events.append(notice)
        with self.assertRaisesRegex(RuntimeError, "one notice"):
            self.case("duplicate-paid-notice", name)
        self.events.pop()
        self.events.remove(notice)
        self.events.insert(0, notice)
        with self.assertRaisesRegex(RuntimeError, "pending turn"):
            self.case("paid-notice-before-turn", name)

    def test_plan_renewal_with_credits_requires_ordinary_notice_and_fresh_allowance(self):
        name = "plan-renewed-before-credit-retry"
        event = {"type": "item.completed", "item": {"type": "error",
            "message": "[codex-alab reset] Allowance restored; continuing the original request without spending another reset."}}
        self.events.insert(3, event)
        result = self.case("renewed", name)
        self.assertEqual((result["modelRequests"], result["consumeRequests"], result["resetNotices"]), (3, 0, 1))

        read = weekly_reset.Mock.usage

        def remains_exhausted(model):
            reading = read(model)
            reading["rate_limit"]["allowed"] = False
            return reading

        with patch.object(weekly_reset.Mock, "usage", remains_exhausted), self.assertRaisesRegex(
                RuntimeError, "restored usage before retry"):
            self.case("still-exhausted", name)

        reply = weekly_reset.Mock.reply

        def delayed_poll(model, method, path, body, **kwargs):
            after_listing = model.renewed_after_listing
            result = reply(model, method, path, body, **kwargs)
            if after_listing:
                model.requests[-1]["at"] += 30
            return result

        with patch.object(weekly_reset.Mock, "reply", delayed_poll), self.assertRaisesRegex(
                RuntimeError, "later quota poll"):
            self.case("delayed", name)

        event["item"]["message"] = "[codex-alab credits] Retrying this request once with usage credits (reported balance 30965.02000)."
        with self.assertRaisesRegex(RuntimeError, "before the recovered answer"):
            self.case("incorrect-paid-notice", name)

    def test_credit_retry_notice_reaches_the_original_turn_before_its_answer(self):
        cases = {"existing-usage-credits": "Retrying this request once with usage credits (reported balance 30965.02000).",
                 "plan-renewed-before-credit-retry":
                     "Allowance restored; continuing the original request without spending another reset."}
        original = self.events.copy()
        for name, message in cases.items():
            prefix = (weekly_reset.CREDIT_NOTICE_PREFIX if name == "existing-usage-credits"
                      else weekly_reset.RESET_NOTICE_PREFIX)
            notice = {"type": "item.completed", "item": {"type": "error", "message": prefix + message}}
            for index in (0, 4, 5):
                with self.subTest(case=name, index=index):
                    self.events = original[:index] + [notice] + original[index:]
                    with self.assertRaisesRegex(RuntimeError, "before the recovered answer"):
                        self.case(f"{name}-{index}", name)

    def test_duplicate_tool_output_with_the_same_id_cannot_pass(self):
        reply = weekly_reset.Mock.reply

        def duplicate(model, method, path, body, **kwargs):
            if path == "/v1/responses" and model.models:
                body["input"].append(body["input"][-1])
            return reply(model, method, path, body, **kwargs)

        with patch.object(weekly_reset.Mock, "reply", duplicate), self.assertRaisesRegex(
                RuntimeError, "completed plan outputs"):
            self.case("duplicate-output")

    def test_missing_wrong_or_duplicate_answer_cannot_pass(self):
        original = self.events[3]
        for change in ("missing", "wrong", "duplicate"):
            with self.subTest(change=change):
                self.events[3:-1] = [] if change == "missing" else [original]
                if change == "wrong":
                    self.events[3] = {"type": "item.completed", "item": {
                        "type": "agent_message", "text": "Unrelated answer."}}
                elif change == "duplicate":
                    self.events.insert(3, original)
                self.assert_refused(change)

    def test_started_or_non_message_answer_cannot_pass(self):
        for change in ("item.started", "error", "todo_list"):
            with self.subTest(change=change):
                self.events[3] = {"type": "item.started" if change == "item.started" else "item.completed",
                                  "item": {"type": "agent_message" if change == "item.started" else change,
                                           "text": weekly_reset.ANSWER}}
                self.assert_refused(change)

    def test_answer_outside_its_turn_cannot_pass(self):
        answer = self.events.pop(3)
        for change in ("before-start", "after-end"):
            with self.subTest(change=change):
                self.events.insert(0 if change == "before-start" else len(self.events), answer)
                self.assert_refused(change)
                self.events.remove(answer)

    def test_reversed_turn_boundaries_cannot_pass(self):
        self.events[0], self.events[-1] = self.events[-1], self.events[0]
        self.assert_refused("reversed")

    def test_warning_items_and_completed_tool_work_remain_valid(self):
        self.events.insert(3, {"type": "item.completed", "item": {
            "type": "error", "message": "Waiting for account quota"}})
        self.assertTrue(self.case("warning")["passed"])


class WeeklyResetCleanupTests(unittest.TestCase):
    @contextlib.contextmanager
    def fixture(self, *, body_error=None, failures=None, main=False, summary_failure=None):
        evidence = SuccessfulTurnEvidenceTests()
        evidence.setUp()
        try:
            output = evidence.root / "results"
            if not main:
                output.mkdir()
            binary = evidence.root / "codex"
            binary.write_bytes(b"unexecuted fixture")
            calls = []
            saved = []
            failures = failures or {}
            fixture = SimpleNamespace(output=output, binary=binary, calls=calls,
                                      evidence=evidence, saved=saved)

            def cleanup(phase):
                def action():
                    calls.append(phase)
                    if phase in failures:
                        raise failures[phase]
                return action

            evidence.server.shutdown.side_effect = cleanup("shutdown")
            evidence.worker.join.side_effect = cleanup("join")
            evidence.server.server_close.side_effect = cleanup("close")
            save = weekly_reset.write_json

            def write_json(path, value):
                if path.name == "requests.json":
                    cleanup("write")()
                elif path == output / "result.json" and summary_failure == "write":
                    raise OSError("fixture summary write denied")
                elif path.name == "result.json":
                    saved.append((path, value.copy(), list(calls)))
                save(path, value)

            def run(*args, **kwargs):
                result = evidence.run_binary(*args, **kwargs)
                if body_error is not None:
                    raise body_error
                return result

            digest = weekly_reset.file_sha256

            def file_sha256(path):
                if path == Path(weekly_reset.__file__) and summary_failure == "hash":
                    raise OSError("fixture probe hash denied")
                return digest(path)

            with patch.object(weekly_reset, "serve", evidence.serve), \
                    patch.object(weekly_reset, "run", side_effect=run) as execute, \
                    patch.object(weekly_reset, "write_json", side_effect=write_json), \
                    patch.object(weekly_reset, "file_sha256", side_effect=file_sha256), \
                    patch("builtins.print", side_effect=(
                        BrokenPipeError("fixture summary pipe closed") if summary_failure == "print" else None)):
                fixture.execute = execute
                yield fixture
        finally:
            evidence.doCleanups()

    def case(self, fixture):
        return weekly_reset.case(fixture.binary, fixture.output, "preflight-beyond-24h")

    def main(self, fixture):
        with patch.object(sys, "argv", [str(weekly_reset.__file__), "--codex", str(fixture.binary),
                                       "--output", str(fixture.output), "--case", "preflight-beyond-24h",
                                       "--case", "preflight-weekly-49h"]):
            return weekly_reset.main()

    def capture(self, action):
        try:
            return None, action()
        except BaseException as error:
            return error, None

    def assert_cleanup(self, fixture, *, shutdown_failed=False, write_failed=False):
        expected = ["shutdown", "close", "write"] if shutdown_failed else ["shutdown", "join", "close", "write"]
        self.assertEqual(fixture.calls, expected)
        trace = fixture.output / "preflight-beyond-24h/requests.json"
        if write_failed:
            self.assertFalse(trace.exists())
        else:
            self.assertEqual(json.loads(trace.read_text()), fixture.evidence.model.requests)

    def assert_diagnostics(self, error, *tokens):
        fields = capacity_exec.case_error_fields(error)
        details = fields.get("errorNotes", fields.get("cleanupErrors", []))
        text = "\n".join(details)
        for token in tokens:
            self.assertIn(token, text)
        return fields

    def test_normal_cleanup_preserves_the_full_recovery_oracle(self):
        with self.fixture() as fixture:
            result = self.case(fixture)
            self.assertTrue(result["passed"])
            self.assertEqual((result["modelRequests"], result["consumeRequests"]), (2, 1))
            self.assert_cleanup(fixture)
            self.assertEqual(fixture.saved, [(fixture.output / "preflight-beyond-24h/result.json",
                                              result, fixture.calls)])

    def test_each_cleanup_failure_fails_the_case_and_attempts_later_safe_actions(self):
        for phase in ("shutdown", "join", "close", "write"):
            with self.subTest(phase=phase), self.fixture(failures={phase: OSError("cleanup " + phase)}) as fixture:
                caught, result = self.capture(lambda: self.case(fixture))
                self.assertIsNone(caught)
                self.assertFalse(result["passed"])
                self.assertIn("cleanup " + phase, json.dumps(result["cleanupErrors"]))
                self.assertEqual(json.loads((fixture.output / "preflight-beyond-24h/result.json").read_text()), result)
                self.assert_cleanup(fixture, shutdown_failed=phase == "shutdown", write_failed=phase == "write")
                self.assertFalse(any(value["passed"] for _path, value, _calls in fixture.saved))

    def test_body_errors_keep_identity_and_all_cleanup_evidence(self):
        for make_error in (lambda: RuntimeError("body broke"), lambda: KeyboardInterrupt("body stopped"),
                           lambda: SystemExit(130), lambda: SystemExit(0)):
            for phase in ("shutdown", "join", "close", "write"):
                primary = make_error()
                with self.subTest(body=repr(primary), phase=phase), \
                        self.fixture(body_error=primary, failures={phase: OSError("cleanup " + phase)}) as fixture:
                    caught, _ = self.capture(lambda: self.case(fixture))
                    self.assertIs(caught, primary)
                    self.assert_diagnostics(caught, phase, "OSError", "cleanup " + phase)
                    self.assert_cleanup(fixture, shutdown_failed=phase == "shutdown", write_failed=phase == "write")
                    self.assertFalse((fixture.output / "preflight-beyond-24h/result.json").exists())

    def test_simultaneous_cleanup_failures_do_not_replace_cancellation(self):
        stop = KeyboardInterrupt("body stopped")
        failures = {phase: OSError("cleanup " + phase) for phase in ("shutdown", "close", "write")}
        with self.fixture(body_error=stop, failures=failures) as fixture:
            caught, _ = self.capture(lambda: self.case(fixture))
            self.assertIs(caught, stop)
            self.assert_diagnostics(caught, "shutdown", "join", "skipped", "close", "write")
            self.assert_cleanup(fixture, shutdown_failed=True, write_failed=True)

    def test_cleanup_cancellation_wins_except_over_existing_body_cancellation(self):
        for make_body in (lambda: None, lambda: RuntimeError("body broke"),
                          lambda: KeyboardInterrupt("body stopped"), lambda: SystemExit(0)):
            for make_stop in (lambda: KeyboardInterrupt("cleanup stopped"), lambda: SystemExit(130), lambda: SystemExit(0)):
                primary, stop = make_body(), make_stop()
                expected = primary if isinstance(primary, (KeyboardInterrupt, SystemExit)) else stop
                with self.subTest(body=repr(primary), stop=repr(stop)), \
                        self.fixture(body_error=primary, failures={"close": stop}) as fixture:
                    caught, _ = self.capture(lambda: self.case(fixture))
                    self.assertIs(caught, expected)
                    self.assert_diagnostics(caught, "close", type(stop).__name__)
                    if isinstance(primary, RuntimeError):
                        self.assert_diagnostics(caught, "case body", "body broke")
                    self.assert_cleanup(fixture)

    def test_main_retains_failed_cleanup_and_stops_before_the_next_case(self):
        with self.fixture(failures={"close": OSError("cleanup close")}, main=True) as fixture:
            self.assertEqual(self.main(fixture), 1)
            report = json.loads((fixture.output / "result.json").read_text())
            self.assertFalse(report["passed"])
            self.assertFalse(report["fullSuite"])
            self.assertEqual(report["failedCase"], "preflight-beyond-24h")
            self.assertEqual(len(report["cases"]), 1)
            self.assertFalse(report["cases"][0]["passed"])
            self.assertIn("cleanup close", json.dumps(report["cases"][0]["cleanupErrors"]))
            fixture.execute.assert_called_once()
            self.assert_cleanup(fixture)

    def test_main_retains_primary_error_and_secondary_cleanup_diagnostics(self):
        for make_error in (lambda: RuntimeError("body broke"), lambda: KeyboardInterrupt("body stopped"),
                           lambda: SystemExit(130), lambda: SystemExit(0)):
            for phase in ("shutdown", "close", "write"):
                primary = make_error()
                with self.subTest(body=repr(primary), phase=phase), \
                        self.fixture(body_error=primary, failures={phase: OSError("cleanup " + phase)}, main=True) as fixture:
                    caught, code = self.capture(lambda: self.main(fixture))
                    if isinstance(primary, Exception):
                        self.assertIsNone(caught)
                        self.assertEqual(code, 1)
                    else:
                        self.assertIs(caught, primary)
                    report = json.loads((fixture.output / "result.json").read_text())
                    self.assertFalse(report["passed"])
                    self.assertFalse(report["fullSuite"])
                    self.assertEqual(report["failedCase"], "preflight-beyond-24h")
                    self.assertEqual(report["cases"], [])
                    for key, value in self.assert_diagnostics(primary, phase, "OSError").items():
                        self.assertEqual(report[key], value)
                    if not isinstance(primary, Exception):
                        self.assertTrue(report["interrupted"])
                    fixture.execute.assert_called_once()
                    self.assert_cleanup(fixture, shutdown_failed=phase == "shutdown", write_failed=phase == "write")

    def test_summary_failure_preserves_cancellation_and_cleanup_diagnostics(self):
        for make_stop in (lambda: KeyboardInterrupt("body stopped"), lambda: SystemExit(130), lambda: SystemExit(0)):
            for failure in ("hash", "write", "print"):
                stop = make_stop()
                with self.subTest(stop=repr(stop), failure=failure), \
                        self.fixture(body_error=stop, failures={"close": OSError("cleanup close")},
                                     main=True, summary_failure=failure) as fixture:
                    caught, _ = self.capture(lambda: self.main(fixture))
                    self.assertIs(caught, stop)
                    self.assertIsInstance(stop.__context__, OSError)
                    self.assert_diagnostics(stop, "cleanup close", "OSError")
                    self.assert_cleanup(fixture)
                    summary = fixture.output / "result.json"
                    if failure == "print":
                        self.assertTrue(json.loads(summary.read_text())["interrupted"])
                    else:
                        self.assertFalse(summary.exists())


class CoverageReportTests(unittest.TestCase):
    def run_probe(self, root, selected, fail=None, interruption=None):
        executable = root / "fixture-codex"
        executable.write_bytes(b"fixture binary; never executed")
        output = root / "report"
        args = ["weekly_reset.py", "--codex", str(executable), "--output", str(output)]
        for name in selected:
            args.extend(["--case", name])

        def run_case(_executable, _output, name):
            if name == fail:
                if interruption is not None:
                    raise interruption
                raise RuntimeError("fixture recovery assertion failed")
            return {"case": name, "passed": True}

        with patch.object(sys, "argv", args), patch.object(weekly_reset, "case", run_case), \
                contextlib.redirect_stdout(io.StringIO()):
            code = weekly_reset.main()
        return code, json.loads((output / "result.json").read_text())

    def test_subset_success_is_distinct_from_complete_qualification(self):
        with tempfile.TemporaryDirectory() as directory:
            code, report = self.run_probe(Path(directory), ["late-credit"])
        self.assertEqual(code, 0)
        self.assertTrue(report["passed"])
        self.assertFalse(report["fullSuite"])
        self.assertEqual(report["requestedCases"], ["late-credit"])
        self.assertNotIn("failedCase", report)

    def test_failure_retains_requested_coverage_and_completed_cases(self):
        with tempfile.TemporaryDirectory() as directory:
            code, report = self.run_probe(Path(directory), ["late-credit", "lost-ack"], "lost-ack")
        self.assertEqual(code, 1)
        self.assertFalse(report["passed"])
        self.assertFalse(report["fullSuite"])
        self.assertEqual(report["requestedCases"], ["late-credit", "lost-ack"])
        self.assertEqual([case["case"] for case in report["cases"]], ["late-credit"])
        self.assertEqual(report["failedCase"], "lost-ack")
        self.assertIn("fixture recovery assertion failed", report["error"])

    def test_default_run_accounts_for_every_declared_case(self):
        with tempfile.TemporaryDirectory() as directory:
            code, report = self.run_probe(Path(directory), [])
        self.assertEqual(code, 0)
        self.assertTrue(report["fullSuite"])
        self.assertEqual(report["requestedCases"], list(weekly_reset.CASE_NAMES))
        self.assertEqual([case["case"] for case in report["cases"]], report["requestedCases"])

    def test_cancellation_keeps_incomplete_evidence_and_the_original_exit(self):
        for interruption in [KeyboardInterrupt(), SystemExit(143)]:
            with self.subTest(interruption=type(interruption).__name__), \
                    tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                with self.assertRaises(type(interruption)) as caught:
                    self.run_probe(root, ["late-credit", "lost-ack"], "lost-ack", interruption)
                self.assertIs(caught.exception, interruption)
                report = json.loads((root / "report/result.json").read_text())
                self.assertTrue(report["interrupted"])
                self.assertFalse(report["passed"])
                self.assertFalse(report["fullSuite"])
                self.assertEqual(report["failedCase"], "lost-ack")
                self.assertEqual([case["case"] for case in report["cases"]], ["late-credit"])

    def test_report_failure_does_not_replace_the_original_cancellation(self):
        for kind in (KeyboardInterrupt, lambda: SystemExit(143), lambda: SystemExit(0)):
            for phase in ("hash", "write", "print"):
                interruption = kind()
                failure = (BrokenPipeError("fixture output closed") if phase == "print"
                           else OSError("fixture report unavailable"))
                with self.subTest(interruption=repr(interruption), phase=phase), \
                        tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    source_hash = weekly_reset.file_sha256

                    def hash_file(path):
                        if phase == "hash" and path == Path(weekly_reset.__file__):
                            raise failure
                        return source_hash(path)

                    write_effect = failure if phase == "write" else weekly_reset.write_json
                    print_effect = failure if phase == "print" else None
                    with patch.object(weekly_reset, "file_sha256", side_effect=hash_file), \
                            patch.object(weekly_reset, "write_json", side_effect=write_effect), \
                            patch("builtins.print", side_effect=print_effect), \
                            self.assertRaises(type(interruption)) as caught:
                        self.run_probe(root, ["late-credit", "lost-ack"], "lost-ack", interruption)
                    self.assertIs(caught.exception, interruption)
                    self.assertIs(interruption.__context__, failure)
                    if phase == "print":
                        report = json.loads((root / "report/result.json").read_text())
                        self.assertTrue(report["interrupted"])
                        self.assertFalse(report["passed"])
                        self.assertEqual(report["failedCase"], "lost-ack")

    def test_report_failure_without_cancellation_still_fails(self):
        for phase in ("write", "print"):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as directory:
                failure = OSError("fixture report unavailable")
                write_effect = failure if phase == "write" else weekly_reset.write_json
                print_effect = failure if phase == "print" else None
                with patch.object(weekly_reset, "write_json", side_effect=write_effect), \
                        patch("builtins.print", side_effect=print_effect), \
                        self.assertRaises(OSError) as caught:
                    self.run_probe(Path(directory), ["late-credit"])
                self.assertIs(caught.exception, failure)

    def test_duplicate_selection_is_rejected_before_creating_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as caught:
                self.run_probe(root, ["late-credit", "late-credit"])
            self.assertEqual(caught.exception.code, 2)
            self.assertFalse((root / "report").exists())


class BinaryIdentityReportTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.executable = self.root / "fixture-codex"
        self.executable.write_bytes(b"inert initial fixture; never executed")
        self.initial = hashlib.sha256(self.executable.read_bytes()).hexdigest()
        self.output = self.root / "report"

    def test_package_observation_must_match_before_a_credit_case_starts(self):
        source = "a" * 40
        package = self.root / "package"
        (package / "bin").mkdir(parents=True)
        self.executable.rename(package / "bin" / "codex")
        common = {"version": "test", "target": "test"}
        (package / "codex-package.json").write_text(json.dumps({**common, "entrypoint": "bin/codex"}))
        for name, recorded_source, built, packaged, expected in [
            ("match", source, self.initial, self.initial, True),
            ("different-binary", source, "b" * 64, "b" * 64, False),
            ("changed-during-packaging", source, "b" * 64, self.initial, False),
            ("different-source", "c" * 40, self.initial, self.initial, False),
            ("legacy", None, None, None, True),
        ]:
            with self.subTest(name=name):
                build = {**common, "source": source}
                if recorded_source is not None:
                    build["compiler"] = {"verification": {"source": recorded_source, "binaries": {
                        "codex": {"built_sha256": built, "packaged_sha256": packaged}}}}
                (package / "alab-build.json").write_text(json.dumps(build))
                args = SimpleNamespace(case=["no-credit"], package=package, output=self.root / name)
                with patch.object(weekly_reset, "case", return_value={"passed": True}) as run_case, \
                        contextlib.redirect_stdout(io.StringIO()):
                    code = weekly_reset.main(args)
                report = json.loads((args.output / "result.json").read_text())
                self.assertEqual(code, 0 if expected else 1)
                self.assertEqual(report["passed"], expected)
                self.assertEqual(report["binarySha256"], self.initial)
                self.assertEqual(run_case.call_count, int(expected))
                if not expected:
                    self.assertEqual(report["cases"], [])
                    self.assertNotIn("failedCase", report)

    def run_probe(self, action=lambda: None, selected=("late-credit", "lost-ack")):
        args = ["weekly_reset.py", "--codex", str(self.executable), "--output", str(self.output)]
        for name in selected or ():
            args.extend(["--case", name])
        names = list(selected or weekly_reset.CASE_NAMES)

        def run_case(_executable, _output, name):
            if name == names[-1]:
                action()
            return {"case": name, "passed": True}

        with patch.object(sys, "argv", args), patch.object(weekly_reset, "case", run_case), \
                contextlib.redirect_stdout(io.StringIO()):
            code = weekly_reset.main()
        return code, json.loads((self.output / "result.json").read_text())

    def test_unchanged_binary_keeps_a_successful_full_report(self):
        code, report = self.run_probe(selected=None)
        self.assertEqual(code, 0)
        self.assertTrue(report["passed"])
        self.assertTrue(report["fullSuite"])
        self.assertEqual(report["binarySha256"], self.initial)
        self.assertEqual([case["case"] for case in report["cases"]], list(weekly_reset.CASE_NAMES))

    def test_changed_binary_cannot_pass_under_its_original_digest(self):
        code, report = self.run_probe(lambda: self.executable.write_bytes(b"inert replacement fixture"))
        self.assertEqual(code, 1)
        self.assertFalse(report["passed"])
        self.assertFalse(report["fullSuite"])
        self.assertEqual(report["binarySha256"], self.initial)
        self.assertEqual(len(report["cases"]), 2)
        self.assertTrue(all(case["passed"] for case in report["cases"]))
        self.assertIn("executable changed", report["error"])
        self.assertNotIn("failedCase", report)

    def test_removed_binary_cannot_leave_a_passing_report(self):
        code, report = self.run_probe(self.executable.unlink)
        self.assertEqual(code, 1)
        self.assertFalse(report["passed"])
        self.assertEqual(report["binarySha256"], self.initial)
        self.assertEqual(len(report["cases"]), 2)
        self.assertIn("FileNotFoundError", report["error"])

    def test_fifo_replacement_refuses_without_waiting_for_a_writer(self):
        program = '''import os, pathlib, sys
smoke, executable, output = sys.argv[1:]
sys.path.insert(0, smoke)
import weekly_reset
import capacity_exec
def replace(executable, output, name):
    executable.unlink()
    os.mkfifo(executable)
    return {"case": name, "passed": True}
weekly_reset.case = replace
sys.argv = ["weekly_reset.py", "--codex", executable, "--output", output, "--case", "late-credit"]
raise SystemExit(weekly_reset.main())
'''
        child = subprocess.run([sys.executable, "-I", "-B", "-c", program,
                                str(Path(weekly_reset.__file__).parent),
                                str(self.executable), str(self.output)],
                               capture_output=True, text=True, timeout=5)
        self.assertEqual(child.returncode, 1, child.stderr)
        report = json.loads((self.output / "result.json").read_text())
        self.assertFalse(report["passed"])
        self.assertEqual(report["binarySha256"], self.initial)
        self.assertTrue(report["cases"][0]["passed"])
        self.assertIn("not a regular file", report["error"])

    def test_unreadable_final_identity_keeps_the_failed_summary(self):
        digest, reads = weekly_reset.file_sha256, 0

        def hash_file(path):
            nonlocal reads
            if path == self.executable:
                reads += 1
                if reads == 2:
                    raise PermissionError("fixture final binary read denied")
            return digest(path)

        with patch.object(weekly_reset, "file_sha256", side_effect=hash_file):
            code, report = self.run_probe()
        self.assertEqual(code, 1)
        self.assertFalse(report["passed"])
        self.assertEqual(report["binarySha256"], self.initial)
        self.assertEqual(len(report["cases"]), 2)
        self.assertIn("PermissionError: fixture final binary read denied", report["error"])

    def test_case_failure_retains_its_original_error_after_a_binary_change(self):
        def fail():
            self.executable.unlink()
            raise RuntimeError("fixture case failed")

        code, report = self.run_probe(fail)
        self.assertEqual(code, 1)
        self.assertFalse(report["passed"])
        self.assertEqual(report["error"], "RuntimeError: fixture case failed")
        self.assertEqual(report["failedCase"], "lost-ack")
        self.assertEqual(len(report["cases"]), 1)

    def test_cancellation_keeps_its_original_exit_after_a_binary_change(self):
        for index, interruption in enumerate((KeyboardInterrupt(), SystemExit(0), SystemExit(143))):
            with self.subTest(interruption=repr(interruption)):
                self.executable.write_bytes(b"inert initial fixture; never executed")
                self.output = self.root / ("cancel-" + str(index))

                def cancel():
                    self.executable.unlink()
                    raise interruption

                with self.assertRaises(type(interruption)) as caught:
                    self.run_probe(cancel)
                self.assertIs(caught.exception, interruption)
                report = json.loads((self.output / "result.json").read_text())
                self.assertFalse(report["passed"])
                self.assertTrue(report["interrupted"])
                self.assertEqual(report["binarySha256"], self.initial)
                self.assertEqual(report["failedCase"], "lost-ack")
                self.assertEqual(len(report["cases"]), 1)


class FileHashTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.path = self.root / "inert-file"
        self.path.write_bytes(b"a" * 100)

    def test_empty_and_large_regular_files_keep_their_streaming_digest(self):
        original_read = os.read
        for data in (b"", b"a" * (2 * 1024 * 1024) + b"tail"):
            with self.subTest(size=len(data)):
                self.path.write_bytes(data)
                sizes = []

                def read(descriptor, size):
                    sizes.append(size)
                    return original_read(descriptor, size)

                with patch.object(os, "read", side_effect=read):
                    self.assertEqual(weekly_reset.file_sha256(self.path), hashlib.sha256(data).hexdigest())
                self.assertTrue(sizes)
                self.assertLessEqual(max(sizes), 1024 * 1024)
                if data:
                    self.assertGreater(len(sizes), 2)

    def test_symlinks_and_directories_are_not_hashed(self):
        linked = self.root / "linked"
        linked.symlink_to(self.path)
        for path in (linked, self.root):
            with self.subTest(path=path), self.assertRaises((OSError, RuntimeError)):
                weekly_reset.file_sha256(path)

    def test_continuous_appends_are_refused_after_at_most_the_initial_size_plus_one(self):
        original_read = os.read
        initial = self.path.stat().st_size
        received, calls = 0, 0

        def append_at_read(descriptor, size):
            nonlocal received, calls
            calls += 1
            self.assertLessEqual(calls, 3, "hash reader kept following a growing file")
            chunk = original_read(descriptor, size)
            received += len(chunk)
            with self.path.open("ab") as stream:
                stream.write(b"x")
            return chunk

        with patch.object(os, "read", side_effect=append_at_read), \
                self.assertRaisesRegex(RuntimeError, "changed while hashing"):
            weekly_reset.file_sha256(self.path)
        self.assertEqual(received, initial + 1)
        self.assertEqual(calls, 2)

    def test_truncation_at_the_read_boundary_is_refused(self):
        original_read = os.read
        truncated = False

        def truncate_at_read(descriptor, size):
            nonlocal truncated
            if not truncated:
                self.path.write_bytes(b"short")
                truncated = True
            return original_read(descriptor, size)

        with patch.object(os, "read", side_effect=truncate_at_read), \
                self.assertRaisesRegex(RuntimeError, "changed while hashing"):
            weekly_reset.file_sha256(self.path)
        self.assertTrue(truncated)

    def test_file_mutation_during_read_is_refused_and_descriptor_is_closed(self):
        original_open, original_read = os.open, os.read
        opened = []

        def open_file(*args, **kwargs):
            descriptor = original_open(*args, **kwargs)
            opened.append(descriptor)
            return descriptor

        def read_file(descriptor, size):
            chunk = original_read(descriptor, size)
            if chunk:
                self.path.write_bytes(b"b" * 100)
            return chunk

        with patch.object(os, "open", side_effect=open_file), \
                patch.object(os, "read", side_effect=read_file), \
                self.assertRaisesRegex(RuntimeError, "changed while hashing"):
            weekly_reset.file_sha256(self.path)
        self.assertEqual(len(opened), 1)
        with self.assertRaises(OSError):
            os.fstat(opened[0])

    def test_read_failure_and_cancellation_close_the_descriptor_and_keep_the_original_error(self):
        for error in (OSError("fixture read failed"), KeyboardInterrupt(), SystemExit(0), SystemExit(143)):
            with self.subTest(error=repr(error)):
                original_open = os.open
                opened = []

                def open_file(*args, **kwargs):
                    descriptor = original_open(*args, **kwargs)
                    opened.append(descriptor)
                    return descriptor

                with patch.object(os, "open", side_effect=open_file), \
                        patch.object(os, "read", side_effect=error), \
                        self.assertRaises(type(error)) as caught:
                    weekly_reset.file_sha256(self.path)
                self.assertIs(caught.exception, error)
                self.assertEqual(len(opened), 1)
                with self.assertRaises(OSError):
                    os.fstat(opened[0])


class DeadlineOutageFixtureTests(unittest.TestCase):
    def test_clock_restoration_refuses_early_retry_and_keeps_the_backend_outage(self):
        for name in weekly_reset.DEADLINE_OUTAGE_CASES:
            with self.subTest(case=name), patch.object(weekly_reset.time, "time", return_value=1000):
                early, late = weekly_reset.Mock(name), weekly_reset.Mock(name)
                for mock in (early, late):
                    self.assertEqual(mock.reply("POST", "/v1/responses", {})[0], 200)
                    self.assertEqual(mock.reply("POST", "/v1/responses", {})[0], 429)
                    self.assertEqual(mock.reset, 1003)
                self.assertEqual(early.reply("POST", "/v1/responses", {})[0], 403)
                with patch.object(weekly_reset.time, "time", return_value=1003):
                    self.assertEqual(late.reply("POST", "/v1/responses", {})[0], 200)
                    endpoint = "usage" if name == "usage-outage-deadline" else "rate-limit-reset-credits"
                    self.assertEqual(late.reply("GET", "/backend-api/wham/" + endpoint, {})[0], 503)
                self.assertFalse(late.consumed)
                self.assertFalse(late.spent)


class PrePostFixtureTests(unittest.TestCase):
    def test_selected_credit_does_not_freeze_usage_eligibility(self):
        for name in weekly_reset.PREPOST_CASES:
            with self.subTest(case=name), patch.object(weekly_reset.time, "time", return_value=1000):
                mock = weekly_reset.Mock(name)
                mock.models = [{}, {}] if name != "prepost-preflight-99" else []
                before = mock.usage()
                self.assertEqual(before["account_id"], weekly_reset.ACCOUNT)
                self.assertEqual(mock.post_selection_reads, 0)
                mock.reply("GET", "/backend-api/wham/rate-limit-reset-credits", None)
                fresh = mock.usage()
                self.assertEqual(mock.post_selection_reads, 1)
                if name == "prepost-recovery-restored":
                    self.assertTrue(fresh["rate_limit"]["allowed"])
                    self.assertEqual(fresh["rate_limit"]["secondary_window"]["used_percent"], 99)
                    self.assertEqual(mock.usage()["rate_limit"]["secondary_window"]["used_percent"], 0)
                elif name == "prepost-recovery-shifted":
                    self.assertNotEqual(fresh["rate_limit"]["secondary_window"]["reset_at"],
                                        before["rate_limit"]["secondary_window"]["reset_at"])
                elif name == "prepost-recovery-foreign":
                    self.assertNotEqual(fresh["account_id"], before["account_id"])
                elif name == "prepost-recovery-below-99":
                    self.assertEqual(fresh["rate_limit"]["secondary_window"]["used_percent"], 98)
                else:
                    self.assertEqual(fresh, before)

    def test_a_post_before_the_fresh_observation_is_an_actual_fixture_error(self):
        for name in weekly_reset.PREPOST_CASES:
            with self.subTest(case=name), patch.object(weekly_reset.time, "time", return_value=1000):
                mock = weekly_reset.Mock(name)
                mock.models = [{}, {}]
                mock.reply("GET", "/backend-api/wham/usage", None)
                mock.reply("GET", "/backend-api/wham/rate-limit-reset-credits", None)
                with self.assertRaisesRegex(RuntimeError, "post-selection usage"):
                    mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                               {"redeem_request_id": "a" * 64, "credit_id": "earned-weekly-credit"})


class PropagationFixtureTests(unittest.TestCase):
    def test_immediate_policy_reads_cannot_advance_backend_propagation_time(self):
        for name in weekly_reset.PROPAGATION_CASES:
            with self.subTest(case=name), patch.object(weekly_reset.time, "time", return_value=1000), \
                    patch.object(weekly_reset.time, "monotonic", return_value=10) as clock:
                mock = weekly_reset.Mock(name)
                mock.models = [{}, {}]
                endpoint = "/backend-api/wham/usage"
                mock.reply("GET", endpoint, None)
                mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                           {"redeem_request_id": "a" * 64, "credit_id": "earned-weekly-credit"})
                for _ in range(8):
                    self.assertFalse(json.loads(mock.reply("GET", endpoint, None)[1])["rate_limit"]["allowed"])
                clock.return_value = 69.999
                self.assertFalse(json.loads(mock.reply("GET", endpoint, None)[1])["rate_limit"]["allowed"])
                clock.return_value = 70
                self.assertTrue(json.loads(mock.reply("GET", endpoint, None)[1])["rate_limit"]["allowed"])


class RedemptionPolicyFixtureTests(unittest.TestCase):
    def test_five_hour_cycle_survives_reads_across_a_second_boundary(self):
        with patch.object(weekly_reset.time, "time", return_value=1000) as clock:
            mock = weekly_reset.Mock("preflight-five-hour")
            first = json.loads(mock.reply("GET", "/backend-api/wham/usage", None)[1])
            clock.return_value = 1001
            second = json.loads(mock.reply("GET", "/backend-api/wham/usage", None)[1])
        first_window = first["rate_limit"]["primary_window"]
        second_window = second["rate_limit"]["primary_window"]
        self.assertEqual(first_window["used_percent"], 99)
        self.assertEqual(second_window["used_percent"], 99)
        self.assertEqual(first_window["reset_at"], 4600)
        self.assertEqual(second_window["reset_at"], first_window["reset_at"])
        self.assertEqual(first_window["reset_after_seconds"], 3600)
        self.assertEqual(second_window["reset_after_seconds"], 3599)

    def test_positive_preflights_offer_a_weekly_99_percent_window_beyond_24_hours(self):
        for name in weekly_reset.PREFLIGHT_CASES:
            with self.subTest(case=name), patch.object(weekly_reset.time, "time", return_value=1000):
                mock = weekly_reset.Mock(name)
                self.assertEqual(mock.reply("GET", "/backend-api/wham/usage", None)[0], 200)
                if name in weekly_reset.PREPOST_CASES:
                    mock.reply("GET", "/backend-api/wham/rate-limit-reset-credits", None)
                    mock.reply("GET", "/backend-api/wham/usage", None)
                self.assertEqual(mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                                           {"redeem_request_id": "a" * 64,
                                            "credit_id": "earned-weekly-credit"})[0], 200)
                self.assertTrue(mock.consumed)
                windows = mock.requests[0]["response"]["rate_limit"]
                self.assertEqual(windows["secondary_window"]["used_percent"], 99)
                self.assertGreater(windows["secondary_window"]["reset_at"] - 1000, 24 * 3600)

    def test_reactive_weekly_beyond_24h_consumes_its_exhausted_window(self):
        for original in ("preflight-mixed-windows", "recovery-elapsed-reset"):
            self.assertIn(original, weekly_reset.POLICY_POLL_CASES)
        for hours in (25, 48, 49):
            name = f"recovery-weekly-{hours}h"
            self.assertIn(name, weekly_reset.SUCCESS_CASES)
            self.assertNotIn(name, weekly_reset.POLICY_POLL_CASES)
            with self.subTest(hours=hours), patch.object(weekly_reset.time, "time", return_value=1000):
                mock = weekly_reset.Mock(name)
                self.assertTrue(mock.usage()["rate_limit"]["allowed"])
                mock.models = [{}, {}]
                self.assertEqual(mock.reply("GET", "/backend-api/wham/usage", None)[0], 200)
                windows = mock.requests[0]["response"]["rate_limit"]
                self.assertFalse(windows["allowed"])
                self.assertEqual(windows["primary_window"]["used_percent"], 0)
                self.assertEqual(windows["secondary_window"]["used_percent"], 99)
                self.assertEqual(windows["secondary_window"]["reset_at"] - 1000, hours * 3600)
                mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                           {"redeem_request_id": "a" * 64,
                            "credit_id": "earned-weekly-credit"})
                self.assertTrue(mock.consumed)
                self.assertTrue(mock.spent)

    def test_weekly_redemption_has_a_strict_24_hour_boundary(self):
        for remaining in (24 * 3600 - 1, 24 * 3600, 24 * 3600 + 1, 25 * 3600, 48 * 3600, 49 * 3600):
            with self.subTest(remaining=remaining), patch.object(weekly_reset.time, "time", return_value=1000):
                mock = weekly_reset.Mock("preflight-weekly")
                mock.reset = 1000 + remaining
                mock.reply("GET", "/backend-api/wham/usage", None)
                consume = lambda: mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                                             {"redeem_request_id": "a" * 64,
                                              "credit_id": "earned-weekly-credit"})
                if remaining > 24 * 3600:
                    self.assertEqual(consume()[0], 200)
                    self.assertTrue(mock.consumed)
                else:
                    with self.assertRaisesRegex(RuntimeError, "redemption policy"):
                        consume()
                    self.assertFalse(mock.consumed)
                    self.assertFalse(mock.spent)

    def test_ineligible_weekly_refresh_cannot_borrow_a_healthy_five_hour_refresh(self):
        for reset in (None, 999, 1000):
            with self.subTest(reset=reset), patch.object(weekly_reset.time, "time", return_value=1000):
                mock = weekly_reset.Mock("preflight-weekly")
                mock.reply("GET", "/backend-api/wham/usage", None)
                windows = mock.requests[0]["response"]["rate_limit"]
                windows["secondary_window"]["reset_at"] = reset
                self.assertEqual(windows["secondary_window"]["used_percent"], 99)
                self.assertEqual(windows["primary_window"]["used_percent"], 0)
                self.assertGreater(windows["primary_window"]["reset_at"], 1000)
                with self.assertRaisesRegex(RuntimeError, "redemption policy"):
                    mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                               {"redeem_request_id": "a" * 64,
                                "credit_id": "earned-weekly-credit"})
                self.assertFalse(mock.consumed)

    def test_negative_cases_keep_a_credit_and_refuse_consumption(self):
        for name in weekly_reset.POLICY_BLOCKED_CASES:
            for models in (0, 2):
                with self.subTest(case=name, models=models), \
                        patch.object(weekly_reset.time, "time", return_value=1000):
                    mock = weekly_reset.Mock(name)
                    mock.models = [{} for _ in range(models)]
                    self.assertTrue(mock.credit)
                    self.assertEqual(mock.reply("GET", "/backend-api/wham/usage", None)[0], 200)
                    if name == "preflight-below-99":
                        self.assertEqual(mock.requests[0]["response"]["rate_limit"]["primary_window"]["used_percent"], 95)
                    if name in ("preflight-elapsed-reset", "recovery-elapsed-reset"):
                        self.assertLess(mock.requests[0]["response"]["rate_limit"]["secondary_window"]["reset_at"], 1000)
                    if name == "preflight-mixed-windows":
                        windows = mock.requests[0]["response"]["rate_limit"]
                        self.assertEqual(windows["primary_window"]["used_percent"], 98)
                        self.assertGreater(windows["primary_window"]["reset_at"], 1000)
                        self.assertGreaterEqual(windows["secondary_window"]["used_percent"], 99)
                        self.assertLess(windows["secondary_window"]["reset_at"], 1000)
                    with self.assertRaisesRegex(RuntimeError, "redemption policy"):
                        mock.reply("POST", "/backend-api/wham/rate-limit-reset-credits/consume",
                                   {"redeem_request_id": "a" * 64,
                                    "credit_id": "earned-weekly-credit"})
                    self.assertFalse(mock.consumed)
                    self.assertFalse(mock.spent)


def weekly_usage_read(reset_at):
    return {"path": "/backend-api/wham/usage", "response": {"rate_limit": {"secondary_window": {
        "limit_window_seconds": 604800, "used_percent": 99, "reset_at": reset_at}}}}


class ResetNoticeEvidenceTests(unittest.TestCase):
    @staticmethod
    def event(message, *, event_type="item.completed", item_type="error"):
        return {"type": event_type, "item": {"type": item_type,
                "message": weekly_reset.RESET_NOTICE_PREFIX + message}}

    @staticmethod
    def inventory():
        return [weekly_usage_read(1_900_000_000), {"path": "/backend-api/wham/rate-limit-reset-credits", "response": {
            "available_count": 2, "credits": [
                {"status": "available", "reset_type": "codex_rate_limits",
                 "expires_at": "2098-01-01T00:00:00Z"},
                {"status": "available", "reset_type": "codex_rate_limits",
                 "expires_at": "2099-02-03T04:05:06Z"}]}}]

    def messages(self, *, preserved=False):
        messages = [self.event("Considered earned usage reset: 2 available; expirations: "
                              "2098-01-01T00:00:00+00:00, 2099-02-03T04:05:06+00:00.")]
        if preserved:
            messages.append(self.event("Reset not used: the weekly allowance refreshes at "
                                       "2030-03-17T17:46:40+00:00 (within 24 hours); preserving earned resets."))
        else:
            messages.extend([
                self.event("Using an earned usage reset automatically: the weekly allowance "
                           "refreshes at 2030-03-17T17:46:40+00:00, more than 24 hours away."),
                self.event("Usage allowance restored after the earned reset; "
                           "continuing the original request.")])
        return messages

    def test_extraction_requires_the_nonfatal_exec_warning_shape(self):
        events = [self.event("visible"), self.event("started", event_type="item.started"),
                  self.event("model text", item_type="agent_message"),
                  {"type": "error", "message": weekly_reset.RESET_NOTICE_PREFIX + "fatal"},
                  {"type": "item.completed", "item": {"type": "error", "message": "unrelated"}},
                  {"type": "item.completed", "item": None}]
        self.assertEqual(weekly_reset.notice_messages(events, weekly_reset.RESET_NOTICE_PREFIX), ["visible"])

    def test_positive_cases_require_inventory_automatic_decision_and_confirmation(self):
        for case in weekly_reset.NOTICE_SUCCESS_CASES:
            with self.subTest(case=case):
                weekly_reset.check_reset_notice_events(case, self.messages(), self.inventory())
                for omitted in range(3):
                    with self.subTest(omitted=omitted), self.assertRaises(RuntimeError):
                        weekly_reset.check_reset_notice_events(
                            case, [event for index, event in enumerate(self.messages())
                                   if index != omitted], self.inventory())

    def test_policy_cases_require_inventory_and_the_preservation_reason(self):
        for case in weekly_reset.NOTICE_PRESERVE_CASES:
            with self.subTest(case=case):
                messages = self.messages(preserved=True)
                weekly_reset.check_reset_notice_events(case, messages, self.inventory())
                for omitted in range(2):
                    with self.subTest(omitted=omitted), self.assertRaises(RuntimeError):
                        weekly_reset.check_reset_notice_events(
                            case, messages[:omitted] + messages[omitted + 1:], self.inventory())

    def test_inventory_must_match_the_actual_count_and_every_expiration(self):
        for wrong in ("12 available", "2099-02-03T04:05:07+00:00"):
            with self.subTest(wrong=wrong), self.assertRaisesRegex(RuntimeError, "available count or expiration"):
                messages = self.messages()
                source = "2 available" if wrong.startswith("12") else "2099-02-03T04:05:06+00:00"
                messages[0]["item"]["message"] = messages[0]["item"]["message"].replace(source, wrong)
                weekly_reset.check_reset_notice_events("recovery", messages, self.inventory())

    def test_reset_notices_cannot_be_promoted_to_top_level_errors(self):
        events = self.messages() + [{"type": "error",
                  "message": weekly_reset.RESET_NOTICE_PREFIX + "reset status"}]
        with self.assertRaisesRegex(RuntimeError, "fatal JSON error"):
            weekly_reset.check_reset_notice_events("recovery", events, self.inventory())

    def test_unselected_scenarios_do_not_acquire_new_notice_requirements(self):
        weekly_reset.check_reset_notice_events("unrelated-provider", [], [])


class BackendAuthenticationFixtureTests(unittest.TestCase):
    def test_missing_or_wrong_credentials_are_rejected_before_backend_state_changes(self):
        mock = weekly_reset.Mock("preflight-five-hour")
        mock.expected_access_token = "fixture-negative-control-access-token"
        server, worker = weekly_reset.serve(mock)
        valid = {"Authorization": "Bearer " + mock.expected_access_token,
                 "ChatGPT-Account-Id": weekly_reset.ACCOUNT}

        def request(method, path, headers, body=None):
            with contextlib.closing(http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=2)) as connection:
                connection.request(method, "/backend-api/wham/" + path, body=body, headers=headers)
                with connection.getresponse() as reply:
                    reply.read()
                    return reply.status

        try:
            for field in valid:
                for replacement in (None, "wrong-fixture-credential"):
                    headers = dict(valid)
                    if replacement is None:
                        del headers[field]
                    else:
                        headers[field] = replacement
                    for method, path, body in (
                            ("GET", "usage", None),
                            ("GET", "rate-limit-reset-credits", None),
                            ("POST", "rate-limit-reset-credits/consume",
                             b'{"redeem_request_id":"fixture-key","credit_id":"earned-weekly-credit"}')):
                        with self.subTest(field=field, replacement=replacement, path=path):
                            self.assertEqual(request(method, path, headers, body), 401)
            self.assertFalse(mock.consumed)
            self.assertFalse(mock.spent)
            self.assertEqual(mock.credit_listings, 0)
            self.assertTrue(all(request["status"] == 401
                                and not all(request["authentication"].values())
                                for request in mock.requests))
            self.assertEqual(request("GET", "usage", valid), 200)
            self.assertTrue(all(mock.requests[-1]["authentication"].values()))
            self.assertNotIn(mock.expected_access_token, json.dumps(mock.requests))
            self.assertTrue(all(type(value) is bool for request in mock.requests
                                for value in request["authentication"].values()))
        finally:
            server.shutdown()
            worker.join()
            server.server_close()


class IsolatedExecWrapperTests(unittest.TestCase):
    def test_isolated_wrapper_execs_exact_arguments_and_records_its_native_pid(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pidfile = root / "native.pid"
            program = ("import json, os, sys; "
                       "print(json.dumps({'pid':os.getpid(), 'argv':sys.argv, "
                       "'isolated':sys.flags.isolated, 'noBytecode':sys.dont_write_bytecode})); "
                       "sys.exit(7)")
            result = subprocess.run(
                [sys.executable, "-I", "-B", str(Path(weekly_reset.__file__).resolve()),
                 "--exec-wrapper", str(pidfile), sys.executable, "-I", "-B", "-c", program,
                 "one argument", "literal-$()"],
                cwd=root, env={"PATH": os.defpath}, capture_output=True, text=True, check=False,
            )
            self.assertEqual(result.returncode, 7, result.stderr)
            executed = json.loads(result.stdout)
            self.assertEqual(executed["pid"], int(pidfile.read_text()))
            self.assertEqual(executed["argv"], ["-c", "one argument", "literal-$()"])
            self.assertEqual(executed["isolated"], 1)
            self.assertTrue(executed["noBytecode"])


if __name__ == "__main__":
    unittest.main()
