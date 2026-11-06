#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the real Cargo runner with distinct package/artifact/test copies."""

import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "unit_results.py"
RUNNER = Path(os.environ.get("ALAB_TEST_UNIT_RUNNER", SCRIPT.parents[1] / "codex-rs/target/debug/alab-unit-runner"))
TEST = "same::test_name"


class Tree:
    def __init__(self, base, name, targets=None):
        self.root = base / (name + " repo")
        self.target = base / (name + " output.noindex")
        self.target.mkdir()
        self.behavior = self.target / "behavior.json"
        self.set_behavior(a="FAILED", b="ok")
        self.packages, self.artifacts = [], []
        self.binaries = {}
        targets = targets or {"a": "custom_a", "b": "custom_b"}
        for name, target in targets.items():
            directory = self.root / "codex-rs" / name
            (directory / "src").mkdir(parents=True)
            manifest = directory / "Cargo.toml"
            manifest.write_text(f'[package]\nname = "package-{name}"\nversion = "0.0.0"\n')
            source = directory / "src/lib.rs"
            source.write_text("pub fn source() {}\n")
            package_id = "path+" + directory.as_uri() + f"#package-{name}@0.0.0"
            library = {"kind": ["lib"], "name": target, "src_path": str(source), "test": True}
            self.packages.append({"id": package_id, "name": "package-" + name, "version": "0.0.0",
                                  "manifest_path": str(manifest), "targets": [library]})
            binary = self.target / "debug/deps" / (target + "-" + hashlib.sha256(name.encode()).hexdigest()[:16])
            binary.parent.mkdir(parents=True, exist_ok=True)
            counter = self.target / ("calls-" + name)
            binary.write_text(f'''#!{sys.executable} -I
import json, os, signal, sys
from pathlib import Path
counter = Path({str(counter)!r})
counter.write_text(str(int(counter.read_text()) + 1) if counter.exists() else "1")
behavior = json.loads(Path({str(self.behavior)!r}).read_text())[{name!r}]
context = Path({str(self.target / ('context-' + name + '.json'))!r})
pending = context.with_name(context.name + "." + str(os.getpid()))
pending.write_text(json.dumps({{
    "pid": os.getpid(), "parent_pid": os.getppid(),
    "process_group": os.getpgrp(), "session": os.getsid(0)}}))
os.replace(pending, context)
print("fixture executed", flush=True)
tests = behavior if isinstance(behavior, dict) else {{{TEST!r}: behavior}}
if "--exact" in sys.argv:
    selected = sys.argv[sys.argv.index("--exact") + 1]
    tests = {{test: value for test, value in tests.items() if test == selected}}
failed, passed, status, crash_after_report = 0, 0, 0, False
print("running " + str(len(tests)) + " tests")
for test, outcome in tests.items():
    if outcome == "crash":
        os.kill(os.getpid(), signal.SIGKILL)
    if outcome == "hang":
        while True:
            signal.pause()
    if outcome == "missing":
        sys.exit(101)
    display = "FAILED" if outcome in ("false_failed", "report_then_crash") else outcome
    print("test " + test + " ... " + display)
    failed += display == "FAILED"
    passed += display == "ok"
    status = 101 if outcome == "FAILED" else status
    crash_after_report = crash_after_report or outcome == "report_then_crash"
print("test result: " + ("FAILED. " if failed else "ok. ") + str(passed) + " passed; " + str(failed) + " failed; 0 ignored")
if crash_after_report:
    sys.stdout.flush()
    os.kill(os.getpid(), signal.SIGKILL)
sys.exit(status)
''')
            binary.chmod(0o755)
            self.binaries[name] = binary
            self.artifacts.append({"reason": "compiler-artifact", "package_id": package_id,
                                   "manifest_path": str(manifest), "target": library,
                                   "profile": {"test": True}, "executable": str(binary), "fresh": True})

    def set_behavior(self, **values):
        self.behavior.write_text(json.dumps(values))

    def calls(self, name):
        path = self.target / ("calls-" + name)
        return int(path.read_text()) if path.exists() else 0

    def files(self, state):
        metadata = state / "metadata.json"
        metadata.write_text(json.dumps({"workspace_root": str(self.root / "codex-rs"),
                                       "workspace_members": [p["id"] for p in self.packages],
                                       "target_directory": str(self.target), "packages": self.packages}))
        artifacts = state / "artifacts.jsonl"
        artifacts.write_text("".join(json.dumps(a) + "\n" for a in self.artifacts))
        return metadata, artifacts


class UnitResultsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not RUNNER.is_file():
            raise RuntimeError("Build codex-alab-unit-runner first; set ALAB_TEST_UNIT_RUNNER to its actual artifact")

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="alab-unit-results-")
        self.base = Path(self.temporary.name).resolve()
        self.candidate = Tree(self.base, "candidate")

    def tearDown(self):
        self.temporary.cleanup()

    def command(self, *arguments, cwd=None, env=None):
        if arguments[0] == "run":
            native = list(map(str, arguments[1:]))
            if "--round" not in native:
                native[2:2] = ["--round", "0"]
            command = [str(RUNNER), *native]
        else:
            command = [sys.executable, "-I", str(SCRIPT), *map(str, arguments)]
        return subprocess.run(command,
                              cwd=cwd, env=env, capture_output=True, timeout=60)

    def prepare(self, tree, label, plan=None):
        state = self.base / label
        state.mkdir()
        metadata, artifacts = tree.files(state)
        arguments = ["prepare", "--state", state, "--repo", tree.root, "--target-dir", tree.target,
                     "--metadata", metadata, "--artifacts", artifacts]
        if plan:
            arguments += ["--plan", plan]
        for package in tree.packages:
            arguments += ["-p", package["name"]]
        process = self.command(*arguments)
        return state, process

    def execute(self, tree, state, names=("a", "b")):
        output, statuses = [], []
        for number in range(json.loads((state / "state.json").read_text())["rounds"]):
            output.append(f"ALAB_UNIT_ROUND {number}\n".encode())
            round_statuses = []
            for name in names:
                cwd = tree.root / "codex-rs" / name
                environment = dict(os.environ, CARGO_MANIFEST_DIR=str(cwd))
                process = self.command("run", "--state", state, "--round", number,
                                       tree.binaries[name], cwd=cwd, env=environment)
                output.append(process.stdout + process.stderr)
                if process.returncode:
                    output.append(f"error: test failed, to rerun pass `-p package-{name} --lib`\n".encode())
                if process.returncode < 0:
                    output.append(f"process didn't exit successfully: fixture (signal: {-process.returncode})\n".encode())
                round_statuses.append(process.returncode)
            status = 101 if any(round_statuses) else 0
            (state / f"round-{number}.status").write_text(str(status))
            statuses.append(status)
        log = state / "unit.log"
        log.write_bytes(b"".join(output))
        return log, 101 if any(statuses) else 0

    def collect(self, state, log, status):
        result = state / "results.json"
        process = self.command("collect", "--state", state, "--log", log,
                               "--status", status, "--out", result)
        return process, result

    def original(self, tree=None):
        tree = tree or self.candidate
        state, process = self.prepare(tree, "original")
        self.assertEqual(process.returncode, 0, process.stderr)
        log, status = self.execute(tree, state)
        process, plan = self.collect(state, log, status)
        self.assertEqual(process.returncode, 0, process.stderr)
        return plan

    def test_cursor_redraw_before_a_failed_result_keeps_its_artifact_identity(self):
        state, process = self.prepare(self.candidate, "redraw")
        self.assertEqual(process.returncode, 0, process.stderr)
        log, status = self.execute(self.candidate, state)
        text = log.read_text().replace(
            "test " + TEST + " ... FAILED",
            "\x1b[HNo matching tasks\x1b[0m\x1b[4;16H\x1b[?25h\x1b[?2026l"
            "test same::\x1b[1mtest_name\x1b[0m ... \x1b[31mFAILED\x1b[0m")
        log.write_text(text)
        process, result = self.collect(state, log, status)
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual(
            [(copy["package"], copy["test"]) for copy in json.loads(result.read_text())["failed"]],
            [("package-a", TEST)])

    def test_cursor_redraw_preserves_the_exact_comparison_result(self):
        plan = self.original()
        self.candidate.set_behavior(a="ok", b="ok")
        state, process = self.prepare(self.candidate, "redraw-comparison", plan)
        self.assertEqual(process.returncode, 0, process.stderr)
        log, status = self.execute(self.candidate, state)
        log.write_text(log.read_text().replace(
            "test " + TEST + " ... ok",
            "drawn text\x1b[Htest same::\x1b[1mtest_name\x1b[0m ... \x1b[32mok\x1b[0m"))
        process, result = self.collect(state, log, status)
        self.assertEqual(process.returncode, 0, process.stderr)
        result = json.loads(result.read_text())
        self.assertEqual(result["failed"], [])
        self.assertEqual([(copy["package"], copy["test"]) for copy in result["passed"]],
                         [("package-a", TEST)])

    def test_cursor_redraw_cannot_hide_a_duplicate_failure(self):
        state, process = self.prepare(self.candidate, "redraw-duplicate")
        self.assertEqual(process.returncode, 0, process.stderr)
        log, status = self.execute(self.candidate, state)
        record = "test " + TEST + " ... FAILED"
        log.write_text(log.read_text().replace(record, "drawn text\x1b[H" + record + "\n" + record))
        process, result = self.collect(state, log, status)
        self.assertNotEqual(process.returncode, 0)
        self.assertFalse(result.exists())

    def test_unrelated_same_name_pass_does_not_mask_the_failed_copy(self):
        plan = self.original()
        failures = json.loads(plan.read_text())["failed"]
        self.assertEqual([(copy["package"], copy["test"]) for copy in failures], [("package-a", TEST)])
        state, process = self.prepare(self.candidate, "isolated", plan)
        self.assertEqual(process.returncode, 0, process.stderr)
        log, status = self.execute(self.candidate, state)
        process, result = self.collect(state, log, status)
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual([copy["package"] for copy in json.loads(result.read_text())["failed"]], ["package-a"])
        self.assertEqual(self.candidate.calls("a"), 2)
        self.assertEqual(self.candidate.calls("b"), 1, "unrelated harness must not execute in comparison")

    def test_two_packages_with_the_same_library_basename_remain_distinct(self):
        tree = Tree(self.base, "shared", {"a": "same_library", "b": "same_library"})
        tree.set_behavior(a="FAILED", b="FAILED")
        plan = self.original(tree)
        failures = json.loads(plan.read_text())["failed"]
        self.assertEqual({copy["package"] for copy in failures}, {"package-a", "package-b"})
        self.assertEqual(len({copy["origin"]["executable"] for copy in failures}), 2)
        tree.set_behavior(a="ok", b="FAILED")
        state, process = self.prepare(tree, "shared-isolated", plan)
        self.assertEqual(process.returncode, 0, process.stderr)
        log, status = self.execute(tree, state)
        process, result = self.collect(state, log, status)
        self.assertEqual(process.returncode, 0, process.stderr)
        result = json.loads(result.read_text())
        self.assertEqual([copy["package"] for copy in result["passed"]], ["package-a"])
        self.assertEqual([copy["package"] for copy in result["failed"]], ["package-b"])

    def test_multiple_cases_in_one_library_use_faithful_cargo_rounds(self):
        self.candidate.set_behavior(a={TEST: "FAILED", "another::test_name": "FAILED"}, b="ok")
        plan = self.original()
        state, process = self.prepare(self.candidate, "multiple-rounds", plan)
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual(json.loads((state / "state.json").read_text())["rounds"], 2)
        log, status = self.execute(self.candidate, state)
        process, result = self.collect(state, log, status)
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual({copy["test"] for copy in json.loads(result.read_text())["failed"]},
                         {TEST, "another::test_name"})
        self.assertEqual(self.candidate.calls("a"), 3)
        self.assertEqual(self.candidate.calls("b"), 1)

    def test_zero_matching_tests_cannot_be_a_pass_for_an_original_failure(self):
        plan = self.original()
        self.candidate.set_behavior(a={"different::test": "ok"}, b="ok")
        state, process = self.prepare(self.candidate, "zero-match", plan)
        self.assertEqual(process.returncode, 0, process.stderr)
        log, status = self.execute(self.candidate, state)
        process, result = self.collect(state, log, status)
        self.assertNotEqual(process.returncode, 0)
        self.assertFalse(result.exists())

    def test_missing_or_crashed_expected_copy_cannot_use_another_packages_failure(self):
        plan = self.original()
        for outcome in ("missing", "crash"):
            with self.subTest(outcome=outcome):
                upstream = Tree(self.base, "upstream-" + outcome)
                upstream.set_behavior(a=outcome, b="FAILED")
                state, process = self.prepare(upstream, "compare-" + outcome, plan)
                self.assertEqual(process.returncode, 0, process.stderr)
                log, status = self.execute(upstream, state)
                process, result = self.collect(state, log, status)
                self.assertNotEqual(process.returncode, 0)
                self.assertFalse(result.exists())
                self.assertEqual(upstream.calls("b"), 0)

    def test_backend_success_cannot_override_an_explicit_expected_failure(self):
        plan = self.original()
        state, process = self.prepare(self.candidate, "backend-disagrees", plan)
        self.assertEqual(process.returncode, 0, process.stderr)
        log, _status = self.execute(self.candidate, state)
        process, result = self.collect(state, log, 0)
        self.assertNotEqual(process.returncode, 0)
        self.assertFalse(result.exists())

    def test_a_passing_library_cannot_borrow_another_librarys_exit_status(self):
        self.candidate.set_behavior(a="FAILED", b="FAILED")
        plan = self.original()
        self.candidate.set_behavior(a="false_failed", b="FAILED")
        state, process = self.prepare(self.candidate, "false-outcome", plan)
        self.assertEqual(process.returncode, 0, process.stderr)
        log, status = self.execute(self.candidate, state)
        process, result = self.collect(state, log, status)
        self.assertNotEqual(process.returncode, 0)
        self.assertFalse(result.exists())

    def test_failure_summary_followed_by_actual_signal_cannot_be_attributed(self):
        self.candidate.set_behavior(a="report_then_crash", b="ok")
        state, process = self.prepare(self.candidate, "signalled")
        self.assertEqual(process.returncode, 0, process.stderr)
        log, status = self.execute(self.candidate, state)
        process, result = self.collect(state, log, status)
        self.assertEqual(process.returncode, 137, process.stderr)
        self.assertFalse(result.exists())

    @unittest.skipUnless(os.name == "posix", "process group/session controls require POSIX")
    def test_record_and_compare_exec_preserve_the_actual_process_context(self):
        plan = self.original()
        original = json.loads((self.candidate.target / "context-a.json").read_text())
        state, process = self.prepare(self.candidate, "process-context", plan)
        self.assertEqual(process.returncode, 0, process.stderr)
        log, status = self.execute(self.candidate, state)
        process, _result = self.collect(state, log, status)
        self.assertEqual(process.returncode, 0, process.stderr)
        context = json.loads((self.candidate.target / "context-a.json").read_text())
        records = [json.loads(path.read_text()) for path in state.glob("library-*.json")]
        receipt = next(record for record in records if record["copy_key"] is not None)
        self.assertEqual(context["pid"], receipt["pid"], "exec must keep the Cargo child PID")
        for field in ("parent_pid", "process_group", "session"):
            self.assertEqual(context[field], receipt[field])
            self.assertEqual(context[field], original[field])

    @unittest.skipUnless(os.name == "posix", "runner SIGKILL control requires POSIX")
    def test_killing_the_runner_reaps_the_actual_harness_without_an_escaped_child(self):
        plan = self.original()
        self.candidate.set_behavior(a="hang", b="ok")
        state, process = self.prepare(self.candidate, "runner-killed", plan)
        self.assertEqual(process.returncode, 0, process.stderr)
        cwd = self.candidate.root / "codex-rs/a"
        environment = dict(os.environ, CARGO_MANIFEST_DIR=str(cwd))
        with (state / "execution-proof.log").open("wb") as stream:
            child = subprocess.Popen([str(RUNNER), "--state", str(state), "--round", "0",
                                      str(self.candidate.binaries["a"])], cwd=cwd, env=environment,
                                     stdout=stream, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                deadline = time.monotonic() + 60
                while True:
                    context = json.loads((self.candidate.target / "context-a.json").read_text())
                    if context["pid"] == child.pid and b"fixture executed" in (state / "execution-proof.log").read_bytes():
                        break
                    self.assertLess(time.monotonic(), deadline, "fixture execution proof never arrived")
                    self.assertIsNone(child.poll())
                    time.sleep(0.01)
                self.assertEqual(context["process_group"], child.pid)
                self.assertEqual(context["session"], child.pid)
                child.kill()
                self.assertEqual(child.wait(timeout=60), -signal.SIGKILL)
                with self.assertRaises(ProcessLookupError):
                    os.kill(child.pid, 0)
                with self.assertRaises(ProcessLookupError):
                    os.killpg(child.pid, 0)
            finally:
                if child.poll() is None:
                    os.killpg(child.pid, signal.SIGKILL)
                child.wait()

    def test_missing_runner_receipts_are_not_a_clean_run(self):
        state, process = self.prepare(self.candidate, "uninvoked")
        self.assertEqual(process.returncode, 0, process.stderr)
        log = state / "unit.log"
        log.write_text("test same::test_name ... ok\n")
        process, result = self.collect(state, log, 0)
        self.assertNotEqual(process.returncode, 0)
        self.assertFalse(result.exists())

    def test_state_reader_refuses_non_regular_symlink_and_oversized_files_before_exec(self):
        for kind in ("directory", "symlink", "fifo", "oversized"):
            with self.subTest(kind=kind):
                state, prepared = self.prepare(self.candidate, "state-reader-" + kind)
                self.assertEqual(prepared.returncode, 0, prepared.stderr)
                path = state / "state.json"
                contents = path.read_bytes()
                path.unlink()
                if kind == "directory":
                    path.mkdir()
                elif kind == "symlink":
                    target = state / "outside-state.json"
                    target.write_bytes(contents)
                    path.symlink_to(target)
                elif kind == "fifo":
                    os.mkfifo(path, 0o600)
                else:
                    with path.open("wb") as file:
                        file.truncate(64 * 1024 * 1024 + 1)
                cwd = self.candidate.root / "codex-rs/a"
                process = self.command("run", "--state", state, self.candidate.binaries["a"], cwd=cwd,
                                       env=dict(os.environ, CARGO_MANIFEST_DIR=str(cwd)))
                self.assertNotEqual(process.returncode, 0)
                self.assertEqual(self.candidate.calls("a"), 0, "the actual artifact must never execute")
                if kind == "oversized":
                    self.assertIn(b"unit-test state exceeds 64 MiB", process.stderr)

    def test_runtime_package_directory_and_replaced_artifacts_refuse_before_exec(self):
        for change in ("cwd", "artifact"):
            with self.subTest(change=change):
                state, process = self.prepare(self.candidate, "refused-" + change)
                self.assertEqual(process.returncode, 0, process.stderr)
                cwd = self.candidate.root / "codex-rs/a"
                if change == "cwd":
                    cwd = self.candidate.root / "codex-rs/b"
                else:
                    binary = self.candidate.binaries["a"]
                    binary.write_text(binary.read_text() + "\n# replaced\n")
                process = self.command("run", "--state", state, self.candidate.binaries["a"], cwd=cwd,
                                       env=dict(os.environ, CARGO_MANIFEST_DIR=str(cwd)))
                self.assertNotEqual(process.returncode, 0)
                self.assertEqual(self.candidate.calls("a"), 0)

    def test_metadata_artifact_and_original_source_guards(self):
        plan = self.original()
        for change in ("workspace", "source", "artifact-package", "target", "version"):
            with self.subTest(change=change):
                tree = Tree(self.base, "guard-" + change)
                if change == "source":
                    tree.packages[0]["targets"][0]["src_path"] = str(self.base / "outside.rs")
                elif change == "artifact-package":
                    tree.artifacts[0]["package_id"] = tree.packages[1]["id"]
                elif change == "target":
                    tree.packages[0]["targets"][0]["name"] = "another_target"
                elif change == "version":
                    tree.packages[0]["version"] = "1.0.0"
                state, process = self.prepare(tree, "guard-state-" + change, plan)
                if change == "workspace":
                    # Exercise the guard on actual prepared input, not a string assertion.
                    self.assertEqual(process.returncode, 0, process.stderr)
                    (state / "state.json").unlink()
                    metadata = json.loads((state / "metadata.json").read_text())
                    metadata["workspace_root"] = str(self.base)
                    (state / "metadata.json").write_text(json.dumps(metadata))
                    process = self.command("prepare", "--state", state, "--repo", tree.root,
                                           "--target-dir", tree.target, "--metadata", state / "metadata.json",
                                           "--artifacts", state / "artifacts.jsonl", "--plan", plan,
                                           "-p", "package-a", "-p", "package-b")
                self.assertNotEqual(process.returncode, 0, change)
                self.assertEqual(tree.calls("a"), 0)
                self.assertEqual(tree.calls("b"), 0)


if __name__ == "__main__":
    unittest.main()
