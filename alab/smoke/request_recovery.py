#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check opaque HTTP 400 recovery in an existing Codex binary; build nothing.

Every case uses a private home, a loopback model, and the command guardian.
The model completes a plan step before rejecting the next request. A successful
retry must preserve that input; specific refusals must remain terminal.
Requires Python 3.11 or newer for secondary exception diagnostics.
"""
import argparse
import hashlib
import json
from pathlib import Path
import runpy
import sys

import capacity_exec as probe

CLI = runpy.run_path(str(Path(__file__).resolve().parents[1] / "cli_status.py"))

CASES = (
    ("overload", 503, {"error": {"code": "server_is_overloaded", "message": "Model is at capacity."}}, True),
    ("opaque", 400, {"detail": "Bad Request"}, True),
    ("specific", 400, {"error": {"code": "invalid_prompt", "message": "Invalid tool schema"}}, False),
    ("array", 400, ["Bad Request"], False),
)

# ResponseItem's native work tags; unrelated history metadata is not tool work.
TOOL_CALL_TYPES = frozenset(("function_call", "custom_tool_call", "local_shell_call",
                             "tool_search_call", "web_search_call", "image_generation_call"))
TOOL_OUTPUT_TYPES = frozenset(("function_call_output", "custom_tool_call_output", "tool_search_output"))


class Rejection(probe.Mock):
    def __init__(self, status, envelope):
        super().__init__(False)
        self.status = status
        self.envelope = envelope

    def reply(self, body):
        status, data, content_type, retry_after, record = super().reply(body)
        if status == 503:
            record["responseHeaders"] = {"x-request-id": "req_opaque_smoke-1"}
            return self.status, json.dumps(self.envelope).encode(), content_type, retry_after, record
        return status, data, content_type, retry_after, record


def check_result(code, events, requests, recover):
    types = [event.get("type") for event in events]
    expected = 3 if recover else 2
    probe.require(len(requests) == expected,
                  f"expected {expected} requests, observed {len(requests)}")
    original = requests[0]["body"]["input"]
    history = requests[1]["body"]["input"]
    probe.require(bool(original) and history[:len(original)] == original,
                  "original request input was lost or changed")
    # This fresh exec receives exactly one fixture tool call. Another ID must
    # not hide duplicated work or an unpaired record from the oracle.
    calls = [item for item in history if item.get("type") in TOOL_CALL_TYPES]
    probe.require(len(calls) == 1 and calls[0].get("type") == "function_call"
                  and calls[0].get("call_id") == probe.CALL_ID
                  and calls[0].get("name") == "update_plan"
                  and calls[0].get("namespace") in (None, "", "functions"),
                  "completed plan call is missing, duplicated or changed")
    # History retains the fixture's raw argument string, not a re-encoded view.
    # Equality between two requests alone cannot prove that the first kept it.
    expected_arguments = json.dumps({"plan": [{"step": probe.PLAN, "status": "completed"}]})
    probe.require(calls[0].get("arguments") == expected_arguments,
                  "completed plan call arguments differ from the fixture")
    outputs = [item for item in history if item.get("type") in TOOL_OUTPUT_TYPES]
    probe.require(len(outputs) == 1 and outputs[0].get("type") == "function_call_output"
                  and outputs[0].get("call_id") == probe.CALL_ID
                  and outputs[0].get("name") in (None, "update_plan")
                  and outputs[0].get("namespace") in (None, "", "functions"),
                  "completed plan output is missing, duplicated or changed")
    probe.require(outputs[0].get("output") == "Plan updated",
                  "completed plan output does not record the successful update")
    probe.require(history.index(calls[0]) < history.index(outputs[0]),
                  "completed plan result precedes its call")
    if recover:
        probe.require(code == 0, "retryable error did not recover")
        probe.require("error" not in types, "recovery stream reported a fatal error")
        probe.require(types.count("turn.started") == types.count("turn.completed") == 1
                      and "turn.failed" not in types,
                      "recovery did not retain one successful turn")
        probe.require(requests[2]["body"]["input"] == requests[1]["body"]["input"],
                      "retry changed the input or completed tool output")
        answers = [(index, event["item"].get("text")) for index, event in enumerate(events)
                   if event.get("type") == "item.completed"
                   and event.get("item", {}).get("type") == "agent_message"]
        probe.require(len(answers) == 1 and answers[0][1] == probe.ANSWER,
                      "missing or duplicated recovered answer")
        probe.require(types.index("turn.started") < answers[0][0] < types.index("turn.completed"),
                      "recovered answer is outside its original turn")
        return probe.retry_after_delay(requests[1], requests[2])
    probe.require(code == 1, "specific refusal did not exit with failure")
    probe.require(types.count("turn.failed") == 1 and "turn.completed" not in types,
                  "specific refusal was retried or reported as successful")
    return None


def case(codex, output, name, status, envelope, recover):
    root = output / name
    root.mkdir()
    env = probe.fresh_environment(root)
    model = Rejection(status, envelope)
    server, thread = probe.serve(model)
    result = {"case": name, "passed": False}
    primary = None
    try:
        probe.configure(root, server.server_port, stream_max_retries=2)
        code = probe.run(codex, ["exec", "--json", "--color", "never", "--skip-git-repo-check",
                               "--cd", str(root / "work"), "--sandbox", "read-only", probe.PROMPT],
                         root, env, "exec")
        events = [json.loads(line) for line in (root / "exec-stdout.jsonl").read_text().splitlines()
                  if line.strip()]
        result.update(requestCount=len(model.requests), returncode=code)
        try:
            result["retryAfterSeconds"] = check_result(code, events, model.requests, recover)
            result["passed"] = True
        except RuntimeError as error:
            result["error"] = str(error)
        return result
    except BaseException as error:
        primary = error
        raise
    finally:
        probe.cleanup_case(
            server, thread, lambda: probe.write_json(root / "requests.json", model.requests), result, primary)


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
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
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
        build = None
        if args.package:
            build, _, codex = probe.package_records(args.package.resolve(strict=True))
        else:
            codex = args.codex.resolve(strict=True)
        report.update(binary=str(codex), package=build)
        before = digest(codex)
        report["sha256"] = before
        for name, status, envelope, recover in CASES:
            report["activeCase"] = name
            try:
                result = case(codex, output, name, status, envelope, recover)
            except Exception as error:
                result = {"case": name, "passed": False, **probe.case_error_fields(error)}
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
