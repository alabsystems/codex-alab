# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Fixed smoke inventory through standard discovery, with inert native fixtures."""
from contextlib import contextmanager, redirect_stderr, redirect_stdout
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
    "alab_smoke_inventory_workflow", Path(__file__).resolve().parents[1] / "python_tests.py")
WORKFLOW = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(WORKFLOW)
NATIVE = "test_workflow_inventory_native"
SMOKE_FILES = tuple("workflow_inventory_" + letter + "_test.py" for letter in "abc")


class SmokeDiscoveryTests(unittest.TestCase):
    @contextmanager
    def fixture(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tests, smoke = root / "tests", root / "smoke"
            tests.mkdir()
            smoke.mkdir()
            (tests / (NATIVE + ".py")).write_text('''import unittest
events = []
def setUpModule():
    events.append("native module setup")
    unittest.addModuleCleanup(events.append, "native module cleanup")
def tearDownModule():
    events.append("native module teardown")
class NativeCases(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        events.append("native class setup")
        cls.addClassCleanup(events.append, "native class cleanup")
    @classmethod
    def tearDownClass(cls):
        events.append("native class teardown")
    def test_a(self): events.append("native a")
    def test_b(self): events.append("native b")
    def test_c(self): events.append("native c")
''')
            (tests / "test_workflow_inventory_pure.py").write_text(f'''import unittest
from {NATIVE} import events
class PureCases(unittest.TestCase):
    def test_case(self): events.append("ordinary pure")
''')
            for letter, name in zip("abc", SMOKE_FILES):
                (smoke / name).write_text(f'''import unittest
from {NATIVE} import events
def setUpModule():
    events.append("{letter} module setup")
    unittest.addModuleCleanup(events.append, "{letter} module cleanup")
def tearDownModule():
    events.append("{letter} module teardown")
class SmokeCases(unittest.TestCase):
    def test_case(self): events.append("{letter} body")
''')
            required = tuple(NATIVE + ".NativeCases.test_" + letter for letter in "abc")
            with mock.patch.dict(sys.modules), mock.patch.object(sys, "path", list(sys.path)), \
                    mock.patch.object(WORKFLOW, "TESTS", tests), \
                    mock.patch.object(WORKFLOW, "SMOKE", smoke, create=True), \
                    mock.patch.object(WORKFLOW, "SMOKE_TEST_FILES", SMOKE_FILES, create=True), \
                    mock.patch.object(WORKFLOW, "NATIVE_CLASS", NATIVE + ".NativeCases"), \
                    mock.patch.object(WORKFLOW, "REQUIRED_NATIVE", required):
                yield SimpleNamespace(root=root, tests=tests, smoke=smoke, required=required)

    def events(self):
        module = sys.modules.get(NATIVE)
        return [] if module is None else module.events

    def output(self, action):
        stdout, stderr = io.StringIO(), io.StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            status = action(stdout, stderr)
        lines = stdout.getvalue().splitlines()
        self.assertEqual(len(lines), 2, stdout.getvalue())
        self.assertTrue(lines[0].startswith(WORKFLOW.MANIFEST_PREFIX))
        self.assertTrue(lines[1].startswith(WORKFLOW.SUMMARY_PREFIX))
        return SimpleNamespace(
            status=status, manifest=json.loads(lines[0][len(WORKFLOW.MANIFEST_PREFIX):]),
            summary=json.loads(lines[1][len(WORKFLOW.SUMMARY_PREFIX):]))

    def test_standard_discovery_keeps_each_original_module_identity_once(self):
        with self.fixture() as f:
            cases, errors = WORKFLOW._discover()
            self.assertEqual(errors, ())
            names = [case.id() for case in cases]
            expected = [*f.required, "test_workflow_inventory_pure.PureCases.test_case",
                        *(Path(name).stem + ".SmokeCases.test_case" for name in SMOKE_FILES)]
            self.assertEqual(names, expected)
            self.assertEqual(len(set(names)), len(names))
            self.assertEqual(self.events(), [], "discovery must not execute a test body")
            for name in SMOKE_FILES:
                module = sys.modules[Path(name).stem]
                self.assertEqual(Path(module.__file__).resolve(), (f.smoke / name).resolve())

    def test_smoke_bodies_wait_for_native_success_and_ordinary_cleanup(self):
        for fail_native in (False, True):
            with self.subTest(fail_native=fail_native), self.fixture() as f:
                cases, errors = WORKFLOW._discover()
                if fail_native:
                    def fail(case):
                        self.events().append("native a")
                        case.fail("fixture native prerequisite failed")
                    sys.modules[NATIVE].NativeCases.test_a = fail
                result = self.output(lambda out, err: WORKFLOW._run_cases(
                    cases, discovery_errors=errors, stdout=out, stderr=err))
                self.assertEqual(result.status, 1 if fail_native else 0)
                native = ["native module setup", "native class setup", "native a"]
                if not fail_native:
                    native.extend(["native b", "native c"])
                native.extend(["native class teardown", "native class cleanup",
                               "native module teardown", "native module cleanup"])
                expected = native if fail_native else [*native, "ordinary pure", *(
                    event for letter in "abc" for event in (
                        letter + " module setup", letter + " body",
                        letter + " module teardown", letter + " module cleanup"))]
                self.assertEqual(self.events(), expected)
                self.assertEqual(result.summary["phases"]["pure"]["executed"], not fail_native)
                self.assertEqual(result.summary["full_scope"], not fail_native)
                if not fail_native:
                    self.assertEqual(result.summary["tests_run"], 7)
                    self.assertEqual(result.summary["started_ids"], result.manifest["case_ids"])
                    self.assertEqual(result.summary["finished_ids"], result.manifest["case_ids"])

    def test_smoke_import_errors_block_native_and_remaining_bodies(self):
        with self.fixture() as f:
            (f.smoke / SMOKE_FILES[-1]).write_text("not valid Python !\n")
            cases, errors = WORKFLOW._discover()
            self.assertEqual(len(errors), 1)
            result = self.output(lambda out, err: WORKFLOW._run_cases(
                cases, discovery_errors=errors, stdout=out, stderr=err))
            self.assertEqual(result.status, 1)
            self.assertFalse(result.manifest["discovery_complete"])
            self.assertFalse(result.summary["phases"]["native"]["executed"])
            self.assertFalse(result.summary["phases"]["pure"]["executed"])
            self.assertEqual(self.events(), [])

    def test_missing_or_empty_required_smoke_module_fails_before_execution(self):
        for missing in (False, True):
            with self.subTest(missing=missing), self.fixture() as f:
                path = f.smoke / SMOKE_FILES[-1]
                if missing:
                    path.unlink()
                else:
                    path.write_text("# No unittest cases.\n")
                result = self.output(lambda _out, _err: WORKFLOW.main([]))
                self.assertEqual(result.status, 1)
                self.assertFalse(result.manifest["discovery_complete"])
                self.assertEqual(result.summary["tests_run"], 0)
                self.assertEqual(self.events(), [])

    def test_foreign_cached_smoke_module_is_refused_by_standard_discovery(self):
        with self.fixture() as f:
            foreign_path = f.root / SMOKE_FILES[0]
            foreign_path.write_text("# A different module with the same name.\n")
            module = ModuleType(Path(SMOKE_FILES[0]).stem)
            module.__file__ = str(foreign_path)
            sys.modules[module.__name__] = module
            result = self.output(lambda _out, _err: WORKFLOW.main([]))
            self.assertEqual(result.status, 1)
            self.assertFalse(result.manifest["discovery_complete"])
            self.assertEqual(result.summary["tests_run"], 0)
            self.assertEqual(self.events(), [])


if __name__ == "__main__":
    unittest.main()
