#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Prerequisite orchestration with inert unittest fixtures; no Cargo or native calls."""

from contextlib import contextmanager, redirect_stderr, redirect_stdout
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
from types import ModuleType, SimpleNamespace
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "alab_python_workflow", Path(__file__).resolve().parents[1] / "python_tests.py")
WORKFLOW = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(WORKFLOW)


class PythonWorkflowTests(unittest.TestCase):
    @contextmanager
    def fixture(self):
        events = []

        def method(label):
            def run(_case):
                events.append(label)
            return run

        native_module = ModuleType("python_workflow_fake_native")
        pure_module = ModuleType("python_workflow_fake_pure")
        native = type("NativeCases", (unittest.TestCase,), {
            "__module__": native_module.__name__,
            "test_one": method("native one"),
            "test_two": method("native two"),
            "test_three": method("native three"),
        })
        pure = type("PureCases", (unittest.TestCase,), {
            "__module__": pure_module.__name__,
            "test_one": method("pure one"),
            "test_two": method("pure two"),
        })
        native_module.NativeCases = native
        native_name = f"{native.__module__}.{native.__qualname__}"
        required = tuple(native_name + ".test_" + name for name in ("one", "two", "three"))
        with mock.patch.dict(sys.modules, {
            native_module.__name__: native_module, pure_module.__name__: pure_module,
        }), mock.patch.object(WORKFLOW, "NATIVE_CLASS", native_name), \
                mock.patch.object(WORKFLOW, "REQUIRED_NATIVE", required):
            yield SimpleNamespace(events=events, native=native, pure=pure,
                                  native_module=native_module, pure_module=pure_module,
                                  required=required)

    def cases(self, fixture):
        return [fixture.native("test_one"), fixture.native("test_two"),
                fixture.native("test_three"), fixture.pure("test_one"), fixture.pure("test_two")]

    def output(self, action):
        stdout, stderr = io.StringIO(), io.StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            status = action(stdout, stderr)
        lines = stdout.getvalue().splitlines()
        self.assertEqual(len(lines), 2, stdout.getvalue())
        self.assertTrue(lines[0].startswith(WORKFLOW.MANIFEST_PREFIX))
        self.assertTrue(lines[1].startswith(WORKFLOW.SUMMARY_PREFIX))
        return SimpleNamespace(
            status=status,
            manifest=json.loads(lines[0][len(WORKFLOW.MANIFEST_PREFIX):]),
            summary=json.loads(lines[1][len(WORKFLOW.SUMMARY_PREFIX):]),
            stdout=stdout.getvalue(), stderr=stderr.getvalue(),
        )

    def run_cases(self, cases, **kwargs):
        return self.output(lambda out, err: WORKFLOW._run_cases(
            cases, stdout=out, stderr=err, **kwargs))

    def test_entire_native_class_runs_first_with_stable_order_and_cleanup(self):
        with self.fixture() as f:
            def native_module_setup():
                f.events.append("native module setup")
                unittest.addModuleCleanup(f.events.append, "native module cleanup")

            def native_class_setup(cls):
                f.events.append("native class setup")
                cls.addClassCleanup(f.events.append, "native class cleanup")

            f.native_module.setUpModule = native_module_setup
            f.native_module.tearDownModule = lambda: f.events.append("native module teardown")
            f.native.setUpClass = classmethod(native_class_setup)
            f.native.tearDownClass = classmethod(lambda cls: f.events.append("native class teardown"))
            f.native.test_extra = lambda case: f.events.append("native extra")
            cases = [f.pure("test_two"), f.native("test_three"), f.native("test_extra"),
                     f.pure("test_one"), f.native("test_one"), f.native("test_two")]
            expected_ids = [case.id() for case in cases]
            result = self.run_cases(cases)
            self.assertEqual(f.events, [
                "native module setup", "native class setup", "native three", "native extra",
                "native one", "native two", "native class teardown", "native class cleanup",
                "native module teardown", "native module cleanup", "pure two", "pure one",
            ])
        self.assertEqual(result.status, 0)
        self.assertTrue(result.summary["success"])
        self.assertTrue(result.summary["full_scope"])
        self.assertEqual(result.manifest["case_ids"], expected_ids)
        self.assertEqual(result.manifest["native_ids"], [expected_ids[i] for i in (1, 2, 4, 5)])
        self.assertEqual(result.summary["started_ids"],
                         [expected_ids[i] for i in (1, 2, 4, 5, 0, 3)])
        self.assertEqual(result.summary["started_ids"], result.summary["finished_ids"])
        self.assertEqual(result.summary["passed"], 6)
        self.assertEqual(result.summary["tests_run"], 6)
        self.assertEqual(result.summary["missing_ids"], [])
        digest = hashlib.sha256(json.dumps({
            "case_ids": expected_ids, "native_ids": result.manifest["native_ids"],
        }, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        self.assertEqual(result.manifest["digest"], digest)
        self.assertEqual(result.summary["manifest_digest"], digest)

    def test_native_setup_error_cleans_up_without_starting_cases_or_pure(self):
        with self.fixture() as f:
            def setup(cls):
                f.events.append("native setup")
                cls.addClassCleanup(f.events.append, "native setup cleanup")
                raise RuntimeError("fixture unavailable")

            f.native.setUpClass = classmethod(setup)
            result = self.run_cases(self.cases(f))
            self.assertEqual(f.events, ["native setup", "native setup cleanup"])
            self.assertEqual(result.summary["phases"]["native"]["missing_ids"], list(f.required))
        self.assertEqual(result.status, 1)
        self.assertEqual(result.summary["errors"], 1)
        self.assertEqual(result.summary["tests_run"], 0)
        self.assertFalse(result.summary["full_scope"])
        self.assertFalse(result.summary["phases"]["pure"]["executed"])
        self.assertIn("setUpClass", result.stderr)

    def test_native_cleanup_error_blocks_pure_after_all_native_methods_pass(self):
        with self.fixture() as f:
            def cleanup():
                f.events.append("native module cleanup")
                raise RuntimeError("cleanup refused")

            f.native_module.setUpModule = lambda: unittest.addModuleCleanup(cleanup)
            result = self.run_cases(self.cases(f))
            self.assertEqual(f.events, ["native one", "native two", "native three", "native module cleanup"])
        self.assertEqual(result.status, 1)
        self.assertEqual(result.summary["passed"], 3)
        self.assertEqual(result.summary["errors"], 1)
        self.assertTrue(result.summary["phases"]["native"]["complete"])
        self.assertFalse(result.summary["phases"]["native"]["qualified"])
        self.assertFalse(result.summary["phases"]["pure"]["executed"])

    def test_native_failfast_stops_remaining_cases_and_pure(self):
        with self.fixture() as f:
            f.native.test_one = lambda case: case.fail("native contract failed")
            result = self.run_cases(self.cases(f))
            self.assertEqual(f.events, [])
        self.assertEqual(result.status, 1)
        self.assertEqual(result.summary["tests_run"], 1)
        self.assertEqual(result.summary["failures"], 1)
        self.assertFalse(result.summary["phases"]["pure"]["executed"])
        self.assertFalse(result.summary["full_scope"])

    def test_native_skip_expected_failure_and_unexpected_success_cannot_qualify(self):
        for kind in ("class fixture skip", "class decorator skip", "method skip",
                     "subtest skip", "expected failure", "unexpected success"):
            with self.subTest(kind=kind), self.fixture() as f:
                if kind == "class fixture skip":
                    def setup(cls):
                        raise unittest.SkipTest("native unavailable")
                    f.native.setUpClass = classmethod(setup)
                elif kind == "class decorator skip":
                    unittest.skip("native unavailable")(f.native)
                elif kind == "method skip":
                    f.native.test_one = unittest.skip("native unavailable")(lambda case: None)
                elif kind == "subtest skip":
                    def subtest(case):
                        with case.subTest(value=1):
                            case.skipTest("native subcase unavailable")
                    f.native.test_one = subtest
                elif kind == "expected failure":
                    f.native.test_one = unittest.expectedFailure(lambda case: case.fail("expected"))
                else:
                    f.native.test_one = unittest.expectedFailure(lambda case: None)
                result = self.run_cases(self.cases(f))
                self.assertEqual(result.status, 1)
                self.assertFalse(result.summary["phases"]["native"]["qualified"])
                self.assertFalse(result.summary["phases"]["pure"]["executed"])
                self.assertFalse(any(event.startswith("pure") for event in f.events))

    def test_pure_failure_does_not_stop_later_pure_cases(self):
        with self.fixture() as f:
            def failure(case):
                f.events.append("pure failure")
                case.fail("pure assertion")
            f.pure.test_one = failure
            result = self.run_cases(self.cases(f))
            self.assertEqual(f.events[-2:], ["pure failure", "pure two"])
        self.assertEqual(result.status, 1)
        self.assertTrue(result.summary["full_scope"])
        self.assertEqual(result.summary["tests_run"], 5)
        self.assertEqual(result.summary["passed"], 4)
        self.assertEqual(result.summary["failures"], 1)
        self.assertEqual(result.summary["missing_ids"], [])

    def test_declared_pure_class_skips_start_each_case_like_node_unavailable(self):
        with self.fixture() as f:
            # Source suites declare optional-tool skips on classes/methods. unittest
            # starts each method; this differs from SkipTest raised by setUpClass.
            unittest.skipUnless(False, "node unavailable")(f.pure)
            result = self.run_cases(self.cases(f))
        self.assertEqual(result.status, 0)
        self.assertTrue(result.summary["full_scope"])
        self.assertEqual(result.summary["tests_run"], 5)
        self.assertEqual(result.summary["passed"], 3)
        self.assertEqual(result.summary["skipped"], 2)
        self.assertTrue(result.summary["phases"]["pure"]["qualified"])

    def test_pure_class_fixture_skip_reports_incomplete_scope(self):
        with self.fixture() as f:
            def setup(cls):
                raise unittest.SkipTest("fixture unavailable")
            f.pure.setUpClass = classmethod(setup)
            result = self.run_cases(self.cases(f))
        self.assertEqual(result.status, 1)
        self.assertFalse(result.summary["full_scope"])
        self.assertTrue(result.summary["phases"]["pure"]["was_successful"])
        self.assertEqual(len(result.summary["phases"]["pure"]["missing_ids"]), 2)
        self.assertEqual(result.summary["skipped"], 1)
        self.assertEqual(result.summary["tests_run"], 3)
        self.assertEqual(result.summary["phases"]["pure"].get("integrity_errors", []), [])

    def test_pure_expected_failure_and_subtest_skip_keep_standard_outcomes(self):
        with self.fixture() as f:
            f.pure.test_one = unittest.expectedFailure(lambda case: case.fail("expected"))
            def subtest(case):
                with case.subTest(value=1):
                    case.skipTest("optional subcase")
            f.pure.test_two = subtest
            result = self.run_cases(self.cases(f))
        self.assertEqual(result.status, 0)
        self.assertTrue(result.summary["full_scope"])
        self.assertEqual(result.summary["expected_failures"], 1)
        self.assertEqual(result.summary["skipped"], 1)
        self.assertEqual(result.summary["tests_run"], 5)

    def test_duplicate_missing_native_and_discovery_errors_refuse_before_execution(self):
        for kind in ("duplicate", "missing", "discovery"):
            with self.subTest(kind=kind), self.fixture() as f:
                cases = self.cases(f)
                kwargs = {}
                if kind == "duplicate":
                    cases.append(f.native("test_one"))
                    expected = "duplicate_case_ids"
                elif kind == "missing":
                    cases.pop(2)
                    expected = "missing_required_native_cases"
                else:
                    kwargs["discovery_errors"] = ("raw import trace private marker",)
                    expected = "discovery_errors"
                result = self.run_cases(cases, **kwargs)
                self.assertEqual(result.status, 1)
                self.assertEqual(f.events, [])
                self.assertEqual(result.summary["tests_run"], 0)
                self.assertIn(expected, result.summary["preflight_errors"])
                self.assertFalse(result.summary["phases"]["native"]["executed"])
                self.assertFalse(result.summary["phases"]["pure"]["executed"])
                self.assertFalse(result.summary["full_scope"])
                self.assertNotIn("private marker", result.stdout)

    def test_spoofed_native_class_names_and_ids_do_not_satisfy_prerequisites(self):
        with self.fixture() as f:
            spoof = type(f.native.__name__, (unittest.TestCase,), {
                "__module__": f.native.__module__,
                "__qualname__": f.native.__qualname__,
                "test_one": lambda case: f.events.append("spoof one"),
                "test_two": lambda case: f.events.append("spoof two"),
                "test_three": lambda case: f.events.append("spoof three"),
            })
            cases = [spoof("test_one"), spoof("test_two"), spoof("test_three"), f.pure("test_one")]
            self.assertEqual([case.id() for case in cases[:3]], list(f.required))
            self.assertIsNot(spoof, f.native_module.NativeCases)
            result = self.run_cases(cases)
            self.assertEqual(f.events, [])
        self.assertEqual(result.status, 1)
        self.assertEqual(result.manifest["native_ids"], [])
        self.assertIn("missing_required_native_cases", result.summary["preflight_errors"])
        self.assertFalse(result.summary["phases"]["native"]["executed"])

    def test_changed_case_identity_cannot_qualify_against_a_different_manifest(self):
        with self.fixture() as f:
            original = f.pure("test_one").id()
            reads = 0

            def changed_id(case):
                nonlocal reads
                reads += 1
                return original if reads == 1 else original + ".changed"

            f.pure.id = changed_id
            result = self.run_cases(self.cases(f)[:-1])
        self.assertEqual(result.status, 1)
        self.assertFalse(result.summary["success"])
        self.assertFalse(result.summary["full_scope"])
        self.assertTrue(result.summary["phases"]["pure"]["qualified"])
        self.assertIn(original, result.summary["missing_ids"])

    def test_foreign_success_identity_cannot_qualify_complete_pure_coverage(self):
        with self.fixture() as f:
            cases = self.cases(f)[:-1]
            case = cases[-1]
            original = case.id()
            foreign = original + ".foreign"
            reads = 0

            def success_only_foreign_id():
                nonlocal reads
                reads += 1
                # Manifest, phase selection and start retain the real identity;
                # only addSuccess differs, and stop returns the real identity.
                return foreign if reads == 4 else original

            case.id = success_only_foreign_id
            result = self.run_cases(cases)
        pure = result.summary["phases"]["pure"]
        for name, actual, expected in (
                ("identity reads", reads, 5),
                ("started", pure["started_ids"], [original]),
                ("finished", pure["finished_ids"], [original]),
                ("successful", pure["successful_ids"], [foreign]),
                ("unexpected", pure["unexpected_ids"], [foreign]),
                ("missing", pure["missing_ids"], []),
                ("unfinished", pure["unfinished_ids"], []),
                ("complete", pure["complete"], True),
                ("full scope", result.summary["full_scope"], True),
                ("status", result.status, 1),
                ("qualified", pure["qualified"], False),
                ("success", result.summary["success"], False)):
            with self.subTest(observation=name):
                self.assertEqual(actual, expected)

    def test_duplicate_success_callback_cannot_qualify_complete_pure_coverage(self):
        with self.fixture() as f:
            def duplicate_success(case):
                case._outcome.result.addSuccess(case)

            f.pure.test_one = duplicate_success
            cases = self.cases(f)[:-1]
            original = cases[-1].id()
            result = self.run_cases(cases)
        pure = result.summary["phases"]["pure"]
        for name, actual, expected in (
                ("started", pure["started_ids"], [original]),
                ("finished", pure["finished_ids"], [original]),
                ("successful", pure["successful_ids"], [original, original]),
                ("unexpected", pure["unexpected_ids"], []),
                ("missing", pure["missing_ids"], []),
                ("unfinished", pure["unfinished_ids"], []),
                ("tests run", pure["tests_run"], 1),
                ("complete", pure["complete"], True),
                ("full scope", result.summary["full_scope"], True),
                ("status", result.status, 1),
                ("qualified", pure["qualified"], False),
                ("success", result.summary["success"], False)):
            with self.subTest(observation=name):
                self.assertEqual(actual, expected)

    def test_corrupt_tests_run_count_cannot_qualify_complete_pure_coverage(self):
        with self.fixture() as f:
            def corrupt_count(case):
                case._outcome.result.testsRun += 1

            f.pure.test_one = corrupt_count
            cases = self.cases(f)[:-1]
            original = cases[-1].id()
            result = self.run_cases(cases)
        pure = result.summary["phases"]["pure"]
        for name, actual, expected in (
                ("started", pure["started_ids"], [original]),
                ("finished", pure["finished_ids"], [original]),
                ("successful", pure["successful_ids"], [original]),
                ("unexpected", pure["unexpected_ids"], []),
                ("missing", pure["missing_ids"], []),
                ("unfinished", pure["unfinished_ids"], []),
                ("selected", pure["selected"], 1),
                ("tests run", pure["tests_run"], 2),
                ("complete", pure["complete"], True),
                ("full scope", result.summary["full_scope"], True),
                ("status", result.status, 1),
                ("qualified", pure["qualified"], False),
                ("success", result.summary["success"], False)):
            with self.subTest(observation=name):
                self.assertEqual(actual, expected)

    def test_duplicate_expected_failure_callback_cannot_qualify_complete_pure_coverage(self):
        private_text = "private-duplicate-expected-failure"
        with self.fixture() as f:
            @unittest.expectedFailure
            def duplicate_expected_failure(case):
                try:
                    case.fail(private_text)
                except AssertionError:
                    case._outcome.result.addExpectedFailure(case, sys.exc_info())
                case.fail("ordinary expected body failure")

            f.pure.test_one = duplicate_expected_failure
            cases = self.cases(f)[:-1]
            selected = cases[-1].id()
            result = self.run_cases(cases)
        phase = result.summary["phases"]["pure"]
        self.assertTrue(result.summary["phases"]["native"]["qualified"])
        self.assertTrue(result.summary["full_scope"])
        self.assertTrue(phase["complete"])
        self.assertTrue(phase["was_successful"])
        self.assertEqual(phase["tests_run"], 1)
        self.assertEqual(phase["expected_failures"], 2)
        self.assertEqual(phase["started_ids"], [selected])
        self.assertEqual(phase["finished_ids"], [selected])
        self.assertEqual(phase["successful_ids"], [])
        self.assertEqual(result.status, 1)
        self.assertFalse(phase["qualified"])
        self.assertFalse(result.summary["success"])
        self.assertEqual(phase["integrity_errors"], ["duplicate_expected_failure"])
        self.assertNotIn(private_text, result.stdout)

    def test_expected_failures_for_distinct_cases_reset_callback_tracking(self):
        with self.fixture() as f:
            f.pure.test_one = unittest.expectedFailure(lambda case: case.fail("ordinary first"))
            f.pure.test_two = unittest.expectedFailure(lambda case: case.fail("ordinary second"))
            result = self.run_cases(self.cases(f))
        phase = result.summary["phases"]["pure"]
        self.assertEqual(result.status, 0)
        self.assertTrue(result.summary["success"])
        self.assertTrue(phase["qualified"])
        self.assertEqual(phase["tests_run"], 2)
        self.assertEqual(phase["expected_failures"], 2)
        self.assertEqual(phase["integrity_errors"], [])

    def test_real_cleanup_and_expected_failure_callbacks_match_default_unittest(self):
        @unittest.expectedFailure
        def expected_with_cleanup_skip(case):
            case.addCleanup(lambda: case.skipTest("ordinary cleanup skip"))
            case.fail("ordinary expected body failure")

        @unittest.expectedFailure
        def expected_with_subtest_skip(case):
            with case.subTest(optional=True):
                case.skipTest("ordinary optional subtest")
            case.fail("ordinary expected body failure")

        def cleanup_skips(case):
            case.addCleanup(lambda: case.skipTest("ordinary first cleanup skip"))
            case.addCleanup(lambda: case.skipTest("ordinary second cleanup skip"))

        def cleanup_errors(case):
            def fail(label):
                raise RuntimeError(label)
            case.addCleanup(fail, "ordinary first cleanup error")
            case.addCleanup(fail, "ordinary second cleanup error")

        for body in (expected_with_cleanup_skip, expected_with_subtest_skip,
                     cleanup_skips, cleanup_errors):
            with self.subTest(scenario=body.__name__), self.fixture() as f:
                raw_case = type("RawCase", (unittest.TestCase,), {"test_case": body})
                raw = unittest.TestResult()
                raw_case("test_case").run(raw)
                f.pure.test_one = body
                result = self.run_cases(self.cases(f)[:-1])
                phase = result.summary["phases"]["pure"]
                self.assertEqual(result.status, 0 if raw.wasSuccessful() else 1)
                self.assertEqual(result.summary["success"], raw.wasSuccessful())
                self.assertTrue(result.summary["full_scope"])
                self.assertEqual(phase["qualified"], raw.wasSuccessful())
                self.assertEqual(phase["tests_run"], 1)
                self.assertEqual(phase["skipped"], len(raw.skipped))
                self.assertEqual(phase["expected_failures"], len(raw.expectedFailures))
                self.assertEqual(phase["errors"], len(raw.errors))
                self.assertEqual(phase["integrity_errors"], [])

    def test_fixed_discovery_flattens_once_without_executing_cases(self):
        with self.fixture() as f:
            cases = self.cases(f)
            loader = mock.Mock(errors=[])
            loader.discover.side_effect = [
                unittest.TestSuite([unittest.TestSuite(cases[:1]), unittest.TestSuite(cases[1:2])]),
                *(unittest.TestSuite(cases[index:index + 1]) for index in range(2, 5)),
            ]
            with mock.patch.object(WORKFLOW.unittest, "TestLoader", return_value=loader):
                actual, errors = WORKFLOW._discover()
            self.assertEqual(actual, tuple(cases))
            self.assertEqual(errors, ())
            self.assertEqual(f.events, [])
            self.assertEqual(loader.discover.call_args_list, [
                mock.call(str(WORKFLOW.TESTS), pattern="test_*.py", top_level_dir=str(WORKFLOW.TESTS)),
                *(mock.call(str(WORKFLOW.SMOKE), pattern=name, top_level_dir=str(WORKFLOW.SMOKE))
                  for name in WORKFLOW.SMOKE_TEST_FILES),
            ])

    def test_main_discovery_exception_and_cli_bypass_arguments_fail_closed(self):
        for kind in ("exception", "arguments"):
            with self.subTest(kind=kind):
                with mock.patch.object(WORKFLOW, "_discover", side_effect=RuntimeError("private value")) as discover:
                    result = self.output(lambda out, err: WORKFLOW.main(
                        ["--skip-native"] if kind == "arguments" else []))
                self.assertEqual(result.status, 1)
                self.assertEqual(result.summary["tests_run"], 0)
                self.assertFalse(result.summary["full_scope"])
                self.assertFalse(result.manifest["discovery_complete"])
                self.assertNotIn("private value", result.stdout)
                if kind == "arguments":
                    discover.assert_not_called()
                else:
                    discover.assert_called_once_with()

    def test_interrupt_is_preserved_and_the_last_started_case_is_not_full_scope(self):
        with self.fixture() as f:
            control = KeyboardInterrupt("stop workflow")

            def interrupted(case):
                raise control

            f.pure.test_one = interrupted
            cases = self.cases(f)[:-1]

            def invoke(stdout, stderr):
                try:
                    WORKFLOW._run_cases(cases, stdout=stdout, stderr=stderr)
                except BaseException as error:
                    self.assertIs(error, control)
                    return error
                self.fail("workflow swallowed interruption")

            result = self.output(invoke)
        self.assertIs(result.status, control)
        self.assertFalse(result.summary["success"])
        self.assertFalse(result.summary["full_scope"])
        self.assertEqual(result.summary["interrupted_type"], "KeyboardInterrupt")
        self.assertEqual(result.summary["missing_ids"], [])
        self.assertEqual(result.summary["unfinished_ids"], [])
        self.assertFalse(result.summary["phases"]["pure"]["run_completed"])
        self.assertFalse(result.summary["phases"]["pure"]["complete"])

    def test_summary_output_failure_keeps_the_pending_interrupt_and_a_note(self):
        failure = OSError("observation output unavailable")

        class BrokenSummary(io.StringIO):
            def write(self, text):
                if text.startswith(WORKFLOW.SUMMARY_PREFIX):
                    raise failure
                return super().write(text)

        with self.fixture() as f:
            control = KeyboardInterrupt("stop workflow")

            def interrupted(case):
                raise control

            f.pure.test_one = interrupted
            with self.assertRaises(KeyboardInterrupt) as caught:
                WORKFLOW._run_cases(self.cases(f), stdout=BrokenSummary(), stderr=io.StringIO())
            self.assertIs(caught.exception, control)
            self.assertFalse(any(event.startswith("pure") for event in f.events))
            self.assertIn("summary output failed: OSError", "\n".join(control.__notes__))

    def test_summary_output_failure_after_completed_cases_is_an_error(self):
        failure = OSError("observation output unavailable")

        class BrokenSummary(io.StringIO):
            def write(self, text):
                if text.startswith(WORKFLOW.SUMMARY_PREFIX):
                    raise failure
                return super().write(text)

        with self.fixture() as f:
            with self.assertRaises(OSError) as caught:
                WORKFLOW._run_cases(self.cases(f), stdout=BrokenSummary(), stderr=io.StringIO())
            self.assertIs(caught.exception, failure)
            self.assertEqual(f.events[-2:], ["pure one", "pure two"])

    def test_native_teardown_exit_zero_is_incomplete_and_cli_returns_one(self):
        with self.fixture() as f:
            control = SystemExit(0)

            def teardown(cls):
                raise control

            f.native.tearDownClass = classmethod(teardown)
            cases = self.cases(f)
            with mock.patch.object(WORKFLOW, "_discover", return_value=(cases, ())):
                result = self.output(lambda out, err: WORKFLOW.main([]))
            self.assertEqual(f.events, ["native one", "native two", "native three"])
        self.assertEqual(result.status, 1)
        self.assertFalse(result.summary["success"])
        self.assertFalse(result.summary["full_scope"])
        self.assertEqual(result.summary["interrupted_type"], "SystemExit")
        self.assertEqual(result.summary["passed"], 3)
        self.assertFalse(result.summary["phases"]["native"]["run_completed"])
        self.assertFalse(result.summary["phases"]["native"]["qualified"])
        self.assertFalse(result.summary["phases"]["pure"]["executed"])
        self.assertIn("workflow incomplete", result.stderr)

    def test_discovery_callback_success_exit_is_incomplete_and_cli_returns_one(self):
        for code in (None, 0):
            name = "test_workflow_discovery_exit_" + ("none" if code is None else "zero")
            with self.subTest(code=code), tempfile.TemporaryDirectory() as directory:
                path = Path(directory)
                (path / (name + ".py")).write_text(
                    "def load_tests(loader, tests, pattern):\n"
                    f"    raise SystemExit({code!r})\n")
                try:
                    with mock.patch.object(WORKFLOW, "TESTS", path), \
                            mock.patch.object(sys, "path", list(sys.path)):
                        def invoke(stdout, stderr):
                            try:
                                return WORKFLOW.main([])
                            except BaseException as error:
                                return error

                        result = self.output(invoke)
                finally:
                    sys.modules.pop(name, None)
                self.assertEqual(result.status, 1)
                self.assertFalse(result.manifest["discovery_complete"])
                self.assertFalse(result.summary["success"])
                self.assertFalse(result.summary["full_scope"])
                self.assertEqual(result.summary["discovery_error_count"], 1)
                self.assertIn("discovery_errors", result.summary["preflight_errors"])
                self.assertEqual(result.summary["tests_run"], 0)
                self.assertFalse(result.summary["phases"]["native"]["executed"])
                self.assertFalse(result.summary["phases"]["pure"]["executed"])
                self.assertIn("discovery incomplete", result.stderr)

    def test_discovery_callback_nonzero_exit_and_interrupt_preserve_identity(self):
        for kind, expression, exception in (
                ("nonzero", "SystemExit(7)", SystemExit),
                ("interrupt", "KeyboardInterrupt('stop')", KeyboardInterrupt)):
            name = "test_workflow_discovery_exit_" + kind
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                path = Path(directory)
                (path / (name + ".py")).write_text(
                    f"control = {expression}\n"
                    "def load_tests(loader, tests, pattern):\n"
                    "    raise control\n")
                try:
                    stdout, stderr = io.StringIO(), io.StringIO()
                    with mock.patch.object(WORKFLOW, "TESTS", path), \
                            mock.patch.object(sys, "path", list(sys.path)), \
                            redirect_stdout(stdout), redirect_stderr(stderr), \
                            self.assertRaises(exception) as caught:
                        WORKFLOW.main([])
                    self.assertIs(caught.exception, sys.modules[name].control)
                    self.assertEqual(stdout.getvalue(), "")
                    self.assertEqual(stderr.getvalue(), "")
                finally:
                    sys.modules.pop(name, None)

    def test_nonzero_exit_and_keyboard_interrupt_keep_cli_exception_identity(self):
        for control in (SystemExit(7), KeyboardInterrupt("stop")):
            with self.subTest(kind=type(control).__name__), self.fixture() as f:
                def teardown(cls):
                    raise control

                f.native.tearDownClass = classmethod(teardown)
                with mock.patch.object(WORKFLOW, "_discover", return_value=(self.cases(f), ())):
                    def invoke(stdout, stderr):
                        try:
                            WORKFLOW.main([])
                        except BaseException as error:
                            self.assertIs(error, control)
                            return error
                        self.fail("CLI swallowed nonzero exit or interruption")
                    result = self.output(invoke)
                self.assertIs(result.status, control)
                self.assertFalse(result.summary["full_scope"])
                self.assertFalse(result.summary["phases"]["native"]["qualified"])
                self.assertFalse(result.summary["phases"]["pure"]["executed"])

    def test_case_outcomes_must_belong_to_the_active_selected_object(self):
        for kind in ("missing terminal", "aliased success", "success after stop",
                     "passing subtest without terminal"):
            with self.subTest(kind=kind), self.fixture() as f:
                alias_checks = []

                def run(case, result):
                    result.startTest(case)
                    if kind == "aliased success":
                        alias = type(case)(case._testMethodName)
                        alias_checks.extend((alias is not case, alias.id() == case.id()))
                        result.addSuccess(alias)
                    elif kind == "passing subtest without terminal":
                        subcase = type(case)(case._testMethodName)
                        subcase.test_case = case
                        subcase.id = lambda: "private-passing-subtest-parameter"
                        result.addSubTest(case, subcase, None)
                    result.stopTest(case)
                    if kind == "success after stop":
                        result.addSuccess(case)
                    return result

                f.pure.run = run
                cases = self.cases(f)[:-1]
                selected = cases[-1].id()

                def invoke(stdout, stderr):
                    try:
                        return WORKFLOW._run_cases(cases, stdout=stdout, stderr=stderr)
                    except BaseException as error:
                        return error

                result = self.output(invoke)
                phase = result.summary["phases"]["pure"]
                self.assertEqual(phase["tests_run"], 1)
                self.assertEqual(phase["started_ids"], [selected])
                self.assertEqual(phase["finished_ids"], [selected])
                self.assertEqual(phase["unexpected_ids"], [])
                self.assertTrue(result.summary["phases"]["native"]["qualified"])
                if kind == "aliased success":
                    self.assertEqual(alias_checks, [True, True])
                self.assertNotIn("private-passing-subtest-parameter", result.stdout)
                with self.subTest(contract="exit"):
                    self.assertEqual(result.status, 1)
                with self.subTest(contract="phase qualification"):
                    self.assertFalse(phase["qualified"])
                with self.subTest(contract="workflow success"):
                    self.assertFalse(result.summary["success"])
                with self.subTest(contract="callback integrity"):
                    self.assertTrue(phase.get("integrity_errors", []))

    def test_ordinary_alias_parent_link_cannot_supply_a_selected_case_outcome(self):
        for kind in ("skip", "expected failure"):
            with self.subTest(kind=kind), self.fixture() as f:
                alias_checks = []

                def run(case, result):
                    result.startTest(case)
                    alias = type(case)(case._testMethodName)
                    alias.test_case = case
                    alias_checks.extend((alias is not case, alias.id() == case.id()))
                    if kind == "skip":
                        result.addSkip(alias, "ordinary alias skip")
                    else:
                        try:
                            raise AssertionError("ordinary alias expected failure")
                        except AssertionError:
                            result.addExpectedFailure(alias, sys.exc_info())
                    result.stopTest(case)
                    return result

                f.pure.run = run
                cases = self.cases(f)[:-1]
                selected = cases[-1].id()
                result = self.run_cases(cases)
                phase = result.summary["phases"]["pure"]
                self.assertEqual(alias_checks, [True, True])
                self.assertEqual(phase["started_ids"], [selected])
                self.assertEqual(phase["finished_ids"], [selected])
                self.assertEqual(phase["tests_run"], 1)
                self.assertTrue(phase["complete"])
                self.assertTrue(result.summary["full_scope"])
                self.assertEqual(phase["successful_ids"], [])
                self.assertEqual(phase["unexpected_ids"], [])
                self.assertEqual(phase["skipped"], int(kind == "skip"))
                self.assertEqual(phase["expected_failures"], int(kind == "expected failure"))
                with self.subTest(contract="exit"):
                    self.assertEqual(result.status, 1)
                with self.subTest(contract="phase qualification"):
                    self.assertFalse(phase["qualified"])
                with self.subTest(contract="workflow success"):
                    self.assertFalse(result.summary["success"])
                with self.subTest(contract="callback integrity"):
                    self.assertTrue(phase.get("integrity_errors", []))

    def test_multiple_subtest_skips_and_expected_failure_are_ordinary_outcomes(self):
        private_parameter = "private-skipped-subtest-parameter"
        with self.fixture() as f:
            def skipped(case):
                for value in (1, 2):
                    with case.subTest(value=value, private=private_parameter):
                        case.skipTest("ordinary optional subcase")

            f.pure.test_one = skipped
            f.pure.test_two = unittest.expectedFailure(lambda case: case.fail("ordinary expected"))
            result = self.run_cases(self.cases(f))
        phase = result.summary["phases"]["pure"]
        self.assertEqual(result.status, 0)
        self.assertTrue(result.summary["success"])
        self.assertTrue(result.summary["full_scope"])
        self.assertTrue(phase["qualified"])
        self.assertEqual(phase["tests_run"], 2)
        self.assertEqual(phase["skipped"], 2)
        self.assertEqual(phase["expected_failures"], 1)
        self.assertEqual(phase.get("integrity_errors", []), [])
        self.assertNotIn(private_parameter, result.stdout)

    def test_multiple_cleanup_errors_are_failures_without_protocol_faults(self):
        with self.fixture() as f:
            def cleanup(label):
                f.events.append(label)
                raise RuntimeError(label)

            def setup(case):
                case.addCleanup(cleanup, "first cleanup error")
                case.addCleanup(cleanup, "second cleanup error")

            f.pure.setUp = setup
            result = self.run_cases(self.cases(f)[:-1])
            self.assertEqual(f.events[-3:],
                             ["pure one", "second cleanup error", "first cleanup error"])
        phase = result.summary["phases"]["pure"]
        self.assertEqual(result.status, 1)
        self.assertFalse(result.summary["success"])
        self.assertTrue(result.summary["full_scope"])
        self.assertTrue(phase["complete"])
        self.assertFalse(phase["qualified"])
        self.assertEqual(phase["tests_run"], 1)
        self.assertEqual(phase["errors"], 2)
        self.assertEqual(phase.get("integrity_errors", []), [])

    def test_fixture_holders_are_not_case_outcomes_during_or_after_execution(self):
        private_text = "private-fixture-holder-observation"
        for kind in ("expected failure after stop", "skip during active case"):
            with self.subTest(kind=kind), self.fixture() as f:
                def run(case, result):
                    case.fixture_result = result
                    unittest.TestCase.run(case, result)
                    if kind == "expected failure after stop":
                        try:
                            raise AssertionError(private_text)
                        except AssertionError:
                            result.addExpectedFailure(
                                unittest.suite._ErrorHolder(private_text), sys.exc_info())
                    return result

                def method(case):
                    f.events.append("ordinary pure body")
                    if kind == "skip during active case":
                        case.fixture_result.addSkip(
                            unittest.suite._ErrorHolder(private_text), private_text)

                f.pure.run = run
                f.pure.test_one = method
                cases = self.cases(f)[:-1]
                selected = cases[-1].id()

                def invoke(stdout, stderr):
                    try:
                        return WORKFLOW._run_cases(cases, stdout=stdout, stderr=stderr)
                    except BaseException as error:
                        return error

                result = self.output(invoke)
                phase = result.summary["phases"]["pure"]
                self.assertEqual(f.events[-1], "ordinary pure body")
                self.assertEqual(phase["started_ids"], [selected])
                self.assertEqual(phase["finished_ids"], [selected])
                self.assertEqual(phase["successful_ids"], [selected])
                self.assertEqual(phase["tests_run"], 1)
                self.assertEqual(phase["expected_failures"], int(kind == "expected failure after stop"))
                self.assertEqual(phase["skipped"], int(kind == "skip during active case"))
                self.assertTrue(phase["complete"])
                self.assertTrue(phase["was_successful"])
                self.assertTrue(result.summary["full_scope"])
                self.assertNotIn(private_text, result.stdout)
                with self.subTest(contract="exit"):
                    self.assertEqual(result.status, 1)
                with self.subTest(contract="phase qualification"):
                    self.assertFalse(phase["qualified"])
                with self.subTest(contract="workflow success"):
                    self.assertFalse(result.summary["success"])
                with self.subTest(contract="callback integrity"):
                    self.assertTrue(phase.get("integrity_errors", []))

    def test_completed_pure_class_and_module_teardown_skips_are_fixture_outcomes(self):
        private_text = "private-ordinary-fixture-skip"
        for kind in ("class", "module"):
            with self.subTest(kind=kind), self.fixture() as f:
                def skipped(*_args):
                    raise unittest.SkipTest(private_text)

                if kind == "class":
                    f.pure.tearDownClass = classmethod(skipped)
                else:
                    f.pure_module.tearDownModule = skipped
                result = self.run_cases(self.cases(f))
                phase = result.summary["phases"]["pure"]
                self.assertEqual(f.events[-2:], ["pure one", "pure two"])
                self.assertEqual(result.status, 0)
                self.assertTrue(result.summary["success"])
                self.assertTrue(result.summary["full_scope"])
                self.assertTrue(phase["qualified"])
                self.assertTrue(phase["complete"])
                self.assertEqual(phase["tests_run"], 2)
                self.assertEqual(phase["passed"], 2)
                self.assertEqual(phase["skipped"], 1)
                self.assertEqual(phase["missing_ids"], [])
                self.assertEqual(phase.get("integrity_errors", []), [])
                self.assertNotIn(private_text, result.stdout)


if __name__ == "__main__":
    unittest.main()
