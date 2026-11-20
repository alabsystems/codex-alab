# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Observe the selected synthetic home's managed app server during a live resume."""
import hashlib
import json
import subprocess
import time
from contextlib import ExitStack
from pathlib import Path

import capacity_exec as fixture
from goal_observer_disconnect import SocketRpc, connect


SHARED_EMAIL = "shared@example.invalid"


class ManagedDaemon:
    def __init__(self, binary, root, env, binary_sha256, receipt):
        self.binary, self.root, self.env = binary, root, dict(env)
        self.binary_sha256, self.receipt = binary_sha256, receipt
        self.observer = ExitStack()
        self.home = (root / "codex-home").resolve(strict=True)
        fixture.require(self.home == root.resolve(strict=True) / "codex-home"
                        and Path(env["CODEX_HOME"]).resolve(strict=True) == self.home
                        and "CODEX_AUTH_PATH" not in env, "daemon requires the shared fixture auth.json")
        self.socket = self.home / "app-server-control/app-server-control.sock"
        fixture.require(not self.socket.exists() and not self.socket.is_symlink(),
                        "new fixture already has a daemon socket")

    def command(self, action):
        label = "daemon-" + action
        status = fixture.run(self.binary, ["app-server", "daemon", action], self.root, self.env, label)
        fixture.require(status == 0, f"daemon {action} exited {status}")
        result = json.loads((self.root / (label + "-stdout.jsonl")).read_text())
        self.receipt[action] = result
        fixture.require(Path(result["socketPath"]) == self.socket, "daemon returned a foreign socket")
        return result

    def start(self, login, thread):
        self.login = login
        result = self.command("start")
        fixture.require(result["status"] == "started" and result["backend"] == "pid"
                        and type(result["pid"]) is int and result["pid"] > 0 and self.socket.is_socket(),
                        "fresh managed daemon did not start")
        copied = Path(result["managedCodexPath"]).resolve(strict=True)
        fixture.require(copied.is_relative_to(self.home / "packages"), "daemon executable escaped the fixture")
        with copied.open("rb") as data:
            self.receipt["binary_sha256"] = hashlib.file_digest(data, "sha256").hexdigest()
        fixture.require(self.receipt["binary_sha256"] == self.binary_sha256,
                        "private daemon did not run the selected executable")
        wire = self.observer.enter_context((self.root / "daemon-observer.jsonl").open("x"))
        client = self.observer.enter_context(connect(self.socket))
        self.rpc = SocketRpc(client, wire, "managed-daemon")
        self.rpc.initialize()
        self.check_observer_account("initial")
        started = self.rpc.request("thread/start", {"ephemeral": True, "cwd": str(self.root / "work")})
        self.sentinel = started["thread"]["id"]
        self.receipt["observer_thread"] = self.sentinel
        self.loaded(thread, initial=True)

    def check_observer_account(self, phase):
        identity = self.rpc.request("account/read", {"refreshToken": False})
        account = identity["account"]
        workspace = (identity.get("workspaceRouting") or {}).get("chatgptAccountId")
        self.receipt[phase + "_account"] = {"email": account["email"], "workspace": workspace}
        fixture.require(account.get("type") == "chatgpt" and account["email"] == SHARED_EMAIL
                        and workspace == "workspace-" + self.login, "daemon observer's account changed")

    def loaded(self, thread, *, initial=False):
        label = "daemon-loaded-before" if initial else "daemon-loaded-during"
        result = self.rpc.request("thread/loaded/list", {})
        fixture.require(result.get("nextCursor") is None, "unexpected additional loaded threads")
        self.receipt[label] = result["data"]
        fixture.require(self.sentinel in result["data"], "daemon lost the original observer thread")
        present = thread in result["data"]
        if initial:
            fixture.require(not present, "fixture thread was already loaded on the daemon")
        else:
            self.receipt["resumed_thread_on_original_daemon"] = present
            if self.login == "destination":
                fixture.require(present, "same-login resume did not use its managed daemon")
            self.check_observer_account("after_resume")

    def witness_continuation(self, thread, gateway, prompt):
        self.loaded(thread)
        with gateway.lock:
            owners = [{"login": row["login"], "account": row["account"]} for row in gateway.requests
                      if row["thread"] == thread and prompt in row["history"]]
        self.receipt["continuation_owners"] = owners
        destination = {"login": "destination", "account": "workspace-destination"}
        fixture.require(owners and all(owner == destination for owner in owners),
                        "resumed turn used the wrong account while the original daemon was running")

    def stop(self):
        try:
            self.observer.close()
        finally:
            result = self.command("stop")
            fixture.require(result["status"] == "stopped" and not self.socket.exists(),
                            "managed daemon did not stop and release its socket")


class ImplicitSeedDaemon(ManagedDaemon):
    """Observe the seed TUI's own daemon without attaching to any of its threads."""

    def __init__(self, binary, root, env, binary_sha256, receipt):
        super().__init__(binary, root, env, binary_sha256, receipt)
        self.state = self.home / "app-server-daemon"
        self.state.mkdir(exist_ok=True)
        fixture.require(not any(self.state.iterdir()), "implicit seed daemon needs fresh private state")
        # Keep the supplied executable fixed and let normal startup choose the backend.
        fixture.write_json(self.state / "settings.json", {"updater": {"autoUpdateEnabled": False}})

    def note_live_seed(self):
        self.live_at = time.monotonic()

    def observe(self, thread, *, required):
        self.receipt["started_by_seed"] = self.socket.is_socket()
        if not self.receipt["started_by_seed"]:
            fixture.require(not required, "seed did not start a private daemon")
            return False
        owner = json.loads((self.state / "daemon.pid").read_bytes())
        fixture.require(type(owner["pid"]) is int and owner["pid"] > 0, "missing private daemon PID")
        self.receipt["seed_daemon_pid"] = owner["pid"]
        wire = self.observer.enter_context((self.root / "seed-daemon-observer.jsonl").open("x"))
        self.rpc = SocketRpc(self.observer.enter_context(connect(self.socket)), wire, "idle-writer")
        self.rpc.initialize()
        loaded = self.loaded_ids()
        self.first_list_at = time.monotonic()
        self.receipt.update(thread=thread, loaded_after_exit=loaded)
        fixture.require(thread in loaded or not required, "seed TUI exited without leaving its original thread loaded")
        return thread in loaded

    def loaded_ids(self):
        result = self.rpc.request("thread/loaded/list", {}, timeout=5)
        fixture.require(result.get("nextCursor") is None, "unexpected additional loaded-thread page")
        return result["data"]

    def retry_after_unload(self, thread, gateway, request_count, raw, process, send, displayed, marker):
        from model_resume import REPLY, SEED
        from picker_screen import visible_lines

        conflict = "This conversation is open in another app"
        displayed(conflict)

        def locked_view():
            screen = [line.rstrip() for line in visible_lines(raw.read_bytes()) if line.strip()]
            fixture.require(all(any(text in line for line in screen) for text in
                                (SEED, REPLY, marker, conflict, "press R")),
                            "B did not display A's saved transcript and the writer conflict together")
            return screen

        self.receipt["locked_view"] = locked_view()
        initial = self.loaded_ids()
        fixture.require(thread in initial, "writer already released before B displayed its read-only view")
        deadline = time.monotonic() + 85
        while process.poll() is None and time.monotonic() < deadline:
            loaded = self.loaded_ids()
            if thread not in loaded:
                break
            time.sleep(.5)
        else:
            raise RuntimeError("original thread never naturally unloaded while B waited")
        unloaded_at = time.monotonic()
        elapsed = unloaded_at - self.live_at
        fixture.require(elapsed >= 60, "writer released before the default idle delay")
        locked_view()
        with gateway.lock:
            fixture.require(len(gateway.requests) == request_count, "read-only B view ran inference")
        self.receipt.update(loaded_while_locked=initial, loaded_after_unload=loaded,
                            seconds_since_seed_was_live=elapsed, seconds_after_loaded_observation=unloaded_at - self.first_list_at,
                            locked_inference_requests=0)
        before = raw.stat().st_size
        send(b"r")
        displayed(REPLY, before)
        # A fresh repaint can initially still show the locked transcript.
        while process.poll() is None and time.monotonic() < deadline:
            view = visible_lines(raw.read_bytes(), after=before)
            if view and any(REPLY in line for line in view) and all(conflict not in line for line in view):
                return
            time.sleep(.05)
        raise RuntimeError("R did not reopen the conversation in B's original terminal")

    @staticmethod
    def process_start(pid):
        query = subprocess.run(["ps", "-p", str(pid), "-o", "lstart="], text=True, capture_output=True)
        fixture.require(query.returncode in (0, 1), "could not inspect the private daemon process")
        return query.stdout.strip() or None

    def stop(self):
        try:
            self.observer.close()
        finally:
            self.stop_private_daemon()

    def stop_private_daemon(self):
        # Capture the OS process identities before asking the home-scoped daemon manager to stop.
        # A failed launch can leave a reserved, empty pidfile. Still run normal cleanup in that case.
        owners = {}
        for path in self.state.glob("*.pid"):
            try:
                record = json.loads(path.read_bytes())
            except (FileNotFoundError, json.JSONDecodeError):
                continue
            pid = record["pid"]
            fixture.require(type(pid) is int and pid > 0, "invalid private daemon PID")
            born = self.process_start(pid)
            if born:
                owners[path.name] = (pid, born)
        if self.socket.exists() or self.socket.is_symlink() or any(self.state.glob("*.pid")):
            try:
                if (self.state / "daemon-updater.pid").exists():
                    self.command("restart")  # Apply the original private autoUpdateEnabled=false setting.
            finally:
                result = self.command("stop")
            fixture.require(result["status"] in ("stopped", "notRunning"), "private seed daemon did not stop")
            if self.receipt.get("started_by_seed"):
                copied = Path(result["managedCodexPath"]).resolve(strict=True)
                fixture.require(result["status"] == "stopped" and copied.is_relative_to(self.home / "packages"),
                                "observed daemon was not the fixture's installed copy")
                with copied.open("rb") as data:
                    digest = hashlib.file_digest(data, "sha256").hexdigest()
                fixture.require(digest == self.binary_sha256, "private daemon executable differs from seed")
                self.receipt["package_sha256"] = digest
        deadline = time.monotonic() + 5
        while any(self.process_start(pid) == born for pid, born in owners.values()):
            fixture.require(time.monotonic() < deadline, "original private daemon or updater process remained alive")
            time.sleep(.05)
        fixture.require(not self.socket.is_socket() and not any(self.state.glob("*.pid")),
                        "private seed daemon or updater remained active")
        self.receipt.update(retired=True, retired_processes=owners)
