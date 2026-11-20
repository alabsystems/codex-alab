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
--result-file optionally records a normal command's normalized status only after
the existing guardian retires its owned group; --result-fd does the same on an
anonymous caller-owned inode that is not inherited by source children. macOS
exposes no cross-process descriptor reopening primitive; Linux's documented
uncontained mode is not a security boundary.
Inherited-timeout records retain their separate raw-status format. No record is
source qualification.
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
STOP_REQUESTED = 0xA8


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
        self.pin_launch_destinations = True

    def check(self):
        destinations = []
        for requested in self.paths:
            try:
                path = requested.resolve()
                if self.pin_launch_destinations and path != requested:
                    destinations.append(path)
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
        if self.pin_launch_destinations:
            # A child may keep writing through a directory it opened before a symlink moved.
            self.pin_launch_destinations = False
            self.paths = list(dict.fromkeys([*self.paths, *destinations]))


def observe_exit_with_disk(pid, disk_floor, interval=1.0):
    """Observe without reaping; resource failure unwinds through the same group stop."""
    # NOTE_EXIT wakes promptly even on Python versions that also expose waitid.
    if hasattr(select, "kqueue") or not hasattr(os, "waitid"):
        return observe_exit_kqueue(pid, disk_floor.check, interval)
    flags = os.WEXITED | os.WNOWAIT | os.WNOHANG
    while True:
        try:
            disk_floor.check()
        except DiskSpaceError:
            # A running floor can stop a writer only while the custodian is live.
            if os.waitid(os.P_PID, pid, flags) is None:
                raise
            return
        if os.waitid(os.P_PID, pid, flags) is not None:
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
                    try:
                        check()
                    except DiskSpaceError:
                        # The custodian may have retired during the disk check.
                        events = queue.control(changes, 1, 0)
                        if not events:
                            raise
                        break
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


def caller_descriptors(inherit, exclude=()):
    """Snapshot caller opt-in before internal pipes can occupy descriptor numbers."""
    if not inherit:
        return ()
    directory = next((path for path in ("/dev/fd", "/proc/self/fd") if os.path.isdir(path)), None)
    if directory is None:
        raise RuntimeError("caller descriptors cannot be enumerated on this host")
    descriptors = []
    for name in os.listdir(directory):
        if not name.isdigit() or int(name) <= 2 or int(name) in exclude:
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


def send_completion(events, code, error, observed_exit):
    # One fixed seven-byte frame follows the startup record. It cannot fill even
    # POSIX's minimum pipe capacity while the guardian waits for our exit; paths
    # and arbitrary error strings are never repeated in this final frame.
    kind = 0 if error is None else (1 if isinstance(error, OSError) else 2)
    number = getattr(error, "errno", 0) or 0
    if type(number) is not int or not -(2**31) <= number < 2**31:
        kind, number = 2, 0
    tag = COMPLETED if observed_exit else STOP_REQUESTED
    events.write(COMPLETION.pack(tag, code, kind, number))
    events.flush()


def read_completion(events):
    data = events.read(COMPLETION.size)
    if len(data) != COMPLETION.size:
        raise RuntimeError("command custodian did not prove writer retirement")
    tag, code, kind, number = COMPLETION.unpack(data)
    if tag not in (COMPLETED, STOP_REQUESTED) or kind not in (0, 1, 2):
        raise RuntimeError("command custodian returned an invalid retirement status")
    error = None
    if kind == 1:
        error = OSError(number, os.strerror(number))
    elif kind == 2:
        error = RuntimeError("command custodian could not observe its command safely")
    return code, error, tag == COMPLETED


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
    """Return whether the leased command exited before its custodian requested a stop."""
    if not hasattr(select, "kqueue"):
        while os.waitid(os.P_PID, pid, os.WEXITED | os.WNOWAIT | os.WNOHANG) is None:
            ready, _, _ = select.select([lifeline], [], [], 1.0)
            if ready:
                if os.read(lifeline, 1):
                    raise RuntimeError("unexpected command lifeline data")
                # The command may also have exited while select waited for EOF.
                return os.waitid(os.P_PID, pid, os.WEXITED | os.WNOWAIT | os.WNOHANG) is not None
        return True
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
                return True
            raise
        exited = False
        for event in events:
            command = event.filter == select.KQ_FILTER_PROC and event.ident == pid
            if event.flags & select.KQ_EV_ERROR:
                if command and event.data == errno.ESRCH:
                    exited = True
                else:
                    raise OSError(event.data, os.strerror(event.data))
            elif command and event.fflags & select.KQ_NOTE_EXIT:
                exited = True
        # Both events can arrive together. The custodian has not signalled its group yet.
        if exited:
            return True
        if any(event.filter == select.KQ_FILTER_READ and event.ident == lifeline
               for event in events):
            if os.read(lifeline, 1):
                raise RuntimeError("unexpected command lifeline data")
            return False
        raise RuntimeError("command exit or lifeline could not be observed")


def retire_command(child, *, cooperative_controller=False):
    """Keep the original child until its owned work has settled."""
    try:
        if cooperative_controller:
            # The native publisher owns a separate cohort. It forwards TERM to its
            # original controller and returns only after that cohort has drained.
            os.kill(child.pid, signal.SIGTERM)
        else:
            os.killpg(child.pid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass
    code = child.wait()
    while group_exists(child.pid):
        time.sleep(0.01)
    return code if code >= 0 else 128 - code


def serve_command(argv, env, descriptors, lifeline, events, restore_child_signals,
                  close_fds=(), cooperative_controller=False):
    """Forked single-threaded custodian; inherited flock closes only after retirement."""
    child, failure, code = None, None, 1
    observed_exit = False
    try:
        # Anonymous result writers belong only to the guardian parent. A
        # same-UID source child must not recover one through this long-lived
        # custodian ancestor's descriptor table.
        for descriptor in close_fds:
            try:
                os.close(descriptor)
            except OSError as error:
                if error.errno != errno.EBADF:
                    raise
        os.setsid()
        child = subprocess.Popen(argv, start_new_session=True, close_fds=True,
                                 pass_fds=descriptors, preexec_fn=restore_child_signals,
                                 **({"env": env} if env is not None else {}))
        send_event(events, {"started": True})
        observed_exit = observe_lifeline(child.pid, lifeline)
    except BaseException as error:
        if child is None:
            # The guardian drains this startup record before it observes exit or
            # unmasks cancellation. Preserve the original exec failure only here.
            send_event(events, {"error": error_record(error)})
        else:
            failure = error
    finally:
        if child is not None:
            code = retire_command(child, cooperative_controller=cooperative_controller)
    send_completion(events, code, failure, observed_exit)
    return code


def spawn_custodian(argv, env, descriptors, restore_child_signals, close_fds=(),
                    cooperative_controller=False):
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
                                     restore_child_signals, close_fds=close_fds,
                                     cooperative_controller=cooperative_controller)
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


def run(argv, *, disk_floor=None, env=None, inherit_fds=False, exclude_fds=(),
        cooperative_controller=False):
    """Keep a surviving custodian responsible for the command until all writers stop."""
    if not hasattr(os, "waitid") and not hasattr(select, "kqueue"):
        raise RuntimeError("the cache guardian requires waitid or kqueue")
    if disk_floor is not None:
        disk_floor.check()
    cancellations = {signal.SIGINT, signal.SIGTERM, signal.SIGHUP}
    prior_mask = signal.pthread_sigmask(signal.SIG_BLOCK, cancellations)
    handlers = {}
    child, completed = None, None
    disk_stop = None
    try:
        for signum in cancellations:
            handlers[signum] = signal.signal(signum, interrupted)
        # SIG_IGN can auto-reap a child, invalidating the PID/PGID lease before
        # cleanup. This guardian alone owns the child until its explicit wait.
        handlers[signal.SIGCHLD] = signal.signal(signal.SIGCHLD, signal.SIG_DFL)

        def restore_child_signals():
            signal.signal(signal.SIGCHLD, handlers[signal.SIGCHLD])
            signal.pthread_sigmask(signal.SIG_SETMASK, prior_mask)

        descriptors = caller_descriptors(inherit_fds, exclude_fds)
        child = spawn_custodian(argv, env, descriptors, restore_child_signals,
                                close_fds=exclude_fds, cooperative_controller=cooperative_controller)
        started = read_event(child.events)
        if "error" in started:
            raise restore_error(started["error"])
        if started != {"started": True}:
            raise RuntimeError("command custodian did not establish command custody")
        signal.pthread_sigmask(signal.SIG_SETMASK, prior_mask)
        if disk_floor is None:
            observe_exit(child.pid)
        else:
            try:
                observe_exit_with_disk(child.pid, disk_floor)
            except DiskSpaceError as error:
                # The custodian may still be reaping a command it already observed exit.
                # Its retirement frame identifies whose action ended command custody.
                disk_stop = error
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
    code, error, observed_exit = completed
    if error is not None:
        raise error
    if disk_stop is not None and not observed_exit:
        raise disk_stop
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


def _timeout_notice(message):
    if sys.stderr is None:
        return  # An inherited closed descriptor may now belong to the result file.
    # Keep a broken diagnostic pipe out of CPython's final buffered flush.
    try:
        if sys.stderr is sys.__stderr__:
            os.write(sys.stderr.fileno(), (message + "\n").encode())
        else:
            print(message, file=sys.stderr, flush=True)
    except (OSError, ValueError):
        pass


def run_term_controller(argv, timeout):
    """Give a controller in the inherited group time to retire its own workers."""
    with subprocess.Popen(argv) as controller:
        try:
            return controller.wait(timeout=timeout)
        except BaseException:
            controller.terminate()
            try:
                controller.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                controller.kill()
                controller.wait()
                _timeout_notice("alab: controller did not exit after TERM; nested writer retirement is unconfirmed")
            raise


def run_inherited(argv, *, timeout, result_file=None, retire_on_term=False):
    """Bound one child inside an enclosing guardian's group, with inherited stdio.

    A release fetch controller receives TERM at expiry so it can retire native
    writers outside the inherited group. A second timeout bounds that wait.
    Ordinary commands keep their hard deadline. The enclosing guardian retires
    the inherited group after the failed transaction.
    """
    if timeout is None or not math.isfinite(timeout) or timeout <= 0:
        raise ValueError("command timeout must be finite and positive")
    # Claim evidence before launching; a stale result must never describe a new run.
    # The enclosing guardian's exit, not this file alone, proves group cleanup.
    with (Path(result_file).open("x") if result_file is not None else nullcontext()) as result:
        timed_out = False
        try:
            if retire_on_term:
                code = run_term_controller(argv, timeout)
            else:
                code = subprocess.run(argv, timeout=timeout).returncode
        except subprocess.TimeoutExpired:
            code, timed_out = None, True
            _timeout_notice(f"alab: network command exceeded its {timeout:g}s deadline; transaction must stop")
        if result is not None:
            json.dump({"schema": 1, "argv": list(argv), "timeout_seconds": timeout,
                       "returncode": code, "timed_out": timed_out}, result)
            result.write("\n")
        if timed_out:
            return 124
        return code if code >= 0 else 128 - code


def completion_bytes(argv, code):
    return json.dumps({"schema": 1, "kind": "guardian-command-completion",
                       "completed": True, "argv": list(argv),
                       "normalized_exit_status": code,
                       "cleanup_scope": "owned command process group"},
                      ensure_ascii=True).encode() + b"\n"


def write_all(descriptor, data):
    while data:
        written = os.write(descriptor, data)
        if written == 0:
            raise OSError(errno.EIO, "zero-length command completion receipt write")
        data = data[written:]


def _receipt_cleanup_error(primary, cleanup_errors):
    """Select failure authority after cleanup, then attempt only advisory notes."""
    selected = primary
    if issubclass(type(primary), Exception):
        for _phase, error in cleanup_errors:
            error_type = type(error)
            if issubclass(error_type, KeyboardInterrupt):
                selected = error
                break
            if error_type is not SystemExit:
                continue
            code = error.code
            successful = code is None
            if issubclass(type(code), int):
                code = int.__index__(code)
                successful = (code & 0xff == 0
                              if os.name == "posix" and -sys.maxsize - 1 <= code <= sys.maxsize
                              else code == 0)
            if not successful:
                selected = error
                break
    for phase, _error in cleanup_errors:
        try:
            selected.add_note(f"command completion receipt {phase} failed")
        except BaseException:
            pass
    return selected


def run_recorded_fd(argv, *, result_fd, disk_floor=None, inherit_fds=False):
    """Publish live completion on an unlinked empty inode not inherited by the child.

    Unlike retained --result-file evidence, this ephemeral controller channel
    needs no durability flush. The exact write is the final fallible operation;
    every earlier failure truncates the caller's still-open inode.
    """
    descriptor = os.dup(result_fd)
    validated = False
    try:
        state = os.fstat(descriptor)
        if not stat.S_ISREG(state.st_mode) or state.st_size != 0 or state.st_nlink != 0:
            raise ValueError("--result-fd requires an empty unlinked regular file")
        access = fcntl.fcntl(descriptor, fcntl.F_GETFL) & os.O_ACCMODE
        if access not in (os.O_WRONLY, os.O_RDWR):
            raise ValueError("--result-fd must be writable")
        if os.lseek(descriptor, 0, os.SEEK_CUR) != 0:
            raise ValueError("--result-fd must start at offset zero")
        validated = True
        code = run(argv, disk_floor=disk_floor, inherit_fds=inherit_fds,
                   exclude_fds=(result_fd, descriptor))
        write_all(descriptor, completion_bytes(argv, code))
        # Keep this process's duplicate live until process exit: a late close or
        # durability error must not turn exact live control bytes into failure.
        descriptor = None
        return code
    except BaseException as primary:
        cleanup_errors = []
        if descriptor is not None:
            if validated:
                try:
                    os.ftruncate(descriptor, 0)
                except BaseException as error:
                    cleanup_errors.append(("invalidation", error))
            try:
                os.close(descriptor)
            except BaseException as error:
                cleanup_errors.append(("close", error))
        selected = _receipt_cleanup_error(primary, cleanup_errors)
        if selected is not primary:
            raise selected from primary
        raise


def run_recorded(argv, *, result_file, disk_floor=None, inherit_fds=False):
    """Claim a fresh receipt before launch; complete it only after run() retires.

    The status has run()'s existing signal normalization, not a raw wait status.
    An empty, absent or incomplete receipt establishes no completed outcome.
    The owned group excludes descendants which deliberately escape that group.
    This observes the supervised command, not the receipt writer's eventual exit.
    """
    path = Path(result_file)
    parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    descriptor = None
    claimed = None
    try:
        descriptor = os.open(path.name, os.O_WRONLY | os.O_CREAT | os.O_EXCL
                             | os.O_CLOEXEC | os.O_NOFOLLOW, 0o600, dir_fd=parent)
        claimed = os.fstat(descriptor)
        code = run(argv, disk_floor=disk_floor, inherit_fds=inherit_fds)
        current = os.stat(path.name, dir_fd=parent, follow_symlinks=False)
        if (current.st_dev, current.st_ino) != (claimed.st_dev, claimed.st_ino):
            raise RuntimeError("command completion receipt was replaced after its claim")
        data = completion_bytes(argv, code)
        # Raw writes cannot reflush a buffered complete record after invalidation.
        write_all(descriptor, data)
        os.fsync(descriptor)
        os.fsync(parent)
        # Close inside the publication guard too: a close failure must invalidate
        # even fully written JSON. Do not retry a descriptor consumed by close.
        finished, descriptor = descriptor, None
        os.close(finished)
        finished, parent = parent, None
        os.close(finished)
        return code
    except BaseException as primary:
        cleanup_errors = []
        if descriptor is not None:
            try:
                os.ftruncate(descriptor, 0)
            except BaseException as error:
                cleanup_errors.append(("invalidation", error))
        if claimed is not None:
            try:
                current = os.stat(path.name if parent is not None else path,
                                  dir_fd=parent, follow_symlinks=False)
                if (current.st_dev, current.st_ino) == (claimed.st_dev, claimed.st_ino):
                    os.unlink(path.name if parent is not None else path, dir_fd=parent)
            except FileNotFoundError:
                pass
            except BaseException as error:
                # A permanent/hostile filesystem may also refuse invalidation.
                # Record that limit and continue retiring the owned descriptors.
                cleanup_errors.append(("invalidation", error))
        for phase, remaining in (("close", descriptor), ("parent close", parent)):
            if remaining is not None:
                try:
                    os.close(remaining)
                except BaseException as error:
                    cleanup_errors.append((phase, error))
        selected = _receipt_cleanup_error(primary, cleanup_errors)
        if selected is not primary:
            raise selected from primary
        raise


def main():
    argv = sys.argv[1:]
    lockfile = None
    package_repo = None
    result_file = None
    result_fd = None
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
        if len(argv) < 2 or argv[0] not in ("--cache-lock", "--disk-path", "--min-free-gib",
                                            "--result-file", "--result-fd",
                                            "--package-source-repo"):
            break
        option, value, *argv = argv
        if option == "--cache-lock":
            lockfile = value
        elif option == "--disk-path":
            disk_paths.append(value)
        elif option == "--result-file":
            if result_file is not None or result_fd is not None:
                print("alab: a result receipt may be supplied only once", file=sys.stderr)
                return 2
            result_file = value
        elif option == "--result-fd":
            if result_file is not None or result_fd is not None:
                print("alab: a result receipt may be supplied only once", file=sys.stderr)
                return 2
            try:
                result_fd = int(value)
            except ValueError:
                print("alab: result descriptor must be an integer", file=sys.stderr)
                return 2
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
        print("usage: command_guard.py [--cache-lock FILE] [--result-file FILE | --result-fd FD] "
              "[--min-free-gib N --disk-path PATH ...] -- COMMAND [ARG ...]", file=sys.stderr)
        return 2
    try:
        disk_floor = DiskFloor(disk_paths, minimum_gib) if minimum_gib is not None else None
        if disk_paths and disk_floor is None:
            raise ValueError("--disk-path requires --min-free-gib")
        if argv[:1] == ["--inherited-group-timeout"]:
            if (lockfile is not None or disk_floor is not None or package_repo is not None
                    or inherit_fds or resource_stop or result_fd is not None):
                raise ValueError("an inherited-group command cannot own cache or disk guards")
            retire_on_term = argv[2:3] == ["--retire-on-term"]
            command = argv[3:] if retire_on_term else argv[2:]
            if len(command) < 2 or command[0] != "--":
                raise ValueError("inherited-group timeout requires SECONDS [--retire-on-term] -- COMMAND")
            return run_inherited(command[1:], timeout=float(argv[1]), result_file=result_file,
                                 retire_on_term=retire_on_term)
        if result_file is not None and result_fd is not None:
            raise ValueError("--result-file and --result-fd are mutually exclusive")
        if (result_file is not None or result_fd is not None) and lockfile is not None:
            raise ValueError("a result receipt cannot record a cache guardian")
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
        if result_file is not None:
            return run_recorded(argv, result_file=result_file, disk_floor=disk_floor,
                                inherit_fds=inherit_fds)
        if result_fd is not None:
            if result_fd <= 2:
                raise ValueError("--result-fd must be greater than 2")
            return run_recorded_fd(argv, result_fd=result_fd, disk_floor=disk_floor,
                                   inherit_fds=inherit_fds)
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
