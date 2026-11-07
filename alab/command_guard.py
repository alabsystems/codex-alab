#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Keep a cache-locked command's process group owned until its writers stop.

The guardian and its surviving custodian share the cache's BSD flock through
controller cleanup. The custodian owns the command group and stops it when the
guardian's lifeline closes, including on guardian SIGKILL. A child that deliberately
escapes that group, or SIGKILL of the surviving custodian, is outside this boundary.

An explicit --min-free-gib with --disk-path opts into prelaunch and running disk
checks. --resource-stop opts source builds into exit 75 when disk admission or
observation stops the command; existing package clients retain exit 1 there.
--inherit-fds preserves a compiler caller's inheritable descriptors; it is
refused for cache guardians, whose existing descriptor policy stays in place.
"""

import errno
from contextlib import closing, nullcontext
import fcntl
import json
import math
import os
from pathlib import Path
import runpy
import secrets
import select
import signal
import stat
import struct
import subprocess
import sys
import time


DISK_STOP = 75
COMPLETION = struct.Struct("!BBBi")
COMPLETED = 0xA7


class DiskSpaceError(RuntimeError):
    """Resource admission/observation stopped work, without a compiler verdict."""


class DiskFloor:
    """Check each actual output volume, including paths not allocated yet."""

    def __init__(self, paths, minimum_gib):
        if not paths or not math.isfinite(minimum_gib) or minimum_gib <= 0:
            raise ValueError("a disk floor needs paths and a positive finite GiB limit")
        self.paths = [Path(path) for path in paths]
        if any(not path.is_absolute() for path in self.paths):
            raise ValueError("disk guard paths must be absolute")
        self.minimum_gib = minimum_gib

    def check(self):
        for requested in self.paths:
            try:
                path = requested.resolve()
                while True:
                    try:
                        space = os.statvfs(path)
                        break
                    except FileNotFoundError:
                        if path == path.parent:
                            raise
                        path = path.parent
            except (OSError, RuntimeError) as error:
                raise DiskSpaceError(f"disk measurement unavailable for {requested}: {error}") from error
            free_gib = space.f_bavail * space.f_frsize / 2**30
            if free_gib < self.minimum_gib:
                raise DiskSpaceError(f"disk: {free_gib:.2f} GiB free for {requested}; "
                                   f"{self.minimum_gib:g} GiB running floor; stopping command")


def observe_exit_with_disk(pid, disk_floor, interval=1.0):
    """Poll without reaping; resource failure unwinds through the same group stop."""
    if not hasattr(os, "waitid"):
        return observe_exit_kqueue(pid, disk_floor.check, interval)
    while True:
        disk_floor.check()
        if os.waitid(os.P_PID, pid, os.WEXITED | os.WNOWAIT | os.WNOHANG) is not None:
            return
        time.sleep(interval)


def observe_exit(pid):
    """Observe our sole child's exit while leaving its PID unreaped and leased."""
    if hasattr(os, "waitid"):
        os.waitid(os.P_PID, pid, os.WEXITED | os.WNOWAIT)
        return
    observe_exit_kqueue(pid)


def observe_exit_kqueue(pid, check=None, interval=1.0):
    """The non-reaping observer available on Apple's Python 3.9."""
    # Apple's Python 3.9 has kqueue, but exposes no os.waitid. NOTE_EXIT does
    # not reap. ESRCH at registration also means this unreaped child exited.
    with closing(select.kqueue()) as queue:
        event = select.kevent(pid, filter=select.KQ_FILTER_PROC,
                              flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT,
                              fflags=select.KQ_NOTE_EXIT)
        try:
            changes = [event]
            while True:
                if check is not None:
                    check()
                events = queue.control(changes, 1, interval if check is not None else None)
                changes = []
                if events or check is None:
                    break
        except OSError as error:
            if error.errno == errno.ESRCH:
                return
            raise
        if len(events) == 1 and events[0].ident == pid and events[0].flags & select.KQ_EV_ERROR:
            # kevent can report a changelist error in its eventlist instead of
            # errno. The same leased-child ESRCH case applies to both forms.
            if events[0].data == errno.ESRCH:
                return
            raise OSError(events[0].data, os.strerror(events[0].data))
        if (len(events) != 1 or events[0].ident != pid
                or not events[0].fflags & select.KQ_NOTE_EXIT):
            raise RuntimeError("the command exit could not be observed without reaping")


def group_exists(pgid):
    try:
        os.killpg(pgid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def interrupted(signum, _frame):
    raise SystemExit(128 + signum)


class Custodian:
    """Our unreaped direct child; only it owns and may signal the command group."""

    def __init__(self, pid, events, writer):
        self.pid = pid
        self.events = events
        self.writer = writer
        self.returncode = None

    def close_lifeline(self):
        if self.writer is not None:
            interrupted_close = False
            while True:
                try:
                    os.close(self.writer)
                    break
                except OSError as error:
                    if error.errno == errno.EINTR:
                        # No descriptor allocation intervenes in this dedicated
                        # guardian. close may already have consumed the endpoint.
                        interrupted_close = True
                    elif error.errno == errno.EBADF and interrupted_close:
                        break
                    else:
                        raise
            self.writer = None

    def wait(self, timeout=None):
        deadline = None if timeout is None else time.monotonic() + timeout
        while self.returncode is None:
            pid, status = os.waitpid(self.pid, 0 if deadline is None else os.WNOHANG)
            if pid:
                self.returncode = os.waitstatus_to_exitcode(status)
            elif time.monotonic() >= deadline:
                raise subprocess.TimeoutExpired("command custodian", timeout)
            else:
                time.sleep(0.01)
        return self.returncode


def caller_descriptors(inherit):
    """Snapshot caller opt-in before internal pipes can occupy descriptor numbers."""
    if not inherit:
        return ()
    directory = next((path for path in ("/dev/fd", "/proc/self/fd") if os.path.isdir(path)), None)
    if directory is None:
        raise RuntimeError("caller descriptors cannot be enumerated on this host")
    descriptors = []
    for name in os.listdir(directory):
        if not name.isdigit() or int(name) <= 2:
            continue
        descriptor = int(name)
        try:
            if os.get_inheritable(descriptor):
                descriptors.append(descriptor)
        except OSError as error:
            if error.errno != errno.EBADF:
                raise
    return tuple(sorted(descriptors))


def send_event(events, value):
    data = json.dumps(value, ensure_ascii=True).encode() + b"\n"
    if len(data) > 64 * 1024:
        raise RuntimeError("custodian status is oversized")
    events.write(data)
    events.flush()


def read_event(events):
    data = events.readline(64 * 1024 + 1)
    if len(data) > 64 * 1024 or not data.endswith(b"\n"):
        raise RuntimeError("command custodian ended without a complete status")
    value = json.loads(data)
    if not isinstance(value, dict):
        raise RuntimeError("command custodian returned an invalid status")
    return value


def error_record(error):
    filename = getattr(error, "filename", None)
    return {"type": type(error).__name__,
            "message": error.strerror if isinstance(error, OSError) else str(error),
            "errno": getattr(error, "errno", None),
            "filename": None if filename is None else os.fsdecode(filename),
            "filename_bytes": isinstance(filename, bytes)}


def restore_error(record):
    if record.get("errno") is not None:
        filename = record.get("filename")
        if filename is not None and record.get("filename_bytes"):
            filename = os.fsencode(filename)
        return OSError(record["errno"], record["message"], filename)
    kind = {"ValueError": ValueError, "SubprocessError": subprocess.SubprocessError}.get(
        record.get("type"), RuntimeError)
    return kind(record.get("message", "command custodian failed"))


def send_completion(events, code, error):
    # One fixed seven-byte frame follows the startup record. It cannot fill even
    # POSIX's minimum pipe capacity while the guardian waits for our exit; paths
    # and arbitrary error strings are never repeated in this final frame.
    kind = 0 if error is None else (1 if isinstance(error, OSError) else 2)
    number = getattr(error, "errno", 0) or 0
    if type(number) is not int or not -(2**31) <= number < 2**31:
        kind, number = 2, 0
    events.write(COMPLETION.pack(COMPLETED, code, kind, number))
    events.flush()


def read_completion(events):
    data = events.read(COMPLETION.size)
    if len(data) != COMPLETION.size:
        raise RuntimeError("command custodian did not prove writer retirement")
    tag, code, kind, number = COMPLETION.unpack(data)
    if tag != COMPLETED or kind not in (0, 1, 2):
        raise RuntimeError("command custodian returned an invalid retirement status")
    error = None
    if kind == 1:
        error = OSError(number, os.strerror(number))
    elif kind == 2:
        error = RuntimeError("command custodian could not observe its command safely")
    return code, error


def finish_custodian(child, *, startup_pending=False):
    """Request EOF retirement, drain any startup record, then reap our exact child."""
    try:
        child.close_lifeline()
        if startup_pending:
            # An exec failure may fill the status pipe. Consume startup before
            # waiting, even when a parent-side construction failure triggered us.
            read_event(child.events)
    finally:
        child.wait()
        try:
            completed = read_completion(child.events)
        finally:
            child.events.close()
    return completed


def observe_lifeline(pid, lifeline):
    """Observe command exit or guardian EOF without reaping our leased child."""
    if not hasattr(select, "kqueue"):
        while os.waitid(os.P_PID, pid, os.WEXITED | os.WNOWAIT | os.WNOHANG) is None:
            ready, _, _ = select.select([lifeline], [], [], 1.0)
            if ready:
                if os.read(lifeline, 1):
                    raise RuntimeError("unexpected command lifeline data")
                return
        return
    with closing(select.kqueue()) as queue:
        changes = [
            select.kevent(pid, filter=select.KQ_FILTER_PROC,
                          flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT,
                          fflags=select.KQ_NOTE_EXIT),
            select.kevent(lifeline, filter=select.KQ_FILTER_READ, flags=select.KQ_EV_ADD),
        ]
        try:
            events = queue.control(changes, 2, None)
        except OSError as error:
            if error.errno == errno.ESRCH:
                return
            raise
        for event in events:
            if event.flags & select.KQ_EV_ERROR:
                if event.ident == pid and event.data == errno.ESRCH:
                    return
                raise OSError(event.data, os.strerror(event.data))
            if event.ident == pid and event.fflags & select.KQ_NOTE_EXIT:
                return
            if event.ident == lifeline and event.filter == select.KQ_FILTER_READ:
                if os.read(lifeline, 1):
                    raise RuntimeError("unexpected command lifeline data")
                return
        raise RuntimeError("command exit or lifeline could not be observed")


def retire_command(child):
    """Signal only our unreaped child's group, then prove every writer has stopped."""
    try:
        os.killpg(child.pid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass
    code = child.wait()
    while group_exists(child.pid):
        time.sleep(0.01)
    return code if code >= 0 else 128 - code


def serve_command(argv, env, descriptors, lifeline, events, restore_child_signals):
    """Forked single-threaded custodian; inherited flock closes only after retirement."""
    child, failure, code = None, None, 1
    try:
        os.setsid()
        child = subprocess.Popen(argv, start_new_session=True, close_fds=True,
                                 pass_fds=descriptors, preexec_fn=restore_child_signals,
                                 **({"env": env} if env is not None else {}))
        send_event(events, {"started": True})
        observe_lifeline(child.pid, lifeline)
    except BaseException as error:
        if child is None:
            # The guardian drains this startup record before it observes exit or
            # unmasks cancellation. Preserve the original exec failure only here.
            send_event(events, {"error": error_record(error)})
        else:
            failure = error
    finally:
        if child is not None:
            code = retire_command(child)
    send_completion(events, code, failure)
    return code


def spawn_custodian(argv, env, descriptors, restore_child_signals):
    allocated = []
    reader, sender = None, None
    try:
        lifeline, writer = os.pipe()
        allocated.extend((lifeline, writer))
        incoming, outgoing = os.pipe()
        allocated.extend((incoming, outgoing))
        reader = os.fdopen(incoming, "rb")
        allocated.remove(incoming)
        sender = os.fdopen(outgoing, "wb")
        allocated.remove(outgoing)
        child = Custodian(None, reader, writer)
        pid = os.fork()
    except BaseException:
        # Construct every parent-side channel owner before the fork. File objects own
        # their endpoints once constructed; raw descriptors own the rest.
        for stream in (reader, sender):
            if stream is not None:
                stream.close()
        for descriptor in allocated:
            os.close(descriptor)
        raise
    if pid == 0:
        # Never let a descendant keep the sole guardian writer alive. Every
        # control FD remains CLOEXEC; actual commands receive only caller FDs.
        child.close_lifeline()
        reader.close()
        code = 1
        try:
            with sender:
                code = serve_command(argv, env, descriptors, lifeline, sender,
                                     restore_child_signals)
        finally:
            os._exit(code)
    child.pid = pid
    lifeline_closed = False
    try:
        os.close(lifeline)
        lifeline_closed = True
        sender.close()
    except BaseException:
        # Custody exists as soon as fork returns. Every later failure must wait
        # for its retirement, including failures before the caller reads startup.
        try:
            sender.close()
        finally:
            try:
                finish_custodian(child, startup_pending=True)
            finally:
                if not lifeline_closed:
                    try:
                        os.close(lifeline)
                    except OSError as error:
                        if error.errno != errno.EBADF:
                            raise
        raise
    return child


def run(argv, *, disk_floor=None, env=None, inherit_fds=False):
    """Keep a surviving custodian responsible for the command until all writers stop."""
    if not hasattr(os, "waitid") and not hasattr(select, "kqueue"):
        raise RuntimeError("the cache guardian requires waitid or kqueue")
    if disk_floor is not None:
        disk_floor.check()
    cancellations = {signal.SIGINT, signal.SIGTERM, signal.SIGHUP}
    prior_mask = signal.pthread_sigmask(signal.SIG_BLOCK, cancellations)
    handlers = {}
    child, completed = None, None
    try:
        for signum in cancellations:
            handlers[signum] = signal.signal(signum, interrupted)
        # SIG_IGN can auto-reap a child, invalidating the PID/PGID lease before
        # cleanup. This guardian alone owns the child until its explicit wait.
        handlers[signal.SIGCHLD] = signal.signal(signal.SIGCHLD, signal.SIG_DFL)

        def restore_child_signals():
            signal.signal(signal.SIGCHLD, handlers[signal.SIGCHLD])
            signal.pthread_sigmask(signal.SIG_SETMASK, prior_mask)

        descriptors = caller_descriptors(inherit_fds)
        child = spawn_custodian(argv, env, descriptors, restore_child_signals)
        started = read_event(child.events)
        if "error" in started:
            raise restore_error(started["error"])
        if started != {"started": True}:
            raise RuntimeError("command custodian did not establish command custody")
        signal.pthread_sigmask(signal.SIG_SETMASK, prior_mask)
        if disk_floor is None:
            observe_exit(child.pid)
        else:
            observe_exit_with_disk(child.pid, disk_floor)
    finally:
        signal.pthread_sigmask(signal.SIG_BLOCK, cancellations)
        try:
            if child is not None:
                # Killing this wrapper would strand its separately owned command.
                # EOF asks its sole owner to stop/reap the leased command group.
                completed = finish_custodian(child)
        finally:
            for signum, handler in handlers.items():
                signal.signal(signum, handler)
            signal.pthread_sigmask(signal.SIG_SETMASK, prior_mask)
    code, error = completed
    if error is not None:
        raise error
    return code


def run_with_cache_lock(lockfile, argv, *, disk_floor=None, package_repo=None):
    """Own the existing regular lock file until the command group is quiescent."""
    descriptor = os.open(lockfile, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK, 0o666)
    try:
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            raise ValueError("the package cache lock must be a regular file")
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return 75
        if package_repo is None:
            return run(argv) if disk_floor is None else run(argv, disk_floor=disk_floor)
        # Only this guardian, after run() proves the child group gone, may return
        # package sources to Ready. A killed guardian leaves Active receipts intact.
        environment = dict(os.environ)
        environment.pop("ALAB_PACKAGE_SOURCE_GUARD", None)
        session = None
        try:
            source = runpy.run_path(str(Path(__file__).with_name("package_source.py")))
            session = source["Session"](Path(lockfile).resolve().parent, package_repo)
            environment = session.environment()
        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
            print(f"alab: stable package source unavailable; using fresh source: {error}", file=sys.stderr)
        environment.pop("ALAB_PACKAGE_CACHE_ATTEMPT", None)
        environment.pop("ALAB_PACKAGE_GUARD_PID", None)
        environment.pop("ALAB_PACKAGE_CACHE_PRESERVE", None)
        attempt = None
        if session is not None:
            attempt = secrets.token_hex(16)
            environment["ALAB_PACKAGE_CACHE_ATTEMPT"] = attempt
            environment["ALAB_PACKAGE_GUARD_PID"] = str(os.getpid())
            environment["ALAB_PACKAGE_CACHE_PRESERVE"] = str(
                Path(session.record["common"]) / "alab-preserve-builds")
        code = run(argv, disk_floor=disk_floor, env=environment)
        source_returned = False
        if session is not None:
            try:
                session.finish()
                source_returned = True
            except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError) as error:
                print(f"alab: package sources kept after return refused: {error}", file=sys.stderr)
        # run() proved the producer group retired. Settlement stays inside this
        # coordinator lock and after source return; exceptions/crashes never settle.
        if source_returned and attempt is not None:
            try:
                maintenance = runpy.run_path(str(Path(__file__).with_name("package_cache.py")))
                maintenance["settled"](Path(lockfile).resolve().parent, attempt, code == 0,
                                       env=environment)
            except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
                print(f"alab: package cache settlement deferred: {error}", file=sys.stderr)
        return code
    finally:
        os.close(descriptor)


def run_inherited(argv, *, timeout, result_file=None):
    """Bound one child inside an enclosing guardian's group, with inherited stdio.

    On timeout subprocess.run kills and reaps its direct child. Its caller must
    then exit the guarded transaction: the enclosing guardian stops any remaining
    descendants before releasing its cache. Never create a nested process group.
    """
    if timeout is None or not math.isfinite(timeout) or timeout <= 0:
        raise ValueError("command timeout must be finite and positive")
    # Claim evidence before launching; a stale result must never describe a new run.
    # The enclosing guardian's exit, not this file alone, proves group cleanup.
    with (Path(result_file).open("x") if result_file is not None else nullcontext()) as result:
        timed_out = False
        try:
            code = subprocess.run(argv, timeout=timeout).returncode
        except subprocess.TimeoutExpired:
            code, timed_out = None, True
            print(f"alab: network command exceeded its {timeout:g}s deadline; transaction must stop",
                  file=sys.stderr)
        if result is not None:
            json.dump({"schema": 1, "argv": list(argv), "timeout_seconds": timeout,
                       "returncode": code, "timed_out": timed_out}, result)
            result.write("\n")
        if timed_out:
            return 124
        return code if code >= 0 else 128 - code


def main():
    argv = sys.argv[1:]
    lockfile = None
    package_repo = None
    result_file = None
    inherit_fds = False
    resource_stop = False
    disk_paths, minimum_gib = [], None
    while argv:
        if argv[0] == "--resource-stop":
            resource_stop = True
            argv = argv[1:]
            continue
        if argv[0] == "--inherit-fds":
            inherit_fds = True
            argv = argv[1:]
            continue
        if len(argv) < 2 or argv[0] not in ("--cache-lock", "--disk-path", "--min-free-gib", "--result-file", "--package-source-repo"):
            break
        option, value, *argv = argv
        if option == "--cache-lock":
            lockfile = value
        elif option == "--disk-path":
            disk_paths.append(value)
        elif option == "--result-file":
            result_file = value
        elif option == "--package-source-repo":
            package_repo = value
        else:
            try:
                minimum_gib = float(value)
            except ValueError:
                print("alab: disk floor must be a number", file=sys.stderr)
                return 2
    if argv[:1] == ["--"]:
        argv = argv[1:]
    if not argv:
        print("usage: command_guard.py [--cache-lock FILE] "
              "[--min-free-gib N --disk-path PATH ...] -- COMMAND [ARG ...]", file=sys.stderr)
        return 2
    try:
        disk_floor = DiskFloor(disk_paths, minimum_gib) if minimum_gib is not None else None
        if disk_paths and disk_floor is None:
            raise ValueError("--disk-path requires --min-free-gib")
        if argv[:1] == ["--inherited-group-timeout"]:
            if lockfile is not None or disk_floor is not None or package_repo is not None or inherit_fds or resource_stop:
                raise ValueError("an inherited-group command cannot own cache or disk guards")
            if len(argv) < 4 or argv[2] != "--":
                raise ValueError("inherited-group timeout requires SECONDS -- COMMAND")
            return run_inherited(argv[3:], timeout=float(argv[1]), result_file=result_file)
        if result_file is not None:
            raise ValueError("--result-file requires --inherited-group-timeout")
        if package_repo is not None and lockfile is None:
            raise ValueError("--package-source-repo requires --cache-lock")
        if inherit_fds and lockfile is not None:
            raise ValueError("a cache guardian cannot inherit caller descriptors")
        if lockfile is not None:
            try:
                return run_with_cache_lock(lockfile, argv, disk_floor=disk_floor, package_repo=package_repo)
            finally:
                if package_repo is not None:
                    # The executable package controller is this guardian, not a
                    # waiting shell. Maintenance remains outside the producer lock,
                    # after run() has retired its separately owned writer group.
                    try:
                        maintenance = runpy.run_path(str(Path(__file__).with_name("package_cache.py")))
                        maintenance["compact"](Path(lockfile).parent.resolve())
                    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
                        print(f"alab: package cache maintenance deferred after producer retirement: {error}",
                              file=sys.stderr)
        if inherit_fds:
            return run(argv, disk_floor=disk_floor, inherit_fds=True)
        return run(argv) if disk_floor is None else run(argv, disk_floor=disk_floor)
    except DiskSpaceError as error:
        print(f"alab: disk resource stop: {error}", file=sys.stderr)
        return DISK_STOP if resource_stop else 1
    except (OSError, ValueError, RuntimeError) as error:
        print(f"alab: cache command guardian: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
