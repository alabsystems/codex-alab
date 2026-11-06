#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Real upstream cached resolution and independent ALab pin verification."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
TARGET = "aarch64-apple-darwin"
VERSION = "9.8.7"


class V8Tests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="alab-v8-")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.repo = self.base / "tree with spaces"
        self.cache = self.base / "cache with spaces"
        package = self.repo / "scripts/codex_package"
        package.mkdir(parents=True)
        for name in ("targets.py", "v8.py"):
            shutil.copyfile(ROOT / "scripts/codex_package" / name, package / name)
        (self.repo / "codex-rs").mkdir()
        (self.repo / "codex-rs/Cargo.lock").write_text(
            f'[[package]]\nname = "v8"\nversion = "{VERSION}"\n')
        self.directory = self.cache / f"rusty-v8-{VERSION}-{TARGET}"
        self.directory.mkdir(parents=True)
        profile = f"ptrcomp_sandbox_release_{TARGET}"
        self.archive = self.directory / f"librusty_v8_{profile}.a.gz"
        self.binding = self.directory / f"src_binding_{profile}.rs"
        self.manifest = self.directory / f"rusty_v8_{profile}.sha256"
        self.archive.write_bytes(bytes(range(256)) * 4)
        self.binding.write_bytes(b"// public binding fixture\n")
        self.pin = self.repo / "third_party/v8/rusty_v8_9_8_7_release_manifests.sha256"
        self.pin.parent.mkdir(parents=True)
        self.pin_assets()

    def pin_assets(self):
        self.manifest.write_text("".join(
            f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n"
            for path in (self.archive, self.binding)))
        self.pin.write_text(
            f"{hashlib.sha256(self.manifest.read_bytes()).hexdigest()}  {self.manifest.name}\n")

    def unverified_upstream(self, body=""):
        # This replacement claims success without checking any bytes, so only
        # ALab's separate parent process can reject the corrupted fixtures.
        source = self.repo / "scripts/codex_package/v8.py"
        source.write_text(f'''from pathlib import Path
def resolved_v8_crate_version():
    return {VERSION!r}
def resolve_codex_v8_cargo_env(spec, *, environ, cache_root):
    assert environ == {{}}
    directory = cache_root / {self.directory.name!r}
    {body or "pass"}
    return {{"RUSTY_V8_ARCHIVE": str(directory / {self.archive.name!r}),
            "RUSTY_V8_SRC_BINDING_PATH": str(directory / {self.binding.name!r})}}
''')

    def builder_python(self):
        return os.environ.get("ALAB_PYTHON", sys.executable)

    def run_resolver(self, env=None):
        return subprocess.run(
            [self.builder_python(), "-I", str(ROOT / "alab/v8.py"), "--repo", str(self.repo),
             "--target", TARGET, "--cache", str(self.cache)],
            capture_output=True, text=True, timeout=60, env=env)

    def assert_refused(self, result):
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "", "failed validation must publish no environment")

    def test_real_current_upstream_accepts_exact_pinned_cached_pair(self):
        result = self.run_resolver()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), {
            "version": VERSION, "target": TARGET,
            "RUSTY_V8_ARCHIVE": str(self.archive),
            "RUSTY_V8_SRC_BINDING_PATH": str(self.binding)})

    def test_valid_existing_cache_requires_no_directory_creation(self):
        # Assert that a warmed cache needs no create syscall, including an
        # unnecessary mkdir(exist_ok=True). Real upstream verification runs.
        code = '''import os, runpy, sys
from pathlib import Path
def denied_mkdir(*args, **kwargs):
    raise AssertionError("fixture forbids directory creation")
os.mkdir = denied_mkdir
try:
    os.mkdir(sys.argv[-1])
except AssertionError:
    print("mkdir denial control observed", file=sys.stderr)
else:
    raise AssertionError("mkdir denial control did not run")
sys.argv = sys.argv[1:]
runpy.run_path(sys.argv[0], run_name="__main__")
'''
        result = subprocess.run(
            [self.builder_python(), "-I", "-c", code, str(ROOT / "alab/v8.py"),
             "--repo", str(self.repo), "--target", TARGET, "--cache", str(self.cache)],
            capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("mkdir denial control observed", result.stderr)
        self.assertEqual(json.loads(result.stdout), {
            "version": VERSION, "target": TARGET,
            "RUSTY_V8_ARCHIVE": str(self.archive),
            "RUSTY_V8_SRC_BINDING_PATH": str(self.binding)})

    def test_source_build_and_partial_or_complete_ambient_overrides_are_ignored(self):
        for overrides in ({"V8_FROM_SOURCE": "1"}, {"RUSTY_V8_ARCHIVE": "/untrusted"},
                          {"RUSTY_V8_ARCHIVE": "/untrusted",
                           "RUSTY_V8_SRC_BINDING_PATH": "/untrusted-binding"}):
            with self.subTest(overrides=overrides):
                environment = dict(os.environ, CODEX_REPO_ROOT="/untrusted-tree", **overrides)
                result = self.run_resolver(environment)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout)["RUSTY_V8_ARCHIVE"], str(self.archive))

    def test_bad_manifest_is_rejected_independently_of_upstream_hash_implementation(self):
        self.unverified_upstream("import hashlib; hashlib.sha256 = lambda *args: None")
        self.manifest.write_bytes(self.manifest.read_bytes() + b"untrusted manifest edit\n")
        self.assert_refused(self.run_resolver())

    def test_corrupt_or_missing_artifact_never_publishes_a_partial_environment(self):
        self.unverified_upstream()
        for path in (self.archive, self.binding):
            original = path.read_bytes()
            for missing in (False, True):
                with self.subTest(path=path.name, missing=missing):
                    if missing:
                        path.unlink()
                    else:
                        path.write_bytes(b"corrupt file")
                    self.assert_refused(self.run_resolver())
                    path.write_bytes(original)

    def test_tree_pin_and_manifest_coverage_are_required(self):
        self.unverified_upstream()
        self.pin.unlink()
        self.assert_refused(self.run_resolver())
        self.pin_assets()
        self.manifest.write_text(self.manifest.read_text().splitlines()[0] + "\n")
        self.pin.write_text(
            f"{hashlib.sha256(self.manifest.read_bytes()).hexdigest()}  {self.manifest.name}\n")
        self.assert_refused(self.run_resolver())

    def test_resolver_failure_preserves_diagnostic_without_environment(self):
        self.unverified_upstream('raise RuntimeError("upstream fixture failure witness")')
        result = self.run_resolver()
        self.assert_refused(result)
        self.assertIn("upstream fixture failure witness", result.stderr)

    def test_returned_paths_must_name_the_exact_target_cache_pair(self):
        self.unverified_upstream()
        source = self.repo / "scripts/codex_package/v8.py"
        source.write_text(source.read_text().replace('str(directory /', 'str(Path("/outside") /'))
        self.assert_refused(self.run_resolver())

    def test_symlinked_cached_input_is_refused_even_with_matching_bytes(self):
        self.unverified_upstream()
        backing = self.base / "archive backing"
        self.archive.rename(backing)
        self.archive.symlink_to(backing)
        self.assert_refused(self.run_resolver())

    def test_python_minimum_is_actionable_before_upstream_or_tomllib_import(self):
        code = ('import runpy, sys; sys.version_info = (3, 9); '
                'sys.argv = sys.argv[1:]; runpy.run_path(sys.argv[0], run_name="__main__")')
        result = subprocess.run(
            [sys.executable, "-I", "-c", code, str(ROOT / "alab/v8.py"),
             "--repo", str(self.base / "missing-tree"), "--target", TARGET,
             "--cache", str(self.base / "untouched")],
            capture_output=True, text=True, timeout=60)
        self.assert_refused(result)
        self.assertIn("ALAB_PYTHON", result.stderr)
        self.assertIn("3.11", result.stderr)
        self.assertFalse((self.base / "untouched").exists())

    def test_actual_package_caller_keeps_the_cache_write_and_owned_code_read_grants(self):
        grants = self.base / "package-grants.json"
        command = '''set -euo pipefail
. "$1/alab/package.sh"
python="$2"
fixture_grants="$3"
package_contained() {
  local inputs=()
  while [[ "$1" != -- ]]; do inputs+=("$1"); shift; done
  "$python" -I -c 'import json, sys; json.dump(sys.argv[2:], open(sys.argv[1], "w"))' \
    "$fixture_grants" "${inputs[@]}"
  shift
  "$@"
}
package_v8_env "$4" "$5" "$6"
'''
        result = subprocess.run(
            ["/bin/bash", "-c", command, "package-v8-fixture", str(ROOT), self.builder_python(),
             str(grants), str(self.repo), TARGET, str(self.cache)],
            capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), [VERSION, str(self.archive), str(self.binding)])
        self.assertEqual(json.loads(grants.read_text()),
                         ["--read", str(ROOT / "alab"), str(self.cache)])


if __name__ == "__main__":
    unittest.main()
