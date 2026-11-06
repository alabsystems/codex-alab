#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Allow cleanup only when this clone has no explicit preservation policy."""

import argparse
import os
from pathlib import Path
import stat
import subprocess
import sys


MARKER = "alab-preserve-builds"


def cleanup_allowed(repo):
    """Marker presence preserves builds across every worktree of the clone.

    Contents are explanatory, not executable or parsed. Any existing kind of marker,
    or an inability to establish its absence, preserves rather than deleting data.
    """
    try:
        result = subprocess.run(
            ["git", "-C", str(repo), "rev-parse", "--path-format=absolute", "--git-common-dir"],
            capture_output=True, timeout=20,
        )
        if result.returncode or not result.stdout.endswith(b"\n"):
            raise ValueError("cannot locate the clone's common directory")
        # Only Git's record terminator is framing: whitespace before it can be
        # part of the directory's name. Binary output also preserves path CRs.
        common = Path(os.fsdecode(result.stdout[:-1]))
        if not common.is_absolute() or not stat.S_ISDIR(os.stat(common).st_mode):
            raise ValueError("the clone's common directory is unavailable")
        marker = common / MARKER
        try:
            os.lstat(marker)
        except FileNotFoundError:
            return True
        return False
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"alab: cannot establish cleanup policy ({error}); preserving builds", file=sys.stderr)
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    return 0 if cleanup_allowed(parser.parse_args().repo) else 1


if __name__ == "__main__":
    sys.exit(main())
