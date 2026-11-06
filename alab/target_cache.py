#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Admit a compiler-keyed release target under the locked private package cache."""

import argparse
import os
from pathlib import Path
import re
import stat


def private_directory(info):
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o022:
        raise ValueError("target cache directories must be owned and not group/world writable")


def release_target(cache, compiler, target, homes, *, create=True):
    """Keep reusable outputs separate from source worktrees and other compilers.

    The package guardian owns the cache lock through child-group retirement. Only
    this target, not its parent, is writable by contained builds; they cannot swap
    the admitted root for a symlink. This does not fence unrelated host writers.
    """
    if not re.fullmatch(r"[0-9a-f]{16}", compiler):
        raise ValueError("compiler identity must be the admitted 16-hex digest")
    if not re.fullmatch(r"[A-Za-z0-9_]+(?:-[A-Za-z0-9_]+)+", target):
        raise ValueError("target must be a compiler target triple")
    cache = Path(cache)
    if not cache.is_absolute():
        raise ValueError("package cache must be absolute")
    cache = cache.resolve(strict=True)
    for home in homes:
        home = Path(home).resolve(strict=True)
        if cache == home or cache in home.parents:
            raise ValueError("package cache must not be a home or above it")
    name = f"rust-release-{target}-{compiler}.noindex"
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    parent = os.open(cache, flags)
    try:
        private_directory(os.fstat(parent))
        if create:
            try:
                os.mkdir(name, 0o700, dir_fd=parent)
            except FileExistsError:
                pass
        try:
            held = os.open(name, flags, dir_fd=parent)
        except FileNotFoundError:
            if not create:
                return cache / name
            raise
        try:
            info = os.fstat(held)
            private_directory(info)
            named = os.stat(name, dir_fd=parent, follow_symlinks=False)
            if (named.st_dev, named.st_ino) != (info.st_dev, info.st_ino):
                raise ValueError("target cache root changed during admission")
        finally:
            os.close(held)
    finally:
        os.close(parent)
    return cache / name


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", required=True)
    parser.add_argument("--compiler", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--home", action="append", required=True)
    parser.add_argument("--path-only", action="store_true",
                        help="validate the location without creating it before cache registration")
    args = parser.parse_args()
    print(release_target(args.cache, args.compiler, args.target, args.home,
                         create=not args.path_only))


if __name__ == "__main__":
    main()
