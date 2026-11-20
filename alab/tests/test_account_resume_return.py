# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import account_resume_return as returning


class ReturnRequestTests(unittest.TestCase):
    def setUp(self):
        self.project = Path("/fixture/cedar")
        self.evidence = {"thread": "cedar", "turn": "third", "directory": str(self.project)}
        self.reads = [{"path": path, "login": "source", "account": "workspace-source"}
                      for path in ("/v1/models", "/backend-api/wham/accounts/check")]
        previous = {"role": "user", "content": [{"type": "input_text", "text": returning.terminal.CONTINUE}]}
        self.last = {"history": [returning.terminal.CONTINUE], "conversation_input": [previous],
                     "tool_records": [{"call_id": "cedar", "output": "cedar marker"}]}
        self.request = {"login": "source", "account": "workspace-source", "thread": "cedar", "turn": "third",
                        "tool_records": self.last["tool_records"],
                        "history": [*self.last["history"], returning.terminal.ANSWER, returning.terminal.AFTER_RELEASE],
                        "conversation_input": [
                            previous,
                            {"role": "assistant", "content": [{"type": "output_text", "text": returning.terminal.ANSWER}]},
                            {"role": "user", "content": [{"type": "input_text", "text": returning.terminal.AFTER_RELEASE}]},
                        ]}

    def check(self, request):
        returning.check_request([request], self.reads, self.evidence, ["first", "second"], self.last, self.project)

    def test_extra_sender_fails_even_when_all_known_markers_and_tools_match(self):
        self.check(self.request)
        for role, kind in (("user", "input_text"), ("assistant", "output_text")):
            extra = {"role": role, "content": [{"type": kind, "text": "A sentence from another thread."}]}
            with self.subTest(role=role), self.assertRaisesRegex(RuntimeError, "sender content"):
                self.check(dict(self.request, conversation_input=[extra, *self.request["conversation_input"]]))

    def test_login_workspace_thread_and_turn_each_have_to_match(self):
        for key, value in (("login", "destination"), ("account", "workspace-destination"),
                           ("thread", "birch"), ("turn", "second")):
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                self.check(dict(self.request, **{key: value}))
