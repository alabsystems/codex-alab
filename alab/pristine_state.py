#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check a pristine source with qualification's observer and external scratch."""
import argparse
import os
import re
import stat
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from qualification import checked_root, git, git_environment, worktree_clean


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


def cleanup_trees(owner, scratch):
    from sync_worktree import registrations, verify_worktree_links

    owner = checked_root(owner)
    env = git_environment()
    base = Path(git(owner, 'rev-parse', '--path-format=absolute', '--git-path', 'alab-pristine'))
    common = Path(git(owner, 'rev-parse', '--path-format=absolute', '--git-common-dir'))
    entries = list(registrations(owner, env))
    for tree, fields in entries[1:]:
        if tree.parent != base.parent or b'detached' not in fields:
            continue
        if any(field.split(b' ', 1)[0] in (b'locked', b'prunable') for field in fields):
            continue
        heads = [field[5:] for field in fields if field.startswith(b'HEAD ')]
        if len(heads) != 1 or not re.fullmatch(rb'[0-9a-f]{40}(?:[0-9a-f]{24})?', heads[0]):
            continue
        head = heads[0].decode('ascii')
        if tree.name not in (base.name, base.name + '-' + head):
            continue
        try:
            if not stat.S_ISDIR(tree.lstat().st_mode):
                continue
            verify_worktree_links(common, tree, env)
            workspace = tree / 'codex-rs'
            target = workspace / 'target'
            if not (stat.S_ISDIR(workspace.lstat().st_mode) and stat.S_ISDIR(target.lstat().st_mode)):
                continue
            if clean(tree, head, owner, scratch):
                sys.stdout.buffer.write(os.fsencode(tree) + b'\0')
        except FileNotFoundError:
            continue
        except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
            print(f'alab: preserving pristine checkout {tree}: {error}', file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    if sys.argv[1:2] == ['--cleanup-trees']:
        parser.add_argument('--cleanup-trees', action='store_true')
        parser.add_argument('owner', type=Path)
        parser.add_argument('scratch', type=Path)
        args = parser.parse_args()
        try:
            cleanup_trees(args.owner, args.scratch)
        except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
            print(f'alab: cannot enumerate pristine checkouts: {error}', file=sys.stderr)
            return 1
        return 0
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
