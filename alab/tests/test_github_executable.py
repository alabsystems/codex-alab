# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Executable qualification refuses owned special files before bounded help."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
CHILD = r'''
import json
import os
from pathlib import Path
import runpy
import sys

source, selected, witness, mode, elsewhere = sys.argv[1:]
witness = Path(witness)
api = runpy.run_path(source, run_name="owned_agh_fixture")
Agh, Refused = api["Agh"], api["Refused"]
state = {"stage": "loaded", "help_calls": [], "process_calls": []}

def save():
    witness.write_text(json.dumps(state))

def deny_help(self, *args, **kwargs):
    state["help_calls"].append(list(args))
    save()
    raise AssertionError("special or unavailable executable reached help")

if mode == "refuse":
    Agh.call = deny_help
else:
    original_run = api["subprocess"].run
    def real_run(argv, **kwargs):
        state["process_calls"].append({"argv": argv, "executable": kwargs.get("executable"),
                                       "git_context": any(k.startswith("GIT_") for k in kwargs["env"])})
        return original_run(argv, **kwargs)
    api["subprocess"].run = real_run

os.chdir(elsewhere)
state["stage"] = "before-constructor"
save()
try:
    client = Agh()
except BaseException as error:
    state.update(stage="constructor-error", error_type=type(error).__name__, error=str(error))
else:
    state.update(stage="constructor-returned", executable=client.executable,
                 identity=list(client.identity), sha256=client.sha256)
    if mode == "positive":
        state["probe"] = client.call("probe", anonymous=True).decode()
        image = Path(client.executable)
        image.write_bytes(image.read_bytes() + b"\n# owned identity change\n")
        try:
            client.call("must-not-run")
        except Refused as error:
            state["changed_refusal"] = str(error)
save()
'''


class GithubExecutableTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix="alab-agh-executable-")
        self.addCleanup(scratch.cleanup)
        self.home = Path(scratch.name).resolve()
        self.source = self.home / "source"
        self.source.mkdir()
        for name in ("github.py", "cli_status.py", "file_hash.py"):
            path = self.source / name
            path.write_bytes((ROOT / name).read_bytes())
            path.chmod(0o400)
        self.source.chmod(0o500)
        self.elsewhere = self.home / "elsewhere"
        self.elsewhere.mkdir()
        self.child = self.home / "child.py"
        self.child.write_text(CHILD)
        self.sequence = 0

    def exercise(self, selected, mode="refuse"):
        self.sequence += 1
        witness = self.home / ("witness-" + str(self.sequence) + ".json")
        env = {k: v for k, v in os.environ.items() if not k.startswith(("ALAB_", "GIT_"))}
        env.update(ALAB_AGH=str(selected), GIT_DIR=str(self.home / "unrelated-git-context"))
        command = [sys.executable, "-I", "-B", str(self.child), "source/github.py",
                   str(selected), str(witness), mode, str(self.elsewhere)]
        try:
            done = subprocess.run(command, cwd=self.home, env=env,
                                  capture_output=True, text=True, timeout=4)
        except subprocess.TimeoutExpired:
            # subprocess.run kills and waits for this direct owned child before
            # raising. Preserve its last closed marker rather than retrying it.
            state = json.loads(witness.read_text()) if witness.exists() else {"stage": "no-marker"}
            self.fail("constructor child timed out after 4s and was killed/reaped: " + repr(state))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertTrue(witness.is_file(), done.stderr)
        return json.loads(witness.read_text())

    def assert_refused(self, state, message):
        self.assertEqual(state["stage"], "constructor-error", state)
        self.assertEqual(state["error_type"], "Refused", state)
        self.assertIn(message, state["error"], state)
        self.assertEqual(state["help_calls"], [], state)

    def test_executable_fifo_without_a_writer_is_refused_before_help(self):
        fifo = self.home / "selected-fifo"
        os.mkfifo(fifo, 0o700)
        fifo.chmod(0o700)
        self.assertTrue(os.access(fifo, os.X_OK))
        self.assert_refused(self.exercise(fifo), "not a regular file")

    def test_executable_directory_is_a_typed_regularity_refusal(self):
        directory = self.home / "selected-directory"
        directory.mkdir(mode=0o700)
        self.assertTrue(os.access(directory, os.X_OK))
        self.assert_refused(self.exercise(directory), "not a regular file")

    def test_real_symlinked_executable_keeps_digest_identity_and_call_guard(self):
        image = self.home / "real inert agh"
        raw = (b"#!/bin/sh\nif [ \"$1\" = help ]; then\n"
               b"  printf '%s\\n' '--ref --into --input --anonymous'\n"
               b"else\n  printf 'owned response\\n'\nfi\n")
        image.write_bytes(raw)
        image.chmod(0o700)
        before = image.stat()
        selected = self.home / "selected-agh"
        selected.symlink_to(image.name)
        state = self.exercise(selected, "positive")
        self.assertEqual(state["stage"], "constructor-returned", state)
        self.assertEqual(state["executable"], str(image.resolve()))
        self.assertEqual(state["sha256"], hashlib.sha256(raw).hexdigest())
        self.assertEqual(state["identity"], [before.st_dev, before.st_ino, before.st_size,
                                            before.st_mtime_ns, before.st_ctime_ns])
        self.assertEqual(state["probe"], "owned response\n")
        self.assertEqual([call["argv"] for call in state["process_calls"]],
                         [["agh", "help", "repo", "fetch"], ["agh", "--anonymous", "probe"]])
        self.assertTrue(all(call["executable"] == str(image.resolve()) and not call["git_context"]
                            for call in state["process_calls"]))
        self.assertEqual(state.get("changed_refusal"), "selected Agh executable changed during the gate")

    def test_missing_and_nonexecutable_selections_keep_unavailable_refusal(self):
        missing = self.home / "missing-agh"
        nonexecutable = self.home / "nonexecutable-agh"
        nonexecutable.write_text("#!/bin/sh\nexit 99\n")
        nonexecutable.chmod(0o600)
        for selected in (missing, nonexecutable):
            with self.subTest(selected=selected.name):
                self.assertFalse(os.access(selected, os.X_OK))
                self.assert_refused(self.exercise(selected), "Agh is unavailable")
