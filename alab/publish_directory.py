#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Publish a staged package directory only while its destination is absent."""

import argparse
import ctypes
import errno
import os
from pathlib import Path
import stat
import sys


# Darwin <sys/stdio.h>: RENAME_EXCL; rename(2) requires EEXIST for an existing target.
RENAME_EXCL = 0x00000004


def publish_directory(staged, published):
    staged, published = Path(staged), Path(published)
    if sys.platform != "darwin":
        raise OSError(errno.ENOTSUP, "exclusive package publication requires macOS")
    if not stat.S_ISDIR(staged.lstat().st_mode):
        raise ValueError("the staged package must be a real directory")
    try:
        rename = ctypes.CDLL(None, use_errno=True).renamex_np
    except AttributeError as error:
        raise OSError(errno.ENOTSUP, "renamex_np is unavailable; package kept") from error
    rename.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int
    if rename(os.fsencode(staged), os.fsencode(published), RENAME_EXCL) != 0:
        code = ctypes.get_errno() or errno.EIO
        raise OSError(code, os.strerror(code), str(published))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--staged", type=Path, required=True)
    parser.add_argument("--published", type=Path, required=True)
    args = parser.parse_args()
    try:
        publish_directory(args.staged, args.published)
    except (OSError, ValueError) as error:
        print(f"alab: package publication refused; staging kept: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
