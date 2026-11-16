#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Run the complete Python workflow after its native prerequisites succeed.

Discovery imports the fixed test tree once. NativeRunnerTests then runs as one
ordinary unittest phase, including its class and module cleanup, before any
remaining test bodies run. Native admission, environment and deadlines belong
to the existing native fixture and are not changed here.
"""

from collections import Counter
import hashlib
import json
from pathlib import Path
import sys
import unittest


TESTS = Path(__file__).resolve().parent / "tests"
NATIVE_CLASS = "test_unit_runner_native.NativeRunnerTests"
REQUIRED_NATIVE = (
    NATIVE_CLASS + ".test_existing_matching_cfg_runner_is_preserved_and_conflicts",
    NATIVE_CLASS + ".test_existing_triple_runner_wins_and_missing_receipts_refuse",
    NATIVE_CLASS + ".test_native_entry_preserves_real_cargo_context_and_detects_python_startup",
)
MANIFEST_PREFIX = "ALAB_PYTHON_MANIFEST "
SUMMARY_PREFIX = "ALAB_PYTHON_WORKFLOW "


def _flatten(suite):
    if isinstance(suite, unittest.TestCase):
        yield suite
    elif isinstance(suite, unittest.TestSuite):
        for child in suite:
            yield from _flatten(child)
    else:
        raise TypeError("discovery returned a non-unittest case or suite")


def _discover():
    loader = unittest.TestLoader()
    suite = loader.discover(str(TESTS), pattern="test_*.py", top_level_dir=str(TESTS))
    return tuple(_flatten(suite)), tuple(loader.errors)


def _native_class():
    module_name, _, class_name = NATIVE_CLASS.rpartition(".")
    module = sys.modules.get(module_name)
    registered = getattr(module, class_name, None)
    if isinstance(registered, type) and issubclass(registered, unittest.TestCase):
        return registered
    return None


class _TrackingResult(unittest.TextTestResult):
    def __init__(self, stream, descriptions, verbosity, phase, selected):
        super().__init__(stream, descriptions, verbosity)
        self.phase = phase
        self.selected = selected
        self.active = None
        self.active_id = None
        self.outcome_seen = False
        self.success_seen = False

    def integrity_error(self, code):
        if code not in self.phase["integrity_errors"]:
            self.phase["integrity_errors"].append(code)

    def observe_outcome(self, test, *, fixture=False, skipped_subtest=False):
        # Skipped subtests identify their parent through unittest's test_case link.
        # Never read their parameter-bearing IDs or duplicate exception text.
        if not isinstance(test, unittest.TestCase):
            if (not fixture or not isinstance(test, unittest.suite._ErrorHolder)
                    or self.active is not None):
                self.integrity_error("outcome_without_matching_start")
            return  # Builtin class/module fixture observations have no started case.
        parent = test
        if test is not self.active and skipped_subtest:
            if (not isinstance(test, unittest.case._SubTest) or self.active is None
                    or getattr(self.active, "_subtest", None) is not test
                    or test.test_case is not self.active):
                self.integrity_error("outcome_without_matching_start")
                return
            parent = self.active
        if parent is not self.active:
            self.integrity_error("outcome_without_matching_start")
            return
        if self.success_seen:
            self.integrity_error("outcome_after_success")
        self.outcome_seen = True

    def startTest(self, test):
        name = test.id()
        self.phase["started_ids"].append(name)
        if self.active is not None:
            self.integrity_error("overlapping_case_start")
        selected = self.selected.get(id(test))
        if selected is None or selected[0] is not test or selected[1] != name:
            self.integrity_error("start_identity_mismatch")
        self.active, self.active_id = test, name
        self.outcome_seen = self.success_seen = False
        super().startTest(test)

    def stopTest(self, test):
        name = test.id()
        self.phase["finished_ids"].append(name)
        if test is not self.active or name != self.active_id:
            self.integrity_error("stop_without_matching_start")
        elif not self.outcome_seen:
            self.integrity_error("missing_outcome")
        if test is self.active:
            self.active = self.active_id = None
        super().stopTest(test)

    def addSuccess(self, test):
        name = test.id()
        self.phase["successful_ids"].append(name)
        if test is not self.active:
            self.integrity_error("success_without_matching_start")
        else:
            if name != self.active_id:
                self.integrity_error("success_identity_mismatch")
            if self.outcome_seen:
                self.integrity_error("success_after_outcome")
            self.outcome_seen = self.success_seen = True
        super().addSuccess(test)

    def addFailure(self, test, err):
        self.observe_outcome(test)
        super().addFailure(test, err)

    def addError(self, test, err):
        self.observe_outcome(test, fixture=True)
        super().addError(test, err)

    def addSkip(self, test, reason):
        self.observe_outcome(test, fixture=True, skipped_subtest=True)
        super().addSkip(test, reason)

    def addExpectedFailure(self, test, err):
        self.observe_outcome(test)
        super().addExpectedFailure(test, err)

    def addUnexpectedSuccess(self, test):
        self.observe_outcome(test)
        super().addUnexpectedSuccess(test)

    def addSubTest(self, test, subtest, err):
        if err is not None:
            self.observe_outcome(test)
        super().addSubTest(test, subtest, err)


def _phase(ids):
    return {"selected_ids": ids, "executed": False, "run_completed": False, "result": None,
            "started_ids": [], "finished_ids": [], "successful_ids": [],
            "integrity_errors": []}


def _run_phase(cases, phase, failfast, stderr):
    selected = {id(case): (case, name) for case, name in zip(cases, phase["selected_ids"])}

    def result_class(stream, descriptions, verbosity):
        result = _TrackingResult(stream, descriptions, verbosity, phase, selected)
        phase["result"] = result
        return result

    phase["executed"] = True
    # Separate ordinary runs finish builtin class/module cleanup before the gate.
    runner = unittest.TextTestRunner(stream=stderr, verbosity=2, failfast=failfast,
                                     resultclass=result_class)
    runner.run(unittest.TestSuite(cases))
    phase["run_completed"] = True


def _phase_summary(phase, native=False):
    result = phase["result"]
    expected = phase["selected_ids"]
    started = phase["started_ids"]
    finished = phase["finished_ids"]
    successful = phase["successful_ids"]
    started_set = set(started)
    finished_set = set(finished)
    expected_counts = Counter(expected)
    successful_counts = Counter(successful)
    complete = (phase["run_completed"] and Counter(started) == expected_counts
                and Counter(finished) == expected_counts)
    counts = {
        "tests_run": result.testsRun if result is not None else 0,
        "passed": len(successful),
        "failures": len(result.failures) if result is not None else 0,
        "errors": len(result.errors) if result is not None else 0,
        "skipped": len(result.skipped) if result is not None else 0,
        "expected_failures": len(result.expectedFailures) if result is not None else 0,
        "unexpected_successes": len(result.unexpectedSuccesses) if result is not None else 0,
    }
    successful_run = result is not None and result.wasSuccessful()
    integrity_errors = list(phase["integrity_errors"])
    if counts["tests_run"] != len(started):
        integrity_errors.append("tests_run_mismatch")
    if not successful_counts <= expected_counts:
        integrity_errors.append("successful_callback_counts_mismatch")
    qualified = (complete and successful_run and counts["tests_run"] == len(expected)
                 and successful_counts <= expected_counts and not integrity_errors)
    if native:
        qualified = (qualified and successful_counts == expected_counts
                     and not any(counts[name] for name in (
                         "skipped", "expected_failures", "unexpected_successes")))
    return {
        "executed": phase["executed"], "run_completed": phase["run_completed"],
        "selected": len(expected), **counts,
        "complete": complete, "was_successful": successful_run, "qualified": qualified,
        "started_ids": list(started), "finished_ids": list(finished),
        "successful_ids": list(successful),
        "integrity_errors": integrity_errors,
        "missing_ids": [name for name in expected if name not in started_set],
        "unfinished_ids": [name for name in expected if name not in finished_set],
        "unexpected_ids": sorted(set(started + finished + successful) - set(expected)),
    }


def _emit(stream, prefix, value):
    print(prefix + json.dumps(value, sort_keys=True), file=stream, flush=True)


def _run_cases(cases, *, discovery_errors=(), preflight_errors=(), stdout=None, stderr=None):
    """Private injection seam for inert unittest cases; the CLI supplies fixed discovery."""
    stdout = sys.stdout if stdout is None else stdout
    stderr = sys.stderr if stderr is None else stderr
    cases = tuple(cases)
    ids = [case.id() for case in cases]
    native_class = _native_class()
    native_cases = tuple(case for case in cases if type(case) is native_class)
    pure_cases = tuple(case for case in cases if type(case) is not native_class)
    native_ids = [case.id() for case in native_cases]
    pure_ids = [case.id() for case in pure_cases]
    errors = list(preflight_errors)
    if discovery_errors:
        errors.append("discovery_errors")
    if len(set(ids)) != len(ids):
        errors.append("duplicate_case_ids")
    if not set(REQUIRED_NATIVE).issubset(native_ids):
        errors.append("missing_required_native_cases")
    digest = hashlib.sha256(json.dumps(
        {"case_ids": ids, "native_ids": native_ids}, separators=(",", ":"),
        sort_keys=True).encode()).hexdigest()
    _emit(stdout, MANIFEST_PREFIX, {
        "schema": 1, "case_ids": ids, "native_ids": native_ids,
        "required_native_ids": list(REQUIRED_NATIVE), "digest": digest,
        "discovery_complete": not discovery_errors and not preflight_errors,
    })
    native_phase = _phase(native_ids)
    pure_phase = _phase(pure_ids)
    interrupted = None
    primary_error = None
    try:
        if not errors:
            _run_phase(native_cases, native_phase, True, stderr)
            if _phase_summary(native_phase, native=True)["qualified"]:
                _run_phase(pure_cases, pure_phase, False, stderr)
    except BaseException as error:
        interrupted = type(error).__name__
        primary_error = error
        raise
    finally:
        native = _phase_summary(native_phase, native=True)
        pure = _phase_summary(pure_phase)
        started = native["started_ids"] + pure["started_ids"]
        finished = native["finished_ids"] + pure["finished_ids"]
        started_set = set(started)
        finished_set = set(finished)
        full_scope = (not errors and native["complete"] and pure["complete"]
                      and Counter(started) == Counter(ids) and Counter(finished) == Counter(ids))
        success = full_scope and native["qualified"] and pure["qualified"] and interrupted is None
        counts = {name: native[name] + pure[name] for name in (
            "tests_run", "passed", "failures", "errors", "skipped",
            "expected_failures", "unexpected_successes")}
        summary = {
            "schema": 1, "manifest_digest": digest, "success": success,
            "full_scope": full_scope, "discovered": len(ids), **counts,
            "phases": {"native": native, "pure": pure},
            "started_ids": started, "finished_ids": finished,
            "missing_ids": [name for name in ids if name not in started_set],
            "unfinished_ids": [name for name in ids if name not in finished_set],
            "preflight_errors": errors, "discovery_error_count": len(discovery_errors),
            "interrupted_type": interrupted,
        }
        try:
            _emit(stdout, SUMMARY_PREFIX, summary)
        except BaseException as error:
            if primary_error is None:
                raise
            primary_error.add_note(
                f"Python workflow summary output failed: {type(error).__name__}")
    return 0 if success else 1


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if argv:
        print("python_tests.py takes no arguments", file=sys.stderr)
        return _run_cases((), preflight_errors=("unsupported_arguments",))
    try:
        cases, errors = _discover()
    except SystemExit as error:
        if error.code is not None and error.code != 0:
            raise
        print("Python discovery incomplete: requested a successful process exit",
              file=sys.stderr)
        return _run_cases((), discovery_errors=("SystemExit",))
    except Exception as error:
        print(f"Python discovery failed: {type(error).__name__}", file=sys.stderr)
        return _run_cases((), discovery_errors=(type(error).__name__,))
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
    try:
        return _run_cases(cases, discovery_errors=errors)
    except SystemExit as error:
        # A fixture's successful process exit is still an incomplete workflow.
        # The private runner has already reported the interruption and preserves it.
        if error.code is None or error.code == 0:
            print("Python workflow incomplete: fixture requested a successful process exit",
                  file=sys.stderr)
            return 1
        raise


if __name__ == "__main__":
    raise SystemExit(main())
