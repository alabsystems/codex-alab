#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise actual shell dispatch before any compiler/cache work; native auth is Rust-tested."""
import json
import os
from pathlib import Path
import shutil
import shlex
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
# A shebang cannot contain a Python path with spaces.
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'


class ProjectToolchainTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="alab-project-selector-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.repo = self.root / "source with spaces"
        (self.repo / "alab").mkdir(parents=True)
        for name in ("lib.sh", "check.sh", "package.sh", "github.py", "cli_status.py", "command_guard.py", "project-targo.sh"):
            shutil.copy2(ROOT / "alab" / name, self.repo / "alab" / name)
        self.cache = self.root / "must not exist cache"
        self.record = self.root / "native.json"
        self.front = self.root / "native recorder"
        self.front.write_text(PYTHON_ENTRY + f"import json,os,sys\nfrom pathlib import Path\n"
                              f"Path({str(self.record)!r}).write_text(json.dumps(dict(argv=sys.argv[1:],"
                              f"cwd=os.getcwd(),cargo_home=os.environ.get('CARGO_HOME'),git_dir=os.environ.get('GIT_DIR'),cache_exists=Path({str(self.cache)!r}).exists())))\n"
                              "raise SystemExit(75)\n")
        self.front.chmod(0o755)
        git_program = shutil.which("git")
        self.assertIsNotNone(git_program, "Git is required for project-selection fixtures")
        fixture_path = str(Path(git_program).parent) + ":/usr/bin:/bin:/usr/sbin:/sbin"
        self.env = dict(HOME=str(self.root), PATH=fixture_path,
                        ALAB_PYTHON=sys.executable, ALAB_ATPKG_PUBLISH=str(self.front),
                        ALAB_PACKAGE_CACHE=str(self.cache), GIT_CONFIG_GLOBAL=os.devnull,
                        GIT_CONFIG_NOSYSTEM="1", GIT_AUTHOR_NAME="Fixture",
                        GIT_AUTHOR_EMAIL="fixture@example.invalid", GIT_COMMITTER_NAME="Fixture",
                        GIT_COMMITTER_EMAIL="fixture@example.invalid")
        self.git("init", "-q", "--template=", "-b", "main")
        self.git("add", ".")
        self.git("commit", "-qm", "first")
        self.first = self.git("rev-parse", "HEAD").strip()
        (self.repo / "later").write_text("different HEAD\n")
        self.git("add", ".")
        self.git("commit", "-qm", "later")

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.repo), *args], env=self.env,
                              capture_output=True, text=True, check=True, timeout=20).stdout

    def run_shell(self, body, *args, env=None):
        return subprocess.run(["/bin/bash", "-c", body, "fixture", str(self.repo), *args],
                              env=env or self.env, cwd=self.repo, capture_output=True,
                              text=True, timeout=30)

    def assert_unsupported_source_shell(self, name):
        shell = shutil.which(name, path=self.env["PATH"])
        if shell is None:
            self.skipTest(name + " is unavailable for the wrong-shell fixture")
        external_trace = self.root / "unexpected shell preflight command"
        fake_bin = self.root / "guard command lookup"
        fake_bin.mkdir()
        # A source guard must query a real builtin, not execute a PATH command
        # of the same name when an ambient version variable is misleading.
        for command in ("builtin", "shopt"):
            program = fake_bin / command
            program.write_text("#!/bin/sh\nprintf 'called\\n' >> " +
                               shlex.quote(str(external_trace)) + "\nexit 0\n")
            program.chmod(0o755)
        body = r"""
alab_library_root=source-not-initialized
. "$1/alab/lib.sh"
source_status=$?
printf 'source_status=%s\n' "$source_status"
printf 'library_root=%s\n' "$alab_library_root"
if command -v alab_project_run >/dev/null 2>&1; then
    printf 'selector=defined\n'
    alab_project_run "$1" "" /usr/bin/true
else
    printf 'selector=undefined\n'
fi
printf 'caller=alive\n'
"""
        diagnostic = "alab/lib.sh: Bash is required; source this library from Bash."
        for spoof_version in (False, True):
            for direct in (False, True):
                with self.subTest(shell=name, spoof_version=spoof_version, direct=direct):
                    for record in (self.record, external_trace):
                        record.unlink(missing_ok=True)
                    env = dict(self.env, PATH=str(fake_bin) + ":" + self.env["PATH"])
                    if spoof_version:
                        env["BASH_VERSION"] = "99.0-fixture"
                    if direct:
                        argv = [shell, "-f", str(self.repo / "alab/lib.sh")]
                    else:
                        argv = [shell, "-f", "-c", body, "fixture", str(self.repo)]
                    done = subprocess.run(argv, env=env, cwd=self.repo,
                                          capture_output=True, text=True, timeout=10)
                    self.assertFalse(self.record.exists(),
                                     "an unsupported shell reached the native recorder")
                    self.assertFalse(self.cache.exists())
                    self.assertFalse(external_trace.exists(),
                                     "shell preflight executed an external lookup candidate")
                    self.assertEqual(done.stderr.strip(), diagnostic)
                    if direct:
                        self.assertEqual(done.returncode, 2, done.stderr)
                    else:
                        # Capture the dot command's status: a later successful
                        # printf must not turn a failed source into a green test.
                        self.assertEqual(done.returncode, 0, done.stderr)
                        self.assertEqual(done.stdout.splitlines(), [
                            "source_status=2", "library_root=source-not-initialized",
                            "selector=undefined", "caller=alive",
                        ])

    def test_zsh_rejects_library_before_any_controller_or_helper(self):
        self.assert_unsupported_source_shell("zsh")

    def test_dash_rejects_library_before_any_controller_or_helper(self):
        self.assert_unsupported_source_shell("dash")

    def test_bash_source_preserves_real_helper_and_initialization(self):
        target = str(self.root / "ordinary target with spaces")
        done = self.run_shell(
            'set -euo pipefail; . "$1/alab/lib.sh"; '
            'printf "%s\\n" "$alab_library_root"; alab_check_lint_target "$2"', target)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.splitlines(), [str(self.repo), target + "/tippy"])
        self.assertEqual(done.stderr, "")
        self.assertFalse(self.record.exists())
        self.assertFalse(self.cache.exists())

    def test_gate_enters_working_authority_before_any_target_or_compiler(self):
        done = subprocess.run([str(self.repo / "alab/check.sh")], env=self.env,
                              cwd=self.repo, capture_output=True, text=True, timeout=30)
        self.assertEqual(done.returncode, 75, done.stderr)
        rec = json.loads(self.record.read_text())
        self.assertEqual(rec["argv"][:5], ["toolchain", "run", "--repo", str(self.repo), "--"])
        self.assertFalse(rec["cache_exists"])
        self.assertEqual(rec["cargo_home"], str(self.repo / ".git/alab-cargo-home"))
        self.assertFalse((self.repo / "codex-rs").exists())

    def test_native_selection_uses_the_same_explicit_private_cargo_home(self):
        private = self.root / "private Cargo home"
        env = dict(self.env, ALAB_CARGO_HOME=str(private), CARGO_HOME=str(self.root / "ambient"))
        done = subprocess.run([str(self.repo / "alab/check.sh")], env=env,
                              cwd=self.repo, capture_output=True, text=True, timeout=30)
        self.assertEqual(done.returncode, 75, done.stderr)
        self.assertEqual(json.loads(self.record.read_text())["cargo_home"], str(private))
        self.assertFalse(private.exists(), "selection must precede cache preparation")

    def test_historical_package_binds_full_source_before_cache(self):
        done = self.run_shell('. "$1/alab/package.sh"; shift; package_entry "$@"',
                              "--out", str(self.root / "output with spaces"),
                              "--commit", self.first, "--no-promote")
        self.assertEqual(done.returncode, 75, done.stderr)
        rec = json.loads(self.record.read_text())
        self.assertEqual(rec["argv"][:7], ["toolchain", "run", "--repo", str(self.repo),
                                          "--source", self.first, "--"])
        self.assertEqual(rec["argv"][-2:], ["--commit", self.first])
        self.assertFalse(rec["cache_exists"])
        self.assertNotEqual(self.first, self.git("rev-parse", "HEAD").strip())

    def test_source_resolution_ignores_inherited_git_redirect(self):
        other = self.root / "other"
        other.mkdir()
        self.git("init", "-q", "--template=", str(other))
        done = self.run_shell('. "$1/alab/package.sh"; shift; package_entry "$@"',
                              "--commit", self.first, "--no-promote",
                              env=dict(self.env, GIT_DIR=str(other / ".git")))
        self.assertEqual(done.returncode, 75, done.stderr)
        rec = json.loads(self.record.read_text())
        self.assertEqual(rec["argv"][5], self.first)
        self.assertIsNone(rec["git_dir"])
        self.assertEqual(rec["cargo_home"], str(self.repo / ".git/alab-cargo-home"))

    def test_package_help_needs_neither_native_selector_nor_cache(self):
        done = self.run_shell('. "$1/alab/package.sh"; package_entry --help')
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertFalse(self.record.exists())
        self.assertFalse(self.cache.exists())

    def test_missing_revision_starts_no_native_selector_or_cache(self):
        done = self.run_shell('. "$1/alab/package.sh"; package_entry --commit absent')
        self.assertNotEqual(done.returncode, 0)
        self.assertFalse(self.record.exists())
        self.assertFalse(self.cache.exists())

    def test_explicit_hop_preserves_same_target_in_pristine_cwd(self):
        pristine = self.root / "pristine"
        pristine.mkdir()
        env = dict(self.env, ATPKG_PROJECT_FRONT=str(self.front), ATPKG_PROJECT_TARGO="/usr/bin/true")
        done = subprocess.run([str(self.repo / "alab/project-targo.sh"), "--unverified", "check", "--locked"],
                              env=env, cwd=pristine, capture_output=True, text=True, timeout=30)
        self.assertEqual(done.returncode, 75)
        rec = json.loads(self.record.read_text())
        self.assertEqual(rec["argv"], ["__build_admit", "/usr/bin/true", "--unverified", "check", "--locked"])
        self.assertEqual(rec["cwd"], str(pristine))

    def test_qualification_cannot_disable_the_existing_profile(self):
        done = self.run_shell('. "$1/alab/lib.sh"; alab_project_selected',
                              env=dict(self.env, ALAB_CONTAIN="0"))
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("requires the protected profile", done.stderr)
        self.assertFalse(self.record.exists())

    def test_local_compiler_status_requires_the_selected_execution_root(self):
        execution = self.root / "selected execution"
        execution.mkdir()
        record = self.root / "compiler.json"
        record.write_text(json.dumps({"sysroot": str(execution)}))
        command = '. "$1/alab/package.sh"; package_record_native_compiler "$ALAB_PYTHON" "$2" "$3"'
        done = self.run_shell(command, str(record), str(execution))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIs(json.loads(record.read_text())["admitted"], True)
        for value in ({"sysroot": str(self.root / "other")}, {}, {"sysroot": "relative"}):
            original = json.dumps(value)
            record.write_text(original)
            done = self.run_shell(command, str(record), str(execution))
            self.assertNotEqual(done.returncode, 0)
            self.assertEqual(record.read_text(), original, "a mismatched observation must never be marked admitted")

    def test_absent_selection_cannot_run_compiler(self):
        done = subprocess.run([str(self.repo / "alab/project-targo.sh"), "--version"],
                              env=self.env, capture_output=True, text=True, timeout=30)
        self.assertNotEqual(done.returncode, 0)
        self.assertFalse(self.record.exists())


if __name__ == "__main__":
    unittest.main()
