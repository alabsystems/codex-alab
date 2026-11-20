# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check the banner_preview executable through its plain-text stdout interface."""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import unittest


MODES = {"cruise": "∞", "land": "⤓", "off": "○"}
RELEASE = "0.11 on Codex 0.169"


class BannerPreviewTests(unittest.TestCase):
    def run_preview(self, *args):
        return subprocess.run(
            [self.preview, *args], stdin=subprocess.DEVNULL, capture_output=True,
            encoding="utf-8", timeout=15,
        )

    def output(self, *args):
        result = self.run_preview(*args)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        return result.stdout

    def test_card_and_switch_select_the_requested_mode(self):
        for palette in ((), ("light",)):
            for mode, glyph in MODES.items():
                with self.subTest(palette=palette, mode=mode):
                    frame = self.output("56", *palette, "--mode", mode)
                    self.assertNotIn("\x1b", frame)
                    card, switch = frame.rstrip().rsplit("\n", 1)
                    self.assertIn(RELEASE, card)
                    self.assertIn(RELEASE, switch)
                    self.assertRegex(card, rf"Autopilot\s+{glyph} {mode}\b")
                    self.assertEqual("YOLO mode" in card, bool(palette))
                    self.assertRegex(switch, r"Cruise.*Land.*Off")
                    selected = [name for name in MODES if f"[{name.title()}]" in switch]
                    self.assertEqual(selected, [mode], switch)

    def test_four_columns_keep_the_complete_upstream_version(self):
        self.assertIn("\nCod…\n0.16\n9\n", self.output("4"))

    def test_narrow_card_keeps_the_release_and_fits_the_requested_mode(self):
        for mode, glyph in MODES.items():
            full = f"{glyph} {mode}"
            for width, label in (
                (0, None), (1, glyph), (len(mode) - 1, glyph), (len(mode), mode),
                (len(full) - 1, mode), (len(full), full), (12, full), (13, full),
            ):
                with self.subTest(width=width, mode=mode):
                    card = self.output(str(width), "--mode", mode).partition("\n\n")[0]
                    rows = [line.strip() for line in card.splitlines()]
                    if width == 0:
                        self.assertEqual(rows, [])
                        continue
                    self.assertEqual(rows[-1], label)
                    self.assertNotIn(label, rows[:-1])
                    identity = "".join(rows[:-1])
                    self.assertIn("0.11", identity)
                    self.assertIn("0.169", identity)

    def test_piped_defaults_match_eighty_columns_and_land(self):
        for palette in ((), ("light",)):
            with self.subTest(palette=palette):
                self.assertEqual(self.output(*palette), self.output("80", *palette, "--mode=land"))

    def test_closed_output_pipe_exits_quietly(self):
        reader, writer = os.pipe()
        with os.fdopen(writer, "wb") as pipe:
            os.close(reader)
            result = subprocess.run(
                [self.preview, "56", "--mode", "cruise"], stdin=subprocess.DEVNULL,
                stdout=pipe, stderr=subprocess.PIPE, encoding="utf-8", timeout=15,
            )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")

    def test_help_lists_modes(self):
        help_text = self.output("--help")
        for word in ("--mode", *MODES):
            self.assertIn(word, help_text)
        self.assertNotIn(RELEASE, help_text)

    def test_invalid_modes_exit_before_drawing(self):
        for args in (("--mode",), ("--mode", "unknown"), ("--mode", "land", "--mode", "off")):
            with self.subTest(args=args):
                result = self.run_preview(*args)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn("--mode", result.stderr)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("preview", type=Path, help="path to the built banner_preview executable")
    BannerPreviewTests.preview = parser.parse_args().preview.resolve(strict=True)
    unittest.main(argv=[sys.argv[0]])
