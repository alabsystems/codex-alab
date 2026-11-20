# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Real owned gate outcomes survive observation and publication failures."""

import json
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace
import unittest

import test_qualification as fixture


CHILD = r'''
import ast
import errno
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile

source, repo, output, witness_path = map(Path, sys.argv[1:5])
source, repo, output, witness_path = (path.resolve() for path in (source, repo, output, witness_path))
phase, kind = sys.argv[5:7]
spec = importlib.util.spec_from_file_location("publication_qualification", source)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
final = output / "record.json"
gate = repo / "alab/check.sh"
witness = {"wait_statuses": [], "faults": [], "links": []}
raised = False
failure = (KeyboardInterrupt("controlled publication cancellation") if kind == "keyboard"
           else SystemExit(47) if kind == "exit47"
           else OSError(errno.EIO, "controlled publication " + phase))
original_open = Path.open
original_unlink = Path.unlink
original_temporary = tempfile.NamedTemporaryFile
original_dump = json.dump
original_link = os.link
original_dup2 = os.dup2
original_os_open = os.open
original_popen = module.subprocess.Popen
original_snapshot = module.snapshot
original_hash = module.FILE_HASH["file_sha256"]
writers = {}


def duplicate_descriptor(source, destination, *arguments, **keywords):
    witness.setdefault("duplicated_descriptors", []).append(destination)
    return original_dup2(source, destination, *arguments, **keywords)


def open_descriptor(path, flags, *arguments, **keywords):
    if (path == os.devnull and flags == os.O_WRONLY
            and "broken_output_descriptor" in witness and phase.endswith("-null")):
        witness.setdefault("null_open_errors", []).append(errno.EMFILE)
        raise OSError(errno.EMFILE, "controlled null output allocation failure")
    return original_os_open(path, flags, *arguments, **keywords)


def break_output(descriptor):
    reader, writer = os.pipe()
    os.close(reader)
    os.dup2(writer, descriptor)
    os.close(writer)
    witness["broken_output_descriptor"] = descriptor


def fault(at):
    global raised
    if phase == at and not raised:
        raised = True
        witness["faults"].append(at)
        raise failure


class OwnedStream:
    def __init__(self, stream, role):
        self.stream = stream
        self.role = role
        self.final_record = False
        self.consumed = False
        writers[str(stream.name)] = self

    def __getattr__(self, name):
        return getattr(self.stream, name)

    def write(self, value):
        return self.stream.write(value)

    def close(self):
        if self.consumed:
            raise AssertionError("owned writer was closed twice")
        self.consumed = True
        self.stream.close()
        if self.final_record:
            witness["final_writer_closed"] = self.stream.closed
            witness["final_visible_at_close"] = final.exists()
            fault("record-close")
        if self.role == "log":
            witness["gate_log_closed"] = self.stream.closed
            fault("log-close")

    def __enter__(self):
        return self

    def __exit__(self, *arguments):
        self.close()
        return False


class ControlledPath(type(Path())):
    def open(self, *arguments, **keywords):
        stream = original_open(self, *arguments, **keywords)
        mode = arguments[0] if arguments else keywords.get("mode", "r")
        if self.parent == output and any(flag in mode for flag in "wax"):
            return OwnedStream(stream, "log" if self.name == "gate.log" else "record")
        return stream

    def unlink(self, *arguments, **keywords):
        owner = writers.get(str(self))
        if owner is not None and owner.final_record and self != final and final.exists():
            witness["temporary_cleanup_path"] = str(self)
            witness["temporary_existed_at_cleanup"] = self.is_file()
            fault("temporary-unlink")
        return original_unlink(self, *arguments, **keywords)


def temporary(*arguments, **keywords):
    stream = original_temporary(*arguments, **keywords)
    if Path(stream.name).parent == output:
        return OwnedStream(stream, "record")
    return stream


def dump(value, stream, *arguments, **keywords):
    if isinstance(value, dict) and value.get("kind") == "native-check-execution-observation":
        if not isinstance(stream, OwnedStream):
            raise AssertionError("final JSON did not use an observed owned writer")
        stream.final_record = True
        original_dump(value, stream, *arguments, **keywords)
        stream.flush()
        raw = Path(stream.name).read_bytes()
        complete = json.loads(raw)
        identity = os.fstat(stream.fileno())
        witness.update(final_json_complete=True, final_dump_status=complete["status"],
                       final_dump_qualified_source=complete["qualified_source"],
                       final_writer_path=str(stream.name),
                       final_writer_inode=[identity.st_dev, identity.st_ino],
                       final_visible_at_dump=final.exists())
        fault("record-write")
        return None
    return original_dump(value, stream, *arguments, **keywords)


def link(src, dst, *arguments, **keywords):
    if Path(dst) == final:
        owner = writers.get(str(src))
        event = {"writer_closed": owner is not None and owner.stream.closed,
                 "destination_existed": final.exists()}
        original_link(src, dst, *arguments, **keywords)
        identity = final.stat()
        event["published_inode"] = [identity.st_dev, identity.st_ino]
        witness["links"].append(event)
        return None
    return original_link(src, dst, *arguments, **keywords)


class GatePipe:
    def __init__(self, stream):
        self.stream = stream

    def __getattr__(self, name):
        return getattr(self.stream, name)

    def close(self):
        self.stream.close()
        witness["gate_pipe_closed"] = self.stream.closed
        fault("stdout-close")


class GateProcess:
    def __init__(self, process):
        self.process = process
        self.stdout = GatePipe(process.stdout)

    def __getattr__(self, name):
        return getattr(self.process, name)

    def wait(self, *arguments, **keywords):
        status = self.process.wait(*arguments, **keywords)
        witness["wait_statuses"].append(status)
        if phase.startswith("broken-stdout"):
            break_output(1)
        elif phase.startswith("broken-stderr"):
            break_output(2)
        return status


def popen(command, *arguments, **keywords):
    process = original_popen(command, *arguments, **keywords)
    if isinstance(command, (list, tuple)) and command and str(command[0]) == str(gate):
        witness["actual_gate_started"] = True
        return GateProcess(process)
    return process


def snapshot(*arguments, **keywords):
    observed = original_snapshot(*arguments, **keywords)
    if witness["wait_statuses"]:
        witness["actual_post_wait_snapshot"] = observed
        fault("snapshot")
    return observed


def file_hash(path, *arguments, **keywords):
    global raised
    value = original_hash(path, *arguments, **keywords)
    if Path(path) == output / "gate.log" and witness["wait_statuses"]:
        witness["actual_closed_log_hash"] = value
        if phase == "collision" and "collision_inode" not in witness:
            with original_open(final, "xb") as existing:
                existing.write(b"owned existing record must survive\n")
            identity = final.stat()
            witness["collision_inode"] = [identity.st_dev, identity.st_ino]
            witness["faults"].append("collision")
        fault("hash")
        if phase.startswith("broken-stderr"):
            raised = True
            witness["faults"].append("hash")
            raise failure
        if phase == "hash-epipe":
            raised = True
            witness["faults"].append("hash-epipe")
            raise BrokenPipeError(errno.EPIPE, "controlled hash failure")
    return value


class NoticeBuffer:
    def __init__(self, stream):
        self.stream = stream
        self.final_notice = False

    def write(self, value):
        count = self.stream.write(value)
        if value.startswith(b"qualification record: "):
            self.final_notice = True
            witness["final_notice_written"] = True
            fault("notice-write")
            fault("broken-stdout-cancel")
            fault("broken-stdout-cancel-null")
        return count

    def flush(self):
        self.stream.flush()
        if self.final_notice:
            witness["final_notice_flushed"] = True
            fault("notice-flush")


class NoticeStdout:
    def __init__(self, stream):
        self.stream = stream
        self.buffer = NoticeBuffer(stream.buffer)

    def __getattr__(self, name):
        return getattr(self.stream, name)


module.Path = ControlledPath
module.tempfile.NamedTemporaryFile = temporary
module.json.dump = dump
module.os.link = link
module.os.dup2 = duplicate_descriptor
module.os.open = open_descriptor
module.subprocess.Popen = popen
module.snapshot = snapshot
module.FILE_HASH["file_sha256"] = file_hash
sys.stdout = NoticeStdout(sys.stdout)
sys.argv = [str(source), "--repo", str(repo), "--directory", str(output)]
tree = ast.parse(source.read_bytes(), filename=str(source))
guards = [node for node in tree.body if isinstance(node, ast.If)
          and isinstance(node.test, ast.Compare)
          and isinstance(node.test.left, ast.Name) and node.test.left.id == "__name__"]
assert len(guards) == 2
witness["original_entry_guards"] = len(guards)
module.__name__ = "__main__"
try:
    exec(compile(ast.Module(body=guards, type_ignores=[]), str(source), "exec"), module.__dict__)
except BaseException as error:
    witness["escaped_type"] = type(error).__name__
    witness["escaped_code"] = error.code if isinstance(error, SystemExit) else None
    witness["escaped_failure_identity"] = error is failure
    raise
finally:
    witness["fault_was_raised"] = raised
    witness["final_exists"] = final.exists()
    if phase.endswith("-null") and "broken_output_descriptor" in witness:
        failed_stream = sys.stdout if witness["broken_output_descriptor"] == 1 else sys.stderr
        witness["failed_standard_stream_closed"] = failed_stream.closed
    marker = repo / ".git/publication-gate-ran"
    witness["gate_marker"] = marker.read_text() if marker.exists() else None
    witness_path.write_text(json.dumps(witness))
'''


class QualificationPublicationTests(unittest.TestCase):
    def setUp(self):
        self.world = fixture.QualificationTests()
        self.world.setUp()
        self.addCleanup(self.world.doCleanups)
        self.root = self.world.root
        self.sequence = 0

    def run_cli(self, phase="healthy", gate=0, kind="ordinary"):
        self.sequence += 1
        body = "# owned CLI scenario " + str(self.sequence) + "\n"
        body += 'printf "finite gate stdout\\n"\nprintf "finite gate stderr\\n" >&2\n'
        body += "printf '%s\\n' " + str(gate) + " > .git/publication-gate-ran\n"
        body += 'kill -TERM "$$"\n' if gate == "signal" else "exit " + str(gate) + "\n"
        self.world.install(body)
        source = self.world.git("rev-parse", "HEAD")
        child = self.root / "publication-child.py"
        child.write_text(CHILD)
        output = self.root / ("observation-" + str(self.sequence))
        witness_path = self.root / ("witness-" + str(self.sequence) + ".json")
        done = subprocess.run(
            [sys.executable, "-I", "-B", str(child), str(Path(fixture.QUAL.__file__)),
             str(self.world.repo), str(output), str(witness_path), phase, kind],
            cwd=self.root, capture_output=True, text=True, timeout=30)
        witness = json.loads(witness_path.read_text())
        self.assertEqual(witness["original_entry_guards"], 2)
        self.assertTrue(witness["actual_gate_started"])
        self.assertEqual(witness["gate_marker"], str(gate) + "\n")
        self.assertEqual(witness["wait_statuses"], [-15 if gate == "signal" else gate])
        return SimpleNamespace(done=done, witness=witness, output=output, source=source,
                               expected_exit=143 if gate == "signal" else gate)

    def assert_fault(self, result, phase):
        self.assertIn(phase, result.witness["faults"], result.witness)
        if phase != "collision":
            self.assertTrue(result.witness["fault_was_raised"], result.witness)

    def assert_not_qualified(self, result):
        record = result.output / "record.json"
        if record.exists():
            with self.assertRaises((OSError, ValueError)):
                fixture.QUAL.inspect(self.world.repo, record)

    def test_healthy_cli_retains_an_inspectable_actual_gate_record(self):
        result = self.run_cli()
        self.assertEqual(result.done.returncode, 0, result.done.stderr)
        self.assertEqual(fixture.QUAL.inspect(
            self.world.repo, result.output / "record.json")["source"], result.source)
        self.assertTrue(result.witness["final_json_complete"])
        self.assertTrue(result.witness["final_writer_closed"])

    def test_complete_json_then_write_error_cannot_publish_success(self):
        result = self.run_cli("record-write")
        self.assert_fault(result, "record-write")
        self.assertTrue(result.witness["final_json_complete"])
        self.assertEqual(result.witness["final_dump_status"], 0)
        self.assertEqual(result.witness["final_dump_qualified_source"], result.source)
        self.assertEqual(result.done.returncode, 1, result.done.stderr)
        self.assert_not_qualified(result)

    def test_actual_final_close_error_cannot_publish_success(self):
        result = self.run_cli("record-close")
        self.assert_fault(result, "record-close")
        self.assertTrue(result.witness["final_json_complete"])
        self.assertTrue(result.witness["final_writer_closed"])
        self.assertEqual(result.done.returncode, 1, result.done.stderr)
        self.assert_not_qualified(result)

    def test_publication_waits_for_the_owned_writer_to_close(self):
        result = self.run_cli()
        self.assertEqual(result.done.returncode, 0, result.done.stderr)
        self.assertFalse(result.witness["final_visible_at_dump"])
        self.assertFalse(result.witness["final_visible_at_close"])
        self.assertEqual(len(result.witness["links"]), 1)
        link = result.witness["links"][0]
        self.assertTrue(link["writer_closed"])
        self.assertFalse(link["destination_existed"])
        self.assertEqual(link["published_inode"], result.witness["final_writer_inode"])

    def test_existing_final_record_is_not_replaced(self):
        for gate in (0, 23, 75, "signal"):
            with self.subTest(gate=gate):
                result = self.run_cli("collision", gate)
                self.assert_fault(result, "collision")
                record = result.output / "record.json"
                self.assertEqual(record.read_bytes(), b"owned existing record must survive\n")
                identity = record.stat()
                self.assertEqual([identity.st_dev, identity.st_ino],
                                 result.witness["collision_inode"])
                self.assertEqual(result.done.returncode, result.expected_exit or 1,
                                 result.done.stderr)
                self.assert_not_qualified(result)

    def test_known_gate_failure_survives_final_record_errors(self):
        for gate in (23, 75, "signal"):
            for phase in ("record-write", "record-close"):
                with self.subTest(gate=gate, phase=phase):
                    result = self.run_cli(phase, gate)
                    self.assert_fault(result, phase)
                    self.assertTrue(result.witness["final_json_complete"])
                    self.assertEqual(result.done.returncode, result.expected_exit,
                                     result.done.stderr)
                    self.assert_not_qualified(result)

    def test_known_gate_failure_survives_post_wait_close_errors(self):
        for gate in (23, 75, "signal"):
            for phase in ("stdout-close", "log-close"):
                with self.subTest(gate=gate, phase=phase):
                    result = self.run_cli(phase, gate)
                    self.assert_fault(result, phase)
                    self.assertTrue(result.witness[
                        "gate_pipe_closed" if phase == "stdout-close" else "gate_log_closed"])
                    self.assertEqual(result.done.returncode, result.expected_exit,
                                     result.done.stderr)
                    self.assert_not_qualified(result)

    def test_known_gate_failure_survives_post_wait_observation_errors(self):
        for gate in (23, 75, "signal"):
            for phase in ("snapshot", "hash"):
                with self.subTest(gate=gate, phase=phase):
                    result = self.run_cli(phase, gate)
                    self.assert_fault(result, phase)
                    self.assertEqual(result.done.returncode, result.expected_exit,
                                     result.done.stderr)
                    self.assert_not_qualified(result)

    def test_known_gate_failure_survives_final_notice_errors(self):
        for gate in (23, 75, "signal"):
            for phase in ("notice-write", "notice-flush"):
                with self.subTest(gate=gate, phase=phase):
                    result = self.run_cli(phase, gate)
                    self.assert_fault(result, phase)
                    self.assertEqual(result.done.returncode, result.expected_exit,
                                     result.done.stderr)
                    self.assert_not_qualified(result)

    def test_published_temporary_cleanup_error_is_advisory(self):
        result = self.run_cli("temporary-unlink")
        self.assert_fault(result, "temporary-unlink")
        self.assertTrue(result.witness["temporary_existed_at_cleanup"])
        self.assertEqual(result.done.returncode, 0, result.done.stderr)
        self.assertEqual(fixture.QUAL.inspect(
            self.world.repo, result.output / "record.json")["source"], result.source)

    def test_post_wait_cancellation_preserves_authoritative_identity(self):
        for gate in (0, 23):
            for phase in ("record-close", "snapshot", "temporary-unlink"):
                for kind in ("exit47", "keyboard"):
                    with self.subTest(gate=gate, phase=phase, kind=kind):
                        result = self.run_cli(phase, gate, kind)
                        self.assert_fault(result, phase)
                        self.assertTrue(result.witness["escaped_failure_identity"])
                        if kind == "exit47":
                            self.assertEqual(result.witness["escaped_type"], "SystemExit")
                            self.assertEqual(result.witness["escaped_code"], 47)
                            self.assertEqual(result.done.returncode, 47)
                        else:
                            self.assertEqual(result.witness["escaped_type"], "KeyboardInterrupt")
                            self.assertNotEqual(result.done.returncode, 0)

    def test_real_broken_stdout_does_not_replace_the_gate_status_at_shutdown(self):
        for gate in (0, 23, 75, "signal"):
            with self.subTest(gate=gate):
                result = self.run_cli("broken-stdout", gate)
                self.assertEqual(result.witness["broken_output_descriptor"], 1)
                self.assertTrue(result.witness["final_notice_written"])
                self.assertEqual(result.done.returncode, result.expected_exit or 1,
                                 result.done.stderr)
                self.assertEqual(result.witness["duplicated_descriptors"], [1, 1])
                self.assertNotIn("Exception ignored while flushing", result.done.stderr)

    def test_real_broken_stderr_does_not_replace_the_gate_status_at_shutdown(self):
        for gate in (0, 23, 75, "signal"):
            with self.subTest(gate=gate):
                result = self.run_cli("broken-stderr", gate)
                self.assert_fault(result, "hash")
                self.assertEqual(result.witness["broken_output_descriptor"], 2)
                self.assertEqual(result.done.returncode, result.expected_exit or 1)
                self.assertEqual(result.witness["duplicated_descriptors"], [2, 2])
                self.assert_not_qualified(result)

    def test_broken_output_preserves_pending_cancellation_identity(self):
        for gate in (0, 23):
            for kind in ("exit47", "keyboard"):
                with self.subTest(gate=gate, kind=kind):
                    result = self.run_cli("broken-stdout-cancel", gate, kind)
                    self.assert_fault(result, "broken-stdout-cancel")
                    self.assertTrue(result.witness["escaped_failure_identity"])
                    self.assertEqual(result.done.returncode, 47 if kind == "exit47" else -2,
                                     result.done.stderr)
                    self.assertEqual(result.witness["duplicated_descriptors"], [1, 1])
                    self.assertNotIn("Exception ignored while flushing", result.done.stderr)

    def test_unrelated_epipe_does_not_redirect_a_healthy_output_descriptor(self):
        for gate in (23, 75, "signal"):
            with self.subTest(gate=gate):
                result = self.run_cli("hash-epipe", gate)
                self.assert_fault(result, "hash-epipe")
                self.assertEqual(result.done.returncode, result.expected_exit,
                                 result.done.stderr)
                self.assertNotIn("duplicated_descriptors", result.witness)
                self.assertIn("finite gate stdout", result.done.stdout)
                self.assertNotIn("broken_output_descriptor", result.witness)
