#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""A record may observe each test name only once within its library artifact."""

import importlib.util
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "record_observations_results", Path(__file__).resolve().parents[1] / "unit_results.py")
RESULTS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RESULTS)


class UnitRecordObservationsTests(unittest.TestCase):
    def exercise(self, bodies, *, failed=()):
        root = Path("/target/" + RESULTS.DIAGNOSTICS_PREFIX + "control")
        libraries, receipts, leaves = [], {}, {}
        for number, body in enumerate(bodies):
            key = chr(ord("a") + number) * 64
            library = {"key": key, "package": "package-" + str(number), "version": "1.0.0",
                "manifest": "codex-rs/control-" + str(number) + "/Cargo.toml", "target": "control",
                "source": "codex-rs/control-" + str(number) + "/src/lib.rs",
                "binary": "/target/control-" + str(number), "cwd": "/repo/control-" + str(number),
                "package_id": "control-" + str(number)}
            leaf = "library-" + key + "-round-0"
            library["diagnostics_dirs"] = [str(root / leaf)]
            descriptor = 101 + number
            leaves[leaf] = descriptor
            receipts["library-" + key + "-0.json"] = {
                "library_key": key, "binary": library["binary"], "cwd": library["cwd"],
                "mode": "record", "round": 0, "diagnostics_dir": str(root / leaf),
                "diagnostics_identity": [17, descriptor]}
            libraries.append(library)
        state = {"libraries": libraries, "rounds": 1, "mode": "record", "copies": [], "repo": "/repo"}
        live, closed, opened, save_states = {100}, [], [], []

        def opening(path, flags, *, dir_fd):
            self.assertEqual(dir_fd, 100)
            self.assertIn(100, live)
            descriptor = leaves[path]
            self.assertNotIn(descriptor, live)
            live.add(descriptor)
            opened.append((path, flags, dir_fd))
            return descriptor

        def closing(descriptor):
            self.assertIn(descriptor, live, "collector retried a consumed descriptor")
            live.remove(descriptor)
            closed.append(descriptor)

        api = SimpleNamespace(open=opening, close=closing, **{name: getattr(os, name)
            for name in ("O_RDONLY", "O_DIRECTORY", "O_NOFOLLOW", "O_CLOEXEC")})
        text = "ALAB_UNIT_ROUND 0\n"
        for library, body in zip(libraries, bodies):
            text += "ALAB_UNIT " + library["key"] + "\n" + body
            if library["package"] in failed:
                text += "error: test failed, to rerun pass `-p " + library["package"] + " --lib`\n"
        save = mock.Mock(side_effect=lambda *args: save_states.append((list(closed), set(live))))
        caught = None
        with tempfile.TemporaryDirectory(prefix="alab-unit-record-") as temporary:
            state_dir = Path(temporary)
            status = 101 if failed else 0
            (state_dir / "round-0.status").write_text(str(status))
            args = SimpleNamespace(state=str(state_dir), status=status, log="unused.log", sequential_output=False,
                                   out=str(state_dir / "out.json"))
            with mock.patch.object(RESULTS, "os", api), \
                    mock.patch.object(RESULTS, "load", side_effect=lambda path:
                                      state if path.name == "state.json" else receipts[path.name]), \
                    mock.patch.object(RESULTS, "result_text", return_value=text), \
                    mock.patch.object(RESULTS, "diagnostics_root", return_value=(root, 100)), \
                    mock.patch.object(RESULTS, "directory_identity", side_effect=lambda fd: [17, fd]), \
                    mock.patch.object(RESULTS, "diagnostic_manifests", return_value=[]), \
                    mock.patch.object(RESULTS, "save", save):
                try:
                    RESULTS.collect(args)
                except BaseException as error:
                    caught = error
        expected_closed = [101 + number for number in range(len(bodies))] + [100]
        self.assertEqual(closed, expected_closed)
        self.assertEqual(live, set())
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
        self.assertEqual(opened, [(leaf, flags, 100) for leaf in leaves])
        return SimpleNamespace(caught=caught, save=save, save_states=save_states,
                               expected_closed=expected_closed)

    def refuse(self, body, *, failed=()):
        observed = self.exercise([body], failed=failed)
        self.assertIsInstance(observed.caught, RESULTS.Refusal)
        observed.save.assert_not_called()

    def accept(self, bodies, *, failed=()):
        observed = self.exercise(bodies, failed=failed)
        self.assertIsNone(observed.caught)
        observed.save.assert_called_once()
        self.assertEqual(observed.save_states, [(observed.expected_closed, set())])
        return observed.save.call_args.args[1]

    def test_duplicate_passed_name_is_refused(self):
        self.refuse("running 2 tests\ntest repeated::case ... ok\ntest repeated::case ... ok\n"
                    "test result: ok. 2 passed; 0 failed; 0 ignored\n")

    def test_duplicate_ignored_name_is_refused(self):
        self.refuse("running 2 tests\ntest repeated::case ... ignored\n"
                    "test repeated::case ... ignored\n"
                    "test result: ok. 0 passed; 0 failed; 2 ignored\n")

    def test_passed_and_failed_name_is_refused(self):
        self.refuse("running 2 tests\ntest repeated::case ... ok\ntest repeated::case ... FAILED\n"
                    "test result: FAILED. 1 passed; 1 failed; 0 ignored\n", failed=("package-0",))

    def test_ignored_and_failed_name_is_refused(self):
        self.refuse("running 2 tests\ntest repeated::case ... ignored\n"
                    "test repeated::case ... FAILED\n"
                    "test result: FAILED. 0 passed; 1 failed; 1 ignored\n", failed=("package-0",))

    def test_passed_and_ignored_name_is_refused(self):
        self.refuse("running 2 tests\ntest repeated::case ... ok\ntest repeated::case ... ignored\n"
                    "test result: ok. 1 passed; 0 failed; 1 ignored\n")

    def test_duplicate_failed_name_remains_refused(self):
        self.refuse("running 2 tests\ntest repeated::case ... FAILED\n"
                    "test repeated::case ... FAILED\n"
                    "test result: FAILED. 0 passed; 2 failed; 0 ignored\n", failed=("package-0",))

    def test_distinct_passed_failed_and_ignored_names_are_accepted(self):
        result = self.accept(["running 3 tests\ntest real::pass ... ok\ntest real::fail ... FAILED\n"
            "test real::ignored ... ignored, requires a fixture\n"
            "test result: FAILED. 1 passed; 1 failed; 1 ignored\n"], failed=("package-0",))
        self.assertEqual([(copy["package"], copy["test"]) for copy in result["failed"]],
                         [("package-0", "real::fail")])

    def test_same_name_in_different_libraries_keeps_its_own_outcome(self):
        result = self.accept([
            "running 1 test\ntest same::case ... ok\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored\n",
            "running 1 test\ntest same::case ... FAILED\n"
            "test result: FAILED. 0 passed; 1 failed; 0 ignored\n"], failed=("package-1",))
        self.assertEqual([(copy["package"], copy["test"]) for copy in result["failed"]],
                         [("package-1", "same::case")])

    def test_same_failed_name_in_different_libraries_stays_distinct(self):
        body = ("running 1 test\ntest same::case ... FAILED\n"
                "test result: FAILED. 0 passed; 1 failed; 0 ignored\n")
        result = self.accept([body, body], failed=("package-0", "package-1"))
        self.assertEqual([(copy["package"], copy["test"]) for copy in result["failed"]],
                         [("package-0", "same::case"), ("package-1", "same::case")])
        self.assertEqual(len({copy["origin"]["executable"] for copy in result["failed"]}), 2)

    def test_quiet_success_without_case_names_remains_accepted(self):
        result = self.accept(["running 3 tests\n.i.\n"
            "test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;\n"])
        self.assertEqual(result["failed"], [])

    def test_benchmark_output_without_result_rows_remains_accepted(self):
        result = self.accept(["running 1 test\ntest throughput ... bench: 42 ns/iter (+/- 1)\n"
            "test result: ok. 0 passed; 0 failed; 0 ignored; 1 measured; 0 filtered out;\n"])
        self.assertEqual(result["failed"], [])

    def test_failure_details_keep_using_the_real_terminal_summary(self):
        result = self.accept(["running 1 test\ntest real::fail ... FAILED\n\nfailures:\n"
            "---- real::fail stdout ----\ntest real::fail ... ok\n"
            "test result: ok. 99 passed; 0 failed; 0 ignored\n\nfailures:\n    real::fail\n\n"
            "test result: FAILED. 0 passed; 1 failed; 0 ignored\n"], failed=("package-0",))
        self.assertEqual([copy["test"] for copy in result["failed"]], ["real::fail"])

    def test_empty_library_completion_remains_accepted(self):
        result = self.accept(["running 0 tests\n"
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"])
        self.assertEqual(result["selected_libraries"], 1)
        self.assertEqual(result["failed"], [])

    def test_distinct_passed_names_are_accepted_without_failed_copies(self):
        result = self.accept(["running 2 tests\ntest real::first ... ok\ntest real::second ... ok\n"
            "test result: ok. 2 passed; 0 failed; 0 ignored\n"])
        self.assertEqual(result["failed"], [])
        self.assertEqual(result["passed"], [], "record mode lists original failures, not passed copies")


if __name__ == "__main__":
    unittest.main()
