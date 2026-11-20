#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Explicit read-only GitHub transport for ALab's release/version gates.

Agh owns GitHub access. Git only reads local objects, transfers from admitted
local fixture repositories, and compares/updates local refs. No private
publication checkout, credential helper, or PATH replacement is required.
"""
import argparse
from contextlib import contextmanager
from contextvars import ContextVar
import fnmatch
from functools import cache, partial
import hashlib
import json
import os
from pathlib import Path
import re
import runpy
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
from urllib.parse import quote, unquote, urlsplit


OID = re.compile(r"[0-9a-f]{40}")
REF_COMPARE_FAILURE = re.compile(r"cannot lock ref '(refs/[^'\r\n]+)': "
                                 r"(?:is at [0-9a-f]{40} but expected [0-9a-f]{40}|reference already exists)(?=\s|$)")
ZERO = "0" * 40
MAX_FETCH_REFS = 2048
MAX_LOCAL_REFS = 8192
_GIT_FAILURE_STAGES = {
    "publication-init": "publication transport initialization",
    "publication-candidate-fetch": "publication candidate preparation",
}


class Refused(Exception):
    native_error = None


class TransferDiskStop(Refused):
    """The disk observer stopped this transfer."""


class TrackingRefConflict(Refused):
    def __init__(self, message, ref):
        super().__init__(message)
        self.ref = ref


CLI = runpy.run_path(str(Path(__file__).with_name("cli_status.py")))
_FILE_HASH_PATH = Path(__file__).resolve().with_name("file_hash.py")
successful_exit = CLI["successful_exit"]
failure_detail = CLI["failure_detail"]
report_failure = CLI["report_failure"]


def is_native_error(error, exit_status):
    """Recognize canonical integer metadata before trusting a native failure."""
    return (isinstance(error, dict) and type(error.get("v")) is int and error["v"] == 1
            and type(error.get("exit")) is int and error["exit"] == exit_status
            and (error.get("status") is None or type(error["status"]) is int)
            and error.get("side_effect") == "none")


def native_error_record(text):
    for candidate in [text] + text.splitlines():
        try:
            envelope = json.loads(candidate)
        except ValueError:
            continue
        if isinstance(envelope, dict) and isinstance(envelope.get("error"), dict):
            return envelope["error"]
    return {}


_transport_scope = ContextVar("transport_scope", default=None)
_SETTLE_SECONDS = 5


class _TransportScope:
    pending = False
    settled = True

    def request(self, _signum, _frame):
        self.pending = True

    def check(self):
        if self.pending:
            raise SystemExit(128 + signal.SIGTERM)
        if not self.settled:
            raise Refused("an earlier transport group has not settled")


@contextmanager
def finish_transport_on_termination():
    """Let the admitted synchronous command settle before retiring its scratch."""
    if os.name != "posix" or _transport_scope.get() is not None:
        yield
        return

    scope = _TransportScope()
    owns_term = (threading.current_thread() is threading.main_thread()
                 and signal.getsignal(signal.SIGTERM) == signal.SIG_DFL)
    if owns_term:
        previous = signal.signal(signal.SIGTERM, scope.request)
    token = _transport_scope.set(scope)
    try:
        yield
    finally:
        _transport_scope.reset(token)
        if owns_term:
            signal.signal(signal.SIGTERM, previous)
            if scope.pending:
                signal.raise_signal(signal.SIGTERM)


def _group_gone(pid):
    try:
        os.killpg(pid, 0)  # An existence probe, including after reaping. Never signal that group.
    except ProcessLookupError:
        return True
    except PermissionError:
        pass
    return False


def _can_signal_child():
    return threading.active_count() == 1 and signal.getsignal(signal.SIGCHLD) == signal.SIG_DFL


def _signal_owned(process, sig, *, group):
    # Popen can reap during communicate (also on KeyboardInterrupt). Signal only
    # our still-owned leader's private group, before its owning wait. A caller
    # with a custom SIGCHLD disposition may reap independently.
    if process.returncode is None and _can_signal_child():
        try:
            if hasattr(os, "waitid"):
                os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOWAIT | os.WNOHANG)
            (os.killpg if group else os.kill)(process.pid, sig)
        except (ChildProcessError, ProcessLookupError, PermissionError):
            pass


def _settle_group(process, *, interrupt):
    deadline = time.monotonic() + _SETTLE_SECONDS
    if interrupt:
        _signal_owned(process, signal.SIGKILL, group=True)
    try:
        process.wait(timeout=max(0, deadline - time.monotonic()))
    except subprocess.TimeoutExpired:
        return False
    while not _group_gone(process.pid):
        if time.monotonic() >= deadline:
            return False
        time.sleep(0.01)
    return True


def _read_fetch(process, data, timeout, check, scope):
    deadline = time.monotonic() + timeout
    while True:
        try:
            remaining = max(0, deadline - time.monotonic())
            return process.communicate(input=data, timeout=min(1, remaining))
        except subprocess.TimeoutExpired as expired:
            if time.monotonic() >= deadline:
                raise subprocess.TimeoutExpired(process.args, timeout, output=expired.output,
                                                stderr=expired.stderr) from None
        data = None  # communicate resumes the original buffered input and both output pipes.
        if scope.pending:
            raise SystemExit(128 + signal.SIGTERM)
        if check is not None:
            check()


def _stop_fetch(process, *, native):
    # Agh's Git workers have their own process groups. Ask Agh to retire them.
    # A local Git fetch instead owns the group containing its upload-pack.
    _signal_owned(process, signal.SIGTERM, group=not native)
    try:
        _, err = process.communicate(timeout=_SETTLE_SECONDS)
    except subprocess.TimeoutExpired:
        gone = _settle_group(process, interrupt=True)
        return gone and not native
    gone = _settle_group(process, interrupt=False)
    if not native or process.returncode == 0:
        return gone
    if process.returncode == 130:
        error = native_error_record(err.decode(errors="replace"))
        if is_native_error(error, 130) and error.get("kind") == "interrupted":
            return gone
    # A raw signal exit or an unknown failure gives no receipt for Agh's workers.
    return False


def run_transport(argv, *, capture_output, timeout, input=None, fetch_check=None,
                  native_fetch=False, **kwargs):
    """Run captured Git/Agh, settling its private group while it owns scratch."""
    scope = _transport_scope.get()
    if scope is None:
        if fetch_check is not None:
            raise Refused("guarded Git fetch requires a managed POSIX transport")
        return subprocess.run(argv, input=input, capture_output=capture_output, timeout=timeout, **kwargs)
    scope.check()
    if fetch_check is not None and not _can_signal_child():
        raise Refused("guarded Git transfer requires a single thread and the default SIGCHLD disposition")
    assert capture_output
    scope.settled = False
    try:
        process = subprocess.Popen(argv, stdin=subprocess.PIPE if input is not None else None,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   start_new_session=True, **kwargs)
    except OSError:
        # A failed launch returns only after Popen has retired its failed exec.
        scope.settled = True
        raise
    try:
        try:
            if native_fetch or fetch_check is not None:
                out, err = _read_fetch(process, input, timeout, fetch_check, scope)
            else:
                out, err = process.communicate(input=input, timeout=timeout)
        except BaseException:
            try:
                if native_fetch or fetch_check is not None:
                    scope.settled = _stop_fetch(process, native=native_fetch)
                else:
                    scope.settled = _settle_group(process, interrupt=True)
            except OSError:
                pass  # Preserve the primary error and keep the uncertain writer's scratch.
            raise
        group_gone = _settle_group(process, interrupt=False)
        scope.settled = group_gone
        if native_fetch and process.returncode != 0:
            # If Agh died independently, its separately grouped Git worker may
            # still hold scratch. Return the primary native error to its caller.
            error = native_error_record(err.decode(errors="replace"))
            scope.settled = group_gone and process.returncode > 0 and is_native_error(error, process.returncode)
        if not group_gone or scope.pending:
            scope.check()
        if fetch_check is not None and process.returncode == 0:
            fetch_check()
        return subprocess.CompletedProcess(argv, process.returncode, out, err)
    finally:
        for stream in (process.stdin, process.stdout, process.stderr):
            if stream is not None:
                stream.close()


class _ScratchDirectory:
    """An explicitly retired reservation. A live writer can outlast its caller."""

    def __init__(self, **kwargs):
        self.name = tempfile.mkdtemp(**kwargs)
        self.identity = os.lstat(self.name)
        self.fd = None
        if os.name == "posix":
            self.fd = os.open(self.name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            if not os.path.samestat(self.identity, os.fstat(self.fd)):
                self.close()
                raise Refused("temporary directory changed during allocation")

    def close(self):
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None

    def check_name(self):
        try:
            current = os.lstat(self.name)
        except FileNotFoundError:
            return False
        if not os.path.samestat(self.identity, current):
            raise Refused("temporary directory ownership changed")
        return True

    def cleanup(self):
        scope = _transport_scope.get()
        if scope is not None and not scope.settled:
            message = f"retained scratch with unsettled transport writers: {self.name}"
            if scope.pending:
                # Native TERM takes precedence over Python exception reporting.
                print(message, file=sys.stderr, flush=True)
            raise Refused(message)
        if not self.check_name():
            return
        if self.fd is None:
            shutil.rmtree(self.name)
            return
        # Walk relative to the pinned original directory. A renamed root can
        # never redirect traversal into replacement files at its old name.
        def fail(error):
            raise error
        for _, directories, files, parent in os.fwalk(".", topdown=False, onerror=fail, dir_fd=self.fd):
            for name in files:
                os.unlink(name, dir_fd=parent)
            for name in directories:
                if stat.S_ISLNK(os.stat(name, dir_fd=parent, follow_symlinks=False).st_mode):
                    os.unlink(name, dir_fd=parent)
                else:
                    os.rmdir(name, dir_fd=parent)
        if self.check_name():
            os.rmdir(self.name)


@contextmanager
def _owned_directory(**kwargs):
    directory = _ScratchDirectory(**kwargs)
    try:
        yield directory.name
    except BaseException as primary:
        try:
            directory.cleanup()
        except BaseException as cleanup:
            if isinstance(primary, Exception) and not isinstance(cleanup, Exception):
                raise cleanup from primary
            try:
                note = f"temporary directory cleanup failed: {cleanup!r}"
                if hasattr(primary, "add_note"):
                    primary.add_note(note)
                else:
                    # The macOS bootstrap runs on Python 3.9. Its CLI reads the same notes.
                    primary.__dict__.setdefault("__notes__", []).append(note)
            except BaseException as note_error:
                if isinstance(primary, Exception) and not isinstance(note_error, Exception):
                    raise note_error from primary
        raise
    else:
        directory.cleanup()
    finally:
        directory.close()


@contextmanager
def temporary_directory(**kwargs):
    """Retire owned scratch before delivering TERM or propagating an in-flight failure."""
    with finish_transport_on_termination(), _owned_directory(**kwargs) as path:
        yield path


def _git_reported_no_space(stderr):
    """A bounded diagnostic text signal, not an errno or volume observation."""
    if type(stderr) is not bytes:
        return False
    sample = stderr[-4096:]
    try:
        sample.decode("utf-8")
    except UnicodeDecodeError:
        return False
    return b"No space left on device" in sample


def git(repo, *args, data=None, codes=(0,), failure_stage=None, fetch_check=None, return_result=False):
    if failure_stage is not None and (type(failure_stage) is not str
                                     or failure_stage not in _GIT_FAILURE_STAGES):
        raise Refused("unsupported local Git failure stage")
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
               GIT_OPTIONAL_LOCKS="0", GIT_NO_LAZY_FETCH="1",
               GIT_NO_REPLACE_OBJECTS="1", GIT_GRAFT_FILE=os.devnull,
               GIT_TERMINAL_PROMPT="0")
    options = {"fetch_check": fetch_check} if fetch_check is not None else {}
    done = run_transport(
        ["git", "-c", "core.hooksPath=" + os.devnull, "-c", "core.fsmonitor=false",
         "-c", "protocol.allow=never", "-c", "protocol.file.allow=always",
         "-c", "maintenance.auto=false", "-c", "gc.auto=0", "-C", str(repo), *args],
        input=data, capture_output=True, env=env, timeout=120, **options)
    if done.returncode not in codes:
        stage = args[0] if failure_stage is None else _GIT_FAILURE_STAGES[failure_stage]
        message = "local Git " + stage + " failed (exit " + str(done.returncode) + ")"
        if failure_stage is not None and _git_reported_no_space(done.stderr):
            message += ": Git reported no space left on device"
        if args[0] == "update-ref":
            conflict = REF_COMPARE_FAILURE.search(done.stderr.decode(errors="replace"))
            if conflict:
                raise TrackingRefConflict(message + ": tracking reference changed during fetch", conflict[1])
        raise Refused(message)
    return done if return_result else done.stdout


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
    elif Path(remote).drive or not re.match(r"^[^/]+:", remote):
        candidate = Path(remote).expanduser()
        if not candidate.is_absolute():
            candidate = Path(repo) / candidate
    else:
        # URL schemes and SCP-style authorities are remote syntax even when
        # the checkout contains a directory with the same spelling.
        candidate = None
    if candidate is not None and candidate.is_dir():
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
        hash_file = runpy.run_path(str(_FILE_HASH_PATH))["file_sha256_snapshot"]
        metadata, self.sha256 = hash_file(self.executable, error_type=Refused)
        self.identity = (metadata.st_dev, metadata.st_ino, metadata.st_size,
                         metadata.st_mtime_ns, metadata.st_ctime_ns)
        help_text = self.call("help", "repo", "fetch").decode(errors="replace")
        if not all(flag in help_text for flag in ("--ref", "--into", "--input", "--anonymous")):
            raise Refused("Agh lacks exact anonymous batch fetch; select a qualified client with ALAB_AGH")

    def file_identity(self):
        meta = Path(self.executable).stat()
        return (meta.st_dev, meta.st_ino, meta.st_size, meta.st_mtime_ns, meta.st_ctime_ns)

    def call(self, *args, anonymous=False, absent=False, temp_root=None, fetch_check=None):
        if self.file_identity() != self.identity:
            raise Refused("selected Agh executable changed during the gate")
        flags = ["--anonymous"] if anonymous else []
        # Installed agh is a multi-call aterm link: bind the physical executable,
        # but retain agh as argv[0]. The caller's Git context is never inherited.
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        if temp_root is not None:
            env["TMPDIR"] = str(temp_root)
        # Every native batch owns separately grouped Git workers, including when
        # its caller only checks disk space at launch.
        options = {"native_fetch": True} if args[:2] == ("repo", "fetch") else {}
        if fetch_check is not None:
            options["fetch_check"] = fetch_check
        done = run_transport(["agh", *flags, *args], executable=self.executable,
                              capture_output=True, timeout=120, env=env, **options)
        if done.returncode:
            text = done.stderr.decode(errors="replace")
            error = native_error_record(text)
            typed = is_native_error(error, done.returncode)
            missing = (done.returncode == 6 and error.get("kind") == "not_found"
                       and type(error.get("status")) is int and error["status"] == 404)
            empty = (type(error.get("status")) is int and error["status"] == 409
                     and error.get("message") == "Git Repository is empty.")
            if absent and typed and (missing or empty):
                return None
            failure = Refused("Agh request failed (exit " + str(done.returncode) + "): " + text.strip())
            failure.native_error = error if typed else None
            raise failure
        return done.stdout

    def api(self, path, *, anonymous=False, absent=False, all_pages=False):
        flags = ("--all",) if all_pages else ()
        raw = self.call("api", path, *flags, anonymous=anonymous, absent=absent)
        try:
            return None if raw is None else json.loads(raw)
        except (ValueError, TypeError):
            raise Refused("Agh returned invalid JSON") from None

    def public_ref_records(self, identity, patterns):
        records = []
        for prefix in dict.fromkeys(pattern.split("*", 1)[0] for pattern in patterns):
            try:
                raw = self.call("repo", "public-refs", identity, "--prefix", prefix, anonymous=True)
            except Refused as error:
                # Older native clients enumerate through their paginated API.
                native = error.native_error or {}
                if (native.get("kind") == "usage" and native.get("exit") == 2
                        and native.get("why") in ("unknown flag --prefix", "unknown verb: repo public-refs")):
                    return None
                raise
            try:
                if raw and not raw.endswith(b"\n"):
                    raise ValueError
                for line in raw.decode("utf-8").splitlines():
                    oid, ref = line.split("\t")
                    if not OID.fullmatch(oid) or not ref.startswith(prefix):
                        raise ValueError
                    # Fetch compares the annotated tag object itself with its observation.
                    if not ref.endswith("^{}"):
                        records.append({"ref": ref, "object": {"sha": oid}})
            except ValueError:
                raise Refused("Agh returned a malformed public ref advertisement") from None
        return records

    def fetch_batch(self, identity, repo, rows, *, anonymous=False, scratch_parent=None,
                    fetch_check=None):
        with temporary_directory(prefix="codex-agh-fetch-", suffix=".noindex",
                                 dir=scratch_parent) as directory:
            path = Path(directory) / "refs.json"
            path.write_text(json.dumps(rows, sort_keys=True) + "\n")
            return self.call("repo", "fetch", identity, str(Path(repo).resolve()),
                             "--input", str(path), anonymous=anonymous,
                             temp_root=directory if scratch_parent is not None else None,
                             fetch_check=fetch_check)


def validate_ref_patterns(repo, patterns):
    # Use Git's pattern parser for wildcards. Inside a repository, literal
    # names share its existing NUL-delimited, aborted validation transaction.
    patterns = dict.fromkeys(patterns)
    # A POSIX argv can also carry undecodable filesystem bytes. Leave these
    # with the original argv parser until a matching name needs to be stored.
    literals = {name for name in patterns if name.startswith("refs/") and "*" not in name
                and not re.search(r"[\ud800-\udfff]", name)}
    if len(literals) > 1 and git(repo, "rev-parse", "--git-dir", codes=(0, 128)):
        validate_full_refs(repo, literals)
        for name in literals:
            del patterns[name]
    for name in patterns:
        git(repo, "check-ref-format", "--refspec-pattern", name)


def list_refs(repo, remote, patterns, *, anonymous=False, client=None):
    identity, network = location(repo, remote)
    patterns = tuple(patterns)
    validate_ref_patterns(repo, patterns)
    literals = {pattern for pattern in patterns if "*" not in pattern}
    wildcards = [pattern for pattern in patterns if "*" in pattern]
    found = {}
    if not network:
        raw = git(repo, "ls-remote", "--refs", identity)
        records = [{"ref": ref, "object": {"sha": oid}}
                   for oid, ref in (line.split("\t", 1) for line in raw.decode().splitlines())]
    else:
        client = client or Agh()
        public_refs = getattr(client, "public_ref_records", None)
        records = public_refs(identity, patterns) if anonymous and public_refs else None
        if records is None:
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
            if ref in literals or any(fnmatch.fnmatchcase(ref, pattern) for pattern in wildcards):
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


def git_common_dir(repo):
    raw = git(repo, "rev-parse", "--path-format=absolute", "--git-common-dir")
    if not raw.endswith(b"\n") or b"\0" in raw:
        raise Refused("Git returned an invalid common directory")
    # Git terminates the path with one LF. Any preceding whitespace belongs to the name.
    path = Path(os.fsdecode(raw[:-1]))
    if not path.is_absolute() or not path.is_dir():
        raise Refused("Git did not return an absolute common directory")
    return path.resolve(strict=True)


def guard_ref(repo, ref, common=None):
    repo = Path(repo).resolve(strict=True)
    if common is None:
        common = git_common_dir(repo)
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


def admit_transfer_monitor(admission, common, scratch_parent, *, before_transfer=None):
    """Admit the writer and compose its bound monitor with the caller's own check."""
    monitor = admission(common, scratch_parent) if admission is not None else None
    monitor = monitor if callable(monitor) else None
    if before_transfer is None:
        return monitor
    check = partial(before_transfer, common, scratch_parent)
    check()
    if monitor is None:
        return check

    def observe():
        monitor()
        check()

    return observe


def fetch_authenticated_batch(client, identity, repo, rows, common, anonymous, before_transfer,
                              *, admit_transfer=None):
    def transfer():
        options = {}
        if before_transfer is not None or admit_transfer is not None:
            # Agh can stage the remote pack in its own temporary bare repo before
            # importing it into common. Give the child a fresh directory on the
            # volume the caller checked, including when Python and Agh choose
            # different defaults for their temporary files.
            scratch_parent = Path(tempfile.gettempdir())
            if admit_transfer is not None:
                scratch_parent = scratch_parent.resolve(strict=True)
            options["scratch_parent"] = scratch_parent
            check = admit_transfer_monitor(admit_transfer, common, scratch_parent, before_transfer=before_transfer)
            if check is not None:
                options["fetch_check"] = check
        client.fetch_batch(identity, repo, rows, anonymous=anonymous, **options)

    try:
        transfer()
    except Refused as failure:
        detail = failure.native_error
        if not (is_native_error(detail, 3) and detail.get("kind") == "could_not_run"
                and detail.get("sub") == "git" and detail.get("retriable") is True):
            raise
        # A parallel fetch can have won Agh's compare-and-swap on a shared
        # transfer ref. Revalidate the whole batch with its original object IDs.
        why = detail.get("why")
        conflict = REF_COMPARE_FAILURE.search(why) if isinstance(why, str) else None
        if not conflict or not any(row["into"] == conflict[1]
                                   and current_ref(repo, row["into"]) == row["expected_oid"] for row in rows):
            raise
        for row in rows:
            guard_ref(repo, row["into"], common)
        transfer()


def update_tracking_refs(repo, common, updates):
    def commit(expected):
        commands = ["start"]
        for target, (oid, old) in sorted(expected.items()):
            guard_ref(repo, target, common)
            commands.append("option no-deref")
            if oid == old:
                commands.append(f"verify {target} {oid}")
            else:
                commands.append(f"update {target} {oid} {old}")
        commands.extend(["prepare", "commit", ""])
        git(repo, "update-ref", "-m", "ALab Agh release fetch", "--stdin",
            data="\n".join(commands).encode())

    while True:
        try:
            commit(updates)
            return
        except TrackingRefConflict as conflict:
            if conflict.ref not in updates:
                raise
            oid, old = updates[conflict.ref]
            if oid == old:
                raise
            current = local_refs(repo)
            if current.get(conflict.ref) != oid:
                raise
            # Each retry converts at least the conflicting update to a verify.
            # A changed verify or an unrelated failure immediately ends the batch.
            updates = {ref: (oid, oid if current.get(ref) == oid else old)
                       for ref, (oid, old) in updates.items()}


def missing_local_transfers(repo, transfers, current):
    cached = {oid for oid, slot in transfers.values() if current.get(slot) == oid}
    available = set()
    if cached:
        # A restored ref can precede its object. Let an actual fetch repair it.
        objects = "".join(oid + "\n" for oid in sorted(cached)).encode("ascii")
        available = set(git(repo, "cat-file", "--batch-check=%(objectname)",
                            data=objects).decode("ascii").splitlines())
    return {ref: (oid, slot) for ref, (oid, slot) in transfers.items()
            if current.get(slot) != oid or oid not in available}


def local_fetch_conflict(done):
    """Recognize only Git's local incorrect-old-value status and a single exact diagnosis."""
    if done.returncode != 251 or len(done.stderr) > 4096:
        return None
    lines = [line for line in done.stderr.splitlines() if line and not line.startswith(b"hint:")]
    if len(lines) != 1:
        return None
    return re.fullmatch(r"error: cannot lock ref '(refs/agh/codex-release-[0-9a-f]{24})': "
                        r"is at ([0-9a-f]{40}) but expected ([0-9a-f]{40})",
                        lines[0].decode("ascii", errors="replace"))


def fetch_local_batch(repo, identity, transfers, before, common, patterns, expected, before_transfer, admit_transfer):
    def transfer(rows):
        batch = "".join(f"+{ref}:{slot}\n" for ref, (_, slot) in rows.items()).encode("utf-8")
        args = ("fetch", "--quiet", "--atomic", "--no-tags", "--no-write-fetch-head",
                "--recurse-submodules=no", "--stdin", identity)
        check = admit_transfer_monitor(admit_transfer, common, None, before_transfer=before_transfer)
        managed = check is not None and (before_transfer is not None or _transport_scope.get() is not None)
        if managed:
            with finish_transport_on_termination():
                result = git(repo, *args, data=batch, codes=(0, 251), return_result=True, fetch_check=check)
        else:
            # The unscoped writer stays in the package custodian's inherited group.
            result = git(repo, *args, data=batch, codes=(0, 251), return_result=True)
        if result.returncode:
            conflict = local_fetch_conflict(result)
            if conflict is None:
                raise Refused(f"local Git fetch failed (exit {result.returncode})")
            # Git skipped the successful writer's post-check on this conflict.
            if check is not None:
                check()
            return conflict, check
        if check is not None and not managed:
            check()
        return None

    remaining, prior = transfers, before
    failure = Refused("local Git fetch failed (exit 251)")
    for _attempt in range(2):
        outcome = transfer(remaining)
        if outcome is None:
            return
        conflict, check = outcome
        slot, found, previous = conflict.groups()
        if not any((oid, name) == (found, slot) for oid, name in remaining.values()) or prior.get(slot) != previous:
            raise failure
        # The peer may have raced a fetch of a different head. Check every original
        # named source before reusing any local slot, even for a supplied snapshot.
        observed = list_refs(repo, identity, patterns)
        if any(observed.get(ref) != oid for ref, oid in expected.items()):
            raise failure
        for _, name in transfers.values():
            guard_ref(repo, name, common)
        current = local_refs(repo)
        if current.get(slot) != found or any(current.get(name) not in (oid, prior.get(name))
                                             for oid, name in transfers.values()):
            raise failure
        remaining = missing_local_transfers(repo, transfers, current)
        if not remaining:
            if check is not None:
                check()
            return
        prior = current
    raise failure


def fetch(repo, remote, refspecs, *, anonymous=False, snapshot=None, client=None,
          snapshot_refs=False, before_transfer=None, admit_transfer=None):
    """Observe refs and check available space before and during each guarded transfer."""
    # Git exit statuses are also used to validate metadata before importing any objects.
    if before_transfer is not None and os.name == "posix" and signal.getsignal(signal.SIGCHLD) != signal.SIG_DFL:
        raise Refused("guarded Git fetch requires the default SIGCHLD disposition")
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
        specs.append((source, destination, force))
    if any(not dest.startswith(("refs/remotes/", "refs/upstream-tags/")) for _, dest, _ in specs):
        if not snapshot_refs or git(repo, "rev-parse", "--is-bare-repository").strip() != b"true":
            raise Refused("release fetch only updates metadata or refs in an owned bare snapshot repository")
    validate_ref_patterns(repo, (name for source, dest, _ in specs for name in (source, dest)))
    client = client or (Agh() if network else None)
    observed = (list_refs(repo, remote, [s for s, _, _ in specs], anonymous=anonymous, client=client)
                if snapshot is None else snapshot)
    if len(observed) > MAX_FETCH_REFS:
        raise Refused("reference snapshot exceeds Agh's 2048-reference batch contract")
    common = git_common_dir(repo)
    before = local_refs(repo)

    def transfer_slot(ref):
        return "refs/agh/codex-release-" + hashlib.sha256((identity + "\n" + ref).encode()).hexdigest()[:24]

    updates = {}
    desired = {}
    pending = {}
    for source, destination, force in specs:
        if "*" in source:
            matched = [ref for ref in observed if fnmatch.fnmatchcase(ref, source)]
        elif source in observed:
            matched = [source]
        else:
            raise Refused("remote has no required reference " + source)
        for ref in matched:
            oid = observed[ref]
            if not isinstance(oid, str) or not OID.fullmatch(oid):
                raise Refused("snapshot has an invalid object ID")
            prefix, wildcard, suffix = source.partition("*")
            middle = ref[len(prefix):len(ref) - len(suffix) if suffix else None] if wildcard else ""
            target = destination.replace("*", middle)
            if target in desired and desired[target] != (oid, force, ref):
                raise Refused("different source references or force policies map to one fetch destination")
            desired[target] = (oid, force, ref)
    # Local fetch refspecs are transmitted one per line. Validate both sides
    # even for a caller-supplied reference snapshot.
    validate_full_refs(repo, desired.keys() | {ref for _oid, _force, ref in desired.values()})
    for target, (oid, force, ref) in desired.items():
        guard_ref(repo, target, common)
        old = before.get(target, ZERO)
        updates[target] = (oid, old)
        if old == oid:
            continue
        if old != ZERO and target.startswith("refs/tags/"):
            raise Refused("an existing public release tag is immutable")
        slot = transfer_slot(ref)
        guard_ref(repo, slot, common)
        pending[target] = (oid, old, force, ref, slot)
    if network:
        head_slots = {}
        for oid, _force, ref in desired.values():
            if ref.startswith("refs/heads/"):
                slot = transfer_slot(ref)
                if before.get(slot, ZERO) == oid:
                    head_slots.setdefault(oid, slot)
        rows = {}
        for target, (oid, old, force, ref, slot) in pending.items():
            source = ref
            if ref.startswith("refs/heads/"):
                # Fetch the freshly enumerated branch snapshot, even if its
                # name advances before transport. Tags keep name validation.
                slot = head_slots.setdefault(oid, slot)
                guard_ref(repo, slot, common)
                pending[target] = (oid, old, force, ref, slot)
                source = oid
            # Share head transfers within this batch, retaining bounded,
            # ref-keyed cache names and the original destination CAS values.
            if before.get(slot, ZERO) != oid:
                rows[source] = {"source": source, "into": slot, "expected_oid": oid}
        if rows:
            fetch_authenticated_batch(client, identity, repo, [rows[source] for source in sorted(rows)],
                                      common, anonymous, before_transfer, admit_transfer=admit_transfer)
    else:
        # A source ref has one private slot even when several tracking refs need it.
        # Only a fresh local advertisement can reuse a slot across calls. A supplied
        # snapshot still goes through the named-ref transfer and its final OID check.
        transfers = {ref: (oid, slot) for oid, _old, _force, ref, slot in pending.values()}
        if snapshot is None:
            transfers = missing_local_transfers(repo, transfers, before)
        if transfers:
            expected = {ref: oid for oid, _force, ref in desired.values()}
            fetch_local_batch(repo, identity, transfers, before, common, [s for s, _, _ in specs],
                              expected, before_transfer, admit_transfer)
    after = local_refs(repo)
    for oid, old, force, _ref, slot in pending.values():
        if after.get(slot, ZERO) != oid:
            raise Refused("fetched reference differs from its enumerated object ID; retry a fresh snapshot")
        if old != ZERO and not force:
            git(repo, "merge-base", "--is-ancestor", old, oid)
    if pending:
        update_tracking_refs(repo, common, updates)
    return observed


@cache
def _release_guard():
    return runpy.run_path(str(Path(__file__).resolve().with_name("command_guard.py")))


def admit_release_transfer(common, scratch_parent):
    """Admit this writer and return the check bound to its launch volumes."""
    guard = _release_guard()
    objects = common / "objects"
    paths = [common, objects, objects / "pack"]
    if scratch_parent is not None:
        paths.append(scratch_parent)
    floor = guard["DiskFloor"](paths, 20)

    def check():
        try:
            floor.check()
        except guard["DiskSpaceError"] as error:
            raise TransferDiskStop(str(error)) from error

    check()
    return check


def report_transfer_failure(prefix, error):
    status = report_failure(prefix, error)
    return 75 if isinstance(error, TransferDiskStop) else status


def restore_cli_sigchld():
    # The executable owns its child waits. SIG_IGN can survive exec.
    if os.name == "posix" and signal.getsignal(signal.SIGCHLD) == signal.SIG_IGN:
        signal.signal(signal.SIGCHLD, signal.SIG_DFL)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default=os.getcwd())
    parser.add_argument("--anonymous", action="store_true")
    parser.add_argument("operation", choices=("fetch",))
    parser.add_argument("remote")
    parser.add_argument("refspecs", nargs="+")
    args = parser.parse_args()
    try:
        fetch(args.repo, args.remote, args.refspecs, anonymous=args.anonymous,
              admit_transfer=admit_release_transfer if os.name == "posix" else None)
    except SystemExit as error:
        if not successful_exit(error):
            raise
        return report_failure("ALab GitHub read refused: ", error, incomplete=True)
    except (Refused, OSError, subprocess.TimeoutExpired) as error:
        return report_transfer_failure("ALab GitHub read refused: ", error)
    return 0


if __name__ == "__main__":
    restore_cli_sigchld()
    sys.exit(main())
