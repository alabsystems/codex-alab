# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the account-scoped catalog and ephemeral thread API using scripted replies."""

import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
from account_defaults import check_defaults


def model(name, **fields):
    return {"model": name, "hidden": False,
            "supportedReasoningEfforts": [{"reasoningEffort": "ultra"}], **fields}


def page(*rows, cursor=None):
    return {"data": list(rows), "nextCursor": cursor}


def thread(name, effort="ultra", **fields):
    return {"model": name, "reasoningEffort": effort, "modelProvider": "openai",
            "thread": {"ephemeral": True, "id": "private-thread"}, **fields}


class AccountDefaultTests(unittest.TestCase):
    def check(self, steps):
        calls = iter(steps)
        test = self

        class Rpc:
            def request(self, method, params, **kwargs):
                expected, arguments, reply = next(calls)
                test.assertEqual((method, params), (expected, arguments))
                if isinstance(reply, Exception):
                    raise reply
                return reply

        result = check_defaults(Rpc())
        self.assertIsNone(next(calls, None))
        self.assertNotIn("private-", json.dumps(result))
        return result

    def run_thread(self, catalog, started, *, after=None):
        return self.check([("model/list", {}, catalog),
                           ("thread/start", {"ephemeral": True}, started),
                           ("model/list", {}, catalog if after is None else after)])

    def test_highest_visible_unretired_ultra_release_and_catalog_tie_priority(self):
        name = "gpt-7.10-sol"
        later = page(model("gpt-7.10.0-sol"), model("gpt-7.9.9-sol"))
        first = page(
            model("gpt-8-sol", hidden=True),
            model("gpt-10-sol", upgradeInfo={"retirementAt": 1}),
            model("gpt-11-sol", supportedReasoningEfforts=[{"reasoningEffort": "xhigh"}]),
            *(model(s) for s in ("gpt-20-sol-preview", "gpt-026-sol", "gpt-99.0.0.1-sol",
                                 "gpt-18446744073709551616-sol", "gpt-８-sol", "gpt-11-astra")),
            model(name, upgradeInfo={"retirementAt": 2**62}), cursor="next")
        scan = [("model/list", {}, first), ("model/list", {"cursor": "next"}, later)]
        observed = self.check([*scan, ("thread/start", {"ephemeral": True}, thread(name)), *scan])
        self.assertEqual(observed, {"verified": True, "expectedModel": name,
                                   "model": name, "reasoningEffort": "ultra"})

    def test_mismatches_keep_only_safe_observed_fields(self):
        catalog = page(model("gpt-6.2-sol"), model("gpt-6.2-astra"), model("private-name"))
        result = self.run_thread(catalog, thread("gpt-6.2-astra", "low"))
        self.assertFalse(result["verified"])
        self.assertEqual(result["expectedModel"], "gpt-6.2-sol")
        self.assertEqual((result["model"], result["reasoningEffort"]), ("gpt-6.2-astra", "low"))
        for named in ("private-name", "private-\nvalue", "gpt-5-astra"):
            result = self.run_thread(catalog, thread(named, "private-effort"))
            self.assertNotIn("model", result)
            self.assertNotIn("reasoningEffort", result)
            self.assertFalse(result["verified"])

    def test_catalog_changes_do_not_report_a_stale_default(self):
        result = self.run_thread(page(model("gpt-6-sol")), thread("gpt-7-sol"),
                                 after=page(model("gpt-7-sol")))
        self.assertEqual(result, {"verified": False, "error": "default model changed during verification"})

    def test_private_errors_and_unconfirmed_thread_identity(self):
        for response in (RuntimeError("private-server-failure"),
                         thread("gpt-7-sol", modelProvider="private-provider"),
                         thread("gpt-7-sol", thread={"id": "private-thread", "ephemeral": False})):
            result = self.check([("model/list", {}, page(model("gpt-7-sol"))),
                                 ("thread/start", {"ephemeral": True}, response)])
            self.assertFalse(result["verified"])
            self.assertNotIn("model", result)

    def test_incomplete_cyclic_and_ineligible_catalogs_do_not_start_threads(self):
        partial = page(model("gpt-7-sol"), cursor="private-cursor")
        missing_visibility = model("gpt-8-sol")
        del missing_visibility["hidden"]
        for steps in (
            [("model/list", {}, partial), ("model/list", {"cursor": "private-cursor"}, partial)],
            [("model/list", {}, page(cursor="private-cursor"))],
            [("model/list", {}, page(model("gpt-7-sol"), cursor=False))],
            [("model/list", {}, page(model("gpt-7-sol"), missing_visibility))],
            [("model/list", {}, page(model("gpt-7-sol"), model("gpt-8-sol", hidden=0)))],
            [("model/list", {}, page(model("gpt-7-sol"),
                                     model("gpt-8-sol", supportedReasoningEfforts=[{"reasoningEffort": None}])))],
            [("model/list", {}, page(model("gpt-8-sol", upgradeInfo={"retirementAt": "private-time"})))],
            [("model/list", {}, page(model("gpt-8-sol", upgrade="gpt-9-sol")))],
            [("model/list", {}, {"data": [model("gpt-8-sol")]})],
            [("model/list", {}, RuntimeError("private-source"))],
            [("model/list", {}, page(model("gpt-8-sol", hidden=True)))],
        ):
            result = self.check(steps)
            self.assertFalse(result["verified"])
            self.assertNotIn("expectedModel", result)
