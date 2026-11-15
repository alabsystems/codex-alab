# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Retain native-fixture timeout output using only a mocked subprocess boundary."""
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "native_timeout_fixtures", Path(__file__).with_name("test_unit_runner_native.py"))
NATIVE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NATIVE)
FIXTURE_PARENT = None
FIXTURES = []


class NativeRunnerTimeoutEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="native-timeout-evidence-", dir=FIXTURE_PARENT))
        FIXTURES.append(str(self.root))
        if FIXTURE_PARENT is None:
            self.addCleanup(shutil.rmtree, self.root)
        evidence = self.root / "evidence"
        evidence.mkdir()
        self.fixture = SimpleNamespace(
            root=self.root,
            cargo=["/fixture/native-front", "__build_admit", "/fixture/targo"],
            environment={"FIXTURE_ONLY": "timeout evidence"},
            evidence=evidence,
            invocations=4,
            inherited_signals=mock.Mock(name="inherited_signals"),
        )
        self.arguments = ("--unverified", "test", "--offline", "--locked", "--no-run", "--lib")

    def invoke(self):
        return NATIVE.NativeRunnerTests.run_cargo.__func__(self.fixture, *self.arguments)

    def assert_subprocess_context(self, execute):
        execute.assert_called_once_with(
            [*self.fixture.cargo, *self.arguments], cwd=self.root / "codex-rs",
            env=self.fixture.environment, capture_output=True,
            preexec_fn=self.fixture.inherited_signals, timeout=60)
        self.fixture.inherited_signals.assert_not_called()

    def timeout(self, *, stdout=None, stderr=None):
        return subprocess.TimeoutExpired([*self.fixture.cargo, *self.arguments], 60,
                                         output=stdout, stderr=stderr)

    def assert_timeout_capture(self, original, stdout, stderr):
        with mock.patch.object(NATIVE.subprocess, "run", side_effect=original) as execute:
            with self.assertRaises(subprocess.TimeoutExpired) as caught:
                self.invoke()
        self.assertIs(caught.exception, original)
        self.assert_subprocess_context(execute)
        paths = {extension: self.fixture.evidence / ("cargo-05." + extension)
                 for extension in ("json", "stdout", "stderr")}
        for path in paths.values():
            self.assertTrue(path.is_file(), f"timeout lost {path.name}")
        self.assertEqual(self.fixture.invocations, 5)
        self.assertEqual(paths["stdout"].read_bytes(), stdout)
        self.assertEqual(paths["stderr"].read_bytes(), stderr)
        record = json.loads(paths["json"].read_text())
        self.assertEqual(record["arguments"], list(self.arguments))
        self.assertIsNone(record["status"])
        self.assertEqual(record["outcome"], "timeout")
        self.assertEqual(record["timeout_seconds"], 60)

    def test_timeout_retains_partial_bytes_and_reraises_the_identical_exception(self):
        stdout, stderr = b"partial stdout\x00\xff\n", b"partial stderr\xfe\n"
        self.assert_timeout_capture(self.timeout(stdout=stdout, stderr=stderr), stdout, stderr)

    def test_timeout_without_partial_output_retains_empty_files(self):
        self.assert_timeout_capture(self.timeout(), b"", b"")

    def test_evidence_write_failure_preserves_timeout_and_explains_capture_failure(self):
        original = self.timeout(stdout=b"partial stdout", stderr=b"partial stderr")
        denied = PermissionError("fixture evidence denied")
        with mock.patch.object(NATIVE.subprocess, "run", side_effect=original) as execute, \
                mock.patch.object(Path, "write_text", side_effect=denied):
            with self.assertRaises(subprocess.TimeoutExpired) as caught:
                self.invoke()
        self.assertIs(caught.exception, original)
        self.assert_subprocess_context(execute)
        notes = "\n".join(getattr(original, "__notes__", []))
        self.assertIn("PermissionError", notes)
        self.assertIn("fixture evidence denied", notes)

    def test_completed_process_capture_shape_and_return_identity_stay_unchanged(self):
        original = subprocess.CompletedProcess([*self.fixture.cargo, *self.arguments], 0,
                                               stdout=b"complete stdout", stderr=b"complete stderr")
        with mock.patch.object(NATIVE.subprocess, "run", return_value=original) as execute:
            self.assertIs(self.invoke(), original)
        self.assert_subprocess_context(execute)
        self.assertEqual(self.fixture.invocations, 5)
        self.assertEqual(json.loads((self.fixture.evidence / "cargo-05.json").read_text()),
                         {"arguments": list(self.arguments), "status": 0})
        self.assertEqual((self.fixture.evidence / "cargo-05.stdout").read_bytes(), original.stdout)
        self.assertEqual((self.fixture.evidence / "cargo-05.stderr").read_bytes(), original.stderr)


if __name__ == "__main__":
    unittest.main()
