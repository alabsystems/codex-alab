#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise exact endpoint selection and connection-owned build lifetime."""

import importlib.util
import hashlib
import json
import os
from pathlib import Path
import socket
import stat
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("alab_trust_memory", ROOT / "alab/trust_memory.py")
MEMORY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MEMORY)
FIXTURE_EXECUTABLE = Path(sys.executable).resolve()
IDENTITY = {"version": "trustd.identity.v1", "protocol": "trustd.status.v1", "release": "fixture",
            "commit": "a" * 40, "executable_sha256": hashlib.sha256(FIXTURE_EXECUTABLE.read_bytes()).hexdigest()}


class Daemon:
    def __init__(self, root, *, identity=IDENTITY, budget=100, before_publish=None):
        self.path = root / "host.sock"
        self.held = root / "lease-held"
        self.released = threading.Event()
        self.requests = []
        self.identity, self.budget = identity, budget
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        # Match Trust's stage_private_socket: the rendezvous path never exposes
        # a bind/chmod interval. Strict validation must refuse a published broad
        # socket, even when a compatible process is still starting.
        with tempfile.TemporaryDirectory(prefix=".td-", dir=root) as staging:
            staged = Path(staging) / "s"
            self.listener.bind(str(staged))
            staged.chmod(0o600)
            self.listener.listen()
            if before_publish is not None:
                before_publish(staged)
            staged.rename(self.path)
        self.worker = threading.Thread(target=self.serve, daemon=True)
        self.worker.start()

    def serve(self):
        try:
            connection, _ = self.listener.accept()
            with connection, connection.makefile("rb") as stream:
                for line in stream:
                    request = line.decode().strip()
                    self.requests.append(request)
                    if request == "IDENTITY":
                        reply = json.dumps(self.identity)
                    elif request == "STATUS":
                        reply = json.dumps({"version": "trustd.status.v1", "budget_bytes": self.budget,
                                            "reserved_bytes": 0})
                    elif request == "LEASE":
                        self.held.write_text("held\n")
                        reply = "LEASED"
                    else:
                        reply = "ERR unknown"
                    connection.sendall(reply.encode() + b"\n")
        finally:
            self.held.unlink(missing_ok=True)
            self.released.set()
            self.listener.close()


class MemoryAuthorityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="alab-memory-", dir="/tmp")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.root.chmod(0o700)

    def test_identity_status_and_lease_share_one_connection_until_release(self):
        daemon = Daemon(self.root)
        lease = MEMORY.lease_at(daemon.path, IDENTITY, FIXTURE_EXECUTABLE)
        self.assertEqual(daemon.requests, ["IDENTITY", "STATUS", "LEASE"])
        self.assertTrue(daemon.held.exists())
        self.assertFalse(daemon.released.is_set())
        lease.close()
        self.assertTrue(daemon.released.wait(2))
        self.assertFalse(daemon.held.exists())

    def test_endpoint_is_absent_until_private_socket_is_atomically_published(self):
        endpoint = self.root / "host.sock"

        def inspect_staged(staged):
            self.assertFalse(endpoint.exists())
            self.assertEqual(stat.S_IMODE(staged.parent.stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(staged.stat().st_mode), 0o600)
            with self.assertRaises(FileNotFoundError):
                MEMORY.lease_at(endpoint, IDENTITY, FIXTURE_EXECUTABLE)

        daemon = Daemon(self.root, before_publish=inspect_staged)
        self.assertEqual(stat.S_IMODE(endpoint.stat().st_mode), 0o600)
        lease = MEMORY.lease_at(endpoint, IDENTITY, FIXTURE_EXECUTABLE)
        lease.close()
        self.assertTrue(daemon.released.wait(2))

    def test_published_broad_socket_is_a_definite_refusal_without_spawn(self):
        daemon = Daemon(self.root)
        daemon.path.chmod(0o666)
        with patch.object(MEMORY, "host_endpoint", return_value=daemon.path), \
                patch.object(MEMORY, "selected_identity", return_value=(FIXTURE_EXECUTABLE, IDENTITY)), \
                patch.object(MEMORY.subprocess, "Popen") as spawn:
            with self.assertRaisesRegex(ValueError, "private socket"):
                MEMORY.prepare(self.root)
        spawn.assert_not_called()
        self.assertEqual(daemon.requests, [])
        daemon.path.chmod(0o600)
        connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        connection.connect(str(daemon.path))
        connection.close()
        self.assertTrue(daemon.released.wait(2))

    def test_foreign_build_or_unusable_budget_cannot_take_a_lease(self):
        for name, identity, budget in (("foreign", {**IDENTITY, "commit": "f" * 40}, 100),
                                       ("disabled", IDENTITY, 0)):
            with self.subTest(name=name):
                root = self.root / name
                root.mkdir(mode=0o700)
                daemon = Daemon(root, identity=identity, budget=budget)
                with self.assertRaises(ValueError):
                    MEMORY.lease_at(daemon.path, IDENTITY, FIXTURE_EXECUTABLE)
                self.assertTrue(daemon.released.wait(2))
                self.assertNotIn("LEASE", daemon.requests)

    def test_profile_data_cannot_select_another_owned_socket_or_alias(self):
        daemon = Daemon(self.root)
        with patch.object(MEMORY, "host_endpoint", return_value=daemon.path):
            self.assertEqual(MEMORY.profile_socket(str(daemon.path)), daemon.path)
            for path in (self.root / "other.sock", Path("relative.sock")):
                with self.subTest(path=path), self.assertRaises(ValueError):
                    MEMORY.profile_socket(str(path))
        # End this private test server without touching any real authority.
        connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        connection.connect(str(daemon.path))
        connection.close()
        self.assertTrue(daemon.released.wait(2))

    def test_actual_child_runs_while_keeper_holds_lease_and_failure_is_preserved(self):
        daemon = Daemon(self.root)
        lease = MEMORY.lease_at(daemon.path, IDENTITY, FIXTURE_EXECUTABLE)
        context = self.root / "native-context"
        context.write_bytes(b"inherited controller descriptor")
        descriptor = os.open(context, os.O_RDONLY)
        self.addCleanup(os.close, descriptor)
        os.set_inheritable(descriptor, True)
        command = [sys.executable, "-I", "-B", "-S", "-c",
                   "import os,sys; from pathlib import Path; "
                   "assert Path(sys.argv[1]).is_file(); "
                   "assert os.environ['ALAB_TRUSTD_SOCKET']==sys.argv[2]; "
                   "assert 'TRUST_MEMORY_JOBSERVER_SOCK' not in os.environ; "
                   "assert os.read(int(sys.argv[3]),100)==b'inherited controller descriptor'; sys.exit(17)",
                   str(daemon.held), str(daemon.path), str(descriptor)]
        with patch.dict(os.environ, {"ATPKG_PROJECT_EXECUTION": str(self.root)}), \
                patch.object(MEMORY, "prepare", return_value=(daemon.path, lease)):
            self.assertEqual(MEMORY.run(command), 17)
        self.assertTrue(daemon.released.wait(2))

    def test_memory_domain_overrides_are_refused_before_selecting_or_spawning(self):
        for name in ("TRUSTD_DISABLE", "TRUSTD_RUNTIME_DIR", "TRUST_MEMORY_JOBSERVER", "TRUST_MEMORY_JOBSERVER_SOCK"):
            with self.subTest(name=name), patch.dict(os.environ, {name: ""}), \
                    patch.object(MEMORY, "selected_identity") as identity:
                with self.assertRaises(ValueError):
                    MEMORY.prepare(self.root)
                identity.assert_not_called()

    def test_same_user_listener_cannot_replay_another_executables_identity(self):
        other = Path("/bin/sh").resolve()
        replay = {**IDENTITY, "executable_sha256": hashlib.sha256(other.read_bytes()).hexdigest()}
        daemon = Daemon(self.root, identity=replay)
        with self.assertRaisesRegex(ValueError, "another executable"):
            MEMORY.lease_at(daemon.path, replay, other)
        self.assertTrue(daemon.released.wait(2))
        self.assertNotIn("LEASE", daemon.requests)

    def test_missing_kernel_peer_identity_never_falls_back_to_metadata(self):
        daemon = Daemon(self.root)
        with patch.object(MEMORY, "peer_process", side_effect=ValueError("unavailable peer identity")):
            with self.assertRaisesRegex(ValueError, "unavailable peer identity"):
                MEMORY.lease_at(daemon.path, IDENTITY, FIXTURE_EXECUTABLE)
        self.assertTrue(daemon.released.wait(2))
        self.assertNotIn("LEASE", daemon.requests)

    def test_losing_startup_child_can_adopt_the_compatible_winner_after_publication(self):
        endpoint = self.root / "host.sock"
        published = []

        class LostRace:
            def poll(self):
                return 1

            def wait(self):
                return 1

        def winner():
            time.sleep(0.05)
            published.append(Daemon(self.root))

        def spawn(*args, **kwargs):
            self.assertEqual(args[0], [str(FIXTURE_EXECUTABLE)])
            self.assertEqual(kwargs["env"], {})
            self.assertEqual(kwargs["process_group"], 0)
            kwargs["stderr"].write(b"another trustd owns the endpoint\n")
            kwargs["stderr"].flush()
            threading.Thread(target=winner, daemon=True).start()
            return LostRace()

        with patch.object(MEMORY, "host_endpoint", return_value=endpoint), \
                patch.object(MEMORY, "selected_identity", return_value=(FIXTURE_EXECUTABLE, IDENTITY)), \
                patch.object(MEMORY.subprocess, "Popen", side_effect=spawn) as spawn_call:
            path, lease = MEMORY.prepare(self.root)
        self.assertEqual(path, endpoint)
        self.assertEqual(spawn_call.call_count, 1)
        self.assertEqual(published[0].requests, ["IDENTITY", "STATUS", "LEASE"])
        lease.close()
        self.assertTrue(published[0].released.wait(2))

    def test_connected_peer_identity_disappearance_never_spawns_a_replacement(self):
        daemon = Daemon(self.root)
        with patch.object(MEMORY, "host_endpoint", return_value=daemon.path), \
                patch.object(MEMORY, "selected_identity", return_value=(FIXTURE_EXECUTABLE, IDENTITY)), \
                patch.object(MEMORY, "peer_process", side_effect=FileNotFoundError(2, "peer executable gone")), \
                patch.object(MEMORY.subprocess, "Popen") as spawn:
            with self.assertRaisesRegex(ValueError, "connected Trust memory peer"):
                MEMORY.prepare(self.root)
        spawn.assert_not_called()
        self.assertTrue(daemon.released.wait(2))


if __name__ == "__main__":
    unittest.main()
