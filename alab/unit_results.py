#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Qualify unit-test results by Cargo's package, artifact and runtime context.

Comparison keeps the complete Cargo package/feature graph. Its runner executes only
the previously failed copies, after Cargo supplied their package cwd and environment.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys


MARKER = "ALAB_UNIT "
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
RESULT = re.compile(r"\btest (\S+) \.\.\. (ok|FAILED|ignored)\b")
SUMMARY = re.compile(r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored")
IDENTITY = ("package", "version", "manifest", "target", "source")


class Refusal(Exception):
    def __init__(self, message, status=1):
        super().__init__(message)
        self.status = status


def require(condition, message):
    if not condition:
        raise Refusal(message)


def load(path):
    return json.loads(Path(path).read_text())


def save(path, value, exclusive=False):
    with Path(path).open("x" if exclusive else "w") as stream:
        json.dump(value, stream, sort_keys=True)
        stream.write("\n")


def key(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def relative(path, root):
    try:
        return str(Path(path).resolve().relative_to(root))
    except ValueError as error:
        raise Refusal(f"unit-test source or artifact escapes its declared root: {path}") from error


def snapshot(path):
    value = Path(path).stat()
    require(stat.S_ISREG(value.st_mode), f"unit-test artifact is not a regular file: {path}")
    return [value.st_dev, value.st_ino, value.st_size, value.st_mtime_ns]


def identity(copy):
    return tuple(copy[field] for field in IDENTITY)


def prepare(args):
    root, target, state = Path(args.repo).resolve(), Path(args.target_dir).resolve(), Path(args.state)
    metadata = load(args.metadata)
    require(Path(metadata["workspace_root"]).resolve() == root / "codex-rs",
            "unit-test metadata belongs to another workspace")
    relative(metadata["target_directory"], target)
    members = set(metadata["workspace_members"])
    packages = [package for package in metadata["packages"] if package["id"] in members]
    artifacts = []
    for line in Path(args.artifacts).read_text().splitlines():
        try:
            value = json.loads(line)
        except ValueError:
            continue
        if (value.get("reason") == "compiler-artifact" and value.get("profile", {}).get("test")
                and "lib" in value.get("target", {}).get("kind", []) and value.get("executable")):
            artifacts.append(value)
    libraries = []
    for name in args.package:
        found = [package for package in packages if package["name"] == name]
        require(len(found) == 1, f"unit-test package selection is ambiguous or missing: {name}")
        package = found[0]
        manifest = str(Path(package["manifest_path"]).resolve())
        relative_manifest = relative(manifest, root)
        targets = [t for t in package["targets"] if "lib" in t["kind"]]
        require(len(targets) == 1, f"unit-test library selection is ambiguous or missing: {name}")
        library = targets[0]
        source = str(Path(library["src_path"]).resolve())
        relative_source = relative(source, root)
        found = [a for a in artifacts if a["package_id"] == package["id"]
                 and Path(a["manifest_path"]).resolve() == Path(manifest)
                 and a["target"]["name"] == library["name"]
                 and Path(a["target"]["src_path"]).resolve() == Path(source)]
        require(len(found) == 1, f"no unique compiled unit-test artifact for {name}/{library['name']}")
        binary = str(Path(found[0]["executable"]).resolve())
        relative(binary, target)
        entry = {"package": name, "version": package["version"], "manifest": relative_manifest,
                 "target": library["name"], "source": relative_source, "package_id": package["id"],
                 "cwd": str(Path(manifest).parent), "binary": binary, "snapshot": snapshot(binary)}
        entry["key"] = key([package["id"], library["name"], source])
        require(not any(previous["key"] == entry["key"] for previous in libraries),
                f"unit-test library selected twice: {name}")
        libraries.append(entry)
    require(libraries, "no selected unit-test libraries")
    copies = []
    rounds = {}
    if args.plan:
        planned = load(args.plan)["failed"]
        require(planned, "an isolated comparison needs an original failed copy")
        for original in planned:
            require(isinstance(original["test"], str) and original["test"]
                    and not original["test"].startswith("-")
                    and not any(c.isspace() for c in original["test"]), "invalid unit-test selector")
            found = [library for library in libraries if identity(library) == identity(original)]
            require(len(found) == 1, f"original unit-test source/package/target is absent: {original}")
            copy = dict(original, library_key=found[0]["key"])
            copy["key"] = key([copy["library_key"], copy["test"]])
            require(not any(previous["key"] == copy["key"] for previous in copies),
                    "the same unit-test copy was selected twice")
            copy["round"] = rounds.get(copy["library_key"], 0)
            rounds[copy["library_key"]] = copy["round"] + 1
            copies.append(copy)
    save(state / "state.json", {"repo": str(root), "target_dir": str(target),
                               "mode": "compare" if args.plan else "record",
                               "libraries": libraries, "copies": copies,
                               "rounds": max(rounds.values()) if rounds else 1,
                               "runner_cfg": 'cfg(any(all(), target_os = "alab-unit-'
                                             + os.urandom(16).hex() + '"))'}, exclusive=True)


def result_text(path):
    # A terminal redraw can end directly before libtest's next record. Cursor
    # controls separate those outputs; SGR styling inside a test name does not.
    return ANSI.sub(lambda code: "" if code.group().endswith("m") else " ",
                    Path(path).read_text(errors="replace"))


def collect(args):
    state_dir = Path(args.state)
    state = load(state_dir / "state.json")
    if args.status > 128:
        raise Refusal("unit-test process was interrupted", args.status)
    require(args.status in (0, 101), f"unit-test command did not report a test outcome: {args.status}")
    text = result_text(args.log)
    signal_failure = re.search(r"process didn't exit successfully: .*\(signal: (\d+)", text)
    if signal_failure:
        require(1 <= int(signal_failure.group(1)) <= 127, "invalid Cargo child signal")
        raise Refusal("a unit-test harness died by signal after reporting results",
                      128 + int(signal_failure.group(1)))
    require(not any(int(value) != 101 for value in re.findall(
        r"process didn't exit successfully: .*\(exit status: (\d+)\)", text)),
        "a unit-test harness exited without a normal test outcome")
    receipts = {}
    for round_number in range(state["rounds"]):
        for library in state["libraries"]:
            receipt = load(state_dir / ("library-" + library["key"] + "-" + str(round_number) + ".json"))
            require(receipt["library_key"] == library["key"] and receipt["binary"] == library["binary"]
                    and receipt["cwd"] == library["cwd"] and receipt["mode"] == state["mode"]
                    and receipt["round"] == round_number,
                    "unit-test runtime receipt does not match its prepared Cargo artifact")
            receipts[(library["key"], round_number)] = receipt
    executions = state["libraries"] if state["mode"] == "record" else state["copies"]
    observations = {entry["key"]: {"results": [], "summaries": [], "details": False} for entry in executions}
    execution_round = {entry["key"]: entry.get("round", 0) for entry in executions}
    cargo_failed = {number: set() for number in range(state["rounds"])}
    seen_rounds = set()
    current_round = None
    current = None
    for line in text.splitlines():
        round_marker = re.search(r"ALAB_UNIT_ROUND (\d+)\s*$", line)
        if round_marker:
            current_round = int(round_marker.group(1))
            require(current_round in cargo_failed and current_round not in seen_rounds,
                    "unexpected or repeated Cargo comparison round")
            seen_rounds.add(current_round)
            current = None
        cargo_failure = re.search(r"error: test failed, to rerun pass `-p (\S+) --lib`", line)
        if cargo_failure:
            require(current_round is not None, "Cargo reported a test failure outside its execution round")
            cargo_failed[current_round].add(cargo_failure.group(1))
        elif "error: test failed, to rerun pass" in line:
            raise Refusal("Cargo did not qualify the failing library package")
        marker = re.search(re.escape(MARKER) + r"([0-9a-f]{64})\s*$", line)
        if marker:
            current = marker.group(1)
            require(current in observations and execution_round[current] == current_round,
                    "unit-test output named an unknown Cargo artifact or round")
        if current is None:
            continue
        observed = observations[current]
        if line.strip() == "failures:":
            observed["details"] = True
        if not observed["details"]:
            observed["results"].extend(RESULT.findall(line))
        observed["summaries"].extend(SUMMARY.findall(line))
    failed, passed = [], []
    expected_failed = {number: set() for number in range(state["rounds"])}
    if state["mode"] == "record":
        for library in state["libraries"]:
            observed = observations[library["key"]]
            failures = [name for name, outcome in observed["results"] if outcome == "FAILED"]
            require(observed["summaries"], f"unit-test harness did not report results: {library['package']}")
            require(int(observed["summaries"][-1][1]) == len(failures),
                    "unit-test failure lines disagree with their summaries")
            require(len(set(failures)) == len(failures),
                    "one unit-test copy failed more than once without distinct provenance")
            for name in failures:
                copy = {field: library[field] for field in IDENTITY}
                copy.update(test=name, origin={"repo": state["repo"], "package_id": library["package_id"],
                                               "executable": library["binary"]})
                failed.append(copy)
                expected_failed[0].add(library["package"])
        require(len(failed) <= 60, "too many unit-test failures to attribute individually")
    else:
        libraries = {library["key"]: library for library in state["libraries"]}
        for copy in state["copies"]:
            library = libraries[copy["library_key"]]
            receipt = receipts[(copy["library_key"], copy["round"])]
            launch = receipt["launch_binary"]
            require(str(Path(launch).resolve()) == library["binary"]
                    and receipt["copy_key"] == copy["key"],
                    "isolated unit-test selector or package directory changed")
            observed = observations[copy["key"]]
            matches, summaries = observed["results"], observed["summaries"]
            if not matches and summaries and tuple(map(int, summaries[-1])) == (0, 0, 0):
                raise Refusal(
                    f"expected unit-test copy matched zero tests in {state['repo']}: "
                    f"{copy['package']}/{copy['target']}: {copy['test']}; "
                    "original failure remains gating")
            require(len(matches) == 1 and matches[0][0] == copy["test"] and matches[0][1] in ("ok", "FAILED")
                    and summaries and tuple(map(int, summaries[-1]))
                    == ((0, 1, 0) if matches[0][1] == "FAILED" else (1, 0, 0)),
                    f"expected unit-test copy has no unique explicit result: {copy['package']}/{copy['test']}")
            value = {field: copy[field] for field in (*IDENTITY, "test", "origin")}
            (failed if matches[0][1] == "FAILED" else passed).append(value)
            if matches[0][1] == "FAILED":
                expected_failed[copy["round"]].add(copy["package"])
    require(seen_rounds == set(range(state["rounds"])), "a Cargo execution round did not run")
    round_statuses = []
    for number in range(state["rounds"]):
        status = int((state_dir / ("round-" + str(number) + ".status")).read_text())
        if status > 128:
            raise Refusal("Cargo comparison round was interrupted", status)
        require(status in (0, 101) and (status == 101) == bool(expected_failed[number])
                and cargo_failed[number] == expected_failed[number],
                "Cargo's failing packages disagree with their qualified unit-test results")
        round_statuses.append(status)
    require((args.status == 101) == any(status == 101 for status in round_statuses),
            "Cargo's aggregate outcome disagrees with its execution rounds")
    require((args.status == 101) == bool(failed), "Cargo's outcome disagrees with the expected unit-test copies")
    save(args.out, {"failed": failed, "passed": passed, "selected_libraries": len(state["libraries"]),
                    "executed_copies": None if state["mode"] == "record" else len(state["copies"])})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    setup = commands.add_parser("prepare")
    for name in ("repo", "target-dir", "metadata", "artifacts", "state"):
        setup.add_argument("--" + name, required=True)
    setup.add_argument("--plan")
    setup.add_argument("-p", "--package", action="append", default=[])
    results = commands.add_parser("collect")
    for name in ("state", "log", "out"):
        results.add_argument("--" + name, required=True)
    results.add_argument("--status", required=True, type=int)
    args = parser.parse_args()
    try:
        if args.command == "prepare":
            prepare(args)
        else:
            collect(args)
    except (Refusal, OSError, ValueError, KeyError, TypeError) as error:
        print(f"alab/check.sh: unit-test attribution refused: {error}", file=sys.stderr)
        return error.status if isinstance(error, Refusal) else 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
