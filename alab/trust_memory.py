#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Hold native-selected host trustd across a protected build.

This owns startup/lifetime only. Verified Targo still authenticates the daemon,
checks its budget and takes its own session lease. No compiler socket override,
alternate memory domain, recovery or signal is used.
"""

import argparse
import ctypes
import errno
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import threading
import time


PROFILE_SOCKET = "ALAB_TRUSTD_SOCKET"


def host_endpoint():
    # Trust's documented host_socket_path contract deliberately ignores TMPDIR.
    return Path("/tmp").resolve() / f"trustd-runtime-locks-{os.geteuid()}" / "trust-memory-jobserver.sock"


def validate_endpoint(path):
    root = path.parent
    metadata = root.lstat()
    if (root.resolve() != root or not stat.S_ISDIR(metadata.st_mode)
            or metadata.st_uid != os.geteuid() or stat.S_IMODE(metadata.st_mode) != 0o700):
        raise ValueError("Trust host runtime root must be an unaliased euid-owned 0700 directory")
    metadata = path.lstat()
    if (not stat.S_ISSOCK(metadata.st_mode) or metadata.st_uid != os.geteuid()
            or stat.S_IMODE(metadata.st_mode) & 0o077):
        raise ValueError("Trust host endpoint must be an euid-owned private socket")


def profile_socket(value):
    path = Path(value)
    if not path.is_absolute() or path != host_endpoint():
        raise ValueError("profile memory endpoint differs from Trust's fixed host endpoint")
    validate_endpoint(path)
    return path


def selected_identity(execution):
    execution = execution.resolve(strict=True)
    daemon = execution / "bin/trustd"
    metadata = daemon.lstat()
    if (not stat.S_ISREG(metadata.st_mode) or metadata.st_mode & 0o022
            or metadata.st_uid not in (0, os.geteuid())):
        raise ValueError("selected trustd must be a regular owned non-writable executable")
    digest = hashlib.sha256(daemon.read_bytes()).hexdigest()
    version = subprocess.check_output([str(daemon), "--version"], env={}, text=True, timeout=10)
    lines = version.splitlines()
    fields = dict(line.split(": ", 1) for line in lines if ": " in line)
    values = dict(line.split("=", 1) for line in lines if "=" in line)
    if (not lines or not lines[0].startswith("trustd ") or values.get("trust.identity") != "trustd"
            or values.get("trust.protocol") != "trustd.status.v1"
            or not re.fullmatch(r"[0-9a-f]{40}", fields.get("commit-hash", ""))
            or hashlib.sha256(daemon.read_bytes()).hexdigest() != digest):
        raise ValueError("selected trustd has no stable supported build identity")
    return daemon, {"version": "trustd.identity.v1", "protocol": "trustd.status.v1",
                    "release": lines[0].split(" ", 1)[1], "commit": fields["commit-hash"],
                    "executable_sha256": digest}


def exchange(connection, stream, request):
    connection.sendall(request.encode() + b"\n")
    line = stream.readline(4097)
    if len(line) > 4096 or not line.endswith(b"\n"):
        raise ValueError("Trust memory authority reply is missing, unframed or oversized")
    return line[:-1]


def peer_process(connection, daemon, expected_sha256):
    """Bind protocol data to the kernel peer and the exact selected executable."""
    if sys.platform == "darwin":
        # sys/un.h: SOL_LOCAL=0, LOCAL_PEERPID=2; pid_t is a native 32-bit int.
        raw = connection.getsockopt(0, 2, 4)
        if len(raw) != 4:
            raise ValueError("Trust memory peer PID has an unsupported kernel shape")
        pid = struct.unpack("=i", raw)[0]
        libc = ctypes.CDLL(None, use_errno=True)
        uid, gid = ctypes.c_uint(), ctypes.c_uint()
        libc.getpeereid.argtypes = [ctypes.c_int, ctypes.POINTER(ctypes.c_uint), ctypes.POINTER(ctypes.c_uint)]
        libc.getpeereid.restype = ctypes.c_int
        if libc.getpeereid(connection.fileno(), ctypes.byref(uid), ctypes.byref(gid)) != 0:
            raise OSError(ctypes.get_errno(), "cannot obtain Trust memory peer credentials")
        if pid <= 0 or uid.value != os.geteuid():
            raise ValueError("Trust memory peer is not a live process of this effective user")
        libproc = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        libproc.proc_pidpath.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
        libproc.proc_pidpath.restype = ctypes.c_int
        buffer = ctypes.create_string_buffer(4096)
        length = libproc.proc_pidpath(pid, buffer, len(buffer))
        if length <= 0 or length >= len(buffer):
            raise ValueError("Trust memory peer executable identity is unavailable")
        executable = Path(os.fsdecode(buffer.value))
    elif sys.platform.startswith("linux"):
        raw = connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
        if len(raw) != 12:
            raise ValueError("Trust memory peer credentials have an unsupported kernel shape")
        pid, uid, _gid = struct.unpack("=iII", raw)
        if pid <= 0 or uid != os.geteuid():
            raise ValueError("Trust memory peer is not a live process of this effective user")
        executable = Path(os.readlink(f"/proc/{pid}/exe"))
    else:
        raise ValueError("Trust memory peer process identity is unsupported on this platform")
    if (not executable.is_absolute() or executable.resolve(strict=True) != daemon.resolve(strict=True)
            or not executable.samefile(daemon)):
        raise ValueError("Trust memory peer is running another executable")
    before = executable.stat()
    if hashlib.sha256(executable.read_bytes()).hexdigest() != expected_sha256:
        raise ValueError("Trust memory peer executable bytes differ from the selected daemon")
    after = executable.stat()
    identity = lambda value: (value.st_dev, value.st_ino, value.st_size, value.st_mtime_ns, value.st_ctime_ns)
    if identity(before) != identity(after):
        raise ValueError("Trust memory peer executable changed while authenticating it")
    return pid, identity(after)


def lease_at(endpoint, expected, daemon):
    validate_endpoint(endpoint)
    connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    connected = False
    try:
        connection.settimeout(5)
        connection.connect(str(endpoint))
        connected = True
        peer = peer_process(connection, daemon, expected["executable_sha256"])
        stream = connection.makefile("rb")
        try:
            if json.loads(exchange(connection, stream, "IDENTITY")) != expected:
                raise ValueError("another Trust build owns the host memory endpoint; wait for its clean retirement")
            status = json.loads(exchange(connection, stream, "STATUS"))
            if (not isinstance(status, dict) or status.get("version") != "trustd.status.v1" or type(status.get("budget_bytes")) is not int
                    or status["budget_bytes"] <= 0 or type(status.get("reserved_bytes")) is not int
                    or not 0 <= status["reserved_bytes"] <= status["budget_bytes"]):
                raise ValueError("Trust memory authority returned an unusable status")
            if exchange(connection, stream, "LEASE") != b"LEASED":
                raise ValueError("Trust memory authority refused its lifetime lease")
            if peer_process(connection, daemon, expected["executable_sha256"]) != peer:
                raise ValueError("Trust memory peer process changed during the lease handshake")
        finally:
            stream.close()
        connection.settimeout(None)
        return connection
    except BaseException as error:
        connection.close()
        if connected and isinstance(error, OSError):
            raise ValueError("connected Trust memory peer failed identity/lease authentication") from error
        raise


def prepare(execution):
    for name in ("TRUSTD_DISABLE", "TRUSTD_RUNTIME_DIR", "TRUST_MEMORY_JOBSERVER", "TRUST_MEMORY_JOBSERVER_SOCK"):
        if name in os.environ:
            raise ValueError(f"{name} competes with the native-selected host memory authority")
    daemon, expected = selected_identity(execution)
    endpoint = host_endpoint()
    try:
        return endpoint, lease_at(endpoint, expected, daemon)
    except OSError as error:
        if error.errno not in (errno.ENOENT, errno.ECONNREFUSED):
            raise
    # Trust owns root creation, stable locking, crash epochs and idle retirement.
    # A separate session keeps gate cancellation from killing the shared daemon.
    # start_new_session also works with the native controller's macOS system Python.
    with tempfile.TemporaryFile() as stderr:
        child = subprocess.Popen([str(daemon)], env={}, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.DEVNULL, stderr=stderr, start_new_session=True)
        threading.Thread(target=child.wait, daemon=True).start()
        deadline = time.monotonic() + 10
        while True:
            try:
                return endpoint, lease_at(endpoint, expected, daemon)
            except OSError as error:
                if error.errno not in (errno.ENOENT, errno.ECONNREFUSED):
                    raise
            if child.poll() is not None or time.monotonic() >= deadline:
                stderr.seek(0)
                detail = stderr.read(4096).decode(errors="replace")
                # A losing startup child may exit before the compatible winner
                # publishes its endpoint. Only NotServing is retried above;
                # an identity/status/lease refusal still stops immediately.
                if "another trustd owns" not in detail or time.monotonic() >= deadline:
                    raise ValueError("selected trustd could not establish its host endpoint; no recovery or signal attempted: " + detail)
            time.sleep(0.05)


def run(command, *, supervise=None):
    execution = os.environ.get("ATPKG_PROJECT_EXECUTION", "")
    if not command or not os.path.isabs(execution):
        raise ValueError("memory preparation requires held native project selection and a command")
    endpoint, lease = prepare(Path(execution))
    try:
        environment = dict(os.environ)
        environment[PROFILE_SOCKET] = str(endpoint)
        # Preserve native controller/jobserver descriptors. The lease socket
        # itself is close-on-exec and stays only with this outside keeper.
        if supervise is not None:
            return supervise(command, environment)
        return subprocess.call(command, env=environment, close_fds=False)
    finally:
        lease.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--validate-profile-socket")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    try:
        if args.validate_profile_socket:
            if args.command:
                raise ValueError("profile validation cannot run a command")
            print(profile_socket(args.validate_profile_socket))
            return 0
        command = args.command[1:] if args.command[:1] == ["--"] else args.command
        code = run(command)
        return 128 - code if code < 0 else code
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(3, f"alab: selected Trust memory authority unavailable: {error}\n")


if __name__ == "__main__":
    sys.exit(main())
