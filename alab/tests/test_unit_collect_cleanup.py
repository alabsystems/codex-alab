#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Collector diagnostics preserve failures while closing only owned descriptors."""

import errno
import importlib.util
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "collect_cleanup_results", Path(__file__).resolve().parents[1] / "unit_results.py")
RESULTS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RESULTS)


class DescriptorLedger:
    """The module receives synthetic descriptors; stdlib os is never patched."""

    def __init__(self, *, leaf_error=None, close_errors=None):
        self.leaf_error = leaf_error
        self.close_errors = close_errors or {}
        self.opened = []
        self.closed = []
        self.live = set()
        self.api = SimpleNamespace(open=self.open, close=self.close,
            **{name: getattr(os, name) for name in
               ("O_RDONLY", "O_DIRECTORY", "O_NOFOLLOW", "O_CLOEXEC")})

    def root(self):
        self.live.add(100)
        return 100

    def open(self, path, flags, *, dir_fd=None):
        self.opened.append((path, flags, dir_fd))
        if self.leaf_error is not None:
            raise self.leaf_error
        if dir_fd != 100 or 100 not in self.live or 101 in self.live:
            raise AssertionError("collector opened a leaf without its owned root")
        self.live.add(101)
        return 101

    def close(self, descriptor):
        self.closed.append(descriptor)
        if descriptor not in self.live:
            raise AssertionError("collector retried an already consumed descriptor")
        self.live.remove(descriptor)
        if descriptor in self.close_errors:
            # A close may consume its descriptor before reporting failure.
            raise self.close_errors[descriptor]


class UnitCollectCleanupTests(unittest.TestCase):
    def exercise(self, *, primary=None, primary_at="identity", mismatch=False,
                 receipt_mismatch=False, close_errors=None, leaf_error=None,
                 root_error=None):
        key = "a" * 64
        root = Path("/target/" + RESULTS.DIAGNOSTICS_PREFIX + "control")
        leaf_name = "library-" + key + "-round-0"
        leaf = str(root / leaf_name)
        library = {"key": key, "package": "package-control", "version": "1.0.0",
                   "manifest": "codex-rs/control/Cargo.toml", "target": "control",
                   "source": "codex-rs/control/src/lib.rs", "binary": "/target/control",
                   "cwd": "/repo/codex-rs/control", "diagnostics_dirs": [leaf]}
        state = {"libraries": [library], "rounds": 1, "mode": "record", "copies": []}
        receipt = {"library_key": key, "binary": library["binary"], "cwd": library["cwd"],
                   "mode": "record", "round": 0, "diagnostics_dir": leaf,
                   "diagnostics_identity": [17, 23]}
        if receipt_mismatch:
            receipt["binary"] = "/target/substituted"
        ledger = DescriptorLedger(leaf_error=leaf_error, close_errors=close_errors)
        refusals = []
        real_require = RESULTS.require

        def require(condition, message):
            try:
                real_require(condition, message)
            except RESULTS.Refusal as error:
                refusals.append(error)
                raise

        def load(path):
            if path.name == "state.json":
                return state
            self.assertEqual(path.name, "library-" + key + "-0.json")
            if primary is not None and primary_at == "receipt":
                raise primary
            return receipt

        def diagnostics_root(document):
            self.assertIs(document, state)
            if root_error is not None:
                raise root_error
            return root, ledger.root()

        def identity(descriptor):
            self.assertEqual(descriptor, 101)
            if primary is not None and primary_at == "identity":
                raise primary
            return [17, 24] if mismatch else [17, 23]

        def manifests(descriptor, path):
            self.assertEqual((descriptor, path), (101, Path(leaf)))
            if primary is not None and primary_at == "manifest":
                raise primary
            return [str(Path(leaf) / "fixture/manifest.json")]

        text = ("ALAB_UNIT_ROUND 0\nALAB_UNIT " + key + "\n"
                "test controlled::case ... ok\n"
                "test result: ok. 1 passed; 0 failed; 0 ignored\n")
        save_states = []
        save = mock.Mock(side_effect=lambda *args: save_states.append(
            (list(ledger.closed), set(ledger.live))))
        caught = None
        with tempfile.TemporaryDirectory(prefix="alab-collect-cleanup-") as temporary:
            state_dir = Path(temporary)
            (state_dir / "round-0.status").write_text("0")
            args = SimpleNamespace(state=str(state_dir), status=0, log="unused.log",
                                   out=str(state_dir / "out.json"))
            with mock.patch.object(RESULTS, "os", ledger.api), \
                    mock.patch.object(RESULTS, "load", load), \
                    mock.patch.object(RESULTS, "result_text", return_value=text), \
                    mock.patch.object(RESULTS, "diagnostics_root", diagnostics_root), \
                    mock.patch.object(RESULTS, "directory_identity", identity), \
                    mock.patch.object(RESULTS, "diagnostic_manifests", manifests), \
                    mock.patch.object(RESULTS, "require", require), \
                    mock.patch.object(RESULTS, "save", save):
                try:
                    RESULTS.collect(args)
                except BaseException as error:
                    caught = error
        return SimpleNamespace(ledger=ledger, caught=caught, save=save, refusals=refusals,
                               leaf=leaf, leaf_name=leaf_name, save_states=save_states)

    def assert_failed(self, observed, expected, *, closes=(101, 100), cleanup=()):
        self.assertEqual(observed.ledger.closed, list(closes))
        self.assertEqual(observed.ledger.live, set())
        observed.save.assert_not_called()
        self.assertIs(observed.caught, expected)
        for error in cleanup:
            self.assertIn(repr(error), "\n".join(getattr(expected, "__notes__", [])))

    def test_successful_collection_closes_both_descriptors_before_saving(self):
        observed = self.exercise()
        self.assertIsNone(observed.caught)
        self.assertEqual(observed.ledger.closed, [101, 100])
        self.assertEqual(observed.ledger.live, set())
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
        self.assertEqual(observed.ledger.opened, [(observed.leaf_name, flags, 100)])
        observed.save.assert_called_once()
        self.assertEqual(observed.save_states, [([101, 100], set())])
        result = observed.save.call_args.args[1]
        self.assertEqual(result["failed"], [])
        self.assertEqual(result["selected_libraries"], 1)
        self.assertEqual(result["diagnostics"][0]["directory"], observed.leaf)

    def test_root_open_failure_does_not_close_an_unowned_descriptor(self):
        primary = OSError(errno.EPERM, "controlled root open")
        observed = self.exercise(root_error=primary)
        self.assert_failed(observed, primary, closes=())
        self.assertEqual(observed.ledger.opened, [])

    def test_leaf_open_failure_closes_only_the_owned_root(self):
        primary = OSError(errno.ENOENT, "controlled leaf open")
        self.assert_failed(self.exercise(leaf_error=primary), primary, closes=(100,))

    def test_receipt_mismatch_refusal_survives_root_close_eio(self):
        cleanup = OSError(errno.EIO, "controlled root close")
        observed = self.exercise(receipt_mismatch=True, close_errors={100: cleanup})
        self.assertEqual(len(observed.refusals), 1)
        self.assertIn("runtime receipt", str(observed.refusals[0]))
        self.assert_failed(observed, observed.refusals[0], closes=(100,), cleanup=(cleanup,))
        self.assertEqual(observed.ledger.opened, [])

    def test_receipt_keyboard_interrupt_survives_root_close_eio(self):
        primary = KeyboardInterrupt("controlled receipt read")
        cleanup = OSError(errno.EIO, "controlled root close")
        self.assert_failed(self.exercise(primary=primary, primary_at="receipt",
                                        close_errors={100: cleanup}),
                           primary, closes=(100,), cleanup=(cleanup,))

    def test_receipt_system_exit_survives_root_close_keyboard_interrupt(self):
        primary = SystemExit(47)
        cleanup = KeyboardInterrupt("controlled root close")
        self.assert_failed(self.exercise(primary=primary, primary_at="receipt",
                                        close_errors={100: cleanup}),
                           primary, closes=(100,), cleanup=(cleanup,))

    def test_identity_mismatch_refusal_survives_both_close_eios(self):
        leaf = OSError(errno.EIO, "controlled leaf close")
        root = OSError(errno.EIO, "controlled root close")
        observed = self.exercise(mismatch=True, close_errors={101: leaf, 100: root})
        self.assertEqual(len(observed.refusals), 1)
        self.assertIn("leaf changed", str(observed.refusals[0]))
        self.assert_failed(observed, observed.refusals[0], cleanup=(leaf, root))

    def test_identity_keyboard_interrupt_survives_both_close_eios(self):
        primary = KeyboardInterrupt("controlled identity read")
        leaf = OSError(errno.EIO, "controlled leaf close")
        root = OSError(errno.EIO, "controlled root close")
        self.assert_failed(self.exercise(primary=primary, close_errors={101: leaf, 100: root}),
                           primary, cleanup=(leaf, root))

    def test_identity_system_exit_survives_both_close_cancellations(self):
        primary = SystemExit(47)
        leaf = KeyboardInterrupt("controlled leaf close")
        root = SystemExit(48)
        self.assert_failed(self.exercise(primary=primary, close_errors={101: leaf, 100: root}),
                           primary, cleanup=(leaf, root))

    def test_manifest_keyboard_interrupt_survives_both_close_eios(self):
        primary = KeyboardInterrupt("controlled manifest read")
        leaf = OSError(errno.EIO, "controlled leaf close")
        root = OSError(errno.EIO, "controlled root close")
        self.assert_failed(self.exercise(primary=primary, primary_at="manifest",
                                        close_errors={101: leaf, 100: root}),
                           primary, cleanup=(leaf, root))

    def test_manifest_system_exit_survives_both_close_cancellations(self):
        primary = SystemExit(47)
        leaf = SystemExit(48)
        root = KeyboardInterrupt("controlled root close")
        self.assert_failed(self.exercise(primary=primary, primary_at="manifest",
                                        close_errors={101: leaf, 100: root}),
                           primary, cleanup=(leaf, root))

    def test_leaf_cleanup_cancellation_supersedes_ordinary_refusal_with_cause(self):
        leaf = KeyboardInterrupt("controlled leaf close")
        root = OSError(errno.EIO, "controlled root close")
        observed = self.exercise(mismatch=True, close_errors={101: leaf, 100: root})
        self.assertEqual(len(observed.refusals), 1)
        self.assert_failed(observed, leaf, cleanup=(root,))
        self.assertIs(leaf.__cause__, observed.refusals[0])
        self.assertIn(repr(observed.refusals[0]), "\n".join(getattr(leaf, "__notes__", [])))

    def test_root_cleanup_cancellation_supersedes_ordinary_leaf_error_with_cause(self):
        primary = OSError(errno.EIO, "controlled identity read")
        leaf = OSError(errno.EIO, "controlled leaf close")
        root = SystemExit(48)
        observed = self.exercise(primary=primary, close_errors={101: leaf, 100: root})
        self.assert_failed(observed, root)
        self.assertIs(root.__cause__, primary)
        self.assertIn(repr(primary), "\n".join(getattr(root, "__notes__", [])))
        self.assertIn(repr(leaf), "\n".join(getattr(primary, "__notes__", [])))

    def test_first_normal_close_error_survives_later_root_close_eio(self):
        leaf = OSError(errno.EIO, "controlled leaf close")
        root = OSError(errno.EIO, "controlled root close")
        self.assert_failed(self.exercise(close_errors={101: leaf, 100: root}),
                           leaf, cleanup=(root,))

    def test_first_normal_close_cancellation_survives_later_cancellation(self):
        leaf = KeyboardInterrupt("controlled leaf close")
        root = SystemExit(48)
        self.assert_failed(self.exercise(close_errors={101: leaf, 100: root}),
                           leaf, cleanup=(root,))

    def test_normal_root_close_error_propagates_without_saving(self):
        primary = OSError(errno.EIO, "controlled root close")
        self.assert_failed(self.exercise(close_errors={100: primary}), primary)


if __name__ == "__main__":
    unittest.main()
