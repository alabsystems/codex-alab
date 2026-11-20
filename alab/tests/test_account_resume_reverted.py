# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0

from copy import deepcopy
import json
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock
from uuid import uuid4

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import account_resume_reverted as probe
import model_resume as terminal


class RevertedResume(unittest.TestCase):
    def test_continuation_checks_thread_login_route_and_surviving_messages(self):
        report = {"source": "a", "sibling": "b", "kept": "keep", "reverted": "drop", "sibling_turn": "other",
                  "completed": ["continued"], "events": [], "requests": []}
        gateway = probe.Gateway(report)
        gateway.tokens = {"A": "source", "B": "destination"}

        def item(role, text):
            return {"type": "message", "role": role,
                    "content": [{"type": "input_text" if role == "user" else "output_text", "text": text}]}

        original = [item("user", terminal.SEED)]
        preserved = [*original, item("assistant", terminal.REPLY)]
        cases = [("a", "keep", "A", original), ("a", "drop", "A", [*preserved, item("user", probe.DISCARD)]),
                 ("b", "other", "B", [item("user", terminal.BIRCH_SEED)]),
                 ("a", "continued", "B", [*preserved, item("user", terminal.CONTINUE)])]
        for thread, turn, token, items in cases:
            headers = {"thread-id": thread, "x-codex-turn-metadata": json.dumps({"turn_id": turn}),
                       "authorization": token, "chatgpt-account-id": "workspace-" + gateway.tokens[token]}
            body = {"input": items, "model": "test"}
            gateway.response(body, headers)
        probe.check_requests(report)
        for field, value in [("thread", "b"), ("turn", "drop"), ("login", "source"), ("account", "workspace-source"),
                             ("users", [terminal.CONTINUE]), ("answers", []), ("discarded_context", True)]:
            with self.subTest(field=field):
                mutated = deepcopy(report)
                mutated["requests"][-1][field] = value
                with self.assertRaises(RuntimeError):
                    probe.check_requests(mutated)
        gateway.response({**body, "input": [*items, item("developer", "previous output: " + probe.DISCARD_REPLY)]}, headers)
        self.assertTrue(report["requests"][-1]["discarded_context"])

    def test_active_branch_cannot_borrow_a_completion_from_its_predecessor(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sessions = root / "codex-home/sessions/2026/11/19"
            sessions.mkdir(parents=True)
            thread, turn = str(uuid4()), str(uuid4())
            parent = sessions / f"rollout-test-{thread}.jsonl"
            branch = sessions / f"rollout-test-{thread}_{uuid4()}.jsonl"
            gateway = terminal.Gateway([{"event": "request", "thread": thread, "turn": turn}])
            complete = {"type": "event_msg", "payload": {"type": "task_complete", "turn_id": turn,
                                                         "last_agent_message": terminal.ANSWER}}
            parent.write_text(json.dumps(complete) + "\n")
            branch.write_text(json.dumps({"type": "event_msg", "payload": {"type": "turn_aborted", "turn_id": turn}}) + "\n")
            args = (root, gateway, Mock(poll=lambda: None), terminal.ANSWER, time.monotonic() + 5)
            self.assertEqual(terminal.wait_for_turn_completion(*args), turn)
            with self.assertRaisesRegex(RuntimeError, "turn was aborted"):
                terminal.wait_for_turn_completion(*args, rollout_path=branch)
            branch.write_text(json.dumps(complete) + "\n")
            self.assertEqual(terminal.wait_for_turn_completion(*args, rollout_path=branch), turn)
