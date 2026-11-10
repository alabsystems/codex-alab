#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Use real Cargo and a native entry probe before either language runtime starts."""

import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import tomllib
import unittest

SPEC = importlib.util.spec_from_file_location(
    "native_runner_fixtures", Path(__file__).with_name("test_unit_results.py"))
FIXTURES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURES)
RUNNER, SCRIPT = FIXTURES.RUNNER, FIXTURES.SCRIPT


@unittest.skipUnless(os.name == "posix", "native Cargo exec context requires POSIX")
class NativeRunnerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not RUNNER.is_file():
            raise RuntimeError("Build native runner and set ALAB_TEST_UNIT_RUNNER first")
        cls.temporary = tempfile.TemporaryDirectory(prefix="alab-native-context-")
        cls.root = Path(cls.temporary.name).resolve()
        workspace = cls.root / "codex-rs"
        cls.package = workspace / "native-context"
        (cls.package / "src").mkdir(parents=True)
        (workspace / "Cargo.toml").write_text('[workspace]\nmembers = ["native-context"]\nresolver = "2"\n')
        locked = tomllib.loads((SCRIPT.parent.parent / "codex-rs/Cargo.lock").read_text())["package"]
        versions = {}
        for name in ("libc", "serde_json"):
            found = [entry["version"] for entry in locked if entry["name"] == name]
            if len(found) != 1:
                raise RuntimeError("native probe requires one locked version of " + name)
            versions[name] = found[0]
        (cls.package / "Cargo.toml").write_text(
            '[package]\nname = "alab-native-context"\nversion = "0.0.0"\nedition = "2024"\n'
            '[lib]\nharness = false\n[dependencies]\n' +
            ''.join(f'{name} = "={version}"\n' for name, version in versions.items()))
        (cls.package / "src/lib.rs").write_text((Path(__file__).parent / "fixtures/unit_context.rs").read_text())
        # The target survives independently of scratch source: no test removes a
        # newly built Mach-O if an execution proof fails or an observer times out.
        cls.target = Path(os.environ.get("ALAB_TEST_UNIT_CONTEXT_TARGET", RUNNER.parent / "unit-context.noindex"))
        cls.output = cls.root / "context.json"
        cls.prefix_output = cls.root / "prefix-context.json"
        cls.environment = dict(os.environ, CARGO_TARGET_DIR=str(cls.target), CARGO_BUILD_JOBS="3",
                               CARGO_INCREMENTAL="0", CARGO_PROFILE_DEV_DEBUG="0",
                               ALAB_CONTEXT_OUT=str(cls.output), ALAB_CONTEXT_PREFIX_OUT=str(cls.prefix_output),
                               ALAB_CONTEXT_FD="43",
                               ALAB_CONTEXT_FILE=str(cls.root / "inherited-file"),
                               ALAB_CONTEXT_VALUE="inherited ☃ value", LC_ALL="C",
                               CARGO_BIN_EXE_context_helper="/unchanged helper path")
        if os.environ.get("ALAB_CARGO_HOME"):
            cls.environment.update(CARGO_HOME=os.environ["ALAB_CARGO_HOME"], TARGO_HOME=os.environ["ALAB_CARGO_HOME"])
        # Invoke native admission directly. The native exec prefix installs the
        # controlled input after Cargo/frontends, at the actual runner boundary.
        command = os.environ.get("ALAB_NATIVE_CARGO_COMMAND")
        cls.cargo = (json.loads(command) if command else
                     [os.environ.get("ALAB_NATIVE_CARGO", "targo")])
        if (not isinstance(cls.cargo, list) or not cls.cargo or
                any(not isinstance(word, str) or not word for word in cls.cargo)):
            raise RuntimeError("ALAB_NATIVE_CARGO_COMMAND must be a nonempty JSON array of strings")
        cls.invocations = 0
        cls.evidence = Path(os.environ["ALAB_UNIT_NATIVE_EVIDENCE"]) if os.environ.get("ALAB_UNIT_NATIVE_EVIDENCE") else None
        if cls.evidence:
            cls.evidence.mkdir(parents=True, exist_ok=True)
        (cls.root / "inherited-file").write_bytes(b"inherited descriptor contents")
        cls.metadata = cls.run_cargo("metadata", "--offline", "--format-version", "1")
        if cls.metadata.returncode:
            raise RuntimeError(cls.metadata.stderr.decode(errors="replace"))
        cls.artifacts = cls.run_cargo("--unverified", "test", "--offline", "--locked", "--no-run", "--lib",
                                      "-p", "alab-native-context", "--message-format=json-render-diagnostics")
        if cls.artifacts.returncode:
            raise RuntimeError(cls.artifacts.stderr.decode(errors="replace"))
        found = [entry["executable"] for line in cls.artifacts.stdout.splitlines()
                 if (entry := json.loads(line)).get("reason") == "compiler-artifact" and entry.get("executable")]
        if len(found) != 1:
            raise RuntimeError("expected one native Cargo fixture artifact")
        cls.probe = found[0]

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    @staticmethod
    def inherited_signals():
        signal.signal(signal.SIGHUP, signal.SIG_IGN)
        signal.signal(signal.SIGUSR2, signal.SIG_IGN)
        if hasattr(signal, "SIGXFSZ"):
            signal.signal(signal.SIGXFSZ, signal.SIG_DFL)
        signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGUSR1, signal.SIGUSR2})

    @classmethod
    def run_cargo(cls, *arguments):
        result = subprocess.run([*cls.cargo, *arguments], cwd=cls.root / "codex-rs", env=cls.environment,
                                capture_output=True, preexec_fn=cls.inherited_signals, timeout=60)
        if cls.evidence:
            cls.invocations += 1
            label = f"cargo-{cls.invocations:02}"
            (cls.evidence / (label + ".json")).write_text(json.dumps({"arguments": arguments,
                                                                       "status": result.returncode}))
            (cls.evidence / (label + ".stdout")).write_bytes(result.stdout)
            (cls.evidence / (label + ".stderr")).write_bytes(result.stderr)
        return result

    def prepare(self, name, plan=None):
        state = self.root / name
        state.mkdir()
        (state / "metadata.json").write_bytes(self.metadata.stdout)
        (state / "artifacts.jsonl").write_bytes(self.artifacts.stdout)
        command = [sys.executable, "-I", str(SCRIPT), "prepare", "--repo", str(self.root),
                   "--target-dir", str(self.target), "--state", str(state),
                   "--metadata", str(state / "metadata.json"), "--artifacts", str(state / "artifacts.jsonl"),
                   "-p", "alab-native-context"]
        if plan:
            command += ["--plan", str(plan)]
        result = subprocess.run(command, capture_output=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)
        return state

    def invoke(self, state=None, runner=None, extra=()):
        arguments = ["--unverified", "test", "--offline", "--locked", "--no-fail-fast", "--lib",
                     "-p", "alab-native-context"]
        if state:
            config = json.loads((state / "state.json").read_text())["runner_cfg"]
            runner = runner or [str(RUNNER), "--state", str(state), "--round", "0"]
        else:
            config, runner = "cfg(all())", []
        runner = [self.probe, "--inject-fd", *runner]
        arguments += ["--config", f"target.'{config}'.runner={json.dumps(runner)}"]
        result = self.run_cargo(*arguments, *extra)
        if state:
            (state / "round-0.status").write_text(str(result.returncode))
            (state / "unit.log").write_bytes(b"ALAB_UNIT_ROUND 0\n" + result.stdout + result.stderr)
        return result

    def collect(self, state):
        result = state / "results.json"
        process = subprocess.run([sys.executable, "-I", str(SCRIPT), "collect", "--state", str(state),
                                  "--log", str(state / "unit.log"), "--status",
                                  (state / "round-0.status").read_text(), "--out", str(result)],
                                 capture_output=True, timeout=60)
        return process, result

    def context(self):
        observed = json.loads(self.output.read_text())
        self.assertEqual(observed["environment"], json.loads(self.prefix_output.read_text()))
        return observed

    def retain(self, name, value):
        if self.evidence:
            (self.evidence / (name + ".json")).write_text(json.dumps(value, sort_keys=True))

    def test_native_entry_preserves_real_cargo_context_and_detects_python_startup(self):
        direct = self.invoke()
        self.assertEqual(direct.returncode, 101, direct.stderr)
        before = self.context()
        self.retain("direct-entry", before)
        state = self.prepare("record")
        recorded = self.invoke(state)
        self.assertEqual(recorded.returncode, 101, recorded.stderr)
        after = self.context()
        receipt = json.loads(next(state.glob("library-*.json")).read_text())
        self.retain("native-entry", after)
        self.retain("native-receipt", receipt)
        for field in ("pid", "parent_pid", "session", "process_group", "cwd"):
            self.assertEqual(after[field], receipt[field], field)
        for field in ("signals", "descriptors", "cwd", "arguments",
                      "inherited_fd_offset", "inherited_fd_bytes", "session", "process_group"):
            self.assertEqual(before[field], after[field], field)
        self.assertEqual(after["inherited_fd_offset"], 7)
        self.assertEqual(bytes(after["inherited_fd_bytes"]), b"inherited descriptor contents")
        self.assertNotIn(42, [entry[0] for entry in after["descriptors"]])
        signals = {row[0]: row for row in after["signals"]}
        self.assertEqual(signals[signal.SIGHUP][1], signal.SIG_IGN)
        self.assertEqual(signals[signal.SIGUSR1][3], 1)
        self.assertEqual(signals[signal.SIGUSR2][3], 1)
        collected, plan = self.collect(state)
        self.assertEqual(collected.returncode, 0, collected.stderr)
        comparison = self.prepare("compare", plan)
        compared = self.invoke(comparison)
        self.assertEqual(compared.returncode, 101, compared.stderr)
        compared_context = self.context()
        self.retain("compare-entry", compared_context)
        for field in ("signals", "descriptors", "cwd", "inherited_fd_offset", "inherited_fd_bytes"):
            self.assertEqual(before[field], compared_context[field], field)
        self.assertEqual(compared_context["arguments"][1:], ["--exact", "native::context"])
        collected, result = self.collect(comparison)
        self.assertEqual(collected.returncode, 0, collected.stderr)
        self.assertEqual(len(json.loads(result.read_text())["failed"]), 1)
        negative = self.prepare("python-negative")
        python_runner = [sys.executable, "-I", "-c", "import os,sys; os.execv(sys.argv[1], sys.argv[1:])"]
        unsafe = self.invoke(negative, runner=python_runner)
        self.assertEqual(unsafe.returncode, 101, unsafe.stderr)
        python_context = self.context()
        self.retain("python-negative-entry", python_context)
        self.assertNotEqual(before["signals"], python_context["signals"], "the probe must detect interpreter signal changes")

    def test_existing_matching_cfg_runner_is_preserved_and_conflicts(self):
        state = self.prepare("cfg-conflict")
        result = self.invoke(state, extra=("--config", 'target.\'cfg(all())\'.runner=["/usr/bin/true"]'))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"several matching instances", result.stderr)
        self.assertFalse(list(state.glob("library-*.json")))
        collected, _ = self.collect(state)
        self.assertNotEqual(collected.returncode, 0)

    def test_existing_triple_runner_wins_and_missing_receipts_refuse(self):
        version = self.run_cargo("--version", "--verbose")
        self.assertEqual(version.returncode, 0, version.stderr)
        target = next(line.split(": ", 1)[1] for line in version.stdout.decode().splitlines() if line.startswith("host: "))
        state = self.prepare("triple-wins")
        result = self.invoke(state, extra=("--config", f'target.{target}.runner=["/usr/bin/true"]'))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(list(state.glob("library-*.json")))
        collected, _ = self.collect(state)
        self.assertNotEqual(collected.returncode, 0)


if __name__ == "__main__":
    unittest.main()
