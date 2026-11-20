# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0

from contextlib import closing
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
from account_resume_restored import drop_source, index_state


class RestoredRollout(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name).resolve()
        home = self.root / "codex-home"
        (home / "sessions").mkdir(parents=True)
        self.database = home / "state_5.sqlite"
        with closing(sqlite3.connect(self.database)) as db, db:
            db.execute("CREATE TABLE backfill_state (id INTEGER PRIMARY KEY, status TEXT)")
            db.execute("INSERT INTO backfill_state VALUES (1, 'complete')")
            db.execute("CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, source TEXT, cwd TEXT)")
            for thread, cwd in (("a", "work"), ("b", "second-project")):
                path = home / "sessions" / f"rollout-test-{thread}.jsonl"
                path.write_text(thread)
                db.execute("INSERT INTO threads VALUES (?, ?, 'exec', ?)", (thread, str(path), str(self.root / cwd)))

    def test_drop_source_only_in_the_expected_fixture(self):
        other = self.root / "other"
        (other / "codex-home").mkdir(parents=True)
        with self.assertRaisesRegex(RuntimeError, "disposable fixture"):
            drop_source(other, "a", "b", self.database)
        with self.assertRaisesRegex(RuntimeError, "two indexed"):
            drop_source(self.root, "b", "missing", self.database)
        self.assertEqual(index_state(self.database)["thread_ids"], ["a", "b"])
        drop_source(self.root, "a", "b", self.database)
        self.assertEqual(index_state(self.database), {"thread_ids": ["b"], "backfill": "complete",
                                                     "integrity": "ok", "foreign_key_violations": 0})
        rollouts = sorted(self.database.parent.glob("sessions/*.jsonl"))
        self.assertEqual([path.read_text() for path in rollouts], ["a", "b"])

    def test_incomplete_backfill_is_a_different_recovery_case(self):
        with closing(sqlite3.connect(self.database)) as db, db:
            db.execute("UPDATE backfill_state SET status = 'pending'")
        with self.assertRaisesRegex(RuntimeError, "backfill has not finished"):
            index_state(self.database)
