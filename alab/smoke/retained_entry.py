#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check ordinary terminal resume after retained service entry lacks a native controller."""

import argparse
import json
from pathlib import Path
import sys

import capacity_exec as fixture
import model_resume


CONTROLLER_REQUIRED = "retained session control requires an aterm session endpoint"
MALFORMED_ENTRY = "retained frontend entry unavailable"


def rejected_terminal(binary, prefix, thread, root, env, label):
    raw = root / (label + ".raw")
    custody = root / (label + "-custody.json")
    spec = root / "tmp" / (label + ".json")
    command = [str(binary), *prefix, "--no-daemon", "--no-alt-screen", "resume", thread, model_resume.CONTINUE]
    spec.write_text(json.dumps({"argv": command, "cwd": str(root / "work"), "env": env,
                                "raw": str(raw), "result": str(custody)}))
    # Holding stdin open lets the PTY observe the application's own exit.
    status = fixture.run(sys.executable, ["-I", "-B", "-S", str(model_resume.HERE / "history_pagination.py"),
                                         "--guardian", str(spec)], root, env, label,
                         interact=lambda process, _log: process.wait(timeout=15))
    receipt = json.loads(custody.read_text())
    fixture.require(status == receipt["exit"] == 1 and not receipt["forced"],
                    f"{label} did not refuse through its own terminal process")
    return raw.read_text().strip()


def run(binary, seed_binary, root):
    root.mkdir(mode=0o700)
    env = fixture.fresh_environment(root)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm")
    report = {"passed": False, "binary_sha256": model_resume.CHECK["sha256"](binary),
              "seed_binary_sha256": model_resume.CHECK["sha256"](seed_binary), "controls": [], "events": []}
    completed = report["completed_turns"] = []
    gateway = model_resume.Gateway(report["events"], live=False)
    listener = None
    try:
        listener = fixture.start_listener(gateway.handler())
        port = listener[0].server_port
        model_resume.configure(root, port, "ultra")
        (root / "codex-home/auth.json").write_text(json.dumps({"OPENAI_API_KEY": "sk-loopback-fixture"}))
        receipt, _, _ = model_resume.terminal_session(seed_binary, root, env, "seed", [model_resume.SEED],
                                                       gateway, completed)
        fixture.require(receipt["exit"] == 0 and not receipt["forced"], "original terminal failed")
        original, = [row for row in report["events"] if row["event"] == "request"]
        thread = report["thread"] = original["thread"]
        gateway.publish_successor(receipt["exit"], thread)
        model_resume.configure(root, port, "low", model_preference=model_resume.OTHER)
        rollout, = (root / "codex-home/sessions").rglob("rollout-*.jsonl")
        saved_digest = model_resume.CHECK["sha256"](rollout)
        for label, prefix, expected in [
            ("malformed-frontend", ["--alab-retained-frontend", "bad", "b" * 32], MALFORMED_ENTRY),
            ("unadmitted-frontend", ["--alab-retained-frontend", "a" * 32, "b" * 32], CONTROLLER_REQUIRED),
            ("unadmitted-backend", ["--alab-retained-service"], CONTROLLER_REQUIRED),
        ]:
            text = rejected_terminal(binary, prefix, thread, root, env, label)
            with gateway.lock:
                requests = sum(row["event"] == "request" for row in report["events"])
            unchanged = model_resume.CHECK["sha256"](rollout) == saved_digest
            report["controls"].append({"case": label, "exit": 1, "expected_reason": text == "Error: " + expected,
                                       "provider_requests": requests, "rollout_unchanged": unchanged})
            fixture.require(requests == 1 and unchanged, f"{label} changed the saved conversation")
        receipt, _, _ = model_resume.terminal_session(
            binary, root, env, "terminal", ["resume", thread, model_resume.CONTINUE], gateway, completed)
        fixture.require(receipt["exit"] == 0 and not receipt["forced"], "public resume failed")
        original, resumed = [row for row in report["events"] if row["event"] == "request"]
        fixture.require(original["thread"] == resumed["thread"] == thread
                        and resumed["seed_prompt"] and resumed["prior_reply"] and resumed["resume_prompt"]
                        and resumed["model"] == model_resume.NEW and resumed["effort"] == "high"
                        and [original["turn"], resumed["turn"]] == completed and len(set(completed)) == 2,
                        "public resume lost the original history, effort or turn ownership")
        fixture.require(all(row["expected_reason"] for row in report["controls"]),
                        "ordinary CLI reported the wrong reason for refused retained entry")
        fixture.require(model_resume.CHECK["sha256"](binary) == report["binary_sha256"]
                        and model_resume.CHECK["sha256"](seed_binary) == report["seed_binary_sha256"],
                        "a CLI image changed during the entry check")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        fixture.cleanup_resources((("catalog", *listener),) if listener else (), (), report, sys.exc_info()[1])
        fixture.write_json(root / "result.json", report)
    return report["passed"]


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--seed-binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    options = parser.parse_args()
    sys.exit(0 if run(options.binary.resolve(), (options.seed_binary or options.binary).resolve(),
                      options.output.resolve()) else 1)
