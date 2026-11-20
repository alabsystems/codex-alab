#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Retain the controller and its Cargo receipts outside compiler-writable scratch."""
import os
from pathlib import Path
import shutil
import stat
import sys


def identity(info):
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def retain(compiled, destination):
    for name, mode in (("metadata.json", 0o400), ("artifacts.jsonl", 0o400), ("codex-release", 0o500)):
        source = compiled / name
        flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
        with os.fdopen(os.open(source, flags), "rb") as reader:
            before = os.fstat(reader.fileno())
            if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_uid != os.geteuid():
                raise ValueError(f"expected an owned regular compiler output: {name}")
            if name == "codex-release" and stat.S_IMODE(before.st_mode) != mode:
                raise ValueError("expected the Cargo-selected private executable")
            with (destination / name).open("xb") as writer:
                shutil.copyfileobj(reader, writer)
                writer.flush()
                os.fchmod(writer.fileno(), mode)
                os.fsync(writer.fileno())
            if not identity(before) == identity(os.fstat(reader.fileno())) == identity(source.lstat()):
                raise ValueError(f"compiler output changed while retaining it: {name}")


if __name__ == "__main__":
    try:
        retain(Path(sys.argv[1]), Path(sys.argv[2]))
    except (OSError, ValueError) as error:
        sys.exit(f"could not retain the release controller: {error}")
