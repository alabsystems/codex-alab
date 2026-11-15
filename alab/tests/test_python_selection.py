# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Managed versioned Python must work without an unversioned PATH alias."""

from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import unittest


LIBRARY = Path(__file__).resolve().parents[1] / "lib.sh"


@unittest.skipUnless(
    Path("/bin/bash").is_file() and sys.version_info >= (3, 11),
    "requires Bash and a Python 3.11+ fixture interpreter",
)
class PythonSelectionTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="alab python selection ")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name).resolve()
        self.prefix = self.root / "managed package"
        self.binary = self.prefix / "bin" / "python3.14"
        self.binary.parent.mkdir(parents=True)
        self.trace = self.root / "managed-probes"
        self.binary.write_text(
            "#!/bin/sh\n"
            f"printf 'called\\n' >> {shlex.quote(str(self.trace))}\n"
            f"exec {shlex.quote(sys.executable)} \"$@\"\n"
        )
        self.binary.chmod(0o755)
        self.environment = {
            "PATH": "/usr/bin:/bin",
            "HOME": str(self.root / "home"),
            "ATPKG_BIN": str(self.binary.parent),
        }

    def select(self, override=None):
        environment = dict(self.environment)
        if override is not None:
            environment["ALAB_PYTHON"] = override
        return subprocess.run(
            ["/bin/bash", "-c",
             'set -eu; . "$1"; alab_python_311 "$2"',
             "python-selection", str(LIBRARY), str(self.prefix)],
            cwd=self.root, env=environment, capture_output=True, text=True, timeout=10,
        )

    def test_managed_versioned_binary_is_found_outside_path(self):
        result = self.select()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), str(self.binary))
        self.assertTrue(self.trace.exists())

    def test_unsuitable_unversioned_candidate_does_not_hide_versioned_binary(self):
        old = self.binary.with_name("python3")
        for status in (0, 1):
            with self.subTest(status=status):
                old.write_text(f"#!/bin/sh\nexit {status}\n")
                old.chmod(0o755)
                result = self.select()
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.strip(), str(self.binary))

    def test_managed_versions_are_discovered_without_a_fixed_version_ceiling(self):
        future = self.binary.with_name("python3.99")
        self.binary.rename(future)
        config = future.with_name("python3.10-config")
        config.write_text("#!/bin/sh\nprintf 'not-an-interpreter\\n'\nexit 0\n")
        config.chmod(0o755)
        result = self.select()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), str(future))

    def test_explicit_interpreter_wins_without_probing_managed_candidates(self):
        result = self.select(sys.executable)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), sys.executable)
        self.assertFalse(self.trace.exists())

    def test_invalid_explicit_interpreter_is_refused_without_fallback(self):
        result = self.select("/bin/false")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no Python >= 3.11", result.stderr)
        self.assertFalse(self.trace.exists())

    def test_zero_exit_without_python_response_is_not_an_interpreter(self):
        candidate = self.root / "not Python"
        candidate.write_text("#!/bin/sh\nexit 0\n")
        candidate.chmod(0o755)
        result = self.select(str(candidate))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no Python >= 3.11", result.stderr)
        self.assertFalse(self.trace.exists())

    def test_probe_output_from_a_failed_command_is_not_accepted(self):
        candidate = self.root / "failed probe"
        candidate.write_text("#!/bin/sh\nprintf 'alab-python-311\\n'\nexit 1\n")
        candidate.chmod(0o755)
        result = self.select(str(candidate))
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.trace.exists())

    def test_relative_override_survives_a_directory_change(self):
        result = self.select(str(self.binary.relative_to(self.root)))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(Path(result.stdout.strip()), self.binary)
        other = self.root / "different directory"
        other.mkdir()
        executed = subprocess.run(
            [result.stdout.strip(), "-I", "-c", "print('selected')"],
            cwd=other, capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(executed.returncode, 0, executed.stderr)
        self.assertEqual(executed.stdout, "selected\n")

    def test_relative_virtual_environment_keeps_its_identity(self):
        environment = self.root / "virtual environment"
        subprocess.run(
            [sys.executable, "-I", "-m", "venv", "--without-pip", str(environment)],
            check=True, capture_output=True, timeout=30,
        )
        binary = environment / "bin" / "python3"
        result = self.select(str(binary.relative_to(self.root)))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(Path(result.stdout.strip()), binary)
        executed = subprocess.run(
            [result.stdout.strip(), "-I", "-c", "import sys; print(sys.prefix)"],
            cwd=self.root.parent, capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(executed.returncode, 0, executed.stderr)
        self.assertEqual(executed.stdout.strip(), str(environment))


if __name__ == "__main__":
    unittest.main()
