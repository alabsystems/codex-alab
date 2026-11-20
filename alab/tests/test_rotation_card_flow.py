# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
from contextlib import nullcontext, redirect_stdout
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import tui_host_rotation as rotation


class CardHandoffFlowTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.home = self.root / "codex"
        self.model = rotation.QuotaModel(self.root)
        self.report = {}
        self.source = {"pid": 71, "kernelBirth": 101}
        self.receipt = {"id": "a" * 32, "phase": "consumed", "old_pid": 71,
                        "successor_pid": 72, "successor_birth": 102,
                        "intent": {"thread": "original-thread", "wall": "original-turn"}}
        (self.root / "logs").mkdir()
        (self.root / "logs/aterm.log").write_text("kept names pid 71 and its conversation\n")
        pool = self.root / "harness/claude-harness/accounts-codex"
        pool.mkdir(parents=True)
        (pool / "state.json").write_text('{"active":"target","proven":true}')
        self.paths = {"old": self.root / "main/auth.json", "new": self.root / "target/auth.json"}
        self.ctl = mock.Mock()
        self.card = mock.Mock(spec=["open", "dismiss_source"])
        self.card.open.side_effect = lambda: self.assertFalse(self.model.snapshot_ready.is_set())
        self.card.dismiss_source.side_effect = self.dismiss_source
        self.closed_state = "open"
        self.roots = []
        self.poll = 0
        self.premature = None
        self.wall = {"kind": "foreground", "account": "source", "status": 429}
        self.target = {"kind": "foreground", "account": "target", "status": 200,
                       "monotonicUs": 30, "responseWrittenUs": 40}
        self.enterContext(mock.patch.object(rotation.fixture, "records",
            return_value=[{"pid": 71, "sessionId": "original-thread"}]))
        self.enterContext(mock.patch.object(rotation.fixture, "process_evidence", return_value=self.source))
        self.enterContext(mock.patch.object(rotation.fixture, "pool_ledger",
            return_value=[{"kind": "proven", "account": "target"}]))
        self.enterContext(mock.patch.object(rotation, "receipts", side_effect=lambda _pool: self.roots))
        self.enterContext(mock.patch.object(rotation, "completed_rollout", return_value=["original rollout"]))
        self.enterContext(mock.patch.object(rotation.time, "sleep"))
        # The real kqueue, signed image and rollout content are covered by the full-host fixture.
        watcher = mock.Mock()
        watcher.control.side_effect = lambda *_: ([SimpleNamespace(ident=71, fflags=1)]
                                                  if self.poll == 4 or self.premature == "exit" else [])
        self.enterContext(mock.patch.object(rotation, "select",
            SimpleNamespace(kqueue=lambda: watcher, kevent=mock.Mock(), KQ_FILTER_PROC=-5,
                            KQ_EV_ADD=1, KQ_EV_ONESHOT=16, KQ_NOTE_EXIT=1)))
        self.checked = self.enterContext(mock.patch.object(rotation, "verify"))
        self.claim = self.enterContext(mock.patch.object(rotation, "verify_claim_loss"))
        self.claim.side_effect = lambda *_args, **_kwargs: self.checked.assert_called_once()

    def dismiss_source(self):
        self.assertEqual(self.poll, 3, "wait for the provider to finish writing the refusal")
        self.assertEqual(self.report["native_usage_interval_us"]["source_429_written"], 20)
        self.assertEqual(self.report["model_calls"], rotation.EXPECTED_CALLS[:2])
        return self.closed_state

    def advance(self):
        self.poll += 1
        if self.poll == 2:
            self.card.open.assert_called_once_with()
            self.assertTrue(self.model.snapshot_ready.is_set())
            self.model.source_walled = True
            self.model.http_events.extend([{"kind": "foreground", "account": "source", "status": 200},
                                           self.wall])
        elif self.poll == 3:
            self.card.dismiss_source.assert_not_called()
            self.wall["responseWrittenUs"] = 20
            if self.premature == "target":
                self.model.http_events.append(self.target)
            elif self.premature == "pending":
                self.model.http_events.append({"kind": "foreground", "account": "target", "monotonicUs": 30})
            elif self.premature == "launch":
                self.roots.append(dict(self.receipt, phase="waiting", launch_attempted=True))
        elif self.poll == 4:
            self.card.dismiss_source.assert_called_once_with()
            self.model.http_events.append(self.target)
            self.roots.append(self.receipt)
        elif self.poll > 4:
            self.fail("the full-host loop polled past the completed original turn")
        return True

    def run_turn(self):
        rotation.run_account_turn(self.model, self.ctl, self.report, state=self.root, codex_home=self.home,
                                  binary=self.root / "codex-bin", issuer="https://provider.invalid",
                                  host_running=self.advance, auth_paths=self.paths, target_label="target",
                                  card=self.card)

    def test_weekly_device_footers_are_checked_on_either_side_of_the_original_turn(self):
        emails = {"old": "old@example.invalid", "new": "new@example.invalid"}

        def advance():
            self.poll += 1
            self.assertLessEqual(self.poll, 2)
            if self.poll == 2:
                self.assertTrue(self.model.snapshot_ready.is_set())
                self.model.http_events.extend([{"kind": "foreground", "account": "source", "status": 200},
                                               self.wall, self.target])
                self.roots.append(self.receipt)
            return True

        def observe(ctl, address, phase, pid):
            self.assertIs(ctl, self.ctl)
            if phase == "source":
                self.assertFalse(self.model.snapshot_ready.is_set())
                self.checked.assert_not_called()
            else:
                self.checked.assert_called_once()
            return {"email": address, "pid": pid, "frame": "1.20"}

        with mock.patch.object(rotation, "observe_weekly_footer", side_effect=observe) as footer:
            rotation.run_account_turn(self.model, self.ctl, self.report, state=self.root, codex_home=self.home,
                                      binary=self.root / "codex-bin", issuer="https://provider.invalid",
                                      host_running=advance, auth_paths=self.paths, target_label="target",
                                      footer_emails=emails)
        self.assertEqual(footer.call_args_list, [mock.call(self.ctl, emails["old"], "source", 71),
                                                mock.call(self.ctl, emails["new"], "destination", 72)])
        self.assertEqual(self.report["visible_weekly_footers"]["target"]["email"], emails["new"])

    def test_source_card_and_completed_turn_precede_claim_loss_on_the_receipted_successor(self):
        self.run_turn()
        self.checked.assert_called_once_with(self.model, self.home, self.receipt, ["original rollout"],
                                            self.source, True, self.report, auth_paths=self.paths)
        self.claim.assert_called_once_with(self.card, self.ctl, pid=72, birth=102, thread="original-thread",
                                           codex_home=self.home, report=self.report)
        self.assertEqual(self.report["source_card_at_refusal"], "open")

    def test_refused_source_card_never_releases_the_original_model_request(self):
        self.card.open.side_effect = RuntimeError("source card unavailable")
        with self.assertRaisesRegex(RuntimeError, "source card unavailable"):
            self.run_turn()
        self.assertFalse(self.model.snapshot_ready.is_set())
        self.checked.assert_not_called()
        self.claim.assert_not_called()

    def test_already_withdrawn_source_card_still_requires_verified_handoff(self):
        self.closed_state = "withdrawn"
        self.checked.side_effect = RuntimeError("wrong successor birth")
        with self.assertRaisesRegex(RuntimeError, "wrong successor birth"):
            self.run_turn()
        self.claim.assert_not_called()
        self.assertEqual(self.report["source_card_at_refusal"], "withdrawn")

    def test_target_request_while_source_details_are_open_fails_before_dismissal(self):
        self.premature = "target"
        with self.assertRaisesRegex(RuntimeError, "replacement started"):
            self.run_turn()
        self.card.dismiss_source.assert_not_called()
        self.claim.assert_not_called()
        self.assertEqual(self.report["model_calls"], rotation.EXPECTED_CALLS)

    def test_launch_attempt_while_source_details_are_open_fails_before_dismissal(self):
        self.premature = "launch"
        with self.assertRaisesRegex(RuntimeError, "replacement started"):
            self.run_turn()
        self.card.dismiss_source.assert_not_called()
        self.claim.assert_not_called()
        self.assertEqual(self.report["model_calls"], rotation.EXPECTED_CALLS[:2])

    def test_inflight_target_request_with_no_response_is_already_a_premature_move(self):
        self.premature = "pending"
        with self.assertRaisesRegex(RuntimeError, "replacement started"):
            self.run_turn()
        self.card.dismiss_source.assert_not_called()
        self.claim.assert_not_called()
        self.assertEqual(self.report["model_calls"], rotation.EXPECTED_CALLS[:2])

    def test_early_source_exit_fails_before_attempting_to_dismiss_a_successor_view(self):
        self.premature = "exit"
        with self.assertRaisesRegex(RuntimeError, "source exited"):
            self.run_turn()
        self.card.dismiss_source.assert_not_called()
        self.claim.assert_not_called()


class CardModeTests(unittest.TestCase):
    def test_card_fixture_uses_distinct_provider_identities_with_the_same_mask(self):
        plain = rotation.fixture_slots(Path("/fixture"))
        shared = rotation.fixture_slots(Path("/fixture"), shared_mask=True)
        self.assertEqual(plain["main"].email, "fixture@example.invalid")
        first, second = shared["main"], shared["target"]
        self.assertNotEqual(first.account, second.account)
        self.assertNotEqual(first.email, second.email)
        self.assertEqual((first.email[0], first.email.split("@")[1]),
                         (second.email[0], second.email.split("@")[1]))
        self.assertEqual(first.auth_path, plain["main"].auth_path)
        self.assertEqual(second.auth_path, plain["target"].auth_path)

    def test_card_cli_requests_visible_lab_and_two_device_logins_only_on_opt_in(self):
        for enabled in (False, True):
            with self.subTest(card=enabled), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                host = root / "host"
                host.touch()
                result = root / "result.json"
                args = ["rotation", "--aterm", str(host), "--aterm-lab", str(host),
                        "--package", temp, "--result", str(result)]
                if enabled:
                    args.append("--account-card")
                with (mock.patch.object(sys, "argv", args), mock.patch.object(sys, "platform", "darwin"),
                      mock.patch.object(rotation.signal, "signal"),
                      mock.patch.object(rotation.probe, "fresh_environment", return_value={}),
                      mock.patch.object(rotation, "pin_executable", return_value=host),
                      mock.patch.object(rotation, "run", return_value="build: debug / macos-arm64"),
                      mock.patch.object(rotation.probe, "package_records",
                                        return_value=({"source": "fixture", "version": "1"}, None, host)),
                      mock.patch.object(rotation.fixture, "image_digest", return_value=(host, "digest")),
                      mock.patch.object(rotation.device, "device_login_issuer") as issuer,
                      mock.patch.object(rotation, "listener", side_effect=lambda *a, **kw: nullcontext(1234)),
                      mock.patch.object(rotation.subprocess, "Popen",
                                        side_effect=OSError("fixture launch stopped")) as launch,
                      redirect_stdout(io.StringIO())):
                    self.assertEqual(rotation.main(), 1)
                self.assertEqual(json.loads(result.read_text())["error"], "OSError: fixture launch stopped")
                self.assertEqual("--window" in launch.call_args.args[0], enabled)
                if enabled:
                    self.assertEqual(len(issuer.call_args.args[1]), 2)
                else:
                    issuer.assert_not_called()


if __name__ == "__main__":
    unittest.main()
