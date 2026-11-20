# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import protocol_export_merge as merge


class ProtocolMergeTests(unittest.TestCase):
    def test_disjoint_schema_changes_and_removed_fields(self):
        base = {"old": True, "keep": 1}
        ours = {"keep": 1, "fork": {}}
        theirs = {"old": True, "keep": 2}
        self.assertEqual(list(merge.differences(base, ours, theirs, {"keep": 2, "fork": {}})), [])
        issues = list(merge.differences(base, ours, theirs, {"old": True, "keep": 2}))
        self.assertEqual([path for path, _ in issues], [("fork",), ("old",)])

    def test_same_value_edited_twice_needs_review_even_if_it_matches_one_parent(self):
        for base, ours, theirs in [(None, "A", "B"), ([1], [1, 2], [3, 1]),
                                   ({"x": True}, merge.MISSING, {"x": False})]:
            with self.subTest(base=base):
                issues = list(merge.differences(base, ours, theirs, theirs))
                self.assertIn("both parents", issues[0][1])

    def test_false_zero_and_missing_null_are_distinct_inside_lists_and_objects(self):
        values = [{"k": [False]}, {"k": [0]}, {"k": None}, {}]
        for index, value in enumerate(values):
            for other in values[index + 1:]:
                self.assertTrue(list(merge.differences(value, value, value, other)))

    def test_reads_actual_unmerged_index_and_checks_every_export_group(self):
        def archive(ts, public, internal):
            return json.dumps({"typescript": ts, "json_schema": {"p.json": json.dumps(public)},
                               "internal_json_schema": {"i.json": json.dumps(internal)}}).encode()

        with tempfile.TemporaryDirectory() as folder, patch.object(merge, "decompress", lambda data: data):
            repo = Path(folder)
            subprocess.run(["git", "init", "-q", repo], check=True)
            rows = []
            for stage, data in enumerate([archive({"old.ts": "a"}, {}, {}), archive({}, {"a": 1}, {}),
                                          archive({"old.ts": "a"}, {"b": 2}, {"added": True})], start=1):
                blob = subprocess.check_output(["git", "-C", repo, "hash-object", "-w", "--stdin"],
                                               input=data).decode().strip()
                rows.append(f"100644 {blob} {stage}\t{merge.EXPORTS}\n")
            subprocess.run(["git", "-C", repo, "update-index", "--index-info"],
                           input="".join(rows).encode(), check=True)
            output = repo / Path(merge.EXPORTS).name
            output.write_bytes(archive({}, {"b": 2, "a": 1}, {"added": True}))
            self.assertEqual(merge.compare(repo, output)["issues"], [])
            output.write_bytes(archive({"old.ts": "a"}, {"b": 2}, {}))
            paths = {tuple(item["path"]) for item in merge.compare(repo, output)["issues"]}
            self.assertEqual(paths, {("typescript", "old.ts"), ("json_schema", "p.json", "a"),
                                     ("internal_json_schema", "i.json")})
