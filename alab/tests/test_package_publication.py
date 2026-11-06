#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Only newly owned retained fixtures exercise the actual host publish boundary."""

import errno
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("publish_directory", ROOT / "alab/publish_directory.py")
PUBLISH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PUBLISH)


class PackagePublicationTests(unittest.TestCase):
    def setUp(self):
        self.home = Path(tempfile.mkdtemp(prefix="alab-publish-directory-")).resolve()
        print(f"retained publication fixture: {self.home}", file=sys.stderr)
        self.staged = self.home / "stage with spaces"
        self.staged.mkdir()
        (self.staged / "payload").write_bytes(b"new package fixture; never executed")
        self.output = self.home / "output with spaces"
        self.output.mkdir()
        self.published = self.output / "published"
        self.selection_calls = self.home / "selection-calls"

    def publish(self, promote=True, cleanup=False):
        # Only the exclusive-rename helper may run. A promotion attempt is recorded,
        # never allowed to reach a real selector, even against the old mv boundary.
        command = '''set -euo pipefail
. "$1/alab/package.sh"
fixture_python="$2"
fixture_selection_calls="$3"
fixture_cleanup="$4"
fixture_rm_calls="$5"
fixture_other_scratch="$6"
fixture_host() {
  if [[ "${2:-}" == "$repo_root/alab/publish_directory.py" ]]; then
    "$fixture_python" "$@"
  else
    printf '%s\\n' "$@" >> "$fixture_selection_calls"
    return 97
  fi
}
shift 6
if [[ "$fixture_cleanup" == 1 ]]; then
  package_cleanup_allowed() { return 0; }
  rm() { printf '%s\\n' "$@" >> "$fixture_rm_calls"; }
  wt=unused-fixture-worktree
  scratch_paths=("$1" "$fixture_other_scratch")
  trap package_cleanup EXIT
fi
alab_publish_package fixture_host "$@"
'''
        done = subprocess.run(
            ["/bin/bash", "-c", command, "publication-fixture", str(ROOT), sys.executable,
             str(self.selection_calls), "1" if cleanup else "0", str(self.home / "rm-calls"),
             str(self.home / "other-scratch"), str(self.staged), str(self.published), str(self.output),
             str(self.home / "fake-prefix"), "0" * 40, "1" if promote else "0"],
            capture_output=True, text=True, timeout=30)
        (self.home / "stdout").write_text(done.stdout)
        (self.home / "stderr").write_text(done.stderr)
        facts = {
            "exit": done.returncode,
            "staged_exists": self.staged.exists(),
            "published_is_symlink": self.published.is_symlink(),
            "nested_stage_exists": (self.published / self.staged.name).exists(),
            "selection_attempted": self.selection_calls.exists(),
        }
        (self.home / "observation.json").write_text(json.dumps(facts, indent=2) + "\n")
        return done

    @unittest.skipUnless(sys.platform == "darwin", "the package producer requires macOS")
    def test_destination_created_after_preflight_keeps_both_directories(self):
        self.assertFalse(self.published.exists())
        # A producer wins after package_main's early absence check, before publish.
        self.published.mkdir()
        sentinel = self.published / "sentinel"
        sentinel.write_bytes(b"existing artifact")
        done = self.publish()
        self.assertNotEqual(done.returncode, 0)
        self.assertEqual(sorted(p.name for p in self.published.iterdir()), ["sentinel"])
        self.assertEqual(sentinel.read_bytes(), b"existing artifact")
        self.assertEqual((self.staged / "payload").read_bytes(), b"new package fixture; never executed")
        self.assertFalse(self.selection_calls.exists(), "conflict must not attempt selection")

    @unittest.skipUnless(sys.platform == "darwin", "the package producer requires macOS")
    def test_absent_destination_accepts_cross_parent_same_volume_publication(self):
        done = self.publish(promote=False)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertFalse(self.staged.exists())
        self.assertEqual((self.published / "payload").read_bytes(), b"new package fixture; never executed")
        self.assertFalse(self.selection_calls.exists())

    @unittest.skipUnless(sys.platform == "darwin", "the package producer requires macOS")
    def test_existing_empty_destination_is_not_replaced(self):
        self.published.mkdir()
        identity = self.published.stat().st_ino
        done = self.publish()
        self.assertNotEqual(done.returncode, 0)
        self.assertEqual(self.published.stat().st_ino, identity)
        self.assertEqual(list(self.published.iterdir()), [])
        self.assertEqual((self.staged / "payload").read_bytes(), b"new package fixture; never executed")
        self.assertFalse(self.selection_calls.exists(), "empty conflict must not attempt selection")

    @unittest.skipUnless(sys.platform == "darwin", "the package producer requires macOS")
    def test_failed_publication_keeps_validated_staging_when_cleanup_is_allowed(self):
        self.published = self.output / "absent parent" / "published"
        other = self.home / "other-scratch"
        other.mkdir()
        done = self.publish(cleanup=True)
        self.assertNotEqual(done.returncode, 0)
        calls = (self.home / "rm-calls").read_text().splitlines()
        self.assertNotIn(str(self.staged), calls, "EXIT cleanup must preserve validated staging")
        self.assertIn(str(other), calls, "unrelated scratch keeps the normal cleanup policy")
        self.assertIn(str(self.staged), done.stderr)
        self.assertEqual((self.staged / "payload").read_bytes(), b"new package fixture; never executed")
        self.assertTrue(other.is_dir(), "the rm fixture records attempts without deleting")
        self.assertFalse(self.selection_calls.exists())

    @unittest.skipUnless(sys.platform == "darwin", "the package producer requires macOS")
    def test_selection_failure_keeps_published_package_and_other_cleanup(self):
        other = self.home / "other-scratch"
        other.mkdir()
        done = self.publish(cleanup=True)
        self.assertEqual(done.returncode, 1, done.stderr)
        self.assertFalse(self.staged.exists())
        self.assertEqual((self.published / "payload").read_bytes(), b"new package fixture; never executed")
        self.assertTrue(self.selection_calls.exists(), "the fixture selector refuses after rename")
        calls = (self.home / "rm-calls").read_text().splitlines()
        self.assertNotIn(str(self.staged), calls)
        self.assertNotIn(str(self.published), calls)
        self.assertIn(str(other), calls)
        self.assertTrue(other.is_dir(), "the rm fixture records attempts without deleting")

    @unittest.skipUnless(sys.platform == "darwin", "the package producer requires macOS")
    def test_symlink_destination_keeps_link_target_and_staging(self):
        target = self.home / "existing target"
        target.mkdir()
        (target / "sentinel").write_bytes(b"existing target")
        self.published.symlink_to(target, target_is_directory=True)
        done = self.publish()
        self.assertNotEqual(done.returncode, 0)
        self.assertTrue(self.published.is_symlink())
        self.assertEqual(os.readlink(self.published), str(target))
        self.assertEqual(sorted(p.name for p in target.iterdir()), ["sentinel"])
        self.assertEqual((target / "sentinel").read_bytes(), b"existing target")
        self.assertEqual((self.staged / "payload").read_bytes(), b"new package fixture; never executed")
        self.assertFalse(self.selection_calls.exists(), "symlink conflict must not attempt selection")

    @unittest.skipUnless(sys.platform == "darwin", "the package producer requires macOS")
    def test_dangling_symlink_destination_is_not_replaced(self):
        target = self.home / "missing target"
        self.published.symlink_to(target, target_is_directory=True)
        done = self.publish()
        self.assertNotEqual(done.returncode, 0)
        self.assertTrue(self.published.is_symlink())
        self.assertEqual(os.readlink(self.published), str(target))
        self.assertFalse(target.exists())
        self.assertEqual((self.staged / "payload").read_bytes(), b"new package fixture; never executed")
        self.assertFalse(self.selection_calls.exists(), "dangling link conflict must not attempt selection")

    def test_unsupported_platform_refuses_without_fallback(self):
        with mock.patch.object(PUBLISH.sys, "platform", "unsupported"):
            with self.assertRaises(OSError):
                PUBLISH.publish_directory(self.staged, self.published)
        self.assertTrue(self.staged.is_dir())
        self.assertFalse(self.published.exists())

    def test_cross_device_error_refuses_without_copy_fallback(self):
        library = mock.Mock()
        library.renamex_np.return_value = -1
        with mock.patch.object(PUBLISH.sys, "platform", "darwin"), \
                mock.patch.object(PUBLISH.ctypes, "CDLL", return_value=library), \
                mock.patch.object(PUBLISH.ctypes, "get_errno", return_value=errno.EXDEV):
            with self.assertRaises(OSError) as raised:
                PUBLISH.publish_directory(self.staged, self.published)
        self.assertEqual(raised.exception.errno, errno.EXDEV)
        library.renamex_np.assert_called_once_with(
            os.fsencode(self.staged), os.fsencode(self.published), PUBLISH.RENAME_EXCL)
        self.assertEqual((self.staged / "payload").read_bytes(), b"new package fixture; never executed")
        self.assertFalse(self.published.exists())


class PackageTargetCacheTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="alab-package-target-")).resolve()
        print(f"retained target cache fixture: {self.root}", file=sys.stderr)
        self.home = self.root / "private-home"
        self.home.mkdir()
        self.cache = self.root / "cache with spaces"
        self.cache.mkdir()
        self.compiler = "0123456789abcdef"
        self.target = "aarch64-apple-darwin"

    def prepare(self, source, *, compiler=None, cache=None, target=None):
        # Call the package's actual allocator; no compiler, package build or selector.
        command = r'''set -euo pipefail
. "$1/alab/package.sh"
python="$2"
cache="$3"
compiler_id="$4"
target_triple="$5"
wt="$6"
alab_account_home() { printf '%s\n' "$HOME"; }
cd "$wt"
package_rust_target
'''
        return subprocess.run(
            ["/bin/bash", "-c", command, "target-cache-fixture", str(ROOT), sys.executable,
             str(self.cache if cache is None else cache),
             self.compiler if compiler is None else compiler,
             self.target if target is None else target, str(source)],
            env={"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": str(self.home),
                 "TMPDIR": str(self.root)},
            capture_output=True, text=True, timeout=60)

    def source(self, name, version):
        source = self.root / name
        source.mkdir()
        (source / "Cargo.toml").write_text(f'[workspace.package]\nversion = "{version}"\n')
        (source / "Cargo.lock").write_text(f'[[package]]\nname = "fixture"\nversion = "{version}"\n')
        return source

    def test_successive_stamped_sources_reuse_only_the_same_compiler_target(self):
        first = self.source("source-a", "0.166.0-dev.1+gaaaaaaaaa")
        second = self.source("source-b", "0.166.0-dev.2+gbbbbbbbbb")
        before = {str(f): f.read_bytes() for s in (first, second) for f in s.iterdir()}
        prepared = self.prepare(first)
        self.assertEqual(prepared.returncode, 0, prepared.stderr)
        target = Path(prepared.stdout.strip())
        self.assertEqual(target.parent, self.cache)
        self.assertTrue(target.name.endswith(".noindex"))
        identity = target.stat().st_ino
        (target / "dependency-output").write_bytes(b"inert cached fixture; never executed")
        alias = self.root / "cache-alias"
        alias.symlink_to(self.cache, target_is_directory=True)
        reused = self.prepare(second, cache=alias)
        self.assertEqual(reused.returncode, 0, reused.stderr)
        self.assertEqual(Path(reused.stdout.strip()), target)
        self.assertEqual(target.stat().st_ino, identity)
        self.assertEqual((target / "dependency-output").read_bytes(),
                         b"inert cached fixture; never executed")
        changed = self.prepare(second, compiler="fedcba9876543210")
        self.assertEqual(changed.returncode, 0, changed.stderr)
        other = Path(changed.stdout.strip())
        self.assertNotEqual(other, target)
        self.assertTrue(other.is_dir())
        self.assertFalse((other / "dependency-output").exists())
        self.assertEqual({name: Path(name).read_bytes() for name in before}, before)

    def test_target_symlink_file_and_unsafe_directory_are_preserved_and_refused(self):
        source = self.source("source", "0.166.0-dev.1+gaaaaaaaaa")
        name = f"rust-release-{self.target}-{self.compiler}.noindex"
        decoy = self.root / "decoy"
        decoy.mkdir()
        (decoy / "sentinel").write_bytes(b"foreign directory")
        for kind in ("symlink", "file", "writable-directory"):
            with self.subTest(kind=kind):
                cache = self.root / kind
                cache.mkdir()
                entry = cache / name
                if kind == "symlink":
                    entry.symlink_to(decoy, target_is_directory=True)
                elif kind == "file":
                    entry.write_bytes(b"foreign file")
                else:
                    entry.mkdir()
                    entry.chmod(0o777)
                identity = entry.lstat().st_ino
                done = self.prepare(source, cache=cache)
                self.assertNotEqual(done.returncode, 0)
                self.assertEqual(entry.lstat().st_ino, identity)
                self.assertEqual((decoy / "sentinel").read_bytes(), b"foreign directory")
                if kind == "file":
                    self.assertEqual(entry.read_bytes(), b"foreign file")
                elif kind == "symlink":
                    self.assertTrue(entry.is_symlink())
                else:
                    self.assertEqual(list(entry.iterdir()), [])

    def test_home_ancestor_writable_cache_and_untrusted_keys_are_refused(self):
        source = self.source("source", "0.166.0-dev.1+gaaaaaaaaa")
        unsafe = self.root / "writable-cache"
        unsafe.mkdir()
        unsafe.chmod(0o777)
        for cache in (self.home, self.root, unsafe):
            with self.subTest(cache=cache):
                before = sorted(p.name for p in cache.iterdir())
                done = self.prepare(source, cache=cache)
                self.assertNotEqual(done.returncode, 0)
                self.assertEqual(sorted(p.name for p in cache.iterdir()), before)
        for changes in ({"compiler": "../outside"}, {"target": "../../outside"}):
            with self.subTest(changes=changes):
                done = self.prepare(source, **changes)
                self.assertNotEqual(done.returncode, 0)
                self.assertEqual(list(self.cache.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
