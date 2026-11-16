#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Preparation resource ownership and cancellation across independent cleanups."""

import errno
import importlib.util
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "prepare_cleanup_results", Path(__file__).resolve().parents[1] / "unit_results.py")
RESULTS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RESULTS)


class PreparationLedger:
    """All directory descriptors and creation/removal operations are synthetic."""

    def __init__(self, *, entropy_error=None, parent_error=None, mkdir_error=None,
                 child_error=None, rollback_error=None, close_errors=None):
        self.entropy_error, self.parent_error = entropy_error, parent_error
        self.mkdir_error, self.child_error = mkdir_error, child_error
        self.rollback_error = rollback_error
        self.close_errors = close_errors or {}
        self.entropy_calls, self.parent_calls, self.mkdir_calls = [], [], []
        self.open_calls, self.rollback_calls, self.closed = [], [], []
        self.live, self.created = set(), False
        self.api = SimpleNamespace(urandom=self.urandom, mkdir=self.mkdir,
            open=self.open, close=self.close, rmdir=self.rmdir,
            **{name: getattr(os, name) for name in
               ("O_RDONLY", "O_DIRECTORY", "O_NOFOLLOW", "O_CLOEXEC")})

    def urandom(self, size):
        self.entropy_calls.append(size)
        if len(self.entropy_calls) == 1 and self.entropy_error is not None:
            raise self.entropy_error
        return bytes([len(self.entropy_calls)]) * size

    def parent(self, path):
        self.parent_calls.append(path)
        if self.parent_error is not None:
            raise self.parent_error
        self.live.add(100)
        return 100

    def mkdir(self, name, *, mode, dir_fd):
        self.mkdir_calls.append((name, mode, dir_fd))
        if self.mkdir_error is not None:
            raise self.mkdir_error
        self.created = True

    def open(self, name, flags, *, dir_fd):
        self.open_calls.append((name, flags, dir_fd))
        if self.child_error is not None:
            raise self.child_error
        self.live.add(101)
        return 101

    def close(self, descriptor):
        self.closed.append(descriptor)
        if descriptor not in self.live:
            raise OSError(errno.EBADF, "synthetic descriptor was already consumed")
        self.live.remove(descriptor)
        if descriptor in self.close_errors:
            raise self.close_errors[descriptor]

    def rmdir(self, name, *, dir_fd):
        self.rollback_calls.append((name, dir_fd))
        if self.rollback_error is not None:
            raise self.rollback_error
        self.created = False


class UnitPrepareCleanupTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="alab-prepare-cleanup-")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.root, self.target, self.state = [self.base / name for name in ("repo", "target", "state")]
        package = self.root / "codex-rs" / "control"
        (package / "src").mkdir(parents=True)
        self.target.mkdir()
        self.state.mkdir()
        manifest, source = package / "Cargo.toml", package / "src/lib.rs"
        manifest.write_text('[package]\nname="control"\nversion="0.0.0"\n')
        source.write_text("// inert fixture source\n")
        binary = self.target / "control-artifact"
        binary.write_text("inert regular artifact; never executed\n")
        package_id = "path+" + package.as_uri() + "#control@0.0.0"
        library = {"kind": ["lib"], "name": "control_lib", "src_path": str(source)}
        metadata = {"workspace_root": str(self.root / "codex-rs"),
                    "target_directory": str(self.target), "workspace_members": [package_id],
                    "packages": [{"id": package_id, "name": "control", "version": "0.0.0",
                                  "manifest_path": str(manifest), "targets": [library]}]}
        artifact = {"reason": "compiler-artifact", "profile": {"test": True},
                    "package_id": package_id, "manifest_path": str(manifest),
                    "target": library, "executable": str(binary)}
        metadata_path, artifacts_path = self.state / "metadata.json", self.state / "artifacts.jsonl"
        metadata_path.write_text(json.dumps(metadata))
        artifacts_path.write_text(json.dumps(artifact) + "\n")
        self.args = SimpleNamespace(repo=str(self.root), target_dir=str(self.target),
            state=str(self.state), metadata=str(metadata_path), artifacts=str(artifacts_path),
            package=["control"], plan=None)

    def exercise(self, *, identity_error=None, save_error=None, comparison=False,
                 existing_state=False, **faults):
        ledger = PreparationLedger(**faults)
        saved = []
        if comparison:
            planned = {"package": "control", "version": "0.0.0",
                       "manifest": "codex-rs/control/Cargo.toml", "target": "control_lib",
                       "source": "codex-rs/control/src/lib.rs", "test": "control::test"}
            plan = self.state / "plan.json"
            plan.write_text(json.dumps({"failed": [planned]}))
            self.args.plan = str(plan)
        if existing_state:
            (self.state / "state.json").write_text("existing state must survive\n")
        real_save = RESULTS.save

        def save(path, value, exclusive=False):
            saved.append((path, json.loads(json.dumps(value)), exclusive))
            if save_error is not None:
                raise save_error
            if existing_state:
                real_save(path, value, exclusive=exclusive)

        identity = mock.Mock(side_effect=identity_error) if identity_error is not None else mock.Mock(
            return_value=[17, 23])
        caught, completed = None, False
        with mock.patch.object(RESULTS, "os", ledger.api), \
                mock.patch.object(RESULTS, "open_directory", ledger.parent), \
                mock.patch.object(RESULTS, "directory_identity", identity), \
                mock.patch.object(RESULTS, "save", save):
            try:
                RESULTS.prepare(self.args)
                completed = True
            except BaseException as error:
                caught = error
        return ledger, saved, identity, caught, completed

    def assert_failure(self, primary, *, closes=(101, 100), rollback=True,
                       selected=None, cause=None, notes=(), **faults):
        ledger, saved, identity, caught, completed = self.exercise(**faults)
        self.assertFalse(completed)
        self.assertEqual(ledger.closed, list(closes))
        self.assertEqual(ledger.live, set())
        self.assertEqual(len(ledger.rollback_calls), int(rollback))
        self.assertIs(caught, primary if selected is None else selected)
        if cause is not None:
            self.assertIs(caught.__cause__, cause)
        for secondary in notes:
            self.assertIn(repr(secondary), "\n".join(getattr(caught, "__notes__", [])))
        return ledger, saved, identity

    def test_entropy_failure_precedes_any_descriptor_acquisition(self):
        for primary in (OSError(errno.EIO, "controlled entropy"),
                        KeyboardInterrupt("controlled entropy"), SystemExit(47)):
            with self.subTest(primary=type(primary).__name__):
                ledger, saved, identity, caught, completed = self.exercise(entropy_error=primary)
                self.assertIs(caught, primary)
                self.assertFalse(completed)
                self.assertEqual(ledger.live, set())
                self.assertEqual(ledger.parent_calls, [])
                self.assertEqual(ledger.closed, [])
                self.assertEqual(ledger.mkdir_calls, [])
                self.assertEqual(saved, [])
                identity.assert_not_called()

    def test_identity_error_survives_child_close_eio(self):
        for primary in (RESULTS.Refusal("controlled identity"),
                        KeyboardInterrupt("controlled identity"), SystemExit(47)):
            with self.subTest(primary=type(primary).__name__):
                cleanup = OSError(errno.EIO, "controlled child close")
                self.assert_failure(primary, identity_error=primary,
                                    close_errors={101: cleanup}, notes=(cleanup,))

    def test_child_cleanup_cancellation_overrides_ordinary_identity_error(self):
        for cancellation in (KeyboardInterrupt("controlled child close"), SystemExit(48)):
            with self.subTest(cancellation=type(cancellation).__name__):
                primary = RESULTS.Refusal("controlled identity")
                self.assert_failure(primary, selected=cancellation, cause=primary,
                    identity_error=primary, close_errors={101: cancellation}, notes=(primary,))

    def test_pending_identity_cancellation_survives_child_cleanup_cancellation(self):
        for primary, cleanup in ((KeyboardInterrupt("controlled identity"), SystemExit(48)),
                                 (SystemExit(47), KeyboardInterrupt("controlled child close"))):
            with self.subTest(primary=type(primary).__name__):
                self.assert_failure(primary, identity_error=primary,
                                    close_errors={101: cleanup}, notes=(cleanup,))

    def test_save_error_survives_rollback_eio(self):
        for primary in (RESULTS.Refusal("controlled save"),
                        KeyboardInterrupt("controlled save"), SystemExit(47)):
            with self.subTest(primary=type(primary).__name__):
                cleanup = OSError(errno.EIO, "controlled rollback")
                self.assert_failure(primary, save_error=primary,
                                    rollback_error=cleanup, notes=(cleanup,))

    def test_rollback_cancellation_overrides_ordinary_save_error(self):
        for cancellation in (KeyboardInterrupt("controlled rollback"), SystemExit(48)):
            with self.subTest(cancellation=type(cancellation).__name__):
                primary = RESULTS.Refusal("controlled save")
                self.assert_failure(primary, selected=cancellation, cause=primary,
                    save_error=primary, rollback_error=cancellation, notes=(primary,))

    def test_pending_save_cancellation_survives_rollback_cancellation(self):
        for primary, cleanup in ((KeyboardInterrupt("controlled save"), SystemExit(48)),
                                 (SystemExit(47), KeyboardInterrupt("controlled rollback"))):
            with self.subTest(primary=type(primary).__name__):
                self.assert_failure(primary, save_error=primary,
                                    rollback_error=cleanup, notes=(cleanup,))

    def test_save_error_survives_target_close_eio(self):
        for primary in (RESULTS.Refusal("controlled save"),
                        KeyboardInterrupt("controlled save"), SystemExit(47)):
            with self.subTest(primary=type(primary).__name__):
                cleanup = OSError(errno.EIO, "controlled target close")
                self.assert_failure(primary, save_error=primary,
                                    close_errors={100: cleanup}, notes=(cleanup,))

    def test_first_cleanup_cancellation_survives_remaining_cleanup_failures(self):
        primary = RESULTS.Refusal("controlled identity")
        selected = KeyboardInterrupt("controlled child close")
        rollback = SystemExit(48)
        target = OSError(errno.EIO, "controlled target close")
        self.assert_failure(primary, selected=selected, cause=primary, identity_error=primary,
            close_errors={101: selected, 100: target}, rollback_error=rollback,
            notes=(primary, rollback, target))

    def test_child_open_cancellation_survives_rollback_and_target_close_failures(self):
        primary = KeyboardInterrupt("controlled child open")
        rollback = OSError(errno.EIO, "controlled rollback")
        target = SystemExit(48)
        self.assert_failure(primary, child_error=primary, rollback_error=rollback,
            close_errors={100: target}, closes=(100,), notes=(rollback, target))

    def test_parent_open_failure_owns_no_descriptor(self):
        primary = OSError(errno.EPERM, "controlled target open")
        ledger, saved, identity = self.assert_failure(primary, parent_error=primary,
                                                     closes=(), rollback=False)
        self.assertEqual(ledger.open_calls, [])
        self.assertEqual(ledger.mkdir_calls, [])
        self.assertEqual(saved, [])
        identity.assert_not_called()

    def test_mkdir_failure_closes_only_target_without_rollback(self):
        primary = OSError(errno.EACCES, "controlled mkdir")
        ledger, saved, identity = self.assert_failure(primary, mkdir_error=primary,
                                                     closes=(100,), rollback=False)
        self.assertFalse(ledger.created)
        self.assertEqual(ledger.open_calls, [])
        self.assertEqual(saved, [])
        identity.assert_not_called()

    def test_child_open_failure_rolls_back_created_root(self):
        primary = OSError(errno.EACCES, "controlled child open")
        ledger, saved, identity = self.assert_failure(primary, child_error=primary, closes=(100,))
        self.assertFalse(ledger.created)
        self.assertEqual(saved, [])
        identity.assert_not_called()

    def test_successful_record_preserves_flags_mode_and_exclusive_save(self):
        ledger, saved, identity, caught, completed = self.exercise()
        self.assertIsNone(caught)
        self.assertTrue(completed)
        self.assertEqual(ledger.live, set())
        self.assertEqual(ledger.closed, [101, 100])
        self.assertEqual(ledger.entropy_calls, [16, 16])
        self.assertEqual(ledger.parent_calls, [self.target])
        name = RESULTS.DIAGNOSTICS_PREFIX + "01" * 16
        self.assertEqual(ledger.mkdir_calls, [(name, 0o700, 100)])
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
        self.assertEqual(ledger.open_calls, [(name, flags, 100)])
        self.assertEqual(ledger.rollback_calls, [])
        self.assertTrue(ledger.created)
        identity.assert_called_once_with(101)
        self.assertEqual(len(saved), 1)
        path, value, exclusive = saved[0]
        self.assertEqual(path, self.state / "state.json")
        self.assertIs(exclusive, True)
        self.assertEqual(value["mode"], "record")
        self.assertEqual(value["rounds"], 1)
        self.assertEqual(value["diagnostics_root_identity"], [17, 23])
        self.assertEqual(value["diagnostics_root"], str(self.target / name))
        self.assertEqual(value["copies"], [])
        self.assertEqual(len(value["libraries"][0]["diagnostics_dirs"]), 1)

    def test_successful_comparison_retains_selected_copy_and_diagnostics(self):
        ledger, saved, identity, caught, completed = self.exercise(comparison=True)
        self.assertIsNone(caught)
        self.assertTrue(completed)
        self.assertEqual(ledger.closed, [101, 100])
        self.assertEqual(ledger.live, set())
        self.assertEqual(ledger.rollback_calls, [])
        value = saved[0][1]
        self.assertIs(saved[0][2], True)
        self.assertEqual((value["mode"], value["rounds"]), ("compare", 1))
        self.assertEqual(value["copies"][0]["test"], "control::test")
        self.assertEqual(value["copies"][0]["round"], 0)
        self.assertIsNotNone(value["libraries"][0]["diagnostics_dirs"][0])
        identity.assert_called_once_with(101)

    def test_existing_state_is_never_overwritten(self):
        ledger, saved, identity, caught, completed = self.exercise(existing_state=True)
        self.assertFalse(completed)
        self.assertIsInstance(caught, FileExistsError)
        self.assertEqual((self.state / "state.json").read_text(), "existing state must survive\n")
        self.assertIs(saved[0][2], True)
        self.assertEqual(ledger.closed, [101, 100])
        self.assertEqual(ledger.live, set())
        self.assertFalse(ledger.created)
        self.assertEqual(len(ledger.rollback_calls), 1)
        identity.assert_called_once_with(101)

    def test_normal_target_close_failure_propagates_without_rollback(self):
        primary = OSError(errno.EIO, "controlled target close")
        ledger, saved, identity, caught, completed = self.exercise(close_errors={100: primary})
        self.assertFalse(completed)
        self.assertIs(caught, primary)
        self.assertEqual(ledger.closed, [101, 100])
        self.assertEqual(ledger.live, set())
        self.assertEqual(ledger.rollback_calls, [])
        self.assertTrue(ledger.created)
        self.assertEqual(len(saved), 1)
        identity.assert_called_once_with(101)

    def test_identity_failure_without_secondary_failure_retires_both_descriptors(self):
        primary = RESULTS.Refusal("controlled identity")
        ledger, saved, identity = self.assert_failure(primary, identity_error=primary)
        self.assertFalse(ledger.created)
        self.assertEqual(saved, [])
        identity.assert_called_once_with(101)

    def test_mkdir_cancellation_survives_target_close_eio(self):
        primary = KeyboardInterrupt("controlled mkdir")
        cleanup = OSError(errno.EIO, "controlled target close")
        self.assert_failure(primary, mkdir_error=primary, close_errors={100: cleanup},
                            closes=(100,), rollback=False, notes=(cleanup,))


if __name__ == "__main__":
    unittest.main()
