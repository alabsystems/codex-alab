# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import account_resume_writer as writer


class WriterConversationTests(unittest.TestCase):
    def setUp(self):
        work = Path("/fixture/work")
        context = writer.message("user", f"<environment_context><cwd>{work}</cwd></environment_context>")
        self.seed = [context, writer.message("user", writer.terminal.SEED)]
        source = [*self.seed, writer.message("assistant", writer.terminal.REPLY)]
        self.continued = [*source, writer.message("user", writer.terminal.CONTINUE)]
        self.rollout = b"\n".join(json.dumps({"type": "response_item", "payload": m}).encode() for m in source)
        self.saved = writer.saved_conversation(self.rollout, work)

    def observed(self, messages):
        report = {"events": [], "requests": []}
        gateway = writer.account.Gateway(report)
        gateway.capture_conversation_input = True
        gateway.response({"model": "fixture", "input": messages}, {})
        return report["requests"][0]

    def test_writer_checks_both_requests_and_accepts_independent_provider_instructions(self):
        seed = self.observed(self.seed)
        continued = self.observed([{"role": "developer", "content": "one instruction"}, *self.continued])
        followup = self.observed([{"role": "system", "content": "a different instruction"}, *self.continued])
        writer.check_conversation_input(self.saved, seed, seed, continued, followup)

    def test_unmarked_foreign_text_on_either_request_rejects_unchanged_markers(self):
        seed = self.observed(self.seed)
        good = self.observed(self.continued)
        for role in ("user", "assistant"):
            changed = self.observed([writer.message(role, "A sentence from a different thread."), *self.continued])
            self.assertEqual(changed["history"], good["history"])
            for pair in ((changed, good), (good, changed), (changed, changed)):
                with self.subTest(role=role, pair=pair), self.assertRaisesRegex(RuntimeError, "resumed turn"):
                    writer.check_conversation_input(self.saved, seed, seed, *pair)

    def test_an_extra_part_or_changed_environment_cannot_hide_behind_the_saved_seed(self):
        seed = self.observed(self.seed)
        continued = self.observed(self.continued)
        for i in (0, 1):
            altered = copy.deepcopy(self.seed)
            altered[i]["content"].append({"type": "input_text", "text": "Another thread."})
            changed = self.observed(altered)
            self.assertEqual(changed["history"], seed["history"])
            with self.assertRaisesRegex(RuntimeError, "source turn"):
                writer.check_conversation_input(self.saved, changed, changed, continued, continued)

    def test_unmarked_request_on_original_thread_is_not_a_background_title(self):
        seed = dict(self.observed(self.seed), thread="original")
        extra = dict(self.observed([writer.message("user", "A different conversation.")]), thread="original")
        self.assertFalse(extra["history"])
        with self.assertRaisesRegex(RuntimeError, "request count on the original thread"):
            writer.check_requests(None, {"thread": "original", "requests": [seed, extra, seed], "seed_requests": 3}, None)
