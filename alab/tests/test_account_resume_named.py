# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0

from copy import deepcopy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import account_resume_named as named


class NamedResumeAmbiguity(unittest.TestCase):
    def setUp(self):
        self.a = "11111111-1111-4111-8111-111111111111"
        self.b = "22222222-2222-4222-8222-222222222222"
        title = named.TITLE
        argv = ["codex", "resume", title, named.terminal.BIRCH_CONTINUE]
        self.case = {"argv": argv, "requests": [], "before": {"a": "shaA", "b": "shaB"},
                     "after": {"a": "shaA", "b": "shaB"}, "observer_quit": False, "guardian_status": 1,
                     "custody": {"command": argv, "exit": 1, "owning_reap": True,
                                 "group_absent_after_reap": True, "forced": False},
                     "terminal_text": f"Multiple sessions match '{title}' (including {self.b} and {self.a});"
                                      " use a session UUID to disambiguate."}

    def check(self, case):
        named.check_collision(case, named.TITLE, [self.a, self.b])

    def test_error_in_either_order_and_wrapped_by_a_terminal(self):
        self.check(self.case)
        self.case["terminal_text"] = self.case["terminal_text"].replace(f"{self.b} and {self.a}",
                                                                      f"{self.a} and\r\n {self.b}")
        self.check(self.case)

    def test_rejects_wrong_ids_titles_and_a_bare_uuid_list(self):
        initial = self.case["terminal_text"]
        for text in [initial.replace(self.b, self.a), initial.replace(named.TITLE, "other"),
                     f"{named.TITLE} {self.a} {self.b}", initial.replace("disambiguate.", "continue.")]:
            with self.subTest(text=text), self.assertRaises(RuntimeError):
                self.check(dict(self.case, terminal_text=text))

    def test_a_real_rejection_needs_zero_requests_untouched_history_and_natural_exit(self):
        for update in ({"requests": [{"history": []}]}, {"after": {"a": "shaA"}},
                       {"after": {"a": "shaA", "b": "changed"}}, {"observer_quit": True},
                       {"guardian_status": 0}):
            with self.subTest(update=update), self.assertRaises(RuntimeError):
                self.check(dict(self.case, **update))
        for update in ({"forced": True}, {"group_absent_after_reap": False}, {"owning_reap": False},
                       {"command": ["codex", "resume"]}, {"exit": 0}, {"exit": True}):
            with self.subTest(update=update), self.assertRaises(RuntimeError):
                case = deepcopy(self.case)
                case["custody"].update(update)
                self.check(case)


class NamedResumePage(unittest.TestCase):
    def setUp(self):
        self.title, self.limit = named.TITLE, named.page.LIMIT
        self.a, self.b = "older-exec", "newer-editor"
        self.fillers = [{"id": f"fork-{i}", "source": "vscode", "name": f"{self.title} draft {i}"}
                        for i in range(self.limit - 1)]
        self.first = {"data": [*self.fillers, {"id": self.b, "source": "vscode", "name": self.title}],
                      "nextCursor": "timestamp|editor"}
        self.last = {"data": [{"id": self.a, "source": "exec", "name": self.title}], "nextCursor": None}
        self.expected = {row["id"] for row in self.first["data"] + self.last["data"]}

    def check(self, first, last, b_title=None):
        return named.page.check_pages(first, last, self.expected, self.a, self.b, b_title or self.title)

    def test_only_the_declared_exact_labels_cross_the_real_page_boundary(self):
        self.assertEqual(self.check(self.first, self.last)["exact_positions"],
                         [(self.limit, self.b), (self.limit + 1, self.a)])
        self.first["data"][-1]["name"] = named.page.NEAR_TITLE
        self.assertEqual(self.check(self.first, self.last, named.page.NEAR_TITLE)["exact_positions"],
                         [(self.limit + 1, self.a)])

    def test_swapped_boundary_another_exact_match_missing_term_or_repeated_id_fails(self):
        for mutation in ("swapped", "third", "term", "duplicate", "more"):
            with self.subTest(mutation=mutation), self.assertRaises(RuntimeError):
                first, last = deepcopy(self.first), deepcopy(self.last)
                if mutation == "swapped":
                    first["data"][-1], last["data"][0] = last["data"][0], first["data"][-1]
                elif mutation == "third":
                    first["data"][0]["name"] = self.title
                elif mutation == "term":
                    first["data"][0]["name"] = "unrelated"
                elif mutation == "duplicate":
                    first["data"][0] = first["data"][1]
                else:
                    last["nextCursor"] = "another|position"
                self.check(first, last)
