#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Copy package bytes and modes into empty staging without following input links."""

import argparse
import errno
import os
from pathlib import Path
import stat
import sys


DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
FILE_FLAGS = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC


def checked_mode(info):
    if not (stat.S_ISREG(info.st_mode) or stat.S_ISDIR(info.st_mode)):
        raise ValueError("a package may contain only regular files and directories")
    if info.st_mode & 0o7022:
        raise ValueError("package mode has special or group/world write permissions")
    return stat.S_IMODE(info.st_mode)


def same_entry(before, after):
    if (before.st_dev, before.st_ino, stat.S_IFMT(before.st_mode)) != (
            after.st_dev, after.st_ino, stat.S_IFMT(after.st_mode)):
        raise ValueError("a package entry changed while it was opened")
    checked_mode(after)


def copy_entries(source, destination, staging_identity):
    # All descendants are opened relative to already held directory descriptors.
    # A rename cannot redirect traversal, and O_NOFOLLOW rejects a substituted link.
    with os.scandir(source) as entries:
        for entry in entries:
            before = entry.stat(follow_symlinks=False)
            checked_mode(before)
            directory = stat.S_ISDIR(before.st_mode)
            source_fd = os.open(entry.name, DIRECTORY_FLAGS if directory else FILE_FLAGS,
                                dir_fd=source)
            try:
                after = os.fstat(source_fd)
                same_entry(before, after)
                if directory:
                    if (after.st_dev, after.st_ino) == staging_identity:
                        raise ValueError("staging must be outside the source directory")
                    os.mkdir(entry.name, 0o700, dir_fd=destination)
                    destination_fd = os.open(entry.name, DIRECTORY_FLAGS, dir_fd=destination)
                    try:
                        copy_entries(source_fd, destination_fd, staging_identity)
                        os.fchmod(destination_fd, checked_mode(after))
                    finally:
                        os.close(destination_fd)
                else:
                    destination_fd = os.open(
                        entry.name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
                        | os.O_CLOEXEC, 0o600, dir_fd=destination)
                    try:
                        left = after.st_size
                        while left:
                            chunk = os.read(source_fd, min(left, 1024 * 1024))
                            if not chunk:
                                raise ValueError("a package file shrank while being copied")
                            left -= len(chunk)
                            remaining = memoryview(chunk)
                            while remaining:
                                written = os.write(destination_fd, remaining)
                                if written == 0:
                                    raise OSError(errno.EIO, "package copy made no progress")
                                remaining = remaining[written:]
                        final = os.fstat(source_fd)
                        if os.read(source_fd, 1) or (
                                after.st_size, after.st_mtime_ns, after.st_ctime_ns, after.st_mode) != (
                                final.st_size, final.st_mtime_ns, final.st_ctime_ns, final.st_mode):
                            raise ValueError("a package file changed while being copied")
                        os.fchmod(destination_fd, checked_mode(final))
                    finally:
                        os.close(destination_fd)
            finally:
                os.close(source_fd)


def copy_package(source, destination):
    # Path strips a trailing '/.': it must not turn a final input link into traversal.
    source_fd = os.open(Path(source), DIRECTORY_FLAGS)
    try:
        checked_mode(os.fstat(source_fd))
        destination_fd = os.open(Path(destination), DIRECTORY_FLAGS)
        try:
            destination_info = os.fstat(destination_fd)
            staging_identity = (destination_info.st_dev, destination_info.st_ino)
            source_info = os.fstat(source_fd)
            if (source_info.st_dev, source_info.st_ino) == staging_identity:
                raise ValueError("source and staging must be different directories")
            if os.listdir(destination_fd):
                raise ValueError("the staging directory must be empty")
            copy_entries(source_fd, destination_fd, staging_identity)
            os.fchmod(destination_fd, 0o755)
        finally:
            os.close(destination_fd)
    finally:
        os.close(source_fd)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    try:
        copy_package(args.source, args.destination)
    except (OSError, ValueError) as error:
        print(f"alab: package copy refused: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
