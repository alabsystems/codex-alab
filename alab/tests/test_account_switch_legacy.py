# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The switch test must account for every restart attempt on its synthetic socket."""

import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import account_switch_legacy as smoke
from account_switch_legacy import Control, LegacyHost, SID, check_dispatch


class LegacySwitchTest(unittest.TestCase):
    def test_weekly_reading_belongs_to_its_masked_slot_and_condition(self):
        first = [f"source · current session · {smoke.MASK}", "usable · enrolled", "5h: 12% used", "Weekly: 23% used"]
        second = [f"standby · {smoke.MASK}", "limited · enrolled", "5h: 12% used", "Weekly: 100% used"]
        self.assertTrue(smoke.shows_quota(first + second, "standby", "limited", 100))
        self.assertFalse(smoke.shows_quota(first + second, "source", "usable", 100))
        self.assertFalse(smoke.shows_quota(first + second, "standby", "usable", 100))
        self.assertFalse(smoke.shows_quota(first + second, "standby", "limited", 12))
        self.assertFalse(smoke.shows_quota(first + second[:-1] + first, "standby", "limited", 23))
        self.assertFalse(smoke.shows_quota(second + second, "standby", "limited", 100))

    def test_picked_move_records_every_destination_before_refusing_wrong_routes(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as root, LegacyHost(Path(root), Control) as host:
            host.session = {"process_pid": 123}
            command = "restart pid=123 account=standby"
            for route in ("@" + SID, "@.", "@2", "@t0000000002", None):
                for sep in (" ", "\t", " \t "):
                    with self.subTest(route=route, separator=sep):
                        host.restarts.clear()
                        request = (route + sep if route else "") + command
                        reply = host.reply(request)
                        self.assertEqual(host.restarts, [(route, command)])
                        self.assertEqual(reply, "ERR restart unavailable: account-limited"
                                         if route == "@" + SID else "ERR unsupported")
                        with self.assertRaisesRegex(RuntimeError, "label-only restart"):
                            check_dispatch("picked", host.restarts, 123)
                        if route == "@" + SID:
                            check_dispatch("typed", host.restarts, 123)
                        else:
                            with self.assertRaisesRegex(RuntimeError, "single legacy request"):
                                check_dispatch("typed", host.restarts, 123)

    def test_other_account_changes_remain_observable_when_the_route_is_refused(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as root, LegacyHost(Path(root), Control) as host:
            for route in ("@" + SID, "@.", None):
                line = (route + "\t" if route else "") + "codex-accounts-change fixture-operation"
                self.assertEqual(host.reply(line), "ERR unsupported")
                self.assertEqual(host.unsupported_changes[-1], (route, ["codex-accounts-change", "fixture-operation"]))
            self.assertFalse(host.restarts)

    def test_typed_move_requires_the_expected_pid_label_and_one_call(self):
        expected = ("@" + SID, "restart pid=123 account=standby")
        for restarts in ([], [expected, expected], [("@" + SID, "restart pid=124 account=standby")],
                         [("@" + SID, "restart pid=123 account=source")]):
            with self.subTest(restarts=restarts), self.assertRaisesRegex(RuntimeError, "single legacy request"):
                check_dispatch("typed", restarts, 123)

    def test_verdict_includes_requests_completed_while_listeners_close(self):
        # Model work that was accepted before the terminal quit and finished in server_close.
        # The PTY itself is covered by the standalone smoke. This isolates ordering on both listeners.
        provider = smoke.terminal.Gateway([])
        close_host, close_provider = LegacyHost.server_close, smoke.JoinedProvider.server_close

        def finish_control(host):
            close_host(host)
            host.session = {"process_pid": 123}
            host.snapshots.append("original")
            host.reply("@" + SID + "\trestart pid=123 account=standby")

        def finish_provider(server):
            close_provider(server)
            provider.events.append({"event": "request", "seed_prompt": True})

        def finish_terminal(_python, guardian, *_args, **_kwargs):
            spec = json.loads(Path(guardian[-1]).read_text())
            Path(spec["result"]).write_text(json.dumps({"exit": 0, "forced": False, "owning_reap": True,
                                                       "group_absent_after_reap": True}))
            return 0

        with tempfile.TemporaryDirectory(dir="/tmp") as root:
            report = {}
            with (patch.object(smoke.terminal, "Gateway", return_value=provider),
                  patch.object(LegacyHost, "server_close", finish_control),
                  patch.object(smoke.JoinedProvider, "server_close", finish_provider),
                  patch.object(smoke.fixture, "run", finish_terminal),
                  patch.dict(smoke.terminal.CHECK, {"check_model_terminal_exit": lambda *_: None}),
                  self.assertRaisesRegex(RuntimeError, "label-only restart")):
                smoke.exercise(Path("/unused/terminal"), Path(root), "picked", report)
        self.assertEqual(report["restarts"], [{"route": "@" + SID, "command": "restart pid=123 account=standby"}])
        self.assertEqual(report["provider_calls"], 1)
        self.assertTrue(report["original_turn_only"])
        self.assertFalse(report.get("cleanupErrors"))
