#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Run the actual paired renewal cases and retain exact image-bound evidence."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

BASE_CASES = ("happy", "lost-signal-ack", "panic-before-signal", "lost-consume-ack",
              "late-hold", "owner-stop")
CASES = BASE_CASES + ("weekly-quota", "weekly-quota-rollback", "weekly-quota-exhausted", "weekly-quota-chain", "grace-beyond-budget", "post-signal-resume", "owner-stop-after-signal",
                      "failed-forward-rollback", "successor-prefix-crash")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True, type=Path)
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--case", action="append", choices=CASES)
    parser.add_argument("--quota", action="store_true", help="Use distinct quota accounts for fault cases")
    args = parser.parse_args()
    controller = Path(__file__).with_name("renewal_follow.py").resolve(strict=True)
    host = args.host.resolve(strict=True)
    package = args.package.resolve(strict=True)
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    pinned = (Path(__file__).resolve(), controller, controller.with_name("capacity_exec.py"),
              controller.parent.parent / "command_guard.py", host,
              host.parent / "build.json", package / "bin" / "codex",
              package / "alab-build.json", package / "codex-package.json")
    requested = args.case or CASES
    if len(set(requested)) != len(requested):
        parser.error("each case may be requested only once")
    report = {"passed": False, "scope": "actual paired native/Codex fixture cases",
              "quotaAccounts": args.quota,
              "requestedCases": list(requested), "allSixCasesPassed": False,
              "allAvailableCasesPassed": False,
              "coversLiveEmailAuthentication": False, "coversAllActivationRequirements": False,
              "inputs": {str(path): digest(path) for path in pinned}, "cases": []}
    manifest = root / "qualification.json"

    def save():
        manifest.write_text(json.dumps(report, indent=2) + "\n")

    def unchanged():
        if any(digest(path) != report["inputs"][str(path)] for path in pinned):
            raise RuntimeError("qualification source or image changed during matrix")

    save()
    try:
        for case in requested:
            unchanged()
            case_root = root / case
            with (root / (case + ".log")).open("xb") as log:
                command = [sys.executable, str(controller), "--host", str(host),
                           "--package", str(package), "--output", str(case_root), "--case", case]
                if args.quota:
                    command.append("--quota")
                completed = subprocess.run(
                    command,
                    stdout=log, stderr=subprocess.STDOUT,
                )
            unchanged()
            result_path = case_root / "result.json"
            result = json.loads(result_path.read_text()) if result_path.is_file() else None
            passed = bool(completed.returncode == 0 and result and result.get("passed") is True
                          and result.get("guardian", {}).get("returncode") == 0
                          and result.get("guardian", {}).get("timed_out") is False
                          and result.get("rawHostReturncode") == 0)
            report["cases"].append({"case": case, "passed": passed,
                                    "controllerExit": completed.returncode,
                                    "result": str(result_path),
                                    "resultSha256": digest(result_path) if result else None})
            save()
            print(json.dumps(report["cases"][-1]), flush=True)
            if not passed:
                raise RuntimeError("paired case failed: " + case)
        report["passed"] = True
        report["allSixCasesPassed"] = set(BASE_CASES).issubset(requested)
        report["allAvailableCasesPassed"] = set(requested) == set(CASES)
    except Exception as error:
        report["error"] = type(error).__name__ + ": " + str(error)
    finally:
        save()
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
