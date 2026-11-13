#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check a pristine source with qualification's observer and external scratch."""
import argparse
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from qualification import checked_root, git, worktree_clean


def clean(tree, source, owner, scratch):
    tree = checked_root(tree)
    common = Path(git(tree, 'rev-parse', '--path-format=absolute', '--git-common-dir')).resolve(strict=True)
    expected = Path(git(owner, 'rev-parse', '--path-format=absolute', '--git-common-dir')).resolve(strict=True)
    if common != expected:
        raise ValueError('pristine source belongs to another Git repository')
    scratch = Path(scratch)
    if not scratch.is_absolute():
        raise ValueError('pristine comparison scratch must be absolute')
    scratch = scratch.resolve(strict=True)
    if not scratch.is_dir() or scratch.is_relative_to(common) or scratch.is_relative_to(tree):
        raise ValueError('pristine comparison scratch must stay outside source and Git metadata')
    return worktree_clean(tree, source, scratch_dir=scratch)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('tree', type=Path)
    parser.add_argument('source')
    parser.add_argument('owner', type=Path)
    parser.add_argument('scratch', type=Path)
    args = parser.parse_args()
    try:
        return 0 if clean(args.tree, args.source, args.owner, args.scratch) else 1
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print('alab: pristine source check: ' + str(error), file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
