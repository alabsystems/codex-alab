# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Fake Cargo publishes native bytes atomically and records an adversarial shell status."""
from contextlib import contextmanager
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import unittest


SPEC = importlib.util.spec_from_file_location(
    "check_lint_publication_fixtures", Path(__file__).with_name("test_check_lint_step.py"))
FIXTURES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURES)


class CheckLintRunnerPublicationTests(unittest.TestCase):
    @contextmanager
    def fixture(self):
        if getattr(FIXTURES.CheckLintStepTests, "__unittest_skip__", False):
            self.skipTest(FIXTURES.CheckLintStepTests.__unittest_skip_why__)
        FIXTURES.CheckLintStepTests.setUpClass()
        fixture = FIXTURES.CheckLintStepTests("runTest")
        fixture.setUp()
        try:
            yield fixture
        finally:
            fixture.tearDown()

    def image(self, path):
        metadata = path.stat()
        return {"device": metadata.st_dev, "inode": metadata.st_ino,
                "mode": stat.S_IMODE(metadata.st_mode), "size": metadata.st_size,
                "mtime_ns": metadata.st_mtime_ns, "ctime_ns": metadata.st_ctime_ns,
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}

    def seed_image(self, fixture):
        target = fixture.fake / "publication-target"
        output = target / "native runner output/alab-unit-runner"
        output.parent.mkdir(parents=True)
        source = FIXTURES.RUNNER.read_bytes()
        shutil.copy2(FIXTURES.RUNNER, output)
        output.chmod(0o755)
        retained = output.with_name("retained-old-image")
        os.link(output, retained)
        return target, output, retained, source

    def build(self, fixture, target, *, source=None):
        environment = dict(fixture.environment, CARGO_TARGET_DIR=str(target))
        if source is not None:
            environment["ALAB_FAKE_UNIT_RUNNER"] = str(source)
        command = [environment["ALAB_CARGO"], "--unverified", "build", "--locked", "--manifest-path",
                   environment["ALAB_FAKE_LIBRARY_MANIFEST"], "-p", "codex-alab-unit-runner",
                   "--bin", "alab-unit-runner", "--message-format=json-render-diagnostics"]
        return subprocess.run(command, cwd=fixture.repo / "codex-rs", env=environment,
                              capture_output=True, text=True, timeout=60)

    def test_publishing_replaces_the_inode_and_preserves_the_old_linked_image(self):
        with self.fixture() as fixture:
            target, output, retained, source = self.seed_image(fixture)
            old = self.image(retained)
            result = self.build(fixture, target)
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            # Replacing the other hardlink can change ctime as its link count falls.
            self.assertEqual({key: value for key, value in self.image(retained).items() if key != "ctime_ns"},
                             {key: value for key, value in old.items() if key != "ctime_ns"})
            self.assertEqual(retained.read_bytes(), source)
            published = self.image(output)
            self.assertEqual(published["device"], old["device"])
            self.assertNotEqual(published["inode"], old["inode"], "publication rewrote the existing native image")
            self.assertEqual(published["sha256"], hashlib.sha256(source).hexdigest())
            self.assertEqual(published["mode"], 0o755)
            self.assertEqual(json.loads(result.stdout)["executable"], str(output))

    def test_failed_copy_keeps_both_old_names_and_leaks_no_staging_file(self):
        with self.fixture() as fixture:
            target, output, retained, _source = self.seed_image(fixture)
            old_output, old_retained = self.image(output), self.image(retained)
            names = sorted(path.name for path in output.parent.iterdir())
            result = self.build(fixture, target, source=fixture.fake / "missing-unit-runner")
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(self.image(output), old_output)
            self.assertEqual(self.image(retained), old_retained)
            self.assertEqual(sorted(path.name for path in output.parent.iterdir()), names)

    def test_raw_shell_child_status_is_recorded_before_fake_cargo_aggregates_it(self):
        for returncode, script in ((23, "#!/bin/sh\nexit 23\n"),
                                   (-9, "#!/bin/sh\nkill -KILL $$\n")):
            # The signal control targets only this synthetic shell's own PID.
            with self.subTest(returncode=returncode), self.fixture() as fixture:
                stub = fixture.fake / "adversarial-runner"
                stub.write_text(script)
                stub.chmod(0o755)
                fixture.environment.update(ALAB_FAKE_UNIT_RUNNER=str(stub),
                                           ALAB_UNIT_RESULTS_DIR=str(fixture.fake / "unit-results"))
                fixture.unit_mode(ours="fail", pristine="ok")
                code, output = fixture.check(FIXTURES.CLEAN)
                self.assertEqual(code, 1, output)
                self.assertIn("unit-test attribution refused", output)
                self.assertIn("library-", output)
                self.assertNotIn("ALAB_UNIT ", output)
                for package in ("codex-cli", "codex-core"):
                    self.assertIn(f"native runner child returncode={returncode} package={package}", output)
                path = fixture.fake / "runner-returncodes.jsonl"
                self.assertTrue(path.is_file(), "fake Cargo discarded the raw child statuses")
                records = [json.loads(line) for line in path.read_text().splitlines()]
                self.assertEqual(len(records), 2)
                expected_libraries = {
                    str(fixture.repo / "codex-rs/target/debug/deps" / (crate + "-0123456789abcdef")):
                        str(fixture.repo / "codex-rs" / directory)
                    for crate, directory in (("codex_cli", "cli"), ("codex_core", "core"))
                }
                self.assertEqual({record["library"] for record in records}, set(expected_libraries))
                for record in records:
                    self.assertEqual(record["returncode"], returncode)
                    self.assertEqual(record["cwd"], expected_libraries[record["library"]])
                    runner = record["runner"]
                    self.assertEqual(runner[0], str(fixture.repo / "codex-rs/target/native runner output/alab-unit-runner"))
                    self.assertEqual(runner[1:], ["--state", str(fixture.fake / "unit-results/unit-main"), "--round", "0"])
                    self.assertEqual(record["libtest_args"], [])
                state = fixture.fake / "unit-results/unit-main"
                self.assertEqual((state / "round-0.status").read_text().strip(), "101")
                self.assertEqual(list(state.glob("library-*.json")), [])

    def test_raw_child_status_survives_observation_persistence_failure(self):
        with self.fixture() as fixture:
            stub = fixture.fake / "adversarial-runner"
            stub.write_text("#!/bin/sh\nexit 23\n")
            stub.chmod(0o755)
            fixture.environment.update(ALAB_FAKE_UNIT_RUNNER=str(stub),
                                       ALAB_UNIT_RESULTS_DIR=str(fixture.fake / "unit-results"))
            fixture.unit_mode(ours="fail", pristine="ok")
            observations = fixture.fake / "runner-returncodes.jsonl"
            observations.mkdir()
            code, output = fixture.check(FIXTURES.CLEAN)
            self.assertEqual(code, 1, output)
            self.assertIn("native runner child returncode=23 package=codex-cli", output)
            self.assertIn("IsADirectoryError", output)
            self.assertIn(str(observations), output)
            self.assertNotIn("ALAB_UNIT ", output)
            self.assertTrue(observations.is_dir())
            state = fixture.fake / "unit-results/unit-main"
            self.assertTrue((state / "state.json").is_file())
            self.assertEqual(list(state.glob("library-*.json")), [])


if __name__ == "__main__":
    unittest.main()
