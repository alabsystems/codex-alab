# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check the native banner on POSIX PTYs. Read the local Cargo build's JSON messages on stdin."""
import base64
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import struct
import subprocess
import sys
import termios
import time
import unicodedata


RELEASE = "0.11 on Codex 0.169"
SGR = re.compile(rb"\x1b\[[0-9;:]*m")
# One run of Kitty packets followed by the image's cursor advance.
TILE = re.compile(rb"((?:\x1b_G[^;\x1b]*;[A-Za-z0-9+/=]+\x1b\\)+)\x1b\[(\d+)C")
INLINE = {b"a": b"T", b"t": b"d", b"f": b"100", b"r": b"1", b"C": b"1", b"q": b"2"}
MARK, KITTY = "<mark>", "<kitty>"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def plain_text(frame):
    text = SGR.sub(b"", frame).replace(b"\r\n", b"\n").decode()
    require("\x1b_G" not in text and "\U0010eeee" not in text, "unconsumed graphics command or placeholder")
    require(not re.search(r"[\x00-\x09\x0b-\x1f\x7f-\x9f]", text), "unexpected terminal control in artwork rows")
    return text


def text_cells(row):
    # The fixture uses spacing glyphs and Braille, without joined emoji sequences.
    require(row.isprintable(), "unexpected nonprinting character in compact artwork")
    return sum(2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1
               for ch in row if unicodedata.category(ch) not in ("Mn", "Me"))


def build_outputs(messages):
    events = [json.loads(line) for line in messages]
    require(events and events[-1].get("reason") == "build-finished" and events[-1].get("success") is True,
            "expected a completed successful Cargo build on stdin")
    examples = [event for event in events if event["reason"] == "compiler-artifact"
                and event["target"]["kind"] == ["example"] and event["target"]["name"] == "banner_preview"
                and event.get("executable")]
    require(len(examples) == 1, "expected one banner_preview example executable")
    example = examples[0]
    outputs = {event["out_dir"] for event in events if event["reason"] == "build-script-executed"
               and event["package_id"] == example["package_id"]}
    require(len(outputs) == 1, "expected one matching banner build-script output directory")
    return Path(example["executable"]).resolve(strict=True), Path(outputs.pop()).resolve(strict=True)


def capture(binary, width, light, **settings):
    env = dict(PATH=os.defpath, TERM="xterm-256color", COLORTERM="truecolor",
               TERM_PROGRAM="aterm", TERM_PROGRAM_VERSION="0.209.0")
    env.update(settings)
    master, slave = pty.openpty()
    try:
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 18, width, 0, 0))
        args = [binary, "light"] if light else [binary]
        with subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=slave, stderr=subprocess.PIPE, env=env) as child:
            try:
                output, errors = bytearray(), bytearray()
                streams = {master: output, child.stderr.fileno(): errors}
                deadline = time.monotonic() + 15
                while True:
                    exited = child.poll() is not None
                    ready = select.select(list(streams), [], [], 0 if exited else 0.05)[0]
                    if exited and not ready:
                        break
                    for fd in ready:
                        piece = os.read(fd, 65536)
                        if piece:
                            streams[fd].extend(piece)
                        else:
                            del streams[fd]
                    require(time.monotonic() < deadline, "preview timed out")
                    require(len(output) + len(errors) <= 2**20, "unexpectedly large preview output")
                require(child.returncode == 0 and not errors, f"preview exit {child.returncode}: {errors[:400]!r}")
                return bytes(output)
            finally:
                if child.poll() is None:
                    child.kill()
    finally:
        os.close(slave)
        os.close(master)


def check_native(frame, assets, width, light):
    mark = ("codex-mark-title", 4, 2) if width == 28 else ("codex-mark", 10, 5)
    art = {MARK: mark}
    full_kitty_at = 69 if light else 65
    if width >= full_kitty_at:
        art[KITTY] = ("rainbow-kitty", 22, 4)
    elif width >= 51:
        art[KITTY] = ("rainbow-kitty-compact", 11, 2)
    check_artwork(frame, assets, light, art)


def check_artwork(frame, assets, light, art):
    counts = {name: 0 for name in art}

    def image(match):
        commands, advance = match.groups()
        columns = int(advance)
        packets = commands[:-2].split(b"\x1b\\")
        chunks = []
        for index, packet in enumerate(packets):
            controls, payload = packet[3:].split(b";", 1)
            fields = dict(field.split(b"=", 1) for field in controls.split(b",") if field)
            more = fields.pop(b"m", b"0")
            require(more == (b"1" if index + 1 < len(packets) else b"0"), "incomplete PNG transmission")
            if index == 0:
                require(int(fields.pop(b"c", b"0")) == columns, "image cursor advance disagrees with its placement")
                require(fields == INLINE, "expected an anonymous inline PNG placement")
            else:
                require(not fields, "unexpected placement in a continuation packet")
            chunks.append(payload)
        name = next((tag for tag, (_, cells, _) in art.items() if cells == columns), None)
        require(name is not None, f"unexpected {columns}-column image")
        stem, _, height = art[name]
        row = counts[name]
        require(row < height, f"extra {stem} strip")
        png = base64.b64decode(b"".join(chunks), validate=True)
        require(png == (assets / f"{stem}-{row}.png").read_bytes(), f"wrong pixels for {stem} strip {row}")
        counts[name] += 1
        return name.encode()

    text = plain_text(TILE.sub(image, frame))
    require(("YOLO mode" in text) == light, "unexpected sample palette")
    rows = text.splitlines()
    top, = [index for index, line in enumerate(rows) if "Codex ALab" in line]
    require(top + 1 < len(rows) and RELEASE in rows[top + 1], "missing release pair below title")
    right_corner = rows[top - 1].find("╮") if top else -1
    for name, (_, columns, height) in art.items():
        require(counts[name] == height, f"missing {name} strips")
        for index, line in enumerate(rows):
            expected = top <= index < top + height
            require(line.count(name) == int(expected), f"misplaced {name} on row {index}")
            if expected:
                left, right = line.split(name)
                edge = left if name == MARK else right
                require(not edge.strip(" │"), f"{name} on the wrong side of the session details")
                if name == MARK:
                    require(left == rows[top].partition(MARK)[0], "misaligned Codex mark strips")
                else:
                    require(left.endswith(" ") and right == "  │", "wrong spacing around the rainbow kitty")
                    # The sample's text and border glyphs occupy one cell each.
                    # Count the preceding Codex mark at its transmitted width.
                    start = len(left.replace(MARK, " " * art[MARK][1]))
                    require(start + columns + 2 == right_corner,
                            "rainbow kitty strip does not align with the card's right border")
    return rows[top], rows[top + 1]


def check_narrow_marks(binary, assets):
    title = ("codex-mark-title", 4, 2)
    compact = ("codex-mark-compact", 6, 3)
    full = ("codex-mark", 10, 5)
    # The light sample's longer model leaves less space beside the details.
    for width, dark_layout, light_layout in (
        (24, (None, 2), (None, 2)),
        (25, (title, 0), (title, 0)),
        (26, (title, 0), (title, 0)),
        (30, (title, 2), (title, 2)),
        (31, (compact, 0), (title, 2)),
        (32, (compact, 1), (title, 2)),
        (33, (compact, 2), (title, 2)),
        (34, (full, 2), (full, 2)),
    ):
        for light, (mark, inset) in ((False, dark_layout), (True, light_layout)):
            art = {MARK: mark} if mark else {}
            try:
                identity = check_artwork(capture(binary, width, light), assets, light, art)
                first = MARK if mark else "Codex ALab"
                require(identity[0].index(first) == inset, "unexpected left margin on the launch card")
                if mark:
                    gap = " " if width == 25 else "  "
                    for line, label in zip(identity, ("Codex ALab", RELEASE)):
                        require(line.partition(MARK)[2].startswith(gap + label), "wrong gap after the Codex mark")
                # The narrow title and release consist of ASCII plus the optional image.
                image_cells = " " * mark[1] if mark else ""
                for line in identity:
                    require(len(line.replace(MARK, image_cells).rstrip()) < width,
                            "title or release reaches the terminal's autowrap column")
            except ValueError as error:
                raise ValueError(f"{width} columns, {'light' if light else 'dark'}: {error}") from error


def main():
    binary, assets = build_outputs(sys.stdin)
    widths = (28, 50, 51, 56, 64, 65, 68, 69, 80, 120)
    for width in widths:
        for light in (False, True):
            try:
                check_native(capture(binary, width, light), assets, width, light)
            except ValueError as error:
                raise ValueError(f"{width} columns, {'light' if light else 'dark'}: {error}") from error
    for settings in ({"NO_COLOR": "1"}, {"TMUX": "fixture", "TERM": "tmux-256color"}):
        for width, light in ((51, False), (51, True), (80, False)):
            frame = capture(binary, width, light, **settings)
            require(b"\x1b_G" not in frame and "\U0010eeee".encode() not in frame, f"unexpected graphics in {settings}")
            if "NO_COLOR" in settings:
                require(b"\x1b" not in frame, "NO_COLOR preview contains terminal escapes")
            rows = plain_text(frame).split("\n")
            top, = [i for i, row in enumerate(rows) if "Codex ALab" in row]
            require(top + 1 < len(rows) and RELEASE in rows[top + 1], f"missing fallback release in {settings}")
            if width == 51:
                # At the first fit the six trail bands sit one cell after the release.
                require(rows[top].endswith("   ✧   /\\_/\\  │")
                        and rows[top + 1].endswith(RELEASE + " ▂▃▄▅▆▇(•▾•)  │"),
                        f"misplaced compact text kitty in {settings}, light={light}")
                rule = rows[top - 1] if top else ""
                require(re.fullmatch(r" *╭─+╮", rule) and len(rule) < width,
                        "missing or overflowing top border above compact text kitty")
                require(all(text_cells(row) == len(rule) for row in rows[top:top + 2]),
                        f"compact text kitty does not align with the card's right border in {settings}, light={light}")
    check_narrow_marks(binary, assets)
    print(f"Verified {2 * len(widths)} native artwork frames and six text fallback frames.")
    print("Verified sixteen narrow Codex mark layouts across both sample palettes.")


if __name__ == "__main__":
    main()
