# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Controls for the two independent bare-resume terminal processes."""

import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import model_resume
import account_resume
import account_resume_page


START, END = b"\x1b[?2026h", b"\x1b[?2026l"
HEADING = b"\x1b[1;1HResume a previous session"
TOOLBAR = b"\x1b[3;1H\x1b[2KFilter: All  Logins: All local   Active"
COUNT = "\x1b[29;1H\x1b[2K 1 / 2 · 100%".encode()
BIRCH = b"\x1b[7;17HRemember the birch in the second workspace."
# Candidate ratatui updates from terminal and exec origins position each changed word.
CEDAR = b"\x1b[8;17HRemember\x1b[8;26Hthe\x1b[8;30Hcedar"
EXEC_CEDAR = "\x1b[8;17H●\x1b[8;19HRemember\x1b[8;28Hthe\x1b[8;32Hcedar".encode()
FRAME = START + HEADING + TOOLBAR + COUNT + BIRCH + CEDAR + END


class BareResumeDefaults(unittest.TestCase):
    def test_archive_selector_requires_a_fresh_archived_view_with_only_the_source_row(self):
        active = START + HEADING + TOOLBAR + COUNT.replace(b"1 / 2", b"1 / 1") + BIRCH + END
        archived = START + TOOLBAR.replace(b"Active", b"Archived") + b"\x1b[7;1H\x1b[2K" + CEDAR + END
        read = model_resume.bare_picker_rows
        expected = dict(status="Archived", expected=["cedar"], after=len(active))
        self.assertEqual(read(active + archived, **expected), ["cedar"])
        self.assertIsNone(read(active + archived.replace(b"Archived", b"Active"), **expected))
        self.assertIsNone(read(active + archived.replace(b"\x1b[7;1H\x1b[2K", b""), **expected))
        self.assertIsNone(read(active + archived[:-len(END)], **expected))

    def test_first_login_requires_a_single_source_row_and_a_settled_empty_current_login(self):
        first = START + HEADING + TOOLBAR + COUNT.replace(b"1 / 2", b"1 / 1") + EXEC_CEDAR + END
        read = model_resume.bare_picker_rows
        self.assertEqual(read(first, expected=["cedar"]), ["cedar"])
        self.assertIsNone(read(first.replace(EXEC_CEDAR, BIRCH), expected=["cedar"]))
        self.assertIsNone(read(first.replace(b"1 / 1", b"1 / 2"), expected=["cedar"]))
        current = TOOLBAR.replace(b"All local", b"Current")
        count = COUNT.replace(b"1 / 2", b"0 / 0")
        erased = b"\x1b[8;1H\x1b[2K"
        empty = b"\x1b[7;1HNo sessions yet"
        def narrowed(frame):
            return read(first + frame, after=len(first), login="Current", expected=[])
        self.assertEqual(narrowed(START + current + count + erased + empty + END), [])
        for stale in (START + current + count + empty + END,  # A is still painted.
                      START + current + count + erased + b"\x1b[7;1HLoading sessions" + END,
                      START + TOOLBAR + count + erased + empty + END,  # Wrong login scope.
                      START + current + count + erased + empty):  # Unfinished paint.
            with self.subTest(stale=stale):
                self.assertIsNone(narrowed(stale))

    def test_only_original_exec_rollout_rejects_an_extra_archived_session_or_a_replacement(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            home = root / "codex-home"
            sessions = home / "sessions"
            sessions.mkdir(parents=True)
            original = sessions / "rollout-original.jsonl"
            def save(path, thread):
                path.write_text(json.dumps({"type": "session_meta", "payload": {
                    "id": thread, "cwd": str(root / "work"), "source": "exec"}}) + "\n")
            save(original, "source")
            self.assertEqual(account_resume.only_seed_rollout(root, "source"), "sessions/rollout-original.jsonl")
            archive = home / "archived_sessions"
            archive.mkdir()
            extra = archive / "rollout-sibling.jsonl"
            save(extra, "sibling")
            with self.assertRaisesRegex(RuntimeError, "expected only"):
                account_resume.only_seed_rollout(root, "source")
            extra.unlink()
            save(original, "sibling")
            with self.assertRaisesRegex(RuntimeError, "sole rollout"):
                account_resume.only_seed_rollout(root, "source")

    def test_second_page_requires_the_complete_newer_destination_and_original_source_sets(self):
        original, destination = Path('/cedar'), Path('/birch')
        rows = [{'id': f'b{n}', 'cwd': str(destination), 'source': 'appServer', 'updatedAt': 12}
                for n in range(25)]
        source = {'id': 'a', 'cwd': str(original), 'source': 'exec', 'updatedAt': 11}
        owners = {row['id'] for row in rows}
        current = {'data': rows, 'nextCursor': None}
        first = {'data': rows, 'nextCursor': 'cursor-1'}
        second = {'data': [source], 'nextCursor': None}
        check = account_resume_page.check_pages
        self.assertEqual(check(owners, 'a', original, destination, current, first, second)['second_page_ids'], ['a'])
        for own, recent, last in [
            (dict(current, data=[source, *rows[1:]]), first, second),
            (current, dict(first, data=[source, *rows[1:]]), second),
            (current, first, dict(second, data=[rows[0]])),
            (current, dict(first, data=[dict(rows[0], updatedAt=11), *rows[1:]]), second),
        ]:
            with self.subTest(own=own['data'][0]['id'], recent=recent['data'][0], last=last['data']):
                with self.assertRaises(RuntimeError):
                    check(owners, 'a', original, destination, own, recent, last)

    def test_scrolled_position_requires_a_current_completed_all_local_picker(self):
        initial = START + HEADING + TOOLBAR + BIRCH + COUNT.replace(b' / 2', b' / 25') + END
        final = START + CEDAR + COUNT.replace(b'1 / 2', b'26 / 26') + END
        read = account_resume_page.picker_position
        self.assertEqual(read(initial), (1, 25, True, False))
        self.assertEqual(read(initial + final, after=len(initial)), (26, 26, True, True))
        self.assertIsNone(read(initial + final[:-len(END)], after=len(initial)))
        self.assertIsNone(read(initial + final, after=len(initial + final)))
        self.assertIsNone(read((initial + final).replace(b'All local', b'Current  ')))

    def test_search_beyond_page_one_requires_the_typed_query_and_only_the_original_row(self):
        initial = START + HEADING + TOOLBAR + BIRCH + COUNT.replace(b' / 2', b' / 25') + END
        search = b"\x1b[4;1HSearch: cedar"
        clear_birch = b"\x1b[7;1H\x1b[2K"
        final = START + search + clear_birch + CEDAR + COUNT.replace(b' / 2', b' / 1') + END
        read = account_resume_page.picker_position
        kwargs = dict(after=len(initial), query=account_resume_page.OLDER_QUERY)
        self.assertEqual(read(initial + final, **kwargs), (1, 1, False, True))
        self.assertIsNone(read(initial + final.replace(search, b""), **kwargs))
        self.assertIsNone(read(initial + final[:-len(END)], **kwargs))
        self.assertIsNone(read(initial + final.replace(b"Search: cedar", b"Search: birch"), **kwargs))
        self.assertEqual(read(initial + final.replace(clear_birch, b""), **kwargs), (1, 1, True, True))

    def test_distinct_rows_in_settled_picker_with_positioned_changes(self):
        read = model_resume.bare_picker_rows
        for title in (CEDAR, EXEC_CEDAR):
            with self.subTest(title=title):
                earlier = START + HEADING + TOOLBAR + COUNT + BIRCH + END
                self.assertEqual(read(earlier + START + title + END), ["birch", "cedar"])
                self.assertIsNone(read(earlier + START + title))
        swapped = FRAME.replace(b"[7;", b"[9;")
        self.assertEqual(read(swapped), ["cedar", "birch"])

    def test_rejects_stale_partial_overwritten_and_nonpicker_output(self):
        read = model_resume.bare_picker_rows
        self.assertIsNone(read(FRAME, after=len(FRAME)))
        self.assertIsNone(read(FRAME + START + b"\x1b[8;1H\x1b[2K" + END))
        self.assertIsNone(read(FRAME + START + b"\x1b[8;17H        " + END))
        self.assertIsNone(read(FRAME + START + b"\x1b[2J" + END))
        self.assertIsNone(read(FRAME.replace(HEADING, b"")))
        self.assertIsNone(read(FRAME.replace(COUNT, b"")))
        self.assertIsNone(read(FRAME.replace(b"All local", b"Current")))
        self.assertIsNone(read(FRAME.replace(CEDAR, b"\x1b[7;59HRemember the cedar")))
        self.assertIsNone(read(FRAME.replace(CEDAR, b"\x1b]0;Remember the cedar\x07")))
        self.assertIsNone(read(FRAME.replace(START, b"")))
        self.assertIsNone(read(FRAME + END, after=len(FRAME)))

    def test_resumed_history_survives_positioned_paint_and_cursor_style(self):
        saved = b"\x1b[0 q\x1b[16;3HCedar\x1b[16;9Hmarker\x1b[16;16Hsaved."
        raw = FRAME + START + b"\x1b[2J" + saved
        lines = model_resume.visible_lines
        self.assertEqual(lines(raw, after=len(FRAME)), [])
        self.assertEqual(lines(raw + END, after=len(FRAME))[15].strip(), "Cedar marker saved.")
        erased = raw + END + START + b"\x1b[16;1H\x1b[2K" + END
        self.assertNotIn("Cedar marker saved.", "\n".join(lines(erased)))

    def test_alternate_scroll_query_preserves_painted_rows_and_cursor(self):
        query = b"\x1b[?1007$p"
        read = model_resume.visible_lines
        self.assertEqual(read(query + FRAME.replace(b"cedar", b"ce" + query + b"dar")), read(FRAME))
        with self.assertRaisesRegex(ValueError, "unsupported picker screen operation"):
            read(query.replace(b"$", b"") + FRAME)

    def test_narrow_scopes_must_show_only_birch_after_erase(self):
        read = model_resume.bare_picker_rows
        for directory, login in (("Cwd", "All local"), ("All", "Current")):
            with self.subTest(directory=directory, login=login):
                scope = f"\x1b[3;1H\x1b[2KFilter: {directory}  Logins: {login}   Active".encode()
                changed = START + scope + COUNT.replace(b"/ 2", b"/ 1")
                def narrowed(update):
                    return read(FRAME + update, after=len(FRAME), directory=directory, login=login)
                self.assertIsNone(narrowed(changed + END))
                self.assertIsNone(narrowed(changed + b"\x1b[8;1H\x1b[K"))
                self.assertEqual(narrowed(changed + b"\x1b[8;1H\x1b[K" + END), ["birch"])

    def test_initial_selection_and_narrow_controls_use_literal_separate_invocations(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "tmp").mkdir()
            other = root / "second-project"
            other.mkdir()

            def observe(_python, args, _root, _env, label, *, interact):
                spec = json.loads(Path(args[-1]).read_text())
                self.assertEqual(spec["argv"], ["/fixture/codex", "resume"])
                self.assertEqual(spec["cwd"], str(other))
                self.assertEqual(Path(spec["raw"]), root / (label + ".raw"))
                Path(spec["result"]).write_text(json.dumps({"label": label}))
                return 0

            with patch.object(model_resume.fixture, "run", side_effect=observe):
                for surface, label in [("bare-all-default", "terminal"), ("narrow-bare-all-default", "narrow-picker")]:
                    result, _, _ = model_resume.terminal_session(
                        Path("/fixture/codex"), root, {}, surface, ["resume"], None, [], cwd=other)
                    self.assertEqual(result, {"label": label})
                with self.assertRaisesRegex(RuntimeError, "literal codex resume"):
                    model_resume.terminal_session(Path("/fixture/codex"), root, {}, "bare-all-default",
                                                  ["resume", "--all"], None, [], cwd=other)
