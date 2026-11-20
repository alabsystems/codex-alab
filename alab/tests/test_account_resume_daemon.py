# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
from contextlib import ExitStack
from pathlib import Path
import sys
import tempfile
from threading import Lock
import unittest
from unittest.mock import Mock, call, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
from account_resume_daemon import ImplicitSeedDaemon, ManagedDaemon, SHARED_EMAIL


class AccountResumeDaemon(unittest.TestCase):
    def test_refuses_named_credentials_and_existing_socket_alias(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            home = root / "codex-home"
            home.mkdir()
            env = {"CODEX_HOME": str(home)}
            with self.assertRaisesRegex(RuntimeError, "shared fixture"):
                ManagedDaemon(Path("/fixture/codex"), root, {**env, "CODEX_AUTH_PATH": "A"}, "sha", {})
            control = home / "app-server-control"
            control.mkdir()
            (control / "app-server-control.sock").symlink_to(root / "absent")
            with self.assertRaisesRegex(RuntimeError, "already has a daemon socket"):
                ManagedDaemon(Path("/fixture/codex"), root, env, "sha", {})

    def test_matched_daemon_requires_the_resumed_thread_and_other_account_can_keep_its_backend(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            home = root / "codex-home"
            home.mkdir()
            sink = {}
            daemon = ManagedDaemon(Path("/fixture/codex"), root, {"CODEX_HOME": str(home)}, "sha", sink)

            daemon.rpc = Mock()
            daemon.sentinel = "live-observer"
            daemon.login = "destination"
            loaded = {"data": [daemon.sentinel], "nextCursor": None}
            daemon.rpc.request.return_value = loaded
            daemon.loaded("selected", initial=True)
            with self.assertRaisesRegex(RuntimeError, "same-login resume"):
                daemon.loaded("selected")
            daemon.login = "source"
            identity = {"account": {"type": "chatgpt", "email": SHARED_EMAIL},
                        "workspaceRouting": {"chatgptAccountId": "workspace-source"}}
            daemon.rpc.request.side_effect = [loaded, identity]
            daemon.loaded("selected")
            self.assertFalse(sink["resumed_thread_on_original_daemon"])
            identity["workspaceRouting"]["chatgptAccountId"] = "workspace-destination"
            daemon.rpc.request.side_effect = [loaded, identity]
            with self.assertRaisesRegex(RuntimeError, "observer's account changed"):
                daemon.loaded("selected")


class ImplicitWriterTests(unittest.TestCase):
    def setUp(self):
        self.stack = ExitStack()
        self.addCleanup(self.stack.close)
        root = Path(self.stack.enter_context(tempfile.TemporaryDirectory())).resolve()
        self.home = root / "codex-home"
        self.home.mkdir()
        self.owner = ImplicitSeedDaemon(Path("/fixture/codex"), root, {"CODEX_HOME": str(self.home)}, "sha", {})
        self.raw = root / "terminal.raw"
        self.raw.write_bytes(b"frame")
        self.owner.live_at = 1
        self.gateway = Mock(lock=Lock(), requests=[])
        self.proc = Mock()
        self.proc.poll.return_value = None

    def test_refuses_a_claimed_owner_when_original_thread_was_never_loaded(self):
        self.owner.loaded_ids = Mock(return_value=["unrelated"])
        self.stack.enter_context(patch("picker_screen.visible_lines", return_value=self.lock_screen()))
        with self.assertRaisesRegex(RuntimeError, "already released"):
            self.retry()

    def test_rejects_inference_during_locked_read_and_premature_release(self):
        self.stack.enter_context(patch("picker_screen.visible_lines", return_value=self.lock_screen()))
        self.owner.loaded_ids = Mock(side_effect=[["original", "sibling"], ["sibling"]])
        self.gateway.requests.append({"thread": "original"})
        with patch("account_resume_daemon.time.monotonic", return_value=80):
            with self.assertRaisesRegex(RuntimeError, "ran inference"):
                self.retry()
        self.owner.loaded_ids = Mock(side_effect=[["original"], []])
        with patch("account_resume_daemon.time.monotonic", return_value=2):
            with self.assertRaisesRegex(RuntimeError, "before the default idle delay"):
                self.retry()

    def test_cleanup_on_incomplete_seed_reconciles_updater_even_before_socket_is_ready(self):
        pid = self.owner.state / "daemon-updater.pid"
        pid.write_text("pending")
        def lifecycle(action):
            if action == "restart":
                pid.unlink()
                raise RuntimeError("incomplete updater")
            return {"status": "notRunning"}
        self.owner.command = Mock(side_effect=lifecycle)
        with self.assertRaisesRegex(RuntimeError, "incomplete updater"):
            self.owner.stop()
        self.assertEqual(self.owner.command.call_args_list, [call("restart"), call("stop")])

    def retry(self):
        self.owner.retry_after_unload("original", self.gateway, 0, self.raw, self.proc, Mock(), Mock(), "marker")

    @staticmethod
    def lock_screen():
        from model_resume import SEED, REPLY
        return [SEED, REPLY, "marker", "This conversation is open in another app", "press R"]
