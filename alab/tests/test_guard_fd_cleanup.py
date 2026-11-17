#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Anonymous receipt failure precedence, with controlled and actual inode seams.

Descriptor failures below are injections. A refused truncation may leave partial
bytes; a refused close supplies no physical descriptor-retirement observation.
"""
import ast
import errno
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "alab/command_guard.py"
SPEC = importlib.util.spec_from_file_location("fd_cleanup_guard", SOURCE)
GUARD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GUARD)


def faults(callbacks):
    def touched(label):
        callbacks.append(label)
        raise SystemExit(0)

    class ExitSubclass(SystemExit):
        def __getattribute__(self, name):
            if name in ("__class__", "code"):
                touched("exit." + name)
            return super().__getattribute__(name)

    class Payload:
        def __getattribute__(self, name):
            touched("payload." + name)

    class Wrapped(int):
        def __getattribute__(self, name):
            touched("integer." + name)
        def __index__(self):
            touched("integer.index")
        def __int__(self):
            touched("integer.int")

    class Foreign(BaseException):
        def __getattribute__(self, name):
            if name == "__class__":
                touched("base.class")
            return super().__getattribute__(name)
        def __str__(self):
            touched("base.str")
        def __repr__(self):
            touched("base.repr")

    class Ordinary(Exception):
        def __getattribute__(self, name):
            if name == "__class__":
                touched("ordinary.class")
            return super().__getattribute__(name)
        def __str__(self):
            touched("ordinary.str")
        def __repr__(self):
            touched("ordinary.repr")

    class Interrupt(KeyboardInterrupt):
        def __getattribute__(self, name):
            if name == "__class__":
                touched("interrupt.class")
            return super().__getattribute__(name)

    # The boolean is the declared cancellation policy for an Exception primary.
    return [
        ("ordinary", OSError(errno.EIO, "controlled cleanup refusal"), False),
        ("none", SystemExit(None), False),
        ("zero", SystemExit(0), False),
        ("false", SystemExit(False), False),
        ("wrapped", SystemExit(256), False),
        ("negative_wrapped", SystemExit(-256), False),
        ("nonzero", SystemExit(47), True),
        ("negative", SystemExit(-1), True),
        ("float_zero", SystemExit(0.0), True),
        ("huge", SystemExit(sys.maxsize + 1), True),
        ("keyboard", KeyboardInterrupt(), True),
        ("exit_subclass", ExitSubclass(47), False),
        ("opaque_payload", SystemExit(Payload()), True),
        ("wrapped_int_subclass", SystemExit(Wrapped(256)), False),
        ("nonzero_int_subclass", SystemExit(Wrapped(47)), True),
        ("foreign_base", Foreign(), False),
        ("foreign_ordinary", Ordinary(), False),
        ("interrupt_subclass", Interrupt(), True),
    ]


class AnonymousReceiptCleanupTests(unittest.TestCase):
    def controlled(self, primary, invalidation=None, close=None, *, validation=True,
                   write_failure=False):
        """All descriptor operations here are inert, with independent call records."""
        calls = []
        def truncate(descriptor, length):
            calls.append(("truncate", descriptor, length))
            if invalidation is not None:
                raise invalidation
        def closing(descriptor):
            calls.append(("close", descriptor))
            if close is not None:
                raise close
        def run(argv, **options):
            calls.append(("run", argv, options))
            if not write_failure:
                raise primary
            return 23
        def write(descriptor, data):
            calls.append(("write", descriptor))
            raise primary
        def state(_descriptor):
            return SimpleNamespace(st_mode=stat.S_IFREG, st_size=0,
                                   st_nlink=0 if validation else 1)
        fake_os = SimpleNamespace(dup=lambda _fd: 701, fstat=state,
                                  O_ACCMODE=os.O_ACCMODE, O_WRONLY=os.O_WRONLY,
                                  O_RDWR=os.O_RDWR, SEEK_CUR=os.SEEK_CUR, name=os.name,
                                  lseek=lambda *_args: 0, ftruncate=truncate, close=closing)
        fake_fcntl = SimpleNamespace(F_GETFL=0, fcntl=lambda *_args: os.O_RDWR)
        with (mock.patch.object(GUARD, "os", fake_os),
              mock.patch.object(GUARD, "fcntl", fake_fcntl),
              mock.patch.object(GUARD, "run", run),
              mock.patch.object(GUARD, "write_all", write)):
            try:
                GUARD.run_recorded_fd(["inert-command"], result_fd=700,
                                      disk_floor="inert-floor", inherit_fds=True)
            except BaseException as error:
                selected = error
            else:
                self.fail("controlled primary disappeared")
        return selected, calls

    def test_each_cleanup_phase_keeps_primary_or_first_supported_cancellation(self):
        callbacks = []
        class HostilePrimary(Exception):
            def __getattribute__(self, name):
                if name == "__class__":
                    callbacks.append("primary.class")
                    raise SystemExit(0)
                return super().__getattribute__(name)
        primaries = [("ordinary", lambda: RuntimeError("owned failure"), True),
                     ("resource", lambda: GUARD.DiskSpaceError("owned disk stop"), True),
                     ("hostile_ordinary", HostilePrimary, True),
                     ("keyboard", KeyboardInterrupt, False),
                     ("nonzero", lambda: SystemExit(143), False),
                     ("zero", lambda: SystemExit(0), False),
                     ("base", BaseException, False)]
        for primary_label, new_primary, promotable in primaries:
            for index in range(len(faults(callbacks))):
                for phase in ("invalidation", "close"):
                    label, cleanup, promotes = faults(callbacks)[index]
                    with self.subTest(primary=primary_label, cleanup=label, phase=phase):
                        primary = new_primary()
                        options = {phase: cleanup}
                        selected, calls = self.controlled(primary, **options)
                        expected = cleanup if promotable and promotes else primary
                        self.assertTrue(selected is expected, "wrong primary/cancellation identity")
                        self.assertEqual([row[0] for row in calls], ["run", "truncate", "close"])
                        self.assertEqual(calls[0][2], dict(disk_floor="inert-floor", inherit_fds=True,
                                                        exclude_fds=(700, 701)))
                        self.assertEqual(calls[1:], [("truncate", 701, 0), ("close", 701)])
                        self.assertEqual(selected.__dict__.get("__notes__", []),
                                         ["command completion receipt " + phase + " failed"])
                        if selected is not primary:
                            self.assertTrue(selected.__cause__ is primary)
        self.assertEqual(callbacks, [])

    def test_two_cleanup_failures_attempt_both_and_choose_in_phase_order(self):
        for first_kind, second_kind, expected_phase in (
                ("ordinary", "ordinary", None), ("ordinary", "zero", None),
                ("zero", "ordinary", None), ("zero", "nonzero", "close"),
                ("ordinary", "keyboard", "close"), ("nonzero", "keyboard", "invalidation"),
                ("keyboard", "nonzero", "invalidation"), ("keyboard", "keyboard", "invalidation")):
            with self.subTest(first=first_kind, second=second_kind):
                factory = {"ordinary": lambda: OSError(errno.EIO, "controlled refusal"),
                           "zero": lambda: SystemExit(0), "nonzero": lambda: SystemExit(143),
                           "keyboard": KeyboardInterrupt}
                primary, first, second = RuntimeError("owned failure"), factory[first_kind](), factory[second_kind]()
                selected, calls = self.controlled(primary, first, second)
                expected = {None: primary, "invalidation": first, "close": second}[expected_phase]
                self.assertTrue(selected is expected)
                self.assertEqual([row[0] for row in calls], ["run", "truncate", "close"])
                self.assertEqual(selected.__dict__.get("__notes__", []), ["command completion receipt invalidation failed",
                                                       "command completion receipt close failed"])
                if expected_phase is not None:
                    self.assertTrue(selected.__cause__ is primary)

    def test_optional_note_hooks_cannot_replace_selected_failure(self):
        callbacks = []
        for note_error in (OSError(errno.EIO, "note refusal"), SystemExit(0), SystemExit(47),
                           KeyboardInterrupt(), BaseException()):
            class NotesFail(RuntimeError):
                def add_note(self, _note):
                    callbacks.append("note")
                    raise note_error
            primary = NotesFail("owned failure")
            selected, calls = self.controlled(primary, OSError(errno.EIO, "truncate"),
                                               OSError(errno.EIO, "close"))
            self.assertTrue(selected is primary)
            self.assertEqual([row[0] for row in calls], ["run", "truncate", "close"])
        self.assertEqual(callbacks, ["note"] * 10)

    def test_refused_validation_never_truncates_and_attempts_close_once(self):
        for cleanup in (OSError(errno.EIO, "close refusal"), SystemExit(0), SystemExit(47)):
            selected, calls = self.controlled(None, close=cleanup, validation=False)
            self.assertEqual([row[0] for row in calls], ["close"])
            if type(cleanup) is SystemExit and cleanup.code == 47:
                self.assertTrue(selected is cleanup)
                self.assertIs(type(selected.__cause__), ValueError)
            else:
                self.assertIs(type(selected), ValueError)
            self.assertIn("command completion receipt close failed", selected.__dict__.get("__notes__", []))

    def test_partial_write_failure_uses_the_same_cleanup_selection(self):
        for cleanup in (OSError(errno.EIO, "refusal"), SystemExit(0), KeyboardInterrupt()):
            primary = OSError(errno.ENOSPC, "controlled partial write failure")
            selected, calls = self.controlled(primary, invalidation=cleanup, write_failure=True)
            self.assertTrue(selected is (cleanup if type(cleanup) is KeyboardInterrupt else primary))
            self.assertEqual([row[0] for row in calls], ["run", "write", "truncate", "close"])


CHILD = r'''
import ast, errno, hashlib, json, os, runpy, sys, tempfile
from pathlib import Path
from types import SimpleNamespace
source, root, primary_kind, phase, cleanup_kind, mode = Path(sys.argv[1]), Path(sys.argv[2]), *sys.argv[3:]
value = runpy.run_path(str(source))["main"].__globals__
original_os = value["os"]
writer, name = tempfile.mkstemp(dir=root)
reader = os.open(name, os.O_RDONLY)
os.unlink(name)
owned = []
calls = []
witness = {"forbidden": [], "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}
primary = {"resource": lambda: value["DiskSpaceError"]("controlled resource stop"),
           "ordinary": lambda: OSError(errno.EIO, "controlled command failure"),
           "partial": lambda: OSError(errno.ENOSPC, "controlled partial receipt write"),
           "keyboard": KeyboardInterrupt,
           "nonzero": lambda: SystemExit(143)}[primary_kind]()
cleanup = {"ordinary": lambda: OSError(errno.EIO, "controlled descriptor refusal"),
           "zero": lambda: SystemExit(0), "nonzero": lambda: SystemExit(47),
           "keyboard": KeyboardInterrupt}[cleanup_kind]()
def duplicate(fd):
    result = original_os.dup(fd)
    owned.append(result)
    return result
def truncate(fd, length):
    calls.append("invalidation")
    if phase == "invalidation": raise cleanup
    return original_os.ftruncate(fd, length)
def close(fd):
    calls.append("close")
    if phase == "close": raise cleanup
    return original_os.close(fd)
def run(_argv, **options):
    witness["run"] = {"excluded": list(options["exclude_fds"]),
                      "inherited": options["inherit_fds"]}
    if primary_kind != "partial": raise primary
    return 0
def write(fd, _data):
    original_os.write(fd, b"owned partial receipt")
    raise primary
def forbidden(*_args, **_kwargs):
    witness["forbidden"].append("actor")
    raise AssertionError("inert anonymous receipt fixture started an actor")
value["os"] = SimpleNamespace(**{**vars(original_os), "dup": duplicate,
                                 "ftruncate": truncate, "close": close})
value["run"] = run
value["write_all"] = write
value["spawn_custodian"] = forbidden
value["subprocess"] = SimpleNamespace(run=forbidden, Popen=forbidden)
value["signal"] = SimpleNamespace(signal=forbidden, pthread_sigmask=forbidden)
sys.argv = [str(source), "--resource-stop", "--inherit-fds", "--result-fd", str(writer), "--", "inert-command"]
tree = ast.parse(source.read_bytes(), filename=str(source))
entry = ast.Module(body=[node for node in tree.body if type(node) is ast.If
                        and type(node.test) is ast.Compare
                        and type(node.test.left) is ast.Name
                        and node.test.left.id == "__name__"], type_ignores=[])
assert len(entry.body) == 1
witness["entry_sha256"] = hashlib.sha256(ast.dump(entry).encode()).hexdigest()
value["__name__"] = "__main__"
try:
    if mode == "api": value["run_recorded_fd"](["inert-command"], result_fd=writer, inherit_fds=True)
    else: exec(compile(entry, str(source), "exec"), value)
except BaseException as error:
    witness["primary_identity"] = error is primary
    witness["cleanup_identity"] = error is cleanup
    witness["type"] = type(error).__name__
    witness["code"] = error.code if type(error) is SystemExit else None
    witness["cause_is_primary"] = error.__cause__ is primary
    raise
finally:
    witness["cleanup_attempts"] = calls
    witness["bytes"] = original_os.pread(reader, 4096, 0).decode("ascii")
    witness["original_fd_unchanged"] = original_os.fstat(writer).st_nlink == 0
    (root / "witness.json").write_text(json.dumps(witness, sort_keys=True))
    # Independent fixture compensation; close refusal proves no native retirement.
    for fd in [*owned, reader, writer]:
        try: original_os.close(fd)
        except OSError as error:
            if error.errno != errno.EBADF: raise
'''


class AnonymousReceiptOriginalEntryTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-fd-cleanup-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.sequence = 0

    def child(self, primary, phase, cleanup, mode="entry"):
        root = self.root / str(self.sequence)
        self.sequence += 1
        root.mkdir()
        child = root / "child.py"
        child.write_text(CHILD)
        done = subprocess.run([sys.executable, "-I", "-B", str(child), str(SOURCE), str(root),
                               primary, phase, cleanup, mode], capture_output=True, text=True, timeout=10)
        witness = json.loads((root / "witness.json").read_text())
        self.assertEqual(witness["forbidden"], [])
        self.assertEqual(witness["source_sha256"], hashlib.sha256(SOURCE.read_bytes()).hexdigest())
        tree = ast.parse(SOURCE.read_bytes())
        entry = ast.Module(body=[node for node in tree.body if type(node) is ast.If
                                and type(node.test) is ast.Compare
                                and type(node.test.left) is ast.Name
                                and node.test.left.id == "__name__"], type_ignores=[])
        self.assertEqual(witness["entry_sha256"], hashlib.sha256(ast.dump(entry).encode()).hexdigest())
        self.assertEqual(witness["cleanup_attempts"], ["invalidation", "close"])
        self.assertTrue(witness["original_fd_unchanged"])
        self.assertTrue(witness["run"]["inherited"])
        self.assertEqual(len(witness["run"]["excluded"]), 2)
        self.assertNotEqual(*witness["run"]["excluded"])
        return done, witness

    def test_original_entry_preserves_resource_and_existing_cancellation_statuses(self):
        for primary, status in (("resource", 75), ("ordinary", 1), ("keyboard", -2), ("nonzero", 143)):
            for phase in ("invalidation", "close"):
                for cleanup in ("ordinary", "zero"):
                    with self.subTest(primary=primary, phase=phase, cleanup=cleanup):
                        done, witness = self.child(primary, phase, cleanup)
                        self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                        self.assertEqual(done.stdout, "")
                        self.assertEqual(witness["bytes"], "")
                        if primary == "resource":
                            self.assertIn("disk resource stop", done.stderr)
                        if primary in ("keyboard", "nonzero"):
                            self.assertTrue(witness["primary_identity"])

    def test_first_supported_cleanup_cancellation_can_replace_resource_stop(self):
        for phase in ("invalidation", "close"):
            for cleanup, status in (("keyboard", -2), ("nonzero", 47)):
                with self.subTest(phase=phase, cleanup=cleanup):
                    done, witness = self.child("resource", phase, cleanup)
                    self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                    self.assertEqual(done.stdout, "")
                    self.assertTrue(witness["cleanup_identity"])
                    self.assertTrue(witness["cause_is_primary"])

    def test_real_partial_bytes_remain_when_native_invalidation_was_not_performed(self):
        for mode in ("entry", "api"):
            for phase in ("invalidation", "close"):
                with self.subTest(mode=mode, phase=phase):
                    done, witness = self.child("partial", phase, "ordinary", mode)
                    self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                    self.assertEqual(witness["bytes"], "owned partial receipt" if phase == "invalidation" else "")
                    self.assertEqual(done.stdout, "")
                    if mode == "api":
                        self.assertTrue(witness["primary_identity"])


if __name__ == "__main__":
    unittest.main()
