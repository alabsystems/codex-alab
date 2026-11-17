# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Matrix final-save priority through real files and inert public-main seams."""
from contextlib import contextmanager
import errno
import json
import os
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest import mock

import test_renewal_matrix_log_cleanup as log_fixture


class RenewalMatrixManifestCleanupTests(unittest.TestCase):
    def setUp(self):
        self.world = log_fixture.RenewalMatrixLogCleanupTests()
        self.addCleanup(self.world.doCleanups)
        self.world.setUp()

    @contextmanager
    def fixture(self, *, primary=None, save_error=None, after_write=False):
        # The maintained log fixture independently forbids process/thread/socket
        # operations and observes the real owned case log before its raw fallback.
        with self.world.fixture(primary=primary) as owned:
            target = owned.output / "qualification.json"
            real_open, real_write = Path.open, Path.write_text
            handles, writes, faults = [], [], []
            final_count = 3 if primary is None else 2

            def opening(candidate, mode="r", *args, **keywords):
                raw = real_open(candidate, mode, *args, **keywords)
                if candidate == target and mode == "w":
                    handles.append((raw, raw.fileno()))
                return raw

            def save(candidate, data, *args, **keywords):
                if candidate != target:
                    return real_write(candidate, data, *args, **keywords)
                document = json.loads(data)
                writes.append(document)
                fault = len(writes) == final_count and save_error is not None
                if fault and not after_write:
                    faults.append("before real write")
                    raise save_error
                result = real_write(candidate, data, *args, **keywords)
                if fault:
                    raw, descriptor = handles[-1]
                    self.assertTrue(raw.closed)
                    self.assertTrue(self.is_closed(descriptor))
                    faults.append("after real write and close")
                    raise save_error
                return result

            with mock.patch.object(Path, "open", new=opening), \
                    mock.patch.object(Path, "write_text", new=save):
                try:
                    yield SimpleNamespace(capture=owned.capture, output=owned.output,
                                          writes=writes, faults=faults)
                finally:
                    # Observe before compensating, even when a test assertion or
                    # public-main exception escapes. Only owned regular files close.
                    observed = [(raw.closed, self.is_closed(fd)) for raw, fd in handles]
                    for raw, _descriptor in handles:
                        raw.close()
                    self.assertEqual(observed, [(True, True)] * len(handles))
                    self.assertEqual(len(writes), final_count)
                    self.assertEqual(len(faults), int(save_error is not None))
                    expected = final_count - int(save_error is not None and not after_write)
                    self.assertEqual(len(handles), expected)

    @staticmethod
    def is_closed(descriptor):
        try:
            os.fstat(descriptor)
        except OSError as error:
            return error.errno == errno.EBADF
        return False

    def assert_failed_report(self, run):
        self.assertIs(run.manifest["passed"], False)
        self.assertEqual(run.manifest["cases"], [])

    def assert_healthy_body_report(self, run, *, after_write):
        # No rollback is promised: the final healthy bytes may already be saved.
        self.assertIs(run.manifest["passed"], after_write)
        self.assertIs(run.manifest["cases"][0]["passed"], True)
        self.assertFalse(run.manifest["allSixCasesPassed"])
        self.assertFalse(run.manifest["allAvailableCasesPassed"])

    def test_healthy_api_and_original_entry_keep_the_successful_report(self):
        for entry in (False, True):
            with self.subTest(entry=entry), self.fixture() as fixture:
                run = fixture.capture(entry=entry)
                if entry:
                    self.assertIsInstance(run.error, SystemExit)
                    self.assertEqual(run.error.code, 0)
                else:
                    self.assertIsNone(run.error)
                    self.assertEqual(run.result.status, 0)
                self.assertIs(run.manifest["passed"], True)
                self.assertEqual([row["passed"] for row in fixture.writes], [False, False, True])

    def test_ordinary_body_failure_with_healthy_final_save_keeps_failed_return_and_error(self):
        for primary in (OSError("controller operation"), StopIteration("controller operation")):
            with self.subTest(primary=type(primary).__name__), self.fixture(primary=primary) as fixture:
                run = fixture.capture()
                self.assertIsNone(run.error)
                self.assertEqual(run.result.status, 1)
                self.assert_failed_report(run)
                self.assertEqual(run.manifest["error"], type(primary).__name__ + ": " + str(primary))

    def test_control_primary_with_healthy_final_save_keeps_exact_api_identity(self):
        for primary in (KeyboardInterrupt("controller operation"), SystemExit(47), SystemExit(0)):
            with self.subTest(primary=type(primary).__name__, code=getattr(primary, "code", None)), \
                    self.fixture(primary=primary) as fixture:
                run = fixture.capture()
                self.assertIs(run.error, primary)
                self.assert_failed_report(run)

    def test_active_body_failure_survives_before_and_after_write_final_save_failures(self):
        for primary in (OSError("controller operation"), StopIteration("controller operation"),
                        KeyboardInterrupt("controller operation"), SystemExit(47), SystemExit(0)):
            for secondary in (OSError("final manifest"), SystemExit(0)):
                for after_write in (False, True):
                    with self.subTest(primary=type(primary).__name__, code=getattr(primary, "code", None),
                                      secondary=type(secondary).__name__, after_write=after_write), \
                            self.fixture(primary=primary, save_error=secondary, after_write=after_write) as fixture:
                        expected = "matrix final report: " + type(secondary).__name__ + ": " + str(secondary)
                        before = log_fixture.MATRIX.probe.case_error_fields(primary)
                        earlier = [*before.get("errorNotes", ()), *before.get("cleanupErrors", ())]
                        run = fixture.capture()
                        self.assertIs(run.error, primary)
                        self.assert_failed_report(run)
                        fields = log_fixture.MATRIX.probe.case_error_fields(primary)
                        notes = [*fields.get("errorNotes", ()), *fields.get("cleanupErrors", ())]
                        self.assertEqual(notes.count(expected), earlier.count(expected) + 1)
                        if isinstance(primary, Exception) and after_write:
                            self.assertEqual(run.manifest["error"], type(primary).__name__ + ": " + str(primary))

    def test_save_only_api_failures_keep_exact_native_identity_and_saved_verdict_limit(self):
        for failure in (OSError("final manifest"), StopIteration("final manifest"),
                        KeyboardInterrupt("final manifest"), SystemExit(47), SystemExit(),
                        SystemExit(0), SystemExit(False), SystemExit(256)):
            for after_write in (False, True):
                with self.subTest(failure=type(failure).__name__, code=getattr(failure, "code", None),
                                  after_write=after_write), \
                        self.fixture(save_error=failure, after_write=after_write) as fixture:
                    run = fixture.capture()
                    self.assertIs(run.error, failure)
                    self.assert_healthy_body_report(run, after_write=after_write)

    def test_original_entry_refuses_save_zero_including_healthy_bytes_already_written(self):
        for failure in (SystemExit(), SystemExit(0), SystemExit(False), SystemExit(256), SystemExit(-256)):
            for after_write in (False, True):
                with self.subTest(code=failure.code, after_write=after_write), \
                        self.fixture(save_error=failure, after_write=after_write) as fixture:
                    run = fixture.capture(entry=True)
                    self.assertIsInstance(run.error, SystemExit)
                    self.assertEqual(run.error.code, 1)
                    self.assert_healthy_body_report(run, after_write=after_write)
                    self.assertFalse(run.error.code == 0 and run.manifest["passed"] is True)

    def test_original_entry_handles_ordinary_save_errors_and_preserves_other_controls(self):
        for failure in (OSError("final manifest"), StopIteration("final manifest"),
                        KeyboardInterrupt("final manifest"), SystemExit(47)):
            for after_write in (False, True):
                with self.subTest(failure=type(failure).__name__, code=getattr(failure, "code", None),
                                  after_write=after_write), \
                        self.fixture(save_error=failure, after_write=after_write) as fixture:
                    run = fixture.capture(entry=True)
                    if isinstance(failure, Exception):
                        self.assertIsInstance(run.error, SystemExit)
                        self.assertEqual(run.error.code, 1)
                    else:
                        self.assertIs(run.error, failure)
                    self.assert_healthy_body_report(run, after_write=after_write)

    def test_original_entry_selects_body_primary_when_final_save_also_fails(self):
        for primary in (RuntimeError("controller operation"), KeyboardInterrupt("controller operation"),
                        SystemExit(47), SystemExit(0)):
            for secondary in (OSError("final manifest"), SystemExit(0)):
                for after_write in (False, True):
                    with self.subTest(primary=type(primary).__name__, code=getattr(primary, "code", None),
                                      secondary=type(secondary).__name__, after_write=after_write), \
                            self.fixture(primary=primary, save_error=secondary, after_write=after_write) as fixture:
                        run = fixture.capture(entry=True)
                        if isinstance(primary, Exception) or isinstance(primary, SystemExit) and primary.code == 0:
                            self.assertIsInstance(run.error, SystemExit)
                            self.assertEqual(run.error.code, 1)
                        else:
                            self.assertIs(run.error, primary)
                        self.assert_failed_report(run)

    def test_unavailable_save_message_and_optional_note_hooks_cannot_replace_primary(self):
        class UnavailableSave(OSError):
            def __str__(self):
                raise SystemExit(0)

        class UnavailableNotes(KeyboardInterrupt):
            def add_note(self, _note):
                raise SystemExit(0)

            def __setattr__(self, _name, _value):
                raise SystemExit(0)

        for primary in (KeyboardInterrupt("controller operation"), UnavailableNotes("controller operation")):
            for after_write in (False, True):
                with self.subTest(primary=type(primary).__name__, after_write=after_write), \
                        self.fixture(primary=primary, save_error=UnavailableSave(), after_write=after_write) as fixture:
                    run = fixture.capture()
                    self.assertIs(run.error, primary)
                    self.assert_failed_report(run)
                    if not isinstance(primary, UnavailableNotes):
                        fields = log_fixture.MATRIX.probe.case_error_fields(primary)
                        notes = [*fields.get("errorNotes", ()), *fields.get("cleanupErrors", ())]
                        self.assertIn("matrix final report: UnavailableSave: <message unavailable>", notes)

    def test_healthy_body_inside_unrelated_except_has_no_operation_primary(self):
        for failure in (None, OSError("final manifest"), SystemExit(0)):
            for after_write in ((False,) if failure is None else (False, True)):
                with self.subTest(failure=type(failure).__name__, after_write=after_write):
                    try:
                        raise LookupError("unrelated caller failure")
                    except LookupError:
                        with self.fixture(save_error=failure, after_write=after_write) as fixture:
                            run = fixture.capture()
                    if failure is None:
                        self.assertIsNone(run.error)
                        self.assertEqual(run.result.status, 0)
                        self.assertIs(run.manifest["passed"], True)
                    else:
                        self.assertIs(run.error, failure)
                        self.assert_healthy_body_report(run, after_write=after_write)
