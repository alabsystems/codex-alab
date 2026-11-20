# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Renewal tail/API/entry controls using inert records and no host processes."""
import ast
from contextlib import contextmanager, ExitStack, redirect_stderr, redirect_stdout
import io
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import test_rotation_smoke_runs as rotation


RENEWAL = rotation.renewal
TRACES = ("requests.json", "auxiliary-requests.json", "unknown-requests.json",
          "unrouted-posts.json", "http-events.json")


class RenewalTailTests(unittest.TestCase):
    @contextmanager
    def fixture(self, primary, *, failures=None, summary_failure=None):
        failures = failures or {}
        with ExitStack() as owned:
            temporary = tempfile.TemporaryDirectory
            parent = Path(tempfile.gettempdir()) / ".ubd"
            parent.mkdir(exist_ok=True)
            with mock.patch.object(rotation.tempfile, "TemporaryDirectory", side_effect=lambda **_kwargs:
                                   temporary(prefix="renewal-tail-", dir=parent)):
                world = rotation.World(SimpleNamespace(enterContext=owned.enter_context))
            host, output = world.host("never-executed"), world.root / "output"
            actions = []

            def action(name):
                def call():
                    actions.append(name)
                    if name in failures:
                        raise failures[name]
                return call

            def pair(name):
                return (SimpleNamespace(server_port=1234, shutdown=action(name + ".shutdown"),
                                        server_close=action(name + ".close")),
                        SimpleNamespace(join=action(name + ".join")))

            touch, save = Path.touch, RENEWAL.probe.write_json

            def touch_stop(path, *args, **kwargs):
                if path == output / "stop":
                    action("stop")()
                return touch(path, *args, **kwargs)

            def write(path, value):
                if path.parent == output:
                    if path.name == "result.json":
                        actions.append("report")
                        if summary_failure is not None:
                            raise summary_failure
                    elif path.name in TRACES:
                        action("export." + path.name)()
                return save(path, value)

            arguments = ["renewal_follow.py", "--host", str(host), "--package", str(world.package),
                         "--output", str(output), "--case", "lost-login-expired-plain-issuer", "--under-guardian"]
            with mock.patch.object(sys, "argv", arguments), \
                    mock.patch.object(RENEWAL, "serve", side_effect=[pair("model"), pair("issuer")]), \
                    mock.patch.object(RENEWAL.probe, "configure", side_effect=primary), \
                    mock.patch.object(Path, "touch", touch_stop), \
                    mock.patch.object(RENEWAL.probe, "write_json", side_effect=write), \
                    mock.patch.object(RENEWAL.subprocess, "Popen", side_effect=AssertionError("host start forbidden")) as popen, \
                    mock.patch.object(RENEWAL.subprocess, "run", side_effect=AssertionError("child start forbidden")) as run, \
                    mock.patch.object(RENEWAL.native_host_record, "load", wraps=RENEWAL.native_host_record.load) as load, \
                    mock.patch("builtins.print"), redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                yield SimpleNamespace(world=world, output=output, actions=actions, failures=failures, load=load)
                popen.assert_not_called()
                run.assert_not_called()

    def capture(self, *, entry=False):
        try:
            if not entry:
                return None, RENEWAL.main()
            source = Path(RENEWAL.__file__)
            tree = ast.parse(source.read_bytes(), filename=str(source))
            entries = [node for node in tree.body if isinstance(node, ast.If)
                       and isinstance(node.test, ast.Compare) and isinstance(node.test.left, ast.Name)
                       and node.test.left.id == "__name__"]
            self.assertEqual(len(entries), 1)
            with mock.patch.object(RENEWAL, "__name__", "__main__"):
                exec(compile(ast.Module(body=entries, type_ignores=[]), str(source), "exec"), RENEWAL.__dict__)
        except BaseException as error:
            return error, None
        self.fail("the original entry did not exit")

    def assert_cleanup(self, fixture):
        expected = ["stop"]
        for name in ("model", "issuer"):
            expected.append(name + ".shutdown")
            if name + ".shutdown" not in fixture.failures:
                expected.append(name + ".join")
            expected.append(name + ".close")
        expected.extend("export." + name for name in TRACES)
        actual = [action for action in fixture.actions if action != "report"]
        self.assertEqual(actual, expected)
        for name in TRACES:
            self.assertEqual((fixture.output / name).exists(), "export." + name not in fixture.failures)
        if "report" in fixture.actions:
            self.assertEqual(fixture.actions[-1], "report", "the verdict preceded required cleanup")

    def report(self, fixture):
        report = json.loads((fixture.output / "result.json").read_text())
        self.assertFalse(report["passed"])
        self.assertFalse(report["nativeHost"]["qualifies"])
        self.assertFalse(report["package"]["qualifies"])
        return report

    def test_cancellation_is_reported_and_retains_its_exact_api_identity(self):
        for primary in (KeyboardInterrupt("original"), SystemExit(0), SystemExit(47)):
            with self.subTest(primary=type(primary).__name__), self.fixture(primary) as fixture:
                caught, _ = self.capture()
                self.assertIs(caught, primary)
                self.assertTrue(self.report(fixture)["interrupted"])
                self.assert_cleanup(fixture)

    def test_every_failed_stop_listener_step_and_trace_preserves_cancellation(self):
        phases = ("stop", "model.shutdown", "model.join", "model.close", "issuer.shutdown", "issuer.join", "issuer.close",
                  *("export." + name for name in TRACES))
        for phase in phases:
            primary = KeyboardInterrupt("original cancellation")
            with self.subTest(phase=phase), self.fixture(primary, failures={phase: OSError("cleanup " + phase)}) as fixture:
                caught, _ = self.capture()
                self.assertIs(caught, primary)
                report = self.report(fixture)
                self.assertTrue(report["interrupted"])
                self.assertIn("cleanup " + phase, json.dumps(report))
                self.assert_cleanup(fixture)

    def test_ordinary_body_failure_and_all_cleanup_failures_keep_a_failed_report(self):
        failures = {name: OSError("cleanup " + name)
                    for name in ("model.shutdown", "issuer.close", "export.requests.json")}
        with self.fixture(RuntimeError("original body failure"), failures=failures) as fixture:
            caught, status = self.capture()
            self.assertIsNone(caught)
            self.assertEqual(status, 1)
            report = self.report(fixture)
            self.assertEqual(report["error"], "RuntimeError: original body failure")
            self.assertNotIn("interrupted", report)
            for name in failures:
                self.assertIn("cleanup " + name, json.dumps(report))
            self.assert_cleanup(fixture)

    def test_cleanup_cancellation_outranks_ordinary_body_failure(self):
        for stop in (KeyboardInterrupt("cleanup stopped"), SystemExit(0)):
            with self.subTest(stop=type(stop).__name__), \
                    self.fixture(RuntimeError("body failed"), failures={"model.close": stop}) as fixture:
                caught, _ = self.capture()
                self.assertIs(caught, stop)
                report = self.report(fixture)
                self.assertTrue(report["interrupted"])
                self.assertIn("body failed", json.dumps(report))
                self.assert_cleanup(fixture)

    def test_summary_write_failure_cannot_replace_selected_cancellation(self):
        for primary in (KeyboardInterrupt("original"), SystemExit(0)):
            for failure in (OSError("report write failed"), SystemExit(256)):
                with self.subTest(primary=type(primary).__name__, failure=type(failure).__name__), \
                        self.fixture(primary, summary_failure=failure) as fixture:
                    caught, _ = self.capture()
                    self.assertIs(caught, primary)
                    self.assertIs(caught.__context__, failure)
                    self.assertFalse((fixture.output / "result.json").exists())
                    self.assert_cleanup(fixture)

    def test_original_entry_refuses_success_shaped_interruptions_and_keeps_other_exits(self):
        for primary, status in ((SystemExit(0), 1), (SystemExit(None), 1), (SystemExit(False), 1),
                                (SystemExit(256), 1), (SystemExit(-256), 1), (SystemExit(47), 47),
                                (KeyboardInterrupt("original"), None)):
            with self.subTest(primary=repr(primary)), self.fixture(primary) as fixture:
                caught, _ = self.capture(entry=True)
                if status == 1:
                    self.assertIsInstance(caught, SystemExit)
                    self.assertEqual(caught.code, 1)
                    self.assertIsNot(caught, primary)
                else:
                    self.assertIs(caught, primary)
                self.assertTrue(self.report(fixture)["interrupted"])
                self.assert_cleanup(fixture)

    def test_help_and_argument_errors_do_not_begin_an_observation(self):
        for arguments, code in ((["--help"], 0), ([], 2)):
            with self.subTest(arguments=arguments), self.fixture(KeyboardInterrupt("must not run")) as fixture:
                with mock.patch.object(sys, "argv", ["renewal_follow.py", *arguments]):
                    caught, _ = self.capture(entry=True)
                self.assertIsInstance(caught, SystemExit)
                self.assertEqual(caught.code, code)
                fixture.load.assert_not_called()
                self.assertFalse(fixture.output.exists())
                self.assertEqual(fixture.actions, [])

    def test_lost_login_quota_conflict_keeps_its_parse_error_before_observation(self):
        for entry in (False, True):
            with self.subTest(entry=entry), self.fixture(KeyboardInterrupt("must not run")) as fixture:
                with mock.patch.object(sys, "argv", [*sys.argv, "--quota"]):
                    caught, _ = self.capture(entry=entry)
                self.assertIsInstance(caught, SystemExit)
                self.assertEqual(caught.code, 2)
                fixture.load.assert_not_called()
                self.assertFalse(fixture.output.exists())
                self.assertEqual(fixture.actions, [])

    def test_matrix_pins_the_loaded_cli_policy_without_starting_a_host(self):
        with self.fixture(RuntimeError("not run")) as fixture:
            inputs = rotation.renewal_matrix.pinned_inputs(fixture.world.host("other-inert"), fixture.world.package)
            self.assertIn(Path(RENEWAL.__file__).resolve().parents[1] / "cli_status.py", inputs)
            self.assertIn(Path(RENEWAL.FILE_HASH["__file__"]).resolve(), inputs)
            self.assertEqual(fixture.actions, [])

    def test_multi_listener_success_and_failed_exports_keep_truthful_verdicts(self):
        for failure in (None, "model.shutdown", "first-export"):
            with self.subTest(failure=failure):
                actions = []

                def action(name):
                    def call():
                        actions.append(name)
                        if name == failure:
                            raise OSError("cleanup " + name)
                    return call

                listeners = tuple((name, SimpleNamespace(shutdown=action(name + ".shutdown"),
                                   server_close=action(name + ".close")),
                                   SimpleNamespace(join=action(name + ".join")))
                                  for name in ("model", "issuer"))
                result = {"passed": True}
                RENEWAL.probe.cleanup_resources(listeners, (("first", action("first-export")),
                    ("second", action("second-export"))), result, None, stop=action("stop"))
                expected = ["stop", "model.shutdown"]
                if failure != "model.shutdown":
                    expected.append("model.join")
                expected.extend(["model.close", "issuer.shutdown", "issuer.join", "issuer.close",
                                 "first-export", "second-export"])
                self.assertEqual(actions, expected)
                self.assertEqual(result["passed"], failure is None)
                if failure is not None:
                    self.assertIn("cleanup " + failure, json.dumps(result))

    def test_outer_guardian_refuses_missing_or_failed_case_records_on_zero_exit(self):
        for scenario, case_passed, case_exists, guardian_exists, timed_out, status, expected in (
                ("failed-case", False, True, True, False, 0, 1),
                ("missing-case", True, False, True, False, 0, 1),
                ("missing-guardian", True, True, False, False, 0, 1),
                ("guardian-timeout", True, True, True, True, 0, 1),
                ("passed-control", True, True, True, False, 0, 0),
                ("nonzero-control", True, True, True, False, 47, 47)):
            with self.subTest(scenario=scenario), self.fixture(RuntimeError("must not run")) as fixture:
                def completed(command):
                    self.assertIn("--under-guardian", command)
                    fixture.output.mkdir()
                    if case_exists:
                        (fixture.output / "result.json").write_text(json.dumps({"passed": case_passed}))
                    if guardian_exists:
                        fixture.output.with_name(fixture.output.name + "-guardian-result.json").write_text(
                            json.dumps({"returncode": status, "timed_out": timed_out}))
                    return SimpleNamespace(returncode=status)

                arguments = [value for value in sys.argv if value != "--under-guardian"]
                with mock.patch.object(sys, "argv", arguments), \
                        mock.patch.object(RENEWAL.subprocess, "run", side_effect=completed) as controller:
                    caught, returned = self.capture()
                self.assertIsNone(caught)
                self.assertEqual(returned, expected)
                controller.assert_called_once()
                self.assertEqual(fixture.actions, [])

    def test_matrix_detects_real_cli_helper_mutation_in_its_source_manifest(self):
        self.matrix_helper_mutation("cli_status.py")

    def test_matrix_detects_real_hash_helper_mutation_in_its_source_manifest(self):
        self.matrix_helper_mutation("file_hash.py")

    def matrix_helper_mutation(self, helper_name):
        with self.fixture(RuntimeError("not run")) as fixture:
            fixture.world.package_records(rotation.NATIVE_LAYOUT)
            host = fixture.world.host("record-only", attestation=rotation.native_host_record.CLEAN)
            matrix_source = Path(rotation.renewal_matrix.__file__).resolve()
            source_alab = matrix_source.parents[1]
            copied_alab = fixture.world.root / "copied-controller/alab"
            sources = rotation.renewal_matrix.pinned_inputs(host, fixture.world.package)
            for source in sources:
                if source.is_relative_to(source_alab):
                    target = copied_alab / source.relative_to(source_alab)
                    target.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(source, target)
            spec = importlib.util.spec_from_file_location("renewal_matrix_mutation", copied_alab / "smoke/renewal_matrix.py")
            matrix = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(matrix)
            helper = copied_alab / helper_name

            def mutate(_command, **_kwargs):
                helper.write_text(helper.read_text() + "\n# controlled helper mutation\n")
                return SimpleNamespace(returncode=0)

            output = fixture.world.root / "matrix-output"
            with mock.patch.object(sys, "argv", ["renewal_matrix.py", "--host", str(host), "--package",
                    str(fixture.world.package), "--output", str(output), "--case", "weekly-quota"]), \
                    mock.patch.object(matrix.subprocess, "run", side_effect=mutate) as controller:
                self.assertEqual(matrix.main(), 1)
            report = json.loads((output / "qualification.json").read_text())
            self.assertFalse(report["passed"])
            self.assertEqual(report["cases"], [])
            self.assertEqual(report["error"], "RuntimeError: qualification source or image changed during matrix")
            self.assertNotEqual(report["inputs"][str(helper)], matrix.digest(helper))
            controller.assert_called_once()
            self.assertEqual(fixture.actions, [])

    def test_outer_controller_refuses_incomplete_or_mistyped_receipts(self):
        valid = {"returncode": 0, "timed_out": False}
        for case_passed, guardian in (
                (True, {"returncode": 0}), (True, {"returncode": 0, "timed_out": None}),
                (True, {"returncode": False, "timed_out": False}),
                (True, {"returncode": 0.0, "timed_out": False}),
                (True, {"returncode": "0", "timed_out": False}),
                (True, {"timed_out": False}), (True, None), (True, []),
                (1, valid), ("yes", valid), (None, valid)):
            with self.subTest(case_passed=case_passed, guardian=guardian), self.fixture(RuntimeError("not run")) as fixture:
                def completed(_command):
                    fixture.output.mkdir()
                    (fixture.output / "result.json").write_text(json.dumps({"passed": case_passed}))
                    fixture.output.with_name(fixture.output.name + "-guardian-result.json").write_text(json.dumps(guardian))
                    return SimpleNamespace(returncode=0)

                with mock.patch.object(sys, "argv", [value for value in sys.argv if value != "--under-guardian"]), \
                        mock.patch.object(RENEWAL.subprocess, "run", side_effect=completed):
                    caught, status = self.capture()
                self.assertIsNone(caught)
                self.assertEqual(status, 1)
                self.assertIs(json.loads((fixture.output / "result.json").read_text())["passed"], False)
                self.assertEqual(fixture.actions, [])


if __name__ == "__main__":
    unittest.main()
