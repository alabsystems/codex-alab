#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise target-selected prebuilt inputs and the actual upstream package assembler."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class PackageInputTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="alab-package-inputs-")).resolve()
        print(f"retained package-input fixture: {self.root}", file=sys.stderr)
        self.repo = self.root / "source with spaces"
        self.repo.mkdir()
        self.target = self.root / "target with spaces.noindex"
        self.env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": str(self.root),
                    "TMPDIR": str(self.root), "GIT_CONFIG_NOSYSTEM": "1"}
        self.cli = self.repo / "scripts/codex_package/cli.py"
        self.cli.parent.mkdir(parents=True)
        self.cli.write_text('FLAGS = ["--entrypoint-bin", "--code-mode-host-bin"]\n')
        self.git("init", "--quiet")
        self.git("config", "user.name", "Package fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.old = self.commit()
        self.cli.write_text('FLAGS = ["--entrypoint-bin", "--code-mode-host-bin", "--exec-server-bin"]\n')
        manifest = self.repo / "codex-rs/exec-server-cli/Cargo.toml"
        manifest.parent.mkdir(parents=True)
        manifest.write_text('[package]\nname = "codex-exec-server-cli"\nversion = "0.0.0"\n')
        self.new = self.commit()

    def git(self, *args):
        done = subprocess.run(["/usr/bin/git", "-C", str(self.repo), *args], env=self.env,
                              capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 0, done.stderr)
        return done.stdout.strip()

    def commit(self):
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "fixture")
        return self.git("rev-parse", "HEAD")

    def inputs(self, commit):
        command = r'''set -euo pipefail
. "$1/alab/package.sh"
repo_root="$2"
commit="$3"
sha9="${commit:0:9}"
CARGO_TARGET_DIR="$4"
package_binary_inputs
"$5" -I -c '
import json,sys
values=iter(sys.argv[1:]); result={"executor":int(next(values))}
for name in ("cargo", "prebuilt", "reads", "binaries"):
    result[name]=[next(values) for _ in range(int(next(values)))]
json.dump(result,sys.stdout)
' "$package_exec_server" "${#package_cargo_args[@]}" "${package_cargo_args[@]}" \
  "${#package_prebuilt_args[@]}" "${package_prebuilt_args[@]}" \
  "${#package_input_reads[@]}" "${package_input_reads[@]}" \
  "${#package_first_party[@]}" "${package_first_party[@]}"
'''
        return subprocess.run(["/bin/bash", "-c", command, "package-input-fixture", str(ROOT),
                               str(self.repo), commit, str(self.target), sys.executable],
                              env=self.env, capture_output=True, text=True, timeout=60)

    def test_historical_target_ignores_new_driver_and_worktree_capability(self):
        done = self.inputs(self.old)
        self.assertEqual(done.returncode, 0, done.stderr)
        inputs = json.loads(done.stdout)
        self.assertEqual(inputs["executor"], 0)
        self.assertNotIn("--exec-server-bin", inputs["prebuilt"])
        self.assertNotIn("codex-exec-server-cli", inputs["cargo"])
        self.assertEqual(inputs["binaries"], ["codex", "codex-code-mode-host"])
        self.assertEqual(inputs["reads"], ["--read", str(self.target / "release/codex"),
                                           "--read", str(self.target / "release/codex-code-mode-host")])

    def test_target_inputs_assemble_executor_without_any_native_build(self):
        # A later working-tree edit must not change the immutable target's inputs.
        self.cli.write_text('FLAGS = []\n')
        done = self.inputs(self.new)
        self.assertEqual(done.returncode, 0, done.stderr)
        inputs = json.loads(done.stdout)
        self.assertEqual(inputs["executor"], 1)
        self.assertIn("codex-exec-server-cli", inputs["cargo"])
        executable = self.target / "release/codex-exec-server"
        self.assertIn(str(executable), inputs["reads"])
        self.assertEqual(inputs["binaries"], ["codex", "codex-code-mode-host", "codex-exec-server"])
        for binary in inputs["binaries"]:
            path = self.target / "release" / binary
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes((binary + " fixture bytes").encode())
            path.chmod(0o755)
        rg = self.root / "rg"
        rg.write_bytes(b"inert ripgrep fixture")
        rg.chmod(0o755)
        package = self.root / "assembled"
        command = '''import sys
from unittest.mock import patch
sys.path.insert(0, sys.argv.pop(1))
from codex_package.cli import main
with patch("codex_package.cargo.subprocess.run", side_effect=AssertionError("unexpected native build")):
    sys.exit(main())
'''
        arguments = ["--target", "aarch64-apple-darwin", "--package-version", "0.1.0-dev.1+gfixture",
                     "--package-dir", str(package), "--rg-bin", str(rg), "--strip", "none"]
        assemble = [sys.executable, "-I", "-B", "-c", command, str(ROOT / "scripts")]
        environment = {**self.env, "CODEX_REPO_ROOT": str(ROOT)}
        done = subprocess.run(assemble + arguments + inputs["prebuilt"], env=environment,
                              capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertEqual((package / "bin/codex-exec-server").read_bytes(), executable.read_bytes())
        self.assertEqual(json.loads((package / "codex-package.json").read_text())["execServer"],
                         "bin/codex-exec-server")
        # This control proves that an omitted executor input would trigger Cargo.
        omitted = inputs["prebuilt"][:-2]
        done = subprocess.run(assemble + arguments + omitted, env=environment,
                              capture_output=True, text=True, timeout=60)
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("unexpected native build", done.stderr)

    def test_incomplete_target_or_unreadable_revision_is_refused(self):
        self.git("rm", "--quiet", "codex-rs/exec-server-cli/Cargo.toml")
        incomplete = self.commit()
        for commit in (incomplete, "absent-revision"):
            with self.subTest(commit=commit):
                done = self.inputs(commit)
                self.assertNotEqual(done.returncode, 0)
                self.assertEqual(done.stdout, "")


if __name__ == "__main__":
    unittest.main()
