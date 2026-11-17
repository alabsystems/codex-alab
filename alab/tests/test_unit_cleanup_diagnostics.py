# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Cleanup diagnostics cannot replace the selected unit-result exception."""

import errno
import unittest
from unittest import mock

import test_unit_directory_cleanup as directory_tests


RESULTS = directory_tests.RESULTS


def hooked_error(base, hook, fault, calls, *arguments):
    def fail(self, *unused):
        calls.append(hook)
        raise fault

    if hook in ("__repr__", "add_note"):
        methods = {hook: fail}
    else:
        attribute = "__class__" if hook == "class lookup" else "add_note"

        def access(self, name):
            if name == attribute:
                calls.append(hook)
                raise fault
            return base.__getattribute__(self, name)

        methods = {"__getattribute__": access}
    return type("ControlledDiagnostic", (base,), methods)(*arguments)


class UnitCleanupDiagnosticsTests(unittest.TestCase):
    def walk(self, primary, cleanup):
        ledger = directory_tests.DescriptorLedger(
            open_errors={"first": primary}, close_errors={100: cleanup})
        caught = None
        with mock.patch.object(RESULTS, "os", ledger.api):
            try:
                RESULTS.open_directory("/first/second")
            except BaseException as error:
                caught = error
        # Check ownership independently before an identity assertion can fail.
        self.assertEqual(ledger.closed, [100])
        self.assertEqual(ledger.live, set())
        self.assertEqual(len(ledger.opened), 1)
        return caught

    @staticmethod
    def faults():
        return (OSError(errno.EIO, "diagnostic"), KeyboardInterrupt("diagnostic"),
                SystemExit(0), SystemExit(48))

    @staticmethod
    def primaries():
        return (OSError(errno.ENOENT, "primary"), KeyboardInterrupt("primary"), SystemExit(47))

    def test_secondary_repr_failures_preserve_the_primary(self):
        for primary in self.primaries():
            for fault in self.faults():
                with self.subTest(primary=type(primary).__name__, fault=type(fault).__name__):
                    calls = []
                    cleanup = hooked_error(OSError, "__repr__", fault, calls, errno.EIO, "cleanup")
                    caught = self.walk(primary, cleanup)
                    self.assertEqual(calls, ["__repr__"])
                    self.assertIs(caught, primary)
                    self.assertIn("directory traversal cleanup failed: <unprintable exception>",
                                  caught.__notes__)

    def test_note_calls_preserve_the_primary(self):
        for base, arguments in ((OSError, (errno.ENOENT, "primary")),
                                (KeyboardInterrupt, ("primary",)), (SystemExit, (47,))):
            for fault in self.faults():
                with self.subTest(primary=base.__name__, fault=type(fault).__name__):
                    calls = []
                    primary = hooked_error(base, "add_note", fault, calls, *arguments)
                    caught = self.walk(primary, OSError(errno.EIO, "cleanup"))
                    self.assertEqual(calls, ["add_note"])
                    self.assertIs(caught, primary)

    def test_note_lookup_failures_preserve_the_primary(self):
        for base, arguments in ((OSError, (errno.ENOENT, "primary")),
                                (KeyboardInterrupt, ("primary",)), (SystemExit, (47,))):
            for fault in self.faults():
                with self.subTest(primary=base.__name__, fault=type(fault).__name__):
                    calls = []
                    primary = hooked_error(base, "note lookup", fault, calls, *arguments)
                    caught = self.walk(primary, OSError(errno.EIO, "cleanup"))
                    self.assertEqual(calls, ["note lookup"])
                    self.assertIs(caught, primary)

    def test_promoted_cancellation_survives_primary_repr_failures(self):
        for cleanup in (KeyboardInterrupt("cleanup"), SystemExit(48)):
            for fault in self.faults():
                with self.subTest(cleanup=type(cleanup).__name__, fault=type(fault).__name__):
                    calls = []
                    primary = hooked_error(OSError, "__repr__", fault, calls, errno.ENOENT, "primary")
                    caught = self.walk(primary, cleanup)
                    self.assertEqual(calls, ["__repr__"])
                    self.assertIs(caught, cleanup)
                    self.assertIs(BaseException.__cause__.__get__(caught), primary)
                    self.assertIn("directory traversal failed: <unprintable exception>",
                                  caught.__notes__)

    def test_promoted_cancellation_survives_note_failures(self):
        for base, arguments in ((KeyboardInterrupt, ("cleanup",)), (SystemExit, (48,))):
            for hook in ("add_note", "note lookup"):
                for fault in self.faults():
                    with self.subTest(cleanup=base.__name__, hook=hook, fault=type(fault).__name__):
                        calls = []
                        cleanup = hooked_error(base, hook, fault, calls, *arguments)
                        primary = OSError(errno.ENOENT, "primary")
                        caught = self.walk(primary, cleanup)
                        self.assertEqual(calls, [hook])
                        self.assertIs(caught, cleanup)
                        self.assertIs(BaseException.__cause__.__get__(caught), primary)

    def test_authority_selection_does_not_consult_foreign_class_attributes(self):
        for base, arguments in ((KeyboardInterrupt, ("primary",)), (SystemExit, (47,))):
            with self.subTest(primary=base.__name__):
                calls = []
                primary = hooked_error(base, "class lookup", SystemExit(0), calls, *arguments)
                caught = self.walk(primary, OSError(errno.EIO, "cleanup"))
                self.assertEqual(calls, [])
                self.assertIs(caught, primary)
        calls = []
        primary = OSError(errno.ENOENT, "primary")
        cleanup = hooked_error(KeyboardInterrupt, "class lookup", SystemExit(0), calls, "cleanup")
        caught = self.walk(primary, cleanup)
        self.assertEqual(calls, [])
        self.assertIs(caught, cleanup)
        self.assertIs(BaseException.__cause__.__get__(caught), primary)

    def test_normal_notes_and_cancellation_precedence_keep_their_wording(self):
        for primary in self.primaries():
            cleanup = OSError(errno.EIO, "cleanup")
            caught = self.walk(primary, cleanup)
            self.assertIs(caught, primary)
            self.assertIn("directory traversal cleanup failed: " + repr(cleanup), caught.__notes__)
        for cleanup in (KeyboardInterrupt("cleanup"), SystemExit(48)):
            primary = OSError(errno.ENOENT, "primary")
            caught = self.walk(primary, cleanup)
            self.assertIs(caught, cleanup)
            self.assertIs(BaseException.__cause__.__get__(caught), primary)
            self.assertIn("directory traversal failed: " + repr(primary), caught.__notes__)

    def test_absent_optional_note_transport_does_not_change_authority(self):
        calls = []
        primary = hooked_error(KeyboardInterrupt, "note lookup", AttributeError("no notes"),
                               calls, "primary")
        self.assertIs(self.walk(primary, OSError(errno.EIO, "cleanup")), primary)
        self.assertEqual(calls, ["note lookup"])
        calls = []
        primary = OSError(errno.ENOENT, "primary")
        cleanup = hooked_error(KeyboardInterrupt, "note lookup", AttributeError("no notes"),
                               calls, "cleanup")
        caught = self.walk(primary, cleanup)
        self.assertEqual(calls, ["note lookup"])
        self.assertIs(caught, cleanup)
        self.assertIs(BaseException.__cause__.__get__(caught), primary)


if __name__ == "__main__":
    unittest.main()
