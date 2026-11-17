#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Raw command evidence distinguishes signals and timeouts from ordinary failure."""

import errno
import importlib.util
import json
import os
from pathlib import Path
import signal
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("result_guard", ROOT / "alab/command_guard.py")
GUARD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GUARD)


class GuardResultTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="alab-guard-result-")).resolve()
        print(f"retained raw-result fixture: {self.root}", flush=True)

    def run_recorded(self, source, timeout=10):
        command = [sys.executable, "-I", "-B", "-c", source]
        record = self.root / "result.json"
        code = GUARD.run_inherited(command, timeout=timeout, result_file=record)
        result = json.loads(record.read_text())
        self.assertEqual(result["schema"], 1)
        self.assertEqual(result["argv"], command)
        self.assertEqual(result["timeout_seconds"], timeout)
        return code, result

    def anonymous_result(self, initial=b"", offset=0, read_only=False):
        writer, name = tempfile.mkstemp(dir=self.root, prefix="anonymous-")
        path = Path(name)
        if initial:
            os.write(writer, initial)
        os.lseek(writer, offset, os.SEEK_SET)
        reader = os.open(path, os.O_RDONLY)
        if read_only:
            os.close(writer)
            writer = os.open(path, os.O_RDONLY)
            os.lseek(writer, offset, os.SEEK_SET)
        path.unlink()
        self.addCleanup(self.close_fd, reader)
        self.addCleanup(self.close_fd, writer)
        return reader, writer

    @staticmethod
    def close_fd(descriptor):
        try:
            os.close(descriptor)
        except OSError as error:
            if error.errno != errno.EBADF:
                raise

    @staticmethod
    def fd_bytes(descriptor):
        os.lseek(descriptor, 0, os.SEEK_SET)
        chunks = []
        while True:
            chunk = os.read(descriptor, 65536)
            if not chunk:
                return b"".join(chunks)
            chunks.append(chunk)

    @staticmethod
    def completion_bytes(command, status):
        return json.dumps({"schema": 1, "kind": "guardian-command-completion",
                           "completed": True, "argv": command,
                           "normalized_exit_status": status,
                           "cleanup_scope": "owned command process group"},
                          ensure_ascii=True).encode() + b"\n"

    def result_fd_cli(self, writer, command, *options, pass_fds=()):
        return subprocess.run(
            [sys.executable, "-I", "-B", str(ROOT / "alab/command_guard.py"),
             "--inherit-fds", "--result-fd", str(writer), *options, "--", *command],
            pass_fds=(writer, *pass_fds), capture_output=True, timeout=60)

    def test_normal_nonzero_and_timeout_shaped_exit_are_not_signals_or_timeouts(self):
        for code in (0, 23, 124, 143):
            with self.subTest(code=code):
                record = self.root / f"result-{code}.json"
                command = [sys.executable, "-I", "-B", "-c", f"raise SystemExit({code})"]
                self.assertEqual(GUARD.run_inherited(command, timeout=10, result_file=record), code)
                result = json.loads(record.read_text())
                self.assertEqual(result["returncode"], code)
                self.assertIs(result["timed_out"], False)

    def test_actual_signal_keeps_raw_negative_status_beside_mapped_guardian_exit(self):
        code, result = self.run_recorded("import os, signal; os.kill(os.getpid(), signal.SIGTERM)")
        self.assertEqual(code, 128 + signal.SIGTERM)
        self.assertEqual(result["returncode"], -signal.SIGTERM)
        self.assertIs(result["timed_out"], False)

    def test_actual_deadline_has_no_ordinary_raw_exit(self):
        code, result = self.run_recorded("import time; time.sleep(10)", timeout=0.05)
        self.assertEqual(code, 124)
        self.assertIsNone(result["returncode"])
        self.assertIs(result["timed_out"], True)

    def test_existing_result_is_preserved_and_refused_before_command_starts(self):
        record = self.root / "result.json"
        record.write_bytes(b"existing evidence\n")
        ran = self.root / "command-ran"
        with self.assertRaises(FileExistsError):
            GUARD.run_inherited([sys.executable, "-I", "-B", "-c",
                                 f"from pathlib import Path; Path({str(ran)!r}).touch()"],
                                timeout=10, result_file=record)
        self.assertFalse(ran.exists())
        self.assertEqual(record.read_bytes(), b"existing evidence\n")

    def test_normal_guardian_records_only_the_supervised_normalized_status(self):
        sources = [(code, f"raise SystemExit({code})") for code in (0, 23, 124, 143)]
        sources.append((143, "import os,signal; os.kill(os.getpid(), signal.SIGTERM)"))
        for number, (expected, source) in enumerate(sources):
            with self.subTest(number=number):
                path = self.root / f"normal-{number}.json"
                command = [sys.executable, "-I", "-B", "-c", source]
                self.assertEqual(GUARD.run_recorded(command, result_file=path), expected)
                record = json.loads(path.read_text())
                self.assertEqual(record, {
                    "schema": 1, "kind": "guardian-command-completion", "completed": True,
                    "argv": command, "normalized_exit_status": expected,
                    "cleanup_scope": "owned command process group"})
                self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)

    def test_normal_cli_records_after_the_actual_controller_returns(self):
        path = self.root / "normal-cli.json"
        done = subprocess.run(
            [sys.executable, "-I", "-B", str(ROOT / "alab/command_guard.py"),
             "--result-file", str(path), "--", sys.executable, "-I", "-B", "-c",
             "raise SystemExit(23)"], capture_output=True, timeout=60)
        self.assertEqual(done.returncode, 23, done.stderr)
        self.assertEqual(json.loads(path.read_text())["normalized_exit_status"], 23)

    def test_anonymous_result_fd_records_exact_status_and_is_not_inherited(self):
        for command, expected in (
                (["/usr/bin/true"], 0),
                (["/bin/sh", "-c", "exit 23"], 23),
                (["/bin/sh", "-c", "kill -TERM $$"], 128 + signal.SIGTERM)):
            with self.subTest(command=command):
                reader, writer = self.anonymous_result()
                done = self.result_fd_cli(writer, command)
                self.assertEqual(done.returncode, expected, done.stderr)
                self.assertEqual(self.fd_bytes(reader), self.completion_bytes(command, expected))

        marker, marker_name = tempfile.mkstemp(dir=self.root, prefix="preserved-")
        Path(marker_name).unlink()
        saved = None
        try:
            try:
                saved = os.dup(43)
            except OSError as error:
                self.assertEqual(error.errno, errno.EBADF)
            os.dup2(marker, 43, inheritable=True)
            os.write(43, b"x")
            os.lseek(43, 0, os.SEEK_SET)
            reader, writer = self.anonymous_result()
            state = os.fstat(writer)
            source = '''
import os,sys
result,device,inode=map(int,sys.argv[1:])
assert not os.path.exists(f"/dev/fd/{result}")
assert os.read(43,1)==b"x"
parent=f"/proc/{os.getppid()}/fd"
if os.path.isdir(parent):
    for name in os.listdir(parent):
        try: value=os.stat(os.path.join(parent,name))
        except OSError: continue
        assert (value.st_dev,value.st_ino)!=(device,inode)
'''
            command = [sys.executable, "-I", "-c", source, str(writer),
                       str(state.st_dev), str(state.st_ino)]
            done = self.result_fd_cli(writer, command, pass_fds=(43,))
            self.assertEqual(done.returncode, 0, done.stderr)
            self.assertEqual(self.fd_bytes(reader), self.completion_bytes(command, 0))
        finally:
            os.close(marker)
            if saved is None:
                self.close_fd(43)
            else:
                os.dup2(saved, 43)
                os.close(saved)

    def test_anonymous_result_fd_refuses_invalid_shapes_without_modifying_them(self):
        linked, linked_name = tempfile.mkstemp(dir=self.root, prefix="linked-")
        os.write(linked, b"linked")
        os.lseek(linked, 0, os.SEEK_SET)
        self.addCleanup(self.close_fd, linked)
        self.addCleanup(Path(linked_name).unlink, missing_ok=True)
        nonempty_reader, nonempty = self.anonymous_result(b"nonempty")
        offset_reader, offset = self.anonymous_result(offset=1)
        readonly_reader, readonly = self.anonymous_result(read_only=True)
        pipe_reader, pipe_writer = os.pipe()
        self.addCleanup(self.close_fd, pipe_reader)
        self.addCleanup(self.close_fd, pipe_writer)
        for descriptor, reader, original in (
                (linked, linked, b"linked"),
                (nonempty, nonempty_reader, b"nonempty"),
                (offset, offset_reader, b""),
                (readonly, readonly_reader, b""),
                (pipe_writer, None, None)):
            with self.subTest(descriptor=descriptor, original=original):
                before_offset = None
                try:
                    before_offset = os.lseek(descriptor, 0, os.SEEK_CUR)
                except OSError:
                    pass
                done = self.result_fd_cli(descriptor, ["/usr/bin/true"])
                self.assertEqual(done.returncode, 1, done.stderr)
                if reader is not None:
                    self.assertEqual(self.fd_bytes(reader), original)
                if before_offset is not None and descriptor != linked:
                    self.assertEqual(os.lseek(descriptor, 0, os.SEEK_CUR), before_offset)

    def test_anonymous_result_options_are_single_and_mutually_exclusive(self):
        reader, writer = self.anonymous_result()
        for options in (
                ["--result-fd", str(writer), "--result-fd", str(writer)],
                ["--result-fd", str(writer), "--result-file", str(self.root / "unused")],
                ["--result-file", str(self.root / "unused"), "--result-fd", str(writer)]):
            with self.subTest(options=options):
                done = subprocess.run(
                    [sys.executable, "-I", "-B", str(ROOT / "alab/command_guard.py"),
                     *options, "--", "/usr/bin/true"], pass_fds=(writer,),
                    capture_output=True, timeout=60)
                self.assertEqual(done.returncode, 2, done.stderr)
                self.assertEqual(self.fd_bytes(reader), b"")

    def test_anonymous_result_failures_leave_no_exact_completion(self):
        for command, options, expected in (
                ([str(self.root / "missing-command")], (), 1),
                (["/usr/bin/true"], ("--resource-stop", "--min-free-gib", "1e20",
                                     "--disk-path", "/tmp"), 75)):
            with self.subTest(command=command):
                reader, writer = self.anonymous_result()
                done = self.result_fd_cli(writer, command, *options)
                self.assertEqual(done.returncode, expected, done.stderr)
                self.assertEqual(self.fd_bytes(reader), b"")

        for failure in ("observer", "partial", "zero"):
            with self.subTest(failure=failure):
                reader, writer = self.anonymous_result()
                if failure == "observer":
                    contexts = (mock.patch.object(GUARD, "run",
                                                   side_effect=RuntimeError("observer")),)
                else:
                    def broken(descriptor, data, operation=failure):
                        if operation == "partial":
                            os.write(descriptor, data[:7])
                            raise OSError(errno.EIO, "partial")
                        raise OSError(errno.EIO, "zero")
                    contexts = (mock.patch.object(GUARD, "run", return_value=0),
                                mock.patch.object(GUARD, "write_all", side_effect=broken))
                with contexts[0]:
                    if len(contexts) == 1:
                        with self.assertRaises(RuntimeError):
                            GUARD.run_recorded_fd(["/usr/bin/true"], result_fd=writer)
                    else:
                        with contexts[1], self.assertRaises(OSError):
                            GUARD.run_recorded_fd(["/usr/bin/true"], result_fd=writer)
                self.assertEqual(self.fd_bytes(reader), b"")

    def test_empty_claim_precedes_launch_and_completion_follows_writer_retirement(self):
        path = self.root / "retired.json"
        ready = self.root / "writer.json"
        writer = f'''
import json,os,signal
from pathlib import Path
signal.signal(signal.SIGTERM, signal.SIG_IGN)
Path({str(ready)!r}).write_text(json.dumps([os.getpid(),os.getpgrp()]))
signal.alarm(60)
signal.pause()
'''
        controller = f'''
import json,signal,subprocess,sys,time
from pathlib import Path
signal.alarm(60)
receipt=Path({str(path)!r})
assert receipt.is_file() and receipt.read_bytes()==b''
subprocess.Popen([sys.executable,'-I','-B','-c',{writer!r}],
                 stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
while not Path({str(ready)!r}).exists(): time.sleep(.01)
'''
        self.assertEqual(GUARD.run_recorded(
            [sys.executable, "-I", "-B", "-c", controller], result_file=path), 0)
        self.assertIs(json.loads(path.read_text())["completed"], True)
        pid, group = json.loads(ready.read_text())
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)
        self.assertFalse(GUARD.group_exists(group))

    def test_existing_receipt_kinds_refuse_before_normal_launch(self):
        outside = self.root / "outside"
        outside.write_bytes(b"other owner")
        for kind in ("file", "symlink", "directory"):
            with self.subTest(kind=kind):
                path = self.root / kind
                if kind == "file":
                    path.write_bytes(b"existing evidence")
                elif kind == "symlink":
                    path.symlink_to(outside)
                else:
                    path.mkdir()
                with (mock.patch.object(GUARD, "run") as start,
                      self.assertRaises(FileExistsError)):
                    GUARD.run_recorded(["must-not-run"], result_file=path)
                start.assert_not_called()
                self.assertEqual(outside.read_bytes(), b"other owner")
                if kind == "file":
                    self.assertEqual(path.read_bytes(), b"existing evidence")
                elif kind == "symlink":
                    self.assertTrue(path.is_symlink())
                else:
                    self.assertTrue(path.is_dir())

    def test_launch_and_observer_failures_cannot_publish_completion(self):
        path = self.root / "launch.json"
        with self.assertRaises(FileNotFoundError):
            GUARD.run_recorded([str(self.root / "no-command")], result_file=path)
        self.assertFalse(path.exists())
        path = self.root / "observer.json"
        children = []
        start = GUARD.spawn_custodian

        def tracked(*arguments, **keywords):
            child = start(*arguments, **keywords)
            children.append(child)
            return child

        with (mock.patch.object(GUARD, "spawn_custodian", side_effect=tracked),
              mock.patch.object(GUARD, "observe_exit", side_effect=RuntimeError("observer refused")),
              self.assertRaisesRegex(RuntimeError, "observer refused")):
            GUARD.run_recorded([sys.executable, "-I", "-B", "-c",
                                "import signal; signal.alarm(60); signal.pause()"], result_file=path)
        self.assertFalse(path.exists())
        self.assertTrue(children)
        for child in children:
            self.assertIsNotNone(child.returncode)
            self.assertTrue(child.events.closed)
            self.assertFalse(GUARD.group_exists(child.pid))

    def test_guardian_cancellation_preserves_exit_and_invalidates_claim(self):
        path = self.root / "cancelled.json"
        children = []
        start = GUARD.spawn_custodian

        def cancel(*arguments, **keywords):
            child = start(*arguments, **keywords)
            children.append(child)
            os.kill(os.getpid(), signal.SIGTERM)
            return child

        before = signal.pthread_sigmask(signal.SIG_BLOCK, set())
        with (mock.patch.object(GUARD, "spawn_custodian", side_effect=cancel),
              self.assertRaises(SystemExit) as stopped):
            GUARD.run_recorded([sys.executable, "-I", "-B", "-c",
                                "import signal; signal.alarm(60); signal.pause()"], result_file=path)
        self.assertEqual(stopped.exception.code, 143)
        self.assertEqual(signal.pthread_sigmask(signal.SIG_BLOCK, set()), before)
        self.assertFalse(path.exists())
        self.assertTrue(children)
        for child in children:
            self.assertIsNotNone(child.returncode)
            self.assertFalse(GUARD.group_exists(child.pid))

    def test_publication_failures_invalidate_even_a_fully_written_record(self):
        for operation in ("partial-write", "zero-write", "file-fsync", "directory-fsync",
                          "file-close", "directory-close"):
            with self.subTest(operation=operation):
                path = self.root / (operation + ".json")
                original_write, original_sync, original_close = os.write, os.fsync, os.close
                failed, completed_before_fault = [], []

                def selected(descriptor, directory):
                    value = os.fstat(descriptor)
                    expected = path.parent.stat() if directory else path.stat()
                    return (value.st_dev, value.st_ino) == (expected.st_dev, expected.st_ino)

                def write(descriptor, data):
                    if operation in ("partial-write", "zero-write") and selected(descriptor, False):
                        if operation == "zero-write":
                            return 0
                        original_write(descriptor, data[:7])
                        raise OSError(errno.EIO, "controlled publication failure")
                    return original_write(descriptor, data)

                def sync(descriptor):
                    if (not failed and operation.endswith("fsync")
                            and selected(descriptor, operation.startswith("directory"))):
                        completed_before_fault.append(json.loads(path.read_text()))
                        failed.append(True)
                        raise OSError(errno.EIO, "controlled publication failure")
                    return original_sync(descriptor)

                def close(descriptor):
                    should_fail = (not failed and operation.endswith("close")
                                   and selected(descriptor, operation.startswith("directory")))
                    if should_fail:
                        completed_before_fault.append(json.loads(path.read_text()))
                    original_close(descriptor)
                    if should_fail:
                        failed.append(True)
                        raise OSError(errno.EIO, "controlled publication failure")

                with (mock.patch.object(GUARD.os, "write", side_effect=write),
                      mock.patch.object(GUARD.os, "fsync", side_effect=sync),
                      mock.patch.object(GUARD.os, "close", side_effect=close),
                      self.assertRaises(OSError) as failure):
                    GUARD.run_recorded([sys.executable, "-I", "-B", "-c", "pass"],
                                       result_file=path)
                self.assertEqual(failure.exception.errno, errno.EIO)
                self.assertFalse(path.exists(), "persistence failure must leave no usable success")
                if operation.endswith(("fsync", "close")):
                    self.assertEqual(len(completed_before_fault), 1)
                    self.assertIs(completed_before_fault[0]["completed"], True)
                    self.assertEqual(completed_before_fault[0]["normalized_exit_status"], 0)

    def test_receipt_claim_does_not_join_inherited_caller_descriptors(self):
        path, observed = self.root / "fds.json", self.root / "observed.json"
        inherited = self.root / "caller-fd"
        inherited.write_bytes(b"allowed")
        source = f'''
import json,os
from pathlib import Path
opened=[]
for descriptor in range(64):
    try:
        value=os.fstat(descriptor)
        opened.append([value.st_dev,value.st_ino])
    except OSError: pass
Path({str(observed)!r}).write_text(json.dumps([opened,os.read(int(os.environ['RECEIPT_TEST_FD']),7).decode()]))
'''
        with inherited.open("rb") as stream:
            os.set_inheritable(stream.fileno(), True)
            with mock.patch.dict(os.environ, RECEIPT_TEST_FD=str(stream.fileno())):
                self.assertEqual(GUARD.run_recorded(
                    [sys.executable, "-I", "-B", "-c", source], result_file=path,
                    inherit_fds=True), 0)
        opened, contents = json.loads(observed.read_text())
        self.assertEqual(contents, "allowed")
        for hidden in (path, path.parent):
            value = hidden.stat()
            self.assertNotIn([value.st_dev, value.st_ino], opened)

    def test_replaced_claim_is_neither_completed_nor_removed(self):
        path, replacement = self.root / "replaced.json", self.root / "other-owner"
        replacement.write_bytes(b"replacement belongs to another owner")
        source = f"import os; os.replace({str(replacement)!r}, {str(path)!r})"
        with self.assertRaisesRegex(RuntimeError, "replaced after its claim"):
            GUARD.run_recorded([sys.executable, "-I", "-B", "-c", source], result_file=path)
        self.assertEqual(path.read_bytes(), b"replacement belongs to another owner")

    def test_result_option_cannot_launch_a_cache_guardian(self):
        record = self.root / "result.json"
        ran = self.root / "command-ran"
        lock = self.root / "cache.lock"
        command = [sys.executable, "-I", str(ROOT / "alab/command_guard.py"),
                   "--result-file", str(record), "--cache-lock", str(lock),
                   "--", sys.executable, "-I", "-c",
                   f"from pathlib import Path; Path({str(ran)!r}).touch()"]
        done = subprocess.run(command, capture_output=True, text=True, timeout=10)
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("cannot record a cache guardian", done.stderr)
        self.assertFalse(ran.exists())
        self.assertFalse(record.exists())
        self.assertFalse(lock.exists())


if __name__ == "__main__":
    unittest.main()
