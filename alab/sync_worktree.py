#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Identify sync worktrees and empty verified build directories for daily cleanup."""

import errno
import os
import re
from pathlib import Path
import stat
import subprocess
import sys


BRANCH = b"branch refs/heads/alab-sync"


def registrations(cwd=None, env=None):
    data = subprocess.check_output(["git", "worktree", "list", "--porcelain", "-z"], cwd=cwd, env=env)
    if not data.endswith(b"\0\0"):
        raise ValueError("incomplete Git worktree list")
    for record in data[:-2].split(b"\0\0"):
        location, *fields = record.split(b"\0")
        if not location.startswith(b"worktree ") or not Path(os.fsdecode(location[9:])).is_absolute():
            raise ValueError("invalid Git worktree record")
        ownership = [field for field in fields if field in (b"detached", b"bare") or field.startswith(b"branch ")]
        if len(ownership) != 1:
            raise ValueError("ambiguous Git worktree record")
        yield Path(os.fsdecode(location[9:])), fields


def link_path(file, prefix=b""):
    if not stat.S_ISREG(file.lstat().st_mode):
        raise ValueError(f"expected a regular worktree link at {file}")
    value = file.read_bytes().removesuffix(b"\n")
    if not value.startswith(prefix) or not value[len(prefix):]:
        raise ValueError(f"invalid worktree link at {file}")
    return file.parent / os.fsdecode(value[len(prefix):])


def same_directory(left, right):
    # samefile also recognizes case aliases on case-insensitive volumes. Missing
    # worktrees still have a registered path, possibly below a symlinked parent.
    try:
        return left.samefile(right)
    except (FileNotFoundError, NotADirectoryError):
        return left.resolve() == right.resolve()


def state(common, destination, status):
    entries = list(registrations())
    holders = [path for path, fields in entries if BRANCH in fields]
    if len(holders) > 1:
        raise ValueError("multiple worktrees hold the reserved alab-sync branch")

    target = destination.resolve()
    if holders:
        holder = holders[0]
        if not (holder / ".git").exists():
            if status:
                return f"unavailable {holder}/"
            try:
                unlanded = subprocess.check_output(["git", "rev-list", "-1", "origin/main..alab-sync"])
            except subprocess.CalledProcessError:
                raise ValueError("cannot read missing sync worktree history") from None
            reason = " and alab-sync holds commits main lacks" if unlanded else ""
            raise ValueError(f"registered sync worktree {holder} is unavailable{reason}")
        if not same_directory(holder, target):
            if status:
                return f"elsewhere {holder}/"
            raise ValueError(f"the sync worktree of this clone is {holder}, not {destination}")

    matches = [fields for path, fields in entries if same_directory(path, target)]
    try:
        destination.lstat()
    except FileNotFoundError:
        if matches:
            raise ValueError(f"sync worktree registration exists at unavailable path {destination}")
        return "absent"

    if len(matches) != 1 or same_directory(entries[0][0], target):
        raise ValueError(f"{destination} is occupied and is not a linked worktree of this clone")
    fields = matches[0]
    if any(field.split(b" ", 1)[0] in (b"locked", b"prunable") for field in fields):
        raise ValueError(f"registered sync worktree {destination} is locked or unavailable")
    if not (BRANCH in fields or b"detached" in fields):
        raise ValueError(f"{destination} holds another branch")

    verify_worktree_links(common, target)
    return "present"


def verify_worktree_links(common, target, env=None):
    marker = target / ".git"
    common = common.resolve(strict=True)
    admin = link_path(marker, b"gitdir: ").resolve(strict=True)
    if not admin.parent.samefile(common / "worktrees"):
        raise ValueError(f"worktree link at {target} is outside this clone")
    backlink = link_path(admin / "gitdir").resolve(strict=True)
    if (backlink.name != ".git" or not backlink.parent.samefile(target)
            or not link_path(admin / "commondir").samefile(common)):
        raise ValueError(f"worktree links at {target} do not match this clone and checkout")
    for option, expected in (("--show-toplevel", target), ("--absolute-git-dir", admin)):
        value = subprocess.check_output(["git", "-C", str(target), "rev-parse", option], env=env)
        if not Path(os.fsdecode(value.removesuffix(b"\n"))).samefile(expected):
            raise ValueError(f"Git selected another checkout or registration at {target}")
    # Check the physical entry too, including a dangling lock link Git cannot read.
    try:
        (admin / "locked").lstat()
    except FileNotFoundError:
        return
    raise ValueError(f"registered sync worktree {target} is locked")


def check_linux_mounts(workspace, target, mountinfo):
    """Refuse mount points at the workspace or anywhere in its target, including file binds."""
    seen = False
    for row in mountinfo:
        before, separator, after = row.removesuffix(b"\n").partition(b" - ")
        fields = before.split(b" ", 6)
        if (not row.endswith(b"\n") or not separator or len(fields) < 6 or len(after.split(b" ", 2)) != 3
                or not fields[0].isdigit() or not fields[1].isdigit()
                or not re.fullmatch(rb"[0-9]+:[0-9]+", fields[2])
                or not fields[3].startswith(b"/") or not fields[4].startswith(b"/")
                or re.search(rb"\\(?![0-3][0-7]{2})", fields[4])):
            raise ValueError("cannot read Linux mountinfo record")
        # The kernel octal-escapes spaces, tabs, newlines and backslashes.
        where = re.sub(rb"\\([0-3][0-7]{2})", lambda m: bytes([int(m[1], 8)]), fields[4])
        if where in (workspace, target) or where.startswith(target + b"/"):
            raise ValueError("mounted path at or below cleanup target: " + os.fsdecode(where))
        seen = True
    if not seen:
        raise ValueError("empty Linux mountinfo")


def check_target_identity(descriptors):
    # Device numbers cannot distinguish a same-filesystem bind mount.
    if sys.platform.startswith("linux"):
        # Procfs reports the kernel's names for both, even on a case-folded filesystem.
        workspace, target = (os.readlink(b"/proc/self/fd/%d" % fd) for fd in descriptors[1:])
        if not workspace.startswith(b"/") or os.path.dirname(target) != workspace:
            raise ValueError("cannot identify the Linux cleanup target")
        with open("/proc/self/mountinfo", "rb") as mounts:
            check_linux_mounts(workspace, target, mounts)
    originals = [os.fstat(fd) for fd in descriptors]
    # These descriptors were opened before the process-use checks. The shell
    # subsequently entered the physical workspace directory.
    for path, original in zip((Path('..'), Path('.'), Path('target')), originals):
        current = path.lstat()
        if (not stat.S_ISDIR(current.st_mode) or not os.path.samestat(original, current)
                or current.st_dev != originals[0].st_dev):
            raise ValueError("the selected target or its containing checkout changed")


def check_cleanup_target(common, descriptors):
    import fcntl

    originals = [os.fstat(fd) for fd in descriptors]
    check_target_identity(descriptors)
    verify_worktree_links(common, Path('..').resolve(strict=True))

    def unreadable(error):
        raise error

    for directory, children, files, fd in os.fwalk(".", dir_fd=descriptors[2], onerror=unreadable):
        if (any(name.lower() == ".git" for name in children + files)
                or "HEAD" in files and "objects" in children
                and ("refs" in children or "packed-refs" in files or "reftable" in children)):
            raise ValueError(f"Git metadata in {directory}")
        if ".cargo-lock" in files or ".cargo-lock" in children:
            lock = os.open(".cargo-lock", os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
            try:
                if not stat.S_ISREG(os.fstat(lock).st_mode):
                    raise ValueError(f"unreadable Cargo build lock in {directory}")
                try:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BlockingIOError as error:
                    raise ValueError(f"Cargo is using {directory}/.cargo-lock") from error
            finally:
                os.close(lock)
        # Refuse a mounted subtree before descending into its contents.
        if any(os.stat(name, dir_fd=fd, follow_symlinks=False).st_dev != originals[2].st_dev
               for name in children):
            raise ValueError(f"another filesystem under {directory}")
    check_target_identity(descriptors)
    verify_worktree_links(common, Path('..').resolve(strict=True))


def clear_target_children(descriptors):
    # The final policy probe ran since the recursive inspection. Check the
    # names once more, then anchor every child operand to the original target.
    try:
        check_target_identity(descriptors)
        os.fchdir(descriptors[2])
    except (OSError, ValueError) as error:
        print(f"alab: keeping build target: {error}", file=sys.stderr)
        return

    # Snapshot only immediate children. Preserve every filename byte and the
    # first rm failure across batches, including under a large environment.
    children = os.listdir('.')
    offset = 0
    limit = 256
    while offset < len(children):
        batch = children[offset:offset + limit]
        try:
            status = subprocess.run(["rm", "-rf", "--", *batch], stdout=sys.stderr).returncode
        except OSError as error:
            if error.errno != errno.E2BIG or len(batch) == 1:
                raise
            limit = len(batch) // 2
            continue
        if status:
            sys.exit(status if status > 0 else 128 - status)
        offset += len(batch)
    print("cleared")


if __name__ == "__main__":
    try:
        if sys.argv[1:2] == ["--check-target"]:
            common, tree_fd, workspace_fd, target_fd = sys.argv[2:]
            check_cleanup_target(Path(common), tuple(map(int, (tree_fd, workspace_fd, target_fd))))
        elif sys.argv[1:2] == ["--clear-target"]:
            tree_fd, workspace_fd, target_fd = sys.argv[2:]
            clear_target_children(tuple(map(int, (tree_fd, workspace_fd, target_fd))))
        else:
            print(state(Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3] == "status"))
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(f"alab: cannot use sync worktree: {error}")
