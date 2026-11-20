#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check ident files against raw blobs or their canonical Git checkout bytes."""

from contextlib import contextmanager
import os
from pathlib import Path
import re
import stat
import subprocess
import threading


TIMEOUT = 60
CHUNK = 64 * 1024
OBJECT = re.compile(rb"[0-9a-f]{40}(?:[0-9a-f]{24})?")


def _command(repo, filter_config, *args):
    return ["git", "-C", str(repo), "-c", "core.fsmonitor=false", *filter_config, *args]


def _output(command, env, *, data=None, descriptor=None):
    done = subprocess.run(command, input=data, stdin=descriptor, stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL, env=env, timeout=TIMEOUT)
    if done.returncode:
        raise subprocess.CalledProcessError(done.returncode, command)
    return done.stdout


def _regular_entries(repo, commit, env, filter_config):
    data = _output(_command(repo, filter_config, "ls-tree", "--full-tree", "-r", "-z", commit), env)
    if data and not data.endswith(b"\0"):
        raise ValueError("source tree metadata is not NUL-framed")
    entries, seen = {}, set()
    for row in data.split(b"\0")[:-1]:
        try:
            header, path = row.split(b"\t", 1)
            mode, kind, identity = header.split(b" ")
        except ValueError:
            raise ValueError("source tree metadata is malformed") from None
        if (not path or path.startswith(b"/") or b"\0" in path
                or any(part in (b"", b".", b"..") for part in path.split(b"/"))
                or not OBJECT.fullmatch(identity) or path in seen):
            raise ValueError("source tree has an invalid path or object identity")
        seen.add(path)
        if mode in (b"100644", b"100755") and kind == b"blob":
            entries[path] = identity
        elif not ((mode == b"120000" and kind == b"blob")
                  or (mode == b"160000" and kind == b"commit")):
            raise ValueError("source tree has an unsupported file type")
    return entries


def _ident_paths(repo, entries, env, filter_config):
    if not entries:
        return []
    data = _output(_command(repo, filter_config, "check-attr", "-z", "--stdin", "ident"),
                   env, data=b"".join(path + b"\0" for path in entries))
    if not data.endswith(b"\0"):
        raise ValueError("source attributes are not NUL-framed")
    fields = data.split(b"\0")[:-1]
    if len(fields) != 3 * len(entries):
        raise ValueError("source attributes do not cover the requested files")
    seen, selected = set(), []
    for offset in range(0, len(fields), 3):
        path, attribute, value = fields[offset:offset + 3]
        if path not in entries or path in seen or attribute != b"ident":
            raise ValueError("source attributes have an unexpected path or attribute")
        seen.add(path)
        if value not in (b"unset", b"unspecified"):
            selected.append(path)
    return selected


def _identity(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_size,
            info.st_mtime_ns, info.st_ctime_ns, info.st_nlink)


@contextmanager
def _open_regular(repo, name):
    path = repo
    parents = []
    for part in name.split(b"/")[:-1]:
        info = path.lstat()
        if not stat.S_ISDIR(info.st_mode):
            raise ValueError("ident source has a linked or non-directory parent")
        parents.append((path, (info.st_dev, info.st_ino, info.st_mode)))
        path = path / os.fsdecode(part)
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode):
        raise ValueError("ident source has a linked or non-directory parent")
    parents.append((path, (info.st_dev, info.st_ino, info.st_mode)))
    path = path / os.fsdecode(name.split(b"/")[-1])
    before = path.lstat()
    if not stat.S_ISREG(before.st_mode):
        raise ValueError("ident source is not a regular file")
    descriptor = None
    primary = None
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        if _identity(os.fstat(descriptor)) != _identity(before):
            raise ValueError("ident source changed while opening")
        yield descriptor, path, parents, _identity(before)
    except BaseException as error:
        primary = error
        raise
    finally:
        if descriptor is not None:
            try:
                os.close(descriptor)
            except BaseException as error:
                if primary is None:
                    raise
                try:
                    primary.add_note(f"ident source descriptor close failed: {type(error).__name__}: {error}")
                except BaseException:
                    pass  # Cleanup diagnostics cannot replace the first failure.


def _unchanged(descriptor, path, parents, identity):
    if _identity(os.fstat(descriptor)) != identity or _identity(path.lstat()) != identity:
        raise ValueError("ident source changed during observation")
    for parent, before in parents:
        current = parent.lstat()
        if (current.st_dev, current.st_ino, current.st_mode) != before:
            raise ValueError("ident source parent changed during observation")


def _matches_checkout(command, env, descriptor):
    """Stream expected bytes without retaining a second copy of a large source."""
    process = None
    timer = None
    primary = None

    def stop():
        try:
            process.kill()
        except ProcessLookupError:
            pass

    def timeout():
        expired.set()
        stop()

    try:
        os.lseek(descriptor, 0, os.SEEK_SET)
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env)
        expired = threading.Event()
        timer = threading.Timer(TIMEOUT, timeout)
        timer.daemon = True
        matched = True
        timer.start()
        while True:
            expected = process.stdout.read(CHUNK)
            if not expected:
                break
            actual = bytearray()
            while len(actual) < len(expected):
                chunk = os.read(descriptor, len(expected) - len(actual))
                if not chunk:
                    break
                actual.extend(chunk)
            if actual != expected:
                matched = False
                stop()
                break
        if matched and os.read(descriptor, 1):
            matched = False
        status = process.wait(timeout=TIMEOUT)
        if expired.is_set():
            raise subprocess.TimeoutExpired(command, TIMEOUT)
        if matched and status:
            raise subprocess.CalledProcessError(status, command)
        return matched
    except BaseException as error:
        primary = error
        raise
    finally:
        selected = primary

        def cleanup(phase, action):
            nonlocal selected
            try:
                return action()
            except BaseException as error:
                if selected is None:
                    selected = error
                else:
                    try:
                        selected.add_note(f"ident checkout {phase} failed: {type(error).__name__}: {error}")
                    except BaseException:
                        pass  # Cleanup diagnostics cannot replace the first failure.

        if timer is not None:
            cleanup("timer cancellation", timer.cancel)

            def join_timer():
                if timer.ident is not None:
                    timer.join(timeout=5)

            def confirm_timer():
                if timer.is_alive():
                    raise ValueError("ident checkout timer retirement is unconfirmed")

            cleanup("timer join", join_timer)
            cleanup("timer retirement", confirm_timer)
        if process is not None:
            if cleanup("child status", process.poll) is None:
                cleanup("child stop", stop)
            cleanup("child wait", lambda: process.wait(timeout=5))
            cleanup("stdout close", process.stdout.close)
        if primary is None and selected is not None:
            raise selected


def validate_ident(repo, commit, *, env, filter_config):
    """Accept raw blobs or exact checkout forms under the caller's attribute policy.

    filter_config must disable every external clean, smudge and process driver.
    The supplied fresh-index environment is shared with the caller's status check.
    """
    if not isinstance(commit, str) or not OBJECT.fullmatch(commit.encode("ascii")):
        raise ValueError("ident observation requires an exact source commit")
    repo = Path(repo).resolve(strict=True)
    environment = dict(env, GIT_OPTIONAL_LOCKS="0", GIT_NO_REPLACE_OBJECTS="1", GIT_NO_LAZY_FETCH="1")
    entries = _regular_entries(repo, commit, environment, filter_config)
    for name in _ident_paths(repo, entries, environment, filter_config):
        with _open_regular(repo, name) as (descriptor, path, parents, identity):
            raw_hash = _output(_command(repo, filter_config, "hash-object", "--no-filters", "--stdin"),
                               environment, descriptor=descriptor)
            actual = raw_hash[:-1]
            if not raw_hash.endswith(b"\n") or not OBJECT.fullmatch(actual):
                raise ValueError("ident source hash is malformed")
            if actual != entries[name]:
                command = _command(repo, filter_config, "cat-file", "--filters",
                                   "--path=" + os.fsdecode(name), entries[name].decode("ascii"))
                if not _matches_checkout(command, environment, descriptor):
                    raise ValueError("ident source differs from its raw blob and canonical checkout")
            _unchanged(descriptor, path, parents, identity)
