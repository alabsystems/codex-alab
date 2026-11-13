# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The rotation smokes' controllers, run whole against a stand-in host.

Nothing here says anything about aterm or codex: the host is
fixtures/rotation-host/stand_in_host.py, which plays a session against the controller's
own loopback provider. What is under test is the controllers' wiring, which no unit test
of their parts reaches: that a fixture fault ends a run at once, that a host without a
usable record or with a name aterm's snapshot refuses never starts, that the five-hour
case ends at its outcome and is judged again after its loop, that a report names the
modules that decided it, and that the matrix pins them.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

SMOKE = Path(__file__).resolve().parents[1] / "smoke"
sys.path.insert(0, str(SMOKE))
import capacity_exec as probe
import exec_accounts
import exec_shared_pool
import native_host_record
import provider_fixture
import renewal_follow as renewal
import renewal_matrix

STAND_IN = Path(__file__).resolve().parent / "fixtures" / "rotation-host" / "stand_in_host.py"
PASS = "PASS real-host shipping-qualified policy and wrong-token refusal; worker EOF joined"


class World:
    """A short temp directory with a stand-in package and recorded stand-in hosts."""

    def __init__(self, case):
        # Short: the smokes put sockets and certificates under their output.
        self.root = Path(case.enterContext(tempfile.TemporaryDirectory(dir="/tmp"))).resolve()
        self.package = self.root / "package"
        (self.package / "bin").mkdir(parents=True)
        (self.package / "bin" / "codex").write_text("#!/bin/sh\nexit 9\n")
        (self.package / "alab-build.json").write_text(json.dumps(
            {"version": "0.0.0-stand-in", "target": "stand-in", "source": "0" * 40}))
        (self.package / "codex-package.json").write_text(json.dumps(
            {"version": "0.0.0-stand-in", "target": "stand-in", "entrypoint": "bin/codex"}))

    def host(self, mode, name="aterm-stand-in-host", recorded=True, attestation="trial"):
        """A launcher that runs the stand-in in `mode`, with a record beside it."""
        clean = attestation == native_host_record.CLEAN
        directory = self.root / f"host-{mode}-{name}"
        directory.mkdir()
        host = directory / name
        host.write_text(f'#!/bin/sh\nexec "{sys.executable}" -B "{STAND_IN}" --mode {mode} "$@"\n')
        host.chmod(0o755)
        if recorded:
            (directory / "build.json").write_text(json.dumps({
                "schema": native_host_record.SCHEMA, "source": "1" * 40,
                "source_evidence": "a stand-in written by the unit test; no aterm build exists",
                "attestation": attestation, "builder": "the unit test" if clean else None,
                "built_at": "2026-11-12T09:30:00-08:00" if clean else None,
                "host_sha256": hashlib.sha256(host.read_bytes()).hexdigest(),
                "host_bytes": host.stat().st_size, "platform": "stand-in",
                "shipping_probe": {"ran": True, "returncode": 0, "verdict": PASS,
                                   "seconds": 0.0, "world": None}}))
        return host


def undecodable_usage(model, generation):
    """What the fixture served before provider_fixture.py: a window with no reset distance."""
    body = undecodable_usage.real(model, generation)
    del body["rate_limit"]["primary_window"]["reset_after_seconds"]
    return body


undecodable_usage.real = renewal.Model.usage_body


def run(main, name, *arguments):
    """Run a smoke's main() here, as its command line would, without its output."""
    with mock.patch.object(sys, "argv", [name, *map(str, arguments)]), mock.patch("builtins.print"):
        return main()


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class WallCaseRunTests(unittest.TestCase):
    """renewal_follow.py --case five-hour-wall-soon, whole, with its clocks shortened."""

    CASE = "five-hour-wall-soon"

    def setUp(self):
        self.world = World(self)
        # A run that reaches no outcome ends at 20 s here, not at 270 s.
        self.enterContext(mock.patch.dict(renewal.CASE_BUDGETS, {self.CASE: (20, 25)}))
        self.enterContext(mock.patch.object(renewal, "WALL_LIFT_SECONDS", 1))
        self.output = self.world.root / "out"

    def run_case(self, mode):
        started = time.monotonic()
        code = run(renewal.main, "renewal_follow.py", "--host", self.world.host(mode),
                   "--package", self.world.package, "--output", self.output,
                   "--case", self.CASE, "--under-guardian")
        return code, json.loads((self.output / "result.json").read_text()), time.monotonic() - started

    def test_the_case_runs_to_its_verdict_and_names_what_decided_it(self):
        code, report, seconds = self.run_case("wall")
        self.assertEqual((code, report["passed"], report.get("error")), (0, True, None))
        self.assertLess(seconds, 15)
        waited = report["fiveHourWallWaitedOut"]
        self.assertGreaterEqual(waited["waitedSeconds"], 1)
        self.assertGreaterEqual(waited["walledUsageReads"], 1)
        # The stand-in traces no control verb: how often a host was asked is unknown.
        self.assertIsNone(waited["preparesTraced"])
        self.assertEqual((report["case"], report["requestCount"], report["rawHostReturncode"]),
                         (self.CASE, 3, 0))
        self.assertEqual(report["requestAttribution"],
                         {"foreground": 3, "threadTitle": 0, "unknown": 0, "unroutedPosts": 0})
        # The report names the controller, both fixture modules and the host's attestation.
        self.assertEqual(report["controllerSha256"], sha256(renewal.__file__))
        self.assertEqual(report["fixtureSha256"], {
            "provider_fixture.py": sha256(provider_fixture.__file__),
            "native_host_record.py": sha256(native_host_record.__file__)})
        self.assertEqual((report["nativeHost"]["attestation"], report["nativeHost"]["qualifies"]),
                         ("trial", False))
        events = json.loads((self.output / "http-events.json").read_text())
        self.assertEqual([(event["account"], event["status"]) for event in events
                          if event["method"] == "POST"],
                         [("source", 200), ("source", 429), ("source", 200)])
        self.assertEqual({event["client"] for event in events}, {"codex"})

    def test_a_reply_the_client_cannot_decode_fails_the_run_at_once(self):
        with mock.patch.object(renewal.Model, "usage_body", undecodable_usage):
            code, report, seconds = self.run_case("wall")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertTrue(report["error"].startswith(
            "RuntimeError: fixture served a reply the client cannot decode on /api/codex/usage: "
            "FixtureFault: usage: primary_window.reset_after_seconds"), report["error"])
        # At the loop's next pass, not at the hang detector the stand-in would poll into.
        self.assertNotIn("hang detector", report["error"])
        self.assertLess(seconds, 10)

    def test_a_handoff_fails_the_run_by_name_while_it_runs(self):
        code, report, seconds = self.run_case("handoff")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertEqual(report["error"], "RuntimeError: a five-hour wall that resets soon was "
                         "handed over, charged or replayed: native action(s): signal")
        self.assertNotIn("requestCount", report)
        self.assertLess(seconds, 10)

    def test_a_handoff_after_the_controller_stopped_looking_fails_the_verdict(self):
        code, report, _ = self.run_case("late-handoff")
        self.assertEqual((code, report["passed"]), (1, False))
        # The loop saw the case end (it recorded the count); the verdict then refused it.
        self.assertEqual(report["requestCount"], 3)
        self.assertEqual(report["error"], "RuntimeError: a five-hour wall that resets soon was "
                         "handed over, charged or replayed: native action(s): signal")
        self.assertNotIn("fiveHourWallWaitedOut", report)

    def test_a_host_the_live_snapshot_would_refuse_or_one_unrecorded_never_starts(self):
        refused = {"named 'native_host'": self.world.host("wall", name="native_host"),
                   "native host build record missing": self.world.host("wall", recorded=False)}
        for message, host in refused.items():
            with self.subTest(message=message):
                with self.assertRaisesRegex(RuntimeError, message):
                    run(renewal.main, "renewal_follow.py", "--host", host, "--package",
                        self.world.package, "--output", self.output, "--case", self.CASE)
                self.assertFalse(self.output.exists())


class MatrixInputTests(unittest.TestCase):
    def test_the_matrix_pins_every_module_that_decides_a_run(self):
        world = World(self)
        host = world.host("exit", attestation=native_host_record.CLEAN)
        names = [path.name for path in renewal_matrix.pinned_inputs(host, world.package)]
        self.assertEqual(names, ["renewal_matrix.py", "renewal_follow.py", "capacity_exec.py",
            "provider_fixture.py", "native_host_record.py", "command_guard.py",
            "aterm-stand-in-host", "build.json", "codex", "alab-build.json", "codex-package.json"])
        # Everything the controller imports from its own directory is among them.
        imported = {Path(module.__file__).name for module in (
            renewal, probe, provider_fixture, native_host_record)}
        self.assertLessEqual(imported, set(names))
        # And a run writes them down: here one whose host says nothing and exits.
        output = world.root / "matrix"
        completed = subprocess.run([sys.executable, "-B", str(SMOKE / "renewal_matrix.py"),
            "--host", str(host), "--package", str(world.package), "--output", str(output),
            "--case", "weekly-quota"], capture_output=True, text=True, timeout=120)
        self.assertEqual(completed.returncode, 1, completed.stderr)
        manifest = json.loads((output / "qualification.json").read_text())
        self.assertEqual((manifest["passed"], manifest["error"]),
                         (False, "RuntimeError: paired case failed: weekly-quota"))
        self.assertEqual(manifest["inputs"], {str(path): sha256(path)
            for path in renewal_matrix.pinned_inputs(host, world.package)})
        result = json.loads((output / "weekly-quota" / "result.json").read_text())
        self.assertIn("native host exited before qualification completed", result["error"])


class ExecFixtureFaultTests(unittest.TestCase):
    """exec_accounts.py and exec_shared_pool.py end at a fixture fault, not at their deadline."""

    def setUp(self):
        self.world = World(self)
        self.enterContext(mock.patch.object(probe, "COMMAND_TIMEOUT_SECONDS",
                                            probe.COMMAND_TIMEOUT_SECONDS))
        self.enterContext(mock.patch.object(renewal.Model, "usage_body", undecodable_usage))

    def fails_on_the_fault(self, main, name, *more):
        output = self.world.root / "out"
        started = time.monotonic()
        with self.assertRaisesRegex(RuntimeError, "fixture served a reply the client cannot decode "
                                    "on /api/codex/usage: FixtureFault: usage: primary_window"):
            run(main, name, "--host", self.world.host("exec"), "--package", self.world.package,
                "--output", output, *more)
        # The stand-in lives 60 s and the smokes' own deadlines are 180 s and more.
        self.assertLess(time.monotonic() - started, 30)
        report = json.loads((output / "result.json").read_text())
        self.assertIs(report["passed"], False)
        self.assertIn("FixtureFault: usage: primary_window.reset_after_seconds", report["failure"])
        self.assertEqual((report["nativeHost"]["attestation"], report["nativeHost"]["qualifies"]),
                         ("trial", False))

    def test_the_exec_handoff_smoke_fails_at_once(self):
        self.fails_on_the_fault(exec_accounts.main, "exec_accounts.py")

    def test_the_shared_pool_smoke_fails_at_once(self):
        self.fails_on_the_fault(exec_shared_pool.main, "exec_shared_pool.py", "--sessions", "2")


class ExecHostRecordTests(unittest.TestCase):
    def test_a_host_without_a_record_is_refused_before_anything_is_created(self):
        world = World(self)
        host = world.host("exec", recorded=False)
        for main, name in ((exec_accounts.main, "exec_accounts.py"),
                           (exec_shared_pool.main, "exec_shared_pool.py")):
            with self.subTest(name=name):
                output = world.root / ("out-" + name)
                with self.assertRaisesRegex(RuntimeError, "native host build record missing"):
                    run(main, name, "--host", host, "--package", world.package, "--output", output)
                self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
