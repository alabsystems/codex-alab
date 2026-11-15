#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check a real failed native child's diagnostics after protected cleanup.

The opt-in integration case starts the source-selected protected probe itself.
It must run outside containment; it never copies paths supplied by child stdout.
"""

import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import unittest


ROOT = Path(__file__).resolve().parents[2]
PREFIX = "extension::upgrade_handoff_tests::native_failure_diagnostics_tests::"
FIXTURES = {"shutdown": "shutdown_returned_failure_fixture",
            "incomplete": "incomplete_failure_fixture"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read_regular(path, limit=1024 * 1024):
    """Read bounded evidence without traversing links or opening special files."""
    path = Path(path)
    require(path.is_absolute() and ".." not in path.parts, "noncanonical evidence path")
    descriptor = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        for part in path.parts[1:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                            dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
        child = os.open(path.name, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC,
                        dir_fd=descriptor)
        try:
            value = os.fstat(child)
            require(stat.S_ISREG(value.st_mode) and value.st_size <= limit,
                    f"evidence is not a bounded regular file: {path}")
            with os.fdopen(child, "rb", closefd=False) as stream:
                data = stream.read(limit + 1)
            require(len(data) <= limit, f"evidence grew beyond its limit: {path}")
            return data
        finally:
            os.close(child)
    finally:
        os.close(descriptor)


def load(path):
    return json.loads(read_regular(path))


def verify_case(state, phase):
    """Verify diagnostics, keeping the intentional failure classified as failure."""
    state = Path(state)
    require(phase in FIXTURES, "unknown native diagnostic phase")
    expected_test = PREFIX + FIXTURES[phase]
    require(read_regular(state / "contained.status") == b"101\n",
            "the protected command did not retain the failed status")
    require(read_regular(state / (phase + ".status")) == b"101\n"
            and read_regular(state / phase / "round-0.status") == b"101\n",
            "the actual Cargo round was not a failed native test")
    collected = load(state / (phase + ".collected.json"))
    require(collected["selected_libraries"] == 1 and collected["passed"] == []
            and len(collected["failed"]) == 1
            and collected["failed"][0]["package"] == "codex-alab-autopilot"
            and collected["failed"][0]["test"] == expected_test,
            "the qualified result is not exactly the intended failed native copy")
    log = read_regular(state / (phase + ".log"), 64 * 1024 * 1024).decode(errors="replace")
    require(f"ALAB_NATIVE_DIAGNOSTICS_EXPECTED_FAILURE:{phase}" in log
            and re.search(r"test result: FAILED\. 0 passed; 1 failed; 0 ignored;", log),
            "the actual fixture did not reach its intended native failure")
    diagnostics = collected["diagnostics"]
    require(len(diagnostics) == 1 and diagnostics[0]["round"] == 0,
            "expected one qualified diagnostics leaf")
    leaf = Path(diagnostics[0]["directory"])
    prepared = load(state / phase / "state.json")
    require(len(prepared["libraries"]) == 1 and prepared["mode"] == "record"
            and prepared["rounds"] == 1
            and leaf == Path(prepared["libraries"][0]["diagnostics_dirs"][0])
            and leaf.parent == Path(prepared["diagnostics_root"])
            and leaf.parent.parent == Path(prepared["target_dir"]),
            "diagnostics escaped their prepared native target leaf")
    manifests = diagnostics[0]["manifests"]
    require(len(manifests) == 1, "expected one actual fixture manifest")
    manifest_path = Path(manifests[0])
    require(manifest_path.name == "manifest.json" and manifest_path.parent.parent == leaf
            and re.fullmatch(r"fixture-[0-9]+-[0-9a-f]{16}", manifest_path.parent.name),
            "manifest is outside the exclusive fixture export")
    manifest = load(manifest_path)
    producer = "shutdown_returned" if phase == "shutdown" else "incomplete"
    require(manifest["version"] == 1 and manifest["producer_state"] == producer
            and manifest["declared_producer_state"] == producer,
            "the exporter misreported the observed native cleanup boundary")
    witness_entries = [entry for entry in manifest["entries"] if entry["root"] == "home"
                       and entry["path_hex"] == b"diagnostic-witness.json".hex()]
    require(len(witness_entries) == 1 and witness_entries[0]["result"] == "copied"
            and not witness_entries[0]["truncated"], "the known native witness was not copied")
    witness_path = manifest_path.parent / "home/diagnostic-witness.json"
    witness_data = read_regular(witness_path, 16 * 1024)
    require(witness_entries[0]["bytes"] == len(witness_data), "witness byte count changed")
    witness = json.loads(witness_data)
    scratch = read_regular(state / "contained-scratch", 4096).decode().removesuffix("\n")
    scratch_path = Path(scratch)
    require(witness["phase"] == phase and witness["scratch"] == scratch
            and scratch_path.parent == Path("/tmp").resolve()
            and re.fullmatch(r"alab\.[A-Za-z0-9]{6}", scratch_path.name),
            "the witness does not name this allocated protected scratch")
    require(not os.path.lexists(scratch), "normal protected cleanup left the source scratch")
    outside = [entry for entry in manifest["entries"] if entry["root"] == "home"
               and entry["path_hex"] == b"outside-link".hex()]
    require(len(outside) == 1 and outside[0]["result"] == "omitted"
            and outside[0]["reason"] == "not_regular_file_or_directory"
            and not os.path.lexists(manifest_path.parent / "home/outside-link"),
            "the exporter followed the unregistered outside-root symlink")
    require(not any(entry["result"] == "copied"
                    and bytes.fromhex(entry["path_hex"]).split(b"/")[-1] == b"must-not-copy"
                    for entry in manifest["entries"]), "an outside-root control was copied")


@unittest.skipUnless(os.environ.get("ALAB_TEST_NATIVE_FAILURE_DIAGNOSTICS") == "1",
                     "opt in to the protected native failing-child probe")
class NativeFailureDiagnosticsTests(unittest.TestCase):
    def test_failed_children_survive_normal_protected_scratch_cleanup(self):
        environment = dict(os.environ, ALAB_PYTHON=sys.executable)
        result = subprocess.run(["/bin/bash", str(ROOT / "alab/native_diagnostics_probe.sh")],
                                cwd=ROOT, env=environment, check=False)
        self.assertEqual(result.returncode, 0)


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "--verify":
        try:
            verify_case(Path(sys.argv[2]), sys.argv[3])
        except (OSError, ValueError, KeyError, TypeError) as error:
            raise SystemExit(f"native diagnostic probe refused: {error}") from error
    else:
        unittest.main()
