# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Discriminate the actual thread-list evidence required by the official 0.170.0 comparator."""

from pathlib import Path
import sys
import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
from official_resume_picker import list_controls


class OfficialPickerTests(unittest.TestCase):
    def control(self, change):
        work, sibling = Path("/seed"), Path("/sibling")
        row = {"id": "seed", "source": "exec", "cwd": str(work)}
        identity = {"account": {"type": "chatgpt", "email": "destination@example.invalid"},
                    "workspaceRouting": {"chatgptAccountId": "workspace-destination"}}

        def request(method, params):
            if method == "account/read":
                return identity
            self.assertEqual(method, "thread/list")
            visible = ("exec" in params["sourceKinds"] and not params["currentAccountOnly"]
                       and params.get("cwd", str(work)) == str(work))
            page = {"nextCursor": None, "data": [row] if visible else []}
            change(params, page, row)
            return page

        rpc = Mock()
        rpc.request.side_effect = request
        report = {}
        list_controls(rpc, row["id"], work, sibling, report)
        return report["list_controls"]

    def test_source_row_and_three_independent_narrowings(self):
        results = self.control(lambda *_: None)
        self.assertEqual({name: len(page["rows"]) for name, page in results.items()},
                         {"broad": 1, "account_only": 0, "interactive_only": 0, "cwd_only": 0,
                          "original_cwd": 1, "combined": 0})

    def test_missing_seed_or_each_filter_leak_is_detected(self):
        for name, condition in (
                ("broad", lambda p: not p["currentAccountOnly"] and "exec" in p["sourceKinds"] and "cwd" not in p),
                ("account_only", lambda p: p["currentAccountOnly"] and "exec" in p["sourceKinds"]),
                ("interactive_only", lambda p: not p["currentAccountOnly"] and "exec" not in p["sourceKinds"]),
                ("cwd_only", lambda p: not p["currentAccountOnly"] and p.get("cwd") == "/sibling")):
            def wrong(params, page, row):
                if condition(params):
                    page["data"] = [] if page["data"] else [row]
            with self.subTest(name=name), self.assertRaisesRegex(RuntimeError, name):
                self.control(wrong)

    def test_a_page_with_more_unobserved_rows_is_inconclusive(self):
        with self.assertRaisesRegex(RuntimeError, "broad"):
            self.control(lambda _, page, __: page.update(nextCursor="another-page"))
