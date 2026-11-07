#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Explicit read-only GitHub transport for ALab's release/version gates.

Agh owns GitHub access. Git only reads local objects, transfers from admitted
local fixture repositories, and compares/updates local refs. No private
publication checkout, credential helper, or PATH replacement is required.
"""
import argparse
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from urllib.parse import quote, unquote, urlsplit


OID = re.compile(r"[0-9a-f]{40}")
ZERO = "0" * 40
MAX_FETCH_REFS = 2048
MAX_LOCAL_REFS = 8192


class Refused(Exception):
    pass


def git(repo, *args, data=None, codes=(0,)):
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
               GIT_OPTIONAL_LOCKS="0", GIT_NO_LAZY_FETCH="1",
               GIT_NO_REPLACE_OBJECTS="1", GIT_TERMINAL_PROMPT="0")
    done = subprocess.run(
        ["git", "-c", "core.hooksPath=" + os.devnull, "-c", "core.fsmonitor=false",
         "-c", "protocol.allow=never", "-c", "protocol.file.allow=always",
         "-c", "maintenance.auto=false", "-c", "gc.auto=0", "-C", str(repo), *args],
        input=data, capture_output=True, env=env, timeout=120)
    if done.returncode not in codes:
        raise Refused("local Git " + args[0] + " failed (exit " + str(done.returncode) + ")")
    return done.stdout


def location(repo, remote, named=True):
    if named and not any(char in remote for char in ":/@"):
        value = git(repo, "config", "--get", "remote." + remote + ".url", codes=(0, 1)).decode().strip()
        if value:
            return location(repo, value, named=False)
    if remote.startswith("file:"):
        parsed = urlsplit(remote)
        if (parsed.scheme != "file" or parsed.netloc not in ("", "localhost")
                or parsed.query or parsed.fragment or not parsed.path.startswith("/")):
            raise Refused("a local transfer requires an existing local repository")
        candidate = Path(unquote(parsed.path))
    else:
        candidate = Path(remote).expanduser()
        if not candidate.is_absolute():
            candidate = Path(repo) / candidate
    if candidate.is_dir():
        return str(candidate.resolve()), False
    value = remote
    if value.startswith("git@") and ":" in value:
        host, path = value[4:].split(":", 1)
        if host.lower() not in ("github.com", "gh-" + path.split("/", 1)[0].lower()):
            raise Refused("GitHub reads require an admitted repository URL")
        value = path
    elif value.startswith(("https://", "ssh://")):
        try:
            parsed = urlsplit(value)
            owner_alias = "gh-" + parsed.path.lstrip("/").split("/", 1)[0].lower()
            allowed_hosts = ("github.com", owner_alias) if parsed.scheme == "ssh" else ("github.com",)
            if (parsed.hostname not in allowed_hosts
                    or parsed.password or parsed.port is not None or parsed.query or parsed.fragment
                    or (parsed.scheme == "https" and parsed.username)
                    or (parsed.scheme == "ssh" and parsed.username != "git")):
                raise ValueError
            value = parsed.path.lstrip("/")
        except ValueError:
            raise Refused("GitHub reads require an admitted repository URL") from None
    value = value.removesuffix(".git")
    if (not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*", value)
            or any(part in (".", "..") for part in value.split("/"))):
        raise Refused("GitHub reads require an explicit OWNER/REPOSITORY")
    return value, True


class Agh:
    def __init__(self):
        selected = os.environ.get("ALAB_AGH")
        if selected and not Path(selected).is_absolute():
            raise Refused("ALAB_AGH must name an absolute qualified executable")
        found = selected or shutil.which("agh")
        if not found or not os.access(found, os.X_OK):
            raise Refused("Agh is unavailable; GitHub reads never fall back to raw Git")
        self.executable = str(Path(found).resolve(strict=True))
        self.identity = self.file_identity()
        self.sha256 = hashlib.sha256(Path(self.executable).read_bytes()).hexdigest()
        help_text = self.call("help", "repo", "fetch").decode(errors="replace")
        if not all(flag in help_text for flag in ("--ref", "--into", "--input", "--anonymous")):
            raise Refused("Agh lacks exact anonymous batch fetch; select a qualified client with ALAB_AGH")

    def file_identity(self):
        meta = Path(self.executable).stat()
        return (meta.st_dev, meta.st_ino, meta.st_size, meta.st_mtime_ns, meta.st_ctime_ns)

    def call(self, *args, anonymous=False, absent=False):
        if self.file_identity() != self.identity:
            raise Refused("selected Agh executable changed during the gate")
        flags = ["--anonymous"] if anonymous else []
        # Installed agh is a multi-call aterm link: bind the physical executable,
        # but retain agh as argv[0]. The caller's Git context is never inherited.
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        done = subprocess.run(["agh", *flags, *args], executable=self.executable,
                              capture_output=True, timeout=120, env=env)
        if done.returncode:
            error = {}
            text = done.stderr.decode(errors="replace")
            for candidate in [text] + text.splitlines():
                try:
                    envelope = json.loads(candidate)
                    if isinstance(envelope, dict) and isinstance(envelope.get("error"), dict):
                        error = envelope["error"]
                        break
                except ValueError:
                    pass
            typed = (error.get("v") == 1 and error.get("exit") == done.returncode
                     and error.get("side_effect") == "none")
            missing = (done.returncode == 6 and error.get("kind") == "not_found"
                       and error.get("status") == 404)
            empty = (error.get("status") == 409 and error.get("message") == "Git Repository is empty.")
            if absent and typed and (missing or empty):
                return None
            raise Refused("Agh read failed (exit " + str(done.returncode) + "): " + text.strip())
        return done.stdout

    def api(self, path, *, anonymous=False, absent=False, all_pages=False):
        flags = ("--all",) if all_pages else ()
        raw = self.call("api", path, *flags, anonymous=anonymous, absent=absent)
        try:
            return None if raw is None else json.loads(raw)
        except (ValueError, TypeError):
            raise Refused("Agh returned invalid JSON") from None

    def fetch_batch(self, identity, repo, rows, *, anonymous=False):
        with tempfile.TemporaryDirectory(prefix="codex-agh-fetch-", suffix=".noindex") as directory:
            path = Path(directory) / "refs.json"
            path.write_text(json.dumps(rows, sort_keys=True) + "\n")
            return self.call("repo", "fetch", identity, str(Path(repo).resolve()),
                             "--input", str(path), anonymous=anonymous)


def list_refs(repo, remote, patterns, *, anonymous=False, client=None):
    identity, network = location(repo, remote)
    for pattern in patterns:
        git(repo, "check-ref-format", "--refspec-pattern", pattern)
    found = {}
    if not network:
        raw = git(repo, "ls-remote", "--refs", identity)
        records = [{"ref": ref, "object": {"sha": oid}}
                   for oid, ref in (line.split("\t", 1) for line in raw.decode().splitlines())]
    else:
        client = client or Agh()
        records = []
        for pattern in patterns:
            prefix = pattern.split("*", 1)[0]
            values = client.api("/repos/" + identity + "/git/matching-refs/"
                                + quote(prefix[5:], safe="/"), anonymous=anonymous,
                                absent=True, all_pages=True)
            if values is None:
                values = []
            if not isinstance(values, list):
                raise Refused("Agh returned an invalid reference listing")
            records.extend(values)
        if not records:
            repository = client.api("/repos/" + identity, anonymous=anonymous)
            if (not isinstance(repository, dict) or not isinstance(repository.get("full_name"), str)
                    or repository["full_name"].lower() != identity.lower()):
                raise Refused("Agh returned a mismatched repository identity")
    try:
        for record in records:
            ref, oid = record["ref"], record["object"]["sha"]
            if not isinstance(ref, str) or not isinstance(oid, str) or not OID.fullmatch(oid):
                raise ValueError
            if any(fnmatch.fnmatchcase(ref, pattern) for pattern in patterns):
                if ref in found and found[ref] != oid:
                    raise Refused("remote reference listing changed within one snapshot")
                found[ref] = oid
                if len(found) > MAX_FETCH_REFS:
                    raise Refused("reference snapshot exceeds Agh's 2048-reference batch contract")
    except (KeyError, TypeError, ValueError):
        raise Refused("Agh returned a malformed reference identity") from None
    validate_full_refs(repo, found)
    return found


def current_ref(repo, ref):
    raw = git(repo, "rev-parse", "--verify", "--quiet", ref, codes=(0, 1))
    oid = raw.decode().strip()
    if oid and not OID.fullmatch(oid):
        raise Refused("local reference has an invalid object identity")
    return oid or ZERO


def validate_full_refs(repo, refs):
    """Use Git's ref parser once, without preparing or committing any update.

    A release can enumerate thousands of tags. Spawning check-ref-format for
    every source and destination exhausts the whole fetch deadline even when
    every object is already local. NUL-delimited verify commands preserve each
    name as data; abort leaves existing refs and their reflogs untouched.
    """
    refs = list(refs)
    if any(not isinstance(ref, str) or not ref.startswith("refs/") or "\0" in ref
           for ref in refs):
        raise Refused("reference validation needs full, NUL-free ref names")
    refs = sorted(set(refs))
    if not refs:
        return
    commands = [b"start\0"]
    for ref in refs:
        try:
            name = ref.encode("utf-8")
        except UnicodeEncodeError:
            raise Refused("reference name is not valid UTF-8") from None
        commands.append(b"verify " + name + b"\0" + ZERO.encode("ascii") + b"\0")
    commands.append(b"abort\0")
    git(repo, "update-ref", "--stdin", "-z", data=b"".join(commands))


def local_refs(repo):
    raw = git(repo, "for-each-ref", "--count=" + str(MAX_LOCAL_REFS + 1),
              "--format=%(refname) %(objectname)", "refs/")
    lines = raw.decode().splitlines()
    if len(lines) > MAX_LOCAL_REFS:
        raise Refused("local reference snapshot exceeds its bounded release-reader limit")
    found = {}
    for line in lines:
        ref, oid = line.split(" ", 1)
        if not ref.startswith("refs/") or not OID.fullmatch(oid):
            raise Refused("local reference snapshot has an invalid identity")
        found[ref] = oid
    return found


def guard_ref(repo, ref, common=None):
    repo = Path(repo).resolve(strict=True)
    if common is None:
        common = Path(git(repo, "rev-parse", "--path-format=absolute", "--git-common-dir").decode().strip()).resolve()
    for name in (ref, "logs/" + ref):
        # The admitted destinations are full refs under the shared refs/logs
        # namespace, including in linked worktrees. Discover that root once;
        # repeated Git invocations per tag otherwise consume the network gate.
        path = Path(os.path.abspath(common / name))
        if not path.is_relative_to(common):
            raise Refused("local ref path escapes the repository")
        cursor = common
        for part in path.relative_to(common).parts:
            cursor /= part
            try:
                meta = cursor.lstat()
            except FileNotFoundError:
                continue
            if stat.S_ISLNK(meta.st_mode) or (stat.S_ISREG(meta.st_mode) and meta.st_nlink != 1):
                raise Refused("local ref path has a symbolic or hard-linked component")
            if not stat.S_ISDIR(meta.st_mode) and not stat.S_ISREG(meta.st_mode):
                raise Refused("local ref path contains a special file")
        if path.is_file() and name == ref:
            with path.open("rb") as handle:
                if handle.read(5).startswith(b"ref:"):
                    raise Refused("local fetch destination is a symbolic ref")


def fetch(repo, remote, refspecs, *, anonymous=False, snapshot=None, client=None,
          snapshot_refs=False):
    identity, network = location(repo, remote)
    specs = []
    for spec in refspecs:
        force = spec.startswith("+")
        source, separator, destination = spec.removeprefix("+").partition(":")
        if (not separator or not source.startswith("refs/") or not destination.startswith("refs/")
                or source.count("*") != destination.count("*") or source.count("*") > 1):
            raise Refused("fetch needs explicit paired full-reference refspecs")
        if force and not destination.startswith("refs/upstream-tags/"):
            raise Refused("forced local fetch is restricted to upstream release-tag metadata")
        if not destination.startswith(("refs/remotes/", "refs/upstream-tags/")):
            if not snapshot_refs or git(repo, "rev-parse", "--is-bare-repository").strip() != b"true":
                raise Refused("release fetch only updates metadata or refs in an owned bare snapshot repository")
        git(repo, "check-ref-format", "--refspec-pattern", source)
        git(repo, "check-ref-format", "--refspec-pattern", destination)
        specs.append((source, destination, force))
    client = client or (Agh() if network else None)
    observed = (list_refs(repo, remote, [s for s, _, _ in specs], anonymous=anonymous, client=client)
                if snapshot is None else snapshot)
    if len(observed) > MAX_FETCH_REFS:
        raise Refused("reference snapshot exceeds Agh's 2048-reference batch contract")
    common = Path(git(repo, "rev-parse", "--path-format=absolute", "--git-common-dir").decode().strip()).resolve()
    before = local_refs(repo)
    updates = {}
    desired = {}
    pending = {}
    for source, destination, force in specs:
        matched = [ref for ref in observed if fnmatch.fnmatchcase(ref, source)]
        if "*" not in source and not matched:
            raise Refused("remote has no required reference " + source)
        for ref in matched:
            oid = observed[ref]
            if not isinstance(oid, str) or not OID.fullmatch(oid):
                raise Refused("snapshot has an invalid object ID")
            prefix, wildcard, suffix = source.partition("*")
            middle = ref[len(prefix):len(ref) - len(suffix) if suffix else None] if wildcard else ""
            target = destination.replace("*", middle)
            if target in desired and desired[target][0] != oid:
                raise Refused("different source references map to one fetch destination")
            desired[target] = (oid, force, ref)
    validate_full_refs(repo, desired)
    for target, (oid, force, ref) in desired.items():
        guard_ref(repo, target, common)
        old = before.get(target, ZERO)
        if old == oid:
            continue
        if old != ZERO and target.startswith("refs/tags/"):
            raise Refused("an existing public release tag is immutable")
        slot = "refs/agh/codex-release-" + hashlib.sha256((identity + "\n" + ref).encode()).hexdigest()[:24]
        guard_ref(repo, slot, common)
        pending[target] = (oid, old, force, ref, slot)
        updates[target] = (oid, old)
    if network:
        rows = {}
        for oid, _old, _force, ref, slot in pending.values():
            # A cold graph may be prewarmed through this exact native batch.
            # Fresh enumeration remains authoritative on every canonical run;
            # only cache refs with the same raw OID avoid another transfer.
            if before.get(slot, ZERO) != oid:
                rows[ref] = {"source": ref, "into": slot, "expected_oid": oid}
        if rows:
            client.fetch_batch(identity, repo, [rows[ref] for ref in sorted(rows)], anonymous=anonymous)
    for oid, _old, _force, ref, slot in pending.values():
        if not network:
            git(repo, "fetch", "--quiet", "--no-tags", "--no-write-fetch-head",
                "--recurse-submodules=no", identity, "+" + ref + ":" + slot)
    after = local_refs(repo)
    for oid, old, force, _ref, slot in pending.values():
        if after.get(slot, ZERO) != oid:
            raise Refused("fetched reference differs from its enumerated object ID; retry a fresh snapshot")
        if old != ZERO and not force:
            git(repo, "merge-base", "--is-ancestor", old, oid)
    if updates:
        transaction = "start\noption no-deref\n"
        for target, (oid, old) in sorted(updates.items()):
            guard_ref(repo, target, common)
            transaction += "update " + target + " " + oid + " " + old + "\n"
        transaction += "prepare\ncommit\n"
        git(repo, "update-ref", "-m", "ALab Agh release fetch", "--stdin", data=transaction.encode())
    return observed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default=os.getcwd())
    parser.add_argument("--anonymous", action="store_true")
    parser.add_argument("operation", choices=("fetch",))
    parser.add_argument("remote")
    parser.add_argument("refspecs", nargs="+")
    args = parser.parse_args()
    fetch(args.repo, args.remote, args.refspecs, anonymous=args.anonymous)


if __name__ == "__main__":
    try:
        main()
    except (Refused, OSError, subprocess.TimeoutExpired) as error:
        print("ALab GitHub read refused: " + str(error), file=sys.stderr)
        sys.exit(1)
