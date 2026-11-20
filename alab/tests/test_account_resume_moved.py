# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0

from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import account_resume_moved


class MovedConversation(unittest.TestCase):
    def test_only_the_selected_history_can_grow_and_the_missing_project_stays_absent(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            sessions = home / "sessions"
            sessions.mkdir()
            selected = sessions / "rollout-source.jsonl"
            sibling = sessions / "rollout-destination.jsonl"
            original = home / "previous-location"
            selected.write_bytes(b"original\n")
            sibling.write_bytes(b"separate\n")
            before = account_resume_moved.histories(home)

            def verify():
                return account_resume_moved.verify_continuation(home, original, selected, before)

            for content in (b"original\n", b"replaced\nnext\n"):
                with self.subTest(content=content):
                    selected.write_bytes(content)
                    with self.assertRaisesRegex(RuntimeError, "append to the original"):
                        verify()
            selected.write_bytes(b"original\nnext\n")
            self.assertEqual(verify(), len(b"original\n"))
            sibling.write_bytes(b"changed\n")
            with self.assertRaisesRegex(RuntimeError, "other account"):
                verify()
            sibling.write_bytes(before[sibling])
            archive = home / "archived_sessions"
            archive.mkdir()
            sibling.rename(archive / sibling.name)
            with self.assertRaisesRegex(RuntimeError, "set of saved"):
                verify()
            (archive / sibling.name).rename(sibling)
            original.mkdir()
            with self.assertRaisesRegex(RuntimeError, "recreated the old project"):
                verify()
