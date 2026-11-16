#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Directory descriptor ownership when OS operations fail or are cancelled."""

import errno
import importlib.util
import os
from pathlib import Path
import stat
from types import SimpleNamespace
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "directory_cleanup_results", Path(__file__).resolve().parents[1] / "unit_results.py")
RESULTS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RESULTS)


class DescriptorLedger:
    """All descriptors are synthetic; no operation reaches the real os module."""

    def __init__(self, *, open_errors=None, close_errors=None):
        self.open_errors = open_errors or {}
        self.close_errors = close_errors or {}
        self.opened = []
        self.closed = []
        self.live = set()
        self.api = SimpleNamespace(open=self.open, close=self.close,
            **{name: getattr(os, name) for name in
               ("O_RDONLY", "O_DIRECTORY", "O_NOFOLLOW", "O_CLOEXEC")})

    def open(self, path, flags, *, dir_fd=None):
        if path in self.open_errors:
            raise self.open_errors[path]
        descriptor = 100 + len(self.opened)
        self.opened.append((path, flags, dir_fd, descriptor))
        self.live.add(descriptor)
        return descriptor

    def close(self, descriptor):
        self.closed.append(descriptor)
        if descriptor not in self.live:
            raise OSError(errno.EBADF, "synthetic descriptor was already consumed")
        self.live.remove(descriptor)
        if descriptor in self.close_errors:
            raise self.close_errors[descriptor]


class UnitDirectoryCleanupTests(unittest.TestCase):
    def caught(self, ledger, path="/first/second"):
        with mock.patch.object(RESULTS, "os", ledger.api):
            try:
                RESULTS.open_directory(path)
            except BaseException as error:
                return error
        self.fail("the controlled operation did not raise")

    def assert_parent_close_failure(self, primary, cleanup=None):
        errors = {100: primary}
        if cleanup is not None:
            errors[101] = cleanup
        ledger = DescriptorLedger(close_errors=errors)
        caught = self.caught(ledger)
        self.assertEqual(ledger.closed, [100, 101])
        self.assertEqual(ledger.live, set())
        self.assertIs(caught, primary)
        if cleanup is not None:
            self.assertIn(repr(cleanup), "\n".join(getattr(caught, "__notes__", [])))

    def test_walk_returns_only_the_owned_leaf_and_keeps_nofollow_flags(self):
        ledger = DescriptorLedger()
        with mock.patch.object(RESULTS, "os", ledger.api):
            leaf = RESULTS.open_directory("/first/second")
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
        self.assertEqual(ledger.opened,
                         [("/", flags, None, 100), ("first", flags, 100, 101),
                          ("second", flags, 101, 102)])
        self.assertEqual(leaf, 102)
        self.assertEqual(ledger.closed, [100, 101])
        self.assertEqual(ledger.live, {102})

    def test_root_returns_the_original_descriptor_without_closing_it(self):
        ledger = DescriptorLedger()
        with mock.patch.object(RESULTS, "os", ledger.api):
            self.assertEqual(RESULTS.open_directory("/"), 100)
        self.assertEqual(ledger.closed, [])
        self.assertEqual(ledger.live, {100})

    def test_noncanonical_paths_refuse_before_any_descriptor_is_opened(self):
        for path in ("relative", "/first/../second", "//first/second"):
            with self.subTest(path=path):
                ledger = DescriptorLedger()
                self.assertIsInstance(self.caught(ledger, path), RESULTS.Refusal)
                self.assertEqual(ledger.opened, [])
                self.assertEqual(ledger.closed, [])

    def test_failed_root_open_is_preserved_without_attempting_close(self):
        primary = OSError(errno.EPERM, "controlled root open")
        ledger = DescriptorLedger(open_errors={"/": primary})
        self.assertIs(self.caught(ledger), primary)
        self.assertEqual(ledger.closed, [])
        self.assertEqual(ledger.live, set())

    def test_failed_child_open_closes_its_still_owned_parent(self):
        primary = OSError(errno.ENOENT, "controlled child open")
        ledger = DescriptorLedger(open_errors={"second": primary})
        self.assertIs(self.caught(ledger), primary)
        self.assertEqual(ledger.closed, [100, 101])
        self.assertEqual(ledger.live, set())

    def test_consumed_parent_close_eio_preserves_the_primary(self):
        self.assert_parent_close_failure(OSError(errno.EIO, "controlled parent close"))

    def test_consumed_parent_close_keyboard_interrupt_preserves_cancellation(self):
        self.assert_parent_close_failure(KeyboardInterrupt("controlled parent close"))

    def test_consumed_parent_close_system_exit_preserves_cancellation(self):
        self.assert_parent_close_failure(SystemExit(47))

    def test_deeper_parent_close_failure_cleans_the_new_leaf(self):
        primary = OSError(errno.EIO, "controlled second parent close")
        ledger = DescriptorLedger(close_errors={101: primary})
        self.assertIs(self.caught(ledger, "/first/second/third"), primary)
        self.assertEqual(ledger.closed, [100, 101, 102])
        self.assertEqual(ledger.live, set())

    def test_secondary_close_eio_does_not_replace_the_parent_close_error(self):
        self.assert_parent_close_failure(OSError(errno.EIO, "controlled parent close"),
                                         OSError(errno.EIO, "controlled child cleanup"))

    def test_secondary_close_eio_does_not_replace_pending_cancellation(self):
        for primary in (KeyboardInterrupt("controlled parent close"), SystemExit(47)):
            with self.subTest(primary=type(primary).__name__):
                self.assert_parent_close_failure(primary,
                                                 OSError(errno.EIO, "controlled child cleanup"))

    def test_cleanup_cancellation_wins_over_an_ordinary_open_error(self):
        for cancellation in (KeyboardInterrupt("controlled cleanup"), SystemExit(48)):
            with self.subTest(cancellation=type(cancellation).__name__):
                primary = OSError(errno.ENOENT, "controlled child open")
                ledger = DescriptorLedger(open_errors={"first": primary},
                                          close_errors={100: cancellation})
                caught = self.caught(ledger)
                self.assertIs(caught, cancellation)
                self.assertIs(caught.__cause__, primary)
                self.assertIn(repr(primary), "\n".join(getattr(caught, "__notes__", [])))
                self.assertEqual(ledger.closed, [100])
                self.assertEqual(ledger.live, set())

    def test_pending_cancellation_wins_over_later_cleanup_cancellation(self):
        self.assert_parent_close_failure(KeyboardInterrupt("controlled parent close"), SystemExit(48))


class DiagnosticsRootCleanupTests(unittest.TestCase):
    def exercise(self, *, primary=None, cleanup=None, mismatch=False, invalid=False, open_error=None):
        ledger = DescriptorLedger(close_errors={100: cleanup} if cleanup is not None else None)
        state = {"target_dir": "/target",
                 "diagnostics_root": "/target/" + RESULTS.DIAGNOSTICS_PREFIX + "control",
                 "diagnostics_root_identity": [17, 23]}
        if invalid:
            state["diagnostics_root"] = "/other/" + RESULTS.DIAGNOSTICS_PREFIX + "control"
        opening = mock.Mock(side_effect=open_error if open_error is not None
                            else lambda path: ledger.open(str(path), 0))
        identity = mock.Mock(side_effect=primary) if primary is not None else mock.Mock(
            return_value=[17, 24] if mismatch else [17, 23])
        refusals = []
        real_require = RESULTS.require

        def require(condition, message):
            try:
                real_require(condition, message)
            except RESULTS.Refusal as error:
                refusals.append(error)
                raise

        caught, returned = None, None
        with mock.patch.object(RESULTS, "os", ledger.api), \
                mock.patch.object(RESULTS, "open_directory", opening), \
                mock.patch.object(RESULTS, "directory_identity", identity), \
                mock.patch.object(RESULTS, "require", require):
            try:
                returned = RESULTS.diagnostics_root(state)
            except BaseException as error:
                caught = error
        return ledger, opening, identity, refusals, caught, returned

    def assert_cleanup(self, primary, cleanup, *, mismatch=False):
        ledger, opening, identity, refusals, caught, returned = self.exercise(
            primary=primary, cleanup=cleanup, mismatch=mismatch)
        if mismatch:
            self.assertEqual(len(refusals), 1)
            primary = refusals[0]
        expected = (cleanup if isinstance(primary, Exception)
                    and not isinstance(cleanup, Exception) else primary)
        self.assertIsNone(returned)
        self.assertEqual(ledger.closed, [100])
        self.assertEqual(ledger.live, set())
        opening.assert_called_once()
        identity.assert_called_once_with(100)
        self.assertIs(caught, expected)
        if expected is cleanup:
            self.assertIs(caught.__cause__, primary)
            self.assertIn(repr(primary), "\n".join(getattr(caught, "__notes__", [])))
        else:
            self.assertIn(repr(cleanup), "\n".join(getattr(caught, "__notes__", [])))

    def test_successful_validation_returns_the_still_owned_descriptor(self):
        ledger, opening, identity, refusals, caught, returned = self.exercise()
        self.assertIsNone(caught)
        self.assertEqual(returned,
                         (Path("/target/" + RESULTS.DIAGNOSTICS_PREFIX + "control"), 100))
        self.assertEqual(ledger.live, {100})
        self.assertEqual(ledger.closed, [])
        self.assertEqual(refusals, [])
        opening.assert_called_once()
        identity.assert_called_once_with(100)

    def test_invalid_root_refuses_before_opening_or_closing_a_descriptor(self):
        ledger, opening, identity, refusals, caught, returned = self.exercise(invalid=True)
        self.assertIsNone(returned)
        self.assertEqual(len(refusals), 1)
        self.assertIs(caught, refusals[0])
        opening.assert_not_called()
        identity.assert_not_called()
        self.assertEqual(ledger.opened, [])
        self.assertEqual(ledger.closed, [])

    def test_open_failure_is_preserved_without_closing_a_descriptor(self):
        primary = OSError(errno.EPERM, "controlled root open")
        ledger, opening, identity, refusals, caught, returned = self.exercise(open_error=primary)
        self.assertIs(caught, primary)
        self.assertIsNone(returned)
        self.assertEqual(refusals, [])
        opening.assert_called_once()
        identity.assert_not_called()
        self.assertEqual(ledger.opened, [])
        self.assertEqual(ledger.closed, [])

    def test_identity_mismatch_refusal_closes_the_descriptor_once(self):
        ledger, opening, identity, refusals, caught, returned = self.exercise(mismatch=True)
        self.assertIsNone(returned)
        self.assertEqual(len(refusals), 1)
        self.assertIs(caught, refusals[0])
        self.assertIn("changed after preparation", str(caught))
        self.assertEqual(ledger.closed, [100])
        self.assertEqual(ledger.live, set())
        opening.assert_called_once()
        identity.assert_called_once_with(100)

    def test_actual_identity_mismatch_refusal_survives_close_eio(self):
        self.assert_cleanup(None, OSError(errno.EIO, "controlled root cleanup"), mismatch=True)

    def test_identity_refusal_survives_close_eio(self):
        self.assert_cleanup(RESULTS.Refusal("controlled identity refusal"),
                            OSError(errno.EIO, "controlled root cleanup"))

    def test_identity_oserror_survives_close_eio(self):
        self.assert_cleanup(OSError(errno.EIO, "controlled identity read"),
                            OSError(errno.EIO, "controlled root cleanup"))

    def test_identity_keyboard_interrupt_survives_close_eio(self):
        self.assert_cleanup(KeyboardInterrupt("controlled identity read"),
                            OSError(errno.EIO, "controlled root cleanup"))

    def test_identity_system_exit_survives_close_eio(self):
        self.assert_cleanup(SystemExit(47), OSError(errno.EIO, "controlled root cleanup"))

    def test_cleanup_keyboard_interrupt_overrides_ordinary_refusal_with_context(self):
        self.assert_cleanup(RESULTS.Refusal("controlled identity refusal"),
                            KeyboardInterrupt("controlled root cleanup"))

    def test_cleanup_system_exit_overrides_ordinary_refusal_with_context(self):
        self.assert_cleanup(RESULTS.Refusal("controlled identity refusal"), SystemExit(48))

    def test_pending_keyboard_interrupt_survives_cleanup_system_exit(self):
        self.assert_cleanup(KeyboardInterrupt("controlled identity read"), SystemExit(48))

    def test_pending_system_exit_survives_cleanup_keyboard_interrupt(self):
        self.assert_cleanup(SystemExit(47), KeyboardInterrupt("controlled root cleanup"))


class ManifestCleanupTests(unittest.TestCase):
    def exercise(self, *, primary=None, cleanup=None, scan_error=None,
                 mode=stat.S_IFREG | 0o600, size=11):
        ledger = DescriptorLedger(close_errors={100: cleanup} if cleanup is not None else None)
        entry = SimpleNamespace(name="case", is_dir=mock.Mock(return_value=True))
        scan = mock.MagicMock()
        scan.__enter__.return_value = iter([entry])
        scan.__exit__.return_value = False
        ledger.api.scandir = mock.Mock(side_effect=scan_error) if scan_error is not None else mock.Mock(
            return_value=scan)
        ledger.api.stat = mock.Mock(side_effect=primary) if primary is not None else mock.Mock(
            return_value=SimpleNamespace(st_mode=mode, st_size=size))
        caught, returned = None, None
        with mock.patch.object(RESULTS, "os", ledger.api):
            try:
                returned = RESULTS.diagnostic_manifests(500, Path("/prepared/leaf"))
            except BaseException as error:
                caught = error
        ledger.api.scandir.assert_called_once_with(500)
        if scan_error is None:
            entry.is_dir.assert_called_once_with(follow_symlinks=False)
            flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
            self.assertEqual(ledger.opened, [("case", flags, 500, 100)])
            ledger.api.stat.assert_called_once_with(RESULTS.DIAGNOSTIC_MANIFEST, dir_fd=100,
                                                   follow_symlinks=False)
        else:
            self.assertEqual(ledger.opened, [])
            ledger.api.stat.assert_not_called()
        return ledger, caught, returned

    def assert_primary_survives(self, primary, cleanup):
        ledger, caught, returned = self.exercise(primary=primary, cleanup=cleanup)
        self.assertEqual(ledger.closed, [100])
        self.assertEqual(ledger.live, set())
        self.assertIs(caught, primary)
        self.assertIsNone(returned)
        self.assertIn(repr(cleanup), "\n".join(getattr(caught, "__notes__", [])))

    def test_keyboard_interrupt_survives_secondary_close_eio(self):
        self.assert_primary_survives(KeyboardInterrupt("controlled manifest stat"),
                                    OSError(errno.EIO, "controlled child close"))

    def test_system_exit_survives_secondary_close_eio(self):
        self.assert_primary_survives(SystemExit(47), OSError(errno.EIO, "controlled child close"))

    def test_pending_keyboard_interrupt_survives_cleanup_system_exit(self):
        self.assert_primary_survives(KeyboardInterrupt("controlled manifest stat"), SystemExit(48))

    def test_pending_system_exit_survives_cleanup_keyboard_interrupt(self):
        self.assert_primary_survives(SystemExit(47), KeyboardInterrupt("controlled child close"))

    def test_nonadvisory_error_survives_secondary_close_eio(self):
        self.assert_primary_survives(RuntimeError("controlled manifest stat"),
                                    OSError(errno.EIO, "controlled child close"))

    def test_successful_regular_manifest_is_indexed_and_closed_once(self):
        ledger, caught, returned = self.exercise()
        self.assertIsNone(caught)
        self.assertEqual(returned, ["/prepared/leaf/case/manifest.json"])
        self.assertEqual(ledger.closed, [100])
        self.assertEqual(ledger.live, set())

    def test_missing_manifest_remains_advisory_and_child_is_closed(self):
        ledger, caught, returned = self.exercise(
            primary=OSError(errno.ENOENT, "controlled missing manifest"))
        self.assertIsNone(caught)
        self.assertEqual(returned, [])
        self.assertEqual(ledger.closed, [100])
        self.assertEqual(ledger.live, set())

    def test_scandir_oserror_remains_advisory_without_opening_a_child(self):
        ledger, caught, returned = self.exercise(
            scan_error=OSError(errno.EIO, "controlled scandir"))
        self.assertIsNone(caught)
        self.assertEqual(returned, [])
        self.assertEqual(ledger.closed, [])
        self.assertEqual(ledger.live, set())

    def test_nonregular_and_oversized_manifests_are_not_indexed(self):
        for values in ({"mode": stat.S_IFDIR | 0o700}, {"size": 1024 * 1024 + 1}):
            with self.subTest(values=values):
                ledger, caught, returned = self.exercise(**values)
                self.assertIsNone(caught)
                self.assertEqual(returned, [])
                self.assertEqual(ledger.closed, [100])
                self.assertEqual(ledger.live, set())

    def test_close_oserror_after_success_stays_advisory(self):
        ledger, caught, returned = self.exercise(cleanup=OSError(errno.EIO, "controlled child close"))
        self.assertIsNone(caught)
        self.assertEqual(returned, ["/prepared/leaf/case/manifest.json"])
        self.assertEqual(ledger.closed, [100])
        self.assertEqual(ledger.live, set())


if __name__ == "__main__":
    unittest.main()
