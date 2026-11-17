#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Named-receipt cleanup policy, through the public API and owned resources.

The descriptor ledger is synthetic: a close fault deliberately consumes its
virtual descriptor and proves attempts/order, not physical retirement. The last
method separately observes actual owned file bytes and descriptor retirement.
No command, custodian, signal or fork is permitted by these controlled cases.
"""

import errno
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import test_guard_fd_cleanup as anonymous_tests


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("named_cleanup_guard", ROOT / "alab/command_guard.py")
GUARD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GUARD)
COMMAND = ["inert-owned-command"]
CLEANUP = ["truncate", "stat", "unlink", "close-receipt", "close-parent"]


class DescriptorLedger:
    """Only module-local virtual descriptors; no real descriptor is operated on."""

    parent = 701
    receipt = 702

    def __init__(self, primary=None, failures=None, *, path_kind="claimed", code=23):
        self.primary, self.failures = primary, dict(failures or {})
        self.path_kind, self.code = path_kind, code
        self.calls, self.actors, self.live, self.details = [], [], set(), {}
        self.content, self.linked = b"", False
        self.namespace = SimpleNamespace(**{
            **vars(os), "open": self.open, "fstat": self.fstat,
            "ftruncate": self.truncate, "stat": self.stat, "unlink": self.unlink,
            "close": self.close, "write": self.write, "fsync": self.fsync,
            "fork": self.actor, "kill": self.actor, "killpg": self.actor,
            "waitpid": self.actor, "waitid": self.actor,
        })

    def touch(self, phase):
        self.calls.append(phase)
        if phase in self.failures:
            raise self.failures[phase]

    def actor(self, *_args, **_options):
        self.actors.append("forbidden actor")
        raise AssertionError("the named receipt fixture started an actor")

    def open(self, path, flags, mode=0o777, *, dir_fd=None):
        phase, fd = ("open-parent", self.parent) if dir_fd is None else ("open-receipt", self.receipt)
        self.touch(phase)
        self.details[phase] = (path, flags, mode, dir_fd)
        if dir_fd is not None:
            assert dir_fd == self.parent and self.parent in self.live
            self.linked = True
        self.live.add(fd)
        return fd

    def fstat(self, descriptor):
        assert descriptor == self.receipt and descriptor in self.live
        self.touch("fstat")
        return SimpleNamespace(st_dev=17, st_ino=71, st_mode=stat.S_IFREG | 0o600)

    def truncate(self, descriptor, length):
        assert descriptor == self.receipt and descriptor in self.live and length == 0
        self.touch("truncate")
        self.content = b""

    def stat(self, path, *, dir_fd=None, follow_symlinks=True):
        self.details.setdefault("stat-options", []).append((str(path), dir_fd, follow_symlinks))
        self.touch("stat")
        assert follow_symlinks is False
        if self.path_kind == "absent" or not self.linked:
            raise FileNotFoundError(errno.ENOENT, "owned receipt is absent")
        return SimpleNamespace(st_dev=17, st_ino=99 if self.path_kind == "replaced" else 71)

    def unlink(self, path, *, dir_fd=None):
        self.touch("unlink")
        assert self.path_kind == "claimed" and self.linked
        self.linked = False

    def close(self, descriptor):
        assert descriptor in self.live, "a consumed descriptor was retried"
        self.live.remove(descriptor)
        self.touch("close-receipt" if descriptor == self.receipt else "close-parent")

    def write(self, descriptor, data):
        assert descriptor == self.receipt and descriptor in self.live
        self.touch("write")
        self.content += data
        return len(data)

    def fsync(self, descriptor):
        assert descriptor in self.live
        self.touch("fsync-receipt" if descriptor == self.receipt else "fsync-parent")

    def run(self, argv, **options):
        self.calls.append("run")
        self.details["run"] = (argv, options)
        if self.primary is not None:
            raise self.primary
        return self.code


class NamedReceiptCleanupTests(unittest.TestCase):
    @staticmethod
    def context(ledger):
        return f"attempts={ledger.calls!r}; live={sorted(ledger.live)!r}"

    def invoke(self, ledger):
        with (mock.patch.object(GUARD, "os", ledger.namespace),
              mock.patch.object(GUARD, "run", ledger.run),
              mock.patch.object(GUARD, "subprocess", SimpleNamespace(
                  Popen=ledger.actor, run=ledger.actor))):
            try:
                result = GUARD.run_recorded(COMMAND, result_file="/owned/named/result.json",
                                            disk_floor="inert-floor", inherit_fds=True)
            except BaseException as error:
                return error, None
        return None, result

    def assert_cleanup(self, ledger, expected=CLEANUP):
        tail = ledger.calls[ledger.calls.index("run") + 1:]
        self.assertEqual([phase for phase in tail if not phase.startswith("note")], expected)
        self.assertEqual(ledger.live, set(), "an owned virtual descriptor was abandoned")
        self.assertEqual(ledger.actors, [])

    def assert_notes(self, error, count):
        notes = error.__dict__.get("__notes__", [])
        self.assertEqual(len(notes), count)
        self.assertTrue(all(type(note) is str and note for note in notes))

    def test_pending_base_exceptions_survive_every_cleanup_phase(self):
        for primary_kind, factory in (
                ("keyboard", KeyboardInterrupt), ("nonzero", lambda: SystemExit(47)),
                ("zero", lambda: SystemExit(0)), ("base", BaseException)):
            for phase in ("truncate", "stat", "unlink", "close-receipt", "close-parent"):
                for cleanup_kind, cleanup_factory in (
                        ("ordinary", lambda: OSError(errno.EIO, "owned cleanup refusal")),
                        ("keyboard", KeyboardInterrupt), ("nonzero", lambda: SystemExit(143)),
                        ("zero", lambda: SystemExit(0))):
                    with self.subTest(primary=primary_kind, phase=phase, cleanup=cleanup_kind):
                        primary = factory()
                        ledger = DescriptorLedger(primary, {phase: cleanup_factory()})
                        selected, _ = self.invoke(ledger)
                        self.assertTrue(selected is primary, self.context(ledger))
                        expected = [p for p in CLEANUP if not (phase == "stat" and p == "unlink")]
                        self.assert_cleanup(ledger, expected)
                        self.assert_notes(selected, 1)

    def test_ordinary_primary_uses_the_existing_supported_cancellation_policy(self):
        callbacks = []
        for index in range(len(anonymous_tests.faults(callbacks))):
            for phase in ("truncate", "stat", "unlink", "close-receipt", "close-parent"):
                label, cleanup, promotes = anonymous_tests.faults(callbacks)[index]
                with self.subTest(cleanup=label, phase=phase):
                    primary = RuntimeError("owned ordinary failure")
                    ledger = DescriptorLedger(primary, {phase: cleanup})
                    selected, _ = self.invoke(ledger)
                    self.assertTrue(selected is (cleanup if promotes else primary), self.context(ledger))
                    if promotes:
                        self.assertTrue(selected.__cause__ is primary)
                    expected = [p for p in CLEANUP if not (phase == "stat" and p == "unlink")]
                    self.assert_cleanup(ledger, expected)
                    self.assert_notes(selected, 1)
        self.assertEqual(callbacks, [], "foreign exception metadata or formatting was evaluated")

    def test_multiple_faults_attempt_every_owned_action_before_selection(self):
        for labels, selected_phase in (
                (("ordinary", "ordinary", "ordinary"), None),
                (("zero", "keyboard", "nonzero"), "unlink"),
                (("nonzero", "keyboard", "ordinary"), "truncate"),
                (("ordinary", "zero", "keyboard"), "close-receipt")):
            with self.subTest(faults=labels):
                factories = {"ordinary": lambda: OSError(errno.EIO, "owned refusal"),
                             "zero": lambda: SystemExit(0), "nonzero": lambda: SystemExit(47),
                             "keyboard": KeyboardInterrupt}
                errors = {phase: factories[label]() for phase, label in zip(
                    ("truncate", "unlink", "close-receipt"), labels)}
                primary = RuntimeError("owned primary")
                ledger = DescriptorLedger(primary, errors)
                selected, _ = self.invoke(ledger)
                self.assertTrue(selected is (primary if selected_phase is None else errors[selected_phase]),
                                self.context(ledger))
                self.assert_cleanup(ledger)
                self.assert_notes(selected, 3)

    def test_optional_note_lookup_call_and_formatting_cannot_skip_cleanup(self):
        callbacks = []
        for mode in ("lookup", "call", "truthy"):
            for fault_factory in (lambda: OSError(errno.EIO, "advisory refusal"),
                                  lambda: SystemExit(0), lambda: SystemExit(47), KeyboardInterrupt):
                with self.subTest(mode=mode, fault=fault_factory.__name__):
                    ledger = DescriptorLedger()
                    fault = fault_factory()
                    class Primary(RuntimeError):
                        def __getattribute__(self, name):
                            if name == "__class__":
                                callbacks.append("primary.class")
                                raise SystemExit(0)
                            if name == "add_note":
                                ledger.calls.append("note-lookup")
                                if mode == "lookup":
                                    raise fault
                            return super().__getattribute__(name)
                        def add_note(self, note):
                            ledger.calls.append("note-call")
                            if mode == "call":
                                raise fault
                            BaseException.add_note(self, note)
                            return True
                    primary = Primary("owned primary")
                    # These existing hostile ordinary errors forbid str/repr/class hooks.
                    foreign = {name: error for name, error, _promotes in anonymous_tests.faults(callbacks)}
                    ledger.primary = primary
                    ledger.failures = {"truncate": foreign["foreign_ordinary"],
                                       "close-receipt": OSError(errno.EIO, "close refusal")}
                    selected, _ = self.invoke(ledger)
                    self.assertTrue(selected is primary, self.context(ledger))
                    self.assert_cleanup(ledger)
                    notes = [i for i, phase in enumerate(ledger.calls) if phase.startswith("note")]
                    self.assertTrue(notes or primary.__dict__.get("__notes__"),
                                    "secondary diagnostics were not retained or attempted")
                    if notes:
                        self.assertGreater(min(notes), ledger.calls.index("close-parent"),
                                           "diagnostics ran before owned cleanup completed")
        self.assertEqual(callbacks, [])

    def test_parent_and_partial_receipt_acquisition_have_bounded_ownership(self):
        for phase, expected in (
                ("open-parent", ["open-parent"]),
                ("open-receipt", ["open-parent", "open-receipt", "close-parent"]),
                ("fstat", ["open-parent", "open-receipt", "fstat", "truncate",
                           "close-receipt", "close-parent"])):
            with self.subTest(phase=phase):
                primary = KeyboardInterrupt()
                ledger = DescriptorLedger(failures={phase: primary})
                selected, _ = self.invoke(ledger)
                self.assertTrue(selected is primary, self.context(ledger))
                self.assertEqual(ledger.calls, expected)
                self.assertEqual(ledger.live, set())
                self.assertEqual(ledger.actors, [])
                self.assertNotIn("run", ledger.calls)

        for phase in ("open-receipt", "fstat"):
            with self.subTest(partial_phase=phase, secondary="close cancellation"):
                primary = SystemExit(47)
                ledger = DescriptorLedger(failures={phase: primary,
                                                    "close-parent": SystemExit(0)})
                selected, _ = self.invoke(ledger)
                self.assertTrue(selected is primary, self.context(ledger))
                self.assertEqual(ledger.live, set())
                self.assertEqual(ledger.calls.count("close-parent"), 1)
                self.assert_notes(selected, 1)

    def test_missing_or_replaced_path_is_not_unlinked_during_failure(self):
        for path_kind in ("absent", "replaced"):
            with self.subTest(path=path_kind):
                primary = SystemExit(47)
                ledger = DescriptorLedger(primary, path_kind=path_kind)
                selected, _ = self.invoke(ledger)
                self.assertTrue(selected is primary, self.context(ledger))
                self.assert_cleanup(ledger, ["truncate", "stat", "close-receipt", "close-parent"])
                self.assertNotIn("unlink", ledger.calls)
                self.assertEqual(ledger.path_kind, path_kind)
                self.assert_notes(selected, 0)

    def test_publication_close_consumption_never_retries_either_descriptor(self):
        for phase in ("close-receipt", "close-parent"):
            with self.subTest(phase=phase):
                primary = OSError(errno.EIO, "consumed publication close refused")
                ledger = DescriptorLedger(failures={phase: primary})
                selected, _ = self.invoke(ledger)
                self.assertTrue(selected is primary, self.context(ledger))
                self.assertEqual(ledger.live, set())
                self.assertEqual(ledger.calls.count("close-receipt"), 1)
                self.assertEqual(ledger.calls.count("close-parent"), 1)
                self.assertNotIn("truncate", ledger.calls)
                self.assertFalse(ledger.linked)
                self.assertEqual(ledger.calls.count("unlink"), 1)
                if phase == "close-parent":
                    self.assertEqual(ledger.details["stat-options"][-1],
                                     ("/owned/named/result.json", None, False))

    def test_success_retains_exact_publication_flags_bytes_and_status(self):
        ledger = DescriptorLedger(code=23)
        selected, result = self.invoke(ledger)
        self.assertIsNone(selected)
        self.assertEqual(result, 23)
        self.assertEqual(ledger.calls, ["open-parent", "open-receipt", "fstat", "run", "stat",
                                        "write", "fsync-receipt", "fsync-parent",
                                        "close-receipt", "close-parent"])
        self.assertEqual(ledger.live, set())
        self.assertTrue(ledger.linked)
        self.assertEqual(json.loads(ledger.content), {
            "schema": 1, "kind": "guardian-command-completion", "completed": True,
            "argv": COMMAND, "normalized_exit_status": 23,
            "cleanup_scope": "owned command process group"})
        self.assertEqual(ledger.details["open-parent"][1], os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
        self.assertEqual(ledger.details["open-receipt"][1:],
                         (os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW,
                          0o600, ledger.parent))
        self.assertEqual(ledger.details["run"],
                         (COMMAND, {"disk_floor": "inert-floor", "inherit_fds": True}))
        self.assertEqual(ledger.actors, [])

    def test_partial_write_is_invalidated_before_all_cleanup_attempts(self):
        primary = OSError(errno.ENOSPC, "owned partial write")
        ledger = DescriptorLedger()
        def partial(descriptor, data):
            ledger.write(descriptor, data[:7])
            raise primary
        with mock.patch.object(GUARD, "write_all", partial):
            selected, _ = self.invoke(ledger)
        self.assertTrue(selected is primary, self.context(ledger))
        self.assertEqual(ledger.content, b"")
        self.assertFalse(ledger.linked)
        self.assertEqual(ledger.calls[-5:], CLEANUP)
        self.assertEqual(ledger.live, set())

    def test_original_main_function_keeps_failure_and_resource_statuses(self):
        # This calls the maintained entry function, not a new process/OS exit.
        cases = []
        for cleanup_code in (0, None, 256):
            cases.append(("ordinary", RuntimeError("owned ordinary failure"),
                          SystemExit(cleanup_code), False, 1))
            cases.append(("resource", GUARD.DiskSpaceError("owned disk stop"),
                          SystemExit(cleanup_code), True, 75))
        for primary_kind, factory in (("nonzero", lambda: SystemExit(47)),
                                       ("keyboard", KeyboardInterrupt)):
            for cleanup_factory in (lambda: SystemExit(0), KeyboardInterrupt):
                cases.append((primary_kind, factory(), cleanup_factory(), False, None))
        for label, primary, cleanup, resource_stop, expected in cases:
            with self.subTest(primary=label, cleanup_type=type(cleanup).__name__,
                              resource_stop=resource_stop):
                ledger = DescriptorLedger(primary, {"truncate": cleanup})
                argv = ["command_guard.py", "--result-file", "/owned/named/result.json"]
                if resource_stop:
                    argv.append("--resource-stop")
                argv.extend(["--", *COMMAND])
                output = io.StringIO()
                namespace = SimpleNamespace(**{**vars(sys), "argv": argv, "stderr": output})
                selected, status = None, None
                with (mock.patch.object(GUARD, "os", ledger.namespace),
                      mock.patch.object(GUARD, "run", ledger.run),
                      mock.patch.object(GUARD, "sys", namespace),
                      mock.patch.object(GUARD, "subprocess", SimpleNamespace(
                          Popen=ledger.actor, run=ledger.actor))):
                    try:
                        status = GUARD.main()
                    except BaseException as error:
                        selected = error
                self.assert_cleanup(ledger)
                if expected is None:
                    self.assertTrue(selected is primary, self.context(ledger))
                    self.assertIsNone(status)
                else:
                    self.assertIsNone(selected)
                    self.assertEqual(status, expected)
                    self.assertIn("owned", output.getvalue())

    def test_actual_owned_files_and_descriptors_keep_primary_without_starting_actors(self):
        primaries = (("keyboard", KeyboardInterrupt), ("nonzero", lambda: SystemExit(47)),
                     ("zero", lambda: SystemExit(0)), ("ordinary", lambda: RuntimeError("owned failure")))
        secondaries = (("ordinary", lambda: OSError(errno.EIO, "actual truncate completed then refused")),
                       ("keyboard", KeyboardInterrupt), ("zero", lambda: SystemExit(0)))
        for primary_kind, primary_factory in primaries:
            for secondary_kind, secondary_factory in secondaries:
                with self.subTest(primary=primary_kind, secondary=secondary_kind):
                    with tempfile.TemporaryDirectory(prefix="alab-named-cleanup-") as scratch:
                        root = Path(scratch).resolve()
                        result_path = root / "result.json"
                        outside = root / "outside"
                        outside.write_bytes(b"unrelated owned fixture\n")
                        original_os = os
                        owned, opened, calls, actors = set(), [], [], []
                        primary = primary_factory()
                        secondary = secondary_factory()
                        def opening(*args, **options):
                            descriptor = original_os.open(*args, **options)
                            owned.add(descriptor)
                            opened.append(descriptor)
                            return descriptor
                        def truncating(descriptor, length):
                            self.assertIn(descriptor, owned)
                            calls.append("truncate")
                            original_os.ftruncate(descriptor, length)
                            raise secondary
                        def closing(descriptor):
                            self.assertIn(descriptor, owned)
                            owned.remove(descriptor)
                            calls.append("close-receipt" if descriptor == opened[1] else "close-parent")
                            return original_os.close(descriptor)
                        def stating(*args, **options):
                            calls.append("stat")
                            return original_os.stat(*args, **options)
                        def unlinking(*args, **options):
                            calls.append("unlink")
                            return original_os.unlink(*args, **options)
                        def inert(*_args, **_options):
                            raise primary
                        def forbidden(*_args, **_options):
                            actors.append("forbidden actor")
                            raise AssertionError("actual inode control started an actor")
                        namespace = SimpleNamespace(**{**vars(original_os), "open": opening,
                            "close": closing, "ftruncate": truncating, "stat": stating,
                            "unlink": unlinking, "fork": forbidden,
                            "kill": forbidden, "killpg": forbidden, "waitpid": forbidden,
                            "waitid": forbidden})
                        try:
                            with (mock.patch.object(GUARD, "os", namespace),
                                  mock.patch.object(GUARD, "run", inert),
                                  mock.patch.object(GUARD, "subprocess", SimpleNamespace(
                                      Popen=forbidden, run=forbidden))):
                                try:
                                    GUARD.run_recorded(COMMAND, result_file=result_path)
                                except BaseException as error:
                                    selected = error
                                else:
                                    self.fail("the controlled primary disappeared")
                            diagnostic = (f"attempts={calls!r}; live={sorted(owned)!r}; "
                                          f"result_exists={result_path.exists()}")
                            promotes = primary_kind == "ordinary" and secondary_kind == "keyboard"
                            self.assertTrue(selected is (secondary if promotes else primary), diagnostic)
                            if promotes:
                                self.assertTrue(selected.__cause__ is primary, diagnostic)
                            self.assertFalse(result_path.exists(), diagnostic)
                            self.assertEqual(outside.read_bytes(), b"unrelated owned fixture\n")
                            self.assertEqual(owned, set(), diagnostic)
                            self.assertEqual(calls, CLEANUP, diagnostic)
                            self.assertEqual(actors, [])
                            self.assert_notes(selected, 1)
                            for descriptor in opened:
                                with self.assertRaises(OSError) as closed:
                                    original_os.fstat(descriptor)
                                self.assertEqual(closed.exception.errno, errno.EBADF)
                        finally:
                            # Compensate only still-owned endpoints; clear ownership before close.
                            remaining, owned = list(owned), set()
                            failures = []
                            for descriptor in remaining:
                                try:
                                    original_os.close(descriptor)
                                except OSError as error:
                                    failures.append(error)
                            self.assertEqual(failures, [])


if __name__ == "__main__":
    unittest.main()
