# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Negative controls for the response/notification oracle, including frames received through EOF."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
from goal_observer_disconnect import verify_delivery
from goal_output_order import read_outputs


class GoalWireOrderTests(unittest.TestCase):
    def setUp(self):
        self.requests = {1: ("a", "set", "first"), 2: ("a", "set", "second"), 3: ("b", "clear", None)}
        def pair(request_id):
            thread, operation, objective = self.requests[request_id]
            if operation == "clear":
                return [{"id": request_id, "result": {"cleared": True}},
                        {"method": "thread/goal/cleared", "params": {"threadId": thread}}]
            goal = {"threadId": thread, "objective": objective, "status": "paused"}
            return [{"id": request_id, "result": {"goal": goal}},
                    {"method": "thread/goal/updated", "params": {"threadId": thread, "turnId": None, "goal": goal}}]
        self.first, self.second, self.clear = (pair(request_id) for request_id in self.requests)

    def test_independent_thread_may_progress_between_a_response_and_its_notification(self):
        trace = [self.first[0], *self.clear, self.first[1], *self.second]
        self.assertEqual(read_outputs(trace, self.requests, ("a", "b"))["b"],
                         [("response", "clear", None), ("notification", "clear", None)])

    def test_reordering_dropping_or_duplicating_a_real_message_fails(self):
        valid = [*self.first, *self.second, *self.clear]
        for trace in ([self.first[1], self.first[0], *valid[2:]],     # notification before response
                      [self.first[0], *self.second, self.first[1], *self.clear],  # FIFO broken
                      valid[:-1], valid + [self.clear[1]],              # missing, then late duplicate
                      [*self.first, *self.first, *self.clear]):        # replay replaces another mutation
            with self.subTest(trace=trace), self.assertRaises(RuntimeError):
                read_outputs(trace, self.requests, ("a", "b"))

    def test_refused_request_has_exactly_one_error_through_eof(self):
        refused = {"id": 4, "error": {"code": -32603, "message": "goal backlog is full"}}
        accepted = [*self.first, *self.second, *self.clear]
        read_outputs([refused, *accepted], self.requests, ("a", "b"), refused=refused)
        for unexpected in ([], [refused, refused], [{"id": 4, "result": {"cleared": True}}]):
            with self.subTest(unexpected=unexpected), self.assertRaises(RuntimeError):
                read_outputs([*unexpected, *accepted], self.requests, ("a", "b"), refused=refused)

    def test_wrong_thread_wrong_payload_and_rejected_response_fail(self):
        for wrong in ({"method": "thread/goal/cleared", "params": {"threadId": "a"}},
                      {"method": "thread/goal/cleared", "params": {"threadId": "elsewhere"}},
                      {"id": 3, "result": {"cleared": False}}, {"id": 3, "error": {"code": -1}}):
            trace = [*self.first, *self.second, self.clear[0], wrong]
            with self.subTest(wrong=wrong), self.assertRaises(RuntimeError):
                read_outputs(trace, self.requests, ("a", "b"))

    def test_disconnected_observer_response_and_misordered_survivor_delivery_are_detected(self):
        def receive(value):
            return ("surviving", "receive", value)

        update = receive({"method": "thread/goal/updated", "params": {"threadId": "a"}})
        reply = receive(self.clear[0])
        clear = receive({"method": "thread/goal/cleared", "params": {"threadId": "a"}})
        verify_delivery([update, reply, clear], "a", 1002, 3)
        for trace in ([receive({"id": 1002}), update, reply, clear], [update, update, reply, clear],
                      [reply, clear], [update, clear, reply], [update, reply, receive(self.clear[1])]):
            with self.subTest(trace=trace), self.assertRaises(RuntimeError):
                verify_delivery(trace, "a", 1002, 3)
