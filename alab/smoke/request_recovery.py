#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check WebSocket startup, HTTP and streamed capacity failures, and opaque HTTP 400 recovery.

Every case uses a private home, a loopback model, and the command guardian.
The model rejects startup, the stream carrying a plan call, or a subsequent request.
A successful retry must preserve the tool result. Specific refusals remain terminal.
Use --compaction-only for app-server compaction retries and permission refusals.
Requires Python 3.11 or newer for secondary exception diagnostics.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy
import subprocess
import sys
import threading
import time

import capacity_exec as probe
from provider_fixture import turn_events

CLI = runpy.run_path(str(Path(__file__).resolve().parents[1] / "cli_status.py"))

OVERLOAD = {"code": "server_is_overloaded", "message": "Model is at capacity."}
MISALIGNMENT = {"code": "misalignment_policy_violation", "message": "Fixture policy review is required."}
REPEATED_WAITS = (probe.RETRY_SECONDS, 3)
OPEN_STREAM_CASES = ("streamed-overload", "late-stream-overload")
STREAM_FAILURE = {"type": "response.failed", "response": {
    "id": "response-2", "status": "failed", "error": OVERLOAD}}
CAPACITY_CASES = (
    ("overload", 503, {"error": OVERLOAD}, True),
    ("streamed-overload", 200, STREAM_FAILURE, True),
    ("late-stream-overload", 200, STREAM_FAILURE, True),
    ("tool-stream-overload", 200, {"type": "response.failed", "response": {
        "id": "response-1", "status": "failed", "error": OVERLOAD}}, True),
    ("prewarm-overload", 503, {"error": OVERLOAD}, True),
    ("repeated-overload", 503, {"error": OVERLOAD}, True),
    ("unadvised-overload", 503, {"error": OVERLOAD}, True),
)
CASES = CAPACITY_CASES + (
    ("forbidden-overload", 403, {"error": OVERLOAD}, False),
    ("opaque", 400, {"detail": "Bad Request"}, True),
    ("specific", 400, {"error": {"code": "invalid_prompt", "message": "Invalid tool schema"}}, False),
    ("misalignment-400", 400, {"error": MISALIGNMENT}, False),
    ("misalignment-403", 403, {"error": MISALIGNMENT}, False),
    ("streamed-misalignment", 200, {"type": "response.failed", "response": {
        "id": "response-2", "status": "failed", "error": MISALIGNMENT}}, False),
    ("array", 400, ["Bad Request"], False),
)


class Rejection(probe.Mock):
    def __init__(self, status, envelope, *, fail_with_tool=False, waits=(probe.RETRY_SECONDS,), capacity_codes=()):
        super().__init__(False)
        self.status = status
        self.envelope = envelope
        self.waits = waits
        self.capacity_codes = capacity_codes
        self.fail_with_tool = fail_with_tool

    def reply_for_attempt(self, number, record):
        if self.fail_with_tool:
            if number == 1:
                events = [{"type": "response.created", "response": {"id": "response-1"}},
                          {"type": "response.output_item.done", "item": probe.plan_call()},
                          self.envelope]
                return 200, probe.sse(events), "text/event-stream", probe.RETRY_SECONDS, record
            # The first response included the rejection. Resume at the answer.
            return super().reply_for_attempt(number + 1, record)
        if number == 1:
            return super().reply_for_attempt(number, record)
        refusal_index = number - 2
        if refusal_index < len(self.waits):
            record["responseHeaders"] = {"x-request-id": f"req_opaque_smoke-{refusal_index + 1}"}
            advice = self.waits[refusal_index]
            if self.status == 200:
                return 200, probe.sse([self.envelope]), "text/event-stream", advice, record
            envelope = self.envelope
            if self.capacity_codes:
                code = self.capacity_codes[refusal_index]
                envelope = {"error": {**envelope["error"], "code": code}}
                record["capacityCode"] = code
            return self.status, json.dumps(envelope).encode(), "application/json", advice, record
        # The base fixture sends its answer on attempt three and rejects any extra request.
        return super().reply_for_attempt(3 + refusal_index - len(self.waits), record)


class OpenFailureStream(Rejection):
    def __init__(self, status, envelope, *, expire_advice=False):
        super().__init__(status, envelope)
        self.release = threading.Event()
        self.stream_open = threading.Event()
        self.expire_advice = expire_advice

    def send_failure_stream(self, stream, frame, record):
        if self.expire_advice:
            stream.write(b": waiting for failure\n\n")
            stream.flush()
            if self.release.wait(record["retryAfter"]):
                return
        record["failureFrameStartedAt"] = time.monotonic()
        stream.write(frame)
        stream.flush()
        record["failureFrameFlushedAt"] = time.monotonic()
        self.stream_open.set()
        try:
            self.release.wait()
        finally:
            self.stream_open.clear()

    def reply_for_attempt(self, number, record):
        if number == 2:
            record["holdFailureStream"] = True
        if number == 3:
            record["arrivedDuringFailedStream"] = self.stream_open.is_set()
            self.release.set()
        return super().reply_for_attempt(number, record)


class PrewarmRejection(Rejection):
    def __init__(self, status, envelope):
        super().__init__(status, envelope, waits=())
        self.handshakes = []

    def upgrade(self, headers):
        try:
            metadata = json.loads(headers.get("x-codex-turn-metadata", "{}"))
        except ValueError:
            metadata = {}
        if not isinstance(metadata, dict):
            metadata = {}
        kind = metadata.get("request_kind")
        if kind == "prewarm":
            status, advice, body = self.status, probe.RETRY_SECONDS, self.envelope
        else:
            status = 426 if kind == "turn" else 400
            advice, body = None, {"error": {"message": "Use the HTTP transport"}}
        record = {"requestKind": kind, "threadId": metadata.get("thread_id"), "turnId": metadata.get("turn_id"),
                  "status": status, "retryAfter": advice, "responseHeaders": {"Connection": "close"}}
        with self.lock:
            self.handshakes.append(record)
        return status, json.dumps(body).encode(), "application/json", advice, record


def partial_observations(root, *, compaction):
    """Read surviving evidence after teardown without inferring an outcome."""
    result = {}
    try:
        requests = json.loads((root / "requests.json").read_bytes())
        if isinstance(requests, list):
            result["requestCount"] = len(requests)
    except (OSError, ValueError, RecursionError):
        pass
    log = "app-stdout.jsonl" if compaction else "exec-stdout.jsonl"
    result.update(probe.provider_fixture.partial_stdout_observations(root / log, compaction=compaction))
    return result


def check_result(code, events, requests, recover, *, expect_capacity_notice=False, fail_with_tool=False,
                 waits=(probe.RETRY_SECONDS,), handshakes=None, case_dir=None, refusal=None):
    types = [event.get("type") for event in events]
    expected = 2
    if recover:
        expected = (1 if fail_with_tool else 2) + len(waits)
    probe.require(len(requests) == expected,
                  f"expected {expected} requests, observed {len(requests)}")
    delay = None
    if handshakes is not None:
        delay = probe.provider_fixture.prewarm_retry_delay(handshakes + requests, probe.RETRY_SECONDS)
    try:
        identity = probe.provider_fixture.check_capacity_request_identity(
            events, requests, probe.MODEL, handshake=handshakes[1] if handshakes is not None else None)
        probe.provider_fixture.check_capacity_history(requests, fail_with_tool=fail_with_tool)
    except ValueError as error:
        raise RuntimeError(str(error)) from error
    answers = [(index, event["item"].get("text")) for index, event in enumerate(events)
               if event.get("type") == "item.completed"
               and event.get("item", {}).get("type") == "agent_message"]
    if recover:
        probe.require(code == 0, "retryable error did not recover")
        attempts = requests[-(len(waits) + 1):]
        pairs = zip(attempts[:-1], attempts[1:], waits, strict=True)
        for attempt, (failed, retry, seconds) in enumerate(pairs, 1):
            delay = probe.retry_after_delay(failed, retry, seconds, attempt=attempt if expect_capacity_notice else None)
        probe.require("error" not in types, "recovery stream emitted a top-level JSON error")
        probe.require(turn_events(events) == ["turn.started", "turn.completed"],
                      "recovery did not retain one successful turn")
        probe.require(len(answers) == 1 and answers[0][1] == probe.ANSWER,
                      "missing or duplicated recovered answer")
        probe.require(types.index("turn.started") < answers[0][0] < types.index("turn.completed"),
                      "recovered answer is outside its original turn")
        if expect_capacity_notice:
            notice_waits = (probe.RETRY_SECONDS,) if handshakes is not None else waits
            failures = handshakes[:1] if handshakes is not None else attempts[:-1]
            try:
                probe.provider_fixture.check_capacity_notices(events, answers[0][0], notice_waits, failures)
            except ValueError as error:
                raise RuntimeError(str(error)) from error
    else:
        probe.require(code == 1, "specific refusal did not exit with failure")
        probe.require(turn_events(events) == ["turn.started", "turn.failed"],
                      "specific refusal did not fail its original turn")
        probe.require(not answers, "specific refusal emitted an assistant answer")
        probe.require(not probe.provider_fixture.capacity_notices(events, include_legacy=True),
                      "terminal refusal announced an automatic capacity retry")
        if refusal is not None:
            error = next(event.get("error") for event in events if event.get("type") == "turn.failed")
            probe.require(isinstance(error, dict) and error.get("message") == refusal["message"],
                          "terminal turn did not report the fixture refusal")
    if case_dir is not None:
        probe.provider_fixture.check_capacity_rollout(
            case_dir, *identity, requests=requests, answer=probe.ANSWER if recover else None, failure=refusal)
    return delay


def expected_refusal(status, envelope, port):
    """Exec JSON exposes the message. The saved turn also preserves the native error kind."""
    payload = envelope["response"] if status == 200 else envelope
    if isinstance(payload, list):
        return {"message": json.dumps(payload), "codex_error_info": "other"}
    message = payload["error"]["message"]
    kind = payload["error"]["code"]
    if status == 403 and kind != "misalignment_policy_violation":
        message = (f"unexpected status 403 Forbidden: {message}, "
                   f"url: http://127.0.0.1:{port}/v1/responses, request id: req_opaque_smoke-1")
        kind = {"http_connection_failed": {"http_status_code": status}}
    return {"message": message, "codex_error_info": kind}


def case(codex, output, name, status, envelope, recover):
    root = output / name
    root.mkdir()
    env = probe.fresh_environment(root)
    fail_with_tool = name == "tool-stream-overload"
    waits = {"repeated-overload": REPEATED_WAITS,
             "unadvised-overload": (None, None, 1)}.get(name, (probe.RETRY_SECONDS,))
    startup = name == "prewarm-overload"
    timed_notices = startup or name in ("repeated-overload", "unadvised-overload")
    codes = probe.provider_fixture.REPEATED_CAPACITY_CODES if name == "repeated-overload" else ()
    if startup:
        waits = ()
        model = PrewarmRejection(status, envelope)
        server, thread = probe.serve(model, upgrade=model.upgrade)
    elif name in OPEN_STREAM_CASES:
        model = OpenFailureStream(status, envelope, expire_advice=name == "late-stream-overload")
        server, thread = probe.serve(model, send_failure_stream=model.send_failure_stream)
    else:
        model = Rejection(status, envelope, fail_with_tool=fail_with_tool, waits=waits, capacity_codes=codes)
        server, thread = probe.serve(model)

    def trace():
        return model.handshakes + model.requests if startup else model.requests

    result = {"case": name, "passed": False}
    exports = [("write", lambda: probe.write_json(root / "requests.json", trace()))]
    primary = None
    try:
        # Overload must recover even with the ordinary stream retry budget exhausted.
        capacity = any(name == case[0] for case in CAPACITY_CASES)
        probe.configure(root, server.server_port, stream_max_retries=0 if capacity else 2,
                        supports_websockets=startup)
        args = ["exec", "--json", "--color", "never", "--skip-git-repo-check",
                "--cd", str(root / "work"), "--sandbox", "read-only", probe.PROMPT]
        if timed_notices:
            observed = []
            # The guardian can time out after delivering a notice to stdout.
            exports.append(("write stdout times", lambda: probe.write_json(root / "exec-observed.json", observed)))

            def read_output(child, log):
                for line in child.stdout:
                    if line.strip():
                        observed.append(time.monotonic())
                    log.write(line)

            code = probe.run(codex, args, root, env, "exec", interact=read_output, stdin_pipe=False)
        else:
            code = probe.run(codex, args, root, env, "exec")
        result.update(requestCount=len(trace()), returncode=code)
        events = probe.read_case_stdout(root, result, compaction=False)
        if events is None:
            return result
        result["turnEvents"] = turn_events(events)
        try:
            elapsed_field = "localBackoffSeconds" if name == "unadvised-overload" else "retryAfterSeconds"
            result[elapsed_field] = check_result(
                code, events, model.requests, recover, expect_capacity_notice=capacity,
                fail_with_tool=fail_with_tool, waits=waits, handshakes=model.handshakes if startup else None,
                case_dir=root, refusal=None if recover else expected_refusal(status, envelope, server.server_port))
            if startup:
                probe.provider_fixture.check_live_notices(events, observed, model.handshakes,
                                                         (probe.RETRY_SECONDS,), startup=True)
            elif timed_notices:
                probe.provider_fixture.check_live_notices(events, observed, model.requests[1:], waits,
                                                         expected_codes=codes or None)
            if name in OPEN_STREAM_CASES:
                probe.provider_fixture.check_open_stream_retry(*model.requests[1:])
                if name == "late-stream-overload":
                    probe.provider_fixture.check_expired_stream_advice(*model.requests[1:], waits[0], events)
            result["passed"] = True
        except (OSError, RuntimeError, ValueError) as error:
            result["error"] = str(error)
        return result
    except BaseException as error:
        primary = error
        raise
    finally:
        if name in OPEN_STREAM_CASES:
            model.release.set()
        probe.cleanup_resources((("", server, thread),), exports, result, primary)


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def _arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    selection = parser.add_mutually_exclusive_group(required=True)
    selection.add_argument("--codex", type=Path)
    selection.add_argument("--package", type=Path)
    parser.add_argument("--expected-source", help="require the package build record to name this full Git commit")
    parser.add_argument("--output", type=Path, required=True)
    scope = parser.add_mutually_exclusive_group()
    scope.add_argument("--capacity-only", action="store_true",
                       help="exercise HTTP and streamed model overload, including interrupted tool work")
    scope.add_argument("--compaction-only", action="store_true",
                       help="exercise legacy and v2 compaction through the app-server")
    args = parser.parse_args()
    if args.expected_source is not None and (
            args.package is None or not re.fullmatch(r"[0-9a-f]{40}", args.expected_source)):
        parser.error("--expected-source requires --package and a full lowercase Git commit")
    if sys.version_info < (3, 11):
        parser.error("the request recovery smoke requires Python 3.11 or newer")
    return args


def main(args=None):
    if args is None:
        args = _arguments()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    report = {"passed": False, "cases": []}
    interrupted = None
    try:
        expected = getattr(args, "expected_source", None)
        if expected is not None:
            report["expectedSource"] = expected
        build = None
        if args.package:
            build, _, codex = probe.package_records(args.package.resolve(strict=True))
        else:
            codex = args.codex.resolve(strict=True)
        report.update(binary=str(codex), package=build)
        if expected is not None:
            probe.require(build is not None, "--expected-source requires --package")
            probe.require(build["source"] == expected, f"expected package source {expected}, found {build['source']}")
        before = digest(codex)
        report["sha256"] = before
        if build is not None:
            probe.check_package_executable(build, before)
        compaction = getattr(args, "compaction_only", False)
        if compaction:
            from compaction_recovery import COMPACTION_CASES, compaction_case
            cases, run_case = COMPACTION_CASES, compaction_case
        else:
            cases = CAPACITY_CASES if args.capacity_only else CASES
            run_case = case
        for name, *parameters in cases:
            report["activeCase"] = name
            try:
                result = run_case(codex, output, name, *parameters)
            except Exception as error:
                result = {"case": name, "passed": False, **probe.case_error_fields(error)}
                if compaction or isinstance(error, subprocess.TimeoutExpired):
                    result.update(partial_observations(output / name, compaction=compaction))
            report["cases"].append(result)
            probe.write_json(output / name / "result.json", result)
            del report["activeCase"]
        try:
            if digest(codex) != before:
                report["error"] = "binary changed during the smoke"
        except OSError as error:
            report["error"] = f"binary identity unavailable after smoke: {error}"
        report["passed"] = "error" not in report and all(item["passed"] for item in report["cases"])
    except Exception as error:
        report.update(probe.case_error_fields(error))
    except (KeyboardInterrupt, SystemExit) as error:
        interrupted = error
        report.update(passed=False, interrupted=True, **probe.case_error_fields(error))
    try:
        if interrupted is not None and "activeCase" in report:
            try:
                report["activeCaseObservations"] = partial_observations(
                    output / report["activeCase"], compaction=compaction)
            except BaseException as read_error:
                # Keep the first cancellation and still attempt to save the summary.
                report["activeCaseObservationError"] = probe.case_error_fields(read_error)["error"]
        probe.write_json(output / "result.json", report)
        print(json.dumps(report, indent=2))
    finally:
        # Saving incomplete evidence must not replace the original cancellation,
        # including when the filesystem or output pipe also fails.
        if interrupted is not None:
            raise interrupted
    return 0 if report["passed"] else 1


def cli():
    # Help and argument errors finish before an observation starts. The API
    # preserves cancellation objects; the CLI must refuse incomplete success.
    args = _arguments()
    return CLI["run_operation"](lambda: main(args), "request recovery: ")


if __name__ == "__main__":
    sys.exit(cli())
