# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Drive a CLI through the history smoke's supervised PTY."""

import base64
import json
from pathlib import Path
import re
import subprocess
import sys
import time
import unicodedata

import capacity_exec as fixture
import model_resume


WAIT = 25
SYNC_END = b"\x1b[?2026l"


def read_runs(raw, offset):
    if not raw.exists():
        return b""
    with raw.open("rb") as stream:
        stream.seek(offset)
        data = stream.read()
    # Diff rendering can use cursor motion to skip an unchanged blank cell.
    return b"".join(re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", data).split())


def painted_rows(data, width):
    """Read cursor-addressed Ratatui cells from this fresh post-resize paint."""
    screen = [[" "] * width for _ in range(32)]
    row = col = 0
    text = data.decode("utf8", "replace")
    tokens = re.compile(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b\[[0-?]*[ -/]*[@-~]|\x1b.|.", re.S)
    for token in tokens.finditer(text):
        value = token.group()
        if value.startswith("\x1b["):
            args, cmd = value[2:-1], value[-1]
            if cmd in ("H", "f"):
                y, _, x = args.partition(";")
                row, col = int(y or 1) - 1, int(x or 1) - 1
            elif cmd == "K" and args in ("", "0") and 0 <= row < 32 and 0 <= col < width:
                screen[row][col:] = [" "] * (width - col)
            elif cmd == "J" and args == "2":
                screen = [[" "] * width for _ in range(32)]
            continue
        if len(value) != 1 or value < " ":
            if value == "\r":
                col = 0
            elif value == "\n":
                row += 1
            continue
        if 0 <= row < 32 and 0 <= col < width:
            screen[row][col] = value
        col += 2 if unicodedata.east_asian_width(value) in ("W", "F") else 1
    return ["".join(line) for line in screen]


class Terminal:
    def __init__(self, process, raw):
        self.process = process
        self.raw = raw
        self.index = 0

    def command(self, **fields):
        self.index += 1
        self.process.stdin.write((json.dumps({"id": self.index, **fields}) + "\n").encode())
        self.process.stdin.flush()

    def send(self, keys):
        self.command(write=base64.b64encode(keys).decode())

    def displayed(self, *markers, offset=0):
        end = time.monotonic() + WAIT
        while time.monotonic() < end and self.process.poll() is None:
            runs = read_runs(self.raw, offset)
            if all(b"".join(marker.encode().split()) in runs for marker in markers):
                return
            time.sleep(.04)
        raise RuntimeError(f"terminal did not paint {markers!r} after byte {offset}")

    def paste(self, value, marker):
        offset = self.raw.stat().st_size
        self.send(b"\x1b[200~" + value.encode() + b"\x1b[201~")
        self.displayed(marker, offset=offset)

    def frame(self, width, observe, description):
        offset = self.raw.stat().st_size
        self.command(resize=[32, width])
        end = time.monotonic() + WAIT
        while time.monotonic() < end and self.process.poll() is None:
            with self.raw.open("rb") as stream:
                stream.seek(offset)
                data = stream.read()
            boundary = data.rfind(SYNC_END)
            if boundary >= 0:
                data = data[:boundary + len(SYNC_END)]
                rendered = observe(painted_rows(data, width))
                if rendered:
                    return {"start": offset, "end": offset + len(data), "width": width, "rendered": rendered}
            time.sleep(.04)
        raise RuntimeError(f"fresh frame at {width} columns omitted {description}")

    def quit(self):
        for seconds in (1, 12):
            self.send(b"\x03")
            try:
                self.process.wait(timeout=seconds)
                return
            except subprocess.TimeoutExpired:
                if seconds == 12:
                    raise


def run_pty(cli, root, env, label, exercise, *, cwd=None):
    raw, custody = root / (label + ".raw"), root / (label + "-custody.json")
    spec = root / "tmp" / (label + "-spec.json")
    spec.write_text(json.dumps({"argv": cli, "cwd": str(cwd or root / "work"), "env": env,
                                "raw": str(raw), "result": str(custody)}))

    @model_resume.with_pty_lifeline
    def interact(process, _log):
        exercise(Terminal(process, raw))

    command = ["-I", "-B", "-S", str(Path(__file__).with_name("history_pagination.py")), "--guardian", str(spec)]
    status = fixture.run(sys.executable, command, root, env, label, interact=interact)
    return status, json.loads(custody.read_text())


def drive_pty(cli, root, env, label, exercise, *, cwd=None):
    status, custody = run_pty(cli, root, env, label, exercise, cwd=cwd)
    fixture.require(status == 0, "terminal custody failed")
    model_resume.CHECK["check_model_terminal_exit"](custody, cli)
