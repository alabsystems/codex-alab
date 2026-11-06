# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Replay retained actual foreground/title packets through the fixture server policy."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import renewal_follow as renewal

FIXTURES = Path(__file__).parent / "fixtures" / "renewal-request-attribution"


class AttributionTests(unittest.TestCase):
    def setUp(self):
        self.foreground = json.loads((FIXTURES / "foreground.json").read_text())
        self.title = json.loads((FIXTURES / "thread-title.json").read_text())

    def test_actual_packets_and_unknown_shapes(self):
        self.assertEqual(renewal.request_kind(self.foreground), "foreground")
        self.assertEqual(renewal.request_kind(self.title), "thread-title")
        for mutation in ("schema", "numeric_strict", "instruction", "title_tools", "missing_plan", "empty_tools"):
            body = copy.deepcopy(self.title if mutation in ("schema", "numeric_strict", "instruction", "title_tools") else self.foreground)
            if mutation == "schema":
                body["text"]["format"]["schema"]["properties"]["title"]["maxLength"] = 37
            elif mutation == "numeric_strict":
                body["text"]["format"]["strict"] = 1
            elif mutation == "instruction":
                body["input"][-1]["content"][0]["text"] = "An unrelated auxiliary request"
            elif mutation == "title_tools":
                body["tools"] = self.foreground["tools"]
            elif mutation == "missing_plan":
                body["tools"] = [tool for tool in body["tools"] if tool.get("name") != "update_plan"]
            else:
                body["tools"] = []
            with self.subTest(mutation=mutation):
                self.assertEqual(renewal.request_kind(body), "unknown")

    def test_auxiliary_first_never_consumes_foreground_plan_or_auth_sequence(self):
        with tempfile.TemporaryDirectory() as directory:
            model = renewal.Model(Path(directory))
            title = model.reply(self.title)
            self.assertEqual(title[0], 200)
            self.assertIn(b'Recover original work', title[1])
            self.assertEqual(len(model.requests), 0)
            first = model.reply(self.foreground)
            self.assertEqual(first[0], 200)
            self.assertIn(b'"name": "update_plan"', first[1])
            self.assertEqual(model.reply(self.title)[0], 200)
            self.assertEqual(model.reply(self.foreground)[0], 401)
            self.assertEqual(len(model.requests), 2)
            self.assertEqual(len(model.auxiliary_requests), 2)
            (Path(directory) / "renewed").touch()
            self.assertEqual(model.reply(self.foreground)[0], 200)
            self.assertEqual(len(model.requests), 3)
            self.assertEqual(len(model.auxiliary_requests), 2)
            self.assertEqual(model.reply({"tools": [], "input": []})[0], 400)
            self.assertEqual(len(model.requests), 3)
            self.assertEqual(len(model.unknown_requests), 1)


if __name__ == "__main__":
    unittest.main()
