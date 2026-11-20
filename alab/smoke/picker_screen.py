# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Read the last completed picker paint from the smoke guardian's 32-by-76 PTY."""

import re
import unicodedata

SYNC_START = b"\x1b[?2026h"
SYNC_END = b"\x1b[?2026l"
CSI = re.compile(r"\x1b\[([0-?]*)([ -/]*)([@-~])")
OSC = re.compile(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")


def visible_lines(raw, after=0):
    """Replay all updates to the latest synchronized boundary newer than after."""
    last = raw.rfind(SYNC_END)
    if last < after or raw.rfind(SYNC_START, 0, last) <= raw.rfind(SYNC_END, 0, last):
        return []
    text = raw[:last + len(SYNC_END)].decode("utf-8")
    rows, cols = 32, 76
    cells = [list(" " * cols) for _ in range(rows)]
    row = col = index = 0
    while index < len(text):
        char = text[index]
        if char == "\x1b":
            osc = OSC.match(text, index)
            if osc:
                index = osc.end()
                continue
            csi = CSI.match(text, index)
            if not csi:
                raise ValueError("unsupported escape in captured picker")
            args, intermediate, op = csi.groups()
            index = csi.end()
            if op in "hl" and args == "?1049":
                cells = [list(" " * cols) for _ in range(rows)]
                row = col = 0
            elif (op in "mcn"
                  or (args, intermediate, op) == ("?1007", "$", "p")
                  or op in "hl" and args in ("?25", "?1000", "?1002", "?1003", "?1004", "?1006",
                                              "?1007", "?2004", "?2026")
                  or op == "q" and intermediate == " " and args in ("", "0", "1", "2", "3", "4", "5", "6")
                  or op == "u" and args.startswith(("?", "<", ">"))):
                # Attributes, terminal input modes and device queries do not write cells.
                pass
            elif op in "Hf" and not intermediate:
                position = args.split(";")
                row = min(max(int(position[0] or 1), 1), rows) - 1
                col = min(max(int(position[1] or 1) if len(position) > 1 else 1, 1), cols) - 1
            elif (op in "JK" and args in ("", "0", "1", "2")) or (op == "J" and args == "3"):
                if op == "J" and args == "3":  # discard scrollback
                    continue
                start = (0 if op == "J" else row * cols) if args in ("1", "2") else row * cols + col
                stop = row * cols + col + 1 if args == "1" else (rows if op == "J" else row + 1) * cols
                for cell in range(start, stop):
                    cells[cell // cols][cell % cols] = " "
            else:
                raise ValueError(f"unsupported picker screen operation {csi.group()!r}")
            continue
        index += 1
        if char == "\r":
            col = 0
        elif char == "\n":
            row += 1
            if row == rows:
                cells.pop(0)
                cells.append(list(" " * cols))
                row -= 1
        elif char == "\b":
            col = max(0, col - 1)
        elif char == "\t":
            col = min(cols - 1, (col // 8 + 1) * 8)
        elif char.isprintable() and not unicodedata.combining(char):
            if col >= cols:
                # The picker uses absolute positioning at wrap boundaries.
                raise ValueError("unexpected automatic wrap in captured picker")
            width = 2 if unicodedata.east_asian_width(char) in ("W", "F") else 1
            cells[row][col] = char
            if width == 2 and col + 1 < cols:
                cells[row][col + 1] = ""
            col += width
    return ["".join(line) for line in cells]
