#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the newly built Codex's automatic reset policy using isolated loopback fixtures."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys


CASES = ("preflight-five-hour", "preflight-below-99", "preflight-weekly", "preflight-beyond-24h")
POSITIVE_CASES = frozenset(("preflight-five-hour", "preflight-weekly"))
BOOTSTRAP = ("import runpy,sys; from pathlib import Path; "
             "probe=Path(sys.argv.pop(1)); sys.path.insert(0,str(probe.parent)); "
             "runpy.run_path(str(probe),run_name='__main__')")


def sha256(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise ValueError(f"runtime input is not a regular file: {path}")
        digest = hashlib.sha256()
        while chunk := os.read(descriptor, 1024 * 1024):
            digest.update(chunk)
        after = os.fstat(descriptor)
        if (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (
                after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise ValueError(f"runtime input changed while hashing: {path}")
        return digest.hexdigest()
    finally:
        os.close(descriptor)


def git_head(repo):
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1", GIT_OPTIONAL_LOCKS="0",
               GIT_NO_REPLACE_OBJECTS="1", GIT_NO_LAZY_FETCH="1")
    return subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"],
                                   env=env, text=True).strip()


def retain_diagnostics(scratch, output):
    # Keep commands, native events, HTTP traces and guardian results. The fake
    # HOME/CODEX_HOME, databases and sockets stay in the short protected scratch.
    if not scratch.is_dir():
        return
    for directory in [scratch, *(scratch / name for name in CASES)]:
        if not directory.is_dir() or directory.is_symlink():
            continue
        destination = output / "smoke" / directory.relative_to(scratch)
        destination.mkdir(parents=True, exist_ok=True)
        for path in directory.iterdir():
            if stat.S_ISREG(path.lstat().st_mode):
                shutil.copyfile(path, destination / path.name, follow_symlinks=False)


def run(repo, executable, source, scratch, output):
    if not re.fullmatch(r"[0-9a-f]{40}", source) or git_head(repo) != source:
        raise ValueError("runtime smoke requires the exact checked-out source commit")
    if not all(path.is_absolute() for path in (executable, scratch, output)):
        raise ValueError("runtime executable, scratch and evidence paths must be absolute")
    if scratch.exists() or scratch.is_symlink():
        raise ValueError("runtime smoke requires fresh scratch")
    output.mkdir(mode=0o700)
    probe = repo / "alab/smoke/weekly_reset.py"
    inputs = [repo / "alab/check_runtime.py", probe, probe.with_name("provider_fixture.py"),
              repo / "alab/command_guard.py"]
    before = {str(path.relative_to(repo)): sha256(path) for path in inputs}
    binary = sha256(executable)
    command = [sys.executable, "-I", "-B", "-S", "-c", BOOTSTRAP, str(probe),
               "--codex", str(executable), "--output", str(scratch)]
    for name in CASES:
        command.extend(("--case", name))
    record = {"schema": 1, "kind": "built-codex-runtime-observation", "source": source,
              "binary_sha256": binary, "probe_inputs": before, "requested_cases": list(CASES),
              "command": command, "passed": False,
              "scope": "selected automatic-reset runtime cases with fake auth and loopback; no proof or full-suite claim"}
    status = 1
    try:
        with (output / "stdout.txt").open("xb") as stdout, (output / "stderr.txt").open("xb") as stderr:
            status = subprocess.call(command, cwd=repo, stdout=stdout, stderr=stderr)
        record["exit"] = status
        report_path = scratch / "result.json"
        report = json.loads(report_path.read_text())
        record["smoke_report_sha256"] = sha256(report_path)
        if status != 0 or report.get("passed") is not True:
            raise ValueError("automatic-reset runtime smoke failed; see retained stdout/stderr and report")
        if (report.get("requestedCases") != list(CASES) or report.get("fullSuite") is not False
                or report.get("binarySha256") != binary or report.get("probeSha256") != before["alab/smoke/weekly_reset.py"]):
            raise ValueError("runtime smoke report differs from its requested source/binary/coverage")
        cases = report.get("cases", [])
        if [case.get("case") for case in cases] != list(CASES):
            raise ValueError("runtime smoke omitted a required policy boundary")
        for case in cases:
            positive = case["case"] in POSITIVE_CASES
            if (case.get("passed") is not True or case.get("guardianTimedOut") is not False
                    or case.get("consumeRequests") != int(positive)
                    or case.get("modelRequests") != 2 or (case.get("exit") == 0) != positive):
                raise ValueError("runtime smoke did not observe the required reset/zero-POST behavior")
        if (sha256(executable) != binary or git_head(repo) != source
                or {str(path.relative_to(repo)): sha256(path) for path in inputs} != before):
            raise ValueError("runtime executable or source inputs changed during the smoke")
        record["passed"] = True
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        record["error"] = f"{type(error).__name__}: {error}"
        status = status or 1
    finally:
        retain_diagnostics(scratch, output)
        with (output / "result.json").open("x") as stream:
            json.dump(record, stream, indent=2)
            stream.write("\n")
    return status


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--source", required=True)
    parser.add_argument("--scratch", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        status = run(Path(__file__).resolve().parents[1], args.codex, args.source, args.scratch, args.output)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f"alab: runtime smoke refused: {error}\n")
    print(f"alab: automatic-reset runtime smoke {'passed' if status == 0 else 'failed'}; evidence: {args.output}")
    return 128 - status if status < 0 else status


if __name__ == "__main__":
    sys.exit(main())
