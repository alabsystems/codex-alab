#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Prune codex-alab packages to the newest <n>, never one that is in use.

alab/package.sh runs this as its last step; it can also be run by hand.

usage: prune.py --root <out>/codex [--keep <n>] [--prefix <atpkg prefix>]
                [--codex-home <dir>]... [--repo <clone>] [--dry-run]

The clone-local <git-common-dir>/alab-preserve-builds policy disables deletion. Without
it, collection holds atpkg's existing <prefix>/store.lock BSD flock across its entire
census and deletion; a manual selection cannot move between the snapshot and deletion.
A missing, special, unreadable or contended lock defers collection without creating it.

A package is a directory directly under --root holding a regular alab-build.json; they are
ordered by its "built_at", newest first, and every one past the newest <n> (default 3) is
removed, unless it is IN USE:

  linked   atpkg links it as codex. Read, never followed into a binary:
           - the link marker <prefix>/links/codex (atpkg's linkmode, a small 0600 TOML file:
             `checkout = "<package dir>"`, `bins = ["bin/codex"]`);
           - <prefix>/bin/codex and <prefix>/agents/codex: a symlink is resolved with
             readlink, never read through (a package's bin/codex is a 239 MB Mach-O); a
             regular file is read only when it is a small text shim (the agents/ twin's
             `exec '<path>' "$@"`);
           - `aterm pkg which codex`, when aterm is on PATH.
  running  a live process executes from it: its image (proc_pidpath, for every pid this
           user may read) or its `ps -o comm` path lies inside the package, codex itself or
           its codex-code-mode-host or rg.
  owed     a codex-alab session record that stands (<home>/alab/sessions/<pid>.json or its
           recoverable <pid>.json.<nonce>.tmp, the crash witness aterm relaunches from)
           names it: its resumeArgv[0] (the line a crash relaunch types), its exe or its
           launchArgv0, or a shim one of them is. The record
           of a process that died is exactly a relaunch still owed, with no process left to
           keep its package.

Paths are read whole, spaces and all (`~/Library/Application Support/...`): a shim's quoted
`exec '<path>'`, and in `aterm pkg which` every reading of a path that runs on past a space.

Fail closed: when no link source is readable, a link marker stands but cannot be read, a
sessions directory or a record in it cannot be read, a listed record moves before its read,
or no process can be listed, nothing is pruned. <prefix> defaults to the directory above
$ATPKG_BIN, else
~/Library/Application Support/aterm/pkg; the homes (--codex-home, repeatable) default to
$CODEX_HOME, ~/.codex and each directory in $ALAB_CODEX_HOMES (colon-separated). Exit 0
(whatever it pruned or kept), 2 on a usage error.

The newest package is never pruned (--keep is at least 1), nor is one atpkg links, whatever
the records say.

THE HOMES ARE ALL IT SEES: a session record is read only under those homes. aterm strips
CODEX_* from the shells it starts, so a codex-alab session running under another CODEX_HOME
(one a person exported by hand in that tab) is invisible here unless that home is named
(--codex-home, ALAB_CODEX_HOMES): its package is then kept only while it is linked, running,
or among the newest <n>. A record stands until its own relaunch takes it or it is a day old
(the start-up sweep leaves another tab's record for that tab: CONTRACT §4).
"""

import argparse
from contextlib import contextmanager
import ctypes
import ctypes.util
import json
import os
import re
import runpy
import shutil
import stat
import subprocess
import sys

try:
    import fcntl
except ImportError:
    fcntl = None


cleanup_allowed = runpy.run_path(os.path.join(os.path.dirname(__file__), "retention.py"))["cleanup_allowed"]

# A shim or a marker larger than this is not one (a copied binary, say): never read whole.
SMALL_TEXT = 64 * 1024
PROGRAM = "codex"


def say(words):
    print(f"alab: {words}", file=sys.stderr)


def default_prefix():
    atpkg_bin = os.environ.get("ATPKG_BIN")
    if atpkg_bin:
        return os.path.dirname(atpkg_bin.rstrip("/"))
    return os.path.expanduser("~/Library/Application Support/aterm/pkg")


@contextmanager
def store_guard(prefix):
    """Hold atpkg's writer flock throughout the census and deletion.

    atpkg's lock.rs uses BSD flock on <prefix>/store.lock, opened relative to a
    no-follow prefix directory. Open the existing rendezvous only: pruning never
    creates a store or writes an atpkg holder/claim record. Missing or contended
    locks defer collection instead of racing a manual package selection.
    """
    if fcntl is None:
        raise OSError("BSD flock is unavailable")
    directory = os.open(prefix, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    descriptor = None
    locked = False
    try:
        descriptor = os.open(
            "store.lock", os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC,
            dir_fd=directory,
        )
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            raise OSError("the atpkg writer lock is not regular")
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        locked = True
        current = os.stat(prefix, follow_symlinks=False)
        opened = os.fstat(directory)
        named = os.stat("store.lock", dir_fd=directory, follow_symlinks=False)
        held = os.fstat(descriptor)
        if ((current.st_dev, current.st_ino) != (opened.st_dev, opened.st_ino)
                or (named.st_dev, named.st_ino) != (held.st_dev, held.st_ino)):
            raise OSError("the atpkg writer lock moved during acquisition")
        yield
    finally:
        if descriptor is not None:
            if locked:
                fcntl.flock(descriptor, fcntl.LOCK_UN)
            os.close(descriptor)
        os.close(directory)


def small_text(path):
    """The text of a small regular file at `path` (never through a symlink), else None."""
    try:
        st = os.lstat(path)
    except OSError:
        return None
    if not stat.S_ISREG(st.st_mode) or st.st_size > SMALL_TEXT:
        return None
    try:
        with open(path, "rb") as f:
            data = f.read(SMALL_TEXT + 1)
    except OSError:
        return None
    if b"\0" in data:
        return None
    return data.decode("utf-8", "replace")


def toml_string(value):
    """A TOML basic ("...") or literal ('...') string's value, else None."""
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] == "'":
        return value[1:-1]
    if len(value) >= 2 and value[0] == value[-1] == '"':
        try:
            return json.loads(value)
        except ValueError:
            return None
    return None


def marker_paths(text):
    """The package paths atpkg's link marker names: its checkout, and checkout/<bin> for
    each bin. None when it names no checkout (an unreadable marker)."""
    checkout = None
    bins = []
    for line in text.splitlines():
        m = re.match(r"\s*checkout\s*=\s*(.+?)\s*$", line)
        if m:
            checkout = toml_string(m.group(1))
        m = re.match(r"\s*bins\s*=\s*\[(.*)\]\s*$", line)
        if m:
            bins = [b for b in (toml_string(x) for x in re.findall(r"\"(?:[^\"\\]|\\.)*\"|'[^']*'", m.group(1))) if b]
    if not checkout or not os.path.isabs(checkout):
        return None
    return [checkout] + [os.path.join(checkout, b) for b in bins]


# Where an absolute path may start in a line of text: the line's start, or after a space, a
# quote, `=`, `(` or a backquote.
PATH_START = re.compile(r"(?:^|(?<=[\s'\"=(`]))/")
# Where it may end: before a space or a quote, or at the line's end.
PATH_END = re.compile(r"(?=[\s'\"`])|$")
# What trails a path in prose and is not part of it.
PATH_TRAIL = ")],;:`"
# How many readings of one path start are kept (each runs on past one more space).
PATH_READINGS = 32


def paths_in(text):
    """Every absolute path `text` may name, read whole: a quoted string ('...' or "..."), and
    from each place a path may start, every reading that ends at a later space, quote or the
    line's end, so a path with spaces in it is among them. Each extra reading can only keep a
    package (one inside it is inside it); none drops one."""
    found = []
    for line in text.splitlines():
        for m in re.finditer(r"'([^']*)'|\"((?:[^\"\\]|\\.)*)\"", line):
            quoted = m.group(1) if m.group(1) is not None else m.group(2)
            if quoted.startswith("/"):
                found.append(quoted)
        for start in PATH_START.finditer(line):
            ends = [m.start() for m in PATH_END.finditer(line, start.start() + 1)]
            for end in ends[:PATH_READINGS]:
                reading = line[start.start() : end].rstrip(PATH_TRAIL)
                if len(reading) > 1:
                    found.append(reading)
    return list(dict.fromkeys(found))


def shim_paths(path):
    """What a codex shim at `path` names, without reading a binary: (known, paths)."""
    if os.path.islink(path):
        try:
            target = os.readlink(path)
        except OSError:
            return (False, [])
        if not os.path.isabs(target):
            target = os.path.join(os.path.dirname(path), target)
        return (True, [target, os.path.realpath(path)])
    if not os.path.lexists(path):
        return (False, [])
    text = small_text(path)
    if text is None:
        # A regular file too big for a shim (a copied binary) names no package; it is
        # still a source that was read.
        return (True, [])
    return (True, paths_in(text))


def linked(prefix):
    """The paths atpkg links as codex, or None when that cannot be told."""
    paths = []
    known = False
    marker = os.path.join(prefix, "links", PROGRAM)
    if os.path.lexists(marker):
        text = small_text(marker)
        named = marker_paths(text) if text is not None else None
        if named is None:
            say(f"the link marker {marker} stands but cannot be read")
            return None
        paths += named
        known = True
    for shim in (os.path.join(prefix, "bin", PROGRAM), os.path.join(prefix, "agents", PROGRAM)):
        read, named = shim_paths(shim)
        known = known or read
        paths += named
    if shutil.which("aterm"):
        try:
            done = subprocess.run(
                ["aterm", "pkg", "which", PROGRAM],
                capture_output=True,
                text=True,
                timeout=20,
            )
            if done.returncode == 0:
                known = True
                paths += paths_in(done.stdout)
        except (OSError, subprocess.SubprocessError):
            pass
    return paths if known else None


# A session record larger than this is not one: never read whole.
RECORD_MAX = 1024 * 1024
# Keep the same filenames as alab-tui's record::owner_pid. A quarantine can be the only
# durable witness; its nonce is opaque, and the owning pid is an ASCII u32.
RECORD_NAME = re.compile(r"([0-9]+)\.json(?:\..*\.tmp)?", re.DOTALL)


def default_homes():
    homes = []
    if os.environ.get("CODEX_HOME"):
        homes.append(os.environ["CODEX_HOME"])
    homes.append(os.path.expanduser("~/.codex"))
    homes += [h for h in os.environ.get("ALAB_CODEX_HOMES", "").split(":") if h]
    return list(dict.fromkeys(homes))


def record_text(path):
    """A bounded regular record's text, else None, including a listed record that moved.

    Empty existing files are malformed records, not evidence of removal. Validate the
    opened descriptor so a replacement symlink or FIFO cannot bypass the file checks.
    """
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    except OSError:
        return None
    try:
        with os.fdopen(fd, "rb") as f:
            st = os.fstat(f.fileno())
            if not stat.S_ISREG(st.st_mode) or st.st_size > RECORD_MAX:
                return None
            data = f.read(RECORD_MAX + 1)
            if len(data) > RECORD_MAX:
                return None
            return data.decode("utf-8")
    except (OSError, UnicodeDecodeError):
        return None


def owed(homes):
    """The paths the codex-alab session records under `homes` name (each one's resumeArgv[0],
    exe and launchArgv0, and what a shim among them names), or None when the captured
    records cannot all be read safely, including a custody rename during the scan."""
    paths = []
    for home in homes:
        sessions = os.path.join(home, "alab", "sessions")
        try:
            os.lstat(sessions)
        except FileNotFoundError:
            continue
        except OSError as e:
            say(f"the session records {sessions} cannot be inspected ({e.strerror})")
            return None
        try:
            names = sorted(os.listdir(sessions))
        except OSError as e:
            say(f"the session records {sessions} cannot be listed ({e.strerror})")
            return None
        for name in names:
            match = RECORD_NAME.fullmatch(name)
            if match is None or int(match[1]) > 0xFFFF_FFFF:
                continue  # not a filename the native record sweep can recover
            path = os.path.join(sessions, name)
            text = record_text(path)
            try:
                record = json.loads(text) if text is not None else None
            except ValueError:
                record = None
            if not isinstance(record, dict):
                say(f"the session record {path} stands but cannot be read")
                return None
            argv = record.get("resumeArgv")
            named = [argv[0] if isinstance(argv, list) and argv else None]
            named += [record.get("exe"), record.get("launchArgv0")]
            for p in named:
                if isinstance(p, str) and os.path.isabs(p):
                    paths.append(p)
                    paths += shim_paths(p)[1]
    return paths


def running():
    """{path: pid} of every process image this user can see, or None when none can be
    listed."""
    images = {}
    listed = False
    libproc_path = ctypes.util.find_library("proc") or "/usr/lib/libproc.dylib"
    try:
        libproc = ctypes.CDLL(libproc_path, use_errno=True)
        n = libproc.proc_listallpids(None, 0)
        if n > 0:
            pids = (ctypes.c_int * (n * 2))()
            n = libproc.proc_listallpids(pids, ctypes.sizeof(pids))
            if n > 0:
                listed = True
                buf = ctypes.create_string_buffer(4096)
                for pid in pids[:n]:
                    if pid <= 0:
                        continue
                    if libproc.proc_pidpath(pid, buf, ctypes.sizeof(buf)) > 0:
                        images.setdefault(os.fsdecode(buf.value), pid)
    except (OSError, AttributeError):
        pass
    try:
        done = subprocess.run(
            ["/bin/ps", "-axo", "pid=,comm="], capture_output=True, text=True, timeout=20
        )
        if done.returncode == 0:
            listed = True
            for line in done.stdout.splitlines():
                parts = line.strip().split(None, 1)
                if len(parts) == 2 and parts[0].isdigit() and parts[1].startswith("/"):
                    images.setdefault(parts[1], int(parts[0]))
    except (OSError, subprocess.SubprocessError):
        pass
    return images if listed else None


def inside(path, root):
    """Whether `path` (as named, and resolved) is `root` or lies under it."""
    for p in {path, os.path.realpath(path)}:
        if p == root or p.startswith(root + "/"):
            return True
    return False


def packages(root):
    """Package directories under `root`, newest first (by alab-build.json's built_at)."""
    found = []
    try:
        names = os.listdir(root)
    except OSError:
        return []
    for name in names:
        if name.startswith("."):
            continue
        d = os.path.join(root, name)
        if os.path.islink(d) or not os.path.isdir(d):
            continue
        text = small_text(os.path.join(d, "alab-build.json"))
        if text is None:
            continue
        try:
            built = str(json.loads(text).get("built_at", ""))
        except ValueError:
            built = ""
        found.append((built, d))
    found.sort(reverse=True)
    return [d for _, d in found]


def main():
    ap = argparse.ArgumentParser(description="prune codex-alab packages, never one in use")
    ap.add_argument("--root", required=True, help="<out>/codex, where package.sh writes")
    ap.add_argument("--keep", type=int, default=3)
    ap.add_argument("--prefix", default=None, help="atpkg's prefix")
    ap.add_argument("--repo", default=os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                    help="clone whose common-dir/alab-preserve-builds policy applies")
    ap.add_argument(
        "--codex-home",
        action="append",
        default=None,
        help="a CODEX_HOME whose session records may name a package (repeatable; "
        "default $CODEX_HOME, ~/.codex and $ALAB_CODEX_HOMES)",
    )
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()
    if args.keep < 1:
        ap.error("--keep needs a positive number")
    root = os.path.realpath(args.root)
    prefix = args.prefix or default_prefix()
    if not cleanup_allowed(args.repo):
        say("the clone's cleanup policy preserves all packages")
        return 0
    candidates = packages(root)[args.keep :]
    if not candidates:
        return 0
    try:
        with store_guard(prefix):
            prune_candidates(args, candidates, prefix)
    except OSError as error:
        say(f"pruning stopped ({error}); no further packages removed")
    return 0


def prune_candidates(args, candidates, prefix):
    # `aterm pkg which` is an offline, read-only verb and never takes store.lock.
    links = linked(prefix)
    if links is None:
        say("cannot tell which package aterm links (no link marker, no codex shim, no aterm): pruning nothing")
        return 0
    procs = running()
    if procs is None:
        say("cannot list the running processes: pruning nothing")
        return 0
    records = owed(args.codex_home or default_homes())
    if records is None:
        say("cannot tell which package a session record still names: pruning nothing")
        return 0
    for d in candidates:
        real = os.path.realpath(d)
        link = next((p for p in links if inside(p, real)), None)
        if link is not None:
            say(f"keeping {d}: atpkg links it as codex ({link})")
            continue
        proc = next(((p, pid) for p, pid in procs.items() if inside(p, real)), None)
        if proc is not None:
            say(f"keeping {d}: a running process executes from it (pid {proc[1]}, {proc[0]})")
            continue
        record = next((p for p in records if inside(p, real)), None)
        if record is not None:
            say(f"keeping {d}: a codex-alab session record names it ({record})")
            continue
        if args.dry_run:
            say(f"would prune {d} (newest {args.keep} kept)")
            continue
        if not cleanup_allowed(args.repo):
            say("the clone's cleanup policy now preserves all remaining packages")
            return 0
        say(f"pruning {d} (newest {args.keep} kept)")
        shutil.rmtree(d)
    return 0


if __name__ == "__main__":
    sys.exit(main())
