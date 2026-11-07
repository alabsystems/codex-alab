#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the real source-version boundary without a compiler or publisher."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
VERSION = "0.166.0-dev.1+gaaaaaaaaa"


class PackageSourceVersionTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="alab-source-version-")).resolve()
        print(f"retained source-version fixture: {self.root}", file=sys.stderr)
        self.repo = self.root / "source with spaces"
        self.repo.mkdir()
        self.home = self.root / "home"
        self.home.mkdir()
        self.git_program = shutil.which("git")
        self.assertIsNotNone(self.git_program, "Git is required for source-version fixtures")
        fixture_path = str(Path(self.git_program).parent) + ":/usr/bin:/bin:/usr/sbin:/sbin"
        self.env = {"PATH": fixture_path, "HOME": str(self.home),
                    "TMPDIR": str(self.root), "GIT_CONFIG_NOSYSTEM": "1"}
        self.git("init", "--quiet")
        self.git("config", "user.name", "Package fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.manifest = self.repo / "codex-rs/Cargo.toml"
        self.manifest.parent.mkdir()
        self.manifest.write_text('[workspace.package]\nversion = "0.0.0"\n\n[workspace]\nmembers = []\n')
        self.lock = self.repo / "codex-rs/Cargo.lock"
        self.lock.write_text('[[package]]\nname = "fixture"\nversion = "0.0.0"\n')
        (self.repo / "payload").write_bytes(b"tracked source")
        self.calls = self.root / "metadata-calls"

    def git(self, *args):
        done = subprocess.run([self.git_program, "-C", str(self.repo), *args],
                              env=self.env, capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 0, done.stderr)
        return done.stdout.strip()

    def commit(self, capability):
        if capability is not None:
            source = self.repo / "codex-rs/alab-core/src/version.rs"
            source.parent.mkdir(parents=True)
            # The source capability is a hint; actual compiled versions still face smoke.
            source.write_text('option_env!("CODEX_ALAB_BUILD_VERSION");\n' if capability else
                              'pub fn alab() -> &\'static str { "dev" }\n')
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "fixture source")
        return self.git("rev-parse", "HEAD")

    def prepare(self, commit, mode="versions"):
        command = r'''set -euo pipefail
. "$1/alab/package.sh"
repo_root="$2"
wt="$2"
commit="$3"
sha9="${commit:0:9}"
version="$4"
CARGO_TARGET_DIR="$5"
fixture_calls="$6"
fixture_mode="$7"
fixture_python="$8"
cargo=fixture_cargo
package_contained() {
  [[ "$1" == "$wt/codex-rs/Cargo.lock" && "$2" == "$CARGO_TARGET_DIR" && "$3" == -- ]] || return 91
  shift 3
  "$@"
}
fixture_cargo() {
  [[ "$*" == 'metadata --format-version 1' ]] || return 92
  printf '%s\n' "$*" >> "$fixture_calls"
  [[ "$fixture_mode" != fail ]] || return 93
  "$fixture_python" -I -c '
from pathlib import Path
import sys
lock, version, mode = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
lock.write_text(lock.read_text().replace("0.0.0", version))
if mode == "dependency":
    with lock.open("a") as out:
        out.write("checksum = \"unexpected dependency change\"\n")
elif mode == "source":
    Path("../payload").write_bytes(b"unexpected source change")
' "$wt/codex-rs/Cargo.lock" "$version" "$fixture_mode"
}
package_source_version
printf '%s\n' "$package_leaf_versions"
'''
        done = subprocess.run(
            ["/bin/bash", "-c", command, "source-version-fixture", str(ROOT), str(self.repo),
             commit, VERSION, str(self.root / "target.noindex"), str(self.calls), mode,
             sys.executable], env=self.env, capture_output=True, text=True, timeout=60)
        (self.root / "stdout").write_text(done.stdout)
        (self.root / "stderr").write_text(done.stderr)
        (self.root / "result.json").write_text(json.dumps({"exit": done.returncode,
                                                           "metadata_called": self.calls.exists()}, indent=2))
        return done

    def test_leaf_stamp_keeps_manifest_and_lock_without_metadata(self):
        commit = self.commit(True)
        before = (self.manifest.read_bytes(), self.lock.read_bytes())
        # Capability comes from the immutable commit, not a source file changed later.
        (self.repo / "codex-rs/alab-core/src/version.rs").write_text("changed after commit\n")
        done = self.prepare(commit, mode="fail")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout, "1\n")
        self.assertEqual((self.manifest.read_bytes(), self.lock.read_bytes()), before)
        self.assertFalse(self.calls.exists(), "capable source must not resolve or rewrite the lock")

    def test_legacy_source_still_stamps_the_workspace_and_only_lock_versions(self):
        commit = self.commit(False)
        done = self.prepare(commit)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout, "0\n")
        self.assertIn(f'version = "{VERSION}"', self.manifest.read_text())
        self.assertIn(f'version = "{VERSION}"', self.lock.read_text())
        self.assertEqual(self.calls.read_text(), "metadata --format-version 1\n")
        self.assertIn('version = "0.0.0"', self.git("show", f"{commit}:codex-rs/Cargo.toml"))

    def test_source_before_version_module_uses_legacy_stamp(self):
        done = self.prepare(self.commit(None))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn(f'version = "{VERSION}"', self.manifest.read_text())
        self.assertTrue(self.calls.exists())

    def test_unreadable_commit_refuses_before_source_change_or_metadata(self):
        self.commit(True)
        before = (self.manifest.read_bytes(), self.lock.read_bytes())
        done = self.prepare("f" * 40)
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("cannot read", done.stderr)
        self.assertEqual((self.manifest.read_bytes(), self.lock.read_bytes()), before)
        self.assertFalse(self.calls.exists())

    def test_legacy_metadata_failure_is_not_accepted(self):
        done = self.prepare(self.commit(False), mode="fail")
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("metadata could not resolve", done.stderr)
        self.assertTrue(self.calls.exists())

    def test_legacy_dependency_change_is_not_accepted_or_discarded(self):
        done = self.prepare(self.commit(False), mode="dependency")
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("more than the workspace versions", done.stderr)
        self.assertIn("unexpected dependency change", self.lock.read_text())

    def test_legacy_source_change_is_not_accepted_or_discarded(self):
        done = self.prepare(self.commit(False), mode="source")
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("more than the manifest and lock", done.stderr)
        self.assertEqual((self.repo / "payload").read_bytes(), b"unexpected source change")


if __name__ == "__main__":
    unittest.main()
