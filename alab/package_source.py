#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Reuse exact, clean package sources only after their cache guardian retired its children.

The existing package cache lock serializes supported transactions. This adds source
custody, not a fence against unrelated host writers or children escaping that guardian's
group. Stable generations and unknown sources stay intact; only exact clean fresh
sources requested by their worker can be removed after guardian group retirement.
"""

import argparse
from contextlib import contextmanager
import fcntl
from functools import cache
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import stat
import subprocess
import sys
import tempfile


GUARD_ENV = "ALAB_PACKAGE_SOURCE_GUARD"
POOL = ".ubd/package-sources.noindex"
GUARDS = ".ubd/package-source-guards.noindex"
LIMIT = 128
MAX_METADATA_ENTRIES = 128
MAX_BYTES = 64 * 1024
MAX_STAMP = 8 * 1024 * 1024
STAMP_PATHS = ("codex-rs/Cargo.toml", "codex-rs/Cargo.lock")
HEX = re.compile(r"[0-9a-f]{32}")
COMMIT = re.compile(r"[0-9a-f]{40}(?:[0-9a-f]{24})?")


def directory(path):
    info = Path(path).lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o022:
        raise ValueError(f"source custody needs an owned real private directory: {path}")
    return [info.st_dev, info.st_ino]


def regular_bytes(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o022:
            raise ValueError(f"source custody needs an owned regular file: {path}")
        with os.fdopen(descriptor, "rb", closefd=False) as stream:
            data = stream.read(MAX_BYTES + 1)
        if len(data) > MAX_BYTES:
            raise ValueError("source custody record is too large")
        return data
    finally:
        os.close(descriptor)


def read(path):
    value = json.loads(regular_bytes(path))
    if not isinstance(value, dict) or value.get("schema") != 1:
        raise ValueError("unknown source custody record")
    return value


def write(path, value, *, existing=None):
    """Atomic durable publication; never replace unexpected receipt bytes."""
    path = Path(path)
    directory(path.parent)
    data = (json.dumps(value, sort_keys=True) + "\n").encode()
    if len(data) > MAX_BYTES:
        raise ValueError("source custody record is too large")
    descriptor, temporary = tempfile.mkstemp(prefix=".source-record-", dir=path.parent)
    temporary = Path(temporary)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        if existing is None:
            os.link(temporary, path, follow_symlinks=False)
        else:
            if read(path) != existing:
                raise ValueError("source custody changed before publication")
            os.replace(temporary, path)
        parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(parent)
        finally:
            os.close(parent)
    finally:
        temporary.unlink(missing_ok=True)


def private_child(parent, name):
    parent = Path(parent)
    directory(parent)
    child = parent / name
    try:
        child.mkdir(mode=0o700)
    except FileExistsError:
        pass
    directory(child)
    return child


def metadata_root(cache, relative):
    at = Path(cache)
    directory(at)
    for part in Path(relative).parts:
        at = private_child(at, part)
    return at


def existing_root(cache, relative):
    at = Path(cache)
    directory(at)
    for part in Path(relative).parts:
        at = at / part
        directory(at)
    return at


@cache
def repository_environment():
    # Ask this Git which variables redirect repository-local state. The query
    # needs no repository and must not itself inherit a caller's redirection.
    done = subprocess.run(["git", "rev-parse", "--local-env-vars"], capture_output=True,
                          timeout=60, env={key: value for key, value in os.environ.items()
                                           if not key.startswith("GIT_")})
    names = done.stdout.decode("ascii").splitlines()
    if (done.returncode or not names or len(done.stdout) > MAX_BYTES
            or any(not re.fullmatch(r"GIT_[A-Z0-9_]+", name) for name in names)):
        raise ValueError("Git did not provide bounded repository-local environment names")
    # Command-scoped caller configuration is still ordinary Git configuration.
    return frozenset(name for name in names if not name.startswith("GIT_CONFIG"))


def git_environment(overrides=None):
    return {**{key: value for key, value in os.environ.items() if key not in repository_environment()},
            "GIT_OPTIONAL_LOCKS": "0", "GIT_TERMINAL_PROMPT": "0", "GIT_NO_REPLACE_OBJECTS": "1",
            **(overrides or {})}


def git(repo, *args, env=None):
    # Performance caches cannot supply custody evidence or run a callback between
    # its validation and removal. These command-local settings also reach Git's
    # internal status during worktree remove; ordinary checkout/ref hooks remain.
    done = subprocess.run(["git", "-C", str(repo), "-c", "core.fsmonitor=false",
                           "-c", "core.untrackedCache=false", *args], capture_output=True, timeout=60,
                          env=git_environment(env))
    if done.returncode:
        raise RuntimeError(f"source git {args[0]} failed: {done.stderr.decode(errors='replace').strip()}")
    return os.fsdecode(done.stdout[:-1] if done.stdout.endswith(b"\n") else done.stdout)


def common(repo):
    return Path(git(repo, "rev-parse", "--path-format=absolute", "--git-common-dir")).resolve(strict=True)


def process_start(pid):
    done = subprocess.run(["/bin/ps", "-p", str(pid), "-o", "lstart="],
                          capture_output=True, timeout=60,
                          env={**os.environ, "LC_ALL": "C", "TZ": "UTC"})
    if done.returncode or not done.stdout.strip():
        raise ValueError("source guardian process is unavailable")
    return done.stdout.strip().decode("ascii")


def context(cache):
    named = os.environ.get(GUARD_ENV)
    if not named:
        raise ValueError("no source-owning package guardian")
    path = Path(named)
    root = existing_root(cache, GUARDS)
    if not path.is_absolute() or path.name != "guard.json" or path.parent.parent != root:
        raise ValueError("source guardian is outside the admitted cache")
    value = read(path)
    if (value.get("phase") != "active" or value.get("cache") != str(cache)
            or value.get("cache_id") != directory(cache)
            or value.get("guard_id") != directory(path.parent)
            or not isinstance(value.get("epoch"), str) or not HEX.fullmatch(value["epoch"])
            or not isinstance(value.get("pid"), int)
            or process_start(value["pid"]) != value.get("start")):
        raise ValueError("source guardian custody changed or ended")
    return path, value


def pool_key(repo, compiler, target):
    if not re.fullmatch(r"[0-9a-f]{16}", compiler) or not re.fullmatch(r"[A-Za-z0-9_]+(?:-[A-Za-z0-9_]+)+", target):
        raise ValueError("invalid source compiler or target identity")
    root = common(repo)
    identity = directory(root)
    key = hashlib.sha256(json.dumps([str(root), identity, compiler, target]).encode()).hexdigest()[:32]
    return key, root, identity


@contextmanager
def pool_lock(pool):
    descriptor = os.open(pool / ".lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o022:
            raise ValueError("source pool lock is not private")
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield
    finally:
        os.close(descriptor)


def record_paths(cache, key, source_id):
    if not isinstance(key, str) or not HEX.fullmatch(key) or not isinstance(source_id, str) or not HEX.fullmatch(source_id):
        raise ValueError("invalid source custody address")
    pool = existing_root(cache, POOL) / key
    directory(pool)
    return pool / f"source-{source_id}.json", pool / f"source-{source_id}.noindex"


def validate(cache, record, *, clean=True):
    path, source = record_paths(cache, record.get("key"), record.get("source_id"))
    if record.get("source") != str(source):
        raise ValueError("source receipt names another generation")
    validate_binding(record, clean=clean)
    return path, source


def validate_binding(record, *, clean=True, before_checkout=False):
    if record.get("custody") != 2:
        raise ValueError("source has no checked creation/checkout custody")
    source = Path(record["source"])
    if (not source.is_absolute() or record.get("source_id_fs") != directory(source)
            or record.get("common_id") != directory(record["common"])
            or str(common(source)) != record["common"]
            or Path(git(source, "rev-parse", "--show-toplevel")).resolve(strict=True) != source
            or regular_bytes(source / ".git").decode() != record.get("gitfile")):
        raise ValueError("source or repository identity was replaced")
    entry = Path(git(source, "rev-parse", "--absolute-git-dir"))
    if (entry.resolve(strict=True) != entry or entry.parent != Path(record["common"]) / "worktrees"
            or record.get("entry") != str(entry)
            or record.get("entry_id") != directory(entry)
            or regular_bytes(entry / "gitdir").decode() != record.get("gitdir")
            or record.get("gitdir") != str(source / ".git") + "\n"):
        raise ValueError("source Git registration was replaced")
    try:
        (entry / "locked").lstat()
    except FileNotFoundError:
        pass
    else:
        raise ValueError("source worktree was locked for preservation")
    if git(source, "rev-parse", "HEAD") != record.get("commit"):
        raise ValueError("source HEAD moved outside its custody")
    if metadata(entry) != record.get("metadata"):
        raise ValueError("source Git metadata changed outside its custody")
    detached(source)
    # Status/checkout can trust assume-unchanged or skip-worktree entries. Do not
    # move away from a source whose user has asked Git to ignore its file changes.
    if any(row and not row.startswith("H ") for row in git(source, "ls-files", "-v", "-z").split("\0")):
        raise ValueError("source index suppresses file-change detection")
    if before_checkout:
        if (entry / "index").exists():
            raise ValueError("new source acquired an index before checkout")
    elif git(source, "diff", "--cached", "--name-only", "--no-ext-diff", "--no-textconv", record["commit"], "--"):
        raise ValueError("source index holds staged work")
    if clean and fresh_status(source, record["commit"]):
        raise ValueError("source holds tracked, untracked or ignored work")
    unchanged_binding(record)
    return source


def unchanged_binding(record):
    """Recheck filesystem custody after the last command that can run a callback."""
    if (directory(record["source"]) != record["source_id_fs"]
            or directory(record["common"]) != record["common_id"]
            or directory(record["entry"]) != record["entry_id"]
            or regular_bytes(Path(record["source"]) / ".git").decode() != record["gitfile"]
            or metadata(Path(record["entry"])) != record["metadata"]):
        raise ValueError("source custody changed during Git validation")


def remove_fresh(record, source):
    # Removal runs its own status after validation. Suppress every configured
    # external clean/process filter there, so no second callback can add owner
    # work after the final metadata check. A transformed worktree that needs its
    # filter to appear clean will instead be refused and retained by normal Git.
    keys = git(source, "config", "--name-only", "--null", "--list")
    if len(os.fsencode(keys)) > MAX_BYTES or keys and not keys.endswith("\0"):
        raise ValueError("Git filter configuration is unbounded or not NUL-framed")
    drivers = set()
    for key in keys.split("\0")[:-1]:
        if not key:
            raise ValueError("Git configuration has an ambiguous empty key")
        match = re.fullmatch(r"(filter\..*)\.(?:clean|process|required)", key, re.DOTALL)
        if match:
            if "=" in match[1]:
                raise ValueError("Git filter key cannot be safely overridden for removal")
            drivers.add(match[1])
    options = []
    for driver in sorted(drivers):
        options.extend(("-c", driver + ".clean=", "-c", driver + ".process=",
                        "-c", driver + ".required=false"))
    unchanged_binding(record)
    git(record["common"], *options, "worktree", "remove", "--", str(source))


def fresh_status(source, commit, *, nul=False):
    """Read worktree content against a fresh index, never cached stat/monitor validity."""
    scratch = Path(tempfile.mkdtemp(prefix="alab-source-index-", suffix=".noindex"))
    index = scratch / "index"
    captured = None
    try:
        environment = {"GIT_INDEX_FILE": str(index)}
        git(source, "-c", "core.splitIndex=false", "read-tree", commit, env=environment)
        info = index.lstat()
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid()
                or info.st_mode & 0o022 or info.st_nlink != 1):
            raise ValueError("validation index is not an exact owned file")
        captured = stamp_info(info)
        args = ("-z",) if nul else ()
        return git(source, "-c", "core.splitIndex=false", "status", "--porcelain=v1",
                   "--untracked-files=all", "--ignored", *args, env=environment)
    finally:
        # Remove only the exact unchanged index Git wrote. Any extra/replaced
        # scratch state is retained, never swept up by recursive cleanup.
        try:
            if captured is not None and stamp_info(index.lstat()) == captured:
                index.unlink()
            scratch.rmdir()
        except OSError:
            print(f"alab: kept changed validation scratch {scratch}", file=sys.stderr)


def metadata(entry):
    """Bind per-worktree user state, without treating index refresh as new work."""
    result, count, size = [], 0, 0
    def walk(parent, relative):
        nonlocal count, size
        for path in sorted(parent.iterdir()):
            name = relative / path.name
            info = path.lstat()
            count += 1
            if count > MAX_METADATA_ENTRIES:
                raise ValueError("source Git metadata is unexpectedly large")
            if stat.S_ISDIR(info.st_mode):
                directory(path)
                result.append([str(name), "directory", stat.S_IMODE(info.st_mode)])
                walk(path, name)
            else:
                if info.st_nlink != 1:
                    raise ValueError("source Git metadata has an alias")
                if name == Path("index"):
                    # Git may atomically refresh stat bookkeeping. Status, HEAD,
                    # and the index's stage/skip flags independently fence work.
                    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o022:
                        raise ValueError("source index is not an owned regular file")
                    continue
                data = regular_bytes(path)
                size += len(data)
                if size > MAX_BYTES:
                    raise ValueError("source Git metadata is unexpectedly large")
                result.append([str(name), stat.S_IMODE(info.st_mode), hashlib.sha256(data).hexdigest()])
    walk(entry, Path())
    return result


def detached(source):
    done = subprocess.run(["git", "-C", str(source), "symbolic-ref", "--quiet", "HEAD"],
                          capture_output=True, timeout=60, env=git_environment())
    if done.returncode != 1:
        raise ValueError("source is no longer a readable detached worktree")


def bind(binding, key, source_id, epoch):
    validate_binding(binding)
    return {**binding, "key": key, "source_id": source_id,
            "phase": "active", "epoch": epoch, "returned": False}


def git_identity(repo):
    """Name our automated reflog writes and fix their resolved committer time."""
    environment = {"GIT_COMMITTER_NAME": "ALab package source custody",
                   "GIT_COMMITTER_EMAIL": "package-source@invalid"}
    identity = git(repo, "var", "GIT_COMMITTER_IDENT", env=environment)
    match = re.fullmatch(r"(.+) (-?[0-9]+) ([+-][0-9]{4})", identity)
    if not match or "\n" in identity or "\t" in identity or len(os.fsencode(identity)) > MAX_BYTES:
        raise ValueError("Git did not provide a single bounded committer identity")
    return os.fsencode(identity), {**environment, "GIT_COMMITTER_DATE": f"@{match[2]} {match[3]}"}


def binding(repo, source, commit):
    """Capture identities; the caller must prove content ownership before publication."""
    root = common(repo)
    source = Path(source)
    entry = Path(git(source, "rev-parse", "--absolute-git-dir"))
    return {"schema": 1, "custody": 2, "source": str(source),
              "source_id_fs": directory(source), "common": str(root), "common_id": directory(root),
              "entry": str(entry), "entry_id": directory(entry),
              "metadata": metadata(entry),
              "gitfile": regular_bytes(source / ".git").decode(),
              "gitdir": regular_bytes(entry / "gitdir").decode(), "commit": commit}


def optional_bytes(path):
    try:
        return regular_bytes(path)
    except FileNotFoundError:
        return None


def checked_checkout(record, commit, *, initial=False):
    """Accept only this command's HEAD/log transition, never post-hook additions."""
    if not COMMIT.fullmatch(commit):
        raise ValueError("checkout requires a full commit id")
    source = validate_binding(record, clean=not initial, before_checkout=initial)
    entry = Path(record["entry"])
    before_log = optional_bytes(entry / "logs/HEAD")
    identity, environment = git_identity(source)
    action = "alab-package-checkout-" + secrets.token_hex(16)
    git(source, "checkout", "--no-overwrite-ignore", "--no-recurse-submodules", "--quiet", "--detach", commit,
        env={**environment, "GIT_REFLOG_ACTION": action})
    after_log = optional_bytes(entry / "logs/HEAD")
    if commit == record["commit"]:
        if after_log != before_log:
            raise ValueError("same-HEAD checkout acquired an unowned reflog change")
    else:
        line = os.fsencode(record["commit"] + " " + commit + " ") + identity + b"\t" + action.encode() + b"\n"
        if (before_log is not None and after_log != before_log + line
                or before_log is None and after_log not in (None, line)):
            raise ValueError("checkout reflog is not the exact owned transition")
    current = metadata(entry)
    before = {row[0]: row[1:] for row in record["metadata"]}
    after = {row[0]: row[1:] for row in current}
    if regular_bytes(entry / "HEAD") != commit.encode("ascii") + b"\n":
        raise ValueError("checkout HEAD is not the requested commit")
    if after.get("HEAD", [None])[0] != before["HEAD"][0]:
        raise ValueError("checkout changed HEAD permissions")
    allowed = {"HEAD", "logs/HEAD"}
    # Git can create its logs directory when this is the first logged move.
    if before_log is None and after_log is not None and "logs" not in before:
        if after.get("logs", [None])[0] != "directory":
            raise ValueError("checkout reflog directory is not owned")
        allowed.add("logs")
    if {key: value for key, value in before.items() if key not in allowed} != {
            key: value for key, value in after.items() if key not in allowed}:
        raise ValueError("checkout changed metadata outside its owned HEAD transition")
    if before_log is not None and after["logs/HEAD"][0] != before["logs/HEAD"][0]:
        raise ValueError("checkout changed reflog permissions")
    updated = {**record, "commit": commit, "metadata": current}
    validate_binding(updated)
    return updated


def create_fresh(repo, source, commit):
    """Register without checkout, prove Git's exact birth, then admit the hook run."""
    if not COMMIT.fullmatch(commit):
        raise ValueError("source requires a full commit id")
    named = Path(source)
    reserved = directory(named)
    source = named.resolve(strict=True)
    if directory(source) != reserved or any(source.iterdir()):
        raise ValueError("source reservation changed or is not empty")
    outside_driver(source, repo)
    root = common(repo)
    root_id = directory(root)
    registry = root / "worktrees"
    previous = set()
    if registry.exists():
        directory(registry)
        previous = {path.name for path in registry.iterdir()}
    identity, environment = git_identity(repo)
    git(repo, "worktree", "add", "--quiet", "--detach", "--no-checkout", str(source), commit, env=environment)
    record = binding(repo, source, commit)
    entry = Path(record["entry"])
    if (record["source_id_fs"] != reserved or record["common_id"] != root_id
            or entry.name in previous or set(source.iterdir()) != {source / ".git"}
            or (entry / "index").exists()):
        raise ValueError("source registration was not the empty reserved creation")
    expected = {"HEAD": commit.encode("ascii") + b"\n",
                "commondir": os.fsencode(os.path.relpath(root, entry)) + b"\n",
                "gitdir": os.fsencode(str(source / ".git")) + b"\n"}
    log = optional_bytes(entry / "logs/HEAD")
    if log is not None:
        birth = b"0" * len(commit) + b" " + commit.encode("ascii") + b" " + identity + b"\n"
        if log != birth:
            raise ValueError("source birth reflog is not the exact Git creation")
        expected["logs/HEAD"] = birth
    paths = {row[0] for row in record["metadata"]}
    directories = {row[0] for row in record["metadata"] if row[1] == "directory"}
    # Git's ref-store initialization can also create its empty refs directory.
    # Any child beneath it is a separate unexpected metadata row and refuses.
    if directories - {"logs", "refs"} or paths != set(expected) | directories:
        raise ValueError("source registration acquired unowned Git state")
    if any(regular_bytes(entry / name) != data for name, data in expected.items()):
        raise ValueError("source registration is not the exact Git creation")
    validate_binding(record, clean=False, before_checkout=True)
    return checked_checkout(record, commit, initial=True)


def blob(source, revision):
    done = subprocess.run(["git", "-C", str(source), "cat-file", "blob", revision],
                          capture_output=True, timeout=60, env=git_environment())
    if done.returncode or len(done.stdout) > MAX_STAMP:
        raise ValueError("original stamp blob is unavailable or too large")
    return done.stdout


def stamp_info(info):
    return [info.st_dev, info.st_ino, info.st_uid, info.st_mode, info.st_size,
            info.st_mtime_ns, info.st_ctime_ns, info.st_nlink]


def stamp_file(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        before = os.fstat(descriptor)
        if (not stat.S_ISREG(before.st_mode) or before.st_uid != os.geteuid()
                or before.st_mode & 0o022 or before.st_nlink != 1 or before.st_size > MAX_STAMP):
            raise ValueError("stamp file is not an exact owned regular file")
        with os.fdopen(descriptor, "rb", closefd=False) as stream:
            data = stream.read(MAX_STAMP + 1)
        if stamp_info(os.fstat(descriptor)) != stamp_info(before) or stamp_info(Path(path).lstat()) != stamp_info(before):
            raise ValueError("stamp file changed during capture")
        return data, {"identity": stamp_info(before), "sha256": hashlib.sha256(data).hexdigest()}
    finally:
        os.close(descriptor)


def only_stamps(source):
    rows = fresh_status(source, git(source, "rev-parse", "HEAD"), nul=True).split("\0")
    if any(row and (row[:3] != " M " or row[3:] not in STAMP_PATHS) for row in rows):
        raise ValueError("fresh source holds work beyond its known stamp files")


def capture_stamps(record, version):
    """Record only the legacy helper's exact expected manifest/version-only lock writes."""
    source = validate_binding(record, clean=False)
    if not re.fullmatch(r"[0-9A-Za-z.+-]+", version):
        raise ValueError("invalid legacy stamp version")
    only_stamps(source)
    stamps = {}
    for name in STAMP_PATHS:
        original_id = git(source, "rev-parse", f"{record['commit']}:{name}")
        original = blob(source, original_id)
        data, post = stamp_file(source / name)
        if name.endswith("Cargo.toml"):
            lines = original.decode().splitlines(keepends=True)
            in_package, changed = False, False
            for index, line in enumerate(lines):
                if re.fullmatch(r"\[workspace\.package\]\s*", line):
                    in_package = True
                elif line.startswith("["):
                    in_package = False
                elif in_package and not changed and re.match(r"version\s*=", line):
                    lines[index] = f'version = "{version}"\n'
                    changed = True
            if not changed or data != "".join(lines).encode():
                raise ValueError("manifest is not the helper's exact legacy version stamp")
        else:
            old_lines, new_lines = original.splitlines(keepends=True), data.splitlines(keepends=True)
            if len(old_lines) != len(new_lines) or any(
                    old != new and (not re.fullmatch(rb'version = "[^"\n]+"\n', old)
                                    or new != f'version = "{version}"\n'.encode())
                    for old, new in zip(old_lines, new_lines)):
                raise ValueError("lock contains more than the helper's legacy version stamp")
        stamps[name] = {**post, "original": original_id}
    return {**record, "stamps": stamps}


def verify_stamps(record):
    source = validate_binding(record, clean=False)
    only_stamps(source)
    stamps = record.get("stamps")
    if not isinstance(stamps, dict) or set(stamps) != set(STAMP_PATHS):
        raise ValueError("unknown tool-owned stamp record")
    for name, expected in stamps.items():
        _, current = stamp_file(source / name)
        if current != {key: expected[key] for key in ("identity", "sha256")}:
            raise ValueError("stamp file changed after the helper wrote it")
        if git(source, "rev-parse", f"{record['commit']}:{name}") != expected["original"]:
            raise ValueError("original stamp object changed")
    return source


def restore_stamps(record):
    source = verify_stamps(record)
    for name, expected in record["stamps"].items():
        original = blob(source, expected["original"])
        descriptor = os.open(source / name, os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK)
        try:
            # Use this checked inode, never a pathname truncate or rename over a
            # replacement. Unknown edits preserve the source and its registration.
            before = os.fstat(descriptor)
            if stamp_info(before) != expected["identity"]:
                raise ValueError("stamp inode changed before restoration")
            with os.fdopen(descriptor, "rb", closefd=False) as stream:
                data = stream.read(MAX_STAMP + 1)
            if (hashlib.sha256(data).hexdigest() != expected["sha256"]
                    or stamp_info(os.fstat(descriptor)) != expected["identity"]
                    or stamp_info((source / name).lstat()) != expected["identity"]):
                raise ValueError("stamp contents changed before restoration")
            os.lseek(descriptor, 0, os.SEEK_SET)
            with os.fdopen(descriptor, "wb", closefd=False) as stream:
                stream.write(original)
                stream.truncate()
                stream.flush()
                os.fsync(descriptor)
        finally:
            os.close(descriptor)
    validate_binding(record)


def source_reservations(pool):
    """Bound growth even when registration or receipt publication never completed."""
    generations = set()
    for path in pool.iterdir():
        if path.name == ".lock":
            continue
        named = re.fullmatch(r"source-([0-9a-f]{32})\.(?:json|noindex)", path.name)
        # A receipt and its matching directory consume one reservation. Unknown
        # files, failed temporary publications and unreceipted sources also hold
        # admission; none is adopted or deleted to repair a partial transaction.
        generations.add(("source", named[1]) if named else ("unknown", path.name))
        if len(generations) >= LIMIT:
            return LIMIT
    return len(generations)


def outside_driver(source, repo):
    driver = Path(git(repo, "rev-parse", "--show-toplevel")).resolve(strict=True)
    if source == driver or source in driver.parents or driver in source.parents:
        raise ValueError("package source overlaps its active driver worktree")


def acquire(cache, repo, commit, compiler, target):
    """Claim a clean ready generation, otherwise preserve it and create a fresh one."""
    if not COMMIT.fullmatch(commit):
        raise ValueError("source requires a full commit id")
    cache = Path(cache).resolve(strict=True)
    guard_path, guard = context(cache)
    key, root, root_id = pool_key(repo, compiler, target)
    if guard.get("common") != str(root) or guard.get("common_id") != root_id:
        raise ValueError("source guardian belongs to another repository")
    pool = private_child(metadata_root(cache, POOL), key)
    with pool_lock(pool):
        receipts = sorted(pool.glob("source-*.json"))
        for path in receipts:
            try:
                old = read(path)
                if old.get("phase") != "ready":
                    continue
                bound, source = validate(cache, old)
                if (bound != path or old.get("key") != key or old.get("common") != str(root)
                        or old.get("common_id") != root_id):
                    raise ValueError("source receipt belongs to another pool or repository")
                outside_driver(source, repo)
                detached(source)
                active = {**old, "phase": "active", "epoch": guard["epoch"], "returned": False}
                write(path, active, existing=old)
                validate(cache, active)
                updated = checked_checkout(active, commit)
                validate(cache, updated)
                write(path, updated, existing=active)
                request = {"schema": 1, "key": key, "source_id": old["source_id"], "epoch": guard["epoch"]}
                write(guard_path.parent / f"lease-{old['source_id']}.json", request)
                return {"path": str(source), "managed": True, "reused": True, "binding": updated}
            except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError) as error:
                print(f"alab: preserved package source {path}: {error}", file=sys.stderr)
        if source_reservations(pool) >= LIMIT:
            raise ValueError("source pool is full; preserving every generation")
        source_id = secrets.token_hex(16)
        path, source = record_paths(cache, key, source_id)
        outside_driver(source, repo)
        source.mkdir(mode=0o700)
        created = create_fresh(repo, source, commit)
        record = bind(created, key, source_id, guard["epoch"])
        write(path, record)
        request = {"schema": 1, "key": key, "source_id": source_id, "epoch": guard["epoch"]}
        write(guard_path.parent / f"lease-{source_id}.json", request)
        return {"path": str(source), "managed": True, "reused": False, "binding": record}


def release(cache, source, commit):
    """Request return; only the guardian may publish Ready after group retirement."""
    cache = Path(cache).resolve(strict=True)
    guard_path, guard = context(cache)
    leases = list(guard_path.parent.glob("lease-*.json"))
    if len(leases) > LIMIT:
        raise ValueError("too many source leases")
    for lease in leases:
        request = read(lease)
        path, named = record_paths(cache, request.get("key"), request.get("source_id"))
        if named != Path(source):
            continue
        with pool_lock(path.parent):
            record = read(path)
            if (request.get("epoch") != guard["epoch"] or record.get("epoch") != guard["epoch"]
                    or record.get("phase") != "active" or record.get("commit") != commit):
                raise ValueError("source return does not own this lease")
            validate(cache, record)
            write(path, {**record, "returned": True}, existing=record)
            return
    raise ValueError("source return names no admitted lease")


def release_fresh(cache, record):
    """Request clean-only fresh-source removal after this guardian's group retires."""
    path, guard = context(Path(cache).resolve(strict=True))
    if record.get("common") != guard["common"] or record.get("common_id") != guard["common_id"]:
        raise ValueError("fresh source belongs to another repository")
    if "stamps" in record:
        verify_stamps(record)
    else:
        validate_binding(record)
    request = {**record, "epoch": guard["epoch"]}
    write(path.parent / f"fresh-{secrets.token_hex(16)}.json", request)


class Session:
    """Created while the existing cache lock is held; finished after run() returns."""

    def __init__(self, cache, repo):
        self.cache = Path(cache).resolve(strict=True)
        directory(self.cache)
        root = common(repo)
        parent = metadata_root(self.cache, GUARDS)
        self.path = Path(tempfile.mkdtemp(prefix="guard-", suffix=".noindex", dir=parent)) / "guard.json"
        self.record = {"schema": 1, "phase": "active", "epoch": secrets.token_hex(16),
                       "cache": str(self.cache), "cache_id": directory(self.cache),
                       "common": str(root), "common_id": directory(root),
                       "guard_id": directory(self.path.parent), "pid": os.getpid(),
                       "start": process_start(os.getpid())}
        write(self.path, self.record)

    def environment(self):
        return {**os.environ, GUARD_ENV: str(self.path)}

    def finish(self):
        """The caller has proved its leased child group gone while still holding the lock."""
        if (directory(self.cache) != self.record["cache_id"]
                or existing_root(self.cache, GUARDS) != self.path.parent.parent
                or directory(self.path.parent) != self.record["guard_id"]
                or read(self.path) != self.record):
            raise ValueError("source guardian record changed")
        leases = list(self.path.parent.glob("lease-*.json"))
        if len(leases) > LIMIT:
            raise ValueError("too many source leases")
        for lease in leases:
            try:
                request = read(lease)
                if request.get("epoch") != self.record["epoch"]:
                    raise ValueError("source lease belongs to another guardian")
                path, _ = record_paths(self.cache, request.get("key"), request.get("source_id"))
                with pool_lock(path.parent):
                    record = read(path)
                    if (record.get("epoch") != self.record["epoch"] or record.get("phase") != "active"
                            or record.get("returned") is not True):
                        continue
                    if (record.get("common") != self.record["common"]
                            or record.get("common_id") != self.record["common_id"]):
                        raise ValueError("source return belongs to another repository")
                    validate(self.cache, record)
                    write(path, {**record, "phase": "ready", "returned": False}, existing=record)
            except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError) as error:
                print(f"alab: preserved package source return {lease}: {error}", file=sys.stderr)
        fresh = list(self.path.parent.glob("fresh-*.json"))
        if len(fresh) > LIMIT:
            raise ValueError("too many fresh source returns")
        for request in fresh:
            try:
                record = read(request)
                if (record.get("epoch") != self.record["epoch"] or record.get("common") != self.record["common"]
                        or record.get("common_id") != self.record["common_id"]):
                    raise ValueError("fresh source return belongs to another guardian")
                if "stamps" in record:
                    restore_stamps(record)
                source = validate_binding(record)
                # No force, prune, file restoration, or recursive fallback. Git
                # still refuses any late dirty/locked worktree it observes.
                remove_fresh(record, source)
            except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError) as error:
                print(f"alab: preserved fresh package source {request}: {error}", file=sys.stderr)
        write(self.path, {**self.record, "phase": "closed"}, existing=self.record)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("acquire", "release", "create-fresh", "stamp-fresh", "validate-source", "release-fresh"))
    parser.add_argument("--cache", required=True, type=Path)
    parser.add_argument("--repo", type=Path)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--compiler")
    parser.add_argument("--target")
    parser.add_argument("--source", type=Path)
    parser.add_argument("--binding")
    parser.add_argument("--version")
    args = parser.parse_args()
    try:
        if args.mode == "acquire":
            if not args.repo or not args.compiler or not args.target:
                parser.error("acquire needs --repo, --compiler and --target")
            print(json.dumps(acquire(args.cache, args.repo, args.commit, args.compiler, args.target)))
        elif args.mode == "release":
            if not args.source:
                parser.error("release needs --source")
            release(args.cache, args.source, args.commit)
        elif args.mode == "create-fresh":
            if not args.repo or not args.source:
                parser.error("create-fresh needs --repo and --source")
            print(json.dumps(create_fresh(args.repo, args.source, args.commit)))
        else:
            if not args.binding or len(args.binding) > MAX_BYTES:
                parser.error(f"{args.mode} needs a bounded --binding")
            record = json.loads(args.binding)
            if not isinstance(record, dict) or record.get("schema") != 1:
                raise ValueError("unknown fresh source binding")
            if args.mode == "stamp-fresh":
                if not args.version:
                    parser.error("stamp-fresh needs --version")
                print(json.dumps(capture_stamps(record, args.version)))
            elif args.mode == "validate-source":
                verify_stamps(record) if "stamps" in record else validate_binding(record)
            else:
                release_fresh(args.cache, record)
    except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"alab: package source custody: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
