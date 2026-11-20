# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0

from copy import deepcopy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import websocket_close_parity as probe


def web_event(transport, connection, items, **kwargs):
    return dict(transport=transport, connection=connection, input=items, path="/v1/responses",
                model=probe.fixture.MODEL, previous_response_id=None, warmup=False) | kwargs


def message(role, text):
    return {"role": role, "content": [{"type": "output_text" if role == "assistant" else "input_text", "text": text}]}


class WebSocketCloseOracle(unittest.TestCase):
    def events(self, case):
        close, sent_created, methods = probe.CASES[case]
        baseline = message("developer", "Use the scratch project.")
        history = [message(role, text) for role, text in probe.TRANSCRIPT]
        return [web_event("WS", 1, [history[0]]),
                web_event("WS", 2, [baseline], warmup=True, response_id="warmup"),
                web_event("WS", 2, history, previous_response_id="warmup", close=close, sent_created=sent_created),
                web_event(methods[-1], 3 if methods[-1] == "WS" else None, [baseline, *history])]

    def test_close_boundary_determines_whether_http_is_used(self):
        for case in probe.CASES:
            with self.subTest(case=case):
                events = self.events(case)
                probe.check(case, events)
                events[-1]["transport"] = "WS" if events[-1]["transport"] == "POST" else "POST"
                with self.assertRaisesRegex(RuntimeError, "transport sequence"):
                    probe.check(case, events)

    def test_retry_cannot_drop_prewarmed_items_or_reorder_the_dialogue(self):
        case = "size-before-events"
        for change in ("developer", "order", "connection", "extra_post"):
            with self.subTest(change=change):
                events = deepcopy(self.events(case))
                if change == "developer":
                    del events[-1]["input"][0]
                elif change == "order":
                    items = events[-1]["input"]
                    items[-2], items[-1] = items[-1], items[-2]
                elif change == "connection":
                    events[2]["connection"] = 4
                else:
                    events.append({"unexpected_post": "/v1/other"})
                with self.assertRaises(RuntimeError):
                    probe.check(case, events)
