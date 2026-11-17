# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check quota-wait verdicts over retained traces, without starting a binary."""
import copy
import importlib.util
import json
from pathlib import Path
import signal
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


SOURCE = Path(__file__).resolve().parents[1] / "smoke/quota_wait.py"
sys.path.insert(0, str(SOURCE.parent))
SPEC = importlib.util.spec_from_file_location("quota_wait_smoke", SOURCE)
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)


def events():
    return [{"type": "thread.started", "thread_id": "fixture-thread"},
            {"type": "turn.started"},
            {"type": "item.completed", "item": {"id": "answer", "type": "agent_message",
                                                   "text": SMOKE.probe.ANSWER}},
            {"type": "turn.completed", "usage": {"input_tokens": 0, "cached_input_tokens": 0,
                                                    "output_tokens": 0, "reasoning_output_tokens": 0}}]


class QuotaWaitSmokeTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.events = events()
        self.code = 0
        self.count = 5
        self.change_request = lambda _requests: None
        self.timed_out = False
        self.server = SimpleNamespace(server_port=1234, shutdown=mock.Mock(), server_close=mock.Mock())
        self.thread = SimpleNamespace(join=mock.Mock())
        self.enterContext(mock.patch.object(SMOKE.probe, "serve", side_effect=self.serve))
        self.enterContext(mock.patch.object(SMOKE.probe, "run", side_effect=self.run_binary))

    def serve(self, model):
        self.model = model
        return self.server, self.thread

    def run_binary(self, _binary, _args, root, _env, _label, *, interact=None):
        body = {"input": [{"type": "message", "role": "user", "content": SMOKE.probe.PROMPT}]}
        for number in range(self.count):
            *_, record = self.model.reply(copy.deepcopy(body))
            record.update(headerStartedAt=10 * number, headerAt=10 * number + .001,
                          requestStartedAt=10 * number)
        self.change_request(self.model.requests)
        with (root / "exec-stdout.jsonl").open("xb") as output:
            if interact:
                (root / "native.pid").write_text("999")
                pipe = SimpleNamespace(fileno=lambda: 123)
                notification = {"type": "item.completed", "item": {
                    "id": "wait", "type": "error", "message": "Waiting for account quota"}}
                with (mock.patch.object(SMOKE.select, "select", return_value=([pipe], [], [])),
                      mock.patch.object(SMOKE.os, "read", side_effect=[json.dumps(notification).encode() + b"\n", b""]),
                      mock.patch.object(SMOKE.os, "kill") as send_signal):
                    interact(SimpleNamespace(stdout=pipe), output)
                send_signal.assert_called_once_with(999, signal.SIGINT)
            for event in self.events:
                output.write(json.dumps(event).encode() + b"\n")
        SMOKE.probe.write_json(root / "exec-guardian-result.json",
                               {"returncode": self.code, "timed_out": self.timed_out})
        return self.code

    def case(self, name, *, cancel=False):
        output = self.root / name
        output.mkdir()
        return SMOKE.case(Path("/unexecuted-fixture/codex"), output, cancel)

    def assert_refused(self, name, pattern=None, *, cancel=False):
        with self.assertRaisesRegex(RuntimeError, pattern or "."):
            self.case(name, cancel=cancel)
        case = "cancel-distant-reset" if cancel else "repeated-stale-reset"
        self.assertFalse((self.root / name / case / "result.json").exists())
        self.assertTrue((self.root / name / case / "requests.json").is_file())

    def test_exact_answer_in_one_successful_turn_passes(self):
        result = self.case("valid")
        self.assertTrue(result["passed"])
        self.assertEqual(result["requestCount"], 5)
        retained = json.loads((self.root / "valid/repeated-stale-reset/result.json").read_text())
        self.assertEqual(retained, result)
        self.server.shutdown.assert_called_once_with()
        self.thread.join.assert_called_once_with()
        self.server.server_close.assert_called_once_with()

    def test_missing_wrong_or_duplicated_answer_cannot_pass(self):
        for change in ("missing", "wrong", "duplicated"):
            with self.subTest(change=change):
                self.events = events()
                if change == "missing":
                    self.events.pop(2)
                elif change == "wrong":
                    self.events[2]["item"]["text"] = "Unrelated answer."
                else:
                    self.events.insert(2, copy.deepcopy(self.events[2]))
                self.assert_refused(change)

    def test_missing_replayed_failed_or_reversed_turn_cannot_pass(self):
        for change in ("missing-start", "missing-end", "repeated-start", "repeated-end", "failed", "reversed"):
            with self.subTest(change=change):
                self.events = events()
                if change == "missing-start":
                    self.events.pop(1)
                elif change == "missing-end":
                    self.events.pop()
                elif change == "repeated-start":
                    self.events.insert(1, {"type": "turn.started"})
                elif change == "repeated-end":
                    self.events.append(copy.deepcopy(self.events[-1]))
                elif change == "failed":
                    self.events.append({"type": "turn.failed", "error": {"message": "fixture failure"}})
                else:
                    self.events[1], self.events[-1] = self.events[-1], self.events[1]
                self.assert_refused(change)

    def test_answer_outside_the_turn_cannot_pass(self):
        for change in ("before-start", "after-end"):
            with self.subTest(change=change):
                self.events = events()
                answer = self.events.pop(2)
                self.events.insert(1 if change == "before-start" else len(self.events), answer)
                self.assert_refused(change)

    def test_wait_warning_remains_valid_recovery_evidence(self):
        self.events.insert(2, {"type": "item.completed", "item": {
            "id": "wait", "type": "error", "message": "Waiting for account quota"}})
        self.assertTrue(self.case("warning")["passed"])

    def test_process_requests_input_and_backoff_still_gate_recovery(self):
        for change in ("exit", "extra-request", "lost-input", "early-retry"):
            with self.subTest(change=change):
                self.code, self.count = 0, 5
                self.change_request = lambda _requests: None
                if change == "exit":
                    self.code = 1
                elif change == "extra-request":
                    self.count = 6
                elif change == "lost-input":
                    self.change_request = lambda requests: requests[-1]["body"].update(input=[])
                else:
                    self.change_request = lambda requests: requests[-1].update(
                        requestStartedAt=requests[-2]["headerStartedAt"] + 3.99)
                self.assert_refused(change)

    def test_native_cancellation_keeps_its_handled_failure_contract(self):
        self.events, self.code, self.count = [], 1, 1
        result = self.case("cancel", cancel=True)
        self.assertTrue(result["passed"])
        self.assertEqual(result["rawReturncode"], 1)
        self.assertEqual(result["cancellation"]["signal"], "SIGINT")
        self.assertLess(result["cancellation"]["observedDelaySeconds"], 10)

    def test_native_cancellation_still_refuses_success_retry_and_timeout(self):
        for change in ("successful-exit", "completed-turn", "extra-request", "timeout"):
            with self.subTest(change=change):
                self.events, self.code, self.count, self.timed_out = [], 1, 1, False
                if change == "successful-exit":
                    self.code = 0
                elif change == "completed-turn":
                    self.events = [events()[-1]]
                elif change == "extra-request":
                    self.count = 2
                else:
                    self.timed_out = True
                self.assert_refused(change, cancel=True)


if __name__ == "__main__":
    unittest.main()
