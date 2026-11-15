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

import capacity_exec as probe
import native_host_record
import package_standing
import renewal_follow

BASE_CASES = ("happy", "lost-signal-ack", "panic-before-signal", "lost-consume-ack",
              "late-hold", "owner-stop")
CASES = BASE_CASES + ("weekly-quota", "weekly-quota-rollback", "weekly-quota-exhausted", "weekly-quota-chain", "grace-beyond-budget", "post-signal-resume", "owner-stop-after-signal",
                      "failed-forward-rollback", "successor-prefix-crash", "five-hour-wall-soon",
                      "five-hour-wall-far")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def pinned_inputs(host, package):
    """Every file whose bytes decide what a matrix run does and what it accepts.

    Their hashes are written to qualification.json before the first case and compared
    around every case: the controller and what it imports (the model, the provider
    fixture that decides what is served, the host record's rules that decide which host
    is accepted and the package standing's that decide which package is, the guardian),
    the host and its record, and the package under test.
    """
    smoke = Path(__file__).resolve().parent
    return (Path(__file__).resolve(), smoke / "renewal_follow.py", smoke / "capacity_exec.py",
            smoke / "provider_fixture.py", smoke / "native_host_record.py",
            smoke / "package_standing.py",
            smoke.parent / "command_guard.py", host, host.parent / "build.json",
            package / "bin" / "codex", package / "alab-build.json",
            package / "codex-package.json")


def case_passed(case, returncode, result):
    """Whether one case's run counts: `result` is its result.json, or None when it left none.

    The controller exited 0 and reported a pass under a guardian that neither failed nor
    timed out, the host exited 0, and a case with an oracle of its own
    (renewal_follow.CASE_PROOFS) left that oracle's record: a `five-hour-wall-far` result
    that passed on the weekly oracle alone is not one.
    """
    if returncode != 0 or not isinstance(result, dict) or result.get("passed") is not True:
        return False
    guardian = result.get("guardian") or {}
    proof = renewal_follow.CASE_PROOFS.get(case)
    return bool(guardian.get("returncode") == 0 and guardian.get("timed_out") is False
                and result.get("rawHostReturncode") == 0
                and (proof is None or (isinstance(result.get(proof), dict) and result[proof])))


def package_refusal(package):
    """Why the package under test qualifies no run, or None: read from its own record.

    A trial package (trial_package.py), or one whose record no certified packager wrote,
    is refused as a trial host is: a single case run with it is trial evidence.
    """
    try:
        build = probe.package_records(package)[0]
    except (OSError, ValueError, KeyError, TypeError, RuntimeError) as error:
        return f"its records do not load: {type(error).__name__}: {error}"
    return package_standing.not_qualifying(build)


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
    manifest = root / "qualification.json"
    try:
        refusal = native_host_record.not_qualifying(native_host_record.load(host))
        native_host_record.require_aterm_name(host)
        # This file is the qualification: `passed` is read without the record's sentence.
        native_host_record.require(refusal is None, "the native host record qualifies no "
            f"run: {refusal}; run a case singly with renewal_follow.py for trial evidence")
        refusal = package_refusal(package)
        native_host_record.require(refusal is None, "the package's own record qualifies no "
            f"run: {refusal}; run a case singly with renewal_follow.py for trial evidence")
    except RuntimeError as error:
        # Refuse before anything is hashed, and say what to do: no case has run.
        message = str(error) if (host.parent / "build.json").exists() else (
            "native host build record missing; run native_host_record.py")
        manifest.write_text(json.dumps({"passed": False, "error": message, "cases": []},
                                       indent=2) + "\n")
        print(message, file=sys.stderr)
        return 1
    pinned = pinned_inputs(host, package)
    requested = args.case or CASES
    if len(set(requested)) != len(requested):
        parser.error("each case may be requested only once")
    report = {"passed": False, "scope": "actual paired native/Codex fixture cases",
              "quotaAccounts": args.quota,
              "requestedCases": list(requested), "allSixCasesPassed": False,
              "allAvailableCasesPassed": False,
              "coversLiveEmailAuthentication": False, "coversAllActivationRequirements": False,
              "inputs": {str(path): digest(path) for path in pinned}, "cases": []}

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
            passed = case_passed(case, completed.returncode, result)
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
