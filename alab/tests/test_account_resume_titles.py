# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import account_resume_paged_titles as paged
import account_resume_titles as probe


class DuplicateTitles(unittest.TestCase):
    def setUp(self):
        self.rows = [
            " Resume a previous session", " Filter: All  Logins: All local   Active  Sort: Updated",
            "    1s ago      ● Release notes", "  ⌄ 2s ago      ● Release notes",
            "    │ Session:    Release notes (source-id)", "    │ Conversation:",
            "    └ Cedar marker saved.", "────────────────── 2 / 2 · 100% ─",
        ]

    def matches(self, rows):
        return probe.details(rows, "All local", 2, 2, "source-id", ("Cedar marker saved.",),
                             ("Birch workspace remembered.",))

    def test_same_title_requires_two_real_rows_and_second_row_highlight(self):
        self.assertTrue(self.matches(self.rows))
        self.assertTrue(self.matches(["Search: Release notes", *(r.replace("● ", "") for r in self.rows)]))
        for index, replacement in ((2, "    │ Release notes"), (3, "    2s ago      ● Release notes"),
                                   (7, "─────────── 1 / 2 · 100% ─"),
                                   (1, " Filter: All Logins: Current Active")):
            with self.subTest(replacement=replacement):
                rows = self.rows.copy()
                rows[index] = replacement
                self.assertFalse(self.matches(rows))
        first = self.rows.copy()
        first[2] = "  ⌄ 1s ago      ● Release notes"
        first[3] = "    2s ago      ● Release notes"
        self.assertFalse(self.matches(first))

    def test_selected_uuid_and_recorded_conversation_must_agree(self):
        for index, replacement in ((4, "    │ Session: Release notes (other-id)"),
                                   (6, "    └ Birch workspace remembered."),
                                   (6, "    └ Loading recent transcript...")):
            with self.subTest(replacement=replacement):
                rows = self.rows.copy()
                rows[index] = replacement
                self.assertFalse(self.matches(rows))
        self.assertFalse(self.matches([*self.rows, "    │ Session: Release notes (other-id)"]))
        self.assertFalse(self.matches([*self.rows, "    │ Birch workspace remembered."]))

    def test_same_title_requires_the_two_original_ids_straddling_the_cursor(self):
        a, b = Path("/a"), Path("/b")
        first = [{"id": f"a-{i}", "cwd": str(a), "source": "exec", "updatedAt": 2,
                  "name": probe.TITLE if i == 25 else "filler"} for i in range(1, 26)]
        last = [{"id": "b", "cwd": str(b), "source": "exec", "updatedAt": 1, "name": probe.TITLE}]
        owners = {row["id"] for row in first}

        def check(rows, expected_a="a-25", expected_b="b"):
            return paged.check_boundary({"data": first, "nextCursor": None}, {"data": rows, "nextCursor": "next"},
                                        {"data": last, "nextCursor": None}, owners, expected_a, expected_b, a, b)

        self.assertEqual(check(first)["named"], [(25, "a-25"), (26, "b")])
        with self.assertRaisesRegex(RuntimeError, "opposite sides"):
            check(first, expected_a="a-24")
        with self.assertRaises(RuntimeError):
            check(first, expected_b="a-25")
        with self.assertRaisesRegex(RuntimeError, "opposite sides"):
            check([*first[:-2], first[-1], first[-2]])
        with self.assertRaisesRegex(RuntimeError, "opposite sides"):
            check([dict(first[0], name=probe.TITLE), *first[1:]])
