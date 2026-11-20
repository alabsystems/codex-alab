#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Bound the scheduled sync's live capture and its retained journal."""

from collections import deque
from datetime import datetime
import os
from pathlib import Path
import stat
import sys
import tempfile


MAX_BYTES = 4 * 1024 * 1024
MAX_LINES = 20_000
CHUNK = 64 * 1024
OMITTED = b"[earlier output from this run omitted]\n"


def capture(source, destination):
    # Buffered reads fill a chunk unless at EOF. Keep the extra chunk for a partial final read.
    chunks = deque(iter(lambda: source.read(CHUNK), b""), maxlen=MAX_BYTES // CHUNK + 1)
    destination.write(b"".join(chunks)[-MAX_BYTES:])


def last_lines(data, limit):
    end = len(data) - int(data.endswith(b"\n"))
    for _ in range(limit):
        end = data.rfind(b"\n", 0, end)
        if end < 0:
            return data
    return data[end + 1:]


def entry(output, status):
    now = datetime.now().astimezone().strftime("%Y-%m-%dT%H:%M:%S%z")
    header = f"== {now} alab/daily.sh: exit {status}\n".encode()
    room = MAX_BYTES - len(header)
    lines = output.count(b"\n") + int(bool(output) and not output.endswith(b"\n"))
    if len(output) <= room and lines < MAX_LINES:
        return header + output
    return header + OMITTED + last_lines(output[-(room - len(OMITTED)):], MAX_LINES - 2)


def publish(path, record):
    # Read only the part of the previous log that can survive byte retention.
    room = MAX_BYTES - len(record)
    try:
        old = os.fdopen(os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW), "rb")
    except FileNotFoundError:
        previous = b""
        mask = os.umask(0)
        os.umask(mask)
        mode = 0o666 & ~mask
    else:
        with old:
            info = os.fstat(old.fileno())
            if not stat.S_ISREG(info.st_mode):
                raise ValueError("daily sync journal must be a regular file")
            mode = stat.S_IMODE(info.st_mode) & 0o777
            old.seek(max(0, info.st_size - room))
            previous = old.read(room)
    if previous and not previous.endswith(b"\n"):
        previous = (previous + b"\n")[-room:]
    journal = last_lines(previous + record, MAX_LINES)
    staged = tempfile.NamedTemporaryFile(dir=path.parent, prefix=".alab-daily-log-", delete=False)
    try:
        with staged:
            os.fchmod(staged.fileno(), mode)
            staged.write(journal)
            staged.flush()
            os.fsync(staged.fileno())
        os.replace(staged.name, path)
    except BaseException:
        Path(staged.name).unlink(missing_ok=True)
        raise


def main(args):
    if args == ["capture"]:
        capture(sys.stdin.buffer, sys.stdout.buffer)
    elif len(args) == 4 and args[0] == "publish":
        with open(args[2], "rb") as captured:
            output = captured.read(MAX_BYTES + 1)
        if len(output) > MAX_BYTES:
            raise ValueError("daily capture exceeded its storage limit")
        publish(Path(args[1]), entry(output, int(args[3])))
    else:
        raise ValueError("expected capture or publish <log> <capture> <status>")


if __name__ == "__main__":
    try:
        main(sys.argv[1:])
    except (OSError, ValueError) as error:
        sys.exit(f"alab: could not record daily sync output: {error}")
