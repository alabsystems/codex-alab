# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""CLI-owned output completion preserves statuses and cancellation priority."""

import errno
import importlib.util
from pathlib import Path
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "qualification_cli_output", Path(__file__).resolve().parents[1] / "qualification.py")
QUAL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(QUAL)


class OwnedOutput:
    """An owned flush callback with no OS descriptor or hardware resource."""

    def __init__(self, name, events, error):
        self.name, self.events, self.error = name, events, error

    def flush(self):
        self.events.append(self.name + "-flush")
        if self.error is not None:
            raise self.error


class QualificationCliOutputTests(unittest.TestCase):
    def exercise(self, *, status=0, primary=None, stdout_error=None, stderr_error=None):
        events = []
        returned = caught = None

        def main():
            events.append("main")
            if primary is not None:
                raise primary
            return status

        stdout = OwnedOutput("stdout", events, stdout_error)
        stderr = OwnedOutput("stderr", events, stderr_error)
        with mock.patch.object(QUAL, "main", main), \
                mock.patch.object(QUAL.sys, "stdout", stdout), \
                mock.patch.object(QUAL.sys, "stderr", stderr):
            try:
                returned = QUAL.cli()
            except BaseException as error:
                caught = error
        self.assertEqual(events, ["main", "stdout-flush", "stderr-flush"])
        return returned, caught

    def test_returned_status_survives_healthy_owned_output_completion(self):
        for status in (0, 23, 75, 143):
            with self.subTest(status=status):
                returned, caught = self.exercise(status=status)
                self.assertIsNone(caught)
                self.assertEqual(returned, status)

    def test_pending_cancellation_survives_output_error_and_later_cancellation(self):
        cases = (
            (KeyboardInterrupt("owned main cancellation"), SystemExit(59)),
            (SystemExit(47), KeyboardInterrupt("later owned stderr cancellation")),
        )
        for primary, later in cases:
            with self.subTest(primary=type(primary).__name__, later=type(later).__name__):
                ordinary = OSError(errno.EIO, "owned stdout flush failure")
                returned, caught = self.exercise(
                    primary=primary, stdout_error=ordinary, stderr_error=later)
                self.assertIsNone(returned)
                self.assertIs(caught, primary)
                notes = "\n".join(getattr(caught, "__notes__", ()))
                self.assertIn(repr(ordinary), notes)
                self.assertIn(repr(later), notes)

    def test_successful_help_requires_output_completion_and_keeps_new_cancellation(self):
        help_exit = SystemExit(0)
        returned, caught = self.exercise(primary=help_exit)
        self.assertIsNone(returned)
        self.assertIs(caught, help_exit)
        self.assertEqual(caught.code, 0)

        returned, caught = self.exercise(
            primary=SystemExit(0), stdout_error=OSError(errno.EIO, "owned help flush failure"))
        self.assertEqual(returned, 1)
        self.assertIsNone(caught)

        for cancellation in (KeyboardInterrupt("owned help output cancellation"), SystemExit(47)):
            with self.subTest(cancellation=type(cancellation).__name__):
                returned, caught = self.exercise(
                    primary=SystemExit(0),
                    stdout_error=OSError(errno.EIO, "owned help flush failure"),
                    stderr_error=cancellation)
                self.assertIsNone(returned)
                self.assertIs(caught, cancellation)

    def test_flush_exits_cannot_report_success_or_replace_new_cancellation(self):
        for status in (0, 23):
            for code in (0, 256, None):
                with self.subTest(status=status, code=code):
                    returned, caught = self.exercise(status=status, stdout_error=SystemExit(code))
                    self.assertEqual(returned, status or 1)
                    self.assertIsNone(caught)
            for cancellation in (SystemExit(47), KeyboardInterrupt("owned flush cancellation")):
                with self.subTest(status=status, cancellation=type(cancellation).__name__):
                    returned, caught = self.exercise(status=status, stdout_error=cancellation)
                    self.assertIsNone(returned)
                    self.assertIs(caught, cancellation)
