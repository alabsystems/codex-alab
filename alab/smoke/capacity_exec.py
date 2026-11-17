#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The loopback Responses model, command-guardian runner and package reader the smokes share.

Imported by the smokes beside it, never run. Its own capacity and quota cases, which ran a
package's `codex exec` with the Internet open, became aterm's jailed capacity lane
(`aterm-lab codex-alab-proof --capacity-only`; alab/README.md, "Headless capacity and quota
recovery"), which runs every one of them with only its loopback model reachable.
"""

import http.server
import json
import math
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys
import threading
import time

import provider_fixture

MODEL = "gpt-5"
PROMPT = "Keep this headless capacity smoke task and its completed plan."
CALL_ID = "capacity-smoke-plan"
PLAN = "Completed work survives a capacity retry"
ANSWER = "Headless capacity recovery succeeded."
RETRY_SECONDS = 2
HEADER_EMISSION_LIMIT_SECONDS = 0.050
COMMAND_TIMEOUT_SECONDS = 120
COMMAND_CLEANUP_SECONDS = 5
LISTENER_STARTUP_CLEANUP_SECONDS = 5
GUARD_PATH = Path(__file__).resolve().parents[1] / "command_guard.py"
METADATA_LIMIT = 1024 * 1024


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def _case_label(error):
    try:
        kind = type(error).__name__
    except BaseException:
        kind = "Exception"
    if type(kind) is not str:
        kind = "Exception"
    try:
        return f"{kind}: {error}"
    except BaseException:
        return f"{kind}: <message unavailable>"


def _case_notes(error, attribute):
    try:
        notes = getattr(error, attribute, ())
        return [note for note in notes if type(note) is str] if type(notes) in (list, tuple) else []
    except BaseException:
        return []


def _attach_case_notes(error, notes):
    if not notes:
        return
    try:
        for note in notes:
            error.add_note(note)
        return
    except BaseException:
        pass
    try:
        # Older Python and unavailable native-note hooks use the existing CLI
        # transport. Optional diagnostics never own the pending exception.
        error._alab_case_cleanup_errors = [*_case_notes(error, "_alab_case_cleanup_errors"), *notes]
    except BaseException:
        pass


def case_error_fields(error):
    fields = {"error": _case_label(error)}
    if notes := _case_notes(error, "__notes__"):
        fields["errorNotes"] = notes
    if failures := _case_notes(error, "_alab_case_cleanup_errors"):
        fields["cleanupErrors"] = failures
    return fields


def cleanup_case(server, thread, write_requests, result, primary):
    return cleanup_resources((("", server, thread),), (("write", write_requests),), result, primary)


def cleanup_resources(listeners, exports, result, primary, *, stop=None):
    """Finish all safe fixture cleanup and preserve the primary exception.

    A listener join after failed shutdown can wait forever, so skip only that
    action. Other listeners and each evidence export still get their attempt.
    """
    failures = []
    pending = primary

    def attempt(phase, action):
        nonlocal pending
        try:
            action()
        except BaseException as error:
            if not isinstance(error, Exception) and (pending is None or isinstance(pending, Exception)):
                pending = error
            failures.append(f"case cleanup {phase}: {_case_label(error)}")
            return False
        return True

    if stop is not None:
        attempt("stop", stop)
    for name, server, thread in listeners:
        prefix = name + "." if name else ""
        if attempt(prefix + "shutdown", server.shutdown):
            attempt(prefix + "join", thread.join)
        else:
            failures.append(f"case cleanup {prefix}join: skipped because shutdown failed")
        attempt(prefix + "close", server.server_close)
    for phase, action in exports:
        attempt(phase, action)
    if pending is not None:
        notes = []
        if pending is not primary and primary is not None:
            notes.append(f"case body: {_case_label(primary)}")
            for note in [*_case_notes(primary, "__notes__"),
                         *_case_notes(primary, "_alab_case_cleanup_errors")]:
                notes.append(f"case body note: {note}")
        elif primary is None and "error" in result:
            reason = result["error"] if type(result["error"]) is str else "<message unavailable>"
            notes.append(f"case verifier: {reason}")
        notes.extend(failures)
        _attach_case_notes(pending, notes)
        raise pending
    if failures:
        result.update(passed=False, cleanupErrors=failures)
        result.setdefault("error", "case cleanup failed")


def retry_after_delay(overload, retry):
    """Measure from response headers, excluding request handling before them.

    The client can receive headers between the two recorded header timestamps.
    Use only that measured overlap, and refuse an inconclusive clock sample.
    Handler entry is server-observed arrival, not a TCP wire-time measurement.
    """
    start, header, request = (overload.get("headerStartedAt"), overload.get("headerAt"),
                              retry.get("requestStartedAt"))
    require(all(type(value) in (int, float) and math.isfinite(value)
                for value in (start, header, request)), "missing or invalid Retry-After timing")
    emission = header - start
    require(0 <= emission <= HEADER_EMISSION_LIMIT_SECONDS,
            f"Retry-After timing inconclusive: header emission took {emission:.6f}s")
    delay = request - header
    deadline = start + RETRY_SECONDS
    require(request >= math.nextafter(deadline, -math.inf),
            f"Retry-After bypassed: {delay:.6f}s after headers")
    return delay


def write_json(path, value):
    handle = primary = None
    try:
        handle = path.open("x")
        json.dump(value, handle, indent=2)
        handle.write("\n")
    except BaseException as error:
        primary = error
        raise
    finally:
        if handle is not None:
            try:
                handle.close()
            except BaseException as error:
                if primary is None:
                    if issubclass(type(error), SystemExit) and _successful_exit(error):
                        raise RuntimeError("JSON output close incomplete: requested a successful exit") from error
                    raise
                _attach_case_notes(primary, ["JSON output close failed: " + _case_label(error)])


def _successful_exit(error):
    # Keep this standalone classification aligned with alab/cli_status.py.
    if type(error) is not SystemExit:
        raise RuntimeError("unsupported SystemExit type") from None
    code = error.code
    if code is None:
        return True
    if not issubclass(type(code), int):
        return False
    code = int.__index__(code)
    if os.name == "posix" and -sys.maxsize - 1 <= code <= sys.maxsize:
        return code & 0xff == 0
    return code == 0


def package_record(path):
    """Read bounded regular metadata from the descriptor that refused symlinks."""
    descriptor = primary = None
    try:
        try:
            descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        except OSError as error:
            raise RuntimeError(f"missing regular {path.name}") from error
        before = os.fstat(descriptor)
        require(stat.S_ISREG(before.st_mode), f"missing regular {path.name}")
        require(before.st_size <= METADATA_LIMIT, f"oversized {path.name}")
        chunks, length = [], 0
        while length <= METADATA_LIMIT:
            chunk = os.read(descriptor, METADATA_LIMIT + 1 - length)
            if not chunk:
                break
            chunks.append(chunk)
            length += len(chunk)
        require(length <= METADATA_LIMIT, f"oversized {path.name}")
        after = os.fstat(descriptor)
        require((before.st_dev, before.st_ino, before.st_mode, before.st_size,
                 before.st_mtime_ns, before.st_ctime_ns) ==
                (after.st_dev, after.st_ino, after.st_mode, after.st_size,
                 after.st_mtime_ns, after.st_ctime_ns), f"metadata changed while reading: {path.name}")
        return json.loads(b"".join(chunks).decode())
    except BaseException as error:
        primary = error
        raise
    finally:
        if descriptor is not None:
            try:
                os.close(descriptor)
            except BaseException as error:
                if primary is None:
                    if issubclass(type(error), SystemExit) and _successful_exit(error):
                        raise RuntimeError("metadata descriptor close incomplete: requested a successful exit") from error
                    raise
                _attach_case_notes(primary, ["metadata descriptor close failed: " + _case_label(error)])


def package_records(package):
    records = [package_record(package / name) for name in ("alab-build.json", "codex-package.json")]
    build, manifest = records
    require(build["version"] == manifest["version"], "package versions disagree")
    require(build["target"] == manifest["target"], "package targets disagree")
    require(re.fullmatch(r"[0-9a-f]{40}", build["source"]), "missing exact source commit")
    require(manifest["entrypoint"] == "bin/codex", "unexpected package entrypoint")
    executable = (package / manifest["entrypoint"]).resolve(strict=True)
    require(executable.is_relative_to(package) and executable.is_file(), "entrypoint escapes package")
    return build, manifest, executable


def fresh_environment(root):
    for name in ("home", "codex-home", "work", "xdg", "tmp"):
        (root / name).mkdir()
    # No inherited login, provider, proxy, aterm or agent variables.
    return {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": str(root / "home"),
            "CODEX_HOME": str(root / "codex-home"), "XDG_RUNTIME_DIR": str(root / "xdg"),
            "TMPDIR": str(root / "tmp"), "TERM": "dumb", "NO_COLOR": "1", "LANG": "en_US.UTF-8"}


class OutputFiles:
    """Own named exclusive output files through acquisition and every close."""

    def __init__(self, named_paths):
        self.named_paths = tuple((name, path) for name, path in named_paths)
        self.streams = [None] * len(self.named_paths)

    def __enter__(self):
        try:
            for index, (_name, path) in enumerate(self.named_paths):
                self.streams[index] = path.open("xb")
            return tuple(self.streams)
        except BaseException as error:
            self._close(error)
            raise

    def __exit__(self, _kind, error, _traceback):
        self._close(error)
        # Let the with statement retain the active object and its traceback,
        # including StopIteration or unavailable exception attribute hooks.
        return False

    def _close(self, primary):
        failures = []
        for index in reversed(range(len(self.streams))):
            name, _path = self.named_paths[index]
            stream = self.streams[index]
            if stream is not None:
                try:
                    stream.close()
                except BaseException as error:
                    failures.append((name, error))
        if not failures:
            return
        notes = [f"output close {name}: {_case_label(error)}" for name, error in failures]
        if primary is not None:
            _attach_case_notes(primary, notes)
            return
        selected = failures[0][1]
        for _name, error in failures:
            if not issubclass(type(error), Exception) and not (
                    issubclass(type(error), SystemExit) and _successful_exit(error)):
                selected = error
                break
        if issubclass(type(selected), SystemExit) and _successful_exit(selected):
            failure = RuntimeError("output close incomplete: requested a successful exit")
            _attach_case_notes(failure, notes)
            raise failure from selected
        _attach_case_notes(selected, notes)
        raise selected


class OutputLogs(OutputFiles):
    """The runner's original stdout/stderr paths and handle tuple."""

    def __init__(self, root, label):
        self.root, self.label = root, label
        super().__init__((("stdout", root / (label + "-stdout.jsonl")),
                          ("stderr", root / (label + "-stderr.txt"))))

    @property
    def stdout(self):
        return self.streams[0]

    @property
    def stderr(self):
        return self.streams[1]


def run(executable, args, root, env, label, *, interact=None, stdin_pipe=True, timeout_seconds=None):
    command_timeout = COMMAND_TIMEOUT_SECONDS if timeout_seconds is None else timeout_seconds
    cleanup_timeout = COMMAND_CLEANUP_SECONDS
    command = [str(executable), *args]
    require(GUARD_PATH.is_file(), "missing alab/command_guard.py; keep the probe in its source tree")
    result_path = root / (label + "-guardian-result.json")
    require(not result_path.exists() and not result_path.is_symlink(), "guardian result already exists")
    guardian = [sys.executable, "-I", str(GUARD_PATH), "--", sys.executable, "-I", str(GUARD_PATH),
                "--result-file", str(result_path), "--inherited-group-timeout",
                str(command_timeout), "--", *command]
    write_json(root / (label + "-command.json"), command)
    write_json(root / (label + "-guardian-command.json"), guardian)
    with OutputLogs(root, label) as (stdout, stderr):
        child, pending, cleaning = None, None, False
        primary = None
        handlers = {}

        def cancel(signum, _frame):
            nonlocal pending
            pending = signum
            # Do not interrupt Popen before it returns the child we must reap.
            if child is not None and not cleaning:
                raise SystemExit(128 + signum)

        try:
            for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
                handlers[signum] = signal.getsignal(signum)
                signal.signal(signum, cancel)
            # Keep our direct guardian PID leased until wait, even under SIG_IGN.
            handlers[signal.SIGCHLD] = signal.getsignal(signal.SIGCHLD)
            signal.signal(signal.SIGCHLD, signal.SIG_DFL)
            if pending is not None:
                raise SystemExit(128 + pending)
            # The HTTP server has threads. All preexec/group handling belongs in
            # the dedicated single-threaded guardian, never in this process.
            child = subprocess.Popen(guardian, cwd=root / "work", env=env,
                                     stdin=subprocess.PIPE if interact and stdin_pipe else subprocess.DEVNULL,
                                     stdout=subprocess.PIPE if interact else stdout, stderr=stderr)
            if pending is not None:
                raise SystemExit(128 + pending)
            if interact:
                interact(child, stdout)
                if child.stdin is not None:
                    child.stdin.close()
                    child.stdin = None
                trailing, _ = child.communicate()
                stdout.write(trailing)
                code = child.returncode
            else:
                code = child.wait()
        except BaseException as error:
            primary = error
            raise
        finally:
            cleaning = True
            cleanup_errors = []

            def attempt(phase, action):
                try:
                    action()
                    return True
                except BaseException as error:
                    cleanup_errors.append((phase, error))
                    return False

            try:
                if child is not None:
                    if child.returncode is None:
                        # Popen checks child ownership before relaying TERM: wait may
                        # have reaped it just before an interrupt prevented status storage.
                        # The guardian stops its group before exiting; never kill it.
                        def stop():
                            try:
                                child.terminate()
                            except ProcessLookupError:
                                pass

                        if attempt("terminate", stop):
                            attempt("wait", lambda: child.wait(timeout=cleanup_timeout))
                        # A failed wait/stop may already have published retirement.
                        if child.returncode is None:
                            attempt("poll", child.poll)
                    if child.returncode is None:
                        cleanup_errors.append(("retirement", RuntimeError("guardian retirement unconfirmed")))
                        # Either buffered close can wait for a callback-owned lock;
                        # stdin may also flush into a live, nonreading guardian.
                        for name, pipe in (("stdin", child.stdin), ("stdout", child.stdout)):
                            if pipe is not None:
                                cleanup_errors.append(("pipe." + name, RuntimeError(
                                    "buffered close skipped: guardian retirement unconfirmed")))
                    else:
                        for name, pipe in (("stdin", child.stdin), ("stdout", child.stdout)):
                            if pipe is not None:
                                attempt("pipe." + name + ".close", pipe.close)
                if cleanup_errors:
                    notes = [f"child cleanup {phase}: {_case_label(error)}" for phase, error in cleanup_errors]
                    if primary is not None:
                        _attach_case_notes(primary, notes)
                    else:
                        selected = cleanup_errors[0][1]
                        for _phase, error in cleanup_errors:
                            if not issubclass(type(error), Exception) and not (
                                    issubclass(type(error), SystemExit) and _successful_exit(error)):
                                selected = error
                                break
                        if issubclass(type(selected), SystemExit) and _successful_exit(selected):
                            primary = RuntimeError("child cleanup incomplete: requested a successful exit")
                            _attach_case_notes(primary, notes)
                            raise primary from selected
                        primary = selected
                        _attach_case_notes(primary, notes)
                        raise primary
            except BaseException as error:
                if primary is None:
                    primary = error
                elif error is not primary:
                    _attach_case_notes(primary, ["child cleanup: " + _case_label(error)])
                raise primary
            finally:
                restoration_errors = []
                for signum, handler in handlers.items():
                    try:
                        signal.signal(signum, handler)
                    except BaseException as error:
                        restoration_errors.append((signum, error))
                if restoration_errors:
                    notes = [f"signal restoration {signum}: {_case_label(error)}"
                             for signum, error in restoration_errors]
                    if primary is not None:
                        _attach_case_notes(primary, notes)
                    else:
                        selected = restoration_errors[0][1]
                        for _signum, error in restoration_errors:
                            if not issubclass(type(error), Exception) and not (
                                    issubclass(type(error), SystemExit) and _successful_exit(error)):
                                selected = error
                                break
                        if issubclass(type(selected), SystemExit) and _successful_exit(selected):
                            failure = RuntimeError("signal restoration incomplete: requested a successful exit")
                            _attach_case_notes(failure, notes)
                            raise failure from selected
                        _attach_case_notes(selected, notes)
                        raise selected
    if pending is not None:
        raise SystemExit(128 + pending)
    result = json.loads(result_path.read_text())
    require(isinstance(result, dict) and type(result.get("schema")) is int and result["schema"] == 1
            and result.get("argv") == command
            and type(result.get("timeout_seconds")) in (int, float)
            and result["timeout_seconds"] == command_timeout
            and type(result.get("timed_out")) is bool, "invalid guardian command evidence")
    raw = result.get("returncode")
    if result["timed_out"]:
        require(raw is None and code == 124, "guardian timeout evidence disagrees with exit")
        raise subprocess.TimeoutExpired(command, command_timeout)
    require(type(raw) is int and -signal.NSIG < raw < 256,
            "missing or invalid raw command status")
    require(code == (raw if raw >= 0 else 128 - raw), "guardian exit disagrees with raw command status")
    return raw


def sse(events):
    return "".join(f"event: {event['type']}\ndata: {json.dumps(event)}\n\n" for event in events).encode()


def response(response_id, item, usage=None):
    return sse([{"type": "response.created", "response": {"id": response_id}},
                {"type": "response.output_item.done", "item": item},
                {"type": "response.completed", "response": {"id": response_id, "usage": usage if usage is not None else {
                    "input_tokens": 0, "input_tokens_details": None, "output_tokens": 0,
                    "output_tokens_details": None, "total_tokens": 0}}}])


class Mock:
    def __init__(self, negative, quota=False, unknown_reset=False, refusal_type=None):
        self.negative = negative
        self.quota = quota
        self.unknown_reset = unknown_reset
        self.refusal_type = refusal_type
        self.requests = []
        self.lock = threading.Lock()

    def reply(self, body):
        with self.lock:
            number = len(self.requests) + 1
            record = {"at": time.monotonic(), "unixAt": time.time(), "body": body}
            self.requests.append(record)
        if self.negative or number == 2:
            status = 403 if self.negative else 503
            error = {"error": {"code": "server_is_overloaded", "message": "Model is at capacity."}}
            if self.quota:
                status = 403 if self.negative else 429
                record["quotaResetUnix"] = math.ceil(time.time()) + RETRY_SECONDS
                error = {"error": {"type": "usage_limit_reached",
                                   "message": "Fixture account quota exhausted"}}
                if not self.unknown_reset:
                    error["error"]["resets_at"] = record["quotaResetUnix"]
                if self.refusal_type:
                    status = 429
                    record["responseHeaders"] = {
                        "x-codex-rate-limit-reached-type": self.refusal_type}
            return status, json.dumps(error).encode(), "application/json", RETRY_SECONDS, record
        if number == 1:
            item = {"type": "function_call", "call_id": CALL_ID, "name": "update_plan",
                    "arguments": json.dumps({"plan": [{"step": PLAN, "status": "completed"}]})}
        elif number == 3:
            item = {"type": "message", "role": "assistant", "id": "recovered-message",
                    "content": [{"type": "output_text", "text": ANSWER}]}
        else:
            return 400, b'{"error":{"message":"Unexpected extra request"}}', "application/json", None, record
        return 200, response(f"response-{number}", item), "text/event-stream", None, record


def start_listener(handler, *, bind=None, prepare=None):
    """Own a returned binding through preparation and daemon worker handoff.

    bind(address, handler) transfers its returned server before prepare(server)
    runs. Preparation leaves any replacement socket owned by that server.
    """
    release = entered = None
    server = None
    worker = shutdown_worker = shutdown_entered = None
    committed = released = closed = False
    deadline = startup_error = None
    retired, shutdown_errors = [], []

    def release_owned():
        nonlocal released
        if release is not None:
            release.set()
            released = True

    def close_owned():
        nonlocal closed
        closed = True
        if server is not None:
            server.server_close()

    def remaining():
        nonlocal deadline
        if deadline is None:
            return 0
        try:
            return max(0, deadline - time.monotonic())
        except BaseException as failure:
            deadline = None
            _attach_case_notes(startup_error, ["listener cleanup clock unavailable: " + _case_label(failure)])
            return 0

    def shutdown():
        nonlocal shutdown_worker, shutdown_entered
        if not committed:
            return
        shutdown_entered = threading.Event()

        def request():
            try:
                shutdown_entered.set()
                server.shutdown()
            except BaseException as failure:
                shutdown_errors.append(failure)

        # Store ownership before start(), which can raise after native start.
        shutdown_worker = threading.Thread(target=request, daemon=True)
        shutdown_worker.start()

    def retire(thread, started, name):
        if thread is None:
            return
        require(started is not None and started.wait(remaining()),
                f"listener {name} worker retirement unconfirmed: startup was not observed")
        thread.join(timeout=remaining())
        require(not thread.is_alive(), f"listener {name} worker retirement unconfirmed: cleanup deadline expired")
        retired.append(thread)

    def shutdown_result():
        if shutdown_errors:
            raise shutdown_errors[0]

    # Prepare every independent safe action before acquiring a bound listener.
    actions = (("listener.release", release_owned), ("listener.shutdown", shutdown),
        ("listener.shutdown-retirement", lambda: retire(shutdown_worker, shutdown_entered, "shutdown")),
        ("listener.shutdown-result", shutdown_result),
        ("listener.serving-retirement", lambda: retire(worker, entered, "serving")),
        ("listener.close", close_owned))
    try:
        binding = provider_fixture.LoopbackHTTPServer if bind is None else bind
        server = binding(("127.0.0.1", 0), handler)
        if prepare is not None:
            prepare(server)
        release, entered = threading.Event(), threading.Event()

        def serve():
            entered.set()
            release.wait()
            if committed:
                server.serve_forever()

        worker = threading.Thread(target=serve, daemon=True)
        worker.start()
        committed = True
        release.set()
        return server, worker
    except BaseException as error:
        if server is None:
            raise  # A failed constructor transferred no bound listener.
        startup_error = error
        try:
            try:
                deadline = time.monotonic() + LISTENER_STARTUP_CLEANUP_SECONDS
            except BaseException as failure:
                # No usable clock means nonblocking retirement observations,
                # not permission to skip shutdown dispatch or other cleanup.
                _attach_case_notes(error, ["listener cleanup clock unavailable: " + _case_label(failure)])
            try:
                cleanup_resources((), actions, {"passed": False}, error)
            except BaseException as cleanup_error:
                if cleanup_error is not error:
                    _attach_case_notes(error, ["listener startup cleanup: " + _case_label(cleanup_error),
                        *_case_notes(cleanup_error, "__notes__"),
                        *_case_notes(cleanup_error, "_alab_case_cleanup_errors")])
        finally:
            # Emergency release/close still apply if cleanup setup itself fails.
            for name, action, done in (("release", release_owned, released), ("close", close_owned, closed)):
                if not done:
                    try:
                        action()
                    except BaseException as cleanup_error:
                        _attach_case_notes(error, ["listener startup cleanup " + name + ": " + _case_label(cleanup_error)])
            for name, thread in (("serving", worker), ("shutdown", shutdown_worker)):
                if thread is not None and not any(thread is observed for observed in retired):
                    _attach_case_notes(error, [f"listener {name} worker retirement unconfirmed"])
            # Cleanup or optional diagnostics cannot replace the startup outcome.
            raise error


def serve(mock):
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            request_started = time.monotonic()
            if self.path != "/v1/responses":
                self.send_error(404)
                return
            length = int(self.headers.get("content-length", "0"))
            if not 0 < length <= 8 * 1024 * 1024:
                self.send_error(413)
                return
            body = json.loads(self.rfile.read(length))
            status, data, content_type, retry_after, record = mock.reply(body)
            with mock.lock:
                record["requestStartedAt"] = request_started
            self.send_response(status)
            self.send_header("content-type", content_type)
            self.send_header("content-length", str(len(data)))
            if retry_after is not None:
                self.send_header("Retry-After", str(retry_after))
            for key, value in record.get("responseHeaders", {}).items():
                self.send_header(key, value)
            header_started = time.monotonic()
            self.end_headers()
            header_at = time.monotonic()
            # Bind timing to this exact response even if another request arrives.
            # Retry-After starts at headers; body transmission must not move it.
            with mock.lock:
                record.update(headerStartedAt=header_started, headerAt=header_at)
            self.wfile.write(data)

    return start_listener(Handler)


def configure(root, port, *, stream_max_retries=0):
    config = f'''model = "{MODEL}"
model_provider = "capacity-smoke"
approval_policy = "never"
sandbox_mode = "read-only"
[tools.update_plan]
enabled = true
[model_providers.capacity-smoke]
name = "local capacity smoke"
base_url = "http://127.0.0.1:{port}/v1"
wire_api = "responses"
requires_openai_auth = false
request_max_retries = 0
stream_max_retries = {stream_max_retries}
'''
    with (root / "codex-home/config.toml").open("x") as handle:
        handle.write(config)
