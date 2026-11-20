# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Controls for the two real commands used to validate cross-project resume."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import account_resume


def run_call(directory, call):
    command = json.loads(call["arguments"])["cmd"]
    output = subprocess.run(["/bin/sh", "-c", command], cwd=directory, check=True,
                            stdout=subprocess.PIPE, text=True).stdout
    return {"type": "function_call_output", "call_id": call["call_id"], "output": output}


class ResumeOutputDirectoryTests(unittest.TestCase):
    def test_symlinked_parent_uses_physical_directory_for_the_fixture(self):
        base = Path(self.enterContext(tempfile.TemporaryDirectory()))
        alias = base / "alias"
        alias.symlink_to(base, target_is_directory=True)
        with patch.object(account_resume.fixture, "fresh_environment", side_effect=InterruptedError) as prepare:
            with self.assertRaises(InterruptedError):
                account_resume.run(Path("/unused"), alias / "probe", "id")
        root = base.resolve() / "probe"
        prepare.assert_called_once_with(root)
        self.assertTrue(root.is_dir())
        self.assertEqual(root.stat().st_mode & 0o077, 0)

    def test_existing_output_including_a_dangling_link_is_refused(self):
        base = Path(self.enterContext(tempfile.TemporaryDirectory()))
        existing = base / "existing"
        existing.mkdir()
        linked = base / "linked"
        linked.symlink_to(existing, target_is_directory=True)
        missing = base / "missing"
        dangling = base / "dangling"
        dangling.symlink_to(missing, target_is_directory=True)
        for output in (existing, linked, dangling):
            with self.subTest(output=output), self.assertRaises(FileExistsError):
                account_resume.run(Path("/unused"), output, "id")
        self.assertEqual(list(existing.iterdir()), [])
        self.assertFalse(missing.exists())


class CrossProjectDirectoryTests(unittest.TestCase):
    def setUp(self):
        directory = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.work, self.other = directory / "original", directory / "picker"
        for path, marker in ((self.work, "original marker"), (self.other, "picker marker")):
            path.mkdir()
            (path / "cedar-marker.txt").write_text(marker + "\n")
        seed = account_resume.TOOL_CALL
        self.turn = {"turn": "second-turn", "thread": "same-thread", "history": ["continued"],
                     "tool_records": [seed, run_call(self.work, seed)]}

    def check(self, directory):
        call = account_resume.RESUME_DIRECTORY_CALL
        result = dict(self.turn, history=[*self.turn["history"], "function_call", "function_call_output"],
                      tool_records=[*self.turn["tool_records"], call, run_call(directory, call)])
        return account_resume.check_resumed_directory(self.work, self.other, "original marker", self.turn, result)

    def test_directory_observation_requires_the_commands_actual_workspace(self):
        self.assertEqual(self.check(self.work), str(self.work.resolve()))
        with self.assertRaisesRegex(RuntimeError, "selected physical directory and workspace marker"):
            self.check(self.other)

    def test_replay_in_picker_directory_is_detected_with_source_counter_still_one(self):
        run_call(self.other, account_resume.TOOL_CALL)
        self.assertEqual((self.work / "cedar-calls").read_text(), "original marker\n")
        self.assertEqual((self.other / "cedar-calls").read_text(), "picker marker\n")
        with self.assertRaisesRegex(RuntimeError, "original command ran in the unselected project"):
            self.check(self.work)
