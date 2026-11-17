# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""History's public CLI must refuse unstable hashes without any application launch."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import test_history_cli_status as history_cli


HOOK = r'''
import os
scenario = sys.argv[4]
original_open, original_os_open, original_read, original_close = Path.open, os.open, os.read, os.close
opens, phases, binary_fds = 0, {}, set()
target = 2
witness['hash_reads'] = 0
witness['hash_bytes'] = 0
witness['initial_binary_size'] = binary.stat().st_size
def changed(count):
    if not count: return
    if scenario == 'same-size-final' and not witness.get('mutated'):
        binary.write_bytes(b'X' * witness['initial_binary_size'])
        witness['mutated'] = True
    elif scenario == 'append-final':
        with original_open(binary, 'ab') as stream: stream.write(b'x')
def before_read():
    witness['hash_reads'] += 1
    assert witness['hash_reads'] < 4, 'owned four-call hash growth guard reached'
def after_read(count):
    witness['hash_bytes'] += count
    changed(count)
class ObservedReader:
    def __init__(self, handle): self.handle = handle
    def __getattr__(self, name): return getattr(self.handle, name)
    def __enter__(self): return self
    def __exit__(self, *args): return self.handle.__exit__(*args)
    def readinto(self, buffer):
        before_read()
        count = self.handle.readinto(buffer)
        after_read(count)
        return count
def tracked_open(path, *args, **kwargs):
    global opens
    result = original_open(path, *args, **kwargs)
    if path == binary and args and args[0] == 'rb':
        opens += 1
        if opens == target: return ObservedReader(result)
    return result
def tracked_os_open(path, *args, **kwargs):
    global opens
    result = original_os_open(path, *args, **kwargs)
    if Path(path) == binary:
        binary_fds.add(result)
        opens += 1
        if opens == target: phases[result] = True
    return result
def tracked_read(fd, amount):
    if fd not in phases: return original_read(fd, amount)
    before_read()
    data = original_read(fd, amount)
    after_read(len(data))
    return data
def tracked_close(fd):
    original_close(fd)
    if fd in binary_fds:
        binary_fds.remove(fd)
        if scenario == 'close-zero-initial': raise SystemExit(0)
Path.open, os.open, os.read, os.close = tracked_open, tracked_os_open, tracked_read, tracked_close
def fifo():
    binary.unlink()
    os.mkfifo(binary)
if scenario == 'fifo-initial': fifo()
if scenario == 'fifo-final':
    original_tui = value['run_tui']
    def tui_then_fifo(*args):
        original_tui(*args)
        fifo()
    value['run_tui'] = tui_then_fifo
'''


class HistoryFileHashTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-history-hash-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()

    def child(self, scenario):
        script = self.root / "entry-child.py"
        source = history_cli.CHILD.replace("report_fault = None", HOOK + "\nreport_fault = None")
        source = source.replace("witness['binary_unchanged'] = hashlib.sha256(binary.read_bytes()).hexdigest() == initial",
                                "witness['binary_unchanged'] = binary.is_file() and hashlib.sha256(binary.read_bytes()).hexdigest() == initial")
        script.write_text(source)
        done = subprocess.run([sys.executable, "-I", "-B", str(script), str(history_cli.SOURCE),
                               "healthy", str(self.root), scenario], cwd=self.root,
                              capture_output=True, text=True, timeout=5)
        witness = json.loads((self.root / "witness.json").read_text())
        self.assertEqual(witness["process_attempts"], [])
        self.assertTrue(witness["original_entry_executed"])
        return done, witness

    def test_stable_binary_keeps_the_original_cli_success(self):
        done, witness = self.child("healthy")
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertTrue(witness["report"]["passed"])
        self.assertTrue(witness["binary_unchanged"])
        self.assertEqual(witness["cleanup"], ["shutdown", "join", "close"])

    def test_same_size_overwrite_during_final_hash_cannot_publish_success(self):
        done, witness = self.child("same-size-final")
        self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertTrue(witness["mutated"])
        self.assertFalse(witness["report"]["passed"])
        self.assertIn("changed while hashing", "\n".join(witness["report"]["cleanupErrors"]))
        self.assertEqual(witness["cleanup"], ["shutdown", "join", "close"])
        self.assertNotIn('"passed": true', done.stdout)

    def test_success_shaped_initial_close_failure_keeps_the_original_cli_nonzero(self):
        done, witness = self.child("close-zero-initial")
        self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertIsNone(witness["report"])
        self.assertNotIn("seed_entered", witness)
        self.assertIn("close incomplete", done.stdout + done.stderr)

    def test_continuous_appends_are_refused_at_the_initial_size_plus_one(self):
        done, witness = self.child("append-final")
        self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertFalse(witness["report"]["passed"])
        self.assertEqual(witness["hash_bytes"], witness["initial_binary_size"] + 1)
        self.assertEqual(witness["hash_reads"], 2)
        self.assertIn("changed while hashing", "\n".join(witness["report"]["cleanupErrors"]))

    def test_fifo_at_either_hash_boundary_refuses_without_waiting_for_a_writer(self):
        for scenario in ("fifo-initial", "fifo-final"):
            with self.subTest(scenario=scenario):
                temporary = tempfile.TemporaryDirectory(dir=self.root)
                self.addCleanup(temporary.cleanup)
                prior, self.root = self.root, Path(temporary.name)
                try:
                    done, witness = self.child(scenario)
                finally:
                    self.root = prior
                self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
                if scenario == "fifo-final":
                    self.assertFalse(witness["report"]["passed"])
                    self.assertIn("not a regular file", "\n".join(witness["report"]["cleanupErrors"]))
                    self.assertEqual(witness["cleanup"], ["shutdown", "join", "close"])
                else:
                    self.assertIn("not a regular file", done.stdout + done.stderr)
                    self.assertIsNone(witness["report"])
                    self.assertNotIn("seed_entered", witness)
